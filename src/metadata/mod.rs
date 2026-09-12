//! CDB Global and Local Resource/Dataset Metadata requirements module.
//!
//! Implements the `/req/core/metadata-` requirements class (spec §7.9):
//! the global metadata record for an entire datastore (§7.9.4.1), resource
//! (dataset) metadata (§7.9.4.2), and Requirements Metadata1–Metadata8.
//!
//! Wire names follow the spec tables verbatim (`ID`, `contactPoint`,
//! `CharacterSetCode`, …). Note the spec names the folder `Global_Metadata`
//! in §7.9.3.1 but `global_metadata` in Requirement File6 (§7.5.7); this
//! crate follows File6 via [`crate::hierarchy::GLOBAL_METADATA_DIR`].

pub mod temporal;

use std::fmt;
use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

use crate::coverage::DomainSet;
use crate::hierarchy::{DatastoreLayout, GLOBAL_METADATA_DIR};
use crate::links::{Link, LinkViolation};
use crate::media_types::MediaType;
use crate::tiling::TilingScheme;
use crate::topology::WindingOrder;

pub use temporal::Temporal;

/// File stem of the global metadata file inside `global_metadata/`.
pub const GLOBAL_METADATA_STEM: &str = "global_metadata";

/// A violation of a SHALL requirement in the metadata module.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MetadataViolation {
    #[error("mandatory metadata element {element:?} is missing or empty (spec §7.9.4 tables)")]
    MissingElement { element: &'static str },
    #[error("{value:?} is not an allowed metadata standard (violates /req/core/metadata-standard)")]
    UnknownMetadataStandard { value: String },
    #[error(
        "{value:?} is not an allowed metadata encoding; use xml, json, or gpkg (violates /req/core/metadata-encoding)"
    )]
    UnknownEncoding { value: String },
    #[error(
        "{value:?} is not an allowed unit of measure; use M, FT, K, or MI (violates /req/core/metadata-uom-measure)"
    )]
    UnknownUnitOfMeasure { value: String },
    #[error(
        "{value:?} is not a well-formed BCP 47 language tag: {reason} (violates /req/core/metadata-language)"
    )]
    InvalidLanguageTag { value: String, reason: &'static str },
    #[error(
        "{value:?} is not an RFC 3339 datetime: {reason} (violates /req/core/metadata-datetime)"
    )]
    InvalidDateTime { value: String, reason: String },
    #[error("datetime {value:?} is not in UTC (violates /req/core/metadata-datetime A)")]
    NotUtc { value: String },
    #[error(
        "{value:?} is not a valid temporal geometry: {reason} (violates /req/core/metadata-temporal-interval)"
    )]
    InvalidTemporalInterval { value: String, reason: &'static str },
    #[error(
        "metadata file {file:?} is encoded as {found} but the datastore declares {declared} (violates /req/core/metadata-encoding)"
    )]
    EncodingMismatch {
        file: String,
        declared: MetadataEncoding,
        found: MetadataEncoding,
    },
    #[error(
        "no global metadata file found under {searched:?} (violates /req/core/metadata-repository and /req/core/metadata-global)"
    )]
    MissingGlobalMetadata { searched: PathBuf },
    #[error("global metadata could not be parsed: {reason} (violates /req/core/metadata-encoding)")]
    Malformed { reason: String },
    #[error(transparent)]
    Link(#[from] LinkViolation),
}

/// Operational errors for metadata I/O.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum MetadataError {
    #[error(transparent)]
    Violation(#[from] MetadataViolation),
    #[error(
        "the core cannot write {0}-encoded metadata files; that container belongs to an application profile"
    )]
    UnsupportedEncoding(MetadataEncoding),
    #[error("metadata (de)serialization failed: {0}")]
    Serialization(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn ser_err(error: impl fmt::Display) -> MetadataError {
    MetadataError::Serialization(error.to_string())
}

macro_rules! string_serde {
    ($ty:ty) => {
        impl Serialize for $ty {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = String::deserialize(deserializer)?;
                Self::parse(&value).map_err(serde::de::Error::custom)
            }
        }
        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

/// The metadata standards a CDB datastore may declare
/// (Requirement Metadata2, `/req/core/metadata-standard`, §7.9.3.2).
/// This list is closed: unknown values are rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetadataStandard {
    Iso19115v2019,
    Iso19115v2003,
    Ddms50,
    Ddms41,
    Dcat,
    DcatAp,
    GeoDcatAp,
    Ngcmp,
    Fg3d,
    NoMetadata,
}

impl MetadataStandard {
    pub const ALL: [MetadataStandard; 10] = [
        MetadataStandard::Iso19115v2019,
        MetadataStandard::Iso19115v2003,
        MetadataStandard::Ddms50,
        MetadataStandard::Ddms41,
        MetadataStandard::Dcat,
        MetadataStandard::DcatAp,
        MetadataStandard::GeoDcatAp,
        MetadataStandard::Ngcmp,
        MetadataStandard::Fg3d,
        MetadataStandard::NoMetadata,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            MetadataStandard::Iso19115v2019 => "ISO-19115:2019",
            MetadataStandard::Iso19115v2003 => "ISO-19115:2003",
            MetadataStandard::Ddms50 => "DDMS-5.0",
            MetadataStandard::Ddms41 => "DDMS-4.1",
            MetadataStandard::Dcat => "DCAT",
            MetadataStandard::DcatAp => "DCAT-AP",
            MetadataStandard::GeoDcatAp => "GeoDCAT-AP",
            MetadataStandard::Ngcmp => "NGCMP",
            MetadataStandard::Fg3d => "FG3D",
            MetadataStandard::NoMetadata => "NoMetadata",
        }
    }

    pub fn parse(value: &str) -> Result<Self, MetadataViolation> {
        Self::ALL
            .into_iter()
            .find(|standard| standard.as_str() == value)
            .ok_or_else(|| MetadataViolation::UnknownMetadataStandard {
                value: value.to_owned(),
            })
    }
}
string_serde!(MetadataStandard);

/// The single metadata encoding used datastore-wide
/// (Requirement Metadata5, `/req/core/metadata-encoding`, §7.9.3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetadataEncoding {
    Xml,
    Json,
    Gpkg,
}

impl MetadataEncoding {
    pub fn as_str(self) -> &'static str {
        match self {
            MetadataEncoding::Xml => "xml",
            MetadataEncoding::Json => "json",
            MetadataEncoding::Gpkg => "gpkg",
        }
    }

    pub fn extension(self) -> &'static str {
        self.as_str()
    }

    pub fn parse(value: &str) -> Result<Self, MetadataViolation> {
        match value {
            "xml" => Ok(MetadataEncoding::Xml),
            "json" => Ok(MetadataEncoding::Json),
            "gpkg" => Ok(MetadataEncoding::Gpkg),
            _ => Err(MetadataViolation::UnknownEncoding {
                value: value.to_owned(),
            }),
        }
    }
}
string_serde!(MetadataEncoding);

/// The single unit of measure for measurements datastore-wide
/// (Requirement Metadata8, `/req/core/metadata-uom-measure`, §7.9.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitOfMeasure {
    Meters,
    Feet,
    Kilometers,
    Miles,
}

impl UnitOfMeasure {
    pub fn as_str(self) -> &'static str {
        match self {
            UnitOfMeasure::Meters => "M",
            UnitOfMeasure::Feet => "FT",
            UnitOfMeasure::Kilometers => "K",
            UnitOfMeasure::Miles => "MI",
        }
    }

    pub fn parse(value: &str) -> Result<Self, MetadataViolation> {
        match value {
            "M" => Ok(UnitOfMeasure::Meters),
            "FT" => Ok(UnitOfMeasure::Feet),
            "K" => Ok(UnitOfMeasure::Kilometers),
            "MI" => Ok(UnitOfMeasure::Miles),
            _ => Err(MetadataViolation::UnknownUnitOfMeasure {
                value: value.to_owned(),
            }),
        }
    }
}
string_serde!(UnitOfMeasure);

/// A well-formed IETF BCP 47 language tag
/// (Requirement Metadata4, `/req/core/metadata-language`, §7.9.3.4).
///
/// Validation is syntactic (RFC 5646 subtag shape), not a registry lookup.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LanguageTag(String);

impl LanguageTag {
    pub fn new(tag: impl Into<String>) -> Result<Self, MetadataViolation> {
        fn check(tag: &str) -> Result<(), &'static str> {
            if tag.is_empty() {
                return Err("empty");
            }
            let mut subtags = tag.split('-');
            let primary = subtags.next().unwrap_or_default();
            if primary.is_empty()
                || primary.len() > 8
                || !primary.chars().all(|c| c.is_ascii_alphabetic())
            {
                return Err("primary subtag must be 1-8 ASCII letters");
            }
            for subtag in subtags {
                if subtag.is_empty() {
                    return Err("empty subtag");
                }
                if subtag.len() > 8 || !subtag.chars().all(|c| c.is_ascii_alphanumeric()) {
                    return Err("subtags must be 1-8 ASCII alphanumerics");
                }
            }
            Ok(())
        }
        let tag = tag.into();
        match check(&tag) {
            Ok(()) => Ok(Self(tag)),
            Err(reason) => Err(MetadataViolation::InvalidLanguageTag { value: tag, reason }),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for LanguageTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for LanguageTag {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for LanguageTag {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        LanguageTag::new(value).map_err(serde::de::Error::custom)
    }
}

/// Character coding of a resource's text (§7.9.4.2, `CharacterSetCode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum CharacterSet {
    #[default]
    #[serde(rename = "utf8")]
    Utf8,
    #[serde(rename = "utf16")]
    Utf16,
}

impl CharacterSet {
    fn is_default(value: &CharacterSet) -> bool {
        *value == CharacterSet::Utf8
    }
}

/// Resource type of a dataset record; the §7.9.4.2 table requires the
/// literal `dataset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum ResourceType {
    #[default]
    #[serde(rename = "dataset")]
    Dataset,
}

/// Checks Requirement Metadata5 over a set of metadata file names: every
/// metadata file must match the declared datastore-wide encoding.
pub fn encoding_violations<'a>(
    declared: MetadataEncoding,
    file_names: impl IntoIterator<Item = &'a str>,
) -> Vec<MetadataViolation> {
    let mut violations = Vec::new();
    for name in file_names {
        let (_, Some(extension)) = crate::naming::split_extension(name) else {
            continue;
        };
        let found = match extension.to_ascii_lowercase().as_str() {
            "xml" | "xsd" => MetadataEncoding::Xml,
            "json" => MetadataEncoding::Json,
            "gpkg" => MetadataEncoding::Gpkg,
            _ => continue, // Not a metadata encoding; not this requirement's concern.
        };
        if found != declared {
            violations.push(MetadataViolation::EncodingMismatch {
                file: name.to_owned(),
                declared,
                found,
            });
        }
    }
    violations
}

/// Global metadata for an entire CDB datastore instance (§7.9.4.1 table),
/// plus the datastore-wide declarations of Requirements Metadata2
/// (standard), Metadata5 (encoding), and Metadata8 (`uom`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlobalMetadata {
    #[serde(rename = "ID")]
    pub id: String,
    pub title: String,
    pub description: String,
    #[serde(rename = "contactPoint")]
    pub contact_point: String,
    #[serde(with = "temporal::rfc3339_utc")]
    pub created: DateTime<Utc>,
    pub language: LanguageTag,
    #[serde(rename = "metadataStandard")]
    pub metadata_standard: MetadataStandard,
    #[serde(rename = "metadataEncoding")]
    pub encoding: MetadataEncoding,
    /// Requirement Metadata8-B: the element name SHALL be `uom`.
    pub uom: UnitOfMeasure,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "temporal::rfc3339_utc_option"
    )]
    pub update: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temporal: Option<Temporal>,
    #[serde(
        rename = "accessRights",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub access_rights: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Tiling-scheme definition for a tiled datastore — the conditional
    /// element introduced by Requirement Tiling8
    /// (/req/core/tiling-tilingscheme-definition, §7.10.2.5); wire name
    /// `tilingScheme`. Untiled datastores omit it.
    #[serde(
        rename = "tilingScheme",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub tiling_scheme: Option<TilingScheme>,
}

impl GlobalMetadata {
    pub fn builder() -> GlobalMetadataBuilder {
        GlobalMetadataBuilder::default()
    }

    /// Checks the mandatory elements of the §7.9.4.1 table.
    pub fn validate(&self) -> Result<(), MetadataViolation> {
        let mandatory = [
            (&self.id, "ID"),
            (&self.title, "title"),
            (&self.description, "description"),
            (&self.contact_point, "contactPoint"),
        ];
        for (value, element) in mandatory {
            if value.is_empty() {
                return Err(MetadataViolation::MissingElement { element });
            }
        }
        Ok(())
    }

    pub fn to_json_string(&self) -> Result<String, MetadataError> {
        serde_json::to_string_pretty(self).map_err(ser_err)
    }

    pub fn from_json_str(content: &str) -> Result<Self, MetadataError> {
        let metadata: Self = serde_json::from_str(content).map_err(ser_err)?;
        metadata.validate()?;
        Ok(metadata)
    }

    pub fn to_xml_string(&self) -> Result<String, MetadataError> {
        quick_xml::se::to_string(self).map_err(ser_err)
    }

    pub fn from_xml_str(content: &str) -> Result<Self, MetadataError> {
        let metadata: Self = quick_xml::de::from_str(content).map_err(ser_err)?;
        metadata.validate()?;
        Ok(metadata)
    }

    /// Writes this record into the datastore's `global_metadata` folder as
    /// `global_metadata.<ext>` per the declared encoding
    /// (Requirement Metadata1, `/req/core/metadata-repository`).
    pub fn write_to(&self, layout: &DatastoreLayout) -> Result<PathBuf, MetadataError> {
        self.validate()?;
        let content = match self.encoding {
            MetadataEncoding::Json => self.to_json_string()?,
            MetadataEncoding::Xml => self.to_xml_string()?,
            MetadataEncoding::Gpkg => {
                return Err(MetadataError::UnsupportedEncoding(MetadataEncoding::Gpkg));
            }
        };
        let dir = layout.global_metadata_dir();
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!(
            "{GLOBAL_METADATA_STEM}.{}",
            self.encoding.extension()
        ));
        fs::write(&path, content)?;
        Ok(path)
    }

    /// Reads the global metadata record from a datastore.
    pub fn read_from(layout: &DatastoreLayout) -> Result<Self, MetadataError> {
        let dir = layout.global_metadata_dir();
        for extension in ["json", "xml"] {
            let path = dir.join(format!("{GLOBAL_METADATA_STEM}.{extension}"));
            if !path.is_file() {
                continue;
            }
            let content = fs::read_to_string(&path)?;
            return if extension == "json" {
                Self::from_json_str(&content)
            } else {
                Self::from_xml_str(&content)
            };
        }
        Err(MetadataViolation::MissingGlobalMetadata { searched: dir }.into())
    }

    /// The logical path (link) to the physical global metadata file
    /// (Requirement Metadata3, `/req/core/metadata-global`).
    pub fn locate(layout: &DatastoreLayout) -> Result<String, MetadataError> {
        for extension in ["json", "xml"] {
            let name = format!("{GLOBAL_METADATA_STEM}.{extension}");
            if layout.global_metadata_dir().join(&name).is_file() {
                return Ok(format!("/{GLOBAL_METADATA_DIR}/{name}"));
            }
        }
        Err(MetadataViolation::MissingGlobalMetadata {
            searched: layout.global_metadata_dir(),
        }
        .into())
    }
}

/// Fluent builder for [`GlobalMetadata`]; `build` enforces the mandatory
/// elements of the §7.9.4.1 table and Requirements Metadata2/5/8.
#[derive(Debug, Default)]
pub struct GlobalMetadataBuilder {
    id: Option<String>,
    title: Option<String>,
    description: Option<String>,
    contact_point: Option<String>,
    created: Option<DateTime<Utc>>,
    language: Option<LanguageTag>,
    metadata_standard: Option<MetadataStandard>,
    encoding: Option<MetadataEncoding>,
    uom: Option<UnitOfMeasure>,
    update: Option<DateTime<Utc>>,
    temporal: Option<Temporal>,
    access_rights: Option<String>,
    license: Option<String>,
    tiling_scheme: Option<TilingScheme>,
}

impl GlobalMetadataBuilder {
    #[must_use]
    pub fn id(mut self, value: impl Into<String>) -> Self {
        self.id = Some(value.into());
        self
    }

    #[must_use]
    pub fn title(mut self, value: impl Into<String>) -> Self {
        self.title = Some(value.into());
        self
    }

    #[must_use]
    pub fn description(mut self, value: impl Into<String>) -> Self {
        self.description = Some(value.into());
        self
    }

    #[must_use]
    pub fn contact_point(mut self, value: impl Into<String>) -> Self {
        self.contact_point = Some(value.into());
        self
    }

    #[must_use]
    pub fn created(mut self, value: DateTime<Utc>) -> Self {
        self.created = Some(value);
        self
    }

    #[must_use]
    pub fn language(mut self, value: LanguageTag) -> Self {
        self.language = Some(value);
        self
    }

    #[must_use]
    pub fn standard(mut self, value: MetadataStandard) -> Self {
        self.metadata_standard = Some(value);
        self
    }

    #[must_use]
    pub fn encoding(mut self, value: MetadataEncoding) -> Self {
        self.encoding = Some(value);
        self
    }

    #[must_use]
    pub fn uom(mut self, value: UnitOfMeasure) -> Self {
        self.uom = Some(value);
        self
    }

    #[must_use]
    pub fn update(mut self, value: DateTime<Utc>) -> Self {
        self.update = Some(value);
        self
    }

    #[must_use]
    pub fn temporal(mut self, value: Temporal) -> Self {
        self.temporal = Some(value);
        self
    }

    #[must_use]
    pub fn access_rights(mut self, value: impl Into<String>) -> Self {
        self.access_rights = Some(value.into());
        self
    }

    #[must_use]
    pub fn license(mut self, value: impl Into<String>) -> Self {
        self.license = Some(value.into());
        self
    }

    /// Sets the `tilingScheme` conditional element (Requirement Tiling8,
    /// `/req/core/tiling-tilingscheme-definition`, §7.10.2.5), which a
    /// **tiled** datastore SHALL carry on its global record. Untiled
    /// datastores omit it, so the element stays optional and the setter is
    /// simply not called; [`crate::tiling::TilingScheme::require`] is the
    /// reader's side of the same requirement.
    #[must_use]
    pub fn tiling_scheme(mut self, value: TilingScheme) -> Self {
        self.tiling_scheme = Some(value);
        self
    }

    pub fn build(self) -> Result<GlobalMetadata, MetadataViolation> {
        fn required<T>(value: Option<T>, element: &'static str) -> Result<T, MetadataViolation> {
            value.ok_or(MetadataViolation::MissingElement { element })
        }
        fn required_text(
            value: Option<String>,
            element: &'static str,
        ) -> Result<String, MetadataViolation> {
            match value {
                Some(text) if !text.is_empty() => Ok(text),
                _ => Err(MetadataViolation::MissingElement { element }),
            }
        }
        Ok(GlobalMetadata {
            id: required_text(self.id, "ID")?,
            title: required_text(self.title, "title")?,
            description: required_text(self.description, "description")?,
            contact_point: required_text(self.contact_point, "contactPoint")?,
            created: required(self.created, "created")?,
            language: required(self.language, "language")?,
            metadata_standard: required(self.metadata_standard, "metadataStandard")?,
            encoding: required(self.encoding, "metadataEncoding")?,
            uom: required(self.uom, "uom")?,
            update: self.update,
            temporal: self.temporal,
            access_rights: self.access_rights,
            license: self.license,
            tiling_scheme: self.tiling_scheme,
        })
    }
}

/// A geographic bounding box for the `extent` element (WGS-84 style axis
/// order west, south, east, north; may cross the antimeridian).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bbox {
    pub west: f64,
    pub south: f64,
    pub east: f64,
    pub north: f64,
}

/// Spatial and temporal extents of a resource (§7.9.4.2 `extent`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Extent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spatial: Option<Bbox>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temporal: Option<Temporal>,
}

/// Resource (dataset) metadata (§7.9.4.2 table). Mandatory: `ID`, `type`
/// (the literal `dataset`), `title`, `description`. `associations` reuses
/// the Links module; `CharacterSetCode` defaults to `utf8`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceMetadata {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "type")]
    pub resource_type: ResourceType,
    pub title: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    #[serde(
        rename = "keywordsCodespace",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub keywords_codespace: Option<String>,
    #[serde(
        rename = "externalId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub external_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "temporal::rfc3339_utc_option"
    )]
    pub created: Option<DateTime<Utc>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "temporal::rfc3339_utc_option"
    )]
    pub updated: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub themes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub formats: Vec<MediaType>,
    #[serde(
        rename = "contactPoint",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub contact_point: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rights: Option<String>,
    /// Unit of measure for measurement values (e.g. geometry m coordinates)
    /// in this dataset — the conditional element introduced by Requirement
    /// Geom4 (/req/core/geometry-mvalue, §7.6.3); element name per
    /// Requirement Metadata8.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uom: Option<UnitOfMeasure>,
    /// domainSet metadata for a coverage instance — the conditional
    /// element introduced by Requirement Coverages6
    /// (/req/core/coverage-domainSet, §7.2.6); wire name `domainSet`.
    #[serde(rename = "domainSet", default, skip_serializing_if = "Option::is_none")]
    pub domain_set: Option<DomainSet>,
    /// Winding order of the dataset's generated faces — the conditional
    /// element introduced by Face Topology Requirement 4
    /// (/req/core/topology-winding; the box's slug is `-face-winding`,
    /// §7.13.5.5); wire name `windingOrder`. Fourth use of the §7.9.4.2
    /// conditional-element mechanism.
    #[serde(
        rename = "windingOrder",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub winding_order: Option<WindingOrder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extent: Option<Extent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub associations: Vec<Link>,
    #[serde(
        rename = "CharacterSetCode",
        default,
        skip_serializing_if = "CharacterSet::is_default"
    )]
    pub character_set: CharacterSet,
}

impl ResourceMetadata {
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            resource_type: ResourceType::Dataset,
            title: title.into(),
            description: description.into(),
            keywords: Vec::new(),
            keywords_codespace: None,
            external_id: None,
            publisher: None,
            created: None,
            updated: None,
            themes: Vec::new(),
            formats: Vec::new(),
            contact_point: None,
            license: None,
            rights: None,
            uom: None,
            domain_set: None,
            winding_order: None,
            extent: None,
            associations: Vec::new(),
            character_set: CharacterSet::Utf8,
        }
    }

    /// Checks the mandatory elements of the §7.9.4.2 table and validates
    /// every association link against the Links module.
    pub fn validate(&self) -> Result<(), MetadataViolation> {
        let mandatory = [
            (&self.id, "ID"),
            (&self.title, "title"),
            (&self.description, "description"),
        ];
        for (value, element) in mandatory {
            if value.is_empty() {
                return Err(MetadataViolation::MissingElement { element });
            }
        }
        for link in &self.associations {
            link.validate()?;
        }
        Ok(())
    }

    pub fn to_json_string(&self) -> Result<String, MetadataError> {
        serde_json::to_string_pretty(self).map_err(ser_err)
    }

    pub fn from_json_str(content: &str) -> Result<Self, MetadataError> {
        let metadata: Self = serde_json::from_str(content).map_err(ser_err)?;
        metadata.validate()?;
        Ok(metadata)
    }

    pub fn to_xml_string(&self) -> Result<String, MetadataError> {
        quick_xml::se::to_string(self).map_err(ser_err)
    }

    pub fn from_xml_str(content: &str) -> Result<Self, MetadataError> {
        let metadata: Self = quick_xml::de::from_str(content).map_err(ser_err)?;
        metadata.validate()?;
        Ok(metadata)
    }
}

#[cfg(test)]
mod tests {
    use super::temporal::parse_datetime;
    use super::*;
    use tempfile::tempdir;

    fn dt(text: &str) -> DateTime<Utc> {
        parse_datetime(text).unwrap()
    }

    fn sample_global(encoding: MetadataEncoding) -> GlobalMetadata {
        GlobalMetadata::builder()
            .id("doi:10.5281/cdb.demo.1")
            .title("Yemen demonstration CDB data store")
            .description("Demonstration datastore for the rusty_cdb core test suite")
            .contact_point("CAE")
            .created(dt("2026-07-14T09:30:00Z"))
            .language(LanguageTag::new("en").unwrap())
            .standard(MetadataStandard::Dcat)
            .encoding(encoding)
            .uom(UnitOfMeasure::Meters)
            .build()
            .unwrap()
    }

    /// §7.9.3.2 Requirement Metadata2 — the closed list of standards.
    #[test]
    fn req_core_metadata_standard_list_and_reject() {
        let cases = [
            (MetadataStandard::Iso19115v2019, "ISO-19115:2019"),
            (MetadataStandard::Iso19115v2003, "ISO-19115:2003"),
            (MetadataStandard::Ddms50, "DDMS-5.0"),
            (MetadataStandard::Ddms41, "DDMS-4.1"),
            (MetadataStandard::Dcat, "DCAT"),
            (MetadataStandard::DcatAp, "DCAT-AP"),
            (MetadataStandard::GeoDcatAp, "GeoDCAT-AP"),
            (MetadataStandard::Ngcmp, "NGCMP"),
            (MetadataStandard::Fg3d, "FG3D"),
            (MetadataStandard::NoMetadata, "NoMetadata"),
        ];
        for (standard, text) in cases {
            assert_eq!(standard.as_str(), text);
            assert_eq!(MetadataStandard::parse(text), Ok(standard));
        }
        assert!(matches!(
            MetadataStandard::parse("ISO-19115"),
            Err(MetadataViolation::UnknownMetadataStandard { .. })
        ));
        // Serde round-trip and rejection.
        assert_eq!(
            serde_json::from_str::<MetadataStandard>(r#""DCAT-AP""#).unwrap(),
            MetadataStandard::DcatAp
        );
        assert!(serde_json::from_str::<MetadataStandard>(r#""YAML-MD""#).is_err());
    }

    /// §7.9.3.5 Requirement Metadata5 — xml, json, or gpkg only.
    #[test]
    fn req_core_metadata_encoding_values() {
        assert_eq!(MetadataEncoding::parse("xml"), Ok(MetadataEncoding::Xml));
        assert_eq!(MetadataEncoding::parse("json"), Ok(MetadataEncoding::Json));
        assert_eq!(MetadataEncoding::parse("gpkg"), Ok(MetadataEncoding::Gpkg));
        assert!(matches!(
            MetadataEncoding::parse("yaml"),
            Err(MetadataViolation::UnknownEncoding { .. })
        ));
        assert_eq!(MetadataEncoding::Json.extension(), "json");
    }

    /// §7.9.3.5 Metadata5 — one encoding for all metadata instances.
    #[test]
    fn req_core_metadata_encoding_consistency() {
        let violations = encoding_violations(
            MetadataEncoding::Json,
            [
                "global_metadata.json",
                "Lights.xml",
                "Elevation.tif",
                "Datasets.xsd",
            ],
        );
        assert_eq!(violations.len(), 2);
        assert!(matches!(
            &violations[0],
            MetadataViolation::EncodingMismatch { file, found: MetadataEncoding::Xml, .. }
                if file == "Lights.xml"
        ));
        assert!(
            encoding_violations(
                MetadataEncoding::Json,
                ["global_metadata.json", "RoadNetwork.json"],
            )
            .is_empty()
        );
    }

    /// §7.9.3.4 Requirement Metadata4 — BCP 47 well-formedness.
    #[test]
    fn req_core_metadata_language_bcp47() {
        for tag in ["en", "en-US", "zh-Hant-CN", "x-private"] {
            assert!(LanguageTag::new(tag).is_ok(), "{tag}");
        }
        for tag in [
            "",
            "en_US",
            "xx!",
            "-en",
            "en-",
            "en--US",
            "abcdefghi",
            "en-abcdefghi",
        ] {
            assert!(
                matches!(
                    LanguageTag::new(tag),
                    Err(MetadataViolation::InvalidLanguageTag { .. })
                ),
                "{tag}"
            );
        }
    }

    /// §7.9.4 Requirement Metadata8 — M, FT, K, MI.
    #[test]
    fn req_core_metadata_uom_values() {
        let cases = [
            (UnitOfMeasure::Meters, "M"),
            (UnitOfMeasure::Feet, "FT"),
            (UnitOfMeasure::Kilometers, "K"),
            (UnitOfMeasure::Miles, "MI"),
        ];
        for (uom, text) in cases {
            assert_eq!(uom.as_str(), text);
            assert_eq!(UnitOfMeasure::parse(text), Ok(uom));
        }
        assert!(matches!(
            UnitOfMeasure::parse("CM"),
            Err(MetadataViolation::UnknownUnitOfMeasure { .. })
        ));
    }

    /// §7.9.4.1 — the builder enforces every mandatory element.
    #[test]
    fn req_core_metadata_global_builder_requires_mandatory() {
        let missing_description = GlobalMetadata::builder()
            .id("x")
            .title("t")
            .contact_point("c")
            .created(dt("2026-07-14T09:30:00Z"))
            .language(LanguageTag::new("en").unwrap())
            .standard(MetadataStandard::Dcat)
            .encoding(MetadataEncoding::Json)
            .uom(UnitOfMeasure::Meters)
            .build();
        assert_eq!(
            missing_description,
            Err(MetadataViolation::MissingElement {
                element: "description"
            })
        );
        // Present but empty is also missing.
        let empty_id = GlobalMetadata::builder()
            .id("")
            .title("t")
            .description("d")
            .contact_point("c")
            .created(dt("2026-07-14T09:30:00Z"))
            .language(LanguageTag::new("en").unwrap())
            .standard(MetadataStandard::Dcat)
            .encoding(MetadataEncoding::Json)
            .uom(UnitOfMeasure::Meters)
            .build();
        assert_eq!(
            empty_id,
            Err(MetadataViolation::MissingElement { element: "ID" })
        );
    }

    /// §7.9.4.1 + Metadata8-B — wire names match the spec table and the UoM
    /// element is named `uom`; datetimes serialize in canonical `Z` form.
    #[test]
    fn req_core_metadata_global_wire_names() {
        let value = serde_json::to_value(sample_global(MetadataEncoding::Json)).unwrap();
        let object = value.as_object().unwrap();
        for key in [
            "ID",
            "title",
            "description",
            "contactPoint",
            "created",
            "language",
            "metadataStandard",
            "metadataEncoding",
            "uom",
        ] {
            assert!(object.contains_key(key), "{key}");
        }
        assert_eq!(
            object.len(),
            9,
            "optional elements must be omitted when absent"
        );
        assert_eq!(object["created"], "2026-07-14T09:30:00Z");
        assert_eq!(object["uom"], "M");
    }

    /// §7.9.3.1 Requirement Metadata1 + §7.9.3.3 Metadata3 — the global
    /// metadata file lives in `global_metadata/` and is locatable by link.
    #[test]
    fn req_core_metadata_repository_json_roundtrip_and_locate() {
        let tmp = tempdir().unwrap();
        let layout = DatastoreLayout::create(tmp.path()).unwrap();
        let metadata = sample_global(MetadataEncoding::Json);

        let path = metadata.write_to(&layout).unwrap();
        assert!(path.ends_with("global_metadata/global_metadata.json"));
        assert_eq!(GlobalMetadata::read_from(&layout).unwrap(), metadata);

        let link = GlobalMetadata::locate(&layout).unwrap();
        assert_eq!(link, "/global_metadata/global_metadata.json");
        assert_eq!(
            crate::links::classify_href(&link),
            Ok(crate::links::HrefKind::Relative)
        );
    }

    /// §7.9.3.3 Metadata3 — a datastore without global metadata errors.
    #[test]
    fn req_core_metadata_global_missing_file_errors() {
        let tmp = tempdir().unwrap();
        let layout = DatastoreLayout::create(tmp.path()).unwrap();
        assert!(matches!(
            GlobalMetadata::read_from(&layout),
            Err(MetadataError::Violation(
                MetadataViolation::MissingGlobalMetadata { .. }
            ))
        ));
        assert!(matches!(
            GlobalMetadata::locate(&layout),
            Err(MetadataError::Violation(
                MetadataViolation::MissingGlobalMetadata { .. }
            ))
        ));
    }

    /// §7.9.3.5 — an xml-encoded datastore writes and reads xml.
    #[test]
    fn global_metadata_xml_file_roundtrip() {
        let tmp = tempdir().unwrap();
        let layout = DatastoreLayout::create(tmp.path()).unwrap();
        let mut metadata = sample_global(MetadataEncoding::Xml);
        metadata.license = Some("CC-BY-4.0".into());
        metadata.temporal = Some(Temporal::parse("2025-01-01T00:00:00Z/..").unwrap());

        let path = metadata.write_to(&layout).unwrap();
        assert!(path.ends_with("global_metadata/global_metadata.xml"));
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("<uom>M</uom>"), "{content}");
        assert_eq!(GlobalMetadata::read_from(&layout).unwrap(), metadata);
    }

    /// §7.9.3.6 Metadata6 — UTC is enforced on deserialization too.
    #[test]
    fn req_core_metadata_datetime_enforced_on_deserialize() {
        let json = r#"{
            "ID": "x", "title": "t", "description": "d", "contactPoint": "c",
            "created": "2026-07-14T09:30:00+02:00", "language": "en",
            "metadataStandard": "DCAT", "metadataEncoding": "json", "uom": "M"
        }"#;
        let error = serde_json::from_str::<GlobalMetadata>(json).unwrap_err();
        assert!(error.to_string().contains("UTC"), "{error}");
    }

    /// gpkg is a valid declared encoding (Metadata5) but the core cannot
    /// write that container; profiles do.
    #[test]
    fn gpkg_file_encoding_unsupported_for_write() {
        let tmp = tempdir().unwrap();
        let layout = DatastoreLayout::create(tmp.path()).unwrap();
        assert!(matches!(
            sample_global(MetadataEncoding::Gpkg).write_to(&layout),
            Err(MetadataError::UnsupportedEncoding(MetadataEncoding::Gpkg))
        ));
    }

    /// §7.9.4.2 — mandatory elements and the literal `dataset` type.
    #[test]
    fn resource_metadata_mandatory_and_type() {
        let metadata = ResourceMetadata::new("r1", "RoadNetwork", "Roads");
        metadata.validate().unwrap();

        let mut empty_title = metadata.clone();
        empty_title.title.clear();
        assert_eq!(
            empty_title.validate(),
            Err(MetadataViolation::MissingElement { element: "title" })
        );

        let wrong_type = r#"{"ID":"r","type":"collection","title":"T","description":"D"}"#;
        assert!(serde_json::from_str::<ResourceMetadata>(wrong_type).is_err());
        let value = serde_json::to_value(&metadata).unwrap();
        assert_eq!(value["type"], "dataset");
    }

    /// §7.9.4.2 — `CharacterSetCode` defaults to utf8 and round-trips utf16.
    #[test]
    fn resource_metadata_charset_defaults() {
        let json = r#"{"ID":"r","type":"dataset","title":"T","description":"D"}"#;
        let metadata = ResourceMetadata::from_json_str(json).unwrap();
        assert_eq!(metadata.character_set, CharacterSet::Utf8);
        // utf8 is omitted on the wire; utf16 is explicit.
        let value = serde_json::to_value(&metadata).unwrap();
        assert!(!value.as_object().unwrap().contains_key("CharacterSetCode"));
        let mut utf16 = metadata.clone();
        utf16.character_set = CharacterSet::Utf16;
        let value = serde_json::to_value(&utf16).unwrap();
        assert_eq!(value["CharacterSetCode"], "utf16");
        let back = ResourceMetadata::from_json_str(&utf16.to_json_string().unwrap()).unwrap();
        assert_eq!(back, utf16);
    }

    /// §7.9.4.2 — full record round-trip; `associations` reuse the Links
    /// module and `formats` reuse the Media Types module.
    #[test]
    fn resource_metadata_json_roundtrip_with_associations() {
        let mut metadata = ResourceMetadata::new(
            "tileset-roads",
            "RoadNetwork",
            "This TileSet contains the CDB road and highway network.",
        );
        metadata.keywords = vec!["roads".into(), "streets".into(), "highways".into()];
        metadata.formats = vec![MediaType::GeoPackage];
        metadata.extent = Some(Extent {
            spatial: Some(Bbox {
                west: 42.0,
                south: 12.0,
                east: 45.0,
                north: 19.0,
            }),
            temporal: None,
        });
        metadata.associations = vec![
            Link::new("Tiles/RoadNetwork.gpkg", "item")
                .unwrap()
                .with_media_type("application/geopackage+sqlite3"),
        ];
        metadata.validate().unwrap();

        let json = metadata.to_json_string().unwrap();
        let back = ResourceMetadata::from_json_str(&json).unwrap();
        assert_eq!(back, metadata);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["keywords"].as_array().unwrap().len(), 3);
        assert_eq!(value["formats"][0], "application/geopackage+sqlite3");
    }

    #[test]
    fn resource_metadata_xml_roundtrip() {
        let mut metadata = ResourceMetadata::new("tileset-roads", "RoadNetwork", "Road network");
        metadata.keywords = vec!["roads".into(), "streets".into()];
        metadata.extent = Some(Extent {
            spatial: Some(Bbox {
                west: 42.0,
                south: 12.0,
                east: 45.0,
                north: 19.0,
            }),
            temporal: Some(Temporal::parse("2025-01-01T00:00:00Z/..").unwrap()),
        });
        metadata.associations = vec![Link::new("Tiles/RoadNetwork.gpkg", "item").unwrap()];

        let xml = metadata.to_xml_string().unwrap();
        let back = ResourceMetadata::from_xml_str(&xml).unwrap();
        assert_eq!(back, metadata);
    }

    /// Association links must satisfy the Links module.
    #[test]
    fn resource_metadata_invalid_association_detected() {
        let mut metadata = ResourceMetadata::new("r1", "RoadNetwork", "Roads");
        metadata.associations.push(Link {
            href: "bad href".into(),
            rel: "item".into(),
            media_type: None,
            title: None,
        });
        assert!(matches!(
            metadata.validate(),
            Err(MetadataViolation::Link(LinkViolation::InvalidHref { .. }))
        ));
    }

    #[test]
    fn metadata_error_converts_to_cdb_error() {
        let violation = MetadataViolation::MissingElement { element: "ID" };
        let error: crate::CdbError = MetadataError::from(violation).into();
        assert!(matches!(error, crate::CdbError::Metadata(_)));
    }

    /// §7.6.3 Requirement Geom4 /req/core/geometry-mvalue — m-value units
    /// are "specified in the metadata for a given CDB dataset". The
    /// geometry module makes the optional `uom` element conditional on
    /// resource metadata (element name per Metadata8's naming rule).
    #[test]
    fn req_core_geometry_mvalue_resource_uom_roundtrip() {
        let mut record = ResourceMetadata::new("Roads", "Road Network", "Primary roads");
        assert_eq!(record.uom, None);
        let json = record.to_json_string().unwrap();
        assert!(
            !json.contains("\"uom\""),
            "absent uom must not serialize: {json}"
        );

        record.uom = Some(UnitOfMeasure::Meters);
        let json = record.to_json_string().unwrap();
        // `to_json_string` pretty-prints, so the separator is `": "`.
        assert!(json.contains("\"uom\": \"M\""), "wire name/value: {json}");
        let back = ResourceMetadata::from_json_str(&json).unwrap();
        assert_eq!(back.uom, Some(UnitOfMeasure::Meters));

        let xml = record.to_xml_string().unwrap();
        let back = ResourceMetadata::from_xml_str(&xml).unwrap();
        assert_eq!(back.uom, Some(UnitOfMeasure::Meters));
    }

    /// §7.2.6 Requirement Coverages6 + §7.9.4.2 — a coverage instance's
    /// domainSet rides on its resource metadata record (the conditional
    /// element the coverages module introduces, like Geom4's uom).
    #[test]
    fn req_core_coverage_domainset_resource_roundtrip() {
        use crate::coverage::{DomainSet, GridCellEncoding, GridCorner};

        let mut record = ResourceMetadata::new("Elevation", "Terrain Elevation", "Gridded DEM");
        assert_eq!(record.domain_set, None);
        let json = record.to_json_string().unwrap();
        assert!(
            !json.contains("domainSet"),
            "absent domainSet must not serialize: {json}"
        );

        let mut ds = DomainSet::new("m");
        ds.data_null = Some(-32767.0);
        ds.grid_cell_encoding = GridCellEncoding::ValueIsCorner;
        ds.which_corner = Some(GridCorner::LowerLeft);
        record.domain_set = Some(ds.clone());

        let json = record.to_json_string().unwrap();
        assert!(json.contains("\"domainSet\""), "wire name: {json}");
        assert!(json.contains("value-is-corner") && json.contains("lower-left-corner"));
        let back = ResourceMetadata::from_json_str(&json).unwrap();
        assert_eq!(back.domain_set, Some(ds.clone()));

        let xml = record.to_xml_string().unwrap();
        let back = ResourceMetadata::from_xml_str(&xml).unwrap();
        assert_eq!(back.domain_set, Some(ds));
    }

    /// §7.13.5.5 Face Topology Requirement 4 + §7.9.4.2 — a
    /// topologically structured dataset's winding order rides on its
    /// resource metadata record (the conditional element the topology
    /// module introduces; fourth use of the §7.9.4.2 mechanism).
    #[test]
    fn req_core_topology_winding_resource_roundtrip() {
        use crate::topology::WindingOrder;

        let mut record =
            ResourceMetadata::new("Roads", "Road Topology", "Topologically structured roads");
        assert_eq!(record.winding_order, None);
        let json = record.to_json_string().unwrap();
        assert!(
            !json.contains("windingOrder"),
            "absent windingOrder must not serialize: {json}"
        );

        record.winding_order = Some(WindingOrder::Clockwise);
        let json = record.to_json_string().unwrap();
        assert!(json.contains("\"windingOrder\""), "wire name: {json}");
        assert!(json.contains("clockwise"));
        let back = ResourceMetadata::from_json_str(&json).unwrap();
        assert_eq!(back.winding_order, Some(WindingOrder::Clockwise));

        let xml = record.to_xml_string().unwrap();
        let back = ResourceMetadata::from_xml_str(&xml).unwrap();
        assert_eq!(back.winding_order, Some(WindingOrder::Clockwise));
    }

    /// §7.10.2.5 Requirement Tiling8 /req/core/tiling-tilingscheme-definition
    /// — the tiling-scheme definition rides the global metadata record (the
    /// conditional element the tiling module introduces; third use of the
    /// §7.9.4.2 mechanism, first on the global table).
    #[test]
    fn req_core_tiling_tilingscheme_global_roundtrip() {
        use crate::tiling::TilingScheme;
        let tmp = tempdir().unwrap();
        let layout = DatastoreLayout::create(tmp.path()).unwrap();
        let mut metadata = sample_global(MetadataEncoding::Json);
        assert_eq!(metadata.tiling_scheme, None);
        let json = metadata.to_json_string().unwrap();
        assert!(!json.contains("tilingScheme"));

        metadata.tiling_scheme = Some(TilingScheme::cdb1_global_grid());
        metadata.write_to(&layout).unwrap();
        let back = GlobalMetadata::read_from(&layout).unwrap();
        assert_eq!(back.tiling_scheme, Some(TilingScheme::cdb1_global_grid()));

        let xml = metadata.to_xml_string().unwrap();
        let back = GlobalMetadata::from_xml_str(&xml).unwrap();
        assert_eq!(back.tiling_scheme, Some(TilingScheme::cdb1_global_grid()));
    }

    /// §7.10.2.5 Requirement Tiling8 /req/core/tiling-tilingscheme-definition
    /// — the builder sets the conditional `tilingScheme` element, so a tiled
    /// datastore's global record is constructible through the fluent API and
    /// not only by mutating a built struct. The element stays optional: a
    /// builder that is never told a scheme yields an untiled record.
    #[test]
    fn req_core_tiling_tilingscheme_builder_sets_element() {
        use crate::tiling::TilingScheme;
        let built = GlobalMetadata::builder()
            .id("id")
            .title("Title")
            .description("Description")
            .contact_point("contact")
            .created(Utc::now())
            .language(LanguageTag::new("en").unwrap())
            .standard(MetadataStandard::Dcat)
            .encoding(MetadataEncoding::Json)
            .uom(UnitOfMeasure::Meters)
            .tiling_scheme(TilingScheme::cdb1_global_grid())
            .build()
            .unwrap();
        assert_eq!(built.tiling_scheme, Some(TilingScheme::cdb1_global_grid()));
        assert!(built.to_json_string().unwrap().contains("tilingScheme"));

        let untiled = GlobalMetadata::builder()
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
            .unwrap();
        assert_eq!(untiled.tiling_scheme, None);
    }
}
