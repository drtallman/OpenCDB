mod fixture;

use cdb_migrator::reader::{ReaderFindingKind, SourceEntryType};
use cdb_migrator::{Cdb1Entry, Cdb1Error, Cdb1Tree, GlobalKind};
use std::fs;
use std::path::Path;

// Use a physical temporary parent: the host's /var alias is itself a symlink.
fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap()
}

fn put(root: &Path, rel: &str, bytes: &[u8]) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// Reader contract: every file has exactly one classification and its original path.
#[test]
fn mig_reader_classifies_every_file() {
    let tmp = tempdir();
    fixture::build_1x_tree(tmp.path());
    let tree = Cdb1Tree::open(tmp.path()).unwrap();
    assert_eq!(tree.root(), tmp.path());
    assert_eq!(
        tree.version().unwrap().specification.map(|v| v.as_str()),
        Some("1.2")
    );
    assert!(tree.findings().is_empty());
    let inv = tree.inventory().unwrap();
    let expected = [
        "GTModel/500_GTModelGeometry/A_Culture/D500_S001_T001_AL015_000_bridge.flt",
        "Metadata/Version.xml",
        "Tiles/N32/W118/001_Elevation/L01/U1/N32W118_D001_S001_T001_L01_U1_R0.tif",
        "Tiles/N32/W118/201_RoadNetwork/LC/U0/N32W118_D201_S002_T003_LC05_U0_R0.shp",
        "stray notes.txt",
    ];
    assert_eq!(inv.entries.len(), 5);
    for (entry, rel) in inv.entries.iter().zip(expected) {
        assert_eq!(entry.rel_path(), Some(rel));
        assert_eq!(entry.raw_path(), tmp.path().join(rel));
        assert_eq!(entry.entry_type(), SourceEntryType::RegularFile);
    }
    let tiles: Vec<_> = inv
        .entries
        .iter()
        .filter_map(|e| match e {
            Cdb1Entry::Tile(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(tiles.len(), 2);
    assert!(tiles
        .iter()
        .any(|t| t.file.lod.value() == -5 && t.findings.is_empty()));
    assert!(matches!(
        inv.entries[0],
        Cdb1Entry::Global {
            kind: GlobalKind::GtModel,
            ..
        }
    ));
    assert!(matches!(inv.entries[1], Cdb1Entry::Metadata { .. }));
    assert!(matches!(inv.entries[4], Cdb1Entry::Unrecognized { .. }));
    assert!(!inv.has_unsafe_entries());
}

/// §8.6.2/§8.6.3: directory disagreements and out-of-range row/column survive classification.
#[test]
fn mig_reader_flags_all_file_dir_disagreements() {
    let tmp = tempdir();
    put(
        tmp.path(),
        "Tiles/N33/W119/201_RoadNetwork/L02/U0/N32W118_D001_S001_T001_L01_U2_R3.tif",
        b"x",
    );
    let inv = Cdb1Tree::open(tmp.path()).unwrap().inventory().unwrap();
    let Cdb1Entry::Tile(tile) = &inv.entries[0] else {
        panic!("not a tile")
    };
    for fragment in [
        "geocell",
        "dataset",
        "LOD",
        "UREF",
        "U2 out of range",
        "R3 out of range",
    ] {
        assert!(
            tile.findings.iter().any(|f| f.contains(fragment)),
            "missing {fragment}: {:?}",
            tile.findings
        );
    }
}

/// §8.6: bad directory grammar does not erase a parseable tiled file.
#[test]
fn mig_reader_reports_malformed_tile_directories_and_names() {
    let tmp = tempdir();
    put(
        tmp.path(),
        "Tiles/bad/bad/bad/LC/U+0/N32W118_D001_S001_T001_LC05_U0_R0.tif",
        b"x",
    );
    put(
        tmp.path(),
        "Tiles/N32/W118/001_Elevation/L01/U0/not-a-tile.tif",
        b"y",
    );
    put(tmp.path(), "Tiles/short.tif", b"z");
    let inv = Cdb1Tree::open(tmp.path()).unwrap().inventory().unwrap();
    assert_eq!(inv.entries.len(), 3);
    assert_eq!(
        inv.entries
            .iter()
            .filter(|e| matches!(e, Cdb1Entry::Unrecognized { .. }))
            .count(),
        2
    );
    let tile = inv
        .entries
        .iter()
        .find_map(|e| match e {
            Cdb1Entry::Tile(t) => Some(t),
            _ => None,
        })
        .unwrap();
    assert_eq!(tile.findings.len(), 3);
}

/// Reader case guard: ASCII-folded recognition keeps exact source spellings.
#[test]
fn mig_reader_case_variants_keep_paths_and_all_global_kinds() {
    let tmp = tempdir();
    let rel = "tiles/n32/w118/001_elevation/l01/u1/n32w118_d001_s001_t001_l01_u1_r0.TIF";
    put(tmp.path(), rel, b"x");
    put(tmp.path(), "gtmodel/a", b"x");
    put(tmp.path(), "MMODEL/b", b"x");
    put(tmp.path(), "Navigation/c", b"x");
    let inv = Cdb1Tree::open(tmp.path()).unwrap().inventory().unwrap();
    let tile = inv
        .entries
        .iter()
        .find_map(|e| match e {
            Cdb1Entry::Tile(t) => Some(t),
            _ => None,
        })
        .unwrap();
    assert_eq!(tile.rel_path, rel);
    assert!(tile.findings.is_empty());
    for kind in [
        GlobalKind::GtModel,
        GlobalKind::MModel,
        GlobalKind::Navigation,
    ] {
        assert!(inv
            .entries
            .iter()
            .any(|e| matches!(e, Cdb1Entry::Global {kind: k, ..} if *k == kind)));
    }
}

/// §8.6 LOD boundaries: maximum supported shift and coarse-cell limits remain safe.
#[test]
fn mig_reader_lod_boundary_ranges() {
    let tmp = tempdir();
    for (dir, token, u, r) in [
        ("L23", "L23", 8388607, 8388607),
        ("LC", "LC10", 0, 0),
        ("L00", "L00", 1, 1),
    ] {
        put(tmp.path(), &format!("Tiles/N32/W118/001_Elevation/{dir}/U{u}/N32W118_D001_S001_T001_{token}_U{u}_R{r}.tif"), b"x");
    }
    let inv = Cdb1Tree::open(tmp.path()).unwrap().inventory().unwrap();
    for e in inv.entries {
        let Cdb1Entry::Tile(t) = e else {
            panic!("not tile")
        };
        assert_eq!(
            t.findings.len(),
            if t.file.lod.value() == 0 { 2 } else { 0 }
        );
    }
}

/// No Version.xml is a visible absence, not an inferred version or fatal error.
#[test]
fn mig_reader_tolerates_missing_metadata() {
    let tmp = tempdir();
    fs::create_dir(tmp.path().join("Tiles")).unwrap();
    let tree = Cdb1Tree::open(tmp.path()).unwrap();
    assert!(tree.version().is_none());
    assert!(tree.configuration().is_none());
    assert!(tree.control_metadata_safe());
    assert!(tree
        .findings()
        .iter()
        .any(|f| f.kind == ReaderFindingKind::MissingVersionMetadata));
    assert!(tree.inventory().unwrap().entries.is_empty());
}

/// Discovery folds only ASCII names and preserves both parser declarations and paths.
#[test]
fn mig_reader_discovers_mixed_case_control_files() {
    let tmp = tempdir();
    put(tmp.path(), "mEtAdAtA/vErSiOn.XmL", b"<Version><PreviousIncrementalRootDirectory name='../older'/><Specification version='1.2'/></Version>");
    put(tmp.path(), "mEtAdAtA/cOnFiGuRaTiOn.XmL", b"<Configuration><Version><Folder path='../v'/><Specification version='1.1'/></Version></Configuration>");
    let tree = Cdb1Tree::open(tmp.path()).unwrap();
    assert_eq!(
        tree.version().unwrap().previous_root.as_deref(),
        Some("../older")
    );
    assert_eq!(tree.configuration().unwrap().version_folders, ["../v"]);
    assert_eq!(
        tree.configuration().unwrap().versions[0]
            .declaration
            .specification_raw
            .as_deref(),
        Some("1.1")
    );
    assert_eq!(
        tree.version_path(),
        Some(tmp.path().join("mEtAdAtA/vErSiOn.XmL").as_path())
    );
    assert_eq!(
        tree.configuration_path(),
        Some(tmp.path().join("mEtAdAtA/cOnFiGuRaTiOn.XmL").as_path())
    );
    assert!(tree.control_metadata_safe());
}

/// Malformed control XML remains inventoried and is separately unsafe for planning.
#[test]
fn mig_reader_malformed_controls_are_diagnostics_not_missing() {
    for (name, bytes) in [
        ("Version.xml", b"<Version>".as_slice()),
        ("Configuration.xml", b"<Other/>"),
        ("Version.xml", b"\xff"),
    ] {
        let tmp = tempdir();
        put(tmp.path(), &format!("Metadata/{name}"), bytes);
        let tree = Cdb1Tree::open(tmp.path()).unwrap();
        assert!(!tree.control_metadata_safe());
        assert!(tree
            .findings()
            .iter()
            .any(|f| f.kind == ReaderFindingKind::MalformedControlMetadata
                && f.raw_path == tmp.path().join("Metadata").join(name)));
        assert_eq!(tree.inventory().unwrap().entries.len(), 1);
    }
}

/// Unknown/missing declarations and unsupported extensions are retained as provenance.
#[test]
fn mig_reader_exposes_declaration_provenance() {
    let tmp = tempdir();
    put(
        tmp.path(),
        "Metadata/Version.xml",
        b"<Version><Specification version='9.9'/><Extension name='custom' version='4'/></Version>",
    );
    put(tmp.path(), "Metadata/Configuration.xml", b"<Configuration><Version><Folder path='../a'/></Version><Version><Folder path='../b'/><Specification version='8.0'/><Extension name='other' version='1'/></Version></Configuration>");
    let tree = Cdb1Tree::open(tmp.path()).unwrap();
    assert_eq!(
        tree.version().unwrap().specification_raw.as_deref(),
        Some("9.9")
    );
    assert_eq!(
        tree.version().unwrap().extension.as_ref().unwrap().name,
        "custom"
    );
    for (kind, count) in [
        (ReaderFindingKind::UnknownSpecification, 2),
        (ReaderFindingKind::ExtensionDeclaration, 2),
        (ReaderFindingKind::MissingSpecification, 1),
    ] {
        assert_eq!(
            tree.findings().iter().filter(|f| f.kind == kind).count(),
            count
        );
    }
    assert!(tree.control_metadata_safe()); // parse safety is distinct from migration policy
    put(tmp.path(), "Metadata/Version.xml", b"<Version/>");
    let tree = Cdb1Tree::open(tmp.path()).unwrap();
    assert!(tree
        .findings()
        .iter()
        .any(|f| f.kind == ReaderFindingKind::MissingSpecification
            && f.raw_path.ends_with("Version.xml")));
}

/// Wrong control entry types cannot be silently treated as absent files.
#[test]
fn mig_reader_control_directories_are_unsafe() {
    for rel in [
        "Metadata",
        "Metadata/Version.xml",
        "Metadata/Configuration.xml",
    ] {
        let tmp = tempdir();
        if rel == "Metadata" {
            put(tmp.path(), rel, b"not a directory");
        } else {
            fs::create_dir_all(tmp.path().join(rel)).unwrap();
        }
        let tree = Cdb1Tree::open(tmp.path()).unwrap();
        assert!(!tree.control_metadata_safe());
        assert!(tree
            .findings()
            .iter()
            .any(|f| f.kind == ReaderFindingKind::UnsafeControlMetadata));
    }
}

/// I/O failures in root lookup and a later inventory walk retain the failed path.
#[test]
fn mig_reader_propagates_io_and_rejects_non_directory_root() {
    let tmp = tempdir();
    let missing = tmp.path().join("missing");
    assert!(
        matches!(Cdb1Tree::open(&missing), Err(Cdb1Error::Io(p, e)) if p == missing && e.kind() == std::io::ErrorKind::NotFound)
    );
    put(tmp.path(), "file", b"x");
    assert!(matches!(
        Cdb1Tree::open(&tmp.path().join("file")),
        Err(Cdb1Error::NotADirectory(_))
    ));
    let root = tmp.path().join("tree");
    fs::create_dir(&root).unwrap();
    let tree = Cdb1Tree::open(&root).unwrap();
    fs::remove_dir(&root).unwrap();
    assert!(matches!(tree.inventory(), Err(Cdb1Error::Io(p, _)) if p == root));
}

/// Name-order traversal is independent of insertion order and repeats identically.
#[test]
fn mig_reader_walk_is_deterministic_depth_first() {
    let tmp = tempdir();
    for path in ["z", "a/z", "a/b", "a.txt", "b/x"] {
        put(tmp.path(), path, b"x");
    }
    let tree = Cdb1Tree::open(tmp.path()).unwrap();
    for _ in 0..2 {
        let inv = tree.inventory().unwrap();
        assert_eq!(
            inv.entries
                .iter()
                .map(|e| e.rel_path().unwrap())
                .collect::<Vec<_>>(),
            ["a/b", "a/z", "a.txt", "b/x", "z"]
        );
    }
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::os::unix::fs::{symlink, PermissionsExt};

    /// Links under every classification arm stay unsafe, including dangling links.
    #[test]
    fn mig_reader_never_classifies_links_as_regular_payloads() {
        let tmp = tempdir();
        let outside = tempdir();
        put(outside.path(), "hidden", b"x");
        for rel in [
            "GTModel/a",
            "MModel/b",
            "Navigation/c",
            "Metadata/d",
            "Tiles/N32/W118/001_Elevation/L01/U1/N32W118_D001_S001_T001_L01_U1_R0.tif",
            "dangling",
        ] {
            let link = tmp.path().join(rel);
            fs::create_dir_all(link.parent().unwrap()).unwrap();
            symlink(
                if rel == "dangling" {
                    outside.path().join("absent")
                } else {
                    outside.path().to_path_buf()
                },
                link,
            )
            .unwrap();
        }
        let inv = Cdb1Tree::open(tmp.path()).unwrap().inventory().unwrap();
        assert_eq!(inv.entries.len(), 6);
        assert!(inv.has_unsafe_entries());
        for entry in inv.entries {
            assert_eq!(entry.entry_type(), SourceEntryType::Symlink);
            assert!(matches!(entry, Cdb1Entry::Unsafe { .. }));
        }
    }

    /// Root links cannot be hidden by a trailing slash/dot; inventory rechecks root.
    #[test]
    fn mig_reader_rejects_link_roots_and_root_replacement() {
        let tmp = tempdir();
        let target = tmp.path().join("target");
        fs::create_dir(&target).unwrap();
        let link = tmp.path().join("link");
        symlink(&target, &link).unwrap();
        for path in [
            link.clone(),
            link.join("."),
            Path::new(&format!("{}/", link.display())).to_path_buf(),
        ] {
            assert!(matches!(Cdb1Tree::open(&path), Err(Cdb1Error::Refused(_))));
        }
        let tree = Cdb1Tree::open(&target).unwrap();
        fs::remove_dir(&target).unwrap();
        symlink(tmp.path(), &target).unwrap();
        assert!(matches!(tree.inventory(), Err(Cdb1Error::Refused(_))));
    }

    /// Root ancestry must be checked before traversing any source-path link.
    #[test]
    fn mig_reader_refuses_symlinks_in_root_ancestors() {
        let tmp = tempdir();
        fs::create_dir_all(tmp.path().join("physical/child")).unwrap();
        symlink(tmp.path().join("physical"), tmp.path().join("alias")).unwrap();
        for root in [
            tmp.path().join("alias/child"),
            tmp.path().join("alias/../physical/child"),
        ] {
            assert!(
                matches!(Cdb1Tree::open(&root), Err(Cdb1Error::Refused(_))),
                "{root:?}"
            );
        }
    }

    /// Control links are diagnosed before a read, even with case-folded discovery.
    #[test]
    fn mig_reader_control_links_do_not_hide_chains() {
        for rel in [
            "mEtAdAtA",
            "mEtAdAtA/vErSiOn.XmL",
            "mEtAdAtA/cOnFiGuRaTiOn.XmL",
        ] {
            let tmp = tempdir();
            let link = tmp.path().join(rel);
            fs::create_dir_all(link.parent().unwrap()).unwrap();
            symlink(tmp.path().join("missing"), &link).unwrap();
            let tree = Cdb1Tree::open(tmp.path()).unwrap();
            assert!(tree.version().is_none());
            assert!(tree.configuration().is_none());
            assert!(!tree.control_metadata_safe());
            assert!(tree
                .findings()
                .iter()
                .any(|f| f.kind == ReaderFindingKind::UnsafeControlMetadata && f.raw_path == link));
            assert!(tree.inventory().unwrap().has_unsafe_entries());
        }
    }

    /// Distinct case-folded controls must not be chosen by traversal order.
    #[test]
    fn mig_reader_ambiguous_controls_are_explicit() {
        for (a, b) in [
            ("Metadata/Version.xml", "Metadata/version.XML"),
            ("Metadata/Configuration.xml", "Metadata/configuration.XML"),
            ("Metadata/a", "metadata/b"),
        ] {
            let tmp = tempdir();
            put(tmp.path(), a, b"<Version/>");
            put(tmp.path(), b, b"<Version/>");
            // Some supported hosts use case-insensitive filesystems. Probe names,
            // rather than silently passing a duplicate-case assertion there.
            let dir = if a.ends_with("/a") {
                tmp.path().to_path_buf()
            } else {
                tmp.path().join("Metadata")
            };
            if fs::read_dir(dir).unwrap().count() != 2 {
                continue;
            }
            let tree = Cdb1Tree::open(tmp.path()).unwrap();
            assert!(!tree.control_metadata_safe());
            assert!(tree
                .findings()
                .iter()
                .any(|f| f.kind == ReaderFindingKind::AmbiguousControlMetadata));
            assert!(tree.version().is_none());
            assert!(tree.configuration().is_none());
            assert_eq!(tree.inventory().unwrap().entries.len(), 2);
        }
    }

    /// Invalid UTF-8 names preserve bytes and type without a lossy operational path.
    #[test]
    fn mig_reader_non_utf8_paths_keep_exact_identity() {
        let tmp = tempdir();
        let bad = std::ffi::OsString::from_vec(vec![b'x', 255]);
        let path = tmp.path().join(&bad);
        assert!(matches!(Cdb1Tree::open(&path), Err(Cdb1Error::Refused(_))));
        if let Err(error) = fs::write(&path, b"x") {
            // macOS filesystems reject non-UTF-8 names before the reader runs.
            #[cfg(target_os = "macos")]
            if error.raw_os_error() == Some(92) {
                return;
            }
            panic!("non-UTF-8 fixture: {error}");
        }
        let dir = tmp
            .path()
            .join(std::ffi::OsString::from_vec(vec![b'd', 254]));
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("child"), b"x").unwrap();
        let inv = Cdb1Tree::open(tmp.path()).unwrap().inventory().unwrap();
        assert_eq!(inv.entries.len(), 2);
        assert!(inv.has_unsafe_entries());
        assert!(inv
            .entries
            .iter()
            .any(|e| e.raw_path().as_os_str().as_bytes() == path.as_os_str().as_bytes()));
        for entry in inv.entries {
            assert_eq!(entry.rel_path(), None);
            assert_eq!(entry.entry_type(), SourceEntryType::RegularFile);
            assert!(matches!(entry, Cdb1Entry::Unsafe { .. }));
        }
        assert!(matches!(Cdb1Tree::open(&dir), Err(Cdb1Error::Refused(_))));
    }

    /// Sockets are recorded without opening/blocking on non-regular payloads.
    #[test]
    fn mig_reader_special_files_are_unsafe() {
        let tmp = tempdir();
        let socket = tmp.path().join("socket");
        let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let inv = Cdb1Tree::open(tmp.path()).unwrap().inventory().unwrap();
        assert_eq!(inv.entries[0].raw_path(), socket);
        assert_eq!(inv.entries[0].entry_type(), SourceEntryType::Other);
        assert!(inv.has_unsafe_entries());
    }

    /// A genuine permission error is never reinterpreted as missing metadata.
    #[test]
    fn mig_reader_unreadable_controls_propagate_io() {
        let tmp = tempdir();
        let path = tmp.path().join("Metadata/Version.xml");
        put(tmp.path(), "Metadata/Version.xml", b"<Version/>");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o0)).unwrap();
        let denied = fs::read(&path).is_err();
        let result = Cdb1Tree::open(tmp.path());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        if denied {
            assert!(
                matches!(result, Err(Cdb1Error::Io(p, e)) if p == path && e.kind() == std::io::ErrorKind::PermissionDenied)
            );
        }
    }
}

/// Checking ancestry preserves ordinary relative roots and parent components.
#[test]
fn mig_reader_accepts_relative_roots() {
    let tmp = tempfile::tempdir_in(".").unwrap();
    let tree = Cdb1Tree::open(tmp.path()).unwrap();
    assert!(tree.inventory().unwrap().entries.is_empty());
    fs::create_dir(tmp.path().join("child")).unwrap();
    let parent = tmp.path().join("child/..");
    assert!(Cdb1Tree::open(&parent)
        .unwrap()
        .inventory()
        .unwrap()
        .entries
        .is_empty());
}
