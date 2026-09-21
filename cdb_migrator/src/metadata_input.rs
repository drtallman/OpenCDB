//! Explicit operator metadata input; payload contents remain opaque.
//!
//! The JSON object has `resources` (an array) and optional `attribute_model`.
//! Each resource has `source_path`, `record` (the public opencdb resource wire
//! shape), and explicit `coverage`, `measurement_values`, `generated_faces`
//! booleans. Unknown fields and duplicate JSON keys are rejected throughout.
//!
//! Supported-input rules exceed the core's minimum: keywords and nonblank
//! ID/title/description are required; false flags reject conflicting conditional
//! fields; a domainSet must explicitly state its units and grid-cell encoding.
//! The last rule prevents silent selection of a pixel convention even though
//! the core defines a default. Corner sampling requires an explicit corner.
//!
//! Other domainSet omissions use the core's §7.2.6 input-schema defaults:
//! precision = 1, scale = 1, offset = 0, field_type = "Height", and absent
//! data_null/quantity_definition mean no supplied value. Effective scalar
//! defaults appear when serializing the manifest; absent optional fields remain
//! absent. These are operator input semantics, never observations of payloads.
//! `CharacterSetCode` likewise uses the resource schema's `utf8` default.
//!
//! Parsing and validation do not match inventory entries or inspect payloads.
//! The planner must match every recognized payload exactly once, reject unused
//! entries and duplicate resource IDs, and call the dataset-aware validator.

use std::collections::BTreeSet;
use std::fmt;

use opencdb::metadata::ResourceMetadata;
use opencdb::AttributeModel;
use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::Cdb1Error;

/// Operator declarations for a migration, serialized with effective defaults.
///
/// Public fields support library callers. The planner must call [`Self::validate`]
/// again after any mutation; JSON callers use [`Self::from_json_str`].
#[derive(Debug, Clone, Serialize)]
pub struct MetadataManifest {
    /// One source-relative declaration per recognized tile or global payload.
    pub resources: Vec<ResourceDeclaration>,
    /// Optional operator-declared model; never extracted from opaque source XML.
    pub attribute_model: Option<AttributeModel>,
}

/// Required metadata and explicit applicability assertions for one payload.
///
/// Booleans describe the operator's assertion, not decoded payload properties.
#[derive(Debug, Clone, Serialize)]
pub struct ResourceDeclaration {
    /// Exact portable source-relative path, using `/` separators and no aliases.
    pub source_path: String,
    /// Record to associate with the payload when writing the destination.
    pub record: ResourceMetadata,
    /// Whether this resource is a coverage and therefore needs `domainSet`.
    pub coverage: bool,
    /// Whether this resource contains measurement values needing record `uom`.
    pub measurement_values: bool,
    /// Whether this resource generates faces needing `windingOrder`.
    pub generated_faces: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestWire {
    resources: Vec<Value>,
    attribute_model: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceWire {
    source_path: String,
    record: Value,
    coverage: bool,
    measurement_values: bool,
    generated_faces: bool,
}

impl MetadataManifest {
    /// Parse the closed JSON input schema, apply documented defaults, and validate.
    ///
    /// Errors identify the source path and field when a resource path is
    /// available; malformed JSON and missing paths identify the input location.
    /// No filesystem reads or inventory matching occur here.
    pub fn from_json_str(content: &str) -> Result<Self, Cdb1Error> {
        let StrictValue(value) =
            serde_json::from_str(content).map_err(|error| refused("metadata manifest", error))?;
        let wire: ManifestWire =
            serde_json::from_value(value).map_err(|error| refused("metadata manifest", error))?;
        let mut resources = Vec::with_capacity(wire.resources.len());
        for (index, value) in wire.resources.into_iter().enumerate() {
            let context = value
                .get("source_path")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("resources[{index}]"));
            let wire: ResourceWire =
                serde_json::from_value(value).map_err(|error| refused(&context, error))?;
            check_record_keys(&wire.record).map_err(|error| refused(&context, error))?;
            let record = serde_json::from_value(wire.record)
                .map_err(|error| refused(&format!("{context}: record"), error))?;
            resources.push(ResourceDeclaration {
                source_path: wire.source_path,
                record,
                coverage: wire.coverage,
                measurement_values: wire.measurement_values,
                generated_faces: wire.generated_faces,
            });
        }
        let attribute_model = wire
            .attribute_model
            .map(|value| {
                check_attribute_keys(&value).map_err(|error| refused("attribute_model", error))?;
                // The public parser also canonicalizes leaf text and integer IDs.
                AttributeModel::from_json_str(&value.to_string())
                    .map_err(|error| refused("attribute_model", error))
            })
            .transpose()?;
        let manifest = Self {
            resources,
            attribute_model,
        };
        manifest.validate()?;
        Ok(manifest)
    }

    /// Revalidate records, conditional fields, path safety, and duplicate paths.
    ///
    /// This deliberately does not decide which records match recognized payloads
    /// or enforce identifier uniqueness; those are inventory-planning duties.
    pub fn validate(&self) -> Result<(), Cdb1Error> {
        let mut paths = BTreeSet::new();
        for resource in &self.resources {
            resource.validate()?;
            if !paths.insert(&resource.source_path) {
                return Err(refused(
                    &resource.source_path,
                    "duplicate source_path declaration",
                ));
            }
        }
        if let Some(model) = &self.attribute_model {
            model
                .validate()
                .map_err(|error| refused("attribute_model", error))?;
        }
        Ok(())
    }
}

impl ResourceDeclaration {
    /// Validate one declaration without inspecting its payload or dataset code.
    ///
    /// Core record/domainSet validators enforce spec requirements. Additional
    /// nonblank keywords, explicit applicability, and conflicting-field checks
    /// are migration input restrictions, not new core conformance findings.
    pub fn validate(&self) -> Result<(), Cdb1Error> {
        let fail = |message| refused(&self.source_path, message);
        if self.source_path.is_empty()
            || self.source_path.contains(['\\', ':'])
            || self.source_path.chars().any(char::is_control)
            || self
                .source_path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(fail(
                "source_path must be a safe, slash-separated relative payload path",
            ));
        }
        for (field, value) in [
            ("ID", &self.record.id),
            ("title", &self.record.title),
            ("description", &self.record.description),
        ] {
            if value.trim().is_empty() {
                return Err(fail(&format!("record.{field} must be nonblank")));
            }
        }
        if self.record.keywords.is_empty()
            || self
                .record
                .keywords
                .iter()
                .any(|word| word.trim().is_empty())
        {
            return Err(fail("record.keywords must contain nonblank keywords"));
        }
        self.record
            .validate()
            .map_err(|error| refused(&format!("{}: record", self.source_path), error))?;
        for (flag, applicable, field, present) in [
            (
                "coverage",
                self.coverage,
                "domainSet",
                self.record.domain_set.is_some(),
            ),
            (
                "measurement_values",
                self.measurement_values,
                "uom",
                self.record.uom.is_some(),
            ),
            (
                "generated_faces",
                self.generated_faces,
                "windingOrder",
                self.record.winding_order.is_some(),
            ),
        ] {
            if applicable != present {
                return Err(fail(&format!(
                    "record.{field} {} when {flag} is {applicable}",
                    if applicable {
                        "is required"
                    } else {
                        "must be absent"
                    }
                )));
            }
        }
        // JSON cannot preserve NaN/infinity: serde_json otherwise writes null,
        // which could silently erase optional values. This is an input-format
        // restriction, not a new Coverages6 or extent requirement.
        if let Some(domain) = &self.record.domain_set {
            for (field, value) in [
                ("precision", Some(domain.precision)),
                ("scale", Some(domain.scale)),
                ("offset", Some(domain.offset)),
                ("data_null", domain.data_null),
            ] {
                if value.is_some_and(|number| !number.is_finite()) {
                    return Err(fail(&format!(
                        "record.domainSet.{field} must be finite for JSON output"
                    )));
                }
            }
            domain.validate().map_err(|error| {
                refused(&format!("{}: record.domainSet", self.source_path), error)
            })?;
        }
        if let Some(bbox) = self
            .record
            .extent
            .as_ref()
            .and_then(|extent| extent.spatial.as_ref())
        {
            for (field, value) in [
                ("west", bbox.west),
                ("south", bbox.south),
                ("east", bbox.east),
                ("north", bbox.north),
            ] {
                if !value.is_finite() {
                    return Err(fail(&format!(
                        "record.extent.spatial.{field} must be finite for JSON output"
                    )));
                }
            }
        }
        if let Some(temporal) = self
            .record
            .extent
            .as_ref()
            .and_then(|extent| extent.temporal.as_ref())
        {
            opencdb::metadata::temporal::Temporal::parse(&temporal.to_string()).map_err(
                |error| {
                    refused(
                        &format!("{}: record.extent.temporal", self.source_path),
                        error,
                    )
                },
            )?;
        }
        Ok(())
    }

    /// Validate against a recognized tile's dataset code, or `None` for a global.
    ///
    /// Codes 001–005 must declare coverage (15-113r6 Tables 5-11/5-17).
    /// Code 006 is an XML material table and has no such forced declaration.
    /// Other semantics remain explicit operator assertions. The caller obtains
    /// the dataset from the reader's inventory, never from a second manifest key.
    pub fn validate_for_dataset(&self, dataset: Option<u16>) -> Result<(), Cdb1Error> {
        self.validate()?;
        if dataset.is_some_and(|code| (1..=5).contains(&code)) && !self.coverage {
            return Err(refused(
                &self.source_path,
                "coverage must be true for raster datasets 001..005",
            ));
        }
        Ok(())
    }
}

fn refused(context: &str, error: impl fmt::Display) -> Cdb1Error {
    Cdb1Error::Refused(format!("{context}: {error}"))
}

// Keep the new manifest schema closed without modifying opencdb's frozen open
// wire types. Public deserializers still handle values and spec validation.
fn check_keys<'a>(
    value: &'a Value,
    allowed: &[&str],
    context: &str,
) -> Result<&'a serde_json::Map<String, Value>, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{context} must be an object"))?;
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("{context}: unknown field {key:?}"));
        }
    }
    Ok(object)
}

fn check_record_keys(value: &Value) -> Result<(), String> {
    let record = check_keys(
        value,
        &[
            "ID",
            "type",
            "title",
            "description",
            "keywords",
            "keywordsCodespace",
            "externalId",
            "publisher",
            "created",
            "updated",
            "themes",
            "formats",
            "contactPoint",
            "license",
            "rights",
            "uom",
            "domainSet",
            "windingOrder",
            "extent",
            "associations",
            "CharacterSetCode",
        ],
        "record",
    )?;
    if let Some(value) = record.get("domainSet").filter(|value| !value.is_null()) {
        let domain = check_keys(
            value,
            &[
                "uom",
                "precision",
                "scale",
                "offset",
                "data_null",
                "grid_cell_encoding",
                "which_corner",
                "field_type",
                "quantity_definition",
            ],
            "record.domainSet",
        )?;
        for required in ["uom", "grid_cell_encoding"] {
            if !domain.contains_key(required) {
                return Err(format!(
                    "record.domainSet.{required} is required by the migration input schema"
                ));
            }
        }
    }
    if let Some(value) = record.get("extent").filter(|value| !value.is_null()) {
        let extent = check_keys(value, &["spatial", "temporal"], "record.extent")?;
        if let Some(value) = extent.get("spatial").filter(|value| !value.is_null()) {
            check_keys(
                value,
                &["west", "south", "east", "north"],
                "record.extent.spatial",
            )?;
        }
    }
    if let Some(values) = record.get("associations").and_then(Value::as_array) {
        for value in values {
            check_keys(
                value,
                &["href", "rel", "type", "title"],
                "record.associations",
            )?;
        }
    }
    Ok(())
}

fn check_attribute_keys(value: &Value) -> Result<(), String> {
    let model = check_keys(value, &["schemaUri", "attributes"], "attribute_model")?;
    if let Some(values) = model.get("attributes").and_then(Value::as_array) {
        for value in values {
            check_keys(
                value,
                &["id", "name", "description"],
                "attribute_model.attributes",
            )?;
        }
    }
    Ok(())
}

// serde_json::Value normally retains the last duplicate object key. Detect
// duplicates recursively before any Value-based validation can discard them.
struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct StrictVisitor;
        impl<'de> Visitor<'de> for StrictVisitor {
            type Value = StrictValue;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("JSON with unique object keys")
            }
            fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<StrictValue, E> {
                Ok(StrictValue(value.into()))
            }
            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<StrictValue, E> {
                Ok(StrictValue(value.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<StrictValue, E> {
                Ok(StrictValue(value.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<StrictValue, E> {
                Ok(StrictValue(value.into()))
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<StrictValue, E> {
                Ok(StrictValue(value.into()))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<StrictValue, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<StrictValue, A::Error> {
                let mut values = Vec::new();
                while let Some(StrictValue(value)) = seq.next_element()? {
                    values.push(value);
                }
                Ok(StrictValue(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<StrictValue, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate JSON key {key:?}"
                        )));
                    }
                    let StrictValue(value) = map.next_value()?;
                    values.insert(key, value);
                }
                Ok(StrictValue(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(StrictVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn entry() -> Value {
        json!({"source_path":"Tiles/N32/W118/001_Elevation/L00/U0/N32W118_D001_S001_T001_L00_U0_R0.tif",
            "record":{"ID":"elevation-1","type":"dataset","title":"Elevation",
                "description":"Operator description","keywords":["elevation"]},
            "coverage":false,"measurement_values":false,"generated_faces":false})
    }
    fn manifest(entry: Value) -> String {
        json!({"resources":[entry]}).to_string()
    }
    fn parse(value: Value) -> Result<MetadataManifest, crate::Cdb1Error> {
        MetadataManifest::from_json_str(&manifest(value))
    }

    /// Input contract: missing booleans never become declarations of absence.
    #[test]
    fn mig_metadata_requires_all_applicability_flags() {
        for key in ["coverage", "measurement_values", "generated_faces"] {
            let mut value = entry();
            value.as_object_mut().unwrap().remove(key);
            let error = parse(value).unwrap_err().to_string();
            assert!(error.contains(key), "{error}");
        }
        assert!(parse(entry()).is_ok());
    }

    /// Migration input restricts keywords and blank metadata beyond Metadata7.
    #[test]
    fn mig_metadata_requires_nonblank_record_and_keywords() {
        for field in ["ID", "title", "description"] {
            let mut value = entry();
            value["record"][field] = json!(" \t");
            let error = parse(value).unwrap_err().to_string();
            assert!(error.contains(field) && error.contains("Tiles/"), "{error}");
        }
        for keywords in [json!([]), json!([" "]), json!(["valid", ""])] {
            let mut value = entry();
            value["record"]["keywords"] = keywords;
            assert!(parse(value).unwrap_err().to_string().contains("keywords"));
        }
    }

    /// Coverages6/Geom4/Face4 conditional input requirements, not payload checks.
    #[test]
    fn mig_metadata_conditionals_must_be_supplied_and_consistent() {
        for (flag, field, valid) in [
            (
                "coverage",
                "domainSet",
                json!({"uom":"m", "grid_cell_encoding":"value-is-center"}),
            ),
            ("measurement_values", "uom", json!("M")),
            ("generated_faces", "windingOrder", json!("counterclockwise")),
        ] {
            let mut value = entry();
            value[flag] = json!(true);
            let error = parse(value.clone()).unwrap_err().to_string();
            assert!(error.contains(field) && error.contains("Tiles/"), "{error}");
            value["record"][field] = valid;
            assert!(
                parse(value.clone()).is_ok(),
                "{flag}: {:?}",
                parse(value.clone())
            );
            value[flag] = json!(false);
            assert!(parse(value).unwrap_err().to_string().contains(field));
        }
    }

    /// Input convention: raster units and sampling convention are explicit.
    #[test]
    fn mig_metadata_domain_set_requires_explicit_units_and_grid_encoding() {
        for domain in [
            json!({"grid_cell_encoding":"value-is-center"}),
            json!({"uom":"m"}),
        ] {
            let mut value = entry();
            value["coverage"] = json!(true);
            value["record"]["domainSet"] = domain;
            assert!(parse(value).is_err());
        }
        let mut value = entry();
        value["coverage"] = json!(true);
        value["record"]["domainSet"] = json!({"uom":"m", "grid_cell_encoding":"value-is-corner"});
        assert!(parse(value.clone()).is_err());
        value["record"]["domainSet"]["which_corner"] = json!("lower-left-corner");
        assert!(parse(value).is_ok());
    }

    /// §7.2.6 defaults are effective operator input, never decoded source facts.
    #[test]
    fn mig_metadata_effective_domain_defaults_are_serialized() {
        let mut value = entry();
        value["coverage"] = json!(true);
        value["record"]["domainSet"] = json!({"uom":"m", "grid_cell_encoding":"value-is-center"});
        let effective = serde_json::to_value(parse(value).unwrap()).unwrap();
        let domain = &effective["resources"][0]["record"]["domainSet"];
        assert_eq!(domain["precision"], 1.0);
        assert_eq!(domain["scale"], 1.0);
        assert_eq!(domain["offset"], 0.0);
        assert_eq!(domain["field_type"], "Height");
    }

    /// 15-113r6 Tables 5-11/5-17: 001–005 are rasters; 006 is an XML table.
    #[test]
    fn mig_metadata_known_raster_cannot_opt_out() {
        let input = parse(entry()).unwrap();
        for dataset in 1..=5 {
            let error = input.resources[0]
                .validate_for_dataset(Some(dataset))
                .unwrap_err()
                .to_string();
            assert!(error.contains("coverage") && error.contains("Tiles/"));
        }
        assert!(input.resources[0].validate_for_dataset(Some(6)).is_ok());
        assert!(input.resources[0].validate_for_dataset(None).is_ok());
    }

    /// Manifest paths are portable source-relative keys, not normalizable paths.
    #[test]
    fn mig_metadata_rejects_duplicate_and_unsafe_paths() {
        let duplicate = json!({"resources":[entry(),entry()]}).to_string();
        assert!(MetadataManifest::from_json_str(&duplicate)
            .unwrap_err()
            .to_string()
            .contains("duplicate"));
        for path in [
            "",
            "/Tiles/a.tif",
            "../a",
            "a/../b",
            "./a",
            "a//b",
            "a/",
            "C:/a",
            "a\\b",
            "a\u{0000}b",
            "a/./b",
        ] {
            let mut value = entry();
            value["source_path"] = json!(path);
            assert!(parse(value).is_err(), "{path:?}");
        }
    }

    /// A misspelled input key must not silently disappear in frozen wire types.
    #[test]
    fn mig_metadata_rejects_unknown_and_duplicate_json_keys() {
        let mut value = entry();
        value["covergae"] = json!(true);
        assert!(parse(value).is_err());
        let mut value = entry();
        value["record"]["winding_order"] = json!("clockwise");
        assert!(parse(value).is_err());
        let mut value = entry();
        value["coverage"] = json!(true);
        value["record"]["domainSet"] =
            json!({"uom":"m","grid_cell_encoding":"value-is-center","scsale":1});
        assert!(parse(value).is_err());
        let content =
            manifest(entry()).replace("\"coverage\":false", "\"coverage\":false,\"coverage\":true");
        assert!(MetadataManifest::from_json_str(&content)
            .unwrap_err()
            .to_string()
            .contains("duplicate"));
        assert!(MetadataManifest::from_json_str("{\"resources\":[],\"resource\":[]}").is_err());
    }

    /// Metadata7 still applies when a library caller directly builds Temporal.
    #[test]
    fn mig_metadata_programmatic_invalid_temporal_interval_is_refused() {
        let mut input = parse(entry()).unwrap();
        input.resources[0].record.extent = Some(opencdb::metadata::Extent {
            spatial: None,
            temporal: Some(opencdb::metadata::temporal::Temporal::Interval {
                start: None,
                end: None,
            }),
        });
        let error = input.validate().unwrap_err().to_string();
        assert!(error.contains("extent.temporal") && error.contains("Tiles/"));
    }

    /// Coverages6 recommendations remain warnings, not input errors.
    #[test]
    fn mig_metadata_keeps_coverage_recommendations_as_warnings() {
        let mut value = entry();
        value["coverage"] = json!(true);
        value["record"]["domainSet"] =
            json!({"uom":"https://example.org/unit", "grid_cell_encoding":"value-is-center"});
        let input = parse(value).unwrap();
        assert_eq!(
            input.resources[0]
                .record
                .domain_set
                .as_ref()
                .unwrap()
                .warnings()
                .len(),
            1
        );
    }

    /// Library callers can mutate fields; invalid records must still be refused.
    #[test]
    fn mig_metadata_programmatic_nonfinite_values_are_refused() {
        let mut value = entry();
        value["coverage"] = json!(true);
        value["record"]["domainSet"] = json!({"uom":"m", "grid_cell_encoding":"value-is-center"});
        let input = parse(value).unwrap();
        for field in ["precision", "scale", "offset", "data_null"] {
            let mut changed = input.clone();
            let domain = changed.resources[0].record.domain_set.as_mut().unwrap();
            match field {
                "precision" => domain.precision = f64::NAN,
                "scale" => domain.scale = f64::INFINITY,
                "offset" => domain.offset = f64::NEG_INFINITY,
                _ => domain.data_null = Some(f64::NAN),
            }
            let error = changed.validate().unwrap_err().to_string();
            assert!(error.contains(field) && error.contains("Tiles/"), "{error}");
        }
        let mut changed = input.clone();
        changed.resources[0].record.extent = Some(opencdb::metadata::Extent {
            spatial: Some(opencdb::metadata::Bbox {
                west: f64::NAN,
                south: 0.0,
                east: 1.0,
                north: 1.0,
            }),
            temporal: None,
        });
        assert!(changed.validate().unwrap_err().to_string().contains("west"));
    }

    /// The JSON schema preserves valid optional metadata and public validators.
    #[test]
    fn mig_metadata_optional_fields_roundtrip_and_validate() {
        let mut value = entry();
        value["record"]["extent"] = json!({"spatial":{"west":1,"east":2,"south":3,"north":4},"temporal":"2026-01-01T00:00:00Z"});
        value["record"]["associations"] = json!([{"href":"https://example.org/data","rel":"related","type":"application/json","title":"Source"}]);
        value["record"]["created"] = json!("2026-01-01T00:00:00Z");
        value["record"]["formats"] = json!(["image/tiff"]);
        let input = parse(value.clone()).unwrap();
        let effective = serde_json::to_string(&input).unwrap();
        assert_eq!(
            serde_json::to_value(MetadataManifest::from_json_str(&effective).unwrap()).unwrap(),
            serde_json::to_value(&input).unwrap()
        );
        value["record"]["associations"][0]["href"] = json!("");
        assert!(parse(value.clone()).is_err());
        value["record"]["associations"] = json!([]);
        value["record"]["created"] = json!("2026-01-01T01:00:00+01:00");
        assert!(parse(value).is_err());
    }

    /// Closed schema checking recurses through extents, links, and models.
    #[test]
    fn mig_metadata_nested_typos_and_duplicate_record_keys_are_refused() {
        for (field, nested) in [
            (
                "extent",
                json!({"spatial":{"west":1,"south":2,"east":3,"north":4,"typo":5}}),
            ),
            ("extent", json!({"time":"2026-01-01T00:00:00Z"})),
            (
                "associations",
                json!([{"href":"https://example.org","rel":"related","typo":1}]),
            ),
        ] {
            let mut value = entry();
            value["record"][field] = nested;
            assert!(parse(value).is_err());
        }
        let content = manifest(entry()).replace(
            "\"ID\":\"elevation-1\"",
            "\"ID\":\"elevation-1\",\"ID\":\"other\"",
        );
        assert!(MetadataManifest::from_json_str(&content)
            .unwrap_err()
            .to_string()
            .contains("duplicate"));
        for invalid in ["null", "[]", "{}", "{\"resources\":null}"] {
            assert!(MetadataManifest::from_json_str(invalid).is_err());
        }
    }

    /// Attr2/PAttr1 validate the supplied model; a URI cannot replace its fields.
    #[test]
    fn mig_metadata_attribute_model_is_validated() {
        let valid = json!({"schemaUri":"https://example.org/attributes", "attributes":[
            {"id":"1", "name":"Height", "description":"Operator declared height"}]});
        let content = json!({"resources":[entry()], "attribute_model":valid}).to_string();
        let input = MetadataManifest::from_json_str(&content).unwrap();
        assert!(input.attribute_model.is_some());
        for model in [
            json!({"schemaUri":"https://example.org/schema"}),
            json!({"attributes":[{"id":"1","name":"x","description":"d","typo":1}]}),
            json!({"attributes":[{"id":"1","name":"x","description":"d"},{"id":" 1 ","name":"y","description":"d"}]}),
        ] {
            let content = json!({"resources":[],"attribute_model":model}).to_string();
            assert!(MetadataManifest::from_json_str(&content).is_err());
        }
    }
}
