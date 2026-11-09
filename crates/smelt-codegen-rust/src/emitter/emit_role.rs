//! What a [`FunctionEmitter`] is rendering, as one explicit value.
//!
//! One MIR function body is rendered in several different roles: as its own
//! item (a free function or a method in its declaring class), as an INHERITED
//! copy re-emitted into a subclass's `impl`, as a BASE INITIALIZER copy of an
//! ancestor constructor, or as an inline CLOSURE body inside another emitter.
//! Each role changes which type parameters are in scope and how the receiver is
//! typed. One enum makes the roles exclusive: an emitter cannot be an
//! inherited copy AND a base initializer copy at once, and a closure cannot
//! silently drop one of its parent's settings.
//!
//! [`EmitRole`] makes the role a single value fixed by the constructor that
//! creates the emitter ([`FunctionEmitter::new`], [`FunctionEmitter::inherited_copy`],
//! [`FunctionEmitter::base_initializer_copy`], [`FunctionEmitter::for_generic_trial`],
//! [`FunctionEmitter::child_for_closure`]). The only role state that changes
//! after construction is an own item's [`OwnGenerics`] decision, which every
//! entry point applies through the one rule in [`crate_generic_decision`].

use super::*;

/// Whether a function emitted as its own item declares its type parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OwnGenerics {
    /// The type parameters the function (or its class) declares are in scope.
    ///
    /// This is the state before the crate-wide decision is applied, and the
    /// state a method keeps: class members always render in their class's
    /// `impl<..>` scope.
    Declared,
    /// A free function outside the crate-wide generic set: its own type
    /// parameters are erased to `SmeltUnknown` in its signature, body and every
    /// call site (see `EmitContext::populate_generic_functions`).
    Erased,
    /// The body-cleanliness trial of `EmitContext::populate_generic_functions`:
    /// the type parameters are in scope AND the function's own recursive calls
    /// render against the generic signature it is being tried as (see
    /// [`FunctionEmitter::callee_emits_generics`]).
    Trial,
}

/// The substitution an inherited or base-initializer copy applies to the
/// declaring base's type parameters.
///
/// The copy itself is emitted from a type-substituted clone of its function
/// (`crate::classes::substituted_inherited_member`), but its closures are
/// separate MIR items still spelled in the base's scope, so they render through
/// `closures`. The copy's own emitter also renders closure DECLARATIONS, so it
/// applies `own`: the same map restricted to names the receiving class does not
/// declare itself (see [`FunctionEmitter::inherited_substitution`]).
#[derive(Clone, Debug, Default)]
pub(crate) struct InheritedSubstitution {
    /// Applied to the copy's own rendered types.
    pub(crate) own: HashMap<Symbol, TypeId>,
    /// Handed to every closure the copy emits.
    pub(crate) closures: HashMap<Symbol, TypeId>,
}

/// The base-initializer receiver as seen by a closure emitted inside the copy.
#[derive(Clone, Debug)]
pub(crate) struct BaseInitializerReceiver {
    /// The derived class whose `impl` receives the base initializer copy.
    pub(crate) owner: Symbol,
    /// The locals of this emitter holding the receiver (the copy's own
    /// constructor instance, or a closure's capture of it).
    pub(crate) receivers: HashSet<LocalId>,
}

/// Scope a closure body inherits from the emitter it is rendered inside.
#[derive(Clone, Debug, Default)]
pub(crate) struct ClosureScope {
    /// Type parameters in scope in the enclosing Rust output.
    ///
    /// A closure is emitted inline, so whatever generics the enclosing function
    /// declares are visible to its body; the closure's synthetic `MirFunction`
    /// declares none of its own. The set is captured from the enclosing
    /// emitter's [`FunctionEmitter::current_function_type_params`], so it is
    /// already gated by that emitter's erasure decision and hoisting state.
    pub(crate) enclosing_type_params: HashSet<Symbol>,
    /// The free function on body-cleanliness trial, if the enclosing emitter is
    /// (or is inside) one.
    pub(crate) trial_function: Option<FuncId>,
    /// The declaring base's type parameters mapped into the receiving class's
    /// scope, when the closure is inside an inherited or base-initializer copy.
    /// Applied both to the closure's own types and handed on to its closures.
    pub(crate) inherited_type_args: HashMap<Symbol, TypeId>,
    /// The base-initializer receiver, when the closure is inside such a copy.
    pub(crate) base_initializer: Option<BaseInitializerReceiver>,
}

/// What a [`FunctionEmitter`] is rendering; see the module docs.
#[derive(Clone, Debug)]
pub(crate) enum EmitRole {
    /// The function as its own item: a free function, a member in its
    /// declaring class's `impl`, or a synthetic type-rendering emitter.
    Own {
        /// Whether its own type parameters are in scope.
        generics: OwnGenerics,
    },
    /// A base class's method re-emitted into the subclass `owner`'s `impl`.
    ///
    /// Smelt flattens inheritance, so a base method is re-emitted in every
    /// subclass's impl. A fluent base method annotated with its own class
    /// (`route(..): Hono<..> { return this }`) is JavaScript's polymorphic
    /// `this`: in the subclass copy it answers the subclass, so the copy
    /// returns `Self`. `owner`'s type parameters, not the base's, are in scope.
    InheritedCopy {
        /// The class whose `impl` block receives the copy.
        owner: Symbol,
        /// The base-to-owner type parameter substitution.
        substitution: InheritedSubstitution,
    },
    /// An ancestor CONSTRUCTOR re-emitted as an initializer over `Self` in the
    /// derived class `owner`'s `impl` (see `crate::base_init`).
    ///
    /// The copy receives the instance instead of allocating it, types `this` as
    /// `Self`, and renders type parameters in `owner`'s scope.
    BaseInitializerCopy {
        /// The base-to-owner type parameter substitution.
        substitution: InheritedSubstitution,
        /// The derived class and the local(s) holding the received instance.
        receiver: BaseInitializerReceiver,
    },
    /// A closure body rendered inline inside another emitter.
    Closure(ClosureScope),
}

/// The empty substitution answered by roles that apply none.
fn empty_substitution() -> &'static HashMap<Symbol, TypeId> {
    static EMPTY: std::sync::OnceLock<HashMap<Symbol, TypeId>> = std::sync::OnceLock::new();
    EMPTY.get_or_init(HashMap::new)
}

/// The crate-wide rule for whether a function emitted as its own item declares
/// its type parameters.
///
/// A free function (`HirOrigin::Body`) outside the set
/// `EmitContext::populate_generic_functions` computed renders erased; anything
/// else keeps [`OwnGenerics::Declared`]. The emission entry points
/// ([`FunctionEmitter::emit`], [`FunctionEmitter::emit_method`],
/// [`FunctionEmitter::emit_python_add_impl`]) and the typedness measurement all
/// apply THIS function, so what is measured and what is emitted cannot drift.
pub(crate) fn crate_generic_decision(context: &EmitContext, function: &MirFunction) -> OwnGenerics {
    if matches!(function.origin, HirOrigin::Body(_)) && !context.is_generic_function(function.id) {
        OwnGenerics::Erased
    } else {
        OwnGenerics::Declared
    }
}

impl EmitRole {
    /// Whether an own item's type parameters were erased by the crate-wide
    /// generic decision.
    pub(crate) const fn own_generics_erased(&self) -> bool {
        matches!(self, Self::Own { generics: OwnGenerics::Erased })
    }

    /// The class whose `impl` receives an inherited copy.
    pub(crate) const fn inherited_impl_owner(&self) -> Option<Symbol> {
        match self {
            Self::InheritedCopy { owner, .. } => Some(*owner),
            _ => None,
        }
    }

    /// Whether this emitter renders an inherited copy (not a closure inside one).
    pub(crate) const fn is_inherited_copy(&self) -> bool {
        matches!(self, Self::InheritedCopy { .. })
    }

    /// The base-to-receiver map applied to this emitter's own rendered types.
    pub(crate) fn inherited_type_args(&self) -> &HashMap<Symbol, TypeId> {
        match self {
            Self::InheritedCopy { substitution, .. }
            | Self::BaseInitializerCopy { substitution, .. } => &substitution.own,
            Self::Closure(scope) => &scope.inherited_type_args,
            Self::Own { .. } => empty_substitution(),
        }
    }

    /// The base-to-receiver map handed to closures this emitter renders.
    pub(crate) fn closure_inherited_type_args(&self) -> &HashMap<Symbol, TypeId> {
        match self {
            Self::InheritedCopy { substitution, .. }
            | Self::BaseInitializerCopy { substitution, .. } => &substitution.closures,
            Self::Closure(scope) => &scope.inherited_type_args,
            Self::Own { .. } => empty_substitution(),
        }
    }

    /// The base-initializer receiver, for the copy itself or a closure in it.
    pub(crate) const fn base_initializer(&self) -> Option<&BaseInitializerReceiver> {
        match self {
            Self::BaseInitializerCopy { receiver, .. } => Some(receiver),
            Self::Closure(ClosureScope {
                base_initializer: Some(receiver),
                ..
            }) => Some(receiver),
            _ => None,
        }
    }
}
