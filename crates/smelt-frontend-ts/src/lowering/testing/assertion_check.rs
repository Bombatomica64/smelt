//! The one HIR shape every assertion lowers to: a boolean that, when it does
//! not agree with the assertion's polarity, fails the running test with a
//! located message.
//!
//! vitest matchers, chai and `node:assert` recognizers, and the `toThrow`
//! predicate all end here; nothing here knows any library's vocabulary.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use smelt_hir::{Body, ErasureReason, Expr, ExprId, ExprKind, Literal, Stmt, Type, UnaryOp};

impl ModuleBuilder<'_> {
    /// Fail unless the erased runtime check `name` holds for `actual`.
    #[expect(
        clippy::too_many_arguments,
        reason = "each is one independent part of the source assertion"
    )]
    pub(in crate::lowering) fn runtime_check(
        &mut self,
        name: &str,
        actual: ExprId,
        arguments: &[ExprId],
        inverted: bool,
        span: oxc::span::Span,
        message: &str,
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        let holds = self.runtime_check_expr(name, actual, arguments, span, body);
        self.assertion_holds(holds, inverted, message, span, body);
        Ok(())
    }

    /// The boolean answer of the erased runtime check `name`.
    pub(in crate::lowering) fn runtime_check_expr(
        &mut self,
        name: &str,
        actual: ExprId,
        arguments: &[ExprId],
        span: oxc::span::Span,
        body: &mut Body,
    ) -> ExprId {
        let hir_span = self.span(span.start, span.end);
        let unknown_ty = self.erase_with(ErasureReason::TestHarness, "assertion library check");
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let mut args = vec![Self::erase_to_unknown(actual, unknown_ty, hir_span, body)];
        args.extend(
            arguments
                .iter()
                .map(|argument| Self::erase_to_unknown(*argument, unknown_ty, hir_span, body)),
        );
        body.push_expr(Expr {
            kind: ExprKind::TestRuntimeCheck {
                check: name.to_owned(),
                args,
            },
            ty: bool_ty,
            span: hir_span,
        })
    }

    /// Throw `message` unless `holds` agrees with the assertion's polarity.
    pub(in crate::lowering) fn assertion_holds(
        &mut self,
        holds: ExprId,
        inverted: bool,
        message: &str,
        span: oxc::span::Span,
        body: &mut Body,
    ) {
        let failed = if inverted {
            holds
        } else {
            self.unary_bool_expr(UnaryOp::Not, holds, span, body)
        };
        self.push_test_failure_if(failed, &format!("{message} failed"), span, body);
    }

    /// An assertion that fails whenever it runs (`assert.fail`).
    pub(in crate::lowering) fn always_failing_assertion(
        &mut self,
        message: &str,
        span: oxc::span::Span,
        body: &mut Body,
    ) {
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let hir_span = self.span(span.start, span.end);
        let always = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(true)),
            ty: bool_ty,
            span: hir_span,
        });
        self.push_test_failure_if(always, message, span, body);
    }

    /// Report an assertion this model has no rule for by failing where it runs.
    pub(in crate::lowering) fn unmodelled_assertion(
        &mut self,
        what: &str,
        span: oxc::span::Span,
        body: &mut Body,
    ) {
        self.always_failing_assertion(&format!("{what} is not modelled"), span, body);
    }

    /// Push a throwing failure block guarded by a boolean condition.
    pub(in crate::lowering) fn push_test_failure_if(
        &mut self,
        cond: ExprId,
        message: &str,
        span: oxc::span::Span,
        body: &mut Body,
    ) {
        let failure_block = body.push_block(self.span(span.start, span.end));
        let message = self.test_failure_message(message, span);
        let message = self.string_literal_expr(&message, span, body);
        body.push_stmt_to_block(failure_block, Stmt::Throw(message));
        let failure_stmt = Stmt::If {
            cond,
            then_block: failure_block,
            else_block: None,
        };
        self.push_stmt_to_sink(body, failure_stmt);
    }

    /// Build the runtime message thrown by a failed synthesized assertion.
    ///
    /// A generated assertion throws a plain string, so on its own a failing
    /// suite reports only which matcher failed -- with hundreds of generated
    /// tests that is not enough to find the source assertion. Vitest prints
    /// the failing expression and its location, so this appends the same two
    /// things: the source text of the assertion call and `path:line:column`.
    /// The matcher prefix is left untouched so callers that match on it keep
    /// working.
    fn test_failure_message(&self, message: &str, span: oxc::span::Span) -> String {
        let (line, column) = self.line_column(span.start);
        let path = self.path.as_str();
        self.assertion_snippet(span).map_or_else(
            || format!("{message} ({path}:{line}:{column})"),
            |snippet| format!("{message}: {snippet} ({path}:{line}:{column})"),
        )
    }

    /// Resolve a byte offset in this module's source to a 1-based line and column.
    ///
    /// Columns count UTF-8 characters rather than bytes so the location lines
    /// up with what an editor shows for non-ASCII spec sources.
    fn line_column(&self, offset: u32) -> (usize, usize) {
        let offset = usize::try_from(offset).unwrap_or(usize::MAX);
        let Some(prefix) = self.source.get(..offset) else {
            return (0, 0);
        };
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let column = prefix
            .rsplit_once('\n')
            .map_or(prefix, |(_, last)| last)
            .chars()
            .count()
            + 1;
        (line, column)
    }

    /// Render the source text of an assertion as a single-line message fragment.
    ///
    /// Newlines and runs of whitespace are collapsed so a multi-line `expect`
    /// call still reads as one line, and long assertions are truncated on a
    /// character boundary to keep failure output scannable.
    fn assertion_snippet(&self, span: oxc::span::Span) -> Option<String> {
        const MAX_CHARS: usize = 120;
        let start = usize::try_from(span.start).ok()?;
        let end = usize::try_from(span.end).ok()?;
        let text = self.source.get(start..end)?;
        let mut snippet = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if snippet.is_empty() {
            return None;
        }
        if snippet.chars().count() > MAX_CHARS {
            let cut = snippet
                .char_indices()
                .nth(MAX_CHARS)
                .map_or(snippet.len(), |(index, _)| index);
            snippet.truncate(cut);
            snippet.push_str("...");
        }
        Some(snippet)
    }

    /// Create a boolean unary expression for synthesized test assertions.
    pub(in crate::lowering) fn unary_bool_expr(
        &mut self,
        op: UnaryOp,
        operand: ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> ExprId {
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        body.push_expr(Expr {
            kind: ExprKind::UnaryOp { op, operand },
            ty: bool_ty,
            span: self.span(span.start, span.end),
        })
    }

    /// Create a string literal expression for synthesized test diagnostics.
    pub(in crate::lowering) fn string_literal_expr(
        &mut self,
        value: &str,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> ExprId {
        let ty = self.ctx.krate.types.intern(Type::String);
        body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(value.to_owned())),
            ty,
            span: self.span(span.start, span.end),
        })
    }
}
