impl ModuleBuilder<'_> {
    /// Lower a `const name = (...) => ...` declaration into a HIR function item.
    fn arrow_function_const_declaration(
        &mut self,
        name_text: &str,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let function_hint = type_hint.and_then(|ty| match self.ctx.krate.types.get(ty).cloned() {
            Some(Type::Function(function)) => Some(function),
            _ => None,
        });
        let return_ty = arrow
            .return_type
            .as_ref()
            .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
            .transpose()?
            .or_else(|| function_hint.as_ref().map(|function| function.return_ty))
            .ok_or_else(|| {
                SmeltError::unsupported(
                    self.span(arrow.span.start, arrow.span.end),
                    "exported arrow function constants must have an explicit return type",
                )
            })?;
        if arrow.r#async && !matches!(self.ctx.krate.types.get(return_ty), Some(Type::Future(_))) {
            return Err(SmeltError::unsupported(
                self.span(arrow.span.start, arrow.span.end),
                "async arrow function constants must declare a Promise<T> return type",
            ));
        }

        let saved_locals = std::mem::take(&mut self.locals);
        let saved_async = self.current_async;
        self.current_async = arrow.r#async;
        let mut body = Body::new(None, self.span(arrow.body.span.start, arrow.body.span.end));
        let mut params = Vec::new();
        for param in &arrow.params.items {
            let BindingPattern::BindingIdentifier(binding) = &param.pattern else {
                self.locals = saved_locals;
                self.current_async = saved_async;
                return Err(SmeltError::unsupported(
                    self.span(param.span.start, param.span.end),
                    "destructured arrow function parameters are not lowered yet",
                ));
            };
            let ty = param
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?
                .or_else(|| {
                    function_hint
                        .as_ref()
                        .and_then(|function| function.params.get(params.len()).copied())
                })
                .ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(param.span.start, param.span.end),
                        "arrow function parameters must have explicit type annotations",
                    )
                })?;
            let param_name = self.intern_source_name(binding.name.as_str());
            let local = body.push_local(LocalDecl {
                name: Some(param_name),
                ty,
                mutable: false,
                span: self.span(binding.span.start, binding.span.end),
            });
            body.params.push(local);
            self.locals.insert(binding.name.to_string(), local);
            params.push(Param {
                name: param_name,
                local,
                ty,
                span: self.span(binding.span.start, binding.span.end),
            });
        }

        let mut errors = Vec::new();
        if arrow.expression {
            match arrow.body.statements.as_slice() {
                [Statement::ExpressionStatement(statement)] => {
                    match self.expression(&statement.expression, &mut body) {
                        Ok(value) => {
                            body.push_stmt(Stmt::Return(Some(value)));
                        }
                        Err(error) => errors.push(error),
                    }
                }
                _ => errors.push(SmeltError::unsupported(
                    self.span(arrow.body.span.start, arrow.body.span.end),
                    "expression-bodied arrow functions must contain one expression",
                )),
            }
        } else {
            for statement in &arrow.body.statements {
                if let Err(error) = self.statement(statement, &mut body) {
                    errors.push(error);
                }
            }
        }
        if arrow.r#async {
            body.build_async_state_machine();
        }
        self.locals = saved_locals;
        self.current_async = saved_async;
        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }

        let body_id = self.ctx.krate.push_body(body);
        let name = self.intern_source_name(name_text);
        let item = self.ctx.krate.push_item(Item::Function(Function {
            name,
            span: self.span(arrow.span.start, arrow.span.end),
            params,
            return_ty,
            is_async: arrow.r#async,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::Module,
        }));
        self.items.insert(name_text.to_owned(), item);
        Ok(item)
    }

    /// Convert a supported TypeScript literal expression into an importable const value.
    fn literal_const_expression(
        &mut self,
        expression: &Expression<'_>,
    ) -> Result<ConstLiteral, SmeltError> {
        match expression {
            Expression::NumericLiteral(lit) => Ok(ConstLiteral {
                literal: Literal::Float(lit.value),
                ty: self.ctx.krate.types.intern(Type::Float),
            }),
            Expression::StringLiteral(lit) => Ok(ConstLiteral {
                literal: Literal::String(lit.value.to_string()),
                ty: self.ctx.krate.types.intern(Type::String),
            }),
            Expression::BooleanLiteral(lit) => Ok(ConstLiteral {
                literal: Literal::Bool(lit.value),
                ty: self.ctx.krate.types.intern(Type::Bool),
            }),
            Expression::NullLiteral(_) => Ok(ConstLiteral {
                literal: Literal::None,
                ty: self.ctx.krate.types.intern(Type::None),
            }),
            Expression::Identifier(ident) => self
                .const_literals
                .get(ident.name.as_str())
                .cloned()
                .ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(ident.span.start, ident.span.end),
                        format!(
                            "exported const expression references unresolved const `{}`",
                            ident.name
                        ),
                    )
                }),
            Expression::ParenthesizedExpression(parenthesized) => {
                self.literal_const_expression(&parenthesized.expression)
            }
            Expression::TSAsExpression(as_expr) => {
                self.literal_const_expression(&as_expr.expression)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                self.literal_const_expression(&satisfies.expression)
            }
            Expression::TSNonNullExpression(non_null) => {
                self.literal_const_expression(&non_null.expression)
            }
            Expression::UnaryExpression(unary) => self.unary_literal_const_expression(unary),
            Expression::BinaryExpression(binary) => self.binary_literal_const_expression(binary),
            Expression::CallExpression(call) => self.call_literal_const_expression(call),
            _ => Err(SmeltError::unsupported(
                self.span(expression.span().start, expression.span().end),
                "exported const values currently support primitive literals and foldable primitive expressions",
            )),
        }
    }

    /// Return whether an exported const has known metadata value that is safe to skip.
    fn is_known_non_importable_exported_const(expression: &Expression<'_>) -> bool {
        let Expression::CallExpression(call) = expression else {
            return false;
        };
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return false;
        };
        let Expression::Identifier(object) = &member.object else {
            return false;
        };
        object.name == "Symbol" && member.property.name == "for"
    }

    /// Fold a supported unary expression inside an exported const initializer.
    fn unary_literal_const_expression(
        &mut self,
        unary: &oxc::ast::ast::UnaryExpression<'_>,
    ) -> Result<ConstLiteral, SmeltError> {
        let value = self.literal_const_expression(&unary.argument)?;
        match (unary.operator, value.literal) {
            (UnaryOperator::UnaryPlus, Literal::Float(number)) => Ok(ConstLiteral {
                literal: Literal::Float(number),
                ty: self.ctx.krate.types.intern(Type::Float),
            }),
            (UnaryOperator::UnaryNegation, Literal::Float(number)) => Ok(ConstLiteral {
                literal: Literal::Float(-number),
                ty: self.ctx.krate.types.intern(Type::Float),
            }),
            (UnaryOperator::LogicalNot, Literal::Bool(value)) => Ok(ConstLiteral {
                literal: Literal::Bool(!value),
                ty: self.ctx.krate.types.intern(Type::Bool),
            }),
            _ => Err(SmeltError::unsupported(
                self.span(unary.span.start, unary.span.end),
                "exported const unary expressions currently support numeric plus, numeric negation, and boolean not",
            )),
        }
    }

    /// Fold a supported binary expression inside an exported const initializer.
    fn binary_literal_const_expression(
        &mut self,
        binary: &oxc::ast::ast::BinaryExpression<'_>,
    ) -> Result<ConstLiteral, SmeltError> {
        let lhs = self.literal_const_expression(&binary.left)?;
        let rhs = self.literal_const_expression(&binary.right)?;
        match (lhs.literal, rhs.literal) {
            (Literal::Float(lhs), Literal::Float(rhs)) => {
                let value = match binary.operator {
                    BinaryOperator::Addition => lhs + rhs,
                    BinaryOperator::Subtraction => lhs - rhs,
                    BinaryOperator::Multiplication => lhs * rhs,
                    BinaryOperator::Division => lhs / rhs,
                    BinaryOperator::Remainder => lhs % rhs,
                    BinaryOperator::Exponential => lhs.powf(rhs),
                    _ => {
                        return Err(SmeltError::unsupported(
                            self.span(binary.span.start, binary.span.end),
                            "exported numeric const expressions support arithmetic operators only",
                        ));
                    }
                };
                Ok(ConstLiteral {
                    literal: Literal::Float(value),
                    ty: self.ctx.krate.types.intern(Type::Float),
                })
            }
            (Literal::String(lhs), Literal::String(rhs))
                if binary.operator == BinaryOperator::Addition =>
            {
                Ok(ConstLiteral {
                    literal: Literal::String(format!("{lhs}{rhs}")),
                    ty: self.ctx.krate.types.intern(Type::String),
                })
            }
            _ => Err(SmeltError::unsupported(
                self.span(binary.span.start, binary.span.end),
                "exported const binary expressions currently require matching primitive operands",
            )),
        }
    }

    /// Fold supported pure calls inside exported const initializers.
    fn call_literal_const_expression(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
    ) -> Result<ConstLiteral, SmeltError> {
        let Some(op) = stdlib_dispatch::pure_math_call(call) else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "exported const call expressions currently support only selected Math calls",
            ));
        };
        let args = call
            .arguments
            .iter()
            .map(|argument| self.number_literal_const_argument(argument))
            .collect::<Result<Vec<_>, _>>()?;
        let value = Self::fold_pure_math_const(op, &args).ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "exported const Math call has an unsupported argument count",
            )
        })?;
        Ok(ConstLiteral {
            literal: Literal::Float(value),
            ty: self.ctx.krate.types.intern(Type::Float),
        })
    }

    /// Fold one pure numeric Math operation using JavaScript-compatible f64 behavior.
    fn fold_pure_math_const(op: stdlib_dispatch::PureMathCall, args: &[f64]) -> Option<f64> {
        use stdlib_dispatch::PureMathCall;
        match op {
            PureMathCall::Abs => single_arg(args).map(f64::abs),
            PureMathCall::Floor => single_arg(args).map(f64::floor),
            PureMathCall::Ceil => single_arg(args).map(f64::ceil),
            PureMathCall::Round => single_arg(args).map(f64::round),
            PureMathCall::Trunc => single_arg(args).map(f64::trunc),
            PureMathCall::Max => Some(args.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
            PureMathCall::Min => Some(args.iter().copied().fold(f64::INFINITY, f64::min)),
            PureMathCall::Hypot => Some(args.iter().map(|arg| arg * arg).sum::<f64>().sqrt()),
            PureMathCall::Sqrt => single_arg(args).map(f64::sqrt),
            PureMathCall::Cbrt => single_arg(args).map(f64::cbrt),
            PureMathCall::Sign => single_arg(args).map(|arg| {
                if arg.is_nan() || arg == 0.0_f64 {
                    arg
                } else {
                    arg.signum()
                }
            }),
            PureMathCall::Sin => single_arg(args).map(f64::sin),
            PureMathCall::Cos => single_arg(args).map(f64::cos),
            PureMathCall::Tan => single_arg(args).map(f64::tan),
            PureMathCall::Asin => single_arg(args).map(f64::asin),
            PureMathCall::Acos => single_arg(args).map(f64::acos),
            PureMathCall::Atan => single_arg(args).map(f64::atan),
            PureMathCall::Log => single_arg(args).map(f64::ln),
            PureMathCall::Log10 => single_arg(args).map(f64::log10),
            PureMathCall::Log2 => single_arg(args).map(f64::log2),
            PureMathCall::Exp => single_arg(args).map(f64::exp),
            PureMathCall::Pow => two_args(args).map(|(base, exponent)| base.powf(exponent)),
            PureMathCall::Atan2 => two_args(args).map(|(y, x)| y.atan2(x)),
        }
    }

    /// Fold one numeric argument in an exported const call expression.
    fn number_literal_const_argument(
        &mut self,
        argument: &Argument<'_>,
    ) -> Result<f64, SmeltError> {
        let value = match argument {
            Argument::NumericLiteral(lit) => lit.value,
            Argument::Identifier(ident) => {
                let Some(ConstLiteral {
                    literal: Literal::Float(value),
                    ..
                }) = self.const_literals.get(ident.name.as_str())
                else {
                    return Err(SmeltError::unsupported(
                        self.span(ident.span.start, ident.span.end),
                        format!(
                            "exported const Math.pow argument references non-numeric const `{}`",
                            ident.name
                        ),
                    ));
                };
                *value
            }
            Argument::CallExpression(call) => {
                let ConstLiteral {
                    literal: Literal::Float(value),
                    ..
                } = self.call_literal_const_expression(call)?
                else {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "exported const Math arguments must be numeric",
                    ));
                };
                value
            }
            Argument::UnaryExpression(unary) => {
                let ConstLiteral {
                    literal: Literal::Float(value),
                    ..
                } = self.unary_literal_const_expression(unary)?
                else {
                    return Err(SmeltError::unsupported(
                        self.span(unary.span.start, unary.span.end),
                        "exported const Math.pow arguments must be numeric",
                    ));
                };
                value
            }
            Argument::BinaryExpression(binary) => {
                let ConstLiteral {
                    literal: Literal::Float(value),
                    ..
                } = self.binary_literal_const_expression(binary)?
                else {
                    return Err(SmeltError::unsupported(
                        self.span(binary.span.start, binary.span.end),
                        "exported const Math.pow arguments must be numeric",
                    ));
                };
                value
            }
            Argument::ParenthesizedExpression(parenthesized) => {
                let ConstLiteral {
                    literal: Literal::Float(value),
                    ..
                } = self.literal_const_expression(&parenthesized.expression)?
                else {
                    return Err(SmeltError::unsupported(
                        self.span(parenthesized.span.start, parenthesized.span.end),
                        "exported const Math arguments must be numeric",
                    ));
                };
                value
            }
            Argument::TSAsExpression(as_expr) => {
                let ConstLiteral {
                    literal: Literal::Float(value),
                    ..
                } = self.literal_const_expression(&as_expr.expression)?
                else {
                    return Err(SmeltError::unsupported(
                        self.span(as_expr.span.start, as_expr.span.end),
                        "exported const Math arguments must be numeric",
                    ));
                };
                value
            }
            Argument::TSSatisfiesExpression(satisfies) => {
                let ConstLiteral {
                    literal: Literal::Float(value),
                    ..
                } = self.literal_const_expression(&satisfies.expression)?
                else {
                    return Err(SmeltError::unsupported(
                        self.span(satisfies.span.start, satisfies.span.end),
                        "exported const Math arguments must be numeric",
                    ));
                };
                value
            }
            Argument::TSNonNullExpression(non_null) => {
                let ConstLiteral {
                    literal: Literal::Float(value),
                    ..
                } = self.literal_const_expression(&non_null.expression)?
                else {
                    return Err(SmeltError::unsupported(
                        self.span(non_null.span.start, non_null.span.end),
                        "exported const Math arguments must be numeric",
                    ));
                };
                value
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(argument.span().start, argument.span().end),
                    "exported const Math arguments must be foldable numeric expressions",
                ));
            }
        };
        Ok(value)
    }

    // Continued in the next split builder file.
}
