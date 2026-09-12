//! Datastore facade (Requirements File1–File6, Metadata1/Metadata5, CRS5,
//! Attr1, V1–V6).
//!
//! [`CdbDatastore`] is the operational facade: [`CdbDatastore::create`]
//! materializes a datastore from a [`DatastoreSeed`] (its mandatory §7.9.4.1
//! identity) and an [`ApplicationProfile`] (its policy), and the read/write
//! methods persist and retrieve the global metadata, storage CRS, resource
//! metadata records, attribute model, and versioning journal through the
//! requirements modules.
//!
//! Conformance reporting lives in [`crate::conformance`];
//! [`CdbDatastore::validate`] is the facade's entry point into it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::attribution::{self, AttributeModel, AttributionError};
use crate::conformance::{self, ConformanceReport};
use crate::crs::{CrsError, StorageCrs};
use crate::error::CdbError;
use crate::hierarchy::DatastoreLayout;
use crate::metadata::{
    GLOBAL_METADATA_STEM, GlobalMetadata, MetadataEncoding, MetadataError, MetadataViolation,
    ResourceMetadata,
};
use crate::naming::split_extension;
use crate::profiles::ApplicationProfile;
use crate::versioning::{
    self, ChangeAction, ChangeRecord, CollectionId, CollectionManifest, InverseOp,
    PendingCollection, PendingOp, VersioningError, VersioningViolation,
};

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
    /// Annex A `/conf/minimal-core`. The orchestration lives in
    /// [`crate::conformance::validate`]; this method is the facade's public
    /// entry point and delegates to it.
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
        conformance::validate(self, profile)
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

    /// Reads the datastore's attribute model from
    /// `global_metadata/vector_attributes.<ext>` (Requirements Attr1/Attr2,
    /// §7.1.2) — `Ok(None)` when the file is absent, attribution being an
    /// optional class. The file is looked up in the datastore's declared
    /// encoding — a stray other-encoding `vector_attributes` file is a
    /// Requirement Metadata5 matter, flagged by `validate`'s encoding sweep —
    /// and its content is fully validated, so an invalid model errors on
    /// read. Reading canonicalizes: the parse trims leading and trailing
    /// whitespace from `schemaUri`, `id`, `name`, and `description` before
    /// validating (see [`AttributeModel::from_json_str`]), so an indented,
    /// hand-authored file reads as the model it depicts and a model that came
    /// back from disk is always in canonical form. A GeoPackage-declared
    /// datastore has no core-readable model:
    /// [`MetadataError::UnsupportedEncoding`], as the metadata readers do.
    pub fn attribute_model(&self) -> Result<Option<AttributeModel>, CdbError> {
        let declared = self.global_metadata()?.encoding;
        let Some(name) = attribution::file_name_for(declared) else {
            return Err(CdbError::Metadata(MetadataError::UnsupportedEncoding(
                declared,
            )));
        };
        let path = self.layout.global_metadata_dir().join(&name);
        if !path.is_file() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path).map_err(AttributionError::Io)?;
        let model = if declared == MetadataEncoding::Xml {
            AttributeModel::from_xml_str(&content)?
        } else {
            AttributeModel::from_json_str(&content)?
        };
        Ok(Some(model))
    }

    /// Writes (or rewrites) the attribute model to
    /// `global_metadata/vector_attributes.<ext>` in the datastore's declared
    /// encoding, returning the physical file written — Requirements Attr1-B
    /// (the `global_metadata` location) and Attr1-C (the file name) hold by
    /// construction. The model is validated first (Attr2), so an invalid one
    /// is refused before any file is touched — including a model whose ids
    /// differ only by whitespace, which [`AttributeModel::validate`] treats as
    /// duplicates exactly as the read path does. Serialization is verbatim: a
    /// model holding edge whitespace in its strings is written as-is and comes
    /// back trimmed, because the *parse* canonicalizes — only canonical models
    /// round-trip unchanged. Requirement Metadata5 is
    /// enforced the way [`Self::write_global_metadata`] enforces it: a
    /// `vector_attributes` file already on disk in a *different* encoding
    /// refuses the write with [`MetadataViolation::EncodingMismatch`]. A
    /// same-encoding rewrite is allowed — the model is a plain global
    /// record, deliberately outside the versioning journal (the
    /// reserved-tree fence routes global-metadata changes through dedicated
    /// APIs) and not V3-stamped.
    pub fn write_attribute_model(&self, model: &AttributeModel) -> Result<PathBuf, CdbError> {
        model.validate().map_err(AttributionError::from)?;
        let declared = self.global_metadata()?.encoding;
        let Some(name) = attribution::file_name_for(declared) else {
            return Err(CdbError::Metadata(MetadataError::UnsupportedEncoding(
                declared,
            )));
        };
        let dir = self.layout.global_metadata_dir();
        for (extension, found) in [
            ("json", MetadataEncoding::Json),
            ("xml", MetadataEncoding::Xml),
        ] {
            if found != declared {
                let other = format!("{}.{extension}", attribution::VECTOR_ATTRIBUTES_STEM);
                if dir.join(&other).is_file() {
                    return Err(CdbError::Metadata(MetadataError::Violation(
                        MetadataViolation::EncodingMismatch {
                            file: other,
                            declared,
                            found,
                        },
                    )));
                }
            }
        }
        let content = if declared == MetadataEncoding::Xml {
            model.to_xml_string()?
        } else {
            model.to_json_string()?
        };
        let path = dir.join(&name);
        fs::write(&path, content).map_err(AttributionError::Io)?;
        Ok(path)
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
pub(crate) fn path_extension(logical_path: &str) -> Option<&str> {
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

    /// The archive-mirror path of `asset` under a collection's `archive/`
    /// subdirectory — `versions/<id>/archive/<mirror>`. The subdirectory
    /// namespaces archived prior bytes away from the sibling
    /// `manifest.<enc>`, so a root-level asset named `manifest.json`/`.xml`
    /// cannot overwrite (or be "restored" from) the commit manifest.
    fn archive_path(&self, id: CollectionId, asset: &str) -> PathBuf {
        self.version_dir(id)
            .join("archive")
            .join(asset.trim_start_matches('/'))
    }

    /// The manifest path of one collection dir in the given encoding —
    /// the commit point whose presence marks the dir as committed (an
    /// id-named dir without it is an uncommitted apply).
    fn manifest_path(&self, id: CollectionId, encoding: MetadataEncoding) -> PathBuf {
        self.version_dir(id)
            .join(format!("manifest.{}", encoding.extension()))
    }

    /// Reads one collection's manifest in the datastore's declared
    /// encoding.
    fn read_manifest(
        &self,
        id: CollectionId,
        encoding: MetadataEncoding,
    ) -> Result<CollectionManifest, CdbError> {
        let path = self.manifest_path(id, encoding);
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
    /// files are not journal entries), and a `v######` dir without a
    /// manifest is skipped as an UNCOMMITTED apply — the manifest is the
    /// commit point, so the next apply reuses that sequence and absorbs
    /// the dir. A present-but-unparseable manifest still errors (real
    /// corruption is not an uncommitted apply).
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
                // An id-named dir without a manifest is an UNCOMMITTED
                // apply (the manifest is the commit point) — skip it; the
                // next apply reuses its sequence and absorbs the dir.
                if !self.manifest_path(id, encoding).is_file() {
                    continue;
                }
                manifests.push(self.read_manifest(id, encoding)?);
            }
        }
        manifests.sort_by_key(|manifest| manifest.sequence);
        versioning::validate_journal(&manifests).map_err(versioning_violation)?;
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
    /// Single-writer and non-atomic: a crash mid-apply leaves an
    /// uncommitted `versions/<id>/` directory that the journal skips and
    /// the next apply absorbs; live-tree mutations from the failed attempt
    /// still require operator attention, consistent with the crate-wide
    /// no-concurrency stance.
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
            // Reserved-tree guard: a collection may not address the
            // crate-managed `versions/` journal or `global_metadata/`
            // records (implementation constraint, §7.14.2). Checked before
            // any tree interaction so a forged or corrupt path — asset or
            // linked record — never reaches the live tree.
            for path in std::iter::once(&change.asset).chain(change.resource_record.iter()) {
                if let Some(tree) = versioning::reserved_tree_of(path) {
                    return Err(versioning_violation(
                        VersioningViolation::AssetInReservedTree {
                            asset: path.clone(),
                            tree: tree.to_owned(),
                        },
                    ));
                }
            }
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
        fs::write(self.manifest_path(id, encoding), content).map_err(versioning_io)?;
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
    use crate::profiles::SimulationProfile;
    use tempfile::tempdir;

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
        let archived = store
            .root()
            .join("versions/v000002/archive/Tiles/Elevation.tif");
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
        let archived = store
            .root()
            .join("versions/v000002/archive/Tiles/Buildings.gpkg");
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

    /// Finding C1 (§7.14.6) — a root-level asset literally named
    /// `manifest.json` archives under `versions/<id>/archive/` and never
    /// collides with the sibling commit manifest, so its prior bytes
    /// survive and rollback restores the asset (not manifest bytes).
    #[test]
    fn req_core_versioning_root_manifest_asset_archives_safely() {
        let (_tmp, store) = versioned_store();
        store
            .apply_collection_at(
                PendingCollection::new().create("/manifest.json", *b"asset-not-a-manifest"),
                ts("2026-08-30T10:00:00Z"),
            )
            .unwrap();
        store
            .apply_collection_at(
                PendingCollection::new().replace("/manifest.json", *b"asset-v2"),
                ts("2026-08-30T11:00:00Z"),
            )
            .unwrap();
        let live = store.resolve("/manifest.json").unwrap();
        assert_eq!(fs::read(live).unwrap(), b"asset-v2");
        assert_eq!(
            fs::read(store.root().join("versions/v000002/archive/manifest.json")).unwrap(),
            b"asset-not-a-manifest"
        );
        // The sibling commit manifest coexists and parses: versions()
        // reads it (would fail here if the asset had overwritten it).
        let journal = store.versions().unwrap();
        assert_eq!(journal.len(), 2);
        assert_eq!(journal[1].changes[0].action, ChangeAction::Replaced);
        assert!(
            store
                .root()
                .join("versions/v000002/manifest.json")
                .is_file()
        );
        store
            .rollback_collection_at(
                CollectionId::parse("v000002").unwrap(),
                ts("2026-08-30T12:00:00Z"),
            )
            .unwrap();
        let live = store.resolve("/manifest.json").unwrap();
        assert_eq!(fs::read(live).unwrap(), b"asset-not-a-manifest");
    }

    /// Finding C2 (§7.14.2) — a collection may not address the
    /// crate-managed `versions/` journal or `global_metadata/` records;
    /// forged asset or resource-record targets are rejected as
    /// `AssetInReservedTree` and leave the journal untouched.
    #[test]
    fn req_core_versioning_reserved_tree_assets_rejected() {
        let (_tmp, store) = versioned_store();
        store
            .apply_collection_at(
                PendingCollection::new().create("/Tiles/RoadNetwork.gpkg", *b"road-bytes"),
                ts("2026-08-30T10:00:00Z"),
            )
            .unwrap();
        let cases = vec![
            PendingCollection::new().create("/versions/v000001/manifest.json", *b"forged"),
            PendingCollection::new().delete("/versions/v000001/manifest.json"),
            PendingCollection::new().create("/global_metadata/extra.json", *b"x"),
            PendingCollection::new()
                .create("/Tiles/Ok.gpkg", *b"ok")
                .for_record("/versions/v000001/manifest.json"),
        ];
        for pending in cases {
            match store.apply_collection_at(pending, ts("2026-08-30T11:00:00Z")) {
                Err(CdbError::Versioning(VersioningError::Violation(
                    VersioningViolation::AssetInReservedTree { .. },
                ))) => {}
                other => panic!("expected AssetInReservedTree, got {other:?}"),
            }
        }
        assert_eq!(store.versions().unwrap().len(), 1, "journal untouched");
    }

    /// Finding C3 (§7.14.3) — a `v######` dir without a manifest is an
    /// uncommitted apply: `versions()` skips it, and the next apply reuses
    /// its sequence and absorbs the dir.
    #[test]
    fn req_core_versioning_orphan_version_dir_skipped_and_reused() {
        let (_tmp, store) = versioned_store();
        store
            .apply_collection_at(
                PendingCollection::new().create("/Tiles/A.gpkg", *b"aa"),
                ts("2026-08-30T10:00:00Z"),
            )
            .unwrap();
        fs::create_dir_all(store.root().join("versions/v000002")).unwrap();
        assert_eq!(store.versions().unwrap().len(), 1, "orphan dir skipped");
        let manifest = store
            .apply_collection_at(
                PendingCollection::new().create("/Tiles/B.gpkg", *b"bb"),
                ts("2026-08-30T11:00:00Z"),
            )
            .unwrap();
        assert_eq!(manifest.sequence, 2, "sequence reused");
        assert_eq!(manifest.id.to_string(), "v000002");
        assert_eq!(store.versions().unwrap().len(), 2, "dir absorbed");
    }

    /// A small valid attribute model for the facade tests — the spec's
    /// fixture head plus PAttr1's supplementary URI.
    fn street_model() -> AttributeModel {
        AttributeModel {
            schema_uri: Some("https://example.org/schemas/street.xsd".to_owned()),
            attributes: vec![
                crate::attribution::AttributeDef {
                    id: "1".to_owned(),
                    name: "StreetName".to_owned(),
                    description: "Name of a street as an alphanumeric string".to_owned(),
                },
                crate::attribution::AttributeDef {
                    id: "2".to_owned(),
                    name: "StreetType".to_owned(),
                    description: "Type of street as an alphanumeric string".to_owned(),
                },
            ],
        }
    }

    /// Requirements Attr1-B/C + Attr2 (§7.1.2) — the facade writes the
    /// model to `global_metadata/vector_attributes.json` in the declared
    /// encoding and reads back an equal, valid model.
    #[test]
    fn req_core_attribute_model_facade_roundtrip_json() {
        let tmp = tempdir().unwrap();
        let store = CdbDatastore::create(
            tmp.path(),
            &SimulationProfile::json(),
            DatastoreSeed::new("doi:cdb.demo", "Demo", "A demonstration datastore", "CAE"),
        )
        .unwrap();

        let model = street_model();
        let path = store.write_attribute_model(&model).unwrap();
        assert_eq!(
            path,
            store.root().join("global_metadata/vector_attributes.json")
        );
        assert!(path.is_file());
        assert_eq!(store.attribute_model().unwrap(), Some(model));
    }

    /// Requirements Attr1-B/C + Attr2 (§7.1.2) — the XML twin: the
    /// declared encoding picks `vector_attributes.xml`.
    #[test]
    fn req_core_attribute_model_facade_roundtrip_xml() {
        let tmp = tempdir().unwrap();
        let store = CdbDatastore::create(
            tmp.path(),
            &SimulationProfile::xml(),
            DatastoreSeed::new("doi:cdb.demo", "Demo", "A demonstration datastore", "CAE"),
        )
        .unwrap();

        let model = street_model();
        let path = store.write_attribute_model(&model).unwrap();
        assert_eq!(
            path,
            store.root().join("global_metadata/vector_attributes.xml")
        );
        assert_eq!(store.attribute_model().unwrap(), Some(model));
    }

    /// Attribution is an optional class (§7.1.2): a datastore without a
    /// `vector_attributes` file reads `Ok(None)`.
    #[test]
    fn req_core_attribute_model_facade_absent_none() {
        let tmp = tempdir().unwrap();
        let store = CdbDatastore::create(
            tmp.path(),
            &SimulationProfile::json(),
            DatastoreSeed::new("doi:cdb.demo", "Demo", "A demonstration datastore", "CAE"),
        )
        .unwrap();
        assert_eq!(store.attribute_model().unwrap(), None);
    }

    /// The read path validates (§7.1.2.3): malformed bytes are a
    /// Serialization failure and a parseable file violating Attr2 is a
    /// Violation — no invalid model escapes the facade.
    #[test]
    fn req_core_attribute_model_facade_invalid_file_errors() {
        let tmp = tempdir().unwrap();
        let store = CdbDatastore::create(
            tmp.path(),
            &SimulationProfile::json(),
            DatastoreSeed::new("doi:cdb.demo", "Demo", "A demonstration datastore", "CAE"),
        )
        .unwrap();
        let path = store.root().join("global_metadata/vector_attributes.json");

        fs::write(&path, "not json").unwrap();
        assert!(matches!(
            store.attribute_model(),
            Err(CdbError::Attribution(AttributionError::Serialization(_)))
        ));

        fs::write(
            &path,
            r#"{ "attributes": [
                { "id": "1", "name": "A", "description": "a" },
                { "id": "1", "name": "B", "description": "b" }
            ] }"#,
        )
        .unwrap();
        assert!(matches!(
            store.attribute_model(),
            Err(CdbError::Attribution(AttributionError::Violation(
                crate::attribution::AttributionViolation::DuplicateId { .. }
            )))
        ));
    }

    /// The writer validates first (§7.1.2): an invalid model — here the
    /// empty model — is refused before any file is touched.
    #[test]
    fn req_core_attribute_model_facade_write_refuses_invalid() {
        let tmp = tempdir().unwrap();
        let store = CdbDatastore::create(
            tmp.path(),
            &SimulationProfile::json(),
            DatastoreSeed::new("doi:cdb.demo", "Demo", "A demonstration datastore", "CAE"),
        )
        .unwrap();

        let empty = AttributeModel {
            schema_uri: None,
            attributes: Vec::new(),
        };
        assert!(matches!(
            store.write_attribute_model(&empty),
            Err(CdbError::Attribution(AttributionError::Violation(
                crate::attribution::AttributionViolation::EmptyModel
            )))
        ));
        assert!(
            !store
                .root()
                .join("global_metadata/vector_attributes.json")
                .exists(),
            "nothing written"
        );
    }

    /// Requirement Attr2-B (§7.1.2.3) end to end: the writer never emits
    /// a file the reader would refuse. Ids that differ only by
    /// whitespace are one identifier at validate time too, so the write
    /// is refused before any file is touched.
    #[test]
    fn req_core_attribute_model_facade_write_refuses_whitespace_forged_ids() {
        let tmp = tempdir().unwrap();
        let store = CdbDatastore::create(
            tmp.path(),
            &SimulationProfile::json(),
            DatastoreSeed::new("doi:cdb.demo", "Demo", "A demonstration datastore", "CAE"),
        )
        .unwrap();

        let mut forged = street_model();
        forged.attributes[1].id = " 1 ".to_owned();
        assert!(matches!(
            store.write_attribute_model(&forged),
            Err(CdbError::Attribution(AttributionError::Violation(
                crate::attribution::AttributionViolation::DuplicateId { .. }
            )))
        ));
        assert!(
            !store
                .root()
                .join("global_metadata/vector_attributes.json")
                .exists(),
            "nothing written"
        );
    }

    /// Requirement Attr1-C (§7.1.2.1) names only `xml` and `json`, so a
    /// GeoPackage-declared datastore has no core-readable attribute
    /// model: both facade directions report
    /// [`MetadataError::UnsupportedEncoding`], the metadata readers'
    /// precedent. (`gpkg` is unrepresentable through a profile
    /// constructor, so the declaration is patched on disk — the only way
    /// a `gpkg`-declaring store can exist for the core.)
    #[test]
    fn req_core_attribute_model_facade_gpkg_unsupported() {
        let tmp = tempdir().unwrap();
        let store = CdbDatastore::create(
            tmp.path(),
            &SimulationProfile::json(),
            DatastoreSeed::new("doi:cdb.demo", "Demo", "A demonstration datastore", "CAE"),
        )
        .unwrap();
        let record = store.root().join("global_metadata/global_metadata.json");
        let patched = fs::read_to_string(&record).unwrap().replace(
            r#""metadataEncoding": "json""#,
            r#""metadataEncoding": "gpkg""#,
        );
        assert!(patched.contains("gpkg"), "declaration patched");
        fs::write(&record, patched).unwrap();

        assert!(matches!(
            store.attribute_model(),
            Err(CdbError::Metadata(MetadataError::UnsupportedEncoding(
                MetadataEncoding::Gpkg
            )))
        ));
        assert!(matches!(
            store.write_attribute_model(&street_model()),
            Err(CdbError::Metadata(MetadataError::UnsupportedEncoding(
                MetadataEncoding::Gpkg
            )))
        ));
    }

    /// Requirement Metadata5 — `write_attribute_model` refuses to write
    /// beside a `vector_attributes` file in a different encoding, the
    /// same single-encoding guard as `write_global_metadata`.
    #[test]
    fn req_core_metadata_encoding_write_attribute_model_refuses_switch() {
        let tmp = tempdir().unwrap();
        let store = CdbDatastore::create(
            tmp.path(),
            &SimulationProfile::json(),
            DatastoreSeed::new("doi:cdb.demo", "Demo", "A demonstration datastore", "CAE"),
        )
        .unwrap();
        let stray = store.root().join("global_metadata/vector_attributes.xml");
        fs::write(&stray, "<AttributeModel/>").unwrap();

        assert!(matches!(
            store.write_attribute_model(&street_model()),
            Err(CdbError::Metadata(MetadataError::Violation(
                MetadataViolation::EncodingMismatch { .. }
            )))
        ));
        assert!(
            !store
                .root()
                .join("global_metadata/vector_attributes.json")
                .exists(),
            "refused before writing"
        );
    }
}
