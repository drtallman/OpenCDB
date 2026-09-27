//! Run with `cargo run --example gpkg_metadata --features gpkg-metadata -- DESTINATION_PARENT`.
use opencdb::crs::{CrsViolation, StorageCrs};
use opencdb::metadata::{MetadataEncoding, MetadataStandard, ResourceMetadata, UnitOfMeasure};
use opencdb::naming::StyleGuide;
use opencdb::profiles::StorageTechnology;
use opencdb::{
    ApplicationProfile, CdbDatastore, DatastoreSeed, PendingCollection, RequirementsClass,
    SimulationProfile,
};
use std::path::PathBuf;

// Matches the descriptor in cdb-lint/README.md: no tiling or attribution model.
struct GpkgProfile;
impl ApplicationProfile for GpkgProfile {
    fn name(&self) -> &str {
        "opencdb-gpkg-v1"
    }
    fn style_guide(&self) -> StyleGuide {
        SimulationProfile::json().style_guide()
    }
    fn storage_crs(&self) -> Result<StorageCrs, CrsViolation> {
        SimulationProfile::json().storage_crs()
    }
    fn metadata_standard(&self) -> MetadataStandard {
        MetadataStandard::Dcat
    }
    fn metadata_encoding(&self) -> MetadataEncoding {
        MetadataEncoding::Gpkg
    }
    fn uom(&self) -> UnitOfMeasure {
        UnitOfMeasure::Meters
    }
    fn storage_technology(&self) -> StorageTechnology {
        StorageTechnology::FileSystem
    }
    fn conformance_classes(&self) -> Vec<RequirementsClass> {
        RequirementsClass::ALL.to_vec()
    }
    fn is_resource_metadata(&self, path: &str) -> bool {
        SimulationProfile::json().is_resource_metadata(path)
    }
    fn known_extensions(&self) -> Vec<String> {
        vec!["wkt".into()]
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let destination = match args.next() {
        Some(path) if !path.is_empty() && args.next().is_none() => PathBuf::from(path),
        _ => {
            eprintln!("usage: gpkg_metadata DESTINATION_PARENT");
            std::process::exit(2);
        }
    };
    if destination.join("cdb").try_exists()? {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "destination already contains cdb; choose a fresh parent",
        )
        .into());
    }
    std::fs::create_dir_all(&destination)?;
    let profile = GpkgProfile;
    let store = CdbDatastore::create(
        &destination,
        &profile,
        DatastoreSeed::new(
            "gpkg-example",
            "GeoPackage example",
            "Metadata-only containers",
            "ops@example.com",
        ),
    )?;
    let record = "/Tiles/metadata/Roads.gpkg";
    store.write_resource_metadata(record, &ResourceMetadata::new("roads", "Roads", "Network"))?;
    store.apply_collection(
        PendingCollection::new()
            .description("initial payload")
            .create("/Tiles/Roads.gpkg", b"opaque example payload".to_vec())
            .for_record(record),
    )?;
    let reopened = CdbDatastore::open(store.root())?;
    println!("{}", reopened.validate(&profile)?);
    Ok(())
}
