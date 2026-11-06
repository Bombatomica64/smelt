//! Statement blocks: building a HIR block from a statement and closing its lexical
//! scope.

use crate::lowering::ModuleBuilder;
use crate::SmeltError;
use oxc::ast::ast::Statement;
use smelt_hir::Body;

impl ModuleBuilder<'_> {
    /// Create a block from a statement (wrapping if needed).
    pub(in crate::lowering) fn block_from_statement(
        &mut self,
        statement: &Statement<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::BlockId, SmeltError> {
        let span = self.statement_span(statement);
        let block = body.push_block(span);
        if let Statement::BlockStatement(block_stmt) = statement {
            let saved = self.scope.snapshot_bindings();
            // A nested `function` declaration is bound for its whole BLOCK, not
            // just from its textual position (`lowering::hoisting`).
            for nested_statement in crate::lowering::hoisting::hoisted_statements(&block_stmt.body)
            {
                self.statement_in_block(nested_statement, body, block)?;
            }
            self.scope
                .close_lexical_block(&saved, &Self::lexically_declared_names(&block_stmt.body));
        } else {
            self.statement_in_block(statement, body, block)?;
        }
        Ok(block)
    }

    /// The names a statement list declares LEXICALLY at its own level:
    /// `let`/`const` declarators (not `var`, which is function-scoped) and a
    /// nested `for (let ..)` head's bindings are handled by the loop itself.
    /// Used to close the block's scope ([`LocalScope::close_lexical_block`]).
    pub(in crate::lowering) fn lexically_declared_names(statements: &[Statement<'_>]) -> Vec<String> {
        let mut names = Vec::new();
        for statement in statements {
            if let Statement::VariableDeclaration(decl) = statement
                && !matches!(decl.kind, oxc::ast::ast::VariableDeclarationKind::Var)
            {
                for declarator in &decl.declarations {
                    Self::binding_pattern_names(&declarator.id, &mut names);
                }
            }
        }
        names
    }

    /// Create a HIR block from a JavaScript block statement.
    pub(in crate::lowering) fn block_from_block_statement(
        &mut self,
        block_stmt: &oxc::ast::ast::BlockStatement<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::BlockId, SmeltError> {
        let block = body.push_block(self.span(block_stmt.span.start, block_stmt.span.end));
        let saved = self.scope.snapshot_bindings();
        for statement in crate::lowering::hoisting::hoisted_statements(&block_stmt.body) {
            self.statement_in_block(statement, body, block)?;
        }
        self.scope
            .close_lexical_block(&saved, &Self::lexically_declared_names(&block_stmt.body));
        Ok(block)
    }
}
