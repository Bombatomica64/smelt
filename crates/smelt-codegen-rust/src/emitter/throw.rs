//! Emission of `throw` and of the `catch` binding that recovers its payload.
//!
//! Both the function-level terminator emitter (`control_flow.rs`) and the
//! reduced closure-body emitter (`closures.rs`) end a `Terminator::Throw` here so
//! the two paths cannot drift apart, and the `catch` side reads the payload back
//! through the same ABI. See `crate::thrown` for why the payload crosses the
//! error channel as a `SmeltUnknown`.

use super::*;
use crate::{stdlib, thrown};

impl FunctionEmitter<'_> {
    /// Renders the `return Err(..)` statement for a `Terminator::Throw`.
    ///
    /// The thrown operand is erased to `SmeltUnknown` and handed to the
    /// payload-preserving `smelt_throw` adapter, so a `catch` can recover the
    /// value's class, `name`, `message` and custom fields instead of the
    /// `format!("{}", value)` text that previously replaced them (which rendered
    /// every erased `Error` as the literal `[object Object]`).
    ///
    /// The `Err` variant is annotated with the channel's error type so Rust never
    /// has to infer `E`: a throwing closure whose result is only stored, never
    /// called at a site that pins the error type, would otherwise leave `E`
    /// ambiguous (E0283).
    ///
    /// A program that never uses an erased value has no `SmeltUnknown` in its
    /// prelude, and therefore no payload adapter either; such a throw keeps the
    /// plain stringified `std::io::Error` form, which loses nothing because
    /// without `SmeltUnknown` there is no erased `catch` binding to observe a
    /// payload.
    ///
    /// The erasure goes through `erase_value_text` rather than `value_at_type`
    /// because `Type::Unknown` need not be interned in the type table even when
    /// the prelude defines `SmeltUnknown` (a program can reach `SmeltUnknown`
    /// through a runtime helper without ever naming an `unknown`-typed value), and
    /// requiring the id there would spuriously fail emission.
    pub(super) fn throw_terminator_text(&self, operand: &Operand) -> Result<String, EmitError> {
        let payload = if stdlib::needs_unknown_type(self.mir) {
            let operand_ty = self.operand_ty(operand)?;
            let value_text = self.operand_text(operand)?;
            let erased = if matches!(self.mir.types.get(operand_ty), Some(Type::Unknown)) {
                value_text
            } else {
                self.erase_value_text(&value_text, operand_ty)?
            };
            thrown::throw_expr(&erased)
        } else {
            format!(
                "std::io::Error::new(std::io::ErrorKind::Other, format!(\"{{}}\", {})).into()",
                self.operand_text(operand)?
            )
        };
        Ok(format!(
            "    return Err::<_, Box<dyn std::error::Error>>({payload});\n"
        ))
    }

    /// Renders the `catch` binding for a caught `Box<dyn std::error::Error>`.
    ///
    /// `error_text` names the binding holding the caught error. An erased
    /// (`Unknown`-typed) catch parameter recovers the original thrown payload;
    /// a string-typed one keeps the error's `Display` text, which the payload
    /// ABI defines as the payload's `message` field when it has one.
    pub(super) fn caught_error_value_text(
        &self,
        exception_ty: TypeId,
        error_text: &str,
    ) -> Result<String, EmitError> {
        match self.mir.types.get(exception_ty) {
            Some(Type::String) => Ok(format!("{error_text}.to_string()")),
            Some(Type::Unknown) => Ok(thrown::thrown_value_expr(error_text)),
            _ => self.default_value(exception_ty),
        }
    }
}
