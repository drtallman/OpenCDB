//! Integration: the six honesty rules of design §5, each asserted across all
//! three output formats at once.
//!
//! # This file is deliberately redundant, and the redundancy is the point
//!
//! Every rule here is already tested somewhere else. `cli_check.rs` holds the
//! text report's coverage rows, `cli_output.rs` the JSON artifact's `content`
//! field and its stderr tally, `cli_sarif.rs` the `kind`/`level` mapping,
//! `cli_baseline.rs` the ratchet's verdict line, and `cli_profile.rs` the
//! pre-flight that refuses a broken yardstick. None of that is duplicated here
//! by accident.
//!
//! Those suites are organised by **format**, and a rule asserted one format at
//! a time can be kept by one renderer while another quietly stops keeping it:
//! no test in a per-format file ever holds two renderings of the same report
//! side by side, so nothing there can notice `--format sarif` losing a
//! distinction the text report still draws. The rules are not properties of a
//! renderer. They are one contract the tool keeps in three vocabularies, and
//! this is the only file that says so — every test below drives one datastore
//! through `text`, `json` and `sarif` and asserts the same rule of each.
//!
//! The second reason is survival. A per-format test travels with its format:
//! rewrite the SARIF renderer and `cli_sarif.rs` is rewritten with it, taking
//! honesty rule 3 along as a casualty nobody reviews. These tests are named
//! after the rules instead, so the contract outlives whichever implementation
//! currently discharges it.
//!
//! # What the rules are
//!
//! 1. `unchecked` never renders as a bare green PASS.
//! 2. No output omits coverage.
//! 3. SARIF carries coverage in `result.kind`.
//! 4. `--baseline` and `--deny-warnings` move the exit code, never the report.
//! 5. Detection informs diagnostics, never the yardstick.
//! 6. A broken yardstick is a usage error, never a datastore finding.
//!
//! They descend from documented duties in `docs/CONFORMANCE.md` §6 rather than
//! from cdb-lint's own preferences, which is why these tests keep the `req_`
//! prefix that design §10 otherwise reserves for spec requirements.
//!
//! # Every fixture is a real datastore, and its coverage is checked first
//!
//! `CdbDatastore::create` builds each one on a `tempfile` directory and the
//! library judges it. A hand-built report could be made to say anything, and
//! these are claims about what a *datastore* makes the library say.
//!
//! [`three_valued`] is the fixture behind most of the file: a datastore whose
//! eleven classes land in **all three** coverage states at once. Every test
//! calls [`assert_three_valued`] before it asserts anything about rendering,
//! because a fixture that quietly stopped reaching the third state would let
//! the whole file pass over reports that never exercised the rule it exists to
//! pin.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use cdb_lint::{Env, exit, run};
use rusty_cdb::conformance::{ConformanceReport, ContentCoverage, RequirementsClass};
use rusty_cdb::links::Link;
use rusty_cdb::metadata::{ResourceMetadata, UnitOfMeasure};
use rusty_cdb::profiles::ApplicationProfile;
use rusty_cdb::profiles::simulation::WGS84_2D_WKT;
use rusty_cdb::topology::WindingOrder;
use rusty_cdb::{CdbDatastore, DatastoreSeed, SimulationProfile};
use serde_json::{Value, json};

/// The three formats. Every rule below is asserted in each of them, in one
/// test, over one datastore.
const FORMATS: [&str; 3] = ["text", "json", "sarif"];

/// The ANSI sequences design §6.1 pins to the status tokens. Written out here
/// rather than imported, so the test would notice the renderer silently
/// agreeing with itself about a changed escape.
const GREEN: &str = "\u{1b}[32m";
const YELLOW: &str = "\u{1b}[33m";
const RESET: &str = "\u{1b}[0m";

// ---------------------------------------------------------------------------
// Driving the tool
// ---------------------------------------------------------------------------

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

impl Run {
    /// The report's last line, which is the verdict.
    fn verdict(&self) -> &str {
        self.out
            .trim_end_matches('\n')
            .rsplit('\n')
            .next()
            .unwrap_or_default()
    }

    /// stdout as JSON — for the two machine formats.
    fn document(&self) -> Value {
        serde_json::from_str(&self.out).unwrap_or_else(|error| {
            panic!("the artifact should parse as JSON ({error}):\n{}", self.out)
        })
    }
}

/// Drive [`run`] with `tokens`, in the deterministic environment.
///
/// Every run in this file uses that one environment, and `--color always`
/// rather than a pretend terminal is what asks for colour: an honesty rule
/// must hold whatever the surroundings, so the surroundings are held still and
/// the flags do the varying.
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

/// Drive [`run`] over `root` under that yardstick, rendered as `format`.
fn lint_as(format: &str, root: &Path, extra: &[&str]) -> Run {
    let mut tokens = vec!["--format", format];
    tokens.extend_from_slice(extra);

    lint_sim(root, &tokens)
}

// ---------------------------------------------------------------------------
// The fixtures
// ---------------------------------------------------------------------------

/// A seed with the four mandatory §7.9.4.1 identity elements.
fn seed() -> DatastoreSeed {
    DatastoreSeed::new(
        "doi:10.5066/cdb.lint",
        "Lint Fixture",
        "A datastore built for cdb-lint's own tests",
        "ops@example.com",
    )
}

/// A datastore whose coverage is genuinely three-valued.
///
/// - the five mandatory classes have content and are **checked**;
/// - two resource records carry §7.9.4.2 conditional elements — the Geom4
///   `uom` and the Face4 `windingOrder` — which are Geometry's and Topology's
///   content signals, and both classes come out **unchecked**, because their
///   subjects live inside payloads this crate deliberately does not decode
///   (`docs/CONFORMANCE.md` §6);
/// - the remaining four optional classes govern nothing here, so their
///   coverage is **none**.
///
/// Three states in one report is what makes this file's claims non-vacuous: a
/// renderer cannot conflate `unchecked` with `checked` or with `none` if a
/// single run has to render all three and be read afterwards.
fn three_valued(profile: &dyn ApplicationProfile) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let store = CdbDatastore::create(tmp.path(), profile, seed()).unwrap();

    // Geom4's `uom`: Geometry's content signal.
    let mut roads = ResourceMetadata::new("RoadNetwork", "Road Network", "Vector tile");
    roads.uom = Some(UnitOfMeasure::Meters);
    roads.associations = vec![Link::new("/Tiles/RoadNetwork.gpkg", "describes").unwrap()];

    // Face4's `windingOrder`: Topology's.
    let mut faces = ResourceMetadata::new("RoadFaces", "Road Faces", "Structured faces");
    faces.winding_order = Some(WindingOrder::Counterclockwise);
    faces.associations = vec![Link::new("/Tiles/RoadFaces.gpkg", "describes").unwrap()];

    for (stem, record) in [("RoadNetwork", roads), ("RoadFaces", faces)] {
        store
            .write_resource_metadata(
                &format!(
                    "/Tiles/metadata/{stem}.{}",
                    profile.metadata_encoding().extension()
                ),
                &record,
            )
            .unwrap();
    }

    let root = tmp.path().join(profile.root_folder_name());

    (tmp, root)
}

/// Add one violation and one warning with a single `mkdir`.
///
/// `My Tiles` is a Requirement Name1 violation (`/req/core/name-spaces`) and,
/// being empty, a Recommendation `/req/core/name-empty-folders-A` warning. One
/// directory therefore takes a three-valued datastore to a report carrying all
/// **four** SARIF `kind`/`level` pairs, which is what honesty rule 3 is about.
fn add_violation_and_warning(root: &Path) {
    fs::create_dir(root.join("My Tiles")).unwrap();
}

/// Add one warning and nothing else: a well-named empty folder.
///
/// `Staging` rather than `Tiles`, which [`three_valued`] already populated —
/// a folder holding the two resource records is not empty, and would draw no
/// recommendation at all.
fn add_warning(root: &Path) {
    fs::create_dir(root.join("Staging")).unwrap();
}

/// The report a real `validate` produces for `root` under the yardstick every
/// test here states, so an expectation is drawn from the library rather than
/// guessed.
fn report_for(root: &Path) -> ConformanceReport {
    CdbDatastore::open(root)
        .unwrap()
        .validate(&SimulationProfile::json())
        .unwrap()
}

/// Assert that `root` genuinely reaches all three coverage states, and hand
/// back the report that proves it.
///
/// Every test calls this **before** asserting how the states render. Without
/// it a fixture that stopped producing `unchecked` — a library change, an
/// edited record — would leave every assertion below true of a report that
/// never exercised the rule, and the file would go green while the tool went
/// quiet about the one thing it exists to say.
fn assert_three_valued(root: &Path) -> ConformanceReport {
    let report = report_for(root);
    for wanted in [
        ContentCoverage::Checked,
        ContentCoverage::NoContent,
        ContentCoverage::Unchecked,
    ] {
        assert!(
            coverage_count(&report, wanted) > 0,
            "the fixture must put at least one class into `{wanted}`, or the \
             assertions below prove nothing:\n{report}"
        );
    }
    for class in [RequirementsClass::Geometry, RequirementsClass::Topology] {
        assert_eq!(
            report.class_coverage(class),
            ContentCoverage::Unchecked,
            "the fixture must leave {class} in the third state"
        );
    }

    report
}

/// How many classes a report puts into one coverage state.
fn coverage_count(report: &ConformanceReport, wanted: ContentCoverage) -> usize {
    report
        .classes()
        .filter(|(_, findings)| findings.coverage == wanted)
        .count()
}

/// The aggregate coverage sentence, built from the report rather than
/// hard-coded, so this file pins that every format *states* the tally and
/// leaves the library to decide what it counts.
fn expected_tally(report: &ConformanceReport) -> String {
    format!(
        "{} classes: {} checked, {} no content, {} not checked",
        report.classes().count(),
        coverage_count(report, ContentCoverage::Checked),
        coverage_count(report, ContentCoverage::NoContent),
        coverage_count(report, ContentCoverage::Unchecked),
    )
}

// ---------------------------------------------------------------------------
// Reading the artifacts
// ---------------------------------------------------------------------------

/// The line of a text report naming requirements class `class`.
fn row_for<'a>(text: &'a str, class: &str) -> &'a str {
    text.lines()
        .find(|line| {
            line.trim_start().starts_with('[')
                && line
                    .split_once(']')
                    .is_some_and(|(_, rest)| rest.trim_start().starts_with(class))
        })
        .unwrap_or_else(|| panic!("no row for {class} in:\n{text}"))
}

/// The JSON artifact's entry for one class.
fn class_entry<'a>(document: &'a Value, class: &str) -> &'a Value {
    document["classes"]
        .as_array()
        .expect("classes is an array")
        .iter()
        .find(|entry| entry["class"] == class)
        .unwrap_or_else(|| panic!("no class entry for {class} in:\n{document}"))
}

/// The SARIF run's results.
fn results_of(document: &Value) -> Vec<Value> {
    document["runs"][0]["results"]
        .as_array()
        .expect("a run carries a results array")
        .clone()
}

/// Every result citing `rule_id`.
fn results_for<'a>(results: &'a [Value], rule_id: &str) -> Vec<&'a Value> {
    results
        .iter()
        .filter(|result| result["ruleId"] == rule_id)
        .collect()
}

/// The one result citing `rule_id`.
fn result_for<'a>(results: &'a [Value], rule_id: &str) -> &'a Value {
    let matched = results_for(results, rule_id);
    assert_eq!(
        matched.len(),
        1,
        "expected exactly one result for {rule_id}, found {}",
        matched.len()
    );

    matched[0]
}

/// Every result's `(ruleId, kind, level)`, in document order.
///
/// The identity of a SARIF document for the purpose of "the report did not
/// change": which rules fired, how each was classified, and at what severity.
fn result_shape(document: &Value) -> Vec<(String, String, String)> {
    results_of(document)
        .iter()
        .map(|result| {
            let field = |name: &str| {
                result[name]
                    .as_str()
                    .unwrap_or_else(|| panic!("every result states {name}: {result}"))
                    .to_owned()
            };
            (field("ruleId"), field("kind"), field("level"))
        })
        .collect()
}

/// Assert that `text` names every one of `expected`.
fn names(text: &str, expected: &[&str], label: &str) {
    for fragment in expected {
        assert!(
            text.contains(fragment),
            "{label} should name {fragment:?}:\n{text}"
        );
    }
}

// ---------------------------------------------------------------------------
// Rule 1 — `unchecked` never renders as a bare green PASS
// ---------------------------------------------------------------------------

/// **Honesty rule 1**, in all three formats at once.
///
/// `docs/CONFORMANCE.md` §6 states the position plainly: for Geometry and
/// Topology a pass means "we did not look", never "we looked and it was fine".
/// Each format has its own way of saying that, and each of them is a way of
/// *not* saying it by accident — a `PASS` token, a missing `content` field, an
/// empty results array. All three are checked over one report, because a
/// datastore that reads honest in text and green in SARIF is exactly as
/// misleading as one that reads green everywhere.
#[test]
fn req_cdb_lint_unchecked_never_renders_as_a_pass_in_any_format() {
    let (_tmp, root) = three_valued(&SimulationProfile::json());
    let report = assert_three_valued(&root);

    // The classes the library actually left unjudged, taken from the report
    // rather than assumed — every format below has to say the same of exactly
    // these, and a fourth class arriving in the third state would join them
    // here instead of going unrendered and unnoticed.
    let unjudged: Vec<&str> = report
        .classes()
        .filter(|(_, findings)| findings.coverage == ContentCoverage::Unchecked)
        .map(|(class, _)| class.as_str())
        .collect();
    assert_eq!(unjudged, ["geometry", "topology"]);

    // text: the token, the note, and never the colour of a clean pass.
    let text = lint_as("text", &root, &[]);
    assert_eq!(text.code, exit::OK, "{}\n{}", text.out, text.err);
    for class in &unjudged {
        let row = row_for(&text.out, class);
        assert!(row.contains("[UNCHECKED]"), "{row:?}");
        assert!(!row.contains("[PASS]"), "{row:?}");
        assert!(
            row.contains("content present, no datastore-level check"),
            "the row says what the state means: {row:?}"
        );
    }
    let coloured = lint_as("text", &root, &["--color", "always"]);
    assert!(
        coloured
            .out
            .contains(&format!("{YELLOW}[UNCHECKED]{RESET}")),
        "UNCHECKED is yellow:\n{}",
        coloured.out
    );
    assert!(
        !coloured.out.contains(&format!("{GREEN}[UNCHECKED]")),
        "UNCHECKED must never be green, in any theme, under any flag:\n{}",
        coloured.out
    );

    // json: the wire token, beside the `passed` it must be read with.
    let json = lint_as("json", &root, &[]).document();
    for class in &unjudged {
        let entry = class_entry(&json, class);
        assert_eq!(entry["content"], "unchecked", "{entry}");
        assert_eq!(
            entry["passed"], true,
            "passed and content are independent, and this is the pair a reader \
             has to see together — the whole reason `content` is on the wire: {entry}"
        );
    }

    // sarif: a `review` result, and no `fail` standing in for it.
    let results = results_of(&lint_as("sarif", &root, &[]).document());
    for class in &unjudged {
        let filed: Vec<&Value> = results
            .iter()
            .filter(|result| result["properties"]["class"] == *class)
            .collect();
        assert_eq!(
            filed.len(),
            1,
            "{class} says exactly one thing in this document: {filed:?}"
        );
        assert_eq!(filed[0]["kind"], "review", "{}", filed[0]);
        assert_eq!(filed[0]["level"], "none", "{}", filed[0]);
        assert_ne!(
            filed[0]["kind"], "fail",
            "an unjudged class is not a finding, and a `fail` standing in for \
             one would tell a consumer the opposite of the truth: {}",
            filed[0]
        );
    }
}

// ---------------------------------------------------------------------------
// Rule 2 — no output omits coverage
// ---------------------------------------------------------------------------

/// **Honesty rule 2**: every format states how much was actually checked, and
/// all three agree on the numbers.
///
/// The formats discharge it differently — the text report prints a per-class
/// note and the aggregate line, the SARIF document carries the counts in
/// `run.properties`, and the JSON artifact cannot carry them at all without
/// ceasing to be the library's frozen wire shape, so its aggregate goes to
/// stderr while `content` carries the per-class half. Three mechanisms, one
/// fact, and a reader of any one of them learns it.
#[test]
fn req_cdb_lint_no_format_omits_coverage() {
    let (_tmp, root) = three_valued(&SimulationProfile::json());
    let report = assert_three_valued(&root);
    let tally = expected_tally(&report);

    // text: the per-class notes, and the aggregate.
    let text = lint_as("text", &root, &[]);
    names(&text.out, &[&tally], "the text report");
    for (class, note) in [
        ("geometry", "content present, no datastore-level check"),
        ("topology", "content present, no datastore-level check"),
        ("versioning", "no content this class governs"),
    ] {
        assert!(
            row_for(&text.out, class).contains(note),
            "{class} owes its reader the note {note:?}:\n{}",
            text.out
        );
    }

    // `--quiet` drops the rows that have nothing to report, which is what it
    // is for — and the aggregate survives it, so a reader who sees only the
    // summary still learns how much went unjudged.
    let quiet = lint_as("text", &root, &["--quiet"]);
    names(&quiet.out, &[&tally], "the quiet text report");

    // json: `content` on every class entry, aggregate on stderr.
    let json = lint_as("json", &root, &[]);
    names(&json.err, &[&tally], "stderr beside the JSON artifact");
    assert!(
        !json.out.contains("not checked"),
        "the artifact stays the frozen wire shape:\n{}",
        json.out
    );
    let document = json.document();
    for (class, findings) in report.classes() {
        let entry = class_entry(&document, class.as_str());
        assert_eq!(
            entry["content"],
            findings.coverage.as_str(),
            "{class} is on the wire with the coverage the library gave it"
        );
    }

    // sarif: `run.properties.coverage`, and the counts partition the classes.
    let sarif = lint_as("sarif", &root, &[]).document();
    let coverage = &sarif["runs"][0]["properties"]["coverage"];
    let count = |state: ContentCoverage| coverage_count(&report, state) as u64;
    assert_eq!(coverage["checked"], count(ContentCoverage::Checked));
    assert_eq!(coverage["none"], count(ContentCoverage::NoContent));
    assert_eq!(coverage["unchecked"], count(ContentCoverage::Unchecked));
    assert_eq!(
        coverage["checked"].as_u64().unwrap()
            + coverage["none"].as_u64().unwrap()
            + coverage["unchecked"].as_u64().unwrap(),
        report.classes().count() as u64,
        "the three counts partition the classes: {coverage}"
    );
}

// ---------------------------------------------------------------------------
// Rule 3 — SARIF carries coverage in `result.kind`
// ---------------------------------------------------------------------------

/// **Honesty rule 3**: all four `kind`/`level` pairs, in one document, each
/// written explicitly.
///
/// The omission is the hazard. SARIF defaults an absent `level` to `warning`,
/// so a result that left it out would render a violation as a warning and
/// would leave a `review` — the result meaning "this content went unjudged" —
/// indistinguishable from a finding. Writing both fields costs two keys and
/// removes the ambiguity in both directions.
///
/// The fixture reaches all four rows at once: a violation, a warning, two
/// classes whose content went unjudged, and four governing nothing.
#[test]
fn req_cdb_lint_sarif_states_coverage_in_result_kind() {
    let (_tmp, root) = three_valued(&SimulationProfile::json());
    add_violation_and_warning(&root);
    assert_three_valued(&root);

    let run = lint_as("sarif", &root, &[]);
    assert_eq!(run.code, exit::FINDINGS, "{}", run.err);
    let results = results_of(&run.document());

    for (label, rule_id, kind, level) in [
        (
            "a violation",
            "/req/core/name-spaces".to_owned(),
            "fail",
            "error",
        ),
        (
            "a warning",
            "/req/core/name-empty-folders-A".to_owned(),
            "fail",
            "warning",
        ),
        (
            "coverage `unchecked`",
            RequirementsClass::Geometry.requirements_uri().to_owned(),
            "review",
            "none",
        ),
        (
            "coverage `none`",
            RequirementsClass::Versioning.requirements_uri().to_owned(),
            "notApplicable",
            "none",
        ),
    ] {
        let result = result_for(&results, &rule_id);
        assert_eq!(result["kind"], kind, "{label}: {result}");
        assert_eq!(result["level"], level, "{label}: {result}");
    }

    for result in &results {
        assert!(
            result["kind"].is_string() && result["level"].is_string(),
            "both fields are explicit on every result, because both have \
             defaults a reader would otherwise be handed silently: {result}"
        );
        assert_ne!(
            result["kind"], "pass",
            "a clean checked class emits no result at all: the failures, the \
             unjudged, and the inapplicable are what a consumer acts on: {result}"
        );
    }
}

// ---------------------------------------------------------------------------
// Rule 4 — the two flags move the exit code, never the report
// ---------------------------------------------------------------------------

/// **Honesty rule 4**, the dangerous half: `--baseline` can exit 0 over a
/// datastore that does not conform.
///
/// That is what a ratchet is for, and the containment is that nothing else
/// moves. In all three formats the report is the same report — the same rows,
/// the same findings, the same `conformant: false` — and in the text rendering
/// the verdict still reads `NON-CONFORMANT` with the flag responsible named on
/// the same line. A run whose exit code were the only thing that spoke would
/// tell a CI job this datastore was fine.
#[test]
fn req_cdb_lint_a_baseline_moves_the_exit_code_and_not_the_report() {
    let (tmp, root) = three_valued(&SimulationProfile::json());
    add_violation_and_warning(&root);
    let report = assert_three_valued(&root);

    // Minted the way a user mints one, and outside the datastore: a baseline
    // dropped inside the root would become content the next run judges.
    let baseline = tmp.path().join("baseline.json");
    let minted = lint_as("json", &root, &["-o", &baseline.to_string_lossy()]);
    assert!(
        baseline.is_file(),
        "minting a baseline writes it (exit {}):\n{}",
        minted.code,
        minted.err
    );
    let baseline = baseline.to_string_lossy().into_owned();

    assert!(
        !report.is_conformant(),
        "the fixture must not conform, or the ratchet has nothing to absorb"
    );
    // The line the class rows end at, taken from the report rather than
    // written out, so the split cannot fall behind a twelfth class.
    let tally_marker = format!("\n{} classes:", report.classes().count());

    for format in FORMATS {
        let plain = lint_as(format, &root, &[]);
        let ratcheted = lint_as(format, &root, &["--baseline", &baseline]);

        assert_eq!(plain.code, exit::FINDINGS, "{format}:\n{}", plain.err);
        assert_eq!(
            ratcheted.code,
            exit::OK,
            "{format}: the baseline has to actually move the code, or this \
             proves nothing:\n{}",
            ratcheted.err
        );

        match format {
            "text" => {
                assert!(
                    ratcheted.out.contains("NON-CONFORMANT"),
                    "the verdict is unmoved:\n{}",
                    ratcheted.out
                );
                assert!(
                    ratcheted.verdict().ends_with("exit 0 by --baseline"),
                    "the line names the flag that took the exit code away from \
                     the verdict: {:?}",
                    ratcheted.verdict()
                );
                names(
                    &ratcheted.out,
                    &["[FAIL]", "file-naming", "/req/core/name-spaces"],
                    "the ratcheted report",
                );
                // Everything above the diff section is the report, unchanged.
                let body = |text: &str, marker: &str| {
                    text.split(marker)
                        .next()
                        .unwrap_or_default()
                        .trim_end()
                        .to_owned()
                };
                assert_eq!(
                    body(&plain.out, &tally_marker),
                    body(&ratcheted.out, "\nbaseline   "),
                    "the rows and the findings are the same rows and findings"
                );
            }
            "json" => {
                assert_eq!(
                    ratcheted.out, plain.out,
                    "the artifact is the same bytes: the diff is a conversation \
                     and never enters the document"
                );
                names(&ratcheted.err, &["baseline"], "the diff on stderr");
            }
            _ => {
                let document = ratcheted.document();
                assert_eq!(
                    document["runs"][0]["properties"]["conformant"], false,
                    "the document still says the datastore does not conform"
                );
                assert_eq!(
                    result_shape(&document),
                    result_shape(&plain.document()),
                    "the same rules fired, classified the same way, at the same \
                     levels; `baselineState` is the ratchet's only mark"
                );
            }
        }
    }
}

/// **Honesty rule 4**, the other direction: `--deny-warnings` fails a build
/// over a datastore the report calls **conformant**.
///
/// A warning is a SHOULD, and a SHOULD does not decide conformance, so the
/// flag cannot move the verdict — only the exit code, and the text verdict
/// line says which flag did it. In the two machine formats the document is
/// byte-identical with and without the flag: it records what the datastore is,
/// not what this run was asked to fail on.
#[test]
fn req_cdb_lint_deny_warnings_moves_the_exit_code_and_not_the_report() {
    let (_tmp, root) = three_valued(&SimulationProfile::json());
    add_warning(&root);
    let report = assert_three_valued(&root);

    assert!(
        report.is_conformant(),
        "the fixture must conform, or the flag is not the thing failing the build"
    );

    for format in FORMATS {
        let plain = lint_as(format, &root, &[]);
        let denied = lint_as(format, &root, &["--deny-warnings"]);

        assert_eq!(plain.code, exit::OK, "{format}:\n{}", plain.err);
        assert_eq!(
            denied.code,
            exit::FINDINGS,
            "{format}: the flag has to actually move the code:\n{}",
            denied.err
        );

        if format == "text" {
            assert!(
                !denied.out.contains("NON-CONFORMANT"),
                "a SHOULD never decides conformance:\n{}",
                denied.out
            );
            assert_eq!(
                denied.verdict(),
                "CONFORMANT (1 warning) — exit 1 by --deny-warnings",
                "a build failing over a datastore the report calls conformant \
                 is not a contradiction, but it is a surprise, and the line \
                 owes the reader the explanation"
            );
        } else {
            assert_eq!(
                denied.out, plain.out,
                "{format}: the document records the datastore, not the flags \
                 this run was given"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Rule 5 — detection informs diagnostics, never the yardstick
// ---------------------------------------------------------------------------

/// **Honesty rule 5**: cdb-lint may read a datastore to *suggest* a flag and
/// may never read one to *choose* it.
///
/// Deriving the encoding from the thing being judged would make Requirement
/// Metadata5 unfailable — the crate's own false-green failure mode,
/// reintroduced one layer up. So a run that names the wrong encoding keeps its
/// verdict in every format, the datastore is still convicted, and the
/// suggestion lives on stderr where it cannot be mistaken for part of the
/// report. A run that names no encoding does not run at all, however
/// confidently the datastore answers the question.
#[test]
fn req_cdb_lint_detection_suggests_a_flag_and_never_chooses_one() {
    let (_tmp, root) = three_valued(&SimulationProfile::json());
    assert_three_valued(&root);
    let root_text = root.to_string_lossy().into_owned();

    for format in FORMATS {
        // No encoding stated: exit 2 in every format, with a suggestion and
        // no verdict. The datastore's own answer is legible and still not
        // taken.
        let unstated = lint(&[
            "--profile",
            "simulation",
            "--format",
            format,
            &root_text[..],
        ]);
        assert_eq!(unstated.code, exit::USAGE, "{format}:\n{}", unstated.err);
        assert!(
            unstated.out.is_empty(),
            "{format}: nothing was judged, so stdout stays clean:\n{}",
            unstated.out
        );
        names(
            &unstated.err,
            &["global_metadata.json", "--encoding json"],
            "the suggestion",
        );

        // The wrong encoding stated: the run happens, and the datastore is
        // judged against what the command line said.
        let wrong = lint(&[
            "--profile",
            "simulation",
            "--encoding",
            "xml",
            "--format",
            format,
            &root_text[..],
        ]);
        assert_eq!(
            wrong.code,
            exit::FINDINGS,
            "{format}: the yardstick stands and convicts:\n{}",
            wrong.err
        );
        assert!(
            wrong.out.contains("/req/core/metadata-encoding"),
            "{format}: the finding stays in the report:\n{}",
            wrong.out
        );
        names(
            &wrong.err,
            &["--encoding json", "unchanged"],
            "the note on stderr",
        );

        // And the artifact is not the artifact of the run the hint suggests:
        // a suggestion that had quietly been taken would make these equal.
        let suggested = lint_as(format, &root, &[]);
        assert_eq!(suggested.code, exit::OK, "{format}:\n{}", suggested.err);
        assert_ne!(
            wrong.out, suggested.out,
            "{format}: the hint suggests a re-run; it never performs one"
        );
    }
}

// ---------------------------------------------------------------------------
// Rule 6 — a broken yardstick is a usage error, never a datastore finding
// ---------------------------------------------------------------------------

/// **Honesty rule 6**: every way a descriptor can be broken exits 2, in every
/// format, before the datastore is opened.
///
/// `storage_crs` and `language` return a `Result`, a declared attribute model
/// can be invalid, and `gpkg` parses as a metadata encoding this build does
/// not implement. Letting `validate` meet any of them would file the
/// *profile's* defect as a CRS, Metadata or Attribution violation against a
/// datastore that did nothing wrong — convicting the innocent party, and doing
/// it in a document somebody will quote.
///
/// The datastore here is the conformant three-valued fixture, so a verdict of
/// any kind in the output would be a tell.
#[test]
fn req_cdb_lint_a_broken_yardstick_is_a_usage_error_in_every_format() {
    let (_tmp, root) = three_valued(&SimulationProfile::json());
    assert_three_valued(&root);
    let workspace = tempfile::tempdir().unwrap();

    for (label, field, value) in [
        (
            "WKT that does not parse",
            "storage_crs_wkt",
            json!("GEOGCRS[\"unterminated\""),
        ),
        ("a malformed language tag", "language", json!("en_US!")),
        (
            "an invalid attribute model",
            "attribute_model",
            json!({
                "attributes": [
                    {"id": "1", "name": "StreetName", "description": "Name of a street"},
                    {"id": "1", "name": "StreetType", "description": "Type of a street"},
                ]
            }),
        ),
        (
            "an encoding this build does not implement",
            "metadata_encoding",
            json!("gpkg"),
        ),
    ] {
        let mut descriptor = json!({
            "name": "acme-sim",
            "case_rule": "PascalCase",
            "language": "en",
            "metadata_standard": "DCAT",
            "metadata_encoding": "json",
            "uom": "M",
            "storage_crs_wkt": WGS84_2D_WKT,
        });
        descriptor[field] = value.clone();
        let path = workspace.path().join(format!("{field}.json"));
        fs::write(&path, serde_json::to_string_pretty(&descriptor).unwrap()).unwrap();
        let path = path.to_string_lossy().into_owned();

        for format in FORMATS {
            let run = lint(&[
                "--profile-file",
                &path[..],
                "--format",
                format,
                &root.to_string_lossy(),
            ]);

            assert_eq!(
                run.code,
                exit::USAGE,
                "{label} in {format}: a broken yardstick is a usage error, \
                 never an operational failure and never a finding:\n{}",
                run.err
            );
            assert!(
                run.out.is_empty(),
                "{label} in {format}: nothing was judged, so there is no \
                 artifact:\n{}",
                run.out
            );
            names(&run.err, &[field], "the refusal");
            assert!(
                !run.err.contains("CONFORMANT"),
                "{label} in {format}: the datastore is not the party at fault, \
                 and no verdict about it was reached:\n{}",
                run.err
            );
        }
    }
}
