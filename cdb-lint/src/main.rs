//! The `cdb-lint` binary: read the process's arguments and its ambient
//! state, hand both to [`cdb_lint::run`], and exit with the code it returns.
//!
//! There is deliberately nothing else here. Every decision the tool makes
//! lives in the library, where a test drives it through one function with
//! two writable sinks instead of spawning a process and reading its output
//! back.

use std::ffi::OsString;
use std::io::{self, IsTerminal, Write};

use cdb_lint::{Env, run};

fn main() {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let env = Env {
        no_color: std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty()),
        stdout_is_terminal: io::stdout().is_terminal(),
    };

    let mut out = io::stdout().lock();
    let mut err = io::stderr().lock();
    let code = run(&args, &mut out, &mut err, &env);

    // `process::exit` skips destructors, so the sinks are flushed here.
    let _ = out.flush();
    let _ = err.flush();
    std::process::exit(code);
}
