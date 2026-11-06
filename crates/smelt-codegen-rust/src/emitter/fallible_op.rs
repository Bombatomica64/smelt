//! Emission of the operations that can throw.
//!
//! MIR carries such an op as the callee of a call terminator (`Callee::Op`) so
//! its error reaches an enclosing `try`. Every op answers here, once, with both
//! things the call terminator asks for: the text of the call, whose propagating
//! `?` is what `emit_throwing_call_terminator` turns into the `Ok(Ok(v)) /
//! Ok(Err(e))` shape that binds the caught error, and the typed value the
//! adapter answers. Only the throw crosses the erased channel.

use crate::rust::RustExpr;
use smelt_hir::{Base64Op, Type, TypeId, UriTranscodeOp};
use smelt_mir::{Operand, Rvalue};

use super::call::CallText;
use super::{EmitError, FunctionEmitter};

/// How one fallible operation is called.
pub(super) struct FallibleOpCall {
    /// The rendered call.
    pub(super) text: CallText,
    /// The type the call answers, before coercion to the destination.
    pub(super) source_ty: TypeId,
}

impl FunctionEmitter<'_> {
    /// Renders the call of a fallible operation and states what it answers.
    pub(super) fn fallible_op_call(&self, op: &Rvalue) -> Result<FallibleOpCall, EmitError> {
        let mut args = Vec::new();
        op.reads(|operand| args.push(operand.clone()));
        let (text, source_ty) = match op {
            Rvalue::JsonParse { target, .. } => self.json_parse_call(*target, &args)?,
            Rvalue::Base64Transcode { op, .. } => {
                let value = args.first().ok_or_else(|| {
                    EmitError::new("a base64 transcoder takes one string argument")
                })?;
                let adapter = match op {
                    Base64Op::Encode => crate::thrown::BTOA_FN,
                    Base64Op::Decode => crate::thrown::ATOB_FN,
                };
                let value_text = self.string_like_operand_expr(value, "base64 input")?;
                (
                    CallText::propagating(RustExpr::postfix_on(&adapter, format!("{adapter}({value_text}.as_str())"))),
                    self.type_id(Type::String)?,
                )
            }
            Rvalue::DataViewAccess { write, element, .. } => {
                let source_ty = if *write {
                    self.none_ty
                } else if element.is_bigint() {
                    self.type_id(Type::BigInt)?
                } else {
                    self.type_id(Type::Float)?
                };
                (self.data_view_access_text(*write, *element, &args)?, source_ty)
            }
            Rvalue::BigIntCall { op, .. } => (
                self.bigint_builtin_call_text(*op, &args)?,
                self.type_id(if op.returns_string() {
                    Type::String
                } else {
                    Type::BigInt
                })?,
            ),
            Rvalue::HostModuleCall { op, .. } => (
                self.host_module_call_text(*op, &args)?,
                self.host_module_call_source_ty(*op)?,
            ),
            Rvalue::UrlNew { .. } => {
                (self.url_new_call_text(&args)?, self.url_call_source_ty(true)?)
            }
            Rvalue::UrlOp { .. } => {
                (self.url_set_href_call_text(&args)?, self.url_call_source_ty(false)?)
            }
            Rvalue::RegExpCompile { .. } => {
                let pattern = args.first().map_or_else(
                    || Ok(RustExpr::atom("String::new()")),
                    |arg| self.string_like_operand_expr(arg, "RegExp pattern"),
                )?;
                let flags = args.get(1).map_or_else(
                    || Ok(RustExpr::atom("String::new()")),
                    |arg| self.string_like_operand_expr(arg, "RegExp flags"),
                )?;
                let source_ty = self.regexp_class_ty().ok_or_else(|| {
                    EmitError::new("a RegExp compile call needs the RegExp class type interned")
                })?;
                (
                    CallText::propagating(RustExpr::postfix_on(&crate::thrown::REGEXP_COMPILE_FN, format!(
                        "{}({pattern}, {flags})",
                        crate::thrown::REGEXP_COMPILE_FN
                    ))),
                    source_ty,
                )
            }
            Rvalue::UriTranscode { op, .. } => {
                let value = args
                    .first()
                    .ok_or_else(|| EmitError::new("a URI decoder takes one string argument"))?;
                let adapter = match op {
                    UriTranscodeOp::Decode => crate::thrown::DECODE_URI_FN,
                    UriTranscodeOp::DecodeComponent => crate::thrown::DECODE_URI_COMPONENT_FN,
                    UriTranscodeOp::Encode | UriTranscodeOp::EncodeComponent => {
                        return Err(EmitError::new(
                            "internal: an infallible URI encoder reached the fallible call path",
                        ));
                    }
                };
                let value_text = self.string_like_operand_expr(value, "URI decoder input")?;
                (
                    CallText::propagating(RustExpr::postfix_on(&adapter, format!("{adapter}({value_text}.as_str())"))),
                    self.type_id(Type::String)?,
                )
            }
            other => {
                return Err(EmitError::new(format!(
                    "internal: `{other:?}` is not a fallible operation"
                )));
            }
        };
        Ok(FallibleOpCall { text, source_ty })
    }

    /// Renders `JSON.parse`. A source-declared, decodable target is a typed
    /// entry boundary: parse AND decode strictly, throwing a catchable
    /// `TypeError` on a mismatch (see `emitter::json_decode`). Otherwise the
    /// parse answers a dynamic value, and the destination's own type drives the
    /// ordinary coercion from the erased carrier.
    fn json_parse_call(
        &self,
        target: Option<TypeId>,
        args: &[Operand],
    ) -> Result<(CallText, TypeId), EmitError> {
        let text = args
            .first()
            .ok_or_else(|| EmitError::new("JSON parse takes one string argument"))?;
        if !matches!(self.mir.types.get(self.operand_ty(text)?), Some(Type::String)) {
            return Err(EmitError::new("JSON parse input must be a string"));
        }
        if let Some(decoded) = self.json_decode_target(target) {
            let call = format!(
                "{}(&{})",
                super::json_decode::json_decode_entry_name(decoded),
                self.operand_expr(text)?
            );
            return Ok((CallText::propagating(call), decoded));
        }
        let call = format!("{}(&{})", crate::thrown::JSON_PARSE_FN, self.operand_expr(text)?);
        Ok((CallText::propagating(call), self.type_id(Type::Unknown)?))
    }
}
