//! The finding-code catalogue: one row per code `opencdb` can put on the
//! wire, and the data behind `cdb-lint explain`.
//!
//! A report says `/req/core/attribute-model-content-B` and stops there,
//! because a code is a stable identifier and prose is not. That is right for
//! a machine and useless to a person, who then has to find the clause in a
//! 65 KB matrix or a 3.9 MB standard. This table closes that gap: for each
//! code, the requirements class the report files it under, the clause number
//! in OGC 23-034, and one line saying what the clause is about.
//!
//! # What a row is, and is not
//!
//! **It is documentation.** The `code` column is held against the library by
//! `tests/catalogue_guard.rs`, which scans `src/conformance/` and demands an
//! exact two-way match, so the *set* of rows cannot fall behind the library.
//! The other three columns are hand-authored from the standard and from
//! `docs/CONFORMANCE.md`, and reviewed by eye. Nothing machine-checks that
//! `/req/core/name-case` really is §7.4.7.
//!
//! **A gloss mirrors its clause.** `explain` describes a clause rather than
//! restating an obligation, so a SHALL becomes the present tense and nothing
//! else changes. Requirement Attr2-B reads "Each attribute in the model
//! SHALL have a unique identifier"; the row reads "Each attribute in the
//! model has a unique identifier". Paraphrasing further would point a reader
//! at a document that says something else, which is worse than saying
//! nothing.
//!
//! **A gloss is not a severity.** The same clause can be reported as a
//! violation or as a warning — `docs/CONFORMANCE.md` errata row 19 has two
//! recommendations carrying `/req/` URIs — so severity belongs to the
//! finding and never to the code. The report says which severity a finding
//! carried; this table says what the clause is about.
//!
//! # Where the columns come from
//!
//! | Column | Source |
//! |---|---|
//! | `code` | the literals in `opencdb`'s `src/conformance/` tree |
//! | `class` | the class the report files the code under — `CdbViolation::class` and `CdbWarning::class`, **not** the section of `CONFORMANCE.md` the requirement is documented in |
//! | `section` | the requirement *box* in OGC 23-034, not the class table that lists the box |
//! | `gloss` | the box's own text, or `CONFORMANCE.md` §4 and §5.1 where the draft has no clause to mirror |
//!
//! The `class` column is the one that surprises. `CONFORMANCE.md` groups by
//! *requirement* and the report buckets by *finding class*, and they differ:
//! `/req/core/file-cdb-root-location` is Requirement File2's clause,
//! documented under File Structure, but its only finding is
//! `NamingViolation::PathTraversal`, which the report files under **File
//! Naming**. Two more like it — `/req/core/name-empty-folders-A` is a §7.4
//! naming recommendation filed under File Structure because only the
//! hierarchy walk can see an empty folder, and `/req/core/link-href` and
//! `/req/core/link-rel` reach reports through `MetadataViolation::Link`,
//! which `CdbViolation::class` re-files under Links. The column follows the
//! report, because the report is what a user is holding when they run
//! `explain`.

/// One row of the catalogue.
///
/// Every field is `&'static str`, so a row can be handed to a renderer or a
/// SARIF rule table without allocating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// The stable finding code, exactly as a report spells it — the string a
    /// consumer keys on. Absolute, whitespace-free, and case-sensitive.
    pub code: &'static str,
    /// The requirements class a report files this code under: a
    /// `RequirementsClass::as_str` token, or [`ANY_CLASS`].
    pub class: &'static str,
    /// The clause in OGC 23-034 that defines it — digits and dots, with no
    /// `§` prefix (`7.4.7`), or an annex clause (`A.2`).
    pub section: &'static str,
    /// One line describing the clause, in the present tense and with no
    /// trailing period.
    pub gloss: &'static str,
}

/// The [`Entry::class`] of a code whose filing class is not fixed.
///
/// Exactly one code needs it. `/conf/minimal-core` is Annex A's bundle of
/// the five mandatory classes, and its finding —
/// `CdbViolation::MissingConformanceDeclaration` — is filed under whichever
/// of those five the profile failed to declare. No single token is honest
/// for it, and picking one would be wrong four times in five.
pub const ANY_CLASS: &str = "*";

/// Every finding code, sorted by [`Entry::code`].
///
/// Sorted because `explain --list` prints the table as it stands and because
/// a sorted source file is one an auditor can read against Annex A;
/// `tests/catalogue_guard.rs` holds the order.
pub const CATALOGUE: &[Entry] = &[
    Entry {
        code: "/conf/minimal-core",
        // Filed under the mandatory class the profile omitted; see ANY_CLASS.
        class: ANY_CLASS,
        section: "A.2",
        gloss: "A datastore implements the minimum set of mandatory conformance classes",
    },
    Entry {
        code: "/per/core/attribute-schema-uri",
        class: "attribution",
        // The §7.1.2 class table's spelling. PAttr1's own box (§7.1.2.2)
        // mislabels itself `/per/core/attribute-model`.
        section: "7.1.2.2",
        gloss: "The vector_attributes file may carry a URI linking to an external attribute schema",
    },
    Entry {
        code: "/rec/core/crs/crs-definition",
        class: "crs",
        section: "7.3.1.3",
        gloss: "Coordinates are expressed in WGS-84 — EPSG:4326 in 2D, EPSG:4979 in 3D",
    },
    Entry {
        code: "/rec/core/file-hierarchy-root-name",
        class: "file-structure",
        section: "7.5.6",
        gloss: "The root of the file and folder hierarchy is named /cdb",
    },
    Entry {
        code: "/rec/core/tiling-extension",
        class: "tiling",
        // Recommendation Tiling1's box, whose `/rec/` prefix the §7.10.2
        // class table contradicts (CONFORMANCE.md errata row 21).
        section: "7.10.2.7",
        gloss: "A tiling scheme complies with one of the two tiling extensions",
    },
    Entry {
        code: "/req/core/attribute-model",
        class: "attribution",
        section: "7.1.2.1",
        gloss: "The attribute model a profile specifies is the model the datastore carries",
    },
    Entry {
        code: "/req/core/attribute-model-A",
        class: "attribution",
        section: "7.1.2.1",
        gloss: "An implementation that implements feature attribution specifies an attribute model",
    },
    Entry {
        code: "/req/core/attribute-model-C",
        class: "attribution",
        section: "7.1.2.1",
        gloss: "The attribute model file is named vector_attributes.xml or vector_attributes.json",
    },
    Entry {
        code: "/req/core/attribute-model-content-B",
        class: "attribution",
        section: "7.1.2.3",
        gloss: "Each attribute in the model has a unique identifier",
    },
    Entry {
        code: "/req/core/attribute-model-content-C",
        class: "attribution",
        section: "7.1.2.3",
        gloss: "Each attribute in the model has a name",
    },
    Entry {
        code: "/req/core/attribute-model-content-D",
        class: "attribution",
        section: "7.1.2.3",
        gloss: "Each attribute in the model has a description",
    },
    Entry {
        code: "/req/core/attributes",
        class: "attribution",
        section: "7.1.2",
        gloss: "The Core Attributes requirements class, which governs feature attribution",
    },
    Entry {
        code: "/req/core/coverage-crs",
        class: "coverages",
        section: "7.2.4",
        gloss: "A coverage conforms to the CRS module, so its CRS is the datastore's CRS",
    },
    Entry {
        code: "/req/core/coverage-domainSet",
        class: "coverages",
        section: "7.2.6",
        gloss: "A coverage instance carries the domainSet metadata elements",
    },
    Entry {
        code: "/req/core/coverage-domainSet-A",
        class: "coverages",
        // Coverages6-A. §7.2.6.1's disconnected-environment SHOULD — a `uom`
        // that is not a URI — is prose under this same part, with no box.
        section: "7.2.6",
        gloss: "One unit of measure covers all the grid cell values of a coverage instance",
    },
    Entry {
        code: "/req/core/coverage-domainSet-F",
        class: "coverages",
        section: "7.2.6",
        gloss: "grid_cell_encoding says how a value is assigned to a cell, and names the corner",
    },
    Entry {
        code: "/req/core/coverage-domainSet-H",
        class: "coverages",
        section: "7.2.6",
        gloss: "quantity_definition describes the values a coverage instance contains",
    },
    Entry {
        code: "/req/core/coverage-min-metadata",
        class: "coverages",
        section: "7.2.5",
        gloss: "A coverage instance carries the resource metadata elements the Metadata module specifies",
    },
    Entry {
        code: "/req/core/coverages-",
        class: "coverages",
        // The trailing hyphen is the draft's own (CONFORMANCE.md errata row 3).
        section: "7.2.2",
        gloss: "The Coverages requirements class, which governs a datastore's coverage content",
    },
    Entry {
        code: "/req/core/crs/crsEpoch",
        class: "crs",
        section: "7.3.1.6",
        gloss: "A datastore on a dynamic datum states an epoch as part of its CRS metadata",
    },
    Entry {
        code: "/req/core/crs/crsEpoch-B",
        class: "crs",
        section: "7.3.1.6",
        gloss: "The epoch is a decimal year in the Gregorian calendar, yyyy.00 starting 1 January",
    },
    Entry {
        code: "/req/core/crs/crsMetadata",
        class: "crs",
        // CRS5's box carries a `2.0` version segment its class table omits
        // (CONFORMANCE.md errata row 20); the code follows the table.
        section: "7.3.1.4",
        gloss: "CRS metadata is stored in the global metadata folder, encoded as WKT-2 for CRS",
    },
    Entry {
        code: "/req/core/crs/crsStorage",
        class: "crs",
        section: "7.3.1.2",
        gloss: "Only one CRS is specified for a datastore, and every coordinate references it",
    },
    Entry {
        code: "/req/core/crs/storageCrs-valid-value",
        class: "crs",
        section: "7.3.1.3",
        gloss: "Only non-projected — geodetic or geographic — coordinate reference systems are used",
    },
    Entry {
        code: "/req/core/crs/uom",
        class: "crs",
        // CRS6's box also carries the `2.0` segment; errata row 20 again.
        section: "7.3.1.5",
        gloss: "One coordinate unit of measure covers the datastore, whatever the geometry type",
    },
    Entry {
        code: "/req/core/crs/vcrs-topic2",
        class: "crs",
        section: "7.3.1.7",
        gloss: "A vertical CRS conforms to ISO 19111:2019 and to the rest of the CRS class",
    },
    Entry {
        code: "/req/core/data-representation",
        class: "crs",
        section: "7.3.1",
        gloss: "The CRS requirements class, which governs how a datastore represents coordinates",
    },
    Entry {
        code: "/req/core/file-cdb-root-location",
        // Requirement File2's clause, documented under File Structure — but
        // its only finding is `NamingViolation::PathTraversal`, which the
        // report files under File Naming. The report wins; see the module docs.
        class: "file-naming",
        section: "7.5.3",
        gloss: "A datastore has one root location, and all its content is reached below it",
    },
    Entry {
        code: "/req/core/file-root-global-metadata",
        class: "file-structure",
        section: "7.5.7",
        gloss: "Global metadata is reachable from the root in a folder named global_metadata",
    },
    Entry {
        code: "/req/core/file-system",
        class: "file-structure",
        section: "7.5.1",
        gloss: "The File/Folder Structure requirements class, which governs a datastore's hierarchy",
    },
    Entry {
        code: "/req/core/geometry",
        class: "geometry",
        section: "7.6.2",
        gloss: "The Geometry requirements class, which governs a datastore's geometry content",
    },
    Entry {
        code: "/req/core/geometry-coordinates",
        class: "geometry",
        section: "7.6.3",
        gloss: "Every coordinate is associated to a CRS, and one geometry's coordinates share it",
    },
    Entry {
        code: "/req/core/geometry-mvalue",
        class: "geometry",
        section: "7.6.3",
        gloss: "Geometry with an m coordinate states the m unit in the dataset's metadata",
    },
    Entry {
        code: "/req/core/geometry-types",
        class: "geometry",
        section: "7.6.2.2",
        gloss: "Geometry uses the Simple Features types the standard's geometry table lists",
    },
    Entry {
        code: "/req/core/geometry-zvalue",
        class: "geometry",
        // Geom3's box spells the slug `geometry-zcoordinate`; the code
        // follows the class table (CONFORMANCE.md errata row 4).
        section: "7.6.3",
        gloss: "Geometry with a z coordinate states the z unit in the global metadata unit of measure",
    },
    Entry {
        code: "/req/core/link-href",
        class: "links",
        section: "7.7",
        gloss: "A link's href is encoded as a URL; relative and absolute links are both allowed",
    },
    Entry {
        code: "/req/core/link-rel",
        class: "links",
        section: "7.7",
        gloss: "A link carries rel, the type or semantics of the relation",
    },
    Entry {
        code: "/req/core/links",
        class: "links",
        section: "7.7",
        gloss: "The Links requirements class, which governs a resource's relationship to another",
    },
    Entry {
        code: "/req/core/metadata-",
        class: "metadata",
        // The trailing hyphen is the draft's own (errata row 3). The code is
        // also `MetadataViolation::MissingElement`'s, because the §7.9.4
        // element tables carry no per-element box (CONFORMANCE.md §5.1).
        section: "7.9.3",
        gloss: "The Global-Metadata requirements class, whose element tables list every element",
    },
    Entry {
        code: "/req/core/metadata-datetime",
        class: "metadata",
        // Requirement Metadata6's bare box URI; part B is the rule that
        // bites, part A having its own code below.
        section: "7.9.3.6",
        gloss: "Date and time is formatted according to RFC 3339, section 5.6",
    },
    Entry {
        code: "/req/core/metadata-datetime-A",
        class: "metadata",
        section: "7.9.3.6",
        gloss: "Date and time in metadata is specified in UTC",
    },
    Entry {
        code: "/req/core/metadata-encoding",
        class: "metadata",
        section: "7.9.3.5",
        gloss: "One encoding — xml, json or gpkg — covers every metadata instance in a datastore",
    },
    Entry {
        code: "/req/core/metadata-global",
        class: "metadata",
        section: "7.9.3.3",
        gloss: "A datastore states a Global_Metadata path or link to where its global metadata is",
    },
    Entry {
        code: "/req/core/metadata-language",
        class: "metadata",
        section: "7.9.3.4",
        gloss: "One language, consistent with IETF BCP 47, covers a datastore",
    },
    Entry {
        code: "/req/core/metadata-standard",
        class: "metadata",
        section: "7.9.3.2",
        gloss: "A datastore uses one of the named metadata standards for all its metadata",
    },
    Entry {
        code: "/req/core/metadata-temporal-interval",
        class: "metadata",
        section: "7.9.3.7",
        gloss: "A temporal geometry is a date-time value or a time interval, bounded or half-bounded",
    },
    Entry {
        code: "/req/core/metadata-uom-measure",
        class: "metadata",
        section: "7.9.4",
        gloss: "One unit of measure covers a datastore, in a metadata element named uom",
    },
    Entry {
        code: "/req/core/name-case",
        class: "file-naming",
        section: "7.4.7",
        gloss: "One case rule — PascalCase, camelCase, Snake_case or kebab-case — covers a datastore",
    },
    Entry {
        code: "/req/core/name-empty-folders-A",
        // A §7.4 naming recommendation, but only the hierarchy walk can see
        // an empty folder, so the report files it under File Structure
        // (CONFORMANCE.md errata row 18).
        class: "file-structure",
        section: "7.4.5",
        gloss: "Empty folders, and resources devoid of content, are avoided",
    },
    Entry {
        code: "/req/core/name-extensions-B",
        class: "file-naming",
        section: "7.4.8",
        gloss: "File types the extension table does not list use industry-standard extensions",
    },
    Entry {
        code: "/req/core/name-language-B",
        class: "file-naming",
        section: "7.4.4",
        gloss: "The one language used for names, paths and folders is English",
    },
    Entry {
        code: "/req/core/name-spaces",
        class: "file-naming",
        section: "7.4.2",
        gloss: "Spaces are not used in any resource path, folder name, or resource name",
    },
    Entry {
        code: "/req/core/name-unicode-A",
        class: "file-naming",
        // Recommendation Name1's box, which carries a `/req/` URI although
        // it states a SHOULD (CONFORMANCE.md errata row 19).
        section: "7.4.3",
        gloss: "Unicode characters and other symbols are not used in any path or resource name",
    },
    Entry {
        code: "/req/core/name-unicode-B",
        class: "file-naming",
        section: "7.4.3",
        gloss: "The listed punctuation characters are not used in any path or resource name",
    },
    Entry {
        code: "/req/core/naming-system",
        class: "file-naming",
        // Also the floor code for an empty name, which has no box of its own
        // (CONFORMANCE.md §5.1).
        section: "7.4.1",
        gloss: "The Resource Path/Name Structure requirements class; its floor is a non-empty name",
    },
    Entry {
        code: "/req/core/tiling",
        class: "tiling",
        // The §7.10.2 class box labels itself `/req/core/geometry-`, a
        // copy-paste from §7.6; the crate normalizes it (errata row 1).
        section: "7.10.2",
        gloss: "The Tiling-Abstract requirements class, which governs a datastore's tiled content",
    },
    Entry {
        code: "/req/core/tiling-extension-start-lod",
        class: "tiling",
        section: "7.12.3.5",
        gloss: "The GNOSISGlobalGrid starts at level 0 as a 2x4 grid of 90-degree tiles",
    },
    Entry {
        code: "/req/core/tiling-extension-tile-tessellate",
        class: "tiling",
        section: "7.11.3.6",
        gloss: "A tile below LoD 1 is one geocell, and each higher LoD subdivides it recursively",
    },
    Entry {
        code: "/req/core/tiling-extension-tile-tessellate-A",
        class: "tiling",
        section: "7.11.3.6",
        gloss: "A CDB1GlobalGrid datastore has an LoD range from -10 to 23",
    },
    Entry {
        code: "/req/core/tiling-extension-tms",
        class: "tiling",
        // TCE2, defined identically in both extensions: §7.11.3.2 for
        // CDB1GlobalGrid and §7.12.3.2 for GNOSISGlobalGrid.
        section: "7.11.3.2",
        gloss: "An extension grid conforms to its 2DTMS tile matrix set definition",
    },
    Entry {
        code: "/req/core/tiling-extension-uom",
        class: "tiling",
        // TCE4, likewise doubled: §7.11.3.4 and §7.12.3.4.
        section: "7.11.3.4",
        gloss: "Tile origins, extents and bounding-box metadata are stated in decimal degrees",
    },
    Entry {
        code: "/req/core/tiling-tileset-metadata-elements",
        class: "tiling",
        section: "7.10.2.6",
        gloss: "A tileset's metadata provides at least ID, Title, Description and Keywords",
    },
    Entry {
        code: "/req/core/tiling-tilingscheme-consistent",
        class: "tiling",
        section: "7.10.2.4.1",
        gloss: "One TilingScheme definition holds throughout a datastore",
    },
    Entry {
        code: "/req/core/tiling-tilingscheme-crs",
        class: "tiling",
        section: "7.10.2.4.2",
        gloss: "A tiling scheme's CRS is the same as, or compatible with, the storage CRS",
    },
    Entry {
        code: "/req/core/tiling-tilingscheme-definition",
        class: "tiling",
        section: "7.10.2.5",
        gloss: "The tiling scheme definition is included in the global CDB metadata",
    },
    Entry {
        code: "/req/core/tiling-tilingscheme-extent",
        class: "tiling",
        section: "7.10.2.4.4",
        gloss: "A tiling scheme's extent covers the entire earth with no gaps",
    },
    Entry {
        code: "/req/core/tiling-tilingscheme-uom",
        class: "tiling",
        section: "7.10.2.4.3",
        gloss: "A tiling scheme's unit of measure is the one the CRS metadata specifies",
    },
    Entry {
        code: "/req/core/topology",
        class: "topology",
        section: "7.13.4",
        gloss: "The Topology requirements class, which governs topologically structured vector data",
    },
    Entry {
        code: "/req/core/topology-clip",
        class: "topology",
        section: "7.13.4.7",
        gloss: "An edge crossing a tile boundary is clipped, and the clip node shares one id in both tiles",
    },
    Entry {
        code: "/req/core/topology-edge-dir",
        class: "topology",
        section: "7.13.4.5",
        gloss: "A directed edge includes the node ids of its start node and its end node",
    },
    Entry {
        code: "/req/core/topology-edgeID",
        class: "topology",
        section: "7.13.4.3",
        gloss: "Every edge in a topologically structured vector dataset has a unique identifier",
    },
    Entry {
        code: "/req/core/topology-face-structure",
        class: "topology",
        // Face3's box carries a `/rec/` prefix although its text is a
        // conditional SHALL (CONFORMANCE.md errata row 8); the code follows
        // the §7.13.5.1 class table.
        section: "7.13.5.4",
        gloss: "A generated face is defined as a list of directed nodes and directed edges",
    },
    Entry {
        code: "/req/core/topology-faceID",
        class: "topology",
        section: "7.13.5.3",
        gloss: "Every face in a topologically structured vector dataset has a unique identifier",
    },
    Entry {
        code: "/req/core/topology-nodeID",
        class: "topology",
        section: "7.13.4.2",
        gloss: "Every node in a topologically structured vector dataset has a unique identifier",
    },
    Entry {
        code: "/req/core/topology-winding",
        class: "topology",
        // Face4's box slug is `-face-winding` and `/rec/`-prefixed; errata
        // row 8 again.
        section: "7.13.5.5",
        gloss: "Where faces are generated, the winding order is documented in dataset metadata",
    },
    Entry {
        code: "/req/core/versioning",
        class: "versioning",
        // Requirement V1's box URI collides with this class URI (errata row
        // 9); the class table at §7.14.1 is what this row cites.
        section: "7.14.1",
        gloss: "The Versioning requirements class, which governs applying and tracking changes",
    },
    Entry {
        code: "/req/core/versioning-A",
        class: "versioning",
        section: "7.14.2",
        gloss: "A datastore supports applying and tracking changes to its assets",
    },
    Entry {
        code: "/req/core/versioning-collection",
        class: "versioning",
        // V2's box text repeats V1's; the concept is read from §7.14.3's
        // prose (CONFORMANCE.md errata row 10).
        section: "7.14.3",
        gloss: "A set of changes applied to a datastore together is a versioning collection",
    },
    Entry {
        code: "/req/core/versioning-functions",
        class: "versioning",
        // V4's bare box URI: parts B (delete) and C (update) both reach it,
        // so the code carries no part letter (CONFORMANCE.md §5.1).
        section: "7.14.5",
        gloss: "An implementation supports deleting and updating assets in a datastore",
    },
    Entry {
        code: "/req/core/versioning-functions-A",
        class: "versioning",
        section: "7.14.5",
        gloss: "An implementation supports creating (adding) assets in a datastore",
    },
    Entry {
        code: "/req/core/versioning-metadata-C",
        class: "versioning",
        section: "7.14.4",
        gloss: "A changed asset's resource metadata records the date and time of the change",
    },
    Entry {
        code: "/req/core/versioning-transitory",
        class: "versioning",
        section: "7.14.7",
        gloss: "An implementation captures a change of state for any geospatial asset",
    },
];

/// The row for `code`, matched **exactly**.
///
/// Case-sensitive, because a code is an identifier and not free text —
/// `TilingSchemeId::parse` sets the same precedent in the library. A caller
/// with a half-remembered spelling wants [`containing`] instead.
///
/// A linear scan over eighty-odd rows costs nothing and cannot go wrong;
/// binary search would make [`CATALOGUE`]'s ordering silently load-bearing
/// for correctness rather than only for presentation.
pub fn lookup(code: &str) -> Option<&'static Entry> {
    CATALOGUE.iter().find(|entry| entry.code == code)
}

/// Every row whose code contains `text`, ASCII-case-insensitively, in
/// catalogue order.
///
/// This is the suggestion search, and it folds case deliberately: its whole
/// job is to rescue a user who typed `-content-b` or pasted a code out of a
/// lowercased log. Being generous here costs nothing, because the result is
/// a list of candidates and never a verdict.
pub fn containing(text: &str) -> Vec<&'static Entry> {
    let needle = text.to_ascii_lowercase();

    CATALOGUE
        .iter()
        .filter(|entry| entry.code.to_ascii_lowercase().contains(&needle))
        .collect()
}
