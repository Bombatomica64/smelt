//! Strings emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Converts a string case operation to its Rust text representation.
    pub(super) fn string_case_text(
        &self,
        op: smelt_hir::StringCaseOp,
        operand: &Operand,
    ) -> Result<String, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::String)
        ) {
            return Err(EmitError::new("string case operand must be a string"));
        }
        let receiver_text = self.len_operand_text(operand)?;
        let method_name = match op {
            smelt_hir::StringCaseOp::Lower => "to_lowercase",
            smelt_hir::StringCaseOp::Upper => "to_uppercase",
        };
        Ok(format!("{receiver_text}.{method_name}()"))
    }

    /// Converts a numeric absolute-value operation to Rust text.
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
        if !matches!(
            self.mir.types.get(self.operand_ty(haystack)?),
            Some(Type::String)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(needle)?),
            Some(Type::String)
        ) {
            return Err(EmitError::new("string affix operands must be strings"));
        }
        let method_name = match op {
            smelt_hir::StringAffixOp::StartsWith => "starts_with",
            smelt_hir::StringAffixOp::EndsWith => "ends_with",
        };
        Ok(format!(
            "{}.{method_name}(&{})",
            self.operand_text(haystack)?,
            self.operand_text(needle)?
        ))
    }

    /// Converts a string search operation to Rust text.
    /// Converts a string search operation to Rust text.
    pub(super) fn string_search_text(
        &self,
        op: smelt_hir::StringSearchOp,
        haystack: &Operand,
        needle: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(haystack)?),
            Some(Type::String)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(needle)?),
            Some(Type::String)
        ) {
            return Err(EmitError::new("string search operands must be strings"));
        }
        let (missing, cast) = match self.mir.types.get(dest_ty) {
            Some(Type::Int) => ("-1", "i64"),
            Some(Type::Float) => ("-1.0", "f64"),
            _ => return Err(EmitError::new("string search destination must be numeric")),
        };
        let method_name = match op {
            smelt_hir::StringSearchOp::Find => "find",
            smelt_hir::StringSearchOp::RFind => "rfind",
        };
        Ok(format!(
            "{}.{method_name}(&{}).map_or({missing}, |idx| idx as {cast})",
            self.operand_text(haystack)?,
            self.operand_text(needle)?
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
        if !matches!(
            self.mir.types.get(self.operand_ty(haystack)?),
            Some(Type::String)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(pattern)?),
            Some(Type::String)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(replacement)?),
            Some(Type::String)
        ) {
            return Err(EmitError::new("string replace operands must be strings"));
        }
        let haystack_text = self.operand_text(haystack)?;
        let pattern_text = self.operand_text(pattern)?;
        let replacement_text = self.operand_text(replacement)?;
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
        if !matches!(
            self.mir.types.get(self.operand_ty(haystack)?),
            Some(Type::String)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(affix)?),
            Some(Type::String)
        ) {
            return Err(EmitError::new(
                "string remove-affix operands must be strings",
            ));
        }
        let haystack_text = self.operand_text(haystack)?;
        let affix_text = self.operand_text(affix)?;
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
        if !matches!(
            self.mir.types.get(self.operand_ty(pattern)?),
            Some(Type::String)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(haystack)?),
            Some(Type::String)
        ) {
            return Err(EmitError::new(
                "regex match requires string pattern and haystack operands",
            ));
        }
        let pattern_text = self.operand_text(pattern)?;
        let haystack_text = self.operand_text(haystack)?;
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
        let regex_text = format!(
            "regex::Regex::new(&{}).expect(\"regex compile failed\")",
            self.operand_text(pattern)?
        );
        let haystack_text = self.operand_text(haystack)?;
        let replacement_text = self.operand_text(replacement)?;
        Ok(match op {
            smelt_hir::StringReplaceOp::First => {
                format!("{regex_text}.replace(&{haystack_text}, &{replacement_text}).to_string()")
            }
            smelt_hir::StringReplaceOp::All => {
                format!(
                    "{regex_text}.replace_all(&{haystack_text}, &{replacement_text}).to_string()"
                )
            }
        })
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
        self.require_string_operands(&[pattern, haystack], "regex find")?;
        let regex_text = format!(
            "regex::Regex::new(&{}).expect(\"regex compile failed\")",
            self.operand_text(pattern)?
        );
        let haystack_text = self.operand_text(haystack)?;
        Ok(format!(
            "{regex_text}.find(&{haystack_text}).map(|m| vec![m.as_str().to_owned()])"
        ))
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
            ) {
                return Err(EmitError::new(format!(
                    "{context} requires string operands"
                )));
            }
        }
        Ok(())
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
    /// Converts a string containment operation to Rust text.
    pub(super) fn string_contains_text(
        &self,
        haystack: &Operand,
        needle: &Operand,
    ) -> Result<String, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(haystack)?),
            Some(Type::String)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(needle)?),
            Some(Type::String)
        ) {
            return Err(EmitError::new("string contains operands must be strings"));
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
    ) -> Result<String, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::String)
        ) {
            return Err(EmitError::new("string slice receiver must be a string"));
        }
        self.validate_optional_numeric_index(start, "string slice start index")?;
        self.validate_optional_numeric_index(end, "string slice end index")?;
        let operand_text = self.operand_text(operand)?;
        let len_source = format!("{operand_text}.chars().count()");
        let start_text = self.slice_start_text(start, &len_source)?;
        let len_text = self.slice_len_text(&operand_text, start, end, SliceLenKind::Chars)?;
        Ok(format!(
            "{operand_text}.chars().skip({start_text}).take({len_text}).collect::<String>()"
        ))
    }

    /// Converts a list containment operation to Rust text.
    /// Converts a string split operation to Rust text.
    pub(super) fn string_split_text(
        &self,
        haystack: &Operand,
        separator: &Operand,
        limit: Option<&Operand>,
    ) -> Result<String, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(haystack)?),
            Some(Type::String)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(separator)?),
            Some(Type::String)
        ) {
            return Err(EmitError::new("string split operands must be strings"));
        }
        let haystack_text = self.operand_text(haystack)?;
        let separator_text = self.operand_text(separator)?;
        let base = format!("{haystack_text}.split(&{separator_text}).map(str::to_owned)");
        let Some(limit_operand) = limit else {
            return Ok(format!("{base}.collect::<Vec<_>>()"));
        };
        let limit_text = self.operand_text(limit_operand)?;
        match self.mir.types.get(self.operand_ty(limit_operand)?) {
            Some(Type::None) => Ok(format!("{base}.collect::<Vec<_>>()")),
            Some(Type::Int | Type::Float) => Ok(format!(
                "{base}.take(({limit_text} as f64).max(0.0) as usize).collect::<Vec<_>>()"
            )),
            Some(Type::Optional(inner))
                if matches!(self.mir.types.get(*inner), Some(Type::Int | Type::Float)) =>
            {
                Ok(format!(
                    "if let Some(split_limit) = {limit_text} {{ {base}.take((split_limit as f64).max(0.0) as usize).collect::<Vec<_>>() }} else {{ {base}.collect::<Vec<_>>() }}"
                ))
            }
            _ => Err(EmitError::new(
                "string split limit must be numeric or optional numeric",
            )),
        }
    }

    /// Converts a string into a list of one-character strings.
    pub(super) fn string_chars_text(&self, haystack: &Operand) -> Result<String, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(haystack)?),
            Some(Type::String)
        ) {
            return Err(EmitError::new("string chars operand must be a string"));
        }
        Ok(format!(
            "{}.chars().map(|ch| ch.to_string()).collect::<Vec<_>>()",
            self.operand_text(haystack)?
        ))
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
            return Err(EmitError::new("string join items must be a list"));
        };
        if self.mir.types.get(self.operand_ty(separator)?) != Some(&Type::String) {
            return Err(EmitError::new("string join requires a string separator"));
        }
        let items_text = self.operand_text(items)?;
        let separator_text = self.operand_text(separator)?;
        if self.mir.types.get(*item_ty) == Some(&Type::String) {
            return Ok(format!("{items_text}.join(&{separator_text})"));
        }
        let item_text = match self.mir.types.get(*item_ty) {
            Some(Type::Bool | Type::Int | Type::Float) => "item.to_string()".to_owned(),
            Some(Type::Unknown) => {
                "match item { SmeltUnknown::Null => String::new(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::String(value) => value.clone(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned() }".to_owned()
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
