//! The default `simulation` application profile (spec §5.1; Annex A, Annex B).
//!
//! The CDB 2.0 Core is abstract (§5.1); this profile pins every core
//! singularity so the core becomes implementable for the simulation/gaming
//! use case that CDB 1.x served:
//!
//! - **storage** — a file system (Annex B, [`StorageTechnology::FileSystem`]);
//! - **CRS** — WGS-84, EPSG:4326 in 2D, as WKT-2 metadata (Requirements
//!   CRS3/CRS4/CRS5, Recommendation `/rec/core/crs/crs-definition`);
//! - **metadata** — the DCAT standard (Metadata2), JSON *or* XML encoding
//!   (Metadata5), English (Metadata4/Name3), metres (Metadata8);
//! - **naming** — the PascalCase case rule (Name6) under an English style
//!   guide (Name5), with a sibling `metadata/` convention directory for
//!   resource metadata;
//! - **tiling** — the CDB 1.x global grid ([`TilingSchemeId::Cdb1GlobalGrid`]),
//!   declared here; its requirements class is implemented in Phase 9.

use crate::crs::{CrsViolation, StorageCrs};
use crate::metadata::{MetadataEncoding, MetadataStandard, UnitOfMeasure};
use crate::naming::{self, CaseRule, NamingViolation, StyleGuide};
use crate::profiles::{ApplicationProfile, RequirementsClass, StorageTechnology, TilingSchemeId};

/// The profile's name, used in its Annex A conformance-class URIs.
pub const SIMULATION_PROFILE_NAME: &str = "simulation";

/// The profile's storage CRS: 2D WGS-84 (EPSG:4326) as WKT-2 (ISO 19162),
/// the form Requirement CRS5 stores in the global metadata folder. Geographic
/// per Requirement CRS4; WGS-84 per Recommendation
/// `/rec/core/crs/crs-definition` (CDB 1.x compatibility).
pub const WGS84_2D_WKT: &str = r#"GEOGCRS["WGS 84",
  DATUM["World Geodetic System 1984",
    ELLIPSOID["WGS 84",6378137,298.257223563,LENGTHUNIT["metre",1.0]]],
  CS[ellipsoidal,2],
    AXIS["latitude",north,ORDER[1]],
    AXIS["longitude",east,ORDER[2]],
    ANGLEUNIT["degree",0.0174532925199433],
  ID["EPSG",4326]]"#;

/// The lowercase convention directory that holds resource metadata records
/// (a Requirement Name5 path duty; the style guide reserves it, which
/// exempts it from the PascalCase rule).
const RESOURCE_METADATA_DIR: &str = "metadata";

/// File extensions the recognizer accepts inside a `metadata/` directory:
/// the two encodings this profile can declare plus `gpkg` (the third
/// Metadata5 encoding), so a stray GeoPackage record is *recognized* — and
/// later reported as an encoding mismatch — rather than silently ignored.
const RESOURCE_METADATA_EXTENSIONS: [&str; 3] = ["json", "xml", "gpkg"];

/// The default simulation application profile — the CDB 1.x-compatible
/// restriction of the abstract core (§5.1). All policy is fixed; the only
/// construction choice is the metadata encoding, JSON ([`Self::json`]) or
/// XML ([`Self::xml`]) per Requirement Metadata5. The third Metadata5
/// encoding, GeoPackage, is unrepresentable by construction: no constructor
/// accepts an encoding value, and the core cannot write that container.
#[derive(Debug, Clone)]
pub struct SimulationProfile {
    encoding: MetadataEncoding,
}

impl SimulationProfile {
    /// A simulation profile declaring the JSON metadata encoding
    /// (Requirement Metadata5).
    pub fn json() -> Self {
        Self {
            encoding: MetadataEncoding::Json,
        }
    }

    /// A simulation profile declaring the XML metadata encoding
    /// (Requirement Metadata5).
    pub fn xml() -> Self {
        Self {
            encoding: MetadataEncoding::Xml,
        }
    }

    /// Maps a resource's logical path to its metadata record's logical path
    /// under this profile's convention (Requirement Name5): a sibling
    /// `metadata/` directory holding a record with the same stem and the
    /// declared encoding's extension — `/Tiles/RoadNetwork.gpkg` maps to
    /// `/Tiles/metadata/RoadNetwork.json` under [`SimulationProfile::json`].
    ///
    /// The input must satisfy the crate's structural naming rules
    /// ([`naming::validate_path`]). Defined edge mappings: a resource without
    /// an extension keeps its whole name as the stem (`/Tiles/Elevation`
    /// maps to `/Tiles/metadata/Elevation.json`); a root-level resource maps
    /// into the root convention directory (`/RoadNetwork.gpkg` maps to
    /// `/metadata/RoadNetwork.json`, relative `RoadNetwork.gpkg` to
    /// `metadata/RoadNetwork.json`). The bare datastore root `/` names no
    /// resource and is rejected as [`NamingViolation::EmptyName`].
    pub fn resource_metadata_path(&self, resource: &str) -> Result<String, NamingViolation> {
        naming::validate_path(resource)?;
        let (directory, name) = match resource.rfind('/') {
            Some(index) => resource.split_at(index + 1),
            None => ("", resource),
        };
        if name.is_empty() {
            // Only the bare root "/" survives validate_path with an empty
            // final component; it names no resource.
            return Err(NamingViolation::EmptyName);
        }
        let (stem, _) = naming::split_extension(name);
        Ok(format!(
            "{directory}{RESOURCE_METADATA_DIR}/{stem}.{}",
            self.encoding.extension()
        ))
    }
}

/// The default construction declares the JSON encoding ([`Self::json`]).
impl Default for SimulationProfile {
    fn default() -> Self {
        Self::json()
    }
}

impl ApplicationProfile for SimulationProfile {
    fn name(&self) -> &str {
        SIMULATION_PROFILE_NAME
    }

    /// PascalCase (Name6) and English (Name3), with the lowercase `metadata`
    /// convention directory reserved by design — reserving exempts it from
    /// the case rule (Name5).
    fn style_guide(&self) -> StyleGuide {
        let mut guide = StyleGuide::new(CaseRule::PascalCase, "en");
        guide.reserve_name(RESOURCE_METADATA_DIR);
        guide
    }

    fn storage_crs(&self) -> Result<StorageCrs, CrsViolation> {
        StorageCrs::from_wkt(WGS84_2D_WKT)
    }

    fn metadata_standard(&self) -> MetadataStandard {
        MetadataStandard::Dcat
    }

    fn metadata_encoding(&self) -> MetadataEncoding {
        self.encoding
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

    /// A path is a resource-metadata record when its parent component is
    /// exactly the `metadata` convention directory and its extension is one
    /// of the Metadata5 encodings (ASCII case-insensitive). See
    /// [`Self::resource_metadata_path`] for the forward mapping.
    fn is_resource_metadata(&self, logical_path: &str) -> bool {
        let relative = logical_path.strip_prefix('/').unwrap_or(logical_path);
        let mut components = relative.rsplit('/');
        let file_name = components.next().unwrap_or_default();
        if components.next() != Some(RESOURCE_METADATA_DIR) {
            return false;
        }
        match naming::split_extension(file_name).1 {
            Some(extension) => RESOURCE_METADATA_EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension)),
            None => false,
        }
    }

    /// The CDB 1.x global grid; its requirements class arrives in Phase 9 —
    /// this is the declaration only.
    fn tiling_scheme(&self) -> Option<TilingSchemeId> {
        Some(TilingSchemeId::Cdb1GlobalGrid)
    }

    /// Vouches for the industry-standard `.wkt` extension (Requirement
    /// Name7-B): the crate persists the storage CRS as `crs.wkt`
    /// (Requirement CRS5), an extension outside the Name7 table.
    fn known_extensions(&self) -> Vec<String> {
        vec!["wkt".to_string()]
    }
}

#[cfg(test)]
mod tests {
    use super::SimulationProfile;
    use crate::crs::wkt2::CrsKind;
    use crate::metadata::{MetadataEncoding, MetadataStandard, UnitOfMeasure};
    use crate::naming::CaseRule;
    use crate::profiles::{
        ApplicationProfile, RequirementsClass, StorageTechnology, TilingSchemeId,
    };

    /// Requirements Name5/Name6/Name3 (§7.4.6/§7.4.7/§7.4.2): the profile's
    /// style guide pins PascalCase and English and reserves the lowercase
    /// `metadata` convention directory (reserving exempts it from the case
    /// rule). That this `impl` compiles at all is the compile-time proof that
    /// `style_guide()` is enforceable as a *required* trait item — no
    /// `trybuild` dependency is needed to assert the profile cannot omit it.
    #[test]
    fn req_core_name_ap_guide_simulation_style_guide() {
        let guide = SimulationProfile::json().style_guide();
        assert_eq!(guide.case_rule(), CaseRule::PascalCase);
        assert_eq!(guide.language(), "en");
        assert!(guide.is_reserved("metadata"));
    }

    /// Recommendation `/rec/core/crs/crs-definition` (§7.3.1.3): the profile
    /// pins WGS-84 (EPSG:4326), so the storage CRS builds cleanly and raises
    /// no `NotWgs84` warning.
    #[test]
    fn rec_core_crs_definition_simulation_pins_wgs84() {
        let crs = SimulationProfile::json().storage_crs().unwrap();
        assert_eq!(
            crs.authority(),
            Some(("EPSG".to_owned(), "4326".to_owned()))
        );
        assert!(crs.warnings().is_empty());
    }

    /// Requirement CRS4 (§7.3.1.3): the pinned storage CRS is geographic — an
    /// allowed storage-CRS kind.
    #[test]
    fn req_core_crs_storage_valid_value_simulation_crs_is_geographic() {
        let crs = SimulationProfile::json().storage_crs().unwrap();
        assert_eq!(crs.kind(), CrsKind::Geographic);
    }

    /// Requirement Metadata5 (§7.9.3.5): the encoding is exactly the one the
    /// constructor pins — `json()` -> JSON, `xml()` -> XML, `Default` -> JSON.
    /// `Gpkg` is unrepresentable by construction: no constructor accepts an
    /// encoding, so a simulation datastore can never declare the container
    /// encoding the core cannot write.
    #[test]
    fn req_core_metadata_encoding_simulation_json_or_xml() {
        assert_eq!(
            SimulationProfile::json().metadata_encoding(),
            MetadataEncoding::Json
        );
        assert_eq!(
            SimulationProfile::xml().metadata_encoding(),
            MetadataEncoding::Xml
        );
        assert_eq!(
            SimulationProfile::default().metadata_encoding(),
            MetadataEncoding::Json
        );
    }

    /// Annex A `/conf/minimal-core`: the profile declares conformance to every
    /// mandatory requirements class.
    #[test]
    fn conf_core_minimal_simulation_declares_all_mandatory() {
        let declared = SimulationProfile::json().conformance_classes();
        for class in RequirementsClass::MANDATORY {
            assert!(declared.contains(&class), "missing declaration for {class}");
        }
    }

    /// Annex B storage technology plus the tiling-scheme declaration: the
    /// profile pins file-system storage and the CDB 1.x global grid (whose
    /// requirements class is implemented in Phase 9).
    #[test]
    fn simulation_declares_file_system_and_cdb1_tiling() {
        let profile = SimulationProfile::json();
        assert_eq!(profile.storage_technology(), StorageTechnology::FileSystem);
        assert_eq!(
            profile.tiling_scheme(),
            Some(TilingSchemeId::Cdb1GlobalGrid)
        );
    }

    /// Requirements Metadata2/Metadata8 (§7.9.3.2/§7.9.4): the profile pins the
    /// DCAT metadata standard and metres as the unit of measure.
    #[test]
    fn req_core_metadata_standard_and_uom_pinned() {
        let profile = SimulationProfile::json();
        assert_eq!(profile.metadata_standard(), MetadataStandard::Dcat);
        assert_eq!(profile.uom(), UnitOfMeasure::Meters);
    }

    /// Requirement Name5 (§7.4.6): the profile's resource-metadata convention
    /// is a sibling `metadata/` directory holding a same-stem record in the
    /// declared encoding. The path builder and the recognizer round-trip; the
    /// recognizer rejects the global record and the data file; and the built
    /// path itself satisfies the profile's own style guide.
    #[test]
    fn simulation_resource_metadata_convention() {
        let json = SimulationProfile::json();
        let record = json
            .resource_metadata_path("/Tiles/RoadNetwork.gpkg")
            .unwrap();
        assert_eq!(record, "/Tiles/metadata/RoadNetwork.json");
        assert!(json.is_resource_metadata(&record));

        let xml = SimulationProfile::xml();
        assert_eq!(
            xml.resource_metadata_path("/Tiles/RoadNetwork.gpkg")
                .unwrap(),
            "/Tiles/metadata/RoadNetwork.xml"
        );

        assert!(!json.is_resource_metadata("/global_metadata/global_metadata.json"));
        assert!(!json.is_resource_metadata("/Tiles/RoadNetwork.gpkg"));

        json.style_guide().validate_path(&record).unwrap();
    }

    /// Requirement Name7-B (§7.4.8): the profile vouches for the industry-
    /// standard `.wkt` extension (the storage-CRS record it persists),
    /// suppressing that extension's `NonSpecExtension` warning.
    #[test]
    fn req_core_name_extensions_simulation_vouches_for_wkt() {
        assert!(
            SimulationProfile::json()
                .known_extensions()
                .contains(&"wkt".to_string())
        );
    }
}
