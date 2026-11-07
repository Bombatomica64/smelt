//! Rendering an inherited method's copy on a subclass, and a base initializer's
//! copy: the class-parameter substitution and the receiver upcast they need.

use super::*;
use crate::rust::RustExpr;

impl<'mir> FunctionEmitter<'mir> {
    /// An emitter for `function` re-emitted as an INHERITED copy into the
    /// subclass `owner`'s `impl` block (see [`emit_role::EmitRole::InheritedCopy`]).
    ///
    /// `function` is the type-substituted clone
    /// (`crate::classes::substituted_inherited_member`); the same substitution
    /// is recorded for the closures the copy emits.
    pub(crate) fn inherited_copy(
        mir: &'mir Mir,
        context: &'mir EmitContext,
        function: &'mir MirFunction,
        owner: Symbol,
    ) -> Result<Self, EmitError> {
        let mut emitter = Self::new(mir, context, function)?;
        let substitution = emitter.inherited_substitution(owner);
        emitter.role = emit_role::EmitRole::InheritedCopy {
            owner,
            substitution,
        };
        Ok(emitter)
    }

    /// [`Self::inherited_copy`] emitted as the method a `super` call reaches,
    /// named by `classes::super_copy_name`.
    pub(crate) fn super_copy(
        mir: &'mir Mir,
        context: &'mir EmitContext,
        function: &'mir MirFunction,
        owner: Symbol,
        name: String,
    ) -> Result<Self, EmitError> {
        let mut emitter = Self::inherited_copy(mir, context, function, owner)?;
        emitter.method_name = Some(name);
        Ok(emitter)
    }

    /// An emitter for an ancestor CONSTRUCTOR rendered as the base initializer
    /// copy that runs inside `owner`'s `impl` (see `crate::base_init` and
    /// [`emit_role::EmitRole::BaseInitializerCopy`]).
    pub(crate) fn base_initializer_copy(
        mir: &'mir Mir,
        context: &'mir EmitContext,
        function: &'mir MirFunction,
        owner: Symbol,
    ) -> Result<Self, EmitError> {
        let mut emitter = Self::new(mir, context, function)?;
        let substitution = emitter.inherited_substitution(owner);
        emitter.role = emit_role::EmitRole::BaseInitializerCopy {
            substitution,
            receiver: emit_role::BaseInitializerReceiver {
                owner,
                // The constructor's instance is local 0
                // (`HirOrigin::ClassConstructor` lowering allocates it there);
                // in the copy it is the receiver.
                receivers: HashSet::from([LocalId(0)]),
            },
        };
        Ok(emitter)
    }

    /// The substitution an inherited or base-initializer copy of this
    /// emitter's function into `owner`'s `impl` applies (see
    /// [`emit_role::InheritedSubstitution`]).
    ///
    /// Closures get the whole map. The copy's own emitter also renders closure
    /// DECLARATIONS (a callback's header, its slot type), which are spelled in
    /// the base's scope, so it applies the map too, but only to names `owner`
    /// does not declare: the copy's substituted locals spell `owner`'s own
    /// parameters, and a name both classes declare (`Middle<T> extends
    /// Base<T[]>`) must keep meaning `owner`'s there.
    pub(super) fn inherited_substitution(&self, owner: Symbol) -> emit_role::InheritedSubstitution {
        let substitutions =
            crate::classes::inherited_member_substitution(self.mir, self.function, owner);
        let owner_params: HashSet<Symbol> = crate::classes::find_class(self.mir, owner)
            .map(|class| class.type_params.iter().map(|param| param.name).collect())
            .unwrap_or_default();
        emit_role::InheritedSubstitution {
            own: substitutions
                .iter()
                .filter(|(name, _)| !owner_params.contains(name))
                .map(|(name, ty)| (*name, *ty))
                .collect(),
            closures: substitutions,
        }
    }

    /// Upcast the base-initializer receiver where its DECLARING base type is
    /// asked for, e.g. `onError = (h): Base => { ..; return this }` stored by
    /// the base constructor.
    ///
    /// In the copy the receiver's Rust value is `Self` (the derived class)
    /// while MIR still types it at the declaring base, and the flattened
    /// structs are distinct Rust types with no subtyping between them. The
    /// value is rebuilt at the base type through the same member-wise
    /// structural adapter a base-typed value handed to a derived-typed slot
    /// already uses (Hono's `basePath`): a reference class's callable members
    /// are shared `Rc`s capturing the original instance, so calls through the
    /// upcast value still reach it. `None` when `operand` is not the receiver
    /// or the target is the owner class itself.
    pub(super) fn base_initializer_receiver_upcast_expr(
        &self,
        operand: &Operand,
        target: TypeId,
        scope: &RenderScope,
    ) -> Result<Option<RustExpr>, EmitError> {
        let Some(receiver) = self.role.base_initializer() else {
            return Ok(None);
        };
        let owner = receiver.owner;
        let local = match operand {
            Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => *local,
            _ => return Ok(None),
        };
        if !receiver.receivers.contains(&local) {
            return Ok(None);
        }
        let Some(Type::Class { name, .. }) = self.mir.types.get(target) else {
            return Ok(None);
        };
        if *name == owner {
            return Ok(None);
        }
        let Some(owner_ty) = crate::base_init::constructed_class_ty(self.mir, owner)? else {
            return Ok(None);
        };
        let value_text = self.operand_expr(operand)?;
        self.structural_record_adapter(&value_text, owner_ty, target, scope)
    }

    /// Whether this emitter renders a base initializer copy.
    pub(super) const fn is_base_initializer_copy(&self) -> bool {
        self.role.base_initializer().is_some()
    }

    /// Whether `local` is the constructor instance of a base initializer copy,
    /// i.e. the receiver the copy was handed.
    ///
    /// Its Rust type is `Self` (the impl's class), never the declaring base's
    /// name, which is a different struct under flattening.
    pub(super) fn is_base_initializer_receiver(&self, local: LocalId) -> bool {
        self.role
            .base_initializer()
            .is_some_and(|receiver| receiver.receivers.contains(&local))
    }

    /// Which inherited-copy substitution applies right now; see
    /// [`InheritedScope`].
    fn inherited_scope(&self) -> InheritedScope<'_> {
        if self.inherited_bound_suspended.get() {
            return InheritedScope::Suspended;
        }
        let map = self.role.inherited_type_args();
        if map.is_empty() {
            InheritedScope::None
        } else {
            InheritedScope::Active(map)
        }
    }

    /// The receiving-scope argument of a declaring base's type parameter inside
    /// an inherited copy, unless such a bound is being rendered already.
    pub(super) fn inherited_bound(&self, name: Symbol) -> Option<TypeId> {
        match self.inherited_scope() {
            InheritedScope::Active(map) => map.get(&name).copied(),
            InheritedScope::None | InheritedScope::Suspended => None,
        }
    }

    /// Run `render` on an inherited bound with the inherited map suspended
    /// (see [`InheritedScope::Suspended`]); the previous state is restored
    /// when the guard drops, whichever way `render` returns.
    pub(super) fn with_inherited_bound<R>(&self, render: impl FnOnce() -> R) -> R {
        let _suspended = InheritedBoundSuspension::enter(self);
        render()
    }
}

/// Whether an inherited copy's base-to-receiver type substitution applies to
/// the type being rendered.
///
/// An inherited or base-initializer copy (or a closure inside one) maps the
/// declaring base's type parameters to their arguments in the receiving
/// class's scope (`emit_role::EmitRole::inherited_type_args`). The bound such a
/// parameter maps to is itself spelled in the RECEIVING class's scope, whose
/// parameter may share a declaring parameter's name (`C<T> extends B<T[]>`),
/// so while a bound is rendered the map is suspended — applying it inside
/// would recurse forever.
pub(super) enum InheritedScope<'a> {
    /// The emitter applies no substitution.
    None,
    /// The substitution applies.
    Active(&'a HashMap<Symbol, TypeId>),
    /// A bound from the substitution is being rendered; it does not apply.
    Suspended,
}

/// Holds [`InheritedScope::Suspended`] for as long as it lives.
pub(super) struct InheritedBoundSuspension<'e, 'm> {
    /// The emitter whose substitution is suspended.
    emitter: &'e FunctionEmitter<'m>,
    /// The suspension state to restore on drop.
    previous: bool,
}

impl<'e, 'm> InheritedBoundSuspension<'e, 'm> {
    /// Suspend `emitter`'s inherited substitution.
    pub(super) fn enter(emitter: &'e FunctionEmitter<'m>) -> Self {
        let previous = emitter.inherited_bound_suspended.replace(true);
        Self { emitter, previous }
    }
}

impl Drop for InheritedBoundSuspension<'_, '_> {
    /// Restore the previous suspension state.
    fn drop(&mut self) {
        self.emitter.inherited_bound_suspended.set(self.previous);
    }
}
