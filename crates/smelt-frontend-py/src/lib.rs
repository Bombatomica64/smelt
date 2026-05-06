//! Python frontend: source → Ruff AST → smelt HIR.
//!
//! Mirrors the structure of `smelt-frontend-ts`. Parsing is handled by
//! Astral's `ruff_python_parser`; lowering walks `ruff_python_ast` nodes and
//! produces nodes in `smelt-hir`.
//!
//! ## Design notes
//! * Type annotations in Python are `Expr` nodes in annotation position (not a
//!   separate grammar), so `annotation_to_hir` pattern-matches on `Expr` shape.
//! * `print(...)` is mapped to the same `CONSOLE_LOG_SYMBOL` item as TS's
//!   `console.log(...)` — both compile down to `println!` in codegen.
//! * Strict annotation policy mirrors TS: function params and return types must
//!   have explicit type hints; new local variables require annotated assignment
//!   (`x: int = 5`), bare assignment (`x = 5`) is only allowed to an already-
//!   declared local.

#![expect(
    clippy::many_single_char_names,
    reason = "Ruff AST pattern matches often use source-language single-letter names"
)]
#![expect(
    clippy::unnested_or_patterns,
    reason = "Python type-name matching is kept compact for readability"
)]
#![expect(
    clippy::unnecessary_raw_string_hashes,
    reason = "multiline Python fixtures use raw strings consistently"
)]
#![expect(
    clippy::needless_raw_string_hashes,
    reason = "multiline Python fixtures use raw strings consistently"
)]
#![expect(
    clippy::ignored_unit_patterns,
    reason = "parse handling mirrors the upstream API shape"
)]
#![expect(
    clippy::too_many_lines,
    reason = "Python lowering is still organized around large AST match functions"
)]
#![expect(
    clippy::explicit_iter_loop,
    reason = "Ruff AST containers are iterated explicitly for clarity"
)]
#![expect(
    clippy::redundant_closure_for_method_calls,
    reason = "identifier conversion closures document the projected value"
)]
#![expect(
    clippy::unnecessary_wraps,
    reason = "lowering helpers keep Result signatures to match adjacent fallible helpers"
)]
#![expect(
    clippy::manual_contains,
    reason = "type membership checks are kept uniform with other iterator predicates"
)]
#![expect(
    clippy::match_same_arms,
    reason = "separate Python AST variants are kept visible in match statements"
)]
#![expect(
    clippy::single_match,
    reason = "AST lowering uses match structure ready for nearby variants"
)]
#![expect(
    clippy::single_match_else,
    reason = "AST fallback branches read more clearly as explicit nested matches"
)]
#![expect(
    clippy::missing_const_for_fn,
    reason = "utility const qualification will be handled after behavior cleanup"
)]
#![expect(
    clippy::type_complexity,
    reason = "Ruff AST type shapes will be wrapped by local aliases in a later cleanup"
)]

pub use ruff_python_ast as ast;

use helpers::range_to_span;
use lowering::ModuleBuilder;
use ruff_python_ast::{Mod, ModModule};
use ruff_python_parser::{Mode, ParseOptions, parse};
use smelt_hir::{Crate as HirCrate, FileId, ModuleId, Span};

// ---------------------------------------------------------------------------
// Public API — kept in lockstep with smelt-frontend-ts.
// ---------------------------------------------------------------------------

/// Diagnostic produced by parsing or lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmeltError {
    /// Stable diagnostic code.
    pub code: &'static str,
    /// Source range the diagnostic refers to.
    pub span: Span,
    /// Human-readable message.
    pub message: String,
    /// Optional secondary note.
    pub note: Option<String>,
}

impl SmeltError {
    /// Create an "unsupported construct" error with the given message.
    pub(crate) fn unsupported(span: Span, message: impl Into<String>) -> Self {
        Self {
            code: "smelt::unsupported-py",
            span,
            message: message.into(),
            note: None,
        }
    }

    /// Create a parse error.
    fn parse(span: Span, message: impl Into<String>) -> Self {
        Self {
            code: "smelt::parse-error-py",
            span,
            message: message.into(),
            note: None,
        }
    }

    /// Error for metaclass usage, which is not supported.
    pub(crate) fn no_metaclass(span: Span, class_name: &str) -> Self {
        Self {
            code: "smelt::no-metaclass",
            span,
            message: format!("class '{class_name}': metaclasses are not supported"),
            note: Some(
                "smelt does not support runtime metaclass customisation. \
                 Refactor to use plain class inheritance or a decorator."
                    .to_owned(),
            ),
        }
    }

    /// Error for Django model inheritance, which is not supported.
    pub(crate) fn django_unsupported(span: Span, class_name: &str) -> Self {
        Self {
            code: "smelt::django-unsupported",
            span,
            message: format!(
                "class '{class_name}' inherits from a Django model — Django ORM is not supported"
            ),
            note: Some(
                "Django's Model metaclass and descriptor protocol cannot be expressed in smelt HIR."
                    .to_owned(),
            ),
        }
    }

    /// Error for multiple inheritance, which is not supported.
    pub(crate) fn no_multiple_inheritance(span: Span, class_name: &str) -> Self {
        Self {
            code: "smelt::no-multiple-inheritance",
            span,
            message: format!("class '{class_name}': multiple inheritance is not supported"),
            note: Some(
                "smelt only supports single-base class inheritance. \
                 Use composition or interfaces instead."
                    .to_owned(),
            ),
        }
    }

    /// Error for an unsupported class decorator.
    pub(crate) fn unsupported_decorator(span: Span, class_name: &str, decorator: &str) -> Self {
        Self {
            code: "smelt::unsupported-py",
            span,
            message: format!("class '{class_name}': decorator '@{decorator}' is not supported"),
            note: Some(
                "Only '@dataclass' (and 'dataclasses.dataclass') is allowed on classes.".to_owned(),
            ),
        }
    }
}

/// Reusable lowering context — one per crate, shared across files.
#[derive(Debug)]
pub struct HirCtx {
    /// The crate being assembled.
    pub krate: HirCrate,
}

impl HirCtx {
    /// Create an empty context.
    #[must_use]
    pub fn new() -> Self {
        Self {
            krate: HirCrate::new(),
        }
    }
}

impl Default for HirCtx {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse `source` and return the raw Ruff [`ModModule`] AST.
///
/// Useful for debugging or tooling that only needs the parse tree.
///
/// # Errors
/// Returns `Err` if the source has any syntax errors.
pub fn parse_module(source: &str, file_id: FileId) -> Result<ModModule, Vec<SmeltError>> {
    let parsed = parse(source, ParseOptions::from(Mode::Module)).map_err(|err| {
        vec![SmeltError::parse(
            range_to_span(file_id, err.location),
            err.to_string(),
        )]
    })?;

    if !parsed.errors().is_empty() {
        return Err(parsed
            .errors()
            .iter()
            .map(|err| SmeltError::parse(range_to_span(file_id, err.location), err.to_string()))
            .collect());
    }

    match parsed.into_syntax() {
        Mod::Module(m) => Ok(m),
        Mod::Expression(_) => Err(vec![SmeltError::parse(
            Span::new(file_id, 0, 0),
            "expected a module, got a bare expression",
        )]),
    }
}

/// Parse `source` as a Python module and lower it to HIR.
///
/// # Errors
/// Returns `Err` for parse errors or unsupported Python constructs.
pub fn to_hir(
    source: &str,
    file_id: FileId,
    ctx: &mut HirCtx,
) -> Result<ModuleId, Vec<SmeltError>> {
    to_hir_with_path(source, file_id, "", ctx)
}

/// Parse `source` from `path` as a Python module and lower it to HIR.
///
/// # Errors
/// Returns `Err` for parse errors or unsupported Python constructs.
pub fn to_hir_with_path(
    source: &str,
    file_id: FileId,
    path: &str,
    ctx: &mut HirCtx,
) -> Result<ModuleId, Vec<SmeltError>> {
    let module_ast = parse_module(source, file_id)?;
    let mut builder = ModuleBuilder::new(file_id, path.to_owned(), ctx);
    builder.module(&module_ast)
}

pub(crate) mod helpers;
pub(crate) mod lowering;

#[cfg(test)]
mod tests;
