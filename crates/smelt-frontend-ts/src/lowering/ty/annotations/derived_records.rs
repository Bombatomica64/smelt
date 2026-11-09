//! Records derived from another record by TypeScript's homomorphic utility
//! types: `Pick`, `Omit`, `Partial`, `Required`.
//!
//! A hand-written port gives `Pick<User, "id">` its own struct holding only
//! `id`. Lowering the utility to its base type instead makes the value claim
//! fields it does not have, so `Object.keys`, `JSON.stringify` and spread all
//! report them. Every derivation here is one rule: take the base's own
//! properties, keep some, and fix each kept one's optionality. The shape, not
//! the utility's name, identifies the generated struct, so `Pick<X, "a">` and
//! `Omit<X, "b">` of a two-field `X` are one type.
//!
//! A mapped type has exactly the named properties of its base: call and
//! construct signatures do not survive it, so Smelt's `__smelt*` slots are
//! never carried over. A base with an index signature, or one Smelt holds no
//! member table for (a type parameter, a host type), derives nothing and the
//! caller keeps its fallback.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::decls::types_iface::StructuralMembers;
use oxc::ast::ast::TSType;
use smelt_hir::{Field, FieldDecodeFacts, Item, Symbol, Type, TypeId, TypeParamDef, Visibility};
use smelt_ts_model::{TsTypeId, TsTypeKind};
use std::collections::HashMap;

/// One property of a derived record: its name and whether it may be absent.
type ShapeEntry = (Symbol, bool);

/// The own properties a record can be derived from.
struct RecordSource {
    /// Source spelling of the base type, used to name the generated struct.
    name: String,
    /// Own and inherited properties with type arguments applied, base-most last
    /// overridden by nearer declarations.
    fields: Vec<Field>,
    /// What the base's declaration records per property beyond its storage.
    decode_facts: Vec<FieldDecodeFacts>,
}

impl ModuleBuilder<'_> {
    /// The record `Partial<base>` names: every property may be absent.
    ///
    /// `None` keeps the caller's fallback (see the module docs); a base whose
    /// properties are all optional already is its own `Partial`.
    pub(in crate::lowering) fn partial_record_type(
        &mut self,
        base: TypeId,
        span: smelt_hir::Span,
    ) -> Result<Option<TypeId>, SmeltError> {
        self.derived_record(base, "Partial", span, |_, _| Some(true))
    }

    /// The record `Required<base>` names: no property may be absent.
    pub(in crate::lowering) fn required_record_type(
        &mut self,
        base: TypeId,
        span: smelt_hir::Span,
    ) -> Result<Option<TypeId>, SmeltError> {
        self.derived_record(base, "Required", span, |_, _| Some(false))
    }

    /// The record `Pick<base, keys>` names: only the listed properties.
    pub(in crate::lowering) fn picked_record_type(
        &mut self,
        base: TypeId,
        keys: &[String],
        span: smelt_hir::Span,
    ) -> Result<Option<TypeId>, SmeltError> {
        self.derived_record(base, "Pick", span, |name, field| {
            keys.iter().any(|key| key == name).then_some(field.optional)
        })
    }

    /// The record `Omit<base, keys>` names: all but the listed properties.
    pub(in crate::lowering) fn omitted_record_type(
        &mut self,
        base: TypeId,
        keys: &[String],
        span: smelt_hir::Span,
    ) -> Result<Option<TypeId>, SmeltError> {
        self.derived_record(base, "Omit", span, |name, field| {
            (!keys.iter().any(|key| key == name)).then_some(field.optional)
        })
    }

    /// The property names a `K` type argument fixes, when it fixes them.
    ///
    /// A literal or a union of literals is read from the syntax; anything else
    /// that resolves to one (an alias, `keyof X`) is read from the checker. A
    /// `K` still free at this position answers `None`: the call site resolves
    /// it instead ([`Self::call_result_shaped_by_checker`]).
    pub(in crate::lowering) fn literal_record_keys(&self, keys: &TSType<'_>) -> Option<Vec<String>> {
        if let Some(found) = Self::static_pick_keys(keys) {
            return Some(found);
        }
        let facts = self.ts_facts?;
        let span = oxc::span::GetSpan::span(keys);
        let id = facts.type_node_id_at(&self.path, span.start, span.end)?;
        let mut found = Vec::new();
        self.collect_literal_keys(id, &mut found).then_some(found)
    }

    /// Append the property names checker type `id` denotes; `false` when it
    /// denotes something other than a fixed set of keys.
    fn collect_literal_keys(&self, id: TsTypeId, found: &mut Vec<String>) -> bool {
        let Some(facts) = self.ts_facts else {
            return false;
        };
        match facts.types.get(id).map(|ty| &ty.kind) {
            Some(TsTypeKind::StringLiteral(key)) => {
                found.push(key.clone());
                true
            }
            Some(TsTypeKind::NumberLiteral(key)) => {
                found.push(key.to_string());
                true
            }
            Some(TsTypeKind::Never) => true,
            Some(TsTypeKind::Union { members, .. }) => {
                members.iter().all(|member| self.collect_literal_keys(*member, found))
            }
            _ => false,
        }
    }

    /// `result` reshaped to the properties the checker says the call produced.
    ///
    /// Where a call fixes `K` the checker has already evaluated
    /// `Pick<T, K>`/`Omit<T, K>`/`Partial<T>` for that call and expanded it to
    /// an object type. The definition stays generic or erased; the call site
    /// converts its result to this record. The rule reads only the checker's
    /// property set, so it covers every mapped return type, however spelled,
    /// and leaves a result whose properties are the record's own untouched.
    pub(in crate::lowering) fn call_result_shaped_by_checker(
        &mut self,
        result: TypeId,
        start: u32,
        end: u32,
        span: smelt_hir::Span,
    ) -> Result<TypeId, SmeltError> {
        let id = self.ts_facts.and_then(|facts| facts.type_id_at(&self.path, start, end));
        self.shaped_by_checker_type(result, id, span)
    }

    /// `alias_ty`, the body of a generic alias instantiated at a reference
    /// spanning `start..end`, reshaped to what the checker made of that
    /// reference: `type P<K extends keyof X> = Pick<X, K>` is fixed only by the
    /// `P<"a">` that names it.
    pub(in crate::lowering) fn alias_reference_shaped_by_checker(
        &mut self,
        alias_ty: TypeId,
        start: u32,
        end: u32,
        span: smelt_hir::Span,
    ) -> Result<TypeId, SmeltError> {
        let id = self.ts_facts.and_then(|facts| facts.type_node_id_at(&self.path, start, end));
        self.shaped_by_checker_type(alias_ty, id, span)
    }

    /// `result` reshaped to checker type `id`, when that is an object type
    /// whose properties are all properties of the record `result` names.
    fn shaped_by_checker_type(
        &mut self,
        result: TypeId,
        id: Option<TsTypeId>,
        span: smelt_hir::Span,
    ) -> Result<TypeId, SmeltError> {
        let Some(facts) = self.ts_facts else {
            return Ok(result);
        };
        let Some(TsTypeKind::Object(object)) = id.and_then(|id| facts.types.get(id)).map(|ty| &ty.kind) else {
            return Ok(result);
        };
        let properties: HashMap<&str, bool> =
            object.properties.iter().map(|property| (property.name.as_str(), property.optional)).collect();
        let count = properties.len();
        // `Omit<T, K>` has no base-typed lowering while `T` is free, so its
        // instantiation still names the utility: the record is its first argument.
        let base = match self.ctx.krate.types.get(result) {
            Some(Type::Class { name, args })
                if self.ctx.krate.symbols.get(*name) == Some("Omit")
                    && self.resolve_type_name("Omit") == crate::lowering::resolve::TypeBinding::Lib =>
            {
                args.first().copied().unwrap_or(result)
            }
            _ => result,
        };
        // An anonymous object type is a dictionary here, whose keys are exactly
        // the ones the definition left: no property set to narrow.
        if matches!(self.ctx.krate.types.get(base), Some(Type::Dict(..))) {
            return Ok(base);
        }
        let Some(source) = self.record_source(base, span)? else {
            return Ok(result);
        };
        let known = source
            .fields
            .iter()
            .filter(|field| {
                self.ctx.krate.symbols.get(field.name).is_some_and(|name| properties.contains_key(name))
            })
            .count();
        if known != count {
            return Ok(result);
        }
        let derived = self.derived_record(base, "Pick", span, |name, _| properties.get(name).copied())?;
        Ok(derived.unwrap_or(result))
    }

    /// The record derived from `base` by `select`, which answers each property's
    /// optionality or `None` to drop it; cached crate-wide per shape so every
    /// module spelling the same derivation names the same struct.
    fn derived_record(
        &mut self,
        base: TypeId,
        label: &str,
        span: smelt_hir::Span,
        select: impl Fn(&str, &Field) -> Option<bool>,
    ) -> Result<Option<TypeId>, SmeltError> {
        let Some(source) = self.record_source(base, span)? else {
            return Ok(None);
        };
        let mut shape: Vec<ShapeEntry> = Vec::with_capacity(source.fields.len());
        let mut selected = Vec::with_capacity(source.fields.len());
        let mut base_is_mapped_shape = true;
        for field in &source.fields {
            let name = self.ctx.krate.symbols.get(field.name).unwrap_or_default();
            if name.starts_with("__smelt") {
                base_is_mapped_shape = false;
                continue;
            }
            let Some(optional) = select(name, field) else {
                base_is_mapped_shape = false;
                continue;
            };
            base_is_mapped_shape &= optional == field.optional;
            shape.push((field.name, optional));
            selected.push((field.clone(), optional));
        }
        if base_is_mapped_shape {
            return Ok(Some(base));
        }
        let key = (base, shape);
        if let Some(cached) = self.ctx.derived_records.get(&key) {
            return Ok(Some(*cached));
        }
        let fields: Vec<Field> = selected
            .into_iter()
            .map(|(field, optional)| self.with_optionality(&field, optional, &source.decode_facts))
            .collect();
        let decode_facts = fields
            .iter()
            .map(|field| {
                let declared = source.decode_facts.iter().find(|facts| facts.name == field.name).cloned();
                FieldDecodeFacts {
                    may_be_absent: field.optional,
                    ..declared.unwrap_or(FieldDecodeFacts {
                        name: field.name,
                        may_be_absent: field.optional,
                        admits_null: false,
                        literals: Vec::new(),
                    })
                }
            })
            .collect();
        let type_params = self.type_params_mentioned(&fields, span);
        let args = type_params
            .iter()
            .map(|param| self.ctx.krate.types.intern(Type::TypeParam { name: param.name }))
            .collect();
        let mut name_text = format!("{label}{}", source.name);
        let mut ordinal = 1_usize;
        while self.declared_type_name_exists(&name_text) {
            name_text = format!("{label}{}_{ordinal}", source.name);
            ordinal = ordinal.saturating_add(1);
        }
        let symbol = self.intern_type_name(&name_text);
        let members = StructuralMembers { fields, decode_facts, ..Default::default() };
        self.push_synthesized_interface(symbol, name_text, type_params, members, span);
        let derived = self.ctx.krate.types.intern(Type::Class { name: symbol, args });
        self.ctx.derived_records.insert(key, derived);
        Ok(Some(derived))
    }

    /// `field` with its optionality set, adjusting the storage type to match.
    ///
    /// A property made required stops being `Option` unless the declaration
    /// admits `null` itself: `a?: T | null` under `Required` is still nullable.
    fn with_optionality(&mut self, field: &Field, optional: bool, facts: &[FieldDecodeFacts]) -> Field {
        let inner = match self.ctx.krate.types.get(field.ty) {
            Some(Type::Optional(inner)) => Some(*inner),
            _ => None,
        };
        let ty = match (optional, inner) {
            (true, None) => self.ctx.krate.types.intern(Type::Optional(field.ty)),
            (false, Some(inner))
                if field.optional
                    && !facts.iter().any(|fact| fact.name == field.name && fact.admits_null) =>
            {
                inner
            }
            _ => field.ty,
        };
        Field { ty, optional, ..field.clone() }
    }

    /// The property table of `base`, when it is a class or interface record.
    fn record_source(
        &mut self,
        base: TypeId,
        span: smelt_hir::Span,
    ) -> Result<Option<RecordSource>, SmeltError> {
        let Some(Type::Class { name, args }) = self.ctx.krate.types.get(base).cloned() else {
            return Ok(None);
        };
        let text = self.ctx.krate.symbols.get(name).unwrap_or("Record").to_owned();
        if self.find_interface(name).is_some() {
            if self.interfaces.index_value(name).is_some() {
                return Ok(None);
            }
            let mut fields = Vec::new();
            let mut decode_facts = Vec::new();
            self.collect_interface_properties(name, &args, span, &mut fields, &mut decode_facts)?;
            return Ok(Some(RecordSource { name: text, fields, decode_facts }));
        }
        if self.class_by_symbol(name).is_none() {
            return Ok(None);
        }
        let fields = self.class_properties(base, name)?;
        Ok(Some(RecordSource { name: text, fields, decode_facts: Vec::new() }))
    }

    /// Append `name`'s properties, then those it inherits that it does not
    /// redeclare, with each interface's type arguments applied.
    fn collect_interface_properties(
        &mut self,
        name: Symbol,
        args: &[TypeId],
        span: smelt_hir::Span,
        fields: &mut Vec<Field>,
        decode_facts: &mut Vec<FieldDecodeFacts>,
    ) -> Result<(), SmeltError> {
        let Some(interface) = self.find_interface(name).cloned() else {
            return Ok(());
        };
        let substitutions = self.type_argument_substitution(&interface.type_params, args, span)?;
        for field in self.substituted_fields(&interface.fields, &substitutions) {
            if !fields.iter().any(|known| known.name == field.name) {
                fields.push(field);
            }
        }
        decode_facts.extend(interface.decode_facts.iter().cloned());
        for parent in &interface.extends {
            let parent_args: Vec<TypeId> = parent
                .args
                .iter()
                .map(|arg| self.substitute_type_params(*arg, &substitutions))
                .collect();
            self.collect_interface_properties(parent.parent, &parent_args, span, fields, decode_facts)?;
        }
        Ok(())
    }

    /// The public properties of class type `base`: fields and methods alike,
    /// its own and inherited, each with the type a read of it has.
    fn class_properties(&mut self, base: TypeId, name: Symbol) -> Result<Vec<Field>, SmeltError> {
        let mut members: Vec<(Symbol, bool, smelt_hir::Span)> = Vec::new();
        let mut next = Some(name);
        while let Some(current) = next {
            let Some(class) = self.class_by_symbol(current) else {
                break;
            };
            for field in &class.fields {
                if field.visibility == Visibility::Public && !members.iter().any(|member| member.0 == field.name) {
                    members.push((field.name, field.optional, field.span));
                }
            }
            for item in &class.methods {
                if let Item::Function(function) = self.item_ref(*item)
                    && !members.iter().any(|member| member.0 == function.name)
                {
                    members.push((function.name, false, function.span));
                }
            }
            next = class.base;
        }
        members
            .into_iter()
            .map(|(member, optional, span)| {
                let ty = self.class_field_type(base, member)?;
                Ok(Field { name: member, ty, visibility: Visibility::Public, optional, span })
            })
            .collect()
    }

    /// The type parameters `fields` mention, in first-mention order, declared
    /// on the derived record so it stays a real Rust generic.
    fn type_params_mentioned(&self, fields: &[Field], span: smelt_hir::Span) -> Vec<TypeParamDef> {
        let mut names = Vec::new();
        for field in fields {
            self.collect_type_params(field.ty, &mut names);
        }
        names
            .into_iter()
            .map(|name| TypeParamDef { name, constraint: None, default: None, span })
            .collect()
    }

    /// Append the type parameter names occurring in `ty`.
    fn collect_type_params(&self, ty: TypeId, names: &mut Vec<Symbol>) {
        match self.ctx.krate.types.get(ty) {
            Some(Type::TypeParam { name }) => {
                if !names.contains(name) {
                    names.push(*name);
                }
            }
            Some(other) => {
                for child in other.children() {
                    self.collect_type_params(child, names);
                }
            }
            None => {}
        }
    }
}
