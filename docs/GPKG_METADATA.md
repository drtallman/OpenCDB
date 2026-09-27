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

All required strings must be present; the identity strings must be nonempty.
Optional fields may be absent or null. The JSON field names are case-sensitive.

| JSON field | Type | Status and meaning |
|---|---|---|
| `ID` | string | required datastore identifier |
| `title` | string | required title |
| `description` | string | required description |
| `contactPoint` | string | required contact |
| `created` | UTC RFC 3339 string | required creation instant |
| `language` | BCP 47 string | required datastore language |
| `metadataStandard` | string | required CDB standard token, listed below |
| `metadataEncoding` | string | required; `gpkg` for this binding |
| `uom` | string | required; `M`, `FT`, `K` or `MI` |
| `update` | UTC RFC 3339 string | optional last collection/application update |
| `temporal` | temporal string | optional instant or interval |
| `accessRights` | string | optional access restrictions |
| `license` | string | optional license |
| `tilingScheme` | object | conditional: required for tiled datastores |

The allowed `metadataStandard` values are `ISO-19115:2019`,
`ISO-19115:2003`, `DDMS-5.0`, `DDMS-4.1`, `DCAT`, `DCAT-AP`,
`GeoDCAT-AP`, `NGCMP`, `FG3D`, and `NoMetadata`.

A `tilingScheme` object carries `id`, `crs`, `uom` (all strings), and
`extent` (a bounding-box object). Its coordinate unit is distinct from the
global measurement `uom`. For example, `CDB1GlobalGrid` uses `EPSG:4326`,
`degree` and the whole-earth extent `[-180,-90,180,90]`.

```json
{
  "ID": "gpkg-example",
  "title": "GeoPackage example",
  "description": "Metadata-only containers",
  "contactPoint": "ops@example.com",
  "created": "2026-09-27T12:00:00Z",
  "language": "en",
  "metadataStandard": "DCAT",
  "metadataEncoding": "gpkg",
  "uom": "M"
}
```

<a id="resource-v1"></a>
## Resource metadata

A profile-recognized resource metadata path ending in `.gpkg` holds the
existing `ResourceMetadata` JSON shape. The reference timestamp is
`updated`, otherwise `created`, otherwise the current UTC time.

| JSON field | Type | Status and meaning |
|---|---|---|
| `ID` | string | required nonempty dataset identifier |
| `type` | string | required literal `dataset` |
| `title` | string | required nonempty title |
| `description` | string | required nonempty description |
| `keywords` | string array | optional, defaults empty; required by Tiling10 for tileset metadata |
| `keywordsCodespace` | string | optional keyword vocabulary |
| `externalId` | string | optional external identifier |
| `publisher` | string | optional publisher |
| `created`, `updated` | UTC RFC 3339 strings | optional instants |
| `themes` | string array | optional, defaults empty |
| `formats` | media-type string array | optional, defaults empty |
| `contactPoint` | string | optional contact |
| `license`, `rights` | strings | optional usage statements |
| `uom` | string | conditional Geom4 measurement unit; `M`, `FT`, `K` or `MI` |
| `domainSet` | object | conditional Coverages6 description, below |
| `windingOrder` | string | conditional Face4: `clockwise` or `counterclockwise` |
| `extent` | object | optional `spatial` bounding box and/or `temporal` string |
| `associations` | link-object array | optional, defaults empty |
| `CharacterSetCode` | string | optional: `utf8` (default) or `utf16`; describes resource text, not SQLite storage |

A bounding box has four numeric fields: `west`, `south`, `east`, `north`.
A link has required nonempty `href` and `rel` strings, plus optional `type`
(media type) and `title` strings. Absolute and relative hrefs are allowed;
existing CDB link validators apply to the document.

The `domainSet` field names intentionally preserve the existing JSON API,
including underscores:

| Field | Type/default | Duty |
|---|---|---|
| `uom` | nonempty string | required range-value unit, distinct from measurement units |
| `precision` | number / 1 | smallest meaningful value |
| `scale` | number / 1 | range-value multiplier |
| `offset` | number / 0 | range-value offset |
| `data_null` | optional number | sentinel for absent values |
| `grid_cell_encoding` | string / `value-is-center` | also `value-is-area` or `value-is-corner` |
| `which_corner` | optional string | required for value-is-corner: `lower-left-corner`, `upper-left-corner`, `lower-right-corner` or `upper-right-corner` |
| `field_type` | string / `Height` | quantity being measured |
| `quantity_definition` | optional string | required when field_type is not Height |

```json
{
  "ID": "roads",
  "type": "dataset",
  "title": "Roads",
  "description": "Network",
  "updated": "2026-09-27T12:00:00Z"
}
```

<a id="collection-v1"></a>
## Collection manifests

`versions/v######/manifest.gpkg` holds the existing `CollectionManifest`
JSON shape. The reference timestamp is the collection's `applied` time.

| JSON field | Type | Duty |
|---|---|---|
| `id` | string | required collection ID, `v######` |
| `sequence` | positive integer | required 1-based sequence, redundant with id |
| `applied` | UTC RFC 3339 string | required application time |
| `description` | optional string | operator description |
| `changes` | array of change objects | required ordered changes |

Each change has required `asset` (logical-path string), `action`
(`created`, `replaced`, `deleted`, `state-set` or `state-cleared`), and
`archived` (boolean). `state` is an optional string used by `state-set`;
`resourceRecord` is an optional linked resource-metadata path; `priorState`
is an optional previous state captured for rollback. Replaced/deleted
payloads use the collection's `archive/` mirror. The journal is contiguous
from sequence 1; rollback writes inverse collections and retains history.

```json
{
  "id": "v000001",
  "sequence": 1,
  "applied": "2026-09-27T12:00:00Z",
  "description": "initial payload",
  "changes": [{
    "asset": "/Tiles/Roads.gpkg",
    "action": "created",
    "resourceRecord": "/Tiles/metadata/Roads.gpkg",
    "archived": false
  }]
}
```

## Scalar values and conditional checks

CDB record timestamps retain their full supported precision and must be UTC;
`Z` and `+00:00` are accepted. Only the metadata-extension reference uses
millisecond precision. A temporal string is an instant or `start/end`;
either single bound may be absent using `..` or an empty string, but both
may not be absent. Start must precede or equal end. Finite binary64 values
round-trip through the existing JSON serializers without changing their bits.

The body reuses the existing record parsers: unknown object members are
ignored on read and are not retained by the typed serializers. Missing or
invalid required members fail parsing; missing conditional elements are
judged by the corresponding conformance stage. The existing parser wraps
some typed/required-field decoding errors as `/req/core/metadata-encoding`
findings; this binding preserves that taxonomy rather than inventing new
codes. A known container's malformed collection body remains a Versioning
finding. See the public [conformance matrix](CONFORMANCE.md).

## Attribution and conformance boundary

CDB Attr1-C names only `vector_attributes.json` and `vector_attributes.xml`,
while Metadata5 requires one datastore encoding. A GeoPackage profile with a
required attribute model is refused before creating a datastore; its CLI
descriptor is a usage error. Attribute-model read/write methods retain
`UnsupportedEncoding`. Attribution may still be declared without content.
Existing mixed XML/JSON attribute files are inspected and receive the
existing encoding findings; this binding does not invent a Gpkg model name.

A report covers the supported metadata subset, not every GeoPackage option
or spatial payload. Geometry/Topology signals still carry `unchecked`
coverage. The unnumbered GeoPackage 1.2.1 §2 recommendation to include a user
data table is a warning, with local identifier
`/rec/geopackage/user-data-table`. A fresh metadata-only container therefore
adds a warning without making the CDB non-conformant. No dummy payload table
is written to hide the recommendation.

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

Collection application prepares all linked resource records, the global
update and the new manifest before creating archives or changing payloads.
Two assets may share a resource record; its update is prepared once using
the canonical filesystem target, including case and symlink aliases.
Publication updates that target and preserves a symlink used to reach it.
A linked metadata record cannot also be an asset target in that collection.
Publication follows payload changes with resource updates, the global
update, then the manifest. The manifest is the commit point. If its
installation fails, `versions()` skips the uncommitted directory, but
preceding live payload or metadata changes can remain and need attention.

Journal inspection distinguishes container corruption (Metadata findings)
from a malformed manifest body or sequence gap (Versioning findings).
Unsupported layouts and I/O failures abort inspection. Only successfully
scanned, committed manifests add container recommendations; archive payloads
and uncommitted directories are not metadata documents to inspect.

| Input condition | Result |
|---|---|
| Missing file, denied access, SQLite busy/OOM/I/O failure | operational I/O error |
| Disabled codec | operational `UnsupportedEncoding`; requested CLI descriptor exits 2 |
| Unsupported version, binding kind/URI, additional documents or payload/schema objects | operational `UnsupportedContainer`; CLI inspection exits 3 |
| WAL/SHM/journal companion or non-standalone storage mode | operational `UnsupportedContainer`, no recovery or mutation |
| Corrupt SQLite, missing required structure or invalid required container values | Metadata serialization error, reported as malformed metadata |
| Known container with invalid global/resource document | existing Metadata/Links finding |
| Known container with malformed manifest body or journal gap | existing Versioning finding |

SQLite schema inspection checks the required columns, nullability, defaults,
primary/unique keys and foreign-key relationships. Metadata IDs need not be
1. Required SRS rows preserve the pinned edition's WKT-1 vocabulary; common
WGS 84 datum spellings or its EPSG authority are recognized, while an
unrecognized datum identity is an unsupported subset rather than a CDB
violation. The reader does not load SQLite extensions or execute views and
triggers. It uses read-only connections and does not repair input files.

The runnable [example](../examples/gpkg_metadata.rs) and the matching
[CLI descriptor](../cdb-lint/README.md#optional-geopackage-metadata) demonstrate
a custom profile; they do not change the shipped profiles or claim adoption
of a separate OGC GeoPackage application profile.
