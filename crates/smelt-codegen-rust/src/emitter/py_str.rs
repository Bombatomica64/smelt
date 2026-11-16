//! Emission of Python `str` methods with `CPython`'s own rules
//! (`Rvalue::PyStrCall`, see `smelt_hir::PyStrOp`).
//!
//! Each method is one call of the runtime's `py_str.rs` helper of the same
//! name, which works in code points as CPython does. The receiver and every
//! text operand are borrowed; position and count operands are machine
//! integers the frontend already converted (bounds saturated, as CPython
//! clamps them). A method that can raise answers a `Result`, and its call is
//! a fallible call terminator, so the `ValueError`/`TypeError` reaches an
//! enclosing `except`.
//!
//! Behavioural requirement: `CPython` 3.14's results, checked by the parity
//! fixture `python_str_methods`. Type-preservation requirement: a `str` stays
//! `String`, a position or count is a machine `i64` the frontend converts to a
//! Python `int`, and a split is a `SmeltList<String>`.

use super::call::RenderedCall;
use super::*;
use crate::rust::RustExpr;
use smelt_hir::PyStrOp;

impl FunctionEmitter<'_> {
    /// The call of a Python `str` method: a `Result` for one that can raise
    /// (`PyStrOp::can_raise`), the value otherwise.
    pub(super) fn py_str_call_expr(
        &self,
        op: PyStrOp,
        args: &[Operand],
    ) -> Result<RustExpr, EmitError> {
        let arg = |index: usize| {
            args.get(index).ok_or_else(|| {
                EmitError::new(format!("str.{}() is missing operand {index}", op.name()))
            })
        };
        let text = |index: usize| -> Result<String, EmitError> {
            Ok(format!(
                "&{}",
                self.operand_borrow_expr(arg(index)?)?.tight()
            ))
        };
        let number = |index: usize| -> Result<String, EmitError> {
            Ok(self.operand_expr(arg(index)?)?.delimited().into())
        };
        let call = match op {
            PyStrOp::Title => format!("smelt_py_str_title({})", text(0)?),
            PyStrOp::Capitalize => format!("smelt_py_str_capitalize({})", text(0)?),
            PyStrOp::SwapCase => format!("smelt_py_str_swapcase({})", text(0)?),
            PyStrOp::ZFill => format!("smelt_py_str_zfill({}, {})", text(0)?, number(1)?),
            PyStrOp::Center | PyStrOp::LJust | PyStrOp::RJust => {
                let side = if op == PyStrOp::LJust {
                    'l'
                } else if op == PyStrOp::RJust {
                    'r'
                } else {
                    'c'
                };
                format!(
                    "smelt_py_str_pad({}, {}, {}, {side:?})",
                    text(0)?,
                    number(1)?,
                    text(2)?
                )
            }
            PyStrOp::Count => format!(
                "smelt_py_str_count({}, {}, {}, {})",
                text(0)?,
                text(1)?,
                number(2)?,
                number(3)?
            ),
            PyStrOp::Find | PyStrOp::RFind => format!(
                "smelt_py_str_find({}, {}, {}, {}, {})",
                text(0)?,
                text(1)?,
                number(2)?,
                number(3)?,
                op == PyStrOp::RFind
            ),
            PyStrOp::Index | PyStrOp::RIndex => format!(
                "smelt_py_str_index({}, {}, {}, {}, {})",
                text(0)?,
                text(1)?,
                number(2)?,
                number(3)?,
                op == PyStrOp::RIndex
            ),
            PyStrOp::SplitWhitespace | PyStrOp::RSplitWhitespace => format!(
                "SmeltList::from(smelt_py_str_split_whitespace({}, {}, {}))",
                text(0)?,
                number(1)?,
                op == PyStrOp::RSplitWhitespace
            ),
            PyStrOp::Split | PyStrOp::RSplit => format!(
                "smelt_py_str_split({}, {}, {}, {}).map(SmeltList::from)",
                text(0)?,
                text(1)?,
                number(2)?,
                op == PyStrOp::RSplit
            ),
            PyStrOp::Replace => format!(
                "smelt_py_str_replace({}, {}, {}, {})",
                text(0)?,
                text(1)?,
                text(2)?,
                number(3)?
            ),
            PyStrOp::StripChars | PyStrOp::LStripChars | PyStrOp::RStripChars => {
                let side = if op == PyStrOp::LStripChars {
                    'l'
                } else if op == PyStrOp::RStripChars {
                    'r'
                } else {
                    'b'
                };
                format!(
                    "smelt_py_str_strip_chars({}, {}, {side:?})",
                    text(0)?,
                    text(1)?
                )
            }
        };
        Ok(RustExpr::chain(call))
    }

    /// A raising Python `str` method as a fallible call, with the type it
    /// answers.
    pub(super) fn py_str_fallible_call(
        &self,
        op: PyStrOp,
        args: &[Operand],
    ) -> Result<(RenderedCall, TypeId), EmitError> {
        let answer = if matches!(op, PyStrOp::Index | PyStrOp::RIndex) {
            self.type_id(Type::Int)?
        } else if matches!(op, PyStrOp::Split | PyStrOp::RSplit) {
            let text = self.type_id(Type::PyStr)?;
            self.type_id(Type::List(text))?
        } else {
            self.type_id(Type::PyStr)?
        };
        Ok((
            RenderedCall::propagating(self.py_str_call_expr(op, args)?),
            answer,
        ))
    }

    /// Python's `format(value, spec)` over an `int`, a `float` or a `str`:
    /// the runtime's `py_format.rs` helper for the value's type.
    pub(super) fn py_format_call(
        &self,
        value: &Operand,
        spec: &str,
    ) -> Result<(RenderedCall, TypeId), EmitError> {
        let helper = match self.mir.types.get(self.operand_ty(value)?) {
            Some(Type::PyInt) => format!(
                "smelt_py_format_int({}, {spec:?})",
                self.py_int_ref_expr(value)?.delimited()
            ),
            Some(Type::Float) => format!(
                "smelt_py_format_float({}, {spec:?})",
                self.operand_expr(value)?.delimited()
            ),
            Some(Type::PyStr) => format!(
                "smelt_py_format_str(&{}, {spec:?})",
                self.operand_borrow_expr(value)?.tight()
            ),
            _ => {
                return Err(EmitError::new(
                    "a Python format spec needs an int, a float or a str",
                ));
            }
        };
        Ok((
            RenderedCall::propagating(RustExpr::chain(helper)),
            self.type_id(Type::PyStr)?,
        ))
    }
}
