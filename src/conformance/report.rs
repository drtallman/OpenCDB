//! The conformance report: findings bucketed by [`RequirementsClass`].

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::conformance::{CdbViolation, CdbWarning, RequirementsClass};
use crate::metadata::MetadataViolation;

/// The violations and warnings recorded against a single requirements class,
/// plus whether the datastore held any content that class governs.
/// Fields are open, mirroring [`crate::hierarchy::HierarchyReport`].
///
/// `#[non_exhaustive]`: the fields are public and 1.0 freezes them, so the
/// struct must stay open to fields a later class needs — [`Self::has_content`]
/// is itself exactly such a field. Downstream code reads the fields and
/// constructs values through [`Default`] rather than with a struct literal.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ClassFindings {
    /// SHALL violations for this class.
    pub violations: Vec<CdbViolation>,
    /// SHOULD warnings for this class.
    pub warnings: Vec<CdbWarning>,
    /// Whether the datastore holds content this class governs — a tiling
    /// scheme for Tiling, a `domainSet`-bearing record for Coverages, a
    /// `versions/` journal for Versioning, and so on.
    ///
    /// A declared class with no such content **passes** (design spec §4: the
    /// class describes what the profile *supports*, and Annex A nowhere
    /// requires the content to exist), so this flag never affects pass/fail.
    /// It exists to separate *checked-and-clean* from
    /// *checked-with-no-content*, the reading a silent pass on an empty
    /// datastore would otherwise conflate — the likeliest source of a false
    /// green.
    ///
    /// `true` for the five mandatory classes by construction: the datastore
    /// root, its names, its global metadata record, and its storage CRS are
    /// precisely their content, and `/conf/minimal-core` requires all four.
    pub has_content: bool,
}

/// The outcome of validating a datastore against a profile (Annex A
/// `/conf/minimal-core`): findings bucketed by [`RequirementsClass`].
///
/// **Which classes a report lists** is "the classes that were checked", not
/// "the classes the profile declared" — the two coincide today but will not
/// once the content sweep lands. A report lists the five mandatory classes
/// always, plus every optional class the profile declares (checked by its own
/// stage, whether or not the datastore holds matching content — see
/// [`ClassFindings::has_content`]), plus any class that turns out to govern
/// content the profile failed to declare.
///
/// Conformance is decided by violations alone; warnings never affect
/// [`Self::is_conformant`] or [`Self::class_passed`], and neither does
/// [`Self::class_has_content`].
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
    /// invariant behind `/conf/minimal-core`). They are seeded
    /// content-bearing: their content is the datastore's own mandatory
    /// artifacts (see [`ClassFindings::has_content`]).
    pub(crate) fn new(profile: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        let mut classes = BTreeMap::new();
        for class in RequirementsClass::MANDATORY {
            classes.insert(
                class,
                ClassFindings {
                    has_content: true,
                    ..ClassFindings::default()
                },
            );
        }
        Self {
            profile: profile.into(),
            root: root.into(),
            classes,
        }
    }

    /// Lists `class` with empty findings and no content yet, so a class the
    /// profile declares appears in the report even when its stage finds
    /// nothing to check. Idempotent, and never clears findings or content
    /// already recorded.
    pub(crate) fn declare_class(&mut self, class: RequirementsClass) {
        self.classes.entry(class).or_default();
    }

    /// Records that the datastore holds content `class` governs, listing the
    /// class if it was not listed already. Pass/fail is untouched: this is
    /// the checked-and-clean vs. checked-with-no-content distinction only.
    pub(crate) fn mark_content(&mut self, class: RequirementsClass) {
        self.classes.entry(class).or_default().has_content = true;
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

    /// Whether the datastore held content `class` governs
    /// ([`ClassFindings::has_content`]); `false` for an unlisted class. A
    /// class that passed with `false` was checked against nothing — the
    /// distinction between a clean datastore and an empty one. Never affects
    /// [`Self::class_passed`] or [`Self::is_conformant`].
    pub fn class_has_content(&self, class: RequirementsClass) -> bool {
        match self.classes.get(&class) {
            Some(findings) => findings.has_content,
            None => false,
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
            // §4: a declared class with no content passes — but say so, so a
            // pass on an empty datastore cannot read as a pass on a clean one.
            let content = if findings.has_content {
                ""
            } else {
                " (no content)"
            };
            writeln!(f, "  [{status}] {class}{content}")?;
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
        // Membership, not position: a report lists every mandatory class and
        // no optional class it was never told about. (Once `validate` seeds
        // the profile's declared optional classes, an index-keyed assertion
        // would break for reasons that have nothing to do with this test.)
        for class in RequirementsClass::MANDATORY {
            assert!(listed.contains(&class), "{class} not listed");
        }
        for class in RequirementsClass::OPTIONAL {
            assert!(!listed.contains(&class), "{class} listed unbidden");
        }
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

    /// Design spec §4 — "a declared class with no corresponding content
    /// passes", but the report distinguishes *checked-and-clean* from
    /// *checked-with-no-content*, because a silent pass on an empty
    /// datastore is the failure mode most likely to give a false green. A
    /// declared class starts content-free and passing; marking content flips
    /// the flag without touching pass/fail, and `Display` says which is
    /// which. The mandatory five are content-bearing by construction — the
    /// datastore root, its names, its global metadata and its storage CRS
    /// are exactly their content.
    #[test]
    fn conformance_report_distinguishes_no_content_from_clean() {
        let mut report = ConformanceReport::new("simulation", "/tmp/cdb");
        for class in RequirementsClass::MANDATORY {
            assert!(report.class_has_content(class), "{class}");
        }
        // An unlisted class has no content and no findings.
        assert!(!report.class_has_content(RequirementsClass::Tiling));

        // Declaring a class lists it, passing and content-free.
        report.declare_class(RequirementsClass::Tiling);
        report.declare_class(RequirementsClass::Topology);
        assert!(report.class_passed(RequirementsClass::Tiling));
        assert!(!report.class_has_content(RequirementsClass::Tiling));
        let listed: Vec<RequirementsClass> = report.classes().map(|(class, _)| class).collect();
        assert!(listed.contains(&RequirementsClass::Tiling));

        // Marking content flips only the flag.
        report.mark_content(RequirementsClass::Tiling);
        assert!(report.class_has_content(RequirementsClass::Tiling));
        assert!(report.class_passed(RequirementsClass::Tiling));
        assert!(report.is_conformant());

        let text = report.to_string();
        assert!(text.contains("[PASS] tiling\n"), "{text}");
        assert!(text.contains("[PASS] topology (no content)"), "{text}");
    }
}
