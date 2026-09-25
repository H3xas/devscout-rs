//! The packaged default CLI contains no managed runtime, no targeting
//! packs and no engine assemblies -- asserted mechanically from
//! `cargo package --list`'s own file list, rather than inferred from
//! `Cargo.toml`'s `exclude` setting.

use std::path::Path;
use std::process::Command;

#[test]
fn cargo_package_list_excludes_the_engine_and_every_dotnet_artifact() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));

    let out = Command::new(cargo)
        .current_dir(manifest_dir)
        .args(["package", "--list", "--allow-dirty"])
        .output()
        .expect("cargo package --list must run");
    assert!(
        out.status.success(),
        "cargo package --list failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let listing = String::from_utf8(out.stdout).unwrap();
    let files: Vec<&str> = listing.lines().collect();
    assert!(
        !files.is_empty(),
        "the package must contain at least Cargo.toml"
    );

    for file in &files {
        assert!(
            !file.starts_with("tools/"),
            "packaged crate must not contain the engine directory: {file}"
        );
        let lower = file.to_ascii_lowercase();
        for suffix in [".dll", ".exe", ".pdb", ".runtimeconfig.json", ".deps.json"] {
            assert!(
                !lower.ends_with(suffix),
                "packaged crate must not contain a managed-runtime/engine-assembly file: {file}"
            );
        }
    }
}
