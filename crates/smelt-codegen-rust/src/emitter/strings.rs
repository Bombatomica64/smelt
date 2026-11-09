//! Strings emission helpers.

use crate::rust::RustExpr;
use super::*;

/// The local a string slice binds its receiver's length to, read by both bounds.
const SMELT_STRING_LEN: &str = "smelt_len_units";

impl FunctionEmitter<'_> {
    /// `isWellFormed()` (`repair == false`, a `bool`) or `toWellFormed()`
    /// (`repair == true`, a string) over a JavaScript string's code units.
    pub(super) fn string_well_formed_expr(
        &self,
        operand: &Operand,
        repair: bool,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let receiver_text = self.string_like_operand_expr(operand, "well-formed string")?;
        let (method_name, result_ty) = if repair {
            ("to_well_formed", self.type_id(Type::String)?)
        } else {
            ("is_well_formed", self.type_id(Type::Bool)?)
        };
        let result = receiver_text.postfix(format!(".{method_name}()"));
        self.rendered_at_type(&result, result_ty, dest_ty, &self.render_scope())
    }

    /// Converts a string case operation to its Rust text representation.
    pub(super) fn string_case_expr(
        &self,
        op: smelt_hir::StringCaseOp,
        operand: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let receiver_text = self.string_like_operand_expr(operand, "string case")?;
        let method_name = match (op, self.indexes_utf16()) {
            (smelt_hir::StringCaseOp::Lower, true) => "to_lower_case",
            (smelt_hir::StringCaseOp::Upper, true) => "to_upper_case",
            (smelt_hir::StringCaseOp::Lower, false) => "to_lowercase",
            (smelt_hir::StringCaseOp::Upper, false) => "to_uppercase",
        };
        // The case operation always yields a `String`. A `.to_lowercase()` /
        // `.to_uppercase()` call is a postfix expression, so the rendered value
        // is an atom, and the coercion seam (not this caller) owns the
        // parenthesization decision.
        let result = receiver_text.postfix(format!(".{method_name}()"));
        if matches!(
            self.mir.types.get(dest_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) {
            let string_ty = self.type_id(self.text_type())?;
            self.erase_value(&RenderedValue::new(result, string_ty))
        } else {
            Ok(result)
        }
    }

    /// Converts Unicode normalization to Rust text through `unicode-normalization`.
    pub(super) fn string_normalize_expr(
        &self,
        form: smelt_hir::StringNormalizeForm,
        operand: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let receiver_text = self.string_like_operand_expr(operand, "string normalize")?;
        let method_name = match form {
            smelt_hir::StringNormalizeForm::Nfc => "nfc",
            smelt_hir::StringNormalizeForm::Nfd => "nfd",
            smelt_hir::StringNormalizeForm::Nfkc => "nfkc",
            smelt_hir::StringNormalizeForm::Nfkd => "nfkd",
        };
        let host_text = self.host_utf8_text(receiver_text);
        Ok(RustExpr::opaque(self.text_from_string(&format!(
            "unicode_normalization::UnicodeNormalization::{method_name}({host_text}.as_str()).collect::<String>()", host_text = host_text.tight()
        ))))
    }

    /// Converts one of the four JavaScript URI transcoding globals to a call to
    /// its runtime helper.
    ///
    /// The encoders are total, and only that INFALLIBLE direction reaches this
    /// rvalue path. The decoders never do: ECMA-262 rejects malformed input
    /// with a `URIError`, so `decodeURI`/`decodeURIComponent` are fallible and
    /// lower to a call terminator (`Callee::Op`) whose unwind edge makes that
    /// `URIError` catchable. Encoding cannot fail for well-formed UTF-16 input,
    /// so it needs no such edge and keeps the cheaper form.
    pub(super) fn uri_transcode_expr(
        &self,
        op: smelt_hir::UriTranscodeOp,
        operand: &Operand,
    ) -> Result<RustExpr, EmitError> {
        use smelt_hir::UriTranscodeOp;
        use smelt_stdlib::runtime_symbols::strings;

        let name = match op {
            UriTranscodeOp::Encode => "encodeURI",
            UriTranscodeOp::EncodeComponent => "encodeURIComponent",
            UriTranscodeOp::Decode => "decodeURI",
            UriTranscodeOp::DecodeComponent => "decodeURIComponent",
        };
        let receiver_text = self.string_like_operand_expr(operand, name)?;
        let helper = match op {
            UriTranscodeOp::Encode => strings::ENCODE_URI,
            UriTranscodeOp::EncodeComponent => strings::ENCODE_URI_COMPONENT,
            UriTranscodeOp::Decode => strings::DECODE_URI,
            UriTranscodeOp::DecodeComponent => strings::DECODE_URI_COMPONENT,
        };
        if op.is_fallible() {
            // A fallible op is a call terminator's callee; an `.expect(..)` here
            // would abort where JavaScript lets a `catch` run.
            return Err(EmitError::new(format!(
                "internal: fallible `{name}` reached the infallible rvalue path"
            )));
        }
        // The encoders answer UTF-8 `String`; the value is this body's string.
        Ok(RustExpr::chain(self.text_from_string(&format!("{helper}(&{receiver_text})", receiver_text = receiver_text.tight()))))
    }

    /// Converts JavaScript `left.localeCompare(right)` to a call to the runtime
    /// collation helper (`smelt_locale_compare`).
    ///
    /// Both operands are string-like, so they reach the helper as `&str`; the
    /// helper answers the negative / zero / positive `f64` that a comparator
    /// contract expects. See the runtime item for which collation levels are
    /// modeled.
    pub(super) fn string_locale_compare_expr(
        &self,
        left: &Operand,
        right: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let left_text = self.string_like_operand_expr(left, "localeCompare")?;
        let right_text = self.string_like_operand_expr(right, "localeCompare")?;
        Ok(RustExpr::chain(format!(
            "{locale_compare}(&{left_text}, &{right_text})",
            locale_compare = smelt_stdlib::runtime_symbols::strings::LOCALE_COMPARE, left_text = left_text.tight(), right_text = right_text.tight(),
        )))
    }

    /// Converts a string trim operation to Rust text.
    pub(super) fn string_trim_expr(
        &self,
        side: smelt_hir::StringTrimSide,
        operand: &Operand,
    ) -> Result<RustExpr, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::String | Type::PyStr)
        ) {
            return Err(EmitError::new("string trim operand must be a string"));
        }
        let method_name = match side {
            smelt_hir::StringTrimSide::Both => "trim",
            smelt_hir::StringTrimSide::Start => "trim_start",
            smelt_hir::StringTrimSide::End => "trim_end",
        };
        Ok({ let receiver = self.operand_expr(operand)?; receiver.postfix(format!(".{method_name}().to_owned()")) })
    }

    /// Converts a string affix operation to Rust text.
    pub(super) fn string_affix_expr(
        &self,
        op: smelt_hir::StringAffixOp,
        haystack: &Operand,
        needle: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let haystack_text = self.string_like_operand_expr(haystack, "string affix")?;
        let needle_text = self.string_like_operand_expr(needle, "string affix")?;
        let method_name = match op {
            smelt_hir::StringAffixOp::StartsWith => "starts_with",
            smelt_hir::StringAffixOp::EndsWith => "ends_with",
        };
        Ok(haystack_text.postfix(format!(".{method_name}(&{needle_text})", needle_text = needle_text.tight())))
    }

    /// Converts a string search operation to Rust text.
    pub(super) fn string_search_expr(
        &self,
        op: smelt_hir::StringSearchOp,
        haystack: &Operand,
        needle: &Operand,
        from_index: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let (missing, cast) = match self.mir.types.get(dest_ty) {
            Some(Type::Int) => ("-1", "i64"),
            Some(Type::Float) => ("-1.0", "f64"),
            _ => return Err(EmitError::new("string search destination must be numeric")),
        };
        let method_name = match op {
            smelt_hir::StringSearchOp::Find => "find",
            smelt_hir::StringSearchOp::RFind => "rfind",
            smelt_hir::StringSearchOp::Regex => {
                return self.string_regex_search_expr(haystack, needle, missing, cast);
            }
        };
        // A string haystack is read by borrow: searching only looks at it.
        let haystack_text = if self.mir.types.get(self.operand_ty(haystack)?).is_some_and(Type::is_text) {
            RustExpr::atom(format!("&{}", self.operand_borrow_expr(haystack)?.parenthesized_if_needed()))
        } else {
            self.string_like_operand_expr(haystack, "string search")?
        };
        let needle_text = self.string_like_operand_expr(needle, "string search")?;
        // A JavaScript string searches its code units directly; every index in
        // and out is a unit index.
        if self.indexes_utf16() {
            let from_text = match from_index {
                Some(from_index_operand) => format!("({} as i64).max(0) as usize", self.value_at_type(from_index_operand, self.type_id(Type::Float)?)?.as_cast_operand()),
                None if op == smelt_hir::StringSearchOp::Find => "0".to_owned(),
                None => "usize::MAX".to_owned(),
            };
            let search_method = if op == smelt_hir::StringSearchOp::Find { "index_of" } else { "last_index_of" };
            let receiver = if self.mir.types.get(self.operand_ty(haystack)?).is_some_and(Type::is_text) {
                self.operand_borrow_expr(haystack)?
            } else {
                haystack_text
            };
            return Ok(RustExpr::atom(format!(
                "{receiver}.{search_method}(&{needle_text}, {from_text}).map_or({missing}, |smelt_index| smelt_index as {cast})",
                receiver = receiver.parenthesized_if_needed(), needle_text = needle_text.tight(),
            )));
        }
        if let Some(from_index_operand) = from_index {
            let index_text = self.value_at_type(from_index_operand, self.type_id(Type::Float)?)?;
            return match op {
                smelt_hir::StringSearchOp::Find => Ok(RustExpr::atom(format!(
                    "{{ let smelt_haystack = {haystack_text}; let smelt_needle = {needle_text}; let smelt_from = ({index_text} as i64).max(0) as usize; let (smelt_prefix_bytes, smelt_prefix_index) = {seek}; smelt_haystack.get(smelt_prefix_bytes..).and_then(|smelt_suffix| smelt_suffix.find(&smelt_needle).map(|smelt_index| (smelt_prefix_index + {suffix_index}) as {cast})).unwrap_or({missing}) }}",
                    seek = self.string_seek_text("smelt_haystack", "smelt_from"),
                    suffix_index = self.string_index_of_byte_text("smelt_suffix", "smelt_index"), haystack_text = haystack_text.delimited(), needle_text = needle_text.delimited(), index_text = index_text.as_cast_operand(),
                ))),
                smelt_hir::StringSearchOp::RFind => Ok(RustExpr::atom(format!(
                    "{{ let smelt_haystack = {haystack_text}; let smelt_needle = {needle_text}; let smelt_from = ({index_text} as i64).max(0) as usize; let smelt_end_index = smelt_from.saturating_add({needle_len}); let smelt_end_byte = {end_byte}; smelt_haystack[..smelt_end_byte].rfind(&smelt_needle).map_or({missing}, |smelt_idx| {found_index} as {cast}) }}",
                    needle_len = self.string_len_text("smelt_needle"),
                    end_byte = self.string_byte_of_text("smelt_haystack", "smelt_end_index"),
                    found_index = self.string_index_of_byte_text("smelt_haystack", "smelt_idx"), haystack_text = haystack_text.delimited(), needle_text = needle_text.delimited(), index_text = index_text.as_cast_operand(),
                ))),
                // `search` takes no position argument; the frontend never
                // builds one.
                smelt_hir::StringSearchOp::Regex => {
                    Err(EmitError::new("String.search takes no fromIndex"))
                }
            };
        }
        Ok(RustExpr::atom(format!(
            "{{ let smelt_haystack = &{haystack_text}; smelt_haystack.{method_name}(&{needle_text}).map_or({missing}, |smelt_idx| {found_index} as {cast}) }}",
            haystack_text = haystack_text.parenthesized_if_needed(),
            found_index = self.string_index_of_byte_text("smelt_haystack", "smelt_idx"), needle_text = needle_text.tight(),
        )))
    }

    /// Render `haystack.search(pattern)`: the UTF-16 index of the first match
    /// (JavaScript string indices count UTF-16 code units), or `-1`. A string
    /// pattern is compiled as a `RegExp` with no flags, as `search` does.
    fn string_regex_search_expr(
        &self,
        haystack: &Operand,
        needle: &Operand,
        missing: &str,
        cast: &str,
    ) -> Result<RustExpr, EmitError> {
        let haystack_text = self.string_like_operand_expr(haystack, "string search")?;
        let regex_text = if self.mir.types.get(self.operand_ty(needle)?).is_some_and(Type::is_text) {
            format!("SmeltRegExp::new({}, String::new())", self.operand_expr(needle)?.delimited())
        } else {
            self.operand_expr(needle)?.into_string()
        };
        Ok(RustExpr::atom(format!(
            "{{ let smelt_haystack = {haystack_text}; let smelt_regex = {regex_text}; smelt_regex.search_index(&smelt_haystack).map_or({missing}, |smelt_index| smelt_index as {cast}) }}", haystack_text = haystack_text.delimited()
        )))
    }

    /// Converts a string replacement operation to Rust text.
    pub(super) fn string_replace_expr(
        &self,
        op: smelt_hir::StringReplaceOp,
        haystack: &Operand,
        pattern: &Operand,
        replacement: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let haystack_text = self.string_like_operand_expr(haystack, "string replace")?;
        let replacement_text = self.string_like_operand_expr(replacement, "string replace")?;
        if matches!(
            self.mir.types.get(self.operand_ty(pattern)?),
            Some(Type::Class { name, .. }) if self.is_regexp_class_symbol(*name)?
        ) {
            let regex_text = self.regexp_operand_expr(pattern)?;
            let force_all = matches!(op, smelt_hir::StringReplaceOp::All);
            return Ok(regex_text.postfix(format!(
                ".replace_string(&{haystack_text}, &{replacement_text}, {force_all})", haystack_text = haystack_text.tight(), replacement_text = replacement_text.tight()
            )));
        }
        let pattern_text = self.string_like_operand_expr(pattern, "string replace")?;
        if self.indexes_utf16() {
            let method_name = match op {
                smelt_hir::StringReplaceOp::First => "replace_first",
                smelt_hir::StringReplaceOp::All => "replace_all",
            };
            return Ok(haystack_text.postfix(format!(
                ".{method_name}(&{pattern_text}, &{replacement_text})", pattern_text = pattern_text.tight(), replacement_text = replacement_text.tight()
            )));
        }
        match op {
            smelt_hir::StringReplaceOp::First => Ok(haystack_text.postfix(format!(
                ".replacen(&{pattern_text}, &{replacement_text}, 1)", pattern_text = pattern_text.tight(), replacement_text = replacement_text.tight()
            ))),
            smelt_hir::StringReplaceOp::All => Ok(haystack_text.postfix(format!(
                ".replace(&{pattern_text}, &{replacement_text})", pattern_text = pattern_text.tight(), replacement_text = replacement_text.tight()
            ))),
        }
    }

    /// Converts a string remove-prefix/remove-suffix operation to Rust text.
    pub(super) fn string_remove_affix_expr(
        &self,
        op: smelt_hir::StringAffixOp,
        haystack: &Operand,
        affix: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let haystack_text = self.string_like_operand_expr(haystack, "string remove-affix")?;
        let affix_text = self.string_like_operand_expr(affix, "string remove-affix")?;
        let method_name = match op {
            smelt_hir::StringAffixOp::StartsWith => "strip_prefix",
            smelt_hir::StringAffixOp::EndsWith => "strip_suffix",
        };
        if self.indexes_utf16() {
            let check = if method_name == "strip_prefix" { "starts_with" } else { "ends_with" };
            let cut = if method_name == "strip_prefix" { "smelt_text.slice(smelt_affix.len(), smelt_text.len())" } else { "smelt_text.slice(0, smelt_text.len() - smelt_affix.len())" };
            return Ok(RustExpr::atom(format!(
                "{{ let smelt_text = {haystack_text}; let smelt_affix = {affix_text}; if smelt_text.{check}(&smelt_affix) {{ {cut} }} else {{ smelt_text }} }}", haystack_text = haystack_text.delimited(), affix_text = affix_text.delimited()
            )));
        }
        Ok(haystack_text.postfix(format!(
            ".{method_name}(&{affix_text}).unwrap_or(&{haystack_text}).to_owned()", affix_text = affix_text.tight(), haystack_text = haystack_text.tight()
        )))
    }

    /// Converts a string repeat operation to Rust text.
    pub(super) fn string_repeat_expr(
        &self,
        operand: &Operand,
        count: &Operand,
    ) -> Result<RustExpr, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::String | Type::PyStr)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(count)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new(
                "string repeat requires a string receiver and numeric count",
            ));
        }
        // An integer count is clamped at zero: `i64 as usize` would wrap a
        // negative count into a huge allocation, where Python's `"ab" * -1` is
        // `""`. A float count already saturates at zero through `as usize`.
        let count_expr = self.operand_expr(count)?;
        let count_text = if matches!(self.mir.types.get(self.operand_ty(count)?), Some(Type::Int)) {
            format!("({count_expr}).max(0) as usize", count_expr = count_expr.delimited())
        } else {
            format!("{count_expr} as usize", count_expr = count_expr.as_cast_operand())
        };
        let receiver = self.operand_expr(operand)?;
        Ok(receiver.postfix(format!(".repeat({count_text})")))
    }

    /// Converts a string padding operation to Rust text.
    ///
    /// The generated code counts Rust `char`s rather than JavaScript UTF-16 code units.
    /// Exact Unicode indexing parity is tracked separately with the other string APIs.
    pub(super) fn string_pad_expr(
        &self,
        op: smelt_hir::StringPadOp,
        operand: &Operand,
        target_len: &Operand,
        pad: &Operand,
    ) -> Result<RustExpr, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::String | Type::PyStr)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(target_len)?),
            Some(Type::Int | Type::Float)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(pad)?),
            Some(Type::String | Type::PyStr)
        ) {
            return Err(EmitError::new(
                "string padding requires string receiver, numeric target length, and string padding",
            ));
        }
        let operand_expr = self.operand_expr(operand)?;
        let target_len_text = self.operand_expr(target_len)?;
        let pad_text = self.operand_expr(pad)?;
        let result_text = match (op, self.indexes_utf16()) {
            (smelt_hir::StringPadOp::Start, true) => "smelt_padding + smelt_value",
            (smelt_hir::StringPadOp::End, true) => "smelt_value.to_owned() + &smelt_padding",
            (smelt_hir::StringPadOp::Start, false) => "format!(\"{}{}\", smelt_padding, smelt_value)",
            (smelt_hir::StringPadOp::End, false) => "format!(\"{}{}\", smelt_value, smelt_padding)",
        };
        let padding = self.string_padding_text("smelt_pad", "smelt_needed");
        Ok(RustExpr::atom(format!(
            "{{ let smelt_value = &{operand_expr}; let smelt_target_len = {target_len_text} as usize; let smelt_pad = &{pad_text}; let smelt_current_len = {current_len}; if smelt_current_len >= smelt_target_len || smelt_pad.is_empty() {{ smelt_value.to_owned() }} else {{ let smelt_needed = smelt_target_len - smelt_current_len; let smelt_padding: {text_type} = {padding}; {result_text} }} }}",
            text_type = self.text_type_name(),
            current_len = self.string_len_text("smelt_value"), operand_expr = operand_expr.tight(), target_len_text = target_len_text.as_cast_operand(), pad_text = pad_text.tight(),
        )))
    }

    /// Converts a string character predicate operation to Rust text.
    pub(super) fn string_predicate_expr(
        &self,
        op: smelt_hir::StringPredicateOp,
        operand: &Operand,
    ) -> Result<RustExpr, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::String | Type::PyStr)
        ) {
            return Err(EmitError::new("string predicate operand must be a string"));
        }
        let method_name = match op {
            smelt_hir::StringPredicateOp::IsDigit => "is_ascii_digit",
            smelt_hir::StringPredicateOp::IsAlpha => "is_alphabetic",
            smelt_hir::StringPredicateOp::IsAlnum => "is_alphanumeric",
        };
        let operand_expr = self.operand_expr(operand)?;
        Ok(RustExpr::loose(format!(
            "!{operand_expr}.is_empty() && {operand_expr}.chars().all(char::{method_name})", operand_expr = operand_expr.tight()
        )))
    }

    /// Converts a regex boolean match operation to Rust text using the `fancy-regex` crate.
    ///
    /// The emitted expression compiles the pattern at the call site so the dependency stays
    /// interchangeable with a future cached-regex helper module.
    ///
    /// `fancy-regex` rather than `regex`: both JavaScript and Python patterns
    /// use look-around and backreferences, which the `regex` crate rejects at
    /// compile time — `/\((?!\?:)/.test(s)` (Hono's reg-exp router) panicked
    /// with "look-around ... is not supported". `fancy-regex` delegates every
    /// pattern without those features to `regex` itself, so simple patterns
    /// match exactly as before, and it ships with the `regex` dependency (see
    /// `smelt_stdlib::deps`). A backtracking-limit error at match time answers
    /// "no match", as the `SmeltRegExp` runtime already does.
    pub(super) fn regex_is_match_expr(
        &self,
        op: smelt_hir::RegexMatchOp,
        pattern: &Operand,
        haystack: &Operand,
    ) -> Result<RustExpr, EmitError> {
        // A JavaScript pattern is a `RegExp` on `regress` (`SmeltRegExp`); the
        // frontend only lowers a stateless `test` here, which is a search.
        if self.indexes_utf16() {
            let regexp_text = self.regexp_operand_expr(pattern)?;
            let haystack_text = self.string_like_operand_expr(haystack, "regex match")?;
            return Ok(regexp_text.postfix(format!(".is_match(&{haystack_text})", haystack_text = haystack_text.tight())));
        }
        let pattern_text = self.host_utf8_text(self.string_like_operand_expr(pattern, "regex match")?);
        let haystack_text = self.host_utf8_text(self.string_like_operand_expr(haystack, "regex match")?);
        let regex_text =
            RustExpr::atom(format!("fancy_regex::Regex::new(&{pattern_text}).expect(\"regex compile failed\")", pattern_text = pattern_text.tight()));
        Ok(match op {
            smelt_hir::RegexMatchOp::Search => {
                regex_text.postfix(format!(".is_match(&{haystack_text}).unwrap_or(false)", haystack_text = haystack_text.tight()))
            }
            smelt_hir::RegexMatchOp::Match => {
                regex_text.postfix(format!(
                    ".find(&{haystack_text}).ok().flatten().is_some_and(|smelt_m| smelt_m.start() == 0)", haystack_text = haystack_text.tight()
                ))
            }
            smelt_hir::RegexMatchOp::FullMatch => {
                regex_text.postfix(format!(
                    ".find(&{haystack_text}).ok().flatten().is_some_and(|smelt_m| smelt_m.start() == 0 && smelt_m.end() == {haystack_text}.len())", haystack_text = haystack_text.tight()
                ))
            }
        })
    }

    /// Converts a regex replacement operation to Rust text using the `regex` crate.
    pub(super) fn regex_replace_expr(
        &self,
        op: smelt_hir::StringReplaceOp,
        pattern: &Operand,
        haystack: &Operand,
        replacement: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let force_all = matches!(op, smelt_hir::StringReplaceOp::All);
        // A Python pattern is `re` syntax, matched by `fancy-regex`.
        if !self.indexes_utf16() {
            self.require_string_operands(&[pattern, haystack, replacement], "regex replace")?;
            let pattern_text = self.string_like_operand_expr(pattern, "regex replace")?;
            let haystack_text = self.string_like_operand_expr(haystack, "regex replace")?;
            let replacement_text = self.string_like_operand_expr(replacement, "regex replace")?;
            return Ok(RustExpr::chain(format!(
                "smelt_py_regex_replace(&{pattern_text}, &{haystack_text}, &{replacement_text}, {force_all})", pattern_text = pattern_text.tight(), haystack_text = haystack_text.tight(), replacement_text = replacement_text.tight()
            )));
        }
        let regex_text = self.regexp_operand_expr(pattern)?;
        let haystack_text = self.string_like_operand_expr(haystack, "regex replace")?;
        let replacement_text = self.string_like_operand_expr(replacement, "regex replace")?;
        Ok(regex_text.postfix(format!(
            ".replace_string(&{haystack_text}, &{replacement_text}, {force_all})", haystack_text = haystack_text.tight(), replacement_text = replacement_text.tight()
        )))
    }

    /// Converts `String.prototype.replace(re, fn)` to Rust text.
    ///
    /// The pattern is a `SmeltRegExp` (ECMAScript syntax on `regress`), and
    /// `SmeltRegExp::replace_with` hands each match to the replacer as a typed
    /// `SmeltMatch`. `args` names the ECMA-262 replacer arguments the callback
    /// declared, in order, as resolved by the frontend; each is read off that
    /// match (see [`Self::regex_replacer_argument_texts`]). Only TypeScript
    /// lowers to this op.
    pub(super) fn regex_replace_callback_expr(
        &self,
        op: smelt_hir::StringReplaceOp,
        pattern: &Operand,
        haystack: &Operand,
        callback: &Operand,
        args: &[smelt_hir::RegexReplaceArg],
    ) -> Result<RustExpr, EmitError> {
        let regex_text = self.regexp_operand_expr(pattern)?;
        let haystack_text = self.string_like_operand_expr(haystack, "regex replace callback")?;
        let callback_text = self
            .closure_operand_expr(callback)
            .or_else(|_| self.operand_expr(callback))?;
        let arg_texts = self.regex_replacer_argument_texts(args, callback)?;
        // JavaScript converts the callback's answer to a string (ToString)
        // before substituting it.
        let call_expr = format!("({callback_text})({})", arg_texts.join(", "), callback_text = callback_text.delimited());
        let replacement_expr = self.regex_replacement_as_js_string(call_expr, callback)?;
        let force_all = matches!(op, smelt_hir::StringReplaceOp::All);
        Ok(regex_text.postfix(format!(
            ".replace_with(&{haystack_text}, {force_all}, |smelt_match: &SmeltMatch| {replacement_expr})", haystack_text = haystack_text.tight()
        )))
    }

    /// Renders each declared replacer argument as Rust text at the callback's
    /// own parameter type, read off the current match `smelt_match`.
    ///
    /// Every role has one natural Rust rendering and one natural Smelt type:
    ///
    /// | role | rendering | type |
    /// | --- | --- | --- |
    /// | `Matched` | group 0's text | `string` |
    /// | `Capture(n)` | group `n`'s text, `None` when it did not participate | `string \| undefined` |
    /// | `Position` | the match's code unit offset in the subject | `number` |
    /// | `Source` | the whole subject string | `string` |
    ///
    /// The callback may nonetheless have annotated a parameter differently (its
    /// contextual type is only a *hint*), so each rendering is routed through
    /// `rendered_at_type` to the declared parameter type. A callback whose
    /// parameter types are not recoverable keeps the natural rendering.
    fn regex_replacer_argument_texts(
        &self,
        args: &[smelt_hir::RegexReplaceArg],
        callback: &Operand,
    ) -> Result<Vec<String>, EmitError> {
        let param_tys = match self.mir.types.get(self.operand_ty(callback)?) {
            Some(Type::Function(function)) => function.params.clone(),
            _ => Vec::new(),
        };
        let string_ty = self.existing_type_id(Type::String);
        let float_ty = self.existing_type_id(Type::Float);
        let optional_string_ty =
            string_ty.and_then(|inner| self.existing_type_id(Type::Optional(inner)));
        let mut texts = Vec::with_capacity(args.len());
        for (index, arg) in args.iter().enumerate() {
            let (natural_text, natural_ty) = match arg {
                smelt_hir::RegexReplaceArg::Matched => {
                    (RustExpr::chain("smelt_match.group_owned(0).unwrap_or_default()"), string_ty)
                }
                smelt_hir::RegexReplaceArg::Capture(group) => {
                    (RustExpr::chain(format!("smelt_match.group_owned({group})")), optional_string_ty)
                }
                smelt_hir::RegexReplaceArg::Position => (RustExpr::chain("smelt_match.index()"), float_ty),
                smelt_hir::RegexReplaceArg::Source => (RustExpr::chain("smelt_match.input_owned()"), string_ty),
            };
            let text = match (natural_ty, param_tys.get(index)) {
                (Some(source), Some(&target)) if source != target => {
                    self.rendered_at_type(&natural_text, source, target, &self.render_scope())?
                }
                _ => natural_text,
            };
            texts.push(text.into_string());
        }
        Ok(texts)
    }

    /// Coerce a replacer callback's answer to the `JsString` it substitutes.
    ///
    /// A callback returning a string is used as is; any other answer is erased
    /// and converted with JavaScript's ToString (`undefined` is the text
    /// `undefined`, which a string extraction would read as empty).
    fn regex_replacement_as_js_string(
        &self,
        call_expr: String,
        callback: &Operand,
    ) -> Result<String, EmitError> {
        let return_ty = match self.mir.types.get(self.operand_ty(callback)?) {
            Some(Type::Function(function)) => function.return_ty,
            _ => return Ok(call_expr),
        };
        if matches!(self.mir.types.get(return_ty), Some(Type::String)) {
            return Ok(call_expr);
        }
        let erased = self.erase_rendered(&RustExpr::opaque(call_expr), return_ty)?;
        Ok(format!("smelt_erased_to_js_string(&{erased})", erased = erased.tight()))
    }

    /// Converts a regex split operation to Rust text using the `fancy-regex` crate.
    ///
    /// A backtracking-limit error on one piece drops that piece, the same
    /// no-match answer the other `fancy-regex` sites give.
    pub(super) fn regex_split_expr(
        &self,
        pattern: &Operand,
        haystack: &Operand,
    ) -> Result<RustExpr, EmitError> {
        // A JavaScript pattern splits with ECMA-262 `@@split` (`SmeltRegExp`).
        if self.indexes_utf16() {
            let regexp_text = self.regexp_operand_expr(pattern)?;
            let haystack_text = self.string_like_operand_expr(haystack, "regex split")?;
            return Ok(regexp_text.postfix(format!(".split_string(&{haystack_text})", haystack_text = haystack_text.tight())));
        }
        self.require_string_operands(&[pattern, haystack], "regex split")?;
        let regex_text = RustExpr::atom(format!(
            "fancy_regex::Regex::new(&{}).expect(\"regex compile failed\")",
            self.operand_expr(pattern)?.tight()
        ));
        let haystack_text = self.operand_expr(haystack)?;
        Ok(RustExpr::atom_on(&regex_text, format!(
            "{regex_text}.split(&{haystack_text}).filter_map(Result::ok).map(str::to_owned).collect::<Vec<_>>()", regex_text = regex_text.tight(), haystack_text = haystack_text.tight()
        )))
    }

    /// Converts JavaScript `String.prototype.match(RegExp)` to an optional match array.
    pub(super) fn regex_find_expr(
        &self,
        pattern: &Operand,
        haystack: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let pattern_text = self.regexp_operand_expr(pattern)?;
        let haystack_text = self.string_like_operand_expr(haystack, "regex find")?;
        // The frontend types `String.prototype.match` `string[] | null` only
        // where the regex is statically GLOBAL; that answer is the list of
        // whole matches, which `match_all_strings` builds directly.
        if let Some(Type::Optional(inner)) = self.mir.types.get(dest_ty)
            && let Some(Type::List(item)) = self.mir.types.get(*inner)
            && self.mir.types.get(*item).is_some_and(Type::is_text)
        {
            return Ok(pattern_text.postfix(format!(
                ".match_all_strings(&{haystack_text}).map(SmeltList::from)", haystack_text = haystack_text.tight()
            )));
        }
        // Everywhere else `SmeltRegExp::match_string` answers the typed
        // `Option<SmeltMatch>` the frontend declares (the same value `exec`
        // answers), so the present arm needs no projection. As for `exec`,
        // only an erased result slot takes the explicit adapter.
        let call = pattern_text.postfix(format!(".match_string(&{haystack_text})", haystack_text = haystack_text.tight()));
        if self.optional_inner_is_erased_boundary(dest_ty) {
            return Ok(call.postfix(self.carrier_format(self.carrier()?, format_args!(".map(SmeltMatch::into_smelt_unknown)"))));
        }
        Ok(call)
    }

    /// Converts JavaScript `RegExp.prototype.exec` to a concrete match result.
    ///
    /// The runtime `exec` returns a typed `Option<SmeltMatch>`. The frontend
    /// types the result as `Optional(SmeltMatch)`, so consumer reads (`m[0]`,
    /// `m.index`, `m.input`, `m.groups.name`) stay typed against the concrete
    /// match value. The optional-erasing seam is only taken when the *result
    /// slot itself* is an erased boundary (`Optional(Unknown)`), e.g. the throwaway
    /// null-check `exec` behind `RegExp.prototype.test`, where a single explicit
    /// `SmeltMatch::into_smelt_unknown` adapter erases the match.
    pub(super) fn regex_exec_expr(
        &self,
        regex: &Operand,
        haystack: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let regex_text = self.regexp_operand_expr(regex)?;
        let haystack_text = self.string_like_operand_expr(haystack, "regex exec")?;
        let call = regex_text.postfix(format!(".exec(&{haystack_text})", haystack_text = haystack_text.tight()));
        if self.optional_inner_is_erased_boundary(dest_ty) {
            return Ok(call.postfix(self.carrier_format(self.carrier()?, format_args!(".map(SmeltMatch::into_smelt_unknown)"))));
        }
        Ok(call)
    }

    /// Converts `regex.test(haystack)` on a concrete receiver to a `bool`.
    ///
    /// `SmeltRegExp::test` is the runtime's own method and already carries the
    /// stateful semantics: it runs the same search `exec` does, so a `/g` or
    /// `/y` receiver reads and advances its `lastIndex`. What it does NOT do is
    /// build a match object — which is what the previous lowering did, and then
    /// erased that object to `SmeltUnknown` purely to compare it against
    /// `null`, at two avoidable erasures per call site.
    pub(super) fn regex_test_expr(
        &self,
        regex: &Operand,
        haystack: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let regex_text = self.regexp_operand_expr(regex)?;
        let haystack_text = self.string_like_operand_expr(haystack, "regex test")?;
        Ok(regex_text.postfix(format!(".test(&{haystack_text})", haystack_text = haystack_text.tight())))
    }

    /// Converts JavaScript `String.prototype.matchAll` to concrete match results.
    ///
    /// Mirrors [`Self::regex_exec_expr`]: `match_all_indices` yields typed
    /// `SmeltMatch` values. The frontend types the result as
    /// `List(SmeltMatch)`, so the concrete values flow to consumer reads
    /// untouched; the explicit `SmeltMatch::into_smelt_unknown` adapter is only
    /// applied when the list element slot is the erased `List(Unknown)`
    /// boundary.
    pub(super) fn regex_match_all_expr(
        &self,
        regex: &Operand,
        haystack: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let regex_text = self.regexp_operand_expr(regex)?;
        let haystack_text = self.string_like_operand_expr(haystack, "regex matchAll")?;
        let call = regex_text.postfix(format!(".match_all_indices(&{haystack_text})", haystack_text = haystack_text.tight()));
        if self.list_item_is_erased_boundary(dest_ty) {
            return Ok(RustExpr::atom_on(&call, self.carrier_format(self.carrier()?, format_args!(
                "{call}.into_iter().map(SmeltMatch::into_smelt_unknown).collect::<Vec<_>>()", call = call.tight()
            ))));
        }
        Ok(call)
    }

    /// Returns whether an `Optional(T)` destination erases `T` to `SmeltUnknown`.
    fn optional_inner_is_erased_boundary(&self, dest_ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(dest_ty),
            Some(Type::Optional(inner))
                if matches!(
                    self.mir.types.get(*inner),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                )
        )
    }

    /// Returns whether a `List(T)` destination erases `T` to `SmeltUnknown`.
    fn list_item_is_erased_boundary(&self, dest_ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(dest_ty),
            Some(Type::List(inner))
                if matches!(
                    self.mir.types.get(*inner),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                )
        )
    }

    /// Render a value as a `SmeltRegExp`.
    fn regexp_operand_expr(&self, operand: &Operand) -> Result<RustExpr, EmitError> {
        let text = self.operand_expr(operand)?;
        match self.mir.types.get(self.operand_ty(operand)?) {
            Some(Type::Class { name, .. }) if self.is_regexp_class_symbol(*name)? => Ok(text),
            Some(Type::String | Type::PyStr) => Ok(RustExpr::atom(format!("SmeltRegExp::new({text}, String::new())", text = text.delimited()))),
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. }) => {
                let scrutinee = self.erase_concrete_union_expr(&text, self.operand_ty(operand)?)?;
                // The prelude `SmeltFromUnknown` impl is the single inverse of
                // the `SmeltRegExp` erasure adapter, so `source`, `flags` AND
                // the writable `lastIndex` all survive the round trip.
                Ok(RustExpr::loose(self.carrier_format(self.carrier()?, format_args!(
                    "<SmeltRegExp as SmeltFromUnknown>::smelt_from_unknown({scrutinee})", scrutinee = scrutinee.delimited()
                ))))
            }
            _ => Ok(RustExpr::atom(format!(
                "SmeltRegExp::new({text}.to_string(), String::new())", text = text.tight()
            ))),
        }
    }

    /// Checks that every operand has string type.
    pub(super) fn require_string_operands(
        &self,
        operands: &[&Operand],
        context: &str,
    ) -> Result<(), EmitError> {
        for operand in operands {
            if !matches!(
                self.mir.types.get(self.operand_ty(operand)?),
                Some(Type::String | Type::PyStr)
            ) && self.string_like_operand_expr(operand, context).is_err()
            {
                return Err(EmitError::new(format!(
                    "{context} requires string operands"
                )));
            }
        }
        Ok(())
    }

    /// Renders the `Error.prototype.toString` arm of an erased JavaScript `ToString`.
    ///
    /// JavaScript does not stringify an `Error` as `[object Object]`: it has an
    /// own `toString` that yields `"name"`, `"message"`, or `"name: message"`
    /// depending on which halves are non-empty. Smelt models an error as a record
    /// carrying the `__smelt_error` class marker (see
    /// `error_object_constructor_expression`), so the rule keys off that marker
    /// exactly as the sibling `__smelt_regexp` arm keys off its own -- it is a
    /// general rule for every error value, not a per-library spelling.
    ///
    /// The name is read from an explicit `name` field first (a subclass
    /// constructor may assign one) and falls back to the marker's class name, so
    /// `new TypeError('m')` renders `"TypeError: m"`.
    ///
    /// The arm consumes an owned `SmeltObject` bound as `value` and must be placed
    /// before the generic `SmeltUnknown::Object(_)` arm, which would otherwise
    /// shadow it.
    pub(super) fn js_error_to_string_arm_text() -> &'static str {
        "SmeltUnknown::Object(value) if value.contains_key(\"__smelt_error\") => { let smelt_error_name = match value.get(\"name\") { Some(SmeltUnknown::String(name)) => name.to_string(), _ => match value.get(\"__smelt_error\") { Some(SmeltUnknown::String(class)) => class.to_string(), _ => \"Error\".to_owned() } }; let smelt_error_message = match value.get(\"message\") { Some(SmeltUnknown::String(message)) => message.to_string(), _ => String::new() }; if smelt_error_message.is_empty() { smelt_error_name } else if smelt_error_name.is_empty() { smelt_error_message } else { format!(\"{smelt_error_name}: {smelt_error_message}\") } }, "
    }

    /// Renders the JavaScript `ToString` coercion of an owned erased value.
    ///
    /// `scrutinee_text` must be an owned `SmeltUnknown` expression (the match
    /// arms consume string payloads). The mapping mirrors JS primitive string
    /// coercion; structured values use the platform object placeholder.
    ///
    /// `js_text` picks the result type: a `JsString` (the erased string
    /// payload moves out; other values are coerced to `String` text and
    /// transcoded) or a Rust `String` for a host API that takes UTF-8.
    pub(super) fn js_string_coercion_match_expr(
        &self,
        scrutinee_text: &RustExpr,
        absent: AbsentSpelling,
        js_text: bool,
    ) -> Result<RustExpr, EmitError> {
        let error_arm = Self::js_error_to_string_arm_text();
        let null_text = absent.null_text();
        // A `URLSearchParams` overrides `toString` in the spec: `String(params)`
        // and `${params}` are its QUERY STRING, not `[object Object]`. The
        // erased record narrows back to the concrete list (the origin registry,
        // else its own pairs) and answers `to_text()`, so the encoding rule
        // lives in one place. Gated on the params prelude being emitted at all:
        // the fetch types are pay-for-use, and this match is not.
        let params_arm = if self.context.demand().has(Need::UrlSearchParams) {
            "SmeltUnknown::Object(value) if value.contains_key(\"__smelt_urlsearchparams\") => <SmeltUrlSearchParams as SmeltFromUnknown>::smelt_from_unknown(SmeltUnknown::Object(value)).to_text(),"
        } else {
            ""
        };
        // A `URL` overrides `toString` the same way: it is the `href`, which
        // the erased record carries as a data property.
        let url_arm = if self.context.demand().has(Need::Url) {
            "SmeltUnknown::Object(value) if value.contains_key(\"__smelt_url\") => <SmeltUrl as SmeltFromUnknown>::smelt_from_unknown(SmeltUnknown::Object(value)).href(),"
        } else {
            ""
        };
        // A typed-array VIEW overrides `toString` too: it IS
        // `Array.prototype.toString`, so `String(view)` and `${view}` are the
        // joined elements. The concrete face answers this from its own
        // `to_js_string`; this is the same rule for a view that has crossed
        // into `SmeltUnknown`, so the two faces cannot disagree about one
        // value. Byte STORAGE has no elements and is deliberately not here.
        // Gated on the byte prelude being emitted at all, like the params arm
        // above.
        let byte_view_arm = if self.context.demand().has(Need::Unknown) {
            self.carrier_format(self.carrier()?, format_args!(
                "SmeltUnknown::Object(smelt_value) if {is_view}(&SmeltUnknown::Object(smelt_value.clone())) => {elements}(&SmeltUnknown::Object(smelt_value)).unwrap_or_default().into_iter().map(|smelt_element| match smelt_element {{ SmeltUnknown::Number(smelt_element) => smelt_element.to_string(), _ => String::new() }}).collect::<Vec<_>>().join(\",\"),",
                is_view = smelt_stdlib::runtime_symbols::byte_buffer::IS_VIEW,
                elements = smelt_stdlib::runtime_symbols::byte_buffer::ELEMENTS,
            ))
        } else {
            String::new()
        };
        let undefined_text = absent.text();
        // A JavaScript string answers its erased payload itself; every other
        // value is coerced through the `String` text below.
        let (match_scrutinee, open, close) = if js_text {
            (RustExpr::path("smelt_value"), format!("match {scrutinee_text} {{ SmeltUnknown::String(smelt_text) => smelt_text, smelt_value => JsString::from(", scrutinee_text = scrutinee_text.delimited()), ") }".to_owned())
        } else {
            (scrutinee_text.clone(), String::new(), String::new())
        };
        Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
            "{open}match {match_scrutinee} {{ {params_arm} {url_arm} {byte_view_arm} SmeltUnknown::Null => \"{null_text}\".to_owned(), SmeltUnknown::Undefined => \"{undefined_text}\".to_owned(), SmeltUnknown::Bool(smelt_value) => smelt_value.to_string(), SmeltUnknown::Number(smelt_value) => smelt_number_to_string(smelt_value), {smelt_bigint_arm}SmeltUnknown::String(smelt_value) => smelt_value.to_string(), SmeltUnknown::Symbol(smelt_value) => smelt_symbol_to_string(&smelt_value), SmeltUnknown::Object(smelt_value) if smelt_value.contains_key(\"{SMELT_REGEXP}\") => smelt_regexp_literal(&smelt_value), {error_arm}SmeltUnknown::Array(smelt_values) => smelt_erased_to_js_string(&SmeltUnknown::Array(smelt_values)).to_string_lossy(), SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () {{ [native code] }}\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned() }}{close}", smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => value.to_string(),"), match_scrutinee = match_scrutinee.delimited()
        ))))
    }

    /// Renders an operand as a Rust `String` expression for string methods.
    ///
    /// `String(value)` for a concrete union whose every arm has a typed string
    /// conversion, as one `match` over the arms; `None` for any other operand.
    ///
    /// A hand port matches the enum. Erasing it to `SmeltUnknown` first, which
    /// the general path does for every union, gives the same text but routes
    /// typed data through the dynamic carrier for no reason. An arm whose
    /// conversion is not modeled here (a record, a user class) keeps the
    /// general path for the whole union.
    fn concrete_union_string_expr(&self, operand: &Operand) -> Result<Option<RustExpr>, EmitError> {
        let union_ty = self.operand_ty(operand)?;
        let Some(members) = self.concrete_union_members(union_ty) else {
            return Ok(None);
        };
        let number_to_string = smelt_stdlib::runtime_symbols::number_text::NUMBER_TO_STRING_FN;
        let union_enum_name = union::union_name(union_ty);
        let mut arms = Vec::with_capacity(members.len());
        for (index, member) in members.iter().enumerate() {
            let conversion = match self.mir.types.get(*member) {
                Some(Type::String | Type::PyStr) => "smelt_union_arm.clone()".to_owned(),
                Some(Type::Bool | Type::BigInt) => "smelt_union_arm.to_string()".to_owned(),
                Some(Type::Int | Type::FixedInt(_)) => {
                    format!("{number_to_string}(*smelt_union_arm as f64)")
                }
                Some(Type::Float) => format!("{number_to_string}(*smelt_union_arm)"),
                Some(Type::Class { name, .. }) => match self.stdlib_class_of_symbol(*name)? {
                    Some(smelt_stdlib::StdlibClass::Url) => "smelt_union_arm.href()".to_owned(),
                    Some(smelt_stdlib::StdlibClass::UrlSearchParams) => {
                        "smelt_union_arm.to_text()".to_owned()
                    }
                    _ => return Ok(None),
                },
                _ => return Ok(None),
            };
            let arm_conversion = if self.indexes_utf16() && !matches!(self.mir.types.get(*member), Some(Type::String | Type::PyStr)) {
                self.text_from_string(&conversion)
            } else {
                conversion
            };
            arms.push(format!("{union_enum_name}::M{index}(smelt_union_arm) => {arm_conversion}"));
        }
        Ok(Some(RustExpr::atom(format!(
            "match &{} {{ {} }}",
            self.operand_borrow_expr(operand)?.tight(),
            arms.join(", ")
        ))))
    }

    /// Statically known strings are emitted directly. Unknown values use the
    /// same JavaScript-style primitive coercion as string joining; structured
    /// unknown values use the platform object placeholder.
    pub(super) fn string_like_operand_expr(
        &self,
        operand: &Operand,
        _context: &str,
    ) -> Result<RustExpr, EmitError> {
        if let Some(text) = self.concrete_union_string_expr(operand)? {
            return Ok(text);
        }
        let operand_ty = self.operand_ty(operand)?;
        // Every arm below answers a Rust `String` except the ones that already
        // hold this body's string type: a string operand, and the erased
        // coercion, which picks its result type itself.
        let answers_text = match self.mir.types.get(operand_ty) {
            Some(Type::String | Type::PyStr | Type::Unknown | Type::Union(_) | Type::TypeParam { .. } | Type::Never | Type::Tuple(_) | Type::Future(_) | Type::Function(_) | Type::Generator { .. } | Type::GeneratorResult { .. }) | None => true,
            Some(Type::Optional(inner)) => self.mir.types.get(*inner).is_some_and(Type::is_text),
            _ => false,
        };
        if self.indexes_utf16() && matches!(self.mir.types.get(operand_ty), Some(Type::None)) {
            return Ok(RustExpr::js_string_literal(self.absent_spelling().null_text()));
        }
        let text = self.string_like_operand_rust_text(operand)?;
        if self.indexes_utf16() && !answers_text {
            return Ok(RustExpr::chain(self.text_from_string(&text.into_string())));
        }
        Ok(text)
    }

    /// [`Self::string_like_operand_expr`]'s value as a Rust `String`, except
    /// for an operand that is already a string or is erased (see there).
    fn string_like_operand_rust_text(&self, operand: &Operand) -> Result<RustExpr, EmitError> {
        match self.mir.types.get(self.operand_ty(operand)?) {
            Some(Type::String | Type::PyStr) => match operand {
                Operand::Copy(place) => Ok(self.place_expr(place)?.cloned_value()),
                Operand::Move(place) => self.place_expr(place),
                Operand::Const(_) => self.operand_expr(operand),
            },
            Some(Type::Bool) => Ok(self.operand_expr(operand)?.postfix(".to_string()")),
            // `String(10n)` is `"10"`: a bigint's string form has no `n`.
            Some(Type::BigInt) => Ok(RustExpr::atom(format!("({}).to_string()", self.bigint_ref_expr(operand)?.delimited()))),
            // `String(Symbol('d'))` is `"Symbol(d)"`. (Implicit conversion, a
            // template or `+`, throws a TypeError in JavaScript; the frontend
            // only routes the explicit `String(..)` spelling here.)
            Some(Type::Symbol) => Ok(RustExpr::atom(format!("({}).to_js_string().to_string()", self.operand_expr(operand)?.delimited()))),
            Some(Type::PropKey) => Ok(RustExpr::atom(format!("({}).to_string()", self.operand_expr(operand)?.delimited()))),
            // A NUMBER goes through JavaScript's own rule, which parts company
            // with Rust's `Display` at both ends of the range (`1e+21`, `1e-7`).
            // `Int` is included because a source `number` that Smelt typed as
            // an integer is still a JavaScript number: `String(x)` must not
            // depend on which of the two Smelt inferred, and a JS number is an
            // `f64` in any case.
            Some(Type::Int | Type::FixedInt(_)) => Ok(RustExpr::chain(format!(
                "{fn_name}({text} as f64)",
                fn_name = smelt_stdlib::runtime_symbols::number_text::NUMBER_TO_STRING_FN,
                text = self.operand_expr(operand)?.as_cast_operand(),
            ))),
            Some(Type::Float) => Ok(RustExpr::chain(format!(
                "{fn_name}({text})",
                fn_name = smelt_stdlib::runtime_symbols::number_text::NUMBER_TO_STRING_FN,
                text = self.operand_expr(operand)?.delimited(),
            ))),
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. }) => {
                let text = self
                    .erase_concrete_union_expr(&self.operand_expr(operand)?, self.operand_ty(operand)?)?;
                Ok(self.js_string_coercion_match_expr(&text, self.absent_spelling(), self.indexes_utf16())?)
            }
            Some(Type::Class { name, .. }) if self.is_regexp_class_symbol(*name)? => {
                Ok(Self::regexp_literal_expr(&self.operand_expr(operand)?))
            }
            // A value the source spelled `null` stringifies as the WORD, not as
            // the empty string: `String(null)` is `"null"` in JavaScript and
            // `str(None)` is `"None"` in Python. `Never` is unreachable code and
            // keeps the empty placeholder.
            Some(Type::None) => Ok(RustExpr::string_literal(self.absent_spelling().null_text())),
            Some(Type::Never) => Ok(RustExpr::atom(self.text_empty())),
            // A typed-array VIEW stringifies as its joined elements, because
            // `TypedArray.prototype.toString` is `Array.prototype.toString`;
            // byte STORAGE has no elements and keeps the object tag, which for
            // it is `[object ArrayBuffer]`.
            Some(Type::Class { name, .. })
                if self.stdlib_class_of_symbol(*name)?
                    == Some(smelt_stdlib::StdlibClass::TypedArray) =>
            {
                Ok(self.operand_expr(operand)?.postfix(".to_js_string()"))
            }
            // Storage keeps the object tag, and the tag is the VALUE's class
            // name rather than a fixed word: `SharedArrayBuffer` shares this
            // Rust type and stringifies as `[object SharedArrayBuffer]`.
            Some(Type::Class { name, .. })
                if self.stdlib_class_of_symbol(*name)?
                    == Some(smelt_stdlib::StdlibClass::ArrayBuffer) =>
            {
                Ok(RustExpr::atom(format!(
                    "format!(\"[object {{}}]\", {}.class_name())",
                    self.operand_expr(operand)?.tight()
                )))
            }
            // `URL` and `URLSearchParams` define their own `toString`: the
            // `href` and the urlencoded query.
            Some(Type::Class { name, .. })
                if self.stdlib_class_of_symbol(*name)? == Some(smelt_stdlib::StdlibClass::Url) =>
            {
                Ok(self.operand_expr(operand)?.postfix(".href()"))
            }
            Some(Type::Class { name, .. })
                if self.stdlib_class_of_symbol(*name)?
                    == Some(smelt_stdlib::StdlibClass::UrlSearchParams) =>
            {
                Ok(self.operand_expr(operand)?.postfix(".to_text()"))
            }
            // A `DataView` has no elements to join, so it keeps its tag too.
            Some(Type::Class { name, .. })
                if self.stdlib_class_of_symbol(*name)?
                    == Some(smelt_stdlib::StdlibClass::DataView) =>
            {
                Ok(RustExpr::chain("\"[object DataView]\".to_owned()"))
            }
            Some(Type::List(_) | Type::Set(_) | Type::Dict(_, _) | Type::Class { .. }) => {
                Ok(RustExpr::chain("\"[object Object]\".to_owned()"))
            }
            Some(Type::JsMap(_, _)) => Ok(RustExpr::chain("\"[object Map]\".to_owned()")),
            // An ABSENT optional stringifies as the language's absent word, not
            // as the empty string: `unwrap_or_default()` answered `""` for
            // every `${maybeString}` in the corpus.
            Some(Type::Optional(inner)) if self.mir.types.get(*inner).is_some_and(Type::is_text) => {
                Ok({ let receiver = self.operand_expr(operand)?; receiver.postfix(format!(".unwrap_or_else(|| {})", self.text_literal(self.absent_spelling().text()).delimited())) })
            }
            Some(Type::Optional(inner))
                if self.has_dynamic_shape(*inner) =>
            {
                let text = self.operand_expr(operand)?;
                // A concrete-union `Option` payload stores a tagged enum, so each
                // present value projects back to `SmeltUnknown` before the erased
                // string-coercion match.
                let scrutinee = self.erase_concrete_union_expr(&RustExpr::path("smelt_value"), *inner)?;
                let error_arm = Self::js_error_to_string_arm_text();
                // The absent arm carries no runtime tag, so it takes the
                // language's absent word; the present arms still distinguish
                // `null` from `undefined`, which the tag does know.
                let absent_text = self.absent_spelling().text();
                let null_text = self.absent_spelling().null_text();
                Ok(text.postfix(self.carrier_format(self.carrier()?, format_args!(
                    ".map_or_else(|| \"{absent_text}\".to_owned(), |smelt_value| match {scrutinee} {{ SmeltUnknown::Null => \"{null_text}\".to_owned(), SmeltUnknown::Undefined => \"{absent_text}\".to_owned(), SmeltUnknown::Bool(smelt_value) => smelt_value.to_string(), SmeltUnknown::Number(smelt_value) => smelt_number_to_string(smelt_value), {smelt_bigint_arm}SmeltUnknown::String(smelt_value) => smelt_value.to_string(), SmeltUnknown::Symbol(smelt_value) => smelt_symbol_to_string(&smelt_value), {error_arm}SmeltUnknown::Array(smelt_values) => smelt_erased_to_js_string(&SmeltUnknown::Array(smelt_values)).to_string_lossy(), SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () {{ [native code] }}\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned() }})", smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => value.to_string(),"), scrutinee = scrutinee.delimited()
                ))))
            }
            // An optional PRIMITIVE stringifies its value or the absent word.
            // This is the `${maybeNumber}` / `${maybeFlag}` shape, and it used
            // to fall into the placeholder below and render nothing at all.
            Some(Type::Optional(inner))
                if matches!(
                    self.mir.types.get(*inner),
                    Some(Type::Bool | Type::Int | Type::Float)
                ) =>
            {
                Ok({ let receiver = self.operand_expr(operand)?; receiver.postfix(format!(".map_or_else(|| {:?}.to_owned(), |smelt_value| smelt_value.to_string())", self.absent_spelling().text())) })
            }
            // An optional whose payload is a structured value keeps the empty
            // placeholder for the PRESENT arm — JavaScript renders a list as
            // its joined elements and an object as `[object Object]`, neither of
            // which this seam models yet — but the absent arm is the word.
            Some(Type::Optional(_)) => Ok({ let receiver = self.operand_expr(operand)?; receiver.postfix(format!(".map_or_else(|| {:?}.to_owned(), |_| String::new())", self.absent_spelling().text())) }),
            Some(Type::Tuple(_) | Type::Future(_)) => Ok(RustExpr::atom(self.text_empty())),
            Some(Type::Function(_) | Type::Generator { .. } | Type::GeneratorResult { .. })
            | None => {
                Ok(RustExpr::atom(self.text_empty()))
            }
        }
    }

    /// Converts a string character lookup operation to Rust text.
    pub(super) fn string_char_at_expr(
        &self,
        operand: &Operand,
        index: &Operand,
    ) -> Result<RustExpr, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::String | Type::PyStr)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(index)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new(
                "string charAt requires a string receiver and numeric index",
            ));
        }
        // Read by borrow: the read only looks, and an owned clone of the
        // receiver would copy the whole string per character read.
        // A negative index reads nothing; NaN reads index 0, as `as usize` does.
        let receiver = self.operand_borrow_expr(operand)?;
        Ok(RustExpr::atom(format!(
            "{{ let smelt_index = {index} as f64; if smelt_index < 0.0 {{ None }} else {{ {char_at} }} }}.unwrap_or_default()",
            index = self.operand_expr(index)?.as_cast_operand(),
            char_at = self.string_char_at_text(&receiver.parenthesized_if_needed(), "smelt_index as usize"),
        )))
    }

    /// Converts a string character-code lookup operation to Rust text.
    pub(super) fn string_char_code_at_expr(
        &self,
        operand: &Operand,
        index: &Operand,
    ) -> Result<RustExpr, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::String | Type::PyStr)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(index)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new(
                "string charCodeAt requires a string receiver and numeric index",
            ));
        }
        // Read by borrow, as `string_char_at_expr` does: a scanning loop
        // (`for (i..) url.charCodeAt(i)`, Hono's `getPath` on every request)
        // copied the whole string once per character. When every byte up to
        // and including `index` is ASCII, the character at `index` IS that
        // byte, so the code is read directly instead of decoding the prefix
        // character by character; any other string takes the decoding path,
        // which is exactly the previous text. The closure keeps a borrowed
        // receiver's temporaries alive for the whole read.
        let code_at = self.string_code_unit_at_text("smelt_text", "smelt_index");
        Ok(RustExpr::atom(format!(
            "(|smelt_text: &{text_type}, smelt_index: f64| if smelt_index < 0.0 {{ f64::NAN }} else {{ let smelt_index = smelt_index as usize; {code_at} }})(&{}, {} as f64)",
            self.operand_borrow_expr(operand)?.tight(),
            self.operand_expr(index)?.as_cast_operand(),
            text_type = if self.indexes_utf16() { "JsStr" } else { "str" },
        )))
    }

    /// Converts `s.codePointAt(i)` to an `Option<f64>`: the code point starting
    /// at index `i` (a whole surrogate pair in a JavaScript body), `None` past
    /// either end.
    pub(super) fn string_code_point_at_expr(
        &self,
        operand: &Operand,
        index: &Operand,
    ) -> Result<RustExpr, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::String | Type::PyStr)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(index)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new(
                "string codePointAt requires a string receiver and numeric index",
            ));
        }
        let receiver = self.operand_borrow_expr(operand)?.parenthesized_if_needed();
        let code_point = self.string_code_point_at_text(&receiver, "smelt_index as usize");
        Ok(RustExpr::atom(format!(
            "{{ let smelt_index = {index} as f64; if smelt_index < 0.0 {{ None }} else {{ {code_point}.map(f64::from) }} }}",
            index = self.operand_expr(index)?.as_cast_operand(),
        )))
    }

    /// Converts a string containment operation to Rust text.
    ///
    /// Mirrors `String.prototype.includes(needle, position?)`. When `from_index`
    /// is present the search starts at that position: JavaScript truncates the
    /// position toward zero and clamps it into `[0, length]`, so the emitted code
    /// takes the character prefix at that position (matching the char-based
    /// convention shared with `string_search_expr` and the other string helpers)
    /// and searches the remaining suffix. A `position` past the end can only match
    /// the empty needle, which `str::contains` handles for free.
    ///
    /// The erased-value fast paths only apply when no `position` is given; the
    /// positional form coerces a non-String haystack to a string before matching.
    pub(super) fn string_contains_expr(
        &self,
        haystack: &Operand,
        needle: &Operand,
        from_index: Option<&Operand>,
    ) -> Result<RustExpr, EmitError> {
        let haystack_ty = self.operand_ty(haystack)?;
        let needle_ty = self.operand_ty(needle)?;
        if from_index.is_none() && self.mir.types.get(haystack_ty) == Some(&Type::Unknown) {
            return Ok({ let receiver = self.operand_expr(haystack)?; receiver.postfix(format!(".includes({})", self.operand_expr(needle)?.delimited())) });
        }
        // Other erased haystacks (unscoped type parameters, unions, `never`,
        // erased classes) reach here through specialization; erase the value to
        // the runtime `SmeltUnknown` boundary and use the same `.includes`
        // helper as the plain-`Unknown` fast path above.
        if from_index.is_none()
            && (matches!(
                self.mir.types.get(haystack_ty),
                Some(Type::Union(_) | Type::TypeParam { .. } | Type::Never)
            ) || self.is_erased_class_type(haystack_ty))
        {
            let erased_haystack = self.erase_rendered(
                &self.operand_expr(haystack)?.cloned_value(),
                haystack_ty,
            )?;
            return Ok(erased_haystack.postfix(format!(
                ".includes({})",
                self.operand_expr(needle)?.delimited()
            )));
        }
        if from_index.is_none()
            && self.mir.types.get(needle_ty) == Some(&Type::Unknown)
            && self.mir.types.get(haystack_ty).is_some_and(Type::is_text)
        {
            let erased_haystack = self.erase_rendered(
                &self.operand_expr(haystack)?.cloned_value(),
                self.type_id(Type::String)?,
            )?;
            return Ok(erased_haystack.postfix(format!(
                ".includes({})",
                self.operand_expr(needle)?.delimited()
            )));
        }
        if !matches!(self.mir.types.get(needle_ty), Some(Type::String | Type::PyStr)) {
            return Err(EmitError::new(format!(
                "string contains needle must be a string, got {}",
                Self::type_text_for(self.mir, needle_ty)
                    .unwrap_or_else(|_error| format!("{needle_ty:?}"))
            )));
        }
        if let Some(from_index_operand) = from_index {
            // A non-string haystack (e.g. an erased value reaching the
            // positional form through specialization) coerces through the
            // shared `value_at_type` conversion; genuinely unconvertible
            // shapes still produce that helper's honest error.
            let haystack_text = if matches!(self.mir.types.get(haystack_ty), Some(Type::String | Type::PyStr)) {
                self.operand_expr(haystack)?
            } else {
                self.value_at_type(haystack, self.type_id(Type::String)?)?
            };
            let needle_text = self.operand_expr(needle)?;
            let index_text = self.value_at_type(from_index_operand, self.type_id(Type::Float)?)?;
            if self.indexes_utf16() {
                return Ok(RustExpr::atom(format!(
                    "{{ let smelt_haystack = {haystack_text}; let smelt_from = ({index_text} as i64).max(0) as usize; smelt_haystack.index_of(&{needle_text}, smelt_from).is_some() }}", haystack_text = haystack_text.delimited(), index_text = index_text.as_cast_operand(), needle_text = needle_text.tight()
                )));
            }
            return Ok(RustExpr::atom(format!(
                "{{ let smelt_haystack = {haystack_text}; let smelt_from = ({index_text} as i64).max(0) as usize; let smelt_prefix_bytes = {prefix_bytes}; smelt_haystack.get(smelt_prefix_bytes..).map_or(false, |smelt_suffix| smelt_suffix.contains(&{needle_text})) }}",
                prefix_bytes = self.string_byte_of_text("smelt_haystack", "smelt_from"), haystack_text = haystack_text.delimited(), index_text = index_text.as_cast_operand(), needle_text = needle_text.tight(),
            )));
        }
        if !matches!(self.mir.types.get(haystack_ty), Some(Type::String | Type::PyStr)) {
            return Err(EmitError::new(format!(
                "string contains haystack must be a string, got {}",
                Self::type_text_for(self.mir, haystack_ty)
                    .unwrap_or_else(|_error| format!("{haystack_ty:?}"))
            )));
        }
        Ok({ let receiver = self.operand_expr(haystack)?; receiver.postfix(format!(".contains(&{})", self.operand_expr(needle)?.tight())) })
    }

    /// Converts a string slice operation to Rust text, under the bound rule
    /// `kind` (see [`smelt_hir::StringSliceKind`]).
    pub(super) fn string_slice_expr(
        &self,
        operand: &Operand,
        start: Option<&Operand>,
        end: Option<&Operand>,
        kind: smelt_hir::StringSliceKind,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        self.validate_optional_numeric_index(start, "string slice start index")?;
        self.validate_optional_numeric_index(end, "string slice end index")?;
        let result = if kind != smelt_hir::StringSliceKind::Slice {
            self.string_substring_expr(operand, start, end, kind)?
        } else if matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::String | Type::PyStr)
        ) {
            // Read by borrow: slicing only looks at the receiver, and its
            // length is measured once for both bounds.
            let operand_expr = self.operand_borrow_expr(operand)?;
            let len_source = RustExpr::path(SMELT_STRING_LEN);
            let start_text = self.slice_start_expr(start, &len_source)?;
            let len_expr = self.slice_len_expr_from_len(&len_source, start, end)?;
            RustExpr::atom(format!(
                "{{ let {SMELT_STRING_LEN} = {len}; {slice} }}",
                len = self.string_len_text(&operand_expr.parenthesized_if_needed()),
                slice = self.string_slice_text(
                    &operand_expr.parenthesized_if_needed(),
                    start_text.as_str(),
                    len_expr.as_str(),
                ),
            ))
        } else {
            let operand_expr = self.string_like_operand_expr(operand, "string slice")?;
            let receiver_text = SMELT_STRING;
            let len_source = RustExpr::path(SMELT_STRING_LEN);
            let start_text = self.slice_start_expr(start, &len_source)?;
            let len_expr = self.slice_len_expr_from_len(&len_source, start, end)?;
            RustExpr::atom(format!(
                "{{ let {receiver_text} = {operand_expr}; let {SMELT_STRING_LEN} = {len}; {slice} }}",
                len = self.string_len_text(receiver_text),
                slice = self.string_slice_text(receiver_text, start_text.as_str(), len_expr.as_str()), operand_expr = operand_expr.delimited(),
            ))
        };
        if matches!(
            self.mir.types.get(dest_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) {
            self.erase_rendered(&result, self.type_id(Type::String)?)
        } else {
            Ok(result)
        }
    }

    /// Render `substring(start, end)` or `substr(start, length)` over a string
    /// operand: each bound is `ToIntegerOrInfinity`'d (NaN is 0) and clamped,
    /// `substring` swaps reversed bounds, and `substr` counts a negative start
    /// from the end and takes at most `length` units.
    fn string_substring_expr(
        &self,
        operand: &Operand,
        start: Option<&Operand>,
        end: Option<&Operand>,
        kind: smelt_hir::StringSliceKind,
    ) -> Result<RustExpr, EmitError> {
        let operand_expr = self.string_like_operand_expr(operand, "string substring")?;
        let bound = |given: Option<&Operand>, absent: &str| -> Result<String, EmitError> {
            Ok(match given {
                Some(bound) => format!(
                    "{{ let smelt_bound = {} as f64; if smelt_bound.is_nan() {{ 0.0 }} else {{ smelt_bound.trunc() }} }}",
                    self.operand_expr(bound)?.as_cast_operand()
                ),
                None => absent.to_owned(),
            })
        };
        let start_text = bound(start, "0.0_f64")?;
        let (from, count) = if kind == smelt_hir::StringSliceKind::Substr {
            let length_text = bound(end, "f64::INFINITY")?;
            (
                format!("{{ let smelt_start = {start_text}; if smelt_start < 0.0 {{ (smelt_len + smelt_start).max(0.0) }} else {{ smelt_start.min(smelt_len) }} }}"),
                format!("{length_text}.clamp(0.0, smelt_len - smelt_from)"),
            )
        } else {
            let end_text = bound(end, "smelt_len")?;
            (
                format!("{start_text}.clamp(0.0, smelt_len).min({end_text}.clamp(0.0, smelt_len))"),
                format!("{start_text}.clamp(0.0, smelt_len).max({end_text}.clamp(0.0, smelt_len)) - smelt_from"),
            )
        };
        Ok(RustExpr::atom(format!(
            "{{ let smelt_text = {operand_expr}; let smelt_len = {len} as f64; let smelt_from = {from}; let smelt_count = {count}; {slice} }}",
            len = self.string_len_text("smelt_text"),
            slice = self.string_slice_text("smelt_text", "smelt_from as usize", "smelt_count as usize"), operand_expr = operand_expr.delimited(),
        )))
    }

    /// Converts a list containment operation to Rust text.
    /// Converts a string split operation to Rust text.
    ///
    /// JavaScript accepts both literal strings and `RegExp` separator values.
    /// Preserve RegExp flags and matching behavior instead of stringifying the
    /// separator pattern.
    pub(super) fn string_split_expr(
        &self,
        haystack: &Operand,
        separator: &Operand,
        limit: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let haystack_text = self.string_like_operand_expr(haystack, "string split")?;
        let split_items = if matches!(
            self.mir.types.get(self.operand_ty(separator)?),
            Some(Type::Class { name, .. }) if self.is_regexp_class_symbol(*name)?
        ) {
            let regex_text = self.regexp_operand_expr(separator)?;
            format!("{regex_text}.split_string(&{haystack_text})", regex_text = regex_text.tight(), haystack_text = haystack_text.tight())
        } else if matches!(
            self.mir.types.get(self.operand_ty(separator)?),
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
        ) {
            let separator_unknown = self
                .erase_concrete_union_expr(&self.operand_expr(separator)?, self.operand_ty(separator)?)?;
            self.carrier_format(self.carrier()?, format_args!(
                "{{ let smelt_haystack = {haystack_text}; match {separator_unknown} {{ SmeltUnknown::Object(smelt_object) if smelt_object.contains_key(\"source\") => SmeltRegExp::new(match smelt_object.get(\"source\").cloned() {{ Some(SmeltUnknown::String(source)) => source.to_string(), _ => String::new() }}, match smelt_object.get(\"flags\").cloned() {{ Some(SmeltUnknown::String(flags)) => flags.to_string(), _ => String::new() }}).split_string(&smelt_haystack), smelt_separator_unknown => {{ let smelt_separator = match smelt_separator_unknown {{ SmeltUnknown::Null => String::new(), SmeltUnknown::Undefined => \"undefined\".to_owned(), SmeltUnknown::Bool(smelt_value) => smelt_value.to_string(), SmeltUnknown::Number(smelt_value) => smelt_number_to_string(smelt_value), {smelt_bigint_arm}SmeltUnknown::String(smelt_value) => smelt_value.to_string(), SmeltUnknown::Symbol(smelt_value) => smelt_symbol_to_string(&smelt_value), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () {{ [native code] }}\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned() }}; {split} }} }} }}", split = if self.indexes_utf16() {
                    "smelt_haystack.split(&JsString::from(smelt_separator), usize::MAX)".to_owned()
                } else {
                    format!("if smelt_separator.is_empty() {{ if smelt_haystack.is_empty() {{ Vec::new() }} else {{ {} }} }} else {{ smelt_haystack.split(&smelt_separator).map(str::to_owned).collect::<Vec<_>>() }}", self.string_units_text("smelt_haystack"))
                }, smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => value.to_string(),"), haystack_text = haystack_text.delimited(), separator_unknown = separator_unknown.delimited()
            ))
        } else if self.indexes_utf16() {
            // `JsStr::split` answers JavaScript's cases itself: an empty
            // separator splits into code units, and only `"".split("")` is
            // empty.
            let separator_text = self.string_like_operand_expr(separator, "string split")?;
            format!("{{ let smelt_haystack = {haystack_text}; let smelt_separator = {separator_text}; smelt_haystack.split(&smelt_separator, usize::MAX) }}", haystack_text = haystack_text.delimited(), separator_text = separator_text.delimited())
        } else {
            let separator_text = self.string_like_operand_expr(separator, "string split")?;
            format!(
                "{{ let smelt_haystack = {haystack_text}; let smelt_separator = {separator_text}; if smelt_separator.is_empty() {{ if smelt_haystack.is_empty() {{ Vec::new() }} else {{ {units} }} }} else {{ smelt_haystack.split(&smelt_separator).map(str::to_owned).collect::<Vec<_>>() }} }}",
                units = self.string_units_text("smelt_haystack"), haystack_text = haystack_text.delimited(), separator_text = separator_text.delimited(),
            )
        };
        let result = RustExpr::from(if let Some(limit_operand) = limit {
            let limit_text = self.operand_expr(limit_operand)?;
            match self.mir.types.get(self.operand_ty(limit_operand)?) {
                Some(Type::None) => Ok(split_items),
                Some(Type::Int | Type::Float) => Ok(format!(
                    "{{ let mut smelt_parts = {split_items}; let smelt_limit = {limit_text} as f64; if !smelt_limit.is_finite() || smelt_limit == 0.0 {{ smelt_parts.truncate(0); }} else if smelt_limit.is_sign_positive() {{ smelt_parts.truncate(smelt_limit.floor() as usize); }} smelt_parts }}", limit_text = limit_text.as_cast_operand()
                )),
                Some(Type::Optional(inner))
                    if matches!(self.mir.types.get(*inner), Some(Type::Int | Type::Float)) =>
                {
                    Ok(format!(
                        "{{ let mut smelt_parts = {split_items}; if let Some(smelt_split_limit) = {limit_text} {{ let smelt_limit = smelt_split_limit as f64; if !smelt_limit.is_finite() || smelt_limit == 0.0 {{ smelt_parts.truncate(0); }} else if smelt_limit.is_sign_positive() {{ smelt_parts.truncate(smelt_limit.floor() as usize); }} }} smelt_parts }}", limit_text = limit_text.delimited()
                    ))
                }
                Some(Type::Optional(inner))
                    if matches!(
                        self.mir.types.get(*inner),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    ) =>
                {
                    // A concrete-union `Option` payload stores a tagged enum, so
                    // project each present value back to `SmeltUnknown` before
                    // matching the erased number arm.
                    let limit_scrutinee = if self.concrete_union_members(*inner).is_some() {
                        self.carrier_format(self.carrier()?, format_args!("{limit_text}.map(|smelt_value| smelt_value.into_smelt_unknown())", limit_text = limit_text.tight()))
                    } else {
                        limit_text.into_string()
                    };
                    Ok(self.carrier_format(self.carrier()?, format_args!(
                        "{{ let mut smelt_parts = {split_items}; if let Some(smelt_split_limit) = match {limit_scrutinee} {{ Some(SmeltUnknown::Number(smelt_value)) => Some(smelt_value), Some(SmeltUnknown::Null) | None => None, _ => None }} {{ if !smelt_split_limit.is_finite() || smelt_split_limit == 0.0 {{ smelt_parts.truncate(0); }} else if smelt_split_limit.is_sign_positive() {{ smelt_parts.truncate(smelt_split_limit.floor() as usize); }} }} smelt_parts }}"
                    )))
                }
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => {
                    let limit_scrutinee = self
                        .erase_concrete_union_expr(&limit_text, self.operand_ty(limit_operand)?)?;
                    Ok(self.carrier_format(self.carrier()?, format_args!(
                        "{{ let mut smelt_parts = {split_items}; if let Some(smelt_split_limit) = match {limit_scrutinee} {{ SmeltUnknown::Number(smelt_value) => Some(smelt_value), SmeltUnknown::Null | SmeltUnknown::Undefined => None, _ => None }} {{ if !smelt_split_limit.is_finite() || smelt_split_limit == 0.0 {{ smelt_parts.truncate(0); }} else if smelt_split_limit.is_sign_positive() {{ smelt_parts.truncate(smelt_split_limit.floor() as usize); }} }} smelt_parts }}", limit_scrutinee = limit_scrutinee.delimited()
                    )))
                }
                _ => Err(EmitError::new(
                    "string split limit must be numeric or optional numeric",
                )),
            }
        } else {
            Ok(split_items)
        }?);
        if matches!(
            self.mir.types.get(dest_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) {
            Ok(self.erase_string_array_expr(&result)?)
        } else {
            Ok(result)
        }
    }

    /// Converts a string into a list of one-character strings.
    pub(super) fn string_chars_expr(
        &self,
        haystack: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let haystack_text = self.string_like_operand_expr(haystack, "string chars")?;
        let text = if self.indexes_utf16() {
            RustExpr::atom_on(&haystack_text, format!("{haystack_text}.code_point_strings()", haystack_text = haystack_text.tight()))
        } else {
            RustExpr::atom_on(&haystack_text, format!("{haystack_text}.chars().map(|smelt_ch| smelt_ch.to_string()).collect::<Vec<_>>()", haystack_text = haystack_text.tight()))
        };
        if let Some(Type::List(item_ty)) = self.mir.types.get(dest_ty)
            && self.mir.types.get(*item_ty).is_none_or(|candidate| !candidate.is_text())
        {
            let item_text =
                self.rendered_at_type(&RustExpr::path("smelt_value"), self.type_id(self.text_type())?, *item_ty, &self.render_scope())?;
            return Ok(RustExpr::atom_on(&text, format!(
                "{text}.into_iter().map(|smelt_value| {item_text}).collect::<Vec<_>>()", text = text.tight(), item_text = item_text.delimited()
            )));
        }
        Ok(text)
    }

    /// Converts a string join operation to Rust text.
    pub(super) fn string_join_expr(
        &self,
        items: &Operand,
        separator: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let items_ty = self.operand_ty(items)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(items_ty) else {
            return Ok(RustExpr::atom(self.text_empty()));
        };
        let separator_text = self.string_like_operand_expr(separator, "string join")?;
        let items_text = list_read_expr(&self.operand_expr(items)?);
        if self.mir.types.get(*item_ty).is_some_and(Type::is_text) {
            if self.indexes_utf16() {
                return Ok(RustExpr::chain(format!("JsString::join({items_text}.iter(), &{separator_text})", items_text = items_text.tight(), separator_text = separator_text.tight())));
            }
            return Ok(items_text.postfix(format!(".join(&{separator_text})", separator_text = separator_text.tight())));
        }
        let item_text = match self.mir.types.get(*item_ty) {
            Some(Type::Bool) => "smelt_item.to_string()".to_owned(),
            // `[1e21].join(",")` stringifies each item the way `String(item)`
            // does, so the numeric arms take the same helper.
            Some(Type::Int) => format!(
                "{fn_name}(*smelt_item as f64)",
                fn_name = smelt_stdlib::runtime_symbols::number_text::NUMBER_TO_STRING_FN,
            ),
            Some(Type::Float) => format!(
                "{fn_name}(*smelt_item)",
                fn_name = smelt_stdlib::runtime_symbols::number_text::NUMBER_TO_STRING_FN,
            ),
            // A union element (`(number | string)[]`) stringifies each value at
            // its runtime arm: erase it to the tagged value and take the same
            // `String(item)` arms an `unknown` element does.
            Some(Type::Unknown | Type::Union(_)) => self.carrier_format(self.carrier()?, format_args!(
                "match {item} {{ SmeltUnknown::Null | SmeltUnknown::Undefined => String::new(), SmeltUnknown::Bool(smelt_value) => smelt_value.to_string(), SmeltUnknown::Number(smelt_value) => smelt_number_to_string(smelt_value), {smelt_bigint_arm}SmeltUnknown::String(smelt_value) => smelt_value.to_string(), SmeltUnknown::Symbol(smelt_value) => smelt_symbol_to_string(&smelt_value), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () {{ [native code] }}\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned() }}",
                smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => value.to_string(),"),
                item = if matches!(self.mir.types.get(*item_ty), Some(Type::Union(_))) {
                    format!("({})", self.erase_rendered(&RustExpr::path("smelt_item").cloned_value(), *item_ty)?.delimited())
                } else {
                    "smelt_item".to_owned()
                },
            )),
            Some(Type::Optional(inner)) => match self.mir.types.get(*inner) {
                Some(Type::Bool) => {
                    "smelt_item.as_ref().map_or_else(String::new, |smelt_value| smelt_value.to_string())".to_owned()
                }
                Some(Type::Int) => format!(
                    "smelt_item.as_ref().map_or_else(String::new, |smelt_value| {fn_name}(*smelt_value as f64))",
                    fn_name = smelt_stdlib::runtime_symbols::number_text::NUMBER_TO_STRING_FN,
                ),
                Some(Type::Float) => format!(
                    "smelt_item.as_ref().map_or_else(String::new, |smelt_value| {fn_name}(*smelt_value))",
                    fn_name = smelt_stdlib::runtime_symbols::number_text::NUMBER_TO_STRING_FN,
                ),
                Some(Type::String | Type::PyStr) => {
                    let to_string = if self.indexes_utf16() { "|value| value.to_string_lossy()" } else { "Clone::clone" };
                    format!("smelt_item.as_ref().map_or_else(String::new, {to_string})")
                }
                _ => {
                    return Err(EmitError::new(
                        "string join optional items must contain primitive values",
                    ));
                }
            },
            _ => {
                return Err(EmitError::new(
                    "string join items must be strings, primitives, or unknown",
                ));
            }
        };
        if self.indexes_utf16() {
            return Ok(RustExpr::chain(format!(
                "JsString::join({items_text}.iter().map(|smelt_item| JsString::from({{ {item_text} }})), &{separator_text})", items_text = items_text.tight(), separator_text = separator_text.tight()
            )));
        }
        Ok(RustExpr::atom_on(&items_text, format!(
            "{items_text}.iter().map(|smelt_item| {{ {item_text} }}).collect::<Vec<_>>().join(&{separator_text})", items_text = items_text.tight(), separator_text = separator_text.tight()
        )))
    }

}
