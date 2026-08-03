//! Implements the abstract Tiling requirements class (spec §7.10) and hosts
//! the tiling-scheme identity shared with the extensions (§7.11–§7.12).
//!
//! The Abstract Tiling Requirements Module is optional but binding once a
//! datastore is tiled (Requirement Tiling1, `/req/core/tiling-rule`). It
//! depends on OGC Abstract Specification Topic 22 (Tiling2,
//! `/req/core/tiling-topic22` — the class listing's `tiling-model` slug is
//! the same rule) and on the CRS and Metadata core modules.
//! [`TilingScheme::validate`] and [`validate_tileset_metadata`] are the
//! module's rule enforcement (Tiling1/Tiling3) — the design-level
//! conformance pattern of CRS2 and Geom1.
//!
//! Draft quirks (keyed on meaning): the requirements-class URI is mislabeled
//! `/req/core/geometry-` in the document (copy-paste); slugs missing their
//! leading `/` are normalized here.

pub mod cdb1_grid;

pub use cdb1_grid::{Cdb1GlobalGrid, Lod};

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::crs::{StorageCrs, authority_ids_match};
use crate::metadata::{Bbox, GlobalMetadata, MetadataViolation, ResourceMetadata};

/// The tiling-scheme extensions the spec defines — exactly two: the CDB 1.x
/// global grid and the GNOSIS global grid. Closed: the core admits no other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TilingSchemeId {
    /// The CDB 1.x-compatible global grid (spec §7.11).
    Cdb1GlobalGrid,
    /// The GNOSIS global grid (spec §7.12).
    GnosisGlobalGrid,
}

/// Inherent identity of a tiling scheme: the closed set and its wire spelling.
impl TilingSchemeId {
    /// Both defined tiling schemes, in declaration order.
    pub const ALL: [TilingSchemeId; 2] = [
        TilingSchemeId::Cdb1GlobalGrid,
        TilingSchemeId::GnosisGlobalGrid,
    ];

    /// The scheme's canonical wire spelling, as it appears in tileset
    /// metadata; the exact form [`Self::parse`] accepts.
    pub fn as_str(self) -> &'static str {
        match self {
            TilingSchemeId::Cdb1GlobalGrid => "CDB1GlobalGrid",
            TilingSchemeId::GnosisGlobalGrid => "GNOSISGlobalGrid",
        }
    }

    /// Parses the wire spelling; exact and case-sensitive (the ids are
    /// identifiers, not free text). Unknown values violate the
    /// Recommendation Tiling1 vocabulary.
    pub fn parse(value: &str) -> Result<TilingSchemeId, TilingViolation> {
        TilingSchemeId::ALL
            .into_iter()
            .find(|id| id.as_str() == value)
            .ok_or_else(|| TilingViolation::UnknownTilingScheme {
                value: value.to_owned(),
            })
    }
}

/// Renders the scheme as its canonical wire spelling.
impl fmt::Display for TilingSchemeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A violation of a SHALL requirement of the tiling module (spec
/// §7.10–§7.11), or a hard rejection from its closed vocabularies (e.g. an
/// unknown tiling-scheme identifier at parse time). SHOULD-level findings
/// are [`TilingWarning`]s — the Recommendation Tiling1 preference for the
/// extension schemes surfaces there, never here.
///
/// Intentionally not `Eq`: a later task adds a variant carrying an `f64`
/// bound, which precludes a total-equality derive.
#[derive(Debug, Error, Clone, PartialEq)]
#[non_exhaustive]
pub enum TilingViolation {
    /// A parse-time vocabulary rejection: the identifier set is closed by
    /// the standard's two extension schemes (the vocabulary of
    /// `/rec/core/tiling-extension`), and this value names neither. The
    /// recommendation aspect — a datastore preferring an extension scheme —
    /// is [`TilingWarning::NonExtensionScheme`]'s job.
    #[error(
        "tiling scheme {value:?} is not CDB1GlobalGrid or GNOSISGlobalGrid (/rec/core/tiling-extension)"
    )]
    UnknownTilingScheme { value: String },
    /// Requirement Tiling5 (`/req/core/tiling-tilingscheme-crs`): the scheme's
    /// declared CRS does not provably match the datastore storage CRS.
    #[error(
        "tiling scheme declares CRS {declared} but the datastore CRS is {datastore} (violates /req/core/tiling-tilingscheme-crs)"
    )]
    SchemeCrsMismatch { declared: String, datastore: String },
    /// Requirement Tiling6 (`/req/core/tiling-tilingscheme-uom`): the scheme's
    /// unit of measure is not the storage CRS's coordinate unit.
    #[error(
        "tiling scheme uom {scheme:?} does not match the CRS coordinate unit {crs_unit:?} (violates /req/core/tiling-tilingscheme-uom)"
    )]
    SchemeUomMismatch { scheme: String, crs_unit: String },
    /// Requirement Tiling7 (`/req/core/tiling-tilingscheme-extent`): the
    /// scheme's extent does not cover the entire earth.
    #[error(
        "tiling scheme extent {extent} does not cover the entire earth (violates /req/core/tiling-tilingscheme-extent)"
    )]
    IncompleteExtent { extent: String },
    /// The tiled datastore's global metadata carries no tiling-scheme
    /// definition at all (the Requirement Tiling4 identity, and with it the
    /// whole `/req/core/tiling-tilingscheme-definition` record, is absent).
    #[error(
        "tiled datastore's global metadata has no tilingScheme element (violates /req/core/tiling-tilingscheme-definition)"
    )]
    MissingTilingScheme,
    /// Requirement Tiling10 (`/req/core/tiling-tileset-metadata-elements`): a
    /// tileset's metadata record carries no Keywords element, which the
    /// standard makes mandatory for tilesets beyond the §7.9.4.2 baseline (ID,
    /// Title, Description).
    #[error(
        "tileset metadata has no Keywords element (violates /req/core/tiling-tileset-metadata-elements)"
    )]
    MissingTilesetKeywords,
    /// Requirement TCE7-A (`/req/core/tiling-extension-tile-tessellate`,
    /// §7.11.3.6): a CDB1GlobalGrid Level of Detail must lie within the closed
    /// range −10..=23 ([`cdb1_grid::LOD_MIN`]..=[`cdb1_grid::LOD_MAX`]). The
    /// first CDB1GlobalGrid-family variant.
    #[error(
        "LoD {lod} is outside the CDB1GlobalGrid range -10..=23 (violates /req/core/tiling-extension-tile-tessellate A)"
    )]
    LodOutOfRange { lod: i8 },
    /// A metadata violation surfaced while validating tileset metadata; the
    /// tiling module depends on the Metadata core module.
    #[error(transparent)]
    Metadata(#[from] MetadataViolation),
}

/// A tiling SHOULD-recommendation that was not met — surfaced in a
/// conformance report, never raised as an error.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TilingWarning {
    /// The declared tiling scheme is not one of the two specified extensions
    /// (Recommendation Tiling1, `/rec/core/tiling-extension`): permitted, but
    /// a specified extension is recommended.
    NonExtensionScheme { id: String },
}

/// Renders each warning as the recommendation text a conformance report shows.
impl fmt::Display for TilingWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TilingWarning::NonExtensionScheme { id } => write!(
                f,
                "tiling scheme {id:?} is not one of the specified tiling extensions; CDB1GlobalGrid or GNOSISGlobalGrid is recommended (/rec/core/tiling-extension)"
            ),
        }
    }
}

/// A tiling scheme's definition record: the identity and geospatial framing a
/// tiled datastore declares in its global metadata (spec §7.10.2.4,
/// Requirements Tiling4–Tiling8). The abstract module fixes what a scheme must
/// state; the two concrete extensions (§7.11 CDB1GlobalGrid, §7.12
/// GNOSISGlobalGrid) supply the values.
///
/// Two of the abstract requirements hold here by construction (decision 2 of
/// the module's design): Tiling4 — a scheme has an identity — because [`id`]
/// is a mandatory field, and Tiling8 — the conformant schemes are exactly the
/// two extensions — because [`TilingSchemeId`] is the closed identity set and
/// [`Self::cdb1_global_grid`] is the canonical CDB1GlobalGrid definition.
/// [`Self::validate`] enforces the remaining SHALLs (Tiling5/6/7) against the
/// datastore storage CRS; [`Self::warnings`] carries the Recommendation
/// Tiling1 preference for an extension scheme.
///
/// [`id`]: Self::id
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TilingScheme {
    /// The scheme identifier (Requirement Tiling4). The two conformant
    /// spellings are `CDB1GlobalGrid` and `GNOSISGlobalGrid`
    /// ([`TilingSchemeId`]); any other value is permitted but draws a
    /// [`TilingWarning::NonExtensionScheme`] (Recommendation Tiling1).
    pub id: String,
    /// The scheme's CRS as an `authority:code` identifier (e.g. `EPSG:4326`),
    /// which must provably match the datastore storage CRS (Requirement
    /// Tiling5).
    pub crs: String,
    /// The unit of measure of the scheme's COORDINATE axes — the storage CRS's
    /// coordinate unit (Requirement Tiling6): decimal degrees for EPSG:4326.
    /// This is never a mensuration unit. A CDB datastore keeps four distinct
    /// units — this coordinate unit, the metadata measurement `uom`
    /// (Metadata8), the vertical-CRS length unit (VCRS3), and any dataset
    /// value unit (Geom4) — and only the coordinate unit belongs here.
    pub uom: String,
    /// The geographic extent the scheme covers, which must span the entire
    /// earth with no gaps (Requirement Tiling7).
    pub extent: Bbox,
}

impl TilingScheme {
    /// The canonical CDB1GlobalGrid definition (spec §7.11.3.3, TCE3): an
    /// EPSG:4326 grid measured in decimal degrees over the whole earth. It
    /// validates against a WGS-84 storage CRS and draws no warnings.
    pub fn cdb1_global_grid() -> TilingScheme {
        TilingScheme {
            id: TilingSchemeId::Cdb1GlobalGrid.as_str().to_owned(),
            crs: "EPSG:4326".to_owned(),
            uom: "degree".to_owned(),
            extent: Bbox {
                west: -180.0,
                south: -90.0,
                east: 180.0,
                north: 90.0,
            },
        }
    }

    /// Requires a tiled datastore's global metadata to carry a tiling-scheme
    /// definition (Requirement Tiling8, `/req/core/tiling-tilingscheme-definition`,
    /// §7.10.2.5): a tiled datastore SHALL declare its scheme on the global
    /// record. Returns the borrowed [`TilingScheme`], or
    /// [`TilingViolation::MissingTilingScheme`] when the `tilingScheme` element
    /// is absent.
    pub fn require(global: &GlobalMetadata) -> Result<&TilingScheme, TilingViolation> {
        global
            .tiling_scheme
            .as_ref()
            .ok_or(TilingViolation::MissingTilingScheme)
    }

    /// Enforces Requirements Tiling5/6/7 against the datastore storage CRS,
    /// first-violation-wins in specification order:
    ///
    /// - **Tiling5** (`/req/core/tiling-tilingscheme-crs`): the scheme CRS,
    ///   split on its first `:` into (authority, code), must provably match
    ///   the storage CRS's authority identity (ASCII case-insensitively). A
    ///   scheme CRS lacking a `:` or with an empty side, or a storage CRS with
    ///   no discoverable authority, cannot prove a match and so is a mismatch.
    /// - **Tiling6** (`/req/core/tiling-tilingscheme-uom`): [`Self::uom`] must
    ///   equal the storage CRS's coordinate unit name (ASCII case-insensitive);
    ///   an undiscoverable unit is a mismatch.
    /// - **Tiling7** (`/req/core/tiling-tilingscheme-extent`): [`Self::extent`]
    ///   must be exactly the whole earth (west −180, south −90, east 180,
    ///   north 90).
    pub fn validate(&self, storage_crs: &StorageCrs) -> Result<(), TilingViolation> {
        // Tiling5: the scheme CRS provably matches the storage CRS.
        let datastore_authority = storage_crs.authority();
        let provable_match = match (self.crs.split_once(':'), &datastore_authority) {
            (Some((authority, code)), Some(datastore))
                if !authority.is_empty() && !code.is_empty() =>
            {
                authority_ids_match(&(authority.to_owned(), code.to_owned()), datastore)
            }
            _ => false,
        };
        if !provable_match {
            return Err(TilingViolation::SchemeCrsMismatch {
                declared: self.crs.clone(),
                datastore: match &datastore_authority {
                    Some((authority, code)) => format!("{authority}:{code}"),
                    None => "unidentified".to_owned(),
                },
            });
        }

        // Tiling6: the scheme UoM is the storage CRS's coordinate unit.
        match crate::crs::wkt2::coordinate_units(storage_crs.horizontal())
            .into_iter()
            .next()
        {
            Some(unit) if unit.name.eq_ignore_ascii_case(&self.uom) => {}
            Some(unit) => {
                return Err(TilingViolation::SchemeUomMismatch {
                    scheme: self.uom.clone(),
                    crs_unit: unit.name,
                });
            }
            None => {
                return Err(TilingViolation::SchemeUomMismatch {
                    scheme: self.uom.clone(),
                    crs_unit: "unidentified".to_owned(),
                });
            }
        }

        // Tiling7: the extent covers the entire earth, exactly.
        let whole_earth = self.extent.west == -180.0
            && self.extent.south == -90.0
            && self.extent.east == 180.0
            && self.extent.north == 90.0;
        if !whole_earth {
            return Err(TilingViolation::IncompleteExtent {
                extent: format!(
                    "({},{},{},{})",
                    self.extent.west, self.extent.south, self.extent.east, self.extent.north
                ),
            });
        }

        Ok(())
    }

    /// SHOULD-level findings: Recommendation Tiling1
    /// (`/rec/core/tiling-extension`) prefers one of the two specified
    /// extension schemes. A custom [`Self::id`] yields one
    /// [`TilingWarning::NonExtensionScheme`]; either extension yields none.
    pub fn warnings(&self) -> Vec<TilingWarning> {
        match TilingSchemeId::parse(&self.id) {
            Ok(_) => Vec::new(),
            Err(_) => vec![TilingWarning::NonExtensionScheme {
                id: self.id.clone(),
            }],
        }
    }
}

/// Validates a tileset's metadata record (Requirements Tiling9/Tiling10,
/// `/req/core/tiling-tileset-metadata-standard` and
/// `/req/core/tiling-tileset-metadata-elements`): the record itself must be
/// valid resource metadata (Tiling9 conformance to the declared standard
/// holds by construction — tileset metadata rides the same record scheme
/// and encoding as every other resource), and the Keywords element is
/// mandatory for tilesets (ID, Title, Description are already mandatory in
/// the §7.9.4.2 baseline).
pub fn validate_tileset_metadata(record: &ResourceMetadata) -> Result<(), TilingViolation> {
    record.validate()?;
    if record.keywords.is_empty() {
        return Err(TilingViolation::MissingTilesetKeywords);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crs::StorageCrs;
    use crate::error::CdbError;
    use crate::metadata::Bbox;
    use crate::profiles::simulation::WGS84_2D_WKT;

    /// Recommendation Tiling1 /rec/core/tiling-extension — the scheme ids
    /// have fixed wire spellings; parse is exact and case-sensitive.
    /// (Deferred at Phase 14a: "no parse until Phase 9 fixes the wire
    /// spelling" — this is that fix.)
    #[test]
    fn tiling_scheme_id_parse_wire_spellings() {
        assert_eq!(
            TilingSchemeId::parse("CDB1GlobalGrid").unwrap(),
            TilingSchemeId::Cdb1GlobalGrid
        );
        assert_eq!(
            TilingSchemeId::parse("GNOSISGlobalGrid").unwrap(),
            TilingSchemeId::GnosisGlobalGrid
        );
        for bad in ["cdb1globalgrid", "CDB1GLOBALGRID", "X", ""] {
            assert!(matches!(
                TilingSchemeId::parse(bad),
                Err(TilingViolation::UnknownTilingScheme { .. })
            ));
        }
        // The relocated id keeps its profile-facing path.
        let _: crate::profiles::TilingSchemeId = TilingSchemeId::Cdb1GlobalGrid;
    }

    /// Crate taxonomy convention.
    #[test]
    fn tiling_violation_converts_to_cdb_error() {
        let err: CdbError = TilingViolation::UnknownTilingScheme { value: "x".into() }.into();
        assert!(matches!(err, CdbError::Tiling(_)));
    }

    fn wgs84() -> StorageCrs {
        StorageCrs::from_wkt(WGS84_2D_WKT).unwrap()
    }

    /// §7.10.2.4.2 Requirement Tiling5 /req/core/tiling-tilingscheme-crs —
    /// the scheme CRS must provably match the datastore storage CRS.
    #[test]
    fn req_core_tiling_tilingscheme_crs_matches_storage() {
        let crs = wgs84();
        let mut scheme = TilingScheme::cdb1_global_grid();
        assert!(scheme.validate(&crs).is_ok());
        scheme.crs = "epsg:4326".to_owned(); // case-insensitive match
        assert!(scheme.validate(&crs).is_ok());
        scheme.crs = "EPSG:4269".to_owned();
        assert!(matches!(
            scheme.validate(&crs),
            Err(TilingViolation::SchemeCrsMismatch { .. })
        ));
        scheme.crs = "not-an-authority".to_owned(); // malformed
        assert!(matches!(
            scheme.validate(&crs),
            Err(TilingViolation::SchemeCrsMismatch { .. })
        ));
    }

    /// §7.10.2.4.3 Requirement Tiling6 /req/core/tiling-tilingscheme-uom —
    /// the scheme UoM is the CRS's coordinate unit (decimal degrees for
    /// EPSG:4326); never mensuration.
    #[test]
    fn req_core_tiling_tilingscheme_uom_from_crs() {
        let crs = wgs84();
        let mut scheme = TilingScheme::cdb1_global_grid();
        scheme.uom = "DEGREE".to_owned(); // ASCII case-insensitive
        assert!(scheme.validate(&crs).is_ok());
        scheme.uom = "metre".to_owned();
        assert!(matches!(
            scheme.validate(&crs),
            Err(TilingViolation::SchemeUomMismatch { crs_unit, .. }) if crs_unit == "degree"
        ));
    }

    /// §7.10.2.4.4 Requirement Tiling7 /req/core/tiling-tilingscheme-extent
    /// — the scheme extent covers the entire earth, no gaps.
    #[test]
    fn req_core_tiling_tilingscheme_extent_whole_earth() {
        let crs = wgs84();
        let mut scheme = TilingScheme::cdb1_global_grid();
        scheme.extent = Bbox {
            west: -180.0,
            south: -60.0,
            east: 180.0,
            north: 60.0,
        };
        assert!(matches!(
            scheme.validate(&crs),
            Err(TilingViolation::IncompleteExtent { .. })
        ));
    }

    /// §7.10.2.7 Recommendation Tiling1 /rec/core/tiling-extension — the
    /// scheme SHOULD be one of the two specified extensions; a custom
    /// scheme warns but never errors (SHOULD is not SHALL).
    #[test]
    fn rec_core_tiling_extension_scheme_recommended() {
        let crs = wgs84();
        let mut scheme = TilingScheme::cdb1_global_grid();
        assert!(scheme.warnings().is_empty());
        scheme.id = "GNOSISGlobalGrid".to_owned();
        assert!(scheme.warnings().is_empty());
        scheme.id = "MyCustomGrid".to_owned();
        assert!(matches!(
            scheme.warnings().as_slice(),
            [TilingWarning::NonExtensionScheme { .. }]
        ));
        assert!(scheme.validate(&crs).is_ok());
    }

    /// §7.10.2.5 Requirement Tiling8 + §7.11.3.3 TCE3 — the canonical
    /// CDB1GlobalGrid definition: EPSG:4326, degrees, whole earth.
    #[test]
    fn tiling_scheme_cdb1_preset_validates() {
        let scheme = TilingScheme::cdb1_global_grid();
        assert_eq!(scheme.id, "CDB1GlobalGrid");
        assert_eq!(scheme.crs, "EPSG:4326");
        assert_eq!(scheme.uom, "degree");
        assert_eq!(
            (
                scheme.extent.west,
                scheme.extent.south,
                scheme.extent.east,
                scheme.extent.north
            ),
            (-180.0, -90.0, 180.0, 90.0)
        );
        assert!(scheme.validate(&wgs84()).is_ok());
        assert!(scheme.warnings().is_empty());
    }

    /// §7.10.2.6 Requirements Tiling9/Tiling10 — a tileset's metadata is a
    /// resource-metadata record (standard/encoding conformance by
    /// construction) with Keywords mandatory beyond the §7.9.4.2 baseline.
    #[test]
    fn req_core_tiling_tileset_metadata_elements() {
        use crate::metadata::ResourceMetadata;
        let mut record =
            ResourceMetadata::new("1", "RoadNetwork", "The CDB road and highway network");
        assert!(matches!(
            validate_tileset_metadata(&record),
            Err(TilingViolation::MissingTilesetKeywords)
        ));
        record.keywords = vec!["roads".into(), "streets".into(), "highways".into()];
        assert!(validate_tileset_metadata(&record).is_ok());
        // Delegation: an invalid record surfaces as a Metadata violation.
        let mut invalid = record.clone();
        invalid.title = String::new();
        assert!(matches!(
            validate_tileset_metadata(&invalid),
            Err(TilingViolation::Metadata(_))
        ));
    }

    /// §7.10.2.5 Requirement Tiling8 — a tiled datastore must carry the
    /// scheme definition in its global metadata.
    #[test]
    fn req_core_tiling_tilingscheme_required_when_tiled() {
        let mut global = crate::metadata::GlobalMetadata::builder()
            .id("G")
            .title("T")
            .description("D")
            .contact_point("ops@example.com")
            .language(crate::metadata::LanguageTag::new("en").unwrap())
            .standard(crate::metadata::MetadataStandard::Dcat)
            .encoding(crate::metadata::MetadataEncoding::Json)
            .uom(crate::metadata::UnitOfMeasure::Meters)
            .created(crate::metadata::temporal::parse_datetime("2026-08-02T00:00:00Z").unwrap())
            .build()
            .unwrap();
        assert!(matches!(
            TilingScheme::require(&global),
            Err(TilingViolation::MissingTilingScheme)
        ));
        global.tiling_scheme = Some(TilingScheme::cdb1_global_grid());
        assert_eq!(TilingScheme::require(&global).unwrap().id, "CDB1GlobalGrid");
    }
}
