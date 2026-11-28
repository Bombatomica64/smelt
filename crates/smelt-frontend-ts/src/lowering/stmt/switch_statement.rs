//! `switch` statements that lower to a HIR `Match` on literal labels.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::support::statement_terminates;
use oxc::ast::ast::Statement;
use smelt_hir::{Body, MatchArm, Stmt};

impl ModuleBuilder<'_> {
    /// Lower a `switch` whose cases each end (no fallthrough) and whose labels
    /// are literals the scrutinee compares with as a HIR `Match`, one arm per
    /// case. With `switch_break_label` set (a `break` below a
    /// case's top level), the `Match` sits in a labelled region that `break`
    /// leaves.
    pub(in crate::lowering) fn match_switch_statement(
        &mut self,
        switch_stmt: &oxc::ast::ast::SwitchStatement<'_>,
        scrutinee: smelt_hir::ExprId,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        // A switch on a discriminant property (`switch (x.kind)`) proves,
        // inside each labeled arm, that `x.kind` equals one of that arm's
        // labels, so `x` narrows to the union arms whose recorded literals
        // admit one (`switch_discriminant_narrowing`). The `default` arm
        // proves that it equals none of the switch's labels, unless empty
        // cases fall into it, whose labels it then also handles.
        // Nullish/dynamic boundaries are left untouched: the fact only
        // projects concrete union arms.
        let all_labels = switch_stmt
            .cases
            .iter()
            .filter_map(|case| case.test.as_ref())
            .map(|test| self.literal_case_label(test).ok())
            .collect::<Option<Vec<_>>>();
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
                            let mut kinds: Vec<String> = group.into_iter().flatten().collect();
                            kinds.push(current);
                            self.switch_typeof_case_narrowing(name, &kinds, body)
                        }
                        _ => None,
                    }
                });
                let fact = typeof_fact.or_else(|| {
                    let mut labels = pending_empty_labels.clone();
                    labels.push(self.literal_case_label(case.test.as_ref()?).ok()?);
                    self.switch_discriminant_narrowing(
                        &switch_stmt.discriminant,
                        &labels,
                        true,
                        body,
                    )
                });
                fact.map(|(name, target)| std::collections::HashMap::from([(name, target)]))
            } else {
                // Drop any pending `typeof` group kinds that fell
                // through to the `default` arm: it proves no `typeof`
                // fact.
                pending_kinds.clear();
                all_labels
                    .as_ref()
                    .filter(|_| pending_empty_labels.is_empty())
                    .and_then(|labels| {
                        self.switch_discriminant_narrowing(
                            &switch_stmt.discriminant,
                            labels,
                            false,
                            body,
                        )
                    })
                    .map(|(name, target)| std::collections::HashMap::from([(name, target)]))
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
            // scan loop) was a blocker. The `===` chain used for real
            // fallthrough emits it the same way.
            let case_ends =
                self.with_narrowing(case_narrowing, |this| -> Result<bool, SmeltError> {
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
            if !saw_break && !is_last_case && !case.consequent.iter().any(statement_terminates) {
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

        let label = self.switch_break_label;
        if let Some(label) = label {
            body.push_stmt_to_block(
                block,
                Stmt::LabelStart {
                    label,
                    loops: false,
                },
            );
        }
        body.push_stmt_to_block(
            block,
            Stmt::Match {
                scrutinee,
                arms,
                default,
            },
        );
        if let Some(label) = label {
            body.push_stmt_to_block(block, Stmt::LabelEnd(label));
        }
        Ok(())
    }

    /// The label of the region a `switch`'s `break` leaves, unique per
    /// `switch` (its source offset).
    pub(in crate::lowering) fn switch_break_symbol(
        &mut self,
        switch_stmt: &oxc::ast::ast::SwitchStatement<'_>,
    ) -> smelt_hir::Symbol {
        self.ctx
            .krate
            .symbols
            .intern(&format!("__smelt_switch_break_{}", switch_stmt.span.start))
    }

    /// Whether a case of `switch_stmt` holds an unlabelled `break` below its
    /// top level (the statements of the case and of a block that is one of
    /// them), outside any loop or nested `switch` (whose `break` it would be).
    pub(in crate::lowering) fn switch_has_nested_break(
        switch_stmt: &oxc::ast::ast::SwitchStatement<'_>,
    ) -> bool {
        Self::switch_breaks(switch_stmt, false)
    }

    /// Whether a case of `switch_stmt` holds an unlabelled `break` that leaves
    /// it: [`Self::switch_has_nested_break`], plus (`top_level`) one at a
    /// case's top level, which leaves a `===` chain too (a `Match` arm ends by
    /// itself instead).
    pub(in crate::lowering) fn switch_breaks(
        switch_stmt: &oxc::ast::ast::SwitchStatement<'_>,
        top_level: bool,
    ) -> bool {
        switch_stmt.cases.iter().any(|case| {
            case.consequent.iter().any(|statement| match statement {
                Statement::BlockStatement(block) => block.body.iter().any(nested_break),
                Statement::BreakStatement(break_stmt) => top_level && break_stmt.label.is_none(),
                other => nested_break(other),
            })
        })
    }
}

/// Whether `statement` holds an unlabelled `break` that binds to an enclosing
/// `switch`: one not inside a loop, a nested `switch` or a function.
fn nested_break(statement: &Statement<'_>) -> bool {
    match statement {
        Statement::BreakStatement(break_stmt) => break_stmt.label.is_none(),
        Statement::BlockStatement(block) => block.body.iter().any(nested_break),
        Statement::IfStatement(if_stmt) => {
            nested_break(&if_stmt.consequent)
                || if_stmt.alternate.as_ref().is_some_and(nested_break)
        }
        Statement::TryStatement(try_stmt) => {
            try_stmt.block.body.iter().any(nested_break)
                || try_stmt
                    .handler
                    .as_ref()
                    .is_some_and(|handler| handler.body.body.iter().any(nested_break))
                || try_stmt
                    .finalizer
                    .as_ref()
                    .is_some_and(|finalizer| finalizer.body.iter().any(nested_break))
        }
        Statement::LabeledStatement(labeled) => nested_break(&labeled.body),
        _ => false,
    }
}
