//! End-to-end datastore round-trips through the public [`rusty_cdb`] facade:
//! create → write → reopen → read-back equality. At Phase 14a the round-tripped
//! surface is the global metadata record (Metadata1/Metadata5), the storage CRS
//! (CRS5, including a dynamic-reference-frame coordinate epoch, CRS7), and
//! resource-metadata records (§7.9.4.2). Tile and coverage payload
//! round-tripping arrives with Phase 14b, once those requirements classes
//! exist. Every read and write flows through the facade only, so these tests
//! also prove the datastore is self-describing on disk — no state hides in the
//! [`rusty_cdb::CdbDatastore`] handle.

use std::fs;

use rusty_cdb::crs::{CrsViolation, CrsWarning, Epoch, StorageCrs};
use rusty_cdb::links::Link;
use rusty_cdb::metadata::{MetadataEncoding, MetadataStandard, ResourceMetadata, UnitOfMeasure};
use rusty_cdb::naming::StyleGuide;
use rusty_cdb::profiles::{StorageTechnology, simulation::WGS84_2D_WKT};
use rusty_cdb::{
    ApplicationProfile, CdbDatastore, CdbWarning, DatastoreSeed, RequirementsClass,
    SimulationProfile,
};
use tempfile::TempDir;

/// Builds a populated datastore under `profile`: root metadata and storage CRS
/// (via `create`), a dummy data file at `/Tiles/RoadNetwork.gpkg`, and a valid
/// resource-metadata record at the profile's convention path (`.json` or
/// `.xml`). Returns the temp-dir guard, the datastore, and the resource
/// record's logical path.
fn build_datastore(profile: &SimulationProfile) -> (TempDir, CdbDatastore, String) {
    let tmp = tempfile::tempdir().unwrap();
    let store = CdbDatastore::create(
        tmp.path(),
        profile,
        DatastoreSeed::new(
            "doi:cdb.demo",
            "Demo",
            "A demonstration datastore",
            "ops@example.com",
        ),
    )
    .unwrap();

    let data = store.resolve("/Tiles/RoadNetwork.gpkg").unwrap();
    fs::create_dir_all(data.parent().unwrap()).unwrap();
    fs::write(&data, b"dummy gpkg bytes").unwrap();

    let resource_path = profile
        .resource_metadata_path("/Tiles/RoadNetwork.gpkg")
        .unwrap();
    let mut record = ResourceMetadata::new("roads", "RoadNetwork", "The road network");
    record.associations = vec![Link::new("https://example.com/tiles", "item").unwrap()];
    store
        .write_resource_metadata(&resource_path, &record)
        .unwrap();

    (tmp, store, resource_path)
}

/// The shared round-trip skeleton for both metadata encodings: create, add a
/// license to the global record (a same-encoding rewrite), then reopen the
/// datastore from a fresh handle and confirm the global metadata, storage CRS
/// (WGS-84, no epoch), and resource-metadata record all read back equal — and
/// the reopened datastore still validates conformant.
fn assert_datastore_round_trips(profile: SimulationProfile) {
    let (_tmp, store, resource_path) = build_datastore(&profile);

    let mut global = store.global_metadata().unwrap();
    global.license = Some("CC-BY-4.0".to_owned());
    store.write_global_metadata(&global).unwrap();

    let resource = store.read_resource_metadata(&resource_path).unwrap();

    let reopened = CdbDatastore::open(store.root()).unwrap();
    assert_eq!(reopened.global_metadata().unwrap(), global);

    let crs = reopened.storage_crs().unwrap();
    assert_eq!(crs, StorageCrs::from_wkt(WGS84_2D_WKT).unwrap());
    assert_eq!(crs.epoch(), None);

    assert_eq!(
        reopened.read_resource_metadata(&resource_path).unwrap(),
        resource
    );

    let report = reopened.validate(&profile).unwrap();
    assert!(report.is_conformant(), "{report}");
}

/// Requirements Metadata1/Metadata5, CRS5, §7.9.4.2: a JSON datastore
/// round-trips its global metadata (with a later-added license), storage CRS
/// (WGS-84, no epoch), and a resource-metadata record across a fresh `open`,
/// and stays conformant.
#[test]
fn roundtrip_json_datastore() {
    assert_datastore_round_trips(SimulationProfile::json());
}

/// Requirements Metadata1/Metadata5, CRS5, §7.9.4.2: the same round-trip holds
/// for an XML datastore — the metadata record and resource record are written
/// and read as XML, and the storage CRS and conformance are unaffected by the
/// encoding choice.
#[test]
fn roundtrip_xml_datastore() {
    assert_datastore_round_trips(SimulationProfile::xml());
}

/// Requirement CRS7 (§7.3.1.6, `/req/core/crs/crsEpoch`): a profile pinning a
/// *dynamic* reference frame with a coordinate epoch round-trips the epoch
/// through the datastore's `COORDINATEMETADATA[...,EPOCH[...]]` record.
/// Validated against that same profile the datastore is conformant, yet the
/// CRS class carries the `NotWgs84` *warning* (a SHOULD finding) without
/// failing (a SHALL check) — the crate's SHALL/SHOULD split. The wrapper
/// profile lives in this test module, proving `ApplicationProfile` is
/// implementable outside `profiles`.
#[test]
fn roundtrip_storage_crs_with_epoch() {
    // A dynamic geographic CRS (ITRF2014-like) with no in-WKT frame epoch, so
    // the profile supplies the datastore epoch explicitly. It carries no
    // EPSG:4326/4979 identity, so it earns the WGS-84 recommendation warning.
    const DYNAMIC_WKT: &str = r#"GEOGCRS["ITRF2014",
  DYNAMIC[],
  DATUM["International Terrestrial Reference Frame 2014",
    ELLIPSOID["GRS 1980",6378137,298.257222101,LENGTHUNIT["metre",1.0]]],
  CS[ellipsoidal,2],
    AXIS["latitude",north,ORDER[1]],
    AXIS["longitude",east,ORDER[2]],
    ANGLEUNIT["degree",0.0174532925199433]]"#;

    struct DynamicWgs84Profile(SimulationProfile);
    impl ApplicationProfile for DynamicWgs84Profile {
        fn name(&self) -> &str {
            "simulation-dynamic"
        }
        fn style_guide(&self) -> StyleGuide {
            self.0.style_guide()
        }
        fn storage_crs(&self) -> Result<StorageCrs, CrsViolation> {
            StorageCrs::new(DYNAMIC_WKT, Some(Epoch::new(2017.53)?))
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

    let tmp = tempfile::tempdir().unwrap();
    let profile = DynamicWgs84Profile(SimulationProfile::json());
    let store = CdbDatastore::create(
        tmp.path(),
        &profile,
        DatastoreSeed::new("id", "Title", "Description", "contact"),
    )
    .unwrap();

    // Reopen through the facade; the epoch must survive the round-trip.
    let reopened = CdbDatastore::open(store.root()).unwrap();
    let crs = reopened.storage_crs().unwrap();
    assert_eq!(crs.epoch(), Some(Epoch::new(2017.53).unwrap()));

    // Validated against its own pinning profile: conformant, CRS passes, and
    // the NotWgs84 SHOULD-warning is present without failing the class.
    let report = reopened.validate(&profile).unwrap();
    assert!(report.is_conformant(), "{report}");
    assert!(report.class_passed(RequirementsClass::Crs), "{report}");
    assert!(
        report
            .warnings(RequirementsClass::Crs)
            .iter()
            .any(|warning| matches!(warning, CdbWarning::Crs(CrsWarning::NotWgs84 { .. }))),
        "{report}"
    );
}

/// Requirements File2/File3 (§7.5.3/§7.5.4): after the creating
/// `CdbDatastore` value is dropped, a completely fresh `open` of the same root
/// reads the global metadata and resource metadata back equal to the pre-drop
/// values — every read flows through the facade only, so persistence is proven
/// to live on disk, not in the handle.
#[test]
fn roundtrip_survives_reopen_via_facade_only() {
    let profile = SimulationProfile::json();
    let (_tmp, store, resource_path) = build_datastore(&profile);

    let global_before = store.global_metadata().unwrap();
    let resource_before = store.read_resource_metadata(&resource_path).unwrap();
    let root = store.root().to_path_buf();
    drop(store);

    let reopened = CdbDatastore::open(root).unwrap();
    assert_eq!(reopened.global_metadata().unwrap(), global_before);
    assert_eq!(
        reopened.read_resource_metadata(&resource_path).unwrap(),
        resource_before
    );
}
