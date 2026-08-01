//! Implements the /req/core/geometry requirements class (spec §7.6).
//!
//! The CDB 2.0 Geometry Requirements Module: Simple Features geometry types
//! with GeoPackage-consistent type codes (Geom2), typed Z/M geometries
//! (codes 1001–1004 / 2001–2004), and validation of the units and CRS
//! association requirements (Geom3–Geom6). Codes above 7 are NOT compatible
//! with CDB 1.x code assignments (spec §7.6 note).

use std::fmt;

use thiserror::Error;

/// A violation of a SHALL requirement of the geometry module (§7.6).
/// The module has no warning type: §7.6 contains no SHOULD recommendations.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GeometryViolation {
    /// Requirement Geom2: only codes from the §7.6 table exist.
    #[error(
        "geometry type code {code} is not in the §7.6 table (violates /req/core/geometry-types)"
    )]
    UnknownGeometryCode { code: u16 },
}

/// Geometry type codes of spec §7.6.1 (Requirement Geom2,
/// `/req/core/geometry-types`). Consistent with GeoPackage; codes above 7
/// are NOT consistent with CDB 1.x. Closed enum — the spec table is the
/// authority. Codes 11–14 are usable only where an application profile
/// declares them as extensions ([`GeometryCode::is_extension`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GeometryCode {
    /// 0 — the abstract Simple Features root class.
    Geometry,
    /// 1
    Point,
    /// 2
    Linestring,
    /// 3
    Polygon,
    /// 4
    MultiPoint,
    /// 5
    MultiLinestring,
    /// 6
    MultiPolygon,
    /// 7
    GeometryCollection,
    /// 1001
    PointZ,
    /// 1002
    LinestringZ,
    /// 1003
    PolygonZ,
    /// 1004
    MultiPointZ,
    /// 2001
    PointM,
    /// 2002
    LinestringM,
    /// 2003
    PolygonM,
    /// 2004
    MultiPointM,
    /// 11 — extension (profile-declared only).
    MultiCurve,
    /// 12 — extension (profile-declared only).
    MultiSurface,
    /// 13 — extension (profile-declared only).
    Curve,
    /// 14 — extension (profile-declared only).
    Surface,
}

impl GeometryCode {
    /// Every code in the §7.6 table, in table order.
    pub const ALL: [GeometryCode; 20] = [
        GeometryCode::Geometry,
        GeometryCode::Point,
        GeometryCode::Linestring,
        GeometryCode::Polygon,
        GeometryCode::MultiPoint,
        GeometryCode::MultiLinestring,
        GeometryCode::MultiPolygon,
        GeometryCode::GeometryCollection,
        GeometryCode::PointZ,
        GeometryCode::LinestringZ,
        GeometryCode::PolygonZ,
        GeometryCode::MultiPointZ,
        GeometryCode::PointM,
        GeometryCode::LinestringM,
        GeometryCode::PolygonM,
        GeometryCode::MultiPointM,
        GeometryCode::MultiCurve,
        GeometryCode::MultiSurface,
        GeometryCode::Curve,
        GeometryCode::Surface,
    ];

    /// The numeric code from the §7.6 table.
    pub fn code(self) -> u16 {
        match self {
            GeometryCode::Geometry => 0,
            GeometryCode::Point => 1,
            GeometryCode::Linestring => 2,
            GeometryCode::Polygon => 3,
            GeometryCode::MultiPoint => 4,
            GeometryCode::MultiLinestring => 5,
            GeometryCode::MultiPolygon => 6,
            GeometryCode::GeometryCollection => 7,
            GeometryCode::PointZ => 1001,
            GeometryCode::LinestringZ => 1002,
            GeometryCode::PolygonZ => 1003,
            GeometryCode::MultiPointZ => 1004,
            GeometryCode::PointM => 2001,
            GeometryCode::LinestringM => 2002,
            GeometryCode::PolygonM => 2003,
            GeometryCode::MultiPointM => 2004,
            GeometryCode::MultiCurve => 11,
            GeometryCode::MultiSurface => 12,
            GeometryCode::Curve => 13,
            GeometryCode::Surface => 14,
        }
    }

    /// Parses a numeric code; unknown codes violate Geom2.
    pub fn from_code(code: u16) -> Result<GeometryCode, GeometryViolation> {
        GeometryCode::ALL
            .into_iter()
            .find(|c| c.code() == code)
            .ok_or(GeometryViolation::UnknownGeometryCode { code })
    }

    /// The spec-verbatim type name (§7.6.1 tables).
    pub fn name(self) -> &'static str {
        match self {
            GeometryCode::Geometry => "Geometry",
            GeometryCode::Point => "Point",
            GeometryCode::Linestring => "Linestring",
            GeometryCode::Polygon => "Polygon",
            GeometryCode::MultiPoint => "MultiPoint",
            GeometryCode::MultiLinestring => "MultiLinestring",
            GeometryCode::MultiPolygon => "MultiPolygon",
            GeometryCode::GeometryCollection => "GeometryCollection",
            GeometryCode::PointZ => "Point Z",
            GeometryCode::LinestringZ => "Linestring Z",
            GeometryCode::PolygonZ => "Polygon Z",
            GeometryCode::MultiPointZ => "MultiPoint Z",
            GeometryCode::PointM => "Point M",
            GeometryCode::LinestringM => "Linestring M",
            GeometryCode::PolygonM => "Polygon M",
            GeometryCode::MultiPointM => "MultiPoint M",
            GeometryCode::MultiCurve => "MULTICURVE",
            GeometryCode::MultiSurface => "MULTISURFACE",
            GeometryCode::Curve => "CURVE",
            GeometryCode::Surface => "SURFACE",
        }
    }

    /// Core codes: 0–7, 1001–1004, 2001–2004.
    pub fn is_core(self) -> bool {
        !self.is_extension()
    }

    /// Extension codes 11–14 — usable only where an application profile
    /// declares them (§7.6.1: "may also be specified for use as extensions").
    pub fn is_extension(self) -> bool {
        matches!(
            self,
            GeometryCode::MultiCurve
                | GeometryCode::MultiSurface
                | GeometryCode::Curve
                | GeometryCode::Surface
        )
    }
}

impl fmt::Display for GeometryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CdbError;

    /// §7.6.2.2 Requirement Geom2 /req/core/geometry-types — the code table
    /// is GeoPackage-consistent: 0–7 core, 1001–1004 Z, 2001–2004 M,
    /// 11–14 extension.
    #[test]
    fn req_core_geometry_types_codes_match_geopackage_table() {
        let expected: [(u16, &str); 20] = [
            (0, "Geometry"),
            (1, "Point"),
            (2, "Linestring"),
            (3, "Polygon"),
            (4, "MultiPoint"),
            (5, "MultiLinestring"),
            (6, "MultiPolygon"),
            (7, "GeometryCollection"),
            (1001, "Point Z"),
            (1002, "Linestring Z"),
            (1003, "Polygon Z"),
            (1004, "MultiPoint Z"),
            (2001, "Point M"),
            (2002, "Linestring M"),
            (2003, "Polygon M"),
            (2004, "MultiPoint M"),
            (11, "MULTICURVE"),
            (12, "MULTISURFACE"),
            (13, "CURVE"),
            (14, "SURFACE"),
        ];
        assert_eq!(GeometryCode::ALL.len(), expected.len());
        for (code, name) in expected {
            let parsed = GeometryCode::from_code(code).unwrap();
            assert_eq!(parsed.code(), code);
            assert_eq!(parsed.name(), name);
            assert_eq!(parsed.to_string(), name);
            assert!(GeometryCode::ALL.contains(&parsed));
        }
    }

    /// §7.6.2.2 Requirement Geom2 /req/core/geometry-types — codes outside
    /// the table are rejected.
    #[test]
    fn req_core_geometry_types_unknown_code_rejected() {
        for bad in [8u16, 10, 15, 999, 1005, 2005, 3001] {
            assert!(matches!(
                GeometryCode::from_code(bad),
                Err(GeometryViolation::UnknownGeometryCode { code }) if code == bad
            ));
        }
    }

    /// §7.6.1 note — codes 11–14 "may also be specified for use as
    /// extensions in any CDB 2.0 application profile"; they are not core.
    #[test]
    fn req_core_geometry_types_extension_codes_flagged() {
        for code in [11u16, 12, 13, 14] {
            let c = GeometryCode::from_code(code).unwrap();
            assert!(c.is_extension());
            assert!(!c.is_core());
        }
        for code in [0u16, 1, 7, 1001, 1004, 2001, 2004] {
            let c = GeometryCode::from_code(code).unwrap();
            assert!(c.is_core());
            assert!(!c.is_extension());
        }
    }

    /// Crate taxonomy convention: the module's violation family converts
    /// into the top-level `CdbError`.
    #[test]
    fn geometry_violation_converts_to_cdb_error() {
        let err: CdbError = GeometryViolation::UnknownGeometryCode { code: 999 }.into();
        assert!(matches!(err, CdbError::Geometry(_)));
    }
}
