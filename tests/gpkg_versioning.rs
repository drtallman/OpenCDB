#[cfg_attr(not(feature = "gpkg-metadata"), allow(dead_code))]
#[path = "support/gpkg.rs"]
mod support;

#[cfg(not(feature = "gpkg-metadata"))]
use opencdb::metadata::MetadataEncoding;
use opencdb::metadata::MetadataError;
use opencdb::{CdbDatastore, CdbError, PendingCollection};
use std::path::{Path, PathBuf};

// Includes directory entries and file bytes, so a refusal cannot leave an empty journal.
fn snapshot(root: &Path) -> Vec<(PathBuf, Option<Vec<u8>>)> {
    fn walk(root: &Path, path: &Path, entries: &mut Vec<(PathBuf, Option<Vec<u8>>)>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                entries.push((path.strip_prefix(root).unwrap().to_owned(), None));
                walk(root, &path, entries);
            } else {
                entries.push((
                    path.strip_prefix(root).unwrap().to_owned(),
                    Some(std::fs::read(path).unwrap()),
                ));
            }
        }
    }
    let mut entries = Vec::new();
    walk(root, root, &mut entries);
    entries.sort();
    entries
}

/// An unavailable encoding is refused before archives, payloads or timestamps change.
#[cfg(not(feature = "gpkg-metadata"))]
#[test]
fn gpkg_binding_apply_without_feature_refuses_before_mutation() {
    let tmp = tempfile::tempdir().unwrap();
    let store = CdbDatastore::create(
        tmp.path(),
        &opencdb::SimulationProfile::json(),
        support::seed(),
    )
    .unwrap();
    let path = store.root().join("global_metadata/global_metadata.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["metadataEncoding"] = serde_json::json!("gpkg");
    std::fs::write(path, value.to_string()).unwrap();
    let before = snapshot(store.root());
    assert!(matches!(
        store.apply_collection_at(
            PendingCollection::new().create("/Tiles/Roads.gpkg", b"payload".to_vec()),
            support::ts("2026-09-27T12:00:00Z")
        ),
        Err(CdbError::Metadata(MetadataError::UnsupportedEncoding(
            MetadataEncoding::Gpkg
        )))
    ));
    assert!(snapshot(store.root()) == before, "refusal changed the tree");
}

#[cfg(feature = "gpkg-metadata")]
mod enabled {
    use super::*;
    use opencdb::conformance::RequirementsClass;
    use opencdb::metadata::ResourceMetadata;

    fn store() -> (tempfile::TempDir, CdbDatastore) {
        let tmp = tempfile::tempdir().unwrap();
        let store = CdbDatastore::create(
            tmp.path(),
            &support::GpkgProfile::default(),
            support::seed(),
        )
        .unwrap();
        store
            .write_resource_metadata(
                "/Tiles/metadata/Roads.gpkg",
                &ResourceMetadata::new("r", "Roads", "Network"),
            )
            .unwrap();
        (tmp, store)
    }

    /// CDB V1–V6, especially V3-B/C: timestamps and journal survive reopen/rollback.
    #[test]
    fn req_core_versioning_gpkg_apply_reopen_and_rollback() {
        let (_tmp, store) = store();
        let record = "/Tiles/metadata/Roads.gpkg";
        let t1 = support::ts("2026-09-27T12:00:00Z");
        let t2 = support::ts("2026-09-27T13:00:00Z");
        let t3 = support::ts("2026-09-27T14:00:00Z");
        store
            .apply_collection_at(
                PendingCollection::new()
                    .create("/Tiles/Roads.gpkg", b"old".to_vec())
                    .for_record(record),
                t1,
            )
            .unwrap();
        let changed = store
            .apply_collection_at(
                PendingCollection::new()
                    .replace("/Tiles/Roads.gpkg", b"new".to_vec())
                    .for_record(record),
                t2,
            )
            .unwrap();
        let reopened = CdbDatastore::open(store.root()).unwrap();
        assert_eq!(reopened.versions().unwrap().len(), 2);
        assert_eq!(reopened.global_metadata().unwrap().update, Some(t2));
        assert_eq!(
            reopened.read_resource_metadata(record).unwrap().updated,
            Some(t2)
        );
        reopened.rollback_collection_at(changed.id, t3).unwrap();
        assert_eq!(
            std::fs::read(reopened.resolve("/Tiles/Roads.gpkg").unwrap()).unwrap(),
            b"old"
        );
        assert_eq!(reopened.global_metadata().unwrap().update, Some(t3));
        assert_eq!(
            reopened.read_resource_metadata(record).unwrap().updated,
            Some(t3)
        );
        let report = reopened.validate(&support::GpkgProfile::default()).unwrap();
        assert!(report.is_conformant(), "{report}");
        assert_eq!(
            report.warnings(RequirementsClass::Metadata).len(),
            5,
            "global, record, three manifests"
        );
    }

    /// V3-C: two changed assets may share one managed resource metadata document.
    #[test]
    fn req_core_versioning_gpkg_shared_record_updates_once() {
        let (_tmp, store) = store();
        let record = "/Tiles/metadata/Roads.gpkg";
        let at = support::ts("2026-09-27T12:00:00Z");
        let manifest = store
            .apply_collection_at(
                PendingCollection::new()
                    .create("/Tiles/Roads.gpkg", b"one".to_vec())
                    .for_record(record)
                    .create("/Tiles/Bridges.gpkg", b"two".to_vec())
                    .for_record(record),
                at,
            )
            .unwrap();
        assert_eq!(manifest.changes.len(), 2);
        assert_eq!(
            store.read_resource_metadata(record).unwrap().updated,
            Some(at)
        );
        let report = store.validate(&support::GpkgProfile::default()).unwrap();
        assert_eq!(report.warnings(RequirementsClass::Metadata).len(), 3);
        assert_eq!(
            report
                .warnings(RequirementsClass::Metadata)
                .iter()
                .filter(|w| w.to_string().contains(record))
                .count(),
            1
        );
    }

    /// The single-document binding refuses managed-record/payload conflicts before I/O.
    #[test]
    fn gpkg_binding_apply_rejects_overlapping_record_and_asset_before_mutation() {
        let (_tmp, store) = store();
        let record = "/Tiles/metadata/Roads.gpkg";
        let before = snapshot(store.root());
        let result = store.apply_collection_at(
            PendingCollection::new()
                .create("/Tiles/Roads.gpkg", b"payload".to_vec())
                .for_record(record)
                .replace(record, b"opaque replacement".to_vec()),
            support::ts("2026-09-27T12:00:00Z"),
        );
        assert!(matches!(
            result,
            Err(CdbError::Metadata(
                MetadataError::UnsupportedContainer { .. }
            ))
        ));
        assert!(snapshot(store.root()) == before, "refusal changed the tree");
    }

    /// Binding overlap guards compare filesystem targets even when case aliases exist.
    #[test]
    fn gpkg_binding_apply_case_alias_overlap_preserves_tree() {
        let (_tmp, store) = store();
        let record = "/Tiles/metadata/Roads.gpkg";
        let alias = "/Tiles/metadata/roads.gpkg";
        if !store.resolve(alias).unwrap().exists() {
            // Case-sensitive filesystems do not alias these two names.
            return;
        }
        let before = snapshot(store.root());
        let result = store.apply_collection_at(
            PendingCollection::new()
                .replace(alias, b"requested opaque payload".to_vec())
                .for_record(record),
            support::ts("2026-09-27T12:00:00Z"),
        );
        assert!(matches!(
            result,
            Err(CdbError::Metadata(
                MetadataError::UnsupportedContainer { .. }
            ))
        ));
        assert!(snapshot(store.root()) == before, "refusal changed the tree");
    }

    /// Binding overlap checks also follow symlinks that resolve to a managed record.
    #[cfg(unix)]
    #[test]
    fn gpkg_binding_apply_symlink_overlap_preserves_tree() {
        let (_tmp, store) = store();
        let record = "/Tiles/metadata/Roads.gpkg";
        let alias = "/Tiles/metadata/RoadsAlias.gpkg";
        std::os::unix::fs::symlink("Roads.gpkg", store.resolve(alias).unwrap()).unwrap();
        let before = snapshot(store.root());
        let result = store.apply_collection_at(
            PendingCollection::new()
                .replace(alias, b"requested opaque payload".to_vec())
                .for_record(record),
            support::ts("2026-09-27T12:00:00Z"),
        );
        assert!(matches!(
            result,
            Err(CdbError::Metadata(
                MetadataError::UnsupportedContainer { .. }
            ))
        ));
        assert!(snapshot(store.root()) == before, "refusal changed the tree");
        assert!(
            std::fs::symlink_metadata(store.resolve(alias).unwrap())
                .unwrap()
                .is_symlink()
        );
    }

    /// V3-C: publishing a shared symlinked record updates the target and preserves the link.
    #[cfg(unix)]
    #[test]
    fn req_core_versioning_gpkg_shared_symlink_record_updates_target() {
        let (_tmp, store) = store();
        let record = "/Tiles/metadata/Roads.gpkg";
        let alias = "/Tiles/metadata/RoadsAlias.gpkg";
        std::os::unix::fs::symlink("Roads.gpkg", store.resolve(alias).unwrap()).unwrap();
        let at = support::ts("2026-09-27T12:00:00Z");
        store
            .apply_collection_at(
                PendingCollection::new()
                    .create("/Tiles/Roads.gpkg", b"one".to_vec())
                    .for_record(alias)
                    .create("/Tiles/Bridges.gpkg", b"two".to_vec())
                    .for_record(record),
                at,
            )
            .unwrap();
        assert!(
            std::fs::symlink_metadata(store.resolve(alias).unwrap())
                .unwrap()
                .is_symlink()
        );
        assert_eq!(
            store.read_resource_metadata(record).unwrap().updated,
            Some(at)
        );
        assert_eq!(
            store.read_resource_metadata(alias).unwrap(),
            store.read_resource_metadata(record).unwrap()
        );
    }

    /// Malformed linked metadata must fail before any payload or archive mutation.
    #[test]
    fn gpkg_binding_apply_bad_linked_metadata_preserves_entire_tree() {
        let (_tmp, store) = store();
        let record = "/Tiles/metadata/Roads.gpkg";
        std::fs::write(store.resolve(record).unwrap(), b"bad container").unwrap();
        let before = snapshot(store.root());
        let result = store.apply_collection_at(
            PendingCollection::new()
                .create("/Tiles/Roads.gpkg", b"payload".to_vec())
                .for_record(record),
            support::ts("2026-09-27T12:00:00Z"),
        );
        assert!(matches!(
            result,
            Err(CdbError::Metadata(MetadataError::Serialization(_)))
        ));
        assert!(snapshot(store.root()) == before, "refusal changed the tree");
    }
    /// V4/V6 and inverse collections: CRUD, state timelines and rollback_to restore bytes.
    #[test]
    fn req_core_versioning_gpkg_crud_state_and_rollback_to() {
        let (_tmp, store) = store();
        let record = "/Tiles/metadata/Roads.gpkg";
        let road = "/Tiles/Roads.gpkg";
        let building = "/Tiles/Buildings.gpkg";
        let original = vec![0, 255, 128, 7];
        let first = store
            .apply_collection_at(
                PendingCollection::new()
                    .create(road, original.clone())
                    .for_record(record)
                    .create(building, b"building".to_vec()),
                support::ts("2026-09-27T12:00:00Z"),
            )
            .unwrap();
        store
            .apply_collection_at(
                PendingCollection::new()
                    .replace(road, b"changed".to_vec())
                    .for_record(record)
                    .delete(building),
                support::ts("2026-09-27T13:00:00Z"),
            )
            .unwrap();
        store
            .apply_collection_at(
                PendingCollection::new()
                    .set_state(road, "closed")
                    .for_record(record),
                support::ts("2026-09-27T14:00:00Z"),
            )
            .unwrap();
        store
            .apply_collection_at(
                PendingCollection::new()
                    .clear_state(road)
                    .for_record(record),
                support::ts("2026-09-27T15:00:00Z"),
            )
            .unwrap();
        assert_eq!(
            store
                .state_of(road, support::ts("2026-09-27T13:30:00Z"))
                .unwrap(),
            None
        );
        assert_eq!(
            store
                .state_of(road, support::ts("2026-09-27T14:30:00Z"))
                .unwrap(),
            Some("closed".to_owned())
        );
        assert_eq!(
            store
                .state_of(road, support::ts("2026-09-27T15:30:00Z"))
                .unwrap(),
            None
        );
        let rollback_time = support::ts("2026-09-27T16:00:00Z");
        let inverses = store.rollback_to_at(first.id, rollback_time).unwrap();
        assert_eq!(inverses.len(), 3);
        assert_eq!(
            std::fs::read(store.resolve(road).unwrap()).unwrap(),
            original
        );
        assert_eq!(
            std::fs::read(store.resolve(building).unwrap()).unwrap(),
            b"building"
        );
        assert_eq!(store.state_of(road, rollback_time).unwrap(), None);
        assert_eq!(
            store.read_resource_metadata(record).unwrap().updated,
            Some(rollback_time)
        );
        let reopened = CdbDatastore::open(store.root()).unwrap();
        assert_eq!(reopened.versions().unwrap().len(), 7);
        let report = reopened.validate(&support::GpkgProfile::default()).unwrap();
        assert!(report.is_conformant(), "{report}");
        assert_eq!(report.warnings(RequirementsClass::Metadata).len(), 9);
    }

    fn committed() -> (tempfile::TempDir, CdbDatastore, PathBuf) {
        let (tmp, store) = store();
        store
            .apply_collection_at(
                PendingCollection::new().create("/Tiles/Roads.gpkg", b"road".to_vec()),
                support::ts("2026-09-27T12:00:00Z"),
            )
            .unwrap();
        let manifest = store.root().join("versions/v000001/manifest.gpkg");
        (tmp, store, manifest)
    }

    /// V2: journal gaps and malformed bodies are Versioning; container corruption is Metadata.
    #[test]
    fn req_core_versioning_gpkg_manifest_failure_classification() {
        for mutation in ["body", "container", "unsupported", "gap"] {
            let (_tmp, store, manifest) = committed();
            if mutation == "container" {
                std::fs::write(&manifest, b"corrupt container").unwrap();
            } else if mutation == "gap" {
                store
                    .apply_collection_at(
                        PendingCollection::new().replace("/Tiles/Roads.gpkg", b"new".to_vec()),
                        support::ts("2026-09-27T13:00:00Z"),
                    )
                    .unwrap();
                std::fs::remove_file(&manifest).unwrap();
            } else {
                let db = rusqlite::Connection::open(&manifest).unwrap();
                db.execute_batch(if mutation == "body" {
                    "UPDATE gpkg_metadata SET metadata='not JSON'"
                } else {
                    "CREATE TABLE payload (id INTEGER)"
                })
                .unwrap();
                db.close().unwrap();
            }
            let before = snapshot(store.root());
            let result = store.validate(&support::GpkgProfile::default());
            if mutation == "unsupported" {
                assert!(matches!(
                    result,
                    Err(CdbError::Metadata(
                        MetadataError::UnsupportedContainer { .. }
                    ))
                ));
            } else {
                let report = result.unwrap();
                let class = if mutation == "container" {
                    RequirementsClass::Metadata
                } else {
                    RequirementsClass::Versioning
                };
                assert!(!report.violations(class).is_empty(), "{mutation}: {report}");
            }
            assert!(
                snapshot(store.root()) == before,
                "inspection changed the tree"
            );
        }
    }

    /// Only committed manifests count; archived payloads and abandoned directories are opaque.
    #[test]
    fn gpkg_binding_journal_warnings_ignore_archive_and_uncommitted_files() {
        let (_tmp, store, _manifest) = committed();
        let archive = store.root().join("versions/v000001/archive/Tiles/metadata");
        std::fs::create_dir_all(&archive).unwrap();
        std::fs::write(archive.join("Opaque.gpkg"), b"opaque archived payload").unwrap();
        std::fs::create_dir_all(store.root().join("versions/v000002")).unwrap();
        let report = store.validate(&support::GpkgProfile::default()).unwrap();
        assert!(report.is_conformant(), "{report}");
        assert_eq!(report.warnings(RequirementsClass::Metadata).len(), 3);
        assert_eq!(store.versions().unwrap().len(), 1);
    }

    /// Preparation must not overwrite unsupported global content or mixed record encodings.
    #[test]
    fn gpkg_binding_apply_preparation_refusals_preserve_entire_tree() {
        for mutation in [
            "extra global content",
            "invalid linked body",
            "wrong record encoding",
        ] {
            let (_tmp, store) = store();
            let mut record = "/Tiles/metadata/Roads.gpkg";
            let target = if mutation == "extra global content" {
                store.root().join("global_metadata/global_metadata.gpkg")
            } else {
                store.resolve(record).unwrap()
            };
            if mutation == "wrong record encoding" {
                record = "/Tiles/metadata/Wrong.json";
                std::fs::write(
                    store.resolve(record).unwrap(),
                    ResourceMetadata::new("id", "Title", "Description")
                        .to_json_string()
                        .unwrap(),
                )
                .unwrap();
            } else {
                let db = rusqlite::Connection::open(&target).unwrap();
                db.execute_batch(if mutation == "extra global content" {
                    "CREATE TABLE payload (id INTEGER)"
                } else {
                    "UPDATE gpkg_metadata SET metadata='{}'"
                })
                .unwrap();
                db.close().unwrap();
            }
            let before = snapshot(store.root());
            let result = store.apply_collection_at(
                PendingCollection::new()
                    .create("/Tiles/Roads.gpkg", b"payload".to_vec())
                    .for_record(record),
                support::ts("2026-09-27T12:00:00Z"),
            );
            assert!(result.is_err(), "{mutation}");
            assert!(
                snapshot(store.root()) == before,
                "{mutation}: refusal changed the tree"
            );
        }
    }

    /// Present-but-inaccessible manifests are operational failures, never uncommitted entries.
    #[cfg(unix)]
    #[test]
    fn gpkg_binding_journal_manifest_stat_failure_is_operational() {
        let (_tmp, store, manifest) = committed();
        std::fs::remove_file(&manifest).unwrap();
        std::os::unix::fs::symlink("manifest.gpkg", &manifest).unwrap();
        assert!(matches!(
            store.versions(),
            Err(CdbError::Versioning(
                opencdb::versioning::VersioningError::Io(_)
            ))
        ));
    }
}
