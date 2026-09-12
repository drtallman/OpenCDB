//! The conformance orchestrator: [`validate`], the abstract test behind
//! Annex A `/conf/minimal-core`, and the single directory walk its stages
//! share.

use std::fs;
use std::io;
use std::path::Path;

use crate::attribution::{self, AttributionError};
use crate::conformance::{CdbViolation, CdbWarning, ConformanceReport, RequirementsClass};
use crate::coverage;
use crate::crs::{CrsError, StorageCrs};
use crate::datastore::{CdbDatastore, path_extension};
use crate::error::CdbError;
use crate::geometry;
use crate::hierarchy::HierarchyError;
use crate::metadata::{
    GlobalMetadata, MetadataError, MetadataViolation, ResourceMetadata, encoding_violations,
};
use crate::naming::{NamingWarning, StyleGuide, component_warnings, file_warnings};
use crate::profiles::ApplicationProfile;
use crate::tiling::{self, TilingScheme};
use crate::topology::{self, TopoGraph};
use crate::versioning::{self, VersioningError, VersioningViolation};

/// Validates `datastore` against `profile`, the abstract test behind Annex A
/// `/conf/minimal-core`: it inspects the profile's declarations and the
/// on-disk state of the requirements classes and returns a
/// [`ConformanceReport`] bucketing every finding by [`RequirementsClass`].
///
/// The five mandatory classes (CRS, File Naming, File Structure, Links,
/// Metadata) are always checked. Each **optional** class the profile declares
/// in [`ApplicationProfile::conformance_classes`] then gets a stage of its
/// own, delegating to the free `validate_*` function its requirements module
/// owns. A declared class whose content the datastore does not hold *passes*
/// — declaring Tiling in a datastore holding no tiles is not a violation —
/// but the report records that nothing was checked, via
/// [`crate::conformance::ClassFindings::has_content`], so a pass on an empty
/// datastore cannot be misread as a pass on a clean one.
///
/// Conformance findings are **recorded, never returned as `Err`**: a
/// non-conformant datastore still yields `Ok(report)` whose
/// [`ConformanceReport::is_conformant`] is `false`. `Err` is reserved for
/// *operational* I/O failures — an unreadable directory or file — that
/// prevent inspection from completing at all. Warnings (SHOULD findings)
/// are collected but never affect pass/fail.
///
/// The datastore stores no profile, so the same datastore can be validated
/// against any profile; the profile is purely the yardstick.
pub fn validate(
    datastore: &CdbDatastore,
    profile: &dyn ApplicationProfile,
) -> Result<ConformanceReport, CdbError> {
    let mut report = ConformanceReport::new(profile.name(), datastore.root());

    // Stage 1 — /conf/minimal-core: every mandatory class must be declared.
    let declared = profile.conformance_classes();
    for class in RequirementsClass::MANDATORY {
        if !declared.contains(&class) {
            report.record_violation(CdbViolation::MissingConformanceDeclaration {
                profile: profile.name().to_owned(),
                class,
            });
        }
    }

    // Stage 2 — File Structure: fold the hierarchy report wholesale.
    // Its `Err` is operational (I/O or a malformed root name) — propagate.
    let hierarchy = datastore.layout().validate()?;
    for violation in hierarchy.violations {
        report.record_violation(violation.into());
    }
    for warning in hierarchy.warnings {
        report.record_warning(warning.into());
    }

    // Stage 3 — File Naming: walk the tree once, recording per-name
    // findings and collecting the file inventory the later stages reuse.
    let guide = profile.style_guide();
    let known_extensions = profile.known_extensions();
    let global_metadata_dir = datastore.layout().global_metadata_dir();
    let (global_metadata_files, file_logical_paths) = {
        let mut walk = NamingWalk {
            global_metadata_dir: &global_metadata_dir,
            guide: &guide,
            known_extensions: &known_extensions,
            report: &mut report,
            global_metadata_files: Vec::new(),
            file_logical_paths: Vec::new(),
        };
        walk.walk(datastore.root(), "")
            .map_err(HierarchyError::Io)?;
        (walk.global_metadata_files, walk.file_logical_paths)
    };

    // Stage 4 — Metadata: read the global record and cross-check the
    // profile's Metadata2/5/8/4 declarations, then sweep every metadata
    // file's encoding (Metadata5). The parsed record is kept: the declared
    // optional classes read their §7.9.4.2 conditional elements off it.
    let mut global_record = None;
    match GlobalMetadata::read_from(datastore.layout()) {
        Ok(global) => {
            if global.encoding != profile.metadata_encoding() {
                report.record_violation(CdbViolation::DeclarationMismatch {
                    profile: profile.name().to_owned(),
                    class: RequirementsClass::Metadata,
                    element: "metadata encoding",
                    declared: profile.metadata_encoding().to_string(),
                    found: global.encoding.to_string(),
                    clause: "/req/core/metadata-encoding",
                });
            }
            if global.metadata_standard != profile.metadata_standard() {
                report.record_violation(CdbViolation::DeclarationMismatch {
                    profile: profile.name().to_owned(),
                    class: RequirementsClass::Metadata,
                    element: "metadata standard",
                    declared: profile.metadata_standard().to_string(),
                    found: global.metadata_standard.to_string(),
                    clause: "/req/core/metadata-standard",
                });
            }
            if global.uom != profile.uom() {
                report.record_violation(CdbViolation::DeclarationMismatch {
                    profile: profile.name().to_owned(),
                    class: RequirementsClass::Metadata,
                    element: "uom",
                    declared: profile.uom().to_string(),
                    found: global.uom.to_string(),
                    clause: "/req/core/metadata-uom-measure",
                });
            }
            match profile.language() {
                Ok(declared_language) => {
                    if global.language != declared_language {
                        report.record_violation(CdbViolation::DeclarationMismatch {
                            profile: profile.name().to_owned(),
                            class: RequirementsClass::Metadata,
                            element: "language",
                            declared: declared_language.to_string(),
                            found: global.language.to_string(),
                            clause: "/req/core/metadata-language",
                        });
                    }
                }
                // A profile whose own style-guide language is malformed is a
                // Metadata-class finding, not an operational error.
                Err(violation) => report.record_violation(violation.into()),
            }
            // Recommendation Name3-B: the datastore language SHOULD be
            // English; compare the primary subtag of the on-disk language.
            let primary = global
                .language
                .as_str()
                .split('-')
                .next()
                .unwrap_or_default();
            if !primary.eq_ignore_ascii_case("en") {
                report.record_warning(CdbWarning::LanguageNotEnglish {
                    language: global.language.as_str().to_owned(),
                });
            }
            global_record = Some(global);
        }
        Err(error) => record_metadata_error(&mut report, error)?,
    }

    // Metadata5 sweep: every metadata file must use the declared encoding.
    // Scope = files at the top of `global_metadata/` plus files the profile
    // recognizes as resource metadata — a `.gpkg` *data* file is legitimate
    // and must not be swept.
    let mut sweep_names = global_metadata_files;
    for logical_path in &file_logical_paths {
        if profile.is_resource_metadata(logical_path) {
            sweep_names.push(logical_path.clone());
        }
    }
    for violation in encoding_violations(
        profile.metadata_encoding(),
        sweep_names.iter().map(String::as_str),
    ) {
        report.record_violation(violation.into());
    }

    // Stage 5 — CRS: read the storage CRS and cross-check the profile's
    // single-CRS declaration (CRS3). The parsed CRS is kept: Tiling5/6/7 and
    // Coverages4 are judged against it.
    let mut storage_crs = None;
    match StorageCrs::read_from(datastore.layout()) {
        Ok(on_disk) => {
            match profile.storage_crs() {
                Ok(declared) => {
                    if on_disk != declared {
                        report.record_violation(CdbViolation::DeclarationMismatch {
                            profile: profile.name().to_owned(),
                            class: RequirementsClass::Crs,
                            element: "storage CRS",
                            declared: declared.to_wkt(),
                            found: on_disk.to_wkt(),
                            clause: "/req/core/crs/crsStorage",
                        });
                    }
                }
                // A mis-declared profile CRS is a Crs-class finding.
                Err(violation) => report.record_violation(violation.into()),
            }
            for warning in on_disk.warnings() {
                report.record_warning(warning.into());
            }
            storage_crs = Some(on_disk);
        }
        Err(CrsError::Violation(violation)) => report.record_violation(violation.into()),
        // I/O (and any future operational variant) aborts inspection.
        Err(other) => return Err(other.into()),
    }

    // Stage 6 — Resource metadata + Links: parse each recognized record and
    // record its Metadata/Links findings. Records that parse are kept for
    // stage 7: the conditional elements of §7.9.4.2 (`domainSet`,
    // `windingOrder`, `uom`) are how the optional classes see their content.
    // A record that did NOT parse is absent here by design — its finding is
    // already filed under Metadata, and re-reporting it under an optional
    // class would double-count one fault.
    let mut resource_records: Vec<ResourceMetadata> = Vec::new();
    for logical_path in &file_logical_paths {
        if !profile.is_resource_metadata(logical_path) {
            continue;
        }
        let extension = path_extension(logical_path).map(str::to_ascii_lowercase);
        if extension.as_deref() == Some("gpkg") {
            // The core cannot parse the GeoPackage container; its encoding
            // mismatch (if any) was already recorded by the Metadata5 sweep.
            continue;
        }
        let physical = match datastore.resolve(logical_path) {
            Ok(path) => path,
            // A resolve failure is a naming violation the File Naming walk
            // already recorded; skip rather than abort validation.
            Err(_) => continue,
        };
        let content = fs::read_to_string(&physical).map_err(MetadataError::from)?;
        // `from_*_str` deserialize *and* validate, so a bad link or missing
        // element surfaces here as `Err(Violation(..))`.
        let parsed = match extension.as_deref() {
            Some("xml") => ResourceMetadata::from_xml_str(&content),
            _ => ResourceMetadata::from_json_str(&content),
        };
        match parsed {
            Ok(record) => resource_records.push(record),
            Err(error) => record_metadata_error(&mut report, error)?,
        }
    }

    // Stage 7 — one stage per DECLARED optional class. Declaration drives:
    // Annex A judges a datastore against a profile's declaration, so a class
    // the profile never declared is not checked here (content that exists
    // anyway is the content sweep's business, not this stage's).
    //
    // Every declared class is listed even when its stage finds nothing: a
    // declared class with no corresponding content PASSES — the class says
    // what the profile supports, and nothing in Annex A requires the content
    // to exist — but the report separates that from a clean check via
    // `ClassFindings::has_content`.
    for class in RequirementsClass::OPTIONAL {
        if !declared.contains(&class) {
            continue;
        }
        report.declare_class(class);
        match class {
            RequirementsClass::Attribution => {
                validate_attribution(datastore, global_record.as_ref(), &mut report)?;
            }
            RequirementsClass::Coverages => {
                validate_coverages(&resource_records, storage_crs.as_ref(), &mut report);
            }
            RequirementsClass::Geometry => {
                validate_geometry(&resource_records, &mut report);
            }
            RequirementsClass::Tiling => {
                validate_tiling(
                    global_record.as_ref(),
                    storage_crs.as_ref(),
                    &resource_records,
                    &mut report,
                );
            }
            RequirementsClass::Topology => {
                validate_topology(&resource_records, &mut report);
            }
            RequirementsClass::Versioning => {
                validate_versioning(datastore, global_record.as_ref(), &mut report)?;
            }
            // The mandatory five are not in OPTIONAL; stages 1–6 ran them.
            // Spelled out rather than wildcarded so a new variant is a
            // compile error here, not a silent omission.
            RequirementsClass::Crs
            | RequirementsClass::FileNaming
            | RequirementsClass::FileStructure
            | RequirementsClass::Links
            | RequirementsClass::Metadata => {}
        }
    }

    Ok(report)
}

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
fn validate_attribution(
    datastore: &CdbDatastore,
    global: Option<&GlobalMetadata>,
    report: &mut ConformanceReport,
) -> Result<(), CdbError> {
    // Without a readable global record the declared encoding is unknown —
    // that finding is already filed under Metadata; nothing to check here.
    let Some(global) = global else {
        return Ok(());
    };
    let Some(name) = attribution::file_name_for(global.encoding) else {
        return Ok(());
    };
    let path = datastore.layout().global_metadata_dir().join(&name);
    if !path.is_file() {
        return Ok(());
    }
    report.mark_content(RequirementsClass::Attribution);
    let content = fs::read_to_string(&path).map_err(AttributionError::Io)?;
    if let Err(violation) = attribution::validate_attribute_model_document(&name, &content) {
        report.record_violation(violation.into());
    }
    Ok(())
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
fn validate_coverages(
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
fn validate_geometry(records: &[ResourceMetadata], report: &mut ConformanceReport) {
    for record in records {
        if record.uom.is_none() {
            continue;
        }
        report.mark_content(RequirementsClass::Geometry);
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
/// A datastore with no `tilingScheme` is untiled: no content, and
/// deliberately **no** Tiling8 finding — Requirement Tiling8 binds a *tiled*
/// datastore, and a declared class with no content passes.
fn validate_tiling(
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
fn validate_topology(records: &[ResourceMetadata], report: &mut ConformanceReport) {
    let empty = TopoGraph::new();
    for record in records {
        if record.winding_order.is_none() {
            continue;
        }
        report.mark_content(RequirementsClass::Topology);
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
fn validate_versioning(
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

/// The File Naming walk of [`validate`] (stage 3): a single
/// recursive pass below the datastore root that records each name's Name1/Name6
/// findings and, in the same sweep, collects the file inventory stages 4–6
/// reuse. Symlinks are neither followed nor recursed (`DirEntry::file_type` is a
/// no-follow probe, mirroring [`crate::hierarchy`]'s empty-folder walk), so
/// external-resource links (Permission PFile1) are safe. Non-UTF-8 names are
/// lossily decoded via [`std::ffi::OsStr::to_string_lossy`] before validation.
struct NamingWalk<'a> {
    /// The physical `global_metadata/` directory, so its top-level files can be
    /// picked out for the Metadata5 sweep.
    global_metadata_dir: &'a Path,
    /// The profile's style guide (built once), applied to every name.
    guide: &'a StyleGuide,
    /// Extensions the profile vouches for (Name7-B); their `NonSpecExtension`
    /// warnings are suppressed.
    known_extensions: &'a [String],
    /// The report findings are recorded into.
    report: &'a mut ConformanceReport,
    /// File names at the top level of `global_metadata/`.
    global_metadata_files: Vec<String>,
    /// The logical path of every file found, `/`-joined from the root.
    file_logical_paths: Vec<String>,
}

impl NamingWalk<'_> {
    /// Walks `dir`, whose logical path is `dir_logical` (empty for the root, so
    /// its children read as `/name`).
    fn walk(&mut self, dir: &Path, dir_logical: &str) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();

            // Name1/Name6: structural rules and the datastore-wide case rule.
            if let Err(violation) = self.guide.validate_component(&name) {
                self.report.record_violation(violation.into());
            }

            let child_logical = format!("{dir_logical}/{name}");
            if file_type.is_file() {
                for warning in filter_known(file_warnings(&name), self.known_extensions) {
                    self.report.record_warning(warning.into());
                }
                if dir == self.global_metadata_dir {
                    self.global_metadata_files.push(name.to_string());
                }
                self.file_logical_paths.push(child_logical);
            } else {
                for warning in component_warnings(&name) {
                    self.report.record_warning(warning.into());
                }
                // Recurse into real subdirectories only — never a
                // symlink. The reserved `versions/` journal subtree is
                // versioning's own machinery (Requirement V1, §7.14.2):
                // its `v######` directories and `manifest.<enc>` files are
                // crate-persisted under every case rule, and its archive
                // mirrors were name-checked at their live locations —
                // [`CdbDatastore::versions`] validates the journal itself
                // (parse + contiguity), so the walk does not descend.
                if file_type.is_dir()
                    && !(dir_logical.is_empty() && name == versioning::VERSIONS_DIR)
                {
                    self.walk(&entry.path(), &child_logical)?;
                }
            }
        }
        Ok(())
    }
}

/// Drops the `NonSpecExtension` warnings whose extension the profile vouches
/// for as industry standard (Requirement Name7-B), comparing case-insensitively.
fn filter_known(warnings: Vec<NamingWarning>, known: &[String]) -> Vec<NamingWarning> {
    warnings
        .into_iter()
        .filter(|warning| match warning {
            NamingWarning::NonSpecExtension { extension, .. } => !known
                .iter()
                .any(|vouched| vouched.eq_ignore_ascii_case(extension)),
            _ => true,
        })
        .collect()
}

/// Records a [`MetadataError`] surfaced while reading a metadata record as a
/// conformance finding, or returns it as an operational `Err`. A
/// [`MetadataError::Violation`] is recorded verbatim (a nested
/// [`MetadataViolation::Link`] is re-filed under Links by
/// [`ConformanceReport::record_violation`]); a serialization failure becomes a
/// [`MetadataViolation::Malformed`]. [`MetadataError::UnsupportedEncoding`] is
/// unreachable from the reader (it only reads json/xml), but is armed
/// defensively as `Malformed` so a future caller cannot make it panic. I/O — and
/// any future operational variant — is not a conformance finding and returns
/// `Err`.
fn record_metadata_error(
    report: &mut ConformanceReport,
    error: MetadataError,
) -> Result<(), CdbError> {
    match error {
        MetadataError::Violation(violation) => {
            report.record_violation(violation.into());
            Ok(())
        }
        MetadataError::Serialization(reason) => {
            report.record_violation(MetadataViolation::Malformed { reason }.into());
            Ok(())
        }
        MetadataError::UnsupportedEncoding(encoding) => {
            report.record_violation(
                MetadataViolation::Malformed {
                    reason: format!(
                        "metadata declares the {encoding} encoding the core cannot read"
                    ),
                }
                .into(),
            );
            Ok(())
        }
        other => Err(other.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    use crate::attribution::{AttributeDef, AttributeModel, AttributionViolation};
    use crate::coverage::{CoverageViolation, CoverageWarning, DomainSet};
    use crate::crs::CrsViolation;
    use crate::datastore::DatastoreSeed;
    use crate::hierarchy::{DatastoreLayout, HierarchyWarning};
    use crate::metadata::{MetadataEncoding, MetadataStandard, UnitOfMeasure};
    use crate::naming::{CaseRule, StyleGuide};
    use crate::profiles::{SimulationProfile, StorageTechnology};
    use crate::tiling::{TilingScheme, TilingViolation, TilingWarning};
    use crate::topology::WindingOrder;
    use crate::versioning::{PendingCollection, VersioningViolation};
    use tempfile::tempdir;

    /// A profile that delegates every policy to the default simulation
    /// profile but declares an arbitrary set of requirements classes, so the
    /// declared-class stages can be driven. (`SimulationProfile` itself
    /// declares only the mandatory five, which is why every test below needs
    /// this wrapper.)
    struct DeclaringProfile {
        inner: SimulationProfile,
        classes: Vec<RequirementsClass>,
    }

    impl DeclaringProfile {
        /// Declares every class the core defines.
        fn all() -> Self {
            DeclaringProfile {
                inner: SimulationProfile::json(),
                classes: RequirementsClass::ALL.to_vec(),
            }
        }
    }

    impl ApplicationProfile for DeclaringProfile {
        fn name(&self) -> &str {
            self.inner.name()
        }
        fn style_guide(&self) -> StyleGuide {
            self.inner.style_guide()
        }
        fn storage_crs(&self) -> Result<StorageCrs, CrsViolation> {
            self.inner.storage_crs()
        }
        fn metadata_standard(&self) -> MetadataStandard {
            self.inner.metadata_standard()
        }
        fn metadata_encoding(&self) -> MetadataEncoding {
            self.inner.metadata_encoding()
        }
        fn uom(&self) -> UnitOfMeasure {
            self.inner.uom()
        }
        fn storage_technology(&self) -> StorageTechnology {
            self.inner.storage_technology()
        }
        fn conformance_classes(&self) -> Vec<RequirementsClass> {
            self.classes.clone()
        }
        fn is_resource_metadata(&self, logical_path: &str) -> bool {
            self.inner.is_resource_metadata(logical_path)
        }
        fn known_extensions(&self) -> Vec<String> {
            self.inner.known_extensions()
        }
    }

    /// Creates a fresh JSON datastore for the optional-class stage tests.
    fn fresh_store(tmp: &tempfile::TempDir) -> CdbDatastore {
        CdbDatastore::create(
            tmp.path(),
            &SimulationProfile::json(),
            DatastoreSeed::new("id", "Title", "Description", "contact"),
        )
        .unwrap()
    }

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
        for class in RequirementsClass::OPTIONAL {
            assert!(listed.contains(&class), "{class} not listed: {report}");
            assert!(report.class_passed(class), "{class}: {report}");
            assert!(
                !report.class_has_content(class),
                "{class} should have no content: {report}"
            );
        }
        for class in RequirementsClass::MANDATORY {
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

    /// Annex A `/conf/minimal-core` — all five mandatory classes: a datastore
    /// freshly created from `SimulationProfile::json()` validates clean —
    /// conformant, every mandatory class passes, and (crucially) every class
    /// has **zero** warnings. Zero warnings proves both naming wrinkles are in
    /// force: the `crs` name is auto-reserved (so `crs.wkt` escapes the case
    /// rule) and the profile vouches for the `.wkt` extension (Name7-B, so its
    /// `NonSpecExtension` warning is suppressed). A regression in either
    /// surfaces here as a spurious warning.
    #[test]
    fn validate_fresh_datastore_conformant_no_warnings() {
        let tmp = tempdir().unwrap();
        let profile = SimulationProfile::json();
        let store = CdbDatastore::create(
            tmp.path(),
            &profile,
            DatastoreSeed::new("id", "Title", "Description", "contact"),
        )
        .unwrap();

        let report = store.validate(&profile).unwrap();
        assert!(report.is_conformant(), "{report}");
        for class in RequirementsClass::MANDATORY {
            assert!(report.class_passed(class), "{class}: {report}");
            assert!(
                report.warnings(class).is_empty(),
                "{class} should have no warnings: {report}"
            );
        }
    }

    /// Requirement CRS3 (§7.3.1.2 `/req/core/crs/crsStorage`): when the
    /// on-disk storage CRS differs from the one the profile declares, the Crs
    /// class fails with a `DeclarationMismatch` for the storage CRS. Here the
    /// datastore's `crs.wkt` is overwritten with a valid but *foreign*
    /// geographic CRS (NAD83, EPSG:4269), which parses cleanly yet is not the
    /// profile's WGS-84.
    #[test]
    fn req_core_crs_storage_declaration_mismatch_detected() {
        const NAD83_WKT: &str = r#"GEOGCRS["NAD83",
  DATUM["North American Datum 1983",
    ELLIPSOID["GRS 1980",6378137,298.257222101,LENGTHUNIT["metre",1.0]]],
  CS[ellipsoidal,2],
    AXIS["latitude",north,ORDER[1]],
    AXIS["longitude",east,ORDER[2]],
    ANGLEUNIT["degree",0.0174532925199433],
  ID["EPSG",4269]]"#;

        let tmp = tempdir().unwrap();
        let profile = SimulationProfile::json();
        let store = CdbDatastore::create(
            tmp.path(),
            &profile,
            DatastoreSeed::new("id", "Title", "Description", "contact"),
        )
        .unwrap();

        // Sanity: the foreign CRS parses as a valid storage CRS.
        StorageCrs::from_wkt(NAD83_WKT).unwrap();
        // Overwrite the storage-CRS record with the foreign CRS.
        fs::write(
            store.layout().global_metadata_dir().join("crs.wkt"),
            NAD83_WKT,
        )
        .unwrap();

        let report = store.validate(&profile).unwrap();
        assert!(!report.class_passed(RequirementsClass::Crs), "{report}");
        assert!(
            report
                .violations(RequirementsClass::Crs)
                .iter()
                .any(|v| matches!(
                    v,
                    CdbViolation::DeclarationMismatch {
                        element: "storage CRS",
                        ..
                    }
                )),
            "{report}"
        );
    }

    /// Requirements Metadata2/5/8 (§7.9.3): when the on-disk global metadata
    /// declares a different standard or unit of measure than the profile, the
    /// Metadata class fails with a `DeclarationMismatch` per differing element.
    /// The encoding is left as JSON (matching the profile), so no encoding
    /// mismatch is reported. The doctored record is rewritten through the
    /// module API, bypassing the facade's encoding guard.
    #[test]
    fn req_core_metadata_declaration_mismatch_detected() {
        let tmp = tempdir().unwrap();
        let profile = SimulationProfile::json();
        let store = CdbDatastore::create(
            tmp.path(),
            &profile,
            DatastoreSeed::new("id", "Title", "Description", "contact"),
        )
        .unwrap();

        let mut global = store.global_metadata().unwrap();
        global.metadata_standard = MetadataStandard::Iso19115v2019;
        global.uom = UnitOfMeasure::Feet;
        global.write_to(store.layout()).unwrap();

        let report = store.validate(&profile).unwrap();
        assert!(
            !report.class_passed(RequirementsClass::Metadata),
            "{report}"
        );
        let elements: Vec<&str> = report
            .violations(RequirementsClass::Metadata)
            .iter()
            .filter_map(|v| match v {
                CdbViolation::DeclarationMismatch { element, .. } => Some(*element),
                _ => None,
            })
            .collect();
        assert!(elements.contains(&"metadata standard"), "{report}");
        assert!(elements.contains(&"uom"), "{report}");
    }

    /// Recommendation Name3-B (§7.4.2 `/req/core/name-language` B): a datastore
    /// whose declared language is not English earns a `LanguageNotEnglish`
    /// warning under File Naming — but a SHOULD finding never fails validation,
    /// so the class still passes and the report is still conformant. The
    /// wrapper profile delegates every policy to the simulation profile except
    /// its style-guide language (French) and its name, proving the trait is
    /// implementable outside the `profiles` module.
    #[test]
    fn rec_core_name_lang_non_english_warns() {
        struct FrenchProfile(SimulationProfile);
        impl ApplicationProfile for FrenchProfile {
            fn name(&self) -> &str {
                "simulation-fr"
            }
            fn style_guide(&self) -> StyleGuide {
                let mut guide = StyleGuide::new(CaseRule::PascalCase, "fr");
                guide.reserve_name("metadata");
                guide
            }
            fn storage_crs(&self) -> Result<StorageCrs, CrsViolation> {
                self.0.storage_crs()
            }
            fn metadata_standard(&self) -> MetadataStandard {
                self.0.metadata_standard()
            }
            fn metadata_encoding(&self) -> MetadataEncoding {
                self.0.metadata_encoding()
            }
            fn uom(&self) -> UnitOfMeasure {
                self.0.uom()
            }
            fn storage_technology(&self) -> StorageTechnology {
                self.0.storage_technology()
            }
            fn conformance_classes(&self) -> Vec<RequirementsClass> {
                self.0.conformance_classes()
            }
            fn is_resource_metadata(&self, logical_path: &str) -> bool {
                self.0.is_resource_metadata(logical_path)
            }
            fn known_extensions(&self) -> Vec<String> {
                self.0.known_extensions()
            }
        }

        let tmp = tempdir().unwrap();
        let profile = FrenchProfile(SimulationProfile::json());
        let store = CdbDatastore::create(
            tmp.path(),
            &profile,
            DatastoreSeed::new("id", "Titre", "Description", "contact"),
        )
        .unwrap();

        let report = store.validate(&profile).unwrap();
        assert!(report.is_conformant(), "{report}");
        assert!(
            report.class_passed(RequirementsClass::FileNaming),
            "{report}"
        );
        assert!(
            report.warnings(RequirementsClass::FileNaming).contains(
                &CdbWarning::LanguageNotEnglish {
                    language: "fr".to_owned(),
                }
            ),
            "{report}"
        );
    }

    /// Recommendation RFile1 (§7.5.6): `validate` surfaces the hierarchy
    /// walk's SHOULD findings. A datastore whose root is not named `cdb`
    /// (built here directly through the module APIs) earns a `RootNameNotCdb`
    /// warning under File Structure, yet the class still passes and the report
    /// is still conformant.
    #[test]
    fn validate_surfaces_hierarchy_walk_warnings() {
        use crate::metadata::LanguageTag;
        use crate::profiles::simulation::WGS84_2D_WKT;

        let tmp = tempdir().unwrap();
        let layout = DatastoreLayout::create_named(tmp.path(), "MyStore").unwrap();
        GlobalMetadata::builder()
            .id("id")
            .title("Title")
            .description("Description")
            .contact_point("contact")
            .created(Utc::now())
            .language(LanguageTag::new("en").unwrap())
            .standard(MetadataStandard::Dcat)
            .encoding(MetadataEncoding::Json)
            .uom(UnitOfMeasure::Meters)
            .build()
            .unwrap()
            .write_to(&layout)
            .unwrap();
        StorageCrs::from_wkt(WGS84_2D_WKT)
            .unwrap()
            .write_to(&layout)
            .unwrap();

        let store = CdbDatastore::open(layout.root()).unwrap();
        let report = store.validate(&SimulationProfile::json()).unwrap();

        assert!(report.is_conformant(), "{report}");
        assert!(
            report.class_passed(RequirementsClass::FileStructure),
            "{report}"
        );
        assert!(
            report
                .warnings(RequirementsClass::FileStructure)
                .contains(&CdbWarning::Hierarchy(HierarchyWarning::RootNameNotCdb {
                    name: "MyStore".to_owned(),
                })),
            "{report}"
        );
    }
}
