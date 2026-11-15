//! Indexed access over a type parameter (`T[K]`, `T[keyof T]`), kept unevaluated in a signature's return and evaluated per call.
//!
//! # The rule
//!
//! `function pluck<T, K extends keyof T>(obj: T, key: K): T[K]` promises a
//! type that depends on what a CALL binds `T` and `K` to. At the declaration
//! there is nothing to evaluate, so the annotation lowering erases the access
//! (reason `type-level`), and every call used to inherit that erased result.
//!
//! The return annotation of a function declaration or overload signature is
//! therefore lowered a second time for its CALLERS, with each such access kept
//! as [`Type::IndexedAccess`] ([`ModuleBuilder::deferred_return_type`]). A
//! call binds the signature's type parameters as it always does, and the
//! access is then evaluated against the bound object
//! ([`ModuleBuilder::evaluated_deferred_return`]):
//!
//! | object | key | result |
//! | --- | --- | --- |
//! | declared record (interface, class, object type) | a string literal argument for `K` | that property's read type |
//! | declared record without methods | `keyof T`, or a `K` no literal fixes | the union of its properties' read types |
//! | `Record<string, V>` | any | `V` |
//!
//! Every other combination is not evaluated and the call keeps the erased
//! declared return, exactly as before the rule. So does a call whose
//! evaluation would say nothing (`unknown` among the property types).
//!
//! Where the TypeScript 7 checker resolved the very same call, its answer
//! replaces the evaluation whenever the two are comparable: it is narrower
//! when it knows a key Smelt widened to `string` (a `const` key, an explicit
//! type argument), and wider when Smelt promised more than the source does.
//!
//! # Requirements
//!
//! - Behaviour: unchanged. The declaration is lowered and emitted exactly as
//!   before (the access erased), so the callee runs the same code; a call
//!   whose result is now typed reads that erased result through the checked
//!   conversion every typed call of an erased callee already uses (an
//!   overload promising `number` over an `any` implementation).
//! - Type preservation: the result of a call that binds the object to a
//!   record or a dictionary has the type TypeScript gives it
//!   (`pluck(point, "x")` is `number`), instead of `unknown`. No value that
//!   was typed becomes erased: the rule only ever replaces an erased call
//!   result.
//!
//! # Where the type lives
//!
//! [`Type::IndexedAccess`] exists only in the caller-side return this module
//! produces: [`crate::OverloadSignature::deferred_return`] for an overload,
//! [`crate::HirCtx::deferred_returns`] for a function item. It is never the
//! type of a value. The signature the item is emitted with, its body, and
//! every other reader of an overload (selection, argument hints, an
//! unannotated implementation inheriting the last overload's return) keep
//! the erased lowering.
//!
//! # Erasure records
//!
//! An implementation is emitted with the access erased, so its `type-level`
//! record stays at the declaration. An overload signature emits nothing: the
//! record of an access this module kept is dropped at the signature and made
//! where it does erase something, at each call that selects the overload and
//! cannot evaluate it, and at an unannotated implementation that takes the
//! erased return.
//!
//! # Scope
//!
//! Return positions only. A parameter position (`predicate: (value: T[keyof
//! T]) => boolean`) is typed by the implementation's own parameter, which the
//! emitted function declares erased; typing it per call needs the callee
//! itself generic over the property type (a trait with an associated type).

use std::collections::HashMap;

use oxc::ast::ast::{
    Argument, TSType, TSTypeAnnotation, TSTypeName, TSTypeOperatorOperator, UnaryOperator,
};
use oxc::ast_visit::{Visit, walk};
use smelt_hir::{ErasureReason, Symbol, Type, TypeId, TypeParamDef};

use crate::lowering::ModuleBuilder;
use crate::lowering::js_text::{has_lone_surrogate, js_str_text};

/// Whether a type annotation spells an indexed access anywhere.
#[derive(Default)]
struct IndexedAccessFinder {
    /// Set once an indexed access type is seen.
    found: bool,
}

impl<'a> Visit<'a> for IndexedAccessFinder {
    fn visit_ts_indexed_access_type(&mut self, it: &oxc::ast::ast::TSIndexedAccessType<'a>) {
        self.found = true;
        walk::walk_ts_indexed_access_type(self, it);
    }
}

/// The keys a call's literal arguments fix, by the type parameter each
/// argument's declared type names.
pub(in crate::lowering) type LiteralKeys = HashMap<Symbol, String>;

impl ModuleBuilder<'_> {
    /// The return annotation `annotation` as its callers see it: lowered with
    /// every indexed access over a type parameter kept unevaluated. `None`
    /// when the annotation holds no such access, which is every annotation
    /// the ordinary lowering already says everything about.
    ///
    /// Must run inside the declaration's type-parameter scope. Leaves no
    /// erasure record: the caller decides which lowering's records describe
    /// the declaration (see the module docs).
    pub(in crate::lowering) fn deferred_return_type(
        &mut self,
        annotation: &TSTypeAnnotation<'_>,
    ) -> Option<TypeId> {
        let mut finder = IndexedAccessFinder::default();
        finder.visit_ts_type_annotation(annotation);
        if !finder.found {
            return None;
        }
        let records_before = self.ctx.krate.erasures.len();
        let outer = std::mem::replace(&mut self.defer_indexed_access, true);
        let lowered = self.ts_type_to_hir(&annotation.type_annotation);
        self.defer_indexed_access = outer;
        self.ctx.krate.erasures.truncate(records_before);
        lowered.ok().filter(|ty| self.mentions_indexed_access(*ty))
    }

    /// [`Self::deferred_return_type`] for an overload signature, keeping the
    /// erasure records that describe the signature.
    ///
    /// Returns the erased lowering (what every reader but a call's result
    /// uses) and the deferred one. With a deferred return, the records left
    /// are the deferred lowering's: everything the annotation erases except
    /// the accesses a call evaluates.
    pub(in crate::lowering) fn overload_return_types(
        &mut self,
        annotation: &TSTypeAnnotation<'_>,
    ) -> Result<(TypeId, Option<TypeId>), crate::SmeltError> {
        let mut finder = IndexedAccessFinder::default();
        finder.visit_ts_type_annotation(annotation);
        if !finder.found {
            return Ok((self.ts_type_to_hir(&annotation.type_annotation)?, None));
        }
        let records_before = self.ctx.krate.erasures.len();
        let outer = std::mem::replace(&mut self.defer_indexed_access, true);
        let deferred = self.ts_type_to_hir(&annotation.type_annotation);
        self.defer_indexed_access = outer;
        let deferred = deferred.ok().filter(|ty| self.mentions_indexed_access(*ty));
        let deferred_records = self.ctx.krate.erasures.split_off(records_before);
        let erased = self.ts_type_to_hir(&annotation.type_annotation)?;
        if deferred.is_some() {
            self.ctx.krate.erasures.truncate(records_before);
            self.ctx.krate.erasures.extend(deferred_records);
        }
        Ok((erased, deferred))
    }

    /// The unevaluated access `object[index]`, when deferral is on and the
    /// access is one a call can evaluate: `object` is a type parameter (or an
    /// access already deferred, `T[A][B]`) and `index` is `keyof` that same
    /// object, or a name that lowers to a type parameter or a string-like
    /// type. `None` leaves the access to the ordinary lowering. (`T[number]`
    /// over an array-constrained parameter never reaches here: the parameter
    /// is carried as its element, `ty/element_params.rs`.)
    pub(in crate::lowering) fn deferred_indexed_access(
        &mut self,
        object: TypeId,
        index: &TSType<'_>,
    ) -> Result<Option<TypeId>, crate::SmeltError> {
        if !self.defer_indexed_access
            || !matches!(
                self.ctx.krate.types.get(object),
                Some(Type::TypeParam { .. } | Type::IndexedAccess { .. })
            )
        {
            return Ok(None);
        }
        let index = match index {
            TSType::TSTypeOperatorType(operator)
                if operator.operator == TSTypeOperatorOperator::Keyof =>
            {
                if self.ts_type_to_hir(&operator.type_annotation)? != object {
                    return Ok(None);
                }
                self.ctx.krate.types.intern(Type::String)
            }
            TSType::TSTypeReference(reference)
                if reference.type_arguments.is_none()
                    && matches!(reference.type_name, TSTypeName::IdentifierReference(_)) =>
            {
                let lowered = self.ts_type_to_hir(index)?;
                if !matches!(
                    self.ctx.krate.types.get(lowered),
                    Some(Type::TypeParam { .. } | Type::String | Type::PropKey)
                ) {
                    return Ok(None);
                }
                lowered
            }
            _ => return Ok(None),
        };
        Ok(Some(
            self.ctx
                .krate
                .types
                .intern(Type::IndexedAccess { object, index }),
        ))
    }

    /// Whether `ty` holds an unevaluated indexed access anywhere.
    pub(in crate::lowering) fn mentions_indexed_access(&self, ty: TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::IndexedAccess { .. }) => true,
            Some(other) => other
                .children()
                .any(|child| self.mentions_indexed_access(child)),
            None => false,
        }
    }

    /// The property keys the literal arguments of a call fix: for each
    /// declared parameter whose type is a bare type parameter, the string (or
    /// number, as its property-key spelling) literal passed for it.
    pub(in crate::lowering) fn literal_key_arguments_of(
        &self,
        params: &[TypeId],
        arguments: &[Argument<'_>],
    ) -> LiteralKeys {
        let mut keys = LiteralKeys::new();
        for (param, argument) in params.iter().zip(arguments) {
            let Some(Type::TypeParam { name }) = self.ctx.krate.types.get(*param) else {
                continue;
            };
            let key = match argument {
                Argument::StringLiteral(literal) if !has_lone_surrogate(literal.value) => {
                    Some(js_str_text(literal.value).into_owned())
                }
                Argument::NumericLiteral(literal) => {
                    Some(Self::numeric_property_key_name(literal.value))
                }
                Argument::UnaryExpression(unary)
                    if unary.operator == UnaryOperator::UnaryNegation =>
                {
                    match &unary.argument {
                        oxc::ast::ast::Expression::NumericLiteral(literal) => {
                            Some(Self::numeric_property_key_name(-literal.value))
                        }
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some(key) = key {
                keys.insert(*name, key);
            }
        }
        keys
    }

    /// The deferred return `deferred` evaluated for one call: its type
    /// parameters replaced by `substitutions`, every indexed access evaluated
    /// (see the module table), then reconciled with the checker's type for
    /// the call at `span`.
    ///
    /// `None` when an access cannot be evaluated or a parameter of
    /// `type_params` is still unbound: the call keeps the erased declared
    /// return. Leaves no erasure record.
    pub(in crate::lowering) fn evaluated_deferred_return(
        &mut self,
        deferred: TypeId,
        type_params: &[TypeParamDef],
        substitutions: &HashMap<Symbol, TypeId>,
        keys: &LiteralKeys,
        span: oxc::span::Span,
    ) -> Option<TypeId> {
        let records_before = self.ctx.krate.erasures.len();
        let evaluated = self
            .evaluate_deferred(deferred, substitutions, keys, 0)
            .filter(|ty| !self.mentions_type_params(*ty, type_params));
        let result = evaluated.map(|evaluated| {
            let resolved = self
                .checker_call_type(span.start, span.end)
                .filter(|resolved| self.says_something(*resolved));
            match resolved {
                Some(resolved)
                    if self.type_accepts_all_of(evaluated, resolved)
                        || self.type_accepts_all_of(resolved, evaluated) =>
                {
                    resolved
                }
                _ => evaluated,
            }
        });
        self.ctx.krate.erasures.truncate(records_before);
        result
    }

    /// `ty` with `substitutions` applied and every indexed access evaluated;
    /// `None` when one cannot be.
    ///
    /// A subtree holding no access is substituted exactly as every other
    /// instantiation substitutes it. Around an access, optionals and unions
    /// are re-joined from their evaluated members, because the access may
    /// evaluate to an optional or a union itself.
    fn evaluate_deferred(
        &mut self,
        ty: TypeId,
        substitutions: &HashMap<Symbol, TypeId>,
        keys: &LiteralKeys,
        depth: usize,
    ) -> Option<TypeId> {
        if depth > MAX_DEPTH {
            return None;
        }
        if !self.mentions_indexed_access(ty) {
            return Some(self.substitute_type_params(ty, substitutions));
        }
        match self.ctx.krate.types.get(ty).cloned()? {
            Type::IndexedAccess { object, index } => {
                let object = self.evaluate_deferred(object, substitutions, keys, depth + 1)?;
                let literal = match self.ctx.krate.types.get(index) {
                    Some(Type::TypeParam { name }) => keys.get(name).cloned(),
                    _ => None,
                };
                let index = self.substitute_type_params(index, substitutions);
                self.indexed_access_of(object, index, literal.as_deref())
            }
            Type::Optional(inner) => {
                let inner = self.evaluate_deferred(inner, substitutions, keys, depth + 1)?;
                let none = self.ctx.krate.types.intern(Type::None);
                Some(self.union_of_member_types(&[inner, none]))
            }
            Type::Union(members) => {
                let members = members
                    .into_iter()
                    .map(|member| self.evaluate_deferred(member, substitutions, keys, depth + 1))
                    .collect::<Option<Vec<_>>>()?;
                Some(self.union_of_member_types(&members))
            }
            other => {
                let rebuilt = other.try_map_children(|child| {
                    self.evaluate_deferred(child, substitutions, keys, depth + 1)
                })?;
                Some(self.ctx.krate.types.intern(rebuilt))
            }
        }
    }

    /// The type of `object[index]` for a bound `object`, per the module table;
    /// `None` for every combination the table does not list.
    ///
    /// `literal` is the property a call's literal argument names, when the
    /// index was a type parameter one fixed.
    fn indexed_access_of(
        &mut self,
        object: TypeId,
        index: TypeId,
        literal: Option<&str>,
    ) -> Option<TypeId> {
        let any_key = matches!(
            self.ctx.krate.types.get(index),
            Some(Type::String | Type::PropKey)
        );
        let evaluated = match self.ctx.krate.types.get(object).cloned()? {
            Type::Dict(key, value)
                if (any_key || literal.is_some())
                    && matches!(
                        self.ctx.krate.types.get(key),
                        Some(Type::String | Type::PropKey)
                    ) =>
            {
                value
            }
            Type::Class { name, .. } if any_key || literal.is_some() => {
                let span = self.erasure_anchor;
                let source = self.record_source(object, span).ok()??;
                match literal {
                    Some(key) => {
                        let field = source
                            .fields
                            .iter()
                            .find(|field| self.ctx.krate.symbols.get(field.name) == Some(key))?;
                        self.field_read_type(field)
                    }
                    None if self.record_declares_methods(name) => return None,
                    None => {
                        let members: Vec<TypeId> = source
                            .fields
                            .iter()
                            .map(|field| self.field_read_type(field))
                            .collect();
                        if members.is_empty() {
                            return None;
                        }
                        self.union_of_member_types(&members)
                    }
                }
            }
            _ => return None,
        };
        self.says_something(evaluated).then_some(evaluated)
    }

    /// The type a read of `field` produces: optional when the property may be
    /// absent.
    fn field_read_type(&mut self, field: &smelt_hir::Field) -> TypeId {
        if field.optional {
            smelt_hir::type_normalize::optional_of(&mut self.ctx.krate.types, field.ty)
        } else {
            field.ty
        }
    }

    /// Whether the class or interface `name`, or one it extends, declares a
    /// method: a member `keyof` names whose type the property table of
    /// [`Self::record_source`] may not list.
    fn record_declares_methods(&self, name: Symbol) -> bool {
        if let Some(interface) = self.find_interface(name) {
            return !interface.methods.is_empty()
                || interface
                    .extends
                    .iter()
                    .any(|parent| self.record_declares_methods(parent.parent));
        }
        let mut next = Some(name);
        while let Some(current) = next {
            let Some(class) = self.class_by_symbol(current) else {
                break;
            };
            if !class.methods.is_empty() {
                return true;
            }
            next = class.base;
        }
        false
    }

    /// Whether `ty` states a type: it is not `unknown`, and not a union with
    /// an `unknown` arm (which admits every value, so says no more).
    fn says_something(&self, ty: TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Unknown) | None => false,
            Some(Type::Union(members)) => members
                .iter()
                .all(|member| !matches!(self.ctx.krate.types.get(*member), Some(Type::Unknown))),
            Some(Type::Optional(inner)) => {
                !matches!(self.ctx.krate.types.get(*inner), Some(Type::Unknown))
            }
            Some(_) => true,
        }
    }

    /// Record that a call selecting an overload could not evaluate its
    /// deferred return and keeps the erased one (see the module docs,
    /// "Erasure records").
    pub(in crate::lowering) fn note_unevaluated_deferred_return(&mut self, span: oxc::span::Span) {
        let span = self.span(span.start, span.end);
        let _erased = self.erase_at(
            ErasureReason::TypeLevel,
            span,
            "indexed access not evaluable at this call",
        );
    }
}

/// Nesting depth beyond which an evaluation gives up.
const MAX_DEPTH: usize = 16;
