//! Integration tests for the `smelt` CLI HIR/MIR/build workflows.

use std::{
    error::Error,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// Returns the workspace root for the integration tests.
fn workspace_root() -> Result<&'static Path, io::Error> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "workspace root"))
}

/// Runs the `smelt` binary from the workspace root and returns stdout.
fn smelt(args: &[&str]) -> TestResult<String> {
    let output = Command::new(env!("CARGO_BIN_EXE_smelt"))
        .current_dir(workspace_root()?)
        .args(args)
        .output()?;

    if !output.status.success() {
        return Err(io::Error::other(format!(
            "smelt failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }

    Ok(String::from_utf8(output.stdout)?)
}

/// Temporary project directory used by the integration tests.
struct TempProject {
    path: PathBuf,
}

impl TempProject {
    /// Creates a unique temporary project path.
    fn new() -> Result<Self, std::time::SystemTimeError> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        Ok(Self {
            path: std::env::temp_dir()
                .join(format!("smelt-cli-test-{}-{nonce}", std::process::id())),
        })
    }

    /// Returns the temporary project path.
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.path));
    }
}

/// Runs `cargo run --manifest-path` for a generated crate and returns stdout.
fn cargo_run_manifest(manifest: &Path) -> TestResult<String> {
    let output = Command::new("cargo")
        .arg("run")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(manifest)
        .output()?;

    if !output.status.success() {
        return Err(io::Error::other(format!(
            "generated crate failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }

    Ok(String::from_utf8(output.stdout)?)
}

/// Converts a path to UTF-8 for CLI arguments.
fn utf8_path(path: &Path) -> Result<String, io::Error> {
    path.to_str().map(ToOwned::to_owned).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("path is not valid UTF-8: {}", path.display()),
        )
    })
}

/// Returns the absolute path to an end-to-end example fixture.
fn example_dir(name: &str) -> TestResult<PathBuf> {
    Ok(workspace_root()?
        .join("examples/typescript/end-to-end")
        .join(name))
}

/// Fails the test when `condition` is false.
fn ensure(condition: bool, message: impl Into<String>) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message.into()).into())
    }
}

/// Fails the test when `actual` and `expected` differ.
fn ensure_eq<T>(actual: &T, expected: &T, message: impl Into<String>) -> TestResult
where
    T: PartialEq + std::fmt::Debug,
{
    if actual == expected {
        Ok(())
    } else {
        Err(io::Error::other(message.into()).into())
    }
}

/// Verifies the compiled output for a single end-to-end example fixture.
fn verify_end_to_end_example(name: &str) -> TestResult {
    let example = example_dir(name)?;
    let input = example.join("input.ts");
    let expected_mir = fs::read_to_string(example.join("expected.mir"))?;

    let workspace_root = workspace_root()?;
    let input_path = input.strip_prefix(workspace_root)?;
    let actual_mir = smelt(&["dump-mir", &utf8_path(input_path)?])?;
    ensure_eq(
        &actual_mir,
        &expected_mir,
        format!("MIR mismatch for {name}"),
    )?;

    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("src/main.ts"),
        fs::read_to_string(&input)?,
    )?;
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
    )?;

    let manifest = project_path.join("Smelt.toml");
    let manifest_arg = utf8_path(&manifest)?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let expected_rs = fs::read_to_string(example.join("expected.rs"))?;
    let actual_rs = fs::read_to_string(project_path.join("dist/src/main.rs"))?;
    ensure_eq(
        &actual_rs,
        &expected_rs,
        format!("Rust mismatch for {name}"),
    )?;

    let expected_stdout = fs::read_to_string(example.join("expected.stdout"))?;
    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(
        &actual_stdout,
        &expected_stdout,
        format!("runtime stdout mismatch for {name}"),
    )?;

    Ok(())
}

#[test]
fn dump_hir_prints_compact_hir_for_single_file() -> TestResult {
    let stdout = smelt(&["dump-hir", "examples/typescript/hir/01_number.ts"])?;

    ensure(
        stdout.contains("module examples/typescript/hir/01_number.ts (ModuleId(0))"),
        "missing module header",
    )?;
    ensure(stdout.contains("%0 let count: Float"), "missing count line")?;
    ensure(
        stdout.contains("#3: None = call #2(#1)"),
        "missing call line",
    )?;
    ensure(
        stdout.contains("interned types\n  t0 = Float\n  t1 = None\n"),
        "missing interned types section",
    )?;

    Ok(())
}

#[test]
fn build_hir_reads_entries_relative_to_manifest() -> TestResult {
    let stdout = smelt(&[
        "--manifest-path",
        "examples/typescript/hir/Smelt.toml",
        "build",
        "--hir",
    ])?;

    ensure(
        stdout.contains("module examples/typescript/hir/01_number.ts (ModuleId(0))"),
        "missing module header",
    )?;
    ensure(stdout.contains("s0: let %0: Float = #0"), "missing s0")?;
    ensure(stdout.contains("s1: #3"), "missing s1")?;

    Ok(())
}

#[test]
fn dump_mir_prints_optimized_mir_for_single_file() -> TestResult {
    let stdout = smelt(&["dump-mir", "examples/typescript/hir/05_alias.ts"])?;

    ensure(
        stdout.contains("fn main (FuncId(0)) -> None"),
        "missing fn header",
    )?;
    ensure(
        stdout.contains("%0 user source_value: Float"),
        "missing source value",
    )?;
    ensure(
        stdout.contains("%1 user copied_value: Float"),
        "missing copied value",
    )?;
    ensure(
        stdout.contains("%2 = call @console_log(copy %0) -> bb1"),
        "missing log call",
    )?;
    ensure(stdout.contains("return none"), "missing return none")?;

    Ok(())
}

#[test]
fn build_emits_compilable_rust_crate() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
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
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "const message = \"hello smelt\";\nconsole.log(message);\n",
    )?;

    let manifest = project_path.join("Smelt.toml");
    let manifest_arg = utf8_path(&manifest)?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let generated = fs::read_to_string(project_path.join("dist/src/main.rs"))?;
    ensure(generated.contains("fn main()"), "missing fn main")?;
    ensure(generated.contains("println!"), "missing println")?;

    Ok(())
}

#[test]
fn check_emits_typescript_declaration_stubs_for_linked_modules() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
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
    )?;
    fs::write(
        project_path.join("src/math.ts"),
        "export function add(a: number, b: number): number { return a + b; }\n",
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "import { add } from './math';\nconst result = add(2, 3);\nconsole.log(result);\n",
    )?;

    let manifest = project_path.join("Smelt.toml");
    let manifest_arg = utf8_path(&manifest)?;
    smelt(&["--manifest-path", &manifest_arg, "check"])?;

    let declaration = fs::read_to_string(project_path.join("src/math.d.ts"))?;
    let entry_declaration = fs::read_to_string(project_path.join("src/main.d.ts"))?;
    let python_stub = fs::read_to_string(project_path.join("src/math.pyi"))?;
    let entry_python_stub = fs::read_to_string(project_path.join("src/main.pyi"))?;
    ensure(
        declaration.contains("export declare function add(a: number, b: number): number;"),
        "missing TypeScript declaration",
    )?;
    ensure(
        entry_declaration.contains("Generated by smelt"),
        "missing entry declaration",
    )?;
    ensure(
        python_stub.contains("def add(a: float, b: float) -> float: ..."),
        "missing Python stub",
    )?;
    ensure(
        entry_python_stub.contains("Generated by smelt"),
        "missing entry Python stub",
    )?;

    Ok(())
}

#[test]
fn check_emits_python_stubs_for_linked_modules() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
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
    )?;
    fs::write(
        project_path.join("src/math.py"),
        "def add(a: int, b: int) -> int:\n    return a + b\n",
    )?;
    fs::write(
        project_path.join("src/main.py"),
        "from math import add\nresult: int = add(2, 3)\nprint(result)\n",
    )?;

    let manifest = project_path.join("Smelt.toml");
    let manifest_arg = utf8_path(&manifest)?;
    smelt(&["--manifest-path", &manifest_arg, "check"])?;

    let stub = fs::read_to_string(project_path.join("src/math.pyi"))?;
    let entry_stub = fs::read_to_string(project_path.join("src/main.pyi"))?;
    let typescript_declaration = fs::read_to_string(project_path.join("src/math.d.ts"))?;
    let entry_typescript_declaration = fs::read_to_string(project_path.join("src/main.d.ts"))?;
    ensure(
        stub.contains("def add(a: int, b: int) -> int: ..."),
        "missing Python stub",
    )?;
    ensure(
        entry_stub.contains("Generated by smelt"),
        "missing entry stub",
    )?;
    ensure(
        typescript_declaration
            .contains("export declare function add(a: number, b: number): number;"),
        "missing TypeScript declaration",
    )?;
    ensure(
        entry_typescript_declaration.contains("Generated by smelt"),
        "missing entry TypeScript declaration",
    )?;

    Ok(())
}

#[test]
fn build_runs_python_entry_importing_typescript_function() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
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
    )?;
    fs::write(
        project_path.join("src/math.ts"),
        "export function add(a: number, b: number): number {\n  return a + b;\n}\n",
    )?;
    fs::write(
        project_path.join("src/main.py"),
        "from math import add\nresult: float = add(2.0, 3.0)\nprint(result)\n",
    )?;

    let manifest = project_path.join("Smelt.toml");
    let manifest_arg = utf8_path(&manifest)?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    ensure(
        project_path.join("src/math.d.ts").exists(),
        "missing src/math.d.ts",
    )?;
    ensure(
        project_path.join("src/math.pyi").exists(),
        "missing src/math.pyi",
    )?;
    ensure(
        project_path.join("src/main.d.ts").exists(),
        "missing src/main.d.ts",
    )?;
    ensure(
        project_path.join("src/main.pyi").exists(),
        "missing src/main.pyi",
    )?;
    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"5\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_orders_manifest_entries_by_import_dependencies() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
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
    )?;
    fs::write(
        project_path.join("src/math.ts"),
        "export function add(a: number, b: number): number {\n  return a + b;\n}\n",
    )?;
    fs::write(
        project_path.join("src/main.py"),
        "from math import add\nresult: float = add(2.0, 3.0)\nprint(result)\n",
    )?;

    let manifest = project_path.join("Smelt.toml");
    let manifest_arg = utf8_path(&manifest)?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"5\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_resolves_typescript_index_module_imports() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src/lib"))?;
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
    )?;
    fs::write(
        project_path.join("src/lib/index.ts"),
        "export function add(a: number, b: number): number {\n  return a + b;\n}\n",
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "import { add } from './lib';\nconst result = add(4, 6);\nconsole.log(result);\n",
    )?;

    let manifest = project_path.join("Smelt.toml");
    let manifest_arg = utf8_path(&manifest)?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"10\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_runs_date_fns_style_extensionful_const_import() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src/constants"))?;
    fs::create_dir_all(project_path.join("src/quartersToMonths"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "date-fns-style-import"
version = "0.1.0"

[sources]
entries = [
  "src/quartersToMonths/index.ts",
  "src/constants/index.ts",
  "src/main.ts",
]

[output]
target = "./dist"
crate-name = "date_fns_style_import"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/constants/index.ts"),
        "export const monthsInQuarter = 3;\n",
    )?;
    fs::write(
        project_path.join("src/quartersToMonths/index.ts"),
        "import { monthsInQuarter } from \"../constants/index.ts\";\n\
export function quartersToMonths(quarters: number): number {\n  return Math.trunc(quarters * monthsInQuarter);\n}\n",
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "import { quartersToMonths } from \"./quartersToMonths/index.ts\";\n\
const result = quartersToMonths(2);\nconsole.log(result);\n",
    )?;

    let manifest = project_path.join("Smelt.toml");
    let manifest_arg = utf8_path(&manifest)?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"6\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_runs_typescript_folded_const_expression_import() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-folded-const-import"
version = "0.1.0"

[sources]
entries = ["src/main.ts", "src/constants.ts"]

[output]
target = "./dist"
crate-name = "ts_folded_const_import"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/constants.ts"),
        "export const base = Math.pow(10, 2);\nexport const maxTime = base * 5;\nexport const minTime = -maxTime;\n",
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "import { minTime } from './constants';\nconsole.log(minTime);\n",
    )?;

    let manifest = project_path.join("Smelt.toml");
    let manifest_arg = utf8_path(&manifest)?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"-500\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_resolves_typescript_reexport_imports() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-reexport-import"
version = "0.1.0"

[sources]
entries = ["src/main.ts", "src/index.ts", "src/math.ts"]

[output]
target = "./dist"
crate-name = "ts_reexport_import"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/math.ts"),
        "export function add(a: number, b: number): number {\n  return a + b;\n}\n",
    )?;
    fs::write(
        project_path.join("src/index.ts"),
        "export { add as plus } from './math';\n",
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "import { plus } from './index';\nconst result = plus(4, 5);\nconsole.log(result);\n",
    )?;

    let manifest = project_path.join("Smelt.toml");
    let manifest_arg = utf8_path(&manifest)?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"9\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_resolves_typescript_namespace_imports() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-namespace-import"
version = "0.1.0"

[sources]
entries = ["src/main.ts", "src/number.ts"]

[output]
target = "./dist"
crate-name = "ts_namespace_import"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/number.ts"),
        "export function double(value: number): number {\n  return value * 2;\n}\n",
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "import * as NumberInstances from './number';\nconst result = NumberInstances.double(6);\nconsole.log(result);\n",
    )?;

    let manifest = project_path.join("Smelt.toml");
    let manifest_arg = utf8_path(&manifest)?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"12\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_resolves_typescript_exported_arrow_function_consts() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-exported-arrow-const"
version = "0.1.0"

[sources]
entries = ["src/main.ts", "src/number.ts"]

[output]
target = "./dist"
crate-name = "ts_exported_arrow_const"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/number.ts"),
        "export const double = (value: number): number => value * 2;\n",
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "import { double } from './number';\nconst result = double(6);\nconsole.log(result);\n",
    )?;

    let manifest = project_path.join("Smelt.toml");
    let manifest_arg = utf8_path(&manifest)?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"12\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_resolves_typescript_object_namespace_consts() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-object-namespace"
version = "0.1.0"

[sources]
entries = ["src/main.ts", "src/number.ts"]

[output]
target = "./dist"
crate-name = "ts_object_namespace"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/number.ts"),
        "export function double(value: number): number {\n  return value * 2;\n}\nexport const NumberInstances = { double };\n",
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "import { NumberInstances } from './number';\nconst result = NumberInstances.double(6);\nconsole.log(result);\n",
    )?;

    let manifest = project_path.join("Smelt.toml");
    let manifest_arg = utf8_path(&manifest)?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"12\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_resolves_python_package_init_imports() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src/lib"))?;
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
    )?;
    fs::write(
        project_path.join("src/lib/__init__.py"),
        "def add(a: int, b: int) -> int:\n    return a + b\n",
    )?;
    fs::write(
        project_path.join("src/main.py"),
        "from lib import add\nresult: int = add(7, 8)\nprint(result)\n",
    )?;

    let manifest = project_path.join("Smelt.toml");
    let manifest_arg = utf8_path(&manifest)?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"15\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_resolves_python_package_namespace_imports() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src/httpx"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "py-package-namespace-import"
version = "0.1.0"

[sources]
entries = ["src/main.py", "src/httpx/__init__.py"]

[output]
target = "./dist"
crate-name = "py_package_namespace_import"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/httpx/__init__.py"),
        "def add(a: int, b: int) -> int:\n    return a + b\n",
    )?;
    fs::write(
        project_path.join("src/main.py"),
        "import httpx\nresult: int = httpx.add(7, 8)\nprint(result)\n",
    )?;

    let manifest = project_path.join("Smelt.toml");
    let manifest_arg = utf8_path(&manifest)?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"15\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_runs_typescript_entry_importing_python_function() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
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
    )?;
    fs::write(
        project_path.join("src/math.py"),
        "def add(a: float, b: float) -> float:\n    return a + b\n",
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "import { add } from './math';\nconst result = add(9, 4);\nconsole.log(result);\n",
    )?;

    let manifest = project_path.join("Smelt.toml");
    let manifest_arg = utf8_path(&manifest)?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"13\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn end_to_end_examples_match_expected_outputs() -> TestResult {
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
        verify_end_to_end_example(name)?;
    }

    Ok(())
}
