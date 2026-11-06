//! Typed "rendered value" abstraction for the Rust codegen emitter.
//!
//! The emitter renders Rust source as bare `String`s. Two facts that travel
//! *with* a rendered expression are recomputed at every hop instead:
//!
//! 1. **Its MIR type.** The coercion seam (`emitter/coercion.rs`) already passes
//!    `(text, src_type)` pairs through its `*_text` verbs. Keeping the type
//!    glued to the text removes one class of "which type was this again?" bugs.
//! 2. **Its parenthesization need.** When a caller inlines a rendered fragment
//!    into a larger expression (`SmeltUnknown::String({text})`, `({text} as f64)`,
//!    `Some({text})`, `{text}.clone()`), it silently assumes the fragment binds
//!    tightly enough not to need parentheses. That assumption is each caller's
//!    private guess and the historical source of precedence bugs.
//!
//! [`RenderedValue`] carries all three (text, [`TypeId`], [`Precedence`]) so a
//! consumer can ask the value itself whether it needs wrapping rather than
//! guessing. Phase 1 adopts this only at the coercion seam and its direct
//! callers; the rest of the emitter still threads bare strings and is bridged by
//! the transitional `*_text` wrappers documented in `coercion.rs`.
//!
//! [`cloned_value_text`] and [`borrowed_value_text`] are the part that still reads
//! text. They answer "is this a postfix chain" for a fragment whose producer did
//! not say, which every `*_text` producer would have to start saying before they
//! can go. `erase_value_text` does pass [`RenderedValue::atom`] truthfully: MIR is
//! three-address, so what reaches it is an operand.

use smelt_hir::TypeId;

/// Where a rendered Rust expression sits on the precedence ladder, recorded only
/// at the granularity the emitter actually needs to make parenthesization
/// decisions.
///
/// This is deliberately coarse: the seam never emits raw operator chains like
/// `a + b * c` whose *internal* precedence matters. What it does do is wrap
/// fragments in tighter-binding syntax (a cast, a method call, a `Some(...)`
/// constructor). The only question that matters there is binary: "if I drop this
/// fragment to the left of a `.method()` or `as` cast, or inside a unary/tuple
/// position, will it reassociate?".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Precedence {
    /// A primary or postfix expression that is safe to inline in any position
    /// without parentheses: identifiers, literals, function/method calls,
    /// already-parenthesized groups `(...)`, braced blocks `{ ... }`,
    /// `match { ... }` expressions, struct literals `Foo { ... }`, `vec![...]`,
    /// field access, and indexing. These never reassociate against a trailing
    /// `.method()`, a following `as` cast, or an enclosing constructor call.
    Atom,
    /// An expression whose top-level operator binds looser than the contexts the
    /// seam inlines into, so it must be wrapped in parentheses before being used
    /// as the receiver of a method call, the operand of an `as` cast, the body of
    /// a unary operator, or an unparenthesized argument-like position. Examples:
    /// `x as f64`, `a + b`, `if c { a } else { b }` used as a cast operand,
    /// closures, and range expressions.
    NeedsParens,
}

/// A rendered Rust expression bundled with its MIR type and the parenthesization
/// it requires when inlined into a tighter-binding context.
///
/// Construct one with [`RenderedValue::atom`] (the common case: the text is a
/// primary/postfix expression) or [`RenderedValue::with_precedence`] when the
/// rendered text has a loose top-level operator. Consume it with
/// [`RenderedValue::into_text`] (raw text, no wrapping) or
/// [`RenderedValue::parenthesized_if_needed`] (text safe to drop into a
/// tighter-binding position).
#[derive(Clone, Debug)]
pub(super) struct RenderedValue {
    /// The rendered Rust source for this expression.
    text: String,
    /// The MIR type the rendered expression evaluates to.
    ty: TypeId,
    /// How tightly the rendered text binds, used to decide parenthesization.
    precedence: Precedence,
}

impl RenderedValue {
    /// Builds a rendered value whose text is a primary or postfix expression
    /// ([`Precedence::Atom`]).
    ///
    /// Use this when the text is an identifier, literal, function/method call,
    /// struct literal, braced block, `match`/`if` *block* expression, `vec![...]`,
    /// or anything already wrapped in `(...)` — i.e. anything that never needs
    /// extra parentheses when inlined. This is the overwhelmingly common shape
    /// the coercion seam produces, which is why it is the terse constructor.
    pub(super) fn atom(text: impl Into<String>, ty: TypeId) -> Self {
        Self {
            text: text.into(),
            ty,
            precedence: Precedence::Atom,
        }
    }

    /// Builds a rendered value with an explicit [`Precedence`].
    ///
    /// Use [`Precedence::NeedsParens`] when the rendered text has a loose
    /// top-level operator (a cast `as`, a binary operator, a closure, an `if`
    /// used as a value, a range) so consumers know to wrap it before inlining it
    /// into a tighter-binding position.
    pub(super) fn with_precedence(
        text: impl Into<String>,
        ty: TypeId,
        precedence: Precedence,
    ) -> Self {
        Self {
            text: text.into(),
            ty,
            precedence,
        }
    }

    /// The MIR type this rendered expression evaluates to.
    pub(super) fn ty(&self) -> TypeId {
        self.ty
    }

    /// Borrows the rendered text without any parenthesization.
    ///
    /// Use this for positions that already supply their own grouping (the inside
    /// of `(...)`, `[...]`, `{ ... }`, a `format!` placeholder that the caller
    /// then wraps, etc.) or where the text stands alone as a full statement or
    /// tail expression.
    pub(super) fn text(&self) -> &str {
        &self.text
    }

    /// Consumes the value and returns its rendered text verbatim, without any
    /// parenthesization.
    ///
    /// This is the transitional bridge back to the bare-`String` world: callers
    /// that have not yet adopted [`RenderedValue`] take the text and continue as
    /// before. Prefer [`RenderedValue::parenthesized_if_needed`] when the text is
    /// about to be inlined into a tighter-binding context.
    pub(super) fn into_text(self) -> String {
        self.text
    }

    /// Returns the rendered text wrapped in parentheses only when its
    /// [`Precedence`] is [`Precedence::NeedsParens`].
    ///
    /// Use this whenever the text is inlined as the receiver of a `.method()`,
    /// the operand of an `as` cast, the body of a unary operator, or an
    /// argument-like position where a loose top-level operator would
    /// reassociate. An [`Precedence::Atom`] is returned unchanged so byte-output
    /// stays identical to the historical hand-rolled inlining for the common
    /// case.
    pub(super) fn parenthesized_if_needed(&self) -> String {
        match self.precedence {
            Precedence::Atom => self.text.clone(),
            Precedence::NeedsParens => format!("({})", self.text),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Precedence, RenderedValue};
    use smelt_hir::TypeId;

    /// A throwaway type id; `RenderedValue` never interprets it in these tests.
    const TY: TypeId = TypeId(0);

    /// An atom is inlined verbatim in every position — this is what keeps the
    /// common-case emitted bytes identical to the historical hand-rolled string.
    #[test]
    fn atom_is_never_parenthesized() {
        let value = RenderedValue::atom("smelt_value.to_lowercase()", TY);
        assert_eq!(value.parenthesized_if_needed(), "smelt_value.to_lowercase()");
        assert_eq!(value.into_text(), "smelt_value.to_lowercase()");
    }

    /// A loose operand (here a binary `+`) must be wrapped before it is dropped
    /// into a tighter-binding position. This is exactly the latent precedence bug
    /// the seam used to risk: `SmeltUnknown::Number(a + b as f64)` parses as
    /// `a + (b as f64)`, whereas `SmeltUnknown::Number((a + b) as f64)` is correct.
    #[test]
    fn needs_parens_value_is_wrapped_for_tight_contexts() {
        let value = RenderedValue::with_precedence("a + b", TY, Precedence::NeedsParens);
        assert_eq!(value.parenthesized_if_needed(), "(a + b)");
        // The raw text and statement-position accessors stay unwrapped.
        assert_eq!(value.text(), "a + b");
        assert_eq!(value.into_text(), "a + b");
    }

    /// The cast operand shape the `erase` Int/Float arm builds: wrapping a loose
    /// operand keeps `as f64` from reassociating across it.
    #[test]
    fn cast_operand_wrapping_matches_erase_number_arm() {
        let atom = RenderedValue::atom("value", TY);
        assert_eq!(
            format!("SmeltUnknown::Number({} as f64)", atom.parenthesized_if_needed()),
            "SmeltUnknown::Number(value as f64)"
        );
        let loose = RenderedValue::with_precedence("a + b", TY, Precedence::NeedsParens);
        assert_eq!(
            format!("SmeltUnknown::Number({} as f64)", loose.parenthesized_if_needed()),
            "SmeltUnknown::Number((a + b) as f64)"
        );
    }
}

/// Renders an owned copy of an already-rendered expression, without cloning a
/// value that is already owned.
///
/// Many emitter helpers need an owned value of a subexpression they were handed
/// as text, so they spell `{text}.clone()`. But `text` very often *already* ends
/// in a `.clone()` -- it came from `operand_text`/`local_value_text`, which clone
/// the local they read. The result is `x.clone().clone()`: a second deep copy of
/// a temporary that nothing else can observe. es-toolkit emitted 2881 of them
/// and remeda 1348.
///
/// `X.clone().clone()` computes exactly `X.clone()` for every `X`: `Clone::clone`
/// takes `&self` and returns an owned value, so the outer call re-copies a value
/// that is already owned and yields an equal one (Smelt's containers derive
/// `Clone`, so identity fields such as `SmeltList::id` are preserved either way).
/// Dropping it is therefore always sound.
///
/// The result is parenthesised only when `text` is not already a postfix chain,
/// so a caller can splice it into a larger expression without worrying about
/// precedence while generated code keeps the plain `value.clone()` spelling a
/// hand-written port would use instead of a noisy `(value).clone()`.
///
/// This is a syntax-directed peephole on text this module already owns, not a
/// per-construct special case: it fires wherever a caller asks for an owned
/// copy, whatever construct produced the inner expression.
pub(super) fn cloned_value_text(text: &str) -> String {
    let trimmed = text.trim();
    let already_owned = trimmed.ends_with(".clone()");
    match (is_postfix_chain(trimmed), already_owned) {
        (true, true) => trimmed.to_owned(),
        (true, false) => format!("{trimmed}.clone()"),
        (false, true) => format!("({trimmed})"),
        (false, false) => format!("({trimmed}).clone()"),
    }
}

/// Renders `text` as a shared borrow of the value it evaluates to.
///
/// The counterpart to [`cloned_value_text`] for a callee that reads a value
/// through `&T` instead of taking it by value: a postfix chain keeps the plain
/// `&value` / `&value.field` spelling a hand-written port would use, while any
/// looser expression is parenthesised so the `&` cannot bind more tightly than
/// the caller meant. When the text already ends in a `.clone()` that clone is
/// dropped -- borrowing a value never needs an owned copy of it -- unless the
/// clone is the whole expression's tail on a non-postfix form, where removing
/// it could change what is borrowed, so it is kept and parenthesised instead.
///
/// Like [`cloned_value_text`] this is a syntax-directed peephole on text this
/// module already owns, not a per-construct special case.
pub(super) fn borrowed_value_text(text: &str) -> String {
    let trimmed = text.trim();
    if is_postfix_chain(trimmed) {
        // `a.clone()` and `a` borrow the same value, so the copy is dead weight
        // behind a `&`. Only strip it from a postfix chain, where the prefix is
        // itself a complete expression naming the same place.
        let borrowed = trimmed.strip_suffix(".clone()").unwrap_or(trimmed);
        if is_postfix_chain(borrowed) {
            return format!("&{borrowed}");
        }
        return format!("&{trimmed}");
    }
    format!("&({trimmed})")
}

/// Whether `text` is a primary expression followed only by field, method, and
/// index postfixes -- `a`, `a.b`, `f(x).g()`, `xs[0].y`.
///
/// Such an expression binds tighter than every Rust operator, so appending
/// `.clone()` to it, or splicing it into a larger expression, cannot change how
/// either parses. Anything else (a `match`, an `if`, a block, a binary
/// operation, a unary `*`, a turbofish path) is wrapped in parentheses instead.
///
/// The test is deliberately conservative and purely lexical: at bracket depth
/// zero the text must start with an identifier character and contain no
/// whitespace and no operator character. A false negative only costs a pair of
/// parentheses; a false positive would change the meaning of emitted code, so
/// every construct that is not obviously a postfix chain is rejected.
fn is_postfix_chain(text: &str) -> bool {
    // A string literal is a primary expression too, and static property keys
    // render as one (`"b".to_owned()`), so accept a leading literal and check
    // the postfixes that follow it.
    let rest = match strip_leading_string_literal(text) {
        Some(rest) => rest,
        None => match text.chars().next() {
            Some(first) if first.is_alphanumeric() || first == '_' => text,
            _ => return false,
        },
    };
    let mut depth = 0usize;
    for ch in rest.chars() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => match depth.checked_sub(1) {
                Some(next) => depth = next,
                // Unbalanced here means the text closes a group it never opened;
                // treat it as not a chain rather than guessing.
                None => return false,
            },
            _ if depth > 0 => {}
            ch if ch.is_whitespace() => return false,
            '+' | '-' | '*' | '/' | '%' | '&' | '|' | '^' | '!' | '<' | '>' | '=' | '?' | ','
            | ':' | ';' | '\'' | '"' | '#' | '@' => return false,
            _ => {}
        }
    }
    depth == 0
}

/// Strips a leading Rust string literal from `text`, returning what follows it.
///
/// Returns `None` when `text` does not start with a `"` or the literal is not
/// terminated. Escapes are honoured so a `\"` inside the literal does not end
/// it early.
fn strip_leading_string_literal(text: &str) -> Option<&str> {
    let body = text.strip_prefix('"')?;
    let mut escaped = false;
    for (offset, ch) in body.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some(&body[offset + 1..]);
        }
    }
    None
}
