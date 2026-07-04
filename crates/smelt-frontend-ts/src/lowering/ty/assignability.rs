//! Type-assignability checks and direct string/array method lowering.
//!
//! This split of the lowering builder holds two related groups of helpers.
//! The first models the parts of TypeScript assignability that survive Smelt's
//! erased HIR types (`type_assignable_to` and its function/map/numeric
//! compatibility helpers). The second lowers a batch of direct
//! `String.prototype`/`Array.prototype` methods — padding, `charAt`,
//! `join`, and `includes` containment — into concrete HIR runtime calls,
//! coercing string/numeric/erased operand surfaces to the exact runtime types
//! the emitted calls require.

use crate::lowering::{
    Argument, Body, Expr, ExprKind, Expression, FunctionType, Literal, ModuleBuilder,
    PrimitiveCastOp, SmeltError, Span, StringPadOp, Type,
};
use oxc::span::GetSpan;

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
                    && actual_items
                        .iter()
                        .zip(expected_items.iter())
                        .all(|(actual_item, expected_item)| {
                            self.type_assignable_to_inner(
                                *actual_item,
                                *expected_item,
                                depth + 1,
                            )
                        })
            }
            (Type::Tuple(actual_items), Type::List(expected_item)) => actual_items
                .iter()
                .all(|actual_item| {
                    self.type_assignable_to_inner(*actual_item, expected_item, depth + 1)
                }),
            (Type::Dict(actual_key, actual_value), Type::Dict(expected_key, expected_value)) => {
                self.type_assignable_to_inner(actual_key, expected_key, depth + 1)
                    && self.type_assignable_to_inner(actual_value, expected_value, depth + 1)
            }
            (Type::Function(actual_fn), Type::Function(expected_fn)) => {
                Self::function_arity_assignable(&actual_fn, &expected_fn)
                    && self.function_async_assignable(&actual_fn, &expected_fn, depth)
                    && actual_fn
                        .params
                        .iter()
                        .zip(expected_fn.params.iter())
                        .all(|(actual_param, expected_param)| {
                            self.type_assignable_to_inner(
                                *expected_param,
                                *actual_param,
                                depth + 1,
                            )
                        })
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
                    && actual_args
                        .iter()
                        .zip(expected_args.iter())
                        .all(|(actual_arg, expected_arg)| {
                            self.type_assignable_to_inner(*actual_arg, *expected_arg, depth + 1)
                        })
            }
            (actual_ty, expected_ty) => actual_ty == expected_ty,
        }
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
    pub(in crate::lowering) fn function_arity_assignable(actual: &FunctionType, expected: &FunctionType) -> bool {
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
        if self.ctx.krate.types.get(expected) == Some(&Type::String)
            && let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(actual)
            && self.ctx.krate.symbols.get(*name) == Some("PropertyKey")
        {
            return true;
        }
        if let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(expected)
            && self.ctx.krate.symbols.get(*name) == Some("PropertyKey")
        {
            return !matches!(self.ctx.krate.types.get(actual), Some(Type::None));
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
        match self.ctx.krate.types.get(self.type_param_constraint_or_self(ty)) {
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
        match self.ctx.krate.types.get(self.type_param_constraint_or_self(ty)) {
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

    /// Return whether a type comes from an erased JavaScript surface.
    pub(in crate::lowering) fn erased_or_union_surface(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(self.type_param_constraint_or_self(ty)) {
            Some(Type::Unknown | Type::Class { .. } | Type::TypeParam { .. }) => true,
            Some(Type::Optional(item)) => self.erased_or_union_surface(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.erased_or_union_surface(item)),
            _ => false,
        }
    }

    /// Coerce a padding operand (receiver or pad string) to `Type::String`.
    ///
    /// `String.prototype.padStart`/`padEnd` receivers and pad arguments commonly
    /// arrive as `toString(...)` returns or erased/optional surfaces. A value
    /// already typed `String` is returned unchanged; a string-compatible surface
    /// is converted with a JS `ToString` cast so the runtime padding sees a
    /// concrete string.
    pub(in crate::lowering) fn coerce_pad_string_operand(
        &mut self,
        operand: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let operand_ty = Self::expr_ty(body, operand);
        if self.ctx.krate.types.get(operand_ty) == Some(&Type::String) {
            return Ok(operand);
        }
        if self.is_string_compatible_type(operand_ty) {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToString,
                    operand,
                },
                ty: string_ty,
                span,
            }));
        }
        Ok(operand)
    }

    /// Lower supported string padding calls into HIR string runtime calls.
    pub(in crate::lowering) fn string_pad_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "padStart" => StringPadOp::Start,
            "padEnd" => StringPadOp::End,
            _ => return Ok(None),
        };
        if !(1..=3).contains(&call.arguments.len()) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string padding requires target length and optional string padding",
            ));
        }
        let (operand, target_argument, pad_argument) = if call.arguments.len() == 3 {
            let Some(operand_argument) = call.arguments.first() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "string padding requires a receiver argument",
                ));
            };
            let operand = self.argument(operand_argument, body)?;
            (operand, call.arguments.get(1), call.arguments.get(2))
        } else {
            (
                self.expression(&member.object, body)?,
                call.arguments.first(),
                call.arguments.get(1),
            )
        };
        let Some(target_argument) = target_argument else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string padding requires target length",
            ));
        };
        let target_len = self.argument(target_argument, body)?;
        let pad = if let Some(pad_argument) = pad_argument {
            self.argument(pad_argument, body)?
        } else {
            let ty = self.ctx.krate.types.intern(Type::String);
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(" ".to_owned())),
                ty,
                span: self.span(call.span.start, call.span.end),
            })
        };
        let span = self.span(call.span.start, call.span.end);
        // Coerce the receiver/pad to `String` and the target length to a number
        // when they arrive as string/numeric-compatible or erased surfaces
        // (e.g. `length = 0` numeric defaults, `toString(...)` returns), instead
        // of requiring exact `String`/`Float` types.
        let operand = self.coerce_pad_string_operand(operand, span, body)?;
        let pad = self.coerce_pad_string_operand(pad, span, body)?;
        let target_len = {
            let target_ty = Self::expr_ty(body, target_len);
            if self.ctx.krate.types.get(target_ty) == Some(&Type::Float) {
                target_len
            } else if self.is_numeric_like_type(target_ty)
                || self.optional_numeric_surface(target_ty)
                || self.erased_or_union_surface(target_ty)
            {
                let float_ty = self.ctx.krate.types.intern(Type::Float);
                body.push_expr(Expr {
                    kind: ExprKind::PrimitiveCast {
                        op: PrimitiveCastOp::ToJsNumber,
                        operand: target_len,
                    },
                    ty: float_ty,
                    span,
                })
            } else {
                return Err(SmeltError::unsupported(
                    span,
                    "string padding requires a string receiver, number target length, and string padding",
                ));
            }
        };
        if self.ctx.krate.types.get(Self::expr_ty(body, operand)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, pad)) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                span,
                "string padding requires a string receiver, number target length, and string padding",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringPad {
                op,
                operand,
                target_len,
                pad,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `String.prototype.charAt` and `charCodeAt`.
    pub(in crate::lowering) fn string_char_at_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let returns_code = match member.property.name.as_str() {
            "charAt" => false,
            "charCodeAt" => true,
            _ => return Ok(None),
        };
        let [index_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string charAt/charCodeAt requires exactly one number argument",
            ));
        };
        let span = self.span(call.span.start, call.span.end);
        let mut operand = self.expression(&member.object, body)?;
        let mut index = self.argument(index_argument, body)?;
        // Coerce a string-compatible receiver (e.g. `T extends string` generic)
        // to `String` and a numeric-like index to a JS number, instead of
        // requiring exact `String`/`Float` types.
        let operand_ty = Self::expr_ty(body, operand);
        if self.ctx.krate.types.get(operand_ty) != Some(&Type::String)
            && self.is_string_compatible_type(operand_ty)
        {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            operand = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: operand },
                ty: string_ty,
                span,
            });
        }
        let index_ty = Self::expr_ty(body, index);
        if self.ctx.krate.types.get(index_ty) != Some(&Type::Float)
            && (self.is_numeric_like_type(index_ty)
                || self.optional_numeric_surface(index_ty)
                || self.erased_or_union_surface(index_ty))
        {
            let float_ty = self.ctx.krate.types.intern(Type::Float);
            index = body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToJsNumber,
                    operand: index,
                },
                ty: float_ty,
                span,
            });
        }
        if self.ctx.krate.types.get(Self::expr_ty(body, operand)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, index)) != Some(&Type::Float)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string charAt/charCodeAt requires a string receiver and number argument",
            ));
        }
        let ty = self.ctx.krate.types.intern(if returns_code {
            Type::Float
        } else {
            Type::String
        });
        let kind = if returns_code {
            ExprKind::StringCharCodeAt { operand, index }
        } else {
            ExprKind::StringCharAt { operand, index }
        };
        Ok(Some(body.push_expr(Expr {
            kind,
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.prototype.join` for string arrays.
    pub(in crate::lowering) fn string_join_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "join" {
            return Ok(None);
        }
        if let Expression::Identifier(object) = &member.object
            && (self.namespace_imports.contains(object.name.as_str())
                || self.value_imports.contains(object.name.as_str()))
        {
            if call.arguments.is_empty() || call.arguments.len() > 2 {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "static array join requires array and optional string separator arguments",
                ));
            }
            let Some(items_argument) = call.arguments.first() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "static array join requires array and optional string separator arguments",
                ));
            };
            let items = self.argument(items_argument, body)?;
            let separator = call.arguments.get(1);
            return self.finish_string_join_call(call, items, separator, body);
        }
        let items = self.expression(&member.object, body)?;
        let separator = match call.arguments.as_slice() {
            [] => None,
            [separator_argument] => Some(separator_argument),
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "array join supports zero or one string separator argument",
                ));
            }
        };
        self.finish_string_join_call(call, items, separator, body)
    }

    /// Finish array join lowering after receiver-style or helper-style arguments are known.
    pub(in crate::lowering) fn finish_string_join_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        mut items: smelt_hir::ExprId,
        separator_argument: Option<&Argument<'_>>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let items_ty = self.type_param_constraint_or_self(Self::expr_ty(body, items));
        let items_ty = match self.ctx.krate.types.get(items_ty) {
            Some(Type::List(_)) => items_ty,
            Some(Type::Unknown | Type::TypeParam { .. }) => {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                items = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: items },
                    ty: list_ty,
                    span: self.span(call.span.start, call.span.end),
                });
                list_ty
            }
            Some(Type::Union(members))
                if members.iter().any(|union_member| {
                    matches!(
                        self.ctx.krate.types.get(*union_member),
                        Some(Type::List(_) | Type::Unknown | Type::TypeParam { .. })
                    )
                }) =>
            {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                items = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: items },
                    ty: list_ty,
                    span: self.span(call.span.start, call.span.end),
                });
                list_ty
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "array join requires an array receiver",
                ));
            }
        };
        let Some(Type::List(item_ty)) = self.ctx.krate.types.get(items_ty) else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array join requires an array receiver",
            ));
        };
        if !self.array_join_item_type_supported(*item_ty) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array join currently requires primitive or unknown array items",
            ));
        }
        let separator = match separator_argument {
            None => body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(",".to_owned())),
                ty: string_ty,
                span: self.span(call.span.start, call.span.end),
            }),
            Some(separator_argument) => {
                let separator = self.argument(separator_argument, body)?;
                let separator_ty = Self::expr_ty(body, separator);
                if !self.is_string_compatible_type(separator_ty)
                    && !self.type_contains_unknown(separator_ty)
                    && !self.erased_or_union_surface(separator_ty)
                {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "array join separator must be a string",
                    ));
                }
                separator
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringJoin { items, separator },
            ty: string_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return whether `Array.prototype.join` can stringify this lowered item type.
    pub(in crate::lowering) fn array_join_item_type_supported(&self, item_ty: smelt_hir::TypeId) -> bool {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(item_ty))
        {
            Some(
                Type::Bool
                | Type::String
                | Type::Int
                | Type::Float
                | Type::Unknown
                | Type::TypeParam { .. },
            ) => true,
            Some(Type::Optional(item)) => self.array_join_item_type_supported(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .all(|item| self.array_join_item_type_supported(item)),
            _ => false,
        }
    }

    /// Lower direct TypeScript string containment.
    pub(in crate::lowering) fn string_contains_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "includes" {
            return Ok(None);
        }
        // JavaScript `String.prototype.includes(needle, position?)` takes the
        // needle plus an optional numeric start position handled by the emitter.
        if !(1..=2).contains(&call.arguments.len()) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string includes requires a needle and an optional position argument",
            ));
        }
        let mut haystack = self.expression(&member.object, body)?;
        let Some(needle_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string includes requires a needle and an optional position argument",
            ));
        };
        let mut needle = self.argument(needle_argument, body)?;
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let haystack_ty = Self::expr_ty(body, haystack);
        if self.list_surface_type(haystack_ty).is_some() {
            return Ok(None);
        }
        // A union of concrete members with a string arm (`string | fn` after a
        // `typeof path === 'string'` guard Smelt's erased locals do not
        // re-type) is also asserted down to `String`.
        if self.ctx.krate.types.get(haystack_ty) != Some(&Type::String)
            && (self.type_contains_unknown(haystack_ty)
                || self.erased_or_union_surface(haystack_ty)
                || self.is_string_compatible_type(haystack_ty))
        {
            haystack = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: haystack },
                ty: string_ty,
                span: self.span(member.object.span().start, member.object.span().end),
            });
        }
        let needle_ty = Self::expr_ty(body, needle);
        if self.ctx.krate.types.get(needle_ty) == Some(&Type::Unknown)
            || self.type_contains_unknown(needle_ty)
            || self.erased_or_union_surface(needle_ty)
        {
            needle = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: needle },
                ty: string_ty,
                span: self.span(needle_argument.span().start, needle_argument.span().end),
            });
        }
        let coerced_haystack_ty = Self::expr_ty(body, haystack);
        let coerced_needle_ty = Self::expr_ty(body, needle);
        if self.ctx.krate.types.get(coerced_haystack_ty) != Some(&Type::String)
            || self.ctx.krate.types.get(coerced_needle_ty) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string includes requires string receiver and argument",
            ));
        }
        let from_index = if let Some(position_argument) = call.arguments.get(1) {
            let position = self.argument(position_argument, body)?;
            if !self.slice_index_type_is_number(Self::expr_ty(body, position)) {
                return Err(SmeltError::unsupported(
                    self.span(
                        position_argument.span().start,
                        position_argument.span().end,
                    ),
                    "string includes position must be numeric",
                ));
            }
            Some(position)
        } else {
            None
        };
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringContains {
                haystack,
                needle,
                from_index,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript array containment.
    pub(in crate::lowering) fn list_contains_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "includes" {
            return Ok(None);
        }
        if call.arguments.len() != 1 {
            return Ok(None);
        }
        let mut list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some((list_ty, item_ty)) = self.list_surface_type(list_ty) else {
            return Ok(None);
        };
        if Self::expr_ty(body, list) != list_ty {
            list = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: list },
                ty: list_ty,
                span: self.span(member.object.span().start, member.object.span().end),
            });
        }
        let Some(item_argument) = call.arguments.first() else {
            return Ok(None);
        };
        let item = self.argument(item_argument, body)?;
        if !self.array_item_type_compatible(Self::expr_ty(body, item), item_ty) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array includes argument must match the array element type",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListContains { list, item },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return the concrete list and item type for list-like receiver surfaces.
    pub(in crate::lowering) fn list_surface_type(
        &self,
        ty: smelt_hir::TypeId,
    ) -> Option<(smelt_hir::TypeId, smelt_hir::TypeId)> {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
        {
            Some(Type::List(item_ty)) => Some((ty, *item_ty)),
            Some(Type::Optional(inner)) => match self.ctx.krate.types.get(*inner) {
                Some(Type::List(item_ty)) => Some((*inner, *item_ty)),
                _ => None,
            },
            Some(Type::Union(items)) => {
                let mut list_surface = None;
                for item in items {
                    if let Some(surface) = self.list_surface_type(*item) {
                        if list_surface.replace(surface).is_some() {
                            return None;
                        }
                    }
                }
                list_surface
            }
            _ => None,
        }
    }

    // Continued in the next split builder file.
}
