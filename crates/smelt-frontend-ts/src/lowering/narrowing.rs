//! Flow narrowing: the type a local holds inside a guarded branch.
//!
//! A guard is read off the condition of an `if`, a ternary or a `switch` arm, and
//! applied to the scope of that branch. This module owns the driver and the
//! nullish and truthiness guards; `typeof`, `instanceof` and shape guards live in
//! their own submodules.

mod instanceof_guards;
mod member_guards;
mod typeof_guards;

use crate::lowering::ModuleBuilder;
use oxc::ast::ast::{Expression, Statement};
use oxc::syntax::operator::{BinaryOperator, LogicalOperator, UnaryOperator};
use smelt_hir::{Body, Type};
use std::collections::HashMap;

impl ModuleBuilder<'_> {
    /// Apply a local narrowing in the current lexical lowering context.
    pub(in crate::lowering) fn apply_narrowing(&mut self, name: String, target: smelt_hir::TypeId) {
        self.scope.apply_narrowing(name, target);
    }

    /// Return the active narrowed type for a source local, if any.
    pub(in crate::lowering) fn narrowed_type(&self, name: &str) -> Option<smelt_hir::TypeId> {
        self.scope.narrowed_type(name)
    }

    /// Discover local type facts proven by a boolean guard expression.
    pub(in crate::lowering) fn guard_narrowing(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<HashMap<String, smelt_hir::TypeId>> {
        let mut out = HashMap::new();
        if let Expression::LogicalExpression(logical) = expression
            && logical.operator == LogicalOperator::And
        {
            if let Some(left) = self.guard_narrowing(&logical.left, body) {
                out.extend(left);
            }
            if let Some(right) = self.guard_narrowing(&logical.right, body) {
                out.extend(right);
            }
        } else if let Some((name, target)) = self.typeof_guard(expression, body) {
            out.insert(name, target);
        } else if let Some((name, target)) = self.array_is_array_guard(expression, body) {
            out.insert(name, target);
        } else if let Some((name, target)) = self.in_operator_guard(expression, body) {
            out.insert(name, target);
        } else if let Some((name, target)) = self.property_equality_guard(expression, body) {
            out.insert(name, target);
        } else if let Some((name, target)) = self.instanceof_local_guard(expression, body) {
            out.insert(name, target);
        } else if let Some((name, target)) = self.null_guard(expression) {
            out.insert(name, target);
        } else if let Some((name, target)) = self.optional_some_guard(expression, body) {
            out.insert(name, target);
        } else if let Some((name, target)) = self.truthy_guard(expression, body) {
            out.insert(name, target);
        } else if let Some((name, target)) = self.predicate_call_guard(expression, body) {
            out.insert(name, target);
        } else if let Expression::UnaryExpression(unary) = expression
            && unary.operator == UnaryOperator::LogicalNot
            && let Some(narrowing) = self.inverse_guard_narrowing(&unary.argument, body)
        {
            // `!guard` proves exactly what `guard` answering `false` proves, so
            // the then-branch of `if (!isRaw(r))` sees the members the guard's
            // inverse leaves behind.
            out.extend(narrowing);
        }
        (!out.is_empty()).then_some(out)
    }

    /// Discover local type facts proven after a guard exits early.
    pub(in crate::lowering) fn inverse_guard_narrowing(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<HashMap<String, smelt_hir::TypeId>> {
        let mut out = HashMap::new();
        if let Expression::LogicalExpression(logical) = expression
            && logical.operator == LogicalOperator::Or
        {
            if let Some(left) = self.inverse_guard_narrowing(&logical.left, body) {
                out.extend(left);
            }
            if let Some(right) = self.inverse_guard_narrowing(&logical.right, body) {
                out.extend(right);
            }
        } else if let Some((name, target)) = self.optional_none_inverse_guard(expression, body) {
            out.insert(name, target);
        } else if let Some((name, target)) = self.typeof_inverse_guard(expression, body) {
            out.insert(name, target);
        } else if let Some((name, target)) = self.instanceof_inverse_guard(expression, body) {
            out.insert(name, target);
        } else if let Some((name, target)) = self.predicate_call_inverse_guard(expression, body) {
            out.insert(name, target);
        } else if let Expression::UnaryExpression(unary) = expression
            && unary.operator == UnaryOperator::LogicalNot
            && let Some(narrowing) = self.guard_narrowing(&unary.argument, body)
        {
            out.extend(narrowing);
        }
        (!out.is_empty()).then_some(out)
    }

    /// Recognize `value !== undefined/null` guards.
    pub(in crate::lowering) fn optional_some_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let Expression::BinaryExpression(binary) = expression else {
            return None;
        };
        if !matches!(
            binary.operator,
            BinaryOperator::StrictInequality | BinaryOperator::Inequality
        ) {
            return None;
        }
        let name = match (&binary.left, &binary.right) {
            (Expression::Identifier(identifier), Expression::Identifier(undefined))
                if undefined.name == "undefined" =>
            {
                identifier.name.as_str()
            }
            (Expression::Identifier(identifier), Expression::NullLiteral(_)) => {
                identifier.name.as_str()
            }
            _ => return None,
        };
        let local = self.scope.lookup(name)?;
        let local_ty = self
            .narrowed_type(name)
            .unwrap_or_else(|| Self::local_ty(body, local));
        match self.ctx.krate.types.get(local_ty).cloned() {
            Some(Type::Optional(inner)) => Some((name.to_owned(), inner)),
            Some(Type::Union(items)) => {
                let none_ty = self.ctx.krate.types.intern(Type::None);
                let remaining = items
                    .into_iter()
                    .filter(|item| *item != none_ty)
                    .collect::<Vec<_>>();
                match remaining.as_slice() {
                    [single] => Some((name.to_owned(), *single)),
                    [] => None,
                    _ => Some((
                        name.to_owned(),
                        self.ctx.krate.types.intern(Type::Union(remaining)),
                    )),
                }
            }
            _ => None,
        }
    }

    /// Return whether two branch types can be represented by one callable shape.
    pub(in crate::lowering) fn compatible_function_branch_types(
        &self,
        left: smelt_hir::TypeId,
        right: smelt_hir::TypeId,
    ) -> bool {
        let left = self.type_param_constraint_or_self(left);
        let right = self.type_param_constraint_or_self(right);
        let (Some(Type::Function(left_fn)), Some(Type::Function(right_fn))) = (
            self.ctx.krate.types.get(left),
            self.ctx.krate.types.get(right),
        ) else {
            return false;
        };
        left_fn.params.len() == right_fn.params.len() && left_fn.is_async == right_fn.is_async
    }

    /// Return the callable branch when the other branch is imprecise metadata.
    pub(in crate::lowering) fn single_function_branch_type(
        &self,
        left: smelt_hir::TypeId,
        right: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        let left = self.type_param_constraint_or_self(left);
        let right = self.type_param_constraint_or_self(right);
        match (
            self.ctx.krate.types.get(left),
            self.ctx.krate.types.get(right),
        ) {
            (Some(Type::Function(_)), Some(Type::Class { .. })) => Some(left),
            (Some(Type::Class { .. }), Some(Type::Function(_))) => Some(right),
            _ => None,
        }
    }

    /// Recognize `value === undefined/null` guards whose true branch exits.
    pub(in crate::lowering) fn optional_none_inverse_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let Expression::BinaryExpression(binary) = expression else {
            return None;
        };
        if !matches!(
            binary.operator,
            BinaryOperator::StrictEquality | BinaryOperator::Equality
        ) {
            return None;
        }
        let name = match (&binary.left, &binary.right) {
            (Expression::Identifier(identifier), Expression::Identifier(undefined))
                if undefined.name == "undefined" =>
            {
                identifier.name.as_str()
            }
            (Expression::Identifier(identifier), Expression::NullLiteral(_)) => {
                identifier.name.as_str()
            }
            _ => return None,
        };
        let local = self.scope.lookup(name)?;
        let local_ty = self
            .narrowed_type(name)
            .unwrap_or_else(|| Self::local_ty(body, local));
        match self.ctx.krate.types.get(local_ty).cloned() {
            Some(Type::Optional(inner)) => Some((name.to_owned(), inner)),
            Some(Type::Union(items)) => {
                let none_ty = self.ctx.krate.types.intern(Type::None);
                let remaining = items
                    .into_iter()
                    .filter(|item| *item != none_ty)
                    .collect::<Vec<_>>();
                match remaining.as_slice() {
                    [single] => Some((name.to_owned(), *single)),
                    [] => None,
                    _ => Some((
                        name.to_owned(),
                        self.ctx.krate.types.intern(Type::Union(remaining)),
                    )),
                }
            }
            _ => None,
        }
    }

    /// Return whether a source statement always exits the current flow path.
    pub(in crate::lowering) fn statement_must_exit(statement: &Statement<'_>) -> bool {
        match statement {
            Statement::ReturnStatement(_)
            | Statement::BreakStatement(_)
            | Statement::ContinueStatement(_)
            | Statement::ThrowStatement(_) => true,
            Statement::BlockStatement(block) => {
                block.body.last().is_some_and(Self::statement_must_exit)
            }
            Statement::IfStatement(if_stmt) => {
                if_stmt.alternate.as_ref().is_some_and(|alternate| {
                    Self::statement_must_exit(&if_stmt.consequent)
                        && Self::statement_must_exit(alternate)
                })
            }
            _ => false,
        }
    }

    /// Return whether a no-else guarded branch unconditionally reassigns `name`
    /// to a non-nullish value on its taken path.
    ///
    /// This recognizes the ubiquitous default-initialization idiom
    /// `if (x == null) { x = <value>; }`: on the *not-taken* path the nullish
    /// guard's inverse already proves `x` is non-null, and on the *taken* path
    /// this reassignment makes it non-null too, so `x` is non-null after the `if`.
    /// Only a top-level (unconditionally reached) `x = <expr>` statement counts —
    /// an assignment nested inside another branch is not guaranteed to run — and
    /// the assigned value must not be a `null`/`undefined` literal. The last such
    /// top-level assignment wins, matching JavaScript's flow order.
    pub(in crate::lowering) fn branch_reassigns_to_nonnull(
        consequent: &Statement<'_>,
        name: &str,
    ) -> bool {
        let statements = match consequent {
            Statement::BlockStatement(block) => block.body.as_slice(),
            other => std::slice::from_ref(other),
        };
        let mut reassigned_nonnull = false;
        for statement in statements {
            let Statement::ExpressionStatement(expr_stmt) = statement else {
                continue;
            };
            let Expression::AssignmentExpression(assign) = &expr_stmt.expression else {
                continue;
            };
            if assign.operator != oxc::syntax::operator::AssignmentOperator::Assign {
                continue;
            }
            let oxc::ast::ast::AssignmentTarget::AssignmentTargetIdentifier(target) = &assign.left
            else {
                continue;
            };
            if target.name != name {
                continue;
            }
            reassigned_nonnull = !Self::expression_is_nullish_literal(&assign.right);
        }
        reassigned_nonnull
    }

    /// Collect the identifier names assigned at the top level of a branch.
    ///
    /// Only unconditionally reached assignments to an identifier (plain or compound)
    /// are gathered (assignments nested inside a further branch are excluded), so callers
    /// can reason about facts that hold on every path through the branch. Used
    /// with [`branch_reassigns_to_nonnull`] to compute branch-join narrowing.
    pub(in crate::lowering) fn branch_top_level_assigned_names(
        statement: &Statement<'_>,
    ) -> Vec<String> {
        let statements = match statement {
            Statement::BlockStatement(block) => block.body.as_slice(),
            other => std::slice::from_ref(other),
        };
        let mut names = Vec::new();
        for branch_statement in statements {
            let Statement::ExpressionStatement(expr_stmt) = branch_statement else {
                continue;
            };
            let Expression::AssignmentExpression(assign) = &expr_stmt.expression else {
                continue;
            };
            let oxc::ast::ast::AssignmentTarget::AssignmentTargetIdentifier(target) = &assign.left
            else {
                continue;
            };
            let name = target.name.to_string();
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names
    }

    /// Return the non-null type of an optional local currently in scope.
    ///
    /// Reads the local's active (possibly already-narrowed) type and, when it
    /// is `Optional<T>` or a nullable union, returns the type with the
    /// `None`/`undefined` member removed. Returns `None` for locals that are
    /// unknown here or not nullable. Used to narrow a local after both arms of
    /// an if/else reassign it to a non-null value (branch-join narrowing).
    pub(in crate::lowering) fn optional_local_nonnull_type(
        &mut self,
        name: &str,
        body: &Body,
    ) -> Option<smelt_hir::TypeId> {
        let local = self.scope.lookup(name)?;
        let local_ty = self
            .narrowed_type(name)
            .unwrap_or_else(|| Self::local_ty(body, local));
        match self.ctx.krate.types.get(local_ty).cloned() {
            Some(Type::Optional(inner)) => Some(inner),
            Some(Type::Union(items)) => {
                let none_ty = self.ctx.krate.types.intern(Type::None);
                let remaining = items
                    .into_iter()
                    .filter(|item| *item != none_ty)
                    .collect::<Vec<_>>();
                match remaining.as_slice() {
                    [single] => Some(*single),
                    [] => None,
                    _ => Some(self.ctx.krate.types.intern(Type::Union(remaining))),
                }
            }
            _ => None,
        }
    }

    /// Return whether an expression is a `null` or `undefined` literal.
    fn expression_is_nullish_literal(expression: &Expression<'_>) -> bool {
        match expression {
            Expression::NullLiteral(_) => true,
            Expression::Identifier(identifier) => identifier.name == "undefined",
            Expression::ParenthesizedExpression(paren) => {
                Self::expression_is_nullish_literal(&paren.expression)
            }
            _ => false,
        }
    }

    /// Recognize `value === null` guard expressions.
    pub(in crate::lowering) fn null_guard(
        &mut self,
        expression: &Expression<'_>,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let Expression::BinaryExpression(binary) = expression else {
            return None;
        };
        if !matches!(
            binary.operator,
            BinaryOperator::StrictEquality | BinaryOperator::Equality
        ) {
            return None;
        }
        let (Expression::Identifier(identifier), Expression::NullLiteral(_)) =
            (&binary.left, &binary.right)
        else {
            return None;
        };
        Some((
            identifier.name.to_string(),
            self.ctx.krate.types.intern(Type::None),
        ))
    }

    /// Recognize bare truthiness guards that remove `undefined`/`None` from locals.
    pub(in crate::lowering) fn truthy_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let name = match expression {
            Expression::Identifier(identifier) => identifier.name.as_str(),
            // A TRUTHY optional chain rooted at a local (`x?.a`, `x?.a.b`,
            // `x?.m()`) proves the root present: a nullish root makes the
            // whole chain `undefined`, which is falsy. So `if (!x?.length)
            // return` leaves `x` narrowed to its present type afterwards.
            Expression::ChainExpression(chain) => Self::optional_chain_root_name(chain)?,
            _ => return None,
        };
        let local = self.scope.lookup(name)?;
        let local_ty = match self.narrowed_type(name) {
            Some(ty) => ty,
            None => Self::local_ty_checked(body, local)?,
        };
        match self.ctx.krate.types.get(local_ty).cloned() {
            Some(Type::Optional(inner)) => Some((name.to_owned(), inner)),
            Some(Type::Union(items)) => {
                let none_ty = self.ctx.krate.types.intern(Type::None);
                let remaining = items
                    .into_iter()
                    .filter(|item| *item != none_ty)
                    .collect::<Vec<_>>();
                match remaining.as_slice() {
                    [single] => Some((name.to_owned(), *single)),
                    [] => None,
                    _ => Some((
                        name.to_owned(),
                        self.ctx.krate.types.intern(Type::Union(remaining)),
                    )),
                }
            }
            _ => None,
        }
    }

    /// Name of the local an optional chain is rooted at, when the root is a
    /// plain identifier reached through member reads and calls.
    fn optional_chain_root_name<'b>(
        chain: &'b oxc::ast::ast::ChainExpression<'_>,
    ) -> Option<&'b str> {
        use oxc::ast::ast::ChainElement;
        let mut current: &Expression<'_> = match &chain.expression {
            ChainElement::StaticMemberExpression(member) => &member.object,
            ChainElement::ComputedMemberExpression(member) => &member.object,
            ChainElement::CallExpression(call) => &call.callee,
            _ => return None,
        };
        loop {
            current = match current {
                Expression::Identifier(identifier) => return Some(identifier.name.as_str()),
                Expression::StaticMemberExpression(member) => &member.object,
                Expression::ComputedMemberExpression(member) => &member.object,
                Expression::CallExpression(call) => &call.callee,
                Expression::ParenthesizedExpression(inner) => &inner.expression,
                _ => return None,
            };
        }
    }
}
