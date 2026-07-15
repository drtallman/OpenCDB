//! Crate-wide error taxonomy: one variant family per requirements module.

use thiserror::Error;

use crate::hierarchy::HierarchyError;
use crate::links::LinkViolation;
use crate::metadata::MetadataError;
use crate::naming::NamingViolation;

/// Top-level error for the `rusty_cdb` API.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CdbError {
    /// Resource Path and File Naming module (spec §7.4).
    #[error(transparent)]
    Naming(#[from] NamingViolation),
    /// File Hierarchy Structure module (spec §7.5).
    #[error(transparent)]
    Hierarchy(#[from] HierarchyError),
    /// Links module (spec §7.7).
    #[error(transparent)]
    Link(#[from] LinkViolation),
    /// Global and resource metadata module (spec §7.9).
    #[error(transparent)]
    Metadata(#[from] MetadataError),
}
