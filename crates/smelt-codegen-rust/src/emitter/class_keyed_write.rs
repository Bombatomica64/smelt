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
//! that names no such field is an expando property the struct has no slot for,
//! and is discarded.
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

use crate::rust::RustExpr;
use smelt_hir::{Type, TypeId};
use smelt_mir::{LocalId, Operand, Rvalue};
use smelt_stdlib::runtime_symbols::markers::SMELT_CALL;

use super::{EmitError, FunctionEmitter};
use smelt_mir::Place;

impl FunctionEmitter<'_> {
    /// Emit `base[index] = value` for a class-typed `base` as a match on the
    /// runtime key over the candidate fields (module docs). Returns `None`
    /// when the value is not a plain operand or no field can hold it, leaving
    /// the caller's existing fallback.
    pub(super) fn class_keyed_field_write_expr(
        &self,
        base: LocalId,
        base_ty: TypeId,
        index: &Operand,
        value: &Rvalue,
    ) -> Result<Option<RustExpr>, EmitError> {
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
            // A value typed by the union of the record's property types (`v:
            // S[K]` with `K extends keyof S`, whose base constraint is that
            // union) is stored into each field its union has an arm for,
            // projected to that arm: TypeScript ties the value's type to the
            // key's, so the arm the key selects is the one the value holds.
            let assignable = field.ty == value_ty
                || (value_is_function && self.is_callable_interface_record(field.ty))
                || self.same_call_signature(value_ty, field.ty)
                || self
                    .concrete_union_members(value_ty)
                    .is_some_and(|members| members.contains(&field.ty));
            if !assignable {
                continue;
            }
            let lvalue = self.assignment_place_expr(&Place::Field {
                base,
                field: field.name,
            })?;
            let converted = self.rendered_at_type(
                &RustExpr::path("smelt_keyed_value").cloned_value(),
                value_ty,
                field.ty,
                &scope,
            )?;
            arms.push(format!(
                "{key:?} => {{ {lvalue} = {converted}; }}",
                lvalue = lvalue.delimited(),
                converted = converted.delimited()
            ));
        }
        if arms.is_empty() {
            return Ok(None);
        }
        let key_text = self.record_key_text(index)?;
        Ok(Some(RustExpr::atom(format!(
            "{{ let smelt_keyed_value = {}; let smelt_key = {key_text}; match smelt_key.as_str() {{ {} _ => {{}} }} }}",
            self.operand_expr(value_operand)?.delimited(),
            arms.join(" "),
        ))))
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
                        .is_ok_and(|name| name == SMELT_CALL)
                })
            }),
            _ => false,
        }
    }
}
