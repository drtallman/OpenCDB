//! The optional metadata codec preserves the CLI's usage/finding/operational boundary.
use cdb_lint::{Env, run};
use serde_json::{Value, json};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

fn write_descriptor(parent: &Path, model: Option<Value>) -> PathBuf {
    std::fs::write(
        parent.join("crs.wkt"),
        opencdb::profiles::simulation::WGS84_2D_WKT,
    )
    .unwrap();
    let mut descriptor = json!({
        "name": "opencdb-gpkg-v1", "case_rule": "PascalCase", "language": "en",
        "storage_crs_wkt_path": "crs.wkt", "metadata_standard": "DCAT",
        "metadata_encoding": "gpkg", "uom": "M", "conformance_classes": "all",
        "resource_metadata_dir": "metadata", "known_extensions": ["wkt"]
    });
    if let Some(model) = model {
        descriptor["attribute_model"] = model;
    }
    let path = parent.join("profile.json");
    std::fs::write(&path, descriptor.to_string()).unwrap();
    path
}

struct Run {
    code: i32,
    out: String,
    err: String,
}
fn invoke(args: &[OsString]) -> Run {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run(
        args,
        &mut out,
        &mut err,
        &Env {
            no_color: false,
            stdout_is_terminal: false,
        },
    );
    Run {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}
fn lint(descriptor: &Path, root: &Path, format: &str, extra: &[OsString]) -> Run {
    let mut args = vec![
        OsString::from("--profile-file"),
        descriptor.as_os_str().to_owned(),
        "--format".into(),
        format.into(),
    ];
    args.extend_from_slice(extra);
    args.push(root.as_os_str().to_owned());
    invoke(&args)
}

#[cfg(not(feature = "gpkg-metadata"))]
#[test]
fn cli_gpkg_unavailable_descriptor_is_usage_before_datastore_access() {
    let tmp = tempfile::tempdir().unwrap();
    let descriptor = write_descriptor(tmp.path(), None);
    for format in ["text", "json", "sarif"] {
        let result = lint(&descriptor, &tmp.path().join("absent"), format, &[]);
        assert_eq!(result.code, 2, "{}", result.err);
        assert!(result.err.contains("gpkg-metadata"), "{}", result.err);
        assert!(result.out.is_empty());
        assert!(!tmp.path().join("absent").exists());
    }
}

#[test]
fn cli_gpkg_required_attribute_model_is_usage_before_datastore_access() {
    let tmp = tempfile::tempdir().unwrap();
    let descriptor = write_descriptor(
        tmp.path(),
        Some(json!({"attributes":[{"id":"1","name":"Name","description":"Label"}]})),
    );
    for format in ["text", "json", "sarif"] {
        let result = lint(&descriptor, &tmp.path().join("absent"), format, &[]);
        assert_eq!(result.code, 2, "{}", result.err);
        assert!(result.out.is_empty());
        #[cfg(feature = "gpkg-metadata")]
        assert!(
            result.err.contains("Attr1-C") && result.err.contains("Metadata5"),
            "{}",
            result.err
        );
    }
}

#[test]
fn cli_gpkg_explain_names_geopackage_in_every_build() {
    let result = invoke(&["explain".into(), "/rec/geopackage/user-data-table".into()]);
    assert_eq!(result.code, 0);
    assert!(result.out.contains("OGC 12-128r15 §2"));
    assert!(!result.out.contains("OGC 23-034"));
}

#[test]
fn cli_gpkg_payload_outside_metadata_convention_remains_opaque() {
    let tmp = tempfile::tempdir().unwrap();
    let profile = opencdb::SimulationProfile::json();
    let store = opencdb::CdbDatastore::create(
        tmp.path(),
        &profile,
        opencdb::DatastoreSeed::new("id", "Title", "Description", "ops"),
    )
    .unwrap();
    let path = store.resolve("/Tiles/Payload.gpkg").unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"opaque non-SQLite payload").unwrap();
    let result = invoke(&[
        "--profile".into(),
        "simulation".into(),
        "--encoding".into(),
        "json".into(),
        store.root().as_os_str().to_owned(),
    ]);
    assert_eq!(result.code, 0, "{}\n{}", result.out, result.err);
    assert_eq!(std::fs::read(path).unwrap(), b"opaque non-SQLite payload");
}

#[cfg(feature = "gpkg-metadata")]
mod enabled {
    use super::*;
    use cdb_lint::profile::DescriptorProfile;
    use opencdb::{CdbDatastore, DatastoreSeed};

    fn store() -> (tempfile::TempDir, PathBuf, DescriptorProfile, CdbDatastore) {
        let tmp = tempfile::tempdir().unwrap();
        let descriptor = write_descriptor(tmp.path(), None);
        let profile = DescriptorProfile::load(&descriptor).unwrap();
        let store = CdbDatastore::create(
            tmp.path(),
            &profile,
            DatastoreSeed::new("cli", "CLI", "Gpkg fixture", "ops"),
        )
        .unwrap();
        (tmp, descriptor, profile, store)
    }

    #[test]
    fn cli_gpkg_descriptor_accepts_supported_binding() {
        let (_tmp, _descriptor, profile, store) = store();
        assert!(store.validate(&profile).unwrap().is_conformant());
    }

    /// Report bytes and coverage do not change when warnings change the exit status.
    #[test]
    fn cli_gpkg_formats_preserve_warning_and_report_honesty() {
        let (tmp, descriptor, profile, store) = store();
        let expected = serde_json::to_value(store.validate(&profile).unwrap()).unwrap();
        for format in ["text", "json", "sarif"] {
            let result = lint(&descriptor, store.root(), format, &[]);
            assert_eq!(result.code, 0, "{}", result.err);
            let denied = lint(
                &descriptor,
                store.root(),
                format,
                &["--deny-warnings".into()],
            );
            assert_eq!(denied.code, 1, "{}", denied.err);
            if format == "json" {
                assert_eq!(
                    serde_json::from_str::<Value>(&result.out).unwrap(),
                    expected
                );
                assert_eq!(
                    serde_json::from_str::<Value>(&denied.out).unwrap(),
                    expected
                );
                assert_eq!(expected["classes"].as_array().unwrap().len(), 11);
            } else if format == "text" {
                assert!(result.out.contains("/global_metadata/global_metadata.gpkg"));
                assert!(result.out.contains("warning") || result.out.contains("WARN"));
                for entry in expected["classes"].as_array().unwrap() {
                    assert!(result.out.contains(entry["class"].as_str().unwrap()));
                }
            } else {
                let sarif: Value = serde_json::from_str(&result.out).unwrap();
                let run = &sarif["runs"][0];
                let finding = run["results"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|r| r["ruleId"] == "/rec/geopackage/user-data-table")
                    .unwrap();
                assert_eq!(finding["level"], "warning");
                let rule = run["tool"]["driver"]["rules"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|r| r["id"] == finding["ruleId"])
                    .unwrap();
                assert_eq!(
                    rule["help"]["text"],
                    "OGC 12-128r15 (http://www.opengis.net/doc/IS/geopackage/1.2.1) §2"
                );
                assert_eq!(
                    finding["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
                    "global_metadata/global_metadata.gpkg"
                );
                assert_eq!(run["properties"]["coverage"]["checked"], 5);
                assert_eq!(run["properties"]["coverage"]["none"], 6);
            }
        }
        let baseline = tmp.path().join("baseline.json");
        std::fs::write(&baseline, expected.to_string()).unwrap();
        let result = lint(
            &descriptor,
            store.root(),
            "json",
            &[
                "--baseline".into(),
                baseline.as_os_str().to_owned(),
                "--deny-warnings".into(),
            ],
        );
        assert_eq!(result.code, 0, "{}", result.err);
        assert_eq!(
            serde_json::from_str::<Value>(&result.out).unwrap(),
            expected
        );
    }

    #[test]
    fn cli_gpkg_container_errors_keep_the_four_exit_contract() {
        for unsupported in [true, false] {
            let (_tmp, descriptor, _profile, store) = store();
            let path = store.root().join("global_metadata/global_metadata.gpkg");
            if unsupported {
                let mut bytes = std::fs::read(&path).unwrap();
                bytes[60..64].copy_from_slice(&10400_u32.to_be_bytes());
                std::fs::write(&path, bytes).unwrap();
            } else {
                std::fs::write(&path, b"corrupt container").unwrap();
            }
            for format in ["text", "json", "sarif"] {
                let result = lint(&descriptor, store.root(), format, &[]);
                assert_eq!(
                    result.code,
                    if unsupported { 3 } else { 1 },
                    "{}\n{}",
                    result.out,
                    result.err
                );
                if unsupported {
                    assert!(result.out.is_empty());
                } else {
                    assert!(result.out.contains("/req/core/metadata-encoding"));
                }
            }
        }
    }

    #[test]
    fn cli_gpkg_geometry_and_topology_remain_unchecked_in_all_formats() {
        let (_tmp, descriptor, profile, store) = store();
        let mut record = opencdb::metadata::ResourceMetadata::new("r", "Roads", "Network");
        record.uom = Some(opencdb::metadata::UnitOfMeasure::Meters);
        record.winding_order = Some(opencdb::topology::WindingOrder::Clockwise);
        store
            .write_resource_metadata("/Tiles/metadata/Roads.gpkg", &record)
            .unwrap();
        let expected = serde_json::to_value(store.validate(&profile).unwrap()).unwrap();
        for format in ["text", "json", "sarif"] {
            let result = lint(&descriptor, store.root(), format, &[]);
            assert_eq!(result.code, 0, "{}", result.err);
            match format {
                "text" => assert_eq!(result.out.matches("[UNCHECKED]").count(), 2),
                "json" => {
                    let json: Value = serde_json::from_str(&result.out).unwrap();
                    assert_eq!(json, expected);
                    assert_eq!(
                        json["classes"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter(|c| c["content"] == "unchecked")
                            .count(),
                        2
                    );
                }
                _ => {
                    let json: Value = serde_json::from_str(&result.out).unwrap();
                    assert_eq!(json["runs"][0]["properties"]["coverage"]["unchecked"], 2);
                    assert_eq!(
                        json["runs"][0]["results"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter(|r| r["kind"] == "review")
                            .count(),
                        2
                    );
                }
            }
        }
    }
}
