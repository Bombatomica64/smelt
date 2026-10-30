//! TypeScript-focused integration tests for manifest builds.

mod common;

use std::fs;

use common::{
    TempProject, TestResult, cargo_run_manifest, cargo_run_manifest_with_env, cargo_test_manifest,
    ensure, ensure_eq, smelt,
    smelt_in, smelt_with_env,
    utf8_path,
};

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "end-to-end CLI fixture: the inline project sources and the assertions over the\n             generated output belong to one scenario, and splitting them into helpers would\n             hide what this test actually builds"
)]
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
fn build_lowers_negative_bracket_indexes_as_property_lookups() -> TestResult {
    // Regression for issue #79: a negative (or otherwise out-of-range) bracket
    // index is a JavaScript property lookup, not `.at(...)`. Reads yield
    // `undefined`; a `arr[-1] = x` write sets a string-keyed property that does
    // not change the array's elements, so it is a no-op on the collection while
    // still evaluating its right-hand side. This program exercises array and
    // string reads plus a negative write and asserts the runtime values.
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-negative-bracket"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "ts_negative_bracket"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "const array: number[] = [1, 2, 3];\n\
array[-1] = 99;\n\
const readNeg = array[-1];\n\
const text = \"hi\";\n\
const strNeg = text[-1];\n\
console.log(array.length);\n\
console.log(array[0]);\n\
console.log(readNeg === undefined);\n\
console.log(strNeg === undefined);\n",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(
        &actual_stdout,
        &"3\n1\ntrue\ntrue\n".to_owned(),
        "unexpected stdout",
    )?;

    Ok(())
}

#[test]
fn build_round_trips_class_index_signature_keyed_store() -> TestResult {
    // Issue #84 / #18: a class declaring `[key: string]: T` carries a real
    // runtime keyed store. A dynamic `x[k] = v` write must round-trip to a
    // later `x[k]` read, a missing key must read as `undefined`, and declared
    // named fields must stay concrete alongside the store. This program builds
    // and RUNS the emitted crate, asserting the runtime values rather than only
    // that the code compiles.
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-class-index-store"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "ts_class_index_store"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        r#"class StringBag {
  [key: string]: string;
}

class Counters {
  total: number = 0;
  [key: string]: number;
}

const bag = new StringBag();
bag["a"] = "hello";
bag["a"] = "world";
bag.dotted = "viaDot";
const counters = new Counters();
counters["x"] = 5;
console.log(bag["a"] === "world");
console.log(bag["missing"] === undefined);
console.log(bag.dotted === "viaDot");
console.log(counters["x"] === 5);
console.log(counters["y"] === undefined);
console.log(counters.total);
"#,
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    // 1. computed keyed write round-trips (last write wins): bag["a"] === "world"
    // 2. missing string key reads undefined
    // 3. dotted write/read of an undeclared member round-trips through the store
    // 4. number-value keyed write round-trips: counters["x"] === 5
    // 5. missing number key reads undefined
    // 6. declared named field stays concrete: counters.total === 0
    ensure_eq(
        &actual_stdout,
        &"true\ntrue\ntrue\ntrue\ntrue\n0\n".to_owned(),
        "class index-signature keyed store did not round-trip at runtime",
    )?;

    Ok(())
}

#[test]
fn build_round_trips_optional_class_field() -> TestResult {
    // Issue #97 / #18: an optional class field (`y?: number`) lowers to a real
    // `Option<T>` slot with explicit construction. Constructing with the field
    // present stores `Some(value)`; omitting it stores `None`. Reading the field
    // back through `??` observes the stored value or the fallback. The required
    // field stays concrete. This program builds and RUNS the emitted crate,
    // asserting the runtime values rather than only that the code compiles.
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-optional-class-field"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "ts_optional_class_field"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        r"class Point {
  x: number;
  y?: number;
  constructor(x: number, y?: number) {
    this.x = x;
    this.y = y;
  }
  readY(): number {
    return this.y ?? -1;
  }
}

const present = new Point(1, 2);
const absent = new Point(3);
console.log(present.x);
console.log(present.readY());
console.log(absent.x);
console.log(absent.readY());
",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    // 1. required field stays concrete: present.x === 1
    // 2. present optional field round-trips: present.y ?? -1 === 2
    // 3. required field on the field-omitting instance: absent.x === 3
    // 4. absent optional field reads as the `??` fallback: absent.y ?? -1 === -1
    ensure_eq(
        &actual_stdout,
        &"1\n2\n3\n-1\n".to_owned(),
        "optional class field did not round-trip at runtime",
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
fn build_runs_node_buffer_identity_and_length_program() -> TestResult {
    // End-to-end compile + run coverage for the modeled Node `Buffer`:
    // construction (`Buffer.from`/`Buffer.alloc`/`Buffer.concat`/`new Buffer`),
    // `Buffer.isBuffer`, `instanceof Buffer` (buffer vs non-buffer), and the
    // observable `.length` / `.byteLength` byte-count reads.
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "node-buffer-demo"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "node_buffer_demo"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        r#"const fromArray = Buffer.from([1, 2, 3]);
const allocated = Buffer.alloc(4);
const combined = Buffer.concat([fromArray, allocated]);
const built = new Buffer([9, 8]);

console.log(Buffer.isBuffer(fromArray));
console.log(Buffer.isBuffer("nope"));
console.log(Buffer.isBuffer([1, 2, 3]));
console.log(fromArray instanceof Buffer);
console.log(built instanceof Buffer);
console.log(Buffer.isBuffer(combined));
console.log(fromArray.length);
console.log(allocated.byteLength);
"#,
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(
        &actual_stdout,
        &"true\nfalse\nfalse\ntrue\ntrue\ntrue\n3\n4\n".to_owned(),
        "unexpected Buffer program stdout",
    )?;

    Ok(())
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "end-to-end CLI fixture: the inline project sources and the assertions over the\n             generated output belong to one scenario, and splitting them into helpers would\n             hide what this test actually builds"
)]
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

/// `toThrow(expected)` compares the thrown error with vitest's rules: a string
/// is a substring of the message, a `RegExp` tests it, a class is an
/// `instanceof`. The `.not` forms only pass when the comparison really runs:
/// a matcher that accepted any throw would fail every `.not.toThrow(other)`
/// below, because each callback does throw.
#[test]
fn build_runs_typescript_to_throw_expected_error_comparison() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-to-throw-message"
version = "0.1.0"

[sources]
roots = ["src"]
entries = ["src/index.ts"]
test-prefix = ["**/*.test.ts"]

[output]
target = "./dist"
crate-name = "ts_to_throw_message"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/index.ts"),
        r#"
export class PathError extends Error {}

export function failPath(path: string): void {
  throw new PathError("unsupported path: " + path);
}

export function failPlain(): void {
  throw new Error("plain failure");
}
"#,
    )?;
    fs::write(
        project_path.join("src/index.test.ts"),
        r#"
import { describe, expect, test } from "vitest";
import { PathError, failPath, failPlain } from "./index.ts";

describe("toThrow", () => {
  test("string is a substring of the message", () => {
    expect(() => failPath("/a")).toThrow("unsupported path");
    expect(() => failPath("/a")).not.toThrow("another message");
  });
  test("regexp tests the message", () => {
    expect(() => failPlain()).toThrowError(/plain fail/);
    expect(() => failPlain()).not.toThrowError(/^unsupported/);
  });
  test("class is instanceof", () => {
    expect(() => failPath("/b")).toThrow(PathError);
    expect(() => failPlain()).not.toThrow(PathError);
  });
});
"#,
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let test_stdout = cargo_test_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure(
        test_stdout.contains("3 passed") && test_stdout.contains("test result: ok"),
        "toThrow expected-error comparisons did not all pass",
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

/// A workspace package resolves the same when the manifest path is RELATIVE.
///
/// Run from inside the project (`smelt --manifest-path Smelt.toml`), importer
/// paths are relative and the package search root used to become the empty
/// path, so `read_dir("")` failed and the package was reported unresolved.
#[test]
fn check_resolves_workspace_package_with_a_relative_manifest_path() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::create_dir_all(project_path.join("packages/utility/src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-workspace-relative-manifest"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "ts_workspace_relative_manifest"

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(project_path.join("package.json"), "{\"name\":\"workspace-root\",\"private\":true}\n")?;
    fs::write(
        project_path.join("packages/utility/package.json"),
        "{\"name\":\"utility\",\"source\":\"src/index.ts\"}\n",
    )?;
    fs::write(
        project_path.join("packages/utility/src/index.ts"),
        "export function increment(value: number): number {\n  return value + 1;\n}\n",
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "import { increment } from \"utility\";\nconsole.log(increment(4));\n",
    )?;

    smelt_in(project_path, &["--manifest-path", "Smelt.toml", "check"])?;

    Ok(())
}

/// Write a small library: a barrel, a module it re-exports, a subpath module,
/// and a module the manifest excludes (it imports a package nothing provides,
/// so reaching it would fail the build).
fn write_package_sources(root: &std::path::Path) -> TestResult {
    fs::create_dir_all(root.join("src/extra"))?;
    fs::write(root.join("src/index.ts"), "export * from \"./increment\";\nexport * from \"./client\";\n")?;
    fs::write(
        root.join("src/increment.ts"),
        "export function increment(value: number): number {\n  return value + 1;\n}\n",
    )?;
    fs::write(
        root.join("src/client.ts"),
        "import { connect } from \"no-such-package\";\nexport const client = connect();\n",
    )?;
    fs::write(
        root.join("src/extra/index.ts"),
        "export function twice(value: number): number {\n  return value * 2;\n}\n",
    )?;
    Ok(())
}

/// The manifest and program shared by the `[packages]` tests.
fn write_package_consumer(project_path: &std::path::Path, package_decl: &str) -> TestResult {
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        format!(
            r#"[project]
name = "ts-declared-package"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[packages.utility]
{package_decl}
exclude = ["src/client.ts"]

[output]
target = "./dist"
crate-name = "ts_declared_package"
build = true

[runtime]
clone-strategy = "aggressive"
"#
        ),
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "import { increment } from \"utility\";\nimport { twice } from \"utility/extra\";\nconsole.log(twice(increment(4)));\n",
    )?;
    Ok(())
}

/// A bare import resolves through `[packages.<name>] path`, including a
/// subpath next to the entry, and the package's own excludes prune its tree.
#[test]
fn build_resolves_a_declared_package_from_a_local_path() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    write_package_sources(&project_path.join("vendor/utility"))?;
    write_package_consumer(project_path, "path = \"vendor/utility\"")?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"10\n".to_owned(), "unexpected stdout")?;
    Ok(())
}

/// `[packages.<name>] git` + `rev` fetches the pinned revision into the
/// package cache once and reuses it; a later commit on the branch is not seen.
#[test]
fn build_resolves_a_declared_package_from_a_pinned_git_revision() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    let repo = project_path.join("upstream");
    write_package_sources(&repo)?;
    let git = |args: &[&str]| -> TestResult {
        let status = std::process::Command::new("git")
            .current_dir(&repo)
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main"])
            .args(args)
            .status()?;
        ensure(status.success(), format!("git {args:?} failed"))
    };
    git(&["init", "--quiet"])?;
    git(&["add", "."])?;
    git(&["commit", "--quiet", "-m", "v1"])?;
    git(&["tag", "v1"])?;
    // A later commit that would change the output if it were picked up.
    fs::write(
        repo.join("src/increment.ts"),
        "export function increment(value: number): number {\n  return value + 100;\n}\n",
    )?;
    git(&["commit", "--quiet", "-am", "v2"])?;

    let url = format!("file://{}", utf8_path(&repo)?);
    write_package_consumer(project_path, &format!("git = \"{url}\"\nrev = \"v1\""))?;
    let cache = project_path.join("cache");
    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    for _ in 0_u8..2 {
        smelt_with_env(&["--manifest-path", &manifest_arg, "build"], &[("SMELT_PACKAGE_CACHE", &cache)])?;
        let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
        ensure_eq(&actual_stdout, &"10\n".to_owned(), "unexpected stdout")?;
    }
    ensure(cache.join("utility-v1/.git").is_dir(), "the pinned checkout is cached under <name>-<rev>")?;
    Ok(())
}

/// `process.env.NAME` reads the real environment at run time, so a server
/// sees its `PORT`; the profile's timezone stays fixed whatever `TZ` says.
#[test]
fn process_env_reads_the_environment_at_run_time() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-process-env"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "ts_process_env"

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "const port = Number(process.env.SMELT_TEST_PORT ?? 3000);\nconsole.log(port, process.env.TZ ?? \"UTC\");\n",
    )?;
    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let manifest = project_path.join("dist/Cargo.toml");
    let unset = cargo_run_manifest_with_env(&manifest, &[("TZ", "UTC")], &["SMELT_TEST_PORT"])?;
    ensure_eq(&unset, &"3000 America/Santiago\n".to_owned(), "an unset variable is undefined")?;
    let set = cargo_run_manifest_with_env(&manifest, &[("SMELT_TEST_PORT", "4321"), ("TZ", "UTC")], &[])?;
    ensure_eq(&set, &"4321 America/Santiago\n".to_owned(), "a set variable is read")?;
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
#[expect(
    clippy::too_many_lines,
    reason = "end-to-end CLI fixture: the inline project sources and the assertions over the\n             generated output belong to one scenario, and splitting them into helpers would\n             hide what this test actually builds"
)]
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
fn build_predeclares_union_generator_methods_across_barrel_cycle() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-barrel-cycle-generator-types"
version = "0.1.0"

[sources]
entries = ["src/index.ts"]

[output]
target = "./dist"
crate-name = "ts_barrel_cycle_generator_types"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/result.ts"),
        r#"import type { ResultAsync } from "./index";

export class Ok<T> {
  constructor(readonly value: T) {}
  safeUnwrap(): Generator<number, T, unknown> {
    const value = this.value;
    return (function* (): Generator<number, T, unknown> { return value; })();
  }
}

export class Err<T> {
  constructor(readonly value: T) {}
  safeUnwrap(): Generator<number, T, unknown> {
    const value = this.value;
    return (function* (): Generator<number, T, unknown> { return value; })();
  }
}

export type Result<T> = Ok<T> | Err<T>;
"#,
    )?;
    fs::write(
        project_path.join("src/result-async.ts"),
        r#"import type { Result } from "./index";

export class ResultAsync<T> {
  constructor(readonly promise: Promise<Result<T>>) {}

  async *safeUnwrap(): AsyncGenerator<number, T, unknown> {
    return yield* await this.promise.then((result) => result.safeUnwrap());
  }
}
"#,
    )?;
    fs::write(
        project_path.join("src/index.ts"),
        "export { Ok, Err } from './result';\nexport type { Result } from './result';\nexport { ResultAsync } from './result-async';\n",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

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
        r"class Counter {
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
",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"7\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

/// `dump-hir` on a single file now lowers that file's local relative-import
/// dependencies first, so a `switch` whose case labels reference *imported*
/// string constants (es-toolkit's `case argumentsTag:` pattern, where the tag
/// values live in a sibling module) folds instead of failing with
/// `switch case labels must be string, number, boolean, or null literals`.
///
/// This exercises the isolation-only regression that previously made `dump-hir`
/// and `probe` report a blocker a full manifest build never hit.
#[test]
fn dump_hir_folds_imported_const_switch_labels_from_sibling_module() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("src/tags.ts"),
        r"export const arrayTag = '[object Array]';
export const objectTag = '[object Object]';
",
    )?;
    fs::write(
        project_path.join("src/classify.ts"),
        r"import { arrayTag, objectTag } from './tags';

export function isContainerTag(tag: string): boolean {
  switch (tag) {
    case arrayTag:
    case objectTag:
      return true;
    default:
      return false;
  }
}
",
    )?;

    let importer_arg = utf8_path(&project_path.join("src/classify.ts"))?;
    // A successful dump-hir (a non-zero exit would make `smelt` return `Err`)
    // proves the imported-const case labels folded rather than erroring.
    let stdout = smelt(&["dump-hir", &importer_arg])?;
    ensure(
        stdout.contains("classify.ts"),
        "missing importer module header in dump-hir output",
    )?;
    ensure(
        stdout.contains("fn is_container_tag"),
        "importer function item missing from dump-hir output",
    )?;
    ensure(
        !stdout.contains("switch case labels must be"),
        "imported const switch labels still reported as a blocker",
    )?;

    Ok(())
}

/// A temporary project and the path of its `Smelt.toml`, or why it could not be written.
type ManifestProject = Result<(TempProject, String), Box<dyn std::error::Error>>;

/// Write a manifest-built project whose files are `files` (path, source),
/// entered from `src/main.ts`, and return it with its manifest path.
fn multi_module_project(
    name: &str,
    build: bool,
    files: &[(&str, &str)],
) -> ManifestProject {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    let crate_name = name.replace('-', "_");
    fs::write(
        project_path.join("Smelt.toml"),
        format!(
            "[project]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"src/main.ts\"]\n\n\
[output]\ntarget = \"./dist\"\ncrate-name = \"{crate_name}\"\nbuild = {build}\n\n\
[runtime]\nclone-strategy = \"aggressive\"\n"
        ),
    )?;
    for (path, source) in files {
        fs::write(project_path.join(path), source)?;
    }
    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    Ok((project, manifest_arg))
}

/// The module sources of [`build_types_cycle_early_reads_of_a_pending_class`].
///
/// `main.ts` imports `holder.ts` FIRST, so the depth-first manifest order is
/// `constants, app, types, holder, main`: `app.ts` lowers before the class it
/// reads (`holder -> types -(type-only)-> app -> holder` is a cycle), exactly
/// the shape Hono v5's `hono-base.ts -> context.ts` takes. Node prints
/// `205 417`.
const CYCLE_EARLY_FILES: &[(&str, &str)] = &[
    (
        "src/constants.ts",
        "export const GET_VALUE: unique symbol = Symbol()\n",
    ),
    (
        "src/types.ts",
        "import type { App } from './app'\nexport type AppLike = App<string>\n",
    ),
    (
        "src/holder.ts",
        r"import { GET_VALUE } from './constants'
import type { AppLike } from './types'

interface TextRespond {
  (text: string, status?: number): number
}

export class Holder<E = string> {
  #res: Response | undefined = new Response('hi', { status: 201 })
  owner: AppLike | undefined
  finalized: boolean = false
  get label(): string {
    return 'lbl'
  }
  get text(): TextRespond {
    return (text: string, status?: number): number => text.length + (status ?? 0)
  }
  get [GET_VALUE](): Response | undefined {
    return this.#res
  }
}
",
    ),
    (
        "src/app.ts",
        r"import { GET_VALUE } from './constants'
import type { Holder } from './holder'

const notFound = (c: Holder<number>): number => c.text('404 Not Found', 404)

export class App<E = string> {
  #dispatchInternal(method: string, c: Holder<E>): number {
    const label = c.label
    const done = c.finalized
    const res = c[GET_VALUE]
    if (res && !done) {
      const copy = new Response(res.body, res)
      return copy.status + label.length + c.text('x')
    }
    return method.length
  }
  run(c: Holder<E>): number {
    return this.#dispatchInternal('GET', c)
  }
  miss(c: Holder<number>): number {
    return notFound(c)
  }
}
",
    ),
    (
        "src/main.ts",
        r"import { Holder } from './holder'
import { App } from './app'
const app = new App<number>()
console.log(app.run(new Holder<number>()), app.miss(new Holder<number>()))
",
    ),
];

#[test]
fn build_types_cycle_early_reads_of_a_pending_class() -> TestResult {
    // A module an import cycle lowers BEFORE the class it reads only sees the
    // class's predeclared surface. Behaviour: the program must print what
    // Node prints. Types: the reads must keep their declared types instead of
    // erasing to `SmeltUnknown` — an annotated getter (`label: string`), an
    // annotated data property (`finalized: boolean`), a getter keyed by an
    // IMPORTED unique symbol (`Response | undefined`), and a getter returning a
    // callable interface, called through the getter read rather than as a
    // method the class does not have (that last one used to stop MIR lowering
    // with "class method `text` is not resolvable").
    let (project, manifest) = multi_module_project("cycle-early-reads", false, CYCLE_EARLY_FILES)?;
    smelt(&["--manifest-path", &manifest, "build"])?;
    let generated = fs::read_to_string(project.path().join("dist/src/main.rs"))?;
    for typed in [
        "let label: String = c.__smelt_get_label()",
        "let done: bool = c.finalized",
        "let res: Option<SmeltResponse> = c.__smelt_get___smelt_symbol_unique__at_",
    ] {
        ensure(generated.contains(typed), format!("expected `{typed}` in the cycle-early module"))?;
    }
    ensure(
        generated.contains("c.__smelt_get_text().clone().__smelt_call"),
        "the callable-interface getter must be read, then called",
    )?;
    let (project, manifest) = multi_module_project("cycle-early-reads", true, CYCLE_EARLY_FILES)?;
    smelt(&["--manifest-path", &manifest, "build"])?;
    let stdout = cargo_run_manifest(&project.path().join("dist/Cargo.toml"))?;
    ensure_eq(&stdout, &"205 417\n".to_owned(), "unexpected stdout")?;
    Ok(())
}

#[test]
fn build_keeps_same_offset_unique_symbols_of_two_modules_distinct() -> TestResult {
    // `KEY_A` and `KEY_B` are `Symbol()` calls at the SAME byte offset of two
    // different modules. They are two symbols (Node prints `1 2 false`), so
    // they must name two members; a site identity of offset alone gave both
    // getters the one name `__smelt_get___smelt_symbol_unique__at_36`.
    let files: &[(&str, &str)] = &[
        ("src/a.ts", "export const KEY_A: unique symbol = Symbol()\n"),
        ("src/b.ts", "export const KEY_B: unique symbol = Symbol()\n"),
        (
            "src/main.ts",
            r"import { KEY_A } from './a'
import { KEY_B } from './b'

class Pair {
  get [KEY_A](): number {
    return 1
  }
  get [KEY_B](): number {
    return 2
  }
}

const p = new Pair()
console.log(p[KEY_A], p[KEY_B], KEY_A === KEY_B)
",
        ),
    ];
    let (project, manifest) = multi_module_project("symbol-site-identity", true, files)?;
    smelt(&["--manifest-path", &manifest, "build"])?;
    let stdout = cargo_run_manifest(&project.path().join("dist/Cargo.toml"))?;
    ensure_eq(&stdout, &"1 2 false\n".to_owned(), "unexpected stdout")?;
    Ok(())
}

/// A function-valued property of a record lowered to a `Dict` takes ITS OWN
/// field's signature, not the first function type the record mentions.
///
/// The `Dict` joins every field's type into one value union, so a record with
/// two function-typed fields hinted each value with both signatures and the
/// first one won: Hono's `mount(app, { replaceRequest: (req) => req })` typed
/// the arrow `(Context) => Context` and rewrote every request to
/// `about:blank`. The ambiguous hint is now dropped and the arrow types itself
/// through the TS7 checker, which this CLI path embeds. (The options value is
/// an erased union here, so this is pinned as a HIR check rather than as an
/// end-to-end example, whose corpus must stay free of avoidable erasure.)
#[test]
fn record_function_field_takes_its_own_signature() -> TestResult {
    let project = TempProject::new()?;
    fs::create_dir_all(project.path())?;
    let source = project.path().join("options.ts");
    fs::write(
        &source,
        r"class Ctx { name: string; constructor(name: string) { this.name = name } }
class Req { url: string; constructor(url: string) { this.url = url } }
type Handler = (c: Ctx) => unknown
type Rewrite = (r: Req) => Req
type Options = Handler | { optionHandler?: Handler; rewrite?: Rewrite | false }
function use(options?: Options): string {
  if (options && typeof options !== 'function' && options.rewrite) {
    return options.rewrite(new Req('/app')).url
  }
  return 'none'
}
console.log(use({ rewrite: (req) => req }))
",
    )?;
    let stdout = smelt(&["dump-hir", &utf8_path(&source)?])?;
    ensure(
        stdout.contains("fn(Req) -> Req = closure"),
        "`(req) => req` must take `Rewrite`'s signature",
    )?;
    ensure(
        !stdout.contains("fn(Ctx) -> Ctx = closure"),
        "`(req) => req` was typed by the first function field (`Handler`)",
    )
}
