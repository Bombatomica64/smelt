//! Statement-order hoisting for nested `function` declarations.
//!
//! JavaScript binds a function declaration for the WHOLE scope it is declared
//! in, so a statement that textually precedes the declaration may call it:
//!
//! ```ignore
//! function hoisted(base: number): number {
//!   const scale = 3
//!   return step(base)
//!   function step(value: number): number { return value * scale }
//! }
//! ```
//!
//! Smelt lowers such a declaration to a local closure bound by a `let`, and a
//! `let` after a `return` is dead code — so the binding disappeared from the
//! generated Rust and the call referenced a name nothing declared.
//!
//! The fix is an ORDER, not a rewrite: a declaration is lowered just before the
//! first sibling statement that mentions its name. Hoisting it unconditionally
//! to the top of the block would be wrong for Smelt specifically, because a
//! closure captures by value where it is created: moving `step` above
//! `const scale = 3` would lose the binding it closes over. "Before the first
//! use" keeps every existing capture and is still the general rule, because
//! hoisting is only observable when a use precedes the declaration.
//!
//! Relative order is otherwise preserved, including between two declarations
//! hoisted to the same position.

use oxc::ast::ast::Statement;
use std::collections::HashSet;

/// Collects the identifier names a statement REFERENCES.
///
/// A reference, not a binding: `function step(){}`'s own name is a binding
/// identifier and does not count as a use of itself, which is what keeps a
/// declaration from being hoisted above itself.
struct ReferencedNameCollector<'name> {
    /// The name being looked for.
    name: &'name str,
    /// Whether it was referenced anywhere in the visited statement.
    found: bool,
}

impl<'a> oxc::ast_visit::Visit<'a> for ReferencedNameCollector<'_> {
    fn visit_identifier_reference(
        &mut self,
        identifier: &oxc::ast::ast::IdentifierReference<'a>,
    ) {
        if identifier.name.as_str() == self.name {
            self.found = true;
        }
    }
}

/// Whether `statement` references `name` anywhere inside it.
fn statement_references(statement: &Statement<'_>, name: &str) -> bool {
    use oxc::ast_visit::Visit;
    let mut collector = ReferencedNameCollector { name, found: false };
    collector.visit_statement(statement);
    collector.found
}

/// Return `statements` in the order they must be LOWERED.
///
/// Every element of `statements` appears exactly once. The order differs from
/// source order only when a nested `function` declaration is referenced by an
/// earlier sibling, in which case the declaration moves to just before the
/// first such sibling; see the module docs for why that position and not the
/// top of the block.
pub(in crate::lowering) fn hoisted_statements<'a, 'src>(
    statements: &'a [Statement<'src>],
) -> Vec<&'a Statement<'src>> {
    // The names declared by sibling `function` declarations, paired with the
    // index of the first earlier statement that mentions each one. Computing
    // this first keeps the scan off every block that has no declaration at all,
    // which is almost all of them.
    if std::env::var_os("SMELT_NO_HOIST").is_some() { return statements.iter().collect(); }
    let mut hoists: Vec<(usize, usize)> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for (index, statement) in statements.iter().enumerate() {
        let Statement::FunctionDeclaration(function) = statement else {
            continue;
        };
        let Some(id) = &function.id else {
            continue;
        };
        // Two declarations of one name: the LAST one wins in JavaScript, and it
        // is the one already lowered last, so only the first hoist is recorded
        // and the later declaration keeps its place.
        if !seen.insert(id.name.as_str()) {
            continue;
        }
        let name = id.name.as_str();
        if let Some(target) = statements[..index]
            .iter()
            .position(|earlier| statement_references(earlier, name))
        {
            hoists.push((index, target));
        }
    }
    if hoists.is_empty() {
        return statements.iter().collect();
    }

    let mut ordered = Vec::with_capacity(statements.len());
    for (index, statement) in statements.iter().enumerate() {
        // Declarations hoisted TO this position come first, in source order.
        for &(declaration, target) in &hoists {
            if target == index {
                ordered.push(&statements[declaration]);
            }
        }
        if hoists.iter().any(|&(declaration, _)| declaration == index) {
            continue;
        }
        ordered.push(statement);
    }
    ordered
}

#[cfg(test)]
mod tests {
    use super::hoisted_statements;
    use oxc::allocator::Allocator;
    use oxc::parser::Parser;
    use oxc::span::SourceType;

    /// Parse a program and return its statements in hoisted order, rendered as
    /// the leading token of each statement so the order is readable.
    fn order(source: &str) -> Vec<String> {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
        hoisted_statements(&parsed.program.body)
            .into_iter()
            .map(|statement| {
                let span = oxc::span::GetSpan::span(statement);
                source[span.start as usize..span.end as usize]
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn source_order_is_kept_when_nothing_uses_a_later_declaration() {
        assert_eq!(
            order("const a = 1; function f() { return a }; f()"),
            vec!["const a = 1;", "function f() { return a }", ";", "f()"]
        );
    }

    #[test]
    fn a_declaration_moves_before_the_first_statement_that_uses_it() {
        assert_eq!(
            order("const a = 1; return f(a); function f(x: number) { return x }"),
            vec![
                "const a = 1;",
                "function f(x: number) { return x }",
                "return f(a);"
            ]
        );
    }

    #[test]
    fn a_self_recursive_declaration_is_not_hoisted_above_itself() {
        assert_eq!(
            order("const a = 1; function f(x: number) { return f(x) }"),
            vec!["const a = 1;", "function f(x: number) { return f(x) }"]
        );
    }
}
