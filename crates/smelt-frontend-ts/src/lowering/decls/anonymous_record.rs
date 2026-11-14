//! Anonymous heterogeneous object types lower to a synthesized struct.
//!
//! An inline object type in a type position (`options: { times?: number;
//! backoff?: (n: number) => number }`) has no source name. Smelt lowers such a
//! literal to `Dict<String, V>`, where `V` is the flat union of the field
//! types. That is a faithful layout only when one value type describes every
//! field (`{ a: string; b?: string }` is `Dict<String, Optional<String>>`).
//! When the fields have *different* types the merged `V` is a union that no
//! field actually has: reading `options.times` yields
//! `Optional<Float | fn(Float) -> Float>`, so a number field turns into a
//! number-or-function value and every use of it loses its type.
//!
//! The rule, stated once: a plain-data type literal (named property
//! signatures only: no index signature, call or construct signature, or
//! method) whose fields do not share one value type lowers to a synthesized
//! interface, exactly as the alias spelling `type O = { … }` does. Its name is
//! derived from the structure, so the same shape written in several places, in
//! one file or in several, shares one generated struct.
//!
//! A literal that mentions type parameters (`{ value: V; rest: string }`, as
//! in `type ParseResult<V> = { value: V; rest: string } | null`) lowers to a
//! GENERIC synthesized struct: it declares the parameters it mentions, in
//! first-mention order, and the reference passes them through as its type
//! arguments (`SmeltRecord…<V>`). Substituting the enclosing scope's
//! parameters (an alias instantiation `ParseResult<number>`, a generic call)
//! then instantiates the struct (`SmeltRecord…<f64>`), as a hand-written
//! `struct ParseResult<V> { value: V, rest: String }` would be. The `Dict`
//! lowering it replaces merged `V | string` into every field, and an alias
//! reference over it resolved to a nominal record no struct backs, so the
//! value erased to `SmeltUnknown`. Derived records (`Partial<T>`, `Pick<..>`)
//! bind their parameters the same way (`type_params_mentioned`).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use oxc::ast::ast::{TSSignature, TSTypeLiteral};
use smelt_hir::{Field, Type};

use crate::lowering::ModuleBuilder;

use super::types_iface::StructuralMembers;

impl ModuleBuilder<'_> {
    /// Lower a heterogeneous plain-data type literal to its synthesized struct.
    ///
    /// `fields` are the literal's fields as `type_literal_fields` returned them
    /// (the declared type, not yet wrapped in `Optional` for an optional
    /// field). Returns `None`, leaving the caller's
    /// `Dict` lowering in charge, when the fields share one value type or when
    /// the literal declares anything other than named property signatures. A
    /// literal whose fields mention type parameters yields a generic struct
    /// reference (see the module docs).
    pub(in crate::lowering) fn anonymous_record_literal_type(
        &mut self,
        literal: &TSTypeLiteral<'_>,
        fields: &[Field],
    ) -> Option<smelt_hir::TypeId> {
        // Optionality is a per-field fact a `Dict` value can still carry
        // (`Optional<V>`); only differing value types make it lossy.
        let mut value_tys = Vec::new();
        for field in fields {
            let mut ty = field.ty;
            while let Some(Type::Optional(inner)) = self.ctx.krate.types.get(ty) {
                ty = *inner;
            }
            if !value_tys.contains(&ty) {
                value_tys.push(ty);
            }
        }
        if value_tys.len() < 2 {
            return None;
        }
        let plain_properties = literal.members.iter().all(|member| {
            matches!(member, TSSignature::TSPropertySignature(prop)
                if !prop.computed || self.is_resolvable_property_key(&prop.key))
        });
        if !plain_properties || fields.len() != literal.members.len() {
            return None;
        }
        // An interface field of an optional property stores `Optional<T>`, as
        // `lower_structural_members` records it for the alias spelling.
        let fields = fields
            .iter()
            .map(|field| {
                let ty = if field.optional
                    && !matches!(self.ctx.krate.types.get(field.ty), Some(Type::Optional(_)))
                {
                    self.ctx.krate.types.intern(Type::Optional(field.ty))
                } else {
                    field.ty
                };
                Field { ty, ..field.clone() }
            })
            .collect::<Vec<_>>();
        let decode_facts = literal
            .members
            .iter()
            .zip(&fields)
            .filter_map(|(member, field)| {
                let TSSignature::TSPropertySignature(prop) = member else {
                    return None;
                };
                let annotation = prop.type_annotation.as_ref()?;
                Some(self.field_decode_facts(
                    field.name,
                    prop.optional,
                    &annotation.type_annotation,
                ))
            })
            .collect();
        let minted = self.anonymous_record_name(&fields);
        let name = self.intern_synthesized_type_name(&minted);
        let name_text = minted.as_str().to_owned();
        let span = self.span(0, 0);
        // The struct is generic over exactly the parameters its fields name;
        // the reference forwards them, so the enclosing scope binds them.
        let type_params = self.type_params_mentioned(&fields, span);
        let args = type_params
            .iter()
            .map(|param| self.ctx.krate.types.intern(Type::TypeParam { name: param.name }))
            .collect();
        if self.find_interface(name).is_none() {
            let members = StructuralMembers {
                fields,
                decode_facts,
                ..Default::default()
            };
            let item =
                self.register_callable_object_interface(name, &name_text, type_params, members, span);
            self.items.insert(name_text, item);
        }
        Some(self.ctx.krate.types.intern(Type::Class { name, args }))
    }

    /// Derive the generated name of an anonymous record from its structure.
    ///
    /// The name is a pure function of the field names, interned types and
    /// optionality, so it is stable across the modules of one build and equal
    /// shapes deduplicate onto one struct. The `SmeltRecord` prefix cannot
    /// collide with a source type name reaching this path, since the source
    /// declared no name for the shape.
    fn anonymous_record_name(&self, fields: &[Field]) -> smelt_hir::SynthesizedName {
        let mut hasher = DefaultHasher::new();
        for field in fields {
            self.ctx.krate.symbols.get(field.name).hash(&mut hasher);
            field.ty.hash(&mut hasher);
            field.optional.hash(&mut hasher);
        }
        smelt_hir::SynthesizedName::new(format!("SmeltRecord{:016x}", hasher.finish()))
    }
}
