//! Inferred result types of unannotated functions: the join of the types a
//! body returns or yields, and the generator type of an unannotated generator
//! with its yield accumulator.

use crate::lowering::body_arena::BodyArena;
use crate::lowering::erasure::NotNewErasure;
use crate::lowering::{GeneratorYieldAccumulator, ModuleBuilder};
use oxc::span::GetSpan;
use smelt_hir::{Body, ErasureReason, ExprKind, Stmt, Type};

impl ModuleBuilder<'_> {
    /// Resolve the concrete generator signature active while lowering a body.
    pub(in crate::lowering) fn initialize_generator_yield_accumulator(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
        _body: &mut Body,
    ) -> GeneratorYieldAccumulator {
        if let Some(Type::Generator {
            is_async: _,
            yield_ty,
            return_ty: _,
            next_ty,
        }) = self
            .fn_ctx
            .return_ty
            .and_then(|ty| self.ctx.krate.types.get(ty))
        {
            return GeneratorYieldAccumulator {
                yield_ty: *yield_ty,
                next_ty: *next_ty,
                // The source syntax is authoritative when an overload supplied
                // the contextual generator type (and may have supplied the
                // sync overload before callback compatibility was considered).
                is_async: function.r#async,
            };
        }
        // not a new erasure: `Unknown` here is the "no contextual Generator
        // type" sentinel the yield lowering compares against; an unannotated
        // generator's real carrier is inferred from its body by
        // `inferred_generator_type`, which records its own erasures.
        let yield_ty = self.unknown_type(NotNewErasure::Comparison);
        let next_ty = self
            .checker_generator_next_type(function)
            .unwrap_or_else(|| self.unknown_type(NotNewErasure::Comparison));
        GeneratorYieldAccumulator {
            yield_ty,
            next_ty,
            // An unannotated generator may not have a contextual Generator type,
            // but its syntax still determines whether delegation may suspend.
            is_async: function.r#async,
        }
    }

    /// Leave completion to explicit `return` statements or the normal fallthrough.
    pub(in crate::lowering) fn push_generator_return(
        _accumulator: GeneratorYieldAccumulator,
        _function: &oxc::ast::ast::Function<'_>,
        _body: &mut Body,
    ) {
    }

    /// The type `.next(value)` sends into an unannotated generator
    /// `function`, as the TypeScript checker infers it: `tsc` joins the
    /// contextual types of its `yield` expressions
    /// (`const r: number | undefined = yield n` makes it
    /// `number | undefined`). `None` without checker facts, and for an
    /// inferred `unknown`/`any` (no `yield` result is used) or any answer that
    /// is not fully concrete, which keeps the erased resume channel.
    pub(in crate::lowering) fn checker_generator_next_type(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
    ) -> Option<smelt_hir::TypeId> {
        let spans = [
            Some((function.span.start, function.span.end)),
            function.id.as_ref().map(|id| (id.span.start, id.span.end)),
        ];
        for (start, end) in spans.into_iter().flatten() {
            let records_before = self.ctx.krate.erasures.len();
            let Some(returned) = self.checker_function_return_type(start, end) else {
                continue;
            };
            let next_ty = match self.ctx.krate.types.get(returned) {
                Some(Type::Generator { next_ty, .. }) => *next_ty,
                _ => returned,
            };
            if matches!(
                self.ctx.krate.types.get(returned),
                Some(Type::Generator { .. })
            ) && !self.type_mentions_unknown(next_ty)
            {
                self.ctx.krate.erasures.truncate(records_before);
                return Some(next_ty);
            }
            self.ctx.krate.erasures.truncate(records_before);
        }
        None
    }

    /// The generator type of an unannotated generator METHOD
    /// (`*[Symbol.iterator]() { yield 1; }`), as the checker infers it.
    ///
    /// A method's signature is collected before its body is lowered (same-class
    /// calls and `for-of`/spread over an instance read it), so the body-driven
    /// [`Self::inferred_generator_type`] that an unannotated generator
    /// `function` takes is not available yet; the checker's inferred
    /// `Generator<Y, R, N>` is the type an annotation spelling it would give.
    /// Its yield and return types must be concrete (else `None`, and the
    /// method keeps its erased return); a `next` type the checker leaves
    /// `unknown` (no `yield` result is used) is the erased resume channel, as
    /// in [`Self::inferred_generator_type`].
    pub(in crate::lowering) fn checker_method_generator_type(
        &mut self,
        method: &oxc::ast::ast::MethodDefinition<'_>,
    ) -> Option<smelt_hir::TypeId> {
        if !method.value.generator {
            return None;
        }
        // The checker types a method at its declaration name: the key, which
        // for a computed key (`[Symbol.iterator]`) is the bracketed name
        // around the key expression oxc spans.
        let key = method.key.span();
        let key_span = if method.computed {
            (key.start.saturating_sub(1), key.end.saturating_add(1))
        } else {
            (key.start, key.end)
        };
        let spans = [key_span, (method.value.span.start, method.value.span.end)];
        for (start, end) in spans {
            let records_before = self.ctx.krate.erasures.len();
            let Some(returned) = self.checker_function_return_type(start, end) else {
                continue;
            };
            let Some(Type::Generator {
                yield_ty,
                return_ty,
                next_ty,
                ..
            }) = self.ctx.krate.types.get(returned).cloned()
            else {
                self.ctx.krate.erasures.truncate(records_before);
                continue;
            };
            if self.type_mentions_unknown(yield_ty) || self.type_mentions_unknown(return_ty) {
                self.ctx.krate.erasures.truncate(records_before);
                continue;
            }
            self.ctx.krate.erasures.truncate(records_before);
            let next_ty = if self.type_mentions_unknown(next_ty) {
                self.erase(ErasureReason::MissingAnnotation)
            } else {
                next_ty
            };
            return Some(self.ctx.krate.types.intern(Type::Generator {
                is_async: method.value.r#async,
                yield_ty,
                return_ty,
                next_ty,
            }));
        }
        None
    }

    /// Whether `ty` is or holds an erased (`Unknown`) part.
    fn type_mentions_unknown(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Unknown) | None => true,
            Some(other) => other
                .children()
                .any(|child| self.type_mentions_unknown(child)),
        }
    }

    /// Infer the concrete generator carrier produced by an unannotated body.
    ///
    /// `next_ty` is what the checker infers `.next(value)` sends
    /// (`checker_generator_next_type`, the same type the body's `yield`
    /// expressions were lowered at); without such an answer it remains
    /// `Unknown`, because arbitrary callers may pass any value to
    /// `.next(value)`. That is a genuine dynamic input boundary; yielded and
    /// returned values remain concrete or union-typed and never use that
    /// runtime carrier.
    pub(in crate::lowering) fn inferred_generator_type(
        &mut self,
        body: &Body,
        is_async: bool,
        function: &oxc::ast::ast::Function<'_>,
    ) -> smelt_hir::TypeId {
        let mut yielded = Vec::new();
        for expr in &body.exprs {
            let item_ty = match &expr.kind {
                ExprKind::GeneratorYield { value } => Some(Self::expr_ty(body, *value)),
                ExprKind::GeneratorDelegate { generator } => {
                    match self.ctx.krate.types.get(Self::expr_ty(body, *generator)) {
                        Some(
                            Type::Generator { yield_ty, .. }
                            | Type::List(yield_ty)
                            | Type::Set(yield_ty),
                        ) => Some(*yield_ty),
                        Some(Type::String) => Some(self.ctx.krate.types.intern(Type::String)),
                        Some(Type::Tuple(items)) => {
                            let items = items.clone();
                            Some(self.joined_inferred_type(items))
                        }
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some(item_ty) = item_ty
                && !yielded.contains(&item_ty)
            {
                yielded.push(item_ty);
            }
        }
        let yield_ty = self.joined_inferred_type(yielded);

        let mut returned = Vec::new();
        Self::collect_return_types_from_block(body, body.root, &mut returned);
        let root_falls_through = Self::generator_root_falls_through(body);
        if returned.is_empty() || root_falls_through {
            let none_ty = self.ctx.krate.types.intern(Type::None);
            if !returned.contains(&none_ty) {
                returned.push(none_ty);
            }
        }
        let return_ty = self.joined_inferred_type(returned);
        let next_ty = self
            .checker_generator_next_type(function)
            .unwrap_or_else(|| self.erase(ErasureReason::MissingAnnotation));
        self.ctx.krate.types.intern(Type::Generator {
            is_async,
            yield_ty,
            return_ty,
            next_ty,
        })
    }

    /// Widen an annotated generator completion when control can fall through.
    ///
    /// JavaScript completes such a generator with `undefined`; synthesizing a
    /// concrete `R::default()` changes observable `.next().value` semantics.
    pub(in crate::lowering) fn generator_type_with_fallthrough(
        &mut self,
        body: &Body,
        generator_ty: smelt_hir::TypeId,
    ) -> smelt_hir::TypeId {
        let Some(Type::Generator {
            is_async,
            yield_ty,
            return_ty,
            next_ty,
        }) = self.ctx.krate.types.get(generator_ty).cloned()
        else {
            return generator_ty;
        };
        if !Self::generator_root_falls_through(body)
            || matches!(
                self.ctx.krate.types.get(return_ty),
                Some(Type::None | Type::Optional(..))
            )
        {
            return generator_ty;
        }
        let return_ty = self
            .ctx
            .krate
            .types
            .intern(Type::Optional(return_ty, smelt_hir::Absence::Undefined));
        self.ctx.krate.types.intern(Type::Generator {
            is_async,
            yield_ty,
            return_ty,
            next_ty,
        })
    }

    /// Return whether the root generator block lacks a terminal return.
    fn generator_root_falls_through(body: &Body) -> bool {
        body.block_at(body.root)
            .and_then(|block| block.stmts.last())
            .and_then(|stmt| body.stmt_at(*stmt))
            .is_none_or(|stmt| !matches!(stmt, Stmt::Return(_)))
    }

    /// The one join of the types a body answers with: its returns, its yields.
    ///
    /// Types with a common shape take it, by the same rules a ternary's arms use
    /// (`number | undefined` is `Optional`, `Int` and `Float` are `Float`, two
    /// compatible function types are one). Types without one stay a union: the
    /// join never erases a value it could still name, and only a type already
    /// erased stays erased. No types is `undefined`.
    pub(in crate::lowering) fn joined_inferred_type(
        &mut self,
        mut types: Vec<smelt_hir::TypeId>,
    ) -> smelt_hir::TypeId {
        types.sort_unstable_by_key(|ty| ty.0);
        types.dedup();
        let Some((&first, rest)) = types.split_first() else {
            return self.ctx.krate.types.intern(Type::None);
        };
        let mut joined = first;
        for &ty in rest {
            let Some(pair) = self.common_shape(joined, ty) else {
                return self.ctx.krate.types.intern(Type::Union(types));
            };
            joined = pair;
        }
        joined
    }

    /// The single shape two inferred types share, if they have one.
    fn common_shape(
        &mut self,
        left: smelt_hir::TypeId,
        right: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        let either_erased = self.type_contains_unknown(left) || self.type_contains_unknown(right);
        let has_no_single_shape = |ty: Option<&Type>| matches!(ty, Some(Type::Union(_)));
        if !either_erased
            && (has_no_single_shape(self.ctx.krate.types.get(left))
                || has_no_single_shape(self.ctx.krate.types.get(right)))
        {
            return None;
        }
        let shared = self.conditional_branch_type(left, right, None, 0, 0).ok()?;
        let lost_a_type =
            self.ctx.krate.types.get(shared) == Some(&Type::Unknown) && !either_erased;
        (!lost_a_type).then_some(shared)
    }
}
