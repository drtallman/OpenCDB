//! Integration: the artifact and where it goes — `--format json`, `-o`, and
//! the routing rule that keeps the two streams apart.
//!
//! **stdout carries the artifact, stderr carries the conversation.** The rule
//! earns its keep here: `cdb-lint --format json -o baseline.json` has to mint
//! a file that Task 7's `--baseline` reader accepts, so the JSON document must
//! be the report and nothing else — no banner, no tally, no note. Everything
//! cdb-lint wants to *say* goes to the other stream.
//!
//! The fixtures are real datastores, for the reason `cli_check.rs` records: the
//! honesty rules are claims about what a datastore makes the library say, and
//! a hand-built report could be made to say anything.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use cdb_lint::{Env, exit, run};
use opencdb::conformance::{ContentCoverage, RequirementsClass};
use opencdb::links::Link;
use opencdb::metadata::ResourceMetadata;
use opencdb::profiles::ApplicationProfile;
use opencdb::topology::WindingOrder;
use opencdb::{CdbDatastore, DatastoreSeed, SimulationProfile};

/// The report's documented listing order (`Ord` on `RequirementsClass`,
/// alphabetical), written out rather than derived so the test would notice
/// the enum quietly reordering.
const ORD_ORDER: [&str; 11] = [
    "attribution",
    "coverages",
    "crs",
    "file-naming",
    "file-structure",
    "geometry",
    "links",
    "metadata",
    "tiling",
    "topology",
    "versioning",
];

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

/// Drive [`run`] over `root` under the `simulation`/`json` yardstick.
fn lint_sim(root: &Path, extra: &[&str]) -> Run {
    let mut tokens = vec!["--profile", "simulation", "--encoding", "json"];
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
/// class is declared, listed, and empty.
fn bare(profile: &dyn ApplicationProfile) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    CdbDatastore::create(tmp.path(), profile, seed()).unwrap();
    let root = tmp.path().join(profile.root_folder_name());

    (tmp, root)
}

/// A datastore whose Topology coverage is genuinely `unchecked`.
///
/// One resource record carrying the Face4 `windingOrder` is the whole trick:
/// §7.9.4.2's conditional element is Topology's content signal, and the class
/// can never be `checked`, because a topological graph lives in the payload
/// this crate deliberately does not decode (`docs/CONFORMANCE.md` §6). Smaller
/// than `cli_check.rs`'s `rich` on purpose — the format tests need the third
/// coverage state, not all eleven classes populated.
fn with_topology(profile: &dyn ApplicationProfile) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let store = CdbDatastore::create(tmp.path(), profile, seed()).unwrap();

    let payload = "/Tiles/RoadFaces.gpkg";
    let mut record = ResourceMetadata::new("RoadFaces", "Road Faces", "Structured faces");
    record.winding_order = Some(WindingOrder::Counterclockwise);
    record.associations = vec![Link::new(payload, "describes").unwrap()];
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

    (tmp, root)
}

/// A datastore that fails: judged against the wrong declared encoding, so
/// Requirement Metadata5 convicts it.
fn violating() -> (tempfile::TempDir, PathBuf) {
    bare(&SimulationProfile::json())
}

// ---------------------------------------------------------------------------
// The wire shape
// ---------------------------------------------------------------------------

/// The keys of a JSON object, as a sorted vector, so a set comparison reads
/// as one assertion.
fn keys(value: &serde_json::Value) -> Vec<String> {
    let mut names: Vec<String> = value
        .as_object()
        .expect("a JSON object")
        .keys()
        .cloned()
        .collect();
    names.sort();
    names
}

/// The artifact's key sets are **exactly** these, at all three levels.
///
/// Not "contains" but "equals": an added field is as much a shape change as a
/// removed one, and the document this pins is what `docs/CONFORMANCE.md`
/// describes to outside implementers, what Task 7 reads back as a baseline,
/// and what `1.0` froze. A test that only checked for presence would let
/// cdb-lint quietly decorate the report.
#[test]
fn cli_output_json_has_the_exact_wire_shape() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = with_topology(&profile);

    let run = lint_sim(&root, &["--format", "json"]);
    assert_eq!(run.code, exit::OK, "stderr:\n{}", run.err);

    let document: serde_json::Value =
        serde_json::from_str(&run.out).expect("the artifact parses as JSON");

    assert_eq!(
        keys(&document),
        ["classes", "conformant", "profile", "root"]
    );

    let classes = document["classes"]
        .as_array()
        .expect("classes is an array, never a map: Ord order is load-bearing");
    assert_eq!(classes.len(), ORD_ORDER.len());

    for entry in classes {
        assert_eq!(
            keys(entry),
            [
                "class",
                "content",
                "has_content",
                "passed",
                "requirements_uri",
                "violations",
                "warnings",
            ]
        );
        let content = entry["content"].as_str().expect("content is a string");
        assert!(
            matches!(content, "checked" | "none" | "unchecked"),
            "unexpected coverage token {content:?} — the wire tokens are \
             checked/none/unchecked, and `none` is not spelled `no-content`"
        );
    }
}

/// The classes are an array in the report's documented `Ord` order. A map
/// would leave the order to the serializer, and the order is documented.
#[test]
fn cli_output_json_lists_classes_in_ord_order() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = bare(&profile);

    let run = lint_sim(&root, &["--format", "json"]);
    let document: serde_json::Value = serde_json::from_str(&run.out).unwrap();

    let listed: Vec<&str> = document["classes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["class"].as_str().unwrap())
        .collect();

    assert_eq!(listed, ORD_ORDER);
}

/// Findings carry the four flat fields and no more, so a consumer reads a
/// violation and a warning with one shape and tells them apart by `severity`.
#[test]
fn cli_output_json_findings_are_flat_and_coded() {
    let (_tmp, root) = violating();

    // The wrong declared encoding convicts the datastore under Metadata5.
    let run = lint(&[
        "--profile",
        "simulation",
        "--encoding",
        "xml",
        "--format",
        "json",
        &root.to_string_lossy(),
    ]);
    assert_eq!(run.code, exit::FINDINGS);

    let document: serde_json::Value = serde_json::from_str(&run.out).unwrap();
    let mut seen = 0usize;
    for entry in document["classes"].as_array().unwrap() {
        for finding in entry["violations"].as_array().unwrap() {
            assert_eq!(keys(finding), ["class", "code", "message", "severity"]);
            assert_eq!(finding["severity"], "violation");
            assert!(
                finding["code"].as_str().unwrap().starts_with('/'),
                "a code is an absolute clause URI: {finding}"
            );
            seen += 1;
        }
    }
    assert!(seen > 0, "the fixture must actually produce a violation");
}

/// The artifact is the serialized report and nothing else — byte for byte,
/// plus the one trailing newline a text file owes its reader.
#[test]
fn cli_output_json_artifact_is_exactly_the_serialized_report() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = with_topology(&profile);

    let run = lint_sim(&root, &["--format", "json"]);

    let store = CdbDatastore::open(&root).unwrap();
    let report = store.validate(&profile).unwrap();
    let expected = format!("{}\n", serde_json::to_string_pretty(&report).unwrap());

    assert_eq!(run.out, expected);
}

// ---------------------------------------------------------------------------
// Honesty rule 2 across the format boundary
// ---------------------------------------------------------------------------

/// Honesty rule 2: no output omits coverage. The JSON artifact cannot carry
/// the aggregate without ceasing to be the frozen wire shape, so the tally
/// goes to **stderr** — where it reaches the reader without entering the
/// document. The per-class `content` field carries the other half.
#[test]
fn req_cdb_lint_json_states_the_tally_on_stderr() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = with_topology(&profile);

    let run = lint_sim(&root, &["--format", "json"]);

    assert!(
        run.err.contains("classes:") && run.err.contains("not checked"),
        "the tally belongs on stderr:\n{}",
        run.err
    );
    assert!(
        !run.out.contains("not checked"),
        "the artifact must stay the frozen wire shape:\n{}",
        run.out
    );
}

/// Honesty rule 1 survives the format change: the third coverage state is on
/// the wire, so a machine consumer can tell "we did not look" from "we looked
/// and it was fine" without reading `docs/CONFORMANCE.md`.
#[test]
fn req_cdb_lint_unchecked_survives_the_json_format() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = with_topology(&profile);

    // The fixture has to genuinely produce the third state, or what follows
    // would pass over a report that never exercised it.
    let store = CdbDatastore::open(&root).unwrap();
    let report = store.validate(&profile).unwrap();
    assert_eq!(
        report.class_coverage(RequirementsClass::Topology),
        ContentCoverage::Unchecked,
        "the fixture must put topology into the third state"
    );

    let run = lint_sim(&root, &["--format", "json"]);
    let document: serde_json::Value = serde_json::from_str(&run.out).unwrap();
    let topology = document["classes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["class"] == "topology")
        .expect("topology is listed");

    assert_eq!(topology["content"], "unchecked");
    assert_eq!(topology["has_content"], true);
    assert_eq!(
        topology["passed"], true,
        "passed and content are independent: this is the pair a reader must \
         see together, and the reason `content` is on the wire at all"
    );
}

/// The text artifact already carries the tally, so stderr does not repeat it.
#[test]
fn cli_output_text_does_not_duplicate_the_tally_on_stderr() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = with_topology(&profile);

    let run = lint_sim(&root, &[]);

    assert!(run.out.contains("not checked"), "{}", run.out);
    assert!(
        !run.err.contains("not checked"),
        "the text report already said it:\n{}",
        run.err
    );
}

// ---------------------------------------------------------------------------
// `-o` and the routing rule
// ---------------------------------------------------------------------------

/// `-o` redirects the artifact and leaves stdout empty — for every format.
#[test]
fn cli_output_o_writes_the_artifact_and_leaves_stdout_empty() {
    let profile = SimulationProfile::json();
    let (tmp, root) = with_topology(&profile);

    for format in ["text", "json"] {
        let path = tmp.path().join(format!("report.{format}"));
        let run = lint_sim(&root, &["--format", format, "-o", &path.to_string_lossy()]);

        assert_eq!(run.code, exit::OK, "{format}: {}", run.err);
        assert!(
            run.out.is_empty(),
            "{format}: the artifact went to the file, so stdout is empty:\n{}",
            run.out
        );
        let written = std::fs::read_to_string(&path).expect("the file exists");
        assert!(!written.is_empty(), "{format}: the file is empty");
        if format == "json" {
            serde_json::from_str::<serde_json::Value>(&written)
                .expect("the file holds the JSON artifact");
        } else {
            assert!(written.contains("CONFORMANT"), "{written}");
        }
    }
}

/// Diagnostics are not part of the artifact, so `-o` does not silence them.
#[test]
fn cli_output_diagnostics_still_reach_stderr_with_o() {
    let (tmp, root) = violating();
    let path = tmp.path().join("report.json");

    let run = lint(&[
        "--profile",
        "simulation",
        "--encoding",
        "xml",
        "--format",
        "json",
        "-o",
        &path.to_string_lossy(),
        &root.to_string_lossy(),
    ]);

    assert_eq!(run.code, exit::FINDINGS);
    assert!(run.out.is_empty(), "{}", run.out);
    assert!(
        run.err.contains("/req/core/metadata-encoding"),
        "the encoding hint still speaks:\n{}",
        run.err
    );
    assert!(path.is_file(), "the artifact was still written");
}

/// An output path that cannot be written is an I/O failure that stopped the
/// run, which is exit 3. Exit 2 stays for what can be caught before any work.
#[test]
fn cli_output_unwritable_path_is_operational() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = bare(&profile);

    let run = lint_sim(&root, &["-o", "/no-such-directory-for-cdb-lint/report.txt"]);

    assert_eq!(run.code, exit::OPERATIONAL);
    assert!(
        run.err
            .contains("/no-such-directory-for-cdb-lint/report.txt"),
        "the diagnostic names the path:\n{}",
        run.err
    );
}

/// The exit code is the verdict's, not the format's: a datastore that fails
/// fails identically in every rendering.
#[test]
fn cli_output_exit_code_is_independent_of_format() {
    let (_tmp, root) = violating();

    for format in ["text", "json"] {
        let run = lint(&[
            "--profile",
            "simulation",
            "--encoding",
            "xml",
            "--format",
            format,
            &root.to_string_lossy(),
        ]);

        assert_eq!(run.code, exit::FINDINGS, "{format}: {}", run.err);
    }
}
