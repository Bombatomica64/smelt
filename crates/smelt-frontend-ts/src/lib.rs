//! TypeScript frontend for the Smelt compiler.
//!
//! This module provides parsing and lowering of TypeScript code into the Smelt HIR (High-level
//! Intermediate Representation). It handles type annotations, classes, interfaces, functions,
//! and various control flow constructs.

#![expect(
    clippy::too_many_lines,
    reason = "TypeScript lowering is still organized around large AST match functions"
)]
#![expect(
    clippy::too_many_arguments,
    reason = "class and function lowering currently pass explicit context instead of builder structs"
)]
#![expect(
    clippy::type_complexity,
    reason = "Oxc AST types are verbose and will be wrapped by local aliases in a later cleanup"
)]
#![expect(
    clippy::many_single_char_names,
    reason = "short names appear in generated TypeScript AST pattern matches"
)]
#![expect(
    clippy::cast_possible_truncation,
    reason = "HIR IDs are compact u32 indexes and overflow checks are being centralized incrementally"
)]
#![expect(
    clippy::single_match,
    reason = "declaration lowering keeps match structure ready for nearby variants"
)]
#![expect(
    clippy::doc_markdown,
    reason = "diagnostic docs mention source-language tokens without full rustdoc markup yet"
)]
#![expect(
    clippy::missing_const_for_fn,
    reason = "utility const qualification will be handled after behavior cleanup"
)]
#![expect(
    clippy::must_use_candidate,
    reason = "frontend helpers are mostly internal and will get must_use annotations in a focused pass"
)]
#![expect(
    clippy::missing_errors_doc,
    reason = "public checker docs need a dedicated polish pass"
)]

pub mod checker;

mod context;
mod error;
mod ident;
mod lowering;

pub use context::HirCtx;
pub use error::SmeltError;
pub use ident::camel_to_snake;
pub use lowering::{to_hir, to_hir_with_path};

#[cfg(test)]
mod tests;
