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

use serde::{Deserialize, Serialize};
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

/// The domainSet metadata of a coverage instance (Requirement Coverages6,
/// §7.2.6). Open-fields serde struct: per the §7.2.6 NOTE, elements that
/// have defaults are assumed when unspecified. No `Eq` — it carries f64
/// fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DomainSet {
    /// Coverages6-A: UCUM-style units of the coverage's RANGE VALUES —
    /// distinct from Metadata8's and Geom4's uoms (the three-uom table:
    /// this is a free-form UCUM code string, NOT the metadata
    /// `UnitOfMeasure` enum). Mandatory, non-empty.
    pub uom: String,
    /// Coverages6-B: smallest meaningful value. Default 1.
    #[serde(default = "default_one")]
    pub precision: f64,
    /// Coverages6-C: multiple relative to the uom. Default 1.
    #[serde(default = "default_one")]
    pub scale: f64,
    /// Coverages6-D: offset to the 0 value. Default 0.
    #[serde(default)]
    pub offset: f64,
    /// Coverages6-E: the NULL sentinel; `None` = the coverage has no null
    /// value (approved interpretation — the spec assigns no default and no
    /// mandatory line, and a complete coverage has no null value).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_null: Option<f64>,
    /// Coverages6-F. Default value-is-center.
    #[serde(default)]
    pub grid_cell_encoding: GridCellEncoding,
    /// Coverages6-F: REQUIRED when `grid_cell_encoding` is value-is-corner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub which_corner: Option<GridCorner>,
    /// Coverages6-G (§7.2.6.4's `field_name` is the same concept). Default
    /// "Height" (i.e. elevation).
    #[serde(default = "default_field_type")]
    pub field_type: String,
    /// Coverages6-H: required iff `field_type` is not "Height" (§7.2.6.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantity_definition: Option<String>,
}

/// Serde default for `precision` and `scale` (Coverages6-B/C): 1.
fn default_one() -> f64 {
    1.0
}

/// Serde default for `field_type` (Coverages6-G): "Height".
fn default_field_type() -> String {
    "Height".to_owned()
}

impl DomainSet {
    /// A `DomainSet` for `uom` with every other element at its spec default
    /// (Coverages6-B/C/D/F/G + the §7.2.6 NOTE).
    pub fn new(uom: impl Into<String>) -> DomainSet {
        DomainSet {
            uom: uom.into(),
            precision: default_one(),
            scale: default_one(),
            offset: 0.0,
            data_null: None,
            grid_cell_encoding: GridCellEncoding::ValueIsCenter,
            which_corner: None,
            field_type: default_field_type(),
            quantity_definition: None,
        }
    }

    /// Decodes a raw coverage value (§7.2.6.2): scale and offset apply to
    /// values but NEVER to `data_null`. Returns `None` iff `raw` is the null
    /// sentinel (`data_null == Some(n)` and `raw == n`), else
    /// `Some(raw * scale + offset)`.
    ///
    /// The sentinel test is an f64 `==`, so a `NaN` `data_null` matches
    /// nothing (`NaN != NaN`) — documented, not "fixed": a NaN sentinel is
    /// not a value this decode can recognize.
    pub fn decode_value(&self, raw: f64) -> Option<f64> {
        if self.data_null == Some(raw) {
            None
        } else {
            Some(raw * self.scale + self.offset)
        }
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

    /// §7.2.6 Requirement Coverages6 (B/C/D/F/G) + its NOTE — elements
    /// with defaults are assumed when unspecified: a domainSet carrying
    /// only `uom` parses to the spec defaults.
    #[test]
    fn req_core_coverage_domainset_defaults() {
        let ds: DomainSet = serde_json::from_str(r#"{"uom":"m"}"#).unwrap();
        assert_eq!(ds.uom, "m");
        assert_eq!(ds.precision, 1.0);
        assert_eq!(ds.scale, 1.0);
        assert_eq!(ds.offset, 0.0);
        assert_eq!(ds.data_null, None);
        assert_eq!(ds.grid_cell_encoding, GridCellEncoding::ValueIsCenter);
        assert_eq!(ds.which_corner, None);
        assert_eq!(ds.field_type, "Height");
        assert_eq!(ds.quantity_definition, None);
        // new() mirrors the same defaults, and absent optionals stay off
        // the wire.
        assert_eq!(DomainSet::new("m"), ds);
        let json = serde_json::to_string(&ds).unwrap();
        assert!(!json.contains("data_null") && !json.contains("which_corner"));
    }

    /// §7.2.6.2 — scale and offset apply to values, NEVER to data_null.
    #[test]
    fn req_core_coverage_scale_offset_never_applies_to_data_null() {
        let mut ds = DomainSet::new("m");
        ds.scale = 2.0;
        ds.offset = 10.0;
        ds.data_null = Some(-32767.0);
        assert_eq!(ds.decode_value(-32767.0), None); // null sentinel: untouched
        assert_eq!(ds.decode_value(5.0), Some(20.0)); // 5*2+10
        // Defaults are the identity mapping.
        let identity = DomainSet::new("m");
        assert_eq!(identity.decode_value(7.5), Some(7.5));
        // Without a data_null, every raw value decodes.
        let mut no_null = DomainSet::new("m");
        no_null.scale = 2.0;
        assert_eq!(no_null.decode_value(-32767.0), Some(-65534.0));
    }
}
