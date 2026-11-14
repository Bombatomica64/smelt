//! Type assignability: the parts of TypeScript assignability that survive Smelt's
//! erased HIR types, with the function, map and numeric compatibility rules.

use crate::lowering::{FunctionType, ModuleBuilder, Type};

impl ModuleBuilder<'_> {
    /// Return whether a lowered actual type can be assigned to an expected type.
    ///
    /// This models the parts of TypeScript assignability that survive Smelt's
    /// erased HIR types: bottom `never`, top-like annotations, union inclusion,
    /// nullish optionals, and recursive container/function shapes.
    pub(in crate::lowering) fn type_assignable_to(
        &self,
        actual: smelt_hir::TypeId,
        expected: smelt_hir::TypeId,
    ) -> bool {
        self.type_assignable_to_inner(actual, expected, 0)
    }

    /// Recursive implementation for `type_assignable_to` with a depth guard.
    pub(in crate::lowering) fn type_assignable_to_inner(
        &self,
        actual: smelt_hir::TypeId,
        expected: smelt_hir::TypeId,
        depth: usize,
    ) -> bool {
        if depth > 32 {
            return actual == expected;
        }
        let actual = self.type_param_constraint_or_self(actual);
        let expected = self.type_param_constraint_or_self(expected);
        if actual == expected {
            return true;
        }
        let Some(actual_ty) = self.ctx.krate.types.get(actual).cloned() else {
            return false;
        };
        let Some(expected_ty) = self.ctx.krate.types.get(expected).cloned() else {
            return false;
        };
        match (actual_ty, expected_ty) {
            (Type::Never, _) | (Type::None, Type::Optional(_)) => true,
            (_, Type::Never) | (Type::Int, Type::Float) => false,
            (_, Type::Unknown) | (Type::Unknown, _) => true,
            // An unconstrained generic parameter (constrained ones were already
            // resolved to their constraint above) is erased by tsc before Smelt
            // sees it: at any instantiated use site it stands for the concrete
            // type tsc already checked, so it is assignable in both directions
            // — including nested inside containers such as `T[]` vs `number[]`.
            (Type::TypeParam { .. }, _) | (_, Type::TypeParam { .. }) => true,
            (Type::Union(actual_items), _) => actual_items
                .iter()
                .all(|item| self.type_assignable_to_inner(*item, expected, depth + 1)),
            (_, Type::Union(expected_items)) => expected_items
                .iter()
                .any(|item| self.type_assignable_to_inner(actual, *item, depth + 1)),
            (Type::Optional(actual_item), Type::Optional(expected_item))
            | (Type::List(actual_item), Type::List(expected_item))
            | (Type::Set(actual_item), Type::Set(expected_item))
            | (Type::Future(actual_item), Type::Future(expected_item)) => {
                self.type_assignable_to_inner(actual_item, expected_item, depth + 1)
            }
            (Type::Optional(_), _) => false,
            (_, Type::Optional(expected_item)) => {
                self.type_assignable_to_inner(actual, expected_item, depth + 1)
            }
            (Type::Tuple(actual_items), Type::Tuple(expected_items)) => {
                actual_items.len() == expected_items.len()
                    && actual_items.iter().zip(expected_items.iter()).all(
                        |(actual_item, expected_item)| {
                            self.type_assignable_to_inner(*actual_item, *expected_item, depth + 1)
                        },
                    )
            }
            (Type::Tuple(actual_items), Type::List(expected_item)) => {
                actual_items.iter().all(|actual_item| {
                    self.type_assignable_to_inner(*actual_item, expected_item, depth + 1)
                })
            }
            // `Dict` and `JsMap` are structurally identical and share the same
            // internal machinery: `JsMap` exists only to preserve the `Map`
            // spelling through interning so erasure can stamp the `__smelt_map`
            // marker. At the type-check level the source-spelling distinction is
            // irrelevant, so the two are assignable in both directions with the
            // same recursive key/value check.
            (
                Type::Dict(actual_key, actual_value) | Type::JsMap(actual_key, actual_value),
                Type::Dict(expected_key, expected_value)
                | Type::JsMap(expected_key, expected_value),
            ) => {
                self.type_assignable_to_inner(actual_key, expected_key, depth + 1)
                    && self.type_assignable_to_inner(actual_value, expected_value, depth + 1)
            }
            (Type::Function(actual_fn), Type::Function(expected_fn)) => {
                Self::function_arity_assignable(&actual_fn, &expected_fn)
                    && self.function_async_assignable(&actual_fn, &expected_fn, depth)
                    && actual_fn.params.iter().zip(expected_fn.params.iter()).all(
                        |(actual_param, expected_param)| {
                            self.type_assignable_to_inner(*expected_param, *actual_param, depth + 1)
                        },
                    )
                    && self.type_assignable_to_inner(
                        actual_fn.return_ty,
                        expected_fn.return_ty,
                        depth + 1,
                    )
            }
            (
                Type::Class {
                    name: actual_name,
                    args: actual_args,
                },
                Type::Class {
                    name: expected_name,
                    args: expected_args,
                },
            ) => {
                actual_name == expected_name
                    && actual_args.len() == expected_args.len()
                    && actual_args.iter().zip(expected_args.iter()).all(
                        |(actual_arg, expected_arg)| {
                            self.type_assignable_to_inner(*actual_arg, *expected_arg, depth + 1)
                        },
                    )
            }
            (actual_ty, expected_ty) => actual_ty == expected_ty,
        }
    }

    /// Return whether a function VALUE of `actual` can be adapted to `expected`
    /// by ignoring the arguments it does not declare.
    ///
    /// JavaScript drops the trailing arguments a callback does not name, so
    /// `() => boolean` inhabits `(attempt: number) => boolean` and TypeScript
    /// reduces the union of the two to the wider one. A Rust closure has a fixed
    /// arity, so the adaptation is not free: the Rust backend renders it with the
    /// arity-widening branch of its function-shape adapter. That is why this is a
    /// separate question from [`Self::function_arity_assignable`], which answers
    /// whether the value can be used AS IS.
    ///
    /// Only a narrower value with the same parameter prefix, the same async bit
    /// and an assignable return type widens; a rest parameter in the value is
    /// left to the ordinary assignability rule, which already covers it.
    pub(in crate::lowering) fn function_value_widens_to(
        &self,
        actual: smelt_hir::TypeId,
        expected: smelt_hir::TypeId,
    ) -> bool {
        let (Some(Type::Function(actual_fn)), Some(Type::Function(expected_fn))) = (
            self.ctx.krate.types.get(actual).cloned(),
            self.ctx.krate.types.get(expected).cloned(),
        ) else {
            return false;
        };
        if actual_fn.rest.is_some() || actual_fn.params.len() >= expected_fn.params.len() {
            return false;
        }
        actual_fn.is_async == expected_fn.is_async
            && actual_fn
                .params
                .iter()
                .zip(expected_fn.params.iter())
                .all(|(actual_param, expected_param)| actual_param == expected_param)
            && self.type_assignable_to(actual_fn.return_ty, expected_fn.return_ty)
    }

    /// Return whether a source function's arity can satisfy an expected function type.
    ///
    /// TypeScript permits assigning a function to a target type that calls it with
    /// fewer arguments, provided every parameter the target would *not* supply is
    /// optional (declared after the source's required-parameter count) or absorbed
    /// by a rest parameter. This is what makes a `Promise<void>` `resolve`, typed
    /// `(value?: T) => void`, assignable to a `() => void` slot such as the FIFO
    /// `Array<() => void>` deferred-task queue in a semaphore.
    ///
    /// A target with *more* parameters than the source is only acceptable when the
    /// source has a rest parameter to absorb the extras; otherwise the source could
    /// not be called with all the arguments the target promises to pass.
    pub(in crate::lowering) fn function_arity_assignable(
        actual: &FunctionType,
        expected: &FunctionType,
    ) -> bool {
        let actual_required = actual.required_params.unwrap_or(actual.params.len());
        if expected.params.len() < actual_required {
            // The target would call the source with fewer arguments than the
            // source requires — only legal if those extra source params are
            // optional, which they are not here.
            return false;
        }
        if expected.params.len() > actual.params.len() {
            // The target promises to pass more arguments than the source declares;
            // only a source rest parameter can absorb the surplus.
            return actual.rest.is_some();
        }
        true
    }

    /// Return whether function async metadata is compatible for assignment.
    ///
    /// TypeScript allows an `async` implementation where the expected function
    /// returns a `Promise<T>`-compatible union, such as `T | Promise<T>`. Smelt
    /// keeps both the async bit and the return surface, so the async bit can be
    /// relaxed when the actual return type already satisfies the expected one.
    pub(in crate::lowering) fn function_async_assignable(
        &self,
        actual: &FunctionType,
        expected: &FunctionType,
        depth: usize,
    ) -> bool {
        actual.is_async == expected.is_async
            || self.type_assignable_to_inner(actual.return_ty, expected.return_ty, depth + 1)
    }

    /// Return whether a key argument can be used with a lowered map key type.
    pub(in crate::lowering) fn map_key_type_compatible(
        &self,
        expected: smelt_hir::TypeId,
        actual: smelt_hir::TypeId,
    ) -> bool {
        let expected = self.type_param_constraint_or_self(expected);
        let actual = self.type_param_constraint_or_self(actual);
        if expected == actual {
            return true;
        }
        if matches!(self.ctx.krate.types.get(expected), Some(Type::Unknown))
            || matches!(self.ctx.krate.types.get(actual), Some(Type::Unknown))
        {
            return !matches!(self.ctx.krate.types.get(actual), Some(Type::None));
        }
        // A property-key record takes ANY key: JavaScript converts whatever is
        // used as a key with `ToPropertyKey`, and so does codegen at the site.
        if matches!(self.ctx.krate.types.get(expected), Some(Type::PropKey)) {
            return true;
        }
        if let Some(Type::Union(items)) = self.ctx.krate.types.get(expected) {
            return items
                .iter()
                .any(|item| self.map_key_type_compatible(*item, actual));
        }
        if let Some(Type::Union(items)) = self.ctx.krate.types.get(actual) {
            return items
                .iter()
                .all(|item| self.map_key_type_compatible(expected, *item));
        }
        if let Some(Type::Optional(inner)) = self.ctx.krate.types.get(actual) {
            return self.map_key_type_compatible(expected, *inner);
        }
        // A string-keyed record takes a symbol key too: it stores it under
        // the symbol's reserved string spelling.
        if self.ctx.krate.types.get(expected) == Some(&Type::String)
            && matches!(
                self.ctx.krate.types.get(actual),
                Some(Type::Symbol | Type::PropKey)
            )
        {
            return true;
        }
        false
    }

    /// Return whether numeric literal widening makes two map value types compatible.
    pub(in crate::lowering) fn numeric_type_compatible(
        &self,
        expected: smelt_hir::TypeId,
        actual: smelt_hir::TypeId,
    ) -> bool {
        expected == actual
            || (self.is_numeric_like_type(expected) && self.is_numeric_like_type(actual))
    }

    /// Return whether a type is represented by Smelt's numeric runtime value.
    pub(in crate::lowering) fn is_numeric_like_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
        {
            Some(Type::Float | Type::Int) => true,
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .all(|item| self.is_numeric_like_type(item)),
            _ => false,
        }
    }

    /// Return whether a type is an `Optional<...>` (or union containing one)
    /// wrapping a numeric-like inner type, i.e. the surface produced by a JS
    /// `number | undefined` / optional parameter.
    pub(in crate::lowering) fn optional_numeric_surface(&self, ty: smelt_hir::TypeId) -> bool {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
        {
            Some(Type::Optional(inner_ty)) => {
                self.is_numeric_like_type(*inner_ty) || self.optional_numeric_surface(*inner_ty)
            }
            Some(Type::Union(items)) => {
                let items = items.clone();
                items.iter().copied().all(|item| {
                    self.ctx.krate.types.get(item) == Some(&Type::None)
                        || self.is_numeric_like_type(item)
                }) && items
                    .iter()
                    .copied()
                    .any(|item| self.is_numeric_like_type(item))
            }
            _ => false,
        }
    }

    /// Whether a type is a *declared* class or interface — one this program
    /// actually materializes as a concrete Rust struct.
    ///
    /// `Type::Class` is overloaded in the TypeScript frontend: it names both a
    /// real declared class and the placeholder an unresolved/opaque type name
    /// lowers to. Predicates that ask "is this a dynamic JavaScript surface?"
    /// mean the *placeholder* sense, so they must not sweep up a declared class
    /// that has a concrete Rust representation. This mirrors the emitter's own
    /// discriminator (`is_erased_class_type`): a class type is erased exactly
    /// when neither a class nor an interface of that name is materialized.
    pub(in crate::lowering) fn declared_class_type(&self, ty: smelt_hir::TypeId) -> bool {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(ty) else {
            return false;
        };
        let name = *name;
        self.class_by_symbol(name).is_some() || self.find_interface(name).is_some()
    }

    /// Return whether a type comes from an erased JavaScript surface.
    pub(in crate::lowering) fn erased_or_union_surface(&self, ty: smelt_hir::TypeId) -> bool {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
        {
            Some(Type::Unknown | Type::Class { .. } | Type::TypeParam { .. }) => true,
            Some(Type::Optional(item)) => self.erased_or_union_surface(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.erased_or_union_surface(item)),
            _ => false,
        }
    }

    // Continued in the next split builder file.
}
