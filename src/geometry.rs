//! Implements the /req/core/geometry requirements class (spec §7.6).
//!
//! The CDB 2.0 Geometry Requirements Module: Simple Features geometry types
//! with GeoPackage-consistent type codes (Geom2), typed Z/M geometries
//! (codes 1001–1004 / 2001–2004), and validation of the units and CRS
//! association requirements (Geom3–Geom6). Codes above 7 are NOT compatible
//! with CDB 1.x code assignments (spec §7.6 note).

use std::fmt;

use thiserror::Error;

use crate::crs::{StorageCrs, authority_ids_match};
use crate::metadata::{GlobalMetadata, ResourceMetadata, UnitOfMeasure};

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
    /// §7.6.3 Requirement Geom3 (/req/core/geometry-zvalue; the requirement
    /// box's `geometry-zcoordinate` slug is the same rule).
    #[error(
        "geometry has z coordinates but the datastore declares no z unit of measure (violates /req/core/geometry-zvalue)"
    )]
    MissingZUom,
    /// §7.6.3 Requirement Geom4.
    #[error(
        "geometry has m values but the dataset metadata declares no m unit of measure (violates /req/core/geometry-mvalue)"
    )]
    MissingMUom,
    /// §7.6.3/§7.6.4 Requirements Geom5/Geom6.
    #[error(
        "geometry declares CRS {declared} but the datastore CRS is {datastore} (violates /req/core/geometry-coordinates)"
    )]
    ForeignCrs { declared: String, datastore: String },
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

/// A Simple Features geometry value (Requirement Geom1,
/// `/req/core/geometry-model`, §7.6.2.1). The CDB 2.0 geometry model *is*
/// OGC Simple Feature Access, so the seven planar (xy) types wrap
/// [`geo_types`] directly and the eight typed Z/M geometries wrap the structs
/// of this module. Code 0 (`Geometry`, the abstract Simple Features root) has
/// no variant of its own — it is represented by this enum as a whole.
#[derive(Debug, Clone, PartialEq)]
pub enum CdbGeometry {
    /// Code 1 — a planar point.
    Point(geo_types::Point<f64>),
    /// Code 2 — a planar linestring.
    LineString(geo_types::LineString<f64>),
    /// Code 3 — a planar polygon.
    Polygon(geo_types::Polygon<f64>),
    /// Code 4 — planar points.
    MultiPoint(geo_types::MultiPoint<f64>),
    /// Code 5 — planar linestrings.
    MultiLineString(geo_types::MultiLineString<f64>),
    /// Code 6 — planar polygons.
    MultiPolygon(geo_types::MultiPolygon<f64>),
    /// Code 7 — a heterogeneous collection; members may themselves be Z/M.
    GeometryCollection(Vec<CdbGeometry>),
    /// Code 1001 — a point carrying a z coordinate.
    PointZ(PointZ),
    /// Code 1002 — a linestring carrying z values.
    LineStringZ(LineStringZ),
    /// Code 1003 — a polygon carrying z values.
    PolygonZ(PolygonZ),
    /// Code 1004 — a multipoint carrying z values.
    MultiPointZ(MultiPointZ),
    /// Code 2001 — a point carrying an m coordinate.
    PointM(PointM),
    /// Code 2002 — a linestring carrying m values.
    LineStringM(LineStringM),
    /// Code 2003 — a polygon carrying m values.
    PolygonM(PolygonM),
    /// Code 2004 — a multipoint carrying m values.
    MultiPointM(MultiPointM),
}

impl CdbGeometry {
    /// The §7.6.1 type code (Geom2) of this geometry. A collection is always
    /// [`GeometryCode::GeometryCollection`], regardless of its members.
    pub fn geometry_code(&self) -> GeometryCode {
        match self {
            CdbGeometry::Point(_) => GeometryCode::Point,
            CdbGeometry::LineString(_) => GeometryCode::Linestring,
            CdbGeometry::Polygon(_) => GeometryCode::Polygon,
            CdbGeometry::MultiPoint(_) => GeometryCode::MultiPoint,
            CdbGeometry::MultiLineString(_) => GeometryCode::MultiLinestring,
            CdbGeometry::MultiPolygon(_) => GeometryCode::MultiPolygon,
            CdbGeometry::GeometryCollection(_) => GeometryCode::GeometryCollection,
            CdbGeometry::PointZ(_) => GeometryCode::PointZ,
            CdbGeometry::LineStringZ(_) => GeometryCode::LinestringZ,
            CdbGeometry::PolygonZ(_) => GeometryCode::PolygonZ,
            CdbGeometry::MultiPointZ(_) => GeometryCode::MultiPointZ,
            CdbGeometry::PointM(_) => GeometryCode::PointM,
            CdbGeometry::LineStringM(_) => GeometryCode::LinestringM,
            CdbGeometry::PolygonM(_) => GeometryCode::PolygonM,
            CdbGeometry::MultiPointM(_) => GeometryCode::MultiPointM,
        }
    }

    /// Whether this geometry carries z coordinates — true for a Z variant or
    /// a collection with any (recursively) Z member.
    pub fn has_z(&self) -> bool {
        match self {
            CdbGeometry::PointZ(_)
            | CdbGeometry::LineStringZ(_)
            | CdbGeometry::PolygonZ(_)
            | CdbGeometry::MultiPointZ(_) => true,
            CdbGeometry::GeometryCollection(members) => members.iter().any(CdbGeometry::has_z),
            _ => false,
        }
    }

    /// Whether this geometry carries m coordinates — true for an M variant or
    /// a collection with any (recursively) M member.
    pub fn has_m(&self) -> bool {
        match self {
            CdbGeometry::PointM(_)
            | CdbGeometry::LineStringM(_)
            | CdbGeometry::PolygonM(_)
            | CdbGeometry::MultiPointM(_) => true,
            CdbGeometry::GeometryCollection(members) => members.iter().any(CdbGeometry::has_m),
            _ => false,
        }
    }

    /// The lossless planar ([`geo_types::Geometry`]) view, or `None` when any
    /// z/m coordinate would be dropped. The seven xy variants convert
    /// directly; a collection converts only when every member does; Z/M
    /// variants always yield `None`. Coordinates are never dropped silently.
    pub fn into_xy(self) -> Option<geo_types::Geometry<f64>> {
        match self {
            CdbGeometry::Point(p) => Some(geo_types::Geometry::Point(p)),
            CdbGeometry::LineString(ls) => Some(geo_types::Geometry::LineString(ls)),
            CdbGeometry::Polygon(poly) => Some(geo_types::Geometry::Polygon(poly)),
            CdbGeometry::MultiPoint(mp) => Some(geo_types::Geometry::MultiPoint(mp)),
            CdbGeometry::MultiLineString(mls) => Some(geo_types::Geometry::MultiLineString(mls)),
            CdbGeometry::MultiPolygon(mpoly) => Some(geo_types::Geometry::MultiPolygon(mpoly)),
            CdbGeometry::GeometryCollection(members) => {
                let converted = members
                    .into_iter()
                    .map(CdbGeometry::into_xy)
                    .collect::<Option<Vec<_>>>()?;
                Some(geo_types::Geometry::GeometryCollection(
                    geo_types::GeometryCollection(converted),
                ))
            }
            CdbGeometry::PointZ(_)
            | CdbGeometry::LineStringZ(_)
            | CdbGeometry::PolygonZ(_)
            | CdbGeometry::MultiPointZ(_)
            | CdbGeometry::PointM(_)
            | CdbGeometry::LineStringM(_)
            | CdbGeometry::PolygonM(_)
            | CdbGeometry::MultiPointM(_) => None,
        }
    }

    /// Validates this geometry's units and CRS association against the
    /// datastore it lives in (§7.6.3–§7.6.4, Requirements Geom3–Geom6),
    /// returning the first violation encountered (checks run CRS → z → m).
    ///
    /// `source_crs` is an OPTIONAL CRS claim carried by the geometry. `None`
    /// means the geometry is associated with the containing datastore's CRS
    /// (the normal case) and the CRS check passes. `Some(claim)` must PROVABLY
    /// identify the datastore CRS: Requirement Geom5
    /// (`/req/core/geometry-coordinates`) forbids a foreign or unverifiable
    /// CRS, so a claim is a [`GeometryViolation::ForeignCrs`] when the
    /// datastore CRS differs from it or is unidentified (rendered
    /// `"unidentified"`). Authority and code are compared ASCII
    /// case-insensitively via `authority_ids_match`.
    ///
    /// Geom5-B/Geom6-B (`/req/core/geometry-collection-srs`) hold by
    /// construction: a [`CdbGeometry`] value carries no per-member CRS, so a
    /// collection has a single CRS. No extra recursion is needed here — the
    /// unit checks reach collection members because
    /// [`CdbGeometry::has_z`]/[`CdbGeometry::has_m`] are themselves recursive.
    /// z units come from the global metadata (Geom3,
    /// [`GeometryViolation::MissingZUom`]); m units from the dataset metadata
    /// (Geom4, [`GeometryViolation::MissingMUom`]).
    pub fn validate_in(
        &self,
        ctx: &GeometryContext,
        source_crs: Option<&(String, String)>,
    ) -> Result<(), GeometryViolation> {
        if let Some(claim) = source_crs {
            match &ctx.datastore_crs {
                Some(ds) if authority_ids_match(ds, claim) => {}
                other => {
                    return Err(GeometryViolation::ForeignCrs {
                        declared: format!("{}:{}", claim.0, claim.1),
                        datastore: other
                            .as_ref()
                            .map(|d| format!("{}:{}", d.0, d.1))
                            .unwrap_or_else(|| "unidentified".to_owned()),
                    });
                }
            }
        }
        if self.has_z() && ctx.z_uom.is_none() {
            return Err(GeometryViolation::MissingZUom);
        }
        if self.has_m() && ctx.m_uom.is_none() {
            return Err(GeometryViolation::MissingMUom);
        }
        Ok(())
    }
}

/// The datastore facts a geometry is validated against (§7.6.3–§7.6.4,
/// Requirements Geom3–Geom6): the datastore CRS identity for the Geom5
/// association check, the global z unit of measure (Geom3), and the dataset
/// m unit of measure (Geom4). Construct one from explicit parts with
/// [`GeometryContext::new`], or from datastore metadata with
/// [`GeometryContext::from_datastore`]. Fields are private: a context is only
/// ever consumed by [`CdbGeometry::validate_in`].
#[derive(Debug, Clone)]
pub struct GeometryContext {
    /// The datastore CRS as an `(authority, code)` pair (e.g.
    /// `("EPSG", "4326")`), or `None` when the CRS carries no identifier —
    /// an "unidentified" datastore CRS against which no claim is provable.
    datastore_crs: Option<(String, String)>,
    /// The global-metadata z unit of measure (Geom3), or `None` when none is
    /// declared — required whenever a validated geometry has z coordinates.
    z_uom: Option<UnitOfMeasure>,
    /// The dataset-metadata m unit of measure (Geom4), or `None` when none is
    /// declared — required whenever a validated geometry has m values.
    m_uom: Option<UnitOfMeasure>,
}

impl GeometryContext {
    /// Builds a context from explicit parts: the datastore CRS `(authority,
    /// code)` pair for the Geom5 association check, the global z unit of
    /// measure (Geom3), and the dataset m unit of measure (Geom4). Each part
    /// is optional; a `None` unit fails validation only for a geometry that
    /// actually carries the corresponding z/m coordinates.
    pub fn new(
        datastore_crs: Option<(String, String)>,
        z_uom: Option<UnitOfMeasure>,
        m_uom: Option<UnitOfMeasure>,
    ) -> GeometryContext {
        GeometryContext {
            datastore_crs,
            z_uom,
            m_uom,
        }
    }

    /// Builds a context from datastore metadata: the datastore CRS identity
    /// via [`StorageCrs::authority`] (Geom5), the z unit of measure from the
    /// datastore-wide global metadata (Geom3, `GlobalMetadata::uom`), and the
    /// m unit of measure from the dataset (resource) metadata when one is
    /// supplied and declares it (Geom4, `ResourceMetadata::uom`).
    pub fn from_datastore(
        crs: &StorageCrs,
        global: &GlobalMetadata,
        resource: Option<&ResourceMetadata>,
    ) -> GeometryContext {
        GeometryContext {
            datastore_crs: crs.authority(),
            z_uom: Some(global.uom),
            m_uom: resource.and_then(|r| r.uom),
        }
    }
}

/// Wraps a planar geo-types `Point` directly as [`CdbGeometry::Point`].
impl From<geo_types::Point<f64>> for CdbGeometry {
    fn from(value: geo_types::Point<f64>) -> CdbGeometry {
        CdbGeometry::Point(value)
    }
}

/// Wraps a planar geo-types `LineString` directly as [`CdbGeometry::LineString`].
impl From<geo_types::LineString<f64>> for CdbGeometry {
    fn from(value: geo_types::LineString<f64>) -> CdbGeometry {
        CdbGeometry::LineString(value)
    }
}

/// Wraps a planar geo-types `Polygon` directly as [`CdbGeometry::Polygon`].
impl From<geo_types::Polygon<f64>> for CdbGeometry {
    fn from(value: geo_types::Polygon<f64>) -> CdbGeometry {
        CdbGeometry::Polygon(value)
    }
}

/// Wraps a planar geo-types `MultiPoint` directly as [`CdbGeometry::MultiPoint`].
impl From<geo_types::MultiPoint<f64>> for CdbGeometry {
    fn from(value: geo_types::MultiPoint<f64>) -> CdbGeometry {
        CdbGeometry::MultiPoint(value)
    }
}

/// Wraps a planar geo-types `MultiLineString` directly as
/// [`CdbGeometry::MultiLineString`].
impl From<geo_types::MultiLineString<f64>> for CdbGeometry {
    fn from(value: geo_types::MultiLineString<f64>) -> CdbGeometry {
        CdbGeometry::MultiLineString(value)
    }
}

/// Wraps a planar geo-types `MultiPolygon` directly as
/// [`CdbGeometry::MultiPolygon`].
impl From<geo_types::MultiPolygon<f64>> for CdbGeometry {
    fn from(value: geo_types::MultiPolygon<f64>) -> CdbGeometry {
        CdbGeometry::MultiPolygon(value)
    }
}

/// Converts every member recursively (losslessly, via
/// `From<geo_types::Geometry>`) into a [`CdbGeometry::GeometryCollection`].
impl From<geo_types::GeometryCollection<f64>> for CdbGeometry {
    fn from(value: geo_types::GeometryCollection<f64>) -> CdbGeometry {
        CdbGeometry::GeometryCollection(value.0.into_iter().map(CdbGeometry::from).collect())
    }
}

/// Wraps a [`PointZ`] (code 1001) directly as [`CdbGeometry::PointZ`].
impl From<PointZ> for CdbGeometry {
    fn from(value: PointZ) -> CdbGeometry {
        CdbGeometry::PointZ(value)
    }
}

/// Wraps a [`LineStringZ`] (code 1002) directly as [`CdbGeometry::LineStringZ`].
impl From<LineStringZ> for CdbGeometry {
    fn from(value: LineStringZ) -> CdbGeometry {
        CdbGeometry::LineStringZ(value)
    }
}

/// Wraps a [`PolygonZ`] (code 1003) directly as [`CdbGeometry::PolygonZ`].
impl From<PolygonZ> for CdbGeometry {
    fn from(value: PolygonZ) -> CdbGeometry {
        CdbGeometry::PolygonZ(value)
    }
}

/// Wraps a [`MultiPointZ`] (code 1004) directly as [`CdbGeometry::MultiPointZ`].
impl From<MultiPointZ> for CdbGeometry {
    fn from(value: MultiPointZ) -> CdbGeometry {
        CdbGeometry::MultiPointZ(value)
    }
}

/// Wraps a [`PointM`] (code 2001) directly as [`CdbGeometry::PointM`].
impl From<PointM> for CdbGeometry {
    fn from(value: PointM) -> CdbGeometry {
        CdbGeometry::PointM(value)
    }
}

/// Wraps a [`LineStringM`] (code 2002) directly as [`CdbGeometry::LineStringM`].
impl From<LineStringM> for CdbGeometry {
    fn from(value: LineStringM) -> CdbGeometry {
        CdbGeometry::LineStringM(value)
    }
}

/// Wraps a [`PolygonM`] (code 2003) directly as [`CdbGeometry::PolygonM`].
impl From<PolygonM> for CdbGeometry {
    fn from(value: PolygonM) -> CdbGeometry {
        CdbGeometry::PolygonM(value)
    }
}

/// Wraps a [`MultiPointM`] (code 2004) directly as [`CdbGeometry::MultiPointM`].
impl From<MultiPointM> for CdbGeometry {
    fn from(value: MultiPointM) -> CdbGeometry {
        CdbGeometry::MultiPointM(value)
    }
}

/// Lossless conversion from any [`geo_types::Geometry`]: the geo-types
/// conveniences that are not Simple Features table types collapse to their
/// SF equivalents (a `Line` becomes a two-vertex [`CdbGeometry::LineString`];
/// a `Rect` or `Triangle` becomes a [`CdbGeometry::Polygon`]); a
/// `GeometryCollection` recurses through its members.
impl From<geo_types::Geometry<f64>> for CdbGeometry {
    fn from(value: geo_types::Geometry<f64>) -> CdbGeometry {
        match value {
            geo_types::Geometry::Point(p) => CdbGeometry::Point(p),
            geo_types::Geometry::Line(l) => {
                CdbGeometry::LineString(geo_types::LineString::from(vec![l.start, l.end]))
            }
            geo_types::Geometry::LineString(ls) => CdbGeometry::LineString(ls),
            geo_types::Geometry::Polygon(poly) => CdbGeometry::Polygon(poly),
            geo_types::Geometry::MultiPoint(mp) => CdbGeometry::MultiPoint(mp),
            geo_types::Geometry::MultiLineString(mls) => CdbGeometry::MultiLineString(mls),
            geo_types::Geometry::MultiPolygon(mpoly) => CdbGeometry::MultiPolygon(mpoly),
            geo_types::Geometry::GeometryCollection(gc) => CdbGeometry::from(gc),
            geo_types::Geometry::Rect(r) => CdbGeometry::Polygon(r.to_polygon()),
            geo_types::Geometry::Triangle(t) => CdbGeometry::Polygon(t.to_polygon()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crs::StorageCrs;
    use crate::error::CdbError;
    use crate::metadata::temporal::parse_datetime;
    use crate::metadata::{
        GlobalMetadata, MetadataEncoding, MetadataStandard, ResourceMetadata, UnitOfMeasure,
    };
    use crate::profiles::simulation::WGS84_2D_WKT;

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

    /// §7.6.2.1 Requirement Geom1 /req/core/geometry-model — the model IS
    /// Simple Features via geo-types: lossless conversion both ways for the
    /// XY types, and codes per Geom2. (The draft cites "ISO 19111:2019" in
    /// Geom1's text; that is a typo for Simple Feature Access Part 1.)
    #[test]
    fn req_core_geometry_model_geo_types_identity() {
        use geo_types::{Geometry, LineString, Point};

        let cases: Vec<(Geometry<f64>, GeometryCode)> = vec![
            (Geometry::Point(Point::new(1.0, 2.0)), GeometryCode::Point),
            (
                Geometry::LineString(LineString::from(vec![(0.0, 0.0), (1.0, 1.0)])),
                GeometryCode::Linestring,
            ),
            (
                Geometry::GeometryCollection(geo_types::GeometryCollection(vec![Geometry::Point(
                    Point::new(3.0, 4.0),
                )])),
                GeometryCode::GeometryCollection,
            ),
        ];
        for (geo, code) in cases {
            let cdb = CdbGeometry::from(geo.clone());
            assert_eq!(cdb.geometry_code(), code);
            assert!(!cdb.has_z() && !cdb.has_m());
            assert_eq!(cdb.into_xy(), Some(geo)); // lossless round-trip
        }

        // geo-types conveniences map losslessly into SF table types.
        let line = geo_types::Line::new(
            geo_types::coord! { x: 0.0, y: 0.0 },
            geo_types::coord! { x: 1.0, y: 1.0 },
        );
        assert_eq!(
            CdbGeometry::from(Geometry::Line(line)).geometry_code(),
            GeometryCode::Linestring
        );

        // Z/M variants: correct codes, has_z/has_m, and NO lossy xy view.
        let pz = CdbGeometry::from(PointZ::new(Point::new(1.0, 2.0), 30.0));
        assert_eq!(pz.geometry_code(), GeometryCode::PointZ);
        assert!(pz.has_z() && !pz.has_m());
        assert_eq!(pz.clone().into_xy(), None);

        // A collection MAY contain Z members (each member's own code is in
        // the table); the collection then has_z and has no pure-XY view.
        let coll = CdbGeometry::GeometryCollection(vec![pz]);
        assert_eq!(coll.geometry_code(), GeometryCode::GeometryCollection);
        assert!(coll.has_z());
        assert_eq!(coll.into_xy(), None);
    }

    /// §7.6.3 Requirement Geom3 /req/core/geometry-zvalue — z units SHALL
    /// be specified in the global metadata UoM. (The draft's requirement
    /// box spells the slug `geometry-zcoordinate`; the class listing says
    /// `geometry-zvalue` — we cite the latter.)
    #[test]
    fn req_core_geometry_zvalue_requires_global_uom() {
        use geo_types::Point;
        let pz = CdbGeometry::from(PointZ::new(Point::new(1.0, 2.0), 30.0));

        let no_units = GeometryContext::new(None, None, None);
        assert!(matches!(
            pz.validate_in(&no_units, None),
            Err(GeometryViolation::MissingZUom)
        ));

        let with_z = GeometryContext::new(None, Some(UnitOfMeasure::Meters), None);
        assert!(pz.validate_in(&with_z, None).is_ok());

        // XY geometry needs no units at all.
        let p = CdbGeometry::from(geo_types::Geometry::Point(Point::new(0.0, 0.0)));
        assert!(p.validate_in(&no_units, None).is_ok());
    }

    /// §7.6.3 Requirement Geom4 /req/core/geometry-mvalue — m units come
    /// from the DATASET (resource) metadata, wired via
    /// `GeometryContext::from_datastore`.
    #[test]
    fn req_core_geometry_mvalue_requires_dataset_uom() {
        use geo_types::Point;
        let pm = CdbGeometry::from(PointM::new(Point::new(1.0, 2.0), 0.5));

        let crs = StorageCrs::from_wkt(WGS84_2D_WKT).unwrap();
        let global = GlobalMetadata::builder()
            .id("Store")
            .title("Store")
            .description("Test store")
            .contact_point("ops@example.com")
            .language(crate::metadata::LanguageTag::new("en").unwrap())
            .standard(MetadataStandard::Dcat)
            .encoding(MetadataEncoding::Json)
            .uom(UnitOfMeasure::Meters)
            .created(parse_datetime("2026-08-01T00:00:00Z").unwrap())
            .build()
            .unwrap();

        // Resource metadata WITHOUT uom -> Geom4 violation for M geometry.
        let bare = ResourceMetadata::new("Roads", "Road Network", "Primary roads");
        let ctx = GeometryContext::from_datastore(&crs, &global, Some(&bare));
        assert!(matches!(
            pm.validate_in(&ctx, None),
            Err(GeometryViolation::MissingMUom)
        ));

        // Resource metadata WITH uom -> ok. Global uom flows to z_uom.
        let mut with_uom = bare.clone();
        with_uom.uom = Some(UnitOfMeasure::Meters);
        let ctx = GeometryContext::from_datastore(&crs, &global, Some(&with_uom));
        assert!(pm.validate_in(&ctx, None).is_ok());
    }

    /// §7.6.3 Requirement Geom5 /req/core/geometry-coordinates — a claimed
    /// source CRS must provably match the datastore CRS; an unverifiable
    /// claim (anonymous datastore CRS) is the ambiguity Geom5 forbids.
    #[test]
    fn req_core_geometry_coordinates_foreign_crs_rejected() {
        use geo_types::Point;
        let p = CdbGeometry::from(geo_types::Geometry::Point(Point::new(0.0, 0.0)));
        let epsg4326 = ("EPSG".to_owned(), "4326".to_owned());
        let nad83 = ("EPSG".to_owned(), "4269".to_owned());

        let ctx = GeometryContext::new(Some(epsg4326.clone()), None, None);
        assert!(p.validate_in(&ctx, None).is_ok()); // association via datastore
        assert!(p.validate_in(&ctx, Some(&epsg4326)).is_ok()); // provable match
        // Case-insensitive authority comparison.
        let lower = ("epsg".to_owned(), "4326".to_owned());
        assert!(p.validate_in(&ctx, Some(&lower)).is_ok());
        assert!(matches!(
            p.validate_in(&ctx, Some(&nad83)),
            Err(GeometryViolation::ForeignCrs { .. })
        ));

        // Claim against an anonymous datastore CRS is unverifiable -> foreign.
        let anon = GeometryContext::new(None, None, None);
        assert!(matches!(
            p.validate_in(&anon, Some(&epsg4326)),
            Err(GeometryViolation::ForeignCrs { datastore, .. }) if datastore == "unidentified"
        ));
    }

    /// §7.6.4 Requirement Geom6 /req/core/geometry-collection-srs — members
    /// carry no CRS of their own (single CRS by construction, the same
    /// pattern as CRS2); validation recurses so member unit requirements
    /// still bite.
    #[test]
    fn req_core_geometry_collection_srs_single_crs_by_construction() {
        use geo_types::Point;
        let coll = CdbGeometry::GeometryCollection(vec![
            CdbGeometry::from(geo_types::Geometry::Point(Point::new(0.0, 0.0))),
            CdbGeometry::from(PointZ::new(Point::new(1.0, 1.0), 10.0)),
        ]);

        let no_units = GeometryContext::new(None, None, None);
        assert!(matches!(
            coll.validate_in(&no_units, None),
            Err(GeometryViolation::MissingZUom) // recursion reaches the member
        ));
        let ctx = GeometryContext::new(None, Some(UnitOfMeasure::Meters), None);
        assert!(coll.validate_in(&ctx, None).is_ok());
    }

    /// Branch-coverage pin (not a spec clause): drives three implementation
    /// branches the requirement tests leave uncovered — the per-interior-ring
    /// z-length check in `PolygonZ::new`, the M-side `None` arm of `into_xy`,
    /// and the `Rect`/`Triangle` arms of `From<geo_types::Geometry>`.
    #[test]
    fn geometry_branch_coverage_gaps() {
        use geo_types::{Geometry, LineString, Point, Polygon};

        // (a) Interior-ring COUNT matches, but the single interior ring's z
        // length is wrong -> the per-ring check fires with an "interior <i>"
        // part (distinct from the "interior rings" count branch).
        let exterior = LineString::from(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
        let interior = LineString::from(vec![(2.0, 2.0), (4.0, 2.0), (4.0, 4.0), (2.0, 4.0)]);
        let poly = Polygon::new(exterior, vec![interior]);
        let ext_len = poly.exterior().0.len();
        let int_len = poly.interiors()[0].0.len();
        let err =
            PolygonZ::new(poly, vec![1.0; ext_len], vec![vec![1.0; int_len - 1]]).unwrap_err();
        assert!(matches!(
            &err,
            GeometryViolation::ZLengthMismatch { part, .. } if part.starts_with("interior")
        ));

        // (b) An M geometry has no lossless planar view -> into_xy is None.
        let pm = CdbGeometry::from(PointM::new(Point::new(1.0, 2.0), 0.5));
        assert_eq!(pm.into_xy(), None);

        // (c) Rect and Triangle conveniences both collapse to a Polygon.
        let rect = CdbGeometry::from(Geometry::Rect(geo_types::Rect::new(
            geo_types::coord! { x: 0.0, y: 0.0 },
            geo_types::coord! { x: 2.0, y: 2.0 },
        )));
        assert!(matches!(&rect, CdbGeometry::Polygon(_)));
        assert_eq!(rect.geometry_code(), GeometryCode::Polygon);

        let tri = CdbGeometry::from(Geometry::Triangle(geo_types::Triangle::new(
            geo_types::coord! { x: 0.0, y: 0.0 },
            geo_types::coord! { x: 1.0, y: 0.0 },
            geo_types::coord! { x: 0.0, y: 1.0 },
        )));
        assert!(matches!(&tri, CdbGeometry::Polygon(_)));
        assert_eq!(tri.geometry_code(), GeometryCode::Polygon);
    }
}
