//! The `--format json` artifact: the report's frozen wire shape, unadorned.
//!
//! # Nothing is added
//!
//! The artifact is `serde_json::to_string_pretty` of the report and a
//! trailing newline. cdb-lint contributes no field, reorders nothing, and
//! renames nothing. Two reasons, both load-bearing:
//!
//! - a decorated report is no longer the document `docs/CONFORMANCE.md`
//!   describes to outside implementers, and the whole value of publishing a
//!   wire shape is that one document is *the* document;
//! - `cdb-lint --format json -o baseline.json` has to mint a file that
//!   `--baseline` will accept, so the writer and the reader must agree on a
//!   shape neither of them invented.
//!
//! The shape itself is `opencdb`'s, hand-written there rather than derived
//! and frozen at `1.0`; `tests/cli_output.rs` pins its exact key sets so a
//! change on either side of the boundary fails here.
//!
//! # Where the tally went
//!
//! Honesty rule 2 wants every run to say how much was checked, and this
//! artifact is the one format that cannot say it without breaking the rule
//! above. So it does not: [`write_tally`] puts the same line the text report
//! prints onto **stderr**, where it reaches a human without entering a
//! machine's document. The per-class `content` field — already on the wire,
//! carrying `checked` / `none` / `unchecked` — is the half a machine reads.

use std::io::{self, Write};

use opencdb::conformance::ConformanceReport;

use crate::render::Tally;

/// Renders `report` as the pretty-printed wire shape plus one newline.
///
/// # Errors
///
/// Returns the serializer's error if the report cannot be serialized, or the
/// sink's if it cannot be written. Either is a fact about the run, and the
/// caller answers it with [`crate::exit::OPERATIONAL`] rather than with a
/// verdict it did not deliver.
pub fn render(report: &ConformanceReport, out: &mut dyn Write) -> io::Result<()> {
    let document = serde_json::to_string_pretty(report).map_err(io::Error::other)?;
    writeln!(out, "{document}")
}

/// Writes the coverage tally, which for this format belongs on stderr.
///
/// # Errors
///
/// Returns the sink's own error.
pub fn write_tally(report: &ConformanceReport, err: &mut dyn Write) -> io::Result<()> {
    writeln!(err, "{}", Tally::of(report).line())
}
