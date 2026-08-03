//! Recommendations Coverages7/Coverages8 (§7.2.7–§7.2.8): a tiled coverage
//! follows the abstract tiling module and exactly one tiling extension.
//! This is the integration reserved by TDD_PLAN's Phase 8 row
//! ("integration test once Phase 9 lands"): the tiling scheme, tileset
//! metadata, coverage-instance validation, and grid addressing composed on
//! a real datastore.

use rusty_cdb::coverage::{DomainSet, validate_coverage_instance};
use rusty_cdb::metadata::ResourceMetadata;
use rusty_cdb::tiling::{Cdb1GlobalGrid, Lod, TilingScheme, validate_tileset_metadata};
use rusty_cdb::{CdbDatastore, DatastoreSeed, SimulationProfile};

/// §7.2.7 Rec Coverages7 /req/core/coverage-tiling-abstract and §7.2.8
/// Rec Coverages8 /req/core/coverage-tiling-extension.
#[test]
fn rec_core_coverage_tiling_abstract_and_extension() {
    let tmp = tempfile::tempdir().unwrap();
    let profile = SimulationProfile::json();
    let seed = DatastoreSeed::new(
        "TiledStore",
        "Tiled Store",
        "Tiled coverage demo",
        "ops@example.com",
    );
    let datastore = CdbDatastore::create(tmp.path(), &profile, seed).unwrap();

    // Tiling8: record the scheme in the global metadata.
    let mut global = datastore.global_metadata().unwrap();
    global.tiling_scheme = Some(TilingScheme::cdb1_global_grid());
    datastore.write_global_metadata(&global).unwrap();

    // The scheme validates against the datastore CRS with zero warnings.
    let crs = datastore.storage_crs().unwrap();
    let reread = datastore.global_metadata().unwrap();
    let scheme = TilingScheme::require(&reread).unwrap();
    scheme.validate(&crs).unwrap();
    assert!(scheme.warnings().is_empty());

    // A tiled elevation coverage: tileset-grade resource metadata with a
    // domainSet (Coverages5/6 + Tiling9/10).
    let mut record = ResourceMetadata::new("Elevation", "Terrain Elevation", "Gridded DEM tileset");
    record.keywords = vec!["elevation".into(), "terrain".into(), "dem".into()];
    record.domain_set = Some(DomainSet::new("m"));
    validate_tileset_metadata(&record).unwrap();
    datastore
        .write_resource_metadata("/Tiles/metadata/Elevation.json", &record)
        .unwrap();

    // Coverage-instance validation passes with the datastore CRS.
    let identity = crs.authority();
    assert!(
        identity.is_some(),
        "simulation storage CRS must carry an authority identity"
    );
    let back = datastore
        .read_resource_metadata("/Tiles/metadata/Elevation.json")
        .unwrap();
    validate_coverage_instance(Some(&back), identity.as_ref(), identity.as_ref()).unwrap();

    // Grid addressing over the coverage's area (one geocell at LoD 0 and
    // its quad at LoD 1).
    let t0 = Cdb1GlobalGrid::tile_at(37.25, -122.75, Lod::new(0).unwrap()).unwrap();
    let b0 = Cdb1GlobalGrid::tile_extent(t0);
    assert!(b0.west <= -122.75 && -122.75 < b0.east);
    assert_eq!(Cdb1GlobalGrid::children(t0).len(), 4);

    // The datastore still validates conformant through the facade.
    let report = datastore.validate(&profile).unwrap();
    assert!(report.is_conformant());
}
