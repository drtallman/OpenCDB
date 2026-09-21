//! Pure, source-bound migration planning.
//!
//! Planning reads but never creates output. Sources must remain stable and
//! operator-controlled: snapshots detect observable changes, not adversarial
//! ancestor replacement races. Payload fingerprints are non-cryptographic
//! change detectors, not integrity attestations. Materializers must recheck the
//! plan before and after copying and independently verify copied bytes.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::hash::Hasher;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use opencdb::metadata::{GlobalMetadata, ResourceMetadata};
use opencdb::profiles::ApplicationProfile;
use serde::Serialize;

use crate::address::{global_address, per_geocell_address};
use crate::grammar::{fold, DatasetDir, GeocellId, TileFileName};
use crate::metadata_input::MetadataManifest;
use crate::migrate::{descriptor, map_metadata_standard, MigrationProfile};
use crate::reader::ReaderFinding;
use crate::version_meta::{ConfigurationXml, VersionXml};
use crate::{Cdb1Entry, Cdb1Error, Cdb1Tree, Inventory};

/// Required operator decision for files outside the recognized source grammar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtrasPolicy {
    #[default]
    Unspecified,
    Carry,
    Skip,
}

/// Operator input. Mutable inputs are revalidated and copied into the plan.
#[derive(Debug, Clone, Default)]
pub struct PlanOptions {
    pub id: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub contact: Option<String>,
    pub extras: ExtrasPolicy,
    /// Exact source-relative file keys to literal target-relative file paths.
    /// Defaults are ASCII-folded; explicit values are never silently folded.
    pub rename_map: BTreeMap<String, String>,
    /// Required even when its resources array is empty.
    pub metadata: Option<MetadataManifest>,
}

/// Parse a JSON object of exact source keys and literal relative targets.
/// Duplicate keys are refused before map construction. Source membership and
/// target naming/collisions are checked by [`plan`], including programmatic maps.
pub fn parse_rename_map(content: &str) -> Result<BTreeMap<String, String>, Cdb1Error> {
    let crate::metadata_input::StrictValue(value) =
        serde_json::from_str(content).map_err(|e| refused(format!("rename map: {e}")))?;
    let map: BTreeMap<String, String> =
        serde_json::from_value(value).map_err(|e| refused(format!("rename map: {e}")))?;
    for (source, target) in &map {
        if !safe_relative(source) || !safe_relative(target) {
            return Err(refused(format!(
                "rename map {source:?} -> {target:?}: paths must be safe relative file paths"
            )));
        }
    }
    Ok(map)
}

/// Classification of a carried source file; generated records are separate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Bucket {
    Tile,
    Global,
    Extra,
}

/// A copy instruction. Only immutable references escape the validated plan.
#[derive(Debug, Clone, Serialize)]
pub struct PlannedMove {
    pub source: String,
    pub target: String,
    pub bucket: Bucket,
    /// Exact absolute source identity, never reconstructed from display text.
    pub source_path: PathBuf,
}

/// A generated record and its source/payload association, ready for serialization.
#[derive(Debug, Clone, Serialize)]
pub struct PlannedResource {
    pub source: String,
    pub payload: String,
    pub target: String,
    pub record: ResourceMetadata,
}

/// Effective global identity passed to the public datastore seed constructor.
#[derive(Debug, Clone, Serialize)]
pub struct PlannedSeed {
    pub id: String,
    pub title: String,
    pub description: String,
    pub contact: String,
}

/// Validated, immutable instructions bound to one physical source tree.
///
/// No public constructor, deserializer, or mutable accessor exists. A cloned
/// move/record is merely report data and cannot modify the plan. Original
/// controls, reader findings, operator assertions and all leaves (including
/// skipped leaves) remain available for complete reporting.
#[derive(Debug)]
pub struct MigrationPlan {
    root: PathBuf,
    moves: Vec<PlannedMove>,
    skipped: Vec<String>,
    resources: Vec<PlannedResource>,
    profile: MigrationProfile,
    descriptor: serde_json::Value,
    seed: PlannedSeed,
    standard_note: Option<String>,
    declared: Option<String>,
    effective_declaration: Option<VersionXml>,
    version: Option<VersionXml>,
    configuration: Option<ConfigurationXml>,
    version_path: Option<PathBuf>,
    configuration_path: Option<PathBuf>,
    findings: Vec<ReaderFinding>,
    inventory: Inventory,
    options: PlanOptions,
    operator_metadata: MetadataManifest,
    snapshot: BTreeMap<PathBuf, FileSnapshot>,
}

impl MigrationPlan {
    /// Physical absolute source root to use for materialization.
    pub fn source_root(&self) -> &Path {
        &self.root
    }
    pub fn moves(&self) -> &[PlannedMove] {
        &self.moves
    }
    pub fn skipped(&self) -> &[String] {
        &self.skipped
    }
    pub fn resources(&self) -> &[PlannedResource] {
        &self.resources
    }
    pub fn profile(&self) -> &MigrationProfile {
        &self.profile
    }
    pub fn descriptor(&self) -> &serde_json::Value {
        &self.descriptor
    }
    pub fn seed(&self) -> &PlannedSeed {
        &self.seed
    }
    pub fn standard_note(&self) -> Option<&str> {
        self.standard_note.as_deref()
    }
    /// Raw effective Specification, including an unknown/empty declaration.
    pub fn declared(&self) -> Option<&str> {
        self.declared.as_deref()
    }
    /// Configuration's selected declaration when present, otherwise Version's.
    pub fn effective_declaration(&self) -> Option<&VersionXml> {
        self.effective_declaration.as_ref()
    }
    pub fn version(&self) -> Option<&VersionXml> {
        self.version.as_ref()
    }
    pub fn configuration(&self) -> Option<&ConfigurationXml> {
        self.configuration.as_ref()
    }
    pub fn version_path(&self) -> Option<&Path> {
        self.version_path.as_deref()
    }
    pub fn configuration_path(&self) -> Option<&Path> {
        self.configuration_path.as_deref()
    }
    pub fn findings(&self) -> &[ReaderFinding] {
        &self.findings
    }
    /// Original classifications, including tile-directory findings and skips.
    /// Tile addresses are derived from parsed filenames; directory agreement
    /// is not claimed for a default folded target.
    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }
    pub fn options(&self) -> &PlanOptions {
        &self.options
    }
    /// Complete effective operator assertions/defaults, before added item links.
    pub fn operator_metadata(&self) -> &MetadataManifest {
        &self.operator_metadata
    }

    /// Reopen controls, re-inventory and fingerprint every source leaf. Call
    /// before and after copying; this is not an atomic or adversarial snapshot.
    pub fn verify_source(&self) -> Result<(), Cdb1Error> {
        let fresh = Cdb1Tree::open(&self.root)?;
        validate_controls(&fresh)?;
        let inv = fresh.inventory()?;
        match_inventory(&self.inventory, &inv)?;
        if fresh.version() != self.version.as_ref()
            || fresh.configuration() != self.configuration.as_ref()
            || fresh.findings() != self.findings
            || snapshot(&inv)? != self.snapshot
        {
            return Err(refused("source changed since planning"));
        }
        Ok(())
    }

    /// Additionally reject attempts to apply a plan to another root.
    pub fn verify_tree(&self, tree: &Cdb1Tree) -> Result<(), Cdb1Error> {
        Cdb1Tree::open(tree.root())?;
        if physical(tree.root())? != self.root {
            return Err(refused("plan belongs to a different source root"));
        }
        self.verify_source()
    }
}

/// Validate the complete source and operator input without destination writes.
/// Caller inventories are compared with a fresh reader inventory, never trusted
/// as authorization. A stale tree cannot hide newly added version controls.
pub fn plan(
    tree: &Cdb1Tree,
    inventory: &Inventory,
    options: &PlanOptions,
) -> Result<MigrationPlan, Cdb1Error> {
    let fresh = Cdb1Tree::open(tree.root())?;
    validate_controls(&fresh)?;
    match_inventory(inventory, &fresh.inventory()?)?;
    let root = physical(tree.root())?;
    let fresh = Cdb1Tree::open(&root)?;
    let effective = validate_controls(&fresh)?.cloned();
    let inventory = fresh.inventory()?;
    let before = snapshot(&inventory)?;
    let metadata = options
        .metadata
        .as_ref()
        .ok_or_else(|| refused("metadata manifest is required"))?;
    metadata.validate()?;
    let mut errors = Vec::new();
    let declarations: BTreeMap<_, _> = metadata
        .resources
        .iter()
        .map(|r| (r.source_path.as_str(), r))
        .collect();
    let mut ids: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for r in &metadata.resources {
        ids.entry(&r.record.id).or_default().push(&r.source_path);
    }
    for (id, paths) in ids {
        if paths.len() > 1 {
            errors.push(format!(
                "duplicate resource ID {id:?}: {}",
                paths.join(", ")
            ));
        }
    }
    let known: BTreeSet<_> = inventory
        .entries
        .iter()
        .filter_map(Cdb1Entry::rel_path)
        .collect();
    for (source, target) in &options.rename_map {
        if !safe_relative(source) || !known.contains(source.as_str()) {
            errors.push(format!(
                "rename key {source:?} must match exactly one source file"
            ));
        }
        if !safe_relative(target) {
            errors.push(format!("rename {source:?} has unsafe target {target:?}"));
        }
    }
    let guide = MigrationProfile::new(vec![]).style_guide();
    let mut moves = Vec::new();
    let mut skipped = Vec::new();
    let mut resources = Vec::new();
    let mut used = BTreeSet::new();
    for entry in &inventory.entries {
        let source = entry
            .rel_path()
            .ok_or_else(|| refused(format!("unsafe source {:?}", entry.raw_path())))?;
        let (default, bucket, dataset) = match entry {
            Cdb1Entry::Tile(tile) => (fold(source), Bucket::Tile, Some(tile.file.dataset)),
            Cdb1Entry::Global { .. } => (fold(source), Bucket::Global, None),
            Cdb1Entry::Metadata { .. } => (
                format!(
                    "extras/source_metadata/{}",
                    fold(source.split_once('/').map_or(source, |(_, rest)| rest))
                ),
                Bucket::Extra,
                None,
            ),
            Cdb1Entry::Unrecognized { .. } => match options.extras {
                ExtrasPolicy::Unspecified => {
                    errors.push(format!(
                        "{source}: extras policy must explicitly carry or skip"
                    ));
                    continue;
                }
                ExtrasPolicy::Skip => {
                    if options.rename_map.contains_key(source) {
                        errors.push(format!("rename key {source:?} names skipped extras"));
                    }
                    skipped.push(source.to_owned());
                    continue;
                }
                ExtrasPolicy::Carry => (format!("extras/{}", fold(source)), Bucket::Extra, None),
            },
            Cdb1Entry::Unsafe { .. } => return Err(refused(format!("unsafe source {source:?}"))),
        };
        let target = options.rename_map.get(source).cloned().unwrap_or(default);
        if !safe_relative(&target)
            || target.split('/').any(|part| {
                !part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.')
                    || guide.validate_component(part).is_err()
            })
        {
            errors.push(format!("{source:?}: unconformant target {target:?}"));
        }
        if opencdb::naming::split_extension(target.rsplit('/').next().unwrap_or(&target)).1
            == Some("")
        {
            errors.push(format!(
                "{source:?}: target {target:?} has an empty extension, unusable in the descriptor"
            ));
        }
        if reserved_target(&target) {
            errors.push(format!(
                "{source:?}: reserved or metadata-captured target {target:?}"
            ));
        }
        if let Cdb1Entry::Tile(tile) = entry {
            if let Err(e) = check_tile(&tile.file, &target, options.rename_map.contains_key(source))
            {
                errors.push(format!("{source:?} -> {target:?}: {e}"));
            }
        }
        if matches!(bucket, Bucket::Tile | Bucket::Global) {
            if let Some(declaration) = declarations.get(source) {
                used.insert(source);
                if let Err(e) = declaration.validate_for_dataset(dataset) {
                    errors.push(e.to_string());
                }
                let record_target = format!("metadata/resource_{:08}.json", resources.len());
                let mut record = declaration.record.clone();
                // The absolute logical path resolves from datastore root, not
                // from the generated metadata directory. Source identity is
                // carried separately; do not emit broken source-file links.
                match opencdb::links::Link::new(format!("/{target}"), "item") {
                    Ok(link) => record.associations.push(link),
                    Err(e) => errors.push(format!("{source}: payload association: {e}")),
                }
                if let Err(e) = record.validate() {
                    errors.push(format!("{source}: generated record: {e}"));
                }
                resources.push(PlannedResource {
                    source: source.into(),
                    payload: target.clone(),
                    target: record_target,
                    record,
                });
            } else {
                errors.push(format!("{source}: missing metadata resource declaration"));
            }
        }
        moves.push(PlannedMove {
            source: source.into(),
            target,
            bucket,
            source_path: entry.raw_path().to_path_buf(),
        });
    }
    for source in declarations.keys() {
        if !used.contains(source) {
            errors.push(format!("{source}: surplus metadata resource declaration"));
        }
    }
    collisions(&moves, &resources, &mut errors);
    if !errors.is_empty() {
        return Err(refused(errors.join("\n")));
    }
    let extensions: BTreeSet<_> = moves
        .iter()
        .map(|m| m.target.as_str())
        .chain(resources.iter().map(|r| r.target.as_str()))
        .chain([
            "global_metadata/global_metadata.json",
            "global_metadata/crs.wkt",
        ])
        .filter_map(|p| opencdb::naming::split_extension(p.rsplit('/').next().unwrap_or(p)).1)
        .map(str::to_owned)
        .collect();
    let mut profile = MigrationProfile::new(extensions.into_iter().collect());
    if let Some(model) = &metadata.attribute_model {
        profile = profile.with_attribute_model(model.clone())?;
    }
    let declared = effective.as_ref().and_then(|v| v.specification_raw.clone());
    let (_, standard_note) = map_metadata_standard(
        effective
            .as_ref()
            .and_then(|v| v.metadata_standard.as_deref()),
    );
    let name = root
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| refused("source needs a UTF-8 directory name"))?;
    let seed = PlannedSeed {
        id: options
            .id
            .clone()
            .unwrap_or_else(|| format!("urn:cdb1-migrated:{}", fold(name))),
        title: options.title.clone().unwrap_or_else(|| name.into()),
        description: options.description.clone().unwrap_or_else(|| {
            format!(
                "Migrated from CDB 1.x (declared {}){}",
                declared.as_deref().unwrap_or("undeclared"),
                standard_note
                    .as_ref()
                    .map(|n| format!("; {n}"))
                    .unwrap_or_default()
            )
        }),
        contact: options
            .contact
            .clone()
            .unwrap_or_else(|| "unspecified (operator supplied none)".into()),
    };
    validate_seed(&seed, &profile)?;
    let result = MigrationPlan {
        root,
        moves,
        skipped,
        resources,
        descriptor: descriptor(&profile),
        profile,
        seed,
        standard_note,
        declared,
        effective_declaration: effective,
        version: fresh.version().cloned(),
        configuration: fresh.configuration().cloned(),
        version_path: fresh.version_path().map(Path::to_path_buf),
        configuration_path: fresh.configuration_path().map(Path::to_path_buf),
        findings: fresh.findings().to_vec(),
        inventory,
        options: options.clone(),
        operator_metadata: metadata.clone(),
        snapshot: before,
    };
    result.verify_source()?;
    Ok(result)
}

fn refused(message: impl Into<String>) -> Cdb1Error {
    Cdb1Error::Refused(message.into())
}
fn physical(path: &Path) -> Result<PathBuf, Cdb1Error> {
    fs::canonicalize(path).map_err(|e| Cdb1Error::Io(path.to_path_buf(), e))
}
fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', ':'])
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}
fn reserved_target(path: &str) -> bool {
    let folded = fold(path);
    let first = folded.split('/').next().unwrap_or_default();
    matches!(
        first,
        "global_metadata"
            | "versions"
            | "metadata"
            | "cdb_migrator-descriptor.json"
            | "cdb_migrator-migration-report.json"
    ) || MigrationProfile::new(vec![]).is_resource_metadata(path)
}
fn chain(detail: impl std::fmt::Display) -> Cdb1Error {
    refused(format!(
        "multi-version 1.x stores are not supported in this version: {detail}"
    ))
}
fn validate_controls(tree: &Cdb1Tree) -> Result<Option<&VersionXml>, Cdb1Error> {
    if !tree.control_metadata_safe() {
        return Err(refused(format!(
            "unsafe or malformed source control metadata: {:?}",
            tree.findings()
        )));
    }
    if let Some(previous) = tree.version().and_then(|v| v.previous_root.as_ref()) {
        return Err(chain(format!("Version.xml previous root {previous:?}")));
    }
    if let Some(config) = tree.configuration() {
        for version in &config.versions {
            if let Some(previous) = &version.declaration.previous_root {
                return Err(chain(format!(
                    "Configuration.xml previous root {previous:?}"
                )));
            }
        }
        if config.versions.len() != 1 {
            return Err(chain(format!(
                "Configuration.xml selects {} folders",
                config.versions.len()
            )));
        }
        let selected = &config.versions[0];
        let folder = Path::new(&selected.folder);
        if folder.is_absolute() || selected.folder.contains(['\\', ':']) {
            return Err(chain(format!(
                "Configuration folder {:?} is not source-relative",
                selected.folder
            )));
        }
        let selected_path = tree.root().join(folder);
        // Reuse reader ancestry checks before canonicalization so a link cannot
        // select a root through an alias, even if it resolves to this source.
        Cdb1Tree::open(&selected_path)
            .map_err(|e| chain(format!("Configuration folder {:?}: {e}", selected.folder)))?;
        if physical(&selected_path)? != physical(tree.root())? {
            return Err(chain(format!(
                "Configuration folder {:?} does not select the current root",
                selected.folder
            )));
        }
        return Ok(Some(&selected.declaration));
    }
    Ok(tree.version())
}
fn check_tile(file: &TileFileName, target: &str, explicit: bool) -> Result<(), String> {
    let address = global_address(file)?;
    let inverse = per_geocell_address(address);
    if inverse.geocell != file.geocell
        || inverse.lod != file.lod
        || inverse.uref != file.uref
        || inverse.rref != file.rref
    {
        return Err("filename-derived tile address did not roundtrip".into());
    }
    if !explicit {
        return Ok(());
    }
    let parts: Vec<_> = target.split('/').collect();
    if parts.len() != 7 || parts[0] != "tiles" {
        return Err("tile override needs the seven-component tiles grammar".into());
    }
    let target_file = TileFileName::parse(parts[6])?;
    if target_file != *file || global_address(&target_file)? != address {
        return Err("tile override changes filename identity/address".into());
    }
    let u = parts[5]
        .strip_prefix('u')
        .and_then(|n| n.parse::<u32>().ok());
    if GeocellId::from_dirs(parts[1], parts[2])? != file.geocell
        || DatasetDir::parse(parts[3])?.code != file.dataset
        || parts[4] != fold(&file.lod.dir_name())
        || u != Some(file.uref)
    {
        return Err("tile override directories disagree with filename identity".into());
    }
    Ok(())
}
fn collisions(moves: &[PlannedMove], resources: &[PlannedResource], errors: &mut Vec<String>) {
    let mut paths: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for m in moves {
        paths.entry(fold(&m.target)).or_default().push(&m.source);
    }
    for r in resources {
        paths.entry(fold(&r.target)).or_default().push(&r.source);
    }
    for path in [
        "global_metadata/global_metadata.json",
        "global_metadata/crs.wkt",
        "global_metadata/vector_attributes.json",
        "cdb_migrator-descriptor.json",
        "cdb_migrator-migration-report.json",
    ] {
        paths.entry(path.into()).or_default().push("<generated>");
    }
    for (target, sources) in &paths {
        if sources.len() > 1 {
            errors.push(format!(
                "target collision {target:?}: {}",
                sources.join(", ")
            ));
        }
        let mut ancestor = target.as_str();
        while let Some((parent, _)) = ancestor.rsplit_once('/') {
            if let Some(parents) = paths.get(parent) {
                errors.push(format!(
                    "file/directory collision {parent:?} ({}) and {target:?} ({})",
                    parents.join(", "),
                    sources.join(", ")
                ));
            }
            ancestor = parent;
        }
    }
}
fn validate_seed(seed: &PlannedSeed, profile: &MigrationProfile) -> Result<(), Cdb1Error> {
    // A deterministic validation-only timestamp; the materializer uses the
    // supplied DatastoreSeed creation time or the facade's normal clock.
    let created = opencdb::metadata::temporal::parse_datetime("2000-01-01T00:00:00Z")
        .map_err(|e| refused(e.to_string()))?;
    GlobalMetadata::builder()
        .id(&seed.id)
        .title(&seed.title)
        .description(&seed.description)
        .contact_point(&seed.contact)
        .created(created)
        .language(profile.language().map_err(|e| refused(e.to_string()))?)
        .standard(profile.metadata_standard())
        .encoding(profile.metadata_encoding())
        .uom(profile.uom())
        .build()
        .map_err(|e| refused(format!("global seed: {e}")))?
        .validate()
        .map_err(|e| refused(format!("global seed: {e}")))
}

fn same_entry(a: &Cdb1Entry, b: &Cdb1Entry) -> bool {
    if a.raw_path() != b.raw_path() || a.rel_path() != b.rel_path() {
        return false;
    }
    match (a, b) {
        (Cdb1Entry::Tile(a), Cdb1Entry::Tile(b)) => a.file == b.file && a.findings == b.findings,
        (Cdb1Entry::Global { kind: a, .. }, Cdb1Entry::Global { kind: b, .. }) => a == b,
        (Cdb1Entry::Metadata { .. }, Cdb1Entry::Metadata { .. }) => true,
        (Cdb1Entry::Unrecognized { why: a, .. }, Cdb1Entry::Unrecognized { why: b, .. }) => a == b,
        _ => false,
    }
}
fn match_inventory(expected: &Inventory, fresh: &Inventory) -> Result<(), Cdb1Error> {
    if expected.entries.len() != fresh.entries.len()
        || !expected
            .entries
            .iter()
            .zip(&fresh.entries)
            .all(|(a, b)| same_entry(a, b))
    {
        return Err(refused(
            "source inventory changed, is unsafe, or does not match the fresh reader inventory",
        ));
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct FileStamp {
    len: u64,
    modified: SystemTime,
    created: Option<SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}
fn stamp(path: &Path) -> Result<FileStamp, Cdb1Error> {
    let meta = fs::symlink_metadata(path).map_err(|e| Cdb1Error::Io(path.into(), e))?;
    if !meta.is_file() {
        return Err(refused(format!("unsafe or changed source file {path:?}")));
    }
    Ok(FileStamp {
        len: meta.len(),
        modified: meta.modified().map_err(|e| Cdb1Error::Io(path.into(), e))?,
        created: meta.created().ok(),
        #[cfg(unix)]
        identity: {
            use std::os::unix::fs::MetadataExt;
            (meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec())
        },
    })
}
#[derive(Debug, PartialEq, Eq)]
struct FileSnapshot {
    stamp: FileStamp,
    fingerprint: u64,
}
fn snapshot(inventory: &Inventory) -> Result<BTreeMap<PathBuf, FileSnapshot>, Cdb1Error> {
    let mut result = BTreeMap::new();
    for entry in &inventory.entries {
        if matches!(entry, Cdb1Entry::Unsafe { .. }) {
            return Err(refused(format!("unsafe source {:?}", entry.raw_path())));
        }
        let path = entry.raw_path();
        let before = stamp(path)?;
        let mut file = fs::File::open(path).map_err(|e| Cdb1Error::Io(path.into(), e))?;
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        let mut buffer = [0u8; 65536];
        loop {
            let n = file
                .read(&mut buffer)
                .map_err(|e| Cdb1Error::Io(path.into(), e))?;
            if n == 0 {
                break;
            }
            hash.write(&buffer[..n]);
        }
        if before != stamp(path)? {
            return Err(refused(format!("source changed while planning: {path:?}")));
        }
        result.insert(
            path.to_path_buf(),
            FileSnapshot {
                stamp: before,
                fingerprint: hash.finish(),
            },
        );
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{metadata_input::MetadataManifest, Cdb1Tree};
    use std::fs;

    fn source() -> tempfile::TempDir {
        let parent = fs::canonicalize(std::env::temp_dir()).unwrap();
        tempfile::tempdir_in(parent).unwrap()
    }
    fn options() -> PlanOptions {
        PlanOptions {
            metadata: Some(MetadataManifest {
                resources: vec![],
                attribute_model: None,
            }),
            ..Default::default()
        }
    }
    /// Migration contract: unknown files require an explicit disposition.
    #[test]
    fn mig_plan_extras_require_explicit_policy() {
        let tmp = source();
        fs::write(tmp.path().join("notes.txt"), b"opaque").unwrap();
        let tree = Cdb1Tree::open(tmp.path()).unwrap();
        let inv = tree.inventory().unwrap();
        assert!(plan(&tree, &inv, &options())
            .unwrap_err()
            .to_string()
            .contains("extras"));
        let mut opts = options();
        opts.extras = ExtrasPolicy::Skip;
        let p = plan(&tree, &inv, &opts).unwrap();
        assert!(p.moves().is_empty());
        assert_eq!(p.skipped(), &["notes.txt"]);
        p.verify_source().unwrap();
        fs::write(tmp.path().join("notes.txt"), b"changed").unwrap();
        assert!(p.verify_source().is_err());
    }

    const TILE: &str = "Tiles/N32/W118/001_Elevation/L00/U0/N32W118_D001_S001_T001_L00_U0_R0.tif";
    fn put(tmp: &tempfile::TempDir, path: &str, content: &[u8]) {
        let full = tmp.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, content).unwrap();
    }
    fn payload_options(paths: &[&str]) -> PlanOptions {
        let resources: Vec<_> = paths.iter().enumerate().map(|(i, path)| serde_json::json!({
            "source_path": path, "record": {"ID":format!("id{i}"), "type":"dataset", "title":"Operator title",
            "description":"Operator description", "keywords":["declared"],
            "domainSet":{"uom":"m","grid_cell_encoding":"value-is-center"}},
            "coverage":true, "measurement_values":false,"generated_faces":false
        })).collect();
        PlanOptions {
            metadata: Some(
                MetadataManifest::from_json_str(
                    &serde_json::json!({"resources":resources}).to_string(),
                )
                .unwrap(),
            ),
            ..Default::default()
        }
    }
    fn run(tmp: &tempfile::TempDir, opts: &PlanOptions) -> Result<MigrationPlan, crate::Cdb1Error> {
        let tree = Cdb1Tree::open(tmp.path()).unwrap();
        plan(&tree, &tree.inventory().unwrap(), opts)
    }
    /// Reviewed contract: source metadata stays opaque and records link to payloads.
    #[test]
    fn mig_plan_tiles_metadata_and_generated_assets() {
        let tmp = source();
        put(&tmp, TILE, b"raster");
        put(&tmp, "Metadata/Version.xml", b"<Version/>");
        let p = run(&tmp, &payload_options(&[TILE])).unwrap();
        assert!(p
            .moves()
            .iter()
            .any(|m| m.target == TILE.to_ascii_lowercase() && m.bucket == Bucket::Tile));
        assert!(p
            .moves()
            .iter()
            .any(|m| m.target == "extras/source_metadata/version.xml"));
        assert_eq!(p.resources()[0].target, "metadata/resource_00000000.json");
        assert_eq!(p.resources()[0].source, TILE);
        assert!(p.resources()[0]
            .record
            .associations
            .iter()
            .any(|l| l.href == format!("/{}", TILE.to_ascii_lowercase()) && l.rel == "item"));
        assert_eq!(
            p.descriptor()["known_extensions"],
            serde_json::json!(["json", "tif", "wkt", "xml"])
        );
        assert_eq!(p.operator_metadata().resources[0].record.id, "id0");
        assert!(!tmp.path().join("metadata/resource_00000000.json").exists());
    }
    /// Reviewed single-root policy: precedence cannot erase chain evidence.
    #[test]
    fn mig_plan_controls_and_precedence() {
        let tmp = source();
        put(
            &tmp,
            "Metadata/Version.xml",
            br#"<Version><Specification version="1.0"/></Version>"#,
        );
        put(
            &tmp,
            "Metadata/Configuration.xml",
            br#"<Configuration><Version><Folder path="."/></Version></Configuration>"#,
        );
        let p = run(&tmp, &options()).unwrap();
        assert_eq!(p.declared(), None);
        assert!(p.version().unwrap().specification_raw.is_some());
        assert!(p.configuration().is_some());
        put(
            &tmp,
            "Metadata/Version.xml",
            br#"<Version><PreviousIncrementalRootDirectory name="../old"/></Version>"#,
        );
        assert!(run(&tmp, &options())
            .unwrap_err()
            .to_string()
            .starts_with("multi-version 1.x stores are not supported in this version:"));
        put(&tmp, "Metadata/Version.xml", b"invalid");
        assert!(run(&tmp, &options()).is_err());
    }
    /// A stale open tree or substituted public inventory must not bypass controls.
    #[test]
    fn mig_plan_reopens_controls_and_matches_inventory() {
        let tmp = source();
        put(&tmp, "notes.txt", b"opaque");
        let tree = Cdb1Tree::open(tmp.path()).unwrap();
        let mut inv = tree.inventory().unwrap();
        let mut opts = options();
        opts.extras = ExtrasPolicy::Carry;
        inv.entries.clear();
        assert!(plan(&tree, &inv, &opts).is_err());
        let inv = tree.inventory().unwrap();
        put(
            &tmp,
            "Metadata/Version.xml",
            br#"<Version><PreviousIncrementalRootDirectory name="../old"/></Version>"#,
        );
        let inv2 = tree.inventory().unwrap();
        assert!(plan(&tree, &inv2, &opts)
            .unwrap_err()
            .to_string()
            .contains("multi-version"));
        assert!(plan(&tree, &inv, &opts).is_err());
    }
    /// Every key is an exact source file; targets are literal safe relative names.
    #[test]
    fn mig_plan_rename_paths_and_metadata_capture() {
        let tmp = source();
        put(&tmp, "stray notes.txt", b"x");
        let mut opts = options();
        opts.extras = ExtrasPolicy::Carry;
        assert!(run(&tmp, &opts).is_err());
        for target in [
            "/absolute.txt",
            "../outside.txt",
            "a//b.txt",
            "a/./b.txt",
            "a\\b.txt",
            "extras/a__b.txt",
            "extras/a-b.txt",
            "extras/a.b.c",
            "extras/metadata/opaque.xml",
            "global_metadata/hide.dat",
            "versions/a.dat",
            "cdb_migrator-descriptor.json",
        ] {
            opts.rename_map
                .insert("stray notes.txt".into(), target.into());
            assert!(run(&tmp, &opts).is_err(), "{target}");
        }
        opts.rename_map
            .insert("stray notes.txt".into(), "extras/notes.txt".into());
        assert_eq!(
            run(&tmp, &opts).unwrap().moves()[0].target,
            "extras/notes.txt"
        );
        opts.rename_map
            .insert("missing".into(), "extras/other.txt".into());
        assert!(run(&tmp, &opts)
            .unwrap_err()
            .to_string()
            .contains("missing"));
    }
    /// Name/path collisions include every source and file-as-directory ancestors.
    #[test]
    fn mig_plan_collision_lists_all_paths() {
        let tmp = source();
        for name in ["one", "two", "three"] {
            put(&tmp, name, b"x");
        }
        let mut opts = options();
        opts.extras = ExtrasPolicy::Carry;
        opts.rename_map.insert("one".into(), "extras/x".into());
        opts.rename_map.insert("two".into(), "extras/x".into());
        opts.rename_map
            .insert("three".into(), "extras/x/child".into());
        let error = run(&tmp, &opts).unwrap_err().to_string();
        for name in ["one", "two", "three"] {
            assert!(error.contains(name), "{error}");
        }
    }
    /// Tiles may not change selectors, encoding, geocell, or U/R through a rename.
    #[test]
    fn mig_plan_tile_identity_and_directory_agreement() {
        let tmp = source();
        put(&tmp, TILE, b"x");
        let mut opts = payload_options(&[TILE]);
        for target in [
            TILE.to_ascii_lowercase().replace("s001", "s002"),
            TILE.to_ascii_lowercase().replace(".tif", ".png"),
            TILE.to_ascii_lowercase().replace("n32/", "n33/"),
            "extras/tile.tif".into(),
        ] {
            opts.rename_map.insert(TILE.into(), target.clone());
            assert!(run(&tmp, &opts).is_err(), "{target}");
        }
        opts.rename_map.insert(
            TILE.into(),
            TILE.to_ascii_lowercase()
                .replace("001_elevation", "001_height"),
        );
        assert!(run(&tmp, &opts).is_ok());
    }
    /// Required declarations must match source payloads exactly and IDs are unique.
    #[test]
    fn mig_plan_metadata_completeness_ids_and_seed() {
        let tmp = source();
        put(&tmp, TILE, b"x");
        assert!(run(&tmp, &options())
            .unwrap_err()
            .to_string()
            .contains(TILE));
        let mut opts = payload_options(&[TILE, "GTModel/absent.flt"]);
        assert!(run(&tmp, &opts)
            .unwrap_err()
            .to_string()
            .contains("absent.flt"));
        put(&tmp, "GTModel/absent.flt", b"x");
        opts.metadata.as_mut().unwrap().resources[1].record.id = "id0".into();
        assert!(run(&tmp, &opts).unwrap_err().to_string().contains("id0"));
        opts.metadata.as_mut().unwrap().resources[1].record.id = "id1".into();
        opts.id = Some(String::new());
        assert!(run(&tmp, &opts).is_err());
    }
    /// A plan is bound to one physical root; skipped bytes are also snapshotted.
    #[test]
    fn mig_plan_source_binding_and_fabricated_classification() {
        let tmp = source();
        put(&tmp, "notes.txt", b"x");
        let other = source();
        put(&other, "notes.txt", b"x");
        let mut opts = options();
        opts.extras = ExtrasPolicy::Carry;
        let p = run(&tmp, &opts).unwrap();
        assert!(p
            .verify_tree(&Cdb1Tree::open(other.path()).unwrap())
            .is_err());
        let tree = Cdb1Tree::open(tmp.path()).unwrap();
        let mut inv = tree.inventory().unwrap();
        inv.entries[0] = crate::Cdb1Entry::Metadata {
            rel_path: "notes.txt".into(),
            raw_path: tmp.path().join("notes.txt"),
        };
        assert!(plan(&tree, &inv, &opts).is_err());
        put(&tmp, "new.txt", b"x");
        assert!(p.verify_source().is_err());
    }

    /// CLI input must not lose duplicate rename keys during JSON parsing.
    #[test]
    fn mig_plan_rename_json_rejects_duplicate_keys() {
        assert_eq!(
            parse_rename_map(r#"{"Old.txt":"extras/new.txt"}"#).unwrap()["Old.txt"],
            "extras/new.txt"
        );
        for input in [
            r#"{"old":"one","old":"two"}"#,
            r#"{"old":5}"#,
            r#"[]"#,
            r#"null"#,
            r#"{"old":"../escape"}"#,
        ] {
            assert!(parse_rename_map(input).is_err(), "{input}");
        }
    }
    /// Filename-derived addresses do not certify mismatching source directories.
    #[test]
    fn mig_plan_default_preserves_directory_findings_but_override_must_agree() {
        let tmp = source();
        let mismatched = TILE.replace("/N32/", "/N33/");
        put(&tmp, &mismatched, b"x");
        let mut opts = payload_options(&[&mismatched]);
        let p = run(&tmp, &opts).unwrap();
        assert_eq!(p.moves()[0].target, mismatched.to_ascii_lowercase());
        let crate::Cdb1Entry::Tile(tile) = &p.inventory().entries[0] else {
            panic!()
        };
        assert!(!tile.findings.is_empty());
        opts.rename_map
            .insert(mismatched.clone(), TILE.to_ascii_lowercase());
        assert!(run(&tmp, &opts).is_ok());
        opts.rename_map
            .insert(mismatched.clone(), mismatched.to_ascii_lowercase());
        assert!(run(&tmp, &opts).is_err());
    }
    /// Unknown effective versions remain visible and extensions are provenance.
    #[test]
    fn mig_plan_unknown_config_and_extension_provenance() {
        let tmp = source();
        put(&tmp,"Metadata/Version.xml",br#"<Version><Specification version="1.0"/><Metadata standard="source_schema"/></Version>"#);
        put(&tmp,"Metadata/Configuration.xml",br#"<Configuration><Version><Folder path="."/><Specification version="unknown"/><Extension name="opaque" version="1"/><Metadata standard="config_schema"/></Version></Configuration>"#);
        let p = run(&tmp, &options()).unwrap();
        assert_eq!(p.declared(), Some("unknown"));
        assert!(p.effective_declaration().unwrap().specification.is_none());
        assert_eq!(
            p.effective_declaration()
                .unwrap()
                .extension
                .as_ref()
                .unwrap()
                .name,
            "opaque"
        );
        assert!(p.standard_note().unwrap().contains("config_schema"));
        assert_eq!(p.descriptor()["metadata_standard"], "NoMetadata");
        assert!(!p.findings().is_empty());
    }
    /// Configurations selecting no root, many roots or another root are refused.
    #[test]
    fn mig_plan_configuration_must_resolve_to_current_root() {
        let tmp = source();
        fs::create_dir(tmp.path().join("child")).unwrap();
        for xml in [
            r#"<Configuration/>"#,
            r#"<Configuration><Version><Folder path="child"/></Version></Configuration>"#,
            r#"<Configuration><Version><Folder path="."/></Version><Version><Folder path="."/></Version></Configuration>"#,
            r#"<Configuration><Version><Folder path="."/><PreviousIncrementalRootDirectory name="old"/></Version></Configuration>"#,
        ] {
            put(&tmp, "Metadata/Configuration.xml", xml.as_bytes());
            assert!(run(&tmp, &options()).is_err(), "{xml}");
        }
        put(
            &tmp,
            "Metadata/Configuration.xml",
            br#"<Configuration><Version><Folder path="child/.."/></Version></Configuration>"#,
        );
        assert!(run(&tmp, &options()).is_ok());
    }
    /// Dataset001 cannot evade required coverage, while 006 is operator-declared.
    #[test]
    fn mig_plan_coverage_uses_actual_inventory_dataset() {
        let tmp = source();
        put(&tmp, TILE, b"x");
        let mut opts = payload_options(&[TILE]);
        let decl = &mut opts.metadata.as_mut().unwrap().resources[0];
        decl.coverage = false;
        decl.record.domain_set = None;
        assert!(run(&tmp, &opts)
            .unwrap_err()
            .to_string()
            .contains("coverage"));
        fs::remove_file(tmp.path().join(TILE)).unwrap();
        let six = TILE
            .replace("001_Elevation", "006_RMDescriptor")
            .replace("D001", "D006")
            .replace(".tif", ".xml");
        put(&tmp, &six, b"x");
        opts.metadata.as_mut().unwrap().resources[0].source_path = six;
        assert!(run(&tmp, &opts).is_ok());
    }
    /// Range-invalid names fail address conversion regardless of extras policy.
    #[test]
    fn mig_plan_refuses_out_of_range_tile() {
        let tmp = source();
        let invalid = TILE.replace("_R0", "_R1");
        put(&tmp, &invalid, b"x");
        let mut opts = payload_options(&[&invalid]);
        opts.extras = ExtrasPolicy::Skip;
        assert!(run(&tmp, &opts)
            .unwrap_err()
            .to_string()
            .contains("out of range"));
    }
    /// Even unchanged-length payload edits and new control declarations invalidate plans.
    #[test]
    fn mig_plan_snapshot_rechecks_all_bytes_and_controls() {
        let tmp = source();
        put(&tmp, "notes.txt", b"one");
        let mut opts = options();
        opts.extras = ExtrasPolicy::Skip;
        let p = run(&tmp, &opts).unwrap();
        put(&tmp, "notes.txt", b"two");
        assert!(p.verify_source().is_err());
        let p = run(&tmp, &opts).unwrap();
        put(&tmp, "Metadata/Version.xml", b"<Version/>");
        assert!(p.verify_source().is_err());
    }
    /// Source links cannot be accepted even by skipping extras or a forged inventory.
    #[cfg(unix)]
    #[test]
    fn mig_plan_unsafe_source_and_replacement_refused() {
        let tmp = source();
        let other = source();
        put(&other, "data", b"x");
        std::os::unix::fs::symlink(other.path().join("data"), tmp.path().join("alias")).unwrap();
        let mut opts = options();
        opts.extras = ExtrasPolicy::Skip;
        assert!(run(&tmp, &opts).is_err());
        fs::remove_file(tmp.path().join("alias")).unwrap();
        put(&tmp, "alias", b"x");
        let p = run(&tmp, &opts).unwrap();
        fs::remove_file(tmp.path().join("alias")).unwrap();
        std::os::unix::fs::symlink(other.path().join("data"), tmp.path().join("alias")).unwrap();
        assert!(p.verify_source().is_err());
    }

    /// Descriptor extension entries must be nonempty and usable by cdb-lint.
    #[test]
    fn mig_plan_refuses_empty_extension() {
        let tmp = source();
        put(&tmp, "notes.", b"x");
        let mut opts = options();
        opts.extras = ExtrasPolicy::Carry;
        assert!(run(&tmp, &opts).unwrap_err().to_string().contains("notes."));
    }

    /// Recognized metadata is always carried, and unknown skips remain explicit.
    #[test]
    fn mig_plan_skip_only_unknown_metadata_capture_and_missing_manifest() {
        let tmp = source();
        put(&tmp, "Metadata/Version.xml", b"<Version/>");
        put(&tmp, "notes.txt", b"x");
        assert!(run(&tmp, &PlanOptions::default())
            .unwrap_err()
            .to_string()
            .contains("manifest"));
        let mut opts = options();
        opts.extras = ExtrasPolicy::Skip;
        let p = run(&tmp, &opts).unwrap();
        assert_eq!(p.moves().len(), 1);
        assert_eq!(p.skipped(), &["notes.txt"]);
        assert!(p.version_path().unwrap().ends_with("Metadata/Version.xml"));
        put(&tmp, "Metadata/metadata/opaque.json", b"opaque");
        assert!(run(&tmp, &opts)
            .unwrap_err()
            .to_string()
            .contains("metadata-captured"));
        opts.rename_map.insert(
            "Metadata/metadata/opaque.json".into(),
            "extras/source_metadata/opaque.json".into(),
        );
        assert!(run(&tmp, &opts).is_ok());
    }
    /// Fold guards remain explicit on hosts without case-distinct filenames.
    #[test]
    fn mig_plan_case_collision_guard_lists_every_original() {
        let moves = vec![
            PlannedMove {
                source: "First".into(),
                target: "extras/THING.dat".into(),
                bucket: Bucket::Extra,
                source_path: PathBuf::from("/first"),
            },
            PlannedMove {
                source: "Second".into(),
                target: "extras/thing.dat".into(),
                bucket: Bucket::Extra,
                source_path: PathBuf::from("/second"),
            },
        ];
        let mut errors = vec![];
        collisions(&moves, &[], &mut errors);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("First") && errors[0].contains("Second"));
        assert!(reserved_target("extras/Metadata/opaque.XML"));
        assert!(reserved_target("Global_Metadata/custom.bin"));
    }
    /// Mutable programmatic declarations and map values cannot mutate an issued plan.
    #[test]
    fn mig_plan_owns_effective_input_and_revalidates_programmatic_metadata() {
        let tmp = source();
        put(&tmp, TILE, b"x");
        let mut opts = payload_options(&[TILE]);
        let p = run(&tmp, &opts).unwrap();
        opts.metadata.as_mut().unwrap().resources[0]
            .record
            .keywords
            .clear();
        assert!(!p.operator_metadata().resources[0]
            .record
            .keywords
            .is_empty());
        assert!(run(&tmp, &opts)
            .unwrap_err()
            .to_string()
            .contains("keywords"));
        p.verify_source().unwrap();
    }
}
