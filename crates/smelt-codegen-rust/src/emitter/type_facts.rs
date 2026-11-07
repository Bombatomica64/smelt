//! Yes/no questions about a MIR type or class that decide a Rust representation:
//! container backing, reference-class storage, host class kinds, cloneability.
use super::*;

impl FunctionEmitter<'_> {
    /// Returns the value type produced by awaiting `future`.
    ///
    /// An awaited operand's static type is a `Future<Item>`; the awaited value
    /// is that `Item`. When the operand's type is not spelled as a future (some
    /// promise-handle values flow through erased positions), fall back to the
    /// operand type itself so callers can still drive a coercion against it.
    pub(super) fn awaited_output_ty(&self, future: &Operand) -> Result<TypeId, EmitError> {
        let ty = self.operand_ty(future)?;
        match self.mir.types.get(ty) {
            Some(Type::Future(item)) => Ok(*item),
            _ => Ok(ty),
        }
    }

    /// Returns whether a type contains values that cannot be cloned by the
    /// generated Rust representation.
    pub(super) fn type_contains_noncloneable(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Future(_)) => true,
            Some(
                container @ (Type::List(_)
                | Type::Set(_)
                | Type::Optional(_)
                | Type::Dict(_, _)
                | Type::Tuple(_)),
            ) => container.children().any(|child| self.type_contains_noncloneable(child)),
            // A union's cloneability is its RUST representation's, not its
            // arms'. A union that erases stores a `SmeltUnknown`, and a union
            // with concrete storage stores a generated `SmeltUnion…` that
            // derives `Clone`; either way, reading one out of a place yields an
            // owned value and the read must clone.
            //
            // Recursing into the arms claimed otherwise as soon as ONE arm was a
            // future: `str: string | Promise<string> | HtmlEscapedString` is a
            // plain `SmeltUnknown` parameter in the generated Rust, but every
            // read of it was emitted without `.clone()`, so the first read moved
            // out of the parameter and every later one was `use of moved value`
            // (5 × E0382 in Hono's `html.rs`). Erasing a value to inspect it is
            // a read, not a move — the same rule the concrete-union arm of
            // `coercion::erase` already states.
            _ => false,
        }
    }

    /// Returns whether a type contains an erased `SmeltUnknown` value.
    pub(super) fn type_contains_unknown(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => true,
            // A callable and a nominal type carry their own storage.
            Some(Type::Function(_) | Type::Class { .. }) | None => false,
            Some(other) => other.children().any(|child| self.type_contains_unknown(child)),
        }
    }

    /// Returns whether a Rust `HashSet` is a correct backing for a set of this
    /// element type.
    ///
    /// `HashSet` is used only for value-equality primitives (`bool`, `i64`,
    /// `String`) where Rust `Eq + Hash` both exists and matches JavaScript
    /// SameValueZero. Every other element type — `f64` (no `Eq`), generated
    /// unions and generic type parameters (no `Eq + Hash` bound), and
    /// object-like values whose JS equality is by reference, not structure —
    /// routes through the `SmeltJsSet` runtime container instead, which
    /// projects each element through its `IntoSmeltUnknown` erasure for
    /// SameValueZero membership and preserves insertion order.
    pub(super) fn type_is_hash_set_key_safe(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Bool | Type::Int | Type::String) => true,
            Some(Type::Optional(inner) | Type::Future(inner)) => {
                self.type_is_hash_set_key_safe(*inner)
            }
            Some(Type::Union(items)) => items
                .iter()
                .all(|item| self.type_is_hash_set_key_safe(*item)),
            _ => false,
        }
    }

    /// Names the generated container that backs a source `Set` of `item_ty`:
    /// `SmeltPrimSet` when [`Self::type_is_hash_set_key_safe`] holds, else
    /// `SmeltJsSet`.
    ///
    /// The single owner of that choice. The `Set` type spelling, its default
    /// value, a set literal, and every conversion into a set (`new Set(list)`,
    /// Python `set(list)` / `set(tuple)`) name the container through this, so a
    /// constructed set always has the type its slot declares; a site that
    /// picked on its own once built `SmeltPrimSet::from(..)` into a
    /// `SmeltJsSet<f64>` slot.
    pub(super) fn set_container_name(&self, item_ty: TypeId) -> &'static str {
        if self.type_is_hash_set_key_safe(item_ty) {
            "SmeltPrimSet"
        } else {
            "SmeltJsSet"
        }
    }

    /// Returns whether a dictionary must use JavaScript `Map` key equality.
    ///
    /// Rust `HashMap` is correct for primitive key spaces, but JavaScript Map
    /// compares objects and functions by identity and treats `NaN` as equal to
    /// itself. Dictionaries keyed by erased, generic, or object-like values
    /// therefore use the generated linear `SmeltJsMap` runtime container.
    pub(super) fn dict_uses_js_key_map(&self, key_ty: TypeId) -> bool {
        match self.mir.types.get(key_ty) {
            // A bigint key hashes by value, which is SameValueZero for bigints.
            Some(Type::Bool | Type::Int | Type::FixedInt(_) | Type::BigInt | Type::String) => false,
            Some(Type::Float) => true,
            Some(Type::Optional(inner) | Type::Future(inner)) => self.dict_uses_js_key_map(*inner),
            Some(Type::Union(items)) => items.iter().any(|item| self.dict_uses_js_key_map(*item)),
            Some(
                Type::Unknown
                | Type::TypeParam { .. }
                | Type::Never
                | Type::List(_)
                | Type::Set(_)
                | Type::Dict(_, _)
                | Type::JsMap(_, _)
                | Type::Tuple(_)
                | Type::Function(_)
                | Type::Generator { .. }
                | Type::GeneratorResult { .. }
                | Type::Class { .. },
            )
            | None => true,
            Some(Type::None) => false,
        }
    }

    /// Returns whether a string-keyed dictionary should carry object identity.
    ///
    /// TypeScript object literals lower to records internally. When one of
    /// those records is later boxed as `unknown`, JavaScript observes the
    /// original object identity for Map/Set keys. `SmeltRecord` preserves that
    /// identity while keeping structural equality for assertions.
    pub(super) fn dict_uses_smelt_record(&self, key_ty: TypeId) -> bool {
        self.context.demand().has(Need::Unknown)
            && self.mir.types.get(key_ty) == Some(&Type::String)
    }

    /// Returns whether a Map/dict operation must use the `SmeltRecord` backing.
    ///
    /// Receiver-aware wrapper over [`Self::dict_uses_smelt_record`]. A source
    /// `Map` (`Type::JsMap`) *always* backs onto `SmeltJsMap` — even when
    /// string-keyed — so the `SmeltRecord`-specific emission (notably the
    /// `smelt_is_for_in_record_key` marker filter, which only type-checks over
    /// `SmeltRecord`) must never fire for it. Only a `Type::Dict` receiver can
    /// use the `SmeltRecord` backing, and only under the ordinary key rule.
    pub(super) fn map_op_uses_smelt_record(&self, receiver_ty: TypeId, key_ty: TypeId) -> bool {
        if matches!(self.mir.types.get(receiver_ty), Some(Type::JsMap(_, _))) {
            return false;
        }
        self.dict_uses_smelt_record(key_ty)
    }

    /// Returns whether a Map/dict operation uses the `SmeltJsMap` backing.
    ///
    /// Receiver-aware wrapper over [`Self::dict_uses_js_key_map`]. A source
    /// `Map` (`Type::JsMap`) always backs onto `SmeltJsMap`, so its projections
    /// take the same owned-key/value, symbol-only-filter emission as an
    /// object-keyed dict — regardless of key type. A `Type::Dict` receiver keeps
    /// the ordinary key-driven decision.
    pub(super) fn map_op_uses_js_key_map(&self, receiver_ty: TypeId, key_ty: TypeId) -> bool {
        if matches!(self.mir.types.get(receiver_ty), Some(Type::JsMap(_, _))) {
            return true;
        }
        self.dict_uses_js_key_map(key_ty)
    }

    /// Returns whether two map/dict types lower to different backing containers.
    ///
    /// `Dict` and `JsMap` can share a backing (`Dict` with an object-like key and
    /// any `JsMap` both use `SmeltJsMap`) or differ (a string-keyed `Dict` uses
    /// `SmeltRecord`, a plain-primitive-keyed `Dict` uses `HashMap`, and a
    /// `JsMap` always uses `SmeltJsMap`). A key/value-preserving conversion
    /// (`Object.fromEntries(map)`, an interchangeable `Dict`/`JsMap` assignment)
    /// still needs a real container rebuild when the backings differ, even though
    /// the key and value component types are identical — this reports that case.
    pub(super) fn map_backing_differs(&self, left: &Type, right: &Type) -> bool {
        self.map_backing_tag(left) != self.map_backing_tag(right)
    }

    /// Return a discriminant for a map/dict type's backing container.
    fn map_backing_tag(&self, ty: &Type) -> u8 {
        match ty {
            Type::JsMap(_, _) => 0,
            Type::Dict(key, _) if self.dict_uses_js_key_map(*key) => 0,
            Type::Dict(key, _) if self.dict_uses_smelt_record(*key) => 1,
            Type::Dict(_, _) => 2,
            _ => 3,
        }
    }

    /// Returns whether list membership should use JavaScript SameValueZero.
    ///
    /// `Array.prototype.includes`, `indexOf`, and `splice`-style removals do
    /// not use structural object equality. They compare objects/functions by
    /// reference and treat `NaN` as equal to itself.
    pub(super) fn list_item_uses_same_value_zero(&self, item_ty: TypeId) -> bool {
        match self.mir.types.get(item_ty) {
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Float) => true,
            Some(Type::Class { .. }) => self.is_erased_class_type(item_ty),
            _ => false,
        }
    }

    /// Return the key/value types of a class's index-signature store field.
    ///
    /// A class declaring `[key: string]: T` (issue #84) carries a synthesized
    /// private `Dict` field (named [`smelt_hir::CLASS_INDEX_STORE_FIELD`]) that
    /// backs dynamic keyed reads/writes at runtime. When `ty` is such a class,
    /// this returns the store `Dict`'s `(key_ty, value_ty)` so keyed access can
    /// be routed to `base.__smelt_index_store` instead of erased to a stub.
    /// Named fields are unaffected; only genuinely dynamic keyed access uses it.
    pub(super) fn class_index_store_types(&self, ty: TypeId) -> Option<(TypeId, TypeId)> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return None;
        };
        let class = crate::classes::find_class(self.mir, *name)?;
        let store_field = crate::classes::effective_class_fields(self.mir, class)
            .into_iter()
            .find(|field| {
                self.symbol_source_name(field.name)
                    .is_ok_and(|source| source == smelt_hir::CLASS_INDEX_STORE_FIELD)
            })?;
        match self.mir.types.get(store_field.ty) {
            Some(Type::Dict(key_ty, value_ty)) => Some((*key_ty, *value_ty)),
            _ => None,
        }
    }

    /// Return whether a class type declares a named struct field.
    ///
    /// Used to distinguish a declared field access (`x.size`, concrete struct
    /// field) from an undeclared member access that must route to the class
    /// index-signature store (`x.dynamicName`). The synthesized store field
    /// itself is not treated as a named member here.
    pub(super) fn class_has_named_field(&self, ty: TypeId, field: Symbol) -> bool {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return false;
        };
        // A shape (interface / synthetic object-literal shape) is a record type
        // in exactly the same sense a class is, so its declared fields answer
        // here too; the reference-record field paths in `place` rely on it.
        let Some(fields) = crate::classes::find_class(self.mir, *name)
            .map(|class| crate::classes::effective_class_fields(self.mir, class))
            .or_else(|| {
                self.mir
                    .interfaces
                    .iter()
                    .find(|interface| interface.name == *name)
                    .map(|interface| {
                        crate::classes::effective_interface_fields(self.mir, interface)
                    })
            })
        else {
            return false;
        };
        fields
            .iter()
            .any(|candidate| {
                candidate.name == field
                    && self
                        .symbol_source_name(candidate.name)
                        .is_ok_and(|source| source != smelt_hir::CLASS_INDEX_STORE_FIELD)
            })
    }

    /// Returns whether a type is a callable-interface struct.
    ///
    /// A callable interface (a TypeScript interface with one or more call
    /// signatures) lowers to a class record carrying a synthetic `__smelt_call`
    /// storage field for its underlying callable (see the frontend
    /// `add_interface_call_signature_field`). Detecting that field is how the
    /// emitter recognizes a value that is invoked like a function — for direct
    /// calls and for `.apply`/`.call`/`.bind`, which must operate on the erased
    /// callable rather than on a (non-existent) named struct field.
    pub(super) fn callable_interface_call_field_ty(&self, ty: TypeId) -> Option<TypeId> {
        // Interfaces live in `mir.interfaces` and classes in `mir.classes`;
        // `structural_record_fields` resolves the effective field list for
        // either, so the `__smelt_call` probe works for both record shapes. The
        // returned type is the (function-typed) storage slot for the underlying
        // callable, which the caller erases to invoke the value.
        let fields = self.structural_record_fields(ty)?;
        fields.iter().find_map(|candidate| {
            self.symbol_source_name(candidate.name)
                .ok()
                .filter(|source| *source == SMELT_CALL)
                .map(|_| candidate.ty)
        })
    }

    /// Returns whether a place is a declared-field read on a reference class.
    ///
    /// Such a read is lowered as `base.0.borrow().field.clone()`, which is
    /// already an owned value, so callers must not wrap it in another `.clone()`.
    pub(super) fn place_is_reference_class_field(&self, place: &Place) -> bool {
        let Place::Field { base, field } = place else {
            return false;
        };
        let Ok(base_ty) = self.local_decl(*base).map(|decl| decl.ty) else {
            return false;
        };
        self.is_reference_class_type(base_ty) && self.class_has_named_field(base_ty, *field)
    }

    /// Returns whether reading `operand` holds a `RefCell` borrow guard.
    ///
    /// Two shapes read through a cell: a declared field of a reference class
    /// (`recv.0.borrow().f.clone()`) and a shared closure capture
    /// (`(*smelt_capture_x.borrow())`). Both guards live to the end of the
    /// enclosing statement, so a caller that would run arbitrary code in that
    /// same statement — invoking the value it just read — must bind the read to
    /// a local first.
    pub(super) fn operand_reads_through_ref_cell(&self, operand: &Operand) -> bool {
        let place = match operand {
            Operand::Copy(place) | Operand::Move(place) => place,
            Operand::Const(_) => return false,
        };
        self.place_is_reference_class_field(place) || self.place_reads_through_shared_capture(place)
    }

    /// Returns whether an index read already produces an owned value.
    ///
    /// `place_expr`'s `Place::Index` arm lowers almost every receiver shape to an
    /// expression that CONSTRUCTS its result rather than projecting to a place:
    /// a list reads `.get(i).cloned().unwrap_or(missing)`, a dict
    /// `.get(&key).cloned().unwrap_or(default)`, a string
    /// `.chars().nth(i).map(..).expect(..)`, an erased receiver a `match` whose
    /// every arm builds a fresh `SmeltUnknown`, and a class index store an
    /// `.unwrap_or(default)`. Each of those is already owned, so the `.clone()`
    /// `operand_expr` appends to a `Copy` read is a SECOND whole-value copy of
    /// something nobody else holds — the same redundancy
    /// [`Self::place_is_reference_class_field`] exists to suppress for a
    /// reference-class field.
    ///
    /// Inside a loop that is not a constant: `result[key].push(item)` lowered to
    /// `result.get(&key).cloned().unwrap_or(..).clone()`, deep-copying the whole
    /// group twice per pushed element, so grouping n items copied O(n^2)
    /// elements. `items[i]` paid the same doubling on every element read.
    ///
    /// Answers `false` for the shapes that genuinely project to a place and so
    /// still need the clone to own their result:
    ///
    /// * `Type::Tuple`, which renders as the field projection `base.0`;
    /// * a match-class receiver, whose `match_index_expr` shape this predicate
    ///   deliberately does not model.
    ///
    /// The arms here mirror `place_expr`'s; `false` is always the safe answer, so
    /// a shape added there and missed here keeps today's redundant clone rather
    /// than producing a borrow where an owned value was required.
    pub(super) fn index_place_read_is_owned(&self, place: &Place) -> bool {
        let Place::Index { base, .. } = place else {
            return false;
        };
        let Ok(base_ty) = self.local_decl(*base).map(|decl| decl.ty) else {
            return false;
        };
        // A match-class receiver takes the `match_index_expr` path before the
        // type dispatch below, so it is excluded first.
        if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
            && self.is_match_class_symbol(*name).unwrap_or(false)
        {
            return false;
        }
        match self.mir.types.get(base_ty) {
            Some(Type::List(_) | Type::Dict(_, _) | Type::String) => true,
            Some(Type::Optional(inner)) => {
                matches!(self.mir.types.get(*inner), Some(Type::List(_)))
            }
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => true,
            Some(Type::Class { .. }) => self.class_index_store_types(base_ty).is_some(),
            _ => false,
        }
    }

    /// Returns whether a type is a reference class (handle newtype).
    ///
    /// Reference classes carry shared mutable identity through
    /// `Rc<RefCell<Inner>>`; field access goes through the cell and methods take
    /// `&self` uniformly. Non-class types and value classes answer `false`.
    pub(super) fn is_reference_class_type(&self, ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(ty),
            Some(Type::Class { name, .. }) if self.context.is_reference_class(*name)
        )
    }

    /// Returns whether `local` is the receiver of a reference-class method that
    /// is captured into an escaping closure.
    ///
    /// A reference-class `self` is already a shareable handle (`Rc<RefCell<
    /// Inner>>`), so an escaping closure captures a plain `self.clone()` rather
    /// than wrapping the receiver in a second `Rc<RefCell<_>>`. When this holds,
    /// the receiver is bound once as `let smelt_capture_self = self.clone();` and
    /// every reference renders as that handle (`smelt_capture_self`), with field
    /// access going through its cell (`smelt_capture_self.0.borrow()`). This is
    /// the escaping-`this` mechanism that clears the E0425 `smelt_capture_self`
    /// cluster.
    pub(super) fn is_reference_self_shared_capture(&self, local: LocalId) -> bool {
        self.method_owner_is_reference_class()
            && matches!(self.function.origin, HirOrigin::ClassMethod { .. })
            && self.function.params.first() == Some(&local)
            && self.local_uses_shared_capture_storage(local)
    }

    /// Returns whether the method currently being emitted belongs to a reference
    /// class, so its receiver is `&self` and its field access goes through the
    /// shared cell.
    pub(super) fn method_owner_is_reference_class(&self) -> bool {
        match self.function.origin {
            HirOrigin::ClassConstructor { class, .. }
            | HirOrigin::ClassMethod { class, .. }
            | HirOrigin::ClassStaticMethod { class, .. } => self.context.is_reference_class(class),
            HirOrigin::Body(_) => false,
        }
    }

    /// Returns whether a class symbol names the stdlib `RegExp` class.
    ///
    /// Several emitters dispatch RegExp-shaped class types to the regex
    /// runtime shim; the identity lookup lives in the shared `smelt-stdlib`
    /// registry instead of inline name comparisons.
    pub(super) fn is_regexp_class_symbol(&self, name: Symbol) -> Result<bool, EmitError> {
        Ok(
            smelt_stdlib::typescript_stdlib_class(self.symbol_name(name)?)
                == Some(smelt_stdlib::StdlibClass::RegExp),
        )
    }

    /// Returns the synthetic match-result class identity named by a symbol, if any.
    ///
    /// Both `__SmeltMatch` (the match value) and `__SmeltMatchGroups` (its
    /// named-group accessor) are backed by the generated concrete `SmeltMatch`
    /// Rust type. The two share a Rust representation but differ in how field and
    /// index reads are lowered, so callers inspect the returned identity to pick
    /// the right accessor.
    pub(super) fn match_class_kind(
        &self,
        name: Symbol,
    ) -> Result<Option<smelt_stdlib::StdlibClass>, EmitError> {
        Ok(
            match smelt_stdlib::typescript_stdlib_class(self.symbol_name(name)?) {
                class @ Some(
                    smelt_stdlib::StdlibClass::Match | smelt_stdlib::StdlibClass::MatchGroups,
                ) => class,
                _ => None,
            },
        )
    }

    /// Returns whether a class symbol names a synthetic RegExp match-result class.
    pub(super) fn is_match_class_symbol(&self, name: Symbol) -> Result<bool, EmitError> {
        Ok(self.match_class_kind(name)?.is_some())
    }

    /// Whether `ty` has no fixed runtime shape at the MIR level: `unknown`, a
    /// type parameter, a union, or a class with no concrete generated Rust type
    /// ([`Self::is_erased_class_type`]).
    ///
    /// This is the static test the emitter runs before choosing a runtime-tag
    /// path. It deliberately does not answer the scope-aware question of
    /// whether a type parameter or a concrete union renders as `SmeltUnknown`
    /// at a given position; see `list_items_render_as_unknown` and
    /// `concrete_union_members` for those.
    pub(super) fn has_dynamic_shape(&self, ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(ty)
    }

    /// Returns whether a class-shaped type is emitted as `SmeltUnknown`.
    pub(super) fn is_erased_class_type(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Class { name, .. }) => crate::classes::class_name_renders_erased(self.mir, *name),
            _ => false,
        }
    }

    /// Returns whether a target type can absorb an erased `SmeltUnknown::Function`.
    ///
    /// A function argument flowing into one of these targets should be erased
    /// via `value_at_type` (which yields `Some(SmeltUnknown::Function(..))` for
    /// optionals) rather than dropped to a default. Covers the dynamic surfaces
    /// (`Unknown`, type parameters, unions, erased classes), `Function` itself,
    /// and a single `Optional` layer wrapping any of those — e.g. purry
    /// data-last params typed `arg?: Data | Callback`, where the predicate must
    /// survive as a value so the runtime dispatcher can branch on `typeof`.
    pub(super) fn type_accepts_erased_function(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Function(_) | Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => {
                true
            }
            Some(Type::Optional(inner)) => self.type_accepts_erased_function(*inner),
            _ => self.is_erased_class_type(ty),
        }
    }

    /// Returns whether `default_value` is a concrete literal/container value.
    ///
    /// This excludes classes and callable/composite fallback cases where
    /// `default_value` currently emits `Default::default()` and the generated
    /// Rust type may not implement `Default`.
    pub(super) fn has_plain_default_value(&self, ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(ty),
            Some(
                Type::Bool
                    | Type::Int
                    | Type::Float
                    | Type::String
                    | Type::Unknown
                    | Type::Never
                    | Type::None
                    | Type::List(_)
                    | Type::Set(_)
                    | Type::Dict(_, _)
                    | Type::Optional(_)
            )
        )
    }
}
