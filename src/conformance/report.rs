//! The conformance report: findings bucketed by [`RequirementsClass`].

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::conformance::{CdbViolation, CdbWarning, RequirementsClass};
use crate::metadata::MetadataViolation;

/// The violations and warnings recorded against a single requirements class.
/// Fields are open, mirroring [`crate::hierarchy::HierarchyReport`].
#[derive(Debug, Clone, Default)]
pub struct ClassFindings {
    /// SHALL violations for this class.
    pub violations: Vec<CdbViolation>,
    /// SHOULD warnings for this class.
    pub warnings: Vec<CdbWarning>,
}

/// The outcome of validating a datastore against a profile (Annex A
/// `/conf/minimal-core`): findings bucketed by [`RequirementsClass`]. The five
/// mandatory classes are always listed, whether or not they have findings.
///
/// Conformance is decided by violations alone; warnings never affect
/// [`Self::is_conformant`] or [`Self::class_passed`].
#[derive(Debug, Clone)]
pub struct ConformanceReport {
    profile: String,
    root: PathBuf,
    classes: BTreeMap<RequirementsClass, ClassFindings>,
}

/// Crate-internal construction and recording, consumed by
/// [`crate::conformance::validate`].
impl ConformanceReport {
    /// A fresh report for `profile` at datastore `root`, pre-seeding the five
    /// mandatory classes with empty findings so they are always listed (the
    /// invariant behind `/conf/minimal-core`).
    pub(crate) fn new(profile: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        let mut classes = BTreeMap::new();
        for class in RequirementsClass::MANDATORY {
            classes.insert(class, ClassFindings::default());
        }
        Self {
            profile: profile.into(),
            root: root.into(),
            classes,
        }
    }

    /// Records a violation under its [`CdbViolation::class`]. A nested
    /// `Metadata(Link(..))` is normalized to `Link(..)` first, so every Links
    /// finding is stored under Links as a [`CdbViolation::Link`] value.
    pub(crate) fn record_violation(&mut self, violation: CdbViolation) {
        let violation = match violation {
            CdbViolation::Metadata(MetadataViolation::Link(link)) => CdbViolation::Link(link),
            other => other,
        };
        self.classes
            .entry(violation.class())
            .or_default()
            .violations
            .push(violation);
    }

    /// Records a warning under its [`CdbWarning::class`].
    pub(crate) fn record_warning(&mut self, warning: CdbWarning) {
        self.classes
            .entry(warning.class())
            .or_default()
            .warnings
            .push(warning);
    }
}

impl ConformanceReport {
    /// The name of the profile the datastore was validated against.
    pub fn profile(&self) -> &str {
        &self.profile
    }

    /// The datastore root that was validated.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Whether the datastore conforms: no violation in any class. Warnings
    /// (SHOULD findings) never affect this.
    pub fn is_conformant(&self) -> bool {
        self.classes
            .values()
            .all(|findings| findings.violations.is_empty())
    }

    /// Whether `class` has no violations. An unlisted class — an optional class
    /// that has accrued no findings — is vacuously `true`, so later phases can
    /// add optional classes without breaking existing consumers.
    pub fn class_passed(&self, class: RequirementsClass) -> bool {
        match self.classes.get(&class) {
            Some(findings) => findings.violations.is_empty(),
            None => true,
        }
    }

    /// The violations recorded for `class`; an empty slice for an unlisted
    /// class.
    pub fn violations(&self, class: RequirementsClass) -> &[CdbViolation] {
        match self.classes.get(&class) {
            Some(findings) => &findings.violations,
            None => &[],
        }
    }

    /// The warnings recorded for `class`; an empty slice for an unlisted class.
    pub fn warnings(&self, class: RequirementsClass) -> &[CdbWarning] {
        match self.classes.get(&class) {
            Some(findings) => &findings.warnings,
            None => &[],
        }
    }

    /// Every listed class and its findings, in [`RequirementsClass`] order.
    pub fn classes(&self) -> impl Iterator<Item = (RequirementsClass, &ClassFindings)> {
        self.classes
            .iter()
            .map(|(&class, findings)| (class, findings))
    }
}

impl fmt::Display for ConformanceReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Conformance report — profile {:?}, root {}",
            self.profile,
            self.root.display()
        )?;
        for (class, findings) in &self.classes {
            let status = if findings.violations.is_empty() {
                "PASS"
            } else {
                "FAIL"
            };
            writeln!(f, "  [{status}] {class}")?;
            for violation in &findings.violations {
                writeln!(f, "      violation: {violation}")?;
            }
            for warning in &findings.warnings {
                writeln!(f, "      warning: {warning}")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crs::CrsViolation;
    use crate::links::LinkViolation;
    use crate::naming::NamingViolation;

    /// Annex A: a fresh report always lists exactly the five mandatory classes,
    /// is conformant, and every class passes. A recorded violation fails only
    /// its class; a warning never affects pass/fail; a nested Metadata(Link)
    /// violation lands under Links as a `CdbViolation::Link`.
    #[test]
    fn conformance_report_lists_classes_and_pass_fail() {
        let report = ConformanceReport::new("simulation", "/tmp/cdb");
        let listed: Vec<RequirementsClass> = report.classes().map(|(class, _)| class).collect();
        assert_eq!(listed, RequirementsClass::MANDATORY.to_vec());
        assert_eq!(report.profile(), "simulation");
        assert_eq!(report.root(), Path::new("/tmp/cdb"));
        assert!(report.is_conformant());
        for class in RequirementsClass::MANDATORY {
            assert!(report.class_passed(class), "{class}");
        }

        // A naming violation fails only FileNaming.
        let mut report = ConformanceReport::new("simulation", "/tmp/cdb");
        report.record_violation(NamingViolation::EmptyName.into());
        assert!(!report.is_conformant());
        assert!(!report.class_passed(RequirementsClass::FileNaming));
        for class in [
            RequirementsClass::Crs,
            RequirementsClass::FileStructure,
            RequirementsClass::Links,
            RequirementsClass::Metadata,
        ] {
            assert!(report.class_passed(class), "{class}");
        }
        assert_eq!(report.violations(RequirementsClass::FileNaming).len(), 1);

        // A warning alone never affects pass/fail.
        let mut report = ConformanceReport::new("simulation", "/tmp/cdb");
        report.record_warning(CdbWarning::LanguageNotEnglish {
            language: "fr".to_owned(),
        });
        assert!(report.is_conformant());
        assert!(report.class_passed(RequirementsClass::FileNaming));
        assert_eq!(report.warnings(RequirementsClass::FileNaming).len(), 1);

        // A Metadata(Link) violation is normalized to Links on record.
        let mut report = ConformanceReport::new("simulation", "/tmp/cdb");
        report.record_violation(CdbViolation::Metadata(MetadataViolation::Link(
            LinkViolation::MissingRel,
        )));
        assert!(!report.class_passed(RequirementsClass::Links));
        assert!(report.class_passed(RequirementsClass::Metadata));
        let links = report.violations(RequirementsClass::Links);
        assert_eq!(links.len(), 1);
        assert!(matches!(
            links[0],
            CdbViolation::Link(LinkViolation::MissingRel)
        ));
    }

    /// Design: the human-readable report shows the profile name and a
    /// PASS/FAIL line per class.
    #[test]
    fn conformance_report_display_human_readable() {
        let mut report = ConformanceReport::new("simulation", "/tmp/cdb");
        report.record_violation(CrsViolation::MissingEpoch.into());
        let text = report.to_string();
        assert!(text.contains("simulation"), "{text}");
        assert!(text.contains("PASS"), "{text}");
        assert!(text.contains("FAIL"), "{text}");
    }
}
