//! A callable stored in a nominal record's field keeps the slot's DECLARED ABI.
//!
//! This is the construction-side sibling of the call-side rule
//! `crate::emitter::local_analysis::FunctionEmitter::class_field_declared_function_type`
//! states (round 31, item 3): **substituting a type parameter does not change
//! how the callee is called.**
//!
//! A generated record struct is emitted ONCE, from its declaration, and then
//! instantiated. `interface Router<T> { add: (method: string, path: string,
//! handler: T) => void }` becomes
//!
//! ```ignore
//! struct Router<T> { add: ::std::rc::Rc<dyn Fn(String, String, &T)> }
//! ```
//!
//! — `&T` because
//! [`param_type_is_by_shared_reference`](crate::emitter::FunctionEmitter::param_type_is_by_shared_reference)
//! answers TRUE for a bare type parameter, which is what keeps the declaration's
//! rendering and an erased caller's `&SmeltUnknown` rendering agreeing about one
//! Rust type. At `Router<(SmeltUnknown, RouterRoute)>` Rust instantiates that
//! field as `Rc<dyn Fn(String, String, &(SmeltUnknown, RouterRoute))>`.
//!
//! Every *value* built for that field, however, is built from the field type
//! MIR hands the emitter, which
//! [`structural_record_fields`](crate::emitter::FunctionEmitter::structural_record_fields)
//! has already substituted: a tuple, which the same predicate answers FALSE for
//! on its own perfectly correct terms. The rebuilt callable therefore comes out
//! as `Rc<dyn Fn(String, String, (SmeltUnknown, RouterRoute))>` and does not fit
//! the slot (E0308).
//!
//! The two renderings are both right for the type they were given; what is
//! missing is that only the DECLARATION knows the slot's ABI. This module
//! recovers it and bridges the two with a typed adapter closure — one `&`
//! taken or one `.clone()` per parameter whose ABI the substitution changed,
//! and nothing else. No value is erased: both sides spell the same Rust types,
//! and the adapter exists purely because Rust distinguishes `T` from `&T` while
//! TypeScript does not.

use super::FunctionEmitter;
use crate::EmitError;
use crate::generic_bindings::{CalleeTypeParamBindings, bind_class_type_params};
use crate::type_substitution::TypeSubstitution;
use super::types::MutablePrefix;
use smelt_hir::{FunctionType, Symbol, Type, TypeId};
use smelt_mir::MirField;

/// How one parameter position is passed on each side of the slot seam.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ParamAbi {
    /// The slot's own ABI, read off the record's DECLARED field type.
    declared_is_reference: bool,
    /// The ABI the value was rendered at, read off the substituted field type.
    rendered_is_reference: bool,
}

impl FunctionEmitter<'_> {
    /// The DECLARED type of `field` on the nominal record `record_ty`, plus the
    /// bindings that instantiate the record's own type parameters.
    ///
    /// `Type::Class` names a generated NOMINAL type, which is a generated class
    /// OR an interface record; both carry callable slots and both get
    /// instantiated, so both have a declaration to ask. Answers `None` for a
    /// name that is neither, for a field the declaration does not have, and for
    /// a record whose argument list does not match its parameter list — in every
    /// such case there is nothing to recover and the substituted type stays in
    /// charge, exactly as before.
    fn declared_record_field_slot(
        &self,
        record_ty: TypeId,
        field: Symbol,
    ) -> Option<(TypeId, CalleeTypeParamBindings)> {
        let Type::Class { name, .. } = self.mir.types.get(record_ty)? else {
            return None;
        };
        let (declared_ty, type_params) = self
            .mir
            .classes
            .iter()
            .find(|class| class.name == *name)
            .and_then(|class| {
                let declared = crate::classes::effective_class_fields(self.mir, class)
                    .into_iter()
                    .find(|candidate| candidate.name == field)?;
                Some((declared.ty, class.type_params.clone()))
            })
            .or_else(|| {
                let interface = self
                    .mir
                    .interfaces
                    .iter()
                    .find(|interface| interface.name == *name)?;
                let declared = crate::classes::effective_interface_fields(self.mir, interface)
                    .into_iter()
                    .find(|candidate| candidate.name == field)?;
                Some((declared.ty, interface.type_params.clone()))
            })?;
        let names = type_params
            .iter()
            .map(|param| param.name)
            .collect::<Vec<_>>();
        if names.is_empty() {
            // Nothing was substituted, so the declaration and the rendered type
            // are the same type and the seam cannot exist.
            return None;
        }
        let bindings = bind_class_type_params(self.mir, &names, record_ty);
        Some((declared_ty, bindings))
    }

    /// Per-parameter ABI on both sides of the slot, or `None` when the two
    /// function types are not comparable position by position.
    fn record_slot_param_abis(
        &self,
        declared: &FunctionType,
        rendered: &FunctionType,
    ) -> Option<Vec<ParamAbi>> {
        if declared.params.len() != rendered.params.len()
            || declared.rest != rendered.rest
            || declared.mutable_params != rendered.mutable_params
        {
            return None;
        }
        Some(
            declared
                .params
                .iter()
                .zip(rendered.params.iter())
                .enumerate()
                .map(|(index, (declared_param, rendered_param))| ParamAbi {
                    declared_is_reference: self.callback_param_is_shared_reference(
                        declared,
                        index,
                        *declared_param,
                    ),
                    rendered_is_reference: self.callback_param_is_shared_reference(
                        rendered,
                        index,
                        *rendered_param,
                    ),
                })
                .collect(),
        )
    }

    /// Re-wrap a callable already rendered at a record field's SUBSTITUTED type
    /// so it fits the slot's DECLARED ABI.
    ///
    /// `value_text` is whatever the ordinary coercion produced for the field —
    /// a rebuilt callable, a default callback, a `map_or` over a record lookup.
    /// Answers `None`, leaving that text untouched, unless
    ///
    /// * the record is a generated class or interface with type parameters,
    /// * the field's declared type and its substituted type are both callables
    ///   of the same shape, and
    /// * at least one parameter position's by-shared-reference axis actually
    ///   differs between them.
    ///
    /// The last condition is what keeps this inert: a non-generic record, a
    /// non-callable field and a substitution that did not move the ABI all
    /// render exactly the bytes they rendered before.
    pub(super) fn record_slot_abi_adapter_text(
        &self,
        value_text: &str,
        record_ty: TypeId,
        field: &MirField,
    ) -> Result<Option<String>, EmitError> {
        let Some((declared_ty, bindings)) = self.declared_record_field_slot(record_ty, field.name)
        else {
            return Ok(None);
        };
        if declared_ty == field.ty {
            return Ok(None);
        }
        let (Some(Type::Function(declared)), Some(Type::Function(rendered))) = (
            self.mir.types.get(declared_ty),
            self.mir.types.get(field.ty),
        ) else {
            return Ok(None);
        };
        let (declared, rendered) = (declared.clone(), rendered.clone());
        let Some(abis) = self.record_slot_param_abis(&declared, &rendered) else {
            return Ok(None);
        };
        if abis
            .iter()
            .all(|abi| abi.declared_is_reference == abi.rendered_is_reference)
        {
            return Ok(None);
        }
        // The slot's own Rust type: the DECLARED type rendered at the record's
        // real type arguments. That is the same rendering the generated struct
        // field got, because it is the same declaration under the same
        // instantiation — `Rc<dyn Fn(String, String, &(SmeltUnknown,
        // RouterRoute))>` for `Router<[unknown, RouterRoute]>`.
        //
        // The caller's lexical scope is the base so a record instantiated at a
        // type parameter the enclosing item DOES declare keeps spelling it;
        // `TypeSubstitution::resolve` lets the lexical scope win over a binding
        // for exactly that reason.
        let caller_scope = self.current_function_type_params();
        let substitution = TypeSubstitution::lexical(&caller_scope).with_bindings(&bindings);
        let slot_text = self
            .rust_type(declared_ty, false, &substitution)?
            .into_string();
        let param_decls = self
            .callback_arg_decls(&declared, &substitution, MutablePrefix::Apply)?
            .join(", ");
        let forwarded = abis
            .iter()
            .enumerate()
            .map(|(index, abi)| {
                match (abi.declared_is_reference, abi.rendered_is_reference) {
                    // The slot hands the body a borrow, the wrapped callable
                    // wants the value: copy it out, per call, only where the
                    // two genuinely disagree.
                    (true, false) => format!("arg{index}.clone()"),
                    // The slot hands the body an owned value and the wrapped
                    // callable takes a borrow of it; Rust extends the borrow to
                    // the end of the statement, so naming it is enough.
                    (false, true) => format!("&arg{index}"),
                    _ => format!("arg{index}"),
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        Ok(Some(format!(
            "{{ let smelt_slot_callback = {value_text}; let smelt_slot_adapted: {slot_text} = \
             ::std::rc::Rc::new(move |{param_decls}| (smelt_slot_callback)({forwarded})); \
             smelt_slot_adapted }}"
        )))
    }

    /// Apply [`Self::record_slot_abi_adapter_text`] to a field value, returning
    /// the original text when no adaptation is needed.
    ///
    /// The shape every record adapter's field loop wants: one call, one string
    /// back, no branch at the call site.
    pub(super) fn record_field_value_at_slot_abi(
        &self,
        value_text: String,
        record_ty: TypeId,
        field: &MirField,
    ) -> Result<String, EmitError> {
        Ok(self
            .record_slot_abi_adapter_text(&value_text, record_ty, field)?
            .unwrap_or(value_text))
    }
}
