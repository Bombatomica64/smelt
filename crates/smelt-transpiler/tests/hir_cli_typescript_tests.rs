//! TypeScript-focused integration tests for manifest builds.

mod common;

use std::fs;

use common::{
    TempProject, TestResult, cargo_run_manifest, cargo_run_manifest_with_env, cargo_test_manifest,
    ensure, ensure_eq, smelt,
    smelt_in, smelt_with_env, tool_missing,
    utf8_path,
};

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "end-to-end CLI fixture: the inline project sources and the assertions over the\n             generated output belong to one scenario, and splitting them into helpers would\n             hide what this test actually builds"
)]
fn build_specializes_decorators_and_reuses_packaged_manifest() -> TestResult {
    if tool_missing("build_specializes_decorators", &["bwrap", "node", "tsc"])? {
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
        "let label: JsString = c.__smelt_get_label()",
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
fn build_dispatches_abstract_methods_of_an_imported_abstract_base() -> TestResult {
    // An abstract method is a virtual-dispatch slot on the base class. Called
    // through a base-typed receiver in ANOTHER module, the slot used to be
    // invisible (the per-module class registry holds no fields for imported
    // classes), so `setter.set(..)` lowered as a direct method call and MIR
    // stopped with "class method `set` is not resolvable" (date-fns `parse`
    // over `parse/_lib/Setter.ts`). Behaviour: each subclass's override runs
    // in list order, and Node prints `10 1,1,1`. Types: the list stays
    // `SmeltList<Setter>`, not an erased carrier.
    let files: &[(&str, &str)] = &[
        (
            "src/setter.ts",
            r"export abstract class Setter {
  public abstract priority: number;
  public subPriority = 0;
  public validate(_value: number): boolean {
    return true;
  }
  public abstract set(value: number): number;
}
export class AddSetter extends Setter {
  public priority = 1;
  constructor(private amount: number) {
    super();
  }
  public set(value: number): number {
    return value + this.amount;
  }
}
export class DoubleSetter extends Setter {
  public priority = 2;
  override subPriority = -1;
  public set(value: number): number {
    return value * 2;
  }
}
",
        ),
        (
            "src/main.ts",
            r"import { Setter, AddSetter, DoubleSetter } from './setter'

const setters: Setter[] = [new DoubleSetter(), new AddSetter(3), new DoubleSetter()]
let value = 1
for (const setter of setters) {
  if (setter.validate(value)) value = setter.set(value)
}
console.log(value, setters.map((setter) => setter.priority + setter.subPriority).join(','))
",
        ),
    ];
    let (project, manifest) = multi_module_project("imported-abstract-dispatch", true, files)?;
    smelt(&["--manifest-path", &manifest, "build"])?;
    let generated = fs::read_to_string(project.path().join("dist/src/main.rs"))?;
    ensure(
        generated.contains("SmeltList<Setter>"),
        "the base-typed list must stay a list of the base class",
    )?;
    let stdout = cargo_run_manifest(&project.path().join("dist/Cargo.toml"))?;
    ensure_eq(&stdout, &"10 1,1,1\n".to_owned(), "unexpected stdout")?;
    Ok(())
}

/// Every generated `.rs` file under a built project's `dist/src`, concatenated,
/// so a signature assertion does not depend on which module a function lands in.
fn generated_rust_text(project: &std::path::Path) -> TestResult<String> {
    let mut text = String::new();
    for entry in fs::read_dir(project.join("dist/src"))? {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| extension == "rs") {
            text.push_str(&fs::read_to_string(path)?);
        }
    }
    Ok(text)
}

/// The emitted signature line of free function `name` (`fn name<..>(..) -> ..`).
fn emitted_signature(generated: &str, name: &str) -> TestResult<String> {
    let needle = format!("fn {name}");
    generated
        .lines()
        .find(|line| {
            line.find(&needle).is_some_and(|at| {
                matches!(generated_after(line, at.saturating_add(needle.len())), Some('<' | '('))
            })
        })
        .map(str::to_owned)
        .ok_or_else(|| std::io::Error::other(format!("no `fn {name}` in:\n{generated}")).into())
}

/// The character right after byte `at` of `line`, if any.
fn generated_after(line: &str, at: usize) -> Option<char> {
    line.get(at..).and_then(|rest| rest.chars().next())
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "end-to-end CLI fixture: a two-module program and its assertions are one scenario"
)]
fn build_dispatches_class_bounded_members_through_a_generated_trait() -> TestResult {
    // Bounded-generics plan, increments 1-3. A method call (`a.speak()`), a
    // field read (`a.legs`) and an inherited method with an argument
    // (`a.describe(prefix)`) on `A extends Animal` go through a generated
    // `AnimalLike` trait implemented by `Animal` and every subclass, declared in
    // another module. Behaviour: each receiver's own override runs — Node
    // prints `woof 4`, `yip 3`, `... 2`, then `3 1 p:yip/2 a:.../3` (the
    // instantiation keeps the concrete `Dog`/`Puppy`, so `.age` reads back).
    // Types: both functions lift to `A: AnimalLike + ..` instead of the
    // `SmeltUnknown` signature every bounded parameter had before.
    let files: &[(&str, &str)] = &[
        (
            "src/animals.ts",
            r#"export class Animal {
  constructor(public legs: number, public age: number) {}
  speak(): string {
    return "...";
  }
  describe(prefix: string): string {
    return prefix + ":" + this.speak() + "/" + this.legs;
  }
}
export class Dog extends Animal {
  speak(): string {
    return "woof";
  }
}
export class Puppy extends Dog {
  speak(): string {
    return "yip";
  }
}
"#,
        ),
        (
            "src/main.ts",
            r"import { Animal, Dog, Puppy } from './animals'

function loud<A extends Animal>(a: A): A {
  console.log(a.speak(), a.legs)
  return a
}
function tag<A extends Animal>(a: A, prefix: string): string {
  return a.describe(prefix)
}

const dog = loud(new Dog(4, 3))
const puppy = loud(new Puppy(3, 1))
loud(new Animal(2, 9))
console.log(dog.age, puppy.age, tag(new Puppy(2, 0), 'p'), tag(new Animal(3, 9), 'a'))
",
        ),
    ];
    let (project, manifest) = multi_module_project("class-bound-trait", true, files)?;
    smelt(&["--manifest-path", &manifest, "build"])?;
    let generated = generated_rust_text(project.path())?;
    for name in ["loud", "tag"] {
        let signature = emitted_signature(&generated, name)?;
        ensure(
            signature.contains("<A: AnimalLike + ") && !signature.contains(": SmeltUnknown") && !signature.contains("-> SmeltUnknown"),
            format!("`{name}` must lift with the bound's trait, got `{signature}`"),
        )?;
    }
    for implementor in ["Animal", "Dog", "Puppy"] {
        ensure(
            generated.contains(&format!("impl AnimalLike for {implementor} {{")),
            format!("`{implementor}` must implement the bound trait"),
        )?;
    }
    let stdout = cargo_run_manifest(&project.path().join("dist/Cargo.toml"))?;
    ensure_eq(
        &stdout,
        &"woof 4\nyip 3\n... 2\n3 1 p:yip/2 a:.../3\n".to_owned(),
        "unexpected stdout",
    )?;
    Ok(())
}

#[test]
fn build_lifts_an_interface_bound_called_with_record_literals_and_implementors() -> TestResult {
    // An interface bound: `L extends HasLen` reads a field and calls a method
    // signature through `HasLenLike`, implemented by the interface's own struct
    // (what an object literal of exactly that shape becomes) and by a class
    // declaring `implements HasLen`. Node prints `2 two lit#5 rope7#7` and
    // `rope9`; the literal arguments keep their own closures and the class
    // argument its own method. Types: `L: HasLenLike + ..`, and the result of
    // `longest` over literals is the interface type, not a string-keyed record.
    let files: &[(&str, &str)] = &[(
        "src/main.ts",
        r"interface HasLen {
  len: number
  label(): string
}
class Rope implements HasLen {
  constructor(public len: number) {}
  label(): string {
    return 'rope' + this.len
  }
}

function longest<L extends HasLen>(a: L, b: L): L {
  return a.len >= b.len ? a : b
}
function labelOf<L extends HasLen>(item: L): string {
  return item.label() + '#' + item.len
}

const picked = longest({ len: 1, label: () => 'one' }, { len: 2, label: () => 'two' })
console.log(picked.len, picked.label(), labelOf({ len: 5, label: () => 'lit' }), labelOf(new Rope(7)))
console.log(longest(new Rope(3), new Rope(9)).label())
",
    )];
    let (project, manifest) = multi_module_project("interface-bound-trait", true, files)?;
    smelt(&["--manifest-path", &manifest, "build"])?;
    let generated = generated_rust_text(project.path())?;
    for name in ["longest", "label_of"] {
        let signature = emitted_signature(&generated, name)?;
        ensure(
            signature.contains("<L: HasLenLike + ") && !signature.contains(": SmeltUnknown") && !signature.contains("-> SmeltUnknown"),
            format!("`{name}` must lift with the bound's trait, got `{signature}`"),
        )?;
    }
    ensure(
        generated.contains("impl HasLenLike for HasLen {")
            && generated.contains("impl HasLenLike for Rope {"),
        "the interface struct and the implementing class must implement the trait",
    )?;
    let stdout = cargo_run_manifest(&project.path().join("dist/Cargo.toml"))?;
    ensure_eq(&stdout, &"2 two lit#5 rope7#7\nrope9\n".to_owned(), "unexpected stdout")?;
    Ok(())
}

#[test]
fn build_returns_the_bounded_type_param_chosen_through_its_bound() -> TestResult {
    // A bounded generic returning its own type parameter, chosen by fields read
    // through the bound. Returning `A` keeps the caller's concrete instantiation:
    // the `Dog` that wins still speaks `woof`. Node prints `3 woof 1`. Types:
    // `fn older<A: AnimalLike + ..>(a: A, b: A) -> A`.
    let files: &[(&str, &str)] = &[(
        "src/main.ts",
        r"class Animal {
  constructor(public legs: number, public age: number) {}
  speak(): string {
    return '...'
  }
}
class Dog extends Animal {
  speak(): string {
    return 'woof'
  }
}

function older<A extends Animal>(a: A, b: A): A {
  return a.age >= b.age ? a : b
}

const winner = older(new Dog(4, 5), new Dog(3, 7))
console.log(winner.legs, winner.speak(), older(new Animal(1, 2), new Animal(2, 1)).legs)
",
    )];
    let (project, manifest) = multi_module_project("bound-returns-param", true, files)?;
    smelt(&["--manifest-path", &manifest, "build"])?;
    let generated = generated_rust_text(project.path())?;
    let signature = emitted_signature(&generated, "older")?;
    ensure(
        signature.contains("<A: AnimalLike + ") && signature.contains("(a: A, b: A) -> A"),
        format!("`older` must lift and return `A`, got `{signature}`"),
    )?;
    let stdout = cargo_run_manifest(&project.path().join("dist/Cargo.toml"))?;
    ensure_eq(&stdout, &"3 woof 1\n".to_owned(), "unexpected stdout")?;
    Ok(())
}

#[test]
fn build_lifts_a_bounded_generic_that_forwards_its_parameter_to_another() -> TestResult {
    // `twice<A extends Animal>` passes its own `A` to `loud<A extends Animal>`.
    // Both lift together: the forwarded argument is a real generic carrying the
    // same trait, which `EmitContext::populate_generic_functions` verifies after
    // the body trials (a caller that erased would demote the callee too). Node
    // prints `yip 4` twice, `... 2` twice, then `yip+yip ...+...`.
    let files: &[(&str, &str)] = &[(
        "src/main.ts",
        r"class Animal {
  constructor(public legs: number) {}
  speak(): string {
    return '...'
  }
}
class Puppy extends Animal {
  speak(): string {
    return 'yip'
  }
}

function loud<A extends Animal>(a: A): A {
  console.log(a.speak(), a.legs)
  return a
}
function twice<A extends Animal>(a: A): string {
  return loud(a).speak() + '+' + loud(a).speak()
}

console.log(twice(new Puppy(4)), twice(new Animal(2)))
",
    )];
    let (project, manifest) = multi_module_project("bound-forwarding", true, files)?;
    smelt(&["--manifest-path", &manifest, "build"])?;
    let generated = generated_rust_text(project.path())?;
    for name in ["loud", "twice"] {
        let signature = emitted_signature(&generated, name)?;
        ensure(
            signature.contains("<A: AnimalLike + ") && !signature.contains(": SmeltUnknown") && !signature.contains("-> SmeltUnknown"),
            format!("`{name}` must lift with the bound's trait, got `{signature}`"),
        )?;
    }
    let stdout = cargo_run_manifest(&project.path().join("dist/Cargo.toml"))?;
    ensure_eq(
        &stdout,
        &"yip 4\nyip 4\n... 2\n... 2\nyip+yip ...+...\n".to_owned(),
        "unexpected stdout",
    )?;
    Ok(())
}

#[test]
fn build_keeps_the_erased_fallback_for_a_write_through_a_bound() -> TestResult {
    // A field WRITE through a class bound has no trait setter, so the parameter
    // keeps the erased signature and its dynamic member write — and the write
    // still reaches the caller's object through the erased view's write-through,
    // as in Node (`5 5`). The program has no `unknown` of its own any more (the
    // reads are typed through the bound); MIR interns it for the erased path
    // (`intern_bound_dispatch_carrier_type`), without which this build failed
    // with "type table does not contain literal operand type Unknown".
    let files: &[(&str, &str)] = &[(
        "src/main.ts",
        r"class Animal {
  constructor(public legs: number) {}
  rest(): void {
    this.legs = 0
  }
}

function grow<A extends Animal>(a: A): number {
  a.legs = a.legs + 1
  return a.legs
}

const animal = new Animal(4)
console.log(grow(animal), animal.legs)
",
    )];
    let (project, manifest) = multi_module_project("bound-write-fallback", true, files)?;
    smelt(&["--manifest-path", &manifest, "build"])?;
    let generated = generated_rust_text(project.path())?;
    let signature = emitted_signature(&generated, "grow")?;
    ensure(
        signature.contains("SmeltUnknown") && !signature.contains("AnimalLike"),
        format!("`grow` must keep the erased signature, got `{signature}`"),
    )?;
    let stdout = cargo_run_manifest(&project.path().join("dist/Cargo.toml"))?;
    ensure_eq(&stdout, &"5 5\n".to_owned(), "unexpected stdout")?;
    Ok(())
}

#[test]
fn build_shares_a_date_between_aliases_and_compares_it_by_identity() -> TestResult {
    // Bounded-generics plan, increment 4: `Date` is the shared handle
    // `SmeltDate`, not an erased record. Behaviour (Node prints the three lines
    // below): a setter through one alias is observed through the other
    // (`bump(b)` moves `a` to 86400000), `===` and `==` compare identity (a
    // copy made by `new Date(a)` or `new Date(a.getTime())` is a different
    // date, and mutating the copy leaves `a` alone), `<` compares time values,
    // and an Invalid Date stays invalid through a setter. Before this, the
    // setter re-bound only the local, so Smelt printed `0 0 true ..`. Types: a
    // `Date` parameter is `SmeltDate`, never `SmeltUnknown`.
    let files: &[(&str, &str)] = &[(
        "src/main.ts",
        r"function bump(d: Date): void {
  d.setDate(d.getDate() + 1)
}

const a: Date = new Date(0)
const b = a
bump(b)
const copy = new Date(a)
const sameTime = new Date(a.getTime())
copy.setHours(5)
console.log(a.getTime(), b.getTime(), a === b, a == b, a === copy, a === sameTime, sameTime.getTime())
console.log(copy.getTime(), a < copy, a.toISOString())
const bad = new Date(NaN)
bad.setDate(3)
console.log(isNaN(bad.getTime()), String(bad), bad === bad)
",
    )];
    let (project, manifest) = multi_module_project("date-shared-handle", true, files)?;
    smelt(&["--manifest-path", &manifest, "build"])?;
    let generated = generated_rust_text(project.path())?;
    let signature = emitted_signature(&generated, "bump")?;
    ensure(
        signature.contains("d: SmeltDate") && !signature.contains(": SmeltUnknown"),
        format!("a `Date` parameter must be the concrete handle, got `{signature}`"),
    )?;
    let stdout = cargo_run_manifest(&project.path().join("dist/Cargo.toml"))?;
    ensure_eq(
        &stdout,
        &"86400000 86400000 true true false false 86400000\n104400000 true 1970-01-02T00:00:00.000Z\ntrue Invalid Date true\n"
            .to_owned(),
        "unexpected stdout",
    )?;
    Ok(())
}

#[test]
fn build_lifts_a_date_bounded_generic_to_date_like() -> TestResult {
    // `D extends Date` lifts to `D: DateLike` (increment 4): the setter mutates
    // the caller's date in place and the function returns that same date
    // (`start === moved`), an Invalid Date stays invalid through it. Node
    // prints `172800000 172800000 true 1970-01-03T00:00:00.000Z` and
    // `true false false`. Types: both functions declare `D: DateLike + ..` and
    // read the date through its handle, not through an erased member lookup.
    let files: &[(&str, &str)] = &[(
        "src/main.ts",
        r"function addDays<D extends Date>(date: D, amount: number): D {
  date.setDate(date.getDate() + amount)
  return date
}
function isValid<D extends Date>(date: D): boolean {
  return !isNaN(date.getTime())
}

const start = new Date(0)
const moved = addDays(start, 2)
console.log(start.getTime(), moved.getTime(), start === moved, moved.toISOString())
const invalid = new Date(NaN)
console.log(isValid(start), isValid(invalid), isValid(addDays(invalid, 1)))
",
    )];
    let (project, manifest) = multi_module_project("date-bound-trait", true, files)?;
    smelt(&["--manifest-path", &manifest, "build"])?;
    let generated = generated_rust_text(project.path())?;
    for name in ["add_days", "is_valid"] {
        let signature = emitted_signature(&generated, name)?;
        ensure(
            signature.contains("<D: DateLike + ")
                && !signature.contains(": SmeltUnknown")
                && !signature.contains("-> SmeltUnknown"),
            format!("`{name}` must lift with `DateLike`, got `{signature}`"),
        )?;
    }
    ensure(
        generated.contains(".smelt_date().get_time()"),
        "a `D: DateLike` value is read through its date handle",
    )?;
    let stdout = cargo_run_manifest(&project.path().join("dist/Cargo.toml"))?;
    ensure_eq(
        &stdout,
        &"172800000 172800000 true 1970-01-03T00:00:00.000Z\ntrue false false\n".to_owned(),
        "unexpected stdout",
    )?;
    Ok(())
}

/// Every generated line declaring `needle` (a struct field or a method head),
/// trimmed. Several classes may declare the same member, so a check over a
/// member name has to see all of them.
fn generated_lines_with<'a>(generated: &'a str, needle: &str) -> Vec<&'a str> {
    generated
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with(needle))
        .collect()
}

#[test]
fn build_keeps_same_named_type_params_of_two_generic_classes_distinct() -> TestResult {
    // Bounded-generics plan, increment 5: a type parameter is identified by
    // the item that DECLARES it, not by its spelling. `Holder<Value>` and
    // `Parser<Value>` declare two different `Value`s; `NumberParser extends
    // Parser<number>` gets an inherited copy of `run` in which Parser's
    // `Value` is `number`, but `Holder`'s `Value` is only instantiated by the
    // `new Holder<Value>(..)` expression. Interned by spelling, the copy's
    // substitution rewrote both, and the callback adapter declared
    // `Holder`'s `Value` erased: E0631/E0308 in the inherited `run` (12
    // errors). Behaviour: Node prints the two lines below. Types: the
    // subclass's virtual slot takes the concrete `f64`.
    let files: &[(&str, &str)] = &[(
        "src/main.ts",
        r#"class Holder<Value> {
  constructor(
    public value: Value,
    private check: (value: Value) => boolean,
  ) {}
  ok(): boolean {
    return this.check(this.value);
  }
}

abstract class Parser<Value> {
  abstract parse(input: string): { value: Value } | null;
  accept(_value: Value): boolean {
    return true;
  }
  run(input: string): Holder<Value> | null {
    const parsed = this.parse(input);
    if (!parsed) return null;
    return new Holder<Value>(parsed.value, this.accept);
  }
}

class NumberParser extends Parser<number> {
  parse(input: string): { value: number } | null {
    const n = Number(input);
    return isNaN(n) ? null : { value: n };
  }
  accept(value: number): boolean {
    return value >= 0;
  }
}

const words = new Holder<string>("hi", (value) => value.length > 1);
const parser = new NumberParser();
const held = parser.run("42");
const negative = parser.run("-3");
console.log(words.value, words.ok(), held === null ? "none" : held.value + 1, held === null ? false : held.ok());
console.log(negative === null ? "none" : negative.ok(), parser.run("x") === null);
"#,
    )];
    let (project, manifest) = multi_module_project("type-param-identity", true, files)?;
    smelt(&["--manifest-path", &manifest, "build"])?;
    let generated = generated_rust_text(project.path())?;
    let slots = generated_lines_with(&generated, "accept: ::std::rc::Rc<dyn Fn(");
    ensure(
        slots.contains(&"accept: ::std::rc::Rc<dyn Fn(f64) -> bool>,")
            && slots.contains(&"accept: ::std::rc::Rc<dyn Fn(&Value) -> bool>,"),
        format!("the base slot takes `&Value` and the subclass's `f64`, got {slots:?}"),
    )?;
    let stdout = cargo_run_manifest(&project.path().join("dist/Cargo.toml"))?;
    ensure_eq(
        &stdout,
        &"hi true 43 true\nfalse true\n".to_owned(),
        "unexpected stdout",
    )?;
    Ok(())
}

#[test]
fn build_adapts_a_callback_typed_at_the_callee_parameter_of_a_namespace_call() -> TestResult {
    // radash `_.iterate(5, (acc, idx) => acc + idx, 0)`. Through a namespace
    // import the arrow's parameter keeps the callee's own `T`, which is out of
    // scope at the call and renders erased, while the initial value pins the
    // site at `T = number`. Passing the closure unadapted was E0271.
    // Behaviour: Node prints 15. Types: the adapter is declared at the pinned
    // `f64`, and the initial value stays a plain `f64`.
    let files: &[(&str, &str)] = &[
        (
            "src/lib.ts",
            r"export const iterate = <T>(
  count: number,
  func: (currentValue: T, iteration: number) => T,
  initValue: T,
) => {
  let value = initValue;
  for (let i = 1; i <= count; i++) {
    value = func(value, i);
  }
  return value;
};
",
        ),
        (
            "src/main.ts",
            r#"import * as lib from "./lib";
console.log(lib.iterate(5, (acc, idx) => acc + idx, 0));
"#,
        ),
    ];
    let (project, manifest) = multi_module_project("namespace-generic-callback", true, files)?;
    smelt(&["--manifest-path", &manifest, "build"])?;
    let generated = generated_rust_text(project.path())?;
    ensure(
        generated.contains("move |arg0: &f64, arg1: f64| -> f64"),
        "the callback adapter is declared at the pinned T = f64",
    )?;
    ensure(
        generated.contains(" } }, 0.0)"),
        "the initial value is passed as a plain f64",
    )?;
    let stdout = cargo_run_manifest(&project.path().join("dist/Cargo.toml"))?;
    ensure_eq(&stdout, &"15\n".to_owned(), "unexpected stdout")
}

#[test]
fn build_keeps_a_callback_result_that_widens_the_type_parameter() -> TestResult {
    // `iterate(2, handler, 0)` with a `handler` returning `unknown`: TypeScript
    // infers `T = unknown` from the callback's return, so the string survives.
    // Pinning the site at the initial value's `number` converted it to `NaN`.
    // Behaviour: Node prints `word`. Types: `T` really is `unknown` here, which
    // the source says itself, so the call instantiates at `SmeltUnknown`.
    let files: &[(&str, &str)] = &[(
        "src/main.ts",
        r#"const iterate = <T>(count: number, func: (value: T, i: number) => T, init: T) => {
  let value = init;
  for (let i = 1; i <= count; i++) {
    value = func(value, i);
  }
  return value;
};
const handler: (acc: unknown, i: number) => unknown = (acc, i) => (i > 1 ? "word" : acc);
console.log(iterate(2, handler, 0));
"#,
    )];
    let (project, manifest) = multi_module_project("callback-widens-type-param", true, files)?;
    smelt(&["--manifest-path", &manifest, "build"])?;
    let generated = generated_rust_text(project.path())?;
    ensure(
        generated.contains("move |arg0: &SmeltUnknown, arg1: f64"),
        "the site instantiates at the unknown TypeScript infers",
    )?;
    let stdout = cargo_run_manifest(&project.path().join("dist/Cargo.toml"))?;
    ensure_eq(&stdout, &"word\n".to_owned(), "unexpected stdout")
}

#[test]
fn build_writes_an_erased_list_call_back_through_every_alias() -> TestResult {
    // remeda `heapSiftDown<T>` calling `swapInPlace(data: unknown[], ..)`. The
    // `T[]`/`number[]` argument reaches the `unknown[]` parameter as a converted
    // copy, and the callee's writes must land in the caller's array. Rebinding
    // the caller's place to a new list left every other handle on the old
    // buffer, so `dropFirstBy`'s heap never saw its swaps.
    // Behaviour: Node prints the two lines below. Types: the generic stays a
    // real Rust generic and the concrete local stays `SmeltList<f64>`.
    let files: &[(&str, &str)] = &[(
        "src/main.ts",
        r#"function swap(data: unknown[], i: number, j: number): void {
  [data[i], data[j]] = [data[j], data[i]];
}
function sift<T>(heap: T[]): void {
  swap(heap, 0, 1);
}
function reversed<T>(data: readonly T[]): T[] {
  const heap = data.slice();
  sift(heap);
  return heap;
}
const xs: number[] = [1, 2, 3];
const alias = xs;
swap(xs, 0, 2);
console.log(xs.join(","), alias.join(","));
console.log(reversed(["a", "b"]).join(","), reversed([1, 2]).join(","));
"#,
    )];
    let (project, manifest) = multi_module_project("erased-list-alias", true, files)?;
    smelt(&["--manifest-path", &manifest, "build"])?;
    let generated = generated_rust_text(project.path())?;
    ensure(
        generated.contains("fn sift<T"),
        "`sift` stays a real Rust generic",
    )?;
    ensure(
        generated.contains("let xs: SmeltList<f64>") || generated.contains("let mut xs: SmeltList<f64>"),
        "the concrete local keeps its element type",
    )?;
    let stdout = cargo_run_manifest(&project.path().join("dist/Cargo.toml"))?;
    ensure_eq(&stdout, &"3,2,1 3,2,1\nb,a 2,1\n".to_owned(), "unexpected stdout")
}

#[test]
fn build_erases_a_base_parameter_inside_a_closure_of_an_inherited_copy() -> TestResult {
    // hono `RegExpRouter<T>`: `.map(([h]) => [h, ..])` in a base method, whose
    // inherited copy lands in `class<T> extends RegExpRouter<T>`. The closure
    // is spelled in the base's scope, so its `h` has the BASE's `T`, which
    // renders as the receiving class's generic. Judged out of scope, that value
    // skipped its erasure into the `unknown` array slot (E0308). Behaviour:
    // Node prints `a 7 8`. Types: the copy keeps the receiving class's own
    // parameter, and the concrete subclass keeps `f64`.
    let files: &[(&str, &str)] = &[(
        "src/main.ts",
        r#"class Base<T> {
  items: [T, string][] = [];
  add(h: T, label: string): void {
    this.items.push([h, label]);
  }
  build(): [T, number][] {
    return this.items.map(([h]) => [h, 1]);
  }
}
class Renamed<U> extends Base<U> {
  exported(): [U, number][] {
    return this.build();
  }
}
class Fixed extends Base<number> {
  exported(): number {
    return this.build()[0][0] + 1;
  }
}
const make = () => {
  const Local = class<T> extends Base<T> {
    exported() {
      return this.build();
    }
  };
  const router = new Local<string>();
  router.add("a", "x");
  return router.exported();
};
const renamed = new Renamed<number>();
renamed.add(7, "y");
const fixed = new Fixed();
fixed.add(7, "z");
console.log(make()[0][0], renamed.exported()[0][0], fixed.exported());
"#,
    )];
    let (project, manifest) = multi_module_project("inherited-closure-scope", true, files)?;
    smelt(&["--manifest-path", &manifest, "build"])?;
    let generated = generated_rust_text(project.path())?;
    let heads = generated_lines_with(&generated, "fn build(&self)");
    ensure(
        heads.iter().any(|head| head.contains("SmeltList<(U, f64)>"))
            && heads.iter().any(|head| head.contains("SmeltList<(f64, f64)>")),
        format!("each copy of `build` is typed in its receiving class, got {heads:?}"),
    )?;
    let stdout = cargo_run_manifest(&project.path().join("dist/Cargo.toml"))?;
    ensure_eq(&stdout, &"a 7 8\n".to_owned(), "unexpected stdout")
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "end-to-end CLI fixture: the program and its assertions are one scenario"
)]
fn build_dispatches_a_generic_abstract_base_to_concrete_overrides() -> TestResult {
    // Bounded-generics plan, increment 5: `Parser<Value>` declares abstract
    // `parse`/`validate`, and `NumberParser extends Parser<number>` /
    // `WordParser extends Parser<string>` override them with concrete
    // `value: number` / `value: string` parameters. Calls go through the base
    // (`Parser<number>[]`, the inherited `check`), so they reach the overrides
    // through the virtual slots. Behaviour: Node prints the two lines below.
    // Types: each subclass's slot is the concrete override signature, never
    // `SmeltUnknown`.
    let files: &[(&str, &str)] = &[(
        "src/main.ts",
        r#"abstract class Parser<Value> {
  abstract parse(input: string): Value;
  abstract validate(value: Value): boolean;
  check(input: string): string {
    const value = this.parse(input);
    return this.validate(value) ? "ok" : "rejected";
  }
}

class NumberParser extends Parser<number> {
  constructor(private min: number) {
    super();
  }
  parse(input: string): number {
    return Number(input);
  }
  validate(value: number): boolean {
    return value >= this.min && !isNaN(value);
  }
}

class WordParser extends Parser<string> {
  parse(input: string): string {
    return input.trim();
  }
  validate(value: string): boolean {
    return value.length > 0;
  }
}

function runAll(parsers: Parser<number>[], input: string): string[] {
  return parsers.map((parser) => parser.check(input));
}

const numbers: Parser<number>[] = [new NumberParser(0), new NumberParser(5)];
const words: Parser<string> = new WordParser();
console.log(runAll(numbers, "4").join(","), runAll(numbers, "x").join(","), numbers[1].validate(7), numbers[0].parse("2.5") * 2);
console.log(words.check("  hi "), words.check("   "), words.validate("x"));
"#,
    )];
    let (project, manifest) = multi_module_project("generic-abstract-base", true, files)?;
    smelt(&["--manifest-path", &manifest, "build"])?;
    let generated = generated_rust_text(project.path())?;
    let slots = generated_lines_with(&generated, "validate: ::std::rc::Rc<dyn Fn(");
    for expected in [
        "validate: ::std::rc::Rc<dyn Fn(&Value) -> bool>,",
        "validate: ::std::rc::Rc<dyn Fn(f64) -> bool>,",
        "validate: ::std::rc::Rc<dyn Fn(JsString) -> bool>,",
    ] {
        ensure(
            slots.contains(&expected),
            format!("missing slot `{expected}`, got {slots:?}"),
        )?;
    }
    ensure(
        generated_lines_with(&generated, "fn validate(")
            .iter()
            .all(|line| !line.contains("SmeltUnknown")),
        "an override's parameter is its concrete type",
    )?;
    let stdout = cargo_run_manifest(&project.path().join("dist/Cargo.toml"))?;
    ensure_eq(
        &stdout,
        &"ok,rejected rejected,rejected true 5\nok rejected true\n".to_owned(),
        "unexpected stdout",
    )?;
    Ok(())
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "end-to-end CLI fixture: the program and its assertions are one scenario"
)]
fn build_fixes_a_bounded_generic_method_slot_at_its_bound() -> TestResult {
    // Bounded-generics plan, increment 5 (user decision: fix a bounded method
    // type parameter at its bound). `set<DateType extends Date>` is abstract
    // on `Setter` and overridden by `DaySetter`; it is called through the base
    // (`Setter[]`), so through a virtual slot, which is one stored closure and
    // cannot be generic. The slot, the base method and the override all take
    // and return the bound, `SmeltDate`, so the override mutates the CALLER's
    // date: Node prints `9 9 true` (the alias sees both setters and `set`
    // returns the same date). Erased to `SmeltUnknown`, the override mutated
    // a converted copy and Smelt printed `1 9 false`.
    let files: &[(&str, &str)] = &[(
        "src/main.ts",
        r"abstract class Setter {
  abstract priority: number;
  abstract set<DateType extends Date>(date: DateType, flags: { timestampIsSet: boolean }): DateType;
  validate<DateType extends Date>(_date: DateType): boolean {
    return true;
  }
}

class DaySetter extends Setter {
  priority = 10;
  constructor(private day: number) {
    super();
  }
  set<DateType extends Date>(date: DateType, _flags: { timestampIsSet: boolean }): DateType {
    date.setUTCDate(this.day);
    return date;
  }
  validate<DateType extends Date>(date: DateType): boolean {
    return !isNaN(date.getTime());
  }
}

function applyAll(setters: Setter[], date: Date): Date {
  let result = date;
  for (const setter of setters) {
    if (setter.validate(result)) {
      result = setter.set(result, { timestampIsSet: false });
    }
  }
  return result;
}

const start = new Date(0);
const alias = start;
const out = applyAll([new DaySetter(5), new DaySetter(9)], start);
console.log(alias.getUTCDate(), out.getUTCDate(), out === start);
const bad = new Date(NaN);
console.log(applyAll([new DaySetter(3)], bad) === bad, isNaN(bad.getTime()));
",
    )];
    let (project, manifest) = multi_module_project("bounded-method-slot", true, files)?;
    smelt(&["--manifest-path", &manifest, "build"])?;
    let generated = generated_rust_text(project.path())?;
    let set_slots = generated_lines_with(&generated, "set: ::std::rc::Rc<dyn Fn(");
    ensure(
        !set_slots.is_empty()
            && set_slots.iter().all(|line| {
                *line == "set: ::std::rc::Rc<dyn Fn(SmeltDate, SmeltRecord<JsString, bool>) -> SmeltDate>,"
            }),
        format!("the `set` slot is fixed at the `Date` bound, got {set_slots:?}"),
    )?;
    let validate_slots = generated_lines_with(&generated, "validate: ::std::rc::Rc<dyn Fn(");
    ensure(
        !validate_slots.is_empty()
            && validate_slots
                .iter()
                .all(|line| *line == "validate: ::std::rc::Rc<dyn Fn(SmeltDate) -> bool>,"),
        format!("the `validate` slot is fixed at the `Date` bound, got {validate_slots:?}"),
    )?;
    for method in ["fn set(", "fn validate("] {
        let heads = generated_lines_with(&generated, method);
        ensure(
            !heads.is_empty()
                && heads
                    .iter()
                    .all(|line| line.contains(": SmeltDate") && !line.contains("SmeltUnknown")),
            format!("`{method}` takes the concrete date handle, got {heads:?}"),
        )?;
    }
    let stdout = cargo_run_manifest(&project.path().join("dist/Cargo.toml"))?;
    ensure_eq(&stdout, &"9 9 true\ntrue true\n".to_owned(), "unexpected stdout")?;
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
