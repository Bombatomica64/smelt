//! The type-parameter environment consumed by Rust type lowering.
//!
//! Every Rust type the emitter renders is rendered *somewhere*, and "somewhere"
//! decides what a `Type::TypeParam` may be spelled as. Before this module the
//! answer was carried around as a bare `&HashSet<Symbol>`, which conflates at
//! least four different meanings:
//!
//! * the type parameters the enclosing Rust item declares (a class `impl<T>`
//!   block, or a free function that emits real Rust generics);
//! * a deliberately empty set, meaning "erase every type parameter here";
//! * the type parameters a *callee* emits as real Rust generics, used as a
//!   render scope when comparing a caller-side rendering against a callee-side
//!   one;
//! * a lexical scope narrowed to a subset at one render position.
//!
//! Those are not interchangeable. [`smelt_hir::Symbol`] is globally
//! name-interned, so a caller's `T` and a callee's `T` are the *same* key:
//! rendering a callee's parameter type under the caller's scope does not merely
//! lose the binding, it silently captures the caller's unrelated `T`. Naming the
//! provenance is therefore load-bearing documentation, even though lowering
//! itself never branches on it.
//!
//! [`TypeSubstitution`] additionally carries the slot for the concrete callee
//! bindings collected at a call site (`crate::generic_bindings`). That slot is
//! deliberately unwired: it is `None` at every production call site today, so
//! this module is behaviour-preserving by construction. Attaching bindings is
//! the next increment of the callback-generics plan
//! (`blocker-logs/estk-callback-generics-plan.md`, §4.2), and it becomes a
//! change to *what substitution is passed in*, not to *who renders what*.

#![expect(
    clippy::redundant_pub_crate,
    reason = "the substitution environment is shared with sibling codegen modules"
)]

use crate::EmitError;
use crate::generic_bindings::{CalleeTypeParamBindings, TypeParamBinding};
use smelt_hir::{Symbol, TypeId};
use std::collections::HashSet;

/// Where the in-scope type-parameter set of a [`TypeSubstitution`] came from.
///
/// Lowering never inspects this. It exists so a reader of a call site can tell a
/// deliberate erasure from a lexical scope from a callee's emission scope, all
/// three of which are a `HashSet<Symbol>` and none of which mean the same thing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScopeOrigin {
    /// Nothing is spellable: every type parameter erases to `SmeltUnknown`.
    ///
    /// This is the state the callback-parameter renderers pass today, and the
    /// one the callback-generics plan changes in a later increment.
    DeliberatelyErased,
    /// The type parameters declared by the Rust item currently being emitted.
    Lexical,
    /// The type parameters the *callee* emits as real Rust generics.
    ///
    /// An emission scope, not a call-site binding: it says which names the
    /// callee's own signature spells, which is what a caller-vs-callee
    /// rendering comparison needs. `crate::generic_bindings` cannot express it
    /// and the two must not be conflated.
    CalleeEmission,
    /// A lexical scope narrowed to a subset at one render position.
    LexicalSubset,
}

/// How one type parameter lowers at one render position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Resolved {
    /// Spell it as a Rust identifier: the enclosing item declares it.
    Spelled(Symbol),
    /// Replace it with the concrete type the call site bound it to.
    ///
    /// Unreachable until call-site bindings are attached; see the module
    /// docstring.
    Substituted(TypeId),
    /// Deliberately erased: render the runtime unknown carrier.
    Erased,
}

/// The type-parameter environment for one Rust type render position.
///
/// Borrowed and `Copy`: constructing one allocates nothing, so threading it
/// through the lowering recursion costs exactly what threading a
/// `&HashSet<Symbol>` cost.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TypeSubstitution<'a> {
    /// Names spellable as Rust identifiers here. `None` is an empty scope.
    in_scope: Option<&'a HashSet<Symbol>>,
    /// Provenance of `in_scope`, for readers rather than for lowering.
    ///
    /// Never branched on: it exists so a reader of a construction site can see
    /// *whose* scope is meant. The derived `Debug` reads it, so it is not dead.
    origin: ScopeOrigin,
    /// Concrete instantiations collected at a call site.
    ///
    /// Always `None` today; see the module docstring.
    bindings: Option<&'a CalleeTypeParamBindings>,
}

impl<'a> TypeSubstitution<'a> {
    /// An empty environment: every type parameter renders `SmeltUnknown`.
    ///
    /// Use this wherever the code previously passed `&HashSet::new()`, and say
    /// in a comment at the call site *whose* scope the emptiness stands for.
    pub(crate) const fn erased() -> Self {
        Self {
            in_scope: None,
            origin: ScopeOrigin::DeliberatelyErased,
            bindings: None,
        }
    }

    /// The type parameters declared by the Rust item being emitted.
    pub(crate) const fn lexical(scope: &'a HashSet<Symbol>) -> Self {
        Self {
            in_scope: Some(scope),
            origin: ScopeOrigin::Lexical,
            bindings: None,
        }
    }

    /// The type parameters the callee emits as real Rust generics.
    ///
    /// See [`ScopeOrigin::CalleeEmission`]: this is an emission scope, not a
    /// call-site binding.
    pub(crate) const fn callee_emission(scope: &'a HashSet<Symbol>) -> Self {
        Self {
            in_scope: Some(scope),
            origin: ScopeOrigin::CalleeEmission,
            bindings: None,
        }
    }

    /// A lexical scope already narrowed to one render position's subset.
    pub(crate) const fn lexical_subset(scope: &'a HashSet<Symbol>) -> Self {
        Self {
            in_scope: Some(scope),
            origin: ScopeOrigin::LexicalSubset,
            bindings: None,
        }
    }

    /// Attach the concrete bindings collected at a call site.
    ///
    /// Intentionally has no production caller: the `expect` below is what proves
    /// this module lands inert. Wiring it up is a deliberate, separately
    /// validated increment, and removing the attribute rather than the reason
    /// for it would erase that proof.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the call-site binding seam lands inert; a later increment threads it"
        )
    )]
    pub(crate) const fn with_bindings(self, bindings: &'a CalleeTypeParamBindings) -> Self {
        Self {
            bindings: Some(bindings),
            ..self
        }
    }

    /// Drop the call-site bindings, keeping the lexical scope.
    ///
    /// A substituted type is a *caller-side* type: its own type parameters live
    /// in the caller's world, not the callee's, so recursing into one must not
    /// keep applying the callee's binding map.
    pub(crate) const fn without_bindings(self) -> Self {
        Self {
            bindings: None,
            ..self
        }
    }

    /// Return where this environment's in-scope set came from.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "provenance is documentation, not a lowering input")
    )]
    pub(crate) const fn origin(&self) -> ScopeOrigin {
        self.origin
    }

    /// Return whether `name` is spellable as a Rust identifier here.
    pub(crate) fn spells(&self, name: Symbol) -> bool {
        self.in_scope.is_some_and(|scope| scope.contains(&name))
    }

    /// Decide how one type parameter lowers at this render position.
    ///
    /// The lexical scope wins over any binding: a `T` that the emitted item
    /// itself declares must keep being spelled `T`, even if a call site could
    /// also pin it.
    ///
    /// `Erased` is a decision, not a fallthrough. Without call-site bindings a
    /// callee type parameter has no concrete instantiation to spell here, and
    /// the runtime unknown carrier is the correct rendering for that erased
    /// interop boundary (CLAUDE.md, "SmeltUnknown boundaries"). This introduces
    /// no new erasure: it is exactly the answer the previous
    /// `scoped_type_params.contains(name)` test produced.
    ///
    /// Once bindings are attached, a parameter the callee declares but that is
    /// absent from the binding map is an invariant violation — the binding
    /// matcher and the renderer disagree about the callee's signature — and
    /// returns `Err` rather than silently erasing. That arm cannot fire while
    /// `bindings` is `None`, which is why it is safe to write it now.
    pub(crate) fn resolve(&self, name: Symbol) -> Result<Resolved, EmitError> {
        if self.spells(name) {
            return Ok(Resolved::Spelled(name));
        }
        let Some(bindings) = self.bindings else {
            return Ok(Resolved::Erased);
        };
        match bindings.get(name) {
            Some(TypeParamBinding::Concrete(ty)) => Ok(Resolved::Substituted(ty)),
            // `Unbound`, `Erased`, `Conflict` and `Unsupported` all mean the
            // matcher declined to pin the parameter; the generic-emission gate
            // demotes such callees, so erasure is the correct rendering.
            Some(_) => Ok(Resolved::Erased),
            None => Err(EmitError::new(
                "callee type parameter has no collected binding at this call site",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    //! Cover every `resolve` arm, including the ones no production call site can
    //! reach yet, so the unwired binding seam is still tested.

    use super::*;
    use crate::generic_bindings::{BindingUnsupportedReason, CalleeTypeParamBindings};
    use smelt_hir::SymbolInterner;

    /// Build a binding map for `names` with every entry left `Unbound`.
    fn bindings_for(names: &[Symbol]) -> CalleeTypeParamBindings {
        CalleeTypeParamBindings::unbound(names.iter().copied())
    }

    #[test]
    /// An empty environment erases every type parameter.
    fn erased_environment_erases() {
        let mut interner = SymbolInterner::default();
        let name = interner.intern("T");
        let substitution = TypeSubstitution::erased();
        assert!(!substitution.spells(name));
        assert_eq!(substitution.resolve(name).unwrap(), Resolved::Erased);
        assert_eq!(substitution.origin(), ScopeOrigin::DeliberatelyErased);
    }

    #[test]
    /// A lexical environment resolves exactly the membership test it replaced.
    fn lexical_environment_matches_membership() {
        let mut interner = SymbolInterner::default();
        let in_scope_name = interner.intern("T");
        let other = interner.intern("U");
        let scope: HashSet<Symbol> = [in_scope_name].into_iter().collect();
        let substitution = TypeSubstitution::lexical(&scope);
        assert_eq!(
            substitution.resolve(in_scope_name).unwrap(),
            Resolved::Spelled(in_scope_name)
        );
        assert_eq!(substitution.resolve(other).unwrap(), Resolved::Erased);
    }

    #[test]
    /// A concrete binding substitutes, and a lexical name still wins over it.
    fn concrete_binding_substitutes_but_lexical_wins() {
        let mut interner = SymbolInterner::default();
        let name = interner.intern("T");
        let mut bindings = bindings_for(&[name]);
        bindings.set_for_test(name, TypeParamBinding::Concrete(TypeId(7)));

        let empty = HashSet::new();
        let erased = TypeSubstitution::lexical(&empty).with_bindings(&bindings);
        assert_eq!(
            erased.resolve(name).unwrap(),
            Resolved::Substituted(TypeId(7))
        );

        let scope: HashSet<Symbol> = [name].into_iter().collect();
        let lexical = TypeSubstitution::lexical(&scope).with_bindings(&bindings);
        assert_eq!(lexical.resolve(name).unwrap(), Resolved::Spelled(name));
        assert_eq!(
            lexical.without_bindings().resolve(name).unwrap(),
            Resolved::Spelled(name)
        );
    }

    #[test]
    /// Every non-concrete binding state erases rather than substituting.
    fn weak_bindings_erase() {
        let mut interner = SymbolInterner::default();
        let name = interner.intern("T");
        let empty = HashSet::new();
        for state in [
            TypeParamBinding::Unbound,
            TypeParamBinding::Erased,
            TypeParamBinding::Conflict {
                first: TypeId(1),
                second: TypeId(2),
            },
            TypeParamBinding::Unsupported(BindingUnsupportedReason::Union),
        ] {
            let mut bindings = bindings_for(&[name]);
            bindings.set_for_test(name, state);
            let substitution = TypeSubstitution::lexical(&empty).with_bindings(&bindings);
            assert_eq!(substitution.resolve(name).unwrap(), Resolved::Erased);
        }
    }

    #[test]
    /// A name the binding map never declared is an invariant violation.
    fn undeclared_name_with_bindings_is_an_error() {
        let mut interner = SymbolInterner::default();
        let declared = interner.intern("T");
        let stranger = interner.intern("U");
        let bindings = bindings_for(&[declared]);
        let empty = HashSet::new();
        let substitution = TypeSubstitution::lexical(&empty).with_bindings(&bindings);
        assert!(substitution.resolve(stranger).is_err());
    }
}
