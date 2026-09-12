//! Requirements-class vocabulary (Annex A).

use std::fmt;

/// A CDB Core requirements class (§6; Annex A). Five are **mandatory** — the
/// abstract core bundles them into `/conf/minimal-core` — and six are
/// **optional**, binding only once the content they govern is present. A
/// profile declares conformance per class
/// ([`crate::profiles::ApplicationProfile::conformance_classes`]).
///
/// Variants are declared **alphabetically**, and that is load-bearing:
/// [`Ord`] order is the order a [`crate::conformance::ConformanceReport`]
/// lists its classes.
///
/// `#[non_exhaustive]` because the two grid extensions (§7.11 CDB1GlobalGrid,
/// §7.12 GNOSISGlobalGrid, TCE1–7) are plausible future classes and adding a
/// variant post-1.0 would otherwise be breaking; downstream `match` therefore
/// needs a wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum RequirementsClass {
    /// Optional: Attribution (§7.1, Attr1–Attr2).
    Attribution,
    /// Optional: Coverages (§7.2, Coverages1–Coverages8).
    Coverages,
    /// Mandatory: Coordinate Reference System (§7.3, CRS2–CRS7 + VCRS1–3).
    Crs,
    /// Mandatory: File Naming (§7.4, Name1–Name7).
    FileNaming,
    /// Mandatory: File Structure (§7.5, File1–File6).
    FileStructure,
    /// Optional: Geometry (§7.6, Geom1–Geom6).
    Geometry,
    /// Mandatory: Links (§7.7, Link1–Link4).
    Links,
    /// Mandatory: Metadata (§7.9, Metadata1–Metadata8).
    Metadata,
    /// Optional: the abstract Tiling module (§7.10, Tiling1–Tiling10).
    Tiling,
    /// Optional: Topology (§7.13, Topo1–Topo6 + Face1–Face4).
    Topology,
    /// Optional: Versioning (§7.14, V1–V6).
    Versioning,
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

    /// The six optional classes: each is abstract until the datastore holds
    /// the content it governs, at which point its requirements bind in full.
    /// A profile declares the ones it supports; declaring one costs nothing
    /// when the datastore holds no such content. In [`Ord`] order.
    pub const OPTIONAL: [RequirementsClass; 6] = [
        RequirementsClass::Attribution,
        RequirementsClass::Coverages,
        RequirementsClass::Geometry,
        RequirementsClass::Tiling,
        RequirementsClass::Topology,
        RequirementsClass::Versioning,
    ];

    /// Every class the core defines — [`Self::MANDATORY`] and
    /// [`Self::OPTIONAL`] merged, in [`Ord`] order.
    pub const ALL: [RequirementsClass; 11] = [
        RequirementsClass::Attribution,
        RequirementsClass::Coverages,
        RequirementsClass::Crs,
        RequirementsClass::FileNaming,
        RequirementsClass::FileStructure,
        RequirementsClass::Geometry,
        RequirementsClass::Links,
        RequirementsClass::Metadata,
        RequirementsClass::Tiling,
        RequirementsClass::Topology,
        RequirementsClass::Versioning,
    ];

    /// The class's short name, used in its conformance-class URI (Annex A):
    /// the variant's name in kebab case.
    pub fn as_str(self) -> &'static str {
        match self {
            RequirementsClass::Attribution => "attribution",
            RequirementsClass::Coverages => "coverages",
            RequirementsClass::Crs => "crs",
            RequirementsClass::FileNaming => "file-naming",
            RequirementsClass::FileStructure => "file-structure",
            RequirementsClass::Geometry => "geometry",
            RequirementsClass::Links => "links",
            RequirementsClass::Metadata => "metadata",
            RequirementsClass::Tiling => "tiling",
            RequirementsClass::Topology => "topology",
            RequirementsClass::Versioning => "versioning",
        }
    }

    /// The requirements-module URI this class encodes (§7.1–§7.14). Two draft
    /// quirks are reproduced or corrected keyed on meaning, the crate's
    /// standing convention:
    ///
    /// - the trailing hyphen on the metadata and coverages URIs is verbatim
    ///   from the draft (§7.9, §7.2.2);
    /// - the Tiling-Abstract class box (§7.10.2) labels itself
    ///   `/req/core/geometry-`, a copy-paste from §7.6 that would collide with
    ///   [`RequirementsClass::Geometry`]; the meaning-correct
    ///   `/req/core/tiling` is used here (the same normalization
    ///   [`crate::tiling`] documents).
    pub fn requirements_uri(self) -> &'static str {
        match self {
            RequirementsClass::Attribution => "/req/core/attributes",
            RequirementsClass::Coverages => "/req/core/coverages-",
            RequirementsClass::Crs => "/req/core/data-representation",
            RequirementsClass::FileNaming => "/req/core/naming-system",
            RequirementsClass::FileStructure => "/req/core/file-system",
            RequirementsClass::Geometry => "/req/core/geometry",
            RequirementsClass::Links => "/req/core/links",
            RequirementsClass::Metadata => "/req/core/metadata-",
            RequirementsClass::Tiling => "/req/core/tiling",
            RequirementsClass::Topology => "/req/core/topology",
            RequirementsClass::Versioning => "/req/core/versioning",
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

    /// §6 — the eleven requirements classes the core defines: the five Annex A
    /// mandatory classes plus the six optional ones (§7.1 Attribution, §7.2
    /// Coverages, §7.6 Geometry, §7.10 Tiling, §7.13 Topology, §7.14
    /// Versioning). `ALL` is the disjoint union of `MANDATORY` and `OPTIONAL`,
    /// and is in [`Ord`] order — the order a report lists its classes.
    #[test]
    fn req_core_conformance_eleven_classes_partitioned() {
        assert_eq!(
            RequirementsClass::ALL,
            [
                RequirementsClass::Attribution,
                RequirementsClass::Coverages,
                RequirementsClass::Crs,
                RequirementsClass::FileNaming,
                RequirementsClass::FileStructure,
                RequirementsClass::Geometry,
                RequirementsClass::Links,
                RequirementsClass::Metadata,
                RequirementsClass::Tiling,
                RequirementsClass::Topology,
                RequirementsClass::Versioning,
            ]
        );
        // `ALL` is sorted: Ord order is the report's listing order.
        let mut sorted = RequirementsClass::ALL;
        sorted.sort();
        assert_eq!(sorted, RequirementsClass::ALL);

        // MANDATORY (5) and OPTIONAL (6) partition ALL, disjointly.
        assert_eq!(RequirementsClass::MANDATORY.len(), 5);
        assert_eq!(RequirementsClass::OPTIONAL.len(), 6);
        for class in RequirementsClass::ALL {
            let mandatory = RequirementsClass::MANDATORY.contains(&class);
            let optional = RequirementsClass::OPTIONAL.contains(&class);
            assert!(mandatory ^ optional, "{class}");
        }
        // Both consts are themselves in Ord order.
        let mut mandatory = RequirementsClass::MANDATORY;
        mandatory.sort();
        assert_eq!(mandatory, RequirementsClass::MANDATORY);
        let mut optional = RequirementsClass::OPTIONAL;
        optional.sort();
        assert_eq!(optional, RequirementsClass::OPTIONAL);
    }

    /// Annex A conformance table — the six optional classes spell their short
    /// names the way the mandatory five do (the kebab-cased class name), and
    /// each maps to its §7 requirements-module URI.
    #[test]
    fn req_core_conformance_optional_class_uris() {
        for (class, short, module) in [
            (
                RequirementsClass::Attribution,
                "attribution",
                "/req/core/attributes",
            ),
            (
                RequirementsClass::Coverages,
                "coverages",
                "/req/core/coverages-",
            ),
            (
                RequirementsClass::Geometry,
                "geometry",
                "/req/core/geometry",
            ),
            (RequirementsClass::Tiling, "tiling", "/req/core/tiling"),
            (
                RequirementsClass::Topology,
                "topology",
                "/req/core/topology",
            ),
            (
                RequirementsClass::Versioning,
                "versioning",
                "/req/core/versioning",
            ),
        ] {
            assert_eq!(class.as_str(), short);
            assert_eq!(class.to_string(), short);
            assert_eq!(class.requirements_uri(), module);
            assert_eq!(
                class.conformance_uri("simulation"),
                format!("http://www.opengis.net/spec/CDB/2.0/conf/simulation/{short}")
            );
        }
    }
}
