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
//! A report is also a **serde surface** (`Serialize`): every finding carries
//! a stable machine-readable code — the OGC clause in requirement-URI form,
//! [`CdbViolation::code`] / [`CdbWarning::code`] — so a consumer keys on the
//! code and never parses `Display` text. The shape is documented on
//! [`ConformanceReport`]'s `Serialize` impl and freezes at 1.0.
//!
//! Conformance is decided by violations alone:
//! [`ConformanceReport::is_conformant`] and
//! [`ConformanceReport::class_passed`] ignore warnings — and so does
//! [`ConformanceReport::class_coverage`], which separates the three ways a
//! class can pass ([`ContentCoverage`]): its content was checked and is
//! clean, there was no such content, or there was content this crate has no
//! datastore-level check for. The last is not a defect to be closed but the
//! honest position of Geometry and Topology, whose subjects live in payloads
//! the crate does not decode.
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
pub use report::{ClassFindings, ConformanceReport, ContentCoverage};
pub use validate::validate;
