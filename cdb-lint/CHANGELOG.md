# cdb-lint release notes

## 0.2.0 — unreleased

Adds explicit opt-in validation of the OpenCDB GeoPackage 1.2.1 metadata binding.
This release requires `opencdb` 1.1.0 or later within the 1.x series.

- Enable `gpkg-metadata` when building and select the binding through a JSON
  profile descriptor with `metadata_encoding` set to `gpkg`.
- Report the metadata-container recommendation in text, JSON, SARIF and `explain`,
  identifying its GeoPackage source separately from CDB Core requirements.
- Preserve the four exit codes, coverage reporting and baseline/deny-warning
  behavior. A missing codec is a usage error; an unsupported layout is operational.
- Locate the actual resolved library source in version/catalogue guard tests,
  so the guards also run against a standalone packaged crate.
- Include the Apache-2.0 license and these release notes in the package.

The built-in profile flags still accept JSON/XML. Default builds omit SQLite.
Payload geometry and topology coverage remain unchecked. Required attribute
models are incompatible with this GeoPackage binding. See the README for the
complete descriptor and supported scope.

## 0.1.0

Initial conformance CLI with explicit profiles, text/JSON/SARIF reports, finding
explanations and baseline ratcheting.
