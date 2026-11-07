//! Emission of the operations that can throw.
//!
//! MIR carries such an op as the callee of a call terminator (`Callee::Op`) so
//! its error reaches an enclosing `try`. Every op answers here, once, with both
//! things the call terminator asks for: the text of the call, whose propagating
//! `?` is what `emit_throwing_call_terminator` turns into the `Ok(Ok(v)) /
//! Ok(Err(e))` shape that binds the caught error, and the typed value the
//! adapter answers. Only the throw crosses the erased channel.

use crate::rust::RustExpr;
use smelt_hir::{Base64Op, PythonDivisionOp, Type, TypeId, UriTranscodeOp};
use smelt_mir::{Operand, Rvalue};

use super::call::RenderedCall;
use super::{EmitError, FunctionEmitter};

/// How one fallible operation is called.
pub(super) struct FallibleOpCall {
    /// The rendered call.
    pub(super) text: RenderedCall,
    /// The type the call answers, before coercion to the destination.
    pub(super) source_ty: TypeId,
}

impl FunctionEmitter<'_> {
    /// The runtime function of a Python division over operands of `ty`.
    fn python_division_fn(
        &self,
        op: PythonDivisionOp,
        ty: TypeId,
    ) -> Result<&'static str, EmitError> {
        match (op, self.mir.types.get(ty)) {
            (PythonDivisionOp::FloorDiv, Some(Type::Int)) => Ok("smelt_py_floor_div_int"),
            (PythonDivisionOp::Mod, Some(Type::Int)) => Ok("smelt_py_mod_int"),
            (PythonDivisionOp::FloorDiv, Some(Type::Float)) => Ok("smelt_py_floor_div_float"),
            (PythonDivisionOp::Mod, Some(Type::Float)) => Ok("smelt_py_mod_float"),
            (PythonDivisionOp::TrueDiv, Some(Type::Float)) => Ok("smelt_py_true_div"),
            _ => Err(EmitError::new("a Python division needs two ints or two floats")),
        }
    }

    /// Renders Python's `d[k]`: the value, or a raised `KeyError`.
    fn python_dict_read_call(
        &self,
        dict: &Operand,
        key: &Operand,
    ) -> Result<(RenderedCall, TypeId), EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        let Some(Type::Dict(key_ty, value_ty)) = self.mir.types.get(dict_ty) else {
            return Err(EmitError::new("a Python dict read needs a dict"));
        };
        let key_text = self.dict_key_reference_expr(key, *key_ty)?;
        let dict_text = self.operand_borrow_expr(dict)?;
        let cloned = if self.map_op_uses_smelt_record(dict_ty, *key_ty) { "" } else { ".cloned()" };
        Ok((
            RenderedCall::propagating(format!(
                "{dict_text}.get({key_text}){cloned}.ok_or_else(|| smelt_py_key_error({key_text}))"
            )),
            *value_ty,
        ))
    }

    /// Renders a Python division whose divisor is a nonzero constant.
    pub(super) fn python_division_expr(
        &self,
        op: PythonDivisionOp,
        lhs: &Operand,
        rhs: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let divide = self.python_division_fn(op, self.operand_ty(lhs)?)?;
        Ok(RustExpr::postfix_on(&divide, format!("{divide}({}, {})", self.operand_expr(lhs)?, self.operand_expr(rhs)?)))
    }

    /// Renders the call of a fallible operation and states what it answers.
    pub(super) fn fallible_op_call(&self, op: &Rvalue) -> Result<FallibleOpCall, EmitError> {
        let mut args = Vec::new();
        op.reads(|operand| args.push(operand.clone()));
        let (text, source_ty) = match op {
            Rvalue::JsonParse { target, .. } => self.json_parse_call(*target, &args)?,
            Rvalue::PythonDivision { op, lhs, rhs, .. } => {
                let source_ty = self.operand_ty(lhs)?;
                let divide = self.python_division_fn(*op, source_ty)?;
                let lhs = self.operand_expr(lhs)?;
                let rhs = self.operand_expr(rhs)?;
                (
                    RenderedCall::propagating(format!(
                        "smelt_py_checked_division({lhs}, {rhs}, {divide})"
                    )),
                    source_ty,
                )
            }
            Rvalue::PythonDictRead { dict, key } => self.python_dict_read_call(dict, key)?,
            Rvalue::PythonListRead { list, index } => {
                let list_ty = self.operand_ty(list)?;
                let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
                    return Err(EmitError::new("a Python list read needs a list"));
                };
                let list_text = self.operand_borrow_expr(list)?;
                let index_text = self.operand_expr(index)?;
                (
                    RenderedCall::propagating(format!(
                        "smelt_py_list_read(&{list_text}, {index_text})"
                    )),
                    *item_ty,
                )
            }
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
                    RenderedCall::propagating(RustExpr::postfix_on(&adapter, format!("{adapter}(&{value_text})"))),
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
                (self.data_view_access(*write, *element, &args)?, source_ty)
            }
            Rvalue::BigIntCall { op, .. } => (
                self.bigint_builtin_call(*op, &args)?,
                self.type_id(if op.returns_string() {
                    Type::String
                } else {
                    Type::BigInt
                })?,
            ),
            Rvalue::HostModuleCall { op, .. } => (
                self.host_module_call(*op, &args)?,
                self.host_module_call_source_ty(*op)?,
            ),
            Rvalue::UrlNew { .. } => {
                (self.url_new_call(&args)?, self.url_call_source_ty(true)?)
            }
            Rvalue::UrlOp { .. } => {
                (self.url_set_href_call(&args)?, self.url_call_source_ty(false)?)
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
                    RenderedCall::propagating(RustExpr::postfix_on(&crate::thrown::REGEXP_COMPILE_FN, format!(
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
                    RenderedCall::propagating(RustExpr::postfix_on(&adapter, format!("{adapter}(&{value_text})"))),
                    self.type_id(Type::String)?,
                )
            }
            other => {
                return Err(EmitError::new(format!(
                    "internal: `{other:?}` is not a fallible operation"
                )));
            }
        };
        // A throwing op that answers text (`JSON.stringify`, `atob`, a
        // decode, a digest) builds it as a UTF-8 `String`; a JavaScript string
        // result is that text as a `JsString`. `From`'s identity impl keeps an
        // op that already answers `JsString` unchanged.
        let answered = if self.mir.types.get(source_ty) == Some(&Type::String) && !matches!(op, Rvalue::JsonParse { .. }) {
            if text.propagates() {
                text.map_call(|call| format!("{call}.map(JsString::from)"))
            } else {
                text.map_call(|call| format!("JsString::from({call})"))
            }
        } else {
            text
        };
        Ok(FallibleOpCall { text: answered, source_ty })
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
    ) -> Result<(RenderedCall, TypeId), EmitError> {
        let text = args
            .first()
            .ok_or_else(|| EmitError::new("JSON parse takes one string argument"))?;
        if !matches!(self.mir.types.get(self.operand_ty(text)?), Some(Type::String | Type::PyStr)) {
            return Err(EmitError::new("JSON parse input must be a string"));
        }
        if let Some(decoded) = self.json_decode_target(target) {
            let call = format!(
                "{}(&{})",
                super::json_decode::json_decode_entry_name(decoded),
                self.operand_expr(text)?
            );
            return Ok((RenderedCall::propagating(call), decoded));
        }
        let call = format!("{}(&{})", crate::thrown::JSON_PARSE_FN, self.operand_expr(text)?);
        Ok((RenderedCall::propagating(call), self.type_id(Type::Unknown)?))
    }
}
