//! A computed-key write into a class instance: `obj[key] = value`.
//!
//! JavaScript does not distinguish `obj.get = f` from `obj[name] = f` with
//! `name === 'get'`; the key is simply a runtime string. Hono's `HonoBase`
//! constructor installs every HTTP-verb handler that way:
//!
//! ```ts
//! allMethods.forEach((method) => { this[method] = (args1, ...args) => { .. } })
//! ```
//!
//! A class instance in generated Rust is a struct with one field per member,
//! so the write is a dispatch on the key over the fields the value can be
//! stored in — what a hand port would write as
//! `match key { "get" => self.get = f, "post" => self.post = f, .. }`. A key
//! that names no such field is an expando property the struct has no slot for;
//! it is discarded, which is what every class-index write without an index
//! signature did before (the whole write used to be discarded, so a verb
//! handler installed this way was never callable: `app.get(..)` ran the
//! field's default closure and registered no route).
//!
//! A field is a candidate when the value is assignable to it without a lossy
//! conversion: the same type, or a function value stored into a
//! callable-interface field (a record whose `__smelt_call` slot holds the
//! call, which is how `HandlerInterface`-typed members are emitted). A
//! function field of a DIFFERENT signature is not a candidate: the key's
//! static type (a union of member names in the source) is erased to `string`
//! here, and only the fields the value's own signature fits can be the ones
//! that union named. Private (`#name`) fields are not reachable by a computed
//! key and are never candidates.

use smelt_hir::{Type, TypeId};
use smelt_mir::{LocalId, Operand, Rvalue};

use super::{EmitError, FunctionEmitter};
use smelt_mir::Place;

impl FunctionEmitter<'_> {
    /// Emit `base[index] = value` for a class-typed `base` as a match on the
    /// runtime key over the candidate fields (module docs). Returns `None`
    /// when the value is not a plain operand or no field can hold it, leaving
    /// the caller's existing fallback.
    pub(super) fn class_keyed_field_write_text(
        &self,
        base: LocalId,
        base_ty: TypeId,
        index: &Operand,
        value: &Rvalue,
    ) -> Result<Option<String>, EmitError> {
        let Rvalue::Use(value_operand) = value else {
            return Ok(None);
        };
        let Some(fields) = self.structural_record_fields(base_ty) else {
            return Ok(None);
        };
        let value_ty = self.operand_ty(value_operand)?;
        let value_is_function = matches!(self.mir.types.get(value_ty), Some(Type::Function(_)));
        let scope = self.render_scope();
        let mut arms = Vec::new();
        for field in fields {
            let key = self.symbol_source_name(field.name)?;
            if key.starts_with('#') || key.starts_with("__smelt") {
                continue;
            }
            let assignable = field.ty == value_ty
                || (value_is_function && self.is_callable_interface_record(field.ty))
                || self.same_call_signature(value_ty, field.ty);
            if !assignable {
                continue;
            }
            let lvalue = self.assignment_place_text(&Place::Field {
                base,
                field: field.name,
            })?;
            let converted =
                self.value_at_type_text("smelt_keyed_value.clone()", value_ty, field.ty, &scope)?;
            arms.push(format!("{key:?} => {{ {lvalue} = {converted}; }}"));
        }
        if arms.is_empty() {
            return Ok(None);
        }
        let index_ty = self.operand_ty(index)?;
        let key_text = self.property_key_to_string_text(&self.operand_text(index)?, index_ty)?;
        Ok(Some(format!(
            "{{ let smelt_keyed_value = {}; let smelt_key: String = {key_text}; match smelt_key.as_str() {{ {} _ => {{}} }} }}",
            self.operand_text(value_operand)?,
            arms.join(" ")
        )))
    }

    /// Whether two function types take the same parameters and return the
    /// same type (their throw/arity metadata may differ: a closure literal
    /// records what its body does, a field annotation what it declares).
    fn same_call_signature(&self, value: TypeId, field: TypeId) -> bool {
        match (self.mir.types.get(value), self.mir.types.get(field)) {
            (Some(Type::Function(value_fn)), Some(Type::Function(field_fn))) => {
                value_fn.params == field_fn.params
                    && value_fn.return_ty == field_fn.return_ty
                    && value_fn.is_async == field_fn.is_async
            }
            _ => false,
        }
    }

    /// Whether `ty` is a callable interface: a record with a `__smelt_call`
    /// slot.
    fn is_callable_interface_record(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Class { .. }) => self.structural_record_fields(ty).is_some_and(|fields| {
                fields.iter().any(|field| {
                    self.symbol_source_name(field.name)
                        .is_ok_and(|name| name == "__smelt_call")
                })
            }),
            _ => false,
        }
    }
}
