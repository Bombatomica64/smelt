//! Array literals contextually typed by a still-generic hint.
//!
//! A generic callee's declared parameter types carry its own type parameters
//! (`function chunk<T>(arr: readonly T[], size: number): T[][]`). An array
//! literal passed there (`chunk([1, 2, 3], 2)`) is lowered with that parameter
//! type as its contextual hint, and adopting the hint wholesale typed the
//! literal `List<T>`. That is a type no value in the CALLER has. TypeScript
//! infers `T = number` from the literal, so the literal is `number[]`.
//!
//! The leaked `T` also broke what came after. HIR type parameters are
//! identified by name, so call-site inference matched the callee's `T`
//! against the identical `T`, bound nothing concrete, and the call result
//! kept the callee's `T[][]`. Codegen then had no binding and erased the whole
//! call (`chunk::<SmeltUnknown>` over a `Vec<SmeltUnknown>`). es-toolkit has
//! this shape 290 times as `List<TypeParam(T)> ← number[]` in the TS7 parity
//! report.
//!
//! # The rule
//!
//! When an array literal adopts a list or tuple hint, every hint position that
//! still mentions a type parameter is filled by the type the elements at that
//! position actually lowered to. For a list hint, that requires all elements
//! to agree on one type. For a tuple hint, each element fills its own slot.
//! Elements that are erased or disagree keep the hint (the earlier
//! behaviour). An empty literal has no evidence and also keeps the hint.
//!
//! Behaviour requirement: unchanged. The elements are the same values and
//! only the static type of the literal changes. Type-preservation
//! requirement: the literal keeps the type its elements prove, so argument
//! inference binds the callee's parameter to the caller's concrete type.
//!
//! The rule is safe for a type parameter that is in scope too. A caller's own
//! `T` only reaches an element when the element has type `T`, and then the
//! fill is the identity. `tsc` rejects a concrete element against an in-scope
//! `T` (`const xs: T[] = [1]`).

use smelt_hir::{Body, ExprId, Type, TypeId};

use super::ModuleBuilder;

/// Nesting depth beyond which the type walks below answer conservatively.
const MAX_DEPTH: usize = 16;

impl ModuleBuilder<'_> {
    /// The type an array literal with elements `items` takes under the
    /// list or tuple `hint`, with type-parameter positions filled from the
    /// elements (see the module docs). Any other hint is returned unchanged.
    pub(in crate::lowering) fn fill_open_array_hint(&mut self, hint: TypeId, items: &[ExprId], body: &Body) -> TypeId {
        if items.is_empty() {
            return hint;
        }
        match self.ctx.krate.types.get(hint).cloned() {
            Some(Type::List(hint_item)) => {
                if !self.overload_constraint_contains_unresolved_type_param(hint_item) {
                    return hint;
                }
                let Some(&first_item) = items.first() else {
                    return hint;
                };
                let first = Self::expr_ty(body, first_item);
                let agree = items.iter().all(|item| Self::expr_ty(body, *item) == first);
                if agree && first != hint_item && self.fills_open_slot(first) {
                    self.ctx.krate.types.intern(Type::List(first))
                } else {
                    hint
                }
            }
            Some(Type::Tuple(slots)) if slots.len() == items.len() => {
                let filled = slots
                    .iter()
                    .zip(items)
                    .map(|(&slot, &item)| {
                        let item_ty = Self::expr_ty(body, item);
                        if self.overload_constraint_contains_unresolved_type_param(slot) && self.fills_open_slot(item_ty) {
                            item_ty
                        } else {
                            slot
                        }
                    })
                    .collect::<Vec<_>>();
                if filled == slots { hint } else { self.ctx.krate.types.intern(Type::Tuple(filled)) }
            }
            _ => hint,
        }
    }

    /// Whether an element's lowered type is evidence for an open hint slot:
    /// it mentions neither `unknown` nor a type parameter anywhere. A
    /// type parameter in the element means the element was itself typed from
    /// the open hint, so it proves nothing.
    fn fills_open_slot(&self, ty: TypeId) -> bool {
        self.type_is_closed(ty, 0)
    }

    /// Whether `ty` is free of `Unknown` and `TypeParam` at every depth.
    fn type_is_closed(&self, ty: TypeId, depth: usize) -> bool {
        if depth > MAX_DEPTH {
            return false;
        }
        let next = depth.saturating_add(1);
        match self.ctx.krate.types.get(ty) {
            None | Some(Type::Unknown | Type::TypeParam { .. }) => false,
            Some(Type::List(item) | Type::Set(item) | Type::Optional(item) | Type::Future(item)) => {
                self.type_is_closed(*item, next)
            }
            Some(Type::Dict(key, value) | Type::JsMap(key, value)) => {
                self.type_is_closed(*key, next) && self.type_is_closed(*value, next)
            }
            Some(Type::Tuple(items) | Type::Union(items)) => items.iter().all(|item| self.type_is_closed(*item, next)),
            Some(Type::Class { args, .. }) => args.iter().all(|arg| self.type_is_closed(*arg, next)),
            Some(Type::Function(function)) => {
                function.params.iter().all(|param| self.type_is_closed(*param, next))
                    && self.type_is_closed(function.return_ty, next)
            }
            Some(_) => true,
        }
    }
}

impl ModuleBuilder<'_> {
    /// The element type of a heterogeneous array literal with no contextual
    /// hint, as the TypeScript 7 checker evaluated it.
    ///
    /// `[1, "1", false]` has no single element type Smelt can join, so it used
    /// to erase (`mixed array literal`), while TypeScript types it
    /// `(string | number | boolean)[]`. The same literal written with that
    /// annotation already lowers to a typed union list, so the checker's
    /// element type is taken as if the source had spelled it. The re-spelling
    /// declines like an annotation does (see `checker_types`). An answer that
    /// still mentions `unknown` is no better than the erasure and is dropped.
    ///
    /// `None` when the elements agree (the ordinary join handles them), when
    /// no checker facts are loaded, or when the checker's type is not an array.
    pub(in crate::lowering) fn checker_mixed_array_item_type(
        &mut self,
        span: oxc::span::Span,
        items: &[ExprId],
        body: &Body,
    ) -> Option<TypeId> {
        let first = Self::expr_ty(body, *items.first()?);
        if items.iter().all(|item| Self::expr_ty(body, *item) == first) {
            return None;
        }
        let lowered = self.checker_binding_type(span.start, span.end)?;
        let Some(&Type::List(item_ty)) = self.ctx.krate.types.get(lowered) else {
            return None;
        };
        (!self.mentions_unknown(item_ty, 0)).then_some(item_ty)
    }

    /// Whether `ty` mentions `Unknown` at any depth.
    fn mentions_unknown(&self, ty: TypeId, depth: usize) -> bool {
        if depth > MAX_DEPTH {
            return true;
        }
        let next = depth.saturating_add(1);
        match self.ctx.krate.types.get(ty) {
            None | Some(Type::Unknown) => true,
            Some(Type::List(item) | Type::Set(item) | Type::Optional(item) | Type::Future(item)) => {
                self.mentions_unknown(*item, next)
            }
            Some(Type::Dict(key, value) | Type::JsMap(key, value)) => {
                self.mentions_unknown(*key, next) || self.mentions_unknown(*value, next)
            }
            Some(Type::Tuple(items) | Type::Union(items)) => items.iter().any(|item| self.mentions_unknown(*item, next)),
            Some(Type::Class { args, .. }) => args.iter().any(|arg| self.mentions_unknown(*arg, next)),
            Some(Type::Function(function)) => {
                function.params.iter().any(|param| self.mentions_unknown(*param, next))
                    || self.mentions_unknown(function.return_ty, next)
            }
            Some(_) => false,
        }
    }
}
