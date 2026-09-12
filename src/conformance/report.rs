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
/// `Serialize` but **not** `Deserialize`: findings are output, and
/// `#[non_exhaustive]` means the field set is deliberately still open (see
/// [`ConformanceReport`]'s `Serialize` for the full reasoning). A report's
/// own class entry writes these three fields alongside the class token, its
/// requirements URI, and the derived verdict.
#[derive(Debug, Clone, Default, serde::Serialize)]
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
/// "the classes the profile declared". A report lists:
///
/// - the five mandatory classes, always;
/// - every optional class the profile declares, checked by its own stage
///   whether or not the datastore holds matching content (see
///   [`ClassFindings::has_content`]);
/// - **plus any class that turns out to govern content the profile did not
///   declare.** The content sweep records such content as a
///   [`CdbViolation::DeclarationMismatch`] filed under that class, which
///   lists it. So a class appearing here is not evidence the profile claimed
///   it — a *failing* optional class the profile never mentioned is exactly
///   what an undeclared-content finding looks like. Read
///   [`ApplicationProfile::conformance_classes`] for what was declared.
///
/// [`ApplicationProfile::conformance_classes`]: crate::profiles::ApplicationProfile::conformance_classes
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

/// One class's entry in a serialized [`ConformanceReport`]: the class token,
/// its §7 requirements-module URI, the two flags a reader needs to interpret
/// the entry, and the findings themselves.
///
/// `passed` and `requirements_uri` are *derived* — [`Self::passed`] is
/// "`violations` is empty" and the URI is a function of the class — and are
/// written out anyway. A report is read by tools that are not this crate, and
/// asking every one of them to re-derive the verdict is how two consumers end
/// up disagreeing about whether a datastore conformed.
struct ClassEntry<'a> {
    class: RequirementsClass,
    findings: &'a ClassFindings,
}

impl serde::Serialize for ClassEntry<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let mut entry = serializer.serialize_struct("ClassEntry", 6)?;
        entry.serialize_field("class", &self.class)?;
        entry.serialize_field("requirements_uri", self.class.requirements_uri())?;
        entry.serialize_field("passed", &self.findings.violations.is_empty())?;
        entry.serialize_field("has_content", &self.findings.has_content)?;
        entry.serialize_field("violations", &self.findings.violations)?;
        entry.serialize_field("warnings", &self.findings.warnings)?;
        entry.end()
    }
}

/// The report's wire form (design spec §7), designed rather than derived
/// because it freezes at 1.0:
///
/// ```json
/// {
///   "profile": "simulation",
///   "root": "/tmp/cdb",
///   "conformant": false,
///   "classes": [ { "class": "crs", "requirements_uri": "/req/core/data-representation",
///                  "passed": true, "has_content": true,
///                  "violations": [], "warnings": [] }, … ]
/// }
/// ```
///
/// Three decisions worth stating, since they are permanent:
///
/// - **Classes are an array, not a JSON object keyed by class.** [`Ord`]
///   order is the documented listing order of a report, and an array is the
///   only structure that preserves it in every serializer — a map's key order
///   is the format's business, not the report's. Each entry names its own
///   class, so nothing is lost.
/// - **`root` is written lossily** ([`std::path::Path::to_string_lossy`]).
///   Serializing a report must not fail because a datastore lives under a
///   path that is not valid UTF-8.
/// - **`Serialize` only — deliberately no `Deserialize`.** A report is an
///   *output document*, not a transport format: its findings are structured
///   Rust values ([`CdbViolation`]) that a `code` and a `message` cannot
///   reconstitute, and a type that deserialized into fabricated findings
///   would be lying about where they came from. A consumer reads the JSON as
///   JSON; the one piece that genuinely round-trips, the class token, is
///   [`RequirementsClass`], which *is* `Deserialize`. The same reasoning
///   covers [`ClassFindings`], which is additionally `#[non_exhaustive]` —
///   deriving `Deserialize` on it would freeze a field set that is
///   deliberately open.
impl serde::Serialize for ConformanceReport {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let entries: Vec<ClassEntry<'_>> = self
            .classes
            .iter()
            .map(|(&class, findings)| ClassEntry { class, findings })
            .collect();

        let mut report = serializer.serialize_struct("ConformanceReport", 4)?;
        report.serialize_field("profile", &self.profile)?;
        report.serialize_field("root", &self.root.to_string_lossy())?;
        report.serialize_field("conformant", &self.is_conformant())?;
        report.serialize_field("classes", &entries)?;
        report.end()
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

    /// Design spec §7 — the serialized report mirrors the report's own
    /// organization: the profile and root it was produced for, the one-bit
    /// verdict, and the classes it lists, each entry carrying its class token,
    /// its requirements-module URI, its pass/content flags, and its findings.
    /// Classes are an **array**, not a JSON object: [`Ord`] order is the
    /// documented listing order, and an array is the only structure that
    /// preserves it in every serializer.
    #[test]
    fn req_core_conformance_report_serializes_by_class() {
        let mut report = ConformanceReport::new("simulation", "/tmp/cdb");
        report.declare_class(RequirementsClass::Tiling);
        report.record_violation(NamingViolation::EmptyName.into());
        report.record_warning(CdbWarning::LanguageNotEnglish {
            language: "fr".to_owned(),
        });

        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["profile"], "simulation");
        assert_eq!(value["root"], "/tmp/cdb");
        assert_eq!(value["conformant"], false);

        let classes = value["classes"].as_array().unwrap();
        let listed: Vec<&str> = classes
            .iter()
            .map(|entry| entry["class"].as_str().unwrap())
            .collect();
        // Ord order, mandatory five plus the declared optional class.
        assert_eq!(
            listed,
            vec![
                "crs",
                "file-naming",
                "file-structure",
                "links",
                "metadata",
                "tiling",
            ]
        );

        let naming = &classes[1];
        assert_eq!(naming["class"], "file-naming");
        assert_eq!(naming["requirements_uri"], "/req/core/naming-system");
        assert_eq!(naming["passed"], false);
        assert_eq!(naming["has_content"], true);
        assert_eq!(naming["violations"].as_array().unwrap().len(), 1);
        assert_eq!(naming["warnings"][0]["code"], "/req/core/name-language-B");

        let tiling = classes.last().unwrap();
        assert_eq!(tiling["class"], "tiling");
        assert_eq!(tiling["passed"], true);
        assert_eq!(tiling["has_content"], false);
        assert!(tiling["violations"].as_array().unwrap().is_empty());

        // A clean report is conformant on the wire too.
        let clean = ConformanceReport::new("simulation", "/tmp/cdb");
        assert_eq!(serde_json::to_value(&clean).unwrap()["conformant"], true);
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
