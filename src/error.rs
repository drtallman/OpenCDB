//! Crate-wide error taxonomy: one variant family per requirements module.

use thiserror::Error;

use crate::coverage::CoverageViolation;
use crate::crs::CrsError;
use crate::geometry::GeometryViolation;
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
    /// Coordinate Reference System module (spec §7.3).
    #[error(transparent)]
    Crs(#[from] CrsError),
    /// A geometry requirements violation (/req/core/geometry, §7.6).
    #[error(transparent)]
    Geometry(#[from] GeometryViolation),
    /// A coverages requirements violation (/req/core/coverages-, §7.2).
    #[error(transparent)]
    Coverage(#[from] CoverageViolation),
}
