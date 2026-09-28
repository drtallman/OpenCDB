# GeoPackage release procedure

The prepared versions are `opencdb` 1.1.0 and `cdb-lint` 0.2.0. They are not yet
published. `cdb_migrator` 0.1.0 is already published and is not part of this release.

## Verification

Run from a clean checkout. The GeoPackage pre-publish workflow runs the default
and enabled configurations on native Ubuntu; run the workspace commands on macOS
as well. Windows remains unverified.

```sh
cargo fmt --all --check
cargo build --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
cargo build --workspace --all-features --locked
cargo test --workspace --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
cargo test -p cdb-lint --features opencdb/gpkg-metadata --test cli_gpkg --locked
cargo tree -p opencdb --no-default-features -e normal,build --locked
cargo publish -p opencdb -p cdb-lint --dry-run --locked
cargo publish -p opencdb -p cdb-lint --all-features --dry-run --locked
```

The default dependency tree must omit `rusqlite` and `libsqlite3-sys`. The mixed
feature test ensures a library feature enabled through Cargo does not bypass the
linter's own opt-in. The package gates verify licenses, documentation, the SQL
schema and independent fixtures, then test the unpacked artifacts and install the
CLI. Before the library is published, the standalone CLI checks explicitly patch
its dependency to the unpacked library artifact; the publish dry run separately
verifies Cargo's temporary registry resolution for both packages.

## Publication

After publication is authorized, finalize the unreleased headings and README /
conformance-matrix status, commit them, and require green checks at that exact
commit. Publish the library first, then the linter:

```sh
cargo publish -p opencdb --all-features --locked
cargo publish -p cdb-lint --all-features --locked
```

Verify both versions on crates.io, compare their checksums with the tested
packages, and install the CLI with and without `--features gpkg-metadata` using
`--version 0.2.0 --locked`. Confirm `--version` identifies both crates and exercise
the documented descriptor with the GeoPackage example datastore. Tag the release
commit as `v1.1.0` and `cdb-lint-v0.2.0` after successful publication. Do not republish
or retag the migrator.
