//! Rust emission of JavaScript `bigint` values (`num_bigint::BigInt`).
//!
//! Type-preservation requirement: a bigint parameter, local, return and
//! operator result is spelled `::num_bigint::BigInt`, never `i64`, `i128`,
//! `f64` or `SmeltUnknown`, and no bigint operator round-trips through a
//! float. The runtime behavior (truncating `/`, flooring `>>`, mixed
//! comparisons, the throws) is checked against Node by the end-to-end example
//! `170_bigint_arbitrary_precision`.

use super::*;

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
    let mix = emitted_function_body(&source, "fn mix(");
    // `>>` is fallible (a negative count shifts left and can pass the size
    // cap), so `mix` can throw and returns its bigint through the error
    // channel. This used to assert a bare `-> ::num_bigint::BigInt`, which
    // held only while a shift's `RangeError` escaped `try` as a panic.
    assert!(
        mix.contains(
            "fn mix(a: ::num_bigint::BigInt, b: ::num_bigint::BigInt) -> Result<::num_bigint::BigInt, Box<dyn std::error::Error>>"
        ),
        "{mix}"
    );
    // Operators borrow their operands rather than cloning them.
    assert!(mix.contains("&a * &b"), "{mix}");
    assert!(mix.contains("smelt_bigint_shr(&_smelt_tmp_"), "{mix}");
    assert!(mix.contains("&::num_bigint::BigInt::from(3_i64))?"), "{mix}");
    for narrowed in ["f64", "i64", "i128"] {
        assert!(
            !mix.contains(&format!("as {narrowed}")),
            "a bigint operation narrowed to {narrowed}: {mix}"
        );
    }
    // `/` throws a catchable `RangeError` on zero, so it is a fallible call.
    let half = emitted_function_body(&source, "fn half(");
    assert!(
        half.contains("smelt_bigint_div(&a, &::num_bigint::BigInt::from(2_i64))?"),
        "{half}"
    );
    // A bigint compares with a number by mathematical value.
    let below = emitted_function_body(&source, "fn below(");
    assert!(below.contains("smelt_bigint_compare_number(&a, b)"), "{below}");
}

/// The genuine dynamic boundary behind `SmeltUnknown::BigInt`.
///
/// `describe` takes a source `unknown` and inspects it with `typeof`, so the
/// value it receives must carry its runtime kind: no concrete type, union or
/// scoped generic of `describe`'s signature can say "this one is a bigint"
/// (the caller could pass anything). Erasing the bigint to a `Number` tag
/// instead -- what an `i64` carrier did -- rounds it through `f64` and makes
/// `typeof` answer `"number"`.
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
    assert!(source.contains("BigInt(::num_bigint::BigInt),"), "{source}");
    let run = emitted_function_body(&source, "fn run(");
    assert!(run.contains("SmeltUnknown::BigInt("), "{run}");
    assert!(!run.contains("SmeltUnknown::Number("), "{run}");
    let describe = emitted_function_body(&source, "fn describe(");
    assert!(describe.contains("SmeltUnknown::BigInt(_)"), "{describe}");
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
    let body = emitted_function_body(&source, "fn round_trip(");
    assert!(body.contains("SmeltTypedArray::with_bigint_elements("), "{body}");
    assert!(body.contains(".set_bigint(1.0, &"), "{body}");
    assert!(body.contains(".get_bigint(0.0)"), "{body}");
    assert!(!body.contains(".get(0.0)"), "{body}");
}

#[test]
fn json_stringify_is_fallible_only_where_a_bigint_can_appear() {
    // `JSON.stringify` over a bigint is a catchable `TypeError` ("Do not know
    // how to serialize a BigInt"); it used to be a compile-time blocker. Only
    // a value whose type can hold a bigint takes the fallible call, so every
    // other `JSON.stringify` keeps its infallible rvalue and its signature.
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
    let ledger = emitted_function_body(&source, "fn ledger_json(");
    assert!(
        ledger.contains("-> Result<String, Box<dyn std::error::Error>>"),
        "{ledger}"
    );
    assert!(
        ledger.contains("Do not know how to serialize a BigInt\".to_owned()))?"),
        "{ledger}"
    );
    let plain = emitted_function_body(&source, "fn plain_json(");
    assert!(plain.contains("fn plain_json(owner: String) -> String"), "{plain}");
    assert!(plain.contains(".expect(\"JSON serialization failed\")"), "{plain}");
}
