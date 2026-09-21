//! The migration yardstick and its portable cdb-lint descriptor.
//!
//! Generated records use `NoMetadata`: a source schema declaration is
//! provenance, not evidence of schema translation. The profile's metre unit
//! is the global spatial unit (Metadata8), never an inferred payload unit.

use crate::Cdb1Error;
use opencdb::crs::{CrsViolation, StorageCrs};
use opencdb::metadata::{MetadataEncoding, MetadataStandard, UnitOfMeasure};
use opencdb::naming::{CaseRule, StyleGuide};
use opencdb::profiles::{ApplicationProfile, StorageTechnology};
use opencdb::{AttributeModel, RequirementsClass, TilingSchemeId};

/// Stable profile identity; compare full descriptors when comparing parameters.
pub const MIGRATION_PROFILE_NAME: &str = "cdb-migrator-v1";

// Exact compact WKT-2 from cdb-lint's documented profile descriptor.
const WGS84_2D_WKT: &str = r#"GEOGCRS["WGS 84",DATUM["World Geodetic System 1984",ELLIPSOID["WGS 84",6378137,298.257223563,LENGTHUNIT["metre",1.0]]],CS[ellipsoidal,2],AXIS["latitude",north,ORDER[1]],AXIS["longitude",east,ORDER[2]],ANGLEUNIT["degree",0.0174532925199433],ID["EPSG",4326]]"#;

/// Fixed JSON/NoMetadata migration profile with operator-known extensions.
///
/// Extensions use dotless names. They are sorted and deduplicated; collecting
/// valid payload extensions and including generated `wkt` is the planner's
/// responsibility. A metadata-standard override is deliberately unavailable.
#[derive(Debug, Clone)]
pub struct MigrationProfile {
    known_extensions: Vec<String>,
    attribute_model: Option<AttributeModel>,
}

impl MigrationProfile {
    /// Construct the fixed migration profile with its known extensions.
    pub fn new(mut known_extensions: Vec<String>) -> Self {
        known_extensions.sort();
        known_extensions.dedup();
        Self {
            known_extensions,
            attribute_model: None,
        }
    }

    /// Attach an explicit operator model, validated and canonicalized using the
    /// public model parser. The trait and descriptor then carry the same model;
    /// source XML never provides an implicit model.
    pub fn with_attribute_model(mut self, model: AttributeModel) -> Result<Self, Cdb1Error> {
        let content = model
            .to_json_string()
            .map_err(|error| Cdb1Error::Refused(format!("attribute_model: {error}")))?;
        let model = AttributeModel::from_json_str(&content)
            .map_err(|error| Cdb1Error::Refused(format!("attribute_model: {error}")))?;
        self.attribute_model = Some(model);
        Ok(self)
    }
}

impl ApplicationProfile for MigrationProfile {
    fn name(&self) -> &str {
        MIGRATION_PROFILE_NAME
    }
    fn style_guide(&self) -> StyleGuide {
        let mut guide = StyleGuide::new(CaseRule::SnakeCase, "en");
        guide.reserve_name("metadata");
        guide
    }
    fn storage_crs(&self) -> Result<StorageCrs, CrsViolation> {
        StorageCrs::from_wkt(WGS84_2D_WKT)
    }
    fn metadata_standard(&self) -> MetadataStandard {
        MetadataStandard::NoMetadata
    }
    fn metadata_encoding(&self) -> MetadataEncoding {
        MetadataEncoding::Json
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
    fn is_resource_metadata(&self, logical_path: &str) -> bool {
        let mut components = logical_path.rsplit('/');
        let file_name = components.next().unwrap_or_default();
        components
            .next()
            .is_some_and(|dir| dir.eq_ignore_ascii_case("metadata"))
            && opencdb::naming::split_extension(file_name)
                .1
                .is_some_and(|extension| {
                    ["json", "xml", "gpkg"]
                        .iter()
                        .any(|known| known.eq_ignore_ascii_case(extension))
                })
    }
    fn tiling_scheme(&self) -> Option<TilingSchemeId> {
        Some(TilingSchemeId::Cdb1GlobalGrid)
    }
    fn attribute_model(&self) -> Option<AttributeModel> {
        self.attribute_model.clone()
    }
    fn known_extensions(&self) -> Vec<String> {
        self.known_extensions.clone()
    }
}

/// Return the generated standard and a note preserving a source declaration.
///
/// Every supplied declaration (even `DCAT` or `NoMetadata`) remains provenance;
/// no source schema is translated. Absence requires no mapping note.
pub fn map_metadata_standard(raw: Option<&str>) -> (MetadataStandard, Option<String>) {
    (MetadataStandard::NoMetadata, raw.map(|value| format!(
        "Source metadata standard {value:?} retained as provenance; generated records use NoMetadata; source schema was not translated."
    )))
}

/// The complete cdb-lint `--profile-file` document for this effective profile.
pub fn descriptor(profile: &MigrationProfile) -> serde_json::Value {
    serde_json::json!({
        "name": profile.name(), "case_rule": "Snake_case", "language": "en",
        "storage_crs_wkt": WGS84_2D_WKT,
        "metadata_standard": profile.metadata_standard().as_str(),
        "metadata_encoding": profile.metadata_encoding().as_str(),
        "uom": profile.uom().as_str(),
        "storage_technology": profile.storage_technology().as_str(),
        "conformance_classes": "all", "tiling_scheme": "CDB1GlobalGrid",
        "resource_metadata_dir": "metadata", "root_folder_name": profile.root_folder_name(),
        "known_extensions": profile.known_extensions(), "attribute_model": profile.attribute_model()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencdb::profiles::ApplicationProfile;

    /// Migration contract: generated metadata never claims translated DCAT.
    #[test]
    fn mig_profile_descriptor_shape() {
        let profile = MigrationProfile::new(vec!["tif".into(), "shp".into(), "tif".into()]);
        assert!(profile.storage_crs().is_ok());
        let d = descriptor(&profile);
        assert_eq!(d["name"], "cdb-migrator-v1");
        assert_eq!(d["metadata_standard"], "NoMetadata");
        assert_eq!(d["case_rule"], "Snake_case");
        assert_eq!(d["metadata_encoding"], "json");
        assert_eq!(d["tiling_scheme"], "CDB1GlobalGrid");
        assert_eq!(d["known_extensions"], serde_json::json!(["shp", "tif"]));
    }

    /// Attr1/Attr2: the emitted yardstick carries exactly the explicit model.
    #[test]
    fn mig_profile_explicit_attribute_model_matches_descriptor() {
        let model = opencdb::AttributeModel {
            schema_uri: None,
            attributes: vec![opencdb::AttributeDef {
                id: " 1 ".into(),
                name: " Height ".into(),
                description: " Height in metres ".into(),
            }],
        };
        let profile = MigrationProfile::new(vec!["wkt".into()])
            .with_attribute_model(model)
            .unwrap();
        let declared = profile.attribute_model().unwrap();
        assert_eq!(declared.attributes[0].id, "1");
        assert_eq!(declared.attributes[0].name, "Height");
        assert_eq!(
            descriptor(&profile)["attribute_model"],
            serde_json::to_value(&declared).unwrap()
        );
        assert!(MigrationProfile::new(vec![])
            .with_attribute_model(opencdb::AttributeModel {
                schema_uri: None,
                attributes: vec![]
            })
            .is_err());
        assert!(descriptor(&MigrationProfile::new(vec![]))["attribute_model"].is_null());
    }

    /// Name5 guards all Metadata5 encodings; case errors cannot hide a record.
    #[test]
    fn mig_profile_guards_resource_records_and_validates_real_store() {
        let profile = MigrationProfile::new(vec!["wkt".into()]);
        for path in [
            "metadata/a.json",
            "/tiles/METADATA/a.XML",
            "/metadata/a.GPKG",
        ] {
            assert!(profile.is_resource_metadata(path), "{path}");
        }
        for path in [
            "metadata/a.tif",
            "metadata/a",
            "/metadatas/a.json",
            "/metadata/nested/a.json",
            "/global_metadata/global_metadata.json",
        ] {
            assert!(!profile.is_resource_metadata(path), "{path}");
        }
        assert!(profile.style_guide().is_reserved("metadata"));
        assert_eq!(
            profile.conformance_classes(),
            opencdb::RequirementsClass::ALL
        );
        let temp = tempfile::tempdir().unwrap();
        let store = opencdb::CdbDatastore::create(
            temp.path(),
            &profile,
            opencdb::DatastoreSeed::new(
                "migration",
                "Migration",
                "Explicit migration metadata",
                "ops@example.org",
            ),
        )
        .unwrap();
        let report = store.validate(&profile).unwrap();
        assert!(report.is_conformant(), "{report:?}");
        let d = descriptor(&profile);
        assert_eq!(d.as_object().unwrap().len(), 14);
        assert_eq!(d["uom"], "M");
        assert_eq!(d["language"], "en");
        assert_eq!(d["root_folder_name"], "cdb");
        assert_eq!(d["resource_metadata_dir"], "metadata");
        assert_eq!(d["storage_technology"], "file-system");
        assert_eq!(d["conformance_classes"], "all");
        assert_eq!(
            opencdb::crs::StorageCrs::from_wkt(d["storage_crs_wkt"].as_str().unwrap())
                .unwrap()
                .authority(),
            Some(("EPSG".into(), "4326".into()))
        );
    }

    /// Source declarations are provenance, including recognized 2.0 keywords.
    #[test]
    fn mig_source_standard_is_never_a_schema_translation() {
        for raw in ["DCAT", "ISO-19115:2014", "NoMetadata"] {
            let (standard, note) = map_metadata_standard(Some(raw));
            assert_eq!(standard, opencdb::metadata::MetadataStandard::NoMetadata);
            assert!(note.unwrap().contains(raw));
        }
        assert_eq!(
            map_metadata_standard(None),
            (opencdb::metadata::MetadataStandard::NoMetadata, None)
        );
    }
}
