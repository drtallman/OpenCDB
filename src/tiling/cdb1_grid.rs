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

use crate::tiling::TilingViolation;

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
/// only constructor, [`Lod::new`], rejects out-of-range values, so every `Lod`
/// value is in range by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Lod(i8);

impl Lod {
    /// Constructs a Level of Detail, enforcing Requirement TCE7-A: `value`
    /// must lie in [`LOD_MIN`]..=[`LOD_MAX`] (−10..=23). Returns
    /// [`TilingViolation::LodOutOfRange`] for any value outside that range.
    pub fn new(value: i8) -> Result<Lod, TilingViolation> {
        if (LOD_MIN..=LOD_MAX).contains(&value) {
            Ok(Lod(value))
        } else {
            Err(TilingViolation::LodOutOfRange { lod: value })
        }
    }

    /// The LoD's integer value, guaranteed to lie in
    /// [`LOD_MIN`]..=[`LOD_MAX`].
    pub fn value(self) -> i8 {
        self.0
    }
}

/// The CDB1GlobalGrid tiling scheme (spec §7.11): the CDB 1.x-compatible
/// global grid whose raster sizing and tile addressing this type computes. A
/// unit struct — its operations are level/geocell arithmetic with no
/// per-instance state; tile addressing and zone math accrue in later tasks,
/// with [`Self::raster_size`] the first.
pub struct Cdb1GlobalGrid;

impl Cdb1GlobalGrid {
    /// The raster edge length, in cells, of a tile at `lod` (Requirements
    /// TCE7-B and TCE7-C, §7.11.3.6). At LoD 0 and finer every tile is
    /// [`TILE_SIZE_CELLS`] (1024) cells square; each negative LoD keeps the
    /// whole-geocell extent while halving the cell count — 512 at −1, 256 at
    /// −2, … 1 at −10 ([`LOD_MIN`]).
    pub fn raster_size(lod: Lod) -> u32 {
        let value = lod.value();
        if value >= 0 {
            TILE_SIZE_CELLS
        } else {
            // value ∈ −10..=−1, so 10 + value ∈ 0..=9: a valid, non-overflowing
            // u32 shift. Widen through i32 to avoid i8 arithmetic entirely.
            1u32 << (10 + i32::from(value))
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
        assert!(Lod::new(-10).is_ok());
        assert!(Lod::new(0).is_ok());
        assert!(Lod::new(23).is_ok());
        for bad in [-11i8, 24, i8::MIN, i8::MAX] {
            assert!(matches!(
                Lod::new(bad),
                Err(TilingViolation::LodOutOfRange { lod }) if lod == bad
            ));
        }
    }

    /// §7.11.3.6 Requirement TCE7-B/C — tiles are 1024×1024 cells from
    /// LoD 0 up; negative LoDs keep the geocell extent with halved cells.
    #[test]
    fn req_core_tiling_ext_raster_sizes() {
        assert_eq!(Cdb1GlobalGrid::raster_size(Lod::new(0).unwrap()), 1024);
        assert_eq!(Cdb1GlobalGrid::raster_size(Lod::new(23).unwrap()), 1024);
        assert_eq!(Cdb1GlobalGrid::raster_size(Lod::new(-1).unwrap()), 512);
        assert_eq!(Cdb1GlobalGrid::raster_size(Lod::new(-4).unwrap()), 64);
        assert_eq!(Cdb1GlobalGrid::raster_size(Lod::new(-10).unwrap()), 1);
    }
}
