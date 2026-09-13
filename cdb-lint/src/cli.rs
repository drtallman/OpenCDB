//! The argument grammar: the tokens `main` is handed, the [`Command`] they
//! mean, and the [`UsageError`] they are instead when they mean nothing.
//!
//! Arguments stay [`OsString`] as far as they can, because a datastore under
//! a non-UTF-8 path is still a datastore and still has to be lintable. Only
//! values that must be words — `--profile`, `--encoding`, `--format`,
//! `--color`, and the code given to `explain` — are converted to text, and
//! the conversion failing is a usage error rather than a lossy guess. Paths
//! keep their bytes.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

/// One of the two application profiles compiled into cdb-lint.
///
/// The standard makes the application profile the implementable unit (§5.1),
/// so the profile is never defaulted: a run states its yardstick or it does
/// not run. Anything these two do not describe is expressed as a descriptor
/// file instead — see [`ProfileChoice::File`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinProfile {
    /// The library's `profiles::simulation`: file-system storage, WGS-84,
    /// WKT-2 CRS metadata, CDB1GlobalGrid tiling.
    Simulation,
    /// The library's `profiles::gnosis`: as above, but pinning the
    /// GNOSISGlobalGrid tiling scheme.
    Gnosis,
}

/// The metadata encoding a built-in profile is pinned to.
///
/// Only the two encodings this build implements are spellable. The
/// standard's vocabulary also admits `gpkg`, but `GlobalMetadata::write_to`
/// answers it with `UnsupportedEncoding`; offering the spelling here would
/// promise an encoding the tool cannot read (design §9.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// JSON metadata documents.
    Json,
    /// XML metadata documents.
    Xml,
}

/// How the report is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// The human report: one row per requirements class, then the tally and
    /// the verdict. The default.
    Text,
    /// The library's own serde wire shape, pretty-printed and unadorned —
    /// also the file `--baseline` reads.
    Json,
    /// SARIF 2.1.0, one run, for a code-scanning pipeline.
    Sarif,
}

/// Whether the text report is coloured.
///
/// The `json` and `sarif` formats are never coloured, whatever this says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorChoice {
    /// Colour when stdout is a terminal and `NO_COLOR` is unset. The default.
    Auto,
    /// Colour unconditionally — for a pipeline that renders ANSI itself.
    Always,
    /// Never colour.
    Never,
}

/// The yardstick a check is measured against.
///
/// The two arms are mutually exclusive by construction, which is the point:
/// a run that named both a built-in profile and a descriptor would have two
/// yardsticks and no way to say which one produced the verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileChoice {
    /// A built-in profile and the metadata encoding it is pinned to. Both
    /// halves are required: neither is guessed from the datastore, because
    /// deriving the encoding from the thing being judged would make
    /// Requirement Metadata5 unfailable (design §5 rule 5).
    Builtin {
        /// Which built-in profile.
        profile: BuiltinProfile,
        /// The metadata encoding it declares.
        encoding: Encoding,
    },
    /// A profile descriptor document, whose fields declare the restrictions
    /// an application profile owes the core.
    File(PathBuf),
}

/// A `check` run: one datastore, one yardstick, one report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckArgs {
    /// The datastore's root directory.
    pub root: PathBuf,
    /// The application profile the datastore is judged against.
    pub profile: ProfileChoice,
    /// The report format.
    pub format: Format,
    /// Where the report goes; `None` is stdout. Diagnostics go to stderr
    /// either way — stdout carries the artifact, stderr the conversation.
    pub output: Option<PathBuf>,
    /// A previous `--format json` report to ratchet against. Findings it
    /// already records stop failing the build; new ones still do.
    pub baseline: Option<PathBuf>,
    /// Fail the build on warnings as well as violations. This changes the
    /// exit code, never the verdict: a warning is a SHOULD, and a SHOULD
    /// does not decide conformance.
    pub deny_warnings: bool,
    /// Omit passing class rows from the text report. Failing classes, the
    /// findings, the coverage tally, and the verdict all survive it.
    pub quiet: bool,
    /// Whether to colour the text report.
    pub color: ColorChoice,
}

/// An `explain` run: describe one finding code, or list them all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainArgs {
    /// The code to describe. Mutually exclusive with [`Self::list`], and
    /// exactly one of the two is always present.
    pub query: Option<String>,
    /// Print every catalogue row instead of one.
    pub list: bool,
}

/// What a well-formed command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Check a datastore.
    Check(CheckArgs),
    /// Describe finding codes.
    Explain(ExplainArgs),
    /// Print [`HELP`].
    Help,
    /// Print the version line.
    Version,
}

/// Which usage error occurred, for the one case where the caller can say
/// more than [`parse`] can.
///
/// [`parse`] is pure: it reads the argument vector and never the filesystem,
/// which is what keeps honesty rule 5 (design §5) mechanical rather than
/// merely intended — a parser that cannot look at a datastore cannot let one
/// choose its own yardstick. Design §4.2 nevertheless wants the
/// missing-`--encoding` diagnostic to name the encoding the datastore appears
/// to use. The two are reconciled by *discriminating the error* rather than
/// by making the parser impure: the parser records that this particular
/// failure is enrichable and hands over the root, and the check path — which
/// is allowed to read the disk — appends the suggestion.
///
/// The consumer matches on this enum. It never matches on
/// [`UsageError::message`], for the same reason nothing in cdb-lint keys on a
/// finding's `Display` text.
///
/// Deliberately **not** `#[non_exhaustive]`, unlike the library's own
/// vocabularies: those track a draft standard read by other crates, whereas
/// this one is read only by [`crate::run`]. Leaving it closed means the
/// descriptor and baseline tasks cannot add a kind without the compiler
/// pointing at every place that has to decide what to do with it — which is
/// the behaviour a wildcard arm would quietly suppress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageErrorKind {
    /// `--profile` was given without `--encoding`, **and** a `<ROOT>` was
    /// supplied. The check path may enrich the message with the encoding the
    /// datastore at `root` appears to use. A suggestion is all it may ever
    /// become: the run still fails, and the user still states the yardstick.
    MissingEncoding {
        /// The datastore root the command line named.
        root: PathBuf,
    },
    /// Every other usage error. Nothing about the filesystem could sharpen
    /// the message, so the message stands as [`parse`] wrote it.
    Other,
}

/// A command line that does not name a run cdb-lint can make.
///
/// The message names the specific problem and, where a vocabulary was
/// violated, lists the valid spellings — an error that says only "invalid
/// arguments" makes the user re-read the help text to find what the tool
/// already knew.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageError {
    /// The whole diagnostic, without a trailing newline and without the
    /// `error:` prefix the caller adds.
    pub message: String,
    /// Which error this is, for a caller that can add to it. See
    /// [`UsageErrorKind`].
    pub kind: UsageErrorKind,
}

impl UsageError {
    /// A usage error carrying `message` that no caller can improve on.
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: UsageErrorKind::Other,
        }
    }

    /// The `--profile`-without-`--encoding` error, tagged with the root so
    /// the check path can suggest an encoding.
    fn missing_encoding(message: impl Into<String>, root: PathBuf) -> Self {
        Self {
            message: message.into(),
            kind: UsageErrorKind::MissingEncoding { root },
        }
    }
}

impl fmt::Display for UsageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for UsageError {}

/// The full usage text, printed verbatim by `--help`. It ends with a
/// newline, so it is written rather than line-written.
pub const HELP: &str = "\
cdb-lint — check a datastore against the OGC CDB 2.0 Core Standard (OGC 23-034).

USAGE
    cdb-lint [check] [OPTIONS] <ROOT>
    cdb-lint explain <CODE>
    cdb-lint explain --list
    cdb-lint --help | --version

    <ROOT> is the datastore's root directory. The first argument is read as a
    subcommand only when it is exactly `check` or `explain`, so a datastore
    directory of either name is reached as `./check` or `./explain`.

THE YARDSTICK (state one; cdb-lint never guesses it from the datastore)
    --profile <simulation|gnosis>  a built-in application profile
    --encoding <json|xml>          its metadata encoding; required alongside
                                   --profile
    --profile-file <path.json>     a profile descriptor; excludes both of the
                                   flags above

OPTIONS
    --format <text|json|sarif>     report format [default: text]
    -o, --output <path>            write the report here [default: stdout]
    --baseline <path.json>         a previous `--format json` report; findings
                                   it already records stop failing the build
    --deny-warnings                exit 1 when warnings are present
    -q, --quiet                    omit passing classes from the text report
    --color <auto|always|never>    colour the text report [default: auto]
    -h, --help                     print this help
    -V, --version                  print version information

GRAMMAR
    `--flag value` and `--flag=value` both parse. `--` ends flag parsing.
    Short flags never cluster: write `-q -o out.json`, not `-qo out.json`.
    A repeated flag is an error rather than last-wins, so a script that
    appends a second `--profile` is told about it instead of quietly having
    one of its two yardsticks chosen for it.

EXIT CODES
    0  conformant; with --baseline, no new findings
    1  findings: violations, or warnings under --deny-warnings
    2  usage or configuration error — nothing was judged
    3  operational failure: the datastore could not be inspected
";

/// The flags accumulated by one scan, before they are read as a command.
///
/// Every field records the spelling it was set by, so that a repeat can name
/// both spellings — `-o a --output b` is one flag given twice, and the
/// message has to say which two tokens it means.
#[derive(Debug, Default)]
struct Raw {
    profile: Option<(&'static str, OsString)>,
    encoding: Option<(&'static str, OsString)>,
    profile_file: Option<(&'static str, OsString)>,
    format: Option<(&'static str, OsString)>,
    output: Option<(&'static str, OsString)>,
    baseline: Option<(&'static str, OsString)>,
    color: Option<(&'static str, OsString)>,
    deny_warnings: Option<(&'static str, ())>,
    quiet: Option<(&'static str, ())>,
    list: Option<(&'static str, ())>,
    positionals: Vec<OsString>,
}

/// Record a flag's value, or fail because it already had one.
fn set_once<T>(
    slot: &mut Option<(&'static str, T)>,
    spelling: &'static str,
    value: T,
) -> Result<(), UsageError> {
    match slot {
        Some((first, _)) => Err(repeated(first, spelling)),
        None => {
            *slot = Some((spelling, value));
            Ok(())
        }
    }
}

/// The repeat diagnostic, which names both spellings when they differ.
fn repeated(first: &str, again: &str) -> UsageError {
    let subject = if first == again {
        format!("`{first}` is given more than once")
    } else {
        format!("`{first}` and `{again}` are the same flag, given more than once")
    };
    UsageError::new(format!(
        "{subject}; cdb-lint rejects a repeated flag rather than silently choosing one of its values"
    ))
}

/// Refuse an attached value on a flag that does not take one. `--quiet=no`
/// reads like a way to switch the flag off, and dropping the value would
/// switch it on instead.
fn no_value(spelling: &str, attached: Option<&str>) -> Result<(), UsageError> {
    match attached {
        Some(_) => Err(UsageError::new(format!("`{spelling}` takes no value"))),
        None => Ok(()),
    }
}

/// Refuse a flag that belongs to the other subcommand.
fn reject_foreign<T>(
    slot: &Option<(&'static str, T)>,
    subcommand: &str,
    home: &str,
) -> Result<(), UsageError> {
    match slot {
        Some((spelling, _)) => Err(UsageError::new(format!(
            "`{spelling}` is not valid for `{subcommand}`; it belongs to `{home}`"
        ))),
        None => Ok(()),
    }
}

/// The diagnostic for a short-flag token that is not one of the four.
fn not_a_short_flag(token: &str) -> UsageError {
    UsageError::new(format!(
        "`{token}` is not a short flag; the short flags are `-h`, `-V`, `-o`, and `-q`, \
         they never cluster, and they take their value as the next argument — \
         write `-q -o out.json`"
    ))
}

/// Whether an argument's first byte is `-`, asked of an argument that need
/// not be UTF-8.
fn starts_with_dash(arg: &OsString) -> bool {
    arg.as_encoded_bytes().first() == Some(&b'-')
}

/// Split `--name=value` into its halves; a bare `--name` has no value.
fn split_long(rest: &str) -> (&str, Option<&str>) {
    match rest.split_once('=') {
        Some((name, value)) => (name, Some(value)),
        None => (rest, None),
    }
}

/// The value of a flag: attached with `=`, or the next argument.
fn take_value(
    spelling: &'static str,
    attached: Option<&str>,
    args: &[OsString],
    index: &mut usize,
) -> Result<OsString, UsageError> {
    if let Some(value) = attached {
        return Ok(OsString::from(value));
    }
    match args.get(*index) {
        Some(value) => {
            *index += 1;
            Ok(value.clone())
        }
        None => Err(UsageError::new(format!("`{spelling}` needs a value"))),
    }
}

/// Read an argument list as the command it names.
///
/// `args` excludes `argv[0]`; `main` passes `std::env::args_os().skip(1)`.
///
/// `--help` and `--version` short-circuit: the first of them seen ends the
/// scan and wins over anything after it, so `cdb-lint check --help` is help
/// rather than a complaint about a missing root.
pub fn parse(args: &[OsString]) -> Result<Command, UsageError> {
    let mut raw = Raw::default();
    let mut index = 0;
    let mut end_of_flags = false;
    // A `--` reached before any positional makes the first positional a
    // literal, so a datastore directory named `check` is also reachable as
    // `-- check` and not only as `./check`.
    let mut subcommand_allowed = true;

    while index < args.len() {
        let arg = &args[index];
        index += 1;

        if end_of_flags {
            raw.positionals.push(arg.clone());
            continue;
        }

        let Some(text) = arg.to_str() else {
            if starts_with_dash(arg) {
                return Err(non_utf8_flag(arg));
            }
            raw.positionals.push(arg.clone());
            continue;
        };

        if text == "--" {
            end_of_flags = true;
            subcommand_allowed = !raw.positionals.is_empty();
        } else if let Some(rest) = text.strip_prefix("--") {
            let (name, attached) = split_long(rest);
            match name {
                "help" => {
                    no_value("--help", attached)?;
                    return Ok(Command::Help);
                }
                "version" => {
                    no_value("--version", attached)?;
                    return Ok(Command::Version);
                }
                "profile" => {
                    let value = take_value("--profile", attached, args, &mut index)?;
                    set_once(&mut raw.profile, "--profile", value)?;
                }
                "encoding" => {
                    let value = take_value("--encoding", attached, args, &mut index)?;
                    set_once(&mut raw.encoding, "--encoding", value)?;
                }
                "profile-file" => {
                    let value = take_value("--profile-file", attached, args, &mut index)?;
                    set_once(&mut raw.profile_file, "--profile-file", value)?;
                }
                "format" => {
                    let value = take_value("--format", attached, args, &mut index)?;
                    set_once(&mut raw.format, "--format", value)?;
                }
                "output" => {
                    let value = take_value("--output", attached, args, &mut index)?;
                    set_once(&mut raw.output, "--output", value)?;
                }
                "baseline" => {
                    let value = take_value("--baseline", attached, args, &mut index)?;
                    set_once(&mut raw.baseline, "--baseline", value)?;
                }
                "color" => {
                    let value = take_value("--color", attached, args, &mut index)?;
                    set_once(&mut raw.color, "--color", value)?;
                }
                "deny-warnings" => {
                    no_value("--deny-warnings", attached)?;
                    set_once(&mut raw.deny_warnings, "--deny-warnings", ())?;
                }
                "quiet" => {
                    no_value("--quiet", attached)?;
                    set_once(&mut raw.quiet, "--quiet", ())?;
                }
                "list" => {
                    no_value("--list", attached)?;
                    set_once(&mut raw.list, "--list", ())?;
                }
                _ => return Err(UsageError::new(format!("unknown flag `--{name}`"))),
            }
        } else if text.starts_with('-') && text.len() > 1 {
            match text {
                "-h" => return Ok(Command::Help),
                "-V" => return Ok(Command::Version),
                "-o" => {
                    let value = take_value("-o", None, args, &mut index)?;
                    set_once(&mut raw.output, "-o", value)?;
                }
                "-q" => set_once(&mut raw.quiet, "-q", ())?,
                _ if text.chars().count() > 2 => return Err(not_a_short_flag(text)),
                _ => return Err(UsageError::new(format!("unknown flag `{text}`"))),
            }
        } else {
            raw.positionals.push(arg.clone());
        }
    }

    assemble(raw, subcommand_allowed)
}

/// Which subcommand the first positional named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Subcommand {
    Check,
    Explain,
}

/// The diagnostic for a flag-shaped argument that is not valid UTF-8.
fn non_utf8_flag(arg: &OsString) -> UsageError {
    UsageError::new(format!(
        "`{}` is not valid UTF-8, so it is not a flag",
        arg.to_string_lossy()
    ))
}

/// Read the accumulated flags as a command.
///
/// The first positional is a subcommand only when it is exactly `check` or
/// `explain`; anything else is the root of an implicit check, so the common
/// invocation needs no subcommand at all.
fn assemble(mut raw: Raw, subcommand_allowed: bool) -> Result<Command, UsageError> {
    let subcommand = match raw.positionals.first() {
        Some(first) if subcommand_allowed => match first.to_str() {
            Some("check") => Some(Subcommand::Check),
            Some("explain") => Some(Subcommand::Explain),
            _ => None,
        },
        _ => None,
    };

    if subcommand.is_some() {
        raw.positionals.remove(0);
    }

    match subcommand {
        Some(Subcommand::Explain) => assemble_explain(raw),
        Some(Subcommand::Check) | None => assemble_check(raw),
    }
}

/// Read the accumulated flags as a `check`.
fn assemble_check(raw: Raw) -> Result<Command, UsageError> {
    reject_foreign(&raw.list, "check", "explain")?;

    let mut positionals = raw.positionals.into_iter();
    let Some(root) = positionals.next() else {
        return Err(UsageError::new(
            "missing <ROOT>: the datastore directory to check",
        ));
    };
    if let Some(extra) = positionals.next() {
        return Err(UsageError::new(format!(
            "unexpected extra argument `{}`; cdb-lint checks one datastore per run",
            extra.to_string_lossy()
        )));
    }

    // The root is read before the yardstick so that a run naming neither is
    // told about the root first, and so that the one enrichable diagnostic
    // has a root to carry.
    let root = PathBuf::from(root);
    let profile = yardstick(raw.profile, raw.encoding, raw.profile_file, &root)?;

    Ok(Command::Check(CheckArgs {
        root,
        profile,
        format: match raw.format {
            Some((_, format)) => parse_format(&format)?,
            None => Format::Text,
        },
        output: raw.output.map(|(_, path)| PathBuf::from(path)),
        baseline: raw.baseline.map(|(_, path)| PathBuf::from(path)),
        deny_warnings: raw.deny_warnings.is_some(),
        quiet: raw.quiet.is_some(),
        color: match raw.color {
            Some((_, color)) => parse_color(&color)?,
            None => ColorChoice::Auto,
        },
    }))
}

/// Which yardstick the flags name, and why they name none.
///
/// Exactly one of the two forms has to be present. The diagnostics name the
/// flags actually given rather than restating the whole rule, because a user
/// who typed `--profile` alone knows what a profile is and needs to be told
/// only what is missing.
///
/// `root` is the datastore the command line named — already known to be
/// present, since [`assemble_check`] demands it first. It is carried on the
/// one enrichable diagnostic and used for nothing else here; this function
/// does not touch the filesystem.
fn yardstick(
    profile: Option<(&'static str, OsString)>,
    encoding: Option<(&'static str, OsString)>,
    profile_file: Option<(&'static str, OsString)>,
    root: &Path,
) -> Result<ProfileChoice, UsageError> {
    match (profile, encoding, profile_file) {
        (Some((_, profile)), Some((_, encoding)), None) => Ok(ProfileChoice::Builtin {
            profile: parse_profile(&profile)?,
            encoding: parse_encoding(&encoding)?,
        }),
        (None, None, Some((_, path))) => Ok(ProfileChoice::File(PathBuf::from(path))),
        (Some(_), None, None) => Err(UsageError::missing_encoding(
            "`--profile` is half a yardstick: add `--encoding`, which takes `json` or \
             `xml`. cdb-lint states the metadata encoding rather than reading it off \
             the datastore it is judging",
            root.to_path_buf(),
        )),
        (None, Some(_), None) => Err(UsageError::new(
            "`--encoding` is half a yardstick: add `--profile`, which takes \
             `simulation` or `gnosis`",
        )),
        (None, None, None) => Err(UsageError::new(
            "state the yardstick: either `--profile <simulation|gnosis>` with \
             `--encoding <json|xml>`, or `--profile-file <path.json>`",
        )),
        (profile, encoding, Some(_)) => {
            let mut given = Vec::new();
            if profile.is_some() {
                given.push("`--profile`");
            }
            if encoding.is_some() {
                given.push("`--encoding`");
            }
            Err(UsageError::new(format!(
                "`--profile-file` cannot be combined with {}; a run is measured against \
                 one yardstick, and two would leave the verdict unattributable",
                given.join(" and ")
            )))
        }
    }
}

/// Read the accumulated flags as an `explain`.
fn assemble_explain(raw: Raw) -> Result<Command, UsageError> {
    reject_foreign(&raw.profile, "explain", "check")?;
    reject_foreign(&raw.encoding, "explain", "check")?;
    reject_foreign(&raw.profile_file, "explain", "check")?;
    reject_foreign(&raw.format, "explain", "check")?;
    reject_foreign(&raw.output, "explain", "check")?;
    reject_foreign(&raw.baseline, "explain", "check")?;
    reject_foreign(&raw.color, "explain", "check")?;
    reject_foreign(&raw.deny_warnings, "explain", "check")?;
    reject_foreign(&raw.quiet, "explain", "check")?;

    let mut positionals = raw.positionals.into_iter();
    let query = match positionals.next() {
        Some(code) => Some(text_value("the code given to `explain`", &code)?.to_owned()),
        None => None,
    };
    if let Some(extra) = positionals.next() {
        return Err(UsageError::new(format!(
            "unexpected extra argument `{}`; `explain` describes one code per run",
            extra.to_string_lossy()
        )));
    }

    let list = raw.list.is_some();
    match (&query, list) {
        (Some(_), true) => Err(UsageError::new(
            "`explain` takes a code or `--list`, not both",
        )),
        (None, false) => Err(UsageError::new(
            "`explain` needs a finding code, or `--list` to print every code",
        )),
        _ => Ok(Command::Explain(ExplainArgs { query, list })),
    }
}

/// A flag value that has to be a word.
fn text_value<'value>(subject: &str, value: &'value OsString) -> Result<&'value str, UsageError> {
    value.to_str().ok_or_else(|| {
        UsageError::new(format!(
            "{subject} is not valid UTF-8: `{}`",
            value.to_string_lossy()
        ))
    })
}

/// `--profile`'s vocabulary.
fn parse_profile(value: &OsString) -> Result<BuiltinProfile, UsageError> {
    match text_value("the value of `--profile`", value)? {
        "simulation" => Ok(BuiltinProfile::Simulation),
        "gnosis" => Ok(BuiltinProfile::Gnosis),
        other => Err(UsageError::new(format!(
            "`{other}` is not a profile; `--profile` takes `simulation` or `gnosis`"
        ))),
    }
}

/// `--encoding`'s vocabulary.
fn parse_encoding(value: &OsString) -> Result<Encoding, UsageError> {
    match text_value("the value of `--encoding`", value)? {
        "json" => Ok(Encoding::Json),
        "xml" => Ok(Encoding::Xml),
        other => Err(UsageError::new(format!(
            "`{other}` is not a metadata encoding; `--encoding` takes `json` or `xml`"
        ))),
    }
}

/// `--format`'s vocabulary.
fn parse_format(value: &OsString) -> Result<Format, UsageError> {
    match text_value("the value of `--format`", value)? {
        "text" => Ok(Format::Text),
        "json" => Ok(Format::Json),
        "sarif" => Ok(Format::Sarif),
        other => Err(UsageError::new(format!(
            "`{other}` is not a report format; `--format` takes `text`, `json`, or `sarif`"
        ))),
    }
}

/// `--color`'s vocabulary.
fn parse_color(value: &OsString) -> Result<ColorChoice, UsageError> {
    match text_value("the value of `--color`", value)? {
        "auto" => Ok(ColorChoice::Auto),
        "always" => Ok(ColorChoice::Always),
        "never" => Ok(ColorChoice::Never),
        other => Err(UsageError::new(format!(
            "`{other}` is not a colour choice; `--color` takes `auto`, `always`, or `never`"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An argument vector shaped the way `main` builds one: `argv[0]` excluded.
    fn args(tokens: &[&str]) -> Vec<OsString> {
        tokens.iter().map(OsString::from).collect()
    }

    /// The [`CheckArgs`] of a parse that has to have produced a check.
    fn check(tokens: &[&str]) -> CheckArgs {
        match parse(&args(tokens)) {
            Ok(Command::Check(check)) => check,
            other => panic!("{tokens:?} should parse as a check, got {other:?}"),
        }
    }

    /// The message of a parse that has to have failed.
    fn usage_error(tokens: &[&str]) -> String {
        match parse(&args(tokens)) {
            Err(error) => error.message,
            Ok(command) => panic!("{tokens:?} should be a usage error, got {command:?}"),
        }
    }

    /// Assert that a diagnostic names every one of `expected`.
    fn names(message: &str, expected: &[&str]) {
        for fragment in expected {
            assert!(
                message.contains(fragment),
                "the message should name {fragment}: {message}"
            );
        }
    }

    /// A profile pairing that keeps the other assertions short.
    const SIM_JSON: [&str; 4] = ["--profile", "simulation", "--encoding", "json"];

    /// The synopsis's first line: everything but the yardstick and the root
    /// has a default, and the defaults are the quiet, safe ones.
    #[test]
    fn cli_args_check_is_implicit_and_defaults_are_conservative() {
        let parsed = check(&["--profile", "simulation", "--encoding", "json", "/cdb"]);

        assert_eq!(parsed.root, PathBuf::from("/cdb"));
        assert_eq!(
            parsed.profile,
            ProfileChoice::Builtin {
                profile: BuiltinProfile::Simulation,
                encoding: Encoding::Json,
            }
        );
        assert_eq!(parsed.format, Format::Text);
        assert_eq!(parsed.output, None);
        assert_eq!(parsed.baseline, None);
        assert!(!parsed.deny_warnings);
        assert!(!parsed.quiet);
        assert_eq!(parsed.color, ColorChoice::Auto);
    }

    /// `check` spelled out parses to the same command as `check` omitted.
    #[test]
    fn cli_args_explicit_check_subcommand_matches_the_implicit_one() {
        let mut explicit = vec!["check"];
        explicit.extend(SIM_JSON);
        explicit.push("/cdb");

        let mut implicit = SIM_JSON.to_vec();
        implicit.push("/cdb");

        assert_eq!(check(&explicit), check(&implicit));
    }

    /// The root is a positional, not a trailing one: a script that puts the
    /// path first is not writing a different command.
    #[test]
    fn cli_args_root_may_precede_the_flags() {
        let mut tokens = vec!["/cdb"];
        tokens.extend(SIM_JSON);

        assert_eq!(check(&tokens).root, PathBuf::from("/cdb"));
    }

    /// `--flag value` and `--flag=value` are the same flag.
    #[test]
    fn cli_args_attached_and_separated_values_agree() {
        let separated = check(&[
            "--profile",
            "gnosis",
            "--encoding",
            "xml",
            "--format",
            "json",
            "--output",
            "report.json",
            "--baseline",
            "base.json",
            "--color",
            "never",
            "/cdb",
        ]);
        let attached = check(&[
            "--profile=gnosis",
            "--encoding=xml",
            "--format=json",
            "--output=report.json",
            "--baseline=base.json",
            "--color=never",
            "/cdb",
        ]);

        assert_eq!(separated, attached);
        assert_eq!(
            separated.profile,
            ProfileChoice::Builtin {
                profile: BuiltinProfile::Gnosis,
                encoding: Encoding::Xml,
            }
        );
        assert_eq!(separated.format, Format::Json);
        assert_eq!(separated.output, Some(PathBuf::from("report.json")));
        assert_eq!(separated.baseline, Some(PathBuf::from("base.json")));
        assert_eq!(separated.color, ColorChoice::Never);
    }

    /// The four short forms, each carrying the same meaning as its long one.
    #[test]
    fn cli_args_short_forms_carry_their_long_meanings() {
        let short = check(&[
            "-q",
            "-o",
            "report.json",
            "--profile-file",
            "p.json",
            "/cdb",
        ]);

        assert!(short.quiet);
        assert_eq!(short.output, Some(PathBuf::from("report.json")));
        assert_eq!(parse(&args(&["-h"])), Ok(Command::Help));
        assert_eq!(parse(&args(&["--help"])), Ok(Command::Help));
        assert_eq!(parse(&args(&["-V"])), Ok(Command::Version));
        assert_eq!(parse(&args(&["--version"])), Ok(Command::Version));
    }

    /// The two boolean flags that change the exit code without changing the
    /// report (design §5 rule 4).
    #[test]
    fn cli_args_boolean_flags_are_off_until_asked_for() {
        let mut tokens = vec!["--deny-warnings", "--quiet", "--format", "sarif"];
        tokens.extend(SIM_JSON);
        tokens.push("/cdb");
        let parsed = check(&tokens);

        assert!(parsed.deny_warnings);
        assert!(parsed.quiet);
        assert_eq!(parsed.format, Format::Sarif);
    }

    /// A descriptor replaces the built-in pairing entirely.
    #[test]
    fn cli_args_profile_file_is_the_other_kind_of_yardstick() {
        let parsed = check(&["--profile-file", "acme.json", "/cdb"]);

        assert_eq!(
            parsed.profile,
            ProfileChoice::File(PathBuf::from("acme.json"))
        );
    }

    /// `--` ends flag parsing, so a root that looks like a flag is reachable.
    #[test]
    fn cli_args_double_dash_ends_flag_parsing() {
        let mut tokens = SIM_JSON.to_vec();
        tokens.extend(["--", "--not-a-flag"]);

        assert_eq!(check(&tokens).root, PathBuf::from("--not-a-flag"));
    }

    /// The first positional is a subcommand only when it is exactly `check`
    /// or `explain`; a datastore of either name is reached through a path
    /// that is not that bare word, and the help text says so.
    #[test]
    fn cli_args_only_the_bare_subcommand_words_are_subcommands() {
        let mut tokens = vec!["./check"];
        tokens.extend(SIM_JSON);
        assert_eq!(check(&tokens).root, PathBuf::from("./check"));

        let mut tokens = vec!["explain/"];
        tokens.extend(SIM_JSON);
        assert_eq!(check(&tokens).root, PathBuf::from("explain/"));

        let mut tokens = SIM_JSON.to_vec();
        tokens.extend(["--", "check"]);
        assert_eq!(check(&tokens).root, PathBuf::from("check"));
    }

    /// `explain` has exactly two valid forms.
    #[test]
    fn cli_args_explain_takes_a_code_or_the_list() {
        assert_eq!(
            parse(&args(&["explain", "/req/core/name-spaces"])),
            Ok(Command::Explain(ExplainArgs {
                query: Some("/req/core/name-spaces".to_owned()),
                list: false,
            }))
        );
        assert_eq!(
            parse(&args(&["explain", "--list"])),
            Ok(Command::Explain(ExplainArgs {
                query: None,
                list: true
            }))
        );
    }

    /// A datastore under a non-UTF-8 path still lints — the reason arguments
    /// are [`OsString`] rather than [`String`] all the way in.
    #[cfg(unix)]
    #[test]
    fn cli_args_root_keeps_its_non_utf8_bytes() {
        use std::os::unix::ffi::OsStrExt;

        let root = OsString::from(std::ffi::OsStr::from_bytes(b"/cdb/\xff\xfe"));
        let mut tokens: Vec<OsString> = args(&SIM_JSON);
        tokens.push(root.clone());

        match parse(&tokens) {
            Ok(Command::Check(parsed)) => assert_eq!(parsed.root.as_os_str(), root),
            other => panic!("a non-UTF-8 root should parse, got {other:?}"),
        }
    }

    /// The help text has to answer the two questions the grammar raises on
    /// its own: how to reach a directory named `check`, and what the exit
    /// codes mean.
    #[test]
    fn cli_help_text_documents_the_grammar_s_surprises() {
        assert!(HELP.contains("cdb-lint [check] [OPTIONS] <ROOT>"));
        assert!(HELP.contains("cdb-lint explain <CODE>"));
        assert!(HELP.contains("cdb-lint explain --list"));
        assert!(HELP.contains("./check"));
        for flag in [
            "--profile",
            "--encoding",
            "--profile-file",
            "--format",
            "--output",
            "--baseline",
            "--deny-warnings",
            "--quiet",
            "--color",
            "--help",
            "--version",
        ] {
            assert!(HELP.contains(flag), "the help text should list {flag}");
        }
        for code in ["0", "1", "2", "3"] {
            assert!(HELP.contains(code), "the help text should list exit {code}");
        }
        assert!(HELP.ends_with('\n'), "the help text is written verbatim");
    }

    /// A repeated flag is an error, never last-wins. A script that appends
    /// `--profile gnosis` to a command already saying `--profile simulation`
    /// has a bug, and choosing one of the two yardsticks would hide it.
    #[test]
    fn cli_args_rejects_repeated_flag() {
        let mut tokens = SIM_JSON.to_vec();
        tokens.extend(["--format", "text", "--format", "json", "/cdb"]);

        names(&usage_error(&tokens), &["`--format`", "more than once"]);
    }

    /// The same flag under both spellings is still the same flag.
    #[test]
    fn cli_args_a_long_form_repeats_its_own_short_form() {
        let mut tokens = SIM_JSON.to_vec();
        tokens.extend(["-o", "a.json", "--output", "b.json", "/cdb"]);
        names(
            &usage_error(&tokens),
            &["`-o`", "`--output`", "more than once"],
        );

        let mut tokens = SIM_JSON.to_vec();
        tokens.extend(["-q", "--quiet", "/cdb"]);
        names(&usage_error(&tokens), &["`-q`", "`--quiet`"]);
    }

    /// Short flags never cluster, and the diagnostic says so rather than
    /// reporting `-qo` as an unknown flag the user could go looking for.
    #[test]
    fn cli_args_short_flags_never_cluster() {
        for tokens in [&["-qo", "out.json", "/cdb"], &["-oout.json", "-q", "/cdb"]] {
            names(&usage_error(tokens), &["cluster", "-q -o"]);
        }
    }

    /// An attached value on a flag that takes none is a mistake worth
    /// naming: `--quiet=no` reads like a way to turn the flag off, and
    /// silently dropping the value would turn it on.
    #[test]
    fn cli_args_rejects_a_value_on_a_boolean_flag() {
        let mut tokens = SIM_JSON.to_vec();
        tokens.extend(["--quiet=no", "/cdb"]);

        names(&usage_error(&tokens), &["`--quiet`", "takes no value"]);
    }

    /// An unknown flag names itself, under either spelling.
    #[test]
    fn cli_args_rejects_unknown_flag() {
        names(&usage_error(&["--verbose", "/cdb"]), &["`--verbose`"]);
        names(&usage_error(&["-z", "/cdb"]), &["`-z`"]);
    }

    /// A flag at the end of the line with nothing to take.
    #[test]
    fn cli_args_rejects_missing_flag_value() {
        names(&usage_error(&["--profile"]), &["`--profile`", "value"]);
        names(&usage_error(&["/cdb", "-o"]), &["`-o`", "value"]);
    }

    /// Each vocabulary rejection lists the spellings that would have worked,
    /// so the second attempt is informed rather than guessed.
    #[test]
    fn cli_args_vocabulary_rejections_list_the_valid_spellings() {
        let mut tokens = vec!["--profile", "sim", "--encoding", "json", "/cdb"];
        names(&usage_error(&tokens), &["`sim`", "simulation", "gnosis"]);

        tokens = vec!["--profile", "simulation", "--encoding", "yaml", "/cdb"];
        names(&usage_error(&tokens), &["`yaml`", "json", "xml"]);

        tokens = SIM_JSON.to_vec();
        tokens.extend(["--format", "csv", "/cdb"]);
        names(&usage_error(&tokens), &["`csv`", "text", "json", "sarif"]);

        tokens = SIM_JSON.to_vec();
        tokens.extend(["--color", "maybe", "/cdb"]);
        names(
            &usage_error(&tokens),
            &["`maybe`", "auto", "always", "never"],
        );
    }

    /// `gpkg` is a real token in the standard's vocabulary and this build
    /// implements no such encoding, so it is rejected like any other word
    /// that is not `json` or `xml` (design §9.2).
    #[test]
    fn cli_args_rejects_the_gpkg_encoding() {
        let tokens = ["--profile", "simulation", "--encoding", "gpkg", "/cdb"];

        names(&usage_error(&tokens), &["`gpkg`", "json", "xml"]);
    }

    /// A value that must be a word, and is not text at all.
    #[cfg(unix)]
    #[test]
    fn cli_args_rejects_a_non_utf8_format_value() {
        use std::os::unix::ffi::OsStrExt;

        let mut tokens: Vec<OsString> = args(&SIM_JSON);
        tokens.push(OsString::from("--format"));
        tokens.push(OsString::from(std::ffi::OsStr::from_bytes(b"\xff")));
        tokens.push(OsString::from("/cdb"));

        match parse(&tokens) {
            Err(error) => names(&error.message, &["`--format`", "UTF-8"]),
            Ok(command) => panic!("a non-UTF-8 --format value should fail, got {command:?}"),
        }
    }

    /// A flag-shaped argument that is not text is not a flag, and saying so
    /// beats reporting it as an unknown one.
    #[cfg(unix)]
    #[test]
    fn cli_args_rejects_a_non_utf8_flag() {
        use std::os::unix::ffi::OsStrExt;

        let tokens = vec![OsString::from(std::ffi::OsStr::from_bytes(b"--\xff"))];

        match parse(&tokens) {
            Err(error) => names(&error.message, &["UTF-8"]),
            Ok(command) => panic!("a non-UTF-8 flag should fail, got {command:?}"),
        }
    }

    /// The two halves of a built-in yardstick are required together.
    #[test]
    fn cli_args_profile_and_encoding_require_each_other() {
        names(
            &usage_error(&["--profile", "simulation", "/cdb"]),
            &["`--profile`", "`--encoding`"],
        );
        names(
            &usage_error(&["--encoding", "json", "/cdb"]),
            &["`--encoding`", "`--profile`"],
        );
    }

    /// Design §4.2 wants the missing-`--encoding` error to name the encoding
    /// the datastore appears to use, and [`parse`] cannot look: it is pure,
    /// and honesty rule 5 keeps it that way. So the error is *discriminated*
    /// instead — it carries the root, and the check path enriches it.
    /// Matching on [`UsageErrorKind`] rather than on the message text is the
    /// same contract the findings keep: never key on prose.
    #[test]
    fn cli_args_missing_encoding_carries_the_root_for_the_hint() {
        let error = match parse(&args(&["--profile", "simulation", "/cdb"])) {
            Err(error) => error,
            Ok(command) => panic!("expected a usage error, got {command:?}"),
        };

        assert_eq!(
            error.kind,
            UsageErrorKind::MissingEncoding {
                root: PathBuf::from("/cdb"),
            }
        );
    }

    /// With no root there is nothing to look at, so the error stays
    /// undiscriminated: a hint would have to invent a datastore to describe.
    /// The missing root is reported first anyway, which this pins.
    #[test]
    fn cli_args_missing_encoding_without_a_root_is_not_enrichable() {
        for tokens in [
            &["--profile", "simulation"][..],
            &["--encoding", "json", "/cdb"][..],
            &["/cdb"][..],
            &["--nope", "/cdb"][..],
        ] {
            let error = match parse(&args(tokens)) {
                Err(error) => error,
                Ok(command) => panic!("{tokens:?} should fail, got {command:?}"),
            };

            assert_eq!(error.kind, UsageErrorKind::Other, "{tokens:?}");
        }
    }

    /// A descriptor and a built-in profile are two yardsticks, and a run has
    /// one.
    #[test]
    fn cli_args_profile_file_excludes_the_builtin_pairing() {
        let mut tokens = vec!["--profile-file", "acme.json"];
        tokens.extend(SIM_JSON);
        tokens.push("/cdb");
        names(
            &usage_error(&tokens),
            &["`--profile-file`", "`--profile`", "`--encoding`"],
        );

        let tokens = ["--profile-file", "acme.json", "--encoding", "json", "/cdb"];
        names(&usage_error(&tokens), &["`--profile-file`", "`--encoding`"]);
    }

    /// A check with no yardstick at all.
    #[test]
    fn cli_args_a_check_states_its_yardstick() {
        names(
            &usage_error(&["/cdb"]),
            &["--profile", "--encoding", "--profile-file"],
        );
    }

    /// The root is not optional, under either spelling of the subcommand.
    #[test]
    fn cli_args_rejects_a_missing_root() {
        names(&usage_error(&SIM_JSON), &["<ROOT>"]);
        let mut tokens = vec!["check"];
        tokens.extend(SIM_JSON);
        names(&usage_error(&tokens), &["<ROOT>"]);
    }

    /// One datastore per invocation (design §12), so a second path is a
    /// mistake rather than a second run.
    #[test]
    fn cli_args_rejects_an_extra_positional() {
        let mut tokens = SIM_JSON.to_vec();
        tokens.extend(["/cdb", "/other"]);

        names(&usage_error(&tokens), &["`/other`"]);
    }

    /// `explain` needs exactly one of its two forms.
    #[test]
    fn cli_explain_rejects_neither_and_both() {
        names(&usage_error(&["explain"]), &["--list"]);
        names(
            &usage_error(&["explain", "/req/core/name-spaces", "--list"]),
            &["--list"],
        );
    }

    /// One code per `explain`.
    #[test]
    fn cli_explain_rejects_an_extra_positional() {
        names(
            &usage_error(&["explain", "/req/core/name-spaces", "/req/core/link-href"]),
            &["`/req/core/link-href`"],
        );
    }

    /// A flag that belongs to the other subcommand is not silently ignored:
    /// `explain --format json` asks for something cdb-lint does not do, and
    /// pretending otherwise would print a plain listing to a user waiting
    /// for JSON.
    #[test]
    fn cli_args_subcommands_reject_each_other_s_flags() {
        names(
            &usage_error(&["explain", "--format", "json", "/req/core/name-spaces"]),
            &["`--format`", "explain"],
        );
        let mut tokens = SIM_JSON.to_vec();
        tokens.extend(["--list", "/cdb"]);
        names(&usage_error(&tokens), &["`--list`", "explain"]);
    }

    /// A code that is not text.
    #[cfg(unix)]
    #[test]
    fn cli_explain_rejects_a_non_utf8_code() {
        use std::os::unix::ffi::OsStrExt;

        let tokens = vec![
            OsString::from("explain"),
            OsString::from(std::ffi::OsStr::from_bytes(b"/req/\xff")),
        ];

        match parse(&tokens) {
            Err(error) => names(&error.message, &["UTF-8"]),
            Ok(command) => panic!("a non-UTF-8 code should fail, got {command:?}"),
        }
    }

    /// Help wins over whatever follows it, so a user who cannot remember the
    /// grammar gets the grammar rather than a complaint about it.
    #[test]
    fn cli_args_help_and_version_short_circuit() {
        assert_eq!(parse(&args(&["check", "--help"])), Ok(Command::Help));
        assert_eq!(parse(&args(&["explain", "-h"])), Ok(Command::Help));
        assert_eq!(
            parse(&args(&["--help", "--nonsense"])),
            Ok(Command::Help),
            "the scan ends at --help"
        );
        assert_eq!(parse(&args(&["/cdb", "-V"])), Ok(Command::Version));

        let mut tokens = SIM_JSON.to_vec();
        tokens.extend(["--", "--help"]);
        assert_eq!(
            check(&tokens).root,
            PathBuf::from("--help"),
            "after `--`, `--help` is a root"
        );
    }

    /// [`UsageError`] is an ordinary error: it prints as its message and can
    /// be handled as `dyn Error` by anything that wants to.
    #[test]
    fn cli_usage_error_is_a_standard_error() {
        let error = match parse(&args(&["--nope"])) {
            Err(error) => error,
            Ok(command) => panic!("expected a usage error, got {command:?}"),
        };
        let boxed: Box<dyn std::error::Error> = Box::new(error.clone());

        assert_eq!(error.to_string(), error.message);
        assert_eq!(boxed.to_string(), error.message);
    }
}
