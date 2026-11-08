//! Structural field lists read from annotations: the fields of a type literal,
//! a type reference, a union, or a static `Pick`.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::js_text::{has_lone_surrogate, js_str_text};
use indexmap::IndexMap;
use oxc::ast::ast::{TSSignature, TSType, TSTypeName};
use smelt_hir::{ErasureReason, Field, Type, Visibility};

impl ModuleBuilder<'_> {
    /// Extract named fields from a TypeScript structural type.
    pub(in crate::lowering) fn type_fields_from_ts(&mut self, ty: &TSType<'_>) -> Result<Vec<Field>, SmeltError> {
        match ty {
            TSType::TSTypeLiteral(literal) => self.type_literal_fields(literal),
            TSType::TSIntersectionType(intersection) => {
                let mut fields = Vec::new();
                for member in &intersection.types {
                    fields.extend(self.type_fields_from_ts(member)?);
                }
                Ok(fields)
            }
            TSType::TSUnionType(union) => self.union_type_fields(union),
            TSType::TSTypeReference(reference) => {
                let TSTypeName::IdentifierReference(name) = &reference.type_name else {
                    return Ok(Vec::new());
                };
                self.type_reference_fields(reference, name.name.as_str())
            }
            _ => Ok(Vec::new()),
        }
    }

    /// Extract structural fields from a named type reference.
    ///
    /// This mirrors `type_reference_to_hir` for field metadata: aliases and
    /// interfaces carry structural fields independently from their erased HIR
    /// representation, so references need generic substitutions applied before
    /// callers use the fields for member access.
    pub(in crate::lowering) fn type_reference_fields(
        &mut self,
        reference: &oxc::ast::ast::TSTypeReference<'_>,
        name_text: &str,
    ) -> Result<Vec<Field>, SmeltError> {
        let args = reference
            .type_arguments
            .as_ref()
            .map(|args| args.params.iter().collect::<Vec<_>>())
            .unwrap_or_default();
        if name_text == "Pick" {
            let [base, keys] = args.as_slice() else {
                return Ok(Vec::new());
            };
            let mut fields = self.type_fields_from_ts(base)?;
            if let Some(selected) = Self::static_pick_keys(keys) {
                fields.retain(|field| {
                    self.ctx
                        .krate
                        .symbols
                        .get(field.name)
                        .is_some_and(|name| selected.iter().any(|key| key == name))
                });
            }
            return Ok(fields);
        }

        // `Omit<T, K>`: the base's fields minus the named keys. A key set that
        // is not statically known leaves the base untouched, which is the safe
        // direction -- a field that should have been removed still resolves,
        // where dropping the whole table makes every field vanish.
        if name_text == "Omit" {
            let [base, keys] = args.as_slice() else {
                return Ok(Vec::new());
            };
            let mut fields = self.type_fields_from_ts(base)?;
            if let Some(removed) = Self::static_pick_keys(keys) {
                fields.retain(|field| {
                    !self
                        .ctx
                        .krate
                        .symbols
                        .get(field.name)
                        .is_some_and(|name| removed.iter().any(|key| key == name))
                });
            }
            return Ok(fields);
        }
        // `Required<T>` / `Partial<T>` keep the base's fields and change only
        // their optionality; `Readonly<T>` changes neither. All three are
        // transparent to a field TABLE, which is what a construction site reads.
        if matches!(name_text, "Required" | "Partial" | "Readonly") {
            let [base] = args.as_slice() else {
                return Ok(Vec::new());
            };
            let mut fields = self.type_fields_from_ts(base)?;
            match name_text {
                "Required" => {
                    for field in &mut fields {
                        field.optional = false;
                    }
                }
                "Partial" => {
                    for field in &mut fields {
                        field.optional = true;
                    }
                }
                _ => {}
            }
            return Ok(fields);
        }

        let symbol = self.intern_type_name(name_text);
        // An ambient fetch init is not in the crate, so neither lookup below
        // can see it. Its fields are declared in the shared registry, and
        // reading them here is what lets every utility above compose over one:
        // `Required<Omit<RequestInit, ..>>` is `Omit`'s retain over this table,
        // with `Required`'s optionality applied on top.
        if let Some(fields) = self.ambient_fetch_init_fields(name_text) {
            return Ok(fields);
        }
        if let Some(interface) = self.find_interface(symbol).cloned()
            && !self.alias_shadows_interface(symbol, name_text)
        {
            let lowered_args = args
                .iter()
                .map(|arg| self.ts_type_to_hir(arg))
                .collect::<Result<Vec<_>, _>>()?;
            let substitutions = self.type_argument_substitution(
                &interface.type_params,
                &lowered_args,
                self.span(reference.span.start, reference.span.end),
            )?;
            return Ok(self.substituted_fields(&interface.fields, &substitutions));
        }
        if let Some(alias) = self.visible_type_alias(symbol, name_text).cloned() {
            let lowered_args = args
                .iter()
                .map(|arg| self.ts_type_to_hir(arg))
                .collect::<Result<Vec<_>, _>>()?;
            let substitutions = self.type_argument_substitution(
                &alias.type_params,
                &lowered_args,
                self.span(reference.span.start, reference.span.end),
            )?;
            if let Some(fields) = self.types.alias_fields(symbol).cloned() {
                return Ok(self.substituted_fields(&fields, &substitutions));
            }
        }
        Ok(self.types.alias_fields(symbol)
            .cloned()
            .unwrap_or_default())
    }

    /// Return static string keys from the key side of `Pick<T, K>`.
    pub(in crate::lowering) fn static_pick_keys(ty: &TSType<'_>) -> Option<Vec<String>> {
        match ty {
            TSType::TSLiteralType(literal) => match &literal.literal {
                oxc::ast::ast::TSLiteral::StringLiteral(value) if !has_lone_surrogate(value.value) => {
                    Some(vec![js_str_text(value.value).into_owned()])
                }
                _ => None,
            },
            TSType::TSUnionType(union) => {
                let mut keys = Vec::new();
                for item in &union.types {
                    keys.extend(Self::static_pick_keys(item)?);
                }
                Some(keys)
            }
            _ => None,
        }
    }

    /// Extract fields common to a union of object-like type surfaces.
    pub(in crate::lowering) fn union_type_fields(
        &mut self,
        union: &oxc::ast::ast::TSUnionType<'_>,
    ) -> Result<Vec<Field>, SmeltError> {
        let variants = union
            .types
            .iter()
            .map(|member| self.type_fields_from_ts(member))
            .collect::<Result<Vec<_>, _>>()?;
        let mut grouped: IndexMap<smelt_hir::Symbol, Vec<Field>> = IndexMap::new();
        for fields in &variants {
            for field in fields {
                grouped.entry(field.name).or_default().push(field.clone());
            }
        }
        let mut out = Vec::new();
        for (name, fields) in grouped {
            let mut value_tys = Vec::new();
            let mut optional = fields.len() != variants.len();
            let span = fields
                .first()
                .map_or_else(|| self.span(union.span.start, union.span.end), |field| field.span);
            for field in fields {
                optional |= field.optional;
                if !value_tys.contains(&field.ty) {
                    value_tys.push(field.ty);
                }
            }
            let ty = match value_tys.as_slice() {
                [single] => *single,
                [] => self.unknown_type(), // not a new erasure: unreachable, every grouped field contributes a type
                _ => self.ctx.krate.types.intern(Type::Union(value_tys)),
            };
            out.push(Field {
                name,
                ty,
                visibility: Visibility::Public,
                optional,
                span,
            });
        }
        Ok(out)
    }

    /// Extract named fields from a TypeScript type literal.
    pub(in crate::lowering) fn type_literal_fields(
        &mut self,
        literal: &oxc::ast::ast::TSTypeLiteral<'_>,
    ) -> Result<Vec<Field>, SmeltError> {
        let mut fields = Vec::new();
        for member in &literal.members {
            let TSSignature::TSPropertySignature(prop) = member else {
                continue;
            };
            // A computed field keeps its declared name when the key statically
            // resolves (`[K]`, `[E.Member]`, `[Symbol.iterator]`); a genuinely
            // dynamic key names no static field and is skipped.
            if prop.computed && !self.is_resolvable_property_key(&prop.key) {
                continue;
            }
            let Ok(name) = self.property_key_symbol(&prop.key) else {
                continue;
            };
            let ty = prop
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?
                .unwrap_or_else(|| self.erase(ErasureReason::MissingAnnotation));
            fields.push(Field {
                name,
                ty,
                visibility: Visibility::Public,
                optional: prop.optional,
                span: self.span(prop.span.start, prop.span.end),
            });
        }
        Ok(fields)
    }
}
