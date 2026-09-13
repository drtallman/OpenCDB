//! Process exit codes.
//!
//! A CI job reads `$?` and nothing else, so these four numbers carry the
//! whole verdict for most of cdb-lint's audience. The set is deliberately
//! four rather than three: [`OPERATIONAL`] keeps "the tool could not look at
//! the datastore" apart from [`USAGE`]'s "you asked for something
//! impossible" and [`FINDINGS`]'s "the datastore does not conform".
//! `CdbDatastore::validate` already draws that line — findings come back as
//! `Ok`, and `Err` is reserved for I/O that prevents inspection — and
//! cdb-lint preserves it rather than flattening it (design §4.1).

/// The datastore conforms; under `--baseline`, no new findings appeared.
pub const OK: i32 = 0;

/// The run produced findings that fail the build: violations, or — under
/// `--deny-warnings` — warnings. Warnings alone never reach this code
/// otherwise, because a warning is a SHOULD and a SHOULD does not decide
/// conformance.
pub const FINDINGS: i32 = 1;

/// The command line or the profile descriptor was wrong: an unknown flag, a
/// repeated flag, a missing value, an unreadable or self-contradictory
/// descriptor, or a baseline taken under another profile. Nothing was
/// judged, because the yardstick itself was unusable.
pub const USAGE: i32 = 2;

/// The tool could not inspect the datastore: a `CdbError` out of `open` or
/// `validate`, or a report that could not be written. This is a fact about
/// the run, never about the datastore's conformance — a missing directory
/// exits 3, not 1.
pub const OPERATIONAL: i32 = 3;

#[cfg(test)]
mod tests {
    use super::*;

    /// The four codes are a published interface: a CI job branches on them,
    /// so their numeric values are as much a contract as any function
    /// signature. Pinning them here makes a renumbering a deliberate act.
    #[test]
    fn cli_exit_codes_have_their_published_values() {
        assert_eq!(OK, 0);
        assert_eq!(FINDINGS, 1);
        assert_eq!(USAGE, 2);
        assert_eq!(OPERATIONAL, 3);
    }

    /// Exit 3 exists so that "the datastore is non-conformant" and "the tool
    /// could not look at the datastore" are different facts on the wire
    /// (design §4.1). A build that collapsed them would report a broken mount
    /// as a failed audit.
    #[test]
    fn cli_exit_operational_is_distinct_from_usage_and_findings() {
        assert_ne!(OPERATIONAL, USAGE);
        assert_ne!(OPERATIONAL, FINDINGS);
        assert_ne!(OPERATIONAL, OK);
    }
}
