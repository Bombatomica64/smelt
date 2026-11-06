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
