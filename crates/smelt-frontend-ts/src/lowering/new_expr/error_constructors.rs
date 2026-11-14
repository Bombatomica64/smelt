//! Error construction: `new Error(..)`, `Error(..)`, `DOMException` and the
//! error subclasses, lowered to the error record with its message, `cause` and
//! `errors` parts.

use super::super::ModuleBuilder;
use crate::SmeltError;
use crate::lowering::erasure::NotNewErasure;
use oxc::ast::ast::{Argument, Expression, ObjectPropertyKind, PropertyKey};
use oxc::span::GetSpan;
use smelt_hir::{Body, ErasureReason, Expr, ExprKind, Literal, Span, Type};

impl ModuleBuilder<'_> {
    /// Push one slot of the fixed `Error` instance layout onto a record.
    ///
    /// `ERROR_MARKER_FIELDS` declares the slots every JavaScript `Error`
    /// instance owns, and Smelt models an error as a record. A slot Smelt has no
    /// value for is therefore present and `undefined`, not missing — exactly
    /// what a hand-written Rust `struct` with an `Option` field would be, and
    /// what a `.stack`/`.cause` read already answers either way.
    ///
    /// Materializing them unconditionally is what keeps an error and a copy of
    /// it structurally equal. Every clone helper writes the whole layout back
    /// (`e2.stack = e.stack`, `new Ctor(e.message, { cause: e.cause })`), and a
    /// property STORE always creates the key — so a record that omitted the slot
    /// came back from a round trip with one property MORE than its source.
    ///
    /// The slots are already hidden from `Object.keys`/for-in by the runtime
    /// error filter, so nothing observable enumerates them.
    fn push_error_layout_entry(
        &mut self,
        entries: &mut Vec<(smelt_hir::ExprId, smelt_hir::ExprId)>,
        name: &str,
        value: Option<smelt_hir::ExprId>,
        span: Span,
        body: &mut Body,
    ) {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        // not a new erasure: a slot of the erased Error record, recorded by its builder
        let unknown_ty = self.unknown_type(NotNewErasure::ErasedUpstream);
        let key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(name.to_owned())),
            ty: string_ty,
            span,
        });
        let value = value.unwrap_or_else(|| {
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Undefined),
                ty: unknown_ty,
                span,
            })
        });
        entries.push((key, value));
    }

    /// Build the erased-`Error` record used by `new Error(...)` from an optional
    /// message expression already lowered in `body`.
    ///
    /// Shared by the direct `new Error(message)` path and the constructor-value
    /// adapter (`class_constructor_value_expression`) so both stamp the same
    /// `__smelt_error` marker + `message` shape, keeping `instanceof Error`
    /// identity consistent regardless of how the constructor was invoked. When no
    /// message is supplied the default `"Error"` string is used, matching the
    /// zero-argument `new Error()` behavior.
    pub(in crate::lowering) fn error_object_from_message(
        &mut self,
        message: Option<smelt_hir::ExprId>,
        class_name: &str,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let unknown_ty = self.erase_with(ErasureReason::UntypedBuiltin, "Error record");
        let dict_ty = self
            .ctx
            .krate
            .types
            .intern(Type::Dict(string_ty, unknown_ty));
        let message = message.unwrap_or_else(|| {
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String("Error".to_owned())),
                ty: string_ty,
                span,
            })
        });
        let marker_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("__smelt_error".to_owned())),
            ty: string_ty,
            span,
        });
        // The marker VALUE is the constructor's class name, not a bare `true`.
        // Every error class shared one boolean marker, so `instance_of_expr`
        // could not tell them apart: `new Error('x') instanceof AggregateError`
        // answered `true`, which sent es-toolkit `clone` down the AggregateError
        // branch and rebuilt the error as `new Ctor(obj.errors, obj.message, ..)`
        // — putting `errors` in the message slot and losing the message. It also
        // makes `.name` truthful, which `smelt_get_object_field` reads back.
        let marker_value = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(class_name.to_owned())),
            ty: string_ty,
            span,
        });
        let message_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("message".to_owned())),
            ty: string_ty,
            span,
        });
        let mut entries = vec![(marker_key, marker_value), (message_key, message)];
        self.push_error_layout_entry(&mut entries, "stack", None, span, body);
        self.push_error_layout_entry(&mut entries, "cause", None, span, body);
        let object = body.push_expr(Expr {
            kind: ExprKind::DictLit(entries),
            ty: dict_ty,
            span,
        });
        body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: object,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span,
        })
    }

    /// Lower a built-in Error constructor used as a value to an erased Error object.
    ///
    /// The record carries the shared `__smelt_error` identity marker and the
    /// `message`, plus the retained ES2022 `cause` option and the
    /// `AggregateError` `errors` list when the construction spells them (see
    /// `error_constructor_parts`). `cause`/`errors` mirror JavaScript's
    /// non-enumerable own error properties: reads resolve through the record,
    /// and the runtime for-in/`Object.keys` filters hide them alongside
    /// `message` (see `smelt_is_for_in_object_key`).
    pub(in crate::lowering) fn error_object_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let parts = self.error_constructor_parts(new_expr, body)?;
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let unknown_ty = self.erase_with(ErasureReason::UntypedBuiltin, "Error record");
        let dict_ty = self
            .ctx
            .krate
            .types
            .intern(Type::Dict(string_ty, unknown_ty));
        let span = self.span(new_expr.span.start, new_expr.span.end);
        // Which error class was spelled. Falls back to `Error` for a callee shape
        // that is not a bare identifier, which is the base class anyway.
        let class_name = match &new_expr.callee {
            Expression::Identifier(callee) => callee.name.to_string(),
            _ => "Error".to_owned(),
        };
        let marker_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("__smelt_error".to_owned())),
            ty: string_ty,
            span,
        });
        // See `error_object_from_message`: the marker value carries the class name
        // so error subclasses stay distinguishable and `.name` reads truthfully.
        let marker_value = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(class_name)),
            ty: string_ty,
            span,
        });
        let message_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("message".to_owned())),
            ty: string_ty,
            span,
        });
        let mut entries = vec![(marker_key, marker_value), (message_key, parts.message)];
        self.push_error_layout_entry(&mut entries, "stack", None, span, body);
        self.push_error_layout_entry(&mut entries, "cause", parts.cause, span, body);
        if let Some(errors) = parts.errors {
            let errors_key = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String("errors".to_owned())),
                ty: string_ty,
                span,
            });
            entries.push((errors_key, errors));
        }
        let object = body.push_expr(Expr {
            kind: ExprKind::DictLit(entries),
            ty: dict_ty,
            span,
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: object,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span,
        }))
    }

    /// Lower `new DOMException(message?, name?)` to a concrete marker record.
    ///
    /// `DOMException` is a host error class. es-toolkit re-exports it (with an
    /// `Error` fallback for runtimes without it) and uses it only as the base of
    /// `AbortError`/`TimeoutError` and via `value instanceof DOMException`. Rather
    /// than erase it to a shapeless `SmeltUnknown`, model it like `Error`: a record
    /// carrying a dedicated `__smelt_domexception` marker plus its `message` and
    /// `name`, so the identity survives later dynamic `instanceof` checks (see
    /// `instance_of_expr`). The two-argument form is `(message, name)`; the name
    /// defaults to `"Error"` to match the spec fallback path es-toolkit relies on.
    pub(in crate::lowering) fn domexception_object_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let unknown_ty = self.erase_with(ErasureReason::UntypedBuiltin, "DOMException");
        let dict_ty = self
            .ctx
            .krate
            .types
            .intern(Type::Dict(string_ty, unknown_ty));
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let message = match new_expr.arguments.first() {
            Some(message_arg) => {
                let message = self.argument(message_arg, body)?;
                if self.ctx.krate.types.get(Self::expr_ty(body, message)) == Some(&Type::String) {
                    message
                } else if self.is_string_compatible_type(Self::expr_ty(body, message))
                    || self.type_contains_unknown(Self::expr_ty(body, message))
                {
                    body.push_expr(Expr {
                        kind: ExprKind::TypeAssert { value: message },
                        ty: string_ty,
                        span: self.span(message_arg.span().start, message_arg.span().end),
                    })
                } else {
                    return Err(SmeltError::unsupported(
                        self.span(message_arg.span().start, message_arg.span().end),
                        "DOMException constructor message must be a string",
                    ));
                }
            }
            None => body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(String::new())),
                ty: string_ty,
                span,
            }),
        };
        let name = match new_expr.arguments.get(1) {
            Some(name_arg) => {
                let name = self.argument(name_arg, body)?;
                if self.ctx.krate.types.get(Self::expr_ty(body, name)) == Some(&Type::String) {
                    name
                } else if self.is_string_compatible_type(Self::expr_ty(body, name))
                    || self.type_contains_unknown(Self::expr_ty(body, name))
                {
                    body.push_expr(Expr {
                        kind: ExprKind::TypeAssert { value: name },
                        ty: string_ty,
                        span: self.span(name_arg.span().start, name_arg.span().end),
                    })
                } else {
                    return Err(SmeltError::unsupported(
                        self.span(name_arg.span().start, name_arg.span().end),
                        "DOMException constructor name must be a string",
                    ));
                }
            }
            None => body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String("Error".to_owned())),
                ty: string_ty,
                span,
            }),
        };
        let marker_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("__smelt_domexception".to_owned())),
            ty: string_ty,
            span,
        });
        let marker_value = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(true)),
            ty: bool_ty,
            span,
        });
        let message_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("message".to_owned())),
            ty: string_ty,
            span,
        });
        let name_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("name".to_owned())),
            ty: string_ty,
            span,
        });
        let object = body.push_expr(Expr {
            kind: ExprKind::DictLit(vec![
                (marker_key, marker_value),
                (message_key, message),
                (name_key, name),
            ]),
            ty: dict_ty,
            span,
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: object,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span,
        }))
    }

    /// Lower the positional pieces of a builtin Error construction exactly once.
    ///
    /// Supported shapes, following ES2022:
    /// - `new Error(message?, options?)` (and the `EvalError`..`URIError`
    ///   subclasses), where `options` is an object literal whose `cause`
    ///   property is retained;
    /// - `new AggregateError(errors, message?, options?)`, whose leading
    ///   `errors` iterable is retained as a list value.
    ///
    /// A missing message lowers to the literal `"Error"`, a concrete `string`
    /// passes through, and a string-compatible or erased operand is asserted to
    /// `string`. A non-literal `options` expression is rejected with an honest
    /// blocker rather than guessed at, because whether a `cause` is attached
    /// depends on `"cause" in options`, which a general static rule can only answer
    /// for a literal spelling. Options properties other than `cause` are lowered
    /// for their effects and discarded, matching JavaScript (only `cause` is read).
    fn error_constructor_parts(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<ErrorConstructorParts, SmeltError> {
        let is_aggregate = matches!(
            &new_expr.callee,
            Expression::Identifier(callee) if self.names_host(callee.name.as_str(), "AggregateError")
        );
        let mut arguments = new_expr.arguments.iter();
        let errors = if is_aggregate {
            match arguments.next() {
                Some(errors_arg) => Some(self.argument(errors_arg, body)?),
                None => None,
            }
        } else {
            None
        };
        let message_arg = arguments.next();
        let options_arg = arguments.next();
        if arguments.next().is_some() {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "Error constructor lowering supports at most a message and an options argument",
            ));
        }
        let message = match message_arg {
            None => {
                let ty = self.ctx.krate.types.intern(Type::String);
                body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String("Error".to_owned())),
                    ty,
                    span: self.span(new_expr.span.start, new_expr.span.end),
                })
            }
            Some(message_arg) => {
                let message = self.argument(message_arg, body)?;
                if self.ctx.krate.types.get(Self::expr_ty(body, message)) == Some(&Type::String) {
                    message
                } else if self.is_string_compatible_type(Self::expr_ty(body, message))
                    || self.type_contains_unknown(Self::expr_ty(body, message))
                {
                    let ty = self.ctx.krate.types.intern(Type::String);
                    body.push_expr(Expr {
                        kind: ExprKind::TypeAssert { value: message },
                        ty,
                        span: self.span(message_arg.span().start, message_arg.span().end),
                    })
                } else {
                    return Err(SmeltError::unsupported(
                        self.span(message_arg.span().start, message_arg.span().end),
                        "Error constructor message must be a string",
                    ));
                }
            }
        };
        let cause = match options_arg {
            None => None,
            Some(Argument::ObjectExpression(options)) => {
                let mut cause = None;
                for property in &options.properties {
                    let ObjectPropertyKind::ObjectProperty(property) = property else {
                        return Err(SmeltError::unsupported(
                            self.span(options.span.start, options.span.end),
                            "Error constructor options must not use spread properties",
                        ));
                    };
                    let value = self.expression(&property.value, body)?;
                    if matches!(&property.key, PropertyKey::StaticIdentifier(key) if key.name == "cause")
                    {
                        cause = Some(value);
                    }
                }
                cause
            }
            Some(options_arg) => {
                return Err(SmeltError::unsupported(
                    self.span(options_arg.span().start, options_arg.span().end),
                    "Error constructor options must be an object literal with an optional cause property",
                ));
            }
        };
        Ok(ErrorConstructorParts {
            message,
            cause,
            errors,
        })
    }

    /// Lower `Error(message)`-style calls to the message value used by HIR throws.
    pub(in crate::lowering) fn error_function_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if call.arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Error function lowering supports at most one message argument",
            ));
        }
        let Some(message_arg) = call.arguments.first() else {
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String("Error".to_owned())),
                ty,
                span: self.span(call.span.start, call.span.end),
            }));
        };
        let message = self.argument(message_arg, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, message)) == Some(&Type::String) {
            return Ok(message);
        }
        if self.is_string_compatible_type(Self::expr_ty(body, message))
            || self.type_contains_unknown(Self::expr_ty(body, message))
        {
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: message },
                ty,
                span: self.span(message_arg.span().start, message_arg.span().end),
            }));
        }
        Err(SmeltError::unsupported(
            self.span(message_arg.span().start, message_arg.span().end),
            "Error function message must be a string",
        ))
    }
}

/// The lowered positional pieces of a builtin Error construction.
///
/// Produced by `error_constructor_parts` so the throw path (which keeps only
/// the message) and the error-record path (which retains everything) lower the
/// source arguments exactly once.
struct ErrorConstructorParts {
    /// The `string`-typed message expression (`"Error"` literal when absent).
    message: smelt_hir::ExprId,
    /// The retained ES2022 `cause` option value, when the construction spells
    /// an options object literal with a `cause` property.
    cause: Option<smelt_hir::ExprId>,
    /// The retained `AggregateError` leading `errors` argument.
    errors: Option<smelt_hir::ExprId>,
}
