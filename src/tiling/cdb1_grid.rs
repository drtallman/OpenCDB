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
