# rusty_cdb — TDD Plan for the OGC CDB 2.0 Core Standard

Source spec: `docs/OGC CDB Version 2 - Part 1_ Core Standard.html` (OGC 23-034, version 2.0).

## 1. Goal and scope

Build a Rust API (library crate `rusty_cdb`) for reading and writing CDB 2.0
datastores, developed test-first: every requirement (`/req/core/...`) in the
spec becomes one or more failing tests before its implementation is written.

An important property of the spec: **the CDB 2.0 Core is abstract and cannot be
directly implemented** (§5.1). Only *application profiles* that restrict the
core are implementable. The crate therefore has two layers:

1. **Core layer** — types, traits, and validators that encode every core
   requirement exactly as written (encoding-agnostic, CRS-agnostic, storage-agnostic).
2. **Default profile layer** — a concrete, implementable application profile
   (`profiles::simulation`) that restricts the core the way the spec's own
   examples do: file-system storage, EPSG:4326/4979 (WGS-84), WKT-2 CRS
   metadata, JSON or XML metadata encoding, and the CDB1GlobalGrid tiling
   extension (backwards compatible with CDB 1.x).

The conformance target is Annex A: the mandatory conformance classes are
**CRS, File Naming, File Structure, Links, Metadata**. Optional modules
(Attribution, Coverages, Geometry, Media Types, Tiling + both extensions,
Topology, Versioning) are implemented behind the same TDD discipline, and each
is feature-complete per its requirements class when enabled.

## 2. Toolchain and dependencies (installed)

- Rust stable 1.97.0 via rustup; `cargo test` is the test runner.
- GeoRust crates: `geo` (algorithms: clipping, winding, contains),
  `geo-types` (Simple Features geometry primitives — directly matches the
  spec's Geometry module), `wkt` (WKT geometry I/O), `geojson` (GeoJSON I/O).
- Supporting: `serde`/`serde_json` (JSON metadata), `quick-xml` (XML metadata),
  `chrono` (RFC 3339 / ISO 8601 datetimes, UTC enforcement), `thiserror`
  (error taxonomy), dev-deps `tempfile` (datastore-on-disk tests) and `approx`.
- Deliberately deferred: `proj` and `gdal` (require native libs via Homebrew;
  not needed for core conformance — the core stores CRS *metadata*, it never
  transforms coordinates). Add later if a profile needs reprojection or
  GeoTIFF coverage decoding.

## 3. Crate architecture

```
src/
  lib.rs           // public API surface, re-exports
  error.rs         // CdbError taxonomy (one variant family per requirements module)
  naming.rs        // Resource Path & File Naming module      (mandatory)
  hierarchy.rs     // File Hierarchy Structure module         (mandatory)
  links.rs         // Links module                            (mandatory)
  media_types.rs   // Media Types module                      (optional)
  metadata/
    mod.rs         // Global + resource metadata module       (mandatory)
    temporal.rs    // RFC 3339 datetimes + temporal intervals
  crs/
    mod.rs         // CRS module                              (mandatory)
    wkt2.rs        // minimal WKT-2 (ISO 19162) reader/validator for CRS metadata
    vertical.rs    // optional VCRS class
  geometry.rs      // Geometry module (wraps geo-types)       (optional)
  coverage.rs      // Coverages module (domainSet metadata)   (optional)
  attribution.rs   // Attribution module                      (optional)
  tiling/
    mod.rs         // Abstract tiling module                  (optional)
    cdb1_grid.rs   // CDB1GlobalGrid extension (CDB 1.x compatible)
    gnosis_grid.rs // GNOSISGlobalGrid extension
  topology.rs      // Topology module (+ optional Face class) (optional)
  versioning.rs    // Versioning module                       (optional)
  datastore.rs     // CdbDatastore facade: open/create/read/write/validate
  profiles/
    mod.rs         // ApplicationProfile trait (restrictions over the core)
    simulation.rs  // default WGS-84 + CDB1GlobalGrid file-system profile
tests/             // integration tests = conformance suite (Annex A driven)
  conformance_core.rs
  roundtrip_datastore.rs
```

Unit tests live in each module (`#[cfg(test)] mod tests`) named after the
requirement they verify, e.g. `req_core_name_spaces_rejects_spaces`. Integration
tests in `tests/` exercise whole-datastore scenarios on `tempfile` directories.

## 4. Requirement → test inventory

Every SHALL in the spec, mapped to tests. (R = requirement, Rec =
recommendation — recommendations are implemented as lint-level warnings from
the validator, not hard errors.)

### Phase 1 — Naming (`/req/core/name-*`), mandatory
| Req | Rule | Tests |
|---|---|---|
| Name1 | no spaces in any path/folder/resource name | reject `"road network"`, accept `"road_network"` |
| Name1 (rec) | no unicode | warn on `"café"` |
| Name1-B | forbidden chars `# % & { } \ < > * ? / $ ! ' " : @` and blanks | one test per char class; property test over the full set |
| Name3 | single language, English recommended | validator config carries language; mixed-language detection is out of scope → document as profile duty |
| Name4 (rec) | avoid empty folders | validator warns on empty dirs |
| Name5 | profile must define a style guide | `ApplicationProfile::style_guide()` is a required trait item (compile-time enforcement) |
| Name6 | one case rule per datastore (Pascal/camel/Snake/kebab) | `CaseRule` enum; classify + enforce; mixed-case datastore fails |
| Name7 | file extensions per spec table; industry standard otherwise | extension table lookup: `.tif .rgb .rgba .jp2 .flt .shp .shx .dbf .dbt .xml .xsd .zip .gpkg .gltf .glb .json .bmp`; unknown-but-standard passes with warning |

### Phase 2 — File hierarchy (`/req/core/file-*`), mandatory
| Req | Rule | Tests |
|---|---|---|
| File1 | platform supports folders + hierarchy | `Datastore::create` on tempdir succeeds |
| File2 | datastore has a root; all content reachable from it | path escape (`../`) rejected; every stored resource resolves under root |
| File3 / PFile1 | all files under root or subdirs; links to physical resources allowed | symlinked subfolder accepted |
| File5 | hierarchy begins with `/` | root normalization test |
| RFile1 (rec) | root should be `/cdb` | default create uses `cdb`, custom allowed with warning |
| File6 | `global_metadata` folder at root holds all global metadata/vocabularies/enums | create writes it; open without it → validation error |

### Phase 3 — Links (`/req/core/link-*`), mandatory
| Req | Rule | Tests |
|---|---|---|
| Link1 | `href` is a URL; relative and absolute both allowed | parse absolute http(s), relative path; garbage rejected |
| Link2 | `rel` mandatory | builder without rel fails to compile / validate |
| Link3/4 (rec) | `type` (media type hint) and `title` optional | serde round-trip with and without |

### Phase 4 — Media types, optional
- `MediaType` enum covering the spec table (`model/flt`, `application/geo+json`,
  `application/geopackage+sqlite3`, `image/tiff; application=geotiff`,
  glTF variants, GML, JP2, JSON, PNG, `application/vnd.shp`, TIFF, XML) +
  `Other(String)` for extensibility. Tests: to/from string round-trips,
  extension ↔ media-type consistency with the Phase 1 table.

### Phase 5 — Metadata (`/req/core/metadata-*`), mandatory
| Req | Rule | Tests |
|---|---|---|
| Metadata1 | global metadata file in `Global_Metadata` folder, referenced from root | create/read round-trip |
| Metadata2 | metadata standard ∈ {ISO-19115:2019, ISO-19115:2003, DDMS-5.0, DDMS-4.1, DCAT, DCAT-AP, GeoDCAT-AP, NGCMP, FG3D, NoMetadata} | enum parse/reject |
| Metadata3 | global metadata path/link specified | missing link → error |
| Metadata4 | one language, IETF BCP 47 tag | `en-US` ok, `xx!` rejected |
| Metadata5 | one encoding for all metadata: xml, json, or gpkg | mixed encodings in one store → error |
| Metadata6 | datetimes UTC, RFC 3339 §5.6 | accept `2026-07-14T00:00:00Z`, reject naive/offset-local |
| Metadata7 | temporal intervals ABNF incl. half-bounded `../` forms | parser tests: bounded, `../end`, `start/..`, invalid |
| Metadata8 | one UoM for measurements: M, FT, K, MI; element named `uom` | enum + serde field-name test |
| Global elements table | mandatory: ID, title, description, contactPoint, created, language; optional: update, temporal, accessRights, license | builder validation + XML/JSON round-trips via quick-xml/serde |
| Resource metadata table | mandatory: ID, type(=dataset), title, description; conditional/optional per table (incl. the Geom4 conditional `uom: Option<UnitOfMeasure>`, added in Phase 7); CharacterSetCode default utf8 | same pattern |

### Phase 6 — CRS (`/req/core/crs/*`), mandatory
| Req | Rule | Tests |
|---|---|---|
| CRS2 | ISO 19111 consistency | encoded in type design; doc-tested |
| CRS3 | exactly one CRS per datastore | second CRS registration → error |
| CRS4 | geodetic/geographic only, non-projected | WKT-2 starting `GEOGCRS`/`GEODCRS` (incl. inside `COMPOUNDCRS`) accepted; `PROJCRS` rejected |
| rec crs-definition | WGS-84 / EPSG:4326/4979 recommended | default profile pins it |
| CRS5 | CRS metadata stored in global metadata folder, WKT-2 encoded | round-trip the spec's own compound-CRS example verbatim |
| CRS6 | one UoM for all coordinates | mismatch between CRS angle unit and declared UoM → error |
| CRS7 | dynamic datum ⇒ epoch as decimal Gregorian year (`yyyy.00` = Jan 1) | `Epoch(2017.53)` parse/format; dynamic CRS without epoch → error |
| VCRS1-3 (optional class) | VCRS per ISO 19111, WKT-2 encoded, default units meters | `VERTCRS` parse; missing unit ⇒ meters |

The `wkt` crate handles geometry WKT only, so `crs::wkt2` is a small,
purpose-built tokenizer/validator for WKT-2 CRS strings (keyword tree +
required nodes), not a full parser — enough to satisfy CRS4/CRS5/VCRS tests.

### Phase 7 — Geometry (`/req/core/geometry-*`), optional
| Req | Rule | Tests |
|---|---|---|
| Geom1 | Simple Features conformance | model = `geo-types`; identity documented |
| Geom2 | type codes: 0-7 core, 1001-1004 Z, 2001-2004 M, ext 11-14 | `GeometryCode ↔ geo_types::Geometry` mapping table tests (codes match GeoPackage) |
| Geom3 | Z geometries require z-UoM in global metadata | PointZ without registered UoM → error |
| Geom4 | M geometries require m units in dataset metadata | same pattern |
| Geom5 | every coordinate unambiguously in the one datastore CRS | `CdbGeometry::validate_in` `source_crs` check → `ForeignCrs` (incl. the unverifiable-claim rule: a claim against an anonymous datastore CRS is foreign) |
| Geom6 | GeometryCollection: single CRS across members | by-construction impossibility — members carry no CRS (documented + doc-tested), same pattern as CRS2; recursion covers member z/m-UoM checks |

`geo-types` has no native Z/M; the Z/M variants are eight typed structs behind
a closed enum, superseding the parallel-array `ZGeometry { xy, z: Vec<f64> }`
sketch above — spec-absent combos (MultiPolygon Z, ZM) are then unrepresentable
and Geom2's table holds by construction. Approved design record:
`docs/superpowers/specs/2026-08-01-geometry-module-design.md`.

### Phase 8 — Coverages (`/req/core/coverage-*`), optional
| Req | Rule | Tests |
|---|---|---|
| Coverages1-3 | module conformance rules | validator wiring |
| Coverages4 | coverage CRS = datastore CRS | mismatch → error |
| Coverages5 | resource metadata present per Phase 5 | missing → error |
| Coverages6 | domainSet: uom (mandatory), precision=1, scale=1, offset=0, data_null, grid_cell_encoding=value-is-center (corner variant requires which-corner), field_type=Height, quantity_definition | defaults test; corner-without-which-corner → error; scale/offset apply to values but never to data_null |
| Rec 7/8 | tiled coverages follow tiling module + one tiling extension | integration test once Phase 9 lands |

### Phase 9 — Tiling: abstract module + CDB1GlobalGrid, optional
| Req | Rule | Tests |
|---|---|---|
| Tiling1-3 | tiled content follows this module | wiring |
| Tiling4 | one TilingScheme per datastore | second scheme → error |
| Tiling5 | scheme CRS = storage CRS | mismatch → error |
| Tiling6 | scheme UoM = CRS UoM (e.g. decimal degrees for 4326) | unit test |
| Tiling7 | extent covers entire earth, no gaps | scheme bbox == (-90,-180,90,180) |
| Tiling8/9 | scheme definition + tileset metadata in global metadata, per declared standard | round-trip |
| Tiling10 | tileset metadata ⊇ {ID, Title, Description, Keywords} | builder validation |
| TCE1-2 (CDB1) | conforms to 2DTMS CDB1GlobalGrid | structural tests below |
| TCE3 | EPSG:4326, axis order lat,lon | constructor pins it |
| TCE4 | all tile origins/extents in decimal degrees | bbox type is degrees-only |
| TCE6 | LoD 0 = 1°×1° geocells, matrix 360×180 | `tile_extent(lod0, r, c)`; count test |
| TCE7 | LoD range -10..=23; LoD ≤0 tiles = one geocell; LoD≥1 quad subdivision; tiles 1024×1024 from LoD 0 up | subdivision math: parent/child round-trips, lat/lon → tile index → bbox → contains point; negative LoDs keep geocell extent with reduced raster size |
| Zones/coalescence | CDB 1.x zone table drives matrix width per latitude row | width(lat) table tests at zone boundaries (e.g. ±50°, ±70°, ±75°, ±80°, ±89°) |

### Phase 10 — GNOSISGlobalGrid extension, optional
- Level 0 = 2 rows × 4 cols of 90°×90° tiles (TCE6-G).
- Quad-tree split, except pole-touching tiles split into 3 (no longitude split
  at the pole) → exactly 4 tiles touch each pole at every level (test at
  levels 1-4).
- Levels 0..=28; (level,row,col) packs into a single u64 key — pack/unpack
  round-trip property test.
- Coalescence factors recomputed per tile matrix (contrast test vs CDB1 grid).

### Phase 11 — Topology (`/req/core/topology-*`), optional
| Req | Rule | Tests |
|---|---|---|
| Topo1 | ISO 19107 model | type design: Node/Edge/Face primitives separate from geometry |
| Topo2/3 | unique node & edge IDs | duplicate insert → error |
| Topo4 | directed node: edge IDs signed (− leaving, + entering) | adjacency signs after linking |
| Topo5 | directed edge stores start/end node IDs | constructor invariant |
| Topo6 | edges crossing tile boundaries clipped; clip node shares the same ID in both tiles | clip a two-tile-spanning edge (uses `geo` line clipping); corner-exactly-on-boundary edge case; shared-ID assertion |
| Face1-4 (optional class) | unique face ID; face = list of directed nodes+edges; winding order documented in metadata | build face from edge ring; winding declared or error; island/hole association left to profile (documented) |

### Phase 12 — Versioning (`/req/core/versioning*`), optional
| Req | Rule | Tests |
|---|---|---|
| V1/V2 | apply + track change collections (CRUD) | apply collection, journal records it |
| V3 | on apply: global `update` element refreshed; resource `updated` refreshed | timestamp assertions |
| V4 | create / delete / update assets | one test each on tempdir datastore |
| V5 | minimum capability: whole-file replacement | byte-level replace + history |
| V6 | transitory state changes capturable (e.g. road closed) | state-change event round-trip, rollback view |

### Phase 13 — Attribution (`/req/core/attribute*`), optional
- Attr1: attribute model declared; stored in `global_metadata`; file named
  `vector_attributes.xml|json` (name test, wrong ext → error).
- PAttr1: file may hold a URI to an external schema.
- Attr2: every attribute has unique ID + name + description (spec's
  StreetName/StreetType/StreetWidth example as fixture).

### Phase 14 — Datastore facade, profile trait, conformance suite

Split into **14a** (mandatory core — the `0.1.0` milestone, done) and **14b**
(full conformance over the optional modules, last phase).

**14a (as built):**
- `CdbDatastore::create(parent, &dyn ApplicationProfile, DatastoreSeed)` /
  `open(root)` / `validate(&dyn ApplicationProfile)` → `ConformanceReport`
  listing pass/fail per requirements class. Signature decisions vs the original
  sketch: `create` takes a `DatastoreSeed` because the §7.9.4.1 mandatory
  identity elements (ID, title, description, contactPoint) are instance
  properties no profile can supply — policy fields come only from the profile,
  so collisions are impossible by construction. `validate` takes the profile
  (instead of the datastore storing one) because Annex A judges a datastore
  *against a profile declaration* and the spec defines no on-disk profile
  identifier; the datastore stays self-describing for read/write operations,
  the profile is the yardstick for conformance. `open(root)` keeps its planned
  signature.
- `profiles::ApplicationProfile` (object-safe): every spec singularity duty is
  a required trait item — style guide (Name5), storage CRS (CRS3), metadata
  standard/encoding/UoM (Metadata2/5/8), storage technology (Annex B),
  conformance-class declaration (Annex A `/conf/minimal-core`), and the
  resource-metadata recognizer (a Name5 path duty). Provided defaults only
  where the spec itself defaults: root name `cdb` (RFile1), language derived
  from the style guide (Name3/Metadata4 single source), tiling declaration
  `None`, `known_extensions()` empty (Name7-B vouching hook).
- Annex A: `tests/conformance_core.rs` mechanizes `/conf/minimal-core`
  (profile-declaration inspection) and operationalizes it — a full
  default-profile datastore passes all five mandatory classes with zero
  warnings, and break-one-class scenarios each report the matching violation
  under the right class.
- Round-trip at 14a is restricted to what exists: `tests/roundtrip_datastore.rs`
  covers global metadata (json and xml), storage CRS (incl. a dynamic-datum
  epoch via `COORDINATEMETADATA`), and resource metadata, all through the
  facade with a fresh reopen.

**14b (deferred until Phases 7–9 exist):** extend the conformance suite over
the optional classes and run the full round-trip — create → write vector tiles
+ an elevation coverage + metadata → reopen → read back → byte/value equality.

## 5. TDD working agreement

1. **Red**: for each requirement row above, write the failing test(s) first,
   named `req_<module>_<slug>_...`. Cite the spec section in a doc comment.
2. **Green**: implement the minimum to pass.
3. **Refactor** with tests green; `cargo fmt` + `cargo clippy -D warnings`.
4. Recommendations (SHOULD) become `Warning`s in `ConformanceReport`, tested
   the same way; requirements (SHALL) become `Error`s.
5. Phases 1-6 + 14 deliver the mandatory Annex A conformance class and ship as
   `0.1.0`; each later optional module bumps the minor version.
6. Commit per phase (or per requirement group within big phases), message
   `phase-N(<module>): <requirements covered>`.

## 6. Phase order and rationale

Order: 1 Naming → 2 Hierarchy → 3 Links → 4 Media types → 5 Metadata → 6 CRS
→ 14a facade/conformance for the mandatory core → 7 Geometry → 8 Coverages →
9 Tiling+CDB1 → 10 GNOSIS → 11 Topology → 12 Versioning → 13 Attribution →
14b full conformance suite.

Rationale: pure-function string/path validation first (fast feedback, no I/O),
then the metadata/CRS backbone every other module depends on (the spec notes
metadata and CRS are the only cross-module dependencies), then the mandatory
conformance milestone, then optional modules ordered by dependency (tiling
before topology because edge-clipping needs tile boundaries; coverages before
tiling extensions only for metadata, the tiled-coverage integration test waits
for Phase 9).

## 7. Progress

- Phases 1–6 done (commits `phase-1(naming)` … `phase-6(crs)`), 97 tests.
  Note: the Name4 empty-folder warning is emitted by the hierarchy validator
  (Phase 2), since detecting it requires walking the tree. Phase 5 notes:
  wire names follow the spec tables verbatim (`ID`, `contactPoint`,
  `CharacterSetCode`); datetime parsing is strict about the RFC 3339 `T`
  separator (chrono would tolerate a space); gpkg is accepted as a declared
  encoding but the core cannot write that container (profiles do).
  Phase 6 notes: the WKT-2 parser is comma-lenient because the spec's own
  CRS examples are malformed strict WKT (missing comma between compound
  members, space before `[`) — they parse verbatim in the tests. The storage
  CRS persists as canonical WKT-2 in `global_metadata/crs.wkt`, wrapped in
  `COORDINATEMETADATA[crs,EPOCH[…]]` when a datastore epoch is set; a
  second, different CRS write is refused (CRS3).
- Phase 14a done (commits `phase-14a(profiles)` ×2, `phase-14a(datastore)` ×3,
  `phase-14a(conformance)`), 142 tests (129 unit + 12 integration + 1 doc) —
  the `0.1.0` milestone. Design notes: the `crs` stem joined
  `global_metadata`/`vector_attributes` in `StyleGuide`'s auto-reserved set,
  because this crate's own CRS5 persistence writes `crs.wkt` for any profile;
  the `.wkt` extension stays out of the spec-verbatim Name7 table — instead
  profiles vouch for extra industry-standard extensions via
  `known_extensions()` (the simulation profile vouches `wkt`), so a fresh
  datastore validates with zero violations and zero warnings. The simulation
  profile's resource-metadata convention (its Name5 path duty) is a sibling
  `metadata/` directory with a reserved lowercase name:
  `/Tiles/RoadNetwork.gpkg` → `/Tiles/metadata/RoadNetwork.json`.
  Profile-vs-datastore declaration mismatches are report findings
  (`CdbViolation::DeclarationMismatch`), not operational errors — `error.rs`
  is unchanged by the phase. `ConformanceReport` implements `Display` but
  deliberately not serde (no normative wire format exists; revisit at 14b).
- Phase 7 done (commits `phase-7(geometry)` ×5 incl. one review fix,
  `phase-7(metadata)`), 153 tests (140 unit + 12 integration + 1 doc) — the
  first optional class after the milestone, shipped as `0.2.0`. `geometry`:
  `GeometryCode` (20 codes, GeoPackage-consistent; extension codes 11–14
  flagged non-CDB-1.x), eight typed Z/M structs with length invariants,
  `CdbGeometry` (15 variants, lossless `From` over geo-types incl.
  Line/Rect/Triangle), and `GeometryContext` + `validate_in` (Geom3/4 z/m-UoM
  presence, Geom5/6 foreign-CRS — a claim against an anonymous datastore CRS
  counts as foreign). `GeometryViolation` has six variants and no warning type
  (§7.6 has no SHOULDs); Geom5-B/6-B and the ZM / MultiPolygon-Z absence hold
  by construction (members carry no CRS, spec-absent combos unrepresentable).
  The Geom4 m-value UoM rides on the new `ResourceMetadata.uom`. Design record:
  `docs/superpowers/specs/2026-08-01-geometry-module-design.md`.
- Next: Phase 8 (coverages).
