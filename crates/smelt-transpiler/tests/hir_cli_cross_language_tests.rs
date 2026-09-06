//! Cross-language and fixture-sweep integration tests.

mod common;

use std::fs;

use common::{
    TempProject, TestResult, cargo_run_manifest, ensure, ensure_eq, smelt, utf8_path,
    verify_end_to_end_example, verify_python_end_to_end_example,
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
        generated.contains("#[path = \"main_1.rs\"]\nmod __smelt_module_main_1;"),
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
        r"import alpha, beta
from pkg.sub.helper import (
    compute,
)

result: int = alpha.first() + beta.second() + compute(5)
print(result)
",
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
        "28_regex_match_result",
        "29_callable_object",
        "30_nullish_union_join",
        "31_headers_fetch_type",
    ] {
        verify_end_to_end_example(name)?;
    }

    Ok(())
}

/// Golden-checks the Python example corpus, which no test read before.
///
/// These fixtures shipped with only `input.py` and a `Smelt.toml`, so a Python
/// lowering or formatting regression could not fail anything. Only the HIR and
/// MIR tiers are asserted here: the corpus has no generated-Rust or runtime
/// goldens yet, and adding those needs a `smelt build` per case.
#[test]
fn python_end_to_end_examples_match_expected_dumps() -> TestResult {
    for name in [
        "01_number",
        "02_string",
        "03_boolean",
        "04_none",
        "05_while_sum",
        "06_function",
        "07_if_else",
        "08_match",
    ] {
        verify_python_end_to_end_example(name)?;
    }

    Ok(())
}

#[test]
fn build_runs_nested_compound_condition_while_loop() -> TestResult {
    // A compound-condition inner `while` nested inside an outer loop must lower
    // its back-edge so each iteration re-evaluates the FULL `&&` condition. If
    // the back-edge instead `continue`s the outer loop, `combinations` never
    // advances `indices` and the program infinite-loops. Building and RUNNING
    // the program is the only way to prove the hang is gone; the `combinations`
    // shape mirrors the es-toolkit case that first exposed the bug.
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "nested-compound-while"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "nested_compound_while"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        r"function combinations(n: number, r: number): number[][] {
  const result: number[][] = [];
  const indices: number[] = [];
  for (let k = 0; k < r; k++) indices.push(k);
  while (true) {
    const tuple: number[] = [];
    for (let j = 0; j < r; j++) tuple.push(indices[j]);
    result.push(tuple);
    let i = r - 1;
    while (i >= 0 && indices[i] === i + n - r) i--;
    if (i < 0) break;
    indices[i]++;
    for (let j = i + 1; j < r; j++) indices[j] = indices[j - 1] + 1;
  }
  return result;
}
const c = combinations(4, 2);
console.log(c.length);
console.log(c[0][0]);
console.log(c[0][1]);
console.log(c[5][0]);
console.log(c[5][1]);
",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    // 4 choose 2 = 6 tuples; first is [0,1] and last is [2,3].
    ensure_eq(&actual_stdout, &"6\n0\n1\n2\n3\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_runs_loop_body_switch_with_nested_loop() -> TestResult {
    // A `for` loop whose body is a `switch` (with an inner `for` inside one arm)
    // must lower as a real loop whose arms route their back-edge to `continue`
    // and whose inner loop is preserved. This mirrors the es-toolkit `omit`
    // shape (outer `for` over keys, `switch` on the key kind, inner `for`
    // deleting each nested key). Before the fix the outer loop was either
    // mis-recognized as a compound `while` header — dropping the inner loop body
    // and switching on an uninitialized temp (E0381) — or emitted as a run-once
    // straight-line block that never iterated. Building and RUNNING is the only
    // way to prove both the loop iterates and the inner body executes.
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "loop-body-switch"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "loop_body_switch"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        r"interface Item { tag: string; vals: number[]; }
function f(items: Item[]): number {
  let total = 0;
  for (let i = 0; i < items.length; i++) {
    const it = items[i];
    switch (it.tag) {
      case 'sum': {
        for (let j = 0; j < it.vals.length; j++) {
          total = total + it.vals[j];
        }
        break;
      }
      case 'one': {
        total = total + 1;
        break;
      }
    }
  }
  return total;
}
const items: Item[] = [
  { tag: 'sum', vals: [1, 2, 3] },
  { tag: 'one', vals: [] },
  { tag: 'sum', vals: [10, 20] },
];
console.log(f(items));
",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    // (1+2+3) + 1 + (10+20) = 37; a dropped inner loop or non-iterating outer
    // loop would produce a smaller number.
    ensure_eq(&actual_stdout, &"37\n".to_owned(), "unexpected stdout")?;

    Ok(())
}
