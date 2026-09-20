//! `cdb_migrator` reads OGC CDB 1.x datastores and migrates them to OGC CDB
//! 2.0 datastores through `opencdb`'s public facade. Payloads are opaque
//! bytes: this crate copies them, never decodes them.
mod error;

pub use error::Cdb1Error;
