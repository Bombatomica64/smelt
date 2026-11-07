//! The generic trial: rendering a function once with its own type parameters
//! kept, to decide whether its Rust definition can stay generic.

use super::*;

impl<'mir> FunctionEmitter<'mir> {
    /// An emitter for a generic free function's body-cleanliness trial (see
    /// [`Self::renders_real_generics`] and [`emit_role::OwnGenerics::Trial`]).
    pub(crate) fn for_generic_trial(
        mir: &'mir Mir,
        context: &'mir EmitContext,
        function: &'mir MirFunction,
    ) -> Result<Self, EmitError> {
        let mut emitter = Self::new(mir, context, function)?;
        emitter.role = emit_role::EmitRole::Own {
            generics: emit_role::OwnGenerics::Trial,
        };
        Ok(emitter)
    }

    /// Apply the crate-wide generic decision ([`emit_role::crate_generic_decision`])
    /// to an emitter in the [`emit_role::EmitRole::Own`] role; other roles have
    /// no own generics to decide.
    pub(crate) fn apply_crate_generic_decision(&mut self) {
        if let emit_role::EmitRole::Own { generics } = &mut self.role {
            *generics = emit_role::crate_generic_decision(self.context, self.function);
        }
    }

    /// Force an own item's type parameters out of scope, whatever the crate
    /// decision says (the typedness measurement of closures with no owner).
    pub(crate) fn erase_own_generics(&mut self) {
        self.role = emit_role::EmitRole::Own {
            generics: emit_role::OwnGenerics::Erased,
        };
    }

    /// The free function currently on body-cleanliness trial: this emitter's
    /// own function in the trial role, or the one a closure inherited.
    pub(super) const fn trial_generic_function(&self) -> Option<FuncId> {
        match &self.role {
            emit_role::EmitRole::Own {
                generics: emit_role::OwnGenerics::Trial,
            } => Some(self.function.id),
            emit_role::EmitRole::Closure(scope) => scope.trial_function,
            _ => None,
        }
    }

    /// Return whether this generic free function's body keeps every type
    /// parameter opaque, so the function can emit real Rust generics.
    ///
    /// Trial-renders the body with the type parameters in scope and reads whether
    /// the render touched the erased carrier ([`EmitContext::note_carrier_use`]).
    /// A clean body (pure passthrough) touches nothing and keeps its generics; a
    /// body that inspects, compares, or erases a `T`-typed value falls back to
    /// full erasure. A thrown payload and a mutable-list adapter are real dynamic
    /// boundaries and do not count ([`EmitContext::carrier_boundary`]). The scratch
    /// declared-locals state is reset afterwards so the real emit renders from a
    /// clean slate. The caller only invokes this for signature-generic-safe
    /// functions, so the type parameters are in scope during the trial.
    pub(crate) fn renders_real_generics(&self) -> Result<bool, EmitError> {
        let mut body = String::new();
        // The crate-wide set grows from empty, so a self-recursive function
        // would never see itself as generic and its recursive call would erase
        // its own `T` (`mk(x.into_smelt_unknown())` inside `mk<T>`), failing
        // its own trial forever. The trial asks "does the body stay clean IF
        // this function is generic", so it assumes exactly that.
        // That assumption is this emitter's role (`OwnGenerics::Trial`, see
        // `FunctionEmitter::for_generic_trial`).
        //
        // The rendered text is never emitted, so everything the render mints into the shared context (accessors, hoisted helpers,
        // erasure records) is rolled back when the checkpoint drops.
        {
            let _trial = self.context.checkpoint();
            self.context.take_carrier_touched();
            self.emit_body(&mut body)?;
        }
        let touched_carrier = self.context.take_carrier_touched();
        self.reset_body_emission_state();
        // A partial lift is governed by `classes::type_param_only_moved`, which
        // asks MIR whether each *lifted* parameter is ever inspected. The carrier
        // record cannot be used there: the parameters that did not lift render as
        // `SmeltUnknown` by design, so they touch the carrier legitimately and
        // would demote the whole function, which is the all-or-nothing behaviour
        // per-parameter lifting exists to remove.
        let liftable = crate::generic_lifting::liftable_type_params(
            self.mir,
            self.function,
            &self.context.owned_callback_params,
            &self.context.bound_traits,
            &self.context.erased_arguments,
        );
        if liftable.len() < self.function.type_params.len() {
            return Ok(!liftable.is_empty());
        }
        Ok(!touched_carrier)
    }

    /// Whether a call to the free function `id` renders against its generic
    /// signature: the crate-wide decision, or the function currently on trial
    /// (see [`Self::trial_generic_function`]).
    pub(super) fn callee_emits_generics(&self, id: FuncId) -> bool {
        self.trial_generic_function() == Some(id) || self.context.is_generic_function(id)
    }
}
