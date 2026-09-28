//! Locate the actual dependency source in a checkout or standalone package.
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn manifest() -> PathBuf {
    // Restrict metadata to the host we are testing. Otherwise Cargo may try to
    // download manifests for unrelated target platforms during an offline test.
    let rustc = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .args(["--version", "--verbose"])
        .output()
        .expect("rustc must be available to locate the test host");
    assert!(rustc.status.success(), "rustc host query failed");
    let rustc = String::from_utf8(rustc.stdout).expect("rustc version is UTF-8");
    let host = rustc
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .expect("rustc version must name the host");
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(env!("CARGO"))
        .current_dir(directory)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--offline",
            "--locked",
            "--filter-platform",
            host,
        ])
        .output()
        .expect("Cargo metadata must locate the built dependency");
    assert!(
        output.status.success(),
        "Cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("Cargo metadata must be JSON");
    let packages = metadata["packages"].as_array().expect("metadata packages");
    let cli = packages
        .iter()
        .find(|package| {
            package["manifest_path"]
                .as_str()
                .is_some_and(|path| Path::new(path) == directory.join("Cargo.toml"))
        })
        .expect("metadata must include this linter package");
    let node = metadata["resolve"]["nodes"]
        .as_array()
        .expect("resolved nodes")
        .iter()
        .find(|node| node["id"] == cli["id"])
        .expect("linter resolve node");
    let dependency = node["deps"]
        .as_array()
        .expect("linter dependencies")
        .iter()
        .find(|dependency| dependency["name"] == "opencdb")
        .expect("resolved opencdb dependency");
    let library = packages
        .iter()
        .find(|package| package["id"] == dependency["pkg"])
        .expect("resolved library package");
    PathBuf::from(
        library["manifest_path"]
            .as_str()
            .expect("library manifest path"),
    )
}
