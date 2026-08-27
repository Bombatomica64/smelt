//! One lowering for every spelling of a *callable object* surface.
//!
//! TypeScript can describe "a value that is called like a function and also
//! carries named members" in several interchangeable ways:
//!
//! ```ts
//! interface F { (x: number): string; tag: string }        // interface
//! type F = { (x: number): string; tag: string };          // alias to a type literal
//! type F = ((x: number) => string) & { tag: string };     // intersection
//! function make(): ((x: number) => string) & { tag: string } { … }  // inline, anonymous
//! ```
//!
//! Only the first spelling used to reach the callable-interface class that
//! carries the synthetic `__smelt_call` field, so property writes onto a
//! callable local (`wrapper.tag = …`, see
//! `stmt::assignments::try_collect_callable_local_prop`) were consumed into a
//! typed [`smelt_hir::ExprKind::CallableObjectAssign`] for an `interface` and
//! dropped for the others. Same shape, several spellings, one behaviour: this
//! module routes them all through the interface synthesis.
//!
//! The rule, stated once:
//!
//! * a *named* surface (an alias declaration whose right-hand side is a
//!   callable object) lowers to the interface it is structurally equal to,
//!   under its own name and its own type parameters — exactly as if the source
//!   had spelled `interface`; and
//! * an *anonymous* surface (an inline type literal or intersection in a type
//!   position) lowers to a synthetic interface, named from the structure so two
//!   occurrences of the same shape share one generated struct.
//!
//! An anonymous surface that mentions a type parameter keeps its previous
//! lowering: a synthetic interface has no declaration site at which to bind
//! that parameter, so there is nowhere honest to put it. Named surfaces have no
//! such limit — the alias declaration supplies the binder.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use oxc::ast::ast::TSType;
use smelt_hir::{FunctionType, Interface, Item, Type, TypeParamDef};

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::state::interface_registry::LoweredInterface;

use super::types_iface::StructuralMembers;

impl ModuleBuilder<'_> {
    /// Collect the members of a callable-object surface written in any spelling.
    ///
    /// Returns `Some` only for a surface that is *both* callable (it declares at
    /// least one call signature) and an object (it declares at least one data
    /// field or method): a bare function type stays a [`Type::Function`], and a
    /// plain object type stays a record. Any member the walk does not
    /// understand (a named reference inside an intersection, a mapped type, a
    /// construct-only signature) makes the whole surface unrecognised, so the
    /// pre-existing lowering keeps handling it.
    ///
    /// Errors raised while lowering the members are treated as "not a
    /// callable-object surface" by the callers, which then fall back to the
    /// previous lowering rather than failing a file that used to build.
    pub(in crate::lowering) fn callable_object_surface_members(
        &mut self,
        ty: &TSType<'_>,
    ) -> Result<Option<StructuralMembers>, SmeltError> {
        let mut members = StructuralMembers::default();
        if !self.collect_callable_object_surface(ty, &mut members)? {
            return Ok(None);
        }
        if members.call_signatures.is_empty() {
            return Ok(None);
        }
        if members.fields.is_empty() && members.methods.is_empty() {
            return Ok(None);
        }
        Ok(Some(members))
    }

    /// Walk one arm of a callable-object surface, appending what it declares.
    ///
    /// Returns `false` as soon as an arm is a shape this synthesis does not
    /// model, which makes the whole surface unrecognised (see
    /// [`Self::callable_object_surface_members`]).
    fn collect_callable_object_surface(
        &mut self,
        ty: &TSType<'_>,
        members: &mut StructuralMembers,
    ) -> Result<bool, SmeltError> {
        match ty {
            TSType::TSParenthesizedType(parenthesized) => {
                self.collect_callable_object_surface(&parenthesized.type_annotation, members)
            }
            TSType::TSTypeLiteral(literal) => {
                self.lower_structural_members(&literal.members, members)?;
                Ok(true)
            }
            TSType::TSFunctionType(function) => {
                // A function-type arm of an intersection is the surface's call
                // signature, the same role a `(x: T): R` member plays inside a
                // type literal.
                let function_ty = self.function_type_to_hir(function)?;
                let Some(Type::Function(signature)) = self.ctx.krate.types.get(function_ty) else {
                    return Ok(false);
                };
                members.call_signatures.push(signature.clone());
                Ok(true)
            }
            TSType::TSIntersectionType(intersection) => {
                for arm in &intersection.types {
                    if !self.collect_callable_object_surface(arm, members)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            TSType::TSTypeReference(reference) => self.collect_named_surface_arm(reference, members),
            _ => Ok(false),
        }
    }

    /// Collect a named arm of a callable-object surface (`… & Named`).
    ///
    /// Two names are meaningful here and both keep the surface concrete:
    ///
    /// * an interface (or an alias already lowered as one) contributes its
    ///   members and its call signatures, with the reference's type arguments
    ///   substituted — the object half of `T & MemoizedFunction`; and
    /// * a generic type parameter contributes the call signature of its
    ///   constraint, the same erase-to-constraint rule the rest of type lowering
    ///   applies (`type_param_constraint_or_self`) — the callable half of
    ///   `T & MemoizedFunction`, where the constraint is `(...args: any) => any`.
    ///
    /// The synthetic `__smelt_call` slot a referenced interface already carries
    /// is skipped: the surface re-derives its own from the collected call
    /// signatures, so an interface that is itself callable does not contribute
    /// two call slots.
    fn collect_named_surface_arm(
        &mut self,
        reference: &oxc::ast::ast::TSTypeReference<'_>,
        members: &mut StructuralMembers,
    ) -> Result<bool, SmeltError> {
        let oxc::ast::ast::TSTypeName::IdentifierReference(identifier) = &reference.type_name else {
            return Ok(false);
        };
        let name_text = identifier.name.as_str();
        if let Some(param_ty) = self.type_parameter_type(name_text) {
            let constraint = self.type_param_constraint_or_self(param_ty);
            let Some(Type::Function(signature)) = self.ctx.krate.types.get(constraint) else {
                return Ok(false);
            };
            members.call_signatures.push(signature.clone());
            return Ok(true);
        }
        let name = self.intern_type_name(name_text);
        let Some(interface) = self.find_interface(name).cloned() else {
            return Ok(false);
        };
        let args = reference
            .type_arguments
            .as_ref()
            .map(|args| {
                args.params
                    .iter()
                    .map(|arg| self.ts_type_to_hir(arg))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default();
        let substitutions = self.type_argument_substitution(
            &interface.type_params,
            &args,
            self.span(reference.span.start, reference.span.end),
        )?;
        let call_slot = self.ctx.krate.symbols.intern("__smelt_call");
        let inherited = interface
            .fields
            .iter()
            .filter(|field| field.name != call_slot)
            .cloned()
            .collect::<Vec<_>>();
        members
            .fields
            .extend(self.substituted_fields(&inherited, &substitutions));
        let signatures = self
            .interfaces
            .call_signatures(name)
            .cloned()
            .unwrap_or_default();
        for signature in signatures {
            let substituted = self.substituted_function_type(&signature, &substitutions);
            members.call_signatures.push(substituted);
        }
        Ok(true)
    }

    /// Apply interface type-argument substitutions to a call signature.
    fn substituted_function_type(
        &mut self,
        signature: &FunctionType,
        substitutions: &std::collections::HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> FunctionType {
        FunctionType {
            params: signature
                .params
                .iter()
                .map(|param| self.substitute_type_params(*param, substitutions))
                .collect(),
            rest: signature.rest,
            required_params: signature.required_params,
            mutable_params: signature.mutable_params.clone(),
            return_ty: self.substitute_type_params(signature.return_ty, substitutions),
            is_async: signature.is_async,
            may_throw: signature.may_throw,
        }
    }

    /// Register a callable-object surface as an interface item under `name`.
    ///
    /// This is the same construction [`ModuleBuilder::interface_declaration`]
    /// performs once its members are collected: method fields, the synthetic
    /// `__smelt_call` storage field, the HIR item, the module-local registry and
    /// the shared-context sidecars. Sharing it is what makes the alias and
    /// `interface` spellings produce one class rather than two shapes.
    pub(in crate::lowering) fn register_callable_object_interface(
        &mut self,
        name: smelt_hir::Symbol,
        name_text: &str,
        type_params: Vec<TypeParamDef>,
        members: StructuralMembers,
        span: smelt_hir::Span,
    ) -> smelt_hir::ItemId {
        let StructuralMembers {
            mut fields,
            methods,
            call_signatures,
            construct_signatures,
            index_value_ty,
        } = members;
        self.add_interface_method_fields(&mut fields, &methods);
        self.add_interface_call_signature_field(&mut fields, &call_signatures);
        let item = self.ctx.krate.push_item(Item::Interface(Interface {
            name,
            span,
            type_params,
            extends: Vec::new(),
            fields,
            methods,
        }));
        self.interfaces.register_lowered(LoweredInterface {
            name,
            name_text: name_text.to_owned(),
            item,
            extends: Vec::new(),
            call_signatures: call_signatures.clone(),
            construct_signatures: construct_signatures.clone(),
            index_value_ty,
        });
        self.ctx.interface_extends.insert(name, Vec::new());
        self.ctx
            .interface_call_signatures
            .insert(name, call_signatures);
        if !construct_signatures.is_empty() {
            self.ctx
                .interface_construct_signatures
                .insert(name, construct_signatures);
        }
        if let Some(index_value_ty) = index_value_ty {
            self.ctx.interface_index_values.insert(name, index_value_ty);
        }
        item
    }

    /// Lower an anonymous callable-object surface to its interface class type.
    ///
    /// The surface has no source name, so one is derived from the structure
    /// itself: two occurrences of the same call signature and the same members —
    /// in the same file or in different ones — hash to the same name and share a
    /// single generated struct, and the second occurrence reuses the interface
    /// the first registered.
    ///
    /// Returns `None` when the type is not a callable-object surface, or when
    /// the surface mentions a type parameter (see the module docs), leaving the
    /// caller's existing lowering in charge.
    pub(in crate::lowering) fn anonymous_callable_object_type(
        &mut self,
        ty: &TSType<'_>,
    ) -> Option<smelt_hir::TypeId> {
        let members = self.callable_object_surface_members(ty).ok().flatten()?;
        self.anonymous_callable_object_from_members(members)
    }

    /// Lower an anonymous callable-object *type literal* to its interface class.
    ///
    /// The type-literal spelling (`{ (): void; m(): boolean }`) reaches lowering
    /// through `type_literal_to_hir` rather than as a whole `TSType`, so it gets
    /// its own entry point into the same synthesis.
    pub(in crate::lowering) fn anonymous_callable_object_literal_type(
        &mut self,
        literal: &oxc::ast::ast::TSTypeLiteral<'_>,
    ) -> Option<smelt_hir::TypeId> {
        let mut members = StructuralMembers::default();
        self.lower_structural_members(&literal.members, &mut members)
            .ok()?;
        if members.call_signatures.is_empty()
            || (members.fields.is_empty() && members.methods.is_empty())
        {
            return None;
        }
        self.anonymous_callable_object_from_members(members)
    }

    /// Register (or reuse) the synthetic interface for collected anonymous members.
    fn anonymous_callable_object_from_members(
        &mut self,
        members: StructuralMembers,
    ) -> Option<smelt_hir::TypeId> {
        if self.structural_members_mention_type_param(&members) {
            return None;
        }
        let name_text = self.anonymous_callable_object_name(&members);
        let name = self.intern_type_name(&name_text);
        if self.find_interface(name).is_none() {
            let span = self.span(0, 0);
            let item =
                self.register_callable_object_interface(name, &name_text, Vec::new(), members, span);
            self.items.insert(name_text, item);
        }
        Some(self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        }))
    }

    /// Derive the generated name of an anonymous callable-object surface.
    ///
    /// The name is a pure function of the structure (member names and interned
    /// types, plus the call signatures), so it is stable across the modules of
    /// one build and identical shapes deduplicate onto one struct. The
    /// `SmeltCallableObject` prefix cannot collide with a source type name,
    /// which TypeScript would have had to declare to reach this path.
    fn anonymous_callable_object_name(&self, members: &StructuralMembers) -> String {
        let mut hasher = DefaultHasher::new();
        for field in &members.fields {
            self.ctx.krate.symbols.get(field.name).hash(&mut hasher);
            field.ty.hash(&mut hasher);
            field.optional.hash(&mut hasher);
        }
        for method in &members.methods {
            self.ctx.krate.symbols.get(method.name).hash(&mut hasher);
            method.return_ty.hash(&mut hasher);
            for param in &method.params {
                param.ty.hash(&mut hasher);
            }
        }
        for signature in &members.call_signatures {
            Self::hash_function_signature(signature, &mut hasher);
        }
        format!("SmeltCallableObject{:016x}", hasher.finish())
    }

    /// Fold one call signature into the structural name hash.
    fn hash_function_signature(signature: &FunctionType, hasher: &mut DefaultHasher) {
        signature.params.hash(hasher);
        signature.rest.hash(hasher);
        signature.required_params.hash(hasher);
        signature.return_ty.hash(hasher);
        signature.is_async.hash(hasher);
    }

    /// Return whether any member of a surface mentions a generic type parameter.
    fn structural_members_mention_type_param(&self, members: &StructuralMembers) -> bool {
        members
            .fields
            .iter()
            .any(|field| self.type_mentions_type_param(field.ty))
            || members.methods.iter().any(|method| {
                self.type_mentions_type_param(method.return_ty)
                    || method
                        .params
                        .iter()
                        .any(|param| self.type_mentions_type_param(param.ty))
            })
            || members.call_signatures.iter().any(|signature| {
                self.type_mentions_type_param(signature.return_ty)
                    || signature
                        .params
                        .iter()
                        .any(|param| self.type_mentions_type_param(*param))
            })
    }

    /// Return whether a type mentions a generic type parameter anywhere inside.
    pub(in crate::lowering) fn type_mentions_type_param(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::TypeParam { .. }) => true,
            Some(
                Type::List(inner)
                | Type::Set(inner)
                | Type::Optional(inner)
                | Type::Future(inner),
            ) => self.type_mentions_type_param(*inner),
            Some(Type::Dict(key, value) | Type::JsMap(key, value)) => {
                self.type_mentions_type_param(*key) || self.type_mentions_type_param(*value)
            }
            Some(Type::Tuple(items) | Type::Union(items) | Type::Class { args: items, .. }) => {
                items
                    .iter()
                    .any(|item| self.type_mentions_type_param(*item))
            }
            Some(Type::Function(signature)) => {
                self.type_mentions_type_param(signature.return_ty)
                    || signature
                        .params
                        .iter()
                        .any(|param| self.type_mentions_type_param(*param))
            }
            Some(Type::Generator {
                yield_ty,
                return_ty,
                next_ty,
                ..
            }) => {
                self.type_mentions_type_param(*yield_ty)
                    || self.type_mentions_type_param(*return_ty)
                    || self.type_mentions_type_param(*next_ty)
            }
            Some(Type::GeneratorResult {
                yield_ty,
                return_ty,
            }) => {
                self.type_mentions_type_param(*yield_ty)
                    || self.type_mentions_type_param(*return_ty)
            }
            _ => false,
        }
    }
}
