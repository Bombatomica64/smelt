//! Assertion lowering: vitest `expect(..)` matchers, asymmetric matchers, node
//! `assert` calls and the HIR expressions they compare with.

use crate::lowering::body_arena::BodyArena;
use crate::lowering::{ModuleBuilder, TestMatcher};
use crate::SmeltError;
use oxc::ast::ast::{Argument, Expression};
use oxc::span::GetSpan;
use smelt_hir::{
    BinOp, Body, ErasureReason, Expr, ExprKind, FunctionType, Literal, LocalDecl, Pattern,
    PrimitiveCastOp, PropertyLookup, Span, Stmt, Type, UnaryOp, UnknownKind,
};

/// An `expect(...)` actual value that has already been lowered to HIR.
///
/// The ordinary matcher lowering recovers the actual from the syntactic
/// `expect(...)` call behind the matcher. The `.resolves` / `.rejects` chains
/// cannot: their actual is the *awaited* (or *caught*) value, which only exists
/// after the async lowering has built the `await` or the `try`/`catch` around
/// it. They pass that value in here instead, so a single matcher
/// implementation serves both spellings rather than each modifier growing its
/// own copy of every matcher.
#[derive(Clone, Copy)]
pub(in crate::lowering) struct LoweredActual {
    /// The already-lowered actual value the matcher asserts on.
    pub value: smelt_hir::ExprId,
    /// Whether a `.not` modifier appeared in the matcher chain.
    pub inverted: bool,
}

/// The vitest asymmetric matcher factories.
///
/// An asymmetric matcher is a *value*, not an assertion: `expect.any(Number)`
/// can be stored, nested inside an expected object or array, or passed as one
/// argument of `toHaveBeenCalledWith`. What makes it a matcher is that the deep
/// equality used by `toEqual`/`toStrictEqual`/`toHaveBeenCalledWith` asks it
/// whether the actual value matches, instead of comparing it structurally.
///
/// The name list is the closed vitest API surface, exactly like
/// [`TestMatcher::from_name`]'s: it is the test harness's own vocabulary, not a
/// library's spelling, so recognizing it is not a special case for user code.
const ASYMMETRIC_MATCHER_NAMES: &[&str] = &[
    "any",
    "anything",
    "arrayContaining",
    "objectContaining",
    "stringContaining",
    "stringMatching",
    "closeTo",
];

/// Marker key that identifies an erased asymmetric-matcher record at run time.
///
/// Same convention as `__smelt_vitest_mock`, `__smelt_map` and `__smelt_set`:
/// the value's *kind* travels with it as a branded field, so a runtime helper
/// recognizes it wherever the value ended up.
const ASYMMETRIC_MARKER_KEY: &str = "__smelt_asymmetric";

impl ModuleBuilder<'_> {
    /// Lower `expect.<name>(...)` / `expect.not.<name>(...)` as a matcher VALUE.
    ///
    /// Returns `None` when the call is not an asymmetric matcher, so the
    /// ordinary call path continues.
    ///
    /// The result is a marker record — `{ __smelt_asymmetric: <name>, sample:
    /// [<args>], inverted: <bool> }` — erased to the dynamic carrier, which is
    /// what a matcher's type genuinely is: vitest types these factories `any`,
    /// they are compared against a value of unrelated static type, and the
    /// deep-equality helper discriminates them by their brand at run time.
    ///
    /// Before this, `expect` in a value position fell through to the generic
    /// erased-object path and lowered to an EMPTY record, so the member read
    /// `arrayContaining` answered `undefined`, no callable was found, and the
    /// emitted fallback callback returned `null` — turning
    /// `expect(a).toEqual(expect.arrayContaining(b))` into `a == null`, which
    /// can never hold.
    ///
    /// `sample` holds ALL of the factory's arguments as one array rather than
    /// just the first, so `expect.closeTo(value, precision)` needs no second
    /// record shape.
    pub(in crate::lowering) fn vitest_asymmetric_matcher_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let name = member.property.name.as_str();
        if !ASYMMETRIC_MATCHER_NAMES.contains(&name) {
            return Ok(None);
        }
        let Some(inverted) = self.asymmetric_matcher_receiver_inverted(&member.object) else {
            return Ok(None);
        };
        let span = self.span(call.span.start, call.span.end);
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let unknown_ty = self.erase_with(ErasureReason::TestHarness, "asymmetric matcher");
        let dict_ty = self.ctx.krate.types.intern(Type::Dict(string_ty, unknown_ty));
        let list_ty = self.ctx.krate.types.intern(Type::List(unknown_ty));

        let mut sample_items = Vec::with_capacity(call.arguments.len());
        for argument in &call.arguments {
            // `expect.any(Ctor)` names a CONSTRUCTOR, and its argument is used
            // only for that identity. Smelt does not model a builtin
            // constructor as a first-class constructor object -- `Number` in a
            // value position lowers to the numeric coercion function, and
            // `String` to the string one -- so lowering the reference as a
            // value throws away the very thing the matcher needs. The
            // referenced NAME is what the runtime compares against a value's
            // own class identity (its `Symbol.toStringTag` view or its
            // `__smelt_class` marker), so a constructor REFERENCE contributes
            // its spelling. Any other expression still lowers as a value and
            // has its name read off it at run time.
            if name == "any"
                && let Some(reference) = Self::constructor_reference_text(argument)
            {
                let spelling = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(reference)),
                    ty: string_ty,
                    span,
                });
                sample_items.push(Self::erase_to_unknown(spelling, unknown_ty, span, body));
                continue;
            }
            let lowered = self.argument_with_hint(argument, body, Some(unknown_ty))?;
            sample_items.push(Self::erase_to_unknown(lowered, unknown_ty, span, body));
        }
        let sample = body.push_expr(Expr {
            kind: ExprKind::ListLit(sample_items),
            ty: list_ty,
            span,
        });
        let sample = Self::erase_to_unknown(sample, unknown_ty, span, body);
        let kind_value = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(name.to_owned())),
            ty: string_ty,
            span,
        });
        let kind_value = Self::erase_to_unknown(kind_value, unknown_ty, span, body);
        let inverted_value = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(inverted)),
            ty: bool_ty,
            span,
        });
        let inverted_value = Self::erase_to_unknown(inverted_value, unknown_ty, span, body);
        let string_key = |target: &mut Body, key: &str| {
            target.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(key.to_owned())),
                ty: string_ty,
                span,
            })
        };
        let marker_key = string_key(body, ASYMMETRIC_MARKER_KEY);
        let sample_key = string_key(body, "sample");
        let inverted_key = string_key(body, "inverted");
        let record = body.push_expr(Expr {
            kind: ExprKind::DictLit(vec![
                (marker_key, kind_value),
                (sample_key, sample),
                (inverted_key, inverted_value),
            ]),
            ty: dict_ty,
            span,
        });
        self.asymmetric_matchers_lowered = self.asymmetric_matchers_lowered.saturating_add(1);
        Ok(Some(Self::erase_to_unknown(record, unknown_ty, span, body)))
    }

    /// Build the matcher-aware deep-equality test for `toEqual`/`toStrictEqual`.
    ///
    /// Both operands are erased to the dynamic carrier first, because a matcher
    /// is a marker-bearing record and the comparison is a runtime walk that
    /// consults that marker at every level. It is deliberately NOT
    /// `SmeltUnknown`'s `PartialEq`: library code observes that operator, and
    /// the test harness's markers must stay invisible to it.
    fn vitest_asymmetric_equal_expr(
        &mut self,
        actual: smelt_hir::ExprId,
        expected: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let span = self.span(span.start, span.end);
        let unknown_ty = self.erase_with(ErasureReason::TestHarness, "asymmetric equality");
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let actual = Self::erase_to_unknown(actual, unknown_ty, span, body);
        let expected = Self::erase_to_unknown(expected, unknown_ty, span, body);
        body.push_expr(Expr {
            kind: ExprKind::VitestAsymmetricEqual { actual, expected },
            ty: bool_ty,
            span,
        })
    }

    /// Whether an expected argument writes an asymmetric matcher anywhere.
    ///
    /// Answered from the syntax, before lowering, because it decides whether the
    /// expected value may be contextually typed from the actual's type at all —
    /// a decision that has to be made before the operand is built. The walk
    /// covers the containers vitest allows a matcher to sit in (array and object
    /// literals) plus the type-only wrappers, and it deliberately does not
    /// descend into calls or other computation: a matcher reaching the assertion
    /// through a variable or a helper is caught afterwards by the lowered-matcher
    /// count, which needs no hint decision because such an operand is already
    /// erased.
    fn argument_holds_asymmetric_matcher(&self, argument: &Argument<'_>) -> bool {
        argument
            .as_expression()
            .is_some_and(|expression| self.expression_holds_asymmetric_matcher(expression))
    }

    /// The recursive half of [`Self::argument_holds_asymmetric_matcher`].
    fn expression_holds_asymmetric_matcher(&self, expression: &Expression<'_>) -> bool {
        match expression {
            Expression::CallExpression(call) => {
                let Expression::StaticMemberExpression(member) = &call.callee else {
                    return false;
                };
                ASYMMETRIC_MATCHER_NAMES.contains(&member.property.name.as_str())
                    && self
                        .asymmetric_matcher_receiver_inverted(&member.object)
                        .is_some()
            }
            Expression::ArrayExpression(array) => array.elements.iter().any(|element| {
                element
                    .as_expression()
                    .is_some_and(|value| self.expression_holds_asymmetric_matcher(value))
            }),
            Expression::ObjectExpression(object) => object.properties.iter().any(|property| {
                matches!(property, oxc::ast::ast::ObjectPropertyKind::ObjectProperty(entry)
                    if self.expression_holds_asymmetric_matcher(&entry.value))
            }),
            Expression::ParenthesizedExpression(parenthesized) => {
                self.expression_holds_asymmetric_matcher(&parenthesized.expression)
            }
            Expression::TSAsExpression(as_expr) => {
                self.expression_holds_asymmetric_matcher(&as_expr.expression)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                self.expression_holds_asymmetric_matcher(&satisfies.expression)
            }
            Expression::TSNonNullExpression(non_null) => {
                self.expression_holds_asymmetric_matcher(&non_null.expression)
            }
            _ => false,
        }
    }

    /// The dotted spelling of a constructor REFERENCE argument, if it is one.
    ///
    /// `Number`, `String`, `MyClass`, `Intl.Locale` — a bare name or a
    /// namespaced one. Anything else (a call, a literal, an arbitrary
    /// expression) is `None` and lowers as an ordinary value.
    fn constructor_reference_text(argument: &Argument<'_>) -> Option<String> {
        /// Returns the dotted source text of a bare or namespaced reference, or `None`.
        fn reference_text(expression: &Expression<'_>) -> Option<String> {
            match expression {
                Expression::Identifier(ident) => Some(ident.name.as_str().to_owned()),
                Expression::StaticMemberExpression(member) => {
                    let object = reference_text(&member.object)?;
                    Some(format!("{object}.{}", member.property.name))
                }
                _ => None,
            }
        }
        reference_text(argument.as_expression()?)
    }

    /// Return whether a matcher receiver is `expect` (`Some(false)`) or
    /// `expect.not` (`Some(true)`), and `None` when it is neither.
    fn asymmetric_matcher_receiver_inverted(&self, receiver: &Expression<'_>) -> Option<bool> {
        let is_expect = |ident: &oxc::ast::ast::IdentifierReference<'_>| {
            ident.name == "expect" && self.imports.is_test_builtin("expect")
        };
        match receiver {
            Expression::Identifier(ident) => is_expect(ident).then_some(false),
            Expression::StaticMemberExpression(member) if member.property.name == "not" => {
                match &member.object {
                    Expression::Identifier(ident) => is_expect(ident).then_some(true),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Lower a Node `assert.deepStrictEqual` call statement when one is present.
    pub(in crate::lowering) fn deep_strict_equal_statement(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<bool, SmeltError> {
        let is_deep_strict_equal = match &call.callee {
            Expression::Identifier(ident) => ident.name == "deepStrictEqual",
            Expression::StaticMemberExpression(member)
                if member.property.name == "deepStrictEqual" =>
            {
                matches!(
                    &member.object,
                    Expression::Identifier(object) if object.name == "U" || object.name == "assert"
                )
            }
            _ => false,
        };
        if !is_deep_strict_equal {
            return Ok(false);
        }
        let [actual_arg, expected_arg] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "deepStrictEqual(...) requires actual and expected values",
            ));
        };
        let actual = self.argument(actual_arg, body)?;
        let expected = self.argument(expected_arg, body)?;
        let failed = self.comparison_expr(BinOp::NotEq, actual, expected, call.span, body);
        self.push_test_failure_if(failed, "deepStrictEqual(...) failed", call.span, body);
        Ok(true)
    }

    /// Lower supported Node `assert` equality calls into runtime assertions.
    pub(in crate::lowering) fn node_assert_statement(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<bool, SmeltError> {
        if self.deep_strict_equal_statement(call, body)? {
            return Ok(true);
        }
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(false);
        };
        if member.property.name != "strictEqual" {
            return Ok(false);
        }
        let Expression::Identifier(object) = &member.object else {
            return Ok(false);
        };
        if object.name != "assert" {
            return Ok(false);
        }
        let [actual_arg, expected_arg] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "assert.strictEqual(...) requires actual and expected values",
            ));
        };
        let actual = self.argument(actual_arg, body)?;
        let expected = self.argument(expected_arg, body)?;
        let failed = self.comparison_expr(BinOp::NotEq, actual, expected, call.span, body);
        self.push_test_failure_if(failed, "assert.strictEqual(...) failed", call.span, body);
        Ok(true)
    }

    /// Lower supported `expect(actual).matcher(expected)` calls to failure paths.
    pub(in crate::lowering) fn expect_matcher_statement(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<bool, SmeltError> {
        self.expect_matcher_call(call, None, body)
    }

    /// Return whether the ordinary matcher lowering can assert `name` against an
    /// already-lowered actual value.
    ///
    /// `.resolves` / `.rejects` delegate to [`Self::expect_matcher_call`] with a
    /// [`LoweredActual`], and must know *before* building the surrounding
    /// `await` / `try`-`catch` whether the delegation will be taken; otherwise a
    /// half-built assertion would be stranded in the body when it is not.
    /// `toThrow` is excluded because its actual is a callback to invoke, not a
    /// value to compare, so an awaited actual cannot stand in for it.
    pub(in crate::lowering) fn matcher_accepts_lowered_actual(name: &str) -> bool {
        let name = Self::canonical_matcher_name(name);
        matches!(
            name,
            "toBeUndefined"
                | "toBeNull"
                | "toBeDefined"
                | "toBeTruthy"
                | "toBeFalsy"
                | "toHaveBeenCalled"
                | "toHaveBeenCalledOnce"
                | "toHaveBeenCalledTimes"
                | "toHaveBeenCalledWith"
                | "toHaveBeenNthCalledWith"
                | "toHaveBeenLastCalledWith"
                | "toHaveLastResolvedWith"
                | "toMatchObject"
        ) || TestMatcher::from_name(name).is_some()
    }

    /// Resolve a matcher spelling to the canonical one the lowering implements.
    ///
    /// vitest exposes the jest spellings as aliases of its own matchers: they
    /// are the SAME assertion under a second name. Resolving the name in one
    /// table — rather than growing a second implementation per alias — is what
    /// keeps the alias a vocabulary entry instead of a special case. An unknown
    /// name is returned unchanged, so a matcher the model does not have still
    /// reports itself by the spelling the source used.
    pub(in crate::lowering) fn canonical_matcher_name(name: &str) -> &str {
        match name {
            "toThrowError" => "toThrow",
            "toBeCalled" => "toHaveBeenCalled",
            "toBeCalledWith" => "toHaveBeenCalledWith",
            "toBeCalledTimes" => "toHaveBeenCalledTimes",
            "toBeCalledOnce" => "toHaveBeenCalledOnce",
            "lastCalledWith" => "toHaveBeenLastCalledWith",
            "nthCalledWith" => "toHaveBeenNthCalledWith",
            other => other,
        }
    }

    /// Lower a Vitest `expect(...).matcher(...)` call to HIR assertion statements.
    ///
    /// `actual_override` supplies the actual value for matcher chains whose
    /// receiver is not a syntactic `expect(...)` call — see [`LoweredActual`].
    /// When it is `None` the actual is recovered from the source `expect(...)`
    /// call exactly as before.
    pub(in crate::lowering) fn expect_matcher_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        actual_override: Option<LoweredActual>,
        body: &mut Body,
    ) -> Result<bool, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(false);
        };
        // Every branch below asks about the CANONICAL spelling, so a jest alias
        // reaches the same implementation as the vitest name it aliases.
        let canonical = Self::canonical_matcher_name(member.property.name.as_str());
        if matches!(
            canonical,
            "toThrow" | "toThrowErrorMatchingInlineSnapshot"
        ) {
            // `toThrow` asserts on a callback, so a pre-lowered value cannot
            // stand in for it; `matcher_accepts_lowered_actual` keeps callers
            // from reaching here with one.
            if actual_override.is_some() {
                return Ok(false);
            }
            return self.expect_to_throw_statement(call, member, body);
        }
        if matches!(canonical, "toBeUndefined" | "toBeNull" | "toBeDefined") {
            return self.expect_to_be_none_statement(call, member, actual_override, body, canonical);
        }
        if matches!(canonical, "toBeTruthy" | "toBeFalsy") {
            return self.expect_truthiness_statement(call, member, actual_override, body, canonical);
        }
        if canonical == "toMatchObject" {
            return self.expect_match_object_statement(call, member, actual_override, body);
        }
        if canonical == "toHaveBeenNthCalledWith" {
            return self.expect_mock_nth_called_with_statement(call, member, actual_override, body);
        }
        if matches!(
            canonical,
            "toHaveBeenCalled"
                | "toHaveBeenCalledOnce"
                | "toHaveBeenCalledTimes"
                | "toHaveBeenCalledWith"
                | "toHaveBeenLastCalledWith"
                | "toHaveLastResolvedWith"
        ) {
            let matcher_name = canonical;
            let (actual, inverted) = if let Some(actual) = actual_override {
                (actual.value, actual.inverted)
            } else {
                let (expect_call, inverted) =
                    self.expect_call_from_matcher_object(&member.object)?;
                let Expression::Identifier(expect_ident) = &expect_call.callee else {
                    return Ok(false);
                };
                if !self.imports.is_test_builtin(expect_ident.name.as_str())
                    || expect_ident.name.as_str() != "expect"
                {
                    return Ok(false);
                }
                let actual_arg = expect_call.arguments.first().ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(expect_call.span.start, expect_call.span.end),
                        format!("expect(...).{matcher_name}(...) requires an actual value"),
                    )
                })?;
                (self.argument(actual_arg, body)?, inverted)
            };
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            // Both matchers read the live mock state behind the erased actual
            // (`__smelt_vitest_mock` marker). A value that is not a mock has
            // recorded no calls, so the runtime helpers answer FALSE for it —
            // `vi.spyOn` produces a real mock now, and the vacuous pass the
            // placeholder era needed would only hide a broken assertion.
            let matched = if matches!(matcher_name, "toHaveBeenCalled" | "toHaveBeenCalledOnce") {
                // Both are call-count assertions: "called at least once" is the
                // negation of "called exactly zero times", and `toHaveBeenCalledOnce`
                // is `toHaveBeenCalledTimes(1)`. Reusing the one runtime helper
                // keeps a single definition of what a recorded call is.
                if !call.arguments.is_empty() {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        format!("expect(...).{matcher_name}() does not take arguments"),
                    ));
                }
                let float_ty = self.ctx.krate.types.intern(Type::Float);
                let once = matcher_name == "toHaveBeenCalledOnce";
                let count = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Float(if once { 1.0 } else { 0.0 })),
                    ty: float_ty,
                    span: self.span(call.span.start, call.span.end),
                });
                let called_times = body.push_expr(Expr {
                    kind: ExprKind::VitestMockCalledTimes {
                        mock: actual,
                        count,
                    },
                    ty: bool_ty,
                    span: self.span(call.span.start, call.span.end),
                });
                if once {
                    called_times
                } else {
                    self.unary_bool_expr(UnaryOp::Not, called_times, call.span, body)
                }
            } else if matcher_name == "toHaveBeenCalledTimes" {
                let count_arg = call.arguments.first().ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "expect(...).toHaveBeenCalledTimes(...) requires an expected call count",
                    )
                })?;
                let count = self.argument(count_arg, body)?;
                body.push_expr(Expr {
                    kind: ExprKind::VitestMockCalledTimes {
                        mock: actual,
                        count,
                    },
                    ty: bool_ty,
                    span: self.span(call.span.start, call.span.end),
                })
            } else if matcher_name == "toHaveLastResolvedWith" {
                let expected_arg = call.arguments.first().ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "expect(...).toHaveLastResolvedWith(...) requires an expected value",
                    )
                })?;
                let expected = self.argument(expected_arg, body)?;
                body.push_expr(Expr {
                    kind: ExprKind::VitestMockLastResolvedWith {
                        mock: actual,
                        expected,
                    },
                    ty: bool_ty,
                    span: self.span(call.span.start, call.span.end),
                })
            } else {
                let last = matcher_name == "toHaveBeenLastCalledWith";
                let args = call
                    .arguments
                    .iter()
                    .map(|arg| self.argument(arg, body))
                    .collect::<Result<Vec<_>, _>>()?;
                body.push_expr(Expr {
                    kind: ExprKind::VitestMockCalledWith {
                        mock: actual,
                        args,
                        last,
                    },
                    ty: bool_ty,
                    span: self.span(call.span.start, call.span.end),
                })
            };
            let failed = if inverted {
                matched
            } else {
                self.unary_bool_expr(UnaryOp::Not, matched, call.span, body)
            };
            self.push_test_failure_if(
                failed,
                &format!("expect(...).{matcher_name}(...) failed"),
                call.span,
                body,
            );
            return Ok(true);
        }
        let Some(matcher) = TestMatcher::from_name(canonical) else {
            return Ok(false);
        };
        let mut pending_actual_arg = None;
        let inverted = if let Some(actual) = actual_override {
            actual.inverted
        } else {
            let (expect_call, inverted) =
                self.expect_call_from_matcher_object(&member.object)?;
            let Expression::Identifier(expect_ident) = &expect_call.callee else {
                return Ok(false);
            };
            if !self.imports.is_test_builtin(expect_ident.name.as_str())
                || expect_ident.name.as_str() != "expect"
            {
                return Ok(false);
            }
            pending_actual_arg = Some(expect_call.arguments.first().ok_or_else(|| {
                SmeltError::unsupported(
                    self.span(expect_call.span.start, expect_call.span.end),
                    format!(
                        "expect(...).{}(...) requires an actual value",
                        matcher.source_name()
                    ),
                )
            })?);
            inverted
        };
        let expected_arg = call.arguments.first().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!(
                    "expect(...).{}(...) requires an expected value",
                    matcher.source_name()
                ),
            )
        })?;
        let matchers_before_operands = self.asymmetric_matchers_lowered;
        let actual = match (actual_override, pending_actual_arg) {
            (Some(actual), _) => actual.value,
            (None, Some(actual_arg)) => self.argument(actual_arg, body)?,
            (None, None) => return Ok(false),
        };
        // `toBeInstanceOf` asks what `actual instanceof Ctor` asks; the constructor
        // is a name to resolve, not a value to compare against.
        if matcher == TestMatcher::BeInstanceOf {
            let target = expected_arg.as_expression().ok_or_else(|| {
                SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "expect(...).toBeInstanceOf(...) requires a constructor",
                )
            })?;
            let holds = self.instanceof_test(actual, call.span, target, call.span, body)?;
            let failed = if inverted {
                holds
            } else {
                self.unary_bool_expr(UnaryOp::Not, holds, call.span, body)
            };
            self.push_test_failure_if(
                failed,
                &format!("expect(...).{}(...) failed", matcher.source_name()),
                call.span,
                body,
            );
            return Ok(true);
        }
        // Contextually type the expected value from the actual's type for
        // deep-equality matchers. An expected literal such as
        // `[[1, 'a'], [2, 'b']]` in `expect(zip(...)).toEqual([...])` would
        // otherwise infer as `SmeltList<SmeltList<SmeltUnknown>>` and fail to
        // compare against the actual's `SmeltList<(f64, String)>` (E0308).
        // `array_expression`'s arity guard ignores tuple hints whose arity does
        // not match the literal, so ragged expected values are unaffected.
        //
        // An asymmetric matcher is the one thing that hint must not reach. A
        // matcher's type is the dynamic carrier, so contextually typing
        // `{ tags: expect.arrayContaining(['b']) }` from an actual
        // `{ tags: string[] }` asks the erasure to turn the matcher record into
        // a `SmeltList<String>` — which fails at run time ("unknown is not
        // iterable"). Where a matcher is written, the expected value keeps its
        // own inferred type and the comparison is the matcher-aware walk, which
        // works on the erased carrier anyway.
        let expected_has_matcher = self.argument_holds_asymmetric_matcher(expected_arg);
        let expected = if matches!(matcher, TestMatcher::Equal | TestMatcher::StrictEqual)
            && !expected_has_matcher
        {
            let actual_ty = Self::expr_ty(body, actual);
            self.argument_with_hint(expected_arg, body, Some(actual_ty))?
        } else {
            self.argument(expected_arg, body)?
        };
        // An asymmetric matcher anywhere in either operand changes what the
        // comparison MEANS: `expect(a).toEqual(expect.arrayContaining(b))` asks
        // the matcher, and `expect(o).toEqual({ id: expect.any(Number) })` asks
        // it for one nested field. Counting the matchers lowered while building
        // the two operands answers "is one in there" exactly, at any nesting
        // depth, without re-walking the argument syntax.
        let uses_asymmetric_matcher = (expected_has_matcher
            || self.asymmetric_matchers_lowered != matchers_before_operands)
            && matches!(matcher, TestMatcher::Equal | TestMatcher::StrictEqual);
        // `expect(x)` and every equality matcher take `any`, so a test may
        // compare values whose STATIC types are unrelated — a mocked KV store
        // returns a string where the declaration says `ReadableStream | null`,
        // and the test asserts `toBe('This is index')`. Neither side is
        // assignable to the other, so there is no typed comparison to emit
        // (`Option<SmeltBody> == Option<String>` was E0308). JavaScript compares
        // the runtime VALUES, which is exactly the erased comparison: both
        // operands cross the `unknown` boundary and the matcher's runtime
        // equality decides.
        let (actual, expected) = if matches!(
            matcher,
            TestMatcher::Be | TestMatcher::Equal | TestMatcher::StrictEqual
        ) && !uses_asymmetric_matcher
        {
            self.erase_unrelated_matcher_operands(actual, expected, call.span, body)
        } else {
            (actual, expected)
        };
        // Vitest compares primitive numbers with `Object.is` under every
        // equality matcher, not just `toBe`. Only `toBe` additionally treats
        // objects and arrays by reference, so the identity rule stays gated on
        // it while the numeric rule applies to the deep matchers too.
        let use_strict_identity = !uses_asymmetric_matcher
            && match matcher {
            TestMatcher::Be => self.test_to_be_needs_strict_identity(actual, expected, body),
            TestMatcher::Equal | TestMatcher::StrictEqual => {
                self.test_numbers_need_same_value(actual, expected, body)
            }
            _ => false,
        };
        let mut failed = if uses_asymmetric_matcher {
            let holds = self.vitest_asymmetric_equal_expr(actual, expected, call.span, body);
            self.unary_bool_expr(UnaryOp::Not, holds, call.span, body)
        } else if use_strict_identity {
            let op = if inverted {
                BinOp::StrictEq
            } else {
                BinOp::StrictNotEq
            };
            self.comparison_expr(op, actual, expected, call.span, body)
        } else {
            self.expect_matcher_failure_expr(matcher, actual, expected, call.span, body)?
        };
        if use_strict_identity {
            self.push_test_failure_if(
                failed,
                &format!("expect(...).{}(...) failed", matcher.source_name()),
                call.span,
                body,
            );
            return Ok(true);
        }
        if inverted {
            failed = self.unary_bool_expr(UnaryOp::Not, failed, call.span, body);
        }
        if let Some(rejected) = self.nullish_actual_rejected_expr(matcher, actual, call.span, body)
        {
            failed = self.bool_or_expr(rejected, failed, call.span, body);
        }
        self.push_test_failure_if(
            failed,
            &format!("expect(...).{}(...) failed", matcher.source_name()),
            call.span,
            body,
        );
        Ok(true)
    }

    /// The condition under which a matcher rejects its actual outright.
    ///
    /// `toContain` and `toMatch` do not answer "no" for a `null`/`undefined`
    /// actual — vitest throws (chai's `include` has no container to search, and
    /// `toMatch` requires a string), so the assertion fails under `.not` too.
    /// That rejection is independent of the matcher's own result, which is why
    /// it is OR-ed onto the failure condition AFTER the `.not` inversion.
    /// Only an actual that can be nullish produces one: an optional actual is
    /// tested against `None`, an erased one against its runtime null/undefined
    /// tags; every other matcher and actual type returns `None`.
    fn nullish_actual_rejected_expr(
        &mut self,
        matcher: TestMatcher,
        actual: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        if !matches!(matcher, TestMatcher::Contain | TestMatcher::Match) {
            return None;
        }
        let hir_span = self.span(span.start, span.end);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        match self.ctx.krate.types.get(Self::expr_ty(body, actual)) {
            Some(Type::Optional(_)) => {
                let none_ty = self.ctx.krate.types.intern(Type::None);
                let none = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty: none_ty,
                    span: hir_span,
                });
                Some(body.push_expr(Expr {
                    kind: ExprKind::BinOp {
                        op: BinOp::Eq,
                        lhs: actual,
                        rhs: none,
                    },
                    ty: bool_ty,
                    span: hir_span,
                }))
            }
            Some(Type::Unknown) => {
                let is_null = body.push_expr(Expr {
                    kind: ExprKind::UnknownIs {
                        value: actual,
                        kind: UnknownKind::Null,
                    },
                    ty: bool_ty,
                    span: hir_span,
                });
                let is_undefined = body.push_expr(Expr {
                    kind: ExprKind::UnknownIs {
                        value: actual,
                        kind: UnknownKind::Undefined,
                    },
                    ty: bool_ty,
                    span: hir_span,
                });
                Some(self.bool_or_expr(is_null, is_undefined, span, body))
            }
            _ => None,
        }
    }

    /// Short-circuiting `lhs || rhs` over two boolean expressions.
    ///
    /// Built as a `Conditional` so `rhs` is only evaluated when `lhs` is
    /// false, exactly as JavaScript `||` does.
    fn bool_or_expr(
        &mut self,
        lhs: smelt_hir::ExprId,
        rhs: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let hir_span = self.span(span.start, span.end);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let yes = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(true)),
            ty: bool_ty,
            span: hir_span,
        });
        body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond: lhs,
                then_expr: yes,
                else_expr: rhs,
            },
            ty: bool_ty,
            span: hir_span,
        })
    }

    /// Erase both operands of an equality matcher whose static types are
    /// unrelated (neither assignable to the other), so the comparison runs on
    /// the runtime values the way JavaScript's matcher does. Related operands
    /// are returned unchanged and keep their typed comparison.
    fn erase_unrelated_matcher_operands(
        &mut self,
        actual: smelt_hir::ExprId,
        expected: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> (smelt_hir::ExprId, smelt_hir::ExprId) {
        let actual_ty = Self::expr_ty(body, actual);
        let expected_ty = Self::expr_ty(body, expected);
        if self.type_assignable_to(expected_ty, actual_ty)
            || self.type_assignable_to(actual_ty, expected_ty)
            || !self.matcher_operand_is_scalar_or_instance(actual_ty)
            || !self.matcher_operand_is_scalar_or_instance(expected_ty)
            || (self.matcher_operand_is_number(actual_ty)
                && self.matcher_operand_is_number(expected_ty))
        {
            return (actual, expected);
        }
        let unknown_ty = self.erase_with(ErasureReason::TestHarness, "matcher operands");
        let span = self.span(span.start, span.end);
        let erased_actual = body.push_expr(Expr {
            kind: ExprKind::TypeAssert { value: actual },
            ty: unknown_ty,
            span,
        });
        let erased_expected = body.push_expr(Expr {
            kind: ExprKind::TypeAssert { value: expected },
            ty: unknown_ty,
            span,
        });
        (erased_actual, erased_expected)
    }

    /// Whether an equality-matcher operand is a JavaScript number (`Int` or
    /// `Float`, optionally absent). Two such operands are one JS type even when
    /// the lowering picked different widths, and compare numerically as-is.
    fn matcher_operand_is_number(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Optional(inner)) => self.matcher_operand_is_number(*inner),
            Some(Type::Int | Type::Float) => true,
            _ => false,
        }
    }

    /// Whether an equality-matcher operand is a scalar or a class instance
    /// (optionally absent) with no erased component.
    ///
    /// Only such operands take the unrelated-types erasure above. Collections
    /// already compare through the matcher's structural walk, which erases
    /// element-wise where the two sides disagree, and an operand that already
    /// carries `unknown` compares at runtime anyway; routing either through
    /// extra erased temporaries would add boundary crossings for nothing.
    fn matcher_operand_is_scalar_or_instance(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Optional(inner)) => self.matcher_operand_is_scalar_or_instance(*inner),
            Some(Type::Bool | Type::Int | Type::Float | Type::String) => true,
            // A class instance qualifies only when its type arguments do too:
            // one whose arguments carry `unknown` (an unresolved helper alias
            // such as `Awaited<unknown>`) is already compared at runtime.
            Some(Type::Class { args, .. }) => args
                .clone()
                .into_iter()
                .all(|arg| self.matcher_operand_is_scalar_or_instance(arg)),
            _ => false,
        }
    }

    /// Return whether Vitest `toBe` needs JavaScript `SameValue` semantics.
    pub(in crate::lowering) fn test_to_be_needs_strict_identity(
        &self,
        actual: smelt_hir::ExprId,
        expected: smelt_hir::ExprId,
        body: &Body,
    ) -> bool {
        if self.test_numbers_need_same_value(actual, expected, body) {
            return true;
        }
        let actual_ty = self.type_param_constraint_or_self(Self::expr_ty(body, actual));
        let expected_ty = self.type_param_constraint_or_self(Self::expr_ty(body, expected));
        let actual_ref = self.test_to_be_identity_type(actual_ty);
        let expected_ref = self.test_to_be_identity_type(expected_ty);
        if actual_ref || expected_ref {
            return true;
        }
        self.test_to_be_erased_type(actual_ty) && self.test_to_be_erased_type(expected_ty)
    }

    /// Return whether a numeric comparison needs JavaScript `Object.is`.
    ///
    /// Vitest compares primitive numbers with `Object.is`, so `NaN` equals
    /// `NaN`. Rust's `!=` on `f64` reports `NaN != NaN`, which made every
    /// `expect(mean([])).toEqual(NaN)`-shaped assertion fail even though the
    /// value was the expected `NaN`.
    pub(in crate::lowering) fn test_numbers_need_same_value(
        &self,
        actual: smelt_hir::ExprId,
        expected: smelt_hir::ExprId,
        body: &Body,
    ) -> bool {
        if Self::test_to_be_nan_literal(actual, body)
            || Self::test_to_be_nan_literal(expected, body)
        {
            return true;
        }
        let actual_ty = self.type_param_constraint_or_self(Self::expr_ty(body, actual));
        let expected_ty = self.type_param_constraint_or_self(Self::expr_ty(body, expected));
        matches!(
            self.ctx.krate.types.get(actual_ty),
            Some(Type::Int | Type::Float)
        ) && matches!(
            self.ctx.krate.types.get(expected_ty),
            Some(Type::Int | Type::Float)
        )
    }

    /// Return whether an assertion operand is the JavaScript `NaN` literal.
    pub(in crate::lowering) fn test_to_be_nan_literal(
        value: smelt_hir::ExprId,
        body: &Body,
    ) -> bool {
        matches!(
            body.expr_at(value),
            Some(Expr {
                kind: ExprKind::Literal(Literal::Float(number)),
                ..
            }) if number.is_nan()
        )
    }

    /// Return whether a type has reference identity under JavaScript `toBe`.
    pub(in crate::lowering) fn test_to_be_identity_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Optional(inner)) => self.test_to_be_identity_type(*inner),
            Some(
                Type::List(_)
                | Type::Dict(_, _)
                // A source-spelled `Map` keeps its own type variant (it is a
                // `Dict` in every other respect), so it must be listed here too
                // or `toBe` on a `Map` would fall back to structural equality.
                | Type::JsMap(_, _)
                | Type::Set(_)
                | Type::Tuple(_)
                | Type::Class { .. }
                | Type::Function(_),
            ) => true,
            _ => false,
        }
    }

    /// Return whether a type is erased enough that `toBe` must defer to runtime.
    pub(in crate::lowering) fn test_to_be_erased_type(&self, ty: smelt_hir::TypeId) -> bool {
        matches!(
            self.ctx.krate.types.get(ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        )
    }

    /// Lower nullish zero-argument matchers to strict singleton checks.
    pub(in crate::lowering) fn expect_to_be_none_statement(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        actual_override: Option<LoweredActual>,
        body: &mut Body,
        matcher_name: &str,
    ) -> Result<bool, SmeltError> {
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("expect(...).{matcher_name}() does not take arguments"),
            ));
        }
        let (mut actual, inverted, actual_span) = if let Some(actual) = actual_override {
            (
                actual.value,
                actual.inverted,
                self.span(call.span.start, call.span.end),
            )
        } else {
            let (expect_call, inverted) =
                self.expect_call_from_matcher_object(&member.object)?;
            let Expression::Identifier(expect_ident) = &expect_call.callee else {
                return Ok(false);
            };
            if !self.imports.is_test_builtin(expect_ident.name.as_str())
                || expect_ident.name.as_str() != "expect"
            {
                return Ok(false);
            }
            let actual_arg = expect_call.arguments.first().ok_or_else(|| {
                SmeltError::unsupported(
                    self.span(expect_call.span.start, expect_call.span.end),
                    format!("expect(...).{matcher_name}() requires an actual value"),
                )
            })?;
            let span = self.span(actual_arg.span().start, actual_arg.span().end);
            (self.argument(actual_arg, body)?, inverted, span)
        };
        // `expect(record[key]).toBeUndefined()`: the read is typed by the
        // record's value type, so a missing key reads that type's default and
        // can never compare equal to `undefined`. For a value type that is not
        // itself nullable, "is undefined" is exactly "the key is absent" — the
        // containment test a condition on the same read already uses
        // (`keyed_read_presence_condition`). `toBeNull` keeps the comparison:
        // a missing key is `undefined`, not `null`.
        if matcher_name != "toBeNull"
            && let Some(present) = self.keyed_read_contains_key(actual, actual_span, body)
        {
            let mut failed = if matcher_name == "toBeDefined" {
                self.unary_bool_expr(UnaryOp::Not, present, call.span, body)
            } else {
                present
            };
            if inverted {
                failed = self.unary_bool_expr(UnaryOp::Not, failed, call.span, body);
            }
            self.push_test_failure_if(
                failed,
                &format!("expect(...).{matcher_name}() failed"),
                call.span,
                body,
            );
            return Ok(true);
        }
        let actual_ty = Self::expr_ty(body, actual);
        if !matches!(self.ctx.krate.types.get(actual_ty), Some(Type::Optional(_)))
            && self.assertion_type_contains_unknown(actual_ty)
        {
            // not a new erasure: the actual's type already contains `unknown`
            let unknown_ty = self.unknown_type();
            actual = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: actual },
                ty: unknown_ty,
                span: actual_span,
            });
        }
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let literal = if matcher_name == "toBeNull" {
            Literal::None
        } else {
            Literal::Undefined
        };
        let expected = body.push_expr(Expr {
            kind: ExprKind::Literal(literal),
            ty: none_ty,
            span: self.span(call.span.start, call.span.end),
        });
        // `toBeDefined` is `toBeUndefined` read the other way round: the same
        // comparison against `undefined`, with the failure condition flipped.
        let comparison = if matcher_name == "toBeDefined" {
            BinOp::JsStrictEq
        } else {
            BinOp::JsStrictNotEq
        };
        let mut failed = self.comparison_expr(comparison, actual, expected, call.span, body);
        if inverted {
            failed = self.unary_bool_expr(UnaryOp::Not, failed, call.span, body);
        }
        self.push_test_failure_if(
            failed,
            &format!("expect(...).{matcher_name}() failed"),
            call.span,
            body,
        );
        Ok(true)
    }

    /// Resolve the actual value (and `.not` inversion) a matcher asserts on.
    ///
    /// Either it was already lowered by a `.resolves` / `.rejects` chain (see
    /// [`LoweredActual`]), or it is the first argument of the syntactic
    /// `expect(...)` call behind the matcher. `Ok(None)` means the receiver is
    /// not an `expect(...)` call at all, so the caller falls through to the
    /// ordinary call path.
    fn matcher_actual(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        actual_override: Option<LoweredActual>,
        body: &mut Body,
        matcher_name: &str,
    ) -> Result<Option<(smelt_hir::ExprId, bool)>, SmeltError> {
        if let Some(actual) = actual_override {
            return Ok(Some((actual.value, actual.inverted)));
        }
        let (expect_call, inverted) = self.expect_call_from_matcher_object(&member.object)?;
        let Expression::Identifier(expect_ident) = &expect_call.callee else {
            return Ok(None);
        };
        if !self.imports.is_test_builtin(expect_ident.name.as_str())
            || expect_ident.name.as_str() != "expect"
        {
            return Ok(None);
        }
        let actual_arg = expect_call.arguments.first().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(expect_call.span.start, expect_call.span.end),
                format!("expect(...).{matcher_name}() requires an actual value"),
            )
        })?;
        Ok(Some((self.argument(actual_arg, body)?, inverted)))
    }

    /// Lower `toBeTruthy` / `toBeFalsy` through JavaScript truthiness.
    ///
    /// The coercion is the SAME one an `if (value)` condition uses
    /// ([`Self::lowered_condition_expression`]), so a matcher cannot disagree
    /// with the language about what is truthy — `0`, `""`, `NaN`, `null` and
    /// `undefined` are falsy, everything else is truthy, whatever static type
    /// the actual value has.
    fn expect_truthiness_statement(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        actual_override: Option<LoweredActual>,
        body: &mut Body,
        matcher_name: &str,
    ) -> Result<bool, SmeltError> {
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("expect(...).{matcher_name}() does not take arguments"),
            ));
        }
        let Some((actual, inverted)) =
            self.matcher_actual(member, actual_override, body, matcher_name)?
        else {
            return Ok(false);
        };
        let span = self.span(call.span.start, call.span.end);
        let truthy = self.lowered_condition_expression(actual, span, body)?;
        // `toBeFalsy` asserts the negation, and `.not` inverts whichever of the
        // two was written; the two inversions compose with `!=`.
        let wants_truthy = (matcher_name == "toBeTruthy") != inverted;
        let failed = if wants_truthy {
            self.unary_bool_expr(UnaryOp::Not, truthy, call.span, body)
        } else {
            truthy
        };
        self.push_test_failure_if(
            failed,
            &format!("expect(...).{matcher_name}() failed"),
            call.span,
            body,
        );
        Ok(true)
    }

    /// Lower `toMatchObject(expected)` as a recursive-subset matcher value.
    ///
    /// `toMatchObject` is the subset form of `toEqual`: the actual value may
    /// carry properties the expected object does not mention, at every level.
    /// That is a property of the COMPARISON, not of either operand, so it
    /// lowers to the same branded matcher record the `expect.*` factories use
    /// (kind `matchObject`) and is answered by the matcher-aware deep-equality
    /// walk — which means a nested `expect.any(...)` inside the expected object
    /// keeps working with no extra machinery.
    fn expect_match_object_statement(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        actual_override: Option<LoweredActual>,
        body: &mut Body,
    ) -> Result<bool, SmeltError> {
        let Some((actual, inverted)) =
            self.matcher_actual(member, actual_override, body, "toMatchObject")?
        else {
            return Ok(false);
        };
        let expected_arg = call.arguments.first().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "expect(...).toMatchObject(...) requires an expected value",
            )
        })?;
        let span = self.span(call.span.start, call.span.end);
        let unknown_ty = self.erase_with(ErasureReason::TestHarness, "toMatchObject");
        let expected = self.argument_with_hint(expected_arg, body, Some(unknown_ty))?;
        let expected = Self::erase_to_unknown(expected, unknown_ty, span, body);
        let matcher = self.vitest_matcher_record_expr("matchObject", vec![expected], span, body);
        let holds = self.vitest_asymmetric_equal_expr(actual, matcher, call.span, body);
        let failed = if inverted {
            holds
        } else {
            self.unary_bool_expr(UnaryOp::Not, holds, call.span, body)
        };
        self.push_test_failure_if(
            failed,
            "expect(...).toMatchObject(...) failed",
            call.span,
            body,
        );
        Ok(true)
    }

    /// Build a branded asymmetric-matcher record from already-lowered samples.
    ///
    /// Same shape as [`Self::vitest_asymmetric_matcher_call`] emits for the
    /// `expect.*` factories — `{ __smelt_asymmetric: <kind>, sample: [..],
    /// inverted: false }` — so a matcher the lowering synthesizes is read by the
    /// runtime exactly like one the source wrote. Inversion is not carried here:
    /// a synthesized matcher's `.not` is applied to the assertion's own failure
    /// condition, where the source `.not` modifier is.
    fn vitest_matcher_record_expr(
        &mut self,
        kind: &str,
        samples: Vec<smelt_hir::ExprId>,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let unknown_ty = self.erase_with(ErasureReason::TestHarness, "matcher record");
        let dict_ty = self.ctx.krate.types.intern(Type::Dict(string_ty, unknown_ty));
        let list_ty = self.ctx.krate.types.intern(Type::List(unknown_ty));
        let sample = body.push_expr(Expr {
            kind: ExprKind::ListLit(samples),
            ty: list_ty,
            span,
        });
        let sample = Self::erase_to_unknown(sample, unknown_ty, span, body);
        let kind_value = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(kind.to_owned())),
            ty: string_ty,
            span,
        });
        let kind_value = Self::erase_to_unknown(kind_value, unknown_ty, span, body);
        let inverted_value = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(false)),
            ty: bool_ty,
            span,
        });
        let inverted_value = Self::erase_to_unknown(inverted_value, unknown_ty, span, body);
        let string_key = |target: &mut Body, key: &str| {
            target.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(key.to_owned())),
                ty: string_ty,
                span,
            })
        };
        let marker_key = string_key(body, ASYMMETRIC_MARKER_KEY);
        let sample_key = string_key(body, "sample");
        let inverted_key = string_key(body, "inverted");
        let record = body.push_expr(Expr {
            kind: ExprKind::DictLit(vec![
                (marker_key, kind_value),
                (sample_key, sample),
                (inverted_key, inverted_value),
            ]),
            ty: dict_ty,
            span,
        });
        Self::erase_to_unknown(record, unknown_ty, span, body)
    }

    /// Lower `toHaveBeenNthCalledWith(n, ...args)` over a mock's recorded calls.
    ///
    /// The mock's own `mock.calls` view is the recorded-call list every other
    /// call assertion reads, so the nth call is that list indexed at `n - 1`
    /// (vitest numbers calls from one). The comparison is the matcher-aware deep
    /// equality, which is what makes an asymmetric matcher usable as one of the
    /// expected arguments, exactly as in `toHaveBeenCalledWith`.
    fn expect_mock_nth_called_with_statement(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        actual_override: Option<LoweredActual>,
        body: &mut Body,
    ) -> Result<bool, SmeltError> {
        let Some((actual, inverted)) =
            self.matcher_actual(member, actual_override, body, "toHaveBeenNthCalledWith")?
        else {
            return Ok(false);
        };
        let Some(nth_arg) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "expect(...).toHaveBeenNthCalledWith(...) requires a call number",
            ));
        };
        let span = self.span(call.span.start, call.span.end);
        let unknown_ty = self.erase_with(ErasureReason::TestHarness, "toHaveBeenNthCalledWith");
        let float_ty = self.ctx.krate.types.intern(Type::Float);
        let list_ty = self.ctx.krate.types.intern(Type::List(unknown_ty));
        let nth = self.argument(nth_arg, body)?;
        let one = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Float(1.0)),
            ty: float_ty,
            span,
        });
        let index = body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: BinOp::Sub,
                lhs: nth,
                rhs: one,
            },
            ty: float_ty,
            span,
        });
        let mock_value = Self::erase_to_unknown(actual, unknown_ty, span, body);
        let mock_field = self.ctx.krate.symbols.intern("mock");
        let calls_field = self.ctx.krate.symbols.intern("calls");
        let mock_view = body.push_expr(Expr {
            kind: ExprKind::Field {
                receiver: mock_value,
                field: mock_field,
            },
            ty: unknown_ty,
            span,
        });
        let calls = body.push_expr(Expr {
            kind: ExprKind::Field {
                receiver: mock_view,
                field: calls_field,
            },
            ty: unknown_ty,
            span,
        });
        let nth_call = body.push_expr(Expr {
            kind: ExprKind::Index {
                receiver: calls,
                index,
            },
            ty: unknown_ty,
            span,
        });
        let mut expected_items = Vec::new();
        for argument in call.arguments.iter().skip(1) {
            let lowered = self.argument_with_hint(argument, body, Some(unknown_ty))?;
            expected_items.push(Self::erase_to_unknown(lowered, unknown_ty, span, body));
        }
        let expected = body.push_expr(Expr {
            kind: ExprKind::ListLit(expected_items),
            ty: list_ty,
            span,
        });
        let expected = Self::erase_to_unknown(expected, unknown_ty, span, body);
        let holds = self.vitest_asymmetric_equal_expr(nth_call, expected, call.span, body);
        let failed = if inverted {
            holds
        } else {
            self.unary_bool_expr(UnaryOp::Not, holds, call.span, body)
        };
        self.push_test_failure_if(
            failed,
            "expect(...).toHaveBeenNthCalledWith(...) failed",
            call.span,
            body,
        );
        Ok(true)
    }

    /// Extract `expect(...)` and whether `.not` was present from a matcher receiver.
    pub(in crate::lowering) fn expect_call_from_matcher_object<'a>(
        &self,
        expression: &'a Expression<'a>,
    ) -> Result<(&'a oxc::ast::ast::CallExpression<'a>, bool), SmeltError> {
        if let Expression::CallExpression(expect_call) = expression {
            return Ok((expect_call, false));
        }
        let Expression::StaticMemberExpression(member) = expression else {
            return Err(SmeltError::unsupported(
                self.span(expression.span().start, expression.span().end),
                "expect matcher receiver must be expect(...) or expect(...).not",
            ));
        };
        if member.property.name != "not" {
            return Err(SmeltError::unsupported(
                self.span(member.span.start, member.span.end),
                "only expect(...).not matcher modifiers are supported",
            ));
        }
        let Expression::CallExpression(expect_call) = &member.object else {
            return Err(SmeltError::unsupported(
                self.span(member.object.span().start, member.object.span().end),
                "expect(...).not requires an expect(...) receiver",
            ));
        };
        Ok((expect_call, true))
    }

    /// Lower `expect(() => ...).toThrow(...)` to native HIR exception flow.
    ///
    /// The optional expected-error argument is compared against the caught
    /// value with vitest's rules (`to_throw.rs`); without one, any throw
    /// matches.
    pub(in crate::lowering) fn expect_to_throw_statement(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<bool, SmeltError> {
        let (expect_call, inverted) = self.expect_call_from_matcher_object(&member.object)?;
        let Expression::Identifier(expect_ident) = &expect_call.callee else {
            return Ok(false);
        };
        if !self.imports.is_test_builtin(expect_ident.name.as_str())
            || expect_ident.name.as_str() != "expect"
        {
            return Ok(false);
        }
        let actual_arg = expect_call.arguments.first().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(expect_call.span.start, expect_call.span.end),
                "expect(...).toThrow(...) requires a callback",
            )
        })?;
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let span = self.span(call.span.start, call.span.end);
        let did_throw_name = self.intern_source_name("did_throw");
        let did_throw = body.push_local(LocalDecl {
            name: Some(did_throw_name),
            ty: bool_ty,
            mutable: true,
            span,
        });
        let did_throw_pat = body.push_pattern(Pattern::Binding(did_throw));
        let false_expr = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(false)),
            ty: bool_ty,
            span,
        });
        let did_throw_stmt = Stmt::Let {
            pat: did_throw_pat,
            ty: bool_ty,
            value: Some(false_expr),
        };
        self.push_stmt_to_sink(body, did_throw_stmt);

        let try_block = if let Argument::ArrowFunctionExpression(arrow) = actual_arg {
            let none_ty = self.ctx.krate.types.intern(Type::None);
            let mut callee = self.arrow_closure_body_expr(arrow, &[], none_ty, body)?;
            let function = FunctionType {
                params: Vec::new(),
                rest: None,
                required_params: Some(0),
                mutable_params: Vec::new(),
                return_ty: none_ty,
                is_async: false,
                may_throw: true,
            };
            let throwing_function_ty = self.ctx.krate.types.intern(Type::Function(function));
            callee = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: callee },
                ty: throwing_function_ty,
                span: self.span(arrow.span.start, arrow.span.end),
            });
            let try_block = body.push_block(self.arrow_body_span(arrow));
            let call_expr = body.push_expr(Expr {
                kind: ExprKind::ClosureCall {
                    callee,
                    args: Vec::new(),
                },
                ty: none_ty,
                span: self.span(arrow.span.start, arrow.span.end),
            });
            body.push_stmt_to_block(try_block, Stmt::Expr(call_expr));
            try_block
        } else {
            let mut callee = self.argument(actual_arg, body)?;
            let callee_ty = self
                .ctx
                .krate
                .types
                .get(Self::expr_ty(body, callee))
                .cloned();
            let mut function = match callee_ty {
                Some(Type::Function(function)) => function,
                // `expect(value).toThrow()` may name a callable whose static
                // shape is erased here (a cross-module helper such as
                // `once(...)` resolves to `Unknown` under single-file lowering).
                // It is still a zero-argument callable observed only for whether
                // it throws, so adapt it through a synthesized throwing signature
                // rather than rejecting the matcher.
                Some(Type::Unknown) => FunctionType {
                    params: Vec::new(),
                    rest: None,
                    required_params: Some(0),
                    mutable_params: Vec::new(),
                    return_ty: self.ctx.krate.types.intern(Type::None),
                    is_async: false,
                    may_throw: true,
                },
                _ => {
                    return Err(SmeltError::unsupported(
                        self.span(actual_arg.span().start, actual_arg.span().end),
                        "expect(...).toThrow(...) requires a zero-argument callback",
                    ));
                }
            };
            if Self::is_bind_call_argument(actual_arg) {
                function.params.clear();
            }
            // `toThrow` observes the callable's error result. At this call site a
            // thrown value must flow to the synthesized catch block even when the
            // callable's source signature omitted a throwing annotation.
            function.may_throw = true;
            let throwing_function_ty = self
                .ctx
                .krate
                .types
                .intern(Type::Function(function.clone()));
            callee = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: callee },
                ty: throwing_function_ty,
                span: self.span(actual_arg.span().start, actual_arg.span().end),
            });
            let try_block =
                body.push_block(self.span(actual_arg.span().start, actual_arg.span().end));
            let mut args = Vec::new();
            for param_ty in function.params {
                if !matches!(self.ctx.krate.types.get(param_ty), Some(Type::Optional(_))) {
                    return Err(SmeltError::unsupported(
                        self.span(actual_arg.span().start, actual_arg.span().end),
                        "expect(...).toThrow(...) requires a zero-argument callback",
                    ));
                }
                args.push(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty: param_ty,
                    span: self.span(actual_arg.span().start, actual_arg.span().end),
                }));
            }
            let call_expr = body.push_expr(Expr {
                kind: ExprKind::ClosureCall { callee, args },
                ty: function.return_ty,
                span: self.span(actual_arg.span().start, actual_arg.span().end),
            });
            body.push_stmt_to_block(try_block, Stmt::Expr(call_expr));
            try_block
        };
        // The caught value, for the expected-error argument: `did_throw`
        // becomes "threw an error MATCHING the argument" (see `to_throw.rs`),
        // so a wrong error fails the assertion instead of counting as a throw.
        // `toThrowErrorMatchingInlineSnapshot(snapshot)` compares the thrown
        // error's SERIALIZATION (`[Error: message]`) with a snapshot, not its
        // message with a substring, so the message rules do not apply to it;
        // it keeps the any-throw check.
        let compares_expected = Self::canonical_matcher_name(member.property.name.as_str())
            != "toThrowErrorMatchingInlineSnapshot";
        let (catch_binding, matched) = match call.arguments.first().filter(|_| compares_expected) {
            Some(expected_arg) => self
                .expected_error_predicate(expected_arg, body)?
                .map_or((None, None), |(caught, matched)| (Some(caught), Some(matched))),
            None => (None, None),
        };
        let catch_block = body.push_block(span);
        let did_throw_target = body.push_expr(Expr {
            kind: ExprKind::Local(did_throw),
            ty: bool_ty,
            span,
        });
        let matched = matched.unwrap_or_else(|| {
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(true)),
                ty: bool_ty,
                span,
            })
        });
        body.push_stmt_to_block(
            catch_block,
            Stmt::Assign {
                target: did_throw_target,
                value: matched,
            },
        );
        let try_stmt = Stmt::TryCatch {
            body: try_block,
            catch_binding,
            catch_body: Some(catch_block),
            finally_body: None,
        };
        self.push_stmt_to_sink(body, try_stmt);

        let did_throw_check = body.push_expr(Expr {
            kind: ExprKind::Local(did_throw),
            ty: bool_ty,
            span,
        });
        let failed = if inverted {
            did_throw_check
        } else {
            self.unary_bool_expr(UnaryOp::Not, did_throw_check, call.span, body)
        };
        let message = if inverted {
            "expect(...).not.toThrow(...) failed"
        } else {
            "expect(...).toThrow(...) failed"
        };
        self.push_test_failure_if(failed, message, call.span, body);
        Ok(true)
    }

    /// Return whether an `expect(...).toThrow()` argument is a bound function call.
    pub(in crate::lowering) fn is_bind_call_argument(argument: &Argument<'_>) -> bool {
        let Some(Expression::CallExpression(call)) = argument.as_expression() else {
            return false;
        };
        matches!(
            &call.callee,
            Expression::StaticMemberExpression(member) if member.property.name == "bind"
        )
    }

    /// Route a DEEP-equality comparison of class-typed operands through the
    /// erased structural comparison.
    ///
    /// Vitest's `toEqual`/`toStrictEqual` compare own enumerable properties, not
    /// identity — only `toBe` is identity. A Smelt class with reference
    /// semantics gets `PartialEq = Rc::ptr_eq`, so lowering the deep matchers to
    /// a plain `!=` on the class type asked "is this the same object", which no
    /// freshly built value can ever satisfy: `expect(clone(error)).toEqual(error)`
    /// was unsatisfiable by construction, and so was every other deep assertion
    /// about a class instance.
    ///
    /// Erasing both operands makes the comparison `SmeltUnknown`'s
    /// `PartialEq` — `smelt_unknown_structural_eq`, the structural walk that is
    /// what the matcher means. It is applied only when a class type is involved:
    /// every other operand type already compares structurally, and rerouting
    /// those through erasure would trade correct typed comparisons (float leaves,
    /// tuples, lists) for a slower erased one.
    fn deep_equality_operands(
        &mut self,
        actual: smelt_hir::ExprId,
        expected: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> (smelt_hir::ExprId, smelt_hir::ExprId) {
        let actual_ty = self.type_param_constraint_or_self(Self::expr_ty(body, actual));
        let expected_ty = self.type_param_constraint_or_self(Self::expr_ty(body, expected));
        let is_class = |ty: smelt_hir::TypeId| {
            matches!(self.ctx.krate.types.get(ty), Some(Type::Class { .. }))
        };
        if !is_class(actual_ty) && !is_class(expected_ty) {
            return (actual, expected);
        }
        let unknown_ty = self.erase_with(ErasureReason::TestHarness, "deep equality operands");
        let span = self.span(span.start, span.end);
        let erase = |value: smelt_hir::ExprId, ty: smelt_hir::TypeId, body: &mut Body| {
            if ty == unknown_ty {
                return value;
            }
            body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value,
                    target: unknown_ty,
                },
                ty: unknown_ty,
                span,
            })
        };
        let actual = erase(actual, actual_ty, body);
        let expected = erase(expected, expected_ty, body);
        (actual, expected)
    }

    /// Build the boolean expression that means a supported matcher has failed.
    pub(in crate::lowering) fn expect_matcher_failure_expr(
        &mut self,
        matcher: TestMatcher,
        actual: smelt_hir::ExprId,
        expected: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match matcher {
            TestMatcher::Equal | TestMatcher::StrictEqual => {
                let (actual, expected) =
                    self.deep_equality_operands(actual, expected, span, body);
                Ok(self.comparison_expr(BinOp::NotEq, actual, expected, span, body))
            }
            TestMatcher::Be => {
                Ok(self.comparison_expr(BinOp::NotEq, actual, expected, span, body))
            }
            TestMatcher::Contain => {
                let contains = self.contains_expr(actual, expected, span, body)?;
                Ok(self.unary_bool_expr(UnaryOp::Not, contains, span, body))
            }
            TestMatcher::HaveLength => {
                let len = self.len_expr(actual, span, body)?;
                Ok(self.comparison_expr(BinOp::NotEq, len, expected, span, body))
            }
            TestMatcher::HaveProperty => {
                let contains = self.dict_contains_key_expr(actual, expected, span, body)?;
                Ok(self.unary_bool_expr(UnaryOp::Not, contains, span, body))
            }
            // A comparison matcher FAILS when the opposite comparison holds, so
            // the inverse operator is the failure condition directly — no
            // separate negation step, and no assumption about the operands
            // beyond the ordering the source already asked for.
            TestMatcher::BeLessThan => {
                Ok(self.comparison_expr(BinOp::Gte, actual, expected, span, body))
            }
            TestMatcher::BeLessThanOrEqual => {
                Ok(self.comparison_expr(BinOp::Gt, actual, expected, span, body))
            }
            TestMatcher::BeGreaterThan => {
                Ok(self.comparison_expr(BinOp::Lte, actual, expected, span, body))
            }
            TestMatcher::BeGreaterThanOrEqual => {
                Ok(self.comparison_expr(BinOp::Lt, actual, expected, span, body))
            }
            // `toBeTypeOf` asks the JavaScript `typeof` question, so it goes
            // through the same runtime tag read a `typeof value` expression
            // lowers to rather than a static answer: the matcher is written
            // precisely where the static type is not trusted.
            TestMatcher::BeTypeOf => {
                let span_hir = self.span(span.start, span.end);
                let unknown_ty = self.erase_with(ErasureReason::TestHarness, "toBeTypeOf");
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let value = Self::erase_to_unknown(actual, unknown_ty, span_hir, body);
                let type_name = body.push_expr(Expr {
                    kind: ExprKind::TypeofValue { value },
                    ty: string_ty,
                    span: span_hir,
                });
                Ok(self.comparison_expr(BinOp::NotEq, type_name, expected, span, body))
            }
            // `toMatch` takes either a substring or a RegExp; vitest picks by
            // the ARGUMENT's kind, and so does this, from the lowered expected
            // value's type rather than its spelling.
            TestMatcher::Match => {
                let expected_ty = Self::expr_ty(body, expected);
                let is_regexp = self.stdlib_class_of_type(expected_ty) == Some(smelt_stdlib::StdlibClass::RegExp);
                let holds = if is_regexp {
                    let bool_ty = self.ctx.krate.types.intern(Type::Bool);
                    body.push_expr(Expr {
                        kind: ExprKind::RegexTest {
                            regex: expected,
                            haystack: actual,
                        },
                        ty: bool_ty,
                        span: self.span(span.start, span.end),
                    })
                } else {
                    self.contains_expr(actual, expected, span, body)?
                };
                Ok(self.unary_bool_expr(UnaryOp::Not, holds, span, body))
            }
            TestMatcher::BeInstanceOf => Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "toBeInstanceOf lowers through the instanceof rule, not a value comparison",
            )),
        }
    }

    /// Push a synthesized assertion statement into the block currently being
    /// lowered into, falling back to the body root.
    ///
    /// Assertions are built while lowering an expression, so they cannot use
    /// `Body::push_stmt` directly: that always appends to the function root,
    /// which would hoist an assertion out of the `if` or loop body it was
    /// written in. The statement sink names the block the enclosing statement
    /// lowering is filling.
    pub(in crate::lowering) fn push_assertion_stmt(&self, body: &mut Body, stmt: Stmt) {
        self.push_stmt_to_sink(body, stmt);
    }

    /// Push a throwing failure block guarded by a boolean condition.
    pub(in crate::lowering) fn push_test_failure_if(
        &mut self,
        cond: smelt_hir::ExprId,
        message: &str,
        span: oxc::span::Span,
        body: &mut Body,
    ) {
        let failure_block = body.push_block(self.span(span.start, span.end));
        let message = self.test_failure_message(message, span);
        let message = self.string_literal_expr(&message, span, body);
        body.push_stmt_to_block(failure_block, Stmt::Throw(message));
        let failure_stmt = Stmt::If {
            cond,
            then_block: failure_block,
            else_block: None,
        };
        self.push_stmt_to_sink(body, failure_stmt);
    }

    /// Build the runtime message thrown by a failed synthesized assertion.
    ///
    /// A generated assertion throws a plain string, so on its own a failing
    /// suite reports only which matcher failed -- with hundreds of generated
    /// tests that is not enough to find the source assertion. Vitest prints
    /// the failing expression and its location, so this appends the same two
    /// things: the source text of the assertion call and `path:line:column`.
    /// The matcher prefix is left untouched so callers that match on it keep
    /// working.
    fn test_failure_message(&self, message: &str, span: oxc::span::Span) -> String {
        let (line, column) = self.line_column(span.start);
        let path = self.path.as_str();
        self.assertion_snippet(span).map_or_else(
            || format!("{message} ({path}:{line}:{column})"),
            |snippet| format!("{message}: {snippet} ({path}:{line}:{column})"),
        )
    }

    /// Resolve a byte offset in this module's source to a 1-based line and column.
    ///
    /// Columns count UTF-8 characters rather than bytes so the location lines
    /// up with what an editor shows for non-ASCII spec sources.
    fn line_column(&self, offset: u32) -> (usize, usize) {
        let offset = usize::try_from(offset).unwrap_or(usize::MAX);
        let Some(prefix) = self.source.get(..offset) else {
            return (0, 0);
        };
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let column = prefix
            .rsplit_once('\n')
            .map_or(prefix, |(_, last)| last)
            .chars()
            .count()
            + 1;
        (line, column)
    }

    /// Render the source text of an assertion as a single-line message fragment.
    ///
    /// Newlines and runs of whitespace are collapsed so a multi-line `expect`
    /// call still reads as one line, and long assertions are truncated on a
    /// character boundary to keep failure output scannable.
    fn assertion_snippet(&self, span: oxc::span::Span) -> Option<String> {
        const MAX_CHARS: usize = 120;
        let start = usize::try_from(span.start).ok()?;
        let end = usize::try_from(span.end).ok()?;
        let text = self.source.get(start..end)?;
        let mut snippet = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if snippet.is_empty() {
            return None;
        }
        if snippet.chars().count() > MAX_CHARS {
            let cut = snippet
                .char_indices()
                .nth(MAX_CHARS)
                .map_or(snippet.len(), |(index, _)| index);
            snippet.truncate(cut);
            snippet.push_str("...");
        }
        Some(snippet)
    }

    /// Create a boolean unary expression for synthesized test assertions.
    pub(in crate::lowering) fn unary_bool_expr(
        &mut self,
        op: UnaryOp,
        operand: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        body.push_expr(Expr {
            kind: ExprKind::UnaryOp { op, operand },
            ty: bool_ty,
            span: self.span(span.start, span.end),
        })
    }

    /// Create a boolean comparison expression for synthesized test assertions.
    pub(in crate::lowering) fn comparison_expr(
        &mut self,
        op: BinOp,
        lhs: smelt_hir::ExprId,
        mut rhs: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let lhs_ty = Self::expr_ty(body, lhs);
        let rhs_ty = Self::expr_ty(body, rhs);
        let compares_optional_to_none = matches!(
            (
                self.ctx.krate.types.get(lhs_ty),
                self.ctx.krate.types.get(rhs_ty)
            ),
            (Some(Type::Optional(_)), Some(Type::None))
        );
        if lhs_ty != rhs_ty
            && self.assertion_type_contains_unknown(lhs_ty)
            && !compares_optional_to_none
        {
            rhs = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: rhs },
                ty: lhs_ty,
                span: self.span(span.start, span.end),
            });
        }
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        body.push_expr(Expr {
            kind: ExprKind::BinOp { op, lhs, rhs },
            ty: bool_ty,
            span: self.span(span.start, span.end),
        })
    }

    /// Return whether an assertion actual type contains erased unknown values.
    ///
    /// This is intentionally local to test assertions. The broader lowering
    /// pipeline still uses the narrower `type_contains_unknown` helper because
    /// treating every `Array<unknown>` as unknown-like changes overload choices
    /// in normal library code.
    pub(in crate::lowering) fn assertion_type_contains_unknown(
        &self,
        ty: smelt_hir::TypeId,
    ) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Unknown | Type::TypeParam { .. }) => true,
            Some(
                Type::Optional(item) | Type::List(item) | Type::Set(item) | Type::Future(item),
            ) => self.assertion_type_contains_unknown(*item),
            Some(Type::Dict(key, value)) => {
                self.assertion_type_contains_unknown(*key)
                    || self.assertion_type_contains_unknown(*value)
            }
            Some(Type::Tuple(items) | Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.assertion_type_contains_unknown(item)),
            _ => false,
        }
    }

    /// Create a length expression for synthesized test assertions.
    pub(in crate::lowering) fn len_expr(
        &mut self,
        operand: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let operand_ty = Self::expr_ty(body, operand);
        match self.ctx.krate.types.get(operand_ty) {
            Some(
                Type::String | Type::List(_) | Type::Set(_) | Type::Dict(_, _) | Type::Tuple(_),
            ) => {
                let int_ty = self.ctx.krate.types.intern(Type::Int);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Len { operand },
                    ty: int_ty,
                    span: self.span(span.start, span.end),
                }))
            }
            Some(Type::Unknown)
                if self.allows_unknown_index_access() || self.type_contains_unknown(operand_ty) =>
            {
                let int_ty = self.ctx.krate.types.intern(Type::Int);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Len { operand },
                    ty: int_ty,
                    span: self.span(span.start, span.end),
                }))
            }
            _ if self.allows_unknown_index_access() => {
                let int_ty = self.ctx.krate.types.intern(Type::Int);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Len { operand },
                    ty: int_ty,
                    span: self.span(span.start, span.end),
                }))
            }
            _ => Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "expect(...).toHaveLength(...) requires a string or collection actual value",
            )),
        }
    }

    /// Create a containment expression for synthesized test assertions.
    pub(in crate::lowering) fn contains_expr(
        &mut self,
        actual: smelt_hir::ExprId,
        expected: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let expected_ty = Self::expr_ty(body, expected);
        // The expected value may be erased (`Unknown`) when it comes from a
        // cross-module helper, as in `expect(array).toContain(sample(array))`.
        // A concrete collection actual still supports the runtime containment
        // check against such a value, so an erased expected matches any item
        // type rather than forcing a static element-type equality.
        let expected_is_erased =
            matches!(self.ctx.krate.types.get(expected_ty), Some(Type::Unknown));
        // A `sample(...)`-style helper returns `T | undefined`, so the expected
        // needle is commonly an `Optional(T)` while the actual collection holds
        // `T` (`expect(array).toContain(sample(array))`). JavaScript containment
        // compares the needle against each element regardless of the needle's
        // nullability, so an optional expected whose inner type matches the
        // element type (or is itself erased) is accepted here; the emitter guards
        // the optional at runtime (a `None`/`undefined` needle is never contained
        // in a collection of non-optional elements).
        // Unwrap an optional expected to its inner type; a `None`/`undefined`
        // needle simply never matches, and the emitter guards it at runtime. The
        // containment is supported when the inner type is exactly the element
        // type, or when the inner type is erased (`unknown`/leaked type param):
        // an erased needle is compared against each element after the element is
        // itself erased to the runtime value (JS `includes`/`has` semantics),
        // exactly like a bare erased expected.
        let expected_inner = match self.ctx.krate.types.get(expected_ty) {
            Some(Type::Optional(inner)) => Some(*inner),
            _ => None,
        };
        let expected_inner_is_erased = expected_inner.is_some_and(|inner| {
            matches!(
                self.ctx.krate.types.get(inner),
                Some(Type::Unknown | Type::TypeParam { .. })
            )
        });
        let item_matches = |item_ty: smelt_hir::TypeId| {
            expected_ty == item_ty
                || expected_is_erased
                || expected_inner == Some(item_ty)
                || expected_inner_is_erased
        };
        let kind = match self.ctx.krate.types.get(Self::expr_ty(body, actual)) {
            Some(Type::String)
                if self.ctx.krate.types.get(expected_ty) == Some(&Type::String)
                    || expected_is_erased =>
            {
                ExprKind::StringContains {
                    haystack: actual,
                    needle: expected,
                    from_index: None,
                }
            }
            Some(Type::List(item_ty)) if item_matches(*item_ty) => ExprKind::ListContains {
                list: actual,
                item: expected,
            },
            Some(Type::Set(item_ty)) if item_matches(*item_ty) => ExprKind::SetContains {
                set: actual,
                item: expected,
            },
            Some(Type::Tuple(items)) if items.iter().any(|item| item_matches(*item)) => {
                ExprKind::TupleContains {
                    tuple: actual,
                    item: expected,
                }
            }
            // `string | null` (`headers.get(name)`) and every other optional
            // actual: vitest rejects a nullish actual, so an absent value never
            // contains anything, and a present one is checked at its inner type.
            Some(Type::Optional(inner)) => {
                let inner_ty = *inner;
                return self.optional_contains_expr(actual, inner_ty, expected, span, body);
            }
            Some(Type::Unknown | Type::TypeParam { .. }) => {
                return Ok(self.erased_contains_expr(actual, expected, span, body));
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(span.start, span.end),
                    "expect(...).toContain(...) requires a string, array, set, or tuple actual value with a matching expected value",
                ));
            }
        };
        Ok(body.push_expr(Expr {
            kind,
            ty: bool_ty,
            span: self.span(span.start, span.end),
        }))
    }

    /// Containment over an optional actual (`string | null`, `T[] | undefined`).
    ///
    /// vitest's `toContain`/`toMatch` reject a nullish actual, so the check is
    /// `actual != null && contains(actual!, expected)`: the presence test
    /// guards a `TypeAssert` to the inner type, and the inner containment is
    /// whatever [`Self::contains_expr`] picks for that type — the typed string,
    /// list, set or tuple path, or `erased_contains_expr` for an erased inner
    /// type. The `Conditional` keeps
    /// the unwrap from running when the value is absent.
    fn optional_contains_expr(
        &mut self,
        actual: smelt_hir::ExprId,
        inner_ty: smelt_hir::TypeId,
        expected: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let hir_span = self.span(span.start, span.end);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let none = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty: none_ty,
            span: hir_span,
        });
        let present = body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: BinOp::NotEq,
                lhs: actual,
                rhs: none,
            },
            ty: bool_ty,
            span: hir_span,
        });
        let value = body.push_expr(Expr {
            kind: ExprKind::TypeAssert { value: actual },
            ty: inner_ty,
            span: hir_span,
        });
        let contains = self.contains_expr(value, expected, span, body)?;
        let absent = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(false)),
            ty: bool_ty,
            span: hir_span,
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond: present,
                then_expr: contains,
                else_expr: absent,
            },
            ty: bool_ty,
            span: hir_span,
        }))
    }

    /// Containment over an erased actual (`unknown`, `any`, a leaked type param).
    ///
    /// DYNAMIC BOUNDARY: the actual has no static shape here — it is source
    /// `unknown`/`any`, or a cross-module helper's result that does not resolve
    /// in this lowering unit (`expect(keysIn(buffer)).toContain(k)`,
    /// `expect(JSON.parse(text)).toContain('x')`) — and vitest's `toContain`
    /// picks its semantics from the LIVE value: a string actual is a substring
    /// search, an array actual an element search with `===`, and anything else
    /// fails the assertion. No concrete type, union or scoped generic can make
    /// that choice at compile time, because the source never names one; the
    /// only thing that knows is the value's runtime tag. So the check reads it
    /// (`UnknownIs`) and dispatches:
    ///
    /// * string → `StringContains` over the value viewed as a string, with the
    ///   needle converted by JavaScript `ToString` (what `String.prototype.
    ///   includes` does to a non-string argument);
    /// * array → `ListContains` over the value viewed as an erased list, with
    ///   the needle erased to the same runtime representation;
    /// * otherwise → `false`, so the assertion fails as vitest's does.
    ///
    /// A statically typed actual never reaches this: string/list/set/tuple and
    /// optional actuals keep their typed paths in [`Self::contains_expr`]; an erased
    /// inner type of an optional actual takes this path.
    fn erased_contains_expr(
        &mut self,
        actual: smelt_hir::ExprId,
        expected: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let hir_span = self.span(span.start, span.end);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let string_ty = self.ctx.krate.types.intern(Type::String);
        // not a new erasure: only an already-erased actual reaches this path
        let unknown_ty = self.unknown_type();
        let list_ty = self.ctx.krate.types.intern(Type::List(unknown_ty));
        let expected_ty = Self::expr_ty(body, expected);
        let value = Self::erase_to_unknown(actual, unknown_ty, hir_span, body);

        let is_string = body.push_expr(Expr {
            kind: ExprKind::UnknownIs {
                value,
                kind: UnknownKind::String,
            },
            ty: bool_ty,
            span: hir_span,
        });
        let haystack = body.push_expr(Expr {
            kind: ExprKind::TypeAssert { value },
            ty: string_ty,
            span: hir_span,
        });
        let needle = if expected_ty == string_ty {
            expected
        } else {
            body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToString,
                    operand: expected,
                },
                ty: string_ty,
                span: hir_span,
            })
        };
        let string_contains = body.push_expr(Expr {
            kind: ExprKind::StringContains {
                haystack,
                needle,
                from_index: None,
            },
            ty: bool_ty,
            span: hir_span,
        });

        let is_array = body.push_expr(Expr {
            kind: ExprKind::UnknownIs {
                value,
                kind: UnknownKind::Array,
            },
            ty: bool_ty,
            span: hir_span,
        });
        let list = body.push_expr(Expr {
            kind: ExprKind::TypeAssert { value },
            ty: list_ty,
            span: hir_span,
        });
        let item = if expected_ty == unknown_ty {
            expected
        } else {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: expected },
                ty: unknown_ty,
                span: hir_span,
            })
        };
        let list_contains = body.push_expr(Expr {
            kind: ExprKind::ListContains { list, item },
            ty: bool_ty,
            span: hir_span,
        });
        let neither = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(false)),
            ty: bool_ty,
            span: hir_span,
        });
        let array_or_nothing = body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond: is_array,
                then_expr: list_contains,
                else_expr: neither,
            },
            ty: bool_ty,
            span: hir_span,
        });
        body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond: is_string,
                then_expr: string_contains,
                else_expr: array_or_nothing,
            },
            ty: bool_ty,
            span: hir_span,
        })
    }

    /// Create a dictionary key containment expression for `toHaveProperty`.
    ///
    /// A statically-typed record/map (`Type::Dict`) checks key membership
    /// directly and requires the key type to match. An erased actual
    /// (`Unknown`/`Union`/unconstrained type param, or a class-shaped type
    /// with no local declaration such as the ambient `IArguments` interface —
    /// see [`Self::class_type_erases_to_unknown`]) is a runtime JavaScript
    /// value — for example the erased return of an imported helper — so the
    /// emitted `DictContainsKey` inspects the live `SmeltUnknown::Object` at
    /// runtime; the key may be any string-convertible value there, so no
    /// static key-type match is demanded. This keeps `toHaveProperty` general
    /// over both concrete records and erased object actuals.
    pub(in crate::lowering) fn dict_contains_key_expr(
        &mut self,
        actual: smelt_hir::ExprId,
        expected: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let actual_ty = Self::expr_ty(body, actual);
        match self.ctx.krate.types.get(actual_ty) {
            Some(Type::Dict(key_ty, _)) => {
                if Self::expr_ty(body, expected) != *key_ty {
                    return Err(SmeltError::unsupported(
                        self.span(span.start, span.end),
                        "expect(...).toHaveProperty(...) key must match the object key type",
                    ));
                }
            }
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. }) => {}
            _ if self.class_type_erases_to_unknown(actual_ty) => {}
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(span.start, span.end),
                    format!(
                        "expect(...).toHaveProperty(...) requires an object or map actual value (actual: {:?})",
                        self.ctx.krate.types.get(actual_ty)
                    ),
                ));
            }
        }
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(body.push_expr(Expr {
            kind: ExprKind::DictContainsKey {
                lookup: PropertyLookup::PrototypeChain,
                dict: actual,
                key: expected,
            },
            ty: bool_ty,
            span: self.span(span.start, span.end),
        }))
    }

    /// Create a string literal expression for synthesized test diagnostics.
    pub(in crate::lowering) fn string_literal_expr(
        &mut self,
        value: &str,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let ty = self.ctx.krate.types.intern(Type::String);
        body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(value.to_owned())),
            ty,
            span: self.span(span.start, span.end),
        })
    }
}
