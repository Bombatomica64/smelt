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
#![expect(
    clippy::exhaustive_structs,
    reason = "frontend diagnostics and contexts are constructed directly by workspace callers"
)]
#![expect(
    clippy::arithmetic_side_effects,
    reason = "identifier conversion walks byte offsets derived from validated string boundaries"
)]
#![expect(
    clippy::shadow_reuse,
    reason = "identifier conversion rebinds neighboring-character options while normalizing names"
)]
#![expect(
    clippy::wildcard_enum_match_arm,
    reason = "Oxc AST fallback arms intentionally group unsupported syntax for diagnostics"
)]
#![expect(
    clippy::expect_used,
    reason = "tests use parser expectations to keep fixture setup compact"
)]
#![allow(
    clippy::match_same_arms,
    clippy::unnecessary_wraps,
    reason = "lowering helpers keep Result return types for call-site uniformity while diagnostics evolve"
)]

pub mod checker;

mod context;
mod error;
mod ident;
mod lowering;
#[doc(hidden)]
pub mod test_support;

pub use context::{
    HirCtx, ObjectConst, ObjectConstEntry, ObjectConstEntryValue, ObjectConstValue,
    OverloadSignature,
};
pub use error::SmeltError;
pub use ident::camel_to_snake;
pub use lowering::{
    ConstCollection, ConstLiteral, FrontendOptions, InterfaceHeritageRef, RestParam,
    predeclare_type_declarations_with_path, scan_written_host_globals, to_hir, to_hir_with_options,
    to_hir_with_path,
};

#[cfg(test)]
mod tests;
