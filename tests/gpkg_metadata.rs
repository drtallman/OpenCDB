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
