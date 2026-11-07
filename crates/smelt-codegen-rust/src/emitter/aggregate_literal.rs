//! Aggregate literals: lists, sets, dicts, tuples and class records built from
//! their element operands at a destination type.

use super::*;
use crate::rust::RustExpr;

impl FunctionEmitter<'_> {
    /// Add the phantom field to a record literal's `fields` when `class`, carrying
    /// `type_arg_count` arguments, declares one for type parameters no field spells.
    pub(super) fn push_phantom_field(&self, fields: &mut Vec<String>, class: Symbol, type_arg_count: usize) {
        if self.context.type_param_elision().emits_phantom(class, type_arg_count) {
            fields.push("_smelt_phantom: ::std::marker::PhantomData".to_owned());
        }
    }

    /// A list literal at its destination.
    pub(super) fn list_literal_expr(
        &self,
        value: &Rvalue,
        items: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        // A contextual tuple assertion keeps the literal as a `List`
        // rvalue while changing its destination storage to a Rust tuple.
        // Render its heterogeneous elements directly into that tuple.
        if let Some(Type::Tuple(target_items)) = self.mir.types.get(dest_ty)
            && target_items.len() == items.len()
        {
            let items_text = RustExpr::from(items
                .iter()
                .zip(target_items)
                .map(|(item, target_ty)| {
                    self.value_at_type(&self.list_literal_operand(item), *target_ty)
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", "));
            return if target_items.len() == 1 {
                Ok(RustExpr::atom(format!("({items_text},)")))
            } else {
                Ok(RustExpr::atom(format!("({items_text})")))
            };
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(dest_ty) {
            // `Type::Tuple` belongs here for the same reason as the
            // others: the arm above renders a literal into a Rust tuple
            // when the destination IS one, and an optional destination
            // has to reach it through this recursion. Without the tuple
            // in this list an `Option<(A, B, C)>` destination fell
            // through to the homogeneous `vec![..]` fallback, which
            // infers its element type from the first element and then
            // rejects the rest -- hono's `Matcher<T>` (a
            // `[RegExp, HandlerData<T>[][], StaticMap<T>]` returned as
            // `Matcher<T> | null`) reported it as "expected SmeltRegExp,
            // found SmeltList<SmeltUnknown>" on element 2.
            if matches!(
                self.mir.types.get(*inner),
                Some(
                    Type::List(_)
                        | Type::Tuple(_)
                        | Type::Unknown
                        | Type::TypeParam { .. }
                        | Type::Union(_)
                )
            ) || self.is_erased_class_type(*inner)
            {
                let inner_text = self.rvalue_expr_for_dest(value, *inner)?;
                return Ok(RustExpr::chain(format!("Some({inner_text})")));
            }
        }
        // A list literal whose destination is a UNION belongs in the
        // arm that holds a collection, built at that arm's element
        // type. Falling through to the erasure below turned
        // `f([["a", "b"]])` — where the parameter is
        // `[string, string][] | Record<string, string> | Headers`, the
        // shape WHATWG spells `HeadersInit` — into an erased array the
        // union then reconstructed at runtime
        // (`SmeltUnion5::from_smelt_unknown(SmeltUnknown::Array(..))`),
        // which is two conversions and a lost type for a value whose
        // arm the compiler knows. TypeScript infers `string[][]` for a
        // bare nested literal, so the arm's own tuple spelling is
        // reached by coercing the elements, not by erasing them.
        //
        // Only a UNIQUE collection arm is claimed, the same rule
        // `inject_union_value` uses: with two list arms the
        // literal's own element types cannot say which was meant, and
        // erasure is the honest answer.
        if let Some((index, arm_ty)) =
            self.union_collection_arm_for_list_literal(dest_ty, items.len())
        {
            let inner = self.rvalue_expr_for_dest(value, arm_ty)?;
            return Ok(RustExpr::opaque(format!(
                "{}::M{index}({inner})",
                union::union_name(dest_ty)
            )));
        }
        if self.has_dynamic_shape(dest_ty)
        {
            let items_text = RustExpr::from(items
                .iter()
                .map(|item| self.erase(&self.list_literal_operand(item)))
                .collect::<Result<Vec<_>, _>>()?
                .join(", "));
            let erased = self.erase_array_expr(&items_text);
            // A concrete-union destination rebuilds the tagged union from
            // the erased array.
            if self.concrete_union_members(dest_ty).is_some() {
                return Ok({ let receiver = union::union_name(dest_ty); RustExpr::atom_on(&receiver, format!("{receiver}::from_smelt_unknown({erased})")) });
            }
            return Ok(erased);
        }
        if let Some(&Type::List(item_ty)) = self.mir.types.get(dest_ty) {
            let items_text = RustExpr::from(items
                .iter()
                .map(|item| self.value_at_type(&self.list_literal_operand(item), item_ty))
                .collect::<Result<Vec<_>, _>>()?
                .join(", "));
            // Annotate the backing Vec with the element type so heterogeneous
            // closures coerce to the shared `Rc<dyn Fn>` element (the dest
            // annotation used to drive this before `Type::List` became SmeltList).
            let elem_text = self.rendered_type(item_ty)?;
            return Ok(RustExpr::atom(format!(
                "SmeltList::from({{ let smelt_list_items: Vec<{elem_text}> = vec![{items_text}]; smelt_list_items }})"
            )));
        }
        let items_text = RustExpr::from(items
            .iter()
            .map(|item| self.operand_expr(&self.list_literal_operand(item)))
            .collect::<Result<Vec<_>, _>>()?
            .join(", "));
        Ok(RustExpr::atom(format!("vec![{items_text}]")))
    }

    /// A set literal at its destination.
    pub(super) fn set_literal_expr(
        &self,
        items: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let item_ty = match self.mir.types.get(dest_ty) {
            Some(Type::Set(item_ty)) => Some(*item_ty),
            _ => None,
        };
        // A destination that is not a `Set` keeps the hashed
        // container, as it always has.
        let container =
            item_ty.map_or("SmeltPrimSet", |set_item_ty| self.set_container_name(set_item_ty));
        if items.is_empty() {
            return Ok(RustExpr::atom_on(&container, format!("{container}::new()")));
        }
        let items_text = RustExpr::from(items
            .iter()
            .map(|item| {
                if let Some(set_item_ty) = item_ty {
                    self.value_at_type(item, set_item_ty)
                } else {
                    self.operand_expr(item)
                }
            })
            .collect::<Result<Vec<_>, _>>()?
            .join(", "));
        Ok(RustExpr::atom_on(&container, format!("{container}::from([{items_text}])")))
    }

    /// A dict literal at its destination.
    pub(super) fn dict_literal_expr(
        &self,
        entries: &[(Operand, Operand)],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        if let Some(record_text) = self.record_literal_expr_for_dest(entries, dest_ty)? {
            return Ok(record_text);
        }
        if self.has_dynamic_shape(dest_ty)
        {
            let entries_text = RustExpr::from(entries
                .iter()
                .map(|(key, entry_value)| {
                    Ok(format!(
                        "({}, {})",
                        self.value_at_type(key, self.type_id(Type::String)?)?,
                        self.erase(entry_value)?
                    ))
                })
                .collect::<Result<Vec<_>, EmitError>>()?
                .join(", "));
            return Ok(self.erase_object_expr(&entries_text));
        }
        // A source-spelled `Map` dest and a plain `Dict`/record dest share
        // the same `Rvalue::Dict` literal; `dest_is_js_map` selects the
        // `SmeltJsMap` backing (whose erasure stamps `__smelt_map`) for
        // the former, even when string-keyed.
        let dest_is_js_map = matches!(self.mir.types.get(dest_ty), Some(Type::JsMap(_, _)));
        let dict_types = match self.mir.types.get(dest_ty) {
            Some(Type::Dict(key_ty, value_ty) | Type::JsMap(key_ty, value_ty)) => {
                Some((*key_ty, *value_ty))
            }
            _ => None,
        };
        let entries_text = RustExpr::from(entries
            .iter()
            .map(|(key, entry_value)| {
                let key_text = if let Some((key_ty, _)) = dict_types {
                    if self.mir.types.get(key_ty).is_some_and(Type::is_text)
                        && self.mir.types.get(self.operand_ty(key)?).is_none_or(|ty| !ty.is_text())
                    {
                        self.property_key_to_string_expr(
                            &self.operand_expr(key)?,
                            self.operand_ty(key)?,
                        )?
                    } else {
                        self.value_at_type(key, key_ty)?
                    }
                } else {
                    self.operand_expr(key)?
                };
                let value_text = if let Some((_, value_ty)) = dict_types {
                    if matches!(self.mir.types.get(value_ty), Some(Type::Function(_))) {
                        format!(
                            "{{ let smelt_fn: {} = {}; smelt_fn }}",
                            self.rendered_type(value_ty)?,
                            self.value_at_type(entry_value, value_ty)?
                        )
                    } else {
                        self.value_at_type(entry_value, value_ty)?.into_string()
                    }
                } else {
                    self.operand_expr(entry_value)?.into_string()
                };
                Ok(format!("({key_text}, {value_text})"))
            })
            .collect::<Result<Vec<_>, EmitError>>()?
            .join(", "));
        if dest_is_js_map {
            return Ok(RustExpr::atom(self.carrier_format(format_args!("SmeltJsMap::from([{entries_text}])"))));
        }
        if let Some((key_ty, _)) = dict_types {
            if self.dict_uses_smelt_record(key_ty) {
                return Ok(RustExpr::atom(self.carrier_format(format_args!("SmeltRecord::from([{entries_text}])"))));
            }
            if self.dict_uses_js_key_map(key_ty) {
                return Ok(RustExpr::atom(self.carrier_format(format_args!("SmeltJsMap::from([{entries_text}])"))));
            }
        }
        Ok(RustExpr::loose(format!(
            "::std::collections::HashMap::from([{entries_text}])"
        )))
    }

    /// A tuple literal at its destination.
    pub(super) fn tuple_literal_expr(
        &self,
        items: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        if let Some(Type::Tuple(target_items)) = self.mir.types.get(dest_ty) {
            let offset = items.len().saturating_sub(target_items.len());
            let items_text = RustExpr::from(target_items
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
                .join(", "));
            return if target_items.len() == 1 {
                Ok(RustExpr::atom(format!("({items_text},)")))
            } else {
                Ok(RustExpr::atom(format!("({items_text})")))
            };
        }
        let items_text = RustExpr::from(items
            .iter()
            .map(|item| self.operand_expr(item))
            .collect::<Result<Vec<_>, _>>()?
            .join(", "));
        if items.is_empty() {
            Ok(RustExpr::unit())
        } else if items.len() == 1 {
            Ok(RustExpr::atom(format!("({items_text},)")))
        } else {
            Ok(RustExpr::atom(format!("({items_text})")))
        }
    }

    /// A class instance built from its field initializers.
    pub(super) fn struct_literal_expr(
        &self,
        class: Symbol,
        fields: &[(Symbol, Operand)],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let class_name = sanitize_ident(self.symbol_name(class)?);
        let mir_class = crate::classes::find_class(self.mir, class)
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
                parts.push(format!("{name}: {}", self.operand_expr(field_value)?));
            } else {
                parts.push(format!(
                    "{name}: {}",
                    self.default_value_with_scoped_type_params(
                        field.ty,
                        &TypeSubstitution::lexical(&scoped_type_params),
                    )?
                ));
            }
        }
        self.push_phantom_field(&mut parts, mir_class.name, mir_class.type_params.len());
        // A reference class wraps its inner record in a fresh shared cell.
        // Identity lives in the `Rc`, so the constructor mints one cell;
        // aliasing later clones the handle (`Rc::clone`), sharing it.
        if self.context.is_reference_class(class) {
            let allocated = RustExpr::postfix_on(&class_name, format!(
                "{class_name}(::std::rc::Rc::new(::std::cell::RefCell::new({class_name}Inner {{ {} }})))",
                parts.join(", ")
            ));
            // The class's own constructor allocates its instance here;
            // bind its virtual method slots before any statement of the
            // body can dispatch through them.
            if matches!(
                self.function.origin,
                HirOrigin::ClassConstructor { class: owner, .. } if owner == class
            ) && !self.is_base_initializer_copy()
                && let Some(bindings) =
                    self.bind_own_virtual_slots_expr(mir_class, dest_ty, "smelt_instance")?
            {
                return Ok(RustExpr::atom(format!(
                    "{{ let smelt_instance = {allocated}; {bindings} smelt_instance }}"
                )));
            }
            return Ok(allocated);
        }
        Ok(RustExpr::loose(format!("{class_name} {{ {} }}", parts.join(", "))))
    }

    /// An imported class instance at its destination.
    pub(super) fn external_class_instance_at_dest_expr(
        &self,
        class: Symbol,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let text = self.external_class_instance_expr(class, args)?;
        if self.is_regexp_class_symbol(class)?
            && matches!(self.mir.types.get(dest_ty), Some(Type::String | Type::PyStr))
        {
            Ok(Self::regexp_literal_expr(&text))
        } else {
            Ok(text)
        }
    }

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

    /// The place a compiler temporary copies from, when it is assigned exactly
    /// once by a forwarding copy or a receiver bind.
    ///
    /// `depth` bounds the walks built on this so a malformed (cyclic) assignment
    /// chain cannot loop; real chains are a handful of steps.
    pub(super) fn single_assigned_source(&self, local: LocalId, depth: usize) -> Option<&Place> {
        pub(super) const MAX_DEPTH: usize = 8;
        if depth >= MAX_DEPTH || !matches!(self.local_decl(local).ok()?.kind, LocalKind::Temp) {
            return None;
        }
        let mut assigned = self.function.blocks.iter().flat_map(|block| &block.statements).filter_map(
            |statement| match statement {
                Statement::Assign { dest, value } if *dest == local => Some(value),
                _ => None,
            },
        );
        let value = assigned.next()?;
        if assigned.next().is_some() {
            return None;
        }
        match value {
            Rvalue::Use(Operand::Copy(place) | Operand::Move(place))
            | Rvalue::BindThis {
                callee: Operand::Copy(place) | Operand::Move(place),
                ..
            } => Some(place),
            _ => None,
        }
    }

    /// Render a dictionary literal as a generated record when the destination
    /// type is a known class/interface storage shape.
    ///
    /// TypeScript object literals are structural, but generated Rust records
    /// are nominal. This path uses the destination type already known at the
    /// assignment/call site to construct the nominal Rust record directly
    /// instead of first materializing an unrelated `HashMap`.
    pub(super) fn record_literal_expr_for_dest(
        &self,
        entries: &[(Operand, Operand)],
        dest_ty: TypeId,
    ) -> Result<Option<RustExpr>, EmitError> {
        let Some(Type::Class { name, args }) = self.mir.types.get(dest_ty) else {
            return Ok(None);
        };
        if !self.is_interface_record_type(dest_ty)
            && !self.mir.has_class(*name)
        {
            return Ok(None);
        }
        let Some(fields) = self.structural_record_fields(dest_ty) else {
            return Ok(None);
        };
        if fields.is_empty() {
            return Ok(None);
        }

        // Keyed by the literal's EXACT key text, because that is a JavaScript
        // property name: case-sensitive, and never case-folded. Keying by
        // `sanitize_ident` instead silently dropped every camelCase field --
        // `{ plain: 1, camelCase: "a" }` against `interface Shape { plain?:
        // number; camelCase?: string }` emitted `Shape { plain: Some(1.0),
        // camel_case: None }`, so the program printed `undefined` where Node
        // prints `a`. The field side is what has two spellings (see
        // `symbol_source_name`), and it is the side that must be translated.
        let mut literal_entries = HashMap::new();
        for (entry_key, value) in entries {
            let Operand::Const(Constant::String(key_text)) = entry_key else {
                return Ok(None);
            };
            literal_entries.insert(key_text.as_str(), value);
        }

        let mut field_text = Vec::new();
        for field in fields {
            // Two spellings of one field: the source name matches the literal
            // key, the rendered name is the Rust struct field.
            let source_name = self.symbol_source_name(field.name)?;
            let field_name = sanitize_ident(self.symbol_name(field.name)?);
            let accessor = crate::accessor_prelude::is_accessor_field(self.mir, *name, field.name);
            let value = if let Some(entry_value) = literal_entries.get(source_name) {
                // The literal's getter becomes the field's accessor; a plain
                // value at an accessor field answers itself on every read.
                if accessor
                    && matches!(self.mir.types.get(self.operand_ty(entry_value)?), Some(Type::Function(getter)) if getter.params.is_empty())
                {
                    crate::accessor_prelude::getter_expr(&self.operand_expr(entry_value)?)
                } else if accessor {
                    crate::accessor_prelude::constant_expr(&self.value_at_type(entry_value, field.ty)?)
                } else {
                    self.value_at_type(entry_value, field.ty)?
                }
            } else if matches!(self.mir.types.get(field.ty), Some(Type::Optional(_))) {
                let default = self.default_value(field.ty)?;
                if accessor { crate::accessor_prelude::constant_expr(&default) } else { default }
            } else {
                return Ok(None);
            };
            field_text.push(format!("{field_name}: {value}"));
        }
        self.push_phantom_field(&mut field_text, *name, args.len());

        let type_name = sanitize_ident(self.symbol_name(*name)?);
        // A reference-class record is a handle newtype over its `Inner`
        // struct (it is mutated through somewhere), so the literal builds the
        // inner record inside a fresh shared cell, exactly as the structural
        // adapter does; a bare `Name { .. }` against the newtype is E0560.
        if self.is_reference_class_type(dest_ty) {
            return Ok(Some(RustExpr::postfix_on(&type_name, format!(
                "{type_name}(::std::rc::Rc::new(::std::cell::RefCell::new({type_name}Inner {{ {} }})))",
                field_text.join(", ")
            ))));
        }
        Ok(Some(RustExpr::loose(format!("{type_name} {{ {} }}", field_text.join(", ")))))
    }
}
