//! `JSON.stringify` and `JSON.parse`, and which types can cross them.

use crate::lowering::body_arena::BodyArena;
use crate::lowering::support::source_spelling;
use crate::lowering::{ModuleBuilder, SmeltError};
use oxc::ast::ast::{Argument, CallExpression, Expression};
use oxc::span::GetSpan;
use smelt_hir::{Body, ErasureReason, Expr, ExprKind, Type};
use smelt_stdlib::RuleId;

/// What a type checker needs to know of the typed `JSON.parse<T>(text)` this
/// module lowers: the call takes the target type as its type argument (the
/// lib's `JSON.parse` has none) and returns it.
pub(in crate::lowering) const JSON_PARSE_DECLARATION: &str = "interface JSON {\n  parse<T = any>(text: string, reviver?: (this: any, key: string, value: any) => any): T;\n}\n";

impl ModuleBuilder<'_> {
    /// Lower TypeScript `JSON.stringify(value)` calls for JSON-compatible values.
    pub(in crate::lowering) fn json_stringify_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if self.recognized_call(call) != Some(RuleId::TsJsonStringify) {
            return Ok(None);
        }
        if call.arguments.is_empty() || call.arguments.len() > 3 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "JSON.stringify() currently supports value, replacer, and space arguments",
            ));
        }
        let Some(argument) = call.arguments.first() else {
            return Ok(None);
        };
        // Arguments in source order: the value, then the replacer (a function
        // or an array of keys, applied by the runtime serializer; `null` and
        // `undefined` mean none), then the indentation `space`, which lays the
        // text out afterwards.
        let value = self.argument(argument, body)?;
        let mut space = None;
        let mut replacer = None;
        for (index, extra) in call.arguments.iter().enumerate().skip(1) {
            let lowered = self.argument(extra, body)?;
            if index == 1
                && !matches!(Self::expr_ty(body, lowered), ty if matches!(self.ctx.krate.types.get(ty), Some(Type::None)))
            {
                replacer = Some(lowered);
            }
            if index == 2 {
                space = Some(lowered);
            }
        }
        let value_ty = Self::expr_ty(body, value);
        if !self.is_json_serializable_type(value_ty)
            && self.is_json_serializable_type_but_bigints(value_ty)
        {
            let span = self.span(call.span.start, call.span.end);
            return Ok(Some(self.bigint_json_stringify(value, span, body)));
        }
        // Every other value has a JSON answer (ECMA-262 `SerializeJSONProperty`:
        // a function or symbol property is omitted, `null` in an array), which
        // the runtime serializer gives through the erased carrier; nothing is
        // rejected. The text is `string | undefined` when the value itself may
        // be `undefined`, a function or a symbol, whose top-level answer is
        // `undefined` -- TypeScript's `string` return type does not say so.
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let ty = if self.json_stringify_may_answer_undefined(value_ty, 0)
            && !self.json_stringify_absent_is_null(value_ty, argument)
        {
            self.ctx
                .krate
                .types
                .intern(Type::Optional(string_ty, smelt_hir::Absence::Undefined))
        } else {
            string_ty
        };
        let span = self.span(call.span.start, call.span.end);
        let text = body.push_expr(Expr {
            kind: ExprKind::JsonStringify { value, replacer },
            ty,
            span,
        });
        let Some(space) = space else {
            return Ok(Some(text));
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::JsonIndent { text, space },
            ty,
            span,
        })))
    }

    /// Whether the only undefined-answering case of the value is an absent
    /// case the checker types as `null`, whose JSON text is `"null"`.
    ///
    /// The HIR's `None` carries both `null` and `undefined`, so `None` alone
    /// cannot tell `JSON.stringify(null)` (`"null"`) from
    /// `JSON.stringify(undefined)` (`undefined`); the literal or the checker's
    /// type of the argument can, and an optional's own absence tag says it. Holds for `None`, or `Optional(T)` whose `T` never answers
    /// `undefined`, when the argument is the `null` literal or the checker
    /// admits `null` and not `undefined`/`void`.
    fn json_stringify_absent_is_null(
        &self,
        ty: smelt_hir::TypeId,
        argument: &Argument<'_>,
    ) -> bool {
        let present_answers_text = match self.ctx.krate.types.get(ty) {
            Some(Type::None) => true,
            Some(Type::Optional(inner, _)) => !self.json_stringify_may_answer_undefined(*inner, 1),
            _ => false,
        };
        // The `null` keyword carries no recorded checker type; it is `null`.
        let literal_null = argument.as_expression().is_some_and(|expression| {
            matches!(expression.without_parentheses(), Expression::NullLiteral(_))
        });
        // A `T | null` says so in its type (`smelt_hir::Absence::Null`).
        let tagged_null = matches!(
            self.ctx.krate.types.get(ty),
            Some(Type::Optional(_, smelt_hir::Absence::Null))
        );
        let span = argument.span();
        present_answers_text
            && (literal_null || tagged_null || self.checker_absent_is_null(span.start, span.end))
    }

    /// Whether a value of `ty` may be one whose `JSON.stringify` is
    /// `undefined`: `undefined`, a function or a symbol, or a value whose type
    /// does not say (erased, generic, a union holding one of those).
    fn json_stringify_may_answer_undefined(&self, ty: smelt_hir::TypeId, depth: usize) -> bool {
        if depth > 8 {
            return true;
        }
        match self.ctx.krate.types.get(ty) {
            Some(
                Type::Unknown
                | Type::TypeParam { .. }
                | Type::None
                | Type::Optional(..)
                | Type::Function(_)
                | Type::Symbol
                | Type::PropKey,
            )
            | None => true,
            Some(Type::Union(members)) => members
                .iter()
                .any(|member| self.json_stringify_may_answer_undefined(*member, depth + 1)),
            _ => false,
        }
    }

    /// Lower TypeScript `JSON.parse<T>(text)` calls for JSON-compatible targets.
    pub(in crate::lowering) fn json_parse_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if self.recognized_call(call) != Some(RuleId::TsJsonParse) {
            return Ok(None);
        }
        let [argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "JSON.parse<T>() currently supports exactly one text argument",
            ));
        };
        let ty = if let Some(type_args) = &call.type_arguments {
            let [target_ty] = type_args.params.as_slice() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "JSON.parse requires exactly one type argument",
                ));
            };
            self.ts_type_to_hir(target_ty)?
        } else {
            self.json_parse_fallback_type()
        };
        if !self.is_json_serializable_type(ty) && !self.is_erased_json_class_target(ty) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "JSON.parse<T>() target type must be JSON-compatible",
            ));
        }
        let text = self.json_parse_text_argument(argument, body, "JSON.parse<T>()")?;
        // Only a source-written `<T>` is a declared target; the fallback
        // `Record<string, unknown>` is the frontend's guess, and decoding
        // against it would reject arrays and primitives Node accepts.
        let target = call.type_arguments.is_some().then_some(ty);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::JsonParse { text, target },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return the fallback type for untyped `JSON.parse(text)`.
    ///
    /// **Dynamic boundary.** TypeScript types the call `any`, and the value is
    /// whatever the TEXT holds — `null`, a number, a string, an array or an
    /// object — which only the run-time parse knows: `JSON.parse('null')` is
    /// `null`, so `body === null` validation must fire (Hono's `c.req.json()` is
    /// `JSON.parse` of the body text). No concrete type, union or generic states
    /// "any JSON value" more precisely than the erased carrier; a source that knows
    /// the shape spells it with `JSON.parse<T>` or an `as T` assertion, which keep
    /// their typed targets.
    fn json_parse_fallback_type(&mut self) -> smelt_hir::TypeId {
        self.erase_with(ErasureReason::ExternalType, "JSON.parse")
    }

    /// Lower `JSON.parse(text) as T` using the assertion type as parse target.
    pub(in crate::lowering) fn json_parse_call_with_target(
        &mut self,
        expression: &Expression<'_>,
        target: smelt_hir::TypeId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::CallExpression(call) = expression else {
            return Ok(None);
        };
        if self.recognized_call(call) != Some(RuleId::TsJsonParse) {
            return Ok(None);
        }
        let [argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "JSON.parse() currently supports exactly one text argument",
            ));
        };
        if call.type_arguments.is_some() {
            return Ok(None);
        }
        if !self.is_json_serializable_type(target) && !self.is_erased_json_class_target(target) {
            return Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "JSON.parse assertion target type must be JSON-compatible",
            ));
        }
        let text = self.json_parse_text_argument(argument, body, "JSON.parse()")?;
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::JsonParse {
                text,
                target: Some(target),
            },
            ty: target,
            span: self.span(span.start, span.end),
        })))
    }

    /// Lower an untyped `JSON.parse(text)` whose value flows into a declared type.
    ///
    /// `const v: T = JSON.parse(s)`, `f(JSON.parse(s))` with `f(x: T)` and
    /// `return JSON.parse(s)` from a `T`-returning function all hand the parse a
    /// contextual type the SOURCE wrote. TypeScript types `JSON.parse` as `any`,
    /// so the value enters typed code exactly there: this lowers the parse with
    /// `T` as its decode target, the same expression `JSON.parse(s) as T`
    /// produces, instead of the `Record<string, unknown>` fallback that is later
    /// coerced (leniently) into `T`. Design adapted from Bun's `--aot` rule that a
    /// value which does not match its type throws a `TypeError` where it enters
    /// typed code (oven-sh/bun#45631, `docs/bundler/aot-adoption.mdx`).
    ///
    /// Behavioral requirement: JSON that matches `T` yields the value Node yields.
    /// Type-preservation requirement: the parse is typed `T`, so the backend can
    /// store the decoded value at `T`'s concrete Rust type.
    ///
    /// Returns `None` (keeping the untyped lowering) for a call that is not an
    /// untyped single-argument `JSON.parse`, for a hint that is not a real
    /// declared shape (`unknown`/`any`, a bare type parameter, `void`), and for a
    /// hint `JSON.stringify` could not round-trip — a contextual type never
    /// turns an accepted program into a rejected one.
    pub(in crate::lowering) fn json_parse_call_with_hint(
        &mut self,
        call: &CallExpression<'_>,
        hint: Option<smelt_hir::TypeId>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(target) = hint else {
            return Ok(None);
        };
        if self.recognized_call(call) != Some(RuleId::TsJsonParse)
            || call.type_arguments.is_some()
            || call.arguments.len() != 1
        {
            return Ok(None);
        }
        if !self.is_json_decode_hint(target) {
            return Ok(None);
        }
        let [argument] = call.arguments.as_slice() else {
            return Ok(None);
        };
        let text = self.json_parse_text_argument(argument, body, "JSON.parse()")?;
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::JsonParse {
                text,
                target: Some(target),
            },
            ty: target,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Whether a contextual type is a declared shape a JSON value can be
    /// decoded into: not `unknown`/`any`, a bare type parameter, `void` or
    /// `never`, and a type `JSON.stringify` could round-trip.
    fn is_json_decode_hint(&mut self, target: smelt_hir::TypeId) -> bool {
        !matches!(
            self.ctx.krate.types.get(target),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::None | Type::Never) | None
        ) && self.is_json_serializable_type(target)
    }

    /// Give a `Response`/`Request` `json()` body read its declared target.
    ///
    /// `const v: T = await response.json()` awaits a `Promise<any>` whose value
    /// enters typed code at `T`, exactly as `JSON.parse(text)` does in
    /// [`Self::json_parse_call_with_hint`]. The await hands the operand the
    /// context `Promise<T>`; when `value` is a `json()` read still typed
    /// `Promise<unknown>`, this retypes it `Promise<T>` so the backend decodes
    /// the body strictly into `T` (a catchable `TypeError` on mismatch) instead
    /// of coercing the erased value afterwards. Any other expression, or a hint
    /// that is not a decodable declared shape, is returned unchanged.
    pub(in crate::lowering) fn adopt_json_body_read_target(
        &mut self,
        value: smelt_hir::ExprId,
        hint: Option<smelt_hir::TypeId>,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let Some(Type::Future(target)) = hint
            .and_then(|hint| self.ctx.krate.types.get(hint))
            .cloned()
        else {
            return value;
        };
        let Some(expr) = body.expr_at(value) else {
            return value;
        };
        let is_json_read = matches!(
            expr.kind,
            ExprKind::ResponseOp {
                op: smelt_hir::ResponseOp::Json,
                ..
            } | ExprKind::RequestOp {
                op: smelt_hir::RequestOp::Json,
                ..
            }
        );
        let still_untyped = matches!(
            self.ctx.krate.types.get(expr.ty),
            Some(Type::Future(inner)) if self.ctx.krate.types.get(*inner) == Some(&Type::Unknown)
        );
        if !is_json_read || !still_untyped || !self.is_json_decode_hint(target) {
            return value;
        }
        let future_ty = self.ctx.krate.types.intern(Type::Future(target));
        if let Some(read) = body.expr_at_mut(value) {
            read.ty = future_ty;
        }
        value
    }

    /// Lower and coerce the text operand accepted by `JSON.parse`.
    fn json_parse_text_argument(
        &mut self,
        argument: &Argument<'_>,
        body: &mut Body,
        source_name: &str,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let text = self.argument(argument, body)?;
        let text_ty = Self::expr_ty(body, text);
        if self.ctx.krate.types.get(text_ty) == Some(&Type::String) {
            return Ok(text);
        }
        if self.is_string_compatible_type(text_ty) || self.type_contains_unknown(text_ty) {
            let target = self.ctx.krate.types.intern(Type::String);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: text,
                    target,
                },
                ty: target,
                span: self.span(argument.span().start, argument.span().end),
            }));
        }
        Err(SmeltError::unsupported(
            self.span(argument.span().start, argument.span().end),
            format!("{source_name} text argument must be a string"),
        ))
    }

    /// Return whether a HIR type can be serialized by the JSON mapping.
    fn is_json_serializable_type(&mut self, ty: smelt_hir::TypeId) -> bool {
        self.is_json_serializable_type_inner(ty, &mut Vec::new(), false, false)
    }

    /// Return whether a HIR type would be serializable if its bigints were.
    ///
    /// A bigint has no JSON spelling, but `JSON.stringify` over one is a
    /// run-time `TypeError` that a `try` can catch, not a compile error. A
    /// type that fails [`Self::is_json_serializable_type`] only because of a
    /// bigint lowers to the fallible `BigIntOp::JsonStringify` call.
    fn is_json_serializable_type_but_bigints(&mut self, ty: smelt_hir::TypeId) -> bool {
        self.is_json_serializable_type_inner(ty, &mut Vec::new(), false, true)
    }

    /// Return whether a HIR type can be serialized without recursing forever.
    fn is_json_serializable_type_inner(
        &mut self,
        ty: smelt_hir::TypeId,
        seen: &mut Vec<smelt_hir::Symbol>,
        nested: bool,
        bigint_ok: bool,
    ) -> bool {
        let Some(ty_kind) = self.ctx.krate.types.get(ty).cloned() else {
            return false;
        };
        match ty_kind {
            Type::Bool
            | Type::Int
            | Type::PyInt
            | Type::FixedInt(_)
            | Type::Float
            | Type::String
            | Type::PyStr
            // A symbol serializes like `undefined`: dropped from an object,
            // `null` in an array; the erased serializer applies that rule.
            | Type::Symbol
            | Type::Unknown
            | Type::TypeParam { .. } => true,
            // An unevaluated indexed access is never a value's type (see
            // `Type::IndexedAccess`); it answers as the `unknown` it erases to.
            Type::IndexedAccess { .. } => true,
            // Never a value on its own; only a record key.
            Type::PropKey => true,
            // `JSON.stringify(1n)` throws `TypeError: Do not know how to
            // serialize a BigInt`; there is no JSON spelling to emit. Only the
            // `bigint_ok` walk accepts it, to pick the fallible lowering.
            Type::BigInt => bigint_ok,
            Type::List(item) | Type::Set(item) | Type::Optional(item, _) => {
                self.is_json_serializable_type_inner(item, seen, true, bigint_ok)
            }
            Type::Tuple(items) => items
                .iter()
                .all(|item| self.is_json_serializable_type_inner(*item, seen, true, bigint_ok)),
            Type::Union(items) => items
                .iter()
                .all(|item| self.is_json_serializable_type_inner(*item, seen, true, bigint_ok)),
            Type::Dict(key, value) | Type::JsMap(key, value) => {
                matches!(self.ctx.krate.types.get(key), Some(Type::String | Type::PropKey))
                    && self.is_json_serializable_type_inner(value, seen, true, bigint_ok)
            }
            Type::Class { name, args } => {
                // A HOST OBJECT serializes as `{}`: none of its state is an own
                // enumerable property, so JavaScript has nothing to write.
                // `JSON.stringify(new Blob(['a']))`, `new FormData()`,
                // `new URLSearchParams('a=1')`, `new Headers([..])` and
                // `new Request(url)` are all `{}` in Node. Treating "Smelt does
                // not know this class's fields" as "not serializable" rejected
                // source that JavaScript accepts, and it is what made a
                // `BodyInit` union unserializable as a whole.
                if source_spelling(&self.ctx.krate, name)
                    // A host object serializes cleanly: it erases to a
                    // byte-backed or marker record whose own enumerable
                    // properties are what `Serialize for SmeltUnknown` renders
                    // — a view as its element indices
                    // (`JSON.stringify(new TextEncoder().encode("hi"))` is
                    // `{"0":104,"1":105}`), a marker-only host as `{}`.
                    // Rejecting these made every program that serializes
                    // encoded bytes a blocker even though the value crosses the
                    // boundary cleanly.
                    .is_some_and(|class_name| {
                        smelt_stdlib::host_object_marker(class_name).is_some()
                    })
                {
                    return true;
                }
                if seen.contains(&name) {
                    // A RECURSIVE shape is serializable exactly when its
                    // non-recursive parts are: `JSON.stringify` walks a value,
                    // not a type, and the walk terminates on the value. Hono's
                    // `JSONObject`/`JSONValue`/`JSONArray` triangle is the
                    // canonical example, and answering "unserializable" for the
                    // recursion point made the whole JSON type family
                    // unserializable.
                    return true;
                }
                // A nominal class standing for a TYPE ALIAS is that alias:
                // resolve the alias path rather than judging the stand-in. A
                // recursive alias cannot be expanded at its recursion point, so
                // it arrives here as a nominal class — `JSONObject` inside
                // `JSONValue` is exactly that — and asking whether the alias's
                // own lowered shape is serializable answers the real question.
                if let Some(alias) = self.find_type_alias(name).cloned() {
                    seen.push(name);
                    let serializable = self.is_json_serializable_type_inner(alias.ty, seen, true, bigint_ok);
                    seen.pop();
                    if serializable {
                        return true;
                    }
                }
                // A name with no declaration lowered YET is not judged here.
                // Modules lower in value-import order, and a module imported
                // only for its types (date-fns' `types.ts`, whose `Duration` is
                // an interface of optional numbers) can lower after the module
                // that serializes one of its interfaces. The backend repeats
                // this check over the whole program
                // (`FunctionEmitter::json_stringify_expr`) and refuses a shape
                // that is really not serializable, at this call's site.
                let Some(fields) = self.json_class_fields(name, &args) else {
                    return true;
                };
                seen.push(name);
                let serializable = fields
                    .iter()
                    .all(|field| self.is_json_serializable_type_inner(field.ty, seen, true, bigint_ok));
                seen.pop();
                serializable
            }
            Type::None => true,
            // A FUNCTION-valued member is serializable: `JSON.stringify` drops
            // a function-valued property and writes `null` for one in an array
            // — `{a: () => 1, b: 1}` is `{"b":1}` and `[() => 1]` is `[null]`.
            // Rejecting it made every JSON type family that admits one
            // unserializable as a whole, which is most of them: Hono's
            // `InvalidJSONValue` is `undefined | symbol | ((...args) => unknown)`
            // and appears in `JSONObject`'s index value.
            //
            // The TOP-LEVEL argument stays a blocker: `JSON.stringify(fn)`
            // evaluates to `undefined`, not to a string, so a program that
            // serializes a bare function is doing something the `string` return
            // type does not describe and deserves to be told.
            Type::Function(_) => nested,
            // An iterator object (`JSON.stringify(map.keys())`) serializes as
            // `{}`: its state is not an own enumerable property. It crosses as
            // its erasure, whose `next` and marker are hidden.
            Type::Generator { is_async: false, .. } => true,
            Type::Never
            | Type::Future(_)
            | Type::Generator { .. }
            | Type::GeneratorResult { .. } => false,
        }
    }

    /// Return fields for class-like TypeScript types that can map to JSON objects.
    pub(in crate::lowering) fn json_class_fields(
        &mut self,
        name: smelt_hir::Symbol,
        args: &[smelt_hir::TypeId],
    ) -> Option<Vec<smelt_hir::Field>> {
        if let Some(class) = self.class_by_symbol(name).cloned() {
            let substitutions = self
                .type_argument_substitution(&class.type_params, args, self.span(0, 0))
                .ok()?;
            return Some(self.substituted_fields(&class.fields, &substitutions));
        }
        if let Some(interface) = self.find_interface(name).cloned() {
            let substitutions = self
                .type_argument_substitution(&interface.type_params, args, self.span(0, 0))
                .ok()?;
            return Some(self.substituted_fields(&interface.fields, &substitutions));
        }
        self.types.alias_fields(name).cloned()
    }

    /// Return whether a JSON parse target is a class-like shape whose fields are erased.
    fn is_erased_json_class_target(&self, ty: smelt_hir::TypeId) -> bool {
        matches!(self.ctx.krate.types.get(ty), Some(Type::Class { .. }))
    }
}

impl ModuleBuilder<'_> {
    /// `JSON.stringify` / `JSON.parse` read as values (`const s =
    /// JSON.stringify`): a first-class closure that runs the op the direct
    /// call lowers to, reporting the builtin's own `name` and `length`
    /// ([`Self::name_builtin_static_function_value`]).
    ///
    /// * `JSON.stringify` is `(value, replacer?, space?)`: the serializer
    ///   inspects each argument's runtime kind (any value; a function or a key
    ///   array or nothing; a number or a string or nothing), so the parameters
    ///   are the tagged value at that runtime-narrowing boundary, the two
    ///   trailing ones optional. The text is `string | undefined`, as for a
    ///   direct call on a value whose type does not rule out `undefined`.
    /// * `JSON.parse` is `(text: string)` answering the parsed JSON value,
    ///   which only the text decides (the direct call's `external-type`
    ///   boundary). A reviver is not supported here, as on the direct call.
    ///
    /// Behavioral requirement: a call through the value answers what the
    /// direct call with the same arguments answers. Type preservation: the
    /// closure's parameter and result types are the ones the direct call
    /// gives its arguments and result at an erased argument.
    ///
    /// Returns `None` for any other member, or when `JSON` is shadowed.
    pub(in crate::lowering) fn json_static_function_member(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        outer_body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        if !self.names_host(object.name.as_str(), "JSON") {
            return None;
        }
        let span = self.span(member.span.start, member.span.end);
        match member.property.name.as_str() {
            "stringify" => {
                let value_ty = self.erase_at(
                    ErasureReason::RuntimeNarrowing,
                    span,
                    "JSON.stringify value",
                );
                let replacer_ty = self.erase_at(
                    ErasureReason::RuntimeNarrowing,
                    span,
                    "JSON.stringify replacer",
                );
                let space_ty = self.erase_at(
                    ErasureReason::RuntimeNarrowing,
                    span,
                    "JSON.stringify space",
                );
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let text_ty = self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Optional(string_ty, smelt_hir::Absence::Undefined));
                let closure = self.builtin_closure_expression(
                    ("value", value_ty),
                    &[("replacer", replacer_ty), ("space", space_ty)],
                    text_ty,
                    span,
                    outer_body,
                    false,
                    |body, value, extra| {
                        let text = body.push_expr(Expr {
                            kind: ExprKind::JsonStringify {
                                value,
                                replacer: extra.first().copied(),
                            },
                            ty: text_ty,
                            span,
                        });
                        let space = extra.get(1).copied().unwrap_or(text);
                        ExprKind::JsonIndent { text, space }
                    },
                );
                Some(self.require_leading_closure_params(closure, 1, outer_body))
            }
            "parse" => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let value_ty = self.json_parse_fallback_type();
                Some(self.builtin_unary_closure_expression_with_throw(
                    string_ty,
                    value_ty,
                    span,
                    outer_body,
                    true,
                    |text| ExprKind::JsonParse { text, target: None },
                ))
            }
            _ => None,
        }
    }

    /// Mark only the first `required` parameters of a synthesized closure
    /// value as required, so a call through it may omit the trailing ones
    /// (they read `undefined`), in its expression and its function type.
    fn require_leading_closure_params(
        &mut self,
        closure: smelt_hir::ExprId,
        required: usize,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let Some(expr) = usize::try_from(closure.0)
            .ok()
            .and_then(|index| body.exprs.get_mut(index))
        else {
            return closure;
        };
        let ExprKind::Closure(closure_expr) = &mut expr.kind else {
            return closure;
        };
        closure_expr.required_params = Some(required);
        let Some(Type::Function(function)) = self.ctx.krate.types.get(expr.ty).cloned() else {
            return closure;
        };
        expr.ty = self
            .ctx
            .krate
            .types
            .intern(Type::Function(smelt_hir::FunctionType {
                required_params: Some(required),
                ..function
            }));
        closure
    }
}
