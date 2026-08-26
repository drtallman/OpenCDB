//! `rusty_cdb` — a Rust API for reading, writing, and validating datastores
//! conformant with the OGC CDB 2.0 Core Standard (OGC 23-034).
//!
//! The CDB 2.0 Core is abstract by design; this crate encodes each core
//! requirements module as a Rust module, plus an application-profile layer
//! that makes the core implementable. Development is strictly test-driven
//! against the spec — see `docs/TDD_PLAN.md`.
//!
//! # Example
//!
//! Create a datastore from the default simulation profile and validate it
//! against Annex A `/conf/minimal-core`:
//!
//! ```
//! use rusty_cdb::{CdbDatastore, DatastoreSeed, SimulationProfile};
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let tmp = tempfile::tempdir()?;
//! let profile = SimulationProfile::json();
//! let seed = DatastoreSeed::new("MyStore", "My Store", "Demo datastore", "ops@example.com");
//! let datastore = CdbDatastore::create(tmp.path(), &profile, seed)?;
//! let report = datastore.validate(&profile)?;
//! assert!(report.is_conformant());
//! # Ok(()) }
//! ```

pub mod coverage;
pub mod crs;
pub mod datastore;
pub mod error;
pub mod geometry;
pub mod hierarchy;
pub mod links;
pub mod media_types;
pub mod metadata;
pub mod naming;
pub mod profiles;
pub mod tiling;

pub use coverage::{
    CoverageViolation, CoverageWarning, DomainSet, GridCellEncoding, GridCorner,
    validate_coverage_instance,
};
pub use datastore::{CdbDatastore, CdbViolation, CdbWarning, ConformanceReport, DatastoreSeed};
pub use error::CdbError;
pub use geometry::{
    CdbGeometry, GeometryCode, GeometryContext, GeometryViolation, LineStringM, LineStringZ,
    MultiPointM, MultiPointZ, PointM, PointZ, PolygonM, PolygonZ,
};
pub use profiles::{ApplicationProfile, RequirementsClass, SimulationProfile};
pub use tiling::{
    Cdb1GlobalGrid, Cdb1Lod, Cdb1TileAddress, GnosisGlobalGrid, GnosisLevel, GnosisTileAddress,
    TilingScheme, TilingSchemeId, TilingViolation, TilingWarning, validate_tileset_metadata,
};
