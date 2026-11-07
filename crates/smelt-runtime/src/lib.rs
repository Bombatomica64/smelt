//! Runtime for transpiled code: the *source of truth* for the text
//! `smelt-codegen-rust` inlines into every generated crate.
//!
//! # Why the runtime lives here
//!
//! Runtime text kept as string literals is never type-checked, clippy'd or unit
//! tested on its own: the only way to learn whether a ten-line helper was correct
//! was to emit a whole crate and `cargo test` it.
//!
//! Everything here is ordinary, compiled, unit-tested Rust. Each piece is wrapped
//! in `// @smelt:item <name>` / `// @smelt:item-end` markers, and [`source`]
//! renders the marked text for the emitter under the program's flags, in the
//! order [`order::ORDER`] fixes.
//!
//! The `compiled` feature, on by default, builds that text as a module of this
//! crate, with the same dependencies a generated crate declares. The emitter
//! depends on this crate with the feature off: it needs the text, not the code.
//!
//! # What generated crates depend on
//!
//! Nothing. Generated crates stay self-contained: the selected items are
//! emitted inline into the generated crate root, exactly as before, and no
//! generated `Cargo.toml` gains a `smelt-runtime` dependency. Making this a
//! real dependency needs a stable generated ABI (`SmeltList` reference
//! semantics and byte-buffer views are still in flight), so it is deliberately
//! out of scope.
//!
//! # Invariants for anyone editing these modules
//!
//! * Marked text is emitted **verbatim**, so editing it changes the bytes of
//!   every generated crate. Formatting inside a marked region is part of the
//!   generated output.
//! * Generated crates carry `#![allow(dead_code, non_snake_case, unused_imports,
//!   unused_variables)]`, and every runtime item shares one flat module. Items may
//!   only reference other runtime items, `::std` and the crates their gate links,
//!   never `crate::`, `super::` or a helper defined outside a marked region.
//! * Text outside a marked region, and `//~` notes inside one, are never emitted,
//!   so a family file is free to carry whatever explains it.
//! * `//=` lines are the one thing here that is not compiled: the alternative
//!   text of an `else` branch, which the all-gates-on configuration cannot hold.

pub mod order;
pub mod source;

/// The runtime as compiled, tested Rust: every family in one flat module, which
/// is the shape a generated crate has. Family files are `include!`d rather than
/// declared as modules so that their items can name each other without a `use`
/// statement the emitted text does not have.
///
/// This is the configuration with every gate on. A generated crate carries a
/// selection of the same text.
///
/// The lints below are the ones a generated crate's text cannot satisfy without
/// changing the bytes of every generated crate; see `Cargo.toml`.
#[cfg(feature = "compiled")]
#[allow(
    unsafe_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_doc_comments,
    missing_docs,
    unreachable_pub,
    clippy::possible_missing_else,
    clippy::redundant_closure,
    clippy::unnecessary_to_owned,
    clippy::let_unit_value,
    clippy::incompatible_msrv,
    clippy::useless_conversion,
    clippy::redundant_static_lifetimes,
    clippy::needless_lifetimes,
    clippy::needless_range_loop,
    clippy::too_many_arguments,
    clippy::needless_return,
    clippy::explicit_auto_deref,
    clippy::wrong_self_convention,
    clippy::manual_range_contains,
    clippy::exit,
    clippy::string_add,
    clippy::io_other_error,
    clippy::missing_const_for_thread_local,
    clippy::needless_borrow,
    clippy::derivable_impls,
    reason = "generated-crate text, held verbatim: each of these wants a rewrite that changes the bytes of every generated crate"
)]
mod runtime {
    include!("runtime/abort.rs");
    include!("runtime/accessor.rs");
    include!("runtime/array.rs");
    include!("runtime/bigint.rs");
    include!("runtime/blob.rs");
    include!("runtime/blob_record.rs");
    include!("runtime/boxing.rs");
    include!("runtime/builtin_member.rs");
    include!("runtime/byte_buffer.rs");
    include!("runtime/callable_registry.rs");
    include!("runtime/captures.rs");
    include!("runtime/clock.rs");
    include!("runtime/console_inspect.rs");
    include!("runtime/conversions.rs");
    include!("runtime/crypto.rs");
    include!("runtime/date.rs");
    include!("runtime/erased_function.rs");
    include!("runtime/erased_method.rs");
    include!("runtime/event_emitter.rs");
    include!("runtime/fetch_types.rs");
    include!("runtime/field_store.rs");
    include!("runtime/for_in.rs");
    include!("runtime/form_data.rs");
    include!("runtime/function_object.rs");
    include!("runtime/generator.rs");
    include!("runtime/host_method.rs");
    include!("runtime/host_module.rs");
    include!("runtime/host_override.rs");
    include!("runtime/host_value_erasure.rs");
    include!("runtime/http_server.rs");
    include!("runtime/identity.rs");
    include!("runtime/js_map.rs");
    include!("runtime/js_set.rs");
    include!("runtime/json_indent.rs");
    include!("runtime/json_serde.rs");
    include!("runtime/key_eq.rs");
    include!("runtime/list.rs");
    include!("runtime/math.rs");
    include!("runtime/number_format.rs");
    include!("runtime/object.rs");
    include!("runtime/object_model.rs");
    include!("runtime/prim_set.rs");
    include!("runtime/promise.rs");
    include!("runtime/property_access.rs");
    include!("runtime/record.rs");
    include!("runtime/reflection.rs");
    include!("runtime/py_repr.rs");
    include!("runtime/regexp.rs");
    include!("runtime/regexp_match.rs");
    include!("runtime/sqlite.rs");
    include!("runtime/string_to_number.rs");
    include!("runtime/string_units.rs");
    include!("runtime/strings.rs");
    include!("runtime/structural_eq.rs");
    include!("runtime/symbol_keys.rs");
    include!("runtime/text_codec.rs");
    include!("runtime/this_channel.rs");
    include!("runtime/thrown.rs");
    include!("runtime/timers.rs");
    include!("runtime/to_string_tag.rs");
    include!("runtime/typed_array.rs");
    include!("runtime/unknown.rs");
    include!("runtime/uri.rs");
    include!("runtime/url.rs");
    include!("runtime/value.rs");
    include!("runtime/view_write_through.rs");
    include!("runtime/vitest_mock.rs");

    /// Behaviour tests, inside the flat module so private items are in scope.
    #[cfg(test)]
    mod family_tests {
        use super::*;

        include!("runtime_tests.rs");
    }
}
