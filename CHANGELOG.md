# opencdb release notes

## 1.1.0 — unreleased

Adds optional GeoPackage metadata support through the existing datastore,
metadata and versioning APIs. Enable `gpkg-metadata` and use a custom
application profile declaring `MetadataEncoding::Gpkg`.

- Read and write global/resource records and collection manifests using the
  [OpenCDB GeoPackage 1.2.1 document binding](docs/GPKG_METADATA.md).
- Prepare containers before collection mutation, replace each document
  atomically, and retain manifest-last publication and rollback behavior.
- Inspect recognized metadata containers during conformance checks. Unsupported
  layouts remain operational errors; malformed supported records yield findings.
- Report the GeoPackage recommendation to include a user data table as a warning
  with the local code `/rec/geopackage/user-data-table`.
- Accept equivalent SQL defaults, UNIQUE-column order and mixed-case SRS
  organization names; reject filesystem aliases that overlap collection targets.

This is an additive minor release. Public method signatures, existing JSON/XML
wire formats and report fields remain compatible. New findings and error variants
use non-exhaustive enums. Default builds omit SQLite; enabled builds use bundled
SQLite without requiring a separate system installation.

The built-in profiles retain their JSON/XML choices. Spatial payloads remain
opaque. The binding accepts one metadata document per finalized container and
refuses unsupported layouts, unfinished SQLite journals and required attribute
models. Atomic document replacement is not a transaction over the whole datastore.
See the binding for the full limits and [RELEASING.md](docs/RELEASING.md) for gates.

## 1.0.0

Initial stable public API for CDB 2.0 Core requirements, application profiles,
datastore operations and conformance reports. The API guarantee does not freeze
the content or human-readable wording of conformance findings.
