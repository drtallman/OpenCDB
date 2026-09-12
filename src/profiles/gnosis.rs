//! The `gnosis` application profile (spec §5.1; §7.12; Annex A, Annex B).
//!
//! The same restriction of the abstract core as
//! [`crate::profiles::simulation`], with one singularity pinned
//! differently: the tiling scheme is the **GNOSISGlobalGrid** extension
//! (§7.12, [`TilingSchemeId::GnosisGlobalGrid`]) rather than the CDB 1.x
//! global grid.
//!
//! Its purpose is to exercise the [`ApplicationProfile`] trait. The trait's
//! whole design premise is that profiles vary — §5.1 makes the core
//! implementable *only* through a profile's choices — and until now exactly
//! one profile made those choices, so nothing proved the facade was driven by
//! the declaration rather than by the one implementation behind it. This
//! profile is deliberately **not** a new domain model: it invents no
//! requirement, adds no policy, and every choice other than the tiling scheme
//! and the profile name is the simulation profile's.

use crate::conformance::RequirementsClass;
use crate::crs::{CrsViolation, StorageCrs};
use crate::metadata::{MetadataEncoding, MetadataStandard, UnitOfMeasure};
use crate::naming::{NamingViolation, StyleGuide};
use crate::profiles::simulation::SimulationProfile;
use crate::profiles::{ApplicationProfile, StorageTechnology, TilingSchemeId};

/// The profile's name, used in its Annex A conformance-class URIs.
pub const GNOSIS_PROFILE_NAME: &str = "gnosis";

/// The `gnosis` application profile: the simulation profile's restriction of
/// the core with the §7.12 **GNOSISGlobalGrid** tiling scheme pinned in place
/// of the CDB 1.x global grid.
///
/// It is implemented by *composition* — it holds a [`SimulationProfile`] and
/// forwards every unvaried choice to it — rather than by copying that
/// profile's body. Copying would make a second profile that agrees with the
/// first only until someone edits one of them, and the thing being
/// demonstrated is that the facade is driven by the declaration, not by a
/// particular implementation of it. Two required trait items are answered
/// here: [`Self::name`] and [`Self::tiling_scheme`].
#[derive(Debug, Clone)]
pub struct GnosisProfile {
    base: SimulationProfile,
}

impl GnosisProfile {
    /// A GNOSIS profile declaring the JSON metadata encoding
    /// (Requirement Metadata5).
    pub fn json() -> Self {
        Self {
            base: SimulationProfile::json(),
        }
    }

    /// A GNOSIS profile declaring the XML metadata encoding
    /// (Requirement Metadata5).
    pub fn xml() -> Self {
        Self {
            base: SimulationProfile::xml(),
        }
    }

    /// Maps a resource's logical path to its metadata record's logical path
    /// (Requirement Name5), the simulation profile's `metadata/` convention
    /// unchanged — see [`SimulationProfile::resource_metadata_path`] for the
    /// mapping and its defined edge cases.
    pub fn resource_metadata_path(&self, resource: &str) -> Result<String, NamingViolation> {
        self.base.resource_metadata_path(resource)
    }
}

/// The default construction declares the JSON encoding ([`Self::json`]).
impl Default for GnosisProfile {
    fn default() -> Self {
        Self::json()
    }
}

impl ApplicationProfile for GnosisProfile {
    fn name(&self) -> &str {
        GNOSIS_PROFILE_NAME
    }

    fn style_guide(&self) -> StyleGuide {
        self.base.style_guide()
    }

    fn storage_crs(&self) -> Result<StorageCrs, CrsViolation> {
        self.base.storage_crs()
    }

    fn metadata_standard(&self) -> MetadataStandard {
        self.base.metadata_standard()
    }

    fn metadata_encoding(&self) -> MetadataEncoding {
        self.base.metadata_encoding()
    }

    fn uom(&self) -> UnitOfMeasure {
        self.base.uom()
    }

    fn storage_technology(&self) -> StorageTechnology {
        self.base.storage_technology()
    }

    /// Every class the core defines — the simulation profile's declaration,
    /// deliberately and for the same approved reason. This profile restricts
    /// the whole core rather than a slice of it, and the declaration is
    /// load-bearing: the content sweep reports content whose class a profile
    /// failed to declare, so a profile that pins a tiling scheme and whose
    /// datastores legitimately carry attribute models, coverages, topology
    /// and version journals would otherwise convict its own datastores of a
    /// [`crate::conformance::CdbViolation::DeclarationMismatch`]. An optional
    /// class the datastore holds no content for simply passes with nothing
    /// checked (design spec §4).
    fn conformance_classes(&self) -> Vec<RequirementsClass> {
        self.base.conformance_classes()
    }

    fn is_resource_metadata(&self, logical_path: &str) -> bool {
        self.base.is_resource_metadata(logical_path)
    }

    /// The GNOSISGlobalGrid (§7.12) — the one core singularity this profile
    /// settles differently from [`SimulationProfile`].
    fn tiling_scheme(&self) -> Option<TilingSchemeId> {
        Some(TilingSchemeId::GnosisGlobalGrid)
    }

    fn known_extensions(&self) -> Vec<String> {
        self.base.known_extensions()
    }
}

#[cfg(test)]
mod tests {
    use crate::conformance::CdbViolation;
    use crate::crs::wkt2::CrsKind;
    use crate::datastore::{CdbDatastore, DatastoreSeed};
    use crate::metadata::MetadataEncoding;
    use crate::naming::CaseRule;
    use crate::profiles::{
        ApplicationProfile, GnosisProfile, RequirementsClass, SimulationProfile, StorageTechnology,
        TilingSchemeId,
    };
    use crate::tiling::TilingScheme;

    /// Requirement TCE2 (`/req/core/tiling-extension-tms`, §7.12.3.2) — the
    /// profile pins the GNOSISGlobalGrid tiling scheme, the §7.12 extension,
    /// and that is the one core singularity it settles differently from the
    /// simulation profile.
    #[test]
    fn req_core_tiling_extension_tms_gnosis_profile_pins_gnosis_grid() {
        let profile = GnosisProfile::json();
        assert_eq!(
            profile.tiling_scheme(),
            Some(TilingSchemeId::GnosisGlobalGrid)
        );
        assert_ne!(
            profile.tiling_scheme(),
            SimulationProfile::json().tiling_scheme()
        );
        assert_eq!(profile.name(), "gnosis");
        assert_ne!(profile.name(), SimulationProfile::json().name());
    }

    /// §5.1 — the [`ApplicationProfile`] trait generalizes: two distinct
    /// profile types drive the facade through one `&dyn` handle, and every
    /// core singularity the trait requires is answered by both. The pinned
    /// answers are identical except for the one this profile exists to vary,
    /// which is the point: a second profile that differed everywhere would
    /// prove nothing about the trait and everything about the two impls.
    #[test]
    fn application_profile_generalizes_across_two_profiles() {
        let gnosis = GnosisProfile::json();
        let simulation = SimulationProfile::json();
        let profiles: [&dyn ApplicationProfile; 2] = [&gnosis, &simulation];

        for profile in profiles {
            assert_eq!(profile.storage_technology(), StorageTechnology::FileSystem);
            assert_eq!(profile.metadata_encoding(), MetadataEncoding::Json);
            assert_eq!(profile.style_guide().case_rule(), CaseRule::PascalCase);
            assert_eq!(profile.language().unwrap().as_str(), "en");
            assert_eq!(profile.root_folder_name(), "cdb");
            assert_eq!(profile.storage_crs().unwrap().kind(), CrsKind::Geographic);
            assert!(profile.is_resource_metadata("/Tiles/metadata/RoadNetwork.json"));
            assert!(profile.known_extensions().contains(&"wkt".to_string()));
        }

        // Same answers, different handles: only the two varied items differ.
        assert_eq!(gnosis.metadata_standard(), simulation.metadata_standard());
        assert_eq!(gnosis.uom(), simulation.uom());
        assert_eq!(
            gnosis.storage_crs().unwrap().authority(),
            simulation.storage_crs().unwrap().authority()
        );
        assert_eq!(
            gnosis.conformance_classes(),
            simulation.conformance_classes()
        );
    }

    /// Annex A `/conf/minimal-core` — this profile declares every class the
    /// core defines, exactly as the simulation profile does and for exactly
    /// the same reason: the content sweep reports content whose class a
    /// profile failed to declare, so a profile whose datastores legitimately
    /// carry tiling schemes, attribute models, coverages and journals must
    /// declare those classes or convict its own datastores.
    #[test]
    fn conf_core_minimal_gnosis_declares_all_classes() {
        let declared = GnosisProfile::json().conformance_classes();
        assert_eq!(declared, RequirementsClass::ALL.to_vec());
    }

    /// Requirement Tiling4 (`/req/core/tiling-tilingscheme-consistent`,
    /// §7.10.2.4) — the pin is load-bearing, not decorative: validating a
    /// datastore under this profile cross-checks the `tilingScheme` element
    /// on its global record against GNOSISGlobalGrid, so a CDB1-tiled
    /// datastore is a `DeclarationMismatch` under Tiling *because of which
    /// profile it was judged against*. The same datastore under
    /// [`SimulationProfile`] conforms — which is the whole claim the second
    /// profile exists to make.
    #[test]
    fn req_core_tiling_scheme_consistent_gnosis_profile_judges_by_its_pin() {
        let tmp = tempfile::tempdir().unwrap();
        let gnosis = GnosisProfile::json();
        let seed = DatastoreSeed::new("doi:cdb.gnosis", "Gnosis", "GNOSIS profile", "ops");
        let store = CdbDatastore::create(tmp.path(), &gnosis, seed).unwrap();

        // A datastore tiled by the CDB 1.x grid, judged under GNOSIS.
        let mut global = store.global_metadata().unwrap();
        global.tiling_scheme = Some(TilingScheme::cdb1_global_grid());
        global.write_to(store.layout()).unwrap();

        let report = store.validate(&gnosis).unwrap();
        assert!(!report.class_passed(RequirementsClass::Tiling), "{report}");
        assert!(
            report
                .violations(RequirementsClass::Tiling)
                .iter()
                .any(|violation| matches!(
                    violation,
                    CdbViolation::DeclarationMismatch {
                        element: "tiling scheme",
                        ..
                    }
                )),
            "{report}"
        );
        // Judged under the profile that pins that grid, it conforms.
        let report = store.validate(&SimulationProfile::json()).unwrap();
        assert!(report.is_conformant(), "{report}");

        // And the scheme this profile pins draws no finding.
        let mut global = store.global_metadata().unwrap();
        global.tiling_scheme = Some(TilingScheme::gnosis_global_grid());
        global.write_to(store.layout()).unwrap();
        let report = store.validate(&gnosis).unwrap();
        assert!(report.is_conformant(), "{report}");
    }

    /// Requirement Metadata5 (§7.9.3.5) and Requirement Name5 (§7.4.6) — the
    /// encoding choice and the resource-metadata convention are the
    /// simulation profile's, unchanged: `json()`/`xml()` pin the encoding and
    /// the record path follows the sibling `metadata/` directory.
    #[test]
    fn req_core_metadata_encoding_gnosis_mirrors_simulation_convention() {
        let json = GnosisProfile::json();
        assert_eq!(json.metadata_encoding(), MetadataEncoding::Json);
        assert_eq!(
            GnosisProfile::xml().metadata_encoding(),
            MetadataEncoding::Xml
        );
        assert_eq!(
            GnosisProfile::default().metadata_encoding(),
            MetadataEncoding::Json
        );

        let record = json
            .resource_metadata_path("/Tiles/RoadNetwork.gpkg")
            .unwrap();
        assert_eq!(record, "/Tiles/metadata/RoadNetwork.json");
        assert!(json.is_resource_metadata(&record));
        json.style_guide().validate_path(&record).unwrap();
    }
}
