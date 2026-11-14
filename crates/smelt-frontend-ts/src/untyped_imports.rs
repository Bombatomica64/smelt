//! Where a module reaches values of an untyped import.
//!
//! A package the transpiler resolved to nothing (a dev dependency that is not
//! installed, such as `fast-check` in a checkout without `node_modules`) is an
//! untyped module: lowering reads every value of it as a dynamic
//! `SmeltUnknown`, and the type checker is given the same view (an ambient
//! `declare module "fast-check";`). A callback passed to a call rooted in such
//! an import (`fc.property(gen, (array, size) => ..)`) then has parameters no
//! type reaches: lowering binds them as dynamic values, and the checker
//! reports them as implicitly `any` (TS7006/TS7031). Those diagnostics say the
//! dependency's types are missing, not that the program is ill-typed, so the
//! transpiler does not count them; this module finds exactly those
//! parameters, by position.

use std::collections::HashSet;

use oxc::allocator::Allocator;
use oxc::ast::ast::{Argument, Expression, ImportDeclarationSpecifier, Statement};
use oxc::ast_visit::{Visit, walk};
use oxc::parser::{ParseOptions, Parser};
use oxc::span::SourceType;

/// The parameter-list ranges of the callbacks untyped calls receive.
///
/// Each `(start, end)` byte range is the parameter list of a callback
/// `source` passes, directly, as an argument of a call whose callee is
/// rooted in a binding imported from one of `untyped_modules`
/// (`fc.assert(fc.property(fc.array(fc.integer()), (array) => ..))`).
///
/// A source that does not parse has none.
#[must_use]
pub fn untyped_callback_parameter_spans(source: &str, untyped_modules: &[&str]) -> Vec<(u32, u32)> {
    if untyped_modules.is_empty() {
        return Vec::new();
    }
    let allocator = Allocator::default();
    let parsed = Parser::new(
        &allocator,
        source,
        SourceType::default().with_typescript(true),
    )
    .with_options(ParseOptions::default())
    .parse();
    if !parsed.diagnostics.is_empty() {
        return Vec::new();
    }
    let mut bindings = HashSet::new();
    for statement in &parsed.program.body {
        let Statement::ImportDeclaration(import) = statement else {
            continue;
        };
        // A specifier with a lone surrogate names no installable package.
        if !import
            .source
            .value
            .as_str()
            .is_some_and(|specifier| untyped_modules.contains(&specifier))
        {
            continue;
        }
        for specifier in import.specifiers.iter().flatten() {
            let local = match specifier {
                ImportDeclarationSpecifier::ImportSpecifier(named) => &named.local,
                ImportDeclarationSpecifier::ImportDefaultSpecifier(default) => &default.local,
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(namespace) => &namespace.local,
            };
            bindings.insert(local.name.as_str().to_owned());
        }
    }
    let mut collector = CallbackCollector {
        bindings,
        spans: Vec::new(),
    };
    collector.visit_program(&parsed.program);
    collector.spans
}

/// Collects the parameter ranges of callbacks passed to untyped calls.
struct CallbackCollector {
    /// Local names bound by imports from untyped modules.
    bindings: HashSet<String>,
    /// The parameter ranges found so far.
    spans: Vec<(u32, u32)>,
}

impl CallbackCollector {
    /// Whether `callee` is rooted in an untyped binding: the binding itself,
    /// a member of it, or a member of a call of it (`fc`, `fc.property`,
    /// `fc.integer().map`).
    fn rooted_in_untyped(&self, callee: &Expression<'_>) -> bool {
        let mut current = callee.without_parentheses();
        loop {
            match current {
                Expression::Identifier(identifier) => {
                    return self.bindings.contains(identifier.name.as_str());
                }
                Expression::StaticMemberExpression(member) => {
                    current = member.object.without_parentheses();
                }
                Expression::ComputedMemberExpression(member) => {
                    current = member.object.without_parentheses();
                }
                Expression::CallExpression(call) => current = call.callee.without_parentheses(),
                _ => return false,
            }
        }
    }
}

impl<'a> Visit<'a> for CallbackCollector {
    fn visit_call_expression(&mut self, call: &oxc::ast::ast::CallExpression<'a>) {
        if self.rooted_in_untyped(&call.callee) {
            for argument in &call.arguments {
                let params = match argument {
                    Argument::ArrowFunctionExpression(arrow) => &arrow.params,
                    Argument::FunctionExpression(function) => &function.params,
                    _ => continue,
                };
                self.spans.push((params.span.start, params.span.end));
            }
        }
        walk::walk_call_expression(self, call);
    }
}

#[cfg(test)]
mod tests {
    use super::untyped_callback_parameter_spans;

    /// Only the callbacks of calls rooted in an untyped import are found.
    #[test]
    fn finds_callbacks_of_untyped_calls_only() {
        let source = "import fc from 'fast-check';\nimport { map } from './typed';\nfc.assert(fc.property(fc.array(fc.integer()), (array, size) => array));\nmap([1], (value) => value);\n";
        let spans = untyped_callback_parameter_spans(source, &["fast-check"]);
        let start = source
            .find("(array, size)")
            .and_then(|at| u32::try_from(at).ok());
        let expected = start.map(|at| vec![(at, at + 13)]);
        assert_eq!(Some(spans), expected);
    }
}
