//! The type-parameter environment threaded through **value** rendering.
//!
//! [`crate::type_substitution::TypeSubstitution`] answers the question "how does
//! a `Type::TypeParam` lower *here*?" for **type** rendering: `rust_type` takes
//! one and every nested type position inherits it, so a class's `T` is spelled
//! `T` inside the `impl<T>` block that declares it and erased everywhere else.
//!
//! Value rendering had no such thing. `value_at_type_text` and
//! `extract_value_text` each re-derived the answer from the emitter's *ambient*
//! state at whatever depth they happened to be called, and they derived it
//! differently:
//!
//! * the coercion path erased a bare `Type::TypeParam` target unconditionally,
//!   at any depth;
//! * the recovery path respected the lexical scope, rebuilding the value as `T`
//!   where the emitted item declares one.
//!
//! A map-and-collect renders both: the entries come from one of the two paths
//! and the `collect::<..>()` turbofish from a third rendering of the same
//! position. When the three disagreed the generated Rust did not compile, and no
//! choice of rule for any ONE of them could satisfy the others — that is H42 in
//! `blocker-logs/hono-campaign-plan.md` (§D2), and
//! `blocker-logs/hono-h42-erasure-substitution.md` records the fix.
//!
//! [`RenderScope`] is what makes the answer a property of the *render position*
//! rather than of the call depth: the outermost value-render entry point builds
//! one, and every side of the position — annotation, entries, turbofish,
//! recovery — asks that same scope.
//!
//! # Why it owns its set rather than borrowing one
//!
//! `TypeSubstitution<'a>` borrows a `&'a HashSet<Symbol>` and is `Copy`, which
//! is right for type lowering: `rust_type` is always called from a context that
//! already holds the scope. Value rendering is reached from ~220 sites spread
//! across every emitter shard, and the ambient scope
//! (`FunctionEmitter::current_function_type_params`) is *computed*, not stored —
//! it depends on the hoisted-item flag and the generic-emission suppression
//! cell, both of which change during emission, so it cannot be cached and handed
//! out by reference. Owning the set lets a call site write `&self.render_scope()`
//! in argument position instead of a two-line dance with a temporary binding.
//! [`RenderScope::substitution`] lends the borrowed form whenever a type
//! actually has to be lowered.

use super::FunctionEmitter;
use crate::type_substitution::{ScopeOrigin, TypeSubstitution};
use smelt_hir::Symbol;
use std::collections::HashSet;

/// The type-parameter environment for one value-render position.
///
/// The owned companion of [`TypeSubstitution`]: same question, same answers,
/// but carried by value so it can be built at a render position and threaded
/// down the value-rendering recursion. `origin` is preserved so
/// [`Self::substitution`] hands the type lowerer an environment whose
/// provenance still reads correctly in a `Debug` dump.
#[derive(Clone, Debug)]
pub(crate) struct RenderScope {
    /// Names spellable as Rust identifiers at this position.
    in_scope: HashSet<Symbol>,
    /// Provenance of `in_scope`, for readers rather than for lowering.
    origin: ScopeOrigin,
}

impl RenderScope {
    /// The type parameters declared by the Rust item being emitted.
    ///
    /// This is the scope of an ordinary value render: a statement inside a
    /// generic function or a method of a generic class may spell that item's
    /// type parameters, and must erase every other one.
    pub(crate) const fn lexical(in_scope: HashSet<Symbol>) -> Self {
        Self {
            in_scope,
            origin: ScopeOrigin::Lexical,
        }
    }

    /// An environment in which nothing is spellable: every type parameter
    /// erases to `SmeltUnknown`.
    ///
    /// The correct scope for a value that crosses into a position whose Rust
    /// item declares no generics — a hoisted module-level item, or a callee the
    /// crate-wide gate demoted to an erased signature.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the erased render position is exercised by the unit tests; production value \
                      renders currently reach it through an empty ambient scope instead"
        )
    )]
    pub(crate) fn erased() -> Self {
        Self {
            in_scope: HashSet::new(),
            origin: ScopeOrigin::DeliberatelyErased,
        }
    }

    /// Narrow this scope to the names the OTHER side of a seam also spells.
    ///
    /// A coercion whose target is a *callee-declared* type has two sides, and a
    /// type parameter may be spelled as a Rust identifier only where BOTH
    /// spell it:
    ///
    /// * a name the caller's Rust item does not declare cannot be written in the
    ///   caller's body at all;
    /// * a name the callee did not emit as a real Rust generic is
    ///   `SmeltUnknown` in the emitted signature, so a value spelled at that
    ///   name would not fit the slot.
    ///
    /// [`smelt_hir::Symbol`] is name-interned, so a caller's `T` and a callee's
    /// `T` are the same key; the intersection is what keeps the agreement honest
    /// without pretending the two `T`s are unrelated. Taking the caller's scope
    /// alone is what put `SmeltList<T>` into an argument of an erased
    /// `find_middleware(middleware: SmeltRecord<String, SmeltList<SmeltUnknown>>)`;
    /// taking the callee's alone would spell a `T` the caller's item never
    /// declared.
    pub(crate) fn narrowed_to(&self, other: &HashSet<Symbol>) -> Self {
        Self {
            in_scope: self
                .in_scope
                .iter()
                .filter(|name| other.contains(name))
                .copied()
                .collect(),
            origin: ScopeOrigin::LexicalSubset,
        }
    }

    /// Whether `name` is spellable as a Rust identifier at this position.
    ///
    /// This is *the* query H42 collapsed the two disagreeing erasure rules onto:
    /// a `Type::TypeParam` target renders as its Rust identifier when this
    /// answers `true` and erases to `SmeltUnknown` when it answers `false`, on
    /// every side of the position.
    pub(crate) fn spells(&self, name: Symbol) -> bool {
        self.substitution().spells(name)
    }

    /// Lend this scope to the **type** lowerer.
    ///
    /// `rust_type` takes a borrowed [`TypeSubstitution`]; this is the bridge, so
    /// a container annotation rendered for a value position is lowered under the
    /// very environment that position's entries were coerced in.
    pub(crate) fn substitution(&self) -> TypeSubstitution<'_> {
        match self.origin {
            ScopeOrigin::DeliberatelyErased => TypeSubstitution::erased(),
            ScopeOrigin::Lexical => TypeSubstitution::lexical(&self.in_scope),
            ScopeOrigin::CalleeEmission => TypeSubstitution::callee_emission(&self.in_scope),
            ScopeOrigin::LexicalSubset => TypeSubstitution::lexical_subset(&self.in_scope),
        }
    }
}

impl FunctionEmitter<'_> {
    /// Build the render scope for the emitter's **current** position.
    ///
    /// Every value-rendering entry point that is not itself inside the
    /// value-rendering recursion calls this once and threads the result, so the
    /// whole render of that value agrees about which type parameters are
    /// spellable. It is deliberately recomputed per entry rather than cached:
    /// `FunctionEmitter::current_function_type_params` reads the hoisted-item
    /// flag and the generic-suppression cell, both of which change while a
    /// function is being emitted.
    pub(super) fn render_scope(&self) -> RenderScope {
        RenderScope::lexical(self.current_function_type_params())
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "a panicking assertion is the point of a unit test; matches the sibling \
              tests in crate::type_substitution"
)]
mod tests {
    //! The scope's two questions — "is this name spellable here?" and "what
    //! environment does the type lowerer see?" — must give the same answer, on
    //! every provenance.

    use super::*;
    use crate::type_substitution::Resolved;
    use smelt_hir::SymbolInterner;

    #[test]
    /// A lexical scope spells the names it holds and erases the rest, and the
    /// lent substitution agrees with `spells` on both.
    fn lexical_scope_spells_its_own_names() {
        let mut interner = SymbolInterner::default();
        let held = interner.intern("T");
        let other = interner.intern("U");
        let scope = RenderScope::lexical(HashSet::from([held]));
        assert!(scope.spells(held));
        assert!(!scope.spells(other));
        assert_eq!(
            scope.substitution().resolve(held).unwrap(),
            Resolved::Spelled(held)
        );
        assert_eq!(
            scope.substitution().resolve(other).unwrap(),
            Resolved::Erased
        );
    }

    #[test]
    /// An erased scope spells nothing, whatever the name.
    fn erased_scope_spells_nothing() {
        let mut interner = SymbolInterner::default();
        let name = interner.intern("T");
        let scope = RenderScope::erased();
        assert!(!scope.spells(name));
        assert_eq!(
            scope.substitution().resolve(name).unwrap(),
            Resolved::Erased
        );
    }
}
