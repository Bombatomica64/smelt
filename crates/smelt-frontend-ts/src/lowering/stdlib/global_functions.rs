//! Global functions: `require`, `structuredClone`, `Object.prototype.toString`,
//! `atob`/`btoa` and the URI codecs.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::body_arena::BodyArena;
use crate::lowering::erasure::NotNewErasure;
use oxc::ast::ast::Expression;
use oxc::span::GetSpan;
use smelt_hir::{Body, ErasureReason, Expr, ExprKind, Type};

impl ModuleBuilder<'_> {
    /// Lower `CommonJS` `require(path)` as an opaque JSON-like module object.
    pub(in crate::lowering) fn commonjs_require_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !matches!(&call.callee, Expression::Identifier(identifier) if identifier.name == "require")
        {
            return Ok(None);
        }
        let [argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "require() requires exactly one module path argument",
            ));
        };
        let _ = self.argument(argument, body)?;
        let key_ty = self.ctx.krate.types.intern(Type::String);
        let value_ty = self.erase_with(ErasureReason::ByName, "require");
        let ty = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictLit(Vec::new()),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower `structuredClone(value)`: a deep copy of the value graph.
    ///
    /// A value of an immutable type (a number, a string, a boolean, or an
    /// optional or union of those) is its own copy, so it passes through. Any
    /// other typed value is copied at its own type through the
    /// `SmeltStructuredClone` impls (`codegen-rust/src/emitter/structured_clone.rs`):
    /// lists, records, maps and sets get fresh identities, and a reference
    /// shared inside the graph (or a cycle) stays shared in the copy. An
    /// erased (`unknown`) value is copied by the `smelt_structured_clone`
    /// runtime helper, which keeps host markers.
    ///
    /// The copy is fallible when the value's type can hold a function, which
    /// the structured clone algorithm rejects with a `DataCloneError`.
    pub(in crate::lowering) fn structured_clone_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        if !self.names_host(callee.name.as_str(), "structuredClone") {
            return Ok(None);
        }
        let [value] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "structuredClone requires exactly one value",
            ));
        };
        let operand = self.argument(value, body)?;
        // not a new erasure: only compared against the operand's type
        let unknown_ty = self.unknown_type(NotNewErasure::RuntimeHelperSignature);
        let Some(operand_ty) = body.expr_at(operand).map(|expr| expr.ty) else {
            return Ok(Some(operand));
        };
        let typed = operand_ty != unknown_ty;
        if typed && self.clone_is_identity(operand_ty) {
            return Ok(Some(operand));
        }
        let can_throw = typed && self.clone_may_reach_function(operand_ty, &mut Vec::new());
        let span = self.span(call.span.start, call.span.end);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StructuredClone {
                operand,
                typed,
                can_throw,
            },
            ty: operand_ty,
            span,
        })))
    }

    /// Whether a value of type `ty` is its own structured clone: an
    /// immutable primitive, or an optional or union of them.
    fn clone_is_identity(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(
                Type::Float
                | Type::Int
                | Type::Bool
                | Type::String
                | Type::BigInt
                | Type::None
                | Type::Never,
            ) => true,
            Some(Type::Optional(inner, _)) => self.clone_is_identity(*inner),
            Some(Type::Union(members)) => members.iter().all(|member| self.clone_is_identity(*member)),
            _ => false,
        }
    }

    /// Whether a value of type `ty` can hold a function anywhere in its graph,
    /// a record's fields included: the clone then may throw `DataCloneError`.
    fn clone_may_reach_function(
        &mut self,
        ty: smelt_hir::TypeId,
        seen: &mut Vec<smelt_hir::TypeId>,
    ) -> bool {
        if seen.contains(&ty) {
            return false;
        }
        seen.push(ty);
        let Some(found) = self.ctx.krate.types.get(ty).cloned() else {
            return false;
        };
        match found {
            Type::Function(_) => true,
            Type::Class { .. } => {
                let span = self.erasure_anchor;
                let records_before = self.ctx.krate.erasures.len();
                let fields: Vec<smelt_hir::TypeId> = self
                    .record_source(ty, span)
                    .ok()
                    .flatten()
                    .map(|source| source.fields.iter().map(|field| field.ty).collect())
                    .unwrap_or_default();
                self.ctx.krate.erasures.truncate(records_before);
                fields
                    .into_iter()
                    .any(|field| self.clone_may_reach_function(field, seen))
            }
            other => other
                .children()
                .collect::<Vec<_>>()
                .into_iter()
                .any(|child| self.clone_may_reach_function(child, seen)),
        }
    }

    /// Lower `Object.prototype.toString.call(value)` to the `"[object Tag]"` probe.
    ///
    /// This is the classic JavaScript type-probing idiom (isPlainObject,
    /// lodash `getTag`): the result string distinguishes primitives, arrays,
    /// functions, and host objects by their `@@toStringTag`-style tag. The
    /// lowering resolves the tag at runtime from the erased value's variant
    /// and host identity markers (see the `smelt_object_to_string_tag` runtime
    /// helper), instead of mis-reading `toString`/`call` as fields of the
    /// prototype sentinel. Only fires for the exact unshadowed
    /// `Object.prototype.toString.call(x)` spelling so every other member call
    /// keeps its existing path.
    pub(in crate::lowering) fn object_prototype_to_string_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(call_member) = &call.callee else {
            return Ok(None);
        };
        if call_member.property.name != "call" {
            return Ok(None);
        }
        let Expression::StaticMemberExpression(to_string_member) = &call_member.object else {
            return Ok(None);
        };
        if to_string_member.property.name != "toString" {
            return Ok(None);
        }
        let Expression::StaticMemberExpression(prototype_member) = &to_string_member.object else {
            return Ok(None);
        };
        if prototype_member.property.name != "prototype" {
            return Ok(None);
        }
        let Expression::Identifier(object) = &prototype_member.object else {
            return Ok(None);
        };
        // A local binding or user class named `Object` shadows the builtin.
        if !self.names_host(object.name.as_str(), "Object")
            || self.scope.is_bound("Object")
            || self.classes.contains("Object")
        {
            return Ok(None);
        }
        let [value_arg] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Object.prototype.toString.call requires exactly one value argument",
            ));
        };
        let operand = self.argument(value_arg, body)?;
        let string_ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ObjectToStringTag { operand },
            ty: string_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Which direction of the base64 codec `name` names, if any.
    ///
    /// Asked of the name alone, so the direct-call lowering and the bare-value
    /// form (`values.map(atob)`) cannot disagree about which global is which
    /// direction.
    pub(in crate::lowering) const fn base64_global(name: &str) -> Option<smelt_hir::Base64Op> {
        match name.as_bytes() {
            b"btoa" => Some(smelt_hir::Base64Op::Encode),
            b"atob" => Some(smelt_hir::Base64Op::Decode),
            _ => None,
        }
    }

    /// Lower `btoa(text)` / `atob(text)` to the base64 IR op.
    ///
    /// The operand rule is the URI transcoders': a concrete `string` passes
    /// through and an erased or string-convertible operand is asserted to
    /// `string` first, because `btoa(1)` is `btoa("1")` in JavaScript. A source
    /// binding of either name shadows the global and declines here.
    pub(in crate::lowering) fn base64_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        let name = callee.name.as_str();
        let Some(op) = Self::base64_global(name) else {
            return Ok(None);
        };
        if self.scope.is_bound(name) || self.items.contains_key(name) {
            return Ok(None);
        }
        let operand = self.single_string_operand(name, call, body)?;
        let string_ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Base64Transcode { op, operand },
            ty: string_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower the only argument of a one-string builtin call (`btoa(s)`,
    /// `encodeURI(s)`) to a `string` operand.
    ///
    /// A `string` passes through; a string-compatible or erased operand is
    /// asserted to `string`; anything else, or any arity but one, is rejected
    /// under the builtin's `name`.
    fn single_string_operand(
        &mut self,
        name: &str,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let [value_arg] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("{name} requires exactly one string argument"),
            ));
        };
        let value = self.argument(value_arg, body)?;
        let value_ty = Self::expr_ty(body, value);
        if self.ctx.krate.types.get(value_ty) == Some(&Type::String) {
            return Ok(value);
        }
        if self.is_string_compatible_type(value_ty) || self.type_contains_unknown(value_ty) {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value },
                ty: string_ty,
                span: self.span(value_arg.span().start, value_arg.span().end),
            }));
        }
        Err(SmeltError::unsupported(
            self.span(value_arg.span().start, value_arg.span().end),
            format!("{name} argument must be a string"),
        ))
    }

    /// Which of the four ECMA-262 URI transcoding globals `name` is, if any.
    ///
    /// The four share one grammar and one operand rule and differ only in the
    /// character set they treat as structure, so they share one lowering (see
    /// [`smelt_hir::UriTranscodeOp`]) rather than four near-identical ones.
    pub(in crate::lowering) const fn uri_transcode_global(
        name: &str,
    ) -> Option<smelt_hir::UriTranscodeOp> {
        match name.as_bytes() {
            b"encodeURI" => Some(smelt_hir::UriTranscodeOp::Encode),
            b"encodeURIComponent" => Some(smelt_hir::UriTranscodeOp::EncodeComponent),
            b"decodeURI" => Some(smelt_hir::UriTranscodeOp::Decode),
            b"decodeURIComponent" => Some(smelt_hir::UriTranscodeOp::DecodeComponent),
            _ => None,
        }
    }

    /// Lower `encodeURI`/`encodeURIComponent`/`decodeURI`/`decodeURIComponent`
    /// to the URI transcoding IR op.
    ///
    /// The operand must be string-compatible: a concrete `string` passes
    /// through, and an erased or string-convertible operand is asserted to
    /// `string` first, mirroring how other string builtins accept erased
    /// operands. The bare-value form of each is handled by
    /// `builtin_function_value_expression`, which builds a closure over the
    /// same op.
    pub(in crate::lowering) fn uri_encode_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        let Some(op) = Self::uri_transcode_global(callee.name.as_str()) else {
            return Ok(None);
        };
        if self.scope.is_bound(callee.name.as_str()) {
            return Ok(None);
        }
        let operand = self.single_string_operand(callee.name.as_str(), call, body)?;
        let string_ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::UriTranscode { op, operand },
            ty: string_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }
}
