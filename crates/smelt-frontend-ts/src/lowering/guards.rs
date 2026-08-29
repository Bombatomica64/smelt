//! Lowering helpers for `instanceof`, `typeof`, nullish comparisons, type
//! assertions, call-argument lowering, and `Promise` construction.

use super::{ModuleBuilder, ambient_globals, stdlib_dispatch, unknown_kind_from_typeof};
use crate::SmeltError;
use oxc::ast::ast::{Argument, BindingPattern, Expression, Statement, TSType, TSTypeName};
use oxc::span::GetSpan;
use oxc::syntax::operator::{BinaryOperator, UnaryOperator};
use smelt_hir::{
    AsyncOp, BinOp, Body, DatePart, Expr, ExprKind, FunctionType, Literal, PrimitiveCastOp, Stmt,
    Type, UnaryOp, UnknownKind,
};
use smelt_stdlib::RuleId;
use crate::lowering::support::arrow_block_statements;

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
        let value = self.expression(&binary.left, body)?;
        let value_ty = Self::expr_ty(body, value);
        if let Expression::StaticMemberExpression(member) = &binary.right
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
                span: self.span(binary.span.start, binary.span.end),
            }));
        }
        let Expression::Identifier(class_ident) = &binary.right else {
            return Err(SmeltError::unsupported(
                self.span(binary.right.span().start, binary.right.span().end),
                "TypeScript instanceof requires a direct class constructor on the right side",
            ));
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
                    span: self.span(binary.span.start, binary.span.end),
                }));
            };
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(result)),
                ty,
                span: self.span(binary.span.start, binary.span.end),
            }));
        }
        if Self::is_ts_stdlib_class_name(class_text, smelt_stdlib::StdlibClass::Date)
            && self.expression_is_known_date_value(value, body)
        {
            let ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(true)),
                ty,
                span: self.span(binary.span.start, binary.span.end),
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
                span: self.span(binary.span.start, binary.span.end),
            }));
        }
        let builtin_target = Self::instanceof_builtin_target(class_text);
        if !self.instanceof_supported_left_operand(value_ty) {
            return Err(SmeltError::unsupported(
                self.span(binary.left.span().start, binary.left.span().end),
                "TypeScript instanceof requires a concrete class-typed left operand",
            ));
        }
        if !builtin_target
            && !self.classes.contains(class_text)
            // Module class names are collected before any class body is
            // lowered. A class currently under construction is therefore a
            // valid nominal target even though its final HIR item is inserted
            // only after all of its methods have been emitted.
            && !self.classes.is_pending(class_text)
            && !self.imports.is_value(class_text)
        {
            // `this instanceof bound` against a *function* value (the JS
            // constructor-function idiom for detecting `new`-invocation, as in
            // lodash-compat `bind`/`curry`). Smelt's runtime never constructs
            // closure values with `new`, so no value can be an instance of a
            // plain function: the check is truthfully `false`.
            // Classes are not first-class values in Smelt, so a target that
            // resolves to a local binding or function item can only be a
            // function value, never a constructible class.
            let target_is_function_value = self.scope.is_bound(class_text)
                || self.scope.has_callback(class_text)
                || self
                    .items
                    .get(class_text)
                    .is_some_and(|&item| matches!(self.item_ref(item), smelt_hir::Item::Function(_)));
            if target_is_function_value {
                let ty = self.ctx.krate.types.intern(Type::Bool);
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Bool(false)),
                    ty,
                    span: self.span(binary.span.start, binary.span.end),
                }));
            }
            return Err(SmeltError::for_unresolved_name(
                self.span(class_ident.span.start, class_ident.span.end),
                class_text,
                format!("TypeScript instanceof target `{class_text}` is not a lowered class"),
            ));
        }
        let class = self.intern_type_name(class_text);
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(body.push_expr(Expr {
            kind: ExprKind::InstanceOf { value, class },
            ty,
            span: self.span(binary.span.start, binary.span.end),
        }))
    }

    /// Return true when an expression is a built-in constructor target.
    pub(super) fn instanceof_builtin_target(target: &str) -> bool {
        smelt_stdlib::typescript_stdlib_class(target).is_some()
            // Typed-array views (`x instanceof Uint8Array`) are byte-backed host
            // objects, so they are already covered by the `byte_buffer_role`
            // clause below; naming them here too keeps the recognizer readable.
            || smelt_stdlib::is_typed_array_class_name(target)
            || Self::marker_only_builtin_marker(target).is_some()
            // The byte-backed host objects (`ArrayBuffer`, `SharedArrayBuffer`,
            // `Buffer`, `DataView`) all carry a registry marker their `instanceof`
            // resolves through.
            || smelt_stdlib::byte_buffer_role(target).is_some()
            || matches!(
                target,
                "Promise"
                    | "Blob"
                    // `File` records stamp `__smelt_file` on top of
                    // `__smelt_blob` (see `file_constructor_expression`), so
                    // both `value instanceof File` and `value instanceof Blob`
                    // resolve through their markers in `instance_of_text`.
                    | "File"
                    | "Number"
                    // Boxed primitive wrappers. A real primitive (`true`, `"a"`)
                    // is never `instanceof` its wrapper; only the boxed object
                    // form is, recognized through its `__smelt_boolean` /
                    // `__smelt_string` marker (see `instance_of_text`). Listing
                    // them makes `value instanceof Boolean` / `instanceof String`
                    // lower to a marker check instead of aborting as an unmodeled
                    // class — the correct `false` for the primitives that
                    // es-toolkit's `isBoolean`/`isString` test against.
                    | "Boolean"
                    | "String"
                    // `Symbol` likewise: a primitive symbol erases to
                    // `SmeltUnknown::Symbol` and is not `instanceof Symbol`; only a
                    // boxed `Object(symbol)` wrapper carrying `__smelt_symbol` is.
                    | "Symbol"
                    | "AbortController"
                    | "AbortSignal"
                    | "Error"
                    | "EvalError"
                    | "RangeError"
                    | "ReferenceError"
                    | "SyntaxError"
                    | "TypeError"
                    | "URIError"
                    | "AggregateError"
                    // Host `DOMException`: es-toolkit's `AbortError`/`TimeoutError`
                    // tests probe `value instanceof DOMException`. A `new
                    // DOMException(...)` erases to a record carrying
                    // `__smelt_domexception` (see
                    // `domexception_object_constructor_expression`), recognized
                    // through that marker in `instance_of_text`.
                    | "DOMException"
            )
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

    /// Return true for builtin targets represented by non-class HIR values today.
    ///
    /// `Map` and `Set` are intentionally absent: each source value erases to a
    /// marker object (`{ __smelt_map: [...] }` / `{ __smelt_set: [...] }`), so
    /// `value instanceof Map`/`instanceof Set` resolves through that marker in
    /// `instance_of_text` rather than folding to a constant `false`. `Date`/`RegExp`
    /// are timestamp/marker values whose `instanceof` remains a deliberate `false`
    /// fold for erased operands.
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

    /// Return true when a timestamp-backed expression is statically known to be a JavaScript Date.
    ///
    /// Date values use numeric timestamps in generated Rust, so runtime Rust
    /// type inspection cannot distinguish them from arbitrary source numbers.
    /// TypeScript still guarantees `Date` and `T extends Date` values satisfy
    /// `instanceof Date`, and direct Date constructors retain that provenance
    /// until this predicate is lowered.
    pub(super) fn type_is_known_date_value(&self, ty: smelt_hir::TypeId) -> bool {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
        {
            Some(Type::Class { name, .. }) => {
                self.ctx.krate.symbols.get(*name) == Some("Date")
            }
            Some(Type::Optional(inner)) => self.type_is_known_date_value(*inner),
            Some(Type::Union(items)) => {
                let values = items
                    .iter()
                    .copied()
                    .filter(|item| self.ctx.krate.types.get(*item) != Some(&Type::None))
                    .collect::<Vec<_>>();
                !values.is_empty()
                    && values
                        .into_iter()
                        .all(|item| self.type_is_known_date_value(item))
            }
            _ => false,
        }
    }

    /// Return true when an expression carries JavaScript `Date` identity despite timestamp storage.
    pub(super) fn expression_is_known_date_value(&self, value: smelt_hir::ExprId, body: &Body) -> bool {
        let Some(expr) = body
            .exprs
            .get(usize::try_from(value.0).unwrap_or(usize::MAX))
        else {
            return false;
        };
        if self.type_is_known_date_value(expr.ty) {
            return true;
        }
        match &expr.kind {
            ExprKind::DateFromParts { .. } | ExprKind::DateFromValue { .. } => true,
            ExprKind::Local(local) => self.scope.is_date_value(*local),
            ExprKind::TypeAssert { value: asserted_value } => {
                self.expression_is_known_date_value(*asserted_value, body)
            }
            ExprKind::Call { callee, .. } => body
                .exprs
                .get(usize::try_from(callee.0).unwrap_or(usize::MAX))
                .and_then(|callee| match callee.kind {
                    ExprKind::Item(item) => Some(item),
                    _ => None,
                })
                .is_some_and(|item| self.ctx.date_returning_functions.contains(&item)),
            _ => false,
        }
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
        if kind_lit.value.as_str() == "undefined"
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
        if kind_lit.value.as_str() == "undefined"
            && self.typeof_operand_is_absent_global(&unary.argument)
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
        // A modeled host constructor the crate *reassigns* somewhere
        // (`globalThis.Blob = undefined`) can no longer fold its presence guard
        // to a constant: `typeof Blob === 'undefined'` becomes a dynamic slot
        // probe so `isBlob`/`isFile` observe the override at runtime. Handles the
        // bare identifier spelling (`typeof Blob`) and the global-alias member
        // spelling (`typeof globalThis.Blob`).
        if kind_lit.value.as_str() == "undefined"
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
        if kind_lit.value.as_str() == "undefined"
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
        if kind_lit.value.as_str() == "undefined" {
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
                if matches!(
                    binary.operator,
                    BinaryOperator::StrictInequality | BinaryOperator::Inequality
                ) {
                    return Ok(Some(self.unary_bool_expr(
                        UnaryOp::Not,
                        check,
                        binary.span,
                        body,
                    )));
                }
                return Ok(Some(check));
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
        let Some(kind) = unknown_kind_from_typeof(kind_lit.value.as_str()) else {
            return Err(SmeltError::unsupported(
                self.span(kind_lit.span.start, kind_lit.span.end),
                format!(
                    "typeof narrowing kind `{}` is not supported yet",
                    kind_lit.value
                ),
            ));
        };
        let expected = kind_lit.value.as_str();
        let value = self.expression(&unary.argument, body)?;
        let value_ty = Self::expr_ty(body, value);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        if let Some(Type::Optional(inner)) = self.ctx.krate.types.get(value_ty).cloned()
            && self.static_typeof_match(inner, expected) == Some(true)
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
        if let Some(matches_kind) = self.static_typeof_match(value_ty, expected) {
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
        if matches!(
            binary.operator,
            BinaryOperator::StrictInequality | BinaryOperator::Inequality
        ) {
            return Ok(Some(self.unary_bool_expr(
                UnaryOp::Not,
                check,
                binary.span,
                body,
            )));
        }
        Ok(Some(check))
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
            kind_lit.value.as_str(),
            is_equality,
        )?;
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(value)),
            ty: bool_ty,
            span: self.span(binary.span.start, binary.span.end),
        }))
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
        // No absent ambient globals currently; discard the operand explicitly.
        let _ = name;
        false
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
    pub(super) fn typeof_type_name(&self, ty: smelt_hir::TypeId) -> Option<&'static str> {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Bool) => Some("boolean"),
            Some(Type::Int | Type::Float) => Some("number"),
            Some(Type::String) => Some("string"),
            Some(Type::Function(_)) => Some("function"),
            Some(Type::None) => Some("undefined"),
            Some(
                Type::List(_)
                | Type::Set(_)
                | Type::Dict(_, _)
                | Type::JsMap(_, _)
                | Type::Tuple(_)
                | Type::Class { .. }
                | Type::Optional(_)
                | Type::Generator { .. }
                | Type::GeneratorResult { .. },
            ) => Some("object"),
            Some(
                Type::Unknown
                | Type::Never
                | Type::Union(_)
                | Type::Future(_)
                | Type::TypeParam { .. },
            )
            | None => None,
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
        let Some(value_expression) = body
            .exprs
            .get(usize::try_from(value.0).unwrap_or(usize::MAX))
        else {
            return Ok(None);
        };
        let value = match &value_expression.kind {
            ExprKind::UnknownCast { value: erased, .. }
                if body
                    .exprs
                    .get(usize::try_from(erased.0).unwrap_or(usize::MAX))
                    .is_some_and(|erased_expr| matches!(erased_expr.kind, ExprKind::Local(local)
                        if self.ctx.krate.types.get(Self::local_ty(body, local)) == Some(&Type::Unknown))) =>
            {
                *erased
            }
            _ => value,
        };
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
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
        if self.preserve_specialization_receiver
            && matches!(expression, Expression::ThisExpression(_))
        {
            return self.expression(expression, body);
        }
        if Self::is_const_type_assertion(annotation) {
            return self.expression(expression, body);
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
        let value = self.expression_with_hint(expression, body, Some(target))?;
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

    /// Lower a function call argument.
    pub(super) fn argument(
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
                    kind: ExprKind::Literal(Literal::String(lit.value.to_string())),
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
            Argument::AwaitExpression(await_expr) => {
                if !self.current_async {
                    return Err(SmeltError::unsupported(
                        self.span(await_expr.span.start, await_expr.span.end),
                        "await expressions are only lowered inside async functions",
                    ));
                }
                let awaited = self.expression(&await_expr.argument, body)?;
                let awaited_ty = Self::expr_ty(body, awaited);
                let Some(ty) = self.future_inner_type(awaited_ty) else {
                    let ty = self.ctx.krate.types.intern(Type::Unknown);
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::None),
                        ty,
                        span: self.span(await_expr.span.start, await_expr.span.end),
                    }));
                };
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Await(awaited),
                    ty,
                    span: self.span(await_expr.span.start, await_expr.span.end),
                }))
            }
            Argument::ArrowFunctionExpression(arrow) => self.arrow_function_expression(arrow, body),
            Argument::FunctionExpression(function) => {
                if function.r#async {
                    let ty = self.ctx.krate.types.intern(Type::Unknown);
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
            Argument::SpreadElement(spread) => self.expression(&spread.argument, body),
            _ => Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                format!("call argument kind is not lowered yet: {argument:?}"),
            )),
        }
    }

    /// Lower a call argument with an expected type for literals that need contextual typing.
    pub(super) fn argument_with_hint(
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
            _ => self.argument(argument, body),
        }
    }

    /// Return whether a contextual argument type should preserve a `RegExp` literal object.
    pub(super) fn type_accepts_regexp_literal(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(self.type_param_constraint_or_self(ty)) {
            Some(Type::Class { name, .. }) => self
                .ctx
                .krate
                .symbols
                .get(*name)
                .is_some_and(|name| name == "RegExp"),
            Some(Type::Optional(item)) => self.type_accepts_regexp_literal(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.type_accepts_regexp_literal(item)),
            _ => false,
        }
    }

    /// Lower a `RegExp` literal to a runtime `RegExp` value for erased object contexts.
    pub(super) fn regexp_literal_expression(
        &mut self,
        literal: &oxc::ast::ast::RegExpLiteral<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let pattern = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(
                Self::regex_literal_pattern_text_without_flags(literal),
            )),
            ty: string_ty,
            span: self.span(literal.span.start, literal.span.end),
        });
        let flags = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(literal.regex.flags.to_string())),
            ty: string_ty,
            span: self.span(literal.span.start, literal.span.end),
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::New {
                class: self.intern_type_name("RegExp"),
                args: vec![pattern, flags],
            },
            ty: self.regexp_type(),
            span: self.span(literal.span.start, literal.span.end),
        }))
    }

    /// Lower supported `Promise.*` calls into shared async runtime operations.
    pub(super) fn promise_static_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Promise" {
            return Ok(None);
        }
        if member.property.name == "resolve" {
            if call.arguments.len() > 1 {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Promise.resolve supports at most one value argument",
                ));
            }
            // The resolution value has to travel in the op's operands. Lowering
            // to a bare `Sleep` kept only its *type*, so the emitted future
            // resolved with `default_value(inner_ty)` -- `Promise.resolve(1)`
            // settled as `0`, `Promise.resolve('hello')` as `""`. `Resolve`
            // carries the operand itself; the duration operand preserves the
            // microtask deferral `Sleep` was standing in for.
            let (inner_ty, value) = if let Some(argument) = call.arguments.first() {
                let value = self.argument(argument, body)?;
                (Self::expr_ty(body, value), Some(value))
            } else {
                (self.ctx.krate.types.intern(Type::None), None)
            };
            let duration = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(0.0)),
                ty: self.ctx.krate.types.intern(Type::Float),
                span: self.span(call.span.start, call.span.start),
            });
            let mut args = vec![duration];
            args.extend(value);
            let ty = self.ctx.krate.types.intern(Type::Future(inner_ty));
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::AsyncOp {
                    op: AsyncOp::Resolve,
                    args,
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let op = match member.property.name.as_str() {
            "all" => AsyncOp::All,
            "race" => AsyncOp::Race,
            "allSettled" => AsyncOp::AllSettled,
            other => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    format!("Promise.{other} is not lowered yet"),
                ));
            }
        };
        if call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Promise combinators require exactly one array argument",
            ));
        }
        let Some(first_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Promise combinators require exactly one array argument",
            ));
        };
        let (args, output_ty) = if let Argument::ArrayExpression(array) = first_argument {
            let args = self.promise_array_args(array, body)?;
            let output_ty = self.promise_literal_combinator_output(op, &args, array.span, body)?;
            (args, output_ty)
        } else {
            self.promise_list_combinator_args(op, first_argument, body)?
        };
        let ty = self.ctx.krate.types.intern(Type::Future(output_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::AsyncOp { op, args },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return output type for Promise combinators over a source array literal.
    pub(super) fn promise_literal_combinator_output(
        &mut self,
        op: AsyncOp,
        args: &[smelt_hir::ExprId],
        span: oxc::span::Span,
        body: &Body,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        match op {
            AsyncOp::All | AsyncOp::AllSettled => {
                let outputs = args
                    .iter()
                    .map(|arg| {
                        self.future_inner_type(Self::expr_ty(body, *arg))
                            .ok_or_else(|| {
                                SmeltError::unsupported(
                                    self.span(span.start, span.end),
                                    "Promise combinator entries must be Promise<T> values",
                                )
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(self.ctx.krate.types.intern(Type::Tuple(outputs)))
            }
            AsyncOp::Race => {
                let Some(first) = args.first() else {
                    return Err(SmeltError::unsupported(
                        self.span(span.start, span.end),
                        "Promise.race requires at least one promise",
                    ));
                };
                self.future_inner_type(Self::expr_ty(body, *first))
                    .ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(span.start, span.end),
                            "Promise.race entries must be Promise<T> values",
                        )
                    })
            }
            AsyncOp::Sleep
            | AsyncOp::Resolve
            | AsyncOp::CreateTask
            | AsyncOp::WaitFor
            | AsyncOp::HttpGetText
            | AsyncOp::SetTimeout
            | AsyncOp::ClearTimeout
            | AsyncOp::SetInterval
            | AsyncOp::ClearInterval
            | AsyncOp::Promise
            | AsyncOp::Then
            | AsyncOp::Catch
            | AsyncOp::SpawnLocal => Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                format!("Promise.{op:?} is not lowered yet"),
            )),
        }
    }

    /// Lower Promise combinators over a non-literal list of homogeneous futures.
    pub(super) fn promise_list_combinator_args(
        &mut self,
        op: AsyncOp,
        argument: &Argument<'_>,
        body: &mut Body,
    ) -> Result<(Vec<smelt_hir::ExprId>, smelt_hir::TypeId), SmeltError> {
        if op == AsyncOp::Race {
            return Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                "Promise.race over a non-literal array is not lowered yet",
            ));
        }
        let list = self.argument(argument, body)?;
        let list_ty = self.type_param_constraint_or_self(Self::expr_ty(body, list));
        let Some(Type::List(item_ty)) = self.ctx.krate.types.get(list_ty).cloned() else {
            return Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                "Promise combinators require an array of Promise<T> values",
            ));
        };
        let (list, output_item_ty) = if let Some(output_item_ty) = self.future_inner_type(item_ty) {
            (list, output_item_ty)
        } else {
            let future_item_ty = self.ctx.krate.types.intern(Type::Future(item_ty));
            let future_list_ty = self.ctx.krate.types.intern(Type::List(future_item_ty));
            let list = body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: list,
                    target: future_list_ty,
                },
                ty: future_list_ty,
                span: self.span(argument.span().start, argument.span().end),
            });
            (list, item_ty)
        };
        let output_ty = self.ctx.krate.types.intern(Type::List(output_item_ty));
        Ok((vec![list], output_ty))
    }

    /// Lower supported `new Promise<T>(executor)` expressions to future values.
    ///
    /// Timer executors keep their timeout duration. Other executor forms are
    /// represented as zero-delay futures with the explicit `Promise<T>` output
    /// type so async batching helpers can keep their type surface.
    pub(super) fn promise_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &new_expr.callee else {
            return Ok(None);
        };
        if callee.name != "Promise" {
            return Ok(None);
        }
        let [executor_arg] = new_expr.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "Promise constructor lowering requires one executor",
            ));
        };
        let bare_delay_timer = match executor_arg {
            Argument::ArrowFunctionExpression(executor) => {
                Self::promise_executor_timer_call(executor).filter(|timer_call| {
                    Self::promise_executor_is_bare_delay(executor, timer_call)
                })
            }
            _ => None,
        };
        let mut lowered_executor = None;
        let contextual_output = type_hint
            .map(|hint| {
                // A Promise constructor can be contextualized either by a
                // `Promise<T>` value slot or by the resolved `T` hint supplied
                // for a return expression inside an async function. Both
                // contexts describe the constructor's concrete output `T`.
                self.future_inner_type(hint).unwrap_or(hint)
            })
            .or_else(|| {
                self.promise_constructor_output_type(new_expr)
                    .ok()
                    .flatten()
            });
        let inferred_output = if contextual_output.is_none()
            && bare_delay_timer.is_none()
            && matches!(executor_arg, Argument::Identifier(_))
        {
            let executor = self.argument(executor_arg, body)?;
            let executor_ty = Self::expr_ty(body, executor);
            let inferred = self.promise_executor_resolved_value_type(executor_ty);
            if inferred.is_some() {
                lowered_executor = Some(executor);
            }
            inferred
        } else {
            None
        };
        let output_ty = contextual_output
            .or(inferred_output)
            .unwrap_or_else(|| self.ctx.krate.types.intern(Type::None));
        let ty = self.ctx.krate.types.intern(Type::Future(output_ty));
        // Only collapse a Promise to a bare `Sleep` when its executor is the pure
        // delay shape `new Promise(resolve => setTimeout(resolve, ms))` — the
        // timer's callback IS the `resolve` parameter, so the promise resolves
        // `undefined` after the delay and carries no value. Any executor whose
        // timer callback does real work (e.g. `() => resolve(value)`) must flow
        // through `AsyncOp::Promise`, which threads `resolve`/`reject`; treating
        // it as `Sleep` would silently discard the resolved value.
        let duration = if let Some(timer_call) = bare_delay_timer {
            let Some(duration_argument) = timer_call.arguments.get(1) else {
                return Err(SmeltError::unsupported(
                    self.span(timer_call.span.start, timer_call.span.end),
                    "Promise timer executor must pass a duration argument",
                ));
            };
            self.argument(duration_argument, body)?
        } else {
            let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
            let none_ty = self.ctx.krate.types.intern(Type::None);
            let resolve_value_ty = if self.ctx.krate.types.get(output_ty) == Some(&Type::None) {
                unknown_ty
            } else {
                output_ty
            };
            // `resolve`/`reject` accept their value argument optionally: TypeScript
            // types them `(value?: T) => void`, and `resolve()` with no argument is
            // valid (it settles with `undefined`). Recording `required_params: 0`
            // lets the callbacks satisfy shorter expected function slots such as the
            // `Array<() => void>` deferred-task queue used by promise concurrency
            // primitives (semaphore/mutex).
            let resolve_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
                params: vec![resolve_value_ty],
                rest: None,
                required_params: Some(0),
                mutable_params: Vec::new(),
                return_ty: none_ty,
                is_async: false,
                may_throw: false,
            }));
            let reject_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
                params: vec![unknown_ty],
                rest: None,
                required_params: Some(0),
                mutable_params: Vec::new(),
                return_ty: none_ty,
                is_async: false,
                may_throw: false,
            }));
            let executor_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
                params: vec![resolve_ty, reject_ty],
                rest: None,
                required_params: Some(2),
                mutable_params: Vec::new(),
                return_ty: none_ty,
                is_async: false,
                may_throw: false,
            }));
            let executor_expr = match lowered_executor {
                Some(executor) => executor,
                None => self.argument_with_hint(executor_arg, body, Some(executor_ty))?,
            };
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::AsyncOp {
                    op: AsyncOp::Promise,
                    args: vec![executor_expr],
                },
                ty,
                span: self.span(new_expr.span.start, new_expr.span.end),
            })));
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::AsyncOp {
                op: AsyncOp::Sleep,
                args: vec![duration],
            },
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        })))
    }

    /// Infer `Promise<T>`'s resolved `T` from a named executor's first callback.
    ///
    /// A declaration such as `(resolve: (value: T) => void)` already carries
    /// the useful static shape, so recovering it avoids an erased resolver ABI.
    pub(super) fn promise_executor_resolved_value_type(
        &self,
        executor_ty: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        let Type::Function(executor) = self.ctx.krate.types.get(executor_ty)? else {
            return None;
        };
        let resolve_ty = *executor.params.first()?;
        let Type::Function(resolve) = self.ctx.krate.types.get(resolve_ty)? else {
            return None;
        };
        resolve.params.first().copied()
    }

    /// Return the explicit `Promise<T>` constructor output type when present.
    pub(super) fn promise_constructor_output_type(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
    ) -> Result<Option<smelt_hir::TypeId>, SmeltError> {
        let Some(type_arguments) = &new_expr.type_arguments else {
            return Ok(None);
        };
        let [item] = type_arguments.params.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "Promise construction supports exactly one type argument",
            ));
        };
        self.ts_type_to_hir(item).map(Some)
    }

    /// Return the `setTimeout` call inside a supported Promise executor.
    pub(super) fn promise_executor_timer_call<'a>(
        executor: &'a oxc::ast::ast::ArrowFunctionExpression<'a>,
    ) -> Option<&'a oxc::ast::ast::CallExpression<'a>> {
        // The executor is written either concisely (`resolve => setTimeout(..)`)
        // or as a block with one expression statement. Since oxc 0.147 the
        // concise form carries the expression directly instead of wrapping it in
        // an `ExpressionStatement`, so both shapes are read here.
        let body_expression = if let Some(expression) = executor.get_expression() {
            expression
        } else {
            let [Statement::ExpressionStatement(expr_stmt)] = arrow_block_statements(executor)
            else {
                return None;
            };
            &expr_stmt.expression
        };
        let Expression::CallExpression(call) = body_expression else {
            return None;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return None;
        };
        (callee.name == "setTimeout" && call.arguments.len() == 2).then_some(call)
    }

    /// Return whether a `setTimeout`-bodied Promise executor is a pure delay.
    ///
    /// The pure-delay shape is `(resolve) => setTimeout(resolve, ms)`: the timer
    /// callback is exactly the executor's first (`resolve`) parameter, so the
    /// promise resolves `undefined` after `ms` and carries no value — it is sound
    /// to lower the whole construct to `AsyncOp::Sleep`. When the callback is
    /// anything else (`() => resolve(value)`, a block that rejects, etc.) the
    /// resolved value would be lost by the `Sleep` collapse, so the caller must
    /// route it through `AsyncOp::Promise` instead.
    pub(super) fn promise_executor_is_bare_delay(
        executor: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        timer_call: &oxc::ast::ast::CallExpression<'_>,
    ) -> bool {
        let Some(resolve_param) = executor.params.items.first() else {
            return false;
        };
        let BindingPattern::BindingIdentifier(resolve_binding) = &resolve_param.pattern else {
            return false;
        };
        let Some(Argument::Identifier(callback)) = timer_call.arguments.first() else {
            return false;
        };
        callback.name == resolve_binding.name
    }

    /// Lower small TypeScript timer shims used by async fixtures.
    pub(super) fn timer_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        match callee.name.as_str() {
            "setTimeout" if call.arguments.len() == 1 => {
                let Some(duration_argument) = call.arguments.first() else {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "setTimeout lowering supports the Smelt timer shim shape setTimeout(milliseconds)",
                    ));
                };
                let duration = self.argument(duration_argument, body)?;
                let none_ty = self.ctx.krate.types.intern(Type::None);
                let ty = self.ctx.krate.types.intern(Type::Future(none_ty));
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::AsyncOp {
                        op: AsyncOp::Sleep,
                        args: vec![duration],
                    },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            "setTimeout" if call.arguments.len() == 2 => {
                let Some(callback) = call.arguments.first() else {
                    return Ok(None);
                };
                let Some(duration) = call.arguments.get(1) else {
                    return Ok(None);
                };
                let callback = self.argument(callback, body)?;
                let duration = self.argument(duration, body)?;
                let ty = self.ctx.krate.types.intern(Type::Unknown);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::AsyncOp {
                        op: AsyncOp::SetTimeout,
                        args: vec![callback, duration],
                    },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            "clearTimeout" if call.arguments.len() == 1 => {
                let Some(timeout) = call.arguments.first() else {
                    return Ok(None);
                };
                let timeout = self.argument(timeout, body)?;
                let ty = self.ctx.krate.types.intern(Type::None);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::AsyncOp {
                        op: AsyncOp::ClearTimeout,
                        args: vec![timeout],
                    },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            // `setInterval(callback, period)` registers a repeating timer that
            // re-arms itself after every fire, mirroring the two-argument
            // `setTimeout` shape. The shared virtual-time timer queue drives it,
            // so the only difference at codegen time is the re-arm; see
            // `AsyncOp::SetInterval` in the call emitter.
            "setInterval" if call.arguments.len() == 2 => {
                let Some(callback) = call.arguments.first() else {
                    return Ok(None);
                };
                let Some(duration) = call.arguments.get(1) else {
                    return Ok(None);
                };
                let callback = self.argument(callback, body)?;
                let duration = self.argument(duration, body)?;
                let ty = self.ctx.krate.types.intern(Type::Unknown);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::AsyncOp {
                        op: AsyncOp::SetInterval,
                        args: vec![callback, duration],
                    },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            // `clearInterval(id)` cancels a repeating timer by handle. Intervals
            // share the timer queue with timeouts, so this is the same
            // cancel-by-id as `clearTimeout`.
            "clearInterval" if call.arguments.len() == 1 => {
                let Some(timer) = call.arguments.first() else {
                    return Ok(None);
                };
                let timer = self.argument(timer, body)?;
                let ty = self.ctx.krate.types.intern(Type::None);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::AsyncOp {
                        op: AsyncOp::ClearInterval,
                        args: vec![timer],
                    },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            // `setTimeout(callback, ms, ...args)` / `setInterval(callback, ms,
            // ...args)` forward the extra arguments to the callback when the
            // timer fires.
            //
            // When the callback is a statically typed function and every extra is
            // a concretely typed positional argument (no source spread), the
            // extras are captured by a synthesized zero-argument wrapper closure
            // that calls the callback directly — no erased `Vec<SmeltUnknown>` is
            // produced. A source spread or an untyped extra is a genuine dynamic
            // boundary, so those keep the erased-list path where the extras pack
            // into one operand dispatched through the dynamic callback ABI.
            "setTimeout" | "setInterval" if call.arguments.len() >= 3 => {
                let (Some(callback_arg), Some(duration_arg)) =
                    (call.arguments.first(), call.arguments.get(1))
                else {
                    return Ok(None);
                };
                let callback = self.argument(callback_arg, body)?;
                let duration = self.argument(duration_arg, body)?;
                let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
                let span = self.span(call.span.start, call.span.end);
                let op = if callee.name == "setTimeout" {
                    AsyncOp::SetTimeout
                } else {
                    AsyncOp::SetInterval
                };

                let extra_args = call.arguments.get(2..).unwrap_or_default();
                let has_spread = extra_args
                    .iter()
                    .any(|arg| matches!(arg, Argument::SpreadElement(_)));
                if !has_spread {
                    // Lower each extra exactly once. Either the typed wrapper
                    // consumes them, or they pack into the erased list below —
                    // never both, so source side effects run once.
                    let extras = extra_args
                        .iter()
                        .map(|arg| self.argument(arg, body))
                        .collect::<Result<Vec<_>, _>>()?;
                    if let Some(wrapper) =
                        self.timer_typed_wrapper_closure(callback, &extras, span, body)
                    {
                        return Ok(Some(body.push_expr(Expr {
                            kind: ExprKind::AsyncOp {
                                op,
                                args: vec![wrapper, duration],
                            },
                            ty: unknown_ty,
                            span,
                        })));
                    }
                    let list_ty = self.ctx.krate.types.intern(Type::List(unknown_ty));
                    let extra = body.push_expr(Expr {
                        kind: ExprKind::ListLit(extras),
                        ty: list_ty,
                        span,
                    });
                    return Ok(Some(body.push_expr(Expr {
                        kind: ExprKind::AsyncOp {
                            op,
                            args: vec![callback, duration, extra],
                        },
                        ty: unknown_ty,
                        span,
                    })));
                }

                let extra = self.packed_spread_arguments(
                    unknown_ty,
                    extra_args,
                    span,
                    body,
                )?;
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::AsyncOp {
                        op,
                        args: vec![callback, duration, extra],
                    },
                    ty: unknown_ty,
                    span,
                })))
            }
            "setTimeout" | "clearTimeout" | "setInterval" | "clearInterval" => {
                Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "timer lowering supports setTimeout(milliseconds), setTimeout(callback, milliseconds), clearTimeout(id), setInterval(callback, milliseconds), and clearInterval(id)",
                ))
            }
            _ => Ok(None),
        }
    }

    /// Return targeted diagnostics for deferred object and collection APIs.
    ///
    /// `replaceAll` is handled by `regex_replace_call` (regex pattern) and
    /// `string_replace_call` (literal string pattern), so it is no longer
    /// rejected here. The hook is kept as the place to surface future
    /// deferred object/collection method diagnostics.
    pub(super) fn unsupported_object_collection_call(
        call: &oxc::ast::ast::CallExpression<'_>,
    ) -> Option<SmeltError> {
        let Expression::StaticMemberExpression(_) = &call.callee else {
            return None;
        };
        None
    }

    /// Lower TypeScript `fetch(url[, options])` into an async HTTP GET text operation.
    pub(super) fn fetch_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if stdlib_dispatch::call_rule(call) != Some(RuleId::TsFetch) {
            return Ok(None);
        }
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        if callee.name != "fetch" {
            return Ok(None);
        }
        if !(1..=2).contains(&call.arguments.len()) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "fetch lowering supports fetch(url[, options])",
            ));
        }
        let Some(url_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "fetch lowering supports fetch(url[, options])",
            ));
        };
        let mut url = self.argument(url_argument, body)?;
        if let Some(options_argument) = call.arguments.get(1) {
            let _ = self.argument(options_argument, body)?;
        }
        let url_ty = Self::expr_ty(body, url);
        if self.ctx.krate.types.get(url_ty) != Some(&Type::String) {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            if self.is_string_compatible_type(url_ty) || self.type_contains_unknown(url_ty) {
                url = body.push_expr(Expr {
                    kind: ExprKind::UnknownCast {
                        value: url,
                        target: string_ty,
                    },
                    ty: string_ty,
                    span: self.span(url_argument.span().start, url_argument.span().end),
                });
            } else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "fetch requires a string-compatible URL argument",
                ));
            }
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let ty = self.ctx.krate.types.intern(Type::Future(string_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::AsyncOp {
                op: AsyncOp::HttpGetText,
                args: vec![url],
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower supported TypeScript `Date` calls.
    pub(super) fn date_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if matches!(&call.callee, Expression::StaticMemberExpression(_))
            && stdlib_dispatch::call_rule(call) == Some(RuleId::TsDateNow)
        {
            if !call.arguments.is_empty() {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Date.now() does not accept arguments",
                ));
            }
            let ty = self.ctx.krate.types.intern(Type::Int);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::DateNow,
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }

        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if let Some(expr) = self.date_utc_call(member, call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.date_member_call(member, call, body)? {
            return Ok(Some(expr));
        }
        if stdlib_dispatch::call_rule(call) != Some(RuleId::TsDateToIsoString) {
            return Ok(None);
        }
        let Expression::NewExpression(new_expr) = &member.object else {
            return Ok(None);
        };
        let Expression::Identifier(callee) = &new_expr.callee else {
            return Ok(None);
        };
        if callee.name != "Date" {
            return Ok(None);
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Date.toISOString() does not accept arguments",
            ));
        }
        let timestamp_ms = self.date_constructor_timestamp(new_expr, body)?;
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DateToIsoString { timestamp_ms },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower `Date.UTC(year, month, ...)` into Smelt's timestamp-from-parts form.
    pub(super) fn date_utc_call(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Date" || member.property.name != "UTC" {
            return Ok(None);
        }
        if !(2..=7).contains(&call.arguments.len()) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Date.UTC requires between two and seven numeric arguments",
            ));
        }
        let mut parts = Vec::with_capacity(call.arguments.len());
        for argument in &call.arguments {
            let part = self.argument(argument, body)?;
            if !matches!(
                self.ctx.krate.types.get(Self::expr_ty(body, part)),
                Some(Type::Int | Type::Float)
            ) {
                return Err(SmeltError::unsupported(
                    self.span(argument.span().start, argument.span().end),
                    "Date.UTC arguments must be numeric",
                ));
            }
            parts.push(part);
        }
        let ty = self.ctx.krate.types.intern(Type::Int);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DateFromParts { parts },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower supported `new Date(...)` expressions to a timestamp value.
    pub(super) fn new_date_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let timestamp_ms = self.date_constructor_timestamp(new_expr, body)?;
        let date_name = self.intern_type_name("Date");
        let ty = self.ctx.krate.types.intern(Type::Class {
            name: date_name,
            args: Vec::new(),
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::DateFromValue {
                value: timestamp_ms,
            },
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        }))
    }

    /// Lower `new (date.constructor as DateCtor)(value)` while retaining Date identity.
    pub(super) fn dynamic_date_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !self.is_date_constructor_member_reference(&new_expr.callee, body) {
            return Ok(None);
        }
        let [value] = new_expr.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "dynamic Date constructor calls require exactly one value argument",
            ));
        };
        let value = self.argument(value, body)?;
        let date_name = self.intern_type_name("Date");
        let ty = self.ctx.krate.types.intern(Type::Class {
            name: date_name,
            args: Vec::new(),
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DateFromValue { value },
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        })))
    }

    /// Return whether a `.constructor` callee is read from a statically typed Date local.
    fn is_date_constructor_member_reference(
        &self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> bool {
        let member = match expression {
            Expression::StaticMemberExpression(member) => member,
            Expression::ParenthesizedExpression(parenthesized) => {
                return self.is_date_constructor_member_reference(&parenthesized.expression, body);
            }
            Expression::TSAsExpression(assertion) => {
                return self.is_date_constructor_member_reference(&assertion.expression, body);
            }
            Expression::TSSatisfiesExpression(assertion) => {
                return self.is_date_constructor_member_reference(&assertion.expression, body);
            }
            Expression::TSNonNullExpression(assertion) => {
                return self.is_date_constructor_member_reference(&assertion.expression, body);
            }
            _ => return false,
        };
        if member.property.name != "constructor" {
            return false;
        }
        let Expression::Identifier(receiver) = &member.object else {
            return false;
        };
        let Some(local) = self.scope.lookup(receiver.name.as_str()) else {
            return false;
        };
        let Some(decl) = usize::try_from(local.0)
            .ok()
            .and_then(|index| body.locals.get(index))
        else {
            return false;
        };
        matches!(
            self.ctx.krate.types.get(decl.ty),
            Some(Type::Class { name, .. })
                if self.ctx.krate.symbols.get(*name) == Some("Date")
        )
    }

    /// Lower guarded dynamic Date constructor identifiers such as `new constructor(0)`.
    pub(super) fn dynamic_identifier_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &new_expr.callee else {
            return Ok(None);
        };
        if callee.name != "constructor" || !self.scope.is_bound(callee.name.as_str()) {
            return Ok(None);
        }
        let [value] = new_expr.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "dynamic Date constructor identifiers require exactly one value argument",
            ));
        };
        Ok(Some(self.argument(value, body)?))
    }

    /// Return the timestamp expression represented by a supported `new Date(...)`.
    pub(super) fn date_constructor_timestamp(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if new_expr.arguments.len() >= 2 {
            if new_expr.arguments.len() > 7 {
                return Err(SmeltError::unsupported(
                    self.span(new_expr.span.start, new_expr.span.end),
                    "new Date(year, month, ...) supports at most seven numeric arguments",
                ));
            }
            let mut parts = Vec::with_capacity(new_expr.arguments.len());
            for argument in &new_expr.arguments {
                let part = self.argument(argument, body)?;
                if !matches!(
                    self.ctx.krate.types.get(Self::expr_ty(body, part)),
                    Some(Type::Int | Type::Float)
                ) {
                    return Err(SmeltError::unsupported(
                        self.span(argument.span().start, argument.span().end),
                        "Date constructor parts must be numeric",
                    ));
                }
                parts.push(part);
            }
            let ty = self.ctx.krate.types.intern(Type::Int);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::DateFromParts { parts },
                ty,
                span: self.span(new_expr.span.start, new_expr.span.end),
            }));
        }
        let [timestamp_arg] = new_expr.arguments.as_slice() else {
            let ty = self.ctx.krate.types.intern(Type::Int);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::DateNow,
                ty,
                span: self.span(new_expr.span.start, new_expr.span.end),
            }));
        };
        let timestamp_ms = self.argument(timestamp_arg, body)?;
        let timestamp_ty = Self::expr_ty(body, timestamp_ms);
        if matches!(
            self.ctx.krate.types.get(timestamp_ty),
            Some(Type::Int | Type::Float)
        ) {
            return Ok(timestamp_ms);
        }
        if self.is_date_constructor_arg_type(timestamp_ty) {
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::DateFromValue {
                    value: timestamp_ms,
                },
                ty,
                span: self.span(new_expr.span.start, new_expr.span.end),
            }));
        }
        Err(SmeltError::unsupported(
            self.span(new_expr.span.start, new_expr.span.end),
            "new Date(timestamp) requires a numeric or DateArg-compatible timestamp",
        ))
    }

    /// Return true for types accepted by JavaScript's one-argument Date constructor.
    pub(super) fn is_date_constructor_arg_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(
                Type::Int
                | Type::Float
                | Type::String
                | Type::Unknown
                | Type::TypeParam { .. }
                | Type::Class { .. },
            ) => true,
            Some(Type::Optional(item)) => self.is_date_constructor_arg_type(*item),
            Some(Type::Union(items)) => items.iter().copied().all(|item| {
                matches!(self.ctx.krate.types.get(item), Some(Type::None))
                    || self.is_date_constructor_arg_type(item)
            }),
            _ => false,
        }
    }

    /// Lower supported Date receiver methods using Smelt's timestamp Date model.
    pub(super) fn date_member_call(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let method = member.property.name.as_str();
        if method == "toISOString" {
            if !call.arguments.is_empty() {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Date.toISOString() does not accept arguments",
                ));
            }
            let receiver = self.expression(&member.object, body)?;
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::DateToIsoString {
                    timestamp_ms: receiver,
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if method == "getTime" {
            if !call.arguments.is_empty() {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Date.getTime() does not accept arguments",
                ));
            }
            let receiver = self.expression(&member.object, body)?;
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToFloat,
                    operand: receiver,
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if method == "setTime" {
            if call.arguments.len() != 1 {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Date.setTime() requires exactly one numeric argument",
                ));
            }
            let receiver = self.expression(&member.object, body)?;
            let receiver_ty = Self::expr_ty(body, receiver);
            if !self.is_date_constructor_arg_type(receiver_ty) {
                return Err(SmeltError::unsupported(
                    self.span(member.object.span().start, member.object.span().end),
                    "Date.setTime() receiver must be a timestamp or Date-like value",
                ));
            }
            let Some(argument) = call.arguments.first() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Date.setTime() requires exactly one numeric argument",
                ));
            };
            let value = self.argument(argument, body)?;
            let value_ty = Self::expr_ty(body, value);
            let value = if self.is_date_constructor_arg_type(value_ty) {
                value
            } else if self
                .non_nullish_type(value_ty)
                .is_some_and(|ty| self.is_numeric_like_type(ty))
            {
                self.non_null_assertion_value(
                    value,
                    self.span(argument.span().start, argument.span().end),
                    body,
                )
            } else {
                return Err(SmeltError::unsupported(
                    self.span(argument.span().start, argument.span().end),
                    "Date.setTime() argument must be numeric",
                ));
            };
            if let Expression::Identifier(identifier) = &member.object
                && let Some(local) = self.scope.lookup(identifier.name.as_str())
            {
                let target = body.push_expr(Expr {
                    kind: ExprKind::Local(local),
                    ty: receiver_ty,
                    span: self.span(identifier.span.start, identifier.span.end),
                });
                if let Some(block) = self.current_statement_block {
                    body.push_stmt_to_block(block, Stmt::Assign { target, value });
                } else {
                    body.push_stmt(Stmt::Assign { target, value });
                }
            }
            return Ok(Some(value));
        }
        if method == "getTimezoneOffset" {
            if !call.arguments.is_empty() {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Date.getTimezoneOffset() does not accept arguments",
                ));
            }
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::DateTimezoneOffset,
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let getter_part = match method {
            "getFullYear" | "getUTCFullYear" => Some(DatePart::FullYear),
            "getMonth" | "getUTCMonth" => Some(DatePart::Month),
            "getDate" | "getUTCDate" => Some(DatePart::Date),
            "getDay" | "getUTCDay" => Some(DatePart::Day),
            "getHours" | "getUTCHours" => Some(DatePart::Hour),
            "getMinutes" | "getUTCMinutes" => Some(DatePart::Minute),
            "getSeconds" | "getUTCSeconds" => Some(DatePart::Second),
            "getMilliseconds" | "getUTCMilliseconds" => Some(DatePart::Millisecond),
            _ => None,
        };
        if let Some(part) = getter_part {
            if !call.arguments.is_empty() {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    format!("Date.{method}() does not accept arguments"),
                ));
            }
            let receiver = self.expression(&member.object, body)?;
            let timestamp_ms = self.date_receiver_timestamp(receiver, member, body)?;
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::DateGetPart { part, timestamp_ms },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let setter_part = match method {
            "setFullYear" | "setUTCFullYear" => Some(DatePart::FullYear),
            "setMonth" | "setUTCMonth" => Some(DatePart::Month),
            "setDate" | "setUTCDate" => Some(DatePart::Date),
            "setHours" | "setUTCHours" => Some(DatePart::Hour),
            "setMinutes" | "setUTCMinutes" => Some(DatePart::Minute),
            "setSeconds" | "setUTCSeconds" => Some(DatePart::Second),
            "setMilliseconds" | "setUTCMilliseconds" => Some(DatePart::Millisecond),
            _ => None,
        };
        let Some(part) = setter_part else {
            return Ok(None);
        };
        if call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("Date.{method}() requires at least one numeric argument"),
            ));
        }
        let receiver = self.expression(&member.object, body)?;
        let mut values = Vec::with_capacity(call.arguments.len());
        for argument in &call.arguments {
            let mut value = self.argument(argument, body)?;
            let value_ty = Self::expr_ty(body, value);
            let value_is_numeric = matches!(
                self.ctx.krate.types.get(value_ty),
                Some(Type::Int | Type::Float | Type::Unknown | Type::TypeParam { .. })
            );
            let narrowed_numeric = self
                .non_nullish_type(value_ty)
                .is_some_and(|ty| self.is_numeric_like_type(ty));
            if !value_is_numeric && !narrowed_numeric {
                return Err(SmeltError::unsupported(
                    self.span(argument.span().start, argument.span().end),
                    format!("Date.{method}() arguments must be numeric"),
                ));
            }
            if !value_is_numeric {
                value = self.non_null_assertion_value(
                    value,
                    self.span(argument.span().start, argument.span().end),
                    body,
                );
            }
            values.push(value);
        }
        let ty = Self::expr_ty(body, receiver);
        let value = body.push_expr(Expr {
            kind: ExprKind::DateSetPart {
                part,
                timestamp_ms: receiver,
                values,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        });
        if let Expression::Identifier(identifier) = &member.object
            && let Some(local) = self.scope.lookup(identifier.name.as_str())
        {
            let target = body.push_expr(Expr {
                kind: ExprKind::Local(local),
                ty,
                span: self.span(identifier.span.start, identifier.span.end),
            });
            if let Some(block) = self.current_statement_block {
                body.push_stmt_to_block(block, Stmt::Assign { target, value });
            } else {
                body.push_stmt(Stmt::Assign { target, value });
            }
        }
        Ok(Some(value))
    }

    /// Convert a Date-like receiver into the timestamp expression used by Date operations.
    pub(super) fn date_receiver_timestamp(
        &mut self,
        receiver: smelt_hir::ExprId,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let receiver_ty = Self::expr_ty(body, receiver);
        if matches!(
            self.ctx.krate.types.get(receiver_ty),
            Some(Type::Int | Type::Float)
        ) {
            return Ok(receiver);
        }
        if self.is_date_constructor_arg_type(receiver_ty) {
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToFloat,
                    operand: receiver,
                },
                ty,
                span: self.span(member.object.span().start, member.object.span().end),
            }));
        }
        Err(SmeltError::unsupported(
            self.span(member.object.span().start, member.object.span().end),
            "Date method receiver must be a timestamp or Date-like value",
        ))
    }

    // Continued in the next split builder file.
}
