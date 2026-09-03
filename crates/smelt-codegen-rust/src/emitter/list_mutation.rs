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
        // `base[key].push(item)` reaches codegen with the dict ENTRY as the push
        // receiver once `smelt_mir::opt::DictEntryInPlaceMutation` has fused the
        // copy-out/mutate/copy-back triple that HIR lowering emits. Mutate the
        // stored list through the container's entry accessor, which is what a
        // hand-written Rust port does (`groups.entry(key).or_default().push(x)`)
        // and which copies neither the old nor the new group.
        if let Operand::Copy(Place::Index { base, index, .. })
        | Operand::Move(Place::Index { base, index, .. }) = list
            && let Some(Type::Dict(key_ty, value_ty)) =
                self.mir.types.get(self.local_decl(*base)?.ty)
            && *value_ty == list_ty
        {
            let entry_key_ty = *key_ty;
            let entry_ty = *value_ty;
            let base_text = self.local_mut_value_text(*base)?;
            let key_text = if self.mir.types.get(entry_key_ty) == Some(&Type::String) {
                let source_key = self.operand_ty(index.as_ref())?;
                let index_text = self.operand_text(index.as_ref())?;
                self.property_key_to_string_text(&index_text, source_key)?
            } else {
                self.value_at_type(index.as_ref(), entry_key_ty)?
            };
            let default_value = self.default_value(entry_ty)?;
            let item_text = self.value_at_type(item, *item_ty)?;
            let result = if returns_length {
                "smelt_slot.len() as f64"
            } else {
                "()"
            };
            // The pushed item is materialized BEFORE the entry accessor runs.
            // The item expression may read the same container (for example
            // `groups[key].push(groups[key].length)`), and an object-keyed or
            // record-backed entry handle is a live `RefCell` borrow that such a
            // read would panic on ("already borrowed"). Evaluating the item
            // first also keeps the key and default expressions outside the
            // borrow, since both are arguments of the accessor call.
            //
            // `SmeltJsMap`/`SmeltRecord` hand back a `RefMut<SmeltList<..>>`
            // guard; a plain `HashMap` entry is already a `&mut SmeltList<..>`.
            // Either way the slot derefs to the list, never to its backing
            // `Vec` — `SmeltList` owns its values behind an `Rc<RefCell<..>>`
            // and so implements no `DerefMut` — so the push goes through
            // `list_write_text`, the same borrow every other list write uses
            // (`smelt_slot.push(..)` does not resolve: E0599).
            let accessor = if self.dict_uses_js_key_map(entry_key_ty)
                || self.dict_uses_smelt_record(entry_key_ty)
            {
                // The default is passed as a CLOSURE so it is built only when the
                // key is absent. A JavaScript accumulator loop reaches this once
                // per element with the key already present for all but the first,
                // and an eagerly built empty list is two heap allocations thrown
                // away each time. `HashMap::or_insert_with` is the same rule for
                // the plain-map backing.
                format!("{base_text}.entry_or_insert({key_text}, || {default_value})")
            } else {
                format!("{base_text}.entry({key_text}).or_insert_with(|| {default_value})")
            };
            // The write guard is a statement-local temporary, so it is released
            // before `{result}` reads the length back through the slot.
            let push_text = list_write_text("smelt_slot");
            return Ok(format!(
                "{{ let smelt_push_item = {item_text}; let smelt_slot = {accessor}; {push_text}.push(smelt_push_item); {result} }}"
            ));
        }
        if let Operand::Copy(Place::Field { base, field })
        | Operand::Move(Place::Field { base, field }) = list
            && matches!(
                self.mir.types.get(self.local_decl(*base)?.ty),
                Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
            )
        {
            let base_text = self.local_mut_value_text(*base)?;
            // Source property name, not the Rust-mangled symbol: this reads and
            // rewrites a field of an ERASED object, whose keys are JavaScript
            // property names. Mangling made `obj.someList.push(x)` look up
            // `"some_list"` and miss the array stored under `"someList"`.
            let field_name = self.symbol_source_name(*field)?;
            let item_text = self.value_at_type(item, *item_ty)?;
            let result = if returns_length {
                "smelt_list.len() as f64"
            } else {
                "()"
            };
            return Ok(format!(
                "{{ let mut smelt_list = match {base_text}.clone() {{ SmeltUnknown::Object(map) => match smelt_get_object_field(&map, \"{field_name}\") {{ SmeltUnknown::Array(values) => values.into_vec(), _ => Vec::new() }}, _ => Vec::new() }}; smelt_list.push(({item_text}).into_smelt_unknown()); let smelt_result = {result}; let smelt_value = SmeltUnknown::Array(smelt_list.into()); match &mut {base_text} {{ SmeltUnknown::Object(map) => {{ map.insert(\"{field_name}\".to_owned(), smelt_value); }}, other => {{ let map = SmeltObject::new(Vec::from([(\"{field_name}\".to_owned(), smelt_value)])); *other = SmeltUnknown::Object(map); }} }} smelt_result }}"
            ));
        }
        if let Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) = list
            && let Some(ListAliasOrigin::Field { base, field }) = self.list_alias_origin(*local)
            && self.list_alias_base_is_erased_object(base)?
        {
            let list_text = self.local_value_text(*local)?;
            let base_text = self.local_mut_value_text(base)?;
            let field_name = self.symbol_name(field)?;
            let item_text = self.value_at_type(item, *item_ty)?;
            let result = if returns_length {
                format!("{list_text}.len() as f64")
            } else {
                "()".to_owned()
            };
            let list_mut = list_write_text(&list_text);
            // Materialize the pushed item into a temp before the push, ALWAYS.
            // The receiver's `borrow_mut()` guard must never coexist with a read
            // borrow of the same shared buffer ("already borrowed"), and since a
            // `SmeltList` clone shares its buffer, two DIFFERENT locals can name
            // one cell — `groups[k].push(groups[k][0])` reads through a second
            // handle. A syntactic "does the item mention this local" test cannot
            // see that, so there is no cheap condition to gate this on: aliasing
            // is a runtime property. One `let` removes the whole class.
            let push_expr =
                format!("let smelt_push_item = {item_text}; {list_mut}.push(smelt_push_item);");
            return Ok(format!(
                "{{ {push_expr} let smelt_result = {result}; let smelt_value = SmeltUnknown::Array({list_text}.clone().into_iter().map(IntoSmeltUnknown::into_smelt_unknown).collect()); match &mut {base_text} {{ SmeltUnknown::Object(map) => {{ map.insert(\"{field_name}\".to_owned(), smelt_value); }}, other => {{ let map = SmeltObject::new(Vec::from([(\"{field_name}\".to_owned(), smelt_value)])); *other = SmeltUnknown::Object(map); }} }} smelt_result }}"
            ));
        }
        if let Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) = list
            && let Some(ListAliasOrigin::Index { base, index }) = self.list_alias_origin(*local)
            && let Some(Type::Dict(key_ty, value_ty)) =
                self.mir.types.get(self.local_decl(base)?.ty)
            && *value_ty == list_ty
        {
            let list_text = self.local_value_text(*local)?;
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
            let list_mut = list_write_text(&list_text);
            // Materialize the pushed item into a temp before the push, ALWAYS.
            // The receiver's `borrow_mut()` guard must never coexist with a read
            // borrow of the same shared buffer ("already borrowed"), and since a
            // `SmeltList` clone shares its buffer, two DIFFERENT locals can name
            // one cell — `groups[k].push(groups[k][0])` reads through a second
            // handle. A syntactic "does the item mention this local" test cannot
            // see that, so there is no cheap condition to gate this on: aliasing
            // is a runtime property. One `let` removes the whole class.
            let push_expr =
                format!("let smelt_push_item = {item_text}; {list_mut}.push(smelt_push_item);");
            return Ok(format!(
                "{{ {push_expr} let smelt_result = {result}; {base_text}.insert({key_text}, {list_text}.clone()); smelt_result }}"
            ));
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = list else {
            return Err(EmitError::new(
                "list push receiver must be a mutable local for now",
            ));
        };
        let list_text = self.local_value_text(*local)?;
        let item_text = self.value_at_type(item, *item_ty)?;
        // Evaluate the pushed item into a temporary BEFORE taking the list's
        // mutable borrow in `.push`. Rust evaluates a method receiver before its
        // arguments, so inlining the item would hold the shared cell's
        // `borrow_mut()` guard while the item took a read borrow of that same
        // cell — a runtime "already borrowed" panic (it was an E0502 borrow
        // error back when the elements were an inline `Vec`).
        //
        // This is unconditional. A `SmeltList` clone shares its buffer, so two
        // DIFFERENT locals can name one cell (`groups[k].push(groups[k][0])`
        // reads through a second handle), and the syntactic "does the item
        // mention this local" test that used to gate it cannot see that.
        // Aliasing is a runtime property; one `let` removes the whole class.
        let list_mut = list_write_text(&list_text);
        let push_expr =
            format!("let smelt_push_item = {item_text}; {list_mut}.push(smelt_push_item);");
        if returns_length {
            Ok(format!("{{ {push_expr} {list_text}.len() as f64 }}"))
        } else {
            Ok(format!("{{ {push_expr} () }}"))
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
                        Operand::Copy(Place::Index { base, index, .. })
                        | Operand::Move(Place::Index { base, index, .. }) => {
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
        // A mismatched argument list (e.g. a generic `SmeltList<T>` extending an
        // erased `SmeltList<SmeltUnknown>` receiver) coerces through the shared
        // `value_at_type` conversion instead of failing; genuinely unconvertible
        // shapes still produce that helper's honest error.
        let other_text = if self.operand_ty(other)? == list_ty {
            self.operand_text(other)?
        } else {
            self.value_at_type(other, list_ty)?
        };
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
        let list_text = self.local_value_text(*local)?;
        // The extended-from list is bound before the receiver's write borrow, so
        // `a.extend(a)` cannot hold a read guard and a write guard on one shared
        // buffer at the same time.
        let list_mut = list_write_text(&list_text);
        let other_read = list_read_text("smelt_extend_from");
        if returns_length {
            Ok(format!(
                "{{ let smelt_extend_from = {other_text}; {list_mut}.extend({other_read}.iter().cloned()); {list_text}.len() as f64 }}"
            ))
        } else {
            Ok(format!(
                "{{ let smelt_extend_from = {other_text}; {list_mut}.extend({other_read}.iter().cloned()); () }}"
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
        let list_text = self.local_value_text(*local)?;
        let index_text = self.operand_text(index)?;
        let item_text = self.operand_text(item)?;
        let list_mut = list_write_text(&list_text);
        Ok(format!(
            "{{ let insert_index = usize::try_from({index_text}).expect(\"list insert negative index\"); let smelt_insert_item = {item_text}; {list_mut}.insert(insert_index, smelt_insert_item); () }}"
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
        let list_text = self.local_value_text(*local)?;
        let mut statements = Vec::with_capacity(items.len().saturating_add(1));
        let list_mut = list_write_text(&list_text);
        for item in items.iter().rev() {
            let item_text = self.operand_text(item)?;
            statements.push(format!(
                "let smelt_unshift_item = {item_text}; {list_mut}.insert(0, smelt_unshift_item);"
            ));
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
        let list_text = self.local_value_text(*local)?;
        let list_mut = list_write_text(&list_text);
        if returns_list {
            Ok(format!("{{ {list_mut}.reverse(); {list_text}.clone() }}"))
        } else {
            Ok(format!("{{ {list_mut}.reverse(); () }}"))
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
        let item_ty = *item_ty;
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = list else {
            return Err(EmitError::new(
                "list pop receiver must be a mutable local for now",
            ));
        };
        let list_text = self.local_value_text(*local)?;
        // `Array.prototype.pop` yields `item | undefined`. The two exact-match
        // fast paths keep the historical output; any other destination (e.g. a
        // widened/narrowed optional whose inner differs from the list item type,
        // as when the item type is a union) coerces the popped value to the
        // destination through the standard coercion seam instead of aborting.
        let list_mut = list_write_text(&list_text);
        match self.mir.types.get(dest_ty) {
            Some(Type::Optional(inner)) if *inner == item_ty => Ok(format!("{list_mut}.pop()")),
            _ if dest_ty == item_ty => {
                Ok(format!("{list_mut}.pop().expect(\"pop from empty list\")"))
            }
            Some(Type::Optional(_)) => {
                let pop_ty = self.type_id(Type::Optional(item_ty))?;
                self.value_at_type_text(&format!("{list_mut}.pop()"), pop_ty, dest_ty)
            }
            _ => self.value_at_type_text(
                &format!("{list_mut}.pop().expect(\"pop from empty list\")"),
                item_ty,
                dest_ty,
            ),
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
        let list_text = self.local_value_text(*local)?;
        let list_mut = list_write_text(&list_text);
        Ok(format!(
            "if {list_text}.is_empty() {{ None }} else {{ Some({list_mut}.remove(0)) }}"
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
        let list_text = self.local_value_text(*local)?;
        let list_mut = list_write_text(&list_text);
        Ok(format!(
            "if {list_text}.is_empty() {{ None }} else {{ Some({list_mut}.remove(0)) }}"
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
            "dict" => matches!(
                self.mir.types.get(collection_ty),
                Some(Type::Dict(_, _) | Type::JsMap(_, _))
            ),
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
        // Accept a place-rooted receiver (a class field holding the collection,
        // e.g. `this.__data.clear()`) as well as a plain local, rendering the
        // assignable lvalue so the clear mutates the stored collection in place.
        let (Operand::Copy(collection_place) | Operand::Move(collection_place)) = collection else {
            return Err(EmitError::new(format!(
                "{collection_name} clear receiver must be a place operand"
            )));
        };
        let collection_text = self.assignment_place_text(collection_place)?;
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
        let list_text = self.local_value_text(*local)?;
        let item_text = self.operand_text(item)?;
        // The search runs under a read borrow that a `let` ends before the
        // removal takes its write borrow of the same shared buffer.
        let list_read = list_read_text(&list_text);
        let list_mut = list_write_text(&list_text);
        if self.list_item_uses_same_value_zero(*item_ty) {
            if self.mir.types.get(*item_ty) == Some(&Type::Float) {
                return Ok(format!(
                    "{{ let smelt_needle = {item_text}; let remove_index = {list_read}.iter().position(|item| *item == smelt_needle || (item.is_nan() && smelt_needle.is_nan())).expect(\"list remove missing item\"); {list_mut}.remove(remove_index); () }}"
                ));
            }
            return Ok(format!(
                "{{ let smelt_needle = {item_text}; let remove_index = {list_read}.iter().position(|item| item.same_js_key(&smelt_needle)).expect(\"list remove missing item\"); {list_mut}.remove(remove_index); () }}"
            ));
        }
        Ok(format!(
            "{{ let smelt_needle = {item_text}; let remove_index = {list_read}.iter().position(|item| item == &smelt_needle).expect(\"list remove missing item\"); {list_mut}.remove(remove_index); () }}"
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
        let list_text = self.local_value_text(*local)?;
        // The sort rewrites the shared buffer in place, so every alias of this
        // array sees the new order — which is what `Array.prototype.sort` does.
        // KNOWN LIMITATION: a comparator that reads the array it is sorting
        // (legal in JavaScript) reads the same cell while this write borrow is
        // live and panics "already borrowed"; it was an E0502 borrow error when
        // the elements were an inline `Vec`.
        let list_mut = list_write_text(&list_text);
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
                &list_mut,
                &result_text,
            );
        }
        if key.is_some() || reverse {
            let (prefix, closure) = self.list_sort_by_text(key, reverse, element_ty)?;
            return Ok(format!(
                "{{ {prefix}{list_mut}.sort_by({closure}); {result_text} }}"
            ));
        }
        // A TypeScript receiver (`returns_list`) follows JavaScript's default
        // ordering for every element surface it supports; a Python receiver
        // (`sort()` with no key, `returns_list == false`) keeps source-native
        // scalar ordering and only borrows the JS coercion for erased items.
        if let Some(sort_call) = self.js_default_sort_call_text(element_ty, &list_mut)?
            && (returns_list || !self.element_sorts_natively(element_ty))
        {
            return Ok(format!("{{ {sort_call}; {result_text} }}"));
        }
        match self.mir.types.get(*item_ty) {
            Some(Type::Bool | Type::Int | Type::String) => {
                Ok(format!("{{ {list_mut}.sort(); {result_text} }}"))
            }
            Some(Type::Float) => Ok(format!(
                "{{ {list_mut}.sort_by(|left, right| left.partial_cmp(right).expect(\"list sort incomparable float\")); {result_text} }}"
            )),
            _ => Err(EmitError::new(
                "list sort supports bool, int, float, string, and erased items",
            )),
        }
    }

    /// True when the element type has a native Rust ordering usable directly by
    /// a Python-style `list.sort()` (booleans, integers, floats, strings).
    fn element_sorts_natively(&self, element_ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(element_ty),
            Some(Type::Bool | Type::Int | Type::Float | Type::String)
        )
    }

    /// Emit the `sort_by(..)` CALL (no surrounding block, no trailing `;`) that
    /// implements JavaScript's comparator-less `Array.prototype.sort` for
    /// `element_ty`, or `None` when the element type has no modeled `ToString`.
    ///
    /// JavaScript's default sort compares elements by their `ToString`
    /// coercion, never numerically. Scalars stringify directly; the erased
    /// surfaces (`unknown`, concrete unions, `never`, and a leaked non-scoped
    /// type parameter, all of which render as `SmeltUnknown`) go through the
    /// shared string-coercion match, with concrete unions projected via
    /// `into_smelt_unknown` first so the match sees the erased shape.
    ///
    /// A type parameter that IS in scope renders as a real generic (`T`) with no
    /// `into_smelt_unknown`, so it is deliberately excluded. Structured concrete
    /// shapes (nested lists, records) are excluded for the same reason: their JS
    /// `ToString` is not modeled yet.
    ///
    /// `sort_by` is stable, so equal-key values keep their original order, which
    /// is what the JS default sort does for values sharing a `ToString`.
    ///
    /// Factored out of the comparator-less path so the optional-comparator path
    /// (`array.sort(maybeCompare)`) can reuse the exact same ordering in its
    /// `None` arm.
    fn js_default_sort_call_text(
        &self,
        element_ty: TypeId,
        list_text: &str,
    ) -> Result<Option<String>, EmitError> {
        match self.mir.types.get(element_ty) {
            Some(Type::Bool | Type::Int | Type::Float | Type::String) => Ok(Some(format!(
                "{list_text}.sort_by(|left, right| left.to_string().cmp(&right.to_string()))"
            ))),
            Some(Type::TypeParam { name }) if self.current_function_has_type_param(*name) => {
                Ok(None)
            }
            Some(Type::Unknown | Type::Union(_) | Type::Never | Type::TypeParam { .. }) => {
                let left_key = Self::js_string_coercion_match_text(
                    &self.erase_concrete_union_text("left.clone()", element_ty),
                );
                let right_key = Self::js_string_coercion_match_text(
                    &self.erase_concrete_union_text("right.clone()", element_ty),
                );
                Ok(Some(format!(
                    "{list_text}.sort_by(|left, right| ({left_key}).cmp(&({right_key})))"
                )))
            }
            _ => Ok(None),
        }
    }

    /// Converts a JavaScript `Array.prototype.sort` comparator closure to Rust.
    ///
    /// The comparator is a normal closure operand taking two list items and
    /// returning a number, like other list callbacks. The closure is bound once
    /// and invoked per comparison; its numeric result maps to `Ordering` with
    /// JavaScript semantics (negative -> `Less`, positive -> `Greater`, zero and
    /// `NaN` -> `Equal`).
    ///
    /// A named or optional comparator (`array.sort(compareFn)` where `compareFn`
    /// is `((a, b) => number) | undefined`) is lowered through a synthesized
    /// wrapper closure whose call of the erased/optional inner callback yields a
    /// `SmeltUnknown` result, so the wrapper's declared return type is erased to
    /// `unknown` even though the source comparator is typed `=> number`. The
    /// frontend deliberately admits that erased return (see the `array sort`
    /// comparator check in `stdlib::list_sort_call`) and relies on this emitter
    /// to coerce the comparison result numerically. A `SmeltUnknown` here is a
    /// genuine dynamic boundary (the callback may be absent at runtime), so the
    /// result is coerced through the `SmeltIntoF64` boundary adapter rather than
    /// routed as an ordinary typed value; a concrete `number` return skips the
    /// adapter and compares directly.
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
        let comparator_ty = self.operand_ty(comparator)?;
        // `array.sort(maybeCompare)` where `maybeCompare` may be absent at
        // runtime. ECMA-262 `SortCompare` step 1 makes an `undefined`
        // comparator identical to no comparator at all, so the `None` arm must
        // run the DEFAULT `ToString` ordering, not "every comparison is Equal".
        // The optional stays a real `Option<Rc<dyn Fn..>>` all the way down and
        // is matched here, exactly as hand-written Rust would; it is not erased
        // to `SmeltUnknown`.
        if let Some(Type::Optional(inner)) = self.mir.types.get(comparator_ty).cloned() {
            let Some(default_sort) = self.js_default_sort_call_text(element_ty, list_text)? else {
                return Err(EmitError::new(
                    "array sort with an optional comparator needs a default ordering for its element type",
                ));
            };
            let some_arm = self.list_sort_comparator_call_text(
                inner,
                element_ty,
                "smelt_comparator",
                list_text,
            )?;
            let comparator_text = self.operand_text(comparator)?;
            return Ok(format!(
                "{{ match {comparator_text} {{ Some(smelt_comparator) => {{ {some_arm}; }} None => {{ {default_sort}; }} }}; {result_text} }}"
            ));
        }
        let closure_text = match self.closure_operand_text_for_declared_type(comparator) {
            Ok(closure_text) => closure_text,
            Err(_) => self.operand_text(comparator)?,
        };
        let sort_call = self.list_sort_comparator_call_text(
            comparator_ty,
            element_ty,
            "smelt_comparator",
            list_text,
        )?;
        Ok(format!(
            "{{ let mut smelt_comparator = {closure_text}; {sort_call}; {result_text} }}"
        ))
    }

    /// Emit the `sort_by(..)` call for an already-bound comparator binding.
    ///
    /// `comparator_ty` must be the comparator's function type and
    /// `comparator_ident` the name of an in-scope binding holding it. Split out
    /// of `list_sort_comparator_text` so the optional-comparator `Some(..)` arm
    /// can reuse the identical comparison, argument adaptation and
    /// number-to-`Ordering` mapping as the always-present case.
    fn list_sort_comparator_call_text(
        &self,
        comparator_ty: TypeId,
        element_ty: TypeId,
        comparator_ident: &str,
        list_text: &str,
    ) -> Result<String, EmitError> {
        let Some(Type::Function(function_ty)) = self.mir.types.get(comparator_ty) else {
            return Err(EmitError::new("array sort comparator must be a closure"));
        };
        // A `number` return compares directly; an erased return (`unknown`,
        // concrete union, `never`, or a leaked non-scoped type parameter, all of
        // which render as `SmeltUnknown`) is coerced through `SmeltIntoF64`.
        let coerce_result = match self.mir.types.get(function_ty.return_ty) {
            Some(Type::Float) => false,
            Some(Type::Unknown | Type::Union(_) | Type::Never | Type::TypeParam { .. }) => true,
            _ => {
                return Err(EmitError::new("array sort comparator must return a number"));
            }
        };
        let left_param_ty = function_ty.params.first().copied().unwrap_or(element_ty);
        let right_param_ty = function_ty.params.get(1).copied().unwrap_or(left_param_ty);
        let left_arg = self.callback_call_arg_text(
            function_ty,
            0,
            left_param_ty,
            self.value_at_type_text("left.clone()", element_ty, left_param_ty)?,
        );
        let right_arg = self.callback_call_arg_text(
            function_ty,
            1,
            right_param_ty,
            self.value_at_type_text("right.clone()", element_ty, right_param_ty)?,
        );
        let ordering_coercion = if coerce_result { ".smelt_into_f64()" } else { "" };
        Ok(format!(
            "{list_text}.sort_by(|left, right| {{ let ordering = ({comparator_ident})({left_arg}, {right_arg}){ordering_coercion}; if ordering < 0.0 {{ std::cmp::Ordering::Less }} else if ordering > 0.0 {{ std::cmp::Ordering::Greater }} else {{ std::cmp::Ordering::Equal }} }})"
        ))
    }

    // Validates that an optional slice index is numeric.
}
