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
    /// §7.6.1 Z table (Geom2): every coordinate carries a z value.
    #[error(
        "z values for {part}: expected {expected}, found {found} (violates /req/core/geometry-types)"
    )]
    ZLengthMismatch {
        expected: usize,
        found: usize,
        part: String,
    },
    /// §7.6.1 M table (Geom2): every coordinate carries an m value.
    #[error(
        "m values for {part}: expected {expected}, found {found} (violates /req/core/geometry-types)"
    )]
    MLengthMismatch {
        expected: usize,
        found: usize,
        part: String,
    },
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

/// Which extra-coordinate family a length check belongs to, selecting the
/// [`GeometryViolation`] variant raised on mismatch (§7.6.1 Z/M tables).
#[derive(Clone, Copy)]
enum Extra {
    Z,
    M,
}

/// Verifies that a geometry part carries exactly one extra-coordinate value
/// per xy coordinate (Requirement Geom2, §7.6.1). `part` names the offending
/// ring or coordinate list in the resulting violation message.
fn check_len(
    expected: usize,
    found: usize,
    part: &str,
    extra: Extra,
) -> Result<(), GeometryViolation> {
    if expected == found {
        return Ok(());
    }
    let part = part.to_owned();
    Err(match extra {
        Extra::Z => GeometryViolation::ZLengthMismatch {
            expected,
            found,
            part,
        },
        Extra::M => GeometryViolation::MLengthMismatch {
            expected,
            found,
            part,
        },
    })
}

/// Point with a z coordinate (code 1001, §7.6.1 Z table).
#[derive(Debug, Clone, PartialEq)]
pub struct PointZ {
    xy: geo_types::Point<f64>,
    z: f64,
}

impl PointZ {
    /// A point has exactly one coordinate, so construction is infallible.
    pub fn new(xy: geo_types::Point<f64>, z: f64) -> PointZ {
        PointZ { xy, z }
    }
    /// The horizontal (xy) coordinate.
    pub fn xy(&self) -> &geo_types::Point<f64> {
        &self.xy
    }
    /// The z coordinate value.
    pub fn z(&self) -> f64 {
        self.z
    }
}

/// Linestring with a z value per coordinate (code 1002, §7.6.1 Z table).
#[derive(Debug, Clone, PartialEq)]
pub struct LineStringZ {
    xy: geo_types::LineString<f64>,
    z: Vec<f64>,
}

impl LineStringZ {
    /// Fails with [`GeometryViolation::ZLengthMismatch`] (part `"coordinates"`)
    /// unless there is exactly one z value per xy coordinate.
    pub fn new(
        xy: geo_types::LineString<f64>,
        z: Vec<f64>,
    ) -> Result<LineStringZ, GeometryViolation> {
        check_len(xy.0.len(), z.len(), "coordinates", Extra::Z)?;
        Ok(LineStringZ { xy, z })
    }
    /// The horizontal (xy) coordinates.
    pub fn xy(&self) -> &geo_types::LineString<f64> {
        &self.xy
    }
    /// The per-coordinate z values.
    pub fn z(&self) -> &[f64] {
        &self.z
    }
}

/// Polygon with a z value per coordinate on every ring (code 1003,
/// §7.6.1 Z table). Ring lengths include geo-types' auto-closed coordinate.
#[derive(Debug, Clone, PartialEq)]
pub struct PolygonZ {
    xy: geo_types::Polygon<f64>,
    z_exterior: Vec<f64>,
    z_interiors: Vec<Vec<f64>>,
}

impl PolygonZ {
    /// Fails with [`GeometryViolation::ZLengthMismatch`] if the interior-ring
    /// count differs (part `"interior rings"`), the exterior ring length
    /// differs (part `"exterior"`), or any interior ring length differs
    /// (part `"interior <i>"`), checked in that order.
    pub fn new(
        xy: geo_types::Polygon<f64>,
        z_exterior: Vec<f64>,
        z_interiors: Vec<Vec<f64>>,
    ) -> Result<PolygonZ, GeometryViolation> {
        check_len(
            xy.interiors().len(),
            z_interiors.len(),
            "interior rings",
            Extra::Z,
        )?;
        check_len(
            xy.exterior().0.len(),
            z_exterior.len(),
            "exterior",
            Extra::Z,
        )?;
        for (i, (ring, z)) in xy.interiors().iter().zip(&z_interiors).enumerate() {
            check_len(ring.0.len(), z.len(), &format!("interior {i}"), Extra::Z)?;
        }
        Ok(PolygonZ {
            xy,
            z_exterior,
            z_interiors,
        })
    }
    /// The horizontal (xy) polygon.
    pub fn xy(&self) -> &geo_types::Polygon<f64> {
        &self.xy
    }
    /// The z values along the exterior ring.
    pub fn z_exterior(&self) -> &[f64] {
        &self.z_exterior
    }
    /// The z values for each interior ring, in ring order.
    pub fn z_interiors(&self) -> &[Vec<f64>] {
        &self.z_interiors
    }
}

/// MultiPoint with a z value per point (code 1004, §7.6.1 Z table).
#[derive(Debug, Clone, PartialEq)]
pub struct MultiPointZ {
    xy: geo_types::MultiPoint<f64>,
    z: Vec<f64>,
}

impl MultiPointZ {
    /// Fails with [`GeometryViolation::ZLengthMismatch`] (part `"coordinates"`)
    /// unless there is exactly one z value per point.
    pub fn new(
        xy: geo_types::MultiPoint<f64>,
        z: Vec<f64>,
    ) -> Result<MultiPointZ, GeometryViolation> {
        check_len(xy.0.len(), z.len(), "coordinates", Extra::Z)?;
        Ok(MultiPointZ { xy, z })
    }
    /// The horizontal (xy) points.
    pub fn xy(&self) -> &geo_types::MultiPoint<f64> {
        &self.xy
    }
    /// The per-point z values.
    pub fn z(&self) -> &[f64] {
        &self.z
    }
}

/// Point with an m coordinate (code 2001, §7.6.1 M table).
#[derive(Debug, Clone, PartialEq)]
pub struct PointM {
    xy: geo_types::Point<f64>,
    m: f64,
}

impl PointM {
    /// A point has exactly one coordinate, so construction is infallible.
    pub fn new(xy: geo_types::Point<f64>, m: f64) -> PointM {
        PointM { xy, m }
    }
    /// The horizontal (xy) coordinate.
    pub fn xy(&self) -> &geo_types::Point<f64> {
        &self.xy
    }
    /// The m coordinate value.
    pub fn m(&self) -> f64 {
        self.m
    }
}

/// Linestring with an m value per coordinate (code 2002, §7.6.1 M table).
#[derive(Debug, Clone, PartialEq)]
pub struct LineStringM {
    xy: geo_types::LineString<f64>,
    m: Vec<f64>,
}

impl LineStringM {
    /// Fails with [`GeometryViolation::MLengthMismatch`] (part `"coordinates"`)
    /// unless there is exactly one m value per xy coordinate.
    pub fn new(
        xy: geo_types::LineString<f64>,
        m: Vec<f64>,
    ) -> Result<LineStringM, GeometryViolation> {
        check_len(xy.0.len(), m.len(), "coordinates", Extra::M)?;
        Ok(LineStringM { xy, m })
    }
    /// The horizontal (xy) coordinates.
    pub fn xy(&self) -> &geo_types::LineString<f64> {
        &self.xy
    }
    /// The per-coordinate m values.
    pub fn m(&self) -> &[f64] {
        &self.m
    }
}

/// Polygon with an m value per coordinate on every ring (code 2003,
/// §7.6.1 M table). Ring lengths include geo-types' auto-closed coordinate.
#[derive(Debug, Clone, PartialEq)]
pub struct PolygonM {
    xy: geo_types::Polygon<f64>,
    m_exterior: Vec<f64>,
    m_interiors: Vec<Vec<f64>>,
}

impl PolygonM {
    /// Fails with [`GeometryViolation::MLengthMismatch`] if the interior-ring
    /// count differs (part `"interior rings"`), the exterior ring length
    /// differs (part `"exterior"`), or any interior ring length differs
    /// (part `"interior <i>"`), checked in that order.
    pub fn new(
        xy: geo_types::Polygon<f64>,
        m_exterior: Vec<f64>,
        m_interiors: Vec<Vec<f64>>,
    ) -> Result<PolygonM, GeometryViolation> {
        check_len(
            xy.interiors().len(),
            m_interiors.len(),
            "interior rings",
            Extra::M,
        )?;
        check_len(
            xy.exterior().0.len(),
            m_exterior.len(),
            "exterior",
            Extra::M,
        )?;
        for (i, (ring, m)) in xy.interiors().iter().zip(&m_interiors).enumerate() {
            check_len(ring.0.len(), m.len(), &format!("interior {i}"), Extra::M)?;
        }
        Ok(PolygonM {
            xy,
            m_exterior,
            m_interiors,
        })
    }
    /// The horizontal (xy) polygon.
    pub fn xy(&self) -> &geo_types::Polygon<f64> {
        &self.xy
    }
    /// The m values along the exterior ring.
    pub fn m_exterior(&self) -> &[f64] {
        &self.m_exterior
    }
    /// The m values for each interior ring, in ring order.
    pub fn m_interiors(&self) -> &[Vec<f64>] {
        &self.m_interiors
    }
}

/// MultiPoint with an m value per point (code 2004, §7.6.1 M table).
#[derive(Debug, Clone, PartialEq)]
pub struct MultiPointM {
    xy: geo_types::MultiPoint<f64>,
    m: Vec<f64>,
}

impl MultiPointM {
    /// Fails with [`GeometryViolation::MLengthMismatch`] (part `"coordinates"`)
    /// unless there is exactly one m value per point.
    pub fn new(
        xy: geo_types::MultiPoint<f64>,
        m: Vec<f64>,
    ) -> Result<MultiPointM, GeometryViolation> {
        check_len(xy.0.len(), m.len(), "coordinates", Extra::M)?;
        Ok(MultiPointM { xy, m })
    }
    /// The horizontal (xy) points.
    pub fn xy(&self) -> &geo_types::MultiPoint<f64> {
        &self.xy
    }
    /// The per-point m values.
    pub fn m(&self) -> &[f64] {
        &self.m
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

    /// §7.6.1 Z/M tables (Requirement Geom2) — every coordinate has an
    /// associated z/m value, so lengths must match, per ring for polygons
    /// (geo-types ring lengths include the auto-closed duplicate coordinate).
    #[test]
    fn req_core_geometry_zvalue_length_invariants() {
        use geo_types::{LineString, MultiPoint, Point, Polygon};

        let line = LineString::from(vec![(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)]);
        assert!(LineStringZ::new(line.clone(), vec![10.0, 11.0, 12.0]).is_ok());
        assert!(matches!(
            LineStringZ::new(line.clone(), vec![10.0]),
            Err(GeometryViolation::ZLengthMismatch {
                expected: 3,
                found: 1,
                ..
            })
        ));
        assert!(matches!(
            LineStringM::new(line.clone(), vec![10.0]),
            Err(GeometryViolation::MLengthMismatch {
                expected: 3,
                found: 1,
                ..
            })
        ));

        // Polygon::new auto-closes rings: 3 distinct coords -> 4 ring coords.
        let ring = LineString::from(vec![(0.0, 0.0), (4.0, 0.0), (0.0, 4.0)]);
        let poly = Polygon::new(ring, vec![]);
        let ring_len = poly.exterior().0.len(); // 4 including closing coord
        assert!(PolygonZ::new(poly.clone(), vec![1.0; ring_len], vec![]).is_ok());
        assert!(matches!(
            PolygonZ::new(poly.clone(), vec![1.0; ring_len - 1], vec![]),
            Err(GeometryViolation::ZLengthMismatch { .. })
        ));
        // Interior-ring COUNT mismatch is also a violation.
        assert!(matches!(
            PolygonZ::new(poly.clone(), vec![1.0; ring_len], vec![vec![1.0]]),
            Err(GeometryViolation::ZLengthMismatch { .. })
        ));

        let mp = MultiPoint::from(vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)]);
        assert!(MultiPointZ::new(mp.clone(), vec![5.0, 6.0]).is_ok());
        assert!(MultiPointM::new(mp, vec![5.0]).is_err());

        // PointZ/PointM are infallible: exactly one coordinate, one value.
        let pz = PointZ::new(Point::new(1.0, 2.0), 30.0);
        assert_eq!(pz.z(), 30.0);
        let pm = PointM::new(Point::new(1.0, 2.0), 0.5);
        assert_eq!(pm.m(), 0.5);
    }
}
