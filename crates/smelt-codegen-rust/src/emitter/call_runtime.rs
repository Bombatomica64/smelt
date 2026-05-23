//! Call Runtime emission helpers.

use super::*;
use smelt_hir::FunctionType;

impl FunctionEmitter<'_> {
    /// Render array-literal elements by value without consuming local operands.
    ///
    /// JavaScript array literals copy references/primitive values into the new
    /// array and do not invalidate the source expression. MIR may mark an input
    /// as `Move` when the temporary is otherwise dead, but generated Rust can
    /// still need the same closure parameter later in the literal expression, so
    /// list emission treats local moves as cloneable copies.
    fn list_literal_operand(&self, operand: &Operand) -> Operand {
        match operand {
            Operand::Move(place @ Place::Local(_)) => Operand::Copy(place.clone()),
            _ => operand.clone(),
        }
    }

    /// Render a dictionary literal as a generated record when the destination
    /// type is a known class/interface storage shape.
    ///
    /// TypeScript object literals are structural, but generated Rust records
    /// are nominal. This path uses the destination type already known at the
    /// assignment/call site to construct the nominal Rust record directly
    /// instead of first materializing an unrelated `HashMap`.
    fn record_literal_text_for_dest(
        &self,
        entries: &[(Operand, Operand)],
        dest_ty: TypeId,
    ) -> Result<Option<String>, EmitError> {
        let Some(Type::Class { name, args }) = self.mir.types.get(dest_ty) else {
            return Ok(None);
        };
        if !self.is_interface_record_type(dest_ty)
            && self.mir.classes.iter().all(|class| class.name != *name)
        {
            return Ok(None);
        }
        let Some(fields) = self.structural_record_fields(dest_ty) else {
            return Ok(None);
        };
        if fields.is_empty() {
            return Ok(None);
        }

        let mut literal_entries = HashMap::new();
        for (entry_key, value) in entries {
            let Operand::Const(Constant::String(key_text)) = entry_key else {
                return Ok(None);
            };
            literal_entries.insert(sanitize_ident(key_text), value);
        }

        let mut field_text = Vec::new();
        for field in fields {
            let field_name = sanitize_ident(self.symbol_name(field.name)?);
            let value = if let Some(entry_value) = literal_entries.get(&field_name) {
                self.operand_as_type_text(entry_value, field.ty)?
            } else if matches!(self.mir.types.get(field.ty), Some(Type::Optional(_))) {
                self.default_value(field.ty)?
            } else {
                return Ok(None);
            };
            field_text.push(format!("{field_name}: {value}"));
        }
        if !args.is_empty() {
            field_text.push("_smelt_phantom: ::std::marker::PhantomData".to_owned());
        }

        let type_name = sanitize_ident(self.symbol_name(*name)?);
        Ok(Some(format!("{type_name} {{ {} }}", field_text.join(", "))))
    }

    /// Converts an rvalue to its Rust text representation.
    pub(super) fn rvalue_text(&self, value: &Rvalue) -> Result<String, EmitError> {
        self.rvalue_text_for_dest(value, self.none_ty)
    }

    /// Converts an rvalue to Rust text using the destination type when it affects emission.
    /// Converts an rvalue to Rust text using the destination type when it affects emission.
    pub(super) fn rvalue_text_for_dest(
        &self,
        value: &Rvalue,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        match value {
            Rvalue::Use(operand) => self.operand_as_type_text(operand, dest_ty),
            Rvalue::List(items) => {
                if let Some(Type::Optional(inner)) = self.mir.types.get(dest_ty) {
                    if matches!(
                        self.mir.types.get(*inner),
                        Some(
                            Type::List(_) | Type::Unknown | Type::TypeParam { .. } | Type::Union(_)
                        )
                    ) || self.is_erased_class_type(*inner)
                    {
                        let inner_text = self.rvalue_text_for_dest(value, *inner)?;
                        return Ok(format!("Some({inner_text})"));
                    }
                }
                if matches!(
                    self.mir.types.get(dest_ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(dest_ty)
                {
                    let items_text = items
                        .iter()
                        .map(|item| {
                            self.operand_as_type_text(
                                &self.list_literal_operand(item),
                                self.type_id(Type::Unknown)?,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .join(", ");
                    return Ok(format!("SmeltUnknown::Array(vec![{items_text}])"));
                }
                if let Some(Type::List(item_ty)) = self.mir.types.get(dest_ty) {
                    let items_text = items
                        .iter()
                        .map(|item| {
                            self.operand_as_type_text(&self.list_literal_operand(item), *item_ty)
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .join(", ");
                    return Ok(format!("vec![{items_text}]"));
                }
                let items_text = items
                    .iter()
                    .map(|item| self.operand_text(&self.list_literal_operand(item)))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                Ok(format!("vec![{items_text}]"))
            }
            Rvalue::Set(items) => {
                let set_uses_vec = matches!(self.mir.types.get(dest_ty), Some(Type::Set(item)) if !self.type_is_hash_set_key_safe(*item));
                if items.is_empty() {
                    return Ok(if set_uses_vec {
                        "Vec::new()".to_owned()
                    } else {
                        "::std::collections::HashSet::new()".to_owned()
                    });
                }
                let item_ty = match self.mir.types.get(dest_ty) {
                    Some(Type::Set(item_ty)) => Some(*item_ty),
                    _ => None,
                };
                let items_text = items
                    .iter()
                    .map(|item| {
                        if let Some(set_item_ty) = item_ty {
                            self.operand_as_type_text(item, set_item_ty)
                        } else {
                            self.operand_text(item)
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                if set_uses_vec {
                    return Ok(format!("vec![{items_text}]"));
                }
                Ok(format!("::std::collections::HashSet::from([{items_text}])"))
            }
            Rvalue::Dict(entries) => {
                if let Some(record_text) = self.record_literal_text_for_dest(entries, dest_ty)? {
                    return Ok(record_text);
                }
                if matches!(
                    self.mir.types.get(dest_ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(dest_ty)
                {
                    let entries_text = entries
                        .iter()
                        .map(|(key, entry_value)| {
                            Ok(format!(
                                "({}, {})",
                                self.operand_as_type_text(key, self.type_id(Type::String)?)?,
                                self.operand_as_type_text(
                                    entry_value,
                                    self.type_id(Type::Unknown)?
                                )?
                            ))
                        })
                        .collect::<Result<Vec<_>, EmitError>>()?
                        .join(", ");
                    return Ok(format!(
                        "SmeltUnknown::Object(::std::collections::HashMap::from([{entries_text}]))"
                    ));
                }
                let dict_types = match self.mir.types.get(dest_ty) {
                    Some(Type::Dict(key_ty, value_ty)) => Some((*key_ty, *value_ty)),
                    _ => None,
                };
                let entries_text = entries
                    .iter()
                    .map(|(key, entry_value)| {
                        let key_text = if let Some((key_ty, _)) = dict_types {
                            self.operand_as_type_text(key, key_ty)?
                        } else {
                            self.operand_text(key)?
                        };
                        let value_text = if let Some((_, value_ty)) = dict_types {
                            if matches!(self.mir.types.get(value_ty), Some(Type::Function(_))) {
                                format!(
                                    "{{ let smelt_fn: {} = {}; smelt_fn }}",
                                    self.type_text_with_impl_trait(value_ty, false)?,
                                    self.operand_as_type_text(entry_value, value_ty)?
                                )
                            } else {
                                self.operand_as_type_text(entry_value, value_ty)?
                            }
                        } else {
                            self.operand_text(entry_value)?
                        };
                        Ok(format!("({key_text}, {value_text})"))
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?
                    .join(", ");
                Ok(format!(
                    "::std::collections::HashMap::from([{entries_text}])"
                ))
            }
            Rvalue::Tuple(items) => {
                if let Some(Type::Tuple(target_items)) = self.mir.types.get(dest_ty) {
                    let offset = items.len().saturating_sub(target_items.len());
                    let items_text = target_items
                        .iter()
                        .enumerate()
                        .map(|(index, target_item)| {
                            let item_index = index
                                .checked_add(offset)
                                .ok_or_else(|| EmitError::new("tuple item index overflowed"))?;
                            let item = items.get(item_index).ok_or_else(|| {
                                EmitError::new("tuple destination has more items than literal")
                            })?;
                            self.operand_as_type_text(item, *target_item)
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .join(", ");
                    return if target_items.len() == 1 {
                        Ok(format!("({items_text},)"))
                    } else {
                        Ok(format!("({items_text})"))
                    };
                }
                let items_text = items
                    .iter()
                    .map(|item| self.operand_text(item))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                if items.is_empty() {
                    Ok("()".to_owned())
                } else if items.len() == 1 {
                    Ok(format!("({items_text},)"))
                } else {
                    Ok(format!("({items_text})"))
                }
            }
            Rvalue::Binary { op, lhs, rhs } => {
                if let Some(text) = self.optional_binary_text(*op, lhs, rhs, dest_ty)? {
                    return Ok(text);
                }
                if let Some(text) = self.unknown_binary_text(*op, lhs, rhs)? {
                    return Ok(text);
                }
                if *op == smelt_hir::BinOp::Add
                    && matches!(self.mir.types.get(dest_ty), Some(Type::String))
                {
                    let lhs_text = self.string_like_operand_text(lhs, "string addition")?;
                    let rhs_text = self.string_like_operand_text(rhs, "string addition")?;
                    return Ok(format!("{lhs_text} + &{rhs_text}"));
                }
                if let Some(text) = self.erased_arithmetic_text(*op, lhs, rhs, dest_ty)? {
                    return Ok(text);
                }
                if let Some(text) = self.function_equality_text(*op, lhs, rhs)? {
                    return Ok(text);
                }
                if let Some(text) = self.heterogeneous_equality_text(*op, lhs, rhs)? {
                    return Ok(text);
                }
                if let Some(text) = self.numeric_comparison_text(*op, lhs, rhs, dest_ty)? {
                    return Ok(text);
                }
                if matches!(
                    *op,
                    smelt_hir::BinOp::Add
                        | smelt_hir::BinOp::Sub
                        | smelt_hir::BinOp::Mul
                        | smelt_hir::BinOp::Div
                        | smelt_hir::BinOp::Rem
                ) && matches!(self.mir.types.get(dest_ty), Some(Type::Int | Type::Float))
                {
                    return Ok(format!(
                        "{} {} {}",
                        self.operand_as_type_text(lhs, dest_ty)?,
                        smelt_hir::bin_op_text(*op),
                        self.operand_as_type_text(rhs, dest_ty)?
                    ));
                }
                if matches!(*op, smelt_hir::BinOp::And | smelt_hir::BinOp::Or)
                    && matches!(self.mir.types.get(dest_ty), Some(Type::Bool))
                {
                    return Ok(format!(
                        "{} {} {}",
                        self.truthy_operand_text(lhs)?,
                        smelt_hir::bin_op_text(*op),
                        self.truthy_operand_text(rhs)?
                    ));
                }
                if *op == smelt_hir::BinOp::UShr {
                    let lhs_text = self.operand_text(lhs)?;
                    let rhs_text = self.operand_text(rhs)?;
                    let lhs_trunc_text = self.numeric_trunc_f64_text(&lhs_text);
                    let rhs_trunc_text = self.numeric_trunc_f64_text(&rhs_text);
                    return Ok(format!(
                        "{{ let smelt_shift_value = {lhs_trunc_text}; let smelt_shift_value = if smelt_shift_value.is_finite() {{ smelt_shift_value.rem_euclid(4294967296.0) as u32 }} else {{ 0_u32 }}; let smelt_shift_count = {rhs_trunc_text}; let smelt_shift_count = if smelt_shift_count.is_finite() {{ smelt_shift_count.rem_euclid(4294967296.0) as u32 }} else {{ 0_u32 }}; (smelt_shift_value >> (smelt_shift_count & 31)) as f64 }}"
                    ));
                }
                if matches!(*op, smelt_hir::BinOp::Shl | smelt_hir::BinOp::Shr) {
                    let lhs_text = self.operand_text(lhs)?;
                    let rhs_text = self.operand_text(rhs)?;
                    let lhs_trunc_text = self.numeric_trunc_f64_text(&lhs_text);
                    let rhs_trunc_text = self.numeric_trunc_f64_text(&rhs_text);
                    let op_text = smelt_hir::bin_op_text(*op);
                    let result_cast = if matches!(self.mir.types.get(dest_ty), Some(Type::Int)) {
                        "i64"
                    } else {
                        "f64"
                    };
                    return Ok(format!(
                        "(({lhs_trunc_text} as i128) {op_text} (({rhs_trunc_text} as u32).min(127))) as {result_cast}"
                    ));
                }
                if *op == smelt_hir::BinOp::Add
                    && matches!(
                        self.mir.types.get(self.operand_ty(lhs)?),
                        Some(Type::String)
                    )
                {
                    let rhs_text = self.operand_text(rhs)?;
                    let rhs_expr = match self.mir.types.get(self.operand_ty(rhs)?) {
                        Some(Type::String) => format!("&{rhs_text}"),
                        Some(Type::Optional(inner))
                            if matches!(
                                self.mir.types.get(*inner),
                                Some(Type::Bool | Type::Int | Type::Float | Type::String)
                            ) =>
                        {
                            format!("&{rhs_text}.unwrap_or_default().to_string()")
                        }
                        _ => format!("&{rhs_text}.to_string()"),
                    };
                    return Ok(format!("{} + {rhs_expr}", self.operand_text(lhs)?));
                }
                if matches!(*op, smelt_hir::BinOp::Eq | smelt_hir::BinOp::NotEq) {
                    let lhs_ty = self.operand_ty(lhs)?;
                    let rhs_ty = self.operand_ty(rhs)?;
                    if lhs_ty != rhs_ty
                        || self.type_contains_unknown(lhs_ty)
                        || self.type_contains_unknown(rhs_ty)
                    {
                        let comparison = format!(
                            "{} == {}",
                            self.unknown_wrap_text(lhs)?,
                            self.unknown_wrap_text(rhs)?
                        );
                        return Ok(if *op == smelt_hir::BinOp::NotEq {
                            format!("!({comparison})")
                        } else {
                            comparison
                        });
                    }
                }
                Ok(format!(
                    "{} {} {}",
                    self.operand_text(lhs)?,
                    smelt_hir::bin_op_text(*op),
                    self.operand_text(rhs)?
                ))
            }
            Rvalue::Unary { op, operand } => match op {
                smelt_hir::UnaryOp::Not => Ok(format!("!({})", self.truthy_operand_text(operand)?)),
                smelt_hir::UnaryOp::Neg => Ok(format!("-{}", self.operand_text(operand)?)),
            },
            Rvalue::Conditional {
                cond,
                then_operand,
                else_operand,
            } => Ok(format!(
                "if {} {{ {} }} else {{ {} }}",
                self.operand_text(cond)?,
                self.operand_as_type_text(then_operand, dest_ty)?,
                self.operand_as_type_text(else_operand, dest_ty)?
            )),
            Rvalue::OptionalField { receiver, field } => {
                self.optional_field_text_for_dest(receiver, *field, dest_ty)
            }
            Rvalue::OptionalIndex { receiver, index } => {
                self.optional_index_text(receiver, index, dest_ty)
            }
            Rvalue::OptionalMethod {
                receiver,
                method,
                args,
            } => self.optional_method_text(receiver, *method, args, dest_ty),
            Rvalue::OptionalCoalesce { optional, fallback } => {
                self.optional_coalesce_text(optional, fallback, dest_ty)
            }
            Rvalue::InstanceOf {
                value: operand,
                class,
            } => self.instance_of_text(operand, *class),
            Rvalue::UnknownIs {
                value: unknown_value,
                kind,
            } => self.unknown_is_text(unknown_value, *kind),
            Rvalue::UnknownCast {
                value: unknown_value,
                target,
            } => {
                let cast_text = if self.mir.types.get(*target) == Some(&Type::Unknown) {
                    self.unknown_wrap_text(unknown_value)?
                } else {
                    self.unknown_cast_text(unknown_value, *target)?
                };
                self.rendered_value_as_type_text(&cast_text, *target, dest_ty)
            }
            Rvalue::Struct { class, fields } => {
                let class_name = sanitize_ident(self.symbol_name(*class)?);
                let mir_class = self
                    .mir
                    .classes
                    .iter()
                    .find(|item| item.name == *class)
                    .ok_or_else(|| EmitError::new("struct rvalue references an unknown class"))?;
                let mut parts = Vec::new();
                for field in crate::classes::effective_class_fields(self.mir, mir_class) {
                    let name = sanitize_ident(self.symbol_name(field.name)?);
                    if let Some((_, field_value)) = fields
                        .iter()
                        .find(|(field_name, _)| *field_name == field.name)
                    {
                        parts.push(format!("{name}: {}", self.operand_text(field_value)?));
                    } else {
                        parts.push(format!("{name}: {}", self.default_value(field.ty)?));
                    }
                }
                if !mir_class.type_params.is_empty() {
                    parts.push("_smelt_phantom: ::std::marker::PhantomData".to_owned());
                }
                Ok(format!("{class_name} {{ {} }}", parts.join(", ")))
            }
            Rvalue::ExternalClassInstance { class, args } => {
                let text = self.external_class_instance_text(*class, args)?;
                if self.symbol_name(*class)? == "RegExp"
                    && matches!(self.mir.types.get(dest_ty), Some(Type::String))
                {
                    Ok(format!("{text}.source.clone()"))
                } else {
                    Ok(text)
                }
            }
            Rvalue::Len(operand) => self.len_text(operand, dest_ty),
            Rvalue::NumericAbs(operand) => self.numeric_abs_text(operand),
            Rvalue::NumericRound { op, operand } => self.numeric_round_text(*op, operand, dest_ty),
            Rvalue::NumericExtrema { op, args } => self.numeric_extrema_text(*op, args, dest_ty),
            Rvalue::NumericHypot { args } => self.numeric_hypot_text(args),
            Rvalue::NumericPredicate { op, operand } => self.numeric_predicate_text(*op, operand),
            Rvalue::NumericUnaryFunc { op, operand } => self.numeric_unary_func_text(*op, operand),
            Rvalue::NumericPow { base, exponent } => self.numeric_pow_text(base, exponent),
            Rvalue::NumericAtan2 { y, x } => self.numeric_atan2_text(y, x),
            Rvalue::NumericRandom => Ok("rand::random::<f64>()".to_owned()),
            Rvalue::NumericRandomInt { start, end } => self.numeric_random_int_text(start, end),
            Rvalue::NumericToStringRadix { operand, radix } => {
                self.numeric_to_string_radix_text(operand, radix)
            }
            Rvalue::PrimitiveCast { op, operand } => {
                self.primitive_cast_text(*op, operand, dest_ty)
            }
            Rvalue::StringCase { op, operand } => self.string_case_text(*op, operand),
            Rvalue::StringNormalize { form, operand } => self.string_normalize_text(*form, operand),
            Rvalue::StringTrim { side, operand } => self.string_trim_text(*side, operand),
            Rvalue::StringAffix {
                op,
                haystack,
                needle,
            } => self.string_affix_text(*op, haystack, needle),
            Rvalue::StringSearch {
                op,
                haystack,
                needle,
            } => self.string_search_text(*op, haystack, needle, dest_ty),
            Rvalue::StringReplace {
                op,
                haystack,
                pattern,
                replacement,
            } => self.string_replace_text(*op, haystack, pattern, replacement),
            Rvalue::StringRemoveAffix {
                op,
                haystack,
                affix,
            } => self.string_remove_affix_text(*op, haystack, affix),
            Rvalue::StringRepeat { operand, count } => self.string_repeat_text(operand, count),
            Rvalue::StringPad {
                op,
                operand,
                target_len,
                pad,
            } => self.string_pad_text(*op, operand, target_len, pad),
            Rvalue::StringPredicate { op, operand } => self.string_predicate_text(*op, operand),
            Rvalue::RegexIsMatch {
                op,
                pattern,
                haystack,
            } => self.regex_is_match_text(*op, pattern, haystack),
            Rvalue::RegexReplace {
                op,
                pattern,
                haystack,
                replacement,
            } => self.regex_replace_text(*op, pattern, haystack, replacement),
            Rvalue::RegexReplaceCallback {
                op,
                pattern,
                haystack,
                callback,
            } => self.regex_replace_callback_text(*op, pattern, haystack, callback),
            Rvalue::RegexReplaceFirstMatchUppercase { pattern, haystack } => {
                self.regex_replace_first_match_uppercase_text(pattern, haystack)
            }
            Rvalue::RegexSplit { pattern, haystack } => self.regex_split_text(pattern, haystack),
            Rvalue::RegexFind { pattern, haystack } => self.regex_find_text(pattern, haystack),
            Rvalue::RegexExec { regex, haystack } => self.regex_exec_text(regex, haystack),
            Rvalue::StringCharAt { operand, index } => self.string_char_at_text(operand, index),
            Rvalue::StringCharCodeAt { operand, index } => {
                self.string_char_code_at_text(operand, index)
            }
            Rvalue::StringContains { haystack, needle } => {
                self.string_contains_text(haystack, needle)
            }
            Rvalue::StringSlice {
                operand,
                start,
                end,
            } => self.string_slice_text(operand, start.as_ref(), end.as_ref()),
            Rvalue::ListContains { list, item } => self.list_contains_text(list, item),
            Rvalue::SetContains { set, item } => self.set_contains_text(set, item),
            Rvalue::SetDisjoint { left, right } => self.set_disjoint_text(left, right),
            Rvalue::SetRelation { op, left, right } => self.set_relation_text(*op, left, right),
            Rvalue::SetAdd { set, item } => self.set_add_text(set, item, dest_ty),
            Rvalue::SetRemove { op, set, item } => self.set_remove_text(*op, set, item, dest_ty),
            Rvalue::SetClear { set } => self.collection_clear_text(set, dest_ty, "set"),
            Rvalue::SetCopy { set } => self.set_copy_text(set, dest_ty),
            Rvalue::ListToSet { list } => self.list_to_set_text(list, dest_ty),
            Rvalue::ListPairsToDict { list } => self.list_pairs_to_dict_text(list, dest_ty),
            Rvalue::SetBinary { op, left, right } => {
                self.set_binary_text(*op, left, right, dest_ty)
            }
            Rvalue::SetProjection { op, set } => self.set_projection_text(*op, set, dest_ty),
            Rvalue::ListConcat { left, right } => self.list_concat_text(left, right),
            Rvalue::ListSearch { op, list, item } => self.list_search_text(*op, list, item),
            Rvalue::Closure { id, .. } => {
                if !matches!(
                    self.mir.types.get(dest_ty),
                    Some(
                        Type::Function(_) | Type::Unknown | Type::TypeParam { .. } | Type::Union(_)
                    )
                ) && !self.is_erased_class_type(dest_ty)
                {
                    return self.default_value(dest_ty);
                }
                if let Some(closure) = self
                    .mir
                    .closures
                    .get(id_index(id.0, "closure id does not fit usize")?)
                    && closure.captures.iter().any(|capture| {
                        self.capture_is_borrowed_callback_param(capture.source_local)
                            .unwrap_or(false)
                            || self
                                .capture_symbol_is_borrowed_callback_param(
                                    capture.symbol,
                                    capture.ty,
                                )
                                .unwrap_or(false)
                    })
                {
                    return self.default_value(dest_ty);
                }
                self.closure_text_for_type(*id, dest_ty)
            }
            Rvalue::ClosureCall { callee, args } => {
                let callee_ty = self.operand_ty(callee)?;
                if matches!(
                    self.mir.types.get(callee_ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(callee_ty)
                {
                    let callee_text = self.operand_text(callee)?;
                    let rendered_args = args
                        .iter()
                        .map(|arg| self.unknown_wrap_text(arg))
                        .collect::<Result<Vec<_>, EmitError>>()?;
                    let smelt_call_args = if rendered_args.len() == 1
                        && args.first().is_some_and(|arg| {
                            matches!(
                                self.operand_ty(arg)
                                    .ok()
                                    .and_then(|ty| self.mir.types.get(ty)),
                                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                                    | Some(Type::List(_))
                            )
                        }) {
                        let arg = rendered_args
                            .first()
                            .ok_or_else(|| EmitError::new("erased call is missing argument"))?;
                        format!(
                            "match {arg} {{ SmeltUnknown::Array(values) => values, value => vec![value] }}"
                        )
                    } else {
                        format!("vec![{}]", rendered_args.join(", "))
                    };
                    let call_text = format!(
                        "{{ let smelt_function_value = {callee_text}.clone(); let smelt_call_args = {smelt_call_args}; let smelt_callable = match smelt_function_value {{ SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(mut smelt_object) => match smelt_object.remove(\"__smelt_call\") {{ Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function), _ => None }}, _ => None }}; if let Some(smelt_function) = smelt_callable {{ (&mut *smelt_function.borrow_mut())(smelt_call_args).unwrap_or_else(|error| panic!(\"{{}}\", error)) }} else {{ SmeltUnknown::Null }} }}"
                    );
                    let unknown_ty = self.type_id(Type::Unknown)?;
                    if matches!(self.mir.types.get(dest_ty), Some(Type::Function(_))) {
                        return Ok(call_text);
                    }
                    return self.rendered_value_as_type_text(&call_text, unknown_ty, dest_ty);
                }
                if !matches!(self.mir.types.get(callee_ty), Some(Type::Function(_))) {
                    return self.default_value(dest_ty);
                }
                let callee_text = self.operand_text(callee)?;
                if callee_text == "()" {
                    return self.default_value(dest_ty);
                }
                if callee_text.starts_with("purry_")
                    && args.len() >= 2
                    && let Some(first_arg) = args.first()
                {
                    let data_arg = args
                        .get(1)
                        .ok_or_else(|| EmitError::new("purry call is missing arguments array"))?;
                    let mut rendered_args = Vec::new();
                    let first_arg_text =
                        if let Some(adapter) = self.rest_vector_unknown_adapter_text(first_arg)? {
                            adapter
                        } else {
                            self.operand_text(first_arg)?
                        };
                    rendered_args.push(first_arg_text);
                    rendered_args.push(self.operand_text(data_arg)?);
                    if let Some(lazy_arg) = args.get(2) {
                        rendered_args.push(self.operand_text(lazy_arg)?);
                    } else {
                        rendered_args.push("None".to_owned());
                    }
                    return Ok(format!("{callee_text}({})", rendered_args.join(", ")));
                }
                let emitted_params = self.emitted_function_param_types(&callee_text)?;
                let local_function = match callee {
                    Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => {
                        match self.mir.types.get(self.local_decl(*local)?.ty) {
                            Some(Type::Function(function)) => Some(function),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                let local_params = local_function.map(|function| function.params.as_slice());
                let inferred_function = match self.mir.types.get(callee_ty) {
                    Some(Type::Function(function)) => Some(function),
                    _ => None,
                };
                let inferred_params = match self.mir.types.get(callee_ty) {
                    Some(Type::Function(function)) => function.params.as_slice(),
                    _ => &[],
                };
                let params = local_params
                    .or(emitted_params.as_deref())
                    .unwrap_or(inferred_params);
                let rest_function = local_function.or(inferred_function);
                let mut rendered_args = if let Some(rest_args) =
                    self.rest_vector_call_args_text(args, rest_function)?
                {
                    rest_args
                } else {
                    let mut rendered_args = args
                        .iter()
                        .zip(params.iter())
                        .map(|(arg, param)| {
                            let text = self.operand_as_type_text(arg, *param)?;
                            if self.type_text(*param)? == "Vec<SmeltUnknown>"
                                && text
                                    .contains(".into_iter().map(|value| value).collect::<Vec<_>>()")
                            {
                                Ok(text.replace(
                                    ".into_iter().map(|value| value).collect::<Vec<_>>()",
                                    ".into_iter().map(|value| value.into_smelt_unknown()).collect::<Vec<_>>()",
                                ))
                            } else {
                                Ok(text)
                            }
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    for param in params.iter().skip(args.len()) {
                        rendered_args.push(self.default_value(*param)?);
                    }
                    rendered_args
                };
                if args.is_empty()
                    && rendered_args.as_slice() == ["Vec::new()"]
                    && matches!(
                        self.mir.types.get(callee_ty),
                        Some(Type::Function(function)) if function.params.is_empty()
                    )
                {
                    rendered_args.clear();
                }
                let args_text = rendered_args.join(", ");
                let call_text = match callee {
                    Operand::Copy(place) | Operand::Move(place)
                        if self.is_function_parameter_place(place)? =>
                    {
                        format!("{callee_text}({args_text})")
                    }
                    _ if self.is_function_parameter_name(&callee_text)? => {
                        format!("{callee_text}({args_text})")
                    }
                    _ if self.is_borrowed_callback_capture_name(&callee_text) => {
                        format!("{callee_text}({args_text})")
                    }
                    _ => format!("(&mut *{callee_text}.borrow_mut())({args_text})"),
                };
                let (source_ty, rendered_call_text) = match self.mir.types.get(callee_ty) {
                    Some(Type::Function(function)) => {
                        let returns_future = matches!(
                            self.mir.types.get(function.return_ty),
                            Some(Type::Future(_))
                        );
                        let throwing_call_text = if function.may_throw && !returns_future {
                            format!("{call_text}?")
                        } else {
                            call_text
                        };
                        (function.return_ty, throwing_call_text)
                    }
                    _ => (dest_ty, call_text),
                };
                if matches!(self.mir.types.get(source_ty), Some(Type::Unknown))
                    && matches!(self.mir.types.get(dest_ty), Some(Type::Function(_)))
                {
                    return Ok(rendered_call_text);
                }
                self.rendered_value_as_type_text(&rendered_call_text, source_ty, dest_ty)
            }
            Rvalue::ClosureCallSpread { callee, args } => {
                let callee_text = self.operand_text(callee)?;
                let unknown_ty = self.type_id(Type::Unknown)?;
                let args_ty = self.type_id(Type::List(unknown_ty))?;
                let args_text = self.operand_as_type_text(args, args_ty)?;
                if let Some(Type::Function(function)) = self.mir.types.get(self.operand_ty(callee)?)
                {
                    let call_text = match callee {
                        Operand::Copy(place) | Operand::Move(place)
                            if self.is_function_parameter_place(place)? =>
                        {
                            format!("{callee_text}({args_text})")
                        }
                        _ if self.is_function_parameter_name(&callee_text)? => {
                            format!("{callee_text}({args_text})")
                        }
                        _ if self.is_borrowed_callback_capture_name(&callee_text) => {
                            format!("{callee_text}({args_text})")
                        }
                        _ => format!("(&mut *{callee_text}.borrow_mut())({args_text})"),
                    };
                    return self.rendered_value_as_type_text(
                        &call_text,
                        function.return_ty,
                        dest_ty,
                    );
                }
                let call_text = format!(
                    "{{ let smelt_function_value = {callee_text}.clone(); let smelt_call_args = {args_text}; let smelt_callable = match smelt_function_value {{ SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(mut smelt_object) => match smelt_object.remove(\"__smelt_call\") {{ Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function), _ => None }}, _ => None }}; if let Some(smelt_function) = smelt_callable {{ (&mut *smelt_function.borrow_mut())(smelt_call_args).unwrap_or_else(|error| panic!(\"{{}}\", error)) }} else {{ SmeltUnknown::Null }} }}"
                );
                if matches!(self.mir.types.get(dest_ty), Some(Type::Function(_))) {
                    return Ok(call_text);
                }
                self.rendered_value_as_type_text(&call_text, unknown_ty, dest_ty)
            }
            Rvalue::ListCallback { op, list, callback } => {
                self.list_callback_text(*op, list, callback, dest_ty)
            }
            Rvalue::ListFromLength { length } => self.list_from_length_text(length, dest_ty),
            Rvalue::ListFromLengthMap { length, callback } => {
                self.list_from_length_map_text(length, callback, dest_ty)
            }
            Rvalue::ListReduce {
                list,
                initial,
                callback,
            } => self.list_reduce_text(list, initial.as_ref(), callback, dest_ty),
            Rvalue::ListSlice { list, start, end } => {
                self.list_slice_text(list, start.as_ref(), end.as_ref(), dest_ty)
            }
            Rvalue::ListSplice {
                list,
                start,
                delete_count,
                items,
                mutate,
            } => self.list_splice_text(list, start, delete_count.as_ref(), items, *mutate, dest_ty),
            Rvalue::ListFill {
                list,
                value: fill_value,
                start,
                end,
            } => self.list_fill_text(list, fill_value, start.as_ref(), end.as_ref(), dest_ty),
            Rvalue::ListCopyWithin {
                list,
                target,
                start,
                end,
            } => self.list_copy_within_text(list, target, start, end.as_ref(), dest_ty),
            Rvalue::ListWith {
                list,
                index,
                value: replacement,
            } => self.list_with_text(list, index, replacement, dest_ty),
            Rvalue::ListFlat { list } => self.list_flat_text(list, dest_ty),
            Rvalue::ListProjection { op, list } => self.list_projection_text(*op, list, dest_ty),
            Rvalue::ListPush { list, item } => self.list_push_text(list, item, dest_ty),
            Rvalue::ListExtend { list, other } => self.list_extend_text(list, other, dest_ty),
            Rvalue::ListInsert { list, index, item } => {
                self.list_insert_text(list, index, item, dest_ty)
            }
            Rvalue::ListUnshift { list, items } => self.list_unshift_text(list, items, dest_ty),
            Rvalue::ListReverse { list } => self.list_reverse_text(list, dest_ty),
            Rvalue::ListClear { list } => self.collection_clear_text(list, dest_ty, "list"),
            Rvalue::ListCopy { list } => self.list_copy_text(list, dest_ty),
            Rvalue::TupleToList { tuple } => self.tuple_to_list_text(tuple, dest_ty),
            Rvalue::ListToTuple { list } => self.list_to_tuple_text(list, dest_ty),
            Rvalue::TupleToSet { tuple } => self.tuple_to_set_text(tuple, dest_ty),
            Rvalue::ListCount { list, item } => self.list_count_text(list, item, dest_ty),
            Rvalue::ListSum { list } => self.list_sum_text(list, dest_ty),
            Rvalue::ListBoolFold { op, list } => self.list_bool_fold_text(*op, list),
            Rvalue::ListSorted { list } => self.list_sorted_text(list, dest_ty),
            Rvalue::ListReversed { list } => self.list_reversed_text(list, dest_ty),
            Rvalue::ListEnumerate { list } => self.list_enumerate_text(list, dest_ty),
            Rvalue::ListZip { left, right } => self.list_zip_text(left, right, dest_ty),
            Rvalue::ListRange { start, end, step } => {
                self.list_range_text(start, end, step, dest_ty)
            }
            Rvalue::ListRandomChoice { list } => self.list_random_choice_text(list, dest_ty),
            Rvalue::ListIndex { list, item } => self.list_index_text(list, item, dest_ty),
            Rvalue::ListRemove { list, item } => self.list_remove_text(list, item, dest_ty),
            Rvalue::ListSort { list, comparator } => {
                self.list_sort_text(list, comparator.as_ref(), dest_ty)
            }
            Rvalue::ListPop { list } => self.list_pop_text(list, dest_ty),
            Rvalue::ListShift { list } => self.list_shift_text(list, dest_ty),
            Rvalue::TupleContains { tuple, item } => self.tuple_contains_text(tuple, item),
            Rvalue::TupleIndex { tuple, index } => self.tuple_index_text(tuple, *index, dest_ty),
            Rvalue::TupleSlice { tuple, start, end } => {
                self.tuple_slice_text(tuple, *start, *end, dest_ty)
            }
            Rvalue::DictContainsKey { dict, key } => self.dict_contains_key_text(dict, key),
            Rvalue::DictSet {
                dict,
                key,
                value: dict_value,
            } => self.dict_set_text(dict, key, dict_value, dest_ty),
            Rvalue::DictRemoveKey { dict, key } => self.dict_remove_key_text(dict, key, dest_ty),
            Rvalue::DictGet { dict, key, default } => {
                self.dict_get_text(dict, key, default.as_ref(), dest_ty)
            }
            Rvalue::DictSetDefault { dict, key, default } => {
                self.dict_setdefault_text(dict, key, default, dest_ty)
            }
            Rvalue::DictClear { dict } => self.collection_clear_text(dict, dest_ty, "dict"),
            Rvalue::DictPop { dict, key, default } => {
                self.dict_pop_text(dict, key, default.as_ref(), dest_ty)
            }
            Rvalue::DictUpdate { dict, other } => self.dict_update_text(dict, other, dest_ty),
            Rvalue::DictAssign { target, sources } => {
                self.dict_assign_text(target, sources, dest_ty)
            }
            Rvalue::CallableObjectAssign { callable, props } => {
                self.callable_object_assign_text(callable, props, dest_ty)
            }
            Rvalue::DictCopy { dict } => self.dict_copy_text(dict, dest_ty),
            Rvalue::DictProjection { op, dict } => self.dict_projection_text(*op, dict),
            Rvalue::StringSplit {
                haystack,
                separator,
                limit,
            } => self.string_split_text(haystack, separator, limit.as_ref()),
            Rvalue::StringChars { haystack } => self.string_chars_text(haystack, dest_ty),
            Rvalue::StringJoin { items, separator } => self.string_join_text(items, separator),
            Rvalue::JsonStringify { value: json_value } => {
                self.json_stringify_text(json_value, dest_ty)
            }
            Rvalue::JsonParse { text } => self.json_parse_text(text, dest_ty),
            Rvalue::HttpGetText { url } => self.http_get_text(url),
            Rvalue::DateNow => {
                let text = "chrono::Utc::now().timestamp_millis()";
                self.date_timestamp_result_text(text, dest_ty)
            }
            Rvalue::DateToIsoString { timestamp_ms } => self.date_to_iso_string_text(timestamp_ms),
            Rvalue::DateFromParts { parts } => {
                let text = self.date_from_parts_text(parts)?;
                self.date_timestamp_result_text(&text, dest_ty)
            }
            Rvalue::DateFromValue { value: date_value } => {
                let text = self.date_timestamp_text(date_value)?;
                self.date_timestamp_result_text(&text, dest_ty)
            }
            Rvalue::DateGetPart { part, timestamp_ms } => {
                self.date_get_part_text(*part, timestamp_ms)
            }
            Rvalue::DateSetPart {
                part,
                timestamp_ms,
                values,
            } => {
                let text = self.date_set_part_text(*part, timestamp_ms, values)?;
                self.date_timestamp_result_text(&text, dest_ty)
            }
            Rvalue::UrlField { field, url } => self.url_field_text(*field, url),
            Rvalue::FileReadText { path } => self.file_read_text(path),
            Rvalue::FileWriteText { path, text } => self.file_write_text(path, text),
            Rvalue::Await(operand) => {
                if self.mir.types.get(self.operand_ty(operand)?) == Some(&Type::None) {
                    return self.default_value(dest_ty);
                }
                let awaited = format!("{}.await", self.await_operand_text(operand)?);
                if let Some(local) = operand_local(operand)
                    && self.local_is_async_throwing_call_result(local)?
                {
                    Ok(format!("{awaited}?"))
                } else {
                    Ok(awaited)
                }
            }
            Rvalue::AsyncOp { op, args } => {
                let text = self.async_op_text(*op, args)?;
                if matches!(op, smelt_hir::AsyncOp::Sleep)
                    && let Some(Type::Future(item)) = self.mir.types.get(dest_ty)
                    && self.mir.types.get(*item) != Some(&Type::None)
                {
                    return Ok(format!(
                        "Box::pin(async move {{ {text}.await; {} }}) as {}",
                        self.default_value(*item)?,
                        self.type_text_with_impl_trait(dest_ty, false)?
                    ));
                }
                Ok(text)
            }
        }
    }

    /// Converts a runtime-backed async operation to Rust.
    /// Validates two same-typed set operands.
    pub(super) fn validate_set_pair_operands(
        &self,
        left: &Operand,
        right: &Operand,
        context: &str,
    ) -> Result<TypeId, EmitError> {
        let left_ty = self.operand_ty(left)?;
        if self.operand_ty(right)? != left_ty {
            return Err(EmitError::new(format!(
                "{context} operands must have the same set type"
            )));
        }
        if !matches!(self.mir.types.get(left_ty), Some(Type::Set(_))) {
            return Err(EmitError::new(format!("{context} operands must be sets")));
        }
        Ok(left_ty)
    }

    /// Validates a set receiver and item operand, returning the set type.
    /// Validates a set receiver and item operand, returning the set type.
    pub(super) fn validate_set_item_operands(
        &self,
        set: &Operand,
        item: &Operand,
        context: &str,
    ) -> Result<TypeId, EmitError> {
        let set_ty = self.operand_ty(set)?;
        let Some(Type::Set(item_ty)) = self.mir.types.get(set_ty) else {
            return Err(EmitError::new(format!("{context} receiver must be a set")));
        };
        if self.operand_ty(item)? != *item_ty {
            return Err(EmitError::new(format!(
                "{context} item must match the set element type"
            )));
        }
        Ok(set_ty)
    }

    /// Converts a set insertion operation to Rust text.
    /// Converts a blocking HTTP GET operation to Rust text.
    pub(super) fn http_get_text(&self, url: &Operand) -> Result<String, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(url)?),
            Some(Type::String)
        ) {
            return Err(EmitError::new("HTTP GET URL must be a string"));
        }
        Ok(format!(
            "reqwest::blocking::get({}).expect(\"HTTP GET failed\").text().expect(\"HTTP response body read failed\")",
            self.operand_text(url)?
        ))
    }

    /// Converts a function call to its Rust text representation.
    /// Converts an awaited future operand without cloning it.
    pub(super) fn await_operand_text(&self, operand: &Operand) -> Result<String, EmitError> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => self.place_text(place),
            Operand::Const(_) => self.operand_text(operand),
        }
    }

    /// Emits binary operations involving `Option<T>` by coercing through the
    /// optional's inner type instead of letting Rust compare unrelated shapes.
    fn optional_binary_text(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
        dest_ty: TypeId,
    ) -> Result<Option<String>, EmitError> {
        let lhs_ty = self.operand_ty(lhs)?;
        let rhs_ty = self.operand_ty(rhs)?;
        let lhs_inner = self.optional_inner_ty(lhs_ty);
        let rhs_inner = self.optional_inner_ty(rhs_ty);
        if lhs_inner.is_none() && rhs_inner.is_none() {
            return Ok(None);
        }

        if matches!(op, smelt_hir::BinOp::Eq | smelt_hir::BinOp::NotEq) {
            return self.optional_equality_text(op, lhs, rhs, lhs_inner, rhs_inner);
        }

        if let Some(inner) = lhs_inner
            && rhs_inner.is_none()
            && self.is_numeric_type(inner)
            && self.is_numeric_type(rhs_ty)
        {
            let common_ty = self.common_numeric_type(inner, rhs_ty, dest_ty)?;
            return Ok(Some(format!(
                "{} {} {}",
                self.option_value_as_type_text(lhs, inner, common_ty)?,
                smelt_hir::bin_op_text(op),
                self.operand_as_type_text(rhs, common_ty)?
            )));
        }

        if let Some(inner) = rhs_inner
            && lhs_inner.is_none()
            && self.is_numeric_type(lhs_ty)
            && self.is_numeric_type(inner)
        {
            let common_ty = self.common_numeric_type(lhs_ty, inner, dest_ty)?;
            return Ok(Some(format!(
                "{} {} {}",
                self.operand_as_type_text(lhs, common_ty)?,
                smelt_hir::bin_op_text(op),
                self.option_value_as_type_text(rhs, inner, common_ty)?
            )));
        }

        Ok(None)
    }

    /// Emits equality checks for erased `SmeltUnknown` operands.
    fn unknown_binary_text(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
    ) -> Result<Option<String>, EmitError> {
        if !matches!(op, smelt_hir::BinOp::Eq | smelt_hir::BinOp::NotEq) {
            return Ok(None);
        }
        let lhs_ty = self.operand_ty(lhs)?;
        let rhs_ty = self.operand_ty(rhs)?;
        let lhs_is_erased = matches!(
            self.mir.types.get(lhs_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(lhs_ty);
        let rhs_is_erased = matches!(
            self.mir.types.get(rhs_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(rhs_ty);
        let lhs_is_none = self.mir.types.get(lhs_ty) == Some(&Type::None);
        let rhs_is_none = self.mir.types.get(rhs_ty) == Some(&Type::None);
        let text = if lhs_is_erased && rhs_is_none {
            format!("matches!({}, SmeltUnknown::Null)", self.operand_text(lhs)?)
        } else if rhs_is_erased && lhs_is_none {
            format!("matches!({}, SmeltUnknown::Null)", self.operand_text(rhs)?)
        } else if lhs_is_none || rhs_is_none {
            "false".to_owned()
        } else if lhs_is_erased || rhs_is_erased {
            let lhs_text = if lhs_is_erased {
                self.operand_text(lhs)?
            } else {
                self.unknown_wrap_text(lhs)?
            };
            let rhs_text = if rhs_is_erased {
                self.operand_text(rhs)?
            } else {
                self.unknown_wrap_text(rhs)?
            };
            format!("{lhs_text} == {rhs_text}")
        } else {
            return Ok(None);
        };
        Ok(Some(if op == smelt_hir::BinOp::NotEq {
            format!("!({text})")
        } else {
            text
        }))
    }

    /// Emits equality and inequality for optional operands.
    fn optional_equality_text(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
        lhs_inner: Option<TypeId>,
        rhs_inner: Option<TypeId>,
    ) -> Result<Option<String>, EmitError> {
        let negate = op == smelt_hir::BinOp::NotEq;
        let text = if lhs_inner.is_some() && self.operand_ty(rhs)? == self.none_ty {
            format!("{}.is_none()", self.operand_text(lhs)?)
        } else if rhs_inner.is_some() && self.operand_ty(lhs)? == self.none_ty {
            format!("{}.is_none()", self.operand_text(rhs)?)
        } else if let Some(inner) = lhs_inner
            && rhs_inner.is_none()
        {
            format!(
                "{} == Some({})",
                self.operand_text(lhs)?,
                self.operand_as_type_text(rhs, inner)?
            )
        } else if let Some(inner) = rhs_inner
            && lhs_inner.is_none()
        {
            format!(
                "Some({}) == {}",
                self.operand_as_type_text(lhs, inner)?,
                self.operand_text(rhs)?
            )
        } else {
            return Ok(None);
        };
        Ok(Some(if negate { format!("!({text})") } else { text }))
    }

    /// Emits numeric comparisons after coercing both operands to a shared scalar.
    ///
    /// HIR preserves TypeScript numeric intent even when one side has been
    /// inferred as `number` and the other as an integer-like literal. Rust does
    /// not compare `i64` and `f64` directly, so this keeps generated assertions
    /// and branch predicates type-directed instead of relying on literal shape.
    fn numeric_comparison_text(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
        dest_ty: TypeId,
    ) -> Result<Option<String>, EmitError> {
        if !matches!(
            op,
            smelt_hir::BinOp::Eq
                | smelt_hir::BinOp::NotEq
                | smelt_hir::BinOp::Lt
                | smelt_hir::BinOp::Lte
                | smelt_hir::BinOp::Gt
                | smelt_hir::BinOp::Gte
        ) {
            return Ok(None);
        }
        let lhs_ty = self.operand_ty(lhs)?;
        let rhs_ty = self.operand_ty(rhs)?;
        if !self.is_numeric_type(lhs_ty) || !self.is_numeric_type(rhs_ty) {
            return Ok(None);
        }
        let common_ty = self.common_numeric_type(lhs_ty, rhs_ty, dest_ty)?;
        Ok(Some(format!(
            "{} {} {}",
            self.operand_as_type_text(lhs, common_ty)?,
            smelt_hir::bin_op_text(op),
            self.operand_as_type_text(rhs, common_ty)?
        )))
    }

    /// Emits equality for first-class callback values.
    ///
    /// Stored callbacks lower to `Rc<RefCell<dyn FnMut...>>`, which has no
    /// structural equality. JavaScript compares function values by identity, so
    /// matching callback shapes can use `Rc::ptr_eq`; mismatched shapes are
    /// unequal and compile to a constant.
    fn function_equality_text(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
    ) -> Result<Option<String>, EmitError> {
        if !matches!(op, smelt_hir::BinOp::Eq | smelt_hir::BinOp::NotEq) {
            return Ok(None);
        }
        let lhs_ty = self.operand_ty(lhs)?;
        let rhs_ty = self.operand_ty(rhs)?;
        let lhs_is_function = matches!(self.mir.types.get(lhs_ty), Some(Type::Function(_)));
        let rhs_is_function = matches!(self.mir.types.get(rhs_ty), Some(Type::Function(_)));
        let lhs_contains_function = self.type_contains_function(lhs_ty);
        let rhs_contains_function = self.type_contains_function(rhs_ty);
        if !lhs_contains_function && !rhs_contains_function {
            return Ok(None);
        }
        let equal_text = if lhs_is_function && rhs_is_function && lhs_ty == rhs_ty {
            format!(
                "::std::rc::Rc::ptr_eq(&{}, &{})",
                self.operand_text(lhs)?,
                self.operand_text(rhs)?
            )
        } else {
            "false".to_owned()
        };
        Ok(Some(if op == smelt_hir::BinOp::NotEq {
            format!("!({equal_text})")
        } else {
            equal_text
        }))
    }

    /// Emits equality for structurally compatible values with erased members.
    ///
    /// Test assertions often compare a concrete expected value with a result
    /// whose item type was erased by a generic Remeda entrypoint, for example
    /// `Vec<SmeltUnknown>` against `Vec<f64>`. Rust needs both sides to have the
    /// same `PartialEq` shape, so this coerces the concrete side into the erased
    /// container before comparing.
    fn heterogeneous_equality_text(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
    ) -> Result<Option<String>, EmitError> {
        if !matches!(op, smelt_hir::BinOp::Eq | smelt_hir::BinOp::NotEq) {
            return Ok(None);
        }
        let lhs_ty = self.operand_ty(lhs)?;
        let rhs_ty = self.operand_ty(rhs)?;
        if lhs_ty == rhs_ty {
            return Ok(None);
        }
        let lhs_needs_erased = self.type_contains_unknown(lhs_ty)
            || matches!(
                self.mir.types.get(lhs_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            )
            || self.is_erased_class_type(lhs_ty);
        let rhs_needs_erased = self.type_contains_unknown(rhs_ty)
            || matches!(
                self.mir.types.get(rhs_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            )
            || self.is_erased_class_type(rhs_ty);
        let text = if lhs_needs_erased && !rhs_needs_erased {
            format!(
                "{} == {}",
                self.operand_text(lhs)?,
                self.operand_as_type_text(rhs, lhs_ty)?
            )
        } else if rhs_needs_erased && !lhs_needs_erased {
            if matches!(self.mir.types.get(lhs_ty), Some(Type::Tuple(_)))
                && matches!(self.mir.types.get(rhs_ty), Some(Type::List(_)))
            {
                format!(
                    "{} == {}",
                    self.operand_as_type_text(lhs, self.type_id(Type::Unknown)?)?,
                    self.operand_as_type_text(rhs, self.type_id(Type::Unknown)?)?
                )
            } else {
                format!(
                    "{} == {}",
                    self.operand_as_type_text(lhs, rhs_ty)?,
                    self.operand_text(rhs)?
                )
            }
        } else if self.equality_shapes_are_definitely_incompatible(lhs_ty, rhs_ty) {
            "false".to_owned()
        } else {
            return Ok(None);
        };
        Ok(Some(if op == smelt_hir::BinOp::NotEq {
            format!("!({text})")
        } else {
            text
        }))
    }

    /// Returns whether two static types cannot be equal under JavaScript-style deep equality.
    fn equality_shapes_are_definitely_incompatible(&self, left: TypeId, right: TypeId) -> bool {
        match (self.mir.types.get(left), self.mir.types.get(right)) {
            (Some(Type::Int | Type::Float), Some(Type::Int | Type::Float)) => false,
            (Some(Type::Optional(left_inner)), Some(Type::Optional(right_inner))) => {
                self.equality_shapes_are_definitely_incompatible(*left_inner, *right_inner)
            }
            (Some(Type::Optional(inner)), _) => {
                self.equality_shapes_are_definitely_incompatible(*inner, right)
            }
            (_, Some(Type::Optional(inner))) => {
                self.equality_shapes_are_definitely_incompatible(left, *inner)
            }
            (Some(Type::List(_)), Some(Type::List(_)))
            | (Some(Type::Dict(_, _)), Some(Type::Dict(_, _)))
            | (Some(Type::Set(_)), Some(Type::Set(_))) => false,
            (Some(Type::Tuple(left_items)), Some(Type::Tuple(right_items))) => {
                left_items.len() != right_items.len()
                    || left_items
                        .iter()
                        .zip(right_items.iter())
                        .any(|(left_item, right_item)| {
                            self.equality_shapes_are_definitely_incompatible(
                                *left_item,
                                *right_item,
                            )
                        })
            }
            (
                Some(
                    Type::List(_)
                    | Type::Dict(_, _)
                    | Type::Set(_)
                    | Type::Tuple(_)
                    | Type::Function(_),
                ),
                Some(
                    Type::List(_)
                    | Type::Dict(_, _)
                    | Type::Set(_)
                    | Type::Tuple(_)
                    | Type::Function(_),
                ),
            ) => true,
            (
                Some(Type::Bool | Type::Int | Type::Float | Type::String | Type::None),
                Some(Type::Bool | Type::Int | Type::Float | Type::String | Type::None),
            ) => {
                !matches!(
                    (self.mir.types.get(left), self.mir.types.get(right)),
                    (Some(Type::Int | Type::Float), Some(Type::Int | Type::Float))
                        | (Some(Type::None), Some(Type::None))
                ) && self.mir.types.get(left) != self.mir.types.get(right)
            }
            _ => false,
        }
    }

    /// Returns the inner type for `Option<T>`.
    fn optional_inner_ty(&self, ty: TypeId) -> Option<TypeId> {
        match self.mir.types.get(ty) {
            Some(Type::Optional(inner)) => Some(*inner),
            _ => None,
        }
    }

    /// Emits arithmetic involving erased operands through JavaScript-like numbers.
    fn erased_arithmetic_text(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
        dest_ty: TypeId,
    ) -> Result<Option<String>, EmitError> {
        if !matches!(
            op,
            smelt_hir::BinOp::Add
                | smelt_hir::BinOp::Sub
                | smelt_hir::BinOp::Mul
                | smelt_hir::BinOp::Div
                | smelt_hir::BinOp::Rem
        ) {
            return Ok(None);
        }
        let lhs_ty = self.operand_ty(lhs)?;
        let rhs_ty = self.operand_ty(rhs)?;
        let lhs_erased = matches!(
            self.mir.types.get(lhs_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(lhs_ty);
        let rhs_erased = matches!(
            self.mir.types.get(rhs_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(rhs_ty);
        let has_non_add_string = op != smelt_hir::BinOp::Add
            && matches!(
                (self.mir.types.get(lhs_ty), self.mir.types.get(rhs_ty)),
                (Some(Type::String), _) | (_, Some(Type::String))
            );
        if !lhs_erased && !rhs_erased && !has_non_add_string {
            return Ok(None);
        }
        let float_ty = self.type_id(Type::Float)?;
        let lhs_text = self.operand_as_type_text(lhs, float_ty)?;
        let rhs_text = self.operand_as_type_text(rhs, float_ty)?;
        let numeric_text = format!("{lhs_text} {} {rhs_text}", smelt_hir::bin_op_text(op));
        Ok(Some(match self.mir.types.get(dest_ty) {
            Some(Type::Int) => format!("(({numeric_text}) as f64).trunc() as i64"),
            Some(Type::Float) => numeric_text,
            Some(Type::String) => format!("({numeric_text}).to_string()"),
            _ => format!("SmeltUnknown::Number({numeric_text})"),
        }))
    }

    /// Returns whether a type is a Rust numeric scalar.
    fn is_numeric_type(&self, ty: TypeId) -> bool {
        matches!(self.mir.types.get(ty), Some(Type::Int | Type::Float))
    }

    /// Chooses the Rust scalar type for mixed numeric arithmetic.
    fn common_numeric_type(
        &self,
        lhs: TypeId,
        rhs: TypeId,
        dest_ty: TypeId,
    ) -> Result<TypeId, EmitError> {
        if self.is_numeric_type(dest_ty) {
            return Ok(dest_ty);
        }
        if matches!(self.mir.types.get(lhs), Some(Type::Float))
            || matches!(self.mir.types.get(rhs), Some(Type::Float))
        {
            return self.type_id(Type::Float);
        }
        self.type_id(Type::Int)
    }

    /// Emits an `Option<T>` value expression with a default for arithmetic.
    fn option_value_text(&self, operand: &Operand, inner: TypeId) -> Result<String, EmitError> {
        let fallback = match self.mir.types.get(inner) {
            Some(Type::Int) => "0_i64",
            Some(Type::Float) => "0.0",
            _ => return Ok(self.operand_text(operand)?),
        };
        Ok(format!(
            "{}.unwrap_or({fallback})",
            self.operand_text(operand)?
        ))
    }

    /// Emits an unwrapped optional arithmetic operand coerced to `target`.
    fn option_value_as_type_text(
        &self,
        operand: &Operand,
        inner: TypeId,
        target: TypeId,
    ) -> Result<String, EmitError> {
        let value = self.option_value_text(operand, inner)?;
        self.rendered_value_as_type_text(&value, inner, target)
    }

    /// Emits Rust for an optional-chain field read coerced to a destination type.
    pub(super) fn optional_field_text_for_dest(
        &self,
        receiver: &Operand,
        field: Symbol,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let (receiver_text, inner_ty, is_optional) = self.optional_receiver_parts(receiver)?;
        let field_ty = self.field_access_type(inner_ty, field)?;
        if is_optional {
            let value = self.field_access_text("_smelt_value", inner_ty, field)?;
            if matches!(self.mir.types.get(inner_ty), Some(Type::Unknown))
                && self.symbol_name(field)? == "groups"
            {
                return Ok(format!(
                    "{receiver_text}.as_ref().map(|_smelt_value| {value})"
                ));
            }
            if let Some(Type::Optional(dest_inner)) = self.mir.types.get(dest_ty) {
                if let Some(Type::Optional(field_inner)) = self.mir.types.get(field_ty) {
                    let mapped = self.optional_inner_map_text(&value, *field_inner, *dest_inner)?;
                    return Ok(format!(
                        "{receiver_text}.as_ref().and_then(|_smelt_value| {mapped})"
                    ));
                }
                let mapped = self.rendered_value_as_type_text(&value, field_ty, *dest_inner)?;
                return Ok(format!(
                    "{receiver_text}.as_ref().map(|_smelt_value| {mapped})"
                ));
            }
            let mapped = self.rendered_value_as_type_text(&value, field_ty, dest_ty)?;
            Ok(format!(
                "{receiver_text}.as_ref().map_or({}, |_smelt_value| {mapped})",
                self.default_value(dest_ty)?
            ))
        } else {
            let value = self.field_access_text(&receiver_text, inner_ty, field)?;
            if let Some(Type::Optional(dest_inner)) = self.mir.types.get(dest_ty) {
                if let Some(Type::Optional(field_inner)) = self.mir.types.get(field_ty) {
                    return self.optional_inner_map_text(&value, *field_inner, *dest_inner);
                }
                let mapped = self.rendered_value_as_type_text(&value, field_ty, *dest_inner)?;
                return Ok(format!("Some({mapped})"));
            }
            self.rendered_value_as_type_text(&value, field_ty, dest_ty)
        }
    }

    /// Returns the static type produced by a field read helper.
    fn field_access_type(&self, receiver_ty: TypeId, field: Symbol) -> Result<TypeId, EmitError> {
        if let Some(Type::Dict(_, value)) = self.mir.types.get(receiver_ty) {
            return Ok(*value);
        }
        if matches!(
            self.mir.types.get(receiver_ty),
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
        ) || self.is_erased_class_type(receiver_ty)
        {
            return self.type_id(Type::Unknown);
        }
        let Some(Type::Class { name, .. }) = self.mir.types.get(receiver_ty) else {
            return self.type_id(Type::Unknown);
        };
        if let Some(class) = self.mir.classes.iter().find(|class| class.name == *name) {
            return Ok(crate::classes::effective_class_fields(self.mir, class)
                .into_iter()
                .find(|class_field| class_field.name == field)
                .map_or_else(
                    || self.type_id(Type::Unknown),
                    |class_field| Ok(class_field.ty),
                )?);
        }
        if let Some(interface) = self
            .mir
            .interfaces
            .iter()
            .find(|interface| interface.name == *name)
        {
            return Ok(interface
                .fields
                .iter()
                .find(|item| item.name == field)
                .map_or_else(|| self.type_id(Type::Unknown), |item| Ok(item.ty))?);
        }
        self.type_id(Type::Unknown)
    }

    /// Emits Rust for a TypeScript optional-chain index read.
    pub(super) fn optional_index_text(
        &self,
        receiver: &Operand,
        index: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let (receiver_text, inner_ty, is_optional) = self.optional_receiver_parts(receiver)?;
        let result_ty = if let Some(Type::Optional(inner)) = self.mir.types.get(dest_ty) {
            *inner
        } else {
            self.type_id(Type::Unknown)?
        };
        if is_optional {
            let value =
                self.optional_index_access_text("_smelt_value", inner_ty, index, result_ty)?;
            Ok(format!(
                "{receiver_text}.as_ref().and_then(|_smelt_value| {value})"
            ))
        } else {
            self.optional_index_access_text(&receiver_text, inner_ty, index, result_ty)
        }
    }

    /// Emits Rust for a TypeScript optional-chain method call.
    pub(super) fn optional_method_text(
        &self,
        receiver: &Operand,
        method: Symbol,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let (receiver_text, inner_ty, is_optional) = self.optional_receiver_parts(receiver)?;
        let method_name = sanitize_ident(self.symbol_name(method)?);
        let args_text = args
            .iter()
            .map(|arg| self.operand_text(arg))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        if args.is_empty()
            && self.mir.types.get(inner_ty) == Some(&Type::String)
            && matches!(
                self.symbol_name(method)?,
                "toUpperCase" | "to_upper_case" | "toLowerCase" | "to_lower_case"
            )
        {
            let rust_method_name = match self.symbol_name(method)? {
                "toUpperCase" | "to_upper_case" => "to_uppercase",
                "toLowerCase" | "to_lower_case" => "to_lowercase",
                _ => {
                    return Err(EmitError::new(
                        "unsupported optional string case-conversion method",
                    ));
                }
            };
            if is_optional {
                return Ok(format!(
                    "{receiver_text}.as_ref().map(|_smelt_value| _smelt_value.{rust_method_name}())"
                ));
            }
            return Ok(format!("Some({receiver_text}.{rust_method_name}())"));
        }
        if is_optional {
            if self.optional_method_returns_optional(inner_ty, method, dest_ty)? {
                return Ok(format!(
                    "{receiver_text}.as_ref().and_then(|_smelt_value| _smelt_value.{method_name}({args_text}))"
                ));
            }
            Ok(format!(
                "{receiver_text}.as_ref().map(|_smelt_value| _smelt_value.{method_name}({args_text}))"
            ))
        } else {
            Ok(format!("Some({receiver_text}.{method_name}({args_text}))"))
        }
    }

    /// Return whether optional method chaining should flatten the method result.
    fn optional_method_returns_optional(
        &self,
        receiver_ty: TypeId,
        method: Symbol,
        dest_ty: TypeId,
    ) -> Result<bool, EmitError> {
        let Some(Type::Optional(dest_inner)) = self.mir.types.get(dest_ty) else {
            return Ok(false);
        };
        let Some(return_ty) = self.method_return_type(receiver_ty, method)? else {
            return Ok(false);
        };
        Ok(matches!(
            self.mir.types.get(return_ty),
            Some(Type::Optional(return_inner)) if return_inner == dest_inner
        ))
    }

    /// Resolve the static return type for a known class or interface method.
    fn method_return_type(
        &self,
        receiver_ty: TypeId,
        method: Symbol,
    ) -> Result<Option<TypeId>, EmitError> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(receiver_ty) else {
            return Ok(None);
        };
        if let Some(class) = self.mir.classes.iter().find(|class| class.name == *name) {
            for method_id in &class.methods {
                let function = self
                    .mir
                    .functions
                    .get(id_index(method_id.0, "method index does not fit usize")?)
                    .ok_or_else(|| EmitError::new("class method references an unknown function"))?;
                if function.name == method {
                    return Ok(Some(function.return_ty));
                }
            }
            return Ok(None);
        }
        if let Some(interface) = self
            .mir
            .interfaces
            .iter()
            .find(|interface| interface.name == *name)
        {
            return Ok(interface
                .methods
                .iter()
                .find(|signature| signature.name == method)
                .map(|signature| signature.return_ty));
        }
        Ok(None)
    }

    /// Returns receiver source, the non-optional receiver type, and whether it was optional.
    fn optional_receiver_parts(
        &self,
        receiver: &Operand,
    ) -> Result<(String, TypeId, bool), EmitError> {
        let receiver_ty = self.operand_ty(receiver)?;
        let receiver_text = self.operand_text(receiver)?;
        if let Some(Type::Optional(inner)) = self.mir.types.get(receiver_ty) {
            Ok((receiver_text, *inner, true))
        } else {
            Ok((receiver_text, receiver_ty, false))
        }
    }

    /// Emits TypeScript nullish coalescing for optional operands.
    fn optional_coalesce_text(
        &self,
        optional: &Operand,
        fallback: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let optional_ty = self.operand_ty(optional)?;
        if matches!(
            self.mir.types.get(optional_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(optional_ty)
        {
            let optional_text = self.operand_as_type_text(optional, optional_ty)?;
            let fallback_text = self.operand_as_type_text(fallback, optional_ty)?;
            let coalesced = format!(
                "match {optional_text} {{ SmeltUnknown::Null => {fallback_text}, value => value }}"
            );
            if matches!(
                self.mir.types.get(dest_ty),
                Some(Type::Optional(inner)) if matches!(
                    self.mir.types.get(*inner),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(*inner)
            ) {
                return Ok(format!("Some({coalesced})"));
            }
            if !matches!(
                self.mir.types.get(dest_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            ) && !self.is_erased_class_type(dest_ty)
            {
                return self.rendered_value_as_type_text(&coalesced, optional_ty, dest_ty);
            }
            return Ok(coalesced);
        }
        match self.mir.types.get(optional_ty) {
            Some(Type::Optional(inner)) => {
                let fallback_ty = self.operand_ty(fallback)?;
                if matches!(
                    self.mir.types.get(fallback_ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(fallback_ty)
                {
                    let optional_text = self.operand_text(optional)?;
                    let fallback_text = self.operand_as_type_text(fallback, fallback_ty)?;
                    let mapped_value =
                        self.rendered_value_as_type_text("value", fallback_ty, *inner)?;
                    let fallback_option = format!(
                        "match {fallback_text} {{ SmeltUnknown::Null => None, value => Some({mapped_value}) }}"
                    );
                    if matches!(self.mir.types.get(dest_ty), Some(Type::Optional(dest_inner)) if dest_inner == inner)
                    {
                        return Ok(format!("{optional_text}.or({fallback_option})"));
                    }
                    let coalesced = format!(
                        "{optional_text}.unwrap_or_else(|| {fallback_option}.unwrap_or({}))",
                        self.default_value(*inner)?
                    );
                    if dest_ty == *inner {
                        return Ok(coalesced);
                    }
                    return self.rendered_value_as_type_text(&coalesced, *inner, dest_ty);
                }
                if let Some(Type::Optional(fallback_inner)) = self.mir.types.get(fallback_ty)
                    && fallback_inner == inner
                    && matches!(self.mir.types.get(dest_ty), Some(Type::Optional(dest_inner)) if dest_inner == inner)
                {
                    return Ok(format!(
                        "{}.clone().or({})",
                        self.operand_text(optional)?,
                        self.operand_text(fallback)?
                    ));
                }
                let coalesced = format!(
                    "{}.clone().unwrap_or({})",
                    self.operand_text(optional)?,
                    self.operand_as_type_text(fallback, *inner)?
                );
                if dest_ty == *inner {
                    Ok(coalesced)
                } else {
                    self.rendered_value_as_type_text(&coalesced, *inner, dest_ty)
                }
            }
            Some(Type::None) => self.operand_text(fallback),
            _ => self.operand_text(optional),
        }
    }

    /// Emit a source `Option<S>` as a destination `Option<T>`.
    fn optional_inner_map_text(
        &self,
        value_text: &str,
        source_inner: TypeId,
        dest_inner: TypeId,
    ) -> Result<String, EmitError> {
        if source_inner == dest_inner {
            return Ok(value_text.to_owned());
        }
        let mapped = self.rendered_value_as_type_text("_smelt_inner", source_inner, dest_inner)?;
        Ok(format!("{value_text}.map(|_smelt_inner| {mapped})"))
    }

    /// Packs scalar callback call arguments for an erased rest-vector callback ABI.
    fn rest_vector_call_args_text(
        &self,
        args: &[Operand],
        function: Option<&FunctionType>,
    ) -> Result<Option<Vec<String>>, EmitError> {
        let Some(function_ty) = function else {
            return Ok(None);
        };
        let Some(0) = function_ty.rest else {
            return Ok(None);
        };
        let [param] = function_ty.params.as_slice() else {
            return Ok(None);
        };
        let Some(Type::List(item)) = self.mir.types.get(*param) else {
            return Ok(None);
        };
        if self.mir.types.get(*item) != Some(&Type::Unknown) {
            return Ok(None);
        }
        if args.is_empty() {
            return Ok(None);
        }
        if let [single_arg] = args
            && self.operand_ty(single_arg)? == *param
        {
            return Ok(None);
        }
        let items = args
            .iter()
            .map(|arg| self.operand_as_type_text(arg, *item))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        Ok(Some(vec![format!("vec![{items}]")]))
    }

    /// Emits `Object.assign` when the target is a callable JavaScript value.
    ///
    /// JavaScript functions can carry own properties, but Rust closures cannot.
    /// The current Rust representation keeps the callable value as the runtime
    /// value and relies on MIR operands to preserve evaluation of the property
    /// expressions before this rvalue is emitted. This gives the frontend and
    /// MIR a distinct operation for callable object assignment without making
    /// closure calls or function returns stop compiling.
    fn callable_object_assign_text(
        &self,
        callable: &Operand,
        _props: &[(Symbol, Operand)],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        if matches!(
            self.mir.types.get(dest_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(dest_ty)
        {
            return self.unknown_wrap_text(callable);
        }
        self.operand_text(callable)
    }

    /// Emit an erased imported class instance as an unknown object.
    ///
    /// External constructors from packages that are not part of the current
    /// manifest cannot be called directly from generated Rust. The arguments
    /// are still rendered into a discarded tuple so their lowered code remains
    /// type-checked and any future side-effect modeling has a concrete place to
    /// attach before the value is represented as `SmeltUnknown`.
    fn external_class_instance_text(
        &self,
        class: Symbol,
        args: &[Operand],
    ) -> Result<String, EmitError> {
        let class_name = self.symbol_name(class)?;
        if class_name == "RegExp" {
            let pattern = args.first().map_or_else(
                || Ok("\"\".to_owned()".to_owned()),
                |arg| self.string_like_operand_text(arg, "RegExp pattern"),
            )?;
            let flags = args.get(1).map_or_else(
                || Ok("\"\".to_owned()".to_owned()),
                |arg| self.string_like_operand_text(arg, "RegExp flags"),
            )?;
            return Ok(format!("SmeltRegExp::new({pattern}, {flags})"));
        }
        let args_text = args
            .iter()
            .map(|arg| self.operand_text(arg))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let args_tuple_text = if args_text.is_empty() {
            "()".to_owned()
        } else {
            format!("({args_text},)")
        };
        Ok(format!(
            "{{ let _smelt_external_args = {args_tuple_text}; let mut _smelt_external = ::std::collections::HashMap::new(); _smelt_external.insert(\"__class\".to_owned(), SmeltUnknown::String({class_name:?}.to_owned())); SmeltUnknown::Object(_smelt_external) }}"
        ))
    }

    /// Emits a field read against a named in-scope receiver value.
    fn field_access_text(
        &self,
        receiver_text: &str,
        receiver_ty: TypeId,
        field: Symbol,
    ) -> Result<String, EmitError> {
        if let Some(Type::Dict(key, _)) = self.mir.types.get(receiver_ty)
            && self.mir.types.get(*key) == Some(&Type::String)
        {
            let field_name = self.symbol_source_name(field)?;
            return Ok(format!(
                "{receiver_text}.get({field_name:?}).cloned().expect(\"missing field\")"
            ));
        }
        if matches!(
            self.mir.types.get(receiver_ty),
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
        ) || self.is_erased_class_type(receiver_ty)
        {
            let field_name = self.symbol_source_name(field)?;
            return Ok(format!(
                "match {receiver_text} {{ SmeltUnknown::Object(map) => match map.get({field_name:?}).cloned().unwrap_or(SmeltUnknown::Null) {{ SmeltUnknown::Object(mut getter) if getter.contains_key(\"__smelt_get\") => match getter.remove(\"__smelt_get\") {{ Some(SmeltUnknown::Function(smelt_getter)) => (&mut *smelt_getter.borrow_mut())(Vec::new()).unwrap_or_else(|error| panic!(\"{{}}\", error)), _ => SmeltUnknown::Null }}, value => value }}, _ => SmeltUnknown::Null }}"
            ));
        }
        if matches!(self.mir.types.get(receiver_ty), Some(Type::String)) {
            return self.string_field_text(receiver_text, field);
        }
        if let Some(Type::Class { name, .. }) = self.mir.types.get(receiver_ty)
            && self.symbol_name(*name)? == "RegExp"
        {
            return self.regexp_field_text(receiver_text, field);
        }
        let Some(Type::Class { name, .. }) = self.mir.types.get(receiver_ty) else {
            return Err(EmitError::new(
                "optional field codegen requires a class or string-keyed dict receiver",
            ));
        };
        if let Some(method_text) = self.class_method_reference_text(receiver_text, *name, field)? {
            return Ok(method_text);
        }
        Ok(format!(
            "{receiver_text}.{}.clone()",
            sanitize_ident(self.symbol_name(field)?)
        ))
    }

    /// Emits a bound closure for class method references such as `this.set`.
    ///
    /// TypeScript methods are callable values when read from an instance. Rust
    /// methods are not stored fields, so generated code captures a cloned
    /// receiver and forwards callback invocations through the inherent method.
    pub(super) fn class_method_reference_text(
        &self,
        receiver_text: &str,
        class: Symbol,
        method: Symbol,
    ) -> Result<Option<String>, EmitError> {
        let Some(function) = self.mir.functions.iter().find(|function| {
            matches!(
                function.origin,
                HirOrigin::ClassMethod {
                    class: function_class,
                    method: function_method,
                    ..
                } if function_class == class && function_method == method
            )
        }) else {
            return self.abstract_class_method_reference_text(class, method);
        };
        let method_name = sanitize_ident(self.symbol_name(method)?);
        let unknown_ty = self.type_id(Type::Unknown)?;
        let args = function
            .params
            .iter()
            .skip(1)
            .enumerate()
            .map(|(index, param)| {
                let param_ty = self.function_local_decl(function, *param)?.ty;
                let item =
                    format!("smelt_args.get({index}).cloned().unwrap_or(SmeltUnknown::Null)");
                self.rendered_value_as_type_text(&item, unknown_ty, param_ty)
            })
            .collect::<Result<Vec<_>, EmitError>>()?;
        let call = format!("smelt_receiver.{method_name}({})", args.join(", "));
        let returned = if function.can_throw {
            format!("{call}?")
        } else {
            call
        };
        let returned = self.unknown_wrap_value_text(&returned, function.return_ty)?;
        let receiver_clone = if receiver_text == "self" {
            self.self_struct_clone_text(class)?
        } else {
            format!("{receiver_text}.clone()")
        };
        Ok(Some(format!(
            "{{ let smelt_receiver = {receiver_clone}; SmeltUnknown::Function(::std::rc::Rc::new(::std::cell::RefCell::new(move |smelt_args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>({returned})))) }}"
        )))
    }

    /// Emits an erased callable placeholder for abstract method signatures.
    ///
    /// Abstract methods have no body to emit, but source code can still pass the
    /// method reference through generic callback storage on an abstract base.
    /// The concrete override is a later virtual-dispatch problem; this keeps the
    /// abstract slice type-checkable without inventing a stored field.
    fn abstract_class_method_reference_text(
        &self,
        class: Symbol,
        method: Symbol,
    ) -> Result<Option<String>, EmitError> {
        let Some(class_item) = self.mir.classes.iter().find(|item| item.name == class) else {
            return Ok(None);
        };
        if !class_item
            .abstract_methods
            .iter()
            .any(|candidate| candidate.name == method)
        {
            return Ok(None);
        }
        Ok(Some("SmeltUnknown::Function(::std::rc::Rc::new(::std::cell::RefCell::new(move |_smelt_args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>(SmeltUnknown::Null))))".to_owned()))
    }

    /// Emits a concrete clone of `self` without relying on generic `Clone` bounds.
    fn self_struct_clone_text(&self, class: Symbol) -> Result<String, EmitError> {
        let Some(class_item) = self.mir.classes.iter().find(|item| item.name == class) else {
            return Ok("(*self).clone()".to_owned());
        };
        let mut class_name = crate::classes::class_name_text(self.mir, class_item)?;
        if !class_item.type_params.is_empty() {
            let args = class_item
                .type_params
                .iter()
                .map(|_| "SmeltUnknown")
                .collect::<Vec<_>>()
                .join(", ");
            class_name = format!("{class_name}::<{args}>");
        }
        let mut fields = crate::classes::effective_class_fields(self.mir, class_item)
            .into_iter()
            .map(|field| {
                let name = sanitize_ident(self.symbol_name(field.name)?);
                Ok(format!("{name}: self.{name}.clone()"))
            })
            .collect::<Result<Vec<_>, EmitError>>()?;
        if !class_item.type_params.is_empty() {
            fields.push("_smelt_phantom: ::std::marker::PhantomData".to_owned());
        }
        Ok(format!("{class_name} {{ {} }}", fields.join(", ")))
    }

    /// Emits JavaScript RegExp-like metadata fields for regex strings.
    ///
    /// The frontend currently represents regex literals as pattern strings.
    /// Remeda tests still read fields such as `.source` and `.global` after a
    /// clone, so these field reads need to stay on the string representation
    /// instead of becoming Rust struct projection.
    pub(super) fn string_field_text(
        &self,
        receiver_text: &str,
        field: Symbol,
    ) -> Result<String, EmitError> {
        Ok(match self.symbol_name(field)? {
            "source" => format!("{receiver_text}.clone()"),
            "global" | "ignoreCase" | "ignore_case" | "multiline" => "false".to_owned(),
            "constructor" => "SmeltUnknown::Null".to_owned(),
            "length" => format!("{receiver_text}.chars().count() as i64"),
            _ => "SmeltUnknown::Null".to_owned(),
        })
    }

    /// Emits JavaScript RegExp metadata field reads.
    pub(super) fn regexp_field_text(
        &self,
        receiver_text: &str,
        field: Symbol,
    ) -> Result<String, EmitError> {
        Ok(match self.symbol_name(field)? {
            "source" => format!("{receiver_text}.source.clone()"),
            "global" => format!("{receiver_text}.has_flag('g')"),
            "ignoreCase" | "ignore_case" => format!("{receiver_text}.has_flag('i')"),
            "multiline" => format!("{receiver_text}.has_flag('m')"),
            "sticky" => format!("{receiver_text}.has_flag('y')"),
            "unicode" => format!("{receiver_text}.has_flag('u')"),
            "dotAll" | "dot_all" => format!("{receiver_text}.has_flag('s')"),
            "lastIndex" | "last_index" => format!("*{receiver_text}.last_index.borrow() as f64"),
            "constructor" => "SmeltUnknown::Null".to_owned(),
            _ => "SmeltUnknown::Null".to_owned(),
        })
    }

    /// Emits a JavaScript optional-chain index read against an in-scope receiver value.
    ///
    /// `value?.[index]` short-circuits only when the receiver is nullish, but a
    /// missing array/string element still produces `undefined`. Smelt models
    /// that as `None`, so this helper deliberately avoids the strict
    /// `expect("index out of bounds")` used by normal element access.
    fn optional_index_access_text(
        &self,
        receiver_text: &str,
        receiver_ty: TypeId,
        index: &Operand,
        result_ty: TypeId,
    ) -> Result<String, EmitError> {
        match self.mir.types.get(receiver_ty) {
            Some(Type::List(item_ty)) => {
                let index_text =
                    self.normalized_index_text(&format!("{receiver_text}.len()"), index)?;
                if let Some(Type::Optional(inner)) = self.mir.types.get(*item_ty)
                    && *inner == result_ty
                {
                    Ok(format!(
                        "{receiver_text}.get({index_text}).cloned().flatten()"
                    ))
                } else {
                    Ok(format!("{receiver_text}.get({index_text}).cloned()"))
                }
            }
            Some(Type::Dict(key_ty, _)) => {
                let key_text = if self.mir.types.get(*key_ty) == Some(&Type::String) {
                    self.property_key_to_string_text(
                        &self.operand_text(index)?,
                        self.operand_ty(index)?,
                    )?
                } else {
                    self.operand_as_type_text(index, *key_ty)?
                };
                Ok(format!("{receiver_text}.get(&{key_text}).cloned()"))
            }
            Some(Type::String) => {
                let index_text =
                    self.normalized_index_text(&format!("{receiver_text}.chars().count()"), index)?;
                Ok(format!(
                    "{receiver_text}.chars().nth({index_text}).map(|ch| ch.to_string())"
                ))
            }
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            | Some(Type::Class { .. })
                if matches!(
                    self.mir.types.get(receiver_ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(receiver_ty) =>
            {
                let index_ty = self.operand_ty(index)?;
                let index_text = self.operand_text(index)?;
                let key_text = self.property_key_to_string_text(&index_text, index_ty)?;
                let numeric_index_text = match self.mir.types.get(index_ty) {
                    Some(Type::Int | Type::Float) => index_text,
                    Some(Type::Bool) => format!("if {index_text} {{ 1.0 }} else {{ 0.0 }}"),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    | Some(Type::Class { .. })
                        if self.is_erased_class_type(index_ty)
                            || matches!(
                                self.mir.types.get(index_ty),
                                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                            ) =>
                    {
                        format!(
                            "match {index_text}.clone() {{ SmeltUnknown::Number(value) => value, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::Null | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) => f64::NAN }}"
                        )
                    }
                    _ => "f64::NAN".to_owned(),
                };
                let string_some = if self.mir.types.get(result_ty) == Some(&Type::String) {
                    "value.chars().nth(index).map(|ch| ch.to_string())".to_owned()
                } else {
                    "value.chars().nth(index).map(|ch| SmeltUnknown::String(ch.to_string()))"
                        .to_owned()
                };
                let array_some = if self.mir.types.get(result_ty) == Some(&Type::String) {
                    "values.get(index).cloned().map(|value| match value { SmeltUnknown::String(value) => value, other => other.to_string() })".to_owned()
                } else {
                    "values.get(index).cloned()".to_owned()
                };
                let object_some = if self.mir.types.get(result_ty) == Some(&Type::String) {
                    format!(
                        "values.get(&{key_text}).cloned().map(|value| match value {{ SmeltUnknown::String(value) => value, other => other.to_string() }})"
                    )
                } else {
                    format!("values.get(&{key_text}).cloned()")
                };
                let primitive_none = "SmeltUnknown::Bool(_) | SmeltUnknown::Number(_) | SmeltUnknown::Symbol(_) | SmeltUnknown::Null | SmeltUnknown::Function(_) => None";
                Ok(format!(
                    r"match {receiver_text}.clone() {{
                        SmeltUnknown::String(value) => {{
                            let len = value.chars().count() as i64;
                            let index = {numeric_index_text} as i64;
                            let normalized = if index < 0 {{ len + index }} else {{ index }};
                            usize::try_from(normalized).ok().and_then(|index| {string_some})
                        }}
                        SmeltUnknown::Array(values) => {{
                            let len = values.len() as i64;
                            let index = {numeric_index_text} as i64;
                            let normalized = if index < 0 {{ len + index }} else {{ index }};
                            usize::try_from(normalized).ok().and_then(|index| {array_some})
                        }}
                        SmeltUnknown::Object(values) => {object_some},
                        {primitive_none},
                    }}"
                ))
            }
            _ => Ok("None".to_owned()),
        }
    }

    // Converts an operand to console.log argument format and returns format string and value.
}
