//! The statement dispatcher: one source statement into the current block, and the
//! statement forms too small for a module of their own.

use crate::lowering::peel::{Wrappers, peel};
use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::support::statement_terminates;
use oxc::ast::ast::{AssignmentTarget, Expression, Statement};
use oxc::span::GetSpan;
use oxc::syntax::operator::{AssignmentOperator, BinaryOperator, UnaryOperator};
use smelt_hir::{AsyncOp, BinOp, Body, Expr, ExprKind, Literal, MatchArm, Stmt, Type, UnaryOp};

impl ModuleBuilder<'_> {
    /// Type hint for the value of a `return` statement in the current function.
    ///
    /// For a synchronous function this is simply the declared return type. For
    /// an `async` function the declared return type is `Promise<Inner>`
    /// (`Type::Future`), yet the value produced by a `return X` statement is the
    /// resolved `Inner`, because the async lowering itself is responsible for
    /// wrapping the body into the promise. Hinting the raw `Future` type here
    /// makes literal returns (tuples, arrays, objects) get coerced into a
    /// promise around a non-future value; unwrapping one `Future` layer keeps
    /// the returned expression at the value type it actually has.
    pub(in crate::lowering) fn return_statement_value_hint(&self) -> Option<smelt_hir::TypeId> {
        let return_ty = self.fn_ctx.return_ty?;
        if self.fn_ctx.is_async
            && let Some(inner) = self.future_inner_type(return_ty)
        {
            return Some(inner);
        }
        if let Some(Type::Generator { return_ty, .. }) = self.ctx.krate.types.get(return_ty) {
            return Some(*return_ty);
        }
        Some(return_ty)
    }

    /// `base` when no class, interface or type alias item of the crate carries
    /// it yet, else a compiler-minted `{base_text}R{n}` for the first free `n`.
    ///
    /// MIR resolves a class by its name symbol and refuses two classes (or two
    /// interfaces) of one name, so a declaration whose
    /// name is already held by a DIFFERENT declaration (a constructor function
    /// declared in two `it` callbacks of one file) takes its symbol from here.
    /// The caller interns `base` with the origin it has (a source spelling or
    /// a name the lowering minted); a rename is always a
    /// [`smelt_hir::SynthesizedName`], and the caller records the source
    /// spelling reflection should answer.
    pub(in crate::lowering) fn unclaimed_type_symbol(
        &mut self,
        base: smelt_hir::Symbol,
        base_text: &str,
    ) -> smelt_hir::Symbol {
        let claimed = |krate: &smelt_hir::Crate, symbol: smelt_hir::Symbol| {
            krate.items.iter().any(|item| match item {
                smelt_hir::Item::Class(class) => class.name == symbol,
                smelt_hir::Item::Interface(interface) => interface.name == symbol,
                smelt_hir::Item::TypeAlias(alias) => alias.name == symbol,
                smelt_hir::Item::Function(_)
                | smelt_hir::Item::Const(_)
                | smelt_hir::Item::MutableGlobal(_) => false,
            })
        };
        if !claimed(&self.ctx.krate, base) {
            return base;
        }
        let mut round: u32 = 1;
        loop {
            let candidate = self.intern_synthesized_type_name(&smelt_hir::SynthesizedName::new(
                format!("{base_text}R{round}"),
            ));
            if !claimed(&self.ctx.krate, candidate) {
                return candidate;
            }
            round = round.saturating_add(1);
        }
    }

    /// The class item already lowered from the declaration at `span`, if any.
    ///
    /// Test setup is lowered once per test it runs before, so one source
    /// declaration can reach class lowering several times. A declaration's
    /// span is unique to it, so an existing class item with the same span IS
    /// this declaration, and lowering it again would only mint a copy (es-toolkit's
    /// `isFile.spec.ts` minted six `File` classes from one `beforeAll`).
    pub(in crate::lowering) fn class_item_lowered_at(
        &self,
        span: smelt_hir::Span,
    ) -> Option<(smelt_hir::ItemId, smelt_hir::Symbol)> {
        self.ctx.krate.items.iter().enumerate().find_map(|(index, item)| match item {
            smelt_hir::Item::Class(class) if class.span == span => {
                Some((smelt_hir::ItemId(u32::try_from(index).ok()?), class.name))
            }
            _ => None,
        })
    }

    /// Give a block-local class its own identity when its name is already taken.
    ///
    /// Two functions may each declare `class Data`: they are different classes,
    /// and the name means this one until the enclosing lexical scope ends.
    fn shadow_outer_class(&mut self, class: &oxc::ast::ast::Class<'_>) {
        let Some(id) = &class.id else {
            return;
        };
        let name = id.name.as_str();
        let plain = self.intern_type_name(name);
        let taken = self.classes.item(name).is_some()
            || (self.classes.in_body()
                && (self.classes.declared_in_module(name)
                    || self.ctx.top_level_type_names.contains(name)))
            || self.ctx.krate.items.iter().any(|item| match item {
                smelt_hir::Item::Class(lowered) => lowered.name == plain,
                // An earlier type or interface of the name renders as the same
                // Rust struct name, so it takes the name as a class does.
                smelt_hir::Item::TypeAlias(alias) => alias.name == plain,
                smelt_hir::Item::Interface(interface) => interface.name == plain,
                _ => false,
            });
        if !taken {
            return;
        }
        let internal_name = format!("{}SmeltSuiteF{}S{}", name, self.file_id.0, class.span.start);
        let symbol = self.intern_type_name(&internal_name);
        if self.classes.scoped_type_name(name) == Some(symbol) {
            return;
        }
        self.classes.open_shadow(name);
        self.classes.bind_scoped_type_name(name.to_owned(), symbol);
    }

    /// Give a block-local `type` or `interface` named like a type the crate
    /// already declares its own scoped symbol, as [`Self::shadow_outer_class`]
    /// does for a class: successive tests declaring `type Input` each mean their
    /// own type, and one crate-level item per spelling made every later `Input`
    /// the first one's struct.
    pub(in crate::lowering) fn shadow_outer_type(&mut self, name: &str, start: u32) {
        let plain = self.intern_type_name(name);
        let taken = self.classes.scoped_type_name(name).is_some()
            || (self.classes.in_body() && self.ctx.top_level_type_names.contains(name))
            || self.ctx.krate.items.iter().any(|item| match item {
                smelt_hir::Item::TypeAlias(alias) => alias.name == plain,
                smelt_hir::Item::Interface(interface) => interface.name == plain,
                smelt_hir::Item::Class(class) => class.name == plain,
                _ => false,
            });
        if !taken {
            return;
        }
        let internal_name = format!("{}SmeltSuiteF{}S{}", name, self.file_id.0, start);
        let symbol = self.intern_type_name(&internal_name);
        if self.classes.scoped_type_name(name) == Some(symbol) {
            return;
        }
        self.classes.open_shadow(name);
        self.classes.bind_scoped_type_name(name.to_owned(), symbol);
    }

    /// Lower a statement within a specific block.
    pub(in crate::lowering) fn statement_in_block(
        &mut self,
        statement: &Statement<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let anchor = self.statement_span(statement);
        self.with_erasure_anchor(anchor, |this| {
            this.with_stmt_block(block, |this| this.statement_in_sink(statement, body, block))
        })
    }

    /// [`Self::statement_in_block`] with the statement sink already pointing at
    /// `block` and the erasure anchor at the statement. Every `?` below may
    /// return early: the caller restores both on every exit path.
    fn statement_in_sink(
        &mut self,
        statement: &Statement<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        match statement {
            Statement::EmptyStatement(_) => Ok(()),
            Statement::VariableDeclaration(decl) => self.variable_declaration(decl, body, block),
            Statement::FunctionDeclaration(function) => {
                self.local_function_declaration(function, body, block)
            }
            Statement::ClassDeclaration(class) => {
                self.shadow_outer_class(class);
                if !self.conditional_class_declaration(class)? {
                    let item = self.class_declaration(class)?;
                    self.touch_class_static_initializers(item, body, block);
                    self.evaluate_class_statics(item, class, body)?;
                }
                Ok(())
            }
            Statement::TSTypeAliasDeclaration(alias) => {
                self.shadow_outer_type(alias.id.name.as_str(), alias.span.start);
                self.type_alias_declaration(alias)?;
                Ok(())
            }
            Statement::TSInterfaceDeclaration(interface) => {
                self.shadow_outer_type(interface.id.name.as_str(), interface.span.start);
                self.interface_declaration(interface)?;
                Ok(())
            }
            Statement::TSNamespaceDeclaration(_) => Ok(()),
            // A block-local enum folds like a module-level one, and like a
            // block-local class or type alias it is registered under its name
            // for the rest of the module; it is not published to importers.
            Statement::TSEnumDeclaration(decl) => {
                self.collect_enum_members(decl, false);
                Ok(())
            }
            Statement::ExpressionStatement(expr_stmt) => {
                self.expression_statement(&expr_stmt.expression, expr_stmt.span, body, block)
            }
            Statement::ReturnStatement(return_stmt) => {
                // Inside an `async` function the declared return type is
                // `Promise<Inner>` (a `Type::Future`), but a `return X` statement
                // yields the *resolved* value `Inner`, not the promise itself:
                // the async lowering wraps the whole body into the future. Hint
                // the returned expression with the unwrapped inner type so a
                // tuple/array/object literal lowers to `Inner` directly instead
                // of being coerced into a `SmeltPromise::from_future(..)` around
                // a non-future value.
                let return_hint = self.return_statement_value_hint();
                let value = return_stmt
                    .argument
                    .as_ref()
                    .map(|argument| self.expression_with_hint(argument, body, return_hint))
                    .transpose()?;
                if self.fn_ctx.verifies_assertion_plan {
                    self.verify_planned_assertions(return_stmt.span, body);
                }
                body.push_stmt_to_block(block, Stmt::Return(value));
                Ok(())
            }
            Statement::IfStatement(if_stmt) => {
                let cond = self.condition_expression(&if_stmt.test, body)?;
                let then_narrowing = self.guard_narrowing(&if_stmt.test, body);
                // Assignments performed only within this branch are flow facts
                // for the branch, not for statements reached from either path.
                let then_block = self.with_narrowing(Some(then_narrowing.unwrap_or_default()), |this| {
                    this.block_from_statement(&if_stmt.consequent, body)
                })?;
                let else_narrowing = self.inverse_guard_narrowing(&if_stmt.test, body);
                let else_block = if let Some(alternate) = &if_stmt.alternate {
                    let else_block = self.with_narrowing(Some(else_narrowing.unwrap_or_default()), |this| {
                        this.block_from_statement(alternate, body)
                    })?;
                    Some(else_block)
                } else {
                    None
                };
                body.push_stmt_to_block(
                    block,
                    Stmt::If {
                        cond,
                        then_block,
                        else_block,
                    },
                );
                if if_stmt.alternate.is_none()
                    && Self::statement_must_exit(&if_stmt.consequent)
                    && let Some(narrowing) = self.inverse_guard_narrowing(&if_stmt.test, body)
                {
                    #[expect(
                        clippy::iter_over_hash_type,
                        reason = "keyed inserts into the narrowing scope"
                    )]
                    for (name, target) in narrowing {
                        self.apply_narrowing(name, target);
                    }
                }
                // Default-initialization idiom `if (x == null) { x = <value>; }`:
                // both paths leave `x` non-null (the not-taken path by the nullish
                // guard's inverse, the taken path by the reassignment), so narrow
                // `x` to its non-null type after the `if`. This runs alongside the
                // must-exit case above (a reassigning branch does not exit) and lets
                // later reads/writes such as `x[i] = ...` see the concrete list.
                if if_stmt.alternate.is_none()
                    && let Some((name, non_null_ty)) =
                        self.optional_none_inverse_guard(&if_stmt.test, body)
                    && Self::branch_reassigns_to_nonnull(&if_stmt.consequent, &name)
                {
                    self.apply_narrowing(name, non_null_ty);
                }
                // Branch-join narrowing for the two-armed form
                // `if (cond) { x = <nonnull>; } else { x = <nonnull>; }`: both
                // arms leave `x` non-null on their respective paths, so `x` is
                // non-null after the join regardless of which arm ran. Narrow
                // every optional local that is reassigned to a non-null value at
                // the top level of *both* arms to its non-null type; this lets
                // later reads/writes see the concrete type instead of the
                // declared `Optional<T>` (e.g. `fromIndex?: number` reassigned in
                // both arms then indexed as `f64`).
                if let Some(alternate) = &if_stmt.alternate {
                    for name in Self::branch_top_level_assigned_names(&if_stmt.consequent) {
                        if Self::branch_reassigns_to_nonnull(&if_stmt.consequent, &name)
                            && Self::branch_reassigns_to_nonnull(alternate, &name)
                            && let Some(non_null_ty) = self.optional_local_nonnull_type(&name, body)
                        {
                            self.apply_narrowing(name, non_null_ty);
                        }
                    }
                }
                Ok(())
            }
            Statement::WhileStatement(while_stmt) => {
                if let Some((cond, loop_body, update_target, update_value)) =
                    self.while_assignment_condition_body(while_stmt, body, block)?
                {
                    body.push_stmt_to_block(
                        block,
                        Stmt::WhileUpdate {
                            cond,
                            body: loop_body,
                            update_target,
                            update_value,
                        },
                    );
                    return Ok(());
                }
                let cond = self.condition_expression(&while_stmt.test, body)?;
                let loop_narrowing = self.guard_narrowing(&while_stmt.test, body);
                let loop_body = self.with_narrowing(Some(loop_narrowing.unwrap_or_default()), |this| {
                    this.block_from_statement(&while_stmt.body, body)
                })?;
                body.push_stmt_to_block(
                    block,
                    Stmt::While {
                        cond,
                        body: loop_body,
                    },
                );
                Ok(())
            }
            Statement::DoWhileStatement(do_while) => {
                let loop_body = self.block_from_statement(&do_while.body, body)?;
                let cond = self.condition_expression(&do_while.test, body)?;
                let negated = body.push_expr(Expr {
                    kind: ExprKind::UnaryOp {
                        op: UnaryOp::Not,
                        operand: cond,
                    },
                    ty: self.ctx.krate.types.intern(Type::Bool),
                    span: self.span(do_while.test.span().start, do_while.test.span().end),
                });
                let break_block =
                    body.push_block(self.span(do_while.span.start, do_while.span.end));
                body.push_stmt_to_block(break_block, Stmt::Break);
                body.push_stmt_to_block(
                    loop_body,
                    Stmt::If {
                        cond: negated,
                        then_block: break_block,
                        else_block: None,
                    },
                );
                let true_expr = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Bool(true)),
                    ty: self.ctx.krate.types.intern(Type::Bool),
                    span: self.span(do_while.span.start, do_while.span.end),
                });
                body.push_stmt_to_block(
                    block,
                    Stmt::While {
                        cond: true_expr,
                        body: loop_body,
                    },
                );
                Ok(())
            }
            Statement::ForOfStatement(for_stmt) => {
                let lowered = self.with_lexical_scope(|this| {
                    let mut iter = this.expression(&for_stmt.right, body)?;
                    if for_stmt.r#await
                        && let Some(Type::Future(inner)) =
                            this.ctx.krate.types.get(Self::expr_ty(body, iter)).cloned()
                    {
                        iter = body.push_expr(Expr {
                            kind: ExprKind::Await(iter),
                            ty: inner,
                            span: this.span(for_stmt.right.span().start, for_stmt.right.span().end),
                        });
                    }
                    // A synchronous generator receiver has no indexable Rust
                    // representation, so it cannot flow through the index-based
                    // `Stmt::For` lowering. Drain it through the resume protocol
                    // instead. Async generators keep the existing path (their
                    // resume returns a future).
                    if let Some(Type::Generator {
                        is_async: false,
                        yield_ty,
                        return_ty,
                        ..
                    }) = this.ctx.krate.types.get(Self::expr_ty(body, iter)).cloned()
                    {
                        this.lower_generator_for_of(for_stmt, iter, yield_ty, return_ty, body, block)?;
                        return Ok(None);
                    }
                    let iter = this.for_of_iterable(iter, &for_stmt.right, body);
                    let (pat, loop_body) =
                        this.for_binding_and_body(&for_stmt.left, &for_stmt.body, iter, body)?;
                    Ok::<_, SmeltError>(Some((pat, iter, loop_body)))
                })?;
                let Some((pat, iter, loop_body)) = lowered else {
                    return Ok(());
                };
                body.push_stmt_to_block(
                    block,
                    Stmt::For {
                        pat,
                        iter,
                        body: loop_body,
                    },
                );
                Ok(())
            }
            Statement::ForInStatement(for_stmt) => {
                let (pat, iter, loop_body) = self.with_lexical_scope(|this| {
                    let iter = this.for_in_iterable(&for_stmt.right, body)?;
                    let (pat, loop_body) =
                        this.for_binding_and_body(&for_stmt.left, &for_stmt.body, iter, body)?;
                    Ok::<_, SmeltError>((pat, iter, loop_body))
                })?;
                body.push_stmt_to_block(
                    block,
                    Stmt::For {
                        pat,
                        iter,
                        body: loop_body,
                    },
                );
                Ok(())
            }
            Statement::ForStatement(for_stmt) => self.c_for_statement(for_stmt, body, block),
            Statement::SwitchStatement(switch_stmt) => {
                // A switch with real fallthrough (a case body that can reach
                // the next case) cannot lower to a HIR `Match`; route it
                // through the general single-iteration-loop lowering where JS
                // `break` maps to the loop break.
                if Self::switch_has_fallthrough(switch_stmt) {
                    return self.switch_fallthrough_statement(switch_stmt, body, block);
                }
                let scrutinee = self.expression(&switch_stmt.discriminant, body)?;
                // A switch on a discriminant property (`switch (x.kind)`) proves,
                // inside every non-default arm, that `x` carries that property.
                // The narrowing is only recorded for arms with a case label; the
                // `default` arm is reached when no label matched and cannot rely
                // on the discriminant being present. Nullish/dynamic boundaries
                // are left untouched: the fact only projects concrete union arms.
                let discriminant_narrowing =
                    self.switch_discriminant_narrowing(&switch_stmt.discriminant, body);
                // A `switch (typeof x)` discriminates a union the way a chain of
                // `if (typeof x === 'k')` guards would: each labeled arm proves
                // `x` is the member(s) whose runtime `typeof` matches that arm's
                // label. Unlike the field-discriminant fact above, this narrowing
                // is per-arm, so the arm's own label(s) drive it. `pending_kinds`
                // accumulates the string labels of grouped empty cases
                // (`case 'a': case 'b':`) sharing the next arm's body; a `None`
                // entry marks a label we could not read as a `typeof` kind, which
                // disables narrowing for that group.
                let typeof_switch_local = Self::typeof_identifier_name(&switch_stmt.discriminant);
                let mut pending_kinds: Vec<Option<String>> = Vec::new();
                let mut arms = Vec::new();
                let mut default = None;
                let mut pending_empty_labels = Vec::new();

                let case_count = switch_stmt.cases.len();
                for (case_index, case) in switch_stmt.cases.iter().enumerate() {
                    if case.consequent.is_empty() {
                        if let Some(test) = &case.test {
                            if typeof_switch_local.is_some() {
                                pending_kinds.push(Self::string_literal_value(test));
                            }
                            pending_empty_labels.push(self.literal_case_label(test)?);
                            continue;
                        }
                    }
                    // Discriminant facts apply to labeled arms only; the default
                    // arm handles the "no label matched" path and must not assume
                    // the discriminant property is present.
                    let case_narrowing = if case.test.is_some() {
                        // Prefer the per-arm `typeof` fact when the discriminant is
                        // `typeof x`; otherwise project the shared field-discriminant
                        // fact. Grouped labels for this arm (`pending_kinds`) union
                        // with the arm's own label, and a group we could not read as
                        // kinds records no fact.
                        let typeof_fact = typeof_switch_local.as_ref().and_then(|name| {
                            let group = std::mem::take(&mut pending_kinds);
                            let current = case.test.as_ref().and_then(Self::string_literal_value);
                            match current {
                                Some(current) if group.iter().all(Option::is_some) => {
                                    let mut kinds: Vec<String> =
                                        group.into_iter().flatten().collect();
                                    kinds.push(current);
                                    self.switch_typeof_case_narrowing(name, &kinds, body)
                                }
                                _ => None,
                            }
                        });
                        let fact = typeof_fact.or_else(|| discriminant_narrowing.clone());
                        fact.map(|(name, target)| std::collections::HashMap::from([(name, target)]))
                    } else {
                        // The `default` arm proves no discriminant fact; drop any
                        // pending `typeof` group kinds that fell through to it.
                        pending_kinds.clear();
                        None
                    };
                    let case_block = body.push_block(self.span(case.span.start, case.span.end));
                    // A case body ends at the first statement that leaves it.
                    // The two ways differ in what they LOWER: a JS `break`
                    // exits the switch, which the match arm does by simply
                    // ending, so it emits nothing; a `continue` targets the
                    // ENCLOSING LOOP and has to be emitted, exactly as Rust
                    // spells `continue` inside a `match` arm inside a loop.
                    // Rejecting the second was the only reason Hono's HTML
                    // escaper (`utils/html.ts`, `default: continue` inside the
                    // scan loop) was a blocker. The synthetic-loop lowering
                    // used for real fallthrough still rejects it, and says why.
                    let case_ends = self.with_narrowing(case_narrowing, |this| -> Result<bool, SmeltError> {
                        let mut case_ends = false;
                        for case_statement in &case.consequent {
                            if let Statement::BlockStatement(block_stmt) = case_statement {
                                for nested_statement in &block_stmt.body {
                                    if matches!(nested_statement, Statement::BreakStatement(_)) {
                                        case_ends = true;
                                        break;
                                    }
                                    this.statement_in_block(nested_statement, body, case_block)?;
                                    if matches!(nested_statement, Statement::ContinueStatement(_)) {
                                        case_ends = true;
                                        break;
                                    }
                                }
                                if case_ends {
                                    break;
                                }
                                continue;
                            }
                            if matches!(case_statement, Statement::BreakStatement(_)) {
                                case_ends = true;
                                break;
                            }
                            this.statement_in_block(case_statement, body, case_block)?;
                            if matches!(case_statement, Statement::ContinueStatement(_)) {
                                case_ends = true;
                                break;
                            }
                        }
                        Ok(case_ends)
                    })?;
                    let saw_break = case_ends;
                    let is_last_case = case_index + 1 == case_count;
                    if !saw_break
                        && !is_last_case
                        && !case.consequent.iter().any(statement_terminates)
                    {
                        return Err(SmeltError::unsupported(
                            self.span(case.span.start, case.span.end),
                            "switch fallthrough is not lowered yet; each case must break, return, or throw",
                        ));
                    }

                    if let Some(test) = &case.test {
                        for label in std::mem::take(&mut pending_empty_labels) {
                            arms.push(MatchArm {
                                label,
                                body: case_block,
                            });
                        }
                        arms.push(MatchArm {
                            label: self.literal_case_label(test)?,
                            body: case_block,
                        });
                    } else if default.replace(case_block).is_some() {
                        return Err(SmeltError::unsupported(
                            self.span(case.span.start, case.span.end),
                            "switch statements can only have one default case",
                        ));
                    } else {
                        for label in std::mem::take(&mut pending_empty_labels) {
                            arms.push(MatchArm {
                                label,
                                body: case_block,
                            });
                        }
                    }
                }
                if !pending_empty_labels.is_empty() {
                    return Err(SmeltError::unsupported(
                        self.span(switch_stmt.span.start, switch_stmt.span.end),
                        "switch fallthrough is not lowered yet; each case must break, return, or throw",
                    ));
                }

                body.push_stmt_to_block(
                    block,
                    Stmt::Match {
                        scrutinee,
                        arms,
                        default,
                    },
                );
                Ok(())
            }
            Statement::BreakStatement(break_stmt) => {
                let stmt = match &break_stmt.label {
                    Some(label) => Stmt::BreakLabel(self.ctx.krate.symbols.intern(label.name.as_str())),
                    None => Stmt::Break,
                };
                body.push_stmt_to_block(block, stmt);
                Ok(())
            }
            Statement::ContinueStatement(continue_stmt) => {
                let stmt = match &continue_stmt.label {
                    Some(label) => Stmt::ContinueLabel(self.ctx.krate.symbols.intern(label.name.as_str())),
                    None => Stmt::Continue,
                };
                body.push_stmt_to_block(block, stmt);
                Ok(())
            }
            // `label: statement`. The labelled statement lowers into this same
            // block between two markers, so it is lowered exactly as it would
            // be without its label; MIR resolves `break label` to the end
            // marker and `continue label` to the loop the region opens.
            Statement::LabeledStatement(labeled) => {
                let label = self.ctx.krate.symbols.intern(labeled.label.name.as_str());
                let loops = matches!(
                    labeled.body,
                    Statement::ForStatement(_)
                        | Statement::ForInStatement(_)
                        | Statement::ForOfStatement(_)
                        | Statement::WhileStatement(_)
                        | Statement::DoWhileStatement(_)
                );
                body.push_stmt_to_block(block, Stmt::LabelStart { label, loops });
                self.statement_in_block(&labeled.body, body, block)?;
                body.push_stmt_to_block(block, Stmt::LabelEnd(label));
                Ok(())
            }
            Statement::ThrowStatement(throw_stmt) => {
                let expr = self.expression(&throw_stmt.argument, body)?;
                body.push_stmt_to_block(block, Stmt::Throw(expr));
                Ok(())
            }
            Statement::TryStatement(try_stmt) => {
                // Neither block is known to run to its end: a throw leaves the
                // `try` part-way, and the `catch` runs only on a throw. So a
                // narrowing either block establishes ends with it, and a name
                // either block assigns loses the narrowing it had before the
                // `try` (`let x: T | undefined; try { x = f() } catch {}` leaves
                // `x` possibly absent).
                let try_body = self.with_narrowing(Some(std::collections::HashMap::new()), |this| {
                    this.block_from_block_statement(&try_stmt.block, body)
                })?;
                let (catch_binding, catch_body) = if let Some(handler) = &try_stmt.handler {
                    let (catch_binding, catch_body) = self.with_lexical_scope(|this| {
                        this.with_narrowing(Some(std::collections::HashMap::new()), |this| {
                            let catch_binding = handler
                                .param
                                .as_ref()
                                .map(|param| this.catch_binding(param, body))
                                .transpose()?;
                            let catch_body = this.block_from_block_statement(&handler.body, body)?;
                            Ok::<_, SmeltError>((catch_binding, catch_body))
                        })
                    })?;
                    (catch_binding, Some(catch_body))
                } else {
                    (None, None)
                };
                let mut assigned = AssignedNames::default();
                oxc::ast_visit::Visit::visit_block_statement(&mut assigned, &try_stmt.block);
                if let Some(handler) = &try_stmt.handler {
                    oxc::ast_visit::Visit::visit_block_statement(&mut assigned, &handler.body);
                }
                for name in assigned.names {
                    if self.narrowed_type(&name).is_some()
                        && let Some(local) = self.scope.lookup(&name)
                    {
                        let declared = Self::local_ty(body, local);
                        self.apply_narrowing(name, declared);
                    }
                }
                let finally_body = try_stmt
                    .finalizer
                    .as_ref()
                    .map(|finalizer| self.block_from_block_statement(finalizer, body))
                    .transpose()?;

                body.push_stmt_to_block(
                    block,
                    Stmt::TryCatch {
                        body: try_body,
                        catch_binding,
                        catch_body,
                        finally_body,
                    },
                );
                Ok(())
            }
            Statement::BlockStatement(block_stmt) => {
                // A nested `function` declaration is bound for its whole BLOCK,
                // not just from its textual position, so it lowers before the
                // first sibling that mentions it (`lowering::hoisting`).
                let saved = self.scope.snapshot_bindings();
                let lowered = crate::lowering::hoisting::hoisted_statements(&block_stmt.body)
                    .into_iter()
                    .try_for_each(|child| self.statement_in_block(child, body, block));
                // Its `let`/`const` bindings end with it, like any block's.
                self.scope
                    .close_lexical_block(&saved, &Self::lexically_declared_names(&block_stmt.body));
                lowered
            }
            _ => Err(SmeltError::unsupported(
                self.statement_span(statement),
                format!("statement kind is not lowered yet: {statement:?}"),
            )),
        }
    }

    /// Bind a `for` loop's left side and lower its body, as `(pattern, body block)`.
    ///
    /// A destructuring left side binds a synthetic pattern and destructures it as
    /// the first statements of the loop body.
    fn for_binding_and_body(
        &mut self,
        left: &oxc::ast::ast::ForStatementLeft<'_>,
        loop_statement: &Statement<'_>,
        iter: smelt_hir::ExprId,
        body: &mut Body,
    ) -> Result<(smelt_hir::PatternId, smelt_hir::BlockId), SmeltError> {
        let iter_ty = Self::expr_ty(body, iter);
        let Some((pat, value, binding, annotated_ty, mutable)) =
            self.for_left_destructuring(left, iter_ty, body)?
        else {
            let pat = self.for_left_pattern(left, iter_ty, body)?;
            return Ok((pat, self.block_from_statement(loop_statement, body)?));
        };
        let loop_body = body.push_block(self.statement_span(loop_statement));
        self.binding_declaration(binding, Some(value), annotated_ty, mutable, body, loop_body)?;
        if let Statement::BlockStatement(block_stmt) = loop_statement {
            for nested_statement in &block_stmt.body {
                self.statement_in_block(nested_statement, body, loop_body)?;
            }
        } else {
            self.statement_in_block(loop_statement, body, loop_body)?;
        }
        Ok((pat, loop_body))
    }

    /// Lower an expression in statement position: an expression statement, or
    /// one element of a statement-position comma expression.
    ///
    /// The one definition of the expression-statement dispatch. Each element of
    /// a comma expression is a complete JavaScript expression statement for
    /// side-effect purposes, so it takes the same interceptors as an ordinary
    /// statement: assignments, updates, test assertions, async spawning and
    /// narrowing keep their normal semantics and source order. A copy of this
    /// dispatch for comma elements once lacked the logical-assignment branch,
    /// so `(d ??= side(), b = 1)` ran `side()` even when `d` was set and
    /// `(rec[k] ??= 2, b = 1)` never stored.
    ///
    /// `expression_span` locates the statement (the whole expression statement
    /// at top level, the element inside a comma expression).
    pub(in crate::lowering) fn expression_statement(
        &mut self,
        expression: &Expression<'_>,
        expression_span: oxc::span::Span,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let expression = peel(expression, Wrappers::Parens);
        // `void X;` discards X's result, which a statement does anyway: lower
        // it as the statement `X;`, so a floating promise it starts
        // (`void handle(req, res);`) is driven exactly as a bare `handle(..);`
        // is, instead of being built and dropped unpolled.
        if let Expression::UnaryExpression(unary) = expression
            && unary.operator == UnaryOperator::Void
        {
            return self.expression_statement(&unary.argument, unary.argument.span(), body, block);
        }
        if let Expression::SequenceExpression(sequence) = expression {
            for child in &sequence.expressions {
                self.expression_statement(child, child.span(), body, block)?;
            }
            return Ok(());
        }
        // `Foo.prototype.x = …` assignments for a constructor function were
        // folded into the synthesized class during the block prepass, so the
        // assignment statement itself is dropped.
        if self.is_synthesized_prototype_assignment(expression, body)
            || self.inline_runtime_lifecycle_setup(expression, body, block)?
            || self.is_test_framework_statement(expression)
            || Self::is_vitest_mock_statement(expression)
            || Self::is_top_level_dynamic_import_await(expression)
        {
            return Ok(());
        }
        if let Expression::CallExpression(call) = expression {
            if self.for_each_statement(call, body, block)?
                || self.expect_matcher_statement(call, body)?
                || self.node_assert_statement(call, body)?
            {
                return Ok(());
            }
        }
        if self.assertion_library_statement(expression, body)? {
            return Ok(());
        }
        if let Expression::AssignmentExpression(assign) = expression {
            // A write to a lifted mutable global desugars to a `GlobalSet`
            // expression statement; it must intercept before the discard-only
            // module-global path below.
            if let Some(set) = self.try_global_assignment_expression(assign, body)? {
                body.push_stmt_to_block(block, Stmt::Expr(set));
                return Ok(());
            }
            if block == body.root && self.module_global_assignment_statement(assign, body, block)? {
                return Ok(());
            }
            if self.try_lower_negative_bracket_write_statement(assign, body, block)?
                || self.try_lower_list_length_assignment_statement(assign, body, block)?
                || self.array_destructuring_assignment_statement(assign, body, block)?
                || self.try_collect_callable_local_prop(assign, body, block)?
            {
                return Ok(());
            }
            // A logical assignment stores conditionally; in statement position
            // its value is discarded, so it needs neither a result temporary nor
            // an else arm.
            if Self::is_logical_assignment_operator(assign.operator) {
                self.lower_logical_assignment(assign, body, Some(block), false)?;
                return Ok(());
            }
            let (target, value) = self.assignment_parts(assign, body)?;
            body.push_stmt_to_block(block, Stmt::Assign { target, value });
            return Ok(());
        }
        if let Expression::UpdateExpression(update) = expression {
            // Statement-position `++`/`--` of a lifted mutable global discards
            // its result, so the old-value temp is skipped.
            if let Some(set) = self.try_global_update_expression(update, body, false)? {
                body.push_stmt_to_block(block, Stmt::Expr(set));
                return Ok(());
            }
            let (target, value) = self.update_parts(update, body)?;
            body.push_stmt_to_block(block, Stmt::Assign { target, value });
            return Ok(());
        }
        if let Expression::YieldExpression(yield_expr) = expression
            && self.generator_yield_statement(yield_expr, body, block)?
        {
            return Ok(());
        }
        let assertion_narrowing = self.assertion_call_narrowing(expression, body);
        let value = self.expression(expression, body)?;
        // A property read of an erased receiver can run a getter or a proxy
        // trap, so a bare `value.x;` is bound rather than dropped as unused.
        if matches!(expression.get_inner_expression(), Expression::StaticMemberExpression(_) | Expression::ComputedMemberExpression(_))
            && matches!(self.ctx.krate.types.get(Self::expr_ty(body, value)), Some(Type::Unknown))
        {
            let ty = Self::expr_ty(body, value);
            let span = self.span(expression_span.start, expression_span.end);
            let name = self.intern_source_name("smelt_discarded_read");
            let local = body.push_local(smelt_hir::LocalDecl { name: Some(name), ty, mutable: false, span });
            let pat = body.push_pattern(smelt_hir::Pattern::Binding(local));
            body.push_stmt_to_block(block, Stmt::Let { pat, ty, value: Some(value) });
            return Ok(());
        }
        let value = if matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, value)),
            Some(Type::Future(_))
        ) {
            let none_ty = self.ctx.krate.types.intern(Type::None);
            body.push_expr(Expr {
                kind: ExprKind::AsyncOp {
                    op: AsyncOp::SpawnLocal,
                    args: vec![value],
                },
                ty: none_ty,
                span: self.span(expression_span.start, expression_span.end),
            })
        } else {
            value
        };
        body.push_stmt_to_block(block, Stmt::Expr(value));
        if let Some((name, target)) = assertion_narrowing {
            self.apply_narrowing(name, target);
        }
        Ok(())
    }

    /// Lower writes to known module-level variables without requiring a local target.
    pub(in crate::lowering) fn module_global_assignment_statement(
        &mut self,
        assign: &oxc::ast::ast::AssignmentExpression<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        let AssignmentTarget::AssignmentTargetIdentifier(target) = &assign.left else {
            return Ok(false);
        };
        if assign.operator != AssignmentOperator::Assign {
            return Ok(false);
        }
        if !self.module_globals.contains_key(target.name.as_str()) {
            return Ok(false);
        }
        // `module_globals` records the declared TYPE of every annotated or
        // literal-initialized module-level binding so a function body can look
        // it up; membership does NOT mean the name has no storage. When the
        // module body has a local for it -- which is the case for every
        // module-scope `let`/`var` -- the write must be a real assignment to
        // that local. Discarding it unconditionally made
        // `let n: number | undefined; n = 5; console.log(n)` print `undefined`,
        // and `let m: number = 0; m = 6` print `0`: the RHS was evaluated for
        // its side effects and thrown away. Only a name with no local binding
        // (an inlined const item, an ambient declaration) reaches the discard,
        // which is what the path was for.
        if self.scope.is_bound(target.name.as_str()) {
            return Ok(false);
        }
        let value = self.expression(&assign.right, body)?;
        body.push_stmt_to_block(block, Stmt::Expr(value));
        Ok(true)
    }

    /// Lower `while ((target = value) !== null)` without dropping the assignment.
    pub(in crate::lowering) fn while_assignment_condition_body(
        &mut self,
        while_stmt: &oxc::ast::ast::WhileStatement<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<
        Option<(
            smelt_hir::ExprId,
            smelt_hir::BlockId,
            smelt_hir::ExprId,
            smelt_hir::ExprId,
        )>,
        SmeltError,
    > {
        let test = peel(&while_stmt.test, Wrappers::Parens);
        let Expression::BinaryExpression(binary) = test else {
            return Ok(None);
        };
        if !matches!(
            binary.operator,
            BinaryOperator::StrictInequality | BinaryOperator::Inequality
        ) || !matches!(&binary.right, Expression::NullLiteral(_))
        {
            return Ok(None);
        }
        let left = peel(&binary.left, Wrappers::Parens);
        let Expression::AssignmentExpression(assign) = left else {
            return Ok(None);
        };
        let (target, value) = self.assignment_parts(assign, body)?;
        body.push_stmt_to_block(block, Stmt::Assign { target, value });
        let null_expr = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty: self.ctx.krate.types.intern(Type::None),
            span: self.span(binary.right.span().start, binary.right.span().end),
        });
        let cond = body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: BinOp::NotEq,
                lhs: target,
                rhs: null_expr,
            },
            ty: self.ctx.krate.types.intern(Type::Bool),
            span: self.span(binary.span.start, binary.span.end),
        });
        let loop_body = body.push_block(self.statement_span(&while_stmt.body));
        if let Statement::BlockStatement(block_stmt) = &while_stmt.body {
            for statement in &block_stmt.body {
                self.statement_in_block(statement, body, loop_body)?;
            }
        } else {
            self.statement_in_block(&while_stmt.body, body, loop_body)?;
        }
        let (update_target, update_value) = self.assignment_parts(assign, body)?;
        Ok(Some((cond, loop_body, update_target, update_value)))
    }

    /// Return whether this is a top-level `await import("...")` side-effect load.
    pub(in crate::lowering) fn is_top_level_dynamic_import_await(
        expression: &Expression<'_>,
    ) -> bool {
        let Expression::AwaitExpression(await_expr) = expression else {
            return false;
        };
        matches!(&await_expr.argument, Expression::ImportExpression(_))
    }

    /// Evaluate `export default <expression>` in the module body.
    ///
    /// A name, a function, a class or a literal runs no code when evaluated, so
    /// only an expression that can have effects is lowered.
    pub(in crate::lowering) fn export_default_statement(
        &mut self,
        export: &oxc::ast::ast::ExportDefaultDeclaration<'_>,
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        let Some(expression) = export.declaration.as_expression() else {
            return Ok(());
        };
        if matches!(
            peel(expression, Wrappers::Parens),
            Expression::Identifier(_)
                | Expression::ArrowFunctionExpression(_)
                | Expression::FunctionExpression(_)
                | Expression::ClassExpression(_)
                | Expression::NullLiteral(_)
                | Expression::BooleanLiteral(_)
                | Expression::NumericLiteral(_)
                | Expression::StringLiteral(_)
        ) {
            return Ok(());
        }
        let block = body.root;
        self.with_stmt_block(block, |this| {
            this.expression_statement(expression, export.span, body, block)
        })
    }

    /// Lower a statement in the current scope.
    pub(in crate::lowering) fn statement(&mut self, statement: &Statement<'_>, body: &mut Body) -> Result<(), SmeltError> {
        self.statement_in_block(statement, body, body.root)
    }
}

/// Every identifier an AST subtree assigns or updates, at any depth.
#[derive(Default)]
struct AssignedNames {
    /// The assigned names, without duplicates.
    names: Vec<String>,
}

impl<'a> oxc::ast_visit::Visit<'a> for AssignedNames {
    /// Records the identifier an assignment writes.
    fn visit_assignment_expression(&mut self, assign: &oxc::ast::ast::AssignmentExpression<'a>) {
        if let AssignmentTarget::AssignmentTargetIdentifier(target) = &assign.left {
            let name = target.name.to_string();
            if !self.names.contains(&name) {
                self.names.push(name);
            }
        }
        oxc::ast_visit::walk::walk_assignment_expression(self, assign);
    }

    /// Records the identifier an update (`x++`) writes.
    fn visit_update_expression(&mut self, update: &oxc::ast::ast::UpdateExpression<'a>) {
        if let oxc::ast::ast::SimpleAssignmentTarget::AssignmentTargetIdentifier(target) =
            &update.argument
        {
            let name = target.name.to_string();
            if !self.names.contains(&name) {
                self.names.push(name);
            }
        }
        oxc::ast_visit::walk::walk_update_expression(self, update);
    }
}
