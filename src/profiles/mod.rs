//! Application-profile layer (spec §5.1; Annex A `/conf/minimal-core`; Annex B).
//!
//! The CDB 2.0 Core is abstract and cannot be implemented directly (§5.1);
//! only an *application profile* that pins the core's singularities — one CRS
//! (CRS3), one metadata standard/encoding/language/unit (Metadata2/5/4/8),
//! one case rule and style guide (Name6/Name5) — is implementable. This
//! module defines the [`ApplicationProfile`] trait every profile satisfies and
//! the [`RequirementsClass`] enumeration behind Annex A's `/conf/minimal-core`
//! declaration.

pub mod simulation;

pub use crate::tiling::TilingSchemeId;
pub use simulation::SimulationProfile;

use std::fmt;

use crate::attribution::AttributeModel;
use crate::crs::{CrsViolation, StorageCrs};
use crate::hierarchy::RECOMMENDED_ROOT_NAME;
use crate::metadata::{
    LanguageTag, MetadataEncoding, MetadataStandard, MetadataViolation, UnitOfMeasure,
};
use crate::naming::StyleGuide;

/// A mandatory CDB Core requirements class (Annex A). The abstract core
/// bundles all five into `/conf/minimal-core`; a profile declares conformance
/// per class. `#[non_exhaustive]` leaves room for the optional classes later
/// phases add (tiling, coverage, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum RequirementsClass {
    Crs,
    FileNaming,
    FileStructure,
    Links,
    Metadata,
}

impl RequirementsClass {
    /// The five classes Annex A `/conf/minimal-core` makes mandatory for every
    /// profile. Declaration order equals [`Ord`] order — the order a
    /// conformance report lists them.
    pub const MANDATORY: [RequirementsClass; 5] = [
        RequirementsClass::Crs,
        RequirementsClass::FileNaming,
        RequirementsClass::FileStructure,
        RequirementsClass::Links,
        RequirementsClass::Metadata,
    ];

    /// The class's short name, used in its conformance-class URI (Annex A).
    pub fn as_str(self) -> &'static str {
        match self {
            RequirementsClass::Crs => "crs",
            RequirementsClass::FileNaming => "file-naming",
            RequirementsClass::FileStructure => "file-structure",
            RequirementsClass::Links => "links",
            RequirementsClass::Metadata => "metadata",
        }
    }

    /// The requirements-module URI this class encodes (§7.3–§7.9). The
    /// trailing hyphen on the metadata URI is verbatim from the draft (§7.9).
    pub fn requirements_uri(self) -> &'static str {
        match self {
            RequirementsClass::Crs => "/req/core/data-representation",
            RequirementsClass::FileNaming => "/req/core/naming-system",
            RequirementsClass::FileStructure => "/req/core/file-system",
            RequirementsClass::Links => "/req/core/links",
            RequirementsClass::Metadata => "/req/core/metadata-",
        }
    }

    /// The Annex A conformance-class URI a profile declares for this class.
    /// The published draft omits some path separators; this normalizes them to
    /// the well-formed OGC pattern
    /// `http://www.opengis.net/spec/CDB/2.0/conf/<profile>/<class>`.
    pub fn conformance_uri(self, profile_name: &str) -> String {
        format!(
            "http://www.opengis.net/spec/CDB/2.0/conf/{profile_name}/{}",
            self.as_str()
        )
    }
}

impl fmt::Display for RequirementsClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The storage technology a profile pins (Annex B). The core is abstract as to
/// storage; the default simulation profile uses a file system.
/// `#[non_exhaustive]` leaves room for object stores and databases later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StorageTechnology {
    FileSystem,
}

impl StorageTechnology {
    /// The technology's short name.
    pub fn as_str(self) -> &'static str {
        match self {
            StorageTechnology::FileSystem => "file-system",
        }
    }
}

impl fmt::Display for StorageTechnology {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The restrictions an application profile SHALL declare to make the abstract
/// CDB Core implementable (§5.1). Every core *singularity* is a required item;
/// provided methods cover only what the spec itself defaults
/// (`root_folder_name`, RFile1), derives (`language`, from the one style
/// guide), or leaves optional (`tiling_scheme`, `known_extensions`).
///
/// The trait is object-safe: the facade holds a `&dyn ApplicationProfile`.
pub trait ApplicationProfile {
    /// The profile's name, used in its conformance-class URIs (Annex A).
    fn name(&self) -> &str;

    /// The naming style guide (Requirement Name5): the datastore-wide case
    /// rule (Name6) and language (Name3).
    fn style_guide(&self) -> StyleGuide;

    /// The single storage CRS (Requirement CRS3); its construction may fail
    /// (CRS4/CRS6/CRS7).
    fn storage_crs(&self) -> Result<StorageCrs, CrsViolation>;

    /// The single metadata standard (Requirement Metadata2).
    fn metadata_standard(&self) -> MetadataStandard;

    /// The single metadata encoding (Requirement Metadata5).
    fn metadata_encoding(&self) -> MetadataEncoding;

    /// The single unit of measure (Requirement Metadata8).
    fn uom(&self) -> UnitOfMeasure;

    /// The storage technology (Annex B).
    fn storage_technology(&self) -> StorageTechnology;

    /// The requirements classes the profile declares conformance to; Annex A
    /// `/conf/minimal-core` requires at least [`RequirementsClass::MANDATORY`].
    fn conformance_classes(&self) -> Vec<RequirementsClass>;

    /// Whether `logical_path` names a resource-metadata file under this
    /// profile's convention (a Requirement Name5 path duty).
    fn is_resource_metadata(&self, logical_path: &str) -> bool;

    /// The datastore-wide language (Requirement Name3-A / Metadata4), derived
    /// from the one style guide so there is a single source of truth.
    fn language(&self) -> Result<LanguageTag, MetadataViolation> {
        LanguageTag::new(self.style_guide().language())
    }

    /// The root folder name; defaults to the recommended `cdb` (RFile1).
    fn root_folder_name(&self) -> &str {
        RECOMMENDED_ROOT_NAME
    }

    /// The tiling scheme, if the profile declares the optional tiling class.
    fn tiling_scheme(&self) -> Option<TilingSchemeId> {
        None
    }

    /// The attribute model, if the profile declares the optional
    /// attribution class — Requirement Attr1-A (§7.1.2.1): any profile
    /// "specifying and/or implementing attribution for features SHALL
    /// specify an attribute model".
    fn attribute_model(&self) -> Option<AttributeModel> {
        None
    }

    /// Extensions outside the Requirement Name7 table that this profile
    /// vouches for as industry standard (Requirement Name7-B), suppressing the
    /// `NonSpecExtension` warning for them.
    fn known_extensions(&self) -> Vec<String> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{ApplicationProfile, RequirementsClass, StorageTechnology};
    use crate::attribution::{AttributeDef, AttributeModel};
    use crate::crs::{CrsViolation, StorageCrs};
    use crate::metadata::{MetadataEncoding, MetadataStandard, MetadataViolation, UnitOfMeasure};
    use crate::naming::{CaseRule, StyleGuide};

    /// A valid geographic (EPSG:4326) storage CRS for exercising the trait.
    const TEST_WKT: &str = r#"GEOGCRS["WGS 84",
  DATUM["World Geodetic System 1984",
    ELLIPSOID["WGS 84",6378137,298.257223563,LENGTHUNIT["metre",1.0]]],
  CS[ellipsoidal,2],
    AXIS["latitude",north,ORDER[1]],
    AXIS["longitude",east,ORDER[2]],
    ANGLEUNIT["degree",0.01745329252],
  ID["EPSG",4326]]"#;

    /// A minimal profile implemented in-test to exercise the trait's required
    /// items and its provided defaults without depending on the simulation
    /// profile (Step 2). Its language is configurable so a malformed tag can
    /// be forced.
    struct TestProfile {
        language: String,
    }

    impl TestProfile {
        fn new() -> Self {
            Self {
                language: "en".to_owned(),
            }
        }

        fn with_language(language: &str) -> Self {
            Self {
                language: language.to_owned(),
            }
        }
    }

    impl ApplicationProfile for TestProfile {
        fn name(&self) -> &str {
            "test"
        }

        fn style_guide(&self) -> StyleGuide {
            StyleGuide::new(CaseRule::PascalCase, self.language.clone())
        }

        fn storage_crs(&self) -> Result<StorageCrs, CrsViolation> {
            StorageCrs::from_wkt(TEST_WKT)
        }

        fn metadata_standard(&self) -> MetadataStandard {
            MetadataStandard::Dcat
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
            RequirementsClass::MANDATORY.to_vec()
        }

        fn is_resource_metadata(&self, logical_path: &str) -> bool {
            logical_path.contains("/metadata/")
        }
    }

    /// Annex A `/conf/minimal-core` — the five mandatory requirements classes,
    /// with the spec's short names.
    #[test]
    fn conf_core_minimal_mandatory_classes_enumerated() {
        assert_eq!(
            RequirementsClass::MANDATORY,
            [
                RequirementsClass::Crs,
                RequirementsClass::FileNaming,
                RequirementsClass::FileStructure,
                RequirementsClass::Links,
                RequirementsClass::Metadata,
            ]
        );
        assert_eq!(RequirementsClass::Crs.as_str(), "crs");
        assert_eq!(RequirementsClass::FileNaming.as_str(), "file-naming");
        assert_eq!(RequirementsClass::FileStructure.as_str(), "file-structure");
        assert_eq!(RequirementsClass::Links.as_str(), "links");
        assert_eq!(RequirementsClass::Metadata.as_str(), "metadata");
    }

    /// Annex A conformance table — the conformance-class URI for a profile.
    /// The published draft omits some path separators; the crate normalizes
    /// them to the well-formed OGC pattern.
    #[test]
    fn conf_core_conformance_uri_pattern() {
        for class in RequirementsClass::MANDATORY {
            assert_eq!(
                class.conformance_uri("simulation"),
                format!(
                    "http://www.opengis.net/spec/CDB/2.0/conf/simulation/{}",
                    class.as_str()
                )
            );
        }
        assert_eq!(
            RequirementsClass::Metadata.conformance_uri("simulation"),
            "http://www.opengis.net/spec/CDB/2.0/conf/simulation/metadata"
        );
    }

    /// §7.3–§7.9 — each requirements class maps to its spec-module URI. The
    /// trailing hyphen on the metadata URI is verbatim from the draft (§7.9).
    #[test]
    fn requirements_class_uris_match_spec_modules() {
        assert_eq!(
            RequirementsClass::Crs.requirements_uri(),
            "/req/core/data-representation"
        );
        assert_eq!(
            RequirementsClass::FileNaming.requirements_uri(),
            "/req/core/naming-system"
        );
        assert_eq!(
            RequirementsClass::FileStructure.requirements_uri(),
            "/req/core/file-system"
        );
        assert_eq!(
            RequirementsClass::Links.requirements_uri(),
            "/req/core/links"
        );
        assert_eq!(
            RequirementsClass::Metadata.requirements_uri(),
            "/req/core/metadata-"
        );
    }

    /// The trait is object-safe: the facade drives any profile through a
    /// `&dyn ApplicationProfile`.
    #[test]
    fn application_profile_is_object_safe() {
        let profile = TestProfile::new();
        let handle: &dyn ApplicationProfile = &profile;
        assert_eq!(handle.name(), "test");
        assert_eq!(handle.metadata_encoding(), MetadataEncoding::Json);
        assert_eq!(handle.metadata_standard(), MetadataStandard::Dcat);
        assert_eq!(handle.uom(), UnitOfMeasure::Meters);
        assert_eq!(handle.storage_technology(), StorageTechnology::FileSystem);
        assert_eq!(
            handle.conformance_classes(),
            RequirementsClass::MANDATORY.to_vec()
        );
        assert!(handle.storage_crs().is_ok());
        assert!(handle.is_resource_metadata("/Tiles/metadata/RoadNetwork.json"));
        assert!(!handle.is_resource_metadata("/Tiles/RoadNetwork.gpkg"));
        assert_eq!(handle.language().unwrap().as_str(), "en");
        assert_eq!(handle.root_folder_name(), "cdb");
    }

    /// Requirement Metadata4 / Name3-A — `language()` derives from the single
    /// style guide (one source of truth); a malformed tag surfaces as
    /// InvalidLanguageTag.
    #[test]
    fn req_core_metadata_language_derives_from_style_guide() {
        let good = TestProfile::new();
        assert_eq!(good.language().unwrap().as_str(), "en");

        let bad = TestProfile::with_language("xx!");
        assert!(matches!(
            bad.language(),
            Err(MetadataViolation::InvalidLanguageTag { .. })
        ));
    }

    /// Recommendation RFile1 — the default root folder name is `cdb`.
    #[test]
    fn rec_core_file_hierarchy_profile_default_root_is_cdb() {
        assert_eq!(TestProfile::new().root_folder_name(), "cdb");
        assert_eq!(
            TestProfile::new().root_folder_name(),
            crate::hierarchy::RECOMMENDED_ROOT_NAME
        );
    }

    /// Tiling is an optional class (default: no scheme) and a bare profile
    /// vouches for no extra extensions (Requirement Name7-B).
    #[test]
    fn profile_defaults_tiling_none_and_no_extra_extensions() {
        let profile = TestProfile::new();
        assert_eq!(profile.tiling_scheme(), None);
        assert!(profile.known_extensions().is_empty());
    }

    /// A profile that implements attribution, exercising the Attr1-A
    /// duty through the hook.
    struct AttributedProfile;

    impl ApplicationProfile for AttributedProfile {
        fn name(&self) -> &str {
            "attributed"
        }

        fn style_guide(&self) -> StyleGuide {
            StyleGuide::new(CaseRule::PascalCase, "en")
        }

        fn storage_crs(&self) -> Result<StorageCrs, CrsViolation> {
            StorageCrs::from_wkt(TEST_WKT)
        }

        fn metadata_standard(&self) -> MetadataStandard {
            MetadataStandard::Dcat
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
            RequirementsClass::MANDATORY.to_vec()
        }

        fn is_resource_metadata(&self, logical_path: &str) -> bool {
            logical_path.contains("/metadata/")
        }

        fn attribute_model(&self) -> Option<AttributeModel> {
            Some(AttributeModel {
                schema_uri: Some("https://example.org/schemas/street.xsd".to_owned()),
                attributes: vec![AttributeDef {
                    id: "1".to_owned(),
                    name: "StreetName".to_owned(),
                    description: "Name of a street as an alphanumeric string".to_owned(),
                }],
            })
        }
    }

    /// Requirement Attr1-A (§7.1.2.1) — attribution is an optional
    /// class: a bare profile (and the default simulation profile)
    /// declares no attribute model.
    #[test]
    fn req_core_attribute_model_profile_default_none() {
        assert!(TestProfile::new().attribute_model().is_none());
        assert!(
            crate::profiles::SimulationProfile::json()
                .attribute_model()
                .is_none()
        );
    }

    /// Requirement Attr1-A (§7.1.2.1) — a profile "specifying and/or
    /// implementing attribution for features" specifies its attribute
    /// model through the hook, reachable through the object-safe handle.
    #[test]
    fn req_core_attribute_model_profile_declares() {
        let profile = AttributedProfile;
        let handle: &dyn ApplicationProfile = &profile;
        let model = handle.attribute_model().expect("declared model");
        assert!(model.validate().is_ok());
        assert_eq!(model.attributes[0].name, "StreetName");
        assert!(model.schema_uri.is_some());
    }
}
