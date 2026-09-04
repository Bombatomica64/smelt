//! Function-signature state for the module being lowered.
//!
//! [`FunctionRegistry`] owns what the lowering pass knows about *named
//! functions* before or independently of their bodies: TypeScript overload
//! signatures, rest-parameter metadata, the forward-visible signatures of
//! hoisted declarations, and the two narrowing tables for user assertion
//! (`asserts value is T`) and predicate (`value is T`) functions.
//!
//! # The invariants this struct owns
//!
//! * **Overloads are a source-ordered list per implementation name**, and the
//!   return type inferred for an unannotated implementation is the *last*
//!   signature's. [`FunctionRegistry::push_overload`] appends and
//!   [`FunctionRegistry::last_overload`] reads that end, so no caller re-derives
//!   the ordering rule.
//! * **A local implementation shadows imported overloads.** Overload signatures
//!   are only valid for their own implementation, so a local helper sharing a
//!   name with an imported overloaded function must not inherit its return
//!   surface. [`FunctionRegistry::shadow_implementations`] performs that
//!   shadowing as one operation over the implemented names.

use std::collections::{HashMap, HashSet};

use smelt_hir::{Symbol, TypeId};

use crate::OverloadSignature;
use crate::lowering::{AssertionNarrowing, RestParam};

/// Forward-visible signature of a hoisted function declaration.
type ForwardFunctionType = (Symbol, TypeId);

/// Named-function signature knowledge for one module.
///
/// See the module docs for the invariants this owns.
#[derive(Debug, Default)]
pub(in crate::lowering) struct FunctionRegistry {
    /// TypeScript overload signatures keyed by implementation name, in source
    /// order.
    overloads: HashMap<String, Vec<OverloadSignature>>,
    /// Rest-parameter metadata for top-level function declarations.
    rests: HashMap<String, RestParam>,
    /// Forward-visible function declaration signatures for hoisted callback
    /// calls.
    forward_types: HashMap<String, ForwardFunctionType>,
    /// User assertion functions declared with `asserts value is T`.
    assertions: HashMap<String, AssertionNarrowing>,
    /// User predicate functions declared with `value is T`.
    predicates: HashMap<String, AssertionNarrowing>,
}

impl FunctionRegistry {
    /// Build the registry a fresh module starts from, seeded with the overloads
    /// and rest parameters published by earlier modules.
    pub(in crate::lowering) fn new(
        overloads: HashMap<String, Vec<OverloadSignature>>,
        rests: HashMap<String, RestParam>,
    ) -> Self {
        Self {
            overloads,
            rests,
            ..Self::default()
        }
    }

    /// Append one overload signature for `name`, preserving source order.
    pub(in crate::lowering) fn push_overload(
        &mut self,
        name: String,
        signature: OverloadSignature,
    ) {
        self.overloads.entry(name).or_default().push(signature);
    }

    /// Return every overload signature recorded for `name`.
    pub(in crate::lowering) fn overloads(&self, name: &str) -> Option<&Vec<OverloadSignature>> {
        self.overloads.get(name)
    }

    /// Return the last overload signature declared for `name`.
    ///
    /// An unannotated implementation infers its return type from this end of the
    /// list, which is why the ordering rule lives here.
    pub(in crate::lowering) fn last_overload(&self, name: &str) -> Option<&OverloadSignature> {
        self.overloads.get(name).and_then(|signatures| signatures.last())
    }

    /// Return whether `name` carries overload signatures.
    pub(in crate::lowering) fn has_overloads(&self, name: &str) -> bool {
        self.overloads.contains_key(name)
    }

    /// Replace the overload signatures recorded for `name`.
    pub(in crate::lowering) fn set_overloads(
        &mut self,
        name: String,
        signatures: Vec<OverloadSignature>,
    ) {
        self.overloads.insert(name, signatures);
    }

    /// Drop imported overloads shadowed by implementations in this module.
    ///
    /// Each implemented name is left with an empty signature list rather than
    /// removed, so a later lookup sees "declared here, no overloads" instead of
    /// falling back to the imported surface.
    pub(in crate::lowering) fn shadow_implementations(&mut self, implemented: &HashSet<String>) {
        for name in implemented {
            self.overloads.insert(name.clone(), Vec::new());
        }
    }

    /// Return the rest-parameter metadata of the function named `name`.
    pub(in crate::lowering) fn rest(&self, name: &str) -> Option<RestParam> {
        self.rests.get(name).copied()
    }

    /// Record the rest-parameter metadata of the function named `name`.
    pub(in crate::lowering) fn set_rest(&mut self, name: String, rest: RestParam) {
        self.rests.insert(name, rest);
    }

    /// Return the forward-visible signature of the hoisted function `name`.
    pub(in crate::lowering) fn forward_type(&self, name: &str) -> Option<ForwardFunctionType> {
        self.forward_types.get(name).copied()
    }

    /// Record the forward-visible signature of a hoisted function declaration.
    pub(in crate::lowering) fn set_forward_type(&mut self, name: String, symbol: Symbol, ty: TypeId) {
        self.forward_types.insert(name, (symbol, ty));
    }

    /// Return the narrowing performed by the assertion function `name`.
    pub(in crate::lowering) fn assertion(&self, name: &str) -> Option<AssertionNarrowing> {
        self.assertions.get(name).copied()
    }

    /// Record the narrowing performed by an `asserts value is T` function.
    pub(in crate::lowering) fn set_assertion(
        &mut self,
        name: String,
        narrowing: AssertionNarrowing,
    ) {
        self.assertions.insert(name, narrowing);
    }

    /// Return the narrowing performed by the predicate function `name`.
    pub(in crate::lowering) fn predicate(&self, name: &str) -> Option<AssertionNarrowing> {
        self.predicates.get(name).copied()
    }

    /// Record the narrowing performed by a `value is T` predicate function.
    pub(in crate::lowering) fn set_predicate(
        &mut self,
        name: String,
        narrowing: AssertionNarrowing,
    ) {
        self.predicates.insert(name, narrowing);
    }
}

#[cfg(test)]
mod tests {
    use super::FunctionRegistry;
    use crate::OverloadSignature;
    use smelt_hir::TypeId;
    use std::collections::HashSet;

    /// Build an overload signature identified by its return type.
    fn signature(return_ty: u32) -> OverloadSignature {
        OverloadSignature {
            type_params: Vec::new(),
            params: Vec::new(),
            param_lengths: Vec::new(),
            rest: None,
            min_rest: 0,
            required_params: None,
            return_ty: TypeId(return_ty),
            is_async: false,
        }
    }

    /// Overloads keep source order and the last one is what an unannotated
    /// implementation infers its return type from.
    #[test]
    fn overloads_keep_source_order_and_last_wins() {
        let mut functions = FunctionRegistry::default();
        functions.push_overload("f".to_owned(), signature(1));
        functions.push_overload("f".to_owned(), signature(2));

        assert_eq!(functions.overloads("f").map(Vec::len), Some(2));
        assert_eq!(
            functions.last_overload("f").map(|signature| signature.return_ty),
            Some(TypeId(2))
        );
    }

    /// A local implementation shadows imported overloads: the name stays known
    /// but carries no signatures, so lookup cannot fall back to the import.
    #[test]
    fn shadowing_leaves_the_name_known_with_no_signatures() {
        let mut functions = FunctionRegistry::default();
        functions.push_overload("imported".to_owned(), signature(1));
        functions.shadow_implementations(&HashSet::from(["imported".to_owned()]));

        assert!(functions.has_overloads("imported"));
        assert_eq!(functions.overloads("imported").map(Vec::len), Some(0));
        assert!(functions.last_overload("imported").is_none());
    }
}
