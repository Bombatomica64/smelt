//! Strings emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Converts a string case operation to its Rust text representation.
    pub(super) fn string_case_text(
        &self,
        op: smelt_hir::StringCaseOp,
        operand: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let receiver_text = self.string_like_operand_text(operand, "string case")?;
        let method_name = match op {
            smelt_hir::StringCaseOp::Lower => "to_lowercase",
            smelt_hir::StringCaseOp::Upper => "to_uppercase",
        };
        // The case operation always yields a `String`. A `.to_lowercase()` /
        // `.to_uppercase()` call is a postfix expression, so the rendered value
        // is an atom: erasing it through the coercion seam reproduces the
        // historical `SmeltUnknown::String(...)` wrapping while letting the seam
        // (not this caller) own the parenthesization decision.
        let result = RenderedValue::atom(format!("{receiver_text}.{method_name}()"), dest_ty);
        if matches!(
            self.mir.types.get(dest_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) {
            let string_ty = self.type_id(Type::String)?;
            self.erase_value(&RenderedValue::atom(result.into_text(), string_ty))
        } else {
            Ok(result.into_text())
        }
    }

    /// Converts Unicode normalization to Rust text through `unicode-normalization`.
    pub(super) fn string_normalize_text(
        &self,
        form: smelt_hir::StringNormalizeForm,
        operand: &Operand,
    ) -> Result<String, EmitError> {
        let receiver_text = self.string_like_operand_text(operand, "string normalize")?;
        let method_name = match form {
            smelt_hir::StringNormalizeForm::Nfc => "nfc",
            smelt_hir::StringNormalizeForm::Nfd => "nfd",
            smelt_hir::StringNormalizeForm::Nfkc => "nfkc",
            smelt_hir::StringNormalizeForm::Nfkd => "nfkd",
        };
        Ok(format!(
            "unicode_normalization::UnicodeNormalization::{method_name}({receiver_text}.as_str()).collect::<String>()"
        ))
    }

    /// Converts JavaScript `encodeURI(value)` to a call to the runtime
    /// percent-encoding helper (`smelt_encode_uri`), which implements the
    /// ECMA-262 `encodeURI` character-set rules.
    pub(super) fn uri_encode_text(&self, operand: &Operand) -> Result<String, EmitError> {
        let receiver_text = self.string_like_operand_text(operand, "encodeURI")?;
        Ok(format!(
            "{encode_uri}({receiver_text}.as_str())",
            encode_uri = smelt_stdlib::runtime_symbols::strings::ENCODE_URI,
        ))
    }

    /// Converts JavaScript `left.localeCompare(right)` to a call to the runtime
    /// collation helper (`smelt_locale_compare`).
    ///
    /// Both operands are string-like, so they reach the helper as `&str`; the
    /// helper answers the negative / zero / positive `f64` that a comparator
    /// contract expects. See the runtime item for which collation levels are
    /// modeled.
    pub(super) fn string_locale_compare_text(
        &self,
        left: &Operand,
        right: &Operand,
    ) -> Result<String, EmitError> {
        let left_text = self.string_like_operand_text(left, "localeCompare")?;
        let right_text = self.string_like_operand_text(right, "localeCompare")?;
        Ok(format!(
            "{locale_compare}({left_text}.as_str(), {right_text}.as_str())",
            locale_compare = smelt_stdlib::runtime_symbols::strings::LOCALE_COMPARE,
        ))
    }

    /// Converts a string trim operation to Rust text.
    pub(super) fn string_trim_text(
        &self,
        side: smelt_hir::StringTrimSide,
        operand: &Operand,
    ) -> Result<String, EmitError> {
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
        Ok(format!(
            "{}.{method_name}().to_owned()",
            self.operand_text(operand)?
        ))
    }

    /// Converts a string affix operation to Rust text.
    /// Converts a string affix operation to Rust text.
    pub(super) fn string_affix_text(
        &self,
        op: smelt_hir::StringAffixOp,
        haystack: &Operand,
        needle: &Operand,
    ) -> Result<String, EmitError> {
        let haystack_text = self.string_like_operand_text(haystack, "string affix")?;
        let needle_text = self.string_like_operand_text(needle, "string affix")?;
        let method_name = match op {
            smelt_hir::StringAffixOp::StartsWith => "starts_with",
            smelt_hir::StringAffixOp::EndsWith => "ends_with",
        };
        Ok(format!("{haystack_text}.{method_name}(&{needle_text})"))
    }

    /// Converts a string search operation to Rust text.
    /// Converts a string search operation to Rust text.
    pub(super) fn string_search_text(
        &self,
        op: smelt_hir::StringSearchOp,
        haystack: &Operand,
        needle: &Operand,
        from_index: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let (missing, cast) = match self.mir.types.get(dest_ty) {
            Some(Type::Int) => ("-1", "i64"),
            Some(Type::Float) => ("-1.0", "f64"),
            _ => return Err(EmitError::new("string search destination must be numeric")),
        };
        let method_name = match op {
            smelt_hir::StringSearchOp::Find => "find",
            smelt_hir::StringSearchOp::RFind => "rfind",
        };
        let haystack_text = self.string_like_operand_text(haystack, "string search")?;
        let needle_text = self.string_like_operand_text(needle, "string search")?;
        if let Some(from_index_operand) = from_index {
            let index_text = self.value_at_type(from_index_operand, self.type_id(Type::Float)?)?;
            return match op {
                smelt_hir::StringSearchOp::Find => Ok(format!(
                    "{{ let smelt_haystack = {haystack_text}; let smelt_needle = {needle_text}; let smelt_from = ({index_text} as i64).max(0) as usize; let smelt_prefix_bytes = smelt_haystack.char_indices().nth(smelt_from).map_or(smelt_haystack.len(), |(byte, _)| byte); smelt_haystack.get(smelt_prefix_bytes..).and_then(|suffix| suffix.find(&smelt_needle).map(|index| (smelt_prefix_bytes + index) as {cast})).unwrap_or({missing}) }}"
                )),
                smelt_hir::StringSearchOp::RFind => Ok(format!(
                    "{{ let smelt_haystack = {haystack_text}; let smelt_needle = {needle_text}; let smelt_from = ({index_text} as i64).max(0) as usize; let smelt_end_char = smelt_from.saturating_add(smelt_needle.chars().count()); let smelt_end_byte = smelt_haystack.char_indices().nth(smelt_end_char).map_or(smelt_haystack.len(), |(byte, _)| byte); smelt_haystack[..smelt_end_byte].rfind(&smelt_needle).map_or({missing}, |idx| idx as {cast}) }}"
                )),
            };
        }
        Ok(format!(
            "{haystack_text}.{method_name}(&{needle_text}).map_or({missing}, |idx| idx as {cast})"
        ))
    }

    /// Converts a string replacement operation to Rust text.
    /// Converts a string replacement operation to Rust text.
    pub(super) fn string_replace_text(
        &self,
        op: smelt_hir::StringReplaceOp,
        haystack: &Operand,
        pattern: &Operand,
        replacement: &Operand,
    ) -> Result<String, EmitError> {
        let haystack_text = self.string_like_operand_text(haystack, "string replace")?;
        let replacement_text = self.string_like_operand_text(replacement, "string replace")?;
        if matches!(
            self.mir.types.get(self.operand_ty(pattern)?),
            Some(Type::Class { name, .. }) if self.is_regexp_class_symbol(*name)?
        ) {
            let regex_text = self.regexp_operand_text(pattern)?;
            let force_all = matches!(op, smelt_hir::StringReplaceOp::All);
            return Ok(format!(
                "{regex_text}.replace_string(&{haystack_text}, &{replacement_text}, {force_all})"
            ));
        }
        let pattern_text = self.string_like_operand_text(pattern, "string replace")?;
        match op {
            smelt_hir::StringReplaceOp::First => Ok(format!(
                "{haystack_text}.replacen(&{pattern_text}, &{replacement_text}, 1)"
            )),
            smelt_hir::StringReplaceOp::All => Ok(format!(
                "{haystack_text}.replace(&{pattern_text}, &{replacement_text})"
            )),
        }
    }

    /// Converts a string remove-prefix/remove-suffix operation to Rust text.
    /// Converts a string remove-prefix/remove-suffix operation to Rust text.
    pub(super) fn string_remove_affix_text(
        &self,
        op: smelt_hir::StringAffixOp,
        haystack: &Operand,
        affix: &Operand,
    ) -> Result<String, EmitError> {
        let haystack_text = self.string_like_operand_text(haystack, "string remove-affix")?;
        let affix_text = self.string_like_operand_text(affix, "string remove-affix")?;
        let method_name = match op {
            smelt_hir::StringAffixOp::StartsWith => "strip_prefix",
            smelt_hir::StringAffixOp::EndsWith => "strip_suffix",
        };
        Ok(format!(
            "{haystack_text}.{method_name}(&{affix_text}).unwrap_or(&{haystack_text}).to_owned()"
        ))
    }

    /// Converts a string repeat operation to Rust text.
    /// Converts a string repeat operation to Rust text.
    pub(super) fn string_repeat_text(
        &self,
        operand: &Operand,
        count: &Operand,
    ) -> Result<String, EmitError> {
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
        Ok(format!(
            "{}.repeat({} as usize)",
            self.operand_text(operand)?,
            self.operand_text(count)?
        ))
    }

    /// Converts a string padding operation to Rust text.
    ///
    /// The generated code counts Rust `char`s rather than JavaScript UTF-16 code units.
    /// Exact Unicode indexing parity is tracked separately with the other string APIs.
    /// Converts a string padding operation to Rust text.
    ///
    /// The generated code counts Rust `char`s rather than JavaScript UTF-16 code units.
    /// Exact Unicode indexing parity is tracked separately with the other string APIs.
    pub(super) fn string_pad_text(
        &self,
        op: smelt_hir::StringPadOp,
        operand: &Operand,
        target_len: &Operand,
        pad: &Operand,
    ) -> Result<String, EmitError> {
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
        let operand_text = self.operand_text(operand)?;
        let target_len_text = self.operand_text(target_len)?;
        let pad_text = self.operand_text(pad)?;
        let result_text = match op {
            smelt_hir::StringPadOp::Start => "format!(\"{}{}\", padding, value)",
            smelt_hir::StringPadOp::End => "format!(\"{}{}\", value, padding)",
        };
        Ok(format!(
            "{{ let value = &{operand_text}; let target_len = {target_len_text} as usize; let pad = &{pad_text}; let current_len = value.chars().count(); if current_len >= target_len || pad.is_empty() {{ value.to_owned() }} else {{ let needed = target_len - current_len; let padding: String = pad.chars().cycle().take(needed).collect(); {result_text} }} }}"
        ))
    }

    /// Converts a string character predicate operation to Rust text.
    /// Converts a string character predicate operation to Rust text.
    pub(super) fn string_predicate_text(
        &self,
        op: smelt_hir::StringPredicateOp,
        operand: &Operand,
    ) -> Result<String, EmitError> {
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
        let operand_text = self.operand_text(operand)?;
        Ok(format!(
            "!{operand_text}.is_empty() && {operand_text}.chars().all(char::{method_name})"
        ))
    }

    /// Converts a regex boolean match operation to Rust text using the `regex` crate.
    ///
    /// The emitted expression compiles the pattern at the call site so the dependency stays
    /// interchangeable with a future cached-regex helper module.
    /// Converts a regex boolean match operation to Rust text using the `regex` crate.
    ///
    /// The emitted expression compiles the pattern at the call site so the dependency stays
    /// interchangeable with a future cached-regex helper module.
    pub(super) fn regex_is_match_text(
        &self,
        op: smelt_hir::RegexMatchOp,
        pattern: &Operand,
        haystack: &Operand,
    ) -> Result<String, EmitError> {
        let pattern_text = self.string_like_operand_text(pattern, "regex match")?;
        let haystack_text = self.string_like_operand_text(haystack, "regex match")?;
        let regex_text =
            format!("regex::Regex::new(&{pattern_text}).expect(\"regex compile failed\")");
        Ok(match op {
            smelt_hir::RegexMatchOp::Search => {
                format!("{regex_text}.is_match(&{haystack_text})")
            }
            smelt_hir::RegexMatchOp::Match => {
                format!("{regex_text}.find(&{haystack_text}).is_some_and(|m| m.start() == 0)")
            }
            smelt_hir::RegexMatchOp::FullMatch => {
                format!(
                    "{regex_text}.find(&{haystack_text}).is_some_and(|m| m.start() == 0 && m.end() == {haystack_text}.len())"
                )
            }
        })
    }

    /// Converts a regex replacement operation to Rust text using the `regex` crate.
    /// Converts a regex replacement operation to Rust text using the `regex` crate.
    pub(super) fn regex_replace_text(
        &self,
        op: smelt_hir::StringReplaceOp,
        pattern: &Operand,
        haystack: &Operand,
        replacement: &Operand,
    ) -> Result<String, EmitError> {
        self.require_string_operands(&[pattern, haystack, replacement], "regex replace")?;
        let regex_text = self.regexp_operand_text(pattern)?;
        let haystack_text = self.string_like_operand_text(haystack, "regex replace")?;
        let replacement_text = self.string_like_operand_text(replacement, "regex replace")?;
        let force_all = matches!(op, smelt_hir::StringReplaceOp::All);
        Ok(format!(
            "{regex_text}.replace_string(&{haystack_text}, &{replacement_text}, {force_all})"
        ))
    }

    /// Converts a regex replacement callback operation to Rust text.
    ///
    /// `args` names the ECMA-262 replacer arguments the callback declared, in
    /// order, as resolved by the frontend; each is rendered from the `regex`
    /// crate's `Captures` and from the subject string.
    pub(super) fn regex_replace_callback_text(
        &self,
        op: smelt_hir::StringReplaceOp,
        pattern: &Operand,
        haystack: &Operand,
        callback: &Operand,
        args: &[smelt_hir::RegexReplaceArg],
    ) -> Result<String, EmitError> {
        self.require_string_operands(&[pattern, haystack], "regex replace callback")?;
        let regex_text = format!(
            "regex::Regex::new(&{}).expect(\"regex compile failed\")",
            self.operand_text(pattern)?
        );
        let haystack_text = self.operand_text(haystack)?;
        let callback_text = self
            .closure_operand_text(callback)
            .or_else(|_| self.operand_text(callback))?;
        let arg_texts = self.regex_replacer_argument_texts(args, callback)?;
        // JavaScript `String.prototype.replace(re, fn)` converts the callback's
        // return value to a string (ToString) before substituting it. The Rust
        // `regex` `Replacer` closure must therefore yield a `String`
        // (`AsRef<str>`); a callback typed to return `unknown`/a union/etc.
        // yields `SmeltUnknown`, which is not `AsRef<str>`. When the callback
        // does not already return `String`, route its result through the erase +
        // String-extract boundary so the closure hands `replace` a real string.
        let call_expr = format!("({callback_text})({})", arg_texts.join(", "));
        let replacement_expr = self.regex_replacement_as_string(call_expr, callback)?;
        let replacement = format!("|caps: &regex::Captures<'_>| {replacement_expr}");
        // The subject string is bound once, before the replace call, because a
        // `position`/`string` argument reads it from INSIDE the replacer closure
        // and re-rendering the operand there would evaluate it a second time.
        let method = match op {
            smelt_hir::StringReplaceOp::First => "replace",
            smelt_hir::StringReplaceOp::All => "replace_all",
        };
        Ok(format!(
            "{{ let smelt_subject: String = {haystack_text}.to_string(); \
             {regex_text}.{method}(&smelt_subject, {replacement}).to_string() }}"
        ))
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
    /// `value_at_type_text` to the declared parameter type. A callback whose
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
                    "caps.get(0).expect(\"regex match missing\").as_str().to_string()".to_owned(),
                    string_ty,
                ),
                smelt_hir::RegexReplaceArg::Capture(group) => (
                    format!("caps.get({group}).map(|smelt_group| smelt_group.as_str().to_string())"),
                    optional_string_ty,
                ),
                smelt_hir::RegexReplaceArg::Position => (
                    "smelt_subject[..caps.get(0).expect(\"regex match missing\").start()]\
                     .chars().count() as f64"
                        .to_owned(),
                    float_ty,
                ),
                smelt_hir::RegexReplaceArg::Source => {
                    ("smelt_subject.clone()".to_owned(), string_ty)
                }
            };
            let text = match (natural_ty, param_tys.get(index)) {
                (Some(source), Some(&target)) if source != target => {
                    self.value_at_type_text(&natural_text, source, target)?
                }
                _ => natural_text,
            };
            texts.push(text);
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
        let Some(string_ty) = self.existing_type_id(Type::String) else {
            return Ok(call_expr);
        };
        let erased = self.erase_value_text(&call_expr, return_ty)?;
        self.extract_value_text(&erased, string_ty)
    }

    /// Converts regex replacement with an uppercase first-match callback.
    pub(super) fn regex_replace_first_match_uppercase_text(
        &self,
        pattern: &Operand,
        haystack: &Operand,
    ) -> Result<String, EmitError> {
        self.require_string_operands(&[pattern, haystack], "regex uppercase replace")?;
        let regex_text = format!(
            "regex::Regex::new(&{}).expect(\"regex compile failed\")",
            self.operand_text(pattern)?
        );
        let haystack_text = self.operand_text(haystack)?;
        Ok(format!(
            "{regex_text}.replace(&{haystack_text}, |captures: &regex::Captures<'_>| captures.get(0).map_or_else(String::new, |matched| matched.as_str().to_uppercase())).to_string()"
        ))
    }

    /// Converts a regex split operation to Rust text using the `regex` crate.
    /// Converts a regex split operation to Rust text using the `regex` crate.
    pub(super) fn regex_split_text(
        &self,
        pattern: &Operand,
        haystack: &Operand,
    ) -> Result<String, EmitError> {
        self.require_string_operands(&[pattern, haystack], "regex split")?;
        let regex_text = format!(
            "regex::Regex::new(&{}).expect(\"regex compile failed\")",
            self.operand_text(pattern)?
        );
        let haystack_text = self.operand_text(haystack)?;
        Ok(format!(
            "{regex_text}.split(&{haystack_text}).map(str::to_owned).collect::<Vec<_>>()"
        ))
    }

    /// Converts JavaScript `String.prototype.match(RegExp)` to an optional match array.
    pub(super) fn regex_find_text(
        &self,
        pattern: &Operand,
        haystack: &Operand,
    ) -> Result<String, EmitError> {
        let pattern_text = self.regexp_operand_text(pattern)?;
        let haystack_text = self.string_like_operand_text(haystack, "regex find")?;
        // `SmeltRegExp::match_string` returns the match array as a raw
        // `Option<Vec<String>>`; the Smelt type of `String.prototype.match` is
        // `Option<SmeltList<String>>`, so project the present arm through
        // `SmeltList::from` to agree with the declared slot type.
        Ok(format!(
            "{pattern_text}.match_string(&{haystack_text}).map(SmeltList::from)"
        ))
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
    pub(super) fn regex_exec_text(
        &self,
        regex: &Operand,
        haystack: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let regex_text = self.regexp_operand_text(regex)?;
        let haystack_text = self.string_like_operand_text(haystack, "regex exec")?;
        let call = format!("{regex_text}.exec(&{haystack_text})");
        if self.optional_inner_is_erased_boundary(dest_ty) {
            return Ok(format!("{call}.map(SmeltMatch::into_smelt_unknown)"));
        }
        Ok(call)
    }

    /// Converts JavaScript `String.prototype.matchAll` to concrete match results.
    ///
    /// Mirrors [`Self::regex_exec_text`]: `match_all_indices` yields typed
    /// `SmeltMatch` values. The frontend types the result as
    /// `List(SmeltMatch)`, so the concrete values flow to consumer reads
    /// untouched; the explicit `SmeltMatch::into_smelt_unknown` adapter is only
    /// applied when the list element slot is the erased `List(Unknown)`
    /// boundary.
    pub(super) fn regex_match_all_text(
        &self,
        regex: &Operand,
        haystack: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let regex_text = self.regexp_operand_text(regex)?;
        let haystack_text = self.string_like_operand_text(haystack, "regex matchAll")?;
        let call = format!("{regex_text}.match_all_indices(&{haystack_text})");
        if self.list_item_is_erased_boundary(dest_ty) {
            return Ok(format!(
                "{call}.into_iter().map(SmeltMatch::into_smelt_unknown).collect::<Vec<_>>()"
            ));
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
    fn regexp_operand_text(&self, operand: &Operand) -> Result<String, EmitError> {
        let text = self.operand_text(operand)?;
        match self.mir.types.get(self.operand_ty(operand)?) {
            Some(Type::Class { name, .. }) if self.is_regexp_class_symbol(*name)? => Ok(text),
            Some(Type::String) => Ok(format!("SmeltRegExp::new({text}, String::new())")),
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. }) => {
                let scrutinee = self.erase_concrete_union_text(&text, self.operand_ty(operand)?);
                // The prelude `SmeltFromUnknown` impl is the single inverse of
                // the `SmeltRegExp` erasure adapter, so `source`, `flags` AND
                // the writable `lastIndex` all survive the round trip.
                Ok(format!(
                    "<SmeltRegExp as SmeltFromUnknown>::smelt_from_unknown({scrutinee})"
                ))
            }
            _ => Ok(format!(
                "SmeltRegExp::new({text}.to_string(), String::new())"
            )),
        }
    }

    /// Converts a timestamp in milliseconds to an RFC 3339 timestamp string.
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
            ) && self.string_like_operand_text(operand, context).is_err()
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
    pub(super) fn js_string_coercion_match_text(scrutinee_text: &str) -> String {
        let error_arm = Self::js_error_to_string_arm_text();
        format!(
            "match {scrutinee_text} {{ SmeltUnknown::Null => String::new(), SmeltUnknown::Undefined => \"undefined\".to_owned(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value.to_string(), SmeltUnknown::Object(value) if value.contains_key(\"__smelt_regexp\") => smelt_regexp_literal(&value), {error_arm}SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () {{ [native code] }}\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned() }}"
        )
    }

    /// Renders an operand as a Rust `String` expression for string methods.
    ///
    /// Statically known strings are emitted directly. Unknown values use the
    /// same JavaScript-style primitive coercion as string joining; structured
    /// unknown values use the platform object placeholder.
    pub(super) fn string_like_operand_text(
        &self,
        operand: &Operand,
        _context: &str,
    ) -> Result<String, EmitError> {
        match self.mir.types.get(self.operand_ty(operand)?) {
            Some(Type::String) => match operand {
                Operand::Copy(place) => Ok(format!("{}.clone()", self.place_text(place)?)),
                Operand::Move(place) => self.place_text(place),
                Operand::Const(_) => self.operand_text(operand),
            },
            Some(Type::Bool | Type::Int | Type::Float) => {
                Ok(format!("{}.to_string()", self.operand_text(operand)?))
            }
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. }) => {
                let text = self
                    .erase_concrete_union_text(&self.operand_text(operand)?, self.operand_ty(operand)?);
                Ok(Self::js_string_coercion_match_text(&text))
            }
            Some(Type::Class { name, .. }) if self.is_regexp_class_symbol(*name)? => {
                Ok(Self::regexp_literal_text(&self.operand_text(operand)?))
            }
            Some(Type::None | Type::Never) => Ok("String::new()".to_owned()),
            Some(Type::List(_) | Type::Set(_) | Type::Dict(_, _) | Type::Class { .. }) => {
                Ok("\"[object Object]\".to_owned()".to_owned())
            }
            Some(Type::JsMap(_, _)) => Ok("\"[object Map]\".to_owned()".to_owned()),
            Some(Type::Optional(inner)) if self.mir.types.get(*inner) == Some(&Type::String) => Ok(
                format!("{}.unwrap_or_default()", self.operand_text(operand)?),
            ),
            Some(Type::Optional(inner))
                if matches!(
                    self.mir.types.get(*inner),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(*inner) =>
            {
                let text = self.operand_text(operand)?;
                // A concrete-union `Option` payload stores a tagged enum, so each
                // present value projects back to `SmeltUnknown` before the erased
                // string-coercion match.
                let scrutinee = self.erase_concrete_union_text("value", *inner);
                let error_arm = Self::js_error_to_string_arm_text();
                Ok(format!(
                    "{text}.map_or_else(String::new, |value| match {scrutinee} {{ SmeltUnknown::Null => String::new(), SmeltUnknown::Undefined => \"undefined\".to_owned(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value.to_string(), {error_arm}SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () {{ [native code] }}\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned() }})"
                ))
            }
            Some(Type::Tuple(_) | Type::Optional(_) | Type::Future(_)) => {
                Ok("String::new()".to_owned())
            }
            Some(Type::Function(_) | Type::Generator { .. } | Type::GeneratorResult { .. })
            | None => {
                Ok("String::new()".to_owned())
            }
        }
    }

    /// Converts a string character lookup operation to Rust text.
    /// Converts a string character lookup operation to Rust text.
    pub(super) fn string_char_at_text(
        &self,
        operand: &Operand,
        index: &Operand,
    ) -> Result<String, EmitError> {
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
        Ok(format!(
            "{}.chars().nth({} as usize).map(|ch| ch.to_string()).unwrap_or_default()",
            self.operand_text(operand)?,
            self.operand_text(index)?
        ))
    }

    /// Converts a string character-code lookup operation to Rust text.
    /// Converts a string character-code lookup operation to Rust text.
    pub(super) fn string_char_code_at_text(
        &self,
        operand: &Operand,
        index: &Operand,
    ) -> Result<String, EmitError> {
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
        Ok(format!(
            "{}.chars().nth({} as usize).map_or(f64::NAN, |ch| ch as u32 as f64)",
            self.operand_text(operand)?,
            self.operand_text(index)?
        ))
    }

    /// Converts a string containment operation to Rust text.
    ///
    /// Mirrors `String.prototype.includes(needle, position?)`. When `from_index`
    /// is present the search starts at that position: JavaScript truncates the
    /// position toward zero and clamps it into `[0, length]`, so the emitted code
    /// takes the character prefix at that position (matching the char-based
    /// convention shared with `string_search_text` and the other string helpers)
    /// and searches the remaining suffix. A `position` past the end can only match
    /// the empty needle, which `str::contains` handles for free.
    ///
    /// The erased-value fast paths only apply when no `position` is given; the
    /// frontend coerces an erased haystack to a concrete string before attaching
    /// a `from_index`, so the positional form always sees `Type::String` operands.
    pub(super) fn string_contains_text(
        &self,
        haystack: &Operand,
        needle: &Operand,
        from_index: Option<&Operand>,
    ) -> Result<String, EmitError> {
        let haystack_ty = self.operand_ty(haystack)?;
        let needle_ty = self.operand_ty(needle)?;
        if from_index.is_none() && self.mir.types.get(haystack_ty) == Some(&Type::Unknown) {
            return Ok(format!(
                "{}.includes({})",
                self.operand_text(haystack)?,
                self.operand_text(needle)?
            ));
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
            let erased_haystack = self.erase_value_text(
                &format!("{}.clone()", self.operand_text(haystack)?),
                haystack_ty,
            )?;
            return Ok(format!(
                "{erased_haystack}.includes({})",
                self.operand_text(needle)?
            ));
        }
        if from_index.is_none()
            && self.mir.types.get(needle_ty) == Some(&Type::Unknown)
            && self.mir.types.get(haystack_ty) == Some(&Type::String)
        {
            let erased_haystack = self.erase_value_text(
                &format!("{}.clone()", self.operand_text(haystack)?),
                self.type_id(Type::String)?,
            )?;
            return Ok(format!(
                "{erased_haystack}.includes({})",
                self.operand_text(needle)?
            ));
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
                self.operand_text(haystack)?
            } else {
                self.value_at_type(haystack, self.type_id(Type::String)?)?
            };
            let needle_text = self.operand_text(needle)?;
            let index_text = self.value_at_type(from_index_operand, self.type_id(Type::Float)?)?;
            return Ok(format!(
                "{{ let smelt_haystack = {haystack_text}; let smelt_from = ({index_text} as i64).max(0) as usize; let smelt_prefix_bytes = smelt_haystack.char_indices().nth(smelt_from).map_or(smelt_haystack.len(), |(byte, _)| byte); smelt_haystack.get(smelt_prefix_bytes..).map_or(false, |suffix| suffix.contains(&{needle_text})) }}"
            ));
        }
        if !matches!(self.mir.types.get(haystack_ty), Some(Type::String)) {
            return Err(EmitError::new(format!(
                "string contains haystack must be a string, got {}",
                Self::type_text_for(self.mir, haystack_ty)
                    .unwrap_or_else(|_error| format!("{haystack_ty:?}"))
            )));
        }
        Ok(format!(
            "{}.contains(&{})",
            self.operand_text(haystack)?,
            self.operand_text(needle)?
        ))
    }

    /// Converts a string slice operation to Rust text.
    /// Converts a string slice operation to Rust text.
    pub(super) fn string_slice_text(
        &self,
        operand: &Operand,
        start: Option<&Operand>,
        end: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        self.validate_optional_numeric_index(start, "string slice start index")?;
        self.validate_optional_numeric_index(end, "string slice end index")?;
        let result = if matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::String)
        ) {
            let operand_text = self.operand_text(operand)?;
            let len_source = format!("{operand_text}.chars().count()");
            let start_text = self.slice_start_text(start, &len_source)?;
            let len_text = self.slice_len_text(&operand_text, start, end, SliceLenKind::Chars)?;
            format!(
                "{operand_text}.chars().skip({start_text}).take({len_text}).collect::<String>()"
            )
        } else {
            let operand_text = self.string_like_operand_text(operand, "string slice")?;
            let receiver_text = "__smelt_string";
            let len_source = format!("{receiver_text}.chars().count()");
            let start_text = self.slice_start_text(start, &len_source)?;
            let len_text = self.slice_len_text(receiver_text, start, end, SliceLenKind::Chars)?;
            format!(
                "{{ let {receiver_text} = {operand_text}; {receiver_text}.chars().skip({start_text}).take({len_text}).collect::<String>() }}"
            )
        };
        if matches!(
            self.mir.types.get(dest_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) {
            self.erase_value_text(&result, self.type_id(Type::String)?)
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
    pub(super) fn string_split_text(
        &self,
        haystack: &Operand,
        separator: &Operand,
        limit: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let haystack_text = self.string_like_operand_text(haystack, "string split")?;
        let split_items = if matches!(
            self.mir.types.get(self.operand_ty(separator)?),
            Some(Type::Class { name, .. }) if self.is_regexp_class_symbol(*name)?
        ) {
            let regex_text = self.regexp_operand_text(separator)?;
            format!("{regex_text}.split_string(&{haystack_text})")
        } else if matches!(
            self.mir.types.get(self.operand_ty(separator)?),
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
        ) {
            let separator_unknown = self
                .erase_concrete_union_text(&self.operand_text(separator)?, self.operand_ty(separator)?);
            format!(
                "{{ let smelt_haystack = {haystack_text}; match {separator_unknown} {{ SmeltUnknown::Object(smelt_object) if smelt_object.contains_key(\"source\") => SmeltRegExp::new(match smelt_object.get(\"source\").cloned() {{ Some(SmeltUnknown::String(source)) => source.to_string(), _ => String::new() }}, match smelt_object.get(\"flags\").cloned() {{ Some(SmeltUnknown::String(flags)) => flags.to_string(), _ => String::new() }}).split_string(&smelt_haystack), smelt_separator_unknown => {{ let smelt_separator = match smelt_separator_unknown {{ SmeltUnknown::Null => String::new(), SmeltUnknown::Undefined => \"undefined\".to_owned(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value.to_string(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () {{ [native code] }}\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned() }}; if smelt_separator.is_empty() {{ if smelt_haystack.is_empty() {{ Vec::new() }} else {{ smelt_haystack.chars().map(|ch| ch.to_string()).collect::<Vec<_>>() }} }} else {{ smelt_haystack.split(&smelt_separator).map(str::to_owned).collect::<Vec<_>>() }} }} }} }}"
            )
        } else {
            let separator_text = self.string_like_operand_text(separator, "string split")?;
            format!(
                "{{ let smelt_haystack = {haystack_text}; let smelt_separator = {separator_text}; if smelt_separator.is_empty() {{ if smelt_haystack.is_empty() {{ Vec::new() }} else {{ smelt_haystack.chars().map(|ch| ch.to_string()).collect::<Vec<_>>() }} }} else {{ smelt_haystack.split(&smelt_separator).map(str::to_owned).collect::<Vec<_>>() }} }}"
            )
        };
        let result = if let Some(limit_operand) = limit {
            let limit_text = self.operand_text(limit_operand)?;
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
                        format!("{limit_text}.map(|value| value.into_smelt_unknown())")
                    } else {
                        limit_text
                    };
                    Ok(format!(
                        "{{ let mut smelt_parts = {split_items}; if let Some(split_limit) = match {limit_scrutinee} {{ Some(SmeltUnknown::Number(value)) => Some(value), Some(SmeltUnknown::Null) | None => None, _ => None }} {{ if !split_limit.is_finite() || split_limit == 0.0 {{ smelt_parts.truncate(0); }} else if split_limit.is_sign_positive() {{ smelt_parts.truncate(split_limit.floor() as usize); }} }} smelt_parts }}"
                    ))
                }
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => {
                    let limit_scrutinee = self
                        .erase_concrete_union_text(&limit_text, self.operand_ty(limit_operand)?);
                    Ok(format!(
                        "{{ let mut smelt_parts = {split_items}; if let Some(split_limit) = match {limit_scrutinee} {{ SmeltUnknown::Number(value) => Some(value), SmeltUnknown::Null | SmeltUnknown::Undefined => None, _ => None }} {{ if !split_limit.is_finite() || split_limit == 0.0 {{ smelt_parts.truncate(0); }} else if split_limit.is_sign_positive() {{ smelt_parts.truncate(split_limit.floor() as usize); }} }} smelt_parts }}"
                    ))
                }
                _ => Err(EmitError::new(
                    "string split limit must be numeric or optional numeric",
                )),
            }
        } else {
            Ok(split_items)
        }?;
        if matches!(
            self.mir.types.get(dest_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) {
            Ok(self.erase_string_array_text(&result))
        } else {
            Ok(result)
        }
    }

    /// Converts a string into a list of one-character strings.
    pub(super) fn string_chars_text(
        &self,
        haystack: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let haystack_text = self.string_like_operand_text(haystack, "string chars")?;
        let text = format!("{haystack_text}.chars().map(|ch| ch.to_string()).collect::<Vec<_>>()");
        if let Some(Type::List(item_ty)) = self.mir.types.get(dest_ty)
            && self.mir.types.get(*item_ty) != Some(&Type::String)
        {
            let item_text =
                self.value_at_type_text("value", self.type_id(Type::String)?, *item_ty)?;
            return Ok(format!(
                "{text}.into_iter().map(|value| {item_text}).collect::<Vec<_>>()"
            ));
        }
        Ok(text)
    }

    /// Converts a string join operation to Rust text.
    /// Converts a string join operation to Rust text.
    pub(super) fn string_join_text(
        &self,
        items: &Operand,
        separator: &Operand,
    ) -> Result<String, EmitError> {
        let items_ty = self.operand_ty(items)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(items_ty) else {
            return Ok("String::new()".to_owned());
        };
        let separator_text = self.string_like_operand_text(separator, "string join")?;
        let items_text = list_read_text(&self.operand_text(items)?);
        if self.mir.types.get(*item_ty) == Some(&Type::String) {
            return Ok(format!("{items_text}.join(&{separator_text})"));
        }
        let item_text = match self.mir.types.get(*item_ty) {
            Some(Type::Bool | Type::Int | Type::Float) => "item.to_string()".to_owned(),
            Some(Type::Unknown) => {
                "match item { SmeltUnknown::Null | SmeltUnknown::Undefined => String::new(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value.to_string(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () { [native code] }\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned() }".to_owned()
            }
            Some(Type::Optional(inner)) => match self.mir.types.get(*inner) {
                Some(Type::Bool | Type::Int | Type::Float) => {
                    "item.as_ref().map_or_else(String::new, |value| value.to_string())".to_owned()
                }
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
        Ok(format!(
            "{items_text}.iter().map(|item| {{ {item_text} }}).collect::<Vec<_>>().join(&{separator_text})"
        ))
    }

    // JSON and IO string helpers continue in `strings_io.rs`.
}
