//! Callable fields of a class instance, viewed through an interface record.
//!
//! JavaScript binds `this` from the CALL: `router.match(m, p)` runs whatever
//! function `router.match` currently holds with `this === router`, whether the
//! member is a prototype method or an own property holding a plain function
//! (Hono's reg-exp router installs `match: typeof match<..> = match`, a module
//! function declared `this: R`, and that function replaces itself with
//! `this.match = ..` on its first call).
//!
//! Smelt views a class instance through an interface type by building the
//! interface's record from the instance's members (the structural adapter in
//! `core.rs`). A prototype method becomes a closure bound to the instance
//! (`virtual_method_storage_field_text`), but an own CALLABLE FIELD used to be
//! copied as a bare value: a later `iface.m(..)` then called the stored function
//! with no receiver installed (`this` read `undefined`) and never saw a
//! reassignment of the instance's field. A hand-written port would store a
//! closure that dispatches through the instance, which is what this module
//! emits: a slot of the interface member's type that, on each call,
//!
//! 1. installs the SOURCE INSTANCE as `this` for the duration of the call
//!    (lazily erased, exactly like a direct `instance.field(..)` call does
//!    through `Rvalue::BindThis`), and
//! 2. reads the instance's field AT CALL TIME, so `this.m = other` inside the
//!    callee is observed by the next call through the interface view.
//!
//! The rule is pay-for-use: it applies only when the program reads `this`
//! through the dynamic receiver channel at all ([`program_reads_this`]); a
//! program that never does keeps the plain field copy, which is then
//! indistinguishable.

use smelt_hir::{Type, TypeId};
use smelt_mir::{Mir, MirField, Rvalue};

use super::render_scope::RenderScope;
use super::{EmitError, FunctionEmitter, sanitize_ident};

/// Return whether any function or closure body reads the dynamically scoped
/// `this` receiver (`Rvalue::ThisRead`).
///
/// Installing a receiver is only observable through such a read, so the
/// receiver-bound interface slots below are emitted only when this holds.
pub(crate) fn program_reads_this(mir: &Mir) -> bool {
    crate::stdlib::rvalues(mir).any(|rvalue| matches!(rvalue, Rvalue::ThisRead))
}

impl FunctionEmitter<'_> {
    /// Emit an interface record slot filled by a class instance's callable
    /// FIELD, dispatching through the instance on every call (module docs).
    ///
    /// `source` is the class type being viewed and `source_field` its own
    /// field; `target` is the interface record and `target_field` the member
    /// the slot fills. The
    /// captured instance is the adapter's `smelt_struct_value`. Declines
    /// (`Ok(None)`) — leaving the plain field copy — unless the program reads
    /// `this`, the source is a class (not another interface record), and both
    /// the field and the slot are plain function types.
    pub(super) fn receiver_bound_field_slot_text(
        &self,
        source: TypeId,
        target: TypeId,
        source_field: &MirField,
        target_field: &MirField,
        scope: &RenderScope,
    ) -> Result<Option<String>, EmitError> {
        // Only an INTERFACE view: converting between two classes copies the
        // instance's members into another class value, whose callable fields
        // keep their own function values (and identity).
        if !self.context.program_reads_this() || !self.is_interface_record_type(target) {
            return Ok(None);
        }
        let Some(Type::Class { name, .. }) = self.mir.types.get(source) else {
            return Ok(None);
        };
        if !self.mir.classes.iter().any(|class| class.name == *name) {
            return Ok(None);
        }
        let Some(Type::Function(slot_fn)) = self.mir.types.get(target_field.ty).cloned() else {
            return Ok(None);
        };
        // An erased-rest callable is emitted as a `SmeltErasedFunction` value,
        // not an `Rc<dyn Fn>`, and is not called with `(..)`; it keeps the copy.
        if self.is_erased_unknown_rest_function(&slot_fn) {
            return Ok(None);
        }
        if !matches!(self.mir.types.get(source_field.ty), Some(Type::Function(_))) {
            return Ok(None);
        }
        let field_name = sanitize_ident(self.symbol_name(source_field.name)?);
        let field_read = if self.is_reference_class_type(source) {
            format!("smelt_slot_receiver.0.borrow().{field_name}.clone()")
        } else {
            format!("smelt_slot_receiver.{field_name}.clone()")
        };
        let callee = self.value_at_type_text(&field_read, source_field.ty, target_field.ty, scope)?;
        // Every generated class implements `IntoSmeltUnknown`, so the deferred
        // erasure calls it instead of inlining the class's whole erased view
        // into each slot (which multiplied a class with many callable fields
        // by its own erasure size, once per slot).
        let erase = "::std::clone::Clone::clone(&smelt_bound_receiver).into_smelt_unknown()";
        let args = (0..slot_fn.params.len())
            .map(|index| format!("smelt_slot_arg_{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        Ok(Some(format!(
            "{{ let smelt_slot_receiver = smelt_struct_value.clone(); \
             let smelt_slot: {ty} = ::std::rc::Rc::new(move |{args}| {{ \
             let _smelt_this_guard = smelt_push_this_lazy({{ let smelt_bound_receiver = smelt_slot_receiver.clone(); ::std::rc::Rc::new(move || {erase}) }}); \
             let smelt_slot_callee = {callee}; \
             (smelt_slot_callee)({args}) }}); smelt_slot }}",
            ty = self.type_text_with_impl_trait(target_field.ty, false)?,
        )))
    }

    /// Emit a callable slot of a record projected out of an ERASED object
    /// (`record_map` holds the object's entries), dispatching through that
    /// object on every call.
    ///
    /// The erased-to-record projection is the same seam as the class view
    /// above, one step later: an instance erased first (its callable fields
    /// and prototype methods become entries) and then narrowed to an
    /// interface. A method call through the record is a member call on the
    /// object in JavaScript, so the slot reads the member from the object at
    /// call time (seeing a `this.m = ..` write the callee made through the
    /// object, which also writes through to a class instance) and installs
    /// the object as `this`. Declines unless the program reads `this` and the
    /// slot is a plain (non-optional) function type.
    pub(super) fn erased_record_callable_slot_text(
        &self,
        record_map: &str,
        target: TypeId,
        field: &MirField,
        scope: &RenderScope,
    ) -> Result<Option<String>, EmitError> {
        // Only an INTERFACE record: narrowing an erased object back to a CLASS
        // rebuilds that class's value, whose callable fields must stay the
        // very function values the object holds (`b.d === d` after a deep
        // clone), not wrappers around them.
        if !self.context.program_reads_this() || !self.is_interface_record_type(target) {
            return Ok(None);
        }
        let Some(Type::Function(slot_fn)) = self.mir.types.get(field.ty).cloned() else {
            return Ok(None);
        };
        if self.is_erased_unknown_rest_function(&slot_fn) {
            return Ok(None);
        }
        let key = self.symbol_name(field.name)?;
        let unknown_ty = self.type_id(Type::Unknown)?;
        let callee = self.value_at_type_text(
            &format!("smelt_live_member(&smelt_slot_source, {key:?})"),
            unknown_ty,
            field.ty,
            scope,
        )?;
        let args = (0..slot_fn.params.len())
            .map(|index| format!("smelt_slot_arg_{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        Ok(Some(format!(
            "{{ let smelt_slot_source: SmeltUnknown = {record_map}.clone().into_smelt_unknown(); \
             let smelt_slot: {ty} = ::std::rc::Rc::new(move |{args}| {{ \
             let smelt_slot_callee = {callee}; \
             let _smelt_this_guard = smelt_push_this(smelt_slot_source.clone()); \
             (smelt_slot_callee)({args}) }}); smelt_slot }}",
            ty = self.type_text_with_impl_trait(field.ty, false)?,
        )))
    }
}
