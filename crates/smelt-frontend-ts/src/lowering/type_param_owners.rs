//! Which declaring ITEM owns each type parameter of a module.
//!
//! `Type::TypeParam` is identified by its symbol alone, and every substitution
//! map in the pipeline is keyed by it, so the symbol has to say WHICH
//! parameter it is, not only how it is spelled. Two unrelated parameters that
//! share a spelling (`class Holder<Value>` and `class Parser<Value>`) must be
//! two types, or a substitution meant for one silently rewrites the other.
//!
//! The identity is the declaring item: the function, class, interface, type
//! alias or method that declares the parameter, plus the parameter's name. It
//! is deliberately NOT the declaration site: the overload signatures of one
//! function (`function pick<T, K>(..): ..; function pick<T, K>(..) {..}`) each
//! spell their own `<T, K>` list, but TypeScript treats them as one item, and a
//! body that reads `T` must see the same `T` its overloads describe. Likewise a
//! method's overloads and a merged `interface` share their parameters.
//!
//! The owner key is a path of item names from the module root (`Parser.set`,
//! `static Box.of`), qualified by the file. A declaration that is not a named
//! item (an arrow, a function or class expression, a function type, a call
//! signature in a type literal, a namespace block) has no overload set and is
//! keyed by its own source offset instead, which also keeps it distinct from a
//! same-named parameter of the item around it (`function f<T>() { const g =
//! <T>(x: T) => x }`).
//!
//! The scan is purely syntactic and runs once per parse, before any type
//! parameter is interned; both the predeclaration pass and the body pass key
//! the same declaration to the same owner.

use std::collections::HashMap;

use oxc::ast::ast::{
    ArrowFunctionExpression, Class, Function, FunctionType, MethodDefinition, MethodDefinitionKind,
    Program, TSCallSignatureDeclaration, TSConstructSignatureDeclaration, TSConstructorType,
    TSFunctionType, TSInterfaceBody, TSInterfaceDeclaration, TSMethodSignature, TSModuleBlock,
    TSSignature, TSTypeAliasDeclaration, TSTypeParameterDeclaration,
};
use oxc::ast_visit::{Visit, walk};
use oxc::semantic::ScopeFlags;

/// Map every type parameter's span start to the owner path of the item that
/// declares it (see the module docs).
pub(super) fn type_param_owners(program: &Program<'_>) -> HashMap<u32, String> {
    let mut collector = OwnerCollector::default();
    collector.visit_program(program);
    collector.owners
}

/// Walks a module and records the owner path of each type-parameter list.
#[derive(Default)]
struct OwnerCollector {
    /// Owner path segments from the module root down to the current item.
    path: Vec<String>,
    /// A segment chosen by the parent for the next owner node it visits
    /// (a class method's or interface member's name), consumed by that node.
    named_next: Option<String>,
    /// Type-parameter span start -> owner path.
    owners: HashMap<u32, String>,
}

impl OwnerCollector {
    /// Run `walk` with `segment` pushed as the innermost owner.
    fn within(&mut self, segment: String, walk: impl FnOnce(&mut Self)) {
        self.path.push(segment);
        walk(self);
        self.path.pop();
    }

    /// The segment of an owner node: the name its parent chose, else `@offset`.
    fn segment_or_offset(&mut self, start: u32) -> String {
        self.named_next
            .take()
            .unwrap_or_else(|| format!("@{start}"))
    }
}

impl<'a> Visit<'a> for OwnerCollector {
    fn visit_ts_type_parameter_declaration(&mut self, it: &TSTypeParameterDeclaration<'a>) {
        let owner = self.path.join(".");
        for param in &it.params {
            self.owners.insert(param.span.start, owner.clone());
        }
        walk::walk_ts_type_parameter_declaration(self, it);
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        // A named DECLARATION (an implementation or an overload signature) is
        // an item; an expression is not, even when it has a name.
        let declared_name = matches!(
            it.r#type,
            FunctionType::FunctionDeclaration | FunctionType::TSDeclareFunction
        )
        .then(|| it.id.as_ref().map(|id| id.name.to_string()))
        .flatten();
        let segment = self
            .named_next
            .take()
            .unwrap_or_else(|| declared_name.unwrap_or_else(|| format!("@{}", it.span.start)));
        self.within(segment, |this| walk::walk_function(this, it, flags));
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        self.named_next = None;
        let segment = format!("@{}", it.span.start);
        self.within(segment, |this| {
            walk::walk_arrow_function_expression(this, it);
        });
    }

    fn visit_class(&mut self, it: &Class<'a>) {
        self.named_next = None;
        let segment = match (&it.id, it.is_declaration()) {
            (Some(id), true) => id.name.to_string(),
            _ => format!("@{}", it.span.start),
        };
        self.within(segment, |this| walk::walk_class(this, it));
    }

    fn visit_method_definition(&mut self, it: &MethodDefinition<'a>) {
        // Decorators and the key are visited first and outside the method, so
        // nothing in them can take the method's segment.
        self.visit_decorators(&it.decorators);
        self.visit_property_key(&it.key);
        let name = if it.computed {
            None
        } else {
            it.key
                .static_name()
                .map(|name| crate::lowering::js_text::static_name_text(name).into_owned())
                .or_else(|| it.key.private_name().map(|name| format!("#{name}")))
        };
        self.named_next = Some(match name {
            Some(name) if it.r#static => format!("static {name}"),
            Some(name) => name,
            None => format!("@{}", it.span.start),
        });
        let flags = match it.kind {
            MethodDefinitionKind::Get => ScopeFlags::Function | ScopeFlags::GetAccessor,
            MethodDefinitionKind::Set => ScopeFlags::Function | ScopeFlags::SetAccessor,
            MethodDefinitionKind::Constructor => ScopeFlags::Function | ScopeFlags::Constructor,
            MethodDefinitionKind::Method => ScopeFlags::Function,
        };
        self.visit_function(&it.value, flags);
    }

    fn visit_ts_interface_declaration(&mut self, it: &TSInterfaceDeclaration<'a>) {
        self.named_next = None;
        let segment = it.id.name.to_string();
        self.within(segment, |this| {
            walk::walk_ts_interface_declaration(this, it);
        });
    }

    fn visit_ts_interface_body(&mut self, it: &TSInterfaceBody<'a>) {
        // An interface's members are items of the interface: overloads of one
        // method (or of its call signature) share their parameters. The same
        // signatures inside a type literal are anonymous.
        for signature in &it.body {
            self.named_next = match signature {
                TSSignature::TSMethodSignature(method) if !method.computed => method
                    .key
                    .static_name()
                    .map(|name| crate::lowering::js_text::static_name_text(name).into_owned()),
                TSSignature::TSCallSignatureDeclaration(_) => Some("()".to_owned()),
                TSSignature::TSConstructSignatureDeclaration(_) => Some("new()".to_owned()),
                _ => None,
            };
            self.visit_ts_signature(signature);
            self.named_next = None;
        }
    }

    fn visit_ts_method_signature(&mut self, it: &TSMethodSignature<'a>) {
        let segment = self.segment_or_offset(it.span.start);
        self.within(segment, |this| walk::walk_ts_method_signature(this, it));
    }

    fn visit_ts_call_signature_declaration(&mut self, it: &TSCallSignatureDeclaration<'a>) {
        let segment = self.segment_or_offset(it.span.start);
        self.within(segment, |this| {
            walk::walk_ts_call_signature_declaration(this, it);
        });
    }

    fn visit_ts_construct_signature_declaration(
        &mut self,
        it: &TSConstructSignatureDeclaration<'a>,
    ) {
        let segment = self.segment_or_offset(it.span.start);
        self.within(segment, |this| {
            walk::walk_ts_construct_signature_declaration(this, it);
        });
    }

    fn visit_ts_type_alias_declaration(&mut self, it: &TSTypeAliasDeclaration<'a>) {
        self.named_next = None;
        let segment = it.id.name.to_string();
        self.within(segment, |this| {
            walk::walk_ts_type_alias_declaration(this, it);
        });
    }

    fn visit_ts_function_type(&mut self, it: &TSFunctionType<'a>) {
        self.named_next = None;
        let segment = format!("@{}", it.span.start);
        self.within(segment, |this| walk::walk_ts_function_type(this, it));
    }

    fn visit_ts_constructor_type(&mut self, it: &TSConstructorType<'a>) {
        self.named_next = None;
        let segment = format!("@{}", it.span.start);
        self.within(segment, |this| walk::walk_ts_constructor_type(this, it));
    }

    fn visit_ts_module_block(&mut self, it: &TSModuleBlock<'a>) {
        // Namespace members are items of THEIR namespace: `A.f` and `B.f`
        // are different functions.
        self.named_next = None;
        let segment = format!("@{}", it.span.start);
        self.within(segment, |this| walk::walk_ts_module_block(this, it));
    }
}

#[cfg(test)]
mod tests {
    use super::type_param_owners;
    use oxc::allocator::Allocator;
    use oxc::parser::Parser;
    use oxc::span::SourceType;

    /// Every type parameter of `source` in source order, as (spelling, owner).
    fn owners_of(source: &str) -> Vec<(String, String)> {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let mut found = type_param_owners(&parsed.program)
            .into_iter()
            .collect::<Vec<_>>();
        found.sort();
        found
            .into_iter()
            .map(|(start, owner)| {
                let name = usize::try_from(start)
                    .ok()
                    .and_then(|start| source.get(start..))
                    .unwrap_or_default()
                    .chars()
                    .take_while(|c| c.is_alphanumeric())
                    .collect();
                (name, owner)
            })
            .collect()
    }

    /// Overload signatures of one function share the function as owner, while
    /// same-named parameters of different items have different owners.
    #[test]
    fn overloads_share_owner_and_items_do_not() {
        let owners = owners_of(
            "function pick<T>(a: T): T;\nfunction pick<T>(a: T): T { return a }\n\
             class Holder<Value> {}\nabstract class Parser<Value> { abstract parse<D>(d: D): Value; parse2<D>(d: D) {} }\n\
             interface I<T> { m<U>(u: U): U; m<U>(u: U, x: number): U; <C>(c: C): C }\n\
             type Alias<T> = { m<U>(u: U): U };\n\
             function outer<T>() { const g = <T>(x: T) => x; }",
        );
        let owner_list = owners
            .iter()
            .map(|(_, owner)| owner.as_str())
            .collect::<Vec<_>>();
        let named = [
            "pick",
            "pick",
            "Holder",
            "Parser",
            "Parser.parse",
            "Parser.parse2",
            "I",
            "I.m",
            "I.m",
            "I.()",
            "Alias",
        ];
        assert_eq!(
            owner_list.get(..named.len()),
            Some(named.as_slice()),
            "{owner_list:?}"
        );
        let tail = owner_list.get(named.len()..).unwrap_or_default();
        assert_eq!(tail.len(), 3, "{owner_list:?}");
        assert!(
            tail.first()
                .is_some_and(|owner| owner.starts_with("Alias.@")),
            "{owner_list:?}"
        );
        assert_eq!(tail.get(1), Some(&"outer"), "{owner_list:?}");
        assert!(
            tail.get(2)
                .is_some_and(|owner| owner.starts_with("outer.@")),
            "{owner_list:?}"
        );
    }
}
