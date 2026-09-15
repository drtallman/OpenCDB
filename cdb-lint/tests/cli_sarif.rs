//! Integration: `--format sarif`, the SARIF 2.1.0 rendering of a report.
//!
//! SARIF is the format a CI system reads, and it is the one place where
//! honesty rule 1 has to survive a translation into somebody else's
//! vocabulary. It does, and the mapping is what this file pins: a class whose
//! content went **unjudged** becomes a `review` result — SARIF's own term for
//! a finding "requiring further analysis by a human" — and never a silent
//! absence, which is what a consumer would otherwise read as a clean class.
//!
//! Every fixture is a real datastore, for the reason `cli_check.rs` records:
//! these are claims about what a datastore makes the library say, and a
//! hand-built report could be made to say anything.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use cdb_lint::{Env, exit, run};
use opencdb::conformance::{ContentCoverage, RequirementsClass};
use opencdb::links::Link;
use opencdb::metadata::ResourceMetadata;
use opencdb::profiles::ApplicationProfile;
use opencdb::topology::WindingOrder;
use opencdb::{CdbDatastore, DatastoreSeed, SimulationProfile};

/// The 2.1.0 schema the document names, fetched and checked against the
/// document this renderer emits.
const SCHEMA: &str =
    "https://docs.oasis-open.org/sarif/sarif/v2.1.0/errata01/os/schemas/sarif-schema-2.1.0.json";

/// A terminal-free, colour-free environment: the deterministic one.
fn plain_env() -> Env {
    Env {
        no_color: false,
        stdout_is_terminal: false,
    }
}

/// One run of the whole tool: the exit code, stdout, and stderr.
struct Run {
    code: i32,
    out: String,
    err: String,
}

/// Drive [`run`] with `tokens`.
fn lint(tokens: &[&str]) -> Run {
    let args: Vec<OsString> = tokens.iter().map(OsString::from).collect();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run(&args, &mut out, &mut err, &plain_env());

    Run {
        code,
        out: String::from_utf8_lossy(&out).into_owned(),
        err: String::from_utf8_lossy(&err).into_owned(),
    }
}

/// Drive [`run`] over `root` under a `simulation`/`<encoding>` yardstick.
fn lint_as(root: &Path, encoding: &str, extra: &[&str]) -> Run {
    let mut tokens = vec!["--profile", "simulation", "--encoding", encoding];
    tokens.extend_from_slice(extra);
    let root = root.to_string_lossy().into_owned();
    tokens.push(&root);

    lint(&tokens)
}

/// A seed with the four mandatory §7.9.4.1 identity elements.
fn seed() -> DatastoreSeed {
    DatastoreSeed::new(
        "doi:10.5066/cdb.lint",
        "Lint Fixture",
        "A datastore built for cdb-lint's own tests",
        "ops@example.com",
    )
}

/// A bare datastore: the five mandatory classes carry content, every optional
/// class is declared, listed, and empty. Conformant under its own encoding.
fn bare(profile: &dyn ApplicationProfile) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    CdbDatastore::create(tmp.path(), profile, seed()).unwrap();
    let root = tmp.path().join(profile.root_folder_name());

    (tmp, root)
}

/// The fixture behind most of this file: one datastore that reaches **all
/// four** rows of the kind/level table in a single run.
///
/// - a resource record carrying the Face4 `windingOrder` puts Topology into
///   the third coverage state (`unchecked`) — §7.9.4.2's conditional element
///   is Topology's content signal, and the graph itself lives in a payload
///   this crate deliberately does not decode;
/// - five optional classes hold nothing at all, so their coverage is `none`;
/// - an empty folder whose name carries a space is two findings at once: a
///   `NamingViolation::ContainsSpace`, which knows a **name** and no path, and
///   a `HierarchyWarning::EmptyFolder`, which knows the **path**. They are the
///   two halves of the location discipline, produced by one `mkdir`.
///
/// Judged with `--encoding xml` it also fails Requirement Metadata5 three
/// times, two of which are `EncodingMismatch`: one naming a bare file name,
/// one naming a datastore logical path. Same clause, same variant, and only
/// one of them is a location.
fn rich(profile: &dyn ApplicationProfile) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let store = CdbDatastore::create(tmp.path(), profile, seed()).unwrap();

    let mut record = ResourceMetadata::new("RoadFaces", "Road Faces", "Structured faces");
    record.winding_order = Some(WindingOrder::Counterclockwise);
    record.associations = vec![Link::new("/Tiles/RoadFaces.gpkg", "describes").unwrap()];
    store
        .write_resource_metadata(
            &format!(
                "/Tiles/metadata/RoadFaces.{}",
                profile.metadata_encoding().extension()
            ),
            &record,
        )
        .unwrap();

    let root = tmp.path().join(profile.root_folder_name());
    std::fs::create_dir(root.join("My Tiles")).unwrap();

    (tmp, root)
}

/// The document, parsed.
fn parse(text: &str) -> serde_json::Value {
    serde_json::from_str(text).expect("the artifact parses as JSON")
}

/// The single run's results.
fn results_of(document: &serde_json::Value) -> Vec<serde_json::Value> {
    document["runs"][0]["results"]
        .as_array()
        .expect("a run carries a results array")
        .clone()
}

/// Every result citing `rule_id`.
fn results_for<'a>(results: &'a [serde_json::Value], rule_id: &str) -> Vec<&'a serde_json::Value> {
    results
        .iter()
        .filter(|result| result["ruleId"] == rule_id)
        .collect()
}

/// The one result citing `rule_id`.
fn result_for<'a>(results: &'a [serde_json::Value], rule_id: &str) -> &'a serde_json::Value {
    let matched = results_for(results, rule_id);
    assert_eq!(
        matched.len(),
        1,
        "expected exactly one result for {rule_id}, found {}",
        matched.len()
    );

    matched[0]
}

/// A result's one artifact URI.
fn uri_of(result: &serde_json::Value) -> &str {
    result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"]
        .as_str()
        .expect("every result carries an artifact location")
}

// ---------------------------------------------------------------------------
// The document
// ---------------------------------------------------------------------------

/// The shell: a 2.1.0 document with one run, naming the published schema.
#[test]
fn cli_sarif_document_is_a_sarif_2_1_0_run() {
    let (_tmp, root) = bare(&SimulationProfile::json());

    let run = lint_as(&root, "json", &["--format", "sarif"]);
    assert_eq!(run.code, exit::OK, "stderr:\n{}", run.err);

    let document = parse(&run.out);
    assert_eq!(document["version"], "2.1.0");
    assert_eq!(document["$schema"], SCHEMA);

    let runs = document["runs"].as_array().expect("runs is an array");
    assert_eq!(runs.len(), 1, "one datastore, one run");

    let driver = &runs[0]["tool"]["driver"];
    assert_eq!(driver["name"], "cdb-lint");
    assert_eq!(driver["version"], env!("CARGO_PKG_VERSION"));
    assert!(
        driver.get("informationUri").is_none(),
        "there is no public repository URL to give, and inventing one is worse \
         than omitting an optional field: {driver}"
    );
}

// ---------------------------------------------------------------------------
// Honesty rule 1, in SARIF's vocabulary
// ---------------------------------------------------------------------------

/// **The honesty-critical mapping.** All four rows of the table, in one
/// document, with both fields written explicitly on every result.
///
/// `level` is never omitted, and that is the whole point. SARIF defaults an
/// absent `level` to `warning`, so an omitted level would render a violation
/// as a warning and would leave a `review` result — the one that says "this
/// content went unjudged" — indistinguishable from one. Writing both fields
/// costs two keys and removes the ambiguity in both directions.
#[test]
fn req_cdb_lint_sarif_kind_and_level_are_explicit_on_every_result() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = rich(&profile);

    let run = lint_as(&root, "xml", &["--format", "sarif"]);
    assert_eq!(run.code, exit::FINDINGS, "stderr:\n{}", run.err);
    let results = results_of(&parse(&run.out));

    // A violation: fail / error.
    let violation = result_for(&results, "/req/core/name-spaces");
    assert_eq!(violation["kind"], "fail");
    assert_eq!(violation["level"], "error");

    // A warning: fail / warning. A SHOULD still fails the SARIF result — it
    // is a finding — but its level says which kind of finding it is.
    let warning = result_for(&results, "/req/core/name-empty-folders-A");
    assert_eq!(warning["kind"], "fail");
    assert_eq!(warning["level"], "warning");

    // Coverage `unchecked`: review / none.
    let review = result_for(&results, RequirementsClass::Topology.requirements_uri());
    assert_eq!(review["kind"], "review");
    assert_eq!(review["level"], "none");

    // Coverage `none`: notApplicable / none.
    let absent = result_for(&results, RequirementsClass::Versioning.requirements_uri());
    assert_eq!(absent["kind"], "notApplicable");
    assert_eq!(absent["level"], "none");

    for result in &results {
        assert!(
            result["kind"].is_string() && result["level"].is_string(),
            "both fields are explicit on every result, because both have \
             defaults a reader would otherwise be handed silently: {result}"
        );
        assert_ne!(
            result["kind"], "pass",
            "clean checked classes emit no result: the failures, the unjudged \
             and the inapplicable are what a consumer acts on"
        );
    }
}

/// A class whose content went unjudged produces a `review` result — and no
/// `fail` stands in for it, in either direction. `docs/CONFORMANCE.md` §6:
/// for Geometry and Topology a pass means "we did not look", never "we looked
/// and it was fine".
#[test]
fn req_cdb_lint_sarif_unchecked_is_a_review_never_a_fail() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = rich(&profile);

    // The fixture has to genuinely reach the third state, or what follows
    // would pass over a report that never exercised it.
    let store = CdbDatastore::open(&root).unwrap();
    let report = store.validate(&profile).unwrap();
    assert_eq!(
        report.class_coverage(RequirementsClass::Topology),
        ContentCoverage::Unchecked,
        "the fixture must put topology into the third state"
    );

    let run = lint_as(&root, "json", &["--format", "sarif"]);
    let results = results_of(&parse(&run.out));

    let topology: Vec<&serde_json::Value> = results
        .iter()
        .filter(|result| result["properties"]["class"] == "topology")
        .collect();
    assert_eq!(
        topology.len(),
        1,
        "topology says exactly one thing here: {topology:?}"
    );
    assert_eq!(topology[0]["kind"], "review");
    assert_eq!(topology[0]["level"], "none");
    assert_eq!(
        topology[0]["ruleId"],
        RequirementsClass::Topology.requirements_uri()
    );
    assert!(
        topology[0]["message"]["text"]
            .as_str()
            .is_some_and(|text| text.contains("no datastore-level check")),
        "the message says plainly what the state means: {}",
        topology[0]["message"]
    );
}

// ---------------------------------------------------------------------------
// The rule table
// ---------------------------------------------------------------------------

/// Every `ruleId` a result cites resolves to a declared rule. Class-level
/// results cite the class's own requirements URI, which is a real code in the
/// vocabulary — a `DeclarationMismatch` already cites it.
#[test]
fn cli_sarif_every_rule_id_resolves_to_a_rule() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = rich(&profile);

    let run = lint_as(&root, "xml", &["--format", "sarif"]);
    let document = parse(&run.out);

    let declared: BTreeSet<String> = document["runs"][0]["tool"]["driver"]["rules"]
        .as_array()
        .expect("the driver declares rules")
        .iter()
        .map(|rule| {
            rule["id"]
                .as_str()
                .expect("a reportingDescriptor requires an id")
                .to_owned()
        })
        .collect();
    assert!(!declared.is_empty());

    let results = results_of(&document);
    assert!(!results.is_empty(), "the fixture must produce results");
    for result in &results {
        let rule_id = result["ruleId"].as_str().expect("a result cites a rule");
        assert!(
            declared.contains(rule_id),
            "{rule_id} resolves to no rule in tool.driver.rules"
        );
    }
}

/// Rules carry the standard's verified identity in `help.text` and **no**
/// `helpUri`.
///
/// The draft's only absolute requirement URI (Requirement Link1's box, spec
/// line 1476) drops the `req` segment its own class table uses, so no
/// dereferenceable URL can be built from a finding code. Synthesizing one
/// would hand every reader a 404, which is worse than omitting an optional
/// field.
#[test]
fn cli_sarif_rules_cite_the_spec_and_synthesize_no_help_uri() {
    let (_tmp, root) = bare(&SimulationProfile::json());

    let run = lint_as(&root, "json", &["--format", "sarif"]);
    let document = parse(&run.out);
    let rules = document["runs"][0]["tool"]["driver"]["rules"]
        .as_array()
        .expect("the driver declares rules")
        .clone();

    for rule in &rules {
        assert!(
            rule.get("helpUri").is_none(),
            "no code yields a dereferenceable URL: {rule}"
        );
        let help = rule["help"]["text"].as_str().expect("help.text");
        assert!(
            help.starts_with("OGC 23-034 (http://www.opengis.net/doc/IS/CDB-core/2.0) §"),
            "{help}"
        );
        assert!(
            rule["shortDescription"]["text"].is_string(),
            "a rule glosses its clause: {rule}"
        );
    }
    assert!(
        !run.out.contains("helpUri"),
        "not anywhere in the document either"
    );
}

/// `/conf/minimal-core` has no single class, so its rule states none.
///
/// Annex A's bundle is filed under whichever mandatory class the profile
/// failed to declare, which is a property of the *finding* and not of the
/// code. The catalogue records that with a marker, and a marker must never
/// reach a consumer dressed as a class token.
#[test]
fn req_cdb_lint_sarif_a_rule_without_one_class_states_none() {
    let (_tmp, root) = bare(&SimulationProfile::json());

    let run = lint_as(&root, "json", &["--format", "sarif"]);
    let document = parse(&run.out);
    let rules = document["runs"][0]["tool"]["driver"]["rules"]
        .as_array()
        .expect("the driver declares rules")
        .clone();

    for rule in &rules {
        assert_ne!(
            rule["properties"]["class"], "*",
            "the catalogue's cross-class marker is not a class token: {rule}"
        );
    }

    let bundle = rules
        .iter()
        .find(|rule| rule["id"] == "/conf/minimal-core")
        .expect("Annex A's bundle is in the vocabulary");
    assert!(
        bundle["properties"].get("class").is_none(),
        "the report files it under the class the profile omitted, so the rule \
         names none: {bundle}"
    );
    assert_eq!(bundle["properties"]["section"], "A.2");
}

// ---------------------------------------------------------------------------
// Locations
// ---------------------------------------------------------------------------

/// Locations hang off one `originalUriBaseIds` entry, so a consumer can
/// re-root the whole document by rewriting a single URI.
#[test]
fn cli_sarif_locations_hang_off_the_datastore_root_base() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = rich(&profile);

    let run = lint_as(&root, "xml", &["--format", "sarif"]);
    let document = parse(&run.out);

    let base = document["runs"][0]["originalUriBaseIds"]["DATASTORE_ROOT"]["uri"]
        .as_str()
        .expect("the base is an artifactLocation carrying a uri");
    assert!(base.starts_with("file:///"), "{base}");
    assert!(
        base.ends_with('/'),
        "a base URI ends in a separator or relative resolution eats its last \
         segment: {base}"
    );

    for result in results_of(&document) {
        assert_eq!(
            result["locations"][0]["physicalLocation"]["artifactLocation"]["uriBaseId"],
            "DATASTORE_ROOT",
            "every result locates against the one base: {result}"
        );
    }
}

/// **The location discipline.** A finding whose variant carries a path
/// locates there, relative to the datastore root; a finding whose variant
/// carries only a *name* locates at the root, because a name is not a
/// location and deriving a path from one would be fabrication.
///
/// The fixture produces both from one directory: `My Tiles` is a
/// `ContainsSpace` violation that knows the name and nothing else, and an
/// `EmptyFolder` warning that knows exactly where it is. It also produces the
/// harder pair — one `EncodingMismatch` naming a bare file name and another
/// naming a datastore logical path, same clause and same variant.
#[test]
fn req_cdb_lint_sarif_a_path_locates_and_a_name_locates_at_the_root() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = rich(&profile);

    let run = lint_as(&root, "xml", &["--format", "sarif"]);
    let results = results_of(&parse(&run.out));

    // A path: the empty folder's own place, relative to the root, and
    // percent-encoded, because a URI reference may not carry a raw space.
    let empty_folder = result_for(&results, "/req/core/name-empty-folders-A");
    assert_eq!(uri_of(empty_folder), "My%20Tiles");

    // A name: the root. `ContainsSpace` carries `name: "My Tiles"` and no
    // path, and this datastore happens to hold that name in two places at
    // once — the folder, and nothing else about it that cdb-lint may assume.
    let spaces = result_for(&results, "/req/core/name-spaces");
    assert_eq!(uri_of(spaces), ".");

    // Same clause, same variant, two different kinds of string.
    let encodings = results_for(&results, "/req/core/metadata-encoding");
    let uris: BTreeSet<&str> = encodings.iter().map(|result| uri_of(result)).collect();
    assert!(
        uris.contains("Tiles/metadata/RoadFaces.json"),
        "a logical path is a location: {uris:?}"
    );
    assert!(
        uris.contains("."),
        "a bare file name is not, and gluing a directory onto one would \
         invent a place the finding never named: {uris:?}"
    );

    // Class-level results carry the root, never a guess.
    let review = result_for(&results, RequirementsClass::Topology.requirements_uri());
    assert_eq!(uri_of(review), ".");
}

// ---------------------------------------------------------------------------
// Honesty rule 2: the coverage tally
// ---------------------------------------------------------------------------

/// SARIF carries the aggregate itself, so it goes in the document and not on
/// stderr.
///
/// This is where SARIF differs from `--format json`: the JSON artifact is the
/// library's frozen wire shape and cannot gain a field, so its tally is owed
/// to stderr. A SARIF run has `run.properties`, which exists for exactly this,
/// and a machine reading the document gets the number a human would.
#[test]
fn req_cdb_lint_sarif_carries_the_tally_in_run_properties() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = rich(&profile);

    let run = lint_as(&root, "json", &["--format", "sarif"]);
    let document = parse(&run.out);
    let properties = &document["runs"][0]["properties"];

    assert_eq!(properties["profile"], "simulation");
    assert_eq!(properties["conformant"], false);

    let store = CdbDatastore::open(&root).unwrap();
    let report = store.validate(&profile).unwrap();
    let count = |wanted: ContentCoverage| {
        report
            .classes()
            .filter(|(_, findings)| findings.coverage == wanted)
            .count()
    };
    assert_eq!(
        properties["coverage"]["checked"],
        count(ContentCoverage::Checked)
    );
    assert_eq!(
        properties["coverage"]["none"],
        count(ContentCoverage::NoContent)
    );
    assert_eq!(
        properties["coverage"]["unchecked"],
        count(ContentCoverage::Unchecked)
    );
    assert_eq!(
        properties["coverage"]["checked"].as_u64().unwrap()
            + properties["coverage"]["none"].as_u64().unwrap()
            + properties["coverage"]["unchecked"].as_u64().unwrap(),
        report.classes().count() as u64,
        "the three counts partition the classes"
    );

    assert!(
        !run.err.contains("not checked") && !run.err.contains("classes:"),
        "the document said it, so stderr does not repeat it:\n{}",
        run.err
    );
}

// ---------------------------------------------------------------------------
// The verdict is not the format's business
// ---------------------------------------------------------------------------

/// The exit code is the verdict's, not the format's: a datastore that fails
/// fails identically in every rendering, and one that passes passes.
#[test]
fn cli_sarif_exit_code_is_independent_of_format() {
    let profile = SimulationProfile::json();
    let (_tmp, clean) = bare(&profile);
    let (_tmp2, failing) = rich(&profile);

    for format in ["text", "json", "sarif"] {
        let run = lint_as(&clean, "json", &["--format", format]);
        assert_eq!(run.code, exit::OK, "{format}: {}", run.err);

        let run = lint_as(&failing, "json", &["--format", format]);
        assert_eq!(run.code, exit::FINDINGS, "{format}: {}", run.err);
    }
}
