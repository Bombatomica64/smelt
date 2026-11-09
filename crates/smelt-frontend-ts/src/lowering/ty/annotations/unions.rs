//! Union annotations: flattening members, dropping members, the union-or-unknown
//! join, and the dict/list/byte-source unions annotations name.

use crate::lowering::erasure::NotNewErasure;
use crate::lowering::ModuleBuilder;
use smelt_hir::{ErasureReason, Type};

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
    pub(in crate::lowering) fn dict_union_type(&mut self, items: &[smelt_hir::TypeId]) -> Option<smelt_hir::TypeId> {
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
    pub(in crate::lowering) fn list_union_type(&mut self, items: &[smelt_hir::TypeId]) -> Option<smelt_hir::TypeId> {
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
}
