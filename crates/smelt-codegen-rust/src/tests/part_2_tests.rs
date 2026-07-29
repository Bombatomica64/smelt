//! Split codegen tests chunk.

use super::*;

#[test]
fn emits_typescript_number_to_string_method() {
    let source = source_for(
        r"
const value = 42;
const text = value.toString();
",
    );

    assert!(source.contains(".to_string()"));
}

#[test]
fn emits_invalid_date_to_string_as_invalid_date_text() {
    let source = source_for(
        r"
function stringify(result: Date): string {
  return result.toString();
}
",
    );

    assert!(source.contains("\"Invalid Date\".to_owned()"), "{source}");
    assert!(source.contains("timestamp_ms.is_finite()"), "{source}");
}

#[test]
fn emits_typescript_number_parse_float() {
    let source = source_for(
        r#"
const value = Number.parseFloat("42.5");
"#,
    );

    assert!(source.contains(".parse::<f64>().unwrap_or(f64::NAN)"));
}

#[test]
fn emits_parse_float_string_coercion_for_erased_inputs() {
    let source = source_for(
        r#"
function parseValue(value: any): number {
  return parseFloat(value);
}
"#,
    );
    let program = source
        .split_once(PRELUDE_END_MARKER)
        .map_or(source.as_str(), |(_, program)| program);

    assert!(
        program.contains("SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value"),
        "expected erased input to pass through JavaScript string coercion: {program}"
    );
    assert!(
        program.contains(".parse::<f64>().unwrap_or(f64::NAN)"),
        "expected the coerced string to feed parseFloat: {program}"
    );
}

#[test]
fn emits_typescript_number_parse_int() {
    let source = source_for(
        r#"
const value = Number.parseInt("42");
"#,
    );

    assert!(source.contains(".parse::<i64>().map(|value| value as f64).unwrap_or(f64::NAN)"));
}

#[test]
fn optional_arrow_parameter_is_nullable_for_undefined_comparison() {
    // An optional arrow parameter (`arg2?: number`) has type `number | undefined`
    // inside the body, so it must lower to `Option<f64>` and an `arg2 === undefined`
    // guard must compare against `None` rather than constant-folding to `false`
    // (which is what happened when the optional marker was dropped).
    let source = source_for(
        r"
const isMissing = (_: number, arg2?: number): boolean => arg2 === undefined;
const result = isMissing(1);
",
    );

    assert!(
        source.contains("Option<f64>"),
        "expected optional arrow parameter to lower as Option<f64>: {source}"
    );
}

#[test]
fn extracts_borrowed_predicate_argument_from_dynamic_spread_dispatch() {
    // A borrowed `&dyn Fn(..) -> bool` parameter fed from a dynamic `SmeltUnknown`
    // (here a `...args` rest element forwarded by spread) must recover the real
    // callable and reborrow it, instead of substituting a no-op default predicate
    // that always returns `false`.
    let source = source_for(
        r"
const impl = (
  data: number,
  predicate: (value: number) => boolean,
  ...rest: readonly number[]
): number => (predicate(data) ? data : rest[0]);

function dispatch(...args: readonly unknown[]): unknown {
  // @ts-expect-error -- exercises dynamic dispatch through erased arguments
  return impl(...args);
}
",
    );

    // The narrowed callable's source value is hoisted into a
    // `let smelt_source_value = ..` binding (so a callable object can be
    // recovered on re-erasure) before the callable is matched and reborrowed.
    assert!(
        source.contains("&*({ let smelt_source_value =")
            && source.contains("let smelt_function = match smelt_source_value"),
        "expected borrowed predicate argument to reborrow an extracted callable: {source}"
    );
    assert!(
        source.contains("smelt_restore_function_origin::<::std::rc::Rc<dyn Fn(f64) -> bool>>"),
        "expected the dynamic predicate to be extracted to a typed bool callback: {source}"
    );
}

#[test]
fn emits_typescript_infinity_identifier() {
    let source = source_for(
        r"
const upper = Infinity;
const lower = -Infinity;
const missing = NaN;
",
    );

    assert!(source.contains("f64::INFINITY"));
    assert!(source.contains("f64::NAN"));
}

#[test]
fn emits_typescript_instanceof_as_class_check() {
    let source = source_for(
        r"
class Box {
  constructor() {}
}
class Other {
  constructor() {}
}
const value = new Box();
const yes = value instanceof Box;
const no = value instanceof Other;
",
    );

    assert!(source.contains("true"));
    assert!(source.contains("false"));
}

#[test]
fn emits_typescript_global_numeric_parse_calls() {
    let source = source_for(
        r#"
const intValue = parseInt("42");
const floatValue = parseFloat("42.5");
"#,
    );

    assert!(source.contains(".parse::<i64>().map(|value| value as f64).unwrap_or(f64::NAN)"));
    assert!(source.contains(".parse::<f64>().unwrap_or(f64::NAN)"));
}

#[test]
fn emits_number_predicate_calls() {
    let source = source_for(
        r"
const value = 4;
const finite = Number.isFinite(value);
const nan = Number.isNaN(value);
",
    );

    assert!(source.contains(".is_finite();"));
    assert!(source.contains(".is_nan();"));
}

#[test]
fn emits_nan_predicate_for_erased_date_numeric_getter() {
    let source = source_for(
        r"
function invalid<ResultDate extends Date>(value: ResultDate): boolean {
  return isNaN(value.getTime());
}
",
    );

    // Scope the negative assertion to the generated program: a bounded generic
    // free function now emits the `SmeltUnknown` prelude (whose `for...in`
    // helpers legitimately contain `return false;`), so the assertion must look
    // only at the emitted `invalid` body, not the runtime prelude.
    let program = source
        .split_once(crate::PRELUDE_END_MARKER)
        .map_or(source.as_str(), |(_, program)| program);
    assert!(program.contains(".is_nan()"), "{source}");
    assert!(!program.contains("return false;"), "{source}");
}

#[test]
fn emits_nan_predicate_for_optional_numeric_and_date_values() {
    let source = source_for(
        r"
function numeric(value: number | undefined): boolean {
  return value != null && isNaN(value);
}

function optionalResult(): number | undefined {
  return undefined;
}

function numericResult(): boolean {
  const value = optionalResult();
  return value != null && isNaN(value);
}

function dateValue(value: Date | undefined): boolean {
  return value instanceof Date && isNaN(value.getTime());
}
",
    );

    assert!(source.contains("unwrap_or(f64::NAN)"), "{source}");
    assert!(source.contains(".map_or(f64::NAN"), "{source}");
    assert!(source.contains(".is_nan()"), "{source}");
    assert!(source.contains("if "), "{source}");
}

#[test]
fn emits_runtime_date_identity_for_unknown_instanceof_guard() {
    let source = source_for(
        r"
function isDate(value: unknown): boolean {
  return value instanceof Date;
}

const candidate = new Date(NaN);
const date = isDate(candidate);
const number = isDate(1);
",
    );

    assert!(source.contains("\"__smelt_date\".to_owned()"), "{source}");
    assert!(
        source.contains("value.contains_key(\"__smelt_date\")"),
        "{source}"
    );
}

#[test]
fn emits_runtime_blob_identity_for_unknown_instanceof_guard() {
    let source = source_for(
        r#"
function isBlob(value: unknown): boolean {
  if (typeof Blob === "undefined") {
    return false;
  }
  return value instanceof Blob;
}

const candidate = new Blob(["content"], { type: "text/plain" });
const yes = isBlob(candidate);
const no = isBlob(1);
"#,
    );

    assert!(source.contains("\"__smelt_blob\".to_owned()"), "{source}");
    assert!(
        source.contains("value.contains_key(\"__smelt_blob\")"),
        "{source}"
    );
}

#[test]
fn emits_blob_record_helper_for_file_constructor() {
    let source = source_for(
        r#"
const file = new File(["content"], "file.txt", { type: "text/plain" });
const isFile = file instanceof File;
const isBlob = file instanceof Blob;
"#,
    );

    // Construction routes through the shared runtime helper (which stamps
    // `__smelt_file` on top of `__smelt_blob`), and both instanceof checks
    // resolve through their marker keys.
    assert!(
        source.contains("fn smelt_blob_record_from_parts("),
        "{source}"
    );
    assert!(source.contains("smelt_blob_record_from_parts(("), "{source}");
    assert!(
        source.contains("value.contains_key(\"__smelt_file\")"),
        "{source}"
    );
    assert!(
        source.contains("value.contains_key(\"__smelt_blob\")"),
        "{source}"
    );
}

#[test]
fn omits_blob_record_helper_without_blob_construction() {
    let source = source_for("const value: number = 1;");
    assert!(
        !source.contains("smelt_blob_record_from_parts"),
        "the Blob/File record helper must stay gated behind actual Blob/File construction: {source}"
    );
}

#[test]
fn emits_runtime_boxed_number_identity_for_unknown_instanceof_guard() {
    let source = source_for(
        r"
function isBoxedNumber(value: unknown): boolean {
  return value instanceof Number;
}

const boxed = new Number(42);
const yes = isBoxedNumber(boxed);
const no = isBoxedNumber(42);
",
    );

    assert!(source.contains("\"__smelt_number\".to_owned()"), "{source}");
    assert!(
        source.contains("value.contains_key(\"__smelt_number\")"),
        "{source}"
    );
}

#[test]
fn emits_vitest_to_be_nan_using_same_value_semantics() {
    let source = source_for(
        r#"
import { test, expect } from "vitest";

test("NaN equality", () => {
  const value = NaN;
  expect(value).toBe(NaN);
  expect(Object.is(value, NaN)).toBe(true);
});
"#,
    );

    assert!(source.contains(".is_nan() &&"), "{source}");
    assert!(!source.contains("!= f64::NAN"), "{source}");
    assert!(!source.contains("== f64::NAN"), "{source}");
}

#[test]
fn emits_timezone_context_that_preserves_nan_values() {
    let source = source_for(
        r#"
import { tz } from "@date-fns/tz";
const context = tz("Asia/Singapore");
"#,
    );

    assert!(source.contains("if timestamp_ms.is_finite()"), "{source}");
    assert!(source.contains("else { f64::NAN }"), "{source}");
}

#[test]
fn emits_optional_date_type_parameter_or_nan_as_selected_value() {
    let source = source_for(
        r"
function select<ResultDate extends Date>(
  result: ResultDate | undefined,
): unknown {
  return result || NaN;
}
",
    );

    assert!(source.contains(".map_or_else("), "{source}");
    assert!(source.contains("f64::NAN"), "{source}");
    assert!(!source.contains("let _smelt_tmp_1: bool"), "{source}");
}

#[test]
fn emits_negated_optional_date_type_parameter_as_presence_check() {
    let source = source_for(
        r"
function absent<ResultDate extends Date>(
  result: ResultDate | undefined,
): boolean {
  return !result;
}
",
    );

    // Scope assertions to the generated program: a bounded generic free
    // function now emits the `SmeltUnknown` prelude, whose match arms mention
    // `Some(SmeltUnknown::Number(value))`, so the negative assertion must look
    // only at the emitted `absent` body rather than the runtime prelude.
    let program = source
        .split_once(crate::PRELUDE_END_MARKER)
        .map_or(source.as_str(), |(_, program)| program);
    assert!(
        program.contains(
            "result.clone().as_ref().map_or(true, |value| matches!(value, SmeltUnknown::Null | SmeltUnknown::Undefined))"
        ),
        "{source}"
    );
    assert!(
        !program.contains("Some(SmeltUnknown::Number(value))"),
        "{source}"
    );
}

#[test]
fn emits_python_string_search_as_int() {
    let source = source_for_py(
        r#"
word: str = "Smelt"
first: int = word.find("m")
last: int = word.rfind("t")
"#,
    );

    assert!(source.contains(".find(&"));
    assert!(source.contains(".rfind(&"));
    assert!(source.contains(".map_or(-1,"));
}

#[test]
fn emits_python_list_and_string_slices() {
    let source = source_for_py(
        r#"
values: list[int] = [1, 2, 3, 4]
all_values: list[int] = values[:]
tail_values: list[int] = values[1:]
mid_values: list[int] = values[1:3]
last_values: list[int] = values[-2:]
word: str = "smelting"
all_text: str = word[:]
tail_text: str = word[1:]
mid_text: str = word[1:4]
last_text: str = word[-3:]
"#,
    );

    assert!(source.contains(".iter().skip(0usize).take("));
    assert!(source.contains("let index = 1 as i64"));
    assert!(source.contains("clamp(0, len) as usize"));
    assert!(source.contains(".cloned().collect::<Vec<_>>()"));
    assert!(source.contains(".chars().skip(0usize).take("));
    assert!(source.matches("if index < 0").count() >= 2);
    assert!(source.contains(".collect::<String>();"));
}

#[test]
fn emits_python_negative_list_and_string_indexes() {
    let source = source_for_py(
        r#"
values: list[int] = [1, 2, 3]
last_value: int = values[-1]
word: str = "abc"
last_char: str = word[-1]
"#,
    );

    assert!(source.contains("let normalized = if index < 0 { len + index } else { index }"));
    assert!(source.contains(".get({ let len = values.len() as i64;"));
    assert!(source.contains(".chars().nth({ let len = word.chars().count() as i64;"));
}

#[test]
fn emits_python_tuple_index_and_slice() {
    let source = source_for_py(
        r#"
pair: tuple[str, int] = ("Ada", 1)
name: str = pair[0]
rank: int = pair[-1]
tail: tuple[int] = pair[1:]
empty: tuple[()] = pair[:0]
"#,
    );

    assert!(source.contains(".0.clone()"));
    assert!(source.contains(".1.clone()"));
    assert!(source.contains(".1.clone(),)"));
    assert!(source.contains(": () = ();"));
}

#[test]
fn emits_typescript_tuple_index() {
    let source = source_for(
        r#"
const pair: [string, number] = ["Ada", 1];
const name = pair[0];
const count = pair[1];
"#,
    );

    assert!(source.contains(".0.clone();"));
    assert!(source.contains(".1.clone();"));
}

#[test]
fn emits_python_list_append_method() {
    let source = source_for_py(
        r"
values: list[int] = [1, 2]
result: None = values.append(3)
",
    );

    assert!(source.contains("let mut"));
    assert!(source.contains("Vec<i64>"));
    assert!(source.contains(".push(3);"));
    assert!(source.contains("()"));
}

#[test]
fn emits_python_list_extend_method() {
    let source = source_for_py(
        r"
left: list[int] = [1, 2]
right: list[int] = [3, 4]
result: None = left.extend(right)
",
    );

    assert!(source.contains("let mut"));
    assert!(source.contains(".extend("));
    assert!(source.contains(".iter().cloned());"));
    assert!(source.contains("()"));
}

#[test]
fn emits_python_list_insert_method() {
    let source = source_for_py(
        r"
values: list[int] = [1, 2]
result: None = values.insert(1, 0)
",
    );

    assert!(source.contains("let mut"));
    assert!(source.contains("let insert_index = usize::try_from(1)"));
    assert!(source.contains(".insert(insert_index, 0);"));
    assert!(source.contains("()"));
}

#[test]
fn emits_python_list_reverse_method() {
    let source = source_for_py(
        r"
values: list[int] = [1, 2]
result: None = values.reverse()
",
    );

    assert!(source.contains("let mut"));
    assert!(source.contains(".reverse();"));
    assert!(source.contains("()"));
}

#[test]
fn emits_python_list_pop_method() {
    let source = source_for_py(
        r"
values: list[int] = [1, 2]
item: int = values.pop()
",
    );

    assert!(source.contains("let mut"));
    assert!(source.contains(".pop().expect(\"pop from empty list\")"));
}

#[test]
fn emits_python_collection_clear_methods() {
    let source = source_for_py(
        r#"
values: list[int] = [1, 2]
list_result: None = values.clear()
mapping: dict[str, int] = {"a": 1}
dict_result: None = mapping.clear()
"#,
    );

    assert!(source.contains("let mut"));
    assert!(source.matches(".clear();").count() >= 2);
    assert!(source.matches("()").count() >= 2);
}

#[test]
fn emits_missing_class_record_reads_as_optional_values() {
    let source = source_for(
        r"
class Parser {
  run(): number { return 1; }
}
function read(parsers: Record<string, Parser>, key: string): number {
  const parser = parsers[key];
  if (parser) {
    return parser.run();
  }
  return 0;
}
",
    );

    assert!(source.contains(".get(&key.clone()).cloned()"), "{source}");
    assert!(
        !source.contains("expect(\"index out of bounds\")"),
        "{source}"
    );
    assert!(
        source.contains("expect(\"optional value was absent after narrowing\")"),
        "{source}"
    );
}

#[test]
fn emits_nullable_module_array_reads_without_negative_index_panics() {
    let source = source_for(
        r"
const daysInMonths = [31, null, 31];
function read(month: number): number | undefined {
  return daysInMonths[month];
}
",
    );

    assert!(
        source.contains("vec![Some(31.0), None::<f64>, Some(31.0)]"),
        "{source}"
    );
    assert!(
        source.contains("usize::try_from(normalized).ok()"),
        "{source}"
    );
    assert!(!source.contains("negative index out of bounds"), "{source}");
}

#[test]
fn emits_array_at_as_optional_index_without_negative_index_panics() {
    let source = source_for(
        r"
function last<T>(values: readonly T[]): T | undefined {
  return values.at(-1);
}
",
    );

    assert!(
        source.contains("usize::try_from(normalized).ok()"),
        "{source}"
    );
    assert!(!source.contains("negative index out of bounds"), "{source}");
    assert!(source.contains(".and_then(|index|"), "{source}");
}

#[test]
fn guards_callback_function_table_calls_selected_through_a_local() {
    let source = source_for(
        r"
type Formatter = (value: string) => string;
const lower: Formatter = (value) => value.toLowerCase();
export const table: Record<string, Formatter> = { a: lower };
export function apply(values: string[]): string[] {
  return values.map((value) => {
    const key = value[0];
    const formatter = table[key];
    if (formatter) {
      return formatter(value);
    }
    return value;
  });
}
",
    );

    assert!(source.contains("== \"a\".to_owned()"), "{source}");
    assert!(source.contains("if "), "{source}");
    assert!(source.contains("unknown function table key"), "{source}");
}

#[test]
fn emits_encode_uri_helper_for_encode_uri_usage() {
    let source = source_for(
        r#"
const encoded = encodeURI("https://ex.com/a b?x=1&y=2#frag");
const asValue = encodeURI;
"#,
    );

    // Both the direct call and the first-class value form route through the
    // shared percent-encoding runtime helper.
    assert!(source.contains("fn smelt_encode_uri("), "{source}");
    assert!(source.contains("= smelt_encode_uri("), "{source}");
}

#[test]
fn omits_encode_uri_helper_without_usage() {
    let source = source_for("const value: number = 1;");
    assert!(
        !source.contains("smelt_encode_uri"),
        "the encodeURI helper must stay gated behind actual encodeURI usage: {source}"
    );
}

#[test]
fn emits_object_to_string_tag_probe_for_prototype_to_string_call() {
    let source = source_for(
        r"
export function tag(value: unknown): string {
  return Object.prototype.toString.call(value);
}
",
    );

    // The classic `"[object Tag]"` probe resolves through the runtime helper
    // (variant plus host identity markers), not through field reads on the
    // prototype sentinel.
    assert!(source.contains("fn smelt_object_to_string_tag("), "{source}");
    assert!(source.contains("= smelt_object_to_string_tag(&("), "{source}");
}

#[test]
fn emits_structured_clone_deep_copy_for_erased_value() {
    let source = source_for(
        r"
export function copy(value: unknown): unknown {
  return structuredClone(value);
}
",
    );

    // `structuredClone` on an erased value routes through the runtime deep-clone
    // helper (fresh identities, markers preserved), not an identity pass-through.
    assert!(source.contains("fn smelt_structured_clone("), "{source}");
    assert!(source.contains("smelt_structured_clone("), "{source}");
}

#[test]
fn structured_clone_of_concrete_value_stays_pass_through() {
    let source = source_for(
        r"
interface Point { x: number; y: number; }
export function copy(value: Point): Point {
  return structuredClone(value);
}
",
    );

    // A concretely typed argument keeps its static shape: HIR values are
    // immutable, so no runtime deep-clone helper call is emitted for it (the
    // erased deep-clone helper is reserved for genuinely dynamic `unknown`
    // values, per the SmeltUnknown boundary rules).
    assert!(
        !source.contains("smelt_structured_clone("),
        "concretely typed structuredClone must not route through the erased deep-clone helper: {source}"
    );
}

#[test]
fn emits_reflected_constructor_prototype_for_host_markers() {
    let source = source_for(
        r"
export function proto(value: unknown): unknown {
  return Object.getPrototypeOf(value);
}
",
    );

    // Host-marker objects (Date/Map/Set/RegExp/Error/...) expose a cached
    // per-kind prototype whose `constructor` slot is a real callable, so
    // es-toolkit `clone`'s `new Constructor(obj)` rebuilds the value.
    assert!(source.contains("fn smelt_reflected_prototype("), "{source}");
    assert!(source.contains("fn smelt_reflected_construct("), "{source}");
    assert!(source.contains("fn smelt_reflected_marker_kind("), "{source}");
}

#[test]
fn map_carries_identity_marker_across_erasure_boundary() {
    // A `Map` erased to `unknown` must keep its Map-ness: no concrete type,
    // union, or scoped generic can carry the `[object Map]` identity across the
    // dynamic boundary, so the erasure adapter stamps a `__smelt_map` marker
    // object that the runtime tag helper and `SmeltFromUnknown` round-trip read.
    let source = source_for(
        r"
export function erase(): unknown {
  const m = new Map<string, number>([['a', 1]]);
  return m;
}
",
    );

    // The Map erasure adapter stamps the marker (identity boundary).
    assert!(
        source.contains("IntoSmeltUnknown for SmeltJsMap")
            && source.contains("\"__smelt_map\".to_owned()"),
        "Map must erase to a __smelt_map marker object: {source}"
    );
    // The tag helper reports [object Map]/[object Set] off the markers.
    assert!(
        source.contains("if map.contains_key(\"__smelt_map\") { return \"[object Map]\".to_owned(); }")
            && source.contains(
                "if map.contains_key(\"__smelt_set\") { return \"[object Set]\".to_owned(); }"
            ),
        "toStringTag must have Map/Set marker arms: {source}"
    );
    // The un-erase adapter restores entries and identity from the marker.
    assert!(
        source.contains("if let Some(SmeltUnknown::Array(pairs)) = object.get(\"__smelt_map\")"),
        "SmeltFromUnknown must round-trip the __smelt_map marker: {source}"
    );
    // The marker key stays hidden from for...in / Object.keys enumeration.
    assert!(
        source.contains("key != \"__smelt_map\" && key != \"__smelt_set\""),
        "for-in filter must hide the Map/Set markers: {source}"
    );
}

#[test]
fn emits_timer_prelude_for_set_timeout_value_form() {
    let source = source_for(
        r"
const scheduled = globalThis.setTimeout;
const id = scheduled(() => {}, 4);
",
    );

    // The timer op lives inside the synthesized first-class closure body, so
    // the prelude gate must scan closure rvalues too.
    assert!(source.contains("fn smelt_set_timeout("), "{source}");
}

#[test]
fn emits_error_record_with_cause_and_aggregate_errors() {
    let source = source_for(
        r#"
const error = new Error("boom", { cause: "root" });
const aggregate = new AggregateError([new Error("a")], "many");
const cause = error.cause;
"#,
    );

    // The ES2022 options form retains `cause` (and AggregateError's `errors`)
    // on the marker-bearing error record.
    assert!(source.contains("\"__smelt_error\""), "{source}");
    assert!(source.contains("\"cause\""), "{source}");
    assert!(source.contains("\"errors\""), "{source}");
}
