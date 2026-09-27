mod fixture;

use cdb_migrator::metadata_input::MetadataManifest;
use cdb_migrator::migrate::migrate;
use cdb_migrator::plan::{plan, ExtrasPolicy, MigrationPlan, PlanOptions};
use cdb_migrator::{Cdb1Entry, Cdb1Tree};
use opencdb::{metadata::ResourceMetadata, CdbDatastore};
use serde_json::{json, Value};
use std::{fs, path::Path};

struct Temp {
    dir: tempfile::TempDir,
    physical: std::path::PathBuf,
}
impl Temp {
    fn path(&self) -> &Path {
        &self.physical
    }
    fn keep(self) -> std::path::PathBuf {
        let _ = self.dir.keep();
        self.physical
    }
}
fn temp() -> Temp {
    let dir = tempfile::tempdir().unwrap();
    let physical = dir.path().canonicalize().unwrap();
    Temp { dir, physical }
}

const TIME: &str = "2026-09-20T12:00:00Z";
const DESCRIPTOR: &str = "cdb_migrator-descriptor.json";
const REPORT: &str = "cdb_migrator-migration-report.json";

fn prepared(root: &Path) -> (Cdb1Tree, MigrationPlan) {
    fixture::build_1x_tree(root);
    let tree = Cdb1Tree::open(root).unwrap();
    let inv = tree.inventory().unwrap();
    let resources: Vec<_> = inv.entries.iter().filter(|e| matches!(e, Cdb1Entry::Tile(_) | Cdb1Entry::Global{..})).enumerate().map(|(i,e)| {
        let coverage = matches!(e, Cdb1Entry::Tile(t) if t.file.dataset == 1);
        let mut record = json!({"ID":format!("id{i}"),"type":"dataset","title":"Operator title","description":"Explicit assertion","keywords":["declared"]});
        if coverage { record["domainSet"] = json!({"uom":"m","grid_cell_encoding":"value-is-center"}); }
        json!({"source_path":e.rel_path().unwrap(),"record":record,"coverage":coverage,"measurement_values":false,"generated_faces":false})
    }).collect();
    let options = PlanOptions {
        extras: ExtrasPolicy::Skip,
        metadata: Some(MetadataManifest::from_json_str(&json!({"resources":resources,"attribute_model":{"attributes":[{"id":"height","name":"Height","description":"Declared height"}]}}).to_string()).unwrap()),
        ..Default::default()
    };
    let p = plan(&tree, &inv, &options).unwrap();
    (tree, p)
}

/// Migration contract: facade materialization preserves opaque bytes, records,
/// filename-derived addresses, and the complete unmodified conformance report.
#[test]
fn mig_roundtrip_synthetic_tree_is_conformant() {
    let src = temp();
    let (tree, p) = prepared(src.path());
    let out = temp();
    let report = migrate(&tree, &p, out.path(), Some(TIME)).unwrap();
    assert!(
        report.conformant,
        "{}",
        serde_json::to_string_pretty(&report).unwrap()
    );
    assert_eq!((report.migrated, report.extras), (4, 1));
    assert_eq!(report.skipped, vec!["stray notes.txt"]);
    assert!(report.entries.iter().all(|e| e.verified));
    let store = CdbDatastore::open(out.path().join("cdb")).unwrap();
    for m in p.moves() {
        assert_eq!(
            fs::read(&m.source_path).unwrap(),
            fs::read(store.resolve(&m.target).unwrap()).unwrap()
        );
    }
    for r in p.resources() {
        let record = ResourceMetadata::from_json_str(
            &fs::read_to_string(store.resolve(&r.target).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            record.updated.unwrap(),
            opencdb::metadata::temporal::parse_datetime(TIME).unwrap()
        );
        let link = record
            .associations
            .iter()
            .find(|a| a.rel == "item")
            .unwrap();
        assert_eq!(
            store.resolve(&link.href).unwrap(),
            store.resolve(&r.payload).unwrap()
        );
        assert_eq!(
            fs::read(store.resolve(&link.href).unwrap()).unwrap(),
            fs::read(src.path().join(&r.source)).unwrap()
        );
    }
    assert_eq!(
        store.attribute_model().unwrap(),
        p.operator_metadata().attribute_model
    );
    let wire = serde_json::to_value(&report).unwrap();
    assert_eq!(
        wire["conformance"],
        serde_json::to_value(store.validate(p.profile()).unwrap()).unwrap()
    );
    assert_eq!(wire["descriptor"], *p.descriptor());
    assert_eq!(wire["source_root"], src.path().to_str().unwrap());
    assert_eq!(
        wire["global_metadata"],
        serde_json::to_value(store.global_metadata().unwrap()).unwrap()
    );
    assert_eq!(wire["generated_records"].as_array().unwrap().len(), 3);
    for resource in wire["generated_records"].as_array().unwrap() {
        assert_eq!(resource["record"]["updated"], TIME);
        let record: Value = serde_json::from_slice(
            &fs::read(store.resolve(resource["target"].as_str().unwrap()).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(resource["record"], record);
    }
    assert_eq!(
        wire["operator_metadata"],
        serde_json::to_value(p.operator_metadata()).unwrap()
    );
    assert_eq!(wire["observed"].as_array().unwrap().len(), 5);
    assert_eq!(wire["declared"], "1.2");
    assert_eq!(
        wire["source_declarations"]["version"]["metadata_standard"],
        "DCAT"
    );
    assert_eq!(wire["payload_semantics"], "unchecked");
    assert_eq!(wire["embedded_references"], "unchecked");
    assert!(wire["limitations"]
        .as_str()
        .unwrap()
        .contains("case-sensitive"));
    let addresses: Vec<_> = wire["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["address_verification"].as_object())
        .collect();
    assert_eq!(addresses.len(), 2);
    assert_eq!(
        (
            addresses[0]["lod"].as_i64(),
            addresses[0]["row"].as_u64(),
            addresses[0]["col"].as_u64()
        ),
        (Some(1), Some(114), Some(124))
    );
    assert_eq!(
        (
            addresses[1]["lod"].as_i64(),
            addresses[1]["row"].as_u64(),
            addresses[1]["col"].as_u64()
        ),
        (Some(-5), Some(57), Some(62))
    );
    assert!(addresses
        .iter()
        .all(|a| a["verified"] == true && a["basis"] == "filename-derived"));
    assert_eq!(store.versions().unwrap().len(), 4);
    assert_eq!(
        store.global_metadata().unwrap().created,
        opencdb::metadata::temporal::parse_datetime(TIME).unwrap()
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(out.path().join(REPORT)).unwrap()).unwrap(),
        wire
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(out.path().join(DESCRIPTOR)).unwrap()).unwrap(),
        *p.descriptor()
    );
    assert!(fs::read(out.path().join(DESCRIPTOR))
        .unwrap()
        .ends_with(b"\n"));
    if let Ok(keep) = std::env::var("CDB_MIGRATOR_KEEP_OUTPUT") {
        // Manual external cdb-lint check uses this real test output; opt-in only.
        fs::rename(out.keep(), keep).unwrap();
    }
}

/// Fixed timestamps make datastore bytes and the complete report repeatable.
#[test]
fn mig_roundtrip_fixed_timestamp_is_deterministic() {
    let src = temp();
    let (tree, p) = prepared(src.path());
    let a = temp();
    let b = temp();
    migrate(&tree, &p, a.path(), Some(TIME)).unwrap();
    migrate(&tree, &p, b.path(), Some(TIME)).unwrap();
    fn files(root: &Path, dir: &Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
        let mut result = std::collections::BTreeMap::new();
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                result.extend(files(root, &path));
            } else {
                result.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
                );
            }
        }
        result
    }
    let af = files(a.path(), a.path());
    let bf = files(b.path(), b.path());
    assert_eq!(af.keys().collect::<Vec<_>>(), bf.keys().collect::<Vec<_>>());
    for (path, bytes) in af {
        if path == Path::new(REPORT) {
            let mut av: Value = serde_json::from_slice(&bytes).unwrap();
            let mut bv: Value = serde_json::from_slice(&bf[&path]).unwrap();
            assert_eq!(
                av["conformance"]["root"],
                a.path().join("cdb").to_str().unwrap()
            );
            assert_eq!(
                bv["conformance"]["root"],
                b.path().join("cdb").to_str().unwrap()
            );
            av["conformance"]["root"] = Value::Null;
            bv["conformance"]["root"] = Value::Null;
            assert_eq!(av, bv);
        } else {
            assert_eq!(bytes, bf[&path], "{path:?}");
        }
    }
}

/// Preflight rejects all occupied result paths without touching their contents.
#[test]
fn mig_refuses_existing_outputs_before_writing() {
    for name in [
        "cdb",
        DESCRIPTOR,
        REPORT,
        "cdb_migrator-descriptor.json.partial",
        "cdb_migrator-migration-report.json.partial",
    ] {
        let src = temp();
        let (tree, p) = prepared(src.path());
        let out = temp();
        fs::write(out.path().join(name), b"original").unwrap();
        assert!(
            migrate(&tree, &p, out.path(), Some(TIME)).is_err(),
            "{name}"
        );
        assert_eq!(fs::read(out.path().join(name)).unwrap(), b"original");
        assert_eq!(
            fs::read_dir(out.path()).unwrap().count(),
            1,
            "must preflight {name}"
        );
    }
}
/// No output may be created inside the source, including absent suffix paths.
#[test]
fn mig_refuses_nested_output_before_mutation() {
    let src = temp();
    let (tree, p) = prepared(src.path());
    let nested = src.path().join("new_output/deeper");
    assert!(migrate(&tree, &p, &nested, Some(TIME)).is_err());
    assert!(!src.path().join("new_output").exists());
    assert!(migrate(&tree, &p, src.path(), Some(TIME)).is_err());
    assert!(!src.path().join("cdb").exists());
    p.verify_source().unwrap();
}
/// Existing physical ancestors resolve case aliases before any absent suffix.
#[test]
fn mig_refuses_case_alias_nested_output() {
    let dir = temp();
    let source = dir.path().join("Source");
    fs::create_dir(&source).unwrap();
    let (tree, p) = prepared(&source);
    let alias = dir.path().join("source");
    if !alias.exists() {
        return;
    } // Host-aware: no alias exists on a case-sensitive FS.
    let nested = alias.join("absent/deeper");
    assert!(migrate(&tree, &p, &nested, Some(TIME)).is_err());
    assert!(!source.join("absent").exists());
    p.verify_source().unwrap();
}
/// Destination parents may be new, provided their existing ancestors are safe.
#[test]
fn mig_creates_absent_safe_parent() {
    let src = temp();
    let (tree, p) = prepared(src.path());
    let out = temp();
    let parent = out.path().join("new/deep");
    assert!(migrate(&tree, &p, &parent, None).unwrap().conformant);
}
/// Operator mistakes and observable source changes refuse before output creation.
#[test]
fn mig_refuses_stale_source_other_root_and_bad_timestamp() {
    let src = temp();
    let (tree, p) = prepared(src.path());
    let out = temp();
    for timestamp in ["invalid", "2026-09-20T12:00:00+01:00"] {
        assert!(matches!(
            migrate(&tree, &p, out.path(), Some(timestamp)),
            Err(cdb_migrator::Cdb1Error::Refused(_))
        ));
    }
    let other = temp();
    let (other_tree, _) = prepared(other.path());
    assert!(migrate(&other_tree, &p, out.path(), Some(TIME)).is_err());
    fs::write(
        src.path().join("stray notes.txt"),
        b"changed skipped source",
    )
    .unwrap();
    assert!(migrate(&tree, &p, out.path(), Some(TIME)).is_err());
    assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
}
#[cfg(unix)]
/// Symlinks/special files introduced after planning are never followed; output
/// symlink ancestors and dangling companions must also refuse without writes.
#[test]
fn mig_refuses_links_and_special_files() {
    use std::os::unix::{fs::symlink, net::UnixListener};
    let src = temp();
    let (tree, p) = prepared(src.path());
    let out = temp();
    let real = temp();
    symlink(real.path(), out.path().join("alias")).unwrap();
    assert!(migrate(&tree, &p, &out.path().join("alias"), Some(TIME)).is_err());
    assert_eq!(fs::read_dir(real.path()).unwrap().count(), 0);
    fs::remove_file(out.path().join("alias")).unwrap();
    symlink(real.path().join("missing"), out.path().join(REPORT)).unwrap();
    assert!(migrate(&tree, &p, out.path(), Some(TIME)).is_err());
    assert!(!out.path().join("cdb").exists());
    fs::remove_file(out.path().join(REPORT)).unwrap();
    fs::remove_file(src.path().join("stray notes.txt")).unwrap();
    symlink(real.path(), src.path().join("stray notes.txt")).unwrap();
    assert!(migrate(&tree, &p, out.path(), Some(TIME)).is_err());
    fs::remove_file(src.path().join("stray notes.txt")).unwrap();
    let _socket = UnixListener::bind(src.path().join("stray notes.txt")).unwrap();
    assert!(migrate(&tree, &p, out.path(), Some(TIME)).is_err());
    assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
}

/// SHOULD findings and unchecked payload classes survive unchanged, while
/// directory disagreements remain source findings with filename-derived evidence.
#[test]
fn mig_preserves_warnings_unchecked_classes_and_reader_findings() {
    let src = temp();
    let (_, base) = prepared(src.path());
    fs::remove_file(src.path().join("Metadata/Version.xml")).unwrap();
    let old = base
        .resources()
        .iter()
        .find(|r| r.source.contains("201_RoadNetwork"))
        .unwrap()
        .source
        .clone();
    let new = old.replace("Tiles/N32/", "Tiles/N33/");
    fs::create_dir_all(src.path().join(&new).parent().unwrap()).unwrap();
    fs::rename(src.path().join(&old), src.path().join(&new)).unwrap();
    let mut options = base.options().clone();
    let metadata = options.metadata.as_mut().unwrap();
    for r in &mut metadata.resources {
        if r.source_path == old {
            r.source_path = new.clone();
        }
        if let Some(domain) = r.record.domain_set.as_mut() {
            domain.uom = "urn:example:metres".into();
        }
        r.measurement_values = true;
        r.record.uom = Some(opencdb::metadata::UnitOfMeasure::Meters);
        r.generated_faces = true;
        r.record.winding_order = Some(opencdb::topology::WindingOrder::Counterclockwise);
    }
    let tree = Cdb1Tree::open(src.path()).unwrap();
    let p = plan(&tree, &tree.inventory().unwrap(), &options).unwrap();
    let out = temp();
    let report = migrate(&tree, &p, out.path(), Some(TIME)).unwrap();
    assert!(report.conformant);
    let wire = serde_json::to_value(&report).unwrap();
    assert!(!wire["reader_findings"].as_array().unwrap().is_empty());
    let observed = wire["observed"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["source"] == new)
        .unwrap();
    assert!(!observed["findings"].as_array().unwrap().is_empty());
    let classes = wire["conformance"]["classes"].as_array().unwrap();
    for class in ["geometry", "topology"] {
        assert_eq!(
            classes.iter().find(|c| c["class"] == class).unwrap()["content"],
            "unchecked"
        );
    }
    let warnings = classes.iter().find(|c| c["class"] == "coverages").unwrap()["warnings"]
        .as_array()
        .unwrap();
    assert!(!warnings.is_empty());
    assert!(warnings[0]["code"].is_string());
    assert_eq!(
        wire["conformance"],
        serde_json::to_value(
            CdbDatastore::open(out.path().join("cdb"))
                .unwrap()
                .validate(p.profile())
                .unwrap()
        )
        .unwrap()
    );
}
/// An OS write failure leaves a visibly incomplete datastore and no report.
#[test]
fn mig_operational_failure_has_no_completed_report() {
    let src = temp();
    fs::write(src.path().join("notes.txt"), b"opaque").unwrap();
    let tree = Cdb1Tree::open(src.path()).unwrap();
    let mut opts = PlanOptions {
        extras: ExtrasPolicy::Carry,
        ..Default::default()
    };
    opts.rename_map.insert(
        "notes.txt".into(),
        format!("extras/{}.txt", "x".repeat(300)),
    );
    let p = plan(&tree, &tree.inventory().unwrap(), &opts).unwrap();
    let out = temp();
    assert!(migrate(&tree, &p, out.path(), Some(TIME)).is_err());
    assert!(out.path().join("cdb").is_dir());
    assert!(!out.path().join(REPORT).exists());
    assert!(!out.path().join(DESCRIPTOR).exists());
    assert_eq!(fs::read(src.path().join("notes.txt")).unwrap(), b"opaque");
}
/// Carry counts include extras, while an extras-only plan requires no invented
/// operator records and a binary file larger than a comparison buffer survives.
#[test]
fn mig_carries_opaque_extras_without_invented_records() {
    let src = temp();
    let bytes: Vec<u8> = (0..150_001).map(|i| (i % 251) as u8).collect();
    fs::write(src.path().join("DATA.bin"), &bytes).unwrap();
    let tree = Cdb1Tree::open(src.path()).unwrap();
    let p = plan(
        &tree,
        &tree.inventory().unwrap(),
        &PlanOptions {
            extras: ExtrasPolicy::Carry,
            ..Default::default()
        },
    )
    .unwrap();
    let out = temp();
    let report = migrate(&tree, &p, out.path(), None).unwrap();
    assert_eq!((report.migrated, report.extras), (1, 1));
    assert!(report.skipped.is_empty());
    assert!(report.operator_metadata.resources.is_empty());
    assert!(report.generated_records.is_empty());
    assert_eq!(
        fs::read(out.path().join("cdb/extras/data.bin")).unwrap(),
        bytes
    );
}
