//! Place emission helpers.

use crate::rust::RustExpr;
use super::*;
use crate::emitter::local_binding::PlaceTarget;

impl FunctionEmitter<'_> {
    /// Render a dotted property name as the concrete key type of a dictionary.
    ///
    /// JavaScript permits dotted writes on records whose key domain widened to
    /// `unknown` (for example after adding a symbol-keyed property). Reads and
    /// writes share this conversion so `record.loop` addresses the same entry.
    pub(super) fn dict_field_key_expr(
        &self,
        key: TypeId,
        field: Symbol,
    ) -> Result<RustExpr, EmitError> {
        let field_name = self.symbol_source_name(field)?;
        if self.mir.types.get(key) == Some(&Type::String) {
            return Ok(RustExpr::js_string_literal(field_name));
        }
        if self.mir.types.get(key).is_some_and(Type::is_text) {
            return Ok(RustExpr::chain(format!("{field_name:?}.to_owned()")).owned_literal());
        }
        // A dotted member of a property-key record is the string key it names.
        if self.mir.types.get(key) == Some(&Type::PropKey) {
            let string_ty = self.type_id(Type::String)?;
            return self.rendered_at_type(&RustExpr::js_string_literal(field_name), string_ty, key, &self.render_scope());
        }
        if self.has_dynamic_shape(key)
        {
            return self.erase_rendered(
                &RustExpr::chain(format!("{field_name:?}.to_owned()")).owned_literal(),
                self.type_id(Type::String)?,
            );
        }
        self.default_value(key)
    }

    /// Whether `ty` is an optional whose payload is a union or erased value, so
    /// that only a runtime read can reach the arm a narrowing proved.
    pub(super) fn is_optional_of_dynamic_shape(&self, ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(ty),
            Some(Type::Optional(inner))
                if matches!(
                    self.mir.types.get(*inner),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                )
        )
    }

    /// Converts a place to its Rust text representation.
    pub(super) fn place_expr(&self, place: &Place) -> Result<RustExpr, EmitError> {
        match place {
            Place::Local(local) => self.local_value(*local),
            // A read of a mutable global lowers to `ExprKind::GlobalGet`, not
            // to a place, so `Place::Global` never appears in a read position.
            Place::Global { .. } => Err(EmitError::new(
                "internal: a mutable-global place was read as a value;                  reads lower to GlobalGet",
            )),
            Place::Field { base, field } => self.field_place_expr(*base, *field),
            Place::Index {
                base,
                index,
                negative,
            } => self.index_place_expr(*base, index, *negative),
        }
    }

    /// Emits a descriptor getter invocation for one statically known class field.
    pub(super) fn descriptor_getter_expr(
        &self,
        base: LocalId,
        field: Symbol,
    ) -> Result<Option<RustExpr>, EmitError> {
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
        let method_name = self.method_rust_name(method_symbol)?;
        let base_text = self.local_value(base)?;
        // A getter that can throw is emitted returning
        // `Result<_, Box<dyn Error>>`, so the READ has to propagate like any
        // other fallible call. Without the `?` the caller gets the `Result`
        // itself where the property's value is expected, and the first thing
        // done to a read value is usually `.clone()` -- which is how this
        // surfaced: `Box<dyn Error>` is not `Clone`, so a spread of a throwing
        // getter failed with `E0599` on the `Result` rather than with a type
        // mismatch naming the property. Every consumer of a property read wants
        // the value, never the `Result`, so the propagation belongs here rather
        // than in each consumer.
        //
        // `?` only travels where the ENCLOSING function returns `Result`. A
        // closure whose contextual callback type is not fallible is emitted as
        // a plain `Fn(..) -> T`, so a `?` in its body is `E0277` ("the `?`
        // operator can only be used in a closure that returns `Result`"). The
        // throw is not dropped there: it takes the same panic path every other
        // non-fallible callback seam uses (`blocker-logs/hono-fallible-ops.md`,
        // the round-6 floor), so the program still stops at the throw instead
        // of continuing with a fabricated value.
        let fallible_read = |read: RustExpr| -> RustExpr {
            if !getter.can_throw {
                return read;
            }
            if self.function.can_throw {
                return RustExpr::atom(format!("{read}?"));
            }
            RustExpr::postfix_on(&read, format!("{read}.unwrap_or_else(|error| smelt_panic_throw(error))"))
        };
        if getter_class == owner.name {
            return Ok(Some(fallible_read(RustExpr::postfix_on(&base_text, format!("{base_text}.{method_name}()")))));
        }
        let descriptor_value = self.descriptor_value(getter_class, descriptor)?;
        let arguments = getter
            .params
            .iter()
            .skip(1)
            .enumerate()
            .map(|(index, _)| {
                if index == 0 {
                    base_text.cloned_value()
                } else {
                    RustExpr::atom("Default::default()")
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        Ok(Some(fallible_read(RustExpr::postfix_on(&descriptor_value, format!(
            "{descriptor_value}.{method_name}({arguments})"
        )))))
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
            // An accessor that declares only a getter cannot be the target of a
            // SOURCE assignment -- `tsc` rejects `c.req = x` for a getter-only
            // `get req()` before Smelt ever sees it. So a write reaching here is
            // always the writeback MIR synthesizes for a PROJECTED receiver
            // (`c.req.routeIndex = i`, `PlaceWritebacks`): the projection was
            // copied into a temporary, the real write landed in that temporary,
            // and MIR replays the copy through the place it came from because it
            // cannot tell a shared handle from a value representation.
            //
            // Through a getter-only accessor that replay is always a no-op:
            // JavaScript hands the getter's object back BY REFERENCE, so the
            // mutation the temporary received is already visible in the owner,
            // and there is no setter for it to be committed through in the first
            // place. Emitting the read-only note keeps the reason in the
            // generated source instead of failing the whole crate.
            return Ok(Some(format!(
                "// `{}.{}` is a getter-only accessor: the projected write already landed through the handle it returns.",
                self.symbol_name(owner.name).unwrap_or("<class>"),
                self.symbol_name(field).unwrap_or("<field>"),
            )));
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
        let rendered = self.rvalue_expr_for_dest(value, write_ty)?;
        let method_name = self.method_rust_name(method_symbol)?;
        let base_text = self.local_mut_value(base)?;
        if setter_class == owner.name {
            return Ok(Some(format!("{base_text}.{method_name}({rendered});")));
        }
        let descriptor_value = self.descriptor_value(setter_class, descriptor)?;
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
                        base_text.cloned_value().into_string()
                    }
                } else {
                    "Default::default()".to_owned()
                }
            })
            .collect::<Vec<_>>();
        if let Some(last) = arguments.last_mut() {
            *last = rendered.into();
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
        let class = crate::classes::find_class(self.mir, *name)?;
        if let Some(descriptor) = class
            .descriptors
            .iter()
            .find(|descriptor| descriptor.name == field)
        {
            // An instance DATA property shadows a prototype accessor of the same
            // name, which is why a matching field wins here. A `Visibility::
            // Hidden` field is not an own property at all — a JavaScript `#name`
            // private field lives in a separate private-name namespace, and a
            // synthesized storage slot has no source name — so it cannot shadow
            // anything. Counting it did: `#res` and `get res()/set res()` on the
            // same class (Hono's `context.ts`) intern the same symbol, so the
            // accessor pair was ignored and `ctx.res = r` wrote the private slot
            // directly, skipping the setter's body.
            let instance_storage_shadows = !descriptor.data_descriptor
                && crate::classes::effective_class_fields(self.mir, class)
                    .iter()
                    .any(|candidate| {
                        candidate.name == field
                            && candidate.visibility != smelt_hir::Visibility::Hidden
                    });
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
    fn descriptor_value(
        &self,
        class_symbol: Symbol,
        descriptor: &MirDescriptor,
    ) -> Result<RustExpr, EmitError> {
        let descriptor_class = crate::classes::find_class(self.mir, class_symbol)
            .ok_or_else(|| EmitError::new("descriptor class is not materialized"))?;
        let class_name = crate::classes::class_name_text(self.context, descriptor_class)?;
        let fields = descriptor
            .value_fields
            .iter()
            .map(|field| {
                Ok(format!(
                    "{}: {}",
                    self.context.names().field_text(self.mir, field.name),
                    descriptor_literal_expr(&field.value)
                ))
            })
            .collect::<Result<Vec<_>, EmitError>>()?;
        if fields.is_empty() {
            Ok(RustExpr::atom(format!("{class_name}::default()")))
        } else {
            Ok(RustExpr::loose(format!(
                "{class_name} {{ {}, ..Default::default() }}",
                fields.join(", ")
            )))
        }
    }

    /// Converts a place to its Rust text representation for assignment.
    pub(super) fn assignment_place_expr(&self, place: &Place) -> Result<RustExpr, EmitError> {
        match self.assignment_place_target(place)? {
            PlaceTarget::Direct(text) | PlaceTarget::SharedCell { text, .. } => Ok(text),
        }
    }

    /// `text`, which writes through `base`, as a target behind `base`'s cell guard
    /// when `base` lives in a shared cell.
    fn place_target_through(&self, base: LocalId, text: String) -> Result<PlaceTarget, EmitError> {
        Ok(match self.local_shared_cell(base)? {
            Some(cell) => PlaceTarget::SharedCell { cell, text: RustExpr::from(text) },
            None => PlaceTarget::Direct(RustExpr::from(text)),
        })
    }

    /// The left-hand side an assignment to `place` writes.
    pub(super) fn assignment_place_target(&self, place: &Place) -> Result<PlaceTarget, EmitError> {
        match place {
            Place::Local(local) => {
                let text = self.local_mut_value(*local)?;
                self.place_target_through(*local, text.into_string())
            }
            // A write through a mutable global is emitted as one whole
            // statement (`global_place_assign_expr`) because the cell borrow
            // has to scope the mutation. There is no lvalue FRAGMENT that
            // callers could compose, so this is an error rather than a
            // best-effort spelling that would borrow twice.
            Place::Global { .. } => Err(EmitError::new(
                "write through a mutable global has no lvalue fragment;                  it is emitted as a whole statement",
            )),
            Place::Index {
                base,
                index,
                negative,
            } => {
                let base_ty = self.local_decl(*base)?.ty;
                match self.mir.types.get(base_ty) {
                    Some(Type::List(_)) => {
                        // The indexed lvalue is written through `IndexMut`, which
                        // needs a mutable borrow, but the length used to normalize
                        // the index only needs `&self`. Reading the length through
                        // the mutable form put two `borrow_mut()` of the same
                        // shared-capture `RefCell` in one expression. The primary
                        // list-index write (`emit_assign_place_statement`) never
                        // reaches this arm — it binds the index to a temp first;
                        // this lvalue is only consumed directly by block-wrapped
                        // callers such as `collection_clear_expr`.
                        let base_mut = self.local_mut_value(*base)?;
                        let base_read = self.local_value(*base)?;
                        let index_text =
                            self.normalized_index_expr(&RustExpr::postfix_on(&base_read, format!("{base_read}.len()")), index, *negative)?;
                        self.place_target_through(*base, format!("{base_mut}[{index_text}]"))
                    }
                    Some(Type::Dict(key_ty, _)) => {
                        let key_text = if self.mir.types.get(*key_ty).is_some_and(Type::is_text) {
                            let source_key = self.operand_ty(index)?;
                            let index_text = self.operand_expr(index)?;
                            self.property_key_to_string_expr(&index_text, source_key)?
                        } else {
                            self.value_at_type(index, *key_ty)?
                        };
                        Ok(PlaceTarget::Direct(RustExpr::from(format!(
                            "*{}.get_mut(&{key_text}).expect(\"index out of bounds\")",
                            self.local_mut_value(*base)?
                        ))))
                    }
                    Some(Type::Tuple(items)) => {
                        let tuple_index = self.tuple_index(index, items.len())?;
                        let text = format!("{}.{tuple_index}", self.local_mut_value(*base)?);
                        self.place_target_through(*base, text)
                    }
                    _ => Err(EmitError::new(
                        "index assignment codegen is only implemented for lists, dicts, and tuples",
                    )),
                }
            }
            Place::Field { base, field } => {
                let declared_ty = self.local_decl(*base)?.ty;
                // A receiver whose type is still optional at the WRITE. `tsc`
                // proved it present (the source either narrowed it or asserted
                // it), so the lvalue unwraps in place: `as_mut()` keeps the
                // write inside the value the binding holds, where a copy would
                // silently drop it. Without this the lvalue fell through to the
                // READ expression below (`base.as_ref().and_then(..)`), which is
                // not an lvalue at all (E0070).
                let (base_ty, base_read_text, base_write_text) =
                    if let Some(Type::Optional(inner)) = self.mir.types.get(declared_ty) {
                        (
                            *inner,
                            format!(
                                "{}.as_ref().expect(\"optional value was absent after narrowing\")",
                                self.local_value(*base)?
                            ),
                            format!(
                                "{}.as_mut().expect(\"optional value was absent after narrowing\")",
                                self.local_mut_value(*base)?
                            ),
                        )
                    } else {
                        (
                            declared_ty,
                            self.local_value(*base)?.into_string(),
                            self.local_mut_value(*base)?.into_string(),
                        )
                    };
                // A declared field of a reference class is written through a
                // narrow `borrow_mut()`. The statement's right-hand side has
                // already been reduced to an operand by MIR temping, so the
                // mutable borrow never spans a re-entrant call. Checked before
                // the structural-record path because a reference class is still
                // record-shaped structurally.
                if self.is_reference_class_type(base_ty)
                    && self.class_has_named_field(base_ty, *field)
                {
                    return Ok(PlaceTarget::Direct(RustExpr::from(format!(
                        "{base_read_text}.0.borrow_mut().{}",
                        self.context.names().field_text(self.mir, *field)
                    ))));
                }
                if self.structural_record_fields(base_ty).is_some() {
                    let text = format!(
                        "{base_write_text}.{}",
                        self.context.names().field_text(self.mir, *field)
                    );
                    return self.place_target_through(*base, text);
                }
                Ok(PlaceTarget::Direct(self.place_expr(place)?))
            }
        }
    }

    /// A typed dictionary read `base[index]`, with `base_text` the receiver.
    ///
    /// Shared by a dictionary-typed place and a dictionary arm of a concrete
    /// union (`concrete_union_index_reads`), so both answer a missing key the
    /// same way.
    pub(super) fn dict_index_read_expr(
        &self,
        base_text: &RustExpr,
        key_ty: TypeId,
        value_ty: TypeId,
        index: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let key_text = if self.mir.types.get(key_ty).is_some_and(Type::is_text) {
            let source_key = self.operand_ty(index)?;
            let index_text = self.operand_expr(index)?;
            self.property_key_to_string_expr(&index_text, source_key)?
        } else {
            self.value_at_type(index, key_ty)?
        };
        let default_value = self.default_value(value_ty)?;
        // A missing key answers `undefined`, which only a value
        // type that RENDERS as `SmeltUnknown` can hold. A union
        // with a generated enum (`SmeltUnionNNNN`) does not: the
        // record's Rust value type is that enum, so answering
        // `SmeltUnknown::Undefined` was `expected SmeltUnion127,
        // found SmeltUnknown` (9 of the router slice's errors).
        // It falls to the same `unwrap_or(default)` every other
        // concrete value type uses, whose union arm
        // `default_value` already emits. A union WITHOUT a
        // generated enum still renders as `SmeltUnknown` and
        // still answers `undefined`.
        let value_is_unknownish = matches!(
            self.mir.types.get(value_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) && self.concrete_union_members(value_ty).is_none();
        if self.dict_uses_smelt_record(key_ty) && value_is_unknownish {
            // A keyed read is a property read like a dotted one: a stored
            // accessor (an object literal's `get`) is called, and a miss
            // consults the prototype entries before answering `undefined`.
            Ok(RustExpr::chain(format!("smelt_record_property(&{base_text}, &{key_text})")))
        } else if self.dict_uses_js_key_map(key_ty) && value_is_unknownish {
            Ok(RustExpr::postfix_on(&base_text, self.carrier_format(format_args!(
                "{base_text}.get(&{key_text}).cloned().unwrap_or(SmeltUnknown::Undefined)"
            ))))
        } else if self.dict_uses_smelt_record(key_ty)
            || self.dict_uses_js_key_map(key_ty)
        {
            Ok(RustExpr::postfix_on(&base_text, format!(
                "{base_text}.get(&{key_text}).unwrap_or({default_value})"
            )))
        } else if value_is_unknownish {
            Ok(RustExpr::postfix_on(&base_text, self.carrier_format(format_args!(
                "{base_text}.get(&{key_text}).cloned().unwrap_or(SmeltUnknown::Undefined)"
            ))))
        } else {
            Ok(RustExpr::postfix_on(&base_text, format!(
                "{base_text}.get(&{key_text}).cloned().unwrap_or({default_value})"
            )))
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

    /// Converts an element index into a Rust `usize` expression for a WRITE.
    ///
    /// Negative indexes are offset from the collection length. An index that is
    /// still negative after normalization cannot address a slot, so the write
    /// form keeps the panic: silently redirecting the store to some other slot
    /// would corrupt the collection.
    pub(super) fn normalized_index_expr(
        &self,
        len_expr: &RustExpr,
        index: &Operand,
        negative: NegativeIndex,
    ) -> Result<RustExpr, EmitError> {
        self.normalized_index_expr_with_fallback(
            len_expr,
            index,
            negative,
            "usize::try_from(smelt_normalized).expect(\"negative index out of bounds\")",
        )
    }

    /// The value a list slot holds when JavaScript would answer `undefined`.
    ///
    /// This is the one "missing element" notion the emitter has, and two places
    /// must agree on it: an out-of-range element READ (`arr[99]`), and the holes
    /// `Array(n)` allocates at construction. If they disagreed, `Array(3)[0]`
    /// and `[][0]` would answer differently for the same element type.
    ///
    /// JS out-of-bounds element access is `undefined`, not `null`. A type
    /// parameter that is in scope for the current generic function is a real
    /// Rust generic, so its missing value is `Default::default()` (a `T`), not
    /// the erased `SmeltUnknown::Undefined` used for genuinely erased element
    /// types. A concrete generated union element is likewise a tagged
    /// `SmeltUnion…`, so its missing value must be a union value
    /// (`default_value` produces one) rather than an erased tag.
    pub(super) fn element_missing_value(&self, item_ty: TypeId) -> Result<RustExpr, EmitError> {
        let item_is_in_scope_type_param = matches!(
            self.mir.types.get(item_ty),
            Some(Type::TypeParam { name })
                if self.current_function_has_type_param(*name)
        );
        if item_is_in_scope_type_param {
            return self.default_value(item_ty);
        }
        if matches!(
            self.mir.types.get(item_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) && self.concrete_union_members(item_ty).is_none()
        {
            return Ok(self.carrier_expr(&RustExpr::undefined()));
        }
        self.default_value(item_ty)
    }

    /// Converts an element index into a Rust `usize` expression for a READ.
    ///
    /// Element reads are total in both source languages: an out-of-range index
    /// answers `undefined`/the missing value, never an error. A positive
    /// out-of-range index already reaches that behavior through `Vec::get`
    /// returning `None`, so a negative index that addresses no slot must reach
    /// it too. Converting the miss to `usize::MAX` (never a valid slot of a live
    /// `Vec`, whose capacity is bounded by `isize::MAX` bytes) makes the
    /// subsequent `get` miss instead of panicking, so both directions of
    /// out-of-range agree.
    ///
    /// Which negative indexes address a slot is `negative`'s question, not this
    /// one's; see [`Self::normalized_index_expr_with_fallback`].
    pub(super) fn normalized_read_index_expr(
        &self,
        len_expr: &RustExpr,
        index: &Operand,
        negative: NegativeIndex,
    ) -> Result<RustExpr, EmitError> {
        self.normalized_index_expr_with_fallback(
            len_expr,
            index,
            negative,
            "usize::try_from(smelt_normalized).unwrap_or(usize::MAX)",
        )
    }

    /// Shared body of the read/write index normalizers.
    ///
    /// `usize_conversion` is the trailing expression that turns the normalized
    /// `i64` (bound as `normalized`) into a `usize`; it is the only part that
    /// differs between a read (miss) and a write (panic).
    ///
    /// `negative` decides whether a negative subscript is normalized at all.
    /// Python counts back from the end, so `xs[-1]` becomes `len - 1` and names
    /// the last element. JavaScript does not: a negative subscript is a PROPERTY
    /// KEY, so `xs[-1]` addresses no element and reads `undefined` even when the
    /// array is non-empty. Wrapping unconditionally silently answered `xs[-1]`
    /// with `xs[len - 1]` in generated TypeScript — a wrong value, not a crash.
    /// Leaving the index alone lets the same out-of-range machinery below turn
    /// it into the miss both languages already agree on for `xs[len]`.
    /// The `smelt_`-prefixed binding names are load-bearing. This snippet is
    /// emitted INTO a user function body, and a shared capture is rewritten
    /// through that body textually (`replace_shared_capture_uses`), which cannot
    /// tell an emitter-authored binding from a use of the captured local. Hono's
    /// `compose` captures a mutable `index`, so plain `let index = ..` here came
    /// back as `let (*smelt_capture_index.borrow_mut()) = ..`, which is not a
    /// pattern and does not parse. `smelt_` is reserved and no source identifier
    /// reaches it, so the collision cannot recur.
    fn normalized_index_expr_with_fallback(
        &self,
        len_expr: &RustExpr,
        index: &Operand,
        negative: NegativeIndex,
        usize_conversion: &str,
    ) -> Result<RustExpr, EmitError> {
        if let Some(text) = self.fixed_int_index_expr(index, usize_conversion)? {
            return Ok(text);
        }
        let index_ty = self.operand_ty(index)?;
        let index_text = if matches!(self.mir.types.get(index_ty), Some(Type::Int | Type::Float)) {
            self.operand_expr(index)?
        } else {
            self.value_at_type(index, self.type_id(Type::Float)?)?
        };
        let normalize = match negative {
            NegativeIndex::FromEnd => {
                format!("let smelt_len = {len_expr} as i64; let smelt_index = {index_text} as i64; let smelt_normalized = if smelt_index < 0 {{ smelt_len + smelt_index }} else {{ smelt_index }};")
            }
            // No `len` binding: nothing consults the length when a negative
            // index cannot reach a slot, and emitting it anyway would take a
            // second borrow of the receiver inside its own `get(..)` argument.
            NegativeIndex::OutOfRange => {
                format!("let smelt_normalized = {index_text} as i64;")
            }
        };
        Ok(RustExpr::atom(format!("{{ {normalize} {usize_conversion} }}")))
    }

    /// A JS element read that flows into an optional slot keeps its fallibility.
    ///
    /// `arr[i]` has TypeScript type `T` (without `noUncheckedIndexedAccess`),
    /// so a source function such as `last<T>(arr: T[]): T | undefined` lowers to
    /// an infallible read coerced into `Option<T>` — which produced
    /// `Some(Default::default())` for an out-of-range index instead of `None`.
    /// When the coercion target is `Option<..>` the read itself is the natural
    /// `Option` producer, so emit `get(..).cloned()` and let the miss stay a
    /// miss. Returns `None` when the operand is not an element read this rule
    /// applies to, leaving the caller's ordinary coercion in place.
    pub(super) fn optional_element_read_expr(
        &self,
        operand: &Operand,
        inner: TypeId,
    ) -> Result<Option<RustExpr>, EmitError> {
        let (Operand::Copy(Place::Index {
            base,
            index,
            negative,
        })
        | Operand::Move(Place::Index {
            base,
            index,
            negative,
        })) = operand
        else {
            return Ok(None);
        };
        let base_ty = self.local_decl(*base)?.ty;
        // A `Match` receiver has its own keyed-read lowering; leave it alone.
        if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
            && self.is_match_class_symbol(*name)?
        {
            return Ok(None);
        }
        // A typed-array view's element read misses like a list's: `u8[5]` on
        // a two-byte view is `undefined` where the reader can see it.
        if self.is_typed_array_view_class_type(base_ty)? {
            let base_text = self.local_value(*base)?;
            let index_text = self.numeric_operand_expr(index)?;
            let element_ty = if self.is_bigint_view_class_type(base_ty)? { Type::BigInt } else { Type::Float };
            if self.mir.types.get(inner) == Some(&element_ty) {
                let method = if element_ty == Type::BigInt { "get_bigint" } else { "get" };
                return Ok(Some(RustExpr::postfix_on(&base_text, format!("{base_text}.{method}({index_text})"))));
            }
            return Ok(None);
        }
        match self.mir.types.get(base_ty).cloned() {
            Some(Type::List(item_ty)) => {
                let base_text = self.local_value(*base)?;
                let index_text =
                    self.normalized_read_index_expr(&RustExpr::postfix_on(&base_text, format!("{base_text}.len()")), index, *negative)?;
                // Read borrow, as in the total list index read above: the
                // `.len()` in the index argument borrows the same shared cell.
                let read = { let receiver = list_read_expr(&base_text); RustExpr::postfix_on(&receiver, format!("{receiver}.get({index_text}).cloned()")) };
                if item_ty == inner {
                    return Ok(Some(read));
                }
                // `T[][i]` where the element is itself optional collapses the
                // two layers the same way JS does: a missing slot and a present
                // `undefined` are both `undefined`.
                if self.mir.types.get(item_ty) == Some(&Type::Optional(inner)) {
                    return Ok(Some(RustExpr::postfix_on(&read, format!("{read}.flatten()"))));
                }
                // A missing slot and a present erased `null`/`undefined` are
                // both the absent case (`optional_value_map_expr`).
                let Ok(mapped) = self.optional_value_map_expr(&read, item_ty, inner, &self.render_scope()) else {
                    return Ok(None);
                };
                Ok(Some(mapped))
            }
            Some(Type::String | Type::PyStr) if self.mir.types.get(inner).is_some_and(Type::is_text) => {
                let base_text = self.local_value(*base)?;
                let index_text = self
                    .normalized_read_index_expr(&self.string_len_expr(&base_text), index, *negative)?;
                Ok(Some(RustExpr::chain(
                    self.string_char_at_text(&base_text.parenthesized_if_needed(), index_text.as_str()),
                )))
            }
            // A keyed RECORD read (`rec[key]`) misses exactly like an element
            // read: a missing key is `undefined`, which the record's own
            // `get` already answers as `None` (an owned `Option<V>` for the
            // `SmeltRecord` a string-keyed record is; other key types render
            // other maps and keep the caller's ordinary coercion).
            Some(Type::Dict(key_ty, value_ty)) if self.dict_uses_smelt_record(key_ty) => {
                let base_text = self.local_value(*base)?;
                let key_text = self.dict_key_reference_expr(index, key_ty)?;
                let read = RustExpr::postfix_on(&base_text, format!("{base_text}.get({key_text})"));
                if value_ty == inner {
                    return Ok(Some(read));
                }
                if self.mir.types.get(value_ty) == Some(&Type::Optional(inner)) {
                    return Ok(Some(RustExpr::postfix_on(&read, format!("{read}.flatten()"))));
                }
                let Ok(mapped) = self.rendered_at_type(&RustExpr::path("value"), value_ty, inner, &self.render_scope())
                else {
                    return Ok(None);
                };
                Ok(Some(RustExpr::postfix_on(&read, format!("{read}.map(|value| {mapped})"))))
            }
            _ => Ok(None),
        }
    }

    /// A JS element read that flows into an ERASED slot keeps its fallibility.
    ///
    /// The erased twin of [`Self::optional_element_read_expr`], and the two must
    /// agree: an out-of-range element read is `undefined` in JavaScript, so the
    /// optional target answers `None` and the erased target must answer
    /// `SmeltUnknown::Undefined`.
    ///
    /// Without this the read was made TOTAL first and erased afterwards, so the
    /// miss became the element type's own missing value
    /// ([`Self::element_missing_value`]) and erased as that value: for
    /// `b: string[]`, `row[i] = b[99]` into an erased `row` stored `''`, and for
    /// `number[]` it stored `0`, where JavaScript stores `undefined`. Emitting
    /// `get(..).cloned().map(erase).unwrap_or(SmeltUnknown::Undefined)` keeps the
    /// miss a miss and erases only the values that were really there.
    ///
    /// This adds no new erasure: the caller has already decided the destination
    /// is `SmeltUnknown`, and the rule only changes WHICH tag a miss produces.
    ///
    /// Returns `None` — leaving the caller's ordinary erase-after-read in place —
    /// when the operand is not an element read this rule applies to, or when the
    /// total read already erases its miss to `SmeltUnknown::Undefined` (an
    /// already-erased element type), so those emissions stay byte-identical.
    pub(super) fn erased_element_read_expr(
        &self,
        operand: &Operand,
    ) -> Result<Option<RustExpr>, EmitError> {
        let (Operand::Copy(Place::Index {
            base,
            index,
            negative,
        })
        | Operand::Move(Place::Index {
            base,
            index,
            negative,
        })) = operand
        else {
            return Ok(None);
        };
        let base_ty = self.local_decl(*base)?.ty;
        // A `Match` receiver has its own keyed-read lowering; leave it alone.
        if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
            && self.is_match_class_symbol(*name)?
        {
            return Ok(None);
        }
        // A typed-array view's miss erases to `undefined`, its values to numbers.
        if self.is_typed_array_view_class_type(base_ty)? {
            let base_text = self.local_value(*base)?;
            let index_text = self.numeric_operand_expr(index)?;
            let (method, element) = if self.is_bigint_view_class_type(base_ty)? {
                ("get_bigint", Type::BigInt)
            } else {
                ("get", Type::Float)
            };
            let erased = self.erase_rendered(&RustExpr::path("smelt_element"), self.type_id(element)?)?;
            return Ok(Some(RustExpr::postfix_on(&base_text, self.carrier_format(format_args!(
                "{base_text}.{method}({index_text}).map(|smelt_element| {erased}).unwrap_or(SmeltUnknown::Undefined)"
            )))));
        }
        match self.mir.types.get(base_ty).cloned() {
            Some(Type::List(item_ty)) => {
                // The existing total read is already correct whenever erasing
                // its missing value yields `Undefined` (element types that are
                // themselves erased). Skipping those keeps their output stable.
                let missing = self.element_missing_value(item_ty)?;
                if self.erase_rendered(&missing, item_ty)?.is_undefined() {
                    return Ok(None);
                }
                let base_text = self.local_value(*base)?;
                let index_text =
                    self.normalized_read_index_expr(&RustExpr::postfix_on(&base_text, format!("{base_text}.len()")), index, *negative)?;
                let erased_value = self.erase_rendered(&RustExpr::path("value"), item_ty)?;
                let read_expr = list_read_expr(&base_text);
                Ok(Some(RustExpr::postfix_on(&read_expr, self.carrier_format(format_args!(
                    "{read_expr}.get({index_text}).cloned().map(|value| {erased_value}).unwrap_or(SmeltUnknown::Undefined)"
                )))))
            }
            Some(Type::String | Type::PyStr) => {
                let base_text = self.local_value(*base)?;
                let index_text = self
                    .normalized_read_index_expr(&self.string_len_expr(&base_text), index, *negative)?;
                let char_at = self.string_char_at_text(&base_text.parenthesized_if_needed(), index_text.as_str());
                Ok(Some(RustExpr::postfix_on(&RustExpr::chain(char_at.clone()), self.carrier_format(format_args!(
                    "{char_at}.map(|ch| SmeltUnknown::String(ch.into())).unwrap_or(SmeltUnknown::Undefined)"
                )))))
            }
            // A keyed RECORD read misses like an element read (a missing key
            // is `undefined`), and `SmeltRecord::get` already answers the miss
            // as `None`, so only the present value is erased. Same scope as
            // the optional twin's record arm: string-keyed `SmeltRecord`s only,
            // and element types whose total miss would not already erase to
            // `Undefined`.
            Some(Type::Dict(key_ty, value_ty)) if self.dict_uses_smelt_record(key_ty) => {
                let missing = self.element_missing_value(value_ty)?;
                if self.erase_rendered(&missing, value_ty)?.is_undefined() {
                    return Ok(None);
                }
                let base_text = self.local_value(*base)?;
                let key_text = self.dict_key_reference_expr(index, key_ty)?;
                let erased_value = self.erase_rendered(&RustExpr::path("value"), value_ty)?;
                Ok(Some(RustExpr::postfix_on(&base_text, self.carrier_format(format_args!(
                    "{base_text}.get({key_text}).map(|value| {erased_value}).unwrap_or(SmeltUnknown::Undefined)"
                )))))
            }
            _ => Ok(None),
        }
    }

    /// Emit a runtime index read for values whose concrete shape is erased.
    ///
    /// TypeScript generic and unknown receivers may still be strings, arrays,
    /// or objects at runtime. Returning `Null` here hides lowering bugs and
    /// breaks later casts, so the generated Rust dispatches on `SmeltUnknown`
    /// and answers `SmeltUnknown::Undefined` when the runtime value is not indexable.
    ///
    /// An out-of-range ELEMENT read on a string or array runtime value answers
    /// `SmeltUnknown::Undefined`, not `Null` — the same rule the typed reads use
    /// ([`Self::element_missing_value`],
    /// [`Self::erased_element_read_expr`]), and the reason `zipWith` on ragged
    /// inputs produced `"3null"` where JavaScript produces `"3undefined"`.
    /// Whether a missing OBJECT PROPERTY should likewise be `Undefined` is a
    /// separate question about property access and is deliberately left alone.
    ///
    /// The OBJECT arm reads through `smelt_get_object_field`, the same helper
    /// the erased STATIC field read uses. In JavaScript `o[k]` and `o.k` are one
    /// operation, so the two spellings have to answer identically; when this
    /// arm carried its own inlined `byte_buffer_element(..).unwrap_or_else(||
    /// values.get(..))` they diverged, and every marker-record rule the static
    /// read grew (`err.name` off `__smelt_error`, `x.constructor`, a `Map`'s
    /// `size` and iteration methods, the global object's builtin constructors)
    /// was invisible to the computed spelling.
    pub(super) fn unknown_index_expr(
        &self,
        base_text: &RustExpr,
        index: &Operand,
    ) -> Result<RustExpr, EmitError> {
        // `base_text` is usually already an owned temporary (an operand render
        // clones the local it reads), so take an owned copy rather than
        // deep-copying the erased base a second time on every index read.
        let base_text = &base_text.cloned_value();
        let index_ty = self.operand_ty(index)?;
        let index_text = self.operand_expr(index)?;
        let key_text = self.property_key_to_string_expr(&index_text, index_ty)?;
        if matches!(self.mir.types.get(index_ty), Some(Type::String | Type::PyStr)) {
            return Ok(RustExpr::atom(format!(
                r#"match {base_text} {{
                    SmeltUnknown::String(value) => {{
                        let smelt_key = {key_text};
                        if smelt_key == "length" {{
                            SmeltUnknown::Number({string_len} as f64)
                        }} else {{
                            smelt_key.to_string().parse::<usize>().ok().and_then(|index| {char_at}.map(|ch| SmeltUnknown::String(ch.into()))).unwrap_or(SmeltUnknown::Undefined)
                        }}
                    }}
                    SmeltUnknown::Array(values) => smelt_get_array_field(&values, &{key_text}),
                    SmeltUnknown::Object(values) => smelt_get_object_field(&values, &{key_text}),
                    _ => SmeltUnknown::Undefined,
                }}"#,
                string_len = self.string_len_text("value"),
                char_at = self.string_char_at_text("value", "index"),
            )));
        }
        let numeric_index_text = match self.mir.types.get(index_ty) {
            Some(Type::Int | Type::Float) => index_text,
            Some(Type::Bool) => RustExpr::loose(format!("if {index_text} {{ 1.0 }} else {{ 0.0 }}")),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            | Some(Type::Class { .. })
                if self.is_erased_class_type(index_ty)
                    || matches!(
                        self.mir.types.get(index_ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    ) =>
            {
                let scrutinee =
                    self.erase_concrete_union_expr(&index_text.cloned_value(), index_ty);
                RustExpr::atom(self.carrier_format(format_args!(
                    "match {scrutinee} {{ SmeltUnknown::Number(value) => value, SmeltUnknown::String(value) => smelt_js_string_to_number(&value), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, {smelt_bigint_arm}SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }}", smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => smelt_bigint_to_number(&value),")
                )))
            }
            _ => RustExpr::atom("f64::NAN"),
        };

        Ok(RustExpr::atom(format!(
            r"match {base_text} {{
                    SmeltUnknown::String(value) => {{
                        let smelt_len = {string_len} as i64;
                        let smelt_index = {numeric_index_text} as i64;
                        let smelt_normalized = if smelt_index < 0 {{ smelt_len + smelt_index }} else {{ smelt_index }};
                        usize::try_from(smelt_normalized).ok().and_then(|index| {char_at}.map(|ch| SmeltUnknown::String(ch.into()))).unwrap_or(SmeltUnknown::Undefined)
                    }}
                    SmeltUnknown::Array(values) => {{
                        let smelt_len = values.len() as i64;
                        let smelt_index = {numeric_index_text} as i64;
                        let smelt_normalized = if smelt_index < 0 {{ smelt_len + smelt_index }} else {{ smelt_index }};
                        usize::try_from(smelt_normalized).ok().and_then(|index| values.get(index).cloned()).unwrap_or(SmeltUnknown::Undefined)
                    }}
                SmeltUnknown::Object(values) => smelt_get_object_field(&values, &{key_text}),
                _ => SmeltUnknown::Undefined,
            }}",
            string_len = self.string_len_text("value"),
            char_at = self.string_char_at_text("value", "index"),
        )))
    }

}

/// Converts a statically materialized descriptor literal to Rust source.
fn descriptor_literal_expr(literal: &smelt_hir::Literal) -> RustExpr {
    match literal {
        smelt_hir::Literal::Bool(boolean) => RustExpr::path(boolean.to_string()),
        smelt_hir::Literal::Int(integer) => RustExpr::opaque(integer.to_string()),
        smelt_hir::Literal::BigInt(digits) => crate::bigint_prelude::literal_expr(digits),
        smelt_hir::Literal::Float(number) => {
            if number.is_nan() {
                RustExpr::atom("f64::NAN")
            } else if number.is_infinite() && number.is_sign_positive() {
                RustExpr::atom("f64::INFINITY")
            } else if number.is_infinite() {
                RustExpr::atom("f64::NEG_INFINITY")
            } else {
                RustExpr::chain(format!("{number:?}"))
            }
        }
        smelt_hir::Literal::String(text) => RustExpr::chain(format!("{text:?}.to_owned()")).owned_literal(),
        smelt_hir::Literal::StringUnits(units) => RustExpr::js_string_units_literal(units),
        smelt_hir::Literal::Symbol(_)
        | smelt_hir::Literal::Undefined
        | smelt_hir::Literal::None => RustExpr::atom("Default::default()"),
    }
}

impl FunctionEmitter<'_> {
    /// A field read.
    fn field_place_expr(&self, base: LocalId, field: Symbol) -> Result<RustExpr, EmitError> {
        let base_ty = self.local_decl(base)?.ty;
        if let Some(Type::Dict(key, value)) = self.mir.types.get(base_ty) {
            let key_text = self.dict_field_key_expr(*key, field)?;
            let base_text = self.local_value(base)?;
            if matches!(self.mir.types.get(*value), Some(Type::Optional(_))) {
                if self.dict_uses_smelt_record(*key) || self.dict_uses_js_key_map(*key) {
                    return Ok(RustExpr::postfix_on(&base_text, format!("{base_text}.get(&{key_text}).flatten()")));
                }
                return Ok(RustExpr::postfix_on(&base_text, format!("{base_text}.get(&{key_text}).cloned().flatten()")));
            }
            if self.mir.types.get(*key).is_some_and(Type::is_text)
                && self.renders_erased(*value)?
            {
                // A property the record does not hold reads as
                // `undefined` in JavaScript, never `null`. The two are
                // distinguishable under `===`, so answering `Null` here
                // silently defeats every `value === undefined` guard the
                // source writes over a dynamic record. A DOTTED read is a
                // property read, so a miss consults the prototype entries
                // an erased class instance carries before answering
                // `undefined` (`err.getResponse` after `'getResponse' in
                // err` narrowed an erased error to a record arm). A stored
                // accessor (an object literal's `get`) is called.
                return Ok(RustExpr::chain(format!(
                    "smelt_record_property(&{base_text}, &{key_text})"
                )));
            }
            if self.dict_uses_smelt_record(*key) || self.dict_uses_js_key_map(*key) {
                return Ok(RustExpr::postfix_on(&base_text, format!(
                    "{base_text}.get(&{key_text}).expect(\"missing field\")"
                )));
            }
            return Ok(RustExpr::postfix_on(&base_text, format!(
                "{base_text}.get(&{key_text}).cloned().expect(\"missing field\")"
            )));
        }
        // A `.length` read on a CONCRETE collection (typed list or set)
        // projects to the backing `Vec`/`HashSet` length as an `f64`,
        // matching how list mutation helpers surface `.len() as f64`.
        // Without this arm the read falls through to the raw struct-field
        // fallback (`base.length`), which no collection type has. This
        // mirrors the erased-receiver `TsLength` arm below but stays on
        // the concrete representation instead of erasing.
        if matches!(
            self.mir.types.get(base_ty),
            Some(Type::List(_) | Type::Set(_))
        ) && smelt_stdlib::typescript_field_rule(self.symbol_source_name(field)?)
            == Some(smelt_stdlib::FieldRule::TsLength)
        {
            return Ok(RustExpr::atom(format!("({}.len() as f64)", self.local_value(base)?)));
        }
        // A tuple's `.length` is its arity, fixed by its type. Rust
        // tuples have no `length` field, so the raw fallback below
        // was E0609.
        if let Some(Type::Tuple(items)) = self.mir.types.get(base_ty)
            && smelt_stdlib::typescript_field_rule(self.symbol_source_name(field)?)
                == Some(smelt_stdlib::FieldRule::TsLength)
        {
            return Ok(RustExpr::chain(format!("{}.0", items.len())));
        }
        // A value whose static type is a GENERATED union is dispatched
        // through its own enum arms: when every arm declares the
        // property at one type, the read is that type and each arm
        // projects its own struct field. Erasing first would throw away
        // a type the source already fixed, and in a crate that interns
        // no `unknown` there is no erased carrier to answer with at all.
        if smelt_stdlib::typescript_field_rule(self.symbol_source_name(field)?)
            == Some(smelt_stdlib::FieldRule::TsLength)
            && let Some(text) = self.concrete_union_length_read_expr(base)?
        {
            return Ok(text);
        }
        if let Some(text) = self.concrete_union_field_read_expr(base, field)? {
            return Ok(text);
        }
        // A `never` receiver (a callback parameter typed `T[keyof T]`
        // over `{}`) has no value of its own and is stored erased, so
        // a read through it is the erased read too. Falling through to
        // the struct-field fallback named whichever generated struct
        // happens to declare the field.
        if matches!(
            self.mir.types.get(base_ty),
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. } | Type::Never)
        ) || self.is_erased_class_type(base_ty)
        {
            let field_name = self.symbol_source_name(field)?;
            let field_rule = smelt_stdlib::typescript_field_rule(field_name);
            let base_text = self.local_value(base)?;
            // A concrete-union receiver stores a tagged enum; project it
            // back to `SmeltUnknown` before the erased-object field match.
            let scrutinee =
                self.erase_concrete_union_expr(&base_text.cloned_value(), base_ty);
            if field_rule == Some(smelt_stdlib::FieldRule::TsLength) {
                // A callable's `.length` is its declared arity, which the
                // erasure boundary parks in the function-length registry
                // (an `Rc<dyn Fn>` has nowhere to carry it). Without this
                // arm an erased callable answered `null`, and es-toolkit
                // `rest(func)` — whose default split point is
                // `func.length - 1` — reshaped every call wrongly.
                return Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "match {scrutinee} {{ SmeltUnknown::String(value) => SmeltUnknown::Number({string_len} as f64), SmeltUnknown::Array(value) => SmeltUnknown::Number(value.len() as f64), value @ SmeltUnknown::Function(_) => SmeltUnknown::Number({function_length}(&value)), SmeltUnknown::Object(map) => match smelt_get_object_field(&map, \"length\") {{ SmeltUnknown::Undefined | SmeltUnknown::Null if map.contains_key(\"{SMELT_CALL}\") => SmeltUnknown::Number({function_length}(&SmeltUnknown::Object(map))), value => value }}, _ => SmeltUnknown::Null }}",
                    function_length =
                        smelt_stdlib::runtime_symbols::function_length::READ,
                    string_len = self.string_len_text("value"),
                ))));
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
                return Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "match {scrutinee} {{ SmeltUnknown::Object(map) => smelt_host_method(&map, {field_name:?}).unwrap_or_else(|| smelt_get_object_field(&map, {field_name:?})), _ => SmeltUnknown::Undefined }}"
                ))));
            }
            // `Function.prototype.apply`/`call` must resolve when the
            // erased receiver is a `SmeltUnknown::Function` (not just an
            // object). The plain object field match below returns
            // `Undefined` for a function receiver, collapsing every
            // invocation to a null callback (see `partial`/`partialRight`).
            // The runtime helper binds the callable with the correct
            // this-dropping/argument-spreading semantics and falls back to
            // the ordinary field read for object receivers.
            if matches!(field_name, "apply" | "call") {
                return Ok(RustExpr::chain(format!(
                    "smelt_function_method({scrutinee}, {field_name:?})"
                )));
            }
            // `Object.prototype.valueOf` exists on EVERY value, so it can
            // never resolve through the own-field read: a boxed primitive
            // (`Object(1)`, `new Number(1)`) is a marker object with no own
            // `valueOf`, and an erased primitive is not an object at all.
            // Both used to collapse to a null callback, which is what made
            // `isEqualWith(1, Object(1), noop)` answer `false`. The runtime
            // helper unwraps the wrapper and still prefers a user-defined
            // own `valueOf` when the receiver has one.
            if field_name == "valueOf" {
                return Ok(RustExpr::chain(format!("smelt_value_of_method({scrutinee})")));
            }
            // Every receiver shape through one prelude helper: an object
            // record, an erased ARRAY (whose `length`, indices and named
            // side-table properties are all readable), and the
            // `Object.prototype` sentinel. Inlining an object-only `match`
            // here answered `undefined` for `arr.x` and for
            // `Object.prototype.toString`.
            return Ok(RustExpr::chain(format!(
                "smelt_get_unknown_field(&{scrutinee}, {field_name:?})"
            )));
        }
        self.optional_or_host_field_place_expr(base, field, base_ty)
    }

    /// A field read through an optional receiver or a host class.
    fn optional_or_host_field_place_expr(
        &self,
        base: LocalId,
        field: Symbol,
        base_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        if let Some(Type::Optional(inner)) = self.mir.types.get(base_ty)
            && (matches!(
                self.mir.types.get(*inner),
                Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
            ) || self.is_erased_class_type(*inner))
        {
            let field_name = self.symbol_source_name(field)?;
            let base_text = self.local_value(base)?;
            if smelt_stdlib::typescript_field_rule(field_name)
                == Some(smelt_stdlib::FieldRule::TsLength)
            {
                return Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "match {base_text}.clone().unwrap_or(SmeltUnknown::Null) {{ SmeltUnknown::String(value) => SmeltUnknown::Number({string_len} as f64), SmeltUnknown::Array(value) => SmeltUnknown::Number(value.len() as f64), SmeltUnknown::Object(map) => smelt_get_object_field(&map, \"length\"), _ => SmeltUnknown::Null }}",
                    string_len = self.string_len_text("value"),
                ))));
            }
            return Ok(RustExpr::chain(self.carrier_format(format_args!(
                "smelt_get_unknown_field(&{base_text}.clone().unwrap_or(SmeltUnknown::Undefined), {field_name:?})"
            ))));
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(base_ty)
            && let Some(Type::Dict(key_ty, _)) = self.mir.types.get(*inner)
            && self.mir.types.get(*key_ty).is_some_and(Type::is_text)
        {
            let field_name = self.symbol_source_name(field)?;
            let base_text = self.local_value(base)?;
            return Ok(RustExpr::postfix_on(&base_text, format!(
                "{base_text}.as_ref().and_then(|_smelt_value| _smelt_value.get({field_name:?}))"
            )));
        }
        if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
            && self.is_regexp_class_symbol(*name)?
        {
            return self.regexp_field_expr(&self.local_value(base)?, field);
        }
        if self.is_url_search_params_class_type(base_ty)? {
            return self
                .url_search_params_field_expr(&self.local_value(base)?, field);
        }
        if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
            && let Some(kind) = self.match_class_kind(*name)?
        {
            return self.match_field_expr(&self.local_value(base)?, kind, field);
        }
        // A REFERENCE class reached through an optional receiver keeps
        // its handle: its fields live behind `Rc<RefCell<Inner>>`, so
        // the read is `.0.borrow().field`, never a field of the handle
        // itself. Checked before the structural-record arm below because
        // a reference class is still record-shaped structurally — that
        // arm emitted `_smelt_value.field` and the generated crate did
        // not compile (E0609).
        if let Some(Type::Optional(inner)) = self.mir.types.get(base_ty)
            && self.is_reference_class_type(*inner)
            && self.class_has_named_field(*inner, field)
            && let Some(field_ty) = self
                .structural_record_fields(*inner)
                .and_then(|fields| {
                    fields
                        .iter()
                        .find(|candidate| candidate.name == field)
                        .map(|candidate| candidate.ty)
                })
        {
            let base_text = self.local_value(base)?;
            let field_name = self.context.names().field_text(self.mir, field);
            let read = format!("_smelt_value.0.borrow().{field_name}.clone()");
            return if matches!(self.mir.types.get(field_ty), Some(Type::Optional(_))) {
                Ok(RustExpr::postfix_on(&base_text, format!("{base_text}.as_ref().and_then(|_smelt_value| {read})")))
            } else {
                Ok(RustExpr::postfix_on(&base_text, format!("{base_text}.as_ref().map(|_smelt_value| {read})")))
            };
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(base_ty)
            && let Some(fields) = self.structural_record_fields(*inner)
            && let Some(field_ty) = fields
                .iter()
                .find(|candidate| candidate.name == field)
                .map(|candidate| candidate.ty)
        {
            let base_text = self.local_value(base)?;
            let field_name = self.context.names().field_text(self.mir, field);
            let read = if crate::accessor_prelude::is_accessor_field_of_type(self.mir, *inner, field) {
                crate::accessor_prelude::read_expr(&RustExpr::path(format!("_smelt_value.{field_name}")))
            } else {
                RustExpr::chain(format!("_smelt_value.{field_name}.clone()")).ending_in_clone()
            };
            return if matches!(self.mir.types.get(field_ty), Some(Type::Optional(_))) {
                Ok(RustExpr::postfix_on(&base_text, format!("{base_text}.as_ref().and_then(|_smelt_value| {read})")))
            } else {
                Ok(RustExpr::postfix_on(&base_text, format!("{base_text}.as_ref().map(|_smelt_value| {read})")))
            };
        }
        self.named_field_place_expr(base, field, base_ty)
    }

    /// A field read by name, on function values, strings and classes.
    fn named_field_place_expr(
        &self,
        base: LocalId,
        field: Symbol,
        base_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        // A JavaScript function is an object: a property read on one
        // answers its own-property bag, where `prototype` is
        // materialised on first read and where a source-written member
        // (`wrapper.placeholder`, `partialed.prototype`) lands. The
        // receiver is erased first so the read shares the one bag every
        // representation of that function reads through, and so
        // `prototype.constructor` can be the function itself.
        //
        // `Function.prototype`'s own members keep their existing
        // answers: `apply`/`call`/`bind` are resolved by the
        // callable-interface and erased-receiver paths, and `length` /
        // `name` by the arity registry, so routing them here would
        // change which helper answers them.
        if matches!(self.mir.types.get(base_ty), Some(Type::Function(_))) {
            let field_name = self.symbol_source_name(field)?;
            if matches!(field_name, "apply" | "call" | "bind" | "length" | "name") {
                return Ok(self.null_value());
            }
            let base_text = self.local_value(base)?;
            let erased = self.erase_rendered(&base_text.cloned_value(), base_ty)?;
            return Ok(RustExpr::chain(format!(
                "smelt_get_unknown_field(&{erased}, {field_name:?})"
            )));
        }
        if smelt_stdlib::typescript_field_rule(self.symbol_source_name(field)?)
            == Some(smelt_stdlib::FieldRule::TsConstructor)
        {
            return Ok(self.null_value());
        }
        // `.apply` / `.call` / `.bind` on a callable-interface value are
        // Function.prototype methods on the *underlying callable*, not
        // named struct fields. The interface struct has no such field
        // (reading `receiver.apply` is an `E0609`), so read the synthetic
        // `__smelt_call` storage slot and erase that callable to a
        // `SmeltUnknown::Function`. The erased-call coercion the caller
        // wraps this read in extracts and invokes it; `.bind` routes the
        // same way and yields a closure over the erased callable. Erasing
        // only the `__smelt_call` field (rather than the whole struct)
        // keeps the callable live — a whole-struct `into_smelt_unknown`
        // erases every function field to `Null`.
        if matches!(self.symbol_source_name(field)?, "apply" | "call" | "bind")
            && let Some(call_ty) = self.callable_interface_call_field_ty(base_ty)
        {
            let call_field = { let receiver = self.local_value(base)?; RustExpr::postfix_on(&receiver, format!("{receiver}.{SMELT_CALL}.clone()")) }.ending_in_clone();
            return self.erase_rendered(&call_field, call_ty);
        }
        if matches!(self.mir.types.get(base_ty), Some(Type::String | Type::PyStr)) {
            return self.string_field_expr(&self.local_value(base)?, field);
        }
        if let Some(getter) = self.descriptor_getter_expr(base, field)? {
            return Ok(getter);
        }
        if self.storage_field_is_function(base_ty, field) {
            // A function-typed field of a REFERENCE class lives inside
            // the shared cell like every other declared field, so it is
            // read through `.0.borrow()`. Without this the arm below
            // named a field the handle newtype does not have (E0609);
            // it is reached before the general reference-class field arm
            // further down, so the projection has to be repeated here.
            if self.is_reference_class_type(base_ty)
                && self.class_has_named_field(base_ty, field)
            {
                return Ok({ let receiver = self.local_value(base)?; RustExpr::postfix_on(&receiver, format!("{receiver}.0.borrow().{}.clone()", self.context.names().field_text(self.mir, field))) }.ending_in_clone());
            }
            return Ok({ let receiver = self.local_value(base)?; RustExpr::postfix_on(&receiver, format!("{receiver}.{}.clone()", self.context.names().field_text(self.mir, field))) }.ending_in_clone());
        }
        if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
            && let Some(method_text) = self.class_method_reference_expr(
                &self.local_value(base)?,
                *name,
                field,
            )?
        {
            return Ok(method_text);
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(base_ty)
            && matches!(self.mir.types.get(*inner), Some(Type::Function(_)))
        {
            return Ok(self.null_value());
        }
        // A dotted read of an UNDECLARED member on an index-signature
        // class (`bag.name` where `name` is not a struct field) is a
        // keyed lookup into the runtime store (issue #84). Declared
        // fields keep their concrete struct access via the fallback
        // below; only names with no matching field route to the store.
        if let Some((key_ty, value_ty)) = self.class_index_store_types(base_ty)
            && !self.class_has_named_field(base_ty, field)
        {
            let base_text = self.local_value(base)?;
            let key = self.dict_field_key_expr(key_ty, field)?;
            let store_text =
                RustExpr::postfix_on(&base_text, format!("{base_text}.{}", smelt_hir::CLASS_INDEX_STORE_FIELD));
            let default_value = self.default_value(value_ty)?;
            let string_key_ty = self.type_id(Type::String)?;
            let getter = if self.dict_uses_smelt_record(string_key_ty)
                || self.dict_uses_js_key_map(string_key_ty)
            {
                RustExpr::postfix_on(&store_text, format!("{store_text}.get(&{key})"))
            } else {
                RustExpr::postfix_on(&store_text, format!("{store_text}.get(&{key}).cloned()"))
            };
            return Ok(RustExpr::postfix_on(&getter, format!("{getter}.unwrap_or({default_value})")));
        }
        // A dotted read of a member a CALLABLE-OBJECT record does not
        // declare. JavaScript lets a function own arbitrary properties,
        // and TypeScript accepts such a read wherever the receiver's
        // static shape is widened (or the author suppressed the check —
        // es-toolkit's `flow` spec reads `curried.placeholder` off a
        // `CurriedFunction1`). The struct has no such Rust field, so the
        // plain fallback below emitted `receiver.placeholder`, an E0609
        // in the generated crate. The value's own properties live in the
        // erased callable's property bag, so read the source-spelled key
        // from there and answer `undefined` when it carries none —
        // exactly what JavaScript answers for an absent property.
        if !self.class_has_named_field(base_ty, field)
            && self.callable_interface_call_field_ty(base_ty).is_some()
        {
            let base_text = self.local_value(base)?;
            let key = self.symbol_source_name(field)?;
            return Ok(RustExpr::postfix_on(&base_text, format!("{base_text}.{SMELT_CALL}.smelt_property({key:?})")));
        }
        // A declared field of a reference class lives inside the shared
        // cell. Read it through a narrow `borrow()` and clone the value
        // out so the borrow guard is a short-lived temporary that ends
        // with the statement — never held across a re-entrant call.
        if self.is_reference_class_type(base_ty)
            && self.class_has_named_field(base_ty, field)
        {
            return Ok({ let receiver = self.local_value(base)?; RustExpr::postfix_on(&receiver, format!("{receiver}.0.borrow().{}.clone()", self.context.names().field_text(self.mir, field))) }.ending_in_clone());
        }
        let field_text = { let receiver = self.local_value(base)?; RustExpr::postfix_on(&receiver, format!("{receiver}.{}", self.context.names().field_text(self.mir, field))) };
        // An accessor field runs its getter on every read; see
        // `crate::accessor_prelude`.
        if crate::accessor_prelude::is_accessor_field_of_type(self.mir, base_ty, field) {
            return Ok(crate::accessor_prelude::read_expr(&field_text));
        }
        Ok(field_text)
    }

    /// An index read.
    fn index_place_expr(
        &self,
        base: LocalId,
        index: &Operand,
        negative: NegativeIndex,
    ) -> Result<RustExpr, EmitError> {
        let base_ty = self.local_decl(base)?.ty;
        if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
            && self.is_match_class_symbol(*name)?
        {
            return self.match_index_expr(&self.local_value(base)?, index);
        }
        // An indexed read on a typed-array view decodes ONE element at
        // the view's own width and signedness. A negative or fractional
        // index is a named property in JavaScript, not an element, and
        // `get` answers `None` for it — the same "no such element"
        // answer an out-of-range read gets.
        if let Some(text) = self.typed_array_index_read_expr(base_ty, base, index)? {
            return Ok(text);
        }
        // A generated union of tuples/lists reads each arm at its own
        // type instead of erasing (see `concrete_union_index_reads`).
        if let Some(text) = self.concrete_union_index_read_expr(base, index, negative)? {
            return Ok(text);
        }
        match self.mir.types.get(base_ty) {
            Some(Type::List(item_ty)) => {
                // A list index READ only ever calls `.get(..).cloned()`
                // and `.len()` — both take `&self`, so the receiver must
                // borrow the backing `Vec` immutably. Using the mutable
                // form here (`borrow_mut()`) is not just unnecessary: when
                // `base` is a shared closure capture (`Rc<RefCell<Vec<_>>>`)
                // the `.len()` inside the normalized-index argument expands
                // to a SECOND `borrow_mut()` of the same cell, so the single
                // `arr.get({ ... arr.len() ... })` expression holds two
                // simultaneous mutable borrows and panics at runtime with
                // "already borrowed". Two simultaneous shared `borrow()`s
                // are allowed, matching the sibling string/optional-list
                // read arms below. The same argument is why the list's own
                // shared buffer is read through `list_read_expr`: the
                // `.len()` in the index argument takes a second borrow of
                // that cell while this one is live.
                let base_text = self.local_value(base)?;
                let index_text =
                    self.normalized_read_index_expr(&RustExpr::postfix_on(&base_text, format!("{base_text}.len()")), index, negative)?;
                let missing = self.element_missing_value(*item_ty)?;
                let read_expr = list_read_expr(&base_text);
                // `unwrap_or` takes its argument BY VALUE, so the
                // out-of-range value is constructed on every read and then
                // dropped unused on the overwhelmingly common in-range one.
                // A JavaScript index read is in range almost always, and in
                // a loop this is per element: `sumBy` built and dropped a
                // `Default::default()` ten thousand times per call to
                // answer a question that never came up. `unwrap_or_else`
                // costs nothing when the element is present.
                // `element_missing_value` is a pure value expression,
                // so moving it into a closure cannot change what it means.
                Ok(RustExpr::postfix_on(&read_expr, format!(
                    "{read_expr}.get({index_text}).cloned().unwrap_or_else(|| {missing})"
                )))
            }
            Some(Type::Optional(inner_ty))
                if matches!(self.mir.types.get(*inner_ty), Some(Type::List(_))) =>
            {
                let Some(Type::List(item_ty)) = self.mir.types.get(*inner_ty) else {
                    return Ok(self.null_value());
                };
                let base_text = self.local_value(base)?;
                let index_text = self.normalized_read_index_expr(
                    &RustExpr::postfix_on(&base_text, format!("{base_text}.as_ref().map_or(0, SmeltList::len)")),
                    index,
                    negative,
                )?;
                let access =
                    if matches!(self.mir.types.get(*item_ty), Some(Type::Optional(_))) {
                        "values.borrow().get({index_text}).cloned().flatten()"
                    } else {
                        "values.borrow().get({index_text}).cloned()"
                    };
                Ok(RustExpr::postfix_on(&base_text, format!(
                    "{base_text}.as_ref().and_then(|values| {})",
                    access.replace("{index_text}", &index_text)
                )))
            }
            Some(Type::Dict(key_ty, value_ty)) => {
                let base_text = self.local_value(base)?;
                self.dict_index_read_expr(&base_text, *key_ty, *value_ty, index)
            }
            Some(Type::String | Type::PyStr) => {
                let base_text = self.local_value(base)?;
                let index_text = self.normalized_read_index_expr(
                    &self.string_len_expr(&base_text),
                    index,
                    negative,
                )?;
                // A string index read is TOTAL, like the list read
                // above: `''[0]` is `undefined`, never a throw, so an
                // out-of-range index answers the element type's missing
                // value (`""`) instead of panicking. A caller that must
                // see the miss (`s[i] === undefined`, `s[i] ?? f`, an
                // optional slot) reads through
                // `optional_element_read_expr` instead.
                let char_at = self.string_char_at_text(&base_text.parenthesized_if_needed(), index_text.as_str());
                Ok(RustExpr::postfix_on(&RustExpr::chain(char_at.clone()), format!("{char_at}.unwrap_or_default()")))
            }
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => {
                // A concrete generated union is not a `SmeltUnknown`, so
                // the runtime-narrowing `match` below cannot scrutinise
                // it directly: it goes through the union's
                // `IntoSmeltUnknown` boundary adapter first, exactly as
                // the INDEX operand of the same read already does.
                // Without it the arms read `expected SmeltUnion127,
                // found SmeltUnknown` (6 of the router slice's errors).
                let base_text = self.erase_concrete_union_expr(
                    &self.local_value(base)?,
                    base_ty,
                );
                self.unknown_index_expr(&base_text, index)
            }
            // A narrowed optional union or `unknown` holds the value
            // the narrowing proved, whichever arm that is.
            Some(Type::Optional(_)) if self.is_optional_of_dynamic_shape(base_ty) => {
                let erased = self.erase_rendered(&self.local_value(base)?, base_ty)?;
                self.unknown_index_expr(&erased, index)
            }
            Some(Type::Tuple(items)) => {
                let tuple_index = self.tuple_index(index, items.len())?;
                Ok(RustExpr::tuple_field(&self.local_value(base)?, tuple_index))
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
                let base_text = self.local_value(base)?;
                let store_text =
                    RustExpr::postfix_on(&base_text, format!("{base_text}.{}", smelt_hir::CLASS_INDEX_STORE_FIELD));
                let optional_read =
                    self.dict_index_optional_read_expr(&store_text, key_ty, value_ty, index)?;
                // An optional store value has already been flattened by
                // the read, so the result IS the value type — a missing
                // key and a stored `undefined` are the same `None`. Its
                // default is that `None`, so defaulting again would try
                // to `unwrap_or` an `Option<inner>` with an `Option`.
                if matches!(self.mir.types.get(value_ty), Some(Type::Optional(_))) {
                    return Ok(optional_read);
                }
                let default_value = self.default_value(value_ty)?;
                Ok(RustExpr::postfix_on(&optional_read, format!("{optional_read}.unwrap_or({default_value})")))
            }
            // A class receiver read with a RUN-TIME key. The key can
            // name any declared member (a field, a method, a host
            // body reader), so it is looked up on the value's erased
            // view, which carries the fields and prototype members of
            // a program class and resolves a host class's modeled
            // members (`smelt_host_method`). Answering `undefined`
            // unconditionally made `raw[key]()` (Hono's request body
            // cache) call nothing.
            Some(Type::Class { .. }) if self.mir.types.get(self.operand_ty(index)?).is_some_and(Type::is_text)
                && self.context.demand().has(Need::Unknown) =>
            {
                let base_text = self.erase(&Operand::Copy(Place::Local(base)))?;
                let index_ty = self.operand_ty(index)?;
                let key_text =
                    self.property_key_to_string_expr(&self.operand_expr(index)?, index_ty)?;
                Ok(RustExpr::chain(format!("smelt_get_unknown_field(&{base_text}, &{key_text})")))
            }
            // No modelled keyed storage on this receiver, so the key
            // is not a property of it: JavaScript answers `undefined`
            // for a missing property, which is a DIFFERENT value from
            // `null` under `===` (see `absent_value`).
            _ => Ok(self.absent_value()),
        }
    }
}
