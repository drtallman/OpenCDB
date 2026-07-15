//! Crate-wide error taxonomy: one variant family per requirements module.

use thiserror::Error;

use crate::naming::NamingViolation;

/// Top-level error for the `rusty_cdb` API.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CdbError {
    /// Resource Path and File Naming module (spec §7.4).
    #[error(transparent)]
    Naming(#[from] NamingViolation),
}
