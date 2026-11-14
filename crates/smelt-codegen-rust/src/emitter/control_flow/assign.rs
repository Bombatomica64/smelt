//! Assignment statements: a value written to a place, per place shape.

use super::*;

impl FunctionEmitter<'_> {
    /// Emits an assignment to a local, field, or index place.
    pub(in crate::emitter) fn emit_assign_place_statement(
        &self,
        place: &Place,
        value: &Rvalue,
        out: &mut String,
    ) -> Result<(), EmitError> {
        match place {
            // A write through a module-level mutable global goes straight into
            // the cell; see `global_place_assign_expr` for why it must not go
            // through a materialized copy.
            Place::Global { base, projection } => {
                let statement = self.global_place_assign_expr(*base, projection, value)?;
                out.push_str("    ");
                out.push_str(statement.as_str());
                out.push_str(";\n");
                // The whole statement is emitted; the shared tail below asks
                // for `place_ty`, which a global place deliberately has not.
                return Ok(());
            }
            Place::Field { base, field } => {
                if self.emit_field_write(*base, *field, value, out)? {
                    return Ok(());
                }
            }
            Place::Index {
                base,
                index,
                negative,
            } => return self.emit_index_write(*base, index, *negative, value, out),
            Place::Local(_) => {}
        }

        self.binding_uses.take_mut_borrows();
        let rendered_value = self.rvalue_expr_for_dest(value, self.place_ty(place)?)?;
        let value_mut_borrows = self.binding_uses.take_mut_borrows();
        if let Place::Local(local) = place
            && (!self.is_local_declared(*local)
                || (matches!(self.local_decl(*local)?.kind, LocalKind::Temp)
                    && !self.mutable_locals.contains(local)))
        {
            let decl = self.local_decl(*local)?;
            let name = self.local_name(*local)?;
            let mutability = if self.mutable_locals.contains(local) {
                "mut "
            } else {
                ""
            };
            if self.local_uses_shared_capture_storage(*local) {
                out.push_str(&format!(
                    "    let smelt_capture_{name}: ::std::rc::Rc<::std::cell::RefCell<{}>> = ::std::rc::Rc::new(::std::cell::RefCell::new({rendered_value}));\n",
                    self.capture_cell_type(*local, decl.ty)?, rendered_value = rendered_value.delimited()
                ));
            } else {
                out.push_str(&format!(
                    "    let {mutability}{name}: {} = {rendered_value};\n",
                    self.rendered_type(decl.ty)?,
                    rendered_value = rendered_value.delimited()
                ));
            }
            self.mark_local_declared(*local);
            return Ok(());
        }
        let target = self.assignment_place_target(place)?;
        let assignment = target.text();
        // The target's guard is taken after the value is evaluated, so a value
        // that holds its own guard on the same cell has to finish first.
        if target
            .guarded_cell()
            .is_some_and(|cell| value_mut_borrows.contains(&cell.cell))
        {
            out.push_str(&format!(
                "    {{ let smelt_next_value = {rendered_value}; {assignment} = smelt_next_value; }}\n", rendered_value = rendered_value.delimited(), assignment = assignment.delimited()
            ));
            return Ok(());
        }
        out.push_str(&format!(
            "    {assignment} = {rendered_value};\n",
            assignment = assignment.delimited(),
            rendered_value = rendered_value.delimited()
        ));
        Ok(())
    }

    /// A write to a field that needs more than a plain assignment.
    /// Answers whether the statement is complete; `false` hands it to the shared tail of
    /// [`Self::emit_assign_place_statement`].
    fn emit_field_write(
        &self,
        base: LocalId,
        field: Symbol,
        value: &Rvalue,
        out: &mut String,
    ) -> Result<bool, EmitError> {
        let base_ty = self.local_decl(base)?.ty;
        // A JavaScript `RegExp.lastIndex` write stores into a
        // `RefCell<usize>`, so the numeric right-hand side (typed
        // `f64`) is narrowed back to `usize` at the write seam. The
        // matching read path lives in `regexp_field_expr`.
        if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
            && self.is_regexp_class_symbol(*name)?
            && matches!(self.symbol_name(field)?, "lastIndex" | "last_index")
        {
            let rendered_value = self.rvalue_expr_for_dest(value, self.type_id(Type::Float)?)?;
            out.push_str(&format!(
                "    *{}.last_index.borrow_mut() = ({rendered_value}) as usize;\n",
                self.local_value(base)?.tight(),
                rendered_value = rendered_value.delimited()
            ));
            return Ok(true);
        }
        // A JavaScript function is an object, so a property write onto
        // a function-typed place is a real store, not a no-op, and the
        // place-text path below has no slot for it. It lands in the
        // function's identity-keyed own-property bag, which every other
        // representation of the same function reads through (see
        // `smelt-runtime`'s `function_object.rs`). This is the write side of
        // the `smelt_get_unknown_field` function arm.
        if let Some(Type::Function(function)) = self.mir.types.get(base_ty).cloned() {
            let unknown_ty = self.type_id(Type::Unknown)?;
            let rendered_value = self.rvalue_expr_for_dest(value, unknown_ty)?;
            let field_name = self.symbol_source_name(field)?;
            let base_text = self.local_value(base)?;
            // `Type::Function` has two Rust spellings (see `rust_type`):
            // the `SmeltErasedFunction` struct, which carries the store
            // as a method, and a typed `Rc<dyn Fn(..)>`, which reaches
            // the same bag through the free function.
            if self.is_erased_unknown_rest_function(&function) && !function.may_throw {
                out.push_str(&format!(
                    "    {base_text}.smelt_set_property({field_name:?}, {rendered_value});\n",
                    base_text = base_text.tight(),
                    rendered_value = rendered_value.delimited()
                ));
            } else {
                out.push_str(&format!(
                    "    smelt_set_function_property(&{base_text}, {field_name:?}, {rendered_value});\n", base_text = base_text.tight(), rendered_value = rendered_value.delimited()
                ));
            }
            return Ok(true);
        }
        if let Some(statement) = self.descriptor_setter_statement(base, field, value)? {
            out.push_str("    ");
            out.push_str(&statement);
            out.push('\n');
            return Ok(true);
        }
        if let Some(Type::Dict(key, item)) = self.mir.types.get(base_ty) {
            let rendered_value = self.rvalue_expr_for_dest(value, *item)?;
            let key_text = self.dict_field_key_expr(*key, field)?;
            out.push_str(&format!(
                "    {}.insert({key_text}, {rendered_value});\n",
                self.local_mut_value(base)?.tight(),
                key_text = key_text.delimited(),
                rendered_value = rendered_value.delimited(),
            ));
            return Ok(true);
        }
        // A CONCRETE union stores a tagged `SmeltUnion…` enum, so a
        // dotted property write cannot be matched with
        // `SmeltUnknown::Object(..)` patterns the way the erased arm
        // below does (`expected SmeltUnion778, found SmeltUnknown`).
        //
        // The dispatch rule is the union's OWN arms. When every arm
        // declares the property, each arm takes its member's typed
        // field write and nothing is erased. When it does not — the
        // property belongs to one arm only, which is the shape a
        // source-level narrowing selects — the write crosses the same
        // boundary adapter the INDEXED write on a union receiver
        // already uses (see the `Place::Index` arm below, H26's rule on
        // the write path): erase out, mutate the erased view, convert
        // back in and commit. Mutating a copy without committing it
        // would drop the write silently. `x.k = v` and `x["k"] = v` are
        // one operation in TypeScript, so they take one rule here.
        if self.concrete_union_members(base_ty).is_some() {
            if let Some(statement) = self.concrete_union_field_assign_expr(base, field, value)? {
                out.push_str("    ");
                out.push_str(statement.as_str());
                out.push('\n');
                return Ok(true);
            }
            let unknown_ty = self.type_id(Type::Unknown)?;
            let rendered_value = self.rvalue_expr_for_dest(value, unknown_ty)?;
            let field_name = self.symbol_source_name(field)?;
            let union_text = self.union_rust_type(base_ty)?;
            let erased = self.erase_concrete_union_expr(&self.local_value(base)?, base_ty)?;
            out.push_str(&format!(
                "    {{ let smelt_value = {rendered_value}; let mut smelt_erased = {erased}; smelt_index_assign(&mut smelt_erased, {field_name:?}.to_owned(), smelt_value); {} = {union_text}::from_smelt_unknown(smelt_erased); }}\n",
                self.local_mut_value(base)?.delimited(), rendered_value = rendered_value.delimited(), erased = erased.delimited()
            ));
            return Ok(true);
        }
        if matches!(
            self.mir.types.get(base_ty),
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
        ) || self.is_erased_class_type(base_ty)
        {
            let unknown_ty = self.type_id(Type::Unknown)?;
            let rendered_value = self.rvalue_expr_for_dest(value, unknown_ty)?;
            let base_text = self.local_mut_value(base)?;
            // A JavaScript array is an object with named properties too, so
            // an ARRAY receiver stores the property in the array's side
            // table (`SmeltArray::set_named_property`) and stays an array.
            // The fallback arm — a primitive receiver, where JavaScript
            // silently discards the write — keeps replacing the value.
            //
            // The SOURCE property name, not the Rust-mangled symbol. An
            // erased object is a keyed map whose keys are JavaScript
            // property names, and every read side uses
            // `symbol_source_name` — `smelt_get_object_field(&map, "someProp")`.
            // Writing through `symbol_name` stored `"some_prop"` instead, so
            // `obj.someProp = 1; obj.someProp` read back `undefined` and
            // `Object.keys(obj)` reported the mangled spelling. Every
            // camelCase property assigned to an `any`/`unknown` receiver was
            // silently lost.
            let field_name = self.symbol_source_name(field)?;
            // When the assigned value reads the receiver (e.g. JS
            // self-aliasing `original.self = original`), evaluate it into a
            // temporary BEFORE taking the receiver's mutable borrow. Inlining
            // the value inside the `&mut` match arms would borrow the receiver
            // immutably while it is mutably borrowed (E0502). This mirrors the
            // `Index` erased-object path below. Non-self-referential values
            // keep the simpler inline form to avoid churn.
            // A write through an erased reference-class VIEW also lands
            // on the instance the view stands for; see
            // `view_write_through`. The value is bound once so the view
            // and the instance receive the same value.
            // A write through a CLASS VALUE reaches the class's live static
            // storage (`smelt_class_static_write`) when the program has class
            // values at all.
            // A proxy can be behind any erased receiver: its `set` trap decides.
            if self.context.demand().has(Need::Construct) {
                out.push_str(&self.carrier_format(self.carrier()?, format_args!(
                    "    {{ let smelt_value = {rendered_value}; smelt_index_assign(&mut {base_text}, {field_name:?}, smelt_value); }}\n", rendered_value = rendered_value.delimited(), base_text = base_text.tight()
                )));
                return Ok(true);
            }
            let write_through = self.context.erased_view_write_through();
            let class_statics = self.context.demand().has(Need::ClassValue);
            if write_through || class_statics {
                let view_write = if write_through {
                    format!(" smelt_object_write_through(smelt_map, {field_name:?}, &smelt_value);")
                } else {
                    String::new()
                };
                let static_write = if class_statics {
                    format!(" smelt_class_static_write(smelt_map, {field_name:?}, &smelt_value);")
                } else {
                    String::new()
                };
                out.push_str(&self.carrier_format(self.carrier()?, format_args!(
                    "    {{ let smelt_value = {rendered_value}; match &mut {base_text} {{ SmeltUnknown::Object(smelt_map) => {{ smelt_map.insert({field_name:?}.to_owned(), smelt_value.clone());{view_write}{static_write} }}, SmeltUnknown::Array(smelt_values) => {{ smelt_values.set_named_property({field_name:?}.to_owned(), smelt_value); }}, SmeltUnknown::Function(smelt_function) => {{ smelt_set_function_property(smelt_function, {field_name:?}, smelt_value); }}, other => {{ *other = SmeltUnknown::Object(SmeltObject::new(Vec::from([({field_name:?}.to_owned(), smelt_value)]))); }} }} }}\n", rendered_value = rendered_value.delimited(), base_text = base_text.tight()
                )));
                return Ok(true);
            }
            // A FUNCTION receiver keeps being a function: the write
            // lands in its identity-keyed own-property bag (see
            // `smelt-runtime`'s `function_object.rs`), which every other
            // representation of that function reads through. Falling
            // through to the primitive arm replaced the callable with a
            // one-property record, so `(f as any).x = 1` lost `f`.
            if value.reads_local(base) {
                out.push_str(&self.carrier_format(self.carrier()?, format_args!(
                    "    {{ let smelt_value = {rendered_value}; match &mut {base_text} {{ SmeltUnknown::Object(smelt_map) => {{ smelt_map.insert({field_name:?}.to_owned(), smelt_value); }}, SmeltUnknown::Array(smelt_values) => {{ smelt_values.set_named_property({field_name:?}.to_owned(), smelt_value); }}, SmeltUnknown::Function(smelt_function) => {{ smelt_set_function_property(smelt_function, {field_name:?}, smelt_value); }}, other => {{ *other = SmeltUnknown::Object(SmeltObject::new(Vec::from([({field_name:?}.to_owned(), smelt_value)]))); }} }} }}\n", rendered_value = rendered_value.delimited(), base_text = base_text.tight()
                )));
            } else {
                out.push_str(&self.carrier_format(self.carrier()?, format_args!(
                    "    match &mut {base_text} {{ SmeltUnknown::Object(smelt_map) => {{ smelt_map.insert({field_name:?}.to_owned(), {rendered_value}); }}, SmeltUnknown::Array(smelt_values) => {{ smelt_values.set_named_property({field_name:?}.to_owned(), {rendered_value}); }}, SmeltUnknown::Function(smelt_function) => {{ smelt_set_function_property(smelt_function, {field_name:?}, {rendered_value}); }}, other => {{ *other = SmeltUnknown::Object(SmeltObject::new(Vec::from([({field_name:?}.to_owned(), {rendered_value})]))); }} }}\n", base_text = base_text.tight(), rendered_value = rendered_value.delimited()
                )));
            }
            return Ok(true);
        }
        // A dotted write to an UNDECLARED member on an index-signature
        // class (`bag.name = value`) inserts into the runtime store
        //, mirroring the computed `bag[k] = value` write, so
        // it round-trips to a later read. Declared fields keep their
        // concrete struct-field assignment via the tail path below.
        if let Some((key_ty, value_ty)) = self.class_index_store_types(base_ty)
            && !self.class_has_named_field(base_ty, field)
        {
            let rendered_value = self.rvalue_expr_for_dest(value, value_ty)?;
            out.push_str(&format!(
                "    {}.{}.insert({}, {rendered_value});\n",
                self.local_mut_value(base)?.tight(),
                smelt_hir::CLASS_INDEX_STORE_FIELD,
                self.dict_field_key_expr(key_ty, field)?.delimited(),
                rendered_value = rendered_value.delimited(),
            ));
            return Ok(true);
        }
        Ok(false)
    }

    /// A write to an index place, by what the base holds.
    fn emit_index_write(
        &self,
        base: LocalId,
        index: &Operand,
        negative: NegativeIndex,
        value: &Rvalue,
        out: &mut String,
    ) -> Result<(), EmitError> {
        let base_ty = self.local_decl(base)?.ty;
        // An indexed write through a typed-array view encodes ONE
        // element into the shared storage, so a write through one view
        // is visible through every other view over the same buffer.
        // Out-of-range and non-integer indices are dropped, which is
        // what JavaScript does for a view (they would be named
        // properties, and a view has none).
        if let Some(statement) =
            self.typed_array_index_write_statement(base_ty, base, index, value)?
        {
            out.push_str("    ");
            out.push_str(&statement);
            out.push('\n');
            return Ok(());
        }
        match self.mir.types.get(base_ty) {
            Some(Type::Dict(key, item)) => {
                let rendered_value = self.rvalue_expr_for_dest(value, *item)?;
                let key_text = if self.mir.types.get(*key).is_some_and(Type::is_text) {
                    let index_ty = self.operand_ty(index)?;
                    if index_ty == *key {
                        self.value_at_type(index, *key)?
                    } else {
                        let index_text = self.operand_expr(index)?;
                        self.property_key_to_string_expr(&index_text, index_ty)?
                    }
                } else {
                    self.value_at_type(index, *key)?
                };
                out.push_str(&format!(
                    "    {}.insert({}, {rendered_value});\n",
                    self.local_mut_value(base)?.tight(),
                    key_text.delimited(),
                    rendered_value = rendered_value.delimited()
                ));
            }
            Some(Type::List(item)) => {
                let rendered_value = self.rvalue_expr_for_dest(value, *item)?;
                // The receiver is grown and written through `resize` /
                // `IndexMut`, which need a mutable borrow, but the two
                // length reads (normalizing the index and bounds-checking
                // the resize) only need `&self`. Rendering the length
                // through the mutable form put two `borrow_mut()` of the
                // same shared-capture `RefCell` on the resize line. The
                // index is bound to `smelt_assign_index` first, so each
                // read borrow drops at its statement boundary before the
                // mutable write; splitting the read borrow keeps every
                // emitted statement to at most one `borrow_mut()` of the
                // cell, avoiding the "already borrowed" panic. Write-path
                // twin of the list index READ arm in `place.rs`.
                let base_mut = list_write_expr(&self.local_value(base)?);
                let base_read = self.local_value(base)?;
                let index_text =
                    self.normalized_index_expr(&base_read.postfix(".len()"), index, negative)?;
                // Growing past the end leaves JS *holes*, which read as
                // `undefined`, not `null` (see `array_hole_value`).
                let default_value = self.array_hole_value(*item)?;
                // The assigned value is bound before the write for the same
                // reason the index is: `a[i] = a[j]` would otherwise evaluate
                // a READ borrow of the same shared cell whose guard is still
                // live when the write takes its `borrow_mut()`, panicking
                // "already borrowed". A `let` ends the read guard first.
                out.push_str(&format!(
                    "    {{ let smelt_assign_index = {index_text}; let smelt_assign_value = {rendered_value}; if smelt_assign_index >= {base_read}.len() {{ {base_mut}.resize(smelt_assign_index.saturating_add(1), {default_value}); }} {base_mut}[smelt_assign_index] = smelt_assign_value; }}\n", index_text = index_text.delimited(), rendered_value = rendered_value.delimited(), base_read = base_read.tight(), base_mut = base_mut.tight(), default_value = default_value.delimited()
                ));
            }
            Some(Type::Tuple(items)) => {
                let tuple_index = self.tuple_index(index, items.len())?;
                let item_ty = items
                    .get(tuple_index)
                    .copied()
                    .ok_or_else(|| EmitError::new("tuple index is out of bounds"))?;
                let rendered_value = self.rvalue_expr_for_dest(value, item_ty)?;
                out.push_str(&format!(
                    "    {}.{tuple_index} = {rendered_value};\n",
                    self.local_mut_value(base)?.tight(),
                    rendered_value = rendered_value.delimited()
                ));
            }
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. }) => {
                let rendered_value =
                    self.rvalue_expr_for_dest(value, self.type_id(Type::Unknown)?)?;
                let index_ty = self.operand_ty(index)?;
                let index_text = self.operand_expr(index)?;
                let key_text = self.property_key_to_string_expr(&index_text, index_ty)?;
                // A CONCRETE union stores a tagged `SmeltUnion…` enum, so
                // it is not a `&mut SmeltUnknown` and cannot be handed to
                // the erased keyed-write helper — this arm matched
                // `Type::Union(_)` and passed the enum straight through
                // (`expected &mut SmeltUnknown, found &mut SmeltUnion164`,
                // 360 of the hono router slice's errors).
                //
                // This is H26's rule on the WRITE path, and a write needs
                // one more step than a read: cross the boundary adapter
                // OUT, mutate the erased view, then cross back IN and
                // commit the result to the place. Mutating a copy without
                // committing it would drop the write silently, which is the
                // failure this whole family is about — so the write-back is
                // the point, not a detail.
                //
                // A union with no generated enum still renders as
                // `SmeltUnknown` and keeps the direct call below.
                if self.concrete_union_members(base_ty).is_some() {
                    let union_text = self.union_rust_type(base_ty)?;
                    let erased =
                        self.erase_concrete_union_expr(&self.local_value(base)?, base_ty)?;
                    out.push_str(&format!(
                        "    {{ let smelt_key = {key_text}; let smelt_value = {rendered_value}; let mut smelt_erased = {erased}; smelt_index_assign(&mut smelt_erased, smelt_key, smelt_value); {} = {union_text}::from_smelt_unknown(smelt_erased); }}\n",
                        self.local_mut_value(base)?.delimited(), key_text = key_text.delimited(), rendered_value = rendered_value.delimited(), erased = erased.delimited()
                    ));
                    return Ok(());
                }
                out.push_str(&format!(
                    "    {{ let smelt_key = {key_text}; let smelt_value = {rendered_value}; smelt_index_assign(&mut {}, smelt_key, smelt_value); }}\n",
                    self.local_mut_value(base)?.tight(), key_text = key_text.delimited(), rendered_value = rendered_value.delimited()
                ));
            }
            // A class with an index signature backs keyed writes with a
            // real store field: `bag[key] = value` inserts
            // into `bag.__smelt_index_store` so the write round-trips to a
            // later `bag[key]` read. The value is rendered at the store's
            // declared value type, keeping `T` concrete.
            Some(Type::Class { .. }) if self.class_index_store_types(base_ty).is_some() => {
                let (key_ty, value_ty) = self
                    .class_index_store_types(base_ty)
                    .ok_or_else(|| EmitError::new("class index store types missing"))?;
                let rendered_value = self.rvalue_expr_for_dest(value, value_ty)?;
                let key_text = if self.mir.types.get(key_ty).is_some_and(Type::is_text) {
                    let index_ty = self.operand_ty(index)?;
                    if index_ty == key_ty {
                        self.value_at_type(index, key_ty)?
                    } else {
                        let index_text = self.operand_expr(index)?;
                        self.property_key_to_string_expr(&index_text, index_ty)?
                    }
                } else {
                    self.value_at_type(index, key_ty)?
                };
                out.push_str(&format!(
                    "    {}.{}.insert({key_text}, {rendered_value});\n",
                    self.local_mut_value(base)?.tight(),
                    smelt_hir::CLASS_INDEX_STORE_FIELD,
                    key_text = key_text.delimited(),
                    rendered_value = rendered_value.delimited(),
                ));
            }
            // A computed-key write into a class instance dispatches on
            // the runtime key over the fields that can hold the value
            // (see `class_keyed_write`).
            Some(Type::Class { .. })
                if let Some(statement) =
                    self.class_keyed_field_write_expr(base, base_ty, index, value)? =>
            {
                out.push_str("    ");
                out.push_str(statement.as_str());
                out.push('\n');
            }
            _ => {
                let rendered_value = self.rvalue_expr(value)?;
                out.push_str(&format!(
                    "    let _ = {rendered_value};\n",
                    rendered_value = rendered_value.delimited()
                ));
            }
        }
        Ok(())
    }
}
