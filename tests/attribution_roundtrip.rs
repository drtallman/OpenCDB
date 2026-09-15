//! Integration: the attribute model on a real datastore (Requirements
//! Attr1/PAttr1/Attr2, §7.1.2) — create → write → reopen → read back →
//! validate, for both metadata encodings. Proves the schema file rides
//! `global_metadata/` without disturbing Annex A `/conf/minimal-core`
//! conformance.

use opencdb::{
    AttributeDef, AttributeModel, CdbDatastore, DatastoreSeed, RequirementsClass, SimulationProfile,
};

/// The spec's §7.1.2.3 example table, verbatim (including its stray
/// closing parenthesis), plus PAttr1's supplementary external-schema URI.
fn street_model() -> AttributeModel {
    AttributeModel {
        schema_uri: Some("https://example.org/schemas/street.xsd".to_owned()),
        attributes: vec![
            AttributeDef {
                id: "1".to_owned(),
                name: "StreetName".to_owned(),
                description: "Name of a street as an alphanumeric string".to_owned(),
            },
            AttributeDef {
                id: "2".to_owned(),
                name: "StreetType".to_owned(),
                description: "Type street as an alphanumeric string (Interstate, Arterial, . . .)"
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

/// `/req/core/attribute-model` A–C + `/req/core/attribute-model-content`
/// A–D + `per/core/attribute-schema-uri` (§7.1.2) on a JSON datastore:
/// the model written through the facade survives a reopen equal, and the
/// datastore stays fully conformant with the schema file present.
#[test]
fn req_core_attributes_roundtrip_json() {
    let tmp = tempfile::tempdir().unwrap();
    let profile = SimulationProfile::json();
    let seed = DatastoreSeed::new("doi:cdb.attr", "Attributed", "Attribution roundtrip", "ops");
    let store = CdbDatastore::create(tmp.path(), &profile, seed).unwrap();

    let model = street_model();
    let path = store.write_attribute_model(&model).unwrap();
    assert!(path.ends_with("global_metadata/vector_attributes.json"));

    let reopened = CdbDatastore::open(store.root()).unwrap();
    assert_eq!(reopened.attribute_model().unwrap(), Some(model));
    let report = reopened.validate(&profile).unwrap();
    assert!(report.is_conformant(), "report: {report}");
    for &class in RequirementsClass::MANDATORY {
        assert!(
            report.warnings(class).is_empty(),
            "{class} should have no warnings with the schema file present: {report}"
        );
    }
}

/// The XML twin (§7.1.2): the declared encoding picks
/// `vector_attributes.xml`; equality and conformance hold the same.
#[test]
fn req_core_attributes_roundtrip_xml() {
    let tmp = tempfile::tempdir().unwrap();
    let profile = SimulationProfile::xml();
    let seed = DatastoreSeed::new("doi:cdb.attr", "Attributed", "Attribution roundtrip", "ops");
    let store = CdbDatastore::create(tmp.path(), &profile, seed).unwrap();

    let model = street_model();
    let path = store.write_attribute_model(&model).unwrap();
    assert!(path.ends_with("global_metadata/vector_attributes.xml"));

    let reopened = CdbDatastore::open(store.root()).unwrap();
    assert_eq!(reopened.attribute_model().unwrap(), Some(model));
    let report = reopened.validate(&profile).unwrap();
    assert!(report.is_conformant(), "report: {report}");
    for &class in RequirementsClass::MANDATORY {
        assert!(
            report.warnings(class).is_empty(),
            "{class} should have no warnings with the schema file present: {report}"
        );
    }
}
