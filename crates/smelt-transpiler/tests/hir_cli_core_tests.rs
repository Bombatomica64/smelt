//! Core CLI integration tests for HIR/MIR rendering and basic build/check flows.

mod common;

use std::fs;

use common::{
    TempProject, TestResult, cargo_test_manifest, ensure, ensure_eq, smelt, smelt_in,
    utf8_path,
};

#[test]
fn build_specializes_python_decorators_and_emits_package_artifact() -> TestResult {
    if !std::path::Path::new("/usr/bin/bwrap").is_file()
        || std::process::Command::new("python3")
            .arg("--version")
            .output()
            .is_err()
    {
        return Ok(());
    }
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "python-specialization-cache"
version = "0.1.0"

[sources]
entries = ["src/main.py"]

[output]
target = "./dist"
crate-name = "python_specialization_cache"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/main.py"),
        r#"def decorate(function):
    def wrapped(name: str) -> str:
        return "wrapped " + function(name)
    return wrapped

@decorate
def greet(name: str) -> str:
    return "hello " + name

print(greet("smelt"))
"#,
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;
    let artifact_root = project_path.join("dist/.smelt/specialization/python");
    ensure(
        fs::read_dir(&artifact_root)?
            .filter_map(Result::ok)
            .any(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "json")
            }),
        "Python specialization package artifact was not emitted",
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
        // After copy propagation resolves the alias to `%0`, move-on-last-use
        // turns the final use into a move (the value is dead afterwards).
        stdout.contains("%2 = call @console_log(move %0) -> bb1"),
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
fn build_can_emit_library_crate_root() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::create_dir_all(project_path.join("dist/src"))?;
    fs::write(project_path.join("dist/src/main.rs"), "fn main() {}\n")?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "generated-library"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "generated_library"
kind = "library"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "export function add(left: number, right: number): number { return left + right; }\n",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let generated = fs::read_to_string(project_path.join("dist/src/lib.rs"))?;
    let source_module = fs::read_to_string(project_path.join("dist/src/source_main.rs"))?;
    ensure(
        generated.contains("#[path = \"source_main.rs\"]"),
        "library root did not include source module",
    )?;
    ensure(
        source_module.contains("fn add("),
        "missing generated library function",
    )?;
    ensure(
        !project_path.join("dist/src/main.rs").exists(),
        "stale program crate root was not removed",
    )?;

    Ok(())
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "end-to-end CLI fixture: the inline project sources and the assertions over the\n             generated output belong to one scenario, and splitting them into helpers would\n             hide what this test actually builds"
)]
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
    fs::write(
        project_path.join("src/main.py"),
        "\nprint(\"rich-null-file\")\n",
    )?;

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
fn build_runs_python_value_returning_or_none() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "py-or-none"
version = "0.1.0"

[sources]
entries = ["src/main.py"]

[output]
target = "./dist"
crate-name = "py_or_none"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/main.py"),
        r#"
class Obj:
    id: str

    def __init__(self, id: str) -> None:
        self.id = id

obj: Obj = Obj("a")
value: str | None = obj.id or None
print(value)
"#,
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    // CPython prints `a`: `obj.id or None` is the truthy string. This asserted
    // `Some("a")` -- Rust's `Option` Debug leaking into program output through
    // `console.log`/`print`, which no source language produces.
    let actual_stdout = common::cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"a\n".to_owned(), "unexpected stdout")?;

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
        "export interface Options { in: number; }\nexport function add(a: number, b: number): number { return a + b; }\n",
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
    ensure(
        declaration.contains("\"in\": number;"),
        "reserved TypeScript declaration property was not quoted",
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
fn check_treats_generated_declarations_as_declaration_inputs() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "declaration-input"
version = "0.1.0"

[sources]
entries = ["src/generated.d.ts", "src/generated.pyi"]

[output]
target = "./dist"
crate-name = "declaration_input"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/generated.d.ts"),
        "// Generated by smelt. Do not edit.\n\nexport interface Options {\n  in: ((Date | number | string) => DateType) | null | null;\n}\n",
    )?;
    fs::write(
        project_path.join("src/generated.pyi"),
        "# Generated by smelt. Do not edit.\n\ndef add(value: float) -> float: ...\n",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "check"])?;

    ensure(
        !project_path.join("src/generated.d.d.ts").exists(),
        "declaration inputs should not be re-stubbed",
    )?;
    ensure(
        !project_path.join("src/generated.pyi.d.ts").exists(),
        "Python stub inputs should not receive TypeScript stubs",
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
    ensure(
        stub.contains("def add(a: int, b: int) -> int: ..."),
        "missing Python stub",
    )?;
    ensure(
        entry_stub.contains("Generated by smelt"),
        "missing entry stub",
    )?;
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

#[test]
fn check_orders_manifest_entries_by_type_only_import_dependencies() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src/isSaturday"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "type-only-import-order"
version = "0.1.0"

[sources]
entries = ["src/isSaturday/index.ts", "src/index.ts", "src/types.ts"]

[output]
target = "./dist"
crate-name = "type_only_import_order"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/types.ts"),
        r"export interface ContextOptions<DateType extends Date = Date> {
  in?: DateType;
}
",
    )?;
    fs::write(
        project_path.join("src/index.ts"),
        r#"export type { ContextOptions } from "./types";
"#,
    )?;
    fs::write(
        project_path.join("src/isSaturday/index.ts"),
        r#"import type { ContextOptions } from "../index";

interface LocalOptions<DateType extends Date = Date> extends ContextOptions<DateType> {}

export function touch(options: LocalOptions): void {
  const context = options.in;
}
"#,
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "check"])?;

    Ok(())
}

/// The manifest text used by the two manifest-directory tests below.
///
/// One entry, one module reached only by a TYPE-only import, and an `exclude`
/// glob naming that module. A type-only import still puts the module in the
/// dependency closure, so the exclude is the only thing that keeps it out — and
/// whether it is applied is visible as a file in the generated crate.
const EXCLUDE_PROJECT_MANIFEST: &str = r#"[project]
name = "exclude-cwd"
version = "0.1.0"

[sources]
roots = ["src"]
entries = ["src/main.ts"]
exclude = ["src/notes.ts"]

[output]
target = "./dist"
crate-name = "exclude_cwd"
build = false

[runtime]
clone-strategy = "aggressive"
"#;

/// Write the exclude fixture into `project_path`.
fn write_exclude_project(project_path: &std::path::Path) -> TestResult {
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(project_path.join("Smelt.toml"), EXCLUDE_PROJECT_MANIFEST)?;
    fs::write(
        project_path.join("src/notes.ts"),
        "export interface Note { id: number }\nexport function noteMarker(): number { return 7 }\n",
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "import type { Note } from './notes'\nconst note: Note = { id: 1 }\nconsole.log(note.id)\n",
    )?;
    Ok(())
}

/// Read every generated `.rs` file under `dir`, sorted by file name.
fn read_generated_sources(dir: &std::path::Path) -> TestResult<Vec<(String, String)>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| extension == "rs") {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned();
            files.push((name, fs::read_to_string(&path)?));
        }
    }
    files.sort();
    Ok(files)
}

#[test]
fn exclude_globs_resolve_against_the_manifest_directory_not_the_process_directory() -> TestResult {
    // `[sources] exclude` globs are written relative to the MANIFEST, so which
    // directory they are matched against cannot depend on where `smelt` was
    // invoked from. It did: the manifest directory came from
    // `manifest_path.parent()`, which is `Some("")` — not `None` — for a path
    // with no directory component, so the usual `.unwrap_or(".")` never fired.
    // An empty prefix makes `strip_prefix` succeed while stripping nothing, so
    // every canonicalized dependency path stayed absolute and no
    // manifest-relative glob could match it.
    //
    // NON-VACUOUS: before the fix, running from inside the project with
    // `--manifest-path Smelt.toml` emitted `dist/src/notes.rs` for the module
    // the manifest excludes, while the same tree built with an absolute
    // manifest path did not.
    let project = TempProject::new()?;
    let project_path = project.path();
    write_exclude_project(project_path)?;

    for manifest_arg in ["Smelt.toml", "./Smelt.toml"] {
        drop(fs::remove_dir_all(project_path.join("dist")));
        smelt_in(project_path, &["--manifest-path", manifest_arg, "build"])?;
        ensure(
            !project_path.join("dist/src/notes.rs").exists(),
            "an excluded module was lowered for a cwd-relative manifest path",
        )?;
    }

    // The absolute spelling, from an unrelated working directory, has to agree.
    drop(fs::remove_dir_all(project_path.join("dist")));
    let absolute = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &absolute, "build"])?;
    ensure(
        !project_path.join("dist/src/notes.rs").exists(),
        "an excluded module was lowered for an absolute manifest path",
    )?;
    ensure(
        project_path.join("dist/src/main.rs").exists(),
        "the entry module was not lowered",
    )?;

    Ok(())
}

#[test]
fn building_twice_produces_identical_output() -> TestResult {
    // `smelt build` writes a `.d.ts` and a `.pyi` beside every module it lowers,
    // so the second build runs over a source tree that contains the first
    // build's output. Those stubs must never become inputs: a generated
    // `context.d.ts` standing in for the real `context.ts` would quietly change
    // the crate. The property is "a build is a function of the SOURCE", and this
    // pins it by comparing every emitted file byte for byte.
    let project = TempProject::new()?;
    let project_path = project.path();
    write_exclude_project(project_path)?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;
    let first = read_generated_sources(&project_path.join("dist/src"))?;
    ensure(!first.is_empty(), "the first build emitted nothing")?;
    ensure(
        project_path.join("src/main.d.ts").exists(),
        "the first build did not write its stubs into the source tree",
    )?;

    smelt(&["--manifest-path", &manifest_arg, "build"])?;
    let second = read_generated_sources(&project_path.join("dist/src"))?;
    ensure_eq(
        &format!("{second:?}"),
        &format!("{first:?}"),
        "a second build over the first build's stubs changed the generated crate",
    )?;

    Ok(())
}
