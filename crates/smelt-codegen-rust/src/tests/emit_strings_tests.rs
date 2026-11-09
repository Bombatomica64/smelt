//! Generated Rust for string operations.

use super::*;

#[test]
fn types_a_string_predicate_callback_result_as_a_bool() {
    // `StringAffix` yields a concrete `bool`. Typing the node with the
    // surrounding expectation instead declared the temporary `SmeltUnknown`
    // while the expression that filled it stayed a Rust `bool`, so the emitted
    // crate did not compile (E0308) -- and repairing that at the boundary would
    // still have left an erase-then-truthiness round trip for a value that was
    // statically boolean.
    let source = source_for(
        r#"
export function firstA(names: string[]): string | undefined {
  return names.find((name: string) => name.startsWith("a"));
}
"#,
    );

    assert!(
        source.contains("let _smelt_tmp_3: bool = closure_arg_0.clone().starts_with("),
        "{source}"
    );
    // Scoped to the source from `fn first_a(` onward: the runtime prelude
    // legitimately spells both `SmeltUnknown::Bool(..)` and the truthiness match.
    let body = source
        .split_once("fn first_a(")
        .map_or("", |(_, tail)| tail);
    assert!(
        !body.contains("SmeltUnknown"),
        "the predicate must not erase: {source}"
    );
}

#[test]
fn emits_optional_string_match_with_some_patterns() {
    let source = source_for(
        r#"
export function label(value?: string): string {
  switch (value) {
    case "a":
      return "A";
    default:
      return "other";
  }
}
"#,
    );

    assert!(
        source.contains("match value.clone().map(|smelt_text| smelt_text.to_string_lossy()).as_deref()"),
        "{source}"
    );
    assert!(source.contains("Some(\"a\") =>"), "{source}");
}

#[test]
fn emits_contextual_string_arrow_number_suffix_addition_as_concat() {
    let source = source_for(
        r#"
type LocalizeFn<Value> = (value: Value, options?: { unit?: string }) => string;
type Localize = {
  ordinalNumber: LocalizeFn<number>;
};

const feminineUnits = ["second", "minute"];

const ordinalNumber: LocalizeFn<number> = (dirtyNumber, options) => {
  const number = Number(dirtyNumber);
  const unit = options?.unit;
  if (number === 0) return "0";
  let suffix;
  if (number === 1) {
    suffix = unit && feminineUnits.includes(unit) ? "ère" : "er";
  } else {
    suffix = "ème";
  }
  return number + suffix;
};

export const localize: Localize = {
  ordinalNumber,
};
"#,
    );

    assert!(
        source.contains("smelt_number_to_string(number)) + &match suffix"),
        "{source}"
    );
    assert!(
        !source.contains("let _smelt_tmp_16: f64")
            && !source.contains("return _smelt_tmp_16.clone();"),
        "{source}"
    );
}

#[test]
fn emits_callback_string_concat_with_borrowed_rhs() {
    let source = source_for(
        r#"
function label(values: string[]): string[] {
  return values.map((value, index) => "" + (index === 0 ? value : value.toLowerCase()));
}
"#,
    );

    assert!(source.contains("JsString::new() + &_smelt_tmp_"), "{source}");
    assert!(
        source.contains("closure_arg_0.clone().to_lower_case()"),
        "{source}"
    );
}

#[test]
fn emits_case_conversion_for_erased_callback_string_indexes() {
    let source = source_for(
        r"
function initialCaps<T extends string>(values: T[]): string[] {
  return values.map((value) => value[0].toUpperCase());
}
",
    );

    assert!(source.contains("SmeltUnknown::String(value)"), "{source}");
    assert!(source.contains("smelt_value.char_at(smelt_position)"), "{source}");
    assert!(source.contains(".to_upper_case()"), "{source}");
    assert!(!source.contains("String::new().to_upper_case()") && !source.contains("JsString::new().to_upper_case()"), "{source}");
}

#[test]
fn keeps_unknown_conditionals_erased_before_string_compatible_fallbacks() {
    let source = source_for(
        r"
const value: unknown = 1;
const fallback: Date = new Date();
const selected: unknown = value ? value : fallback;
",
    );

    assert!(
        // The temp index is incidental (it moves with the fixture's own
        // statement count); the property is that the truthy arm yields the
        // erased value UNCHANGED and only the `Date` fallback is erased. Old
        // requirement: the fallback was re-tagged by a marker `match`; new
        // (`Date` is the concrete `SmeltDate`): it erases through the date's
        // own boundary adapter, which stamps the same `__smelt_date` marker.
        source.contains("{ value } else { fallback.clone().into_smelt_unknown() }"),
        "{source}"
    );
    assert!(
        !source.contains("selected = SmeltUnknown::String"),
        "{source}"
    );
}

#[test]
fn emits_radix_to_string_and_numeric_shift_surface() {
    let source = source_for(
        r"
const binary = (10n).toString(2);
const left = 1n << 8n;
const right = left >> 1n;
const pivot = (4 + 10) >>> 1;
const numberLeft = 1 << 31;
const numberRight = -8 >> 1;
function shiftRaw(raw: bigint): bigint {
  return raw >> 1n;
}
",
    );

    // `bigint.toString(radix)` keeps every digit: the runtime renders the
    // unbounded value in the radix (it used to narrow it to `i128` first). A
    // radix outside 2..=36 is a catchable `RangeError`, so it is a fallible
    // call ending in `?` (it used to panic past any enclosing `try`).
    assert!(
        source.contains("smelt_bigint_to_string_radix(&::num_bigint::BigInt::from(10_i64), 2.0).map(JsString::from).map(JsString::from)?"),
        "{source}"
    );
    // A `number` shift is JavaScript's int32 shift: `ToInt32(a)` shifted by
    // `ToUint32(b) & 31` (`1 << 31` is -2147483648 in Node).
    assert!(source.contains(".wrapping_shl("), "{source}");
    assert!(source.contains(".wrapping_shr("), "{source}");
    // A `bigint` shift is NOT: `1n << 8n` is `256n` and `(1n << 40n)` is
    // `1099511627776n` in Node, with no 32-bit wrap. This test first asserted
    // the opposite -- that `1n << 8n` was an int32 shift -- which encoded the
    // literal mistyping and made remeda's `asBigInt`
    // (`(result << 8n) + BigInt(byte)`) wrap after four bytes. It then
    // asserted an `i64` carrier shifted through `(x as f64) as i128`, which
    // dropped low bits past 2^53 and wrapped past 2^63. A bigint is now its
    // own unbounded type, `num_bigint::BigInt`, and its shift is the runtime's
    // arithmetic shift, never a float round trip. The shift is a fallible
    // call (a negative count shifts the other way and can pass the size cap,
    // a catchable `RangeError`), so `shift_raw` returns its bigint through
    // the error channel; it used to assert a bare `-> ::num_bigint::BigInt`.
    assert!(source.contains("let left: ::num_bigint::BigInt"), "{source}");
    assert!(
        source.contains(
            "fn shift_raw(raw: ::num_bigint::BigInt) -> Result<::num_bigint::BigInt, Box<dyn std::error::Error>>"
        ),
        "{source}"
    );
    assert!(
        source.contains("smelt_bigint_shr(&raw, &::num_bigint::BigInt::from(1_i64))?"),
        "{source}"
    );
    assert!(!source.contains("as i128) >>"), "{source}");
    assert!(source.contains("rem_euclid(4294967296.0)"), "{source}");
}

#[test]
fn erased_receiver_two_callback_match_stays_dynamic_not_string_match() {
    // Regression: an erased receiver whose `.match(okFn, errFn)` call shape is a
    // neverthrow-style two-callback dispatch must NOT be lowered to
    // `String.prototype.match`; only a single regex/string-pattern argument on an
    // erased receiver qualifies. Wrong arity routes to dynamic member dispatch.
    let source = source_for(
        r"
function fold(value: any): number {
  return value.match((ok: number) => ok, (_err: string) => 0);
}
",
    );

    assert!(!source.contains(".match_string(&"), "{source}");
}

#[test]
fn emits_javascript_string_split_empty_separator_and_limit_semantics() {
    let source = source_for(
        r#"
function splitChars(value: string): string[] {
  return value.split("");
}

function splitNegative(value: string): string[] {
  return value.split(",", -1);
}
"#,
    );

    assert!(source.contains("if map.contains_key(\"__smelt_unmodeled\") { return \"[object Unmodeled]\".to_owned(); } if let Some(SmeltUnknown::String(tag)) = map.get(\"__smelt_symbol_to_string_tag\") { return format!(\"[object {tag}]\"); } if map.contains_key(\"__smelt_date\") { return \"[object Date]\".to_owned(); } if map.contains_key(\"__smelt_regexp\") { return \"[object RegExp]\".to_owned(); } if map.contains_key(\"__smelt_error\") { return \"[object Error]\".to_owned(); } if map.contains_key(\"__smelt_global_object\") { return \"[object global]\".to_owned(); } if map.contains_key(\"__smelt_map\") { return \"[object Map]\".to_owned(); } if map.contains_key(\"__smelt_set\") { return \"[object Set]\".to_owned(); } if map.contains_key(\"__smelt_arguments\") { return \"[object Arguments]\".to_owned(); } if map.contains_key(\"__smelt_abortcontroller\") { return \"[object AbortController]\".to_owned(); } if map.contains_key(\"__smelt_abortsignal\") { return \"[object AbortSignal]\".to_owned(); } if map.contains_key(\"__smelt_arraybuffer\") { return \"[object ArrayBuffer]\".to_owned(); } if map.contains_key(\"__smelt_sharedarraybuffer\") { return \"[object SharedArrayBuffer]\".to_owned(); } if map.contains_key(\"__smelt_int8array\") { return \"[object Int8Array]\".to_owned(); } if map.contains_key(\"__smelt_uint8array\") { return \"[object Uint8Array]\".to_owned(); } if map.contains_key(\"__smelt_uint8clampedarray\") { return \"[object Uint8ClampedArray]\".to_owned(); } if map.contains_key(\"__smelt_int16array\") { return \"[object Int16Array]\".to_owned(); } if map.contains_key(\"__smelt_uint16array\") { return \"[object Uint16Array]\".to_owned(); } if map.contains_key(\"__smelt_int32array\") { return \"[object Int32Array]\".to_owned(); } if map.contains_key(\"__smelt_uint32array\") { return \"[object Uint32Array]\".to_owned(); } if map.contains_key(\"__smelt_float32array\") { return \"[object Float32Array]\".to_owned(); } if map.contains_key(\"__smelt_float64array\") { return \"[object Float64Array]\".to_owned(); } if map.contains_key(\"__smelt_bigint64array\") { return \"[object BigInt64Array]\".to_owned(); } if map.contains_key(\"__smelt_biguint64array\") { return \"[object BigUint64Array]\".to_owned(); } if map.contains_key(\"__smelt_buffer\") { return \"[object Uint8Array]\".to_owned(); } if map.contains_key(\"__smelt_dataview\") { return \"[object DataView]\".to_owned(); } if map.contains_key(\"__smelt_weakmap\") { return \"[object WeakMap]\".to_owned(); } if map.contains_key(\"__smelt_weakset\") { return \"[object WeakSet]\".to_owned(); } if map.contains_key(\"__smelt_file\") { return \"[object File]\".to_owned(); } if map.contains_key(\"__smelt_blob\") { return \"[object Blob]\".to_owned(); } if map.contains_key(\"__smelt_headers\") { return \"[object Headers]\".to_owned(); } if map.contains_key(\"__smelt_urlsearchparams\") { return \"[object URLSearchParams]\".to_owned(); } if map.contains_key(\"__smelt_url\") { return \"[object URL]\".to_owned(); } if map.contains_key(\"__smelt_crypto_key\") { return \"[object CryptoKey]\".to_owned(); } if map.contains_key(\"__smelt_request\") { return \"[object Request]\".to_owned(); } if map.contains_key(\"__smelt_response\") { return \"[object Response]\".to_owned(); } if map.contains_key(\"__smelt_textencoder\") { return \"[object TextEncoder]\".to_owned(); } if map.contains_key(\"__smelt_textdecoder\") { return \"[object TextDecoder]\".to_owned(); } if map.contains_key(\"__smelt_eventemitter\") { return \"[object EventEmitter]\".to_owned(); } if map.contains_key(\"__smelt_httpserver\") { return \"[object Server]\".to_owned(); } if map.contains_key(\"__smelt_incomingmessage\") { return \"[object IncomingMessage]\".to_owned(); } if map.contains_key(\"__smelt_serverresponse\") { return \"[object ServerResponse]\".to_owned(); } if map.contains_key(\"__smelt_formdata\") { return \"[object FormData]\".to_owned(); } if map.contains_key(\"__smelt_databasesync\") { return \"[object DatabaseSync]\".to_owned(); } if map.contains_key(\"__smelt_statementsync\") { return \"[object StatementSync]\".to_owned(); } if map.contains_key(\"__smelt_readablestream\") { return \"[object ReadableStream]\".to_owned(); } if map.contains_key(\"__smelt_domexception\") { return \"[object DOMException]\".to_owned(); } if map.contains_key(\"__smelt_intl_collator\") { return \"[object Intl.Collator]\".to_owned(); } if map.contains_key(\"__smelt_intl_displaynames\") { return \"[object Intl.DisplayNames]\".to_owned(); } if map.contains_key(\"__smelt_intl_durationformat\")"), "{source}");
    assert!(
        source.contains("smelt_haystack.split(&smelt_separator, usize::MAX)"),
        "{source}"
    );
    assert!(
        source.contains("else if smelt_limit.is_sign_positive()"),
        "{source}"
    );
    assert!(!source.contains(".max(0.0) as usize"), "{source}");
}

#[test]
fn keeps_an_owned_string_when_a_typeof_local_escapes() {
    let source = source_for(
        r"
export function kindOf(value: unknown): string {
  return typeof value;
}
",
    );

    // Returned as the function's `String`, which is not a read this analysis
    // knows how to spell against a `&'static str`, so the local keeps the owned
    // form. The whitelist falls back rather than guessing.
    assert!(
        source.contains("let _smelt_tmp_1: JsString = JsString::from(smelt_typeof(&value));"),
        "{source}"
    );
}

#[test]
fn reads_erased_callback_string_length_property() {
    let source = source_for(
        r#"
function project(values: string[], mapper: (value: unknown) => unknown): unknown[] {
  return values.map(mapper);
}

export function run(): unknown[] {
  return project(["aa", "b"], (value) => value.length);
}
"#,
    );

    assert!(
        source.contains(
            "SmeltUnknown::String(smelt_value) => SmeltUnknown::Number(smelt_value.len() as f64)"
        ),
        "{source}"
    );
    assert!(
        source.contains("SmeltUnknown::Array(smelt_value) => SmeltUnknown::Number(smelt_value.len() as f64)"),
        "{source}"
    );
}

#[test]
fn lowers_string_starts_with_inside_callback_body() {
    // `value.startsWith(prefix)` inside a callback body lowers to the same
    // prefix test the direct `string_affix_call` path emits.
    let source = source_for(
        r#"
const words = ["apple", "banana"];
const flags = words.map(word => word.startsWith("a"));
"#,
    );

    assert!(
        source.contains("starts_with"),
        "string .startsWith inside a callback should lower to a starts_with test\n{source}"
    );
}

#[test]
fn lowers_string_ends_with_on_erased_callback_receiver() {
    // An erased (`unknown`) callback receiver reaching the string-only
    // `endsWith` method is coerced to a string before the suffix test.
    let source = source_for(
        r#"
declare const values: unknown[];
const flags = values.map(value => value.endsWith("x"));
"#,
    );

    assert!(
        source.contains("ends_with"),
        "string .endsWith on an erased callback receiver should still lower to ends_with\n{source}"
    );
}

/// A non-boolean operand asked for at `bool` is a truthiness test, at the
/// OPERAND entry point of the coercion as well as the text one.
///
/// `value_at_type` has its own arms and then falls through to the raw operand,
/// so a coercion it had no arm for handed the value back unchanged. A `&&`
/// chain whose middle operand is a string is the case that found it: hono's
/// trie-router `insert` writes
/// `getPattern(p, nextP) || (nextP === undefined && p && …)`, whose chain value
/// lands in a `bool` local, and the branch emitted `p.clone()` (E0308).
#[test]
fn a_string_operand_at_bool_is_a_truthiness_test() {
    let source = source_for(
        r"
export function pick(parts: string[], index: number): string | null {
  const p: string = parts[index];
  const nextP: string | undefined = parts[index + 1];
  return nextP === undefined && p && p.indexOf('*') === p.length - 1 ? p : null;
}
",
    );

    // The enabling condition: the chain's value really is held in a `bool`
    // local, which is what makes the branch a coercion to `bool` at all.
    assert!(
        source.contains("let _smelt_tmp_6: bool = if"),
        "the `&&` chain must lower its value into a bool local: {source}"
    );
    assert!(
        source.contains("!(p.clone()).is_empty()"),
        "a string in a boolean position must test truthiness: {source}"
    );
    assert!(
        !source.contains("bool = if _smelt_tmp_5 { p.clone() }"),
        "the raw string must not be handed back at `bool`: {source}"
    );
}
