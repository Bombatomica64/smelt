//! TypeScript-focused integration tests for manifest builds.

mod common;

use std::fs;

use common::{
    TempProject, TestResult, cargo_run_manifest, cargo_test_manifest, ensure, ensure_eq, smelt,
    utf8_path,
};

#[test]
fn build_specializes_decorators_and_reuses_packaged_manifest() -> TestResult {
    if !std::path::Path::new("/usr/bin/bwrap").is_file()
        || std::process::Command::new("node")
            .arg("--version")
            .output()
            .is_err()
        || std::process::Command::new("tsc")
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
name = "ts-specialization-cache"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "ts_specialization_cache"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        r#"function identity(value: any, _context: any): any {
  return value;
}

@identity
export class Example {
  @identity
  static label: string = "ready";
}

console.log(Example.label);
"#,
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;
    let artifact_root = project_path.join("dist/.smelt/specialization/typescript");
    let artifact = fs::read_dir(&artifact_root)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .ok_or("TypeScript specialization package artifact was not emitted")?;
    let first_bytes = fs::read(&artifact)?;
    let first_modified = fs::metadata(&artifact)?.modified()?;

    smelt(&["--manifest-path", &manifest_arg, "build"])?;
    ensure_eq(
        &fs::read(&artifact)?,
        &first_bytes,
        "cache hit changed package artifact bytes",
    )?;
    ensure_eq(
        &fs::metadata(&artifact)?.modified()?,
        &first_modified,
        "cache hit rewrote an identical package artifact",
    )?;
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

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
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

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"6\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_runs_date_fns_quarters_to_months_tests() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src/constants"))?;
    fs::create_dir_all(project_path.join("src/quartersToMonths"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "date-fns-quarters-to-months"
version = "0.1.0"

[sources]
entries = [
  "src/quartersToMonths/test.ts",
  "src/quartersToMonths/index.ts",
  "src/constants/index.ts",
]

[output]
target = "./dist"
crate-name = "date_fns_quarters_to_months"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/constants/index.ts"),
        r#"
export const daysInWeek = 7;
export const daysInYear = 365.2425;
export const maxTime = Math.pow(10, 8) * 24 * 60 * 60 * 1000;
export const minTime = -maxTime;
export const millisecondsInWeek = 604800000;
export const millisecondsInDay = 86400000;
export const millisecondsInMinute = 60000;
export const millisecondsInHour = 3600000;
export const millisecondsInSecond = 1000;
export const minutesInYear = 525600;
export const minutesInMonth = 43200;
export const minutesInDay = 1440;
export const minutesInHour = 60;
export const monthsInQuarter = 3;
export const monthsInYear = 12;
export const quartersInYear = 4;
export const secondsInHour = 3600;
export const secondsInMinute = 60;
export const secondsInDay = secondsInHour * 24;
export const secondsInWeek = secondsInDay * 7;
export const secondsInYear = secondsInDay * daysInYear;
export const secondsInMonth = secondsInYear / 12;
export const secondsInQuarter = secondsInMonth * 3;
export const constructFromSymbol = Symbol.for("constructDateFrom");
"#,
    )?;
    fs::write(
        project_path.join("src/quartersToMonths/index.ts"),
        r#"
import { monthsInQuarter } from "../constants/index.ts";

export function quartersToMonths(quarters: number): number {
  return Math.trunc(quarters * monthsInQuarter);
}
"#,
    )?;
    fs::write(
        project_path.join("src/quartersToMonths/test.ts"),
        r#"
import { describe, expect, it } from "vitest";
import { quartersToMonths } from "./index.ts";

describe("quartersToMonths", () => {
  it("converts quarters to months", () => {
    expect(quartersToMonths(1)).toBe(3);
    expect(quartersToMonths(2)).toBe(6);
  });

  it("uses floor rounding", () => {
    expect(quartersToMonths(1.5)).toBe(4);
    expect(quartersToMonths(0.3)).toBe(0);
  });

  it("handles border values", () => {
    expect(quartersToMonths(0.4)).toBe(1);
    expect(quartersToMonths(0)).toBe(0);
  });

  it("properly works with negative numbers", () => {
    expect(quartersToMonths(12.34)).toBe(37);
    expect(quartersToMonths(-12.34)).toBe(-37);
  });
});
"#,
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let test_stdout = cargo_test_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure(
        test_stdout.contains("test result: ok"),
        "generated tests did not pass",
    )?;

    Ok(())
}

#[test]
fn build_discovers_typescript_tests_from_glob() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src/math"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-test-glob"
version = "0.1.0"

[sources]
roots = ["src"]
entries = ["src/math/index.ts"]
test-prefix = ["**/*.test.ts"]

[output]
target = "./dist"
crate-name = "ts_test_glob"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/math/index.ts"),
        "export function add(a: number, b: number): number {\n  return a + b;\n}\n",
    )?;
    fs::write(
        project_path.join("src/math/add.test.ts"),
        r#"
import { describe, expect, test } from "vitest";
import { add } from "./index.ts";

describe("add", () => {
  test("adds numbers", () => {
    expect(add(2, 5)).toBe(7);
  });
});
"#,
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let test_stdout = cargo_test_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure(
        test_stdout.contains("running 1 test") && test_stdout.contains("1 passed"),
        "glob-discovered test did not run",
    )?;
    ensure(
        test_stdout.contains("test result: ok"),
        "generated tests did not pass",
    )?;

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

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
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

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"9\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_resolves_typescript_reexported_object_alias_imports() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src/_lib/defaultLocale"))?;
    fs::create_dir_all(project_path.join("src/locale/en-US"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-reexport-object-alias"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "ts_reexport_object_alias"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/locale/en-US/index.ts"),
        "export const enUS = { code: \"en-US\" };\n",
    )?;
    fs::write(
        project_path.join("src/_lib/defaultLocale/index.ts"),
        "export { enUS as defaultLocale } from \"../../locale/en-US/index.ts\";\n",
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "import { defaultLocale } from \"./_lib/defaultLocale/index.ts\";\n\
console.log(defaultLocale.code);\n",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"en-US\n".to_owned(), "unexpected stdout")?;

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

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
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

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"12\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_resolves_typescript_exported_function_expression_consts() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-exported-fn-expr-const"
version = "0.1.0"

[sources]
entries = ["src/main.ts", "src/stub.ts"]

[output]
target = "./dist"
crate-name = "ts_exported_fn_expr_const"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    // `export const stub = function () { ... }` binds an anonymous function
    // expression to a module name and must lower like `function stub() { ... }`.
    fs::write(
        project_path.join("src/stub.ts"),
        "export const stubThree = function () {\n  return 3;\n};\n",
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "import { stubThree } from './stub';\nconst result = stubThree();\nconsole.log(result);\n",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"3\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_resolves_switch_case_labels_from_imported_string_consts() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-switch-const-labels"
version = "0.1.0"

[sources]
entries = ["src/main.ts", "src/tags.ts"]

[output]
target = "./dist"
crate-name = "ts_switch_const_labels"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    // lodash/es-toolkit compat code labels switch cases with references to
    // module string constants (`case stringTag:`). The dependency module is
    // lowered first, so the importer folds these to literals.
    fs::write(
        project_path.join("src/tags.ts"),
        "export const stringTag = '[object String]';\nexport const numberTag = '[object Number]';\n",
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "import { stringTag, numberTag } from './tags';\nexport function classify(tag: string): number {\n  switch (tag) {\n    case stringTag:\n      return 1;\n    case numberTag:\n      return 2;\n    default:\n      return 0;\n  }\n}\nconst result = classify('[object Number]');\nconsole.log(result);\n",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"2\n".to_owned(), "unexpected stdout")?;

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

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"12\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_follows_typescript_dependency_closure() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src/lib"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-dependency-closure"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "ts_dependency_closure"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/lib/helper.ts"),
        "export function addOne(value: number): number {\n  return value + 1;\n}\n",
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "import { addOne } from './lib/helper';\nconst result = addOne(4);\nconsole.log(result);\n",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"5\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_follows_named_imports_through_parent_index_barrel() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src/add"))?;
    fs::create_dir_all(project_path.join("src/sub"))?;
    fs::create_dir_all(project_path.join("src/fp/_lib"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-parent-index-barrel"
version = "0.1.0"

[sources]
entries = ["src/fp/_lib/main.ts"]

[output]
target = "./dist"
crate-name = "ts_parent_index_barrel"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/add/index.ts"),
        "export function add(value: number): number {\n  return value + 1;\n}\n",
    )?;
    fs::write(
        project_path.join("src/sub/index.ts"),
        "export function sub(value: number): number {\n  return value - 1;\n}\n",
    )?;
    fs::write(
        project_path.join("src/index.ts"),
        "export * from \"./add/index.ts\";\nexport * from \"./sub/index.ts\";\n",
    )?;
    fs::write(
        project_path.join("src/fp/_lib/main.ts"),
        "import { add } from \"../../index.ts\";\nconst result = add(4);\nconsole.log(result);\n",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"5\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_resolves_named_import_from_workspace_package_source_fallback() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::create_dir_all(project_path.join("packages/utility/src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-workspace-package-import"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "ts_workspace_package_import"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("package.json"),
        "{\"name\":\"workspace-root\",\"private\":true}\n",
    )?;
    fs::write(
        project_path.join("packages/utility/package.json"),
        "{\"name\":\"utility\",\"main\":\"dist/index.js\",\"module\":\"dist/index.js\"}\n",
    )?;
    fs::write(
        project_path.join("packages/utility/src/index.ts"),
        "export * from \"./increment\";\n",
    )?;
    fs::write(
        project_path.join("packages/utility/src/increment.ts"),
        "export function increment(value: number): number {\n  return value + 1;\n}\n",
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "import { increment } from \"utility\";\nconsole.log(increment(4));\n",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"5\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_dependency_closure_ignores_typescript_type_only_edges() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-type-only-closure"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "ts_type_only_closure"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/types.ts"),
        "export type OnlyType = number;\nimport { missing } from './missing-runtime';\n",
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "import type { OnlyType } from './types';\nconst result = 7;\nconsole.log(result);\n",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"7\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_orders_multiline_typescript_type_import_dependencies() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-multiline-type-import-order"
version = "0.1.0"

[sources]
entries = ["src/main.ts", "src/types.ts"]

[output]
target = "./dist"
crate-name = "ts_multiline_type_import_order"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/types.ts"),
        "export interface Options { value?: number; }\n",
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        r#"import type {
  Options,
} from "./types";

export function read(options?: Options): number {
  return options?.value ?? 0;
}
"#,
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "check"])?;

    Ok(())
}

#[test]
fn build_resolves_cyclic_type_import_inherited_option_fields() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src/_lib/defaultOptions"))?;
    fs::create_dir_all(project_path.join("src/locale"))?;
    fs::create_dir_all(project_path.join("src/start"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-cyclic-type-option-fields"
version = "0.1.0"

[sources]
entries = [
  "src/_lib/defaultOptions/index.ts",
  "src/start/index.ts",
  "src/locale/types.ts",
  "src/types.ts",
]

[output]
target = "./dist"
crate-name = "ts_cyclic_type_option_fields"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/types.ts"),
        r#"import type { Locale } from "./locale/types.ts";
export type * from "./locale/types.ts";

export interface WeekOptions {
  weekStartsOn?: 0 | 1;
}

export interface FirstWeekContainsDateOptions {
  firstWeekContainsDate?: 1;
}

export interface LocalizedOptions<LocaleFields extends keyof Locale> {
  locale?: Pick<Locale, LocaleFields>;
}
"#,
    )?;
    fs::write(
        project_path.join("src/locale/types.ts"),
        r#"import type { LocalizedOptions, WeekOptions } from "../types.ts";

export interface Locale {
  options?: LocaleOptions;
}

export interface LocaleOptions
  extends WeekOptions,
    LocalizedOptions<"options"> {}
"#,
    )?;
    fs::write(
        project_path.join("src/_lib/defaultOptions/index.ts"),
        r#"import type {
  FirstWeekContainsDateOptions,
  Locale,
  LocalizedOptions,
  WeekOptions,
} from "../../types.ts";

export type DefaultOptions = LocalizedOptions<keyof Locale> &
  WeekOptions &
  FirstWeekContainsDateOptions;

let defaultOptions: DefaultOptions = {};

export function getDefaultOptions(): DefaultOptions {
  return defaultOptions;
}
"#,
    )?;
    fs::write(
        project_path.join("src/start/index.ts"),
        r#"import { getDefaultOptions } from "../_lib/defaultOptions/index.ts";
import type { LocalizedOptions, WeekOptions } from "../types.ts";

export interface StartOptions extends LocalizedOptions<"options">, WeekOptions {}

export function read(options?: StartOptions): number {
  const defaultOptions = getDefaultOptions();
  return options?.weekStartsOn ??
    options?.locale?.options?.weekStartsOn ??
    defaultOptions.weekStartsOn ??
    defaultOptions.locale?.options?.weekStartsOn ??
    0;
}
"#,
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "check"])?;

    Ok(())
}

#[test]
fn build_dependency_closure_allows_typescript_import_cycles() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-import-cycle"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "ts_import_cycle"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/a.ts"),
        "import './b';\nexport function aValue(): number {\n  return 4;\n}\n",
    )?;
    fs::write(project_path.join("src/b.ts"), "import './a';\n")?;
    fs::write(
        project_path.join("src/main.ts"),
        "import { aValue } from './a';\nconst result = aValue();\nconsole.log(result);\n",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"4\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_runs_construct_signature_slot_construction() -> TestResult {
    // A constructor-only interface (`interface CounterConstructor { new ():
    // Counter }`) is a typed constructor slot. A class value passed where the
    // slot is expected is adapted into a constructor closure, and `new ctor()`
    // constructs the concrete `Counter` — the generated Rust must compile and
    // run, keeping the concrete constructed type end to end (issue #54).
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "construct-slot"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "construct_slot"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        r#"class Counter {
  value: number = 7;
}

interface CounterConstructor {
  new (): Counter;
}

function make(ctor: CounterConstructor): Counter {
  return new ctor();
}

const counter = make(Counter);
console.log(counter.value);
"#,
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"7\n".to_owned(), "unexpected stdout")?;

    Ok(())
}
