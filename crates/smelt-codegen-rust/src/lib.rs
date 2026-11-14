//! Code generator for emitting Rust source code from the MIR.
//!
//! This crate provides functionality to transform a MIR (Middle Intermediate Representation)
//! into valid Rust source code, including struct definitions and function implementations.

#![expect(
    clippy::doc_markdown,
    reason = "codegen docs mention generated language tokens that are not fully marked up yet"
)]
#![expect(
    clippy::missing_errors_doc,
    reason = "public codegen entrypoint docs need a focused polish pass"
)]
#![expect(
    clippy::type_complexity,
    reason = "control-flow pattern recognizers currently return structured tuples"
)]
#![expect(
    clippy::format_push_string,
    reason = "Rust code emission is still written as direct string assembly"
)]
#![expect(
    clippy::too_many_arguments,
    reason = "structured codegen emitters pass explicit control-flow context"
)]
#![expect(
    clippy::too_many_lines,
    reason = "what stays long is one op table or one ordered rule list, which reads worse in pieces"
)]
#![expect(
    clippy::literal_string_with_formatting_args,
    reason = "generated Rust format strings intentionally contain braces"
)]
#![expect(
    clippy::wildcard_enum_match_arm,
    reason = "operand formatting groups future constants with existing constants"
)]
#![expect(
    clippy::unused_self,
    reason = "helper methods stay on the emitter for future shared state"
)]
#![expect(
    clippy::needless_pass_by_value,
    reason = "type lookup helpers accept owned HIR type values for call-site readability"
)]
#![expect(
    clippy::missing_const_for_fn,
    reason = "const qualification buys nothing here: emitter helpers only run at build time"
)]
#![expect(
    clippy::needless_pass_by_ref_mut,
    reason = "emitter entrypoints reserve mutability for incremental generation state"
)]
#![expect(
    clippy::manual_let_else,
    clippy::match_same_arms,
    clippy::needless_question_mark,
    clippy::option_if_let_else,
    clippy::unnested_or_patterns,
    clippy::unnecessary_wraps,
    reason = "emission keeps one explicit branch per source case, which combinator rewrites would hide"
)]
#![cfg_attr(
    test,
    expect(
        clippy::match_wild_err_arm,
        clippy::panic,
        reason = "codegen tests keep fixture setup compact and fail fast on invalid test inputs"
    )
)]

use smelt_mir::Mir;

mod accessor_prelude;
pub(crate) mod base_init;
mod bigint_prelude;
pub(crate) mod bound_traits;
pub(crate) mod class_proto;
pub(crate) mod classes;
pub(crate) mod classify;
mod console_inspect_prelude;
mod contract;
mod crate_builder;
mod crate_writer;
pub(crate) mod deps;
mod derive_impls;
mod emitter;
mod erased_list_alias;
mod error;
mod gates;
pub(crate) mod generic_bindings;
pub(crate) mod generic_elision;
pub(crate) mod generic_lifting;
pub(crate) mod globals;
pub(crate) mod lazy_view;
mod module_map;
pub(crate) mod names;
mod options;
mod program_types;
pub(crate) mod reference_origin;
mod reference_storage;
mod reflection_prelude;
pub(crate) mod runtime_demand;
pub(crate) mod runtime_prelude;
mod runtime_symbols_check;
pub mod rust;
pub(crate) mod startup;
pub(crate) mod stdlib;
pub(crate) mod thrown;
pub(crate) mod type_substitution;
mod typed_array_prelude;

pub use contract::{
    MODULE_SOURCE_HEADER, OPTIONAL_UNKNOWN_OR_UNDEFINED_FN, PRELUDE_END_MARKER,
    RECORD_ADAPTER_HELPER_PREFIX, RECORD_CONVERSION_HELPER_PREFIX,
};
pub use crate_builder::{emit_source, emit_source_with_allocator};
pub use crate_writer::{emit_crate, emit_crate_with_modules, emit_crate_with_modules_reporting};
pub use emitter::typedness::{KindCounts, Typedness, TypednessCounts, TypednessMeasurement};
/// Prelude helpers that alias an erased generic list into another erased slot.
///
/// The SmeltUnknown report classifies their call sites as a boundary; see
/// `erased_list_alias` for why.
pub use erased_list_alias::{ERASED_GENERIC_ARRAY_ALIAS_FN, ERASED_GENERIC_LIST_ALIAS_FN};
pub use error::EmitError;
pub(crate) use error::{compact_index, id_index};
pub use options::{
    CrateKind, EmitOptions, GeneratedAllocator, PackageMetadata, ReleaseProfile, RustEdition,
};

/// Measure how much of optimized MIR reaches Rust typed.
///
/// Counts values, property reads and calls, each concrete, generic or erased,
/// per source file. Uses the emitter's own type rendering so it matches the
/// generated crate; see `emitter::typedness`.
pub fn measure_typedness(mir: &Mir) -> Result<TypednessMeasurement, EmitError> {
    emitter::typedness::measure(mir)
}

/// Tests for the code generator.
#[cfg(test)]
mod tests;
