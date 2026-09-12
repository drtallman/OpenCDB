//! Requirements-class vocabulary (Annex A).

use std::fmt;

/// A mandatory CDB Core requirements class (Annex A). The abstract core
/// bundles all five into `/conf/minimal-core`; a profile declares conformance
/// per class. `#[non_exhaustive]` leaves room for the optional classes later
/// phases add (tiling, coverage, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum RequirementsClass {
    Crs,
    FileNaming,
    FileStructure,
    Links,
    Metadata,
}

impl RequirementsClass {
    /// The five classes Annex A `/conf/minimal-core` makes mandatory for every
    /// profile. Declaration order equals [`Ord`] order — the order a
    /// conformance report lists them.
    pub const MANDATORY: [RequirementsClass; 5] = [
        RequirementsClass::Crs,
        RequirementsClass::FileNaming,
        RequirementsClass::FileStructure,
        RequirementsClass::Links,
        RequirementsClass::Metadata,
    ];

    /// The class's short name, used in its conformance-class URI (Annex A).
    pub fn as_str(self) -> &'static str {
        match self {
            RequirementsClass::Crs => "crs",
            RequirementsClass::FileNaming => "file-naming",
            RequirementsClass::FileStructure => "file-structure",
            RequirementsClass::Links => "links",
            RequirementsClass::Metadata => "metadata",
        }
    }

    /// The requirements-module URI this class encodes (§7.3–§7.9). The
    /// trailing hyphen on the metadata URI is verbatim from the draft (§7.9).
    pub fn requirements_uri(self) -> &'static str {
        match self {
            RequirementsClass::Crs => "/req/core/data-representation",
            RequirementsClass::FileNaming => "/req/core/naming-system",
            RequirementsClass::FileStructure => "/req/core/file-system",
            RequirementsClass::Links => "/req/core/links",
            RequirementsClass::Metadata => "/req/core/metadata-",
        }
    }

    /// The Annex A conformance-class URI a profile declares for this class.
    /// The published draft omits some path separators; this normalizes them to
    /// the well-formed OGC pattern
    /// `http://www.opengis.net/spec/CDB/2.0/conf/<profile>/<class>`.
    pub fn conformance_uri(self, profile_name: &str) -> String {
        format!(
            "http://www.opengis.net/spec/CDB/2.0/conf/{profile_name}/{}",
            self.as_str()
        )
    }
}

impl fmt::Display for RequirementsClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Annex A `/conf/minimal-core` — the five mandatory requirements classes,
    /// with the spec's short names.
    #[test]
    fn conf_core_minimal_mandatory_classes_enumerated() {
        assert_eq!(
            RequirementsClass::MANDATORY,
            [
                RequirementsClass::Crs,
                RequirementsClass::FileNaming,
                RequirementsClass::FileStructure,
                RequirementsClass::Links,
                RequirementsClass::Metadata,
            ]
        );
        assert_eq!(RequirementsClass::Crs.as_str(), "crs");
        assert_eq!(RequirementsClass::FileNaming.as_str(), "file-naming");
        assert_eq!(RequirementsClass::FileStructure.as_str(), "file-structure");
        assert_eq!(RequirementsClass::Links.as_str(), "links");
        assert_eq!(RequirementsClass::Metadata.as_str(), "metadata");
    }

    /// Annex A conformance table — the conformance-class URI for a profile.
    /// The published draft omits some path separators; the crate normalizes
    /// them to the well-formed OGC pattern.
    #[test]
    fn conf_core_conformance_uri_pattern() {
        for class in RequirementsClass::MANDATORY {
            assert_eq!(
                class.conformance_uri("simulation"),
                format!(
                    "http://www.opengis.net/spec/CDB/2.0/conf/simulation/{}",
                    class.as_str()
                )
            );
        }
        assert_eq!(
            RequirementsClass::Metadata.conformance_uri("simulation"),
            "http://www.opengis.net/spec/CDB/2.0/conf/simulation/metadata"
        );
    }

    /// §7.3–§7.9 — each requirements class maps to its spec-module URI. The
    /// trailing hyphen on the metadata URI is verbatim from the draft (§7.9).
    #[test]
    fn requirements_class_uris_match_spec_modules() {
        assert_eq!(
            RequirementsClass::Crs.requirements_uri(),
            "/req/core/data-representation"
        );
        assert_eq!(
            RequirementsClass::FileNaming.requirements_uri(),
            "/req/core/naming-system"
        );
        assert_eq!(
            RequirementsClass::FileStructure.requirements_uri(),
            "/req/core/file-system"
        );
        assert_eq!(
            RequirementsClass::Links.requirements_uri(),
            "/req/core/links"
        );
        assert_eq!(
            RequirementsClass::Metadata.requirements_uri(),
            "/req/core/metadata-"
        );
    }
}
