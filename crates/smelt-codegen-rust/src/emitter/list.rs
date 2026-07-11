//! List emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Converts a list containment operation to Rust text.
    pub(super) fn list_contains_text(
        &self,
        list: &Operand,
        item: &Operand,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(list_item_ty)) = self.mir.types.get(list_ty) else {
            return Ok("false".to_owned());
        };
        let item_ty = *list_item_ty;
        let needle_ty = self.operand_ty(item)?;
        if needle_ty != item_ty {
            // A `T | undefined` needle (e.g. `array.includes(sample(array))`)
            // still checks containment against a `Vec<T>`: unwrap the optional
            // and guard on `Some`, since a `None`/`undefined` needle is never an
            // element of a list of non-optional `T`.
            if let Some(Type::Optional(opt_inner)) = self.mir.types.get(needle_ty) {
                let inner = *opt_inner;
                let compare = if inner == item_ty {
                    self.list_element_match_predicate(item_ty).to_owned()
                } else if matches!(
                    self.mir.types.get(inner),
                    Some(Type::Unknown | Type::TypeParam { .. })
                ) {
                    // An erased needle (`unknown | undefined`) compares against
                    // each element after the element is erased to the runtime
                    // `SmeltUnknown` value, matching JS `includes` semantics.
                    let erased_item = self.erase_value_text("item.clone()", item_ty)?;
                    format!("({erased_item}).same_js_key(&smelt_needle)")
                } else {
                    return Ok("false".to_owned());
                };
                let list_text = self.operand_text(list)?;
                let item_text = self.operand_text(item)?;
                return Ok(format!(
                    "{{ match {item_text} {{ Some(smelt_needle) => {list_text}.iter().any(|item| {compare}), None => false }} }}"
                ));
            }
            return Ok("false".to_owned());
        }
        let list_text = self.operand_text(list)?;
        let item_text = self.operand_text(item)?;
        if self.list_item_uses_same_value_zero(item_ty) {
            if self.mir.types.get(item_ty) == Some(&Type::Float) {
                return Ok(format!(
                    "{{ let smelt_needle = {item_text}; {list_text}.iter().any(|item| *item == smelt_needle || (item.is_nan() && smelt_needle.is_nan())) }}"
                ));
            }
            return Ok(format!(
                "{{ let smelt_needle = {item_text}; {list_text}.iter().any(|item| item.same_js_key(&smelt_needle)) }}"
            ));
        }
        Ok(format!("{list_text}.contains(&{item_text})"))
    }

    /// Per-element comparison predicate against a bound `smelt_needle` value.
    ///
    /// Mirrors the direct-needle containment comparison so an optional-needle
    /// containment check (`list.includes(sample(list))`) reuses the same
    /// SameValueZero / float-NaN / structural-identity semantics after the
    /// optional has been narrowed to `smelt_needle`. `item` is the closure
    /// binding for each `&element`.
    fn list_element_match_predicate(&self, item_ty: TypeId) -> &'static str {
        if self.list_item_uses_same_value_zero(item_ty) {
            if self.mir.types.get(item_ty) == Some(&Type::Float) {
                "*item == smelt_needle || (item.is_nan() && smelt_needle.is_nan())"
            } else {
                "item.same_js_key(&smelt_needle)"
            }
        } else {
            "*item == smelt_needle"
        }
    }

    /// Converts a set containment operation to Rust text.
    /// Converts a list-to-set constructor conversion to Rust text.
    /// Converts a set containment operation to Rust text.
    /// Converts a list-to-set constructor conversion to Rust text.
    pub(super) fn list_to_set_text(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list-to-set source must be a list"));
        };
        let set_item_ty = *item_ty;
        if !matches!(self.mir.types.get(dest_ty), Some(Type::Set(dest_item)) if *dest_item == set_item_ty)
        {
            return Err(EmitError::new(
                "list-to-set destination must be set of the list item type",
            ));
        }
        // Collect into whichever container the destination `Set` uses: a plain
        // `HashSet` for value-equality primitives, or `SmeltJsSet` (SameValueZero
        // membership, insertion order) for `f64`, unions, generics, and
        // object-like elements. Both dedup on `FromIterator`, matching JS
        // `new Set(array)`.
        let container = if self.type_is_hash_set_key_safe(set_item_ty) {
            "::std::collections::HashSet<_>"
        } else {
            "SmeltJsSet<_>"
        };
        Ok(format!(
            "{}.iter().cloned().collect::<{container}>()",
            self.operand_text(list)?
        ))
    }

    /// Converts a list of key/value pair tuples to a Rust `HashMap`.
    /// Converts a list of key/value pair tuples to a Rust `HashMap`.
    /// Converts a list of key/value pair tuples to a Rust `HashMap`.
    /// Converts a list of key/value pair tuples to a Rust `HashMap`.
    pub(super) fn list_pairs_to_dict_text(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("dict() pair source must be a list"));
        };
        let Some(Type::Tuple(items)) = self.mir.types.get(*item_ty) else {
            return Err(EmitError::new(
                "dict() pair source must contain tuple items",
            ));
        };
        let [key_ty, value_ty] = items.as_slice() else {
            return Err(EmitError::new(
                "dict() pair source tuples must have length two",
            ));
        };
        if !matches!(
            self.mir.types.get(dest_ty),
            Some(Type::Dict(dest_key, dest_value)) if dest_key == key_ty && dest_value == value_ty
        ) {
            return Err(EmitError::new(
                "dict() destination must match pair key and value types",
            ));
        }
        Ok(format!(
            "{}.iter().cloned().collect::<::std::collections::HashMap<_, _>>()",
            self.operand_text(list)?
        ))
    }

    /// Converts a list concatenation operation to Rust text.
    /// Converts a list concatenation operation to Rust text.
    /// Converts a list concatenation operation to Rust text.
    /// Converts a list concatenation operation to Rust text.
    pub(super) fn list_concat_text(
        &self,
        left: &Operand,
        right: &Operand,
    ) -> Result<String, EmitError> {
        let left_ty = self.operand_ty(left)?;
        let right_ty = self.operand_ty(right)?;
        let left_erased = matches!(
            self.mir.types.get(left_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(left_ty);
        let right_erased = matches!(
            self.mir.types.get(right_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(right_ty);
        if left_erased || right_erased {
            // Coerce both operands to the materialized result list type. When one
            // side is a concrete `List<X>` (e.g. the empty `[]` synthesized for an
            // erased `[...data]` spread), prefer `List<X>` over `List<Unknown>` so
            // the erased side's elements are unwrapped to the concrete element type
            // and `List<Unknown>` need not be interned for this function.
            let list_ty = self.concat_result_list_ty(left, right)?;
            let left_text = self.value_at_type_text(&self.operand_text(left)?, left_ty, list_ty)?;
            let right_text =
                self.value_at_type_text(&self.operand_text(right)?, right_ty, list_ty)?;
            return Ok(format!(
                "{left_text}.iter().cloned().chain({right_text}.iter().cloned()).collect::<Vec<_>>()"
            ));
        }
        let (Some(Type::List(left_item)), Some(Type::List(right_item))) =
            (self.mir.types.get(left_ty), self.mir.types.get(right_ty))
        else {
            return Ok("Default::default()".to_owned());
        };
        // `concat` (and spread) always yields a NEW array, so even concatenating
        // an empty list produces a fresh reference identity, not an alias.
        if matches!(self.mir.types.get(*left_item), Some(Type::Never))
            && self.is_empty_list_operand(left)?
        {
            return Ok(format!("{}.fresh_copy()", self.operand_text(right)?));
        }
        if matches!(self.mir.types.get(*right_item), Some(Type::Never))
            && self.is_empty_list_operand(right)?
        {
            return Ok(format!("{}.fresh_copy()", self.operand_text(left)?));
        }
        let has_never_item = matches!(self.mir.types.get(*left_item), Some(Type::Never))
            || matches!(self.mir.types.get(*right_item), Some(Type::Never));
        if left_item != right_item && !has_never_item {
            return Ok("Default::default()".to_owned());
        }
        Ok(format!(
            "{}.iter().cloned().chain({}.iter().cloned()).collect::<Vec<_>>()",
            self.operand_text(left)?,
            self.operand_text(right)?
        ))
    }

    /// Compute the list type the materialized concat result carries.
    ///
    /// `[...left, ...right]` collects into a single `Vec`, so both operands must
    /// be coerced to one element type. When both are concrete `List`s with the
    /// same item (or one is an empty `List<Never>`) that shared item wins. When
    /// exactly one side is erased (`Unknown`/`TypeParam`/`Union`/erased class) and
    /// the other is a concrete `List<X>`, the concrete `List<X>` wins — the erased
    /// side's runtime `SmeltUnknown` elements are unwrapped to `X`, and `List<X>`
    /// is guaranteed interned because the concrete operand carries it. Only when no
    /// operand offers a concrete list type do we fall back to `List<Unknown>`.
    pub(super) fn concat_result_list_ty(
        &self,
        left: &Operand,
        right: &Operand,
    ) -> Result<TypeId, EmitError> {
        let left_ty = self.operand_ty(left)?;
        let right_ty = self.operand_ty(right)?;
        let left_item = match self.mir.types.get(left_ty) {
            Some(Type::List(item)) => Some(*item),
            _ => None,
        };
        let right_item = match self.mir.types.get(right_ty) {
            Some(Type::List(item)) => Some(*item),
            _ => None,
        };
        if let (Some(left_item_ty), Some(right_item_ty)) = (left_item, right_item) {
            if left_item_ty == right_item_ty
                || matches!(self.mir.types.get(right_item_ty), Some(Type::Never))
            {
                return self.type_id(Type::List(left_item_ty));
            }
            if matches!(self.mir.types.get(left_item_ty), Some(Type::Never)) {
                return self.type_id(Type::List(right_item_ty));
            }
        }
        if let Some(left_item_ty) = left_item
            && self.is_erased_concat_operand(right_ty)
        {
            return self.type_id(Type::List(left_item_ty));
        }
        if let Some(right_item_ty) = right_item
            && self.is_erased_concat_operand(left_ty)
        {
            return self.type_id(Type::List(right_item_ty));
        }
        let unknown_ty = self.type_id(Type::Unknown)?;
        self.type_id(Type::List(unknown_ty))
    }

    /// Whether a concat operand erases to a runtime `SmeltUnknown` value, so its
    /// elements must be unwrapped when collected alongside a concrete-typed side.
    fn is_erased_concat_operand(&self, ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(ty)
    }

    /// Return whether an operand is statically known to be an empty list.
    fn is_empty_list_operand(&self, operand: &Operand) -> Result<bool, EmitError> {
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = operand
        else {
            return Ok(false);
        };
        for block in &self.function.blocks {
            for statement in &block.statements {
                let Statement::Assign { dest, value } = statement else {
                    continue;
                };
                if dest == local && matches!(value, Rvalue::List(items) if items.is_empty()) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Inline a local list-concat temporary when it is consumed as a spread
    /// argument vector before regular statement emission preserved it.
    pub(super) fn inline_list_concat_operand_text(
        &self,
        operand: &Operand,
    ) -> Result<Option<String>, EmitError> {
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = operand
        else {
            return Ok(None);
        };
        for block in &self.function.blocks {
            for statement in &block.statements {
                let Statement::Assign { dest, value } = statement else {
                    continue;
                };
                if dest != local {
                    continue;
                }
                if let Rvalue::ListConcat { left, right } = value {
                    return self.list_concat_text(left, right).map(Some);
                }
            }
        }
        Ok(None)
    }

    /// Converts a list search operation to Rust text.
    /// Converts a list search operation to Rust text.
    /// Converts a list slice operation to Rust text.
    /// Converts a list slice operation to Rust text.
    pub(super) fn list_slice_text(
        &self,
        list: &Operand,
        start: Option<&Operand>,
        end: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        if matches!(
            self.mir.types.get(list_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(list_ty)
        {
            return self.unknown_list_slice_text(list, start, end, dest_ty);
        }
        let Some(Type::List(source_item_ty)) = self.mir.types.get(list_ty) else {
            return Ok("Default::default()".to_owned());
        };
        self.validate_optional_numeric_index(start, "list slice start index")?;
        self.validate_optional_numeric_index(end, "list slice end index")?;
        let list_text = self.operand_text(list)?;
        let len_source = format!("{list_text}.len()");
        let start_text = self.slice_start_text(start, &len_source)?;
        let len_text = self.slice_len_text(&list_text, start, end, SliceLenKind::Len)?;
        let Some(Type::List(dest_item_ty)) = self.mir.types.get(dest_ty) else {
            return Ok(format!(
                "{list_text}.iter().skip({start_text}).take({len_text}).cloned().collect::<Vec<_>>()"
            ));
        };
        if source_item_ty == dest_item_ty {
            return Ok(format!(
                "{list_text}.iter().skip({start_text}).take({len_text}).cloned().collect::<Vec<_>>()"
            ));
        }
        let item_text = { self.value_at_type_text("value", *source_item_ty, *dest_item_ty)? };
        Ok(format!(
            "{list_text}.iter().skip({start_text}).take({len_text}).cloned().map(|value| {item_text}).collect::<Vec<_>>()"
        ))
    }

    /// Converts `.slice(...)` on an erased value to Rust text.
    fn unknown_list_slice_text(
        &self,
        list: &Operand,
        start: Option<&Operand>,
        end: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let float_ty = self.type_id(Type::Float)?;
        let list_text = self.erase(list)?;
        let start_text = start.map_or_else(
            || Ok("0.0".to_owned()),
            |operand| self.value_at_type(operand, float_ty),
        )?;
        let end_text = end.map_or_else(
            || Ok("None::<f64>".to_owned()),
            |operand| {
                self.value_at_type(operand, float_ty)
                    .map(|text| format!("Some({text})"))
            },
        )?;
        let sliced_values_text = "smelt_slice_values.into_iter().skip(smelt_start_index as usize).take(smelt_take_len).collect::<Vec<_>>()";
        let result_text = if matches!(
            self.mir.types.get(dest_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(dest_ty)
        {
            self.erase_unknown_array_text(sliced_values_text)
        } else {
            sliced_values_text.to_owned()
        };
        Ok(format!(
            "{{ let smelt_slice_value = {list_text}; let smelt_slice_start = {start_text}; let smelt_slice_end = {end_text}; let smelt_slice_values = match smelt_slice_value {{ SmeltUnknown::Array(values) => values.into_vec(), SmeltUnknown::String(value) => value.chars().map(|ch| SmeltUnknown::String(ch.to_string())).collect::<Vec<_>>(), _ => Vec::new() }}; let smelt_slice_len = smelt_slice_values.len() as i64; let smelt_start_index = smelt_slice_start as i64; let smelt_start_index = if smelt_start_index < 0 {{ smelt_slice_len + smelt_start_index }} else {{ smelt_start_index }}.clamp(0, smelt_slice_len); let smelt_end_index = smelt_slice_end.map_or(smelt_slice_len, |end| {{ let end = end as i64; if end < 0 {{ smelt_slice_len + end }} else {{ end }} }}).clamp(0, smelt_slice_len); let smelt_take_len = smelt_end_index.saturating_sub(smelt_start_index) as usize; {result_text} }}"
        ))
    }

    /// Converts an array splice or toSpliced operation to Rust text.
    pub(super) fn list_splice_text(
        &self,
        list: &Operand,
        start: &Operand,
        delete_count: Option<&Operand>,
        items: &[MirListSpliceItem],
        mutate: bool,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("array splice receiver must be a list"));
        };
        self.validate_optional_numeric_index(Some(start), "array splice start index")?;
        self.validate_optional_numeric_index(delete_count, "array splice delete count")?;
        for item in items {
            let expected_ty = if item.spread { list_ty } else { *item_ty };
            if self.operand_ty(&item.value)? != expected_ty {
                return Err(EmitError::new(
                    "array splice replacement items must match the array element type",
                ));
            }
        }
        if dest_ty != list_ty {
            return Err(EmitError::new("array splice destination must be a list"));
        }
        let list_text = self.operand_text(list)?;
        let start_text = self.value_at_type(start, self.type_id(Type::Float)?)?;
        let delete_count_text = delete_count
            .map(|count| self.value_at_type(count, self.type_id(Type::Float)?))
            .transpose()?
            .unwrap_or_else(|| "splice_len as f64".to_owned());
        let replacement_text = self.list_splice_replacement_text(items)?;
        let receiver_text = if mutate {
            let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = list
            else {
                return Err(EmitError::new(
                    "array splice receiver must be a mutable local for now",
                ));
            };
            self.local_mut_value_text(*local)?
        } else {
            format!("{list_text}.clone()")
        };
        let delete_count_float_text = format!("({delete_count_text} as f64)");
        if mutate {
            Ok(format!(
                "{{ let splice_len = {receiver_text}.len(); let splice_start = if {start_text} < 0.0 {{ splice_len.saturating_sub((-{start_text}) as usize) }} else {{ ({start_text} as usize).min(splice_len) }}; let splice_delete = (({delete_count_float_text}).max(0.0) as usize).min(splice_len.saturating_sub(splice_start)); {receiver_text}.splice(splice_start..splice_start + splice_delete, {replacement_text}).collect::<Vec<_>>() }}"
            ))
        } else {
            Ok(format!(
                "{{ let mut spliced = {receiver_text}; let splice_len = spliced.len(); let splice_start = if {start_text} < 0.0 {{ splice_len.saturating_sub((-{start_text}) as usize) }} else {{ ({start_text} as usize).min(splice_len) }}; let splice_delete = (({delete_count_float_text}).max(0.0) as usize).min(splice_len.saturating_sub(splice_start)); spliced.splice(splice_start..splice_start + splice_delete, {replacement_text}).for_each(drop); spliced }}"
            ))
        }
    }

    /// Converts scalar and spread splice replacements into a Rust vector expression.
    fn list_splice_replacement_text(
        &self,
        items: &[MirListSpliceItem],
    ) -> Result<String, EmitError> {
        if items.iter().all(|item| !item.spread) {
            let items_text = items
                .iter()
                .map(|item| self.operand_text(&item.value))
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            return Ok(format!("vec![{items_text}]"));
        }
        let mut statements = Vec::new();
        for item in items {
            let value_text = self.operand_text(&item.value)?;
            if item.spread {
                statements.push(format!(
                    "splice_replacements.extend(({value_text}).iter().cloned());"
                ));
            } else {
                statements.push(format!("splice_replacements.push({value_text});"));
            }
        }
        Ok(format!(
            "{{ let mut splice_replacements = Vec::new(); {} splice_replacements }}",
            statements.join(" ")
        ))
    }

    /// Converts an array fill operation to Rust text.
    pub(super) fn list_fill_text(
        &self,
        list: &Operand,
        value: &Operand,
        start: Option<&Operand>,
        end: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("array fill receiver must be a list"));
        };
        if self.operand_ty(value)? != *item_ty || dest_ty != list_ty {
            return Err(EmitError::new(
                "array fill value and destination must match the list type",
            ));
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = list else {
            return Err(EmitError::new(
                "array fill receiver must be a mutable local for now",
            ));
        };
        let list_text = self.local_mut_value_text(*local)?;
        let value_text = self.operand_text(value)?;
        let start_text = start
            .map(|operand| self.operand_text(operand))
            .transpose()?
            .unwrap_or_else(|| "0.0".to_owned());
        let end_text = end
            .map(|operand| self.operand_text(operand))
            .transpose()?
            .unwrap_or_else(|| "(fill_len as f64)".to_owned());
        Ok(format!(
            "{{ let fill_len = {list_text}.len(); let fill_start = if {start_text} < 0.0 {{ fill_len.saturating_sub((-{start_text}) as usize) }} else {{ ({start_text} as usize).min(fill_len) }}; let fill_end = if {end_text} < 0.0 {{ fill_len.saturating_sub((-{end_text}) as usize) }} else {{ ({end_text} as usize).min(fill_len) }}; for fill_index in fill_start..fill_end {{ {list_text}[fill_index] = {value_text}.clone(); }} {list_text}.clone() }}"
        ))
    }

    /// Converts an array copyWithin operation to Rust text.
    pub(super) fn list_copy_within_text(
        &self,
        list: &Operand,
        target: &Operand,
        start: &Operand,
        end: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        if !matches!(self.mir.types.get(list_ty), Some(Type::List(_))) || dest_ty != list_ty {
            return Err(EmitError::new(
                "array copyWithin destination must match the list type",
            ));
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = list else {
            return Err(EmitError::new(
                "array copyWithin receiver must be a mutable local for now",
            ));
        };
        let list_text = self.local_mut_value_text(*local)?;
        let target_text = self.operand_text(target)?;
        let start_text = self.operand_text(start)?;
        let end_text = end
            .map(|operand| self.operand_text(operand))
            .transpose()?
            .unwrap_or_else(|| "(copy_len as f64)".to_owned());
        Ok(format!(
            "{{ let copy_len = {list_text}.len(); let copy_target = if {target_text} < 0.0 {{ copy_len.saturating_sub((-{target_text}) as usize) }} else {{ ({target_text} as usize).min(copy_len) }}; let copy_start = if {start_text} < 0.0 {{ copy_len.saturating_sub((-{start_text}) as usize) }} else {{ ({start_text} as usize).min(copy_len) }}; let copy_end = if {end_text} < 0.0 {{ copy_len.saturating_sub((-{end_text}) as usize) }} else {{ ({end_text} as usize).min(copy_len) }}; let copy_items = {list_text}.iter().skip(copy_start).take(copy_end.saturating_sub(copy_start)).cloned().collect::<Vec<_>>(); for (offset, item) in copy_items.into_iter().enumerate() {{ if copy_target + offset < copy_len {{ {list_text}[copy_target + offset] = item; }} }} {list_text}.clone() }}"
        ))
    }

    /// Converts a list push operation to Rust text.
    /// Converts a list copy operation to Rust text.
    /// Converts a list push operation to Rust text.
    /// Converts a list copy operation to Rust text.
    pub(super) fn list_copy_text(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        if !matches!(self.mir.types.get(list_ty), Some(Type::List(_))) {
            return Err(EmitError::new("list copy receiver must be a list"));
        }
        if dest_ty != list_ty {
            return Err(EmitError::new(
                "list copy destination must match the receiver list type",
            ));
        }
        // A JS array copy produces a NEW reference, so mint a fresh id rather
        // than sharing the source list's identity (`SmeltList::clone` shares it).
        Ok(format!("{}.fresh_copy()", self.operand_text(list)?))
    }

    /// Converts an array `with` operation to Rust text.
    pub(super) fn list_with_text(
        &self,
        list: &Operand,
        index: &Operand,
        value: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("array with receiver must be a list"));
        };
        if dest_ty != list_ty || self.operand_ty(value)? != *item_ty {
            return Err(EmitError::new(
                "array with value and destination must match the list type",
            ));
        }
        let list_text = self.operand_text(list)?;
        let index_text = self.operand_text(index)?;
        let value_text = self.operand_text(value)?;
        Ok(format!(
            "{{ let mut with_items = {list_text}.clone(); let with_len = with_items.len(); let with_index = if {index_text} < 0.0 {{ with_len.saturating_sub((-{index_text}) as usize) }} else {{ {index_text} as usize }}; if with_index >= with_len {{ panic!(\"array with index out of bounds\"); }} with_items[with_index] = {value_text}; with_items }}"
        ))
    }

    /// Converts JavaScript array flattening to Rust text.
    ///
    /// Nested typed lists retain the simple depth-one representation. Arrays
    /// crossing an erased `unknown` boundary need runtime recursion because
    /// their nested shape and requested depth are only available at execution.
    pub(super) fn list_flat_text(
        &self,
        list: &Operand,
        depth: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        if matches!(
            self.mir.types.get(list_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(list_ty)
        {
            let Some(Type::List(item_ty)) = self.mir.types.get(dest_ty) else {
                return Ok("Default::default()".to_owned());
            };
            if self.mir.types.get(*item_ty) != Some(&Type::Unknown) {
                return Err(EmitError::new(
                    "erased array flat destination must be list[unknown]",
                ));
            }
            let depth_text = self.flat_depth_text(depth)?;
            let list_text = self.operand_text(list)?;
            return Ok(format!(
                "{{ fn smelt_flat_values(values: Vec<SmeltUnknown>, depth: i64) -> Vec<SmeltUnknown> {{ if depth <= 0 {{ return values; }} values.into_iter().flat_map(|value| match value {{ SmeltUnknown::Array(items) => smelt_flat_values(items.into_vec(), depth - 1), value => vec![value] }}).collect::<Vec<_>>() }} let smelt_flat_depth = ({depth_text}).max(0.0).floor() as i64; let smelt_flat_input = match {list_text} {{ SmeltUnknown::Array(values) => values.into_vec(), SmeltUnknown::String(value) => value.chars().map(|ch| SmeltUnknown::String(ch.to_string())).collect::<Vec<_>>(), _ => Vec::new() }}; smelt_flat_values(smelt_flat_input, smelt_flat_depth) }}"
            ));
        }
        let Some(Type::List(nested_ty)) = self.mir.types.get(list_ty) else {
            return Ok("Default::default()".to_owned());
        };
        if self.mir.types.get(*nested_ty) == Some(&Type::Unknown)
            && self.mir.types.get(dest_ty) == Some(&Type::List(*nested_ty))
        {
            let depth_text = self.flat_depth_text(depth)?;
            return Ok(format!(
                "{{ fn smelt_flat_values(values: Vec<SmeltUnknown>, depth: i64) -> Vec<SmeltUnknown> {{ if depth <= 0 {{ return values; }} values.into_iter().flat_map(|value| match value {{ SmeltUnknown::Array(items) => smelt_flat_values(items.into_vec(), depth - 1), value => vec![value] }}).collect::<Vec<_>>() }} let smelt_flat_depth = ({depth_text}).max(0.0).floor() as i64; smelt_flat_values({}.clone().into_vec(), smelt_flat_depth) }}",
                self.operand_text(list)?
            ));
        }
        if let Some(Type::Tuple(items)) = self.mir.types.get(*nested_ty) {
            if depth.is_some() {
                return Err(EmitError::new(
                    "array flat with explicit depth requires an erased nested list",
                ));
            }
            let Some(Type::List(dest_item_ty)) = self.mir.types.get(dest_ty) else {
                return Err(EmitError::new("array flat destination must be a list"));
            };
            let item_texts = items
                .iter()
                .enumerate()
                .map(|(index, item_ty)| {
                    self.value_at_type_text(
                        &format!("items.{index}.clone()"),
                        *item_ty,
                        *dest_item_ty,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            return Ok(format!(
                "{}.iter().flat_map(|items| vec![{item_texts}]).collect::<Vec<_>>()",
                self.operand_text(list)?
            ));
        }
        let Some(Type::List(item_ty)) = self.mir.types.get(*nested_ty) else {
            return Ok("Default::default()".to_owned());
        };
        if depth.is_some() {
            return Err(EmitError::new(
                "array flat with explicit depth requires an erased nested list",
            ));
        }
        if self.mir.types.get(dest_ty) != Some(&Type::List(*item_ty)) {
            return Err(EmitError::new(
                "array flat destination must match nested item type",
            ));
        }
        Ok(format!(
            "{}.iter().flat_map(|items| items.iter().cloned()).collect::<Vec<_>>()",
            self.operand_text(list)?
        ))
    }

    /// Emits the JavaScript `Array.flat` depth expression as a runtime number.
    fn flat_depth_text(&self, depth: Option<&Operand>) -> Result<String, EmitError> {
        match depth {
            None => Ok("1.0_f64".to_owned()),
            Some(depth_operand) => {
                let text = self.operand_text(depth_operand)?;
                match self.mir.types.get(self.operand_ty(depth_operand)?) {
                    Some(Type::Optional(inner))
                        if self.mir.types.get(*inner) == Some(&Type::Float) =>
                    {
                        Ok(format!("{text}.clone().unwrap_or(1.0)"))
                    }
                    Some(Type::Float) => Ok(text),
                    _ => Err(EmitError::new("array flat depth must be numeric")),
                }
            }
        }
    }

    /// Converts array keys, values, and entries projections to Rust text.
    pub(super) fn list_projection_text(
        &self,
        op: smelt_hir::ListProjectionOp,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let list_text = self.operand_text(list)?;
        if matches!(
            self.mir.types.get(list_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) {
            return match op {
                smelt_hir::ListProjectionOp::Keys => {
                    let int_ty = self.type_id(Type::Int)?;
                    if self.mir.types.get(dest_ty) != Some(&Type::List(int_ty)) {
                        return Err(EmitError::new("array keys destination must be int list"));
                    }
                    Ok(format!(
                        "match {list_text}.clone() {{ SmeltUnknown::Array(values) => (0..values.len()).map(|idx| idx as i64).collect::<Vec<_>>(), _ => Vec::new() }}"
                    ))
                }
                smelt_hir::ListProjectionOp::Values => {
                    let Some(Type::List(value_ty)) = self.mir.types.get(dest_ty) else {
                        return Err(EmitError::new(
                            "dynamic array values destination must be a list",
                        ));
                    };
                    let item_text = self.extract_value_text("item", *value_ty)?;
                    Ok(format!(
                        "match {list_text}.clone() {{ SmeltUnknown::Array(values) => values.into_iter().map(|item| {item_text}).collect::<Vec<_>>(), _ => Vec::new() }}"
                    ))
                }
                smelt_hir::ListProjectionOp::Entries => {
                    let int_ty = self.type_id(Type::Int)?;
                    let Some(Type::List(tuple_ty)) = self.mir.types.get(dest_ty) else {
                        return Err(EmitError::new("array entries destination must be a list"));
                    };
                    let Some(Type::Tuple(items)) = self.mir.types.get(*tuple_ty) else {
                        return Err(EmitError::new("array entries item must be a tuple"));
                    };
                    if items.first() != Some(&int_ty) || items.len() != 2 {
                        return Err(EmitError::new(
                            "dynamic array entries destination must contain (int, item) tuples",
                        ));
                    }
                    let Some(item_ty) = items.get(1).copied() else {
                        return Err(EmitError::new(
                            "dynamic array entries destination must contain an item type",
                        ));
                    };
                    let item_text = self.extract_value_text("item", item_ty)?;
                    Ok(format!(
                        "match {list_text}.clone() {{ SmeltUnknown::Array(values) => values.into_iter().enumerate().map(|(idx, item)| (idx as i64, {item_text})).collect::<Vec<_>>(), _ => Vec::new() }}"
                    ))
                }
            };
        }
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("array projection receiver must be a list"));
        };
        match op {
            smelt_hir::ListProjectionOp::Keys => {
                let int_ty = self.type_id(Type::Int)?;
                if self.mir.types.get(dest_ty) != Some(&Type::List(int_ty)) {
                    return Err(EmitError::new("array keys destination must be int list"));
                }
                Ok(format!(
                    "(0..{list_text}.len()).map(|idx| idx as i64).collect::<Vec<_>>()"
                ))
            }
            smelt_hir::ListProjectionOp::Values => {
                if dest_ty != list_ty {
                    return Err(EmitError::new(
                        "array values destination must match receiver",
                    ));
                }
                Ok(format!("{list_text}.clone()"))
            }
            smelt_hir::ListProjectionOp::Entries => {
                let int_ty = self.type_id(Type::Int)?;
                let Some(Type::List(tuple_ty)) = self.mir.types.get(dest_ty) else {
                    return Err(EmitError::new("array entries destination must be a list"));
                };
                let Some(Type::Tuple(items)) = self.mir.types.get(*tuple_ty) else {
                    return Err(EmitError::new("array entries item must be a tuple"));
                };
                if items.as_slice() != [int_ty, *item_ty] {
                    return Err(EmitError::new(
                        "array entries destination must contain (int, item) tuples",
                    ));
                }
                if self.type_contains_function(*item_ty) {
                    Ok(format!(
                        "{list_text}.into_iter().enumerate().map(|(idx, item)| (idx as i64, item)).collect::<Vec<_>>()"
                    ))
                } else {
                    Ok(format!(
                        "{list_text}.iter().cloned().enumerate().map(|(idx, item)| (idx as i64, item)).collect::<Vec<_>>()"
                    ))
                }
            }
        }
    }

    /// Converts a list count operation to Rust text.
    /// Converts a list count operation to Rust text.
    /// Converts a tuple containment operation to Rust text.
    /// Converts a list-to-tuple constructor conversion to Rust text.
    pub(super) fn list_to_tuple_text(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list-to-tuple source must be a list"));
        };
        let Some(Type::Tuple(items)) = self.mir.types.get(dest_ty) else {
            return Err(EmitError::new("list-to-tuple destination must be a tuple"));
        };
        if !items.iter().all(|tuple_item| tuple_item == item_ty) {
            return Err(EmitError::new(
                "list-to-tuple destination items must match the list item type",
            ));
        }
        let items_text = (0..items.len())
            .map(|idx| format!("tuple_items[{idx}].clone()"))
            .collect::<Vec<_>>()
            .join(", ");
        let tuple_text = if items.len() == 1 {
            format!("({items_text},)")
        } else {
            format!("({items_text})")
        };
        Ok(format!(
            "{{ let tuple_items = {}; if tuple_items.len() != {} {{ panic!(\"tuple() length mismatch\"); }} {tuple_text} }}",
            self.operand_text(list)?,
            items.len()
        ))
    }

    // Converts a homogeneous tuple-to-set constructor conversion to Rust text.
}
