//! Strings emission helpers.

use crate::rust::RustExpr;
use super::*;

impl FunctionEmitter<'_> {
    /// Converts a string case operation to its Rust text representation.
    pub(super) fn string_case_expr(
        &self,
        op: smelt_hir::StringCaseOp,
        operand: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let receiver_text = self.string_like_operand_expr(operand, "string case")?;
        let method_name = match op {
            smelt_hir::StringCaseOp::Lower => "to_lowercase",
            smelt_hir::StringCaseOp::Upper => "to_uppercase",
        };
        // The case operation always yields a `String`. A `.to_lowercase()` /
        // `.to_uppercase()` call is a postfix expression, so the rendered value
        // is an atom: erasing it through the coercion seam reproduces the
        // historical `SmeltUnknown::String(...)` wrapping while letting the seam
        // (not this caller) own the parenthesization decision.
        let result = RustExpr::postfix_on(&receiver_text, format!("{receiver_text}.{method_name}()"));
        if matches!(
            self.mir.types.get(dest_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) {
            let string_ty = self.type_id(Type::String)?;
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
        Ok(RustExpr::opaque(format!(
            "unicode_normalization::UnicodeNormalization::{method_name}({receiver_text}.as_str()).collect::<String>()"
        )))
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
        Ok(RustExpr::chain(format!("{helper}({receiver_text}.as_str())")))
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
            "{locale_compare}({left_text}.as_str(), {right_text}.as_str())",
            locale_compare = smelt_stdlib::runtime_symbols::strings::LOCALE_COMPARE,
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
            Some(Type::String)
        ) {
            return Err(EmitError::new("string trim operand must be a string"));
        }
        let method_name = match side {
            smelt_hir::StringTrimSide::Both => "trim",
            smelt_hir::StringTrimSide::Start => "trim_start",
            smelt_hir::StringTrimSide::End => "trim_end",
        };
        Ok({ let receiver = self.operand_expr(operand)?; RustExpr::postfix_on(&receiver, format!("{receiver}.{method_name}().to_owned()")) })
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
        Ok(RustExpr::postfix_on(&haystack_text, format!("{haystack_text}.{method_name}(&{needle_text})")))
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
        let haystack_text = self.string_like_operand_expr(haystack, "string search")?;
        let needle_text = self.string_like_operand_expr(needle, "string search")?;
        if let Some(from_index_operand) = from_index {
            let index_text = self.value_at_type(from_index_operand, self.type_id(Type::Float)?)?;
            return match op {
                smelt_hir::StringSearchOp::Find => Ok(RustExpr::atom(format!(
                    "{{ let smelt_haystack = {haystack_text}; let smelt_needle = {needle_text}; let smelt_from = ({index_text} as i64).max(0) as usize; let smelt_prefix_bytes = smelt_haystack.char_indices().nth(smelt_from).map_or(smelt_haystack.len(), |(byte, _)| byte); smelt_haystack.get(smelt_prefix_bytes..).and_then(|suffix| suffix.find(&smelt_needle).map(|index| (smelt_prefix_bytes + index) as {cast})).unwrap_or({missing}) }}"
                ))),
                smelt_hir::StringSearchOp::RFind => Ok(RustExpr::atom(format!(
                    "{{ let smelt_haystack = {haystack_text}; let smelt_needle = {needle_text}; let smelt_from = ({index_text} as i64).max(0) as usize; let smelt_end_char = smelt_from.saturating_add(smelt_needle.chars().count()); let smelt_end_byte = smelt_haystack.char_indices().nth(smelt_end_char).map_or(smelt_haystack.len(), |(byte, _)| byte); smelt_haystack[..smelt_end_byte].rfind(&smelt_needle).map_or({missing}, |idx| idx as {cast}) }}"
                ))),
                // `search` takes no position argument; the frontend never
                // builds one.
                smelt_hir::StringSearchOp::Regex => {
                    Err(EmitError::new("String.search takes no fromIndex"))
                }
            };
        }
        Ok(RustExpr::postfix_on(&haystack_text, format!(
            "{haystack_text}.{method_name}(&{needle_text}).map_or({missing}, |idx| idx as {cast})"
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
        let regex_text = if self.mir.types.get(self.operand_ty(needle)?) == Some(&Type::String) {
            format!("SmeltRegExp::new({}, String::new())", self.operand_expr(needle)?)
        } else {
            self.operand_expr(needle)?.into_string()
        };
        Ok(RustExpr::atom(format!(
            "{{ let smelt_haystack = {haystack_text}; let smelt_regex = {regex_text}; match smelt_regex.compiled().find(&smelt_haystack) {{ Ok(Some(smelt_found)) => smelt_haystack[..smelt_found.start()].encode_utf16().count() as {cast}, _ => {missing} }} }}"
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
            return Ok(RustExpr::postfix_on(&regex_text, format!(
                "{regex_text}.replace_string(&{haystack_text}, &{replacement_text}, {force_all})"
            )));
        }
        let pattern_text = self.string_like_operand_expr(pattern, "string replace")?;
        match op {
            smelt_hir::StringReplaceOp::First => Ok(RustExpr::postfix_on(&haystack_text, format!(
                "{haystack_text}.replacen(&{pattern_text}, &{replacement_text}, 1)"
            ))),
            smelt_hir::StringReplaceOp::All => Ok(RustExpr::postfix_on(&haystack_text, format!(
                "{haystack_text}.replace(&{pattern_text}, &{replacement_text})"
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
        Ok(RustExpr::postfix_on(&haystack_text, format!(
            "{haystack_text}.{method_name}(&{affix_text}).unwrap_or(&{haystack_text}).to_owned()"
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
            Some(Type::String)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(count)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new(
                "string repeat requires a string receiver and numeric count",
            ));
        }
        Ok({ let receiver = self.operand_expr(operand)?; RustExpr::postfix_on(&receiver, format!("{receiver}.repeat({} as usize)", self.operand_expr(count)?)) })
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
            Some(Type::String)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(target_len)?),
            Some(Type::Int | Type::Float)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(pad)?),
            Some(Type::String)
        ) {
            return Err(EmitError::new(
                "string padding requires string receiver, numeric target length, and string padding",
            ));
        }
        let operand_expr = self.operand_expr(operand)?;
        let target_len_text = self.operand_expr(target_len)?;
        let pad_text = self.operand_expr(pad)?;
        let result_text = match op {
            smelt_hir::StringPadOp::Start => "format!(\"{}{}\", padding, value)",
            smelt_hir::StringPadOp::End => "format!(\"{}{}\", value, padding)",
        };
        Ok(RustExpr::atom(format!(
            "{{ let value = &{operand_expr}; let target_len = {target_len_text} as usize; let pad = &{pad_text}; let current_len = value.chars().count(); if current_len >= target_len || pad.is_empty() {{ value.to_owned() }} else {{ let needed = target_len - current_len; let padding: String = pad.chars().cycle().take(needed).collect(); {result_text} }} }}"
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
            Some(Type::String)
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
            "!{operand_expr}.is_empty() && {operand_expr}.chars().all(char::{method_name})"
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
        let pattern_text = self.string_like_operand_expr(pattern, "regex match")?;
        let haystack_text = self.string_like_operand_expr(haystack, "regex match")?;
        let regex_text =
            RustExpr::atom(format!("fancy_regex::Regex::new(&{pattern_text}).expect(\"regex compile failed\")"));
        Ok(match op {
            smelt_hir::RegexMatchOp::Search => {
                RustExpr::postfix_on(&regex_text, format!("{regex_text}.is_match(&{haystack_text}).unwrap_or(false)"))
            }
            smelt_hir::RegexMatchOp::Match => {
                RustExpr::postfix_on(&regex_text, format!(
                    "{regex_text}.find(&{haystack_text}).ok().flatten().is_some_and(|m| m.start() == 0)"
                ))
            }
            smelt_hir::RegexMatchOp::FullMatch => {
                RustExpr::postfix_on(&regex_text, format!(
                    "{regex_text}.find(&{haystack_text}).ok().flatten().is_some_and(|m| m.start() == 0 && m.end() == {haystack_text}.len())"
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
        self.require_string_operands(&[pattern, haystack, replacement], "regex replace")?;
        let regex_text = self.regexp_operand_expr(pattern)?;
        let haystack_text = self.string_like_operand_expr(haystack, "regex replace")?;
        let replacement_text = self.string_like_operand_expr(replacement, "regex replace")?;
        let force_all = matches!(op, smelt_hir::StringReplaceOp::All);
        Ok(RustExpr::postfix_on(&regex_text, format!(
            "{regex_text}.replace_string(&{haystack_text}, &{replacement_text}, {force_all})"
        )))
    }

    /// Converts a regex replacement callback operation to Rust text.
    ///
    /// `args` names the ECMA-262 replacer arguments the callback declared, in
    /// order, as resolved by the frontend; each is rendered from the
    /// `fancy-regex` `Captures` and from the subject string. Like every regex
    /// site here it compiles with `fancy-regex`, because JavaScript patterns
    /// use look-around (Hono's `buildWildcardRegExp` replaces with
    /// `(?=[/{]|$)`), which the `regex` crate rejects; see
    /// [`Self::regex_is_match_expr`].
    pub(super) fn regex_replace_callback_expr(
        &self,
        op: smelt_hir::StringReplaceOp,
        pattern: &Operand,
        haystack: &Operand,
        callback: &Operand,
        args: &[smelt_hir::RegexReplaceArg],
    ) -> Result<RustExpr, EmitError> {
        self.require_string_operands(&[pattern, haystack], "regex replace callback")?;
        let regex_text = format!(
            "fancy_regex::Regex::new(&{}).expect(\"regex compile failed\")",
            self.operand_expr(pattern)?
        );
        let haystack_text = self.operand_expr(haystack)?;
        let callback_text = self
            .closure_operand_expr(callback)
            .or_else(|_| self.operand_expr(callback))?;
        let arg_texts = self.regex_replacer_argument_texts(args, callback)?;
        // JavaScript `String.prototype.replace(re, fn)` converts the callback's
        // return value to a string (ToString) before substituting it. The Rust
        // `regex` `Replacer` closure must therefore yield a `String`
        // (`AsRef<str>`); a callback typed to return `unknown`/a union/etc.
        // yields `SmeltUnknown`, which is not `AsRef<str>`. When the callback
        // does not already return `String`, erase its result and take ToString of
        // that, so the closure hands `replace` a real string.
        let call_expr = format!("({callback_text})({})", arg_texts.join(", "));
        let replacement_expr = self.regex_replacement_as_string(call_expr, callback)?;
        let replacement = format!("|caps: &fancy_regex::Captures<'_>| {replacement_expr}");
        // The subject string is bound once, before the replace call, because a
        // `position`/`string` argument reads it from INSIDE the replacer closure
        // and re-rendering the operand there would evaluate it a second time.
        let method = match op {
            smelt_hir::StringReplaceOp::First => "replace",
            smelt_hir::StringReplaceOp::All => "replace_all",
        };
        Ok(RustExpr::atom(format!(
            "{{ let smelt_subject: String = {haystack_text}.to_string(); \
             {regex_text}.{method}(&smelt_subject, {replacement}).to_string() }}"
        )))
    }

    /// Renders each declared replacer argument as Rust text at the callback's
    /// own parameter type.
    ///
    /// Every role has one natural Rust rendering and one natural Smelt type:
    ///
    /// | role | rendering | type |
    /// | --- | --- | --- |
    /// | `Matched` | capture group 0's text | `string` |
    /// | `Capture(n)` | group `n`'s text, `None` when it did not participate | `string \| undefined` |
    /// | `Position` | characters before the match in the subject | `number` |
    /// | `Source` | the whole subject string | `string` |
    ///
    /// The callback may nonetheless have annotated a parameter differently (its
    /// contextual type is only a *hint*), so each rendering is routed through
    /// `rendered_at_type` to the declared parameter type. A callback whose
    /// parameter types are not recoverable keeps the natural rendering, which is
    /// what an erased-callback ABI expects anyway.
    ///
    /// `Position` counts CHARACTERS rather than bytes, matching the index
    /// convention the string helpers in this module already use; JavaScript
    /// counts UTF-16 code units, so the two agree outside astral planes.
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
                smelt_hir::RegexReplaceArg::Matched => (
                    RustExpr::chain("caps.get(0).expect(\"regex match missing\").as_str().to_string()"),
                    string_ty,
                ),
                smelt_hir::RegexReplaceArg::Capture(group) => (
                    RustExpr::chain(format!("caps.get({group}).map(|smelt_group| smelt_group.as_str().to_string())")),
                    optional_string_ty,
                ),
                smelt_hir::RegexReplaceArg::Position => (
                    RustExpr::cast("smelt_subject[..caps.get(0).expect(\"regex match missing\").start()]\
                     .chars().count() as f64"),
                    float_ty,
                ),
                smelt_hir::RegexReplaceArg::Source => {
                    (RustExpr::chain("smelt_subject.clone()").ending_in_clone(), string_ty)
                }
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

    /// Coerce a regex replacement callback's return value to a Rust `String`.
    ///
    /// JS ToStrings the callback result before substituting. A callback already
    /// returning `String` needs no wrapping (it is a valid `Replacer`); any
    /// other return type is erased to `SmeltUnknown` and extracted back as a
    /// `String`, applying the same ToString rules JS uses.
    fn regex_replacement_as_string(
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
        // The substitution is ToString of the answer: `undefined` is the text
        // `undefined`, which a string extraction would read as empty.
        let erased = self.erase_rendered(&RustExpr::opaque(call_expr), return_ty)?;
        Ok(format!("smelt_erased_to_js_string(&{erased})"))
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
        self.require_string_operands(&[pattern, haystack], "regex split")?;
        let regex_text = RustExpr::atom(format!(
            "fancy_regex::Regex::new(&{}).expect(\"regex compile failed\")",
            self.operand_expr(pattern)?
        ));
        let haystack_text = self.operand_expr(haystack)?;
        Ok(RustExpr::atom_on(&regex_text, format!(
            "{regex_text}.split(&{haystack_text}).filter_map(Result::ok).map(str::to_owned).collect::<Vec<_>>()"
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
            && self.mir.types.get(*item) == Some(&Type::String)
        {
            return Ok(RustExpr::postfix_on(&pattern_text, format!(
                "{pattern_text}.match_all_strings(&{haystack_text}).map(SmeltList::from)"
            )));
        }
        // Everywhere else `SmeltRegExp::match_string` answers the typed
        // `Option<SmeltMatch>` the frontend declares (the same value `exec`
        // answers), so the present arm needs no projection. As for `exec`,
        // only an erased result slot takes the explicit adapter.
        let call = RustExpr::postfix_on(&pattern_text, format!("{pattern_text}.match_string(&{haystack_text})"));
        if self.optional_inner_is_erased_boundary(dest_ty) {
            return Ok(RustExpr::postfix_on(&call, self.carrier_format(format_args!("{call}.map(SmeltMatch::into_smelt_unknown)"))));
        }
        Ok(call)
    }

    /// Converts JavaScript `RegExp.prototype.exec` to a concrete match result.
    ///
    /// The runtime `exec` returns a typed `Option<SmeltMatch>`. The frontend now
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
        let call = RustExpr::postfix_on(&regex_text, format!("{regex_text}.exec(&{haystack_text})"));
        if self.optional_inner_is_erased_boundary(dest_ty) {
            return Ok(RustExpr::postfix_on(&call, self.carrier_format(format_args!("{call}.map(SmeltMatch::into_smelt_unknown)"))));
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
        Ok(RustExpr::postfix_on(&regex_text, format!("{regex_text}.test(&{haystack_text})")))
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
        let call = RustExpr::postfix_on(&regex_text, format!("{regex_text}.match_all_indices(&{haystack_text})"));
        if self.list_item_is_erased_boundary(dest_ty) {
            return Ok(RustExpr::atom_on(&call, self.carrier_format(format_args!(
                "{call}.into_iter().map(SmeltMatch::into_smelt_unknown).collect::<Vec<_>>()"
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
            Some(Type::String) => Ok(RustExpr::atom(format!("SmeltRegExp::new({text}, String::new())"))),
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. }) => {
                let scrutinee = self.erase_concrete_union_expr(&text, self.operand_ty(operand)?);
                // The prelude `SmeltFromUnknown` impl is the single inverse of
                // the `SmeltRegExp` erasure adapter, so `source`, `flags` AND
                // the writable `lastIndex` all survive the round trip.
                Ok(RustExpr::loose(self.carrier_format(format_args!(
                    "<SmeltRegExp as SmeltFromUnknown>::smelt_from_unknown({scrutinee})"
                ))))
            }
            _ => Ok(RustExpr::atom(format!(
                "SmeltRegExp::new({text}.to_string(), String::new())"
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
                Some(Type::String)
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
    pub(super) fn js_string_coercion_match_expr(
        &self,
        scrutinee_text: &RustExpr,
        absent: AbsentSpelling,
    ) -> RustExpr {
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
            self.carrier_format(format_args!(
                "SmeltUnknown::Object(value) if {is_view}(&SmeltUnknown::Object(value.clone())) => {elements}(&SmeltUnknown::Object(value)).unwrap_or_default().into_iter().map(|element| match element {{ SmeltUnknown::Number(element) => element.to_string(), _ => String::new() }}).collect::<Vec<_>>().join(\",\"),",
                is_view = smelt_stdlib::runtime_symbols::byte_buffer::IS_VIEW,
                elements = smelt_stdlib::runtime_symbols::byte_buffer::ELEMENTS,
            ))
        } else {
            String::new()
        };
        let undefined_text = absent.text();
        RustExpr::atom(self.carrier_format(format_args!(
            "match {scrutinee_text} {{ {params_arm} {url_arm} {byte_view_arm} SmeltUnknown::Null => \"{null_text}\".to_owned(), SmeltUnknown::Undefined => \"{undefined_text}\".to_owned(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Number(value) => smelt_number_to_string(value), {smelt_bigint_arm}SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value.to_string(), SmeltUnknown::Object(value) if value.contains_key(\"{SMELT_REGEXP}\") => smelt_regexp_literal(&value), {error_arm}SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () {{ [native code] }}\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned() }}", smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => value.to_string(),")
        )))
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
        let number_to_string = crate::number_format_prelude::NUMBER_TO_STRING_FN;
        let union_enum_name = union::union_name(union_ty);
        let mut arms = Vec::with_capacity(members.len());
        for (index, member) in members.iter().enumerate() {
            let conversion = match self.mir.types.get(*member) {
                Some(Type::String) => "smelt_union_arm.clone()".to_owned(),
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
            arms.push(format!("{union_enum_name}::M{index}(smelt_union_arm) => {conversion}"));
        }
        Ok(Some(RustExpr::atom(format!(
            "match &{} {{ {} }}",
            self.operand_borrow_expr(operand)?,
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
        match self.mir.types.get(self.operand_ty(operand)?) {
            Some(Type::String) => match operand {
                Operand::Copy(place) => Ok({ let receiver = self.place_expr(place)?; RustExpr::postfix_on(&receiver, format!("{receiver}.clone()")) }.ending_in_clone()),
                Operand::Move(place) => self.place_expr(place),
                Operand::Const(_) => self.operand_expr(operand),
            },
            Some(Type::Bool) => Ok({ let receiver = self.operand_expr(operand)?; RustExpr::postfix_on(&receiver, format!("{receiver}.to_string()")) }),
            // `String(10n)` is `"10"`: a bigint's string form has no `n`.
            Some(Type::BigInt) => Ok(RustExpr::atom(format!("({}).to_string()", self.bigint_ref_expr(operand)?))),
            // A NUMBER goes through JavaScript's own rule, which parts company
            // with Rust's `Display` at both ends of the range (`1e+21`, `1e-7`).
            // `Int` is included because a source `number` that Smelt typed as
            // an integer is still a JavaScript number: `String(x)` must not
            // depend on which of the two Smelt inferred, and a JS number is an
            // `f64` in any case.
            Some(Type::Int | Type::FixedInt(_)) => Ok(RustExpr::chain(format!(
                "{fn_name}({text} as f64)",
                fn_name = crate::number_format_prelude::NUMBER_TO_STRING_FN,
                text = self.operand_expr(operand)?,
            ))),
            Some(Type::Float) => Ok(RustExpr::chain(format!(
                "{fn_name}({text})",
                fn_name = crate::number_format_prelude::NUMBER_TO_STRING_FN,
                text = self.operand_expr(operand)?,
            ))),
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. }) => {
                let text = self
                    .erase_concrete_union_expr(&self.operand_expr(operand)?, self.operand_ty(operand)?);
                Ok(self.js_string_coercion_match_expr(&text, self.absent_spelling()))
            }
            Some(Type::Class { name, .. }) if self.is_regexp_class_symbol(*name)? => {
                Ok(Self::regexp_literal_expr(&self.operand_expr(operand)?))
            }
            // A value the source spelled `null` stringifies as the WORD, not as
            // the empty string: `String(null)` is `"null"` in JavaScript and
            // `str(None)` is `"None"` in Python. `Never` is unreachable code and
            // keeps the empty placeholder.
            Some(Type::None) => Ok(RustExpr::chain(format!("{:?}.to_owned()", self.absent_spelling().null_text())).owned_literal()),
            Some(Type::Never) => Ok(RustExpr::atom("String::new()")),
            // A typed-array VIEW stringifies as its joined elements, because
            // `TypedArray.prototype.toString` is `Array.prototype.toString`;
            // byte STORAGE has no elements and keeps the object tag, which for
            // it is `[object ArrayBuffer]`.
            Some(Type::Class { name, .. })
                if self.stdlib_class_of_symbol(*name)?
                    == Some(smelt_stdlib::StdlibClass::TypedArray) =>
            {
                Ok({ let receiver = self.operand_expr(operand)?; RustExpr::postfix_on(&receiver, format!("{receiver}.to_js_string()")) })
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
                    self.operand_expr(operand)?
                )))
            }
            // `URL` and `URLSearchParams` define their own `toString`: the
            // `href` and the urlencoded query.
            Some(Type::Class { name, .. })
                if self.stdlib_class_of_symbol(*name)? == Some(smelt_stdlib::StdlibClass::Url) =>
            {
                Ok({ let receiver = self.operand_expr(operand)?; RustExpr::postfix_on(&receiver, format!("{receiver}.href()")) })
            }
            Some(Type::Class { name, .. })
                if self.stdlib_class_of_symbol(*name)?
                    == Some(smelt_stdlib::StdlibClass::UrlSearchParams) =>
            {
                Ok({ let receiver = self.operand_expr(operand)?; RustExpr::postfix_on(&receiver, format!("{receiver}.to_text()")) })
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
            Some(Type::Optional(inner)) if self.mir.types.get(*inner) == Some(&Type::String) => {
                Ok({ let receiver = self.operand_expr(operand)?; RustExpr::postfix_on(&receiver, format!("{receiver}.unwrap_or_else(|| {:?}.to_owned())", self.absent_spelling().text())) })
            }
            Some(Type::Optional(inner))
                if self.has_dynamic_shape(*inner) =>
            {
                let text = self.operand_expr(operand)?;
                // A concrete-union `Option` payload stores a tagged enum, so each
                // present value projects back to `SmeltUnknown` before the erased
                // string-coercion match.
                let scrutinee = self.erase_concrete_union_expr(&RustExpr::path("value"), *inner);
                let error_arm = Self::js_error_to_string_arm_text();
                // The absent arm carries no runtime tag, so it takes the
                // language's absent word; the present arms still distinguish
                // `null` from `undefined`, which the tag does know.
                let absent_text = self.absent_spelling().text();
                let null_text = self.absent_spelling().null_text();
                Ok(RustExpr::postfix_on(&text, self.carrier_format(format_args!(
                    "{text}.map_or_else(|| \"{absent_text}\".to_owned(), |value| match {scrutinee} {{ SmeltUnknown::Null => \"{null_text}\".to_owned(), SmeltUnknown::Undefined => \"{absent_text}\".to_owned(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Number(value) => smelt_number_to_string(value), {smelt_bigint_arm}SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value.to_string(), {error_arm}SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () {{ [native code] }}\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned() }})", smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => value.to_string(),")
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
                Ok({ let receiver = self.operand_expr(operand)?; RustExpr::postfix_on(&receiver, format!("{receiver}.map_or_else(|| {:?}.to_owned(), |value| value.to_string())", self.absent_spelling().text())) })
            }
            // An optional whose payload is a structured value keeps the empty
            // placeholder for the PRESENT arm — JavaScript renders a list as
            // its joined elements and an object as `[object Object]`, neither of
            // which this seam models yet — but the absent arm is the word.
            Some(Type::Optional(_)) => Ok({ let receiver = self.operand_expr(operand)?; RustExpr::postfix_on(&receiver, format!("{receiver}.map_or_else(|| {:?}.to_owned(), |_| String::new())", self.absent_spelling().text())) }),
            Some(Type::Tuple(_) | Type::Future(_)) => Ok(RustExpr::atom("String::new()")),
            Some(Type::Function(_) | Type::Generator { .. } | Type::GeneratorResult { .. })
            | None => {
                Ok(RustExpr::atom("String::new()"))
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
            Some(Type::String)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(index)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new(
                "string charAt requires a string receiver and numeric index",
            ));
        }
        // Read by borrow: `chars()` only reads, and the owned clone
        // `operand_expr` gives was a whole-string copy per character read.
        Ok({ let receiver = self.operand_borrow_expr(operand)?; RustExpr::postfix_on(&receiver, format!("{receiver}.chars().nth({} as usize).map(|ch| ch.to_string()).unwrap_or_default()", self.operand_expr(index)?)) })
    }

    /// Converts a string character-code lookup operation to Rust text.
    pub(super) fn string_char_code_at_expr(
        &self,
        operand: &Operand,
        index: &Operand,
    ) -> Result<RustExpr, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::String)
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
        Ok(RustExpr::atom(format!(
            "(|smelt_text: &str, smelt_index: usize| match smelt_text.as_bytes().get(..=smelt_index) {{ Some(smelt_prefix) if smelt_prefix.is_ascii() => f64::from(smelt_prefix[smelt_index]), _ => smelt_text.chars().nth(smelt_index).map_or(f64::NAN, |ch| ch as u32 as f64) }})(&{}, {} as usize)",
            self.operand_borrow_expr(operand)?,
            self.operand_expr(index)?
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
            return Ok({ let receiver = self.operand_expr(haystack)?; RustExpr::postfix_on(&receiver, format!("{receiver}.includes({})", self.operand_expr(needle)?)) });
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
                &{ let receiver = self.operand_expr(haystack)?; RustExpr::postfix_on(&receiver, format!("{receiver}.clone()")) }.ending_in_clone(),
                haystack_ty,
            )?;
            return Ok(RustExpr::postfix_on(&erased_haystack, format!(
                "{erased_haystack}.includes({})",
                self.operand_expr(needle)?
            )));
        }
        if from_index.is_none()
            && self.mir.types.get(needle_ty) == Some(&Type::Unknown)
            && self.mir.types.get(haystack_ty) == Some(&Type::String)
        {
            let erased_haystack = self.erase_rendered(
                &{ let receiver = self.operand_expr(haystack)?; RustExpr::postfix_on(&receiver, format!("{receiver}.clone()")) }.ending_in_clone(),
                self.type_id(Type::String)?,
            )?;
            return Ok(RustExpr::postfix_on(&erased_haystack, format!(
                "{erased_haystack}.includes({})",
                self.operand_expr(needle)?
            )));
        }
        if !matches!(self.mir.types.get(needle_ty), Some(Type::String)) {
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
            let haystack_text = if matches!(self.mir.types.get(haystack_ty), Some(Type::String)) {
                self.operand_expr(haystack)?
            } else {
                self.value_at_type(haystack, self.type_id(Type::String)?)?
            };
            let needle_text = self.operand_expr(needle)?;
            let index_text = self.value_at_type(from_index_operand, self.type_id(Type::Float)?)?;
            return Ok(RustExpr::atom(format!(
                "{{ let smelt_haystack = {haystack_text}; let smelt_from = ({index_text} as i64).max(0) as usize; let smelt_prefix_bytes = smelt_haystack.char_indices().nth(smelt_from).map_or(smelt_haystack.len(), |(byte, _)| byte); smelt_haystack.get(smelt_prefix_bytes..).map_or(false, |suffix| suffix.contains(&{needle_text})) }}"
            )));
        }
        if !matches!(self.mir.types.get(haystack_ty), Some(Type::String)) {
            return Err(EmitError::new(format!(
                "string contains haystack must be a string, got {}",
                Self::type_text_for(self.mir, haystack_ty)
                    .unwrap_or_else(|_error| format!("{haystack_ty:?}"))
            )));
        }
        Ok({ let receiver = self.operand_expr(haystack)?; RustExpr::postfix_on(&receiver, format!("{receiver}.contains(&{})", self.operand_expr(needle)?)) })
    }

    /// Converts a string slice operation to Rust text.
    pub(super) fn string_slice_expr(
        &self,
        operand: &Operand,
        start: Option<&Operand>,
        end: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        self.validate_optional_numeric_index(start, "string slice start index")?;
        self.validate_optional_numeric_index(end, "string slice end index")?;
        let result = if matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::String)
        ) {
            let operand_expr = self.operand_expr(operand)?;
            let len_source = RustExpr::postfix_on(&operand_expr, format!("{operand_expr}.chars().count()"));
            let start_text = self.slice_start_expr(start, &len_source)?;
            let len_expr = self.slice_len_expr(&operand_expr, start, end, SliceLenKind::Chars)?;
            RustExpr::atom_on(&operand_expr, format!(
                "{operand_expr}.chars().skip({start_text}).take({len_expr}).collect::<String>()"
            ))
        } else {
            let operand_expr = self.string_like_operand_expr(operand, "string slice")?;
            let receiver_text = SMELT_STRING;
            let len_source = RustExpr::postfix_on(&receiver_text, format!("{receiver_text}.chars().count()"));
            let start_text = self.slice_start_expr(start, &len_source)?;
            let len_expr = self.slice_len_expr(&RustExpr::opaque(receiver_text), start, end, SliceLenKind::Chars)?;
            RustExpr::atom(format!(
                "{{ let {receiver_text} = {operand_expr}; {receiver_text}.chars().skip({start_text}).take({len_expr}).collect::<String>() }}"
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
            format!("{regex_text}.split_string(&{haystack_text})")
        } else if matches!(
            self.mir.types.get(self.operand_ty(separator)?),
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
        ) {
            let separator_unknown = self
                .erase_concrete_union_expr(&self.operand_expr(separator)?, self.operand_ty(separator)?);
            self.carrier_format(format_args!(
                "{{ let smelt_haystack = {haystack_text}; match {separator_unknown} {{ SmeltUnknown::Object(smelt_object) if smelt_object.contains_key(\"source\") => SmeltRegExp::new(match smelt_object.get(\"source\").cloned() {{ Some(SmeltUnknown::String(source)) => source.to_string(), _ => String::new() }}, match smelt_object.get(\"flags\").cloned() {{ Some(SmeltUnknown::String(flags)) => flags.to_string(), _ => String::new() }}).split_string(&smelt_haystack), smelt_separator_unknown => {{ let smelt_separator = match smelt_separator_unknown {{ SmeltUnknown::Null => String::new(), SmeltUnknown::Undefined => \"undefined\".to_owned(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Number(value) => smelt_number_to_string(value), {smelt_bigint_arm}SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value.to_string(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () {{ [native code] }}\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned() }}; if smelt_separator.is_empty() {{ if smelt_haystack.is_empty() {{ Vec::new() }} else {{ smelt_haystack.chars().map(|ch| ch.to_string()).collect::<Vec<_>>() }} }} else {{ smelt_haystack.split(&smelt_separator).map(str::to_owned).collect::<Vec<_>>() }} }} }} }}", smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => value.to_string(),")
            ))
        } else {
            let separator_text = self.string_like_operand_expr(separator, "string split")?;
            format!(
                "{{ let smelt_haystack = {haystack_text}; let smelt_separator = {separator_text}; if smelt_separator.is_empty() {{ if smelt_haystack.is_empty() {{ Vec::new() }} else {{ smelt_haystack.chars().map(|ch| ch.to_string()).collect::<Vec<_>>() }} }} else {{ smelt_haystack.split(&smelt_separator).map(str::to_owned).collect::<Vec<_>>() }} }}"
            )
        };
        let result = RustExpr::from(if let Some(limit_operand) = limit {
            let limit_text = self.operand_expr(limit_operand)?;
            match self.mir.types.get(self.operand_ty(limit_operand)?) {
                Some(Type::None) => Ok(split_items),
                Some(Type::Int | Type::Float) => Ok(format!(
                    "{{ let mut smelt_parts = {split_items}; let smelt_limit = {limit_text} as f64; if !smelt_limit.is_finite() || smelt_limit == 0.0 {{ smelt_parts.truncate(0); }} else if smelt_limit.is_sign_positive() {{ smelt_parts.truncate(smelt_limit.floor() as usize); }} smelt_parts }}"
                )),
                Some(Type::Optional(inner))
                    if matches!(self.mir.types.get(*inner), Some(Type::Int | Type::Float)) =>
                {
                    Ok(format!(
                        "{{ let mut smelt_parts = {split_items}; if let Some(split_limit) = {limit_text} {{ let smelt_limit = split_limit as f64; if !smelt_limit.is_finite() || smelt_limit == 0.0 {{ smelt_parts.truncate(0); }} else if smelt_limit.is_sign_positive() {{ smelt_parts.truncate(smelt_limit.floor() as usize); }} }} smelt_parts }}"
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
                        self.carrier_format(format_args!("{limit_text}.map(|value| value.into_smelt_unknown())"))
                    } else {
                        limit_text.into_string()
                    };
                    Ok(self.carrier_format(format_args!(
                        "{{ let mut smelt_parts = {split_items}; if let Some(split_limit) = match {limit_scrutinee} {{ Some(SmeltUnknown::Number(value)) => Some(value), Some(SmeltUnknown::Null) | None => None, _ => None }} {{ if !split_limit.is_finite() || split_limit == 0.0 {{ smelt_parts.truncate(0); }} else if split_limit.is_sign_positive() {{ smelt_parts.truncate(split_limit.floor() as usize); }} }} smelt_parts }}"
                    )))
                }
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => {
                    let limit_scrutinee = self
                        .erase_concrete_union_expr(&limit_text, self.operand_ty(limit_operand)?);
                    Ok(self.carrier_format(format_args!(
                        "{{ let mut smelt_parts = {split_items}; if let Some(split_limit) = match {limit_scrutinee} {{ SmeltUnknown::Number(value) => Some(value), SmeltUnknown::Null | SmeltUnknown::Undefined => None, _ => None }} {{ if !split_limit.is_finite() || split_limit == 0.0 {{ smelt_parts.truncate(0); }} else if split_limit.is_sign_positive() {{ smelt_parts.truncate(split_limit.floor() as usize); }} }} smelt_parts }}"
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
            Ok(self.erase_string_array_expr(&result))
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
        let text = RustExpr::atom_on(&haystack_text, format!("{haystack_text}.chars().map(|ch| ch.to_string()).collect::<Vec<_>>()"));
        if let Some(Type::List(item_ty)) = self.mir.types.get(dest_ty)
            && self.mir.types.get(*item_ty) != Some(&Type::String)
        {
            let item_text =
                self.rendered_at_type(&RustExpr::path("value"), self.type_id(Type::String)?, *item_ty, &self.render_scope())?;
            return Ok(RustExpr::atom_on(&text, format!(
                "{text}.into_iter().map(|value| {item_text}).collect::<Vec<_>>()"
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
            return Ok(RustExpr::atom("String::new()"));
        };
        let separator_text = self.string_like_operand_expr(separator, "string join")?;
        let items_text = list_read_expr(&self.operand_expr(items)?);
        if self.mir.types.get(*item_ty) == Some(&Type::String) {
            return Ok(RustExpr::postfix_on(&items_text, format!("{items_text}.join(&{separator_text})")));
        }
        let item_text = match self.mir.types.get(*item_ty) {
            Some(Type::Bool) => "item.to_string()".to_owned(),
            // `[1e21].join(",")` stringifies each item the way `String(item)`
            // does, so the numeric arms take the same helper.
            Some(Type::Int) => format!(
                "{fn_name}(*item as f64)",
                fn_name = crate::number_format_prelude::NUMBER_TO_STRING_FN,
            ),
            Some(Type::Float) => format!(
                "{fn_name}(*item)",
                fn_name = crate::number_format_prelude::NUMBER_TO_STRING_FN,
            ),
            // A union element (`(number | string)[]`) stringifies each value at
            // its runtime arm: erase it to the tagged value and take the same
            // `String(item)` arms an `unknown` element does.
            Some(Type::Unknown | Type::Union(_)) => self.carrier_format(format_args!(
                "match {item} {{ SmeltUnknown::Null | SmeltUnknown::Undefined => String::new(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Number(value) => smelt_number_to_string(value), {smelt_bigint_arm}SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value.to_string(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () {{ [native code] }}\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned() }}",
                smelt_bigint_arm = self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => value.to_string(),"),
                item = if matches!(self.mir.types.get(*item_ty), Some(Type::Union(_))) {
                    format!("({})", self.erase_rendered(&RustExpr::chain("item.clone()").ending_in_clone(), *item_ty)?)
                } else {
                    "item".to_owned()
                },
            )),
            Some(Type::Optional(inner)) => match self.mir.types.get(*inner) {
                Some(Type::Bool) => {
                    "item.as_ref().map_or_else(String::new, |value| value.to_string())".to_owned()
                }
                Some(Type::Int) => format!(
                    "item.as_ref().map_or_else(String::new, |value| {fn_name}(*value as f64))",
                    fn_name = crate::number_format_prelude::NUMBER_TO_STRING_FN,
                ),
                Some(Type::Float) => format!(
                    "item.as_ref().map_or_else(String::new, |value| {fn_name}(*value))",
                    fn_name = crate::number_format_prelude::NUMBER_TO_STRING_FN,
                ),
                Some(Type::String) => {
                    "item.as_ref().map_or_else(String::new, Clone::clone)".to_owned()
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
        Ok(RustExpr::atom_on(&items_text, format!(
            "{items_text}.iter().map(|item| {{ {item_text} }}).collect::<Vec<_>>().join(&{separator_text})"
        )))
    }

}
