//! Total, deterministic inspection of a CDB 1.x source tree.
//!
//! Payloads are opaque. Malformed names and control XML are findings, while
//! filesystem failures are errors. Links and special files are inventoried
//! without following/opening them. ASCII case folding in control discovery is
//! a migration safety guard, not certification of the source naming rules.
//!
//! Control declarations and findings describe the state at `open`; `inventory`
//! walks the current tree. Neither is an atomic filesystem snapshot. Callers
//! must keep the source stable during inspection and recheck it before copying.
use std::fs;
use std::path::{Path, PathBuf};

use crate::grammar::{DatasetDir, GeocellId, TileFileName};
use crate::version_meta::{ConfigurationXml, VersionXml};
use crate::Cdb1Error;

/// An opened source and its inspection-time control metadata.
#[derive(Debug)]
pub struct Cdb1Tree {
    root: PathBuf,
    version: Option<VersionXml>,
    configuration: Option<ConfigurationXml>,
    version_path: Option<PathBuf>,
    configuration_path: Option<PathBuf>,
    findings: Vec<ReaderFinding>,
}

/// Depth-first, name-ordered inventory of every non-directory source entry.
#[derive(Debug)]
pub struct Inventory {
    pub entries: Vec<Cdb1Entry>,
}

impl Inventory {
    /// Whether any entry must not be treated as an ordinary copyable file.
    pub fn has_unsafe_entries(&self) -> bool {
        self.entries
            .iter()
            .any(|e| matches!(e, Cdb1Entry::Unsafe { .. }))
    }
}

/// Source entry type observed with `symlink_metadata`, without following links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceEntryType {
    RegularFile,
    Symlink,
    /// Socket, FIFO, device, or another non-directory, non-regular entry.
    Other,
}

/// Every leaf receives one classification; unsafe leaves retain exact identity.
#[derive(Debug)]
pub enum Cdb1Entry {
    Tile(Cdb1TileRef),
    Global {
        kind: GlobalKind,
        rel_path: String,
        raw_path: PathBuf,
    },
    Metadata {
        rel_path: String,
        raw_path: PathBuf,
    },
    Unrecognized {
        rel_path: String,
        raw_path: PathBuf,
        why: String,
    },
    /// A link/special file or a file with an unrepresentable relative path.
    /// `raw_path` is operational identity, never a lossy display conversion.
    Unsafe {
        raw_path: PathBuf,
        rel_path: Option<String>,
        entry_type: SourceEntryType,
        why: String,
    },
}

impl Cdb1Entry {
    /// Exact operational source path (root joined to original components).
    pub fn raw_path(&self) -> &Path {
        match self {
            Self::Tile(tile) => &tile.raw_path,
            Self::Global { raw_path, .. }
            | Self::Metadata { raw_path, .. }
            | Self::Unrecognized { raw_path, .. }
            | Self::Unsafe { raw_path, .. } => raw_path,
        }
    }

    /// Slash-separated original relative spelling, or `None` for non-UTF-8.
    pub fn rel_path(&self) -> Option<&str> {
        match self {
            Self::Tile(tile) => Some(&tile.rel_path),
            Self::Global { rel_path, .. }
            | Self::Metadata { rel_path, .. }
            | Self::Unrecognized { rel_path, .. } => Some(rel_path),
            Self::Unsafe { rel_path, .. } => rel_path.as_deref(),
        }
    }

    /// Observed source type; an unsafe path can still name a regular file.
    pub fn entry_type(&self) -> SourceEntryType {
        match self {
            Self::Unsafe { entry_type, .. } => *entry_type,
            _ => SourceEntryType::RegularFile,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalKind {
    GtModel,
    MModel,
    Navigation,
}

/// Parseable tiled name, retaining disagreements rather than dropping the file.
#[derive(Debug)]
pub struct Cdb1TileRef {
    pub rel_path: String,
    pub raw_path: PathBuf,
    pub file: TileFileName,
    /// File/directory disagreements and U/R range findings.
    pub findings: Vec<String>,
}

/// Typed reader diagnostics distinguish parse safety from declaration policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderFindingKind {
    MalformedControlMetadata,
    AmbiguousControlMetadata,
    UnsafeControlMetadata,
    MissingVersionMetadata,
    MissingSpecification,
    UnknownSpecification,
    /// Recorded provenance; extension semantics have not been implemented.
    ExtensionDeclaration,
}

/// A control/declaration finding with its original operational path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReaderFinding {
    /// Actual source path, or expected Version.xml path when it is absent.
    pub raw_path: PathBuf,
    pub kind: ReaderFindingKind,
    pub message: String,
}

impl Cdb1Tree {
    /// Inspect a directory and discover unambiguous, ASCII-case-folded controls.
    /// Missing controls are allowed; malformed/unsafe controls become findings.
    /// The root and all its ancestors must be real directories: provide a
    /// physical path rather than an alias such as macOS `/var` or `/tmp`.
    pub fn open(root: &Path) -> Result<Self, Cdb1Error> {
        check_root(root)?;
        let mut tree = Self {
            root: root.to_path_buf(),
            version: None,
            configuration: None,
            version_path: None,
            configuration_path: None,
            findings: Vec::new(),
        };
        tree.discover_controls()?;
        if tree.version_path.is_none() && tree.control_metadata_safe() {
            tree.finding(
                root.join("Metadata/Version.xml"),
                ReaderFindingKind::MissingVersionMetadata,
                "Version.xml is absent; no specification version is inferred".into(),
            );
        }
        Ok(tree)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn version(&self) -> Option<&VersionXml> {
        self.version.as_ref()
    }

    pub fn configuration(&self) -> Option<&ConfigurationXml> {
        self.configuration.as_ref()
    }

    /// Exact discovered path, including when its contents failed to parse.
    pub fn version_path(&self) -> Option<&Path> {
        self.version_path.as_deref()
    }

    /// Exact discovered path, including when its contents failed to parse.
    pub fn configuration_path(&self) -> Option<&Path> {
        self.configuration_path.as_deref()
    }

    pub fn findings(&self) -> &[ReaderFinding] {
        &self.findings
    }

    /// No malformed, ambiguous, or unsafe controls were found at `open`.
    /// This does not approve chains, unknown declarations, or extensions for
    /// migration; their provenance remains in the parsed controls and findings.
    pub fn control_metadata_safe(&self) -> bool {
        !self.findings.iter().any(|f| {
            matches!(
                f.kind,
                ReaderFindingKind::MalformedControlMetadata
                    | ReaderFindingKind::AmbiguousControlMetadata
                    | ReaderFindingKind::UnsafeControlMetadata
            )
        })
    }

    /// Inspect all current leaves without following links. Errors never produce
    /// a partial successful inventory. Raw names determine sibling order.
    pub fn inventory(&self) -> Result<Inventory, Cdb1Error> {
        check_root(&self.root)?;
        let mut entries = Vec::new();
        // Explicit stack keeps deeply nested input off the Rust call stack.
        let mut pending = child_paths(&self.root, Path::new(""))?;
        pending.reverse();
        while let Some(relative) = pending.pop() {
            let raw_path = self.root.join(&relative);
            let meta = metadata(&raw_path)?;
            if meta.is_dir() {
                let mut children = child_paths(&raw_path, &relative)?;
                children.reverse();
                pending.extend(children);
                continue;
            }
            let entry_type = if meta.file_type().is_symlink() {
                SourceEntryType::Symlink
            } else if meta.is_file() {
                SourceEntryType::RegularFile
            } else {
                SourceEntryType::Other
            };
            entries.push(classify_leaf(raw_path, &relative, entry_type));
        }
        Ok(Inventory { entries })
    }

    fn finding(&mut self, raw_path: PathBuf, kind: ReaderFindingKind, message: String) {
        self.findings.push(ReaderFinding {
            raw_path,
            kind,
            message,
        });
    }

    fn discover_controls(&mut self) -> Result<(), Cdb1Error> {
        let root = self.root.clone();
        let children = child_paths(&root, Path::new(""))?;
        let Some(dir) = self.control_path(&root, &children, "Metadata") else {
            return Ok(());
        };
        if !metadata(&dir)?.is_dir() {
            self.finding(
                dir,
                ReaderFindingKind::UnsafeControlMetadata,
                "Metadata is not a real directory; links are not followed".into(),
            );
            return Ok(());
        }
        let children = child_paths(&dir, Path::new(""))?;
        for (name, is_version) in [("Version.xml", true), ("Configuration.xml", false)] {
            if let Some(path) = self.control_path(&dir, &children, name) {
                if is_version {
                    self.version_path = Some(path.clone());
                } else {
                    self.configuration_path = Some(path.clone());
                }
                self.read_control(&path, is_version)?;
            }
        }
        Ok(())
    }

    fn control_path(&mut self, parent: &Path, children: &[PathBuf], name: &str) -> Option<PathBuf> {
        let paths: Vec<_> = children
            .iter()
            .filter(|p| named(p, name))
            .map(|p| parent.join(p))
            .collect();
        if paths.len() > 1 {
            for path in paths {
                self.finding(
                    path,
                    ReaderFindingKind::AmbiguousControlMetadata,
                    format!("{name} paths collide after ASCII case folding"),
                );
            }
            None
        } else {
            paths.into_iter().next()
        }
    }

    fn read_control(&mut self, path: &Path, is_version: bool) -> Result<(), Cdb1Error> {
        if !metadata(path)?.is_file() {
            self.finding(
                path.to_path_buf(),
                ReaderFindingKind::UnsafeControlMetadata,
                "control is not a regular file; links and special files are not opened".into(),
            );
            return Ok(());
        }
        let bytes = fs::read(path).map_err(|e| Cdb1Error::Io(path.to_path_buf(), e))?;
        let xml = match std::str::from_utf8(&bytes) {
            Ok(xml) => xml,
            Err(why) => {
                self.finding(
                    path.to_path_buf(),
                    ReaderFindingKind::MalformedControlMetadata,
                    format!("control XML is not UTF-8: {why}"),
                );
                return Ok(());
            }
        };
        let result = if is_version {
            VersionXml::parse(xml).map(|version| {
                self.declaration_findings(path, &version, "Version");
                self.version = Some(version);
            })
        } else {
            ConfigurationXml::parse(xml).map(|configuration| {
                for (index, version) in configuration.versions.iter().enumerate() {
                    self.declaration_findings(
                        path,
                        &version.declaration,
                        &format!(
                            "Configuration Version {} (folder {:?})",
                            index + 1,
                            version.folder
                        ),
                    );
                }
                self.configuration = Some(configuration);
            })
        };
        if let Err(why) = result {
            self.finding(
                path.to_path_buf(),
                ReaderFindingKind::MalformedControlMetadata,
                why,
            );
        }
        Ok(())
    }

    fn declaration_findings(&mut self, path: &Path, version: &VersionXml, context: &str) {
        if version.specification_raw.is_none() {
            self.finding(
                path.to_path_buf(),
                ReaderFindingKind::MissingSpecification,
                format!("{context}: Specification declaration is absent; no version is inferred"),
            );
        } else if version.specification.is_none() {
            self.finding(
                path.to_path_buf(),
                ReaderFindingKind::UnknownSpecification,
                format!(
                    "{context}: unrecognized Specification {:?}",
                    version.specification_raw
                ),
            );
        }
        if let Some(extension) = &version.extension {
            self.finding(path.to_path_buf(), ReaderFindingKind::ExtensionDeclaration, format!("{context}: extension {:?} version {:?} is provenance only; semantics are not implemented", extension.name, extension.version));
        }
    }
}

fn check_root(root: &Path) -> Result<(), Cdb1Error> {
    if root.to_str().is_none() {
        return Err(Cdb1Error::Refused("source root path is not UTF-8".into()));
    }
    // Components removes trailing separators/`.` so a terminal link cannot
    // be hidden by `link/` or `link/.`. Check ancestors in traversal order:
    // even `alias/..` must not traverse a link to reach an otherwise safe root.
    let supplied: PathBuf = root.components().collect();
    let checked = if supplied.is_absolute() {
        supplied
    } else {
        std::env::current_dir()
            .map_err(|e| Cdb1Error::Io(root.to_path_buf(), e))?
            .join(supplied)
    };
    let ancestors: Vec<_> = checked.ancestors().collect();
    for path in ancestors.into_iter().rev() {
        let meta = metadata(path)?;
        if meta.file_type().is_symlink() {
            return Err(Cdb1Error::Refused(format!(
                "source root {root:?} traverses symbolic link {path:?}"
            )));
        }
        if !meta.is_dir() {
            return Err(Cdb1Error::NotADirectory(root.to_path_buf()));
        }
    }
    Ok(())
}

fn metadata(path: &Path) -> Result<fs::Metadata, Cdb1Error> {
    fs::symlink_metadata(path).map_err(|e| Cdb1Error::Io(path.to_path_buf(), e))
}

/// Sorted original relative paths, with every read_dir error propagated.
fn child_paths(dir: &Path, parent: &Path) -> Result<Vec<PathBuf>, Cdb1Error> {
    let mut children = fs::read_dir(dir)
        .map_err(|e| Cdb1Error::Io(dir.to_path_buf(), e))?
        .map(|entry| entry.map(|e| parent.join(e.file_name())))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| Cdb1Error::Io(dir.to_path_buf(), e))?;
    children.sort();
    Ok(children)
}

fn named(path: &Path, name: &str) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case(name))
}

fn slash_path(path: &Path) -> Option<String> {
    path.components()
        .map(|p| p.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()
        .map(|parts| parts.join("/"))
}

fn classify_leaf(raw_path: PathBuf, relative: &Path, entry_type: SourceEntryType) -> Cdb1Entry {
    match (entry_type, slash_path(relative)) {
        (SourceEntryType::RegularFile, Some(rel)) => classify(raw_path, rel),
        (_, rel_path) => {
            let why = match entry_type {
                SourceEntryType::Symlink => "symbolic link is not followed",
                SourceEntryType::Other => "non-regular source entry is not opened",
                SourceEntryType::RegularFile => "relative path is not UTF-8; raw identity retained",
            };
            Cdb1Entry::Unsafe {
                raw_path,
                rel_path,
                entry_type,
                why: why.into(),
            }
        }
    }
}

fn classify(raw_path: PathBuf, rel_path: String) -> Cdb1Entry {
    let parts: Vec<_> = rel_path.split('/').collect();
    if parts[0].eq_ignore_ascii_case("Tiles") {
        if parts.len() == 7 {
            return classify_tile(raw_path, &rel_path, &parts);
        }
        let why = format!(
            "Tiles path has {} components; the grammar needs 7",
            parts.len()
        );
        return Cdb1Entry::Unrecognized {
            raw_path,
            rel_path,
            why,
        };
    }
    if parts.len() >= 2 {
        for (name, kind) in [
            ("GTModel", GlobalKind::GtModel),
            ("MModel", GlobalKind::MModel),
            ("Navigation", GlobalKind::Navigation),
        ] {
            if parts[0].eq_ignore_ascii_case(name) {
                return Cdb1Entry::Global {
                    kind,
                    rel_path,
                    raw_path,
                };
            }
        }
        if parts[0].eq_ignore_ascii_case("Metadata") {
            return Cdb1Entry::Metadata { rel_path, raw_path };
        }
    }
    Cdb1Entry::Unrecognized {
        rel_path,
        raw_path,
        why: "not under Tiles/, GTModel/, MModel/, Navigation/ or Metadata/".into(),
    }
}

fn classify_tile(raw_path: PathBuf, rel: &str, parts: &[&str]) -> Cdb1Entry {
    let file = match TileFileName::parse(parts[6]) {
        Ok(file) => file,
        Err(why) => {
            return Cdb1Entry::Unrecognized {
                rel_path: rel.into(),
                raw_path,
                why,
            }
        }
    };
    let mut findings = Vec::new();
    match GeocellId::from_dirs(parts[1], parts[2]) {
        Ok(dir_cell) if dir_cell != file.geocell => findings.push(format!(
            "directories {}/{} disagree with file geocell {}{}",
            parts[1],
            parts[2],
            file.geocell.lat_dir_name(),
            file.geocell.lon_dir_name()
        )),
        Ok(_) => {}
        Err(why) => findings.push(format!("geocell directories: {why}")),
    }
    match DatasetDir::parse(parts[3]) {
        Ok(dir) if dir.code != file.dataset => findings.push(format!(
            "dataset directory {} disagrees with file dataset D{:03}",
            parts[3], file.dataset
        )),
        Ok(_) => {}
        Err(why) => findings.push(format!("dataset directory: {why}")),
    }
    if !parts[4].eq_ignore_ascii_case(&file.lod.dir_name()) {
        findings.push(format!(
            "LOD directory {} disagrees with file LOD {}",
            parts[4],
            file.lod.token()
        ));
    }
    let dir = parts[5];
    let uref_ok = dir
        .as_bytes()
        .first()
        .is_some_and(|c| c.eq_ignore_ascii_case(&b'U'))
        && dir.get(1..).is_some_and(|digits| {
            !digits.is_empty()
                && digits.bytes().all(|c| c.is_ascii_digit())
                && digits.parse::<u32>().is_ok_and(|u| u == file.uref)
        });
    if !uref_ok {
        findings.push(format!(
            "UREF directory {dir} disagrees with file U{}",
            file.uref
        ));
    }
    // §8.6.2.5/§8.6.3.1: parsed LOD is −10..=23, so this shift cannot overflow.
    let per_geocell = if file.lod.value() <= 0 {
        1_u32
    } else {
        1_u32 << file.lod.value()
    };
    if file.uref >= per_geocell {
        findings.push(format!(
            "U{} out of range for {}",
            file.uref,
            file.lod.token()
        ));
    }
    if file.rref >= per_geocell {
        findings.push(format!(
            "R{} out of range for {}",
            file.rref,
            file.lod.token()
        ));
    }
    Cdb1Entry::Tile(Cdb1TileRef {
        rel_path: rel.into(),
        raw_path,
        file,
        findings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap()
    }

    /// Control discovery must refuse case-folded collisions on every host,
    /// including filesystems unable to create those two source names natively.
    #[test]
    fn mig_reader_control_selection_refuses_ambiguous_original_paths() {
        for name in ["Metadata", "Version.xml", "Configuration.xml"] {
            let tmp = tempdir();
            let mut tree = Cdb1Tree::open(tmp.path()).unwrap();
            let upper = PathBuf::from(name.to_ascii_uppercase());
            let lower = PathBuf::from(name.to_ascii_lowercase());
            let children = vec![upper.clone(), PathBuf::from("other"), lower.clone()];
            assert_eq!(tree.control_path(tmp.path(), &children, name), None);
            assert!(!tree.control_metadata_safe());
            let paths: Vec<_> = tree
                .findings()
                .iter()
                .filter(|f| f.kind == ReaderFindingKind::AmbiguousControlMetadata)
                .map(|f| f.raw_path.clone())
                .collect();
            assert_eq!(paths, [tmp.path().join(upper), tmp.path().join(lower)]);
        }
    }

    /// Unique discovery preserves case; absent names are not selected heuristically.
    #[test]
    fn mig_reader_control_selection_preserves_unique_path() {
        let tmp = tempdir();
        let mut tree = Cdb1Tree::open(tmp.path()).unwrap();
        let children = vec![PathBuf::from("version.xml"), PathBuf::from("myversion.xml")];
        assert_eq!(
            tree.control_path(tmp.path(), &children, "Version.xml"),
            Some(tmp.path().join("version.xml"))
        );
        assert_eq!(
            tree.control_path(tmp.path(), &children, "Configuration.xml"),
            None
        );
        assert!(tree.control_metadata_safe());
    }

    /// Non-UTF-8 source identity survives classification even where the host
    /// filesystem prohibits creating such a name. No display string becomes I/O.
    #[cfg(unix)]
    #[test]
    fn mig_reader_non_utf8_classification_retains_bytes_and_type() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let rel = PathBuf::from(std::ffi::OsString::from_vec(
            b"Metadata/bad\xff/file".to_vec(),
        ));
        let raw = Path::new("source").join(&rel);
        for entry_type in [
            SourceEntryType::RegularFile,
            SourceEntryType::Symlink,
            SourceEntryType::Other,
        ] {
            let entry = classify_leaf(raw.clone(), &rel, entry_type);
            assert_eq!(
                entry.raw_path().as_os_str().as_bytes(),
                b"source/Metadata/bad\xff/file"
            );
            assert_eq!(entry.rel_path(), None);
            assert_eq!(entry.entry_type(), entry_type);
            assert!(matches!(entry, Cdb1Entry::Unsafe { .. }));
        }
    }
}
