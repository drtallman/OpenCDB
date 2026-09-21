//! The migration yardstick and its portable cdb-lint descriptor.
//!
//! Generated records use `NoMetadata`: a source schema declaration is
//! provenance, not evidence of schema translation. The profile's metre unit
//! is the global spatial unit (Metadata8), never an inferred payload unit.

use crate::Cdb1Error;
use opencdb::crs::{CrsViolation, StorageCrs};
use opencdb::metadata::{MetadataEncoding, MetadataStandard, UnitOfMeasure};
use opencdb::naming::{CaseRule, StyleGuide};
use opencdb::profiles::{ApplicationProfile, StorageTechnology};
use opencdb::{AttributeModel, RequirementsClass, TilingSchemeId};

/// Stable profile identity; compare full descriptors when comparing parameters.
pub const MIGRATION_PROFILE_NAME: &str = "cdb-migrator-v1";

// Exact compact WKT-2 from cdb-lint's documented profile descriptor.
const WGS84_2D_WKT: &str = r#"GEOGCRS["WGS 84",DATUM["World Geodetic System 1984",ELLIPSOID["WGS 84",6378137,298.257223563,LENGTHUNIT["metre",1.0]]],CS[ellipsoidal,2],AXIS["latitude",north,ORDER[1]],AXIS["longitude",east,ORDER[2]],ANGLEUNIT["degree",0.0174532925199433],ID["EPSG",4326]]"#;

/// Fixed JSON/NoMetadata migration profile with operator-known extensions.
///
/// Extensions use dotless names. They are sorted and deduplicated; collecting
/// valid payload extensions and including generated `wkt` is the planner's
/// responsibility. A metadata-standard override is deliberately unavailable.
#[derive(Debug, Clone)]
pub struct MigrationProfile {
    known_extensions: Vec<String>,
    attribute_model: Option<AttributeModel>,
}

impl MigrationProfile {
    /// Construct the fixed migration profile with its known extensions.
    pub fn new(mut known_extensions: Vec<String>) -> Self {
        known_extensions.sort();
        known_extensions.dedup();
        Self {
            known_extensions,
            attribute_model: None,
        }
    }

    /// Attach an explicit operator model, validated and canonicalized using the
    /// public model parser. The trait and descriptor then carry the same model;
    /// source XML never provides an implicit model.
    pub fn with_attribute_model(mut self, model: AttributeModel) -> Result<Self, Cdb1Error> {
        let content = model
            .to_json_string()
            .map_err(|error| Cdb1Error::Refused(format!("attribute_model: {error}")))?;
        let model = AttributeModel::from_json_str(&content)
            .map_err(|error| Cdb1Error::Refused(format!("attribute_model: {error}")))?;
        self.attribute_model = Some(model);
        Ok(self)
    }
}

impl ApplicationProfile for MigrationProfile {
    fn name(&self) -> &str {
        MIGRATION_PROFILE_NAME
    }
    fn style_guide(&self) -> StyleGuide {
        let mut guide = StyleGuide::new(CaseRule::SnakeCase, "en");
        guide.reserve_name("metadata");
        guide
    }
    fn storage_crs(&self) -> Result<StorageCrs, CrsViolation> {
        StorageCrs::from_wkt(WGS84_2D_WKT)
    }
    fn metadata_standard(&self) -> MetadataStandard {
        MetadataStandard::NoMetadata
    }
    fn metadata_encoding(&self) -> MetadataEncoding {
        MetadataEncoding::Json
    }
    fn uom(&self) -> UnitOfMeasure {
        UnitOfMeasure::Meters
    }
    fn storage_technology(&self) -> StorageTechnology {
        StorageTechnology::FileSystem
    }
    fn conformance_classes(&self) -> Vec<RequirementsClass> {
        RequirementsClass::ALL.to_vec()
    }
    fn is_resource_metadata(&self, logical_path: &str) -> bool {
        let mut components = logical_path.rsplit('/');
        let file_name = components.next().unwrap_or_default();
        components
            .next()
            .is_some_and(|dir| dir.eq_ignore_ascii_case("metadata"))
            && opencdb::naming::split_extension(file_name)
                .1
                .is_some_and(|extension| {
                    ["json", "xml", "gpkg"]
                        .iter()
                        .any(|known| known.eq_ignore_ascii_case(extension))
                })
    }
    fn tiling_scheme(&self) -> Option<TilingSchemeId> {
        Some(TilingSchemeId::Cdb1GlobalGrid)
    }
    fn attribute_model(&self) -> Option<AttributeModel> {
        self.attribute_model.clone()
    }
    fn known_extensions(&self) -> Vec<String> {
        self.known_extensions.clone()
    }
}

/// Return the generated standard and a note preserving a source declaration.
///
/// Every supplied declaration (even `DCAT` or `NoMetadata`) remains provenance;
/// no source schema is translated. Absence requires no mapping note.
pub fn map_metadata_standard(raw: Option<&str>) -> (MetadataStandard, Option<String>) {
    (MetadataStandard::NoMetadata, raw.map(|value| format!(
        "Source metadata standard {value:?} retained as provenance; generated records use NoMetadata; source schema was not translated."
    )))
}

/// The complete cdb-lint `--profile-file` document for this effective profile.
pub fn descriptor(profile: &MigrationProfile) -> serde_json::Value {
    serde_json::json!({
        "name": profile.name(), "case_rule": "Snake_case", "language": "en",
        "storage_crs_wkt": WGS84_2D_WKT,
        "metadata_standard": profile.metadata_standard().as_str(),
        "metadata_encoding": profile.metadata_encoding().as_str(),
        "uom": profile.uom().as_str(),
        "storage_technology": profile.storage_technology().as_str(),
        "conformance_classes": "all", "tiling_scheme": "CDB1GlobalGrid",
        "resource_metadata_dir": "metadata", "root_folder_name": profile.root_folder_name(),
        "known_extensions": profile.known_extensions(), "attribute_model": profile.attribute_model()
    })
}

/// Companion descriptor filename, beside the datastore root.
pub const DESCRIPTOR_FILE: &str = "cdb_migrator-descriptor.json";
/// Completed report filename, beside the datastore root.
pub const REPORT_FILE: &str = "cdb_migrator-migration-report.json";
/// One collection per copied source file; the frozen journal has six digits.
pub const MAX_MIGRATED_FILES: usize = opencdb::versioning::SEQUENCE_MAX as usize;

/// A copied payload. Address evidence applies only to parsed tile filenames.
#[derive(Debug, serde::Serialize)]
pub struct ReportEntry {
    pub source: String,
    pub target: String,
    pub bucket: crate::plan::Bucket,
    pub bytes: u64,
    pub verified: bool,
    pub address_verification: Option<serde_json::Value>,
}
/// Completed materialization, including the unchanged validator wire report.
/// `conformant` is solely the verdict under `descriptor`, never certification
/// of opaque payload semantics or embedded-reference usability.
#[derive(Debug, serde::Serialize)]
pub struct MigrationReport {
    pub conformant: bool,
    pub migrated: usize,
    pub extras: usize,
    pub skipped: Vec<String>,
    pub declared: Option<String>,
    pub standard_note: Option<String>,
    pub entries: Vec<ReportEntry>,
    pub conformance: opencdb::ConformanceReport,
    pub descriptor: serde_json::Value,
    pub source_root: std::path::PathBuf,
    pub global_metadata: opencdb::metadata::GlobalMetadata,
    pub source_declarations: serde_json::Value,
    pub reader_findings: Vec<serde_json::Value>,
    pub observed: Vec<serde_json::Value>,
    pub generated_records: Vec<crate::plan::PlannedResource>,
    pub operator_metadata: crate::metadata_input::MetadataManifest,
    pub seed: crate::plan::PlannedSeed,
    pub rename_map: std::collections::BTreeMap<String, String>,
    pub extras_policy: crate::plan::ExtrasPolicy,
    pub payload_semantics: &'static str,
    pub embedded_references: &'static str,
    pub limitations: &'static str,
}

/// Copy a validated plan and verify each output against its source.
///
/// A supplied timestamp uses the public strict UTC RFC3339 parser and sets both
/// global creation and every collection/linked-record update. One whole file
/// is buffered per collection, ordered by first two target components then
/// target. At most [`MAX_MIGRATED_FILES`] source files can be carried in v1.
/// Memory also includes inventory, plan, operator manifest, generated metadata,
/// journal metadata and report; bounded payload batches do not mean constant
/// total memory. No large-corpus capacity claim is made.
///
/// Sources and destination ancestors must remain operator-controlled and
/// stable. Observable changes refuse; hostile simultaneous ancestor replacement
/// is not prevented. Failed writes can leave partial output, but no completed
/// migration report. A completed validation with findings returns `Ok` with
/// `conformant == false` and the full findings, for CLI exit-code handling.
pub fn migrate(
    tree: &crate::Cdb1Tree,
    plan: &crate::plan::MigrationPlan,
    out_parent: &std::path::Path,
    timestamp: Option<&str>,
) -> Result<MigrationReport, Cdb1Error> {
    use opencdb::versioning::PendingCollection;
    use opencdb::{CdbDatastore, DatastoreSeed};
    use std::fs;
    let applied = timestamp
        .map(opencdb::metadata::temporal::parse_datetime)
        .transpose()
        .map_err(|e| Cdb1Error::Refused(format!("timestamp: {e}")))?;
    plan.verify_tree(tree)?;
    let out_parent = preflight(plan, out_parent)?;
    fs::create_dir_all(&out_parent).map_err(|e| Cdb1Error::Io(out_parent.clone(), e))?;
    let parent_anchor = Anchors::capture(&out_parent)?;
    let seed = plan.seed();
    let mut seed = DatastoreSeed::new(&seed.id, &seed.title, &seed.description, &seed.contact);
    if let Some(time) = applied {
        seed = seed.created(time);
    }
    let store = CdbDatastore::create(&out_parent, plan.profile(), seed).map_err(operational)?;
    let root_anchor = Anchors::capture(store.root())?;
    let mut global = store.global_metadata().map_err(operational)?;
    global.tiling_scheme = Some(opencdb::TilingScheme::cdb1_global_grid());
    store.write_global_metadata(&global).map_err(operational)?;
    if let Some(model) = plan.profile().attribute_model() {
        store.write_attribute_model(&model).map_err(operational)?;
    }
    for resource in plan.resources() {
        root_anchor.verify()?;
        let record_path = store.resolve(&resource.target).map_err(operational)?;
        safe_directory(
            record_path
                .parent()
                .ok_or_else(|| operational("record has no parent"))?,
        )?;
        require_absent(&record_path)?;
        store
            .write_resource_metadata(&resource.target, &resource.record)
            .map_err(operational)?;
    }
    let mut moves: Vec<_> = plan.moves().iter().collect();
    moves.sort_by_cached_key(|m| {
        (
            m.target.split('/').take(2).collect::<Vec<_>>().join("/"),
            m.target.clone(),
        )
    });
    let resources: std::collections::BTreeMap<_, _> = plan
        .resources()
        .iter()
        .map(|r| (r.source.as_str(), r))
        .collect();
    let mut entries = Vec::with_capacity(moves.len());
    for m in moves {
        parent_anchor.verify()?;
        root_anchor.verify()?;
        safe_regular(&m.source_path)?;
        let target = store.resolve(&m.target).map_err(operational)?;
        safe_directory(
            target
                .parent()
                .ok_or_else(|| operational("target has no parent"))?,
        )?;
        require_absent(&target)?;
        let bytes =
            fs::read(&m.source_path).map_err(|e| Cdb1Error::Io(m.source_path.clone(), e))?;
        let size = bytes.len() as u64;
        let mut pending = PendingCollection::new().create(&m.target, bytes);
        if let Some(record) = resources.get(m.source.as_str()) {
            pending = pending.for_record(&record.target);
        }
        if let Some(time) = applied {
            store.apply_collection_at(pending, time)
        } else {
            store.apply_collection(pending)
        }
        .map_err(operational)?;
        let target = store.resolve(&m.target).map_err(operational)?;
        verify_bytes(&m.source_path, &target)?;
        entries.push(ReportEntry {
            source: m.source.clone(),
            target: m.target.clone(),
            bucket: m.bucket,
            bytes: size,
            verified: true,
            address_verification: verify_address(plan, m)?,
        });
    }
    plan.verify_source()?;
    let generated_records = plan
        .resources()
        .iter()
        .map(|r| {
            let mut written = r.clone();
            written.record = store
                .read_resource_metadata(&r.target)
                .map_err(operational)?;
            Ok(written)
        })
        .collect::<Result<Vec<_>, Cdb1Error>>()?;
    let conformance = store.validate(plan.profile()).map_err(operational)?;
    let report = MigrationReport {
        conformant: conformance.is_conformant(),
        migrated: entries.len(),
        extras: entries
            .iter()
            .filter(|e| e.bucket == crate::plan::Bucket::Extra)
            .count(),
        skipped: plan.skipped().to_vec(),
        declared: plan.declared().map(str::to_owned),
        standard_note: plan.standard_note().map(str::to_owned),
        entries,
        conformance,
        source_root: plan.source_root().to_owned(),
        global_metadata: store.global_metadata().map_err(operational)?,
        descriptor: plan.descriptor().clone(),
        source_declarations: source_declarations(plan),
        reader_findings: plan
            .findings()
            .iter()
            .map(|f| {
                serde_json::json!({
                    "raw_path": f.raw_path,
                    "kind": format!("{:?}", f.kind),
                    "message": f.message
                })
            })
            .collect(),
        observed: plan.inventory().entries.iter().map(observation).collect(),
        generated_records,
        operator_metadata: plan.operator_metadata().clone(),
        seed: plan.seed().clone(),
        rename_map: plan.options().rename_map.clone(),
        extras_policy: plan.options().extras,
        payload_semantics: "unchecked",
        embedded_references: "unchecked",
        limitations: "Conformance is exactly under the emitted descriptor. \
            Operator metadata is an assertion; payload semantics and embedded-reference \
            usability were not decoded or verified. Renaming can break case-sensitive \
            embedded paths. Address checks are filename-derived, not source conformance. \
            Sources and destination parents must remain stable and operator-controlled; \
            hostile concurrent ancestor replacement is not prevented.",
    };
    plan.verify_source()?;
    parent_anchor.verify()?;
    root_anchor.verify()?;
    // Both serializations and both writes finish before final publication.
    let descriptor_bytes = pretty(plan.descriptor())?;
    let report_bytes = pretty(&report)?;
    let descriptor_stage = stage_json(&out_parent, DESCRIPTOR_FILE, &descriptor_bytes)?;
    let report_stage = stage_json(&out_parent, REPORT_FILE, &report_bytes)?;
    plan.verify_source()?;
    parent_anchor.verify()?;
    root_anchor.verify()?;
    publish(&descriptor_stage, &out_parent.join(DESCRIPTOR_FILE))?;
    publish(&report_stage, &out_parent.join(REPORT_FILE))?;
    Ok(report)
}

fn operational(error: impl std::fmt::Display) -> Cdb1Error {
    Cdb1Error::Operational(error.to_string())
}
fn pretty(value: &impl serde::Serialize) -> Result<Vec<u8>, Cdb1Error> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(operational)?;
    bytes.push(b'\n');
    Ok(bytes)
}
fn verify_bytes(source: &std::path::Path, target: &std::path::Path) -> Result<(), Cdb1Error> {
    use std::io::Read;
    safe_regular(source)?;
    safe_regular(target)?;
    let mut src = std::fs::File::open(source).map_err(|e| Cdb1Error::Io(source.to_owned(), e))?;
    let mut dst = std::fs::File::open(target).map_err(|e| Cdb1Error::Io(target.to_owned(), e))?;
    let mut a = [0; 65536];
    let mut b = [0; 65536];
    loop {
        let n = src
            .read(&mut a)
            .map_err(|e| Cdb1Error::Io(source.to_owned(), e))?;
        if n == 0 {
            if dst
                .read(&mut b[..1])
                .map_err(|e| Cdb1Error::Io(target.to_owned(), e))?
                == 0
            {
                return Ok(());
            }
            break;
        }
        dst.read_exact(&mut b[..n])
            .map_err(|e| Cdb1Error::Io(target.to_owned(), e))?;
        if a[..n] != b[..n] {
            break;
        }
    }
    Err(operational(format!(
        "byte verification failed: {source:?} -> {target:?}"
    )))
}
fn verify_address(
    plan: &crate::plan::MigrationPlan,
    m: &crate::plan::PlannedMove,
) -> Result<Option<serde_json::Value>, Cdb1Error> {
    use crate::address::{global_address, per_geocell_address};
    if m.bucket != crate::plan::Bucket::Tile {
        return Ok(None);
    }
    let source = plan
        .inventory()
        .entries
        .iter()
        .find_map(|e| match e {
            crate::Cdb1Entry::Tile(t) if t.rel_path == m.source => Some(t),
            _ => None,
        })
        .ok_or_else(|| operational("planned tile absent from inventory"))?;
    let name = m.target.rsplit('/').next().unwrap_or_default();
    let target = crate::grammar::TileFileName::parse(name).map_err(operational)?;
    let address = global_address(&target).map_err(operational)?;
    let inverse = per_geocell_address(address);
    if target != source.file
        || address != global_address(&source.file).map_err(operational)?
        || inverse.geocell != source.file.geocell
        || inverse.lod != source.file.lod
        || inverse.uref != source.file.uref
        || inverse.rref != source.file.rref
    {
        return Err(operational(format!(
            "filename-derived address verification failed: {}",
            m.source
        )));
    }
    Ok(Some(
        serde_json::json!({"basis":"filename-derived","verified":true,"lod":address.lod().value(),"row":address.row(),"col":address.col(),"latitude_sw":inverse.geocell.lat_sw,"longitude_sw":inverse.geocell.lon_sw,"uref":inverse.uref,"rref":inverse.rref,"source_findings":source.findings}),
    ))
}
fn observation(e: &crate::Cdb1Entry) -> serde_json::Value {
    use crate::Cdb1Entry;
    let (class, details, findings) = match e {
        Cdb1Entry::Tile(t) => (
            "tile",
            serde_json::json!({"dataset":t.file.dataset,"cs1":t.file.cs1,"cs2":t.file.cs2,"extension":t.file.extension}),
            t.findings.clone(),
        ),
        Cdb1Entry::Global { kind, .. } => (
            "global",
            serde_json::json!({"kind":format!("{kind:?}")}),
            vec![],
        ),
        Cdb1Entry::Metadata { .. } => ("source_metadata", serde_json::Value::Null, vec![]),
        Cdb1Entry::Unrecognized { why, .. } => {
            ("unrecognized", serde_json::json!({"why":why}), vec![])
        }
        Cdb1Entry::Unsafe { why, .. } => ("unsafe", serde_json::json!({"why":why}), vec![]),
    };
    serde_json::json!({"source":e.rel_path(),"raw_path":e.raw_path(),"classification":class,"details":details,"findings":findings})
}
fn declaration(v: &crate::version_meta::VersionXml) -> serde_json::Value {
    serde_json::json!({"specification":v.specification.map(|s|s.as_str()),"specification_raw":v.specification_raw,"specification_authority":v.specification_authority,"specification_update":v.specification_update,"previous_root":v.previous_root,"comment":v.comment,"metadata_standard":v.metadata_standard,"extension":v.extension.as_ref().map(|e|serde_json::json!({"name":e.name,"version":e.version}))})
}
fn source_declarations(p: &crate::plan::MigrationPlan) -> serde_json::Value {
    serde_json::json!({"version_path":p.version_path(),"version":p.version().map(declaration),"configuration_path":p.configuration_path(),"configuration":p.configuration().map(|c|serde_json::json!({"version_folders":c.version_folders,"comment":c.comment,"versions":c.versions.iter().map(|v|serde_json::json!({"folder":v.folder,"declaration":declaration(&v.declaration)})).collect::<Vec<_>>()})),"effective":p.effective_declaration().map(declaration)})
}

// Resolve the physical spelling of every existing safe ancestor before adding
// absent suffixes. This catches case aliases on case-insensitive hosts too.
fn safe_directory(path: &std::path::Path) -> Result<std::path::PathBuf, Cdb1Error> {
    use std::path::{Component, PathBuf};
    if path.to_str().is_none() {
        return Err(Cdb1Error::Refused("output/source path is not UTF-8".into()));
    }
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|e| Cdb1Error::Io(path.to_owned(), e))?
            .join(path)
    };
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => {
                return Err(Cdb1Error::Refused(format!(
                    "parent traversal is not supported: {path:?}"
                )))
            }
            Component::CurDir => continue,
            _ => resolved.push(component.as_os_str()),
        }
        match std::fs::symlink_metadata(&resolved) {
            Ok(meta) => {
                if !meta.is_dir() || meta.file_type().is_symlink() {
                    return Err(Cdb1Error::Refused(format!(
                        "unsafe directory ancestor: {resolved:?}"
                    )));
                }
                resolved = std::fs::canonicalize(&resolved)
                    .map_err(|e| Cdb1Error::Io(resolved.clone(), e))?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Cdb1Error::Io(resolved, e)),
        }
    }
    Ok(resolved)
}
fn require_absent(path: &std::path::Path) -> Result<(), Cdb1Error> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Err(Cdb1Error::Refused(format!(
            "destination already exists: {path:?}"
        ))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Cdb1Error::Io(path.to_owned(), e)),
    }
}
fn safe_regular(path: &std::path::Path) -> Result<(), Cdb1Error> {
    let parent = path
        .parent()
        .ok_or_else(|| Cdb1Error::Refused(format!("file has no parent: {path:?}")))?;
    safe_directory(parent)?;
    let meta = std::fs::symlink_metadata(path).map_err(|e| Cdb1Error::Io(path.to_owned(), e))?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(Cdb1Error::Refused(format!(
            "not a safe regular file: {path:?}"
        )));
    }
    Ok(())
}
fn check_collection_count(count: usize) -> Result<(), Cdb1Error> {
    if count > MAX_MIGRATED_FILES {
        return Err(Cdb1Error::Refused(format!("v1 supports at most {MAX_MIGRATED_FILES} carried source files (one collection per file); requested {count}")));
    }
    Ok(())
}
fn preflight(
    plan: &crate::plan::MigrationPlan,
    parent: &std::path::Path,
) -> Result<std::path::PathBuf, Cdb1Error> {
    check_collection_count(plan.moves().len())?;
    let parent = safe_directory(parent)?;
    if parent.starts_with(plan.source_root()) || plan.source_root().starts_with(parent.join("cdb"))
    {
        return Err(Cdb1Error::Refused(
            "source and output must be disjoint".into(),
        ));
    }
    for name in [
        "cdb",
        DESCRIPTOR_FILE,
        REPORT_FILE,
        &format!("{DESCRIPTOR_FILE}.partial"),
        &format!("{REPORT_FILE}.partial"),
    ] {
        require_absent(&parent.join(name))?;
    }
    Ok(parent)
}
// Keep directory identities as well as rechecking types. Normal child writes
// change mtime, so that field cannot be an identity check for output ancestors.
struct Anchors(Vec<(std::path::PathBuf, std::fs::Metadata)>);
impl Anchors {
    fn capture(path: &std::path::Path) -> Result<Self, Cdb1Error> {
        safe_directory(path)?;
        let entries = path
            .ancestors()
            .map(|p| {
                std::fs::symlink_metadata(p)
                    .map(|m| (p.to_owned(), m))
                    .map_err(|e| Cdb1Error::Io(p.to_owned(), e))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self(entries))
    }
    fn verify(&self) -> Result<(), Cdb1Error> {
        for (path, before) in &self.0 {
            let after =
                std::fs::symlink_metadata(path).map_err(|e| Cdb1Error::Io(path.clone(), e))?;
            let mut same = after.is_dir()
                && !after.file_type().is_symlink()
                && before.created().ok() == after.created().ok();
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                same &= before.dev() == after.dev() && before.ino() == after.ino();
            }
            if !same {
                return Err(Cdb1Error::Refused(format!(
                    "output ancestor changed: {path:?}"
                )));
            }
        }
        Ok(())
    }
}
// Stage fully written JSON under visibly incomplete names. Hard-link publication
// fails on any existing final path rather than replacing it (rename would).
fn stage_json(
    parent: &std::path::Path,
    name: &str,
    bytes: &[u8],
) -> Result<std::path::PathBuf, Cdb1Error> {
    use std::io::Write;
    let path = parent.join(format!("{name}.partial"));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| Cdb1Error::Io(path.clone(), e))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| Cdb1Error::Io(path.clone(), e))?;
    Ok(path)
}
fn publish(staged: &std::path::Path, final_path: &std::path::Path) -> Result<(), Cdb1Error> {
    std::fs::hard_link(staged, final_path).map_err(|e| Cdb1Error::Io(final_path.to_owned(), e))?;
    // Once the complete final artifact exists, an optional temporary-name cleanup
    // failure must not falsely report that publication itself failed.
    let _ = std::fs::remove_file(staged);
    Ok(())
}

#[cfg(test)]
mod materialization_tests {
    use super::*;
    /// Journal capacity is checked before writes, including the exact boundary.
    #[test]
    fn mig_collection_capacity_preflight() {
        assert!(check_collection_count(0).is_ok());
        assert!(check_collection_count(999_999).is_ok());
        assert!(matches!(
            check_collection_count(1_000_000),
            Err(Cdb1Error::Refused(_))
        ));
    }
    /// Readback compares actual bytes across chunk boundaries and catches extra
    /// trailing bytes, truncation, and differences after the first buffer.
    #[test]
    fn mig_byte_verification_detects_corruption() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let source = root.join("source");
        let target = root.join("target");
        let data = vec![42; 131_073];
        std::fs::write(&source, &data).unwrap();
        for corrupt in [vec![42; 131_072], vec![42; 131_074], {
            let mut b = data.clone();
            b[70_000] = 43;
            b
        }] {
            std::fs::write(&target, corrupt).unwrap();
            assert!(verify_bytes(&source, &target).is_err());
        }
        std::fs::write(&target, data).unwrap();
        assert!(verify_bytes(&source, &target).is_ok());
    }
    /// Exclusive publication cannot replace a result created after preflight;
    /// failed staging cannot leave a final success report.
    #[test]
    fn mig_companion_publication_is_exclusive() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let staged = stage_json(&root, REPORT_FILE, b"complete").unwrap();
        let final_path = root.join(REPORT_FILE);
        std::fs::write(&final_path, b"existing").unwrap();
        assert!(publish(&staged, &final_path).is_err());
        assert_eq!(std::fs::read(&final_path).unwrap(), b"existing");
        assert!(stage_json(&root, REPORT_FILE, b"replacement").is_err());
        std::fs::remove_file(final_path).unwrap();
        assert!(stage_json(&root.join("absent"), REPORT_FILE, b"complete").is_err());
        assert!(!root.join("absent").join(REPORT_FILE).exists());
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use opencdb::profiles::ApplicationProfile;

    /// Migration contract: generated metadata never claims translated DCAT.
    #[test]
    fn mig_profile_descriptor_shape() {
        let profile = MigrationProfile::new(vec!["tif".into(), "shp".into(), "tif".into()]);
        assert!(profile.storage_crs().is_ok());
        let d = descriptor(&profile);
        assert_eq!(d["name"], "cdb-migrator-v1");
        assert_eq!(d["metadata_standard"], "NoMetadata");
        assert_eq!(d["case_rule"], "Snake_case");
        assert_eq!(d["metadata_encoding"], "json");
        assert_eq!(d["tiling_scheme"], "CDB1GlobalGrid");
        assert_eq!(d["known_extensions"], serde_json::json!(["shp", "tif"]));
    }

    /// Attr1/Attr2: the emitted yardstick carries exactly the explicit model.
    #[test]
    fn mig_profile_explicit_attribute_model_matches_descriptor() {
        let model = opencdb::AttributeModel {
            schema_uri: None,
            attributes: vec![opencdb::AttributeDef {
                id: " 1 ".into(),
                name: " Height ".into(),
                description: " Height in metres ".into(),
            }],
        };
        let profile = MigrationProfile::new(vec!["wkt".into()])
            .with_attribute_model(model)
            .unwrap();
        let declared = profile.attribute_model().unwrap();
        assert_eq!(declared.attributes[0].id, "1");
        assert_eq!(declared.attributes[0].name, "Height");
        assert_eq!(
            descriptor(&profile)["attribute_model"],
            serde_json::to_value(&declared).unwrap()
        );
        assert!(MigrationProfile::new(vec![])
            .with_attribute_model(opencdb::AttributeModel {
                schema_uri: None,
                attributes: vec![]
            })
            .is_err());
        assert!(descriptor(&MigrationProfile::new(vec![]))["attribute_model"].is_null());
    }

    /// Name5 guards all Metadata5 encodings; case errors cannot hide a record.
    #[test]
    fn mig_profile_guards_resource_records_and_validates_real_store() {
        let profile = MigrationProfile::new(vec!["wkt".into()]);
        for path in [
            "metadata/a.json",
            "/tiles/METADATA/a.XML",
            "/metadata/a.GPKG",
        ] {
            assert!(profile.is_resource_metadata(path), "{path}");
        }
        for path in [
            "metadata/a.tif",
            "metadata/a",
            "/metadatas/a.json",
            "/metadata/nested/a.json",
            "/global_metadata/global_metadata.json",
        ] {
            assert!(!profile.is_resource_metadata(path), "{path}");
        }
        assert!(profile.style_guide().is_reserved("metadata"));
        assert_eq!(
            profile.conformance_classes(),
            opencdb::RequirementsClass::ALL
        );
        let temp = tempfile::tempdir().unwrap();
        let store = opencdb::CdbDatastore::create(
            temp.path(),
            &profile,
            opencdb::DatastoreSeed::new(
                "migration",
                "Migration",
                "Explicit migration metadata",
                "ops@example.org",
            ),
        )
        .unwrap();
        let report = store.validate(&profile).unwrap();
        assert!(report.is_conformant(), "{report:?}");
        let d = descriptor(&profile);
        assert_eq!(d.as_object().unwrap().len(), 14);
        assert_eq!(d["uom"], "M");
        assert_eq!(d["language"], "en");
        assert_eq!(d["root_folder_name"], "cdb");
        assert_eq!(d["resource_metadata_dir"], "metadata");
        assert_eq!(d["storage_technology"], "file-system");
        assert_eq!(d["conformance_classes"], "all");
        assert_eq!(
            opencdb::crs::StorageCrs::from_wkt(d["storage_crs_wkt"].as_str().unwrap())
                .unwrap()
                .authority(),
            Some(("EPSG".into(), "4326".into()))
        );
    }

    /// Source declarations are provenance, including recognized 2.0 keywords.
    #[test]
    fn mig_source_standard_is_never_a_schema_translation() {
        for raw in ["DCAT", "ISO-19115:2014", "NoMetadata"] {
            let (standard, note) = map_metadata_standard(Some(raw));
            assert_eq!(standard, opencdb::metadata::MetadataStandard::NoMetadata);
            assert!(note.unwrap().contains(raw));
        }
        assert_eq!(
            map_metadata_standard(None),
            (opencdb::metadata::MetadataStandard::NoMetadata, None)
        );
    }
}
