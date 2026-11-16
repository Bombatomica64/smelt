//! Emission of Python `int` operations ([`Type::PyInt`]).
//!
//! A Python `int` is a `SmeltPyInt` (see `crate::py_int`), which is not `Copy`,
//! so every operator here borrows its operands, as `emitter::bigint` does for a
//! JavaScript bigint: `&a + &b`, the spelling a hand-written port uses with
//! operator impls on references, and one that keeps a loop accumulator from
//! being cloned twice per iteration.
//!
//! Behavioural requirements (`CPython`), checked by the parity fixture
//! `python_int_precision` against `python3`:
//!
//! * `+ - * & | ^ << >>` and unary `-`/`~` are exact at every magnitude
//!   (`2 ** 100`, `x * 2` past `i64`, `9223372036854775807 + 1`);
//! * `//` and `%` floor toward negative infinity and raise
//!   `ZeroDivisionError`; `/` is correctly rounded true division into a
//!   `float`;
//! * an `int` meets a `float` by mathematical value in a comparison
//!   (`2**53 + 1 > 2.0**53`), and converts to one only through an explicit
//!   `float(n)` or arithmetic with a `float`, which raises `OverflowError`
//!   past `f64`'s range.
//!
//! Type-preservation requirement: a Python `int` operand or result is never
//! spelled `i64`, `f64` or `SmeltUnknown`; it stays `SmeltPyInt` end to end.
//! The conversions out are explicit: a sequence index (`to_index`), a size
//! (`to_size`), a `float`, or a crossing into TypeScript, which reads it as a
//! `number`.

use super::call::RenderedCall;
use super::*;
use crate::py_int::PY_INT_TYPE;
use crate::rust::RustExpr;

impl FunctionEmitter<'_> {
    /// Whether `ty` is the Python `int` type.
    pub(super) fn is_py_int_type(&self, ty: TypeId) -> bool {
        matches!(self.mir.types.get(ty), Some(Type::PyInt))
    }

    /// Renders a BORROW of a Python `int` operand: a place in place (`&n`),
    /// anything else as a borrowed temporary, which lives to the end of the
    /// enclosing statement.
    pub(super) fn py_int_ref_expr(&self, operand: &Operand) -> Result<RustExpr, EmitError> {
        if let Some(place) = operand.place()
            && self.is_py_int_type(self.place_ty(place)?)
            && !self.place_reads_through_shared_capture(place)
            && !self.place_is_reference_class_field(place)
            && !self.index_place_read_is_owned(place)
        {
            return Ok(RustExpr::loose(format!(
                "&{}",
                self.place_expr(place)?.tight()
            )));
        }
        Ok(RustExpr::loose(format!(
            "&{}",
            self.operand_expr(operand)?.tight()
        )))
    }

    /// The conversion of a rendered value between a Python `int` and the
    /// machine scalars, or `None` when neither side is a Python `int`.
    ///
    /// * an `i64`, a fixed-width integer or a `bool` at a Python `int` is
    ///   exact (`len(xs)`, a `str.find` position, `True + 1`);
    /// * a Python `int` at a `float` is the nearest double, infinite past its
    ///   range: this is the crossing into a TypeScript `number`. A Python
    ///   `float(n)` raises instead, and lowers through its own checked cast;
    /// * a Python `int` at an `i64` is a machine position the frontend proved
    ///   in range;
    /// * a TypeScript `number` at a Python `int` (an argument of a Python
    ///   function called from TypeScript, a write to a Python `int` field) is
    ///   the `int` of its whole part, as Python's `int(x)` reads it. A `NaN`
    ///   or an infinity has no `int`; this crossing reads it as `0`, the one
    ///   lossy case of a boundary TypeScript's types cannot narrow.
    pub(super) fn py_int_conversion(
        &self,
        value_text: &RustExpr,
        source: TypeId,
        target: TypeId,
    ) -> Option<RustExpr> {
        match (self.mir.types.get(source)?, self.mir.types.get(target)?) {
            (Type::Int | Type::FixedInt(_) | Type::Bool, Type::PyInt) => Some(RustExpr::chain(
                format!("{PY_INT_TYPE}::from({})", value_text.delimited()),
            )),
            (Type::Float, Type::PyInt) => Some(RustExpr::chain(format!(
                "{PY_INT_TYPE}::from_f64({}).unwrap_or_default()",
                value_text.delimited()
            ))),
            (Type::PyInt, Type::Float) => Some(RustExpr::chain(format!(
                "({}).to_f64_lossy()",
                value_text.delimited()
            ))),
            (Type::PyInt, Type::Int) => Some(RustExpr::chain(format!(
                "({}).to_i64_saturating()",
                value_text.delimited()
            ))),
            _ => None,
        }
    }

    /// Renders a raising Python numeric operation (`Rvalue::PyNumericCall`).
    ///
    /// Every runtime method answers a `Result` whose error is the raised
    /// `SmeltPyException`; the trailing `?` is what marks the call fallible to
    /// `emit_throwing_call_terminator`, which routes the error to an
    /// enclosing `except`.
    pub(super) fn py_int_call(
        &self,
        op: smelt_hir::PyNumericOp,
        args: &[Operand],
    ) -> Result<RenderedCall, EmitError> {
        Ok(RenderedCall::propagating(self.py_numeric_text(op, args)?))
    }

    /// The text of a Python numeric operation's runtime call: a `Result` for
    /// one that can raise, the value for the others.
    fn py_numeric_text(
        &self,
        op: smelt_hir::PyNumericOp,
        args: &[Operand],
    ) -> Result<RustExpr, EmitError> {
        use smelt_hir::PyNumericOp;
        let arg = |index: usize| {
            args.get(index)
                .ok_or_else(|| EmitError::new(format!("{} is missing operand {index}", op.name())))
        };
        let int =
            |index: usize| -> Result<RustExpr, EmitError> { self.py_int_ref_expr(arg(index)?) };
        let method = |name: &str, operands: usize| -> Result<RustExpr, EmitError> {
            let receiver = int(0)?;
            let rest = (1..operands).map(int).collect::<Result<Vec<_>, _>>()?;
            Ok(RustExpr::chain(format!(
                "({receiver}).{name}({})",
                rest.iter()
                    .map(RustExpr::delimited)
                    .collect::<Vec<_>>()
                    .join(", "),
                receiver = receiver.delimited()
            )))
        };
        let text = match op {
            PyNumericOp::Parse => RustExpr::chain(format!(
                "{PY_INT_TYPE}::parse(&{}, 10)",
                self.operand_expr(arg(0)?)?.tight()
            )),
            PyNumericOp::ParseBase => RustExpr::chain(format!(
                "{PY_INT_TYPE}::parse_with_base(&{}, {})",
                self.operand_expr(arg(0)?)?.tight(),
                int(1)?.delimited()
            )),
            PyNumericOp::FromFloat => RustExpr::chain(format!(
                "{PY_INT_TYPE}::from_f64({})",
                self.operand_expr(arg(0)?)?.delimited()
            )),
            PyNumericOp::ToFloat => method("to_f64", 1)?,
            PyNumericOp::TrueDiv => method("true_div", 2)?,
            PyNumericOp::FloorDiv => method("floor_div", 2)?,
            PyNumericOp::Mod => method("modulo", 2)?,
            PyNumericOp::Pow => method("pow", 2)?,
            PyNumericOp::PowFloat => method("pow_float", 2)?,
            PyNumericOp::PowMod => method("pow_mod", 3)?,
            PyNumericOp::Shl => method("shl", 2)?,
            PyNumericOp::Shr => method("shr", 2)?,
            PyNumericOp::ToIndex => method("to_index", 1)?,
            PyNumericOp::ToSize => method("to_size", 1)?,
            PyNumericOp::RoundToInt => RustExpr::chain(format!(
                "smelt_py_round_float_to_int({})",
                self.operand_expr(arg(0)?)?.delimited()
            )),
            PyNumericOp::RoundFloat => RustExpr::chain(format!(
                "smelt_py_round_float({}, {})",
                self.operand_expr(arg(0)?)?.delimited(),
                self.operand_expr(arg(1)?)?.delimited()
            )),
            PyNumericOp::RoundInt => RustExpr::chain(format!(
                "smelt_py_round_int({}, {})",
                int(0)?.delimited(),
                self.operand_expr(arg(1)?)?.delimited()
            )),
            PyNumericOp::Chr => RustExpr::chain(format!(
                "smelt_py_chr({})",
                self.operand_expr(arg(0)?)?.delimited()
            )),
            PyNumericOp::Ord => RustExpr::chain(format!(
                "smelt_py_ord(&{})",
                self.operand_borrow_expr(arg(0)?)?.tight()
            )),
            PyNumericOp::ParseFloat => RustExpr::chain(format!(
                "smelt_py_float_parse(&{})",
                self.operand_expr(arg(0)?)?.tight()
            )),
            PyNumericOp::Range => RustExpr::chain(format!(
                "{PY_INT_TYPE}::range({}, {}, {}).map(SmeltList::from)",
                int(0)?.delimited(),
                int(1)?.delimited(),
                int(2)?.delimited()
            )),
            PyNumericOp::FloatPow => RustExpr::chain(format!(
                "smelt_py_float_pow({}, {})",
                self.operand_expr(arg(0)?)?.delimited(),
                self.operand_expr(arg(1)?)?.delimited()
            )),
        };
        Ok(text)
    }

    /// The value of a Python numeric operation that cannot raise
    /// (`PyNumericOp::can_raise` is false), on the assignment path.
    pub(super) fn py_numeric_value_expr(
        &self,
        op: smelt_hir::PyNumericOp,
        args: &[Operand],
    ) -> Result<RustExpr, EmitError> {
        self.py_numeric_text(op, args)
    }

    /// Renders a binary operation with a Python `int` operand, if this is one.
    ///
    /// Two `int`s meet through `SmeltPyInt`'s borrowed operators (`+ - * & | ^`,
    /// the comparisons); an `int` against a `float` compares by mathematical
    /// value (`cmp_f64`), never through a lossy cast. `None` for any other
    /// operand pair. Python's `/`, `//`, `%`, `**` and shifts never reach a
    /// `BinOp`: the frontend lowers them to `PyNumericCall`s, which can raise.
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "single-shape recogniser: only the arithmetic, bitwise and comparison operators have an `int` rendering; any other operator answers `None`"
    )]
    pub(super) fn py_int_binary_expr(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
        dest_ty: TypeId,
    ) -> Result<Option<RustExpr>, EmitError> {
        use smelt_hir::BinOp;
        let lhs_ty = self.operand_ty(lhs)?;
        let rhs_ty = self.operand_ty(rhs)?;
        let (lhs_int, rhs_int) = (self.is_py_int_type(lhs_ty), self.is_py_int_type(rhs_ty));
        if !lhs_int && !rhs_int {
            return Ok(None);
        }
        let comparison = match op {
            BinOp::Lt => Some("Less"),
            BinOp::Gt => Some("Greater"),
            BinOp::Eq | BinOp::StrictEq | BinOp::JsStrictEq | BinOp::JsLooseEq => Some("Equal"),
            _ => None,
        };
        let is_float = |ty: TypeId| matches!(self.mir.types.get(ty), Some(Type::Float));
        let (text, result) = if lhs_int && rhs_int {
            let lhs_ref = self.py_int_ref_expr(lhs)?;
            let rhs_ref = self.py_int_ref_expr(rhs)?;
            match op {
                BinOp::Add
                | BinOp::Sub
                | BinOp::Mul
                | BinOp::BitAnd
                | BinOp::BitOr
                | BinOp::BitXor => (
                    format!(
                        "{} {} {}",
                        lhs_ref.as_binary_operand(),
                        smelt_hir::bin_op_text(op),
                        rhs_ref.as_binary_operand()
                    ),
                    Type::PyInt,
                ),
                BinOp::Lt
                | BinOp::Lte
                | BinOp::Gt
                | BinOp::Gte
                | BinOp::Eq
                | BinOp::NotEq
                | BinOp::StrictEq
                | BinOp::StrictNotEq
                | BinOp::JsStrictEq
                | BinOp::JsStrictNotEq
                | BinOp::JsLooseEq
                | BinOp::JsLooseNotEq => (
                    format!(
                        "{} {} {}",
                        lhs_ref.as_binary_operand(),
                        smelt_hir::bin_op_text(op),
                        rhs_ref.as_binary_operand()
                    ),
                    Type::Bool,
                ),
                BinOp::Div
                | BinOp::Rem
                | BinOp::Shl
                | BinOp::Shr
                | BinOp::UShr
                | BinOp::And
                | BinOp::Or
                | BinOp::IMul => return Ok(None),
            }
        } else if (lhs_int && is_float(rhs_ty)) || (rhs_int && is_float(lhs_ty)) {
            // `int` against `float`: compare as `cmp_f64` answers from the
            // `int`'s side, flipping the operator when the `int` is on the
            // right. `!=` and `>=`/`<=` are the negations of `==`, `<` and `>`;
            // NaN answers `false` to all but `!=`, as in `CPython`.
            let (int_side, float_side, flipped) = if lhs_int {
                (lhs, rhs, false)
            } else {
                (rhs, lhs, true)
            };
            let int_ref = self.py_int_ref_expr(int_side)?;
            let float_text = self.operand_expr(float_side)?;
            let ordering = |name: &str| {
                format!(
                    "({}).cmp_f64({}) == Some(::std::cmp::Ordering::{name})",
                    int_ref.delimited(),
                    float_text.delimited()
                )
            };
            let flip = |side: &'static str| match (side, flipped) {
                ("Less", true) => "Greater",
                ("Greater", true) => "Less",
                (unflipped, _) => unflipped,
            };
            let text = match op {
                BinOp::Lt
                | BinOp::Gt
                | BinOp::Eq
                | BinOp::StrictEq
                | BinOp::JsStrictEq
                | BinOp::JsLooseEq => ordering(flip(comparison.unwrap_or("Equal"))),
                BinOp::Lte => format!(
                    "matches!(({}).cmp_f64({}), Some(::std::cmp::Ordering::{} | ::std::cmp::Ordering::Equal))",
                    int_ref.delimited(),
                    float_text.delimited(),
                    flip("Less")
                ),
                BinOp::Gte => format!(
                    "matches!(({}).cmp_f64({}), Some(::std::cmp::Ordering::{} | ::std::cmp::Ordering::Equal))",
                    int_ref.delimited(),
                    float_text.delimited(),
                    flip("Greater")
                ),
                BinOp::NotEq | BinOp::StrictNotEq | BinOp::JsStrictNotEq | BinOp::JsLooseNotEq => {
                    format!("!({})", ordering("Equal"))
                }
                _ => return Ok(None),
            };
            (text, Type::Bool)
        } else {
            return Ok(None);
        };
        let result_ty = self.type_id(result)?;
        if result_ty != dest_ty && self.mir.types.get(dest_ty) == Some(&Type::Unknown) {
            return Ok(Some(
                self.erase_rendered(&RustExpr::opaque(text), result_ty)?,
            ));
        }
        Ok(Some(RustExpr::loose(text)))
    }

    /// Python's `min(a, b, ...)` / `max(a, b, ...)` over `int`s, optionally
    /// followed by a spread list: a fold with `Ord::min`/`Ord::max`, exact at
    /// every magnitude. Equal `int`s are indistinguishable, so which of them
    /// the fold keeps cannot be observed.
    pub(super) fn py_int_extrema_expr(
        &self,
        op: smelt_hir::NumericExtremaOp,
        args: &[Operand],
        spread: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let method = match op {
            smelt_hir::NumericExtremaOp::Min => "min",
            smelt_hir::NumericExtremaOp::Max => "max",
        };
        let (first, rest) = args
            .split_first()
            .ok_or_else(|| EmitError::new("min()/max() of ints needs a first argument"))?;
        let mut rendered = self.value_at_type(first, dest_ty)?;
        for arg in rest {
            rendered = RustExpr::chain(format!(
                "::std::cmp::Ord::{method}({}, {})",
                rendered.delimited(),
                self.value_at_type(arg, dest_ty)?.delimited()
            ));
        }
        if let Some(spread_list) = spread {
            let list_text = list_read_expr(&self.operand_expr(spread_list)?);
            rendered = list_text.postfix(format!(
                ".iter().cloned().fold({}, ::std::cmp::Ord::{method})",
                rendered.delimited()
            ));
        }
        Ok(rendered)
    }
}
