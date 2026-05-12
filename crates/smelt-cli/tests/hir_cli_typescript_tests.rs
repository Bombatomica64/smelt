//! TypeScript-focused integration tests for manifest builds.

mod common;

use std::fs;

use common::{
    TempProject, TestResult, cargo_run_manifest, cargo_test_manifest, ensure, ensure_eq, smelt,
    utf8_path,
};

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
