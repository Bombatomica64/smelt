//! Runtime prelude for JavaScript `bigint` values.
//!
//! A TypeScript `bigint` lowers to [`smelt_hir::Type::BigInt`] and is carried in
//! Rust as [`BIGINT_TYPE`] (`num_bigint::BigInt`): unbounded, like JavaScript's
//! own bigint, so `2n ** 100n` and `9_999_999_999_999_999_999_999n` are exact
//! and nothing ever round-trips through `f64`. `num-bigint` is the crate a Rust
//! team would reach for, and its operators already have the semantics the spec
//! asks for in most places:
//!
//! * `/` truncates toward zero and `%` takes the dividend's sign, exactly as
//!   ECMA-262 `BigInt::divide` / `BigInt::remainder` do;
//! * `&`, `|`, `^` and `!` treat a negative value as an infinite
//!   two's-complement bit string (ECMA-262 `BigIntBitwiseOp`);
//! * `>>` rounds toward negative infinity (`-5n >> 1n` is `-3n`).
//!
//! What this module adds is the part the crate does not decide: the JavaScript
//! THROWS (a zero divisor, a negative exponent, `BigInt(1.5)`, a malformed
//! `BigInt("1.5")`, an out-of-range `asIntN` width), the conversions between a
//! bigint and a `number` or a string, mixed `bigint`/`number` comparisons, and a
//! signed shift count (`x << -2n` is `x >> 2n`).
//!
//! The fallible operations answer `Result<_, Box<dyn Error>>` with the same
//! branded error record a source `throw new RangeError(..)` builds, because each
//! is reached through a fallible builtin call (`BuiltinFn::BigInt`) whose
//! unwind edge can land in a source `catch`. The infallible helpers that can
//! still throw on an absurd input (a shift or power whose result would exceed
//! V8's maximum bigint size) throw through the panic route, the same channel
//! every other rvalue-level throw uses.
//!
//! Emitted only when the crate's type table contains a bigint (see
//! `stdlib::needs_bigint_runtime`), and then inside the `SmeltUnknown` block,
//! because every throw payload is an erased error record.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]

use crate::rust::CodeWriter;

/// The Rust type a JavaScript `bigint` is carried as.
pub(crate) const BIGINT_TYPE: &str = "::num_bigint::BigInt";

/// V8's limit on a bigint's magnitude, in bits (`BigInt::kMaxLengthBits`).
///
/// A shift or power whose result would need more bits than this throws
/// `RangeError: Maximum BigInt size exceeded` in Node rather than trying to
/// allocate it; the helpers check the same bound so a hostile count cannot make
/// the generated program allocate gigabytes.
const MAX_BIGINT_BITS: u64 = 1 << 30;

/// Renders the bigint constant whose canonical decimal digits are `digits`.
///
/// A value that fits `i64` is built with `From<i64>`, the spelling a
/// hand-written port uses; anything wider is parsed from its digits, which are
/// already validated by the frontend (only an optional `-` and ASCII digits).
pub(crate) fn literal_text(digits: &str) -> String {
    if let Ok(small) = digits.parse::<i64>() {
        format!("{BIGINT_TYPE}::from({small}_i64)")
    } else {
        format!("{digits:?}.parse::<{BIGINT_TYPE}>().expect(\"bigint literal digits\")")
    }
}

/// Emits the bigint runtime helpers into the generated prelude.
pub(crate) fn emit(writer: &mut CodeWriter) {
    let range_error = crate::thrown::error_payload_record_expr("RangeError", "message");
    let syntax_error = crate::thrown::error_payload_record_expr("SyntaxError", "message");
    let type_error = crate::thrown::error_payload_record_expr("TypeError", "message");
    let throw_range = crate::thrown::throw_expr(&range_error);
    let throw_syntax = crate::thrown::throw_expr(&syntax_error);
    let throw_type = crate::thrown::throw_expr(&type_error);
    writer.blank_line();
    writer.line("/// A JavaScript `RangeError` raised by a bigint operation.");
    writer.line(format!(
        "fn smelt_bigint_range_error(message: String) -> Box<dyn ::std::error::Error> {{ {throw_range} }}"
    ));
    writer.line("/// A JavaScript `TypeError` raised by `JSON.stringify` over a bigint.");
    writer.line(format!(
        "fn smelt_bigint_type_error_value(message: String) -> Box<dyn ::std::error::Error> {{ {throw_type} }}"
    ));
    writer.line("/// A JavaScript `SyntaxError` raised by `BigInt(string)`.");
    writer.line(format!(
        "fn smelt_bigint_syntax_error(message: String) -> Box<dyn ::std::error::Error> {{ {throw_syntax} }}"
    ));
    writer.line("/// Throw a JavaScript `TypeError` raised by a bigint operation through the panic route.");
    writer.line("///");
    writer.line("/// Reached only where the static types cannot rule the operation out:");
    writer.line("/// TypeScript rejects `1n + 1` and `1n >>> 1n` at compile time.");
    writer.line(format!(
        "fn smelt_bigint_type_error(message: &str) -> ! {{ let message = message.to_owned(); smelt_panic_throw({throw_type}) }}"
    ));
    writer.blank_line();
    writer.line("/// `lhs / rhs`: truncating division; a zero divisor is a `RangeError`.");
    writer.line(format!(
        "fn smelt_bigint_div(lhs: &{BIGINT_TYPE}, rhs: &{BIGINT_TYPE}) -> Result<{BIGINT_TYPE}, Box<dyn ::std::error::Error>> {{ if ::num_traits::Zero::is_zero(rhs) {{ return Err(smelt_bigint_range_error(\"Division by zero\".to_owned())); }} Ok(lhs / rhs) }}"
    ));
    writer.line("/// `lhs % rhs`: the remainder takes the dividend's sign; a zero divisor is a `RangeError`.");
    writer.line(format!(
        "fn smelt_bigint_rem(lhs: &{BIGINT_TYPE}, rhs: &{BIGINT_TYPE}) -> Result<{BIGINT_TYPE}, Box<dyn ::std::error::Error>> {{ if ::num_traits::Zero::is_zero(rhs) {{ return Err(smelt_bigint_range_error(\"Division by zero\".to_owned())); }} Ok(lhs % rhs) }}"
    ));
    // `0n ** huge`, `1n ** huge` and `(-1n) ** huge` are exact and tiny, so they
    // are answered before the size bound; every other base grows by at least
    // one bit per unit of exponent.
    writer.line("/// `base ** exponent`: a negative exponent is a `RangeError`.");
    writer.line(format!(
        "fn smelt_bigint_pow(base: &{BIGINT_TYPE}, exponent: &{BIGINT_TYPE}) -> Result<{BIGINT_TYPE}, Box<dyn ::std::error::Error>> {{ if exponent.sign() == ::num_bigint::Sign::Minus {{ return Err(smelt_bigint_range_error(\"Exponent must be non-negative\".to_owned())); }} let one = {BIGINT_TYPE}::from(1_i64); if ::num_traits::Zero::is_zero(exponent) {{ return Ok(one); }} if ::num_traits::Zero::is_zero(base) || *base == one {{ return Ok(base.clone()); }} if *base == -&one {{ return Ok(if ::num_integer::Integer::is_even(exponent) {{ one }} else {{ -one }}); }} match u32::try_from(exponent) {{ Ok(power) if base.bits().saturating_sub(1).saturating_mul(u64::from(power)) <= {MAX_BIGINT_BITS} => Ok(base.pow(power)), _ => Err(smelt_bigint_range_error(\"Maximum BigInt size exceeded\".to_owned())) }} }}"
    ));
    // A shift count is itself a bigint and may be negative, which reverses the
    // direction. A right shift past every bit leaves `0n` or `-1n`; a left
    // shift past V8's size cap (2^30 bits) is a catchable `RangeError`, so both
    // return a `Result` and reach a `try` through the fallible call route.
    writer.line("/// `value << count`, where a negative count shifts right.");
    writer.line(format!(
        "fn smelt_bigint_shl(value: &{BIGINT_TYPE}, count: &{BIGINT_TYPE}) -> Result<{BIGINT_TYPE}, Box<dyn ::std::error::Error>> {{ if count.sign() == ::num_bigint::Sign::Minus {{ return smelt_bigint_shr(value, &-count); }} if ::num_traits::Zero::is_zero(value) {{ return Ok(value.clone()); }} match u64::try_from(count) {{ Ok(bits) if value.bits().saturating_add(bits) <= {MAX_BIGINT_BITS} => Ok(value << bits), _ => Err(smelt_bigint_range_error(\"Maximum BigInt size exceeded\".to_owned())) }} }}"
    ));
    writer.line("/// `value >> count`: an arithmetic shift rounding toward negative infinity.");
    writer.line(format!(
        "fn smelt_bigint_shr(value: &{BIGINT_TYPE}, count: &{BIGINT_TYPE}) -> Result<{BIGINT_TYPE}, Box<dyn ::std::error::Error>> {{ if count.sign() == ::num_bigint::Sign::Minus {{ return smelt_bigint_shl(value, &-count); }} Ok(match u64::try_from(count) {{ Ok(bits) if bits < value.bits() => value >> bits, _ => if value.sign() == ::num_bigint::Sign::Minus {{ {BIGINT_TYPE}::from(-1_i64) }} else {{ {BIGINT_TYPE}::from(0_i64) }} }}) }}"
    ));
    writer.blank_line();
    writer.line("/// `BigInt(number)`: only an integral, finite number converts; anything else is a `RangeError`.");
    writer.line(format!(
        "fn smelt_bigint_from_number(value: f64) -> Result<{BIGINT_TYPE}, Box<dyn ::std::error::Error>> {{ if value.is_finite() && value.fract() == 0.0 {{ if let Some(converted) = <{BIGINT_TYPE} as ::num_traits::FromPrimitive>::from_f64(value) {{ return Ok(converted); }} }} Err(smelt_bigint_range_error(format!(\"The number {{}} cannot be converted to a BigInt because it is not an integer\", smelt_number_to_string(value)))) }}"
    ));
    // ECMA-262 `StringToBigInt`: surrounding whitespace is ignored, the empty
    // string is `0n`, a `0x`/`0o`/`0b` prefix selects the radix (and forbids a
    // sign), and anything else must be an optionally signed run of decimal
    // digits -- no fraction, no exponent, no separators.
    writer.line("/// `BigInt(string)`: ECMA-262 `StringToBigInt`; an unparsable string is a `SyntaxError`.");
    writer.line(format!(
        "fn smelt_bigint_from_string(text: &str) -> Result<{BIGINT_TYPE}, Box<dyn ::std::error::Error>> {{ let trimmed = text.trim(); if trimmed.is_empty() {{ return Ok({BIGINT_TYPE}::from(0_i64)); }} let lowered = trimmed.get(..2).map(str::to_ascii_lowercase); let (radix, digits, negative) = match lowered.as_deref() {{ Some(\"0x\") => (16, &trimmed[2..], false), Some(\"0o\") => (8, &trimmed[2..], false), Some(\"0b\") => (2, &trimmed[2..], false), _ => match trimmed.as_bytes()[0] {{ b'-' => (10, &trimmed[1..], true), b'+' => (10, &trimmed[1..], false), _ => (10, trimmed, false) }} }}; if !digits.is_empty() && digits.chars().all(|digit| digit.is_digit(radix)) {{ if let Some(parsed) = {BIGINT_TYPE}::parse_bytes(digits.as_bytes(), radix) {{ return Ok(if negative {{ -parsed }} else {{ parsed }}); }} }} Err(smelt_bigint_syntax_error(format!(\"Cannot convert {{text}} to a BigInt\"))) }}"
    ));
    writer.line("/// `BigInt(value)` over an erased value: ECMA-262 `ToBigInt` after `ToPrimitive`.");
    writer.line(format!(
        "fn smelt_bigint_from_unknown(value: &SmeltUnknown) -> Result<{BIGINT_TYPE}, Box<dyn ::std::error::Error>> {{ match value {{ SmeltUnknown::BigInt(value) => Ok(value.clone()), SmeltUnknown::Number(value) => smelt_bigint_from_number(*value), SmeltUnknown::Bool(value) => Ok({BIGINT_TYPE}::from(i64::from(*value))), SmeltUnknown::String(value) => smelt_bigint_from_string(value), other => {{ let message = format!(\"Cannot convert {{other}} to a BigInt\"); Err({throw_type}) }} }} }}"
    ));
    writer.line("/// `Number(bigint)`: the nearest `f64` (ties to even), `Infinity` past `f64::MAX`.");
    writer.line(format!(
        "fn smelt_bigint_to_number(value: &{BIGINT_TYPE}) -> f64 {{ ::num_traits::ToPrimitive::to_f64(value).unwrap_or(f64::NAN) }}"
    ));
    writer.line("/// ECMA-262 `ToIndex` on an `asIntN`/`asUintN` width: a negative or unsafe width is a `RangeError`.");
    writer.line(
        "fn smelt_bigint_width(bits: f64) -> Result<u64, Box<dyn ::std::error::Error>> { let integer = if bits.is_nan() { 0.0 } else { bits.trunc() }; if !(0.0..=9007199254740991.0).contains(&integer) { return Err(smelt_bigint_range_error(\"Invalid value: not (convertible to) a safe integer\".to_owned())); } Ok(integer as u64) }",
    );
    // A width wider than the value's own magnitude leaves it unchanged, which
    // is what keeps `BigInt.asIntN(2 ** 40, x)` from building a 2^40-bit
    // modulus; only a narrower width does the modular arithmetic.
    writer.line("/// `BigInt.asIntN(bits, value)`: `value` wrapped into the signed `bits`-wide range.");
    writer.line(format!(
        "fn smelt_bigint_as_int_n(bits: f64, value: &{BIGINT_TYPE}) -> Result<{BIGINT_TYPE}, Box<dyn ::std::error::Error>> {{ let bits = smelt_bigint_width(bits)?; if bits == 0 {{ return Ok({BIGINT_TYPE}::from(0_i64)); }} if bits > value.bits().saturating_add(1) {{ return Ok(value.clone()); }} let modulus = {BIGINT_TYPE}::from(1_i64) << bits; let wrapped = ::num_integer::Integer::mod_floor(value, &modulus); Ok(if wrapped >= (&modulus >> 1_u32) {{ wrapped - modulus }} else {{ wrapped }}) }}"
    ));
    writer.line("/// `BigInt.asUintN(bits, value)`: `value` modulo `2^bits`.");
    writer.line(format!(
        "fn smelt_bigint_as_uint_n(bits: f64, value: &{BIGINT_TYPE}) -> Result<{BIGINT_TYPE}, Box<dyn ::std::error::Error>> {{ let bits = smelt_bigint_width(bits)?; if value.sign() != ::num_bigint::Sign::Minus && bits >= value.bits() {{ return Ok(value.clone()); }} if bits > {MAX_BIGINT_BITS} {{ return Err(smelt_bigint_range_error(\"Maximum BigInt size exceeded\".to_owned())); }} let modulus = {BIGINT_TYPE}::from(1_i64) << bits; Ok(::num_integer::Integer::mod_floor(value, &modulus)) }}"
    ));
    writer.line("/// `bigint.toString(radix)`: a radix outside `2..=36` is a catchable `RangeError`.");
    writer.line(format!(
        "fn smelt_bigint_to_string_radix(value: &{BIGINT_TYPE}, radix: f64) -> Result<String, Box<dyn ::std::error::Error>> {{ let radix = if radix.is_nan() {{ 0.0 }} else {{ radix.trunc() }}; if !(2.0..=36.0).contains(&radix) {{ return Err(smelt_bigint_range_error(\"toString() radix argument must be between 2 and 36\".to_owned())); }} Ok(value.to_str_radix(radix as u32)) }}"
    ));
    // The erasure adapters. A bigint keeps its own tag in the erased carrier;
    // recovering one at bigint type accepts the tag and, like every other
    // `SmeltFromUnknown` impl, coerces the primitives a dynamic value can hold
    // rather than panicking (an integral number, a boolean, a numeric string),
    // with `0n` for anything else.
    writer.blank_line();
    writer.line(format!(
        "impl IntoSmeltUnknown for {BIGINT_TYPE} {{ fn into_smelt_unknown(self) -> SmeltUnknown {{ SmeltUnknown::BigInt(self) }} }}"
    ));
    writer.line(format!(
        "impl SmeltFromUnknown for {BIGINT_TYPE} {{ fn smelt_from_unknown(value: SmeltUnknown) -> Self {{ match value {{ SmeltUnknown::BigInt(value) => value, SmeltUnknown::Number(value) => smelt_bigint_from_number(value).unwrap_or_default(), SmeltUnknown::Bool(value) => Self::from(i64::from(value)), SmeltUnknown::String(value) => smelt_bigint_from_string(&value).unwrap_or_default(), _ => Self::default() }} }} }}"
    ));
    // A bigint is a primitive: `Map`/`Set` membership (SameValueZero) and
    // `===` both compare it by value.
    writer.line(format!(
        "impl SmeltJsKeyEq for {BIGINT_TYPE} {{ fn same_js_key(&self, other: &Self) -> bool {{ self == other }} fn js_key_hash(&self) -> Option<u64> {{ Some(smelt_js_hash_one(self)) }} }}"
    ));
    writer.line(format!(
        "impl SmeltJsStrictEq for {BIGINT_TYPE} {{ fn js_strict_eq(&self, other: &Self) -> bool {{ self == other }} }}"
    ));
    writer.blank_line();
    // ECMA-262 `ApplyStringOrNumericBinaryOperator` over two erased values:
    // a string operand of `+` concatenates, two bigints take the bigint
    // operator, two numbers the number one, and a bigint mixed with anything
    // else is a `TypeError`. The number arm keeps the int32 semantics of the
    // bitwise and shift operators.
    writer.line("/// `left <op> right` over two erased values that may be bigints or numbers.");
    writer.line(
        "fn smelt_unknown_arithmetic(op: &str, left: SmeltUnknown, right: SmeltUnknown) -> SmeltUnknown { if op == \"+\" && (matches!(left, SmeltUnknown::String(_)) || matches!(right, SmeltUnknown::String(_))) { return SmeltUnknown::String(format!(\"{left}{right}\").into()); } match (&left, &right) { (SmeltUnknown::BigInt(left), SmeltUnknown::BigInt(right)) => SmeltUnknown::BigInt(match op { \"+\" => left + right, \"-\" => left - right, \"*\" => left * right, \"/\" => smelt_bigint_div(left, right).unwrap_or_else(|error| smelt_panic_throw(error)), \"%\" => smelt_bigint_rem(left, right).unwrap_or_else(|error| smelt_panic_throw(error)), \"<<\" => smelt_bigint_shl(left, right).unwrap_or_else(|error| smelt_panic_throw(error)), \">>\" => smelt_bigint_shr(left, right).unwrap_or_else(|error| smelt_panic_throw(error)), \"&\" => left & right, \"|\" => left | right, \"^\" => left ^ right, _ => smelt_bigint_type_error(\"BigInts have no unsigned right shift, use >> instead\") }), (SmeltUnknown::BigInt(_), _) | (_, SmeltUnknown::BigInt(_)) => smelt_bigint_type_error(\"Cannot mix BigInt and other types, use explicit conversions\"), _ => { let left = smelt_unknown_to_number(&left); let right = smelt_unknown_to_number(&right); let int32 = |value: f64| if value.is_finite() { value.trunc().rem_euclid(4294967296.0) as u32 as i32 } else { 0 }; SmeltUnknown::Number(match op { \"+\" => left + right, \"-\" => left - right, \"*\" => left * right, \"/\" => left / right, \"%\" => left % right, \"<<\" => f64::from(int32(left).wrapping_shl(int32(right) as u32 & 31)), \">>\" => f64::from(int32(left).wrapping_shr(int32(right) as u32 & 31)), \">>>\" => f64::from((int32(left) as u32).wrapping_shr(int32(right) as u32 & 31)), \"&\" => f64::from(int32(left) & int32(right)), \"|\" => f64::from(int32(left) | int32(right)), _ => f64::from(int32(left) ^ int32(right)) }) } } }",
    );
    // ECMA-262 `BigInt::lessThan` against a Number compares MATHEMATICAL
    // values: `1n < 1.5`, `2n == 2.0`, and nothing is ordered against NaN.
    writer.line("/// Compare a bigint with a number by mathematical value; `None` against `NaN`.");
    writer.line(format!(
        "fn smelt_bigint_compare_number(value: &{BIGINT_TYPE}, number: f64) -> Option<::std::cmp::Ordering> {{ if number.is_nan() {{ return None; }} if number.is_infinite() {{ return Some(if number > 0.0 {{ ::std::cmp::Ordering::Less }} else {{ ::std::cmp::Ordering::Greater }}); }} let floor = number.floor(); let floor_value = <{BIGINT_TYPE} as ::num_traits::FromPrimitive>::from_f64(floor)?; match value.cmp(&floor_value) {{ ::std::cmp::Ordering::Equal if number > floor => Some(::std::cmp::Ordering::Less), ordering => Some(ordering) }} }}"
    ));
}
