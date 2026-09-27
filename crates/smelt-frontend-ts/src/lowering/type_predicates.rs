//! User-defined type predicates: `value is T` guards and `asserts value is T`.
//!
//! A TypeScript type predicate is a *signature* fact: whichever callable carries
//! it — a function declaration, an arrow or function expression bound to a
//! variable, a variable annotated with a predicate function type, or a class
//! method — a call to it proves `T` for the named argument when it answers
//! `true` (a guard) or when it returns at all (an assertion). This module owns
//! both halves of that rule:
//!
//! * **Recording.** [`ModuleBuilder::collect_type_predicate_signatures`] runs as
//!   a prepass before any body is lowered, so a hoisted function body that calls
//!   a predicate declared further down still narrows through it.
//!   [`ModuleBuilder::register_variable_type_predicates`] is also called for
//!   block-local variable declarations as they are lowered.
//! * **Applying.** [`ModuleBuilder::call_type_predicate`] resolves a call's
//!   callee — a plain name, or a member read on a class-typed receiver /
//!   class name — to the recorded predicate and names the local it narrows.
//!   The positive fact feeds `guard_narrowing`; the NEGATIVE fact
//!   ([`ModuleBuilder::predicate_call_inverse_guard`]) removes the proven
//!   members from the local's union, exactly as `tsc` narrows the false branch,
//!   and feeds `inverse_guard_narrowing` (else branches and early exits).
//!
//! The representation follows the type: a narrowed local whose declared type is
//! a generated union is read through the existing narrowing cast, which the
//! emitter lowers to a `match` that unwraps the member — never an erasure.

use oxc::ast::ast::{
    Argument, BindingPattern, CallExpression, ClassElement, Expression, FormalParameters,
    MethodDefinitionKind, PropertyKey, Statement, TSType, TSTypeAnnotation,
    TSTypeParameterDeclaration,
};
use smelt_hir::{Body, Type, TypeId};

use crate::lowering::{AssertionNarrowing, ModuleBuilder};

/// Which runtime effect a type-predicate signature proves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::lowering) enum PredicateKind {
    /// `value is T`: the fact holds where the call answered `true`.
    Guard,
    /// `asserts value is T`: the fact holds after the call returns.
    Assertion,
}

/// Return the plain (non-computed, non-private) name of a class member key.
fn member_key_name<'a>(key: &'a PropertyKey<'a>) -> Option<&'a str> {
    match key {
        PropertyKey::StaticIdentifier(ident) => Some(ident.name.as_str()),
        PropertyKey::StringLiteral(literal) => Some(literal.value.as_str()),
        _ => None,
    }
}

impl ModuleBuilder<'_> {
    /// Resolve the narrowing a callable signature's return annotation declares.
    ///
    /// Returns `None` unless the annotation is `param is T` or
    /// `asserts param is T` naming one of `params` by a plain identifier. The
    /// target is lowered inside the signature's own type-parameter scope so a
    /// generic predicate's `T` resolves the way its body sees it. A bare
    /// `asserts condition` has no target type and proves nothing structural.
    pub(in crate::lowering) fn signature_type_predicate(
        &mut self,
        type_params: Option<&TSTypeParameterDeclaration<'_>>,
        params: &FormalParameters<'_>,
        return_type: Option<&TSTypeAnnotation<'_>>,
    ) -> Option<(PredicateKind, AssertionNarrowing)> {
        let annotation = return_type?;
        let TSType::TSTypePredicate(predicate) = &annotation.type_annotation else {
            return None;
        };
        let kind = if predicate.asserts {
            PredicateKind::Assertion
        } else {
            PredicateKind::Guard
        };
        self.push_type_parameter_scope(type_params).ok()?;
        let resolved = match kind {
            PredicateKind::Guard => self
                .predicate_return_type(&annotation.type_annotation)
                .and_then(Result::ok),
            PredicateKind::Assertion => self
                .assertion_return_type(&annotation.type_annotation)
                .and_then(Result::ok)
                .and_then(|(name, target)| target.map(|target| (name, target))),
        };
        self.pop_type_parameter_scope();
        let (parameter_name, target) = resolved?;
        let param_index = params.items.iter().position(|param| {
            matches!(
                &param.pattern,
                BindingPattern::BindingIdentifier(binding)
                    if binding.name.as_str() == parameter_name
            )
        })?;
        Some((
            kind,
            AssertionNarrowing {
                param_index,
                target,
            },
        ))
    }

    /// Record the predicate signatures of a module's top-level declarations.
    ///
    /// Runs before any body is lowered (function declarations register theirs
    /// in `predeclare_function_item`), covering the callables that are not
    /// function declarations: variables bound to arrows / function expressions
    /// or annotated with a predicate function type, and class methods (and
    /// arrow-valued class properties).
    pub(in crate::lowering) fn collect_type_predicate_signatures(
        &mut self,
        program: &oxc::ast::ast::Program<'_>,
    ) {
        use oxc::ast::ast::Declaration;
        for statement in &program.body {
            match statement {
                Statement::VariableDeclaration(variable) => {
                    self.register_variable_type_predicates(variable);
                }
                Statement::ClassDeclaration(class) => self.register_class_type_predicates(class),
                Statement::ExportDeclaration(export) => match &export.declaration {
                    Declaration::VariableDeclaration(variable) => {
                        self.register_variable_type_predicates(variable);
                    }
                    Declaration::ClassDeclaration(class) => {
                        self.register_class_type_predicates(class);
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    /// Record the predicate signatures of the callables a declaration binds.
    ///
    /// A binding is a predicate when its initializer is an arrow or function
    /// expression whose return annotation is a predicate, or when the binding
    /// itself is annotated with a function type whose return is one (the only
    /// spelling TypeScript accepts for an arrow ASSERTION function).
    pub(in crate::lowering) fn register_variable_type_predicates(
        &mut self,
        decl: &oxc::ast::ast::VariableDeclaration<'_>,
    ) {
        for declarator in &decl.declarations {
            let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                continue;
            };
            let from_initializer = match &declarator.init {
                Some(Expression::ArrowFunctionExpression(arrow)) => self.signature_type_predicate(
                    arrow.type_parameters.as_deref(),
                    &arrow.params,
                    arrow.return_type.as_deref(),
                ),
                Some(Expression::FunctionExpression(function)) => self
                    .signature_type_predicate(
                        function.type_parameters.as_deref(),
                        &function.params,
                        function.return_type.as_deref(),
                    ),
                _ => None,
            };
            let found = from_initializer.or_else(|| {
                let annotation = declarator.type_annotation.as_ref()?;
                let TSType::TSFunctionType(function_ty) = &annotation.type_annotation else {
                    return None;
                };
                self.signature_type_predicate(
                    function_ty.type_parameters.as_deref(),
                    &function_ty.params,
                    Some(&function_ty.return_type),
                )
            });
            let Some((kind, narrowing)) = found else {
                continue;
            };
            let name = binding.name.to_string();
            match kind {
                PredicateKind::Guard => self.functions.set_predicate(name, narrowing),
                PredicateKind::Assertion => self.functions.set_assertion(name, narrowing),
            }
        }
    }

    /// Record the predicate signatures of a class's methods.
    ///
    /// Methods are keyed by the class's SOURCE name, whether they are static,
    /// and the method name; [`Self::call_type_predicate`] rebuilds the same key
    /// from a receiver's class type (or a class-name receiver for statics). An
    /// arrow-valued property (`isRaw = (r): r is Raw => ...`) is called exactly
    /// like a method, so it registers the same way.
    fn register_class_type_predicates(&mut self, class: &oxc::ast::ast::Class<'_>) {
        let Some(id) = &class.id else {
            return;
        };
        let class_name = id.name.to_string();
        if self
            .push_type_parameter_scope(class.type_parameters.as_deref())
            .is_err()
        {
            return;
        }
        for element in &class.body.body {
            let (key, is_static, found) = match element {
                ClassElement::MethodDefinition(method)
                    if method.kind == MethodDefinitionKind::Method && !method.computed =>
                {
                    let found = self.signature_type_predicate(
                        method.value.type_parameters.as_deref(),
                        &method.value.params,
                        method.value.return_type.as_deref(),
                    );
                    (&method.key, method.r#static, found)
                }
                ClassElement::PropertyDefinition(property) if !property.computed => {
                    let found = match &property.value {
                        Some(Expression::ArrowFunctionExpression(arrow)) => self
                            .signature_type_predicate(
                                arrow.type_parameters.as_deref(),
                                &arrow.params,
                                arrow.return_type.as_deref(),
                            ),
                        _ => None,
                    };
                    (&property.key, property.r#static, found)
                }
                _ => continue,
            };
            let (Some(method_name), Some((kind, narrowing))) = (member_key_name(key), found)
            else {
                continue;
            };
            self.functions.set_method_type_predicate(
                (class_name.clone(), is_static, method_name.to_owned()),
                kind,
                narrowing,
            );
        }
        self.pop_type_parameter_scope();
    }

    /// Resolve which class a member-call receiver dispatches to.
    ///
    /// A receiver that is a local (including `this`) answers its class type's
    /// source name as an INSTANCE receiver; a bare identifier that is not a
    /// local is taken as a class name for a STATIC call. Anything else — or a
    /// local whose type is not a class — has no statically known method table.
    fn predicate_receiver_class(
        &self,
        object: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, bool)> {
        let name = match object {
            Expression::Identifier(identifier) => identifier.name.as_str(),
            Expression::ThisExpression(_) => "this",
            _ => return None,
        };
        let Some(local) = self.scope.lookup(name) else {
            return matches!(object, Expression::Identifier(_)).then(|| (name.to_owned(), true));
        };
        let ty = self
            .narrowed_type(name)
            .or_else(|| Self::local_ty_checked(body, local))?;
        let Some(Type::Class { name: symbol, .. }) = self.ctx.krate.types.get(ty) else {
            return None;
        };
        let class_name = self
            .ctx
            .krate
            .names
            .get(*symbol)
            .or_else(|| self.ctx.krate.symbols.get(*symbol))?;
        Some((class_name.to_owned(), false))
    }

    /// Resolve a call to a recorded type predicate of `kind`.
    ///
    /// Returns the source name of the local the call narrows and the proven
    /// type. Only a plain-identifier argument names a local that flow facts can
    /// attach to.
    pub(in crate::lowering) fn call_type_predicate(
        &self,
        call: &CallExpression<'_>,
        body: &Body,
        kind: PredicateKind,
    ) -> Option<(String, TypeId)> {
        let narrowing = match &call.callee {
            Expression::Identifier(callee) => match kind {
                PredicateKind::Guard => self.functions.predicate(callee.name.as_str()),
                PredicateKind::Assertion => self.functions.assertion(callee.name.as_str()),
            },
            Expression::StaticMemberExpression(member) => {
                let (class_name, is_static) = self.predicate_receiver_class(&member.object, body)?;
                self.functions.method_type_predicate(
                    &(class_name, is_static, member.property.name.to_string()),
                    kind,
                )
            }
            _ => None,
        }?;
        let Argument::Identifier(identifier) = call.arguments.get(narrowing.param_index)? else {
            return None;
        };
        Some((identifier.name.to_string(), narrowing.target))
    }

    /// Recognize a call to a user-defined `value is T` predicate.
    pub(in crate::lowering) fn predicate_call_guard(
        &self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, TypeId)> {
        let Expression::CallExpression(call) = expression else {
            return None;
        };
        self.call_type_predicate(call, body, PredicateKind::Guard)
    }

    /// The type a local keeps where a `value is T` predicate answered `false`.
    ///
    /// `tsc` narrows the false branch by removing the proven members from the
    /// local's union; this does the same over interned member identity. A local
    /// that is not a union (or keeps every member, or would keep none) gains no
    /// fact, so the false branch reads it at its declared type as before.
    pub(in crate::lowering) fn predicate_call_inverse_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, TypeId)> {
        let Expression::CallExpression(call) = expression else {
            return None;
        };
        let (name, target) = self.call_type_predicate(call, body, PredicateKind::Guard)?;
        let local = self.scope.lookup(&name)?;
        let ty = self
            .narrowed_type(&name)
            .or_else(|| Self::local_ty_checked(body, local))?;
        let remaining = self.predicate_false_branch_type(ty, target)?;
        Some((name, remaining))
    }

    /// Discover the narrowing applied by a successful assertion call statement.
    pub(in crate::lowering) fn assertion_call_narrowing(
        &self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, TypeId)> {
        let Expression::CallExpression(call) = expression else {
            return None;
        };
        self.call_type_predicate(call, body, PredicateKind::Assertion)
    }

    /// The type a local of type `ty` keeps where a predicate proving
    /// `proven` answered `false`: `ty` without `proven`'s members.
    ///
    /// An optional union keeps its optionality around the remaining members.
    /// Returns `None` when `ty` is not a union, when nothing was removed, or
    /// when nothing would remain.
    fn predicate_false_branch_type(&mut self, ty: TypeId, proven: TypeId) -> Option<TypeId> {
        let proven = self.flatten_union_member_types(proven);
        self.remove_proven_members(ty, &proven)
    }

    /// [`Self::predicate_false_branch_type`] over a flattened member list.
    fn remove_proven_members(&mut self, ty: TypeId, proven: &[TypeId]) -> Option<TypeId> {
        match self.ctx.krate.types.get(ty).cloned()? {
            Type::Union(items) => {
                let remaining = items
                    .iter()
                    .copied()
                    .filter(|item| !proven.contains(item))
                    .collect::<Vec<_>>();
                if remaining.is_empty() || remaining.len() == items.len() {
                    return None;
                }
                Some(match remaining.as_slice() {
                    [single] => *single,
                    _ => self.ctx.krate.types.intern(Type::Union(remaining)),
                })
            }
            Type::Optional(inner) => {
                let remaining = self.remove_proven_members(inner, proven)?;
                Some(self.ctx.krate.types.intern(Type::Optional(remaining)))
            }
            _ => None,
        }
    }
}
