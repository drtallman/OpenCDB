//! A staleness guard for [`cdb_lint::catalogue::CATALOGUE`].
//!
//! The catalogue is documentation, hand-authored against OGC 23-034 and
//! `docs/CONFORMANCE.md`. This test is the machinery that stops it becoming
//! *stale* documentation: it scans the library's `src/conformance/` tree for
//! the code literals the library can emit and asserts an **exact two-way
//! match** against the catalogue, so a new code in `opencdb` fails
//! cdb-lint's build until somebody writes its row.
//!
//! # What this proves, and what it does not
//!
//! It proves **completeness in both directions**: every code the library can
//! put on the wire has a row, and every row names a code the library can
//! actually emit. It also checks the *shape* of the other three columns — a
//! `class` that is a real requirements-class token, a `section` that looks
//! like a clause number, no duplicate codes, and the whole list in code
//! order.
//!
//! It does **not** prove the `class`, `section` or `gloss` columns are
//! right. Those are hand-authored and reviewed by eye against the standard;
//! nothing here reads OGC 23-034. A row can be complete, well-shaped, sorted
//! — and still cite the wrong clause. `explain` says as much rather than
//! implying the catalogue is derived from the document it points at.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use cdb_lint::catalogue::{self, CATALOGUE, Entry};
use opencdb::conformance::RequirementsClass;

/// The library's conformance tree, found relative to *this* crate rather
/// than to the working directory, which cargo is free to choose.
fn conformance_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/conformance")
}

/// Every `.rs` file under `root`, recursively, in path order so that a
/// failure message reads the same on every machine.
fn rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir)
            .unwrap_or_else(|error| panic!("{} must be readable: {error}", dir.display()));
        for entry in entries {
            let path = entry
                .unwrap_or_else(|error| panic!("{} must be readable: {error}", dir.display()))
                .path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                found.push(path);
            }
        }
    }

    found.sort();
    found
}

/// The files a `#[cfg(test)] mod <name>;` declaration in `sources` puts
/// behind `cfg(test)` in their entirety.
///
/// `src/conformance/validate/support.rs` is one: it is fixture-building code
/// for the orchestrator's own tests, and a literal there is not a code the
/// library can emit. Resolving the declaration beats hard-coding the name,
/// which would silently stop covering a second such module.
fn test_only_modules(sources: &[PathBuf]) -> BTreeSet<PathBuf> {
    let mut skipped = BTreeSet::new();

    for path in sources {
        let text = read(path);
        let lines: Vec<&str> = text.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            if line.trim() != "#[cfg(test)]" {
                continue;
            }
            let Some(next) = lines.get(index + 1) else {
                continue;
            };
            let Some(name) = declared_module(next) else {
                continue;
            };
            let Some(dir) = path.parent() else { continue };
            skipped.insert(dir.join(format!("{name}.rs")));
            skipped.insert(dir.join(name).join("mod.rs"));
        }
    }

    skipped
}

/// The module name in a `mod <name>;` declaration — a file module, never an
/// inline `mod <name> { … }`.
fn declared_module(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix("pub ").unwrap_or(line.trim());
    let name = rest.strip_prefix("mod ")?.strip_suffix(';')?.trim();

    name.chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_')
        .then_some(name)
}

/// `text` truncated at the file's inline `#[cfg(test)] mod tests { … }`
/// block, which runs to the end of the file in every module of the tree.
fn without_inline_tests(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();

    for (index, line) in lines.iter().enumerate() {
        if line.trim() != "#[cfg(test)]" {
            continue;
        }
        let is_inline = lines
            .get(index + 1)
            .is_some_and(|next| next.trim_end().ends_with('{') && next.contains("mod "));
        if is_inline {
            return lines[..index].join("\n");
        }
    }

    text.to_owned()
}

/// Every finding-code literal in `text`: a double-quoted string opening with
/// one of the four clause families.
///
/// A literal ending in a solidus is a *prefix fragment* — `"/req/core/"` and
/// its siblings appear in doc comments explaining the vocabulary — and names
/// no clause, so it is discarded.
fn code_literals(text: &str) -> Vec<String> {
    const FAMILIES: [&str; 4] = ["/req/", "/rec/", "/per/", "/conf/"];

    let mut found = Vec::new();
    for candidate in text.split('"').skip(1).step_by(2) {
        if FAMILIES.iter().any(|family| candidate.starts_with(family))
            && !candidate.ends_with('/')
            && !candidate.contains(char::is_whitespace)
        {
            found.push(candidate.to_owned());
        }
    }

    found
}

/// Read a file or fail with the path in the message.
fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{} must be readable: {error}", path.display()))
}

/// Constructs that would desynchronize [`code_literals`]'s split-on-quote
/// scan: a quote written as a char literal, or a file whose quote characters
/// do not pair up (one string literal carrying an odd number of escaped
/// quotes). Either puts every literal after it off-phase, so a code added
/// below the construct would be invisible to the scan — the *silent* version
/// of the failure this file exists to make loud. Refusing the construct
/// keeps the scanner simple and the failure noisy: whoever introduces one is
/// told to rephrase it or to teach this guard to lex.
///
/// Paired escaped quotes inside one literal are tolerated: they leave the
/// file's phase intact, and the mis-split fragments they produce fail the
/// family filter. An independent lexer confirmed the scan and the tree agree
/// under exactly these rules (final review, 2026-09-14).
fn parity_hazards(text: &str) -> Vec<String> {
    let mut hazards = Vec::new();
    if let Some(index) = text.find("'\"'") {
        hazards.push(format!("a quote char-literal at byte {index}"));
    }
    let quotes = text.bytes().filter(|byte| *byte == b'"').count();
    if quotes % 2 != 0 {
        hazards.push(format!("an odd number of quote characters ({quotes})"));
    }

    hazards
}

/// Every code the library's non-test conformance source can emit, each with
/// the files it appears in.
fn emitted_codes() -> BTreeMap<String, BTreeSet<String>> {
    let root = conformance_dir();
    let sources = rust_sources(&root);
    assert!(
        sources.len() >= 4,
        "{} should hold the conformance modules, found {sources:?}",
        root.display()
    );
    let skipped = test_only_modules(&sources);

    let mut codes: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for path in &sources {
        if skipped.contains(path) {
            continue;
        }
        let text = without_inline_tests(&read(path));
        let hazards = parity_hazards(&text);
        assert!(
            hazards.is_empty(),
            "{} holds {} — the split-on-quote scan would silently miss any \
             code literal after it; rephrase the construct, or teach this \
             guard to lex",
            path.display(),
            hazards.join(" and ")
        );
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        for code in code_literals(&text) {
            codes.entry(code).or_default().insert(name.clone());
        }
    }

    codes
}

/// The catalogue covers exactly the codes the library emits — no gaps, and
/// no rows for clauses no finding cites.
///
/// The scan is the whole `src/conformance/` tree and not the two files that
/// hold most of the vocabulary: the content sweep emits
/// `/req/core/attribute-model` and `/req/core/tiling-tilingscheme-consistent`
/// through `CdbViolation::DeclarationMismatch { clause, .. }`, and neither
/// appears in `finding.rs` or `class.rs`. A narrower scan would let a real
/// code ship with no row.
#[test]
fn cli_catalogue_guard_covers_every_code_the_library_emits() {
    let emitted = emitted_codes();
    let catalogued: BTreeSet<&str> = CATALOGUE.iter().map(|entry| entry.code).collect();

    let missing: Vec<String> = emitted
        .iter()
        .filter(|(code, _)| !catalogued.contains(code.as_str()))
        .map(|(code, files)| format!("{code} (emitted from {files:?})"))
        .collect();
    assert!(
        missing.is_empty(),
        "{} code(s) the library emits have no catalogue row — add them to \
         cdb-lint/src/catalogue.rs: {missing:#?}",
        missing.len()
    );

    let stale: Vec<&str> = catalogued
        .iter()
        .filter(|code| !emitted.contains_key(**code))
        .copied()
        .collect();
    assert!(
        stale.is_empty(),
        "{} catalogue row(s) name a code no library finding cites; delete them \
         or fix the spelling: {stale:#?}",
        stale.len()
    );
}

/// Every `class` is a real Annex A class token, or the one documented
/// marker for a code whose filing class is not fixed.
///
/// `RequirementsClass::ALL` is the vocabulary rather than a hand-written
/// list, so a twelfth class needs no edit here.
#[test]
fn cli_catalogue_guard_classes_are_requirements_class_tokens() {
    let tokens: BTreeSet<&str> = RequirementsClass::ALL
        .iter()
        .map(|class| class.as_str())
        .collect();

    for entry in CATALOGUE {
        assert!(
            tokens.contains(entry.class) || entry.class == catalogue::ANY_CLASS,
            "{}: class {:?} is neither a requirements-class token {tokens:?} nor \
             the cross-class marker {:?}",
            entry.code,
            entry.class,
            catalogue::ANY_CLASS
        );
    }
}

/// The cross-class marker stays exceptional.
///
/// `/conf/minimal-core` is filed under whichever mandatory class the profile
/// omitted, so no single token is honest for it. That is the *only* code in
/// the vocabulary whose class varies; pinning the count here means a second
/// one has to be looked at rather than absorbed.
#[test]
fn cli_catalogue_guard_only_annex_a_has_no_fixed_class() {
    let cross_class: Vec<&str> = CATALOGUE
        .iter()
        .filter(|entry| entry.class == catalogue::ANY_CLASS)
        .map(|entry| entry.code)
        .collect();

    assert_eq!(cross_class, ["/conf/minimal-core"]);
}

/// Every `section` is a clause number: dot-separated segments that are
/// either all digits (`7.10.2.4.1`) or a single annex letter (`A.2`).
///
/// The annex letter is not decoration. OGC 23-034 numbers its one normative
/// annex `A.1`, `A.2`, and Annex A.2 is where `/conf/minimal-core` is
/// defined, so a digits-only rule would leave that row unable to cite its
/// own clause.
#[test]
fn cli_catalogue_guard_sections_are_clause_numbers() {
    for entry in CATALOGUE {
        assert!(!entry.section.is_empty(), "{}: empty section", entry.code);
        for segment in entry.section.split('.') {
            let digits = !segment.is_empty() && segment.bytes().all(|byte| byte.is_ascii_digit());
            let annex = segment.len() == 1
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit());
            assert!(
                digits || annex,
                "{}: section {:?} is not a clause number",
                entry.code,
                entry.section
            );
        }
    }
}

/// No duplicate codes, and the list is in code order.
///
/// Sorted is what lets `explain --list` print the catalogue as it stands and
/// what keeps the source file reviewable as a table; strict ordering carries
/// uniqueness with it, and the two are asserted together so a failure names
/// the pair that broke.
#[test]
fn cli_catalogue_guard_is_sorted_and_unique() {
    for pair in CATALOGUE.windows(2) {
        let [left, right] = pair else { continue };
        assert!(
            left.code < right.code,
            "catalogue is out of order or repeats a code: {:?} then {:?}",
            left.code,
            right.code
        );
    }
}

/// Glosses are one line, non-empty, and carry no trailing period — one
/// aligned line per row is what `explain --list` prints, and a stray
/// newline would break the column it prints in.
#[test]
fn cli_catalogue_guard_glosses_are_one_bare_line() {
    for entry in CATALOGUE {
        assert!(!entry.gloss.is_empty(), "{}: empty gloss", entry.code);
        assert!(
            !entry.gloss.contains('\n'),
            "{}: gloss spans lines",
            entry.code
        );
        assert_eq!(
            entry.gloss.trim(),
            entry.gloss,
            "{}: gloss is padded",
            entry.code
        );
        assert!(
            !entry.gloss.ends_with('.'),
            "{}: gloss ends with a period: {:?}",
            entry.code,
            entry.gloss
        );
    }
}

/// The two lookups agree with the table they read.
#[test]
fn cli_catalogue_guard_lookup_is_exact_and_containing_is_not() {
    let attr2b = catalogue::lookup("/req/core/attribute-model-content-B")
        .expect("Attr2-B is in the vocabulary");
    assert_eq!(attr2b.code, "/req/core/attribute-model-content-B");

    // Exact lookup is case-sensitive: a code is an identifier.
    assert_eq!(
        catalogue::lookup("/REQ/CORE/ATTRIBUTE-MODEL-CONTENT-B"),
        None
    );
    assert_eq!(catalogue::lookup("attribute-model-content-B"), None);

    // The suggestion search folds case and matches anywhere, because its job
    // is to rescue a half-remembered code.
    let suggestions = catalogue::containing("-CONTENT-b");
    assert_eq!(
        suggestions
            .iter()
            .map(|entry| entry.code)
            .collect::<Vec<_>>(),
        ["/req/core/attribute-model-content-B"]
    );
    assert!(catalogue::containing("no-such-clause").is_empty());
}

/// The scanner reads the same source the library compiles, so its own rules
/// are pinned rather than left to the tree's current shape.
#[test]
fn cli_catalogue_guard_scanner_skips_tests_and_prefix_fragments() {
    let source = "\
/// Doc comment naming the \"/req/core/\" family.
const REAL: &str = \"/req/core/name-spaces\";
const SPACED: &str = \"/req/core/name-language B\";

#[cfg(test)]
mod tests {
    const FIXTURE: &str = \"/req/core/invented-for-a-test\";
}
";

    let scanned = code_literals(&without_inline_tests(source));

    assert_eq!(scanned, ["/req/core/name-spaces"]);
    assert_eq!(declared_module("    #[cfg(test)]"), None);
    assert_eq!(declared_module("mod support;"), Some("support"));
    assert_eq!(declared_module("pub mod support;"), Some("support"));
    assert_eq!(declared_module("mod tests {"), None);
}

/// The scanner refuses the two constructs that would desynchronize its
/// split-on-quote pass, so a phase flip fails the build instead of silently
/// hiding every code literal after it.
///
/// A concatenated code escaping the scan is a *documented* limit (design
/// §13); an off-phase scan was not, and unlike concatenation it hides codes
/// that are sitting right there as literals.
#[test]
fn cli_catalogue_guard_scanner_refuses_parity_hazards() {
    let clean = "const OK: &str = \"/req/core/name-spaces\";";
    assert_eq!(parity_hazards(clean), Vec::<String>::new());

    // A quote char-literal flips the phase for the rest of the file.
    let char_literal = "if character == '\"' { }";
    assert!(
        parity_hazards(char_literal)
            .iter()
            .any(|hazard| hazard.contains("char-literal")),
        "{:?}",
        parity_hazards(char_literal)
    );

    // One embedded escaped quote leaves the file's quote count odd.
    let odd = "const MESSAGE: &str = \"say \\\" once\";";
    assert!(
        parity_hazards(odd)
            .iter()
            .any(|hazard| hazard.contains("odd number")),
        "{:?}",
        parity_hazards(odd)
    );

    // Paired escaped quotes keep the phase and are tolerated: the mis-split
    // fragments they produce fail the family filter instead.
    let paired = "const MESSAGE: &str = \"say \\\"hi\\\" now\";";
    assert_eq!(parity_hazards(paired), Vec::<String>::new());
    assert_eq!(
        code_literals(paired),
        Vec::<String>::new(),
        "the fragments are not codes"
    );
}

/// The catalogue is a `&[Entry]` of `&'static str`, so a row can be handed
/// straight to a renderer — this is the shape Task 5's SARIF rule table
/// consumes.
#[test]
fn cli_catalogue_guard_rows_are_static() {
    fn takes_static(entry: &'static Entry) -> &'static str {
        entry.code
    }

    let first = CATALOGUE.first().expect("the catalogue is not empty");

    assert_eq!(takes_static(first), first.code);
}
