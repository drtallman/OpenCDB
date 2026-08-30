//! Integration: versioning collections on a real datastore (Requirements
//! V1–V6, §7.14) — apply, journal, state timeline, byte-level rollback,
//! and conformance (`validate()` stays green through it all), for both
//! metadata encodings.

use chrono::{DateTime, Utc};
use rusty_cdb::{
    CdbDatastore, ChangeAction, CollectionId, DatastoreSeed, PendingCollection, SimulationProfile,
};

fn ts(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}

/// V1–V6 end to end on a JSON datastore: create/replace/delete/state
/// collections with V3 timestamps, as-of state views, single-collection
/// rollback, whole-datastore rollback_to, byte equality of restored
/// content, and a conformant validate() after everything.
#[test]
fn req_core_versioning_full_roundtrip_json() {
    let tmp = tempfile::tempdir().unwrap();
    let profile = SimulationProfile::json();
    let seed = DatastoreSeed::new("doi:cdb.v12", "Versioned", "Versioning roundtrip", "ops");
    let store = CdbDatastore::create(tmp.path(), &profile, seed).unwrap();

    let original: Vec<u8> = vec![0, 159, 146, 150, 255, 1, 2, 3];
    let replacement: Vec<u8> = vec![255, 0, 128, 7];
    store
        .write_resource_metadata(
            "/Tiles/metadata/RoadNetwork.json",
            &rusty_cdb::metadata::ResourceMetadata::new("RoadNetwork", "Road Network", "Roads"),
        )
        .unwrap();

    store
        .apply_collection_at(
            PendingCollection::new()
                .description("initial content")
                .create("/Tiles/RoadNetwork.gpkg", original.clone())
                .for_record("/Tiles/metadata/RoadNetwork.json")
                .create("/Tiles/Buildings.gpkg", *b"buildings-v1"),
            ts("2026-08-30T10:00:00Z"),
        )
        .unwrap();
    store
        .apply_collection_at(
            PendingCollection::new()
                .description("roadworks")
                .replace("/Tiles/RoadNetwork.gpkg", replacement.clone())
                .for_record("/Tiles/metadata/RoadNetwork.json")
                .delete("/Tiles/Buildings.gpkg"),
            ts("2026-08-30T11:00:00Z"),
        )
        .unwrap();
    let closed = store
        .apply_collection_at(
            PendingCollection::new().set_state("/Tiles/RoadNetwork.gpkg", "closed"),
            ts("2026-08-30T12:00:00Z"),
        )
        .unwrap();
    assert_eq!(closed.changes[0].action, ChangeAction::StateSet);

    // V3: the last apply's instant is on the global update element and the
    // linked record's updated element from the roadworks collection.
    let global = store.global_metadata().unwrap();
    assert_eq!(global.update, Some(ts("2026-08-30T12:00:00Z")));
    let record = store
        .read_resource_metadata("/Tiles/metadata/RoadNetwork.json")
        .unwrap();
    assert_eq!(record.updated, Some(ts("2026-08-30T11:00:00Z")));

    // V6 as-of views across the timeline.
    assert_eq!(
        store
            .state_of("/Tiles/RoadNetwork.gpkg", ts("2026-08-30T11:30:00Z"))
            .unwrap(),
        None
    );
    assert_eq!(
        store
            .state_of("/Tiles/RoadNetwork.gpkg", ts("2026-08-30T12:30:00Z"))
            .unwrap(),
        Some("closed".to_owned())
    );

    // Roll back the state collection alone, then the whole datastore to v1.
    let state_rollback = store
        .rollback_collection_at(
            CollectionId::parse("v000003").unwrap(),
            ts("2026-08-30T13:00:00Z"),
        )
        .unwrap();
    assert_eq!(state_rollback.changes[0].action, ChangeAction::StateCleared);
    assert_eq!(
        store
            .state_of("/Tiles/RoadNetwork.gpkg", ts("2026-08-30T13:30:00Z"))
            .unwrap(),
        None
    );
    let inverses = store
        .rollback_to_at(
            CollectionId::parse("v000001").unwrap(),
            ts("2026-08-30T14:00:00Z"),
        )
        .unwrap();
    assert_eq!(
        inverses.len(),
        3,
        "state rollback + roadworks + its inverse"
    );

    // Byte equality of the restored point-in-time content.
    let road = store.resolve("/Tiles/RoadNetwork.gpkg").unwrap();
    assert_eq!(std::fs::read(road).unwrap(), original);
    let buildings = store.resolve("/Tiles/Buildings.gpkg").unwrap();
    assert_eq!(std::fs::read(buildings).unwrap(), b"buildings-v1");

    // The journal grew monotonically and the datastore stays conformant.
    assert_eq!(store.versions().unwrap().len(), 7);
    let report = store.validate(&profile).unwrap();
    assert!(report.is_conformant(), "report: {report}");
}

/// The declared-encoding journal on an XML datastore: manifests persist as
/// `manifest.xml`, round-trip through `versions()`, roll back, and the
/// datastore stays conformant.
#[test]
fn req_core_versioning_xml_datastore_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let profile = SimulationProfile::xml();
    let seed = DatastoreSeed::new("doi:cdb.v12x", "Versioned", "XML versioning", "ops");
    let store = CdbDatastore::create(tmp.path(), &profile, seed).unwrap();

    store
        .apply_collection_at(
            PendingCollection::new().create("/Tiles/RoadNetwork.gpkg", *b"road-v1"),
            ts("2026-08-30T10:00:00Z"),
        )
        .unwrap();
    store
        .apply_collection_at(
            PendingCollection::new().replace("/Tiles/RoadNetwork.gpkg", *b"road-v2"),
            ts("2026-08-30T11:00:00Z"),
        )
        .unwrap();
    assert!(store.root().join("versions/v000002/manifest.xml").is_file());
    let journal = store.versions().unwrap();
    assert_eq!(journal.len(), 2);
    assert_eq!(journal[1].changes[0].action, ChangeAction::Replaced);

    store
        .rollback_collection_at(
            CollectionId::parse("v000002").unwrap(),
            ts("2026-08-30T12:00:00Z"),
        )
        .unwrap();
    let road = store.resolve("/Tiles/RoadNetwork.gpkg").unwrap();
    assert_eq!(std::fs::read(road).unwrap(), b"road-v1");
    let report = store.validate(&profile).unwrap();
    assert!(report.is_conformant(), "report: {report}");
}
