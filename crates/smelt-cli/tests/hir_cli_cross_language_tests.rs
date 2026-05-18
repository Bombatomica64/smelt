//! Cross-language and fixture-sweep integration tests.

mod common;

use std::fs;

use common::{
    TempProject, TestResult, cargo_run_manifest, ensure, ensure_eq, smelt, utf8_path,
    verify_end_to_end_example,
};

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

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"5\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_keeps_one_main_when_python_entry_imports_typescript_main_module() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src/lib"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "cross-main-name"
version = "0.1.0"

[sources]
entries = ["src/main.py"]

[output]
target = "./dist"
crate-name = "cross_main_name"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/lib/main.ts"),
        "export function add(a: number, b: number): number {\n  return a + b;\n}\n",
    )?;
    fs::write(
        project_path.join("src/main.py"),
        "from lib.main import add\nresult: float = add(4.0, 6.0)\nprint(result)\n",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let generated = fs::read_to_string(project_path.join("dist/src/main.rs"))?;
    ensure_eq(
        &generated.matches("fn main(").count(),
        &1,
        "generated Rust should contain one executable main function",
    )?;
    ensure(
        generated.contains("mod main_1;"),
        "dependency module body should be emitted in its source module",
    )?;
    let dependency_module = fs::read_to_string(project_path.join("dist/src/main_1.rs"))?;
    ensure(
        dependency_module.contains("fn main_1()"),
        "dependency module body should be renamed away from main",
    )?;
    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"10\n".to_owned(), "unexpected stdout")?;

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

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
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

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"15\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_discovers_python_ast_import_forms() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src/pkg/sub"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "py-ast-import-discovery"
version = "0.1.0"

[sources]
entries = ["src/main.py"]

[output]
target = "./dist"
crate-name = "py_ast_import_discovery"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(project_path.join("src/pkg/__init__.py"), "")?;
    fs::write(project_path.join("src/pkg/sub/__init__.py"), "")?;
    fs::write(
        project_path.join("src/alpha.py"),
        "def first() -> int:\n    return 2\n",
    )?;
    fs::write(
        project_path.join("src/beta.py"),
        "def second() -> int:\n    return 3\n",
    )?;
    fs::write(
        project_path.join("src/pkg/util.py"),
        "def bonus() -> int:\n    return 4\n",
    )?;
    fs::write(
        project_path.join("src/pkg/sub/helper.py"),
        "from ..util import bonus\n\ndef compute(value: int) -> int:\n    return bonus() + value\n",
    )?;
    fs::write(
        project_path.join("src/main.py"),
        r#"import alpha, beta
from pkg.sub.helper import (
    compute,
)

result: int = alpha.first() + beta.second() + compute(5)
print(result)
"#,
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"14\n".to_owned(), "unexpected stdout")?;

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

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
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
        "27_optional_chains",
    ] {
        verify_end_to_end_example(name)?;
    }

    Ok(())
}
