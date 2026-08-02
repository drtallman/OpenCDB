//! Implements the abstract Tiling requirements class (spec §7.10) and hosts
//! the tiling-scheme identity shared with the extensions (§7.11–§7.12).
//!
//! The Abstract Tiling Requirements Module is optional but binding once a
//! datastore is tiled (Requirement Tiling1, `/req/core/tiling-rule`). It
//! depends on OGC Abstract Specification Topic 22 (Tiling2,
//! `/req/core/tiling-topic22` — the class listing's `tiling-model` slug is
//! the same rule) and on the CRS and Metadata core modules.
//! `TilingScheme::validate` and `validate_tileset_metadata` are the
//! module's rule enforcement (Tiling1/Tiling3) — the design-level
//! conformance pattern of CRS2 and Geom1.
//!
//! Draft quirks (keyed on meaning): the requirements-class URI is mislabeled
//! `/req/core/geometry-` in the document (copy-paste); slugs missing their
//! leading `/` are normalized here.

pub mod cdb1_grid;

use std::fmt;

use thiserror::Error;

use crate::metadata::MetadataViolation;

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

/// A violation of a SHALL requirement in the tiling module (spec §7.10–§7.11).
///
/// Intentionally not `Eq`: a later task adds a variant carrying an `f64`
/// bound, which precludes a total-equality derive.
#[derive(Debug, Error, Clone, PartialEq)]
#[non_exhaustive]
pub enum TilingViolation {
    /// A tiling-scheme identifier outside the closed vocabulary
    /// (Recommendation Tiling1, `/rec/core/tiling-extension`).
    #[error(
        "tiling scheme {value:?} is not CDB1GlobalGrid or GNOSISGlobalGrid (/rec/core/tiling-extension)"
    )]
    UnknownTilingScheme { value: String },
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CdbError;

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
}
