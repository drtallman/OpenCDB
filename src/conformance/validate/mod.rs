//! The conformance orchestrator: [`validate`], the abstract test behind
//! Annex A `/conf/minimal-core`, and the single directory walk its stages
//! share.
//!
//! The module is split along the seams the orchestrator already draws: this
//! file owns stages 1–6 and the walk, `stages` owns the per-class stage each
//! declared optional class gets in stage 7, and `sweep` owns the stage-8
//! content sweep. `ContentSignals` stays here because the walk and stages 4
//! and 6 are what fill it; the sweep only reads it.

mod stages;
#[cfg(test)]
mod support;
mod sweep;

use std::fs;
use std::io;
use std::path::Path;

use crate::attribution::VECTOR_ATTRIBUTES_STEM;
use crate::conformance::{CdbViolation, CdbWarning, ConformanceReport, RequirementsClass};
use crate::crs::{CrsError, StorageCrs};
use crate::datastore::{CdbDatastore, path_extension};
use crate::error::CdbError;
use crate::hierarchy::{GLOBAL_METADATA_DIR, HierarchyError};
use crate::metadata::{
    GlobalMetadata, MetadataError, MetadataViolation, ResourceMetadata, encoding_violations,
};
use crate::naming::{
    NamingWarning, StyleGuide, component_warnings, file_warnings, guard_eq, split_extension,
};
use crate::profiles::ApplicationProfile;
use crate::versioning;

use stages::{
    validate_attribution, validate_coverages, validate_geometry, validate_tiling,
    validate_topology, validate_versioning,
};
use sweep::sweep_content;

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
/// [`crate::conformance::ClassFindings::coverage`], so a pass on an empty
/// datastore cannot be misread as a pass on a clean one. The same field
/// carries the third case, [`crate::conformance::ContentCoverage::Unchecked`]:
/// the Geometry and Topology stages have content to look at but no
/// datastore-level check that could fail, and the report says so rather than
/// reporting them as checked.
///
/// A final **content sweep** closes the other half:
/// content whose class the profile never declared is reported as a
/// [`CdbViolation::DeclarationMismatch`] under that class, so a report may
/// list a class the profile said nothing about. The sweep reads only the
/// signals the directory walk and the metadata parses already produced — it
/// adds no traversal and never inspects payload bytes — and carries the two
/// declaration cross-checks (Requirement Tiling4's tiling scheme and
/// Requirement Attr1's attribute model).
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
    for &class in RequirementsClass::MANDATORY {
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
    let mut signals = ContentSignals::default();
    let (global_metadata_files, file_logical_paths) = {
        let mut walk = NamingWalk {
            guide: &guide,
            known_extensions: &known_extensions,
            report: &mut report,
            signals: &mut signals,
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
            // Sweep signal: the Tiling8 `tilingScheme` element (§7.9.4.2) is
            // what makes a datastore tiled, read off the record already
            // parsed here rather than from any tile payload.
            signals.tiling_scheme = global
                .tiling_scheme
                .as_ref()
                .map(|scheme| scheme.id.clone());
            global_record = Some(global);
        }
        Err(error) => record_metadata_error(&mut report, None, error)?,
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
            Ok(record) => {
                // Sweep signals: the two record-borne conditional elements of
                // §4's table. The first record carrying each is enough — the
                // sweep convicts a class once, not once per record.
                if record.winding_order.is_some() && signals.winding_order_record.is_none() {
                    signals.winding_order_record = Some(record.id.clone());
                }
                if record.domain_set.is_some() && signals.domain_set_record.is_none() {
                    signals.domain_set_record = Some(record.id.clone());
                }
                resource_records.push(record);
            }
            Err(error) => record_metadata_error(&mut report, Some(logical_path), error)?,
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
    // to exist — but the report separates that from a clean check, and from
    // an uncheckable one, via `ClassFindings::coverage`.
    //
    // `attribute_model` keeps what the Attribution stage parsed: stage 8
    // cross-checks it against the profile's own declaration, and it is the
    // only reader of that file.
    let mut attribute_model = None;
    for &class in RequirementsClass::OPTIONAL {
        if !declared.contains(&class) {
            continue;
        }
        report.declare_class(class);
        match class {
            RequirementsClass::Attribution => {
                attribute_model = validate_attribution(
                    datastore,
                    global_record.as_ref(),
                    &signals.attribute_model_files,
                    &mut report,
                )?;
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
                validate_versioning(
                    datastore,
                    global_record.as_ref(),
                    signals.versions_journal,
                    &mut report,
                )?;
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

    // Stage 8 — the content sweep and the two declaration cross-checks.
    sweep_content(
        profile,
        &declared,
        &signals,
        attribute_model.as_ref(),
        &mut report,
    );

    Ok(report)
}

/// The content signals of design spec §4's table, collected by the passes
/// that were already happening: the File Naming walk (stage 3) sees the
/// directory entries, stage 4 parses the global metadata record, and stage 6
/// parses the resource metadata records.
///
/// **Every signal is a directory entry or a parsed metadata element — never a
/// payload byte.** The crate does not decode GeoPackage containers or raster
/// files, so "this datastore holds topology" means *a resource metadata
/// record carries the Face4 `windingOrder` conditional element*, and "this
/// datastore holds a coverage" means *a record carries the Coverages6
/// `domainSet`*. That is the §7.9.4.2 conditional-element mechanism doing
/// exactly the job it exists for, and it is why the sweep needs no traversal
/// of its own.
///
/// Geometry has no signal here, and its absence is deliberate: Requirement
/// Geom4's `uom` — the Geometry stage's own content signal — declares a
/// *unit*, and is conditional on m coordinates living in a payload this crate
/// cannot see. A unit declaration is too weak to convict a profile of failing
/// to declare a class, so an undeclared Geometry class is not swept.
#[derive(Debug, Default)]
struct ContentSignals {
    /// File names at the top of `global_metadata/` whose stem is the
    /// reserved [`VECTOR_ATTRIBUTES_STEM`], canonical spelling or not.
    attribute_model_files: Vec<String>,
    /// The id of the `tilingScheme` element on the global record.
    tiling_scheme: Option<String>,
    /// A `versions/` journal directory directly below the root.
    versions_journal: bool,
    /// The id of the first resource record carrying `windingOrder`.
    winding_order_record: Option<String>,
    /// The id of the first resource record carrying `domainSet`.
    domain_set_record: Option<String>,
}

/// The File Naming walk of [`validate`] (stage 3): a single
/// recursive pass below the datastore root that records each name's Name1/Name6
/// findings and, in the same sweep, collects the file inventory stages 4–6
/// reuse. Symlinks are neither followed nor recursed (`DirEntry::file_type` is a
/// no-follow probe, mirroring [`crate::hierarchy`]'s empty-folder walk), so
/// external-resource links (Permission PFile1) are safe. Non-UTF-8 names are
/// lossily decoded via [`std::ffi::OsStr::to_string_lossy`] before validation.
struct NamingWalk<'a> {
    /// The profile's style guide (built once), applied to every name.
    guide: &'a StyleGuide,
    /// Extensions the profile vouches for (Name7-B); their `NonSpecExtension`
    /// warnings are suppressed.
    known_extensions: &'a [String],
    /// The report findings are recorded into.
    report: &'a mut ConformanceReport,
    /// The content sweep's signals, filled as the walk meets the directory
    /// entries that carry them (design spec §4's first and third rows) — the
    /// reason the sweep needs no traversal of its own.
    signals: &'a mut ContentSignals,
    /// File names at the top level of `global_metadata/`.
    global_metadata_files: Vec<String>,
    /// The logical path of every file found, `/`-joined from the root.
    file_logical_paths: Vec<String>,
}

/// Guard: does `dir_logical` name the datastore root's `global_metadata/`
/// folder (Requirement File6, §7.5.7)? ASCII case-folded per the crate's case
/// stance ([`crate::naming::guard_eq`]) — on a case-insensitive filesystem
/// `Global_Metadata/` *is* that folder, and a byte-exact test would leave the
/// Metadata5 encoding sweep and the Attr1 content signal blind to everything
/// inside it. Only the root's own child qualifies: a nested
/// `Tiles/global_metadata/` is ordinary content, as File6 intends.
fn is_global_metadata_dir(dir_logical: &str) -> bool {
    let relative = dir_logical.trim_start_matches('/');
    !relative.is_empty() && guard_eq(relative, GLOBAL_METADATA_DIR)
}

impl NamingWalk<'_> {
    /// Walks `dir`, whose logical path is `dir_logical` (empty for the root, so
    /// its children read as `/name`).
    ///
    /// **Entries are visited in name order, not `read_dir` order.** The
    /// report is a document that gets diffed and archived, and `read_dir` is
    /// hash-ordered on APFS and creation-ordered elsewhere, so an unsorted
    /// walk made the order of findings within a class — and the evidence
    /// value the first-record-wins signals quote — a property of the *host*
    /// rather than of the datastore. Sorting here is the one place that fixes
    /// it for every downstream consumer: the file inventory, the
    /// `global_metadata/` listing, the record parse order of stage 6 and the
    /// signals of [`ContentSignals`] all inherit it. `OsString`'s order is a
    /// byte order, which is all that is needed — it must be *a* total order,
    /// not a locale-aware one.
    fn walk(&mut self, dir: &Path, dir_logical: &str) -> io::Result<()> {
        let in_global_metadata = is_global_metadata_dir(dir_logical);
        let mut entries = fs::read_dir(dir)?.collect::<io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
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
                if in_global_metadata {
                    self.global_metadata_files.push(name.to_string());
                    // Sweep signal: an attribute-model file lives at the top
                    // of `global_metadata/` (Requirement Attr1-B). The stem
                    // is the signal, not the whole name — a mis-named file is
                    // precisely what the sweep exists to catch, and
                    // Requirement Attr1-C then judges the name. Finding the
                    // file is a guard, so the stem match folds ASCII case;
                    // judging the name is a requirement, so
                    // `attribution::parse_file_name` stays byte-exact. That
                    // pairing is what convicts `Vector_Attributes.json`
                    // rather than ignoring it.
                    if guard_eq(split_extension(&name).0, VECTOR_ATTRIBUTES_STEM) {
                        self.signals.attribute_model_files.push(name.to_string());
                    }
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
                // (parse + contiguity), so the walk does not descend. The
                // name match is a guard and folds ASCII case: on a
                // case-insensitive filesystem `Versions/` IS the journal, and
                // descending into it would name-check the crate's own
                // `v000001/manifest.json` against the profile's case rule.
                //
                // The name match resolves symlinks — and only here. A
                // symlinked `versions/` still holds a journal, so treating it
                // as "no journal" would let a datastore escape the
                // undeclared-content conviction by a link, and would make the
                // Versioning stage (which reads this one signal) disagree
                // with the sweep about the same bytes. Resolving is the
                // pessimistic direction; *descending* stays no-follow below.
                // A name that matches but does not resolve to a directory —
                // a dangling link, a plain file — is not the journal, and its
                // own findings come from the naming rules above.
                let journal = dir_logical.is_empty()
                    && guard_eq(&name, versioning::VERSIONS_DIR)
                    && (file_type.is_dir()
                        || fs::metadata(entry.path()).is_ok_and(|meta| meta.is_dir()));
                if journal {
                    // Sweep signal: the journal's existence is Versioning's
                    // content, seen here as a directory entry under the root.
                    // It is also the *stage's* signal — one guard, one answer.
                    self.signals.versions_journal = true;
                }
                if file_type.is_dir() && !journal {
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
///
/// `subject` names the document the failure is about — a record's logical
/// path, or `None` for the datastore's one global record. It is prefixed onto
/// a `Malformed` reason, because that variant's message is neutral about
/// *which* metadata document failed, and a bare parser message ("EOF while
/// parsing an object at line 1 column 14") cannot be acted on in a datastore
/// holding many records.
fn record_metadata_error(
    report: &mut ConformanceReport,
    subject: Option<&str>,
    error: MetadataError,
) -> Result<(), CdbError> {
    let about = |reason: String| match subject {
        Some(path) => format!("{path}: {reason}"),
        None => format!("global metadata: {reason}"),
    };
    match error {
        MetadataError::Violation(violation) => {
            report.record_violation(violation.into());
            Ok(())
        }
        MetadataError::Serialization(reason) => {
            report.record_violation(
                MetadataViolation::Malformed {
                    reason: about(reason),
                }
                .into(),
            );
            Ok(())
        }
        MetadataError::UnsupportedEncoding(encoding) => {
            report.record_violation(
                MetadataViolation::Malformed {
                    reason: about(format!(
                        "declares the {encoding} encoding the core cannot read"
                    )),
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

    use super::support::{DeclaringProfile, fresh_store, model_of};
    use crate::attribution::AttributionViolation;
    use crate::crs::CrsViolation;
    use crate::datastore::DatastoreSeed;
    use crate::hierarchy::{DatastoreLayout, HierarchyWarning};
    use crate::metadata::{MetadataEncoding, MetadataStandard, UnitOfMeasure};
    use crate::naming::CaseRule;
    use crate::profiles::{SimulationProfile, StorageTechnology};
    use crate::versioning::PendingCollection;
    use tempfile::tempdir;

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
        for &class in RequirementsClass::MANDATORY {
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

    /// §7.5.7 Requirement File6 under the crate's case stance — the
    /// `global_metadata/` detection is a guard and folds ASCII case, so the
    /// Metadata5 encoding sweep and the Attr1 content signal are not blind on
    /// a case-insensitive filesystem. Filesystem-independent by construction:
    /// it is a comparison on the logical path the walk built. Only the root's
    /// own child qualifies — a nested folder of the same name is content.
    #[test]
    fn req_core_file_root_global_metadata_dir_guard_folds_case() {
        for dir in ["/global_metadata", "/Global_Metadata", "/GLOBAL_METADATA"] {
            assert!(is_global_metadata_dir(dir), "{dir}");
        }
        for dir in [
            "",
            "/",
            "/Tiles",
            "/Tiles/global_metadata",
            "/global_metadata_backup",
        ] {
            assert!(!is_global_metadata_dir(dir), "{dir:?}");
        }
    }

    /// §7.1.2.1 Requirement Attr1-C under the crate's case stance — the two
    /// halves of the split rule meeting on one file.
    ///
    /// The sweep's *signal* is a folded stem match (a guard: find the file
    /// that claims to be the attribute model, however it is spelled), while
    /// the name check itself stays byte-exact (a requirement: Attr1-C
    /// mandates one literal name). So `Vector_Attributes.json` is seen and
    /// then convicted. Before the fold it drew no signal at all, and the
    /// datastore looked clean while carrying a model the facade can only
    /// read by the accident of a case-insensitive host.
    ///
    /// Filesystem-independent by construction: the file is literally named
    /// `Vector_Attributes.json` on disk in either regime, so it is the
    /// string comparison, not the host, that does the work.
    #[test]
    fn req_core_attribute_model_mis_cased_stem_swept() {
        let tmp = tempdir().unwrap();
        let store = fresh_store(&tmp);
        let canonical = store
            .write_attribute_model(&model_of("1", "StreetName"))
            .unwrap();
        fs::rename(
            &canonical,
            store
                .layout()
                .global_metadata_dir()
                .join("Vector_Attributes.json"),
        )
        .unwrap();
        let report = store.validate(&DeclaringProfile::all()).unwrap();
        assert!(
            report
                .violations(RequirementsClass::Attribution)
                .iter()
                .any(|violation| matches!(
                    violation,
                    CdbViolation::Attribution(AttributionViolation::InvalidFileName { .. })
                )),
            "{report}"
        );
    }

    /// §7.14.2 Requirement V1 under the crate's case stance — the walk's
    /// journal guard folds, so a `Versions/` directory is recognized as the
    /// crate-managed journal and is not descended into.
    ///
    /// The observable is filesystem-independent: the directory is literally
    /// named `Versions` on disk in either regime, and without the fold the
    /// walk name-checks the journal's own `v000001/` and `manifest.json`,
    /// neither of which any profile case rule admits. Those leaked findings
    /// are what must not appear.
    #[test]
    fn req_core_versioning_journal_dir_guard_folds_case() {
        let tmp = tempdir().unwrap();
        let store = fresh_store(&tmp);
        store
            .apply_collection(PendingCollection::new().create("/Tiles/Roads.gpkg", *b"roads"))
            .unwrap();
        fs::rename(store.root().join("versions"), store.root().join("Versions")).unwrap();
        let report = store.validate(&DeclaringProfile::all()).unwrap();
        let leaked: Vec<String> = report
            .violations(RequirementsClass::FileNaming)
            .iter()
            .map(ToString::to_string)
            .filter(|text| text.contains("v000001") || text.contains("manifest"))
            .collect();
        assert!(
            leaked.is_empty(),
            "journal internals name-checked: {leaked:?}"
        );
    }

    /// §7.9.4.2 Requirement Metadata5 — **a record that fails to parse is
    /// reported as the record it is, and is named.**
    ///
    /// [`MetadataViolation::Malformed`] is the finding for *any* metadata
    /// document this crate cannot parse, and stage 6 routes every resource
    /// record's parse failure through it — but its message said "global
    /// metadata could not be parsed" and named no file, so a datastore with
    /// forty records told its owner only that something, somewhere, was
    /// unreadable. The message is neutral now, and the orchestrator prefixes
    /// the record's logical path onto the reason.
    #[test]
    fn req_core_metadata_malformed_record_is_named() {
        let tmp = tempdir().unwrap();
        let store = fresh_store(&tmp);
        let record = ResourceMetadata::new("Roads", "Road Network", "Tiled roads");
        store
            .write_resource_metadata("/Tiles/metadata/Roads.json", &record)
            .unwrap();
        fs::write(
            store.root().join("Tiles/metadata/Roads.json"),
            "{\"ID\": \"Roads\"",
        )
        .unwrap();

        let report = store.validate(&DeclaringProfile::all()).unwrap();
        let text = report
            .violations(RequirementsClass::Metadata)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.contains("/Tiles/metadata/Roads.json"),
            "the failing record must be named: {report}"
        );
        assert!(
            !text.contains("global metadata could not be parsed"),
            "a resource record is not the global record: {report}"
        );
    }

    /// §7.14.2 Requirement V1 — **the Versioning stage and the content sweep
    /// answer "does this datastore have a journal?" from one guard.**
    ///
    /// They used to answer it twice: the walk's no-follow `file_type` probe
    /// (which the sweep reads) said no for a symlinked `versions/`, while the
    /// stage's own `root().join("versions").is_dir()` followed the link and
    /// said yes. A datastore therefore escaped the undeclared-content
    /// conviction by a symlink while a declaring profile still validated the
    /// journal behind it. The walk now resolves the journal name — a symlink
    /// must not hide content from the sweep — while still refusing to
    /// *descend* into any symlink (Permission PFile1's external resources),
    /// and the stage reads that one signal instead of re-deriving a private
    /// path.
    #[cfg(unix)]
    #[test]
    fn req_core_versioning_journal_guard_agrees_across_stage_and_sweep() {
        let tmp = tempdir().unwrap();
        let store = fresh_store(&tmp);
        store
            .apply_collection(PendingCollection::new().create("/Tiles/Roads.gpkg", *b"roads"))
            .unwrap();
        // Relocate the journal and leave a symlink in its place.
        let real = tmp.path().join("journal");
        fs::rename(store.root().join("versions"), &real).unwrap();
        std::os::unix::fs::symlink(&real, store.root().join("versions")).unwrap();

        // Declared: the journal behind the link is content, and it validates.
        let declared = store.validate(&DeclaringProfile::all()).unwrap();
        assert_eq!(
            declared.class_coverage(RequirementsClass::Versioning),
            crate::conformance::ContentCoverage::Checked,
            "{declared}"
        );
        // Undeclared: the same content convicts the profile.
        let swept = store.validate(&DeclaringProfile::mandatory_only()).unwrap();
        assert!(
            swept
                .violations(RequirementsClass::Versioning)
                .iter()
                .any(|violation| matches!(
                    violation,
                    CdbViolation::DeclarationMismatch {
                        element: "versions journal",
                        ..
                    }
                )),
            "{swept}"
        );
    }

    /// Design spec §4 — **the walk visits each directory's entries in name
    /// order, so a report is a function of the datastore's content and not of
    /// its filesystem.**
    ///
    /// `validate` was already deterministic for a fixed tree, but the order
    /// came from `read_dir`, which is hash-ordered on APFS and
    /// creation-ordered elsewhere: the *evidence value* a
    /// `DeclarationMismatch` quotes ("the record that carries `windingOrder`")
    /// and the order of findings within a class therefore differed between
    /// hosts holding identical bytes. A conformance report is a document that
    /// gets diffed and archived, so it is sorted at the source — one
    /// `sort_by` per directory, which also makes the first-record-wins
    /// signals name the lexicographically first record rather than an
    /// arbitrary one.
    #[test]
    fn conf_core_walk_visits_entries_in_name_order() {
        let tmp = tempdir().unwrap();
        let store = fresh_store(&tmp);

        // Written in an order that is neither lexicographic nor its reverse;
        // on APFS `read_dir` returns them in a third order again.
        for id in ["Delta", "Hotel", "Alpha", "Golf", "Charlie", "Bravo"] {
            let mut record = ResourceMetadata::new(id, "Road Network", "Tiled roads");
            record.keywords = vec!["transportation".to_owned()];
            record.winding_order = Some(crate::topology::WindingOrder::Counterclockwise);
            store
                .write_resource_metadata(&format!("/Tiles/metadata/{id}.json"), &record)
                .unwrap();
        }

        let report = store.validate(&DeclaringProfile::mandatory_only()).unwrap();
        let evidence: Vec<String> = report
            .violations(RequirementsClass::Topology)
            .iter()
            .filter_map(|violation| match violation {
                CdbViolation::DeclarationMismatch { found, .. } => Some(found.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            evidence,
            vec!["resource metadata record \"Alpha\"".to_owned()],
            "evidence must name the first record in NAME order: {report}"
        );
    }
}
