//! Implements the GNOSISGlobalGrid Tiling Extension requirements class
//! (spec §7.12, TCE1–TCE6) — the 2DTMS-registered variable-width global
//! grid. Of special note (§7.12): this extension IS NOT compatible with the
//! OGC CDB 1.x tiling structure.
//!
//! The registered TileMatrixSet definition (Requirement TCE2-B,
//! `/req/core/tiling-extension-tms`) supplies what §7.12 itself does not
//! state: zoom levels 0..=28, 256×256-cell tiles, and per-matrix
//! variable-width coalescence (informative §7.12.2/§7.12.4). Unlike
//! [`cdb1_grid`](super::cdb1_grid) there is no TCE7 analog — no negative
//! levels, no raster halving — and coalescence factors re-adjust at each
//! tile matrix instead of holding per latitude band. The two grids are
//! deliberately parallel surfaces with no shared trait (rule of two;
//! see the module doc of [`crate::tiling`]).
//!
//! Draft quirks: the requirements-class URI is mislabeled
//! `/req/core/geometry-` (as in §7.11); Requirement TCE5's box
//! (`/req/core/tiling-extension-metadata`) is missing from the document
//! (as in §7.11) — tileset-metadata duties are covered via Tiling9/Tiling10
//! in the abstract module; the TCE6-C box carries a "SHALL_" typo. §7.12's
//! TCE1–TCE4 slugs collide with §7.11's, so docs here always say "§7.12"
//! beside a TCE number.

use crate::metadata::Bbox;
use crate::tiling::TilingViolation;

/// The finest GNOSISGlobalGrid zoom level (Requirement TCE2-B, §7.12.3.2):
/// the registered TileMatrixSet defines tile matrices 0..=28 — the deepest
/// set whose (level, row, col) still packs into a single 64-bit key.
pub const LEVEL_MAX: u8 = 28;

/// The raster edge length, in cells, of every GNOSISGlobalGrid tile: the
/// registered TileMatrixSet pins `tileWidth`/`tileHeight` 256, binding via
/// Requirement TCE2-B (§7.12.3.2) — §7.12 itself states no raster rule
/// (contrast the CDB1GlobalGrid's TCE7-C 1024).
pub const TILE_SIZE_CELLS: u32 = 256;

/// A GNOSISGlobalGrid zoom level (§7.12.3.5 calls it "zoom level" or "tile
/// matrix identifier"): a validated integer in 0..=[`LEVEL_MAX`]. Negative
/// levels do not exist in this scheme and are unrepresentable (`u8`). The
/// only constructor, [`GnosisLevel::new`], rejects values above
/// [`LEVEL_MAX`], so every `GnosisLevel` is in range by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GnosisLevel(u8);

impl GnosisLevel {
    /// Constructs a zoom level, enforcing Requirement TCE2-B (§7.12.3.2):
    /// `value` must lie in 0..=[`LEVEL_MAX`] (28). Returns
    /// [`TilingViolation::GnosisLevelOutOfRange`] above that range.
    pub fn new(value: u8) -> Result<GnosisLevel, TilingViolation> {
        if value <= LEVEL_MAX {
            Ok(GnosisLevel(value))
        } else {
            Err(TilingViolation::GnosisLevelOutOfRange { level: value })
        }
    }

    /// The level's integer value, guaranteed to lie in 0..=[`LEVEL_MAX`].
    pub fn value(self) -> u8 {
        self.0
    }
}

/// A GNOSISGlobalGrid tile address: a zoom level plus a `(row, col)` index
/// into that level's nominal tile-matrix (Requirement TCE6, §7.12.3.5),
/// numbered from the north-west corner per the registered TileMatrixSet
/// (`pointOfOrigin [90, −180]`, `cornerOfOrigin topLeft`) — `row` 0 is the
/// northernmost band, `col` 0 begins at longitude −180°.
///
/// The fields are private and every `GnosisTileAddress` is produced only by
/// [`GnosisGlobalGrid::address`], [`GnosisGlobalGrid::tile_at`], or
/// [`GnosisTileAddress::from_key`], so the type carries an invariant:
/// `row`/`col` are in range for `level`, and `col` is aligned to its row's
/// coalescence factor. [`GnosisGlobalGrid::parent`] and
/// [`GnosisGlobalGrid::children`] preserve it. Read the parts back with
/// [`Self::level`], [`Self::row`], and [`Self::col`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GnosisTileAddress {
    level: GnosisLevel,
    row: u64,
    col: u64,
}

impl GnosisTileAddress {
    /// The address's zoom level.
    pub fn level(self) -> GnosisLevel {
        self.level
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

    /// Packs the address into a single 64-bit key — the §7.12.2 observation
    /// that levels 0..=28 are exactly where "the matrix, row and column
    /// identifiers can still fit together within a single 64-bit key":
    /// `level` in the top 5 bits (bits 59..64), `row` in the next 29
    /// (30..59), `col` in the low 30 (0..30) — 5 + 29 + 30 = 64, exactly
    /// full at level 28 (height 2³⁰⁄₂ = 2²⁹, width 2³⁰). The invariant
    /// (`row < 2·2ⁿ ≤ 2²⁹`, `col < 4·2ⁿ ≤ 2³⁰`) keeps the fields disjoint,
    /// and numeric key order is (level, row, col) lexicographic order. The
    /// bit layout is this crate's convention; the spec fixes only that the
    /// three fit.
    pub fn key(self) -> u64 {
        (u64::from(self.level.value()) << 59) | (self.row << 30) | self.col
    }

    /// Unpacks a 64-bit key back into a validated address — the inverse of
    /// [`Self::key`]. Rejects encodings naming no real tile with the same
    /// violations as [`GnosisGlobalGrid::address`]:
    /// [`TilingViolation::GnosisLevelOutOfRange`] for level bits above 28,
    /// [`TilingViolation::GnosisTileOutOfRange`] for an out-of-matrix
    /// row/col, and [`TilingViolation::MisalignedColumn`] for a column not
    /// aligned to its row's coalescence factor.
    pub fn from_key(key: u64) -> Result<GnosisTileAddress, TilingViolation> {
        // key >> 59 ≤ 31: the cast to u8 is exact.
        let level = GnosisLevel::new((key >> 59) as u8)?;
        let row = (key >> 30) & ((1u64 << 29) - 1);
        let col = key & ((1u64 << 30) - 1);
        GnosisGlobalGrid::address(level, row, col)
    }
}

/// The GNOSISGlobalGrid tiling scheme (spec §7.12): the 2DTMS-registered
/// variable-width global grid whose tile addressing and per-matrix
/// coalescence this type computes. A unit struct — its operations are level
/// arithmetic with no per-instance state: [`Self::matrix_size`] sizes the
/// pyramid, [`Self::address`] / [`Self::tile_at`] resolve tiles,
/// [`Self::tile_extent`] gives their geographic bounds, and
/// [`Self::parent`] / [`Self::children`] walk between zoom levels. The
/// surface deliberately parallels [`Cdb1GlobalGrid`](super::Cdb1GlobalGrid)
/// where semantics match; there is no `raster_size` (every tile is
/// [`TILE_SIZE_CELLS`] square) and no negative levels.
pub struct GnosisGlobalGrid;

impl GnosisGlobalGrid {
    /// The nominal tile edge length in decimal degrees at `level`:
    /// `90°/2ⁿ` (= 45/2ⁿ⁻¹, exactly representable in binary floating point
    /// through n = 28). Nominal tiles are square; polar coalescence widens a
    /// tile by aggregating whole nominal columns, never changing `h`.
    fn h(level: GnosisLevel) -> f64 {
        90.0 * (0.5f64).powi(i32::from(level.value()))
    }

    /// The nominal tile-matrix size `(width, height)` in tiles at `level`
    /// (Requirement TCE6, §7.12.3.5): 4×2 of 90° tiles at level 0, each
    /// finer level doubling both — `4·2ⁿ × 2·2ⁿ`. Width counts nominal
    /// columns before coalescence.
    pub fn matrix_size(level: GnosisLevel) -> (u64, u64) {
        // level.value() ≤ 28, so the shift and products stay far below u64::MAX.
        let factor = 1u64 << u32::from(level.value());
        (4 * factor, 2 * factor)
    }

    /// The coalescence factor of nominal row `row` at `level` (the
    /// registered TileMatrixSet's variableMatrixWidths, Requirement TCE2-B):
    /// with pole distance `d = min(row, height − 1 − row)` and
    /// `bit_length(d)` the bit count of `d`'s binary form
    /// (`bit_length(0) = 0`), the factor is `2^max(0, n − bit_length(d))` —
    /// `2ⁿ` at the poles (so exactly 4 tiles touch each pole, each 90°
    /// wide), halving with each doubling of pole distance, 1 in the
    /// equatorial half. Verified verbatim against the registry's level 0–3
    /// tables. Callers pass an in-range row.
    fn factor_for_row(level: GnosisLevel, row: u64) -> u32 {
        let (_, height) = Self::matrix_size(level);
        let d = row.min(height - 1 - row);
        let bit_length = 64 - d.leading_zeros();
        let n = u32::from(level.value());
        if bit_length >= n {
            1
        } else {
            // n − bit_length ≤ 28: a valid, non-overflowing u32 shift.
            1u32 << (n - bit_length)
        }
    }

    /// The coalescence factor for `row` at `level` (2DTMS variable-width
    /// tiling, Requirement TCE2, §7.12.3.2): how many nominal columns the
    /// row aggregates into each tile — `2ⁿ` at the poles, re-adjusted at
    /// every tile matrix (§7.12.4), unlike the CDB1GlobalGrid's constant
    /// latitude bands. Returns [`TilingViolation::GnosisTileOutOfRange`] if
    /// `row` is outside the level's matrix height; only the row is checked
    /// here, so that violation carries `col: 0` as a not-applicable
    /// sentinel.
    pub fn coalescence_factor(level: GnosisLevel, row: u64) -> Result<u32, TilingViolation> {
        let (_, height) = Self::matrix_size(level);
        if row >= height {
            return Err(TilingViolation::GnosisTileOutOfRange {
                level: level.value(),
                row,
                col: 0,
            });
        }
        Ok(Self::factor_for_row(level, row))
    }

    /// Builds a validated [`GnosisTileAddress`] from raw indices
    /// (Requirements TCE6/TCE2-B, §7.12.3.5/§7.12.3.2): `row`/`col` must lie
    /// within [`Self::matrix_size`] (else
    /// [`TilingViolation::GnosisTileOutOfRange`]), and `col` must be a
    /// multiple of the row's coalescence factor (else
    /// [`TilingViolation::MisalignedColumn`]) — a misaligned column names no
    /// real tile under the 2DTMS variable-width rule.
    pub fn address(
        level: GnosisLevel,
        row: u64,
        col: u64,
    ) -> Result<GnosisTileAddress, TilingViolation> {
        let (width, height) = Self::matrix_size(level);
        if row >= height || col >= width {
            return Err(TilingViolation::GnosisTileOutOfRange {
                level: level.value(),
                row,
                col,
            });
        }
        let factor = Self::factor_for_row(level, row);
        if !col.is_multiple_of(u64::from(factor)) {
            return Err(TilingViolation::MisalignedColumn { col, factor });
        }
        Ok(GnosisTileAddress { level, row, col })
    }

    /// The tile at `level` containing the decimal-degree point `(lat, lon)`
    /// (Requirements TCE3/TCE4, §7.12.3.3–§7.12.3.4; the parameter order is
    /// the scheme's latitude,longitude axis order). The coordinate must be
    /// finite with latitude in [−90, 90] and longitude in [−180, 180], else
    /// [`TilingViolation::CoordinateOutOfRange`]. The point falls in the
    /// tile whose half-open extent `[south, north) × [west, east)` contains
    /// it — the binding tile-addressing convention shared with the
    /// CDB1GlobalGrid: `row = ⌈(90−lat)/h⌉ − 1` (south-inclusive),
    /// `col = ⌊(lon+180)/h⌋` (west-inclusive), ±90°/±180° edges clamped to
    /// the last in-range row/column — with the nominal column snapped down
    /// to the row's coalescence factor so the address names the real
    /// (possibly widened) tile.
    pub fn tile_at(
        lat: f64,
        lon: f64,
        level: GnosisLevel,
    ) -> Result<GnosisTileAddress, TilingViolation> {
        if lat.is_nan() || lon.is_nan() || lat.abs() > 90.0 || lon.abs() > 180.0 {
            return Err(TilingViolation::CoordinateOutOfRange { lat, lon });
        }
        let h = Self::h(level);
        let (width, height) = Self::matrix_size(level);
        let row = (((90.0 - lat) / h).ceil() - 1.0)
            .max(0.0)
            .min((height - 1) as f64) as u64;
        let col_nominal = (((lon + 180.0) / h).floor() as u64).min(width - 1);
        let factor = Self::factor_for_row(level, row);
        let col = col_nominal - (col_nominal % u64::from(factor));
        Ok(GnosisTileAddress { level, row, col })
    }

    /// The geographic extent of `addr` as a WGS-84 bounding box in decimal
    /// degrees (Requirement TCE4, §7.12.3.4). North and south come from the
    /// row (`north = 90 − row·h`, `south = north − h`); west from the
    /// column (`west = −180 + col·h`); and east spans the row's coalescence
    /// factor (`east = west + factor·h`), so a polar tile is `factor`
    /// nominal columns — always 90° — wide. Because every factor divides
    /// the matrix width, the eastmost tile of a row ends exactly on +180°.
    pub fn tile_extent(addr: GnosisTileAddress) -> Bbox {
        let h = Self::h(addr.level);
        let factor = Self::factor_for_row(addr.level, addr.row);
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

    /// The tile that contains `addr` one zoom level coarser (Requirement
    /// TCE6-C, §7.12.3.5), or `None` at level 0 where the pyramid bottoms
    /// out: `(level − 1, row/2, col/2 snapped down to the parent row's
    /// coalescence factor)` — the same shape as the CDB1GlobalGrid's
    /// quadtree branch.
    pub fn parent(addr: GnosisTileAddress) -> Option<GnosisTileAddress> {
        let level_v = addr.level.value();
        if level_v == 0 {
            return None;
        }
        // level_v ≥ 1, so level_v − 1 is in range; `.ok()?` is a total,
        // panic-free way to obtain the coarser level.
        let parent_level = GnosisLevel::new(level_v - 1).ok()?;
        let row = addr.row / 2;
        let parent_factor = Self::factor_for_row(parent_level, row);
        let half = addr.col / 2;
        let col = half - (half % u64::from(parent_factor));
        Some(GnosisTileAddress {
            level: parent_level,
            row,
            col,
        })
    }

    /// The tiles one zoom level finer that partition `addr` (Requirement
    /// TCE6-C, §7.12.3.5), or empty at [`LEVEL_MAX`]. Uniform factor-driven
    /// enumeration: for each child row `2·row` and `2·row + 1`, step
    /// columns by that child row's own coalescence factor across the
    /// parent's doubled span `[2·col, 2·col + 2·factor)`. The 3-way pole
    /// split falls out of the formula — a polar parent's polar child has
    /// double the parent's factor (1 tile, never split longitude-wise) and
    /// its equator-ward row keeps it (2 tiles), so polar parents yield 3
    /// children and all others 4; every child is aligned by construction.
    pub fn children(addr: GnosisTileAddress) -> Vec<GnosisTileAddress> {
        let level_v = addr.level.value();
        if level_v == LEVEL_MAX {
            return Vec::new();
        }
        // level_v < LEVEL_MAX, so level_v + 1 is in range; an Err is
        // impossible and an empty child set is the safe, panic-free fallback.
        let child_level = match GnosisLevel::new(level_v + 1) {
            Ok(l) => l,
            Err(_) => return Vec::new(),
        };
        let factor = u64::from(Self::factor_for_row(addr.level, addr.row));
        let mut children = Vec::new();
        for child_row in [addr.row * 2, addr.row * 2 + 1] {
            let step = u64::from(Self::factor_for_row(child_level, child_row));
            let mut col = addr.col * 2;
            while col < addr.col * 2 + 2 * factor {
                children.push(GnosisTileAddress {
                    level: child_level,
                    row: child_row,
                    col,
                });
                col += step;
            }
        }
        children
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiling::TilingViolation;

    /// §7.12.3.2 Requirement TCE2-B /req/core/tiling-extension-tms — the
    /// registered TileMatrixSet defines tile matrices 0..=28; negative
    /// levels are unrepresentable by construction (`u8`).
    #[test]
    fn req_core_tiling_ext_gnosis_level_range() {
        assert!(GnosisLevel::new(0).is_ok());
        assert!(GnosisLevel::new(28).is_ok());
        for bad in [29u8, 255] {
            assert!(matches!(
                GnosisLevel::new(bad),
                Err(TilingViolation::GnosisLevelOutOfRange { level }) if level == bad
            ));
        }
    }

    fn level(v: u8) -> GnosisLevel {
        GnosisLevel::new(v).unwrap()
    }

    /// §7.12.3.5 Requirement TCE6-A/B /req/core/tiling-extension-start-lod
    /// — zoom level 0 ("tile matrix identifier 0") is a 2×4 grid of 90°×90°
    /// tiles, no coalescence, north-west origin (the registry's
    /// pointOfOrigin [90, −180], cornerOfOrigin topLeft).
    #[test]
    fn req_core_tiling_ext_gnosis_level0_start() {
        assert_eq!(GnosisGlobalGrid::matrix_size(level(0)), (4, 2));
        assert_eq!(
            GnosisGlobalGrid::coalescence_factor(level(0), 0).unwrap(),
            1
        );
        assert_eq!(
            GnosisGlobalGrid::coalescence_factor(level(0), 1).unwrap(),
            1
        );
        // Row 0 = northernmost, col 0 = −180°; each tile 90°×90°.
        let t = GnosisGlobalGrid::address(level(0), 0, 0).unwrap();
        assert_eq!((t.level(), t.row(), t.col()), (level(0), 0, 0));
        let b = GnosisGlobalGrid::tile_extent(t);
        assert_eq!(
            (b.west, b.south, b.east, b.north),
            (-180.0, 0.0, -90.0, 90.0)
        );
    }

    /// §7.12.3.2 Requirement TCE2-B /req/core/tiling-extension-tms — the
    /// OGC-registered GNOSISGlobalGrid definition, pinned verbatim: matrix
    /// sizes and variableMatrixWidths for tile matrices 0–3 (registry JSON;
    /// unlisted rows are factor 1), 256×256-cell tiles, highest matrix 28.
    #[test]
    fn req_core_tiling_ext_gnosis_registry_fixtures() {
        assert_eq!(TILE_SIZE_CELLS, 256);
        assert_eq!(LEVEL_MAX, 28);
        assert_eq!(GnosisGlobalGrid::matrix_size(level(1)), (8, 4));
        assert_eq!(GnosisGlobalGrid::matrix_size(level(2)), (16, 8));
        assert_eq!(GnosisGlobalGrid::matrix_size(level(3)), (32, 16));
        let tables: [(u8, &[(u64, u32)]); 3] = [
            (1, &[(0, 2), (1, 1), (2, 1), (3, 2)]),
            (2, &[(0, 4), (1, 2), (2, 1), (5, 1), (6, 2), (7, 4)]),
            (
                3,
                &[
                    (0, 8),
                    (1, 4),
                    (2, 2),
                    (3, 2),
                    (4, 1),
                    (11, 1),
                    (12, 2),
                    (13, 2),
                    (14, 4),
                    (15, 8),
                ],
            ),
        ];
        for (lvl, rows) in tables {
            for &(row, want) in rows {
                assert_eq!(
                    GnosisGlobalGrid::coalescence_factor(level(lvl), row).unwrap(),
                    want,
                    "level {lvl} row {row}"
                );
            }
        }
        // Bounds and alignment are enforced.
        assert!(matches!(
            GnosisGlobalGrid::coalescence_factor(level(0), 2),
            Err(TilingViolation::GnosisTileOutOfRange { .. })
        ));
        assert!(matches!(
            GnosisGlobalGrid::address(level(1), 0, 1),
            Err(TilingViolation::MisalignedColumn { col: 1, factor: 2 })
        ));
        assert!(matches!(
            GnosisGlobalGrid::address(level(1), 4, 0),
            Err(TilingViolation::GnosisTileOutOfRange { .. })
        ));
    }

    /// §7.12.3.3 Requirement TCE3 /req/core/tiling-extension-crs +
    /// §7.12.3.4 Requirement TCE4 /req/core/tiling-extension-uom —
    /// EPSG:4326 with axis order latitude,longitude, everything in decimal
    /// degrees; coordinates validated; ±90°/±180° edges clamp inward; a
    /// point lies inside its tile's half-open extent.
    #[test]
    fn req_core_tiling_ext_gnosis_crs_uom_coordinates() {
        for (lat, lon) in [(90.5, 0.0), (0.0, -180.5), (f64::NAN, 0.0), (0.0, f64::NAN)] {
            assert!(matches!(
                GnosisGlobalGrid::tile_at(lat, lon, level(2)),
                Err(TilingViolation::CoordinateOutOfRange { .. })
            ));
        }
        // Point round-trip at several levels (half-open containment).
        for lvl in [0u8, 1, 2, 5] {
            let t = GnosisGlobalGrid::tile_at(37.25, -122.75, level(lvl)).unwrap();
            let b = GnosisGlobalGrid::tile_extent(t);
            assert!(b.west <= -122.75 && -122.75 < b.east && b.south <= 37.25 && 37.25 < b.north);
        }
        // Edges clamp inward.
        let south = GnosisGlobalGrid::tile_at(-90.0, 0.0, level(1)).unwrap();
        assert_eq!(south.row(), 3);
        let east = GnosisGlobalGrid::tile_at(0.0, 180.0, level(1)).unwrap();
        assert_eq!(GnosisGlobalGrid::tile_extent(east).east, 180.0);
        let north = GnosisGlobalGrid::tile_at(90.0, -180.0, level(1)).unwrap();
        assert_eq!((north.row(), north.col()), (0, 0));
        // A nominal column inside a coalesced polar tile snaps west.
        let polar = GnosisGlobalGrid::tile_at(89.0, 50.0, level(2)).unwrap();
        assert_eq!(polar.row(), 0);
        assert_eq!(polar.col() % 4, 0);
    }

    /// §7.12.3.5 Requirement TCE6-C /req/core/tiling-extension-start-lod —
    /// tile splitting per the 2DTMS GNOSISGlobalGrid annexes: pole-touching
    /// tiles split 3-way (the polar child is never split longitude-wise),
    /// all others 4-way; children exactly tile the parent; parent inverts
    /// children; the chain ends at levels 0 and 28.
    #[test]
    fn req_core_tiling_ext_gnosis_splitting() {
        // A north-polar parent → 3 children; the polar child stays 90° wide.
        let polar = GnosisGlobalGrid::address(level(1), 0, 0).unwrap();
        let kids = GnosisGlobalGrid::children(polar);
        assert_eq!(kids.len(), 3);
        let pb = GnosisGlobalGrid::tile_extent(polar);
        let mut area = 0.0;
        for k in &kids {
            assert_eq!(GnosisGlobalGrid::parent(*k), Some(polar));
            let kb = GnosisGlobalGrid::tile_extent(*k);
            assert!(
                kb.west >= pb.west
                    && kb.east <= pb.east
                    && kb.south >= pb.south
                    && kb.north <= pb.north
            );
            area += (kb.east - kb.west) * (kb.north - kb.south);
        }
        assert!((area - (pb.east - pb.west) * (pb.north - pb.south)).abs() < 1e-9);
        let polar_child = kids.iter().find(|k| k.row() == 0).unwrap();
        let pcb = GnosisGlobalGrid::tile_extent(*polar_child);
        assert_eq!((pcb.east - pcb.west, pcb.north), (90.0, 90.0));
        // The south pole mirrors.
        let south = GnosisGlobalGrid::address(level(1), 3, 0).unwrap();
        assert_eq!(GnosisGlobalGrid::children(south).len(), 3);
        // A non-pole tile quad-splits, and the children invert.
        let mid = GnosisGlobalGrid::address(level(1), 1, 2).unwrap();
        let kids = GnosisGlobalGrid::children(mid);
        assert_eq!(kids.len(), 4);
        for k in kids {
            assert_eq!(GnosisGlobalGrid::parent(k), Some(mid));
        }
        // The chain ends at the range limits.
        assert_eq!(
            GnosisGlobalGrid::parent(GnosisGlobalGrid::address(level(0), 0, 0).unwrap()),
            None
        );
        assert!(
            GnosisGlobalGrid::children(GnosisGlobalGrid::tile_at(0.0, 0.0, level(28)).unwrap())
                .is_empty()
        );
    }

    /// §7.12.2 (binding via TCE2-B) — exactly 4 real tiles touch each pole
    /// at every level, each 90° wide.
    #[test]
    fn req_core_tiling_ext_gnosis_four_pole_tiles() {
        for lvl in [1u8, 2, 3, 4] {
            let (width, height) = GnosisGlobalGrid::matrix_size(level(lvl));
            for row in [0, height - 1] {
                let f = GnosisGlobalGrid::coalescence_factor(level(lvl), row).unwrap();
                assert_eq!(width / u64::from(f), 4, "level {lvl} row {row}");
                let t = GnosisGlobalGrid::address(level(lvl), row, 0).unwrap();
                let b = GnosisGlobalGrid::tile_extent(t);
                assert_eq!(b.east - b.west, 90.0);
            }
        }
    }

    /// §7.12.4 (binding via TCE2-B) — GNOSIS coalescence factors re-adjust
    /// at each tile matrix, unlike the CDB1GlobalGrid's per-latitude-band
    /// constancy: the polar row's factor is 2ⁿ at level n while a CDB1
    /// polar geocell holds factor 12 down its pyramid.
    #[test]
    fn req_core_tiling_ext_gnosis_coalescence_readjusts() {
        for (lvl, want) in [(1u8, 2u32), (2, 4), (3, 8), (4, 16)] {
            assert_eq!(
                GnosisGlobalGrid::coalescence_factor(level(lvl), 0).unwrap(),
                want
            );
        }
        use crate::tiling::cdb1_grid::{Cdb1GlobalGrid, Cdb1Lod};
        for l in [0i8, 1, 3] {
            let lod = Cdb1Lod::new(l).unwrap();
            let t = Cdb1GlobalGrid::tile_at(89.5, 0.5, lod).unwrap();
            assert_eq!(
                Cdb1GlobalGrid::coalescence_factor(lod, t.row()).unwrap(),
                12
            );
        }
    }

    /// §7.12.2 (binding via TCE2-B) — "the matrix, row and column
    /// identifiers can still fit together within a single 64-bit key":
    /// 5 + 29 + 30 bits, exactly full at level 28. Round-trips, numeric key
    /// order = (level, row, col) order, invalid encodings rejected.
    #[test]
    fn req_core_tiling_ext_gnosis_key_roundtrip() {
        let mut prev_level_origin = None;
        for lvl in [0u8, 1, 4, 28] {
            let (width, height) = GnosisGlobalGrid::matrix_size(level(lvl));
            let f_top = GnosisGlobalGrid::coalescence_factor(level(lvl), 0).unwrap();
            let f_bot = GnosisGlobalGrid::coalescence_factor(level(lvl), height - 1).unwrap();
            let corners = [
                (0, 0),
                (0, width - u64::from(f_top)),
                (height - 1, 0),
                (height - 1, width - u64::from(f_bot)),
                (height / 2, 0),
            ];
            for (row, col) in corners {
                let addr = GnosisGlobalGrid::address(level(lvl), row, col).unwrap();
                let key = addr.key();
                assert_eq!(GnosisTileAddress::from_key(key).unwrap(), addr);
                if let Some(origin) = prev_level_origin {
                    assert!(key > origin, "level bits dominate the ordering");
                }
            }
            prev_level_origin = Some(GnosisGlobalGrid::address(level(lvl), 0, 0).unwrap().key());
        }
        // Within a level the order is (row, col) lexicographic.
        let a = GnosisGlobalGrid::address(level(2), 1, 0).unwrap().key();
        let b = GnosisGlobalGrid::address(level(2), 1, 2).unwrap().key();
        let c = GnosisGlobalGrid::address(level(2), 2, 0).unwrap().key();
        assert!(a < b && b < c);
        // Invalid encodings are rejected with the addressing violations.
        assert!(matches!(
            GnosisTileAddress::from_key(29u64 << 59),
            Err(TilingViolation::GnosisLevelOutOfRange { level: 29 })
        ));
        assert!(matches!(
            GnosisTileAddress::from_key(2u64 << 30), // level 0, row 2 ≥ height 2
            Err(TilingViolation::GnosisTileOutOfRange { .. })
        ));
        assert!(matches!(
            GnosisTileAddress::from_key((1u64 << 59) | 1), // level 1, row 0 has factor 2
            Err(TilingViolation::MisalignedColumn { .. })
        ));
    }
}
