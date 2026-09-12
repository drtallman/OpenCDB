//! The conformance orchestrator: [`validate`], the abstract test behind
//! Annex A `/conf/minimal-core`, and the single directory walk its stages
//! share.

use std::fs;
use std::io;
use std::path::Path;

use crate::conformance::{CdbViolation, CdbWarning, ConformanceReport, RequirementsClass};
use crate::crs::{CrsError, StorageCrs};
use crate::datastore::{CdbDatastore, path_extension};
use crate::error::CdbError;
use crate::hierarchy::HierarchyError;
use crate::metadata::{
    GlobalMetadata, MetadataError, MetadataViolation, ResourceMetadata, encoding_violations,
};
use crate::naming::{NamingWarning, StyleGuide, component_warnings, file_warnings};
use crate::profiles::ApplicationProfile;
use crate::versioning;

/// Validates `datastore` against `profile`, the abstract test behind Annex A
/// `/conf/minimal-core`: it inspects the profile's declarations and the
/// on-disk state of the five mandatory requirements classes (CRS, File
/// Naming, File Structure, Links, Metadata) and returns a
/// [`ConformanceReport`] bucketing every finding by [`RequirementsClass`].
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
    // file's encoding (Metadata5).
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
    // single-CRS declaration (CRS3).
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
        }
        Err(CrsError::Violation(violation)) => report.record_violation(violation.into()),
        // I/O (and any future operational variant) aborts inspection.
        Err(other) => return Err(other.into()),
    }

    // Stage 6 — Resource metadata + Links: parse each recognized record and
    // record its Metadata/Links findings.
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
        if let Err(error) = parsed {
            record_metadata_error(&mut report, error)?;
        }
    }

    Ok(report)
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

    use crate::crs::CrsViolation;
    use crate::datastore::DatastoreSeed;
    use crate::hierarchy::{DatastoreLayout, HierarchyWarning};
    use crate::metadata::{MetadataEncoding, MetadataStandard, UnitOfMeasure};
    use crate::naming::{CaseRule, StyleGuide};
    use crate::profiles::{SimulationProfile, StorageTechnology};
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
