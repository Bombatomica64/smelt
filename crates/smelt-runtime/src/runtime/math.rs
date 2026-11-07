// JavaScript `Math` operations whose Rust spelling disagrees with the specification.

// JavaScript rounds a tie toward +∞; Rust's `f64::round` rounds a tie away
// from zero. They disagree for every negative value whose fraction is
// exactly 0.5 — `Math.round(-1.5)` is `-1` in JavaScript and `-2.0` in
// Rust — which is what made es-toolkit's `round` specs disagree.
//
// Not `(x + 0.5).floor()`: the ECMA-262 note on `Math.round` calls out that
// the naive form is wrong for very large `x`, where adding `0.5` is not
// representable. `floor` is exact at those magnitudes and the fraction is
// then `0`, so the value passes through unchanged.
//
// `-0` is preserved: JavaScript's `Math.round(-0.5)` is `-0`, and
// `Object.is(-0, 0)` is `false`, so a caller that inspects the sign of a
// zero must see the JavaScript answer. NaN and the infinities pass through
// because `floor` already leaves them alone.
// @smelt:item smelt_math_round when math_round
/// JavaScript `Math.round`: a tie rounds toward +∞, not away from zero.
fn smelt_math_round(value: f64) -> f64 { let floor = value.floor(); let rounded = if value - floor >= 0.5 { floor + 1.0 } else { floor }; if rounded == 0.0 && value.is_sign_negative() { -0.0 } else { rounded } }

// @smelt:item-end

// @smelt:item smelt_py_floor_div_int when python_division
/// Python `int // int`: the quotient rounded toward negative infinity.
fn smelt_py_floor_div_int(lhs: i64, rhs: i64) -> i64 { let quotient = lhs.wrapping_div(rhs); if lhs.wrapping_rem(rhs) != 0 && (lhs < 0) != (rhs < 0) { quotient - 1 } else { quotient } }
/// Python `int % int`: the remainder takes the sign of the divisor.
fn smelt_py_mod_int(lhs: i64, rhs: i64) -> i64 { let remainder = lhs.wrapping_rem(rhs); if remainder != 0 && (remainder < 0) != (rhs < 0) { remainder + rhs } else { remainder } }
/// Python `float % float`: CPython's `float_rem`, sign of the divisor, and a zero
/// result keeps it too.
fn smelt_py_mod_float(lhs: f64, rhs: f64) -> f64 { let remainder = lhs % rhs; if remainder == 0.0 { 0.0_f64.copysign(rhs) } else if (remainder < 0.0) != (rhs < 0.0) { remainder + rhs } else { remainder } }
/// Python `float // float`: CPython's `float_floor_div`, exact where `(a / b).floor()`
/// is not (`1.0 // 0.1` is `9.0`).
fn smelt_py_floor_div_float(lhs: f64, rhs: f64) -> f64 { let mut remainder = lhs % rhs; let mut quotient = (lhs - remainder) / rhs; if remainder != 0.0 && (remainder < 0.0) != (rhs < 0.0) { remainder += rhs; quotient -= 1.0; } let _ = remainder; if quotient == 0.0 { return 0.0_f64.copysign(lhs / rhs); } let floored = quotient.floor(); if quotient - floored > 0.5 { floored + 1.0 } else { floored } }
/// Python `float / float`, the unchecked half of a checked true division.
fn smelt_py_true_div(lhs: f64, rhs: f64) -> f64 { lhs / rhs }
/// Run a Python division, raising `ZeroDivisionError` on a zero divisor.
fn smelt_py_checked_division<T: PartialEq + Default>(lhs: T, rhs: T, divide: fn(T, T) -> T) -> Result<T, Box<dyn ::std::error::Error>> { if rhs == T::default() { return Err(Box::new(::std::io::Error::other("division by zero"))); } Ok(divide(lhs, rhs)) }

// @smelt:item-end

// @smelt:item smelt_py_list_read when python_list_read
/// Python's `xs[i]`: a negative `i` counts from the end, and an `i` out of range
/// raises `IndexError`.
fn smelt_py_list_read<T: Clone>(list: &SmeltList<T>, index: i64) -> Result<T, Box<dyn ::std::error::Error>> { let items = list.borrow(); let at = if index < 0 { index.saturating_add(i64::try_from(items.len()).unwrap_or(i64::MAX)) } else { index }; usize::try_from(at).ok().and_then(|at| items.get(at).cloned()).ok_or_else(|| Box::new(::std::io::Error::other("list index out of range")) as Box<dyn ::std::error::Error>) }

// @smelt:item-end

// @smelt:item smelt_py_key_error when python_dict_read
/// The `KeyError` of a missing `d[key]`, worded as `str(error)` words it: the
/// key's `repr`.
fn smelt_py_key_error<K: ::std::fmt::Debug>(key: &K) -> Box<dyn ::std::error::Error> { let text = format!("{key:?}"); let repr = match text.strip_prefix('"').and_then(|inner| inner.strip_suffix('"')) { Some(inner) => format!("'{inner}'"), None => text }; Box::new(::std::io::Error::other(repr)) }

// @smelt:item-end
