//! Rust emission of JavaScript `bigint` values (`num_bigint::BigInt`).
//!
//! Type-preservation requirement: a bigint parameter, local, return and
//! operator result is spelled `::num_bigint::BigInt`, never `i64`, `i128`,
//! `f64` or `SmeltUnknown`, and no bigint operator round-trips through a
//! float. Each test compares whole emitted items for that reason. Behaviour
//! (truncating `/`, flooring `>>`, mixed comparisons, the throws) is checked
//! against Node by the `bigint_boundaries` Node-parity fixture and the example
//! `170_bigint_arbitrary_precision`.

use super::*;

/// Operators borrow their operands, `>>` and `/` are fallible calls, and a
/// mixed comparison goes through the mathematical-value helper.
#[test]
fn a_bigint_is_an_unbounded_rust_integer_end_to_end() {
    let source = source_for(
        r"
export function mix(a: bigint, b: bigint): bigint {
  return ((a * b) >> 3n) ^ -a;
}
export function half(a: bigint): bigint {
  return a / 2n;
}
export function below(a: bigint, b: number): boolean {
  return a < b;
}
",
    );
    assert_eq!(
        [
            rust_fn(&source, "mix"),
            rust_fn(&source, "half"),
            rust_fn(&source, "below")
        ]
        .join("\n\n"),
        r"fn mix(a: ::num_bigint::BigInt, b: ::num_bigint::BigInt) -> Result<::num_bigint::BigInt, Box<dyn std::error::Error>> {
let _smelt_tmp_4: ::num_bigint::BigInt;
let _smelt_tmp_5: ::num_bigint::BigInt;
let _smelt_tmp_2: ::num_bigint::BigInt = &a * &b;
let _smelt_tmp_3: ::num_bigint::BigInt = smelt_bigint_shr(&_smelt_tmp_2, &::num_bigint::BigInt::from(3_i64))?;
_smelt_tmp_4 = -(&a);
_smelt_tmp_5 = &_smelt_tmp_3 ^ &_smelt_tmp_4;
return Ok(_smelt_tmp_5);
}

fn half(a: ::num_bigint::BigInt) -> Result<::num_bigint::BigInt, Box<dyn std::error::Error>> {
let _smelt_tmp_1: ::num_bigint::BigInt = smelt_bigint_div(&a, &::num_bigint::BigInt::from(2_i64))?;
return Ok(_smelt_tmp_1);
}

fn below(a: ::num_bigint::BigInt, b: f64) -> bool {
let _smelt_tmp_2: bool = matches!(smelt_bigint_compare_number(&a, b), Some(::std::cmp::Ordering::Less));
return _smelt_tmp_2;
}"
    );
}

/// The genuine dynamic boundary behind `SmeltUnknown::BigInt`.
///
/// `describe` takes a source `unknown` and inspects it with `typeof`, so the
/// value it receives must carry its runtime kind: no concrete type, union or
/// scoped generic of `describe`'s signature can say "this one is a bigint"
/// (the caller could pass anything). A `Number` tag would round the value
/// through `f64` and make `typeof` answer `"number"`.
#[test]
fn a_bigint_crossing_an_unknown_boundary_keeps_its_own_tag() {
    let source = source_for(
        r#"
export function describe(value: unknown): string {
  return typeof value === "bigint" ? "bigint" : "other";
}
export function run(): string {
  return describe(9_999_999_999_999_999_999_999n);
}
"#,
    );
    assert_eq!(
        [rust_fn(&source, "run"), rust_fn(&source, "describe")].join("\n\n"),
        r#"fn run() -> JsString {
let _smelt_tmp_0: JsString = describe(SmeltUnknown::BigInt(("9999999999999999999999".parse::<::num_bigint::BigInt>().expect("bigint literal digits")).clone()));
return _smelt_tmp_0;
}

fn describe(value: SmeltUnknown) -> JsString {
let mut _smelt_tmp_2: JsString;
let _smelt_tmp_1: bool = matches!(value.clone(), SmeltUnknown::BigInt(_));
let _smelt_tmp_2: JsString = if _smelt_tmp_1 { JsString::from("bigint") } else { JsString::from("other") };
return _smelt_tmp_2;
}"#
    );
}

/// The bigint runtime is pay-for-use: a program with no bigint carries neither
/// the `num-bigint` types nor the erased carrier's `BigInt` tag.
#[test]
fn a_program_without_bigints_carries_no_bigint_runtime() {
    let source = source_for(
        r"
export function kind(value: unknown): string {
  return typeof value;
}
",
    );
    assert!(source.contains("pub enum SmeltUnknown"), "{source}");
    assert!(!source.contains("num_bigint"), "{source}");
    assert!(!source.contains("SmeltUnknown::BigInt"), "{source}");
}

/// A `BigInt64Array` element is a bigint read from its eight bytes, not the
/// `f64` every other view's element is: a 64-bit element above 2^53 has no
/// exact `f64`.
#[test]
fn a_bigint_view_element_is_read_and_written_as_a_bigint() {
    let source = source_for(
        r"
export function roundTrip(): bigint {
  const view = new BigInt64Array([1n, 2n]);
  view[1] = 9_223_372_036_854_775_807n;
  return view[0] + view[1];
}
",
    );
    assert_eq!(
        rust_fn(&source, "round_trip"),
        r"fn round_trip() -> ::num_bigint::BigInt {
let _smelt_tmp_1: SmeltList<::num_bigint::BigInt> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<::num_bigint::BigInt> = vec![::num_bigint::BigInt::from(1_i64), ::num_bigint::BigInt::from(2_i64)]; smelt_list_items }));
let mut view: SmeltTypedArray = SmeltTypedArray::with_bigint_elements(SmeltTypedArrayKind::BigInt64, &_smelt_tmp_1.to_vec());
view.set_bigint(1.0, &::num_bigint::BigInt::from(9223372036854775807_i64));
let _smelt_tmp_3: ::num_bigint::BigInt = &view.get_bigint(0.0).unwrap_or_default() + &view.get_bigint(1.0).unwrap_or_default();
return _smelt_tmp_3;
}"
    );
}

/// `JSON.stringify` over a bigint is a catchable `TypeError`. Only a value
/// whose type can hold a bigint takes the fallible call, so every other
/// `JSON.stringify` keeps its infallible rvalue and its signature.
#[test]
fn json_stringify_is_fallible_only_where_a_bigint_can_appear() {
    let source = source_for(
        r"
interface Ledger {
  owner: string;
  balance: bigint;
}
export function ledgerJson(ledger: Ledger): string {
  return JSON.stringify(ledger);
}
export function plainJson(owner: string): string {
  return JSON.stringify({ owner });
}
",
    );
    assert_eq!(
        [
            rust_fn(&source, "ledger_json"),
            rust_fn(&source, "plain_json")
        ]
        .join("\n\n"),
        r#"fn ledger_json(ledger: Ledger) -> Result<JsString, Box<dyn std::error::Error>> {
let _smelt_tmp_1: JsString = smelt_json_stringify(&{ let smelt_object_value = ledger.clone(); let smelt_struct_value = smelt_object_value.clone(); let mut smelt_object_entries = Vec::<(String, _)>::new(); smelt_object_entries.push(("owner".to_owned(), SmeltUnknown::String(smelt_object_value.owner.into()))); smelt_object_entries.push(("balance".to_owned(), SmeltUnknown::BigInt(smelt_object_value.balance.clone()))); SmeltUnknown::Object(SmeltObject::new(smelt_object_entries)) }).map(JsString::from).map_err(|_| smelt_bigint_type_error_value("Do not know how to serialize a BigInt".to_owned())).map(JsString::from)?;
return Ok(_smelt_tmp_1);
}

fn plain_json(owner: JsString) -> JsString {
let _smelt_tmp_1: SmeltRecord<JsString, JsString> = SmeltRecord::from([(JsString::from("owner"), owner.clone())]);
let _smelt_tmp_2: JsString = JsString::from(smelt_json_stringify(&{ let smelt_record = _smelt_tmp_1.clone(); SmeltUnknown::Object(SmeltObject::with_id(smelt_record.id, smelt_record.iter().map(|(smelt_key, smelt_value)| (smelt_key, SmeltUnknown::String(smelt_value.into()))).collect())) }).expect("JSON serialization failed"));
return _smelt_tmp_2;
}"#
    );
}
