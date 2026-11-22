//! The JavaScript `Function.prototype.name` of every function written in a module.
//!
//! A function's name is fixed when the function value is created. A function
//! declaration or named function expression takes its own identifier; an
//! anonymous one (arrow, `function () {}`) takes the name the spec's
//! `NamedEvaluation` hands it from its syntactic position
//! (<https://tc39.es/ecma262/#sec-isanonymousfunctiondefinition>):
//!
//! * a binding initializer: `const f = () => 1`, a default
//!   (`{ f = () => 1 }`, `(f = () => 1) => ..`) — the bound identifier;
//! * an assignment to a plain identifier (`f = ..`, `f ??= ..`, and the
//!   destructuring-assignment defaults `[f = ..] = xs`, `({ f = .. } = o)`);
//! * an object literal property or method, or a class field — the property
//!   key (`get k` / `set k` for accessors, `#k` for a private field);
//! * `export default () => 1` — `"default"`.
//!
//! TypeScript wrappers (`as`, `satisfies`, `!`, `<T>`) vanish from the emitted
//! JavaScript, leaving at most parentheses, so a wrapped function is still
//! anonymous and still named by its position.
//!
//! The scan is syntactic and keyed by the function node's span start, so every
//! lowering path that builds the closure for that node reads the same name.

use std::collections::HashMap;

use oxc::ast::ast::{
    AssignmentExpression, AssignmentOperator, AssignmentPattern, AssignmentTarget,
    AssignmentTargetPropertyIdentifier, AssignmentTargetWithDefault, BindingPattern,
    ExportDefaultDeclaration, ExportDefaultDeclarationKind, Expression, FormalParameter, Function,
    ObjectProperty, Program, PropertyDefinition, PropertyKey, PropertyKind, VariableDeclarator,
};
use oxc::ast_visit::{Visit, walk};
use oxc::syntax::scope::ScopeFlags;

impl crate::lowering::ModuleBuilder<'_> {
    /// The `Function.prototype.name` symbol of the function node starting at
    /// `span_start`, or `None` for an anonymous function.
    pub(in crate::lowering) fn closure_name_at(
        &mut self,
        span_start: u32,
    ) -> Option<smelt_hir::Symbol> {
        let name = self.function_names.get(&span_start)?.clone();
        Some(self.intern_source_name(&name))
    }
}

/// Map each function node's span start to the name the function reports.
pub(in crate::lowering) fn function_names(program: &Program<'_>) -> HashMap<u32, String> {
    let mut collector = FunctionNameCollector::default();
    collector.visit_program(program);
    collector.names
}

/// The [`Visit`] pass behind [`function_names`].
#[derive(Default)]
struct FunctionNameCollector {
    /// Function span start to `Function.prototype.name`.
    names: HashMap<u32, String>,
}

impl FunctionNameCollector {
    /// Name `value` `name` when it is an anonymous function definition.
    fn name_anonymous(&mut self, value: &Expression<'_>, name: impl Into<String>) {
        let start = match value.get_inner_expression() {
            Expression::ArrowFunctionExpression(arrow) => arrow.span.start,
            Expression::FunctionExpression(function) if function.id.is_none() => {
                function.span.start
            }
            _ => return,
        };
        self.names.entry(start).or_insert_with(|| name.into());
    }

    /// Name the default of a binding pattern `name = value` after its identifier.
    fn name_binding_default(&mut self, left: &BindingPattern<'_>, value: &Expression<'_>) {
        if let BindingPattern::BindingIdentifier(binding) = left {
            self.name_anonymous(value, binding.name.as_str());
        }
    }
}

/// The name a property key gives a function, when the key is static.
fn property_key_name(key: &PropertyKey<'_>) -> Option<String> {
    if let PropertyKey::PrivateIdentifier(private) = key {
        return Some(format!("#{}", private.name.as_str()));
    }
    key.static_name()?
        .into_cow_str()
        .map(std::borrow::Cow::into_owned)
}

impl<'a> Visit<'a> for FunctionNameCollector {
    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        if let Some(id) = &function.id {
            self.names
                .insert(function.span.start, id.name.as_str().to_owned());
        }
        walk::walk_function(self, function, flags);
    }

    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
        if let Some(init) = &declarator.init {
            self.name_binding_default(&declarator.id, init);
        }
        walk::walk_variable_declarator(self, declarator);
    }

    fn visit_assignment_pattern(&mut self, pattern: &AssignmentPattern<'a>) {
        self.name_binding_default(&pattern.left, &pattern.right);
        walk::walk_assignment_pattern(self, pattern);
    }

    fn visit_formal_parameter(&mut self, param: &FormalParameter<'a>) {
        if let Some(initializer) = &param.initializer {
            self.name_binding_default(&param.pattern, initializer);
        }
        walk::walk_formal_parameter(self, param);
    }

    fn visit_assignment_expression(&mut self, assignment: &AssignmentExpression<'a>) {
        if matches!(
            assignment.operator,
            AssignmentOperator::Assign
                | AssignmentOperator::LogicalAnd
                | AssignmentOperator::LogicalOr
                | AssignmentOperator::LogicalNullish
        ) && let AssignmentTarget::AssignmentTargetIdentifier(target) = &assignment.left
        {
            self.name_anonymous(&assignment.right, target.name.as_str());
        }
        walk::walk_assignment_expression(self, assignment);
    }

    fn visit_assignment_target_with_default(&mut self, target: &AssignmentTargetWithDefault<'a>) {
        if let AssignmentTarget::AssignmentTargetIdentifier(binding) = &target.binding {
            self.name_anonymous(&target.init, binding.name.as_str());
        }
        walk::walk_assignment_target_with_default(self, target);
    }

    fn visit_assignment_target_property_identifier(
        &mut self,
        target: &AssignmentTargetPropertyIdentifier<'a>,
    ) {
        if let Some(init) = &target.init {
            self.name_anonymous(init, target.binding.name.as_str());
        }
        walk::walk_assignment_target_property_identifier(self, target);
    }

    fn visit_object_property(&mut self, property: &ObjectProperty<'a>) {
        // `__proto__: value` sets the prototype; it names nothing.
        let is_proto_setter = !property.computed
            && !property.shorthand
            && property.kind == PropertyKind::Init
            && property_key_name(&property.key).as_deref() == Some("__proto__");
        if !is_proto_setter && let Some(key) = property_key_name(&property.key) {
            let name = match property.kind {
                PropertyKind::Init => key,
                PropertyKind::Get => format!("get {key}"),
                PropertyKind::Set => format!("set {key}"),
            };
            self.name_anonymous(&property.value, name);
        }
        walk::walk_object_property(self, property);
    }

    fn visit_property_definition(&mut self, definition: &PropertyDefinition<'a>) {
        if let Some(value) = &definition.value
            && let Some(key) = property_key_name(&definition.key)
        {
            self.name_anonymous(value, key);
        }
        walk::walk_property_definition(self, definition);
    }

    fn visit_export_default_declaration(&mut self, declaration: &ExportDefaultDeclaration<'a>) {
        match &declaration.declaration {
            ExportDefaultDeclarationKind::FunctionDeclaration(function)
                if function.id.is_none() =>
            {
                self.names
                    .entry(function.span.start)
                    .or_insert_with(|| "default".to_owned());
            }
            kind => {
                if let Some(expression) = kind.as_expression() {
                    self.name_anonymous(expression, "default");
                }
            }
        }
        walk::walk_export_default_declaration(self, declaration);
    }
}

#[cfg(test)]
mod tests {
    use oxc::allocator::Allocator;
    use oxc::parser::Parser;
    use oxc::span::SourceType;

    use super::function_names;

    /// The names of `source`'s functions, in source order.
    fn names(source: &str) -> Vec<String> {
        let allocator = Allocator::default();
        let program = Parser::new(&allocator, source, SourceType::ts())
            .parse()
            .program;
        let mut names: Vec<_> = function_names(&program).into_iter().collect();
        names.sort();
        names.into_iter().map(|(_, name)| name).collect()
    }

    #[test]
    fn names_follow_named_evaluation() {
        assert_eq!(
            names(
                "const a = () => 1; let b = (function () {}) as F; c ??= () => 2;\n\
                 const o = { d: () => 3, get e() { return 1; }, 'f': function () {}, g() {} };\n\
                 class K { h = () => 4; #i = () => 5; }\n\
                 function j(k = () => 6) {} const { l = () => 7 } = o; [m = () => 8] = [];\n\
                 const n = function own() {}; export default () => 9;"
            ),
            [
                "a", "b", "c", "d", "get e", "f", "g", "h", "#i", "j", "k", "l", "m", "own",
                "default"
            ]
        );
    }

    #[test]
    fn positions_outside_named_evaluation_stay_anonymous() {
        assert!(
            names("x.y = () => 1; [() => 2]; f(() => 3); const { p: q } = { p: () => 4 } as any;")
                .iter()
                .all(|name| name == "p")
        );
        assert_eq!(
            names("const o = { __proto__: () => 1 }; o.k = () => 2;"),
            Vec::<String>::new()
        );
    }
}
