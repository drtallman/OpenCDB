//! `cdb_migrator` reads OGC CDB 1.x datastores and migrates them to OGC CDB
//! 2.0 datastores through `opencdb`'s public facade. Payloads are opaque
//! bytes: this crate copies them, never decodes them.
pub mod address;
mod error;
pub mod grammar;
pub mod reader;
pub mod version_meta;

pub use error::Cdb1Error;

pub use reader::{Cdb1Entry, Cdb1TileRef, Cdb1Tree, GlobalKind, Inventory};

pub mod metadata_input;
pub mod migrate;
