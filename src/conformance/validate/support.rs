//! Test fixtures shared by the three `validate` submodules: a profile whose
//! conformance-class declaration and cross-check yardsticks can be varied
//! freely, and the datastore/model constructors the stage, sweep and
//! orchestrator tests all build on.

use crate::attribution::{AttributeDef, AttributeModel};
use crate::conformance::RequirementsClass;
use crate::crs::{CrsViolation, StorageCrs};
use crate::datastore::{CdbDatastore, DatastoreSeed};
use crate::metadata::{MetadataEncoding, MetadataStandard, UnitOfMeasure};
use crate::naming::StyleGuide;
use crate::profiles::{ApplicationProfile, SimulationProfile, StorageTechnology};
use crate::tiling::TilingSchemeId;

/// A profile that delegates every policy to the default simulation
/// profile but declares an arbitrary set of requirements classes, so the
/// declared-class stages can be driven — including the *restricted*
/// declarations the content sweep is aimed at, which
/// `SimulationProfile` itself can no longer express now that it
/// truthfully declares all eleven classes.
pub(super) struct DeclaringProfile {
    pub(super) inner: SimulationProfile,
    pub(super) classes: Vec<RequirementsClass>,
    /// The Tiling4 cross-check's yardstick; `None` (the trait default)
    /// means the profile pins no scheme.
    pub(super) scheme: Option<TilingSchemeId>,
    /// The attribute-model cross-check's yardstick.
    pub(super) model: Option<AttributeModel>,
}

impl DeclaringProfile {
    /// Declares every class the core defines.
    pub(super) fn all() -> Self {
        DeclaringProfile {
            inner: SimulationProfile::json(),
            classes: RequirementsClass::ALL.to_vec(),
            scheme: None,
            model: None,
        }
    }

    /// Declares only the five mandatory classes — the shape the content
    /// sweep is aimed at.
    pub(super) fn mandatory_only() -> Self {
        DeclaringProfile {
            classes: RequirementsClass::MANDATORY.to_vec(),
            ..DeclaringProfile::all()
        }
    }

    /// Pins the tiling scheme the Tiling4 cross-check compares against.
    pub(super) fn with_scheme(mut self, scheme: TilingSchemeId) -> Self {
        self.scheme = Some(scheme);
        self
    }

    /// Pins the attribute model the Attribution cross-check compares
    /// against.
    pub(super) fn with_model(mut self, model: AttributeModel) -> Self {
        self.model = Some(model);
        self
    }
}

impl ApplicationProfile for DeclaringProfile {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn style_guide(&self) -> StyleGuide {
        self.inner.style_guide()
    }
    fn storage_crs(&self) -> Result<StorageCrs, CrsViolation> {
        self.inner.storage_crs()
    }
    fn metadata_standard(&self) -> MetadataStandard {
        self.inner.metadata_standard()
    }
    fn metadata_encoding(&self) -> MetadataEncoding {
        self.inner.metadata_encoding()
    }
    fn uom(&self) -> UnitOfMeasure {
        self.inner.uom()
    }
    fn storage_technology(&self) -> StorageTechnology {
        self.inner.storage_technology()
    }
    fn conformance_classes(&self) -> Vec<RequirementsClass> {
        self.classes.clone()
    }
    fn is_resource_metadata(&self, logical_path: &str) -> bool {
        self.inner.is_resource_metadata(logical_path)
    }
    fn tiling_scheme(&self) -> Option<TilingSchemeId> {
        self.scheme
    }
    fn attribute_model(&self) -> Option<AttributeModel> {
        self.model.clone()
    }
    fn known_extensions(&self) -> Vec<String> {
        self.inner.known_extensions()
    }
}

/// A one-attribute model, the Attr2 minimum.
pub(super) fn model_of(id: &str, name: &str) -> AttributeModel {
    AttributeModel {
        schema_uri: None,
        attributes: vec![AttributeDef {
            id: id.to_owned(),
            name: name.to_owned(),
            description: "An attribute of a street".to_owned(),
        }],
    }
}

/// Creates a fresh JSON datastore for the optional-class stage tests.
pub(super) fn fresh_store(tmp: &tempfile::TempDir) -> CdbDatastore {
    CdbDatastore::create(
        tmp.path(),
        &SimulationProfile::json(),
        DatastoreSeed::new("id", "Title", "Description", "contact"),
    )
    .unwrap()
}
