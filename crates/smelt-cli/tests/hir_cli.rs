#![expect(
    clippy::too_many_lines,
    reason = "integration tests keep complete CLI scenarios together"
)]

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

struct TempProject {
    path: PathBuf,
}

impl TempProject {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        Self {
            path: std::env::temp_dir()
                .join(format!("smelt-cli-test-{}-{nonce}", std::process::id())),
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn cargo_run_manifest(manifest: &Path) -> String {
    let output = Command::new("cargo")
        .arg("run")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(manifest)
        .output()
        .expect("run generated crate");

    assert!(
        output.status.success(),
        "generated crate failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout).expect("generated stdout utf8")
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
    let project = TempProject::new();
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src")).expect("create temp project");
    fs::write(
        project_path.join("Smelt.toml"),
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
        project_path.join("src/main.ts"),
        "const message = \"hello smelt\";\nconsole.log(message);\n",
    )
    .expect("write source");

    let manifest = project_path.join("Smelt.toml");
    smelt(&[
        "--manifest-path",
        manifest.to_str().expect("manifest path utf8"),
        "build",
    ]);

    let generated =
        fs::read_to_string(project_path.join("dist/src/main.rs")).expect("generated main");
    assert!(generated.contains("fn main()"));
    assert!(generated.contains("println!"));
}

#[test]
fn check_emits_typescript_declaration_stubs_for_linked_modules() {
    let project = TempProject::new();
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src")).expect("create temp project");
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "linked-ts"
version = "0.1.0"

[sources]
entries = ["src/math.ts", "src/main.ts"]

[output]
target = "./dist"
crate-name = "linked_ts"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )
    .expect("write manifest");
    fs::write(
        project_path.join("src/math.ts"),
        "export function add(a: number, b: number): number { return a + b; }\n",
    )
    .expect("write helper source");
    fs::write(
        project_path.join("src/main.ts"),
        "import { add } from './math';\nconst result = add(2, 3);\nconsole.log(result);\n",
    )
    .expect("write main source");

    let manifest = project_path.join("Smelt.toml");
    smelt(&[
        "--manifest-path",
        manifest.to_str().expect("manifest path utf8"),
        "check",
    ]);

    let declaration = fs::read_to_string(project_path.join("src/math.d.ts"))
        .expect("generated TypeScript declaration");
    let entry_declaration = fs::read_to_string(project_path.join("src/main.d.ts"))
        .expect("generated TypeScript entry declaration");
    let python_stub =
        fs::read_to_string(project_path.join("src/math.pyi")).expect("generated Python stub");
    let entry_python_stub =
        fs::read_to_string(project_path.join("src/main.pyi")).expect("generated Python entry stub");
    assert!(declaration.contains("export declare function add(a: number, b: number): number;"));
    assert!(entry_declaration.contains("Generated by smelt"));
    assert!(python_stub.contains("def add(a: float, b: float) -> float: ..."));
    assert!(entry_python_stub.contains("Generated by smelt"));
}

#[test]
fn check_emits_python_stubs_for_linked_modules() {
    let project = TempProject::new();
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src")).expect("create temp project");
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "linked-py"
version = "0.1.0"

[sources]
entries = ["src/math.py", "src/main.py"]

[output]
target = "./dist"
crate-name = "linked_py"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )
    .expect("write manifest");
    fs::write(
        project_path.join("src/math.py"),
        "def add(a: int, b: int) -> int:\n    return a + b\n",
    )
    .expect("write helper source");
    fs::write(
        project_path.join("src/main.py"),
        "from math import add\nresult: int = add(2, 3)\nprint(result)\n",
    )
    .expect("write main source");

    let manifest = project_path.join("Smelt.toml");
    smelt(&[
        "--manifest-path",
        manifest.to_str().expect("manifest path utf8"),
        "check",
    ]);

    let stub =
        fs::read_to_string(project_path.join("src/math.pyi")).expect("generated Python stub");
    let entry_stub =
        fs::read_to_string(project_path.join("src/main.pyi")).expect("generated Python entry stub");
    let typescript_declaration = fs::read_to_string(project_path.join("src/math.d.ts"))
        .expect("generated TypeScript declaration");
    let entry_typescript_declaration = fs::read_to_string(project_path.join("src/main.d.ts"))
        .expect("generated TypeScript entry declaration");
    assert!(stub.contains("def add(a: int, b: int) -> int: ..."));
    assert!(entry_stub.contains("Generated by smelt"));
    assert!(
        typescript_declaration
            .contains("export declare function add(a: number, b: number): number;")
    );
    assert!(entry_typescript_declaration.contains("Generated by smelt"));
}

#[test]
fn build_runs_python_entry_importing_typescript_function() {
    let project = TempProject::new();
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src")).expect("create temp project");
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "cross-run"
version = "0.1.0"

[sources]
entries = ["src/math.ts", "src/main.py"]

[output]
target = "./dist"
crate-name = "cross_run"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )
    .expect("write manifest");
    fs::write(
        project_path.join("src/math.ts"),
        "export function add(a: number, b: number): number {\n  return a + b;\n}\n",
    )
    .expect("write TypeScript source");
    fs::write(
        project_path.join("src/main.py"),
        "from math import add\nresult: float = add(2.0, 3.0)\nprint(result)\n",
    )
    .expect("write Python source");

    let manifest = project_path.join("Smelt.toml");
    smelt(&[
        "--manifest-path",
        manifest.to_str().expect("manifest path utf8"),
        "build",
    ]);

    assert!(project_path.join("src/math.d.ts").exists());
    assert!(project_path.join("src/math.pyi").exists());
    assert!(project_path.join("src/main.d.ts").exists());
    assert!(project_path.join("src/main.pyi").exists());
    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"));
    assert_eq!(actual_stdout, "5\n");
}

#[test]
fn build_orders_manifest_entries_by_import_dependencies() {
    let project = TempProject::new();
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src")).expect("create temp project");
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "cross-run-reversed"
version = "0.1.0"

[sources]
entries = ["src/main.py", "src/math.ts"]

[output]
target = "./dist"
crate-name = "cross_run_reversed"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )
    .expect("write manifest");
    fs::write(
        project_path.join("src/math.ts"),
        "export function add(a: number, b: number): number {\n  return a + b;\n}\n",
    )
    .expect("write TypeScript source");
    fs::write(
        project_path.join("src/main.py"),
        "from math import add\nresult: float = add(2.0, 3.0)\nprint(result)\n",
    )
    .expect("write Python source");

    let manifest = project_path.join("Smelt.toml");
    smelt(&[
        "--manifest-path",
        manifest.to_str().expect("manifest path utf8"),
        "build",
    ]);

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"));
    assert_eq!(actual_stdout, "5\n");
}

#[test]
fn build_resolves_typescript_index_module_imports() {
    let project = TempProject::new();
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src/lib")).expect("create temp project");
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-index-import"
version = "0.1.0"

[sources]
entries = ["src/main.ts", "src/lib/index.ts"]

[output]
target = "./dist"
crate-name = "ts_index_import"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )
    .expect("write manifest");
    fs::write(
        project_path.join("src/lib/index.ts"),
        "export function add(a: number, b: number): number {\n  return a + b;\n}\n",
    )
    .expect("write TypeScript helper source");
    fs::write(
        project_path.join("src/main.ts"),
        "import { add } from './lib';\nconst result = add(4, 6);\nconsole.log(result);\n",
    )
    .expect("write TypeScript entry source");

    let manifest = project_path.join("Smelt.toml");
    smelt(&[
        "--manifest-path",
        manifest.to_str().expect("manifest path utf8"),
        "build",
    ]);

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"));
    assert_eq!(actual_stdout, "10\n");
}

#[test]
fn build_resolves_python_package_init_imports() {
    let project = TempProject::new();
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src/lib")).expect("create temp project");
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "py-package-import"
version = "0.1.0"

[sources]
entries = ["src/main.py", "src/lib/__init__.py"]

[output]
target = "./dist"
crate-name = "py_package_import"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )
    .expect("write manifest");
    fs::write(
        project_path.join("src/lib/__init__.py"),
        "def add(a: int, b: int) -> int:\n    return a + b\n",
    )
    .expect("write Python package source");
    fs::write(
        project_path.join("src/main.py"),
        "from lib import add\nresult: int = add(7, 8)\nprint(result)\n",
    )
    .expect("write Python entry source");

    let manifest = project_path.join("Smelt.toml");
    smelt(&[
        "--manifest-path",
        manifest.to_str().expect("manifest path utf8"),
        "build",
    ]);

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"));
    assert_eq!(actual_stdout, "15\n");
}

#[test]
fn build_runs_typescript_entry_importing_python_function() {
    let project = TempProject::new();
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src")).expect("create temp project");
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-imports-py"
version = "0.1.0"

[sources]
entries = ["src/main.ts", "src/math.py"]

[output]
target = "./dist"
crate-name = "ts_imports_py"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )
    .expect("write manifest");
    fs::write(
        project_path.join("src/math.py"),
        "def add(a: float, b: float) -> float:\n    return a + b\n",
    )
    .expect("write Python source");
    fs::write(
        project_path.join("src/main.ts"),
        "import { add } from './math';\nconst result = add(9, 4);\nconsole.log(result);\n",
    )
    .expect("write TypeScript source");

    let manifest = project_path.join("Smelt.toml");
    smelt(&[
        "--manifest-path",
        manifest.to_str().expect("manifest path utf8"),
        "build",
    ]);

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"));
    assert_eq!(actual_stdout, "13\n");
}

#[test]
fn end_to_end_examples_match_expected_outputs() {
    for name in [
        "01_number",
        "02_string",
        "03_boolean",
        "04_null",
        "05_alias",
        "06_array_literal",
        "07_tuple_literal",
        "08_record_literal",
        "09_index_access",
        "10_unary_logical",
        "11_console_log_expressions",
        "12_while_sum",
        "13_for_of_sum",
        "14_c_for_loop",
        "15_break_continue",
        "16_switch_break_no_fallthrough",
        "17_mutating_array",
        "18_class_fields",
        "19_constructor",
        "20_method_call",
        "21_this_field",
        "22_mutating_method",
        "23_interface_shape",
        "24_interface_method_signature",
        "25_private_protected_metadata",
        "26_interface_inheritance_optional_computed",
    ] {
        let example = workspace_root()
            .join("examples/typescript/end-to-end")
            .join(name);
        let input = example.join("input.ts");
        let expected_mir = fs::read_to_string(example.join("expected.mir")).expect("expected MIR");

        let actual_mir = smelt(&[
            "dump-mir",
            input
                .strip_prefix(workspace_root())
                .expect("relative input")
                .to_str()
                .expect("input path utf8"),
        ]);
        assert_eq!(actual_mir, expected_mir, "MIR mismatch for {name}");

        let project = TempProject::new();
        let project_path = project.path();
        fs::create_dir_all(project_path.join("src")).expect("create temp project");
        fs::write(
            project_path.join("src/main.ts"),
            fs::read_to_string(&input).expect("input source"),
        )
        .expect("write temp source");
        fs::write(
            project_path.join("Smelt.toml"),
            r#"[project]
name = "example-app"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "example_app"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
        )
        .expect("write manifest");

        let manifest = project_path.join("Smelt.toml");
        smelt(&[
            "--manifest-path",
            manifest.to_str().expect("manifest path utf8"),
            "build",
        ]);

        let expected_rs = fs::read_to_string(example.join("expected.rs")).expect("expected Rust");
        let actual_rs =
            fs::read_to_string(project_path.join("dist/src/main.rs")).expect("actual Rust");
        assert_eq!(actual_rs, expected_rs, "Rust mismatch for {name}");

        let expected_stdout =
            fs::read_to_string(example.join("expected.stdout")).expect("expected stdout");
        let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"));
        assert_eq!(
            actual_stdout, expected_stdout,
            "runtime stdout mismatch for {name}"
        );
    }
}
