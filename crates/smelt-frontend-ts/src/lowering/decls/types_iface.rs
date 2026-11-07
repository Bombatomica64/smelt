//! Type-level declarations: type aliases, interfaces, type namespaces and the
//! structural members they share.

use crate::lowering::support::source_spelling;
use crate::SmeltError;
use crate::lowering::state::interface_registry::LoweredInterface;
use crate::lowering::{InterfaceHeritageRef, ModuleBuilder};
use oxc::ast::ast::{
    BindingPattern, Declaration, Statement, TSNamespaceDeclarationBody, TSSignature,
};
use smelt_hir::{
    ErasureReason, Field, FunctionType, Interface, Item, MethodSig, ParamSig, Type, Visibility,
};

impl ModuleBuilder<'_> {

    /// The symbol a module-scope type declaration interns for its own name.
    ///
    /// One entry point for every type-level kind — `interface`, `type` alias,
    /// `enum` — so a name that is ambiguous across the crate takes this
    /// module's rendering of it whatever kind declared it. Ambiguity is
    /// cross-kind because the generated Rust has one type namespace: Hono's
    /// `interface Context { varIndex: number }` and its `class Context<E, P, I>`
    /// collided exactly as two classes do. See
    /// [`Self::module_qualified_type_name`] for the scheme and
    /// `manifest_type_renames` in the transpiler for how it is computed.
    ///
    /// `local_name` is the declaration's own identifier and `qualified_name` is
    /// that identifier after [`Self::qualified_type_declaration_name`] has
    /// applied the active namespace path. The rename map is keyed by the SOURCE
    /// spelling of a module-scope declaration, and the scanner that builds it
    /// only walks top-level statements, so a declaration nested in a namespace
    /// (where the two names differ) is already disambiguated by its namespace
    /// prefix and keeps that spelling untouched.
    pub(in crate::lowering) fn declared_type_name_symbol(
        &mut self,
        local_name: &str,
        qualified_name: &str,
    ) -> smelt_hir::Symbol {
        if local_name == qualified_name
            && let Some(symbol) = self.module_qualified_type_name(local_name)
        {
            return symbol;
        }
        self.intern_type_name(qualified_name)
    }

    /// Lower a TypeScript type alias declaration to HIR.
    ///
    /// An alias whose right-hand side is a *callable object* — a call signature
    /// plus named members, written either as a type literal or as an
    /// intersection — declares exactly what the `interface` spelling of the same
    /// shape declares, so it lowers to an interface item under the alias's own
    /// name and type parameters (see `decls::callable_object`). Every other
    /// alias keeps the erasing type-alias lowering below.
    pub(in crate::lowering) fn type_alias_declaration(
        &mut self,
        alias: &oxc::ast::ast::TSTypeAliasDeclaration<'_>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let local_name_text = alias.id.name.as_str();
        let name_text = self.types.qualify(local_name_text);
        let name = self.declared_type_name_symbol(local_name_text, &name_text);
        let type_params = self.push_type_parameter_scope(alias.type_parameters.as_deref())?;
        let surface = self
            .callable_object_surface_members(&alias.type_annotation)
            .ok()
            .flatten();
        if let Some(members) = surface {
            self.pop_type_parameter_scope();
            let span = self.span(alias.span.start, alias.span.end);
            let item =
                self.register_callable_object_interface(name, &name_text, type_params, members, span);
            self.items.insert(name_text, item);
            return Ok(item);
        }
        // An alias of a plain object type (`type R = { a: string; b?: number }`)
        // declares exactly the structural type `interface R { .. }` declares,
        // so it lowers to the same interface item and gets the same generated
        // struct. It used to stay an alias whose references named a record no
        // struct backed, which erased every value of the type. A literal with
        // an index signature is a dictionary and keeps the alias lowering.
        if let oxc::ast::ast::TSType::TSTypeLiteral(literal) = &alias.type_annotation
            && !literal.members.is_empty()
            && literal.members.iter().all(|member| {
                matches!(
                    member,
                    TSSignature::TSPropertySignature(_)
                        | TSSignature::TSMethodSignature(_)
                )
            })
        {
            let mut members = StructuralMembers::default();
            let lowered = self.lower_structural_members(&literal.members, &mut members);
            self.pop_type_parameter_scope();
            lowered?;
            let span = self.span(alias.span.start, alias.span.end);
            let item = self.push_lowered_interface(
                name,
                name_text.clone(),
                type_params,
                Vec::new(),
                members,
                span,
            );
            self.items.insert(name_text, item);
            return Ok(item);
        }
        let result = self.ts_type_to_hir(&alias.type_annotation);
        let decode_facts = self.alias_decode_facts_of(&alias.type_annotation);
        let fields = self.type_fields_from_ts(&alias.type_annotation).ok();
        let is_callable_object = Self::ts_type_is_callable_object_surface(&alias.type_annotation);
        self.pop_type_parameter_scope();
        let ty = result?;
        if is_callable_object {
            self.types.mark_callable_object_alias(name);
            self.ctx.callable_object_aliases.write().insert(name);
        }
        // An alias that NAMES a nominal type (`type IdContext = Context<Env,
        // '/:id'>`, `type A = SomeInterface`) is that type: a reference to the
        // alias must lower to the class or interface it names. Recording the
        // target's fields as the ALIAS's own made every reference resolve to a
        // nominal record called after the alias (see the alias arm of
        // `type_reference_to_hir`), which no struct backs, so it erased: Hono's
        // `IdContext`-typed handlers saw `c` as `SmeltUnknown` and read
        // `c.req.param(..)` dynamically. Only a structural right-hand side (an
        // object type, an intersection, a mapped type) is a record of its own.
        // Only a reference that SPELLS the class is that class: a utility
        // type over it (`Partial<Body & {..}>`, `Pick<..>`) also lowers to the
        // class symbol here, but its members differ (Hono's `BodyCache`), so
        // it keeps the alias's own fields.
        let names_nominal_type = match (self.ctx.krate.types.get(ty), &alias.type_annotation) {
            (
                Some(Type::Class { name: target, .. }),
                oxc::ast::ast::TSType::TSTypeReference(reference),
            ) if *target != name => {
                let spelled = match &reference.type_name {
                    oxc::ast::ast::TSTypeName::IdentifierReference(ident) => Some(ident.name.as_str()),
                    _ => None,
                };
                let target_name = source_spelling(&self.ctx.krate, *target);
                spelled.is_some_and(|spelled| {
                    target_name.is_some_and(|target_name| {
                        target_name == spelled
                            || self.imports.imported_name(spelled).is_some_and(|imported| imported == target_name)
                    })
                })
            }
            _ => false,
        };
        if let Some(fields) = fields
            && !fields.is_empty()
            && !names_nominal_type
        {
            self.types.set_alias_fields(name, fields.clone());
            self.ctx.type_alias_fields.write().insert(name, fields);
        } else if self.find_type_alias(name).is_some_and(|earlier| earlier.span.file == self.file_id) {
            // A later declaration of the name in this file (block-local aliases
            // of one name in successive tests) replaces the earlier one, fields
            // included.
            self.types.clear_alias_fields(name);
            self.ctx.type_alias_fields.write().remove(&name);
        }
        let item = self
            .ctx
            .krate
            .push_item(Item::TypeAlias(smelt_hir::TypeAlias {
                name,
                type_params,
                ty,
                span: self.span(alias.span.start, alias.span.end),
                decode_facts,
            }));
        self.items.insert(name_text, item);
        Ok(item)
    }

    /// Whether an interface member is an index signature over `number`.
    fn is_numeric_index_signature(member: &TSSignature<'_>) -> bool {
        matches!(member, TSSignature::TSIndexSignature(index) if matches!(
            index.parameter.type_annotation.type_annotation,
            oxc::ast::ast::TSType::TSNumberKeyword(_)
        ))
    }

    /// Lower a TypeScript interface declaration to HIR.
    pub(in crate::lowering) fn interface_declaration(
        &mut self,
        interface: &oxc::ast::ast::TSInterfaceDeclaration<'_>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let local_name_text = interface.id.name.as_str();
        let name_text = self.types.qualify(local_name_text);
        let name = self.declared_type_name_symbol(local_name_text, &name_text);
        if interface.body.body.iter().any(Self::is_numeric_index_signature) {
            self.classes.mark_array_like(name);
        }
        let type_params = self.push_type_parameter_scope(interface.type_parameters.as_deref())?;
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        let mut call_signatures = Vec::new();
        let mut construct_signatures = Vec::new();
        let mut index_value_ty = None;
        let mut decode_facts = Vec::new();

        let mut heritage_refs = Vec::new();
        let result = (|| {
            for heritage in &interface.extends {
                let (parent_name, parent_args) = self.interface_heritage(heritage)?;
                if self.ctx.krate.symbols.get(parent_name) == Some("Date") {
                    continue;
                }
                heritage_refs.push(InterfaceHeritageRef {
                    parent: parent_name,
                    args: parent_args.clone(),
                });
                let Some(parent) = self.find_interface(parent_name).cloned() else {
                    // An extended name that is not a lowerable user interface
                    // resolves instead to a type alias, a `typeof`/namespace or
                    // value import, a dotted/qualified library type, or a global
                    // ambient lib type such as `Array`/`ArrayLike`. None of these
                    // can contribute structural fields here, but TypeScript has
                    // already validated the heritage, so the child interface
                    // keeps its own members and the parent is treated as an
                    // opaque base rather than blocking the whole file.
                    continue;
                };
                let substitutions = self.type_argument_substitution(
                    &parent.type_params,
                    &parent_args,
                    self.span(heritage.span.start, heritage.span.end),
                )?;
                fields.extend(self.substituted_fields(&parent.fields, &substitutions));
                methods.extend(self.substituted_methods(&parent.methods, &substitutions));
            }

            let mut declared = StructuralMembers {
                fields: std::mem::take(&mut fields),
                methods: std::mem::take(&mut methods),
                ..StructuralMembers::default()
            };
            self.lower_structural_members(&interface.body.body, &mut declared)?;
            fields = declared.fields;
            methods = declared.methods;
            call_signatures = declared.call_signatures;
            construct_signatures = declared.construct_signatures;
            index_value_ty = declared.index_value_ty;
            decode_facts = declared.decode_facts;
            Ok(())
        })();
        self.pop_type_parameter_scope();
        result?;
        Ok(self.push_lowered_interface(
            name,
            name_text,
            type_params,
            heritage_refs,
            StructuralMembers {
                fields,
                methods,
                call_signatures,
                construct_signatures,
                index_value_ty,
                decode_facts,
            },
            self.span(interface.span.start, interface.span.end),
        ))
    }

    /// Push an interface item for lowered members and record its sidecars.
    ///
    /// Shared by `interface I { .. }` and by an object-shaped `type I = { .. }`,
    /// which declares the same structural type (see `type_alias_declaration`).
    /// Register an interface the frontend synthesizes (no source declaration),
    /// such as the record `Partial<X>` names; see
    /// `ModuleBuilder::partial_record_type`.
    pub(in crate::lowering) fn push_synthesized_interface(
        &mut self,
        name: smelt_hir::Symbol,
        name_text: String,
        members: StructuralMembers,
        span: smelt_hir::Span,
    ) -> smelt_hir::ItemId {
        let item = self.push_lowered_interface(name, name_text.clone(), Vec::new(), Vec::new(), members, span);
        self.items.insert(name_text, item);
        item
    }

    fn push_lowered_interface(
        &mut self,
        name: smelt_hir::Symbol,
        name_text: String,
        type_params: Vec<smelt_hir::TypeParamDef>,
        heritage_refs: Vec<InterfaceHeritageRef>,
        members: StructuralMembers,
        span: smelt_hir::Span,
    ) -> smelt_hir::ItemId {
        let StructuralMembers {
            mut fields,
            methods,
            call_signatures,
            construct_signatures,
            index_value_ty,
            decode_facts,
            ..
        } = members;
        // Method signatures describe callable members. Generated Rust interface
        // structs only carry data fields, so each declared method is also given
        // a function-typed storage field of the same name. This lets a value
        // typed as the interface be invoked through the ordinary field-call
        // machinery (`receiver.method` is a function value; `receiver.method()`
        // is a closure call) exactly like a class virtual-method field, and lets
        // an object literal satisfy the interface by supplying the method as a
        // property. The `methods` list is retained so class `implements`
        // validation continues to match a class method against the requirement.
        self.add_interface_method_fields(&mut fields, &methods);
        // A callable interface (`interface F { (x: T): R; prop: … }`) is, at
        // runtime, a function value that also carries own properties. The
        // generated Rust interface struct stores the ordinary data/method
        // fields, but it also needs a slot for the underlying callable so a
        // value typed as this interface can be invoked. Append a synthetic
        // `__smelt_call` storage field whose type is the first call signature's
        // `Type::Function` (for a single signature or same-shape overloads), or
        // else what `overloaded_call_signature_slot_type` answers for the set,
        // so the struct def, `Default`, `into_smelt_unknown`, and struct-literal
        // construction all pick it up through the existing field machinery. The
        // erased callable is what `into_smelt_unknown` writes under the matching
        // `"__smelt_call"` object key that call routing later extracts.
        self.add_interface_call_signature_field(&mut fields, &call_signatures);
        let interface = Item::Interface(Interface {
            name,
            span,
            type_params,
            extends: heritage_refs
                .iter()
                .map(|heritage| smelt_hir::InterfaceHeritage {
                    parent: heritage.parent,
                    args: heritage.args.clone(),
                })
                .collect(),
            fields,
            methods,
            decode_facts,
        });
        // The same declaration is lowered more than once: the predeclaration
        // passes run before every module is known, and the module's own
        // lowering runs again once they all are. Each later lowering sees more
        // of the crate (an alias another file declares), so it REPLACES the
        // earlier item instead of appending a second one. Appending left the
        // first, least-resolved copy as the one the backend emits (it keeps
        // the first item per name), while the frontend's own lookups already
        // took the latest: Hono's `ContextOptions.notFoundHandler` kept the
        // placeholder for `NotFoundHandler<E>` and erased to `SmeltUnknown`
        // (`blocker-logs/hono-crud-perf-round2.md`). Separate declarations of
        // one name (interface merging, a `.d.ts` then its `.ts`) have
        // different spans and are still appended.
        let earlier = self.ctx.krate.items.iter().position(|existing| {
            matches!(existing, Item::Interface(existing) if existing.name == name && existing.span == span)
        });
        let item = if let Some(position) = earlier {
            self.ctx.krate.items[position] = interface;
            smelt_hir::ItemId(u32::try_from(position).unwrap_or(u32::MAX))
        } else {
            self.ctx.krate.push_item(interface)
        };
        // One call records the item, the locally-lowered mark and every sidecar
        // together; see `InterfaceRegistry` for why they cannot be split.
        self.interfaces.register_lowered(LoweredInterface {
            name,
            name_text,
            item,
            extends: heritage_refs.clone(),
            call_signatures: call_signatures.clone(),
            construct_signatures: construct_signatures.clone(),
            index_value_ty,
        });
        // Mirror the same facts into the shared context for later modules.
        self.ctx.interface_extends.write().insert(name, heritage_refs);
        self.ctx
            .interface_call_signatures
            .write().insert(name, call_signatures);
        if !construct_signatures.is_empty() {
            self.ctx
                .interface_construct_signatures
                .write().insert(name, construct_signatures);
        }
        if let Some(index_value_ty) = index_value_ty {
            self.ctx.interface_index_values.write().insert(name, index_value_ty);
        }
        item
    }

    /// Add function-typed storage fields for an interface's method signatures.
    ///
    /// A TypeScript interface method such as `count(): number` is a callable
    /// member, but the generated Rust interface struct only stores data fields.
    /// This mirrors the class virtual-method-field lowering
    /// (`add_virtual_class_method_fields`): each method becomes a field of the
    /// same name whose type is the corresponding `Type::Function`, so
    /// `receiver.count` reads a callable value and `receiver.count()` lowers
    /// through the shared field-call path instead of the class-only
    /// `resolve_method` machinery. An existing data field of the same name (for
    /// example a method also declared as a property signature) is left
    /// untouched.
    pub(in crate::lowering) fn add_interface_method_fields(
        &mut self,
        fields: &mut Vec<Field>,
        methods: &[MethodSig],
    ) {
        for method in methods {
            if fields.iter().any(|field| field.name == method.name) {
                continue;
            }
            let params = method
                .params
                .iter()
                .map(|param| param.ty)
                .collect::<Vec<_>>();
            let mutable_params =
                self.mutable_params_from_returned_tuple_state(&params, method.return_ty);
            let function_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
                params,
                rest: method.rest,
                required_params: method.required_params,
                mutable_params,
                return_ty: method.return_ty,
                is_async: method.is_async,
                may_throw: false,
            }));
            fields.push(Field {
                name: method.name,
                ty: function_ty,
                visibility: method.visibility,
                optional: false,
                span: method.span,
            });
        }
    }

    /// Add the synthetic `__smelt_call` storage field for a callable interface.
    ///
    /// A TypeScript interface with one or more call signatures
    /// (`interface F { (x: T): R; … }`) describes a value that is invoked like a
    /// function while still carrying its declared data/method fields. The
    /// generated Rust interface struct cannot itself be a closure, so the
    /// underlying callable is kept in a dedicated `__smelt_call` field. Its type
    /// is `Type::Function` of the first signature when there is one signature
    /// or every overload has the same shape (`overloaded_call_signature_slot_type`
    /// answers `None`), and that helper's merged or erased slot type otherwise.
    /// Call routing invokes a value of this
    /// interface by erasing it (`into_smelt_unknown`, which writes this field
    /// under the `"__smelt_call"` object key) and extracting that callable.
    ///
    /// The name matches the object key produced by `CallableObjectAssign`
    /// construction (`callable_object_assign_text`) and read by the erased-call
    /// coercion. A pre-existing field named `__smelt_call` (which user source
    /// can legally declare) is left untouched. Interfaces with no call
    /// signature are unchanged.
    pub(in crate::lowering) fn add_interface_call_signature_field(
        &mut self,
        fields: &mut Vec<Field>,
        call_signatures: &[FunctionType],
    ) {
        let Some(signature) = call_signatures.first() else {
            return;
        };
        let name = self.ctx.krate.symbols.intern("__smelt_call");
        if fields.iter().any(|field| field.name == name) {
            return;
        }
        let function_ty = match self.overloaded_call_signature_slot_type(call_signatures) {
            Some(erased_ty) => erased_ty,
            None => self
                .ctx
                .krate
                .types
                .intern(Type::Function(signature.clone())),
        };
        let span = self.span(0, 0);
        fields.push(Field {
            name,
            ty: function_ty,
            visibility: Visibility::Public,
            optional: false,
            span,
        });
    }

    /// Return the erased variadic slot type for an *overloaded* callable interface.
    ///
    /// A callable interface may declare several call signatures of different
    /// arities:
    ///
    /// ```ts
    /// interface CurriedFunction2<T1, T2, R> {
    ///   (): CurriedFunction2<T1, T2, R>;
    ///   (t1: T1): CurriedFunction1<T2, R>;
    ///   (t1: T1, t2: T2): R;
    /// }
    /// ```
    ///
    /// The struct has exactly one `__smelt_call` slot, so no single concrete
    /// signature can store the value: which overload runs is decided by the
    /// *runtime* argument list, which is a genuine dynamic boundary, not a
    /// shape a concrete Rust `Fn` type could carry. Typing the slot from the
    /// FIRST signature — as this used to — made every call site adapt to that
    /// signature and silently discard the arguments the caller actually passed:
    /// es-toolkit's `curried(2, 3)` emitted `(smelt_callback)()`, calling the
    /// zero-argument overload and answering a defaulted value with no
    /// diagnostic.
    ///
    /// So an overload set whose signatures are not all the same collapses to one
    /// erased-rest variadic signature `(...args: unknown[]) => unknown`, exactly
    /// as [`ModuleBuilder::type_annotation`] already collapses a *union* of
    /// differing function types — the same fact spelled a different way.
    /// The precise per-call type is not lost:
    /// `interface_call_signature_type_for_args` still selects the overload from
    /// the call's argument count and argument types, and the adapter coerces
    /// the erased value into it.
    ///
    /// Signatures that differ only in their parameter *types* need the erased
    /// slot for the same reason ones of differing arity do: overloads such as
    /// `(value: string): string; (value: number): number;` describe two
    /// incompatible Rust `Fn` types, and a slot typed from the first of them
    /// cannot store — or be called as — the second.
    ///
    /// Returns `None` when every overload has the identical shape, so a
    /// single-signature (or uniformly-shaped) callable interface keeps its
    /// precise concrete slot type and nothing is erased that need not be.
    /// An overload set whose signatures share one result is not erased either:
    /// it answers the merged concrete signature from
    /// `merged_overload_call_signature`, and only a set whose result differs
    /// per overload takes the erased variadic slot.
    fn overloaded_call_signature_slot_type(
        &mut self,
        call_signatures: &[FunctionType],
    ) -> Option<smelt_hir::TypeId> {
        let mut shapes = call_signatures.iter().map(|signature| {
            (
                signature.params.clone(),
                signature.rest,
                signature.return_ty,
                signature.is_async,
            )
        });
        let first_shape = shapes.next()?;
        if !shapes.any(|shape| shape != first_shape) {
            return None;
        }
        // Overloads that share one result are one concrete signature whose
        // parameters accept every overload's arguments; only a set whose result
        // depends on the arguments needs the erased slot below. See
        // `decls::call_signature`.
        if let Some(merged) = self.merged_overload_call_signature(call_signatures) {
            return Some(self.ctx.krate.types.intern(Type::Function(merged)));
        }
        let item_ty = self.erase(ErasureReason::TypeLevel);
        let rest_list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
        // not a new erasure: same overload-slot decision recorded for the rest item
        let return_ty = self.unknown_type();
        Some(self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: vec![rest_list_ty],
            rest: Some(0),
            required_params: Some(0),
            mutable_params: Vec::new(),
            return_ty,
            is_async: false,
            may_throw: false,
        })))
    }

    /// Lower TypeScript namespace declarations that contain exported type declarations.
    /// Since oxc 0.147 an identifier-named `namespace Foo {}` is a
    /// `TSNamespaceDeclaration` with a plain `BindingIdentifier` id and a
    /// non-optional body. A string-named ambient module (`declare module "x"`)
    /// is a separate `TSExternalModuleDeclaration` node, which this frontend
    /// does not lower — matching the previous behaviour, where a string module
    /// name yielded `None` and the declaration was skipped.
    pub(in crate::lowering) fn type_namespace_declaration(
        &mut self,
        module_decl: &oxc::ast::ast::TSNamespaceDeclaration<'_>,
    ) -> Result<Vec<smelt_hir::ItemId>, SmeltError> {
        self.types.push_namespace(module_decl.id.name.to_string());
        let result = self.type_namespace_body(&module_decl.body);
        self.types.pop_namespace();
        result
    }

    /// Lower type declarations from a namespace body.
    pub(in crate::lowering) fn type_namespace_body(
        &mut self,
        body: &TSNamespaceDeclarationBody<'_>,
    ) -> Result<Vec<smelt_hir::ItemId>, SmeltError> {
        match body {
            TSNamespaceDeclarationBody::TSNamespaceDeclaration(module_decl) => {
                self.type_namespace_declaration(module_decl)
            }
            TSNamespaceDeclarationBody::TSModuleBlock(block) => {
                let mut items = Vec::new();
                for statement in &block.body {
                    match statement {
                        Statement::TSTypeAliasDeclaration(alias) => {
                            items.push(self.type_alias_declaration(alias)?);
                        }
                        Statement::TSInterfaceDeclaration(interface) => {
                            items.push(self.interface_declaration(interface)?);
                        }
                        Statement::TSNamespaceDeclaration(module_decl) => {
                            items.extend(self.type_namespace_declaration(module_decl)?);
                        }
                        Statement::ExportDeclaration(export) => {
                            match &export.declaration {
                                Declaration::TSTypeAliasDeclaration(alias) => {
                                    items.push(self.type_alias_declaration(alias)?);
                                }
                                Declaration::TSInterfaceDeclaration(interface) => {
                                    items.push(self.interface_declaration(interface)?);
                                }
                                Declaration::TSNamespaceDeclaration(module_decl) => {
                                    items.extend(self.type_namespace_declaration(module_decl)?);
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
                Ok(items)
            }
        }
    }

    // Continued in the next split builder file.
}

/// Structural members collected from a set of TypeScript type members.
///
/// A TypeScript interface body and a type literal's member list are the same
/// grammar (`TSSignature`), so one collection covers every spelling of a
/// structural type: an `interface` declaration, a `type X = { … }` alias, and
/// each object half of an intersection. Keeping the accumulators in one value
/// lets [`ModuleBuilder::lower_structural_members`] append to a partially
/// filled set (interface heritage first, then the declared members; one
/// intersection arm after another) without every caller threading five
/// separate vectors.
#[derive(Debug, Default)]
pub(in crate::lowering) struct StructuralMembers {
    /// Data fields in declaration order.
    pub(in crate::lowering) fields: Vec<Field>,
    /// Non-optional method signatures.
    pub(in crate::lowering) methods: Vec<MethodSig>,
    /// Call signatures making the type callable.
    pub(in crate::lowering) call_signatures: Vec<FunctionType>,
    /// Construct signatures (`new (): T`).
    pub(in crate::lowering) construct_signatures: Vec<FunctionType>,
    /// Value type of a string index signature, when declared.
    pub(in crate::lowering) index_value_ty: Option<smelt_hir::TypeId>,
    /// Strict-JSON decode facts of the declared properties (see
    /// `ty::decode_facts`); a later declaration of the same name replaces an
    /// earlier one, as it does in `fields`.
    pub(in crate::lowering) decode_facts: Vec<smelt_hir::FieldDecodeFacts>,
}

impl ModuleBuilder<'_> {
    /// Lower a list of TypeScript type members into structural members.
    ///
    /// This is the single implementation of "what does a member signature mean"
    /// shared by every structural spelling; see [`StructuralMembers`]. Members
    /// are appended to `members`, so heritage-derived fields already present are
    /// preserved and a later declaration of the same name wins in source order.
    pub(in crate::lowering) fn lower_structural_members(
        &mut self,
        signatures: &[TSSignature<'_>],
        members: &mut StructuralMembers,
    ) -> Result<(), SmeltError> {
        let StructuralMembers {
            fields,
            methods,
            call_signatures,
            construct_signatures,
            index_value_ty,
            decode_facts,
        } = members;
        for sig in signatures {
            match sig {
                TSSignature::TSPropertySignature(prop) => {
                    // A computed property signature keeps its declared field
                    // when the key statically resolves (`[K]`, `[E.Member]`,
                    // `[Symbol.iterator]`); a genuinely dynamic key has no
                    // named field to record and is skipped.
                    if prop.computed && !self.is_resolvable_property_key(&prop.key) {
                        continue;
                    }
                    let ty = prop
                        .type_annotation
                        .as_ref()
                        .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                        .transpose()?
                        .ok_or_else(|| {
                            SmeltError::unsupported(
                                self.span(prop.span.start, prop.span.end),
                                "interface fields require explicit type annotations",
                            )
                        })?;
                    let field_ty = if prop.optional {
                        self.ctx.krate.types.intern(Type::Optional(ty))
                    } else {
                        ty
                    };
                    let name = self.property_key_symbol(&prop.key)?;
                    if let Some(annotation) = &prop.type_annotation {
                        let facts = self.field_decode_facts(
                            name,
                            prop.optional,
                            &annotation.type_annotation,
                        );
                        decode_facts.retain(|existing| existing.name != name);
                        decode_facts.push(facts);
                    }
                    fields.push(Field {
                        name,
                        ty: field_ty,
                        visibility: Visibility::Public,
                        optional: prop.optional,
                        span: self.span(prop.span.start, prop.span.end),
                    });
                }
                TSSignature::TSMethodSignature(method) => {
                    if (method.computed && !self.is_resolvable_property_key(&method.key))
                        || method.this_param.is_some()
                    {
                        return Err(SmeltError::unsupported(
                            self.span(method.span.start, method.span.end),
                            "dynamic computed and this-parameter interface methods are not lowered yet",
                        ));
                    }
                    let _method_type_params =
                        self.push_type_parameter_scope(method.type_parameters.as_deref())?;
                    let result = (|| {
                        let return_ty = method
                            .return_type
                            .as_ref()
                            .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                            .transpose()?
                            .ok_or_else(|| {
                                SmeltError::unsupported(
                                    self.span(method.span.start, method.span.end),
                                    "interface methods require explicit return types",
                                )
                            })?;
                        let mut params = Vec::new();
                        for (index, param) in method.params.items.iter().enumerate() {
                            let ty = param
                                .type_annotation
                                .as_ref()
                                .map(|annotation| {
                                    self.ts_type_to_hir(&annotation.type_annotation)
                                })
                                .transpose()?
                                .ok_or_else(|| {
                                    SmeltError::unsupported(
                                        self.span(param.span.start, param.span.end),
                                        "interface method parameters require explicit types",
                                    )
                                })?;
                            // An optional parameter (`x?: T`) has the same
                            // `Optional<T>` ABI as an optional function-type
                            // parameter so under-application can pass a typed
                            // `None`. Recording it here keeps the callable
                            // method field's arity in sync with the
                            // `required_params` count below.
                            let ty = if param.optional {
                                self.ctx.krate.types.intern(Type::Optional(ty))
                            } else {
                                ty
                            };
                            let (param_name, param_span) =
                                if let BindingPattern::BindingIdentifier(binding) =
                                    &param.pattern
                                {
                                    (
                                        self.intern_source_name(binding.name.as_str()),
                                        self.span(binding.span.start, binding.span.end),
                                    )
                                } else {
                                    (
                                        self.synthetic_param_symbol(index),
                                        self.span(param.span.start, param.span.end),
                                    )
                                };
                            params.push(ParamSig {
                                name: param_name,
                                ty,
                                span: param_span,
                            });
                        }
                        // A trailing rest parameter (`...args: T[]`) becomes
                        // the final `List<T>` slot; its index feeds the
                        // `rest` metadata so call lowering packs the tail
                        // instead of mistaking the array for a fixed param.
                        let mut rest_index = None;
                        if let Some(rest) = &method.params.rest {
                            let rest_ty = rest
                                .type_annotation
                                .as_ref()
                                .map(|annotation| {
                                    self.function_type_rest_param_to_hir(
                                        &annotation.type_annotation,
                                    )
                                })
                                .transpose()?
                                .ok_or_else(|| {
                                    SmeltError::unsupported(
                                        self.span(rest.span.start, rest.span.end),
                                        "interface method rest parameters require explicit array types",
                                    )
                                })?;
                            // The rest binding's own identifier is purely
                            // type-level in an interface method signature, so
                            // a synthetic slot name is sufficient for the
                            // generated callable field. A fixed-width tuple
                            // rest is fixed parameters (`lowering::tuple_rest`).
                            let span = self.span(rest.span.start, rest.span.end);
                            let slot_tys = self.fixed_tuple_rest_elements(rest_ty).unwrap_or_else(|| {
                                rest_index = Some(params.len());
                                vec![rest_ty]
                            });
                            for slot_ty in slot_tys {
                                params.push(ParamSig {
                                    name: self.synthetic_param_symbol(params.len()),
                                    ty: slot_ty,
                                    span,
                                });
                            }
                        }
                        let required_params =
                            Self::formal_parameters_required_count(&method.params);
                        Ok((return_ty, params, rest_index, required_params))
                    })();
                    self.pop_type_parameter_scope();
                    let (return_ty, params, rest_index, required_params) = result?;
                    if method.optional {
                        let param_tys = params.iter().map(|param| param.ty).collect::<Vec<_>>();
                        let mutable_params = self
                            .mutable_params_from_returned_tuple_state(&param_tys, return_ty);
                        let function_ty =
                            self.ctx.krate.types.intern(Type::Function(FunctionType {
                                params: param_tys,
                                rest: rest_index,
                                required_params: Some(required_params),
                                mutable_params,
                                return_ty,
                                is_async: matches!(
                                    self.ctx.krate.types.get(return_ty),
                                    Some(Type::Future(_))
                                ),
                                may_throw: false,
                            }));
                        fields.push(Field {
                            name: self.property_key_symbol(&method.key)?,
                            ty: function_ty,
                            visibility: Visibility::Public,
                            optional: true,
                            span: self.span(method.span.start, method.span.end),
                        });
                        continue;
                    }
                    methods.push(MethodSig {
                        name: self.property_key_symbol(&method.key)?,
                        params,
                        rest: rest_index,
                        required_params: Some(required_params),
                        return_ty,
                        visibility: Visibility::Public,
                        is_async: matches!(
                            self.ctx.krate.types.get(return_ty),
                            Some(Type::Future(_))
                        ),
                        span: self.span(method.span.start, method.span.end),
                    });
                }
                TSSignature::TSCallSignatureDeclaration(signature) => {
                    call_signatures.push(self.call_signature_to_hir(signature)?);
                }
                TSSignature::TSIndexSignature(index) => {
                    *index_value_ty =
                        Some(self.ts_type_to_hir(&index.type_annotation.type_annotation)?);
                }
                TSSignature::TSConstructSignatureDeclaration(signature) => {
                    // A construct signature `new (args): T` is, at runtime,
                    // an ordinary callable value: `new value(args)` invokes
                    // it to produce a `T`. Lower it to the same
                    // `FunctionType` a `new (args) => T` constructor-type
                    // annotation produces, so a reference to this interface
                    // can resolve to a typed constructor slot (a
                    // `Type::Function`) instead of an erased dictionary. Its
                    // own type parameters are scoped so generic construct
                    // signatures resolve their parameters.
                    let _construct_type_params =
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
                                .map(|annotation| {
                                    self.ts_type_to_hir(&annotation.type_annotation)
                                })
                                .transpose()?
                                .unwrap_or_else(|| self.erase(ErasureReason::MissingAnnotation));
                            params.push(ty);
                        }
                        Ok::<_, SmeltError>((return_ty, params))
                    })();
                    self.pop_type_parameter_scope();
                    let (return_ty, params) = result?;
                    construct_signatures.push(FunctionType {
                        mutable_params: self
                            .mutable_params_from_returned_tuple_state(&params, return_ty),
                        params,
                        rest: None,
                        required_params: None,
                        return_ty,
                        is_async: false,
                        may_throw: false,
                    });
                }
            }
        }
        Ok(())
    }
}
