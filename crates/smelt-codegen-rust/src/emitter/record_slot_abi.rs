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

use crate::rust::RustExpr;
use super::FunctionEmitter;
use crate::EmitError;
use crate::generic_bindings::{CalleeTypeParamBindings, bind_record_instantiation};
use crate::type_substitution::TypeSubstitution;
use super::types::MutablePrefix;
use smelt_hir::{FunctionType, Symbol, Type, TypeId};
use smelt_mir::{MirField, Operand, Place};

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
        let (declared_ty, type_params) = crate::classes::find_class(self.mir, *name)
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
        let bindings = bind_record_instantiation(self.mir, &names, record_ty);
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
    pub(super) fn record_slot_abi_adapter(
        &self,
        value_text: &RustExpr,
        record_ty: TypeId,
        field: &MirField,
    ) -> Result<Option<RustExpr>, EmitError> {
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
        if !self.slot_abis_differ(&declared, &rendered) {
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
        let slot = TypeSubstitution::lexical(&caller_scope).with_bindings(&bindings);
        let value_side = TypeSubstitution::lexical(&caller_scope);
        Ok(Some(self.slot_abi_bridge(
            value_text,
            (field.ty, &value_side),
            (declared_ty, &slot),
        )?))
    }

    /// Whether two shapes of one callable slot pass any argument differently,
    /// at any depth.
    ///
    /// The top level compares each parameter's by-shared-reference axis. A
    /// parameter or return that is itself a callable is compared the same way,
    /// recursively: `otherwise: (handler: (value: i) => R) => R` declares the
    /// INNER parameter `&i`, so at `i = number` the slot holds
    /// `Fn(Rc<dyn Fn(&f64) -> R>) -> R` while the substituted type renders
    /// `Fn(Rc<dyn Fn(f64) -> R>) -> R`. Only the declaration knows the former.
    fn slot_abis_differ(&self, left: &FunctionType, right: &FunctionType) -> bool {
        let Some(abis) = self.record_slot_param_abis(left, right) else {
            return false;
        };
        abis.iter()
            .any(|abi| abi.declared_is_reference != abi.rendered_is_reference)
            || left
                .params
                .iter()
                .zip(right.params.iter())
                .any(|(left_param, right_param)| {
                    self.nested_slot_callables(*left_param, *right_param).is_some()
                })
            || (!left.may_throw
                && !right.may_throw
                && self
                    .nested_slot_callables(left.return_ty, right.return_ty)
                    .is_some())
    }

    /// The two function types of one nested callable position, when both are
    /// callables and their ABIs differ somewhere ([`Self::slot_abis_differ`]).
    fn nested_slot_callables(
        &self,
        left: TypeId,
        right: TypeId,
    ) -> Option<(FunctionType, FunctionType)> {
        if left == right {
            return None;
        }
        let (Some(Type::Function(left_function)), Some(Type::Function(right_function))) =
            (self.mir.types.get(left), self.mir.types.get(right))
        else {
            return None;
        };
        self.slot_abis_differ(left_function, right_function)
            .then(|| (left_function.clone(), right_function.clone()))
    }

    /// Wrap `value_text`, a callable rendered at `from`, as a callable of the
    /// Rust type `to` renders, where both describe the same slot and differ
    /// only in how arguments are passed ([`Self::slot_abis_differ`]).
    ///
    /// Each side is a function type plus the environment its type parameters
    /// render in. Per parameter the closure takes the `to` ABI and forwards the
    /// `from` ABI: one `&` taken or one `.clone()` where the reference axis
    /// differs; a nested callable whose own ABI differs is bridged the other
    /// way round (parameters are contravariant). A nested callable return is
    /// bridged in the same direction as the slot. No value is converted, so no
    /// type is erased: both sides are the same Rust types up to `&`.
    fn slot_abi_bridge(
        &self,
        value_text: &RustExpr,
        (from_ty, from_env): (TypeId, &TypeSubstitution<'_>),
        (to_ty, to_env): (TypeId, &TypeSubstitution<'_>),
    ) -> Result<RustExpr, EmitError> {
        let (Some(Type::Function(from_function)), Some(Type::Function(to_function))) =
            (self.mir.types.get(from_ty), self.mir.types.get(to_ty))
        else {
            return Ok(value_text.clone());
        };
        let (from, to) = (from_function.clone(), to_function.clone());
        let Some(abis) = self.record_slot_param_abis(&to, &from) else {
            return Ok(value_text.clone());
        };
        let slot_text = self.rust_type(to_ty, false, to_env)?.into_string();
        let param_decls = self
            .callback_arg_decls(&to, to_env, MutablePrefix::Apply)?
            .join(", ");
        let forwarded = abis
            .iter()
            .enumerate()
            .map(|(index, abi)| {
                if let (Some(to_param), Some(from_param)) =
                    (to.params.get(index), from.params.get(index))
                    && self.nested_slot_callables(*to_param, *from_param).is_some()
                {
                    let owned = if abi.declared_is_reference {
                        RustExpr::path(format!("smelt_arg{index}")).cloned_value()
                    } else {
                        RustExpr::path(format!("smelt_arg{index}"))
                    };
                    let bridged =
                        self.slot_abi_bridge(&owned, (*to_param, to_env), (*from_param, from_env))?;
                    return Ok(if abi.rendered_is_reference {
                        format!("&{bridged}", bridged = bridged.tight())
                    } else {
                        bridged.into_string()
                    });
                }
                Ok(match (abi.declared_is_reference, abi.rendered_is_reference) {
                    // The slot hands the body a borrow, the wrapped callable
                    // wants the value: copy it out, per call, only where the
                    // two genuinely disagree.
                    (true, false) => format!("smelt_arg{index}.clone()"),
                    // The slot hands the body an owned value and the wrapped
                    // callable takes a borrow of it; Rust extends the borrow to
                    // the end of the statement, so naming it is enough.
                    (false, true) => format!("&smelt_arg{index}"),
                    _ => format!("smelt_arg{index}"),
                })
            })
            .collect::<Result<Vec<_>, EmitError>>()?
            .join(", ");
        let call = RustExpr::atom(format!("(smelt_slot_callback)({forwarded})"));
        let returned = if !from.may_throw
            && !to.may_throw
            && self.nested_slot_callables(from.return_ty, to.return_ty).is_some()
        {
            self.slot_abi_bridge(&call, (from.return_ty, from_env), (to.return_ty, to_env))?
        } else {
            call
        };
        Ok(RustExpr::atom(format!(
            "{{ let smelt_slot_callback = {value_text}; let smelt_slot_adapted: {slot_text} = \
             ::std::rc::Rc::new(move |{param_decls}| {returned}); \
             smelt_slot_adapted }}", value_text = value_text.delimited(), returned = returned.delimited()
        )))
    }

    /// An argument for parameter `index` of a call through a record slot,
    /// re-wrapped at the slot's DECLARED ABI when that parameter is a callable
    /// whose nested ABI the record's instantiation changed.
    ///
    /// The call-side sibling of [`Self::record_slot_abi_adapter`]. The top
    /// level `&`/value choice is already taken from the declaration by the
    /// caller (`class_field_declared_function_type`); this covers what that
    /// cannot: a callback argument whose own parameters the declaration passes
    /// by reference (`m.otherwise((v) => v + 1)` against `otherwise: (handler:
    /// (value: i) => R) => R` at `i = number` needs `Fn(&f64)`, the argument
    /// was rendered `Fn(f64)`). Answers `rendered` unchanged otherwise.
    pub(super) fn slot_argument_at_declared_abi(
        &self,
        callee: &Operand,
        index: usize,
        substituted_param: TypeId,
        rendered: RustExpr,
    ) -> Result<RustExpr, EmitError> {
        let (Operand::Copy(Place::Field { base, field })
        | Operand::Move(Place::Field { base, field })) = callee
        else {
            return Ok(rendered);
        };
        let base_ty = self.local_decl(*base)?.ty;
        let Some((declared_ty, bindings)) = self.declared_record_field_slot(base_ty, *field) else {
            return Ok(rendered);
        };
        let Some(Type::Function(declared)) = self.mir.types.get(declared_ty) else {
            return Ok(rendered);
        };
        let Some(declared_param) = declared.params.get(index).copied() else {
            return Ok(rendered);
        };
        if self
            .nested_slot_callables(substituted_param, declared_param)
            .is_none()
        {
            return Ok(rendered);
        }
        let caller_scope = self.current_function_type_params();
        let slot = TypeSubstitution::lexical(&caller_scope).with_bindings(&bindings);
        let value_side = TypeSubstitution::lexical(&caller_scope);
        self.slot_abi_bridge(
            &rendered,
            (substituted_param, &value_side),
            (declared_param, &slot),
        )
    }

    /// Whether a value read off `source_field` of the record `source` is
    /// already at the slot ABI `target_field` of `target` declares.
    ///
    /// True when both records instantiate the SAME declared slot (one
    /// declaration, so one Rust field type). The caller asks only when the
    /// field's coercion passed the read through untouched, so the value is the
    /// stored callable as it is, which the generated struct already holds at
    /// the declared ABI. Wrapping it in
    /// [`Self::record_slot_abi_adapter`] would treat it as rendered at the
    /// substituted ABI and forward the wrong side of the `&`/`.clone()` seam
    /// (E0308 on Hono's `Router<[unknown, RouterRoute]>` rebuilt from an
    /// `options.router ?? new SmartRouter(..)` value).
    pub(super) fn record_slot_read_keeps_slot_abi(
        &self,
        source: TypeId,
        source_field: &MirField,
        target: TypeId,
        target_field: &MirField,
    ) -> bool {
        let (Some((source_declared, _)), Some((target_declared, _))) = (
            self.declared_record_field_slot(source, source_field.name),
            self.declared_record_field_slot(target, target_field.name),
        ) else {
            return false;
        };
        source_declared == target_declared
    }

    /// Apply [`Self::record_slot_abi_adapter`] to a field value, returning
    /// the original text when no adaptation is needed.
    ///
    /// The shape every record adapter's field loop wants: one call, one string
    /// back, no branch at the call site.
    pub(super) fn record_field_value_at_slot_abi(
        &self,
        value_text: String,
        record_ty: TypeId,
        field: &MirField,
    ) -> Result<RustExpr, EmitError> {
        Ok(self
            .record_slot_abi_adapter(&RustExpr::opaque(&value_text), record_ty, field)?
            .unwrap_or_else(|| RustExpr::from(value_text)))
    }
}
