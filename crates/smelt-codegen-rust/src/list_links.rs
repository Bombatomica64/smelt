//! A typed list at an `unknown[]` slot is the same array there.
//!
//! # The defect this closes
//!
//! `unknown[]` is `SmeltList<SmeltUnknown>` and a `number[]` is
//! `SmeltList<f64>`: two buffers of two element types. The conversion between
//! them rebuilt the list element by element, a copy that kept only the id, so
//!
//! ```ts
//! function fill(xs: unknown[]) { xs[0] = 42; xs.push(7); }
//! const nums: number[] = [1, 2, 3];
//! fill(nums as unknown[]);   // Node: nums is [42, 2, 3, 7]; the copy: [1, 2, 3]
//! ```
//!
//! # The representation
//!
//! The typed list keeps its `Vec<E>`, and the `unknown[]` value keeps the
//! `Vec<SmeltUnknown>` every `unknown[]` operation already speaks. The
//! conversion LINKS the two (`SmeltList::into_linked_unknown_list`, runtime
//! item `smelt_list_sync`): the erased list takes the typed list's identity,
//! and every access to either buffer first pulls the other's elements across
//! when the other side was written since they were last in step. That is the
//! hand-written answer for "one array seen at two element types" when one of
//! the two is `Vec<SmeltUnknown>`: the typed side keeps direct, typed access,
//! and the dynamic side pays for the conversion, once per write on the other
//! side, not per access.
//!
//! # Requirements
//!
//! Behavior, for a typed list `t` and its `unknown[]` view `u`:
//!
//! - a write through `u` (index store, `push`, `splice`, `length`) is read
//!   through `t`, and a write through `t` is read through `u`, however long
//!   both are held;
//! - `u === t`, and a second conversion of `t` is the same `u`;
//! - a value `t`'s element type cannot hold, written through `u`, detaches
//!   the two (each keeps what it holds), as a live erased array detaches.
//!
//! Type preservation: `t` keeps its element type and its direct access; `u`
//! is the `unknown[]` the source declares, so no value is erased that the
//! source did not erase. A program without such a conversion emits exactly
//! what it emitted before: the link is a runtime item gated on
//! [`program_links_lists`] (`list_links`).

#![expect(
    clippy::redundant_pub_crate,
    reason = "link planning is shared by the runtime demand and the function emitter"
)]

use smelt_hir::{Type, TypeId};
use smelt_mir::Mir;

use crate::rust::RustExpr;

/// Whether `source` (a list's element type) is an element a link can carry
/// to an `unknown[]` slot whose element type is `target`: the slot's element
/// is `unknown` itself, and the source element is a concrete type (an erased
/// or generic element already shares the erased representation).
pub(crate) fn links_elements(mir: &Mir, source: TypeId, target: TypeId) -> bool {
    matches!(mir.types.get(target), Some(Type::Unknown))
        && !matches!(
            mir.types.get(source),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::None) | None
        )
}

/// Whether some value flow of the program hands a list of a concrete element
/// type to an `unknown[]` slot (a parameter, a binding, an assertion
/// `xs as unknown[]`, an element of another list): the runtime then carries
/// the link (`Need::ListLinks`).
pub(crate) fn program_links_lists(mir: &Mir) -> bool {
    let mut found = false;
    crate::classify::for_each_value_flow_into_parts(mir, |source, slot| {
        if let (Some(Type::List(from)), Some(Type::List(to))) =
            (mir.types.get(source), mir.types.get(slot))
            && links_elements(mir, *from, *to)
        {
            found = true;
        }
    });
    found
}

/// The linked conversion of the typed list `list` to `unknown[]`, with the
/// element erasure `element` (an expression of `smelt_value`).
pub(crate) fn linked_list_expr(list: &RustExpr, element: &RustExpr) -> RustExpr {
    RustExpr::atom(format!(
        "{{ let smelt_l: SmeltList<_> = {list}.into(); smelt_l.into_linked_unknown_list(|smelt_value| {element}) }}",
        list = list.tight(),
        element = element.delimited()
    ))
}
