//! Handing an erased generic array to another erased slot without copying it.
//!
//! A `T[]` whose `T` is out of scope at the emitted item is rendered under the
//! erased-generic ABI: `T` becomes `SmeltUnknown`, so the array is stored as
//! `SmeltList<SmeltUnknown>`, exactly like a source `unknown[]`. When such an
//! array flows into a slot whose elements also render as `SmeltUnknown` (an
//! `unknown[]`, another erased `U[]`, or an `unknown` slot holding a
//! `SmeltUnknown::Array`), the two sides already share one representation:
//! `Rc<RefCell<Vec<SmeltUnknown>>>` plus the array's identity. JavaScript hands
//! over the SAME array object there, so the conversion must alias the buffer
//! and keep the id, not rebuild it element by element (#250).
//!
//! The two prelude helpers below are that conversion and nothing else. The
//! emitter calls them only where `FunctionEmitter::list_items_render_as_unknown`
//! holds for BOTH sides and the SOURCE element is an erased type parameter
//! ([`leaves_erased_generic`]; the call sites are in `emitter/coercion.rs`).
//! Their signatures pin `SmeltUnknown` elements, so a generated crate that
//! reached them with a concrete `SmeltList<f64>` or an in-scope `SmeltList<T>`
//! would not compile.
//!
//! # Why this is a `SmeltUnknown` boundary
//!
//! The helpers create no erasure: the erased-generic ABI erased `T` before the
//! helper is reached, because no Rust generic in scope can name it. The helper
//! is where a value leaves that ABI. Before #250 those conversions rebuilt the
//! array through `into_smelt_unknown()`, one call per element: the outbound
//! adapter for an erased `T`. The SmeltUnknown report (`smelt-transpiler`'s
//! `unknown_report::classify_line`) counted those lines as a legitimate
//! boundary through its `into_smelt_unknown` marker. #250 made them alias for
//! speed and the marker went with the rebuild, so the same lines moved to
//! avoidable with no erasure added. The report now keys on
//! [`ERASED_GENERIC_LIST_ALIAS_FN`] and [`ERASED_GENERIC_ARRAY_ALIAS_FN`]
//! instead, which restores that classification and no more.
//!
//! What is deliberately NOT routed through a helper, so the report keeps
//! counting it as avoidable exactly as it did before #250:
//!
//! - a source `unknown[]`, into `unknown` or into an erased `T[]` slot. The
//!   first aliased before #250 too, as `SmeltUnknown::Array(list.into())`; the
//!   second rebuilt through an identity map with no adapter. Neither line
//!   carried a boundary marker, and both keep their plain spelling, because
//!   the erasure is the source's own `unknown`, which the metric exists to see
//!   reduced.
//! - the general list conversion `Into::<SmeltList<_>>::into(..)`, which every
//!   list-typed rvalue goes through, including ones that erase a concrete
//!   value.
//!
//! Where the value can still be carried concretely, these helpers are not
//! reached: a concrete element list converts each element through its adapter
//! (`concrete_list_erasure_still_converts_elements`), and an in-scope generic
//! `SmeltList<T>` converts through `IntoSmeltUnknown`
//! (`erased_generic_list_argument_judges_each_side_at_its_own_scope`).

use smelt_hir::Type;
use smelt_mir::Mir;

use crate::rust::CodeWriter;

/// The prelude helper that hands an erased generic list to an erased-list slot.
///
/// The SmeltUnknown report keys its boundary classification on this name.
pub const ERASED_GENERIC_LIST_ALIAS_FN: &str = "smelt_alias_erased_generic_list";

/// The prelude helper that hands an erased generic list to an `unknown` slot.
///
/// The SmeltUnknown report keys its boundary classification on this name.
pub const ERASED_GENERIC_ARRAY_ALIAS_FN: &str = "smelt_alias_erased_generic_array";

/// Whether an already-erased list with this element type is leaving the
/// erased-generic ABI, and so converts through a named alias helper.
///
/// Call only once `FunctionEmitter::list_items_render_as_unknown` holds for
/// the source and the target, which makes a type-parameter element an
/// out-of-scope (erased) one. A `Type::Unknown` element is a source
/// `unknown[]` and keeps its plain spelling; see the module docs.
#[must_use]
pub fn leaves_erased_generic(source_item: Option<&Type>) -> bool {
    matches!(source_item, Some(Type::TypeParam { .. }))
}

/// Renders the alias of an erased generic list into an erased-list slot.
///
/// Call only where [`leaves_erased_generic`] holds for the source element.
/// `value` is any rendered expression whose type converts into
/// `SmeltList<SmeltUnknown>` (a `SmeltList<SmeltUnknown>` itself, by the
/// reflexive `From`, or a `Vec<SmeltUnknown>`). The result is the same list
/// handle: same id, same buffer.
#[must_use]
pub fn list_alias_text(value: &str) -> String {
    format!("{ERASED_GENERIC_LIST_ALIAS_FN}({value})")
}

/// Renders the erasure of an erased generic list into a `SmeltUnknown::Array`.
///
/// Call only where [`leaves_erased_generic`] holds for the list's element.
///
/// `value` is any rendered expression whose type converts into `SmeltArray`
/// (an owned or borrowed `SmeltList<SmeltUnknown>`, a `Vec<SmeltUnknown>`).
/// `From<SmeltList<SmeltUnknown>> for SmeltArray` shares the list's buffer and
/// id, so the erased value is the same array. The `SmeltUnknown::Array` tag
/// stays at the call site so the report still counts the occurrence.
#[must_use]
pub fn array_alias_text(value: &str) -> String {
    format!("SmeltUnknown::Array({ERASED_GENERIC_ARRAY_ALIAS_FN}({value}))")
}

/// Whether a program can reach either alias helper.
///
/// Both helpers need a list whose element is a type parameter, so a
/// `Type::List` of a `Type::TypeParam` in the type table is a cheap necessary
/// condition. It is not an exact one: a type parameter that is in scope
/// everywhere it appears renders as a Rust generic and never reaches a helper,
/// which then sits unused under the generated crate's `#![allow(dead_code)]`.
#[must_use]
pub fn needed(mir: &Mir) -> bool {
    mir.types.all().iter().any(|ty| {
        matches!(ty, Type::List(item)
            if matches!(mir.types.get(*item), Some(Type::TypeParam { .. })))
    })
}

/// Emits both alias helpers into the `SmeltUnknown` prelude.
///
/// Must follow the `From<SmeltList<SmeltUnknown>> for SmeltArray` impls, which
/// the array helper's conversion goes through. Each helper is one `.into()`
/// call marked `#[inline]`, so it compiles to the conversion #250 emitted
/// inline and keeps that change's speedup.
pub fn emit_prelude(writer: &mut CodeWriter) {
    writer.line(
        "/// Hand an erased generic list to an erased-list slot: the same array, never a copy.",
    );
    writer.line("#[inline]");
    writer.line(format!(
        "fn {ERASED_GENERIC_LIST_ALIAS_FN}(list: impl Into<SmeltList<SmeltUnknown>>) -> SmeltList<SmeltUnknown> {{ list.into() }}"
    ));
    writer.line("/// Hand an erased generic list to an `unknown` slot: the same array, never a copy.");
    writer.line("#[inline]");
    writer.line(format!(
        "fn {ERASED_GENERIC_ARRAY_ALIAS_FN}(list: impl Into<SmeltArray>) -> SmeltArray {{ list.into() }}"
    ));
}
