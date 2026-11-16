//! Union annotations: flattening members, dropping members, the union-or-unknown
//! join, and the dict/list/byte-source unions annotations name.

use crate::lowering::ModuleBuilder;
use crate::lowering::erasure::NotNewErasure;
use smelt_hir::{ErasureReason, FunctionType, Type};

impl ModuleBuilder<'_> {
    /// Expand a union member into its flat constituent arms.
    ///
    /// A union arm may itself resolve to a `Type::Union` (composed type alias)
    /// or a `Type::Optional` (`inner | undefined`). TypeScript unions are
    /// always flat, and every consumer that walks union arms — typeof
    /// narrowing, nullish narrowing, key/value coercion — assumes flat arms.
    /// Returning the nested arms here keeps that invariant so no arm is hidden
    /// inside another union. Non-union members are returned unchanged.
    pub(in crate::lowering) fn flatten_union_member_types(
        &mut self,
        ty: smelt_hir::TypeId,
    ) -> Vec<smelt_hir::TypeId> {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Union(items)) => {
                let items = items.clone();
                items
                    .into_iter()
                    .flat_map(|item| self.flatten_union_member_types(item))
                    .collect()
            }
            Some(Type::Optional(inner)) => {
                let inner_ty = *inner;
                let none_ty = self.ctx.krate.types.intern(Type::None);
                let mut out = self.flatten_union_member_types(inner_ty);
                if !out.contains(&none_ty) {
                    out.push(none_ty);
                }
                out
            }
            _ => vec![ty],
        }
    }

    /// The union of already-lowered member types, as a union annotation
    /// spelling those members lowers.
    ///
    /// The one statement of how members join, shared by the annotation
    /// lowering (`A | B`) and by every place that builds a union from types
    /// it computed (an indexed access evaluated to several property types,
    /// `ty/deferred_index.rs`): nested unions and optionals are flattened,
    /// `never` arms vanish, the nullish arms collapse into one `Optional`
    /// around a single remaining arm, lists and dictionaries with one key
    /// type merge, and function arms whose call shapes disagree become one
    /// variadic erased signature.
    ///
    /// The members carry no source spelling here, so a bare absent member
    /// counts as `undefined`: the absent value is `null` only when every
    /// member holding one is itself a `T | null`
    /// (`TypeInterner::absent_is_null`).
    pub(in crate::lowering) fn union_of_member_types(
        &mut self,
        members: &[smelt_hir::TypeId],
    ) -> smelt_hir::TypeId {
        let absent_only_null = members.iter().all(|member_ty| {
            self.ctx.krate.types.absent_is_null(*member_ty)
                || !self
                    .flatten_union_member_types(*member_ty)
                    .iter()
                    .any(|flat| matches!(self.ctx.krate.types.get(*flat), Some(Type::None)))
        });
        self.union_of_member_types_with_absence(members, absent_only_null)
    }

    /// [`Self::union_of_member_types`] with the caller's knowledge of the
    /// absent arms: `absent_only_null` says every absent arm is `null` (the
    /// keyword, or a member that is itself `T | null`), so the resulting
    /// optional's absent value is `null`, not `undefined`
    /// (`TypeInterner::intern_null_optional`).
    pub(in crate::lowering) fn union_of_member_types_with_absence(
        &mut self,
        members: &[smelt_hir::TypeId],
        absent_only_null: bool,
    ) -> smelt_hir::TypeId {
        let mut lowered = Vec::new();
        let mut nullish = Vec::new();
        let mut saw_never = false;
        for member_ty in members {
            // Flatten members that are themselves unions/optionals so the
            // resulting union has only concrete top-level arms. Type
            // aliases such as `type Criterion = A | B | null` lower to a
            // `Type::Union`, and composing them (`Criterion | C`) would
            // otherwise nest a union inside a union. Downstream typeof
            // narrowing, nullish narrowing, and coercion all inspect only
            // the top-level arms, so a nested union arm is treated as one
            // atomic member and gets dropped or mishandled wholesale.
            for flat_ty in self.flatten_union_member_types(*member_ty) {
                if matches!(self.ctx.krate.types.get(flat_ty), Some(Type::Never)) {
                    saw_never = true;
                } else if matches!(self.ctx.krate.types.get(flat_ty), Some(Type::None)) {
                    if !nullish.contains(&flat_ty) {
                        nullish.push(flat_ty);
                    }
                } else if !lowered.contains(&flat_ty) {
                    lowered.push(flat_ty);
                }
            }
        }
        // A union whose arms are all function types but whose call
        // shapes disagree (different parameter counts, or a rest arm
        // mixed with a fixed-arity arm) has no single call signature to
        // select: TypeScript would require a caller to satisfy every
        // arm. Picking one arm inconsistently at different points (the
        // `func` parameter type, the packed-argument wrapper, the
        // `SmeltUnknown::Function` adapter) is exactly what produces
        // mismatched arities in generated Rust (E0057 — e.g. es-toolkit
        // `once<F extends (() => any) | ((...args: any[]) => void)>`).
        //
        // The honest JavaScript semantics of *calling* such a value is a
        // variadic call: arguments are passed positionally and the result
        // is observed dynamically. Collapse the arms into one variadic
        // erased-rest signature (`(...args: unknown[]) => unknown`) so
        // every consumer agrees on arity. Function unions that already
        // share one call shape are left untouched so their concrete
        // parameter/return types survive.
        if lowered.len() >= 2
            && lowered
                .iter()
                .all(|arm_ty| matches!(self.ctx.krate.types.get(*arm_ty), Some(Type::Function(_))))
        {
            let mut shapes = lowered.iter().filter_map(|arm_ty| {
                if let Some(Type::Function(signature)) = self.ctx.krate.types.get(*arm_ty) {
                    Some((signature.params.len(), signature.rest.is_some()))
                } else {
                    None
                }
            });
            let first_shape = shapes.next();
            let arities_differ = shapes.any(|shape| Some(shape) != first_shape);
            if arities_differ {
                let item_ty = self.erase(ErasureReason::TypeLevel);
                let rest_list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                let return_ty = self.unknown_type(NotNewErasure::ErasedUpstream); // not a new erasure: same mixed-arity decision recorded for the rest item above
                let unified = self.ctx.krate.types.intern(Type::Function(FunctionType {
                    params: vec![rest_list_ty],
                    rest: Some(0),
                    required_params: Some(0),
                    mutable_params: Vec::new(),
                    return_ty,
                    is_async: false,
                    may_throw: false,
                }));
                if nullish.is_empty() {
                    return unified;
                }
                lowered = vec![unified];
            }
        }
        if let Some(list_ty) = self.list_union_type(&lowered) {
            if nullish.is_empty() {
                return list_ty;
            }
            lowered = vec![list_ty];
        }
        if let Some(dict_ty) = self.dict_union_type(&lowered) {
            if nullish.is_empty() {
                return dict_ty;
            }
            lowered = vec![dict_ty];
        }
        if lowered.len() == 1 && !nullish.is_empty() {
            let single = lowered.remove(0);
            if absent_only_null {
                return self.ctx.krate.types.intern_null_optional(single);
            }
            self.ctx.krate.types.intern(Type::Optional(single))
        } else if lowered.len() == 1 {
            lowered.remove(0)
        } else if lowered.is_empty() && nullish.is_empty() && saw_never {
            self.ctx.krate.types.intern(Type::Never)
        } else if nullish.is_empty()
            || lowered.len() < 2
            || !lowered
                .iter()
                .all(|arm| self.union_arm_has_concrete_storage(*arm))
        {
            // Fewer than two present arms leave no union to make
            // optional, and a union with an arm the generated crate
            // stores erased (`unknown`, a type parameter, a class
            // nothing declares) is the erased value, which already
            // holds `null` and `undefined`: the union stays as written.
            lowered.extend(nullish);
            self.ctx.krate.types.intern(Type::Union(lowered))
        } else {
            // Two or more present arms and an absent value: the optional
            // of the present union, as a single present arm already is
            // (and as an optional parameter `x?: A | B` is). When every
            // absent arm is `null`, the optional is a `T | null`
            // (`TypeInterner::intern_null_optional`): its `None` erases
            // and prints as `null`, inside a container too
            // (`emitter::absent_value` in the Rust backend).
            let present = self.ctx.krate.types.intern(Type::Union(lowered));
            if absent_only_null {
                return self.ctx.krate.types.intern_null_optional(present);
            }
            self.ctx.krate.types.intern(Type::Optional(present))
        }
    }

    /// Build a union type from unique item types, falling back to `unknown` for an empty set.
    pub(in crate::lowering) fn union_of_types_or_unknown(
        &mut self,
        items: Vec<smelt_hir::TypeId>,
    ) -> smelt_hir::TypeId {
        // not a new erasure: an erased item was recorded where it was decided
        let unknown = self.unknown_type(NotNewErasure::ErasedUpstream);
        if items.contains(&unknown) {
            return unknown;
        }
        match items.as_slice() {
            [] => self.erase_with(ErasureReason::TypeLevel, "empty item set"),
            [single] => *single,
            _ => self.ctx.krate.types.intern(Type::Union(items)),
        }
    }

    /// `Exclude<A, B>`: the arms of `A` that `B` does not name.
    ///
    /// TypeScript distributes `A extends B ? never : A` over `A`'s union arms,
    /// so the result is `A` minus every arm `B` covers. Two consequences follow
    /// from how Smelt interns types, and both are the general rule rather than
    /// a special case:
    ///
    /// * A union of LITERAL types is already its arms' common base type here
    ///   (`200 | 204 | 404` interns as `Float`, because every arm's widened
    ///   type is `number`). Such a base is not a `Type::Union`, so nothing can
    ///   be subtracted from it and the answer is the base itself — which is the
    ///   right answer: removing some literals from a literal union still leaves
    ///   a union of literals, and that is still `number`. Answering `never`
    ///   (or, as before this arm existed, `unknown`) would lose a type the
    ///   source fixed. Hono's `ContentfulStatusCode =
    ///   Exclude<StatusCode, ContentlessStatusCode>` is exactly this shape.
    /// * A union whose arms Smelt does model distinctly (`string | number`
    ///   minus `number`) really does lose the named arms.
    ///
    /// Removing every arm leaves `never`, matching TypeScript.
    pub(super) fn union_without_members(
        &mut self,
        base: smelt_hir::TypeId,
        excluded: smelt_hir::TypeId,
    ) -> smelt_hir::TypeId {
        let Some(Type::Union(members)) = self.ctx.krate.types.get(base).cloned() else {
            return base;
        };
        let removed = self.flatten_union_member_types(excluded);
        let kept: Vec<smelt_hir::TypeId> = members
            .into_iter()
            .filter(|member| !removed.contains(member))
            .collect();
        if kept.is_empty() {
            return self.ctx.krate.types.intern(Type::Never);
        }
        self.union_of_types_or_unknown(kept)
    }

    /// `ArrayBufferView` (a typed-array view or a `DataView`), or with
    /// `with_storage` `BufferSource` (those plus an `ArrayBuffer`).
    pub(super) fn byte_source_union_type(&mut self, with_storage: bool) -> smelt_hir::TypeId {
        let mut members = vec!["Uint8Array", "DataView"];
        if with_storage {
            members.push("ArrayBuffer");
        }
        let items = members
            .into_iter()
            .map(|name| {
                let name = self.intern_type_name(name);
                self.ctx.krate.types.intern(Type::Class {
                    name,
                    args: Vec::new(),
                })
            })
            .collect();
        self.ctx.krate.types.intern(Type::Union(items))
    }

    /// Collapse a union of dictionaries with one key type into ONE dictionary
    /// whose value is the union of the arms' values: `Record<string, string> |
    /// Record<string, string[]>` stores as `Record<string, string | string[]>`.
    ///
    /// The union of two record TYPES is a static claim `tsc` already checked;
    /// at run time it is one object whose entries hold either value, which is
    /// exactly the merged dictionary. Kept as a union of two dictionary arms,
    /// every entry write had to pick ONE arm's storage, so Hono's
    /// `getQueryParams` pushed into a `string[]` entry of a record built as the
    /// `string` arm and read `"[object Object]"` back. Arms with different key
    /// types do not merge.
    pub(in crate::lowering) fn dict_union_type(
        &mut self,
        items: &[smelt_hir::TypeId],
    ) -> Option<smelt_hir::TypeId> {
        if items.len() < 2 {
            return None;
        }
        let mut key_ty = None;
        let mut value_tys = Vec::new();
        for item in items {
            let Some(Type::Dict(key, value)) = self.ctx.krate.types.get(*item).cloned() else {
                return None;
            };
            if key_ty.is_some_and(|existing| existing != key) {
                return None;
            }
            key_ty = Some(key);
            for flat in self.flatten_union_member_types(value) {
                if !value_tys.contains(&flat) {
                    value_tys.push(flat);
                }
            }
        }
        let key_ty = key_ty?;
        let value_ty = self.union_of_types_or_unknown(value_tys);
        Some(self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty)))
    }

    /// Collapse unions of list-like arms, which differ by an empty tuple branch or by
    /// element type, into a list type.
    pub(in crate::lowering) fn list_union_type(
        &mut self,
        items: &[smelt_hir::TypeId],
    ) -> Option<smelt_hir::TypeId> {
        let mut element_tys = Vec::new();
        for item in items {
            match self.ctx.krate.types.get(*item).cloned() {
                Some(Type::List(element_ty)) if !element_tys.contains(&element_ty) => {
                    element_tys.push(element_ty);
                }
                Some(Type::List(_)) => {}
                Some(Type::Tuple(tuple_items)) if tuple_items.is_empty() => {}
                _ => return None,
            }
        }
        if element_tys.is_empty() {
            return None;
        }
        let element_ty = match element_tys.as_slice() {
            [single] => *single,
            _ => self.ctx.krate.types.intern(Type::Union(element_tys)),
        };
        Some(self.ctx.krate.types.intern(Type::List(element_ty)))
    }

    /// Whether a union arm has concrete storage in the generated crate, so a
    /// union of such arms is a generated enum rather than the erased carrier.
    ///
    /// Follows codegen's `union_member_is_concrete`, more strictly: primitives
    /// are; a list, set, tuple or future is when everything it holds is; a
    /// class or interface is when it is declared (or a stdlib class with a
    /// concrete runtime type), since an undeclared one renders erased; a record,
    /// map or function arm, `unknown`, `never`, a type parameter, and a nullish
    /// or optional arm are not. A class
    /// declared later than this annotation reads as undeclared here, which only
    /// keeps the union in its erased form.
    pub(in crate::lowering) fn union_arm_has_concrete_storage(
        &self,
        ty: smelt_hir::TypeId,
    ) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(
                Type::Bool
                | Type::Int
                | Type::FixedInt(_)
                | Type::BigInt
                | Type::Float
                | Type::String
                | Type::PyStr
                | Type::PyInt
                | Type::Symbol
                | Type::PropKey,
            ) => true,
            Some(
                container @ (Type::List(_)
                | Type::Set(_)
                | Type::Future(_)
                | Type::Tuple(_)
                | Type::Union(_)
                | Type::GeneratorResult { .. }),
            ) => container
                .children()
                .all(|child| self.union_arm_has_concrete_storage(child)),
            Some(Type::Generator {
                yield_ty,
                return_ty,
                ..
            }) => {
                self.union_arm_has_concrete_storage(*yield_ty)
                    && self.union_arm_has_concrete_storage(*return_ty)
            }
            Some(Type::Class { name, args }) => {
                let declared = self.find_class(*name).is_some()
                    || self.find_interface(*name).is_some()
                    || self.ctx.krate.symbols.get(*name).is_some_and(|text| {
                        smelt_stdlib::typescript_stdlib_class(text)
                            .is_some_and(smelt_stdlib::StdlibClass::has_concrete_runtime_type)
                    });
                declared
                    && args
                        .iter()
                        .all(|arg| self.union_arm_has_concrete_storage(*arg))
            }
            // A record or map arm and a function arm stay erased: a value
            // reaches such a union through a merged record type or another
            // callable spelling than the arm's (`Record<string, string> |
            // Record<string, string[]>` assigned from one record of a merged
            // value type), which the generated enum does not inject.
            Some(
                Type::Dict(_, _)
                | Type::JsMap(_, _)
                | Type::Function(_)
                | Type::Unknown
                | Type::Never
                | Type::TypeParam { .. }
                // A deferred `T[K]` is settled per call, like the type
                // parameter it reads: no storage of its own here.
                | Type::IndexedAccess { .. }
                | Type::None
                | Type::Optional(_),
            )
            | None => false,
        }
    }
}
