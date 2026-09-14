//! Reading a previous run back, and working out what is new since.
//!
//! # A record of a run, not a report
//!
//! [`ReportSnapshot`] is **not** a `ConformanceReport` wearing a disguise, and
//! naming it as one would be the first of several small lies. The library's
//! report types are `Serialize` only, and that is a decision rather than an
//! oversight: the wire shape is lossy by design, and `{code, class, severity,
//! message}` cannot reconstruct a `NamingViolation::ContainsSpace { name }`
//! without parsing `Display` text — the one thing this codebase forbids
//! everywhere else. So cdb-lint owns a type that is honestly a *record of a
//! previous run*: four fields per finding, none of them a `CdbViolation`, and
//! no method that pretends otherwise.
//!
//! # Identity is `(class, code, severity) → count`
//!
//! A finding is **new** when its triple is absent from the baseline, or when
//! this run's count for that triple exceeds the baseline's. Three choices are
//! packed into that sentence and each is deliberate:
//!
//! - **The message text is excluded.** `src/lib.rs` in the library states that
//!   a finding's message may change within `1.x` while its `code` may not, so
//!   a message-keyed baseline would fail spuriously the day the library
//!   improved a sentence. The exclusion looks like laziness until you know
//!   that; it is the same contract every `code()` in the crate exists to serve.
//! - **The severity is included.** Nothing guarantees a code stays on one side
//!   of the SHALL/SHOULD line forever, and a clause that moved from warning to
//!   violation would otherwise be absorbed by a baseline that only ever saw it
//!   as a warning.
//! - **Counts, not sets.** A second occurrence of a code the baseline already
//!   records is new. A ratchet that counted only distinct codes would sit
//!   quietly while one datastore got steadily worse in one place.
//!
//! The class in a key is the class the report **filed the finding under** —
//! the entry it sits in — on both sides of the comparison, so the two are
//! derived the same way. (The library files every finding under its own
//! `class()`, so the finding's own `class` field always agrees; it is read
//! back for completeness and not keyed on.)
//!
//! # Tolerant at runtime, strict in a test
//!
//! Two needs pull in opposite directions, and they get different answers. At
//! runtime the DTO is **tolerant of unknown fields**, because a baseline
//! written by a newer cdb-lint — or by a newer library that added a field —
//! must still read; `#[serde(deny_unknown_fields)]` here would turn a forward
//! step into a broken build. The strictness lives in
//! `tests/cli_baseline.rs::cli_baseline_the_snapshot_mirrors_the_frozen_wire_shape`,
//! which serializes a **real** report and asserts its exact key sets, so the
//! DTO cannot silently drift from the shape `1.0` froze.
//!
//! # A baseline from another profile is refused
//!
//! [`ReportSnapshot::require_profile`] rejects a snapshot whose `profile`
//! differs from the run's, with [`crate::exit::USAGE`]: a `gnosis` baseline
//! applied to a `simulation` run would silently absorb findings the two
//! profiles judge differently. Root mismatches are **ignored** — CI paths
//! move, and a datastore's identity is not its path.
//!
//! **The hole in that check, stated plainly:** it compares the profile *name*,
//! which is all the wire shape records. Two different descriptor profiles that
//! share a `name` pass it, and cdb-lint cannot close that from here — the
//! report carries a string, not a profile. A run that mints its baselines from
//! the same descriptor it checks against is safe; a shop with two descriptors
//! both named `acme` is not, and no amount of care inside this module changes
//! that.
//!
//! # The hazard
//!
//! A baseline can exit 0 over a datastore that does not conform. That is what
//! a ratchet is, and it is the most dangerous thing this tool does. Nothing in
//! this module hides it: the diff is an artifact in its own right, the report
//! is untouched, and the verdict line in [`crate::render::text`] names the flag
//! that moved the exit code (design §5 rule 4).

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use rusty_cdb::conformance::ConformanceReport;

use crate::cli::UsageError;

/// The wire token in a finding's `severity` field for a SHALL finding.
///
/// Both tokens are written here rather than borrowed from the library, which
/// spells them in its serializer and exposes no constant. The wire-shape guard
/// in `tests/cli_baseline.rs` asserts a real report still uses them, so the
/// duplication cannot drift unnoticed.
pub const VIOLATION: &str = "violation";

/// The wire token in a finding's `severity` field for a SHOULD finding.
pub const WARNING: &str = "warning";

/// A previous run's report, read back from a `--format json` document.
///
/// Every field mirrors the frozen wire shape. Two of them —
/// [`Self::root`] and [`Self::conformant`] — are recorded and deliberately not
/// judged on: the root because a datastore's identity is not its path, and the
/// verdict because *this* run's report decides this run's verdict.
#[derive(Debug, Clone, Deserialize)]
pub struct ReportSnapshot {
    /// The name of the application profile the previous run was measured
    /// against. [`Self::require_profile`] holds this run to the same one.
    pub profile: String,
    /// The datastore root the previous run judged, as the lossy string the
    /// wire shape records. Never compared: CI paths move.
    pub root: String,
    /// The previous run's verdict. Recorded for a reader; this run's report is
    /// what decides this run's verdict.
    pub conformant: bool,
    /// One entry per requirements class the previous run listed.
    pub classes: Vec<ClassSnapshot>,
}

/// One class's entry in a previous run's report.
#[derive(Debug, Clone, Deserialize)]
pub struct ClassSnapshot {
    /// The class token, e.g. `file-naming`. A plain `String` rather than a
    /// `RequirementsClass`, so a baseline naming a class this build has never
    /// heard of still reads instead of failing the run.
    pub class: String,
    /// The class's §7 requirements-module URI, as recorded.
    pub requirements_uri: String,
    /// Whether the class recorded no violations, as recorded.
    pub passed: bool,
    /// The previous run's coverage token — `checked`, `none`, or `unchecked`.
    /// [`crate::render::sarif`] compares it against this run's to decide
    /// whether a class-level result is `new` or `unchanged`.
    pub content: String,
    /// The coarse form of [`Self::content`], as recorded.
    pub has_content: bool,
    /// The class's violations.
    pub violations: Vec<FindingSnapshot>,
    /// The class's warnings.
    pub warnings: Vec<FindingSnapshot>,
}

/// One finding in a previous run's report: the four flat fields the wire shape
/// carries, and nothing reconstructed.
#[derive(Debug, Clone, Deserialize)]
pub struct FindingSnapshot {
    /// The stable requirement-URI code — half the identity.
    pub code: String,
    /// The class the previous run filed this finding under. It always equals
    /// the enclosing entry's `class`, because the library files a finding under
    /// its own `class()`; the key is taken from the entry so that both sides of
    /// the comparison are derived the same way.
    pub class: String,
    /// `violation` or `warning` — the other half of the identity.
    pub severity: String,
    /// The previous run's message. Read back so the DTO mirrors the wire shape,
    /// and **never** compared: message text may change within `1.x`.
    pub message: String,
}

impl ReportSnapshot {
    /// Reads a baseline document.
    ///
    /// # Errors
    ///
    /// A baseline that is absent, unreadable, not JSON, or not a report is a
    /// [`UsageError`], which the caller answers with [`crate::exit::USAGE`]:
    /// nothing was judged, and the fault is in the request rather than in any
    /// datastore (design §4.1). Every message names the file and says what the
    /// flag expected, because a user who pointed `--baseline` at the wrong path
    /// needs to know what the right one looks like.
    pub fn load(path: &Path) -> Result<Self, UsageError> {
        let text = fs::read_to_string(path)
            .map_err(|error| rejected(path, format_args!("cannot be read: {error}")))?;

        serde_json::from_str(&text)
            .map_err(|error| rejected(path, format_args!("is not a report: {error}")))
    }

    /// Holds this run to the profile the baseline was taken under.
    ///
    /// # Errors
    ///
    /// A [`UsageError`] when the two names differ. Two profiles judge the same
    /// datastore by different rules, so a baseline from one applied to a run
    /// under the other would silently absorb real findings — the failure mode a
    /// ratchet must never have.
    ///
    /// The comparison is on the *name*, which is all the wire shape records;
    /// the module docs state what that cannot catch.
    pub fn require_profile(&self, expected: &str, source: &Path) -> Result<(), UsageError> {
        if self.profile == expected {
            return Ok(());
        }

        Err(rejected(
            source,
            format_args!(
                "was taken under profile `{}`, and this run is measured against `{expected}`. \
                 Two profiles judge a datastore by different rules, so a baseline from one \
                 would silently absorb findings the other means to report. Re-mint the \
                 baseline under `{expected}`, or run against the profile it was taken under",
                self.profile
            ),
        ))
    }

    /// Compares `report` against this record of a previous run.
    ///
    /// `source` is the file the snapshot was read from; it is carried into the
    /// diff so the rendered section can name it, and is used for nothing else.
    pub fn compare(&self, report: &ConformanceReport, source: &Path) -> BaselineDiff {
        let recorded = self.tally();
        let current = tally_report(report);

        let mut rows = Vec::new();
        let mut new_violations = 0;
        let mut new_warnings = 0;
        for (key, &now) in &current {
            let before = recorded.get(key).copied().unwrap_or_default();
            if now <= before {
                continue;
            }
            match key.severity.as_str() {
                VIOLATION => new_violations += now - before,
                WARNING => new_warnings += now - before,
                // Unreachable from a real run — every key on this side of the
                // comparison was built by `tally_report` out of the two tokens
                // above — and deliberately harmless if it ever is not: a
                // severity this build does not know is reported and counted
                // towards neither exit rule. Treating it as a violation would
                // fail builds over a vocabulary change, and as a warning would
                // hide one.
                _ => {}
            }
            let change = if before == 0 {
                Change::New
            } else {
                Change::Increased
            };
            rows.push(DiffRow {
                change,
                key: key.clone(),
                baseline: before,
                now,
            });
        }

        // Resolved findings are informational and are listed last, so the rows
        // that can fail a build read first.
        for (key, &before) in &recorded {
            let now = current.get(key).copied().unwrap_or_default();
            if now < before {
                rows.push(DiffRow {
                    change: Change::Resolved,
                    key: key.clone(),
                    baseline: before,
                    now,
                });
            }
        }

        BaselineDiff {
            source: source.to_path_buf(),
            recorded,
            coverage: self.coverage(),
            rows,
            new_violations,
            new_warnings,
        }
    }

    /// This record's findings as `(class, code, severity) → count`.
    fn tally(&self) -> BTreeMap<FindingKey, usize> {
        let mut counts = BTreeMap::new();
        for entry in &self.classes {
            for finding in entry.violations.iter().chain(&entry.warnings) {
                let key = FindingKey::new(&entry.class, &finding.code, &finding.severity);
                *counts.entry(key).or_default() += 1;
            }
        }

        counts
    }

    /// This record's per-class coverage tokens.
    fn coverage(&self) -> BTreeMap<String, String> {
        self.classes
            .iter()
            .map(|entry| (entry.class.clone(), entry.content.clone()))
            .collect()
    }
}

/// A report's findings as `(class, code, severity) → count`.
///
/// Keyed on the class the report **filed** each finding under — the entry it
/// iterates out of — which is how [`ReportSnapshot::tally`] keys its side too.
fn tally_report(report: &ConformanceReport) -> BTreeMap<FindingKey, usize> {
    let mut counts = BTreeMap::new();
    for (class, findings) in report.classes() {
        for violation in &findings.violations {
            let key = FindingKey::new(class.as_str(), violation.code(), VIOLATION);
            *counts.entry(key).or_default() += 1;
        }
        for warning in &findings.warnings {
            let key = FindingKey::new(class.as_str(), warning.code(), WARNING);
            *counts.entry(key).or_default() += 1;
        }
    }

    counts
}

/// The house shape of every baseline diagnostic: the document, then what is
/// wrong with it, then what the flag expected. Naming the file matters when a
/// CI job runs several.
fn rejected(path: &Path, detail: fmt::Arguments<'_>) -> UsageError {
    UsageError::new(format!(
        "the baseline {} {detail}. A baseline is a report minted by \
         `cdb-lint --format json -o <path>` and read back to decide which findings are new",
        path.display()
    ))
}

/// What identifies a finding across two runs: the class it was filed under,
/// its stable code, and its severity.
///
/// Message text is **not** here, and the module docs say why. `Ord` is derived
/// so a diff's rows come out in one order whatever the reports did, which is
/// what lets two runs over the same bytes produce the same diff.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FindingKey {
    /// The requirements class the finding was filed under.
    pub class: String,
    /// The finding's stable requirement-URI code.
    pub code: String,
    /// [`VIOLATION`] or [`WARNING`].
    pub severity: String,
}

impl FindingKey {
    /// The key of a finding of `severity`, filed under `class`, citing `code`.
    pub fn new(class: &str, code: &str, severity: &str) -> Self {
        Self {
            class: class.to_owned(),
            code: code.to_owned(),
            severity: severity.to_owned(),
        }
    }
}

/// How one triple's count moved between the baseline and this run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// The baseline records nothing for this triple.
    New,
    /// The baseline records fewer of this triple than this run found.
    Increased,
    /// This run found fewer than the baseline records — informational, and
    /// never a reason to fail a build.
    Resolved,
}

impl Change {
    /// The label this change prints under in a rendered diff.
    pub fn label(self) -> &'static str {
        match self {
            Change::New => "new",
            Change::Increased => "increased",
            Change::Resolved => "resolved",
        }
    }
}

/// One line of a diff: what moved, for which triple, and by how much.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffRow {
    /// Which direction the count moved.
    pub change: Change,
    /// The triple whose count moved.
    pub key: FindingKey,
    /// How many the baseline records.
    pub baseline: usize,
    /// How many this run found.
    pub now: usize,
}

/// What a baseline says, beside what this run found.
///
/// It carries the whole of the baseline's tally rather than only the rows that
/// moved, because [`crate::render::sarif`] needs the *unchanged* counts too:
/// for a triple the baseline records `n` times, the first `n` results in report
/// order are `unchanged` and the rest are `new`.
#[derive(Debug, Clone)]
pub struct BaselineDiff {
    source: PathBuf,
    recorded: BTreeMap<FindingKey, usize>,
    coverage: BTreeMap<String, String>,
    rows: Vec<DiffRow>,
    new_violations: usize,
    new_warnings: usize,
}

impl BaselineDiff {
    /// The file the baseline was read from.
    pub fn source(&self) -> &Path {
        &self.source
    }

    /// Every triple whose count moved: the new and increased ones first, in key
    /// order, then the resolved ones.
    pub fn rows(&self) -> &[DiffRow] {
        &self.rows
    }

    /// How many violation occurrences this run has that the baseline does not.
    /// Non-zero is the one condition that fails a build on its own.
    pub fn new_violations(&self) -> usize {
        self.new_violations
    }

    /// How many warning occurrences this run has that the baseline does not.
    /// Always reported; fails the build only under `--deny-warnings`.
    pub fn new_warnings(&self) -> usize {
        self.new_warnings
    }

    /// Whether this run found nothing the baseline had not already recorded.
    /// Resolved findings do not count: a datastore that only got better has
    /// nothing new.
    pub fn nothing_new(&self) -> bool {
        self.new_violations == 0 && self.new_warnings == 0
    }

    /// How many of `key` the baseline records.
    pub fn recorded(&self, key: &FindingKey) -> usize {
        self.recorded.get(key).copied().unwrap_or_default()
    }

    /// The coverage token the baseline records for `class`, or `None` for a
    /// class it never listed.
    pub fn coverage_of(&self, class: &str) -> Option<&str> {
        self.coverage.get(class).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A snapshot from a JSON document, for the arms a real datastore cannot
    /// conveniently reach. `tests/cli_baseline.rs` drives the whole flow over
    /// real datastores and real minted baselines; these pin the arithmetic.
    fn snapshot(document: &str) -> ReportSnapshot {
        match serde_json::from_str(document) {
            Ok(snapshot) => snapshot,
            Err(error) => panic!("the document should parse: {error}\n{document}"),
        }
    }

    /// A one-class record with `violations` and `warnings` spelled out.
    fn record(class: &str, violations: &[&str], warnings: &[&str]) -> String {
        let finding = |code: &str, severity: &str| {
            format!(
                r#"{{"code": "{code}", "class": "{class}", "severity": "{severity}",
                      "message": "whatever the library said that day"}}"#
            )
        };
        let list = |codes: &[&str], severity: &str| {
            codes
                .iter()
                .map(|code| finding(code, severity))
                .collect::<Vec<_>>()
                .join(",")
        };

        format!(
            r#"{{"profile": "simulation", "root": "/cdb", "conformant": false,
                 "classes": [{{"class": "{class}", "requirements_uri": "/req/core/x",
                               "passed": false, "content": "checked", "has_content": true,
                               "violations": [{}], "warnings": [{}]}}]}}"#,
            list(violations, VIOLATION),
            list(warnings, WARNING)
        )
    }

    /// The key of the one triple `class`/`code`/`severity` names.
    fn key(class: &str, code: &str, severity: &str) -> FindingKey {
        FindingKey::new(class, code, severity)
    }

    /// The identity is the triple and nothing else. Two findings that differ
    /// only in their message are one key, which is what keeps a baseline from
    /// failing the day the library improves a sentence.
    #[test]
    fn cli_snapshot_identity_is_the_triple_alone() {
        let one = snapshot(&record("file-naming", &["/req/core/name-spaces"], &[]));
        let other = snapshot(
            &record("file-naming", &["/req/core/name-spaces"], &[]).replace(
                "whatever the library said that day",
                "something else entirely",
            ),
        );

        assert_eq!(one.tally(), other.tally());
        assert_eq!(
            one.tally()
                .get(&key("file-naming", "/req/core/name-spaces", VIOLATION)),
            Some(&1)
        );
    }

    /// Severity is in the key: the same code on the other side of the
    /// SHALL/SHOULD line is a different finding, not the same one.
    #[test]
    fn cli_snapshot_severity_splits_a_key() {
        let baseline = snapshot(&record("tiling", &[], &["/req/core/tiling-scheme"]));
        let counts = baseline.tally();

        assert_eq!(counts.len(), 1);
        assert_eq!(
            counts.get(&key("tiling", "/req/core/tiling-scheme", WARNING)),
            Some(&1)
        );
        assert_eq!(
            counts.get(&key("tiling", "/req/core/tiling-scheme", VIOLATION)),
            None
        );
    }

    /// The count is a count: two occurrences of one code are two.
    #[test]
    fn cli_snapshot_counts_repeated_codes() {
        let baseline = snapshot(&record(
            "file-naming",
            &["/req/core/name-spaces", "/req/core/name-spaces"],
            &[],
        ));

        assert_eq!(
            baseline
                .tally()
                .get(&key("file-naming", "/req/core/name-spaces", VIOLATION)),
            Some(&2)
        );
    }

    /// A baseline from another profile is refused, and the diagnostic names
    /// both profiles so the reader can see which way round the mistake is.
    #[test]
    fn cli_snapshot_refuses_another_profile() {
        let baseline = snapshot(&record("crs", &[], &[]));
        let path = Path::new("/tmp/base.json");

        assert!(baseline.require_profile("simulation", path).is_ok());

        let error = match baseline.require_profile("gnosis", path) {
            Err(error) => error,
            Ok(()) => panic!("a foreign profile must be refused"),
        };
        assert!(error.message.contains("simulation"), "{}", error.message);
        assert!(error.message.contains("gnosis"), "{}", error.message);
    }

    /// The root is recorded and never judged on: CI paths move, and a
    /// datastore's identity is not its path.
    #[test]
    fn cli_snapshot_ignores_the_root() {
        let baseline = snapshot(&record("crs", &[], &[]));

        assert_eq!(baseline.root, "/cdb");
        assert!(
            baseline
                .require_profile("simulation", Path::new("/tmp/base.json"))
                .is_ok()
        );
    }

    /// An unknown field is tolerated: a baseline minted by a newer build still
    /// reads. The strict shape assertion lives in `tests/cli_baseline.rs`,
    /// over a real serialized report.
    #[test]
    fn cli_snapshot_tolerates_unknown_fields() {
        let document = record("crs", &[], &[]).replace(
            r#""profile": "simulation""#,
            r#""profile": "simulation", "from_the_future": {"nested": [1, 2, 3]}"#,
        );

        assert_eq!(snapshot(&document).profile, "simulation");
    }

    /// A document that is not a report is refused, and the message says what
    /// one is.
    #[test]
    fn cli_snapshot_refuses_a_document_that_is_not_a_report() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let path = tmp.path().join("baseline.json");
        std::fs::write(&path, r#"{"hello": "world"}"#).expect("the document");

        let error = match ReportSnapshot::load(&path) {
            Err(error) => error,
            Ok(_) => panic!("a non-report must be refused"),
        };

        assert!(error.message.contains("baseline.json"), "{}", error.message);
        assert!(error.message.contains("--format json"), "{}", error.message);
    }

    /// A baseline that is not there is refused before anything is judged.
    #[test]
    fn cli_snapshot_refuses_a_missing_file() {
        let tmp = tempfile::tempdir().expect("a temporary directory");

        let error = match ReportSnapshot::load(&tmp.path().join("nope.json")) {
            Err(error) => error,
            Ok(_) => panic!("a missing baseline must be refused"),
        };

        assert!(error.message.contains("nope.json"), "{}", error.message);
        assert!(
            error.message.contains("cannot be read"),
            "{}",
            error.message
        );
    }

    /// A severity token this build has never heard of reads back as its own
    /// key rather than being folded into one of the two it knows.
    ///
    /// The baseline is the only side that can carry one — this run's keys are
    /// built from [`VIOLATION`] and [`WARNING`] here — so such a finding can
    /// only ever appear as *resolved*, which is the harmless direction. Folding
    /// it into `violation` would fail builds over a vocabulary change, and into
    /// `warning` would hide one.
    #[test]
    fn cli_snapshot_an_unknown_severity_keeps_its_own_key() {
        let baseline = snapshot(&record("crs", &[], &[]).replace(
            r#""violations": []"#,
            r#""violations": [{"code": "/req/core/x", "class": "crs",
                               "severity": "advisory", "message": "m"}]"#,
        ));

        let counts = baseline.tally();

        assert_eq!(counts.get(&key("crs", "/req/core/x", "advisory")), Some(&1));
        assert_eq!(counts.get(&key("crs", "/req/core/x", VIOLATION)), None);
        assert_eq!(counts.get(&key("crs", "/req/core/x", WARNING)), None);
    }
}
