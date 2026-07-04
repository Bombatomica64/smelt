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

    /// Render the runtime dispatch snippet for invoking a dynamically-typed
    /// callable (`SmeltUnknown`).
    ///
    /// When the callee is erased to `SmeltUnknown` (source `unknown`, a union, or
    /// an erased class), the concrete callable shape is only known at runtime: it
    /// may be a `SmeltUnknown::Function`, or an object that exposes a
    /// `__smelt_call` function member (a callable object). This snippet clones the
    /// callee value, coerces the argument expression into a `Vec<SmeltUnknown>`,
    /// extracts whichever callable form is present, invokes it (propagating a
    /// thrown error via `panic!`), and falls back to `SmeltUnknown::Null` when the
    /// value is not callable.
    ///
    /// `callee_text` is the expression producing the callee value. `args_expr` is
    /// the expression fed to `Into::into` to build the argument vector; callers
    /// differ only in how they materialize that expression (an explicit
    /// `vec![...]` for `ClosureCall`, a pre-flattened list for
    /// `ClosureCallSpread`), so it is the single parameterized hole in the shared
    /// snippet.
    fn dynamic_callable_dispatch_text(&self, callee_text: &str, args_expr: &str) -> String {
        format!(
            "{{ let smelt_function_value = {callee_text}.clone(); let smelt_call_args: Vec<SmeltUnknown> = Into::into({args_expr}); let smelt_callable = match smelt_function_value {{ SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get(\"__smelt_call\") {{ Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function), _ => None }}, _ => None }}; if let Some(smelt_function) = smelt_callable {{ (smelt_function)(smelt_call_args).unwrap_or_else(|error| panic!(\"{{}}\", error)) }} else {{ SmeltUnknown::Null }} }}"
        )
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
                self.value_at_type(entry_value, field.ty)?
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
        let text = self.rvalue_text_for_dest_inner(value, dest_ty)?;
        // An rvalue that emits a bare `SmeltUnknown` constructor (`SmeltUnknown::
        // Number(..)` from arithmetic, a boxed literal) but is assigned to a
        // concrete-union destination is reconstructed into the tagged union.
        // Guarding on the literal `SmeltUnknown::` prefix keeps values that
        // already produced the tagged union (`SmeltUnion…::M0(..)`, a `Use` of a
        // union local) untouched, avoiding a double wrap.
        if text.starts_with("SmeltUnknown::") && self.concrete_union_members(dest_ty).is_some() {
            return Ok(format!(
                "{}::from_smelt_unknown({text})",
                union::union_name(dest_ty)
            ));
        }
        // List-producing operations (concat/slice/flat/map/copy/…) emit a bare
        // `Vec`, but `Type::List` now lowers to the identity-bearing `SmeltList`.
        // Coerce through `Into` at this single choke point: it is a no-op when the
        // rvalue already produced a `SmeltList` (the blanket `From<T> for T`) and
        // wraps a fresh-identity list otherwise (`From<Vec<T>>`).
        if matches!(self.mir.types.get(dest_ty), Some(Type::List(_))) {
            // `Default::default()` is an ambiguous `Into` source (both Vec and
            // SmeltList satisfy it); emit the SmeltList default directly.
            if text == "Default::default()" {
                return Ok("SmeltList::default()".to_owned());
            }
            return Ok(format!("Into::<SmeltList<_>>::into({text})"));
        }
        Ok(text)
    }

    /// Render an rvalue at a destination type, before list-identity coercion.
    ///
    /// This is the dispatch body behind [`Self::rvalue_text_for_dest`]: it
    /// matches every `Rvalue` variant and produces the raw Rust expression for
    /// it. The public wrapper then applies the single `SmeltList` `Into`
    /// choke-point coercion for list-typed destinations, so variants here may
    /// freely emit bare `Vec` expressions.
    fn rvalue_text_for_dest_inner(
        &self,
        value: &Rvalue,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        match value {
            Rvalue::Use(operand)
                if matches!(
                    self.mir.types.get(self.operand_ty(operand)?),
                    Some(Type::Optional(inner))
                        if *inner == dest_ty
                            && !matches!(self.mir.types.get(dest_ty), Some(Type::Function(_)))
                ) =>
            {
                Ok(format!(
                    "{}.clone().expect(\"optional value was absent after narrowing\")",
                    self.operand_text(operand)?
                ))
            }
            Rvalue::Use(operand) => self.value_at_type(operand, dest_ty),
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
                        .map(|item| self.erase(&self.list_literal_operand(item)))
                        .collect::<Result<Vec<_>, _>>()?
                        .join(", ");
                    return Ok(self.erase_array_text(&items_text));
                }
                if let Some(&Type::List(item_ty)) = self.mir.types.get(dest_ty) {
                    let items_text = items
                        .iter()
                        .map(|item| self.value_at_type(&self.list_literal_operand(item), item_ty))
                        .collect::<Result<Vec<_>, _>>()?
                        .join(", ");
                    // Annotate the backing Vec with the element type so heterogeneous
                    // closures coerce to the shared `Rc<dyn Fn>` element (the dest
                    // annotation used to drive this before `Type::List` became SmeltList).
                    let elem_text = self.type_text_with_impl_trait(item_ty, false)?;
                    return Ok(format!(
                        "SmeltList::from({{ let smelt_list_items: Vec<{elem_text}> = vec![{items_text}]; smelt_list_items }})"
                    ));
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
                            self.value_at_type(item, set_item_ty)
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
                                self.value_at_type(key, self.type_id(Type::String)?)?,
                                self.erase(entry_value)?
                            ))
                        })
                        .collect::<Result<Vec<_>, EmitError>>()?
                        .join(", ");
                    return Ok(self.erase_object_text(&entries_text));
                }
                let dict_types = match self.mir.types.get(dest_ty) {
                    Some(Type::Dict(key_ty, value_ty)) => Some((*key_ty, *value_ty)),
                    _ => None,
                };
                let entries_text = entries
                    .iter()
                    .map(|(key, entry_value)| {
                        let key_text = if let Some((key_ty, _)) = dict_types {
                            self.value_at_type(key, key_ty)?
                        } else {
                            self.operand_text(key)?
                        };
                        let value_text = if let Some((_, value_ty)) = dict_types {
                            if matches!(self.mir.types.get(value_ty), Some(Type::Function(_))) {
                                format!(
                                    "{{ let smelt_fn: {} = {}; smelt_fn }}",
                                    self.type_text_with_impl_trait(value_ty, false)?,
                                    self.value_at_type(entry_value, value_ty)?
                                )
                            } else {
                                self.value_at_type(entry_value, value_ty)?
                            }
                        } else {
                            self.operand_text(entry_value)?
                        };
                        Ok(format!("({key_text}, {value_text})"))
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?
                    .join(", ");
                if let Some((key_ty, _)) = dict_types {
                    if self.dict_uses_smelt_record(key_ty) {
                        return Ok(format!("SmeltRecord::from([{entries_text}])"));
                    }
                    if self.dict_uses_js_key_map(key_ty) {
                        return Ok(format!("SmeltJsMap::from([{entries_text}])"));
                    }
                }
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
                            self.value_at_type(item, *target_item)
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
                if let Some(text) = self.strict_identity_text(*op, lhs, rhs)? {
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
                        self.value_at_type(lhs, dest_ty)?,
                        smelt_hir::bin_op_text(*op),
                        self.value_at_type(rhs, dest_ty)?
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
                if matches!(
                    *op,
                    smelt_hir::BinOp::BitAnd | smelt_hir::BinOp::BitOr | smelt_hir::BinOp::BitXor
                ) {
                    // JavaScript bitwise `&`, `|`, `^` coerce both operands with
                    // `ToInt32` (truncate to integer, take modulo 2^32 as a signed
                    // 32-bit value), operate, and yield a signed 32-bit number.
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
                        "({{ let smelt_bit_lhs = {{ let smelt_bit_v = {lhs_trunc_text}; if smelt_bit_v.is_finite() {{ smelt_bit_v.trunc().rem_euclid(4294967296.0) as u32 as i32 }} else {{ 0_i32 }} }}; let smelt_bit_rhs = {{ let smelt_bit_v = {rhs_trunc_text}; if smelt_bit_v.is_finite() {{ smelt_bit_v.trunc().rem_euclid(4294967296.0) as u32 as i32 }} else {{ 0_i32 }} }}; (smelt_bit_lhs {op_text} smelt_bit_rhs) as {result_cast} }})"
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
                        let comparison = format!("{} == {}", self.erase(lhs)?, self.erase(rhs)?);
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
                smelt_hir::UnaryOp::Neg => {
                    let operand_ty = self.operand_ty(operand)?;
                    let numeric_text = if matches!(
                        self.mir.types.get(operand_ty),
                        Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. } | Type::Never)
                    ) {
                        let float_ty = self.type_id(Type::Float)?;
                        self.primitive_cast_text(
                            smelt_hir::PrimitiveCastOp::ToFloat,
                            operand,
                            float_ty,
                        )?
                    } else {
                        self.operand_text(operand)?
                    };
                    if self.mir.types.get(dest_ty) == Some(&Type::Unknown) {
                        self.erase_value_text(
                            &format!("-({numeric_text})"),
                            self.type_id(Type::Float)?,
                        )
                    } else {
                        Ok(format!("-{numeric_text}"))
                    }
                }
            },
            Rvalue::Conditional {
                cond,
                then_operand,
                else_operand,
            } => Ok(format!(
                "if {} {{ {} }} else {{ {} }}",
                self.operand_text(cond)?,
                self.value_at_type(then_operand, dest_ty)?,
                self.value_at_type(else_operand, dest_ty)?
            )),
            Rvalue::FunctionTableLookup { key, cases } => {
                self.function_table_lookup_text(key, cases, dest_ty)
            }
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
            } => self.tag_check(unknown_value, *kind),
            Rvalue::TypeofValue {
                value: unknown_value,
            } => self.typeof_value_text(unknown_value),
            Rvalue::PrototypeSentinel {
                value: unknown_value,
            } => self.prototype_sentinel_text(unknown_value),
            Rvalue::UnknownCast {
                value: unknown_value,
                target,
            } => {
                let source = self.operand_ty(unknown_value)?;
                let value_text = self.operand_text(unknown_value)?;
                if let Some(projected) =
                    self.project_union_value_text(&value_text, source, *target)?
                {
                    return self.value_at_type_text(&projected, *target, dest_ty);
                }
                let target_rust_ty = self.type_text_with_impl_trait(*target, false)?;
                let cast_text = if target_rust_ty == "SmeltUnknown" {
                    self.erase(unknown_value)?
                } else {
                    self.extract(unknown_value, *target)?
                };
                self.value_at_type_text(&cast_text, *target, dest_ty)
            }
            Rvalue::Struct { class, fields } => {
                let class_name = sanitize_ident(self.symbol_name(*class)?);
                let mir_class = self
                    .mir
                    .classes
                    .iter()
                    .find(|item| item.name == *class)
                    .ok_or_else(|| EmitError::new("struct rvalue references an unknown class"))?;
                let effective_fields = self
                    .structural_record_fields(dest_ty)
                    .unwrap_or_else(|| crate::classes::effective_class_fields(self.mir, mir_class));
                let scoped_type_params = mir_class
                    .type_params
                    .iter()
                    .map(|param| param.name)
                    .collect::<HashSet<_>>();
                let mut parts = Vec::new();
                for field in effective_fields {
                    let name = sanitize_ident(self.symbol_name(field.name)?);
                    if let Some((_, field_value)) = fields
                        .iter()
                        .find(|(field_name, _)| *field_name == field.name)
                    {
                        parts.push(format!("{name}: {}", self.operand_text(field_value)?));
                    } else {
                        parts.push(format!(
                            "{name}: {}",
                            self.default_value_with_scoped_type_params(
                                field.ty,
                                &scoped_type_params,
                            )?
                        ));
                    }
                }
                if !mir_class.type_params.is_empty() {
                    parts.push("_smelt_phantom: ::std::marker::PhantomData".to_owned());
                }
                Ok(format!("{class_name} {{ {} }}", parts.join(", ")))
            }
            Rvalue::ExternalClassInstance { class, args } => {
                let text = self.external_class_instance_text(*class, args)?;
                if self.is_regexp_class_symbol(*class)?
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
            Rvalue::NumericToFixed { operand, digits } => {
                self.numeric_to_fixed_text(operand, digits)
            }
            Rvalue::ParseIntRadix { operand, radix } => self.parse_int_radix_text(operand, radix),
            Rvalue::PrimitiveCast { op, operand } => {
                self.primitive_cast_text(*op, operand, dest_ty)
            }
            Rvalue::StringCase { op, operand } => self.string_case_text(*op, operand, dest_ty),
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
                from_index,
            } => self.string_search_text(*op, haystack, needle, from_index.as_ref(), dest_ty),
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
            Rvalue::RegexFind { pattern, haystack } => {
                let text = self.regex_find_text(pattern, haystack)?;
                let string_ty = self.type_id(Type::String)?;
                let list_ty = self.type_id(Type::List(string_ty))?;
                let source_ty = self.type_id(Type::Optional(list_ty))?;
                self.value_at_type_text(&text, source_ty, dest_ty)
            }
            Rvalue::RegexExec { regex, haystack } => {
                self.regex_exec_text(regex, haystack, dest_ty)
            }
            Rvalue::RegexMatchAll { regex, haystack } => {
                self.regex_match_all_text(regex, haystack, dest_ty)
            }
            Rvalue::StringCharAt { operand, index } => self.string_char_at_text(operand, index),
            Rvalue::StringCharCodeAt { operand, index } => {
                self.string_char_code_at_text(operand, index)
            }
            Rvalue::StringContains {
                haystack,
                needle,
                from_index,
            } => self.string_contains_text(haystack, needle, from_index.as_ref()),
            Rvalue::StringSlice {
                operand,
                start,
                end,
            } => self.string_slice_text(operand, start.as_ref(), end.as_ref(), dest_ty),
            Rvalue::ListContains { list, item } => {
                let text = self.list_contains_text(list, item)?;
                let bool_ty = self.type_id(Type::Bool)?;
                self.value_at_type_text(&text, bool_ty, dest_ty)
            }
            Rvalue::SetContains { set, item } => {
                let text = self.set_contains_text(set, item)?;
                let bool_ty = self.type_id(Type::Bool)?;
                self.value_at_type_text(&text, bool_ty, dest_ty)
            }
            Rvalue::SetDisjoint { left, right } => {
                let text = self.set_disjoint_text(left, right)?;
                let bool_ty = self.type_id(Type::Bool)?;
                self.value_at_type_text(&text, bool_ty, dest_ty)
            }
            Rvalue::SetRelation { op, left, right } => {
                let text = self.set_relation_text(*op, left, right)?;
                let bool_ty = self.type_id(Type::Bool)?;
                self.value_at_type_text(&text, bool_ty, dest_ty)
            }
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
            Rvalue::ListConcat { left, right } => {
                let text = self.list_concat_text(left, right)?;
                let source_ty = self.concat_result_list_ty(left, right)?;
                self.value_at_type_text(&text, source_ty, dest_ty)
            }
            Rvalue::ListSearch {
                op,
                list,
                item,
                from_index,
            } => {
                let text = self.list_search_text(*op, list, item, from_index.as_ref())?;
                let float_ty = self.type_id(Type::Float)?;
                self.value_at_type_text(&text, float_ty, dest_ty)
            }
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
                    && (closure.escapes
                        || !matches!(self.mir.types.get(dest_ty), Some(Type::Function(_))))
                {
                    return self.default_value(dest_ty);
                }
                // Emit the closure value directly at its destination type.
                //
                // A bare function-item-as-value wrapper still carries a stable
                // `function_item_key`, but reference identity is now preserved at
                // the ERASE site (see `coercion::erase`), which routes erased
                // function-item references through a per-item accessor. Here, in
                // typed context, identity does not matter, so this arm emits the
                // plain fresh closure regardless of `function_item_key`.
                Ok(self.closure_text_for_type(*id, dest_ty)?)
            }
            Rvalue::ClosureCall { callee, args } => {
                let callee_ty = self.operand_ty(callee)?;
                if matches!(
                    self.mir.types.get(callee_ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(callee_ty)
                {
                    let callee_text = self.operand_text(callee)?;
                    let rendered_args =
                        args.iter()
                            .map(|arg| self.erase(arg))
                            .collect::<Result<Vec<_>, EmitError>>()?;
                    // A `ClosureCall` carries the exact argument list; spread calls
                    // are represented separately as `ClosureCallSpread`, which packs
                    // and flattens the runtime vector. Pass these arguments verbatim
                    // so a single array argument is delivered as one value instead of
                    // being mistaken for a spread and flattened into its elements.
                    let smelt_call_args = format!("vec![{}]", rendered_args.join(", "));
                    let call_text =
                        self.dynamic_callable_dispatch_text(&callee_text, &smelt_call_args);
                    let unknown_ty = self.type_id(Type::Unknown)?;
                    if matches!(self.mir.types.get(dest_ty), Some(Type::Function(_))) {
                        return Ok(call_text);
                    }
                    return self.value_at_type_text(&call_text, unknown_ty, dest_ty);
                }
                if !matches!(self.mir.types.get(callee_ty), Some(Type::Function(_))) {
                    return self.default_value(dest_ty);
                }
                let callee_text = self.operand_text(callee)?;
                if callee_text == "()" {
                    return self.default_value(dest_ty);
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
                        .enumerate()
                        .map(|(index, (arg, param))| {
                            let text = if rest_function.is_some_and(|function| {
                                function.mutable_params.contains(&index)
                            }) {
                                self.mutable_reference_argument_text(arg, *param)?
                            } else {
                                self.value_at_type(arg, *param)?
                            };
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
                let callee_is_erased_rest = matches!(
                    self.mir.types.get(callee_ty),
                    Some(Type::Function(function))
                        if self.is_erased_unknown_rest_function(function) && !function.may_throw
                );
                let call_text = match callee {
                    _ if callee_is_erased_rest => {
                        format!("{callee_text}.call({args_text})")
                    }
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
                    _ => format!("({callee_text})({args_text})"),
                };
                let (source_ty, rendered_call_text) = match self.mir.types.get(callee_ty) {
                    Some(Type::Function(function)) => {
                        let returns_future = matches!(
                            self.mir.types.get(function.return_ty),
                            Some(Type::Future(_))
                        );
                        let throwing_call_text = if function.may_throw && !returns_future {
                            format!("{call_text}.unwrap_or_else(|error| panic!(\"{{}}\", error))")
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
                if callee_is_erased_rest && self.mir.types.get(dest_ty) == Some(&Type::None) {
                    return Ok(format!("{{ {rendered_call_text}; () }}"));
                }
                self.value_at_type_text(&rendered_call_text, source_ty, dest_ty)
            }
            Rvalue::ClosureCallSpread { callee, args } => {
                let callee_text = self.operand_text(callee)?;
                let unknown_ty = self.type_id(Type::Unknown)?;
                let args_ty = self.type_id(Type::List(unknown_ty))?;
                let args_text = self
                    .inline_list_concat_operand_text(args)?
                    .unwrap_or(self.value_at_type(args, args_ty)?);
                if let Some(Type::Function(function)) = self.mir.types.get(self.operand_ty(callee)?)
                {
                    let callee_is_erased_rest =
                        self.is_erased_unknown_rest_function(function) && !function.may_throw;
                    // A typed callee with leading positional parameters before its
                    // rest cannot receive the packed spread list as a single
                    // argument; redistribute the list into `(positionals…,
                    // rest_list)` read from an in-scope `smelt_spread_args`.
                    let split_args = if callee_is_erased_rest {
                        None
                    } else {
                        self.spread_leading_positional_call_args_text(function)?
                    };
                    // The argument list handed to the callee: either the split
                    // positional+rest text (reads `smelt_spread_args`) or the
                    // packed list verbatim.
                    let inner_args = split_args.as_deref().unwrap_or(&args_text);
                    let inner_call = match callee {
                        _ if callee_is_erased_rest => {
                            format!("{callee_text}.call({inner_args})")
                        }
                        Operand::Copy(place) | Operand::Move(place)
                            if self.is_function_parameter_place(place)? =>
                        {
                            format!("{callee_text}({inner_args})")
                        }
                        _ if self.is_function_parameter_name(&callee_text)? => {
                            format!("{callee_text}({inner_args})")
                        }
                        _ if self.is_borrowed_callback_capture_name(&callee_text) => {
                            format!("{callee_text}({inner_args})")
                        }
                        _ => format!("({callee_text})({inner_args})"),
                    };
                    // When the arguments were split, bind the packed list once so
                    // the positional/rest reads above resolve; otherwise the call
                    // stands alone.
                    let call_text = if split_args.is_some() {
                        format!("{{ let smelt_spread_args = {args_text}; {inner_call} }}")
                    } else {
                        inner_call
                    };
                    if callee_is_erased_rest && self.mir.types.get(dest_ty) == Some(&Type::None) {
                        return Ok(format!("{{ {call_text}; () }}"));
                    }
                    return self.value_at_type_text(&call_text, function.return_ty, dest_ty);
                }
                let call_text = self.dynamic_callable_dispatch_text(&callee_text, &args_text);
                if matches!(self.mir.types.get(dest_ty), Some(Type::Function(_))) {
                    return Ok(call_text);
                }
                self.value_at_type_text(&call_text, unknown_ty, dest_ty)
            }
            Rvalue::ListCallback { op, list, callback } => {
                self.list_callback_text(*op, list, callback, dest_ty)
            }
            Rvalue::ListFromLength { length } => self.list_from_length_text(length, dest_ty),
            Rvalue::ListRepeat {
                value: repeat_value,
                count,
            } => self.list_repeat_text(repeat_value, count, dest_ty),
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
            Rvalue::ListFlat { list, depth } => self.list_flat_text(list, depth.as_ref(), dest_ty),
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
            Rvalue::ListSorted { list, key, reverse } => {
                self.list_sorted_text(list, key.as_ref(), *reverse, dest_ty)
            }
            Rvalue::ListReversed { list } => self.list_reversed_text(list, dest_ty),
            Rvalue::ListEnumerate { list } => self.list_enumerate_text(list, dest_ty),
            Rvalue::ListZip { left, right } => self.list_zip_text(left, right, dest_ty),
            Rvalue::ListRange { start, end, step } => {
                self.list_range_text(start, end, step, dest_ty)
            }
            Rvalue::ListRandomChoice { list } => self.list_random_choice_text(list, dest_ty),
            Rvalue::ListIndex { list, item } => self.list_index_text(list, item, dest_ty),
            Rvalue::ListRemove { list, item } => self.list_remove_text(list, item, dest_ty),
            Rvalue::ListSort {
                list,
                comparator,
                key,
                reverse,
            } => self.list_sort_text(list, comparator.as_ref(), key.as_ref(), *reverse, dest_ty),
            Rvalue::ListPop { list } => self.list_pop_text(list, dest_ty),
            Rvalue::ListShift { list } => self.list_shift_text(list, dest_ty),
            Rvalue::ListNext { list } => self.list_next_text(list, dest_ty),
            Rvalue::IteratorDone { result } => {
                Ok(format!("{}.is_none()", self.operand_text(result)?))
            }
            Rvalue::IteratorValue { result } => {
                let text = self.operand_text(result)?;
                if self.mir.types.get(dest_ty) == Some(&Type::Unknown) {
                    self.erase_value_text(&text, self.operand_ty(result)?)
                } else {
                    self.value_at_type(result, dest_ty)
                }
            }
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
            } => self.string_split_text(haystack, separator, limit.as_ref(), dest_ty),
            Rvalue::StringChars { haystack } => self.string_chars_text(haystack, dest_ty),
            Rvalue::StringJoin { items, separator } => self.string_join_text(items, separator),
            Rvalue::JsonStringify { value: json_value } => {
                self.json_stringify_text(json_value, dest_ty)
            }
            Rvalue::JsonParse { text } => self.json_parse_text(text, dest_ty),
            Rvalue::HttpGetText { url } => self.http_get_text(url),
            Rvalue::DateNow => {
                let text = "SMELT_DATE_NOW.with(::std::cell::Cell::get).unwrap_or_else(|| chrono::Utc::now().timestamp_millis())";
                self.date_timestamp_result_text(text, dest_ty)
            }
            Rvalue::DateSetNow { timestamp } => Ok(format!(
                "{{ SMELT_DATE_NOW.with(|value| value.set(Some(({}) as i64))); {} }}",
                self.date_timestamp_text(timestamp)?,
                self.default_value(dest_ty)?
            )),
            Rvalue::DateResetNow => Ok(format!(
                "{{ SMELT_DATE_NOW.with(|value| value.set(None)); {} }}",
                self.default_value(dest_ty)?
            )),
            Rvalue::DateTimezoneOffset => {
                Ok("SMELT_DATE_TIMEZONE_OFFSET.with(::std::cell::Cell::get)".to_owned())
            }
            Rvalue::DateSetTimezoneOffset { offset } => Ok(format!(
                "{{ SMELT_DATE_TIMEZONE_OFFSET.with(|value| value.set({})); {} }}",
                self.value_at_type(offset, self.type_id(Type::Float)?)?,
                self.default_value(dest_ty)?
            )),
            Rvalue::DateResetTimezoneOffset => Ok(format!(
                "{{ SMELT_DATE_TIMEZONE_OFFSET.with(|value| value.set(0.0)); {} }}",
                self.default_value(dest_ty)?
            )),
            Rvalue::DateTimezoneContext { timezone } => Ok(format!(
                "{{ let smelt_timezone_name = {}; let smelt_timezone: chrono_tz::Tz = smelt_timezone_name.parse().expect(\"invalid IANA time zone\"); ::std::rc::Rc::new(move |value: SmeltUnknown| -> SmeltUnknown {{ let timestamp_ms = match value {{ SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") {{ Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }}, SmeltUnknown::String(value) => chrono::DateTime::parse_from_rfc3339(&value).map(|date| date.timestamp_millis() as f64).unwrap_or_else(|_| value.parse::<f64>().unwrap_or(f64::NAN)), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }}; let local_timestamp_ms = if timestamp_ms.is_finite() {{ chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp_ms as i64).map_or(f64::NAN, |date| date.with_timezone(&smelt_timezone).naive_local().and_utc().timestamp_millis() as f64) }} else {{ f64::NAN }}; SmeltUnknown::Object(SmeltObject::new(::std::collections::HashMap::from([(\"__smelt_date\".to_owned(), SmeltUnknown::Number(local_timestamp_ms)), (\"__smelt_timezone\".to_owned(), SmeltUnknown::String(smelt_timezone_name.clone()))]))) }}) }}",
                self.operand_text(timezone)?
            )),
            Rvalue::DateToIsoString { timestamp_ms } => self.date_to_iso_string_text(timestamp_ms),
            Rvalue::DateToString { timestamp_ms } => self.date_to_string_text(timestamp_ms),
            Rvalue::DateFromParts { parts } => {
                let text = self.date_from_parts_text(parts)?;
                self.date_timestamp_result_text(&text, dest_ty)
            }
            Rvalue::DateFromValue { value: date_value } => {
                let text = self.date_timestamp_text(date_value)?;
                self.date_timestamp_result_preserving_receiver_text(&text, dest_ty, date_value)
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
                self.date_timestamp_result_preserving_receiver_text(&text, dest_ty, timestamp_ms)
            }
            Rvalue::UrlField { field, url } => self.url_field_text(*field, url),
            Rvalue::FileReadText { path } => self.file_read_text(path),
            Rvalue::FileWriteText { path, text } => self.file_write_text(path, text),
            Rvalue::Await(operand) => {
                if self.mir.types.get(self.operand_ty(operand)?) == Some(&Type::None) {
                    return self.default_value(dest_ty);
                }
                Ok(format!("{}.await?", self.await_operand_text(operand)?))
            }
            Rvalue::AsyncOp { op, args } => {
                let text = self.async_op_text(*op, args, dest_ty)?;
                if matches!(op, smelt_hir::AsyncOp::Sleep)
                    && let Some(Type::Future(item)) = self.mir.types.get(dest_ty)
                    && self.mir.types.get(*item) != Some(&Type::None)
                {
                    return Ok(format!(
                        "Box::pin(async move {{ {text}.await?; Ok::<_, Box<dyn std::error::Error>>({}) }}) as {}",
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

        if matches!(
            op,
            smelt_hir::BinOp::Eq
                | smelt_hir::BinOp::NotEq
                | smelt_hir::BinOp::StrictEq
                | smelt_hir::BinOp::StrictNotEq
                | smelt_hir::BinOp::JsStrictEq
                | smelt_hir::BinOp::JsStrictNotEq
        ) {
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
                self.value_at_type(rhs, common_ty)?
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
                self.value_at_type(lhs, common_ty)?,
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
        if !matches!(
            op,
            smelt_hir::BinOp::Eq
                | smelt_hir::BinOp::NotEq
                | smelt_hir::BinOp::StrictEq
                | smelt_hir::BinOp::StrictNotEq
                | smelt_hir::BinOp::JsStrictEq
                | smelt_hir::BinOp::JsStrictNotEq
        ) {
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
        let strict = matches!(
            op,
            smelt_hir::BinOp::StrictEq | smelt_hir::BinOp::StrictNotEq
        );
        let js_strict = matches!(
            op,
            smelt_hir::BinOp::JsStrictEq | smelt_hir::BinOp::JsStrictNotEq
        );
        let strict_nullish = strict || js_strict;
        // Pick the erased nullish tag for a `None`-typed literal: the `undefined`
        // literal matches `Undefined`, every other `None`-typed operand (the
        // `null` literal or a unit value) matches `Null`. Used by the
        // erased-vs-none arms where the none side is essentially always a literal.
        let nullish_pattern = |operand: &Operand| {
            if strict_nullish {
                if matches!(operand, Operand::Const(Constant::Undefined)) {
                    "SmeltUnknown::Undefined"
                } else {
                    "SmeltUnknown::Null"
                }
            } else {
                "SmeltUnknown::Null | SmeltUnknown::Undefined"
            }
        };
        // `null` and `undefined` are byte-identical in MIR (both `Type::None`), so
        // the JS nullish kind can only be proven from an explicit literal:
        // `Constant::None` is `null`, `Constant::Undefined` is `undefined`. Two
        // `None`-typed operands therefore strict-differ ONLY when one is provably
        // the `null` literal and the other provably the `undefined` literal. In
        // every other case — including a unit temporary produced by an
        // `undefined`-valued expression such as `clone(undefined)` compared
        // against the `undefined` literal — they share the same nullish value and
        // strict-equal. (The old code keyed on "is undefined?", so a non-literal
        // unit actual wrongly differed from the `undefined` literal.)
        let provably_null =
            |operand: &Operand| matches!(operand, Operand::Const(Constant::None));
        let provably_undefined =
            |operand: &Operand| matches!(operand, Operand::Const(Constant::Undefined));
        let text = if lhs_is_none && rhs_is_none {
            if strict_nullish {
                let distinct = (provably_null(lhs) && provably_undefined(rhs))
                    || (provably_undefined(lhs) && provably_null(rhs));
                (!distinct).to_string()
            } else {
                "true".to_owned()
            }
        } else if lhs_is_erased && rhs_is_none {
            format!(
                "matches!({}, {})",
                self.operand_text(lhs)?,
                nullish_pattern(rhs)
            )
        } else if rhs_is_erased && lhs_is_none {
            format!(
                "matches!({}, {})",
                self.operand_text(rhs)?,
                nullish_pattern(lhs)
            )
        } else if lhs_is_none || rhs_is_none {
            "false".to_owned()
        } else if lhs_is_erased || rhs_is_erased {
            let lhs_text = if lhs_is_erased {
                self.operand_text(lhs)?
            } else {
                self.erase(lhs)?
            };
            let rhs_text = if rhs_is_erased {
                self.operand_text(rhs)?
            } else {
                self.erase(rhs)?
            };
            if js_strict {
                // JavaScript `===`/`!==` on erased values: reference identity for
                // objects/arrays/functions, value for primitives, NaN-unequal.
                format!("{lhs_text}.js_strict_eq(&{rhs_text})")
            } else if strict {
                // `Object.is` / `a === b || Object.is(a,b)` SameValueZero idiom:
                // NaN-equal, reference objects.
                format!("{lhs_text}.same_js_key(&{rhs_text})")
            } else {
                // Loose/structural `==`/`!=` on erased values: SmeltUnknown's
                // structural `Eq` (deep), which the `toEqual`/`toStrictEqual`
                // matchers and `isDeepEqual` depend on.
                format!("{lhs_text} == {rhs_text}")
            }
        } else {
            return Ok(None);
        };
        Ok(Some(
            if matches!(
                op,
                smelt_hir::BinOp::NotEq
                    | smelt_hir::BinOp::StrictNotEq
                    | smelt_hir::BinOp::JsStrictNotEq
            ) {
                format!("!({text})")
            } else {
                text
            },
        ))
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
        let negate = matches!(
            op,
            smelt_hir::BinOp::NotEq
                | smelt_hir::BinOp::StrictNotEq
                | smelt_hir::BinOp::JsStrictNotEq
        );
        let strict = matches!(
            op,
            smelt_hir::BinOp::StrictEq | smelt_hir::BinOp::StrictNotEq
        );
        let js_strict = matches!(
            op,
            smelt_hir::BinOp::JsStrictEq | smelt_hir::BinOp::JsStrictNotEq
        );
        let strict_nullish = strict || js_strict;
        if strict
            && let (Some(left_inner), Some(right_inner)) = (lhs_inner, rhs_inner)
            && let (Some(Type::Dict(left_key, _)), Some(Type::Dict(right_key, _))) = (
                self.mir.types.get(left_inner),
                self.mir.types.get(right_inner),
            )
            && self.mir.types.get(*left_key) == Some(&Type::String)
            && self.mir.types.get(*right_key) == Some(&Type::String)
        {
            let text = format!(
                "match ({}.as_ref(), {}.as_ref()) {{ (Some(left), Some(right)) => left.id == right.id, (None, None) => true, _ => false }}",
                self.operand_text(lhs)?,
                self.operand_text(rhs)?
            );
            return Ok(Some(if negate { format!("!({text})") } else { text }));
        }
        let text = if let Some(inner) = lhs_inner
            && self.operand_ty(rhs)? == self.none_ty
        {
            let lhs_text = self.operand_text(lhs)?;
            if self.optional_inner_preserves_erased_singletons(inner) {
                self.optional_erased_singleton_equality_text(&lhs_text, rhs, strict_nullish, inner)?
            } else {
                format!("{lhs_text}.is_none()")
            }
        } else if let Some(inner) = rhs_inner
            && self.operand_ty(lhs)? == self.none_ty
        {
            let rhs_text = self.operand_text(rhs)?;
            if self.optional_inner_preserves_erased_singletons(inner) {
                self.optional_erased_singleton_equality_text(&rhs_text, lhs, strict_nullish, inner)?
            } else {
                format!("{rhs_text}.is_none()")
            }
        } else if let Some(inner) = lhs_inner
            && rhs_inner.is_none()
        {
            format!(
                "{} == Some({})",
                self.operand_text(lhs)?,
                self.value_at_type(rhs, inner)?
            )
        } else if let Some(inner) = rhs_inner
            && lhs_inner.is_none()
        {
            format!(
                "Some({}) == {}",
                self.value_at_type(lhs, inner)?,
                self.operand_text(rhs)?
            )
        } else {
            return Ok(None);
        };
        Ok(Some(if negate { format!("!({text})") } else { text }))
    }

    /// Emits equality between `Option<SmeltUnknown>`-like storage and a JS
    /// nullish singleton.
    fn optional_erased_singleton_equality_text(
        &self,
        option_text: &str,
        singleton: &Operand,
        strict_nullish: bool,
        inner: TypeId,
    ) -> Result<String, EmitError> {
        let pattern = if strict_nullish {
            if matches!(singleton, Operand::Const(Constant::Undefined)) {
                "SmeltUnknown::Undefined"
            } else {
                "SmeltUnknown::Null"
            }
        } else {
            "SmeltUnknown::Null | SmeltUnknown::Undefined"
        };
        // A concrete-union `Option` payload stores a tagged enum; project each
        // present value to `SmeltUnknown` before the nullish tag match. A present
        // union value never holds `null`/`undefined` (those are the `None`), so
        // this preserves the exact loose/strict comparison semantics.
        let scrutinee = if self.concrete_union_members(inner).is_some() {
            "value.clone().into_smelt_unknown()"
        } else {
            "value"
        };
        let missing_matches =
            !strict_nullish || matches!(singleton, Operand::Const(Constant::Undefined));
        if missing_matches {
            Ok(format!(
                "{option_text}.as_ref().map_or(true, |value| matches!({scrutinee}, {pattern}))"
            ))
        } else {
            Ok(format!(
                "{option_text}.as_ref().is_some_and(|value| matches!({scrutinee}, {pattern}))"
            ))
        }
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
                | smelt_hir::BinOp::JsStrictEq
                | smelt_hir::BinOp::JsStrictNotEq
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
        let lhs_text = self.operand_text(lhs)?;
        let rhs_text = self.operand_text(rhs)?;
        let mut common_ty = self.common_numeric_type(lhs_ty, rhs_ty, dest_ty)?;
        if matches!(self.mir.types.get(common_ty), Some(Type::Int))
            && (lhs_text.contains(" as f64") || rhs_text.contains(" as f64"))
        {
            common_ty = self.type_id(Type::Float)?;
        }
        Ok(Some(format!(
            "{} {} {}",
            self.value_at_type_text(&lhs_text, lhs_ty, common_ty)?,
            smelt_hir::bin_op_text(op),
            self.value_at_type_text(&rhs_text, rhs_ty, common_ty)?
        )))
    }

    /// Emits equality for first-class callback values.
    ///
    /// Stored callbacks lower to `Rc<dyn Fn...>`, which has no
    /// structural equality. JavaScript compares function values by identity, so
    /// matching callback shapes can use `Rc::ptr_eq`; mismatched shapes are
    /// unequal and compile to a constant.
    fn function_equality_text(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
    ) -> Result<Option<String>, EmitError> {
        if !matches!(
            op,
            smelt_hir::BinOp::Eq
                | smelt_hir::BinOp::NotEq
                | smelt_hir::BinOp::StrictEq
                | smelt_hir::BinOp::StrictNotEq
                | smelt_hir::BinOp::JsStrictEq
                | smelt_hir::BinOp::JsStrictNotEq
        ) {
            return Ok(None);
        }
        let lhs_ty = self.operand_ty(lhs)?;
        let rhs_ty = self.operand_ty(rhs)?;
        let lhs_contains_function = self.type_contains_function(lhs_ty);
        let rhs_contains_function = self.type_contains_function(rhs_ty);
        if !lhs_contains_function && !rhs_contains_function {
            return Ok(None);
        }
        let equal_text = if lhs_ty == rhs_ty && lhs_contains_function && rhs_contains_function {
            self.function_bearing_equality_text(
                &self.operand_text(lhs)?,
                &self.operand_text(rhs)?,
                lhs_ty,
            )?
        } else {
            "false".to_owned()
        };
        Ok(Some(
            if matches!(
                op,
                smelt_hir::BinOp::NotEq
                    | smelt_hir::BinOp::StrictNotEq
                    | smelt_hir::BinOp::JsStrictNotEq
            ) {
                format!("!({equal_text})")
            } else {
                equal_text
            },
        ))
    }

    /// Emits structural equality recursively while comparing function leaves by identity.
    fn function_bearing_equality_text(
        &self,
        left: &str,
        right: &str,
        ty: TypeId,
    ) -> Result<String, EmitError> {
        Ok(match self.mir.types.get(ty) {
            Some(Type::Function(_)) => {
                format!("::std::rc::Rc::ptr_eq(&{left}, &{right})")
            }
            Some(Type::List(item)) => {
                let item_equal =
                    self.function_bearing_equality_text("left_item", "right_item", *item)?;
                format!(
                    "{left}.len() == {right}.len() && {left}.iter().zip({right}.iter()).all(|(left_item, right_item)| {item_equal})"
                )
            }
            Some(Type::Dict(key, value)) if self.mir.types.get(*key) == Some(&Type::String) => {
                let value_equal =
                    self.function_bearing_equality_text("left_value", "right_value", *value)?;
                format!(
                    "{left}.len() == {right}.len() && {left}.iter().all(|(key, left_value)| {right}.get(&key).is_some_and(|right_value| {value_equal}))"
                )
            }
            Some(Type::Optional(inner)) => {
                let item_equal =
                    self.function_bearing_equality_text("left_value", "right_value", *inner)?;
                format!(
                    "match ({left}.as_ref(), {right}.as_ref()) {{ (Some(left_value), Some(right_value)) => {item_equal}, (None, None) => true, _ => false }}"
                )
            }
            _ => format!("{left} == {right}"),
        })
    }

    /// Emits JavaScript SameValue checks for numeric and reference values.
    fn strict_identity_text(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
    ) -> Result<Option<String>, EmitError> {
        if !matches!(
            op,
            smelt_hir::BinOp::StrictEq | smelt_hir::BinOp::StrictNotEq
        ) {
            return Ok(None);
        }
        let lhs_ty = self.operand_ty(lhs)?;
        let rhs_ty = self.operand_ty(rhs)?;
        let equal_text = match (self.mir.types.get(lhs_ty), self.mir.types.get(rhs_ty)) {
            (Some(Type::Int | Type::Float), Some(Type::Int | Type::Float)) => {
                let lhs_text = self.float_operand_text(lhs)?;
                let rhs_text = self.float_operand_text(rhs)?;
                format!(
                    "{{ let lhs: f64 = {lhs_text}; let rhs: f64 = {rhs_text}; (lhs.is_nan() && rhs.is_nan()) || (lhs == rhs && (lhs != 0.0 || lhs.is_sign_negative() == rhs.is_sign_negative())) }}"
                )
            }
            (Some(Type::Dict(lhs_key, _)), Some(Type::Dict(rhs_key, _)))
                if self.mir.types.get(*lhs_key) == Some(&Type::String)
                    && self.mir.types.get(*rhs_key) == Some(&Type::String) =>
            {
                format!(
                    "{}.id == {}.id",
                    self.operand_text(lhs)?,
                    self.operand_text(rhs)?
                )
            }
            (Some(Type::Function(_)), Some(Type::Function(_))) if lhs_ty == rhs_ty => {
                format!(
                    "::std::rc::Rc::ptr_eq(&{}, &{})",
                    self.operand_text(lhs)?,
                    self.operand_text(rhs)?
                )
            }
            (Some(Type::List(_)), Some(Type::List(_))) => {
                // Typed lists are identity-bearing (`SmeltList`), so reference
                // equality compares the JS reference id rather than giving up.
                format!(
                    "{}.id() == {}.id()",
                    self.operand_text(lhs)?,
                    self.operand_text(rhs)?
                )
            }
            (
                Some(Type::List(_) | Type::Set(_) | Type::Tuple(_) | Type::Class { .. }),
                Some(Type::List(_) | Type::Set(_) | Type::Tuple(_) | Type::Class { .. }),
            ) => "false".to_owned(),
            (left, right)
                if matches!(
                    left,
                    Some(
                        Type::List(_)
                            | Type::Dict(_, _)
                            | Type::Set(_)
                            | Type::Tuple(_)
                            | Type::Class { .. }
                            | Type::Function(_)
                    )
                ) || matches!(
                    right,
                    Some(
                        Type::List(_)
                            | Type::Dict(_, _)
                            | Type::Set(_)
                            | Type::Tuple(_)
                            | Type::Class { .. }
                            | Type::Function(_)
                    )
                ) =>
            {
                "false".to_owned()
            }
            _ => return Ok(None),
        };
        Ok(Some(if op == smelt_hir::BinOp::StrictNotEq {
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
        if !matches!(
            op,
            smelt_hir::BinOp::Eq
                | smelt_hir::BinOp::NotEq
                | smelt_hir::BinOp::StrictEq
                | smelt_hir::BinOp::StrictNotEq
                | smelt_hir::BinOp::JsStrictEq
                | smelt_hir::BinOp::JsStrictNotEq
        ) {
            return Ok(None);
        }
        let lhs_ty = self.operand_ty(lhs)?;
        let rhs_ty = self.operand_ty(rhs)?;
        if lhs_ty == rhs_ty {
            return Ok(None);
        }
        if self.equality_shapes_are_definitely_incompatible(lhs_ty, rhs_ty) {
            let text = "false".to_owned();
            return Ok(Some(
                if matches!(
                    op,
                    smelt_hir::BinOp::NotEq
                        | smelt_hir::BinOp::StrictNotEq
                        | smelt_hir::BinOp::JsStrictNotEq
                ) {
                    format!("!({text})")
                } else {
                    text
                },
            ));
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
                self.value_at_type(rhs, lhs_ty)?
            )
        } else if rhs_needs_erased && !lhs_needs_erased {
            if matches!(self.mir.types.get(lhs_ty), Some(Type::Tuple(_)))
                && matches!(self.mir.types.get(rhs_ty), Some(Type::List(_)))
            {
                format!("{} == {}", self.erase(lhs)?, self.erase(rhs)?)
            } else {
                format!(
                    "{} == {}",
                    self.value_at_type(lhs, rhs_ty)?,
                    self.operand_text(rhs)?
                )
            }
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
            // A tuple (e.g. `splitAt`'s `[T[], T[]]`) and a list literal of the
            // same shape are structurally comparable once both are erased to
            // `SmeltUnknown::Array`; they are NOT definitely incompatible. Let the
            // erase-both equality path handle them instead of folding to `false`.
            (Some(Type::Tuple(_)), Some(Type::List(_)))
            | (Some(Type::List(_)), Some(Type::Tuple(_))) => false,
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
            (
                Some(Type::Class { .. }),
                Some(Type::Bool | Type::Int | Type::Float | Type::String | Type::None),
            )
            | (
                Some(Type::Bool | Type::Int | Type::Float | Type::String | Type::None),
                Some(Type::Class { .. }),
            ) => true,
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
        let lhs_text = self.value_at_type(lhs, float_ty)?;
        let rhs_text = self.value_at_type(rhs, float_ty)?;
        // The combined `lhs <op> rhs` is a binary expression: a loose operand
        // that must be parenthesized before it becomes a cast operand
        // (`(...) as f64`) or a method receiver (`(...).to_string()`). Carrying
        // its precedence in a `RenderedValue` lets the value wrap itself instead
        // of relying on each arm below to remember the parentheses by hand.
        let numeric = RenderedValue::with_precedence(
            format!("{lhs_text} {} {rhs_text}", smelt_hir::bin_op_text(op)),
            float_ty,
            Precedence::NeedsParens,
        );
        Ok(Some(match self.mir.types.get(dest_ty) {
            Some(Type::Int) => format!(
                "({} as f64).trunc() as i64",
                numeric.parenthesized_if_needed()
            ),
            Some(Type::Float) => numeric.into_text(),
            Some(Type::String) => {
                format!("{}.to_string()", numeric.parenthesized_if_needed())
            }
            // The erased target is already an `f64` value in argument position
            // (no reassociation); the coercion seam owns the `Number` tag text.
            _ => self.erase_f64_text(&numeric.into_text()),
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
        self.value_at_type_text(&value, inner, target)
    }

    /// Emits Rust for selecting a function value from a static string-keyed table.
    fn function_table_lookup_text(
        &self,
        key: &Operand,
        cases: &[(String, Operand)],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let key_text = self.operand_text(key)?;
        let cases_text = cases
            .iter()
            .map(|(case_key, case)| {
                let case_text = self.value_at_type(case, dest_ty)?;
                Ok(format!("{case_key:?} => {case_text}"))
            })
            .collect::<Result<Vec<_>, EmitError>>()?
            .join(", ");
        Ok(format!(
            "{{ let __smelt_function_key = {key_text}; match __smelt_function_key.as_str() {{ {cases_text}, _ => panic!(\"unknown function table key: {{}}\", __smelt_function_key) }} }}"
        ))
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
                let mapped = self.value_at_type_text(&value, field_ty, *dest_inner)?;
                return Ok(format!(
                    "{receiver_text}.as_ref().map(|_smelt_value| {mapped})"
                ));
            }
            let mapped = self.value_at_type_text(&value, field_ty, dest_ty)?;
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
                let mapped = self.value_at_type_text(&value, field_ty, *dest_inner)?;
                return Ok(format!("Some({mapped})"));
            }
            self.value_at_type_text(&value, field_ty, dest_ty)
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
            // The nullish `match` operates on the erased `SmeltUnknown` form. A
            // concrete-union operand stores a tagged enum, so both the scrutinee
            // and the fallback are rendered erased here and the tagged union is
            // reconstructed by the destination coercion below.
            let scrutinee_ty = if self.concrete_union_members(optional_ty).is_some() {
                self.type_id(Type::Unknown)?
            } else {
                optional_ty
            };
            let optional_text = self.value_at_type(optional, scrutinee_ty)?;
            let fallback_text = self.value_at_type(fallback, scrutinee_ty)?;
            let coalesced = format!(
                "match {optional_text} {{ SmeltUnknown::Null | SmeltUnknown::Undefined => {fallback_text}, value => value }}"
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
            // A concrete-union destination is not an erased boundary: coerce the
            // erased coalesced value into the tagged union (`from_smelt_unknown`)
            // rather than leaving it as `SmeltUnknown`.
            let dest_is_erased = (matches!(
                self.mir.types.get(dest_ty),
                Some(Type::Unknown | Type::TypeParam { .. })
            ) || matches!(
                self.mir.types.get(dest_ty),
                Some(Type::Union(_))
            ) && self.concrete_union_members(dest_ty).is_none())
                || self.is_erased_class_type(dest_ty);
            if !dest_is_erased {
                return self.value_at_type_text(&coalesced, scrutinee_ty, dest_ty);
            }
            return Ok(coalesced);
        }
        match self.mir.types.get(optional_ty) {
            Some(Type::Optional(inner)) => {
                if matches!(
                    self.mir.types.get(dest_ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(dest_ty)
                {
                    let optional_text = self.operand_text(optional)?;
                    let present_text = self.value_at_type_text("value", *inner, dest_ty)?;
                    let fallback_text = self.value_at_type(fallback, dest_ty)?;
                    return Ok(format!(
                        "{optional_text}.map_or_else(|| {fallback_text}, |value| {present_text})"
                    ));
                }
                let fallback_ty = self.operand_ty(fallback)?;
                if matches!(
                    self.mir.types.get(fallback_ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(fallback_ty)
                {
                    let optional_text = self.operand_text(optional)?;
                    let fallback_text = self.value_at_type(fallback, fallback_ty)?;
                    let mapped_value = self.value_at_type_text("value", fallback_ty, *inner)?;
                    let fallback_option = format!(
                        "match {fallback_text} {{ SmeltUnknown::Null | SmeltUnknown::Undefined => None, value => Some({mapped_value}) }}"
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
                    return self.value_at_type_text(&coalesced, *inner, dest_ty);
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
                    self.value_at_type(fallback, *inner)?
                );
                if dest_ty == *inner {
                    Ok(coalesced)
                } else {
                    self.value_at_type_text(&coalesced, *inner, dest_ty)
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
        let mapped = self.value_at_type_text("_smelt_inner", source_inner, dest_inner)?;
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
            .map(|arg| self.value_at_type(arg, *item))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        Ok(Some(vec![format!("SmeltList::from(vec![{items}])")]))
    }

    /// Render call-argument text for a spread across a leading-positional + rest
    /// signature, binding the packed list to `smelt_spread_args`.
    ///
    /// A JavaScript call like `callee(data, ...extraArgs)` lowers to a
    /// `ClosureCallSpread` whose runtime argument list is the concatenation
    /// `[data, ...extraArgs]`. When the typed callee declares leading positional
    /// parameters before its rest (`(data: T, ...rest: U[]) => …`, i.e. `rest`
    /// starts after index 0), the packed list must be redistributed: the first
    /// `rest` elements fill the positional parameters and the remainder becomes
    /// the rest `SmeltList`. Returns `None` when the callee has no leading
    /// positional before the rest (index-0 rest is handled by the plain spread
    /// path) or when the shape does not match `[positionals…, rest_list]`.
    ///
    /// The returned string is the comma-separated argument list that reads from
    /// an in-scope `smelt_spread_args` binding; the caller wraps the whole call
    /// in a block that first binds `smelt_spread_args` to the packed list.
    fn spread_leading_positional_call_args_text(
        &self,
        function: &FunctionType,
    ) -> Result<Option<String>, EmitError> {
        let Some(rest_index) = function.rest else {
            return Ok(None);
        };
        if rest_index == 0 {
            return Ok(None);
        }
        // The rest parameter is the last declared parameter; everything before it
        // is a leading positional the packed list must supply by index.
        let Some(expected_params_len) = rest_index.checked_add(1) else {
            return Ok(None);
        };
        if function.params.len() != expected_params_len {
            return Ok(None);
        }
        let Some((rest_param, positional_params)) = function.params.split_last() else {
            return Ok(None);
        };
        let Some(Type::List(rest_item)) = self.mir.types.get(*rest_param) else {
            return Ok(None);
        };
        let unknown_ty = self.type_id(Type::Unknown)?;
        let rendered_capacity = positional_params
            .len()
            .checked_add(1)
            .ok_or_else(|| EmitError::new("spread call argument count overflowed usize"))?;
        let mut rendered = Vec::with_capacity(rendered_capacity);
        for (index, param) in positional_params.iter().enumerate() {
            // Each positional reads the erased element at its index (absent
            // arguments become `undefined`, matching JS) and coerces to the
            // declared parameter type.
            let element = format!(
                "smelt_spread_args.get({index}).cloned().unwrap_or(SmeltUnknown::Undefined)"
            );
            rendered.push(self.value_at_type_text(&element, unknown_ty, *param)?);
        }
        // The rest parameter collects the remaining elements as a fresh
        // `SmeltList`; coerce each element to the rest item type when needed.
        let rest_text = if self.mir.types.get(*rest_item) == Some(&Type::Unknown) {
            format!(
                "SmeltList::from(smelt_spread_args.iter().skip({rest_index}).cloned().collect::<Vec<_>>())"
            )
        } else {
            let item_text = self.value_at_type_text("value", unknown_ty, *rest_item)?;
            format!(
                "SmeltList::from(smelt_spread_args.iter().skip({rest_index}).cloned().map(|value| {item_text}).collect::<Vec<_>>())"
            )
        };
        rendered.push(rest_text);
        Ok(Some(rendered.join(", ")))
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
        props: &[(Symbol, Operand)],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        if matches!(
            self.mir.types.get(dest_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(dest_ty)
        {
            let callable_text = self.erase(callable)?;
            let mut entries = vec![format!(
                "smelt_object.insert(\"__smelt_call\".to_owned(), {callable_text});"
            )];
            for (key, value) in props {
                let key_text = self.symbol_source_name(*key)?;
                let value_text = self.erase(value)?;
                entries.push(format!(
                    "smelt_object.insert({key_text:?}.to_owned(), {value_text});"
                ));
            }
            return Ok(format!(
                "{{ let mut smelt_object = ::std::collections::HashMap::new(); {} SmeltUnknown::Object(SmeltObject::new(smelt_object)) }}",
                entries.join(" ")
            ));
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
        if smelt_stdlib::typescript_stdlib_class(class_name)
            == Some(smelt_stdlib::StdlibClass::RegExp)
        {
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
            "{{ let _smelt_external_args = {args_tuple_text}; let mut _smelt_external = ::std::collections::HashMap::new(); _smelt_external.insert(\"__class\".to_owned(), SmeltUnknown::String({class_name:?}.to_owned())); SmeltUnknown::Object(SmeltObject::new(_smelt_external)) }}"
        ))
    }

    /// Emits a field read against a named in-scope receiver value.
    fn field_access_text(
        &self,
        receiver_text: &str,
        receiver_ty: TypeId,
        field: Symbol,
    ) -> Result<String, EmitError> {
        if let Some(Type::Dict(key, value)) = self.mir.types.get(receiver_ty)
            && self.mir.types.get(*key) == Some(&Type::String)
        {
            let field_name = self.symbol_source_name(field)?;
            if matches!(self.mir.types.get(*value), Some(Type::Optional(_))) {
                if self.dict_uses_smelt_record(*key) {
                    return Ok(format!("{receiver_text}.get({field_name:?}).flatten()"));
                }
                return Ok(format!(
                    "{receiver_text}.get({field_name:?}).cloned().flatten()"
                ));
            }
            if self.dict_uses_smelt_record(*key) {
                return Ok(format!(
                    "{receiver_text}.get({field_name:?}).expect(\"missing field\")"
                ));
            }
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
                "match {receiver_text} {{ SmeltUnknown::Object(map) => match map.get({field_name:?}).unwrap_or(SmeltUnknown::Null) {{ SmeltUnknown::Object(mut getter) if getter.contains_key(\"__smelt_get\") => match getter.remove(\"__smelt_get\") {{ Some(SmeltUnknown::Function(smelt_getter)) => (smelt_getter)(Vec::new()).unwrap_or_else(|error| panic!(\"{{}}\", error)), _ => SmeltUnknown::Null }}, value => value }}, _ => SmeltUnknown::Null }}"
            ));
        }
        if matches!(self.mir.types.get(receiver_ty), Some(Type::String)) {
            return self.string_field_text(receiver_text, field);
        }
        if let Some(Type::Class { name, .. }) = self.mir.types.get(receiver_ty)
            && self.is_regexp_class_symbol(*name)?
        {
            return self.regexp_field_text(receiver_text, field);
        }
        if self.storage_field_is_function(receiver_ty, field) {
            return Ok(format!(
                "{receiver_text}.{}.clone()",
                sanitize_ident(self.symbol_name(field)?)
            ));
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
                self.value_at_type_text(&item, unknown_ty, param_ty)
            })
            .collect::<Result<Vec<_>, EmitError>>()?;
        let call = format!("smelt_receiver.{method_name}({})", args.join(", "));
        let returned = if function.can_throw {
            format!("{call}?")
        } else {
            call
        };
        let wrapped_returned = self.erase_value_text(&returned, function.return_ty)?;
        let receiver_clone = if receiver_text == "self" {
            self.self_struct_clone_text(class)?
        } else {
            format!("{receiver_text}.clone()")
        };
        Ok(Some(format!(
            "{{ let smelt_receiver = {receiver_clone}; SmeltUnknown::Function(::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>({wrapped_returned}))) }}"
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
        Ok(Some("SmeltUnknown::Function(::std::rc::Rc::new(move |_smelt_args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>(SmeltUnknown::Null)))".to_owned()))
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
            "length" => format!("({receiver_text}.chars().count() as i64)"),
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
            "constructor" => {
                "SmeltUnknown::Object(SmeltObject::new(::std::collections::HashMap::from([])))"
                    .to_owned()
            }
            _ => "SmeltUnknown::Null".to_owned(),
        })
    }

    /// Emits a typed field read against a concrete `SmeltMatch` receiver.
    ///
    /// `kind` distinguishes the match value itself (`__SmeltMatch`) from its
    /// named-group accessor (`__SmeltMatchGroups`), which is the same underlying
    /// `SmeltMatch` value obtained through a `.groups` read:
    ///
    /// * On the match value: `index` -> `f64`, `length` -> `f64`, `input` ->
    ///   `String`, and `groups` yields the receiver itself (the named-group
    ///   accessor shares the `SmeltMatch` representation).
    /// * On the named-group accessor: every field is a named capture group,
    ///   read as `Option<String>` (JavaScript `undefined` when absent).
    ///
    /// These reads never build a `SmeltUnknown` property bag; the match value
    /// stays statically typed all the way to the primitive result.
    pub(super) fn match_field_text(
        &self,
        receiver_text: &str,
        kind: smelt_stdlib::StdlibClass,
        field: Symbol,
    ) -> Result<String, EmitError> {
        let field_name = self.symbol_source_name(field)?;
        match kind {
            smelt_stdlib::StdlibClass::MatchGroups => Ok(format!(
                "{receiver_text}.named_group_owned({field_name:?})"
            )),
            _ => Ok(match field_name {
                "index" => format!("{receiver_text}.index()"),
                "length" => format!("{receiver_text}.length()"),
                "input" => format!("{receiver_text}.input_owned()"),
                // `match.groups` is the same underlying `SmeltMatch` value; a
                // subsequent named-group read resolves through the
                // `MatchGroups` branch above.
                "groups" => format!("{receiver_text}.clone()"),
                _ => "SmeltUnknown::Null".to_owned(),
            }),
        }
    }

    /// Emits a numbered capture-group read against a concrete `SmeltMatch`.
    ///
    /// `match[n]` reads the n-th numbered group (entry 0 is the whole match) as
    /// an owned `Option<String>` — `None` for a group that did not participate,
    /// matching JavaScript `undefined`.
    pub(super) fn match_index_text(
        &self,
        receiver_text: &str,
        index: &Operand,
    ) -> Result<String, EmitError> {
        let index_ty = self.operand_ty(index)?;
        let index_text = if matches!(self.mir.types.get(index_ty), Some(Type::Int | Type::Float)) {
            self.operand_text(index)?
        } else {
            self.value_at_type(index, self.type_id(Type::Float)?)?
        };
        Ok(format!("{receiver_text}.group_owned({index_text} as usize)"))
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
        // A numbered group read (`match[n]`) has an `Optional(String)` result, so
        // MIR routes it through `OptionalIndex`; `group_owned` already yields the
        // `Option<String>` this path expects.
        if let Some(Type::Class { name, .. }) = self.mir.types.get(receiver_ty)
            && self.is_match_class_symbol(*name)?
        {
            return self.match_index_text(receiver_text, index);
        }
        match self.mir.types.get(receiver_ty) {
            Some(Type::List(item_ty)) => {
                let index_text =
                    self.optional_normalized_index_text(&format!("{receiver_text}.len()"), index)?;
                if let Some(Type::Optional(inner)) = self.mir.types.get(*item_ty)
                    && *inner == result_ty
                {
                    Ok(format!(
                        "({index_text}).and_then(|index| {receiver_text}.get(index).cloned().flatten())"
                    ))
                } else {
                    Ok(format!(
                        "({index_text}).and_then(|index| {receiver_text}.get(index).cloned())"
                    ))
                }
            }
            Some(Type::Dict(key_ty, _)) => {
                let key_text = if self.mir.types.get(*key_ty) == Some(&Type::String) {
                    let index_ty = self.operand_ty(index)?;
                    if index_ty == *key_ty {
                        self.value_at_type(index, *key_ty)?
                    } else {
                        self.property_key_to_string_text(&self.operand_text(index)?, index_ty)?
                    }
                } else {
                    self.value_at_type(index, *key_ty)?
                };
                if self.dict_uses_smelt_record(*key_ty) || self.dict_uses_js_key_map(*key_ty) {
                    Ok(format!("{receiver_text}.get(&{key_text})"))
                } else {
                    Ok(format!("{receiver_text}.get(&{key_text}).cloned()"))
                }
            }
            Some(Type::String) => {
                let index_text = self.optional_normalized_index_text(
                    &format!("{receiver_text}.chars().count()"),
                    index,
                )?;
                Ok(format!(
                    "({index_text}).and_then(|index| {receiver_text}.chars().nth(index).map(|ch| ch.to_string()))"
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
                            "match {index_text}.clone() {{ SmeltUnknown::Number(value) => value, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }}"
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
                        "values.get(&{key_text}).map(|value| match value {{ SmeltUnknown::String(value) => value, other => other.to_string() }})"
                    )
                } else {
                    format!("values.get(&{key_text})")
                };
                let primitive_none = "SmeltUnknown::Bool(_) | SmeltUnknown::Number(_) | SmeltUnknown::Symbol(_) | SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => None";
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

    /// Normalize an optional JavaScript array/string read without panicking on misses.
    ///
    /// Indexed source reads whose value is already optional model JavaScript
    /// `undefined`; a negative or out-of-range normalized position therefore
    /// remains `None` instead of entering strict Python-style index behavior.
    fn optional_normalized_index_text(
        &self,
        len_expr: &str,
        index: &Operand,
    ) -> Result<String, EmitError> {
        let index_ty = self.operand_ty(index)?;
        let index_text = if matches!(self.mir.types.get(index_ty), Some(Type::Int | Type::Float)) {
            self.operand_text(index)?
        } else {
            self.value_at_type(index, self.type_id(Type::Float)?)?
        };
        Ok(format!(
            "{{ let len = {len_expr} as i64; let index = {index_text} as i64; let normalized = if index < 0 {{ len + index }} else {{ index }}; usize::try_from(normalized).ok() }}"
        ))
    }

    // Converts an operand to console.log argument format and returns format string and value.
}
