# opencdb — OGC CDB 2.0 Core conformance matrix

**Crate version 1.0.0** · **Standard:** OGC CDB Version 2, Part 1: Core
(OGC 23-034, version 2.0) · **Audience:** implementers auditing this crate's
conformance claim.

This document maps every requirement of the CDB 2.0 Core to the API item that
implements it and the test that proves it. It is written by hand and kept
honest by two tests (§10) rather than generated, because generating it would
need a test-name registry nothing else in the crate wants.

It also records, in clearly marked sections, everything an auditor needs in
order to disagree with us on purpose rather than by accident: the draft
defects this crate normalized (§7), the one place it interprets a silence in
the spec (§8), the crate-wide case stance (§9), and what a "conformant"
verdict does **not** mean (§6).

---

## 1. How to read the matrix

Each class section is a table with four columns:

| Column | Meaning |
|---|---|
| **Requirement** | The spec's own label (Name6, Metadata5, Tiling8, V3, …) and its part letter where the box has one. |
| **Code** | The finding's stable machine-readable code — the clause in requirement-URI form, exactly what `CdbViolation::code()` / `CdbWarning::code()` return. A consumer keys on this and never parses display text. `—` means the requirement holds by construction and produces no finding. |
| **API** | The public item that implements or enforces it. |
| **Tests** | The test functions that prove it. Every name here exists in the crate; §10's guard fails the build otherwise. |

Run any cited test with `cargo test <name>`; run the whole suite with
`cargo test`. Unit tests live in the module they verify, integration tests in
`tests/`.

**SHALL vs SHOULD is carried by the finding kind, never by the code.** A
`CdbViolation` is a SHALL failure and decides conformance; a `CdbWarning` is a
SHOULD finding and never does. Four warnings carry a `/req/core/` code, for
three distinct reasons — none of them a promotion or demotion:

| Warning | Why its code says `/req/` |
|---|---|
| Name1-A (`/req/core/name-unicode-A`) | A SHOULD in a box **labelled Recommendation Name1** that the draft nonetheless prefixes `/req/` (errata row 19). The code reproduces the draft. |
| Rec Name4 (`/req/core/name-empty-folders-A`) | Same defect, same box family (errata row 19). |
| Name3-B (`/req/core/name-language-B`) | A SHOULD written as a lettered part *inside* a genuine **Requirement** box — the one case that really is that. |
| Name7-B (`/req/core/name-extensions-B`) | Not a SHOULD at all: a **SHALL** ("industry standard extensions SHALL be used") whose predicate no validator can decide. Reported as a warning a profile silences by vouching for the extension (`known_extensions`); the requirement's force is untouched. |

---

## 2. What "conformant" means here

1. **Conformance is judged against a profile declaration.** The CDB 2.0 Core
   is abstract and cannot be implemented directly (§5.1); only an application
   profile that pins the core's singularities can be. `CdbDatastore::validate`
   therefore takes the profile as its yardstick, and the same bytes may be
   conformant under one profile and not another.
2. **Five classes are mandatory** — Annex A bundles them as `/conf/minimal-core`
   — and six are optional, binding once the datastore holds the content they
   govern.
3. **A pass has three meanings, and the report says which.** Every listed
   class carries a `ContentCoverage` (`ConformanceReport::class_coverage`,
   serialized as the `content` field of a class entry) alongside its verdict:

   | `content` | Meaning |
   |---|---|
   | `checked` | the datastore held content this class governs, and it was judged |
   | `none` | the datastore held no such content — a declared class with nothing to check **passes** |
   | `unchecked` | content was present but this crate has no datastore-level check that could judge it |

   None of the three affects pass/fail. The third is not a gap awaiting work:
   it is the standing position of **Geometry and Topology**, whose subjects
   live inside payloads the crate deliberately does not decode — see §6. A
   consumer that reads `"passed": true` without reading `content` will
   mistake "we did not look" for "we looked and it was fine", which is the
   false green this document exists to prevent.
4. **Undeclared content is reported.** A content sweep reports content whose
   class the profile never declared as
   `CdbViolation::DeclarationMismatch`, filed under that class. A report
   therefore lists every class declared **plus** any class content betrayed.
5. **Findings are recorded, never raised.** A non-conformant datastore still
   yields `Ok(report)`; `Err` is reserved for operational I/O failure.

---

## 3. The eleven requirements classes

| Class | Short name | Requirements URI | Spec | Status |
|---|---|---|---|---|
| Attribution | `attribution` | `/req/core/attributes` | §7.1 | optional |
| Coverages | `coverages` | `/req/core/coverages-` | §7.2 | optional |
| CRS | `crs` | `/req/core/data-representation` | §7.3 | **mandatory** |
| File Naming | `file-naming` | `/req/core/naming-system` | §7.4 | **mandatory** |
| File Structure | `file-structure` | `/req/core/file-system` | §7.5 | **mandatory** |
| Geometry | `geometry` | `/req/core/geometry` | §7.6 | optional |
| Links | `links` | `/req/core/links` | §7.7 | **mandatory** |
| Metadata | `metadata` | `/req/core/metadata-` | §7.9 | **mandatory** |
| Tiling | `tiling` | `/req/core/tiling` | §7.10 (+ §7.11, §7.12 extensions) | optional |
| Topology | `topology` | `/req/core/topology` | §7.13 | optional |
| Versioning | `versioning` | `/req/core/versioning` | §7.14 | optional |

A profile's Annex A conformance-class URI is
`http://www.opengis.net/spec/CDB/2.0/conf/<profile>/<short name>`
(`RequirementsClass::conformance_uri`). Media Types (§7.8) is implemented as
the `media_types` module but is not a requirements class of its own — it has
no requirement boxes.

The enum is `#[non_exhaustive]`: the two grid extensions (§7.11, §7.12) are
plausible future classes.

---

## 4. The matrix

### 4.1 File Naming — `/req/core/naming-system` (§7.4, mandatory)

| Requirement | Code | API | Tests |
|---|---|---|---|
| Name1 — no blanks in any name | `/req/core/name-spaces` | `naming::validate_component`, `NamingViolation::ContainsSpace` | `req_core_name_spaces_rejects_space`, `req_core_name_spaces_rejects_tab_and_newline`, `req_core_name_spaces_accepts_separator_names` |
| Name1-A (SHOULD) — avoid unicode | `/req/core/name-unicode-A` | `naming::component_warnings`, `NamingWarning::NonAscii` | `rec_core_name_unicode_warns_on_non_ascii_but_validates` |
| Name1-B — forbidden characters | `/req/core/name-unicode-B` | `NamingViolation::ForbiddenCharacter`, `NamingViolation::ControlCharacter` | `req_core_name_unicode_rejects_each_forbidden_character`, `req_core_name_unicode_component_rejects_forward_slash` |
| Name3-A — one language per datastore | — | `StyleGuide::language`, `ApplicationProfile::language` | `req_core_metadata_language_derives_from_style_guide` |
| Name3-B (SHOULD) — English | `/req/core/name-language-B` | `CdbWarning::LanguageNotEnglish` | `rec_core_name_lang_non_english_warns` |
| Name5 — the profile's style guide | — | `ApplicationProfile::style_guide` (a required trait item), `StyleGuide` | `req_core_name_ap_guide_style_guide_enforces_case_and_structure`, `req_core_name_ap_guide_spec_mandated_names_are_exempt`, `req_core_name_ap_guide_simulation_style_guide`, `req_core_name_ap_guide_crate_persistence_names_reserved`, `req_core_name_ap_guide_resource_metadata_dir_folds_case` |
| Name6 — one case rule per datastore | `/req/core/name-case` | `CaseRule`, `CaseRule::matches`, `StyleGuide::validate_component` | `req_core_name_case_pascal`, `req_core_name_case_camel`, `req_core_name_case_snake_incl_all_caps_variant`, `req_core_name_case_kebab`, `req_core_name_case_digit_only_names_match_any_rule`, `req_core_name_case_classify`, `req_core_name_case_applies_to_stem_not_extension`, `req_core_name_case_display_uses_spec_spellings`, `req_core_name_case_mixed_datastore_detected`, `req_core_name_case_rule_is_not_folded`, `conf_core_break_naming_reports_case_rule_violation` |
| Name7 — the extension table | — | `naming::known_extension`, `naming::split_extension` | `req_core_name_extensions_spec_table_is_recognized` |
| Name7-B (SHALL, undecidable predicate) — industry-standard extensions | `/req/core/name-extensions-B` | `NamingWarning::NonSpecExtension`, `ApplicationProfile::known_extensions` | `req_core_name_extensions_unknown_extension_warns`, `req_core_name_extensions_simulation_vouches_for_wkt` |
| (floor) — a name is non-empty | `/req/core/naming-system` | `NamingViolation::EmptyName`, `NamingViolation::EmptyPathComponent` | `req_core_name_spaces_accepts_separator_names` |
| (root escape) — a path stays under the root | `/req/core/file-cdb-root-location` | `NamingViolation::PathTraversal` | `req_core_file_cdb_root_location_path_traversal_rejected` |

Two cross-class notes. A `NamingViolation` is always filed under File Naming,
including `PathTraversal`, whose *code* names Requirement File2's clause —
the class says who found it, the code says which clause it breaks.
Recommendation Name4 (empty folders) lives in §7.4 but is detected by the
hierarchy walk and is filed under File Structure; see §4.2.

### 4.2 File Structure — `/req/core/file-system` (§7.5, mandatory)

| Requirement | Code | API | Tests |
|---|---|---|---|
| File1 — folders and hierarchy | — | `DatastoreLayout::create_named`, `CdbDatastore::create` | `req_core_file_structure_supports_folder_hierarchy`, `req_core_file_structure_create_builds_root_metadata_and_crs` |
| File2 — one root, all content below it | `/req/core/file-cdb-root-location` | `DatastoreLayout::resolve`, `CdbDatastore::resolve` | `req_core_file_cdb_root_location_contains_all_content`, `req_core_file_cdb_root_location_rejects_escape` |
| File3 / PFile1 — physical resources, links permitted | — | `DatastoreLayout::open` (symlinks are neither followed nor recursed) | `req_core_file_hierarchy_open_requires_existing_directory_root`, `per_core_file_hierarchy_link_symlinked_subfolder_is_accessible` |
| File5 — hierarchy begins at `/` | — | `DatastoreLayout::resolve` logical paths | `req_core_file_hierarchy_root_logical_paths_begin_with_slash` |
| RFile1 (SHOULD) — root named `cdb` | `/rec/core/file-hierarchy-root-name` | `HierarchyWarning::RootNameNotCdb`, `ApplicationProfile::root_folder_name` | `rec_core_file_hierarchy_root_name_cdb_recommended`, `rec_core_file_hierarchy_root_name_match_folds_case`, `rec_core_file_hierarchy_profile_default_root_is_cdb` |
| File6 — `global_metadata/` at the root | `/req/core/file-root-global-metadata` | `HierarchyViolation::MissingGlobalMetadata`, `DatastoreLayout::global_metadata_dir` | `req_core_file_root_global_metadata_created_and_required`, `req_core_file_root_global_metadata_dir_guard_folds_case`, `conf_core_break_structure_reports_missing_global_metadata_dir` |
| Name4 (SHOULD) — avoid empty folders | `/req/core/name-empty-folders-A` | `HierarchyWarning::EmptyFolder` | `rec_core_name_empty_folders_warned` |

### 4.3 Links — `/req/core/links` (§7.7, mandatory)

| Requirement | Code | API | Tests |
|---|---|---|---|
| Link1 — `href` is a URL (absolute or relative) | `/req/core/link-href` | `Link::new`, `Link::kind`, `LinkViolation::InvalidHref` | `req_core_link_href_accepts_absolute_and_relative`, `req_core_link_href_rejects_malformed`, `conf_core_break_links_reports_invalid_association` |
| Link2 — `rel` is mandatory | `/req/core/link-rel` | `LinkViolation::MissingRel` | `req_core_link_rel_is_mandatory` |
| Link3 / Link4 — optional `type` and `title` | — | `Link::with_media_type`, `Link::with_title` | `rec_core_link_media_type_and_title_roundtrip`, `rec_core_link_optionals_omitted_when_absent` |

A `MetadataViolation::Link` — a bad association link inside a metadata record —
is re-filed under Links, because an association link has no class of its own.
It is the only cross-class normalization in the crate.

### 4.4 Metadata — `/req/core/metadata-` (§7.9, mandatory)

| Requirement | Code | API | Tests |
|---|---|---|---|
| Metadata1 / Metadata3 — a global record in `global_metadata/`, locatable | `/req/core/metadata-global` | `GlobalMetadata::read_from`, `GlobalMetadata::write_to`, `GlobalMetadata::locate` | `req_core_metadata_repository_json_roundtrip_and_locate`, `req_core_metadata_global_missing_file_errors`, `conf_core_break_metadata_reports_missing_global_record` |
| Metadata2 — one metadata standard | `/req/core/metadata-standard` | `MetadataStandard`, `ApplicationProfile::metadata_standard` | `req_core_metadata_standard_list_and_reject`, `req_core_metadata_standard_and_uom_pinned` |
| Metadata4 — one BCP 47 language | `/req/core/metadata-language` | `LanguageTag` | `req_core_metadata_language_bcp47` |
| Metadata5 — one encoding (xml/json/gpkg) | `/req/core/metadata-encoding` | `MetadataEncoding`, `metadata::encoding_violations`, `CdbDatastore::write_global_metadata` | `req_core_metadata_encoding_values`, `req_core_metadata_encoding_consistency`, `req_core_metadata_encoding_write_global_metadata_refuses_switch`, `req_core_metadata_encoding_write_attribute_model_refuses_switch`, `req_core_metadata_encoding_simulation_json_or_xml`, `req_core_metadata_encoding_gnosis_mirrors_simulation_convention`, `req_core_metadata_declaration_mismatch_detected` |
| Metadata6 — UTC, RFC 3339 §5.6 | `/req/core/metadata-datetime`, `/req/core/metadata-datetime-A` | `metadata::temporal` | `req_core_metadata_datetime_accepts_utc_forms`, `req_core_metadata_datetime_rejects_non_utc`, `req_core_metadata_datetime_rejects_malformed`, `req_core_metadata_datetime_formats_canonical_z`, `req_core_metadata_datetime_enforced_on_deserialize` |
| Metadata7 — temporal intervals (incl. half-bounded) | `/req/core/metadata-temporal-interval` | `metadata::temporal::Temporal` (its `Interval` arm), `metadata::temporal::parse_datetime` | `req_core_metadata_temporal_instant`, `req_core_metadata_temporal_bounded_interval`, `req_core_metadata_temporal_half_bounded`, `req_core_metadata_temporal_rejects_invalid` |
| Metadata8 — one unit of measure, element `uom` | `/req/core/metadata-uom-measure` | `UnitOfMeasure` | `req_core_metadata_uom_values`, `req_core_metadata_standard_and_uom_pinned` |
| §7.9.4.1 — global element table | `/req/core/metadata-` (`MissingElement`) | `GlobalMetadata`, `GlobalMetadataBuilder` | `req_core_metadata_global_builder_requires_mandatory`, `req_core_metadata_global_wire_names`, `req_core_metadata_malformed_record_is_named` |
| §7.9.4.2 — resource element table and its four conditional elements | `/req/core/metadata-` | `ResourceMetadata` (`uom`, `domainSet`, `windingOrder`; `tilingScheme` on the global record) | `req_core_geometry_mvalue_resource_uom_roundtrip`, `req_core_coverage_domainset_resource_roundtrip`, `req_core_topology_winding_resource_roundtrip`, `req_core_tiling_tilingscheme_global_roundtrip` |

The §7.9.4.2 conditional-element mechanism is how four optional classes make
themselves visible at the datastore level; it is also the only signal the
content sweep reads from a record (§2.4).

#### The global record on the wire

§7.9.4.1's element table is not the record's whole wire schema, and a reader
who builds from that table alone writes a record this crate cannot parse. The
canonical file is `global_metadata/global_metadata.json` (or `.xml`, per the
declared encoding), and the record carries **nine required elements**, spelled
exactly as follows:

| Element | Value |
|---|---|
| `ID` | any string — upper-case, as §7.9.4.1's table prints it |
| `title`, `description`, `contactPoint` | strings |
| `created` | RFC 3339 §5.6 in UTC; written canonically with `Z` (Metadata6) |
| `language` | a BCP 47 tag (Metadata4) |
| `metadataStandard` | one of the ten §7.9.3.2 keywords, e.g. `DCAT` (Metadata2) |
| `metadataEncoding` | `json`, `xml` or `gpkg` (Metadata5) |
| `uom` | `M`, `FT`, `K` or `MI` (Metadata8) |

`update`, `temporal`, `accessRights` and `license` are optional, and
`tilingScheme` — Tiling8's conditional element, an object of `id`, `crs`,
`uom` and `extent` (`west`/`south`/`east`/`north`) — appears on tiled
datastores.

The last four required rows are the metadata module's "one X per datastore"
declarations, present on the record itself: each must also **agree with the
profile's pin**, and a valid value that disagrees is convicted under that
requirement's own code. `metadataStandard` and `metadataEncoding` have no row
in §7.9.4.1's element table at all (erratum 22); `uom`'s element name is
mandated by Metadata8-B, but the table equally lacks its row. Two
consequences worth stating for implementers:

- **A record that does not parse draws one violation under
  `/req/core/metadata-encoding`,** naming the first failure — a missing
  required element, a non-UTC datetime, an unlisted standard keyword, a
  malformed language tag. The per-requirement codes
  (`/req/core/metadata-standard`, `/req/core/metadata-language`,
  `/req/core/metadata-uom-measure`, …) judge records that parse.
- Discovery by trial is one absence per validation run, because a parser
  reports only the first missing element. This table exists so nobody does
  that again: a cold second-implementer build (September 2026), working from
  the published documents alone, needed one validator round each to learn
  `metadataStandard` and `metadataEncoding` — neither of which any published
  document then named.

### 4.5 CRS — `/req/core/data-representation` (§7.3, mandatory)

| Requirement | Code | API | Tests |
|---|---|---|---|
| CRS2 — ISO 19111 consistency | — | type design; `crs::wkt2` accepts `GEOGCRS`/`GEODCRS`/`VERTCRS`/`COMPOUNDCRS` | `req_core_crs_metadata_spec_compound_example_verbatim` |
| CRS3 — exactly one CRS per datastore | `/req/core/crs/crsStorage` | `StorageCrs::write_to`, `CrsViolation::CrsAlreadyDefined` | `req_core_crs_storage_one_crs_per_datastore`, `req_core_crs_storage_declaration_mismatch_detected` |
| CRS4 — geodetic/geographic, never projected | `/req/core/crs/storageCrs-valid-value` | `StorageCrs::from_wkt`, `CrsViolation::NonGeodeticStorageCrs`, `CrsViolation::CompoundHorizontalNotGeodetic` | `req_core_crs_storage_valid_value_accepts_geographic`, `req_core_crs_storage_valid_value_rejects_non_geodetic`, `req_core_crs_storage_valid_value_simulation_crs_is_geographic` |
| `/rec/core/crs/crs-definition` (SHOULD) — WGS-84 | `/rec/core/crs/crs-definition` | `CrsWarning::NotWgs84` | `rec_core_crs_definition_wgs84_recommended`, `rec_core_crs_definition_simulation_pins_wgs84` |
| CRS5 — CRS metadata as WKT-2 in the global metadata folder, canonical file `global_metadata/crs.wkt` | `/req/core/crs/crsMetadata` | `StorageCrs::read_from`/`write_to`, `CrsViolation::MissingCrsMetadata`, `CrsViolation::InvalidWkt` | `req_core_crs_metadata_file_roundtrip`, `conf_core_break_crs_reports_missing_crs_metadata` |
| CRS6 — one coordinate unit | `/req/core/crs/uom` | `CrsViolation::InconsistentCoordinateUnits` | `req_core_crs_uom_mismatch_detected` |
| CRS7 — dynamic datum requires a decimal-year epoch | `/req/core/crs/crsEpoch`, `/req/core/crs/crsEpoch-B` | `crs::Epoch`, `CrsViolation::MissingEpoch`, `CrsViolation::InvalidEpoch` | `req_core_crs_epoch_dynamic_requires_epoch`, `req_core_crs_epoch_roundtrips_through_coordinatemetadata`, `req_core_crs_epoch_decimal_year_format` |
| VCRS1–3 — vertical CRS, WKT-2, metres by default | `/req/core/crs/vcrs-topic2` | `crs::vertical`, `CrsViolation::NotAVerticalCrs` | `req_core_crs_vcrs_parses_spec_example`, `req_core_crs_vcrs_units_default_to_meters`, `req_core_crs_vcrs_rejects_non_vertical` |

The CRS5 file name is part of the contract: the reader probes
`global_metadata/crs.wkt` and nothing else — the spec names the folder and
the encoding but no file, so this crate's spelling is the definition. A
`.wkt` file under any other name is ordinary content: the class draws
`/req/core/crs/crsMetadata` for the record that is not there, and unless the
stray stem happens to satisfy the case rule, Name6 convicts it too. (The
cold second-implementer build guessed `storage_crs.wkt` and lost both
classes at once; nothing published then named the file.)

### 4.6 Attribution — `/req/core/attributes` (§7.1, optional)

| Requirement | Code | API | Tests |
|---|---|---|---|
| Attr1-A — a profile implementing attribution specifies a model | `/req/core/attribute-model-A` | `ApplicationProfile::attribute_model`, `AttributionViolation::EmptyModel`, `AttributionViolation::Malformed` | `req_core_attribute_model_profile_default_none`, `req_core_attribute_model_profile_declares`, `req_core_attribute_model_content_empty_model_rejected` |
| Attr1-B — the model lives in `global_metadata/` | — | `CdbDatastore::write_attribute_model`, `CdbDatastore::attribute_model` | `req_core_attribute_model_facade_roundtrip_json`, `req_core_attribute_model_facade_roundtrip_xml`, `req_core_attribute_model_facade_absent_none`, `req_core_attribute_model_facade_write_is_canonical`, `req_core_attributes_roundtrip_json`, `req_core_attributes_roundtrip_xml` |
| Attr1-C — the file is named `vector_attributes.json` or `vector_attributes.xml`, literally (the requirement names both extensions and does **not** defer to the datastore's metadata encoding — a wrong-encoding model is Metadata5's finding, and is still read and judged as a model) | `/req/core/attribute-model-C` | `attribution::parse_file_name`, `attribution::file_name_for`, `AttributionViolation::InvalidFileName` | `req_core_attribute_model_file_name_rules`, `req_core_attribute_model_file_name_swept`, `req_core_attribute_model_mis_cased_stem_swept`, `req_core_attribution_stage_reads_model_of_either_extension` |
| Attr1 — the profile's model matches the datastore's (both sides compared **canonically**, and `write_attribute_model` writes canonically, so a datastore this crate produced is never convicted against the model it was written from) | `/req/core/attribute-model` | the content sweep's cross-check, `CdbDatastore::write_attribute_model` | `req_core_attribute_model_declaration_mismatch_detected`, `req_core_attribute_model_declaration_compared_canonically`, `req_core_attribute_model_facade_write_is_canonical` |
| Attr2-B — unique, non-blank ids | `/req/core/attribute-model-content-B` | `AttributeModel::validate`, `AttributionViolation::DuplicateId`, `AttributionViolation::EmptyId` | `req_core_attribute_model_content_duplicate_id_rejected`, `req_core_attribute_model_content_validate_canonicalizes_ids`, `req_core_attribute_model_content_whitespace_ids_are_not_distinct`, `req_core_attribute_model_content_numeric_id_message`, `req_core_attribute_model_content_integer_ids_parse` |
| Attr2-C / Attr2-D — name and description per attribute | `/req/core/attribute-model-content-C`, `/req/core/attribute-model-content-D` | `AttributeDef`, `AttributionViolation::EmptyName`, `AttributionViolation::EmptyDescription` | `req_core_attribute_model_content_fixture_validates`, `req_core_attribute_model_content_blank_elements_rejected` |
| PAttr1 (permission) — a supplementary external schema URI | `/per/core/attribute-schema-uri` | `AttributeModel::schema_uri`, `AttributionViolation::InvalidSchemaUri` | `per_core_attribute_schema_uri_shapes`, `per_core_attribute_schema_uri_rejects_interior_whitespace` |
| (document entry point) | — | `attribution::validate_attribute_model_document` | `req_core_attribute_model_document_entry_point_validates`, `req_core_attribution_stage_validates_model_document` |
| (encoding round-trips and write guards) | — | `AttributeModel::from_json_str`/`from_xml_str` | `req_core_attribute_model_json_roundtrip`, `req_core_attribute_model_xml_roundtrip`, `req_core_attribute_model_parse_validates`, `req_core_attribute_model_parse_canonicalizes_whitespace`, `req_core_attribute_model_parse_rejects_whitespace_only_values`, `req_core_attribute_model_facade_write_refuses_invalid`, `req_core_attribute_model_facade_write_refuses_whitespace_forged_ids`, `req_core_attribute_model_facade_invalid_file_errors`, `req_core_attribute_model_facade_gpkg_unsupported` |

### 4.7 Coverages — `/req/core/coverages-` (§7.2, optional)

| Requirement | Code | API | Tests |
|---|---|---|---|
| Coverages1–3 — module conformance | — | `coverage::validate_coverage_instance` + module-level conformance statements | `req_core_coverage_min_metadata_required` |
| Coverages4 — the coverage CRS is the datastore CRS | `/req/core/coverage-crs` | `CoverageViolation::CoverageCrsMismatch` | `req_core_coverage_crs_matches_datastore` |
| Coverages5 — resource metadata present | `/req/core/coverage-min-metadata` | `CoverageViolation::MissingResourceMetadata` | `req_core_coverage_min_metadata_required` |
| Coverages6-A — mandatory `uom` | `/req/core/coverage-domainSet-A` | `DomainSet::uom`, `CoverageViolation::EmptyUom` | `req_core_coverage_domainset_uom_mandatory` |
| Coverages6-B/C/D/E — precision, scale, offset, `data_null` | `/req/core/coverage-domainSet` | `DomainSet`, `DomainSet::decode_value` | `req_core_coverage_domainset_defaults`, `req_core_coverage_scale_offset_never_applies_to_data_null`, `req_core_coverage_domainset_floats_are_bit_exact_across_json_roundtrip` |
| Coverages6-F — grid cell encoding | `/req/core/coverage-domainSet-F` | `GridCellEncoding`, `GridCorner`, `CoverageViolation::CornerWithoutWhichCorner` | `req_core_coverage_domainset_corner_requires_which_corner` |
| Coverages6-H — quantity definition | `/req/core/coverage-domainSet-H` | `CoverageViolation::MissingQuantityDefinition` | `req_core_coverage_domainset_quantity_definition_conditional` |
| §7.2.6.1 (SHOULD) — a `uom` that is not a URI | `/req/core/coverage-domainSet-A` | `CoverageWarning::UomLooksLikeUri` | `rec_core_coverage_uom_uri_discouraged` |
| Coverages7 / Coverages8 (SHOULD) — tiled coverages follow the tiling module and one extension | — | integration | `rec_core_coverage_tiling_abstract_and_extension` |
| (record round-trip and validation stage) | — | `ResourceMetadata::domain_set` | `req_core_coverage_domainset_resource_roundtrip`, `req_core_coverages_stage_validates_domain_set_records` |

### 4.8 Geometry — `/req/core/geometry` (§7.6, optional)

**Read §6 before reading a "Geometry: conformant" verdict.**

| Requirement | Code | API | Tests |
|---|---|---|---|
| Geom1 — Simple Features conformance | — | `geo-types` is the model; `CdbGeometry` wraps it losslessly | `req_core_geometry_model_geo_types_identity` |
| Geom2 — type codes | `/req/core/geometry-types` | `GeometryCode`, the eight typed Z/M structs | `req_core_geometry_types_codes_match_geopackage_table`, `req_core_geometry_types_unknown_code_rejected`, `req_core_geometry_types_extension_codes_flagged`, `req_core_geometry_zvalue_length_invariants` |
| Geom3 — Z geometries need a z unit in global metadata | `/req/core/geometry-zvalue` | `GeometryViolation::MissingZUom`, `GeometryContext` | `req_core_geometry_zvalue_requires_global_uom` |
| Geom4 — M geometries need a unit in dataset metadata | `/req/core/geometry-mvalue` | `ResourceMetadata::uom`, `GeometryViolation::MissingMUom`, `geometry::validate_geometry_metadata` | `req_core_geometry_mvalue_requires_dataset_uom`, `req_core_geometry_mvalue_resource_uom_roundtrip`, `req_core_geometry_mvalue_record_entry_point_validates_metadata`, `req_core_geometry_stage_checks_geom4_uom_element_only` |
| Geom5 — every coordinate in the one datastore CRS | `/req/core/geometry-coordinates` | `CdbGeometry::validate_in`, `GeometryViolation::ForeignCrs` | `req_core_geometry_coordinates_foreign_crs_rejected` |
| Geom6 — one CRS across a GeometryCollection | — | by construction: members carry no CRS | `req_core_geometry_collection_srs_single_crs_by_construction` |

### 4.9 Tiling — `/req/core/tiling` (§7.10, optional; extensions §7.11 / §7.12)

| Requirement | Code | API | Tests |
|---|---|---|---|
| Tiling1–3 — tiled content follows this module | — | `TilingScheme::validate`, `validate_tileset_metadata` + module conformance statements | `req_core_tiling_tilingscheme_crs_matches_storage` |
| Tiling4 — one scheme definition datastore-wide | `/req/core/tiling-tilingscheme-consistent` | `ApplicationProfile::tiling_scheme` vs the on-disk element (content-sweep cross-check) | `req_core_tiling_scheme_declaration_mismatch_detected`, `req_core_tiling_scheme_consistent_gnosis_profile_judges_by_its_pin` |
| Tiling5 — scheme CRS matches the storage CRS | `/req/core/tiling-tilingscheme-crs` | `TilingScheme::validate`, `TilingViolation::SchemeCrsMismatch` | `req_core_tiling_tilingscheme_crs_matches_storage` |
| Tiling6 — scheme uom is the CRS coordinate unit | `/req/core/tiling-tilingscheme-uom` | `TilingViolation::SchemeUomMismatch` | `req_core_tiling_tilingscheme_uom_from_crs` |
| Tiling7 — the extent covers the whole earth | `/req/core/tiling-tilingscheme-extent` | `TilingViolation::IncompleteExtent` | `req_core_tiling_tilingscheme_extent_whole_earth` |
| Tiling8 — the definition lives in global metadata | `/req/core/tiling-tilingscheme-definition` | `TilingScheme::require`, `GlobalMetadata::tiling_scheme`, `GlobalMetadataBuilder::tiling_scheme` | `req_core_tiling_tilingscheme_required_when_tiled`, `req_core_tiling_tilingscheme_global_roundtrip`, `req_core_tiling_tilingscheme_builder_sets_element` |
| Tiling9 / Tiling10 — tileset metadata ⊇ {ID, Title, Description, Keywords} | `/req/core/tiling-tileset-metadata-elements` | `validate_tileset_metadata`, `TilingViolation::MissingTilesetKeywords` | `req_core_tiling_tileset_metadata_elements`, `req_core_tiling_stage_validates_scheme_and_tileset_metadata` |
| Rec Tiling1 (SHOULD) — use one of the two extensions | `/rec/core/tiling-extension` | `TilingWarning::NonExtensionScheme`, `TilingViolation::UnknownTilingScheme` | `rec_core_tiling_extension_scheme_recommended` |
| TCE1–3 (§7.11) — CDB1GlobalGrid, EPSG:4326, lat/lon order | `/req/core/tiling-extension-tms` | `TilingScheme::cdb1_global_grid`, `Cdb1GlobalGrid` | `req_core_tiling_ext_lod0_geocells` |
| TCE4 (§7.11) — degrees, coordinates validated | `/req/core/tiling-extension-uom` | `TilingViolation::CoordinateOutOfRange` | `req_core_tiling_ext_coordinate_validation` |
| TCE6 / TCE7 (§7.11) — LoD −10..=23, geocells, quad subdivision, raster sizes | `/req/core/tiling-extension-tile-tessellate`, `/req/core/tiling-extension-tile-tessellate-A` | `Cdb1Lod`, `Cdb1TileAddress`, `Cdb1GlobalGrid::parent`/`children`/`tile_at`/`tile_extent` | `req_core_tiling_ext_lod_range`, `req_core_tiling_ext_raster_sizes`, `req_core_tiling_ext_subdivision`, `req_core_tiling_ext_zone_coalescence` |
| TCE1–4 (§7.12) — GNOSISGlobalGrid, registered variable-width TMS | `/req/core/tiling-extension-tms` | `TilingScheme::gnosis_global_grid`, `GnosisGlobalGrid`, `TilingViolation::GnosisLevelOutOfRange` | `req_core_tiling_ext_gnosis_level_range`, `req_core_tiling_ext_gnosis_registry_fixtures`, `req_core_tiling_ext_gnosis_crs_uom_coordinates`, `req_core_tiling_ext_gnosis_coalescence_readjusts` |
| TCE6 (§7.12) — level 0 is 2×4, splitting per the 2DTMS annexes | `/req/core/tiling-extension-start-lod` | `GnosisLevel`, `GnosisGlobalGrid::children`, `GnosisTileAddress` | `req_core_tiling_ext_gnosis_level0_start`, `req_core_tiling_ext_gnosis_splitting`, `req_core_tiling_ext_gnosis_four_pole_tiles` |
| §7.12.2 — the 64-bit tile key | `/req/core/tiling-extension-tms` | `GnosisTileAddress::key`, `GnosisTileAddress::from_key` | `req_core_tiling_ext_gnosis_key_roundtrip` |

The two grid extensions are implemented as parallel surfaces by convention,
deliberately without a shared trait; neither is a requirements class of its
own at 1.0.

### 4.10 Topology — `/req/core/topology` (§7.13, optional)

**Read §6 before reading a "Topology: conformant" verdict.** Like Geometry,
this class reports `content: "unchecked"` when a datastore carries topology:
the graph lives in a payload the crate does not decode, so the items below
are real API and real tests, but the *datastore-level stage* judges almost
nothing.

| Requirement | Code | API | Tests |
|---|---|---|---|
| Topo1 — the ISO 19107 model | `/req/core/topology` | `TopoNode`, `TopoEdge`, `TopoFace`, `SignedEdge`, `DirectedEdge`, `DirectedNode` | `req_core_topology_topic1_primitives_are_coordinate_free` |
| Topo2 — unique node ids | `/req/core/topology-nodeID` | `TopoGraph::insert_node`, `TopologyViolation::DuplicateNodeId` | `req_core_topology_nodeid_duplicate_insert_rejected` |
| Topo3 — unique edge ids | `/req/core/topology-edgeID` | `TopoGraph::insert_edge`, `TopologyViolation::DuplicateEdgeId` | `req_core_topology_edgeid_duplicate_insert_rejected` |
| Topo4 — directed node: signed edge ids | `/req/core/topology-edge-dir` | `TopoGraph` adjacency, `NodeSign`, `SignedEdge` | `req_core_topology_node_dir_sign_rendering`, `req_core_topology_node_dir_signs_after_linking`, `req_core_topology_node_dir_isolated_node_has_empty_view` |
| Topo5 — directed edge stores its endpoints | `/req/core/topology-edge-dir` | `TopoEdge`, `TopologyViolation::UnknownNodeId` | `req_core_topology_edge_dir_endpoints_must_exist`, `req_core_topology_edge_dir_loop_and_geometryless_edges_legal` |
| Topo6 — clip at tile boundaries, one shared clip node | `/req/core/topology-clip` | `TopoGraph::clip_edge_to_tile`, `EdgeClipOutcome` | `req_core_topology_clip_two_tile_edge_shares_one_clip_node`, `req_core_topology_clip_non_crossing_edge_untouched`, `req_core_topology_clip_preconditions_rejected`, `req_core_topology_clip_edge_bounding_a_face_rejected`, `req_core_topology_clip_endpoint_corner_touch_is_not_a_crossing`, `req_core_topology_clip_outside_graze_through_corner_mints_nothing`, `req_core_topology_clip_vertex_on_boundary_crossing_mints_at_vertex`, `req_core_topology_clip_collinear_boundary_run_stays_inside`, `req_core_topology_clip_zigzag_multi_crossing_alternates_parts`, `req_core_topology_clip_gnosis_extent_grid_agnostic`, `req_core_topology_clip_oracle_geo_line_intersection_agrees`, `req_core_topology_clip_boundary_vertex_from_outside_is_a_touch`, `req_core_topology_clip_rejects_non_finite_polyline`, `req_core_topology_clip_id_headroom_guard_is_pre_mutation`, `req_core_topology_clip_endpoint_lands_on_boundary_from_outside`, `req_core_topology_clip_seeded_property_mini_sweep`, `req_core_topology_clip_network_across_two_cdb1_tiles` |
| Face1 — unique face ids | `/req/core/topology-faceID` | `TopoGraph::insert_face`, `TopologyViolation::DuplicateFaceId` | `req_core_topology_faceid_duplicate_insert_rejected` |
| Face2 / Face3 — a face is a chained, closed boundary | `/req/core/topology-face-structure` | `TopoFace`, `TopoFace::directed_nodes` | `req_core_topology_face_structure_ring_chains_and_closes`, `req_core_topology_face_structure_rejects_broken_rings`, `req_core_topology_face_structure_rejects_unknown_edge` |
| Face4 — the winding order is declared in metadata | `/req/core/topology-winding` | `WindingOrder`, `ResourceMetadata::winding_order`, `validate_topology_dataset` | `req_core_topology_face_winding_wire_spellings`, `req_core_topology_face_winding_required_when_faces_exist`, `req_core_topology_winding_resource_roundtrip`, `req_core_topology_face_winding_dataset_roundtrip`, `req_core_topology_stage_validates_winding_order_records` |
| (serde surface) | — | `Serialize`/`Deserialize` on the primitives and `TopoGraph` | `req_core_topology_primitives_serde_round_trip`, `req_core_topology_graph_deserialize_enforces_invariants`, `req_core_topology_winding_serde_uses_wire_spelling` |

§7.13 contains no SHOULD-level text, so this class has **no warning type** —
a decision reaffirmed in three consecutive phases, not an oversight.

### 4.11 Versioning — `/req/core/versioning` (§7.14, optional)

| Requirement | Code | API | Tests |
|---|---|---|---|
| V1 — apply and track changes | `/req/core/versioning` | `CdbDatastore::apply_collection`, `CdbDatastore::apply_collection_at`, `VersioningViolation::InvalidAssetPath`, `VersioningViolation::AssetInReservedTree` | `req_core_versioning_apply_preconditions_pre_mutation`, `req_core_versioning_reserved_tree_assets_rejected`, `req_core_versioning_reserved_tree_mis_cased_assets_rejected`, `req_core_versioning_reserved_tree_guard_folds_case` |
| V2 — the versioning collection | `/req/core/versioning-collection` | `PendingCollection`, `CollectionManifest`, `CollectionId`, `versioning::validate_journal`, `CdbDatastore::versions` | `req_core_versioning_collection_id_format_and_cap`, `req_core_versioning_collection_requires_changes`, `req_core_versioning_collection_rejects_duplicate_asset`, `req_core_versioning_collection_rejects_empty_state`, `req_core_versioning_collection_rejects_invalid_asset_path`, `req_core_versioning_manifest_json_roundtrip`, `req_core_versioning_manifest_xml_roundtrip`, `req_core_versioning_journal_sequence_contiguous`, `req_core_versioning_versions_contiguity_and_strays`, `req_core_versioning_orphan_version_dir_skipped_and_reused`, `req_core_versioning_stage_checks_journal_integrity` |
| V3 — metadata updates on apply (global `update`, record `updated`) | `/req/core/versioning-metadata-C` | the apply pipeline, `ChangeRecord::resource_record`, `VersioningViolation::ResourceRecordMissing` | `req_core_versioning_metadata_timestamps_three_way`, `req_core_versioning_builder_attaches_record_to_last_change` |
| V4 — create / delete / update assets | `/req/core/versioning-functions`, `/req/core/versioning-functions-A` | `PendingCollection::create`/`replace`/`delete`, `VersioningViolation::AssetAlreadyExists`, `VersioningViolation::AssetMissing` | `req_core_versioning_functions_create_asset`, `req_core_versioning_functions_delete_asset`, `req_core_versioning_change_action_wire_spellings` |
| V5 — file replacement, byte-faithful | `/req/core/versioning-collection` | the archive step of the apply pipeline | `req_core_versioning_file_replacement_byte_faithful`, `req_core_versioning_root_manifest_asset_archives_safely` |
| V6 — capture state changes | `/req/core/versioning-transitory` | `PendingCollection::set_state`/`clear_state`, `CdbDatastore::state_of`, `versioning::state_from_manifests`, `VersioningViolation::EmptyState`, `VersioningViolation::AssetStateMissing` | `req_core_versioning_transitory_set_and_clear_state`, `req_core_versioning_state_from_manifests_timeline` |
| §7.14 intro — rollback (beyond the boxes) | `/req/core/versioning` | `CdbDatastore::rollback_collection[_at]`, `CdbDatastore::rollback_to[_at]`, `InverseOp`, `VersioningViolation::NotLatestCollection`, `VersioningViolation::UnknownCollection` | `req_core_versioning_inverse_ops_crud`, `req_core_versioning_inverse_ops_states`, `req_core_versioning_rollback_latest_only`, `req_core_versioning_rollback_collection_restores_bytes`, `req_core_versioning_rollback_to_restores_point` |
| (the journal is not user content) | — | the conformance walk skips the reserved `versions/` subtree, and its one journal guard answers for both the Versioning stage and the content sweep | `req_core_versioning_journal_dir_guard_folds_case`, `req_core_versioning_journal_guard_agrees_across_stage_and_sweep` |

§7.14 contains no SHOULD-level text, so this class has **no warning type**.

### 4.12 Annex A — `/conf/minimal-core` and the conformance machinery

| Duty | Code | API | Tests |
|---|---|---|---|
| A profile declares its conformance classes; the mandatory five are required | `/conf/minimal-core` | `ApplicationProfile::conformance_classes`, `CdbViolation::MissingConformanceDeclaration` | `conf_core_minimal_mandatory_classes_enumerated`, `conf_core_conformance_uri_pattern`, `conf_core_minimal_profile_declaration_inspection`, `conf_core_minimal_missing_declaration_fails_class`, `conf_core_minimal_simulation_declares_all_mandatory`, `conf_core_minimal_gnosis_declares_all_classes`, `req_core_conformance_eleven_classes_partitioned`, `req_core_conformance_optional_class_uris` |
| The validator runs one stage per declared class | — | `conformance::validate`, `CdbDatastore::validate` | `conf_core_minimal_full_datastore_all_mandatory_classes_pass`, `conf_core_declared_optional_class_with_no_content_passes` |
| Undeclared content is reported under its class, and marked `unchecked` — the class had no stage run, so nothing judged the content | the class's requirements URI | the content sweep, `CdbViolation::DeclarationMismatch`, `ContentCoverage::Unchecked` | `conf_core_sweep_reports_undeclared_content`, `req_core_conformance_restricted_profile_reports_declaration_mismatch`, `conf_core_sweep_records_content_without_claiming_a_check` |
| A report is a function of the datastore, not of the host filesystem: the walk visits each directory in name order | — | `conformance::validate`'s naming walk | `conf_core_walk_visits_entries_in_name_order` |
| Findings carry class, severity, and a stable code | — | `CdbViolation::class`/`code`, `CdbWarning::class`/`code`, `ClassFindings`, `ConformanceReport` | `req_core_conformance_violation_codes_are_stable_uris`, `req_core_conformance_warning_codes_are_stable_uris`, `req_core_conformance_optional_class_violations_wrap`, `req_core_conformance_optional_class_warnings_wrap` |
| A report is a serde surface | — | `Serialize` on `ConformanceReport`, `ClassFindings`, both finding kinds; `RequirementsClass` round-trips | `req_core_conformance_report_serializes_by_class`, `req_core_conformance_finding_serializes_flat`, `req_core_conformance_class_token_round_trips` |
| Profiles vary; the facade is driven by the declaration | — | `ApplicationProfile`, `SimulationProfile`, `GnosisProfile` | `req_core_conformance_full_roundtrip_simulation_profile`, `req_core_conformance_full_roundtrip_gnosis_profile`, `req_core_tiling_extension_tms_gnosis_profile_pins_gnosis_grid` |
| The matrix itself stays honest | — | `docs/CONFORMANCE.md` | `req_core_conformance_matrix_lists_every_requirements_class`, `req_core_conformance_matrix_cites_only_real_tests` |

---

## 5. Finding codes

Every violation and warning carries a stable code — the clause it is about,
in requirement-URI form. Consumers key on the code and never on display text,
which carries values and phrasing the crate is free to improve. The whole
vocabulary lives in one auditable table in `src/conformance/finding.rs`, as
exhaustive matches with no wildcard arm, so a new variant cannot ship without
a code.

Three normalizations make every code a single whitespace-free token:

1. **Always absolute** — `/req/core/…` for a requirement box, `/rec/core/…`
   for a recommendation box, `/per/core/…` for a permission box, and
   `/conf/minimal-core` for Annex A's bundle. The draft is inconsistent: the
   requirement boxes of §7.11, §7.12 and §7.14 drop the leading solidus that
   their own class tables use, and Tiling9 does the reverse (see errata row
   5). One form is used here.

   **The prefix follows the box's own URI, not its label** — that is what
   "keyed on meaning" means here, and it has exactly two exceptions, both in
   §7.4: Recommendation Name1 and Recommendation Name4 are labelled
   recommendations but carry `/req/` URIs (errata row 19). Their codes
   reproduce what the draft wrote, so this crate emits `/req/core/name-unicode-A`
   and `/req/core/name-empty-folders-A` as **warnings**. Inventing a `/rec/`
   spelling for them would make the code unfindable in the document it names.
   The `code` and the finding's severity are independent, by design (§1).

   Only the *code* is normalized. A finding's **display text** may quote a
   box's own spelling, including a missing solidus, because prose is free to
   cite the draft as written; consumers key on `code` and never on text.
2. **A part letter joins with a hyphen** — the draft puts a part's letter in
   its own table column, so `/req/core/attribute-model-content` + `B` is not
   one token anywhere in the document; this crate writes
   `/req/core/attribute-model-content-B` (see errata row 6).
3. **The code names the clause, not the severity** (see §1).
4. **A code discriminates.** No violation carries the bare
   requirements-module URI of a class the content sweep can convict
   (Attribution, Coverages, Tiling, Topology, Versioning): that string is
   what an undeclared-content `DeclarationMismatch` cites, and a consumer
   keying on `code` has to be able to tell "this datastore holds undeclared
   versioning content" from "this collection addressed a reserved tree".
   `req_core_conformance_finding_codes_discriminate_from_sweep` enforces it.

**Many-to-one is expected.** A code identifies a clause, and one clause is
breakable in several ways: `DuplicateId` and `EmptyId` are both
`/req/core/attribute-model-content-B`.

### 5.1 Codes this crate had to judge

Nine findings have no slug to read off the draft. Each is recorded here so
an auditor sees the reasoning rather than guessing at it:

| Finding | Code | Why |
|---|---|---|
| `NamingViolation::EmptyName`, `NamingViolation::EmptyPathComponent` | `/req/core/naming-system` | No box of its own; an empty name is the naming system's floor. |
| `NamingViolation::ControlCharacter` | `/req/core/name-unicode-B` | The crate's own portability extension of Name1-B's character rule; the draft does not spell control characters out. |
| `MetadataViolation::MissingElement` | `/req/core/metadata-` | The §7.9.4 element tables carry no per-element box; the module URI is the honest answer. |
| `TopologyViolation::UnknownEdgeId` | `/req/core/topology-edgeID` | Raised from both the clip (§7.13.4.7) and the face-boundary (§7.13.5.4) paths, so it names neither: Topo3 is the clause that makes an edge identifier mean something. It may not be the bare `/req/core/topology`, per normalization 4. |
| `VersioningViolation::AssetMissing` | `/req/core/versioning-functions` | V4-B (delete) and V4-C (update) both reach it, so no part letter. |
| `VersioningViolation::InvalidAssetPath`, `VersioningViolation::AssetInReservedTree` | `/req/core/versioning-A` | V1's own box URI collides with the versioning class URI (errata row 9), so the part letter is what keeps `code` a discriminator (normalization 4). |
| `VersioningViolation::UnknownCollection`, `VersioningViolation::NotLatestCollection` | `/req/core/versioning-collection` | Rollback preconditions: both are about *which* collection is addressed, which is Requirement V2's subject. The draft has no box for rollback (§7.14 intro only). |
| `CoverageWarning::UomLooksLikeUri` | `/req/core/coverage-domainSet-A` | §7.2.6.1's disconnected-environment SHOULD is prose under the `uom` element's own requirement part; it has no box. |
| `TilingViolation::UnknownTilingScheme` | `/rec/core/tiling-extension` | A vocabulary rejection against the set that recommendation closes. |

---

## 6. Honesty notes — what a conformant verdict does not mean

**These notes are also machine-readable.** Everything below about Geometry
and Topology is carried in the report itself as
`ContentCoverage::Unchecked` — `content: "unchecked"` on the wire, `[PASS]
geometry (content not checked)` in the rendered text (§2 item 3). A tool that
keys on `content` does not have to have read this section to avoid the false
green; a human reading only `passed` does.

**The crate does not decode payloads.** GeoPackage containers, raster files,
and model files are opaque bytes it stores and returns verbatim (`proj` and
`gdal` are deliberately absent). Every content signal it reads is a directory
entry or a parsed metadata element — never a payload byte.

**Geometry's stage cannot fail.** Requirements Geom1–Geom6 govern geometry
*instances*, which live in those payloads. The Geometry stage of `validate`
reaches only the Geom4 `uom` conditional element, and every record reaching
it was already validated at parse time — so the stage re-checks a record
known to be valid and can produce no finding at all. "Geometry: conformant"
therefore means **nothing was inspected**, and the report says exactly that
(`content: "unchecked"`).
Instance-level validation remains an API-boundary duty: a caller holding a
decoded geometry calls `CdbGeometry::validate_in` against a
`GeometryContext`, and that is where Geom2/Geom3/Geom5/Geom6 actually bite.
Geom4 is unenforceable at the datastore level in *both* directions: the crate
can see neither the m coordinates nor their absence, so it can neither
require the `uom` nor forbid it.

**Topology's stage validates against an empty graph, and so cannot fail
either.** A topological graph lives in the dataset payload, so Face4's
face-count arm ("once the graph contains faces, the record SHALL declare a
`windingOrder`") cannot fire from the datastore level. It stays an
API-boundary duty for a caller holding a decoded `TopoGraph`. What the stage
does check holds by construction — a record that declares a winding order
declares a valid one, since `WindingOrder` is a closed enum the parse already
rejected bad values for — so this class too reports `content: "unchecked"`
whenever a datastore carries topology.

**Coverages4 is judged as association-via-datastore.** A per-instance CRS
claim lives in the coverage payload, so the stage passes `source_crs` as
`None` — the normal case, which Coverages4 permits. A coverage claiming a
*different* CRS can only be caught by the caller, through
`coverage::validate_coverage_instance` directly.

**Geometry is never swept.** The content sweep convicts a profile of failing
to declare a class only on the five signals of §2.4. Geometry is deliberately
absent from that list even though its stage has a content marker: the Geom4
`uom` declares a *unit*, conditional on m coordinates the crate cannot see,
which is too weak to convict a profile. The asymmetry is intentional — `uom`
is a content marker for the stage and never a conviction for the sweep — and
`req_core_conformance_restricted_profile_reports_declaration_mismatch` pins it.

**An undeclared class that the sweep convicts reports `unchecked`, not
`checked`.** Declaration drives which classes get a stage (§2 item 1), so a
class the profile never declared had *no* stage run: the sweep sees the
content — a `vector_attributes.*` entry, a `tilingScheme`, a `versions/`
journal, a `windingOrder` or a `domainSet` — and files the
`DeclarationMismatch`, but nothing of this crate's judged that content. The
report says so. A datastore must never report *more* checking because its
profile declared *less*.

**Round-trip fidelity: exact, with one documented exception.** Everything this
crate owns — metadata records, the attribute model, versioning manifests and
archived payload bytes — survives write → reopen → read unchanged, in both
encodings, and `tests/full_conformance.rs` asserts it. Two details are worth
recording:

- `DomainSet`'s `precision`, `scale`, `offset` and `data_null`
  (Coverages6-B/C/D/E) are **bit-exact** across a JSON round trip. That
  required enabling `serde_json`'s `float_roundtrip` feature: the default
  parser is a fast approximation that drifts one ULP on roughly 30 % of
  random `f64` values, and `DomainSet::decode` is `raw * scale + offset`, so a
  drifted `scale` would silently change every decoded coverage value.
  `req_core_coverage_domainset_floats_are_bit_exact_across_json_roundtrip`
  pins it. XML was always exact.
- **The XML encoding normalizes `\r` to `\n`** inside element text, so a
  metadata description or keyword containing a carriage return comes back with
  a line feed in an XML datastore and unchanged in a JSON one. This is not a
  defect to fix: XML 1.0 §2.11 *requires* a parser to perform that
  normalization, so no conformant XML writer/reader pair can preserve a bare
  `\r`. A profile that needs byte-exact control characters in metadata text
  should choose the JSON encoding.

**The semver guarantee covers the API, not the findings.** See
`src/lib.rs`'s crate documentation: 1.0 freezes the public API surface, not
the *content* of a conformance report. A future spec erratum may change what a
validator reports — or this crate may revise an interpretation recorded in
§7 or §8 — without that being a breaking API change.

---

## 7. Errata — draft defects normalized keyed on meaning

The published CDB 2.0 draft carries defects that an implementation cannot
simply reproduce. This crate's standing convention is to **normalize keyed on
meaning** and to record every such decision. This list is the consolidated
catalogue; it is also the seed of the errata package this project intends to
file with the OGC CDB SWG.

| # | Where | Defect | What this crate does |
|---|---|---|---|
| 1 | §7.10.2, §7.11.3, §7.12.3 | The `/req/core/geometry-` copy-paste from §7.6 appears in **three** requirements-class boxes, not one: Tiling-Abstract (spec line 1817), the CDB1GlobalGrid extension (line 1969) and the GNOSISGlobalGrid extension (line 2079) all label themselves with it. Beyond the collision with the Geometry class, it leaves the **two grid extensions indistinguishable from each other by class URI** — which matters directly to §3's `#[non_exhaustive]` note, where those two are named as the likeliest future `RequirementsClass` variants. | Uses `/req/core/tiling` for the abstract class; the two extensions are represented inside it (§4.9) rather than as classes of their own, so no URI has to be invented for them. |
| 2 | §7.1.2.1 vs §7.4.8, §7.9.3.5 | Requirement Name7's extension table admits `*.xsd` for XML and Metadata5 admits a `gpkg` datastore encoding, while Attr1-C fixes the attribute model's name to `vector_attributes.<ext>` "where `<ext>` is either xml or json". Nothing says which governs `vector_attributes.xsd`, or what an all-`gpkg` datastore's attribute model is called. | Lets both speak: the encoding sweep stays silent, Attr1-C convicts. Without this, an XML datastore could carry an attribute model nothing would ever load and still look clean. The reading is that **Attr1-C is encoding-independent** — it names both extensions and defers to nothing — so `vector_attributes.json` in an XML datastore is a valid Attr1-C name that Metadata5 convicts as the wrong *encoding* while the Attribution stage still reads and judges the model. A `gpkg`-encoded datastore declares no model name of its own (`attribution::file_name_for` yields `None`) and so has no attribution content unless one of the two core spellings is physically present, in which case the same split applies. |
| 3 | §7.2.2, §7.9 | The Coverages and Metadata module URIs carry a trailing hyphen (`/req/core/coverages-`, `/req/core/metadata-`). | Reproduced verbatim — a trailing hyphen is ugly, not ambiguous. |
| 4 | §7.6.3 | Geom3's slug is inconsistent: the class table says `/req/core/geometry-zvalue`, the requirement box says `req/core/geometry-zcoordinate` (also missing its leading solidus). | Uses `/req/core/geometry-zvalue`, the class table's spelling. |
| 5 | §7.10.2, §7.11.3, §7.12.3, §7.14.2–.7 | Requirement box URIs in both grid extensions and in all six versioning boxes are written without the leading solidus their own class tables use; Tiling9 is the mirror image (no solidus in the §7.10.2 class table, solidus in its box). Attribution, by contrast, is solidus-consistent throughout. | Normalized to absolute (§5). |
| 6 | throughout | A multi-part requirement puts each part's letter in its own table column, so the draft never spells a part's identity as one token — `/req/core/attribute-model-content` and `B` are separate cells. Single-part boxes carry no letter at all. | Hyphen-joined into one token in codes (`…-content-B`); display prose follows the draft's two-column layout. |
| 7 | §7.11, §7.12 | TCE5's tileset-metadata requirement box is **missing** from the document in both extensions. | Tileset duties are covered by Tiling9/10 at the abstract level. |
| 8 | §7.13.5 | Face1's box reuses Topo1's URI verbatim; the Face3 and Face4 boxes carry `/rec/` prefixes although their labels and text are conditional SHALLs. | Read as SHALLs; Face4 codes to `/req/core/topology-winding` (its box's own slug is `-face-winding`). |
| 9 | §7.14.2 | V1's box URI collides with the versioning class URI. | Cites §7.14.2 rather than inventing a slug. |
| 10 | §7.14.3 | The table says `version-collection`, the box says `versioning-collection`. | Uses `/req/core/versioning-collection`, reading the concept from the §7.14.3 prose. |
| 11 | §7.14.4 | The clause carries an editorial TODO admitting it lacks words for who made a change and how a change links to a resource record. | Filled implementation-defined: `CollectionManifest::description` and `ChangeRecord::resource_record`. |
| 12 | §7.13.2 | Topo1's box cites ISO "19101" where 19107 is meant; V6's text reads "the ability capture". | Read through the typo. |
| 13 | §7.9.4.2 | There is **no** per-record tileset-metadata signal: no conditional element, and `ResourceType` has the single `Dataset` variant. | See §8 — this crate's one interpretation of a silence. |
| 14 | §2, Table 1 | Two of the five suggested conformance-class URIs omit the separator after `<name>` (`…/conf/<name>file-naming`, `…/conf/<name>file-structure`); the other three carry it. Annex A's own single URI (`/conf/minimal-core`) is well formed. | Normalized to `http://www.opengis.net/spec/CDB/2.0/conf/<profile>/<class>`. |
| 15 | §7.2.6 | Coverages6-E assigns `data_null` no default and no mandatory line. | Read as "absent means the coverage has no null value". |
| 16 | §7.3.1.4, §7.3.1.9 | Both CRS examples are malformed strict WKT-2: a missing comma between the two compound members, and a space before `[` in `COMPOUNDCRS [`. The §7.3.1.9 copy is additionally bracket-unbalanced — it closes `VERTCRS` and leaves `COMPOUNDCRS` open, where the otherwise identical §7.3.1.4 copy (spec line 989) closes both. The two differ in nothing else but the title string (`"I3S Compound CRS"` vs `"CDB Compound CRS"`). | The WKT-2 reader is comma- and whitespace-lenient, so the spec's own examples parse verbatim. The unbalanced bracket is the one thing leniency cannot cover — an unclosed node is an unreadable document — so the `SPEC_COMPOUND` fixture takes **§7.3.1.4's copy whole**, title included: verbatim, with no correction at all. |
| 17 | §7.1.2.3 | The example attribute table carries a stray closing parenthesis in a description. | Reproduced verbatim in the test fixture. |
| 18 | §7.4.5 | Recommendation Name4 (avoid empty folders) is a *naming* recommendation that can only be detected by walking the tree. | Emitted by the hierarchy validator and filed under File Structure; its code stays `/req/core/name-empty-folders-A`. |
| 19 | §7.4.3, §7.4.5 | **Two recommendation boxes carry `/req/` URIs.** Recommendation Name1 is `/req/core/name-unicode` (spec line 1145) and Recommendation Name4 is `/req/core/name-empty-folders` (line 1164), although both boxes are labelled *Recommendation* and both state SHOULDs. Row 8's defect family, inverted: there, SHALL boxes carry `/rec/` prefixes. | Reproduced: both codes keep the `/req/core/` prefix the draft gave them, and both findings are `CdbWarning`s. This is the sole exception to §5 normalization 1, and it is noted there. |
| 20 | §7.3.1.4, §7.3.1.5 | **CRS5 and CRS6 are the only two boxes in the document carrying a version segment.** Their boxes read `/req/2.0/core/crs/crsMetadata` (spec line 987) and `/req/2.0/core/crs/uom` (line 1024), while the §7.3.1 class table spells both without the `2.0` (lines 958, 960) — as does every other box in the draft. | Normalized to the class-table form, `/req/core/crs/crsMetadata` and `/req/core/crs/uom` (`finding.rs`). A version segment inside a clause URI would also make every code version-dependent, which Annex A's own `/conf/minimal-core` shows is not the draft's intent. |
| 21 | §7.4.1 vs §7.4.2–.8 | **§7.4 numbers its own naming requirements two different ways.** The §7.4.1 class table (spec lines 1110–1136) reads Name1=`name-spaces`, Rec Name1=`name-unicode`, **Name2**=`name-language`, **Rec Name2**=`name-empty-folders`, **Name3** *and* **Name5** both =`name-ap-guide`, **Name4** *and* **Name6** both =`name-case`, Name7=`name-extensions`. The requirement boxes of §7.4.2–.8 read Name1, Rec Name1, **Name3**=`name-language`, **Rec Name4**=`name-empty-folders`, Name5, Name6=`name-case`, Name7. Same defect family as rows 4, 9 and 10. | **The boxes govern**, here and throughout §4.1: this document's labels are `Name3-A`/`Name3-B` for the language rule, `Rec Name4` for empty folders, `Name5` for the style-guide duty and `Name6` for the case rule. An auditor reading the class table and searching for "Name3-B" will land on `name-ap-guide` instead; that is the draft's disagreement with itself, not this crate's. (Smaller and adjacent: the §7.10.2 class table lists Recommendation Tiling1 as `/req/core/tiling-extension` while its own box, spec line 1932, says `*/rec/core/tiling-extension`; the crate uses the box.) |
| 22 | §7.9.4.1 vs §7.9.3.2, §7.9.3.5 | **The global element table omits the elements two of its own module's requirements need on disk.** Metadata2 ("SHALL use one of the following metadata standards") and Metadata5 ("one encoding covers every metadata instance") each declare one value per datastore, but §7.9.4.1's element inventory — the table with the Mandatory/Optional column — has no row placing either declaration in the record, and names no element for them anywhere. Metadata8-B at least mandates its element's *name* ("SHALL be uom"), yet the table lacks that row too. A second implementer building the record from the table alone produces a file with no stated standard or encoding, and there is nowhere else the declarations could live. | The record carries them: `metadataStandard` and `metadataEncoding`, spelled in the table's own camelCase convention (`contactPoint`), alongside Metadata8-B's `uom`. All three are required elements; §4.4's "global record on the wire" table is the full schema. Discovered the hard way by the cold second-implementer build this document's §4.4 now cites. |

---

## 8. Interpretation — Tiling9/Tiling10 and what counts as tileset metadata

Requirements Tiling9 and Tiling10 say a **tileset**'s metadata shall follow
the declared standard and shall include at least ID, Title, Description, and
Keywords. The draft never says which resource metadata records are tileset
metadata: §7.9.4.2 defines no tileset conditional element, `ResourceType` has
the single `Dataset` variant, and neither grid extension parses a tile
address out of a path.

**This crate reads every resource metadata record of a tiling-declaring
datastore as tileset metadata.** A `tilingScheme` element on the global record
is what makes a datastore tiled (Tiling8); once it is present, Tiling9/10
bind on each record.

Two consequences an implementer must plan for:

1. **`keywords` becomes effectively mandatory** on every resource metadata
   record in such a datastore. A record without keywords draws
   `TilingViolation::MissingTilesetKeywords`.
2. **A datastore that mixes tiled and non-tiled resources under one scheme
   will see violations** on its non-tiled records, which arguably should not
   bind.

The alternative — dropping Tiling9/10 for want of a signal — silently
discards a SHALL, which we judge the worse failure. A profile that needs the
narrower reading can carry a per-record convention of its own and validate
with `tiling::validate_tileset_metadata` directly rather than relying on the
datastore-level stage.

---

## 9. Case stance — guards fold, requirements don't

This is **user-visible behavior, not an erratum**. One crate-internal helper
(`naming::guard_eq`) folds ASCII case, and exactly one rule decides who uses
it:

- A **guard** decides *which on-disk thing a name points at*. Guards fold
  ASCII case, because on a case-insensitive filesystem `Versions/` **is** the
  journal and `Global_Metadata/` **is** File6's folder — a byte-exact guard is
  no guard at all.
- A **requirement** judges the name itself. Requirements stay byte-exact,
  because folding would make them vacuous.

Folding is ASCII-only (`str::eq_ignore_ascii_case`): full Unicode case folding
needs tables the crate has no business carrying, and CDB names are ASCII by
construction.

**Guard sites (fold):**

| Site | What it decides |
|---|---|
| `versioning::reserved_tree_of` | Whether a versioning collection may address a path. Returns the canonical spelling whatever was offered. |
| `DatastoreLayout::validate`'s root-name match | Whether the root is RFile1's recommended folder. |
| the conformance walk's `global_metadata/` detection | Whether the walk is standing in File6's folder. |
| the `vector_attributes` stem signal | Which file claims to be the attribute model. |
| the conformance walk's `versions/` descent guard | Whether a directory is the crate's own journal and therefore not name-checked. |
| `SimulationProfile::is_resource_metadata`'s `metadata/` component | Whether a path is a resource metadata record. |

**Requirement sites (byte-exact):**

| Site | Why |
|---|---|
| `CaseRule::matches` (Name6) | The requirement is *about* case. |
| `StyleGuide::is_reserved` | The exemption gate that decides whether Name6 applies. Folding it would exempt `Global_Metadata` from the case rule — making Name6 vacuous for exactly the names the spec mandates verbatim. |
| `attribution::parse_file_name` (Attr1-C) | The requirement mandates one literal name. |
| `AttributeModel::validate`'s id uniqueness (Attr2-B) | An id is a requirement's subject, not a path; a profile whose vocabulary distinguishes `AL013` from `al013` would lose that distinction. |

The exemption list itself, since a name off it answers to Name6: the library
reserves four stems — `global_metadata`, `vector_attributes`, `crs`,
`versions` — and a profile widens the set with its `reserved_names` and its
resource-metadata directory, nothing else. `crs` is on the list for
`crs.wkt`, the CRS5 record (§4.5).

**The accepted cost.** On a case-sensitive volume, a genuine directory named
`Versions/` or `Global_Metadata/` — one the datastore author meant as ordinary
content — is treated as reserved and **refused as a versioning asset target**
(`VersioningViolation::AssetInReservedTree`). We accept this: such a name
violates Requirement Name6 under every case rule this crate's profiles pin,
so it would be flagged anyway, and the alternative is a fence that a
re-casing walks straight through. On a case-insensitive volume the same
re-casing previously produced a **silent successful overwrite of the crate's
own journal manifest**; that is what the stance closes.

Guards are *lookups*, not fences, when the filesystem itself answers the
question: `GlobalMetadata::read_from`, `StorageCrs::read_from`,
`CdbDatastore::attribute_model` and the manifest readers probe canonical
names directly and are deliberately left alone. Where that matters — a
mis-cased `vector_attributes` or `global_metadata` file, or a
`vector_attributes` file spelled with the other encoding's extension — the
conformance sweep convicts it and the Attribution stage still reads it, which
is the right layer. The facade stays a canonical-name reader on purpose: it is
a reader for datastores that *are* conformant, and `validate` is the tool that
says whether one is.

---

## 10. How this document is kept honest

`tests/conformance_matrix.rs` holds three guards that run with every
`cargo test`:

- **`req_core_conformance_matrix_lists_every_requirements_class`** — for each
  variant of `RequirementsClass::ALL`, the document must mention the class's
  short name *and* cite its requirements URI, and must cite
  `/conf/minimal-core`. A class added post-1.0 (the enum is
  `#[non_exhaustive]` for exactly that reason) fails the build until this
  document grows the section that audits it.
- **`req_core_conformance_matrix_cites_only_real_tests`** — every backticked
  `req_*` / `rec_*` / `conf_*` / `per_*` token in this file must resolve to a
  function somewhere in `src/` or `tests/` that **carries `#[test]`**. An
  audit that follows a citation to a renamed or deleted test is worse served
  than by no citation at all, and a test-shaped *helper* would be worse
  still — it would look like evidence while running never. The guard asserts
  that it found a plausible number of tests on both sides, so it cannot pass
  by looking in the wrong place, and it checks a deliberately planted
  non-`#[test]` decoy is rejected, so its attribute lookback is proven rather
  than assumed.
- **`req_core_conformance_matrix_cites_only_real_api_items`** — every
  backticked Rust-path token in the matrix's **API** column must resolve,
  segment by segment, to an identifier `src/` declares. The Tests column was
  guarded from the start and this one was not; a review found
  `metadata::temporal::TemporalInterval` here — a type that was never
  written — which is precisely the dead end the document exists to prevent.
  Tokens that are deliberately *not* crate items (the spec's `domainSet`,
  `windingOrder` and `tilingScheme` element names, the WKT-2 keywords,
  `serde`'s own traits) are listed by name in the test, so admitting a new
  one is a deliberate act rather than a silent exemption. Only §4's tables
  are scanned — §5.1's and §7's third columns are prose, where a backticked
  encoding or file name is not a claim about the API.

No guard can check that a cited test *proves* what the row claims. That is
what the citation is for: the row tells you where to look, and the test's own
doc comment cites the spec clause it verifies.

**This document has an executable form.** `cdb-lint` — the workspace's CLI,
`cdb-lint/` at `0.1.0` — runs the same `CdbDatastore::validate` against a
stated profile and renders the verdict as text, as the JSON wire shape §2
describes, or as SARIF. Every finding code in §5 has a row in its catalogue
carrying the class the report files it under, the clause in OGC 23-034, and one
line describing that clause; `cdb-lint explain /req/core/name-spaces` prints
one, and `cdb-lint explain --list` prints them all. A fourth staleness guard
lives there rather than here: `cdb-lint/tests/catalogue_guard.rs` scans this
crate's `src/conformance/` tree for the code literals the library can emit and
demands an exact two-way match, so a code that ships without a row fails
cdb-lint's build — and a row naming a code the library cannot emit fails it
too. Like the three guards above, it proves completeness rather than
correctness: the class, clause and gloss columns there are hand-authored and
reviewed by eye, exactly as this document's are.

§6 is the part of this document the CLI implements rather than cites.
`cdb-lint/tests/cli_honesty.rs` holds the tool to those notes in all three
output formats at once: `unchecked` never renders as a green pass, no output
omits coverage, and a ratchet may move an exit code but never the report. The
two renderers spell the third state differently and mean the same thing — this
crate's `Display` writes `[PASS] geometry (content not checked)`, where the CLI
gives the state a token of its own, `[UNCHECKED] geometry  content present, no
datastore-level check`, and never paints it the colour of a pass.
