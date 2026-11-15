//! A static method's `this`: the class it was called on, as a Rust generic.
//!
//! # The defect this closes
//!
//! JavaScript binds `this` in a static method to the class the method was
//! CALLED on. `class Sub extends Base {}` inherits `static bump() {
//! this.count++ }`, and `Sub.bump()` writes `Sub.count` (an own property of
//! `Sub` from then on), reads `Sub`'s own statics, and `new this()` builds a
//! `Sub`. Smelt bound `this` to the declaring class when it lowered the method,
//! so `Sub.bump()` wrote `Base.count` and `new this()` built (or failed to
//! build) a `Base`.
//!
//! # The representation
//!
//! A hand-writing Rust team would make the receiver a type parameter bounded
//! by a trait of the class's statics, which is what a static method that uses
//! `this` becomes:
//!
//! ```text
//! trait BaseSmeltStatics {                       // what `this` is used for
//!     fn __smelt_static_get_count() -> f64;
//!     fn __smelt_static_set_count(value: f64) -> f64;
//!     fn __smelt_static_construct() -> Base;
//! }
//! impl BaseSmeltStatics for Base { .. }           // Base's own statics
//! impl BaseSmeltStatics for Sub { .. }            // Sub's: its inherited-static accessors
//! impl Base {
//!     fn bump<SmeltThis: BaseSmeltStatics>(_smelt_this: PhantomData<SmeltThis>) -> f64 { .. }
//! }
//! Base::bump(PhantomData::<Sub>)                  // `Sub.bump()`
//! ```
//!
//! - The frontend lowers each use of `this` in such a method to an
//!   `Rvalue::StaticDispatch` with no receiver (`smelt_hir::StaticMember`),
//!   and a static method reached through a class that inherits it
//!   (`Sub.bump()`) to one whose receiver is that class.
//! - The receiver travels as a zero-sized `PhantomData<SmeltThis>` argument,
//!   so a caller names it as a value and the method's own generics stay
//!   inferred.
//! - The method lowers ONCE, in the module that declares it: a subclass in
//!   another module only adds its `impl` of the trait (its own and inherited
//!   statics, its constructor, its own overrides of the static methods
//!   reached through `this`), which codegen writes from the closed world.
//!
//! Pay-for-use: a static method that never uses `this` keeps its plain
//! associated function, and a class none of whose static methods does gets no
//! trait.
#![expect(
    clippy::redundant_pub_crate,
    reason = "receiver helpers are shared by the crate builder and the function emitter"
)]

use std::collections::{HashMap, HashSet};

use smelt_hir::{StaticMember, Symbol};
use smelt_mir::{BasicBlock, Callee, FuncId, HirOrigin, Mir, Rvalue, Statement, Terminator};

/// The generic type parameter naming the receiver class.
pub(crate) const RECEIVER_GENERIC: &str = "SmeltThis";

/// The zero-sized parameter carrying the receiver class.
pub(crate) const RECEIVER_PARAM: &str = "_smelt_this";

/// The Rust name of the statics trait of the class whose type is named
/// `type_name`; claimed with the class (`names::plan`).
pub(crate) fn statics_trait_name(type_name: &str) -> String {
    format!("{type_name}SmeltStatics")
}

/// The argument a caller passes for the receiver class whose Rust type is
/// `receiver_type`.
pub(crate) fn receiver_argument(receiver_type: &str) -> String {
    format!("::std::marker::PhantomData::<{receiver_type}>")
}

/// Which static methods take a receiver class and what each class's statics
/// trait holds, computed once per crate.
#[derive(Debug, Default)]
pub(crate) struct StaticReceiverPlan {
    /// The static methods that use `this` (directly or in a closure they
    /// create), each mapped to the class whose trait bounds its receiver.
    generic: HashMap<FuncId, Symbol>,
    /// Each class with a statics trait, and the members its static methods
    /// reach through `this`, in first-use order.
    traits: Vec<(Symbol, Vec<StaticMember>)>,
}

impl StaticReceiverPlan {
    /// Scan every static method of `mir`.
    pub(crate) fn compute(mir: &Mir) -> Self {
        let closure_members: Vec<Vec<(Symbol, StaticMember)>> = mir
            .closures
            .iter()
            .map(|closure| receiver_uses(&closure.blocks))
            .collect();
        let mut plan = Self::default();
        for function in &mir.functions {
            let HirOrigin::ClassStaticMethod { class, .. } = function.origin else {
                continue;
            };
            let mut uses = receiver_uses(&function.blocks);
            for closure in created_closures(mir, &function.blocks) {
                if let Some(members) = closure_members.get(closure) {
                    uses.extend(members.iter().copied());
                }
            }
            if uses.is_empty() {
                continue;
            }
            plan.generic.insert(function.id, class);
            for (owner, member) in uses {
                if !plan.traits.iter().any(|(name, _)| *name == owner) {
                    plan.traits.push((owner, Vec::new()));
                }
                if let Some((_, members)) = plan.traits.iter_mut().find(|(name, _)| *name == owner)
                    && !members.contains(&member)
                {
                    members.push(member);
                }
            }
            // The class of a receiver-generic method always has a trait,
            // even when only its closures name the members.
            if !plan.traits.iter().any(|(name, _)| *name == class) {
                plan.traits.push((class, Vec::new()));
            }
        }
        // A class no other class extends is its methods' only receiver: its
        // `this` is the class itself, reached directly (pay-for-use).
        plan.traits
            .retain(|(class, _)| receiver_classes(mir, *class).len() > 1);
        let kept: HashSet<Symbol> = plan.traits.iter().map(|(class, _)| *class).collect();
        plan.generic.retain(|_, class| kept.contains(class));
        plan
    }

    /// Whether `class` has a statics trait: a static method of it uses `this`
    /// and some class extends it.
    pub(crate) fn has_trait(&self, class: Symbol) -> bool {
        self.traits.iter().any(|(name, _)| *name == class)
    }

    /// The class bounding `function`'s receiver, when it takes one.
    pub(crate) fn receiver_class(&self, function: FuncId) -> Option<Symbol> {
        self.generic.get(&function).copied()
    }

    /// The members of `class`'s statics trait.
    pub(crate) fn members(&self, class: Symbol) -> &[StaticMember] {
        self.traits
            .iter()
            .find(|(name, _)| *name == class)
            .map_or(&[], |(_, members)| members.as_slice())
    }
}

/// The `(class, member)` pairs a body reaches through its own `this`.
fn receiver_uses(blocks: &[BasicBlock]) -> Vec<(Symbol, StaticMember)> {
    let mut uses = Vec::new();
    let mut note = |value: &Rvalue| {
        if let Rvalue::StaticDispatch {
            receiver: None,
            class,
            member,
            ..
        } = value
        {
            uses.push((*class, *member));
        }
    };
    for block in blocks {
        for statement in &block.statements {
            if let Statement::Assign { value, .. } = statement {
                note(value);
            }
        }
        if let Some(Terminator::Call {
            callee: Callee::Op(op, _),
            ..
        }) = &block.terminator
        {
            note(op);
        }
    }
    uses
}

/// The closures a body creates, transitively (a closure's own closures
/// included), as indices into `Mir::closures`.
fn created_closures(mir: &Mir, blocks: &[BasicBlock]) -> Vec<usize> {
    let mut found = Vec::new();
    let mut pending = vec![blocks];
    let mut seen = HashSet::new();
    while let Some(next) = pending.pop() {
        for block in next {
            for statement in &block.statements {
                let Statement::Assign {
                    value: Rvalue::Closure { id, .. },
                    ..
                } = statement
                else {
                    continue;
                };
                let Some(index) = mir.closures.iter().position(|closure| closure.id == *id) else {
                    continue;
                };
                if !seen.insert(index) {
                    continue;
                }
                found.push(index);
                if let Some(closure) = mir.closures.get(index) {
                    pending.push(&closure.blocks);
                }
            }
        }
    }
    found
}

/// The instance classes a receiver bounded by `class`'s trait can be: `class`
/// and every non-generic class extending it (a statics frame is never a
/// receiver here). A generic subclass cannot be named as a receiver without
/// type arguments its statics never supply; a call through one passes the
/// declaring class.
pub(crate) fn receiver_classes(mir: &Mir, class: Symbol) -> Vec<Symbol> {
    mir.classes
        .iter()
        .filter(|candidate| candidate.statics_of.is_none() && candidate.type_params.is_empty())
        .filter(|candidate| crate::classes::class_chain_contains(mir, candidate.name, class))
        .map(|candidate| candidate.name)
        .collect()
}
