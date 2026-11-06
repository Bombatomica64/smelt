//! Call arguments: lowering one source argument, with or without a parameter hint.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::js_text::js_str_text;
use oxc::ast::ast::Argument;
use oxc::span::GetSpan;
use oxc::syntax::operator::BinaryOperator;
use smelt_hir::{Body, ErasureReason, Expr, ExprKind, Literal, Type};

impl ModuleBuilder<'_> {
    /// Lower a function call argument.
    pub(in crate::lowering) fn argument(
        &mut self,
        argument: &Argument<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let lowered = self.argument_shape(argument, body)?;
        // A read of a Bun `int32` slot is a `number` to every consumer.
        Ok(self.int32_read_as_number(lowered, body))
    }

    /// Lower one call argument; see [`Self::argument`].
    fn argument_shape(
        &mut self,
        argument: &Argument<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match argument {
            Argument::NumericLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::Float);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Float(lit.value)),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Argument::BigIntLiteral(lit) => {
                self.bigint_literal_expression(lit.value.as_str(), lit.span, body)
            }
            Argument::StringLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::String);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(js_str_text(lit.value).into_owned())),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Argument::BooleanLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::Bool);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Bool(lit.value)),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Argument::NullLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::None);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Argument::RegExpLiteral(literal) => {
                // A regex literal in argument position is a `RegExp` value, not
                // its source string — mirror the `Expression::RegExpLiteral`
                // lowering. (Dedicated string methods like `split`/`replace`
                // match `RegExpLiteral` in their own handlers and never reach
                // this generic argument path.) Emitting the source string here
                // erased a regex passed to a generic/closure callee — e.g.
                // `isShallowEqual(data, /a/u)` saw a string instead of a regex.
                let ty = self.regexp_type();
                let pattern = Self::regex_literal_pattern_text_without_flags(literal);
                let flags = literal.regex.flags.to_string();
                let pattern = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(pattern)),
                    ty: self.ctx.krate.types.intern(Type::String),
                    span: self.span(literal.span.start, literal.span.end),
                });
                let flags = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(flags)),
                    ty: self.ctx.krate.types.intern(Type::String),
                    span: self.span(literal.span.start, literal.span.end),
                });
                Ok(body.push_expr(Expr {
                    kind: ExprKind::New {
                        class: self.intern_type_name("RegExp"),
                        args: vec![pattern, flags],
                    },
                    ty,
                    span: self.span(literal.span.start, literal.span.end),
                }))
            }
            Argument::Identifier(ident) => self.identifier_expression(
                ident.name.as_str(),
                ident.span.start,
                ident.span.end,
                body,
            ),
            Argument::ThisExpression(this_expr) => {
                self.identifier_expression("this", this_expr.span.start, this_expr.span.end, body)
            }
            Argument::Super(super_expr) => {
                self.identifier_expression("this", super_expr.span.start, super_expr.span.end, body)
            }
            Argument::BinaryExpression(binary) => {
                if binary.operator == BinaryOperator::Instanceof {
                    return self.instanceof_expression(binary, body);
                }
                if binary.operator == BinaryOperator::In {
                    return self.in_expression(binary, body);
                }
                self.binary_expression(binary, body)
            }
            Argument::ConditionalExpression(conditional) => {
                self.conditional_expression(conditional, body, None)
            }
            Argument::LogicalExpression(logical) => self.logical_expression(logical, body),
            Argument::UnaryExpression(unary) => self.unary_expression(unary, body),
            Argument::UpdateExpression(update) => self.update_expression(update, body),
            Argument::ArrayExpression(array) => self.array_expression(array, body, None),
            Argument::ObjectExpression(object) => self.object_expression(object, body, None),
            Argument::CallExpression(call) => self.call_expression(call, body),
            Argument::ChainExpression(chain) => self.chain_expression(chain, body),
            Argument::TemplateLiteral(template) => self.template_literal_expression(template, body),
            Argument::TaggedTemplateExpression(tagged) => {
                self.tagged_template_expression(tagged, body)
            }
            Argument::TSAsExpression(as_expr) => self.type_assertion_expression(
                &as_expr.expression,
                &as_expr.type_annotation,
                as_expr.span,
                body,
            ),
            Argument::TSTypeAssertion(assertion) => self.type_assertion_expression(
                &assertion.expression,
                &assertion.type_annotation,
                assertion.span,
                body,
            ),
            Argument::TSSatisfiesExpression(satisfies) => {
                let target = self.ts_type_to_hir(&satisfies.type_annotation)?;
                self.expression_with_hint(&satisfies.expression, body, Some(target))
            }
            Argument::TSNonNullExpression(non_null) => self.non_null_assertion_expression(
                &non_null.expression,
                self.span(non_null.span.start, non_null.span.end),
                body,
            ),
            Argument::NewExpression(new_expr) => {
                self.new_expression_with_hint(new_expr, body, None)
            }
            Argument::ComputedMemberExpression(member) => self.computed_member(member, body),
            Argument::StaticMemberExpression(member) => self.static_member(member, body),
            // A private-name field read is a member read like any other; the
            // `#` sigil only puts the property in a separate namespace. It is
            // delegated to the same helper the expression position uses, so
            // `f(this.#field)` and `const x = this.#field` cannot diverge.
            Argument::PrivateFieldExpression(member) => self.private_field_member(
                &member.object,
                member.field.name.as_str(),
                member.span,
                body,
            ),
            // Delegated so an awaited call argument lowers through the same rule
            // as an awaited expression; the argument spelling used to have its
            // own copy that dropped the operand for a non-future type.
            Argument::AwaitExpression(await_expr) => {
                self.await_expression(await_expr, None, body)
            }
            Argument::ArrowFunctionExpression(arrow) => self.arrow_function_expression(arrow, body),
            Argument::FunctionExpression(function) => {
                if function.r#async {
                    let ty = self.erase_with(ErasureReason::UnmodeledType, "async function expression argument");
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::None),
                        ty,
                        span: self.span(function.span.start, function.span.end),
                    }));
                }
                self.function_expression_value(function, None, function.span, body)
            }
            Argument::TSInstantiationExpression(instantiation) => {
                self.expression(&instantiation.expression, body)
            }
            // A spread reaching here is a VARIADIC caller: a rest list, a
            // stdlib shim like `Math.max(...xs)`, a `new`, a super call. Those
            // read the operand as one value on purpose. A fixed-arity call must
            // not reach here -- `lower_call_arg` asserts that -- because
            // lowering the operand as a single argument is what silently bound a
            // whole tuple to parameter 0 and let the emitter default the rest.
            Argument::SpreadElement(spread) => self.expression(&spread.argument, body),
            // Every other argument is an expression, and means what it means anywhere.
            _ => match argument.as_expression() {
                Some(expression) => self.expression(expression, body),
                None => Err(SmeltError::unsupported(
                    self.span(argument.span().start, argument.span().end),
                    format!("call argument kind is not lowered yet: {argument:?}"),
                )),
            },
        }
    }

    /// Lower a call argument with an expected type for literals that need contextual typing.
    pub(in crate::lowering) fn argument_with_hint(
        &mut self,
        argument: &Argument<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match argument {
            Argument::ArrayExpression(array) => self.array_expression(array, body, type_hint),
            Argument::ObjectExpression(object) => self.object_expression(object, body, type_hint),
            Argument::ArrowFunctionExpression(arrow) => {
                self.arrow_function_expression_with_hint(arrow, body, type_hint)
            }
            Argument::FunctionExpression(function) => {
                self.function_expression_value(function, type_hint, function.span, body)
            }
            Argument::RegExpLiteral(literal)
                if type_hint
                    .is_some_and(|hint| self.ctx.krate.types.get(hint) == Some(&Type::String)) =>
            {
                let ty = self.ctx.krate.types.intern(Type::String);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(Self::regex_literal_pattern_text(
                        literal,
                    ))),
                    ty,
                    span: self.span(literal.span.start, literal.span.end),
                }))
            }
            Argument::RegExpLiteral(literal)
                if type_hint
                    .is_some_and(|hint| self.ctx.krate.types.get(hint) == Some(&Type::Unknown)) =>
            {
                self.regexp_literal_expression(literal, body)
            }
            Argument::RegExpLiteral(literal)
                if type_hint.is_some_and(|hint| self.type_accepts_regexp_literal(hint)) =>
            {
                self.regexp_literal_expression(literal, body)
            }
            Argument::TSAsExpression(as_expr) => self.type_assertion_expression(
                &as_expr.expression,
                &as_expr.type_annotation,
                as_expr.span,
                body,
            ),
            Argument::TSTypeAssertion(assertion) => self.type_assertion_expression(
                &assertion.expression,
                &assertion.type_annotation,
                assertion.span,
                body,
            ),
            Argument::TSSatisfiesExpression(satisfies) => {
                let target = self.ts_type_to_hir(&satisfies.type_annotation)?;
                self.expression_with_hint(&satisfies.expression, body, Some(target))
            }
            Argument::TSNonNullExpression(non_null) => {
                let value = self.expression_with_hint(&non_null.expression, body, type_hint)?;
                // A source-level `!` asserts nothing at runtime. When the sink
                // itself accepts nullish values (its type still has a nullish
                // arm to strip), the assertion has nothing to narrow and must
                // stay a no-op: narrowing here would unwrap the value only for
                // the argument coercion to re-wrap it, turning a legitimate
                // absent value into a runtime panic. JS passes the value
                // through unchanged.
                if type_hint.is_some_and(|hint| self.type_admits_nullish(hint)) {
                    return Ok(value);
                }
                Ok(self.non_null_assertion_value(
                    value,
                    self.span(non_null.span.start, non_null.span.end),
                    body,
                ))
            }
            // A class name passed where a constructor *type* is expected
            // (`makeError(TypeError, ...)`, `factory(MyClass)`). The class name
            // is a constructor value; the expected constructor type lowered to a
            // `Type::Function` (see `constructor_type_to_hir`), so adapt the
            // class into a callable closure `(args) => new Class(args)` that
            // matches that signature. Falls through to plain lowering when the
            // identifier is not a class-as-constructor for this hint.
            Argument::Identifier(identifier) => {
                if let Some(expr) = self.class_constructor_value_expression(
                    identifier.name.as_str(),
                    type_hint,
                    identifier.span,
                    body,
                )? {
                    return Ok(expr);
                }
                self.argument(argument, body)
            }
            // `f(JSON.parse(s))` with `f(x: T)`: the parameter type is the
            // declared target the parsed value enters (see
            // `json_parse_call_with_hint`).
            Argument::CallExpression(call) => {
                if let Some(parsed) = self.json_parse_call_with_hint(call, type_hint, body)? {
                    return Ok(parsed);
                }
                self.argument(argument, body)
            }
            _ => self.argument(argument, body),
        }
    }
}
