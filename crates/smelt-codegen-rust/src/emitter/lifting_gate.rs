//! The crate's generic-lifting gate, asked through the [`EmitContext`].
//!
//! Whether a function's type parameter is emitted as a real Rust generic
//! depends on four per-crate tables the context owns: which callback
//! parameters a function owns, the bound-trait plan, which class type
//! parameters are carried, and the memo of call sites that pass an erased
//! argument. Every caller used to thread all four into
//! [`crate::generic_lifting`] by hand; these methods ask the question instead
//! and keep the tables private to the context.

use super::{EmitContext, EmitError};
use smelt_hir::Symbol;
use smelt_mir::{FuncId, LocalId, Mir, MirFunction};
use std::collections::HashSet;

impl EmitContext {
    /// The type parameters of `function` that lift to real Rust generics
    /// (see [`crate::generic_lifting::liftable_type_params`]).
    pub(crate) fn liftable_type_params(
        &self,
        mir: &Mir,
        function: &MirFunction,
    ) -> HashSet<Symbol> {
        crate::generic_lifting::liftable_type_params(
            mir,
            function,
            &self.owned_callback_params,
            &self.bound_traits,
            &self.type_param_elision,
            &self.erased_arguments,
        )
    }

    /// Whether `function` declares any real Rust generic of its own (see
    /// [`crate::generic_lifting::function_emits_rust_generics`]).
    pub(crate) fn function_emits_rust_generics(&self, mir: &Mir, function: &MirFunction) -> bool {
        crate::generic_lifting::function_emits_rust_generics(
            mir,
            function,
            &self.owned_callback_params,
            &self.bound_traits,
            &self.type_param_elision,
            &self.erased_arguments,
        )
    }

    /// The generated `F{n}` generic of each borrowed callback parameter of
    /// `function` (see [`crate::generic_lifting::callback_generic_params`]).
    pub(crate) fn callback_generic_params(
        &self,
        mir: &Mir,
        function: &MirFunction,
    ) -> Result<Vec<(LocalId, String)>, EmitError> {
        crate::generic_lifting::callback_generic_params(
            mir,
            function,
            &self.owned_callback_params,
            &self.bound_traits,
            &self.type_param_elision,
            &self.erased_arguments,
            &self.names,
        )
    }

    /// The `<..>` entries `function` declares for its lifted type parameters,
    /// each with the bounds a generated type parameter carries in this crate
    /// (see [`crate::generic_lifting::function_impl_generics_list`]).
    pub(crate) fn function_impl_generics_list(
        &self,
        mir: &Mir,
        function: &MirFunction,
    ) -> Result<Vec<String>, EmitError> {
        crate::generic_lifting::function_impl_generics_list(
            mir,
            function,
            &self.owned_callback_params,
            &self.bound_traits,
            &self.type_param_elision,
            &self.erased_arguments,
            crate::classes::generated_type_param_bounds(&self.demand),
        )
    }

    /// Whether `function` owns the callback held in its parameter `local`
    /// (the ownership fixpoint, `compute_owned_callback_params`).
    pub(crate) fn owns_callback_param(&self, function: FuncId, local: LocalId) -> bool {
        self.owned_callback_params.contains(&(function, local))
    }
}
