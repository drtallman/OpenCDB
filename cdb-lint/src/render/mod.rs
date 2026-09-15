//! Turning a [`ConformanceReport`] into an artifact somebody can read.
//!
//! One module per `--format`. Every one of them owes design §5's honesty
//! rules the same thing in its own vocabulary: the three
//! [`ContentCoverage`] states must survive the translation, because a format
//! that flattens them reports "we did not look" as "we looked and it was
//! fine" — the one lie the third state exists to prevent.
//!
//! # The tally lives here, not in a format
//!
//! Honesty rule 2 asks every run to state how much was actually checked, and
//! the formats discharge it differently: the text report prints the tally as
//! its penultimate line, the SARIF document carries the same three counts in
//! `run.properties`, and the JSON artifact — which cannot gain a field without
//! ceasing to be the frozen wire shape — puts the line on stderr instead. All
//! of them count through the same `Tally`. Independent counters would be
//! independent chances to disagree about a number whose whole purpose is to be
//! trusted.
//!
//! [`ConformanceReport`]: opencdb::conformance::ConformanceReport
//! [`ContentCoverage`]: opencdb::conformance::ContentCoverage

pub mod json;
pub mod sarif;
pub mod text;

use opencdb::conformance::{ClassFindings, ConformanceReport, ContentCoverage};

/// The running counts behind the aggregate line.
///
/// The line prints on every run, `--quiet` included, because it is the one
/// place a reader who sees nothing else still learns how much was actually
/// checked (design §5 rule 2). Its three coverage counts always sum to the
/// class count.
#[derive(Debug, Default)]
pub(crate) struct Tally {
    pub(crate) classes: usize,
    pub(crate) checked: usize,
    pub(crate) no_content: usize,
    pub(crate) unchecked: usize,
    pub(crate) violations: usize,
    pub(crate) warnings: usize,
}

impl Tally {
    /// Folds a whole report in.
    ///
    /// Over the classes the *report* lists rather than a fixed roster:
    /// `RequirementsClass` is `#[non_exhaustive]`, and a report lists every
    /// class it has something to say about, so asking the report is the only
    /// way the tally cannot fall behind a twelfth class.
    pub(crate) fn of(report: &ConformanceReport) -> Self {
        let mut tally = Self::default();
        for (_, findings) in report.classes() {
            tally.add(findings);
        }
        tally
    }

    /// Folds one class's findings in.
    pub(crate) fn add(&mut self, findings: &ClassFindings) {
        self.classes += 1;
        match findings.coverage {
            ContentCoverage::Checked => self.checked += 1,
            ContentCoverage::NoContent => self.no_content += 1,
            ContentCoverage::Unchecked => self.unchecked += 1,
        }
        self.violations += findings.violations.len();
        self.warnings += findings.warnings.len();
    }

    /// The aggregate line, without its newline.
    pub(crate) fn line(&self) -> String {
        format!(
            "{} {}: {} checked, {} no content, {} not checked · {} {} · {} {}",
            self.classes,
            plural(self.classes, "class", "classes"),
            self.checked,
            self.no_content,
            self.unchecked,
            self.violations,
            plural(self.violations, "violation", "violations"),
            self.warnings,
            plural(self.warnings, "warning", "warnings"),
        )
    }
}

/// `singular` for exactly one, `many` otherwise — including zero.
pub(crate) fn plural(count: usize, singular: &'static str, many: &'static str) -> &'static str {
    if count == 1 { singular } else { many }
}
