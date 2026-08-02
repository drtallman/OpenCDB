//! Implements the /req/core/coverages- requirements class (spec §7.2).
//!
//! The CDB 2.0 Coverages module is abstract and optional, but binding once
//! coverage content is used (Requirement Coverages1). It depends on OGC
//! Abstract Specification Topic 6 / ISO 19123 (Coverages2) and on the CRS
//! and Metadata core modules. This module is metadata + rules only — no
//! raster decoding (encodings are application-profile business).
//! `validate_coverage_instance` and `DomainSet::validate` are the
//! module's rule enforcement (Coverages1/Coverages3), the same
//! design-level conformance pattern as CRS2 and Geom1.
//!
//! Requirement-box slugs are cited (`/req/core/coverage-…`); the class
//! listing's plural forms (`coverages-…`) are the same rules.

use std::fmt;

use thiserror::Error;

/// A violation of a SHALL requirement of the coverages module (§7.2).
/// Non-exhaustive: later tasks (domain set, coverage instance) add variants.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CoverageViolation {
    /// Requirement Coverages6-F (§7.2.6.3): a grid cell encoding must be one
    /// of the three spec wire spellings.
    #[error(
        "grid cell encoding {value:?} is not value-is-center|value-is-area|value-is-corner (violates /req/core/coverage-domainSet F)"
    )]
    UnknownGridCellEncoding { value: String },
    /// Requirement Coverages6-F (§7.2.6.3): a grid corner must be one of the
    /// four spec wire spellings.
    #[error(
        "grid corner {value:?} is not lower-left-corner|upper-left-corner|lower-right-corner|upper-right-corner (violates /req/core/coverage-domainSet F)"
    )]
    UnknownGridCorner { value: String },
}

/// How a value is assigned to a grid cell (Coverages6-F, §7.2.6.3).
/// Closed set for this Standard revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GridCellEncoding {
    /// "value-is-center" — the default: the value is the cell center.
    #[default]
    ValueIsCenter,
    /// "value-is-area" — the entire cell has the same value.
    ValueIsArea,
    /// "value-is-corner" — mesh-of-values, CDB 1.x style; REQUIRES a
    /// [`GridCorner`] stating which corner.
    ValueIsCorner,
}

impl GridCellEncoding {
    /// Every grid-cell encoding, in spec-listing order (Coverages6-F).
    pub const ALL: [GridCellEncoding; 3] = [
        GridCellEncoding::ValueIsCenter,
        GridCellEncoding::ValueIsArea,
        GridCellEncoding::ValueIsCorner,
    ];

    /// The spec wire spelling (Coverages6-F, §7.2.6.3).
    pub fn as_str(self) -> &'static str {
        match self {
            GridCellEncoding::ValueIsCenter => "value-is-center",
            GridCellEncoding::ValueIsArea => "value-is-area",
            GridCellEncoding::ValueIsCorner => "value-is-corner",
        }
    }

    /// Parses a wire spelling; unknown values violate Coverages6-F.
    pub fn parse(value: &str) -> Result<GridCellEncoding, CoverageViolation> {
        GridCellEncoding::ALL
            .into_iter()
            .find(|encoding| encoding.as_str() == value)
            .ok_or_else(|| CoverageViolation::UnknownGridCellEncoding {
                value: value.to_owned(),
            })
    }
}

/// Serializes as the spec wire string (e.g. "value-is-center").
impl serde::Serialize for GridCellEncoding {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// Deserializes from the spec wire string; unknown values error.
impl<'de> serde::Deserialize<'de> for GridCellEncoding {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        GridCellEncoding::parse(&value).map_err(serde::de::Error::custom)
    }
}

/// Displays as the spec wire string (e.g. "value-is-center").
impl fmt::Display for GridCellEncoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which corner of a grid cell a "value-is-corner" mesh value sits on
/// (Coverages6-F, §7.2.6.3). Closed set for this Standard revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GridCorner {
    /// "lower-left-corner"
    LowerLeft,
    /// "upper-left-corner"
    UpperLeft,
    /// "lower-right-corner"
    LowerRight,
    /// "upper-right-corner"
    UpperRight,
}

impl GridCorner {
    /// Every grid corner, in spec-listing order (Coverages6-F).
    pub const ALL: [GridCorner; 4] = [
        GridCorner::LowerLeft,
        GridCorner::UpperLeft,
        GridCorner::LowerRight,
        GridCorner::UpperRight,
    ];

    /// The spec wire spelling (Coverages6-F, §7.2.6.3).
    pub fn as_str(self) -> &'static str {
        match self {
            GridCorner::LowerLeft => "lower-left-corner",
            GridCorner::UpperLeft => "upper-left-corner",
            GridCorner::LowerRight => "lower-right-corner",
            GridCorner::UpperRight => "upper-right-corner",
        }
    }

    /// Parses a wire spelling; unknown values violate Coverages6-F.
    pub fn parse(value: &str) -> Result<GridCorner, CoverageViolation> {
        GridCorner::ALL
            .into_iter()
            .find(|corner| corner.as_str() == value)
            .ok_or_else(|| CoverageViolation::UnknownGridCorner {
                value: value.to_owned(),
            })
    }
}

/// Serializes as the spec wire string (e.g. "lower-left-corner").
impl serde::Serialize for GridCorner {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// Deserializes from the spec wire string; unknown values error.
impl<'de> serde::Deserialize<'de> for GridCorner {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        GridCorner::parse(&value).map_err(serde::de::Error::custom)
    }
}

/// Displays as the spec wire string (e.g. "lower-left-corner").
impl fmt::Display for GridCorner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CdbError;

    /// §7.2.6.3 / Coverages6-F — the grid enums carry the spec's exact
    /// wire spellings and reject unknown values.
    #[test]
    fn grid_enum_wire_spellings_roundtrip() {
        let encodings = [
            (GridCellEncoding::ValueIsCenter, "value-is-center"),
            (GridCellEncoding::ValueIsArea, "value-is-area"),
            (GridCellEncoding::ValueIsCorner, "value-is-corner"),
        ];
        assert_eq!(GridCellEncoding::ALL.len(), encodings.len());
        for (variant, wire) in encodings {
            assert_eq!(variant.as_str(), wire);
            assert_eq!(variant.to_string(), wire);
            assert_eq!(GridCellEncoding::parse(wire).unwrap(), variant);
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, format!("{wire:?}"));
            assert_eq!(
                serde_json::from_str::<GridCellEncoding>(&json).unwrap(),
                variant
            );
        }
        assert_eq!(GridCellEncoding::default(), GridCellEncoding::ValueIsCenter);
        assert!(matches!(
            GridCellEncoding::parse("value-is-random"),
            Err(CoverageViolation::UnknownGridCellEncoding { .. })
        ));

        let corners = [
            (GridCorner::LowerLeft, "lower-left-corner"),
            (GridCorner::UpperLeft, "upper-left-corner"),
            (GridCorner::LowerRight, "lower-right-corner"),
            (GridCorner::UpperRight, "upper-right-corner"),
        ];
        assert_eq!(GridCorner::ALL.len(), corners.len());
        for (variant, wire) in corners {
            assert_eq!(variant.as_str(), wire);
            assert_eq!(GridCorner::parse(wire).unwrap(), variant);
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(serde_json::from_str::<GridCorner>(&json).unwrap(), variant);
        }
        assert!(matches!(
            GridCorner::parse("center"),
            Err(CoverageViolation::UnknownGridCorner { .. })
        ));
    }

    /// Crate taxonomy convention: the module's violation family converts
    /// into the top-level `CdbError`.
    #[test]
    fn coverage_violation_converts_to_cdb_error() {
        let err: CdbError = CoverageViolation::UnknownGridCorner { value: "x".into() }.into();
        assert!(matches!(err, CdbError::Coverage(_)));
    }
}
