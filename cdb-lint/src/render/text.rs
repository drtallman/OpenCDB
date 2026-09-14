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
//! [`code`]: rusty_cdb::conformance::CdbViolation::code

use std::io::{self, Write};

use rusty_cdb::conformance::{
    ClassFindings, ConformanceReport, ContentCoverage, RequirementsClass,
};
use rusty_cdb::metadata::MetadataEncoding;

use crate::render::{Tally, plural};

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
/// # Errors
///
/// Returns the sink's own error. A report that could not be written is a fact
/// about the run, and the caller answers it with [`crate::exit::OPERATIONAL`]
/// rather than with a verdict it did not deliver.
pub fn render(
    report: &ConformanceReport,
    options: &TextOptions,
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

    writeln!(out)?;
    writeln!(out, "{}", tally.line())?;
    write_verdict(out, report.is_conformant(), &tally, options.deny_warnings)
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
/// Conformance is decided by violations alone, so `--deny-warnings` can only
/// divorce the exit code from the verdict, never change it. When it does, the
/// line says which flag is responsible: a build failing over a datastore the
/// report calls conformant is not a contradiction, but it is a surprise, and
/// the surprise is owed an explanation on the same line (design §5 rule 4).
fn write_verdict(
    out: &mut dyn Write,
    conformant: bool,
    tally: &Tally,
    deny_warnings: bool,
) -> io::Result<()> {
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
}
