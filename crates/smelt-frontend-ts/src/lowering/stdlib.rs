//! The JavaScript standard library, one module per family. `call_dispatch` routes a
//! call to its family; `callee_forms`, `item_call` and `member_call` lower the calls
//! no family claims.

mod abort_signal;
mod array_construction;
mod blob;
mod buffer;
pub(in crate::lowering) mod call_dispatch;
mod callee_forms;
mod collections;
mod crypto;
mod date;
mod event_emitter;
mod fetch_types;
mod form_data;
mod function_bind;
mod generic_call_inference;
mod global_functions;
mod host_modules;
mod http_server;
mod immediately_invoked;
mod item_call;
pub(in crate::lowering) mod json;
mod lists;
mod local_callable;
mod math;
mod member_call;
mod numbers;
mod object_assign;
mod objects;
mod overload_resolution;
mod primitive_casts;
mod promise;
mod regexp;
mod sqlite;
mod strings;
pub(in crate::lowering) mod symbols;
mod text_codec;
mod timers;
mod typed_array;
mod url;

use crate::lowering::erasure::NotNewErasure;
use smelt_hir::{ErasureReason, Type};

use super::ModuleBuilder;

impl ModuleBuilder<'_> {
    /// The erased type for a result derived from `input`, recording `reason`
    /// only when the erasure is new.
    ///
    /// When `input` is itself exactly `Type::Unknown`, the result is unknown
    /// purely by propagation — the input's erasure was recorded where it was
    /// decided — so nothing is recorded. Any other input (a type parameter, a
    /// class, a union) carried a static shape that this site drops, so the
    /// decision is recorded for `reason` at the current anchor span. Either way
    /// the returned id is the same interned `Type::Unknown`.
    pub(in crate::lowering) fn erase_unless_unknown(
        &mut self,
        input: smelt_hir::TypeId,
        reason: ErasureReason,
    ) -> smelt_hir::TypeId {
        if self.ctx.krate.types.get(input) == Some(&Type::Unknown) {
            self.unknown_type(NotNewErasure::ErasedUpstream)
        } else {
            self.erase(reason)
        }
    }
}
