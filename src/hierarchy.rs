//! File Hierarchy Structure requirements module.
//!
//! Implements the `/req/core/file-system` requirements class (spec §7.5):
//! the datastore root, the hierarchy under it, and the `global_metadata`
//! folder. Also emits the empty-folder recommendation of §7.4.5
//! (Recommendation Name4), since detecting it requires walking the tree.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::naming::NamingViolation;

/// Folder at the datastore root holding all global metadata, controlled
/// vocabularies, and enumerations (Requirement File6, §7.5.7).
pub const GLOBAL_METADATA_DIR: &str = "global_metadata";

/// Recommended name of the root folder (Recommendation RFile1, §7.5.6).
pub const RECOMMENDED_ROOT_NAME: &str = "cdb";

/// Operational errors raised while creating, opening, or resolving paths in
/// a datastore hierarchy.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum HierarchyError {
    #[error("datastore root {0:?} does not exist (/req/core/file-cdb-root-location)")]
    RootMissing(PathBuf),
    #[error("datastore root {0:?} is not a directory (/req/core/file-structure)")]
    RootNotADirectory(PathBuf),
    #[error(transparent)]
    Naming(#[from] NamingViolation),
    #[error("i/o error under datastore root: {0}")]
    Io(#[from] std::io::Error),
}

/// A violation of a SHALL requirement found by [`DatastoreLayout::validate`].
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum HierarchyViolation {
    /// Requirement File6: `global_metadata` must exist at the root.
    #[error(
        "no global_metadata folder at datastore root {root:?} (violates /req/core/file-root-global-metadata)"
    )]
    MissingGlobalMetadata { root: PathBuf },
}

/// A finding against a SHOULD recommendation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum HierarchyWarning {
    /// Recommendation Name4 (§7.4.5): empty folders should be avoided.
    EmptyFolder(PathBuf),
    /// Recommendation RFile1 (§7.5.6): the root should be named `cdb`.
    RootNameNotCdb { name: String },
}

impl fmt::Display for HierarchyWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HierarchyWarning::EmptyFolder(path) => {
                write!(
                    f,
                    "empty folder {path:?} (recommendation /req/core/name-empty-folders A)"
                )
            }
            HierarchyWarning::RootNameNotCdb { name } => write!(
                f,
                "datastore root is named {name:?}; \"cdb\" is recommended \
                 (/rec/core/file-hierarchy-root-name)"
            ),
        }
    }
}

/// Findings from validating a datastore hierarchy.
#[derive(Debug, Default)]
pub struct HierarchyReport {
    pub violations: Vec<HierarchyViolation>,
    pub warnings: Vec<HierarchyWarning>,
}

/// The logical root of any CDB hierarchy (Requirement File5, §7.5.5).
pub const fn logical_root() -> &'static str {
    "/"
}

/// Normalizes a datastore-relative path to its logical form, which always
/// begins with `/` (Requirement File5).
pub fn logical_path(path: &str) -> Result<String, NamingViolation> {
    crate::naming::validate_path(path)?;
    let relative = path.strip_prefix('/').unwrap_or(path);
    Ok(format!("/{relative}"))
}

/// A physical CDB datastore rooted at a directory. All content is reachable
/// from the root (Requirements File2, File3).
#[derive(Debug, Clone)]
pub struct DatastoreLayout {
    root: PathBuf,
}

impl DatastoreLayout {
    /// Creates a new datastore under `parent` using the recommended root
    /// name `cdb` (RFile1), with its `global_metadata` folder (File6).
    pub fn create(parent: &Path) -> Result<Self, HierarchyError> {
        Self::create_named(parent, RECOMMENDED_ROOT_NAME)
    }

    /// Creates a new datastore under `parent` with a custom root name.
    pub fn create_named(parent: &Path, root_name: &str) -> Result<Self, HierarchyError> {
        crate::naming::validate_component(root_name)?;
        let root = parent.join(root_name);
        fs::create_dir(&root)?;
        fs::create_dir(root.join(GLOBAL_METADATA_DIR))?;
        Ok(Self { root })
    }

    /// Opens an existing datastore root. Structural conformance is checked
    /// by [`Self::validate`], not here.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, HierarchyError> {
        let root = root.into();
        if !root.exists() {
            return Err(HierarchyError::RootMissing(root));
        }
        if !root.is_dir() {
            return Err(HierarchyError::RootNotADirectory(root));
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn global_metadata_dir(&self) -> PathBuf {
        self.root.join(GLOBAL_METADATA_DIR)
    }

    /// Resolves a datastore-relative logical path to a physical path under
    /// the root. Rejects traversal so content cannot escape the root
    /// (Requirement File2-B).
    pub fn resolve(&self, logical: &str) -> Result<PathBuf, HierarchyError> {
        crate::naming::validate_path(logical)?;
        let relative = logical.strip_prefix('/').unwrap_or(logical);
        Ok(self.root.join(relative))
    }

    /// Checks the SHALL requirements and SHOULD recommendations of this
    /// module against the on-disk state.
    pub fn validate(&self) -> Result<HierarchyReport, HierarchyError> {
        let mut report = HierarchyReport::default();
        if !self.global_metadata_dir().is_dir() {
            report
                .violations
                .push(HierarchyViolation::MissingGlobalMetadata {
                    root: self.root.clone(),
                });
        }
        // RFile1's root-name match is a guard, so it folds ASCII case
        // (`naming::guard_eq`): on a case-insensitive filesystem `CDB/` *is*
        // the recommended root, and warning about it would be noise. Its
        // spelling remains Requirement Name6's business.
        let root_name = self.root.file_name().map(|n| n.to_string_lossy());
        let recommended = root_name
            .as_deref()
            .is_some_and(|name| crate::naming::guard_eq(name, RECOMMENDED_ROOT_NAME));
        if !recommended {
            report.warnings.push(HierarchyWarning::RootNameNotCdb {
                name: root_name.unwrap_or_default().into_owned(),
            });
        }
        walk_empty_dirs(&self.root, &mut report.warnings)?;
        Ok(report)
    }
}

/// Recursively records empty directories. Symlinks are neither followed nor
/// reported, so links to external resources (Permission PFile1) are safe.
fn walk_empty_dirs(dir: &Path, warnings: &mut Vec<HierarchyWarning>) -> std::io::Result<()> {
    let mut saw_entry = false;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        saw_entry = true;
        if entry.file_type()?.is_dir() {
            walk_empty_dirs(&entry.path(), warnings)?;
        }
    }
    if !saw_entry {
        warnings.push(HierarchyWarning::EmptyFolder(dir.to_path_buf()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// §7.5.2 Requirement File1 — folders and hierarchy are supported.
    #[test]
    fn req_core_file_structure_supports_folder_hierarchy() {
        let tmp = tempdir().unwrap();
        let layout = DatastoreLayout::create(tmp.path()).unwrap();
        let nested = layout.resolve("Tiles/N50/E010").unwrap();
        fs::create_dir_all(&nested).unwrap();
        assert!(nested.is_dir());
    }

    /// §7.5.3 Requirement File2 — the root exists and content lives under it.
    #[test]
    fn req_core_file_cdb_root_location_contains_all_content() {
        let tmp = tempdir().unwrap();
        let layout = DatastoreLayout::create(tmp.path()).unwrap();
        assert!(layout.root().ends_with("cdb"));
        let resolved = layout.resolve("Metadata/Version.xml").unwrap();
        assert!(resolved.starts_with(layout.root()));
    }

    /// §7.5.3 File2-B — paths cannot escape the root.
    #[test]
    fn req_core_file_cdb_root_location_rejects_escape() {
        let tmp = tempdir().unwrap();
        let layout = DatastoreLayout::create(tmp.path()).unwrap();
        assert!(matches!(
            layout.resolve("../outside"),
            Err(HierarchyError::Naming(
                NamingViolation::PathTraversal { .. }
            ))
        ));
    }

    /// §7.5.4 Requirement File3 — opening requires an existing directory.
    #[test]
    fn req_core_file_hierarchy_open_requires_existing_directory_root() {
        let tmp = tempdir().unwrap();
        assert!(matches!(
            DatastoreLayout::open(tmp.path().join("missing")),
            Err(HierarchyError::RootMissing(_))
        ));
        let file = tmp.path().join("not_a_dir");
        fs::write(&file, b"x").unwrap();
        assert!(matches!(
            DatastoreLayout::open(&file),
            Err(HierarchyError::RootNotADirectory(_))
        ));
        let layout = DatastoreLayout::create(tmp.path()).unwrap();
        assert!(DatastoreLayout::open(layout.root()).is_ok());
    }

    /// §7.5.4 Permission PFile1 — symlinked subfolders are allowed.
    #[cfg(unix)]
    #[test]
    fn per_core_file_hierarchy_link_symlinked_subfolder_is_accessible() {
        let tmp = tempdir().unwrap();
        let layout = DatastoreLayout::create(tmp.path()).unwrap();
        let external = tmp.path().join("external_assets");
        fs::create_dir(&external).unwrap();
        fs::write(external.join("data.tif"), b"x").unwrap();
        std::os::unix::fs::symlink(&external, layout.root().join("Linked")).unwrap();

        let resolved = layout.resolve("Linked/data.tif").unwrap();
        assert!(resolved.starts_with(layout.root()));
        assert!(fs::read(&resolved).is_ok());
        // Validation must tolerate the symlink (and must not follow it).
        layout.validate().unwrap();
    }

    /// §7.5.5 Requirement File5 — the logical hierarchy begins with `/`.
    #[test]
    fn req_core_file_hierarchy_root_logical_paths_begin_with_slash() {
        assert_eq!(logical_root(), "/");
        assert_eq!(logical_path("Tiles").unwrap(), "/Tiles");
        assert_eq!(logical_path("/Tiles").unwrap(), "/Tiles");
        assert_eq!(logical_path("/").unwrap(), "/");
    }

    /// §7.5.6 Recommendation RFile1 — root SHOULD be named `cdb`.
    #[test]
    fn rec_core_file_hierarchy_root_name_cdb_recommended() {
        let tmp = tempdir().unwrap();
        let layout = DatastoreLayout::create(tmp.path()).unwrap();
        let report = layout.validate().unwrap();
        assert!(
            !report
                .warnings
                .iter()
                .any(|w| matches!(w, HierarchyWarning::RootNameNotCdb { .. }))
        );

        let named = DatastoreLayout::create_named(tmp.path(), "MyStore").unwrap();
        let report = named.validate().unwrap();
        assert!(report.warnings.contains(&HierarchyWarning::RootNameNotCdb {
            name: "MyStore".into()
        }));
    }

    /// §7.5.6 Recommendation RFile1 with the crate's case stance — matching
    /// the root folder's name is a *guard*, so it folds ASCII case. On a
    /// case-insensitive, case-preserving filesystem `CDB/` **is** the
    /// recommended root, and a recommendation warning for it would be noise.
    #[test]
    fn rec_core_file_hierarchy_root_name_match_folds_case() {
        for name in ["CDB", "Cdb", "cDb"] {
            let tmp = tempdir().unwrap();
            let layout = DatastoreLayout::create_named(tmp.path(), name).unwrap();
            let report = layout.validate().unwrap();
            assert!(
                !report
                    .warnings
                    .iter()
                    .any(|w| matches!(w, HierarchyWarning::RootNameNotCdb { .. })),
                "{name}: {:?}",
                report.warnings
            );
        }
    }

    /// §7.5.7 Requirement File6 — `global_metadata` at the root.
    #[test]
    fn req_core_file_root_global_metadata_created_and_required() {
        let tmp = tempdir().unwrap();
        let layout = DatastoreLayout::create(tmp.path()).unwrap();
        assert!(layout.global_metadata_dir().is_dir());
        let report = layout.validate().unwrap();
        assert!(report.violations.is_empty());

        fs::remove_dir(layout.global_metadata_dir()).unwrap();
        let report = layout.validate().unwrap();
        assert_eq!(
            report.violations,
            vec![HierarchyViolation::MissingGlobalMetadata {
                root: layout.root().to_path_buf()
            }]
        );
    }

    /// §7.4.5 Recommendation Name4 — empty folders warn (emitted here since
    /// it requires walking the tree).
    #[test]
    fn rec_core_name_empty_folders_warned() {
        let tmp = tempdir().unwrap();
        let layout = DatastoreLayout::create(tmp.path()).unwrap();
        // Give global_metadata content so the only empty folder is ours.
        fs::write(layout.global_metadata_dir().join("placeholder.json"), b"{}").unwrap();
        let empty = layout.resolve("Empty").unwrap();
        fs::create_dir(&empty).unwrap();

        let report = layout.validate().unwrap();
        assert_eq!(
            report.warnings,
            vec![HierarchyWarning::EmptyFolder(empty.clone())]
        );
        // The message names the clause, not just the section: the finding's
        // stable code is `/req/core/name-empty-folders-A`, and a reader of
        // the human-readable text should be able to look the same clause up.
        let rendered = HierarchyWarning::EmptyFolder(empty).to_string();
        assert!(
            rendered.contains("/req/core/name-empty-folders A"),
            "{rendered}"
        );
    }

    /// Root names obey the naming module.
    #[test]
    fn create_named_validates_root_name() {
        let tmp = tempdir().unwrap();
        assert!(matches!(
            DatastoreLayout::create_named(tmp.path(), "my store"),
            Err(HierarchyError::Naming(
                NamingViolation::ContainsSpace { .. }
            ))
        ));
    }
}
