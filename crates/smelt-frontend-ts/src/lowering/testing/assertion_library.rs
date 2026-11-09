//! Assertion libraries other than `expect` from vitest: chai and `node:assert`.
//!
//! A test written against chai (`expect(a).to.deep.equal(b)`, `assert.isTrue(c)`)
//! must fail when the value is wrong, exactly as under Node. Neither library
//! needs its own comparison code: each recognizer here finds the name, the
//! expected arguments and the actual value and hands them to
//! [`ModuleBuilder::matcher_site_statement`], or, for a chai rule vitest has no
//! matcher for, to [`LibraryCheck`].
//!
//! Which library a name belongs to is decided by the import that bound it, never
//! by its spelling, so a local helper called `assert` is an ordinary call.
//!
//! A name the model does not have is a failing assertion that says so. It must
//! not fall through to a value that answers every call, because a test through
//! such a value passes having asserted nothing.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::testing::matchers::{LoweredActual, MatcherSite, SiteActual};
use oxc::ast::ast::{Argument, CallExpression, Expression};
use oxc::span::GetSpan;
use smelt_hir::{BinOp, Body, ErasureReason, Expr, ExprId, ExprKind, Literal, LocalDecl, Pattern, Stmt, Type, UnaryOp};

/// An assertion rule, once the library's spelling is gone.
#[derive(Clone, Copy, Debug, PartialEq)]
enum LibraryCheck {
    /// The vitest matcher of this name has the same meaning.
    Matcher(&'static str),
    /// `==`.
    LooseEqual,
    /// `===`.
    StrictEqual,
    /// `=== true` / `=== false`.
    IsBool(bool),
    /// `Object.prototype.toString` names this type.
    TypeIs(&'static str),
    /// As [`Self::TypeIs`], with the name in the first argument.
    TypeArgument,
    /// Neither `null` nor `undefined`.
    Exists,
    /// Answered by the runtime over erased values (`smelt_test_check`).
    Runtime(&'static str),
    /// Fails, with the message in the first argument.
    Fail,
}

/// The chai words that read as English and change nothing.
const CHAI_LANGUAGE_CHAINS: &[&str] = &[
    "to", "be", "been", "is", "that", "which", "and", "has", "have", "with", "at", "of", "same",
    "but", "does", "still", "also",
];

/// The rule chai's `expect` chain ends in, given the flags collected before it.
///
/// `None` is a word this model does not have.
fn chai_terminal(name: &str, deep: bool, contains: bool) -> Option<LibraryCheck> {
    use LibraryCheck::{Exists, Fail, IsBool, Matcher, Runtime, StrictEqual, TypeArgument};
    Some(match name {
        "equal" | "equals" | "eq" if deep => Matcher("toStrictEqual"),
        "equal" | "equals" | "eq" => StrictEqual,
        "eql" | "eqls" => Matcher("toStrictEqual"),
        "true" => IsBool(true),
        "false" => IsBool(false),
        "null" => Matcher("toBeNull"),
        "undefined" => Matcher("toBeUndefined"),
        "NaN" => Matcher("toBeNaN"),
        "ok" => Matcher("toBeTruthy"),
        "exist" | "exists" => Exists,
        "empty" => Runtime("is_empty"),
        "a" | "an" => TypeArgument,
        "instanceof" | "instanceOf" => Matcher("toBeInstanceOf"),
        "include" | "includes" | "contain" | "contains" | "string" if deep => Runtime("deep_include"),
        "include" | "includes" | "contain" | "contains" | "string" => Runtime("include"),
        "lengthOf" | "length" => Runtime("length_is"),
        "match" | "matches" => Matcher("toMatch"),
        "above" | "gt" | "greaterThan" => Matcher("toBeGreaterThan"),
        "below" | "lt" | "lessThan" => Matcher("toBeLessThan"),
        "least" | "gte" | "greaterThanOrEqual" => Matcher("toBeGreaterThanOrEqual"),
        "most" | "lte" | "lessThanOrEqual" => Matcher("toBeLessThanOrEqual"),
        "within" => Runtime("within"),
        "closeTo" | "approximately" => Runtime("close_to"),
        "property" | "ownProperty" | "haveOwnProperty" => Runtime("has_property"),
        "keys" | "key" => Runtime("keys_are"),
        "members" if contains => Runtime("includes_members"),
        "members" => Runtime("same_members"),
        "oneOf" => Runtime("one_of"),
        "satisfy" | "satisfies" => Runtime("satisfies"),
        "throw" | "throws" | "Throw" => Matcher("toThrow"),
        "fail" => Fail,
        _ => return None,
    })
}

/// The rule of `assert.<name>(actual, ...)`, shared by chai and `node:assert`,
/// and whether the name asserts the opposite.
fn assert_method(name: &str, chai: bool) -> Option<(LibraryCheck, bool)> {
    use LibraryCheck::{Exists, Fail, IsBool, LooseEqual, Matcher, Runtime, StrictEqual, TypeArgument, TypeIs};
    let deep_rule = if chai { Matcher("toStrictEqual") } else { Matcher("toEqual") };
    Some(match name {
        "equal" => (LooseEqual, false),
        "notEqual" => (LooseEqual, true),
        "strictEqual" => (StrictEqual, false),
        "notStrictEqual" => (StrictEqual, true),
        "deepEqual" => (deep_rule, false),
        "notDeepEqual" => (deep_rule, true),
        "deepStrictEqual" => (Matcher("toStrictEqual"), false),
        "notDeepStrictEqual" => (Matcher("toStrictEqual"), true),
        "isTrue" => (IsBool(true), false),
        "isNotTrue" => (IsBool(true), true),
        "isFalse" => (IsBool(false), false),
        "isNotFalse" => (IsBool(false), true),
        "isNull" => (Matcher("toBeNull"), false),
        "isNotNull" => (Matcher("toBeNull"), true),
        "isUndefined" => (Matcher("toBeUndefined"), false),
        "isDefined" => (Matcher("toBeUndefined"), true),
        "ok" | "isOk" => (Matcher("toBeTruthy"), false),
        "notOk" | "isNotOk" => (Matcher("toBeTruthy"), true),
        "isNaN" => (Matcher("toBeNaN"), false),
        "isNotNaN" => (Matcher("toBeNaN"), true),
        "exists" => (Exists, false),
        "notExists" => (Exists, true),
        "isEmpty" => (Runtime("is_empty"), false),
        "isNotEmpty" => (Runtime("is_empty"), true),
        "isArray" => (TypeIs("array"), false),
        "isNotArray" => (TypeIs("array"), true),
        "isString" => (TypeIs("string"), false),
        "isNotString" => (TypeIs("string"), true),
        "isNumber" => (TypeIs("number"), false),
        "isNotNumber" => (TypeIs("number"), true),
        "isBoolean" => (TypeIs("boolean"), false),
        "isNotBoolean" => (TypeIs("boolean"), true),
        "isFunction" => (TypeIs("function"), false),
        "isNotFunction" => (TypeIs("function"), true),
        "isObject" => (TypeIs("object"), false),
        "isNotObject" => (TypeIs("object"), true),
        "typeOf" => (TypeArgument, false),
        "notTypeOf" => (TypeArgument, true),
        "instanceOf" => (Matcher("toBeInstanceOf"), false),
        "notInstanceOf" => (Matcher("toBeInstanceOf"), true),
        "include" => (Runtime("include"), false),
        "notInclude" => (Runtime("include"), true),
        "deepInclude" => (Runtime("deep_include"), false),
        "lengthOf" => (Runtime("length_is"), false),
        "match" => (Matcher("toMatch"), false),
        "notMatch" | "doesNotMatch" => (Matcher("toMatch"), true),
        "property" | "hasProperty" => (Runtime("has_property"), false),
        "notProperty" => (Runtime("has_property"), true),
        "isAbove" => (Matcher("toBeGreaterThan"), false),
        "isBelow" => (Matcher("toBeLessThan"), false),
        "isAtLeast" => (Matcher("toBeGreaterThanOrEqual"), false),
        "isAtMost" => (Matcher("toBeLessThanOrEqual"), false),
        "closeTo" | "approximately" => (Runtime("close_to"), false),
        "throws" | "throw" | "Throw" => (Matcher("toThrow"), false),
        "doesNotThrow" => (Matcher("toThrow"), true),
        "fail" => (Fail, false),
        "sameMembers" => (Runtime("same_members"), false),
        "includeMembers" => (Runtime("includes_members"), false),
        "hasAllKeys" | "hasKeys" => (Runtime("keys_are"), false),
        "oneOf" => (Runtime("one_of"), false),
        _ => return None,
    })
}

/// Which assertion library a local name was imported from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LibraryRoot {
    /// chai's `assert`.
    ChaiAssert,
    /// chai's `expect`.
    ChaiExpect,
    /// The chai module object (`import chai from "chai"`).
    ChaiModule,
    /// `node:assert`.
    NodeAssert,
}

/// How many arguments of `assert.<name>` are expected values: the rest are a
/// failure message, which changes nothing about the outcome.
fn expected_arity(check: LibraryCheck) -> usize {
    match check {
        LibraryCheck::IsBool(_)
        | LibraryCheck::TypeIs(_)
        | LibraryCheck::Exists
        | LibraryCheck::Matcher(
            "toBeNull" | "toBeUndefined" | "toBeNaN" | "toBeTruthy",
        )
        | LibraryCheck::Runtime("is_empty") => 0,
        LibraryCheck::Runtime("close_to" | "within" | "has_property") => 2,
        LibraryCheck::Matcher("toThrow") => 2,
        LibraryCheck::Fail => 1,
        _ => 1,
    }
}

impl ModuleBuilder<'_> {
    /// Whether `name` ends a chai chain.
    pub(in crate::lowering) fn is_chai_terminal(name: &str) -> bool {
        chai_terminal(name, false, false).is_some()
    }

    /// The library that bound `name`, by its import.
    fn library_root(&self, name: &str) -> Option<LibraryRoot> {
        // vitest's `expect` is chai's, extended: `.to.equal` and `.greaterThan` work on it.
        if name == "expect" && self.imports.is_test_builtin(name) {
            return Some(LibraryRoot::ChaiExpect);
        }
        let source = self.imports.import_source(name)?;
        let imported = self.imports.imported_name(name).unwrap_or(name);
        match source {
            "chai" => match imported {
                "assert" => Some(LibraryRoot::ChaiAssert),
                "expect" => Some(LibraryRoot::ChaiExpect),
                "default" | "*" => Some(LibraryRoot::ChaiModule),
                _ => None,
            },
            "assert" | "node:assert" | "assert/strict" | "node:assert/strict" => {
                matches!(imported, "default" | "*" | "strict" | "ok" | "assert")
                    .then_some(LibraryRoot::NodeAssert)
            }
            _ => None,
        }
    }

    /// The library root of `expression`, seeing through `chai.assert`/`chai.expect`.
    fn library_root_of(&self, expression: &Expression<'_>) -> Option<LibraryRoot> {
        match expression.get_inner_expression() {
            Expression::Identifier(identifier) if !self.scope.is_bound(identifier.name.as_str()) => {
                self.library_root(identifier.name.as_str())
            }
            Expression::StaticMemberExpression(member) => {
                match self.library_root_of(&member.object)? {
                    LibraryRoot::ChaiModule => match member.property.name.as_str() {
                        "assert" => Some(LibraryRoot::ChaiAssert),
                        "expect" => Some(LibraryRoot::ChaiExpect),
                        _ => None,
                    },
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Lower a chai or `node:assert` assertion statement.
    ///
    /// Returns `false` when `expression` is neither.
    pub(in crate::lowering) fn assertion_library_statement(
        &mut self,
        expression: &Expression<'_>,
        body: &mut Body,
    ) -> Result<bool, SmeltError> {
        if let Expression::CallExpression(call) = expression
            && self.assert_interface_statement(call, body)?
        {
            return Ok(true);
        }
        self.chai_expect_statement(expression, body)
    }

    /// Lower `assert(value)` and `assert.<method>(...)`.
    fn assert_interface_statement(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<bool, SmeltError> {
        // The callable form is `assert.ok`.
        let (name, chai) = match self.library_root_of(&call.callee) {
            Some(LibraryRoot::ChaiAssert) => ("ok", true),
            Some(LibraryRoot::NodeAssert) => ("ok", false),
            _ => {
                let Expression::StaticMemberExpression(member) = &call.callee else {
                    return Ok(false);
                };
                let chai = match self.library_root_of(&member.object) {
                    Some(LibraryRoot::ChaiAssert) => true,
                    Some(LibraryRoot::NodeAssert) => false,
                    _ => return Ok(false),
                };
                (member.property.name.as_str(), chai)
            }
        };
        let library = if chai { "chai" } else { "node:assert" };
        let Some((check, inverted)) = assert_method(name, chai) else {
            self.unmodelled_assertion(&format!("{library} assert.{name}"), call.span, body);
            return Ok(true);
        };
        if check == LibraryCheck::Fail {
            self.always_failing_assertion(&format!("assert.{name}(...)"), call.span, body);
            return Ok(true);
        }
        let Some(actual) = call.arguments.first() else {
            self.unmodelled_assertion(&format!("{library} assert.{name} without a value"), call.span, body);
            return Ok(true);
        };
        // `assert.throws(fn, Err, "shown on failure")` in Node, but chai's second
        // argument after the class is a message the error must contain.
        let arity = if check == LibraryCheck::Matcher("toThrow") && !chai { 1 } else { expected_arity(check) };
        let end = (1 + arity).min(call.arguments.len());
        let arguments = call.arguments.get(1..end).unwrap_or_default();
        self.library_check(check, Some(actual), None, inverted, arguments, call.span, &format!("assert.{name}(...)"), body)?;
        Ok(true)
    }

    /// Lower a statement that is a chai `expect(...)` chain.
    fn chai_expect_statement(
        &mut self,
        expression: &Expression<'_>,
        body: &mut Body,
    ) -> Result<bool, SmeltError> {
        // From the outside in: the last word of the chain comes first.
        let mut links: Vec<(&str, Option<&[Argument<'_>]>, oxc::span::Span)> = Vec::new();
        let mut cursor = expression.get_inner_expression();
        // `chai.should()` only installs the getter this lowering already models.
        if let Expression::CallExpression(call) = cursor
            && let Expression::StaticMemberExpression(member) = call.callee.get_inner_expression()
            && member.property.name == "should"
            && self.library_root_of(&member.object) == Some(LibraryRoot::ChaiModule)
        {
            return Ok(true);
        }
        let mut should_receiver = None;
        let expect_call = loop {
            match cursor {
                Expression::CallExpression(call) => {
                    if self.library_root_of(&call.callee) == Some(LibraryRoot::ChaiExpect) {
                        break Some(call);
                    }
                    let Expression::StaticMemberExpression(member) = &call.callee else {
                        return Ok(false);
                    };
                    links.push((member.property.name.as_str(), Some(call.arguments.as_slice()), call.span));
                    cursor = member.object.get_inner_expression();
                }
                Expression::StaticMemberExpression(member)
                    if member.property.name == "should" && self.source.contains("chai") =>
                {
                    should_receiver = Some(member.object.get_inner_expression());
                    break None;
                }
                Expression::StaticMemberExpression(member) => {
                    links.push((member.property.name.as_str(), None, member.span));
                    cursor = member.object.get_inner_expression();
                }
                _ => return Ok(false),
            }
        };
        let span = expression.span();
        let vitest_expect = expect_call.is_some();
        let actual = expect_call.and_then(|call| call.arguments.first());
        if actual.is_none() && should_receiver.is_none() {
            self.unmodelled_assertion("chai expect() without a value", span, body);
            return Ok(true);
        }
        links.reverse();
        // A chain may assert more than once (`.to.be.a("string").and.equal("x")`),
        // on one value, so a value that is read more than once gets a local.
        let asserting = links.iter().filter(|(name, args, _)| {
            !CHAI_LANGUAGE_CHAINS.contains(name) && (args.is_some() || !is_chai_flag(name))
        }).count();
        let shared = match (should_receiver, actual) {
            (Some(receiver), _) => Some(self.bind_expression_once(receiver, body)?),
            (None, Some(actual)) if asserting > 1 => Some(self.bind_actual_once(actual, body)?),
            _ => None,
        };
        let (mut inverted, mut deep, mut contains) = (false, false, false);
        for (name, arguments, link_span) in links {
            if arguments.is_none() && CHAI_LANGUAGE_CHAINS.contains(&name) {
                continue;
            }
            if arguments.is_none() {
                match name {
                    "not" => { inverted = true; continue; }
                    "deep" | "nested" => { deep = true; continue; }
                    "include" | "contain" | "includes" | "contains" => { contains = true; continue; }
                    "own" | "ordered" | "any" | "all" | "itself" => continue,
                    _ => {}
                }
            }
            let Some(check) = chai_terminal(name, deep, contains) else {
                self.unmodelled_assertion(&format!("chai .{name}"), link_span, body);
                return Ok(true);
            };
            if check == LibraryCheck::Fail {
                self.always_failing_assertion(&format!("expect(...).{name}(...)"), link_span, body);
                return Ok(true);
            }
            let message = format!("expect(...).{name}(...)");
            self.library_check(check, actual, shared, inverted, arguments.unwrap_or(&[]), link_span, &message, body)?;
        }
        if vitest_expect {
            self.tick_planned_assertion(span, body);
        }
        Ok(true)
    }

    /// Evaluate a `.should` receiver into a local the chain reads.
    fn bind_expression_once(
        &mut self,
        receiver: &Expression<'_>,
        body: &mut Body,
    ) -> Result<ExprId, SmeltError> {
        let value = self.expression(receiver, body)?;
        let ty = Self::expr_ty(body, value);
        let span = self.expression_span(receiver);
        let name = self.intern_source_name("chai_actual");
        let local = body.push_local(LocalDecl { name: Some(name), ty, mutable: false, span });
        let pat = body.push_pattern(Pattern::Binding(local));
        self.push_stmt_to_sink(body, Stmt::Let { pat, ty, value: Some(value) });
        Ok(body.push_expr(Expr { kind: ExprKind::Local(local), ty, span }))
    }

    /// Evaluate an actual value into a local, for a chain that reads it twice.
    fn bind_actual_once(
        &mut self,
        actual: &Argument<'_>,
        body: &mut Body,
    ) -> Result<ExprId, SmeltError> {
        let value = self.argument(actual, body)?;
        let ty = Self::expr_ty(body, value);
        let span = self.span(actual.span().start, actual.span().end);
        let name = self.intern_source_name("chai_actual");
        let local = body.push_local(LocalDecl { name: Some(name), ty, mutable: false, span });
        let pat = body.push_pattern(Pattern::Binding(local));
        self.push_stmt_to_sink(body, Stmt::Let { pat, ty, value: Some(value) });
        Ok(body.push_expr(Expr { kind: ExprKind::Local(local), ty, span }))
    }

    /// Lower one rule against `actual` (or the value already bound in `shared`).
    #[expect(clippy::too_many_arguments, reason = "each is one independent part of the source assertion")]
    fn library_check(
        &mut self,
        check: LibraryCheck,
        actual: Option<&Argument<'_>>,
        shared: Option<ExprId>,
        inverted: bool,
        arguments: &[Argument<'_>],
        span: oxc::span::Span,
        message: &str,
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        if let LibraryCheck::Matcher(name) = check {
            let actual = match (name == "toThrow", shared, actual) {
                (_, _, Some(argument)) if name == "toThrow" => SiteActual::Argument { argument, inverted },
                (_, Some(value), _) => SiteActual::Lowered(LoweredActual { value, inverted }),
                (_, None, Some(argument)) => SiteActual::Argument { argument, inverted },
                (_, None, None) => {
                    self.unmodelled_assertion(&format!("{message} on a `.should` receiver"), span, body);
                    return Ok(());
                }
            };
            let lowered_throw = name == "toThrow" && matches!(actual, SiteActual::Lowered(_));
            let site = MatcherSite { name, arguments, span, actual };
            // A `.should` chain's lowered callee reaches `toThrow` directly; the
            // general dispatch keeps declining a lowered `.resolves` value.
            let handled = if lowered_throw {
                self.expect_to_throw_statement(site, body)?
            } else {
                self.matcher_site_statement(site, body)?
            };
            debug_assert!(handled, "{name} is a modelled matcher");
            return Ok(());
        }
        let value = if let Some(value) = shared {
            value
        } else {
            let Some(actual) = actual else {
                self.unmodelled_assertion(&format!("{message} on a `.should` receiver"), span, body);
                return Ok(());
            };
            self.argument(actual, body)?
        };
        let holds = match check {
            LibraryCheck::LooseEqual | LibraryCheck::StrictEqual => {
                let Some(expected) = arguments.first() else {
                    self.unmodelled_assertion(&format!("{message} without an expected value"), span, body);
                    return Ok(());
                };
                let expected = self.argument(expected, body)?;
                let (value, expected) = self.erase_unrelated_matcher_operands(value, expected, span, body);
                let op = if check == LibraryCheck::LooseEqual { BinOp::JsLooseEq } else { BinOp::JsStrictEq };
                self.comparison_expr(op, value, expected, span, body)
            }
            LibraryCheck::IsBool(wanted) => {
                let bool_ty = self.ctx.krate.types.intern(Type::Bool);
                let hir_span = self.span(span.start, span.end);
                let literal = body.push_expr(Expr { kind: ExprKind::Literal(Literal::Bool(wanted)), ty: bool_ty, span: hir_span });
                let (value, literal) = self.erase_unrelated_matcher_operands(value, literal, span, body);
                self.comparison_expr(BinOp::JsStrictEq, value, literal, span, body)
            }
            LibraryCheck::Exists => {
                let empty = [];
                return self.runtime_check("exists", value, &empty, inverted, span, message, body);
            }
            LibraryCheck::TypeIs(name) => {
                let name = self.string_literal_expr(name, span, body);
                return self.runtime_check("type_is", value, &[name], inverted, span, message, body);
            }
            LibraryCheck::TypeArgument => {
                let Some(type_name) = arguments.first() else {
                    self.unmodelled_assertion(&format!("{message} without a type name"), span, body);
                    return Ok(());
                };
                let name = self.argument(type_name, body)?;
                return self.runtime_check("type_is", value, &[name], inverted, span, message, body);
            }
            LibraryCheck::Runtime(name) => {
                let arguments = arguments
                    .iter()
                    .map(|argument| self.argument(argument, body))
                    .collect::<Result<Vec<_>, _>>()?;
                return self.runtime_check(name, value, &arguments, inverted, span, message, body);
            }
            LibraryCheck::Matcher(_) | LibraryCheck::Fail => return Ok(()),
        };
        self.assertion_holds(holds, inverted, message, span, body);
        Ok(())
    }

    /// Fail unless the erased runtime check `name` holds for `actual`.
    #[expect(clippy::too_many_arguments, reason = "each is one independent part of the source assertion")]
    pub(in crate::lowering) fn runtime_check(
        &mut self,
        name: &str,
        actual: ExprId,
        arguments: &[ExprId],
        inverted: bool,
        span: oxc::span::Span,
        message: &str,
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        let holds = self.runtime_check_expr(name, actual, arguments, span, body);
        self.assertion_holds(holds, inverted, message, span, body);
        Ok(())
    }

    /// The boolean answer of the erased runtime check `name`.
    pub(in crate::lowering) fn runtime_check_expr(
        &mut self,
        name: &str,
        actual: ExprId,
        arguments: &[ExprId],
        span: oxc::span::Span,
        body: &mut Body,
    ) -> ExprId {
        let hir_span = self.span(span.start, span.end);
        let unknown_ty = self.erase_with(ErasureReason::TestHarness, "assertion library check");
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let mut args = vec![Self::erase_to_unknown(actual, unknown_ty, hir_span, body)];
        args.extend(arguments.iter().map(|argument| Self::erase_to_unknown(*argument, unknown_ty, hir_span, body)));
        body.push_expr(Expr {
            kind: ExprKind::TestRuntimeCheck { check: name.to_owned(), args },
            ty: bool_ty,
            span: hir_span,
        })
    }

    /// Throw `message` unless `holds` agrees with the assertion's polarity.
    pub(in crate::lowering) fn assertion_holds(
        &mut self,
        holds: ExprId,
        inverted: bool,
        message: &str,
        span: oxc::span::Span,
        body: &mut Body,
    ) {
        let failed = if inverted { holds } else { self.unary_bool_expr(UnaryOp::Not, holds, span, body) };
        self.push_test_failure_if(failed, &format!("{message} failed"), span, body);
    }

    /// An assertion that fails whenever it runs (`assert.fail`).
    pub(in crate::lowering) fn always_failing_assertion(
        &mut self,
        message: &str,
        span: oxc::span::Span,
        body: &mut Body,
    ) {
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let hir_span = self.span(span.start, span.end);
        let always = body.push_expr(Expr { kind: ExprKind::Literal(Literal::Bool(true)), ty: bool_ty, span: hir_span });
        self.push_test_failure_if(always, message, span, body);
    }

    /// Report an assertion this model has no rule for by failing where it runs.
    pub(in crate::lowering) fn unmodelled_assertion(
        &mut self,
        what: &str,
        span: oxc::span::Span,
        body: &mut Body,
    ) {
        self.always_failing_assertion(&format!("{what} is not modelled"), span, body);
    }
}

/// Whether a member word, without a call, sets a flag rather than asserting.
fn is_chai_flag(name: &str) -> bool {
    matches!(name, "not" | "deep" | "nested" | "include" | "contain" | "includes" | "contains" | "own" | "ordered" | "any" | "all" | "itself")
}

impl ModuleBuilder<'_> {
    /// Whether `name` is `fast-check`'s module object or its `fc` re-export.
    pub(in crate::lowering) fn is_fast_check_namespace(&self, name: &str) -> bool {
        if self.scope.is_bound(name) {
            return false;
        }
        match self.imports.import_source(name) {
            Some("fast-check") => self.imports.imported_name(name).is_some_and(|imported| matches!(imported, "default" | "*" | "fc")),
            Some("@fast-check/vitest") => self.imports.imported_name(name) == Some("fc"),
            _ => false,
        }
    }

    /// Whether `expression` is built from the `fc` object, so a `.map` on it is
    /// the arbitrary's own method rather than `Array.prototype.map`.
    pub(in crate::lowering) fn is_fast_check_value(&self, expression: &Expression<'_>) -> bool {
        match expression.get_inner_expression() {
            Expression::Identifier(identifier) => self.is_fast_check_namespace(identifier.name.as_str()),
            Expression::CallExpression(call) => self.is_fast_check_value(&call.callee),
            Expression::StaticMemberExpression(member) => self.is_fast_check_value(&member.object),
            _ => false,
        }
    }

    /// Lower `test.prop(arbitraries, parameters)(name, predicate)` to a run of
    /// the property.
    pub(in crate::lowering) fn property_test_statement(
        &mut self,
        prop_call: &CallExpression<'_>,
        predicate: &Argument<'_>,
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        let span = self.span(prop_call.span.start, prop_call.span.end);
        let Some(arbitraries) = prop_call.arguments.first() else {
            self.unmodelled_assertion("test.prop without arbitraries", prop_call.span, body);
            return Ok(());
        };
        let unknown_ty = self.erase_with(ErasureReason::TestHarness, "test.prop");
        let arbitraries = self.argument_with_hint(arbitraries, body, Some(unknown_ty))?;
        let arbitraries = Self::erase_to_unknown(arbitraries, unknown_ty, span, body);
        let predicate = self.argument(predicate, body)?;
        let predicate = Self::erase_to_unknown(predicate, unknown_ty, span, body);
        let parameters = prop_call
            .arguments
            .get(1)
            .map(|parameters| {
                let value = self.argument_with_hint(parameters, body, Some(unknown_ty))?;
                Ok::<_, SmeltError>(Self::erase_to_unknown(value, unknown_ty, span, body))
            })
            .transpose()?;
        let none = self.ctx.krate.types.intern(Type::None);
        let run = body.push_expr(Expr {
            kind: ExprKind::FastCheckRun { arbitraries, predicate, parameters },
            ty: none,
            span,
        });
        self.push_stmt_to_sink(body, Stmt::Expr(run));
        Ok(())
    }
}
