//! The human report of design §6.1: a header, one row per requirements
//! class, the findings beneath the rows that carry them, an aggregate tally,
//! and a verdict.
//!
//! ```text
//! cdb-lint 0.1.0 (rusty_cdb 1.0.0)
//! datastore  /Users/x/cdb
//! profile    simulation (json)
//!
//!   [PASS]      attribution
//!   [FAIL]      file-naming
//!       violation  /req/core/name-spaces
//!                  whitespace in name "My Tiles" (violates /req/core/name-spaces)
//!   [UNCHECKED] geometry           content present, no datastore-level check
//!   [N/A]       versioning         no content this class governs
//!
//! 11 classes: 8 checked, 1 no content, 2 not checked · 1 violation · 0 warnings
//! NON-CONFORMANT
//! ```
//!
//! # Status and coverage are orthogonal
//!
//! Four tokens, and they answer two different questions. *Did this class
//! record a violation?* decides `FAIL`. *How much of its content was
//! actually judged?* decides everything else — and it keeps being asked on a
//! failing row, because the content sweep files its findings against classes
//! whose coverage is `unchecked` (`docs/CONFORMANCE.md` §6). A renderer that
//! dropped the note on a `FAIL` row would report a datastore as *more*
//! checked the less its profile declared.
//!
//! So: the token is `FAIL` whenever violations exist, and otherwise follows
//! the coverage; the note prints whenever the coverage is anything but
//! `checked`, `FAIL` rows included.
//!
//! `UNCHECKED` is yellow and never green, in any theme, under any flag —
//! honesty rule 1, and [`super`]'s note on why.
//!
//! # The messages are the library's
//!
//! A finding prints its stable [`code`] beside the crate's own message,
//! reproduced verbatim. cdb-lint neither rewrites nor re-cases nor wraps
//! them: the code is the durable identity a reader takes to `cdb-lint
//! explain`, and the message is the library's to word.
//!
//! # Under `--baseline`, the diff is part of the artifact
//!
//! A ratcheted run prints one more section — what is new, what grew, and
//! (informationally) what went away — between the class rows and the tally:
//!
//! ```text
//! baseline   /ci/cdb-baseline.json
//!   new        violation  file-naming     /req/core/name-spaces
//!   increased  warning    file-structure  /req/core/name-empty-folders-A  (baseline 1, now 2)
//! ```
//!
//! **The rest of the report is unchanged.** A baseline moves the exit code and
//! nothing else: the rows still say what they said, the verdict still reads
//! `NON-CONFORMANT` over a datastore that does not conform, and the verdict
//! line names the flag responsible for an exit code that no longer follows
//! from it (design §5 rule 4). That containment is the whole reason a ratchet
//! is safe to ship.
//!
//! [`code`]: rusty_cdb::conformance::CdbViolation::code

use std::io::{self, Write};

use rusty_cdb::conformance::{
    ClassFindings, ConformanceReport, ContentCoverage, RequirementsClass,
};
use rusty_cdb::metadata::MetadataEncoding;

use crate::render::{Tally, plural};
use crate::snapshot::{BaselineDiff, Change, DiffRow};

/// SGR green, for `PASS`.
const GREEN: &str = "\u{1b}[32m";
/// SGR red, for `FAIL`.
const RED: &str = "\u{1b}[31m";
/// SGR yellow, for `UNCHECKED` — never green, whatever the flags say.
const YELLOW: &str = "\u{1b}[33m";
/// SGR dim, for `N/A`.
const DIM: &str = "\u{1b}[2m";
/// SGR reset, closing every painted span.
const RESET: &str = "\u{1b}[0m";

/// Visible width of the widest bracketed status token, `[UNCHECKED]`. Rows
/// are padded to it so the class column lines up whether or not the tokens
/// carry invisible colour bytes.
const STATUS_WIDTH: usize = 11;

/// Width of the class-name column, which puts every coverage note in one
/// place.
const CLASS_WIDTH: usize = 19;

/// Column the finding text starts in: six spaces of indent, a nine-column
/// severity label, and two spaces.
const FINDING_INDENT: usize = 17;

/// Width of a diff row's change column, measured off the longest label,
/// `increased`.
const CHANGE_WIDTH: usize = 11;

/// Width of a diff row's severity column, measured off `violation`.
const SEVERITY_WIDTH: usize = 11;

/// Width of a diff row's class column, measured off `file-structure`. Narrower
/// than [`CLASS_WIDTH`] because a diff row has no coverage note to line up,
/// and a code is easier to read closer to the class that filed it.
const DIFF_CLASS_WIDTH: usize = 16;

/// How the text report is rendered.
///
/// Everything ambient is decided before this struct is built — `--color
/// auto` has already asked the terminal, `NO_COLOR` has already had its say —
/// so rendering is a pure function of a report and these four values, and a
/// test can pin the output without owning a terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextOptions {
    /// The metadata encoding the profile declares, named in the header
    /// beside the profile so a reader can see the whole yardstick. It is
    /// *declared*, never detected: cdb-lint may read a datastore to suggest a
    /// flag and never to choose one (design §5 rule 5).
    pub encoding: MetadataEncoding,
    /// Whether to emit ANSI colour.
    pub color: bool,
    /// Whether to omit class rows that have nothing to report.
    pub quiet: bool,
    /// Whether `--deny-warnings` is in force. It changes neither the report
    /// nor the verdict — only whether the verdict line has to explain an exit
    /// code that no longer follows from it (design §5 rule 4).
    pub deny_warnings: bool,
}

/// Writes `report` to `out` in the layout above.
///
/// Class order is the report's own, which is `RequirementsClass`'s `Ord` —
/// and the report's order is a function of the datastore rather than of the
/// host filesystem (`docs/CONFORMANCE.md`, amendment 21). This renderer
/// inherits that determinism by leaving it alone, so two runs over the same
/// bytes produce the same text and a diff of two reports means something.
///
/// `baseline` is the comparison a `--baseline` run made, or `None` for a run
/// that compared against nothing. It adds a section and can change the verdict
/// *line*; it never changes the verdict, the rows, or the findings.
///
/// # Errors
///
/// Returns the sink's own error. A report that could not be written is a fact
/// about the run, and the caller answers it with [`crate::exit::OPERATIONAL`]
/// rather than with a verdict it did not deliver.
pub fn render(
    report: &ConformanceReport,
    options: &TextOptions,
    baseline: Option<&BaselineDiff>,
    out: &mut dyn Write,
) -> io::Result<()> {
    writeln!(
        out,
        "cdb-lint {} (rusty_cdb {})",
        env!("CARGO_PKG_VERSION"),
        crate::RUSTY_CDB_VERSION
    )?;
    writeln!(out, "datastore  {}", report.root().display())?;
    writeln!(
        out,
        "profile    {} ({})",
        report.profile(),
        options.encoding.as_str()
    )?;

    let mut tally = Tally::default();
    let mut rows = 0usize;
    for (class, findings) in report.classes() {
        tally.add(findings);
        if options.quiet && findings.violations.is_empty() && findings.warnings.is_empty() {
            continue;
        }
        if rows == 0 {
            writeln!(out)?;
        }
        rows += 1;
        write_row(out, class, findings, options.color)?;
        for violation in &findings.violations {
            write_finding(out, "violation", violation.code(), violation)?;
        }
        for warning in &findings.warnings {
            write_finding(out, "warning", warning.code(), warning)?;
        }
    }

    if let Some(diff) = baseline {
        writeln!(out)?;
        write_baseline_section(diff, out)?;
    }

    writeln!(out)?;
    writeln!(out, "{}", tally.line())?;
    write_verdict(
        out,
        report.is_conformant(),
        &tally,
        options.deny_warnings,
        baseline,
    )
}

/// Writes the baseline diff: the file compared against, then one row per
/// `(class, code, severity)` whose count moved.
///
/// Public because the machine formats owe their reader the same section
/// without being allowed to carry it: the JSON artifact is the library's
/// frozen wire shape and cannot gain a field, and SARIF can say `new` on a
/// result but has no way to mention a finding that no longer exists — design
/// §8 forbids synthesizing `absent` results for those. Both therefore print
/// this section to **stderr**, where it reaches a human without entering a
/// machine's document, exactly as the coverage tally does (design §6.2).
///
/// # Errors
///
/// Returns the sink's own error.
pub fn write_baseline_section(diff: &BaselineDiff, out: &mut dyn Write) -> io::Result<()> {
    writeln!(out, "baseline   {}", diff.source().display())?;

    // Stated even when nothing moved, because "nothing is new" is the single
    // most consequential thing a ratcheted run can say: it is the sentence
    // that turns a non-conformant datastore into exit 0.
    if diff.nothing_new() {
        writeln!(out, "  no new findings since baseline")?;
    }

    for row in diff.rows() {
        write_diff_row(out, row)?;
    }

    if diff.rows().iter().any(|row| row.change == Change::Resolved) {
        writeln!(
            out,
            "  (resolved findings are informational: a build never fails because it got better)"
        )?;
    }

    Ok(())
}

/// One diff row: what moved, at what severity, for which class and code.
///
/// The counts are appended except in the one case where they say nothing a
/// reader cannot see — a finding that is wholly new and occurred once.
fn write_diff_row(out: &mut dyn Write, row: &DiffRow) -> io::Result<()> {
    let counts = if row.baseline == 0 && row.now == 1 {
        String::new()
    } else {
        format!("  (baseline {}, now {})", row.baseline, row.now)
    };

    writeln!(
        out,
        "  {:CHANGE_WIDTH$}{:SEVERITY_WIDTH$}{:DIFF_CLASS_WIDTH$}{}{counts}",
        row.change.label(),
        row.key.severity,
        row.key.class,
        row.key.code
    )
}

/// One class row: the status token, the class, and — when there is one — the
/// coverage note.
fn write_row(
    out: &mut dyn Write,
    class: RequirementsClass,
    findings: &ClassFindings,
    color: bool,
) -> io::Result<()> {
    let status = Status::of(!findings.violations.is_empty(), findings.coverage);
    let bracketed = format!("[{}]", status.token());
    let padding = STATUS_WIDTH.saturating_sub(bracketed.chars().count());
    let painted = if color {
        format!("{}{bracketed}{RESET}", status.colour())
    } else {
        bracketed
    };
    match note(findings.coverage) {
        // The note column is fixed, so the notes line up under each other
        // even when the class names do not.
        Some(note) => writeln!(
            out,
            "  {painted}{:padding$} {:CLASS_WIDTH$}{note}",
            "",
            class.as_str()
        ),
        None => writeln!(out, "  {painted}{:padding$} {}", "", class.as_str()),
    }
}

/// One finding: its stable code, then the library's own message verbatim on
/// the next line, indented to the same column.
fn write_finding(
    out: &mut dyn Write,
    label: &str,
    code: &str,
    message: &dyn std::fmt::Display,
) -> io::Result<()> {
    writeln!(out, "      {label:<9}  {code}")?;
    writeln!(out, "{:FINDING_INDENT$}{message}", "")
}

/// A row's status: the four tokens of design §6.1, as a closed enum.
///
/// A type rather than a `&str` so that [`Self::colour`] is an exhaustive
/// match over four named variants. `UNCHECKED` is then structurally unable to
/// fall through to some other arm's colour — honesty rule 1 becomes a
/// property the compiler checks, instead of a string comparison that a fifth
/// token could silently route to the wildcard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    /// Content was present and this crate's check ran over it, cleanly.
    Pass,
    /// The class recorded at least one violation.
    Fail,
    /// Content was present that this crate has no datastore-level check for.
    Unchecked,
    /// The datastore held nothing this class governs.
    NotApplicable,
}

impl Status {
    /// The status of a class, from the two questions design §6.1 keeps
    /// apart: a class that recorded a violation is [`Self::Fail`]
    /// **whatever** its coverage, and only a class with nothing against it
    /// lets the coverage speak. Taking the coverage as a second argument
    /// rather than deriving one from the other is what makes that
    /// orthogonality a property of the signature.
    fn of(has_violations: bool, coverage: ContentCoverage) -> Self {
        if has_violations {
            return Status::Fail;
        }
        match coverage {
            ContentCoverage::Checked => Status::Pass,
            ContentCoverage::NoContent => Status::NotApplicable,
            ContentCoverage::Unchecked => Status::Unchecked,
        }
    }

    /// The token printed in brackets.
    fn token(self) -> &'static str {
        match self {
            Status::Pass => "PASS",
            Status::Fail => "FAIL",
            Status::Unchecked => "UNCHECKED",
            Status::NotApplicable => "N/A",
        }
    }

    /// The colour the token is painted in.
    fn colour(self) -> &'static str {
        match self {
            Status::Pass => GREEN,
            Status::Fail => RED,
            Status::Unchecked => YELLOW,
            Status::NotApplicable => DIM,
        }
    }
}

/// The coverage note, or `None` when the content was checked and there is
/// nothing to qualify.
fn note(coverage: ContentCoverage) -> Option<&'static str> {
    match coverage {
        ContentCoverage::Checked => None,
        ContentCoverage::NoContent => Some("no content this class governs"),
        ContentCoverage::Unchecked => Some("content present, no datastore-level check"),
    }
}

/// The verdict line.
///
/// Conformance is decided by violations alone, so neither flag can change the
/// verdict — only divorce the exit code from it, and in opposite directions.
/// When one does, the line says which: a build failing over a datastore the
/// report calls conformant is not a contradiction, but it is a surprise, and
/// so is a build passing over one it calls non-conformant. Both surprises are
/// owed an explanation on the same line (design §5 rule 4).
fn write_verdict(
    out: &mut dyn Write,
    conformant: bool,
    tally: &Tally,
    deny_warnings: bool,
    baseline: Option<&BaselineDiff>,
) -> io::Result<()> {
    if let Some(diff) = baseline {
        return writeln!(
            out,
            "{}",
            ratcheted_verdict(
                conformant,
                tally,
                deny_warnings,
                diff.new_violations(),
                diff.new_warnings(),
            )
        );
    }
    if !conformant {
        return writeln!(out, "NON-CONFORMANT");
    }
    if deny_warnings && tally.warnings > 0 {
        return writeln!(
            out,
            "CONFORMANT ({} {}) — exit 1 by --deny-warnings",
            tally.warnings,
            plural(tally.warnings, "warning", "warnings")
        );
    }
    writeln!(out, "CONFORMANT")
}

/// The verdict line of a `--baseline` run: the verdict, what is new since the
/// baseline, and — when a flag moved the exit code — which one.
///
/// A pure function of the five facts it is handed — the diff enters as its two
/// counts rather than as a whole [`BaselineDiff`] — so the sentence a reader
/// will quote back at us is pinned by unit tests rather than reconstructed from
/// a rendered report.
///
/// The head always carries a count under `--baseline`, because the number the
/// ratchet is absorbing is the number that matters: `NON-CONFORMANT` alone,
/// beside exit 0, would be true and useless.
fn ratcheted_verdict(
    conformant: bool,
    tally: &Tally,
    deny_warnings: bool,
    new_violations: usize,
    new_warnings: usize,
) -> String {
    let head = if !conformant {
        format!(
            "NON-CONFORMANT ({} {})",
            tally.violations,
            plural(tally.violations, "violation", "violations")
        )
    } else if tally.warnings > 0 {
        format!(
            "CONFORMANT ({} {})",
            tally.warnings,
            plural(tally.warnings, "warning", "warnings")
        )
    } else {
        "CONFORMANT".to_owned()
    };

    // What the run exits with, and what it would have exited with had no
    // baseline been given. Naming a flag is warranted exactly when those two
    // differ, which is what "the flag moved the exit code" means.
    let failing = new_violations > 0 || (deny_warnings && new_warnings > 0);
    let would_fail_unratcheted = !conformant || (deny_warnings && tally.warnings > 0);

    let attribution = if !failing && would_fail_unratcheted {
        "; exit 0 by --baseline"
    } else if failing && new_violations == 0 {
        // Nothing new failed on its own; the flag asked for this one.
        "; exit 1 by --deny-warnings"
    } else {
        // The exit code follows the verdict, so no flag is responsible for it
        // and naming one would be an invented explanation.
        ""
    };

    format!(
        "{head} — {} since baseline{attribution}",
        new_summary(new_violations, new_warnings)
    )
}

/// What is new since the baseline, as a phrase.
///
/// Violations and warnings are counted apart, because they fail a build under
/// different conditions and a single total would hide which kind arrived.
fn new_summary(violations: usize, warnings: usize) -> String {
    let phrase = |count: usize, singular: &'static str, many: &'static str| {
        format!("{count} new {}", plural(count, singular, many))
    };

    match (violations, warnings) {
        (0, 0) => "no new findings".to_owned(),
        (_, 0) => phrase(violations, "violation", "violations"),
        (0, _) => phrase(warnings, "warning", "warnings"),
        _ => format!(
            "{}, {}",
            phrase(violations, "violation", "violations"),
            phrase(warnings, "warning", "warnings")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four tokens are a published vocabulary — a reader greps for them
    /// and a script keys on them — and the two questions behind them are
    /// independent. A class carrying a violation is `FAIL` in all three
    /// coverage states, and a class carrying none is never `FAIL` in any of
    /// them.
    #[test]
    fn req_cdb_lint_status_and_coverage_are_orthogonal() {
        for coverage in [
            ContentCoverage::Checked,
            ContentCoverage::NoContent,
            ContentCoverage::Unchecked,
        ] {
            assert_eq!(Status::of(true, coverage).token(), "FAIL", "{coverage:?}");
        }

        assert_eq!(Status::of(false, ContentCoverage::Checked).token(), "PASS");
        assert_eq!(Status::of(false, ContentCoverage::NoContent).token(), "N/A");
        assert_eq!(
            Status::of(false, ContentCoverage::Unchecked).token(),
            "UNCHECKED"
        );
    }

    /// `UNCHECKED` is yellow. The assertion is written as an inequality
    /// against green as well, because "not green" is the promise honesty rule
    /// 1 actually makes and a later theme could keep the first half while
    /// breaking the second.
    #[test]
    fn req_cdb_lint_unchecked_is_yellow_and_never_green() {
        assert_eq!(Status::Unchecked.colour(), YELLOW);
        assert_ne!(Status::Unchecked.colour(), GREEN);
        assert_eq!(Status::Pass.colour(), GREEN);
        assert_eq!(Status::Fail.colour(), RED);
        assert_eq!(Status::NotApplicable.colour(), DIM);
    }

    /// Every coverage state that is not `checked` earns a note, and the
    /// wording is the report's contract with a reader who never opens
    /// `docs/CONFORMANCE.md`.
    #[test]
    fn req_cdb_lint_every_qualified_coverage_state_prints_a_note() {
        assert_eq!(note(ContentCoverage::Checked), None);
        assert_eq!(
            note(ContentCoverage::NoContent),
            Some("no content this class governs")
        );
        assert_eq!(
            note(ContentCoverage::Unchecked),
            Some("content present, no datastore-level check")
        );
    }

    /// Zero is plural, one is singular, and more than one is plural. The
    /// tally reads as prose or it does not get read.
    #[test]
    fn cli_text_counts_agree_with_their_nouns() {
        assert_eq!(plural(0, "violation", "violations"), "violations");
        assert_eq!(plural(1, "violation", "violations"), "violation");
        assert_eq!(plural(2, "violation", "violations"), "violations");
    }

    /// A tally with `violations` violations and `warnings` warnings; the
    /// coverage counts play no part in a verdict line.
    fn tally(violations: usize, warnings: usize) -> Tally {
        Tally {
            classes: 11,
            checked: 11,
            no_content: 0,
            unchecked: 0,
            violations,
            warnings,
        }
    }

    /// **Honesty rule 4, as one table.** Both flags can take the exit code
    /// away from the verdict, in opposite directions, and the line names
    /// whichever one did it — never both, never neither, and never a flag that
    /// changed nothing.
    ///
    /// The ratchet row is the dangerous one: exit 0 over a datastore the same
    /// line calls `NON-CONFORMANT`. It is spelled out here, character for
    /// character, because it is the sentence that stops an exit code from
    /// being the only thing that speaks.
    #[test]
    fn req_cdb_lint_the_verdict_line_names_the_flag_that_moved_the_exit_code() {
        // conformant, tally, deny-warnings, new violations, new warnings
        let cases = [
            (
                (false, tally(3, 0), false, 0, 0),
                "NON-CONFORMANT (3 violations) — no new findings since baseline; \
                 exit 0 by --baseline",
            ),
            (
                (false, tally(4, 0), false, 1, 0),
                "NON-CONFORMANT (4 violations) — 1 new violation since baseline",
            ),
            (
                (false, tally(3, 1), false, 0, 1),
                "NON-CONFORMANT (3 violations) — 1 new warning since baseline; \
                 exit 0 by --baseline",
            ),
            (
                (false, tally(3, 1), true, 0, 1),
                "NON-CONFORMANT (3 violations) — 1 new warning since baseline; \
                 exit 1 by --deny-warnings",
            ),
            (
                (true, tally(0, 0), false, 0, 0),
                "CONFORMANT — no new findings since baseline",
            ),
            (
                (true, tally(0, 2), true, 0, 0),
                "CONFORMANT (2 warnings) — no new findings since baseline; exit 0 by --baseline",
            ),
            (
                (true, tally(0, 2), true, 0, 1),
                "CONFORMANT (2 warnings) — 1 new warning since baseline; \
                 exit 1 by --deny-warnings",
            ),
            (
                (true, tally(0, 2), false, 0, 1),
                "CONFORMANT (2 warnings) — 1 new warning since baseline",
            ),
        ];

        for ((conformant, tally, deny_warnings, violations, warnings), expected) in cases {
            assert_eq!(
                ratcheted_verdict(conformant, &tally, deny_warnings, violations, warnings),
                expected
            );
        }
    }

    /// Without a baseline the line is the one it always was: a flag that was
    /// never given cannot be blamed for an exit code.
    #[test]
    fn cli_text_an_unratcheted_verdict_is_unchanged() {
        let rendered = |conformant: bool, warnings: usize, deny: bool| {
            let mut out = Vec::new();
            write_verdict(&mut out, conformant, &tally(0, warnings), deny, None).unwrap();
            String::from_utf8(out).unwrap()
        };

        assert_eq!(rendered(true, 0, false), "CONFORMANT\n");
        assert_eq!(rendered(true, 1, false), "CONFORMANT\n");
        assert_eq!(rendered(false, 0, false), "NON-CONFORMANT\n");
        assert_eq!(
            rendered(true, 1, true),
            "CONFORMANT (1 warning) — exit 1 by --deny-warnings\n"
        );
    }

    /// Violations and warnings are counted apart, because they fail a build
    /// under different conditions.
    #[test]
    fn cli_text_the_new_summary_counts_the_two_severities_apart() {
        assert_eq!(new_summary(0, 0), "no new findings");
        assert_eq!(new_summary(1, 0), "1 new violation");
        assert_eq!(new_summary(0, 3), "3 new warnings");
        assert_eq!(new_summary(2, 1), "2 new violations, 1 new warning");
    }

    /// A diff row's counts are appended except in the one case where they say
    /// nothing a reader cannot already see.
    #[test]
    fn cli_text_a_diff_row_states_its_counts_unless_they_are_obvious() {
        let row = |change: Change, baseline: usize, now: usize| {
            let row = DiffRow {
                change,
                key: crate::snapshot::FindingKey::new(
                    "file-naming",
                    "/req/core/name-spaces",
                    crate::snapshot::VIOLATION,
                ),
                baseline,
                now,
            };
            let mut out = Vec::new();
            write_diff_row(&mut out, &row).unwrap();
            String::from_utf8(out).unwrap()
        };

        assert_eq!(
            row(Change::New, 0, 1),
            "  new        violation  file-naming     /req/core/name-spaces\n"
        );
        assert_eq!(
            row(Change::New, 0, 3),
            "  new        violation  file-naming     /req/core/name-spaces  (baseline 0, now 3)\n"
        );
        assert_eq!(
            row(Change::Increased, 1, 2),
            "  increased  violation  file-naming     /req/core/name-spaces  (baseline 1, now 2)\n"
        );
        assert_eq!(
            row(Change::Resolved, 2, 0),
            "  resolved   violation  file-naming     /req/core/name-spaces  (baseline 2, now 0)\n"
        );
    }
}
