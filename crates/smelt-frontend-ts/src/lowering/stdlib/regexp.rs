//! Regular expressions: literals, `new RegExp`, `test`/`exec`, and the string methods
//! that take a pattern (`match`, `replace`).

use crate::lowering::erasure::NotNewErasure;
use crate::lowering::body_arena::BodyArena;
use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use oxc::ast::ast::{Argument, CallExpression, Expression};
use oxc::span::GetSpan;
use smelt_hir::{
    Body, Expr, ExprKind, FunctionType, Literal, RegexMatchOp, Span, StringReplaceOp, Type,
};
use smelt_stdlib::RuleId;

/// The callee, arguments and span of a `new X(..)` or `X(..)` expression: the two
/// spellings of constructing a `RegExp`.
fn regexp_construction<'e, 'a>(
    expression: &'e Expression<'a>,
) -> Option<(&'e Expression<'a>, &'e [Argument<'a>], oxc::span::Span)> {
    match expression {
        Expression::NewExpression(new_expr) => {
            Some((&new_expr.callee, new_expr.arguments.as_slice(), new_expr.span))
        }
        Expression::CallExpression(call_expr) => {
            Some((&call_expr.callee, call_expr.arguments.as_slice(), call_expr.span))
        }
        _ => None,
    }
}

/// Whether a callee is the bare global `RegExp`.
fn callee_is_regexp(callee: &Expression<'_>) -> bool {
    matches!(callee, Expression::Identifier(callee) if callee.name == "RegExp")
}

impl ModuleBuilder<'_> {
    /// Resolve which ECMA-262 replacer arguments a `.replace(re, fn)` callback
    /// declared.
    ///
    /// `RegExp.prototype[@@replace]` calls the replacer with a fixed positional
    /// list — `(matched, p1, …, pN, position, string)` where `N` is the
    /// pattern's capture-group count — and a callback declares a *prefix* of
    /// it. So the meaning of the second parameter is a property of the PATTERN,
    /// not of the callback: with no capture groups it is the match position (a
    /// number), with one it is capture group 1 (a string that is `undefined`
    /// when the group did not participate).
    ///
    /// `N` therefore has to be known here. It is, whenever the pattern lowered
    /// to a string literal — which covers every regex literal, every
    /// `new RegExp('…')` on a literal, and every module `const` bound to one,
    /// because `regex_replacement_pattern` folds all three to `Literal::String`.
    ///
    /// # Errors
    ///
    /// * A callback declaring more parameters than the spec supplies is a
    ///   source error, and reported as one rather than silently truncated.
    /// * A callback with more than one parameter over a pattern whose text is
    ///   not statically known cannot have its parameter ROLES resolved at all.
    ///   Reporting that is the honest outcome: guessing "capture group" would
    ///   hand a number-typed parameter a string (and the reverse) with no
    ///   diagnostic.
    fn regex_replacer_arg_plan(
        pattern: smelt_hir::ExprId,
        declared_arity: usize,
        span: Span,
        body: &Body,
    ) -> Result<Vec<smelt_hir::RegexReplaceArg>, SmeltError> {
        use smelt_hir::RegexReplaceArg;

        // A single-parameter (or parameterless) callback needs no capture
        // count: argument 0 is the matched substring for every pattern.
        if declared_arity <= 1 {
            return Ok(vec![RegexReplaceArg::Matched; declared_arity]);
        }
        let Some(pattern_text) = Self::string_literal_expr_text(body, pattern) else {
            return Err(SmeltError::unsupported(
                span,
                format!(
                    "regex replacement callback declares {declared_arity} parameters, but the \
                     pattern text is not statically known, so it cannot be decided which are \
                     capture groups and which are the match position and subject string"
                ),
            ));
        };
        let capture_count = smelt_stdlib::js_regex::capture_group_count(&pattern_text);
        let mut roles = Vec::with_capacity(usize::try_from(capture_count).unwrap_or(0) + 3);
        roles.push(RegexReplaceArg::Matched);
        for group in 1..=capture_count {
            roles.push(RegexReplaceArg::Capture(group));
        }
        roles.push(RegexReplaceArg::Position);
        roles.push(RegexReplaceArg::Source);
        if declared_arity > roles.len() {
            return Err(SmeltError::unsupported(
                span,
                format!(
                    "regex replacement callback declares {declared_arity} parameters, but the \
                     pattern has {capture_count} capture group(s), so the replacer is called with \
                     only {} arguments (matched, captures, position, subject)",
                    roles.len()
                ),
            ));
        }
        roles.truncate(declared_arity);
        Ok(roles)
    }

    /// The contextual function type a regex replacer callback is lowered
    /// against, built from its resolved argument roles.
    ///
    /// Each role has exactly one type the spec fixes, so the callback's
    /// parameters get concrete types rather than being erased: the matched text
    /// and the subject string are `string`, the position is a `number`, and a
    /// capture group is `string | undefined` because a group that did not
    /// participate in the match is passed `undefined`.
    fn regex_replacer_callback_type(
        &mut self,
        args: &[smelt_hir::RegexReplaceArg],
    ) -> smelt_hir::TypeId {
        use smelt_hir::RegexReplaceArg;

        let string_ty = self.ctx.krate.types.intern(Type::String);
        let float_ty = self.ctx.krate.types.intern(Type::Float);
        let optional_string_ty = self.ctx.krate.types.intern(Type::Optional(string_ty));
        let params = args
            .iter()
            .map(|arg| match arg {
                RegexReplaceArg::Matched | RegexReplaceArg::Source => string_ty,
                RegexReplaceArg::Capture(_) => optional_string_ty,
                RegexReplaceArg::Position => float_ty,
            })
            .collect();
        // not a new erasure: "no return hint", so the replacer keeps the type its
        // body answers and the replacement applies `ToString` to it.
        let return_ty = self.unknown_type(NotNewErasure::NoHint);
        self.ctx.krate.types.intern(Type::Function(FunctionType {
            params,
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty,
            is_async: false,
            may_throw: false,
        }))
    }

    /// The text of a lowered string-literal expression, if it is one.
    ///
    /// The regex pattern argument is folded to a `Literal::String` by
    /// `regex_replacement_pattern` for every statically known spelling, so this
    /// is how the pattern's own text is recovered after lowering.
    fn string_literal_expr_text(body: &Body, expr: smelt_hir::ExprId) -> Option<String> {
        match body.expr_at(expr).map(|held| &held.kind) {
            Some(ExprKind::Literal(Literal::String(text))) => Some(text.clone()),
            // A regex literal's `RegExp` value: its source is the first argument.
            Some(ExprKind::New { args, .. }) => {
                args.first().and_then(|source| Self::string_literal_expr_text(body, *source))
            }
            _ => None,
        }
    }

    /// Lower supported JavaScript regular-expression replacement calls.
    pub(in crate::lowering) fn regex_replace_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let mut op = match member.property.name.as_str() {
            "replace" => StringReplaceOp::First,
            "replaceAll" => StringReplaceOp::All,
            _ => return Ok(None),
        };
        // Utility namespaces may export a collection helper named `replace`
        // with a wider signature. Defer before applying String/RegExp instance
        // arity rules so the namespace member-call path owns that invocation.
        if self.imported_utility_object(&member.object) {
            return Ok(None);
        }
        let [pattern_arg, replacement_arg] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "regex replacement requires pattern and replacement arguments",
            ));
        };
        let haystack = self.expression(&member.object, body)?;
        let Some((pattern, pattern_op)) = self.regex_replacement_pattern(pattern_arg, body)? else {
            return Ok(None);
        };
        if let Some(pattern_op) = pattern_op {
            op = pattern_op;
        }
        if matches!(
            replacement_arg,
            Argument::ArrowFunctionExpression(_) | Argument::FunctionExpression(_)
        ) {
            let declared_arity = match replacement_arg {
                Argument::ArrowFunctionExpression(arrow) => arrow.params.items.len(),
                Argument::FunctionExpression(function) => function.params.items.len(),
                _ => 1,
            };
            let arg_span = self.span(replacement_arg.span().start, replacement_arg.span().end);
            let args = Self::regex_replacer_arg_plan(pattern, declared_arity, arg_span, body)?;
            let callback_ty = self.regex_replacer_callback_type(&args);
            let callback = self.argument_with_hint(replacement_arg, body, Some(callback_ty))?;
            let callback_ty_actual = Self::expr_ty(body, callback);
            let callback_ok = self
                .function_member_type(callback_ty_actual)
                .and_then(|ty| self.ctx.krate.types.get(ty).cloned())
                .is_some_and(|ty| {
                    matches!(ty, Type::Function(function) if function.params.len() == args.len() && self.is_string_compatible_type(function.return_ty))
                });
            if !callback_ok {
                return Err(SmeltError::unsupported(
                    arg_span,
                    format!(
                        "regex replacement callback must accept a match string and return a string ({:?})",
                        self.ctx.krate.types.get(callback_ty_actual)
                    ),
                ));
            }
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::RegexReplaceCallback {
                    op,
                    pattern,
                    haystack,
                    callback,
                    args,
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let replacement = self.argument(replacement_arg, body)?;
        // A named function replacement (`.replace(re, escapeString)`) is a
        // callback replacement exactly like an inline arrow; dispatch it
        // through the same callback op when its shape fits.
        if let Some(function_ty) = self.function_member_type(Self::expr_ty(body, replacement))
            && let Some(Type::Function(function)) = self.ctx.krate.types.get(function_ty).cloned()
            && function.required_params.unwrap_or(function.params.len()) <= 1
            && !function.params.is_empty()
            && self.is_string_compatible_type(function.return_ty)
        {
            // A named replacer is called with the same spec argument list as an
            // inline one, and Rust has no optional parameters: the emitted
            // closure must supply one argument per DECLARED parameter, not one
            // per required parameter.
            let arg_span = self.span(replacement_arg.span().start, replacement_arg.span().end);
            let args =
                Self::regex_replacer_arg_plan(pattern, function.params.len(), arg_span, body)?;
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::RegexReplaceCallback {
                    op,
                    pattern,
                    haystack,
                    callback: replacement,
                    args,
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if !(self.is_string_compatible_type(Self::expr_ty(body, haystack))
            || self.type_contains_unknown(Self::expr_ty(body, haystack)))
            || !(self.ctx.krate.types.get(Self::expr_ty(body, pattern)) == Some(&Type::String)
                || self.is_regexp_class_type(Self::expr_ty(body, pattern)))
            || self.ctx.krate.types.get(Self::expr_ty(body, replacement)) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "regex replacement requires string-compatible receiver, pattern, and replacement",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::RegexReplace {
                op,
                pattern,
                haystack,
                replacement,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Reject flags a replacement cannot honour.
    fn require_replacement_flags(
        &self,
        flags: &str,
        span: oxc::span::Span,
    ) -> Result<(), SmeltError> {
        if flags
            .chars()
            .any(|flag| !matches!(flag, 'd' | 'g' | 'i' | 'm' | 's' | 'u' | 'v'))
        {
            return Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "regex replacement does not support the sticky `y` flag",
            ));
        }
        Ok(())
    }

    /// Extract a string pattern from regex replacement pattern forms.
    pub(in crate::lowering) fn regex_replacement_pattern(
        &mut self,
        argument: &Argument<'_>,
        body: &mut Body,
    ) -> Result<Option<(smelt_hir::ExprId, Option<StringReplaceOp>)>, SmeltError> {
        let Some(expression) = argument.as_expression() else {
            return Ok(None);
        };
        if let Some((callee, arguments, span)) = regexp_construction(expression) {
            if !callee_is_regexp(callee) {
                return Ok(None);
            }
            let [regex_pattern_arg] = arguments else {
                return Err(SmeltError::unsupported(
                    self.span(span.start, span.end),
                    "regex replacement supports RegExp(pattern) without flags",
                ));
            };
            return Ok(Some((self.argument(regex_pattern_arg, body)?, None)));
        }
        match expression {
            Expression::RegExpLiteral(literal) => {
                let flags = literal.regex.flags.to_string();
                self.require_replacement_flags(&flags, literal.span)?;
                let op = flags
                    .contains('g')
                    .then_some(StringReplaceOp::All);
                Ok(Some((self.regexp_literal_expression(literal, body)?, op)))
            }
            Expression::Identifier(identifier) => {
                let Some((pattern, flags, _ty)) =
                    self.consts.regexp(identifier.name.as_str()).cloned()
                else {
                    return Ok(None);
                };
                self.require_replacement_flags(&flags, identifier.span)?;
                let op = flags.contains('g').then_some(StringReplaceOp::All);
                let span = self.span(identifier.span.start, identifier.span.end);
                Ok(Some((self.regexp_value_expression(pattern, flags, span, body), op)))
            }
            _ => Ok(None),
        }
    }

    /// Whether a `.test(..)` receiver is a regex with NO `lastIndex` state
    /// AND no flags the pattern path would drop.
    ///
    /// Two questions, and both have to answer yes for `is_match` to be the same
    /// call:
    ///
    /// * **`g` and `y` are the stateful flags.** Both make `test` read and
    ///   advance `lastIndex`, so a regex carrying either has to run the same
    ///   search `exec` does. Their absence is what makes the call a pure
    ///   predicate.
    /// * **The remaining flags have to survive the trip.**
    ///   `regexp_pattern_expression` answers a PATTERN STRING, and
    ///   `regex_literal_pattern_text` folds a literal's `i`/`m`/`s` into it as
    ///   an inline `(?i)` group — but the `new RegExp(pattern, flags)` arm
    ///   takes only the pattern argument and drops the flags. So a literal is
    ///   eligible whatever its non-stateful flags, and a constructed regex only
    ///   when it names no flags at all. Widening that needs the constructor arm
    ///   to fold its flags first; taking the fast path without it made
    ///   `new RegExp("^a+$", "i").test("AAA")` answer `false` where Node
    ///   answers `true`.
    ///
    /// Anything else answers false, including a regex reached through a
    /// variable: its flags belong to the VALUE, not to this expression, and
    /// guessing "probably not global" would silently drop the state a `/g`
    /// regex's caller is relying on.
    fn regexp_test_is_stateless(receiver: &Expression<'_>) -> bool {
        /// The flags that give a regex `lastIndex` state.
        fn has_stateful_flag(flags: &str) -> bool {
            flags.contains('g') || flags.contains('y')
        }
        /// A constructed regex is eligible only with no flags argument at all.
        fn is_flagless_construction(arguments: &[Argument<'_>]) -> bool {
            arguments.len() == 1
        }
        match receiver {
            Expression::RegExpLiteral(literal) => {
                !has_stateful_flag(&literal.regex.flags.to_string())
            }
            _ => regexp_construction(receiver).is_some_and(|(callee, arguments, _)| {
                callee_is_regexp(callee) && is_flagless_construction(arguments)
            }),
        }
    }

    /// Lower TypeScript `new RegExp(pattern).test(text)` to a regex boolean match.
    pub(in crate::lowering) fn regexp_test_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "test" {
            return Ok(None);
        }
        let is_known_regexp_test = self.recognized_call(call) == Some(RuleId::TsRegExpTest);
        if !is_known_regexp_test && !self.could_be_regexp_test_receiver(&member.object, body) {
            return Ok(None);
        }
        let [haystack_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "RegExp.test() requires exactly one string argument",
            ));
        };
        let haystack = self.argument(haystack_argument, body)?;
        let Some(haystack) = self.regexp_text_operand(haystack, body) else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "RegExp.test() requires a string haystack",
            ));
        };
        // A STATELESS regex asks a yes/no question, and `is_match` is that
        // question. A stateful one is lowered below to `ExprKind::RegexTest`,
        // the runtime's own `test`: `test` on a `/g` or `/y` regex reads and
        // advances `lastIndex` (`/a/g` over `"aa"` answers true, true, then
        // FALSE and resets), so it has to run the same search `exec` does.
        // Without a flag Node leaves `lastIndex` at 0 and the two are
        // indistinguishable, so the stateless spelling takes the cheaper
        // `RegexIsMatch`. See `blocker-logs/regex-literal-test-erases-match.md`.
        //
        // Only a spelling whose flags are VISIBLE here can take the fast path.
        // A `RegExp` receiver reached through a variable takes `RegexTest`,
        // because its flags are a runtime property of the value and a `/g`
        // regex must stay stateful.
        if Self::regexp_test_is_stateless(&member.object) {
            let Some(pattern) = self.regexp_pattern_expression(&member.object, body, true)? else {
                return Ok(None);
            };
            let ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::RegexIsMatch {
                    op: RegexMatchOp::Search,
                    pattern,
                    haystack,
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let receiver = self.expression(&member.object, body)?;
        // A receiver typed as a union WITH a `RegExp` arm (`matcher: RegExp |
        // true` read after `matcher === true ||`, Hono's trie-router) or as the
        // erased boundary holds a RegExp OBJECT whenever `.test` is reached, so
        // it takes the same runtime `test` as a concrete receiver: the emitter
        // recovers the `SmeltRegExp` from the value (`SmeltFromUnknown`). The
        // string-pattern path below stringified the object instead (`/^x$/`)
        // and compiled THAT text as the pattern, so the test never matched.
        let receiver_ty = Self::expr_ty(body, receiver);
        let receiver_may_hold_regexp = match self.ctx.krate.types.get(receiver_ty) {
            Some(Type::Unknown) => true,
            Some(Type::Union(items)) => items.iter().any(|item| self.is_regexp_class_type(*item)),
            _ => false,
        };
        if receiver_may_hold_regexp || self.is_regexp_class_type(receiver_ty) {
            // A concrete receiver whose flags are NOT readable here — a regex
            // held in a variable, or built with a computed flags string. Its
            // `test` may be stateful, so it runs the runtime's own `test`,
            // which performs the same search `exec` does and updates
            // `lastIndex` with it.
            //
            // This used to be `exec(..) != null`, which built a match object
            // and erased it to `SmeltUnknown` just to compare against `null`:
            // two avoidable erasures per call site for an answer the runtime
            // already had as a `bool`.
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::RegexTest {
                    regex: receiver,
                    haystack,
                },
                ty: bool_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let Some(pattern) =
            self.regexp_pattern_expression(&member.object, body, is_known_regexp_test)?
        else {
            return Ok(None);
        };
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::RegexIsMatch {
                op: RegexMatchOp::Search,
                pattern,
                haystack,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return whether an untagged `.test(...)` receiver is plausibly a `RegExp` value.
    ///
    /// Many validation libraries expose their own `.test(...)` APIs. Smelt only
    /// routes untagged calls into regex lowering when the receiver is a direct
    /// regex-producing expression or a local/module value with a regex-like type
    /// surface.
    ///
    /// Any other receiver whose STATIC type is a `RegExp` qualifies too
    /// ([`Self::static_receiver_type`]): `build(path).test(text)`, where `build`
    /// is a module function declared to return `RegExp`, is the same runtime
    /// `test` as `const re = build(path); re.test(text)`. It used to fall through
    /// to a generic member call whose callee was the regex VALUE itself, so the
    /// call answered `null` (Hono's `RegExpRouter` never matched a wildcard
    /// middleware: `findMiddleware` asks `buildWildcardRegExp(k).test(path)`).
    fn could_be_regexp_test_receiver(&mut self, expression: &Expression<'_>, body: &Body) -> bool {
        let syntactic = match expression {
            Expression::NewExpression(_) | Expression::CallExpression(_) => {
                regexp_construction(expression).is_some_and(|(callee, _, _)| callee_is_regexp(callee))
            }
            Expression::RegExpLiteral(_) => true,
            Expression::Identifier(identifier) => self
                .scope
                .lookup(identifier.name.as_str())
                .and_then(|local| {
                    body.local_at(local)
                        .map(|decl| decl.ty)
                })
                .or_else(|| self.module_globals.get(identifier.name.as_str()).copied())
                .is_some_and(|ty| self.regexp_receiver_type(ty)),
            Expression::StaticMemberExpression(member) => {
                let Expression::Identifier(object) = &member.object else {
                    return false;
                };
                self.consts.object(object.name.as_str())
                    .and_then(|object_const| {
                        object_const
                            .entries
                            .iter()
                            .find(|entry| entry.key == member.property.name.as_str())
                    })
                    .is_some_and(|entry| self.regexp_receiver_type(entry.value_ty))
            }
            _ => false,
        };
        syntactic
            || self
                .static_receiver_type(expression, body)
                .is_some_and(|ty| self.is_regexp_class_type(ty))
    }

    /// Return whether a HIR type can represent a regex value in TS lowering.
    fn regexp_receiver_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
        {
            Some(Type::String) => true,
            Some(Type::Class { .. }) => self.is_regexp_class_type(ty),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.regexp_receiver_type(item)),
            _ => false,
        }
    }

    /// Whether `ty` (through a type parameter's constraint) is the `RegExp`
    /// class. Its name is interned in either the symbol table or the name
    /// table depending on how it was spelled (`RegExp` annotation vs a regex
    /// value), so both are consulted — the receiver gate and the post-lowering
    /// check of [`Self::regexp_test_call`] must agree on this.
    fn is_regexp_class_type(&self, ty: smelt_hir::TypeId) -> bool {
        self.stdlib_class_of_type(self.type_param_constraint_or_self(ty)) == Some(smelt_stdlib::StdlibClass::RegExp)
    }

    /// Lower TypeScript `pattern.exec(text)` to an optional match array.
    pub(in crate::lowering) fn regexp_exec_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "exec" {
            return Ok(None);
        }
        let [haystack_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "RegExp.exec() requires exactly one string argument",
            ));
        };
        let regex = self.expression(&member.object, body)?;
        let haystack = self.argument(haystack_argument, body)?;
        let Some(haystack) = self.regexp_text_operand(haystack, body) else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "RegExp.exec() requires a string haystack",
            ));
        };
        // The result is a concrete match value (or `null`); consumer reads
        // (`m[0]`, `m.index`, `m.input`, `m.groups.name`) resolve to typed
        // accessors on the generated `SmeltMatch` type instead of the erased
        // `SmeltUnknown` boundary.
        let match_ty = self.match_result_type();
        let ty = self.ctx.krate.types.intern(Type::Optional(match_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::RegexExec { regex, haystack },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower TypeScript `text.match(pattern)` to an optional match array.
    pub(in crate::lowering) fn string_match_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "match" {
            return Ok(None);
        }
        let haystack = self.expression(&member.object, body)?;
        let haystack_ty = Self::expr_ty(body, haystack);
        // Disambiguate `String.prototype.match` from unrelated `.match` methods
        // (e.g. neverthrow's `result.match(okFn, errFn)`) by call shape, so both
        // behaviors survive. A definitely-string receiver is always String.match.
        // An erased/string-compatible-but-not-definite receiver is only String.match
        // when the call shape is unambiguously a regex search: exactly one argument
        // whose type is RegExp / a regex literal / a string pattern. Any other shape
        // (wrong arity, function-typed callbacks) returns `Ok(None)` so the call
        // routes to dynamic member dispatch.
        let receiver_is_string = self.string_match_receiver_type_is_string(haystack_ty);
        if !receiver_is_string && !self.is_string_compatible_type(haystack_ty) {
            return Ok(None);
        }
        let [pattern_argument] = call.arguments.as_slice() else {
            if receiver_is_string {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "String.match() requires exactly one RegExp argument",
                ));
            }
            return Ok(None);
        };
        let pattern = self.argument(pattern_argument, body)?;
        let pattern_ty = Self::expr_ty(body, pattern);
        let static_flags = self.static_regexp_flags(pattern, body);
        // Erased receivers only lower to String.match for regex-shaped arguments;
        // a function-typed argument (neverthrow single-callback style) stays dynamic.
        if !(receiver_is_string
            || self.regexp_receiver_type(pattern_ty)
            || self.type_contains_unknown(pattern_ty))
        {
            return Ok(None);
        }
        let pattern = if self.is_string_compatible_type(pattern_ty) {
            pattern
        } else if self.ctx.krate.types.get(pattern_ty) == Some(&Type::Unknown)
            || self.type_contains_unknown(pattern_ty)
        {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: pattern,
                    target: string_ty,
                },
                ty: string_ty,
                span: self.span(pattern_argument.span().start, pattern_argument.span().end),
            })
        } else {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: pattern },
                ty: string_ty,
                span: self.span(pattern_argument.span().start, pattern_argument.span().end),
            })
        };
        let ty = self.string_match_result_type(static_flags);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::RegexFind { pattern, haystack },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// The result type of `String.prototype.match` given the regex's static flags.
    ///
    /// ECMAScript `RegExp.prototype[@@match]` answers two different shapes:
    ///
    /// * a GLOBAL regex yields the plain array of every whole match (no
    ///   groups, no `index`/`input`), so a regex whose flags are fixed by
    ///   its expression and include `g` is typed `string[] | null` exactly;
    /// * otherwise the answer IS `RegExpBuiltinExec`, the same match value
    ///   `re.exec` returns, so it carries the same concrete type: a numbered
    ///   group that did not participate reads `undefined`, not `""`.
    ///
    /// A regex whose flags are only known at run time takes the match type;
    /// the runtime then builds a global answer as a match value whose
    /// numbered entries are the whole matches (`SmeltRegExp::match_string`).
    pub(in crate::lowering) fn string_match_result_type(
        &mut self,
        static_flags: Option<String>,
    ) -> smelt_hir::TypeId {
        if static_flags.is_some_and(|flags| flags.contains('g')) {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            let list_ty = self.ctx.krate.types.intern(Type::List(string_ty));
            self.ctx.krate.types.intern(Type::Optional(list_ty))
        } else {
            let match_ty = self.match_result_type();
            self.ctx.krate.types.intern(Type::Optional(match_ty))
        }
    }

    /// Return the flags of a lowered regex operand when its expression fixes them.
    ///
    /// A regex literal and a module-level regex constant both lower to
    /// `new RegExp(pattern, "<flags>")`, a `RegExp(pattern)` construction has
    /// no flags, and a STRING pattern (`text.match("x")`) is converted by
    /// `String.prototype.match` into a flagless `new RegExp(x)`. Any other
    /// operand (a parameter, a local, a field) carries its flags in the VALUE,
    /// so the answer is `None` rather than a guess.
    pub(in crate::lowering) fn static_regexp_flags(&self, pattern: smelt_hir::ExprId, body: &Body) -> Option<String> {
        let expr = body.expr_at(pattern)?;
        match &expr.kind {
            ExprKind::New { class, args }
                if self.stdlib_class_of_symbol(*class) == Some(smelt_stdlib::StdlibClass::RegExp) =>
            {
                match args.as_slice() {
                    [_] => Some(String::new()),
                    [_, flags] => match &body.expr_at(*flags)?.kind {
                        ExprKind::Literal(Literal::String(flags)) => Some(flags.clone()),
                        _ => None,
                    },
                    _ => None,
                }
            }
            _ if self.ctx.krate.types.get(expr.ty) == Some(&Type::String) => Some(String::new()),
            _ => None,
        }
    }

    /// The type of a regex match value's ARRAY view: `(string | undefined)[]`.
    ///
    /// Entry 0 is the whole match; a numbered group that did not participate
    /// is `undefined`, exactly as JavaScript stores it in the match array.
    pub(in crate::lowering) fn match_array_view_type(&mut self) -> smelt_hir::TypeId {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let group_ty = self.ctx.krate.types.intern(Type::Optional(string_ty));
        self.ctx.krate.types.intern(Type::List(group_ty))
    }

    /// View a regex match value as the JavaScript array it is.
    ///
    /// A match result (`re.exec(s)`, `s.match(re)`) is an `Array` carrying
    /// `index`, `input` and `groups` as extra properties. Smelt keeps it typed
    /// as the concrete match class so those properties stay typed, and converts
    /// it to its array view — `(string | undefined)[]`, see
    /// [`Self::match_array_view_type`] — wherever it is USED as an array:
    /// array-method receivers, `for...of`, and spread. The conversion is an
    /// explicit `TypeAssert` whose backend coercion reads the numbered groups
    /// out of the match, so every list rule then applies unchanged.
    ///
    /// Returns `None` when `value` is not a match value (the named-groups
    /// record is not an array and is left alone).
    pub(in crate::lowering) fn match_array_view(
        &mut self,
        value: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(Self::expr_ty(body, value))
        else {
            return None;
        };
        if self.match_stdlib_class(*name) != Some(smelt_stdlib::StdlibClass::Match) {
            return None;
        }
        let ty = self.match_array_view_type();
        Some(body.push_expr(Expr {
            kind: ExprKind::TypeAssert { value },
            ty,
            span,
        }))
    }

    /// Return whether every possible receiver variant is definitely a string.
    fn string_match_receiver_type_is_string(&self, ty: smelt_hir::TypeId) -> bool {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
        {
            Some(Type::String) => true,
            Some(Type::Union(items)) => {
                !items.is_empty()
                    && items
                        .iter()
                        .copied()
                        .all(|item| self.string_match_receiver_type_is_string(item))
            }
            _ => false,
        }
    }

    /// Lower a `new RegExp(...)` construction to a RegExp-typed value, using `New` for a
    /// literal pattern and flags and `RegExpCompile` otherwise.
    pub(in crate::lowering) fn regexp_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        // `new RegExp()` (zero arguments) is legal JavaScript for the empty
        // pattern `/(?:)/`, which es-toolkit's `merge` spec constructs as a
        // representative non-plain-object source value. Model it as an empty
        // pattern string so it lowers exactly like `new RegExp('')`.
        let pattern = match new_expr.arguments.as_slice() {
            [] => body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(String::new())),
                ty: self.ctx.krate.types.intern(Type::String),
                span: self.span(new_expr.span.start, new_expr.span.end),
            }),
            [pattern_argument] | [pattern_argument, _] => {
                let pattern = self.argument(pattern_argument, body)?;
                self.regexp_pattern_operand(pattern, body)?.ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        "RegExp construction requires a string pattern",
                    )
                })?
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(new_expr.span.start, new_expr.span.end),
                    "RegExp construction requires a string pattern and optional flags",
                ));
            }
        };
        let flags = if let Some(flags_argument) = new_expr.arguments.get(1) {
            let flags = self.argument(flags_argument, body)?;
            self.regexp_text_operand(flags, body).ok_or_else(|| {
                SmeltError::unsupported(
                    self.span(flags_argument.span().start, flags_argument.span().end),
                    "RegExp flags must be string-compatible",
                )
            })?
        } else {
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(String::new())),
                ty: self.ctx.krate.types.intern(Type::String),
                span: self.span(new_expr.span.start, new_expr.span.end),
            })
        };
        // A pattern and flags that are both source literals are known when the
        // program is written, exactly like a `/re/` literal, and keep the
        // infallible construction. Anything computed is compiled at RUN time
        // through the fallible builtin, so a malformed pattern throws the
        // catchable `SyntaxError` JavaScript throws at construction.
        let is_literal = |id: smelt_hir::ExprId| {
            body.expr_at(id)
                .is_some_and(|expr| matches!(expr.kind, ExprKind::Literal(Literal::String(_))))
        };
        if !is_literal(pattern) || !is_literal(flags) {
            return Ok(body.push_expr(Expr {
                kind: ExprKind::RegExpCompile { pattern, flags },
                ty: self.regexp_type(),
                span: self.span(new_expr.span.start, new_expr.span.end),
            }));
        }
        Ok(body.push_expr(Expr {
            kind: ExprKind::New {
                class: self.intern_type_name("RegExp"),
                args: vec![pattern, flags],
            },
            ty: self.regexp_type(),
            span: self.span(new_expr.span.start, new_expr.span.end),
        }))
    }

    /// Extract a pattern string from a supported TypeScript RegExp-producing expression.
    fn regexp_pattern_expression(
        &mut self,
        expression: &Expression<'_>,
        body: &mut Body,
        require_regexp_receiver: bool,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if let Some((callee, arguments, span)) = regexp_construction(expression) {
            if !callee_is_regexp(callee) {
                if !require_regexp_receiver {
                    return Ok(None);
                }
                return Err(SmeltError::unsupported(
                    self.span(span.start, span.end),
                    "RegExp.test() requires a RegExp receiver",
                ));
            }
            let ([pattern_argument] | [pattern_argument, _]) = arguments else {
                return Err(SmeltError::unsupported(
                    self.span(span.start, span.end),
                    "RegExp construction requires a string pattern and optional flags",
                ));
            };
            let pattern = self.argument(pattern_argument, body)?;
            return self.regexp_pattern_operand(pattern, body);
        }
        match expression {
            Expression::RegExpLiteral(literal) => Ok(Some(self.regexp_literal_expression(literal, body)?)),
            Expression::Identifier(_)
            | Expression::StaticMemberExpression(_)
            | Expression::ComputedMemberExpression(_) => {
                let pattern = self.expression(expression, body)?;
                Ok(self.regexp_text_operand(pattern, body))
            }
            _ if require_regexp_receiver => Err(SmeltError::unsupported(
                self.expression_span(expression),
                "RegExp.test() requires a RegExp receiver",
            )),
            _ => Ok(None),
        }
    }

    /// Coerce a regex text expression to the string representation used by Rust regex APIs.
    fn regexp_pattern_operand(
        &mut self,
        pattern: smelt_hir::ExprId,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        self.regexp_text_operand(pattern, body)
            .map(Some)
            .ok_or_else(|| {
                let span = body
                    .expr_at(pattern)
                    .expect("expr id should point to an existing expression")
                    .span;
                SmeltError::unsupported(span, "RegExp.test() requires a string pattern")
            })
    }

    /// Coerce a regex text expression to the string representation used by Rust regex APIs.
    fn regexp_text_operand(
        &mut self,
        pattern: smelt_hir::ExprId,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let pattern_ty = Self::expr_ty(body, pattern);
        if self.is_string_compatible_type(pattern_ty) {
            return Some(pattern);
        }
        if self.ctx.krate.types.get(pattern_ty) == Some(&Type::Unknown)
            || self.type_contains_unknown(pattern_ty)
        {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            let span = body.expr_at(pattern)?.span;
            return Some(body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: pattern,
                    target: string_ty,
                },
                ty: string_ty,
                span,
            }));
        }
        None
    }

    /// Return whether a contextual argument type should preserve a `RegExp` literal object.
    pub(in crate::lowering) fn type_accepts_regexp_literal(&self, ty: smelt_hir::TypeId) -> bool {
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
    pub(in crate::lowering) fn regexp_literal_expression(
        &mut self,
        literal: &oxc::ast::ast::RegExpLiteral<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(literal.span.start, literal.span.end);
        Ok(self.regexp_value_expression(
            Self::regex_literal_pattern_text_without_flags(literal),
            literal.regex.flags.to_string(),
            span,
            body,
        ))
    }

    /// A `new RegExp(pattern, flags)` value over a JavaScript pattern source.
    pub(in crate::lowering) fn regexp_value_expression(
        &mut self,
        pattern: String,
        flags: String,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let pattern = body.push_expr(Expr { kind: ExprKind::Literal(Literal::String(pattern)), ty: string_ty, span });
        let flags = body.push_expr(Expr { kind: ExprKind::Literal(Literal::String(flags)), ty: string_ty, span });
        body.push_expr(Expr {
            kind: ExprKind::New {
                class: self.intern_type_name("RegExp"),
                args: vec![pattern, flags],
            },
            ty: self.regexp_type(),
            span,
        })
    }
}
