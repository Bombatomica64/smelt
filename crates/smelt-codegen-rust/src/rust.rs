//! Small Rust source-building primitives for the Rust code generator.
//!
//! This module is intentionally conservative: it does not try to model the full
//! Rust grammar. Instead, it gives codegen a few typed wrappers for the fragments
//! that are easiest to misuse when emitted as raw strings. Future refactors can
//! migrate expression, statement, and item emission here incrementally without
//! changing the MIR-facing codegen API all at once.

/// Where a rendered Rust expression sits on the emitter's precedence ladder,
/// tightest first.
///
/// The producer of an expression states this, so a consumer asks the value
/// whether it needs wrapping instead of reading its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Precedence {
    /// An identifier or dotted field path: `a`, `self.items`.
    Path,
    /// A primary followed only by field, method and index postfixes, with no
    /// path separator in the spine: `a.b()`, `xs[0].y`, `"k".to_owned()`.
    Chain,
    /// Binds as tightly as a postfix expression but does not start a plain
    /// chain: a path call, a turbofish, a parenthesized group, a block, a
    /// `match`, a macro. Generated code spells `(Foo::new()).clone()` for these.
    Atom,
    /// An `as` cast. A further cast needs no parentheses; a postfix does.
    Cast,
    /// A loose top-level operator: a binary or unary operator, a closure, an
    /// `if`, a range.
    Loose,
    /// The producer did not say. Treated as [`Self::Loose`], which can only
    /// cost a pair of parentheses.
    Unstated,
}

impl Precedence {
    /// Whether this expression can take a postfix without changing how the
    /// generated source spells it.
    #[must_use]
    pub fn is_chain(self) -> bool {
        matches!(self, Self::Path | Self::Chain)
    }

    /// Whether a postfix, a cast or a unary operator can be applied to this
    /// expression without parentheses.
    #[must_use]
    pub fn binds_tightly(self) -> bool {
        self <= Self::Atom
    }
}

/// What a value knows about its own form; bare text knows nothing.
pub trait Formed {
    /// How tightly this expression binds.
    fn precedence(&self) -> Precedence;
    /// TEMPORARY migration audit.
    fn origin(&self) -> String { "bare".to_owned() }
}

impl Formed for str {
    fn precedence(&self) -> Precedence {
        Precedence::Unstated
    }
}

impl Formed for String {
    fn precedence(&self) -> Precedence {
        Precedence::Unstated
    }
}

impl<T: Formed + ?Sized> Formed for &T {
    fn precedence(&self) -> Precedence {
        (**self).precedence()
    }
    fn origin(&self) -> String { (**self).origin() }
}

/// The spelling of a method-call clone, as [`RustExpr::ending_in_clone`] records it.
const CLONE_SUFFIX: &str = ".clone()";

/// A Rust expression fragment that has been rendered as source text, with the
/// facts its producer knows about that text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustExpr {
    /// The rendered source.
    text: String,
    /// How tightly the rendered text binds.
    precedence: Precedence,
    /// Whether the text ends in a `.clone()` call, so it is already owned.
    clone_tail: bool,
    /// Whether the text is `"literal".to_owned()`, the emitter's own spelling of
    /// a string constant.
    owned_literal: bool,
    /// TEMPORARY migration audit: who built this expression.
    pub origin: &'static std::panic::Location<'static>,
}

impl RustExpr {
    /// `text`, at `precedence`.
    #[track_caller]
    fn with_precedence(text: impl Into<String>, precedence: Precedence) -> Self {
        Self { text: text.into(), precedence, clone_tail: false, owned_literal: false, origin: std::panic::Location::caller() }
    }

    /// An identifier or field path.
    #[must_use]
    #[track_caller]
    pub fn path(text: impl Into<String>) -> Self {
        Self::with_precedence(text, Precedence::Path)
    }

    /// A postfix chain; see [`Precedence::Chain`].
    #[must_use]
    #[track_caller]
    pub fn chain(text: impl Into<String>) -> Self {
        Self::with_precedence(text, Precedence::Chain)
    }

    /// A tightly binding non-chain; see [`Precedence::Atom`].
    #[must_use]
    #[track_caller]
    pub fn atom(text: impl Into<String>) -> Self {
        Self::with_precedence(text, Precedence::Atom)
    }

    /// An `as` cast.
    #[must_use]
    #[track_caller]
    pub fn cast(text: impl Into<String>) -> Self {
        Self::with_precedence(text, Precedence::Cast)
    }

    /// An expression with a loose top-level operator.
    #[must_use]
    #[track_caller]
    pub fn loose(text: impl Into<String>) -> Self {
        Self::with_precedence(text, Precedence::Loose)
    }

    /// An expression whose producer does not state its form.
    #[must_use]
    #[track_caller]
    pub fn opaque(text: impl AsRef<str>) -> Self {
        Self::with_precedence(text.as_ref(), Precedence::Unstated)
    }

    /// `text`, a postfix applied to `receiver`: it binds as `receiver` does.
    #[must_use]
    #[track_caller]
    pub fn postfix_on(receiver: &(impl Formed + ?Sized), text: impl Into<String>) -> Self {
        let precedence = match receiver.precedence() {
            Precedence::Path => Precedence::Chain,
            other => other,
        };
        Self::with_precedence(text, precedence)
    }

    /// `text`, a field of `base`: a path stays a path, so `self.items` is
    /// spelled without the parentheses a method chain may need.
    #[must_use]
    #[track_caller]
    pub fn field_on(base: &(impl Formed + ?Sized), text: impl Into<String>) -> Self {
        Self::with_precedence(text, base.precedence())
    }

    /// `text`, a postfix on `receiver` whose spine holds a path separator or a
    /// turbofish: an [`Precedence::Atom`] at best.
    #[must_use]
    #[track_caller]
    pub fn atom_on(receiver: &(impl Formed + ?Sized), text: impl Into<String>) -> Self {
        Self::with_precedence(text, receiver.precedence().max(Precedence::Atom))
    }

    /// This expression, recorded as ending in a `.clone()` call.
    #[must_use]
    pub fn ending_in_clone(mut self) -> Self {
        self.clone_tail = true;
        self
    }

    /// Creates an owned Rust `String` expression from arbitrary string data.
    ///
    /// The literal body is escaped with Rust's debug string formatter, then
    /// `.to_owned()` is appended to match Smelt's generated `String` convention.
    #[must_use]
    pub fn string_literal(value: &str) -> Self {
        Self::chain(format!("{value:?}.to_owned()")).owned_literal()
    }

    /// This expression, recorded as spelling `"literal".to_owned()`.
    #[must_use]
    pub fn owned_literal(mut self) -> Self {
        self.owned_literal = true;
        self
    }

    /// TEMPORARY migration audit.
    #[track_caller]
    fn audit(&self, kind: &str) {
        use std::io::Write;
        if self.precedence != Precedence::Unstated { return; }
        let Ok(path) = std::env::var("SMELT_SHADOW") else { return };
        let at = std::panic::Location::caller();
        let short: String = self.text.chars().take(90).collect::<String>().replace('\n', " ");
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "U {kind} {}:{} {}:{} Unstated {short}", at.file(), at.line(), self.origin.file(), self.origin.line());
        }
    }

    /// How tightly this expression binds.
    #[must_use]
    pub fn binding(&self) -> Precedence {
        self.precedence
    }

    /// Whether this expression spells `"literal".to_owned()`.
    #[must_use]
    pub fn is_owned_literal(&self) -> bool {
        self.owned_literal
    }

    /// An owned copy of this value, without cloning what is already owned.
    ///
    /// `X.clone().clone()` computes exactly `X.clone()`: `Clone::clone` takes
    /// `&self` and returns an owned value, so a second call re-copies a temporary
    /// nothing else observes. A chain keeps the plain `value.clone()` spelling; any
    /// other form is parenthesised so the result splices into a larger expression.
    #[must_use]
    #[track_caller]
    pub fn cloned_value(&self) -> Self {
        self.audit("clone");
        match (self.precedence.is_chain(), self.clone_tail) {
            (true, true) => self.clone(),
            (true, false) => Self::chain(format!("{}{CLONE_SUFFIX}", self.text)).ending_in_clone(),
            (false, true) => Self::atom(format!("({})", self.text)),
            (false, false) => Self::atom(format!("({}){CLONE_SUFFIX}", self.text)).ending_in_clone(),
        }
    }

    /// A shared borrow of the value this expression evaluates to.
    ///
    /// A chain keeps the plain `&value` spelling and loses a trailing `.clone()`:
    /// `a.clone()` and `a` borrow equal values, so the copy is dead weight behind
    /// a `&`. Any looser expression is parenthesised so the `&` cannot bind more
    /// tightly than the caller meant.
    #[must_use]
    #[track_caller]
    pub fn borrowed_value(&self) -> Self {
        self.audit("borrow");
        if !self.precedence.is_chain() {
            return Self::loose(format!("&({})", self.text));
        }
        let place = if self.clone_tail {
            &self.text[..self.text.len() - CLONE_SUFFIX.len()]
        } else {
            self.text.as_str()
        };
        Self::loose(format!("&{place}"))
    }

    /// The text, parenthesized only when it cannot take a postfix.
    #[must_use]
    #[track_caller]
    pub fn parenthesized_if_needed(&self) -> String {
        self.audit("paren");
        if self.precedence.binds_tightly() {
            self.text.clone()
        } else {
            format!("({})", self.text)
        }
    }

    /// The text as the operand of an `as` cast: a cast or a unary operator binds
    /// at least as tightly, anything looser is parenthesized.
    #[must_use]
    #[track_caller]
    pub fn as_cast_operand(&self) -> String {
        self.audit("cast");
        if self.precedence <= Precedence::Cast {
            self.text.clone()
        } else {
            format!("({})", self.text)
        }
    }

    /// Returns the rendered source text for this expression.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Consumes the expression and returns its rendered source text.
    #[must_use]
    pub fn into_string(self) -> String {
        self.text
    }
}

impl Formed for RustExpr {
    fn precedence(&self) -> Precedence {
        self.precedence
    }
    fn origin(&self) -> String { format!("{}:{}", self.origin.file(), self.origin.line()) }
}

impl From<String> for RustExpr {
    /// Text whose producer states nothing about it.
    #[track_caller]
    fn from(text: String) -> Self {
        Self::opaque(text)
    }
}

impl From<&str> for RustExpr {
    /// Text whose producer states nothing about it.
    #[track_caller]
    fn from(text: &str) -> Self {
        Self::opaque(text)
    }
}

impl From<RustExpr> for String {
    fn from(expr: RustExpr) -> Self {
        expr.text
    }
}

impl std::borrow::Borrow<str> for RustExpr {
    /// Lets a slice of expressions `join` into a comma-separated list.
    fn borrow(&self) -> &str {
        &self.text
    }
}

impl AsRef<str> for RustExpr {
    fn as_ref(&self) -> &str {
        &self.text
    }
}

impl std::ops::Deref for RustExpr {
    type Target = str;

    /// The text, for the positions that splice it into a larger template.
    fn deref(&self) -> &str {
        &self.text
    }
}

/// The `.to_owned()` that follows the emitter's own spelling of a string constant.
const TO_OWNED_SUFFIX: &str = ".to_owned()";

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
pub fn erased_string(value: &RustExpr) -> RustExpr {
    if value.owned_literal {
        let literal = &value.text[..value.text.len() - TO_OWNED_SUFFIX.len()];
        return RustExpr::atom(format!("SmeltUnknown::String({literal}.into())"));
    }
    if value.precedence == Precedence::Path {
        return RustExpr::atom(format!("SmeltUnknown::String({value}.into())"));
    }
    RustExpr::atom(format!("SmeltUnknown::String(({value}).into())"))
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
