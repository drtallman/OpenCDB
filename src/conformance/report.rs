//! The conformance report: findings bucketed by [`RequirementsClass`].

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::conformance::{CdbViolation, CdbWarning, RequirementsClass};
use crate::metadata::MetadataViolation;

/// How much a requirements class's content was actually judged — the third
/// state a `has_content` boolean could not express.
///
/// A class that *passes* has done so in one of three ways, and a machine
/// consumer reading `"passed": true` needs to know which:
///
/// | State | Meaning |
/// |---|---|
/// | [`Self::Checked`] | content was present and this crate's datastore-level check ran over it |
/// | [`Self::NoContent`] | the datastore held nothing this class governs |
/// | [`Self::Unchecked`] | content was present, but this crate has **no** datastore-level check that could judge it |
///
/// The third state is not a gap to be closed later: it is the honest reading
/// of Geometry and Topology, whose subjects (geometry instances, topological
/// graphs) live inside payloads this crate deliberately does not decode
/// (`docs/CONFORMANCE.md` §6). Their stages therefore cannot produce a
/// finding, and reporting them as `Checked` would assert a check that
/// provably never ran.
///
/// None of the three affects pass/fail
/// ([`ConformanceReport::class_passed`], [`ConformanceReport::is_conformant`]).
///
/// **Closed, not `#[non_exhaustive]`**, unlike the class and finding
/// vocabularies: those track a draft standard that can grow, whereas these
/// three states are a complete partition of what this crate can know about a
/// class — content was judged, was absent, or was beyond the validator's
/// reach. A consumer is meant to `match` all three exhaustively.
///
/// **Deliberately not `Ord`/`Hash`**, unlike [`RequirementsClass`], whose
/// ordering is load-bearing (it is the documented order a report lists its
/// classes in). These three states have no meaningful rank — `Unchecked` is
/// not "more" than `Checked` — so deriving an order would freeze an
/// accidental one at 1.0, and removing a derive afterwards is a breaking
/// change. Equality is all the type promises.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ContentCoverage {
    /// The datastore holds no content this class governs. A **declared**
    /// class in this state passes (design spec §4: the class describes what
    /// the profile *supports*, and Annex A nowhere requires the content to
    /// exist) — but the report says so rather than rendering a silent pass,
    /// which is the likeliest source of a false green.
    #[default]
    NoContent,
    /// Content was present and a datastore-level check ran over it: the
    /// class is *clean*, not merely quiet. The five mandatory classes are in
    /// this state by construction — the datastore root, its names, its
    /// global metadata record and its storage CRS are precisely their
    /// content, and `/conf/minimal-core` requires all four.
    Checked,
    /// Content was present and went **unjudged**: this crate has no
    /// datastore-level check for it. Geometry and Topology are the two
    /// classes in this state; see the type's own documentation and
    /// `docs/CONFORMANCE.md` §6. A pass here means "we did not look", never
    /// "we looked and it was fine".
    Unchecked,
}

impl ContentCoverage {
    /// The stable wire token: `"none"`, `"checked"`, or `"unchecked"`.
    pub fn as_str(self) -> &'static str {
        match self {
            ContentCoverage::NoContent => "none",
            ContentCoverage::Checked => "checked",
            ContentCoverage::Unchecked => "unchecked",
        }
    }

    /// Whether the datastore held content this class governs at all —
    /// [`Self::Checked`] or [`Self::Unchecked`]. This is the question
    /// [`ConformanceReport::class_has_content`] answers; it deliberately does
    /// **not** distinguish judged content from unjudged.
    pub fn has_content(self) -> bool {
        !matches!(self, ContentCoverage::NoContent)
    }
}

impl fmt::Display for ContentCoverage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The token — [`ContentCoverage::as_str`] — is the wire form. Hand-written
/// rather than derived for the same reason [`RequirementsClass`]'s is: the
/// Rust variant names must never leak into a shape that freezes at 1.0.
impl serde::Serialize for ContentCoverage {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// The violations and warnings recorded against a single requirements class,
/// plus how far the datastore's content for that class was actually judged.
/// Fields are open, mirroring [`crate::hierarchy::HierarchyReport`].
///
/// `#[non_exhaustive]`: the fields are public and 1.0 freezes them, so the
/// struct must stay open to fields a later class needs — [`Self::coverage`]
/// is itself exactly such a field. Downstream code reads the fields and
/// constructs values through [`Default`] rather than with a struct literal.
///
/// Deliberately **neither `Serialize` nor `Deserialize`**. A derived
/// `Serialize` on a `#[non_exhaustive]` struct is a second public wire shape
/// that silently gains a field the day a later class needs one — exactly the
/// drift [`ConformanceReport`]'s hand-written impl exists to avoid, and it
/// would disagree with that impl besides (no `class`, no `passed`). The one
/// serialized form of these findings is the report's own class entry, which
/// writes them alongside the class token, its requirements URI and the
/// derived verdict.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ClassFindings {
    /// SHALL violations for this class.
    pub violations: Vec<CdbViolation>,
    /// SHOULD warnings for this class.
    pub warnings: Vec<CdbWarning>,
    /// How far the datastore's content for this class was judged: checked,
    /// absent, or present-but-unjudged. See [`ContentCoverage`]. Never
    /// affects pass/fail.
    ///
    /// The content signal is per class — a tiling scheme for Tiling, a
    /// `domainSet`-bearing record for Coverages, a `versions/` journal for
    /// Versioning, and so on.
    ///
    /// **What [`ContentCoverage::NoContent`] does and does not mean.** It
    /// means *this run found no such content*, which is not the same as the
    /// datastore holding none. A resource metadata record that fails to parse
    /// is dropped before the optional stages see it, and a `GlobalMetadata`
    /// or storage CRS that cannot be read makes the stages that depend on it
    /// return early — so a store whose records do carry `domainSet`, `uom` or
    /// `windingOrder` can still render `(no content)` for those classes. In
    /// every such case the *reason* is filed as a violation under Metadata or
    /// CRS, so the report is never silently wrong overall; but this field
    /// alone cannot tell "absent" from "unreadable".
    pub coverage: ContentCoverage,
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
///   [`ClassFindings::coverage`]);
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
/// [`Self::class_coverage`] (nor its coarser form,
/// [`Self::class_has_content`]).
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
    /// [`ContentCoverage::Checked`]: their content is the datastore's own
    /// mandatory artifacts, and the mandatory stages do judge it.
    pub(crate) fn new(profile: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        let mut classes = BTreeMap::new();
        for &class in RequirementsClass::MANDATORY {
            classes.insert(
                class,
                ClassFindings {
                    coverage: ContentCoverage::Checked,
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

    /// Records that the datastore holds content `class` governs **and that a
    /// datastore-level check ran over it** ([`ContentCoverage::Checked`]),
    /// listing the class if it was not listed already. Pass/fail is
    /// untouched.
    ///
    /// Does not overwrite [`ContentCoverage::Unchecked`]: a class with any
    /// unjudged content must never go on to claim it was checked. The
    /// pessimistic direction is the only safe one — over-claiming a check is
    /// precisely the false green this vocabulary exists to prevent.
    pub(crate) fn mark_content(&mut self, class: RequirementsClass) {
        let findings = self.classes.entry(class).or_default();
        if findings.coverage == ContentCoverage::NoContent {
            findings.coverage = ContentCoverage::Checked;
        }
    }

    /// Records that the datastore holds content `class` governs for which
    /// this crate has **no** datastore-level check
    /// ([`ContentCoverage::Unchecked`]) — the Geometry and Topology case.
    /// Listing and pass/fail behave exactly as [`Self::mark_content`]; only
    /// the honesty of the report changes. Sticky, per that method's note.
    pub(crate) fn mark_unchecked_content(&mut self, class: RequirementsClass) {
        self.classes.entry(class).or_default().coverage = ContentCoverage::Unchecked;
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

    /// Whether this run found content `class` governs; `false` for an
    /// unlisted class. A class that passed with `false` was checked against
    /// nothing — the distinction between a clean datastore and an empty one.
    /// Never affects [`Self::class_passed`] or [`Self::is_conformant`].
    ///
    /// This is the coarse question. It does **not** distinguish content that
    /// was judged from content that this crate has no datastore-level check
    /// for, nor "no such content" from "content that could not be read" — see
    /// [`Self::class_coverage`] for the first and [`ClassFindings::coverage`]
    /// for the second.
    pub fn class_has_content(&self, class: RequirementsClass) -> bool {
        self.class_coverage(class).has_content()
    }

    /// How far `class`'s content was judged: checked, absent, or
    /// present-but-unjudged ([`ContentCoverage`]). An unlisted class is
    /// [`ContentCoverage::NoContent`]. Never affects
    /// [`Self::class_passed`] or [`Self::is_conformant`] — it is how a
    /// consumer tells a *clean* pass from a *vacuous* one.
    pub fn class_coverage(&self, class: RequirementsClass) -> ContentCoverage {
        match self.classes.get(&class) {
            Some(findings) => findings.coverage,
            None => ContentCoverage::NoContent,
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
/// its §7 requirements-module URI, the three flags a reader needs to
/// interpret the entry, and the findings themselves.
///
/// `passed`, `requirements_uri` and `has_content` are *derived* — `passed` is
/// "`violations` is empty", the URI is a function of the class, and
/// `has_content` is `content != "none"` — and are written out anyway. A
/// report is read by tools that are not this crate, and asking every one of
/// them to re-derive the verdict is how two consumers end up disagreeing
/// about whether a datastore conformed.
///
/// `content` is the load-bearing one: it is the only field that separates
/// `passed: true` because the content was judged clean from `passed: true`
/// because nothing was, or could be, judged. `has_content` is kept beside it
/// for the consumer that only needs the coarse question answered.
struct ClassEntry<'a> {
    class: RequirementsClass,
    findings: &'a ClassFindings,
}

impl serde::Serialize for ClassEntry<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let mut entry = serializer.serialize_struct("ClassEntry", 7)?;
        entry.serialize_field("class", &self.class)?;
        entry.serialize_field("requirements_uri", self.class.requirements_uri())?;
        entry.serialize_field("passed", &self.findings.violations.is_empty())?;
        entry.serialize_field("content", &self.findings.coverage)?;
        entry.serialize_field("has_content", &self.findings.coverage.has_content())?;
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
///                  "passed": true, "content": "checked", "has_content": true,
///                  "violations": [], "warnings": [] }, … ]
/// }
/// ```
///
/// Four decisions worth stating, since they are permanent:
///
/// - **A class entry carries `content`, not just `has_content`.** A verdict
///   of `"passed": true` has three quite different meanings — the content was
///   judged clean, there was no content, or there was content this crate has
///   no datastore-level check for ([`ContentCoverage`]) — and a machine
///   consumer that cannot tell them apart will read the third as the first.
///   That is the false green the whole report layer is built to avoid, so the
///   distinction is on the wire rather than only in `docs/CONFORMANCE.md`.
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
            // §4: a declared class with no content passes, and so does one
            // whose content this crate cannot judge — but say which, so
            // neither can read as a pass on content that was checked clean.
            let content = match findings.coverage {
                ContentCoverage::Checked => "",
                ContentCoverage::NoContent => " (no content)",
                ContentCoverage::Unchecked => " (content not checked)",
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
        for &class in RequirementsClass::MANDATORY {
            assert!(listed.contains(&class), "{class} not listed");
        }
        for &class in RequirementsClass::OPTIONAL {
            assert!(!listed.contains(&class), "{class} listed unbidden");
        }
        assert_eq!(report.profile(), "simulation");
        assert_eq!(report.root(), Path::new("/tmp/cdb"));
        assert!(report.is_conformant());
        for &class in RequirementsClass::MANDATORY {
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
        for &class in RequirementsClass::MANDATORY {
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

    /// Design spec §4 (as-built amendment: the third content state) — a class
    /// is in one of **three** states, not two: it was checked against content,
    /// it was declared and had no content, or it carries content this crate
    /// has no datastore-level check for. The third is the honest reading of
    /// the Geometry and Topology stages, whose subjects live in payloads the
    /// crate does not decode. All three pass; none affects
    /// [`ConformanceReport::is_conformant`]. `Unchecked` is sticky: once any
    /// of a class's content has gone unjudged, the class never claims to have
    /// been checked.
    #[test]
    fn req_core_conformance_report_three_content_states() {
        let mut report = ConformanceReport::new("simulation", "/tmp/cdb");

        // Unlisted: no content, and no claim of a check.
        assert_eq!(
            report.class_coverage(RequirementsClass::Geometry),
            ContentCoverage::NoContent
        );
        assert!(!ContentCoverage::NoContent.has_content());

        // Declared: listed, passing, still no content.
        report.declare_class(RequirementsClass::Geometry);
        report.declare_class(RequirementsClass::Tiling);
        report.declare_class(RequirementsClass::Topology);
        assert_eq!(
            report.class_coverage(RequirementsClass::Geometry),
            ContentCoverage::NoContent
        );

        // Checked content: the class was judged and came back clean.
        report.mark_content(RequirementsClass::Tiling);
        assert_eq!(
            report.class_coverage(RequirementsClass::Tiling),
            ContentCoverage::Checked
        );
        assert!(report.class_has_content(RequirementsClass::Tiling));

        // Unchecked content: content is real, the check is not.
        report.mark_unchecked_content(RequirementsClass::Geometry);
        assert_eq!(
            report.class_coverage(RequirementsClass::Geometry),
            ContentCoverage::Unchecked
        );
        assert!(
            report.class_has_content(RequirementsClass::Geometry),
            "unchecked content is still content"
        );
        assert!(report.class_passed(RequirementsClass::Geometry));
        assert!(report.is_conformant());

        // Sticky pessimism, both orders: a class that has any unchecked
        // content never reports itself checked.
        report.mark_content(RequirementsClass::Geometry);
        assert_eq!(
            report.class_coverage(RequirementsClass::Geometry),
            ContentCoverage::Unchecked
        );
        report.mark_unchecked_content(RequirementsClass::Tiling);
        assert_eq!(
            report.class_coverage(RequirementsClass::Tiling),
            ContentCoverage::Unchecked
        );

        // `Display` renders all three distinguishably.
        let text = report.to_string();
        assert!(
            text.contains("[PASS] geometry (content not checked)"),
            "{text}"
        );
        assert!(text.contains("[PASS] topology (no content)"), "{text}");
        assert!(text.contains("[PASS] crs\n"), "{text}");

        // ... and so does the wire shape, which carries the token alongside
        // the derived boolean rather than in place of it.
        let value = serde_json::to_value(&report).unwrap();
        let entry = |name: &str| {
            value["classes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["class"] == name)
                .cloned()
                .unwrap()
        };
        assert_eq!(entry("geometry")["content"], "unchecked");
        assert_eq!(entry("geometry")["has_content"], true);
        assert_eq!(entry("topology")["content"], "none");
        assert_eq!(entry("topology")["has_content"], false);
        assert_eq!(entry("crs")["content"], "checked");
        assert_eq!(entry("crs")["has_content"], true);
    }

    /// Design spec §7 — [`ContentCoverage`]'s three tokens are the wire form
    /// and are stable: a consumer keys on them. Hand-written like
    /// [`RequirementsClass`]'s so the Rust variant names never leak.
    #[test]
    fn req_core_conformance_content_coverage_tokens() {
        for (coverage, token, has_content) in [
            (ContentCoverage::NoContent, "none", false),
            (ContentCoverage::Checked, "checked", true),
            (ContentCoverage::Unchecked, "unchecked", true),
        ] {
            assert_eq!(coverage.as_str(), token);
            assert_eq!(coverage.to_string(), token);
            assert_eq!(coverage.has_content(), has_content);
            assert_eq!(
                serde_json::to_string(&coverage).unwrap(),
                format!("\"{token}\"")
            );
        }
        assert_eq!(ContentCoverage::default(), ContentCoverage::NoContent);
    }
}
