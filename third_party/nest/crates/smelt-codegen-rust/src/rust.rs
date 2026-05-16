//! Small Rust source-building primitives for the Rust code generator.
//!
//! This module is intentionally conservative: it does not try to model the full
//! Rust grammar. Instead, it gives codegen a few typed wrappers for the fragments
//! that are easiest to misuse when emitted as raw strings. Future refactors can
//! migrate expression, statement, and item emission here incrementally without
//! changing the MIR-facing codegen API all at once.

/// A Rust expression fragment that has already been rendered as source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustExpr(String);

impl RustExpr {
    /// Creates an owned Rust `String` expression from arbitrary string data.
    ///
    /// The literal body is escaped with Rust's debug string formatter, then
    /// `.to_owned()` is appended to match Smelt's generated `String` convention.
    #[must_use]
    pub fn string_literal(value: &str) -> Self {
        Self(format!("{value:?}.to_owned()"))
    }

    /// Returns the rendered source text for this expression.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the expression and returns its rendered source text.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl std::fmt::Display for RustExpr {
    /// Writes the rendered expression text.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A valid Rust identifier rendered from source-language or MIR data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustIdent(String);

impl RustIdent {
    /// Sanitizes arbitrary text into a valid Rust identifier.
    ///
    /// Invalid characters are replaced with underscores, leading digits are
    /// escaped by replacement, and Rust keywords receive a trailing underscore.
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self(sanitize_identifier(name))
    }

    /// Returns the rendered identifier text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the identifier and returns the rendered source text.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl std::fmt::Display for RustIdent {
    /// Writes the rendered identifier text.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A line-oriented writer for Rust source with indentation support.
#[derive(Debug, Default, Clone)]
pub struct CodeWriter {
    /// The accumulated source text.
    output: String,
    /// The current indentation depth in four-space units.
    indent: usize,
}

impl CodeWriter {
    /// Creates an empty Rust source writer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a complete source line at the current indentation level.
    pub fn line(&mut self, text: impl AsRef<str>) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
        self.output.push_str(text.as_ref());
        self.output.push('\n');
    }

    /// Appends a blank line.
    pub fn blank_line(&mut self) {
        self.output.push('\n');
    }

    /// Appends a braced block and runs the callback at one deeper indentation.
    ///
    /// The `header` must contain the text before the opening brace, for example
    /// `fn main()` or `impl Widget`. The writer emits both braces and newlines.
    pub fn block(&mut self, header: impl AsRef<str>, body: impl FnOnce(&mut Self)) {
        self.line(format!("{} {{", header.as_ref()));
        self.indent = self.indent.saturating_add(1);
        body(self);
        self.indent = self.indent.saturating_sub(1);
        self.line("}");
    }

    /// Consumes the writer and returns the accumulated source text.
    #[must_use]
    pub fn finish(self) -> String {
        self.output
    }
}

/// Sanitizes arbitrary text into a valid Rust identifier string.
#[must_use]
pub fn sanitize_identifier(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for (idx, ch) in name.chars().enumerate() {
        if ch == '_' || ch.is_ascii_alphanumeric() && (idx > 0 || !ch.is_ascii_digit()) {
            out.push(ch);
        } else {
            out.push('_');
        }
    }

    if out.is_empty() || is_rust_keyword(&out) {
        out.push('_');
    }
    out
}

/// Returns true when the identifier is reserved by Rust.
fn is_rust_keyword(name: &str) -> bool {
    matches!(
        name,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
            | "try"
    )
}

#[cfg(test)]
mod tests {
    use super::{CodeWriter, RustExpr, RustIdent, sanitize_identifier};

    #[test]
    fn sanitizes_invalid_and_reserved_identifiers() {
        assert_eq!(sanitize_identifier("valid_name"), "valid_name");
        assert_eq!(sanitize_identifier("1st-value"), "_st_value");
        assert_eq!(sanitize_identifier("fn"), "fn_");
        assert_eq!(RustIdent::new("self").as_str(), "self_");
    }

    #[test]
    fn renders_owned_string_literals_with_escaping() {
        assert_eq!(
            RustExpr::string_literal("a\n\"b\"").as_str(),
            "\"a\\n\\\"b\\\"\".to_owned()"
        );
    }

    #[test]
    fn writes_indented_blocks() {
        let mut writer = CodeWriter::new();
        writer.block("fn main()", |writer| {
            writer.line("let value = 1;");
        });

        assert_eq!(writer.finish(), "fn main() {\n    let value = 1;\n}\n");
    }
}
