//! CDB Attribution Requirements Module (spec §7.1) — the pure model.
//!
//! Implements the `/req/core/attributes` requirements class (Attr1,
//! Permission PAttr1, Attr2; §7.1.2): the attribute model persisted as
//! `global_metadata/vector_attributes.<ext>`, its content rules, and the
//! Attr1-C file-name rules. All I/O lives on the
//! [`crate::datastore::CdbDatastore`] facade; this module is types and
//! rules only, the same split as every other requirements module.
//!
//! Draft quirks, doc-noted keyed on meaning:
//! - PAttr1's URI: the class table says `/per/core/attribute-schema-uri`,
//!   the permission box says `/per/core/attribute-model` — colliding with
//!   Attr1's requirement URI (the V1 lesson). Cited here in the
//!   non-colliding table form.
//! - The class table labels PAttr1 "Requirement"; the box says
//!   "Permission" (a permission is what it is).
//! - Typos: "External loation", "intnerational", "e.g. and ID and a
//!   name", "Type street as an alphanumeric string".
//! - Silent on a URI-only file: this crate requires the inline minimum
//!   ALWAYS — the datastore stays self-describing offline (the
//!   determinism principle) and Attr2 stays checkable without network
//!   I/O. PAttr1's URI supplements the inline model, never replaces it.
//! - Silent on an empty model: rejected here — a model with zero
//!   attributes "specifies" nothing (fails Attr1-A's "specify an
//!   attribute model" and Attr2-A's minimum).
//!
//! Attribute ids are strings: Attr2-B says only "unique identifier", and
//! the inline-minimum rule means NAS/LCCS-backed profiles must project
//! codes like `AL013` inline, which an integer id cannot carry. The
//! spec's informative fixture (`1`/`2`/`3`) still parses — the JSON
//! parse canonicalizes bare non-negative-integer ids to their decimal
//! strings. Id uniqueness is byte-exact after trimming, and stays so
//! under the crate-wide case stance — *guards fold, requirements don't*
//! (the crate-internal `naming::guard_eq`). An id is a requirement's
//! subject, not a path: folding it would silently change what
//! [`AttributionViolation::DuplicateId`] means, and a profile whose
//! vocabulary distinguishes `AL013` from `al013` would lose that
//! distinction. Only Attr1-C's *file* name meets a guard, and only on the
//! finding side — see [`parse_file_name`].
//!
//! Parsing canonicalizes, validation rejects blanks. Both parse
//! functions trim leading/trailing whitespace from `schemaUri` and from
//! every attribute's `id`, `name`, and `description` before validating,
//! so a hand-authored, indented `vector_attributes.xml` reads as the
//! model it depicts, the two encodings carry one model to one in-memory
//! value, and whitespace cannot make two ids "distinct" for Attr2-B.
//! Whitespace-only values are still violations — the trim runs first,
//! then the blank check fires. Attr2-B uniqueness is compared on trimmed
//! ids, so the in-memory checker agrees with the read path and the crate
//! never writes a file it would refuse to read back. The cost is deliberate: a string stored
//! with meaningful edge whitespace does not survive a write→read round
//! trip unchanged (no CDB attribute vocabulary assigns meaning to it).
//!
//! This module deliberately has NO warning type — §7.1 contains no
//! SHOULD-level finding — and no dependency on the links module: PAttr1's
//! `schema_uri` is a bare URI, not a Links-class link object, so a
//! private RFC 3986 scheme check keeps the crate's near-zero
//! cross-module dependency doctrine.

use std::collections::BTreeSet;
use std::io;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::metadata::MetadataEncoding;
use crate::naming::split_extension;

/// File stem of the attribute-model schema file (Requirement Attr1-C):
/// `vector_attributes.<ext>` with `<ext>` either `xml` or `json`, stored
/// at the top of `global_metadata/` (Attr1-B). Reserved in
/// [`crate::naming::StyleGuide`] the way `global_metadata` is.
pub const VECTOR_ATTRIBUTES_STEM: &str = "vector_attributes";

/// A violation of a SHALL requirement of the attribution module
/// (§7.1.2), or of one of its doc-noted implementation constraints.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AttributionViolation {
    /// Attr1-A/Attr2-A (§7.1.2.1/.3): a model with zero attributes
    /// "specifies" no attribute model (doc-noted strict reading).
    #[error(
        "the attribute model contains no attributes; an attribute model must specify at least one (violates req/core/attribute-model A and req/core/attribute-model-content A, §7.1.2)"
    )]
    EmptyModel,
    /// Attr2-B (§7.1.2.3): each attribute needs a unique identifier.
    #[error(
        "attribute id {id:?} appears more than once; each attribute must have a unique identifier (violates req/core/attribute-model-content B, §7.1.2.3)"
    )]
    DuplicateId { id: String },
    /// Attr2-B (§7.1.2.3): an empty, blank, or control-character id is
    /// not an identifier.
    #[error(
        "attribute at position {position} has an empty, blank, or control-character id; each attribute must have a unique identifier (violates req/core/attribute-model-content B, §7.1.2.3)"
    )]
    EmptyId { position: usize },
    /// Attr2-C (§7.1.2.3): each attribute needs a name.
    #[error(
        "attribute {id:?} has an empty, blank, or control-character name; each attribute must have a name (violates req/core/attribute-model-content C, §7.1.2.3)"
    )]
    EmptyName { id: String },
    /// Attr2-D (§7.1.2.3): each attribute needs a description.
    #[error(
        "attribute {id:?} has an empty, blank, or control-character description; each attribute must have a description (violates req/core/attribute-model-content D, §7.1.2.3)"
    )]
    EmptyDescription { id: String },
    /// Permission PAttr1 (§7.1.2.2): a present external-schema URI must
    /// be a URI — an RFC 3986 scheme, a non-empty remainder, no control
    /// characters.
    #[error("schema URI {uri:?} is not a valid URI (per/core/attribute-schema-uri, §7.1.2.2)")]
    InvalidSchemaUri { uri: String },
    /// Attr1-C (§7.1.2.1): the schema file is `vector_attributes.<ext>`
    /// with `<ext>` either `xml` or `json`.
    #[error(
        "file name {name:?} is not vector_attributes.<ext> with <ext> xml or json (violates req/core/attribute-model C, §7.1.2.1)"
    )]
    InvalidFileName { name: String },
    /// Attr1-A (§7.1.2.1): the schema file was read but its bytes are not a
    /// parseable attribute model, so the datastore "specifies" none. A
    /// conformance finding, not an operational failure — the read
    /// succeeded; mirrors [`crate::metadata::MetadataViolation::Malformed`].
    #[error(
        "attribute model document is malformed: {reason} (violates req/core/attribute-model A, §7.1.2.1)"
    )]
    Malformed { reason: String },
}

/// Operational failure of the attribution module: an I/O or encoding
/// failure, or an [`AttributionViolation`]. The error/violation split
/// mirrors `versioning`'s.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AttributionError {
    /// A SHALL violation (§7.1.2).
    #[error(transparent)]
    Violation(#[from] AttributionViolation),
    /// Filesystem failure while reading or writing the schema file.
    #[error("attribution I/O failure: {0}")]
    Io(#[from] io::Error),
    /// Schema-file (de)serialization failure.
    #[error("attribution schema encoding failure: {0}")]
    Serialization(String),
}

/// One attribute definition of the model — the Attr2 minimum: a unique
/// identifier, a name, and a description (§7.1.2.3; the shape of the
/// spec's StreetName/StreetType/StreetWidth fixture).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributeDef {
    /// The unique identifier (Attr2-B). String-typed, and canonicalized
    /// on the way in: a bare non-negative integer id in an incoming JSON
    /// document (the spec fixture's style) becomes its decimal string,
    /// and both parse paths trim surrounding whitespace. Uniqueness is
    /// compared on the trimmed value, so `"1"` and `" 1 "` are one
    /// identifier however the model was built.
    pub id: String,
    /// The attribute's name (Attr2-C).
    pub name: String,
    /// The attribute's description (Attr2-D).
    pub description: String,
}

/// The attribute model persisted as
/// `global_metadata/vector_attributes.<ext>` (Attr1, §7.1.2.1): the
/// datastore-wide attribute/classification scheme, with PAttr1's
/// optional pointer to the external location of the full schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributeModel {
    /// External location of the attribution schema (Permission PAttr1,
    /// §7.1.2.2). Supplements the inline minimum; never replaces it.
    #[serde(rename = "schemaUri", default, skip_serializing_if = "Option::is_none")]
    pub schema_uri: Option<String>,
    /// The attribute definitions, in declaration order (Attr2). Defaults
    /// to empty on decode so an absent list surfaces as the
    /// [`AttributionViolation::EmptyModel`] violation, not a parse error.
    #[serde(default)]
    pub attributes: Vec<AttributeDef>,
}

/// Blank-or-control predicate shared by the Attr2 element checks — the
/// versioning `EmptyState` posture: a value is present only if it has
/// visible content and no control characters.
fn is_blank(value: &str) -> bool {
    value.trim().is_empty() || value.chars().any(char::is_control)
}

/// RFC 3986 scheme well-formedness: `ALPHA *( ALPHA / DIGIT / "+" / "-"
/// / "." )`, a `:`, and a remainder with visible content — a blank
/// remainder is no more a value than a blank name is ([`is_blank`]'s
/// posture). The check stays scoped to the scheme and the remainder's
/// presence (design decision 6); it is not a full RFC 3986 parser, so a
/// space *inside* an otherwise well-formed remainder is not this
/// module's finding.
fn has_uri_shape(uri: &str) -> bool {
    match uri.split_once(':') {
        Some((scheme, rest)) => {
            let mut characters = scheme.chars();
            characters
                .next()
                .is_some_and(|first| first.is_ascii_alphabetic())
                && characters.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
                && !rest.trim().is_empty()
        }
        None => false,
    }
}

impl AttributeModel {
    /// Canonicalizes the model's leaf text: leading and trailing
    /// whitespace is trimmed from `schema_uri` and from every
    /// attribute's `id`, `name`, and `description`. Run by both parse
    /// functions before validation, so one logical model has one
    /// in-memory form no matter which encoding — or which indentation —
    /// carried it.
    fn canonicalize(&mut self) {
        if let Some(uri) = &mut self.schema_uri {
            let trimmed = uri.trim();
            if trimmed.len() != uri.len() {
                *uri = trimmed.to_owned();
            }
        }
        for attribute in &mut self.attributes {
            for field in [
                &mut attribute.id,
                &mut attribute.name,
                &mut attribute.description,
            ] {
                let trimmed = field.trim();
                if trimmed.len() != field.len() {
                    *field = trimmed.to_owned();
                }
            }
        }
    }

    /// Validates the Attr2 content rules and PAttr1's URI shape — the
    /// single validation source. Parse functions call this before
    /// returning, and the facade calls it before writing.
    ///
    /// Attr2-B uniqueness is compared on *trimmed* ids, so a model built
    /// in memory cannot forge two "distinct" ids out of whitespace the
    /// way a parsed one cannot: the writer therefore never emits a file
    /// the reader would refuse with `DuplicateId`.
    ///
    /// One asymmetry is deliberate, in the safe direction: this method
    /// does not canonicalize, it only compares. A `schema_uri` held in
    /// memory with leading or trailing whitespace fails as
    /// `InvalidSchemaUri`, while the same text read from a file passes —
    /// the parse trimmed it first. Validation stays a pure predicate on
    /// the value it is given (the facade relies on that when it refuses
    /// a write), and the stricter of the two verdicts is the in-memory
    /// one, so nothing invalid reaches disk.
    pub fn validate(&self) -> Result<(), AttributionViolation> {
        if let Some(uri) = &self.schema_uri
            && (uri.chars().any(char::is_control) || !has_uri_shape(uri))
        {
            return Err(AttributionViolation::InvalidSchemaUri { uri: uri.clone() });
        }
        if self.attributes.is_empty() {
            return Err(AttributionViolation::EmptyModel);
        }
        let mut seen = BTreeSet::new();
        for (position, attribute) in self.attributes.iter().enumerate() {
            if is_blank(&attribute.id) {
                return Err(AttributionViolation::EmptyId { position });
            }
            if is_blank(&attribute.name) {
                return Err(AttributionViolation::EmptyName {
                    id: attribute.id.clone(),
                });
            }
            if is_blank(&attribute.description) {
                return Err(AttributionViolation::EmptyDescription {
                    id: attribute.id.clone(),
                });
            }
            if !seen.insert(attribute.id.trim()) {
                return Err(AttributionViolation::DuplicateId {
                    id: attribute.id.clone(),
                });
            }
        }
        Ok(())
    }

    /// Serializes the model as pretty JSON.
    pub fn to_json_string(&self) -> Result<String, AttributionError> {
        serde_json::to_string_pretty(self)
            .map_err(|e| AttributionError::Serialization(e.to_string()))
    }

    /// Parses and validates a JSON model — a model obtained from bytes
    /// is always valid. Parsing canonicalizes: a bare non-negative
    /// integer id (the spec fixture's style) becomes its decimal string
    /// in a value pre-pass, and leaf text (`schemaUri`, `id`, `name`,
    /// `description`) is trimmed before validation and before the
    /// Attr2-B uniqueness check. A model whose strings carry leading or trailing
    /// whitespace therefore does **not** survive
    /// `from_json_str(to_json_string(m))` unchanged — the whitespace is
    /// dropped, deliberately; canonical models are a fixed point. An id
    /// that is a number the canonicalization cannot carry (negative,
    /// fractional, or wider than `u64`) is a `Serialization` failure.
    pub fn from_json_str(content: &str) -> Result<AttributeModel, AttributionError> {
        let mut value: serde_json::Value = serde_json::from_str(content)
            .map_err(|e| AttributionError::Serialization(e.to_string()))?;
        if let Some(attributes) = value.get_mut("attributes").and_then(|a| a.as_array_mut()) {
            for attribute in attributes {
                if let Some(id) = attribute.get_mut("id") {
                    if let Some(number) = id.as_u64() {
                        *id = serde_json::Value::String(number.to_string());
                    } else if id.is_number() {
                        return Err(AttributionError::Serialization(
                            "attribute id must be a string or a non-negative integer that fits \
                             in 64 bits; quote the value to keep it verbatim"
                                .to_owned(),
                        ));
                    }
                }
            }
        }
        let mut model: AttributeModel = serde_json::from_value(value)
            .map_err(|e| AttributionError::Serialization(e.to_string()))?;
        model.canonicalize();
        model.validate()?;
        Ok(model)
    }

    /// Serializes the model as XML.
    pub fn to_xml_string(&self) -> Result<String, AttributionError> {
        quick_xml::se::to_string(self).map_err(|e| AttributionError::Serialization(e.to_string()))
    }

    /// Parses and validates an XML model — a model obtained from bytes
    /// is always valid. Leaf text (`schemaUri`, `id`, `name`,
    /// `description`) is trimmed before validation and before the
    /// Attr2-B uniqueness check, so a
    /// hand-authored, indented `vector_attributes.xml` reads as the
    /// model it depicts (quick-xml itself preserves element text
    /// verbatim, indentation included) and ids cannot be made distinct
    /// by invisible formatting. As on [`Self::from_json_str`], strings
    /// with leading or trailing whitespace do not survive a write→read
    /// round trip unchanged.
    pub fn from_xml_str(content: &str) -> Result<AttributeModel, AttributionError> {
        let mut model: AttributeModel = quick_xml::de::from_str(content)
            .map_err(|e| AttributionError::Serialization(e.to_string()))?;
        model.canonicalize();
        model.validate()?;
        Ok(model)
    }
}

/// The Attr1-C file name for a metadata encoding: `Some` for the two
/// extensions the requirement allows (`json`, `xml`), `None` otherwise —
/// a GeoPackage-declared datastore's attribute model is not a core file.
pub fn file_name_for(encoding: MetadataEncoding) -> Option<String> {
    match encoding {
        MetadataEncoding::Json | MetadataEncoding::Xml => {
            Some(format!("{VECTOR_ATTRIBUTES_STEM}.{}", encoding.extension()))
        }
        MetadataEncoding::Gpkg => None,
    }
}

/// Parses an Attr1-C file name, returning the encoding its extension
/// denotes. The name must be the canonical form exactly — stem
/// [`VECTOR_ATTRIBUTES_STEM`] (a reserved name, so the profile's case
/// rule never applies to it) and a lowercase `json` or `xml` extension,
/// i.e. precisely what [`file_name_for`] emits.
///
/// The match is case-*sensitive*, unlike [`crate::naming`]'s Requirement
/// Name7 extension table, and the difference is deliberate: Name7
/// resolves a media type for an arbitrary file whose name the crate does
/// not control, so leniency there costs nothing, while Attr1-C mandates
/// one literal file name for one specific file — and a literal-name
/// mandate is read literally. Accepting `vector_attributes.JSON` here
/// would also break the invariant that a name this function blesses is a
/// name the facade can find: [`crate::datastore::CdbDatastore::attribute_model`]
/// probes only the canonical name, so on a case-sensitive volume a
/// mis-cased file would be "valid" and yet silently unreadable.
///
/// This is the *requirement* half of the crate's case stance — guards fold,
/// requirements don't (the crate-internal `naming::guard_eq`) — and it is
/// paired with the guard half: the conformance sweep *finds* the file with a
/// folded stem match and then hands it here to be *judged* byte-exactly, so
/// `Vector_Attributes.json` is convicted rather than ignored.
pub fn parse_file_name(name: &str) -> Result<MetadataEncoding, AttributionViolation> {
    let (stem, extension) = split_extension(name);
    if stem == VECTOR_ATTRIBUTES_STEM {
        match extension {
            Some("json") => return Ok(MetadataEncoding::Json),
            Some("xml") => return Ok(MetadataEncoding::Xml),
            _ => {}
        }
    }
    Err(AttributionViolation::InvalidFileName {
        name: name.to_owned(),
    })
}

/// Validates an attribute-model document as stored at
/// `global_metadata/<file_name>` — the Attribution class's datastore-level
/// entry point (Requirements Attr1-B/C and Attr2, §7.1.2).
///
/// `file_name` is checked against Attr1-C by [`parse_file_name`], and the
/// encoding it names selects the parser; the parse then validates the
/// content against Attr2 (and Permission PAttr1) and canonicalizes it, so
/// the returned model is always a valid one. The *location* duty, Attr1-B
/// (top of `global_metadata/`), is the caller's — it chose the file it read.
///
/// Every failure is a SHALL finding, never an operational error: the caller
/// has already read the bytes, so a document that does not parse is a
/// non-conformant datastore ([`AttributionViolation::Malformed`]), not an
/// I/O problem. This is why the return type is
/// [`AttributionViolation`] rather than [`AttributionError`].
pub fn validate_attribute_model_document(
    file_name: &str,
    content: &str,
) -> Result<AttributeModel, AttributionViolation> {
    let parsed = match parse_file_name(file_name)? {
        MetadataEncoding::Xml => AttributeModel::from_xml_str(content),
        // `parse_file_name` yields only Json or Xml; Json is the remaining
        // case and Gpkg is unreachable, so no arm can panic.
        _ => AttributeModel::from_json_str(content),
    };
    match parsed {
        Ok(model) => Ok(model),
        Err(AttributionError::Violation(violation)) => Err(violation),
        Err(AttributionError::Serialization(reason)) => {
            Err(AttributionViolation::Malformed { reason })
        }
        // The parsers do no I/O, so this arm is unreachable in practice; it
        // is armed defensively as `Malformed` rather than with a panic.
        Err(AttributionError::Io(error)) => Err(AttributionViolation::Malformed {
            reason: error.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The spec's informative Attr2 fixture, verbatim — including its
    /// stray closing parenthesis on StreetWidth's description.
    fn street_fixture() -> AttributeModel {
        AttributeModel {
            schema_uri: None,
            attributes: vec![
                AttributeDef {
                    id: "1".to_owned(),
                    name: "StreetName".to_owned(),
                    description: "Name of a street as an alphanumeric string".to_owned(),
                },
                AttributeDef {
                    id: "2".to_owned(),
                    name: "StreetType".to_owned(),
                    description:
                        "Type street as an alphanumeric string (Interstate, Arterial, . . .)"
                            .to_owned(),
                },
                AttributeDef {
                    id: "3".to_owned(),
                    name: "StreetWidth".to_owned(),
                    description: "Width of street in feet as an integer number)".to_owned(),
                },
            ],
        }
    }

    /// `/req/core/attribute-model-content` (§7.1.2.3) — the spec's own
    /// StreetName/StreetType/StreetWidth example satisfies the minimum,
    /// with and without PAttr1's supplementary URI.
    #[test]
    fn req_core_attribute_model_content_fixture_validates() {
        let model = street_fixture();
        assert!(model.validate().is_ok());

        let with_uri = AttributeModel {
            schema_uri: Some("https://example.org/schemas/street.xsd".to_owned()),
            ..model
        };
        assert!(with_uri.validate().is_ok());
    }

    /// `/req/core/attribute-model` A + `/req/core/attribute-model-content`
    /// A (§7.1.2) — a model with zero attributes specifies nothing, even
    /// when it carries an external-schema URI (the inline minimum is
    /// always required; PAttr1 supplements, never replaces).
    #[test]
    fn req_core_attribute_model_content_empty_model_rejected() {
        let empty = AttributeModel {
            schema_uri: None,
            attributes: Vec::new(),
        };
        assert_eq!(empty.validate(), Err(AttributionViolation::EmptyModel));

        let uri_only = AttributeModel {
            schema_uri: Some("https://example.org/schemas/nas.xsd".to_owned()),
            attributes: Vec::new(),
        };
        assert_eq!(uri_only.validate(), Err(AttributionViolation::EmptyModel));
    }

    /// `/req/core/attribute-model-content` B (§7.1.2.3) — each attribute
    /// needs a UNIQUE identifier; uniqueness is byte-exact.
    #[test]
    fn req_core_attribute_model_content_duplicate_id_rejected() {
        let mut model = street_fixture();
        model.attributes[2].id = "1".to_owned();
        assert_eq!(
            model.validate(),
            Err(AttributionViolation::DuplicateId { id: "1".to_owned() })
        );
    }

    /// `/req/core/attribute-model-content` B/C/D (§7.1.2.3) — an empty,
    /// blank, or control-character id, name, or description does not
    /// satisfy "a unique identifier", "a name", "a description".
    #[test]
    fn req_core_attribute_model_content_blank_elements_rejected() {
        let mut blank_id = street_fixture();
        blank_id.attributes[0].id = "   ".to_owned();
        assert_eq!(
            blank_id.validate(),
            Err(AttributionViolation::EmptyId { position: 0 })
        );

        let mut control_id = street_fixture();
        control_id.attributes[1].id = "2\u{7}".to_owned();
        assert_eq!(
            control_id.validate(),
            Err(AttributionViolation::EmptyId { position: 1 })
        );

        let mut blank_name = street_fixture();
        blank_name.attributes[1].name = String::new();
        assert_eq!(
            blank_name.validate(),
            Err(AttributionViolation::EmptyName { id: "2".to_owned() })
        );

        let mut blank_description = street_fixture();
        blank_description.attributes[2].description = "\t \n".to_owned();
        assert_eq!(
            blank_description.validate(),
            Err(AttributionViolation::EmptyDescription { id: "3".to_owned() })
        );
    }

    /// `per/core/attribute-schema-uri` (Permission PAttr1, §7.1.2.2) — a
    /// present URI must have an RFC 3986 scheme and a non-empty
    /// remainder; anything else is refused. The permission's box URI
    /// (`/per/core/attribute-model`) collides with Attr1's requirement
    /// URI — cited in the class table's non-colliding form.
    #[test]
    fn per_core_attribute_schema_uri_shapes() {
        for valid in [
            "https://example.org/schemas/nas.xsd",
            "http://mmisw.org/ont/cf/parameter/air_temperature",
            "urn:ogc:def:crs:EPSG::4326",
            "file:///schemas/lccs.json",
        ] {
            let model = AttributeModel {
                schema_uri: Some(valid.to_owned()),
                ..street_fixture()
            };
            assert!(model.validate().is_ok(), "expected valid URI: {valid}");
        }

        for invalid in [
            "",
            "   ",
            "not a uri",
            "://missing-scheme",
            "http:",
            "http:   ",
            "a:\u{a0}",
            "1http://leading-digit",
            "ht tp://space-in-scheme",
            "https://example.org/\u{7}",
        ] {
            let model = AttributeModel {
                schema_uri: Some(invalid.to_owned()),
                ..street_fixture()
            };
            assert_eq!(
                model.validate(),
                Err(AttributionViolation::InvalidSchemaUri {
                    uri: invalid.to_owned()
                }),
                "expected invalid URI: {invalid:?}"
            );
        }
    }

    /// Requirement Attr1-C encodings (§7.1.2.1) — the model round-trips
    /// through JSON; `schemaUri` is omitted entirely when absent.
    #[test]
    fn req_core_attribute_model_json_roundtrip() {
        let bare = street_fixture();
        let json = bare.to_json_string().unwrap();
        assert!(!json.contains("schemaUri"));
        assert_eq!(AttributeModel::from_json_str(&json).unwrap(), bare);

        let with_uri = AttributeModel {
            schema_uri: Some("https://example.org/schemas/street.xsd".to_owned()),
            ..street_fixture()
        };
        let json = with_uri.to_json_string().unwrap();
        assert!(json.contains("schemaUri"));
        assert_eq!(AttributeModel::from_json_str(&json).unwrap(), with_uri);
    }

    /// Requirement Attr1-C encodings (§7.1.2.1) — the model round-trips
    /// through XML with the same camelCase element convention as the
    /// versioning manifest.
    #[test]
    fn req_core_attribute_model_xml_roundtrip() {
        let with_uri = AttributeModel {
            schema_uri: Some("https://example.org/schemas/street.xsd".to_owned()),
            ..street_fixture()
        };
        let xml = with_uri.to_xml_string().unwrap();
        assert!(xml.contains("<schemaUri>"));
        assert!(xml.contains("<name>StreetName</name>"));
        assert_eq!(AttributeModel::from_xml_str(&xml).unwrap(), with_uri);

        let bare = street_fixture();
        let xml = bare.to_xml_string().unwrap();
        assert!(!xml.contains("schemaUri"));
        assert_eq!(AttributeModel::from_xml_str(&xml).unwrap(), bare);
    }

    /// `/req/core/attribute-model-content` (§7.1.2.3) — a document
    /// hand-written from the spec's example table, with bare integer
    /// ids, parses; ids canonicalize to their decimal strings.
    #[test]
    fn req_core_attribute_model_content_integer_ids_parse() {
        let literal = r#"{
  "attributes": [
    { "id": 1, "name": "StreetName", "description": "Name of a street as an alphanumeric string" },
    { "id": 2, "name": "StreetType", "description": "Type street as an alphanumeric string (Interstate, Arterial, . . .)" },
    { "id": 3, "name": "StreetWidth", "description": "Width of street in feet as an integer number)" }
  ]
}"#;
        let model = AttributeModel::from_json_str(literal).unwrap();
        assert_eq!(model, street_fixture());
    }

    /// Parse functions validate before returning — a structurally valid
    /// document that violates Attr2 is a Violation, and malformed bytes
    /// are a Serialization failure; no invalid model escapes a parse.
    #[test]
    fn req_core_attribute_model_parse_validates() {
        let duplicate = r#"{
  "attributes": [
    { "id": "1", "name": "A", "description": "a" },
    { "id": "1", "name": "B", "description": "b" }
  ]
}"#;
        assert!(matches!(
            AttributeModel::from_json_str(duplicate),
            Err(AttributionError::Violation(
                AttributionViolation::DuplicateId { .. }
            ))
        ));

        let empty = r#"{ "schemaUri": "https://example.org/s.xsd" }"#;
        assert!(matches!(
            AttributeModel::from_json_str(empty),
            Err(AttributionError::Violation(
                AttributionViolation::EmptyModel
            ))
        ));

        assert!(matches!(
            AttributeModel::from_json_str("not json"),
            Err(AttributionError::Serialization(_))
        ));
        assert!(matches!(
            AttributeModel::from_xml_str("<open"),
            Err(AttributionError::Serialization(_))
        ));
    }

    /// `/req/core/attribute-model-content` B/C/D (§7.1.2.3) — parsing
    /// canonicalizes leaf text: a hand-authored, indented XML document
    /// (the natural shape of the externally-authored file §7.1 is about)
    /// parses to the same model as the compact form, and to the same
    /// model as the equivalent JSON document — the two encodings are
    /// interchangeable carriers of one model.
    #[test]
    fn req_core_attribute_model_parse_canonicalizes_whitespace() {
        let pretty = "<AttributeModel>\n  \
             <schemaUri>\n    https://example.org/schemas/street.xsd\n  </schemaUri>\n  \
             <attributes>\n    \
               <id>\n      1\n    </id>\n    \
               <name>\n      StreetName\n    </name>\n    \
               <description>\n      Name of a street as an alphanumeric string\n    </description>\n  \
             </attributes>\n\
             </AttributeModel>";
        let expected = AttributeModel {
            schema_uri: Some("https://example.org/schemas/street.xsd".to_owned()),
            attributes: vec![AttributeDef {
                id: "1".to_owned(),
                name: "StreetName".to_owned(),
                description: "Name of a street as an alphanumeric string".to_owned(),
            }],
        };
        assert_eq!(AttributeModel::from_xml_str(pretty).unwrap(), expected);

        let compact = expected.to_xml_string().unwrap();
        assert_eq!(
            AttributeModel::from_xml_str(&compact).unwrap(),
            AttributeModel::from_xml_str(pretty).unwrap()
        );

        let json = r#"{
  "schemaUri": "  https://example.org/schemas/street.xsd  ",
  "attributes": [
    { "id": " 1 ", "name": "  StreetName ", "description": "\tName of a street as an alphanumeric string " }
  ]
}"#;
        assert_eq!(AttributeModel::from_json_str(json).unwrap(), expected);
    }

    /// `/req/core/attribute-model-content` B/C/D (§7.1.2.3) — trimming is
    /// canonicalization, not leniency: a value that is whitespace-only is
    /// still empty after the trim, so the blank check fires on both
    /// parse paths.
    #[test]
    fn req_core_attribute_model_parse_rejects_whitespace_only_values() {
        let blank_name = "<AttributeModel><attributes>\
             <id>1</id><name>   </name><description>d</description>\
             </attributes></AttributeModel>";
        assert!(matches!(
            AttributeModel::from_xml_str(blank_name),
            Err(AttributionError::Violation(
                AttributionViolation::EmptyName { .. }
            ))
        ));

        let blank_id = r#"{ "attributes": [ { "id": "  ", "name": "A", "description": "a" } ] }"#;
        assert!(matches!(
            AttributeModel::from_json_str(blank_id),
            Err(AttributionError::Violation(AttributionViolation::EmptyId {
                position: 0
            }))
        ));

        let blank_uri = r#"{ "schemaUri": "http:   ", "attributes": [ { "id": "1", "name": "A", "description": "a" } ] }"#;
        assert!(matches!(
            AttributeModel::from_json_str(blank_uri),
            Err(AttributionError::Violation(
                AttributionViolation::InvalidSchemaUri { .. }
            ))
        ));
    }

    /// `/req/core/attribute-model-content` B (§7.1.2.3) — the uniqueness
    /// check itself canonicalizes, so an in-memory model built by hand
    /// (never through a parse) cannot forge two "distinct" ids out of
    /// whitespace either. Without this the crate could write, with an
    /// `Ok`, a file it would then refuse to read back.
    #[test]
    fn req_core_attribute_model_content_validate_canonicalizes_ids() {
        let forged = AttributeModel {
            schema_uri: None,
            attributes: vec![
                AttributeDef {
                    id: "1".to_owned(),
                    name: "StreetName".to_owned(),
                    description: "Name of a street as an alphanumeric string".to_owned(),
                },
                AttributeDef {
                    id: " 1 ".to_owned(),
                    name: "StreetType".to_owned(),
                    description: "Type street as an alphanumeric string".to_owned(),
                },
            ],
        };
        assert_eq!(
            forged.validate(),
            Err(AttributionViolation::DuplicateId {
                id: " 1 ".to_owned()
            })
        );

        let round_tripped = AttributeModel::from_json_str(&forged.to_json_string().unwrap());
        assert!(
            matches!(
                round_tripped,
                Err(AttributionError::Violation(
                    AttributionViolation::DuplicateId { .. }
                ))
            ),
            "validate and the read path must agree: {round_tripped:?}"
        );
    }

    /// `/req/core/attribute-model-content` B (§7.1.2.3) — ids that differ
    /// only by surrounding whitespace are the same identifier, in both
    /// encodings: canonicalization runs before the uniqueness check, so
    /// invisible formatting cannot defeat Attr2-B on the XML path.
    #[test]
    fn req_core_attribute_model_content_whitespace_ids_are_not_distinct() {
        let xml = "<AttributeModel>\
             <attributes><id>1</id><name>A</name><description>a</description></attributes>\
             <attributes><id> 1 </id><name>B</name><description>b</description></attributes>\
             </AttributeModel>";
        assert!(
            matches!(
                AttributeModel::from_xml_str(xml),
                Err(AttributionError::Violation(
                    AttributionViolation::DuplicateId { ref id }
                )) if id == "1"
            ),
            "xml: {:?}",
            AttributeModel::from_xml_str(xml)
        );

        let json = r#"{ "attributes": [
            { "id": "1", "name": "A", "description": "a" },
            { "id": " 1 ", "name": "B", "description": "b" }
        ] }"#;
        assert!(matches!(
            AttributeModel::from_json_str(json),
            Err(AttributionError::Violation(
                AttributionViolation::DuplicateId { .. }
            ))
        ));
    }

    /// `/req/core/attribute-model-content` B (§7.1.2.3) — a numeric id
    /// the canonicalizing pre-pass cannot carry (negative, fractional, or
    /// wider than `u64`) is refused with a message that names the fix,
    /// not serde's re-typed view of the literal.
    #[test]
    fn req_core_attribute_model_content_numeric_id_message() {
        for literal in ["-1", "1.5", "18446744073709551616"] {
            let document = format!(
                r#"{{ "attributes": [ {{ "id": {literal}, "name": "A", "description": "a" }} ] }}"#
            );
            match AttributeModel::from_json_str(&document) {
                Err(AttributionError::Serialization(message)) => assert!(
                    message.contains("quote the value"),
                    "expected an actionable message for id {literal}: {message}"
                ),
                other => panic!("expected a Serialization failure for id {literal}: {other:?}"),
            }
        }
    }

    /// Requirement Attr1-C (§7.1.2.1) — the schema file is
    /// `vector_attributes.<ext>` with `<ext>` either `xml` or `json`:
    /// exact stem and exact lowercase extension, nothing else.
    #[test]
    fn req_core_attribute_model_file_name_rules() {
        assert_eq!(
            parse_file_name("vector_attributes.json"),
            Ok(MetadataEncoding::Json)
        );
        assert_eq!(
            parse_file_name("vector_attributes.xml"),
            Ok(MetadataEncoding::Xml)
        );
        for bad in [
            "vector_attributes.txt",
            "vector_attributes",
            "Vector_Attributes.json",
            "vector_attributes.JSON",
            "vector_attributes.Xml",
            "vector_attributes.json.bak",
            "attributes.json",
        ] {
            assert_eq!(
                parse_file_name(bad),
                Err(AttributionViolation::InvalidFileName {
                    name: bad.to_owned()
                }),
                "expected invalid file name: {bad}"
            );
        }

        assert_eq!(
            file_name_for(MetadataEncoding::Json).as_deref(),
            Some("vector_attributes.json")
        );
        assert_eq!(
            file_name_for(MetadataEncoding::Xml).as_deref(),
            Some("vector_attributes.xml")
        );
        assert_eq!(file_name_for(MetadataEncoding::Gpkg), None);
    }

    /// Requirements Attr1-C and Attr2 (§7.1.2.1/.3) — the datastore-level
    /// entry point for the Attribution class: the document's file name
    /// selects the parser (Attr1-C), and the parse validates the content
    /// (Attr2). A name that is not `vector_attributes.<json|xml>` is
    /// `InvalidFileName`; bytes that are not a parseable model are
    /// `Malformed` (a conformance finding, not an I/O failure — the file
    /// was read successfully, it simply is not an attribute model).
    #[test]
    fn req_core_attribute_model_document_entry_point_validates() {
        let model = street_fixture();
        let json = model.to_json_string().unwrap();
        assert_eq!(
            validate_attribute_model_document("vector_attributes.json", &json).unwrap(),
            model
        );

        let xml = model.to_xml_string().unwrap();
        assert_eq!(
            validate_attribute_model_document("vector_attributes.xml", &xml).unwrap(),
            model
        );

        // Attr1-C: the name is the canonical one, read literally.
        assert_eq!(
            validate_attribute_model_document("attributes.json", &json),
            Err(AttributionViolation::InvalidFileName {
                name: "attributes.json".to_owned()
            })
        );

        // Unparseable bytes are a SHALL finding, not an operational error.
        assert!(matches!(
            validate_attribute_model_document("vector_attributes.json", "{not json"),
            Err(AttributionViolation::Malformed { .. })
        ));

        // Attr2-A: a parseable but empty model is still a violation.
        assert_eq!(
            validate_attribute_model_document("vector_attributes.json", r#"{"attributes":[]}"#),
            Err(AttributionViolation::EmptyModel)
        );
    }
}
