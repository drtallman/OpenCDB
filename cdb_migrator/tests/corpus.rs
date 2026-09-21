use cdb_migrator::{Cdb1Entry, Cdb1Tree};
use std::collections::BTreeSet;
use std::path::Path;

fn leaves(root: &Path, paths: &mut BTreeSet<std::path::PathBuf>) {
    for entry in std::fs::read_dir(root).expect("corpus directory readable") {
        let entry = entry.expect("corpus entry readable");
        let kind = entry.file_type().expect("file type readable");
        assert!(!kind.is_symlink(), "corpus must use physical regular files");
        if kind.is_dir() {
            leaves(&entry.path(), paths);
        } else {
            assert!(kind.is_file(), "corpus contains special file");
            assert!(paths.insert(entry.path()));
        }
    }
}

/// Reader corpus contract: every physical leaf is classified exactly once.
/// CDB1_CORPUS_ROOT contains trees marked by Tiles/ or GTModel/;
/// loose raster fixture directories are excluded. Unset skips without I/O.
/// This inventories opaque content, not payload semantics or migration acceptance.
#[test]
fn mig_corpus_inventories_cleanly() {
    let Some(root) = std::env::var_os("CDB1_CORPUS_ROOT") else {
        return;
    };
    let mut trees = 0;
    let mut total = 0;
    let mut total_tiles = 0;
    for entry in std::fs::read_dir(root).expect("corpus root readable") {
        let entry = entry.expect("corpus entry readable");
        if !entry.file_type().expect("corpus type").is_dir() {
            continue;
        }
        let path = entry.path();
        let is_tree = std::fs::read_dir(&path)
            .expect("candidate root readable")
            .any(|child| {
                let child = child.expect("candidate marker readable");
                let marker = child.file_name();
                marker.to_str().is_some_and(|name| {
                    name.eq_ignore_ascii_case("Tiles") || name.eq_ignore_ascii_case("GTModel")
                }) && child.file_type().expect("marker type readable").is_dir()
            });
        if !is_tree {
            continue;
        }
        let tree = Cdb1Tree::open(&path).expect("opens physical corpus tree");
        let inv = tree.inventory().expect("inventories without I/O failure");
        assert!(!inv.has_unsafe_entries(), "{}", path.display());
        let observed: BTreeSet<_> = inv
            .entries
            .iter()
            .map(|e| e.raw_path().to_path_buf())
            .collect();
        assert_eq!(
            observed.len(),
            inv.entries.len(),
            "duplicate inventory entry"
        );
        let mut expected = BTreeSet::new();
        leaves(&path, &mut expected);
        assert_eq!(observed, expected, "every leaf accounted once");
        let tiles = inv
            .entries
            .iter()
            .filter(|e| matches!(e, Cdb1Entry::Tile(_)))
            .count();
        eprintln!(
            "{}: {} entries, {} tiles",
            path.display(),
            inv.entries.len(),
            tiles
        );
        trees += 1;
        total += inv.entries.len();
        total_tiles += tiles;
    }
    assert!(trees > 0, "corpus root had no tree directories");
    eprintln!("TOTAL: {trees} trees, {total} entries, {total_tiles} tiles");
}
