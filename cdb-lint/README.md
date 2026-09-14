# cdb-lint

A command-line conformance checker for **OGC CDB 2.0 Core** (OGC 23-034)
datastores. It wraps the `rusty_cdb` library's `CdbDatastore::validate` in a
command, so an implementer who writes CDB 2.0 datastores in any language has
something to check the result with.

`docs/TDD_PLAN.md` §8 charters it in one line: *a `cdb-lint`-style binary
wrapping `CdbDatastore::validate` → an Annex-A-style report; the adoption
artifact implementers test against.*

What it judges is **structure and metadata**: the file hierarchy, the naming
system, the links, the metadata records, the declared CRS, the attribute model,
the tiling declaration, the versioning journal. What it does **not** judge is
payload content — the bytes inside a GeoPackage, a raster, or a model file are
never decoded. That boundary is not an omission to be fixed later; it is stated
plainly in [what a pass does and does not mean](#what-a-pass-does-and-does-not-mean),
and the tool reports it on every run rather than leaving you to read this file
to find it out.

---

## Build and run

cdb-lint is a member of this repository's Cargo workspace, alongside the
`rusty_cdb` library it wraps.

```sh
cargo run -p cdb-lint -- --help                       # from the repository root
cargo build --release -p cdb-lint                     # → target/release/cdb-lint
cargo install --path cdb-lint                         # → ~/.cargo/bin/cdb-lint
```

A first real run:

```sh
$ cdb-lint --profile simulation --encoding json /srv/data/cdb
cdb-lint 0.1.0 (rusty_cdb 1.0.0)
datastore  /srv/data/cdb
profile    simulation (json)

  [PASS]      attribution
  [PASS]      coverages
  [PASS]      crs
  [FAIL]      file-naming
      violation  /req/core/name-spaces
                 whitespace in name "My Tiles" (violates /req/core/name-spaces)
  [PASS]      file-structure
  [UNCHECKED] geometry           content present, no datastore-level check
  [PASS]      links
  [PASS]      metadata
  [PASS]      tiling
  [UNCHECKED] topology           content present, no datastore-level check
  [N/A]       versioning         no content this class governs

11 classes: 8 checked, 1 no content, 2 not checked · 1 violation · 0 warnings
NON-CONFORMANT
```

Four status tokens, and two of them are not verdicts: `UNCHECKED` says content
was present and nothing judged it, and `N/A` says the class governs nothing
here. Neither affects pass or fail, and
[both are explained below](#what-a-pass-does-and-does-not-mean).

---

## Command surface

```
cdb-lint [check] [OPTIONS] <ROOT>
cdb-lint explain <CODE>
cdb-lint explain --list
cdb-lint --help | --version
```

`<ROOT>` is the datastore's root directory. The first argument is read as a
subcommand only when it is exactly `check` or `explain`, so a datastore
directory of either name is reached as `./check` or `./explain`.

### Flags

| Flag | Default | What it does |
|---|---|---|
| `--profile <simulation\|gnosis>` | *required* unless `--profile-file` | the built-in application profile to judge against |
| `--encoding <json\|xml>` | *required* alongside `--profile` | the metadata encoding that profile declares |
| `--profile-file <path.json>` | — | a profile descriptor; excludes both flags above |
| `--format <text\|json\|sarif>` | `text` | how the report is rendered |
| `-o, --output <path>` | stdout | where the report goes; diagnostics still go to stderr |
| `--baseline <path.json>` | — | a previous `--format json` report; findings it already records stop failing the build |
| `--deny-warnings` | off | exit 1 when warnings are present |
| `-q, --quiet` | off | omit class rows with nothing to report, from the text report |
| `--color <auto\|always\|never>` | `auto` | colour the text report |
| `-h, --help` | — | print the usage text |
| `-V, --version` | — | print `cdb-lint <version> (rusty_cdb <version>)` |

There is **no default profile and no default encoding**. The yardstick is
stated or the run does not happen — see
[detection never chooses the yardstick](#detection-never-chooses-the-yardstick).

### Grammar

- `--flag value` and `--flag=value` both parse; `--` ends flag parsing.
- Short forms exist only for `-h`, `-V`, `-o` and `-q`, and never cluster:
  write `-q -o out.json`, not `-qo out.json`.
- **A repeated flag is an error**, not last-wins. A script that appends
  `--profile gnosis` to a command line that already says `--profile simulation`
  has a bug, and silently picking one of the two yardsticks would hide it.
- **A separated value may not look like a flag.** `-o --deny-warnings` is a
  script's mistake, and swallowing the flag would misname the file *and*
  silently drop the request — so it is an error naming both tokens. A file
  genuinely named like a flag is reached with `--output=--deny-warnings` or a
  `./` prefix; a bare `-` passes as a file name.
- `--color auto` emits ANSI only when stdout is a terminal **and the artifact
  is going there**: under `-o` the report is a file, and a file gets no ANSI
  (`--color always` still colours it). `NO_COLOR` set to any non-empty value
  forces `never`, whatever the flag says. The `json` and `sarif` formats are
  never coloured.

### Output and streams

**stdout carries the artifact; stderr carries the conversation.** `-o` moves
the artifact to a file and leaves that split intact, so
`cdb-lint --format json … > report.json` yields a report and nothing else.

- `text` — the human report: a header, one row per requirements class, the
  findings under the rows that carry them, the coverage tally, and the verdict.
- `json` — the library's own serde wire shape, pretty-printed and **unadorned**.
  cdb-lint adds no field, which is what lets the same file be read back as a
  baseline. The one thing that cannot fit in it — the aggregate coverage tally —
  is printed to stderr instead.
- `sarif` — SARIF 2.1.0, one run, for a code-scanning pipeline. Coverage rides
  in `result.kind` and `run.properties.coverage`; locations hang off one
  `originalUriBaseIds` entry named `DATASTORE_ROOT`. No `helpUri` is
  synthesized: the draft standard's own absolute requirement URI disagrees with
  its relative ones about the shape of the path, so a URL built from a finding
  code would resolve to nothing. `help.text` carries the document's verified
  identity instead.

---

## Exit codes

This is the CI contract. A job that branches on `$?` gets four distinct facts.

| Code | Meaning |
|---|---|
| `0` | the datastore conforms — or, with `--baseline`, no new findings appeared |
| `1` | findings that fail the build: violations, or — under `--deny-warnings` — warnings |
| `2` | usage or configuration error: the command line or the profile was unusable, and **nothing was judged** |
| `3` | operational failure: the tool could not inspect the datastore (missing root, unreadable file, unwritable `-o` path) |

Exit 3 is deliberately not folded into 1 or 2. "The datastore does not conform"
and "the tool could not look at the datastore" are different facts, and a
broken mount must not be reported as a failed audit.

Warnings alone never reach exit 1 without `--deny-warnings`: a warning is a
SHOULD, and a SHOULD does not decide conformance.

---

## What a pass does and does not mean

**Read this section before you read an exit code.** It is the one place where
a conformance tool can mislead without saying anything false.

A CDB 2.0 conformance verdict is given per **requirements class** — eleven of
them, five mandatory and six optional. Every class in the report carries a
verdict *and* a coverage state, and the two are independent:

| Coverage | In the text report | On the wire | Meaning |
|---|---|---|---|
| checked | `[PASS]` / `[FAIL]` | `"content": "checked"` | the datastore held content this class governs, and it was judged |
| none | `[N/A]`, "no content this class governs" | `"content": "none"` | the datastore held no such content; a declared class with nothing to check passes |
| unchecked | `[UNCHECKED]`, "content present, no datastore-level check" | `"content": "unchecked"` | content **was** present, and nothing judged it |

That third state is the important one. For **Geometry** and **Topology** it is
permanent, not a gap awaiting work: their requirements govern geometry
instances and topological graphs, which live inside payload files this crate
deliberately does not decode. When such a datastore passes those two classes,
the pass means *we did not look* — never *we looked and it was fine*.

cdb-lint never lets that read as a clean pass. `[UNCHECKED]` is yellow and
never green, in any theme, under any flag; the JSON artifact carries `content`
on every class; SARIF emits a `review` result — its own term for a finding
"requiring further analysis by a human" — rather than saying nothing at all.
Every run also prints an aggregate that survives `--quiet`:

```
11 classes: 5 checked, 4 no content, 2 not checked · 0 violations · 0 warnings
CONFORMANT
```

Two more limits worth knowing before you quote a verdict:

- **A verdict is relative to a profile.** The CDB 2.0 Core is abstract and
  cannot be implemented directly; only an application profile that pins its
  singularities can be. The same bytes may conform under one profile and not
  another, and the report names the profile it used.
- **A class the profile never declared is reported, not silently skipped** —
  but it is reported as `unchecked`, because no stage ran over that content. A
  datastore must never look *more* checked because its profile declared *less*.

The full position, including why Coverages4 is judged as
association-via-datastore and why Geometry is never swept, is
`docs/CONFORMANCE.md` §6, "Honesty notes — what a conformant verdict does not
mean". cdb-lint's rules descend from it, and `cdb-lint/tests/cli_honesty.rs`
holds the tool to them in all three output formats.

---

## Profiles

### The two built-ins

Both pin file-system storage, WGS-84 (EPSG:4326/4979), WKT-2 CRS metadata, and
`PascalCase` names; they differ in one restriction.

| `--profile` | Tiling scheme |
|---|---|
| `simulation` | `CDB1GlobalGrid` (§7.11 — the CDB 1.x-compatible grid) |
| `gnosis` | `GNOSISGlobalGrid` (§7.12) |

Each is used with `--encoding json` or `--encoding xml`, which states the
metadata encoding the profile declares.

### A profile descriptor

Two hard-coded profiles serve only the people already using this crate, so a
profile can also be stated in a JSON document and passed with `--profile-file`.
It replaces `--profile` and `--encoding` — a run with two yardsticks would have
an unattributable verdict, so combining them is an error.

This one is complete — every field the document admits, stating what the
built-in `simulation` profile states:

```json
{
  "name": "acme-sim",
  "case_rule": "PascalCase",
  "language": "en",
  "reserved_names": ["metadata"],
  "storage_crs_wkt": "GEOGCRS[\"WGS 84\",DATUM[\"World Geodetic System 1984\",ELLIPSOID[\"WGS 84\",6378137,298.257223563,LENGTHUNIT[\"metre\",1.0]]],CS[ellipsoidal,2],AXIS[\"latitude\",north,ORDER[1]],AXIS[\"longitude\",east,ORDER[2]],ANGLEUNIT[\"degree\",0.0174532925199433],ID[\"EPSG\",4326]]",
  "metadata_standard": "DCAT",
  "metadata_encoding": "json",
  "uom": "M",
  "storage_technology": "file-system",
  "conformance_classes": "all",
  "tiling_scheme": "CDB1GlobalGrid",
  "resource_metadata_dir": "metadata",
  "root_folder_name": "cdb",
  "known_extensions": ["wkt"],
  "attribute_model": null
}
```

```sh
cdb-lint --profile-file acme-sim.json /path/to/cdb
```

Seven of those fields are required — `name`, `case_rule`, `language`,
`metadata_standard`, `metadata_encoding`, `uom`, and exactly one of
`storage_crs_wkt` or `storage_crs_wkt_path`; the rest default as shown above. A
multi-line CRS reads better out of its own file, and
`"storage_crs_wkt_path": "wgs84.wkt"` resolves against the directory holding the
descriptor rather than the shell's working directory, so a checked-in
descriptor means the same thing wherever it is run from.

| Field | Required | Default | Accepted values |
|---|---|---|---|
| `name` | yes | — | any string; the report records it, and `--baseline` compares it |
| `case_rule` | yes | — | `PascalCase`, `camelCase`, `Snake_case`, `kebab-case` |
| `language` | yes | — | a BCP 47 tag, e.g. `en` |
| `metadata_standard` | yes | — | `ISO-19115:2019`, `ISO-19115:2003`, `DDMS-5.0`, `DDMS-4.1`, `DCAT`, `DCAT-AP`, `GeoDCAT-AP`, `NGCMP`, `FG3D`, `NoMetadata` |
| `metadata_encoding` | yes | — | `json`, `xml` (`gpkg` parses but is refused — see below) |
| `uom` | yes | — | `M`, `FT`, `K`, `MI` |
| `storage_crs_wkt` | one of the two | — | WKT-2 (ISO 19162) text |
| `storage_crs_wkt_path` | one of the two | — | a path to a WKT-2 file, resolved **against the descriptor's own directory** |
| `reserved_names` | no | `[]` | names exempted from the case rule, beyond the ones the library reserves itself; each entry must be one possible path component (non-empty, no `/`) |
| `storage_technology` | no | `"file-system"` | `file-system` |
| `conformance_classes` | no | `"all"` | `"all"`, `"mandatory"`, or an array of class tokens (`attribution`, `coverages`, `crs`, `file-naming`, `file-structure`, `geometry`, `links`, `metadata`, `tiling`, `topology`, `versioning`) |
| `tiling_scheme` | no | none declared | `CDB1GlobalGrid`, `GNOSISGlobalGrid` |
| `resource_metadata_dir` | no | `"metadata"` | the directory name holding resource-metadata records — one path component, and it is exempted from the case rule automatically, exactly as the built-ins exempt their own `metadata/` |
| `root_folder_name` | no | `"cdb"` | the datastore's root folder name — one path component |
| `known_extensions` | no | `[]` | file extensions outside the §7.4 table this profile vouches for, spelled without their dot (`tif`, not `.tif`) |
| `attribute_model` | no | `null` | an inline attribute model, shaped exactly like a `vector_attributes.json` |

Two behaviours are worth stating outright:

- **Unknown keys are rejected.** A typo that fell back to a default would
  produce a yardstick nobody stated, which is worse than a failed run.
- **A broken descriptor exits 2, before the datastore is opened.** WKT that
  does not parse, a malformed language tag, an invalid attribute model, or
  `"metadata_encoding": "gpkg"` — which this build does not implement — are all
  usage errors naming the field at fault. So is a field whose value could
  never take effect: a `resource_metadata_dir` carrying a `/` matches no
  directory component, so every resource record would go unrecognized and the
  report would claim more conformance than was checked; a dotted
  `known_extensions` entry vouches for nothing; a blank or newline-carrying
  `name` would forge the report's own header. Letting validation meet any of
  them would file the *profile's* defects as findings against a datastore
  that did nothing wrong — or, worse, file nothing at all.

### Detection never chooses the yardstick

cdb-lint reads the datastore to **suggest** a flag and never to **choose** one.
Run without `--encoding` and it fails, naming the encoding the datastore
appears to use so the second attempt is informed:

```
error: `--profile` is half a yardstick: add `--encoding`, which takes `json` or
`xml`. cdb-lint states the metadata encoding rather than reading it off the
datastore it is judging — the datastore's global metadata is
global_metadata.json; you probably want --encoding json
try `cdb-lint --help` for usage
```

Run with the *wrong* encoding and the datastore is convicted under Requirement
Metadata5, the exit code says so, and a note on stderr suggests the other
spelling while stating that the report is unchanged. Deriving the encoding from
the thing being judged would make that requirement unfailable.

---

## explain

A report prints stable finding codes and no prose, because a code is an
identifier and prose is not. `explain` is the other half of that bargain.

```
$ cdb-lint explain /req/core/attribute-model-content-B
/req/core/attribute-model-content-B
  class    attribution
  spec     OGC 23-034 §7.1.2.3
  Each attribute in the model has a unique identifier
```

`cdb-lint explain --list` prints every code with its class, clause and gloss.
An unknown code exits 2 and suggests the codes containing what you typed, so
`explain name-spaces` finds `/req/core/name-spaces` — and a dash-leading
fragment works too: after `explain`, a token no flag vocabulary recognizes is
the query, so `explain -content-b` finds
`/req/core/attribute-model-content-B`.

The catalogue is documentation with a completeness check, not a derived
artifact: a test scans the library for the codes it can emit and demands an
exact two-way match, so a code cannot ship without a row and a row cannot name
a code the library never emits. Nothing machine-checks that a row cites the
right clause; those columns are hand-authored and reviewed.

---

## Baselines

A baseline turns cdb-lint into a **ratchet**: existing findings stop failing
the build, new ones still do. It is the right tool for adopting a conformance
check on a datastore that does not yet pass, and it is the most dangerous
feature here.

### Minting one

A baseline file *is* a `--format json` report. There is no second format:

```sh
cdb-lint --profile simulation --encoding json --format json -o ci/cdb-baseline.json /path/to/cdb
cdb-lint --profile simulation --encoding json --baseline ci/cdb-baseline.json /path/to/cdb
```

Write it outside the datastore. A baseline dropped inside the root becomes
content the next run judges.

### What counts as new

Identity is **`(class, code, severity)` → count**. A finding is new when its
triple is absent from the baseline, or when its count exceeds the baseline's —
so a second occurrence of an already-recorded code is new too, and a datastore
cannot get steadily worse in one place unnoticed.

Message text is deliberately **not** part of the identity: wording may change
within `1.x` of the library, and a message-keyed baseline would fail spuriously
the day a sentence improved.

Locations are not part of it either, because the report format carries none —
which cuts one way worth knowing: a finding **fixed in one place can offset a
new one of the same triple somewhere else**. Equal counts read as nothing new.
The ratchet holds each triple's count, never its individual sites.

A baseline taken under a **different profile** is refused (exit 2): two
profiles judge by different rules, and absorbing one's findings into the
other's run would swallow real ones. A baseline whose `root` differs is
accepted — CI paths move, and a datastore's identity is not its path.

The refusal compares the profile **name**, which is all the report records —
and the name carries no `--encoding` half, so a baseline minted under
`simulation`/`xml` reads as the same profile in a `simulation`/`json` run and
can absorb real Metadata5 findings. **Re-mint the baseline whenever the
declared encoding changes.** See [Two honest limits](#two-honest-limits).

Findings that went away are reported and change nothing. A build never fails
because it got better.

### The hazard, stated plainly

**`--baseline` can exit 0 over a datastore that does not conform.** That is
what a ratchet is for. The containment is that nothing else moves: the report
is the same report, and the verdict line always says what happened.

```
NON-CONFORMANT (3 violations) — no new findings since baseline; exit 0 by --baseline
```

The same discipline applies in the other direction — `--deny-warnings` can fail
a build over a datastore the report calls conformant, and the line says so:

```
CONFORMANT (3 warnings) — exit 1 by --deny-warnings
```

Neither flag ever changes the verdict, the rows, or the findings. If you
consume only `$?`, you are reading half of what the run said.

---

## Two honest limits

1. **The baseline's profile check compares the profile *name*.** That is all
   the wire shape records, so two different descriptors that share a `name`
   pass the check — and the two built-in pairings of one profile share a name
   *by construction*, since `--encoding` never reaches the wire: a
   `simulation`/`xml` baseline is accepted by a `simulation`/`json` run.
   cdb-lint cannot close that hole — the report carries a string, not a
   profile — and saying so beats implying a guarantee the data cannot
   support. Give descriptors distinct names, and re-mint a baseline whenever
   the declared encoding changes.
2. **Message text may change within `1.x`.** The library freezes its API at
   `1.0`, not the wording of its findings. Consumers key on `code` — which is
   stable, absolute, and what `explain` takes — and treat `message` as prose
   for humans.

---

## Versions and stability

`cdb-lint` is at **0.1.0** and is pre-1.0 in earnest: its flags, its rendered
output, its SARIF property names and its descriptor schema may change in a
later `0.x`. Pin a version if a pipeline depends on the shape of what it reads.

The library it wraps, **`rusty_cdb` 1.0**, is under semver — but the freeze
covers the public *API surface*, not the *content* of a conformance report. A
spec erratum or a revised interpretation may change what a validator reports
without that being a breaking API change. `cdb-lint --version` names both
versions for exactly that reason, and every report's header repeats them.

---

## Where the rules come from

| Document | What it holds |
|---|---|
| `docs/CONFORMANCE.md` | the public conformance matrix: requirement → API → test, the errata list, and §6's honesty notes that these rules descend from |
| `docs/superpowers/specs/2026-09-13-cdb-lint-design.md` | this tool's design: the command surface, the honesty contract, the scope fence, and the risks |
| `docs/TDD_PLAN.md` §8 | the post-1.0 charter this effort was drawn from |
| OGC 23-034 | the standard itself — `http://www.opengis.net/doc/IS/CDB-core/2.0` |
