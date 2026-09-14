//! Integration: `cdb-lint explain` — what a finding code means, and what
//! happens when the code is not one.
//!
//! A report says `/req/core/attribute-model-content-B` and nothing more,
//! because a code is a stable identifier and prose is not. `explain` is the
//! other half of that bargain: the same routing rule as everywhere else —
//! the description is an artifact and goes to stdout, a failed lookup is a
//! conversation and goes to stderr — plus one deliberate asymmetry. Matching
//! a code is **case-sensitive**, because a code is an identifier; suggesting
//! one folds case, because a suggestion's job is to help.

use std::collections::BTreeSet;
use std::ffi::OsString;

use cdb_lint::catalogue::{CATALOGUE, lookup};
use cdb_lint::{Env, exit, run};

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

/// Drive `run` with `tokens`.
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

/// An exact lookup prints the code, its class, the clause it cites, and the
/// gloss — as an artifact, on stdout, exiting 0.
#[test]
fn cli_explain_describes_one_code() {
    let run = lint(&["explain", "/req/core/attribute-model-content-B"]);

    assert_eq!(run.code, exit::OK, "{}", run.err);
    assert!(
        run.err.is_empty(),
        "a description is not a diagnostic: {}",
        run.err
    );
    assert_eq!(
        run.out,
        "/req/core/attribute-model-content-B\n  \
         class    attribution\n  \
         spec     OGC 23-034 §7.1.2.3\n  \
         Each attribute in the model has a unique identifier\n"
    );
}

/// The spec line names the document, so a reader can find the clause without
/// already knowing which standard a CDB code belongs to.
#[test]
fn cli_explain_names_the_standard_and_the_clause() {
    let run = lint(&["explain", "/req/core/name-case"]);

    assert_eq!(run.code, exit::OK);
    assert!(run.out.contains("OGC 23-034 §7.4.7"), "{}", run.out);
}

/// `--list` prints the whole catalogue, one row per line, to stdout.
#[test]
fn cli_explain_list_prints_every_row() {
    let run = lint(&["explain", "--list"]);

    assert_eq!(run.code, exit::OK, "{}", run.err);
    assert!(run.err.is_empty(), "{}", run.err);

    let lines: Vec<&str> = run.out.lines().collect();
    assert_eq!(lines.len(), CATALOGUE.len(), "one line per row");
    assert_eq!(lines.len(), 82, "the vocabulary is 82 codes");

    for (line, entry) in lines.iter().zip(CATALOGUE) {
        assert!(
            line.starts_with(entry.code),
            "{line:?} should open with its code"
        );
        assert!(
            line.contains(entry.class),
            "{line:?} should carry its class"
        );
        assert!(
            line.contains(entry.section),
            "{line:?} should carry its section"
        );
        assert!(
            line.contains(entry.gloss),
            "{line:?} should carry its gloss"
        );
    }
}

/// The listing is aligned: the class and the section start in the same
/// column on every row, so the output reads as a table rather than as ragged
/// prose. The columns are located by walking past each field's padding,
/// which pins the alignment without pinning the widths.
#[test]
fn cli_explain_list_is_aligned() {
    let run = lint(&["explain", "--list"]);

    let mut class_columns = BTreeSet::new();
    let mut section_columns = BTreeSet::new();

    for (line, entry) in run.out.lines().zip(CATALOGUE) {
        let after_code = &line[entry.code.len()..];
        let class_at = entry.code.len() + after_code.len() - after_code.trim_start().len();
        assert!(line[class_at..].starts_with(entry.class), "{line:?}");
        class_columns.insert(class_at);

        let after_class = &line[class_at + entry.class.len()..];
        let section_at =
            class_at + entry.class.len() + after_class.len() - after_class.trim_start().len();
        assert!(line[section_at..].starts_with(entry.section), "{line:?}");
        section_columns.insert(section_at);
    }

    assert_eq!(
        class_columns.len(),
        1,
        "the class column moves: {class_columns:?}"
    );
    assert_eq!(
        section_columns.len(),
        1,
        "the section column moves: {section_columns:?}"
    );
}

/// An unknown code is a usage error: exit 2, on stderr, with stdout left
/// clean so a pipeline is never handed half an answer.
#[test]
fn cli_explain_rejects_an_unknown_code() {
    let run = lint(&["explain", "/req/core/not-a-clause"]);

    assert_eq!(run.code, exit::USAGE);
    assert!(run.out.is_empty(), "stdout should stay clean: {}", run.out);
    assert!(run.err.contains("/req/core/not-a-clause"), "{}", run.err);
}

/// A partial query is answered with the codes that contain it. A user who
/// typed a fragment is helped rather than stonewalled.
#[test]
fn cli_explain_suggests_codes_containing_the_query() {
    let run = lint(&["explain", "attribute-model"]);

    assert_eq!(run.code, exit::USAGE);
    assert!(run.out.is_empty(), "{}", run.out);
    for code in [
        "/req/core/attribute-model",
        "/req/core/attribute-model-A",
        "/req/core/attribute-model-C",
        "/req/core/attribute-model-content-B",
        "/req/core/attribute-model-content-C",
        "/req/core/attribute-model-content-D",
    ] {
        assert!(
            run.err.contains(code),
            "{code} is missing from: {}",
            run.err
        );
    }
    // Not a code that has nothing to do with the query.
    assert!(!run.err.contains("/req/core/name-spaces"), "{}", run.err);
}

/// A query nothing contains says so, and points at the listing rather than
/// leaving the user to guess what the vocabulary is.
#[test]
fn cli_explain_points_at_the_listing_when_nothing_matches() {
    let run = lint(&["explain", "zzz-no-such-thing"]);

    assert_eq!(run.code, exit::USAGE);
    assert!(run.out.is_empty(), "{}", run.out);
    assert!(run.err.contains("explain --list"), "{}", run.err);
}

/// Matching an exact code is case-sensitive: a code is an identifier, and
/// `TilingSchemeId::parse` sets the precedent in the library. The wrong case
/// therefore does not resolve — but the suggestion search folds case, so the
/// user is handed the spelling they meant instead of a dead end.
#[test]
fn cli_explain_exact_match_is_case_sensitive() {
    let run = lint(&["explain", "/REQ/CORE/NAME-SPACES"]);

    assert_eq!(run.code, exit::USAGE, "{}", run.out);
    assert!(run.out.is_empty(), "{}", run.out);
    assert!(
        run.err.contains("/req/core/name-spaces"),
        "the right spelling should be suggested: {}",
        run.err
    );
}

/// The class a code is filed under is the class the **report** files it
/// under, not the section of `docs/CONFORMANCE.md` the requirement is
/// documented in.
///
/// `/req/core/file-cdb-root-location` is the trap: it is Requirement File2's
/// clause and `CONFORMANCE.md` documents it under §4.2 File Structure, but
/// its only finding is `NamingViolation::PathTraversal`, which
/// `CdbViolation::class` files under **File Naming**. A user who saw the
/// code in a report saw it under file-naming, and `explain` has to agree
/// with what they are holding.
#[test]
fn cli_explain_class_is_the_class_the_report_files_it_under() {
    let run = lint(&["explain", "/req/core/file-cdb-root-location"]);

    assert_eq!(run.code, exit::OK, "{}", run.err);
    assert!(run.out.contains("class    file-naming"), "{}", run.out);

    // The same rule, the other documented crossing: a §7.4 naming
    // recommendation that only the hierarchy walk can detect.
    let run = lint(&["explain", "/req/core/name-empty-folders-A"]);
    assert!(run.out.contains("class    file-structure"), "{}", run.out);
}

/// Annex A's bundle is filed under whichever mandatory class the profile
/// omitted, so `explain` says that rather than naming one of the five.
#[test]
fn cli_explain_annex_a_has_no_single_class() {
    let run = lint(&["explain", "/conf/minimal-core"]);

    assert_eq!(run.code, exit::OK, "{}", run.err);
    assert!(run.out.contains("OGC 23-034 §A.2"), "{}", run.out);
    assert!(
        run.out.contains("mandatory class"),
        "the varying class should be spelled out: {}",
        run.out
    );
}

/// Every catalogue row is reachable through `explain`, and each prints its
/// own gloss — the listing and the single lookup read the same table.
#[test]
fn cli_explain_resolves_every_catalogued_code() {
    for entry in CATALOGUE {
        let run = lint(&["explain", entry.code]);

        assert_eq!(run.code, exit::OK, "{}: {}", entry.code, run.err);
        assert!(
            run.out.starts_with(entry.code),
            "{} printed {:?}",
            entry.code,
            run.out
        );
        assert!(run.out.contains(entry.gloss), "{}: {}", entry.code, run.out);
        assert_eq!(lookup(entry.code), Some(entry));
    }
}
