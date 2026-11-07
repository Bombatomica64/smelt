//! `String.prototype` methods and `String.fromCharCode`/`fromCodePoint`.

use crate::SmeltError;
use crate::lowering::{Literal, ModuleBuilder, Span, StringPadOp};
use crate::lowering::js_text::js_str_text;
use oxc::ast::ast::{Argument, Expression};
use oxc::span::GetSpan;
use smelt_hir::{
    Body, ErasureReason, Expr, ExprKind, PrimitiveCastOp, StringAffixOp, StringCaseOp,
    StringNormalizeForm, StringReplaceOp, StringSearchOp, StringTrimSide, Type,
};

impl ModuleBuilder<'_> {
    /// Lower direct TypeScript string case methods.
    pub(in crate::lowering) fn string_case_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "toLowerCase" | "toLocaleLowerCase" => StringCaseOp::Lower,
            "toUpperCase" | "toLocaleUpperCase" => StringCaseOp::Upper,
            _ => return Ok(None),
        };
        if call.arguments.is_empty() {
            let operand = self.expression(&member.object, body)?;
            if call.optional || member.optional {
                let operand = self.optionalize_index_receiver(operand, body);
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let ty = self.optional_chain_result_type(string_ty);
                let method = self.intern_source_name(member.property.name.as_str());
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::OptionalMethod {
                        receiver: operand,
                        method,
                        args: Vec::new(),
                    },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })));
            }
            let operand_ty = Self::expr_ty(body, operand);
            let effective_operand_ty = self.type_param_constraint_or_self(operand_ty);
            if self.ctx.krate.types.get(effective_operand_ty) == Some(&Type::String) {
                let ty = self.ctx.krate.types.intern(Type::String);
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::StringCase { op, operand },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })));
            }
            if self.string_method_erased_receiver(effective_operand_ty) {
                let ty = self.ctx.krate.types.intern(Type::String);
                let operand = body.push_expr(Expr {
                    kind: ExprKind::UnknownCast {
                        value: operand,
                        target: ty,
                    },
                    ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::StringCase { op, operand },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })));
            }
            if self.allows_unknown_index_access() {
                let ty = self.ctx.krate.types.intern(Type::String);
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::StringCase { op, operand },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })));
            }
        }
        Err(SmeltError::unsupported(
            self.span(call.span.start, call.span.end),
            "string case methods require a string receiver and no arguments",
        ))
    }

    /// Return whether an erased receiver can be treated as a string method target.
    pub(in crate::lowering) fn string_method_erased_receiver(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Class { .. }) => true,
            // `string | undefined` surfaces (e.g. `const [first] = words`)
            // come from tsc-checked positions where the payload is a string;
            // treat the optional wrapper as an erased string surface too.
            Some(Type::Optional(inner)) => {
                self.string_method_erased_receiver(*inner)
                    || self.ctx.krate.types.get(*inner) == Some(&Type::String)
            }
            Some(Type::Union(items)) => items.iter().any(|item| {
                let item = self.type_param_constraint_or_self(*item);
                matches!(
                    self.ctx.krate.types.get(item),
                    Some(Type::String | Type::Unknown | Type::TypeParam { .. } | Type::Class { .. })
                )
            }),
            _ => false,
        }
    }

    /// Lower direct TypeScript Unicode string normalization.
    pub(in crate::lowering) fn string_normalize_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "normalize" {
            return Ok(None);
        }
        let form = match call.arguments.as_slice() {
            [] => StringNormalizeForm::Nfc,
            [Argument::StringLiteral(literal)] => match js_str_text(literal.value).as_ref() {
                "NFC" => StringNormalizeForm::Nfc,
                "NFD" => StringNormalizeForm::Nfd,
                "NFKC" => StringNormalizeForm::Nfkc,
                "NFKD" => StringNormalizeForm::Nfkd,
                _ => {
                    return Err(SmeltError::unsupported(
                        self.span(literal.span.start, literal.span.end),
                        "string normalize form must be NFC, NFD, NFKC, or NFKD",
                    ));
                }
            },
            [_] => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "string normalize requires a literal normalization form",
                ));
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "string normalize accepts at most one argument",
                ));
            }
        };
        let operand = self.expression(&member.object, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        if !(self.is_string_compatible_type(operand_ty) || self.type_contains_unknown(operand_ty)) {
            return Err(SmeltError::unsupported(
                self.span(member.object.span().start, member.object.span().end),
                "string normalize requires a string receiver",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringNormalize { form, operand },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower JavaScript `receiver.localeCompare(other)`.
    ///
    /// Only the zero-argument-locale form is lowered: `locales` and `options`
    /// select CLDR collation data Smelt does not carry, so a call that passes
    /// them defers to the ordinary dynamic member path rather than silently
    /// answering with the default collation. Both operands must be string-like;
    /// the result is a `number`, so `xs.sort((a, b) => a.localeCompare(b))`
    /// type-checks as an ordinary comparator.
    pub(in crate::lowering) fn string_locale_compare_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "localeCompare" {
            return Ok(None);
        }
        let [right_argument] = call.arguments.as_slice() else {
            return Ok(None);
        };
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let mut left = self.expression(&member.object, body)?;
        let left_ty = Self::expr_ty(body, left);
        if !(self.is_string_compatible_type(left_ty) || self.type_contains_unknown(left_ty)) {
            return Ok(None);
        }
        left = body.push_expr(Expr {
            kind: ExprKind::TypeAssert { value: left },
            ty: string_ty,
            span: self.span(member.object.span().start, member.object.span().end),
        });
        let mut right = self.argument(right_argument, body)?;
        let right_ty = Self::expr_ty(body, right);
        if !(self.is_string_compatible_type(right_ty) || self.type_contains_unknown(right_ty)) {
            return Err(SmeltError::unsupported(
                self.span(right_argument.span().start, right_argument.span().end),
                "localeCompare requires a string argument",
            ));
        }
        right = body.push_expr(Expr {
            kind: ExprKind::TypeAssert { value: right },
            ty: string_ty,
            span: self.span(right_argument.span().start, right_argument.span().end),
        });
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringLocaleCompare { left, right },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript string trimming.
    pub(in crate::lowering) fn string_trim_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let side = match member.property.name.as_str() {
            "trim" => StringTrimSide::Both,
            "trimStart" => StringTrimSide::Start,
            "trimEnd" => StringTrimSide::End,
            _ => return Ok(None),
        };
        if !call.arguments.is_empty() {
            return Ok(None);
        }
        let mut operand = self.expression(&member.object, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        if self.ctx.krate.types.get(operand_ty) != Some(&Type::String) {
            // Coerce a string-compatible receiver (e.g. the return of a user
            // `toString`-like helper typed `unknown`/generic) to `String`.
            if self.is_string_compatible_type(operand_ty) {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                operand = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: operand },
                    ty: string_ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
            } else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "string trim requires a string receiver",
                ));
            }
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringTrim { side, operand },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript string prefix and suffix tests.
    pub(in crate::lowering) fn string_affix_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "startsWith" => StringAffixOp::StartsWith,
            "endsWith" => StringAffixOp::EndsWith,
            _ => return Ok(None),
        };
        if !(1..=2).contains(&call.arguments.len()) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string prefix/suffix methods require one needle and an optional position argument",
            ));
        }
        let mark = self.speculation_mark(body);
        let mut haystack = self.expression(&member.object, body)?;
        let method = self.intern_source_name(member.property.name.as_str());
        // An erased receiver the checker types as something other than a
        // string (ts-pattern's `P.string`, a namespace member) is not a string.
        let haystack_ty = Self::expr_ty(body, haystack);
        if self.receiver_owns_member(haystack_ty, method)
            || (self.type_contains_unknown(haystack_ty)
                && self.checker_types_as_non_string(member.object.span().start, member.object.span().end))
        {
            self.roll_back_to(mark, body);
            return Ok(None);
        }
        let Some(needle_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string prefix/suffix methods require one needle and an optional position argument",
            ));
        };
        let mut needle = self.argument(needle_argument, body)?;
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let haystack_ty = Self::expr_ty(body, haystack);
        if self.is_string_compatible_type(haystack_ty) || self.type_contains_unknown(haystack_ty)
        {
            haystack = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: haystack },
                ty: string_ty,
                span: self.span(member.object.span().start, member.object.span().end),
            });
        }
        // JavaScript `startsWith(target, position)` tests the prefix starting at
        // `position`; `endsWith(target, endPosition)` tests the suffix of the
        // string truncated to `endPosition`. Both reduce to a substring of the
        // haystack followed by the unbounded affix test.
        if let Some(position_argument) = call.arguments.get(1) {
            let position = self.argument(position_argument, body)?;
            let position_ty = Self::expr_ty(body, position);
            let position = if self.ctx.krate.types.get(position_ty) == Some(&Type::Float) {
                Some(position)
            } else if self.is_numeric_like_type(position_ty)
                || self.optional_numeric_surface(position_ty)
                || self.erased_or_union_surface(position_ty)
            {
                let float_ty = self.ctx.krate.types.intern(Type::Float);
                Some(body.push_expr(Expr {
                    kind: ExprKind::PrimitiveCast {
                        op: PrimitiveCastOp::ToJsNumber,
                        operand: position,
                    },
                    ty: float_ty,
                    span: self.span(position_argument.span().start, position_argument.span().end),
                }))
            } else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "string prefix/suffix position argument must be numeric",
                ));
            };
            let (start, end) = match op {
                StringAffixOp::StartsWith => (position, None),
                StringAffixOp::EndsWith => (None, position),
            };
            haystack = body.push_expr(Expr {
                // A position clamps to `[0, length]`, as `substring` bounds do.
                kind: ExprKind::StringSlice {
                    operand: haystack,
                    start,
                    end,
                    kind: smelt_hir::StringSliceKind::Substring,
                },
                ty: string_ty,
                span: self.span(member.object.span().start, member.object.span().end),
            });
        }
        let needle_ty = Self::expr_ty(body, needle);
        if self.is_string_compatible_type(needle_ty) || self.type_contains_unknown(needle_ty)
        {
            needle = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: needle },
                ty: string_ty,
                span: self.span(needle_argument.span().start, needle_argument.span().end),
            });
        }
        if self.ctx.krate.types.get(Self::expr_ty(body, haystack)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, needle)) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string prefix/suffix methods require string receiver and argument",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringAffix {
                op,
                haystack,
                needle,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower `string.search(pattern)` on a concrete string receiver.
    ///
    /// Not modeled before: the call fell through to a member read of `search`
    /// on the string and answered `undefined`, so `str.search(escapeRe) === -1`
    /// was never true and Hono's HTML escaper returned its input unescaped. The
    /// pattern is a `RegExp` or a string (compiled as a pattern at run time).
    fn string_regex_search_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let [pattern_argument] = call.arguments.as_slice() else {
            return Ok(None);
        };
        let haystack = self.expression(&member.object, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, haystack)) != Some(&Type::String) {
            return Ok(None);
        }
        let needle = self.argument(pattern_argument, body)?;
        let needle_ty = Self::expr_ty(body, needle);
        let needle_is_pattern = self.ctx.krate.types.get(needle_ty) == Some(&Type::String)
            || self.stdlib_class_of_type(needle_ty) == Some(smelt_stdlib::StdlibClass::RegExp);
        if !needle_is_pattern {
            return Ok(None);
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringSearch {
                op: StringSearchOp::Regex,
                haystack,
                needle,
                from_index: None,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript string search methods.
    pub(in crate::lowering) fn string_search_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "indexOf" => StringSearchOp::Find,
            "lastIndexOf" => StringSearchOp::RFind,
            "search" => return self.string_regex_search_call(call, member, body),
            _ => return Ok(None),
        };
        if !(1..=2).contains(&call.arguments.len()) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string search requires one needle and an optional fromIndex argument",
            ));
        }
        let haystack = self.expression(&member.object, body)?;
        let Some(needle_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string search requires one needle and an optional fromIndex argument",
            ));
        };
        let needle = self.argument(needle_argument, body)?;
        let from_index = if let Some(from_index_argument) = call.arguments.get(1) {
            let from_index = self.argument(from_index_argument, body)?;
            if !self.slice_index_type_is_number(Self::expr_ty(body, from_index)) {
                return Err(SmeltError::unsupported(
                    self.span(from_index_argument.span().start, from_index_argument.span().end),
                    "string search fromIndex must be numeric",
                ));
            }
            Some(from_index)
        } else {
            None
        };
        if self.ctx.krate.types.get(Self::expr_ty(body, haystack)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, needle)) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string search methods require string receiver and argument",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringSearch {
                op,
                haystack,
                needle,
                from_index,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower `String.prototype.matchAll` to a match-record list surface.
    pub(in crate::lowering) fn string_match_all_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "matchAll" {
            return Ok(None);
        }
        let [pattern_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string matchAll requires one RegExp argument",
            ));
        };
        let mut haystack = self.expression(&member.object, body)?;
        let haystack_ty = Self::expr_ty(body, haystack);
        if self.ctx.krate.types.get(haystack_ty) != Some(&Type::String) {
            // Coerce string-compatible surfaces (optional/defaulted params,
            // erased generics, unions with a string arm) to `String` like the
            // other string-method receivers do.
            if self.is_string_compatible_type(haystack_ty)
                || self.string_method_erased_receiver(haystack_ty)
            {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                haystack = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: haystack },
                    ty: string_ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
            } else {
                return Err(SmeltError::unsupported(
                    self.span(member.object.span().start, member.object.span().end),
                    "string matchAll requires a string receiver",
                ));
            }
        }
        let regex = self.argument(pattern_argument, body)?;
        // Each match is a concrete JavaScript match value (numbered groups,
        // `groups`, `index`, `input`). Typing the list element as the synthetic
        // match class lets consumer reads resolve to typed `SmeltMatch`
        // accessors instead of the erased `SmeltUnknown` boundary.
        let match_ty = self.match_result_type();
        let ty = self.ctx.krate.types.intern(Type::List(match_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::RegexMatchAll { regex, haystack },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript literal string replacement.
    pub(in crate::lowering) fn string_replace_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        // `replace` substitutes the first match; `replaceAll` substitutes every
        // match. With a plain string search value both are literal substring
        // replacements (Rust `str::replacen`/`str::replace`); regex-pattern
        // forms are handled earlier by `regex_replace_call`.
        let op = match member.property.name.as_str() {
            "replace" => StringReplaceOp::First,
            "replaceAll" => StringReplaceOp::All,
            _ => return Ok(None),
        };
        if self.imported_utility_object(&member.object) {
            return Ok(None);
        }
        let [pattern_argument, replacement_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string replace requires exactly two string arguments",
            ));
        };
        let haystack = self.expression(&member.object, body)?;
        let pattern = self.argument(pattern_argument, body)?;
        let replacement = self.argument(replacement_argument, body)?;
        let haystack_ty = Self::expr_ty(body, haystack);
        let pattern_ty = Self::expr_ty(body, pattern);
        let replacement_ty = Self::expr_ty(body, replacement);
        if !(self.is_string_compatible_type(haystack_ty) || self.type_contains_unknown(haystack_ty))
            || !self.is_string_compatible_type(pattern_ty)
            || !self.is_string_compatible_type(replacement_ty)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string replace requires string-compatible receiver, pattern, and replacement",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringReplace {
                op,
                haystack,
                pattern,
                replacement,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript string repetition.
    pub(in crate::lowering) fn string_repeat_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "repeat" {
            return Ok(None);
        }
        let [count_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string repeat requires exactly one number argument",
            ));
        };
        let span = self.span(call.span.start, call.span.end);
        let mut operand = self.expression(&member.object, body)?;
        let mut count = self.argument(count_argument, body)?;
        // Coerce a string-compatible receiver to `String` and a numeric-like
        // count to a JS number rather than requiring exact `String`/`Float`.
        let operand_ty = Self::expr_ty(body, operand);
        if self.ctx.krate.types.get(operand_ty) != Some(&Type::String)
            && self.is_string_compatible_type(operand_ty)
        {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            operand = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: operand },
                ty: string_ty,
                span,
            });
        }
        let count_ty = Self::expr_ty(body, count);
        if self.ctx.krate.types.get(count_ty) != Some(&Type::Float)
            && (self.is_numeric_like_type(count_ty)
                || self.optional_numeric_surface(count_ty)
                || self.erased_or_union_surface(count_ty))
        {
            let float_ty = self.ctx.krate.types.intern(Type::Float);
            count = body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToJsNumber,
                    operand: count,
                },
                ty: float_ty,
                span,
            });
        }
        if self.ctx.krate.types.get(Self::expr_ty(body, operand)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, count)) != Some(&Type::Float)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string repeat requires a string receiver and number argument",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringRepeat { operand, count },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Coerce a padding operand (receiver or pad string) to `Type::String`.
    ///
    /// `String.prototype.padStart`/`padEnd` receivers and pad arguments commonly
    /// arrive as `toString(...)` returns or erased/optional surfaces. A value
    /// already typed `String` is returned unchanged; a string-compatible surface
    /// is converted with a JS `ToString` cast so the runtime padding sees a
    /// concrete string.
    pub(in crate::lowering) fn coerce_pad_string_operand(
        &mut self,
        operand: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let operand_ty = Self::expr_ty(body, operand);
        if self.ctx.krate.types.get(operand_ty) == Some(&Type::String) {
            return Ok(operand);
        }
        if self.is_string_compatible_type(operand_ty) {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToString,
                    operand,
                },
                ty: string_ty,
                span,
            }));
        }
        Ok(operand)
    }

    /// Lower supported string padding calls into HIR string runtime calls.
    pub(in crate::lowering) fn string_pad_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "padStart" => StringPadOp::Start,
            "padEnd" => StringPadOp::End,
            _ => return Ok(None),
        };
        if !(1..=3).contains(&call.arguments.len()) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string padding requires target length and optional string padding",
            ));
        }
        let (operand, target_argument, pad_argument) = if call.arguments.len() == 3 {
            let Some(operand_argument) = call.arguments.first() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "string padding requires a receiver argument",
                ));
            };
            let operand = self.argument(operand_argument, body)?;
            (operand, call.arguments.get(1), call.arguments.get(2))
        } else {
            (
                self.expression(&member.object, body)?,
                call.arguments.first(),
                call.arguments.get(1),
            )
        };
        let Some(target_argument) = target_argument else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string padding requires target length",
            ));
        };
        let target_len = self.argument(target_argument, body)?;
        let pad = if let Some(pad_argument) = pad_argument {
            self.argument(pad_argument, body)?
        } else {
            let ty = self.ctx.krate.types.intern(Type::String);
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(" ".to_owned())),
                ty,
                span: self.span(call.span.start, call.span.end),
            })
        };
        let span = self.span(call.span.start, call.span.end);
        // Coerce the receiver/pad to `String` and the target length to a number
        // when they arrive as string/numeric-compatible or erased surfaces
        // (e.g. `length = 0` numeric defaults, `toString(...)` returns), instead
        // of requiring exact `String`/`Float` types.
        let operand = self.coerce_pad_string_operand(operand, span, body)?;
        let pad = self.coerce_pad_string_operand(pad, span, body)?;
        let target_len = {
            let target_ty = Self::expr_ty(body, target_len);
            if self.ctx.krate.types.get(target_ty) == Some(&Type::Float) {
                target_len
            } else if self.is_numeric_like_type(target_ty)
                || self.optional_numeric_surface(target_ty)
                || self.erased_or_union_surface(target_ty)
            {
                let float_ty = self.ctx.krate.types.intern(Type::Float);
                body.push_expr(Expr {
                    kind: ExprKind::PrimitiveCast {
                        op: PrimitiveCastOp::ToJsNumber,
                        operand: target_len,
                    },
                    ty: float_ty,
                    span,
                })
            } else {
                return Err(SmeltError::unsupported(
                    span,
                    "string padding requires a string receiver, number target length, and string padding",
                ));
            }
        };
        if self.ctx.krate.types.get(Self::expr_ty(body, operand)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, pad)) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                span,
                "string padding requires a string receiver, number target length, and string padding",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringPad {
                op,
                operand,
                target_len,
                pad,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `String.prototype.charAt`, `charCodeAt` and
    /// `codePointAt` (a number, or `undefined` past the end).
    pub(in crate::lowering) fn string_char_at_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let method = member.property.name.as_str();
        if !matches!(method, "charAt" | "charCodeAt" | "codePointAt") {
            return Ok(None);
        }
        let [index_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string charAt/charCodeAt requires exactly one number argument",
            ));
        };
        let span = self.span(call.span.start, call.span.end);
        let mut operand = self.expression(&member.object, body)?;
        let mut index = self.argument(index_argument, body)?;
        // Coerce a string-compatible receiver (e.g. `T extends string` generic)
        // to `String` and a numeric-like index to a JS number, instead of
        // requiring exact `String`/`Float` types.
        let operand_ty = Self::expr_ty(body, operand);
        if self.ctx.krate.types.get(operand_ty) != Some(&Type::String)
            && self.is_string_compatible_type(operand_ty)
        {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            operand = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: operand },
                ty: string_ty,
                span,
            });
        }
        let index_ty = Self::expr_ty(body, index);
        if self.ctx.krate.types.get(index_ty) != Some(&Type::Float)
            && (self.is_numeric_like_type(index_ty)
                || self.optional_numeric_surface(index_ty)
                || self.erased_or_union_surface(index_ty))
        {
            let float_ty = self.ctx.krate.types.intern(Type::Float);
            index = body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToJsNumber,
                    operand: index,
                },
                ty: float_ty,
                span,
            });
        }
        if self.ctx.krate.types.get(Self::expr_ty(body, operand)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, index)) != Some(&Type::Float)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string charAt/charCodeAt requires a string receiver and number argument",
            ));
        }
        let (kind, ty) = match method {
            "charCodeAt" => (ExprKind::StringCharCodeAt { operand, index }, Type::Float),
            "codePointAt" => {
                let float_ty = self.ctx.krate.types.intern(Type::Float);
                (ExprKind::StringCodePointAt { operand, index }, Type::Optional(float_ty))
            }
            _ => (ExprKind::StringCharAt { operand, index }, Type::String),
        };
        let ty = self.ctx.krate.types.intern(ty);
        Ok(Some(body.push_expr(Expr {
            kind,
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.prototype.join` for string arrays.
    pub(in crate::lowering) fn string_join_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "join" {
            return Ok(None);
        }
        // `Class.join(..)` on a class that declares a static `join` is that
        // method, not `Array.prototype.join`.
        if self.static_member_is_class_static_method(member) {
            return Ok(None);
        }
        if let Expression::Identifier(object) = &member.object
            && (self.imports.is_namespace(object.name.as_str())
                || self.imports.is_value(object.name.as_str()))
        {
            if call.arguments.is_empty() || call.arguments.len() > 2 {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "static array join requires array and optional string separator arguments",
                ));
            }
            let Some(items_argument) = call.arguments.first() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "static array join requires array and optional string separator arguments",
                ));
            };
            let items = self.argument(items_argument, body)?;
            let separator = call.arguments.get(1);
            return self.finish_string_join_call(call, items, separator, body);
        }
        let items = self.expression(&member.object, body)?;
        let separator = match call.arguments.as_slice() {
            [] => None,
            [separator_argument] => Some(separator_argument),
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "array join supports zero or one string separator argument",
                ));
            }
        };
        self.finish_string_join_call(call, items, separator, body)
    }

    /// Finish array join lowering after receiver-style or helper-style arguments are known.
    pub(in crate::lowering) fn finish_string_join_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        mut items: smelt_hir::ExprId,
        separator_argument: Option<&Argument<'_>>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let items_ty = self.type_param_constraint_or_self(Self::expr_ty(body, items));
        let items_ty = match self.ctx.krate.types.get(items_ty) {
            Some(Type::List(_)) => items_ty,
            Some(Type::Unknown | Type::TypeParam { .. }) => {
                // An already-erased receiver was recorded where it was decided.
                let item_ty = if self.ctx.krate.types.get(items_ty) == Some(&Type::Unknown) {
                    self.unknown_type()
                } else {
                    self.erase(ErasureReason::UnfilledGeneric)
                };
                let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                items = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: items },
                    ty: list_ty,
                    span: self.span(call.span.start, call.span.end),
                });
                list_ty
            }
            Some(Type::Union(members))
                if members.iter().any(|union_member| {
                    matches!(
                        self.ctx.krate.types.get(*union_member),
                        Some(Type::List(_) | Type::Unknown | Type::TypeParam { .. })
                    )
                }) =>
            {
                let item_ty = self.erase(ErasureReason::ByName);
                let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                items = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: items },
                    ty: list_ty,
                    span: self.span(call.span.start, call.span.end),
                });
                list_ty
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "array join requires an array receiver",
                ));
            }
        };
        let Some(Type::List(item_ty)) = self.ctx.krate.types.get(items_ty) else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array join requires an array receiver",
            ));
        };
        if !self.array_join_item_type_supported(*item_ty) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array join currently requires primitive or unknown array items",
            ));
        }
        let separator = match separator_argument {
            None => body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(",".to_owned())),
                ty: string_ty,
                span: self.span(call.span.start, call.span.end),
            }),
            Some(separator_argument) => {
                let separator = self.argument(separator_argument, body)?;
                let separator_ty = Self::expr_ty(body, separator);
                if !self.is_string_compatible_type(separator_ty)
                    && !self.type_contains_unknown(separator_ty)
                    && !self.erased_or_union_surface(separator_ty)
                {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "array join separator must be a string",
                    ));
                }
                separator
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringJoin { items, separator },
            ty: string_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return whether `Array.prototype.join` can stringify this lowered item type.
    pub(in crate::lowering) fn array_join_item_type_supported(&self, item_ty: smelt_hir::TypeId) -> bool {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(item_ty))
        {
            Some(
                Type::Bool
                | Type::String
                | Type::Int
                | Type::Float
                | Type::Unknown
                | Type::TypeParam { .. },
            ) => true,
            Some(Type::Optional(item)) => self.array_join_item_type_supported(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .all(|item| self.array_join_item_type_supported(item)),
            _ => false,
        }
    }

    /// Lower direct TypeScript string containment.
    pub(in crate::lowering) fn string_contains_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "includes" {
            return Ok(None);
        }
        // `ns.includes(collection, value, fromIndex?)` on a utility *namespace*
        // object (a `import * as _` star import or a registered object namespace)
        // is the lodash/es-toolkit free-function `includes`, whose second
        // argument is the searched value, not a numeric start position. It is not
        // `String.prototype.includes(needle, position?)`, so defer to the generic
        // namespace member-call path instead of coercing the value argument into a
        // position and rejecting a non-numeric one.
        if self.imported_utility_object(&member.object) {
            return Ok(None);
        }
        // JavaScript `String.prototype.includes(needle, position?)` takes the
        // needle plus an optional numeric start position handled by the emitter.
        if !(1..=2).contains(&call.arguments.len()) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string includes requires a needle and an optional position argument",
            ));
        }
        let mut haystack = self.expression(&member.object, body)?;
        let Some(needle_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string includes requires a needle and an optional position argument",
            ));
        };
        let mut needle = self.argument(needle_argument, body)?;
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let haystack_ty = Self::expr_ty(body, haystack);
        if self.list_surface_type(haystack_ty).is_some() {
            return Ok(None);
        }
        // An ERASED receiver the checker narrowed to an array is not a string:
        // `typeof o === 'string' ? .. : typeof o === 'function' ? .. :
        // o.includes(x)` over an untyped parameter (Hono's CORS `origin`). The
        // call is left to the runtime `includes`, which dispatches on the
        // value; asserting it to `String` searched `"[object Object]"`.
        // The same holds for any erased receiver the checker types as something
        // that cannot be a string (ts-pattern's `P.string` pattern object), and
        // for a receiver whose own type declares `includes`.
        let (start, end) = (member.object.span().start, member.object.span().end);
        let method = self.intern_source_name("includes");
        if (self.type_contains_unknown(haystack_ty)
            && (self.checker_types_as_array(start, end) || self.checker_types_as_non_string(start, end)))
            || self.receiver_owns_member(haystack_ty, method)
        {
            return Ok(None);
        }
        // A union of concrete members with a string arm (`string | fn` after a
        // `typeof path === 'string'` guard Smelt's erased locals do not
        // re-type) is also asserted down to `String`.
        if self.ctx.krate.types.get(haystack_ty) != Some(&Type::String)
            && (self.type_contains_unknown(haystack_ty)
                || self.erased_or_union_surface(haystack_ty)
                || self.is_string_compatible_type(haystack_ty))
        {
            haystack = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: haystack },
                ty: string_ty,
                span: self.span(member.object.span().start, member.object.span().end),
            });
        }
        let needle_ty = Self::expr_ty(body, needle);
        // A needle typed by a parameter bounded by `string` (`substr extends
        // string`) is a string, as the haystack's rule above already says.
        if self.ctx.krate.types.get(needle_ty) != Some(&Type::String)
            && (self.type_contains_unknown(needle_ty)
                || self.erased_or_union_surface(needle_ty)
                || self.is_string_compatible_type(needle_ty))
        {
            needle = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: needle },
                ty: string_ty,
                span: self.span(needle_argument.span().start, needle_argument.span().end),
            });
        }
        let coerced_haystack_ty = Self::expr_ty(body, haystack);
        let coerced_needle_ty = Self::expr_ty(body, needle);
        if self.ctx.krate.types.get(coerced_haystack_ty) != Some(&Type::String)
            || self.ctx.krate.types.get(coerced_needle_ty) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string includes requires string receiver and argument",
            ));
        }
        let from_index = if let Some(position_argument) = call.arguments.get(1) {
            let position = self.argument(position_argument, body)?;
            if !self.slice_index_type_is_number(Self::expr_ty(body, position)) {
                return Err(SmeltError::unsupported(
                    self.span(
                        position_argument.span().start,
                        position_argument.span().end,
                    ),
                    "string includes position must be numeric",
                ));
            }
            Some(position)
        } else {
            None
        };
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringContains {
                haystack,
                needle,
                from_index,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower `String.fromCharCode(...codes)` / `String.fromCodePoint(...points)`.
    ///
    /// The arguments are gathered into one numeric list (each taken through
    /// `ToNumber` first, as the spec does, so an erased argument such as a
    /// typed-array element read still yields its number) and converted by a
    /// single primitive cast. Before this the call was not recognized at all:
    /// `String` resolved to its erased host value and the call answered
    /// `undefined`, so `binary += String.fromCharCode(bytes[i])` built the text
    /// `"nullnull.."` (every Hono base64 encoder test).
    pub(in crate::lowering) fn string_from_codes_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "fromCharCode" => PrimitiveCastOp::FromCharCodes,
            "fromCodePoint" => PrimitiveCastOp::FromCodePoints,
            _ => return Ok(None),
        };
        let span = self.span(call.span.start, call.span.end);
        // One spread of a code list or a typed array (`String.fromCharCode(
        // ...new Uint8Array(signature))`, how base64 encoders turn bytes into
        // a binary string): the spread operand IS the code list. An erased
        // operand is read as a number list, as `[...x]` reads it. The operand
        // is lowered once and always used, so its effects run exactly once.
        if let [Argument::SpreadElement(spread)] = call.arguments.as_slice() {
            let operand = self.expression(&spread.argument, body)?;
            let operand_ty = Self::expr_ty(body, operand);
            let float_ty = self.ctx.krate.types.intern(Type::Float);
            let codes_ty = self.ctx.krate.types.intern(Type::List(float_ty));
            let operand = if operand_ty == codes_ty || self.is_modeled_class(operand_ty, smelt_stdlib::StdlibClass::TypedArray) {
                operand
            } else if matches!(
                self.ctx.krate.types.get(operand_ty),
                Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. } | Type::List(_))
            ) {
                body.push_expr(Expr {
                    kind: ExprKind::UnknownCast {
                        value: operand,
                        target: codes_ty,
                    },
                    ty: codes_ty,
                    span,
                })
            } else {
                return Err(SmeltError::unsupported(
                    span,
                    "`String.fromCharCode(...x)` spreads a list of codes or a typed array",
                ));
            };
            let string_ty = self.ctx.krate.types.intern(Type::String);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast { op, operand },
                ty: string_ty,
                span,
            })));
        }
        if Self::call_has_spread_arguments(call) {
            return Ok(None);
        }
        let float_ty = self.ctx.krate.types.intern(Type::Float);
        let mut codes = Vec::with_capacity(call.arguments.len());
        for argument in &call.arguments {
            let value = self.argument(argument, body)?;
            let value = if matches!(
                self.ctx.krate.types.get(Self::expr_ty(body, value)),
                Some(Type::Float)
            ) {
                value
            } else {
                body.push_expr(Expr {
                    kind: ExprKind::PrimitiveCast {
                        op: PrimitiveCastOp::ToJsNumber,
                        operand: value,
                    },
                    ty: float_ty,
                    span,
                })
            };
            codes.push(value);
        }
        let list_ty = self.ctx.krate.types.intern(Type::List(float_ty));
        let list = body.push_expr(Expr {
            kind: ExprKind::ListLit(codes),
            ty: list_ty,
            span,
        });
        let string_ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::PrimitiveCast { op, operand: list },
            ty: string_ty,
            span,
        })))
    }

    /// Lower supported string split calls into HIR string runtime calls.
    pub(in crate::lowering) fn string_split_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "split" {
            return Ok(None);
        }
        if let Expression::Identifier(object) = &member.object
            && (self.imports.is_namespace(object.name.as_str())
                || self.imports.is_value(object.name.as_str()))
        {
            if call.arguments.len() < 2 || call.arguments.len() > 3 {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "static string split requires value, separator, and optional limit arguments",
                ));
            }
            let Some(haystack_argument) = call.arguments.first() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "static string split requires value, separator, and optional limit arguments",
                ));
            };
            let Some(separator_argument) = call.arguments.get(1) else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "static string split requires value, separator, and optional limit arguments",
                ));
            };
            let haystack = self.argument(haystack_argument, body)?;
            let separator = self.argument(separator_argument, body)?;
            let limit = call
                .arguments
                .get(2)
                .map(|argument| self.argument(argument, body))
                .transpose()?;
            return self.finish_string_split_call(call, haystack, separator, limit, body);
        }
        if call.arguments.is_empty() || call.arguments.len() > 2 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string split requires a separator and optional limit argument",
            ));
        }
        let haystack = self.expression(&member.object, body)?;
        let Some(separator_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string split requires a separator argument",
            ));
        };
        let separator = self.argument(separator_argument, body)?;
        let limit = call
            .arguments
            .get(1)
            .map(|argument| self.argument(argument, body))
            .transpose()?;
        self.finish_string_split_call(call, haystack, separator, limit, body)
    }

    /// Finish string split lowering after the receiver-style or helper-style arguments are known.
    pub(in crate::lowering) fn finish_string_split_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        haystack: smelt_hir::ExprId,
        separator: smelt_hir::ExprId,
        limit: Option<smelt_hir::ExprId>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let haystack_ty = Self::expr_ty(body, haystack);
        let separator_ty = Self::expr_ty(body, separator);
        if !(self.is_string_compatible_type(haystack_ty)
            || self.type_contains_unknown(haystack_ty)
            || self.erased_or_union_surface(haystack_ty))
            || !self.string_split_separator_type_is_supported(separator_ty)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string split requires string receiver and separator",
            ));
        }
        if let Some(limit) = limit {
            let limit_ty = Self::expr_ty(body, limit);
            if !self.string_split_limit_type_is_supported(limit_ty) {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "string split limit must be numeric or undefined",
                ));
            }
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let ty = self.ctx.krate.types.intern(Type::List(string_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringSplit {
                haystack,
                separator,
                limit,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return whether a type can act as a JavaScript string split separator.
    pub(in crate::lowering) fn string_split_separator_type_is_supported(
        &self,
        ty: smelt_hir::TypeId,
    ) -> bool {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
        {
            Some(Type::String | Type::Unknown | Type::TypeParam { .. }) => true,
            Some(Type::Class { name, .. }) => self.stdlib_class_of_symbol(*name) == Some(smelt_stdlib::StdlibClass::RegExp),
            Some(Type::Optional(item)) => self.string_split_separator_type_is_supported(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.string_split_separator_type_is_supported(item)),
            _ => false,
        }
    }

    /// Return whether a type can act as a JavaScript string split limit.
    pub(in crate::lowering) fn string_split_limit_type_is_supported(
        &self,
        ty: smelt_hir::TypeId,
    ) -> bool {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
        {
            Some(Type::Int | Type::Float | Type::None | Type::Unknown | Type::TypeParam { .. }) => {
                true
            }
            Some(Type::Optional(item)) => self.string_split_limit_type_is_supported(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.string_split_limit_type_is_supported(item)),
            _ => false,
        }
    }
}
