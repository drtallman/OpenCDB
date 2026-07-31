//! `rusty_cdb` — a Rust API for reading, writing, and validating datastores
//! conformant with the OGC CDB 2.0 Core Standard (OGC 23-034).
//!
//! The CDB 2.0 Core is abstract by design; this crate encodes each core
//! requirements module as a Rust module, plus an application-profile layer
//! that makes the core implementable. Development is strictly test-driven
//! against the spec — see `docs/TDD_PLAN.md`.

pub mod crs;
pub mod datastore;
pub mod error;
pub mod hierarchy;
pub mod links;
pub mod media_types;
pub mod metadata;
pub mod naming;
pub mod profiles;

pub use datastore::{CdbViolation, CdbWarning, ConformanceReport};
pub use error::CdbError;
pub use profiles::{ApplicationProfile, RequirementsClass, SimulationProfile};
