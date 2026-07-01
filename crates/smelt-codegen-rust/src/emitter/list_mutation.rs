//! List Mutation emission helpers.

use super::*;

/// Source location for a list local copied out of a mutable JavaScript property.
#[derive(Clone)]
enum ListAliasOrigin {
    /// A statically named field on an erased object.
    Field {
        /// Object local that owns the aliased field.
        base: LocalId,
        /// Static field name copied into the local list alias.
        field: Symbol,
    },
    /// A dynamic dictionary entry.
    Index {
        /// Dictionary local that owns the aliased entry.
        base: LocalId,
        /// Dynamic key operand copied into the local list alias.
        index: Box<Operand>,
    },
}

impl FunctionEmitter<'_> {
    /// Converts a list push operation to Rust text.
    pub(super) fn list_push_text(
        &self,
        list: &Operand,
        item: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list push receiver must be a list"));
        };
        let returns_length = match self.mir.types.get(dest_ty) {
            Some(Type::Float) => true,
            Some(Type::None) => false,
            _ => {
                return Err(EmitError::new(
                    "list push destination must be number or None",
                ));
            }
        };
        if let Operand::Copy(Place::Field { base, field })
        | Operand::Move(Place::Field { base, field }) = list
            && matches!(
                self.mir.types.get(self.local_decl(*base)?.ty),
                Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
            )
        {
            let base_text = self.local_mut_value_text(*base)?;
            let field_name = self.symbol_name(*field)?;
            let item_text = self.value_at_type(item, *item_ty)?;
            let result = if returns_length {
                "smelt_list.len() as f64"
            } else {
                "()"
            };
            return Ok(format!(
                "{{ let mut smelt_list = match {base_text}.clone() {{ SmeltUnknown::Object(map) => match smelt_get_object_field(&map, \"{field_name}\") {{ SmeltUnknown::Array(values) => values.into_vec(), _ => Vec::new() }}, _ => Vec::new() }}; smelt_list.push(({item_text}).into_smelt_unknown()); let smelt_result = {result}; let smelt_value = SmeltUnknown::Array(smelt_list.into()); match &mut {base_text} {{ SmeltUnknown::Object(map) => {{ map.insert(\"{field_name}\".to_owned(), smelt_value); }}, other => {{ let map = SmeltObject::new(::std::collections::HashMap::from([(\"{field_name}\".to_owned(), smelt_value)])); *other = SmeltUnknown::Object(map); }} }} smelt_result }}"
            ));
        }
        if let Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) = list
            && let Some(ListAliasOrigin::Field { base, field }) = self.list_alias_origin(*local)
            && self.list_alias_base_is_erased_object(base)?
        {
            let list_text = self.local_mut_value_text(*local)?;
            let base_text = self.local_mut_value_text(base)?;
            let field_name = self.symbol_name(field)?;
            let item_text = self.value_at_type(item, *item_ty)?;
            let result = if returns_length {
                format!("{list_text}.len() as f64")
            } else {
                "()".to_owned()
            };
            return Ok(format!(
                "{{ {list_text}.push({item_text}); let smelt_result = {result}; let smelt_value = SmeltUnknown::Array({list_text}.clone().into_iter().map(IntoSmeltUnknown::into_smelt_unknown).collect()); match &mut {base_text} {{ SmeltUnknown::Object(map) => {{ map.insert(\"{field_name}\".to_owned(), smelt_value); }}, other => {{ let map = SmeltObject::new(::std::collections::HashMap::from([(\"{field_name}\".to_owned(), smelt_value)])); *other = SmeltUnknown::Object(map); }} }} smelt_result }}"
            ));
        }
        if let Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) = list
            && let Some(ListAliasOrigin::Index { base, index }) = self.list_alias_origin(*local)
            && let Some(Type::Dict(key_ty, value_ty)) =
                self.mir.types.get(self.local_decl(base)?.ty)
            && *value_ty == list_ty
        {
            let list_text = self.local_mut_value_text(*local)?;
            let base_text = self.local_mut_value_text(base)?;
            let key_text = if self.mir.types.get(*key_ty) == Some(&Type::String) {
                let source_key = self.operand_ty(index.as_ref())?;
                let index_text = self.operand_text(index.as_ref())?;
                self.property_key_to_string_text(&index_text, source_key)?
            } else {
                self.value_at_type(index.as_ref(), *key_ty)?
            };
            let item_text = self.value_at_type(item, *item_ty)?;
            let result = if returns_length {
                format!("{list_text}.len() as f64")
            } else {
                "()".to_owned()
            };
            return Ok(format!(
                "{{ {list_text}.push({item_text}); let smelt_result = {result}; {base_text}.insert({key_text}, {list_text}.clone()); smelt_result }}"
            ));
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = list else {
            return Err(EmitError::new(
                "list push receiver must be a mutable local for now",
            ));
        };
        let list_text = self.local_mut_value_text(*local)?;
        let item_text = self.value_at_type(item, *item_ty)?;
        if returns_length {
            Ok(format!(
                "{{ {list_text}.push({item_text}); {list_text}.len() as f64 }}"
            ))
        } else {
            Ok(format!("{{ {list_text}.push({item_text}); () }}"))
        }
    }

    /// Find the mutable property whose array value initialized a local alias.
    fn list_alias_origin(&self, local: LocalId) -> Option<ListAliasOrigin> {
        self.list_alias_origin_inner(local, &mut HashSet::new())
    }

    /// Follow simple local copies until reaching the property read.
    fn list_alias_origin_inner(
        &self,
        local: LocalId,
        seen: &mut HashSet<LocalId>,
    ) -> Option<ListAliasOrigin> {
        if !seen.insert(local) {
            return None;
        }
        let mut origin = None;
        for block in &self.function.blocks {
            for statement in &block.statements {
                let Statement::Assign {
                    dest,
                    value: Rvalue::Use(source),
                } = statement
                else {
                    continue;
                };
                if *dest == local {
                    if origin.is_some() {
                        return None;
                    }
                    origin = match source {
                        Operand::Copy(Place::Field { base, field })
                        | Operand::Move(Place::Field { base, field }) => {
                            Some(ListAliasOrigin::Field {
                                base: *base,
                                field: *field,
                            })
                        }
                        Operand::Copy(Place::Index { base, index })
                        | Operand::Move(Place::Index { base, index }) => {
                            Some(ListAliasOrigin::Index {
                                base: *base,
                                index: index.clone(),
                            })
                        }
                        Operand::Copy(Place::Local(source_local))
                        | Operand::Move(Place::Local(source_local)) => {
                            self.list_alias_origin_inner(*source_local, seen)
                        }
                        Operand::Const(_) => None,
                    };
                }
            }
        }
        origin
    }

    /// Return whether a local stores an erased object whose fields need writeback.
    fn list_alias_base_is_erased_object(&self, base: LocalId) -> Result<bool, EmitError> {
        let base_ty = self.local_decl(base)?.ty;
        Ok(matches!(
            self.mir.types.get(base_ty),
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
        ) || self.is_erased_class_type(base_ty))
    }

    /// Converts a list extend operation to Rust text.
    /// Converts a list extend operation to Rust text.
    pub(super) fn list_extend_text(
        &self,
        list: &Operand,
        other: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        if !matches!(self.mir.types.get(list_ty), Some(Type::List(_))) {
            return Err(EmitError::new("list extend receiver must be a list"));
        }
        if self.operand_ty(other)? != list_ty {
            return Err(EmitError::new(
                "list extend argument must match the receiver list type",
            ));
        }
        let returns_length = match self.mir.types.get(dest_ty) {
            Some(Type::Float) => true,
            Some(Type::None) => false,
            _ => {
                return Err(EmitError::new(
                    "list extend destination must be number or None",
                ));
            }
        };
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = list else {
            return Err(EmitError::new(
                "list extend receiver must be a mutable local for now",
            ));
        };
        let list_text = self.local_mut_value_text(*local)?;
        let other_text = self.operand_text(other)?;
        if returns_length {
            Ok(format!(
                "{{ {list_text}.extend({other_text}.iter().cloned()); {list_text}.len() as f64 }}"
            ))
        } else {
            Ok(format!(
                "{{ {list_text}.extend({other_text}.iter().cloned()); () }}"
            ))
        }
    }

    /// Converts a list insert operation to Rust text.
    ///
    /// Python accepts negative indexes with insertion-before-end behavior. This
    /// direct mapping intentionally rejects negative indexes at runtime until
    /// Python-compatible negative-index lowering is modeled explicitly.
    /// Converts a list insert operation to Rust text.
    ///
    /// Python accepts negative indexes with insertion-before-end behavior. This
    /// direct mapping intentionally rejects negative indexes at runtime until
    /// Python-compatible negative-index lowering is modeled explicitly.
    pub(super) fn list_insert_text(
        &self,
        list: &Operand,
        index: &Operand,
        item: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list insert receiver must be a list"));
        };
        if !matches!(self.mir.types.get(self.operand_ty(index)?), Some(Type::Int)) {
            return Err(EmitError::new("list insert index must be int"));
        }
        if self.operand_ty(item)? != *item_ty {
            return Err(EmitError::new(
                "list insert item must match the list element type",
            ));
        }
        if !matches!(self.mir.types.get(dest_ty), Some(Type::None)) {
            return Err(EmitError::new("list insert destination must be None"));
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = list else {
            return Err(EmitError::new(
                "list insert receiver must be a mutable local for now",
            ));
        };
        let list_text = self.local_mut_value_text(*local)?;
        let index_text = self.operand_text(index)?;
        let item_text = self.operand_text(item)?;
        Ok(format!(
            "{{ let insert_index = usize::try_from({index_text}).expect(\"list insert negative index\"); {list_text}.insert(insert_index, {item_text}); () }}"
        ))
    }

    /// Converts a list unshift operation to Rust text.
    /// Converts a list unshift operation to Rust text.
    pub(super) fn list_unshift_text(
        &self,
        list: &Operand,
        items: &[Operand],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list unshift receiver must be a list"));
        };
        if !matches!(self.mir.types.get(dest_ty), Some(Type::Float)) {
            return Err(EmitError::new("list unshift destination must be number"));
        }
        for item in items {
            if self.operand_ty(item)? != *item_ty {
                return Err(EmitError::new(
                    "list unshift item must match the list element type",
                ));
            }
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = list else {
            return Err(EmitError::new(
                "list unshift receiver must be a mutable local for now",
            ));
        };
        let list_text = self.local_mut_value_text(*local)?;
        let mut statements = Vec::with_capacity(items.len().saturating_add(1));
        for item in items.iter().rev() {
            let item_text = self.operand_text(item)?;
            statements.push(format!("{list_text}.insert(0, {item_text});"));
        }
        statements.push(format!("{list_text}.len() as f64"));
        Ok(format!("{{ {} }}", statements.join(" ")))
    }

    /// Converts a list reverse operation to Rust text.
    /// Converts a list reverse operation to Rust text.
    pub(super) fn list_reverse_text(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        if !matches!(self.mir.types.get(list_ty), Some(Type::List(_))) {
            return Err(EmitError::new("list reverse receiver must be a list"));
        }
        let returns_list = if dest_ty == list_ty {
            true
        } else if matches!(self.mir.types.get(dest_ty), Some(Type::None)) {
            false
        } else {
            return Err(EmitError::new(
                "list reverse destination must be list or None",
            ));
        };
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = list else {
            return Err(EmitError::new(
                "list reverse receiver must be a mutable local for now",
            ));
        };
        let list_text = self.local_mut_value_text(*local)?;
        if returns_list {
            Ok(format!("{{ {list_text}.reverse(); {list_text}.clone() }}"))
        } else {
            Ok(format!("{{ {list_text}.reverse(); () }}"))
        }
    }

    /// Converts a list pop operation to Rust text.
    /// Converts a list pop operation to Rust text.
    pub(super) fn list_pop_text(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list pop receiver must be a list"));
        };
        let returns_optional = match self.mir.types.get(dest_ty) {
            Some(Type::Optional(inner)) if *inner == *item_ty => true,
            _ if dest_ty == *item_ty => false,
            _ => {
                return Err(EmitError::new(
                    "list pop destination must be item or optional item",
                ));
            }
        };
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = list else {
            return Err(EmitError::new(
                "list pop receiver must be a mutable local for now",
            ));
        };
        let list_text = self.local_mut_value_text(*local)?;
        if returns_optional {
            Ok(format!("{list_text}.pop()"))
        } else {
            Ok(format!("{list_text}.pop().expect(\"pop from empty list\")"))
        }
    }

    /// Converts a list shift operation to Rust text.
    /// Converts a list shift operation to Rust text.
    pub(super) fn list_shift_text(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list shift receiver must be a list"));
        };
        if !matches!(self.mir.types.get(dest_ty), Some(Type::Optional(inner)) if *inner == *item_ty)
        {
            return Err(EmitError::new(
                "list shift destination must be an optional item",
            ));
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = list else {
            return Err(EmitError::new(
                "list shift receiver must be a mutable local for now",
            ));
        };
        let list_text = self.local_mut_value_text(*local)?;
        Ok(format!(
            "if {list_text}.is_empty() {{ None }} else {{ Some({list_text}.remove(0)) }}"
        ))
    }

    /// Converts JavaScript iterator `next()` over a lowered list to Rust text.
    pub(super) fn list_next_text(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list next receiver must be a list"));
        };
        if !matches!(self.mir.types.get(dest_ty), Some(Type::Optional(inner)) if *inner == *item_ty)
        {
            return Err(EmitError::new(
                "list next destination must be an optional item",
            ));
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = list else {
            return Err(EmitError::new("list next receiver must be a mutable local"));
        };
        let list_text = self.local_mut_value_text(*local)?;
        Ok(format!(
            "if {list_text}.is_empty() {{ None }} else {{ Some({list_text}.remove(0)) }}"
        ))
    }

    /// Converts a collection clear operation to Rust text.
    /// Converts a collection clear operation to Rust text.
    pub(super) fn collection_clear_text(
        &self,
        collection: &Operand,
        dest_ty: TypeId,
        collection_name: &str,
    ) -> Result<String, EmitError> {
        let collection_ty = self.operand_ty(collection)?;
        let expected_collection = match collection_name {
            "list" => matches!(self.mir.types.get(collection_ty), Some(Type::List(_))),
            "dict" => matches!(self.mir.types.get(collection_ty), Some(Type::Dict(_, _))),
            "set" => matches!(self.mir.types.get(collection_ty), Some(Type::Set(_))),
            _ => false,
        };
        if !expected_collection {
            return Err(EmitError::new(format!(
                "{collection_name} clear receiver has the wrong type"
            )));
        }
        if !matches!(self.mir.types.get(dest_ty), Some(Type::None)) {
            return Err(EmitError::new(format!(
                "{collection_name} clear destination must be None"
            )));
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = collection
        else {
            return Err(EmitError::new(format!(
                "{collection_name} clear receiver must be a mutable local for now"
            )));
        };
        let collection_text = self.local_mut_value_text(*local)?;
        Ok(format!("{{ {collection_text}.clear(); () }}"))
    }

    /// Converts a list copy operation to Rust text.
    /// Converts a list remove operation to Rust text.
    pub(super) fn list_remove_text(
        &self,
        list: &Operand,
        item: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list remove receiver must be a list"));
        };
        if self.operand_ty(item)? != *item_ty {
            return Err(EmitError::new(
                "list remove item must match the list element type",
            ));
        }
        if !matches!(self.mir.types.get(dest_ty), Some(Type::None)) {
            return Err(EmitError::new("list remove destination must be None"));
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = list else {
            return Err(EmitError::new(
                "list remove receiver must be a mutable local for now",
            ));
        };
        let list_text = self.local_mut_value_text(*local)?;
        let item_text = self.operand_text(item)?;
        if self.list_item_uses_same_value_zero(*item_ty) {
            if self.mir.types.get(*item_ty) == Some(&Type::Float) {
                return Ok(format!(
                    "{{ let smelt_needle = {item_text}; let remove_index = {list_text}.iter().position(|item| *item == smelt_needle || (item.is_nan() && smelt_needle.is_nan())).expect(\"list remove missing item\"); {list_text}.remove(remove_index); () }}"
                ));
            }
            return Ok(format!(
                "{{ let smelt_needle = {item_text}; let remove_index = {list_text}.iter().position(|item| item.same_js_key(&smelt_needle)).expect(\"list remove missing item\"); {list_text}.remove(remove_index); () }}"
            ));
        }
        Ok(format!(
            "{{ let remove_index = {list_text}.iter().position(|item| item == &{item_text}).expect(\"list remove missing item\"); {list_text}.remove(remove_index); () }}"
        ))
    }

    /// Converts a list sort operation to Rust text.
    ///
    /// Python callers use a `None` destination and get source-compatible scalar
    /// ordering for the currently supported item types. TypeScript callers use
    /// the list destination, so codegen sorts by stringified item text to match
    /// JavaScript's no-comparator `Array.prototype.sort` behavior.
    pub(super) fn list_sort_text(
        &self,
        list: &Operand,
        comparator: Option<&Operand>,
        key: Option<&Operand>,
        reverse: bool,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list sort receiver must be a list"));
        };
        let element_ty = *item_ty;
        let returns_list = if dest_ty == list_ty {
            true
        } else if matches!(self.mir.types.get(dest_ty), Some(Type::None)) {
            false
        } else {
            return Err(EmitError::new("list sort destination must be list or None"));
        };
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = list else {
            return Err(EmitError::new(
                "list sort receiver must be a mutable local for now",
            ));
        };
        let list_text = self.local_mut_value_text(*local)?;
        let result_text = if returns_list {
            format!("{list_text}.clone()")
        } else {
            "()".to_owned()
        };
        if let Some(comparator_operand) = comparator {
            return self.list_sort_comparator_text(
                comparator_operand,
                element_ty,
                returns_list,
                &list_text,
                &result_text,
            );
        }
        if key.is_some() || reverse {
            let (prefix, closure) = self.list_sort_by_text(key, reverse, element_ty)?;
            return Ok(format!(
                "{{ {prefix}{list_text}.sort_by({closure}); {result_text} }}"
            ));
        }
        match self.mir.types.get(*item_ty) {
            Some(Type::Bool | Type::Int | Type::Float | Type::String) if returns_list => {
                Ok(format!(
                    "{{ {list_text}.sort_by(|left, right| left.to_string().cmp(&right.to_string())); {result_text} }}"
                ))
            }
            Some(Type::Bool | Type::Int | Type::String) => {
                Ok(format!("{{ {list_text}.sort(); {result_text} }}"))
            }
            Some(Type::Float) => Ok(format!(
                "{{ {list_text}.sort_by(|left, right| left.partial_cmp(right).expect(\"list sort incomparable float\")); {result_text} }}"
            )),
            _ => Err(EmitError::new(
                "list sort supports bool, int, float, and string items",
            )),
        }
    }

    /// Converts a JavaScript `Array.prototype.sort` comparator closure to Rust.
    ///
    /// The comparator is a normal closure operand taking two list items and
    /// returning a number, like other list callbacks. The closure is bound once
    /// and invoked per comparison; its numeric result maps to `Ordering` with
    /// JavaScript semantics (negative -> `Less`, positive -> `Greater`, zero and
    /// `NaN` -> `Equal`).
    fn list_sort_comparator_text(
        &self,
        comparator: &Operand,
        element_ty: TypeId,
        returns_list: bool,
        list_text: &str,
        result_text: &str,
    ) -> Result<String, EmitError> {
        if !returns_list {
            return Err(EmitError::new(
                "comparator sort is only supported for array sort",
            ));
        }
        let Some(Type::Function(function_ty)) = self.mir.types.get(self.operand_ty(comparator)?)
        else {
            return Err(EmitError::new("array sort comparator must be a closure"));
        };
        if self.mir.types.get(function_ty.return_ty) != Some(&Type::Float) {
            return Err(EmitError::new("array sort comparator must return a number"));
        }
        let left_param_ty = function_ty.params.first().copied().unwrap_or(element_ty);
        let right_param_ty = function_ty.params.get(1).copied().unwrap_or(left_param_ty);
        let left_arg = self.value_at_type_text("left.clone()", element_ty, left_param_ty)?;
        let right_arg = self.value_at_type_text("right.clone()", element_ty, right_param_ty)?;
        let closure_text = match self.closure_operand_text_for_declared_type(comparator) {
            Ok(closure_text) => closure_text,
            Err(_) => self.operand_text(comparator)?,
        };
        Ok(format!(
            "{{ let mut smelt_comparator = {closure_text}; {list_text}.sort_by(|left, right| {{ let ordering = (smelt_comparator)({left_arg}, {right_arg}); if ordering < 0.0 {{ std::cmp::Ordering::Less }} else if ordering > 0.0 {{ std::cmp::Ordering::Greater }} else {{ std::cmp::Ordering::Equal }} }}); {result_text} }}"
        ))
    }

    // Validates that an optional slice index is numeric.
}
