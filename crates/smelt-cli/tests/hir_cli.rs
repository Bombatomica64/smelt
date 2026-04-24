use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

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

fn temp_project() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    std::env::temp_dir().join(format!("smelt-cli-test-{}-{nonce}", std::process::id()))
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

#[test]
fn dump_mir_prints_optimized_mir_for_single_file() {
    let stdout = smelt(&["dump-mir", "examples/typescript/hir/05_alias.ts"]);

    assert!(stdout.contains("fn main (FuncId(0)) -> None"));
    assert!(stdout.contains("%0 user source_value: Float"));
    assert!(stdout.contains("%1 user copied_value: Float"));
    assert!(stdout.contains("%2 = call @console_log(copy %0) -> bb1"));
    assert!(stdout.contains("return none"));
}

#[test]
fn build_emits_compilable_rust_crate() {
    let project = temp_project();
    fs::create_dir_all(project.join("src")).expect("create temp project");
    fs::write(
        project.join("Smelt.toml"),
        r#"[project]
name = "generated-app"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "generated_app"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )
    .expect("write manifest");
    fs::write(
        project.join("src/main.ts"),
        "const message = \"hello smelt\";\nconsole.log(message);\n",
    )
    .expect("write source");

    let manifest = project.join("Smelt.toml");
    smelt(&[
        "--manifest-path",
        manifest.to_str().expect("manifest path utf8"),
        "build",
    ]);

    let generated = fs::read_to_string(project.join("dist/src/main.rs")).expect("generated main");
    assert!(generated.contains("fn main()"));
    assert!(generated.contains("println!"));
}
