//! Object-shaped annotations: type literals, `Partial` records, object
//! intersections, and the callable-object and opaque-object surface tests.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::js_text::{has_lone_surrogate, js_str_text};
use oxc::ast::ast::{TSSignature, TSType, TSTypeName};
use smelt_hir::{ErasureReason, FunctionType, Type};

impl ModuleBuilder<'_> {
    /// Return whether a source type is a callable value with object fields.
    pub(in crate::lowering) fn ts_type_is_known_callable_object_surface(
        &mut self,
        ty: &TSType<'_>,
    ) -> bool {
        if Self::ts_type_is_callable_object_surface(ty) {
            return true;
        }
        let TSType::TSTypeReference(reference) = ty else {
            return false;
        };
        let TSTypeName::IdentifierReference(name) = &reference.type_name else {
            return false;
        };
        let symbol = self.intern_type_name(name.name.as_str());
        self.types.is_callable_object_alias(symbol)
    }

    /// Return whether a type's syntax is an intersection of callable and object surfaces.
    pub(in crate::lowering) fn ts_type_is_callable_object_surface(ty: &TSType<'_>) -> bool {
        let TSType::TSIntersectionType(intersection) = ty else {
            return false;
        };
        let mut has_callable = false;
        let mut has_object = false;
        for member in &intersection.types {
            match member {
                TSType::TSFunctionType(_) => has_callable = true,
                TSType::TSTypeLiteral(literal) => {
                    has_object |= !literal.members.is_empty();
                }
                TSType::TSTypeReference(_) => has_object = true,
                TSType::TSParenthesizedType(parenthesized) => {
                    if matches!(
                        &parenthesized.type_annotation,
                        TSType::TSFunctionType(_) | TSType::TSConstructorType(_)
                    ) {
                        has_callable = true;
                    } else {
                        has_object = true;
                    }
                }
                _ => {}
            }
        }
        has_callable && has_object
    }

    /// Return whether an intersection mixes an opaque reference with object fields.
    ///
    /// Smelt cannot currently preserve both halves of `Alias & { field: ... }`
    /// unless the alias resolves locally to a concrete class or dict. Erasing the
    /// surface to `unknown` keeps runtime field access and callable-object values
    /// intact instead of collapsing the value to only the object-literal fields.
    pub(in crate::lowering) fn ts_type_is_opaque_object_intersection_surface(
        ty: &TSType<'_>,
    ) -> bool {
        let TSType::TSIntersectionType(intersection) = ty else {
            return false;
        };
        let mut has_reference = false;
        let mut has_object_literal_fields = false;
        for member in &intersection.types {
            match member {
                TSType::TSTypeReference(_) => has_reference = true,
                TSType::TSTypeLiteral(literal) => {
                    has_object_literal_fields |= !literal.members.is_empty();
                }
                TSType::TSParenthesizedType(parenthesized) => {
                    if matches!(&parenthesized.type_annotation, TSType::TSTypeReference(_)) {
                        has_reference = true;
                    }
                }
                _ => {}
            }
        }
        has_reference && has_object_literal_fields
    }

    /// Convert an inline TypeScript object type into Smelt's record-like dict type.
    /// Whether every member of a type literal names an `Object.prototype`
    /// member, so that every object satisfies it (see `type_literal_to_hir`).
    fn type_literal_is_object_prototype_surface(
        literal: &oxc::ast::ast::TSTypeLiteral<'_>,
    ) -> bool {
        const OBJECT_PROTOTYPE_MEMBERS: [&str; 6] = [
            "toString",
            "toLocaleString",
            "valueOf",
            "hasOwnProperty",
            "isPrototypeOf",
            "propertyIsEnumerable",
        ];
        !literal.members.is_empty()
            && literal.members.iter().all(|member| {
                let key = match member {
                    TSSignature::TSPropertySignature(property) if !property.computed => {
                        &property.key
                    }
                    TSSignature::TSMethodSignature(method) if !method.computed => &method.key,
                    _ => return false,
                };
                let name = match key {
                    oxc::ast::ast::PropertyKey::StaticIdentifier(identifier) => {
                        std::borrow::Cow::Borrowed(identifier.name.as_str())
                    }
                    oxc::ast::ast::PropertyKey::StringLiteral(literal)
                        if !has_lone_surrogate(literal.value) =>
                    {
                        js_str_text(literal.value)
                    }
                    _ => return false,
                };
                OBJECT_PROTOTYPE_MEMBERS.contains(&name.as_ref())
            })
    }

    pub(in crate::lowering) fn type_literal_to_hir(
        &mut self,
        literal: &oxc::ast::ast::TSTypeLiteral<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        // A type literal every member of which `Object.prototype` already
        // supplies (`{ toString: () => string }`, `{ valueOf(): number }`)
        // states no layout: EVERY object satisfies it through its prototype,
        // a `Date` or a class instance included, so it is the open-layout
        // boundary `object` is (`ErasureReason::OpenLayout`), not a record of
        // those members. Lowering it as a record rebuilt any value reaching it
        // from its own entries and lost the rest: remeda's `Comparable`
        // (`... | { valueOf(): ComparablePrimitive }`) turned each `Date` into
        // a record of no-op callbacks, so `firstBy`/`sortBy` compared `NaN`s.
        if Self::type_literal_is_object_prototype_surface(literal) {
            return Ok(self.erase(ErasureReason::OpenLayout));
        }
        // A type literal that is callable *and* carries named members is a
        // callable object, not a bare function: it lowers to the same
        // callable-interface class the `interface` spelling produces (see
        // `decls::callable_object`) so its members survive. A literal with only
        // call signatures stays a plain function type below.
        if let Some(class_ty) = self.anonymous_callable_object_literal_type(literal) {
            return Ok(class_ty);
        }
        if let Some(TSSignature::TSCallSignatureDeclaration(signature)) = literal
            .members
            .iter()
            .find(|member| matches!(member, TSSignature::TSCallSignatureDeclaration(_)))
        {
            let _type_params =
                self.push_type_parameter_scope(signature.type_parameters.as_deref())?;
            let result = (|| {
                let return_ty = signature
                    .return_type
                    .as_ref()
                    .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                    .transpose()?
                    .unwrap_or_else(|| self.erase(ErasureReason::MissingAnnotation));
                let mut params = Vec::new();
                for param in &signature.params.items {
                    let ty = param
                        .type_annotation
                        .as_ref()
                        .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                        .transpose()?
                        .unwrap_or_else(|| self.erase(ErasureReason::MissingAnnotation));
                    params.push(if param.optional {
                        self.ctx.krate.types.intern(Type::Optional(ty))
                    } else {
                        ty
                    });
                }
                let rest = if let Some(rest) = &signature.params.rest {
                    let rest_ty = rest
                        .type_annotation
                        .as_ref()
                        .map(|annotation| {
                            self.function_type_rest_param_to_hir(&annotation.type_annotation)
                        })
                        .transpose()?
                        .ok_or_else(|| {
                            SmeltError::unsupported(
                                self.span(rest.span.start, rest.span.end),
                                "call signature rest parameters require explicit array types",
                            )
                        })?;
                    self.push_rest_param_type(&mut params, rest_ty)?
                } else {
                    None
                };
                Ok::<_, SmeltError>(self.ctx.krate.types.intern(Type::Function(FunctionType {
                    mutable_params:
                        self.mutable_params_from_returned_tuple_state(&params, return_ty),
                    params,
                    rest,
                    required_params: Some(Self::formal_parameters_required_count(
                        &signature.params,
                    )),
                    return_ty,
                    is_async: matches!(self.ctx.krate.types.get(return_ty), Some(Type::Future(_))),
                    may_throw: false,
                })))
            })();
            self.pop_type_parameter_scope();
            return result;
        }
        if let [TSSignature::TSConstructSignatureDeclaration(signature)] =
            literal.members.as_slice()
        {
            let _type_params =
                self.push_type_parameter_scope(signature.type_parameters.as_deref())?;
            let result = (|| {
                let return_ty = signature
                    .return_type
                    .as_ref()
                    .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                    .transpose()?
                    .unwrap_or_else(|| self.erase(ErasureReason::MissingAnnotation));
                let mut params = Vec::new();
                for param in &signature.params.items {
                    let ty = param
                        .type_annotation
                        .as_ref()
                        .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                        .transpose()?
                        .unwrap_or_else(|| self.erase(ErasureReason::MissingAnnotation));
                    params.push(ty);
                }
                let rest = if let Some(rest) = &signature.params.rest {
                    let rest_ty = rest
                        .type_annotation
                        .as_ref()
                        .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                        .transpose()?
                        .unwrap_or_else(|| self.erase(ErasureReason::MissingAnnotation));
                    let index = params.len();
                    params.push(rest_ty);
                    Some(index)
                } else {
                    None
                };
                Ok::<_, SmeltError>(self.ctx.krate.types.intern(Type::Function(FunctionType {
                    mutable_params:
                        self.mutable_params_from_returned_tuple_state(&params, return_ty),
                    params,
                    rest,
                    required_params: Some(Self::formal_parameters_required_count(
                        &signature.params,
                    )),
                    return_ty,
                    is_async: false,
                    may_throw: false,
                })))
            })();
            self.pop_type_parameter_scope();
            return result;
        }
        let mut key_ty = self.ctx.krate.types.intern(Type::String);
        let mut member_tys = Vec::new();
        for member in &literal.members {
            if let TSSignature::TSIndexSignature(index) = member {
                member_tys.push(self.ts_type_to_hir(&index.type_annotation.type_annotation)?);
            }
            // A member under a runtime key (`[k]: V` with a block-local
            // `const k = Symbol()`) has no field name, but its value is still
            // one of the record's values, and its key may be a symbol: the
            // record is keyed by property key, as the literal that builds it.
            if let TSSignature::TSPropertySignature(property) = member
                && property.computed
                && !self.is_resolvable_property_key(&property.key)
                && let Some(annotation) = &property.type_annotation
            {
                let value_ty = self.ts_type_to_hir(&annotation.type_annotation)?;
                member_tys.push(if property.optional {
                    self.ctx.krate.types.intern(Type::Optional(value_ty))
                } else {
                    value_ty
                });
                key_ty = self.ctx.krate.types.intern(Type::PropKey);
            }
        }
        let fields = self.type_literal_fields(literal)?;
        for field in &fields {
            member_tys.push(if field.optional {
                self.ctx.krate.types.intern(Type::Optional(field.ty))
            } else {
                field.ty
            });
        }
        if member_tys.is_empty() {
            let value_ty = self.erase(ErasureReason::OpenLayout);
            return Ok(self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty)));
        }
        // The value type is the FLAT union of the members' types, as a union
        // annotation lowers: `{ a: string; b?: string }` holds `string` or
        // nothing, which is `Optional<String>`. Collecting the raw member types
        // gave `String | Optional<String>`, a union with an optional arm, which
        // has no concrete representation and erased every record of the alias.
        let mut value_tys = Vec::new();
        let mut nullable = false;
        for member_ty in member_tys {
            for arm in self.flatten_union_member_types(member_ty) {
                if matches!(self.ctx.krate.types.get(arm), Some(Type::None)) {
                    nullable = true;
                } else if !value_tys.contains(&arm) {
                    value_tys.push(arm);
                }
            }
        }
        // Fields of different types have no common `Dict` value type: the
        // merged union would retype every field (see `decls::anonymous_record`).
        if let Some(record_ty) = self.anonymous_record_literal_type(literal, &fields) {
            return Ok(record_ty);
        }
        let value_ty = match value_tys.as_slice() {
            [] => self.ctx.krate.types.intern(Type::None),
            [single] => *single,
            _ => self.ctx.krate.types.intern(Type::Union(value_tys)),
        };
        let value_ty =
            if nullable && !matches!(self.ctx.krate.types.get(value_ty), Some(Type::None)) {
                self.ctx.krate.types.intern(Type::Optional(value_ty))
            } else {
                value_ty
            };
        Ok(self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty)))
    }

    /// Lower an object-shaped intersection `A & B` structurally.
    ///
    /// An intersection is **not** a union: a value of `A & B` has all the
    /// members of both, so it inhabits neither arm of `A | B`. Lowering it to
    /// `Type::Union` forced codegen to recover the value by picking an arm --
    /// which for two object arms is undecidable from the runtime tag alone, and
    /// the chosen arm then retyped every leaf (`age: 36` came back as `"36"`).
    ///
    /// HIR spells a record as `Dict(String, V)`, so when every member is a
    /// known record the structural merge is: the union of their keys (always
    /// `String`) against the per-field union of their value types -- which
    /// collapses to the shared value type when the members agree and to
    /// `Unknown` when they do not.
    ///
    /// A `Class` member is object-shaped but names no members here, so it
    /// contributes the erased value type -- keeping the record spelling this
    /// arm has always produced for a class intersection.
    ///
    /// A MODELED host class (`Response`, `Request`, `Headers`, ... -- one the
    /// stdlib registry gives its own Rust representation) intersected with
    /// other object shapes (records, or aliases of them such as a generic
    /// `TypedResponse<T>`) is that class. Its values are never records, so spelling
    /// `Response & TypedResponse<T>` as a record retyped a real response as an
    /// empty `SmeltRecord` (which then could not even be returned where a
    /// `Response` is expected). And the record half cannot live on the value
    /// either way: a host `Response` carries exactly its modeled members, so
    /// the extra keys of such an intersection are type-level brands
    /// (`TypedResponse`'s `_data`/`_status`/`_format`) no runtime value of this
    /// representation can hold. The class is the precise type a hand port
    /// writes; a read of a brand member is then a loud lowering blocker rather
    /// than an erased `undefined`.
    ///
    /// Anything else makes the merged shape genuinely unknown, and there the
    /// honest answer is the erased boundary type. In particular an intersection
    /// of *type parameters* (`merge<T, S>(..): T & S`) must not be claimed as a
    /// record: `merge` is called on arrays too and returns one, so asserting a
    /// record would force the value into a record and lose the array.
    pub(super) fn intersection_object_type(
        &mut self,
        members: &[smelt_hir::TypeId],
    ) -> smelt_hir::TypeId {
        let modeled = members
            .iter()
            .copied()
            .filter(|member| self.stdlib_class_of_type(*member).is_some())
            .collect::<Vec<_>>();
        if let [host_class] = modeled.as_slice()
            && members.iter().all(|member| {
                member == host_class
                    || matches!(
                        self.ctx.krate.types.get(*member),
                        Some(Type::Dict(..) | Type::Class { .. })
                    )
            })
        {
            return *host_class;
        }
        let mut value_ty: Option<smelt_hir::TypeId> = None;
        for member in members {
            let member_value = match self.ctx.krate.types.get(*member) {
                Some(Type::Dict(_, field_ty)) => *field_ty,
                Some(Type::Class { .. }) => self.erase(ErasureReason::TypeLevel),
                _ => return self.erase(ErasureReason::TypeLevel),
            };
            match value_ty {
                None => value_ty = Some(member_value),
                Some(existing) if existing == member_value => {}
                // Two different field types under one key: the merged record's
                // value type is their union, which for unrelated shapes is the
                // erased boundary.
                Some(_) => value_ty = Some(self.erase(ErasureReason::TypeLevel)),
            }
        }
        let Some(value_ty) = value_ty else {
            return self.erase(ErasureReason::TypeLevel);
        };
        let key_ty = self.ctx.krate.types.intern(Type::String);
        self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty))
    }
}
