//! TypeScript type annotations lowered to HIR types: the `ts_type_to_hir`
//! entry point and its dispatch over annotation shapes, type queries, and
//! indexed access types.
//!
//! Each annotation family and the type queries over lowered types live in the
//! child module for their concern; type reference name resolution is
//! `type_references`.

mod callables;
mod derived_records;
mod fetch_init;
mod field_types;
mod host_types;
mod method_resolution;
mod object_types;
mod record_keys;
mod settled_result;
mod structural_fields;
mod tuples_rest;
mod type_queries;
mod type_references;
mod unions;

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::erasure::NotNewErasure;
use oxc::ast::ast::{Expression, TSType, TSTypeName, TSTypeQueryExprName};
use oxc::span::GetSpan;
use oxc::syntax::operator::UnaryOperator;
use smelt_hir::{ErasureReason, FunctionType, Item, Type};

impl ModuleBuilder<'_> {
    /// Lower a TypeScript type annotation into HIR type metadata.
    ///
    /// `never` is preserved as a bottom marker instead of being converted to
    /// `None` or `Unknown`; callers that need executable values must reject it.
    ///
    /// Bun's `int32` is an int32 slot type only as the WHOLE annotation; every
    /// nested type lowers through this same function one level deeper, where
    /// it is a `number` (see `lowering::int32`).
    pub(in crate::lowering) fn ts_type_to_hir(
        &mut self,
        ty: &TSType<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        let whole = self.int32_type_depth == 0;
        if whole && self.is_bun_int32_annotation(ty) {
            return Ok(self
                .ctx
                .krate
                .types
                .intern(Type::FixedInt(smelt_hir::IntWidth::I32)));
        }
        let anchor = self.span(ty.span().start, ty.span().end);
        let lowered = self.with_nested_type_position(|this| {
            this.with_erasure_anchor(anchor, |this| {
                let records_before = this.ctx.krate.erasures.len();
                this.ts_type_shape_to_hir(ty)
                    .map(|lowered| this.checker_evaluated_or(lowered, ty.span(), records_before))
            })
        });
        Ok(self.int32_only_as_whole_type(lowered?, whole))
    }

    /// Run `lower` one type-annotation level deeper, restoring the depth on
    /// every exit path of `lower`.
    ///
    /// Depth `0` is exactly the whole-type position of a slot, which is the
    /// only position where Bun's `int32` is an int32 (see `lowering::int32`).
    fn with_nested_type_position<R>(&mut self, lower: impl FnOnce(&mut Self) -> R) -> R {
        self.int32_type_depth += 1;
        let result = lower(self);
        self.int32_type_depth -= 1;
        result
    }

    /// `lowered`, or the TypeScript 7 checker's evaluation of the annotation
    /// when `lowered` erased to `Unknown` (see `lowering/checker_types.rs`).
    ///
    /// The erasure records made since `records_before` belong to the decision
    /// being overturned, so they are dropped when the checker answers and put
    /// back when it declines.
    fn checker_evaluated_or(
        &mut self,
        lowered: smelt_hir::TypeId,
        span: oxc::span::Span,
        records_before: usize,
    ) -> smelt_hir::TypeId {
        if self.ts_facts.is_none()
            || !matches!(self.ctx.krate.types.get(lowered), Some(Type::Unknown))
        {
            return lowered;
        }
        let overturned: Vec<_> = self.ctx.krate.erasures.drain(records_before..).collect();
        if let Some(evaluated) = self.checker_annotation_type(span.start, span.end) {
            return evaluated;
        }
        self.ctx.krate.erasures.extend(overturned);
        lowered
    }

    /// Lower the shape of a TypeScript type annotation; see [`Self::ts_type_to_hir`].
    fn ts_type_shape_to_hir(&mut self, ty: &TSType<'_>) -> Result<smelt_hir::TypeId, SmeltError> {
        match ty {
            TSType::TSAnyKeyword(_) => Ok(self.erase(ErasureReason::ExplicitAny)),
            TSType::TSUnknownKeyword(_) => Ok(self.erase(ErasureReason::SourceUnknown)),
            TSType::TSObjectKeyword(_) => Ok(self.erase(ErasureReason::OpenLayout)),
            TSType::TSTypeQuery(query) => Ok(self.type_query_to_hir(query)),
            TSType::TSNumberKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Float)),
            TSType::TSBigIntKeyword(_) => Ok(self.ctx.krate.types.intern(Type::BigInt)),
            TSType::TSStringKeyword(_) | TSType::TSTemplateLiteralType(_) => {
                Ok(self.ctx.krate.types.intern(Type::String))
            }
            TSType::TSBooleanKeyword(_) | TSType::TSTypePredicate(_) => {
                Ok(self.ctx.krate.types.intern(Type::Bool))
            }
            TSType::TSSymbolKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Symbol)),
            TSType::TSNeverKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Never)),
            TSType::TSVoidKeyword(_) | TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_) => {
                Ok(self.ctx.krate.types.intern(Type::None))
            }
            TSType::TSLiteralType(literal) => match &literal.literal {
                oxc::ast::ast::TSLiteral::StringLiteral(_) => {
                    Ok(self.ctx.krate.types.intern(Type::String))
                }
                oxc::ast::ast::TSLiteral::NumericLiteral(_) => {
                    Ok(self.ctx.krate.types.intern(Type::Float))
                }
                oxc::ast::ast::TSLiteral::BigIntLiteral(_) => {
                    Ok(self.ctx.krate.types.intern(Type::BigInt))
                }
                oxc::ast::ast::TSLiteral::UnaryExpression(unary)
                    if unary.operator == UnaryOperator::UnaryNegation
                        && matches!(unary.argument, Expression::NumericLiteral(_)) =>
                {
                    Ok(self.ctx.krate.types.intern(Type::Float))
                }
                oxc::ast::ast::TSLiteral::BooleanLiteral(_) => {
                    Ok(self.ctx.krate.types.intern(Type::Bool))
                }
                _ => Err(SmeltError::unsupported(
                    self.span(ty.span().start, ty.span().end),
                    format!("literal type annotation is not lowered yet: {ty:?}"),
                )),
            },
            // `null | undefined` (and nothing else) keeps which of the two a
            // value is: `Optional<None>`, present unit for `null`, as the
            // ternary join `c ? null : undefined` does (see
            // `null_or_undefined_join`). With any other arm the two collapse
            // into that arm's `Optional`, as everywhere else.
            TSType::TSUnionType(union)
                if union
                    .types
                    .iter()
                    .any(|member| matches!(member, TSType::TSNullKeyword(_)))
                    && union
                        .types
                        .iter()
                        .any(|member| matches!(member, TSType::TSUndefinedKeyword(_)))
                    && union.types.iter().all(|member| {
                        matches!(
                            member,
                            TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_)
                        )
                    }) =>
            {
                let none = self.ctx.krate.types.intern(Type::None);
                Ok(self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Optional(none, smelt_hir::Absence::Undefined)))
            }
            TSType::TSUnionType(union) => {
                let mut members = Vec::new();
                // Which absent values the union admits, so the result keeps
                // `null` apart from `undefined` (`Type::Optional`'s tag).
                let (mut admits_undefined, mut admits_null) = (false, false);
                // A direct object-literal arm of a union with another
                // non-nullish arm may be a discriminated arm
                // (`anonymous_record_literal_type`); nested literals are not,
                // and `{ a: 1 } | undefined` discriminates nothing.
                let value_arms = union
                    .types
                    .iter()
                    .filter(|member| {
                        !matches!(
                            member,
                            TSType::TSNullKeyword(_)
                                | TSType::TSUndefinedKeyword(_)
                                | TSType::TSVoidKeyword(_)
                        )
                    })
                    .count();
                // The union is discriminated when every object-literal arm
                // has a REQUIRED literal-typed property (`{ kind: "a"; .. } |
                // { kind: "b"; .. }`); then each such arm keeps its own record.
                // An arm whose discriminant is optional (`{ type?: "one" }`)
                // keeps the dictionary, as do its sibling arms: a record field
                // stores an absent property as `None`, so `{ type: undefined }`
                // would lose the key `'type' in v` and pattern matchers test,
                // and a union mixing `Dict` and record arms has no concrete
                // representation.
                let discriminated = value_arms > 1
                    && union.types.iter().all(|member| match member {
                        TSType::TSTypeLiteral(literal) => {
                            Self::type_literal_has_literal_property(literal)
                        }
                        _ => true,
                    })
                    && union
                        .types
                        .iter()
                        .any(|member| matches!(member, TSType::TSTypeLiteral(_)));
                for member in &union.types {
                    let arm_literal = match member {
                        TSType::TSTypeLiteral(literal) if discriminated => Some(literal.span.start),
                        _ => None,
                    };
                    let previous_arm = std::mem::replace(&mut self.union_arm_literal, arm_literal);
                    let member_ty = self.ts_type_to_hir(member);
                    self.union_arm_literal = previous_arm;
                    let mut member_ty = member_ty?;
                    let (member_undefined, member_null) =
                        self.ts_member_absences(member, member_ty);
                    admits_undefined |= member_undefined;
                    admits_null |= member_null;
                    if matches!(self.ctx.krate.types.get(member_ty), Some(Type::Function(_)))
                        && self.ts_type_is_known_callable_object_surface(member)
                    {
                        member_ty = self.erase(ErasureReason::UnmodeledType);
                    }
                    members.push(member_ty);
                }
                // `ArrayLike<T> | Iterable<T>`: beside an array arm an iterable
                // keeps the array representation (module `iterable_values`).
                if members
                    .iter()
                    .any(|member| matches!(self.ctx.krate.types.get(*member), Some(Type::List(_))))
                {
                    members = members
                        .into_iter()
                        .map(|member| self.iterable_member_as_array(member))
                        .collect();
                }
                Ok(
                    self.union_of_member_types_with_absence(
                        &members,
                        admits_undefined,
                        admits_null,
                    ),
                )
            }
            TSType::TSIntersectionType(intersection) => {
                // An intersection of a call signature with object members is a
                // callable object: the same shape an `interface` with a call
                // signature declares, so it lowers to the same callable-interface
                // class instead of being erased below (see
                // `decls::callable_object`). Only a surface Smelt can name this
                // way is claimed; everything else keeps the erasing behaviour.
                if let Some(class_ty) = self.anonymous_callable_object_type(ty) {
                    return Ok(class_ty);
                }
                if Self::ts_type_is_callable_object_surface(ty) {
                    return Ok(self.erase(ErasureReason::UnmodeledType));
                }
                if Self::ts_type_is_opaque_object_intersection_surface(ty) {
                    return Ok(self.erase(ErasureReason::TypeLevel));
                }
                let mut meaningful = Vec::new();
                let mut fields = Vec::new();
                // Whether some non-function member has no readable fields
                // (a type parameter `TObj`, an opaque alias). Attached to a
                // function arm, such a member's properties would be dropped.
                let mut unreadable_object_member = false;
                for member in &intersection.types {
                    if matches!(member, TSType::TSTypeLiteral(lit) if lit.members.is_empty()) {
                        continue;
                    }
                    let member_fields = self.type_fields_from_ts(member);
                    // `pattern & Iterable<pattern>`: an intersection reads an
                    // iterable member as the array it was (module `iterable_values`).
                    let member_ty = self.ts_type_to_hir(member)?;
                    let member_ty = self.iterable_member_as_array(member_ty);
                    let member_fields = member_fields.unwrap_or_default();
                    unreadable_object_member |= member_fields.is_empty()
                        && !matches!(self.ctx.krate.types.get(member_ty), Some(Type::Function(_)));
                    fields.extend(member_fields);
                    meaningful.push(member_ty);
                }
                match meaningful.as_slice() {
                    [] => Ok(self.ctx.krate.types.intern(Type::None)),
                    [single] => Ok(*single),
                    _ if meaningful.iter().any(|member_ty| {
                        matches!(
                            self.ctx.krate.types.get(*member_ty),
                            Some(Type::Function(_))
                        )
                    }) =>
                    {
                        // A function intersected with an object whose fields
                        // are not known (`(() => void) & TObj`) is a callable
                        // object with an open property set. Narrowing it to
                        // the function arm would claim keyed reads and
                        // `Object.assign` copies of it are reads of a bare
                        // function; the honest answer is the erased callable
                        // object, as for `ts_type_is_callable_object_surface`.
                        if unreadable_object_member {
                            return Ok(self.erase(ErasureReason::UnmodeledType));
                        }
                        let function_ty = *meaningful
                            .iter()
                            .find(|member_ty| {
                                matches!(
                                    self.ctx.krate.types.get(**member_ty),
                                    Some(Type::Function(_))
                                )
                            })
                            .expect("function member should exist");
                        if !fields.is_empty() {
                            self.types.set_callable_fields(function_ty, fields.clone());
                            self.ctx.callable_fields.write().insert(function_ty, fields);
                        }
                        Ok(function_ty)
                    }
                    _ => Ok(self.intersection_object_type(&meaningful)),
                }
            }
            TSType::TSConditionalType(conditional) => {
                let true_ty = self.ts_type_to_hir(&conditional.true_type)?;
                let false_ty = self.ts_type_to_hir(&conditional.false_type)?;
                if true_ty == false_ty {
                    return Ok(true_ty);
                }
                // Arms that agree once a type-parameter arm is read at its
                // constraint describe one runtime type: under
                // `T extends string`, `T extends `${infer F}${infer R}` ?
                // `${Uppercase<F>}${R}` : T` is a `string` on both branches,
                // the type a hand port gives it (es-toolkit's `Capitalize<T>`).
                // Only when the arms still differ is the result decided by
                // the type-level test, which Smelt does not evaluate.
                let true_erased = self.type_param_constraint_or_self(true_ty);
                let false_erased = self.type_param_constraint_or_self(false_ty);
                if true_erased == false_erased {
                    Ok(true_erased)
                } else {
                    Ok(self.erase(ErasureReason::TypeLevel))
                }
            }
            TSType::TSArrayType(array) => {
                let element_ty = self.ts_type_to_hir(&array.element_type)?;
                Ok(self.ctx.krate.types.intern(Type::List(element_ty)))
            }
            TSType::TSParenthesizedType(parenthesized) => {
                self.ts_type_to_hir(&parenthesized.type_annotation)
            }
            TSType::TSTupleType(tuple) => {
                if let Some(rest_ty) = self.homogeneous_tuple_rest_type(tuple)? {
                    return Ok(rest_ty);
                }
                if let Some(rest_ty) = self.tuple_rest_list_type(tuple)? {
                    return Ok(rest_ty);
                }
                if let Some(list_ty) = self.leading_rest_tuple_list_type(tuple)? {
                    return Ok(list_ty);
                }
                let mut items = Vec::new();
                for item in &tuple.element_types {
                    if self.tuple_rest_type_erases_to_empty(item)? {
                        continue;
                    }
                    items.push(self.tuple_element_type_to_hir(item)?);
                }
                Ok(self.ctx.krate.types.intern(Type::Tuple(items)))
            }
            TSType::TSTypeOperatorType(operator)
                if operator.operator == oxc::ast::ast::TSTypeOperatorOperator::Readonly =>
            {
                self.ts_type_to_hir(&operator.type_annotation)
            }
            TSType::TSTypeOperatorType(operator)
                if operator.operator == oxc::ast::ast::TSTypeOperatorOperator::Unique =>
            {
                self.ts_type_to_hir(&operator.type_annotation)
            }
            TSType::TSTypeOperatorType(operator)
                if operator.operator == oxc::ast::ast::TSTypeOperatorOperator::Keyof =>
            {
                Ok(self.ctx.krate.types.intern(Type::String))
            }
            TSType::TSNamedTupleMember(named) => {
                let inner = self.tuple_element_type_to_hir(&named.element_type)?;
                Ok(if named.optional {
                    self.ctx
                        .krate
                        .types
                        .intern(Type::Optional(inner, smelt_hir::Absence::Undefined))
                } else {
                    inner
                })
            }
            TSType::TSTypeReference(reference) => {
                let ty = self.type_reference_to_hir(reference)?;
                // An array-like interface admits any list, which no struct of
                // its named members can hold.
                match self.ctx.krate.types.get(ty) {
                    Some(Type::Class { name, .. }) if self.classes.is_array_like(*name) => {
                        Ok(self.erase(ErasureReason::OpenLayout))
                    }
                    _ => Ok(ty),
                }
            }
            TSType::TSIndexedAccessType(indexed) => self.indexed_access_type_to_hir(indexed),
            TSType::TSTypeLiteral(literal) => self.type_literal_to_hir(literal),
            TSType::TSMappedType(mapped) => {
                let value_ty = mapped
                    .type_annotation
                    .as_ref()
                    .map(|annotation| self.ts_type_to_hir(annotation))
                    .transpose()?
                    .unwrap_or_else(|| self.erase(ErasureReason::TypeLevel));
                if let TSType::TSTypeOperatorType(operator) = &mapped.constraint
                    && operator.operator == oxc::ast::ast::TSTypeOperatorOperator::Keyof
                {
                    let source_ty = self.ts_type_to_hir(&operator.type_annotation)?;
                    let source_ty = self.type_param_constraint_or_self(source_ty);
                    if self.mapped_key_source_is_iterable(source_ty) {
                        return Ok(self.ctx.krate.types.intern(Type::List(value_ty)));
                    }
                }
                let key_ty = self.ctx.krate.types.intern(Type::String);
                Ok(self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty)))
            }
            TSType::TSFunctionType(function) => self.function_type_to_hir(function),
            TSType::TSConstructorType(constructor) => self.constructor_type_to_hir(constructor),
            TSType::TSThisType(_this_ty) => {
                let Some(class_name) = self.current_class.clone() else {
                    return Ok(self.erase(ErasureReason::TypeLevel));
                };
                // `this` in a method/return position resolves to the enclosing
                // class type. Prefer the fully lowered class item when it is
                // already registered (it carries the canonical name symbol), but
                // fall back to interning the enclosing class name directly. The
                // class currently being lowered is not inserted into `classes`
                // until its whole body finishes, so a `this` annotation on one of
                // its own methods (`clear(): this`, `... as this`) would otherwise
                // fail to resolve — this notably affects classes declared inside a
                // function body (e.g. a test's `describe` callback), which are
                // lowered inline without a forward-declaration pass. The interned
                // symbol matches the class item's name because both come from
                // `intern_type_name(class_text)`.
                let name = self
                    .classes
                    .item(&class_name)
                    .and_then(|class_item| match self.item_ref(class_item) {
                        Item::Class(class) => Some(class.name),
                        _ => None,
                    })
                    .unwrap_or_else(|| self.intern_type_name(&class_name));
                Ok(self.ctx.krate.types.intern(Type::Class {
                    name,
                    args: Vec::new(),
                }))
            }
            _ => Err(SmeltError::unsupported(
                self.span(ty.span().start, ty.span().end),
                format!("type annotation is not lowered yet: {ty:?}"),
            )),
        }
    }

    /// Lower a `typeof name` type query to the type of the VALUE it names.
    ///
    /// A type query asks for the type of a value binding, so the answer is
    /// whatever the frontend already resolved that binding to: a function's
    /// signature, or a module-level binding's own type. Both are known before
    /// any body is lowered (`predeclare_function_items`,
    /// `collect_module_globals`).
    ///
    /// This is what makes the standard "const object as a closed value set"
    /// idiom lower to what it denotes:
    ///
    /// ```ignore
    /// const mimes = { json: "application/json" } as const;
    /// type Mime = (typeof mimes)[keyof typeof mimes];   // -> string
    /// ```
    ///
    /// `keyof T` is already `string` and an indexed access over a `Dict` is
    /// already its value type, so resolving the query is the only missing link.
    /// While it answered `Unknown`, an alias written this way erased — and,
    /// because an erased member makes a whole union non-concrete
    /// (`union_member_is_concrete`), one such alias inside a union erased the
    /// union entirely: Hono's `ResponseHeadersInit` reached `new Headers(init)`
    /// as `SmeltUnknown` for exactly this reason, with no diagnostic naming the
    /// alias. See `standards-generic-arm-and-typeof-indexed-alias.md` (in git history).
    ///
    /// A query naming something with no resolvable value type still answers
    /// `Unknown`: `typeof SomeClass` is the CONSTRUCTOR, not an instance, and
    /// that is a separate shape.
    pub(in crate::lowering) fn type_query_to_hir(
        &mut self,
        query: &oxc::ast::ast::TSTypeQuery<'_>,
    ) -> smelt_hir::TypeId {
        if let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name
            && let Some(value_ty) = self.value_binding_type(ident.name.as_str())
        {
            return value_ty;
        }
        if let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name
            && let Some(item) = self.items.get(ident.name.as_str()).copied()
            && let Item::Function(function) = self.item_ref(item).clone()
        {
            let function_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
                params: function.params.iter().map(|param| param.ty).collect(),
                rest: function.rest,
                required_params: None,
                mutable_params: Vec::new(),
                return_ty: function.return_ty,
                is_async: function.is_async,
                may_throw: false,
            }));
            // An instantiation expression (`typeof match<Router<T>, T>`) names
            // the function at those type arguments: its own parameters are
            // replaced, so the declaring scope's `T` reads through it.
            let Some(type_arguments) = &query.type_arguments else {
                return function_ty;
            };
            let lowered = type_arguments
                .params
                .iter()
                .map(|arg| self.ts_type_to_hir(arg))
                .collect::<Result<Vec<_>, _>>();
            let Ok(lowered) = lowered else {
                return function_ty;
            };
            let span = self.span(query.span.start, query.span.end);
            return self
                .type_argument_substitution(&function.type_params, &lowered, span)
                .map_or(function_ty, |substitutions| {
                    self.substitute_type_params(function_ty, &substitutions)
                });
        }
        self.erase(ErasureReason::TypeLevel)
    }

    /// The resolved type of a VALUE binding named in a type position.
    ///
    /// Only module-level bindings are consulted: they are collected in a
    /// prepass, so a `typeof` in an annotation sees them regardless of where the
    /// annotation sits relative to the declaration. A `const` item's own
    /// recorded type is preferred when both are present because it is the one
    /// the value's reads use.
    fn value_binding_type(&self, name: &str) -> Option<smelt_hir::TypeId> {
        if let Some(item) = self.items.get(name).copied() {
            match self.item_ref(item) {
                Item::Const(constant) => return Some(constant.ty),
                Item::MutableGlobal(global) => return Some(global.ty),
                _ => {}
            }
        }
        self.module_globals.get(name).copied()
    }

    /// Convert an indexed access type such as `Parameters<Fn>[0]` to an item type.
    pub(in crate::lowering) fn indexed_access_type_to_hir(
        &mut self,
        indexed: &oxc::ast::ast::TSIndexedAccessType<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        let object_ty = self.ts_type_to_hir(&indexed.object_type)?;
        if let Some(deferred) = self.deferred_indexed_access(object_ty, &indexed.index_type)? {
            return Ok(deferred);
        }
        // `S[K]` over a declared record `S` and a key parameter
        // `K extends keyof S`: the base constraint `S[keyof S]`
        // (`deferred_index::keyof_record_read_type`).
        if let TSType::TSTypeReference(reference) = &indexed.index_type
            && reference.type_arguments.is_none()
            && matches!(reference.type_name, TSTypeName::IdentifierReference(_))
            && matches!(
                self.ctx.krate.types.get(object_ty),
                Some(Type::Class { .. })
            )
        {
            let key_ty = self.ts_type_to_hir(&indexed.index_type)?;
            if matches!(
                self.ctx.krate.types.get(key_ty),
                Some(Type::TypeParam { .. })
            ) && let Some(read) = self.keyof_record_read_type(object_ty, key_ty)
            {
                return Ok(read);
            }
        }
        match self.ctx.krate.types.get(object_ty) {
            Some(Type::Tuple(items)) => {
                if let Some(index) = Self::numeric_literal_type_index(&indexed.index_type) {
                    return Ok(items
                        .get(index)
                        .copied()
                        .unwrap_or_else(|| self.erase(ErasureReason::TypeLevel)));
                }
                Ok(self.erase(ErasureReason::TypeLevel))
            }
            Some(Type::List(item) | Type::Set(item)) => Ok(*item),
            Some(Type::Dict(_, value)) => Ok(*value),
            // not a new erasure: the indexed object is already erased
            Some(Type::Unknown) => Ok(self.unknown_type(NotNewErasure::ErasedUpstream)),
            _ => Ok(self.erase(ErasureReason::TypeLevel)),
        }
    }

    /// Extract a non-negative integer from a numeric literal type.
    pub(in crate::lowering) fn numeric_literal_type_index(ty: &TSType<'_>) -> Option<usize> {
        let TSType::TSLiteralType(literal) = ty else {
            return None;
        };
        let oxc::ast::ast::TSLiteral::NumericLiteral(number) = &literal.literal else {
            return None;
        };
        let text = number.raw.as_ref()?;
        (number.value.fract() == 0.0 && number.value >= 0.0)
            .then(|| text.parse::<usize>().ok())
            .flatten()
    }
}
