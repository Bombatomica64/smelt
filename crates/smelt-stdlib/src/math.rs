//! The `Math` and `Number` namespace members a frontend recognizes.
//!
//! One table per kind of member, so a name is spelled once whichever rule reads
//! it: a direct call, a member passed as a value, or a constant-folded initializer.

/// A function member of the ECMAScript `Math` namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MathFunction {
    /// `Math.abs(value)`.
    Abs,
    /// `Math.floor(value)`.
    Floor,
    /// `Math.ceil(value)`.
    Ceil,
    /// `Math.round(value)`.
    Round,
    /// `Math.trunc(value)`.
    Trunc,
    /// `Math.max(...values)`.
    Max,
    /// `Math.min(...values)`.
    Min,
    /// `Math.hypot(...values)`.
    Hypot,
    /// `Math.sqrt(value)`.
    Sqrt,
    /// `Math.cbrt(value)`.
    Cbrt,
    /// `Math.sign(value)`.
    Sign,
    /// `Math.sin(value)`.
    Sin,
    /// `Math.cos(value)`.
    Cos,
    /// `Math.tan(value)`.
    Tan,
    /// `Math.asin(value)`.
    Asin,
    /// `Math.acos(value)`.
    Acos,
    /// `Math.atan(value)`.
    Atan,
    /// `Math.log(value)`.
    Log,
    /// `Math.log10(value)`.
    Log10,
    /// `Math.log2(value)`.
    Log2,
    /// `Math.exp(value)`.
    Exp,
    /// `Math.pow(base, exponent)`.
    Pow,
    /// `Math.atan2(y, x)`.
    Atan2,
    /// `Math.imul(left, right)`.
    Imul,
    /// `Math.clz32(value)`.
    Clz32,
    /// `Math.random()`.
    Random,
}

/// Every modeled `Math` function with its source spelling.
pub const MATH_FUNCTIONS: &[(&str, MathFunction)] = &[
    ("abs", MathFunction::Abs),
    ("floor", MathFunction::Floor),
    ("ceil", MathFunction::Ceil),
    ("round", MathFunction::Round),
    ("trunc", MathFunction::Trunc),
    ("max", MathFunction::Max),
    ("min", MathFunction::Min),
    ("hypot", MathFunction::Hypot),
    ("sqrt", MathFunction::Sqrt),
    ("cbrt", MathFunction::Cbrt),
    ("sign", MathFunction::Sign),
    ("sin", MathFunction::Sin),
    ("cos", MathFunction::Cos),
    ("tan", MathFunction::Tan),
    ("asin", MathFunction::Asin),
    ("acos", MathFunction::Acos),
    ("atan", MathFunction::Atan),
    ("log", MathFunction::Log),
    ("log10", MathFunction::Log10),
    ("log2", MathFunction::Log2),
    ("exp", MathFunction::Exp),
    ("pow", MathFunction::Pow),
    ("atan2", MathFunction::Atan2),
    ("imul", MathFunction::Imul),
    ("clz32", MathFunction::Clz32),
    ("random", MathFunction::Random),
];

/// Return the `Math` function spelled `name`.
#[must_use]
pub fn math_function(name: &str) -> Option<MathFunction> {
    MATH_FUNCTIONS
        .iter()
        .find(|(spelling, _)| *spelling == name)
        .map(|(_, function)| *function)
}

/// The numeric constants of the `Number` namespace, with their ECMAScript values.
const NUMBER_CONSTANTS: &[(&str, f64)] = &[
    ("NaN", f64::NAN),
    ("POSITIVE_INFINITY", f64::INFINITY),
    ("NEGATIVE_INFINITY", f64::NEG_INFINITY),
    ("MAX_VALUE", f64::MAX),
    // The smallest positive DENORMAL double. `f64::MIN_POSITIVE` is the smallest
    // normal one, which is a different number (2.2e-308).
    ("MIN_VALUE", 5e-324),
    ("MAX_SAFE_INTEGER", 9_007_199_254_740_991.0),
    ("MIN_SAFE_INTEGER", -9_007_199_254_740_991.0),
    ("EPSILON", f64::EPSILON),
];

/// The numeric constants of the `Math` namespace, with their ECMAScript values.
const MATH_CONSTANTS: &[(&str, f64)] = &[
    ("PI", std::f64::consts::PI),
    ("E", std::f64::consts::E),
    ("LN2", std::f64::consts::LN_2),
    ("LN10", std::f64::consts::LN_10),
    ("LOG2E", std::f64::consts::LOG2_E),
    ("LOG10E", std::f64::consts::LOG10_E),
    ("SQRT2", std::f64::consts::SQRT_2),
    ("SQRT1_2", std::f64::consts::FRAC_1_SQRT_2),
];

/// Find `name` in a constant table.
fn constant(table: &[(&str, f64)], name: &str) -> Option<f64> {
    table
        .iter()
        .find(|(spelling, _)| *spelling == name)
        .map(|(_, value)| *value)
}

/// Return the value of `Number.<name>`.
#[must_use]
pub fn number_constant(name: &str) -> Option<f64> {
    constant(NUMBER_CONSTANTS, name)
}

/// Return the value of `Math.<name>`.
#[must_use]
pub fn math_constant(name: &str) -> Option<f64> {
    constant(MATH_CONSTANTS, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RuleId, typescript_call_rule};

    /// Every `Math` function reaches a rule, so the table and recognition agree.
    #[test]
    fn every_math_function_is_recognized() {
        for (name, function) in MATH_FUNCTIONS {
            let expected = if *function == MathFunction::Random {
                RuleId::TsMathRandom
            } else {
                RuleId::TsMathNumeric
            };
            assert_eq!(typescript_call_rule(Some("Math"), name), Some(expected), "Math.{name}");
        }
    }

    /// Each constant has the IEEE-754 bit pattern Node 24 reports for it, read
    /// with `new Float64Array([Number.MIN_VALUE])`. Bits rather than values, so
    /// `NaN` compares and a denormal cannot round to a neighbour.
    #[test]
    fn constants_match_the_source_runtime() {
        for (name, bits) in [
            ("NaN", 0x7ff8_0000_0000_0000_u64),
            ("POSITIVE_INFINITY", 0x7ff0_0000_0000_0000),
            ("NEGATIVE_INFINITY", 0xfff0_0000_0000_0000),
            ("MAX_VALUE", 0x7fef_ffff_ffff_ffff),
            ("MIN_VALUE", 0x1),
            ("MAX_SAFE_INTEGER", 0x433f_ffff_ffff_ffff),
            ("MIN_SAFE_INTEGER", 0xc33f_ffff_ffff_ffff),
            ("EPSILON", 0x3cb0_0000_0000_0000),
        ] {
            assert_eq!(number_constant(name).map(f64::to_bits), Some(bits), "Number.{name}");
        }
        for (name, bits) in [
            ("PI", 0x4009_21fb_5444_2d18_u64),
            ("E", 0x4005_bf0a_8b14_5769),
            ("LN2", 0x3fe6_2e42_fefa_39ef),
            ("LN10", 0x4002_6bb1_bbb5_5516),
            ("LOG2E", 0x3ff7_1547_652b_82fe),
            ("LOG10E", 0x3fdb_cb7b_1526_e50e),
            ("SQRT2", 0x3ff6_a09e_667f_3bcd),
            ("SQRT1_2", 0x3fe6_a09e_667f_3bcd),
        ] {
            assert_eq!(math_constant(name).map(f64::to_bits), Some(bits), "Math.{name}");
        }
    }
}
