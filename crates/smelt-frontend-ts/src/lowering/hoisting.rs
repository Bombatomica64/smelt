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
//! The position is transitive. A hoisted declaration that calls a LATER
//! sibling declaration needs that sibling lowered before it, because the
//! closure captures it where it is created:
//!
//! ```ignore
//! const r = f(2)
//! function f(x) { return g(x) * 10 }
//! function g(x) { return x + 1 }
//! ```
//!
//! `f` moves above `const r`, so `g` must move there too, ahead of `f`.
//! Targets are therefore propagated over the reference graph of the sibling
//! declarations, and the declarations hoisted to one position are emitted
//! dependencies first.
//!
//! The work is linear in the block: each statement is walked once for every
//! sibling name at a time ([`super::scoped_references::statement_name_uses`]),
//! the references form a graph, and targets propagate along it in one
//! Dijkstra-style pass (every edge has weight zero and a target only takes
//! the minimum, so each declaration is settled once).
//!
//! "Mentions" means a reference that resolves to the sibling declaration, as
//! decided by [`super::scoped_references`]: a parameter or nested declaration
//! of the same name is a different binding and does not move anything.
//!
//! Relative order is otherwise preserved, including between two declarations
//! hoisted to the same position that do not depend on each other. Mutually
//! recursive declarations keep source order between themselves.

use oxc::ast::ast::Statement;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use super::scoped_references::statement_name_uses;

/// One sibling `function` declaration that may move.
struct Declaration<'name> {
    /// Its statement index.
    index: usize,
    /// Its bound name.
    name: &'name str,
    /// Where it is lowered: its own index, or the index of the earliest
    /// statement that must see it.
    target: usize,
}

/// The first sibling `function` declaration of each name in `statements`.
///
/// Two declarations of one name: the LAST one wins in JavaScript, and it is
/// the one already lowered last, so only the first may be hoisted and the
/// later declaration keeps its place.
fn sibling_declarations<'name>(statements: &'name [Statement<'_>]) -> Vec<Declaration<'name>> {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut declarations = Vec::new();
    for (index, statement) in statements.iter().enumerate() {
        let Statement::FunctionDeclaration(function) = statement else {
            continue;
        };
        let Some(id) = &function.id else {
            continue;
        };
        if seen.insert(id.name.as_str()) {
            declarations.push(Declaration { index, name: id.name.as_str(), target: index });
        }
    }
    declarations
}

/// For each statement, the sibling declarations (by position in
/// `declarations`) it refers to: one scoped walk per statement.
fn reference_graph(statements: &[Statement<'_>], declarations: &[Declaration<'_>]) -> Vec<Vec<usize>> {
    let names: HashSet<&str> = declarations.iter().map(|declaration| declaration.name).collect();
    let by_name: HashMap<&str, usize> =
        declarations.iter().enumerate().map(|(d, declaration)| (declaration.name, d)).collect();
    statements
        .iter()
        .map(|statement| {
            let mut referenced: Vec<usize> = statement_name_uses(statement, &names)
                .referenced
                .iter()
                .filter_map(|name| by_name.get(name.as_str()).copied())
                .collect();
            referenced.sort_unstable();
            referenced
        })
        .collect()
}

/// Return `statements` in the order they must be LOWERED.
///
/// Every element of `statements` appears exactly once. The order differs from
/// source order only when a nested `function` declaration must be visible to
/// an earlier statement (directly, or through another hoisted declaration); see
/// the module docs for the position it moves to and why.
pub(in crate::lowering) fn hoisted_statements<'a, 'src>(
    statements: &'a [Statement<'src>],
) -> Vec<&'a Statement<'src>> {
    let mut declarations = sibling_declarations(statements);
    if declarations.is_empty() {
        return statements.iter().collect();
    }
    let refers = reference_graph(statements, &declarations);
    let declaration_at: HashMap<usize, usize> =
        declarations.iter().enumerate().map(|(d, declaration)| (declaration.index, d)).collect();
    // A plain statement is lowered where it stands, so it seeds the target of
    // every declaration it refers to; a declaration referring to another
    // passes on its own target, settled below.
    for (s, referenced) in refers.iter().enumerate() {
        if declaration_at.contains_key(&s) {
            continue;
        }
        for &d in referenced {
            if let Some(declaration) = declarations.get_mut(d) {
                declaration.target = declaration.target.min(s);
            }
        }
    }
    let mut queue: BinaryHeap<Reverse<(usize, usize)>> =
        declarations.iter().enumerate().map(|(d, declaration)| Reverse((declaration.target, d))).collect();
    let mut settled = vec![false; declarations.len()];
    while let Some(Reverse((target, e))) = queue.pop() {
        if settled.get(e).copied().unwrap_or(true) {
            continue;
        }
        if let Some(flag) = settled.get_mut(e) {
            *flag = true;
        }
        let Some(index) = declarations.get(e).map(|declaration| declaration.index) else {
            continue;
        };
        for &d in refers.get(index).map_or(&[][..], Vec::as_slice) {
            if d == e {
                continue;
            }
            if let Some(declaration) = declarations.get_mut(d)
                && target < declaration.target
            {
                declaration.target = target;
                queue.push(Reverse((target, d)));
            }
        }
    }
    if declarations.iter().all(|declaration| declaration.target == declaration.index) {
        return statements.iter().collect();
    }

    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for (d, declaration) in declarations.iter().enumerate() {
        if declaration.target != declaration.index {
            groups.entry(declaration.target).or_default().push(d);
        }
    }
    let mut ordered = Vec::with_capacity(statements.len());
    for (index, statement) in statements.iter().enumerate() {
        if let Some(group) = groups.get(&index) {
            for d in dependency_order(group, &declarations, &refers) {
                if let Some(moved) = declarations.get(d).and_then(|declaration| statements.get(declaration.index)) {
                    ordered.push(moved);
                }
            }
        }
        let moved = declaration_at
            .get(&index)
            .and_then(|&d| declarations.get(d))
            .is_some_and(|declaration| declaration.target != index);
        if !moved {
            ordered.push(statement);
        }
    }
    ordered
}

/// Order the declarations hoisted to one position so that one referring to
/// another comes after it; ties and cycles keep source order. `group` is in
/// source order.
fn dependency_order(group: &[usize], declarations: &[Declaration<'_>], refers: &[Vec<usize>]) -> Vec<usize> {
    let mut remaining = group.to_vec();
    let mut ordered = Vec::with_capacity(group.len());
    while !remaining.is_empty() {
        // The first (in source order) declaration none of the remaining ones
        // must precede; with a cycle, simply the first.
        let ready = remaining
            .iter()
            .position(|&d| {
                let needs = declarations
                    .get(d)
                    .and_then(|declaration| refers.get(declaration.index))
                    .map_or(&[][..], Vec::as_slice);
                !remaining.iter().any(|&other| other != d && needs.binary_search(&other).is_ok())
            })
            .unwrap_or(0);
        ordered.push(remaining.remove(ready));
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

    #[test]
    fn a_hoisted_declaration_brings_the_later_declarations_it_calls() {
        assert_eq!(
            order("const r = f(2); function f(x: number) { return g(x) } function g(x: number) { return x } return r"),
            vec![
                "function g(x: number) { return x }",
                "function f(x: number) { return g(x) }",
                "const r = f(2);",
                "return r"
            ]
        );
    }

    #[test]
    fn a_parameter_named_like_a_later_declaration_does_not_hoist_it() {
        assert_eq!(
            order("const apply = (g: number) => g * 2; const scale = 3; function g() { return scale } return apply(1) + g()"),
            vec![
                "const apply = (g: number) => g * 2;",
                "const scale = 3;",
                "function g() { return scale }",
                "return apply(1) + g()"
            ]
        );
    }
}
