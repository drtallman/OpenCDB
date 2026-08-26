//! Implements the CDB1GlobalGrid Tiling Extension requirements class
//! (spec §7.11, TCE1–TCE7) — the CDB 1.x-backwards-compatible global grid,
//! addressed per the OGC Two Dimensional Tile Matrix Set Standard (2DTMS)
//! with variable-width (coalesced) rows.
//!
//! Draft quirks: the requirements-class URI is mislabeled
//! `/req/core/geometry-`; Requirement TCE5's box
//! (`/req/core/tiling-extension-metadata`) is missing from the document —
//! tileset metadata duties are covered via Tiling9/Tiling10 in the abstract
//! module.

use crate::metadata::Bbox;
use crate::tiling::TilingViolation;

/// The CDB 1.x latitude coalescence zones, as `(upper |latitude| bound,
/// column-coalescence factor)` pairs in ascending order (CDB 1.x zone table /
/// OGC 2DTMS Annex E2 variable-width tiling). A 1° geocell whose equator-most
/// integer latitude is `a` takes the factor of the first pair whose bound `a`
/// is strictly below; the final pair (the polar zone) catches everything up to
/// the pole by falling through. Every factor divides 360, so coalesced columns
/// tile each row exactly.
const ZONE_BANDS: [(f64, u32); 6] = [
    (50.0, 1),
    (70.0, 2),
    (75.0, 3),
    (80.0, 4),
    (89.0, 6),
    (90.0, 12),
];

/// The coarsest CDB1GlobalGrid Level of Detail (Requirement TCE7-A,
/// §7.11.3.6): LoD −10, where a whole geocell is a single raster cell.
pub const LOD_MIN: i8 = -10;

/// The finest CDB1GlobalGrid Level of Detail (Requirement TCE7-A, §7.11.3.6):
/// LoD 23, the deepest level the scheme addresses.
pub const LOD_MAX: i8 = 23;

/// The raster edge length, in cells, of a tile at LoD 0 and finer (Requirement
/// TCE7-B, §7.11.3.6): every such tile is a 1024×1024 grid of cells.
pub const TILE_SIZE_CELLS: u32 = 1024;

/// A CDB1GlobalGrid Level of Detail: a validated integer in the closed range
/// [`LOD_MIN`]..=[`LOD_MAX`] (−10..=23, Requirement TCE7-A, §7.11.3.6).
/// Non-negative levels subdivide a geocell; negative levels coarsen it. The
/// only constructor, [`Cdb1Lod::new`], rejects out-of-range values, so every `Cdb1Lod`
/// value is in range by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Cdb1Lod(i8);

impl Cdb1Lod {
    /// Constructs a Level of Detail, enforcing Requirement TCE7-A: `value`
    /// must lie in [`LOD_MIN`]..=[`LOD_MAX`] (−10..=23). Returns
    /// [`TilingViolation::Cdb1LodOutOfRange`] for any value outside that range.
    pub fn new(value: i8) -> Result<Cdb1Lod, TilingViolation> {
        if (LOD_MIN..=LOD_MAX).contains(&value) {
            Ok(Cdb1Lod(value))
        } else {
            Err(TilingViolation::Cdb1LodOutOfRange { lod: value })
        }
    }

    /// The LoD's integer value, guaranteed to lie in
    /// [`LOD_MIN`]..=[`LOD_MAX`].
    pub fn value(self) -> i8 {
        self.0
    }
}

/// A CDB1GlobalGrid tile address: a Level of Detail plus a `(row, col)` index
/// into that level's tile-matrix (Requirements TCE6/TCE7, §7.11.3.5–§7.11.3.6),
/// numbered from the north-west corner per OGC 2DTMS — `row` 0 is the
/// northernmost band, `col` 0 begins at longitude −180°.
///
/// The fields are private and every `Cdb1TileAddress` is produced only by
/// [`Cdb1GlobalGrid::address`] or [`Cdb1GlobalGrid::tile_at`], so the type
/// carries an invariant: `row`/`col` are in range for `lod`, and `col` is
/// aligned to its row's coalescence factor. [`Cdb1GlobalGrid::parent`] and
/// [`Cdb1GlobalGrid::children`] preserve it. Read the parts back with
/// [`Self::lod`], [`Self::row`], and [`Self::col`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cdb1TileAddress {
    lod: Cdb1Lod,
    row: u64,
    col: u64,
}

impl Cdb1TileAddress {
    /// The address's Level of Detail.
    pub fn lod(self) -> Cdb1Lod {
        self.lod
    }

    /// The address's row index, counted from the northernmost band (row 0).
    pub fn row(self) -> u64 {
        self.row
    }

    /// The address's column index, counted from longitude −180° (col 0); a
    /// multiple of the row's coalescence factor by construction.
    pub fn col(self) -> u64 {
        self.col
    }
}

/// The CDB1GlobalGrid tiling scheme (spec §7.11): the CDB 1.x-compatible
/// global grid whose raster sizing, tile addressing, and zone coalescence this
/// type computes. A unit struct — its operations are level/geocell arithmetic
/// with no per-instance state: [`Self::raster_size`] and [`Self::matrix_size`]
/// size the pyramid, [`Self::address`] / [`Self::tile_at`] resolve tiles,
/// [`Self::tile_extent`] gives their geographic bounds, and [`Self::parent`] /
/// [`Self::children`] walk between Levels of Detail.
pub struct Cdb1GlobalGrid;

impl Cdb1GlobalGrid {
    /// The raster edge length, in cells, of a tile at `lod` (Requirements
    /// TCE7-B and TCE7-C, §7.11.3.6). At LoD 0 and finer every tile is
    /// [`TILE_SIZE_CELLS`] (1024) cells square; each negative LoD keeps the
    /// whole-geocell extent while halving the cell count — 512 at −1, 256 at
    /// −2, … 1 at −10 ([`LOD_MIN`]).
    pub fn raster_size(lod: Cdb1Lod) -> u32 {
        let value = lod.value();
        if value >= 0 {
            TILE_SIZE_CELLS
        } else {
            // value ∈ −10..=−1, so 10 + value ∈ 0..=9: a valid, non-overflowing
            // u32 shift. Widen through i32 to avoid i8 arithmetic entirely.
            1u32 << (10 + i32::from(value))
        }
    }

    /// The nominal tile edge length in decimal degrees at `lod`: a whole
    /// geocell (1.0°) at LoD ≤ 0, halving with each finer level so
    /// `h = (1/2)ⁿ` at LoD n ≥ 1 (exact in binary floating point for
    /// n ≤ 23). Nominal tiles are square; latitude coalescence widens a tile
    /// by aggregating whole nominal columns, never changing `h`.
    fn h(lod: Cdb1Lod) -> f64 {
        let n = lod.value();
        if n <= 0 {
            1.0
        } else {
            (0.5f64).powi(i32::from(n))
        }
    }

    /// The column-coalescence factor of the row's enclosing 1° geocell (CDB
    /// 1.x zone table / 2DTMS Annex E2). The geocell index counted from the
    /// north is `g = ⌊row·h⌋ ∈ [0, 179]`; its equator-most integer latitude is
    /// `a = 89 − g` in the northern hemisphere (`g < 90`) and `a = g − 90` in
    /// the southern, so the two hemispheres share a factor by symmetry. The
    /// factor is the first [`ZONE_BANDS`] entry whose bound exceeds `a`; a
    /// geocell abutting the pole (`a = 89`) falls through to the polar factor
    /// 12. Callers pass an in-range row, so `a ≤ 89` and a band always matches.
    fn zone_width_for_row(lod: Cdb1Lod, row: u64) -> u32 {
        // row·h ∈ [0, 180) for an in-range row, so g ∈ [0, 179]; the cast is
        // exact and cannot truncate meaningfully.
        let g = ((row as f64) * Self::h(lod)).floor() as i64;
        let a = if g < 90 { 89 - g } else { g - 90 };
        for (bound, width) in ZONE_BANDS {
            if (a as f64) < bound {
                return width;
            }
        }
        // Unreachable for an in-range row (a ≤ 89 < 90); the polar factor is
        // the safe default for a hypothetical out-of-range geocell.
        12
    }

    /// The tile-matrix size `(width, height)` in tiles at `lod` (Requirement
    /// TCE6, §7.11.3.5): the LoD-0 geocell matrix is 360×180, and each finer
    /// level quadruples it to `360·2ⁿ × 180·2ⁿ`; every LoD ≤ 0 keeps the
    /// 360×180 geocell matrix (negative levels coarsen a geocell's raster, not
    /// the tiling). Width counts nominal columns before coalescence.
    pub fn matrix_size(lod: Cdb1Lod) -> (u64, u64) {
        let n = lod.value();
        if n <= 0 {
            (360, 180)
        } else {
            let factor = 1u64 << i32::from(n);
            (360 * factor, 180 * factor)
        }
    }

    /// The coalescence factor for `row` at `lod` (CDB 1.x zones / 2DTMS
    /// variable-width tiling): how many nominal columns the row's geocell band
    /// aggregates into each tile — 1 away from the poles, up to 12 abutting
    /// them. Returns [`TilingViolation::TileOutOfRange`] if `row` is outside
    /// the LoD's matrix height. Only the row is checked here, so that
    /// violation carries `col: 0` as a not-applicable sentinel.
    pub fn coalescence_factor(lod: Cdb1Lod, row: u64) -> Result<u32, TilingViolation> {
        let (_, height) = Self::matrix_size(lod);
        if row >= height {
            return Err(TilingViolation::TileOutOfRange {
                lod: lod.value(),
                row,
                col: 0,
            });
        }
        Ok(Self::zone_width_for_row(lod, row))
    }

    /// Builds a validated [`Cdb1TileAddress`] from raw indices (Requirements
    /// TCE6/TCE7): `row`/`col` must lie within [`Self::matrix_size`]
    /// (else [`TilingViolation::TileOutOfRange`]), and `col` must be a multiple
    /// of the row's coalescence factor (else
    /// [`TilingViolation::MisalignedColumn`]) — a misaligned column names no
    /// real tile under the 2DTMS variable-width rule.
    pub fn address(lod: Cdb1Lod, row: u64, col: u64) -> Result<Cdb1TileAddress, TilingViolation> {
        let (width, height) = Self::matrix_size(lod);
        if row >= height || col >= width {
            return Err(TilingViolation::TileOutOfRange {
                lod: lod.value(),
                row,
                col,
            });
        }
        let factor = Self::zone_width_for_row(lod, row);
        if !col.is_multiple_of(u64::from(factor)) {
            return Err(TilingViolation::MisalignedColumn { col, factor });
        }
        Ok(Cdb1TileAddress { lod, row, col })
    }

    /// The tile at `lod` containing the decimal-degree point `(lat, lon)`
    /// (Requirement TCE4, §7.11.3.4). The coordinate must be finite with
    /// latitude in [−90, 90] and longitude in [−180, 180], else
    /// [`TilingViolation::CoordinateOutOfRange`]. The point falls in the tile
    /// whose half-open extent `[west, east) × [south, north)` contains it; the
    /// resulting nominal column is snapped down to the row's coalescence
    /// factor so the address names the real (possibly widened) tile.
    pub fn tile_at(lat: f64, lon: f64, lod: Cdb1Lod) -> Result<Cdb1TileAddress, TilingViolation> {
        if lat.is_nan() || lon.is_nan() || lat.abs() > 90.0 || lon.abs() > 180.0 {
            return Err(TilingViolation::CoordinateOutOfRange { lat, lon });
        }
        let h = Self::h(lod);
        let (width, height) = Self::matrix_size(lod);
        // Rows count south from +90° and columns east from −180°, so each tile
        // owns its half-open extent [south, north) × [west, east): the
        // south/west (equator-most, prime-meridian-most) edges are inclusive.
        // A column is ⌊(lon+180)/h⌋ (west-inclusive); a row is the mirror,
        // ⌈(90−lat)/h⌉ − 1, which equals ⌊(90−lat)/h⌋ for an interior point but
        // pulls a point sitting exactly on a horizontal grid line into the tile
        // north of the line (south-inclusive). The clamps pin ±90°/±180° edges
        // to the last in-range row/column.
        let row = (((90.0 - lat) / h).ceil() - 1.0)
            .max(0.0)
            .min((height - 1) as f64) as u64;
        let col_nominal = (((lon + 180.0) / h).floor() as u64).min(width - 1);
        let factor = Self::zone_width_for_row(lod, row);
        let col = col_nominal - (col_nominal % u64::from(factor));
        Ok(Cdb1TileAddress { lod, row, col })
    }

    /// The geographic extent of `addr` as a WGS-84 bounding box (Requirements
    /// TCE6/TCE7). North and south come from the row (`north = 90 − row·h`,
    /// `south = north − h`); west from the column (`west = −180 + col·h`); and
    /// east spans the row's coalescence factor (`east = west + factor·h`), so a
    /// polar tile is `factor` nominal columns wide. Because every factor
    /// divides 360, the eastmost tile of a row ends exactly on +180°.
    pub fn tile_extent(addr: Cdb1TileAddress) -> Bbox {
        let h = Self::h(addr.lod);
        let factor = Self::zone_width_for_row(addr.lod, addr.row);
        let north = 90.0 - (addr.row as f64) * h;
        let south = north - h;
        let west = -180.0 + (addr.col as f64) * h;
        let east = west + f64::from(factor) * h;
        Bbox {
            west,
            south,
            east,
            north,
        }
    }

    /// The tile that contains `addr` one Level of Detail coarser (Requirement
    /// TCE7-B, §7.11.3.6), or `None` at [`LOD_MIN`] where the pyramid bottoms
    /// out. Through the negative LoDs a geocell coarsens in place, so the
    /// parent keeps the same `(row, col)` one level up. From LoD 1 the scheme
    /// is a quadtree: the parent halves the row, halves the column, then snaps
    /// the column down to the parent row's coalescence factor.
    pub fn parent(addr: Cdb1TileAddress) -> Option<Cdb1TileAddress> {
        let lod_v = addr.lod.value();
        if lod_v == LOD_MIN {
            return None;
        }
        // lod_v > LOD_MIN, so lod_v − 1 is in range; `.ok()?` is a total,
        // panic-free way to obtain the coarser Cdb1Lod.
        let parent_lod = Cdb1Lod::new(lod_v - 1).ok()?;
        if lod_v <= 0 {
            Some(Cdb1TileAddress {
                lod: parent_lod,
                row: addr.row,
                col: addr.col,
            })
        } else {
            let row = addr.row / 2;
            let parent_factor = Self::zone_width_for_row(parent_lod, row);
            let half = addr.col / 2;
            let col = half - (half % u64::from(parent_factor));
            Some(Cdb1TileAddress {
                lod: parent_lod,
                row,
                col,
            })
        }
    }

    /// The tiles one Level of Detail finer that partition `addr` (Requirement
    /// TCE7-B, §7.11.3.6), or empty at [`LOD_MAX`]. Below LoD 0 a geocell
    /// refines in place, yielding the single same-`(row, col)` tile one level
    /// down. From LoD 0 the scheme is a quadtree: exactly four children at the
    /// doubled row/column origin — `(2·row, 2·col)`, `(2·row, 2·col + factor)`,
    /// `(2·row + 1, 2·col)`, `(2·row + 1, 2·col + factor)` — where `factor` is
    /// the coalescence factor of `addr`'s row (identical for the children,
    /// which share the same geocell).
    pub fn children(addr: Cdb1TileAddress) -> Vec<Cdb1TileAddress> {
        let lod_v = addr.lod.value();
        if lod_v == LOD_MAX {
            return Vec::new();
        }
        // lod_v < LOD_MAX, so lod_v + 1 is in range; an Err is impossible and
        // an empty child set is the safe, panic-free fallback.
        let child_lod = match Cdb1Lod::new(lod_v + 1) {
            Ok(lod) => lod,
            Err(_) => return Vec::new(),
        };
        if lod_v < 0 {
            vec![Cdb1TileAddress {
                lod: child_lod,
                row: addr.row,
                col: addr.col,
            }]
        } else {
            let factor = u64::from(Self::zone_width_for_row(addr.lod, addr.row));
            let r0 = addr.row * 2;
            let c0 = addr.col * 2;
            vec![
                Cdb1TileAddress {
                    lod: child_lod,
                    row: r0,
                    col: c0,
                },
                Cdb1TileAddress {
                    lod: child_lod,
                    row: r0,
                    col: c0 + factor,
                },
                Cdb1TileAddress {
                    lod: child_lod,
                    row: r0 + 1,
                    col: c0,
                },
                Cdb1TileAddress {
                    lod: child_lod,
                    row: r0 + 1,
                    col: c0 + factor,
                },
            ]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiling::TilingViolation;

    /// §7.11.3.6 Requirement TCE7-A — LoD range −10..=23.
    #[test]
    fn req_core_tiling_ext_lod_range() {
        assert!(Cdb1Lod::new(-10).is_ok());
        assert!(Cdb1Lod::new(0).is_ok());
        assert!(Cdb1Lod::new(23).is_ok());
        for bad in [-11i8, 24, i8::MIN, i8::MAX] {
            assert!(matches!(
                Cdb1Lod::new(bad),
                Err(TilingViolation::Cdb1LodOutOfRange { lod }) if lod == bad
            ));
        }
    }

    /// §7.11.3.6 Requirement TCE7-B/C — tiles are 1024×1024 cells from
    /// LoD 0 up; negative LoDs keep the geocell extent with halved cells.
    #[test]
    fn req_core_tiling_ext_raster_sizes() {
        assert_eq!(Cdb1GlobalGrid::raster_size(Cdb1Lod::new(0).unwrap()), 1024);
        assert_eq!(Cdb1GlobalGrid::raster_size(Cdb1Lod::new(23).unwrap()), 1024);
        assert_eq!(Cdb1GlobalGrid::raster_size(Cdb1Lod::new(-1).unwrap()), 512);
        assert_eq!(Cdb1GlobalGrid::raster_size(Cdb1Lod::new(-4).unwrap()), 64);
        assert_eq!(Cdb1GlobalGrid::raster_size(Cdb1Lod::new(-10).unwrap()), 1);
    }

    fn lod(v: i8) -> Cdb1Lod {
        Cdb1Lod::new(v).unwrap()
    }

    /// §7.11.3.5 Requirement TCE6 — LoD 0 is the 1°×1° Geocell matrix,
    /// width 360 × height 180, north-west origin per 2DTMS.
    #[test]
    fn req_core_tiling_ext_lod0_geocells() {
        assert_eq!(Cdb1GlobalGrid::matrix_size(lod(0)), (360, 180));
        assert_eq!(Cdb1GlobalGrid::matrix_size(lod(-10)), (360, 180));
        assert_eq!(Cdb1GlobalGrid::matrix_size(lod(1)), (720, 360));
        // Row 0 = northernmost; col 0 = -180.
        let t = Cdb1GlobalGrid::address(lod(0), 0, 0).unwrap();
        let b = Cdb1GlobalGrid::tile_extent(t);
        assert_eq!(
            (b.west, b.south, b.east, b.north),
            (-180.0, 89.0, -168.0, 90.0)
        ); // pole row: 12° coalescence
        // An equatorial tile is exactly 1°×1°.
        let eq = Cdb1GlobalGrid::tile_at(0.5, 0.5, lod(0)).unwrap();
        let be = Cdb1GlobalGrid::tile_extent(eq);
        assert_eq!((be.east - be.west, be.north - be.south), (1.0, 1.0));
    }

    /// §7.11.3.6 Requirement TCE7-B — quad subdivision from LoD 1; one
    /// geocell down through the negative LoDs; children tile the parent.
    #[test]
    fn req_core_tiling_ext_subdivision() {
        // Point round-trip at several LoDs.
        for l in [-3i8, 0, 2, 5] {
            let t = Cdb1GlobalGrid::tile_at(37.25, -122.75, lod(l)).unwrap();
            let b = Cdb1GlobalGrid::tile_extent(t);
            assert!(b.west <= -122.75 && -122.75 < b.east && b.south <= 37.25 && 37.25 < b.north);
        }
        // Quad children at LoD >= 0; parent inverts; extents tile the parent.
        let t = Cdb1GlobalGrid::tile_at(10.5, 20.5, lod(2)).unwrap();
        let kids = Cdb1GlobalGrid::children(t);
        assert_eq!(kids.len(), 4);
        let pb = Cdb1GlobalGrid::tile_extent(t);
        let mut area = 0.0;
        for k in kids {
            assert_eq!(Cdb1GlobalGrid::parent(k), Some(t));
            let kb = Cdb1GlobalGrid::tile_extent(k);
            assert!(
                kb.west >= pb.west
                    && kb.east <= pb.east
                    && kb.south >= pb.south
                    && kb.north <= pb.north
            );
            area += (kb.east - kb.west) * (kb.north - kb.south);
        }
        assert!((area - (pb.east - pb.west) * (pb.north - pb.south)).abs() < 1e-9);
        // Negative LoDs: single chain over the same geocell extent.
        let g = Cdb1GlobalGrid::tile_at(10.5, 20.5, lod(-2)).unwrap();
        let kids = Cdb1GlobalGrid::children(g);
        assert_eq!(kids.len(), 1);
        assert_eq!(kids[0].lod().value(), -1);
        assert_eq!(
            Cdb1GlobalGrid::tile_extent(kids[0]),
            Cdb1GlobalGrid::tile_extent(g)
        );
        assert_eq!(Cdb1GlobalGrid::parent(kids[0]), Some(g));
        // Chain ends at the range limits.
        assert_eq!(
            Cdb1GlobalGrid::parent(Cdb1GlobalGrid::tile_at(0.0, 0.0, lod(-10)).unwrap()),
            None
        );
        assert!(
            Cdb1GlobalGrid::children(Cdb1GlobalGrid::tile_at(0.0, 0.0, lod(23)).unwrap())
                .is_empty()
        );
    }

    /// CDB 1.x zones / 2DTMS Annex E2 variable width — coalescence factors
    /// at the zone boundaries, alignment enforcement, pyramid constancy.
    #[test]
    fn req_core_tiling_ext_zone_coalescence() {
        // width(lat) at LoD 0 rows straddling each boundary, north + south.
        let cases = [
            (49.5, 1u32),
            (50.5, 2),
            (69.5, 2),
            (70.5, 3),
            (74.5, 3),
            (75.5, 4),
            (79.5, 4),
            (80.5, 6),
            (88.5, 6),
            (89.5, 12),
        ];
        for (lat, want) in cases {
            for signed in [lat, -lat] {
                let t = Cdb1GlobalGrid::tile_at(signed, 0.0, lod(0)).unwrap();
                let f = Cdb1GlobalGrid::coalescence_factor(lod(0), t.row()).unwrap();
                assert_eq!(f, want, "lat {signed}");
            }
        }
        // Misaligned col rejected; tile_at snaps.
        assert!(matches!(
            Cdb1GlobalGrid::address(lod(0), 0, 5),
            Err(TilingViolation::MisalignedColumn { factor: 12, .. })
        ));
        let t = Cdb1GlobalGrid::tile_at(89.5, 0.5, lod(0)).unwrap();
        assert_eq!(t.col() % 12, 0);
        // Coalesced extent spans factor × nominal width.
        let b = Cdb1GlobalGrid::tile_extent(t);
        assert_eq!(b.east - b.west, 12.0);
        // Factor constant down the pyramid for a polar geocell.
        for l in [0i8, 1, 3] {
            let p = Cdb1GlobalGrid::tile_at(89.5, 0.5, lod(l)).unwrap();
            assert_eq!(
                Cdb1GlobalGrid::coalescence_factor(lod(l), p.row()).unwrap(),
                12
            );
        }
    }

    /// §7.11.3.4 Requirement TCE4 — coordinates validated and clamped;
    /// everything in decimal degrees.
    #[test]
    fn req_core_tiling_ext_coordinate_validation() {
        assert!(matches!(
            Cdb1GlobalGrid::tile_at(90.5, 0.0, lod(0)),
            Err(TilingViolation::CoordinateOutOfRange { .. })
        ));
        assert!(matches!(
            Cdb1GlobalGrid::tile_at(0.0, -180.5, lod(0)),
            Err(TilingViolation::CoordinateOutOfRange { .. })
        ));
        assert!(matches!(
            Cdb1GlobalGrid::tile_at(f64::NAN, 0.0, lod(0)),
            Err(TilingViolation::CoordinateOutOfRange { .. })
        ));
        // Edges clamp inward.
        let south = Cdb1GlobalGrid::tile_at(-90.0, 0.0, lod(0)).unwrap();
        assert_eq!(south.row(), 179);
        let east = Cdb1GlobalGrid::tile_at(0.0, 180.0, lod(0)).unwrap();
        let b = Cdb1GlobalGrid::tile_extent(east);
        assert_eq!(b.east, 180.0);
        // Out-of-range address rejected.
        assert!(matches!(
            Cdb1GlobalGrid::address(lod(0), 180, 0),
            Err(TilingViolation::TileOutOfRange { .. })
        ));
    }
}
