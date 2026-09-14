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

/// Whether a completed, unratcheted run's findings fail the build:
/// violations always do, warnings only when `--deny-warnings` asks.
///
/// This predicate is written once, here, and read from two places — the
/// exit code in `lib::verdict_code` and the verdict line's flag attribution
/// in `render::text` — because honesty rule 4's promise is exactly that the
/// two agree: the line names the flag that moved the code, and two
/// independent spellings of "moved" would be two chances to drift apart
/// silently.
pub(crate) fn fails(conformant: bool, has_warnings: bool, deny_warnings: bool) -> bool {
    !conformant || (deny_warnings && has_warnings)
}

/// Whether a ratcheted run fails the build: only what is **new** since the
/// baseline counts — new violations always, new warnings when
/// `--deny-warnings` asks. Shared for the same reason as [`fails`].
pub(crate) fn ratchet_fails(
    new_violations: usize,
    new_warnings: usize,
    deny_warnings: bool,
) -> bool {
    new_violations > 0 || (deny_warnings && new_warnings > 0)
}

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

    /// The one failing predicate, as its whole truth table: violations fail,
    /// warnings fail only when denied, and a ratchet asks the same two
    /// questions of what is *new*. `lib::verdict_code` and the verdict line
    /// both read these, so the table here is the table everywhere.
    #[test]
    fn cli_exit_the_failing_predicate_has_one_truth_table() {
        // (conformant, has_warnings, deny) → fails
        for (conformant, has_warnings, deny, expected) in [
            (true, false, false, false),
            (true, true, false, false),
            (true, true, true, true),
            (true, false, true, false),
            (false, false, false, true),
            (false, true, true, true),
        ] {
            assert_eq!(
                fails(conformant, has_warnings, deny),
                expected,
                "fails({conformant}, {has_warnings}, {deny})"
            );
        }
        // (new violations, new warnings, deny) → ratchet_fails
        for (violations, warnings, deny, expected) in [
            (0, 0, false, false),
            (0, 3, false, false),
            (0, 3, true, true),
            (1, 0, false, true),
            (1, 0, true, true),
            (0, 0, true, false),
        ] {
            assert_eq!(
                ratchet_fails(violations, warnings, deny),
                expected,
                "ratchet_fails({violations}, {warnings}, {deny})"
            );
        }
    }
}
