//! Conformance vocabulary and the datastore validator (Annex A
//! `/conf/minimal-core`).
//!
//! This module owns the class vocabulary every profile declares *from*
//! ([`RequirementsClass`]), the two finding kinds a run produces
//! ([`CdbViolation`] for a spec **SHALL**, [`CdbWarning`] for a spec
//! **SHOULD** — never conflated), the [`ConformanceReport`] that buckets
//! them by class, and [`validate`], the orchestrator behind
//! [`crate::datastore::CdbDatastore::validate`].
//!
//! Conformance is decided by violations alone:
//! [`ConformanceReport::is_conformant`] and
//! [`ConformanceReport::class_passed`] ignore warnings — and so does
//! [`ConformanceReport::class_has_content`], which separates a class checked
//! against real content from one checked against none.
//!
//! Each optional class's stage delegates to a free `validate_*` function
//! owned by the requirements module itself
//! ([`crate::coverage::validate_coverage_instance`],
//! [`crate::tiling::validate_tileset_metadata`],
//! [`crate::topology::validate_topology_dataset`],
//! [`crate::geometry::validate_geometry_metadata`],
//! [`crate::attribution::validate_attribute_model_document`],
//! [`crate::versioning::validate_journal`]) rather than to a validator
//! registry: the rules stay next to the types they judge.

mod class;
mod finding;
mod report;
mod validate;

pub use class::RequirementsClass;
pub use finding::{CdbViolation, CdbWarning};
pub use report::{ClassFindings, ConformanceReport};
pub use validate::validate;
