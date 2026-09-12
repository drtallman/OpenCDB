//! The two finding kinds a conformance run produces: [`CdbViolation`] (a
//! spec **SHALL** failure) and [`CdbWarning`] (a spec **SHOULD** finding).
//!
//! The split is the crate's standing policy and is never blurred: a
//! violation is a `thiserror` `Error`, a warning is `Display` only and is
//! deliberately not an `Error`. Each finding names the
//! [`RequirementsClass`] it belongs to, which is how a
//! [`crate::conformance::ConformanceReport`] buckets it.
//!
//! Both kinds also carry a **stable code** ([`CdbViolation::code`],
//! [`CdbWarning::code`]): the OGC clause the finding is about, normalized to
//! one whitespace-free absolute token. The code vocabulary lives in this
//! module as a set of exhaustive `match`es over every requirements module's
//! violation and warning enum — one auditable table rather than a method per
//! module — so a new variant cannot ship without a code, and the mapping can
//! be read against Annex A in one place. It is what both finding kinds
//! serialize under.

use std::fmt;

use thiserror::Error;

use crate::attribution::AttributionViolation;
use crate::conformance::RequirementsClass;
use crate::coverage::{CoverageViolation, CoverageWarning};
use crate::crs::{CrsViolation, CrsWarning};
use crate::geometry::GeometryViolation;
use crate::hierarchy::{HierarchyViolation, HierarchyWarning};
use crate::links::LinkViolation;
use crate::metadata::MetadataViolation;
use crate::naming::{NamingViolation, NamingWarning};
use crate::tiling::{TilingViolation, TilingWarning};
use crate::topology::TopologyViolation;
use crate::versioning::VersioningViolation;

/// A datastore-wide SHALL violation, gathering every requirements module's
/// violation plus the two profile-layer findings Annex A `/conf/minimal-core`
/// introduces. Each variant maps to exactly one [`RequirementsClass`] via
/// [`CdbViolation::class`], which is how a
/// [`crate::conformance::ConformanceReport`] buckets it.
///
/// `PartialEq` but deliberately **not** `Eq`: [`TilingViolation`] carries
/// `f64` fields (`CoordinateOutOfRange`) and is itself not `Eq`, so folding
/// it in here costs the total-equality derive. Comparison with `==` and
/// `assert_eq!` is unaffected.
#[derive(Debug, Error, Clone, PartialEq)]
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
    /// An Attribution violation (spec §7.1).
    #[error(transparent)]
    Attribution(#[from] AttributionViolation),
    /// A Coverages violation (spec §7.2).
    #[error(transparent)]
    Coverage(#[from] CoverageViolation),
    /// A Geometry violation (spec §7.6).
    #[error(transparent)]
    Geometry(#[from] GeometryViolation),
    /// A Tiling violation (spec §7.10–§7.12).
    #[error(transparent)]
    Tiling(#[from] TilingViolation),
    /// A Topology violation (spec §7.13).
    #[error(transparent)]
    Topology(#[from] TopologyViolation),
    /// A Versioning violation (spec §7.14).
    #[error(transparent)]
    Versioning(#[from] VersioningViolation),
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
    ///
    /// A [`MetadataViolation`] *nested inside* an optional class's violation
    /// (`CoverageViolation::Metadata` and its siblings) is **not** re-filed:
    /// it is that class's verdict on its instance — "this coverage is
    /// non-conformant because its record is" — and the record's own Metadata
    /// finding is filed separately by the metadata stage. Only the
    /// Metadata→Links normalization crosses classes, because an association
    /// link has no home of its own.
    pub fn class(&self) -> RequirementsClass {
        match self {
            CdbViolation::Naming(_) => RequirementsClass::FileNaming,
            CdbViolation::Hierarchy(_) => RequirementsClass::FileStructure,
            CdbViolation::Link(_) => RequirementsClass::Links,
            CdbViolation::Metadata(MetadataViolation::Link(_)) => RequirementsClass::Links,
            CdbViolation::Metadata(_) => RequirementsClass::Metadata,
            CdbViolation::Crs(_) => RequirementsClass::Crs,
            CdbViolation::Attribution(_) => RequirementsClass::Attribution,
            CdbViolation::Coverage(_) => RequirementsClass::Coverages,
            CdbViolation::Geometry(_) => RequirementsClass::Geometry,
            CdbViolation::Tiling(_) => RequirementsClass::Tiling,
            CdbViolation::Topology(_) => RequirementsClass::Topology,
            CdbViolation::Versioning(_) => RequirementsClass::Versioning,
            CdbViolation::MissingConformanceDeclaration { class, .. } => *class,
            CdbViolation::DeclarationMismatch { class, .. } => *class,
        }
    }

    /// The **stable machine-readable code** for this violation: the OGC
    /// clause it breaks, in requirement-URI form
    /// (`/req/core/attribute-model-content-B`). It is the field a consumer
    /// keys on — no tool should ever have to parse [`Display`] text, which
    /// carries values and phrasing the crate is free to improve.
    ///
    /// [`Display`]: std::fmt::Display
    ///
    /// Three normalizations make the vocabulary a set of single tokens,
    /// spelled the way [`RequirementsClass::requirements_uri`] spells a
    /// module URI:
    ///
    /// - **always absolute** — `/req/core/…` for a requirement box,
    ///   `/rec/core/…` for a recommendation box, `/per/core/…` for a
    ///   permission box, and `/conf/minimal-core` for Annex A's bundle.
    ///   The draft writes the attribution and versioning slugs without the
    ///   leading solidus and the rest with it; one form is used here.
    /// - **a part letter joins with a hyphen** — the draft's
    ///   `/req/core/attribute-model-content B` becomes
    ///   `…-content-B`, so a code never contains whitespace.
    /// - **the code names the clause, not the severity.** SHALL vs SHOULD is
    ///   the finding's *kind* ([`CdbViolation`] vs [`CdbWarning`]), never its
    ///   code, so a SHOULD the draft wrote inside a requirement box keeps
    ///   that box's path (see [`CdbWarning::code`]).
    ///
    /// Several variants share a code: a code identifies the clause, and one
    /// clause is breakable in more than one way. The mapping is an exhaustive
    /// `match` over every module's violation enum with no wildcard arm, so a
    /// new variant cannot ship without being assigned one.
    pub fn code(&self) -> &'static str {
        match self {
            CdbViolation::Naming(violation) => naming_code(violation),
            CdbViolation::Hierarchy(violation) => hierarchy_code(violation),
            CdbViolation::Link(violation) => link_code(violation),
            CdbViolation::Metadata(violation) => metadata_code(violation),
            CdbViolation::Crs(violation) => crs_code(violation),
            CdbViolation::Attribution(violation) => attribution_code(violation),
            CdbViolation::Coverage(violation) => coverage_code(violation),
            CdbViolation::Geometry(violation) => geometry_code(violation),
            CdbViolation::Tiling(violation) => tiling_code(violation),
            CdbViolation::Topology(violation) => topology_code(violation),
            CdbViolation::Versioning(violation) => versioning_code(violation),
            CdbViolation::MissingConformanceDeclaration { .. } => MINIMAL_CORE_CLAUSE,
            // The clause is already carried: a mismatch is filed against the
            // clause the declaration contradicts.
            CdbViolation::DeclarationMismatch { clause, .. } => clause,
        }
    }
}

/// A violation serializes as a flat, self-describing record — its stable
/// [`code`], the [`class`] it is filed under, the severity that separates it
/// from a [`CdbWarning`], and the human-readable message — never as a mirror
/// of the Rust variant tree. The enum is `#[non_exhaustive]` and its inner
/// families grow every phase; the wire shape freezes at 1.0 and must survive
/// that. Deliberately `Serialize` only, for the reason
/// [`ConformanceReport`]'s own impl records.
///
/// [`ConformanceReport`]: crate::conformance::ConformanceReport
///
/// [`code`]: CdbViolation::code
/// [`class`]: CdbViolation::class
impl serde::Serialize for CdbViolation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_finding(serializer, self.code(), self.class(), "violation", self)
    }
}

/// The one wire shape both finding kinds use, so a consumer can read a
/// violation and a warning with the same four fields and tell them apart by
/// `severity` alone.
fn serialize_finding<S: serde::Serializer>(
    serializer: S,
    code: &str,
    class: RequirementsClass,
    severity: &str,
    message: &dyn fmt::Display,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeStruct;

    let mut finding = serializer.serialize_struct("Finding", 4)?;
    finding.serialize_field("code", code)?;
    finding.serialize_field("class", &class)?;
    finding.serialize_field("severity", severity)?;
    finding.serialize_field("message", &message.to_string())?;
    finding.end()
}

/// Annex A's bundle of the five mandatory classes: the clause a
/// [`CdbViolation::MissingConformanceDeclaration`] cites. It is a
/// *conformance* class, not a requirement, so it has no `/req/core/` box of
/// its own — the one code in the vocabulary outside the `/…/core/` families.
const MINIMAL_CORE_CLAUSE: &str = "/conf/minimal-core";

/// The stable code for a [`NamingViolation`] (spec §7.4).
fn naming_code(violation: &NamingViolation) -> &'static str {
    match violation {
        // No box of its own: an empty name is the naming system's floor.
        NamingViolation::EmptyName | NamingViolation::EmptyPathComponent { .. } => {
            "/req/core/naming-system"
        }
        NamingViolation::ContainsSpace { .. } => "/req/core/name-spaces",
        // ControlCharacter is the crate's portability extension of Name1-B's
        // character rule (see its variant doc); it files under that box.
        NamingViolation::ForbiddenCharacter { .. } | NamingViolation::ControlCharacter { .. } => {
            "/req/core/name-unicode-B"
        }
        NamingViolation::CaseRuleViolation { .. } => "/req/core/name-case",
        NamingViolation::PathTraversal { .. } => "/req/core/file-cdb-root-location",
    }
}

/// The stable code for a [`HierarchyViolation`] (spec §7.5).
fn hierarchy_code(violation: &HierarchyViolation) -> &'static str {
    match violation {
        HierarchyViolation::MissingGlobalMetadata { .. } => "/req/core/file-root-global-metadata",
    }
}

/// The stable code for a [`LinkViolation`] (spec §7.7).
fn link_code(violation: &LinkViolation) -> &'static str {
    match violation {
        LinkViolation::InvalidHref { .. } => "/req/core/link-href",
        LinkViolation::MissingRel => "/req/core/link-rel",
    }
}

/// The stable code for a [`MetadataViolation`] (spec §7.9). A nested
/// [`MetadataViolation::Link`] reports the *Links* code, matching the way
/// [`CdbViolation::class`] files it under Links.
fn metadata_code(violation: &MetadataViolation) -> &'static str {
    match violation {
        // The §7.9.4 element tables, which carry no per-element box.
        MetadataViolation::MissingElement { .. } => "/req/core/metadata-",
        MetadataViolation::UnknownMetadataStandard { .. } => "/req/core/metadata-standard",
        MetadataViolation::UnknownEncoding { .. }
        | MetadataViolation::EncodingMismatch { .. }
        | MetadataViolation::Malformed { .. } => "/req/core/metadata-encoding",
        MetadataViolation::UnknownUnitOfMeasure { .. } => "/req/core/metadata-uom-measure",
        MetadataViolation::InvalidLanguageTag { .. } => "/req/core/metadata-language",
        MetadataViolation::InvalidDateTime { .. } => "/req/core/metadata-datetime",
        MetadataViolation::NotUtc { .. } => "/req/core/metadata-datetime-A",
        MetadataViolation::InvalidTemporalInterval { .. } => "/req/core/metadata-temporal-interval",
        MetadataViolation::MissingGlobalMetadata { .. } => "/req/core/metadata-global",
        MetadataViolation::Link(link) => link_code(link),
    }
}

/// The stable code for a [`CrsViolation`] (spec §7.3). The CRS module's
/// slugs are path-segmented (`/req/core/crs/crsEpoch`), which is the draft's
/// own spelling for this module and is kept verbatim.
fn crs_code(violation: &CrsViolation) -> &'static str {
    match violation {
        CrsViolation::InvalidWkt { .. }
        | CrsViolation::NotACrs { .. }
        | CrsViolation::MissingCrsMetadata { .. } => "/req/core/crs/crsMetadata",
        CrsViolation::NonGeodeticStorageCrs { .. }
        | CrsViolation::CompoundHorizontalNotGeodetic { .. } => {
            "/req/core/crs/storageCrs-valid-value"
        }
        CrsViolation::InconsistentCoordinateUnits { .. } => "/req/core/crs/uom",
        CrsViolation::MissingEpoch => "/req/core/crs/crsEpoch",
        CrsViolation::InvalidEpoch { .. } => "/req/core/crs/crsEpoch-B",
        CrsViolation::CrsAlreadyDefined { .. } => "/req/core/crs/crsStorage",
        CrsViolation::NotAVerticalCrs { .. } => "/req/core/crs/vcrs-topic2",
    }
}

/// The stable code for an [`AttributionViolation`] (spec §7.1). PAttr1 is a
/// *permission* box, so its code keeps the `/per/core/` family.
fn attribution_code(violation: &AttributionViolation) -> &'static str {
    match violation {
        // Attr1-A and Attr2-A both bite; Attr1-A is the primary — a model
        // that specifies nothing specifies no model.
        AttributionViolation::EmptyModel | AttributionViolation::Malformed { .. } => {
            "/req/core/attribute-model-A"
        }
        AttributionViolation::DuplicateId { .. } | AttributionViolation::EmptyId { .. } => {
            "/req/core/attribute-model-content-B"
        }
        AttributionViolation::EmptyName { .. } => "/req/core/attribute-model-content-C",
        AttributionViolation::EmptyDescription { .. } => "/req/core/attribute-model-content-D",
        AttributionViolation::InvalidSchemaUri { .. } => "/per/core/attribute-schema-uri",
        AttributionViolation::InvalidFileName { .. } => "/req/core/attribute-model-C",
    }
}

/// The stable code for a [`CoverageViolation`] (spec §7.2).
fn coverage_code(violation: &CoverageViolation) -> &'static str {
    match violation {
        CoverageViolation::UnknownGridCellEncoding { .. }
        | CoverageViolation::UnknownGridCorner { .. }
        | CoverageViolation::CornerWithoutWhichCorner => "/req/core/coverage-domainSet-F",
        CoverageViolation::EmptyUom => "/req/core/coverage-domainSet-A",
        CoverageViolation::MissingQuantityDefinition { .. } => "/req/core/coverage-domainSet-H",
        CoverageViolation::MissingResourceMetadata => "/req/core/coverage-min-metadata",
        CoverageViolation::MissingDomainSet => "/req/core/coverage-domainSet",
        CoverageViolation::CoverageCrsMismatch { .. } => "/req/core/coverage-crs",
        CoverageViolation::Metadata(metadata) => metadata_code(metadata),
    }
}

/// The stable code for a [`GeometryViolation`] (spec §7.6).
fn geometry_code(violation: &GeometryViolation) -> &'static str {
    match violation {
        GeometryViolation::UnknownGeometryCode { .. }
        | GeometryViolation::ZLengthMismatch { .. }
        | GeometryViolation::MLengthMismatch { .. } => "/req/core/geometry-types",
        GeometryViolation::MissingZUom => "/req/core/geometry-zvalue",
        GeometryViolation::MissingMUom => "/req/core/geometry-mvalue",
        GeometryViolation::ForeignCrs { .. } => "/req/core/geometry-coordinates",
        GeometryViolation::Metadata(metadata) => metadata_code(metadata),
    }
}

/// The stable code for a [`TilingViolation`] (spec §7.10–§7.12).
fn tiling_code(violation: &TilingViolation) -> &'static str {
    match violation {
        // A vocabulary rejection against the two specified extensions; the
        // box that closes the set is the recommendation's.
        TilingViolation::UnknownTilingScheme { .. } => "/rec/core/tiling-extension",
        TilingViolation::SchemeCrsMismatch { .. } => "/req/core/tiling-tilingscheme-crs",
        TilingViolation::SchemeUomMismatch { .. } => "/req/core/tiling-tilingscheme-uom",
        TilingViolation::IncompleteExtent { .. } => "/req/core/tiling-tilingscheme-extent",
        TilingViolation::MissingTilingScheme => "/req/core/tiling-tilingscheme-definition",
        TilingViolation::MissingTilesetKeywords => "/req/core/tiling-tileset-metadata-elements",
        TilingViolation::Cdb1LodOutOfRange { .. } => "/req/core/tiling-extension-tile-tessellate-A",
        TilingViolation::TileOutOfRange { .. } => "/req/core/tiling-extension-tile-tessellate",
        // TCE2's box, shared by both extension grids.
        TilingViolation::GnosisLevelOutOfRange { .. }
        | TilingViolation::MisalignedColumn { .. } => "/req/core/tiling-extension-tms",
        TilingViolation::GnosisTileOutOfRange { .. } => "/req/core/tiling-extension-start-lod",
        TilingViolation::CoordinateOutOfRange { .. } => "/req/core/tiling-extension-uom",
        TilingViolation::Metadata(metadata) => metadata_code(metadata),
    }
}

/// The stable code for a [`TopologyViolation`] (spec §7.13).
fn topology_code(violation: &TopologyViolation) -> &'static str {
    match violation {
        TopologyViolation::DuplicateNodeId { .. } => "/req/core/topology-nodeID",
        TopologyViolation::DuplicateEdgeId { .. } => "/req/core/topology-edgeID",
        TopologyViolation::DuplicateFaceId { .. } => "/req/core/topology-faceID",
        TopologyViolation::UnknownNodeId { .. } => "/req/core/topology-edge-dir",
        // Raised from both the clip (§7.13.4.7) and the face-boundary
        // (§7.13.5.4) paths, so it names neither: Topo3 is the clause that
        // makes an edge identifier mean something, and a reference to an id
        // no edge carries breaks it from the other side. It may **not** be
        // the bare `/req/core/topology` module URI — that is what the content
        // sweep's undeclared-Topology `DeclarationMismatch` cites, and `code`
        // has to tell the two apart.
        TopologyViolation::UnknownEdgeId { .. } => "/req/core/topology-edgeID",
        TopologyViolation::EdgeHasNoGeometry { .. }
        | TopologyViolation::EdgeGeometryEndpointMismatch { .. }
        | TopologyViolation::EdgeGeometryNotFinite { .. }
        | TopologyViolation::InvalidClipExtent { .. }
        | TopologyViolation::EdgeInFace { .. } => "/req/core/topology-clip",
        TopologyViolation::FaceBoundaryEmpty { .. }
        | TopologyViolation::FaceBoundaryNotChained { .. }
        | TopologyViolation::FaceBoundaryNotClosed { .. } => "/req/core/topology-face-structure",
        TopologyViolation::UnknownWindingOrder { .. }
        | TopologyViolation::WindingOrderUndeclared => "/req/core/topology-winding",
        TopologyViolation::Metadata(metadata) => metadata_code(metadata),
    }
}

/// The stable code for a [`VersioningViolation`] (spec §7.14).
fn versioning_code(violation: &VersioningViolation) -> &'static str {
    match violation {
        VersioningViolation::EmptyCollection
        | VersioningViolation::DuplicateAssetInCollection { .. }
        | VersioningViolation::SequenceExhausted { .. }
        | VersioningViolation::ManifestSequenceGap { .. }
        | VersioningViolation::MalformedManifest { .. } => "/req/core/versioning-collection",
        VersioningViolation::EmptyState { .. } | VersioningViolation::AssetStateMissing { .. } => {
            "/req/core/versioning-transitory"
        }
        // V1 has one part, A, and the crate's convention (errata §7 row 6) is
        // to hyphen-join a part letter so the code stays one token. The part
        // letter is load-bearing here beyond tidiness: V1's box URI collides
        // with the versioning *class* URI (errata row 9), which is what the
        // content sweep's undeclared-Versioning `DeclarationMismatch` cites,
        // so a bare `/req/core/versioning` would make `code` unable to
        // separate "undeclared versioning content" from "this collection
        // addressed a reserved tree".
        VersioningViolation::InvalidAssetPath { .. }
        | VersioningViolation::AssetInReservedTree { .. } => "/req/core/versioning-A",
        // Rollback preconditions: both identify *which* collection is being
        // addressed, which is Requirement V2's subject.
        VersioningViolation::UnknownCollection { .. }
        | VersioningViolation::NotLatestCollection { .. } => "/req/core/versioning-collection",
        VersioningViolation::AssetAlreadyExists { .. } => "/req/core/versioning-functions-A",
        // V4-B (delete) and V4-C (update) both reach it, so no part letter.
        VersioningViolation::AssetMissing { .. } => "/req/core/versioning-functions",
        VersioningViolation::ResourceRecordMissing { .. } => "/req/core/versioning-metadata-C",
    }
}

/// A datastore-wide SHOULD finding. Deliberately **not** an `Error`: a spec
/// recommendation must never masquerade as a failure. Wraps each module's
/// warning and adds [`CdbWarning::LanguageNotEnglish`] for Recommendation
/// Name3-B (`/req/core/name-language B`).
///
/// Only the modules that *have* SHOULD-level text appear here. Of the six
/// optional classes just two do — Coverages (§7.2.6.1) and Tiling
/// (Recommendation Tiling1) — so Attribution (§7.1), Topology (§7.13), and
/// Versioning (§7.14) have **no** warning variant, and their modules carry no
/// warning type at all. That is a decision reaffirmed across Phases 11, 12,
/// and 13, recorded here so it is not mistaken for an oversight: inventing a
/// warning would promote silence into a recommendation the spec never wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CdbWarning {
    /// A File Naming recommendation finding (spec §7.4).
    Naming(NamingWarning),
    /// A File Structure recommendation finding (spec §7.5).
    Hierarchy(HierarchyWarning),
    /// A CRS recommendation finding (spec §7.3).
    Crs(CrsWarning),
    /// A Coverages recommendation finding (spec §7.2).
    Coverage(CoverageWarning),
    /// A Tiling recommendation finding (spec §7.10).
    Tiling(TilingWarning),
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
            CdbWarning::Coverage(_) => RequirementsClass::Coverages,
            CdbWarning::Tiling(_) => RequirementsClass::Tiling,
        }
    }

    /// The **stable machine-readable code** for this warning, on exactly the
    /// terms [`CdbViolation::code`] documents: the clause the recommendation
    /// belongs to, absolute and whitespace-free.
    ///
    /// A code names the clause; the SHALL/SHOULD split is carried by the
    /// finding's kind, not by its code. The draft writes several of its
    /// SHOULDs as lettered parts *inside* requirement boxes — Name1-A inside
    /// `/req/core/name-unicode`, Name3-B inside `/req/core/name-language`,
    /// Name7-B inside `/req/core/name-extensions` — so those codes keep the
    /// `/req/core/` path and are warnings all the same. Recommendations with
    /// boxes of their own (`/rec/core/file-hierarchy-root-name`,
    /// `/rec/core/crs/crs-definition`, `/rec/core/tiling-extension`) keep
    /// theirs. Nothing here is promoted or demoted by its spelling.
    pub fn code(&self) -> &'static str {
        match self {
            CdbWarning::Naming(NamingWarning::NonAscii { .. }) => "/req/core/name-unicode-A",
            CdbWarning::Naming(NamingWarning::NonSpecExtension { .. }) => {
                "/req/core/name-extensions-B"
            }
            CdbWarning::Hierarchy(HierarchyWarning::EmptyFolder(_)) => {
                "/req/core/name-empty-folders-A"
            }
            CdbWarning::Hierarchy(HierarchyWarning::RootNameNotCdb { .. }) => {
                "/rec/core/file-hierarchy-root-name"
            }
            CdbWarning::Crs(CrsWarning::NotWgs84 { .. }) => "/rec/core/crs/crs-definition",
            // §7.2.6.1's disconnected-environment prose sits under the
            // domainSet `uom` element's own requirement part; it has no box.
            CdbWarning::Coverage(CoverageWarning::UomLooksLikeUri { .. }) => {
                "/req/core/coverage-domainSet-A"
            }
            CdbWarning::Tiling(TilingWarning::NonExtensionScheme { .. }) => {
                "/rec/core/tiling-extension"
            }
            CdbWarning::LanguageNotEnglish { .. } => "/req/core/name-language-B",
        }
    }
}

/// A warning serializes in the same four-field shape a [`CdbViolation`]
/// does, with `severity` the field that tells them apart. `Serialize` only,
/// for the same reason.
impl serde::Serialize for CdbWarning {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_finding(serializer, self.code(), self.class(), "warning", self)
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

impl From<CoverageWarning> for CdbWarning {
    fn from(warning: CoverageWarning) -> Self {
        CdbWarning::Coverage(warning)
    }
}

impl From<TilingWarning> for CdbWarning {
    fn from(warning: TilingWarning) -> Self {
        CdbWarning::Tiling(warning)
    }
}

impl fmt::Display for CdbWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CdbWarning::Naming(warning) => warning.fmt(f),
            CdbWarning::Hierarchy(warning) => warning.fmt(f),
            CdbWarning::Crs(warning) => warning.fmt(f),
            CdbWarning::Coverage(warning) => warning.fmt(f),
            CdbWarning::Tiling(warning) => warning.fmt(f),
            CdbWarning::LanguageNotEnglish { language } => write!(
                f,
                "datastore language {language:?}; English is recommended \
                 (/req/core/name-language B)"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Design spec §7 — every finding carries a **stable machine-readable
    /// code**, the requirement-URI form, so a consumer never parses `Display`
    /// text. Shape invariants hold for every variant (absolute, whitespace-
    /// free, an OGC clause path), and the spot-checks pin the codes the spec
    /// names by hand: Attr2-B is `/req/core/attribute-model-content-B`, a
    /// `DeclarationMismatch` reports the clause it already carries, and a
    /// missing declaration cites Annex A's bundle rather than a `/req/` box.
    #[test]
    fn req_core_conformance_violation_codes_are_stable_uris() {
        let samples: Vec<CdbViolation> = vec![
            NamingViolation::ContainsSpace {
                name: "a b".to_owned(),
            }
            .into(),
            HierarchyViolation::MissingGlobalMetadata {
                root: PathBuf::from("/tmp/cdb"),
            }
            .into(),
            LinkViolation::MissingRel.into(),
            MetadataViolation::MissingElement { element: "ID" }.into(),
            CdbViolation::Metadata(MetadataViolation::Link(LinkViolation::MissingRel)),
            CrsViolation::MissingEpoch.into(),
            AttributionViolation::DuplicateId {
                id: "AL013".to_owned(),
            }
            .into(),
            CoverageViolation::MissingDomainSet.into(),
            GeometryViolation::MissingMUom.into(),
            TilingViolation::MissingTilesetKeywords.into(),
            TopologyViolation::WindingOrderUndeclared.into(),
            VersioningViolation::EmptyCollection.into(),
            CdbViolation::MissingConformanceDeclaration {
                profile: "simulation".to_owned(),
                class: RequirementsClass::Links,
            },
            CdbViolation::DeclarationMismatch {
                profile: "simulation".to_owned(),
                class: RequirementsClass::Crs,
                element: "storage CRS",
                declared: "EPSG:4326".to_owned(),
                found: "EPSG:3857".to_owned(),
                clause: "/req/core/crs/crsStorage",
            },
        ];
        for violation in &samples {
            let code = violation.code();
            assert!(code.starts_with('/'), "{code} is not absolute");
            assert!(
                !code.chars().any(char::is_whitespace),
                "{code} contains whitespace"
            );
            assert!(
                code.starts_with("/req/core/")
                    || code.starts_with("/rec/core/")
                    || code.starts_with("/per/core/")
                    || code.starts_with("/conf/"),
                "{code} is not an OGC clause path"
            );
        }

        // Spot-checks, hand-keyed to the spec's requirement boxes.
        assert_eq!(
            CdbViolation::from(AttributionViolation::DuplicateId {
                id: "AL013".to_owned()
            })
            .code(),
            "/req/core/attribute-model-content-B"
        );
        assert_eq!(
            CdbViolation::from(AttributionViolation::InvalidFileName {
                name: "Vector_Attributes.json".to_owned()
            })
            .code(),
            "/req/core/attribute-model-C"
        );
        assert_eq!(
            CdbViolation::from(NamingViolation::ContainsSpace {
                name: "a b".to_owned()
            })
            .code(),
            "/req/core/name-spaces"
        );
        // A Links violation nested in a metadata record reports the Links code.
        assert_eq!(
            CdbViolation::Metadata(MetadataViolation::Link(LinkViolation::MissingRel)).code(),
            "/req/core/link-rel"
        );
        // A DeclarationMismatch reports the clause it was filed under.
        assert_eq!(
            CdbViolation::DeclarationMismatch {
                profile: "simulation".to_owned(),
                class: RequirementsClass::Crs,
                element: "storage CRS",
                declared: "EPSG:4326".to_owned(),
                found: "EPSG:3857".to_owned(),
                clause: "/req/core/crs/crsStorage",
            }
            .code(),
            "/req/core/crs/crsStorage"
        );
        assert_eq!(
            CdbViolation::MissingConformanceDeclaration {
                profile: "simulation".to_owned(),
                class: RequirementsClass::Links,
            }
            .code(),
            "/conf/minimal-core"
        );
    }

    /// Design spec §7 — warnings carry codes on the same terms. A code names
    /// the *clause*; the SHALL/SHOULD split is carried by the finding's kind,
    /// so a recommendation the draft writes inside a `/req/core/` box (Name1-A
    /// inside `/req/core/name-unicode`) keeps that box's path and is still a
    /// warning. Recommendations with boxes of their own keep `/rec/core/`.
    #[test]
    fn req_core_conformance_warning_codes_are_stable_uris() {
        let samples = vec![
            CdbWarning::Naming(NamingWarning::NonAscii {
                name: "café".to_owned(),
            }),
            CdbWarning::Hierarchy(HierarchyWarning::RootNameNotCdb {
                name: "MyStore".to_owned(),
            }),
            CdbWarning::Crs(CrsWarning::NotWgs84 {
                found: "NTF (Paris)".to_owned(),
            }),
            CdbWarning::Coverage(CoverageWarning::UomLooksLikeUri {
                uom: "urn:ogc:def:uom:EPSG::9001".to_owned(),
            }),
            CdbWarning::Tiling(TilingWarning::NonExtensionScheme {
                id: "MyOwnGrid".to_owned(),
            }),
            CdbWarning::LanguageNotEnglish {
                language: "fr".to_owned(),
            },
        ];
        for warning in &samples {
            let code = warning.code();
            assert!(code.starts_with('/'), "{code} is not absolute");
            assert!(
                !code.chars().any(char::is_whitespace),
                "{code} contains whitespace"
            );
        }
        assert_eq!(
            CdbWarning::Naming(NamingWarning::NonAscii {
                name: "café".to_owned()
            })
            .code(),
            "/req/core/name-unicode-A"
        );
        assert_eq!(
            CdbWarning::Hierarchy(HierarchyWarning::RootNameNotCdb {
                name: "MyStore".to_owned()
            })
            .code(),
            "/rec/core/file-hierarchy-root-name"
        );
        assert_eq!(
            CdbWarning::LanguageNotEnglish {
                language: "fr".to_owned()
            }
            .code(),
            "/req/core/name-language-B"
        );
    }

    /// Design spec §7 — a finding serializes as a flat, self-describing
    /// record: its stable `code`, the `class` it is filed under, its
    /// `severity`, and the human `message`. Deliberately **not** a mirror of
    /// the Rust variant tree, which is `#[non_exhaustive]` and will grow: the
    /// wire shape freezes at 1.0 and must survive a new variant.
    #[test]
    fn req_core_conformance_finding_serializes_flat() {
        let violation: CdbViolation = NamingViolation::ContainsSpace {
            name: "a b".to_owned(),
        }
        .into();
        let value = serde_json::to_value(&violation).unwrap();
        assert_eq!(value["code"], "/req/core/name-spaces");
        assert_eq!(value["class"], "file-naming");
        assert_eq!(value["severity"], "violation");
        assert_eq!(value["message"], violation.to_string());
        assert_eq!(value.as_object().unwrap().len(), 4);

        let warning = CdbWarning::LanguageNotEnglish {
            language: "fr".to_owned(),
        };
        let value = serde_json::to_value(&warning).unwrap();
        assert_eq!(value["code"], "/req/core/name-language-B");
        assert_eq!(value["class"], "file-naming");
        assert_eq!(value["severity"], "warning");
        assert_eq!(value["message"], warning.to_string());
    }

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

    /// SHALL/SHOULD separation for the optional classes: Coverages (§7.2.6.1)
    /// and Tiling (Recommendation Tiling1) are the only two carrying
    /// SHOULD-level text, so they — and only they — fold into [`CdbWarning`].
    /// §7.1 (Attribution), §7.13 (Topology) and §7.14 (Versioning) contain no
    /// SHOULD, so they deliberately have no warning variant; their modules
    /// carry no warning type at all.
    #[test]
    fn req_core_conformance_optional_class_warnings_wrap() {
        let coverage_warning = CoverageWarning::UomLooksLikeUri {
            uom: "urn:ogc:def:uom:EPSG::9001".to_owned(),
        };
        let wrapped: CdbWarning = coverage_warning.clone().into();
        assert_eq!(wrapped.to_string(), coverage_warning.to_string());
        assert_eq!(wrapped.class(), RequirementsClass::Coverages);

        let tiling_warning = TilingWarning::NonExtensionScheme {
            id: "MyOwnGrid".to_owned(),
        };
        let wrapped: CdbWarning = tiling_warning.clone().into();
        assert_eq!(wrapped.to_string(), tiling_warning.to_string());
        assert_eq!(wrapped.class(), RequirementsClass::Tiling);
    }

    /// Taxonomy for the six optional classes (§7.1, §7.2, §7.6, §7.10,
    /// §7.13, §7.14): each module's violation folds into [`CdbViolation`]
    /// via `From` and `class()` files it under its own class. A metadata
    /// violation *nested* inside an optional-class violation stays under
    /// that class — it is the instance's verdict, and the record's own
    /// Metadata finding is filed separately by the metadata stage.
    #[test]
    fn req_core_conformance_optional_class_violations_wrap() {
        let attribution: CdbViolation = AttributionViolation::EmptyModel.into();
        assert_eq!(attribution.class(), RequirementsClass::Attribution);

        let coverage: CdbViolation = CoverageViolation::MissingDomainSet.into();
        assert_eq!(coverage.class(), RequirementsClass::Coverages);

        let geometry: CdbViolation = GeometryViolation::MissingMUom.into();
        assert_eq!(geometry.class(), RequirementsClass::Geometry);

        let tiling: CdbViolation = TilingViolation::MissingTilesetKeywords.into();
        assert_eq!(tiling.class(), RequirementsClass::Tiling);

        let topology: CdbViolation = TopologyViolation::WindingOrderUndeclared.into();
        assert_eq!(topology.class(), RequirementsClass::Topology);

        let versioning: CdbViolation = VersioningViolation::ManifestSequenceGap {
            expected: 2,
            found: 3,
        }
        .into();
        assert_eq!(versioning.class(), RequirementsClass::Versioning);

        // A delegated metadata violation keeps its optional class.
        let nested: CdbViolation =
            CoverageViolation::Metadata(MetadataViolation::MissingElement { element: "ID" }).into();
        assert_eq!(nested.class(), RequirementsClass::Coverages);
    }

    /// Design spec §7 — [`CdbViolation::code`] is what a machine consumer
    /// keys on, so it has to *discriminate*. The content sweep files an
    /// undeclared-content [`CdbViolation::DeclarationMismatch`] citing the
    /// class's own requirements-module URI
    /// ([`RequirementsClass::requirements_uri`]); any other violation
    /// emitting that same bare URI would be indistinguishable from it on the
    /// wire — "this datastore holds undeclared versioning content" reading
    /// identically to "this collection addressed a reserved tree".
    ///
    /// No violation of a class the sweep can convict may therefore carry its
    /// bare module URI. The five classes the sweep has a signal for are
    /// Attribution, Coverages, Tiling, Topology and Versioning (§4's table);
    /// the sweep never convicts Geometry, and the mandatory five are outside
    /// its scope entirely, which is why `NamingViolation::EmptyName` may keep
    /// `/req/core/naming-system`.
    #[test]
    fn req_core_conformance_finding_codes_discriminate_from_sweep() {
        let swept = [
            RequirementsClass::Attribution,
            RequirementsClass::Coverages,
            RequirementsClass::Tiling,
            RequirementsClass::Topology,
            RequirementsClass::Versioning,
        ];
        // The variants that once carried a bare module URI, plus one
        // already-distinct neighbour per class as a control.
        let violations: Vec<CdbViolation> = vec![
            TopologyViolation::UnknownEdgeId {
                id: crate::topology::EdgeId(9),
            }
            .into(),
            TopologyViolation::WindingOrderUndeclared.into(),
            VersioningViolation::InvalidAssetPath {
                asset: String::new(),
                source: crate::naming::NamingViolation::EmptyName,
            }
            .into(),
            VersioningViolation::AssetInReservedTree {
                asset: "/versions/v000001/manifest.json".to_owned(),
                tree: "versions".to_owned(),
            }
            .into(),
            VersioningViolation::UnknownCollection {
                id: "v000009".to_owned(),
            }
            .into(),
            VersioningViolation::NotLatestCollection {
                id: "v000001".to_owned(),
                latest: "v000002".to_owned(),
            }
            .into(),
            VersioningViolation::EmptyCollection.into(),
            AttributionViolation::EmptyModel.into(),
            CoverageViolation::MissingDomainSet.into(),
            TilingViolation::MissingTilesetKeywords.into(),
        ];

        for violation in &violations {
            for class in swept {
                assert_ne!(
                    violation.code(),
                    class.requirements_uri(),
                    "{violation} is indistinguishable from an undeclared-{class} finding"
                );
            }
        }

        // ... and the sweep's own finding does use the bare URI, so the two
        // sides of the rule are pinned together.
        let mismatch = CdbViolation::DeclarationMismatch {
            profile: "simulation".to_owned(),
            class: RequirementsClass::Versioning,
            element: "versions journal",
            declared: "no versioning conformance class".to_owned(),
            found: "versions/".to_owned(),
            clause: RequirementsClass::Versioning.requirements_uri(),
        };
        assert_eq!(
            mismatch.code(),
            RequirementsClass::Versioning.requirements_uri()
        );
    }
}
