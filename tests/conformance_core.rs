//! Annex A Abstract Test Suite — `/conf/minimal-core`.
//!
//! The CDB 2.0 Core defines exactly one conformance test (Annex A): the
//! abstract test `/conf/minimal-core`, whose test method is *inspection of the
//! application profile*. A datastore satisfies it by implementing the five
//! mandatory requirements classes — CRS, File Naming, File Structure, Links,
//! and Metadata (spec §2). These integration tests exercise that test end to
//! end against real `tempfile` datastores driven through the public
//! [`rusty_cdb`] facade: one test mechanizes the literal declaration
//! inspection, one proves a fully-populated datastore conformant, and the rest
//! break exactly one requirements class each and confirm the
//! [`rusty_cdb::ConformanceReport`] localizes the failure to that class.

use std::fs;

use rusty_cdb::crs::{CrsViolation, StorageCrs};
use rusty_cdb::hierarchy::HierarchyViolation;
use rusty_cdb::links::{Link, LinkViolation};
use rusty_cdb::metadata::{
    MetadataEncoding, MetadataStandard, MetadataViolation, ResourceMetadata, UnitOfMeasure,
};
use rusty_cdb::naming::{NamingViolation, StyleGuide};
use rusty_cdb::profiles::StorageTechnology;
use rusty_cdb::{
    ApplicationProfile, CdbDatastore, CdbViolation, DatastoreSeed, RequirementsClass,
    SimulationProfile,
};
use tempfile::TempDir;

/// Builds a fully-populated, conformant simulation datastore: the root
/// metadata and storage CRS (written by `create`), a dummy data file at
/// `/Tiles/RoadNetwork.gpkg`, and a valid resource-metadata record at
/// `/Tiles/metadata/RoadNetwork.json` carrying one `item` association link.
/// Returns the temp-dir guard (kept alive by the caller), the datastore, and
/// the profile it was created from — the yardstick for `validate`.
fn full_datastore() -> (TempDir, CdbDatastore, SimulationProfile) {
    let tmp = tempfile::tempdir().unwrap();
    let profile = SimulationProfile::json();
    let store = CdbDatastore::create(
        tmp.path(),
        &profile,
        DatastoreSeed::new(
            "doi:cdb.demo",
            "Demo",
            "A demonstration datastore",
            "ops@example.com",
        ),
    )
    .unwrap();

    // A dummy data file (its parent directory is created on demand).
    let data = store.resolve("/Tiles/RoadNetwork.gpkg").unwrap();
    fs::create_dir_all(data.parent().unwrap()).unwrap();
    fs::write(&data, b"dummy gpkg bytes").unwrap();

    // A valid resource-metadata record with one association link.
    let mut record = ResourceMetadata::new("roads", "RoadNetwork", "The road network");
    record.associations = vec![Link::new("https://example.com/tiles", "item").unwrap()];
    store
        .write_resource_metadata("/Tiles/metadata/RoadNetwork.json", &record)
        .unwrap();

    (tmp, store, profile)
}

/// Annex A §A.2 test method — *inspection of the application profile*: the
/// literal abstract test mechanized. The simulation profile's declared
/// conformance classes include every mandatory class; no datastore is needed.
#[test]
fn conf_core_minimal_profile_declaration_inspection() {
    let declared = SimulationProfile::json().conformance_classes();
    for class in RequirementsClass::MANDATORY {
        assert!(
            declared.contains(&class),
            "profile must declare mandatory class {class}"
        );
    }
}

/// Annex A `/conf/minimal-core` — the pass case across all five classes: a
/// fully-populated simulation datastore validates clean. It is conformant,
/// every mandatory class passes, and (crucially) every class has zero
/// warnings — proving the crate's naming wrinkles hold end to end (`crs` is
/// auto-reserved so `crs.wkt` escapes the case rule; the profile vouches for
/// the `.wkt` extension so its non-spec-extension warning is suppressed).
#[test]
fn conf_core_minimal_full_datastore_all_mandatory_classes_pass() {
    let (_tmp, store, profile) = full_datastore();

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

/// Annex A `/conf/minimal-core`: the abstract test is falsified by the
/// profile's *declaration*, independent of the on-disk state. A profile that
/// omits a mandatory class — here Links — makes even a perfect datastore
/// non-conformant, with a `MissingConformanceDeclaration` filed under the
/// undeclared class; the other four classes still pass. The wrapper delegates
/// every policy to the simulation profile, so the missing declaration is the
/// sole difference (and lives outside `profiles`, proving the trait is
/// implementable there).
#[test]
fn conf_core_minimal_missing_declaration_fails_class() {
    struct MissingLinksProfile(SimulationProfile);
    impl ApplicationProfile for MissingLinksProfile {
        fn name(&self) -> &str {
            "simulation-no-links"
        }
        fn style_guide(&self) -> StyleGuide {
            self.0.style_guide()
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
            RequirementsClass::MANDATORY
                .into_iter()
                .filter(|class| *class != RequirementsClass::Links)
                .collect()
        }
        fn is_resource_metadata(&self, logical_path: &str) -> bool {
            self.0.is_resource_metadata(logical_path)
        }
        fn known_extensions(&self) -> Vec<String> {
            self.0.known_extensions()
        }
    }

    let (_tmp, store, _profile) = full_datastore();
    let profile = MissingLinksProfile(SimulationProfile::json());

    let report = store.validate(&profile).unwrap();
    assert!(!report.is_conformant(), "{report}");
    assert!(!report.class_passed(RequirementsClass::Links), "{report}");
    assert!(
        report
            .violations(RequirementsClass::Links)
            .iter()
            .any(|violation| matches!(
                violation,
                CdbViolation::MissingConformanceDeclaration {
                    class: RequirementsClass::Links,
                    ..
                }
            )),
        "{report}"
    );
    for class in [
        RequirementsClass::Crs,
        RequirementsClass::FileNaming,
        RequirementsClass::FileStructure,
        RequirementsClass::Metadata,
    ] {
        assert!(report.class_passed(class), "{class}: {report}");
    }
}

/// Requirement CRS5 (§7.3.1.4, `/req/core/crs/crsMetadata`): with the
/// storage-CRS record `global_metadata/crs.wkt` deleted, the CRS class fails
/// with `MissingCrsMetadata`. The other four classes still pass — the CRS
/// record is not their evidence.
#[test]
fn conf_core_break_crs_reports_missing_crs_metadata() {
    let (_tmp, store, profile) = full_datastore();
    fs::remove_file(store.layout().global_metadata_dir().join("crs.wkt")).unwrap();

    let report = store.validate(&profile).unwrap();
    assert!(!report.class_passed(RequirementsClass::Crs), "{report}");
    assert!(
        report
            .violations(RequirementsClass::Crs)
            .iter()
            .any(|violation| matches!(
                violation,
                CdbViolation::Crs(CrsViolation::MissingCrsMetadata { .. })
            )),
        "{report}"
    );
    for class in [
        RequirementsClass::FileNaming,
        RequirementsClass::FileStructure,
        RequirementsClass::Links,
        RequirementsClass::Metadata,
    ] {
        assert!(report.class_passed(class), "{class}: {report}");
    }
}

/// Requirement Name6 (§7.4.7, `/req/core/name-case`): a file whose stem
/// violates the datastore's PascalCase rule — `road_network.tif` under
/// `Tiles/` — fails the File Naming class with `CaseRuleViolation`. The other
/// four classes still pass (the file is data, not metadata, and `.tif` is a
/// spec-table extension, so no encoding or extension finding arises).
#[test]
fn conf_core_break_naming_reports_case_rule_violation() {
    let (_tmp, store, profile) = full_datastore();
    let offending = store.resolve("/Tiles/road_network.tif").unwrap();
    fs::write(&offending, b"x").unwrap();

    let report = store.validate(&profile).unwrap();
    assert!(
        !report.class_passed(RequirementsClass::FileNaming),
        "{report}"
    );
    assert!(
        report
            .violations(RequirementsClass::FileNaming)
            .iter()
            .any(|violation| matches!(
                violation,
                CdbViolation::Naming(NamingViolation::CaseRuleViolation { .. })
            )),
        "{report}"
    );
    for class in [
        RequirementsClass::Crs,
        RequirementsClass::FileStructure,
        RequirementsClass::Links,
        RequirementsClass::Metadata,
    ] {
        assert!(report.class_passed(class), "{class}: {report}");
    }
}

/// Requirement File6 (§7.5.7, `/req/core/file-root-global-metadata`): with the
/// `global_metadata/` folder removed, the File Structure class fails with
/// `MissingGlobalMetadata`. This break is deliberately *not* surgical — that
/// folder is also the home of the global metadata record (Metadata1) and the
/// storage CRS (CRS5), so the Metadata and CRS classes fail as collateral.
/// The test therefore asserts only the File Structure target plus that File
/// Naming and Links — whose evidence lives elsewhere in the tree — still pass.
#[test]
fn conf_core_break_structure_reports_missing_global_metadata_dir() {
    let (_tmp, store, profile) = full_datastore();
    fs::remove_dir_all(store.layout().global_metadata_dir()).unwrap();

    let report = store.validate(&profile).unwrap();
    assert!(
        !report.class_passed(RequirementsClass::FileStructure),
        "{report}"
    );
    assert!(
        report
            .violations(RequirementsClass::FileStructure)
            .iter()
            .any(|violation| matches!(
                violation,
                CdbViolation::Hierarchy(HierarchyViolation::MissingGlobalMetadata { .. })
            )),
        "{report}"
    );
    assert!(
        report.class_passed(RequirementsClass::FileNaming),
        "{report}"
    );
    assert!(report.class_passed(RequirementsClass::Links), "{report}");
}

/// Requirement Link2 (§7.7, `/req/core/link-rel`): a resource-metadata record
/// whose association has an empty `rel` fails the Links class with
/// `MissingRel`. The record is otherwise well-formed, so serde deserializes it
/// and parse-time validation catches the bad link. The raw JSON is built from
/// a valid record's serialization with only the `rel` value replaced, keeping
/// the test robust against serde field-name drift. The other four classes
/// still pass.
#[test]
fn conf_core_break_links_reports_invalid_association() {
    let (_tmp, store, profile) = full_datastore();

    let mut record = ResourceMetadata::new("roads", "RoadNetwork", "The road network");
    record.associations = vec![Link::new("https://example.com/tiles", "item").unwrap()];
    let json = record.to_json_string().unwrap();
    let broken = json.replace(r#""rel": "item""#, r#""rel": """#);
    assert_ne!(broken, json, "the rel replacement must apply");

    let path = store.resolve("/Tiles/metadata/RoadNetwork.json").unwrap();
    fs::write(&path, broken).unwrap();

    let report = store.validate(&profile).unwrap();
    assert!(!report.class_passed(RequirementsClass::Links), "{report}");
    assert!(
        report
            .violations(RequirementsClass::Links)
            .iter()
            .any(|violation| matches!(violation, CdbViolation::Link(LinkViolation::MissingRel))),
        "{report}"
    );
    for class in [
        RequirementsClass::Crs,
        RequirementsClass::FileNaming,
        RequirementsClass::FileStructure,
        RequirementsClass::Metadata,
    ] {
        assert!(report.class_passed(class), "{class}: {report}");
    }
}

/// Requirements Metadata1/Metadata3 (§7.9.3.1/§7.9.3.3): with the global
/// metadata record deleted but its folder and the storage CRS intact, the
/// Metadata class fails with `MissingGlobalMetadata`. The other four classes
/// still pass — the folder still exists (File Structure), the CRS is readable
/// (CRS), the tree names are legal (File Naming), and the resource metadata
/// links are valid (Links).
#[test]
fn conf_core_break_metadata_reports_missing_global_record() {
    let (_tmp, store, profile) = full_datastore();
    fs::remove_file(
        store
            .layout()
            .global_metadata_dir()
            .join("global_metadata.json"),
    )
    .unwrap();

    let report = store.validate(&profile).unwrap();
    assert!(
        !report.class_passed(RequirementsClass::Metadata),
        "{report}"
    );
    assert!(
        report
            .violations(RequirementsClass::Metadata)
            .iter()
            .any(|violation| matches!(
                violation,
                CdbViolation::Metadata(MetadataViolation::MissingGlobalMetadata { .. })
            )),
        "{report}"
    );
    for class in [
        RequirementsClass::Crs,
        RequirementsClass::FileNaming,
        RequirementsClass::FileStructure,
        RequirementsClass::Links,
    ] {
        assert!(report.class_passed(class), "{class}: {report}");
    }
}
