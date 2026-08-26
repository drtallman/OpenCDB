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
}
