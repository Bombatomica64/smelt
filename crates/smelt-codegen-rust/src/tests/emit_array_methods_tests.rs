//! Generated Rust for array and `Set` methods.

use super::*;

#[test]
fn emits_python_pytest_raises_as_result_control_flow() {
    let source = source_for_py_path(
        r#"
import pytest

def test_raises():
    with pytest.raises(Exception):
        raise ValueError("boom")
"#,
        "tests/test_raises.py",
    );
    assert_eq!(
        rust_items(&source, &["fn test_raises"]),
        r#"fn test_raises() -> Result<(), Box<dyn std::error::Error>> {
let _smelt_tmp_1: SmeltPyException;
let _smelt_tmp_2: bool;
let mut __smelt_pytest_raised: bool = false;
_smelt_tmp_1 = SmeltPyException::new(&["ValueError", "Exception", "BaseException"], vec![SmeltPyException::arg(&"boom".to_owned())]);
__smelt_pytest_raised = true;
_smelt_tmp_2 = !(__smelt_pytest_raised);
if _smelt_tmp_2 {
return Err::<_, Box<dyn std::error::Error>>(std::io::Error::new(std::io::ErrorKind::Other, format!("{}", "pytest.raises(...) did not raise".to_owned())).into());
} else {
return Ok(());
}
}"#
    );
}

#[test]
fn emits_python_pytest_raises_match_as_regex_check() {
    let source = source_for_py_path(
        r#"
import pytest

def test_raises_match():
    with pytest.raises(Exception, match="bo+m"):
        raise ValueError("booom")
"#,
        "tests/test_raises_match.py",
    );
    assert_eq!(
        rust_items(&source, &["fn test_raises_match"]),
        r#"fn test_raises_match() -> Result<(), Box<dyn std::error::Error>> {
let __smelt_pytest_exception: String;
let _smelt_tmp_2: SmeltPyException;
let _smelt_tmp_3: bool;
let _smelt_tmp_4: bool;
let _smelt_tmp_5: bool;
let mut __smelt_pytest_raised: bool = false;
_smelt_tmp_2 = SmeltPyException::new(&["ValueError", "Exception", "BaseException"], vec![SmeltPyException::arg(&"booom".to_owned())]);
__smelt_pytest_exception = _smelt_tmp_2.to_string();
__smelt_pytest_raised = true;
_smelt_tmp_3 = fancy_regex::Regex::new(&"bo+m".to_owned()).expect("regex compile failed").is_match(&__smelt_pytest_exception).unwrap_or(false);
_smelt_tmp_4 = !(_smelt_tmp_3);
if _smelt_tmp_4 {
return Err::<_, Box<dyn std::error::Error>>(std::io::Error::new(std::io::ErrorKind::Other, format!("{}", "pytest.raises(...) match failed".to_owned())).into());
} else {
_smelt_tmp_5 = !(__smelt_pytest_raised);
if _smelt_tmp_5 {
return Err::<_, Box<dyn std::error::Error>>(std::io::Error::new(std::io::ErrorKind::Other, format!("{}", "pytest.raises(...) did not raise".to_owned())).into());
} else {
return Ok(());
}
}
}"#
    );
}

#[test]
fn emits_typescript_vitest_test_case_as_rust_test() {
    let source = source_for(
        r#"
import { test, expect } from "vitest";

test("adds numbers", () => {
  expect(1 + 1).toBe(2);
});
"#,
    );
    assert_eq!(
        rust_items(&source, &["fn test_adds_numbers"]),
        r#"fn test_adds_numbers() -> Result<(), Box<dyn std::error::Error>> {
let _smelt_tmp_0: f64 = 1.0 + 1.0;
let _smelt_tmp_1: bool = !({ let smelt_lhs: f64 = _smelt_tmp_0; let smelt_rhs: f64 = 2.0; (smelt_lhs.is_nan() && smelt_rhs.is_nan()) || (smelt_lhs == smelt_rhs && (smelt_lhs != 0.0 || smelt_lhs.is_sign_negative() == smelt_rhs.is_sign_negative())) });
if _smelt_tmp_1 {
return Err::<_, Box<dyn std::error::Error>>(std::io::Error::new(std::io::ErrorKind::Other, format!("{}", JsString::from("expect(...).toBe(...) failed: expect(1 + 1).toBe(2) (<memory>:5:3)"))).into());
} else {
return Ok(());
}
}"#
    );
}

#[test]
fn emits_nullish_vitest_matchers_as_success_for_null_literals() {
    let source = source_for(
        r#"
import { test, expect } from "vitest";

test("nullish", () => {
  expect(undefined).toBeUndefined();
  expect(null).toBeNull();
});
"#,
    );
    assert_eq!(
        rust_items(&source, &["fn test_nullish"]),
        r#"fn test_nullish() -> Result<(), Box<dyn std::error::Error>> {
let _smelt_tmp_1: bool;
let _smelt_tmp_0: bool = !(true);
if _smelt_tmp_0 {
return Err::<_, Box<dyn std::error::Error>>(std::io::Error::new(std::io::ErrorKind::Other, format!("{}", JsString::from("expect(...).toBeUndefined() failed: expect(undefined).toBeUndefined() (<memory>:5:3)"))).into());
} else {
_smelt_tmp_1 = !(true);
if _smelt_tmp_1 {
return Err::<_, Box<dyn std::error::Error>>(std::io::Error::new(std::io::ErrorKind::Other, format!("{}", JsString::from("expect(...).toBeNull() failed: expect(null).toBeNull() (<memory>:6:3)"))).into());
} else {
return Ok(());
}
}
}"#
    );
}

#[test]
fn emits_optional_unknown_to_be_undefined_without_unwrap() {
    let source = source_for(
        r#"
import { test, expect } from "vitest";

function maybeValue(flag: boolean): unknown | undefined {
  if (flag) {
    return undefined;
  }
  return "x";
}

test("optional unknown", () => {
  expect(maybeValue(true)).toBeUndefined();
});
"#,
    );
    assert_eq!(
        rust_items(&source, &["fn test_optional_unknown"]),
        r#"fn test_optional_unknown() -> Result<(), Box<dyn std::error::Error>> {
let _smelt_tmp_1: bool;
let _smelt_tmp_0: Option<SmeltUnknown> = maybe_value(true);
_smelt_tmp_1 = !(_smelt_tmp_0.as_ref().map_or(true, |smelt_value| matches!(smelt_value, SmeltUnknown::Undefined)));
if _smelt_tmp_1 {
return Err::<_, Box<dyn std::error::Error>>(smelt_throw(SmeltUnknown::String("expect(...).toBeUndefined() failed: expect(maybeValue(true)).toBeUndefined() (<memory>:12:3)".into())));
} else {
return Ok(());
}
}"#
    );
}

#[test]
fn erases_explicit_undefined_literal_to_undefined_tag() {
    let source = source_for(
        r"
function value(): unknown {
  return undefined;
}
",
    );
    assert_eq!(
        rust_items(&source, &["fn value"]),
        r"fn value() -> SmeltUnknown {
return SmeltUnknown::Undefined;
}"
    );
}

#[test]
fn erases_void_operator_to_undefined_tag() {
    let source = source_for(
        r"
function value(): unknown {
  return void 0;
}
",
    );
    assert_eq!(
        rust_items(&source, &["fn value"]),
        r"fn value() -> SmeltUnknown {
return SmeltUnknown::Undefined;
}"
    );
}

#[test]
fn keeps_null_literal_erasure_as_null_tag() {
    let source = source_for(
        r"
function value(): unknown {
  return null;
}
",
    );
    assert_eq!(
        rust_items(&source, &["fn value"]),
        r"fn value() -> SmeltUnknown {
return SmeltUnknown::Null;
}"
    );
}

#[test]
fn emits_typescript_describe_it_as_flattened_rust_test() {
    let source = source_for(
        r#"
import { describe, it, expect } from "vitest";

describe("math helpers", () => {
  it("adds numbers", () => {
    expect(1 + 1).toBe(2);
  });
});
"#,
    );
    assert_eq!(
        rust_items(&source, &["fn test_math_helpers_adds_numbers"]),
        r#"fn test_math_helpers_adds_numbers() -> Result<(), Box<dyn std::error::Error>> {
let _smelt_tmp_0: f64 = 1.0 + 1.0;
let _smelt_tmp_1: bool = !({ let smelt_lhs: f64 = _smelt_tmp_0; let smelt_rhs: f64 = 2.0; (smelt_lhs.is_nan() && smelt_rhs.is_nan()) || (smelt_lhs == smelt_rhs && (smelt_lhs != 0.0 || smelt_lhs.is_sign_negative() == smelt_rhs.is_sign_negative())) });
if _smelt_tmp_1 {
return Err::<_, Box<dyn std::error::Error>>(std::io::Error::new(std::io::ErrorKind::Other, format!("{}", JsString::from("expect(...).toBe(...) failed: expect(1 + 1).toBe(2) (<memory>:6:5)"))).into());
} else {
return Ok(());
}
}"#
    );
}

#[test]
fn emits_distinct_classes_for_same_named_sibling_suite_bindings() {
    let source = source_for(
        r#"
import { describe, expect, it } from "vitest";
describe("outer", () => {
  describe("first", () => {
    class Person { constructor() {} }
    it("constructs", () => { expect(new Person()).toBeInstanceOf(Person); });
  });
  describe("second", () => {
    class Person {
      name: string;
      friends: Person[] = [];
      self?: Person;
      constructor(name: string) { this.name = name; }
    }
    it("keeps fields", () => {
      const person = new Person("jake");
      person.self = person;
      person.friends = [person];
      expect(person.name).toBe("jake");
    });
  });
});
"#,
    );

    let person_structs = source
        .lines()
        .filter(|line| {
            let Some(name) = line
                .strip_prefix("struct ")
                .and_then(|rest| rest.split([' ', '{', '(']).next())
            else {
                return false;
            };
            name.starts_with("PersonSmeltSuiteF") && !name.ends_with("Inner")
        })
        .count();
    assert_eq!(person_structs, 2, "{source}");
    assert!(
        source.contains("::new(JsString::from(\"jake\"))"),
        "{source}"
    );
    assert!(source.contains(".self_ ="), "{source}");
    assert!(source.contains(".friends ="), "{source}");
}

#[test]
fn emits_typescript_vitest_common_positive_matchers() {
    let source = source_for(
        r#"
import { test, expect } from "vitest";

test("common matchers", () => {
  expect(1 + 1).toEqual(2);
  expect([1, 2, 3]).toContain(2);
  expect([1, 2, 3]).toHaveLength(3);
  expect(["a"]).toStrictEqual(["a"]);
  expect([1, 2, 3]).not.toContain(4);
  expect(new Date()).toBeInstanceOf(Date);
  const user: Record<string, string> = { name: "Ada" };
  expect(user).toHaveProperty("name");
  U.deepStrictEqual([1, 2], [1, 2]);
});
"#,
    );
    assert_eq!(
        rust_items(&source, &["fn test_common_matchers"]),
        r#"fn test_common_matchers() -> Result<(), Box<dyn std::error::Error>> {
let user: SmeltRecord<JsString, JsString>;
let _smelt_tmp_3: SmeltList<f64>;
let _smelt_tmp_4: bool;
let _smelt_tmp_5: bool;
let _smelt_tmp_6: SmeltList<f64>;
let _smelt_tmp_7: i64;
let _smelt_tmp_8: bool;
let _smelt_tmp_9: SmeltList<JsString>;
let _smelt_tmp_10: SmeltList<JsString>;
let _smelt_tmp_11: bool;
let _smelt_tmp_12: SmeltList<f64>;
let _smelt_tmp_13: bool;
let _smelt_tmp_14: bool;
let _smelt_tmp_15: bool;
let _smelt_tmp_16: bool;
let _smelt_tmp_18: bool;
let _smelt_tmp_19: bool;
let _smelt_tmp_20: SmeltList<f64>;
let _smelt_tmp_21: SmeltList<f64>;
let _smelt_tmp_22: bool;
let _smelt_tmp_1: f64 = 1.0 + 1.0;
let _smelt_tmp_2: bool = !({ let smelt_lhs: f64 = _smelt_tmp_1; let smelt_rhs: f64 = 2.0; (smelt_lhs.is_nan() && smelt_rhs.is_nan()) || (smelt_lhs == smelt_rhs && (smelt_lhs != 0.0 || smelt_lhs.is_sign_negative() == smelt_rhs.is_sign_negative())) });
if _smelt_tmp_2 {
return Err::<_, Box<dyn std::error::Error>>(std::io::Error::new(std::io::ErrorKind::Other, format!("{}", JsString::from("expect(...).toEqual(...) failed: expect(1 + 1).toEqual(2) (<memory>:5:3)"))).into());
} else {
_smelt_tmp_3 = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<f64> = vec![1.0, 2.0, 3.0]; smelt_list_items }));
_smelt_tmp_4 = { let smelt_needle = 2.0; _smelt_tmp_3.borrow().iter().any(|smelt_item| *smelt_item == smelt_needle || (smelt_item.is_nan() && smelt_needle.is_nan())) };
_smelt_tmp_5 = !(_smelt_tmp_4);
if _smelt_tmp_5 {
return Err::<_, Box<dyn std::error::Error>>(std::io::Error::new(std::io::ErrorKind::Other, format!("{}", JsString::from("expect(...).toContain(...) failed: expect([1, 2, 3]).toContain(2) (<memory>:6:3)"))).into());
} else {
_smelt_tmp_6 = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<f64> = vec![1.0, 2.0, 3.0]; smelt_list_items }));
_smelt_tmp_7 = _smelt_tmp_6.len() as i64;
_smelt_tmp_8 = (_smelt_tmp_7 as f64) != 3.0;
if _smelt_tmp_8 {
return Err::<_, Box<dyn std::error::Error>>(std::io::Error::new(std::io::ErrorKind::Other, format!("{}", JsString::from("expect(...).toHaveLength(...) failed: expect([1, 2, 3]).toHaveLength(3) (<memory>:7:3)"))).into());
} else {
_smelt_tmp_9 = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<JsString> = vec![JsString::from("a")]; smelt_list_items }));
_smelt_tmp_10 = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<JsString> = vec![JsString::from("a")]; smelt_list_items }));
_smelt_tmp_11 = _smelt_tmp_9 != _smelt_tmp_10;
if _smelt_tmp_11 {
return Err::<_, Box<dyn std::error::Error>>(std::io::Error::new(std::io::ErrorKind::Other, format!("{}", JsString::from("expect(...).toStrictEqual(...) failed: expect([\"a\"]).toStrictEqual([\"a\"]) (<memory>:8:3)"))).into());
} else {
_smelt_tmp_12 = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<f64> = vec![1.0, 2.0, 3.0]; smelt_list_items }));
_smelt_tmp_13 = { let smelt_needle = 4.0; _smelt_tmp_12.borrow().iter().any(|smelt_item| *smelt_item == smelt_needle || (smelt_item.is_nan() && smelt_needle.is_nan())) };
_smelt_tmp_14 = !(_smelt_tmp_13);
_smelt_tmp_15 = !(_smelt_tmp_14);
if _smelt_tmp_15 {
return Err::<_, Box<dyn std::error::Error>>(std::io::Error::new(std::io::ErrorKind::Other, format!("{}", JsString::from("expect(...).toContain(...) failed: expect([1, 2, 3]).not.toContain(4) (<memory>:9:3)"))).into());
} else {
_smelt_tmp_16 = !(true);
if _smelt_tmp_16 {
return Err::<_, Box<dyn std::error::Error>>(std::io::Error::new(std::io::ErrorKind::Other, format!("{}", JsString::from("expect(...).toBeInstanceOf(...) failed: expect(new Date()).toBeInstanceOf(Date) (<memory>:10:3)"))).into());
} else {
user = SmeltRecord::from([(JsString::from("name"), JsString::from("Ada"))]);
_smelt_tmp_18 = user.contains_key(&JsString::from("name"));
_smelt_tmp_19 = !(_smelt_tmp_18);
if _smelt_tmp_19 {
return Err::<_, Box<dyn std::error::Error>>(std::io::Error::new(std::io::ErrorKind::Other, format!("{}", JsString::from("expect(...).toHaveProperty(...) failed: expect(user).toHaveProperty(\"name\") (<memory>:12:3)"))).into());
} else {
_smelt_tmp_20 = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<f64> = vec![1.0, 2.0]; smelt_list_items }));
_smelt_tmp_21 = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<f64> = vec![1.0, 2.0]; smelt_list_items }));
_smelt_tmp_22 = !(_smelt_tmp_20.len() == _smelt_tmp_21.len() && _smelt_tmp_20.borrow().iter().zip(_smelt_tmp_21.borrow().iter()).all(|(smelt_left_item, smelt_right_item)| (smelt_left_item == smelt_right_item || (smelt_left_item.is_nan() && smelt_right_item.is_nan()))));
if _smelt_tmp_22 {
return Err::<_, Box<dyn std::error::Error>>(std::io::Error::new(std::io::ErrorKind::Other, format!("{}", JsString::from("deepStrictEqual(...) failed: U.deepStrictEqual([1, 2], [1, 2]) (<memory>:13:3)"))).into());
} else {
return Ok(());
}
}
}
}
}
}
}
}
}"#
    );
}

/// Vitest compares primitive numbers with `Object.is` under every equality
/// matcher, so `NaN` equals `NaN`.
///
/// Only `toBe` used the `Object.is` comparison; `toEqual` and `toStrictEqual`
/// emitted a plain `!=`, which on `f64` reports `NaN != NaN`. Assertions like
/// `expect(mean([])).toEqual(NaN)` therefore failed on the value they wanted.
/// Objects and arrays keep structural comparison under the deep matchers --
/// only `toBe` compares those by reference.
#[test]
fn deep_matchers_compare_numbers_with_object_is() {
    let source = source_for(
        r#"
import { test, expect } from "vitest";

function mean(values: number[]): number {
  return values.length === 0 ? NaN : values[0];
}

test("nan", () => {
  expect(mean([])).toEqual(NaN);
  expect(mean([])).toStrictEqual(NaN);
  expect([1]).toEqual([1]);
});
"#,
    );
    assert_eq!(
        rust_items(&source, &["fn test_nan"]),
        r#"fn test_nan() -> Result<(), Box<dyn std::error::Error>> {
let _smelt_tmp_2: bool;
let _smelt_tmp_3: SmeltList<f64>;
let _smelt_tmp_5: bool;
let _smelt_tmp_6: SmeltList<f64>;
let _smelt_tmp_7: SmeltList<f64>;
let _smelt_tmp_8: bool;
let _smelt_tmp_0: SmeltList<f64> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<f64> = vec![]; smelt_list_items }));
let _smelt_tmp_1: f64 = mean(_smelt_tmp_0);
_smelt_tmp_2 = !({ let smelt_lhs: f64 = _smelt_tmp_1; let smelt_rhs: f64 = f64::NAN; (smelt_lhs.is_nan() && smelt_rhs.is_nan()) || (smelt_lhs == smelt_rhs && (smelt_lhs != 0.0 || smelt_lhs.is_sign_negative() == smelt_rhs.is_sign_negative())) });
if _smelt_tmp_2 {
return Err::<_, Box<dyn std::error::Error>>(std::io::Error::new(std::io::ErrorKind::Other, format!("{}", JsString::from("expect(...).toEqual(...) failed: expect(mean([])).toEqual(NaN) (<memory>:9:3)"))).into());
} else {
_smelt_tmp_3 = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<f64> = vec![]; smelt_list_items }));
let _smelt_tmp_4: f64 = mean(_smelt_tmp_3);
_smelt_tmp_5 = !({ let smelt_lhs: f64 = _smelt_tmp_4; let smelt_rhs: f64 = f64::NAN; (smelt_lhs.is_nan() && smelt_rhs.is_nan()) || (smelt_lhs == smelt_rhs && (smelt_lhs != 0.0 || smelt_lhs.is_sign_negative() == smelt_rhs.is_sign_negative())) });
if _smelt_tmp_5 {
return Err::<_, Box<dyn std::error::Error>>(std::io::Error::new(std::io::ErrorKind::Other, format!("{}", JsString::from("expect(...).toStrictEqual(...) failed: expect(mean([])).toStrictEqual(NaN) (<memory>:10:3)"))).into());
} else {
_smelt_tmp_6 = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<f64> = vec![1.0]; smelt_list_items }));
_smelt_tmp_7 = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<f64> = vec![1.0]; smelt_list_items }));
_smelt_tmp_8 = !(_smelt_tmp_6.len() == _smelt_tmp_7.len() && _smelt_tmp_6.borrow().iter().zip(_smelt_tmp_7.borrow().iter()).all(|(smelt_left_item, smelt_right_item)| (smelt_left_item == smelt_right_item || (smelt_left_item.is_nan() && smelt_right_item.is_nan()))));
if _smelt_tmp_8 {
return Err::<_, Box<dyn std::error::Error>>(std::io::Error::new(std::io::ErrorKind::Other, format!("{}", JsString::from("expect(...).toEqual(...) failed: expect([1]).toEqual([1]) (<memory>:11:3)"))).into());
} else {
return Ok(());
}
}
}
}"#
    );
}

/// A failed assertion must name the source assertion and its location.
///
/// Generated suites throw a plain string, so without the snippet and
/// `path:line:column` suffix a large generated suite reports only which
/// matcher failed, which is not enough to find the offending spec line.
#[test]
fn generated_assertion_failures_carry_source_snippet_and_location() {
    let source = source_for(
        r#"
import { test, expect } from "vitest";

test("located", () => {
  expect(1 + 1).toEqual(2);
  expect([
    1,
    2,
  ]).toHaveLength(2);
});
"#,
    );

    assert!(
        source.contains("expect(...).toEqual(...) failed: expect(1 + 1).toEqual(2) (<memory>:5:3)"),
        "{source}"
    );
    // A multi-line assertion collapses onto one line so the message stays scannable.
    assert!(
        source.contains(
            "expect(...).toHaveLength(...) failed: expect([ 1, 2, ]).toHaveLength(2) (<memory>:6:3)"
        ),
        "{source}"
    );
}

#[test]
fn emits_identity_bearing_erased_arrays_for_strict_matchers() {
    let source = source_for(
        r#"
import { test, expect } from "vitest";

test("array reference identity", () => {
  const original: unknown = [1];
  const alias: unknown = original;
  const copy: unknown = [1];
  expect(alias).toBe(original);
  expect(copy).not.toBe(original);
  expect(copy).toStrictEqual(original);
});
"#,
    );
    assert_eq!(
        rust_items(&source, &["fn test_array_reference_identity"]),
        r#"fn test_array_reference_identity() -> Result<(), Box<dyn std::error::Error>> {
let _smelt_tmp_6: bool;
let _smelt_tmp_7: bool;
let original: SmeltUnknown = SmeltUnknown::Array(vec![SmeltUnknown::Number(1.0 as f64)].into());
let alias: SmeltUnknown = original.clone();
let copy: SmeltUnknown = SmeltUnknown::Array(vec![SmeltUnknown::Number(1.0 as f64)].into());
let _smelt_tmp_5: bool = !(original.clone().same_js_key(&original));
if _smelt_tmp_5 {
return Err::<_, Box<dyn std::error::Error>>(smelt_throw(SmeltUnknown::String("expect(...).toBe(...) failed: expect(alias).toBe(original) (<memory>:8:3)".into())));
} else {
_smelt_tmp_6 = copy.clone().same_js_key(&original);
if _smelt_tmp_6 {
return Err::<_, Box<dyn std::error::Error>>(smelt_throw(SmeltUnknown::String("expect(...).toBe(...) failed: expect(copy).not.toBe(original) (<memory>:9:3)".into())));
} else {
_smelt_tmp_7 = !(copy == original);
if _smelt_tmp_7 {
return Err::<_, Box<dyn std::error::Error>>(smelt_throw(SmeltUnknown::String("expect(...).toStrictEqual(...) failed: expect(copy).toStrictEqual(original) (<memory>:10:3)".into())));
} else {
return Ok(());
}
}
}
}"#
    );
}

#[test]
fn emits_same_value_zero_identity_for_erased_object_is_or_strict_equality() {
    let source = source_for(
        r"
export function same(data: unknown, other: unknown): boolean {
  return data === other || Object.is(data, other);
}
",
    );
    assert_eq!(
        rust_items(&source, &["fn same"]),
        r"fn same(data: SmeltUnknown, other: SmeltUnknown) -> bool {
let _smelt_tmp_2: bool = data.clone().same_js_key(&other);
return _smelt_tmp_2;
}"
    );
}

#[test]
fn emits_strict_identity_for_optional_records() {
    // The record must be identity-bearing for `toBe` to read an `id`: a
    // string-keyed dict only uses the `SmeltRecord` container (which carries an
    // object id) when the program emits the `SmeltUnknown` prelude, otherwise it
    // is a plain `HashMap` with no identity to read. The `unknown` field keeps
    // this program on the identity-bearing representation.
    let source = source_for(
        r#"
import { expect } from "vitest";

function compare(left?: { a: unknown }, right?: { a: unknown }): void {
  expect(left).toBe(right);
  expect(left).not.toBe(right);
}
"#,
    );
    assert_eq!(
        rust_items(&source, &["fn compare"]),
        r#"fn compare(left: Option<SmeltRecord<JsString, SmeltUnknown>>, right: Option<SmeltRecord<JsString, SmeltUnknown>>) -> Result<(), Box<dyn std::error::Error>> {
let _smelt_tmp_3: bool;
let _smelt_tmp_2: bool = !(match (left.clone().as_ref(), right.clone().as_ref()) { (Some(left), Some(right)) => left.id == right.id, (None, None) => true, _ => false });
if _smelt_tmp_2 {
return Err::<_, Box<dyn std::error::Error>>(smelt_throw(SmeltUnknown::String("expect(...).toBe(...) failed: expect(left).toBe(right) (<memory>:5:3)".into())));
} else {
_smelt_tmp_3 = match (left.clone().as_ref(), right.clone().as_ref()) { (Some(left), Some(right)) => left.id == right.id, (None, None) => true, _ => false };
if _smelt_tmp_3 {
return Err::<_, Box<dyn std::error::Error>>(smelt_throw(SmeltUnknown::String("expect(...).toBe(...) failed: expect(left).not.toBe(right) (<memory>:6:3)".into())));
} else {
return Ok(());
}
}
}"#
    );
}

#[test]
fn emits_map_constructor_has_and_get_methods() {
    let source = source_for(
        r#"
const mapping: Map<string, number> = new Map();
const literal = new Map([["a", 1], ["b", 2]]);
const has = mapping.has("a");
const value = mapping.get("a");
"#,
    );
    assert_eq!(
        rust_items(&source, &["fn main"]),
        r#"fn main() {
let mapping: SmeltJsMap<JsString, f64> = SmeltJsMap::from([]);
let literal: SmeltJsMap<JsString, f64> = SmeltJsMap::from([(JsString::from("a"), 1.0), (JsString::from("b"), 2.0)]);
let has: bool = mapping.contains_key(&JsString::from("a"));
let value: Option<f64> = mapping.get(&JsString::from("a"));
return;
}"#
    );
}

/// A literal whose only nullish spelling is uniform still uses `Option`.
///
/// The mixed-spelling boundary above must not widen every nullable literal --
/// one spelling has one empty state, so `Optional(T)` represents it exactly and
/// the concrete element type is kept.
#[test]
fn uniformly_nullish_array_literal_stays_optional() {
    let source = source_for(
        r"
export function rows(): number {
  const row = [1, 2, null];
  return row.length;
}
",
    );
    assert_eq!(
        rust_items(&source, &["fn rows"]),
        r"fn rows() -> f64 {
let row: SmeltList<Option<f64>> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<Option<f64>> = vec![Some(1.0), Some(2.0), None::<f64>]; smelt_list_items }));
let _smelt_tmp_2: f64 = row.len() as f64;
return _smelt_tmp_2;
}"
    );
}

#[test]
fn emits_reference_identity_for_typed_map_strict_equality() {
    // `toBe` on a source `Map` is JavaScript reference identity: a clone with
    // identical entries is `!==` the original. The typed container
    // (`SmeltJsMap`) carries a stable object `id`, so the comparison reads that
    // id instead of falling back to Rust structural `PartialEq`.
    let source = source_for(
        r#"
import { test, expect } from "vitest";

test("map identity", () => {
  const map = new Map<number, string>([[1, "a"]]);
  const alias = map;
  expect(alias).toBe(map);
});
"#,
    );
    assert_eq!(
        rust_items(&source, &["fn test_map_identity"]),
        r#"fn test_map_identity() -> Result<(), Box<dyn std::error::Error>> {
let map: SmeltJsMap<f64, JsString> = SmeltJsMap::from([(1.0, JsString::from("a"))]);
let alias: SmeltJsMap<f64, JsString> = map.clone();
let _smelt_tmp_3: bool = !(map.clone().id == map.clone().id);
if _smelt_tmp_3 {
return Err::<_, Box<dyn std::error::Error>>(smelt_throw(SmeltUnknown::String("expect(...).toBe(...) failed: expect(alias).toBe(map) (<memory>:7:3)".into())));
} else {
return Ok(());
}
}"#
    );
}

#[test]
fn emits_reference_identity_for_typed_set_strict_equality() {
    // A `Set` whose element cannot key a Rust `HashSet` uses `SmeltJsSet`,
    // which carries an object `id`; `toBe` compares that id.
    let source = source_for(
        r#"
import { test, expect } from "vitest";

test("set identity", () => {
  const values = new Set<number>([1]);
  const alias = values;
  expect(alias).toBe(values);
});
"#,
    );
    assert_eq!(
        rust_items(&source, &["fn test_set_identity"]),
        r#"fn test_set_identity() -> Result<(), Box<dyn std::error::Error>> {
let values: SmeltJsSet<f64> = SmeltJsSet::from([1.0]);
let alias: SmeltJsSet<f64> = values.clone();
let _smelt_tmp_3: bool = !(values.clone().id == values.clone().id);
if _smelt_tmp_3 {
return Err::<_, Box<dyn std::error::Error>>(smelt_throw(SmeltUnknown::String("expect(...).toBe(...) failed: expect(alias).toBe(values) (<memory>:7:3)".into())));
} else {
return Ok(());
}
}"#
    );
}

#[test]
fn keeps_structural_equality_for_deep_matchers_on_reference_values() {
    // The identity rule fires for `toBe` only. `toEqual`/`toStrictEqual` (and
    // the `isDeepEqual`-shaped `==` they lower to) must keep using the
    // containers' structural `PartialEq`, otherwise deep comparison of two
    // distinct-but-equal objects would report inequality.
    let source = source_for(
        r#"
import { test, expect } from "vitest";

test("map structural", () => {
  const map = new Map<number, string>([[1, "a"]]);
  const copy = new Map<number, string>([[1, "a"]]);
  expect(copy).toEqual(map);
  expect(copy).toStrictEqual(map);
});
"#,
    );
    assert_eq!(
        rust_items(&source, &["fn test_map_structural"]),
        r#"fn test_map_structural() -> Result<(), Box<dyn std::error::Error>> {
let _smelt_tmp_5: bool;
let map: SmeltJsMap<f64, JsString> = SmeltJsMap::from([(1.0, JsString::from("a"))]);
let copy: SmeltJsMap<f64, JsString> = SmeltJsMap::from([(1.0, JsString::from("a"))]);
let _smelt_tmp_4: bool = copy.clone() != map.clone();
if _smelt_tmp_4 {
return Err::<_, Box<dyn std::error::Error>>(smelt_throw(SmeltUnknown::String("expect(...).toEqual(...) failed: expect(copy).toEqual(map) (<memory>:7:3)".into())));
} else {
_smelt_tmp_5 = copy != map;
if _smelt_tmp_5 {
return Err::<_, Box<dyn std::error::Error>>(smelt_throw(SmeltUnknown::String("expect(...).toStrictEqual(...) failed: expect(copy).toStrictEqual(map) (<memory>:8:3)".into())));
} else {
return Ok(());
}
}
}"#
    );
}

#[test]
fn emits_reference_identity_for_optional_maps() {
    // `Optional` of a reference type keeps identity on the present values while
    // two absent values still compare equal (`undefined === undefined`).
    let source = source_for(
        r#"
import { expect } from "vitest";

function compare(left?: Map<number, string>, right?: Map<number, string>): void {
  expect(left).toBe(right);
}
"#,
    );
    assert_eq!(
        rust_items(&source, &["fn compare"]),
        r#"fn compare(left: Option<SmeltJsMap<f64, JsString>>, right: Option<SmeltJsMap<f64, JsString>>) -> Result<(), Box<dyn std::error::Error>> {
let _smelt_tmp_2: bool = !(match (left.clone().as_ref(), right.clone().as_ref()) { (Some(left), Some(right)) => left.id == right.id, (None, None) => true, _ => false });
if _smelt_tmp_2 {
return Err::<_, Box<dyn std::error::Error>>(smelt_throw(SmeltUnknown::String("expect(...).toBe(...) failed: expect(left).toBe(right) (<memory>:5:3)".into())));
} else {
return Ok(());
}
}"#
    );
}

#[test]
fn emits_false_for_identityless_reference_strict_equality() {
    // A tuple lowers to a Rust tuple, which stores no object id. Rather than
    // inventing an identity, the comparison keeps reporting "not the same
    // reference".
    let source = source_for(
        r#"
import { test, expect } from "vitest";

test("tuple identity", () => {
  const pair: [number, number] = [1, 2];
  const other: [number, number] = [1, 2];
  expect(other).not.toBe(pair);
});
"#,
    );
    assert_eq!(
        rust_items(&source, &["fn test_tuple_identity"]),
        r#"fn test_tuple_identity() -> Result<(), Box<dyn std::error::Error>> {
let pair: (f64, f64) = (1.0, 2.0);
let other: (f64, f64) = (1.0, 2.0);
let _smelt_tmp_4: bool = false;
if _smelt_tmp_4 {
return Err::<_, Box<dyn std::error::Error>>(std::io::Error::new(std::io::ErrorKind::Other, format!("{}", JsString::from("expect(...).toBe(...) failed: expect(other).not.toBe(pair) (<memory>:7:3)"))).into());
} else {
return Ok(());
}
}"#
    );
}
