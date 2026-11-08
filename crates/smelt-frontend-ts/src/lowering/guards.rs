//! Runtime tests as expressions: `instanceof`, `typeof`, nullish comparisons and
//! type assertions.

use crate::lowering::body_arena::BodyArena;
use crate::lowering::peel::{Wrappers, peel};
use super::{ModuleBuilder, ambient_globals, unknown_kind_from_typeof};
use crate::SmeltError;
use crate::lowering::js_text::js_str_text;
use oxc::ast::ast::{Expression, TSType, TSTypeName};
use oxc::span::GetSpan;
use oxc::syntax::operator::{BinaryOperator, UnaryOperator};
use smelt_hir::{BinOp, Body, Expr, ExprKind, Literal, Type, UnaryOp, UnknownKind};

impl ModuleBuilder<'_> {
    /// Return whether a source constructor name resolves to a modeled TypeScript stdlib class.
    pub(super) fn is_ts_stdlib_class_name(name: &str, class: smelt_stdlib::StdlibClass) -> bool {
        smelt_stdlib::typescript_stdlib_class(name) == Some(class)
    }

    /// Lower a TypeScript `instanceof` binary expression into a HIR predicate.
    pub(super) fn instanceof_expression(
        &mut self,
        binary: &oxc::ast::ast::BinaryExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(binary.span.start, binary.span.end);
        if let Some(selected) =
            self.conditional_class_select(Some(&binary.right), span, body, |this, body| this.instanceof_expression(binary, body))?
        {
            return Ok(selected);
        }
        let value = self.expression(&binary.left, body)?;
        self.instanceof_test(value, binary.left.span(), &binary.right, binary.span, body)
    }

    /// Build the `instanceof` predicate of an already lowered `value`.
    ///
    /// Shared by the source operator and `expect(value).toBeInstanceOf(Ctor)`.
    /// `value_span` locates the value and `span` the whole test.
    pub(in crate::lowering) fn instanceof_test(
        &mut self,
        value: smelt_hir::ExprId,
        value_span: oxc::span::Span,
        target: &Expression<'_>,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let value_ty = Self::expr_ty(body, value);
        if let Expression::StaticMemberExpression(member) = target
            && (self
                .namespace_member_name(member)
                .is_some_and(|(namespace, _)| self.imports.is_namespace(namespace))
                || matches!(
                    &member.object,
                    Expression::Identifier(object) if self.imports.is_value(object.name.as_str())
                ))
        {
            let ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(false)),
                ty,
                span: self.span(span.start, span.end),
            }));
        }
        // A constructor read from a value (`pair.errorClass`) is only known at
        // run time, so the prototype-chain walk runs there too.
        let Expression::Identifier(class_ident) = target else {
            let target = self.expression(target, body)?;
            let ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::InstanceOfValue { value, target },
                ty,
                span: self.span(span.start, span.end),
            }));
        };
        let class_text = class_ident.name.as_str();
        // `x instanceof Uint8Array` (any typed-array view) resolves through the
        // view's registry marker, like every other byte-backed host object — the
        // typed arrays are no longer numeric lists whose identity had to be
        // guessed from the static type, so this needs no fold of its own and falls
        // through to the `InstanceOf` path below.
        // `x instanceof Array`. Smelt backs a JavaScript array with a plain list,
        // so this asks exactly the question `Array.isArray(x)` asks (see
        // `array_is_array_call`) and shares its single rule, `static_array_match`:
        // fold only when the operand's static type settles the answer, otherwise
        // resolve it through the runtime array probe `UnknownIs { Array }`. A
        // user-declared `class Array` owns the name and falls through to the
        // ordinary class path below.
        if class_text == "Array" && !self.classes.contains("Array") {
            let ty = self.ctx.krate.types.intern(Type::Bool);
            let Some(result) = self.static_array_match(value_ty) else {
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::UnknownIs {
                        value,
                        kind: UnknownKind::Array,
                    },
                    ty,
                    span: self.span(span.start, span.end),
                }));
            };
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(result)),
                ty,
                span: self.span(span.start, span.end),
            }));
        }
        // A `Future` is the Rust side of a `Promise`, so its static type settles
        // `x instanceof Promise`.
        if class_text == "Promise"
            && !self.classes.contains("Promise")
            && matches!(self.ctx.krate.types.get(value_ty), Some(Type::Future(_)))
        {
            let ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(true)),
                ty,
                span: self.span(span.start, span.end),
            }));
        }
        if Self::is_ts_stdlib_class_name(class_text, smelt_stdlib::StdlibClass::Date)
            && self.expression_is_known_date_value(value, body)
        {
            let ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(true)),
                ty,
                span: self.span(span.start, span.end),
            }));
        }
        // `Date` and `RegExp` both erase to marker-bearing objects
        // (`__smelt_date` / `__smelt_regexp`), so an ERASED operand's identity is
        // recoverable at runtime and must not be folded away — `instance_of_text`
        // emits the marker probe. Folding `RegExp` unconditionally is what made
        // es-toolkit `cloneDeepWithImpl` skip its `valueToClone instanceof RegExp`
        // branch for an `unknown`-typed regex and fall through to the generic
        // `Object.create(getPrototypeOf(x))` path, losing `source` and `flags`.
        // Concrete operands still fold: their storage carries no marker.
        //
        // RegExp exempts only the fully erased types, NOT `Union`/`Optional` as
        // Date does. A concrete union stores a tagged enum (`SmeltUnion*`), and
        // routing one into the marker probe emits a `SmeltUnknown` match against
        // that enum — which is what broke `truncate.rs`, where the receiver is an
        // `Optional<Union>` separator. Date's wider set is left exactly as it was.
        let regexp_target =
            Self::is_ts_stdlib_class_name(class_text, smelt_stdlib::StdlibClass::RegExp);
        let date_target =
            Self::is_ts_stdlib_class_name(class_text, smelt_stdlib::StdlibClass::Date);
        let operand_keeps_marker_identity = (date_target
            && matches!(
                self.ctx.krate.types.get(value_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Optional(_))
            ))
            || (regexp_target
                && matches!(
                    self.ctx.krate.types.get(value_ty),
                    Some(Type::Unknown | Type::TypeParam { .. })
                ));
        if Self::instanceof_fold_false_builtin_target(class_text)
            && !operand_keeps_marker_identity
            && !self.instanceof_concrete_class(value_ty)
        {
            let ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(false)),
                ty,
                span: self.span(span.start, span.end),
            }));
        }
        let builtin_target = Self::instanceof_builtin_target(class_text);
        if !self.instanceof_supported_left_operand(value_ty) {
            return Err(SmeltError::unsupported(
                self.span(value_span.start, value_span.end),
                "TypeScript instanceof requires a concrete class-typed left operand",
            ));
        }
        if !builtin_target
            // Module class names are collected before any class body is
            // lowered. A class currently under construction is therefore a
            // valid nominal target even though its final HIR item is inserted
            // only after all of its methods have been emitted.
            && !self.classes.is_known(class_text)
            && !self.imports.is_value(class_text)
            // A class named from inside its own members (`input instanceof
            // Some` in a method of a block-local `Some`) is the class being
            // lowered, nominal before its item is inserted; a block-local class
            // shadowing another of its name is bound under its scoped name.
            && self.current_class.as_deref() != Some(class_text)
            && self.classes.scoped_type_name(class_text).is_none()
        {
            // `x instanceof f` against a *function* value (the JS
            // constructor-function idiom, as in lodash-compat `bind`/`curry`
            // and es-toolkit `partial`). Every non-arrow JavaScript function is
            // a constructor, so this is `OrdinaryHasInstance`: walk `x`'s
            // prototype chain looking for the object `f.prototype`. It cannot be
            // answered at compile time — which function value the binding holds,
            // and what its `prototype` is, are both runtime facts — so it
            // lowers to the runtime predicate rather than folding.
            //
            // Classes are not first-class values in Smelt, so a target that
            // resolves to a local binding or function item can only be a
            // function value, never a nominal class; the nominal path below
            // keeps its compile-time marker probe.
            let target_is_function_value = self.scope.is_bound(class_text)
                || self
                    .items
                    .get(class_text)
                    .is_some_and(|&item| matches!(self.item_ref(item), smelt_hir::Item::Function(_)));
            // The one shape that cannot be lowered: `this instanceof wrapper`
            // read from INSIDE `wrapper`'s own body (the JS new-detection idiom
            // in lodash-compat `curry`/`curryRight`). The constructor value is
            // the closure being defined, so the body has no binding for it --
            // a self-capturing closure is a capability Smelt does not have yet
            // -- and naming it would emit an unresolved local. Such a target
            // keeps the previous `false` answer; every namable target walks the
            // chain. Recorded here rather than silently: JavaScript answers
            // `true` for a `new`-invocation, so this stays wrong until a
            // closure can capture itself.
            let target_is_namable = !self
                .defining_local_functions
                .iter()
                .any(|defining| defining == class_text);
            if target_is_function_value && !target_is_namable {
                let ty = self.ctx.krate.types.intern(Type::Bool);
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Bool(false)),
                    ty,
                    span: self.span(span.start, span.end),
                }));
            }
            if target_is_function_value {
                let target = self.identifier_expression(
                    class_text,
                    class_ident.span.start,
                    class_ident.span.end,
                    body,
                )?;
                let ty = self.ctx.krate.types.intern(Type::Bool);
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::InstanceOfValue { value, target },
                    ty,
                    span: self.span(span.start, span.end),
                }));
            }
            return Err(SmeltError::for_unresolved_name(
                self.span(class_ident.span.start, class_ident.span.end),
                class_text,
                format!("TypeScript instanceof target `{class_text}` is not a lowered class"),
            ));
        }
        let class = if let Some(scoped) = self.classes.scoped_type_name(class_text) {
            scoped
        } else {
            let aliased = self.classes.alias_target(class_text).to_owned();
            self.intern_type_name(&aliased)
        };
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(body.push_expr(Expr {
            kind: ExprKind::InstanceOf { value, class },
            ty,
            span: self.span(span.start, span.end),
        }))
    }

    /// Return true when an expression is a built-in constructor target.
    ///
    /// Any class with a host-object registry marker is a valid target: the marker
    /// exists so `instanceof` can be answered, whether or not the class has a
    /// modeled surface. The same holds for a modeled stdlib class and an
    /// ECMAScript error class; `Promise` answers through its runtime tag.
    pub(super) fn instanceof_builtin_target(target: &str) -> bool {
        target == "Promise"
            || smelt_stdlib::host_object_marker(target).is_some()
            || smelt_stdlib::typescript_stdlib_class(target).is_some()
            || smelt_stdlib::is_error_class_name(target)
    }

    /// Return true for host global constructors that Smelt always models as
    /// present, so `typeof X === 'undefined'` environment-support guards fold to
    /// a constant instead of failing to resolve the bare `X` identifier.
    ///
    /// The set must stay in lock-step with what codegen actually models: each
    /// name here has a concrete constructor lowering and a working `instanceof`
    /// path (the marker-only host builtins, the byte-backed host objects, and
    /// `Blob`/`File`). Folding a presence guard `true` for a name whose positive
    /// branch the runtime cannot satisfy would reintroduce the erased-vs-runtime
    /// disagreement the globals plan warns against, so unmodeled host globals are
    /// deliberately excluded.
    pub(super) fn is_known_defined_global_constructor(name: &str) -> bool {
        matches!(name, "Blob" | "File")
            || Self::marker_only_builtin_marker(name).is_some()
            || smelt_stdlib::byte_buffer_role(name).is_some()
    }

    /// Answer a `typeof x === '…'` / `!==` comparison from its equality-form
    /// `check`: the check itself for `===` and `==`, its negation for `!==` and
    /// `!=`.
    fn typeof_comparison_result(
        &mut self,
        operator: BinaryOperator,
        check: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        if matches!(
            operator,
            BinaryOperator::StrictInequality | BinaryOperator::Inequality
        ) {
            self.unary_bool_expr(UnaryOp::Not, check, span, body)
        } else {
            check
        }
    }

    /// Return true for builtin targets represented by non-class HIR values today.
    ///
    /// `Map` and `Set` are intentionally absent: each source value erases to a
    /// marker object (`{ __smelt_map: [...] }` / `{ __smelt_set: [...] }`), so
    /// `value instanceof Map`/`instanceof Set` resolves through that marker in
    /// `instance_of_text` rather than folding to a constant `false`. For `Date`
    /// and `RegExp` the `false` fold applies only to a concrete operand, whose
    /// storage carries no marker; an erased operand keeps its marker and is
    /// probed at runtime (see `operand_keeps_marker_identity` in the
    /// `instanceof` lowering).
    pub(super) fn instanceof_fold_false_builtin_target(target: &str) -> bool {
        matches!(
            smelt_stdlib::typescript_stdlib_class(target),
            Some(smelt_stdlib::StdlibClass::Date | smelt_stdlib::StdlibClass::RegExp)
        )
    }

    /// Return true when `instanceof` can be emitted as a concrete HIR class check.
    pub(super) fn instanceof_concrete_class(&self, ty: smelt_hir::TypeId) -> bool {
        matches!(self.ctx.krate.types.get(ty), Some(Type::Class { .. }))
    }

    /// Return true when an `instanceof` left operand can participate in a lowered guard.
    pub(super) fn instanceof_supported_left_operand(&self, ty: smelt_hir::TypeId) -> bool {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
        {
            Some(
                Type::Class { .. }
                | Type::Unknown
                | Type::TypeParam { .. }
                | Type::Future(_)
                | Type::Float
                | Type::Int
                | Type::String
                | Type::Bool
                | Type::None
                // A plain object/record value (`transform(obj): Record<…>`)
                // carries no nominal class identity in Smelt's record model, so
                // `value instanceof UserClass` resolves through the concrete
                // `InstanceOf` codegen to `false` instead of aborting the build
                // (records are never instances of a user-declared class here).
                // A source `Map` (`JsMap`) is likewise a supported operand: it is
                // `instanceof Map` and `false` for any other class. A source `Set`
                // is analogously `instanceof Set` and `false` for any other class.
                | Type::Dict(..)
                | Type::JsMap(..)
                | Type::Set(..),
            ) => true,
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.instanceof_supported_left_operand(item)),
            Some(Type::Optional(item)) => self.instanceof_supported_left_operand(*item),
            _ => false,
        }
    }

    /// Lower `typeof value === "kind"` checks using known HIR types when possible.
    pub(super) fn unknown_typeof_comparison(
        &mut self,
        binary: &oxc::ast::ast::BinaryExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !matches!(
            binary.operator,
            BinaryOperator::StrictEquality
                | BinaryOperator::StrictInequality
                | BinaryOperator::Equality
                | BinaryOperator::Inequality
        ) {
            return Ok(None);
        }
        if let Some(expr) = self.global_typeof_probe(binary, body) {
            return Ok(Some(expr));
        }
        let Expression::UnaryExpression(unary) = &binary.left else {
            return Ok(None);
        };
        if unary.operator != UnaryOperator::Typeof {
            return Ok(None);
        }
        let Expression::StringLiteral(kind_lit) = &binary.right else {
            return Ok(None);
        };
        if kind_lit.value == "undefined"
            && let Expression::Identifier(identifier) = &unary.argument
            && identifier.name == "crypto"
        {
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            let result = !matches!(
                binary.operator,
                BinaryOperator::StrictInequality | BinaryOperator::Inequality
            );
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(result)),
                ty: bool_ty,
                span: self.span(binary.span.start, binary.span.end),
            })));
        }
        // Ambient globals that the default deterministic non-DOM, non-Node
        // profile models as *absent* (e.g. `Buffer`, accessed bare or through a
        // global alias as `globalThis.Buffer`). Their `typeof` existence guards
        // fold to the absent answer: `=== 'undefined'` is `true`, `!==` is
        // `false`. es-toolkit's `isBuffer` (`typeof globalThis.Buffer !==
        // 'undefined' && globalThis.Buffer.isBuffer(x)`) then short-circuits to a
        // constant `false`, which is the correct result in a non-Node runtime,
        // instead of resolving `globalThis.Buffer` to a bogus empty object.
        if kind_lit.value == "undefined" && self.typeof_operand_is_absent_global(&unary.argument) {
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            let result = !matches!(
                binary.operator,
                BinaryOperator::StrictInequality | BinaryOperator::Inequality
            );
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(result)),
                ty: bool_ty,
                span: self.span(binary.span.start, binary.span.end),
            })));
        }
        // A modeled host constructor the crate *reassigns* somewhere
        // (`globalThis.Blob = undefined`) can no longer fold its presence guard
        // to a constant: `typeof Blob === 'undefined'` becomes a dynamic slot
        // probe so `isBlob`/`isFile` observe the override at runtime. Handles the
        // bare identifier spelling (`typeof Blob`) and the global-alias member
        // spelling (`typeof globalThis.Blob`).
        if kind_lit.value == "undefined"
            && let Some(name) = self.typeof_operand_written_host_global(&unary.argument)
        {
            let name = name.to_owned();
            let negated = !matches!(
                binary.operator,
                BinaryOperator::StrictInequality | BinaryOperator::Inequality
            );
            return Ok(Some(self.host_global_present_expr(
                &name,
                negated,
                binary.span,
                body,
            )));
        }
        // Modeled host constructors (e.g. `Blob`) are always present, so the
        // `typeof Blob === 'undefined'` support guards used by `isBlob` and the
        // `cloneDeepWith` clone paths fold to a constant: `=== 'undefined'` is
        // `false`, `!== 'undefined'` is `true`. Without this, the bare `Blob`
        // identifier would fail to resolve as a value.
        if kind_lit.value == "undefined"
            && let Expression::Identifier(identifier) = &unary.argument
            && Self::is_known_defined_global_constructor(identifier.name.as_str())
        {
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            let result = matches!(
                binary.operator,
                BinaryOperator::StrictInequality | BinaryOperator::Inequality
            );
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(result)),
                ty: bool_ty,
                span: self.span(binary.span.start, binary.span.end),
            })));
        }
        if kind_lit.value == "undefined" {
            let value = self.expression(&unary.argument, body)?;
            let value_ty = Self::expr_ty(body, value);
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            if matches!(self.ctx.krate.types.get(value_ty), Some(Type::Optional(_))) {
                let check = body.push_expr(Expr {
                    kind: ExprKind::UnknownIs {
                        value,
                        kind: UnknownKind::Null,
                    },
                    ty: bool_ty,
                    span: self.span(binary.span.start, binary.span.end),
                });
                return Ok(Some(self.typeof_comparison_result(binary.operator, check, binary.span, body)));
            }
            // A statically *erased* operand has no known `typeof` answer, so the
            // presence question has to survive to runtime. `type_matches_typeof`
            // has no `Unknown` arm and answers `false` for it, which would fold
            // `typeof <erased> !== 'undefined'` to the constant `true` — the
            // exact "only fold when the type pins a single spelling" rule that
            // `typeof_expression` already documents. Emit the runtime tag test
            // instead for every type whose runtime tag is not pinned: `unknown`,
            // a union (whose arms disagree unless *all* of them match, which the
            // static answer below already handles), and an unconstrained type
            // parameter.
            if self.typeof_answer_is_dynamic(value_ty) {
                let check = body.push_expr(Expr {
                    kind: ExprKind::UnknownIs {
                        value,
                        kind: UnknownKind::Undefined,
                    },
                    ty: bool_ty,
                    span: self.span(binary.span.start, binary.span.end),
                });
                return Ok(Some(self.typeof_comparison_result(binary.operator, check, binary.span, body)));
            }
            let matches_kind = self.type_matches_typeof(value_ty, "undefined");
            let result = if matches!(
                binary.operator,
                BinaryOperator::StrictInequality | BinaryOperator::Inequality
            ) {
                !matches_kind
            } else {
                matches_kind
            };
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(result)),
                ty: bool_ty,
                span: self.span(binary.span.start, binary.span.end),
            })));
        }
        let expected = js_str_text(kind_lit.value);
        let Some(kind) = unknown_kind_from_typeof(&expected) else {
            return Err(SmeltError::unsupported(
                self.span(kind_lit.span.start, kind_lit.span.end),
                format!("typeof narrowing kind `{expected}` is not supported yet"),
            ));
        };
        let value = self.expression(&unary.argument, body)?;
        let value_ty = Self::expr_ty(body, value);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        if let Some(Type::Optional(inner)) = self.ctx.krate.types.get(value_ty).cloned()
            && self.static_typeof_match(inner, &expected) == Some(true)
        {
            let absent = body.push_expr(Expr {
                kind: ExprKind::UnknownIs {
                    value,
                    kind: UnknownKind::Null,
                },
                ty: bool_ty,
                span: self.span(binary.span.start, binary.span.end),
            });
            let present = self.unary_bool_expr(UnaryOp::Not, absent, binary.span, body);
            if matches!(
                binary.operator,
                BinaryOperator::StrictInequality | BinaryOperator::Inequality
            ) {
                return Ok(Some(self.unary_bool_expr(
                    UnaryOp::Not,
                    present,
                    binary.span,
                    body,
                )));
            }
            return Ok(Some(present));
        }
        if let Some(matches_kind) = self.static_typeof_match(value_ty, &expected) {
            let result = if matches!(
                binary.operator,
                BinaryOperator::StrictInequality | BinaryOperator::Inequality
            ) {
                !matches_kind
            } else {
                matches_kind
            };
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(result)),
                ty: bool_ty,
                span: self.span(binary.span.start, binary.span.end),
            })));
        }
        let check = body.push_expr(Expr {
            kind: ExprKind::UnknownIs { value, kind },
            ty: bool_ty,
            span: self.span(binary.span.start, binary.span.end),
        });
        Ok(Some(self.typeof_comparison_result(binary.operator, check, binary.span, body)))
    }

    /// Fold a `typeof <global-alias> ===/!== "<kind>"` feature probe to a literal.
    ///
    /// In the non-DOM Node-compatible profile every recognized global alias
    /// (`globalThis`, `global`, `self`) is a present object, so existence probes
    /// such as `typeof globalThis !== "undefined"` and `typeof globalThis ===
    /// "object"` have a known answer and never observe the global object's
    /// identity. The probe is matched in either operand order. Anything that is
    /// not a recognized existence probe returns `None` so it falls through to the
    /// ordinary `typeof` comparison handling (which keeps honest blockers for
    /// real dynamic global usage).
    pub(super) fn global_typeof_probe(
        &mut self,
        binary: &oxc::ast::ast::BinaryExpression<'_>,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let is_equality = !matches!(
            binary.operator,
            BinaryOperator::StrictInequality | BinaryOperator::Inequality
        );
        // Accept `typeof g <op> "kind"` and the mirrored `"kind" <op> typeof g`.
        let (typeof_side, literal_side) = (&binary.left, &binary.right);
        let probe = ambient_globals::typeof_identifier_name(typeof_side)
            .map(|name| (name, literal_side))
            .or_else(|| {
                ambient_globals::typeof_identifier_name(&binary.right)
                    .map(|name| (name, &binary.left))
            });
        let (operand_name, literal) = probe?;
        let Expression::StringLiteral(kind_lit) = literal else {
            return None;
        };
        let operand_is_global_alias = self.is_ambient_global_alias(operand_name);
        let value = ambient_globals::global_typeof_probe_value(
            operand_is_global_alias,
            &js_str_text(kind_lit.value),
            is_equality,
        )?;
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(value)),
            ty: bool_ty,
            span: self.span(binary.span.start, binary.span.end),
        }))
    }

    /// Return whether the `typeof` answer for a type is decided only at runtime.
    ///
    /// A type pins its runtime tag when every value it can hold answers the same
    /// `typeof`. `unknown` pins nothing; an unconstrained type parameter pins
    /// nothing; a union pins its answer only when its arms agree. Anything else
    /// (a concrete primitive, list, class, `Optional`) has a static answer, so
    /// the caller may keep folding the comparison to a literal.
    ///
    /// Used by [`Self::unknown_typeof_comparison`] so an erased operand emits a
    /// runtime tag test instead of a fabricated constant.
    pub(super) fn typeof_answer_is_dynamic(&self, ty: smelt_hir::TypeId) -> bool {
        let resolved = self.type_param_constraint_or_self(ty);
        match self.ctx.krate.types.get(resolved) {
            // An unconstrained type parameter resolves to itself here.
            Some(Type::Unknown | Type::TypeParam { .. }) => true,
            Some(Type::Union(items)) => {
                let items = items.clone();
                let mut agrees = None;
                for item in items {
                    if self.typeof_answer_is_dynamic(item) {
                        return true;
                    }
                    let matches_kind = self.type_matches_typeof(item, "undefined");
                    if *agrees.get_or_insert(matches_kind) != matches_kind {
                        return true;
                    }
                }
                false
            }
            _ => false,
        }
    }

    /// Return whether a name resolves to the ambient global object in this module.
    ///
    /// A base alias spelling (`globalThis` / `global` / `self`) only counts when it
    /// is *not* shadowed by a module-local binding — an import, declared item,
    /// module global, or local variable. es-toolkit, for example, imports its own
    /// `globalThis` shim (`import { globalThis } from "../_internal/globalThis"`);
    /// that imported binding is an ordinary value, not the ambient global, so it
    /// must not be normalized or erased. A name explicitly recorded as a
    /// `const g = globalThis;` alias always counts.
    pub(super) fn is_ambient_global_alias(&self, name: &str) -> bool {
        if self.imports.is_global_object_alias(name) {
            return true;
        }
        if !ambient_globals::is_global_alias_name(name) {
            return false;
        }
        // Shadowed by a local binding/import/item -> not the ambient global.
        !(self.scope.is_bound(name)
            || self.imports.is_imported_binding(name)
            || self.items.contains_key(name)
            || self.module_globals.contains_key(name)
            || self.consts.has_literal(name)
            || self.consts.has_object(name))
    }

    /// Return whether an expression refers to the ambient global object.
    ///
    /// This is true for a non-shadowed base global alias and for a local
    /// identifier recorded as a global-object alias by `const g = globalThis;`.
    /// Any other expression — including a member access or computed access on the
    /// global object — is rejected, so callers never mistake a deeper path for the
    /// global object itself.
    pub(super) fn expr_is_global_alias(&self, expression: &Expression<'_>) -> bool {
        match expression {
            Expression::Identifier(identifier) => {
                self.is_ambient_global_alias(identifier.name.as_str())
            }
            _ => false,
        }
    }

    /// Return true when a `typeof <operand>` operand names an ambient global the
    /// default profile models as absent.
    ///
    /// Recognizes both the bare spelling (`typeof Buffer`) and the global-alias
    /// member spelling (`typeof globalThis.Buffer`, `typeof global.Buffer`),
    /// since es-toolkit reaches `Buffer` through `globalThis`. Only a static,
    /// non-optional member off a recognized global alias counts; any other shape
    /// falls through to ordinary lowering.
    pub(super) fn typeof_operand_is_absent_global(&self, operand: &Expression<'_>) -> bool {
        match operand {
            Expression::Identifier(identifier) => {
                Self::is_absent_ambient_global(identifier.name.as_str())
            }
            Expression::StaticMemberExpression(member) if !member.optional => {
                self.expr_is_global_alias(&member.object)
                    && Self::is_absent_ambient_global(member.property.name.as_str())
            }
            _ => false,
        }
    }

    /// Return true for ambient globals the default deterministic profile treats
    /// as absent (no runtime support, no modeled value).
    ///
    /// No ambient global is modeled as absent today: `Buffer` used to live here
    /// (Node-only, non-Node default profile) but is now a modeled host object
    /// with a concrete byte-buffer representation and a working `instanceof` /
    /// `Buffer.isBuffer` identity, so it is reported *present* through
    /// `is_known_defined_global_constructor` instead. The empty set is kept as a
    /// deliberate seam: a genuinely unsupported ambient global can be reinstated
    /// here to fold its `typeof` existence guards to the absent answer.
    pub(super) fn is_absent_ambient_global(name: &str) -> bool {
        // Derived from the profile registry rather than a hand-kept list here, so
        // "which globals are absent" cannot drift between the `"X" in globalThis`
        // answer and the `typeof X` answer. The global-object ALIASES are
        // excluded: `self` is absent as a *member* of the global object in this
        // profile but is still a usable alias spelling for the global object
        // itself, and `global_typeof_probe` answers those first.
        !ambient_globals::is_global_alias_name(name)
            && smelt_stdlib::global_member_presence(name) == smelt_stdlib::GlobalPresence::Absent
    }

    /// Constant truth value of a guard expression, computed WITHOUT lowering it.
    ///
    /// Recognizes `typeof <absent-global> ===/!== "undefined"`, the one guard
    /// shape whose fold is what makes the *other* operand of a `&&` unlowerable:
    /// the profile deliberately provides no `window` binding, so
    /// `typeof window !== 'undefined' && window?.document != null` can only be
    /// lowered if the dead right operand is never visited. Answering purely from
    /// the AST is what lets [`Self::logical_expression`] short-circuit before any
    /// of the value-shape helpers lower the right operand.
    pub(super) fn static_guard_value(&self, expression: &Expression<'_>) -> Option<bool> {
        let Expression::BinaryExpression(binary) = peel(expression, Wrappers::Parens)
        else {
            return None;
        };
        if !matches!(
            binary.operator,
            BinaryOperator::StrictEquality
                | BinaryOperator::StrictInequality
                | BinaryOperator::Equality
                | BinaryOperator::Inequality
        ) {
            return None;
        }
        let Expression::UnaryExpression(unary) = &binary.left else {
            return None;
        };
        if unary.operator != UnaryOperator::Typeof {
            return None;
        }
        let Expression::StringLiteral(kind_lit) = &binary.right else {
            return None;
        };
        if kind_lit.value != "undefined" || !self.typeof_operand_is_absent_global(&unary.argument) {
            return None;
        }
        // The operand is absent, so `typeof X === "undefined"` is `true` and
        // `!==` is `false`.
        Some(!matches!(
            binary.operator,
            BinaryOperator::StrictInequality | BinaryOperator::Inequality
        ))
    }

    /// Return a static array-identity answer when the operand's type settles it.
    ///
    /// `Array.isArray(x)` and `x instanceof Array` ask the SAME question, so both
    /// route through this one rule: fold only when the static type genuinely
    /// settles the answer, and otherwise emit the runtime probe
    /// (`UnknownIs { Array }`), which the emitter renders against a
    /// `SmeltUnknown` tag, a concrete `SmeltUnion` variant, or an `Option`
    /// payload as appropriate.
    ///
    /// - A list/tuple *is* an array — `Some(true)`.
    /// - A fully erased operand (`unknown`, an unconstrained type parameter)
    ///   carries the answer only at runtime — `None`.
    /// - A union settles the question only when every arm agrees; a union with an
    ///   array arm (e.g. `string | string[]`) is exactly the case where the test
    ///   is a real runtime question.
    /// - An optional adds `undefined`, which is never an array, so it can only
    ///   settle the question in the negative; `T[] | undefined` still needs the
    ///   probe. This is the case that folded `isArray(value?: any)` — the
    ///   `Option<SmeltUnknown>` parameter of es-toolkit's `isArray` — to a
    ///   constant `false`, killing the array branch of `toCamelCaseKeys` and
    ///   `toSnakeCaseKeys`.
    /// - Any other concrete type carries no array identity — `Some(false)`.
    pub(super) fn static_array_match(&self, ty: smelt_hir::TypeId) -> Option<bool> {
        let resolved_ty = self.type_param_constraint_or_self(ty);
        match self.ctx.krate.types.get(resolved_ty).cloned() {
            Some(Type::Unknown | Type::TypeParam { .. }) => None,
            Some(Type::List(_) | Type::Tuple(_)) => Some(true),
            Some(Type::Union(items)) => {
                let mut matches = items
                    .into_iter()
                    .map(|item| self.static_array_match(item))
                    .collect::<Option<Vec<_>>>()?
                    .into_iter();
                let first = matches.next()?;
                matches.all(|item| item == first).then_some(first)
            }
            Some(Type::Optional(inner)) => match self.static_array_match(inner) {
                Some(false) => Some(false),
                _ => None,
            },
            Some(_) => Some(false),
            None => None,
        }
    }

    /// Return a static `typeof` comparison result when all runtime variants agree.
    pub(super) fn static_typeof_match(&self, ty: smelt_hir::TypeId, expected: &str) -> Option<bool> {
        let resolved_ty = self.type_param_constraint_or_self(ty);
        match self.ctx.krate.types.get(resolved_ty).cloned() {
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Future(_)) => None,
            Some(Type::Class { name, .. })
                if self.ctx.krate.symbols.get(name) == Some("PropertyKey") =>
            {
                None
            }
            Some(Type::Union(items)) => {
                let mut matches = items
                    .into_iter()
                    .map(|item| self.static_typeof_match(item, expected))
                    .collect::<Option<Vec<_>>>()?
                    .into_iter();
                let first = matches.next()?;
                matches.all(|item| item == first).then_some(first)
            }
            Some(Type::Optional(inner)) => {
                let present = self.static_typeof_match(inner, expected)?;
                let absent = expected == "undefined";
                (present == absent).then_some(present)
            }
            Some(_) => Some(self.type_matches_typeof(resolved_ty, expected)),
            None => None,
        }
    }

    /// Return the JavaScript `typeof` string represented by a lowered type.
    ///
    /// Concrete types answer through the shared map, [`Type::runtime_kind`].
    /// `None` is `"undefined"`. An optional is not folded: absent it is
    /// `"undefined"`, present it is its value's. A future is not folded either
    /// (`static_typeof_match` declines it too). Both stay a runtime `typeof`.
    pub(super) fn typeof_type_name(&self, ty: smelt_hir::TypeId) -> Option<&'static str> {
        match self.ctx.krate.types.get(ty) {
            Some(Type::None) => Some("undefined"),
            Some(Type::Optional(_) | Type::Future(_)) | None => None,
            Some(concrete) => concrete.runtime_kind().map(UnknownKind::typeof_name),
        }
    }

    /// Lower `value === null` checks for TypeScript `unknown` values.
    pub(super) fn unknown_null_comparison(
        &mut self,
        binary: &oxc::ast::ast::BinaryExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !matches!(
            binary.operator,
            BinaryOperator::StrictEquality
                | BinaryOperator::StrictInequality
                | BinaryOperator::Equality
                | BinaryOperator::Inequality
        ) {
            return Ok(None);
        }
        let Some((value_expr, nullish_expr)) =
            Self::nullish_comparison_parts(&binary.left, &binary.right)
        else {
            return Ok(None);
        };
        let is_undefined_comparison = Self::is_undefined_expression(nullish_expr);
        let is_strict = matches!(
            binary.operator,
            BinaryOperator::StrictEquality | BinaryOperator::StrictInequality
        );
        let value = self.expression(value_expr, body)?;
        let Some(value_expression) = body.expr_at(value)
        else {
            return Ok(None);
        };
        let value = match &value_expression.kind {
            ExprKind::UnknownCast { value: erased, .. }
                if body.expr_at(*erased)
                    .is_some_and(|erased_expr| matches!(erased_expr.kind, ExprKind::Local(local)
                        if self.ctx.krate.types.get(Self::local_ty(body, local)) == Some(&Type::Unknown))) =>
            {
                *erased
            }
            _ => value,
        };
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        // An optional whose type admits only `null` is never `undefined`, so a
        // strict test against `undefined` is decided by the type, as it is in
        // JavaScript; the storage cannot tell the two apart
        // (`lowering::null_absent`).
        if is_strict
            && is_undefined_comparison
            && matches!(self.ctx.krate.types.get(Self::expr_ty(body, value)), Some(Type::Optional(_)))
            && self.checker_absent_is_null(value_expr.span().start, value_expr.span().end)
        {
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(
                    binary.operator == BinaryOperator::StrictInequality,
                )),
                ty: bool_ty,
                span: self.span(binary.span.start, binary.span.end),
            })));
        }
        if self.ctx.krate.types.get(Self::expr_ty(body, value)) != Some(&Type::Unknown) {
            let none = body.push_expr(Expr {
                kind: ExprKind::Literal(if is_undefined_comparison {
                    Literal::Undefined
                } else {
                    Literal::None
                }),
                ty: self.ctx.krate.types.intern(Type::None),
                span: self.span(binary.span.start, binary.span.end),
            });
            let op = match binary.operator {
                BinaryOperator::StrictEquality => BinOp::JsStrictEq,
                BinaryOperator::StrictInequality => BinOp::JsStrictNotEq,
                BinaryOperator::Equality => BinOp::Eq,
                BinaryOperator::Inequality => BinOp::NotEq,
                _ => {
                    return Err(SmeltError::unsupported(
                        self.span(binary.span.start, binary.span.end),
                        "nullish comparison operator is not supported",
                    ));
                }
            };
            return Ok(Some(self.comparison_expr(
                op,
                value,
                none,
                binary.span,
                body,
            )));
        }
        let check = if is_strict {
            body.push_expr(Expr {
                kind: ExprKind::UnknownIs {
                    value,
                    kind: if is_undefined_comparison {
                        UnknownKind::Undefined
                    } else {
                        UnknownKind::Null
                    },
                },
                ty: bool_ty,
                span: self.span(binary.span.start, binary.span.end),
            })
        } else {
            let null_check = body.push_expr(Expr {
                kind: ExprKind::UnknownIs {
                    value,
                    kind: UnknownKind::Null,
                },
                ty: bool_ty,
                span: self.span(binary.span.start, binary.span.end),
            });
            let undefined_check = body.push_expr(Expr {
                kind: ExprKind::UnknownIs {
                    value,
                    kind: UnknownKind::Undefined,
                },
                ty: bool_ty,
                span: self.span(binary.span.start, binary.span.end),
            });
            body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::Or,
                    lhs: null_check,
                    rhs: undefined_check,
                },
                ty: bool_ty,
                span: self.span(binary.span.start, binary.span.end),
            })
        };
        let negated = matches!(
            binary.operator,
            BinaryOperator::StrictInequality | BinaryOperator::Inequality
        );
        if negated {
            return Ok(Some(self.unary_bool_expr(
                UnaryOp::Not,
                check,
                binary.span,
                body,
            )));
        }
        Ok(Some(check))
    }

    /// Return the compared value and singleton for `value == null/undefined`.
    pub(super) fn nullish_comparison_parts<'a>(
        left: &'a Expression<'a>,
        right: &'a Expression<'a>,
    ) -> Option<(&'a Expression<'a>, &'a Expression<'a>)> {
        if Self::is_nullish_expression(left) {
            Some((right, left))
        } else if Self::is_nullish_expression(right) {
            Some((left, right))
        } else {
            None
        }
    }

    /// Return whether an expression is JavaScript `null` or `undefined`.
    pub(super) fn is_nullish_expression(expression: &Expression<'_>) -> bool {
        matches!(expression, Expression::NullLiteral(_))
            || Self::is_undefined_expression(expression)
    }

    /// Return whether an expression is the JavaScript `undefined` identifier.
    pub(super) fn is_undefined_expression(expression: &Expression<'_>) -> bool {
        matches!(expression, Expression::Identifier(identifier) if identifier.name == "undefined")
    }

    /// Lower TypeScript type assertions against `unknown` as checked extractions.
    pub(super) fn type_assertion_expression(
        &mut self,
        expression: &Expression<'_>,
        annotation: &TSType<'_>,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if self.fn_ctx.preserve_receiver
            && matches!(expression, Expression::ThisExpression(_))
        {
            return self.expression(expression, body);
        }
        if Self::is_const_type_assertion(annotation) {
            return self.expression(expression, body);
        }
        if self.is_bun_int32_annotation(annotation) {
            let value = self.expression(expression, body)?;
            return Ok(self.int32_assertion_expression(value, span, body));
        }
        let target = self.ts_type_to_hir(annotation)?;
        if self.concrete_type_requires_never_value(target) {
            // TypeScript assertions are erased at runtime. An assertion such
            // as `null as unknown as never` does not construct an impossible
            // value; it evaluates the original `null`. Keep the operand's real
            // shape instead of forcing an uninhabited Rust destination. Actual
            // declarations/containers whose storage type requires `never`
            // remain rejected by their declaration and literal checks.
            return self.expression(expression, body);
        }
        if let Some(parsed) = self.json_parse_call_with_target(expression, target, span, body)? {
            return Ok(parsed);
        }
        // A call asserted to `target` instantiates its callee's unbound type
        // parameters from it (see `bind_from_asserted_return`).
        let operand = peel(expression, Wrappers::Parens);
        let previous = std::mem::replace(
            &mut self.asserted_call_return,
            match operand {
                Expression::CallExpression(call) => Some((call.span, target)),
                _ => None,
            },
        );
        let value = self.expression_with_hint(expression, body, Some(target));
        self.asserted_call_return = previous;
        let value = value?;
        if Self::expr_ty(body, value) == target {
            return Ok(value);
        }
        if self.ctx.krate.types.get(Self::expr_ty(body, value)) == Some(&Type::Unknown)
            && target != Self::expr_ty(body, value)
        {
            return Ok(body.push_expr(Expr {
                kind: ExprKind::UnknownCast { value, target },
                ty: target,
                span: self.span(span.start, span.end),
            }));
        }
        // A TypeScript tuple assertion applied to a list-typed value
        // (`xs.filter(...) as [T]`) is type-level only: at runtime the value is
        // still a JS array. Smelt lowers tuples to Rust tuples and lists to the
        // identity-bearing `SmeltList`, which are incompatible representations,
        // so materializing the tuple would repackage the whole list into a
        // 1-tuple (`(SmeltUnknown,)`) that no longer satisfies a `SmeltList`
        // callee. Preserve the list value and its type; the tuple spelling is
        // erased.
        if matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, value)),
            Some(Type::List(_))
        ) && matches!(self.ctx.krate.types.get(target), Some(Type::Tuple(_)))
        {
            return Ok(value);
        }
        // The mirror: a tuple asserted to a list type (a tuple rest parameter
        // spread through `args as Parameters<F>`, see `lowering::tuple_rest`)
        // is still the same fixed-width array, and keeping the tuple keeps
        // every element's own type.
        if matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, value)),
            Some(Type::Tuple(_))
        ) && matches!(self.ctx.krate.types.get(target), Some(Type::List(_)))
        {
            return Ok(value);
        }
        // An assertion is erased at runtime, so it cannot make an ABSENT value
        // present: `(record[k] || record.fallback) as Entry` still evaluates to
        // `undefined` when neither key exists, and the source then tests it
        // (`if (entry)`). When the operand is statically `Optional<U>` and the
        // asserted type admits no nullish value, the result keeps the
        // optionality (`Optional<target>`). Dropping it would make the
        // follow-up truthiness test provably true and fold it away. Scoped to
        // OBJECT targets — the only ones whose truthiness folds — over a
        // payload that is not itself erased: `key as string` on an
        // `unknown | undefined` is the checked extraction of the erased value,
        // not a presence-preserving assertion.
        let target = if matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, value)),
            Some(Type::Optional(inner)) if !self.type_contains_unknown(*inner)
        ) && !self.is_nullishable_type(target)
            && self.type_is_always_truthy_object_surface(target)
        {
            self.ctx.krate.types.intern(Type::Optional(target))
        } else {
            target
        };
        if Self::expr_ty(body, value) == target {
            return Ok(value);
        }
        Ok(body.push_expr(Expr {
            kind: ExprKind::TypeAssert { value },
            ty: target,
            span: self.span(span.start, span.end),
        }))
    }

    /// Return whether a TypeScript assertion is the runtime-erased `as const` form.
    pub(super) fn is_const_type_assertion(annotation: &TSType<'_>) -> bool {
        matches!(
            annotation,
            TSType::TSTypeReference(reference)
                if matches!(
                    &reference.type_name,
                    TSTypeName::IdentifierReference(name) if name.name == "const"
                )
        )
    }
}
