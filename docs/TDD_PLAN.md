# rusty_cdb — TDD Plan for the OGC CDB 2.0 Core Standard

Source spec: OGC CDB Version 2 Part 1: Core Standard (OGC 23-034, version 2.0),
<http://www.opengis.net/doc/IS/CDB-core/2.0>. Design records referenced below
("design note") are working notes kept outside the published tree.

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
| Global elements table | mandatory: ID, title, description, contactPoint, created, language; optional: update, temporal, accessRights, license; conditional `tiling_scheme` / wire `tilingScheme` (Tiling8, added in Phase 9 — the first conditional element on the global table) | builder validation + XML/JSON round-trips via quick-xml/serde |
| Resource metadata table | mandatory: ID, type(=dataset), title, description; conditional/optional per table (incl. the Geom4 conditional `uom: Option<UnitOfMeasure>`, added in Phase 7, and the Coverages6 conditional `domain_set: Option<DomainSet>` / wire `domainSet`, added in Phase 8); CharacterSetCode default utf8 | same pattern |

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
and Geom2's table holds by construction. Approved design record: the
2026-08-01 geometry design note.

### Phase 8 — Coverages (`/req/core/coverage-*`), optional
| Req | Rule | Tests |
|---|---|---|
| Coverages1-3 | module conformance rules | realized as the free `coverage::validate_coverage_instance` fn + `DomainSet::validate` + module-doc conformance statements (design-level, per CRS2/Geom1) |
| Coverages4 | coverage CRS = datastore CRS | Geom5-style provable-match → `CoverageCrsMismatch` (shared `crs::authority_ids_match`); a claim against an anonymous datastore CRS ⇒ mismatch ("unidentified") |
| Coverages5 | resource metadata present per Phase 5 | missing → `MissingResourceMetadata`; realized on `ResourceMetadata` (delegates to Metadata via `#[from]`) |
| Coverages6 | domainSet: uom (mandatory), precision=1, scale=1, offset=0, data_null, grid_cell_encoding=value-is-center (corner variant requires which-corner), field_type=Height, quantity_definition | realized on `ResourceMetadata.domain_set` (§7.9.4.2 conditional element, 2nd after Geom4); defaults test; corner-without-which-corner → error; scale/offset apply to values but never to data_null |
| Rec 7/8 | tiled coverages follow tiling module + one tiling extension | realized by `tests/tiled_coverage.rs` (`rec_core_coverage_tiling_abstract_and_extension`), added in Phase 9 |

### Phase 9 — Tiling: abstract module (§7.10) + CDB1GlobalGrid (§7.11), optional (done, as built)
| Req | Rule | Realization |
|---|---|---|
| Tiling1-3 | tiled content follows this module | module-doc conformance statements + the validators `TilingScheme::validate` (method) and the free `validate_tileset_metadata` — the design-level pattern of CRS2/Geom1 |
| Tiling4 | same `TilingScheme` definition used datastore-wide (`/req/core/tiling-tilingscheme-consistent`) | by construction — one `tilingScheme` element on the one global record (datastore-wide consistency) |
| Tiling5 | scheme CRS provably matches storage CRS | `TilingScheme::validate` → `SchemeCrsMismatch` (shared `crs::authority_ids_match`; an anonymous datastore CRS ⇒ "unidentified" mismatch) |
| Tiling6 | scheme UoM = CRS coordinate unit (decimal degrees for 4326) | `validate` → `SchemeUomMismatch` (ASCII case-insensitive; the coordinate unit, never a mensuration unit) |
| Tiling7 | extent covers entire earth, no gaps | `validate` → `IncompleteExtent`; whole earth = `Bbox` (west −180, south −90, east 180, north 90) |
| Tiling8 | scheme definition in global metadata | `TilingScheme::require(&GlobalMetadata)` (associated fn) → `MissingTilingScheme`; realized as the conditional `GlobalMetadata.tiling_scheme` / wire `tilingScheme` — the third §7.9.4.2-style conditional element (after Geom4 `uom`, Coverages6 `domainSet`) and the *first* on the global table |
| Tiling9/10 | tileset metadata per declared standard ⊇ {ID, Title, Description, Keywords} | free `validate_tileset_metadata`: Tiling9 standard/encoding conformance by construction (rides the resource-metadata record), Tiling10 → `MissingTilesetKeywords`; TCE5's own metadata box is absent from the document, so tileset duties live here |
| TCE1-3 (CDB1) | 2DTMS CDB1GlobalGrid; EPSG:4326, axis order lat,lon | `TilingScheme::cdb1_global_grid` preset; addressing per OGC 2DTMS |
| TCE4 | tile origins/extents in decimal degrees; coordinates validated | `Bbox` is degrees-only; `tile_at` → `CoordinateOutOfRange` on non-finite / out-of-[−90,90]×[−180,180] — the `f64`-bearing variant the enum's dropped `Eq` derive was reserved for |
| TCE6 | LoD 0 = 1°×1° geocells, matrix 360×180, NW origin | `matrix_size` / `tile_extent`; row 0 = northernmost, col 0 = −180° |
| TCE7 | LoD −10..=23; LoD ≤0 = one geocell (raster halves 512…1); LoD≥1 quad subdivision; tiles 1024×1024 from LoD 0 up | `Lod` (validated), `raster_size`, `parent` / `children` — negative LoDs single-chain over the same extent, quadtree from LoD 0; parent/child round-trips + point containment |
| Zones/coalescence | CDB 1.x zone table drives matrix width per latitude row | `ZONE_BANDS` = the |lat|-band const `[(50,1),(70,2),(75,3),(80,4),(89,6),(90,12)]`, matched by a row's equator-most integer latitude with row-exact boundaries (tests at ±50/70/75/80/89°); `coalescence_factor`, and the `MisalignedColumn` column-alignment rule |

**Binding tile-addressing convention (Task 6 ruling).** Tiles are half-open
cells `[south, north) × [west, east)`: column `= ⌊(lon+180)/h⌋` (west-inclusive)
and row `= ⌈(90−lat)/h⌉ − 1` (south-inclusive) — symmetric — with ±90°/±180°
edges clamped to the last in-range row/column (`h = 1.0` at LoD ≤ 0, `(1/2)ⁿ` at
LoD n ≥ 1). This supersedes the earlier prose floor row formula, which disagreed
with the containment test at grid-aligned latitudes; the ceiling−1 form is the
shipped, reviewer-verified convention (incl. the lat = 90 clamp).

### Phase 10 — GNOSISGlobalGrid extension, optional (done, as built)
| Req | Rule | Realization |
|---|---|---|
| TCE1 (§7.12) | conformance with the module | module-doc conformance statements (the design-level pattern of CRS2/Geom1/Tiling1-3) |
| TCE2 (§7.12) | conform to 2DTMS TileMatrixSet + VariableMatrixWidth and the registered GNOSISGlobalGrid definition | formula-driven grid: coalescence `2^max(0, n − bit_length(pole_distance))`, verified verbatim against the registry's level 0-3 `variableMatrixWidths` fixtures; `GnosisLevel` (0..=28, `u8` — negatives unrepresentable) → `GnosisLevelOutOfRange`; 256×256 tiles (`gnosis_grid::TILE_SIZE_CELLS`, registry-pinned — §7.12 states no raster rule itself) |
| TCE3 (§7.12) | EPSG:4326, axis order lat,lon | `TilingScheme::gnosis_global_grid()` preset; `tile_at(lat, lon, level)` parameter order |
| TCE4 (§7.12) | origins/extents/bboxes in decimal degrees | degrees-only `Bbox` extents; `tile_at` validation reuses `CoordinateOutOfRange` (the two extensions share the `-uom` slug) |
| TCE5 (§7.12) | metadata box MISSING from the draft (again) | tileset metadata duties covered via Tiling9/10; doc-noted |
| TCE6 (§7.12) | level 0 = 2×4 grid of 90°×90° tiles; splitting per the 2DTMS annexes | `matrix_size` (4·2ⁿ × 2·2ⁿ); `parent`/`children` with uniform factor-driven enumeration — the 3-way pole split falls out of the formula (3 children at poles, 4 elsewhere; 4 tiles always touch each pole, each 90° wide, tested at levels 1-4) |
| u64 keys (§7.12.2, via TCE2-B) | (level,row,col) in one 64-bit key | `GnosisTileAddress::key`/`from_key`: level<<59 \| row<<30 \| col — 5+29+30 = exactly 64 at level 28; numeric order = (level,row,col) order |

Phase 10 also resolved the two decisions deferred from Phase 9 (recorded in
the 2026-08-26 GNOSIS-grid design note): **no shared grid
trait** — the two grids are deliberately parallel surfaces by convention
(rule of two: no generic consumer exists) — and the **symmetric rename**
`Lod` → `Cdb1Lod`, `TileAddress` → `Cdb1TileAddress`, `LodOutOfRange` →
`Cdb1LodOutOfRange` (breaking, 0.5.0), with GNOSIS types prefixed
`GnosisLevel`/`GnosisTileAddress`. The same half-open tile-addressing
convention (§4 Phase 9 note) binds both grids.

### Phase 11 — Topology (`/req/core/topology-*`), optional — as built
| Req | Rule | Realization/Tests |
|---|---|---|
| Topo1 | ISO 19107 model (box typos "19101") | type design: `TopoNode`/`TopoEdge`/`TopoFace` + ISO vocabulary (`SignedEdge`, `DirectedEdge`, `DirectedNode`), coordinate-free constructible |
| Topo2/3 | unique node & edge IDs | `TopoGraph::insert_node/insert_edge` duplicate → error |
| Topo4 | directed node: edge IDs signed (− leaving, + entering) | adjacency maintained by the graph (wrong signs unrepresentable); isolated nodes exempt; Display renders `-17`/`+17` |
| Topo5 | directed edge stores start/end node IDs | mandatory fields + unknown-endpoint insert error; clip parts chain through minted nodes |
| Topo6 | clip at tile boundaries; clip node shares one ID in both tiles | `clip_edge_to_tile(edge, Bbox)`: closed containment (`geo::Intersects`), Liang–Barsky slabs, crossings pinned to the boundary coordinate exactly (both grids' extents are dyadic → adjacent tiles agree bitwise); touch/graze/vertex/collinear/zigzag cases; preconditions `EdgeHasNoGeometry`/`EdgeGeometryEndpointMismatch`/`InvalidClipExtent`/`EdgeInFace`/`EdgeGeometryNotFinite`; `geo::line_intersection` as test oracle (`BooleanOps::clip` rejected: integer snapping breaks the exactness Topo6 turns on); final-review hardening: endpoint-exact lerp (t=0/1), finite-coordinate precondition, mint-headroom guard, seeded mini-sweep |
| Face1-4 (optional class) | unique face ID; face = directed nodes+edges; winding in metadata | `insert_face` chain/close validation ("exterior boundary" reading); `directed_nodes` derived (Rec Topology 1 structural, never fires); `windingOrder` rides `ResourceMetadata` (4th §7.9.4.2 use) + `validate_topology_dataset`; islands = profile duty (doc only); `WindingOrder` wire spellings clockwise/counterclockwise |

As-built notes: no `TopologyWarning` type — §7.13 has no SHOULD-level finding
(first optional class without one). No serde on primitives/`TopoGraph` (no
spec'd topology encoding; revisit at 14b). Face1's box reuses Topo1's URI
verbatim; Face3/Face4 boxes carry `/rec/` prefixes but are conditional
SHALLs per their labels and text.

### Phase 12 — Versioning (`/req/core/versioning*`), optional — as built
| Req | Rule | Realization/Tests |
|---|---|---|
| V1 | apply + track changes incl. CRUD (box URI collides with the class URI — cite §7.14.2) | `CdbDatastore::apply_collection[_at]` + the manifest journal; every precondition pre-mutation |
| V2 | versioning collection (box text is a subset of V1's; table `version-collection` vs box `versioning-collection`; concept read from §7.14.3 prose) | `PendingCollection` builder → `CollectionManifest` in immutable `versions/v######/` commit dirs (declared-encoding `manifest.<enc>`); 1-based capped ids; one change per asset; contiguity checked at read |
| V3 | on apply: metadata updates; global `Update`; resource `Updated` (+ the spec's own editorial TODO on who/description linkage) | one shared `applied` instant → manifest + `GlobalMetadata.update` + each linked record's `updated` (three-way equality tested); manifest `description` + per-change `resourceRecord` fill the admitted gap |
| V4 | create/delete/update assets | the three ops; `AssetAlreadyExists`/`AssetMissing` preconditions |
| V5 | minimum: file replacement | byte-faithful replace (binary-content test); prior bytes archived in the collection dir |
| V6 | capture state changes ("the ability capture" typo) | `SetState`/`ClearState` events with recorded `priorState`; `state_of(asset, as_of)` timeline; free-form states (profiles may restrict) |

Beyond the boxes (§7.14 intro's "enables … rollback", user-chosen): full
byte-level rollback — `rollback_collection[_at]` (latest-only) and
`rollback_to[_at]` (tail inverses in descending order) build inverse
collections from manifest + archive and apply them through the SAME
pipeline, so rollbacks are journaled and V3-stamped. As-built notes: no
`VersioningWarning` (no SHOULD in §7.14); `VersioningError { Violation,
Io, Serialization }`; the conformance walk skips the reserved `versions/`
subtree (journal validated by `versions()` itself: parse + contiguity);
the plan's code was gate-verified in a scratch tree before freezing;
final-review hardening: archive/ subdir (manifest-collision-proof),
AssetInReservedTree guard, orphan version dirs skipped as uncommitted,
blank/control states rejected.

### Phase 13 — Attribution (`/req/core/attribute*`), optional — as built
- Attr1: `attribution.rs` — `AttributeModel { schema_uri, attributes }` /
  `AttributeDef { id, name, description }`; written by the facade to
  `global_metadata/vector_attributes.<declared-enc>` so Attr1-B (location)
  and Attr1-C (name) hold by construction; `parse_file_name`/`file_name_for`
  encode Attr1-C standalone — the name is matched LITERALLY (exact reserved
  stem, exact lowercase `json`/`xml` extension, anything else → error), so a
  name the crate blesses is always a name the facade can read; `naming`'s
  case-insensitive Name7 table is a media-type lookup for files the crate
  does not name, a different job.
- PAttr1: optional `schema_uri`, validated for RFC 3986 scheme shape plus a
  remainder with visible content, by a module-local check (no links-module
  dependency — the URI is not a Links-class link object). The inline minimum is ALWAYS required: the URI
  supplements, never replaces (doc-noted; the spec is silent).
- Attr2: unique id + name + description per attribute (the spec's
  StreetName/StreetType/StreetWidth table as fixture, verbatim including
  its stray paren); blank/control-char elements rejected (the versioning
  `EmptyState` posture); the empty model rejected (doc-noted strict
  reading). Ids are STRINGS — the inline-minimum rule means NAS/LCCS codes
  must project inline — with bare-integer JSON ids canonicalized in a
  `from_json_str` value pre-pass (quick-xml cannot drive untagged/`any`
  serde leniency). Both parse paths canonicalize leaf whitespace before
  validating and before the uniqueness check — quick-xml preserves element
  text verbatim, so without it a pretty-printed `vector_attributes.xml` was
  unreadable and whitespace could forge a "distinct" id; whitespace-only
  values still fail as blank. `validate()` compares ids trimmed, so the
  in-memory Attr2-B checker agrees with the read path and the facade can
  never write a file it would refuse to read back (re-review fix).
- Facade: `attribute_model()` → `Ok(None)` when absent, full validation on
  read; `write_attribute_model` validates first, writes the declared
  encoding only, and mirrors `write_global_metadata`'s Metadata5
  cross-encoding guard; a Gpkg-declared store is `UnsupportedEncoding`.
  Profile hook `ApplicationProfile::attribute_model()` defaults `None`
  (Attr1-A); `SimulationProfile` declares none. Deferred to 14b (uniform
  optional-class join): `RequirementsClass` membership + `CdbViolation`
  mapping, the profile-declaration cross-check, and **the Attr1-C name
  sweep** — `validate()` does not yet walk the top of `global_metadata/`
  passing `vector_attributes*` entries through `parse_file_name`, so
  `vector_attributes.txt` draws only a Name7-B warning and
  `vector_attributes.xsd` in an XML store draws nothing at all (the
  Metadata5 sweep skips unknown extensions and maps `xsd → Xml`); Attr1-C
  is the only check that can catch either.
- `tests/attribution_roundtrip.rs`: create → write → reopen → read equal →
  `validate()` conformant, JSON and XML.

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
  `None`, attribute-model declaration `None` (Attr1-A),
  `known_extensions()` empty (Name7-B vouching hook).
- Annex A: `tests/conformance_core.rs` mechanizes `/conf/minimal-core`
  (profile-declaration inspection) and operationalizes it — a full
  default-profile datastore passes all five mandatory classes with zero
  warnings, and break-one-class scenarios each report the matching violation
  under the right class.
- Round-trip at 14a is restricted to what exists: `tests/roundtrip_datastore.rs`
  covers global metadata (json and xml), storage CRS (incl. a dynamic-datum
  epoch via `COORDINATEMETADATA`), and resource metadata, all through the
  facade with a fresh reopen.

**14b (as built) — the last phase of the plan, shipped as `1.0.0`:**

| Item | Realization |
|---|---|
| The `conformance` module | Extracted from `datastore.rs` (2,794 → 1,807 lines): `class.rs` (`RequirementsClass`, moved out of `profiles` — conformance owns the vocabulary, profiles declare *from* it), `finding.rs` (`CdbViolation`/`CdbWarning` + the one auditable `code()` table), `report.rs` (`ClassFindings`/`ConformanceReport`/`ContentCoverage`), `validate/` (`mod.rs` the orchestrator, `stages.rs` the optional-class stages, `sweep.rs` the content sweep, `support.rs` the test profiles). All five types stay re-exported at the crate root; `CdbDatastore::validate` delegates. |
| Eleven requirements classes | `RequirementsClass` gains Attribution, Coverages, Geometry, Tiling, Topology, Versioning (alphabetical, because `Ord` order **is** the report's listing order); `MANDATORY` joined by `OPTIONAL` and `ALL`. `CdbWarning` gains `Coverage` and `Tiling` — the only two optional classes with SHOULD-level text. Topology, Versioning and Attribution deliberately have **no** warning variant. |
| Optional-class stages | One stage per declared class, each delegating to the owning module's free fn — `validate_coverage_instance`, `validate_tileset_metadata`, `validate_topology_dataset`, and the three added here: `geometry::validate_geometry_metadata`, `attribution::validate_attribute_model_document`, `versioning::validate_journal`. A declared class with no content **passes**, recorded separately via `ClassFindings::coverage` so a vacuous pass is never mistaken for a clean one. That field is a **three-state** `ContentCoverage` (`checked` / `none` / `unchecked`), not a boolean: the Geometry and Topology stages are structurally incapable of producing a finding — their subjects live in payloads the crate does not decode — so they mark content `unchecked`, and the report never claims a check that provably never ran. |
| The content sweep | Rides the existing walk (no third traversal): five signals — a `vector_attributes.*` entry, the `tilingScheme` element, a `versions/` journal, `windingOrder`, `domainSet` — each reported under its class as `CdbViolation::DeclarationMismatch` when undeclared, clause = the class's `requirements_uri()`. Signals come only from directory entries and parsed metadata, never payload bytes. Geometry is deliberately **not** swept (Geom4's `uom` is a unit declaration, too weak to convict). Closes the Phase 13 carry-forward: `attribution::parse_file_name` finally has a production caller, so `vector_attributes.xsd` in an XML store and `Vector_Attributes.json` stop being invisible. Two cross-checks ride along: Tiling4 (profile pin vs the on-disk element) and the attribute model, both gated on `Some` since both trait methods default to `None`. `GlobalMetadataBuilder::tiling_scheme` added so the element can be written at all. |
| The case stance | `naming::guard_eq` (ASCII-only, `pub(crate)`): **guards fold, requirements don't.** Six fold sites (reserved-tree guard, root-name match, `global_metadata/` detection, the `vector_attributes` stem signal, the `versions/` descent guard, `is_resource_metadata`'s `metadata/` component — the last two were not on the design spec's list); four deliberate non-fold sites (Name6's `CaseRule::matches`, `StyleGuide::is_reserved`, Attr1-C's `parse_file_name`, Attr2-B id uniqueness). Closes the item parked since Phase 12. |
| serde surfaces | `ConformanceReport` is `Serialize` in a designed (not derived) shape: class-keyed array in `Ord` order, every finding a flat `{code, class, severity, message}` record. `Deserialize` only where a round-trip is meaningful (`RequirementsClass`, the topology primitives). `TopoGraph`'s `Deserialize` replays the constructors, so a wire graph meets the same SHALLs an in-memory one does. |
| `GnosisProfile` | The simulation profile by composition with `TilingSchemeId::GnosisGlobalGrid` pinned — the second implementation the `ApplicationProfile` trait's premise always needed. |
| The full round-trip | `tests/full_conformance.rs`: create → tile payloads (opaque blobs) at real CDB1/GNOSIS tile addresses → an elevation coverage with a `domainSet` → an attribute model → two versioning collections → reopen → byte equality, value equality on every structure the crate owns, and all eleven classes conformant with zero warnings — under **both** shipped profiles — plus the restricted-profile case that gives the content sweep its integration coverage. |
| The public matrix | `docs/CONFORMANCE.md` (requirement → code → API → test, the errata list, the Tiling9/10 interpretation, the case stance, the honesty notes), guarded by `tests/conformance_matrix.rs`. Closes the §8 "public conformance matrix" ambition inside the crate. |
| `1.0.0` | The API surface freezes; the *content* of conformance findings deliberately does not (`src/lib.rs`). |

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

**Complete as of `v1.0.0`.** Order, all phases delivered in it:
1 Naming → 2 Hierarchy → 3 Links → 4 Media types → 5 Metadata → 6 CRS
→ 14a facade/conformance for the mandatory core → 7 Geometry → 8 Coverages →
9 Tiling+CDB1 → 10 GNOSIS → 11 Topology → 12 Versioning → 13 Attribution →
14b full conformance suite. Nothing follows it inside this crate: the §8
efforts are separate projects.

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
  Line/Rect/Triangle), and `CdbGeometry::validate_in` against a
  `GeometryContext` (Geom3/4 z/m-UoM presence, Geom5/6 foreign-CRS — a claim
  against an anonymous datastore CRS counts as foreign). `GeometryViolation` has six variants and no warning type
  (§7.6 has no SHOULDs); Geom5-B/6-B and the ZM / MultiPolygon-Z absence hold
  by construction (members carry no CRS, spec-absent combos unrepresentable).
  The Geom4 m-value UoM rides on the new `ResourceMetadata.uom`. Design
  record: the 2026-08-01 geometry design note.
- Phase 8 done (commits `phase-8(coverage)` ×4, `phase-8(metadata)`, after a
  `refactor(crs)` that extracted the shared `authority_ids_match` and a `docs`
  prep commit), 166 tests (153 unit + 12 integration + 1 doc) — the second
  optional class, shipped as `0.3.0`. `coverage`: `DomainSet` (the eight §7.2.6
  domainSet elements A–H with spec defaults — precision/scale 1, offset 0,
  value-is-center, field_type Height), `GridCellEncoding`/`GridCorner` (closed
  sets, spec wire spellings), `decode_value` making §7.2.6.2 executable
  (scale/offset touch values but never `data_null`), `DomainSet::validate`, and
  the free `validate_coverage_instance` fn (Coverages4/5/6, Geom5-style
  provable-match CRS rule via `crs::authority_ids_match`). `CoverageViolation`
  has nine variants and, with `CoverageWarning::UomLooksLikeUri` (the §7.2.6.1
  SHOULD), coverage is the first OPTIONAL class carrying warnings (geometry had
  violations only). The Coverages6-A `uom` is the third distinct uom — the
  three-uom table: `GlobalMetadata.uom` (M/FT/K/MI enum) / `ResourceMetadata.uom`
  (Geom4, same enum) / free-form UCUM-style `DomainSet.uom` string. Coverages5/6
  ride on the new `ResourceMetadata.domain_set` (wire `domainSet`, the second
  §7.9.4.2 conditional element after Geom4's `uom`); Coverages7/8 (tiled-coverage
  recommendations) defer to Phase 9. Design record: the 2026-08-02 coverages
  design note.
- Phase 9 done (commits `phase-9(tiling)` ×6 incl. one review fix,
  `phase-9(metadata)`, `phase-9(conformance)`), 183 tests (169 unit + 13
  integration + 1 doc) — the third optional phase, bringing two requirements
  classes (the abstract Tiling module §7.10 and the CDB1GlobalGrid extension
  §7.11), shipped as `0.4.0`. `tiling::mod` (§7.10): relocated `TilingSchemeId`
  (the closed two-extension set + `parse`; `profiles` re-exports it),
  `TilingScheme` (the `cdb1_global_grid` preset; `validate` enforces Tiling5
  provable-match CRS / Tiling6 CRS-unit uom / Tiling7 whole-earth, `warnings`
  carries Rec Tiling1, `require` enforces Tiling8), and the free
  `validate_tileset_metadata` (Tiling9/10, Keywords mandatory). `TilingViolation`
  has 11 variants (incl. the `f64`-bearing `CoordinateOutOfRange`, for which the
  enum forgoes `Eq`) plus one `TilingWarning`. `tiling::cdb1_grid` (§7.11): `Lod`
  (−10..=23), `TileAddress` (validated, u64, north-origin per 2DTMS), and
  `Cdb1GlobalGrid` matrix/raster sizes, the CDB 1.x zone table
  `[(50,1),(70,2),(75,3),(80,4),(89,6),(90,12)]`, coalescence, and
  `address`/`tile_at`/`tile_extent`/`parent`/`children` (see the binding
  half-open tile-addressing convention recorded in §4). Tiling8 rides the new
  conditional `GlobalMetadata.tiling_scheme` (wire `tilingScheme`) — the third
  §7.9.4.2-style conditional element and the first on the global table;
  embedding the `f64`-bearing `TilingScheme` (via `Bbox`) there also costs
  `GlobalMetadata` its `Eq` derive, following the `ResourceMetadata`/`domainSet`
  precedent. The
  Coverages Rec7/8 reservation closes via the `tests/tiled_coverage.rs`
  integration test. Design record: the 2026-08-02 tiling design note.
- Phase 10 done (commits `phase-10(tiling)` ×7), 192 tests (178 unit + 13
  integration + 1 doc) — the fourth optional phase, completing the tiling
  extensions, shipped as `0.5.0`. `tiling::gnosis_grid` (§7.12): the
  registered variable-width TMS as closed-form math (factor
  `2^max(0, n − bit_length(d))`, registry levels 0-3 pinned as fixtures),
  `GnosisLevel` (0..=28), `GnosisTileAddress` (+ 64-bit key pair), and
  `GnosisGlobalGrid` paralleling `Cdb1GlobalGrid`'s surface minus
  `raster_size` (256×256 constant, registry-pinned) with 3-way pole
  splitting emerging from uniform factor-driven child enumeration. The
  deferred rule-of-two (no trait) and naming (symmetric `Cdb1`/`Gnosis`
  prefixes; breaking) decisions are resolved per the 2026-08-26 GNOSIS-grid
  design note; the parked
  Tiling4 field-doc citation is fixed. Facade wiring of the tiling classes
  and any GNOSIS-declaring profile remain 14b scope.
- Phase 11 done (`phase-11(topology)` commits): Topo1–6 + Face1–4 in
  `topology.rs`, the `windingOrder` conditional element on
  `ResourceMetadata`, and `tests/topology_network.rs`; 226 tests
  (210 unit + 15 integration + 1 doc); tagged `v0.6.0`.
- Phase 12 done (`phase-12(versioning)` commits): V1–V6 in `versioning.rs`
  + the `CdbDatastore` apply/journal/rollback facade, the `versions`
  reserved name and walk carve-out, and
  `tests/versioning_roundtrip.rs`; 254 tests (236 unit + 17 integration
  + 1 doc); tagged `v0.7.0`.
- Phase 13 done (`phase-13(attribution)` commits): Attr1/PAttr1/Attr2 in
  `attribution.rs` + the `ApplicationProfile::attribute_model` hook and
  the `CdbDatastore` read/write facade, and
  `tests/attribution_roundtrip.rs`; final-review fixes: parse-time leaf
  whitespace canonicalization on both encodings (Attr2-B uniqueness hole +
  hand-authored XML), literal Attr1-C name matching, blank-URI remainder
  rejected, Gpkg facade coverage, crate-root re-exports;
  281 tests (261 unit + 19 integration + 1 doc); tagged `v0.8.0`.
- Phase 14b done (`phase-14b(conformance)` commits ×10): the `conformance`
  module extraction, eleven requirements classes with optional-class
  warnings, the six optional-class stages, the content sweep with the
  Attr1-C closure and the Tiling4/attribute-model cross-checks, the
  crate-wide case stance, serde on reports and topology primitives, the
  GNOSIS profile and the interior-whitespace URI tightening, then
  `tests/full_conformance.rs`, `tests/conformance_matrix.rs` and
  `docs/CONFORMANCE.md`, the `validate.rs` split into `validate/`, and the
  post-review fix wave, and a final fix wave over an adversarial oracle
  harness (3,000 randomized datastores) plus the whole-branch review;
  343 tests (317 unit + 25 integration + 1 doc);
  tagged `v1.0.0`. As-built notes: `CdbViolation` forgoes its `Eq` derive
  (`TilingViolation` carries `f64`); `ClassFindings` is
  `#[non_exhaustive]` and carries a three-state `ContentCoverage` rather
  than a `has_content` boolean, so the two stages that cannot fail report
  `unchecked` instead of claiming a check;
  `RequirementsClass::{MANDATORY,OPTIONAL,ALL}` are `&'static` slices, not
  fixed-size arrays, so a twelfth class stays a `1.x` change as the
  `#[non_exhaustive]` enum promises;
  `SimulationProfile::conformance_classes()` widened from `MANDATORY` to
  `ALL` (the truthful declaration — an under-declaring profile convicts its
  own datastores), which is why the sweep's integration coverage needs the
  deliberately narrow profile in `tests/full_conformance.rs`; finding codes
  are normalized absolute and hyphen-joined (the draft is inconsistent), must
  discriminate from the sweep's own `DeclarationMismatch`, and the nine codes
  with no draft slug are listed in `docs/CONFORMANCE.md` §5.1. The final wave
  made the sweep mark `unchecked` rather than `checked` (an undeclared class
  had no stage run), read the attribute model under either Attr1-C extension
  (the requirement is encoding-independent), canonicalize on write as the
  parse paths already did, sort each directory's entries so a report does not
  depend on `read_dir` order, and enable `serde_json`'s `float_roundtrip` so
  `DomainSet`'s four f64 elements are bit-exact in JSON as they were in XML.
- **The plan is complete.** Everything further is TDD_PLAN §8 territory:
  separate projects, each with its own brainstorm→spec→plan cycle.

## 8. Post-1.0 follow-on efforts (separate projects; architecture TBD)

Recorded 2026-08-30. These are chartered ambitions, NOT crate scope: none
enters a phase plan until explicitly kicked off, and each gets its own
brainstorm→spec→plan cycle (likely as sibling crates/tools rather than
additions to the core). Rationale: after 14b the crate is, to the best of
current knowledge, the first public OGC CDB 2.0 implementation and the only
executable conformance check for it — these efforts turn that position into
an ecosystem.

- ~~**Conformance CLI**~~ — **done, 2026-09-13/14**: shipped as the
  `cdb-lint` crate at `0.1.0`, a workspace sibling (the repository is now a
  two-crate workspace, and every gate is workspace-wide). It wraps
  `CdbDatastore::validate` in a command: two built-in profiles or a JSON
  profile descriptor as the yardstick, `text`/`json`/`sarif` reports, four CI
  exit codes (0 conformant, 1 findings, 2 usage, 3 operational), `cdb-lint
  explain` over an 82-row finding-code catalogue whose completeness is held
  against `src/conformance/` by a staleness guard, and `--baseline`
  ratcheting. Zero new dependencies entered the lockfile, and **nothing under
  `src/` changed** — the library kept its `v1.0.0` bytes, and the design spec
  makes any exception to that an explicit amendment rather than a quiet edit.
  Design: the 2026-09-13 cdb-lint design note; usage:
  `cdb-lint/README.md`. Its six honesty rules — chief among them that
  `unchecked` never renders as a green pass — descend from
  `docs/CONFORMANCE.md` §6 and are held across all three formats by
  `cdb-lint/tests/cli_honesty.rs`. As-built notes: the descriptor is JSON
  rather than TOML (no new dependency may parse TOML) and rejects unknown
  keys; `MetadataEncoding::Gpkg` parses but is refused at pre-flight with
  exit 2, because "this build does not implement that encoding" is a fact
  about the request and exit 3 means the tool could not look; SARIF
  synthesizes no `helpUri`, since the draft's one absolute requirement URI
  (Requirement Link1's box) drops the `req` segment its own class table
  carries, so no dereferenceable URL can be built from a finding code — a
  defect for the errata package below, not for cdb-lint. **Final review,
  same day**: a whole-branch review and the independent oracle harness
  (its sixth phase, sixth time finding real defects) surfaced fifteen
  findings — chief among them a descriptor `resource_metadata_dir` that
  could silence every record-level check into a false CONFORMANT — all
  fixed on `main` with regression tests, and the hand-built SARIF was
  validated against the published OASIS schema; the design spec's §15
  holds the record.
- ~~**Public conformance matrix**~~ — **done in 14b**: folded into the crate
  as `docs/CONFORMANCE.md`, guarded by `tests/conformance_matrix.rs`. The
  errata catalogue below can now be assembled from its §7 rather than from
  scattered doc comments.
- **Spec errata package to OGC** — consolidate the crate's doc-noted defect
  catalogue (duplicated URIs, /rec/-vs-/req/ mislabels, missing boxes, the
  §7.14.4 editorial TODO, typos) into change requests to the CDB SWG;
  positions the project as a contributor to the standard. Precede any
  public "first/only" positioning with a fresh landscape-verification
  sweep (post-Jan-2026 ecosystem check).
- **CDB 1.x reader + 1.x→2.0 migration tool** — the likely killer adoption
  feature (all fielded data is 1.x); a separate project with its own spec
  corpus (1.x volumes), NOT an extension of the 2.0 core modules. The
  technical bridge already exists in-crate: `Cdb1GlobalGrid` is the
  1.x-compatible tiling scheme (LOD/row/col ↔ geocell math match), so a
  migrator maps 1.x tile files onto identical 2.0 tile addresses and
  translates layout/metadata around them — the tiling itself round-trips.
- **GeoPackage metadata encoding** — lift the deliberate
  `MetadataEncoding::Gpkg` unsupported stance behind a feature gate
  (needs a sqlite/gpkg dependency; keep out of the core path).
- **Content codecs** — imagery/raster and model payload decode (feature-
  gated `gdal` or pure-Rust codecs) so "work with CDB" extends from
  structure/metadata into content; explicitly outside Part 1 Core scope.
- **Reprojection** — feature-gated `proj` only if a profile genuinely
  needs coordinate transformation (§2's standing rationale holds
  until then).
