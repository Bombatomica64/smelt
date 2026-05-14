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
                let items_text = items
                    .iter()
                    .map(|item| self.operand_text(item))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                Ok(format!("vec![{items_text}]"))
            }
            Rvalue::Set(items) => {
                let items_text = items
                    .iter()
                    .map(|item| self.operand_text(item))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                Ok(format!("::std::collections::HashSet::from([{items_text}])"))
            }
            Rvalue::Dict(entries) => {
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
                            self.operand_as_type_text(entry_value, value_ty)?
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
                if *op == smelt_hir::BinOp::Add
                    && matches!(
                        self.mir.types.get(self.operand_ty(lhs)?),
                        Some(Type::String)
                    )
                {
                    let rhs_text = self.operand_text(rhs)?;
                    let rhs_expr = if matches!(
                        self.mir.types.get(self.operand_ty(rhs)?),
                        Some(Type::String)
                    ) {
                        format!("&{rhs_text}")
                    } else {
                        format!("&{rhs_text}.to_string()")
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
            Rvalue::Unary { op, operand } => {
                let op_text = match op {
                    smelt_hir::UnaryOp::Not => "!",
                    smelt_hir::UnaryOp::Neg => "-",
                };
                Ok(format!("{op_text}{}", self.operand_text(operand)?))
            }
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
            Rvalue::OptionalField { receiver, field } => self.optional_field_text(receiver, *field),
            Rvalue::OptionalIndex { receiver, index } => self.optional_index_text(receiver, index),
            Rvalue::OptionalMethod {
                receiver,
                method,
                args,
            } => self.optional_method_text(receiver, *method, args),
            Rvalue::OptionalCoalesce { optional, fallback } => {
                self.optional_coalesce_text(optional, fallback)
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
            } => self.unknown_cast_text(unknown_value, *target),
            Rvalue::Struct { class, fields } => {
                let class_name = sanitize_ident(self.symbol_name(*class)?);
                let mir_class = self
                    .mir
                    .classes
                    .iter()
                    .find(|item| item.name == *class)
                    .ok_or_else(|| EmitError::new("struct rvalue references an unknown class"))?;
                let mut parts = Vec::new();
                for field in &mir_class.fields {
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
                Ok(format!("{class_name} {{ {} }}", parts.join(", ")))
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
            Rvalue::Closure { id, .. } => self.closure_text(*id),
            Rvalue::ClosureCall { callee, args } => {
                let args_text = args
                    .iter()
                    .map(|arg| self.operand_text(arg))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                Ok(format!("{}({args_text})", self.operand_text(callee)?))
            }
            Rvalue::ListCallback { op, list, callback } => {
                self.list_callback_text(*op, list, callback, dest_ty)
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
            } => self.string_split_text(haystack, separator),
            Rvalue::StringJoin { items, separator } => self.string_join_text(items, separator),
            Rvalue::JsonStringify { value: json_value } => {
                self.json_stringify_text(json_value, dest_ty)
            }
            Rvalue::JsonParse { text } => self.json_parse_text(text, dest_ty),
            Rvalue::HttpGetText { url } => self.http_get_text(url),
            Rvalue::DateNow => Ok("chrono::Utc::now().timestamp_millis()".to_owned()),
            Rvalue::DateToIsoString { timestamp_ms } => self.date_to_iso_string_text(timestamp_ms),
            Rvalue::DateFromParts { parts } => self.date_from_parts_text(parts),
            Rvalue::DateGetPart { part, timestamp_ms } => {
                self.date_get_part_text(*part, timestamp_ms)
            }
            Rvalue::DateSetPart {
                part,
                timestamp_ms,
                values,
            } => self.date_set_part_text(*part, timestamp_ms, values),
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
            Operand::Const(_) => Err(EmitError::new("await operand cannot be a constant")),
        }
    }

    /// Emits Rust for a TypeScript optional-chain field read.
    pub(super) fn optional_field_text(
        &self,
        receiver: &Operand,
        field: Symbol,
    ) -> Result<String, EmitError> {
        let (receiver_text, inner_ty, is_optional) = self.optional_receiver_parts(receiver)?;
        if is_optional {
            let value = self.field_access_text("_smelt_value", inner_ty, field)?;
            Ok(format!(
                "{receiver_text}.as_ref().map(|_smelt_value| {value})"
            ))
        } else {
            let value = self.field_access_text(&receiver_text, inner_ty, field)?;
            Ok(format!("Some({value})"))
        }
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
    ) -> Result<String, EmitError> {
        let optional_ty = self.operand_ty(optional)?;
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
            Some(Type::Dict(_, _)) => Ok(format!(
                "{receiver_text}.get(&{}).cloned().expect(\"index out of bounds\")",
                self.operand_text(index)?
            )),
            Some(Type::String) => {
                let index_text =
                    self.normalized_index_text(&format!("{receiver_text}.chars().count()"), index)?;
                Ok(format!(
                    "{receiver_text}.chars().nth({index_text}).map(|ch| ch.to_string()).expect(\"index out of bounds\")"
                ))
            }
            _ => Err(EmitError::new(
                "optional index codegen requires a list, string, or dict receiver",
            )),
        }
    }

    // Converts an operand to console.log argument format and returns format string and value.
}
