//! Map emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Converts a dictionary key containment operation to Rust text.
    pub(super) fn dict_contains_key_text(
        &self,
        dict: &Operand,
        key: &Operand,
    ) -> Result<String, EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        // A `"field" in value` test over a concrete union projects to a static
        // discriminant check instead of a runtime object lookup. The frontend
        // keeps such receivers at their union type precisely so this fast path
        // can fire; `concrete_union_field_check` returns `None` when the union is
        // not fully concrete, leaving the erased fallback below to run.
        if matches!(self.mir.types.get(dict_ty), Some(Type::Union(_)))
            && let Some(field) = self.operand_string_literal(key)
            && let Some(check) =
                self.concrete_union_field_check(&self.operand_text(dict)?, dict_ty, &field)
        {
            return Ok(check);
        }
        let Some(Type::Dict(key_ty, _) | Type::JsMap(key_ty, _)) = self.mir.types.get(dict_ty)
        else {
            if self.dict_contains_key_uses_erased_object(dict_ty) {
                let dict_text = self.operand_text(dict)?;
                let key_text = self.operand_text(key)?;
                let key_value = match self.mir.types.get(self.operand_ty(key)?) {
                    Some(Type::String) => key_text,
                    Some(Type::Int | Type::Float | Type::Bool) => format!("{key_text}.to_string()"),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => {
                        self.property_key_to_string_text(&key_text, self.operand_ty(key)?)?
                    }
                    _ => return Ok("false".to_owned()),
                };
                return Ok(format!(
                    "{{ let smelt_key = {key_value}; match {dict_text}.clone() {{ SmeltUnknown::Object(values) => values.contains_key(&smelt_key), SmeltUnknown::Array(values) => smelt_key == \"length\" || smelt_key == \"__smelt_symbol_iterator\" || smelt_key.parse::<usize>().ok().is_some_and(|index| index < values.len()), SmeltUnknown::String(value) => smelt_key == \"length\" || smelt_key == \"__smelt_symbol_iterator\" || smelt_key.parse::<usize>().ok().is_some_and(|index| index < value.chars().count()), _ => false }} }}"
                ));
            }
            return Ok("false".to_owned());
        };
        if self.operand_ty(key)? != *key_ty {
            return Ok("false".to_owned());
        }
        Ok(format!(
            "{}.contains_key(&{})",
            self.operand_text(dict)?,
            self.operand_text(key)?
        ))
    }

    /// Return the string value when an operand is a constant string literal.
    ///
    /// Used to recognize a static discriminant key in `"field" in value` so the
    /// concrete-union field check can be emitted from a literal property name.
    fn operand_string_literal(&self, operand: &Operand) -> Option<String> {
        match operand {
            Operand::Const(Constant::String(value)) => Some(value.clone()),
            _ => None,
        }
    }

    /// Return whether a dictionary containment check must inspect an erased
    /// JavaScript object value at runtime.
    ///
    /// Class-shaped types without a local declaration (ambient interfaces such
    /// as `IArguments`) are represented as `SmeltUnknown` at runtime — see
    /// [`Self::is_erased_class_type`] — so their containment checks go through
    /// the same live-object inspection as `unknown` values.
    fn dict_contains_key_uses_erased_object(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Unknown | Type::Union(_)) => true,
            Some(Type::TypeParam { name }) => !self.current_function_has_type_param(*name),
            Some(Type::Class { .. }) => self.is_erased_class_type(ty),
            _ => false,
        }
    }

    /// Converts a dictionary get operation to Rust text.
    /// Converts a dictionary get operation to Rust text.
    pub(super) fn dict_get_text(
        &self,
        dict: &Operand,
        key: &Operand,
        default: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        let Some(Type::Dict(key_ty, value_ty) | Type::JsMap(key_ty, value_ty)) =
            self.mir.types.get(dict_ty)
        else {
            return Err(EmitError::new("dict get receiver must be a dict"));
        };
        if !self.dict_key_operand_is_compatible(key, *key_ty)? {
            return Ok("Default::default()".to_owned());
        }
        let key_text = self.dict_key_operand_text(key, *key_ty)?;
        if let Some(default_operand) = default {
            if self.operand_ty(default_operand)? != *value_ty {
                return Err(EmitError::new(
                    "dict get default must match the dict value type",
                ));
            }
            if dest_ty != *value_ty {
                return Err(EmitError::new(
                    "dict get destination with default must match the dict value type",
                ));
            }
            let get_text =
                if self.dict_uses_smelt_record(*key_ty) || self.dict_uses_js_key_map(*key_ty) {
                    format!("{}.get(&{})", self.operand_text(dict)?, key_text)
                } else {
                    format!("{}.get(&{}).cloned()", self.operand_text(dict)?, key_text)
                };
            return Ok(format!(
                "{get_text}.unwrap_or({})",
                self.operand_text(default_operand)?
            ));
        }
        if dest_ty == *value_ty
            && self.mir.types.get(*key_ty) == Some(&Type::String)
            && self.type_text_with_impl_trait(*value_ty, false)? == "SmeltUnknown"
        {
            return Ok(format!(
                "{}.get(&{}).unwrap_or(SmeltUnknown::Null)",
                self.operand_text(dict)?,
                key_text
            ));
        }
        match (self.mir.types.get(*value_ty), self.mir.types.get(dest_ty)) {
            (Some(Type::Optional(value_inner)), Some(Type::Optional(dest_inner)))
                if value_inner == dest_inner =>
            {
                let get_text =
                    if self.dict_uses_smelt_record(*key_ty) || self.dict_uses_js_key_map(*key_ty) {
                        format!("{}.get(&{})", self.operand_text(dict)?, key_text)
                    } else {
                        format!("{}.get(&{}).cloned()", self.operand_text(dict)?, key_text)
                    };
                Ok(format!("{get_text}.flatten()"))
            }
            (_, Some(Type::Optional(dest_inner))) if dest_inner == value_ty => {
                if self.dict_uses_smelt_record(*key_ty) || self.dict_uses_js_key_map(*key_ty) {
                    Ok(format!("{}.get(&{})", self.operand_text(dict)?, key_text))
                } else {
                    Ok(format!(
                        "{}.get(&{}).cloned()",
                        self.operand_text(dict)?,
                        key_text
                    ))
                }
            }
            _ => Err(EmitError::new(
                "dict get destination without default must be optional value",
            )),
        }
    }

    /// Converts a dictionary setdefault operation to Rust text.
    ///
    /// This mapping supports the explicit-default form only, so generated Rust
    /// can preserve the dictionary value type without inventing a `None`
    /// default for non-optional values.
    /// Converts a dictionary setdefault operation to Rust text.
    ///
    /// This mapping supports the explicit-default form only, so generated Rust
    /// can preserve the dictionary value type without inventing a `None`
    /// default for non-optional values.
    pub(super) fn dict_setdefault_text(
        &self,
        dict: &Operand,
        key: &Operand,
        default: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        let Some(Type::Dict(key_ty, value_ty) | Type::JsMap(key_ty, value_ty)) =
            self.mir.types.get(dict_ty)
        else {
            return Err(EmitError::new("dict setdefault receiver must be a dict"));
        };
        if self.operand_ty(key)? != *key_ty {
            return Err(EmitError::new(
                "dict setdefault key must match the dict key type",
            ));
        }
        if self.operand_ty(default)? != *value_ty {
            return Err(EmitError::new(
                "dict setdefault default must match the dict value type",
            ));
        }
        if dest_ty != *value_ty {
            return Err(EmitError::new(
                "dict setdefault destination must match the dict value type",
            ));
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = dict else {
            return Err(EmitError::new(
                "dict setdefault receiver must be a mutable local for now",
            ));
        };
        let dict_text = self.local_mut_value_text(*local)?;
        let key_text = self.operand_text(key)?;
        let default_text = self.operand_text(default)?;
        if self.dict_uses_smelt_record(*key_ty) || self.dict_uses_js_key_map(*key_ty) {
            Ok(format!(
                "{{ if let Some(value) = {dict_text}.get(&{key_text}) {{ value }} else {{ {dict_text}.insert({key_text}, {default_text}.clone()); {default_text} }} }}"
            ))
        } else {
            Ok(format!(
                "{{ {dict_text}.entry({key_text}).or_insert({default_text}).clone() }}"
            ))
        }
    }

    /// Converts a dictionary insertion operation to Rust text.
    /// Converts a dictionary insertion operation to Rust text.
    pub(super) fn dict_set_text(
        &self,
        dict: &Operand,
        key: &Operand,
        value: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        let Some(Type::Dict(key_ty, value_ty) | Type::JsMap(key_ty, value_ty)) =
            self.mir.types.get(dict_ty)
        else {
            return Ok("Default::default()".to_owned());
        };
        if !self.dict_key_operand_is_compatible(key, *key_ty)? {
            return Ok(self.operand_text(dict)?);
        }
        if self.operand_ty(value)? != *value_ty {
            return Ok(self.operand_text(dict)?);
        }
        if dest_ty != dict_ty {
            return Err(EmitError::new(
                "dict set destination must match the receiver dict type",
            ));
        }
        // The receiver may be a plain local or a place rooted in one (a class
        // field holding a `Map`, e.g. `this.__data.set(k, v)`). Render the
        // assignable lvalue for the place so the insertion mutates the stored
        // dict in place rather than a temporary copy.
        let (Operand::Copy(dict_place) | Operand::Move(dict_place)) = dict else {
            return Err(EmitError::new(
                "dict set receiver must be a place operand",
            ));
        };
        let dict_text = self.assignment_place_text(dict_place)?;
        let key_text = self.dict_key_operand_text(key, *key_ty)?;
        let value_text = self.value_at_type(value, *value_ty)?;
        Ok(format!(
            "{{ {dict_text}.insert({key_text}, {value_text}); {dict_text}.clone() }}"
        ))
    }

    /// Return whether a key operand can be used for a dictionary operation.
    fn dict_key_operand_is_compatible(
        &self,
        key: &Operand,
        key_ty: TypeId,
    ) -> Result<bool, EmitError> {
        let operand_ty = self.operand_ty(key)?;
        if operand_ty == key_ty {
            return Ok(true);
        }
        Ok(matches!(
            self.mir.types.get(operand_ty),
            Some(Type::Unknown | Type::TypeParam { .. })
        ) || matches!(self.mir.types.get(operand_ty), Some(Type::Optional(inner)) if *inner == key_ty))
    }

    /// Render a dictionary key, unwrapping optional keys after frontend narrowing.
    fn dict_key_operand_text(&self, key: &Operand, key_ty: TypeId) -> Result<String, EmitError> {
        let operand_ty = self.operand_ty(key)?;
        if matches!(self.mir.types.get(operand_ty), Some(Type::Optional(inner)) if *inner == key_ty)
        {
            let key_text = self.operand_text(key)?;
            let default = self.default_value(key_ty)?;
            return Ok(format!("{key_text}.clone().unwrap_or({default})"));
        }
        if operand_ty == key_ty {
            return self.value_at_type(key, key_ty);
        }
        if self.mir.types.get(key_ty) == Some(&Type::String) {
            let key_text = self.operand_text(key)?;
            return self.property_key_to_string_text(&key_text, operand_ty);
        }
        self.value_at_type(key, key_ty)
    }

    /// Converts a dictionary key removal operation to Rust text.
    /// Converts a dictionary key removal operation to Rust text.
    pub(super) fn dict_remove_key_text(
        &self,
        dict: &Operand,
        key: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        let Some(Type::Dict(key_ty, _) | Type::JsMap(key_ty, _)) = self.mir.types.get(dict_ty)
        else {
            if matches!(
                self.mir.types.get(dict_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Never | Type::Union(_))
            ) || self.is_erased_class_type(dict_ty)
            {
                if !matches!(self.mir.types.get(dest_ty), Some(Type::Bool)) {
                    return Err(EmitError::new("dict remove destination must be bool"));
                }
                let rendered_key = self.operand_text(key)?;
                let key_text =
                    self.property_key_to_string_text(&rendered_key, self.operand_ty(key)?)?;
                if let Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) =
                    dict
                {
                    return Ok(format!(
                        "match &mut {} {{ SmeltUnknown::Object(map) => map.remove(&{key_text}).is_some(), _ => true }}",
                        self.local_mut_value_text(*local)?
                    ));
                }
                return Ok("true".to_owned());
            }
            // JavaScript `delete list[i]` punches a hole while preserving
            // length. Smelt lists are dense `Vec<T>` with no hole
            // representation, so the delete lowers to a successful no-op —
            // the same explicit deferral style as no-op list `length` growth.
            // Subsequent reads observe the retained element instead of a hole.
            if matches!(self.mir.types.get(dict_ty), Some(Type::List(_) | Type::Tuple(_))) {
                if !matches!(self.mir.types.get(dest_ty), Some(Type::Bool)) {
                    return Err(EmitError::new("dict remove destination must be bool"));
                }
                return Ok("true".to_owned());
            }
            return Err(EmitError::new(format!(
                "dict remove receiver must be a dict, got {}",
                Self::type_text_for(self.mir, dict_ty)
                    .unwrap_or_else(|_error| format!("{dict_ty:?}"))
            )));
        };
        // Coerce a dynamically-typed (`Unknown`/optional/union) key to the dict's
        // key type instead of dropping the removal — mirrors `dict_set_text`.
        // `delete out[key]` lowers here with `key` erased even though `out` is
        // keyed by `String` (omit) or a union (intersection's `Map.delete`).
        if !self.dict_key_operand_is_compatible(key, *key_ty)? {
            return Ok("false".to_owned());
        }
        if !matches!(self.mir.types.get(dest_ty), Some(Type::Bool)) {
            return Err(EmitError::new("dict remove destination must be bool"));
        }
        // Accept a place-rooted receiver (a class field holding a `Map`, e.g.
        // `this.__data.delete(k)`) as well as a plain local, rendering the
        // assignable lvalue so the removal mutates the stored dict in place.
        let (Operand::Copy(dict_place) | Operand::Move(dict_place)) = dict else {
            return Err(EmitError::new(
                "dict remove receiver must be a place operand",
            ));
        };
        Ok(format!(
            "{}.remove(&{}).is_some()",
            self.assignment_place_text(dict_place)?,
            self.dict_key_operand_text(key, *key_ty)?
        ))
    }

    /// Converts a dictionary pop operation to Rust text.
    /// Converts a dictionary pop operation to Rust text.
    pub(super) fn dict_pop_text(
        &self,
        dict: &Operand,
        key: &Operand,
        default: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        let Some(Type::Dict(key_ty, value_ty) | Type::JsMap(key_ty, value_ty)) =
            self.mir.types.get(dict_ty)
        else {
            return Err(EmitError::new("dict pop receiver must be a dict"));
        };
        if self.operand_ty(key)? != *key_ty {
            return Err(EmitError::new("dict pop key must match the dict key type"));
        }
        if dest_ty != *value_ty {
            return Err(EmitError::new(
                "dict pop destination must match the dict value type",
            ));
        }
        if let Some(default_operand) = default
            && self.operand_ty(default_operand)? != *value_ty
        {
            return Err(EmitError::new(
                "dict pop default must match the dict value type",
            ));
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = dict else {
            return Err(EmitError::new(
                "dict pop receiver must be a mutable local for now",
            ));
        };
        let dict_text = self.local_mut_value_text(*local)?;
        let key_text = self.operand_text(key)?;
        if let Some(default_operand) = default {
            let default_text = self.operand_text(default_operand)?;
            Ok(format!(
                "{dict_text}.remove(&{key_text}).unwrap_or({default_text})"
            ))
        } else {
            Ok(format!(
                "{dict_text}.remove(&{key_text}).expect(\"dict pop missing key\")"
            ))
        }
    }

    /// Converts a dictionary update operation to Rust text.
    /// Converts a dictionary update operation to Rust text.
    pub(super) fn dict_update_text(
        &self,
        dict: &Operand,
        other: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        if !matches!(self.mir.types.get(dict_ty), Some(Type::Dict(_, _))) {
            return Err(EmitError::new("dict update receiver must be a dict"));
        }
        if self.operand_ty(other)? != dict_ty {
            return Err(EmitError::new(
                "dict update argument must match the receiver dict type",
            ));
        }
        if !matches!(self.mir.types.get(dest_ty), Some(Type::None)) {
            return Err(EmitError::new("dict update destination must be None"));
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = dict else {
            return Err(EmitError::new(
                "dict update receiver must be a mutable local for now",
            ));
        };
        let dict_text = self.local_mut_value_text(*local)?;
        let other_text = self.operand_text(other)?;
        Ok(format!(
            "{{ {dict_text}.extend({other_text}.iter().map(|(key, value)| (key.clone(), value.clone()))); () }}"
        ))
    }

    /// Converts a dictionary assign operation to Rust text.
    pub(super) fn dict_assign_text(
        &self,
        target: &Operand,
        sources: &[Operand],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let target_ty = self.operand_ty(target)?;
        if !matches!(self.mir.types.get(target_ty), Some(Type::Dict(_, _))) {
            return Err(EmitError::new("dict assign target must be a dict"));
        }
        if dest_ty != target_ty {
            return Err(EmitError::new(
                "dict assign destination must match the target dict type",
            ));
        }
        // JavaScript `Object.assign(target, ...sources)` mutates `target` in
        // place and returns it. When `target` is a mutable local, extend that
        // local directly so the caller's binding observes the merge (a plain
        // discarded `Object.assign(result, src)` must still update `result`),
        // then evaluate to the mutated dict. When `target` is not a place
        // (e.g. `Object.assign({}, a, b)`), fall back to merging into a fresh
        // clone since there is nothing to write back.
        let target_place = match target {
            Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => Some(*local),
            _ => None,
        };
        let (accumulator, mut steps, final_value) = match target_place {
            Some(local) => {
                let target_text = self.local_mut_value_text(local)?;
                let final_value = format!("{target_text}.clone()");
                (target_text, Vec::new(), final_value)
            }
            None => {
                let target_text = self.operand_text(target)?;
                (
                    "assigned".to_owned(),
                    vec![format!("let mut assigned = {target_text}.clone();")],
                    "assigned".to_owned(),
                )
            }
        };
        for source in sources {
            let source_ty = self.operand_ty(source)?;
            let source_text = if source_ty == target_ty {
                self.operand_text(source)?
            } else if !matches!(self.mir.types.get(source_ty), Some(Type::Dict(_, _))) {
                self.object_spread_unknown_source_text(source, target_ty)?
            } else {
                continue;
            };
            steps.push(format!(
                "{accumulator}.extend({source_text}.iter().map(|(key, value)| (key.clone(), value.clone())));"
            ));
        }
        steps.push(final_value);
        Ok(format!("{{ {} }}", steps.join(" ")))
    }

    /// Converts an unknown object-spread source into the typed record target.
    ///
    /// JavaScript object spread copies enumerable object fields at runtime even
    /// when the source is statically opaque. The resulting object can still
    /// have a narrower static surface from later explicit properties, so each
    /// copied value is converted to the target record value type as it is
    /// inserted.
    fn object_spread_unknown_source_text(
        &self,
        source: &Operand,
        target_ty: TypeId,
    ) -> Result<String, EmitError> {
        let Some(Type::Dict(key_ty, value_ty)) = self.mir.types.get(target_ty) else {
            return self.value_at_type_text(
                &self.operand_text(source)?,
                self.operand_ty(source)?,
                target_ty,
            );
        };
        if self.mir.types.get(*key_ty) != Some(&Type::String) {
            return self.value_at_type_text(
                &self.operand_text(source)?,
                self.operand_ty(source)?,
                target_ty,
            );
        }
        let value_text = self.extract_value_text("value", *value_ty)?;
        let source_text = self.operand_text(source)?;
        // The spread-merge match inspects a dynamic `SmeltUnknown::Object`, so a
        // source that still carries a concrete static shape (a typed options
        // struct, an `Option<Struct>`, a type parameter, etc.) must first be
        // erased to `SmeltUnknown` through its boundary adapter. Sources already
        // typed as `unknown` are left untouched so the common dynamic spread
        // keeps its previous byte-identical emission.
        let source_ty = self.operand_ty(source)?;
        let scrutinee_text = if matches!(self.mir.types.get(source_ty), Some(Type::Unknown)) {
            format!("{source_text}.clone()")
        } else {
            format!("IntoSmeltUnknown::into_smelt_unknown({source_text}.clone())")
        };
        Ok(format!(
            "match {scrutinee_text} {{ SmeltUnknown::Object(map) => SmeltRecord::with_id_from_entries(map.id, map.into_iter().map(|(key, value)| (key, {value_text}))), _ => SmeltRecord::new() }}"
        ))
    }

    /// Converts a dictionary copy operation to Rust text.
    /// Converts a dictionary copy operation to Rust text.
    pub(super) fn dict_copy_text(
        &self,
        dict: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        if !matches!(self.mir.types.get(dict_ty), Some(Type::Dict(_, _))) {
            if matches!(
                self.mir.types.get(dict_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            ) && matches!(
                self.mir.types.get(dest_ty),
                Some(Type::Dict(key_ty, value_ty))
                    if self.mir.types.get(*key_ty) == Some(&Type::String)
                        && self.mir.types.get(*value_ty) == Some(&Type::Unknown)
            ) {
                let dict_text = self.operand_text(dict)?;
                return Ok(format!(
                    "match {dict_text}.clone() {{ SmeltUnknown::Object(map) => SmeltRecord::with_id_from_entries(map.id, map.into_iter()), _ => Default::default() }}"
                ));
            }
            return Ok("Default::default()".to_owned());
        }
        if dest_ty != dict_ty {
            return Err(EmitError::new(
                "dict copy destination must match the receiver dict type",
            ));
        }
        Ok(format!("{}.clone()", self.operand_text(dict)?))
    }

    /// Converts a dictionary projection operation to Rust text.
    /// Converts a dictionary projection operation to Rust text.
    pub(super) fn dict_projection_text(
        &self,
        op: smelt_hir::DictProjectionOp,
        dict: &Operand,
    ) -> Result<String, EmitError> {
        let dict_text = self.operand_text(dict)?;
        if matches!(
            self.mir.types.get(self.operand_ty(dict)?),
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
        ) {
            return match op {
                smelt_hir::DictProjectionOp::FromEntries => Ok(format!(
                    "match {dict_text} {{ SmeltUnknown::Array(entries) => entries.into_iter().filter_map(|entry| match entry {{ SmeltUnknown::Array(values) if values.len() >= 2 => {{ let mut values = values.into_iter(); let key = match values.next()? {{ SmeltUnknown::String(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), _ => return None }}; Some((key, values.next()?)) }}, _ => None }}).collect::<SmeltRecord<String, SmeltUnknown>>(), _ => SmeltRecord::new() }}"
                )),
                smelt_hir::DictProjectionOp::Keys => Ok(format!(
                    "match {dict_text} {{ SmeltUnknown::Object(map) => map.keys().into_iter().filter(|key| !key.starts_with(\"__smelt_symbol:\") && key != \"__smelt_class\").collect(), _ => Vec::new() }}"
                )),
                smelt_hir::DictProjectionOp::ForInKeys => Ok(format!(
                    "match {dict_text} {{ SmeltUnknown::Object(map) => map.keys().into_iter().filter(|key| smelt_is_for_in_object_key(&map, key)).collect(), _ => Vec::new() }}"
                )),
                smelt_hir::DictProjectionOp::Symbols => Ok(format!(
                    "match {dict_text} {{ SmeltUnknown::Object(map) => map.keys().into_iter().filter_map(|key| key.strip_prefix(\"__smelt_symbol:\").map(str::to_owned)).collect(), _ => Vec::new() }}"
                )),
                smelt_hir::DictProjectionOp::Values => Ok(format!(
                    "match {dict_text} {{ SmeltUnknown::Object(map) => map.iter().filter(|(key, _)| !key.starts_with(\"__smelt_symbol:\") && key != \"__smelt_class\").map(|(_, value)| value).collect(), _ => Vec::new() }}"
                )),
                smelt_hir::DictProjectionOp::Entries => Ok(format!(
                    "match {dict_text} {{ SmeltUnknown::Object(map) => map.into_iter().filter(|(key, _)| !key.starts_with(\"__smelt_symbol:\") && key != \"__smelt_class\").collect::<Vec<_>>(), _ => Vec::new() }}"
                )),
            };
        }
        if !matches!(
            self.mir.types.get(self.operand_ty(dict)?),
            Some(Type::Dict(_, _) | Type::JsMap(_, _))
        ) {
            return Err(EmitError::new("dict projection receiver must be a dict"));
        }
        match op {
            smelt_hir::DictProjectionOp::FromEntries => {
                Err(EmitError::new("fromEntries receiver must be erased"))
            }
            smelt_hir::DictProjectionOp::Keys => {
                let Some(Type::Dict(key_ty, _) | Type::JsMap(key_ty, _)) =
                    self.mir.types.get(self.operand_ty(dict)?)
                else {
                    return Err(EmitError::new("dict projection receiver must be a dict"));
                };
                if self.mir.types.get(*key_ty) == Some(&Type::String) {
                    // `Object.keys` returns own enumerable string keys; exclude
                    // both symbol keys and Smelt's internal marker keys
                    // (`__smelt_date`, `__smelt_regexp`/`source`/`flags`, ...),
                    // which are representation details, not real JS properties.
                    // This is why e.g. `isShallowEqual(/a/, /b/)` (no own keys) is
                    // equal. The marker filter (`smelt_is_for_in_record_key`) is
                    // defined only over `SmeltRecord` — the sole backing where
                    // those markers can appear — so the `SmeltJsMap` and plain
                    // dict backings keep the symbol-only filter (they never carry
                    // internal markers, and the helper would not type-check there).
                    if self.dict_uses_smelt_record(*key_ty) {
                        Ok(format!(
                            "{dict_text}.keys().filter(|key| !key.starts_with(\"__smelt_symbol:\") && smelt_is_for_in_record_key(&{dict_text}, key)).collect::<Vec<_>>()"
                        ))
                    } else if self.dict_uses_js_key_map(*key_ty) {
                        Ok(format!(
                            "{dict_text}.keys().filter(|key| !key.starts_with(\"__smelt_symbol:\")).collect::<Vec<_>>()"
                        ))
                    } else {
                        Ok(format!(
                            "{dict_text}.keys().filter(|key| !key.starts_with(\"__smelt_symbol:\")).cloned().collect::<Vec<_>>()"
                        ))
                    }
                } else if self.dict_uses_js_key_map(*key_ty) {
                    Ok(format!(
                        "{dict_text}.keys().filter(|key| !matches!(key, SmeltUnknown::Symbol(_))).collect::<Vec<_>>()"
                    ))
                } else {
                    Ok(format!("{dict_text}.keys().cloned().collect::<Vec<_>>()"))
                }
            }
            smelt_hir::DictProjectionOp::ForInKeys => {
                let Some(Type::Dict(key_ty, _) | Type::JsMap(key_ty, _)) =
                    self.mir.types.get(self.operand_ty(dict)?)
                else {
                    return Err(EmitError::new("dict projection receiver must be a dict"));
                };
                if self.mir.types.get(*key_ty) == Some(&Type::String) {
                    if self.dict_uses_smelt_record(*key_ty) || self.dict_uses_js_key_map(*key_ty) {
                        Ok(format!(
                            "{dict_text}.keys().filter(|key| smelt_is_for_in_record_key(&{dict_text}, key)).collect::<Vec<_>>()"
                        ))
                    } else {
                        Ok(format!(
                            "{dict_text}.keys().filter(|key| smelt_is_for_in_record_key(&{dict_text}, key)).cloned().collect::<Vec<_>>()"
                        ))
                    }
                } else if self.dict_uses_js_key_map(*key_ty) {
                    Ok(format!(
                        "{dict_text}.keys().filter(|key| !matches!(key, SmeltUnknown::Symbol(_))).collect::<Vec<_>>()"
                    ))
                } else {
                    Ok(format!("{dict_text}.keys().cloned().collect::<Vec<_>>()"))
                }
            }
            smelt_hir::DictProjectionOp::Symbols => {
                let Some(Type::Dict(key_ty, _) | Type::JsMap(key_ty, _)) =
                    self.mir.types.get(self.operand_ty(dict)?)
                else {
                    return Err(EmitError::new("dict projection receiver must be a dict"));
                };
                if self.mir.types.get(*key_ty) == Some(&Type::String) {
                    Ok(format!(
                        "{dict_text}.keys().filter_map(|key| key.strip_prefix(\"__smelt_symbol:\").map(str::to_owned)).collect::<Vec<_>>()"
                    ))
                } else if self.dict_uses_js_key_map(*key_ty) {
                    Ok(format!(
                        "{dict_text}.keys().filter_map(|key| match key {{ SmeltUnknown::Symbol(value) => Some(value), _ => None }}).collect::<Vec<_>>()"
                    ))
                } else {
                    Ok("Vec::<String>::new()".to_owned())
                }
            }
            smelt_hir::DictProjectionOp::Values => {
                let Some(Type::Dict(key_ty, _) | Type::JsMap(key_ty, _)) =
                    self.mir.types.get(self.operand_ty(dict)?)
                else {
                    return Err(EmitError::new("dict projection receiver must be a dict"));
                };
                if self.mir.types.get(*key_ty) == Some(&Type::String) {
                    if self.dict_uses_smelt_record(*key_ty) || self.dict_uses_js_key_map(*key_ty) {
                        Ok(format!(
                            "{dict_text}.iter().filter(|(key, _)| !key.starts_with(\"__smelt_symbol:\") && key != \"__smelt_class\").map(|(_, value)| value).collect::<Vec<_>>()"
                        ))
                    } else {
                        Ok(format!(
                            "{dict_text}.iter().filter(|(key, _)| !key.starts_with(\"__smelt_symbol:\") && key != \"__smelt_class\").map(|(_, value)| value.clone()).collect::<Vec<_>>()"
                        ))
                    }
                } else if self.dict_uses_js_key_map(*key_ty) {
                    Ok(format!(
                        "{dict_text}.iter().filter(|(key, _)| !matches!(key, SmeltUnknown::Symbol(_))).map(|(_, value)| value).collect::<Vec<_>>()"
                    ))
                } else {
                    Ok(format!("{dict_text}.values().cloned().collect::<Vec<_>>()"))
                }
            }
            smelt_hir::DictProjectionOp::Entries => {
                let Some(Type::Dict(key_ty, _) | Type::JsMap(key_ty, _)) =
                    self.mir.types.get(self.operand_ty(dict)?)
                else {
                    return Err(EmitError::new("dict projection receiver must be a dict"));
                };
                if self.mir.types.get(*key_ty) == Some(&Type::String) {
                    if self.dict_uses_smelt_record(*key_ty) || self.dict_uses_js_key_map(*key_ty) {
                        Ok(format!(
                            "{dict_text}.iter().filter(|(key, _)| !key.starts_with(\"__smelt_symbol:\") && key != \"__smelt_class\").collect::<Vec<_>>()"
                        ))
                    } else {
                        Ok(format!(
                            "{dict_text}.iter().filter(|(key, _)| !key.starts_with(\"__smelt_symbol:\") && key != \"__smelt_class\").map(|(key, value)| (key.clone(), value.clone())).collect::<Vec<_>>()"
                        ))
                    }
                } else if self.dict_uses_js_key_map(*key_ty) {
                    Ok(format!(
                        "{dict_text}.iter().filter(|(key, _)| !matches!(key, SmeltUnknown::Symbol(_))).collect::<Vec<_>>()"
                    ))
                } else {
                    Ok(format!(
                        "{dict_text}.iter().map(|(key, value)| (key.clone(), value.clone())).collect::<Vec<_>>()"
                    ))
                }
            }
        }
    }

    /// Converts a JSON serialization operation to Rust text.
    ///
    /// The Serde JSON backend is intentionally confined to this helper and
    /// Cargo dependency injection, making it replaceable without changing HIR
    /// or frontend lowering.
    /// Converts a JSON serialization operation to Rust text.
    ///
    /// The Serde JSON backend is intentionally confined to this helper and
    /// Cargo dependency injection, making it replaceable without changing HIR
    /// or frontend lowering.
    pub(super) fn json_stringify_text(
        &self,
        value: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        if !matches!(self.mir.types.get(dest_ty), Some(Type::String)) {
            return Err(EmitError::new("JSON stringify destination must be string"));
        }
        if !self.is_json_serializable_type(self.operand_ty(value)?) {
            return Err(EmitError::new(
                "JSON stringify value must be JSON-serializable",
            ));
        }
        Ok(format!(
            "serde_json::to_string(&{}).expect(\"JSON serialization failed\")",
            self.operand_text(value)?
        ))
    }

    /// Converts a JSON parse operation to Rust text.
    ///
    /// Serde stays behind this helper so future backend changes do not affect
    /// the frontend lowering shape.
    /// Converts a JSON parse operation to Rust text.
    ///
    /// Serde stays behind this helper so future backend changes do not affect
    /// the frontend lowering shape.
    pub(super) fn json_parse_text(
        &self,
        text: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(text)?),
            Some(Type::String)
        ) {
            return Err(EmitError::new("JSON parse input must be a string"));
        }
        if !self.is_json_serializable_type(dest_ty) {
            return Err(EmitError::new(
                "JSON parse destination must be JSON-compatible",
            ));
        }
        // `JSON.parse` yields a dynamic JS value. Record/map destinations lower
        // to `SmeltRecord`/`SmeltJsMap`, which do not implement `Deserialize`,
        // so deserializing directly into them fails (was E0277 in `isJSON`).
        // Parse into the erased `SmeltUnknown` (which is `Deserialize`) and then
        // run the ordinary coercion into the concrete destination.
        if matches!(self.mir.types.get(dest_ty), Some(Type::Dict(_, _) | Type::JsMap(_, _))) {
            let parsed = format!(
                "serde_json::from_str::<SmeltUnknown>(&{}).expect(\"JSON parse failed\")",
                self.operand_text(text)?
            );
            return self.extract_value_text(&parsed, dest_ty);
        }
        Ok(format!(
            "serde_json::from_str::<{}>(&{}).expect(\"JSON parse failed\")",
            self.type_text(dest_ty)?,
            self.operand_text(text)?
        ))
    }

    // Returns whether a type is supported by the current JSON serializer path.
}
