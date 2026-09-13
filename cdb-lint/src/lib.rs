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

pub mod cli;
pub mod exit;

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
        Ok(cli::Command::Check(_) | cli::Command::Explain(_)) => {
            // The check and explain paths, the report renderers, and the
            // colour decision `env` feeds are not built yet.
            let _ = env;
            write_to(err, "not yet implemented\n", exit::OPERATIONAL)
        }
        Err(error) => write_to(
            err,
            &format!("error: {error}\ntry `cdb-lint --help` for usage\n"),
            exit::USAGE,
        ),
    }
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

    /// Both runs that do real work are still stubs; they say so on stderr
    /// and exit 3, which is the code for "the tool did not inspect
    /// anything".
    #[test]
    fn cli_run_check_and_explain_are_not_implemented_yet() {
        for tokens in [
            &["--profile", "simulation", "--encoding", "json", "/cdb"][..],
            &["explain", "--list"][..],
        ] {
            let (code, out, err) = run_capturing(tokens);

            assert_eq!(code, exit::OPERATIONAL, "{tokens:?}");
            assert!(out.is_empty(), "{tokens:?} wrote {out}");
            assert!(err.contains("not yet implemented"), "{tokens:?}: {err}");
        }
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
