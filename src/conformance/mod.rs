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
//! [`ConformanceReport::class_passed`] ignore warnings.

mod class;
mod finding;
mod report;
mod validate;

pub use class::RequirementsClass;
pub use finding::{CdbViolation, CdbWarning};
pub use report::{ClassFindings, ConformanceReport};
pub use validate::validate;
