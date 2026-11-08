//! Class declarations and class expressions: the class item, its fields and
//! field initializers, its `extends` clause, and the names given to anonymous and
//! host-shadowing classes.

use std::borrow::Cow;
use std::collections::HashSet;
use crate::SmeltError;
use crate::lowering::{ModuleBuilder, specialization};
use crate::lowering::js_text::js_str_text;
use crate::lowering::support::visibility;
use oxc::ast::ast::{
    BindingPattern, ClassElement, Expression, MethodDefinitionKind, MethodDefinitionType,
    PropertyKey,
};
use oxc::span::GetSpan;
use smelt_hir::{
    Body, CLASS_INDEX_STORE_FIELD, Class, ErasureReason, Expr, ExprKind, Field, FunctionOwner, Item,
    Literal, Type, Visibility,
};

impl ModuleBuilder<'_> {
    /// Build a deterministic synthetic name for an anonymous class expression.
    ///
    /// Anonymous classes have no source identifier, but the rest of the class
    /// lowering pipeline keys every class on a name (the class registry, the interned
    /// `Type::Class` symbol, field/method metadata). The class span start offset
    /// is unique within a module, so `__smelt_anon_class_<offset>` gives a stable,
    /// collision-free name without threading a mutable counter through lowering.
    pub(super) fn anonymous_class_name(class: &oxc::ast::ast::Class<'_>) -> String {
        format!("__smelt_anon_class_{}", class.span.start)
    }

    /// Give a named class EXPRESSION its own type symbol when its name is a
    /// modeled host class.
    ///
    /// A named class expression binds its name only inside the class's own
    /// body, so nothing outside can name it as a type. A modeled host class of
    /// the same spelling, on the other hand, is named by every reference to the
    /// host value — and both would otherwise intern the SAME
    /// `Type::Class { name }`, leaving one type identity standing for two
    /// different Rust types: the emitted struct and the runtime type the host
    /// class lowers to.
    ///
    /// That is not a theoretical clash. es-toolkit's `isBlob`/`isFile` specs
    /// install `globalThis.File = class File extends Blob { .. }`; the class
    /// expression emits a real `File` struct while `new File(..)` in the same
    /// module keeps dispatching on the host override slot, whose native
    /// fallback is a `SmeltBlob`. One shared symbol therefore declared the
    /// struct's constructor result as `SmeltBlob` and the blob fallback as
    /// `File` — seven `error[E0308]`s in the generated crate, with nothing in
    /// the compiler able to tell the two meanings apart.
    ///
    /// So the host keeps the spelling and the class expression takes an
    /// internal symbol, the same way a `describe` callback's class declarations
    /// do (`enter_test_suite_class_scope`). Deliberately NOT applied to a class
    /// DECLARATION: that name IS in the enclosing scope, the source genuinely
    /// shadows the host class, and every reference — including from another
    /// module through an import — must keep resolving to the one symbol.
    fn host_shadowing_class_expression_name(
        &mut self,
        class: &oxc::ast::ast::Class<'_>,
        class_source_name: &str,
    ) -> Option<smelt_hir::Symbol> {
        if class.r#type != oxc::ast::ast::ClassType::ClassExpression {
            return None;
        }
        // Anonymous expressions already get a collision-free synthetic name;
        // there is no spelling to take away from the host class.
        class.id.as_ref()?;
        smelt_stdlib::typescript_stdlib_class(class_source_name)?;
        let internal_name = format!(
            "{class_source_name}SmeltClassExprF{}S{}",
            self.file_id.0, class.span.start,
        );
        // The internal name is the Rust-facing RENDERING; the source spelling
        // stays the recorded original name, because that is what reflection
        // reads. `__smelt_class` and the `instanceof` class registry both go
        // through the original name, and JavaScript answers `File` there — the
        // disambiguation exists for the Rust type system, not for the program.
        // The same expression lowered again keeps its identity; a different
        // declaration whose internal name is somehow taken gets a fresh one.
        let span = self.span(class.span.start, class.span.end);
        if let Some((_, existing)) = self.class_item_lowered_at(span) {
            return Some(existing);
        }
        let symbol = self.unclaimed_type_symbol(&internal_name);
        self.ctx.krate.names.record(symbol, class_source_name);
        self.ctx.local_class_expressions.insert(symbol);
        Some(symbol)
    }

    /// The Rust-facing symbol for a source TYPE name, qualified when the name
    /// is ambiguous across the crate.
    ///
    /// Type identity in HIR is the name symbol: `Type::Class { name }` carries
    /// it and `resolve_method` goes from it back to the class item. Two modules
    /// declaring a type of the same name therefore shared one symbol for two
    /// different types, and every method of the loser reported "unknown class
    /// method" — Hono's two `Node` classes, which stopped the router slice
    /// transpiling. `HirCtx::type_renames` is the crate-wide answer to "is
    /// this name ambiguous, and what is this module's rendering of it",
    /// computed before any module lowers.
    ///
    /// Ambiguity is CROSS-KIND, which is why this is not called
    /// `..._class_name`: the generated Rust puts a class's struct, an
    /// interface's struct, a structural alias's struct and an enum in one type
    /// namespace, so `interface Context` in one module and `class Context<E, P,
    /// I>` in another collide exactly as two classes do — Hono's is the pair
    /// that produced 437 `E0609` (`no field var_index`, because reads of the
    /// interface's field landed on the class struct) and 222 `E0107` (the class
    /// takes three type arguments and the interface none). Every declaration
    /// kind consults this one map, and the declaring module wins for its own
    /// spelling whatever kind it declared.
    ///
    /// Returns `None` for an unambiguous name so it keeps its bare spelling and
    /// every existing golden stays byte-identical. When it does rename, the
    /// SOURCE spelling is recorded as the symbol's original name, exactly as
    /// [`Self::host_shadowing_class_expression_name`] does: `instanceof` and
    /// `__smelt_class` read that, and JavaScript answers `Node` for both
    /// classes. Only the Rust type name differs.
    ///
    /// For an ambiguous name it also answers `Some` for the module whose
    /// rendering IS the bare spelling — the symbol is then the same one
    /// `intern_type_name` would give, so nothing about the emitted crate
    /// changes, but the caller binds the name in this module's own scope. That
    /// binding is the point: without it a reference to the name resolved
    /// through the crate-wide by-name item map, whose entry for an ambiguous
    /// spelling is whichever module registered last, so Hono's trie router
    /// resolved its own `Node<T>` annotation to the reg-exp router's class and
    /// read that class's fields (H61).
    pub(in crate::lowering) fn module_qualified_type_name(
        &mut self,
        class_source_name: &str,
    ) -> Option<smelt_hir::Symbol> {
        let rendered = self
            .ctx
            .type_renames
            .get(&self.path)
            .and_then(|renames| renames.get(class_source_name))?
            .clone();
        let symbol = self.ctx.krate.symbols.intern(&rendered);
        self.ctx.krate.names.record(symbol, class_source_name);
        Some(symbol)
    }

    /// Lower a class declaration to HIR.
    ///
    /// Anonymous classes (`class {}` in an expression position) are named with a
    /// deterministic synthetic identifier so they register like a declared class
    /// and can be referenced as a class value; see [`Self::anonymous_class_name`].
    pub(in crate::lowering) fn class_declaration(
        &mut self,
        class: &oxc::ast::ast::Class<'_>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        self.class_declaration_named(class, None)
    }

    /// [`Self::class_declaration`] for a class EXPRESSION that initializes a
    /// binding: `const Foo = class { … }` declares a class named `Foo`.
    ///
    /// That is the name TypeScript infers, and it is what makes `new Foo()`,
    /// `Foo` in type position, `x instanceof Foo` and `extends Foo` resolve
    /// nominally. The name is passed to exactly this call, so a class
    /// expression nested deeper inside the same initializer still takes its
    /// synthetic anonymous name. A class DECLARATION always uses its own
    /// identifier.
    pub(in crate::lowering) fn class_declaration_named(
        &mut self,
        class: &oxc::ast::ast::Class<'_>,
        binding_name: Option<&str>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        // Named classes use their source identifier; anonymous class expressions
        // fall back to a synthetic name so the class still registers and can be
        // referenced as a value. The name is owned here and borrowed as
        // `class_text` for the rest of lowering.
        // A class EXPRESSION initializing a binding is named by that binding
        // (see `class_declaration_named`).
        let class_source_name = self.forced_class_name.take().unwrap_or_else(|| {
            class.id.as_ref().map_or_else(
                || binding_name.map_or_else(|| Self::anonymous_class_name(class), str::to_owned),
                |id| id.name.to_string(),
            )
        });
        let scoped_or_host_name = self
            .classes
            .scoped_type_name(&class_source_name)
            .or_else(|| self.host_shadowing_class_expression_name(class, &class_source_name));
        // `module_qualified_type_name` changes the class's RUST NAME and
        // nothing else, so the module-local string-keyed metadata below stays
        // under the SOURCE spelling. Every reader of those maps derives its key
        // from the source name — either directly, or through
        // `krate.names.get(symbol)`, which answers the recorded original name —
        // so keying them by the qualified rendering silently missed: a renamed
        // class's own `this.#items.push(v)` stopped finding its field metadata
        // and lowered as an erased dynamic call, which is a wrong answer rather
        // than a blocker. `registry_text` is `Some` only for that case.
        let (class_name, registry_text) = match scoped_or_host_name {
            Some(name) => (name, None),
            None => match self.module_qualified_type_name(&class_source_name) {
                // A renamed class must still answer to its source spelling
                // inside its own module: `new Node()` and a `Node` type
                // annotation both resolve through the scoped type name, and the
                // class item registers under the source name (see the
                // `classes.register` at the end of this function), so binding
                // the two together here is what keeps the module internally
                // consistent while the Rust type is `Node_1`. Deliberately not
                // done for the host-shadowing case above, whose whole point is
                // that the spelling keeps resolving to the HOST class outside
                // the class expression's own body.
                Some(qualified) => {
                    self.classes
                        .bind_scoped_type_name(class_source_name.clone(), qualified);
                    (qualified, Some(class_source_name.clone()))
                }
                None => (self.intern_type_name(&class_source_name), None),
            },
        };
        let class_name_owned = registry_text.unwrap_or_else(|| {
            self.ctx
                .krate
                .symbols
                .get(class_name)
                .unwrap_or(&class_source_name)
                .to_owned()
        });
        let class_text = class_name_owned.as_str();
        let class_span = self.span(class.span.start, class.span.end);
        // One source declaration is one class item: lowering it again (test
        // setup inlined into each test) answers the item it already has.
        if let Some((item, existing)) = self.class_item_lowered_at(class_span)
            && existing == class_name
        {
            self.classes.register(class_source_name, item);
            return Ok(item);
        }
        let materialized = self.materialized_class(&class_source_name).cloned();
        if !class.decorators.is_empty() && materialized.is_none() {
            return Err(SmeltError::specialization_required(
                self.span(class.span.start, class.span.end),
                &class_source_name,
            ));
        }
        if materialized.is_some() {
            self.validate_specialization_adapters(self.span(class.span.start, class.span.end))?;
        }
        let statics_depth = self.classes.statics_in_flight_depth();
        let item = self.with_type_parameter_scope(class.type_parameters.as_deref(), |this, type_params| {
            let class_type_args = type_params
                .iter()
                .map(|param| {
                    this.ctx
                        .krate
                        .types
                        .intern(Type::TypeParam { name: param.name })
                })
                .collect::<Vec<_>>();
            let class_ty = this.ctx.krate.types.intern(Type::Class {
                name: class_name,
                args: class_type_args,
            });
            let (base, base_args) = this.class_extends_clause(class)?;
            if let Some(base_name) = base {
                this.classes
                    .set_base(class_text.to_owned(), class_name, base_name, base_args.clone());
            }
            let mut fields = Vec::new();
            // Methods this class reassigns through `this`, which are carried as
            // function-typed fields instead of inherent methods. Scanned once here
            // because both loops below need it: the field loop converts them, the
            // method loop skips them.
            let assigned_methods = methods_assigned_on_this(&class.body);
            let mut field_initializers = Vec::new();
            let mut constructor = None;
            let mut methods = Vec::new();
            let mut static_methods = Vec::new();
            // Static class constants declared directly in source (`static NAME =
            // <literal>`), as opposed to `static_fields` merged from specialization.
            let mut source_static_fields: Vec<smelt_hir::StaticField> = Vec::new();
            let mut abstract_methods = Vec::new();
            let mut descriptor_accessors = Vec::new();
            // Accessor descriptors built directly from plain source getters on a
            // non-materialized class. Materialized classes derive their descriptors
            // from the manifest instead (see `merge_materialized_class_descriptors`).
            let mut source_descriptors: Vec<smelt_hir::Descriptor> = Vec::new();
            // Value type declared by a class string/number index signature, if any.
            // Recorded from the `TSIndexSignature` element below and published to the
            // index-value maps after the class is fully lowered.
            let mut class_index_value_ty = None;

            for element in &class.body.body {
                match element {
                    // A method this class reassigns through `this` is carried as a
                    // function-typed field rather than as an inherent method, which
                    // cannot be assigned. The field's type is the method's own
                    // signature and its initializer is the method's own body, so a
                    // class that never actually assigns behaves exactly as before.
                    ClassElement::MethodDefinition(method)
                        if !method.r#static
                            && method.kind == MethodDefinitionKind::Method
                            && method.value.body.is_some()
                            && property_key_plain_name(&method.key)
                                .is_some_and(|name| assigned_methods.contains(name.as_ref())) =>
                    {
                        let name = this.property_key_symbol(&method.key)?;
                        let ty = this.method_signature_function_type(method)?;
                        field_initializers.push((
                            name,
                            ClassFieldInit::Method(&method.value),
                            ty,
                            this.span(method.span.start, method.span.end),
                        ));
                        fields.push(Field {
                            name,
                            ty,
                            visibility: visibility(method.accessibility),
                            optional: false,
                            span: this.span(method.span.start, method.span.end),
                        });
                    }
                    ClassElement::PropertyDefinition(property) => {
                        if !property.decorators.is_empty() && materialized.is_none() {
                            return Err(SmeltError::specialization_required(
                                this.span(property.span.start, property.span.end),
                                class_text,
                            ));
                        }
                        if property.computed && !this.is_resolvable_property_key(&property.key) {
                            return Err(SmeltError::unsupported(
                                this.span(property.span.start, property.span.end),
                                "dynamic computed property names are not lowered yet",
                            ));
                        }
                        if property.r#static {
                            if materialized.is_some() {
                                continue;
                            }
                            // A `static NAME: T = <literal>` declares a class-level
                            // constant resolvable via `Class.NAME`. Lower it to a
                            // materialized static field (issue #98). Non-literal or
                            // untyped static initializers are not yet lowered.
                            if let Some(static_field) = this.class_static_field(property)? {
                                source_static_fields.push(static_field);
                                continue;
                            }
                            // A static field initialized to a function or arrow
                            // expression (`static c = function () {};`) is a static
                            // *callable* member, not a data constant. Materializing it
                            // as an associated function needs the static-method
                            // lowering path, which is not wired to property
                            // initializers yet; until then the field is not emitted
                            // rather than blocking the whole class. This keeps classes
                            // that merely *carry* such a member lowerable (its value is
                            // a function, never a data value read back structurally);
                            // an actual `Class.c(...)` use would fail at the call site
                            // rather than silently reading a wrong value.
                            if matches!(
                                property.value,
                                Some(
                                    Expression::FunctionExpression(_)
                                        | Expression::ArrowFunctionExpression(_)
                                )
                            ) {
                                continue;
                            }
                            return Err(SmeltError::unsupported(
                                this.span(property.span.start, property.span.end),
                                "static fields require a concrete literal initializer",
                            ));
                        }
                        let name = this.property_key_symbol(&property.key)?;
                        let mut ty = if let Some(annotation) = &property.type_annotation {
                            this.ts_type_to_hir(&annotation.type_annotation)?
                        } else if let Some(value) = &property.value {
                            let mut field_body =
                                Body::new(None, this.span(value.span().start, value.span().end));
                            let value = this.expression(value, &mut field_body)?;
                            Self::expr_ty(&field_body, value)
                        } else {
                            return Err(SmeltError::unsupported(
                                this.span(property.span.start, property.span.end),
                                "class fields require explicit type annotations",
                            ));
                        };
                        if property.optional {
                            ty = this.field_type_with_optional(ty, true);
                        }
                        if let Some(value) = &property.value {
                            field_initializers.push((
                                name,
                                ClassFieldInit::Expression(value),
                                ty,
                                this.span(property.span.start, property.span.end),
                            ));
                        }
                        // A `#name` field is an ECMAScript *private name*, so it is not an
                        // own property of the instance (`obj['#name']` is `undefined`, and
                        // it never shows up in `Object.keys`/`JSON`/a structural compare).
                        // A source `private`/`protected` modifier is only a compile-time
                        // restriction, so such a field stays an ordinary own property.
                        let field_visibility =
                            if matches!(&property.key, PropertyKey::PrivateIdentifier(_)) {
                                Visibility::Hidden
                            } else {
                                visibility(property.accessibility)
                            };
                        fields.push(Field {
                            name,
                            ty,
                            visibility: field_visibility,
                            optional: property.optional,
                            span: this.span(property.span.start, property.span.end),
                        });
                    }
                    ClassElement::MethodDefinition(method)
                        if method.kind == MethodDefinitionKind::Get =>
                    {
                        if !method.decorators.is_empty() {
                            if materialized.is_some() {
                                continue;
                            }
                            return Err(SmeltError::specialization_required(
                                this.span(method.span.start, method.span.end),
                                class_text,
                            ));
                        }
                        if method.computed && !this.is_resolvable_property_key(&method.key) {
                            return Err(SmeltError::unsupported(
                                this.span(method.span.start, method.span.end),
                                "dynamic computed method names are not lowered yet",
                            ));
                        }
                        if method.r#static {
                            if materialized.is_some() {
                                continue;
                            }
                            // Static getters need associated-accessor lowering that
                            // is not implemented yet; static *methods* are handled in
                            // the method-lowering pass below.
                            return Err(SmeltError::unsupported(
                                this.span(method.span.start, method.span.end),
                                "static accessors are not lowered yet",
                            ));
                        }
                        // A plain source getter (`get x() { … }`) lowers to a `&self`
                        // method plus an accessor descriptor in the second class pass
                        // below, so reads of `obj.x` dispatch to the computed method
                        // instead of a dropped stored field (see the reference-class
                        // spec's finding #3). For a materialized/auto-accessor class
                        // the getter still keeps its backing field.
                        if materialized.is_some() {
                            let name = this.property_key_symbol(&method.key)?;
                            let ty = method
                                .value
                                .return_type
                                .as_ref()
                                .map(|annotation| this.ts_type_to_hir(&annotation.type_annotation))
                                .transpose()?
                                .unwrap_or_else(|| this.erase(ErasureReason::MissingAnnotation));
                            fields.push(Field {
                                // The accessor's backing storage, not a source
                                // property: it must stay out of every erased view.
                                visibility: Visibility::Hidden,
                                name,
                                ty,
                                optional: false,
                                span: this.span(method.span.start, method.span.end),
                            });
                        }
                    }
                    ClassElement::TSIndexSignature(sig) => {
                        // A class string/number index signature `[key: string]: T`
                        // (or `[key: number]: T`) declares a keyed store whose value
                        // type is the statically known `T`. Record `T` here (in the
                        // field-collection pass, before the index-value maps are
                        // published below) so member and computed access can fall
                        // back to it when no declared named field or method matches.
                        // Declared named fields are collected separately and keep
                        // their concrete types; the index value is only the
                        // dynamic-keyed fallback shape, never a replacement for named
                        // members.
                        let value_ty =
                            this.ts_type_to_hir(&sig.type_annotation.type_annotation)?;
                        class_index_value_ty = Some(value_ty);
                    }
                    _ => {}
                }
            }
            for element in &class.body.body {
                let ClassElement::MethodDefinition(method) = element else {
                    continue;
                };
                if method.kind != MethodDefinitionKind::Constructor {
                    continue;
                }
                for param in &method.value.params.items {
                    // `readonly x` (with no explicit accessibility) is still a
                    // parameter property and declares a public instance field, so a
                    // constructor arg such as `constructor(readonly innerError: Error)`
                    // must produce a field (was E0609 on `.innerError`).
                    if param.accessibility.is_none() && !param.readonly {
                        continue;
                    }
                    let BindingPattern::BindingIdentifier(binding) = &param.pattern else {
                        return Err(SmeltError::unsupported(
                            this.span(param.span.start, param.span.end),
                            "destructured constructor parameter properties are not lowered yet",
                        ));
                    };
                    let name = this.intern_source_name(binding.name.as_str());
                    let ty = if let Some(annotation) = &param.type_annotation {
                        this.ts_type_to_hir(&annotation.type_annotation)?
                    } else if let Some(default) = &param.initializer {
                        let mut initializer_body =
                            Body::new(None, this.span(default.span().start, default.span().end));
                        let value = this.expression(default, &mut initializer_body)?;
                        Self::expr_ty(&initializer_body, value)
                    } else {
                        this.erase(ErasureReason::MissingAnnotation)
                    };
                    fields.push(Field {
                        name,
                        ty,
                        visibility: visibility(param.accessibility),
                        optional: false,
                        span: this.span(param.span.start, param.span.end),
                    });
                }
            }
            let mut static_fields = materialized
                .as_ref()
                .map_or_else(Vec::new, |materialized_class| {
                    this.merge_materialized_class_members(materialized_class, &mut fields, class_span)
                });
            // Source-declared `static NAME = <literal>` constants extend the
            // materialized set; a materialized value takes precedence when a member
            // is named by both, so only append source statics not already present.
            for static_field in source_static_fields {
                if !static_fields
                    .iter()
                    .any(|existing| existing.name == static_field.name)
                {
                    static_fields.push(static_field);
                }
            }
            this.classes.push_statics_in_flight(class_source_name.clone(), static_fields.clone());
            let method_sigs = this.class_method_signatures(&class.body.body)?;
            let virtual_method_fields =
                this.virtual_method_field_names(&class.body.body, class.r#abstract);
            this.add_virtual_class_method_fields(&mut fields, &method_sigs, &virtual_method_fields);
            // Runtime keyed store for a class index signature (issue #84 / #18).
            //
            // A class declaring `[key: string]: T` (or `[key: number]: T`) needs a
            // real backing store so dynamic keyed writes round-trip at runtime:
            // `x[k] = v; x[k]` must return `v`, and a missing key must read as
            // `undefined`. The store is a synthesized private `Record<string, T>`
            // field that sits ALONGSIDE the declared named fields (which stay
            // concrete). It is added to the class field list here, so it flows
            // through MIR, struct emission, the `Default`/`Clone` derives, and the
            // constructor's default struct literal automatically; codegen only has
            // to route keyed access to it. The field is `Hidden` so it is excluded
            // from erased object projection. Number-keyed signatures still use a
            // string-keyed store because JavaScript object property keys are strings.
            //
            // Value type `T` stays concrete (`string`, `number`, a class, ...). A
            // union index value type (`[key: string]: string | number`) is subject
            // to the SAME pre-existing limitation as a plain union-typed class field:
            // the generated union enum lacks `Debug`/`Default`, so a struct storing
            // it does not derive them. That is an orthogonal codegen gap tracked
            // separately, not specific to the index store.
            if let Some(value_ty) = class_index_value_ty {
                let store_name = this.intern_source_name(CLASS_INDEX_STORE_FIELD);
                let key_ty = this.ctx.krate.types.intern(Type::String);
                let store_ty = this.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
                if !fields.iter().any(|field| field.name == store_name) {
                    fields.push(Field {
                        name: store_name,
                        ty: store_ty,
                        // Compiler-synthesized keyed storage, never a source property.
                        visibility: Visibility::Hidden,
                        optional: false,
                        span: class_span,
                    });
                }
            }
            this.classes.set_fields(class_text.to_owned(), fields.clone());
            this.classes.set_methods(class_text.to_owned(), method_sigs.clone());
            // Publish the class index-signature value type (if any) so member and
            // computed access can resolve keyed reads/writes through it while the
            // class's own method bodies are still being lowered, and so later
            // modules see it too. Named fields resolve first; this is the fallback.
            //
            // This drives the *type* level of the index signature (issue #84): the
            // statically known value type `T` types keyed reads as `Optional<T>`
            // (missing key -> undefined) and undeclared member reads as `T`, while
            // declared named fields keep their concrete types. The matching runtime
            // keyed *store* — the synthesized `__smelt_index_store` field added above
            // — makes dynamic writes round-trip: codegen routes `x[k] = v` to
            // `x.__smelt_index_store.insert(k, v)` and `x[k]` to a store lookup.
            if let Some(value_ty) = class_index_value_ty {
                this.classes.set_index_value(class_name, value_ty);
                this.ctx.class_index_values.write().insert(class_name, value_ty);
            }
            this.add_overridden_base_method_fields(base, &method_sigs);

            for element in &class.body.body {
                match element {
                    ClassElement::PropertyDefinition(_) => {}
                    ClassElement::MethodDefinition(method) => {
                        // Already lowered as a function-typed field above, because
                        // this class assigns over it through `this`.
                        if !method.r#static
                            && method.kind == MethodDefinitionKind::Method
                            && method.value.body.is_some()
                            && property_key_plain_name(&method.key)
                                .is_some_and(|name| assigned_methods.contains(name.as_ref()))
                        {
                            continue;
                        }
                        // An overload declaration has no JavaScript body. TypeScript
                        // validates overload applicability before Smelt runs; when
                        // the class also contains the matching concrete method, only
                        // that runtime implementation becomes HIR.
                        if method.value.body.is_none()
                            && this.class_method_has_implementation(&class.body.body, method)?
                        {
                            continue;
                        }
                        if !method.decorators.is_empty() && materialized.is_none() {
                            return Err(SmeltError::specialization_required(
                                this.span(method.span.start, method.span.end),
                                class_text,
                            ));
                        }
                        // Plain source getters AND setters are lowered as class
                        // methods (`__smelt_get_x` / `__smelt_set_x`) and registered
                        // as one accessor descriptor per property name below, which
                        // is what routes `obj.x` and `obj.x = v` to them.
                        if method.computed && !this.is_resolvable_property_key(&method.key) {
                            return Err(SmeltError::unsupported(
                                this.span(method.span.start, method.span.end),
                                "dynamic computed method names are not lowered yet",
                            ));
                        }
                        if method.r#static {
                            if materialized.is_some() {
                                continue;
                            }
                            // Only plain `static foo(..)` methods are lowered as
                            // receiver-free associated functions (issue #98). Static
                            // getters/setters and computed static keys are deferred.
                            if method.kind != MethodDefinitionKind::Method {
                                return Err(SmeltError::unsupported(
                                    this.span(method.span.start, method.span.end),
                                    "static accessors are not lowered yet",
                                ));
                            }
                            let item = this.class_static_function(
                                class_text,
                                class_name,
                                method,
                            )?;
                            static_methods.push(item);
                            continue;
                        }
                        if method.r#type == MethodDefinitionType::TSAbstractMethodDefinition {
                            if !matches!(method.kind, MethodDefinitionKind::Method) {
                                return Err(SmeltError::unsupported(
                                    this.span(method.span.start, method.span.end),
                                    "abstract constructors are not lowered yet",
                                ));
                            }
                            let sig = this.abstract_class_method_sig(method)?;
                            abstract_methods.push(sig);
                            continue;
                        }
                        if !matches!(
                            method.kind,
                            MethodDefinitionKind::Constructor
                                | MethodDefinitionKind::Method
                                | MethodDefinitionKind::Get
                                | MethodDefinitionKind::Set
                        ) {
                            return Err(SmeltError::unsupported(
                                this.span(method.span.start, method.span.end),
                                "getters and setters are not lowered yet",
                            ));
                        }
                        let item = if method.kind == MethodDefinitionKind::Constructor {
                            if constructor.is_some() {
                                return Err(SmeltError::unsupported(
                                    this.span(method.span.start, method.span.end),
                                    "duplicate constructors are not allowed",
                                ));
                            }
                            let item = this.class_function(
                                class_text,
                                class_name,
                                class_ty,
                                method,
                                true,
                                &field_initializers,
                            )?;
                            constructor = Some(item);
                            item
                        } else {
                            let item = this.class_function(
                                class_text,
                                class_name,
                                class_ty,
                                method,
                                false,
                                &[],
                            )?;
                            if matches!(
                                method.kind,
                                MethodDefinitionKind::Get | MethodDefinitionKind::Set
                            ) {
                                descriptor_accessors.push((
                                    this.property_key_symbol(&method.key)?,
                                    method.kind,
                                    item,
                                ));
                            }
                            // Register a source-level accessor descriptor for a plain
                            // getter or setter so member reads dispatch to the
                            // computed method and member WRITES dispatch to the
                            // setter. A property with both accessors is ONE
                            // descriptor carrying both halves, whichever order the
                            // two appear in the class body, because that is what the
                            // read and write emitters look up by property name.
                            if materialized.is_none()
                                && matches!(
                                    method.kind,
                                    MethodDefinitionKind::Get | MethodDefinitionKind::Set
                                )
                            {
                                let name = this.property_key_symbol(&method.key)?;
                                match method.kind {
                                    MethodDefinitionKind::Get => {
                                        // `Unknown` is interned only when an
                                        // accessor genuinely has no annotation:
                                        // interning it unconditionally would add an
                                        // `Unknown` entry to the type table of every
                                        // crate that declares any class method,
                                        // which flips crate-wide erasure decisions
                                        // (record backing, the erased prelude) for
                                        // code that has no erased value at all.
                                        let read_ty = match method.value.return_type.as_ref() {
                                            Some(annotation) => {
                                                this.ts_type_to_hir(&annotation.type_annotation)?
                                            }
                                            None => this.erase(ErasureReason::MissingAnnotation),
                                        };
                                        if let Some(existing) = source_descriptors
                                            .iter_mut()
                                            .find(|descriptor| descriptor.name == name)
                                        {
                                            existing.read_ty = read_ty;
                                            existing.getter = Some(item);
                                        } else {
                                            source_descriptors.push(smelt_hir::Descriptor {
                                                name,
                                                read_ty,
                                                write_ty: None,
                                                getter: Some(item),
                                                setter: None,
                                                data_descriptor: false,
                                                is_static: false,
                                                value_fields: Vec::new(),
                                            });
                                        }
                                    }
                                    MethodDefinitionKind::Set => {
                                        // The written type is the setter's own
                                        // parameter type; a setter takes exactly one
                                        // parameter, and an unannotated one is the
                                        // erased boundary the same way an
                                        // unannotated getter return is.
                                        let write_ty = match method
                                            .value
                                            .params
                                            .items
                                            .first()
                                            .and_then(|param| param.type_annotation.as_ref())
                                        {
                                            Some(annotation) => {
                                                this.ts_type_to_hir(&annotation.type_annotation)?
                                            }
                                            None => this.erase(ErasureReason::MissingAnnotation),
                                        };
                                        if let Some(existing) = source_descriptors
                                            .iter_mut()
                                            .find(|descriptor| descriptor.name == name)
                                        {
                                            existing.write_ty = Some(write_ty);
                                            existing.setter = Some(item);
                                        } else {
                                            source_descriptors.push(smelt_hir::Descriptor {
                                                name,
                                                // A set-only property has no getter,
                                                // so no READ ever dispatches through
                                                // this descriptor (a getterless
                                                // descriptor read is an emit error,
                                                // and JavaScript answers `undefined`
                                                // for one). The slot mirrors the
                                                // written type rather than interning
                                                // `Unknown`, which would add an
                                                // erased entry to the crate's type
                                                // table for a value that never
                                                // exists.
                                                read_ty: write_ty,
                                                write_ty: Some(write_ty),
                                                getter: None,
                                                setter: Some(item),
                                                data_descriptor: false,
                                                is_static: false,
                                                value_fields: Vec::new(),
                                            });
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            methods.push(item);
                            item
                        };
                        let _ = item;
                    }
                    ClassElement::AccessorProperty(accessor) => {
                        if materialized.is_some() {
                            continue;
                        }
                        if !accessor.decorators.is_empty() {
                            return Err(SmeltError::specialization_required(
                                this.span(accessor.span.start, accessor.span.end),
                                class_text,
                            ));
                        }
                        return Err(SmeltError::unsupported(
                            this.span(accessor.span.start, accessor.span.end),
                            "accessor properties are not lowered yet",
                        ));
                    }
                    ClassElement::StaticBlock(block) => {
                        return Err(SmeltError::unsupported(
                            this.span(block.span.start, block.span.end),
                            "static blocks are not lowered yet",
                        ));
                    }
                    ClassElement::TSIndexSignature(_) => {
                        // The class index signature's value type is collected in the
                        // field-collection pass above and published to the
                        // index-value maps; nothing further is emitted for the
                        // signature member itself here.
                    }
                }
            }

            if let Some(materialized_class) = &materialized {
                let lifted_methods = materialized_class
                    .methods
                    .iter()
                    .filter(|method| {
                        method.binding_mode == smelt_specialize::BindingMode::Instance
                            && this.materialized_callable_needs_lift(&method.callable)
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                for materialized_method in lifted_methods {
                    let method_name =
                        this.intern_source_name(specialization::materialized_member_name(
                            &materialized_method.name,
                        ));
                    let lifted = this.lift_materialized_class_callable(
                        &materialized_method.callable,
                        &materialized_method.signature,
                        specialization::materialized_member_name(&materialized_method.name),
                        class_name,
                        class_ty,
                    )?;
                    let original_name =
                        this.intern_source_name(&format!("__smelt_original_{}", materialized_method.name));
                    let originals = methods
                        .iter()
                        .copied()
                        .filter(|item| {
                            matches!(
                                this.item_ref(*item),
                                Item::Function(function) if function.name == method_name
                            )
                        })
                        .collect::<Vec<_>>();
                    for original in originals {
                        let index = usize::try_from(original.0).unwrap_or(usize::MAX);
                        if let Some(Item::Function(function)) =
                            this.ctx.krate.items.get_mut(index)
                        {
                            function.name = original_name;
                            function.owner = FunctionOwner::ClassMethod {
                                class: class_name,
                                method: original_name,
                            };
                        }
                    }
                    methods.push(lifted);
                }
            }

            if constructor.is_none() {
                constructor = Some(this.synthesize_default_class_constructor(
                    class_text,
                    class_name,
                    class_ty,
                    base.is_some(),
                    &field_initializers,
                    this.span(class.span.start, class.span.end),
                )?);
            }
            let mut instance_initializers = Vec::new();
            if let Some(materialized_class) = &materialized {
                let initializer_signature = smelt_specialize::FunctionSignature {
                    parameters: Vec::new(),
                    return_type: smelt_specialize::StaticType::Null,
                    is_async: false,
                    throws: false,
                };
                for initializer in materialized_class.initializers.iter().filter(|initializer| {
                    initializer.kind == smelt_specialize::InitializerKind::Instance
                }) {
                    let hidden_name = format!("__smelt_initializer_{}", initializer.order);
                    let item = this.lift_materialized_class_callable(
                        &initializer.callable,
                        &initializer_signature,
                        &hidden_name,
                        class_name,
                        class_ty,
                    )?;
                    methods.push(item);
                    instance_initializers.push((
                        item,
                        initializer.target_kind.clone(),
                        initializer.target_name.clone(),
                    ));
                }
            }
            if let Some(constructor) = constructor {
                this.prepend_materialized_initializer_calls(
                    constructor,
                    &instance_initializers,
                    class_ty,
                    class_span,
                )?;
            }
            let descriptors = materialized.as_ref().map_or_else(
                || source_descriptors,
                |materialized_class| {
                    this.merge_materialized_class_descriptors(
                        materialized_class,
                        &descriptor_accessors,
                        class_span,
                    )
                },
            );

            let implements = class
                .implements
                .iter()
                .filter_map(|imp| this.implements_reference(imp).transpose())
                .collect::<Result<Vec<_>, _>>()?;
            this.inject_error_marker_fields(base, class_span, &mut fields);
            let item = this.ctx.krate.push_item(Item::Class(Class {
                name: class_name,
                span: this.span(class.span.start, class.span.end),
                kind: if class.r#abstract {
                    smelt_hir::ClassKind::Abstract
                } else {
                    smelt_hir::ClassKind::Plain
                },
                type_params,
                base,
                base_args,
                fields,
                static_fields,
                descriptors,
                constructor,
                methods,
                static_methods,
                abstract_methods,
                implements,
                protocols: Vec::new(),
            }));
            Ok(item)
        });
        self.classes.truncate_statics_in_flight(statics_depth);
        let item = item?;
        self.classes.register(class_source_name, item);
        self.validate_implements(item)?;
        Ok(item)
    }

    /// Lower a class *expression* used in value position into a class value.
    ///
    /// A class expression (`class Foo {}` or anonymous `class {}` used as an
    /// operand, e.g. an array element `[class {}]`) evaluates to the class
    /// constructor. We lower it through the same [`Self::class_declaration`]
    /// pipeline so the class is registered (fields, methods, constructor, and a
    /// `Type::Class` symbol) exactly like a declared class, then materialize the
    /// class value the same way a bare class-name identifier does in
    /// `identifier_expression`: a `Literal::None` placeholder typed as the class
    /// so downstream code carries the class identity through the type.
    pub(in crate::lowering) fn class_expression_value(
        &mut self,
        class: &oxc::ast::ast::Class<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let item = self.class_declaration(class)?;
        let Item::Class(lowered) = self.item_ref(item) else {
            return Err(SmeltError::unsupported(
                self.span(class.span.start, class.span.end),
                "class expression did not lower to a class item",
            ));
        };
        let name = lowered.name;
        let ty = self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty,
            span: self.span(class.span.start, class.span.end),
        }))
    }

    /// Lower the single supported TypeScript `extends` shape for class declarations.
    pub(in crate::lowering) fn class_extends_clause(
        &mut self,
        class: &oxc::ast::ast::Class<'_>,
    ) -> Result<(Option<smelt_hir::Symbol>, Vec<smelt_hir::TypeId>), SmeltError> {
        // Since oxc 0.147 the superclass expression and its type arguments live
        // together on `Class::heritage`.
        let Some(heritage) = &class.heritage else {
            return Ok((None, Vec::new()));
        };
        let super_class = &heritage.expression;
        let name = match super_class {
            Expression::Identifier(identifier) => identifier.name.to_string(),
            Expression::StaticMemberExpression(member) => {
                let Expression::Identifier(object) = &member.object else {
                    return Err(SmeltError::unsupported(
                        self.span(super_class.span().start, super_class.span().end),
                        "class extends currently requires a direct base class identifier or imported namespace member",
                    ));
                };
                if !self.imports.is_value(object.name.as_str())
                    && !self.imports.is_namespace(object.name.as_str())
                {
                    return Err(SmeltError::unsupported(
                        self.span(super_class.span().start, super_class.span().end),
                        "class extends currently requires a direct base class identifier or imported namespace member",
                    ));
                }
                format!("{}.{}", object.name, member.property.name)
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(super_class.span().start, super_class.span().end),
                    "class extends currently requires a direct base class identifier or imported namespace member",
                ));
            }
        };
        let name = name.as_str();
        // `class X extends Object {}` names the universal root constructor as its
        // base. Every JavaScript class already descends from `Object`, so an
        // explicit `extends Object` contributes no fields, methods, or distinct
        // constructor behavior: the subclass behaves exactly like a base-less
        // class (its instances are still non-plain objects with their own
        // constructor identity). Model it as no declared base rather than
        // requiring an `Object` class item that Smelt does not synthesize. A
        // user-declared class or value import literally named `Object` shadows
        // the global and is handled through the normal declared-base path below.
        if name == "Object"
            && !self.classes.contains(name)
            && !self.imports.is_value(name)
        {
            return Ok((None, Vec::new()));
        }
        // An `extends` clause names a TYPE, so it resolves like every other type
        // reference in this module: through the module's import aliases and
        // through the crate rename map for a name two modules both declare.
        // Interning the LOCAL spelling instead recorded a base class that no
        // module declares (`import { Hono as HonoBase }` recorded `HonoBase`),
        // so every inherited field and method was silently dropped (`E0609`).
        // The resolution is the identity for a local, unambiguous name, which is
        // why this stays a general rule rather than an alias special case.
        let mut base = self.resolve_type_reference_symbol(name);
        // …and when the name is bound to a class DECLARED UNDER ANOTHER SPELLING,
        // the declaration is what the base chain has to record. An export alias
        // (`class Hono {..}; export { Hono as HonoBase }`) is exactly that: the
        // importing module sees `HonoBase`, no module declares a class of that
        // name, and the type-reference resolution above answers with the bare
        // alias spelling. The class registry knows which declaration the alias
        // is bound to, so the alias resolves to that class's own symbol. This
        // only fires when the resolved symbol is not itself a declared class, so
        // a locally declared or renamed base keeps the resolution above.
        if self.class_by_symbol(base).is_none()
            && let Some(item) = self.classes.item(name)
            && let Item::Class(declared) = self.item_ref(item)
        {
            base = declared.name;
        }
        // A modeled JavaScript host constructor (`Blob`, `File`, `ArrayBuffer`, the
        // boxed primitive wrappers, …) is a legitimate base even though it is not a
        // source-declared class: `smelt_stdlib::host_object` gives it a concrete
        // constructed identity, so `class File extends Blob {}` resolves the same
        // way the error/collection builtins below do. Keying on the shared registry
        // covers modeled host constructors; the error and collection builtins below
        // are a hard-coded name list kept alongside it.
        let allowed_builtin = smelt_stdlib::host_object_by_class(name).is_some()
            || smelt_stdlib::is_error_class_name(name)
            || matches!(name, "Date" | "Map" | "ReadonlyMap" | "Set" | "ReadonlySet");
        if !allowed_builtin
            && !self.classes.contains(name)
            && !self.imports.is_value(name)
            && !name.contains('.')
        {
            return Err(SmeltError::unsupported(
                self.span(super_class.span().start, super_class.span().end),
                format!("base class `{name}` is not declared"),
            ));
        }
        let mut args = heritage
            .type_arguments
            .as_ref()
            .map(|type_args| {
                type_args
                    .params
                    .iter()
                    .map(|arg| self.ts_type_to_hir(arg))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default();
        // A heritage clause is a type reference and obeys the same rule as any
        // other: trailing type arguments the BASE declares a default for may be
        // omitted (`class Derived extends Slot<boolean>` where the base is
        // `Slot<T, U = string, V = number>` means `extends Slot<boolean, string,
        // number>`). The full list matters here beyond arity: a derived class is
        // monomorphized against `base_args`, so a missing argument leaves the
        // base's own parameter name standing in an inherited member's signature,
        // where nothing declares it. See
        // `ModuleBuilder::type_arguments_with_defaults`.
        if let Some(base_type_params) = self
            .ctx
            .class_type_params
            .get(&base)
            .cloned()
            .or_else(|| self.find_class(base).map(|class| class.type_params.clone()))
            && let Some(completed) = self.type_arguments_with_defaults(&base_type_params, &args)
        {
            args = completed;
        }
        Ok((Some(base), args))
    }
}

/// What a class field's initializer is lowered from.
///
/// A declared field carries a source expression. A method reassigned through
/// `this` (see [`methods_assigned_on_this`]) is lowered as a function-typed
/// field whose initializer is the method's own body, so it needs the function
/// rather than an expression: an inherent Rust method cannot be assigned, and
/// there is no source expression to point at.
#[derive(Clone, Copy)]
pub(in crate::lowering) enum ClassFieldInit<'a> {
    /// A declared field's `= <expr>` initializer.
    Expression(&'a Expression<'a>),
    /// A reassigned method's own body, lowered as a function value.
    Method(&'a oxc::ast::ast::Function<'a>),
}

/// The plain source spelling of a property key, when it has one.
///
/// Used to match a method against the names collected by
/// [`methods_assigned_on_this`], which are syntactic: a `this.<name> = ...`
/// target is always a static identifier or a string-literal key, so those are
/// the only two spellings that can match. A private name is deliberately
/// excluded -- `this.#m = ...` targets a private slot, not a method member.
fn property_key_plain_name<'a>(key: &'a PropertyKey<'a>) -> Option<Cow<'a, str>> {
    match key {
        PropertyKey::StaticIdentifier(ident) => Some(Cow::Borrowed(ident.name.as_str())),
        PropertyKey::StringLiteral(lit) => Some(js_str_text(lit.value)),
        _ => None,
    }
}

/// Collect the method names a class body reassigns through `this.<name> = ...`.
///
/// JavaScript lets an instance replace one of its own methods at runtime, and
/// Hono's `SmartRouter` does exactly that: it picks a concrete router on the
/// first request and then rewrites its own `match` so later requests skip the
/// selection loop (`smart-router/router.ts`). A Rust inherent method cannot be
/// assigned, so such a member is lowered as a function-typed FIELD initialised
/// to the method's own body instead, which the emitter already knows how to
/// carry (an `Rc<dyn Fn(..)>` field, reassignable, with calls dispatching
/// through it).
///
/// The scan is deliberately syntactic and deliberately narrow: only
/// `this.<name> = ...` inside the class's own body, where the receiver names
/// the class being lowered without any type resolution. Assignment through an
/// instance from outside the class (`instance.method = ...`) needs the
/// receiver's class, which is not knowable from the AST, and stays a named
/// blocker rather than being guessed at. Every member this converts, that
/// general rule would also convert, so the narrow form does not have to be
/// unwound to widen it later.
///
/// Walking continues into nested nodes, so an assignment inside a callback,
/// a loop body or a nested closure in the class body is recorded too.
fn methods_assigned_on_this(body: &oxc::ast::ast::ClassBody<'_>) -> HashSet<String> {
    let mut names = HashSet::new();
    let mut collector = ThisMethodAssignmentCollector { names: &mut names };
    for element in &body.body {
        oxc::ast_visit::Visit::visit_class_element(&mut collector, element);
    }
    names
}

/// AST collector for `this.<name> = ...` writes anywhere in a class body.
struct ThisMethodAssignmentCollector<'names> {
    /// Accumulates each member name assigned through `this`.
    names: &'names mut HashSet<String>,
}

impl<'a> oxc::ast_visit::Visit<'a> for ThisMethodAssignmentCollector<'_> {
    /// Records the member name when an assignment writes through `this`.
    fn visit_assignment_expression(&mut self, assign: &oxc::ast::ast::AssignmentExpression<'a>) {
        if let Some(name) = assignment_target_this_member_name(&assign.left) {
            self.names.insert(name.into_owned());
        }
        oxc::ast_visit::walk::walk_assignment_expression(self, assign);
    }
}

/// Yield the member name of an assignment target of the form `this.<name>`.
///
/// Both the static spelling and a string-literal computed key are recognised,
/// mirroring `assignment_target_host_global_name`. An optional (`this?.x`) or
/// dynamically computed target yields nothing.
fn assignment_target_this_member_name<'a>(
    target: &'a oxc::ast::ast::AssignmentTarget<'a>,
) -> Option<Cow<'a, str>> {
    use oxc::ast::ast::AssignmentTarget;
    let (object, property) = match target {
        AssignmentTarget::StaticMemberExpression(member) if !member.optional => {
            (&member.object, Cow::Borrowed(member.property.name.as_str()))
        }
        AssignmentTarget::ComputedMemberExpression(member) if !member.optional => {
            let Expression::StringLiteral(key) = &member.expression else {
                return None;
            };
            (&member.object, js_str_text(key.value))
        }
        _ => return None,
    };
    matches!(object, Expression::ThisExpression(_)).then_some(property)
}
