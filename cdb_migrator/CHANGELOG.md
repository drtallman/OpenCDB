# cdb_migrator release notes

## 0.1.0 — 2026-09-27

Initial Rust library and command-line release for reading CDB 1.x trees and
migrating them into CDB 2.0 datastores through `opencdb` 1.0.0.

### Included

- Deterministic inventory of tiles, global payloads, source metadata, unknown
  files and unsafe filesystem entries, preserving original source paths.
- Recognition of source declarations 1.0/1.1/1.2 and pre-OGC 3.0/3.1/3.2 using
  the verified OGC 15-113r6 layout/control subset.
- Explicit operator metadata, rename maps, carry/skip policy and dry-run plans.
- Source-bound migration plans, payload-byte and tile-address verification,
  generated metadata, versioning collections and complete migration reports.
- An emitted application-profile descriptor for independent `cdb-lint` checks.
- Refusal of empty/option-shaped path arguments, ambiguous controls, unsafe
  filesystem entries, unsupported chains and conflicting destination paths.

### Scope and limitations

- One source root per migration; no incremental chains or multi-root selection.
- Payloads and embedded references are opaque. Renaming may break embedded
  references; operator metadata is an assertion, not decoded evidence.
- A conformant result applies to the emitted descriptor and recorded coverage;
  it does not certify payload semantics or application interoperability.
- Source/output paths must be physical, disjoint and stable. The destination
  filesystem must support hard links. Failed output is retained for diagnosis;
  retry at a fresh location.
- One collection per copied file limits a run to 999,999 files. Inventory,
  plans, reports and journals remain in memory along with the largest payload.
  Large-store throughput and the Yemen corpus have not been measured.
- Native validation targets macOS and Ubuntu Linux. Windows remains unverified.

### Release checks

The [README](README.md#validation-evidence) records the eleven-tree San Diego
acceptance and its explicit metadata assumptions. The
[pre-publish workflow](https://github.com/drtallman/OpenCDB/actions/workflows/migrator-prepublish.yml)
checks Linux filesystem behavior, workspace gates, package contents, registry
dependencies and CLI installation. Require a green run for the selected release
commit before publishing.
