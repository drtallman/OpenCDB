# OpenCDB

Rust tools for the OGC CDB 2.0 Core Standard (OGC 23-034): a library that
reads, writes, and validates CDB datastores, and a command-line conformance
checker built on it.

CDB is the Open Geospatial Consortium's standard for geospatial
synthetic-environment datastores: the terrain, imagery, and feature databases
that flight simulators and GIS webapps stream at runtime. The standard lives at
<http://www.opengis.net/doc/IS/CDB-core/2.0>. This CDB has no relation to
djb's constant-database file format.

| Crate | Version | Purpose |
|---|---|---|
| `opencdb` | 1.0.0 | the library: types, validators, profiles, and a conformance reporter for every core requirements module |
| [`cdb-lint`](cdb-lint/README.md) | 0.1.0 | the CLI: judges a datastore against a profile and renders the report as text, JSON, or SARIF |

## The library

The CDB 2.0 Core is abstract by design: §5.1 of the standard says it cannot
be implemented directly, only restricted into an implementable *application
profile*. The crate mirrors that split.

- **Core modules** encode each requirements module as types and validators,
  independent of encoding, CRS choice, and storage: `naming`, `hierarchy`,
  `links`, `media_types`, `metadata`, `crs`, `geometry`, `coverage`,
  `attribution`, `tiling` (with the CDB1GlobalGrid and GNOSISGlobalGrid
  extensions), `topology`, and `versioning`.
- **Profiles** supply the restrictions. The `ApplicationProfile` trait names
  every choice a profile must pin. `SimulationProfile` pins file-system
  storage, WGS-84, WKT-2 CRS metadata, JSON or XML metadata encoding, and the
  CDB 1.x-compatible CDB1GlobalGrid tiling; `GnosisProfile` pins
  GNOSISGlobalGrid instead.
- **Validation** produces an Annex A report. `CdbDatastore::validate` walks
  the datastore once and returns a `ConformanceReport`: violations for the
  spec's SHALLs, warnings for its SHOULDs, and a stable requirement-URI code
  on every finding.

```rust
use opencdb::{CdbDatastore, DatastoreSeed, SimulationProfile};

let profile = SimulationProfile::json();
let seed = DatastoreSeed::new("MyStore", "My Store", "Demo datastore", "ops@example.com");
let datastore = CdbDatastore::create(parent_dir, &profile, seed)?;
let report = datastore.validate(&profile)?;
assert!(report.is_conformant());
```

## The linter

```sh
cargo install --path cdb-lint
cdb-lint --profile simulation --encoding json /path/to/datastore
```

```text
cdb-lint 0.1.0 (opencdb 1.0.0)
datastore  /path/to/datastore
profile    simulation (json)

  [PASS]      crs
  [FAIL]      file-naming
      violation  /req/core/name-spaces
                 whitespace in name "My Tiles" (violates /req/core/name-spaces)
  [UNCHECKED] geometry           content present, no datastore-level check
  ...

11 classes: 8 checked, 1 no content, 2 not checked · 1 violation · 0 warnings
NON-CONFORMANT
```

Four exit codes separate four facts: 0, the datastore conforms; 1, findings
fail the build; 2, the command line or profile was unusable; 3, the tool
could not inspect the datastore. `--format json` emits the library's frozen
wire shape, `--format sarif` emits SARIF 2.1.0 for code-scanning pipelines,
`--baseline` turns the check into a ratchet for datastores that do not yet
pass, and `cdb-lint explain <code>` documents any finding code. The
[cdb-lint README](cdb-lint/README.md) covers the whole surface, including
what a pass does and does not mean.

## Conformance

Coverage spans all eleven Annex A requirements classes: the five mandatory
classes (CRS, File Naming, File Structure, Links, Metadata) and the six
optional ones (Attribution, Coverages, Geometry, Tiling, Topology,
Versioning).

[docs/CONFORMANCE.md](docs/CONFORMANCE.md) is the public conformance matrix.
It maps every requirement to the API item that implements it and the test
that proves it, lists the draft defects the crate normalizes, and states
what a conformant verdict does not mean. Three tests fail whenever the
matrix falls behind the code.

A report never claims more than was checked. Validation judges structure and
metadata and never decodes payload bytes, so geometry and topology content
is reported as unchecked rather than passed, in every output format.

## What 1.0 freezes

`opencdb` 1.0 freezes the public API surface under semantic versioning:
types, traits, signatures, module paths, and the serde wire shape of a
report. It deliberately leaves the content of findings unfrozen: the
standard is a draft that carries recorded defects, so the findings a
datastore draws may change within 1.x. Key on a finding's `code()`, never on
its display text. `cdb-lint` is versioned separately at 0.1.0 so its flags
and output can evolve without touching the library's semver.

## Development

Every change is test-driven against the spec: first a failing test named
`req_<module>_<slug>` citing the clause, then the minimum implementation,
then a refactor with the gates green. The workspace holds 553 tests.

```sh
cargo test --workspace                                      # the gate
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

[docs/TDD_PLAN.md](docs/TDD_PLAN.md) holds the requirement inventory, the
module layout, and the phase order that carried the project from the first
naming test to 1.0.

## Dependencies

`geo-types` supplies the geometry model, matching the spec's OGC Simple
Features mandate; the crate adds the Z and M variants the spec needs as thin
typed wrappers. A purpose-built tokenizer in `crs/wkt2.rs` reads CRS WKT-2.
`proj` and `gdal` are absent on purpose: the core stores CRS metadata and
never transforms coordinates, so nothing here needs a native library.

## License

Apache-2.0. See [LICENSE](LICENSE).
