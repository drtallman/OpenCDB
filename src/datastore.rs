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
//!
//! [`CdbDatastore`] is the operational facade: [`CdbDatastore::create`]
//! materializes a datastore from a [`DatastoreSeed`] (its mandatory §7.9.4.1
//! identity) and an [`ApplicationProfile`] (its policy), and the read/write
//! methods persist and retrieve the global metadata, storage CRS, and
//! resource metadata records through the requirements modules.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use thiserror::Error;

use crate::crs::{CrsError, CrsViolation, CrsWarning, StorageCrs};
use crate::error::CdbError;
use crate::hierarchy::{DatastoreLayout, HierarchyError, HierarchyViolation, HierarchyWarning};
use crate::links::LinkViolation;
use crate::metadata::{
    GLOBAL_METADATA_STEM, GlobalMetadata, MetadataEncoding, MetadataError, MetadataViolation,
    ResourceMetadata, encoding_violations,
};
use crate::naming::{
    NamingViolation, NamingWarning, StyleGuide, component_warnings, file_warnings, split_extension,
};
use crate::profiles::{ApplicationProfile, RequirementsClass};
use crate::versioning::{
    self, ChangeAction, ChangeRecord, CollectionId, CollectionManifest, InverseOp,
    PendingCollection, PendingOp, VersioningError, VersioningViolation,
};

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

/// Crate-internal construction and recording, consumed by
/// [`CdbDatastore::validate`].
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

/// The four mandatory identity elements of a datastore's global metadata
/// record (§7.9.4.1 table: `ID`, `title`, `description`, `contactPoint`) plus
/// an optional creation instant. Everything else a global record carries —
/// language, standard, encoding, unit of measure — is *policy*, supplied by
/// the [`ApplicationProfile`] at [`CdbDatastore::create`]; a seed therefore
/// cannot overlap or contradict the profile.
///
/// Emptiness is not validated here: [`CdbDatastore::create`] catches it when
/// the global-metadata builder runs, before any directory is created.
#[derive(Debug, Clone)]
pub struct DatastoreSeed {
    id: String,
    title: String,
    description: String,
    contact_point: String,
    created: Option<DateTime<Utc>>,
}

impl DatastoreSeed {
    /// A seed carrying the four mandatory §7.9.4.1 identity elements. The
    /// creation instant defaults to the moment of [`CdbDatastore::create`];
    /// override it with [`Self::created`].
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        description: impl Into<String>,
        contact_point: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: description.into(),
            contact_point: contact_point.into(),
            created: None,
        }
    }

    /// Sets the record's creation instant (§7.9.4.1 `created`), overriding the
    /// default of "now" captured at create time.
    #[must_use]
    pub fn created(mut self, value: DateTime<Utc>) -> Self {
        self.created = Some(value);
        self
    }
}

/// The operational facade over a single CDB datastore: it creates and opens
/// datastores and reads and writes their global metadata, storage CRS, and
/// resource metadata records through the requirements modules.
///
/// A datastore stores **no** profile. It is self-describing for read/write
/// operations — encoding, CRS, and identity all live on disk — whereas an
/// [`ApplicationProfile`] is the yardstick only for conformance (`validate`,
/// added in a later step). Operations therefore never depend on which profile
/// a caller happens to hold.
#[derive(Debug, Clone)]
pub struct CdbDatastore {
    layout: DatastoreLayout,
}

impl CdbDatastore {
    /// Creates a new datastore under `parent`, materializing its root folder
    /// (named by [`ApplicationProfile::root_folder_name`] — `cdb` by default,
    /// Requirement File1/RFile1), the `global_metadata/` folder (Requirement
    /// File6), the global metadata record (Requirement Metadata1, composed
    /// from the `seed`'s identity and the profile's policy), and the
    /// storage-CRS record (Requirement CRS5).
    ///
    /// All policy is validated **before** the first disk write: the
    /// global-metadata builder checks the mandatory §7.9.4.1 elements and the
    /// profile's storage CRS is resolved up front, so a bad seed or a
    /// mis-declared profile fails before any directory exists.
    ///
    /// `create` is **not** atomic: a failure after the first write may leave a
    /// partial datastore on disk. The 0.1.0 milestone ships no cleanup guard.
    pub fn create(
        parent: &Path,
        profile: &dyn ApplicationProfile,
        seed: DatastoreSeed,
    ) -> Result<Self, CdbError> {
        let DatastoreSeed {
            id,
            title,
            description,
            contact_point,
            created,
        } = seed;
        // Compose the global record from the seed's identity and the profile's
        // policy; the builder validates the mandatory §7.9.4.1 elements.
        let language = profile.language().map_err(MetadataError::from)?;
        let metadata = GlobalMetadata::builder()
            .id(id)
            .title(title)
            .description(description)
            .contact_point(contact_point)
            .created(created.unwrap_or_else(Utc::now))
            .language(language)
            .standard(profile.metadata_standard())
            .encoding(profile.metadata_encoding())
            .uom(profile.uom())
            .build()
            .map_err(MetadataError::from)?;
        // Resolve the profile's storage CRS; a mis-declared CRS fails here,
        // still before any disk write (Requirements CRS3/CRS4).
        let crs = profile.storage_crs().map_err(CrsError::from)?;
        // Policy is now proven; only now touch the disk.
        let layout = DatastoreLayout::create_named(parent, profile.root_folder_name())?;
        metadata.write_to(&layout)?;
        crs.write_to(&layout)?;
        Ok(Self { layout })
    }

    /// Opens an existing datastore rooted at `root` (Requirement File3: the
    /// root must be an existing directory). Conformance is **not** judged
    /// here — that is the job of `validate`; `open` only confirms the root is
    /// a usable directory.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, CdbError> {
        let layout = DatastoreLayout::open(root)?;
        Ok(Self { layout })
    }

    /// Validates the datastore against `profile`, the abstract test behind
    /// Annex A `/conf/minimal-core`: it inspects the profile's declarations
    /// and the on-disk state of the five mandatory requirements classes (CRS,
    /// File Naming, File Structure, Links, Metadata) and returns a
    /// [`ConformanceReport`] bucketing every finding by [`RequirementsClass`].
    ///
    /// Conformance findings are **recorded, never returned as `Err`**: a
    /// non-conformant datastore still yields `Ok(report)` whose
    /// [`ConformanceReport::is_conformant`] is `false`. `Err` is reserved for
    /// *operational* I/O failures — an unreadable directory or file — that
    /// prevent inspection from completing at all. Warnings (SHOULD findings)
    /// are collected but never affect pass/fail.
    ///
    /// The datastore stores no profile, so the same datastore can be validated
    /// against any profile; the profile is purely the yardstick.
    pub fn validate(
        &self,
        profile: &dyn ApplicationProfile,
    ) -> Result<ConformanceReport, CdbError> {
        let mut report = ConformanceReport::new(profile.name(), self.root());

        // Stage 1 — /conf/minimal-core: every mandatory class must be declared.
        let declared = profile.conformance_classes();
        for class in RequirementsClass::MANDATORY {
            if !declared.contains(&class) {
                report.record_violation(CdbViolation::MissingConformanceDeclaration {
                    profile: profile.name().to_owned(),
                    class,
                });
            }
        }

        // Stage 2 — File Structure: fold the hierarchy report wholesale.
        // Its `Err` is operational (I/O or a malformed root name) — propagate.
        let hierarchy = self.layout.validate()?;
        for violation in hierarchy.violations {
            report.record_violation(violation.into());
        }
        for warning in hierarchy.warnings {
            report.record_warning(warning.into());
        }

        // Stage 3 — File Naming: walk the tree once, recording per-name
        // findings and collecting the file inventory the later stages reuse.
        let guide = profile.style_guide();
        let known_extensions = profile.known_extensions();
        let global_metadata_dir = self.layout.global_metadata_dir();
        let (global_metadata_files, file_logical_paths) = {
            let mut walk = NamingWalk {
                global_metadata_dir: &global_metadata_dir,
                guide: &guide,
                known_extensions: &known_extensions,
                report: &mut report,
                global_metadata_files: Vec::new(),
                file_logical_paths: Vec::new(),
            };
            walk.walk(self.root(), "").map_err(HierarchyError::Io)?;
            (walk.global_metadata_files, walk.file_logical_paths)
        };

        // Stage 4 — Metadata: read the global record and cross-check the
        // profile's Metadata2/5/8/4 declarations, then sweep every metadata
        // file's encoding (Metadata5).
        match GlobalMetadata::read_from(&self.layout) {
            Ok(global) => {
                if global.encoding != profile.metadata_encoding() {
                    report.record_violation(CdbViolation::DeclarationMismatch {
                        profile: profile.name().to_owned(),
                        class: RequirementsClass::Metadata,
                        element: "metadata encoding",
                        declared: profile.metadata_encoding().to_string(),
                        found: global.encoding.to_string(),
                        clause: "/req/core/metadata-encoding",
                    });
                }
                if global.metadata_standard != profile.metadata_standard() {
                    report.record_violation(CdbViolation::DeclarationMismatch {
                        profile: profile.name().to_owned(),
                        class: RequirementsClass::Metadata,
                        element: "metadata standard",
                        declared: profile.metadata_standard().to_string(),
                        found: global.metadata_standard.to_string(),
                        clause: "/req/core/metadata-standard",
                    });
                }
                if global.uom != profile.uom() {
                    report.record_violation(CdbViolation::DeclarationMismatch {
                        profile: profile.name().to_owned(),
                        class: RequirementsClass::Metadata,
                        element: "uom",
                        declared: profile.uom().to_string(),
                        found: global.uom.to_string(),
                        clause: "/req/core/metadata-uom-measure",
                    });
                }
                match profile.language() {
                    Ok(declared_language) => {
                        if global.language != declared_language {
                            report.record_violation(CdbViolation::DeclarationMismatch {
                                profile: profile.name().to_owned(),
                                class: RequirementsClass::Metadata,
                                element: "language",
                                declared: declared_language.to_string(),
                                found: global.language.to_string(),
                                clause: "/req/core/metadata-language",
                            });
                        }
                    }
                    // A profile whose own style-guide language is malformed is a
                    // Metadata-class finding, not an operational error.
                    Err(violation) => report.record_violation(violation.into()),
                }
                // Recommendation Name3-B: the datastore language SHOULD be
                // English; compare the primary subtag of the on-disk language.
                let primary = global
                    .language
                    .as_str()
                    .split('-')
                    .next()
                    .unwrap_or_default();
                if !primary.eq_ignore_ascii_case("en") {
                    report.record_warning(CdbWarning::LanguageNotEnglish {
                        language: global.language.as_str().to_owned(),
                    });
                }
            }
            Err(error) => record_metadata_error(&mut report, error)?,
        }

        // Metadata5 sweep: every metadata file must use the declared encoding.
        // Scope = files at the top of `global_metadata/` plus files the profile
        // recognizes as resource metadata — a `.gpkg` *data* file is legitimate
        // and must not be swept.
        let mut sweep_names = global_metadata_files;
        for logical_path in &file_logical_paths {
            if profile.is_resource_metadata(logical_path) {
                sweep_names.push(logical_path.clone());
            }
        }
        for violation in encoding_violations(
            profile.metadata_encoding(),
            sweep_names.iter().map(String::as_str),
        ) {
            report.record_violation(violation.into());
        }

        // Stage 5 — CRS: read the storage CRS and cross-check the profile's
        // single-CRS declaration (CRS3).
        match StorageCrs::read_from(&self.layout) {
            Ok(on_disk) => {
                match profile.storage_crs() {
                    Ok(declared) => {
                        if on_disk != declared {
                            report.record_violation(CdbViolation::DeclarationMismatch {
                                profile: profile.name().to_owned(),
                                class: RequirementsClass::Crs,
                                element: "storage CRS",
                                declared: declared.to_wkt(),
                                found: on_disk.to_wkt(),
                                clause: "/req/core/crs/crsStorage",
                            });
                        }
                    }
                    // A mis-declared profile CRS is a Crs-class finding.
                    Err(violation) => report.record_violation(violation.into()),
                }
                for warning in on_disk.warnings() {
                    report.record_warning(warning.into());
                }
            }
            Err(CrsError::Violation(violation)) => report.record_violation(violation.into()),
            // I/O (and any future operational variant) aborts inspection.
            Err(other) => return Err(other.into()),
        }

        // Stage 6 — Resource metadata + Links: parse each recognized record and
        // record its Metadata/Links findings.
        for logical_path in &file_logical_paths {
            if !profile.is_resource_metadata(logical_path) {
                continue;
            }
            let extension = path_extension(logical_path).map(str::to_ascii_lowercase);
            if extension.as_deref() == Some("gpkg") {
                // The core cannot parse the GeoPackage container; its encoding
                // mismatch (if any) was already recorded by the Metadata5 sweep.
                continue;
            }
            let physical = match self.resolve(logical_path) {
                Ok(path) => path,
                // A resolve failure is a naming violation the File Naming walk
                // already recorded; skip rather than abort validation.
                Err(_) => continue,
            };
            let content = fs::read_to_string(&physical).map_err(MetadataError::from)?;
            // `from_*_str` deserialize *and* validate, so a bad link or missing
            // element surfaces here as `Err(Violation(..))`.
            let parsed = match extension.as_deref() {
                Some("xml") => ResourceMetadata::from_xml_str(&content),
                _ => ResourceMetadata::from_json_str(&content),
            };
            if let Err(error) = parsed {
                record_metadata_error(&mut report, error)?;
            }
        }

        Ok(report)
    }

    /// The datastore's file hierarchy (Requirement File5).
    pub fn layout(&self) -> &DatastoreLayout {
        &self.layout
    }

    /// The datastore's physical root directory (Requirement File2).
    pub fn root(&self) -> &Path {
        self.layout.root()
    }

    /// Resolves a datastore-relative logical path to a physical path under the
    /// root, rejecting traversal that would escape it (Requirement File2-B).
    pub fn resolve(&self, logical: &str) -> Result<PathBuf, CdbError> {
        Ok(self.layout.resolve(logical)?)
    }

    /// Reads the datastore's global metadata record (Requirements
    /// Metadata1/Metadata3).
    pub fn global_metadata(&self) -> Result<GlobalMetadata, CdbError> {
        Ok(GlobalMetadata::read_from(&self.layout)?)
    }

    /// Writes (or rewrites) the global metadata record. Enforces Requirement
    /// Metadata5 — one metadata encoding per datastore — the way
    /// [`StorageCrs::write_to`] enforces the single-CRS rule (CRS3): if a
    /// global record already exists on disk in a *different* encoding, the
    /// write is refused with [`MetadataViolation::EncodingMismatch`]
    /// (`declared` = the incoming record's encoding, `found` = the on-disk
    /// file's). A same-encoding rewrite — e.g. adding a license — is allowed.
    pub fn write_global_metadata(&self, metadata: &GlobalMetadata) -> Result<PathBuf, CdbError> {
        let dir = self.layout.global_metadata_dir();
        for (extension, found) in [
            ("json", MetadataEncoding::Json),
            ("xml", MetadataEncoding::Xml),
        ] {
            if found != metadata.encoding {
                let name = format!("{GLOBAL_METADATA_STEM}.{extension}");
                if dir.join(&name).is_file() {
                    return Err(CdbError::Metadata(MetadataError::Violation(
                        MetadataViolation::EncodingMismatch {
                            file: name,
                            declared: metadata.encoding,
                            found,
                        },
                    )));
                }
            }
        }
        Ok(metadata.write_to(&self.layout)?)
    }

    /// Reads the datastore's storage CRS (Requirement CRS5).
    pub fn storage_crs(&self) -> Result<StorageCrs, CdbError> {
        Ok(StorageCrs::read_from(&self.layout)?)
    }

    /// Writes a resource (dataset) metadata record (§7.9.4.2) to
    /// `logical_path`, returning the physical file written.
    ///
    /// The record is validated first, so an invalid one is refused before any
    /// file is touched. Then Requirement Metadata5 is enforced against the
    /// path: its final-component extension must denote the datastore's
    /// declared encoding, mapped by the same table as
    /// [`crate::metadata::encoding_violations`] (`json` → JSON, `xml`/`xsd`
    /// → XML, `gpkg` → GeoPackage). A recognized extension for a *different*
    /// encoding is a [`MetadataViolation::EncodingMismatch`]; an absent or
    /// unrecognized extension is a [`MetadataViolation::Malformed`] (that
    /// mismatch variant cannot name a "no encoding" found value). A
    /// `gpkg`-declared datastore cannot be written by the core and yields
    /// [`MetadataError::UnsupportedEncoding`], as [`GlobalMetadata::write_to`]
    /// does.
    pub fn write_resource_metadata(
        &self,
        logical_path: &str,
        metadata: &ResourceMetadata,
    ) -> Result<PathBuf, CdbError> {
        metadata.validate().map_err(MetadataError::from)?;
        let declared = self.global_metadata()?.encoding;
        match path_extension(logical_path).and_then(extension_encoding) {
            Some(found) if found == declared => {}
            Some(found) => {
                return Err(CdbError::Metadata(MetadataError::Violation(
                    MetadataViolation::EncodingMismatch {
                        file: logical_path.to_owned(),
                        declared,
                        found,
                    },
                )));
            }
            None => {
                return Err(CdbError::Metadata(MetadataError::Violation(
                    MetadataViolation::Malformed {
                        reason: format!(
                            "resource metadata path {logical_path:?} must end in .{} to match \
                             the datastore's declared encoding",
                            declared.extension()
                        ),
                    },
                )));
            }
        }
        let content = match declared {
            MetadataEncoding::Json => metadata.to_json_string()?,
            MetadataEncoding::Xml => metadata.to_xml_string()?,
            MetadataEncoding::Gpkg => {
                return Err(CdbError::Metadata(MetadataError::UnsupportedEncoding(
                    MetadataEncoding::Gpkg,
                )));
            }
        };
        let path = self.resolve(logical_path)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(MetadataError::from)?;
        }
        fs::write(&path, content).map_err(MetadataError::from)?;
        Ok(path)
    }

    /// Reads a resource metadata record from `logical_path`, parsing it by the
    /// path's final-component extension (§7.9.4.2): `json` as JSON,
    /// `xml`/`xsd` as XML. Any other extension is a
    /// [`MetadataViolation::Malformed`] — the core reads only the two textual
    /// encodings.
    pub fn read_resource_metadata(&self, logical_path: &str) -> Result<ResourceMetadata, CdbError> {
        let path = self.resolve(logical_path)?;
        let content = fs::read_to_string(&path).map_err(MetadataError::from)?;
        match path_extension(logical_path).and_then(extension_encoding) {
            Some(MetadataEncoding::Json) => Ok(ResourceMetadata::from_json_str(&content)?),
            Some(MetadataEncoding::Xml) => Ok(ResourceMetadata::from_xml_str(&content)?),
            _ => Err(CdbError::Metadata(MetadataError::Violation(
                MetadataViolation::Malformed {
                    reason: format!(
                        "resource metadata path {logical_path:?} has no json or xml extension"
                    ),
                },
            ))),
        }
    }
}

/// The extension of a logical path's final component, if any. Isolating the
/// final component keeps a dot in a parent directory name from being mistaken
/// for the file's extension.
fn path_extension(logical_path: &str) -> Option<&str> {
    let file_name = logical_path.rsplit('/').next().unwrap_or(logical_path);
    split_extension(file_name).1
}

/// Maps a file extension to the metadata encoding it denotes, using the same
/// table as [`crate::metadata::encoding_violations`] (Requirement Metadata5,
/// §7.9.3.5): `json` → JSON, `xml`/`xsd` → XML, `gpkg` → GeoPackage. Any
/// other extension yields `None`.
fn extension_encoding(extension: &str) -> Option<MetadataEncoding> {
    match extension.to_ascii_lowercase().as_str() {
        "xml" | "xsd" => Some(MetadataEncoding::Xml),
        "json" => Some(MetadataEncoding::Json),
        "gpkg" => Some(MetadataEncoding::Gpkg),
        _ => None,
    }
}

/// The File Naming walk of [`CdbDatastore::validate`] (stage 3): a single
/// recursive pass below the datastore root that records each name's Name1/Name6
/// findings and, in the same sweep, collects the file inventory stages 4–6
/// reuse. Symlinks are neither followed nor recursed (`DirEntry::file_type` is a
/// no-follow probe, mirroring [`crate::hierarchy`]'s empty-folder walk), so
/// external-resource links (Permission PFile1) are safe. Non-UTF-8 names are
/// lossily decoded via [`std::ffi::OsStr::to_string_lossy`] before validation.
struct NamingWalk<'a> {
    /// The physical `global_metadata/` directory, so its top-level files can be
    /// picked out for the Metadata5 sweep.
    global_metadata_dir: &'a Path,
    /// The profile's style guide (built once), applied to every name.
    guide: &'a StyleGuide,
    /// Extensions the profile vouches for (Name7-B); their `NonSpecExtension`
    /// warnings are suppressed.
    known_extensions: &'a [String],
    /// The report findings are recorded into.
    report: &'a mut ConformanceReport,
    /// File names at the top level of `global_metadata/`.
    global_metadata_files: Vec<String>,
    /// The logical path of every file found, `/`-joined from the root.
    file_logical_paths: Vec<String>,
}

impl NamingWalk<'_> {
    /// Walks `dir`, whose logical path is `dir_logical` (empty for the root, so
    /// its children read as `/name`).
    fn walk(&mut self, dir: &Path, dir_logical: &str) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();

            // Name1/Name6: structural rules and the datastore-wide case rule.
            if let Err(violation) = self.guide.validate_component(&name) {
                self.report.record_violation(violation.into());
            }

            let child_logical = format!("{dir_logical}/{name}");
            if file_type.is_file() {
                for warning in filter_known(file_warnings(&name), self.known_extensions) {
                    self.report.record_warning(warning.into());
                }
                if dir == self.global_metadata_dir {
                    self.global_metadata_files.push(name.to_string());
                }
                self.file_logical_paths.push(child_logical);
            } else {
                for warning in component_warnings(&name) {
                    self.report.record_warning(warning.into());
                }
                // Recurse into real subdirectories only — never a
                // symlink. The reserved `versions/` journal subtree is
                // versioning's own machinery (Requirement V1, §7.14.2):
                // its `v######` directories and `manifest.<enc>` files are
                // crate-persisted under every case rule, and its archive
                // mirrors were name-checked at their live locations —
                // [`CdbDatastore::versions`] validates the journal itself
                // (parse + contiguity), so the walk does not descend.
                if file_type.is_dir()
                    && !(dir_logical.is_empty() && name == versioning::VERSIONS_DIR)
                {
                    self.walk(&entry.path(), &child_logical)?;
                }
            }
        }
        Ok(())
    }
}

/// Drops the `NonSpecExtension` warnings whose extension the profile vouches
/// for as industry standard (Requirement Name7-B), comparing case-insensitively.
fn filter_known(warnings: Vec<NamingWarning>, known: &[String]) -> Vec<NamingWarning> {
    warnings
        .into_iter()
        .filter(|warning| match warning {
            NamingWarning::NonSpecExtension { extension, .. } => !known
                .iter()
                .any(|vouched| vouched.eq_ignore_ascii_case(extension)),
            _ => true,
        })
        .collect()
}

/// Records a [`MetadataError`] surfaced while reading a metadata record as a
/// conformance finding, or returns it as an operational `Err`. A
/// [`MetadataError::Violation`] is recorded verbatim (a nested
/// [`MetadataViolation::Link`] is re-filed under Links by
/// [`ConformanceReport::record_violation`]); a serialization failure becomes a
/// [`MetadataViolation::Malformed`]. [`MetadataError::UnsupportedEncoding`] is
/// unreachable from the reader (it only reads json/xml), but is armed
/// defensively as `Malformed` so a future caller cannot make it panic. I/O — and
/// any future operational variant — is not a conformance finding and returns
/// `Err`.
fn record_metadata_error(
    report: &mut ConformanceReport,
    error: MetadataError,
) -> Result<(), CdbError> {
    match error {
        MetadataError::Violation(violation) => {
            report.record_violation(violation.into());
            Ok(())
        }
        MetadataError::Serialization(reason) => {
            report.record_violation(MetadataViolation::Malformed { reason }.into());
            Ok(())
        }
        MetadataError::UnsupportedEncoding(encoding) => {
            report.record_violation(
                MetadataViolation::Malformed {
                    reason: format!(
                        "metadata declares the {encoding} encoding the core cannot read"
                    ),
                }
                .into(),
            );
            Ok(())
        }
        other => Err(other.into()),
    }
}

/// Wraps a versioning SHALL violation for the facade's `CdbError` surface.
fn versioning_violation(violation: VersioningViolation) -> CdbError {
    CdbError::Versioning(VersioningError::Violation(violation))
}

/// Wraps a versioning-path I/O failure for the facade's `CdbError` surface.
fn versioning_io(error: io::Error) -> CdbError {
    CdbError::Versioning(VersioningError::Io(error))
}

/// Attaches an optional resource-record link to the most recently added
/// change of a pending collection (rollback inverse assembly).
fn with_record(pending: PendingCollection, record: Option<String>) -> PendingCollection {
    match record {
        Some(record) => pending.for_record(record),
        None => pending,
    }
}

/// Versioning facade (Requirements V1–V6, `/req/core/versioning*`,
/// §7.14): applying and tracking versioning collections, reading the
/// journal, and byte-level rollback. The pure model lives in
/// [`crate::versioning`]; these methods own all I/O.
impl CdbDatastore {
    /// The `versions/` journal directory (§7.14.3; reserved name).
    fn versions_dir(&self) -> PathBuf {
        self.layout.root().join(versioning::VERSIONS_DIR)
    }

    /// The immutable directory of one applied collection.
    fn version_dir(&self, id: CollectionId) -> PathBuf {
        self.versions_dir().join(id.to_string())
    }

    /// The archive-mirror path of `asset` inside a collection directory.
    fn archive_path(&self, id: CollectionId, asset: &str) -> PathBuf {
        self.version_dir(id).join(asset.trim_start_matches('/'))
    }

    /// Reads one collection's manifest in the datastore's declared
    /// encoding.
    fn read_manifest(
        &self,
        id: CollectionId,
        encoding: MetadataEncoding,
    ) -> Result<CollectionManifest, CdbError> {
        let path = self
            .version_dir(id)
            .join(format!("manifest.{}", encoding.extension()));
        let content = fs::read_to_string(&path).map_err(versioning_io)?;
        let manifest = match encoding {
            MetadataEncoding::Json => CollectionManifest::from_json_str(&content),
            MetadataEncoding::Xml => CollectionManifest::from_xml_str(&content),
            MetadataEncoding::Gpkg => {
                return Err(CdbError::Metadata(MetadataError::UnsupportedEncoding(
                    MetadataEncoding::Gpkg,
                )));
            }
        };
        manifest.map_err(CdbError::Versioning)
    }

    /// The versioning journal: every applied collection's manifest in
    /// sequence order (Requirements V1/V2, §7.14.2–.3).
    ///
    /// The journal must be contiguous from sequence 1 — a missing entry is
    /// [`VersioningViolation::ManifestSequenceGap`], because rollback's
    /// inverse chains are only sound over an unbroken journal. Directory
    /// entries that do not parse as `v######` ids are skipped (stray
    /// files are not journal entries).
    pub fn versions(&self) -> Result<Vec<CollectionManifest>, CdbError> {
        let dir = self.versions_dir();
        let mut manifests = Vec::new();
        if dir.is_dir() {
            let encoding = self.global_metadata()?.encoding;
            for entry in fs::read_dir(&dir).map_err(versioning_io)? {
                let entry = entry.map_err(versioning_io)?;
                let name = entry.file_name().to_string_lossy().into_owned();
                let Some(id) = CollectionId::parse(&name) else {
                    continue;
                };
                manifests.push(self.read_manifest(id, encoding)?);
            }
        }
        manifests.sort_by_key(|manifest| manifest.sequence);
        for (index, manifest) in manifests.iter().enumerate() {
            let expected = index as u32 + 1;
            if manifest.sequence != expected {
                return Err(versioning_violation(
                    VersioningViolation::ManifestSequenceGap {
                        expected,
                        found: manifest.sequence,
                    },
                ));
            }
        }
        Ok(manifests)
    }

    /// The state of `asset` as of `as_of` (Requirement V6, §7.14.7):
    /// replays the journal's state actions in sequence order over the
    /// manifests applied at or before `as_of`.
    pub fn state_of(&self, asset: &str, as_of: DateTime<Utc>) -> Result<Option<String>, CdbError> {
        let manifests = self.versions()?;
        Ok(versioning::state_from_manifests(
            manifests
                .iter()
                .filter(|manifest| manifest.applied <= as_of),
            asset,
        )
        .map(str::to_owned))
    }

    /// Applies a versioning collection with `applied = Utc::now()`
    /// (Requirements V1–V6). See [`CdbDatastore::apply_collection_at`].
    pub fn apply_collection(
        &self,
        pending: PendingCollection,
    ) -> Result<CollectionManifest, CdbError> {
        self.apply_collection_at(pending, Utc::now())
    }

    /// Applies a versioning collection at an explicit instant — the
    /// deterministic primitive (the seam mirrors
    /// [`DatastoreSeed::created`]).
    ///
    /// Pipeline, with **every check before any mutation**: collection
    /// invariants → journal scan for the next sequence → precondition
    /// sweep against the live tree (`Create` targets must not exist,
    /// `Replace`/`Delete`/state targets must, `ClearState` needs a
    /// current state, linked records must exist — V3-C presupposes a
    /// record to update) → archive prior bytes of `Replace`/`Delete`
    /// targets into `versions/<id>/` → mutate the live tree → refresh
    /// each linked record's `updated` (V3-C) → refresh the global
    /// `update` (V3-B) → write `manifest.<enc>` last as the commit point.
    /// The SAME `applied` instant lands in all three places (V3-A's "date
    /// and time of the collection of modification(s)").
    ///
    /// Single-writer and non-atomic: a crash mid-apply can leave mutated
    /// assets without a manifest; recovery is operator business,
    /// consistent with the crate-wide no-concurrency stance.
    pub fn apply_collection_at(
        &self,
        pending: PendingCollection,
        applied: DateTime<Utc>,
    ) -> Result<CollectionManifest, CdbError> {
        pending.validate().map_err(versioning_violation)?;
        let journal = self.versions()?;
        let sequence = journal.len() as u32 + 1;
        let id = CollectionId::from_sequence(sequence).map_err(versioning_violation)?;
        let mut global = self.global_metadata()?;
        let encoding = global.encoding;

        // Precondition sweep — nothing below may touch the tree.
        let mut physicals = Vec::with_capacity(pending.changes.len());
        for change in &pending.changes {
            let physical = self.resolve(&change.asset)?;
            match &change.op {
                PendingOp::Create { .. } => {
                    if physical.exists() {
                        return Err(versioning_violation(
                            VersioningViolation::AssetAlreadyExists {
                                asset: change.asset.clone(),
                            },
                        ));
                    }
                }
                PendingOp::Replace { .. } | PendingOp::Delete | PendingOp::SetState { .. } => {
                    if !physical.is_file() {
                        return Err(versioning_violation(VersioningViolation::AssetMissing {
                            asset: change.asset.clone(),
                        }));
                    }
                }
                PendingOp::ClearState => {
                    if !physical.is_file() {
                        return Err(versioning_violation(VersioningViolation::AssetMissing {
                            asset: change.asset.clone(),
                        }));
                    }
                    if versioning::state_from_manifests(journal.iter(), &change.asset).is_none() {
                        return Err(versioning_violation(
                            VersioningViolation::AssetStateMissing {
                                asset: change.asset.clone(),
                            },
                        ));
                    }
                }
            }
            if let Some(record) = &change.resource_record {
                let record_physical = self.resolve(record)?;
                if !record_physical.is_file() {
                    return Err(versioning_violation(
                        VersioningViolation::ResourceRecordMissing {
                            record: record.clone(),
                        },
                    ));
                }
            }
            physicals.push(physical);
        }

        // Archive phase — copies before any live-tree change.
        let version_dir = self.version_dir(id);
        fs::create_dir_all(&version_dir).map_err(versioning_io)?;
        for (change, physical) in pending.changes.iter().zip(&physicals) {
            if matches!(change.op, PendingOp::Replace { .. } | PendingOp::Delete) {
                let mirror = self.archive_path(id, &change.asset);
                if let Some(parent) = mirror.parent() {
                    fs::create_dir_all(parent).map_err(versioning_io)?;
                }
                fs::copy(physical, &mirror).map_err(versioning_io)?;
            }
        }

        // Mutate phase.
        for (change, physical) in pending.changes.iter().zip(&physicals) {
            match &change.op {
                PendingOp::Create { bytes } | PendingOp::Replace { bytes } => {
                    if let Some(parent) = physical.parent() {
                        fs::create_dir_all(parent).map_err(versioning_io)?;
                    }
                    fs::write(physical, bytes).map_err(versioning_io)?;
                }
                PendingOp::Delete => {
                    fs::remove_file(physical).map_err(versioning_io)?;
                }
                PendingOp::SetState { .. } | PendingOp::ClearState => {}
            }
        }

        // V3-C: refresh each linked record's `updated` element.
        for change in &pending.changes {
            if let Some(record) = &change.resource_record {
                let mut resource = self.read_resource_metadata(record)?;
                resource.updated = Some(applied);
                self.write_resource_metadata(record, &resource)?;
            }
        }

        // V3-B: refresh the global `update` element.
        global.update = Some(applied);
        self.write_global_metadata(&global)?;

        // Manifest last — the commit point.
        let changes = pending
            .changes
            .iter()
            .map(|change| {
                let (action, state, archived) = match &change.op {
                    PendingOp::Create { .. } => (ChangeAction::Created, None, false),
                    PendingOp::Replace { .. } => (ChangeAction::Replaced, None, true),
                    PendingOp::Delete => (ChangeAction::Deleted, None, true),
                    PendingOp::SetState { state } => {
                        (ChangeAction::StateSet, Some(state.clone()), false)
                    }
                    PendingOp::ClearState => (ChangeAction::StateCleared, None, false),
                };
                let prior_state = match &change.op {
                    PendingOp::SetState { .. } | PendingOp::ClearState => {
                        versioning::state_from_manifests(journal.iter(), &change.asset)
                            .map(str::to_owned)
                    }
                    _ => None,
                };
                ChangeRecord {
                    asset: change.asset.clone(),
                    action,
                    state,
                    resource_record: change.resource_record.clone(),
                    archived,
                    prior_state,
                }
            })
            .collect();
        let manifest = CollectionManifest {
            id,
            sequence,
            applied,
            description: pending.description.clone(),
            changes,
        };
        let content = match encoding {
            MetadataEncoding::Json => manifest.to_json_string(),
            MetadataEncoding::Xml => manifest.to_xml_string(),
            MetadataEncoding::Gpkg => {
                return Err(CdbError::Metadata(MetadataError::UnsupportedEncoding(
                    MetadataEncoding::Gpkg,
                )));
            }
        }
        .map_err(CdbError::Versioning)?;
        fs::write(
            version_dir.join(format!("manifest.{}", encoding.extension())),
            content,
        )
        .map_err(versioning_io)?;
        Ok(manifest)
    }

    /// Builds and applies the inverse of `target` (no latest-check): the
    /// shared engine of [`CdbDatastore::rollback_collection_at`] (which
    /// guards) and [`CdbDatastore::rollback_to_at`] (which processes the
    /// tail in strict descending order — exactly the order under which
    /// every inverse's preconditions hold).
    fn apply_inverse_of(
        &self,
        target: &CollectionManifest,
        applied: DateTime<Utc>,
    ) -> Result<CollectionManifest, CdbError> {
        let mut pending =
            PendingCollection::new().description(format!("rollback of {}", target.id));
        for op in target.inverse_ops() {
            pending = match op {
                InverseOp::Delete {
                    asset,
                    resource_record,
                } => with_record(pending.delete(asset), resource_record),
                InverseOp::RestoreReplace {
                    asset,
                    resource_record,
                } => {
                    let bytes =
                        fs::read(self.archive_path(target.id, &asset)).map_err(versioning_io)?;
                    with_record(pending.replace(asset, bytes), resource_record)
                }
                InverseOp::RestoreCreate {
                    asset,
                    resource_record,
                } => {
                    let bytes =
                        fs::read(self.archive_path(target.id, &asset)).map_err(versioning_io)?;
                    with_record(pending.create(asset, bytes), resource_record)
                }
                InverseOp::SetState {
                    asset,
                    state,
                    resource_record,
                } => with_record(pending.set_state(asset, state), resource_record),
                InverseOp::ClearState {
                    asset,
                    resource_record,
                } => with_record(pending.clear_state(asset), resource_record),
            };
        }
        self.apply_collection_at(pending, applied)
    }

    /// Rolls back the LATEST applied collection with
    /// `applied = Utc::now()`. See
    /// [`CdbDatastore::rollback_collection_at`].
    pub fn rollback_collection(&self, id: CollectionId) -> Result<CollectionManifest, CdbError> {
        self.rollback_collection_at(id, Utc::now())
    }

    /// Rolls back one collection at an explicit instant — restoring every
    /// changed asset's prior bytes and states from the collection's
    /// archive and manifest (§7.14 intro's "rollback … a given asset",
    /// realized beyond the boxes).
    ///
    /// Only the LATEST collection may be rolled back directly
    /// ([`VersioningViolation::NotLatestCollection`] otherwise): undoing
    /// an older collection beneath newer work would restore stale bytes.
    /// The rollback is itself an applied collection — journaled,
    /// timestamped (V3), and rollbackable.
    pub fn rollback_collection_at(
        &self,
        id: CollectionId,
        applied: DateTime<Utc>,
    ) -> Result<CollectionManifest, CdbError> {
        let journal = self.versions()?;
        let Some(target) = journal.iter().find(|manifest| manifest.id == id) else {
            return Err(versioning_violation(
                VersioningViolation::UnknownCollection { id: id.to_string() },
            ));
        };
        let Some(latest) = journal.last() else {
            return Err(versioning_violation(
                VersioningViolation::UnknownCollection { id: id.to_string() },
            ));
        };
        if latest.id != id {
            return Err(versioning_violation(
                VersioningViolation::NotLatestCollection {
                    id: id.to_string(),
                    latest: latest.id.to_string(),
                },
            ));
        }
        self.apply_inverse_of(target, applied)
    }

    /// Rolls back the whole datastore to its content as of collection
    /// `id` with `applied = Utc::now()`. See
    /// [`CdbDatastore::rollback_to_at`].
    pub fn rollback_to(&self, id: CollectionId) -> Result<Vec<CollectionManifest>, CdbError> {
        self.rollback_to_at(id, Utc::now())
    }

    /// Rolls back the whole datastore to its content as of collection
    /// `id` (§7.14 intro's "rollback to previous versions of the entire
    /// datastore"): snapshots the collections after `id` at entry and
    /// applies their inverses in strict DESCENDING sequence order — the
    /// order under which each inverse's preconditions hold, since
    /// undoing sequence `n` restores exactly the content sequence `n−1`
    /// changed. Every inverse shares the one `applied` instant and is
    /// appended to the journal; the returned manifests are in application
    /// order. Rolling back to the latest collection is a no-op returning
    /// an empty list.
    pub fn rollback_to_at(
        &self,
        id: CollectionId,
        applied: DateTime<Utc>,
    ) -> Result<Vec<CollectionManifest>, CdbError> {
        let journal = self.versions()?;
        if !journal.iter().any(|manifest| manifest.id == id) {
            return Err(versioning_violation(
                VersioningViolation::UnknownCollection { id: id.to_string() },
            ));
        }
        let tail: Vec<&CollectionManifest> = journal
            .iter()
            .filter(|manifest| manifest.sequence > id.sequence())
            .collect();
        let mut applied_manifests = Vec::with_capacity(tail.len());
        for target in tail.iter().rev() {
            applied_manifests.push(self.apply_inverse_of(target, applied)?);
        }
        Ok(applied_manifests)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hierarchy::HierarchyError;
    use crate::metadata::{MetadataStandard, UnitOfMeasure};
    use crate::naming::{CaseRule, StyleGuide};
    use crate::profiles::{SimulationProfile, StorageTechnology};
    use tempfile::tempdir;

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

    /// Requirements File1/File6, Metadata1, CRS5 (§7.5.2/§7.5.7/§7.9.3.1/
    /// §7.3.1.4): `create` materializes the datastore root with a
    /// `global_metadata/` folder holding both the global metadata record and
    /// the storage-CRS record.
    #[test]
    fn req_core_file_structure_create_builds_root_metadata_and_crs() {
        let tmp = tempdir().unwrap();
        let store = CdbDatastore::create(
            tmp.path(),
            &SimulationProfile::json(),
            DatastoreSeed::new("doi:cdb.demo", "Demo", "A demonstration datastore", "CAE"),
        )
        .unwrap();

        assert!(store.root().ends_with("cdb"));
        let global = store.root().join("global_metadata");
        assert!(global.join("global_metadata.json").is_file());
        assert!(global.join("crs.wkt").is_file());
    }

    /// Requirements Metadata2/4/5/8 + §7.9.4.1: the global record `create`
    /// writes merges the seed's four identity elements with the profile's
    /// policy — language, standard, encoding, and unit of measure.
    #[test]
    fn create_composes_seed_identity_and_profile_policy() {
        let tmp = tempdir().unwrap();
        let store = CdbDatastore::create(
            tmp.path(),
            &SimulationProfile::json(),
            DatastoreSeed::new(
                "doi:cdb.demo",
                "Demo title",
                "Demo description",
                "contact@example.test",
            ),
        )
        .unwrap();

        let global = store.global_metadata().unwrap();
        assert_eq!(global.id, "doi:cdb.demo");
        assert_eq!(global.title, "Demo title");
        assert_eq!(global.description, "Demo description");
        assert_eq!(global.contact_point, "contact@example.test");
        assert_eq!(global.language.as_str(), "en");
        assert_eq!(global.metadata_standard, MetadataStandard::Dcat);
        assert_eq!(global.encoding, MetadataEncoding::Json);
        assert_eq!(global.uom, UnitOfMeasure::Meters);
    }

    /// §7.9.4.1: the four identity elements are mandatory. An empty `ID` fails
    /// `create` at policy time — the global-metadata builder rejects it before
    /// any directory is created (policy precedes the first disk write).
    #[test]
    fn create_rejects_empty_seed_identity() {
        let tmp = tempdir().unwrap();
        let result = CdbDatastore::create(
            tmp.path(),
            &SimulationProfile::json(),
            DatastoreSeed::new("", "Title", "Description", "contact"),
        );

        assert!(matches!(
            result,
            Err(CdbError::Metadata(MetadataError::Violation(
                MetadataViolation::MissingElement { .. }
            )))
        ));
        assert!(
            !tmp.path().join("cdb").exists(),
            "no datastore directory must be created when policy fails"
        );
    }

    /// Requirement File3 (§7.5.4): `open` of a path that does not exist is a
    /// hierarchy `RootMissing` error.
    #[test]
    fn open_missing_root_errors() {
        let tmp = tempdir().unwrap();
        let result = CdbDatastore::open(tmp.path().join("does-not-exist"));
        assert!(matches!(
            result,
            Err(CdbError::Hierarchy(HierarchyError::RootMissing(_)))
        ));
    }

    /// Requirement Metadata5 (§7.9.3.5): one metadata encoding per datastore.
    /// `write_global_metadata` refuses a record whose encoding differs from
    /// the on-disk global record's, but a same-encoding rewrite (here, adding
    /// a license) succeeds and reads back equal.
    #[test]
    fn req_core_metadata_encoding_write_global_metadata_refuses_switch() {
        let tmp = tempdir().unwrap();
        let store = CdbDatastore::create(
            tmp.path(),
            &SimulationProfile::json(),
            DatastoreSeed::new("id", "Title", "Description", "contact"),
        )
        .unwrap();

        // Switching the datastore to XML is refused (Metadata5).
        let mut switched = store.global_metadata().unwrap();
        switched.encoding = MetadataEncoding::Xml;
        assert!(matches!(
            store.write_global_metadata(&switched),
            Err(CdbError::Metadata(MetadataError::Violation(
                MetadataViolation::EncodingMismatch { .. }
            )))
        ));

        // A same-encoding rewrite succeeds and round-trips.
        let mut licensed = store.global_metadata().unwrap();
        licensed.license = Some("CC-BY-4.0".to_owned());
        store.write_global_metadata(&licensed).unwrap();
        assert_eq!(store.global_metadata().unwrap(), licensed);
    }

    /// Requirement Metadata5 + §7.9.4.2: a valid resource-metadata record
    /// writes to a `.json` path on a JSON datastore and reads back equal; the
    /// same record is refused when the path's `.xml` extension contradicts the
    /// declared encoding; and an invalid record is refused before any file is
    /// written.
    #[test]
    fn resource_metadata_roundtrip_and_declared_encoding_enforced() {
        let tmp = tempdir().unwrap();
        let store = CdbDatastore::create(
            tmp.path(),
            &SimulationProfile::json(),
            DatastoreSeed::new("id", "Title", "Description", "contact"),
        )
        .unwrap();

        let record = ResourceMetadata::new("roads", "RoadNetwork", "The road network");
        let path = store
            .write_resource_metadata("/Tiles/metadata/RoadNetwork.json", &record)
            .unwrap();
        assert!(path.is_file());
        assert!(path.starts_with(store.root()));
        assert_eq!(
            store
                .read_resource_metadata("/Tiles/metadata/RoadNetwork.json")
                .unwrap(),
            record
        );

        // The path's extension must denote the datastore's JSON encoding.
        assert!(matches!(
            store.write_resource_metadata("/Tiles/metadata/RoadNetwork.xml", &record),
            Err(CdbError::Metadata(MetadataError::Violation(
                MetadataViolation::EncodingMismatch { .. }
            )))
        ));

        // An invalid record (empty title) is refused before any file exists.
        let invalid = ResourceMetadata::new("bad", "", "Missing title");
        let invalid_path = "/Tiles/metadata/Bad.json";
        assert!(matches!(
            store.write_resource_metadata(invalid_path, &invalid),
            Err(CdbError::Metadata(MetadataError::Violation(
                MetadataViolation::MissingElement { .. }
            )))
        ));
        assert!(!store.resolve(invalid_path).unwrap().exists());
    }

    /// Annex A `/conf/minimal-core` — all five mandatory classes: a datastore
    /// freshly created from `SimulationProfile::json()` validates clean —
    /// conformant, every mandatory class passes, and (crucially) every class
    /// has **zero** warnings. Zero warnings proves both naming wrinkles are in
    /// force: the `crs` name is auto-reserved (so `crs.wkt` escapes the case
    /// rule) and the profile vouches for the `.wkt` extension (Name7-B, so its
    /// `NonSpecExtension` warning is suppressed). A regression in either
    /// surfaces here as a spurious warning.
    #[test]
    fn validate_fresh_datastore_conformant_no_warnings() {
        let tmp = tempdir().unwrap();
        let profile = SimulationProfile::json();
        let store = CdbDatastore::create(
            tmp.path(),
            &profile,
            DatastoreSeed::new("id", "Title", "Description", "contact"),
        )
        .unwrap();

        let report = store.validate(&profile).unwrap();
        assert!(report.is_conformant(), "{report}");
        for class in RequirementsClass::MANDATORY {
            assert!(report.class_passed(class), "{class}: {report}");
            assert!(
                report.warnings(class).is_empty(),
                "{class} should have no warnings: {report}"
            );
        }
    }

    /// Requirement CRS3 (§7.3.1.2 `/req/core/crs/crsStorage`): when the
    /// on-disk storage CRS differs from the one the profile declares, the Crs
    /// class fails with a `DeclarationMismatch` for the storage CRS. Here the
    /// datastore's `crs.wkt` is overwritten with a valid but *foreign*
    /// geographic CRS (NAD83, EPSG:4269), which parses cleanly yet is not the
    /// profile's WGS-84.
    #[test]
    fn req_core_crs_storage_declaration_mismatch_detected() {
        const NAD83_WKT: &str = r#"GEOGCRS["NAD83",
  DATUM["North American Datum 1983",
    ELLIPSOID["GRS 1980",6378137,298.257222101,LENGTHUNIT["metre",1.0]]],
  CS[ellipsoidal,2],
    AXIS["latitude",north,ORDER[1]],
    AXIS["longitude",east,ORDER[2]],
    ANGLEUNIT["degree",0.0174532925199433],
  ID["EPSG",4269]]"#;

        let tmp = tempdir().unwrap();
        let profile = SimulationProfile::json();
        let store = CdbDatastore::create(
            tmp.path(),
            &profile,
            DatastoreSeed::new("id", "Title", "Description", "contact"),
        )
        .unwrap();

        // Sanity: the foreign CRS parses as a valid storage CRS.
        StorageCrs::from_wkt(NAD83_WKT).unwrap();
        // Overwrite the storage-CRS record with the foreign CRS.
        fs::write(
            store.layout().global_metadata_dir().join("crs.wkt"),
            NAD83_WKT,
        )
        .unwrap();

        let report = store.validate(&profile).unwrap();
        assert!(!report.class_passed(RequirementsClass::Crs), "{report}");
        assert!(
            report
                .violations(RequirementsClass::Crs)
                .iter()
                .any(|v| matches!(
                    v,
                    CdbViolation::DeclarationMismatch {
                        element: "storage CRS",
                        ..
                    }
                )),
            "{report}"
        );
    }

    /// Requirements Metadata2/5/8 (§7.9.3): when the on-disk global metadata
    /// declares a different standard or unit of measure than the profile, the
    /// Metadata class fails with a `DeclarationMismatch` per differing element.
    /// The encoding is left as JSON (matching the profile), so no encoding
    /// mismatch is reported. The doctored record is rewritten through the
    /// module API, bypassing the facade's encoding guard.
    #[test]
    fn req_core_metadata_declaration_mismatch_detected() {
        let tmp = tempdir().unwrap();
        let profile = SimulationProfile::json();
        let store = CdbDatastore::create(
            tmp.path(),
            &profile,
            DatastoreSeed::new("id", "Title", "Description", "contact"),
        )
        .unwrap();

        let mut global = store.global_metadata().unwrap();
        global.metadata_standard = MetadataStandard::Iso19115v2019;
        global.uom = UnitOfMeasure::Feet;
        global.write_to(store.layout()).unwrap();

        let report = store.validate(&profile).unwrap();
        assert!(
            !report.class_passed(RequirementsClass::Metadata),
            "{report}"
        );
        let elements: Vec<&str> = report
            .violations(RequirementsClass::Metadata)
            .iter()
            .filter_map(|v| match v {
                CdbViolation::DeclarationMismatch { element, .. } => Some(*element),
                _ => None,
            })
            .collect();
        assert!(elements.contains(&"metadata standard"), "{report}");
        assert!(elements.contains(&"uom"), "{report}");
    }

    /// Recommendation Name3-B (§7.4.2 `/req/core/name-language` B): a datastore
    /// whose declared language is not English earns a `LanguageNotEnglish`
    /// warning under File Naming — but a SHOULD finding never fails validation,
    /// so the class still passes and the report is still conformant. The
    /// wrapper profile delegates every policy to the simulation profile except
    /// its style-guide language (French) and its name, proving the trait is
    /// implementable outside the `profiles` module.
    #[test]
    fn rec_core_name_lang_non_english_warns() {
        struct FrenchProfile(SimulationProfile);
        impl ApplicationProfile for FrenchProfile {
            fn name(&self) -> &str {
                "simulation-fr"
            }
            fn style_guide(&self) -> StyleGuide {
                let mut guide = StyleGuide::new(CaseRule::PascalCase, "fr");
                guide.reserve_name("metadata");
                guide
            }
            fn storage_crs(&self) -> Result<StorageCrs, CrsViolation> {
                self.0.storage_crs()
            }
            fn metadata_standard(&self) -> MetadataStandard {
                self.0.metadata_standard()
            }
            fn metadata_encoding(&self) -> MetadataEncoding {
                self.0.metadata_encoding()
            }
            fn uom(&self) -> UnitOfMeasure {
                self.0.uom()
            }
            fn storage_technology(&self) -> StorageTechnology {
                self.0.storage_technology()
            }
            fn conformance_classes(&self) -> Vec<RequirementsClass> {
                self.0.conformance_classes()
            }
            fn is_resource_metadata(&self, logical_path: &str) -> bool {
                self.0.is_resource_metadata(logical_path)
            }
            fn known_extensions(&self) -> Vec<String> {
                self.0.known_extensions()
            }
        }

        let tmp = tempdir().unwrap();
        let profile = FrenchProfile(SimulationProfile::json());
        let store = CdbDatastore::create(
            tmp.path(),
            &profile,
            DatastoreSeed::new("id", "Titre", "Description", "contact"),
        )
        .unwrap();

        let report = store.validate(&profile).unwrap();
        assert!(report.is_conformant(), "{report}");
        assert!(
            report.class_passed(RequirementsClass::FileNaming),
            "{report}"
        );
        assert!(
            report.warnings(RequirementsClass::FileNaming).contains(
                &CdbWarning::LanguageNotEnglish {
                    language: "fr".to_owned(),
                }
            ),
            "{report}"
        );
    }

    /// Recommendation RFile1 (§7.5.6): `validate` surfaces the hierarchy
    /// walk's SHOULD findings. A datastore whose root is not named `cdb`
    /// (built here directly through the module APIs) earns a `RootNameNotCdb`
    /// warning under File Structure, yet the class still passes and the report
    /// is still conformant.
    #[test]
    fn validate_surfaces_hierarchy_walk_warnings() {
        use crate::metadata::LanguageTag;
        use crate::profiles::simulation::WGS84_2D_WKT;

        let tmp = tempdir().unwrap();
        let layout = DatastoreLayout::create_named(tmp.path(), "MyStore").unwrap();
        GlobalMetadata::builder()
            .id("id")
            .title("Title")
            .description("Description")
            .contact_point("contact")
            .created(Utc::now())
            .language(LanguageTag::new("en").unwrap())
            .standard(MetadataStandard::Dcat)
            .encoding(MetadataEncoding::Json)
            .uom(UnitOfMeasure::Meters)
            .build()
            .unwrap()
            .write_to(&layout)
            .unwrap();
        StorageCrs::from_wkt(WGS84_2D_WKT)
            .unwrap()
            .write_to(&layout)
            .unwrap();

        let store = CdbDatastore::open(layout.root()).unwrap();
        let report = store.validate(&SimulationProfile::json()).unwrap();

        assert!(report.is_conformant(), "{report}");
        assert!(
            report.class_passed(RequirementsClass::FileStructure),
            "{report}"
        );
        assert!(
            report
                .warnings(RequirementsClass::FileStructure)
                .contains(&CdbWarning::Hierarchy(HierarchyWarning::RootNameNotCdb {
                    name: "MyStore".to_owned(),
                })),
            "{report}"
        );
    }

    fn versioned_store() -> (tempfile::TempDir, CdbDatastore) {
        let tmp = tempdir().unwrap();
        let store = CdbDatastore::create(
            tmp.path(),
            &SimulationProfile::json(),
            DatastoreSeed::new(
                "doi:cdb.versioning",
                "Versioned",
                "Versioning fixture",
                "ops",
            ),
        )
        .unwrap();
        (tmp, store)
    }

    fn ts(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }

    /// Requirements V1/V4-A req/core/versioning[-functions] (§7.14.2/.5) —
    /// applying a create collection writes the asset, journals a manifest
    /// under `versions/v000001/`, and records the change.
    #[test]
    fn req_core_versioning_functions_create_asset() {
        let (_tmp, store) = versioned_store();
        let manifest = store
            .apply_collection_at(
                PendingCollection::new()
                    .description("initial roads")
                    .create("/Tiles/RoadNetwork.gpkg", *b"road-bytes"),
                ts("2026-08-30T10:00:00Z"),
            )
            .unwrap();
        assert_eq!(manifest.sequence, 1);
        assert_eq!(manifest.id.to_string(), "v000001");
        assert_eq!(manifest.changes.len(), 1);
        assert_eq!(manifest.changes[0].action, ChangeAction::Created);
        assert!(!manifest.changes[0].archived);
        let live = store.resolve("/Tiles/RoadNetwork.gpkg").unwrap();
        assert_eq!(fs::read(live).unwrap(), b"road-bytes");
        assert!(
            store
                .root()
                .join("versions/v000001/manifest.json")
                .is_file()
        );
        let journal = store.versions().unwrap();
        assert_eq!(journal.len(), 1);
        assert_eq!(journal[0], manifest);
    }

    /// Requirements V4-C/V5 req/core/versioning-file-replacement (§7.14.5–
    /// .6) — replacement is byte-faithful for binary content and the prior
    /// bytes are archived in the collection's directory.
    #[test]
    fn req_core_versioning_file_replacement_byte_faithful() {
        let (_tmp, store) = versioned_store();
        let original: Vec<u8> = vec![0, 159, 146, 150, 255, 1, 2, 3];
        let replacement: Vec<u8> = vec![255, 0, 128, 7];
        store
            .apply_collection_at(
                PendingCollection::new().create("/Tiles/Elevation.tif", original.clone()),
                ts("2026-08-30T10:00:00Z"),
            )
            .unwrap();
        let manifest = store
            .apply_collection_at(
                PendingCollection::new().replace("/Tiles/Elevation.tif", replacement.clone()),
                ts("2026-08-30T11:00:00Z"),
            )
            .unwrap();
        assert_eq!(manifest.changes[0].action, ChangeAction::Replaced);
        assert!(manifest.changes[0].archived);
        let live = store.resolve("/Tiles/Elevation.tif").unwrap();
        assert_eq!(fs::read(live).unwrap(), replacement);
        let archived = store.root().join("versions/v000002/Tiles/Elevation.tif");
        assert_eq!(fs::read(archived).unwrap(), original);
    }

    /// Requirement V4-B req/core/versioning-functions (§7.14.5) — deleting
    /// removes the live asset and archives its prior bytes.
    #[test]
    fn req_core_versioning_functions_delete_asset() {
        let (_tmp, store) = versioned_store();
        store
            .apply_collection_at(
                PendingCollection::new().create("/Tiles/Buildings.gpkg", *b"buildings"),
                ts("2026-08-30T10:00:00Z"),
            )
            .unwrap();
        let manifest = store
            .apply_collection_at(
                PendingCollection::new().delete("/Tiles/Buildings.gpkg"),
                ts("2026-08-30T11:00:00Z"),
            )
            .unwrap();
        assert_eq!(manifest.changes[0].action, ChangeAction::Deleted);
        assert!(manifest.changes[0].archived);
        assert!(!store.resolve("/Tiles/Buildings.gpkg").unwrap().exists());
        let archived = store.root().join("versions/v000002/Tiles/Buildings.gpkg");
        assert_eq!(fs::read(archived).unwrap(), b"buildings");
    }

    /// Requirement V3 req/core/versioning-metadata (§7.14.4) — one instant
    /// lands in all three places: the manifest's `applied` (V3-A), the
    /// global `update` (V3-B), and the linked record's `updated` (V3-C).
    #[test]
    fn req_core_versioning_metadata_timestamps_three_way() {
        let (_tmp, store) = versioned_store();
        store
            .write_resource_metadata(
                "/Tiles/metadata/RoadNetwork.json",
                &ResourceMetadata::new("RoadNetwork", "Road Network", "Roads"),
            )
            .unwrap();
        let applied = ts("2026-08-30T12:34:56Z");
        let manifest = store
            .apply_collection_at(
                PendingCollection::new()
                    .create("/Tiles/RoadNetwork.gpkg", *b"road-bytes")
                    .for_record("/Tiles/metadata/RoadNetwork.json"),
                applied,
            )
            .unwrap();
        assert_eq!(manifest.applied, applied);
        assert_eq!(store.global_metadata().unwrap().update, Some(applied));
        let record = store
            .read_resource_metadata("/Tiles/metadata/RoadNetwork.json")
            .unwrap();
        assert_eq!(record.updated, Some(applied));
        assert_eq!(
            manifest.changes[0].resource_record.as_deref(),
            Some("/Tiles/metadata/RoadNetwork.json")
        );
    }

    /// Requirement Topo6-style pre-mutation discipline for §7.14: every
    /// precondition failure leaves the tree and journal untouched.
    #[test]
    fn req_core_versioning_apply_preconditions_pre_mutation() {
        let (_tmp, store) = versioned_store();
        store
            .apply_collection_at(
                PendingCollection::new().create("/Tiles/RoadNetwork.gpkg", *b"road-bytes"),
                ts("2026-08-30T10:00:00Z"),
            )
            .unwrap();

        type Expected = fn(&VersioningViolation) -> bool;
        let cases: Vec<(PendingCollection, Expected)> = vec![
            (
                PendingCollection::new().create("/Tiles/RoadNetwork.gpkg", *b"xx"),
                |violation| matches!(violation, VersioningViolation::AssetAlreadyExists { .. }),
            ),
            (
                PendingCollection::new().replace("/Tiles/Missing.gpkg", *b"xx"),
                |violation| matches!(violation, VersioningViolation::AssetMissing { .. }),
            ),
            (
                PendingCollection::new().delete("/Tiles/Missing.gpkg"),
                |violation| matches!(violation, VersioningViolation::AssetMissing { .. }),
            ),
            (
                PendingCollection::new().set_state("/Tiles/Missing.gpkg", "closed"),
                |violation| matches!(violation, VersioningViolation::AssetMissing { .. }),
            ),
            (
                PendingCollection::new().clear_state("/Tiles/RoadNetwork.gpkg"),
                |violation| matches!(violation, VersioningViolation::AssetStateMissing { .. }),
            ),
            (
                PendingCollection::new()
                    .create("/Tiles/New.gpkg", *b"xx")
                    .for_record("/Tiles/metadata/Missing.json"),
                |violation| matches!(violation, VersioningViolation::ResourceRecordMissing { .. }),
            ),
        ];
        for (pending, expected) in cases {
            match store.apply_collection_at(pending, ts("2026-08-30T11:00:00Z")) {
                Err(CdbError::Versioning(VersioningError::Violation(violation))) => {
                    assert!(expected(&violation), "unexpected violation: {violation}");
                }
                other => panic!("expected a versioning violation, got {other:?}"),
            }
        }
        assert_eq!(store.versions().unwrap().len(), 1, "journal untouched");
        let live = store.resolve("/Tiles/RoadNetwork.gpkg").unwrap();
        assert_eq!(fs::read(live).unwrap(), b"road-bytes", "tree untouched");
        assert!(!store.root().join("versions/v000002").exists());
    }

    /// Requirement V6 req/core/versioning-transitory (§7.14.7) — state
    /// set/clear round-trips through the journal with recorded priors, and
    /// the as-of view replays the timeline.
    #[test]
    fn req_core_versioning_transitory_set_and_clear_state() {
        let (_tmp, store) = versioned_store();
        store
            .apply_collection_at(
                PendingCollection::new().create("/Tiles/RoadNetwork.gpkg", *b"road-bytes"),
                ts("2026-08-30T10:00:00Z"),
            )
            .unwrap();
        let set = store
            .apply_collection_at(
                PendingCollection::new().set_state("/Tiles/RoadNetwork.gpkg", "closed"),
                ts("2026-08-30T11:00:00Z"),
            )
            .unwrap();
        assert_eq!(set.changes[0].action, ChangeAction::StateSet);
        assert_eq!(set.changes[0].state.as_deref(), Some("closed"));
        assert_eq!(
            set.changes[0].prior_state, None,
            "first-ever set has no prior"
        );
        let cleared = store
            .apply_collection_at(
                PendingCollection::new().clear_state("/Tiles/RoadNetwork.gpkg"),
                ts("2026-08-30T12:00:00Z"),
            )
            .unwrap();
        assert_eq!(cleared.changes[0].action, ChangeAction::StateCleared);
        assert_eq!(cleared.changes[0].prior_state.as_deref(), Some("closed"));
        assert_eq!(
            store
                .state_of("/Tiles/RoadNetwork.gpkg", ts("2026-08-30T10:30:00Z"))
                .unwrap(),
            None
        );
        assert_eq!(
            store
                .state_of("/Tiles/RoadNetwork.gpkg", ts("2026-08-30T11:30:00Z"))
                .unwrap(),
            Some("closed".to_owned())
        );
        assert_eq!(
            store
                .state_of("/Tiles/RoadNetwork.gpkg", ts("2026-08-30T12:30:00Z"))
                .unwrap(),
            None
        );
    }

    /// Journal integrity (§7.14.3) — a removed collection directory is a
    /// contiguity violation, and stray entries in `versions/` are skipped.
    #[test]
    fn req_core_versioning_versions_contiguity_and_strays() {
        let (_tmp, store) = versioned_store();
        store
            .apply_collection_at(
                PendingCollection::new().create("/Tiles/A.gpkg", *b"aa"),
                ts("2026-08-30T10:00:00Z"),
            )
            .unwrap();
        store
            .apply_collection_at(
                PendingCollection::new().create("/Tiles/B.gpkg", *b"bb"),
                ts("2026-08-30T11:00:00Z"),
            )
            .unwrap();
        fs::write(store.root().join("versions/readme.txt"), "stray").unwrap();
        fs::create_dir_all(store.root().join("versions/scratch")).unwrap();
        assert_eq!(store.versions().unwrap().len(), 2, "strays are skipped");
        fs::remove_dir_all(store.root().join("versions/v000001")).unwrap();
        assert!(matches!(
            store.versions(),
            Err(CdbError::Versioning(VersioningError::Violation(
                VersioningViolation::ManifestSequenceGap {
                    expected: 1,
                    found: 2,
                }
            )))
        ));
    }

    /// Rollback safety (doc-noted constraint) — only the latest collection
    /// rolls back directly; unknown targets are rejected.
    #[test]
    fn req_core_versioning_rollback_latest_only() {
        let (_tmp, store) = versioned_store();
        store
            .apply_collection_at(
                PendingCollection::new().create("/Tiles/A.gpkg", *b"aa"),
                ts("2026-08-30T10:00:00Z"),
            )
            .unwrap();
        store
            .apply_collection_at(
                PendingCollection::new().create("/Tiles/B.gpkg", *b"bb"),
                ts("2026-08-30T11:00:00Z"),
            )
            .unwrap();
        let first = CollectionId::parse("v000001").unwrap();
        assert!(matches!(
            store.rollback_collection_at(first, ts("2026-08-30T12:00:00Z")),
            Err(CdbError::Versioning(VersioningError::Violation(
                VersioningViolation::NotLatestCollection { .. }
            )))
        ));
        let unknown = CollectionId::parse("v000009").unwrap();
        assert!(matches!(
            store.rollback_collection_at(unknown, ts("2026-08-30T12:00:00Z")),
            Err(CdbError::Versioning(VersioningError::Violation(
                VersioningViolation::UnknownCollection { .. }
            )))
        ));
    }

    /// §7.14 intro's "rollback … a given asset" — rolling back the latest
    /// collection restores replaced bytes and deleted files from the
    /// archive, as a new journaled collection.
    #[test]
    fn req_core_versioning_rollback_collection_restores_bytes() {
        let (_tmp, store) = versioned_store();
        store
            .apply_collection_at(
                PendingCollection::new()
                    .create("/Tiles/A.gpkg", *b"aa-original")
                    .create("/Tiles/B.gpkg", *b"bb-original"),
                ts("2026-08-30T10:00:00Z"),
            )
            .unwrap();
        store
            .apply_collection_at(
                PendingCollection::new()
                    .replace("/Tiles/A.gpkg", *b"aa-changed")
                    .delete("/Tiles/B.gpkg"),
                ts("2026-08-30T11:00:00Z"),
            )
            .unwrap();
        let second = CollectionId::parse("v000002").unwrap();
        let inverse = store
            .rollback_collection_at(second, ts("2026-08-30T12:00:00Z"))
            .unwrap();
        assert_eq!(inverse.sequence, 3);
        assert_eq!(inverse.description.as_deref(), Some("rollback of v000002"));
        assert_eq!(inverse.changes[0].action, ChangeAction::Replaced);
        assert_eq!(inverse.changes[1].action, ChangeAction::Created);
        let a = store.resolve("/Tiles/A.gpkg").unwrap();
        assert_eq!(fs::read(a).unwrap(), b"aa-original");
        let b = store.resolve("/Tiles/B.gpkg").unwrap();
        assert_eq!(fs::read(b).unwrap(), b"bb-original");
        assert_eq!(store.versions().unwrap().len(), 3);
    }

    /// §7.14 intro's "rollback to previous versions of the entire
    /// datastore" — rollback_to snapshots the tail and applies inverses in
    /// descending order, restoring the content as of the target.
    #[test]
    fn req_core_versioning_rollback_to_restores_point() {
        let (_tmp, store) = versioned_store();
        store
            .apply_collection_at(
                PendingCollection::new().create("/Tiles/A.gpkg", *b"a-v1"),
                ts("2026-08-30T10:00:00Z"),
            )
            .unwrap();
        store
            .apply_collection_at(
                PendingCollection::new().replace("/Tiles/A.gpkg", *b"a-v2"),
                ts("2026-08-30T11:00:00Z"),
            )
            .unwrap();
        store
            .apply_collection_at(
                PendingCollection::new()
                    .replace("/Tiles/A.gpkg", *b"a-v3")
                    .create("/Tiles/B.gpkg", *b"b-v3"),
                ts("2026-08-30T12:00:00Z"),
            )
            .unwrap();
        let target = CollectionId::parse("v000001").unwrap();
        let applied = ts("2026-08-30T13:00:00Z");
        let inverses = store.rollback_to_at(target, applied).unwrap();
        assert_eq!(inverses.len(), 2);
        assert_eq!(inverses[0].sequence, 4, "inverse of v000003 first");
        assert_eq!(inverses[1].sequence, 5, "then inverse of v000002");
        assert!(inverses.iter().all(|manifest| manifest.applied == applied));
        let a = store.resolve("/Tiles/A.gpkg").unwrap();
        assert_eq!(fs::read(a).unwrap(), b"a-v1");
        assert!(!store.resolve("/Tiles/B.gpkg").unwrap().exists());
        assert_eq!(store.versions().unwrap().len(), 5);
        let noop = store
            .rollback_to_at(CollectionId::parse("v000005").unwrap(), applied)
            .unwrap();
        assert!(noop.is_empty());
    }
}
