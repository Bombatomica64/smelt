//! Callback-heavy method lowering: array/set/string iteration and closures.
//!
//! This is the largest split of the lowering builder. It lowers the JavaScript
//! methods whose semantics are carried by callbacks or closures — array
//! iteration (`map`/`filter`/`reduce`/`find`/`sort`/`concat`), set and dict
//! projections, string transforms, and the closure-capture machinery that
//! turns a source arrow/function argument into an HIR [`CallbackExpr`]. The
//! helpers here classify receiver and argument surfaces, synthesize the
//! capture list, and emit the concrete list/set/string runtime call kinds.
//!
//! The implementation is split across focused submodules, each defining more
//! methods on the shared `ModuleBuilder` type:

mod body_lowering;
mod classify;
mod closures;
mod dispatch;
mod list_ops;
mod timer_wrapper;
mod transforms;

use crate::lowering::ModuleBuilder;
use smelt_hir::{ErasureReason, Type, TypeId};

impl ModuleBuilder<'_> {
    /// Erase a value derived from `source_ty` (a receiver or collection whose
    /// element/member type the callback lowering cannot read) for `reason`.
    ///
    /// When `source_ty` is itself already `unknown` — or a union with an
    /// `unknown` arm — the derived value is erased only because its input was,
    /// and that erasure was recorded where it was decided; nothing new is
    /// recorded then. Otherwise (a type parameter, class, or concrete union
    /// whose shape the callback surface does not model) the decision is new
    /// and is recorded at the current anchor with `detail`. Either way the
    /// returned id is the interned `Type::Unknown`.
    pub(in crate::lowering) fn callback_erase_unless_unknown(
        &mut self,
        source_ty: TypeId,
        reason: ErasureReason,
        detail: &str,
    ) -> TypeId {
        let already_erased = match self.ctx.krate.types.get(source_ty) {
            Some(Type::Unknown) => true,
            Some(Type::Union(items)) => items
                .iter()
                .any(|item| matches!(self.ctx.krate.types.get(*item), Some(Type::Unknown))),
            _ => false,
        };
        if already_erased {
            // not a new erasure: the source value is already erased upstream
            self.unknown_type()
        } else {
            self.erase_with(reason, detail)
        }
    }
}
