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
}

/// A fact a producer states about an expression, beyond how it binds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExprFact {
    /// Nothing more than its precedence.
    Plain,
    /// `"literal".to_owned()`, the emitter's own spelling of a string constant.
    OwnedLiteral,
    /// `JsString::from("literal")`, the emitter's spelling of a JavaScript
    /// string constant.
    JsLiteral,
    /// The unit value `()`.
    Unit,
    /// The `self` receiver.
    Receiver,
    /// The erased `undefined` constant.
    Undefined,
    /// A tuple-indexed place, see [`RustExpr::indexed_place`].
    IndexedPlace,
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
    /// What the producer knows this expression to be, beyond how it binds.
    fact: ExprFact,
    /// Whether this default value resolves through a type's `Default` impl, at
    /// any depth, so a generated impl must bound that type.
    delegates_default: bool,
}

impl RustExpr {
    /// `text`, at `precedence`.
    fn with_precedence(text: impl Into<String>, precedence: Precedence) -> Self {
        Self { text: text.into(), precedence, clone_tail: false, fact: ExprFact::Plain, delegates_default: false }
    }

    /// An identifier or field path.
    #[must_use]
    pub fn path(text: impl Into<String>) -> Self {
        Self::with_precedence(text, Precedence::Path)
    }

    /// A postfix chain; see [`Precedence::Chain`].
    #[must_use]
    pub fn chain(text: impl Into<String>) -> Self {
        Self::with_precedence(text, Precedence::Chain)
    }

    /// A tightly binding non-chain; see [`Precedence::Atom`].
    #[must_use]
    pub fn atom(text: impl Into<String>) -> Self {
        Self::with_precedence(text, Precedence::Atom)
    }

    /// An `as` cast.
    #[must_use]
    pub fn cast(text: impl Into<String>) -> Self {
        Self::with_precedence(text, Precedence::Cast)
    }

    /// An expression with a loose top-level operator.
    #[must_use]
    pub fn loose(text: impl Into<String>) -> Self {
        Self::with_precedence(text, Precedence::Loose)
    }

    /// An expression whose producer does not state its form.
    #[must_use]
    pub fn opaque(text: impl AsRef<str>) -> Self {
        Self::with_precedence(text.as_ref(), Precedence::Unstated)
    }

    /// `text`, a postfix applied to `receiver`: it binds as `receiver` does.
    #[must_use]
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
    pub fn field_on(base: &(impl Formed + ?Sized), text: impl Into<String>) -> Self {
        Self::with_precedence(text, base.precedence())
    }

    /// Element `index` of the tuple `base`. It re-reads a place only when `base`
    /// is one.
    #[must_use]
    pub fn tuple_field(base: &Self, index: usize) -> Self {
        let element = Self::postfix_on(base, format!("{base}.{index}"));
        if base.is_trivial_place() { element.indexed_place() } else { element }
    }

    /// `text`, a postfix on `receiver` whose spine holds a path separator or a
    /// turbofish: an [`Precedence::Atom`] at best.
    #[must_use]
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

    /// A JavaScript string constant: `JsString::from("x")`.
    ///
    /// The literal body is escaped with Rust's debug string formatter. A
    /// Python `str` or a host string (a URL, a header name) is a Rust `String`
    /// and uses [`Self::string_literal`] instead.
    #[must_use]
    pub fn js_string_literal(value: &str) -> Self {
        let text = if value.is_empty() { JS_STRING_EMPTY.to_owned() } else { format!("{JS_STRING_FROM}{value:?})") };
        Self { fact: ExprFact::JsLiteral, ..Self::chain(text) }
    }

    /// A JavaScript string held as UTF-16 code units (one with a lone
    /// surrogate, which Rust text cannot spell): `JsString::from_units(&[..])`.
    #[must_use]
    pub fn js_string_units_literal(units: &[u16]) -> Self {
        let listed = units.iter().map(|unit| format!("0x{unit:04X}")).collect::<Vec<_>>().join(", ");
        Self::chain(format!("JsString::from_units(&[{listed}])"))
    }

    /// This expression, recorded as spelling `"literal".to_owned()`.
    #[must_use]
    pub fn owned_literal(mut self) -> Self {
        self.fact = ExprFact::OwnedLiteral;
        self
    }

    /// The unit value, which is also how an absent constant renders.
    #[must_use]
    pub fn unit() -> Self {
        Self { fact: ExprFact::Unit, ..Self::atom("()") }
    }

    /// The `self` receiver of a method body.
    #[must_use]
    pub fn receiver() -> Self {
        Self { fact: ExprFact::Receiver, ..Self::path("self") }
    }

    /// The erased `undefined` constant.
    #[must_use]
    pub fn undefined() -> Self {
        Self { fact: ExprFact::Undefined, ..Self::atom("SmeltUnknown::Undefined") }
    }

    /// This expression, recorded as a tuple-indexed place such as `a.0`: it binds
    /// as a chain but re-reads a binding with no evaluation, as a path does.
    #[must_use]
    pub fn indexed_place(mut self) -> Self {
        self.fact = ExprFact::IndexedPlace;
        self
    }

    /// This default value, recorded as resolving through `Default`.
    #[must_use]
    pub fn delegating_default(mut self) -> Self {
        self.delegates_default = true;
        self
    }

    /// This default value, delegating exactly when one of `parts` does.
    #[must_use]
    pub fn delegating_with<'a>(mut self, parts: impl IntoIterator<Item = &'a Self>) -> Self {
        self.delegates_default |= parts.into_iter().any(Self::delegates_default);
        self
    }

    /// Whether this default value resolves through a type's `Default` impl.
    #[must_use]
    pub fn delegates_default(&self) -> bool {
        self.delegates_default
    }

    /// Whether this expression is the unit value.
    #[must_use]
    pub fn is_unit(&self) -> bool {
        self.fact == ExprFact::Unit
    }

    /// Whether this expression is the `self` receiver.
    #[must_use]
    pub fn is_receiver(&self) -> bool {
        self.fact == ExprFact::Receiver
    }

    /// Whether this expression is the erased `undefined` constant.
    #[must_use]
    pub fn is_undefined(&self) -> bool {
        self.fact == ExprFact::Undefined
    }

    /// Whether this expression already ends in a `.clone()` call.
    #[must_use]
    pub fn has_clone_tail(&self) -> bool {
        self.clone_tail
    }

    /// Whether duplicating this expression re-reads a binding without
    /// re-evaluating anything or moving it twice.
    #[must_use]
    pub fn is_trivial_place(&self) -> bool {
        self.precedence == Precedence::Path || self.fact == ExprFact::IndexedPlace
    }

    /// How tightly this expression binds.
    #[must_use]
    pub fn binding(&self) -> Precedence {
        self.precedence
    }

    /// An owned copy of this value, without cloning what is already owned.
    ///
    /// `X.clone().clone()` computes exactly `X.clone()`: `Clone::clone` takes
    /// `&self` and returns an owned value, so a second call re-copies a temporary
    /// nothing else observes. A chain keeps the plain `value.clone()` spelling; any
    /// other form is parenthesised so the result splices into a larger expression.
    #[must_use]
    pub fn cloned_value(&self) -> Self {
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
    pub fn borrowed_value(&self) -> Self {
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
    pub fn parenthesized_if_needed(&self) -> String {
        if self.precedence.binds_tightly() {
            self.text.clone()
        } else {
            format!("({})", self.text)
        }
    }

    /// The text as the operand of an `as` cast: a cast or a unary operator binds
    /// at least as tightly, anything looser is parenthesized.
    #[must_use]
    pub fn as_cast_operand(&self) -> String {
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
}

impl From<String> for RustExpr {
    /// Text whose producer states nothing about it.
    fn from(text: String) -> Self {
        Self::opaque(text)
    }
}

impl From<&str> for RustExpr {
    /// Text whose producer states nothing about it.
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

/// The prefix of [`RustExpr::js_string_literal`]'s spelling; the literal and a
/// closing parenthesis follow.
const JS_STRING_FROM: &str = "JsString::from(";

/// [`RustExpr::js_string_literal`]'s spelling of the empty string.
const JS_STRING_EMPTY: &str = "JsString::new()";

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
    if value.fact == ExprFact::OwnedLiteral {
        let literal = &value.text[..value.text.len() - TO_OWNED_SUFFIX.len()];
        return RustExpr::atom(format!("SmeltUnknown::String({literal}.into())"));
    }
    if value.fact == ExprFact::JsLiteral {
        let literal = value.text.strip_prefix(JS_STRING_FROM).and_then(|rest| rest.strip_suffix(')')).unwrap_or("\"\"");
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

#[cfg(test)]
mod tests {
    use super::{CodeWriter, RustExpr, erased_string};

    /// The emitter's own literal spelling erases straight from the `&'static str`,
    /// so a constant JavaScript string costs one allocation, not a `String` that is
    /// immediately copied into an `Rc<str>` and dropped.
    #[test]
    fn erases_a_string_literal_without_an_intermediate_string() {
        assert_eq!(
            erased_string(&RustExpr::string_literal("hi")).as_str(),
            r#"SmeltUnknown::String("hi".into())"#
        );
    }

    /// A JavaScript string literal is spelled through `JsString` and erases
    /// from its `&'static str` the same way; the empty string is `new()`.
    #[test]
    fn spells_and_erases_a_javascript_string_literal() {
        assert_eq!(RustExpr::js_string_literal("a\"b").as_str(), r#"JsString::from("a\"b")"#);
        assert_eq!(RustExpr::js_string_literal("").as_str(), "JsString::new()");
        assert_eq!(
            erased_string(&RustExpr::js_string_literal("hi")).as_str(),
            r#"SmeltUnknown::String("hi".into())"#
        );
        assert_eq!(
            erased_string(&RustExpr::js_string_literal("")).as_str(),
            r#"SmeltUnknown::String("".into())"#
        );
    }

    /// A path needs no parentheses (`.into()` binds tighter than any operator),
    /// but anything that could end in a binary operator does, or the conversion
    /// would attach to that operator's right operand instead of the whole value.
    #[test]
    fn parenthesizes_every_erased_expression_that_is_not_a_path() {
        assert_eq!(
            erased_string(&RustExpr::path("self.message")).as_str(),
            "SmeltUnknown::String(self.message.into())"
        );
        assert_eq!(
            erased_string(&RustExpr::loose("left + right")).as_str(),
            "SmeltUnknown::String((left + right).into())"
        );
        // Two literals concatenated are not one literal: the `.to_owned()` suffix
        // belongs to the second operand, not to the whole expression.
        assert_eq!(
            erased_string(&RustExpr::loose(r#""a".to_owned() + &"b".to_owned()"#)).as_str(),
            r#"SmeltUnknown::String(("a".to_owned() + &"b".to_owned()).into())"#
        );
    }

    /// An owned copy is taken once: a chain that already ends in `.clone()` is not
    /// cloned again, and a form that cannot take a postfix is parenthesised.
    #[test]
    fn an_owned_copy_clones_only_what_is_not_owned() {
        assert_eq!(RustExpr::path("items").cloned_value().as_str(), "items.clone()");
        let owned = RustExpr::chain("items.clone()").ending_in_clone();
        assert_eq!(owned.cloned_value().as_str(), "items.clone()");
        assert_eq!(RustExpr::atom("Foo::new()").cloned_value().as_str(), "(Foo::new()).clone()");
        assert_eq!(RustExpr::loose("a + b").cloned_value().as_str(), "(a + b).clone()");
        let owned_atom = RustExpr::atom("Foo::new().clone()").ending_in_clone();
        assert_eq!(owned_atom.cloned_value().as_str(), "(Foo::new().clone())");
        assert_eq!(RustExpr::opaque("a").cloned_value().as_str(), "(a).clone()");
    }

    /// A borrow drops a chain's trailing clone, and keeps `&` off a looser operator.
    #[test]
    fn a_borrow_reads_through_a_trailing_clone() {
        assert_eq!(RustExpr::path("items").borrowed_value().as_str(), "&items");
        let owned = RustExpr::chain("items.clone()").ending_in_clone();
        assert_eq!(owned.borrowed_value().as_str(), "&items");
        assert_eq!(RustExpr::loose("a + b").borrowed_value().as_str(), "&(a + b)");
    }

    /// Each binding level answers the two inlining questions on its own terms.
    #[test]
    fn precedence_decides_parentheses() {
        assert_eq!(RustExpr::chain("a.b()").parenthesized_if_needed(), "a.b()");
        assert_eq!(RustExpr::cast("a as i64").parenthesized_if_needed(), "(a as i64)");
        assert_eq!(RustExpr::cast("a as i64").as_cast_operand(), "a as i64");
        assert_eq!(RustExpr::loose("a + b").as_cast_operand(), "(a + b)");
        assert_eq!(RustExpr::opaque("a").parenthesized_if_needed(), "(a)");
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
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent facts about what one rendered type spells"
)]
pub struct RustType {
    /// The rendered Rust text.
    text: String,
    /// What the type is, recorded by the arm that rendered it.
    shape: RustShape,
    /// Whether a typed `dyn Fn` callable appears anywhere inside this type.
    holds_callable: bool,
    /// Whether the erased runtime carrier appears anywhere inside this type.
    holds_carrier: bool,
    /// Whether the rendered type spells an erased value anywhere inside it.
    spells_erased: bool,
    /// Whether the rendered type spells a type parameter anywhere inside it.
    spells_type_param: bool,
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
            spells_erased: matches!(shape, RustShape::Erased | RustShape::ErasedFunction),
            spells_type_param: shape == RustShape::Generic,
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
            self.spells_erased |= part.spells_erased;
            self.spells_type_param |= part.spells_type_param;
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

    /// Whether the type spells an erased value anywhere inside it: the carrier
    /// or the erased callable. A type built on the carrier, such as a runtime
    /// container that uses it internally, does not.
    #[must_use]
    pub fn spells_erased(&self) -> bool {
        self.spells_erased
    }

    /// Whether the type spells a type parameter anywhere inside it.
    #[must_use]
    pub fn spells_type_param(&self) -> bool {
        self.spells_type_param
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

impl From<String> for RustType {
    /// A concrete type holding no callable, from already-rendered text.
    fn from(text: String) -> Self {
        Self::raw(text)
    }
}

impl AsRef<str> for RustType {
    fn as_ref(&self) -> &str {
        &self.text
    }
}

impl std::borrow::Borrow<str> for RustType {
    /// Lets a slice of types `join` into a comma-separated list.
    fn borrow(&self) -> &str {
        &self.text
    }
}

impl std::ops::Deref for RustType {
    type Target = str;

    /// The text, for the positions that splice it into a larger template.
    fn deref(&self) -> &str {
        &self.text
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
