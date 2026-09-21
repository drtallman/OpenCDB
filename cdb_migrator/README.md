# cdb_migrator

An unpublished Rust reader and migrator for OGC CDB 1.x trees. It copies opaque
payload bytes into a CDB 2.0 datastore through `opencdb`'s public facade, generates
records from explicit operator metadata, and validates against an emitted
application-profile descriptor. It does not decode raster, model, geometry,
topology, or embedded-reference content.

The source reference is [OGC CDB 1.x, OGC 15-113r6](https://docs.ogc.org/is/15-113r6/15-113r6.html);
the destination reference is [CDB 2.0 Core, OGC 23-034](https://docs.ogc.org/is/23-034/23-034.html).
The reader recognizes specification declarations `1.0`, `1.1`, `1.2`, `3.0`,
`3.1`, and `3.2` using the verified 15-113r6 layout/control subset. This is not
blanket support for every historical schema. Missing or unknown declarations
remain visible in reader findings and provenance; no source version is inferred.

## Build and commands

Build from this workspace; the package is not published to crates.io:

```sh
cargo build --workspace
./target/debug/cdb_migrator --help
./target/debug/cdb_migrator inventory /private/tmp/source_cdb
./target/debug/cdb_migrator migrate /private/tmp/source_cdb /private/tmp/migration_out \
  --metadata-file /private/tmp/metadata.json \
  --rename-map /private/tmp/renames.json --carry-extras \
  --id example --title 'Example datastore' --description 'Operator description' \
  --contact ops@example.com --timestamp 2026-09-20T12:00:00Z
./target/debug/cdb-lint --profile-file /private/tmp/migration_out/cdb_migrator-descriptor.json \
  --format json /private/tmp/migration_out/cdb
```

Use physical paths: root and ancestor symlinks are refused, including macOS
`/tmp` and `/var` aliases. Source and output must be disjoint. The destination
parent can exist, but its `cdb` and both companion filenames (including their
`.partial` forms) must be absent. Inputs and destination parent must stay stable
and operator-controlled throughout execution. Observable changes are rejected;
this is not an atomic snapshot or protection against malicious ancestor replacement.

`inventory <root>` emits one tab-separated line per entry: `TILE`, `GLOBAL`,
`META`, `UNRECOGNIZED` with a reason, or `UNSAFE` with a reason. A `SUMMARY` gives
all counts. Backslashes and control characters are escaped (`\\`, `\t`, `\n`,
`\r`, `\u{...}`), so raw filenames cannot introduce rows. Non-UTF-8 unsafe paths
are explicitly rendered as `raw-path-hex:` followed by the original raw path's
platform-encoded bytes. The raw path follows the supplied root spelling and can
be relative. Neither display convention changes source identity. Inventory
success means entry classification completed, not that the source conforms or
can be migrated. It can include unsafe entries and can complete when control
metadata is malformed. The command does not print tree or control findings;
callers can inspect them through `Cdb1Tree::findings()`, and migration retains
them in refusal diagnostics or report provenance, as applicable.
Non-UTF-8 command arguments are refused without replacement or panic.

`migrate <root> <out-parent>` accepts:

| Option | Meaning |
| --- | --- |
| `--metadata-file JSON` | Explicit resource records and content applicability declarations |
| `--rename-map JSON` | Exact source-relative filenames to literal target-relative filenames |
| `--carry-extras` / `--skip-extras` | Mutually exclusive disposition of unrecognized files |
| `--id`, `--title`, `--description`, `--contact` | Override global identity defaults |
| `--timestamp RFC3339` | UTC creation/update/collection time; otherwise use the facade clock |
| `--dry-run` | Print planned `MOVE`/`SKIP` rows and counts; create nothing |

Options follow both positional paths. Bare option tokens cannot fill either path
slot; use an explicit path such as `./--dry-run` for a literal name beginning
with `-`. Repeated or unknown options are rejected.
A dry run validates the source and operator plan only: its rows are planned,
not copied or verified, and it makes no output-conformance claim. Destination
preflight, collection capacity, actual I/O and validation occur during migration.
Global identity defaults identify the source and explicitly mark contact as
unspecified; supply real operator identity when needed.

| Exit | Meaning |
| --- | --- |
| 0 | Inventory/dry-run completed, or migration completed and is conformant under its descriptor |
| 1 | Migration completed with a nonconformant report |
| 2 | Invalid arguments, operator input, or unsupported/refused request |
| 3 | Operational failure, including I/O, XML errors returned as errors, facade, serialization or output failure |

Malformed controls retained by the reader are inventory findings and cause
migration planning to refuse. Migration stdout is the complete JSON report;
diagnostics go to stderr. An output-stream error can produce exit 3 even after
completed artifacts have been published, so inspect existing artifacts before retrying.

## Explicit metadata and names

Every recognized tile or global payload requires exactly one declaration. For
example, this is a **synthetic operator assertion**, not metadata inferred from
an elevation filename:

```json
{
  "resources": [
    {
      "source_path": "Tiles/N32/W118/001_Elevation/L00/U0/N32W118_D001_S001_T001_L00_U0_R0.tif",
      "record": {
        "ID": "example_elevation",
        "type": "dataset",
        "title": "Example elevation",
        "description": "Operator-supplied description",
        "keywords": ["example"],
        "domainSet": {
          "uom": "m",
          "grid_cell_encoding": "value-is-center"
        }
      },
      "coverage": true,
      "measurement_values": false,
      "generated_faces": false
    }
  ],
  "attribute_model": null
}
```

The JSON schemas reject unknown fields and duplicate keys. Each record needs
nonblank ID, title, description and keywords. Dataset codes 001–005 must declare
coverage and a `domainSet`; other opaque applicability remains an operator
assertion. `domainSet` must explicitly supply units and grid-cell encoding;
corner sampling also requires `which_corner`. Omitted domain scalars use the
public schema defaults (precision 1, scale 1, offset 0, field type `Height`);
optional null sentinel and quantity definition stay absent. Choose values from
knowledge of the source, not this example. `measurement_values: true` requires
record `uom`; `generated_faces: true` requires `windingOrder`. False declarations
reject those conflicting conditional fields. An optional `attribute_model`
contains explicit attribute definitions and an optional supplementary schema URI.
No units, winding order or attribute model is inferred from payload bytes.

Declarations must match every recognized payload exactly, with unique IDs and
no unused entries. An empty or extra-only tree needs no metadata manifest.
Original metadata XML is carried as opaque source metadata; it is not translated
into generated records. Generated records declare `NoMetadata`; a source's
metadata-standard declaration remains provenance.

Default targets fold ASCII letters to lowercase. Arbitrary dataset/model labels
cannot recover their original case from the folded name; exact original paths
are retained in the report. Unsupported names and all file/directory/generated
path collisions require an explicit repair or refusal. A rename map is a plain
JSON object, for example:

```json
{
  "GTModel/tree.rgb.attr": "gtmodel/tree_rgb.attr"
}
```

This preserves the final `attr` extension while removing the interior dot from
the stem. Values are literal safe destination-relative names; they are not
silently folded. Tile overrides must preserve parsed tile identity and address.
Rename-map keys identify files, not directories. Per-file mappings can separate
case aliases by placing their files in distinct destination directories:

```json
{
  "Foo/a.txt": "extras/foo_upper/a.txt",
  "foo/b.txt": "extras/foo_lower/b.txt"
}
```

Embedded references are **unchecked and may break** after any case folding or rename.

Without an extras flag, migration refuses only when unrecognized entries exist.
`--carry-extras` puts those files under `extras/`; `--skip-extras` records their
exact original names as skipped. Recognized source metadata is always preserved
under `extras/source_metadata/`, independently of that choice. Skipping extras
cannot bypass required recognized-payload metadata, unsafe filesystem entries,
unsupported version chains, or invalid tile addresses.

## Outputs and scope of acceptance

Each completed migration produces three outputs under the output parent:

- `cdb/`: datastore, generated resource/global metadata and versioning journal.
- `cdb_migrator-descriptor.json`: complete effective validation profile.
- `cdb_migrator-migration-report.json`: full conformance report plus exact source
  accounting, original declarations/findings, effective operator metadata,
  actual generated records/global metadata, copy and address verification,
  renames, skipped content and explicit unchecked semantics.

These companion filenames differ from the stable profile identity,
`cdb-migrator-v1`. Parameters such as known extensions and attribute definitions
can differ while that name stays fixed. Compare the **complete descriptor** when
reusing a baseline: the frozen linter's profile-name check alone cannot establish
that two migrations used equivalent parameters.

Acceptance requires complete source accounting, direct copied-byte equality,
filename-derived tile-address preservation, required generated metadata and a
CONFORMANT verdict under the emitted descriptor. Independently run `cdb-lint`
with `--profile-file` as shown above. Inspect class coverage as well as findings:
`unchecked` is not a payload check. Filename-derived addresses do not certify
source directory agreement. A conformant result does not certify operator
assertions, opaque payload validity, embedded-reference usability or application
interoperability. The report preserves the complete validator result, including
warnings and coverage; no warning is promoted to a violation.

v1 supports one source root. Incremental chains and configurations selecting
another or multiple roots are refused. One versioning collection is created per
copied source file; the public `MAX_MIGRATED_FILES` limit is 999,999. Generated
records do not consume collections. Memory includes the in-memory inventory,
plan, operator manifest, report and journal metadata, plus the largest buffered
payload. Bounded payload batches do not mean constant total memory. The facade
scans the journal for each collection, so large-store speed and capacity are
not established by the small acceptance corpus.

The destination filesystem must support hard links: companions are published
exclusively from `.partial` files, with the report published last. Failures retain
incomplete artifacts for diagnosis and never overwrite an existing result.
Retry with a fresh output location. Fixed timestamps make datastore and descriptor
bytes deterministic for identical inputs; the report's absolute `conformance.root`
necessarily reflects its actual destination.

## Validation evidence

The environment-gated inventory test accepts a directory of roots containing
`Tiles/` or `GTModel/`; loose raster fixture directories are excluded:

```sh
CDB1_CORPUS_ROOT=/private/tmp/corpus/Tests/Data \
  cargo test -p cdb_migrator --test corpus -- --nocapture
```

When unset, this test returns without corpus I/O. Vendor bytes and generated
acceptance outputs are kept outside the repository.

On 2026-09-21, all eleven San Diego roots in
[CesiumGS/cdb-to-3dtiles](https://github.com/CesiumGS/cdb-to-3dtiles/tree/0f9487f03c9e5f7d0c8e2505509ef07fa3eb41fc/Tests/Data)
were migrated through the CLI: 1,455 files / 239,094,424 bytes, 741 tiles,
707 global payloads, seven explicitly carried unknown files, and 1,448 generated
resource records. Three explicit `tree.rgb.attr` renames retained `attr`.
Independent streaming comparisons matched every copied byte; source SHA-256
baselines and exact source accounting remained unchanged; an independent
filename parser and integer grid calculation checked all 741 addresses. All
eleven independent `cdb-lint` runs returned 0, with eleven classes, zero warnings
and zero violations each; their entire JSON reports matched the embedded reports.

Those test-only manifests explicitly assumed dataset 001 metre/Height ranges,
dataset 004 dimensionless/ImageSample ranges with a test quantity URI, center
sampling, precision/scale 1, offset 0 and no null sentinel. Other resources were
declared noncoverage; all declared no measurement values or generated faces,
and no attribute model was supplied. These declarations were not checked against
payload headers or contents. All trees lacked `Version.xml`; the missing-source
version findings were retained. **Yemen-scale corpus: NOT TESTED.** Native
case-distinct and non-UTF-8 filesystem fixtures were unavailable on the macOS
volume; production helper tests cover those guards without claiming Linux-native
verification. Whole-branch review remains separate from this task's acceptance.
