//! Place emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Converts a place to its Rust text representation.
    pub(super) fn place_text(&self, place: &Place) -> Result<String, EmitError> {
        match place {
            Place::Local(local) => self.local_value_text(*local),
            Place::Field { base, field } => {
                let base_ty = self.local_decl(*base)?.ty;
                if let Some(Type::Dict(key, value)) = self.mir.types.get(base_ty) {
                    let field_name = self.symbol_source_name(*field)?;
                    let key_text = if self.mir.types.get(*key) == Some(&Type::String) {
                        format!("{field_name:?}.to_owned()")
                    } else if matches!(
                        self.mir.types.get(*key),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    ) || self.is_erased_class_type(*key)
                    {
                        self.erase_value_text(
                            &format!("{field_name:?}.to_owned()"),
                            self.type_id(Type::String)?,
                        )?
                    } else {
                        self.default_value(*key)?
                    };
                    let base_text = self.local_value_text(*base)?;
                    if matches!(self.mir.types.get(*value), Some(Type::Optional(_))) {
                        if self.dict_uses_smelt_record(*key) || self.dict_uses_js_key_map(*key) {
                            return Ok(format!("{base_text}.get(&{key_text}).flatten()"));
                        }
                        return Ok(format!("{base_text}.get(&{key_text}).cloned().flatten()"));
                    }
                    if self.mir.types.get(*key) == Some(&Type::String)
                        && self.type_text_with_impl_trait(*value, false)? == "SmeltUnknown"
                    {
                        return Ok(format!(
                            "{base_text}.get(&{key_text}).unwrap_or(SmeltUnknown::Null)"
                        ));
                    }
                    if self.dict_uses_smelt_record(*key) || self.dict_uses_js_key_map(*key) {
                        return Ok(format!(
                            "{base_text}.get(&{key_text}).expect(\"missing field\")"
                        ));
                    }
                    return Ok(format!(
                        "{base_text}.get(&{key_text}).cloned().expect(\"missing field\")"
                    ));
                }
                if matches!(
                    self.mir.types.get(base_ty),
                    Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
                ) || self.is_erased_class_type(base_ty)
                {
                    let field_name = self.symbol_source_name(*field)?;
                    let field_rule = smelt_stdlib::typescript_field_rule(field_name);
                    let base_text = self.local_value_text(*base)?;
                    // A concrete-union receiver stores a tagged enum; project it
                    // back to `SmeltUnknown` before the erased-object field match.
                    let scrutinee =
                        self.erase_concrete_union_text(&format!("{base_text}.clone()"), base_ty);
                    if field_rule == Some(smelt_stdlib::FieldRule::TsLength) {
                        return Ok(format!(
                            "match {scrutinee} {{ SmeltUnknown::String(value) => SmeltUnknown::Number(value.chars().count() as f64), SmeltUnknown::Array(value) => SmeltUnknown::Number(value.len() as f64), SmeltUnknown::Object(map) => smelt_get_object_field(&map, \"length\"), _ => SmeltUnknown::Null }}"
                        ));
                    }
                    if field_rule == Some(smelt_stdlib::FieldRule::TsSort) {
                        return Ok(format!(
                            "match {scrutinee} {{ SmeltUnknown::Array(value) => smelt_array_sort_method(value), SmeltUnknown::Object(map) => smelt_get_object_field(&map, \"sort\"), _ => SmeltUnknown::Null }}"
                        ));
                    }
                    // `AbortController`/`AbortSignal` methods are surfaced as
                    // runtime-helper-bound closures that mutate the shared abort
                    // record (see `smelt_abort_method`). Plain data fields
                    // (`signal`, `aborted`, ...) keep the ordinary erased-object
                    // read; the helper only intercepts a method read when the
                    // receiver actually carries an abort marker.
                    if matches!(
                        field_name,
                        "abort"
                            | "addEventListener"
                            | "removeEventListener"
                            | "dispatchEvent"
                            | "throwIfAborted"
                    ) {
                        return Ok(format!(
                            "match {scrutinee} {{ SmeltUnknown::Object(map) if map.contains_key(\"__smelt_abortcontroller\") || map.contains_key(\"__smelt_abortsignal\") => smelt_abort_method(map, {field_name:?}), SmeltUnknown::Object(map) => smelt_get_object_field(&map, {field_name:?}), _ => SmeltUnknown::Undefined }}"
                        ));
                    }
                    return Ok(format!(
                        "match {scrutinee} {{ SmeltUnknown::Object(map) => smelt_get_object_field(&map, {field_name:?}), _ => SmeltUnknown::Undefined }}"
                    ));
                }
                if let Some(Type::Optional(inner)) = self.mir.types.get(base_ty)
                    && (matches!(
                        self.mir.types.get(*inner),
                        Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
                    ) || self.is_erased_class_type(*inner))
                {
                    let field_name = self.symbol_source_name(*field)?;
                    let base_text = self.local_value_text(*base)?;
                    if smelt_stdlib::typescript_field_rule(field_name)
                        == Some(smelt_stdlib::FieldRule::TsLength)
                    {
                        return Ok(format!(
                            "match {base_text}.clone().unwrap_or(SmeltUnknown::Null) {{ SmeltUnknown::String(value) => SmeltUnknown::Number(value.chars().count() as f64), SmeltUnknown::Array(value) => SmeltUnknown::Number(value.len() as f64), SmeltUnknown::Object(map) => smelt_get_object_field(&map, \"length\"), _ => SmeltUnknown::Null }}"
                        ));
                    }
                    return Ok(format!(
                        "match {base_text}.clone().unwrap_or(SmeltUnknown::Undefined) {{ SmeltUnknown::Object(map) => smelt_get_object_field(&map, {field_name:?}), _ => SmeltUnknown::Undefined }}"
                    ));
                }
                if let Some(Type::Optional(inner)) = self.mir.types.get(base_ty)
                    && let Some(Type::Dict(key_ty, _)) = self.mir.types.get(*inner)
                    && self.mir.types.get(*key_ty) == Some(&Type::String)
                {
                    let field_name = self.symbol_source_name(*field)?;
                    let base_text = self.local_value_text(*base)?;
                    return Ok(format!(
                        "{base_text}.as_ref().and_then(|_smelt_value| _smelt_value.get({field_name:?}))"
                    ));
                }
                if let Some(Type::Optional(inner)) = self.mir.types.get(base_ty)
                    && self.symbol_source_name(*field)? == "value"
                {
                    let wrapped = self.erase_value_text("value", *inner)?;
                    return Ok(format!(
                        "{}.clone().map_or(SmeltUnknown::Undefined, |value| {wrapped})",
                        self.local_value_text(*base)?
                    ));
                }
                if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
                    && self.is_regexp_class_symbol(*name)?
                {
                    return self.regexp_field_text(&self.local_value_text(*base)?, *field);
                }
                if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
                    && let Some(kind) = self.match_class_kind(*name)?
                {
                    return self.match_field_text(&self.local_value_text(*base)?, kind, *field);
                }
                if let Some(Type::Optional(inner)) = self.mir.types.get(base_ty)
                    && let Some(fields) = self.structural_record_fields(*inner)
                    && let Some(field_ty) = fields
                        .iter()
                        .find(|candidate| candidate.name == *field)
                        .map(|candidate| candidate.ty)
                {
                    let base_text = self.local_value_text(*base)?;
                    let field_name = sanitize_ident(self.symbol_name(*field)?);
                    return if matches!(self.mir.types.get(field_ty), Some(Type::Optional(_))) {
                        Ok(format!(
                            "{base_text}.as_ref().and_then(|_smelt_value| _smelt_value.{field_name}.clone())"
                        ))
                    } else {
                        Ok(format!(
                            "{base_text}.as_ref().map(|_smelt_value| _smelt_value.{field_name}.clone())"
                        ))
                    };
                }
                if matches!(self.mir.types.get(base_ty), Some(Type::Function(_))) {
                    return Ok(self.null_value_text());
                }
                if smelt_stdlib::typescript_field_rule(self.symbol_source_name(*field)?)
                    == Some(smelt_stdlib::FieldRule::TsConstructor)
                {
                    return Ok(self.null_value_text());
                }
                if matches!(self.mir.types.get(base_ty), Some(Type::String)) {
                    return self.string_field_text(&self.local_value_text(*base)?, *field);
                }
                if let Some(getter) = self.descriptor_getter_text(*base, *field)? {
                    return Ok(getter);
                }
                if self.storage_field_is_function(base_ty, *field) {
                    return Ok(format!(
                        "{}.{}.clone()",
                        self.local_value_text(*base)?,
                        sanitize_ident(self.symbol_name(*field)?)
                    ));
                }
                if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
                    && let Some(method_text) = self.class_method_reference_text(
                        &self.local_value_text(*base)?,
                        *name,
                        *field,
                    )?
                {
                    return Ok(method_text);
                }
                if let Some(Type::Optional(inner)) = self.mir.types.get(base_ty)
                    && matches!(self.mir.types.get(*inner), Some(Type::Function(_)))
                {
                    return Ok(self.null_value_text());
                }
                // A dotted read of an UNDECLARED member on an index-signature
                // class (`bag.name` where `name` is not a struct field) is a
                // keyed lookup into the runtime store (issue #84). Declared
                // fields keep their concrete struct access via the fallback
                // below; only names with no matching field route to the store.
                if let Some((_key_ty, value_ty)) = self.class_index_store_types(base_ty)
                    && !self.class_has_named_field(base_ty, *field)
                {
                    let base_text = self.local_value_text(*base)?;
                    let key = self.symbol_source_name(*field)?;
                    let store_text =
                        format!("{base_text}.{}", smelt_hir::CLASS_INDEX_STORE_FIELD);
                    let default_value = self.default_value(value_ty)?;
                    let string_key_ty = self.type_id(Type::String)?;
                    let getter = if self.dict_uses_smelt_record(string_key_ty)
                        || self.dict_uses_js_key_map(string_key_ty)
                    {
                        format!("{store_text}.get(&{key:?}.to_owned())")
                    } else {
                        format!("{store_text}.get(&{key:?}.to_owned()).cloned()")
                    };
                    return Ok(format!("{getter}.unwrap_or({default_value})"));
                }
                // A declared field of a reference class lives inside the shared
                // cell. Read it through a narrow `borrow()` and clone the value
                // out so the borrow guard is a short-lived temporary that ends
                // with the statement — never held across a re-entrant call.
                if self.is_reference_class_type(base_ty)
                    && self.class_has_named_field(base_ty, *field)
                {
                    return Ok(format!(
                        "{}.0.borrow().{}.clone()",
                        self.local_value_text(*base)?,
                        sanitize_ident(self.symbol_name(*field)?)
                    ));
                }
                Ok(format!(
                    "{}.{}",
                    self.local_value_text(*base)?,
                    sanitize_ident(self.symbol_name(*field)?)
                ))
            }
            Place::Index { base, index } => {
                let base_ty = self.local_decl(*base)?.ty;
                if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
                    && self.is_match_class_symbol(*name)?
                {
                    return self.match_index_text(&self.local_value_text(*base)?, index);
                }
                match self.mir.types.get(base_ty) {
                    Some(Type::List(item_ty)) => {
                        let base_text = self.local_mut_value_text(*base)?;
                        let index_text =
                            self.normalized_index_text(&format!("{base_text}.len()"), index)?;
                        // JS out-of-bounds element access is `undefined`, not
                        // `null`. A type parameter that is in scope for the
                        // current generic function is a real Rust generic, so
                        // its missing value is `Default::default()` (a `T`), not
                        // the erased `SmeltUnknown::Undefined` used for genuinely
                        // erased element types.
                        let item_is_in_scope_type_param = matches!(
                            self.mir.types.get(*item_ty),
                            Some(Type::TypeParam { name })
                                if self.current_function_has_type_param(*name)
                        );
                        let missing = if item_is_in_scope_type_param {
                            self.default_value(*item_ty)?
                        } else if matches!(
                            self.mir.types.get(*item_ty),
                            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                        ) {
                            "SmeltUnknown::Undefined".to_owned()
                        } else {
                            self.default_value(*item_ty)?
                        };
                        Ok(format!(
                            "{base_text}.get({index_text}).cloned().unwrap_or({missing})"
                        ))
                    }
                    Some(Type::Optional(inner_ty))
                        if matches!(self.mir.types.get(*inner_ty), Some(Type::List(_))) =>
                    {
                        let Some(Type::List(item_ty)) = self.mir.types.get(*inner_ty) else {
                            return Ok(self.null_value_text());
                        };
                        let base_text = self.local_value_text(*base)?;
                        let index_text = self.normalized_index_text(
                            &format!("{base_text}.as_ref().map_or(0, Vec::len)"),
                            index,
                        )?;
                        let access =
                            if matches!(self.mir.types.get(*item_ty), Some(Type::Optional(_))) {
                                "values.get({index_text}).cloned().flatten()"
                            } else {
                                "values.get({index_text}).cloned()"
                            };
                        Ok(format!(
                            "{base_text}.as_ref().and_then(|values| {})",
                            access.replace("{index_text}", &index_text)
                        ))
                    }
                    Some(Type::Dict(key_ty, value_ty)) => {
                        let key_text = if self.mir.types.get(*key_ty) == Some(&Type::String) {
                            let source_key = self.operand_ty(index)?;
                            let index_text = self.operand_text(index)?;
                            self.property_key_to_string_text(&index_text, source_key)?
                        } else {
                            self.value_at_type(index, *key_ty)?
                        };
                        let base_text = self.local_value_text(*base)?;
                        let default_value = self.default_value(*value_ty)?;
                        let value_is_unknownish = matches!(
                            self.mir.types.get(*value_ty),
                            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                        );
                        if (self.dict_uses_smelt_record(*key_ty)
                            || self.dict_uses_js_key_map(*key_ty))
                            && value_is_unknownish
                        {
                            Ok(format!(
                                "{base_text}.get(&{key_text}).cloned().unwrap_or(SmeltUnknown::Null)"
                            ))
                        } else if self.dict_uses_smelt_record(*key_ty)
                            || self.dict_uses_js_key_map(*key_ty)
                        {
                            Ok(format!(
                                "{base_text}.get(&{key_text}).unwrap_or({default_value})"
                            ))
                        } else if value_is_unknownish {
                            Ok(format!(
                                "{base_text}.get(&{key_text}).cloned().unwrap_or(SmeltUnknown::Null)"
                            ))
                        } else {
                            Ok(format!(
                                "{base_text}.get(&{key_text}).cloned().unwrap_or({default_value})"
                            ))
                        }
                    }
                    Some(Type::String) => {
                        let base_text = self.local_value_text(*base)?;
                        let index_text = self.normalized_index_text(
                            &format!("{base_text}.chars().count()"),
                            index,
                        )?;
                        Ok(format!(
                            "{base_text}.chars().nth({index_text}).map(|ch| ch.to_string()).expect(\"index out of bounds\")"
                        ))
                    }
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => {
                        self.unknown_index_text(&self.local_value_text(*base)?, index)
                    }
                    Some(Type::Tuple(items)) => {
                        let tuple_index = self.tuple_index(index, items.len())?;
                        Ok(format!("{}.{tuple_index}", self.local_value_text(*base)?))
                    }
                    // A class with an index signature backs keyed reads with a
                    // real store field (issue #84). A non-optional keyed read
                    // (declared value type has no `undefined`) reads the store by
                    // key, defaulting a missing key to the value type's default.
                    Some(Type::Class { .. })
                        if self.class_index_store_types(base_ty).is_some() =>
                    {
                        let (key_ty, value_ty) = self
                            .class_index_store_types(base_ty)
                            .ok_or_else(|| EmitError::new("class index store types missing"))?;
                        let base_text = self.local_value_text(*base)?;
                        let store_text =
                            format!("{base_text}.{}", smelt_hir::CLASS_INDEX_STORE_FIELD);
                        let optional_read =
                            self.dict_index_optional_read_text(&store_text, key_ty, index)?;
                        let default_value = self.default_value(value_ty)?;
                        Ok(format!("{optional_read}.unwrap_or({default_value})"))
                    }
                    _ => Ok(self.null_value_text()),
                }
            }
        }
    }

    /// Emits a descriptor getter invocation for one statically known class field.
    pub(super) fn descriptor_getter_text(
        &self,
        base: LocalId,
        field: Symbol,
    ) -> Result<Option<String>, EmitError> {
        let base_ty = self.local_decl(base)?.ty;
        let Some((owner, descriptor)) = self.descriptor_for_field(base_ty, field) else {
            return Ok(None);
        };
        let Some(getter_id) = descriptor.getter else {
            return Err(EmitError::new(
                "materialized descriptor read has no source getter",
            ));
        };
        let getter = self
            .mir
            .functions
            .get(usize::try_from(getter_id.0).unwrap_or(usize::MAX))
            .ok_or_else(|| EmitError::new("descriptor getter function is missing"))?;
        let HirOrigin::ClassMethod {
            class: getter_class,
            method: method_symbol,
            ..
        } = getter.origin
        else {
            return Err(EmitError::new(
                "descriptor getter did not lower as a class method",
            ));
        };
        let method_name = sanitize_ident(self.symbol_name(method_symbol)?);
        let base_text = self.local_value_text(base)?;
        if getter_class == owner.name {
            return Ok(Some(format!("{base_text}.{method_name}()")));
        }
        let descriptor_value = self.descriptor_value_text(getter_class, descriptor)?;
        let arguments = getter
            .params
            .iter()
            .skip(1)
            .enumerate()
            .map(|(index, _)| {
                if index == 0 {
                    format!("{base_text}.clone()")
                } else {
                    "Default::default()".to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        Ok(Some(format!(
            "{descriptor_value}.{method_name}({arguments})"
        )))
    }

    /// Emits a descriptor setter statement for one statically known class field.
    pub(super) fn descriptor_setter_statement(
        &self,
        base: LocalId,
        field: Symbol,
        value: &Rvalue,
    ) -> Result<Option<String>, EmitError> {
        let base_ty = self.local_decl(base)?.ty;
        let Some((owner, descriptor)) = self.descriptor_for_field(base_ty, field) else {
            return Ok(None);
        };
        let Some(setter_id) = descriptor.setter else {
            return Err(EmitError::new(
                "materialized descriptor write has no source setter",
            ));
        };
        let setter = self
            .mir
            .functions
            .get(usize::try_from(setter_id.0).unwrap_or(usize::MAX))
            .ok_or_else(|| EmitError::new("descriptor setter function is missing"))?;
        let HirOrigin::ClassMethod {
            class: setter_class,
            method: method_symbol,
            ..
        } = setter.origin
        else {
            return Err(EmitError::new(
                "descriptor setter did not lower as a class method",
            ));
        };
        let write_ty = descriptor
            .write_ty
            .ok_or_else(|| EmitError::new("read-only descriptor cannot be assigned"))?;
        let rendered = self.rvalue_text_for_dest(value, write_ty)?;
        let method_name = sanitize_ident(self.symbol_name(method_symbol)?);
        let base_text = self.local_mut_value_text(base)?;
        if setter_class == owner.name {
            return Ok(Some(format!("{base_text}.{method_name}({rendered});")));
        }
        let descriptor_value = self.descriptor_value_text(setter_class, descriptor)?;
        let mut arguments = setter
            .params
            .iter()
            .skip(1)
            .enumerate()
            .map(|(index, parameter)| {
                if index == 0 {
                    if self.parameter_needs_mutable_reference_in(setter, *parameter) {
                        format!("&mut {base_text}")
                    } else {
                        format!("{base_text}.clone()")
                    }
                } else {
                    "Default::default()".to_owned()
                }
            })
            .collect::<Vec<_>>();
        if let Some(last) = arguments.last_mut() {
            rendered.clone_into(last);
        }
        Ok(Some(format!(
            "{descriptor_value}.{method_name}({});",
            arguments.join(", ")
        )))
    }

    /// Finds a descriptor through the concrete class inheritance chain.
    pub(super) fn descriptor_for_field(
        &self,
        ty: TypeId,
        field: Symbol,
    ) -> Option<(&MirClass, &MirDescriptor)> {
        let Type::Class { name, .. } = self.mir.types.get(ty)? else {
            return None;
        };
        let class = self.mir.classes.iter().find(|class| class.name == *name)?;
        if let Some(descriptor) = class
            .descriptors
            .iter()
            .find(|descriptor| descriptor.name == field)
        {
            let instance_storage_shadows = !descriptor.data_descriptor
                && crate::classes::effective_class_fields(self.mir, class)
                    .iter()
                    .any(|candidate| candidate.name == field);
            if !instance_storage_shadows {
                return Some((class, descriptor));
            }
        }
        let base = class.base?;
        let base_ty = self
            .mir
            .types
            .all()
            .iter()
            .position(|candidate| {
                matches!(candidate, Type::Class { name: candidate_name, .. } if *candidate_name == base)
            })
            .and_then(|index| u32::try_from(index).ok())
            .map(TypeId)?;
        self.descriptor_for_field(base_ty, field)
    }

    /// Constructs concrete static descriptor state as a Rust value.
    fn descriptor_value_text(
        &self,
        class_symbol: Symbol,
        descriptor: &MirDescriptor,
    ) -> Result<String, EmitError> {
        let descriptor_class = self
            .mir
            .classes
            .iter()
            .find(|candidate| candidate.name == class_symbol)
            .ok_or_else(|| EmitError::new("descriptor class is not materialized"))?;
        let class_name = crate::classes::class_name_text(self.mir, descriptor_class)?;
        let fields = descriptor
            .value_fields
            .iter()
            .map(|field| {
                Ok(format!(
                    "{}: {}",
                    sanitize_ident(self.symbol_name(field.name)?),
                    descriptor_literal_text(&field.value)
                ))
            })
            .collect::<Result<Vec<_>, EmitError>>()?;
        if fields.is_empty() {
            Ok(format!("{class_name}::default()"))
        } else {
            Ok(format!(
                "{class_name} {{ {}, ..Default::default() }}",
                fields.join(", ")
            ))
        }
    }

    /// Converts a place to its Rust text representation for assignment.
    pub(super) fn assignment_place_text(&self, place: &Place) -> Result<String, EmitError> {
        match place {
            Place::Local(local) => self.local_mut_value_text(*local),
            Place::Index { base, index } => {
                let base_ty = self.local_decl(*base)?.ty;
                match self.mir.types.get(base_ty) {
                    Some(Type::List(_)) => {
                        let base_text = self.local_mut_value_text(*base)?;
                        let index_text =
                            self.normalized_index_text(&format!("{base_text}.len()"), index)?;
                        Ok(format!("{base_text}[{index_text}]"))
                    }
                    Some(Type::Dict(key_ty, _)) => {
                        let key_text = if self.mir.types.get(*key_ty) == Some(&Type::String) {
                            let source_key = self.operand_ty(index)?;
                            let index_text = self.operand_text(index)?;
                            self.property_key_to_string_text(&index_text, source_key)?
                        } else {
                            self.value_at_type(index, *key_ty)?
                        };
                        Ok(format!(
                            "*{}.get_mut(&{key_text}).expect(\"index out of bounds\")",
                            self.local_mut_value_text(*base)?
                        ))
                    }
                    Some(Type::Tuple(items)) => {
                        let tuple_index = self.tuple_index(index, items.len())?;
                        Ok(format!(
                            "{}.{tuple_index}",
                            self.local_mut_value_text(*base)?
                        ))
                    }
                    _ => Err(EmitError::new(
                        "index assignment codegen is only implemented for lists, dicts, and tuples",
                    )),
                }
            }
            Place::Field { base, field } => {
                let base_ty = self.local_decl(*base)?.ty;
                // A declared field of a reference class is written through a
                // narrow `borrow_mut()`. The statement's right-hand side has
                // already been reduced to an operand by MIR temping, so the
                // mutable borrow never spans a re-entrant call. Checked before
                // the structural-record path because a reference class is still
                // record-shaped structurally.
                if self.is_reference_class_type(base_ty)
                    && self.class_has_named_field(base_ty, *field)
                {
                    return Ok(format!(
                        "{}.0.borrow_mut().{}",
                        self.local_value_text(*base)?,
                        sanitize_ident(self.symbol_name(*field)?)
                    ));
                }
                if self.structural_record_fields(base_ty).is_some() {
                    return Ok(format!(
                        "{}.{}",
                        self.local_mut_value_text(*base)?,
                        sanitize_ident(self.symbol_name(*field)?)
                    ));
                }
                self.place_text(place)
            }
        }
    }

    /// Resolves a statically known tuple index for Rust field access.
    pub(super) fn tuple_index(&self, index: &Operand, len: usize) -> Result<usize, EmitError> {
        let value = match index {
            Operand::Const(Constant::Int(value)) => usize::try_from(*value).ok(),
            _ => None,
        }
        .ok_or_else(|| EmitError::new("tuple index must be a non-negative constant integer"))?;
        if value >= len {
            return Err(EmitError::new("tuple index is out of bounds"));
        }
        Ok(value)
    }

    /// Gets the type of a place.
    /// Converts a Python-style element index into a Rust `usize` expression.
    ///
    /// Negative indexes are offset from the collection length. Bounds are not
    /// clamped because Python element indexing raises when the normalized index
    /// is still outside the collection; the generated Rust keeps that behavior
    /// with `expect` on negative conversion and the eventual indexed lookup.
    pub(super) fn normalized_index_text(
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
            "{{ let len = {len_expr} as i64; let index = {index_text} as i64; let normalized = if index < 0 {{ len + index }} else {{ index }}; usize::try_from(normalized).expect(\"negative index out of bounds\") }}"
        ))
    }

    /// Emit a runtime index read for values whose concrete shape is erased.
    ///
    /// TypeScript generic and unknown receivers may still be strings, arrays,
    /// or objects at runtime. Returning `Null` here hides lowering bugs and
    /// breaks later casts, so the generated Rust dispatches on `SmeltUnknown`
    /// and panics only when the runtime value is not indexable.
    pub(super) fn unknown_index_text(
        &self,
        base_text: &str,
        index: &Operand,
    ) -> Result<String, EmitError> {
        let index_ty = self.operand_ty(index)?;
        let index_text = self.operand_text(index)?;
        let key_text = self.property_key_to_string_text(&index_text, index_ty)?;
        if matches!(self.mir.types.get(index_ty), Some(Type::String)) {
            return Ok(format!(
                r#"match {base_text}.clone() {{
                    SmeltUnknown::String(value) => {{
                        let smelt_key = {key_text};
                        if smelt_key == "length" {{
                            SmeltUnknown::Number(value.chars().count() as f64)
                        }} else {{
                            smelt_key.parse::<usize>().ok().and_then(|index| value.chars().nth(index).map(|ch| SmeltUnknown::String(ch.to_string()))).unwrap_or(SmeltUnknown::Null)
                        }}
                    }}
                    SmeltUnknown::Array(values) => {{
                        let smelt_key = {key_text};
                        if smelt_key == "length" {{
                            SmeltUnknown::Number(values.len() as f64)
                        }} else {{
                            smelt_key.parse::<usize>().ok().and_then(|index| values.get(index).cloned()).unwrap_or(SmeltUnknown::Null)
                        }}
                    }}
                    SmeltUnknown::Object(values) => values.get(&{key_text}).unwrap_or(SmeltUnknown::Null),
                    _ => SmeltUnknown::Null,
                }}"#
            ));
        }
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
                let scrutinee =
                    self.erase_concrete_union_text(&format!("{index_text}.clone()"), index_ty);
                format!(
                    "match {scrutinee} {{ SmeltUnknown::Number(value) => value, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }}"
                )
            }
            _ => "f64::NAN".to_owned(),
        };

        Ok(format!(
            r"match {base_text}.clone() {{
                    SmeltUnknown::String(value) => {{
                        let len = value.chars().count() as i64;
                        let index = {numeric_index_text} as i64;
                        let normalized = if index < 0 {{ len + index }} else {{ index }};
                        usize::try_from(normalized).ok().and_then(|index| value.chars().nth(index).map(|ch| SmeltUnknown::String(ch.to_string()))).unwrap_or(SmeltUnknown::Null)
                    }}
                    SmeltUnknown::Array(values) => {{
                        let len = values.len() as i64;
                        let index = {numeric_index_text} as i64;
                        let normalized = if index < 0 {{ len + index }} else {{ index }};
                        usize::try_from(normalized).ok().and_then(|index| values.get(index).cloned()).unwrap_or(SmeltUnknown::Null)
                    }}
                SmeltUnknown::Object(values) => values.get(&{key_text}).unwrap_or(SmeltUnknown::Null),
                _ => SmeltUnknown::Null,
            }}"
        ))
    }

    // Unknown/runtime type helpers continue in `unknown.rs`.
}

/// Converts a statically materialized descriptor literal to Rust source.
fn descriptor_literal_text(literal: &smelt_hir::Literal) -> String {
    match literal {
        smelt_hir::Literal::Bool(boolean) => boolean.to_string(),
        smelt_hir::Literal::Int(integer) => integer.to_string(),
        smelt_hir::Literal::Float(number) => {
            if number.is_nan() {
                "f64::NAN".to_owned()
            } else if number.is_infinite() && number.is_sign_positive() {
                "f64::INFINITY".to_owned()
            } else if number.is_infinite() {
                "f64::NEG_INFINITY".to_owned()
            } else {
                format!("{number:?}")
            }
        }
        smelt_hir::Literal::String(text) => format!("{text:?}.to_owned()"),
        smelt_hir::Literal::Symbol(_)
        | smelt_hir::Literal::Undefined
        | smelt_hir::Literal::None => "Default::default()".to_owned(),
    }
}
