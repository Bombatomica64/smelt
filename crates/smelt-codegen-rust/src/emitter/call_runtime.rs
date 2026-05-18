//! Call Runtime emission helpers.

use super::*;

impl FunctionEmitter<'_> {
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
                if matches!(
                    self.mir.types.get(dest_ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(dest_ty)
                {
                    let items_text = items
                        .iter()
                        .map(|item| self.operand_as_type_text(item, self.type_id(Type::Unknown)?))
                        .collect::<Result<Vec<_>, _>>()?
                        .join(", ");
                    return Ok(format!("SmeltUnknown::Array(vec![{items_text}])"));
                }
                if matches!(
                    self.mir.types.get(dest_ty),
                    Some(Type::List(item)) if self.mir.types.get(*item) == Some(&Type::Unknown)
                ) {
                    let items_text = items
                        .iter()
                        .map(|item| self.operand_as_type_text(item, self.type_id(Type::Unknown)?))
                        .collect::<Result<Vec<_>, _>>()?
                        .join(", ");
                    return Ok(format!("vec![{items_text}]"));
                }
                let items_text = items
                    .iter()
                    .map(|item| self.operand_text(item))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                Ok(format!("vec![{items_text}]"))
            }
            Rvalue::Set(items) => {
                if items.is_empty() {
                    return Ok("::std::collections::HashSet::new()".to_owned());
                }
                let items_text = items
                    .iter()
                    .map(|item| self.operand_text(item))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                Ok(format!("::std::collections::HashSet::from([{items_text}])"))
            }
            Rvalue::Dict(entries) => {
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
                            if matches!(self.mir.types.get(value_ty), Some(Type::Function(_)))
                                && self.operand_ty(entry_value)? == value_ty
                            {
                                format!(
                                    "{{ let smelt_fn: {} = {}; smelt_fn }}",
                                    self.type_text_with_impl_trait(value_ty, false)?,
                                    self.operand_text(entry_value)?
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
                if let Some(text) = self.erased_arithmetic_text(*op, lhs, rhs, dest_ty)? {
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
                Ok(format!(
                    "{} {} {}",
                    self.operand_text(lhs)?,
                    smelt_hir::bin_op_text(*op),
                    self.operand_text(rhs)?
                ))
            }
            Rvalue::Unary { op, operand } => match op {
                smelt_hir::UnaryOp::Not => Ok(format!("!{}", self.truthy_operand_text(operand)?)),
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
            Rvalue::OptionalIndex { receiver, index } => self.optional_index_text(receiver, index),
            Rvalue::OptionalMethod {
                receiver,
                method,
                args,
            } => self.optional_method_text(receiver, *method, args),
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
                let cast_text = self.unknown_cast_text(unknown_value, *target)?;
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
                self.external_class_instance_text(*class, args)
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
            Rvalue::Closure { id, .. } => self.closure_text_for_type(*id, dest_ty),
            Rvalue::ClosureCall { callee, args } => {
                let callee_text = self.operand_text(callee)?;
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
                let local_params = match callee {
                    Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => {
                        match self.mir.types.get(self.local_decl(*local)?.ty) {
                            Some(Type::Function(function)) => Some(function.params.as_slice()),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                let inferred_params = match self.mir.types.get(self.operand_ty(callee)?) {
                    Some(Type::Function(function)) => function.params.as_slice(),
                    _ => &[],
                };
                let params = emitted_params
                    .as_deref()
                    .or(local_params)
                    .unwrap_or(inferred_params);
                let mut rendered_args = args
                    .iter()
                    .zip(params.iter())
                    .map(|(arg, param)| {
                        let text = self.operand_as_type_text(arg, *param)?;
                        if self.type_text(*param)? == "Vec<SmeltUnknown>"
                            && text.contains(".into_iter().map(|value| value).collect::<Vec<_>>()")
                        {
                            Ok(text.replace(
                                ".into_iter().map(|value| value).collect::<Vec<_>>()",
                                ".into_iter().map(IntoSmeltUnknown::into_smelt_unknown).collect::<Vec<_>>()",
                            ))
                        } else {
                            Ok(text)
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                for param in params.iter().skip(args.len()) {
                    rendered_args.push(self.default_value(*param)?);
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
                let source_ty = match self.mir.types.get(self.operand_ty(callee)?) {
                    Some(Type::Function(function)) => function.return_ty,
                    _ => dest_ty,
                };
                self.rendered_value_as_type_text(&call_text, source_ty, dest_ty)
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
                self.list_slice_text(list, start.as_ref(), end.as_ref())
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
                self.callable_object_assign_text(callable, props)
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
            Rvalue::Await(operand) => Ok(format!("{}.await", self.await_operand_text(operand)?)),
            Rvalue::AsyncOp { op, args } => self.async_op_text(*op, args),
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
            Some(Type::Int) => format!("({numeric_text}).trunc() as i64"),
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
            if let Some(Type::Optional(dest_inner)) = self.mir.types.get(dest_ty) {
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
        let Some(class) = self.mir.classes.iter().find(|class| class.name == *name) else {
            return self.type_id(Type::Unknown);
        };
        Ok(crate::classes::effective_class_fields(self.mir, class)
            .into_iter()
            .find(|class_field| class_field.name == field)
            .map_or_else(
                || self.type_id(Type::Unknown),
                |class_field| Ok(class_field.ty),
            )?)
    }

    /// Emits Rust for a TypeScript optional-chain index read.
    pub(super) fn optional_index_text(
        &self,
        receiver: &Operand,
        index: &Operand,
    ) -> Result<String, EmitError> {
        let (receiver_text, inner_ty, is_optional) = self.optional_receiver_parts(receiver)?;
        if is_optional {
            let value = self.index_access_text("_smelt_value", inner_ty, index)?;
            Ok(format!(
                "{receiver_text}.as_ref().map(|_smelt_value| {value})"
            ))
        } else {
            let value = self.index_access_text(&receiver_text, inner_ty, index)?;
            Ok(format!("Some({value})"))
        }
    }

    /// Emits Rust for a TypeScript optional-chain method call.
    pub(super) fn optional_method_text(
        &self,
        receiver: &Operand,
        method: Symbol,
        args: &[Operand],
    ) -> Result<String, EmitError> {
        let (receiver_text, _inner_ty, is_optional) = self.optional_receiver_parts(receiver)?;
        let method_name = sanitize_ident(self.symbol_name(method)?);
        let args_text = args
            .iter()
            .map(|arg| self.operand_text(arg))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        if is_optional {
            Ok(format!(
                "{receiver_text}.as_ref().map(|_smelt_value| _smelt_value.{method_name}({args_text}))"
            ))
        } else {
            Ok(format!("Some({receiver_text}.{method_name}({args_text}))"))
        }
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
            Some(Type::Optional(inner)) => Ok(format!(
                "{}.clone().unwrap_or({})",
                self.operand_text(optional)?,
                self.operand_as_type_text(fallback, *inner)?
            )),
            Some(Type::None) => self.operand_text(fallback),
            _ => self.operand_text(optional),
        }
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
    ) -> Result<String, EmitError> {
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
            let field_name = self.symbol_name(field)?;
            return Ok(format!(
                "{receiver_text}.get({field_name:?}).cloned().expect(\"missing field\")"
            ));
        }
        if matches!(
            self.mir.types.get(receiver_ty),
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
        ) {
            let field_name = self.symbol_name(field)?;
            return Ok(format!(
                "match {receiver_text} {{ SmeltUnknown::Object(map) => map.get({field_name:?}).cloned().unwrap_or(SmeltUnknown::Null), _ => SmeltUnknown::Null }}"
            ));
        }
        if self.is_erased_class_type(receiver_ty) {
            return Ok("SmeltUnknown::Null".to_owned());
        }
        let Some(Type::Class { .. }) = self.mir.types.get(receiver_ty) else {
            return Err(EmitError::new(
                "optional field codegen requires a class or string-keyed dict receiver",
            ));
        };
        Ok(format!(
            "{receiver_text}.{}.clone()",
            sanitize_ident(self.symbol_name(field)?)
        ))
    }

    /// Emits an index read against a named in-scope receiver value.
    fn index_access_text(
        &self,
        receiver_text: &str,
        receiver_ty: TypeId,
        index: &Operand,
    ) -> Result<String, EmitError> {
        match self.mir.types.get(receiver_ty) {
            Some(Type::List(_)) => {
                let index_text =
                    self.normalized_index_text(&format!("{receiver_text}.len()"), index)?;
                Ok(format!(
                    "{receiver_text}.get({index_text}).cloned().expect(\"index out of bounds\")"
                ))
            }
            Some(Type::Dict(key_ty, _)) => {
                let key_text = self.operand_as_type_text(index, *key_ty)?;
                Ok(format!(
                    "{receiver_text}.get(&{key_text}).cloned().expect(\"index out of bounds\")"
                ))
            }
            Some(Type::String) => {
                let index_text =
                    self.normalized_index_text(&format!("{receiver_text}.chars().count()"), index)?;
                Ok(format!(
                    "{receiver_text}.chars().nth({index_text}).map(|ch| ch.to_string()).expect(\"index out of bounds\")"
                ))
            }
            _ => Ok("Default::default()".to_owned()),
        }
    }

    // Converts an operand to console.log argument format and returns format string and value.
}
