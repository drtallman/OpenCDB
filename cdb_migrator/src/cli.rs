//! Command-line argument parsing and execution with injected output streams.
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::Write;
use std::path::Path;

use crate::metadata_input::MetadataManifest;
use crate::migrate::{migrate, MigrationReport};
use crate::plan::{parse_rename_map, plan, ExtrasPolicy, PlanOptions};
use crate::{Cdb1Entry, Cdb1Error, Cdb1Tree, Inventory};

const USAGE: &str = "cdb_migrator inventory <root>\ncdb_migrator migrate <root> <out-parent> [--dry-run]\n  [--carry-extras | --skip-extras] [--metadata-file JSON] [--rename-map JSON]\n  [--id S] [--title S] [--description S] [--contact S] [--timestamp RFC3339]\nUse physical paths (on macOS, /private/tmp rather than /tmp).\nMigration stdout is the complete JSON report; payload semantics and embedded\nreferences remain unchecked. Dry-run prints planned moves and creates nothing.\nExits: 0 success, 1 completed nonconformant, 2 usage/refused, 3 operational.\n";

/// Run arguments excluding the executable name, writing diagnostics to `err`.
///
/// Returns 0 for successful inventory/dry-run or conformant migration, 1 for a
/// completed nonconformant report, 2 for usage/refusal, and 3 for operational
/// failures (including output errors). Migration stdout is the full JSON report.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match execute(args, out) {
        Ok(code) => code,
        Err(error) => diagnose(error, err),
    }
}

/// Decode operating-system arguments without panic or lossy replacement.
///
/// Any non-UTF-8 argument is a usage refusal before reading the source.
pub fn run_os(
    args: impl IntoIterator<Item = OsString>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let args: Result<Vec<_>, _> = args.into_iter().map(|arg| arg.into_string()).collect();
    match args {
        Ok(args) => run(&args, out, err),
        Err(_) => diagnose(
            Cdb1Error::Refused("arguments must be valid UTF-8".into()),
            err,
        ),
    }
}

fn diagnose(error: Cdb1Error, err: &mut dyn Write) -> i32 {
    let code = if matches!(error, Cdb1Error::Refused(_)) {
        2
    } else {
        3
    };
    if writeln!(err, "cdb_migrator: {}", display(&error.to_string())).is_err() {
        3
    } else {
        code
    }
}
fn output_error(error: impl std::fmt::Display) -> Cdb1Error {
    Cdb1Error::Operational(format!("command output: {error}"))
}
fn usage(message: &str) -> Cdb1Error {
    Cdb1Error::Refused(format!("{message}; use cdb_migrator --help"))
}
fn read_input(path: &str) -> Result<String, Cdb1Error> {
    std::fs::read_to_string(path).map_err(|e| Cdb1Error::Io(path.into(), e))
}
fn execute(args: &[String], out: &mut dyn Write) -> Result<i32, Cdb1Error> {
    if args == ["--help"]
        || (args.len() == 2
            && matches!(args[0].as_str(), "inventory" | "migrate")
            && args[1] == "--help")
    {
        out.write_all(USAGE.as_bytes()).map_err(output_error)?;
        return Ok(0);
    }
    match args.first().map(String::as_str) {
        Some("inventory") if args.len() == 2 => {
            let tree = Cdb1Tree::open(Path::new(&args[1]))?;
            render_inventory(&tree.inventory()?, out)?;
            Ok(0)
        }
        Some("migrate") if args.len() >= 3 => {
            let mut options = PlanOptions::default();
            let mut dry_run = false;
            let mut timestamp = None;
            let mut metadata_file = None;
            let mut rename_file = None;
            let mut seen = BTreeSet::new();
            let mut index = 3;
            while index < args.len() {
                let flag = args[index].as_str();
                if !seen.insert(flag) {
                    return Err(usage(&format!("repeated option {flag}")));
                }
                match flag {
                    "--dry-run" => dry_run = true,
                    "--carry-extras" | "--skip-extras" => {
                        if options.extras != ExtrasPolicy::Unspecified {
                            return Err(usage(
                                "--carry-extras and --skip-extras are mutually exclusive",
                            ));
                        }
                        options.extras = if flag == "--carry-extras" {
                            ExtrasPolicy::Carry
                        } else {
                            ExtrasPolicy::Skip
                        };
                    }
                    "--id" | "--title" | "--description" | "--contact" | "--timestamp"
                    | "--metadata-file" | "--rename-map" => {
                        index += 1;
                        let value = args
                            .get(index)
                            .filter(|v| !v.starts_with("--"))
                            .ok_or_else(|| usage(&format!("{flag} requires a value")))?;
                        match flag {
                            "--id" => options.id = Some(value.clone()),
                            "--title" => options.title = Some(value.clone()),
                            "--description" => options.description = Some(value.clone()),
                            "--contact" => options.contact = Some(value.clone()),
                            "--timestamp" => timestamp = Some(value.as_str()),
                            "--metadata-file" => metadata_file = Some(value.as_str()),
                            "--rename-map" => rename_file = Some(value.as_str()),
                            _ => return Err(usage("unsupported value option")),
                        }
                    }
                    _ => return Err(usage(&format!("unknown option {flag}"))),
                }
                index += 1;
            }
            if let Some(time) = timestamp {
                opencdb::metadata::temporal::parse_datetime(time)
                    .map_err(|e| Cdb1Error::Refused(format!("timestamp: {e}")))?;
            }
            if let Some(path) = metadata_file {
                options.metadata = Some(MetadataManifest::from_json_str(&read_input(path)?)?);
            }
            if let Some(path) = rename_file {
                options.rename_map = parse_rename_map(&read_input(path)?)?;
            }
            let tree = Cdb1Tree::open(Path::new(&args[1]))?;
            let inventory = tree.inventory()?;
            let plan = plan(&tree, &inventory, &options)?;
            if dry_run {
                for entry in plan.moves() {
                    writeln!(
                        out,
                        "MOVE\t{}\t{}",
                        display(&entry.source),
                        display(&entry.target)
                    )
                    .map_err(output_error)?;
                }
                for path in plan.skipped() {
                    writeln!(out, "SKIP\t{}", display(path)).map_err(output_error)?;
                }
                writeln!(
                    out,
                    "SUMMARY\tmoves={}\tskipped={}\trecords={}\tdry_run=true",
                    plan.moves().len(),
                    plan.skipped().len(),
                    plan.resources().len()
                )
                .map_err(output_error)?;
                return Ok(0);
            }
            let report = migrate(&tree, &plan, Path::new(&args[2]), timestamp)?;
            render_completed(&report, out)
        }
        _ => Err(usage("invalid command or argument count")),
    }
}

fn render_completed(report: &MigrationReport, out: &mut dyn Write) -> Result<i32, Cdb1Error> {
    serde_json::to_writer_pretty(&mut *out, report).map_err(output_error)?;
    writeln!(out).map_err(output_error)?;
    Ok(if report.conformant { 0 } else { 1 })
}

// Keep one physical line per entry; escaping backslashes makes the display
// unambiguous. Actual reader and report identities remain untouched.
fn display(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| match ch {
            '\\' => "\\\\".chars().collect::<Vec<_>>(),
            '\n' => "\\n".chars().collect(),
            '\r' => "\\r".chars().collect(),
            '\t' => "\\t".chars().collect(),
            ch if ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}') => {
                ch.escape_unicode().collect()
            }
            ch => vec![ch],
        })
        .collect()
}

fn render_inventory(inventory: &Inventory, out: &mut dyn Write) -> Result<(), Cdb1Error> {
    let mut counts = [0usize; 5];
    for entry in &inventory.entries {
        let (kind, reason, counter) = match entry {
            Cdb1Entry::Tile(_) => ("TILE", None, 0),
            Cdb1Entry::Global { .. } => ("GLOBAL", None, 1),
            Cdb1Entry::Metadata { .. } => ("META", None, 2),
            Cdb1Entry::Unrecognized { why, .. } => ("UNRECOGNIZED", Some(why), 3),
            Cdb1Entry::Unsafe { why, .. } => ("UNSAFE", Some(why), 4),
        };
        counts[counter] += 1;
        let path = match entry.rel_path() {
            Some(path) => display(path),
            None => {
                // Explicit hex of the original platform encoding, never U+FFFD.
                let bytes = entry.raw_path().as_os_str().as_encoded_bytes();
                format!(
                    "raw-path-hex:{}",
                    bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
                )
            }
        };
        write!(out, "{kind}\t{path}").map_err(output_error)?;
        if let Some(reason) = reason {
            write!(out, "\t{}", display(reason)).map_err(output_error)?;
        }
        writeln!(out).map_err(output_error)?;
    }
    writeln!(
        out,
        "SUMMARY\tentries={}\ttiles={}\tglobals={}\tmetadata={}\tunrecognized={}\tunsafe={}",
        inventory.entries.len(),
        counts[0],
        counts[1],
        counts[2],
        counts[3],
        counts[4]
    )
    .map_err(output_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::fs;
    use std::path::{Path, PathBuf};

    fn invoke(args: &[&str]) -> (i32, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(
            &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            &mut out,
            &mut err,
        );
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }
    fn temp() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let physical = dir.path().canonicalize().unwrap();
        (dir, physical)
    }
    fn put(root: &Path, rel: &str, bytes: &[u8]) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    /// CLI contract: malformed requests are usage errors before filesystem work.
    #[test]
    fn mig_cli_usage_errors_exit_2() {
        for args in [
            vec![],
            vec!["frobnicate"],
            vec!["inventory"],
            vec!["inventory", "x", "y"],
            vec!["migrate", "x"],
            vec!["migrate", "x", "y", "--bogus"],
            vec!["migrate", "x", "y", "--id"],
            vec!["migrate", "x", "y", "--id", "a", "--id", "b"],
            vec!["migrate", "x", "y", "--carry-extras", "--skip-extras"],
            vec!["migrate", "x", "y", "--timestamp", "bad", "--dry-run"],
        ] {
            let (code, out, err) = invoke(&args);
            assert_eq!(code, 2, "{args:?}: {err}");
            assert!(out.is_empty());
            assert!(!err.is_empty());
        }
    }
    /// CLI contract: help is available without a source tree.
    #[test]
    fn mig_cli_help() {
        for args in [
            vec!["--help"],
            vec!["inventory", "--help"],
            vec!["migrate", "--help"],
        ] {
            let (code, out, err) = invoke(&args);
            assert_eq!(code, 0);
            assert!(out.contains("--metadata-file"));
            assert!(err.is_empty());
        }
    }
    /// CLI contract: missing input is operational; malformed controls remain inventory findings.
    #[test]
    fn mig_cli_operational_failures_exit_3() {
        assert_eq!(invoke(&["inventory", "/nonexistent/xyzzy"]).0, 3);
        let (_hold, root) = temp();
        put(&root, "Metadata/Version.xml", b"<Version>");
        assert_eq!(invoke(&["inventory", root.to_str().unwrap()]).0, 0);
        assert_eq!(
            invoke(&[
                "migrate",
                root.to_str().unwrap(),
                root.join("unused").to_str().unwrap()
            ])
            .0,
            2
        );
        let mut err = Vec::new();
        assert_eq!(
            diagnose(
                Cdb1Error::Xml("control.xml".into(), "invalid".into()),
                &mut err
            ),
            3
        );
        assert_eq!(
            diagnose(Cdb1Error::Operational("failed".into()), &mut err),
            3
        );
    }
    /// Inventory display must classify once and prevent control characters from injecting rows.
    #[test]
    fn mig_cli_inventory_escapes_raw_names() {
        let (_hold, root) = temp();
        put(&root, "GTModel/tree.flt", b"opaque");
        put(&root, "Metadata/Version.xml", b"<Version/>");
        put(&root, "bad\nTILE\tfake\r\u{1b}\\name.txt", b"opaque");
        let (code, out, err) = invoke(&["inventory", root.to_str().unwrap()]);
        assert_eq!(code, 0, "{err}");
        assert_eq!(out.lines().count(), 4, "{out}");
        assert!(out.contains("GLOBAL\tGTModel/tree.flt\n"));
        assert!(out.contains("META\tMetadata/Version.xml\n"));
        assert!(out.contains("bad\\nTILE\\tfake\\r\\u{1b}\\\\name.txt"));
        assert!(out.contains("SUMMARY\tentries=3"));
    }
    /// Dry runs must honor carry/skip and renames, but never create the output parent.
    #[test]
    fn mig_cli_dry_run_and_extras_policy() {
        let (_hold, root) = temp();
        let source = root.join("source");
        put(&source, "odd name.txt", b"opaque");
        let output = root.join("new_output");
        let renames = root.join("renames.json");
        fs::write(&renames, r#"{"odd name.txt":"extras/odd_name.txt"}"#).unwrap();
        let base = [
            "migrate",
            source.to_str().unwrap(),
            output.to_str().unwrap(),
        ];
        assert_eq!(invoke(&base).0, 2);
        let mut args = base.to_vec();
        args.extend(["--dry-run", "--skip-extras"]);
        let (code, out, err) = invoke(&args);
        assert_eq!(code, 0, "{err}");
        assert!(out.contains("SKIP\todd name.txt"));
        assert!(!output.exists());
        let mut args = base.to_vec();
        args.extend([
            "--dry-run",
            "--carry-extras",
            "--rename-map",
            renames.to_str().unwrap(),
        ]);
        let (code, out, err) = invoke(&args);
        assert_eq!(code, 0, "{err}");
        assert!(out.contains("MOVE\todd name.txt\textras/odd_name.txt"));
        assert!(!output.exists());
    }
    /// JSON inputs use the strict library parsers and missing files stay operational.
    #[test]
    fn mig_cli_strict_json_inputs() {
        let (_hold, root) = temp();
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        let output = root.join("output");
        let input = root.join("input.json");
        for (flag, text) in [
            ("--metadata-file", r#"{"resources":[],"typo":true}"#),
            ("--metadata-file", r#"{"resources":[],"resources":[]}"#),
            ("--rename-map", r#"{"a":"extras/a","a":"extras/b"}"#),
            ("--rename-map", r#"{"a":true}"#),
        ] {
            fs::write(&input, text).unwrap();
            assert_eq!(
                invoke(&[
                    "migrate",
                    source.to_str().unwrap(),
                    output.to_str().unwrap(),
                    flag,
                    input.to_str().unwrap()
                ])
                .0,
                2
            );
            assert!(!output.exists());
        }
        fs::remove_file(&input).unwrap();
        assert_eq!(
            invoke(&[
                "migrate",
                source.to_str().unwrap(),
                output.to_str().unwrap(),
                "--metadata-file",
                input.to_str().unwrap()
            ])
            .0,
            3
        );
    }
    /// CLI options must reach the real facade, record writer, descriptor and report.
    #[test]
    fn mig_cli_materializes_payload_and_identity() {
        let (_hold, root) = temp();
        let source = root.join("source");
        put(&source, "GTModel/Tree.flt", b"opaque model bytes");
        let output = root.join("output");
        let input = root.join("manifest.json");
        let manifest = json!({"resources":[{"source_path":"GTModel/Tree.flt","record":{"ID":"tree","type":"dataset","title":"Tree","description":"Test declaration","keywords":["test"]},"coverage":false,"measurement_values":false,"generated_faces":false}]});
        fs::write(&input, manifest.to_string()).unwrap();
        let (code, out, err) = invoke(&[
            "migrate",
            source.to_str().unwrap(),
            output.to_str().unwrap(),
            "--metadata-file",
            input.to_str().unwrap(),
            "--id",
            "example",
            "--title",
            "Example",
            "--description",
            "Explicit test",
            "--contact",
            "ops@example.com",
            "--timestamp",
            "2026-09-20T12:00:00Z",
        ]);
        assert_eq!(code, 0, "{err}");
        let report: Value = serde_json::from_str(&out).unwrap();
        let disk: Value =
            serde_json::from_slice(&fs::read(output.join(crate::migrate::REPORT_FILE)).unwrap())
                .unwrap();
        assert_eq!(report, disk);
        assert_eq!(
            report["seed"],
            json!({"id":"example","title":"Example","description":"Explicit test","contact":"ops@example.com"})
        );
        assert_eq!(report["global_metadata"]["created"], "2026-09-20T12:00:00Z");
        assert_eq!(report["generated_records"].as_array().unwrap().len(), 1);
        assert_eq!(
            report["operator_metadata"]["resources"][0]["source_path"],
            "GTModel/Tree.flt"
        );
        assert_eq!(
            fs::read(output.join("cdb/gtmodel/tree.flt")).unwrap(),
            b"opaque model bytes"
        );
        assert!(output.join(crate::migrate::DESCRIPTOR_FILE).is_file());
        assert_eq!(
            invoke(&[
                "migrate",
                source.to_str().unwrap(),
                output.to_str().unwrap(),
                "--metadata-file",
                input.to_str().unwrap()
            ])
            .0,
            2
        );
    }
    /// Process arguments containing invalid UTF-8 must refuse without panic/replacement.
    #[cfg(unix)]
    #[test]
    fn mig_cli_non_utf8_arguments_refuse() {
        use std::os::unix::ffi::OsStringExt;
        let mut out = Vec::new();
        let mut err = Vec::new();
        assert_eq!(
            run_os(
                [
                    OsString::from("inventory"),
                    OsString::from_vec(vec![b'/', 255])
                ],
                &mut out,
                &mut err
            ),
            2
        );
        assert!(out.is_empty());
        assert!(!err.is_empty());
    }
    /// Output failures cannot be advertised as successful commands.
    #[test]
    fn mig_cli_output_failure_is_operational() {
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("closed"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut err = Vec::new();
        assert_eq!(run(&["--help".into()], &mut Broken, &mut err), 3);
        assert!(!err.is_empty());
    }
    /// Completed-report boundary: a real failing validator result retains findings and exit1.
    /// This deliberately revalidates damaged output; it is not an accepted failing migration.
    #[test]
    fn mig_cli_completed_nonconformant_report_boundary() {
        let (_hold, root) = temp();
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        let tree = Cdb1Tree::open(&source).unwrap();
        let plan = plan(&tree, &tree.inventory().unwrap(), &PlanOptions::default()).unwrap();
        let output = root.join("output");
        let mut report = migrate(&tree, &plan, &output, None).unwrap();
        put(
            &output.join("cdb"),
            "Bad Name.txt",
            b"deliberate validator violation",
        );
        let store = opencdb::CdbDatastore::open(output.join("cdb")).unwrap();
        report.conformance = store.validate(plan.profile()).unwrap();
        report.conformant = report.conformance.is_conformant();
        assert!(!report.conformant);
        let mut out = Vec::new();
        assert_eq!(render_completed(&report, &mut out).unwrap(), 1);
        let wire: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(wire, serde_json::to_value(&report).unwrap());
        assert!(wire["conformance"]["classes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| !c["violations"].as_array().unwrap().is_empty()));
    }
    /// Unsafe non-UTF-8 display preserves exact platform bytes without replacement.
    #[cfg(unix)]
    #[test]
    fn mig_cli_unsafe_inventory_identity_display() {
        use std::os::unix::ffi::OsStringExt;
        let inventory = Inventory {
            entries: vec![Cdb1Entry::Unsafe {
                raw_path: PathBuf::from(OsString::from_vec(vec![b'/', 255])),
                rel_path: None,
                entry_type: crate::reader::SourceEntryType::RegularFile,
                why: "non UTF-8\nunsafe".into(),
            }],
        };
        let mut out = Vec::new();
        render_inventory(&inventory, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.starts_with("UNSAFE\traw-path-hex:2fff\tnon UTF-8\\nunsafe\n"),
            "{text}"
        );
        assert_eq!(text.lines().count(), 2);
    }
}
