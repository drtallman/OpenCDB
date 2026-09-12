//! The two finding kinds a conformance run produces: [`CdbViolation`] (a
//! spec **SHALL** failure) and [`CdbWarning`] (a spec **SHOULD** finding).
//!
//! The split is the crate's standing policy and is never blurred: a
//! violation is a `thiserror` `Error`, a warning is `Display` only and is
//! deliberately not an `Error`. Each finding names the
//! [`RequirementsClass`] it belongs to, which is how a
//! [`crate::conformance::ConformanceReport`] buckets it.

use std::fmt;

use thiserror::Error;

use crate::conformance::RequirementsClass;
use crate::coverage::CoverageWarning;
use crate::crs::{CrsViolation, CrsWarning};
use crate::hierarchy::{HierarchyViolation, HierarchyWarning};
use crate::links::LinkViolation;
use crate::metadata::MetadataViolation;
use crate::naming::{NamingViolation, NamingWarning};
use crate::tiling::TilingWarning;

/// A datastore-wide SHALL violation, gathering every requirements module's
/// violation plus the two profile-layer findings Annex A `/conf/minimal-core`
/// introduces. Each variant maps to exactly one [`RequirementsClass`] via
/// [`CdbViolation::class`], which is how a
/// [`crate::conformance::ConformanceReport`] buckets it.
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
}
