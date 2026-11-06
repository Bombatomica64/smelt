//! Typed "rendered value" abstraction for the Rust codegen emitter.
//!
//! A [`RustExpr`] knows how its text binds; a [`RenderedValue`] adds the MIR type
//! it evaluates to, which is what the coercion seam needs. A value crossing the
//! seam carries both, so no verb re-derives the type or guesses at parentheses.

use smelt_hir::TypeId;

use crate::rust::{Formed, Precedence, RustExpr};

/// A rendered Rust expression bundled with its MIR type.
#[derive(Clone, Debug)]
pub(super) struct RenderedValue {
    /// The rendered expression.
    expr: RustExpr,
    /// The MIR type the rendered expression evaluates to.
    ty: TypeId,
}

impl RenderedValue {
    /// `expr`, known to evaluate to `ty`.
    pub(super) fn new(expr: RustExpr, ty: TypeId) -> Self {
        Self { expr, ty }
    }

    /// The MIR type this rendered expression evaluates to.
    pub(super) fn ty(&self) -> TypeId {
        self.ty
    }

    /// The rendered expression.
    pub(super) fn expr(&self) -> &RustExpr {
        &self.expr
    }

    /// The text, safe to drop into a tighter-binding position.
    #[allow(dead_code)]
    pub(super) fn parenthesized_if_needed(&self) -> String {
        self.expr.parenthesized_if_needed()
    }
}

impl Formed for RenderedValue {
    fn precedence(&self) -> Precedence {
        self.expr.binding()
    }
}

#[cfg(test)]
mod tests {
    use super::RenderedValue;
    use crate::rust::RustExpr;
    use smelt_hir::TypeId;

    /// A throwaway type id; `RenderedValue` never interprets it in these tests.
    const TY: TypeId = TypeId(0);

    /// A chain is inlined verbatim in every position.
    #[test]
    fn a_chain_is_never_parenthesized() {
        let value = RenderedValue::new(RustExpr::chain("smelt_value.to_lowercase()"), TY);
        assert_eq!(value.parenthesized_if_needed(), "smelt_value.to_lowercase()");
    }

    /// A loose operand must be wrapped before it is dropped into a tighter
    /// position: `SmeltUnknown::Number(a + b as f64)` parses as `a + (b as f64)`.
    #[test]
    fn a_loose_value_is_wrapped_for_tight_contexts() {
        let value = RenderedValue::new(RustExpr::loose("a + b"), TY);
        assert_eq!(value.parenthesized_if_needed(), "(a + b)");
        assert_eq!(value.expr().as_str(), "a + b");
    }

    /// The cast operand shape the `erase` Int/Float arm builds.
    #[test]
    fn cast_operand_wrapping_matches_erase_number_arm() {
        let chain = RenderedValue::new(RustExpr::path("value"), TY);
        assert_eq!(
            format!("SmeltUnknown::Number({} as f64)", chain.parenthesized_if_needed()),
            "SmeltUnknown::Number(value as f64)"
        );
        let loose = RenderedValue::new(RustExpr::loose("a + b"), TY);
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
/// in a `.clone()` -- it came from `operand_expr`/`local_value`, which clone
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
#[track_caller]
pub(super) fn cloned_value_text<T: crate::rust::Formed + AsRef<str> + ?Sized>(value: &T) -> crate::rust::RustExpr {
    let expr = shadow_expr(value);
    let lexical = cloned_value_lexical(value.as_ref());
    shadow_note("clone", &value.origin(), value.precedence(), value.as_ref(), &lexical, expr.as_ref().map(|e| e.cloned_value().into_string()));
    crate::rust::RustExpr::opaque(lexical)
}

/// TEMPORARY migration audit: the operand of an `as` cast, as spelled today.
#[track_caller]
pub(super) fn cast_operand_text(value: &RustExpr) -> String {
    let lexical = value.as_str().to_owned();
    let structured = match value.binding() {
        crate::rust::Precedence::Unstated => None,
        crate::rust::Precedence::Loose => Some(format!("({lexical})")),
        _ => Some(lexical.clone()),
    };
    shadow_note("cast", &crate::rust::Formed::origin(value), value.binding(), value.as_str(), &lexical, structured);
    lexical
}

/// TEMPORARY migration audit: rebuilds the expression the structured path sees.
fn shadow_expr<T: crate::rust::Formed + AsRef<str> + ?Sized>(value: &T) -> Option<crate::rust::RustExpr> {
    use crate::rust::{Precedence, RustExpr};
    let text = value.as_ref();
    let clone_tail = text.ends_with(".clone()");
    let expr = match value.precedence() {
        Precedence::Unstated => return None,
        Precedence::Path => RustExpr::path(text),
        Precedence::Chain => RustExpr::chain(text),
        Precedence::Atom => RustExpr::atom(text),
        Precedence::Cast => RustExpr::cast(text),
        Precedence::Loose => RustExpr::loose(text),
    };
    Some(if clone_tail { expr.ending_in_clone() } else { expr })
}

/// TEMPORARY migration audit: logs a call site whose structure disagrees.
#[track_caller]
fn shadow_note(kind: &str, origin: &str, stated: crate::rust::Precedence, text: &str, lexical: &str, structured: Option<String>) {
    use std::io::Write;
    let Ok(path) = std::env::var("SMELT_SHADOW") else { return };
    let verdict = match structured {
        None => "U",
        Some(s) if s == lexical => return,
        Some(_) => "M",
    };
    let at = std::panic::Location::caller();
    let short: String = text.chars().take(90).collect::<String>().replace('\n', " ");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{verdict} {kind} {}:{} {origin} {stated:?} {short}", at.file(), at.line());
    }
}

fn cloned_value_lexical(text: &str) -> String {
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
#[track_caller]
pub(super) fn borrowed_value_text<T: crate::rust::Formed + AsRef<str> + ?Sized>(value: &T) -> crate::rust::RustExpr {
    let expr = shadow_expr(value);
    let lexical = borrowed_value_lexical(value.as_ref());
    shadow_note("borrow", &value.origin(), value.precedence(), value.as_ref(), &lexical, expr.as_ref().map(|e| e.borrowed_value().into_string()));
    crate::rust::RustExpr::opaque(lexical)
}

fn borrowed_value_lexical(text: &str) -> String {
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
