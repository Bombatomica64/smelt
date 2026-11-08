//! Callback body lowering: block/terminating/side-effect statements, guard
//! narrowing, param-pattern binding and inference, arrow closure bodies,
//! throw analysis, and capture-name collection over expressions/statements.

use crate::lowering::body_arena::BodyArena;
use crate::lowering::state::fn_ctx::BodyKind;
use crate::lowering::state::local_scope::ScopeGroups;
use crate::lowering::{
    Argument, ArrayExpressionElement, AssignmentTarget, BinaryOperator, BindingPattern,
    Body, CaptureMode, ChainElement, ClosureCapture, Expr,
    ExprKind, Expression, ForStatementInit, ForStatementLeft, FunctionType, HashMap, HashSet,
    LocalDecl, ModuleBuilder, ObjectPropertyKind, Param, SimpleAssignmentTarget, SmeltError, Span, Statement, Stmt, Type,
};
use smelt_hir::ErasureReason;
use super::super::support::{arrow_block_statements, statement_terminates};
/// The body form of a callback lowered through a real HIR closure body.
///
/// Selects how [`ModuleBuilder::closure_body_expr_from_parts`] lowers the
/// callback body: an arrow expression body (`x => x + 1`) whose single
/// expression becomes the closure's return, or a statement block (`{ ... }`)
/// shared by block-bodied arrows and `function` expression callbacks.
///
/// The variants hold only borrowed AST references, so the enum is `Copy` and is
/// passed by value.
#[derive(Clone, Copy)]
enum ClosureBodyKind<'a, 'src> {
    /// An expression-bodied arrow; its return expression is lowered with a
    /// contextual type hint and may drive return-type inference.
    ArrowExpression(&'a oxc::ast::ast::ArrowFunctionExpression<'src>),
    /// A statement block, used by block-bodied arrows and function expressions.
    Statements(&'a [Statement<'src>]),
}

impl ModuleBuilder<'_> {
    /// Read arrow parameter types, using contextual function types for omitted annotations.
    pub(in crate::lowering) fn arrow_callback_param_types_with_hint(
        &mut self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        contextual_function: Option<&FunctionType>,
    ) -> Result<Vec<smelt_hir::TypeId>, SmeltError> {
        let mut params = Vec::new();
        for (index, param) in arrow.params.items.iter().enumerate() {
            let ty = param
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?
                .or_else(|| {
                    contextual_function
                        .and_then(|function| self.contextual_param_type_at(function, index))
                });
            // An unannotated identifier parameter whose context Smelt lost (or
            // erased) takes the type TypeScript's contextual typing gave it.
            let ty = match (&param.pattern, ty) {
                (BindingPattern::BindingIdentifier(binding), contextual)
                    if param.type_annotation.is_none()
                        && contextual
                            .is_none_or(|contextual_ty| matches!(self.ctx.krate.types.get(contextual_ty), Some(Type::Unknown))) =>
                {
                    let checked = self.checker_binding_type(binding.span.start, binding.span.end);
                    checked.unwrap_or_else(|| {
                        contextual.unwrap_or_else(|| self.infer_unannotated_arrow_param_type(arrow, index))
                    })
                }
                (_, Some(ty)) => ty,
                (BindingPattern::BindingIdentifier(_), None) => {
                    self.infer_unannotated_arrow_param_type(arrow, index)
                }
                (_, None) => self.erase_with(
                    ErasureReason::MissingAnnotation,
                    "destructured closure parameter",
                ),
            };
            // An optional parameter (`x?: T`) has type `T | undefined` inside the
            // body, matching the function-type lowering in `function_type_to_hir`
            // (`lowering/ty/annotations.rs`). Without
            // this the param looks non-nullable, so an `x === undefined` guard
            // constant-folds to `false`.
            // A default the caller may omit has the same ABI, as for a function
            // declaration; a contextual signature keeps its own parameter.
            let omittable_default = contextual_function
                .and_then(|function| function.params.get(index))
                .is_none()
                && Self::closure_param_default(param).is_some();
            let ty = if (param.optional || omittable_default)
                && !matches!(self.ctx.krate.types.get(ty), Some(Type::Optional(_)))
            {
                self.ctx.krate.types.intern(Type::Optional(ty))
            } else {
                ty
            };
            params.push(ty);
        }
        if let Some(rest) = &arrow.params.rest {
            let BindingPattern::BindingIdentifier(binding) = &rest.rest.argument else {
                return Err(SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "destructured rest closure parameters need closure-body lowering",
                ));
            };
            let rest_index = arrow.params.items.len();
            // The rest parameter's type: its annotation; else the tuple of the
            // contextual signature's remaining parameters when that signature
            // has none of its own (TypeScript's rule, and the slot ABI the
            // closure is adapted into); else the checker's type for the
            // binding. A fixed-width tuple becomes fixed parameters
            // (`lowering::tuple_rest`).
            let annotated = rest
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?;
            let contextual_tuple = contextual_function
                .filter(|function| function.rest.is_none())
                .map(|function| {
                    let elements = function.params.get(rest_index..).unwrap_or_default().to_vec();
                    self.ctx.krate.types.intern(Type::Tuple(elements))
                });
            // A tuple the SOURCE did not write is taken only when every element
            // keeps a type: splitting an erased context (`(x: any, y: any) =>`)
            // into one erased parameter per element types nothing and erases
            // more values than the one list did.
            let typed_tuple = |builder: &Self, ty: smelt_hir::TypeId| {
                builder.fixed_tuple_rest_elements(ty).is_some_and(|elements| {
                    elements.iter().all(|element| !builder.type_contains_unknown(*element))
                })
            };
            let tuple_ty = match annotated {
                Some(ty) => self.fixed_tuple_rest_elements(ty).map(|_| ty),
                None => contextual_tuple
                    .filter(|ty| typed_tuple(self, *ty))
                    .or_else(|| {
                        if contextual_function.is_some() {
                            return None;
                        }
                        self.checker_binding_type(binding.span.start, binding.span.end)
                            .filter(|ty| typed_tuple(self, *ty))
                    }),
            };
            if let Some(tuple_ty) = tuple_ty
                && let Some(elements) = self.fixed_tuple_rest_elements(tuple_ty)
            {
                self.tuple_rest_arrows
                    .insert((arrow.params.span.start, arrow.params.span.end), tuple_ty);
                params.extend(elements);
                return Ok(params);
            }
            self.tuple_rest_arrows
                .remove(&(arrow.params.span.start, arrow.params.span.end));
            let ty = annotated
                .or_else(|| {
                    contextual_function.map(|function| {
                        if function.rest == Some(rest_index)
                            && let Some(rest_ty) = function.params.get(rest_index)
                        {
                            return *rest_ty;
                        }
                        // The element type is the union of the remaining
                        // parameters' types, FLAT: a parameter that is itself
                        // a union (`detail: number | string`) contributes its
                        // arms, as TypeScript's unions are flat. A nested
                        // union cannot be a concrete Rust enum, so leaving it
                        // nested erased the whole rest list.
                        let mut item_tys = Vec::new();
                        for param_ty in function.params.iter().skip(rest_index).copied() {
                            for arm in self.flatten_union_member_types(param_ty) {
                                if !item_tys.contains(&arm) {
                                    item_tys.push(arm);
                                }
                            }
                        }
                        let item_ty = match item_tys.as_slice() {
                            [single] => *single,
                            [] => self.erase_with(
                                ErasureReason::MissingAnnotation,
                                "closure rest parameter",
                            ),
                            _ => self.ctx.krate.types.intern(Type::Union(item_tys)),
                        };
                        self.ctx.krate.types.intern(Type::List(item_ty))
                    })
                })
                .unwrap_or_else(|| {
                    let item_ty = self.erase_with(
                        ErasureReason::MissingAnnotation,
                        "closure rest parameter",
                    );
                    self.ctx.krate.types.intern(Type::List(item_ty))
                });
            let Ok((ty, _)) = self.rest_param_array_type(ty) else {
                return Err(SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "rest closure parameter type must be an array type",
                ));
            };
            params.push(ty);
        }
        Ok(params)
    }

    /// Resolve the contextual type for a *fixed* arrow parameter at `index`
    /// against a callback signature that may end in a rest parameter.
    ///
    /// A user callback can spell fewer, fixed parameters than a variadic
    /// contextual signature (e.g. `(item, item2, item3)` against
    /// `(...args: T[]) => R`, as in `unzipWith`). A fixed parameter that lands
    /// at or past the signature's rest position takes the rest's *element*
    /// type, not the rest list/tuple type itself — otherwise `item` would be
    /// typed `T[]` and arithmetic on it fails (E0369/E0308). Parameters before
    /// the rest position resolve positionally as before.
    fn contextual_param_type_at(
        &self,
        function: &FunctionType,
        index: usize,
    ) -> Option<smelt_hir::TypeId> {
        if let Some(rest_index) = function.rest
            && index >= rest_index
            && let Some(rest_ty) = function.params.get(rest_index).copied()
        {
            return match self.ctx.krate.types.get(rest_ty) {
                Some(Type::List(item)) => Some(*item),
                Some(Type::Tuple(items)) => items
                    .get(index - rest_index)
                    .copied()
                    .or_else(|| items.last().copied()),
                _ => Some(rest_ty),
            };
        }
        function.params.get(index).copied()
    }

    /// Record a `MissingAnnotation` erasure when a closure lowered with no
    /// return annotation and no contextual return type still returns `unknown`
    /// (or `Promise<unknown>`).
    ///
    /// Such closures are lowered with the `unknown` "no hint" sentinel so the
    /// body's own returns can be inferred; only when that inference fails is
    /// the return value actually erased, and that is recorded here at the
    /// current anchor. `closure` must be the closure expression in `body`.
    pub(in crate::lowering) fn record_unhinted_closure_return(
        &mut self,
        closure: smelt_hir::ExprId,
        body: &Body,
    ) {
        let return_ty = match self.ctx.krate.types.get(Self::expr_ty(body, closure)) {
            Some(Type::Function(function)) => function.return_ty,
            _ => return,
        };
        let erased = match self.ctx.krate.types.get(return_ty) {
            Some(Type::Unknown) => true,
            Some(Type::Future(inner)) => {
                matches!(self.ctx.krate.types.get(*inner), Some(Type::Unknown))
            }
            _ => false,
        };
        if erased {
            let _ = self.erase_with(ErasureReason::MissingAnnotation, "closure return");
        }
    }

    /// Infer a conservative type for an unannotated arrow parameter.
    ///
    /// TypeScript normally gets these from contextual typing. When Smelt loses
    /// that context through imported generic helpers, arithmetic use inside the
    /// callback is still enough to recover a numeric parameter; other cases use
    /// `unknown` so typed library code can keep lowering.
    pub(in crate::lowering) fn infer_unannotated_arrow_param_type(
        &mut self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        index: usize,
    ) -> smelt_hir::TypeId {
        let Some(param_name) = arrow
            .params
            .items
            .get(index)
            .and_then(|param| Self::simple_binding_pattern_name(&param.pattern))
        else {
            return self.erase_with(ErasureReason::MissingAnnotation, "closure parameter");
        };
        if self.arrow_param_used_as_number(arrow, param_name) {
            self.ctx.krate.types.intern(Type::Float)
        } else {
            self.erase_with(ErasureReason::MissingAnnotation, param_name)
        }
    }

    /// Return a simple identifier name from a binding or defaulted binding pattern.
    pub(in crate::lowering) fn simple_binding_pattern_name<'a>(pattern: &'a BindingPattern<'a>) -> Option<&'a str> {
        match pattern {
            BindingPattern::BindingIdentifier(binding) => Some(binding.name.as_str()),
            BindingPattern::AssignmentPattern(assign) => {
                Self::simple_binding_pattern_name(&assign.left)
            }
            _ => None,
        }
    }

    /// Return true when an arrow parameter participates in arithmetic.
    pub(in crate::lowering) fn arrow_param_used_as_number(
        &self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        param_name: &str,
    ) -> bool {
        self.arrow_return_expression(arrow)
            .is_ok_and(|expr| Self::expression_uses_identifier_in_arithmetic(expr, param_name))
    }

    /// Scan an expression for arithmetic involving a named identifier.
    pub(in crate::lowering) fn expression_uses_identifier_in_arithmetic(expression: &Expression<'_>, name: &str) -> bool {
        match expression {
            Expression::BinaryExpression(binary)
                if matches!(
                    binary.operator,
                    BinaryOperator::Subtraction
                        | BinaryOperator::Multiplication
                        | BinaryOperator::Division
                        | BinaryOperator::Remainder
                        | BinaryOperator::Exponential
                ) =>
            {
                Self::expression_contains_identifier(&binary.left, name)
                    || Self::expression_contains_identifier(&binary.right, name)
            }
            Expression::BinaryExpression(binary) => {
                Self::expression_uses_identifier_in_arithmetic(&binary.left, name)
                    || Self::expression_uses_identifier_in_arithmetic(&binary.right, name)
            }
            Expression::NewExpression(new_expr) => {
                matches!(
                    &new_expr.callee,
                    Expression::Identifier(identifier)
                        if Self::is_ts_stdlib_class_name(
                            identifier.name.as_str(),
                            smelt_stdlib::StdlibClass::Date
                        )
                )
                    && new_expr.arguments.iter().any(|argument| {
                        argument
                            .as_expression()
                            .is_some_and(|expr| Self::expression_contains_identifier(expr, name))
                    })
            }
            Expression::ParenthesizedExpression(parenthesized) => {
                Self::expression_uses_identifier_in_arithmetic(&parenthesized.expression, name)
            }
            Expression::TSAsExpression(as_expr) => {
                Self::expression_uses_identifier_in_arithmetic(&as_expr.expression, name)
            }
            _ => false,
        }
    }

    /// Return true when an expression contains a named identifier.
    pub(in crate::lowering) fn expression_contains_identifier(expression: &Expression<'_>, name: &str) -> bool {
        match expression {
            Expression::Identifier(identifier) => identifier.name == name,
            Expression::BinaryExpression(binary) => {
                Self::expression_contains_identifier(&binary.left, name)
                    || Self::expression_contains_identifier(&binary.right, name)
            }
            Expression::ParenthesizedExpression(parenthesized) => {
                Self::expression_contains_identifier(&parenthesized.expression, name)
            }
            Expression::TSAsExpression(as_expr) => {
                Self::expression_contains_identifier(&as_expr.expression, name)
            }
            _ => false,
        }
    }

    /// Lower a local arrow function through a real HIR closure body.
    ///
    /// Async arrows use the same body lowering as async function items: the
    /// closure body is lowered with `fn_ctx.is_async` enabled, receives the
    /// contextual return type for return-expression hints, and records async
    /// state metadata after await expressions have been collected.
    ///
    /// This is a thin wrapper over [`Self::closure_body_expr_from_parts`], which
    /// carries the shared parameter-binding, capture-collection and closure-
    /// emission logic used by both arrow and `function` expression callbacks.
    pub(in crate::lowering) fn arrow_closure_body_expr(
        &mut self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        param_tys: &[smelt_hir::TypeId],
        return_ty: smelt_hir::TypeId,
        outer_body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(arrow.span.start, arrow.span.end);
        let body_kind = if arrow.is_expression() {
            ClosureBodyKind::ArrowExpression(arrow)
        } else {
            ClosureBodyKind::Statements(arrow_block_statements(arrow))
        };
        let declares_erased_return = self.declares_erased_return(arrow.return_type.as_deref(), return_ty)?;
        self.closure_body_expr_from_parts(
            &arrow.params,
            body_kind,
            BodyKind::Arrow,
            arrow.r#async,
            span,
            param_tys,
            return_ty,
            declares_erased_return,
            outer_body,
        )
    }

    /// Lower a `function (...) { ... }` expression callback through a real HIR
    /// closure body.
    ///
    /// Function expressions are the non-arrow sibling of the callback surface
    /// accepted by array methods. They always have a block body (never an
    /// expression body) and reuse the same shared closure-body machinery as
    /// arrow callbacks, so a `function`-form callback is lowered identically to
    /// the equivalent arrow. The callback's own `arguments` binding is not modeled here; array-method callbacks that
    /// reference `arguments` are rejected by the general body lowering as usual.
    pub(in crate::lowering) fn function_closure_body_expr(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
        param_tys: &[smelt_hir::TypeId],
        return_ty: smelt_hir::TypeId,
        outer_body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(function.span.start, function.span.end);
        let Some(function_body) = &function.body else {
            return Err(SmeltError::unsupported(
                span,
                "function expression callbacks must have a body",
            ));
        };
        let declares_erased_return = self.declares_erased_return(function.return_type.as_deref(), return_ty)?;
        self.closure_body_expr_from_parts(
            &function.params,
            ClosureBodyKind::Statements(&function_body.statements),
            BodyKind::Function,
            function.r#async,
            span,
            param_tys,
            return_ty,
            declares_erased_return,
            outer_body,
        )
    }

    /// Whether the source itself declares `any`/`unknown` as this closure's return.
    ///
    /// `Unknown` is also closure-body lowering's "no hint" sentinel, which asks it
    /// to infer the return from the body. A declared return is the source's answer
    /// and is not inferred over: `(v: any): any => null` hands its caller the value
    /// `null` at an erased type, where inference would type it `void`.
    fn declares_erased_return(
        &mut self,
        annotation: Option<&oxc::ast::ast::TSTypeAnnotation<'_>>,
        return_ty: smelt_hir::TypeId,
    ) -> Result<bool, SmeltError> {
        let Some(annotation) = annotation else {
            return Ok(false);
        };
        if self.ctx.krate.types.get(return_ty) != Some(&Type::Unknown) {
            return Ok(false);
        }
        let declared = self.ts_type_to_hir(&annotation.type_annotation)?;
        Ok(self.ctx.krate.types.get(declared) == Some(&Type::Unknown))
    }

    /// The default a closure parameter applies when its argument is omitted.
    ///
    /// A default equal to the type's zero value needs none: an omitted argument
    /// already arrives as that value (see `initializer_is_type_zero_default`).
    fn closure_param_default<'src>(
        param: &'src oxc::ast::ast::FormalParameter<'src>,
    ) -> Option<&'src Expression<'src>> {
        if !matches!(param.pattern, BindingPattern::BindingIdentifier(_)) {
            return None;
        }
        param
            .initializer
            .as_deref()
            .filter(|initializer| !Self::initializer_is_type_zero_default(initializer))
    }

    /// The type a closure body's `return` statements join to, if any.
    ///
    /// Used when the caller had no return type to give a block-bodied callback
    /// (see `infer_block_return`): the body's own returns are the answer the
    /// source wrote, joined by [`Self::joined_inferred_type`].
    ///
    /// This is asked only of a body that cannot fall off its end, so one with no
    /// `return` at all only throws: it answers `never`. `None` is a bare
    /// `return;`, which says nothing about a value, so the caller keeps its
    /// fallback.
    fn inferred_block_return_ty(&mut self, closure_body: &Body) -> Option<smelt_hir::TypeId> {
        let mut returned = Vec::new();
        for stmt in &closure_body.stmts {
            let Stmt::Return(value) = stmt else {
                continue;
            };
            returned.push(Self::expr_ty(closure_body, (*value)?));
        }
        if returned.is_empty() {
            return Some(self.ctx.krate.types.intern(Type::Never));
        }
        Some(self.joined_inferred_type(returned))
    }

    /// Lower a callback closure body from its constituent parts.
    ///
    /// Shared implementation behind [`Self::arrow_closure_body_expr`] and
    /// [`Self::function_closure_body_expr`]. It binds the formal parameters into
    /// closure locals, collects the captured outer locals referenced by the
    /// body, lowers the body (an arrow expression body or a statement block) and
    /// emits the closure expression with the inferred/annotated return type.
    ///
    /// `body_kind` selects the body form: [`ClosureBodyKind::ArrowExpression`]
    /// lowers the single expression through the return-expression hint path (and
    /// may infer the return type when it is left `Unknown`), while
    /// [`ClosureBodyKind::Statements`] lowers a statement block — the form used
    /// by block-bodied arrows and by `function` expression callbacks.
    #[expect(
        clippy::too_many_arguments,
        reason = "shared closure-body lowering threads the params, body form, async flag, span and both contextual types through one call"
    )]
    fn closure_body_expr_from_parts<'src>(
        &mut self,
        params: &'src oxc::ast::ast::FormalParameters<'src>,
        body_kind: ClosureBodyKind<'_, '_>,
        kind: BodyKind,
        is_async: bool,
        span: Span,
        param_tys: &[smelt_hir::TypeId],
        return_ty: smelt_hir::TypeId,
        declares_erased_return: bool,
        outer_body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let is_expression_body = matches!(body_kind, ClosureBodyKind::ArrowExpression(_));
        let mut closure_body = Body::new(None, span);
        let mut closure_params = Vec::new();
        let mut param_names = HashSet::new();
        let mut saved_locals = Vec::new();
        let mut defaulted_params = Vec::new();

        for (index, param) in params.items.iter().enumerate() {
            let ty = param_tys.get(index).copied().ok_or_else(|| {
                SmeltError::unsupported(
                    self.span(param.span.start, param.span.end),
                    "closure parameter count does not match its type",
                )
            })?;
            let (symbol, param_span) = match &param.pattern {
                BindingPattern::BindingIdentifier(binding) => (
                    self.intern_source_name(binding.name.as_str()),
                    self.span(binding.span.start, binding.span.end),
                ),
                _ => (
                    self.synthetic_param_symbol(index),
                    self.span(param.span.start, param.span.end),
                ),
            };
            let local = closure_body.push_local(LocalDecl {
                name: Some(symbol),
                ty,
                mutable: false,
                span: param_span,
            });
            closure_body.params.push(local);
            closure_params.push(Param {
                name: symbol,
                local,
                ty,
                span: param_span,
            });
            match &param.pattern {
                BindingPattern::BindingIdentifier(binding) => {
                    param_names.insert(binding.name.as_str().to_owned());
                    saved_locals.push((
                        binding.name.as_str().to_owned(),
                        self.scope.bind(binding.name.as_str().to_owned(), local),
                    ));
                    if matches!(self.ctx.krate.types.get(ty), Some(Type::Optional(_)))
                        && let Some(initializer) = Self::closure_param_default(param)
                    {
                        defaulted_params.push((
                            local,
                            ty,
                            initializer,
                            symbol,
                            binding.name.to_string(),
                            param_span,
                        ));
                    }
                }
                pattern => {
                    let mut names = Vec::new();
                    Self::binding_pattern_names(pattern, &mut names);
                    for name in &names {
                        param_names.insert(name.clone());
                        saved_locals.push((name.clone(), self.scope.lookup(name.as_str())));
                    }
                    let value = closure_body.push_expr(Expr {
                        kind: ExprKind::Local(local),
                        ty,
                        span: param_span,
                    });
                    let root = closure_body.root;
                    self.binding_declaration(
                        pattern,
                        Some(value),
                        Some(ty),
                        false,
                        &mut closure_body,
                        root,
                    )?;
                }
            }
        }

        let tuple_rest = self
            .tuple_rest_arrows
            .get(&(params.span.start, params.span.end))
            .copied();
        if let Some(rest) = &params.rest
            && let Some(tuple_ty) = tuple_rest
        {
            // A fixed-width tuple rest: the trailing parameter types are its
            // elements, bound as the tuple (`lowering::tuple_rest`).
            let BindingPattern::BindingIdentifier(binding) = &rest.rest.argument else {
                return Err(SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "destructured rest closure parameters need closure-body lowering",
                ));
            };
            let elements = self.fixed_tuple_rest_elements(tuple_ty).unwrap_or_default();
            let span = self.span(binding.span.start, binding.span.end);
            let previous = self.scope.lookup(binding.name.as_str());
            for param in
                self.bind_tuple_rest_params(binding.name.as_str(), tuple_ty, &elements, span, &mut closure_body)
            {
                closure_params.push(Param {
                    name: param.name,
                    local: param.local,
                    ty: param.ty,
                    span,
                });
            }
            param_names.insert(binding.name.as_str().to_owned());
            saved_locals.push((binding.name.as_str().to_owned(), previous));
        } else if let Some(rest) = &params.rest {
            let BindingPattern::BindingIdentifier(binding) = &rest.rest.argument else {
                return Err(SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "destructured rest closure parameters need closure-body lowering",
                ));
            };
            let ty = param_tys.last().copied().ok_or_else(|| {
                SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "rest closure parameter count does not match its type",
                )
            })?;
            let symbol = self.intern_source_name(binding.name.as_str());
            let local = closure_body.push_local(LocalDecl {
                name: Some(symbol),
                ty,
                mutable: false,
                span: self.span(binding.span.start, binding.span.end),
            });
            closure_body.params.push(local);
            closure_params.push(Param {
                name: symbol,
                local,
                ty,
                span: self.span(binding.span.start, binding.span.end),
            });
            param_names.insert(binding.name.as_str().to_owned());
            saved_locals.push((
                binding.name.as_str().to_owned(),
                self.scope.bind(binding.name.as_str().to_owned(), local),
            ));
        }

        // JavaScript's `Function.length`: the parameters before the first omittable one.
        let required_params = Some(
            params
                .items
                .iter()
                .position(|param| param.optional || Self::formal_parameter_has_default(param))
                .unwrap_or(params.items.len()),
        );
        let mut capture_names = Vec::new();
        for (_, _, initializer, ..) in &defaulted_params {
            self.collect_expression_capture_names(initializer, &param_names, &mut capture_names);
        }
        match body_kind {
            ClosureBodyKind::ArrowExpression(arrow) => {
                let return_expression = self.arrow_return_expression(arrow)?;
                self.collect_expression_capture_names(
                    return_expression,
                    &param_names,
                    &mut capture_names,
                );
            }
            ClosureBodyKind::Statements(statements) => {
                for statement in statements {
                    self.collect_statement_capture_names(
                        statement,
                        &param_names,
                        &mut capture_names,
                    );
                }
            }
        }
        capture_names.sort();
        capture_names.dedup();

        let mut captures = Vec::new();
        // A capture the source never reassigns keeps the facts proven around the
        // closure, as TypeScript's own control flow does: the type the guard
        // established still holds whenever the closure runs.
        let mut captured_narrowings = HashMap::new();
        for name in capture_names {
            let Some(source_local) = saved_locals
                .iter()
                .find_map(|(saved_name, prior)| (saved_name == &name).then_some(*prior).flatten())
                .or_else(|| self.scope.lookup(name.as_str()))
            else {
                continue;
            };
            let Some(source_decl) = outer_body.local_at(source_local)
            else {
                continue;
            };
            let symbol = source_decl
                .name
                .unwrap_or_else(|| self.ctx.krate.symbols.intern(name.as_str()));
            let body_local = closure_body.push_local(LocalDecl {
                name: Some(symbol),
                ty: source_decl.ty,
                mutable: source_decl.mutable,
                span: source_decl.span,
            });
            if !source_decl.mutable
                && let Some(narrowed) = self.narrowed_type(name.as_str())
            {
                captured_narrowings.insert(name.clone(), narrowed);
            }
            saved_locals.push((name.clone(), self.scope.bind(name, body_local)));
            captures.push(ClosureCapture {
                source_local,
                body_local: Some(body_local),
                symbol,
                ty: source_decl.ty,
                mode: CaptureMode::ByRef,
            });
        }

        let infer_expression_return = is_expression_body
            && !declares_erased_return
            && matches!(self.ctx.krate.types.get(return_ty), Some(Type::Unknown));
        // The same inference for a BLOCK-bodied callback.
        //
        // `return_ty` here is the CALLER's fallback, and for `map` that is
        // `Unknown` — the element type of the mapped list is exactly what the
        // callback is supposed to answer, so the caller has nothing better to
        // offer. An expression-bodied arrow already inferred its own type; a
        // block-bodied one keeping the fallback would be typed `-> Unknown`, and
        // its `map` would produce a `List<Unknown>`.
        //
        // That is how Hono's `buildRegExpStr` got a `List<Unknown>` out of a
        // `map` whose every arm is a string, and the erasure only surfaced two
        // frames later as `list unshift item must match the list element type`
        // (H54). Reading the body's own returns is what the source says.
        // ... and only when the body cannot fall off its end.
        //
        // A body that can reach its closing brace without a `return` answers
        // `undefined` on that path, which is NOT part of the join of its
        // explicit returns. Inferring from the returns alone typed such a
        // callback as if the fall-through could not happen, and for a
        // customizer protocol that path is the whole contract: es-toolkit's
        // `mergeWith` customizer returns a value to override and falls through
        // to mean "not handled". Eight of its tests went red on the first
        // version of this inference, which is what taught the condition.
        //
        // `statement_terminates` is the same conservative test the switch
        // lowering uses for "can control reach the next case": a `return` or
        // `throw`, an `if` whose every arm terminates, a `try` whose block and
        // handler both do, and a `switch` with a `default` whose every clause
        // terminates. It still says NO for shapes that do terminate but need
        // real flow analysis to prove it (a `while (true)`, a `switch` without a
        // `default` that nonetheless covers its scrutinee), and a `no` here only
        // means the caller's fallback is kept — the pre-existing behaviour.
        let body_always_returns = match &body_kind {
            ClosureBodyKind::ArrowExpression(_) => false,
            ClosureBodyKind::Statements(statements) => {
                statements.iter().any(|statement| statement_terminates(statement))
            }
        };
        let infer_block_return = !is_expression_body
            && body_always_returns
            && !declares_erased_return
            && matches!(self.ctx.krate.types.get(return_ty), Some(Type::Unknown));
        // A return HINT of `Unknown` erases the returned value on the way out
        // (`return_statement_value_hint`), which would make the inference below
        // read back the very `Unknown` it is trying to replace. With no hint the
        // returns keep their own types and the join is the source's answer.
        let ctx_return_ty = if infer_block_return {
            None
        } else {
            Some(return_ty)
        };
        // A closure body is a fresh body: no statement block or postfix-update
        // deferral of the enclosing statement crosses into it (`FnCtx::nested`).
        let ctx = self.nested_fn_ctx(kind, is_async, ctx_return_ty);
        // `LocalId`s are per body, so the callable-write registry is too.
        let scoped_state = ScopeGroups::NARROWINGS.with(ScopeGroups::CALLABLE_PROPS);
        let (lowering_result, actual_return_ty) =
            self.with_nested_body(ctx, scoped_state, |this, _| {
                this.scope.push_narrowing_scope(captured_narrowings);
                let mut actual_return_ty = return_ty;
                // A binding declared inside a function body is not in scope outside it.
                //
                // The `saved_locals` bookkeeping above restores only the names THIS
                // lowering bound (the parameters and the captures); every `const`/`let`
                // the body's own statements declare — and every slot
                // `predeclare_forward_referenced_locals` reserves for one — stayed bound
                // in the enclosing scope after the closure was finished. A later SIBLING
                // closure then saw them, and because a `LocalId` is an index into the
                // body that owns it, a stale one silently named a DIFFERENT slot of the
                // new body: `local_arrow_existing_body_local` can only check that the
                // index exists. Two `test(() => { const add = .. })` closures in one
                // `describe` therefore aliased their locals onto each other's slots
                // (radash `curry.test.ts`, where the second test's `add` and `repeatX`
                // shared one slot and the emitted callback adapter was handed the other
                // binding's type).
                //
                // Snapshotting here — after the parameters and captures are bound, so
                // the body still sees everything it legitimately captures — and
                // restoring once the body is lowered drops exactly the bindings the body
                // introduced, and nothing else.
                let bindings_before_body = this.scope.snapshot_bindings();
                let predeclare_result = this
                    .apply_parameter_defaults(defaulted_params, &mut closure_body)
                    .and_then(|()| match body_kind {
                        ClosureBodyKind::ArrowExpression(_) => Ok(()),
                        ClosureBodyKind::Statements(statements) => this
                            .predeclare_local_function_declarations(statements, &mut closure_body)
                            .and_then(|()| {
                                this.predeclare_forward_referenced_locals(
                                    statements,
                                    &mut closure_body,
                                )
                            }),
                    });
                let lowering_result = if let Err(error) = predeclare_result {
                    Err(error)
                } else {
                    match body_kind {
                        ClosureBodyKind::ArrowExpression(arrow) => {
                            match this.arrow_return_expression(arrow) {
                                Ok(return_expression) => {
                                    // For an async expression-bodied arrow the closure
                                    // return type is `Promise<Inner>` (`Type::Future`),
                                    // but the body expression itself produces `Inner` —
                                    // the async wrapper adds the promise. Hint the body
                                    // at the awaited inner type (via
                                    // `return_statement_value_hint`, which unwraps one
                                    // `Future` layer for async bodies) so an array or
                                    // tuple literal body keeps its own value type instead
                                    // of being coerced to the future type.
                                    let hint = if infer_expression_return {
                                        None
                                    } else {
                                        this.return_statement_value_hint()
                                    };
                                    this.expression_with_hint(return_expression, &mut closure_body, hint)
                                        .map(|value| {
                                            if infer_expression_return {
                                                actual_return_ty = Self::expr_ty(&closure_body, value);
                                            }
                                            closure_body.push_stmt(Stmt::Return(Some(value)));
                                        })
                                }
                                Err(error) => Err(error),
                            }
                        }
                        ClosureBodyKind::Statements(statements) => {
                            let mut result = Ok(());
                            for statement in crate::lowering::hoisting::hoisted_statements(statements) {
                                if let Err(error) = this.statement(statement, &mut closure_body) {
                                    result = Err(error);
                                    break;
                                }
                            }
                            result
                        }
                    }
                };
                if infer_block_return
                    && lowering_result.is_ok()
                    && let Some(inferred) = this.inferred_block_return_ty(&closure_body)
                {
                    actual_return_ty = inferred;
                }
                if is_async {
                    closure_body.finish_async();
                }
                this.scope.restore_bindings(bindings_before_body);
                let mut unconsumed = Vec::new();
                this.report_unconsumed_callable_props(&mut unconsumed);
                let lowering_result = match unconsumed.into_iter().next() {
                    Some(error) if lowering_result.is_ok() => Err(error),
                    _ => lowering_result,
                };
                (lowering_result, actual_return_ty)
            });
        for (name, prior) in saved_locals.into_iter().rev() {
            if let Some(local) = prior {
                self.scope.bind(name, local);
            } else {
                self.scope.unbind(name.as_str());
            }
        }
        lowering_result?;
        let may_throw = Self::body_contains_uncaught_throw(&closure_body);
        let body_id = self.ctx.krate.push_body(closure_body);
        let rest_index = params
            .rest
            .as_ref()
            .filter(|_| tuple_rest.is_none())
            .map(|_| params.items.len());
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: param_tys.to_vec(),
            rest: rest_index,
            required_params,
            mutable_params: Vec::new(),
            return_ty: actual_return_ty,
            is_async,
            may_throw,
        }));
        Ok(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: closure_params,
                rest: rest_index,
                required_params,
                return_ty: actual_return_ty,
                captures,
                body: body_id,
                function_item: None,
                span,
            }),
            ty: closure_ty,
            span,
        }))
    }

    /// Returns whether a lowered body directly contains an `await` expression.
    ///
    /// Only this body's own expression arena is scanned; awaits inside nested
    /// closure bodies live in their own [`Body`] arenas and belong to those
    /// closures, not to this one. A body that directly awaits must itself be
    /// async — HIR validation rejects `await` outside an async function. Some
    /// lowerings (notably the Vitest `expect(...).rejects.toThrow(...)` async
    /// matcher) desugar into an inline `await` even when the source callback was
    /// not spelled `async`, because in JavaScript the test returns the pending
    /// promise for the framework to await; inlining that await makes the body
    /// genuinely async, so callers use this to mark the function accordingly.
    pub(in crate::lowering) fn body_contains_await(body: &Body) -> bool {
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Await(_)))
    }

    /// Returns whether a lowered closure body can throw past its own boundary.
    ///
    /// Try bodies with a catch handler are considered locally handled here: the
    /// MIR lowering attaches exception edges for nested calls and explicit
    /// throws, so only statements that can escape the closure need to widen the
    /// closure ABI to `Result`.
    pub(in crate::lowering) fn body_contains_uncaught_throw(body: &Body) -> bool {
        Self::block_contains_uncaught_throw(body, body.root)
    }

    /// Returns whether a HIR block contains a throw not protected by catch.
    pub(in crate::lowering) fn block_contains_uncaught_throw(body: &Body, block: smelt_hir::BlockId) -> bool {
        let Some(block_data) = body.block_at(block)
        else {
            return false;
        };
        block_data.stmts.iter().any(|stmt_id| {
            let Some(stmt) = body.stmt_at(*stmt_id)
            else {
                return false;
            };
            Self::stmt_contains_uncaught_throw(body, stmt)
        })
    }

    /// Returns whether a HIR statement can throw out of the surrounding body.
    pub(in crate::lowering) fn stmt_contains_uncaught_throw(body: &Body, stmt: &Stmt) -> bool {
        match stmt {
            Stmt::Throw(_) => true,
            Stmt::If {
                then_block,
                else_block,
                ..
            } => {
                Self::block_contains_uncaught_throw(body, *then_block)
                    || else_block
                        .is_some_and(|block| Self::block_contains_uncaught_throw(body, block))
            }
            Stmt::While {
                body: loop_body, ..
            }
            | Stmt::WhileUpdate {
                body: loop_body, ..
            }
            | Stmt::WhileUpdateBlock {
                body: loop_body, ..
            }
            | Stmt::For {
                body: loop_body, ..
            } => Self::block_contains_uncaught_throw(body, *loop_body),
            Stmt::Match { arms, default, .. } => {
                arms.iter()
                    .any(|arm| Self::block_contains_uncaught_throw(body, arm.body))
                    || default.is_some_and(|block| Self::block_contains_uncaught_throw(body, block))
            }
            Stmt::TryCatch {
                body: try_body,
                catch_body,
                finally_body,
                ..
            } => {
                let try_escapes = if catch_body.is_none() {
                    Self::block_contains_uncaught_throw(body, *try_body)
                } else {
                    false
                };
                let catch_escapes = catch_body
                    .is_some_and(|block| Self::block_contains_uncaught_throw(body, block));
                let finally_escapes = finally_body
                    .is_some_and(|block| Self::block_contains_uncaught_throw(body, block));
                try_escapes || catch_escapes || finally_escapes
            }
            Stmt::Let { .. }
            | Stmt::Assign { .. }
            | Stmt::Expr(_)
            | Stmt::Return(_)
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabelStart { .. }
            | Stmt::LabelEnd(_)
            | Stmt::BreakLabel(_)
            | Stmt::ContinueLabel(_) => false,
        }
    }

    /// Lower an arrow function expression as a first-class closure value.
    pub(in crate::lowering) fn arrow_function_expression(
        &mut self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        self.arrow_function_expression_with_hint(arrow, body, None)
    }

    /// Lower an arrow function expression using an optional contextual function type.
    pub(in crate::lowering) fn arrow_function_expression_with_hint(
        &mut self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        self.push_closure_type_parameter_scope(arrow.type_parameters.as_deref())?;
        let result = (|| {
            let contextual_function = type_hint.and_then(|hint| {
                let function_hint = self.function_member_type(hint).unwrap_or(hint);
                if let Some(Type::Function(function)) = self.ctx.krate.types.get(function_hint) {
                    Some(function.clone())
                } else {
                    None
                }
            });
            let params =
                self.arrow_callback_param_types_with_hint(arrow, contextual_function.as_ref())?;
            let explicit_return_ty = arrow
                .return_type
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?;
            let contextual_return =
                self.closure_contextual_return(contextual_function.as_ref(), arrow.r#async);
            let no_return_hint = explicit_return_ty.is_none() && contextual_return.is_none();
            let mut return_ty = explicit_return_ty
                .or(contextual_return)
                // not a new erasure: `unknown` is the "no return hint" sentinel
                // of closure-body lowering (it infers block returns); a return
                // that stays erased is recorded below.
                .unwrap_or_else(|| self.unknown_type());
            if arrow.r#async
                && !matches!(self.ctx.krate.types.get(return_ty), Some(Type::Future(_)))
            {
                return_ty = self.ctx.krate.types.intern(Type::Future(return_ty));
            }
            let closure = self.arrow_closure_body_expr(arrow, &params, return_ty, body)?;
            if no_return_hint {
                self.record_unhinted_closure_return(closure, body);
            }
            Ok(closure)
        })();
        self.pop_type_parameter_scope();
        result
    }

    /// The return type a contextual signature fixes for a closure, if any.
    ///
    /// A return that mentions an unresolved type parameter or erased type is what
    /// the closure's own body is about to answer: `countBy(xs, x => x.name)` is
    /// how `K` gets its `string`, so the signature must not pre-empt the body.
    /// An `async` body answers the awaited value, not its own return, so its
    /// signature stays.
    pub(in crate::lowering) fn closure_contextual_return(
        &self,
        contextual: Option<&FunctionType>,
        is_async: bool,
    ) -> Option<smelt_hir::TypeId> {
        contextual.map(|function| function.return_ty).filter(|ty| {
            is_async || (!self.type_mentions_type_param(*ty) && !self.type_contains_unknown(*ty))
        })
    }

    /// Return the expression produced by an arrow function body.
    pub(in crate::lowering) fn arrow_return_expression<'a>(
        &self,
        arrow: &'a oxc::ast::ast::ArrowFunctionExpression<'a>,
    ) -> Result<&'a Expression<'a>, SmeltError> {
        // A concise body is the expression itself since oxc 0.147, rather than a
        // block holding one `ExpressionStatement`.
        if let Some(body_expression) = arrow.get_expression() {
            Ok(body_expression)
        } else {
            let [Statement::ReturnStatement(statement)] = arrow_block_statements(arrow) else {
                return Err(SmeltError::unsupported(
                    self.arrow_body_span(arrow),
                    "block-bodied closures currently require a single return statement",
                ));
            };
            statement.argument.as_ref().ok_or_else(|| {
                SmeltError::unsupported(
                    self.span(statement.span.start, statement.span.end),
                    "closure return statements must return a value",
                )
            })
        }
    }

    /// Collect outer identifier names referenced by an arrow body expression.
    pub(in crate::lowering) fn collect_expression_capture_names(
        &self,
        expression: &Expression<'_>,
        param_names: &HashSet<String>,
        captures: &mut Vec<String>,
    ) {
        match expression {
            Expression::ThisExpression(_) => {
                if !param_names.contains("this") && self.scope.is_bound("this") {
                    captures.push("this".to_owned());
                }
            }
            Expression::Identifier(identifier) => {
                let name = identifier.name.as_str();
                if !param_names.contains(name) && self.scope.is_bound(name) {
                    captures.push(name.to_owned());
                }
            }
            Expression::CallExpression(call) => {
                self.collect_expression_capture_names(&call.callee, param_names, captures);
                for arg in &call.arguments {
                    self.collect_argument_capture_names(arg, param_names, captures);
                }
            }
            Expression::NewExpression(new_expr) => {
                self.collect_expression_capture_names(&new_expr.callee, param_names, captures);
                for arg in &new_expr.arguments {
                    self.collect_argument_capture_names(arg, param_names, captures);
                }
            }
            Expression::ParenthesizedExpression(parenthesized) => self
                .collect_expression_capture_names(&parenthesized.expression, param_names, captures),
            Expression::BinaryExpression(binary) => {
                self.collect_expression_capture_names(&binary.left, param_names, captures);
                self.collect_expression_capture_names(&binary.right, param_names, captures);
            }
            Expression::LogicalExpression(logical) => {
                self.collect_expression_capture_names(&logical.left, param_names, captures);
                self.collect_expression_capture_names(&logical.right, param_names, captures);
            }
            Expression::UnaryExpression(unary) => {
                self.collect_expression_capture_names(&unary.argument, param_names, captures);
            }
            Expression::AwaitExpression(await_expr) => {
                self.collect_expression_capture_names(&await_expr.argument, param_names, captures);
            }
            Expression::YieldExpression(yield_expr) => {
                if let Some(argument) = &yield_expr.argument {
                    self.collect_expression_capture_names(argument, param_names, captures);
                }
            }
            Expression::AssignmentExpression(assignment) => {
                self.collect_assignment_target_capture_names(
                    &assignment.left,
                    param_names,
                    captures,
                );
                self.collect_expression_capture_names(&assignment.right, param_names, captures);
            }
            Expression::ConditionalExpression(conditional) => {
                self.collect_expression_capture_names(&conditional.test, param_names, captures);
                self.collect_expression_capture_names(
                    &conditional.consequent,
                    param_names,
                    captures,
                );
                self.collect_expression_capture_names(
                    &conditional.alternate,
                    param_names,
                    captures,
                );
            }
            Expression::TemplateLiteral(template) => {
                for template_expression in &template.expressions {
                    self.collect_expression_capture_names(
                        template_expression,
                        param_names,
                        captures,
                    );
                }
            }
            Expression::StaticMemberExpression(member) => {
                self.collect_expression_capture_names(&member.object, param_names, captures);
            }
            // An ES private field (`this.#status`) is a member expression like
            // any other: the RECEIVER is an ordinary expression that may name a
            // captured binding, and `this` is the only receiver the language
            // allows here. Leaving it out of the walk left `this` uncaptured, so
            // the closure body resolved `this` to whatever the enclosing scope
            // offered — the arrow's first parameter — and the read was typed
            // against that parameter.
            Expression::PrivateFieldExpression(member) => {
                self.collect_expression_capture_names(&member.object, param_names, captures);
            }
            Expression::ComputedMemberExpression(member) => {
                self.collect_expression_capture_names(&member.object, param_names, captures);
                self.collect_expression_capture_names(&member.expression, param_names, captures);
            }
            Expression::ObjectExpression(object) => {
                for property in &object.properties {
                    match property {
                        ObjectPropertyKind::ObjectProperty(property) => {
                            if property.computed
                                && let Some(key) = property.key.as_expression()
                            {
                                self.collect_expression_capture_names(key, param_names, captures);
                            }
                            self.collect_expression_capture_names(
                                &property.value,
                                param_names,
                                captures,
                            );
                        }
                        ObjectPropertyKind::SpreadProperty(spread) => self
                            .collect_expression_capture_names(
                                &spread.argument,
                                param_names,
                                captures,
                            ),
                    }
                }
            }
            Expression::ArrayExpression(array) => {
                for element in &array.elements {
                    match element {
                        ArrayExpressionElement::SpreadElement(spread) => self
                            .collect_expression_capture_names(
                                &spread.argument,
                                param_names,
                                captures,
                            ),
                        other => {
                            if let Some(expr) = other.as_expression() {
                                self.collect_expression_capture_names(expr, param_names, captures);
                            }
                        }
                    }
                }
            }
            Expression::ArrowFunctionExpression(arrow) => {
                let mut nested_params = param_names.clone();
                for param in &arrow.params.items {
                    if let BindingPattern::BindingIdentifier(binding) = &param.pattern {
                        nested_params.insert(binding.name.as_str().to_owned());
                    }
                }
                // A concise body carries the expression directly (oxc 0.147),
                // so it must be walked too or its captures are missed.
                if let Some(body_expression) = arrow.get_expression() {
                    self.collect_expression_capture_names(
                        body_expression,
                        &nested_params,
                        captures,
                    );
                }
                for statement in arrow_block_statements(arrow) {
                    self.collect_statement_capture_names(statement, &nested_params, captures);
                }
            }
            Expression::FunctionExpression(function) => {
                let mut nested_params = param_names.clone();
                for param in &function.params.items {
                    if let BindingPattern::BindingIdentifier(binding) = &param.pattern {
                        nested_params.insert(binding.name.as_str().to_owned());
                    }
                }
                if let Some(rest) = &function.params.rest
                    && let BindingPattern::BindingIdentifier(binding) = &rest.rest.argument
                {
                    nested_params.insert(binding.name.as_str().to_owned());
                }
                if let Some(body) = &function.body {
                    for statement in &body.statements {
                        self.collect_statement_capture_names(statement, &nested_params, captures);
                    }
                }
            }
            Expression::TSAsExpression(as_expr) => {
                self.collect_expression_capture_names(&as_expr.expression, param_names, captures);
            }
            Expression::TSTypeAssertion(assertion) => {
                self.collect_expression_capture_names(&assertion.expression, param_names, captures);
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                self.collect_expression_capture_names(&satisfies.expression, param_names, captures);
            }
            Expression::TSNonNullExpression(non_null) => {
                self.collect_expression_capture_names(&non_null.expression, param_names, captures);
            }
            Expression::ChainExpression(chain) => match &chain.expression {
                ChainElement::CallExpression(call) => {
                    self.collect_expression_capture_names(&call.callee, param_names, captures);
                    for arg in &call.arguments {
                        self.collect_argument_capture_names(arg, param_names, captures);
                    }
                }
                ChainElement::StaticMemberExpression(member) => {
                    self.collect_expression_capture_names(&member.object, param_names, captures);
                }
                ChainElement::ComputedMemberExpression(member) => {
                    self.collect_expression_capture_names(&member.object, param_names, captures);
                    self.collect_expression_capture_names(
                        &member.expression,
                        param_names,
                        captures,
                    );
                }
                ChainElement::TSNonNullExpression(non_null) => {
                    self.collect_expression_capture_names(
                        &non_null.expression,
                        param_names,
                        captures,
                    );
                }
                ChainElement::PrivateFieldExpression(member) => {
                    self.collect_expression_capture_names(&member.object, param_names, captures);
                }
            },
            Expression::UpdateExpression(update) => {
                self.collect_simple_assignment_target_capture_names(
                    &update.argument,
                    param_names,
                    captures,
                );
            }
            Expression::TaggedTemplateExpression(tagged) => {
                self.collect_expression_capture_names(&tagged.tag, param_names, captures);
                for expression in &tagged.quasi.expressions {
                    self.collect_expression_capture_names(expression, param_names, captures);
                }
            }
            Expression::TSInstantiationExpression(instantiation) => {
                self.collect_expression_capture_names(
                    &instantiation.expression,
                    param_names,
                    captures,
                );
            }
            Expression::PrivateInExpression(private_in) => {
                self.collect_expression_capture_names(&private_in.right, param_names, captures);
            }
            Expression::SequenceExpression(sequence) => {
                for sequence_expr in &sequence.expressions {
                    self.collect_expression_capture_names(sequence_expr, param_names, captures);
                }
            }
            _ => {}
        }
    }

    /// Collect captures referenced by a call/new argument, including function literals.
    fn collect_argument_capture_names(
        &self,
        argument: &Argument<'_>,
        param_names: &HashSet<String>,
        captures: &mut Vec<String>,
    ) {
        match argument {
            Argument::SpreadElement(spread) => self.collect_expression_capture_names(
                &spread.argument,
                param_names,
                captures,
            ),
            Argument::ArrowFunctionExpression(arrow) => {
                let mut nested_params = param_names.clone();
                for param in &arrow.params.items {
                    if let BindingPattern::BindingIdentifier(binding) = &param.pattern {
                        nested_params.insert(binding.name.as_str().to_owned());
                    }
                }
                // A concise body carries the expression directly (oxc 0.147),
                // so it must be walked too or its captures are missed.
                if let Some(body_expression) = arrow.get_expression() {
                    self.collect_expression_capture_names(
                        body_expression,
                        &nested_params,
                        captures,
                    );
                }
                for statement in arrow_block_statements(arrow) {
                    self.collect_statement_capture_names(statement, &nested_params, captures);
                }
            }
            Argument::FunctionExpression(function) => {
                let mut nested_params = param_names.clone();
                for param in &function.params.items {
                    if let BindingPattern::BindingIdentifier(binding) = &param.pattern {
                        nested_params.insert(binding.name.as_str().to_owned());
                    }
                }
                if let Some(rest) = &function.params.rest
                    && let BindingPattern::BindingIdentifier(binding) = &rest.rest.argument
                {
                    nested_params.insert(binding.name.as_str().to_owned());
                }
                if let Some(body) = &function.body {
                    for statement in &body.statements {
                        self.collect_statement_capture_names(
                            statement,
                            &nested_params,
                            captures,
                        );
                    }
                }
            }
            other => {
                if let Some(expression) = other.as_expression() {
                    self.collect_expression_capture_names(expression, param_names, captures);
                }
            }
        }
    }

    /// Collect captured locals referenced by a simple assignment target
    /// (the target of `x++`, `--y`, or `obj[i]++`).
    ///
    /// `++counter` and `startIndex++` in the curry/bind/after family mutate a
    /// captured enclosing local; without traversing the update target the
    /// mutated local is never recorded as a capture and the closure body fails
    /// with `unresolved identifier`.
    pub(in crate::lowering) fn collect_simple_assignment_target_capture_names(
        &self,
        target: &SimpleAssignmentTarget<'_>,
        param_names: &HashSet<String>,
        captures: &mut Vec<String>,
    ) {
        match target {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) => {
                let name = identifier.name.as_str();
                if !param_names.contains(name) && self.scope.is_bound(name) {
                    captures.push(name.to_owned());
                }
            }
            SimpleAssignmentTarget::StaticMemberExpression(member) => {
                self.collect_expression_capture_names(&member.object, param_names, captures);
            }
            // `this.#count++` writes through a private field; see the
            // `Expression::PrivateFieldExpression` arm of
            // [`Self::collect_expression_capture_names`] for why the receiver
            // must be walked.
            SimpleAssignmentTarget::PrivateFieldExpression(member) => {
                self.collect_expression_capture_names(&member.object, param_names, captures);
            }
            SimpleAssignmentTarget::ComputedMemberExpression(member) => {
                self.collect_expression_capture_names(&member.object, param_names, captures);
                self.collect_expression_capture_names(&member.expression, param_names, captures);
            }
            _ => {}
        }
    }

    /// Collect captured locals referenced by an assignment target.
    pub(in crate::lowering) fn collect_assignment_target_capture_names(
        &self,
        target: &AssignmentTarget<'_>,
        param_names: &HashSet<String>,
        captures: &mut Vec<String>,
    ) {
        match target {
            AssignmentTarget::AssignmentTargetIdentifier(identifier) => {
                let name = identifier.name.as_str();
                if !param_names.contains(name) && self.scope.is_bound(name) {
                    captures.push(name.to_owned());
                }
            }
            AssignmentTarget::ComputedMemberExpression(member) => {
                self.collect_expression_capture_names(&member.object, param_names, captures);
                self.collect_expression_capture_names(&member.expression, param_names, captures);
            }
            AssignmentTarget::StaticMemberExpression(member) => {
                self.collect_expression_capture_names(&member.object, param_names, captures);
            }
            // `this.#status = value` inside a class-field arrow: the write
            // destination's receiver is a capture like any other. Missing it
            // dropped the write entirely -- see the
            // `Expression::PrivateFieldExpression` arm of
            // [`Self::collect_expression_capture_names`].
            AssignmentTarget::PrivateFieldExpression(member) => {
                self.collect_expression_capture_names(&member.object, param_names, captures);
            }
            _ => {}
        }
    }

    /// Collect outer identifier names referenced by a block-bodied arrow statement.
    pub(in crate::lowering) fn collect_statement_capture_names(
        &self,
        statement: &Statement<'_>,
        param_names: &HashSet<String>,
        captures: &mut Vec<String>,
    ) {
        match statement {
            Statement::VariableDeclaration(decl) => {
                let mut local_names = param_names.clone();
                for declarator in &decl.declarations {
                    if let BindingPattern::BindingIdentifier(binding) = &declarator.id {
                        local_names.insert(binding.name.as_str().to_owned());
                    }
                }
                for declarator in &decl.declarations {
                    if let Some(init) = &declarator.init {
                        self.collect_expression_capture_names(init, &local_names, captures);
                    }
                }
            }
            Statement::ExpressionStatement(statement) => {
                self.collect_expression_capture_names(&statement.expression, param_names, captures);
            }
            Statement::ReturnStatement(statement) => {
                if let Some(argument) = &statement.argument {
                    self.collect_expression_capture_names(argument, param_names, captures);
                }
            }
            Statement::IfStatement(statement) => {
                self.collect_expression_capture_names(&statement.test, param_names, captures);
                self.collect_statement_capture_names(&statement.consequent, param_names, captures);
                if let Some(alternate) = &statement.alternate {
                    self.collect_statement_capture_names(alternate, param_names, captures);
                }
            }
            Statement::BlockStatement(block) => {
                for child in &block.body {
                    self.collect_statement_capture_names(child, param_names, captures);
                }
            }
            Statement::ForOfStatement(statement) => {
                self.collect_expression_capture_names(&statement.right, param_names, captures);
                let mut local_names = param_names.clone();
                Self::collect_for_left_binding_names(&statement.left, &mut local_names);
                self.collect_statement_capture_names(&statement.body, &local_names, captures);
            }
            Statement::ForInStatement(statement) => {
                self.collect_expression_capture_names(&statement.right, param_names, captures);
                let mut local_names = param_names.clone();
                Self::collect_for_left_binding_names(&statement.left, &mut local_names);
                self.collect_statement_capture_names(&statement.body, &local_names, captures);
            }
            Statement::WhileStatement(statement) => {
                self.collect_expression_capture_names(&statement.test, param_names, captures);
                self.collect_statement_capture_names(&statement.body, param_names, captures);
            }
            Statement::TryStatement(statement) => {
                for child in &statement.block.body {
                    self.collect_statement_capture_names(child, param_names, captures);
                }
                if let Some(handler) = &statement.handler {
                    let mut local_names = param_names.clone();
                    if let Some(param) = &handler.param {
                        let mut binding_names = Vec::new();
                        Self::binding_pattern_names(&param.pattern, &mut binding_names);
                        local_names.extend(binding_names);
                    }
                    for child in &handler.body.body {
                        self.collect_statement_capture_names(child, &local_names, captures);
                    }
                }
                if let Some(finalizer) = &statement.finalizer {
                    for child in &finalizer.body {
                        self.collect_statement_capture_names(child, param_names, captures);
                    }
                }
            }
            Statement::ThrowStatement(statement) => {
                self.collect_expression_capture_names(&statement.argument, param_names, captures);
            }
            Statement::ForStatement(statement) => {
                // C-style `for (let i = 0; i < n; ++i)` loops appear throughout
                // the curry/bind/partial family's returned closures. The init
                // declaration introduces loop-scoped locals (`i`); the test,
                // update, and body all reference enclosing captures
                // (`partialArgs.length`, `predicates[i]`). Thread the init's
                // declared names so the loop variable is not treated as a
                // capture while the genuinely-enclosing locals still are.
                let mut local_names = param_names.clone();
                if let Some(ForStatementInit::VariableDeclaration(decl)) = &statement.init {
                    for declarator in &decl.declarations {
                        let mut binding_names = Vec::new();
                        Self::binding_pattern_names(&declarator.id, &mut binding_names);
                        local_names.extend(binding_names);
                    }
                }
                match &statement.init {
                    Some(ForStatementInit::VariableDeclaration(decl)) => {
                        for declarator in &decl.declarations {
                            if let Some(init) = &declarator.init {
                                self.collect_expression_capture_names(
                                    init,
                                    &local_names,
                                    captures,
                                );
                            }
                        }
                    }
                    Some(init) => {
                        if let Some(expression) = init.as_expression() {
                            self.collect_expression_capture_names(
                                expression,
                                &local_names,
                                captures,
                            );
                        }
                    }
                    None => {}
                }
                if let Some(test) = &statement.test {
                    self.collect_expression_capture_names(test, &local_names, captures);
                }
                if let Some(update) = &statement.update {
                    self.collect_expression_capture_names(update, &local_names, captures);
                }
                self.collect_statement_capture_names(&statement.body, &local_names, captures);
            }
            Statement::DoWhileStatement(statement) => {
                self.collect_statement_capture_names(&statement.body, param_names, captures);
                self.collect_expression_capture_names(&statement.test, param_names, captures);
            }
            Statement::SwitchStatement(statement) => {
                self.collect_expression_capture_names(
                    &statement.discriminant,
                    param_names,
                    captures,
                );
                for case in &statement.cases {
                    if let Some(test) = &case.test {
                        self.collect_expression_capture_names(test, param_names, captures);
                    }
                    for child in &case.consequent {
                        self.collect_statement_capture_names(child, param_names, captures);
                    }
                }
            }
            Statement::LabeledStatement(labeled) => {
                self.collect_statement_capture_names(&labeled.body, param_names, captures);
            }
            Statement::FunctionDeclaration(function) => {
                // A nested `function` declaration is lowered into a closure of
                // this frame, so every enclosing binding ITS body reads is a
                // binding this frame must hold as well. Without this arm a name
                // used only inside the declaration is never captured by the
                // frame that contains it, and the declaration's own capture
                // then resolves in a further-out frame — two different values
                // collide on one source local id. Walked exactly like a
                // function EXPRESSION, plus the declaration's own name, which
                // is bound by this statement rather than captured.
                let mut nested_params = param_names.clone();
                if let Some(id) = &function.id {
                    nested_params.insert(id.name.as_str().to_owned());
                }
                for param in &function.params.items {
                    if let BindingPattern::BindingIdentifier(binding) = &param.pattern {
                        nested_params.insert(binding.name.as_str().to_owned());
                    }
                }
                if let Some(rest) = &function.params.rest
                    && let BindingPattern::BindingIdentifier(binding) = &rest.rest.argument
                {
                    nested_params.insert(binding.name.as_str().to_owned());
                }
                if let Some(body) = &function.body {
                    for statement in &body.statements {
                        self.collect_statement_capture_names(statement, &nested_params, captures);
                    }
                }
            }
            _ => {}
        }
    }

    /// Add names declared by a `for...of` or `for...in` left binding to a local name set.
    pub(in crate::lowering) fn collect_for_left_binding_names(left: &ForStatementLeft<'_>, names: &mut HashSet<String>) {
        let ForStatementLeft::VariableDeclaration(decl) = left else {
            return;
        };
        for declarator in &decl.declarations {
            let mut binding_names = Vec::new();
            Self::binding_pattern_names(&declarator.id, &mut binding_names);
            names.extend(binding_names);
        }
    }

}
