p = '/home/user/smelt/.claude/worktrees/agent-aa8e2a786664e0a39/crates/smelt-frontend-ts/src/lowering/module_init.rs'
s = open(p).read()

def rep(old, new, count=1):
    global s
    n = s.count(old)
    assert n == count, (old, n)
    s = s.replace(old, new)

# forward_arrow_const_names (2 sites)
rep("""                        if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                            && matches!(
                                declarator.init,
                                Some(Expression::ArrowFunctionExpression(_))
                            )
                        {""", """                        if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                            && declarator
                                .init
                                .as_ref()
                                .is_some_and(|init| Self::liftable_function_initializer_span(init).is_some())
                        {""")
rep("""                            if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                                && matches!(
                                    declarator.init,
                                    Some(Expression::ArrowFunctionExpression(_))
                                )
                            {""", """                            if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                                && declarator.init.as_ref().is_some_and(|init| {
                                    Self::liftable_function_initializer_span(init).is_some()
                                })
                            {""")
# is_liftable_exported_arrow_const
rep("""            && decl.declarations.iter().all(|declarator| {
                matches!(
                    (&declarator.id, &declarator.init),
                    (
                        BindingPattern::BindingIdentifier(binding),
                        Some(Expression::ArrowFunctionExpression(_)),
                    ) if forward_arrow_consts.contains(binding.name.as_str())
                )
            })""", """            && decl.declarations.iter().all(|declarator| {
                matches!(
                    (&declarator.id, &declarator.init),
                    (BindingPattern::BindingIdentifier(binding), Some(init))
                        if Self::liftable_function_initializer_span(init).is_some()
                            && forward_arrow_consts.contains(binding.name.as_str())
                )
            })""")
# arrow_const_declaration_names
rep("""                matches!(
                    declarator.init,
                    Some(Expression::ArrowFunctionExpression(_))
                )
                .then(|| binding.name.to_string())""", """                declarator
                    .init
                    .as_ref()
                    .and_then(Self::liftable_function_initializer_span)
                    .map(|_| binding.name.to_string())""")
# arrow_const_dependencies_are_lowered
rep("""            let Some(Expression::ArrowFunctionExpression(arrow)) = &declarator.init else {
                return true;
            };
            let text = self
                .source
                .get(
                    usize::try_from(arrow.span.start).unwrap_or(usize::MAX)
                        ..usize::try_from(arrow.span.end).unwrap_or(usize::MAX),
                )""", """            let Some(span) = declarator
                .init
                .as_ref()
                .and_then(Self::liftable_function_initializer_span)
            else {
                return true;
            };
            let text = self
                .source
                .get(
                    usize::try_from(span.start).unwrap_or(usize::MAX)
                        ..usize::try_from(span.end).unwrap_or(usize::MAX),
                )""")
# retain_capturable_arrow_consts
rep("""                let Some(Expression::ArrowFunctionExpression(arrow)) = &declarator.init else {
                    continue;
                };
                if !candidates.contains(binding.name.as_str()) {
                    continue;
                }
                let text = self
                    .source
                    .get(
                        usize::try_from(arrow.span.start).unwrap_or(usize::MAX)
                            ..usize::try_from(arrow.span.end).unwrap_or(usize::MAX),
                    )""", """                let Some(span) = declarator
                    .init
                    .as_ref()
                    .and_then(Self::liftable_function_initializer_span)
                else {
                    continue;
                };
                if !candidates.contains(binding.name.as_str()) {
                    continue;
                }
                let text = self
                    .source
                    .get(
                        usize::try_from(span.start).unwrap_or(usize::MAX)
                            ..usize::try_from(span.end).unwrap_or(usize::MAX),
                    )""")
# arrow_function_const_item_declarations
rep("""            let Some(Expression::ArrowFunctionExpression(arrow)) = &declarator.init else {
                continue;
            };
            if !forward_arrow_consts.contains(binding.name.as_str()) {
                continue;
            }
            let type_hint = declarator""", """            if !forward_arrow_consts.contains(binding.name.as_str()) {
                continue;
            }
            // `const name = function inner() { .. }` is the same lexical
            // binding as an arrow const: lift it through the named-function
            // path the exported form already uses, so a function body that
            // calls `name` reaches the item instead of a placeholder. The
            // inner name `inner` binds only inside its own body.
            if let Some(Expression::FunctionExpression(function)) = &declarator.init
                && function.body.is_some()
            {
                items.push(self.function_declaration_named(function, binding.name.as_str())?);
                continue;
            }
            let Some(Expression::ArrowFunctionExpression(arrow)) = &declarator.init else {
                continue;
            };
            let type_hint = declarator""")
# helper before arrow_const_declaration_names doc
rep("""    /// Return top-level arrow binding names declared by one variable statement.""", """    /// The source span of a `const` initializer the forward-arrow queue may lift
    /// into a module-level callable item, or `None` for any other initializer.
    ///
    /// Both an arrow and a function expression with a body qualify: `const f =
    /// function g() { .. }` is the same lexical binding as `const f = () =>
    /// ..`, and the name `g` is scoped to the function's own body. A generator
    /// function expression is not a plain callable item, so it stays a value.
    pub(super) fn liftable_function_initializer_span(init: &Expression<'_>) -> Option<oxc::span::Span> {
        match init {
            Expression::ArrowFunctionExpression(arrow) => Some(arrow.span),
            Expression::FunctionExpression(function)
                if function.body.is_some() && !function.generator =>
            {
                Some(function.span)
            }
            _ => None,
        }
    }

    /// Return top-level arrow binding names declared by one variable statement.""")
open(p, 'w').write(s)
