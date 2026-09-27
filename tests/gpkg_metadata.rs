#[path = "support/gpkg.rs"]
mod support;

use opencdb::metadata::{MetadataEncoding, MetadataError};
use opencdb::{CdbDatastore, CdbError};

/// CDB §7.9.3.1/.3 and §7.9.4.2: global and resource metadata survive reopen.
#[cfg(feature = "gpkg-metadata")]
#[test]
fn req_core_metadata_gpkg_facade_roundtrip() {
    use opencdb::metadata::{GlobalMetadata, ResourceMetadata};
    let tmp = tempfile::tempdir().unwrap();
    let store = CdbDatastore::create(
        tmp.path(),
        &support::GpkgProfile::default(),
        support::seed(),
    )
    .unwrap();
    let mut resource = ResourceMetadata::new("roads", "Roads", "Network");
    resource.created = Some(support::ts("2026-09-27T12:00:00.123456Z"));
    store
        .write_resource_metadata("/Tiles/metadata/Roads.gpkg", &resource)
        .unwrap();
    let mut global = store.global_metadata().unwrap();
    global.license = Some("CC-BY-4.0".into());
    store.write_global_metadata(&global).unwrap();
    let reopened = CdbDatastore::open(store.root()).unwrap();
    assert_eq!(
        reopened
            .read_resource_metadata("/Tiles/metadata/Roads.gpkg")
            .unwrap(),
        resource
    );
    assert_eq!(reopened.global_metadata().unwrap(), global);
    assert_eq!(global.encoding, MetadataEncoding::Gpkg);
    assert_eq!(
        reopened.storage_crs().unwrap(),
        store.storage_crs().unwrap()
    );
    assert_eq!(
        GlobalMetadata::locate(reopened.layout()).unwrap(),
        "/global_metadata/global_metadata.gpkg"
    );
}

#[cfg(not(feature = "gpkg-metadata"))]
#[test]
fn gpkg_binding_create_without_feature_refuses_before_io() {
    let tmp = tempfile::tempdir().unwrap();
    let result = CdbDatastore::create(
        tmp.path(),
        &support::GpkgProfile::default(),
        support::seed(),
    );
    assert!(matches!(
        result,
        Err(CdbError::Metadata(MetadataError::UnsupportedEncoding(
            MetadataEncoding::Gpkg
        )))
    ));
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
}

#[cfg(feature = "gpkg-metadata")]
mod enabled {
    use super::*;
    use opencdb::SimulationProfile;
    use opencdb::attribution::{AttributeDef, AttributeModel};
    use opencdb::metadata::{GlobalMetadata, MetadataViolation, ResourceMetadata};

    fn model() -> AttributeModel {
        AttributeModel {
            schema_uri: None,
            attributes: vec![AttributeDef {
                id: "1".into(),
                name: "Name".into(),
                description: "Label".into(),
            }],
        }
    }

    /// Metadata5 (§7.9.3.5): every facade write preserves the physical encoding.
    #[test]
    fn req_core_metadata_gpkg_rejects_cross_encoding_writes() {
        let tmp = tempfile::tempdir().unwrap();
        let store = CdbDatastore::create(
            tmp.path(),
            &support::GpkgProfile::default(),
            support::seed(),
        )
        .unwrap();
        let resource = ResourceMetadata::new("roads", "Roads", "Network");
        for path in ["/Tiles/metadata/Roads.json", "/Tiles/metadata/Roads.xml"] {
            assert!(matches!(
                store.write_resource_metadata(path, &resource),
                Err(CdbError::Metadata(MetadataError::Violation(
                    MetadataViolation::EncodingMismatch { .. }
                )))
            ));
            assert!(!store.resolve(path).unwrap().exists());
        }
        for encoding in [MetadataEncoding::Json, MetadataEncoding::Xml] {
            let mut global = store.global_metadata().unwrap();
            global.encoding = encoding;
            assert!(matches!(
                store.write_global_metadata(&global),
                Err(CdbError::Metadata(MetadataError::Violation(
                    MetadataViolation::EncodingMismatch { .. }
                )))
            ));
        }
        let other = tempfile::tempdir().unwrap();
        let json = CdbDatastore::create(other.path(), &SimulationProfile::json(), support::seed())
            .unwrap();
        let mut global = json.global_metadata().unwrap();
        global.encoding = MetadataEncoding::Gpkg;
        assert!(matches!(
            json.write_global_metadata(&global),
            Err(CdbError::Metadata(MetadataError::Violation(
                MetadataViolation::EncodingMismatch { .. }
            )))
        ));
        assert!(
            !json
                .root()
                .join("global_metadata/global_metadata.gpkg")
                .exists()
        );
    }

    /// Metadata1/3 (§7.9.3.1/.3): a GeoPackage global candidate may not be ambiguous.
    #[test]
    fn req_core_metadata_gpkg_global_discovery_refuses_ambiguity() {
        for extra in [
            ["json"].as_slice(),
            ["xml"].as_slice(),
            ["json", "xml"].as_slice(),
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let store = CdbDatastore::create(
                tmp.path(),
                &support::GpkgProfile::default(),
                support::seed(),
            )
            .unwrap();
            for ext in extra {
                std::fs::write(
                    store
                        .root()
                        .join(format!("global_metadata/global_metadata.{ext}")),
                    "{}",
                )
                .unwrap();
            }
            assert!(matches!(
                store.global_metadata(),
                Err(CdbError::Metadata(
                    MetadataError::UnsupportedContainer { .. }
                ))
            ));
            assert!(matches!(
                GlobalMetadata::locate(store.layout()),
                Err(MetadataError::UnsupportedContainer { .. })
            ));
        }
    }

    /// Attr1-C and Metadata5: required XML/JSON attribution cannot share Gpkg encoding.
    #[test]
    fn req_core_metadata_gpkg_attribute_model_refused_before_creation() {
        let tmp = tempfile::tempdir().unwrap();
        let profile = support::GpkgProfile {
            model: Some(model()),
        };
        assert!(matches!(
            CdbDatastore::create(tmp.path(), &profile, support::seed()),
            Err(CdbError::Metadata(
                MetadataError::UnsupportedContainer { .. }
            ))
        ));
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
        let store = CdbDatastore::create(
            tmp.path(),
            &support::GpkgProfile::default(),
            support::seed(),
        )
        .unwrap();
        assert!(matches!(
            store.attribute_model(),
            Err(CdbError::Metadata(MetadataError::UnsupportedEncoding(
                MetadataEncoding::Gpkg
            )))
        ));
        assert!(matches!(
            store.write_attribute_model(&model()),
            Err(CdbError::Metadata(MetadataError::UnsupportedEncoding(
                MetadataEncoding::Gpkg
            )))
        ));
    }

    /// Metadata5: recognition folds ASCII case; unknown suffixes are refused.
    #[test]
    fn req_core_metadata_gpkg_case_and_unknown_extensions() {
        let tmp = tempfile::tempdir().unwrap();
        let store = CdbDatastore::create(
            tmp.path(),
            &support::GpkgProfile::default(),
            support::seed(),
        )
        .unwrap();
        let resource = ResourceMetadata::new("roads", "Roads", "Network");
        store
            .write_resource_metadata("/Tiles/metadata/Roads.GPKG", &resource)
            .unwrap();
        assert_eq!(
            store
                .read_resource_metadata("/Tiles/metadata/Roads.GPKG")
                .unwrap(),
            resource
        );
        assert!(matches!(
            store.write_resource_metadata("/Tiles/metadata/Roads.bad", &resource),
            Err(CdbError::Metadata(MetadataError::Violation(
                MetadataViolation::Malformed { .. }
            )))
        ));
        std::fs::write(store.resolve("/Tiles/metadata/Roads.bad").unwrap(), "{}").unwrap();
        assert!(matches!(
            store.read_resource_metadata("/Tiles/metadata/Roads.bad"),
            Err(CdbError::Metadata(MetadataError::Violation(
                MetadataViolation::Malformed { .. }
            )))
        ));
    }
}

/// Metadata5: even feature-disabled builds must guard existing Gpkg files.
#[test]
fn req_core_metadata_gpkg_existing_file_blocks_encoding_switch_in_all_builds() {
    let tmp = tempfile::tempdir().unwrap();
    let store = CdbDatastore::create(
        tmp.path(),
        &opencdb::SimulationProfile::json(),
        support::seed(),
    )
    .unwrap();
    let global = store.global_metadata().unwrap();
    let dir = store.root().join("global_metadata");
    std::fs::remove_file(dir.join("global_metadata.json")).unwrap();
    std::fs::write(
        dir.join("global_metadata.gpkg"),
        b"opaque existing container",
    )
    .unwrap();
    assert!(matches!(
        store.write_global_metadata(&global),
        Err(CdbError::Metadata(MetadataError::Violation(
            opencdb::metadata::MetadataViolation::EncodingMismatch {
                found: MetadataEncoding::Gpkg,
                ..
            }
        )))
    ));
    assert!(!dir.join("global_metadata.json").exists());
}

/// Metadata1/3: discovery must distinguish inaccessible candidates from absence.
#[cfg(unix)]
#[test]
fn req_core_metadata_gpkg_discovery_propagates_stat_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let store = CdbDatastore::create(
        tmp.path(),
        &opencdb::SimulationProfile::json(),
        support::seed(),
    )
    .unwrap();
    let global = store.global_metadata().unwrap();
    let path = store.root().join("global_metadata/global_metadata.gpkg");
    std::os::unix::fs::symlink("global_metadata.gpkg", &path).unwrap();
    assert!(matches!(
        store.global_metadata(),
        Err(CdbError::Metadata(MetadataError::Io(_)))
    ));
    assert!(matches!(
        opencdb::metadata::GlobalMetadata::locate(store.layout()),
        Err(MetadataError::Io(_))
    ));
    assert!(matches!(
        store.write_global_metadata(&global),
        Err(CdbError::Metadata(MetadataError::Io(_)))
    ));
}

/// The GeoPackage recommendation uses the existing four-field warning wire shape.
#[test]
fn gpkg_binding_warning_has_stable_report_shape() {
    use opencdb::conformance::{CdbWarning, RequirementsClass};
    use opencdb::metadata::MetadataWarning;
    let warning = CdbWarning::Metadata(MetadataWarning::GpkgWithoutUserData {
        file: "/global_metadata/global_metadata.gpkg".to_owned(),
    });
    assert_eq!(warning.class(), RequirementsClass::Metadata);
    assert_eq!(warning.code(), "/rec/geopackage/user-data-table");
    let wire = serde_json::to_value(warning).unwrap();
    assert_eq!(wire.as_object().unwrap().len(), 4);
    assert_eq!(wire["severity"], "warning");
    assert_eq!(wire["class"], "metadata");
}

/// A disabled codec cannot mark an unread recognized metadata record as checked.
#[cfg(not(feature = "gpkg-metadata"))]
#[test]
fn gpkg_binding_validation_without_feature_is_operational() {
    let tmp = tempfile::tempdir().unwrap();
    let profile = opencdb::SimulationProfile::json();
    let store = CdbDatastore::create(tmp.path(), &profile, support::seed()).unwrap();
    let path = store.resolve("/Tiles/metadata/Roads.gpkg").unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"opaque Gpkg bytes").unwrap();
    assert!(matches!(
        store.validate(&profile),
        Err(CdbError::Metadata(MetadataError::UnsupportedEncoding(
            MetadataEncoding::Gpkg
        )))
    ));
}

#[cfg(feature = "gpkg-metadata")]
mod conformance {
    use super::*;
    use opencdb::conformance::RequirementsClass as Class;
    use opencdb::metadata::{ResourceMetadata, UnitOfMeasure};

    /// GeoPackage 1.2.1 §2 is a SHOULD: a fresh metadata datastore remains conformant.
    #[test]
    fn req_core_metadata_gpkg_fresh_store_has_one_recommendation() {
        let tmp = tempfile::tempdir().unwrap();
        let profile = support::GpkgProfile::default();
        let store = CdbDatastore::create(tmp.path(), &profile, support::seed()).unwrap();
        let report = store.validate(&profile).unwrap();
        assert!(report.is_conformant(), "{report}");
        assert_eq!(report.warnings(Class::Metadata).len(), 1, "{report}");
        assert_eq!(
            report.warnings(Class::Metadata)[0].code(),
            "/rec/geopackage/user-data-table"
        );
    }

    /// Link1/2: an association carried inside Gpkg enters the existing Links stage.
    #[test]
    fn req_core_metadata_gpkg_invalid_association_is_a_links_finding() {
        let tmp = tempfile::tempdir().unwrap();
        let profile = support::GpkgProfile::default();
        let store = CdbDatastore::create(tmp.path(), &profile, support::seed()).unwrap();
        let logical = "/Tiles/metadata/Roads.gpkg";
        let path = store
            .write_resource_metadata(logical, &ResourceMetadata::new("roads", "Roads", "Network"))
            .unwrap();
        let db = rusqlite::Connection::open(path).unwrap();
        let content: String = db
            .query_row("SELECT metadata FROM gpkg_metadata", [], |r| r.get(0))
            .unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&content).unwrap();
        value["associations"] = serde_json::json!([{"href":"", "rel":"describes"}]);
        db.execute("UPDATE gpkg_metadata SET metadata=?1", [value.to_string()])
            .unwrap();
        db.close().unwrap();
        let report = store.validate(&profile).unwrap();
        assert!(!report.violations(Class::Links).is_empty(), "{report}");
        assert_eq!(report.warnings(Class::Metadata).len(), 2, "{report}");
    }

    /// Coverages6, Tiling10 and Face4 conditional metadata retain their existing stages.
    #[test]
    fn req_core_metadata_gpkg_conditional_records_reach_class_stages() {
        let tmp = tempfile::tempdir().unwrap();
        let profile = support::GpkgProfile::default();
        let store = CdbDatastore::create(tmp.path(), &profile, support::seed()).unwrap();
        let mut global = store.global_metadata().unwrap();
        global.tiling_scheme = Some(opencdb::tiling::TilingScheme::cdb1_global_grid());
        store.write_global_metadata(&global).unwrap();
        let mut record = ResourceMetadata::new("roads", "Roads", "Network");
        record.domain_set = Some(opencdb::coverage::DomainSet::new("m"));
        record.uom = Some(UnitOfMeasure::Meters);
        record.winding_order = Some(opencdb::topology::WindingOrder::Clockwise);
        store
            .write_resource_metadata("/Tiles/metadata/Roads.gpkg", &record)
            .unwrap();
        let report = store.validate(&profile).unwrap();
        assert!(
            report
                .violations(Class::Tiling)
                .iter()
                .any(|v| v.code() == "/req/core/tiling-tileset-metadata-elements"),
            "{report}"
        );
        let coverage = |class| {
            report
                .classes()
                .find(|(found, _)| *found == class)
                .unwrap()
                .1
                .coverage
        };
        assert_eq!(
            coverage(Class::Coverages),
            opencdb::conformance::ContentCoverage::Checked
        );
        assert_eq!(
            coverage(Class::Geometry),
            opencdb::conformance::ContentCoverage::Unchecked
        );
        assert_eq!(
            coverage(Class::Topology),
            opencdb::conformance::ContentCoverage::Unchecked
        );
        assert_eq!(report.warnings(Class::Metadata).len(), 2);
    }
}

/// Container recommendations follow the physical encoding, not a JSON declaration.
#[test]
fn gpkg_binding_json_claiming_gpkg_gets_no_container_warning() {
    let tmp = tempfile::tempdir().unwrap();
    let profile = opencdb::SimulationProfile::json();
    let store = CdbDatastore::create(tmp.path(), &profile, support::seed()).unwrap();
    let path = store.root().join("global_metadata/global_metadata.json");
    let mut document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    document["metadataEncoding"] = serde_json::json!("gpkg");
    std::fs::write(path, document.to_string()).unwrap();
    let report = store.validate(&profile).unwrap();
    assert!(!report.is_conformant());
    assert!(
        report
            .warnings(opencdb::conformance::RequirementsClass::Metadata)
            .is_empty()
    );
}

/// Unsupported containers abort; corrupt recognized containers are Metadata findings.
#[cfg(feature = "gpkg-metadata")]
#[test]
fn gpkg_binding_validation_distinguishes_corrupt_and_unsupported_containers() {
    use opencdb::metadata::ResourceMetadata;
    let tmp = tempfile::tempdir().unwrap();
    let profile = support::GpkgProfile::default();
    let store = CdbDatastore::create(tmp.path(), &profile, support::seed()).unwrap();
    let path = store
        .write_resource_metadata(
            "/Tiles/metadata/Roads.gpkg",
            &ResourceMetadata::new("roads", "Roads", "Network"),
        )
        .unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE payload (id INTEGER)")
        .unwrap();
    db.close().unwrap();
    assert!(matches!(
        store.validate(&profile),
        Err(CdbError::Metadata(
            MetadataError::UnsupportedContainer { .. }
        ))
    ));
    std::fs::write(path, b"corrupt container").unwrap();
    let report = store.validate(&profile).unwrap();
    assert!(
        report
            .violations(opencdb::conformance::RequirementsClass::Metadata)
            .iter()
            .any(|v| v.code() == "/req/core/metadata-encoding")
    );
}
