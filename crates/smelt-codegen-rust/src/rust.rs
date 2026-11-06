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

/// Whether `text` is exactly one Rust string literal (no trailing operators).
///
/// Used by [`erased_string`] to tell the emitter's own literal spelling apart
/// from an arbitrary rendered expression that merely ends in a quote.
#[must_use]
fn is_string_literal(text: &str) -> bool {
    let Some(body) = text.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')) else {
        return false;
    };
    let mut escaped = false;
    for ch in body.chars() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            // A closing quote inside the body means `text` was more than one
            // literal (e.g. `"a".to_owned() + "b"`), so it is not a literal.
            return false;
        }
    }
    !escaped
}

/// Renders the erased-string construction for a rendered `String`-typed expression.
///
/// `SmeltUnknown::String` holds a shared immutable `Rc<str>` because JavaScript
/// strings are immutable, so an owned `String` is converted at this seam. When
/// the expression is the emitter's own literal spelling (`"x".to_owned()`), the
/// intermediate `String` is dropped and the `Rc<str>` is built directly from the
/// `&'static str`: one allocation instead of two. Every other expression is
/// converted with `.into()`, which is a no-op for an `Rc<str>` and a single
/// buffer move-and-copy for a `String`.
#[must_use]
pub fn erased_string(text: &str) -> String {
    match text.strip_suffix(".to_owned()") {
        Some(literal) if is_string_literal(literal) => {
            format!("SmeltUnknown::String({literal}.into())")
        }
        _ if is_field_path(text) => format!("SmeltUnknown::String({text}.into())"),
        _ => format!("SmeltUnknown::String(({text}).into())"),
    }
}

/// Whether `text` is a bare identifier or dotted field path (`self.message`).
///
/// Method-call and field syntax bind tighter than every operator, so appending
/// `.into()` to such a path needs no parentheses; anything else gets them,
/// because the expression may end in a binary operator whose operand `.into()`
/// would otherwise steal.
#[must_use]
fn is_field_path(text: &str) -> bool {
    !text.is_empty()
        && text.split('.').all(|segment| {
            let mut chars = segment.chars();
            chars
                .next()
                .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
                && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        })
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

/// Sanitizes an identifier into a Rust-safe spelling.
pub(crate) fn sanitize_ident(name: &str) -> String {
    RustIdent::new(name).into_string()
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

    if out.is_empty() || is_rust_keyword(&out) || is_rust_prelude_name(&out) {
        out.push('_');
    }
    out
}

/// Returns true when the identifier names an item of Rust's prelude that the
/// generated runtime spells unqualified.
///
/// A source class named `Box` or `Vec` is legal TypeScript, but declaring
/// `struct Box` shadows `std::boxed::Box` for the whole crate, and every
/// prelude line that writes `Box<dyn ..>` then refers to the user's struct
/// (E0107). Renaming the source item the way a keyword is renamed keeps both:
/// the declaration and every reference go through this function.
///
/// `main` is the same case one level up: the crate root defines the binary's
/// entry point `fn main`, so a source function named `main` (`async function
/// main() {..}; main()`, a common script shape) was a second `main` (E0428).
fn is_rust_prelude_name(name: &str) -> bool {
    matches!(
        name,
        "Box" | "Vec" | "Option" | "Result" | "String" | "Some" | "None" | "Ok" | "Err" | "main"
    )
}

/// Returns true when the identifier is reserved by Rust.
///
/// This is the full keyword list of the Rust 2024 edition generated crates
/// are built with: the strict keywords, the 2018 additions (`async`, `await`,
/// `dyn`), and every reserved keyword, including `gen`, which the 2024 edition
/// reserved for generator blocks. A name on it is renamed with a trailing `_`
/// (not spelled as a raw identifier) because `self`, `Self`, `super` and
/// `crate` cannot be raw identifiers at all, and one scheme for every keyword
/// keeps a declaration and its references agreeing without knowing which
/// edition introduced the word. The weak keywords (`union`, `raw`, `safe`,
/// `macro_rules`) remain valid identifiers and are deliberately absent.
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
            | "gen"
    )
}

#[cfg(test)]
mod tests {
    use super::{CodeWriter, RustExpr, RustIdent, erased_string, sanitize_identifier};

    /// The emitter's own literal spelling erases straight from the `&'static str`,
    /// so a constant JavaScript string costs one allocation, not a `String` that is
    /// immediately copied into an `Rc<str>` and dropped.
    #[test]
    fn erases_a_string_literal_without_an_intermediate_string() {
        assert_eq!(
            erased_string(&RustExpr::string_literal("hi").into_string()),
            r#"SmeltUnknown::String("hi".into())"#
        );
    }

    /// A path needs no parentheses (`.into()` binds tighter than any operator),
    /// but anything that could end in a binary operator does, or the conversion
    /// would attach to that operator's right operand instead of the whole value.
    #[test]
    fn parenthesizes_every_erased_expression_that_is_not_a_path() {
        assert_eq!(
            erased_string("self.message"),
            "SmeltUnknown::String(self.message.into())"
        );
        assert_eq!(
            erased_string("left + right"),
            "SmeltUnknown::String((left + right).into())"
        );
        // Two literals concatenated are not one literal: the `.to_owned()` suffix
        // belongs to the second operand, not to the whole expression.
        assert_eq!(
            erased_string(r#""a".to_owned() + &"b".to_owned()"#),
            r#"SmeltUnknown::String(("a".to_owned() + &"b".to_owned()).into())"#
        );
    }

    #[test]
    fn sanitizes_invalid_and_reserved_identifiers() {
        assert_eq!(sanitize_identifier("valid_name"), "valid_name");
        assert_eq!(sanitize_identifier("1st-value"), "_st_value");
        assert_eq!(sanitize_identifier("fn"), "fn_");
        // Reserved by the 2024 edition only; `fn gen` does not parse there.
        assert_eq!(sanitize_identifier("gen"), "gen_");
        for reserved in ["async", "await", "dyn", "try", "abstract", "become", "box", "do",
            "final", "macro", "override", "priv", "typeof", "unsized", "virtual", "yield"]
        {
            assert_eq!(sanitize_identifier(reserved), format!("{reserved}_"));
        }
        // Weak keywords are ordinary identifiers.
        assert_eq!(sanitize_identifier("union"), "union");
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

/// A Rust type fragment that has already been rendered as source text.
///
/// Deliberately a newtype over `String` rather than a structured type. The
/// canonical lowering entry point (`FunctionEmitter::rust_type`) is required to
/// reproduce today's rendered bytes exactly, and every structural rendering
/// decision it makes — container spellings, the one-tuple `(T,)` form, the
/// `impl Fn` versus `Rc<dyn Fn>` split, inference placeholders — is a place a
/// re-derived `Display` implementation could differ by a character. Wrapping the
/// existing text costs nothing and makes the return type nameable now; giving
/// `RustType` an internal representation later is a change behind these same
/// constructors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustType {
    /// The rendered Rust text.
    text: String,
    /// What the type is, recorded by the arm that rendered it.
    shape: RustShape,
    /// Whether a typed `dyn Fn` callable appears anywhere inside this type.
    holds_callable: bool,
    /// Whether the erased runtime carrier appears anywhere inside this type.
    holds_carrier: bool,
}

/// What a rendered Rust type is, for a decision that depends on the output type
/// rather than the MIR type.
///
/// MIR cannot answer these on its own: whether a `T` erases, or a union stays
/// on the dynamic carrier, depends on the scope the type renders in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustShape {
    /// The runtime carrier `SmeltUnknown`.
    Erased,
    /// The erased callable `SmeltErasedFunction`.
    ErasedFunction,
    /// A typed callable, `Rc<dyn Fn(..)>` or `impl Fn(..)`.
    Callable,
    /// A type parameter spelled as a Rust generic.
    Generic,
    /// A nominal type spelled with type arguments (`Name<..>`).
    GenericClass,
    /// Any other concrete type.
    Concrete,
}

impl RustType {
    /// A concrete type holding no callable, from already-rendered text.
    #[must_use]
    pub fn raw(text: impl Into<String>) -> Self {
        Self::with_shape(text, RustShape::Concrete)
    }

    /// `text`, of the given shape.
    #[must_use]
    pub fn with_shape(text: impl Into<String>, shape: RustShape) -> Self {
        Self {
            text: text.into(),
            shape,
            holds_callable: shape == RustShape::Callable,
            holds_carrier: shape == RustShape::Erased,
        }
    }

    /// The runtime carrier.
    #[must_use]
    pub fn erased() -> Self {
        Self::with_shape("SmeltUnknown", RustShape::Erased)
    }

    /// A concrete type built around `parts`, which it holds.
    #[must_use]
    pub fn composed<'a>(text: impl Into<String>, parts: impl IntoIterator<Item = &'a Self>) -> Self {
        Self::raw(text).holding(parts)
    }

    /// This type, recorded as holding what `parts` hold.
    #[must_use]
    pub fn holding<'a>(mut self, parts: impl IntoIterator<Item = &'a Self>) -> Self {
        for part in parts {
            self.holds_callable |= part.holds_callable;
            self.holds_carrier |= part.holds_carrier;
        }
        self
    }

    /// This type, recorded as being built on the erased runtime carrier.
    #[must_use]
    pub fn on_carrier(mut self) -> Self {
        self.holds_carrier = true;
        self
    }

    /// What this type is.
    #[must_use]
    pub fn shape(&self) -> RustShape {
        self.shape
    }

    /// Whether a typed `dyn Fn` callable appears anywhere inside this type.
    #[must_use]
    pub fn holds_callable(&self) -> bool {
        self.holds_callable
    }

    /// Whether the erased runtime carrier appears anywhere inside this type.
    #[must_use]
    pub fn holds_carrier(&self) -> bool {
        self.holds_carrier
    }

    /// Returns the rendered source text for this type.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Consumes the type and returns its rendered source text.
    #[must_use]
    pub fn into_string(self) -> String {
        self.text
    }
}

impl std::fmt::Display for RustType {
    /// Writes the rendered type text.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod rust_type_tests {
    use super::*;

    #[test]
    fn a_callable_inside_a_composed_type_is_visible_from_outside() {
        let callable = RustType::with_shape("::std::rc::Rc<dyn Fn()>", RustShape::Callable);
        let list = RustType::composed("SmeltList<::std::rc::Rc<dyn Fn()>>", [&callable]);
        let optional = RustType::composed("Option<SmeltList<::std::rc::Rc<dyn Fn()>>>", [&list]);
        assert!(optional.holds_callable());
        assert_eq!(optional.shape(), RustShape::Concrete);
    }

    #[test]
    fn a_plain_type_holds_no_callable() {
        let item = RustType::raw("f64");
        assert!(!RustType::composed("SmeltList<f64>", [&item]).holds_callable());
        assert!(!RustType::erased().holds_callable());
        assert_eq!(RustType::erased().as_str(), "SmeltUnknown");
    }
}
