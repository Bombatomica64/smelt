use std::{path::Path, process::Command};

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
}

fn smelt(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_smelt"))
        .current_dir(workspace_root())
        .args(args)
        .output()
        .expect("run smelt");

    assert!(
        output.status.success(),
        "smelt failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout).expect("stdout utf8")
}

#[test]
fn dump_hir_prints_compact_hir_for_single_file() {
    let stdout = smelt(&["dump-hir", "examples/typescript/hir/01_number.ts"]);

    assert!(stdout.contains("module examples/typescript/hir/01_number.ts (ModuleId(0))"));
    assert!(stdout.contains("%0 let count: Float"));
    assert!(stdout.contains("#3: None = call #2(#1)"));
    assert!(stdout.contains("interned types\n  t0 = Float\n  t1 = None\n"));
}

#[test]
fn build_hir_reads_entries_relative_to_manifest() {
    let stdout = smelt(&[
        "--manifest-path",
        "examples/typescript/hir/Smelt.toml",
        "build",
        "--hir",
    ]);

    assert!(stdout.contains("module examples/typescript/hir/01_number.ts (ModuleId(0))"));
    assert!(stdout.contains("s0: let %0: Float = #0"));
    assert!(stdout.contains("s1: #3"));
}
