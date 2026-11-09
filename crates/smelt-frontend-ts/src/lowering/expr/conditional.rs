//! Conditional (ternary) expressions: lowering both arms and joining their
//! types into one result type.

use crate::SmeltError;
use crate::lowering::body_arena::BodyArena;
use crate::lowering::peel::{Wrappers, peel};
use oxc::ast::ast::Expression;
use smelt_hir::{Body, ErasureReason, Expr, ExprKind, Span, Type};
use super::super::ModuleBuilder;

impl ModuleBuilder<'_> {
    /// Lower one arm of a conditional expression, capturing any statement-level
    /// side effects it produces into a per-arm block so they run only when that
    /// arm is taken.
    ///
    /// Effectful sub-expressions such as a postfix `k++` inside `xs[k++]` lower
    /// their store into the current statement sink (see `update_expression`).
    /// For a ternary operand that block is the *enclosing* statement block, so
    /// without redirection the increment is hoisted out of the branch and runs
    /// unconditionally on every evaluation — the es-toolkit `partial`/
    /// `partialRight` `providedArgs[idx++]` placeholder bug. Redirecting the arm
    /// to a fresh block and wrapping it as an `ExprKind::Block` (tail = the arm
    /// value) keeps the increment inside the arm, where MIR lowers it within the
    /// matching switch branch. Arms with no side effects return their expression
    /// unwrapped so simple ternaries are unchanged.
    pub(in crate::lowering) fn lower_conditional_arm(
        &mut self,
        body: &mut Body,
        span: Span,
        lower: impl FnOnce(&mut Self, &mut Body) -> Result<smelt_hir::ExprId, SmeltError>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let arm_block = body.push_block(span);
        let arm_expr = self.with_stmt_block(arm_block, |this| lower(this, body))?;
        if body.blocks[arm_block.0 as usize].stmts.is_empty() {
            return Ok(arm_expr);
        }
        body.blocks[arm_block.0 as usize].tail = Some(arm_expr);
        let ty = Self::expr_ty(body, arm_expr);
        Ok(body.push_expr(Expr {
            kind: ExprKind::Block(arm_block),
            ty,
            span,
        }))
    }

    /// Lower a TypeScript conditional expression when it appears outside normal expression nodes.
    pub(in crate::lowering) fn conditional_expression(
        &mut self,
        conditional: &oxc::ast::ast::ConditionalExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        // A constructor-presence guard `Ctor ? new Ctor(...) : fallback` where
        // `Ctor` is a bare host constructor Smelt always models as present (typed
        // arrays, `ArrayBuffer`/`Blob`/`Buffer`, the marker-only host builtins).
        // The environment probe is always true in Smelt's model, so the ternary
        // yields its consequent; folding to that branch keeps its concrete shape
        // (es-toolkit's `merge` spec writes `Uint8Array ? new Uint8Array([1]) : {
        // buffer: [1] }`, whose `List` and `Dict` arms have no common concrete
        // Rust type — widening the merge to `SmeltUnknown` to reconcile them is
        // exactly the avoidable erasure the ABI rules forbid). A user binding or
        // class of the same name shadows the global and is not folded.
        if let Expression::Identifier(test) = &conditional.test
            && self.identifier_is_always_present_global_constructor(test.name.as_str())
        {
            return self.expression_with_hint(&conditional.consequent, body, type_hint);
        }
        let cond = self.condition_expression(&conditional.test, body)?;
        let arm_span = self.span(conditional.span.start, conditional.span.end);
        // Each arm sees the test's flow facts, exactly as the ternary arm of
        // `expression_with_hint` and an `if` statement do. This entry point
        // is a ternary in ARGUMENT position (`console.log(first ? first.join(',')
        // : ..)`).
        let then_narrowing = self.guard_narrowing(&conditional.test, body);
        let then_expr = self.with_narrowing(then_narrowing, |this| {
            this.lower_conditional_arm(body, arm_span, |slf, body| {
                slf.expression_with_hint(&conditional.consequent, body, type_hint)
            })
        });
        let then_expr = then_expr?;
        // The consequent's type is the alternate's hint (`c ? [1] : []` types
        // the empty literal as the consequent's list) — except a `null` /
        // `undefined` consequent, which says nothing about the alternate's
        // shape: `c ? null : d ? ['a'] : ['b']` would otherwise lower the
        // nested array literals AGAINST `None`. Likewise a consequent that is
        // a `never`-returning function (`c ? rethrow : (e) => handle(e)`): its
        // return says nothing about the alternate's. The outer hint is the
        // right one there.
        //
        // A non-empty array literal has a shape of its own, and a literal takes
        // its hint on trust because a checked program's hint is always right.
        // This one is only the other arm's type: `c ? ["a"] : [1]` would type
        // `[1]` as `string[]`. The arms join below instead.
        let then_ty = Self::expr_ty(body, then_expr);
        let alternate_has_own_shape = matches!(
            peel(&conditional.alternate, Wrappers::Parens),
            Expression::ArrayExpression(array) if !array.elements.is_empty()
        );
        // A primitive consequent says nothing about the alternate either, and
        // hinting it would convert a different primitive arm to it
        // (`c ? s : Number(s)` turned the number into a string).
        let then_is_primitive = matches!(
            self.ctx.krate.types.get(then_ty),
            Some(Type::String | Type::Float | Type::Int | Type::Bool)
        );
        let branch_hint = if self.ctx.krate.types.get(then_ty) == Some(&Type::None)
            || self.function_returns_never(then_ty)
            || alternate_has_own_shape
            || then_is_primitive
        {
            type_hint
        } else {
            Some(then_ty)
        };
        let else_narrowing = self.inverse_guard_narrowing(&conditional.test, body);
        let else_expr = self.with_narrowing(else_narrowing, |this| {
            this.lower_conditional_arm(body, arm_span, |slf, body| {
                slf.expression_with_hint(&conditional.alternate, body, branch_hint)
            })
        });
        let else_expr = else_expr?;
        let ty = self.ternary_branch_type(
            body,
            then_expr,
            else_expr,
            type_hint,
            conditional.span.start,
            conditional.span.end,
        )?;
        Ok(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            },
            ty,
            span: self.span(conditional.span.start, conditional.span.end),
        }))
    }

    /// Whether `ty` is an object map: `[]` cannot take its type from such an
    /// arm, because the value stays an array at run time (`c ? [] : {}`).
    fn is_dict_type(&self, ty: smelt_hir::TypeId) -> bool {
        matches!(self.ctx.krate.types.get(ty), Some(Type::Dict(..)))
    }

    /// `c ? null : undefined`: the one join of two `None` arms that must keep
    /// which of the two it holds.
    ///
    /// TypeScript's `null` and `undefined` share `Type::None`, and a `None`
    /// value is the unit `()`, so a value that is `null` on one path and
    /// `undefined` on the other would print, compare and erase as whichever
    /// spelling its type defaults to. When one arm is the `null` literal and
    /// the other the `undefined` literal, the result is `Optional<None>`
    /// instead: absent is `undefined` and present-unit is `null`, so both
    /// survive a return, a comparison and an erasure. `None` when the arms are
    /// not that pair.
    fn null_or_undefined_join(
        &mut self,
        body: &Body,
        then_expr: smelt_hir::ExprId,
        else_expr: smelt_hir::ExprId,
    ) -> Option<smelt_hir::TypeId> {
        let then_literal = Self::nullish_literal_of(body, then_expr)?;
        let else_literal = Self::nullish_literal_of(body, else_expr)?;
        if then_literal == else_literal {
            return None;
        }
        let none = self.ctx.krate.types.intern(Type::None);
        Some(self.ctx.krate.types.intern(Type::Optional(none)))
    }

    /// `Some(true)` for a `null` literal arm, `Some(false)` for `undefined`.
    fn nullish_literal_of(body: &Body, expr: smelt_hir::ExprId) -> Option<bool> {
        match &body.expr_at(expr)?.kind {
            ExprKind::Literal(smelt_hir::Literal::None) => Some(true),
            ExprKind::Literal(smelt_hir::Literal::Undefined) => Some(false),
            ExprKind::TypeAssert { value } => Self::nullish_literal_of(body, *value),
            _ => None,
        }
    }

    /// Whether `ty` names a stdlib class backed by a concrete Rust runtime
    /// type (`StdlibClass::has_concrete_runtime_type`).
    fn concrete_stdlib_class_type(&self, ty: smelt_hir::TypeId) -> bool {
        matches!(self.ctx.krate.types.get(ty), Some(Type::Class { name, .. })
            if self
                .ctx
                .krate
                .symbols
                .get(*name)
                .and_then(smelt_stdlib::typescript_stdlib_class)
                .is_some_and(smelt_stdlib::StdlibClass::has_concrete_runtime_type))
    }

    /// The union of two conditional arms, flattening an arm that is already a
    /// union so `A : (B | C)` is `A | B | C`.
    fn union_of_branch_types(&mut self, then_ty: smelt_hir::TypeId, else_ty: smelt_hir::TypeId) -> smelt_hir::TypeId {
        let mut members = Vec::new();
        for arm in [then_ty, else_ty] {
            match self.ctx.krate.types.get(arm).cloned() {
                Some(Type::Union(items)) => members.extend(items),
                _ => members.push(arm),
            }
        }
        members.dedup();
        self.ctx.krate.types.intern(Type::Union(members))
    }

    /// Whether `ty` is a function type whose declared return is `never`.
    fn function_returns_never(&self, ty: smelt_hir::TypeId) -> bool {
        let ty = self.type_param_constraint_or_self(ty);
        matches!(
            self.ctx.krate.types.get(ty),
            Some(Type::Function(function))
                if matches!(self.ctx.krate.types.get(function.return_ty), Some(Type::Never))
        )
    }

    /// The type of a ternary `c ? a : b` whose arms lowered to `then_expr`
    /// and `else_expr`: the conditional expression's own join, in value and
    /// argument position alike.
    ///
    /// TypeScript types `c ? a : b` as `typeof a | typeof b`, so after every
    /// closer common shape (same type, numeric widening, `None`/optional,
    /// functions, an empty-list arm, a union already holding the other arm,
    /// strings, lists) two concrete unrelated arms join to a generated union
    /// rather than failing. The empty-list arms read the arm EXPRESSIONS, which
    /// is why this takes them and not just their types.
    ///
    /// Not [`Self::conditional_branch_type`], which is a shared "do these two
    /// types reconcile?" query for `reduce`, callback returns and the logical
    /// operators, and answers `Err` where a ternary joins to a union. A
    /// ternary in argument position once asked that query, so
    /// `console.log(c ? 1 : "a")` and `f(c ? recordA : recordB)` failed to
    /// lower while the same ternary assigned to a `const` was a union.
    fn ternary_branch_type(
        &mut self,
        body: &Body,
        then_expr: smelt_hir::ExprId,
        else_expr: smelt_hir::ExprId,
        type_hint: Option<smelt_hir::TypeId>,
        start: u32,
        end: u32,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        let then_ty = Self::expr_ty(body, then_expr);
        let else_ty = Self::expr_ty(body, else_expr);
        // `never` is the bottom type: `c ? rethrow : (e) => handle(e)`, with
        // `rethrow: (e: unknown) => never`, is a callable returning what
        // `handle` returns, because the `never` arm returns nothing at all.
        // Checked before every other function rule, which would keep the
        // `never` arm's signature and type the join's result by its erased
        // return.
        let never_join = if self.compatible_function_branch_types(then_ty, else_ty) {
            match (self.function_returns_never(then_ty), self.function_returns_never(else_ty)) {
                (true, false) => Some(else_ty),
                (false, true) => Some(then_ty),
                _ => None,
            }
        } else {
            None
        };
        let ty = if let Some(joined) = never_join {
            joined
        } else if let Some(nullish) = self.null_or_undefined_join(body, then_expr, else_expr) {
            nullish
        } else if then_ty == else_ty {
            then_ty
        } else if self.numeric_type_compatible(then_ty, else_ty) {
            self.ctx.krate.types.intern(Type::Float)
        } else if matches!(
            (
                self.ctx.krate.types.get(then_ty),
                self.ctx.krate.types.get(else_ty)
            ),
            (Some(Type::TypeParam { .. }), Some(Type::TypeParam { .. }))
        ) {
            then_ty
        } else if self.date_runtime_float_matches_type_param(then_ty, else_ty) {
            else_ty
        } else if self.date_runtime_float_matches_type_param(else_ty, then_ty) {
            then_ty
        } else if Self::is_empty_list_expr(body, then_expr) && !self.is_dict_type(else_ty) {
            else_ty
        } else if Self::is_empty_list_expr(body, else_expr) && !self.is_dict_type(then_ty) {
            then_ty
        } else if self.ctx.krate.types.get(then_ty) == Some(&Type::None) {
            self.ctx.krate.types.intern(Type::Optional(else_ty))
        } else if self.ctx.krate.types.get(else_ty) == Some(&Type::None) {
            self.ctx.krate.types.intern(Type::Optional(then_ty))
        } else if let Some(unified) =
            self.unify_optional_conditional_branches(then_ty, else_ty)
        {
            // One arm is `Optional<inner>` and the other unifies with
            // `inner`, so the TypeScript union `inner | undefined` IS
            // `Optional<inner>`, which is a closer answer than anything
            // below. It must precede the string-compatible test, which
            // accepts any `Type::Class` — the variant also spells an
            // opaque unresolved name — and would unify a
            // `Headers`/`Headers | undefined` join to `String`.
            unified
        } else if self.compatible_function_branch_types(then_ty, else_ty) {
            then_ty
        } else if let Some(function_ty) = self.single_function_branch_type(then_ty, else_ty) {
            function_ty
        } else if matches!(self.ctx.krate.types.get(then_ty), Some(Type::Union(items)) if items.contains(&else_ty)) {
            then_ty
        } else if matches!(self.ctx.krate.types.get(else_ty), Some(Type::Union(items)) if items.contains(&then_ty)) {
            else_ty
        } else if type_hint
            .is_some_and(|hint| self.ctx.krate.types.get(hint) == Some(&Type::Unknown))
            || self.ctx.krate.types.get(then_ty) == Some(&Type::Unknown)
            || self.ctx.krate.types.get(else_ty) == Some(&Type::Unknown)
        {
            // The hint or an arm is already erased; a concrete arm is erased
            // here, and recorded as such.
            self.erased_join(&[then_ty, else_ty])
        } else if self.concrete_stdlib_class_type(then_ty) || self.concrete_stdlib_class_type(else_ty)
        {
            // A modeled stdlib class with a concrete runtime type (a typed
            // array, `Headers`, `URL`, ..) is no more string-compatible than a
            // declared one: the string test below accepts any `Type::Class`,
            // which joined `typeof s === 'string' ? encode(s) : s` over
            // `string | BufferSource` to `String`. The two arms are two members.
            self.union_of_branch_types(then_ty, else_ty)
        } else if self.declared_class_type(then_ty) || self.declared_class_type(else_ty) {
            // A *declared* class arm has no common Rust shape with
            // anything but itself, and it does have a concrete join: the
            // generated tagged union. Every closer answer has already
            // been tried above — same type, numeric, `None`, a function
            // pair, a union that already contains the other arm — so
            // reaching here with a declared class means the two arms are
            // genuinely two members, which is what TypeScript types
            // `c ? a : b` as.
            //
            // Without this arm the branches fall into the
            // string-compatible test below, and that test accepts ANY
            // `Type::Class` — the variant also spells an opaque
            // unresolved name — so the conditional unified to `String`
            // and emitted a `String` local that the class values were
            // then assigned into. `flag ? new Doc("d") : "text"` at a
            // `string | Doc` return type produced
            // `let mut _smelt_tmp: String;` and `_smelt_tmp = <Doc>`,
            // which does not compile.
            //
            // Not only when both arms are declared classes: `Doc : "text"` is the
            // commoner shape, since a union of a class and a primitive is how an
            // optional-payload result is usually spelled.
            self.ctx
                .krate
                .types
                .intern(Type::Union(vec![then_ty, else_ty]))
        } else if self.is_string_compatible_type(then_ty)
            && (self.is_string_compatible_type(else_ty)
                || self.union_has_string_compatible_member(else_ty))
            || self.is_string_compatible_type(else_ty)
                && self.union_has_string_compatible_member(then_ty)
        {
            self.ctx.krate.types.intern(Type::String)
        } else if matches!(self.ctx.krate.types.get(then_ty), Some(Type::Dict(_, _)))
            && matches!(self.ctx.krate.types.get(else_ty), Some(Type::Dict(_, _)))
        {
            self.ctx
                .krate
                .types
                .intern(Type::Union(vec![then_ty, else_ty]))
        } else if let (Some(Type::List(then_item)), Some(Type::List(else_item))) = (
            self.ctx.krate.types.get(then_ty).cloned(),
            self.ctx.krate.types.get(else_ty).cloned(),
        ) {
            // Both branches are arrays whose element types differ. Unify
            // the element types with the same branch rules and keep an
            // array shape, falling back to a list of the union of the
            // element types when they have no closer common shape.
            let item_ty = self.unify_conditional_list_item_type(then_item, else_item);
            self.ctx.krate.types.intern(Type::List(item_ty))
        } else if self.type_contains_unknown(then_ty) || self.type_contains_unknown(else_ty)
        {
            // An arm already holds `unknown`; a concrete other arm is erased
            // here, and recorded as such.
            self.erased_join(&[then_ty, else_ty])
        } else if let Some(hint) = type_hint
            && !self.concrete_type_requires_never_value(hint)
        {
            hint
        } else if self.erased_or_union_surface(then_ty)
            || self.erased_or_union_surface(else_ty)
            || matches!(self.ctx.krate.types.get(then_ty), Some(Type::Union(_)))
            || matches!(self.ctx.krate.types.get(else_ty), Some(Type::Union(_)))
            || matches!(
                (
                    self.ctx.krate.types.get(then_ty),
                    self.ctx.krate.types.get(else_ty)
                ),
                (Some(Type::Function(_)), Some(Type::Function(_)))
            )
        {
            // One branch keeps a union/erased surface with no single
            // concrete Rust shape (e.g. `isArrayLike(source) ? source :
            // Object.values(source)` where `source` stays `ArrayLike<T>
            // | Record<string, T>`): the merged value is a genuine
            // dynamic boundary and widens to `unknown`.
            self.erase_with(ErasureReason::DynamicOperator, "?: union arms")
        } else if !self.concrete_type_requires_never_value(then_ty)
            && !self.concrete_type_requires_never_value(else_ty)
        {
            // Two concrete, unrelated arms. TypeScript types `c ? a : b`
            // as `typeof a | typeof b`, and that union is the join — the
            // same rule `??` applies (see
            // the concrete-arms branch of `??` lowering).
            // Blocking here refused well-typed TypeScript; picking one
            // arm's type would emit that arm's type carrying the other
            // arm's value.
            self.ctx
                .krate
                .types
                .intern(Type::Union(vec![then_ty, else_ty]))
        } else {
            return Err(SmeltError::unsupported(
                self.span(start, end),
                format!(
                    "conditional expression branches must have the same lowered type (then: {:?}, else: {:?})",
                    self.ctx.krate.types.get(then_ty),
                    self.ctx.krate.types.get(else_ty)
                ),
            ));
        };
        Ok(ty)
    }

    /// Return whether a bare identifier names a host constructor that Smelt
    /// always models as present, so a `Ctor ? … : …` environment-presence guard
    /// folds to its consequent.
    ///
    /// The set matches the constructors with a concrete construct + `instanceof`
    /// lowering: the typed-array views and
    /// `ArrayBuffer`/`Blob`/`File`/`Buffer`/`DataView` (all byte-backed host
    /// objects), plus the marker-only host builtins.
    /// A local binding or user class of the same name shadows the global, so it
    /// is excluded — the guard then lowers normally.
    fn identifier_is_always_present_global_constructor(&self, name: &str) -> bool {
        let Some(host) = self.host_global_named(name) else {
            return false;
        };
        smelt_stdlib::is_typed_array_class_name(name) || Self::is_known_defined_global_constructor(host)
    }

    /// Compute the result type for a conditional expression's branches.
    pub(in crate::lowering) fn conditional_branch_type(
        &mut self,
        then_ty: smelt_hir::TypeId,
        else_ty: smelt_hir::TypeId,
        type_hint: Option<smelt_hir::TypeId>,
        start: u32,
        end: u32,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        if then_ty == else_ty {
            Ok(then_ty)
        } else if self.numeric_type_compatible(then_ty, else_ty) {
            Ok(self.ctx.krate.types.intern(Type::Float))
        } else if matches!(
            (
                self.ctx.krate.types.get(then_ty),
                self.ctx.krate.types.get(else_ty)
            ),
            (Some(Type::TypeParam { .. }), Some(Type::TypeParam { .. }))
        ) {
            Ok(then_ty)
        } else if self.date_runtime_float_matches_type_param(then_ty, else_ty) {
            Ok(else_ty)
        } else if self.date_runtime_float_matches_type_param(else_ty, then_ty) {
            Ok(then_ty)
        } else if self.ctx.krate.types.get(then_ty) == Some(&Type::None) {
            Ok(self.ctx.krate.types.intern(Type::Optional(else_ty)))
        } else if self.ctx.krate.types.get(else_ty) == Some(&Type::None) {
            Ok(self.ctx.krate.types.intern(Type::Optional(then_ty)))
        } else if let Some(unified) = self.unify_optional_conditional_branches(then_ty, else_ty) {
            // One branch is `Optional<inner>` and the other is compatible with
            // `inner` (equal or numerically widenable), e.g. a `number | undefined`
            // flow-typed local vs a `number` literal (`isNaN(x) ? 0 : x`). Merge
            // to `Optional<unified-inner>` so the optional surface is preserved
            // instead of aborting because a bare and an optional numeric differ.
            Ok(unified)
        } else if self.compatible_function_branch_types(then_ty, else_ty) {
            Ok(then_ty)
        } else if let Some(function_ty) = self.single_function_branch_type(then_ty, else_ty) {
            Ok(function_ty)
        } else if matches!(self.ctx.krate.types.get(then_ty), Some(Type::Union(items)) if items.contains(&else_ty)) {
            Ok(then_ty)
        } else if matches!(self.ctx.krate.types.get(else_ty), Some(Type::Union(items)) if items.contains(&then_ty)) {
            Ok(else_ty)
        } else if let (Some(Type::List(then_item)), Some(Type::List(else_item))) = (
            self.ctx.krate.types.get(then_ty).cloned(),
            self.ctx.krate.types.get(else_ty).cloned(),
        ) {
            // Both branches are arrays whose element types differ (e.g.
            // `index ? numberArray : numberOrNullArray`). Unify the element
            // types and return a list of the unified element type so the array
            // shape is preserved instead of collapsing the whole value to
            // `unknown`.
            let item_ty = self.unify_conditional_list_item_type(then_item, else_item);
            Ok(self.ctx.krate.types.intern(Type::List(item_ty)))
        } else if type_hint
            .is_some_and(|hint| self.ctx.krate.types.get(hint) == Some(&Type::Unknown))
            || self.ctx.krate.types.get(then_ty) == Some(&Type::Unknown)
            || self.ctx.krate.types.get(else_ty) == Some(&Type::Unknown)
            || self.type_contains_unknown(then_ty)
            || self.type_contains_unknown(else_ty)
        {
            // The hint or an arm already holds `unknown`; a concrete arm is
            // erased here, and recorded as such.
            Ok(self.erased_join(&[then_ty, else_ty]))
        } else if let Some(hint) = type_hint
            && !self.concrete_type_requires_never_value(hint)
        {
            Ok(hint)
        } else if self.erased_or_union_surface(then_ty)
            || self.erased_or_union_surface(else_ty)
            || matches!(self.ctx.krate.types.get(then_ty), Some(Type::Union(_)))
            || matches!(self.ctx.krate.types.get(else_ty), Some(Type::Union(_)))
            || matches!(
                (
                    self.ctx.krate.types.get(then_ty),
                    self.ctx.krate.types.get(else_ty)
                ),
                (Some(Type::Function(_)), Some(Type::Function(_)))
            )
        {
            // One branch keeps a union/erased surface with no single concrete
            // Rust shape (e.g. `isArrayLike(source) ? source :
            // Object.values(source)` where `source` stays `ArrayLike<T> |
            // Record<string, T>`): the merged value is a genuine dynamic
            // boundary and widens to `unknown`.
            Ok(self.erase_with(ErasureReason::DynamicOperator, "?: union arms"))
        } else {
            // NO union join here, deliberately. Despite the name, this is a
            // shared *reconcilability query*, not the conditional expression's
            // own type decision: `reduce`/`fold` (`callbacks/list_ops.rs`),
            // `callbacks/transforms.rs` and the logical-operand merge in
            // `expr/operators.rs` all call it and treat `Err` as "these two
            // types have no common lowered shape". Answering a union here makes
            // a `string` accumulator with a `boolean`-returning callback
            // reconcile, which is what
            // `reduce_named_callback_rejects_irreconcilable_return_type` exists
            // to prevent — the fold would assign a `boolean` into a `String`.
            //
            // The ternary's own decision (the inline chain in
            // `expression_with_hint`'s `ConditionalExpression` arm) DOES join to
            // a union, because `c ? a : b` is `typeof a | typeof b` in
            // TypeScript. The two are different questions and only one of them
            // is about a conditional.
            Err(SmeltError::unsupported(
                self.span(start, end),
                format!(
                    "conditional expression branches must have the same lowered type (then: {:?}, else: {:?})",
                    self.ctx.krate.types.get(then_ty),
                    self.ctx.krate.types.get(else_ty)
                ),
            ))
        }
    }

    /// Merge a conditional whose branches are a value and an `Optional` of a
    /// compatible value into a single `Optional` result.
    ///
    /// Returns `Some(Optional<inner>)` when exactly one branch is `Optional<a>`
    /// and the other branch's type unifies with `a` (identical, or numerically
    /// widenable so `Float`/`Int` mix), or when both branches are optionals whose
    /// inners unify. This preserves the optional surface produced by flow typing
    /// (`x: number | undefined; cond ? 0 : x`) instead of failing because a bare
    /// numeric and an optional numeric have different lowered shapes. Returns
    /// `None` when neither branch is optional or the inners are unrelated.
    fn unify_optional_conditional_branches(
        &mut self,
        then_ty: smelt_hir::TypeId,
        else_ty: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        let then_opt = match self.ctx.krate.types.get(then_ty) {
            Some(Type::Optional(inner)) => Some(*inner),
            _ => None,
        };
        let else_opt = match self.ctx.krate.types.get(else_ty) {
            Some(Type::Optional(inner)) => Some(*inner),
            _ => None,
        };
        let inner = match (then_opt, else_opt) {
            (Some(then_inner), Some(else_inner)) => {
                self.unify_compatible_branch_inner(then_inner, else_inner)?
            }
            (Some(then_inner), None) => {
                self.unify_compatible_branch_inner(then_inner, else_ty)?
            }
            (None, Some(else_inner)) => {
                self.unify_compatible_branch_inner(then_ty, else_inner)?
            }
            (None, None) => return None,
        };
        Some(self.ctx.krate.types.intern(Type::Optional(inner)))
    }

    /// Return the unified inner type for two conditional-branch inners that are
    /// identical or numerically compatible, or `None` if unrelated.
    fn unify_compatible_branch_inner(
        &mut self,
        left: smelt_hir::TypeId,
        right: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        if left == right {
            Some(left)
        } else if self.numeric_type_compatible(left, right) {
            Some(self.ctx.krate.types.intern(Type::Float))
        } else {
            None
        }
    }

    /// Unify the element types of two array branches of a conditional expression.
    ///
    /// Both branches are already known to be `List<...>`; this picks an element
    /// type for the merged `List<...>` result. It never fails: when the elements
    /// have no closer common shape it widens to their union (or `unknown` when an
    /// element is itself erased), so an array-producing ternary always keeps an
    /// array shape rather than aborting lowering.
    pub(in crate::lowering) fn unify_conditional_list_item_type(
        &mut self,
        then_item: smelt_hir::TypeId,
        else_item: smelt_hir::TypeId,
    ) -> smelt_hir::TypeId {
        if then_item == else_item {
            then_item
        } else if self.numeric_type_compatible(then_item, else_item) {
            self.ctx.krate.types.intern(Type::Float)
        } else if self.ctx.krate.types.get(then_item) == Some(&Type::None) {
            self.ctx.krate.types.intern(Type::Optional(else_item))
        } else if self.ctx.krate.types.get(else_item) == Some(&Type::None) {
            self.ctx.krate.types.intern(Type::Optional(then_item))
        } else if let (Some(Type::List(then_inner)), Some(Type::List(else_inner))) = (
            self.ctx.krate.types.get(then_item).cloned(),
            self.ctx.krate.types.get(else_item).cloned(),
        ) {
            let inner = self.unify_conditional_list_item_type(then_inner, else_inner);
            self.ctx.krate.types.intern(Type::List(inner))
        } else if self.type_contains_unknown(then_item) || self.type_contains_unknown(else_item) {
            // An element already holds `unknown`; a concrete other element is
            // erased here, and recorded as such.
            self.erased_join(&[then_item, else_item])
        } else {
            self.ctx
                .krate
                .types
                .intern(Type::Union(vec![then_item, else_item]))
        }
    }

    /// Return whether a timestamp-backed Date branch can flow into a generic date type.
    pub(in crate::lowering) fn date_runtime_float_matches_type_param(
        &self,
        actual: smelt_hir::TypeId,
        expected: smelt_hir::TypeId,
    ) -> bool {
        self.ctx.krate.types.get(actual) == Some(&Type::Float)
            && matches!(
                self.ctx.krate.types.get(expected),
                Some(Type::TypeParam { .. })
            )
    }

    /// Return true when an expression is an uninhabited empty array literal.
    pub(in crate::lowering) fn is_empty_list_expr(body: &Body, expr: smelt_hir::ExprId) -> bool {
        matches!(
            body.expr_at(expr),
            Some(Expr {
                kind: ExprKind::ListLit(items),
                ..
            }) if items.is_empty()
        )
    }
}
