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
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use thiserror::Error;

use crate::crs::{CrsError, CrsViolation, CrsWarning, StorageCrs};
use crate::error::CdbError;
use crate::hierarchy::{DatastoreLayout, HierarchyViolation, HierarchyWarning};
use crate::links::LinkViolation;
use crate::metadata::{
    GLOBAL_METADATA_STEM, GlobalMetadata, MetadataEncoding, MetadataError, MetadataViolation,
    ResourceMetadata,
};
use crate::naming::{NamingViolation, NamingWarning, split_extension};
use crate::profiles::{ApplicationProfile, RequirementsClass};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hierarchy::HierarchyError;
    use crate::metadata::{MetadataStandard, UnitOfMeasure};
    use crate::profiles::SimulationProfile;
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
}
