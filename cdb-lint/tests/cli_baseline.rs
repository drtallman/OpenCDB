//! Integration: `--baseline`, the ratchet — the most dangerous feature in the
//! tool, and the one whose tests are mostly about containing it.
//!
//! A baseline lets a run exit 0 over a datastore that does not conform. That
//! is what a ratchet is for, and it is exactly the shape of the lie this whole
//! tool was written to avoid, so the containment is the substance here: the
//! report never changes, the verdict still reads `NON-CONFORMANT`, and the
//! verdict line names the flag that moved the exit code away from it (design
//! §5 rule 4).
//!
//! **Every baseline is minted the way a user mints one** — `cdb-lint --format
//! json -o baseline.json` — and then read back. That round trip *is* the
//! contract: if the writer and the reader ever disagree about the wire shape,
//! every test in this file fails at once rather than the disagreement being
//! discovered by somebody's CI.
//!
//! Every fixture is a real datastore, for the reason `cli_check.rs` records:
//! these are claims about what a datastore makes the library say, and a
//! hand-built report could be made to say anything.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use cdb_lint::snapshot::ReportSnapshot;
use cdb_lint::{Env, exit, run};
use opencdb::conformance::{ConformanceReport, RequirementsClass};
use opencdb::profiles::ApplicationProfile;
use opencdb::{CdbDatastore, DatastoreSeed, SimulationProfile};

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

/// Drive [`run`] over `root` under a `<profile>`/`json` yardstick.
fn lint_as(profile: &str, root: &Path, extra: &[&str]) -> Run {
    let mut tokens = vec!["--profile", profile, "--encoding", "json"];
    tokens.extend_from_slice(extra);
    let root = root.to_string_lossy().into_owned();
    tokens.push(&root);

    lint(&tokens)
}

/// Mint a baseline the way a user does, and hand back the path.
///
/// Nothing here writes into the datastore: a baseline dropped inside the root
/// would become content the next run judges, which is a fine way to make a
/// test measure itself.
fn mint(root: &Path, into: &Path) -> PathBuf {
    mint_as("simulation", root, into)
}

/// [`mint`] under a named built-in profile.
fn mint_as(profile: &str, root: &Path, into: &Path) -> PathBuf {
    let path = into.to_string_lossy().into_owned();
    let run = lint_as(profile, root, &["--format", "json", "-o", &path]);

    assert!(
        into.is_file(),
        "minting a baseline should write it (exit {}):\n{}",
        run.code,
        run.err
    );

    into.to_path_buf()
}

/// Run `root` against `baseline`.
fn against(root: &Path, baseline: &Path, extra: &[&str]) -> Run {
    let baseline = baseline.to_string_lossy().into_owned();
    let mut tokens = vec!["--baseline", &baseline[..]];
    tokens.extend_from_slice(extra);

    lint_as("simulation", root, &tokens)
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
/// class is declared, listed, and empty. Conformant, with no findings at all.
///
/// The temporary directory is the *parent* of the root, so a baseline written
/// beside it is near the datastore without being in it.
fn bare(profile: &dyn ApplicationProfile) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    CdbDatastore::create(tmp.path(), profile, seed()).unwrap();
    let root = tmp.path().join(profile.root_folder_name());

    (tmp, root)
}

/// Add one Requirement Name1 violation (`/req/core/name-spaces`): a directory
/// whose name carries a space. The payload inside keeps the empty-folder
/// recommendation out of it, so the fixture adds exactly one finding.
fn add_named_violation(root: &Path, name: &str) {
    let directory = root.join(name);
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("Payload.gpkg"), b"x").unwrap();
}

/// Add one Recommendation `/req/core/name-empty-folders-A` warning.
fn add_warning(root: &Path, name: &str) {
    fs::create_dir(root.join(name)).unwrap();
}

/// The report a real `validate` produces for `root` under `simulation`/json,
/// so an expectation can be drawn from the library rather than guessed.
fn report_for(root: &Path) -> ConformanceReport {
    let store = CdbDatastore::open(root).unwrap();

    store.validate(&SimulationProfile::json()).unwrap()
}

/// How many violations a report carries, over the classes it lists.
fn violation_count(report: &ConformanceReport) -> usize {
    report
        .classes()
        .map(|(_, findings)| findings.violations.len())
        .sum()
}

/// Assert that `text` names every one of `expected`.
fn names(text: &str, expected: &[&str], label: &str) {
    for fragment in expected {
        assert!(
            text.contains(fragment),
            "{label} should contain {fragment:?}:\n{text}"
        );
    }
}

// ---------------------------------------------------------------------------
// The ratchet itself
// ---------------------------------------------------------------------------

/// **Honesty rule 4.** A baseline can exit 0 over a datastore that does not
/// conform — that is what a ratchet is — and the containment is that nothing
/// else moves: the report is the same report, the verdict still reads
/// `NON-CONFORMANT`, and the last line names the flag that took the exit code
/// away from it.
///
/// A run whose exit code were the only thing that spoke would tell a CI job
/// this datastore was fine. This is the test that says it never does.
#[test]
fn req_cdb_lint_the_ratchet_exits_0_and_still_says_non_conformant() {
    let (tmp, root) = bare(&SimulationProfile::json());
    add_named_violation(&root, "My Tiles");
    let baseline = mint(&root, &tmp.path().join("baseline.json"));

    let violations = violation_count(&report_for(&root));
    assert!(violations > 0, "the fixture must not conform");

    let run = against(&root, &baseline, &[]);

    assert_eq!(run.code, exit::OK, "stdout:\n{}\n{}", run.out, run.err);
    assert!(run.out.contains("NON-CONFORMANT"), "{}", run.out);
    assert_eq!(
        run.verdict(),
        format!(
            "NON-CONFORMANT ({violations} {}) — no new findings since baseline; \
             exit 0 by --baseline",
            if violations == 1 {
                "violation"
            } else {
                "violations"
            }
        ),
        "the exit code is never the only thing that speaks:\n{}",
        run.out
    );
    // The report itself is untouched: the finding is still printed, still
    // under its class, still with its code.
    names(
        &run.out,
        &["[FAIL]", "file-naming", "/req/core/name-spaces"],
        "the report",
    );
}

/// **Honesty rule 4, the other half.** `--baseline` changes the exit code and
/// adds a section. It changes nothing the report *says*: the same rows, the
/// same findings, in the same order, and — in the machine format — the same
/// bytes.
///
/// This is the containment that makes a ratchet safe to ship. A baseline that
/// could quietly drop a finding from the report would be a tool for hiding
/// them, and the section would be the alibi.
#[test]
fn req_cdb_lint_a_baseline_changes_the_exit_code_and_nothing_the_report_says() {
    let (tmp, root) = bare(&SimulationProfile::json());
    add_named_violation(&root, "My Tiles");
    let baseline = mint(&root, &tmp.path().join("baseline.json"));

    let plain = lint_as("simulation", &root, &[]);
    let ratcheted = against(&root, &baseline, &[]);

    assert_eq!(plain.code, exit::FINDINGS, "{}", plain.out);
    assert_eq!(
        ratcheted.code,
        exit::OK,
        "the baseline has to actually move the code, or this proves nothing"
    );

    // Everything above the diff section is the report, and it is the same
    // report.
    let body = |text: &str, marker: &str| {
        text.split(marker)
            .next()
            .unwrap_or_default()
            .trim_end()
            .to_owned()
    };
    assert_eq!(
        body(&plain.out, "\n11 classes:"),
        body(&ratcheted.out, "\nbaseline   "),
    );

    // And the machine artifact is the same bytes either way: the diff is a
    // conversation, so it never enters the document.
    let plain = lint_as("simulation", &root, &["--format", "json"]);
    let ratcheted = against(&root, &baseline, &["--format", "json"]);
    assert_eq!(plain.out, ratcheted.out);
}

/// A clean run against its own baseline: nothing is new, and the verdict is
/// the plain one — no flag moved the exit code, so no flag is named.
#[test]
fn cli_baseline_a_clean_run_against_its_own_baseline_is_quiet() {
    let (tmp, root) = bare(&SimulationProfile::json());
    let baseline = mint(&root, &tmp.path().join("baseline.json"));

    let run = against(&root, &baseline, &[]);

    assert_eq!(run.code, exit::OK, "{}\n{}", run.out, run.err);
    assert_eq!(run.verdict(), "CONFORMANT — no new findings since baseline");
    names(
        &run.out,
        &["baseline", "no new findings since baseline"],
        "the report",
    );
}

// ---------------------------------------------------------------------------
// What counts as new
// ---------------------------------------------------------------------------

/// A finding the baseline never recorded fails the build, and the diff names
/// the `(class, code)` pair that did it.
#[test]
fn cli_baseline_a_new_finding_fails_the_build() {
    let (tmp, root) = bare(&SimulationProfile::json());
    let baseline = mint(&root, &tmp.path().join("baseline.json"));
    add_named_violation(&root, "My Tiles");

    let run = against(&root, &baseline, &[]);

    assert_eq!(run.code, exit::FINDINGS, "{}\n{}", run.out, run.err);
    names(
        &run.out,
        &["new", "file-naming", "/req/core/name-spaces"],
        "the diff",
    );
    assert!(
        run.verdict().starts_with("NON-CONFORMANT"),
        "{}",
        run.verdict()
    );
    assert!(
        run.verdict().contains("1 new violation since baseline"),
        "{}",
        run.verdict()
    );
}

/// Identity is `(class, code, severity) → count`, so a **second** occurrence of
/// a code the baseline already records is new too. A ratchet that counted only
/// distinct codes would absorb a datastore that got steadily worse in one
/// place.
#[test]
fn cli_baseline_a_count_increase_is_new() {
    let (tmp, root) = bare(&SimulationProfile::json());
    add_named_violation(&root, "My Tiles");
    let baseline = mint(&root, &tmp.path().join("baseline.json"));
    add_named_violation(&root, "Your Tiles");

    let run = against(&root, &baseline, &[]);

    assert_eq!(run.code, exit::FINDINGS, "{}\n{}", run.out, run.err);
    names(
        &run.out,
        &["increased", "/req/core/name-spaces", "(baseline 1, now 2)"],
        "the diff",
    );
}

/// Message text is deliberately **not** part of the identity: `src/lib.rs` in
/// the library says message wording may change within `1.x`, so a
/// message-keyed baseline would fail spuriously the day a sentence improved.
/// Two different bad names produce two findings with one key, which is the
/// same fact seen from the other side.
#[test]
fn cli_baseline_identity_ignores_the_message_text() {
    let (tmp, root) = bare(&SimulationProfile::json());
    add_named_violation(&root, "My Tiles");
    let baseline = mint(&root, &tmp.path().join("baseline.json"));

    // Same class, same code, same severity — a different message.
    fs::remove_dir_all(root.join("My Tiles")).unwrap();
    add_named_violation(&root, "Your Tiles");

    let run = against(&root, &baseline, &[]);

    assert_eq!(
        run.code,
        exit::OK,
        "a re-worded finding is not a new finding:\n{}\n{}",
        run.out,
        run.err
    );
    assert!(
        run.out.contains("Your Tiles"),
        "the report still prints the library's own message:\n{}",
        run.out
    );
}

/// A finding that went away is reported and changes nothing: a build never
/// fails because it got better.
#[test]
fn cli_baseline_a_resolved_finding_is_reported_and_passes() {
    let (tmp, root) = bare(&SimulationProfile::json());
    add_named_violation(&root, "My Tiles");
    let baseline = mint(&root, &tmp.path().join("baseline.json"));
    fs::remove_dir_all(root.join("My Tiles")).unwrap();

    let run = against(&root, &baseline, &[]);

    assert_eq!(run.code, exit::OK, "{}\n{}", run.out, run.err);
    names(
        &run.out,
        &["resolved", "/req/core/name-spaces", "(baseline 1, now 0)"],
        "the diff",
    );
    assert_eq!(run.verdict(), "CONFORMANT — no new findings since baseline");
}

// ---------------------------------------------------------------------------
// Warnings, and the two flags that move an exit code
// ---------------------------------------------------------------------------

/// New warnings are always reported and fail the build only when asked to.
/// Both flags then have to be legible on the verdict line: `--baseline` moves
/// an exit code down, `--deny-warnings` moves it up, and a line that named
/// neither would leave a reader guessing which.
#[test]
fn cli_baseline_new_warnings_fail_only_under_deny_warnings() {
    let (tmp, root) = bare(&SimulationProfile::json());
    let baseline = mint(&root, &tmp.path().join("baseline.json"));
    add_warning(&root, "Tiles");

    let allowed = against(&root, &baseline, &[]);
    assert_eq!(allowed.code, exit::OK, "{}\n{}", allowed.out, allowed.err);
    names(
        &allowed.out,
        &["new", "warning", "/req/core/name-empty-folders-A"],
        "the diff",
    );

    let denied = against(&root, &baseline, &["--deny-warnings"]);
    assert_eq!(
        denied.code,
        exit::FINDINGS,
        "{}\n{}",
        denied.out,
        denied.err
    );
    assert_eq!(
        denied.verdict(),
        "CONFORMANT (1 warning) — 1 new warning since baseline; exit 1 by --deny-warnings"
    );
}

/// The other direction of the same interaction: `--deny-warnings` would fail
/// this build, and the baseline says the warnings are old news. The exit code
/// is 0 over warnings that `--deny-warnings` was asked to fail on, so the line
/// has to say which flag won.
#[test]
fn req_cdb_lint_a_baseline_absorbs_the_warnings_deny_warnings_would_fail_on() {
    let (tmp, root) = bare(&SimulationProfile::json());
    add_warning(&root, "Tiles");
    let baseline = mint(&root, &tmp.path().join("baseline.json"));

    let run = against(&root, &baseline, &["--deny-warnings"]);

    assert_eq!(run.code, exit::OK, "{}\n{}", run.out, run.err);
    assert_eq!(
        run.verdict(),
        "CONFORMANT (1 warning) — no new findings since baseline; exit 0 by --baseline"
    );
}

// ---------------------------------------------------------------------------
// Refusing a baseline that is not this run's
// ---------------------------------------------------------------------------

/// A baseline taken under another profile is refused, exit 2. Two profiles
/// judge the same datastore by different rules, so absorbing one's findings
/// into the other's run would silently swallow real ones.
#[test]
fn cli_baseline_refuses_a_baseline_from_another_profile() {
    let (tmp, root) = bare(&SimulationProfile::json());
    let baseline = mint_as("gnosis", &root, &tmp.path().join("gnosis.json"));

    let run = against(&root, &baseline, &[]);

    assert_eq!(run.code, exit::USAGE, "{}\n{}", run.out, run.err);
    assert!(run.out.is_empty(), "nothing was judged:\n{}", run.out);
    names(&run.err, &["gnosis", "simulation"], "the refusal");
}

/// A baseline whose `root` differs is accepted: CI paths move, and a
/// datastore's identity is not its path.
#[test]
fn cli_baseline_accepts_a_baseline_whose_root_differs() {
    let (tmp, root) = bare(&SimulationProfile::json());
    let (_other_tmp, other_root) = bare(&SimulationProfile::json());
    let baseline = mint(&other_root, &tmp.path().join("elsewhere.json"));

    assert_ne!(root, other_root);

    let run = against(&root, &baseline, &[]);

    assert_eq!(run.code, exit::OK, "{}\n{}", run.out, run.err);
    assert!(
        run.out.contains("no new findings since baseline"),
        "{}",
        run.out
    );
}

// ---------------------------------------------------------------------------
// A baseline this run cannot use at all
// ---------------------------------------------------------------------------

/// A baseline that is not there is a configuration error, catchable before
/// anything is judged: exit 2, and stdout stays empty.
#[test]
fn cli_baseline_a_missing_file_is_a_usage_error() {
    let (tmp, root) = bare(&SimulationProfile::json());
    let missing = tmp.path().join("no-such-baseline.json");

    let run = against(&root, &missing, &[]);

    assert_eq!(run.code, exit::USAGE, "{}\n{}", run.out, run.err);
    assert!(run.out.is_empty(), "nothing was judged:\n{}", run.out);
    assert!(
        run.err.contains("no-such-baseline.json"),
        "the diagnostic names the file:\n{}",
        run.err
    );
}

/// A file that is not a report is exit 2 as well, and the message says what
/// was expected — a user who pointed `--baseline` at the wrong file needs to
/// know what the right one looks like.
#[test]
fn cli_baseline_a_malformed_file_is_a_usage_error() {
    let (tmp, root) = bare(&SimulationProfile::json());

    for (name, content) in [
        ("not-json.json", "this is not JSON at all"),
        ("not-a-report.json", r#"{"hello": "world"}"#),
        ("an-array.json", "[]"),
        (
            "half-a-report.json",
            r#"{"profile": "simulation", "root": "/cdb", "conformant": true}"#,
        ),
    ] {
        let path = tmp.path().join(name);
        fs::write(&path, content).unwrap();

        let run = against(&root, &path, &[]);

        assert_eq!(run.code, exit::USAGE, "{name}:\n{}\n{}", run.out, run.err);
        assert!(
            run.out.is_empty(),
            "{name}: nothing was judged:\n{}",
            run.out
        );
        names(&run.err, &[name, "--format json"], "the diagnostic");
    }
}

// ---------------------------------------------------------------------------
// The readback type
// ---------------------------------------------------------------------------

/// The keys of a JSON object, sorted, so a set comparison reads as one
/// assertion.
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

/// [`ReportSnapshot`] is a DTO for a shape it does not own, so the shape is
/// pinned against a **real serialized report** rather than against the DTO's
/// own idea of itself. Not "contains" but "equals": an added field is as much
/// a drift as a removed one.
///
/// The strictness lives here rather than on the runtime type, deliberately.
/// `#[serde(deny_unknown_fields)]` on [`ReportSnapshot`] would make a baseline
/// written by a *newer* cdb-lint unreadable by an older one, which is the
/// opposite of what a baseline is for — so the runtime type is tolerant and
/// this test is strict.
#[test]
fn cli_baseline_the_snapshot_mirrors_the_frozen_wire_shape() {
    let profile = SimulationProfile::json();
    let (_tmp, root) = bare(&profile);
    add_named_violation(&root, "My Tiles");
    add_warning(&root, "Tiles");

    let report = report_for(&root);
    let document = serde_json::to_value(&report).expect("a report serializes");

    assert_eq!(
        keys(&document),
        ["classes", "conformant", "profile", "root"]
    );

    let mut findings = 0usize;
    for entry in document["classes"].as_array().expect("classes is an array") {
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
        for (list, severity) in [("violations", "violation"), ("warnings", "warning")] {
            for finding in entry[list].as_array().expect("an array of findings") {
                assert_eq!(keys(finding), ["class", "code", "message", "severity"]);
                assert_eq!(
                    finding["severity"], severity,
                    "the severity token is half the baseline key"
                );
                findings += 1;
            }
        }
    }
    assert!(findings > 0, "the fixture must produce findings to pin");

    let snapshot: ReportSnapshot =
        serde_json::from_value(document).expect("the DTO reads the wire shape");
    assert_eq!(snapshot.profile, report.profile());
    assert_eq!(snapshot.classes.len(), RequirementsClass::ALL.len());
}

/// At runtime the DTO is tolerant of what it does not know: a baseline minted
/// by a newer cdb-lint, or by a newer library, still reads. The identity it
/// keys on is `(class, code, severity)`, and a field nobody here has heard of
/// cannot change that.
#[test]
fn cli_baseline_the_snapshot_tolerates_unknown_fields() {
    let (tmp, root) = bare(&SimulationProfile::json());
    add_named_violation(&root, "My Tiles");
    let baseline = mint(&root, &tmp.path().join("baseline.json"));

    let mut document: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&baseline).unwrap()).unwrap();
    document["from_the_future"] = serde_json::json!("a field this build never heard of");
    for entry in document["classes"].as_array_mut().unwrap() {
        entry["also_new"] = serde_json::json!(42);
        for finding in entry["violations"].as_array_mut().unwrap() {
            finding["fingerprint"] = serde_json::json!("abc123");
        }
    }
    fs::write(&baseline, serde_json::to_string_pretty(&document).unwrap()).unwrap();

    let run = against(&root, &baseline, &[]);

    assert_eq!(run.code, exit::OK, "{}\n{}", run.out, run.err);
    assert!(
        run.out.contains("no new findings since baseline"),
        "the unknown fields changed nothing:\n{}",
        run.out
    );
}

// ---------------------------------------------------------------------------
// The other two formats
// ---------------------------------------------------------------------------

/// The JSON artifact is the frozen wire shape and stays that way: the diff is
/// a conversation, so it goes to stderr beside the coverage tally already
/// there. A decorated artifact would no longer be the document a later run can
/// read back as a baseline.
#[test]
fn cli_baseline_json_keeps_its_artifact_and_puts_the_diff_on_stderr() {
    let (tmp, root) = bare(&SimulationProfile::json());
    let baseline = mint(&root, &tmp.path().join("baseline.json"));
    add_named_violation(&root, "My Tiles");

    let run = against(&root, &baseline, &["--format", "json"]);

    assert_eq!(run.code, exit::FINDINGS, "{}", run.err);
    let expected = format!(
        "{}\n",
        serde_json::to_string_pretty(&report_for(&root)).unwrap()
    );
    assert_eq!(run.out, expected, "the artifact is the report, unadorned");
    names(
        &run.err,
        &["baseline", "new", "/req/core/name-spaces"],
        "the diff on stderr",
    );
}

/// SARIF carries the diff where SARIF carries it: `result.baselineState` on
/// every result, `new` or `unchanged` and nothing else. Resolved findings are
/// **not** synthesized as `absent` results — the text diff reports those — so
/// a consumer never sees a result for a finding that no longer exists.
#[test]
fn cli_baseline_sarif_marks_every_result_new_or_unchanged() {
    let (tmp, root) = bare(&SimulationProfile::json());
    add_named_violation(&root, "My Tiles");
    let baseline = mint(&root, &tmp.path().join("baseline.json"));
    add_named_violation(&root, "Your Tiles");

    let run = against(&root, &baseline, &["--format", "sarif"]);
    assert_eq!(run.code, exit::FINDINGS, "{}", run.err);

    let document: serde_json::Value = serde_json::from_str(&run.out).expect("SARIF parses");
    let results = document["runs"][0]["results"]
        .as_array()
        .expect("an array of results");
    assert!(!results.is_empty());

    let mut states = Vec::new();
    for result in results {
        let state = result["baselineState"]
            .as_str()
            .unwrap_or_else(|| panic!("every result carries a baselineState: {result}"));
        assert!(
            matches!(state, "new" | "unchanged"),
            "only two of the four values are used: {state}"
        );
        if result["ruleId"] == "/req/core/name-spaces" {
            states.push(state.to_owned());
        }
    }
    assert!(
        !results
            .iter()
            .any(|result| result["baselineState"] == "absent"),
        "a resolved finding is never synthesized as a result"
    );
    assert_eq!(
        states,
        ["unchanged", "new"],
        "the baseline recorded one of this triple, so the first result in \
         report order is unchanged and the rest are new"
    );
}

/// Without `--baseline` there is nothing to compare against, so no result
/// claims a baseline state. SARIF's own default for an absent
/// `baselineState` is "unknown", which is the truth here.
#[test]
fn cli_baseline_sarif_omits_the_state_when_no_baseline_is_given() {
    let (_tmp, root) = bare(&SimulationProfile::json());
    add_named_violation(&root, "My Tiles");

    let run = lint_as("simulation", &root, &["--format", "sarif"]);

    let document: serde_json::Value = serde_json::from_str(&run.out).expect("SARIF parses");
    for result in document["runs"][0]["results"].as_array().unwrap() {
        assert!(
            result.get("baselineState").is_none(),
            "no baseline, no state: {result}"
        );
    }
}
