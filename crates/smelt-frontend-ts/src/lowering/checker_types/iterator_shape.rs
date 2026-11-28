//! The iterator-protocol type of an unannotated protocol method, from the
//! structural type the TypeScript checker infers for it.
//!
//! `[Symbol.iterator]() { return { next: () => ({ value: n, done: false }) } }`
//! has no annotation, so its result is the object literal's own type,
//! `{ next: () => { value: number; done: boolean } }`, which Smelt would lower
//! as an erased record. TypeScript iterates it all the same: the checker reads
//! the iteration types off that structure (`getIterationTypesOfIterator`),
//! taking the yield type from the steps whose `done` is not `true` and the
//! return type from the steps whose `done` is not `false`.
//!
//! This module applies the same rule and spells the result as the lib type an
//! annotation would name, `Iterator<T, R>` for `[Symbol.iterator]()` and
//! `IteratorResult<T, R>` for `next()`, lowered through the ordinary annotation
//! path (see the parent module's re-spelling rule). The method then has
//! exactly the type the annotation `[Symbol.iterator](): Iterator<T, R>` gives
//! it, and its body's literals take the protocol lowering
//! (`lowering::stmt::iterator_protocol`).
//!
//! Requirements: the element type is the one TypeScript computes for a
//! `for...of` over the instance (`T`, never erased: a part the re-spelling
//! declines keeps the method's erased return, as before); behaviour is that of
//! the annotated method.

use super::Respeller;
use crate::lowering::ModuleBuilder;
use oxc::ast::ast::{MethodDefinition, PropertyKey, TSType, TSTypeName};
use oxc::ast_visit::{Visit, walk};
use oxc::span::GetSpan;
use smelt_hir::TypeId;
use smelt_ts_model::{TsTypeFacts, TsTypeId, TsTypeKind};

/// Which protocol shape a method's result is read as.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ProtocolShape {
    /// An iterator: an object whose `next()` answers steps (`[Symbol.iterator]()`).
    Iterator,
    /// One step `{ value, done }` (`next()`).
    Step,
}

/// The value types of a step union: those of the steps that may yield, and of
/// those that may complete.
#[derive(Default)]
struct StepValues {
    /// Values of the steps whose `done` is not `true` (`None`: no `value`,
    /// so `undefined`).
    yields: Vec<Option<TsTypeId>>,
    /// Values of the steps whose `done` is not `false`.
    returns: Vec<Option<TsTypeId>>,
}

/// The members of `id`: its union members, or itself.
fn members(facts: &TsTypeFacts, id: TsTypeId) -> Vec<TsTypeId> {
    match facts.types.get(id).map(|ty| &ty.kind) {
        Some(TsTypeKind::Union { members, .. }) => members.clone(),
        _ => vec![id],
    }
}

/// The value types of `id` read as a step type: each member an object with a
/// boolean `done` and no property besides `value`. A member without `value`
/// carries `undefined`. `None` for any other type.
fn step_values(facts: &TsTypeFacts, id: TsTypeId) -> Option<StepValues> {
    let mut values = StepValues::default();
    for member in members(facts, id) {
        let Some(TsTypeKind::Object(object)) = facts.types.get(member).map(|ty| &ty.kind) else {
            return None;
        };
        if !object.call_signatures.is_empty()
            || !object.construct_signatures.is_empty()
            || !object.index_signatures.is_empty()
        {
            return None;
        }
        let mut value = None;
        let mut done = None;
        for property in &object.properties {
            match property.name.as_str() {
                "value" => value = Some(property.ty),
                "done" => done = Some(property.ty),
                _ => return None,
            }
        }
        let (may_yield, may_complete) =
            match done.map(|done| facts.types.get(done).map(|ty| &ty.kind)) {
                // A record without `done` is no step (and JSON shows the
                // difference): only the protocol's own shape is read as one.
                None => return None,
                Some(Some(TsTypeKind::BooleanLiteral(true))) => (false, true),
                Some(Some(TsTypeKind::BooleanLiteral(false))) => (true, false),
                Some(Some(TsTypeKind::Boolean)) => (true, true),
                Some(_) => return None,
            };
        if may_yield {
            values.yields.push(value);
        }
        if may_complete {
            values.returns.push(value);
        }
    }
    (!values.yields.is_empty() || !values.returns.is_empty()).then_some(values)
}

/// The one call signature of `id` when it takes no required argument: its
/// return type.
fn nullary_return(facts: &TsTypeFacts, id: TsTypeId) -> Option<TsTypeId> {
    let Some(TsTypeKind::Object(object)) = facts.types.get(id).map(|ty| &ty.kind) else {
        return None;
    };
    let [signature] = object.call_signatures.as_slice() else {
        return None;
    };
    (signature.min_argument_count == 0 && signature.type_parameters.is_empty())
        .then_some(signature.return_type)
}

/// The step type of `id` read as an iterator: an object whose `next` is a
/// nullary function, with no other members than `return`, `throw` and
/// `[Symbol.iterator]` (each a function). State members are not part of the
/// shape: the object literal lowering decides those.
fn iterator_step(facts: &TsTypeFacts, id: TsTypeId) -> Option<TsTypeId> {
    let Some(TsTypeKind::Object(object)) = facts.types.get(id).map(|ty| &ty.kind) else {
        return None;
    };
    let mut step = None;
    for property in &object.properties {
        match property.name.as_str() {
            "next" => step = Some(nullary_return(facts, property.ty)?),
            "return" | "throw"
                if matches!(
                    facts.types.get(property.ty).map(|ty| &ty.kind),
                    Some(TsTypeKind::Object(member)) if !member.call_signatures.is_empty()
                ) => {}
            name if name.starts_with("__@iterator") => {}
            _ => return None,
        }
    }
    step
}

impl ModuleBuilder<'_> {
    /// The `Iterator<T, R>` an unannotated, non-generator
    /// `[Symbol.iterator]()` answers, or the `IteratorResult<T, R>` an
    /// unannotated `next()` answers, as the checker infers its result's
    /// structure (see the module docs). `None` for any other method, without
    /// checker facts, or when the structure is no protocol shape or a value
    /// type cannot be spelled here.
    pub(in crate::lowering) fn checker_method_protocol_type(
        &mut self,
        method: &MethodDefinition<'_>,
    ) -> Option<TypeId> {
        if method.value.generator || method.value.r#async || method.value.return_type.is_some() {
            return None;
        }
        let shape = if method.computed && self.is_symbol_iterator_key(&method.key) {
            ProtocolShape::Iterator
        } else if !method.computed
            && matches!(&method.key, PropertyKey::StaticIdentifier(name) if name.name == "next")
        {
            ProtocolShape::Step
        } else {
            return None;
        };
        let facts = self.ts_facts?;
        // The checker types a method at its declaration name: the key, which
        // for a computed key is the bracketed name around the key expression.
        let key = method.key.span();
        let key_span = if method.computed {
            (key.start.saturating_sub(1), key.end.saturating_add(1))
        } else {
            (key.start, key.end)
        };
        let spans = [key_span, (method.value.span.start, method.value.span.end)];
        let (start, end) = (method.value.span.start, method.value.span.end);
        let returned = spans.into_iter().find_map(|(from, to)| {
            nullary_return(facts, facts.type_id_at(&self.path, from, to)?)
        })?;
        // `return this;` infers the polymorphic `this`, which an annotation
        // `: this` spells: the enclosing class (the checker's constraint of
        // `this`), an iterator when its own `next()` is one
        // (`iteration_generator` asks).
        if shape == ProtocolShape::Iterator
            && let Some(TsTypeKind::TypeParameter {
                is_this: true,
                constraint: Some(class),
                ..
            }) = facts.types.get(returned).map(|ty| &ty.kind)
        {
            let text = Respeller {
                builder: self,
                facts,
                annotation: (start, end),
                records: false,
            }
            .respell(*class, 0)?;
            return self
                .lower_respelled_annotation(&text, start, end)
                .filter(|ty| {
                    matches!(
                        self.ctx.krate.types.get(*ty),
                        Some(smelt_hir::Type::Class { .. })
                    )
                });
        }
        let step = match shape {
            ProtocolShape::Iterator => iterator_step(facts, returned)?,
            ProtocolShape::Step => returned,
        };
        let values = step_values(facts, step)?;
        let respeller = Respeller {
            builder: self,
            facts,
            annotation: (start, end),
            records: false,
        };
        // The values' union, spelled; `None` when every value is `undefined`
        // (the lib's open `TReturn`, which Smelt reads as `undefined`).
        let is_undefined = |ty: &Option<TsTypeId>| {
            ty.is_none_or(|ty| {
                matches!(
                    facts.types.get(ty).map(|ty| &ty.kind),
                    Some(TsTypeKind::Undefined | TsTypeKind::Void)
                )
            })
        };
        let spell = |types: &[Option<TsTypeId>]| -> Option<Option<String>> {
            let mut spelled: Vec<String> = Vec::new();
            for ty in types.iter().filter(|ty| !is_undefined(ty)).flatten() {
                let text = respeller.respell(*ty, 0)?;
                if !spelled.contains(&text) {
                    spelled.push(text);
                }
            }
            let undefined_too = types.iter().any(is_undefined);
            if spelled.is_empty() {
                return Some(None);
            }
            if undefined_too {
                spelled.push("undefined".to_owned());
            }
            Some(Some(spelled.join(" | ")))
        };
        let yield_text = spell(&values.yields)?.unwrap_or_else(|| "undefined".to_owned());
        let return_text = spell(&values.returns)?;
        let name = match shape {
            ProtocolShape::Iterator => "Iterator",
            ProtocolShape::Step => "IteratorResult",
        };
        let text = return_text.map_or_else(
            || format!("{name}<{yield_text}>"),
            |completion| format!("{name}<{yield_text}, {completion}>"),
        );
        let records_before = self.ctx.krate.erasures.len();
        let lowered = self.lower_respelled_annotation(&text, start, end);
        if lowered.is_none_or(|ty| self.type_mentions_unknown(ty)) {
            self.ctx.krate.erasures.truncate(records_before);
            return None;
        }
        lowered
    }

    /// `annotated`, the lowered return annotation of `function`, with the
    /// step's `TReturn` taken from the body when the annotation is the lib's
    /// `IteratorResult<T>` (or `IteratorResult<T, any>`), which leaves
    /// `TReturn` at `any`.
    ///
    /// Smelt reads an open `TReturn` as `undefined`, which dropped the value
    /// of `return { value: 0, done: true }` (Node keeps it: `next()` answers
    /// `{ value: 0, done: true }`). The checker types each step the body
    /// returns as the literal it is (`{ value: number; done: true }`), so the
    /// return type is the union of the values of the returned steps whose
    /// `done` is not `false`, by the rule of an unannotated `next()` (module
    /// docs). The annotation's `any` is assignable to that type, so the method
    /// still satisfies `Iterator<T>`. A body whose completions are all
    /// `undefined`, a returned value that is no step literal type, a value
    /// type that does not lower without erasure, or no checker facts keep
    /// `annotated`: there the open `TReturn` is the dynamic boundary the
    /// source wrote, and its value is not kept (see
    /// `docs/design/iterator-protocol.md`, section 5).
    pub(in crate::lowering) fn open_step_return_type(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
        annotation: &TSType<'_>,
        annotated: TypeId,
    ) -> TypeId {
        let Some(smelt_hir::Type::GeneratorResult {
            yield_ty,
            return_ty,
        }) = self.ctx.krate.types.get(annotated).cloned()
        else {
            return annotated;
        };
        if function.generator
            || function.r#async
            || !matches!(
                self.ctx.krate.types.get(return_ty),
                Some(smelt_hir::Type::None)
            )
            || !leaves_return_open(annotation)
        {
            return annotated;
        }
        let (Some(facts), Some(body)) = (self.ts_facts, function.body.as_ref()) else {
            return annotated;
        };
        let mut returns = ReturnedExpressions::default();
        walk::walk_function_body(&mut returns, body);
        let mut completions = Vec::new();
        for (start, end) in returns.spans {
            let Some(values) = facts
                .type_id_at(&self.path, start, end)
                .and_then(|returned| step_values(facts, returned))
            else {
                return annotated;
            };
            completions.extend(values.returns);
        }
        let is_undefined = |ty: &Option<TsTypeId>| {
            ty.is_none_or(|ty| {
                matches!(
                    facts.types.get(ty).map(|ty| &ty.kind),
                    Some(TsTypeKind::Undefined | TsTypeKind::Void)
                )
            })
        };
        let span = annotation.span();
        let respeller = Respeller {
            builder: self,
            facts,
            annotation: (span.start, span.end),
            records: false,
        };
        let mut spelled: Vec<String> = Vec::new();
        for ty in completions.iter().filter(|ty| !is_undefined(ty)).flatten() {
            let Some(text) = respeller.respell(*ty, 0) else {
                return annotated;
            };
            if !spelled.contains(&text) {
                spelled.push(text);
            }
        }
        if spelled.is_empty() {
            return annotated;
        }
        if completions.iter().any(is_undefined) {
            spelled.push("undefined".to_owned());
        }
        let records_before = self.ctx.krate.erasures.len();
        let Some(completion) = self
            .lower_respelled_annotation(&spelled.join(" | "), span.start, span.end)
            .filter(|ty| !self.type_mentions_unknown(*ty))
        else {
            self.ctx.krate.erasures.truncate(records_before);
            return annotated;
        };
        self.ctx
            .krate
            .types
            .intern(smelt_hir::Type::GeneratorResult {
                yield_ty,
                return_ty: completion,
            })
    }
}

/// Whether `annotation` is `IteratorResult<T>` or `IteratorResult<T, any>`:
/// the step whose `TReturn` the lib leaves at `any`. (The caller has already
/// lowered it to the lib's step type, so the name is not the source's own.)
fn leaves_return_open(annotation: &TSType<'_>) -> bool {
    let TSType::TSTypeReference(reference) = annotation else {
        return false;
    };
    let TSTypeName::IdentifierReference(name) = &reference.type_name else {
        return false;
    };
    if name.name != "IteratorResult" {
        return false;
    }
    let args = reference
        .type_arguments
        .as_ref()
        .map(|args| args.params.as_slice())
        .unwrap_or_default();
    match args {
        [_] => true,
        [_, returned] => matches!(returned, TSType::TSAnyKeyword(_)),
        _ => false,
    }
}

/// The spans of the expressions a function body returns, outside nested
/// functions and classes (whose `return`s are their own).
#[derive(Default)]
struct ReturnedExpressions {
    /// `(start, end)` of each `return` argument; a bare `return;` returns
    /// `undefined` and is no step.
    spans: Vec<(u32, u32)>,
}

impl<'a> Visit<'a> for ReturnedExpressions {
    fn visit_return_statement(&mut self, it: &oxc::ast::ast::ReturnStatement<'a>) {
        if let Some(argument) = &it.argument {
            let span = argument.span();
            self.spans.push((span.start, span.end));
        }
    }

    fn visit_function(
        &mut self,
        _it: &oxc::ast::ast::Function<'a>,
        _flags: oxc::syntax::scope::ScopeFlags,
    ) {
    }

    fn visit_arrow_function_expression(
        &mut self,
        _it: &oxc::ast::ast::ArrowFunctionExpression<'a>,
    ) {
    }

    fn visit_class(&mut self, _it: &oxc::ast::ast::Class<'a>) {}
}
