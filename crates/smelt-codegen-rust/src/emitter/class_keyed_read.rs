//! A computed-key read of a declared record with a `keyof` key: `s[k]`.
//!
//! ```ts
//! interface S { volume: number; name: string }
//! function get<K extends keyof S>(s: S, k: K): S[K] { return s[k]; }
//! ```
//!
//! TypeScript accepts a computed read of a record without an index signature
//! only when the key is typed as the record's keys (`keyof S`, or a parameter
//! `K extends keyof S`). The frontend types such a read as the base constraint
//! `S[keyof S]`, the union of the record's property types
//! (`deferred_index::keyof_record_read_type`). A record is a struct here, so
//! the read is a dispatch on the key over its fields, what a hand port would
//! write as `match k { "volume" => V::Number(s.volume), "name" => V::String(s.name.clone()) }`.
//!
//! # Requirements
//!
//! - Behaviour: the read yields the value of the property the key names, as
//!   JavaScript does. Before this rule a key typed by a type parameter read
//!   `undefined` unconditionally (`get(s, "volume")` printed `undefined`).
//! - Type preservation: the read is typed as the union of the field types
//!   (one field type when they agree), each arm injected into it; no erased
//!   value is involved when the key is a `string`.
//!
//! # Scope
//!
//! The record declares no method, accessor or index signature (`keyof` then
//! names only its data fields), every field type is a plain type (no optional
//! or union field, whose read type the union would have to flatten), and the
//! union of the field types is one the frontend interned. Every other receiver
//! keeps the existing index-read lowering. A key outside the record's fields
//! is unreachable for a program TypeScript accepted, so it panics with the
//! key, rather than inventing an `undefined` the read type cannot hold.

use crate::rust::RustExpr;
use smelt_hir::{Type, TypeId};
use smelt_mir::{LocalId, MirField, Operand, Place};

use super::{EmitError, FunctionEmitter};

/// The decided read: the type it yields and the fields it dispatches over.
pub(super) struct ClassKeyedRead {
    /// The type the read yields: the one field type, or their union.
    pub(super) read_ty: TypeId,
    /// The data fields a key can name, in declaration order.
    fields: Vec<MirField>,
}

impl FunctionEmitter<'_> {
    /// The keyed read of `base_ty` by `index`, when the module's rule applies.
    /// Decided once for both the read's type (`place_type::index_place_ty`) and
    /// its rendering ([`Self::class_keyed_read_expr`]).
    pub(super) fn class_keyed_read(
        &self,
        base_ty: TypeId,
        index: &Operand,
    ) -> Result<Option<ClassKeyedRead>, EmitError> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty) else {
            return Ok(None);
        };
        let class_name = *name;
        if !self.keyof_key(self.operand_ty(index)?)
            || self.is_match_class_symbol(class_name)?
            || self.class_index_store_types(base_ty).is_some()
            || self.is_typed_array_view_class_type(base_ty)?
            || self.record_has_non_field_members(class_name)
        {
            return Ok(None);
        }
        let Some(record_fields) = self.structural_record_fields(base_ty) else {
            return Ok(None);
        };
        let mut fields = Vec::new();
        let mut field_tys: Vec<TypeId> = Vec::new();
        for field in record_fields {
            let key = self.symbol_source_name(field.name)?;
            if key.starts_with('#') || key.starts_with("__smelt") {
                continue;
            }
            if matches!(
                self.mir.types.get(field.ty),
                None | Some(
                    Type::Optional(..) | Type::Union(_) | Type::Unknown | Type::TypeParam { .. }
                )
            ) {
                return Ok(None);
            }
            if !field_tys.contains(&field.ty) {
                field_tys.push(field.ty);
            }
            fields.push(field);
        }
        let read_ty = match field_tys.as_slice() {
            [] => return Ok(None),
            [single] => *single,
            _ => match self.mir.types.lookup(&Type::Union(field_tys)) {
                Some(union) => union,
                None => return Ok(None),
            },
        };
        Ok(Some(ClassKeyedRead { read_ty, fields }))
    }

    /// Render the decided read of `base[index]`: a match on the key's text
    /// over the record's fields, each field injected into the read type.
    pub(super) fn class_keyed_read_expr(
        &self,
        read: &ClassKeyedRead,
        base: LocalId,
        index: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let scope = self.render_scope();
        let mut arms = Vec::new();
        for field in &read.fields {
            let key = self.symbol_source_name(field.name)?;
            let value = self.place_expr(&Place::Field {
                base,
                field: field.name,
            })?;
            let injected =
                self.rendered_at_type(&value.cloned_value(), field.ty, read.read_ty, &scope)?;
            arms.push(format!("{key:?} => {},", injected.delimited()));
        }
        let key_text = self.record_key_text(index)?;
        Ok(RustExpr::atom(format!(
            "{{ let smelt_key = {key_text}; match smelt_key.as_str() {{ {} smelt_key => unreachable!(\"`{{smelt_key}}` is not a key of the record\"), }} }}",
            arms.join(" ")
        )))
    }

    /// The property-key text of a computed record key, as an owned
    /// `String` expression a `match` on `.as_str()` reads.
    pub(super) fn record_key_text(&self, index: &Operand) -> Result<String, EmitError> {
        let index_ty = self.operand_ty(index)?;
        let index_text = self.operand_expr(index)?;
        // A key typed by a type parameter the signature erased arrives as the
        // runtime carrier; its property-key text is `smelt_property_key`.
        if self.renders_erased(index_ty)? {
            return Ok(format!(
                "smelt_property_key({})",
                index_text.cloned_value().delimited()
            ));
        }
        Ok(format!(
            "({}).to_string()",
            self.property_key_to_string_expr(&index_text, index_ty)?
                .delimited()
        ))
    }

    /// Whether a key of type `key` is typed as a record's keys: a `string`
    /// (or property key), or a type parameter constrained by one, which is
    /// how `keyof S` and `K extends keyof S` reach the backend.
    fn keyof_key(&self, key: TypeId) -> bool {
        let key_ty = match self.mir.types.get(key) {
            Some(Type::TypeParam { name }) => {
                match self
                    .function
                    .type_params
                    .iter()
                    .find(|param| param.name == *name)
                    .and_then(|param| param.constraint)
                {
                    Some(constraint) => constraint,
                    None => return false,
                }
            }
            _ => key,
        };
        matches!(
            self.mir.types.get(key_ty),
            Some(Type::String | Type::PropKey)
        )
    }

    /// Whether the class or interface `name` (or one it extends) declares a
    /// member `keyof` names that is not a data field: a method, an accessor.
    fn record_has_non_field_members(&self, name: smelt_hir::Symbol) -> bool {
        if let Some(class) = crate::classes::find_class(self.mir, name) {
            return !class.methods.is_empty()
                || !class.abstract_methods.is_empty()
                || !class.descriptors.is_empty()
                || class
                    .base
                    .is_some_and(|base| self.record_has_non_field_members(base));
        }
        match self
            .mir
            .interfaces
            .iter()
            .find(|interface| interface.name == name)
        {
            Some(interface) => {
                !interface.methods.is_empty()
                    || interface
                        .extends
                        .iter()
                        .any(|parent| self.record_has_non_field_members(parent.parent))
            }
            None => true,
        }
    }
}
