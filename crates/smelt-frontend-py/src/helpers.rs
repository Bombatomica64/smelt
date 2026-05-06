//! Helper utilities for Python frontend diagnostics and AST inspection.
#![allow(
    clippy::redundant_pub_crate,
    reason = "helpers are shared by sibling modules after the crate split"
)]

use ruff_python_ast::{Decorator, Expr, Operator, Stmt};
use ruff_text_size::TextRange;
use smelt_hir::{FileId, Span};

use crate::SmeltError;

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

/// Detect Django model base classes: `models.Model`, `django.db.models.Model`, etc.
pub(crate) fn is_django_model_base(expr: &Expr) -> bool {
    match expr {
        Expr::Attribute(a) => {
            if a.attr.as_str() == "Model" {
                return expr_chain_contains_name(&a.value, "models")
                    || expr_chain_contains_name(&a.value, "django");
            }
            false
        }
        _ => false,
    }
}

/// Return `true` if `expr` (an attribute chain) contains `name` anywhere.
fn expr_chain_contains_name(expr: &Expr, name: &str) -> bool {
    match expr {
        Expr::Name(n) => n.id.as_str() == name,
        Expr::Attribute(a) => a.attr.as_str() == name || expr_chain_contains_name(&a.value, name),
        _ => false,
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
