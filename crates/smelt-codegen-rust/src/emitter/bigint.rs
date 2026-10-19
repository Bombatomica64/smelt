//! Emission of JavaScript `bigint` operations ([`Type::BigInt`]).
//!
//! A bigint is a `num_bigint::BigInt` (see `crate::bigint_prelude`), which is
//! not `Copy`, so every operator here works on BORROWED operands: `&a + &b`
//! rather than `a.clone() + b.clone()`. `num-bigint` implements its operators
//! for references, which is the spelling a hand-written port uses, and it keeps
//! a hot loop such as `result = (result << 8n) + BigInt(byte)` from cloning its
//! accumulator twice per iteration.
//!
//! Behavioral requirements (ECMA-262 `BigInt::*`), checked by the end-to-end
//! example `170_bigint_arbitrary_precision` against Node:
//!
//! * `+ - * & | ^` and unary `-` / `~` are exact at every magnitude;
//! * `/` truncates toward zero and `%` takes the dividend's sign; both throw a
//!   catchable `RangeError` on a zero divisor, as `**` does on a negative
//!   exponent (those three are fallible builtin calls, see
//!   [`FunctionEmitter::bigint_builtin_call_text`]);
//! * `>>` floors (`-5n >> 1n` is `-3n`), and a negative count reverses `<<`;
//! * a bigint compares with a `number` by mathematical value (`1n < 1.5`,
//!   `2n == 2`), `===` between the two is always `false`, and nothing is
//!   ordered against `NaN`;
//! * arithmetic mixing a bigint and a number is a `TypeError`.
//!
//! Type-preservation requirement: a bigint operand or result is never spelled
//! `f64`, `i64` or `SmeltUnknown`; it stays `num_bigint::BigInt` end to end.

use super::*;
use crate::bigint_prelude::BIGINT_TYPE;

impl FunctionEmitter<'_> {
    /// Whether `ty` is the JavaScript `bigint` type.
    pub(super) fn is_bigint_type(&self, ty: TypeId) -> bool {
        matches!(self.mir.types.get(ty), Some(Type::BigInt))
    }

    /// The `SmeltUnknown::BigInt` arm `arm` of an exhaustive match over the
    /// erased carrier, or nothing in a crate without bigints.
    ///
    /// The tag only exists when the crate carries a bigint (the variant is
    /// pay-for-use, like the rest of the bigint runtime), so a match that lists
    /// every tag splices this in instead of naming the variant unconditionally.
    pub(super) fn bigint_unknown_arm(&self, arm: &str) -> String {
        if self.context.needs_bigint() {
            format!("{arm} ")
        } else {
            String::new()
        }
    }

    /// Renders a BORROW of a bigint operand.
    ///
    /// A place is borrowed in place (`&value`), which is what lets an operator
    /// read it without the clone `operand_text` adds for a non-`Copy` read; a
    /// constant is borrowed as a temporary, which lives to the end of the
    /// enclosing statement.
    pub(super) fn bigint_ref_text(&self, operand: &Operand) -> Result<String, EmitError> {
        match operand {
            Operand::Copy(place) | Operand::Move(place)
                if self.is_bigint_type(self.place_ty(place)?)
                    && !self.place_reads_through_shared_capture(place)
                    && !self.place_is_reference_class_field(place)
                    && !self.index_place_read_is_owned(place) =>
            {
                Ok(format!("&{}", self.place_text(place)?))
            }
            Operand::Const(_) => Ok(format!("&{}", self.operand_text(operand)?)),
            _ => Ok(format!("&({})", self.operand_text(operand)?)),
        }
    }

    /// Renders a bigint operand as an OWNED `BigInt`, converting a numeric
    /// constant the frontend left untyped.
    fn bigint_owned_text(&self, operand: &Operand) -> Result<String, EmitError> {
        match operand {
            Operand::Const(Constant::BigInt(digits)) => Ok(crate::bigint_prelude::literal_text(digits)),
            _ => self.operand_text(operand),
        }
    }

    /// Whether a value of `ty` can hold a bigint at run time: a bigint, or a
    /// union (or optional) with a bigint member.
    pub(super) fn type_may_hold_bigint(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::BigInt) => true,
            Some(Type::Union(items)) => items.iter().any(|item| self.type_may_hold_bigint(*item)),
            Some(Type::Optional(inner)) => self.type_may_hold_bigint(*inner),
            _ => false,
        }
    }

    /// Renders an arithmetic, bitwise or shift operation whose operands are
    /// only known at run time to be a bigint or a number.
    ///
    /// `number | bigint` is the type of a value such as remeda's
    /// `let out = typeof data[0] === "bigint" ? 0n : 0`, and `out += value`
    /// has to add two bigints when both are bigints and two numbers when both
    /// are numbers -- which arm is live is decided by the values, not by any
    /// static type. Both operands are presented to the erased carrier through
    /// their `IntoSmeltUnknown` adapters (a `number | bigint` union is a tagged
    /// enum whose tag says which arm it holds) and `smelt_unknown_arithmetic`
    /// dispatches on the tags, throwing the JavaScript `TypeError` for a mixed
    /// pair; the result is recovered at the destination's own type. Narrowing
    /// the union to its `number` member instead is what used to panic on a
    /// bigint ("union guard selected an excluded member").
    fn bigint_dynamic_arithmetic_text(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
        dest_ty: TypeId,
    ) -> Result<Option<String>, EmitError> {
        use smelt_hir::BinOp;
        if !self.context.needs_bigint() {
            return Ok(None);
        }
        let op_text = match op {
            BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::Div
            | BinOp::Rem
            | BinOp::Shl
            | BinOp::Shr
            | BinOp::UShr
            | BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor => smelt_hir::bin_op_text(op),
            _ => return Ok(None),
        };
        let lhs_ty = self.operand_ty(lhs)?;
        let rhs_ty = self.operand_ty(rhs)?;
        let unions = |ty: TypeId| {
            !self.is_bigint_type(ty) && self.type_may_hold_bigint(ty)
        };
        if !unions(lhs_ty) && !unions(rhs_ty) {
            return Ok(None);
        }
        let text = format!(
            "smelt_unknown_arithmetic({op_text:?}, {}, {})",
            self.erase(lhs)?,
            self.erase(rhs)?
        );
        let unknown_ty = self.type_id(Type::Unknown)?;
        if dest_ty == unknown_ty {
            return Ok(Some(text));
        }
        Ok(Some(self.value_at_type_text(&text, unknown_ty, dest_ty, &self.render_scope())?))
    }

    /// Renders a binary operation with a bigint operand, if this is one.
    ///
    /// Returns `None` unless one operand is a bigint and the other is a bigint
    /// or a number; an erased or optional operand is left to the general
    /// emitter, which erases both sides and lets `SmeltUnknown`'s own operators
    /// decide at run time.
    pub(super) fn bigint_binary_text(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
        dest_ty: TypeId,
    ) -> Result<Option<String>, EmitError> {
        if let Some(text) = self.bigint_dynamic_arithmetic_text(op, lhs, rhs, dest_ty)? {
            return Ok(Some(text));
        }
        let lhs_ty = self.operand_ty(lhs)?;
        let rhs_ty = self.operand_ty(rhs)?;
        let lhs_big = self.is_bigint_type(lhs_ty);
        let rhs_big = self.is_bigint_type(rhs_ty);
        if !lhs_big && !rhs_big {
            return Ok(None);
        }
        let numeric = |ty: TypeId| {
            matches!(
                self.mir.types.get(ty),
                Some(Type::Float | Type::Int | Type::FixedInt(_))
            )
        };
        let text = if lhs_big && rhs_big {
            self.bigint_pair_text(op, lhs, rhs)?
        } else if (lhs_big && numeric(rhs_ty)) || (rhs_big && numeric(lhs_ty)) {
            self.bigint_mixed_text(op, lhs, rhs, lhs_big)?
        } else {
            return Ok(None);
        };
        let Some((rendered, result)) = text else {
            return Ok(None);
        };
        // A result stored into an erased slot (a `bigint` flowing into an
        // `unknown`) crosses the boundary through the ordinary erasure.
        let result_ty = self.type_id(result)?;
        if result_ty != dest_ty && self.mir.types.get(dest_ty) == Some(&Type::Unknown) {
            return Ok(Some(self.erase_value_text(&rendered, result_ty)?));
        }
        Ok(Some(rendered))
    }

    /// Renders an operation between two bigints and its result type.
    fn bigint_pair_text(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
    ) -> Result<Option<(String, Type)>, EmitError> {
        use smelt_hir::BinOp;
        let lhs_ref = self.bigint_ref_text(lhs)?;
        let rhs_ref = self.bigint_ref_text(rhs)?;
        let text = match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor => {
                (
                    format!("{lhs_ref} {} {rhs_ref}", smelt_hir::bin_op_text(op)),
                    Type::BigInt,
                )
            }
            BinOp::Shl => (format!("smelt_bigint_shl({lhs_ref}, {rhs_ref})"), Type::BigInt),
            BinOp::Shr => (format!("smelt_bigint_shr({lhs_ref}, {rhs_ref})"), Type::BigInt),
            // Fallible: the frontend lowers bigint `/` and `%` to a builtin call
            // (`BigIntOp::Div`/`Rem`), so a `BinOp` never carries them; render
            // the same helpers here should a pass ever rebuild one, with the
            // throw routed through the panic channel.
            BinOp::Div => (
                format!("smelt_bigint_div({lhs_ref}, {rhs_ref}).unwrap_or_else(|error| smelt_panic_throw(error))"),
                Type::BigInt,
            ),
            BinOp::Rem => (
                format!("smelt_bigint_rem({lhs_ref}, {rhs_ref}).unwrap_or_else(|error| smelt_panic_throw(error))"),
                Type::BigInt,
            ),
            BinOp::UShr => (
                "smelt_bigint_type_error(\"BigInts have no unsigned right shift, use >> instead\")"
                    .to_owned(),
                Type::BigInt,
            ),
            BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte => (
                format!("{lhs_ref} {} {rhs_ref}", smelt_hir::bin_op_text(op)),
                Type::Bool,
            ),
            BinOp::Eq | BinOp::StrictEq | BinOp::JsStrictEq => {
                (format!("{lhs_ref} == {rhs_ref}"), Type::Bool)
            }
            BinOp::NotEq | BinOp::StrictNotEq | BinOp::JsStrictNotEq => {
                (format!("{lhs_ref} != {rhs_ref}"), Type::Bool)
            }
            BinOp::And | BinOp::Or | BinOp::IMul => return Ok(None),
        };
        Ok(Some(text))
    }

    /// Renders an operation between a bigint and a number.
    ///
    /// Comparisons are by mathematical value; `===` is `false` because the two
    /// are different types; arithmetic throws JavaScript's `TypeError`.
    fn bigint_mixed_text(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
        lhs_big: bool,
    ) -> Result<Option<(String, Type)>, EmitError> {
        use smelt_hir::BinOp;
        let (big, number) = if lhs_big { (lhs, rhs) } else { (rhs, lhs) };
        let big_ref = self.bigint_ref_text(big)?;
        let number_text = if matches!(self.mir.types.get(self.operand_ty(number)?), Some(Type::Float)) {
            self.operand_text(number)?
        } else {
            format!("{} as f64", self.operand_text(number)?)
        };
        let ordering = format!("smelt_bigint_compare_number({big_ref}, {number_text})");
        // The helper orders `bigint` against `number`; a `number < bigint`
        // comparison reads the same ordering from the other side.
        let (less, greater) = if lhs_big {
            ("Less", "Greater")
        } else {
            ("Greater", "Less")
        };
        let text = match op {
            BinOp::Lt => format!("matches!({ordering}, Some(::std::cmp::Ordering::{less}))"),
            BinOp::Gt => format!("matches!({ordering}, Some(::std::cmp::Ordering::{greater}))"),
            BinOp::Lte => format!(
                "matches!({ordering}, Some(::std::cmp::Ordering::{less} | ::std::cmp::Ordering::Equal))"
            ),
            BinOp::Gte => format!(
                "matches!({ordering}, Some(::std::cmp::Ordering::{greater} | ::std::cmp::Ordering::Equal))"
            ),
            BinOp::Eq => format!("matches!({ordering}, Some(::std::cmp::Ordering::Equal))"),
            BinOp::NotEq => format!("!matches!({ordering}, Some(::std::cmp::Ordering::Equal))"),
            // A bigint is never strictly equal to (or the same value as) a number.
            BinOp::JsStrictEq | BinOp::StrictEq => "false".to_owned(),
            BinOp::JsStrictNotEq | BinOp::StrictNotEq => "true".to_owned(),
            BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::Div
            | BinOp::Rem
            | BinOp::Shl
            | BinOp::Shr
            | BinOp::UShr
            | BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor => {
                return Ok(Some((
                    "smelt_bigint_type_error(\"Cannot mix BigInt and other types, use explicit conversions\")"
                        .to_owned(),
                    Type::BigInt,
                )));
            }
            BinOp::And | BinOp::Or | BinOp::IMul => return Ok(None),
        };
        Ok(Some((text, Type::Bool)))
    }

    /// Renders unary `-` over a bigint, if the operand is one.
    pub(super) fn bigint_negation_text(&self, operand: &Operand) -> Result<Option<String>, EmitError> {
        if !self.is_bigint_type(self.operand_ty(operand)?) {
            return Ok(None);
        }
        Ok(Some(format!("-{}", self.bigint_ref_text(operand)?)))
    }

    /// Renders a fallible bigint builtin call (`BuiltinFn::BigInt`).
    ///
    /// The trailing `?` is what marks the call fallible to
    /// `emit_throwing_call_terminator`, which binds the caught `RangeError`
    /// and jumps to the handler's catch block.
    pub(super) fn bigint_builtin_call_text(
        &self,
        op: smelt_hir::BigIntOp,
        args: &[Operand],
    ) -> Result<String, EmitError> {
        use smelt_hir::BigIntOp;
        let arg = |index: usize| {
            args.get(index).ok_or_else(|| {
                EmitError::new(format!("{} is missing operand {index}", op.name()))
            })
        };
        match op {
            BigIntOp::From => self.bigint_from_text(arg(0)?),
            BigIntOp::AsIntN | BigIntOp::AsUintN => {
                let helper = if op == BigIntOp::AsIntN {
                    "smelt_bigint_as_int_n"
                } else {
                    "smelt_bigint_as_uint_n"
                };
                let bits = arg(0)?;
                let value = arg(1)?;
                let bits_text = self.bigint_width_text(bits)?;
                Ok(format!("{helper}({bits_text}, {})?", self.bigint_operand_ref_text(value)?))
            }
            BigIntOp::Div | BigIntOp::Rem | BigIntOp::Pow => {
                let helper = match op {
                    BigIntOp::Div => "smelt_bigint_div",
                    BigIntOp::Rem => "smelt_bigint_rem",
                    _ => "smelt_bigint_pow",
                };
                Ok(format!(
                    "{helper}({}, {})?",
                    self.bigint_operand_ref_text(arg(0)?)?,
                    self.bigint_operand_ref_text(arg(1)?)?
                ))
            }
        }
    }

    /// A borrowed bigint operand, converting an erased one at the boundary.
    ///
    /// An `unknown` operand reaching a bigint operation (a generic or `any`
    /// value) goes through the checked `ToBigInt` adapter rather than a cast.
    fn bigint_operand_ref_text(&self, operand: &Operand) -> Result<String, EmitError> {
        let ty = self.operand_ty(operand)?;
        if self.is_bigint_type(ty) {
            return self.bigint_ref_text(operand);
        }
        Ok(format!("&{}", self.bigint_from_value_text(operand, ty, true)?))
    }

    /// `BigInt(value)`, ending in `?`.
    fn bigint_from_text(&self, operand: &Operand) -> Result<String, EmitError> {
        let ty = self.operand_ty(operand)?;
        if self.is_bigint_type(ty) {
            // `BigInt(1n)` is the value itself; the `Ok(..)?` keeps the call
            // shape the throwing-terminator emitter expects.
            return Ok(format!(
                "Ok::<{BIGINT_TYPE}, Box<dyn ::std::error::Error>>({})?",
                self.bigint_owned_text(operand)?
            ));
        }
        Ok(format!("{}?", self.bigint_from_value_text(operand, ty, false)?))
    }

    /// The `Result`-returning conversion of a non-bigint value to a bigint.
    ///
    /// With `panic_route`, the result is unwrapped through the panic channel
    /// instead (for a conversion nested inside another operation's arguments).
    fn bigint_from_value_text(
        &self,
        operand: &Operand,
        ty: TypeId,
        panic_route: bool,
    ) -> Result<String, EmitError> {
        let text = match self.mir.types.get(ty) {
            Some(Type::Float) => {
                format!("smelt_bigint_from_number({})", self.operand_text(operand)?)
            }
            Some(Type::Int | Type::FixedInt(_)) => {
                format!("smelt_bigint_from_number({} as f64)", self.operand_text(operand)?)
            }
            Some(Type::String) => {
                format!("smelt_bigint_from_string(&{})", self.operand_text(operand)?)
            }
            // `BigInt(true)` is `1n`; a boolean never throws.
            Some(Type::Bool) => format!(
                "Ok::<{BIGINT_TYPE}, Box<dyn ::std::error::Error>>({BIGINT_TYPE}::from(i64::from({})))",
                self.operand_text(operand)?
            ),
            _ => format!("smelt_bigint_from_unknown(&{})", self.erase(operand)?),
        };
        Ok(if panic_route {
            format!("{text}.unwrap_or_else(|error| smelt_panic_throw(error))")
        } else {
            text
        })
    }

    /// An `asIntN`/`asUintN` width as the `f64` the runtime's `ToIndex` takes.
    fn bigint_width_text(&self, bits: &Operand) -> Result<String, EmitError> {
        match self.mir.types.get(self.operand_ty(bits)?) {
            Some(Type::Float) => self.operand_text(bits),
            Some(Type::Int | Type::FixedInt(_)) => {
                Ok(format!("{} as f64", self.operand_text(bits)?))
            }
            _ => {
                let float_ty = self.type_id(Type::Float)?;
                self.primitive_cast_text(smelt_hir::PrimitiveCastOp::ToFloat, bits, float_ty)
            }
        }
    }
}
