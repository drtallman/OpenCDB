use opencdb::attribution::AttributeModel;
use opencdb::conformance::RequirementsClass;
use opencdb::crs::{CrsViolation, StorageCrs};
use opencdb::metadata::{MetadataEncoding, MetadataStandard, UnitOfMeasure};
use opencdb::naming::StyleGuide;
use opencdb::profiles::StorageTechnology;
use opencdb::{ApplicationProfile, SimulationProfile};

#[derive(Default)]
pub struct GpkgProfile {
    pub model: Option<AttributeModel>,
}
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
        SimulationProfile::json().known_extensions()
    }
    fn attribute_model(&self) -> Option<AttributeModel> {
        self.model.clone()
    }
}
pub fn seed() -> opencdb::DatastoreSeed {
    opencdb::DatastoreSeed::new("gpkg-test", "GeoPackage", "Metadata fixture", "ops")
}
#[allow(dead_code)] // Shared by several integration-test binaries.
pub fn ts(s: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .unwrap()
        .with_timezone(&chrono::Utc)
}
