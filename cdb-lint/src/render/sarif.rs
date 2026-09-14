//! The `--format sarif` artifact: a SARIF 2.1.0 document, one run,
//! hand-built on `serde_json` (design §6.3).
//!
//! SARIF is what a CI system reads, and it is the one format where honesty
//! rule 1 has to survive translation into somebody else's vocabulary. It
//! does, and the mapping is the design's happiest accident: SARIF already
//! distinguishes the three [`ContentCoverage`] states, so nothing had to be
//! invented.
//!
//! | Report element | `kind` | `level` |
//! |---|---|---|
//! | violation | `fail` | `error` |
//! | warning | `fail` | `warning` |
//! | class, coverage `unchecked` | `review` | `none` |
//! | class, coverage `none` | `notApplicable` | `none` |
//!
//! `review` is SARIF's own term for a result "requiring further analysis by a
//! human", which is precisely what `unchecked` means: content was present and
//! this crate has no datastore-level check for it, so a pass there means "we
//! did not look" and never "we looked and it was fine"
//! (`docs/CONFORMANCE.md` §6).
//!
//! # `level` is written, never omitted
//!
//! SARIF defaults an absent `level` to `warning`. So an omitted level is not
//! silence — it is an assertion, and the wrong one twice over: a violation
//! without a level renders as a warning, and a `review` without one reads as a
//! warning rather than as the unjudged content it stands for. Both fields are
//! therefore explicit on every result. (The 2.1.0 schema also permits `none`
//! alongside a non-`fail` kind, so saying it costs nothing.)
//!
//! # No `pass` results
//!
//! A class that was checked and came back clean emits nothing. The failures,
//! the unjudged, and the inapplicable are what a consumer acts on; a run of
//! green `pass` results would bury them and would tempt a reader to count
//! results instead of reading `run.properties.coverage`, which is the number
//! that actually says how much was looked at.
//!
//! # Every rule, not only the cited ones
//!
//! `tool.driver.rules` carries the whole [`catalogue`] — all of it, on every
//! run. `rules` describes what the *tool* can report, not what this run
//! happened to find, and a stable driver means two runs over two datastores
//! differ only in their results. It also makes "every `ruleId` resolves" true
//! by construction rather than by a filter that could drift.
//!
//! # `baselineState`, under `--baseline` only
//!
//! A `--baseline` run stamps every result with SARIF's own `baselineState`,
//! and uses two of its four values: `unchanged` for a result the baseline
//! already recorded, `new` for one it did not. For a triple the baseline
//! records *n* times, the first *n* results in report order are `unchanged`
//! and the rest are `new`. Which individual result gets which is arbitrary —
//! two results for one triple are interchangeable — but it is **deterministic**,
//! because report order is: `validate` sorts each directory's entries by name,
//! so two runs over the same bytes produce the same document.
//!
//! `absent` is never synthesized. A resolved finding has no result to hang a
//! state on, and inventing one would put a location, a rule, and a message
//! into a document for something this run did not find; the text diff on
//! stderr reports those instead (design §8).
//!
//! A run given no baseline stamps nothing. SARIF reads an absent
//! `baselineState` as unknown, which is exactly the truth when there was
//! nothing to compare against.
//!
//! # No synthesized `helpUri`
//!
//! The draft's only absolute requirement URI — Requirement Link1's box at
//! spec line 1476 — writes `http://www.opengis.net/spec/CDB/2.0/core/link-href`,
//! dropping the `req` segment its own class table carries. No dereferenceable
//! URL can be built from a finding code, and one that 404s is worse than an
//! omitted optional field. `help.text` carries the document's verified
//! identity instead.
//!
//! [`ContentCoverage`]: rusty_cdb::conformance::ContentCoverage

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Component, Path};

use rusty_cdb::attribution::AttributionViolation;
use rusty_cdb::conformance::{
    CdbViolation, CdbWarning, ClassFindings, ConformanceReport, ContentCoverage, RequirementsClass,
};
use rusty_cdb::coverage::CoverageViolation;
use rusty_cdb::crs::CrsViolation;
use rusty_cdb::geometry::GeometryViolation;
use rusty_cdb::hierarchy::{HierarchyViolation, HierarchyWarning};
use rusty_cdb::metadata::MetadataViolation;
use rusty_cdb::naming::{NamingViolation, NamingWarning};
use rusty_cdb::tiling::TilingViolation;
use rusty_cdb::topology::TopologyViolation;
use rusty_cdb::versioning::VersioningViolation;
use serde_json::{Map, Value, json};

use crate::catalogue;
use crate::render::Tally;
use crate::snapshot::{self, BaselineDiff, FindingKey};

/// The published 2.1.0 schema this document is written against.
const SCHEMA: &str =
    "https://docs.oasis-open.org/sarif/sarif/v2.1.0/errata01/os/schemas/sarif-schema-2.1.0.json";

/// The one value SARIF 2.1.0's `version` accepts.
const VERSION: &str = "2.1.0";

/// The `originalUriBaseIds` entry every location is relative to, so a
/// consumer can re-root the whole document by rewriting one URI.
const DATASTORE_ROOT: &str = "DATASTORE_ROOT";

/// The location of a finding that has none of its own: the datastore itself.
const ROOT_URI: &str = ".";

/// OGC 23-034's verified identity — its internal reference number and its
/// external identifier, both read out of the standard's front matter — quoted
/// in every rule's `help.text` in place of a URL that would not resolve.
const SPEC: &str = "OGC 23-034 (http://www.opengis.net/doc/IS/CDB-core/2.0)";

/// SARIF's `baselineState` for a result the baseline already recorded.
const UNCHANGED: &str = "unchanged";

/// SARIF's `baselineState` for a result the baseline did not record.
const NEW: &str = "new";

/// Renders `report` as a pretty-printed SARIF 2.1.0 document plus one newline.
///
/// `baseline` is the comparison a `--baseline` run made, or `None` for a run
/// that compared against nothing; it decides `result.baselineState` and
/// nothing else about the document.
///
/// # Errors
///
/// Returns the serializer's error if the document cannot be serialized, or the
/// sink's if it cannot be written. Either is a fact about the run, and the
/// caller answers it with [`crate::exit::OPERATIONAL`] rather than with a
/// verdict it did not deliver.
pub fn render(
    report: &ConformanceReport,
    baseline: Option<&BaselineDiff>,
    out: &mut dyn Write,
) -> io::Result<()> {
    let document =
        serde_json::to_string_pretty(&document(report, baseline)).map_err(io::Error::other)?;
    writeln!(out, "{document}")
}

/// The whole document: `version`, `$schema`, and the one run.
fn document(report: &ConformanceReport, baseline: Option<&BaselineDiff>) -> Value {
    let tally = Tally::of(report);
    let mut bases = Map::new();
    bases.insert(
        DATASTORE_ROOT.to_owned(),
        json!({ "uri": base_uri(report.root()) }),
    );

    json!({
        "$schema": SCHEMA,
        "version": VERSION,
        "runs": [{
            "tool": {
                "driver": {
                    "name": "cdb-lint",
                    "version": env!("CARGO_PKG_VERSION"),
                    "rules": rules(),
                }
            },
            "originalUriBaseIds": bases,
            "results": results(report, baseline),
            // Honesty rule 2: no output omits coverage. Unlike the JSON
            // artifact — which is the library's frozen wire shape and cannot
            // gain a field, so its tally is owed to stderr — SARIF has a
            // place for it, and the counts belong with the document a machine
            // keeps rather than in a stream it discards.
            "properties": {
                "profile": report.profile(),
                "conformant": report.is_conformant(),
                "coverage": {
                    "checked": tally.checked,
                    "none": tally.no_content,
                    "unchecked": tally.unchecked,
                },
            },
        }],
    })
}

/// The `file:` URI of the datastore root, ending in `/`.
///
/// [`std::path::absolute`] is lexical: it consults the process's working
/// directory but never the filesystem, so a root given relatively still yields
/// an absolute base without a `stat` and without resolving symlinks the report
/// did not resolve either. A `/`-rooted path takes the `file:` scheme; a
/// Windows drive-rooted one takes it through [`windows_file_uri`]; a path
/// that is neither stays as given, encoded, because `file://x/` would read
/// `x` as a host — a different claim entirely. The path's own bytes are
/// encoded, never a lossy replacement of them.
fn base_uri(root: &Path) -> String {
    let absolute = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
    let bytes = absolute.as_os_str().as_encoded_bytes();
    let mut uri = if bytes.first() == Some(&b'/') {
        format!("file://{}", encode_uri(bytes))
    } else if let Some(windows) = windows_file_uri(bytes) {
        windows
    } else {
        encode_uri(bytes)
    };
    if !uri.ends_with('/') {
        // Without the trailing separator, relative resolution against this
        // base would eat its last segment.
        uri.push('/');
    }

    uri
}

/// A Windows drive-rooted path — `C:\srv\cdb` — as `file:///C:/srv/cdb`.
///
/// This is the one absolute shape `std::path::absolute` produces on Windows
/// that does not begin with a solidus; left to the fallback it would encode
/// as `C%3A%5Csrv%5Ccdb`, whose first segment URI-parses as a scheme. The
/// drive colon stays literal — the conventional `file:` spelling — and the
/// separators become `/` before the segments are encoded. Anything else (a
/// UNC path, a relative fallback) is not claimed here.
fn windows_file_uri(bytes: &[u8]) -> Option<String> {
    let &[drive, b':', separator, ref rest @ ..] = bytes else {
        return None;
    };
    if !drive.is_ascii_alphabetic() || !matches!(separator, b'\\' | b'/') {
        return None;
    }
    let forward: Vec<u8> = rest
        .iter()
        .map(|&byte| if byte == b'\\' { b'/' } else { byte })
        .collect();

    Some(format!(
        "file:///{}:/{}",
        char::from(drive),
        encode_uri(&forward)
    ))
}

/// Percent-encodes a path's bytes into a URI reference (RFC 3986).
///
/// `/` stays a separator; every other byte outside the unreserved set and
/// the sub-delimiters a path segment admits is percent-encoded — non-ASCII
/// and non-UTF-8 bytes included, **as themselves**: the grammar goes out of
/// its way to keep a non-UTF-8 datastore lintable, and a URI fabricated from
/// U+FFFD replacements would address a file that does not exist. Requirement
/// Name1 forbids a space in a CDB name — but a *non-conformant* datastore is
/// exactly what this document describes, and a raw space is not a URI.
///
/// `:` is deliberately encoded although RFC 3986 admits it in a path
/// *segment*: a **relative** reference whose first segment carries a raw
/// colon parses as a scheme (`backup:2024` is a URI with scheme `backup`),
/// and `:` sits in the library's own forbidden-character list, so exactly
/// the datastores this tool convicts can put one in the first segment.
/// Percent-encoded, the byte is unambiguous in every position.
fn encode_uri(path: &[u8]) -> String {
    /// RFC 3986's `pchar` less `%` and `:`, plus the `/` separator: what may
    /// stand unescaped anywhere in the path of a URI reference.
    const SAFE: &[u8] = b"-._~!$&'()*+,;=@/";

    let mut encoded = String::with_capacity(path.len());
    for &byte in path {
        if byte.is_ascii_alphanumeric() || SAFE.contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }

    encoded
}

/// Every catalogue row as a `reportingDescriptor`, in catalogue order.
fn rules() -> Vec<Value> {
    catalogue::CATALOGUE.iter().map(rule).collect()
}

/// One rule: the code, the gloss, the clause, and where to read it.
///
/// The class is a property rather than part of the description because a
/// consumer groups by it. [`catalogue::ANY_CLASS`] is the one row that has
/// none: `/conf/minimal-core` is filed under whichever mandatory class the
/// profile omitted, which is a property of the *finding*. Emitting the marker
/// as if it were a class token would tell four readers in five something
/// false, so the rule states no class and the result states the real one.
fn rule(entry: &catalogue::Entry) -> Value {
    let mut properties = Map::new();
    if entry.class != catalogue::ANY_CLASS {
        properties.insert("class".to_owned(), Value::from(entry.class));
    }
    properties.insert("section".to_owned(), Value::from(entry.section));

    json!({
        "id": entry.code,
        "shortDescription": { "text": entry.gloss },
        "help": { "text": format!("{SPEC} §{}", entry.section) },
        "properties": properties,
    })
}

/// Every result, in the report's own class order — which is a function of the
/// datastore and not of the host filesystem (`docs/CONFORMANCE.md`, amendment
/// 21), so two runs over the same bytes produce the same document.
///
/// Within a class the coverage result comes first and the findings follow,
/// mirroring the text report's row-then-findings layout. A class can carry
/// both: the content sweep files a violation against a class whose coverage is
/// `unchecked`, and a document that dropped the `review` there would report a
/// datastore as *more* checked the less its profile declared.
///
/// `seen` counts how many results each triple has already produced, which is
/// what makes "the first *n* are `unchanged`" a rule rather than a wish.
fn results(report: &ConformanceReport, baseline: Option<&BaselineDiff>) -> Vec<Value> {
    let mut results = Vec::new();
    let mut seen: BTreeMap<FindingKey, usize> = BTreeMap::new();
    for (class, findings) in report.classes() {
        if let Some(result) = coverage_result(class, findings, baseline) {
            results.push(result);
        }
        for violation in &findings.violations {
            let key = FindingKey::new(class.as_str(), violation.code(), snapshot::VIOLATION);
            let state = baseline_state(baseline, key, &mut seen);
            results.push(finding_result(
                Finding::Violation(violation),
                class,
                report.root(),
                state,
            ));
        }
        for warning in &findings.warnings {
            let key = FindingKey::new(class.as_str(), warning.code(), snapshot::WARNING);
            let state = baseline_state(baseline, key, &mut seen);
            results.push(finding_result(
                Finding::Warning(warning),
                class,
                report.root(),
                state,
            ));
        }
    }

    results
}

/// Whether this occurrence of `key` was in the baseline.
///
/// `seen` is the running count of results already emitted for each triple, so
/// the *n*th occurrence of a triple the baseline records *n* times is the first
/// to be `new`. Which of two interchangeable results is called `new` is
/// arbitrary; that the same datastore always gets the same answer is not, and
/// report order supplies it.
///
/// `None` when no baseline was given, and then nothing is counted either: a
/// run with nothing to compare against makes no claim about a baseline.
fn baseline_state(
    baseline: Option<&BaselineDiff>,
    key: FindingKey,
    seen: &mut BTreeMap<FindingKey, usize>,
) -> Option<&'static str> {
    let diff = baseline?;
    let recorded = diff.recorded(&key);
    let occurrence = seen.entry(key).or_default();
    let index = *occurrence;
    *occurrence += 1;

    Some(if index < recorded { UNCHANGED } else { NEW })
}

/// The class-level result standing for a coverage state, or `None` for a
/// class that was actually checked.
///
/// The `ruleId` is the class's own requirements URI, which is a real code in
/// the vocabulary — the content sweep's `DeclarationMismatch` already cites it
/// — so a class-level result resolves to a declared rule like any other.
///
/// Its `baselineState` compares coverage against coverage: a class the
/// baseline already recorded in this state is `unchanged`, and a class that has
/// *become* unjudged — content appeared that this crate does not check — is
/// `new`, which is the news a reader wants. The finding key does not reach here
/// because a coverage result is not a finding; it stands for a state of the
/// class, so the state is what it is compared on.
fn coverage_result(
    class: RequirementsClass,
    findings: &ClassFindings,
    baseline: Option<&BaselineDiff>,
) -> Option<Value> {
    let (kind, message) = match findings.coverage {
        // Checked and clean says itself by having nothing to say.
        ContentCoverage::Checked => return None,
        ContentCoverage::Unchecked => (
            "review",
            format!(
                "{class}: the datastore holds content this class governs and cdb-lint has no \
                 datastore-level check for it, so the content was not judged — a pass here \
                 means \"we did not look\", never \"we looked and it was fine\" \
                 (docs/CONFORMANCE.md §6)"
            ),
        ),
        ContentCoverage::NoContent => (
            "notApplicable",
            format!(
                "{class}: this run found no content this class governs, so nothing about it \
                 was checked. A declared class with no content passes; the class says what the \
                 profile supports, not what the datastore holds"
            ),
        ),
    };

    let state = baseline.map(|diff| {
        if diff.coverage_of(class.as_str()) == Some(findings.coverage.as_str()) {
            UNCHANGED
        } else {
            NEW
        }
    });

    Some(result(
        class.requirements_uri(),
        kind,
        "none",
        message,
        class.as_str(),
        None,
        state,
    ))
}

/// One finding as a `fail` result, with its `level` carrying the SHALL/SHOULD
/// split the crate never blurs.
///
/// `class` is the class the *report* filed the finding under rather than the
/// catalogue's column for the code: the two differ for `/conf/minimal-core`,
/// which has no fixed class, and the report is what the reader is holding.
///
/// The message is the crate's own, reproduced verbatim. cdb-lint neither
/// rewrites nor re-cases it — the code is the durable identity, and the prose
/// is the library's to word.
fn finding_result(
    finding: Finding<'_>,
    class: RequirementsClass,
    root: &Path,
    baseline_state: Option<&str>,
) -> Value {
    let (rule_id, level, message) = match finding {
        Finding::Violation(violation) => (violation.code(), "error", violation.to_string()),
        Finding::Warning(warning) => (warning.code(), "warning", warning.to_string()),
    };

    result(
        rule_id,
        "fail",
        level,
        message,
        class.as_str(),
        locate(finding, root),
        baseline_state,
    )
}

/// One SARIF `result`.
///
/// `location` is a datastore-relative path, or `None` for the datastore
/// itself. `baseline_state` is `--baseline`'s only mark on the document, and it
/// is set here and nowhere else: one builder, so no kind of result can acquire
/// or lose the field by being built somewhere that forgot about it.
fn result(
    rule_id: &str,
    kind: &str,
    level: &str,
    message: String,
    class: &str,
    location: Option<Vec<u8>>,
    baseline_state: Option<&str>,
) -> Value {
    let uri = match location.as_deref() {
        Some(path) => encode_uri(path),
        None => ROOT_URI.to_owned(),
    };

    let mut result = json!({
        "ruleId": rule_id,
        "kind": kind,
        // Explicit, always: an absent `level` defaults to `warning`, which
        // would silently downgrade a violation and silently upgrade a review.
        "level": level,
        "message": { "text": message },
        "locations": [{
            "physicalLocation": {
                "artifactLocation": { "uri": uri, "uriBaseId": DATASTORE_ROOT }
            }
        }],
        "properties": { "class": class },
    });

    // Omitted rather than null when there was no baseline: SARIF reads an
    // absent `baselineState` as unknown, and `null` is not a value the schema
    // admits at all.
    if let (Some(state), Some(object)) = (baseline_state, result.as_object_mut()) {
        object.insert("baselineState".to_owned(), Value::from(state));
    }

    result
}

/// Either kind of finding, so the location matcher and the result builder are
/// written once rather than twice.
#[derive(Debug, Clone, Copy)]
enum Finding<'a> {
    Violation(&'a CdbViolation),
    Warning(&'a CdbWarning),
}

/// Where a finding is, as a **datastore-relative** path — or `None` for a
/// finding whose location is the datastore itself.
///
/// Three rules govern the matcher, and the second is the substance of it:
///
/// - a variant carrying a filesystem path or a datastore logical path locates
///   there;
/// - a variant carrying a **name** locates at the root. A name is not a
///   location, and deriving a path from one would be fabrication —
///   `NamingViolation::ContainsSpace { name }` knows a name was bad, not where
///   the name was, and the walk that produced it visits every directory;
/// - the wildcard arm locates at the root. The finding enums are
///   `#[non_exhaustive]`, so a wildcard is mandatory anyway, and a variant
///   added in a later `1.x` degrades to the root instead of breaking.
///
/// Removing or reshaping a matched variant is a compile error, which is the
/// correct outcome: `1.0` froze the API surface, so a variant that vanishes is
/// news.
fn locate(finding: Finding<'_>, root: &Path) -> Option<Vec<u8>> {
    match finding {
        Finding::Violation(violation) => locate_violation(violation, root),
        Finding::Warning(warning) => locate_warning(warning, root),
    }
}

/// [`locate`] over the violations.
fn locate_violation(violation: &CdbViolation, root: &Path) -> Option<Vec<u8>> {
    match violation {
        // Physical paths the library built from the datastore root.
        // `MissingGlobalMetadata` carries the root itself, which relativizes
        // to nothing and lands back at the root — deliberately, and through a
        // real arm rather than through the wildcard.
        CdbViolation::Hierarchy(HierarchyViolation::MissingGlobalMetadata { root: missing }) => {
            relative_to(root, missing)
        }
        CdbViolation::Crs(CrsViolation::MissingCrsMetadata { searched }) => {
            relative_to(root, searched)
        }

        // A metadata finding, at every nesting the finding tree admits: an
        // optional class's stage reports its instance's record failures
        // through its own violation, and the record is in the same place
        // either way.
        CdbViolation::Metadata(metadata)
        | CdbViolation::Coverage(CoverageViolation::Metadata(metadata))
        | CdbViolation::Geometry(GeometryViolation::Metadata(metadata))
        | CdbViolation::Tiling(TilingViolation::Metadata(metadata))
        | CdbViolation::Topology(TopologyViolation::Metadata(metadata)) => {
            locate_metadata(metadata, root)
        }

        // Versioning addresses assets by datastore logical path (Requirement
        // V1, §7.14.2), so its findings know exactly which asset they mean.
        CdbViolation::Versioning(
            VersioningViolation::DuplicateAssetInCollection { asset }
            | VersioningViolation::EmptyState { asset }
            | VersioningViolation::AssetInReservedTree { asset, .. }
            | VersioningViolation::AssetAlreadyExists { asset }
            | VersioningViolation::AssetMissing { asset }
            | VersioningViolation::AssetStateMissing { asset },
        ) => relative_logical(asset),
        CdbViolation::Versioning(VersioningViolation::ResourceRecordMissing { record }) => {
            relative_logical(record)
        }

        // A name is not a location. The naming walk validates one component at
        // a time and the violation carries that component, so the datastore is
        // all cdb-lint honestly knows.
        CdbViolation::Naming(
            NamingViolation::ContainsSpace { .. }
            | NamingViolation::ForbiddenCharacter { .. }
            | NamingViolation::ControlCharacter { .. }
            | NamingViolation::CaseRuleViolation { .. }
            | NamingViolation::EmptyName,
        )
        | CdbViolation::Attribution(AttributionViolation::InvalidFileName { .. }) => None,

        // A malformed path is the finding's *subject*, not a place: the whole
        // complaint is that it does not address anything under the root, and
        // an `artifactLocation` built from one would point outside
        // `DATASTORE_ROOT` or at a path with a hole in it.
        CdbViolation::Naming(
            NamingViolation::EmptyPathComponent { .. } | NamingViolation::PathTraversal { .. },
        )
        | CdbViolation::Versioning(VersioningViolation::InvalidAssetPath { .. }) => None,

        _ => None,
    }
}

/// [`locate`] over the warnings.
fn locate_warning(warning: &CdbWarning, root: &Path) -> Option<Vec<u8>> {
    match warning {
        CdbWarning::Hierarchy(HierarchyWarning::EmptyFolder(path)) => relative_to(root, path),

        // Names again: the two naming recommendations judge a component, and
        // `RootNameNotCdb` judges the root's own name — which is the root.
        CdbWarning::Naming(
            NamingWarning::NonAscii { .. } | NamingWarning::NonSpecExtension { .. },
        )
        | CdbWarning::Hierarchy(HierarchyWarning::RootNameNotCdb { .. }) => None,

        _ => None,
    }
}

/// [`locate`] over a [`MetadataViolation`], wherever it is nested.
fn locate_metadata(violation: &MetadataViolation, root: &Path) -> Option<Vec<u8>> {
    match violation {
        MetadataViolation::MissingGlobalMetadata { searched } => relative_to(root, searched),
        // The Metadata5 sweep is handed both spellings: a bare file name for
        // the records at the top of `global_metadata/`, and a datastore
        // logical path for every resource record below it. Only the second is
        // a location, and `relative_logical` is what tells them apart.
        MetadataViolation::EncodingMismatch { file, .. } => relative_logical(file),
        _ => None,
    }
}

/// `path` as a datastore-relative path in its own bytes, or `None` when it
/// is the root itself or does not lie beneath it.
///
/// Only ordinary components survive: a `..` would resolve outside
/// `DATASTORE_ROOT`, which is a location the base was chosen to exclude.
/// The component bytes are kept as they are — a non-UTF-8 name is still the
/// name on disk, and the URI encoder speaks bytes.
fn relative_to(root: &Path, path: &Path) -> Option<Vec<u8>> {
    let relative = path.strip_prefix(root).ok()?;
    let mut segments: Vec<&[u8]> = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(name) => segments.push(name.as_encoded_bytes()),
            _ => return None,
        }
    }
    if segments.is_empty() {
        return None;
    }

    Some(segments.join(&b"/"[..]))
}

/// A datastore **logical** path — the `/`-rooted form of Requirement File5,
/// which `hierarchy::logical_path` produces — as a datastore-relative path.
///
/// `None` for anything else, and the discrimination is the point. A bare file
/// name is not a logical path, and gluing a directory onto one would invent a
/// place the finding never named. Empty, `.` and `..` components are refused
/// for the same reason [`relative_to`] refuses them.
fn relative_logical(logical: &str) -> Option<Vec<u8>> {
    let relative = logical.strip_prefix('/')?;
    if relative.is_empty()
        || relative
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return None;
    }

    Some(relative.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use rusty_cdb::metadata::MetadataEncoding;
    use rusty_cdb::naming::CaseRule;

    /// A stand-in datastore root. Nothing here touches the filesystem — the
    /// matcher is a pure function of a finding and a root.
    fn root() -> PathBuf {
        PathBuf::from("/srv/cdb")
    }

    /// Locate a violation, for the arms a real datastore cannot reach through
    /// `validate` (`tests/cli_sarif.rs` drives the ones it can).
    fn locate_of(violation: CdbViolation) -> Option<Vec<u8>> {
        locate(Finding::Violation(&violation), &root())
    }

    /// A located path as bytes, for expectations.
    fn at(path: &str) -> Option<Vec<u8>> {
        Some(path.as_bytes().to_vec())
    }

    /// A variant carrying a real path under the root locates there; one
    /// carrying the root itself, or a path outside it, locates at the root.
    #[test]
    fn cli_sarif_a_physical_path_relativizes_against_the_root() {
        assert_eq!(
            locate_of(
                CrsViolation::MissingCrsMetadata {
                    searched: root().join("global_metadata"),
                }
                .into()
            ),
            at("global_metadata")
        );
        assert_eq!(
            locate_of(HierarchyViolation::MissingGlobalMetadata { root: root() }.into()),
            None,
            "the root relativizes to nothing, which is the root"
        );
        assert_eq!(
            locate_of(
                CrsViolation::MissingCrsMetadata {
                    searched: PathBuf::from("/elsewhere/global_metadata"),
                }
                .into()
            ),
            None,
            "a path outside the base is not a location under it"
        );
    }

    /// Requirement Metadata5's sweep produces one variant carrying two kinds
    /// of string, and only one of them is a place.
    #[test]
    fn cli_sarif_a_logical_path_locates_and_a_bare_name_does_not() {
        let mismatch = |file: &str| {
            locate_of(
                MetadataViolation::EncodingMismatch {
                    file: file.to_owned(),
                    declared: MetadataEncoding::Xml,
                    found: MetadataEncoding::Json,
                }
                .into(),
            )
        };

        assert_eq!(
            mismatch("/Tiles/metadata/Roads.json"),
            at("Tiles/metadata/Roads.json")
        );
        assert_eq!(
            mismatch("global_metadata.json"),
            None,
            "a bare name is not a path, and choosing a directory for it would \
             be fabrication"
        );
        assert_eq!(mismatch("/"), None);
        assert_eq!(mismatch("/Tiles//Roads.json"), None);
        assert_eq!(mismatch("/Tiles/../etc/passwd"), None);
    }

    /// A name is not a location, whichever requirement complained about it.
    #[test]
    fn cli_sarif_a_name_locates_at_the_root() {
        for violation in [
            CdbViolation::from(NamingViolation::ContainsSpace {
                name: "My Tiles".to_owned(),
            }),
            NamingViolation::ForbiddenCharacter {
                name: "a#b".to_owned(),
                character: '#',
            }
            .into(),
            NamingViolation::CaseRuleViolation {
                name: "my_tiles".to_owned(),
                expected: CaseRule::PascalCase,
            }
            .into(),
            AttributionViolation::InvalidFileName {
                name: "Vector_Attributes.json".to_owned(),
            }
            .into(),
        ] {
            assert_eq!(locate_of(violation), None);
        }
    }

    /// A malformed path is a subject, not a place — and a versioning asset
    /// path is a place, which is the contrast that makes the rule a rule.
    #[test]
    fn cli_sarif_a_malformed_path_is_not_a_location() {
        assert_eq!(
            locate_of(
                VersioningViolation::AssetMissing {
                    asset: "/Tiles/N32/W118/Roads.gpkg".to_owned(),
                }
                .into()
            ),
            at("Tiles/N32/W118/Roads.gpkg")
        );
        assert_eq!(
            locate_of(
                VersioningViolation::InvalidAssetPath {
                    asset: "../escape".to_owned(),
                    source: NamingViolation::EmptyName,
                }
                .into()
            ),
            None
        );
        assert_eq!(
            locate_of(
                NamingViolation::PathTraversal {
                    path: "/Tiles/../../etc".to_owned(),
                    component: "..".to_owned(),
                }
                .into()
            ),
            None
        );
    }

    /// A warning carrying a path locates like a violation carrying one.
    #[test]
    fn cli_sarif_a_warning_locates_too() {
        let warning = CdbWarning::Hierarchy(HierarchyWarning::EmptyFolder(
            root().join("Tiles").join("Empty"),
        ));
        assert_eq!(
            locate(Finding::Warning(&warning), &root()),
            at("Tiles/Empty")
        );

        let warning = CdbWarning::Hierarchy(HierarchyWarning::RootNameNotCdb {
            name: "MyStore".to_owned(),
        });
        assert_eq!(locate(Finding::Warning(&warning), &root()), None);
    }

    /// `/conf/minimal-core` files under whichever mandatory class the profile
    /// omitted, so the **result** states the class the report chose while the
    /// rule states none. No built-in profile can reach the finding — both
    /// declare every class — so this is where the pairing is pinned.
    #[test]
    fn req_cdb_lint_sarif_a_classless_rule_still_yields_a_classed_result() {
        let violation = CdbViolation::MissingConformanceDeclaration {
            profile: "restricted".to_owned(),
            class: RequirementsClass::Links,
        };
        let result = finding_result(
            Finding::Violation(&violation),
            RequirementsClass::Links,
            &root(),
            None,
        );

        assert_eq!(result["ruleId"], "/conf/minimal-core");
        assert!(
            result.get("baselineState").is_none(),
            "no baseline, no state: {result}"
        );
        assert_eq!(result["properties"]["class"], "links");
        assert_eq!(result["kind"], "fail");
        assert_eq!(result["level"], "error");

        let bundle = catalogue::CATALOGUE
            .iter()
            .find(|entry| entry.code == "/conf/minimal-core")
            .map(rule);
        assert_eq!(
            bundle
                .as_ref()
                .and_then(|rule| rule["properties"].get("class")),
            None,
            "the marker is not a class token"
        );
    }

    /// A URI reference carries no raw space, and no raw anything else RFC 3986
    /// excludes from a path.
    #[test]
    fn cli_sarif_a_uri_is_percent_encoded() {
        assert_eq!(encode_uri(b"Tiles/N32/Roads.gpkg"), "Tiles/N32/Roads.gpkg");
        assert_eq!(encode_uri(b"My Tiles"), "My%20Tiles");
        assert_eq!(encode_uri(b"100%"), "100%25");
        assert_eq!(encode_uri("caf\u{e9}".as_bytes()), "caf%C3%A9");
        assert_eq!(encode_uri(b"a?b#c"), "a%3Fb%23c");
    }

    /// `:` is encoded although a path *segment* may carry one raw: a relative
    /// reference opening with `backup:2024` URI-parses as scheme `backup`,
    /// and `:` is in the library's own forbidden-character list, so exactly
    /// the non-conformant names this document describes can carry one in the
    /// first segment. Percent-encoded, the byte is unambiguous everywhere.
    #[test]
    fn cli_sarif_a_colon_is_percent_encoded() {
        assert_eq!(encode_uri(b"backup:2024"), "backup%3A2024");
        assert_eq!(base_uri(Path::new("/srv/c:db")), "file:///srv/c%3Adb/");
    }

    /// A non-UTF-8 path keeps its bytes. The grammar goes out of its way to
    /// admit such roots (`cli_args_root_keeps_its_non_utf8_bytes`), and a URI
    /// built from U+FFFD replacements would address a file that does not
    /// exist — a fabricated location in a document whose locations are the
    /// point.
    #[cfg(unix)]
    #[test]
    fn cli_sarif_non_utf8_bytes_survive_into_the_uri() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let odd = Path::new(OsStr::from_bytes(b"/srv/cdb/My\xFFTiles"));
        let warning = CdbWarning::Hierarchy(HierarchyWarning::EmptyFolder(odd.to_path_buf()));
        let located = locate(Finding::Warning(&warning), &root());

        assert_eq!(located, Some(b"My\xFFTiles".to_vec()));
        assert_eq!(encode_uri(b"My\xFFTiles"), "My%FFTiles");
        assert_eq!(
            base_uri(Path::new(OsStr::from_bytes(b"/srv/\xFF"))),
            "file:///srv/%FF/"
        );
        assert!(
            !encode_uri(b"My\xFFTiles").contains("%EF%BF%BD"),
            "no replacement-character fabrication"
        );
    }

    /// The base URI is absolute, `file:`-schemed, and ends in a separator.
    #[test]
    fn cli_sarif_the_base_uri_ends_in_a_separator() {
        assert_eq!(base_uri(Path::new("/srv/cdb")), "file:///srv/cdb/");
        assert_eq!(base_uri(Path::new("/srv/my cdb")), "file:///srv/my%20cdb/");
        assert_eq!(base_uri(Path::new("/")), "file:///");
    }

    /// A Windows drive-rooted absolute path takes the `file:` scheme with
    /// forward slashes and a literal drive colon — `C%3A%5Csrv` would
    /// URI-parse as scheme `C`, breaking every location that resolves
    /// against the base. Exercised at the byte level, since only a Windows
    /// host produces such paths from `std::path::absolute`.
    #[test]
    fn cli_sarif_a_windows_root_takes_the_file_scheme() {
        assert_eq!(
            windows_file_uri(br"C:\srv\my cdb"),
            Some("file:///C:/srv/my%20cdb".to_owned())
        );
        assert_eq!(
            windows_file_uri(b"d:/already/forward"),
            Some("file:///d:/already/forward".to_owned())
        );
        assert_eq!(
            windows_file_uri(br"\\server\share"),
            None,
            "UNC is not claimed"
        );
        assert_eq!(
            windows_file_uri(b"/srv/cdb"),
            None,
            "posix roots go the posix way"
        );
        assert_eq!(windows_file_uri(b"relative"), None);
    }
}
