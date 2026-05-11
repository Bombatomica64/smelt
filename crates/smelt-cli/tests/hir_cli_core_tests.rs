//! Core CLI integration tests for HIR/MIR rendering and basic build/check flows.

mod common;

use std::fs;

use common::{
    TestResult, TempProject, cargo_test_manifest, ensure, ensure_eq, smelt, utf8_path,
};

#[test]
fn dump_hir_prints_compact_hir_for_single_file() -> TestResult {
    let stdout = smelt(&["dump-hir", "examples/typescript/hir/01_number.ts"])?;

    ensure(
        stdout.contains("module examples/typescript/hir/01_number.ts (ModuleId(0))"),
        "missing module header",
    )?;
    ensure(stdout.contains("%0 let count: Float"), "missing count line")?;
    ensure(stdout.contains("#3: None = call #2(#1)"), "missing call line")?;
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

    ensure(stdout.contains("fn main (FuncId(0)) -> None"), "missing fn header")?;
    ensure(stdout.contains("%0 user source_value: Float"), "missing source value")?;
    ensure(stdout.contains("%1 user copied_value: Float"), "missing copied value")?;
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

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let generated = fs::read_to_string(project_path.join("dist/src/main.rs"))?;
    ensure(generated.contains("fn main()"), "missing fn main")?;
    ensure(generated.contains("println!"), "missing println")?;

    Ok(())
}

#[test]
fn build_python_rich_like_null_file_package_fixture() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src/rich"))?;
    fs::create_dir_all(project_path.join("tests"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "rich-null-file"
version = "0.1.0"

[sources]
entries = ["src/rich/_null_file.py", "src/rich/__init__.py", "tests/test_null_file.py", "src/main.py"]

[output]
target = "./dist"
crate-name = "rich_null_file"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/rich/_null_file.py"),
        r#"
class NullFile:
    def write(self, text: str) -> int:
        return 0
    def __enter__(self) -> NullFile:
        return self
    def __exit__(self, *_args: object) -> None:
        pass
    def __iter__(self) -> NullFile:
        return self
    def __next__(self) -> str:
        raise StopIteration
    def __str__(self) -> str:
        return ""

NULL_FILE = NullFile()
"#,
    )?;
    fs::write(
        project_path.join("src/rich/__init__.py"),
        r#"
from ._null_file import NULL_FILE, NullFile

__all__ = ["NULL_FILE", "NullFile"]
"#,
    )?;
    fs::write(
        project_path.join("tests/test_null_file.py"),
        r#"
from rich import NULL_FILE, NullFile

def test_null_file_protocols():
    value: NullFile = NULL_FILE
    text: str = str(value)
    assert text == ""
    with value as handle:
        assert handle.write("ignored") == 0
    for line in value:
        assert line == ""
"#,
    )?;
    fs::write(project_path.join("src/main.py"), "\nprint(\"rich-null-file\")\n")?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;
    let test_stdout = cargo_test_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure(
        test_stdout.contains("test result: ok"),
        "generated Rich-like NullFile tests did not pass",
    )?;

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

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "check"])?;

    let declaration = fs::read_to_string(project_path.join("src/math.d.ts"))?;
    let entry_declaration = fs::read_to_string(project_path.join("src/main.d.ts"))?;
    let python_stub = fs::read_to_string(project_path.join("src/math.pyi"))?;
    let entry_python_stub = fs::read_to_string(project_path.join("src/main.pyi"))?;
    ensure(
        declaration.contains("export declare function add(a: number, b: number): number;"),
        "missing TypeScript declaration",
    )?;
    ensure(entry_declaration.contains("Generated by smelt"), "missing entry declaration")?;
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

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "check"])?;

    let stub = fs::read_to_string(project_path.join("src/math.pyi"))?;
    let entry_stub = fs::read_to_string(project_path.join("src/main.pyi"))?;
    let ts_declaration = fs::read_to_string(project_path.join("src/math.d.ts"))?;
    let entry_ts_declaration = fs::read_to_string(project_path.join("src/main.d.ts"))?;
    ensure(stub.contains("def add(a: int, b: int) -> int: ..."), "missing Python stub")?;
    ensure(entry_stub.contains("Generated by smelt"), "missing entry stub")?;
    ensure(
        ts_declaration.contains("export declare function add(a: number, b: number): number;"),
        "missing TypeScript declaration",
    )?;
    ensure(
        entry_ts_declaration.contains("Generated by smelt"),
        "missing entry TypeScript declaration",
    )?;

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

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = common::cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"5\n".to_owned(), "unexpected stdout")?;

    Ok(())
}
