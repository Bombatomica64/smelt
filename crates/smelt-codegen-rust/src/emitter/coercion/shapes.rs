//! Crossing shapes: the questions the coercion seam asks before it emits.
//!
//! Whether a value of one type can be rendered at another (a dictionary value
//! into a record field, a callback into a differently-shaped function slot, a
//! value into an optional's inner type), and the field or element shape an
//! adapter will walk (a class's storage fields, a function's trailing rest
//! element). Pure predicates over MIR types: nothing here renders Rust text or
//! re-enters a coercion verb, so every adapter can ask them without depending
//! on another adapter.

use super::*;
use crate::generic_bindings::substitute_existing_type;

impl FunctionEmitter<'_> {
    /// Returns whether `source` can be coerced before wrapping into `Option`.
    pub(super) fn can_coerce_to_optional_inner(&self, source: TypeId, inner: TypeId) -> bool {
        self.has_dynamic_shape(inner)
            || self.structural_record_adapter_available(source, inner)
            || self.string_dict_record_adapter_available(source, inner)
            || matches!(
                (self.mir.types.get(source), self.mir.types.get(inner)),
                (Some(Type::Int), Some(Type::Float))
                    | (Some(Type::Float), Some(Type::Int))
                    | (
                        Some(Type::Int | Type::Float | Type::Bool),
                        Some(Type::PyInt)
                    )
                    | (Some(Type::PyInt), Some(Type::Float | Type::Int))
                    | (Some(Type::List(_)), Some(Type::Tuple(_)))
                    | (Some(Type::List(_)), Some(Type::List(_)))
                    | (Some(Type::Dict(_, _)), Some(Type::Dict(_, _)))
                    | (Some(Type::Function(_)), Some(Type::Function(_)))
            )
    }

    /// Returns true when a string-keyed dictionary can fill a structural record.
    fn string_dict_record_adapter_available(&self, source: TypeId, target: TypeId) -> bool {
        let Some(Type::Dict(source_key, _)) = self.mir.types.get(source) else {
            return false;
        };
        self.mir.types.get(*source_key).is_some_and(Type::is_text)
            && self.is_structural_record_adapter_target(target)
            && self
                .structural_record_fields(target)
                .is_some_and(|fields| !fields.is_empty())
    }

    /// Returns whether an erased object can be safely expanded into a record.
    ///
    /// Expansion depth is guarded while rendering so callback-bearing records
    /// may retain nested option bags without infinitely expanding cyclic shapes.
    pub(super) fn can_extract_unknown_object_record(&self, target: TypeId) -> bool {
        self.structural_record_fields(target)
            .is_some_and(|fields| !fields.is_empty())
    }

    /// Returns matching source/target fields for a structural record adapter.
    pub(super) fn structural_record_adapter_fields(
        &self,
        source: TypeId,
        target: TypeId,
    ) -> Option<Vec<(Option<MirField>, MirField)>> {
        if source == target || !self.is_structural_record_adapter_target(target) {
            return None;
        }
        // A field the source class redeclares is stored, and so read here, at
        // its declaring level's type (`redeclared_field`).
        let source_fields: Vec<MirField> = self
            .structural_record_fields(source)?
            .into_iter()
            .map(|mut field| {
                if let Some(stored) = self.redeclared_field_storage(source, field.name) {
                    field.ty = stored;
                }
                field
            })
            .collect();
        let target_fields = self.structural_record_fields(target)?;
        if target_fields.is_empty() {
            // A field-less base class is still a distinct struct, and an
            // instance of a class extending it converts to it (`function
            // f(): Model { return new User(); }` with neither declaring a
            // field): nothing to pair, but the value must be rebuilt as the
            // base or the result is `expected Model, found User`.
            let subclass_of_target = matches!(
                (self.mir.types.get(source), self.mir.types.get(target)),
                (Some(Type::Class { name: from, .. }), Some(Type::Class { name: to, .. }))
                    if crate::classes::class_chain_contains(self.mir, *from, *to)
            );
            return subclass_of_target.then(Vec::new);
        }
        let field_key =
            |field: &MirField| Some(self.context.names().field_text(self.mir, field.name));
        // A DOWNCAST: the target declares every source field and adds more
        // (`base as Derived` where `interface Derived extends Base`). `tsc`
        // only accepts an assertion between comparable types, and a target that
        // is not assignable from the source must then be assignable TO it, so
        // this shape reaches codegen only through an assertion. At runtime the
        // added properties are absent until the source assigns them
        // (`handlerSet.params = {}` right after the cast); the adapter fills
        // them with their default value, the non-optional spelling of absent.
        let is_downcast = !source_fields.is_empty()
            && source_fields.iter().all(|source_field| {
                target_fields
                    .iter()
                    .any(|target_field| field_key(target_field) == field_key(source_field))
            });
        let mut adapted_fields = Vec::new();
        for target_field in target_fields {
            let source_field = source_fields
                .iter()
                .find(|field| field_key(field) == field_key(&target_field))
                .cloned();
            if source_field.is_none()
                && !is_downcast
                && !self.is_virtual_method_storage_field(target, target_field.name)
                && !self.is_interface_method_slot_bound_by_source(source, target, target_field.name)
                && !matches!(
                    self.mir.types.get(target_field.ty),
                    Some(Type::Optional(..))
                )
            {
                return None;
            }
            adapted_fields.push((source_field, target_field));
        }
        Some(adapted_fields)
    }

    /// Returns whether a nominal Rust storage type may receive field-wise values.
    ///
    /// Interface records always use structural assignment. Class storage is
    /// also adapted when a source value exposes every destination field, which
    /// covers TypeScript base-class references emitted as concrete Rust
    /// structs.
    pub(super) fn is_structural_record_adapter_target(&self, target: TypeId) -> bool {
        matches!(self.mir.types.get(target), Some(Type::Class { .. }))
    }

    /// Whether a string-keyed dictionary's value type can populate a record field.
    ///
    /// The sibling emitter modules use this to ask "would the field-wise adapter
    /// have something to put here?" without emitting the adapter — see
    /// `union::record_member_accepts_source_fields`.
    pub(in crate::emitter) fn dict_value_fits_field(&self, value: TypeId, field: TypeId) -> bool {
        self.can_render_dict_value_as(value, field)
    }

    /// Returns whether a dictionary value can be meaningfully assigned to a field.
    pub(super) fn can_render_dict_value_as(&self, source: TypeId, target: TypeId) -> bool {
        self.can_render_non_function_dict_value_as(source, target)
            || self.can_adapt_rendered_function_value(source, target)
    }

    /// Whether a record whose values are `source` can never hold a value of
    /// the field type `target`, because `target` is a union emitted as a
    /// concrete enum and `source` is none of its arms.
    ///
    /// Projecting `{ method: 'HEAD' }` (`Record<string, string>`) into
    /// `RequestInit` reaches `headers: Record | Headers | [string, string][]`;
    /// a string is not a header list, so the key is absent for every value the
    /// record can hold. An erased or union-typed source is still projected at
    /// run time.
    pub(super) fn record_value_misses_concrete_union(
        &self,
        source: TypeId,
        target: TypeId,
    ) -> bool {
        source != target
            && !matches!(
                self.mir.types.get(source),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            )
            && self
                .concrete_union_members(target)
                .is_some_and(|members| !members.contains(&source))
    }

    /// Returns whether a non-callback dictionary value can populate a field.
    fn can_render_non_function_dict_value_as(&self, source: TypeId, target: TypeId) -> bool {
        if let Some(Type::Optional(inner, _)) = self.mir.types.get(target) {
            // An optional field accepts the inner type (the `get` already supplies
            // the outer `Option`) or an absent value. It must ALSO accept a source
            // that is itself optional-shaped — the checks below reach that through
            // `source == target` — so fall through rather than returning here. The
            // early return dropped every field whose dictionary value carried the
            // same `Option<T>` the field declares: an object literal
            // `{ edges: ['leading'] }` projected into an interface whose `edges` is
            // `Array<...> | undefined` silently became `edges: None`.
            if self.can_render_non_function_dict_value_as(source, *inner)
                || matches!(self.mir.types.get(source), Some(Type::None))
            {
                return true;
            }
        }
        source == target
            || self.structural_record_adapter_available(source, target)
            || self.string_dict_record_adapter_available(source, target)
            || matches!(
                (self.mir.types.get(source), self.mir.types.get(target)),
                (Some(Type::Int), Some(Type::Float))
                    | (Some(Type::Float), Some(Type::Int))
                    // A Bun `int32` field the frontend proved every literal
                    // store into (`lowering::int32`).
                    | (Some(Type::Float), Some(Type::FixedInt(_)))
                    | (
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)),
                        _
                    )
                    | (
                        _,
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    )
            )
            || self.is_erased_class_type(source)
            || self.is_erased_class_type(target)
    }

    /// Returns the generated fields for a concrete class/interface storage type.
    pub(in crate::emitter) fn structural_record_fields(&self, ty: TypeId) -> Option<Vec<MirField>> {
        let Some(Type::Class { name, args }) = self.mir.types.get(ty) else {
            return None;
        };
        // The struct that owns the symbol answers: a class over a same-named
        // interface, which merges into it (see `names::type_claim_order`).
        if let Some(class) = crate::classes::find_class(self.mir, *name) {
            return Some(self.substitute_record_field_type_params(
                &class.type_params,
                args,
                crate::classes::effective_class_fields(self.mir, class),
            ));
        }
        let interface = self
            .mir
            .interfaces
            .iter()
            .find(|interface| interface.name == *name)?;
        let fields = crate::classes::effective_interface_fields(self.mir, interface);
        Some(self.substitute_record_field_type_params(&interface.type_params, args, fields))
    }

    /// Substitute concrete class/interface arguments into structural fields.
    ///
    /// Generated storage structs keep their Rust generic parameters, but
    /// adapter emission usually works with an instantiated type such as
    /// `MatchFnResult<f64>`. Field reads must therefore use the instantiated
    /// payload type instead of the declaration-time type parameter.
    pub(in crate::emitter) fn substitute_record_field_type_params(
        &self,
        type_params: &[smelt_hir::TypeParamDef],
        args: &[TypeId],
        fields: Vec<MirField>,
    ) -> Vec<MirField> {
        // A parameter the reference leaves off takes its declared default, as
        // the type renderer does (see the `Type::Class` arm of `rust_type`),
        // so a field's type matches the struct the reference renders as.
        let substitutions = type_params
            .iter()
            .enumerate()
            .filter_map(|(index, param)| {
                args.get(index)
                    .copied()
                    .or(if args.is_empty() { None } else { param.default })
                    .map(|arg| (param.name, arg))
            })
            .collect::<HashMap<_, _>>();
        if substitutions.is_empty() {
            return fields;
        }
        fields
            .into_iter()
            .map(|mut field| {
                // A union with a type-parameter member has no Rust generic to
                // carry it, so the struct declares the field erased whatever the
                // record is instantiated at: every read and adapter sees that.
                let stored_erased = self.union_member_is_type_param(field.ty);
                field.ty = match self.existing_type_id(Type::Unknown) {
                    Some(unknown) if stored_erased => unknown,
                    _ => substitute_existing_type(self.mir, field.ty, &substitutions),
                };
                field
            })
            .collect()
    }

    /// Whether `ty` is a union one of whose members is a type parameter.
    fn union_member_is_type_param(&self, ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(ty),
            Some(Type::Union(members)) if members.iter().any(|member| {
                matches!(self.mir.types.get(*member), Some(Type::TypeParam { .. }))
            })
        )
    }

    /// Returns whether a generated storage type is an interface-shaped record.
    pub(in crate::emitter) fn is_interface_record_type(&self, ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(ty),
            Some(Type::Class { name, .. })
                if self
                    .mir
                    .interfaces
                    .iter()
                    .any(|interface| interface.name == *name)
        )
    }

    /// Returns true when `source` can be field-wise adapted to `target`.
    ///
    /// TypeScript option bags are structurally compatible, but generated Rust
    /// structs are nominal. This predicate intentionally only enables the
    /// adapter when the destination is an emitted interface record; ordinary
    /// class-to-class conversion still keeps Rust's nominal identity.
    pub(in crate::emitter) fn structural_record_adapter_available(
        &self,
        source: TypeId,
        target: TypeId,
    ) -> bool {
        self.structural_record_adapter_fields(source, target)
            .is_some()
            // A modeled host class carries its members behind accessors rather
            // than as generated fields, so the field-pairing answer above is
            // `None` for one; it is still structurally assignable to a record
            // whose every declared member it exposes. Kept in lock-step with
            // `host_class_to_record_adapter`, which emits that conversion.
            || self.host_class_record_adapter_available(source, target)
    }

    /// Whether every member a target record declares can be read off a host value.
    ///
    /// The availability half of `host_class_to_record_adapter`: an
    /// optional target field the source does not expose is absent, any other
    /// missing member means the conversion does not exist.
    fn host_class_record_adapter_available(&self, source: TypeId, target: TypeId) -> bool {
        if source == target || !self.exposes_host_members(source).unwrap_or(false) {
            return false;
        }
        let Some(target_fields) = self.structural_record_fields(target) else {
            return false;
        };
        if target_fields.is_empty() {
            return false;
        }
        target_fields.iter().all(|field| {
            let Ok(member) = self.symbol_source_name(field.name) else {
                return false;
            };
            match self.host_class_member_read(&RustExpr::path("smelt_host_value"), source, member) {
                Ok(Some(_)) => true,
                Ok(None) => matches!(self.mir.types.get(field.ty), Some(Type::Optional(..))),
                Err(_) => false,
            }
        })
    }

    /// Returns whether a callback map value can be wrapped for a typed field.
    ///
    /// The wrapper may discard arguments, as TypeScript permits, but it cannot
    /// invent a different return value or convert incompatible argument types.
    pub(super) fn can_adapt_rendered_function_value(&self, source: TypeId, target: TypeId) -> bool {
        let (Some(Type::Function(source_function)), Some(Type::Function(target_function))) =
            (self.mir.types.get(source), self.mir.types.get(target))
        else {
            return false;
        };
        // The adapter closure takes the *target* parameters and forwards them to
        // the source callback. The source may legally declare more parameters than
        // the target when the surplus are optional (their values are filled with
        // defaults at the forwarding site); this is what lets a `Promise` `resolve`
        // typed `(value?) => void` flow into a `() => void` slot. We still require
        // every target parameter the source actually consumes to be renderable into
        // the matching source parameter type.
        if source_function.is_async || target_function.is_async {
            return false;
        }
        // An erased-rest source is the `SmeltErasedFunction` callable ABI: the
        // adapter does not forward the target's arguments *into* the source's
        // declared rest-list parameter, it packs them into a
        // `Vec<SmeltUnknown>` and calls `.call(..)` (see the `packed` branch in
        // `rendered_function_shape_adapter`). So the per-parameter
        // renderability test below asks the wrong question — `f64` is not
        // renderable as `unknown[]` yet erases to `SmeltUnknown` perfectly
        // well. What the packed call really needs is a return value that can
        // come back out of `SmeltUnknown`, which every erased slot answers.
        // Without this an overloaded callable interface whose overloads differ
        // only by parameter *type* (`(v: string): string; (v: number): number;`)
        // failed to adapt its erased `__smelt_call` slot to the overload the
        // call site selected, emitting a raw field read and an E0308.
        if self.is_erased_unknown_rest_function(source_function) {
            let unknown_ty = self.mir.types.intern_late(Type::Unknown);
            return self
                .can_render_non_function_dict_value_as(unknown_ty, target_function.return_ty);
        }
        // A trailing rest parameter in the SOURCE collects every target
        // argument from its position on (`(...args) => ..` stored in a
        // fixed-arity slot), so each of those must render as the list's
        // element rather than as the list.
        if let Some((rest, element)) = self.trailing_rest_element(source_function) {
            return self
                .can_render_dict_value_as(source_function.return_ty, target_function.return_ty)
                && target_function
                    .params
                    .iter()
                    .enumerate()
                    .all(|(index, target_param)| {
                        let into = if index < rest {
                            source_function.params[index]
                        } else {
                            element
                        };
                        self.can_render_dict_value_as(*target_param, into)
                    });
        }
        // A parameter or return that is itself a callable converts through
        // this same adapter, parameters contravariantly: `ts-pattern`'s
        // `otherwise: (handler: (value: i) => R) => R` read off a
        // `Match<unknown>` and stored in a `Match<number[]>` wraps `handler`
        // back into `(value: unknown) => R` before calling the stored slot.
        let shared = source_function
            .params
            .len()
            .min(target_function.params.len());
        self.can_render_dict_value_as(source_function.return_ty, target_function.return_ty)
            && source_function
                .params
                .iter()
                .take(shared)
                .zip(target_function.params.iter().take(shared))
                .all(|(source_param, target_param)| {
                    self.can_render_dict_value_as(*target_param, *source_param)
                })
    }

    /// The position and element type of a function's trailing rest parameter,
    /// when it has one whose type is a list.
    pub(super) fn trailing_rest_element(&self, function: &FunctionType) -> Option<(usize, TypeId)> {
        let rest = function.rest?;
        if rest + 1 != function.params.len() {
            return None;
        }
        match self.mir.types.get(*function.params.get(rest)?) {
            Some(Type::List(element)) => Some((rest, *element)),
            _ => None,
        }
    }

    /// Whether `list_ty` is a list whose elements render as `SmeltUnknown` at
    /// `scope`.
    ///
    /// `Type::Unknown` is that outright. A `Type::TypeParam` is too whenever
    /// `scope` does not spell it: `rust_type` resolves such a name to
    /// [`crate::type_substitution::Resolved::Erased`], so `List<T>` is emitted
    /// as `SmeltList<SmeltUnknown>` and the per-element conversion the general
    /// path would run is `into_smelt_unknown()` on a value that already IS a
    /// `SmeltUnknown` — the identity impl. An *in-scope* `T` is a real Rust
    /// generic and must keep going through the general path, which converts each
    /// element through the corresponding inbound or outbound boundary adapter.
    ///
    /// Scope-aware rather than unconditional, and deliberately so: pass the
    /// scope the asked-about type was (or will be) rendered at. A value of the
    /// item being emitted was rendered at `&self.render_scope()`; a coercion
    /// target is rendered at the scope its render position threads, which a
    /// callee argument narrows (`callee_parameter_render_scope`).
    pub(in crate::emitter) fn list_items_render_as_unknown(
        &self,
        list_ty: TypeId,
        scope: &RenderScope,
    ) -> bool {
        let Some(Type::List(item)) = self.mir.types.get(list_ty) else {
            return false;
        };
        match self.mir.types.get(*item) {
            Some(Type::Unknown) => true,
            Some(Type::TypeParam { name }) => !scope.spells(*name),
            _ => false,
        }
    }
}
