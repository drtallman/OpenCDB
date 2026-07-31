//! Datastore facade and unified conformance reporting.
//!
//! Implements the reporting surface behind Annex A `/conf/minimal-core`: a
//! [`ConformanceReport`] buckets findings by [`RequirementsClass`], seeded with
//! the five mandatory classes so they are always listed. Per the crate policy,
//! a spec **SHALL** failure is a [`CdbViolation`] (a `thiserror` `Error`) and a
//! spec **SHOULD** finding is a [`CdbWarning`] (`Display` only — never an
//! `Error`); the two are never conflated. Conformance is decided by violations
//! alone: [`ConformanceReport::is_conformant`] and
//! [`ConformanceReport::class_passed`] ignore warnings.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::crs::{CrsViolation, CrsWarning};
use crate::hierarchy::{HierarchyViolation, HierarchyWarning};
use crate::links::LinkViolation;
use crate::metadata::MetadataViolation;
use crate::naming::{NamingViolation, NamingWarning};
use crate::profiles::RequirementsClass;

/// A datastore-wide SHALL violation, gathering every requirements module's
/// violation plus the two profile-layer findings Annex A `/conf/minimal-core`
/// introduces. Each variant maps to exactly one [`RequirementsClass`] via
/// [`CdbViolation::class`], which is how a [`ConformanceReport`] buckets it.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CdbViolation {
    /// A File Naming violation (spec §7.4).
    #[error(transparent)]
    Naming(#[from] NamingViolation),
    /// A File Structure violation (spec §7.5).
    #[error(transparent)]
    Hierarchy(#[from] HierarchyViolation),
    /// A Links violation (spec §7.7).
    #[error(transparent)]
    Link(#[from] LinkViolation),
    /// A Metadata violation (spec §7.9). A nested [`MetadataViolation::Link`]
    /// is a Links finding — see [`CdbViolation::class`].
    #[error(transparent)]
    Metadata(#[from] MetadataViolation),
    /// A CRS violation (spec §7.3).
    #[error(transparent)]
    Crs(#[from] CrsViolation),
    /// The profile fails to declare a mandatory conformance class
    /// (Annex A `/conf/minimal-core`); filed under the undeclared class.
    #[error(
        "profile {profile:?} does not declare mandatory conformance class {class} (violates /conf/minimal-core)"
    )]
    MissingConformanceDeclaration {
        profile: String,
        class: RequirementsClass,
    },
    /// A datastore element contradicts the profile's declaration (e.g. a
    /// different metadata encoding or storage CRS); filed under the class whose
    /// clause is broken.
    #[error(
        "datastore {element} is {found:?} but profile {profile:?} declares {declared:?} (violates {clause})"
    )]
    DeclarationMismatch {
        profile: String,
        class: RequirementsClass,
        element: &'static str,
        declared: String,
        found: String,
        clause: &'static str,
    },
}

impl CdbViolation {
    /// The requirements class this violation belongs to. Module violations map
    /// one-to-one, except a [`MetadataViolation::Link`] — an association link
    /// carried inside a metadata record — which is a Links finding. The two
    /// profile-layer variants return their carried `class`.
    pub fn class(&self) -> RequirementsClass {
        match self {
            CdbViolation::Naming(_) => RequirementsClass::FileNaming,
            CdbViolation::Hierarchy(_) => RequirementsClass::FileStructure,
            CdbViolation::Link(_) => RequirementsClass::Links,
            CdbViolation::Metadata(MetadataViolation::Link(_)) => RequirementsClass::Links,
            CdbViolation::Metadata(_) => RequirementsClass::Metadata,
            CdbViolation::Crs(_) => RequirementsClass::Crs,
            CdbViolation::MissingConformanceDeclaration { class, .. } => *class,
            CdbViolation::DeclarationMismatch { class, .. } => *class,
        }
    }
}

/// A datastore-wide SHOULD finding. Deliberately **not** an `Error`: a spec
/// recommendation must never masquerade as a failure. Wraps each module's
/// warning and adds [`CdbWarning::LanguageNotEnglish`] for Recommendation
/// Name3-B (`/req/core/name-language B`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CdbWarning {
    /// A File Naming recommendation finding (spec §7.4).
    Naming(NamingWarning),
    /// A File Structure recommendation finding (spec §7.5).
    Hierarchy(HierarchyWarning),
    /// A CRS recommendation finding (spec §7.3).
    Crs(CrsWarning),
    /// The datastore language is not English (Recommendation Name3-B); English
    /// is recommended for interoperability.
    LanguageNotEnglish { language: String },
}

impl CdbWarning {
    /// The requirements class this warning belongs to. `LanguageNotEnglish` is
    /// a File Naming recommendation (Name3-B).
    pub fn class(&self) -> RequirementsClass {
        match self {
            CdbWarning::Naming(_) | CdbWarning::LanguageNotEnglish { .. } => {
                RequirementsClass::FileNaming
            }
            CdbWarning::Hierarchy(_) => RequirementsClass::FileStructure,
            CdbWarning::Crs(_) => RequirementsClass::Crs,
        }
    }
}

impl From<NamingWarning> for CdbWarning {
    fn from(warning: NamingWarning) -> Self {
        CdbWarning::Naming(warning)
    }
}

impl From<HierarchyWarning> for CdbWarning {
    fn from(warning: HierarchyWarning) -> Self {
        CdbWarning::Hierarchy(warning)
    }
}

impl From<CrsWarning> for CdbWarning {
    fn from(warning: CrsWarning) -> Self {
        CdbWarning::Crs(warning)
    }
}

impl fmt::Display for CdbWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CdbWarning::Naming(warning) => warning.fmt(f),
            CdbWarning::Hierarchy(warning) => warning.fmt(f),
            CdbWarning::Crs(warning) => warning.fmt(f),
            CdbWarning::LanguageNotEnglish { language } => write!(
                f,
                "datastore language {language:?}; English is recommended \
                 (/req/core/name-language B)"
            ),
        }
    }
}

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

/// Crate-internal construction and recording. Their non-test consumer is
/// `CdbDatastore::validate`, added in a later Phase 14a step; until then the
/// library itself has no caller, hence the `dead_code` allowance.
#[allow(dead_code)]
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

    /// Taxonomy (`/conf/minimal-core`): every module violation folds into
    /// [`CdbViolation`] via `From`, and `class()` files it under the correct
    /// requirements class — the report-layer analogue of the crate's
    /// `*_converts_to_cdb_error` tests. A `LinkViolation` nested inside a
    /// [`MetadataViolation`] still belongs to Links.
    #[test]
    fn cdb_violation_wraps_each_module_violation_and_maps_class() {
        let naming: CdbViolation = NamingViolation::EmptyName.into();
        assert_eq!(naming.class(), RequirementsClass::FileNaming);

        let hierarchy: CdbViolation = HierarchyViolation::MissingGlobalMetadata {
            root: PathBuf::from("/tmp/cdb"),
        }
        .into();
        assert_eq!(hierarchy.class(), RequirementsClass::FileStructure);

        let link: CdbViolation = LinkViolation::MissingRel.into();
        assert_eq!(link.class(), RequirementsClass::Links);

        let metadata: CdbViolation = MetadataViolation::MissingElement { element: "ID" }.into();
        assert_eq!(metadata.class(), RequirementsClass::Metadata);

        let crs: CdbViolation = CrsViolation::MissingEpoch.into();
        assert_eq!(crs.class(), RequirementsClass::Crs);

        // A Link violation nested inside a MetadataViolation belongs to Links.
        let nested = CdbViolation::Metadata(MetadataViolation::Link(LinkViolation::MissingRel));
        assert_eq!(nested.class(), RequirementsClass::Links);

        // The two profile-layer struct variants carry their own class.
        let missing = CdbViolation::MissingConformanceDeclaration {
            profile: "simulation".to_owned(),
            class: RequirementsClass::Links,
        };
        assert_eq!(missing.class(), RequirementsClass::Links);

        let mismatch = CdbViolation::DeclarationMismatch {
            profile: "simulation".to_owned(),
            class: RequirementsClass::Crs,
            element: "storage CRS",
            declared: "EPSG:4326".to_owned(),
            found: "EPSG:3857".to_owned(),
            clause: "/req/core/crs/crsStorage",
        };
        assert_eq!(mismatch.class(), RequirementsClass::Crs);
    }

    /// SHALL/SHOULD separation: module warnings fold into [`CdbWarning`] via
    /// `From`, `Display` delegates to the inner warning, `LanguageNotEnglish`
    /// cites Recommendation Name3-B (`/req/core/name-language B`), and `class()`
    /// files each finding.
    #[test]
    fn cdb_warning_wraps_and_displays() {
        let naming_warning = NamingWarning::NonAscii {
            name: "café".to_owned(),
        };
        let wrapped: CdbWarning = naming_warning.clone().into();
        assert_eq!(wrapped.to_string(), naming_warning.to_string());
        assert_eq!(wrapped.class(), RequirementsClass::FileNaming);

        let hierarchy_warning = HierarchyWarning::RootNameNotCdb {
            name: "MyStore".to_owned(),
        };
        let wrapped: CdbWarning = hierarchy_warning.clone().into();
        assert_eq!(wrapped.to_string(), hierarchy_warning.to_string());
        assert_eq!(wrapped.class(), RequirementsClass::FileStructure);

        let crs_warning = CrsWarning::NotWgs84 {
            found: "NTF (Paris)".to_owned(),
        };
        let wrapped: CdbWarning = crs_warning.clone().into();
        assert_eq!(wrapped.to_string(), crs_warning.to_string());
        assert_eq!(wrapped.class(), RequirementsClass::Crs);

        let language = CdbWarning::LanguageNotEnglish {
            language: "fr".to_owned(),
        };
        assert!(language.to_string().contains("name-language"), "{language}");
        assert!(language.to_string().contains("fr"), "{language}");
        assert_eq!(language.class(), RequirementsClass::FileNaming);
    }

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
