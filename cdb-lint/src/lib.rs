//! `cdb-lint` — a command-line conformance checker for OGC CDB 2.0 Core
//! datastores.
//!
//! The library `rusty_cdb` can already judge a datastore, but only through a
//! Rust API, which reaches Rust programs and nobody else. cdb-lint turns
//! that judgment into a command, so an implementer who writes CDB 2.0
//! datastores in another language has something to check the result with.
//!
//! # The testable seam
//!
//! `main` holds no logic. Everything the tool does runs through [`run`],
//! which takes its arguments as a slice, writes to two [`Write`] sinks
//! rather than to the process's streams, and reads the ambient state it
//! needs from an [`Env`] the caller supplies. A test therefore drives the
//! whole tool without spawning a process, and no test result depends on
//! whether the harness happened to be attached to a terminal.
//!
//! ```
//! use std::ffi::OsString;
//!
//! use cdb_lint::{Env, exit, run};
//!
//! let mut out = Vec::new();
//! let mut err = Vec::new();
//! let env = Env {
//!     no_color: true,
//!     stdout_is_terminal: false,
//! };
//!
//! let code = run(&[OsString::from("--version")], &mut out, &mut err, &env);
//!
//! assert_eq!(code, exit::OK);
//! assert!(String::from_utf8_lossy(&out).starts_with("cdb-lint "));
//! ```

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;

use rusty_cdb::conformance::ConformanceReport;
use rusty_cdb::metadata::MetadataEncoding;
use rusty_cdb::{CdbDatastore, hierarchy, metadata};

use crate::cli::{CheckArgs, ColorChoice, Format, UsageError, UsageErrorKind};
use crate::render::json;
use crate::render::text::{self, TextOptions};

pub mod catalogue;
pub mod cli;
pub mod exit;
pub mod profile;
pub mod render;

/// The version of the `rusty_cdb` library whose judgment this build reports.
///
/// It is hand-maintained, and `tests/version_guard.rs` holds it against the
/// library's own `Cargo.toml` so that it cannot quietly fall behind. It is a
/// constant rather than a build-script product because the two crates live
/// in one workspace and a build script would be machinery for a string.
pub const RUSTY_CDB_VERSION: &str = "1.0.0";

/// The ambient state that would otherwise make a run non-reproducible.
///
/// Both fields are decided by the process's surroundings rather than by its
/// arguments, and both feed `--color auto`. Passing them in rather than
/// reading them where they are used is what lets a test pin the tool's
/// output: `main` reads them from the process, and everything else is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Env {
    /// Whether `NO_COLOR` is set to a non-empty value, which forces
    /// `--color never` whatever the flag says.
    pub no_color: bool,
    /// Whether stdout is a terminal, which is what `--color auto` asks.
    pub stdout_is_terminal: bool,
}

/// Run cdb-lint and return the process exit code.
///
/// `args` excludes `argv[0]`. `out` carries the artifact — the report, the
/// help text, the version line — and `err` carries the conversation:
/// diagnostics, usage errors, and the notes that honesty forbids putting
/// into a machine-readable document. The split holds whatever `--format`
/// says, so `cdb-lint --format json … > report.json` yields a report and
/// nothing else.
///
/// Every failure is a code, never a panic: a broken sink returns
/// [`exit::OPERATIONAL`] rather than unwinding out of `main`.
pub fn run(args: &[OsString], out: &mut dyn Write, err: &mut dyn Write, env: &Env) -> i32 {
    match cli::parse(args) {
        Ok(cli::Command::Help) => write_to(out, cli::HELP, exit::OK),
        Ok(cli::Command::Version) => write_to(
            out,
            &format!(
                "cdb-lint {} (rusty_cdb {RUSTY_CDB_VERSION})\n",
                env!("CARGO_PKG_VERSION")
            ),
            exit::OK,
        ),
        Ok(cli::Command::Check(args)) => check(&args, out, err, env),
        Ok(cli::Command::Explain(args)) => explain(&args, out, err),
        Err(error) => usage(err, &error),
    }
}

/// Describe one finding code, or list the whole vocabulary.
///
/// A report names clauses and says nothing about them, because a code is a
/// stable identifier and prose is not (`docs/CONFORMANCE.md` §5). This is
/// the other half of that bargain, and it keeps the routing rule the rest of
/// the tool keeps: a description is the artifact and goes to stdout, a
/// failed lookup is a conversation and goes to stderr.
///
/// The catalogue is documentation with a completeness check, not a derived
/// artifact — `tests/catalogue_guard.rs` holds its code column against the
/// library and nothing holds the other three — so the output describes a
/// clause and never claims to *be* one.
fn explain(args: &cli::ExplainArgs, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match (args.list, args.query.as_deref()) {
        (true, _) => write_to(out, &listing(), exit::OK),
        (false, Some(query)) => match catalogue::lookup(query) {
            Some(entry) => write_to(out, &describe(entry), exit::OK),
            None => write_to(err, &no_such_code(query), exit::USAGE),
        },
        // `cli::parse` admits exactly two forms of `explain` and this is
        // neither; saying so beats printing nothing and exiting 0.
        (false, None) => write_to(
            err,
            "error: `explain` needs a finding code, or `--list` to print every code\n",
            exit::USAGE,
        ),
    }
}

/// One catalogue row, rendered for a reader.
fn describe(entry: &catalogue::Entry) -> String {
    format!(
        "{}\n  class    {}\n  spec     OGC 23-034 §{}\n  {}\n",
        entry.code,
        class_line(entry.class),
        entry.section,
        entry.gloss
    )
}

/// The class as a sentence rather than as a token.
///
/// [`catalogue::ANY_CLASS`] is not a class and must not be printed as one:
/// `/conf/minimal-core` is filed under whichever mandatory class the profile
/// omitted, and a reader who is told "crs" when their report said
/// "file-naming" has been given a wrong answer confidently.
fn class_line(class: &str) -> &str {
    if class == catalogue::ANY_CLASS {
        return "any mandatory class — the report files it under the one the profile omitted";
    }
    class
}

/// The whole catalogue as an aligned table, one row per line.
///
/// The column widths are measured off the catalogue rather than fixed, so a
/// longer code widens the table instead of ragging it. [`catalogue::CATALOGUE`]
/// is already in code order, so nothing is sorted here.
fn listing() -> String {
    let width = |field: fn(&catalogue::Entry) -> &str| {
        catalogue::CATALOGUE
            .iter()
            .map(|entry| field(entry).len())
            .max()
            .unwrap_or_default()
    };
    let codes = width(|entry| entry.code);
    let classes = width(|entry| entry.class);
    let sections = width(|entry| entry.section);

    let mut rendered = String::new();
    for entry in catalogue::CATALOGUE {
        rendered.push_str(&format!(
            "{:codes$}  {:classes$}  {:sections$}  {}\n",
            entry.code, entry.class, entry.section, entry.gloss
        ));
    }

    rendered
}

/// The diagnostic for a query that is not a code.
///
/// It never stonewalls. A user who typed `-content-b`, or pasted a code out
/// of a lowercased log, gets the codes that contain what they typed —
/// case-folded, because the suggestion's job is to help, where the exact
/// match's job is to be an identifier. A query nothing contains is told so,
/// and pointed at the listing rather than left to guess the vocabulary.
fn no_such_code(query: &str) -> String {
    /// Enough suggestions to rescue a typo, few enough to read. Past this
    /// the listing is the better answer, and the message says so.
    const MAX_SUGGESTIONS: usize = 12;

    let matches = catalogue::containing(query);
    if matches.is_empty() {
        return format!(
            "error: `{query}` is not a finding code, and no code contains it\n\
             try `cdb-lint explain --list` for every code\n"
        );
    }

    let mut message = format!("error: `{query}` is not a finding code; did you mean:\n");
    for entry in matches.iter().take(MAX_SUGGESTIONS) {
        message.push_str(&format!("  {}\n", entry.code));
    }
    if matches.len() > MAX_SUGGESTIONS {
        message.push_str(&format!(
            "  … and {} more; try `cdb-lint explain --list` for every code\n",
            matches.len() - MAX_SUGGESTIONS
        ));
    }

    message
}

/// Check one datastore: resolve the yardstick, open the datastore, judge it,
/// render the verdict, and answer with the code CI reads.
///
/// The three failure kinds stay apart, because they are three different
/// facts and a build has to be able to tell them apart (design §4.1). A
/// yardstick this build cannot construct is a **usage** error — nothing was
/// judged, and the request was the problem. A datastore that cannot be opened
/// or walked is **operational** — the tool could not look, which is a fact
/// about the run and never about conformance, so a missing directory exits 3
/// and not 1. Only a report that was actually produced can reach
/// [`exit::FINDINGS`].
fn check(args: &CheckArgs, out: &mut dyn Write, err: &mut dyn Write, env: &Env) -> i32 {
    let profile = match profile::resolve(&args.profile) {
        Ok(profile) => profile,
        Err(error) => return usage(err, &error),
    };
    // The baseline ratchet arrives in a later task. Saying so beats exiting
    // 0 over a datastore whose findings were never compared to anything.
    if args.baseline.is_some() {
        return write_to(err, "not yet implemented\n", exit::OPERATIONAL);
    }

    let datastore = match CdbDatastore::open(&args.root) {
        Ok(datastore) => datastore,
        Err(error) => return operational(err, &args.root, &error),
    };
    let report = match datastore.validate(&*profile) {
        Ok(report) => report,
        Err(error) => return operational(err, &args.root, &error),
    };

    // Render into a buffer first, so where the artifact *goes* is one
    // decision taken once rather than a sink threaded through every
    // renderer — and so a half-written file is not the way a rendering
    // failure announces itself.
    let mut artifact = Vec::new();
    match args.format {
        Format::Text => {
            let options = TextOptions {
                encoding: profile.metadata_encoding(),
                color: use_color(args.color, env),
                quiet: args.quiet,
                deny_warnings: args.deny_warnings,
            };
            if text::render(&report, &options, &mut artifact).is_err() {
                return exit::OPERATIONAL;
            }
        }
        Format::Json => {
            if json::render(&report, &mut artifact).is_err() {
                return exit::OPERATIONAL;
            }
        }
        // SARIF arrives with its own task, and needs the code catalogue that
        // precedes it.
        Format::Sarif => return write_to(err, "not yet implemented\n", exit::OPERATIONAL),
    }

    if let Err(code) = emit(&artifact, args.output.as_deref(), out, err) {
        return code;
    }

    // Everything below is the conversation, and all of it goes to `err`.
    // The artifact is finished and, under `-o`, already on disk.
    if args.format == Format::Json && json::write_tally(&report, err).is_err() {
        // Honesty rule 2: the JSON artifact cannot carry the aggregate
        // without ceasing to be the frozen wire shape, so the tally is owed
        // to stderr and a run that could not say it did not fully report.
        return exit::OPERATIONAL;
    }
    // A diagnostic, never a yardstick: the note goes to stderr and the report
    // is unchanged (design §5 rule 5).
    if hint_at_encoding_mismatch(&report, profile.metadata_encoding(), err).is_err() {
        return exit::OPERATIONAL;
    }

    verdict_code(&report, args.deny_warnings)
}

/// The exit code a completed run answers with.
///
/// Violations fail the build because conformance failed. Warnings fail it
/// only when asked to: a warning is a SHOULD, and a SHOULD does not decide
/// conformance — `--deny-warnings` moves the code without moving the verdict,
/// and the report says so on its last line.
fn verdict_code(report: &ConformanceReport, deny_warnings: bool) -> i32 {
    if !report.is_conformant() {
        return exit::FINDINGS;
    }
    // Over the classes the *report* lists rather than over a fixed roster:
    // `RequirementsClass` is `#[non_exhaustive]` and a report lists every
    // class it has something to say about, so asking the report is the only
    // way this cannot fall behind a twelfth class.
    let warned = report
        .classes()
        .any(|(_, findings)| !findings.warnings.is_empty());
    if deny_warnings && warned {
        return exit::FINDINGS;
    }
    exit::OK
}

/// Whether the text report is coloured.
///
/// `NO_COLOR` wins over the flag, unconditionally: design §4 states that any
/// non-empty value forces `never`, and a user who exported it did so to stop
/// arguing with individual tools about it.
fn use_color(choice: ColorChoice, env: &Env) -> bool {
    if env.no_color {
        return false;
    }
    match choice {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => env.stdout_is_terminal,
    }
}

/// Notes on stderr that the run's declared encoding and the datastore's
/// disagree, and names the other spelling.
///
/// **This changes nothing.** The report is already written, the datastore
/// still fails Requirement Metadata5, and the exit code still says so.
/// Reading the encoding off the datastore instead of the command line would
/// make Metadata5 unfailable — the crate's own false-green failure mode,
/// reintroduced one layer up (design §5 rule 5) — so detection may suggest a
/// flag and may never choose one.
///
/// The trigger is the finding's stable `code`, never its message text. That
/// is the contract every finding's `code()` exists to serve, and prose is
/// free to change within `1.x` while a code is not.
fn hint_at_encoding_mismatch(
    report: &ConformanceReport,
    declared: MetadataEncoding,
    err: &mut dyn Write,
) -> std::io::Result<()> {
    // Every class, keyed on the code alone. The clause is filed under
    // Metadata today, but the hint's trigger is the code and nothing else —
    // not the class it happens to sit under, and never the message text.
    let mismatched = report.classes().any(|(_, findings)| {
        findings
            .violations
            .iter()
            .any(|violation| violation.code() == ENCODING_MISMATCH)
    });
    if !mismatched {
        return Ok(());
    }
    let other = match declared {
        MetadataEncoding::Json => "xml",
        MetadataEncoding::Xml => "json",
        // Unreachable through either door: no built-in profile can carry
        // `gpkg`, and the descriptor pre-flight rejects it (design §9.2).
        // There is no other spelling to suggest, so nothing is said.
        MetadataEncoding::Gpkg => return Ok(()),
    };
    let note = format!(
        "note: this run declared `--encoding {}`, and the datastore's global metadata \
         record declares a different one ({ENCODING_MISMATCH}). If the datastore is \
         right, re-run with `--encoding {other}`. The report above is unchanged: a \
         datastore is judged against the yardstick you stated, never against itself.\n",
        declared.as_str()
    );
    err.write_all(note.as_bytes())
}

/// Requirement Metadata5's declaration-mismatch clause, and the only thing
/// the encoding hint keys on.
const ENCODING_MISMATCH: &str = "/req/core/metadata-encoding";

/// Report a usage error, enriching the one kind that a look at the disk can
/// sharpen.
///
/// `cli::parse` is pure and cannot look; this can, and does so only to
/// *suggest*. Where the datastore gives no unambiguous answer — neither
/// record, or both — the base message stands, because a guess dressed as a
/// hint is worse than no hint.
fn usage(err: &mut dyn Write, error: &UsageError) -> i32 {
    let mut message = error.message.clone();
    if let UsageErrorKind::MissingEncoding { root } = &error.kind
        && let Some(found) = detect_encoding(root)
    {
        message.push_str(&format!(
            " — the datastore's global metadata is global_metadata.{found}; \
             you probably want --encoding {found}"
        ));
    }

    write_to(
        err,
        &format!("error: {message}\ntry `cdb-lint --help` for usage\n"),
        exit::USAGE,
    )
}

/// The encoding the datastore at `root` appears to use, when exactly one
/// global metadata record is present.
///
/// `None` for neither and `None` for both: with nothing to point at, or two
/// things to point at, there is no fact to report. Nothing downstream may
/// turn this into a yardstick — it feeds one sentence of one error message.
///
/// The directory and the stem come from the library's own constants, so the
/// probe cannot drift away from the record `validate` will go on to read.
fn detect_encoding(root: &Path) -> Option<&'static str> {
    let dir = root.join(hierarchy::GLOBAL_METADATA_DIR);
    let found: Vec<&'static str> = ["json", "xml"]
        .into_iter()
        .filter(|extension| {
            dir.join(format!("{}.{extension}", metadata::GLOBAL_METADATA_STEM))
                .is_file()
        })
        .collect();

    match found.as_slice() {
        [only] => Some(only),
        _ => None,
    }
}

/// Put the finished artifact where the caller asked for it.
///
/// stdout carries the artifact and stderr carries the conversation (design
/// §6); `-o` moves the artifact off stdout and leaves that split intact, so a
/// run with `-o` still says everything it would otherwise have said — just
/// not into the file.
///
/// # Errors
///
/// Returns the exit code the caller should answer with. A file that cannot be
/// created or written is an I/O failure that stopped the run, which is
/// [`exit::OPERATIONAL`]; [`exit::USAGE`] stays for what can be caught before
/// any work is done.
fn emit(
    artifact: &[u8],
    output: Option<&Path>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), i32> {
    let Some(path) = output else {
        return match out.write_all(artifact) {
            Ok(()) => Ok(()),
            Err(_) => Err(exit::OPERATIONAL),
        };
    };

    match std::fs::write(path, artifact) {
        Ok(()) => Ok(()),
        Err(error) => Err(write_to(
            err,
            &format!(
                "error: cannot write the report to {}: {error}\n\
                 this is a fact about the run, not a conformance verdict\n",
                path.display()
            ),
            exit::OPERATIONAL,
        )),
    }
}

/// Report an operational failure: the tool could not inspect the datastore.
fn operational(err: &mut dyn Write, root: &Path, error: &dyn std::error::Error) -> i32 {
    write_to(
        err,
        &format!(
            "error: cannot inspect the datastore at {}: {error}\n\
             this is a fact about the run, not a conformance verdict\n",
            root.display()
        ),
        exit::OPERATIONAL,
    )
}

/// Write `text` and answer with `code`, or with [`exit::OPERATIONAL`] if the
/// sink refused it. A report that could not be written is a fact about the
/// run, so it exits 3 and not 0.
fn write_to(sink: &mut dyn Write, text: &str, code: i32) -> i32 {
    match sink.write_all(text.as_bytes()) {
        Ok(()) => code,
        Err(_) => exit::OPERATIONAL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    /// An argument vector shaped the way `main` builds one.
    fn args(tokens: &[&str]) -> Vec<OsString> {
        tokens.iter().map(OsString::from).collect()
    }

    /// A terminal-free, colour-free environment: the deterministic one.
    fn env() -> Env {
        Env {
            no_color: false,
            stdout_is_terminal: false,
        }
    }

    /// Run and return the exit code with both streams as text.
    fn run_capturing(tokens: &[&str]) -> (i32, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(&args(tokens), &mut out, &mut err, &env());

        (
            code,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        )
    }

    /// Help is an artifact, not a diagnostic: it goes to stdout, so
    /// `cdb-lint --help | less` works.
    #[test]
    fn cli_run_prints_help_to_stdout() {
        let (code, out, err) = run_capturing(&["--help"]);

        assert_eq!(code, exit::OK);
        assert_eq!(out, cli::HELP);
        assert!(err.is_empty(), "help is not a diagnostic: {err}");
    }

    /// The version line names both crates, because the library's version is
    /// what decides a verdict and the CLI's is what decides the flags.
    #[test]
    fn cli_run_version_names_the_cli_and_the_library() {
        let (code, out, err) = run_capturing(&["--version"]);

        assert_eq!(code, exit::OK);
        assert_eq!(
            out,
            format!(
                "cdb-lint {} (rusty_cdb {RUSTY_CDB_VERSION})\n",
                env!("CARGO_PKG_VERSION")
            )
        );
        assert!(err.is_empty());
    }

    /// A usage error is a conversation, so it goes to stderr — leaving
    /// stdout empty rather than half an artifact — and it points at the help
    /// text rather than reprinting it.
    #[test]
    fn cli_run_reports_a_usage_error_on_stderr() {
        let (code, out, err) = run_capturing(&["--nope", "/cdb"]);

        assert_eq!(code, exit::USAGE);
        assert!(out.is_empty(), "stdout should stay clean: {out}");
        assert!(err.contains("`--nope`"), "{err}");
        assert!(err.contains("--help"), "the pointer is missing: {err}");
    }

    /// Both forms of `explain` produce an artifact: it is what the user
    /// asked for, so it goes to stdout and exits 0. `tests/cli_explain.rs`
    /// drives the rendering; this pins the routing.
    #[test]
    fn cli_run_explain_writes_its_answer_to_stdout() {
        for tokens in [
            &["explain", "--list"][..],
            &["explain", "/req/core/name-spaces"][..],
        ] {
            let (code, out, err) = run_capturing(tokens);

            assert_eq!(code, exit::OK, "{tokens:?}: {err}");
            assert!(out.contains("/req/core/name-spaces"), "{tokens:?}: {out}");
            assert!(err.is_empty(), "{tokens:?} said {err}");
        }
    }

    /// A code the catalogue does not hold is a usage error, not an
    /// operational one: nothing was wrong with the tool or the machine, the
    /// request named something that does not exist.
    #[test]
    fn cli_run_explain_reports_an_unknown_code_as_usage() {
        let (code, out, err) = run_capturing(&["explain", "/req/core/nope"]);

        assert_eq!(code, exit::USAGE);
        assert!(out.is_empty(), "stdout should stay clean: {out}");
        assert!(err.contains("/req/core/nope"), "{err}");
    }

    /// The listing is bounded even when every code matches: `explain ""`
    /// asks for a substring every code contains, and the answer is a pointer
    /// at `--list` rather than the whole vocabulary on stderr.
    #[test]
    fn cli_run_explain_caps_its_suggestions() {
        let message = no_such_code("");

        assert!(
            message.lines().count() < catalogue::CATALOGUE.len(),
            "{message}"
        );
        assert!(message.contains("explain --list"), "{message}");
    }

    /// The cross-class marker never reaches a reader as if it were a class.
    #[test]
    fn cli_run_explain_spells_out_a_varying_class() {
        assert_eq!(class_line("attribution"), "attribution");
        assert!(class_line(catalogue::ANY_CLASS).contains("mandatory class"));
    }

    /// A check whose datastore is not there exits 3, and the diagnostic says
    /// which fact that is. Design §4.1 keeps "the tool could not look" apart
    /// from "the datastore does not conform" precisely so a broken mount is
    /// not reported as a failed audit; `tests/cli_check.rs` drives the rest
    /// of the check path over real datastores.
    #[test]
    fn cli_run_check_reports_an_unreachable_datastore_as_operational() {
        let (code, out, err) =
            run_capturing(&["--profile", "simulation", "--encoding", "json", "/cdb"]);

        assert_eq!(code, exit::OPERATIONAL);
        assert!(out.is_empty(), "no half a report: {out}");
        assert!(err.contains("/cdb"), "{err}");
        assert!(err.contains("not a conformance verdict"), "{err}");
    }

    /// The constant is what `tests/version_guard.rs` holds against the
    /// library's manifest; it is a version triple and nothing else.
    #[test]
    fn cli_run_rusty_cdb_version_is_a_version_triple() {
        let parts: Vec<&str> = RUSTY_CDB_VERSION.split('.').collect();

        assert_eq!(parts.len(), 3, "{RUSTY_CDB_VERSION}");
        for part in parts {
            assert!(
                !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()),
                "{RUSTY_CDB_VERSION}"
            );
        }
    }
}
