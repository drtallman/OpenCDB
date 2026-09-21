use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn invoke(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cdb_migrator"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("CLI subprocess executes")
}

/// Omitting the destination before --dry-run must never perform a real migration.
/// The physical subprocess cwd confines any regression's writes to this fixture.
#[test]
fn mig_cli_process_missing_output_flag_creates_nothing() {
    let hold = tempfile::tempdir().unwrap();
    let root = hold.path().canonicalize().unwrap();
    fs::create_dir(root.join("source")).unwrap();
    let result = invoke(&root, &["migrate", "source", "--dry-run"]);
    assert!(
        !root.join("--dry-run").exists(),
        "missing output unexpectedly created artifacts; exit {:?}",
        result.status.code()
    );
    assert_eq!(result.status.code(), Some(2));
    assert!(result.stdout.is_empty());
    assert!(!result.stderr.is_empty());
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
}

/// Explicit relative paths disambiguate literal names beginning with option syntax.
#[test]
fn mig_cli_process_explicit_option_named_paths_remain_supported() {
    let hold = tempfile::tempdir().unwrap();
    let root = hold.path().canonicalize().unwrap();
    fs::create_dir(root.join("--source")).unwrap();
    let inventory = invoke(&root, &["inventory", "./--source"]);
    assert_eq!(inventory.status.code(), Some(0));
    let result = invoke(&root, &["migrate", "./--source", "./--dry-run"]);
    assert_eq!(result.status.code(), Some(0), "{:?}", result.stderr);
    assert!(root.join("--dry-run/cdb").is_dir());
    assert!(root
        .join("--dry-run/cdb_migrator-migration-report.json")
        .is_file());
}
