//! The unit a string index counts, per source language.
//!
//! A JavaScript string index counts UTF-16 code units; a Python `str` index
//! counts code points. Generated strings are UTF-8 `String` in both, so every
//! emit site that measures a string, reads at an index, slices, or turns a
//! byte offset into an index asks this module for its text. TypeScript bodies
//! go through the `smelt_utf16_*` runtime helpers (`runtime/string_units.rs`,
//! demanded by `RuntimeDemand::scan_string_units`); Python bodies use `chars()`.
//!
//! This is the emitter's ONE seam for string indices: no other emitter module
//! spells a `smelt_utf16_*` helper or a `chars()` index walk, so a change of
//! string representation (UTF-16 storage) replaces this module and the runtime
//! file, not the emit sites.

use smelt_hir::{Language, Type};

use super::FunctionEmitter;
use crate::rust::RustExpr;

impl FunctionEmitter<'_> {
    /// The string type of this body's source language: [`Type::PyStr`] in a
    /// Python body, [`Type::String`] (a JavaScript string) otherwise. A string
    /// the body makes itself (a literal, a concatenation) has this type.
    pub(super) fn text_type(&self) -> Type {
        if self.function.language == Language::Python { Type::PyStr } else { Type::String }
    }

    /// The Rust type name of this body's strings: `JsString` (UTF-16) in a
    /// TypeScript body, `String` in a Python one.
    pub(super) fn text_type_name(&self) -> &'static str {
        if self.indexes_utf16() { "JsString" } else { "String" }
    }

    /// `string_expr`, a Rust expression of type `String`, as a value of this
    /// body's string type: wrapped in `JsString::from` in a TypeScript body
    /// (an O(n) transcode), unchanged in a Python one.
    pub(super) fn text_from_string(&self, string_expr: &str) -> String {
        if self.indexes_utf16() { format!("JsString::from({string_expr})") } else { string_expr.to_owned() }
    }

    /// `text`, a value of this body's string type, as UTF-8 text for a Rust
    /// API that takes `&str`: a JavaScript string's lossy UTF-8
    /// (`to_string_lossy`), a Python `String` unchanged.
    pub(super) fn host_utf8_text(&self, text: RustExpr) -> RustExpr {
        if self.indexes_utf16() { RustExpr::postfix_on(&text, format!("{text}.to_string_lossy()")) } else { text }
    }

    /// The constant `value` as a value of this body's string type.
    pub(super) fn text_literal(&self, value: &str) -> RustExpr {
        if self.indexes_utf16() { RustExpr::js_string_literal(value) } else { RustExpr::string_literal(value) }
    }

    /// The empty string of this body's string type.
    pub(super) fn text_empty(&self) -> &'static str {
        if self.indexes_utf16() { "JsString::new()" } else { "String::new()" }
    }

    /// Whether this body's string indices count UTF-16 code units.
    pub(super) fn indexes_utf16(&self) -> bool {
        self.function.language == Language::TypeScript
    }

    /// The `usize` length of the `&str`-coercible `receiver`, in index units.
    pub(super) fn string_len_text(&self, receiver: &str) -> String {
        if self.indexes_utf16() {
            format!("{receiver}.len()")
        } else {
            format!("{receiver}.chars().count()")
        }
    }

    /// [`Self::string_len_text`] as a rendered expression.
    pub(super) fn string_len_expr(&self, receiver: &RustExpr) -> RustExpr {
        if self.indexes_utf16() {
            RustExpr::postfix_on(receiver, format!("{receiver}.len()"))
        } else {
            RustExpr::postfix_on(receiver, format!("{receiver}.chars().count()"))
        }
    }

    /// The `Option<String>` holding the one-unit string at the `usize` index
    /// `index` of `receiver`.
    pub(super) fn string_char_at_text(&self, receiver: &str, index: &str) -> String {
        if self.indexes_utf16() {
            format!("{receiver}.char_at({index})")
        } else {
            format!("{receiver}.chars().nth({index}).map(|ch| ch.to_string())")
        }
    }

    /// The `String` of `count` units of `receiver` starting at unit `start`
    /// (both `usize`, clamped to the string).
    pub(super) fn string_slice_text(&self, receiver: &str, start: &str, count: &str) -> String {
        if self.indexes_utf16() {
            format!("{{ let smelt_start = {start}; {receiver}.slice(smelt_start, smelt_start.saturating_add({count})) }}")
        } else {
            format!("{receiver}.chars().skip({start}).take({count}).collect::<String>()")
        }
    }

    /// The byte offset of index `index` (`usize`) in `receiver`, clamped to its
    /// end. A JavaScript index inside a surrogate pair rounds up past the pair
    /// with `round_up` (a search starting there cannot match the pair) and
    /// down to it otherwise (a match ending there cannot include it).
    pub(super) fn string_byte_of_text(&self, receiver: &str, index: &str, round_up: bool) -> String {
        if self.indexes_utf16() {
            let inside_pair = if round_up { "smelt_byte + 4" } else { "smelt_byte" };
            format!("smelt_utf16_byte_of(&{receiver}, {index}).unwrap_or_else(|smelt_byte| {inside_pair})")
        } else {
            format!("{receiver}.char_indices().nth({index}).map_or({receiver}.len(), |(byte, _)| byte)")
        }
    }

    /// A `(byte, index)` pair for index `index` (`usize`) of `receiver`: the
    /// byte offset a search from `index` starts at (past a surrogate pair the
    /// index falls inside, clamped to the end) and the index of that byte, so
    /// a match found after it is that index plus the units up to the match.
    pub(super) fn string_seek_text(&self, receiver: &str, index: &str) -> String {
        if self.indexes_utf16() {
            format!("match smelt_utf16_byte_of(&{receiver}, {index}) {{ Ok(smelt_byte) if smelt_byte < {receiver}.len() => (smelt_byte, {index}), Ok(smelt_byte) => (smelt_byte, smelt_utf16_len(&{receiver})), Err(smelt_byte) => (smelt_byte + 4, {index} + 1) }}")
        } else {
            format!("match {receiver}.char_indices().nth({index}) {{ Some((smelt_byte, _)) => (smelt_byte, {index}), None => ({receiver}.len(), {receiver}.chars().count()) }}")
        }
    }

    /// The index (`usize`) of the char-boundary byte offset `byte` of `receiver`.
    pub(super) fn string_index_of_byte_text(&self, receiver: &str, byte: &str) -> String {
        if self.indexes_utf16() {
            format!("smelt_utf16_index_of_byte(&{receiver}, {byte})")
        } else {
            format!("{receiver}[..{byte}].chars().count()")
        }
    }

    /// The `String` of `needed` units cut from repetitions of the `&str` `pad`
    /// (a pad's fill, `padStart`/`padEnd`).
    pub(super) fn string_padding_text(&self, pad: &str, needed: &str) -> String {
        if self.indexes_utf16() {
            format!("{pad}.repeat({needed}.div_ceil({pad}.len())).slice(0, {needed})")
        } else {
            format!("{pad}.chars().cycle().take({needed}).collect()")
        }
    }

    /// The `f64` code at the `usize` index `index` of the `&str` `receiver`
    /// (`charCodeAt`), NaN past the end.
    pub(super) fn string_code_unit_at_text(&self, receiver: &str, index: &str) -> String {
        if self.indexes_utf16() {
            format!("{receiver}.unit_at({index}).map_or(f64::NAN, f64::from)")
        } else {
            format!("match {receiver}.as_bytes().get(..={index}) {{ Some(smelt_prefix) if smelt_prefix.is_ascii() => f64::from(smelt_prefix[{index}]), _ => {receiver}.chars().nth({index}).map_or(f64::NAN, |ch| ch as u32 as f64) }}")
        }
    }

    /// The `Option<u32>` code point starting at the `usize` index `index` of
    /// `receiver` (`codePointAt`).
    pub(super) fn string_code_point_at_text(&self, receiver: &str, index: &str) -> String {
        if self.indexes_utf16() {
            format!("{receiver}.code_point_at({index})")
        } else {
            format!("{receiver}.chars().nth({index}).map(u32::from)")
        }
    }

    /// The `Vec<String>` of one string per index unit of `receiver` (`split("")`).
    pub(super) fn string_units_text(&self, receiver: &str) -> String {
        if self.indexes_utf16() {
            format!("{receiver}.split(&JsString::new(), usize::MAX)")
        } else {
            format!("{receiver}.chars().map(|ch| ch.to_string()).collect::<Vec<_>>()")
        }
    }
}
