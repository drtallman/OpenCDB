//! Stage 7 of [`super::validate`]: one stage per **declared** optional
//! requirements class.
//!
//! Each stage reads the content signal its class is recognized by — almost
//! always a §7.9.4.2 conditional element on a metadata record the
//! orchestrator already parsed — and delegates the judging to the free
//! `validate_*` function the requirements module itself owns, so the rules
//! stay next to the types they judge. A stage that finds no content records
//! none: a declared class with nothing to check passes.

use std::fs;

use crate::attribution::{self, AttributeModel, AttributionError};
use crate::conformance::{ConformanceReport, RequirementsClass};
use crate::coverage;
use crate::crs::StorageCrs;
use crate::datastore::CdbDatastore;
use crate::error::CdbError;
use crate::geometry;
use crate::metadata::{GlobalMetadata, ResourceMetadata};
use crate::tiling::{self, TilingScheme};
use crate::topology::{self, TopoGraph};
use crate::versioning::{self, VersioningError, VersioningViolation};

/// Attribution stage (§7.1.2, Requirements Attr1-B/C and Attr2): the
/// attribute model stored at `global_metadata/vector_attributes.<ext>` — the
/// Attr1-B location and the Attr1-C name, derived from the datastore's
/// declared metadata encoding — parses and validates.
///
/// No file means no attribute model, which is **not** a violation: Attr1-A
/// binds a profile "specifying and/or implementing attribution", and a
/// datastore that ships none simply has no attribution content. A
/// GeoPackage-declared datastore likewise has no core-readable model
/// ([`crate::attribution::file_name_for`] yields `None`) and counts as no
/// content. A file that *is* present but unreadable as a model is a SHALL
/// finding, not an I/O error — only the read itself can fail operationally.
///
/// Returns the model when one is present and valid, so the content sweep can
/// cross-check it against the profile's own [`ApplicationProfile::attribute_model`]
/// without reading the file a second time. This stage probes only the
/// canonical Attr1-C name; a *mis-named* model is the sweep's business.
pub(super) fn validate_attribution(
    datastore: &CdbDatastore,
    global: Option<&GlobalMetadata>,
    report: &mut ConformanceReport,
) -> Result<Option<AttributeModel>, CdbError> {
    // Without a readable global record the declared encoding is unknown —
    // that finding is already filed under Metadata; nothing to check here.
    let Some(global) = global else {
        return Ok(None);
    };
    let Some(name) = attribution::file_name_for(global.encoding) else {
        return Ok(None);
    };
    let path = datastore.layout().global_metadata_dir().join(&name);
    if !path.is_file() {
        return Ok(None);
    }
    report.mark_content(RequirementsClass::Attribution);
    let content = fs::read_to_string(&path).map_err(AttributionError::Io)?;
    match attribution::validate_attribute_model_document(&name, &content) {
        Ok(model) => Ok(Some(model)),
        Err(violation) => {
            report.record_violation(violation.into());
            Ok(None)
        }
    }
}

/// Coverages stage (§7.2.4–.6, Requirements Coverages4/5/6): every resource
/// metadata record carrying the Coverages6 `domainSet` conditional element is
/// a coverage instance, and is validated as one by
/// [`crate::coverage::validate_coverage_instance`] — Coverages5's
/// minimum-metadata duty and Coverages6's domainSet content, with Coverages4
/// judged against the datastore's own CRS.
///
/// `source_crs` is passed as `None`: a per-instance CRS claim lives in the
/// coverage payload, which this crate does not decode, so every record here
/// is association-via-datastore (the normal case, which Coverages4 permits).
/// The §7.2.6.1 SHOULD on `uom` is surfaced as a warning, never a failure.
pub(super) fn validate_coverages(
    records: &[ResourceMetadata],
    storage_crs: Option<&StorageCrs>,
    report: &mut ConformanceReport,
) {
    let datastore_crs = storage_crs.and_then(StorageCrs::authority);
    for record in records {
        let Some(domain_set) = record.domain_set.as_ref() else {
            continue;
        };
        report.mark_content(RequirementsClass::Coverages);
        if let Err(violation) =
            coverage::validate_coverage_instance(Some(record), None, datastore_crs.as_ref())
        {
            report.record_violation(violation.into());
        }
        for warning in domain_set.warnings() {
            report.record_warning(warning.into());
        }
    }
}

/// Geometry stage (§7.6.3, Requirement Geom4): the `uom` conditional element
/// on geometry-bearing resource metadata records, validated by
/// [`crate::geometry::validate_geometry_metadata`].
///
/// **This stage is narrow, and a reader must not over-read it.** Geometry is
/// the one requirements class with no on-disk artifact of its own:
/// Requirements Geom1–Geom6 govern geometry *instances*, which a datastore
/// keeps inside payloads (GeoPackage containers, raster files) that this
/// crate deliberately does not decode. So "Geometry: conformant" in a report
/// means only that the Geom4 declarations the records carry are well formed —
/// **not** that any geometry was inspected. Instance-level validation is an
/// API-boundary duty, performed by the caller holding a decoded geometry via
/// [`crate::geometry::CdbGeometry::validate_in`] against a
/// [`crate::geometry::GeometryContext`]. Requirement Geom3's z unit is
/// satisfied datastore-wide by `GlobalMetadata::uom`, which Metadata8 makes
/// mandatory.
///
/// **This stage cannot produce a finding, and the report says so.** Every
/// record reaching it was parsed by `ResourceMetadata::from_{json,xml}_str`,
/// which validates before returning, and stage 6 drops the ones that failed —
/// so [`crate::geometry::validate_geometry_metadata`] re-checks a record
/// already known valid. Content here is therefore marked
/// [`crate::conformance::ContentCoverage::Unchecked`], not `Checked`: a machine consumer reading
/// `"passed": true` for Geometry is told "we did not look", which is true,
/// rather than "we looked and it was fine", which would not be. The call is
/// kept rather than deleted because it is the correct entry point the day a
/// profile supplies decoded geometry.
pub(super) fn validate_geometry(records: &[ResourceMetadata], report: &mut ConformanceReport) {
    for record in records {
        if record.uom.is_none() {
            continue;
        }
        report.mark_unchecked_content(RequirementsClass::Geometry);
        if let Err(violation) = geometry::validate_geometry_metadata(record) {
            report.record_violation(violation.into());
        }
    }
}

/// Tiling stage (§7.10.2, Requirements Tiling5/6/7 and Tiling9/10 plus
/// Recommendation Tiling1): the `tilingScheme` element on the global record —
/// Requirement Tiling8's §7.9.4.2 conditional element — is what makes a
/// datastore *tiled*, and it is this class's content signal.
///
/// With a scheme present, [`crate::tiling::TilingScheme::validate`] judges it
/// against the storage CRS (Tiling5 CRS identity, Tiling6 unit of measure,
/// Tiling7 whole-earth extent), and `warnings()` surfaces Recommendation
/// Tiling1. Each resource metadata record of a tiled datastore is tileset
/// metadata, so [`crate::tiling::validate_tileset_metadata`] holds Tiling9/10
/// over it.
///
/// **That last step is an interpretation, recorded here as one.** The spec
/// gives no per-record tileset signal: §7.9.4.2 defines no tileset
/// conditional element, [`crate::metadata::ResourceType`] has the single
/// `Dataset` variant, and neither grid extension parses a tile address out of
/// a path. So this crate reads *every* resource metadata record of a
/// tiling-declaring datastore as tileset metadata, which makes Tiling10's
/// `keywords` effectively mandatory on all of them. The alternative — dropping
/// Tiling9/10 for want of a signal — silently discards a SHALL, which is the
/// worse failure. The cost lands on a datastore that mixes tiled and untiled
/// resources under one tiling scheme: its untiled records draw findings they
/// arguably should not. `docs/CONFORMANCE.md` carries this on its errata list.
///
/// A datastore with no `tilingScheme` is untiled: no content, and
/// deliberately **no** Tiling8 finding — Requirement Tiling8 binds a *tiled*
/// datastore, and a declared class with no content passes.
pub(super) fn validate_tiling(
    global: Option<&GlobalMetadata>,
    storage_crs: Option<&StorageCrs>,
    records: &[ResourceMetadata],
    report: &mut ConformanceReport,
) {
    let Some(global) = global else {
        return;
    };
    // Tiling8's own accessor: `Err` here means "untiled", not "faulty".
    let Ok(scheme) = TilingScheme::require(global) else {
        return;
    };
    report.mark_content(RequirementsClass::Tiling);
    // Without a readable storage CRS, Tiling5/6/7 have no yardstick; that
    // finding is already filed under Crs.
    if let Some(crs) = storage_crs
        && let Err(violation) = scheme.validate(crs)
    {
        report.record_violation(violation.into());
    }
    for warning in scheme.warnings() {
        report.record_warning(warning.into());
    }
    for record in records {
        if let Err(violation) = tiling::validate_tileset_metadata(record) {
            report.record_violation(violation.into());
        }
    }
}

/// Topology stage (§7.13.5.5, Face Topology Requirement 4): every resource
/// metadata record carrying the `windingOrder` conditional element declares a
/// topologically structured dataset, validated through the topology module's
/// own entry point, [`crate::topology::validate_topology_dataset`].
///
/// The graph passed is empty, and that is not a shortcut but the honest
/// boundary: a topological graph lives in the dataset payload, which this
/// crate does not decode, so the face-count arm of Face4 ("once the graph
/// contains faces, the record SHALL declare a `windingOrder`") cannot fire
/// from the datastore level. It stays an API-boundary duty for a caller
/// holding a decoded [`crate::topology::TopoGraph`]. What *is* checkable here
/// is the converse and it holds by construction: a record that declares a
/// winding order declares a valid one, since `WindingOrder` is a closed enum
/// the parse already rejected bad values for.
///
/// Holding by construction means **this stage cannot produce a finding**, and
/// the report says so: content here is marked [`crate::conformance::ContentCoverage::Unchecked`]
/// rather than `Checked`, so `"passed": true` for Topology reads as "we did
/// not look" — the truth — instead of "we looked and it was fine". Geometry's
/// stage carries the same marking for the same reason.
pub(super) fn validate_topology(records: &[ResourceMetadata], report: &mut ConformanceReport) {
    let empty = TopoGraph::new();
    for record in records {
        if record.winding_order.is_none() {
            continue;
        }
        report.mark_unchecked_content(RequirementsClass::Topology);
        if let Err(violation) = topology::validate_topology_dataset(&empty, record) {
            report.record_violation(violation.into());
        }
    }
}

/// Versioning stage (§7.14.2–.3, Requirements V1/V2): journal integrity —
/// every `versions/v######/` manifest parses, and the sequence they form is
/// contiguous from 1 ([`crate::versioning::validate_journal`], which
/// [`CdbDatastore::versions`] applies on the reader's behalf).
///
/// The `versions/` directory is the content signal: a datastore that has
/// applied no collection has no journal, which is no content rather than a
/// fault. A manifest that fails to parse, or a hole in the sequence, is a
/// SHALL finding; an unreadable directory is operational and returns `Err`.
///
/// `global` is required because the journal is written in the datastore's
/// declared metadata encoding: without a readable global record the manifest
/// file names are unknown, and that finding already sits under Metadata.
pub(super) fn validate_versioning(
    datastore: &CdbDatastore,
    global: Option<&GlobalMetadata>,
    report: &mut ConformanceReport,
) -> Result<(), CdbError> {
    if global.is_none() || !datastore.root().join(versioning::VERSIONS_DIR).is_dir() {
        return Ok(());
    }
    report.mark_content(RequirementsClass::Versioning);
    match datastore.versions() {
        Ok(_) => Ok(()),
        Err(CdbError::Versioning(VersioningError::Violation(violation))) => {
            report.record_violation(violation.into());
            Ok(())
        }
        Err(CdbError::Versioning(VersioningError::Serialization(reason))) => {
            report.record_violation(VersioningViolation::MalformedManifest { reason }.into());
            Ok(())
        }
        Err(other) => Err(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::support::{DeclaringProfile, fresh_store};
    use crate::attribution::{AttributeDef, AttributionViolation};
    use crate::conformance::{CdbViolation, CdbWarning, ContentCoverage};
    use crate::coverage::{CoverageViolation, CoverageWarning, DomainSet};
    use crate::metadata::UnitOfMeasure;
    use crate::tiling::{TilingViolation, TilingWarning};
    use crate::topology::WindingOrder;
    use crate::versioning::PendingCollection;
    use tempfile::tempdir;

    /// Design spec §4 — **a declared class with no corresponding content
    /// passes.** A profile declaring all eleven classes against a freshly
    /// created datastore (no attribute model, no tiling scheme, no resource
    /// records, no journal) is conformant: every optional class is listed and
    /// passing, and every one reports *no content*, so the report cannot be
    /// mistaken for a clean check of content that was never there. The
    /// mandatory five are content-bearing by construction.
    #[test]
    fn conf_core_declared_optional_class_with_no_content_passes() {
        let tmp = tempdir().unwrap();
        let store = fresh_store(&tmp);
        let profile = DeclaringProfile::all();

        let report = store.validate(&profile).unwrap();
        assert!(report.is_conformant(), "{report}");
        let listed: Vec<RequirementsClass> = report.classes().map(|(class, _)| class).collect();
        for &class in RequirementsClass::OPTIONAL {
            assert!(listed.contains(&class), "{class} not listed: {report}");
            assert!(report.class_passed(class), "{class}: {report}");
            assert!(
                !report.class_has_content(class),
                "{class} should have no content: {report}"
            );
        }
        for &class in RequirementsClass::MANDATORY {
            assert!(report.class_has_content(class), "{class}: {report}");
        }
        assert!(report.to_string().contains("(no content)"), "{report}");
    }

    /// Requirements Attr1-B/C and Attr2 (§7.1.2) — the Attribution stage
    /// reads `global_metadata/vector_attributes.<declared-enc>` and validates
    /// the model it holds. A written model gives the class content and
    /// passes; bytes that are not a parseable model fail the class as
    /// `AttributionViolation::Malformed` (a SHALL finding — the read
    /// succeeded).
    #[test]
    fn req_core_attribution_stage_validates_model_document() {
        let tmp = tempdir().unwrap();
        let store = fresh_store(&tmp);
        let profile = DeclaringProfile::all();

        store
            .write_attribute_model(&AttributeModel {
                schema_uri: None,
                attributes: vec![AttributeDef {
                    id: "1".to_owned(),
                    name: "StreetName".to_owned(),
                    description: "Name of a street as an alphanumeric string".to_owned(),
                }],
            })
            .unwrap();

        let report = store.validate(&profile).unwrap();
        assert!(report.is_conformant(), "{report}");
        assert!(
            report.class_has_content(RequirementsClass::Attribution),
            "{report}"
        );

        // Corrupt the document: the class fails, nothing else does.
        fs::write(
            store
                .layout()
                .global_metadata_dir()
                .join("vector_attributes.json"),
            "{not json",
        )
        .unwrap();
        let report = store.validate(&profile).unwrap();
        assert!(
            !report.class_passed(RequirementsClass::Attribution),
            "{report}"
        );
        assert!(matches!(
            report.violations(RequirementsClass::Attribution),
            [CdbViolation::Attribution(
                AttributionViolation::Malformed { .. }
            )]
        ));
        assert!(report.class_passed(RequirementsClass::Metadata), "{report}");
    }

    /// Requirements Coverages4/5/6 (§7.2.4–.6) — the Coverages stage treats
    /// every resource metadata record carrying the Coverages6 `domainSet`
    /// conditional element as a coverage instance (the §7.9.4.2 mechanism)
    /// and validates it. A well-formed domainSet passes and marks content; a
    /// URI-shaped `uom` earns the §7.2.6.1 SHOULD warning without failing;
    /// an empty `uom` fails the class with `EmptyUom`.
    #[test]
    fn req_core_coverages_stage_validates_domain_set_records() {
        let tmp = tempdir().unwrap();
        let store = fresh_store(&tmp);
        let profile = DeclaringProfile::all();

        let mut record = ResourceMetadata::new("Elevation", "Terrain Elevation", "Gridded DEM");
        record.domain_set = Some(DomainSet::new("m"));
        store
            .write_resource_metadata("/Tiles/metadata/Elevation.json", &record)
            .unwrap();

        let report = store.validate(&profile).unwrap();
        assert!(report.is_conformant(), "{report}");
        assert!(
            report.class_has_content(RequirementsClass::Coverages),
            "{report}"
        );
        assert!(report.warnings(RequirementsClass::Coverages).is_empty());

        // Recommendation §7.2.6.1: a URN-shaped uom warns, never fails.
        record.domain_set = Some(DomainSet::new("urn:ogc:def:uom:EPSG::9001"));
        store
            .write_resource_metadata("/Tiles/metadata/Elevation.json", &record)
            .unwrap();
        let report = store.validate(&profile).unwrap();
        assert!(report.is_conformant(), "{report}");
        assert!(
            report.class_passed(RequirementsClass::Coverages),
            "{report}"
        );
        assert!(matches!(
            report.warnings(RequirementsClass::Coverages),
            [CdbWarning::Coverage(
                CoverageWarning::UomLooksLikeUri { .. }
            )]
        ));

        // Coverages6-A: the one mandatory domainSet element cannot be empty.
        record.domain_set = Some(DomainSet::new(""));
        store
            .write_resource_metadata("/Tiles/metadata/Elevation.json", &record)
            .unwrap();
        let report = store.validate(&profile).unwrap();
        assert!(
            !report.class_passed(RequirementsClass::Coverages),
            "{report}"
        );
        assert!(matches!(
            report.violations(RequirementsClass::Coverages),
            [CdbViolation::Coverage(CoverageViolation::EmptyUom)]
        ));
    }

    /// Requirements Tiling5/6/7, Tiling9/10 and Recommendation Tiling1
    /// (§7.10) — the Tiling stage fires on the `tilingScheme` element the
    /// global record carries (Requirement Tiling8's §7.9.4.2 conditional
    /// element): it validates the scheme against the storage CRS, surfaces
    /// Rec Tiling1 as a warning, and holds Tiling9/10 over each tileset
    /// metadata record. A datastore with no scheme is untiled — no content,
    /// and no Tiling8 finding, because §4 makes an undeclared-content class
    /// pass.
    #[test]
    fn req_core_tiling_stage_validates_scheme_and_tileset_metadata() {
        let tmp = tempdir().unwrap();
        let store = fresh_store(&tmp);
        let profile = DeclaringProfile::all();

        // Untiled: declared, no content, no finding.
        let report = store.validate(&profile).unwrap();
        assert!(report.class_passed(RequirementsClass::Tiling), "{report}");
        assert!(!report.class_has_content(RequirementsClass::Tiling));

        // Tiled, with a well-formed tileset record (Tiling10: Keywords).
        let mut global = store.global_metadata().unwrap();
        global.tiling_scheme = Some(TilingScheme::cdb1_global_grid());
        global.write_to(store.layout()).unwrap();
        let mut record = ResourceMetadata::new("Roads", "Road Network", "Tiled roads");
        record.keywords = vec!["transportation".to_owned()];
        store
            .write_resource_metadata("/Tiles/metadata/Roads.json", &record)
            .unwrap();

        let report = store.validate(&profile).unwrap();
        assert!(report.is_conformant(), "{report}");
        assert!(
            report.class_has_content(RequirementsClass::Tiling),
            "{report}"
        );
        assert!(report.warnings(RequirementsClass::Tiling).is_empty());

        // Tiling10: a tileset record without Keywords fails the class.
        record.keywords.clear();
        store
            .write_resource_metadata("/Tiles/metadata/Roads.json", &record)
            .unwrap();
        let report = store.validate(&profile).unwrap();
        assert!(matches!(
            report.violations(RequirementsClass::Tiling),
            [CdbViolation::Tiling(
                TilingViolation::MissingTilesetKeywords
            )]
        ));

        // Recommendation Tiling1: a non-extension scheme warns, never fails.
        let mut custom = TilingScheme::cdb1_global_grid();
        custom.id = "MyOwnGrid".to_owned();
        let mut global = store.global_metadata().unwrap();
        global.tiling_scheme = Some(custom);
        global.write_to(store.layout()).unwrap();
        record.keywords = vec!["transportation".to_owned()];
        store
            .write_resource_metadata("/Tiles/metadata/Roads.json", &record)
            .unwrap();
        let report = store.validate(&profile).unwrap();
        assert!(report.is_conformant(), "{report}");
        assert!(matches!(
            report.warnings(RequirementsClass::Tiling),
            [CdbWarning::Tiling(TilingWarning::NonExtensionScheme { .. })]
        ));
    }

    /// Face Topology Requirement 4 (/req/core/topology-winding, §7.13.5.5) —
    /// the Topology stage fires on the `windingOrder` conditional element a
    /// resource metadata record carries and validates the record through
    /// [`crate::topology::validate_topology_dataset`]. The topological graph
    /// itself lives in an opaque payload, so the face-bearing arm of Face4 is
    /// an API-boundary duty; see the stage's doc comment.
    #[test]
    fn req_core_topology_stage_validates_winding_order_records() {
        let tmp = tempdir().unwrap();
        let store = fresh_store(&tmp);
        let profile = DeclaringProfile::all();

        let mut record = ResourceMetadata::new("Roads", "Road Topology", "Topological roads");
        record.winding_order = Some(WindingOrder::Counterclockwise);
        store
            .write_resource_metadata("/Tiles/metadata/Roads.json", &record)
            .unwrap();

        let report = store.validate(&profile).unwrap();
        assert!(report.is_conformant(), "{report}");
        assert!(
            report.class_has_content(RequirementsClass::Topology),
            "{report}"
        );
        // ... but the content went *unjudged*: with the graph in an opaque
        // payload the stage reduces to a re-validation of an already-valid
        // record, so the report must not claim Topology was checked.
        assert_eq!(
            report.class_coverage(RequirementsClass::Topology),
            ContentCoverage::Unchecked,
            "{report}"
        );
        assert!(
            report
                .to_string()
                .contains("[PASS] topology (content not checked)"),
            "{report}"
        );
        // §7.13 has no SHOULD, so the class can never carry a warning.
        assert!(report.warnings(RequirementsClass::Topology).is_empty());
    }

    /// Requirement Geom4 /req/core/geometry-mvalue (§7.6.3) — the Geometry
    /// stage fires on the `uom` conditional element a resource metadata
    /// record carries, and checks only that. Geometry *instances* live in
    /// payloads this crate does not decode, so a passing Geometry class means
    /// the records' Geom4 declarations are well formed and nothing more.
    #[test]
    fn req_core_geometry_stage_checks_geom4_uom_element_only() {
        let tmp = tempdir().unwrap();
        let store = fresh_store(&tmp);
        let profile = DeclaringProfile::all();

        // A record with no `uom` gives the class no content.
        let mut record = ResourceMetadata::new("Roads", "Road Network", "Vector roads");
        store
            .write_resource_metadata("/Tiles/metadata/Roads.json", &record)
            .unwrap();
        let report = store.validate(&profile).unwrap();
        assert!(
            !report.class_has_content(RequirementsClass::Geometry),
            "{report}"
        );

        // Declaring the Geom4 element makes the record geometry-bearing.
        record.uom = Some(UnitOfMeasure::Meters);
        store
            .write_resource_metadata("/Tiles/metadata/Roads.json", &record)
            .unwrap();
        let report = store.validate(&profile).unwrap();
        assert!(report.is_conformant(), "{report}");
        assert!(
            report.class_has_content(RequirementsClass::Geometry),
            "{report}"
        );
        // ... and the report says, in its own vocabulary rather than only in
        // prose, that nothing was judged: the geometry instances Geom1–Geom6
        // govern live in payloads this crate does not decode.
        assert_eq!(
            report.class_coverage(RequirementsClass::Geometry),
            ContentCoverage::Unchecked,
            "{report}"
        );
        assert!(
            report
                .to_string()
                .contains("[PASS] geometry (content not checked)"),
            "{report}"
        );

        // Every other content-bearing class is genuinely checked, so the
        // third state marks the two weak stages and nothing else.
        assert_eq!(
            report.class_coverage(RequirementsClass::Metadata),
            ContentCoverage::Checked,
            "{report}"
        );
    }

    /// Requirements V1/V2 (§7.14.2–.3) — the Versioning stage checks journal
    /// integrity: the `versions/` manifests parse and their sequence is
    /// contiguous from 1. Applying collections gives the class content; a
    /// corrupt manifest fails it with `MalformedManifest` and a hole punched
    /// in the sequence with `ManifestSequenceGap`. Both are SHALL findings —
    /// the files were read, they are simply not a coherent journal.
    #[test]
    fn req_core_versioning_stage_checks_journal_integrity() {
        let tmp = tempdir().unwrap();
        let store = fresh_store(&tmp);
        let profile = DeclaringProfile::all();

        // A corrupt manifest is a conformance finding, not an I/O error.
        let corrupt_tmp = tempdir().unwrap();
        let corrupt = fresh_store(&corrupt_tmp);
        corrupt
            .apply_collection(PendingCollection::new().create("/Tiles/A.gpkg", *b"a1"))
            .unwrap();
        fs::write(
            corrupt
                .root()
                .join("versions")
                .join("v000001")
                .join("manifest.json"),
            "{not json",
        )
        .unwrap();
        let report = corrupt.validate(&profile).unwrap();
        assert!(matches!(
            report.violations(RequirementsClass::Versioning),
            [CdbViolation::Versioning(
                VersioningViolation::MalformedManifest { .. }
            )]
        ));

        store
            .apply_collection(PendingCollection::new().create("/Tiles/A.gpkg", *b"a1"))
            .unwrap();
        store
            .apply_collection(PendingCollection::new().create("/Tiles/B.gpkg", *b"b1"))
            .unwrap();

        let report = store.validate(&profile).unwrap();
        assert!(report.is_conformant(), "{report}");
        assert!(
            report.class_has_content(RequirementsClass::Versioning),
            "{report}"
        );

        // Punch a hole in the journal: sequence 2 with no sequence 1.
        fs::remove_dir_all(store.root().join("versions").join("v000001")).unwrap();
        let report = store.validate(&profile).unwrap();
        assert!(
            !report.class_passed(RequirementsClass::Versioning),
            "{report}"
        );
        assert!(matches!(
            report.violations(RequirementsClass::Versioning),
            [CdbViolation::Versioning(
                VersioningViolation::ManifestSequenceGap {
                    expected: 1,
                    found: 2
                }
            )]
        ));
    }
}
