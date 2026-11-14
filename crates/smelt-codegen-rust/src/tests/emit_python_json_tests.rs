//! Generated Rust for Python JSON and builtin arrays.

use super::*;

#[test]
fn keeps_continuation_after_short_circuit_call_guard() {
    let source = source_for(
        r"
function valid(value: boolean): boolean {
  return value;
}

export function count(a: boolean, b: boolean): number {
  if (!valid(a) || !valid(b)) return NaN;
  let result = 0;
  while (a) {
    result += 1;
    break;
  }
  return result;
}
",
    );

    // `result` is a counter proven to stay in `0..=1`, so integer range
    // analysis makes it an integer, and the `number` result is its exact conversion.
    assert_eq!(rust_fn(&source, "count"), r"fn count(a: bool, b: bool) -> f64 {
let mut result: usize;
let _smelt_tmp_4: bool;
let mut _smelt_tmp_5: bool;
let _smelt_tmp_7: bool;
let _smelt_tmp_8: usize;
let _smelt_tmp_9: f64;
let _smelt_tmp_3: bool = valid(a);
_smelt_tmp_4 = !(_smelt_tmp_3);
if _smelt_tmp_4 {
_smelt_tmp_5 = true;
} else {
let _smelt_tmp_6: bool = valid(b);
_smelt_tmp_7 = !(_smelt_tmp_6);
_smelt_tmp_5 = _smelt_tmp_7;
}
if _smelt_tmp_5 {
return f64::NAN;
} else {
result = 0;
if a {
_smelt_tmp_8 = result + 1;
result = _smelt_tmp_8;
}
_smelt_tmp_9 = result as f64;
return _smelt_tmp_9;
}
}");
}

#[test]
fn emits_python_top_level_vararg_and_kwarg_functions() {
    let source = source_for_py(
        r#"
def sum_values(*values: int) -> int:
    return values[0] + values[1]

def pick(**kwargs: int) -> int:
    return kwargs["value"] + kwargs["bonus"]

total: int = sum_values(2, 3, 4)
extras: dict[str, int] = {"bonus": 6}
chosen: int = pick(value=5, **extras)
"#,
    );

    // `values[0]` raises `IndexError` and `kwargs["value"]` raises `KeyError`, so
    // both functions can fail, and the collected parameters keep their element types.
    assert_eq!(rust_items(&source, &["sig sum_values", "sig pick"]), r"fn sum_values(values: SmeltList<i64>) -> Result<i64, Box<dyn std::error::Error>>

fn pick(kwargs: SmeltRecord<String, i64>) -> Result<i64, Box<dyn std::error::Error>>");
}

#[test]
fn emits_json_stringify_calls() {
    let ts_source = source_for(
        r"
const values: number[] = [1, 2];
const text = JSON.stringify(values);
",
    );
    let py_source = source_for_py(
        r"
import json
values: list[int] = [1, 2]
text: str = json.dumps(values)
",
    );

    assert!(ts_source.contains("smelt_json_stringify(&"));
    assert!(py_source.contains("smelt_json_stringify(&"));
    assert!(ts_source.contains(".expect(\"JSON serialization failed\")"));
    assert!(py_source.contains(".expect(\"JSON serialization failed\")"));
}

#[test]
fn emits_json_parse_calls() {
    let ts_source = source_for(
        r#"
const text = "[1,2]";
const values = JSON.parse<number[]>(text);
"#,
    );
    let py_source = source_for_py(
        r#"
import json

def run() -> None:
    text: str = "[1,2]"
    values: list[int] = json.loads(text)
"#,
    );

    // `JSON.parse` / `json.loads` is fallible in both source languages, so it
    // goes through the one adapter that reports a malformed document on the
    // error channel instead of aborting. A TypeScript `JSON.parse<T>` names a
    // declared target, so it is a typed entry boundary: the call decodes the
    // parsed value strictly into `T` (`emitter::json_decode`, adapted from
    // Bun's `--aot` entry checks) instead of coercing it leniently. Python's
    // `json.loads` keeps the dynamic parse and the ordinary coercion.
    assert!(
        ts_source.contains(": SmeltList<f64> = smelt_json_decode_"),
        "{ts_source}"
    );
    assert!(ts_source.contains("smelt_json_parse(text)?"), "{ts_source}");
    assert!(py_source.contains("smelt_json_parse(&"), "{py_source}");
    assert!(!ts_source.contains("JSON parse failed"), "{ts_source}");
    assert!(!py_source.contains("JSON parse failed"), "{py_source}");
    assert!(ts_source.contains("let values: SmeltList<f64>"), "{ts_source}");
    assert!(py_source.contains("let values: SmeltList<i64>"), "{py_source}");
}

#[test]
fn emits_untyped_json_parse_as_unknown_record() {
    let ts_source = source_for(
        r#"
const text = "{\"enabled\":true}";
const values = JSON.parse(text);
"#,
    );

    // A record destination is not `Deserialize`; the adapter parses into the
    // erased `SmeltUnknown` (which is) and the record is reached by coercion.
    assert!(ts_source.contains("smelt_json_parse(&"), "{ts_source}");
    assert!(
        ts_source.contains("serde_json::from_str::<SmeltUnknown>(text)"),
        "{ts_source}"
    );
    assert!(
        !ts_source.contains("serde_json::from_str::<SmeltRecord<String, SmeltUnknown>>(&")
            && !ts_source.contains("serde_json::from_str::<SmeltRecord<JsString, SmeltUnknown>>(&"),
        "{ts_source}"
    );
    assert!(ts_source.contains("impl<'de> serde::Deserialize<'de> for SmeltUnknown"));
}

#[test]
fn emits_json_parse_assertion_targets() {
    let ts_source = source_for(
        r#"
interface ServerConfig {
  host: string;
  port: number;
}

const text = "{\"host\":\"localhost\",\"port\":1337}";
const config = JSON.parse(text) as ServerConfig;
const bag = JSON.parse(text) as Record<string, unknown>;
"#,
    );

    assert!(
        ts_source.contains(
            "#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]"
        )
    );
    // Every destination -- a generated interface struct as much as a
    // `Record<string, unknown>` -- is decoded from the parsed dynamic value by
    // a generated strict decoder (`emitter::json_decode`), so there is one
    // parse shape and no non-`Deserialize` destination is ever named to
    // `serde`.
    assert!(
        ts_source.contains(": ServerConfig = smelt_json_decode_"),
        "{ts_source}"
    );
    assert!(
        ts_source.contains(": SmeltRecord<JsString, SmeltUnknown> = smelt_json_decode_"),
        "{ts_source}"
    );
    assert!(ts_source.contains("smelt_json_parse(text)?"), "{ts_source}");
    assert!(
        ts_source.contains("serde_json::from_str::<SmeltUnknown>(text)"),
        "{ts_source}"
    );
    assert!(
        !ts_source.contains("serde_json::from_str::<SmeltRecord<String, SmeltUnknown>>(&")
            && !ts_source.contains("serde_json::from_str::<SmeltRecord<JsString, SmeltUnknown>>(&"),
        "{ts_source}"
    );
    assert!(ts_source.contains("impl<'de> serde::Deserialize<'de> for SmeltUnknown"));
}

/// A declared `JSON.parse` target emits a strict per-type decoder into the
/// target's CONCRETE Rust type (type preservation): the struct is built field
/// by field from checked JSON kinds, a literal property is compared by value,
/// optional / nullable properties follow their declared nullish arms, and the
/// mismatch is thrown as a `TypeError` record.
#[test]
fn emits_strict_json_decoders_for_declared_targets() {
    let ts_source = source_for(
        r#"
type Mode = "fast" | "slow";
interface Item { sku: string; quantity: number }
interface Order {
  kind: "order";
  mode: Mode;
  note?: string;
  discount: number | null;
  items: Item[];
  totals: Record<string, number>;
}
const order: Order = JSON.parse("{}");
"#,
    );

    // The call site binds the concrete struct straight from the entry adapter;
    // no `SmeltUnknown` is stored past the boundary.
    assert!(
        ts_source.contains(": Order = smelt_json_decode_"),
        "{ts_source}"
    );
    assert!(
        ts_source.contains("-> Result<Order, Box<dyn ::std::error::Error>>"),
        "{ts_source}"
    );
    // Per-type decoders answer the concrete storage types.
    assert!(
        ts_source.contains("path: &mut String) -> Result<Order, SmeltDecodeIssue>"),
        "{ts_source}"
    );
    assert!(
        ts_source.contains("path: &mut String) -> Result<SmeltList<Item>, SmeltDecodeIssue>"),
        "{ts_source}"
    );
    assert!(
        ts_source.contains("path: &mut String) -> Result<SmeltRecord<JsString, f64>, SmeltDecodeIssue>"),
        "{ts_source}"
    );
    // A number is a JSON number: no string coercion.
    assert!(
        ts_source.contains(
            "match value { SmeltUnknown::Number(number) => Ok(*number), other => smelt_decode_fail(path, \"number\", other) }"
        ),
        "{ts_source}"
    );
    // Literal properties are checked by value, including through an alias.
    assert!(
        ts_source.contains("matches!(&field, SmeltUnknown::String(text) if text.eq_str(\"order\"))"),
        "{ts_source}"
    );
    assert!(
        ts_source.contains(
            "matches!(&field, SmeltUnknown::String(text) if text.eq_str(\"fast\")) || matches!(&field, SmeltUnknown::String(text) if text.eq_str(\"slow\"))"
        ),
        "{ts_source}"
    );
    // `note?: string` may be absent but not null; `discount: number | null`
    // may be null but not absent.
    assert!(
        ts_source.contains(
            "match object.get(\"note\") { Some(field) => { Some(smelt_decode_string(&field, path)"
        ),
        "{ts_source}"
    );
    assert!(
        ts_source.contains("None => None };"),
        "{ts_source}"
    );
    assert!(
        ts_source.contains(
            "match object.get(\"discount\") { Some(SmeltUnknown::Null) => None, Some(field) =>"
        ),
        "{ts_source}"
    );
    assert!(
        ts_source.contains(
            "None => return smelt_decode_fail(path, \"number | null\", &SmeltUnknown::Undefined)"
        ),
        "{ts_source}"
    );
    // Undeclared properties have no slot: the struct literal names exactly the
    // declared fields.
    assert!(
        ts_source.contains("Ok(Order { kind: field_0, mode: field_1, note: field_2, discount: field_3, items: field_4, totals: field_5 })"),
        "{ts_source}"
    );
    // The mismatch is a catchable `TypeError` record on the throw channel.
    assert!(
        ts_source.contains("SmeltUnknown::String(\"TypeError\".into())"),
        "{ts_source}"
    );
}

/// A property smelt already stores as a dynamic value (an intersection alias
/// names no generated struct) is passed through unchecked, so it does not
/// switch the checks off for the rest of the object; the same alias as the
/// WHOLE target keeps the dynamic parse. A plain object-literal alias lowers
/// to an interface struct, so the fixture spells the dynamic record as an
/// intersection.
#[test]
fn passes_dynamically_stored_properties_through_unchecked() {
    let ts_source = source_for(
        r#"
type Pair = { a: number } & { b: string };
interface Holder { id: number; pair: Pair }
const holder: Holder = JSON.parse("{}");
const bare: Pair = JSON.parse("{}");
"#,
    );
    assert!(
        ts_source.contains(": Holder = smelt_json_decode_"),
        "{ts_source}"
    );
    assert!(
        ts_source.contains("match object.get(\"pair\") { Some(field) => { smelt_decode_unknown(&field, path)? }"),
        "{ts_source}"
    );
    assert!(
        ts_source.contains("match object.get(\"id\") { Some(field) => { smelt_decode_number(&field, path)? }"),
        "{ts_source}"
    );
    assert!(
        ts_source.contains(": SmeltUnknown = smelt_json_parse(&"),
        "{ts_source}"
    );
}

/// A target with no decodable shape -- a class (prototype identity and
/// methods) -- keeps the existing dynamic parse and its coercion, and Python's
/// `json.loads` never decodes.
#[test]
fn keeps_dynamic_json_parse_for_undecodable_targets() {
    let ts_source = source_for(
        r#"
class Counter {
  count = 0;
  bump(): number { return this.count + 1; }
}
const counter = JSON.parse("{}") as Counter;
"#,
    );
    assert!(!ts_source.contains("smelt_json_decode_"), "{ts_source}");
    assert!(ts_source.contains("smelt_json_parse(&"), "{ts_source}");

    let py_source = source_for_py(
        r#"
import json
values: list[int] = json.loads("[1]")
"#,
    );
    assert!(!py_source.contains("smelt_json_decode_"), "{py_source}");
}

#[test]
fn emits_set_constructor_and_has_method() {
    let source = source_for(
        r"
const values: Set<number> = new Set([1, 2, 3]);
const has = values.has(2);
const empty: Set<string> = new Set();
const source: number[] = [1, 2, 3];
const fromSource = new Set(source);
",
    );

    // Both set backings are insertion-ordered: a `Set<number>` (f64
    // elements, no Rust `Eq`/`Hash`) uses `SmeltJsSet` with SameValueZero
    // membership, and a `Set<string>` uses `SmeltPrimSet`, which hashes the
    // values directly and so does not drag in the erased-value carrier.
    assert!(source.contains("let values: SmeltJsSet<f64>"), "{source}");
    assert!(source.contains("let empty: SmeltPrimSet<JsString>"), "{source}");
    assert!(source.contains(".contains(&2.0)"));
    assert!(source.contains("SmeltPrimSet::new();"));
    assert!(source.contains(".iter().cloned().collect::<SmeltJsSet<_>>()"));
}

#[test]
fn emits_set_has_inside_callback() {
    let source = source_for(
        r"
const selected: Set<number> = new Set([1, 2]);
const values: number[] = [1, 2, 3];
const hits = values.filter((value) => selected.has(value));
",
    );

    // The membership test only reads the needle, so it borrows the callback
    // parameter rather than cloning it and borrowing the copy. See
    // `shared_reference_argument_expr`.
    assert!(source.contains(".contains(&closure_arg_0)"), "{source}");
    assert!(
        !source.contains(".contains(&closure_arg_0.clone())"),
        "a set membership test must not clone its needle:\n{source}"
    );
}

#[test]
fn emits_python_pytest_function_as_rust_test() {
    let source = source_for_py_path(
        r"
def test_truth():
    assert True
",
        "tests/test_truth.py",
    );

    assert!(source.contains("#[test]\nfn test_truth()"), "{source}");
    assert_eq!(rust_fn(&source, "test_truth"), r#"fn test_truth() -> Result<(), Box<dyn std::error::Error>> {
let _smelt_tmp_0: bool = !(true);
if _smelt_tmp_0 {
return Err::<_, Box<dyn std::error::Error>>(std::io::Error::new(std::io::ErrorKind::Other, format!("{}", "assertion failed".to_owned())).into());
} else {
return Ok(());
}
}"#);
}

#[test]
fn lowers_bare_array_call_like_new_array() {
    // `Array(total)` and `new Array(total)` are identical in ECMAScript; the
    // bare-call form is the dominant es-toolkit preallocation idiom (e.g.
    // `const result = Array(total); result[i] = ...`). Both lower to a list
    // container that subsequent indexed writes fill.
    let source = source_for(
        r"
export function build(total: number): number[] {
  const result: number[] = Array(total);
  for (let i = 0; i < total; i++) {
    result[i] = i;
  }
  return result;
}
",
    );

    assert_eq!(rust_fn(&source, "build"), r#"fn build(total: f64) -> SmeltList<f64> {
let mut _smelt_tmp_4: bool;
let mut _smelt_tmp_5: f64;
let mut result: SmeltList<f64> = Into::<SmeltList<_>>::into({ let smelt_array_from_length = (total as f64).max(0.0).floor() as usize; (0..smelt_array_from_length).map(|_| 0.0).collect::<Vec<_>>() });
let mut i: f64 = 0.0;
loop {
_smelt_tmp_4 = i < total;
if !(_smelt_tmp_4) { break; }
{ let smelt_assign_index = { let smelt_normalized = i as i64; usize::try_from(smelt_normalized).expect("negative index out of bounds") }; let smelt_assign_value = i; if smelt_assign_index >= result.len() { result.borrow_mut().resize(smelt_assign_index.saturating_add(1), 0.0); } result.borrow_mut()[smelt_assign_index] = smelt_assign_value; }
_smelt_tmp_5 = i + 1.0;
i = _smelt_tmp_5;
}
return result;
}"#);
}

#[test]
fn array_length_allocation_holes_match_out_of_range_reads() {
    // A hole is the element type's MISSING VALUE — the very value an
    // out-of-range read of the same list answers — so construction and reads
    // cannot disagree. A `number[]` allocation therefore stays a `Vec<f64>` of
    // `0.0` holes instead of being erased to `SmeltUnknown` and mapped back.
    let source = source_for(
        r"
export function numbers(n: number): number[] {
  const result: number[] = Array(n);
  return result;
}
export function strings(n: number): string[] {
  const result: string[] = new Array(n);
  return result;
}
",
    );

    assert_eq!(rust_fn(&source, "numbers"), r"fn numbers(n: f64) -> SmeltList<f64> {
let result: SmeltList<f64> = Into::<SmeltList<_>>::into({ let smelt_array_from_length = (n as f64).max(0.0).floor() as usize; (0..smelt_array_from_length).map(|_| 0.0).collect::<Vec<_>>() });
return result;
}");
    assert_eq!(rust_fn(&source, "strings"), r"fn strings(n: f64) -> SmeltList<JsString> {
let result: SmeltList<JsString> = Into::<SmeltList<_>>::into({ let smelt_array_from_length = (n as f64).max(0.0).floor() as usize; (0..smelt_array_from_length).map(|_| JsString::new()).collect::<Vec<_>>() });
return result;
}");
}
