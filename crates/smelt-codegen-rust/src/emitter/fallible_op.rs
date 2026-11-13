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
            RenderedCall::propagating(RustExpr::atom(format!(
                "{dict_text}.get({key_text}){cloned}.ok_or_else(|| smelt_py_key_error({key_text}))", dict_text = dict_text.tight(), key_text = key_text.delimited()
            ))),
            *value_ty,
        ))
    }

    /// The mutable place a dict-mutating fallible op works on.
    fn dict_receiver_text(&self, dict: &Operand) -> Result<RustExpr, EmitError> {
        match dict {
            Operand::Copy(smelt_mir::Place::Local(local)) | Operand::Move(smelt_mir::Place::Local(local)) => {
                self.local_mut_value(*local)
            }
            _ => Err(EmitError::new("a dict pop needs a local receiver")),
        }
    }

    /// Renders Python's `d.pop(k)` with no default: the value, or a raised
    /// `KeyError` whose one argument is the key.
    fn python_dict_pop_call(
        &self,
        dict: &Operand,
        key: &Operand,
    ) -> Result<(RenderedCall, TypeId), EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        let Some(Type::Dict(key_ty, value_ty) | Type::JsMap(key_ty, value_ty)) = self.mir.types.get(dict_ty) else {
            return Err(EmitError::new("dict pop receiver must be a dict"));
        };
        let dict_text = self.dict_receiver_text(dict)?;
        let key_text = self.operand_expr(key)?;
        let remove = self.dict_remove_method(dict_ty, *key_ty);
        Ok((
            RenderedCall::propagating(RustExpr::atom(format!(
                "{{ let smelt_key = {key_text}; {dict_text}.{remove}(&smelt_key).ok_or_else(|| smelt_py_key_error(&smelt_key)) }}", key_text = key_text.delimited(), dict_text = dict_text.tight()
            ))),
            *value_ty,
        ))
    }

    /// Renders Python's `d.popitem()`: the last-inserted `(key, value)` pair,
    /// or a raised `KeyError` on an empty dict.
    fn python_dict_popitem_call(&self, dict: &Operand) -> Result<(RenderedCall, TypeId), EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        let Some(Type::Dict(key_ty, value_ty)) = self.mir.types.get(dict_ty) else {
            return Err(EmitError::new("dict popitem receiver must be a dict"));
        };
        let pop = if self.dict_uses_smelt_record(*key_ty) { "pop_last" } else { "pop" };
        let dict_text = self.dict_receiver_text(dict)?;
        Ok((
            RenderedCall::propagating(RustExpr::atom(format!(
                "{dict_text}.{pop}().ok_or_else(|| smelt_py_key_error(&\"popitem(): dictionary is empty\".to_owned()))", dict_text = dict_text.tight()
            ))),
            self.type_id(Type::Tuple(vec![*key_ty, *value_ty]))?,
        ))
    }

    /// Renders Python's `xs[i]` on a list or a string: a negative index counts
    /// from the end (a string's in code points), and an index past either end
    /// raises `IndexError`.
    fn python_sequence_read_call(
        &self,
        sequence: &Operand,
        index: &Operand,
    ) -> Result<(RenderedCall, TypeId), EmitError> {
        let sequence_ty = self.operand_ty(sequence)?;
        let index_text = self.operand_expr(index)?;
        let sequence_text = self.operand_borrow_expr(sequence)?;
        let position = "let smelt_normalized = if smelt_index < 0 { smelt_len + smelt_index } else { smelt_index }; usize::try_from(smelt_normalized).ok()";
        match self.mir.types.get(sequence_ty) {
            Some(Type::List(item_ty)) => Ok((
                RenderedCall::propagating(RustExpr::atom(format!(
                    "{{ let smelt_index: i64 = {index_text}; let smelt_list = &{sequence_text}; let smelt_items = smelt_list.borrow(); let smelt_len = smelt_items.len() as i64; {position}.and_then(|smelt_at| smelt_items.get(smelt_at).cloned()).ok_or_else(|| smelt_py_index_error(\"list index out of range\")) }}", index_text = index_text.delimited(), sequence_text = sequence_text.tight()
                ))),
                *item_ty,
            )),
            Some(Type::PyStr) => Ok((
                RenderedCall::propagating(RustExpr::atom(format!(
                    "{{ let smelt_index: i64 = {index_text}; let smelt_text = &{sequence_text}; let smelt_len = smelt_text.chars().count() as i64; {position}.and_then(|smelt_at| smelt_text.chars().nth(smelt_at)).map(String::from).ok_or_else(|| smelt_py_index_error(\"string index out of range\")) }}", index_text = index_text.delimited(), sequence_text = sequence_text.tight()
                ))),
                sequence_ty,
            )),
            _ => Err(EmitError::new("a Python sequence read needs a list or a string")),
        }
    }

    /// Renders Python's `min(xs)` / `max(xs)` over a list: a left fold that
    /// replaces the running extremum only when an item compares strictly
    /// less (greater), so the first of equal items wins and a NaN compares as
    /// `CPython` compares it; an empty list raises `ValueError` with
    /// `CPython`'s message.
    fn python_list_extrema_call(
        &self,
        op: smelt_hir::NumericExtremaOp,
        list: &Operand,
    ) -> Result<(RenderedCall, TypeId), EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("a Python min()/max() needs a list"));
        };
        let (comparison, name) = match op {
            smelt_hir::NumericExtremaOp::Min => ("<", "min"),
            smelt_hir::NumericExtremaOp::Max => (">", "max"),
        };
        let list_text = self.operand_borrow_expr(list)?;
        Ok((
            RenderedCall::propagating(RustExpr::atom(format!(
                "{{ let smelt_list = &{list_text}; let smelt_items = smelt_list.borrow(); smelt_items.iter().cloned().reduce(|smelt_best, smelt_item| if smelt_item {comparison} smelt_best {{ smelt_item }} else {{ smelt_best }}).ok_or_else(|| smelt_py_value_error(\"{name}() iterable argument is empty\")) }}",
                list_text = list_text.tight()
            ))),
            *item_ty,
        ))
    }

    /// Renders Python's `xs[start:end:step]` on a list or a string: the
    /// elements (code points) at the positions `smelt_py_slice_positions`
    /// picks.
    fn python_step_slice_call(
        &self,
        sequence: &Operand,
        start: Option<&Operand>,
        end: Option<&Operand>,
        step: &Operand,
    ) -> Result<(RenderedCall, TypeId), EmitError> {
        let sequence_ty = self.operand_ty(sequence)?;
        let bound = |operand: Option<&Operand>| -> Result<String, EmitError> {
            Ok(match operand {
                Some(operand) => format!("Some({} as i64)", self.operand_expr(operand)?.as_cast_operand()),
                None => "None".to_owned(),
            })
        };
        let (start_text, end_text) = (bound(start)?, bound(end)?);
        let step_text = self.operand_expr(step)?;
        let sequence_text = self.operand_borrow_expr(sequence)?;
        let text = match self.mir.types.get(sequence_ty) {
            Some(Type::List(_)) => format!(
                "{{ let smelt_list = &{sequence_text}; let smelt_items = smelt_list.borrow(); smelt_py_slice_positions(smelt_items.len(), {start_text}, {end_text}, {step_text} as i64).map(|smelt_at| SmeltList::from(smelt_at.into_iter().filter_map(|smelt_index| smelt_items.get(smelt_index).cloned()).collect::<Vec<_>>())) }}", sequence_text = sequence_text.tight(), step_text = step_text.as_cast_operand()
            ),
            Some(Type::PyStr) => format!(
                "{{ let smelt_chars = {sequence_text}.chars().collect::<Vec<char>>(); smelt_py_slice_positions(smelt_chars.len(), {start_text}, {end_text}, {step_text} as i64).map(|smelt_at| smelt_at.into_iter().filter_map(|smelt_index| smelt_chars.get(smelt_index)).collect::<String>()) }}", sequence_text = sequence_text.tight(), step_text = step_text.as_cast_operand()
            ),
            _ => return Err(EmitError::new("a Python step slice needs a list or a string")),
        };
        Ok((RenderedCall::propagating(RustExpr::atom(text)), sequence_ty))
    }

    /// Renders a Python division whose divisor is a nonzero constant.
    pub(super) fn python_division_expr(
        &self,
        op: PythonDivisionOp,
        lhs: &Operand,
        rhs: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let divide = self.python_division_fn(op, self.operand_ty(lhs)?)?;
        Ok(RustExpr::atom(divide).postfix(format!("({}, {})", self.operand_expr(lhs)?.delimited(), self.operand_expr(rhs)?.delimited())))
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
                    RenderedCall::propagating(RustExpr::atom(format!(
                        "smelt_py_checked_division({lhs}, {rhs}, {divide})", lhs = lhs.delimited(), rhs = rhs.delimited()
                    ))),
                    source_ty,
                )
            }
            Rvalue::PythonDictRead { dict, key } => self.python_dict_read_call(dict, key)?,
            Rvalue::BindingCheck { binding, unbound } => self.binding_check_call(binding, *unbound)?,
            Rvalue::PythonListRead { list, index } => {
                let list_ty = self.operand_ty(list)?;
                let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
                    return Err(EmitError::new("a Python list read needs a list"));
                };
                let list_text = self.operand_borrow_expr(list)?;
                let index_text = self.operand_expr(index)?;
                (
                    RenderedCall::propagating(RustExpr::atom(format!(
                        "smelt_py_list_read(&{list_text}, {index_text})", list_text = list_text.tight(), index_text = index_text.delimited()
                    ))),
                    *item_ty,
                )
            }
            Rvalue::DictPop { dict, key, default: None } => self.python_dict_pop_call(dict, key)?,
            Rvalue::DictPopItem { dict } => self.python_dict_popitem_call(dict)?,
            Rvalue::ListIndex { list, item } => (
                RenderedCall::propagating(self.list_index_expr(list, item)?),
                self.type_id(Type::Int)?,
            ),
            Rvalue::ListRemove { list, item } => (
                RenderedCall::propagating(self.list_remove_expr(list, item)?),
                self.type_id(Type::None)?,
            ),
            Rvalue::PythonSequenceRead { sequence, index } => self.python_sequence_read_call(sequence, index)?,
            Rvalue::PythonStepSlice { sequence, start, end, step } => {
                self.python_step_slice_call(sequence, start.as_ref(), end.as_ref(), step)?
            }
            Rvalue::PythonListExtrema { op: extremum, list } => self.python_list_extrema_call(*extremum, list)?,
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
                    RenderedCall::propagating(RustExpr::atom(adapter).postfix(format!("(&{value_text})", value_text = value_text.tight()))),
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
                    RenderedCall::propagating(RustExpr::atom(format!(
                        "{}({pattern}, {flags})",
                        crate::thrown::REGEXP_COMPILE_FN, pattern = pattern.delimited(), flags = flags.delimited()
                    ))),
                    source_ty,
                )
            }
            Rvalue::UriTranscode { op, .. } => {
                let value = args
                    .first()
                    .ok_or_else(|| EmitError::new("a URI transcoder takes one string argument"))?;
                let adapter = match op {
                    UriTranscodeOp::Decode => crate::thrown::DECODE_URI_FN,
                    UriTranscodeOp::DecodeComponent => crate::thrown::DECODE_URI_COMPONENT_FN,
                    UriTranscodeOp::Encode => crate::thrown::ENCODE_URI_FN,
                    UriTranscodeOp::EncodeComponent => crate::thrown::ENCODE_URI_COMPONENT_FN,
                };
                let value_text = self.string_like_operand_expr(value, "URI transcoder input")?;
                (
                    RenderedCall::propagating(RustExpr::atom(adapter).postfix(format!("(&{value_text})", value_text = value_text.tight()))),
                    self.type_id(Type::String)?,
                )
            }
            Rvalue::FastCheckRun { arbitraries, predicate, parameters } => (
                RenderedCall::propagating(self.fast_check_run_expr(
                    arbitraries,
                    predicate,
                    parameters.as_ref(),
                )?),
                self.type_id(Type::None)?,
            ),
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
                text.map_call(|call| format!("{call}.map(JsString::from)", call = call.tight()))
            } else {
                text.map_call(|call| format!("JsString::from({call})", call = call.delimited()))
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
                self.operand_expr(text)?.tight()
            );
            return Ok((RenderedCall::propagating(RustExpr::atom(call)), decoded));
        }
        let call = format!("{}(&{})", crate::thrown::JSON_PARSE_FN, self.operand_expr(text)?.tight());
        Ok((RenderedCall::propagating(RustExpr::atom(call)), self.type_id(Type::Unknown)?))
    }
}
