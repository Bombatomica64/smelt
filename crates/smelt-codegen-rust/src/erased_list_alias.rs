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
//! and keep the id, not rebuild it element by element.
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
//! is where a value leaves that ABI, aliasing the list rather than rebuilding
//! it per element. The SmeltUnknown report (`smelt-transpiler`'s
//! `unknown_report::classify_line`) keys its legitimate-boundary classification
//! on [`ERASED_GENERIC_LIST_ALIAS_FN`] and [`ERASED_GENERIC_ARRAY_ALIAS_FN`].
//!
//! What is deliberately NOT routed through a helper, so the report counts it as
//! avoidable:
//!
//! - a source `unknown[]`, into `unknown` or into an erased `T[]` slot, because
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

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are called from the emitter shards, outside this private module"
)]

use crate::rust::RustExpr;
use smelt_hir::Type;


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
pub(crate) fn leaves_erased_generic(source_item: Option<&Type>) -> bool {
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
pub(crate) fn list_alias_expr(value: &RustExpr) -> RustExpr {
    RustExpr::atom(ERASED_GENERIC_LIST_ALIAS_FN).postfix(format!("({value})", value = value.delimited()))
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
pub(crate) fn array_alias_expr(value: &RustExpr) -> RustExpr {
    RustExpr::atom(format!("SmeltUnknown::Array({ERASED_GENERIC_ARRAY_ALIAS_FN}({value}))", value = value.delimited()))
}
