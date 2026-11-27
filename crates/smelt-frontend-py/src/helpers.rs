//! Helper utilities for Python frontend diagnostics and AST inspection.
#![allow(
    clippy::redundant_pub_crate,
    reason = "helpers are shared by sibling modules after the crate split"
)]

use ruff_python_ast::{Decorator, Expr, Operator, Stmt};
use ruff_text_size::TextRange;
use smelt_hir::{FileId, Span};

use crate::SmeltError;

/// Convert a Ruff text range into a `smelt_hir::Span` for the given file ID.
pub(crate) fn range_to_span(file_id: FileId, range: TextRange) -> Span {
    Span::new(file_id, range.start().to_u32(), range.end().to_u32())
}

/// Extract the type name from `Expr::Name` or the attribute from
/// `Expr::Attribute` (e.g. `typing.Optional` → `"Optional"`).
pub(crate) fn expr_type_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Name(n) => Some(n.id.as_str()),
        Expr::Attribute(a) => Some(a.attr.as_str()),
        _ => None,
    }
}

/// Flatten a `T | U | V` tree into a flat list of parts.
pub(crate) fn collect_bitor_parts<'a>(expr: &'a Expr, parts: &mut Vec<&'a Expr>) {
    if let Expr::BinOp(b) = expr {
        if b.op == Operator::BitOr {
            collect_bitor_parts(&b.left, parts);
            collect_bitor_parts(&b.right, parts);
            return;
        }
    }
    parts.push(expr);
}

/// Expect exactly two type arguments from a subscript slice (e.g. `dict[K, V]`).
pub(crate) fn two_type_args(slice: &Expr, span: Span) -> Result<(&Expr, &Expr), SmeltError> {
    if let Expr::Tuple(t) = slice {
        if t.elts.len() == 2 {
            return Ok((&t.elts[0], &t.elts[1]));
        }
    }
    Err(SmeltError::unsupported(
        span,
        "expected exactly two type arguments",
    ))
}

/// Return a short name for a statement kind (for error messages).
pub(crate) fn stmt_kind_name(stmt: &Stmt) -> &'static str {
    match stmt {
        Stmt::FunctionDef(_) => "def",
        Stmt::ClassDef(_) => "class",
        Stmt::Return(_) => "return",
        Stmt::Delete(_) => "del",
        Stmt::Assign(_) => "assign",
        Stmt::AugAssign(_) => "augmented assign",
        Stmt::AnnAssign(_) => "annotated assign",
        Stmt::TypeAlias(_) => "type alias",
        Stmt::For(_) => "for",
        Stmt::While(_) => "while",
        Stmt::If(_) => "if",
        Stmt::With(_) => "with",
        Stmt::Match(_) => "match",
        Stmt::Raise(_) => "raise",
        Stmt::Try(_) => "try",
        Stmt::Assert(_) => "assert",
        Stmt::Import(_) => "import",
        Stmt::ImportFrom(_) => "from import",
        Stmt::Global(_) => "global",
        Stmt::Nonlocal(_) => "nonlocal",
        Stmt::Expr(_) => "expr",
        Stmt::Pass(_) => "pass",
        Stmt::Break(_) => "break",
        Stmt::Continue(_) => "continue",
        Stmt::IpyEscapeCommand(_) => "ipy escape",
    }
}

/// Return a short name for an expression kind (for error messages).
pub(crate) fn expr_kind_name(expr: &Expr) -> &'static str {
    match expr {
        Expr::BoolOp(_) => "bool op",
        Expr::Named(_) => "walrus operator",
        Expr::BinOp(_) => "bin op",
        Expr::UnaryOp(_) => "unary op",
        Expr::Lambda(_) => "lambda",
        Expr::If(_) => "ternary if",
        Expr::Dict(_) => "dict",
        Expr::Set(_) => "set",
        Expr::ListComp(_) => "list comprehension",
        Expr::SetComp(_) => "set comprehension",
        Expr::DictComp(_) => "dict comprehension",
        Expr::Generator(_) => "generator",
        Expr::Await(_) => "await",
        Expr::Yield(_) => "yield",
        Expr::YieldFrom(_) => "yield from",
        Expr::Compare(_) => "compare",
        Expr::Call(_) => "call",
        Expr::FString(_) => "f-string",
        Expr::TString(_) => "t-string",
        Expr::StringLiteral(_) => "string",
        Expr::BytesLiteral(_) => "bytes",
        Expr::NumberLiteral(_) => "number",
        Expr::BooleanLiteral(_) => "bool",
        Expr::NoneLiteral(_) => "None",
        Expr::EllipsisLiteral(_) => "ellipsis",
        Expr::Attribute(_) => "attribute",
        Expr::Subscript(_) => "subscript",
        Expr::Starred(_) => "starred",
        Expr::Name(_) => "name",
        Expr::List(_) => "list",
        Expr::Tuple(_) => "tuple",
        Expr::Slice(_) => "slice",
        Expr::IpyEscapeCommand(_) => "ipy escape",
    }
}

/// Extract the decorator name from `@name`, `@module.name`, or `@name(...)`.
///
/// Returns `None` for anything more complex.
pub(crate) fn decorator_simple_name(dec: &Decorator) -> Option<&str> {
    /// Return the simple name of a decorator expression, recursing through calls.
    fn inner(expr: &Expr) -> Option<&str> {
        match expr {
            Expr::Name(n) => Some(n.id.as_str()),
            Expr::Attribute(a) => Some(a.attr.as_str()),
            Expr::Call(c) => inner(&c.func),
            _ => None,
        }
    }
    inner(&dec.expression)
}

/// If the decorator is a call like `@dataclass(frozen=True)`, extract the
/// `frozen` kwarg value.  Returns `false` if absent or not a bool literal.
pub(crate) fn decorator_frozen_kwarg(dec: &Decorator) -> bool {
    if let Expr::Call(call) = &dec.expression {
        for kw in call.arguments.keywords.iter() {
            if kw.arg.as_ref().map(|a| a.as_str()) == Some("frozen") {
                if let Expr::BooleanLiteral(b) = &kw.value {
                    return b.value;
                }
            }
        }
    }
    false
}

/// Spell a `Name` / `Attribute` chain as written (`abc.ABC`), for diagnostics.
///
/// Returns `None` for any other expression shape.
pub(crate) fn dotted_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Name(name) => Some(name.id.to_string()),
        Expr::Attribute(attribute) => Some(format!(
            "{}.{}",
            dotted_name(&attribute.value)?,
            attribute.attr
        )),
        _ => None,
    }
}

/// If `expr` is a simple `Name`, return the name string.
pub(crate) fn expr_simple_name(expr: &Expr) -> Option<&str> {
    if let Expr::Name(n) = expr {
        Some(n.id.as_str())
    } else {
        None
    }
}

/// Whether a function body is a stub: only `...`, `pass` and docstrings.
///
/// Protocol and overload members are declared with such bodies; anything else
/// is a real implementation.
pub(crate) fn is_stub_body(body: &[Stmt]) -> bool {
    body.iter().all(|stmt| match stmt {
        Stmt::Pass(_) => true,
        Stmt::Expr(expr) => matches!(
            expr.value.as_ref(),
            Expr::StringLiteral(_) | Expr::EllipsisLiteral(_)
        ),
        _ => false,
    })
}

/// The canonical decimal digits of a Python integer literal's source text.
///
/// The parser keeps a literal too wide for a machine word as the token it read
/// (`0xFFFF_FFFF_FFFF_FFFF_FF`, `100_000_000_000_000_000_000`), so this strips
/// the `_` separators, reads the `0x`/`0o`/`0b` prefix, and converts the digits
/// to base 10 by schoolbook multiply-and-add over base-10^9 limbs. `None` when
/// the text is not an integer literal.
pub(crate) fn python_int_literal_digits(token: &str) -> Option<String> {
    const LIMB: u64 = 1_000_000_000;
    let text = token.replace('_', "");
    let lower = text.to_ascii_lowercase();
    let (radix, digits) = match lower.get(..2) {
        Some("0x") => (16, &lower[2..]),
        Some("0o") => (8, &lower[2..]),
        Some("0b") => (2, &lower[2..]),
        _ => (10, lower.as_str()),
    };
    if digits.is_empty() {
        return None;
    }
    // Little-endian base-10^9 limbs of the value read so far.
    let mut limbs: Vec<u64> = vec![0];
    for ch in digits.chars() {
        let mut carry = u64::from(ch.to_digit(radix)?);
        for limb in &mut limbs {
            // `limb < 10^9`, `radix <= 16` and `carry < 16 * 10^9`: no overflow.
            let next = limb.checked_mul(u64::from(radix))?.checked_add(carry)?;
            *limb = next.rem_euclid(LIMB);
            carry = next.div_euclid(LIMB);
        }
        while carry > 0 {
            limbs.push(carry.rem_euclid(LIMB));
            carry = carry.div_euclid(LIMB);
        }
    }
    let mut ordered = limbs.iter().rev();
    let head = ordered.next().map_or_else(String::new, ToString::to_string);
    Some(ordered.fold(head, |mut out, limb| {
        use std::fmt::Write as _;
        // Writing to a `String` cannot fail.
        let _infallible = write!(out, "{limb:09}");
        out
    }))
}

#[cfg(test)]
mod literal_digit_tests {
    use super::python_int_literal_digits;

    /// Every spelling of a wide literal reads to the digits `python3` prints.
    #[test]
    fn wide_literals_read_in_every_radix() {
        assert_eq!(
            python_int_literal_digits("100_000_000_000_000_000_000").as_deref(),
            Some("100000000000000000000")
        );
        assert_eq!(
            python_int_literal_digits("0xFFFF_FFFF_FFFF_FFFF_FF").as_deref(),
            Some("4722366482869645213695")
        );
        assert_eq!(
            python_int_literal_digits("0o1777777777777777777777").as_deref(),
            Some("18446744073709551615")
        );
        assert_eq!(python_int_literal_digits("0b1").as_deref(), Some("1"));
        assert_eq!(
            python_int_literal_digits("18446744073709551615").as_deref(),
            Some("18446744073709551615")
        );
        assert_eq!(python_int_literal_digits("0x"), None);
        assert_eq!(python_int_literal_digits("12a"), None);
    }
}
