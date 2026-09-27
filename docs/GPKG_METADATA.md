# OpenCDB GeoPackage metadata binding, version 1

This optional binding stores one OpenCDB metadata document per GeoPackage.
Enable `gpkg-metadata` and select `MetadataEncoding::Gpkg` in a custom
application profile. The built-in profiles retain their JSON/XML choices.

The normative CDB authority is the bundled **OGC CDB 2.0 Core Standard
(23-034)**, especially §4.5, §7.9.2 and Metadata1–8. Its §3 references
**GeoPackage 1.2.1 (12-128r15)**. This binding uses that edition's base
container and [metadata extension](https://www.geopackage.org/spec121/#extension_metadata).
The file arrangement, JSON bodies and binding identifiers below are
OpenCDB conventions, not an OGC CDB application profile standard.

Each finalized SQLite file has application ID `0x47504B47`, user version
`10200`, the required `gpkg_spatial_ref_sys` and `gpkg_contents` tables,
and the `gpkg_extensions`, `gpkg_metadata` and `gpkg_metadata_reference`
tables. The SRS catalogue contains the reserved -1, 0 and 4326 rows. Its
WKT-1 definition is separate from the datastore's WKT-2 CRS metadata.

Both metadata tables register `gpkg_metadata` with null column names,
`read-write` scope and the pinned extension definition URI. One metadata
row contains the unchanged JSON document, with `dataset` scope and MIME
`application/json`. Its ID is unrestricted. One package-scoped reference
points to it, with a UTC millisecond timestamp and null table, column,
row and parent selectors. There are no user data tables.

The `md_standard_uri` is the full URI
`https://github.com/drtallman/OpenCDB/blob/main/docs/GPKG_METADATA.md`
followed by one of these versioned anchors. It does not replace the CDB
record's own `metadataStandard` field.

<a id="global-v1"></a>
## Global metadata

`global_metadata/global_metadata.gpkg` holds the existing `GlobalMetadata`
JSON shape. `metadataEncoding` is `gpkg`; JSON is the internal document,
not a separate standalone encoding. The reference timestamp is `update`
when present, otherwise `created`.

<a id="resource-v1"></a>
## Resource metadata

A profile-recognized resource metadata path ending in `.gpkg` holds the
existing `ResourceMetadata` JSON shape. The reference timestamp is
`updated`, otherwise `created`, otherwise the current UTC time.

<a id="collection-v1"></a>
## Collection manifests

`versions/v######/manifest.gpkg` holds the existing `CollectionManifest`
JSON shape. The reference timestamp is the collection's `applied` time.

## Container and publication limits

Readers inspect table columns, keys and relationships, not SQL spelling.
Quoted names, case variations, ordinary indexes and SQLite internal
objects are accepted. Extra documents, payload tables, custom extensions,
views, triggers and other container versions are explicitly unsupported.
Readers refuse WAL, shared-memory and journal companions. Only finalized,
standalone files with a single external writer are supported.

Writes prepare a complete database in memory, then publish it by atomic
replacement of a temporary sibling. Existing destinations are inspected
first; unsupported or corrupt content is preserved. Replacement preserves
file permissions. Preparation and failed persistence do not replace old
bytes, and temporary files are removed on failure. Memory scales with the
container size. Atomic file replacement does not make an entire collection
transaction atomic and does not promise byte-identical SQLite layouts.
