//! A staleness guard for `cdb_lint::RUSTY_CDB_VERSION`.
//!
//! cdb-lint prints the library's version in its own version line and in
//! every report header, because the library's version is what decided the
//! verdict. That version is a hand-maintained constant — the two crates
//! share a workspace, and a build script would be machinery for a string —
//! so nothing but this test stops it drifting a release behind and making
//! every report quietly misattribute its own judgment.
//!
//! The manifest is read rather than parsed: a TOML dependency for one field
//! would break the design's zero-new-dependencies rule over a line the
//! reader can find by eye.

use std::fs;
use std::path::{Path, PathBuf};

/// The library's manifest, found relative to this crate rather than to the
/// working directory, which cargo is free to choose.
fn library_manifest() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../Cargo.toml")
}

/// The `version` of the `[package]` section, ignoring every other section —
/// the workspace table above it and the dependency versions below it.
fn package_version(manifest: &str) -> Option<String> {
    let mut in_package = false;

    for line in manifest.lines() {
        let line = line.trim();

        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }

        if let Some(rest) = line.strip_prefix("version") {
            let Some(value) = rest.trim_start().strip_prefix('=') else {
                continue; // `versioning = …`, not `version = …`.
            };
            return value
                .trim()
                .strip_prefix('"')
                .and_then(|value| value.split('"').next())
                .map(str::to_owned);
        }
    }

    None
}

/// The constant equals the version the library actually ships.
#[test]
fn cli_version_guard_matches_the_library_manifest() {
    let path = library_manifest();
    let manifest = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} must be readable: {error}", path.display()));
    let shipped = package_version(&manifest).unwrap_or_else(|| {
        panic!(
            "{} must carry a `version` in its `[package]` section",
            path.display()
        )
    });

    assert_eq!(
        shipped,
        cdb_lint::RUSTY_CDB_VERSION,
        "the library now ships {shipped}, but cdb-lint reports {}. \
         Bump RUSTY_CDB_VERSION in cdb-lint/src/lib.rs to {shipped}, and check \
         whether the release changed any finding this tool prints.",
        cdb_lint::RUSTY_CDB_VERSION
    );
}

/// The extractor reads the `[package]` version and not the first `version`
/// it can find — the manifest it is pointed at opens with a `[workspace]`
/// table and closes with two tables full of dependency versions, and a
/// looser reader would guard the wrong string.
#[test]
fn cli_version_guard_reads_only_the_package_section() {
    let manifest = "\
[workspace]
members = [\"cdb-lint\"]

[package]
name = \"rusty_cdb\"
version = \"9.9.9\"
edition = \"2024\"

[dependencies]
serde = { version = \"1.0.228\", features = [\"derive\"] }
";

    assert_eq!(package_version(manifest).as_deref(), Some("9.9.9"));
    assert_eq!(package_version("[dependencies]\nversion = \"1\"\n"), None);
    assert_eq!(package_version("[package]\nversioning = \"1\"\n"), None);
}
