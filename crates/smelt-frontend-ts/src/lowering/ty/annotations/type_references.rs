//! Type reference resolution: a `TSTypeReference` name (local, imported,
//! qualified, renamed, or a global lib type) resolved to its HIR type, and the
//! checks for names a source module declares or shadows.

use std::collections::HashMap;
use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::resolve::TypeBinding;
use oxc::ast::ast::TSTypeName;
use smelt_hir::{ErasureReason, FunctionType, Item, Type};

impl ModuleBuilder<'_> {
    /// Whether any class, interface or type alias of the crate is named
    /// `name_text`.
    pub(super) fn declared_type_name_exists(&mut self, name_text: &str) -> bool {
        let symbol = self.intern_type_name(name_text);
        self.ctx.krate.items.iter().any(|item| match item {
            Item::Class(class) => class.name == symbol,
            Item::Interface(interface) => interface.name == symbol,
            Item::TypeAlias(alias) => alias.name == symbol,
            _ => false,
        })
    }

    /// Convert type reference to HIR type.
    pub(in crate::lowering) fn type_reference_to_hir(
        &mut self,
        reference: &oxc::ast::ast::TSTypeReference<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        // Bun's `int32` nested in another type is a `number`; the whole-type
        // position never reaches here (`ts_type_to_hir` answers it first).
        if self.is_bun_int32_reference(reference) {
            return Ok(self.ctx.krate.types.intern(Type::Float));
        }
        let name_text = match &reference.type_name {
            TSTypeName::IdentifierReference(name) => name.name.to_string(),
            TSTypeName::QualifiedName(qualified) => Self::qualified_type_name_text(qualified),
            TSTypeName::ThisExpression(_) => {
                return Err(SmeltError::unsupported(
                    self.span(reference.span.start, reference.span.end),
                    "unsupported type reference name",
                ));
            }
        };
        // One resolver decides what the name is: a type parameter in scope
        // first, then a declaration or import of this module, then the lib.
        // Only an unqualified identifier can name a type parameter or shadow
        // the lib (a qualified `NS.Set` never reaches the lib table's
        // spellings).
        let binding = match &reference.type_name {
            TSTypeName::IdentifierReference(_) => self.resolve_type_name(&name_text),
            _ => self.types.param_type(&name_text).map_or(TypeBinding::Source, TypeBinding::Param),
        };
        if let TypeBinding::Param(param_ty) = binding {
            if reference.type_arguments.is_some() {
                return Err(SmeltError::unsupported(
                    self.span(reference.span.start, reference.span.end),
                    "type parameters cannot be used with type arguments",
                ));
            }
            return Ok(param_ty);
        }
        let args = reference
            .type_arguments
            .as_ref()
            .map(|args| args.params.iter().collect::<Vec<_>>())
            .unwrap_or_default();
        // An ambient fetch init dictionary becomes a real interface, so a
        // parameter typed by one is a generated struct rather than an erased
        // record. Asked before the table below because it declines for a name
        // the source declares itself, which the table cannot express.
        if let Some(init_ty) = self.ambient_init_interface_type(&name_text)
            && args.is_empty()
        {
            return Ok(init_ty);
        }
        // The table below lowers the GLOBAL lib types by spelling. A name the
        // module declares itself shadows the global one, so it skips the table
        // and resolves through the declaration path in the fallback arm; an
        // empty spelling matches no builtin arm.
        let builtin_spelling = if binding == TypeBinding::Lib { name_text.as_str() } else { "" };
        // `Omit` with a fixed key set over a known record. Anything else keeps
        // the opaque reference below, whose `T` a call site still instantiates.
        // `builtin_spelling` is only `Omit` when the resolver answered
        // `TypeBinding::Lib`, so a source `Omit` never reaches here.
        if let ("Omit", [base, keys]) = (builtin_spelling, args.as_slice())
            && let Some(removed) = self.literal_record_keys(keys)
        {
            let lowered = self.ts_type_to_hir(base)?;
            let span = self.span(reference.span.start, reference.span.end);
            if let Some(omitted) = self.omitted_record_type(lowered, &removed, span)? {
                return Ok(omitted);
            }
        }
        match (builtin_spelling, args.as_slice()) {
            ("RegExp", []) => Ok(self.regexp_type()),
            // The lib's `type PropertyKey = string | number | symbol`: a VALUE
            // of this type keeps its kind (`typeof` still tells a number from a
            // string), so it is the union of the three, not the converted key a
            // record stores (`Type::PropKey`). A source declaration of the name
            // never reaches here (`builtin_spelling` is empty for it).
            ("PropertyKey", []) => Ok(self.property_key_value_type()),
            // `BodyInit` is a UNION, not an opaque class. Leaving it opaque made
            // `JSON.stringify(body)` report "value must be JSON-serializable
            // (got Class `BodyInit`)" for a value that is a `string` on every
            // path a program actually takes, and it hid the string arm from body
            // construction. The arms are the spec's, in the spec's order; the
            // ones Smelt does not model yet are their host classes, which is
            // what makes them erase honestly rather than disappear.
            // `ArrayBufferView` is the spec's name for "one of the byte
            // views", and Smelt models each of those concretely (its own host
            // identity, element type and marker; see
            // `smelt_stdlib::host_object`). So the reference is a UNION of
            // them, exactly as `BodyInit` below is a union of its arms — not an
            // opaque nominal class, which erases and, as an erased union
            // member, made every union containing it non-concrete. Hono's
            // `crypto.ts` hands `JSON.stringify` a
            // `string | boolean | number | JSONValue | ArrayBufferView | ArrayBuffer`,
            // and this arm is what makes that union serializable: a view
            // serializes as its element indices, byte storage as `{}`, which is
            // what JavaScript does.
            //
            // A source declaration of the name wins, the same way the ambient
            // init dictionaries defer to one.
            ("ArrayBufferView", []) if !self.source_declares_type("ArrayBufferView") => {
                let mut members = Vec::new();
                for view in smelt_stdlib::TYPED_ARRAY_CLASS_NAMES
                    .iter()
                    .copied()
                    .chain(["DataView"])
                {
                    let name = self.intern_type_name(view);
                    members.push(self.ctx.krate.types.intern(Type::Class {
                        name,
                        args: Vec::new(),
                    }));
                }
                Ok(self.ctx.krate.types.intern(Type::Union(members)))
            }
            // `FormDataEntryValue` is the spec's `File | string` alias, the
            // type of every `FormData` value. Left opaque it was an erased
            // nominal class, and injecting one into a `(string | File)[]`
            // picked the `File` arm for what is a `string` on most paths
            // (Hono's `handleParsingAllValues` pushed `'bbb'` as a `File`).
            // Same rule as `ArrayBufferView` above: a lib alias is its union,
            // and a source declaration of the name wins.
            ("FormDataEntryValue", []) if !self.source_declares_type("FormDataEntryValue") => {
                let file_name = self.intern_type_name("File");
                let file_ty = self.ctx.krate.types.intern(Type::Class {
                    name: file_name,
                    args: Vec::new(),
                });
                let string_ty = self.ctx.krate.types.intern(Type::String);
                Ok(self.ctx.krate.types.intern(Type::Union(vec![file_ty, string_ty])))
            }
            ("BodyInit", []) => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let none_ty = self.ctx.krate.types.intern(Type::None);
                let mut members = vec![string_ty];
                // The spec's `BodyInit` is
                // `ReadableStream | XMLHttpRequestBodyInit`, and the second is
                // `Blob | BufferSource | FormData | URLSearchParams | string`.
                // `BufferSource` is `ArrayBufferView | ArrayBuffer`, so the
                // eleven element views belong here too: they are arms a caller
                // really passes (`new Response(encoder.encode(text))`), and
                // leaving them out meant a `BodyInit`-annotated parameter never
                // mentioned the byte family at all.
                for arm in smelt_stdlib::TYPED_ARRAY_CLASS_NAMES {
                    let name = self.intern_type_name(arm);
                    members.push(self.ctx.krate.types.intern(Type::Class {
                        name,
                        args: Vec::new(),
                    }));
                }
                for arm in [
                    "ArrayBuffer",
                    "Blob",
                    "FormData",
                    "URLSearchParams",
                    "ReadableStream",
                ] {
                    let name = self.intern_type_name(arm);
                    members.push(self.ctx.krate.types.intern(Type::Class {
                        name,
                        args: Vec::new(),
                    }));
                }
                members.push(none_ty);
                Ok(self.ctx.krate.types.intern(Type::Union(members)))
            }
            // `RegExpMatchArray` and `RegExpExecArray` are the lib's names for
            // the value `match`/`exec` return, which Smelt models as the match
            // class. Left opaque, an annotated parameter was a nominal class the
            // match value erased into, and `m[1]` read `undefined`. A source
            // declaration of the name wins.
            (lib_name @ ("RegExpMatchArray" | "RegExpExecArray"), [])
                if !self.source_declares_type(lib_name) =>
            {
                Ok(self.match_result_type())
            }
            ("Capitalize" | "Uncapitalize" | "Uppercase" | "Lowercase", [_]) => {
                Ok(self.ctx.krate.types.intern(Type::String))
            }
            ("Array" | "ArrayLike" | "Iterable" | "ReadonlyArray", [item]) => {
                let lowered_item = self.ts_type_to_hir(item)?;
                Ok(self.ctx.krate.types.intern(Type::List(lowered_item)))
            }
            // `Partial<X>` of a concrete interface record is its OWN record
            // with every field optional. Lowering it as `X` made the fields of
            // `merge(options: Partial<Options>)` required: the call filled the
            // missing ones with defaults, and `{ ...defaults, ...options }`
            // then overwrote the real defaults with them.
            // The WebIDL byte-source aliases of the TypeScript lib, as the
            // unions they are: a `BufferSource` is a view or its storage, and
            // an `ArrayBufferView` a typed array or a `DataView`. Smelt models
            // each member concretely and reads a union of them through its
            // live arm, so the alias needs no opaque class of its own.
            ("BufferSource" | "AllowSharedBufferSource", []) => {
                Ok(self.byte_source_union_type(true))
            }
            ("ArrayBufferView", []) => Ok(self.byte_source_union_type(false)),
            ("Partial", [item]) => {
                let base = self.ts_type_to_hir(item)?;
                let span = self.span(reference.span.start, reference.span.end);
                Ok(self.partial_record_type(base, span)?.unwrap_or(base))
            }
            // `Readonly` changes no property's presence or storage.
            ("Readonly", [item]) => self.ts_type_to_hir(item),
            ("Required", [item]) => {
                let base = self.ts_type_to_hir(item)?;
                let span = self.span(reference.span.start, reference.span.end);
                Ok(self.required_record_type(base, span)?.unwrap_or(base))
            }
            ("Extract", [_base, extracted]) => self.ts_type_to_hir(extracted),
            ("Exclude", [base, excluded]) => {
                let base_ty = self.ts_type_to_hir(base)?;
                let excluded_ty = self.ts_type_to_hir(excluded)?;
                Ok(self.union_without_members(base_ty, excluded_ty))
            }
            // A `K` still free here keeps the base; the call that fixes it
            // reshapes the result (`call_result_shaped_by_checker`).
            ("Pick", [base, keys]) => {
                let lowered = self.ts_type_to_hir(base)?;
                let span = self.span(reference.span.start, reference.span.end);
                let picked = match self.literal_record_keys(keys) {
                    Some(selected) => self.picked_record_type(lowered, &selected, span)?,
                    None => None,
                };
                Ok(picked.unwrap_or(lowered))
            }
            ("Set" | "ReadonlySet", [item]) => {
                let lowered_item = self.ts_type_to_hir(item)?;
                Ok(self.ctx.krate.types.intern(Type::Set(lowered_item)))
            }
            ("Record", [key, value]) => {
                let lowered_value = self.ts_type_to_hir(value)?;
                let lowered_key = match self.record_key_type_to_hir(key, reference.span) {
                    Ok(key) => key,
                    Err(error)
                        if matches!(self.ctx.krate.types.get(lowered_value), Some(Type::Never)) =>
                    {
                        drop(error);
                        self.ctx.krate.types.intern(Type::String)
                    }
                    Err(error) => return Err(error),
                };
                Ok(self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Dict(lowered_key, lowered_value)))
            }
            ("Map" | "ReadonlyMap", [key, value]) => {
                // A `Map<K, V>` annotation lowers to the distinct `JsMap` variant
                // (not `Dict`) so the source Map spelling survives interning and
                // erasure can stamp the `__smelt_map` marker. `JsMap` shares all
                // of `Dict`'s machinery and is interchangeable with it under
                // assignability, so this does not reject `Record`-shaped flows.
                let lowered_key = self.ts_type_to_hir(key)?;
                let lowered_value = self.ts_type_to_hir(value)?;
                Ok(self
                    .ctx
                    .krate
                    .types
                    .intern(Type::JsMap(lowered_key, lowered_value)))
            }
            ("Promise", [item]) => {
                let lowered_item = self.ts_type_to_hir(item)?;
                Ok(self.ctx.krate.types.intern(Type::Future(lowered_item)))
            }
            (kind @ ("Generator" | "AsyncGenerator"), [yielded]) => {
                let yield_ty = self.ts_type_to_hir(yielded)?;
                let return_ty = self.ctx.krate.types.intern(Type::None);
                let next_ty = self.erase(ErasureReason::UnfilledGeneric);
                Ok(self.ctx.krate.types.intern(Type::Generator {
                    is_async: kind == "AsyncGenerator",
                    yield_ty,
                    return_ty,
                    next_ty,
                }))
            }
            (kind @ ("Generator" | "AsyncGenerator"), [yielded, returned]) => {
                let yield_ty = self.ts_type_to_hir(yielded)?;
                let return_ty = self.ts_type_to_hir(returned)?;
                let next_ty = self.erase(ErasureReason::UnfilledGeneric);
                Ok(self.ctx.krate.types.intern(Type::Generator {
                    is_async: kind == "AsyncGenerator",
                    yield_ty,
                    return_ty,
                    next_ty,
                }))
            }
            (kind @ ("Generator" | "AsyncGenerator"), [yielded, returned, resumed]) => {
                let yield_ty = self.ts_type_to_hir(yielded)?;
                let return_ty = self.ts_type_to_hir(returned)?;
                let next_ty = self.ts_type_to_hir(resumed)?;
                Ok(self.ctx.krate.types.intern(Type::Generator {
                    is_async: kind == "AsyncGenerator",
                    yield_ty,
                    return_ty,
                    next_ty,
                }))
            }
            // `Parameters<F>` and `ConstructorParameters<C>` ask the same
            // question of two different callable positions: the tuple of
            // parameter types a value is called with. `typeof C` for a class
            // already resolves to that class's CONSTRUCTOR function type, so
            // both spellings read `params` off a `Type::Function` and differ in
            // nothing else — which is why they share one arm rather than one
            // being a special case of the other.
            //
            // Without `ConstructorParameters` here the name fell through to the
            // ordinary type-reference path, resolved to no interface and no
            // alias, and produced a type that is neither a list, a tuple, nor
            // erased; a rest parameter annotated with it then blocked the whole
            // build with `rest parameter type must resolve to an array type`.
            // Hono's `src/client/types.ts` is exactly that shape
            // (`webSocket?: (...args: ConstructorParameters<typeof WebSocket>) => WebSocket`),
            // and it stopped the crate from being emitted at all.
            ("Parameters" | "ConstructorParameters", [function_arg]) => {
                let function_ty = self.ts_type_to_hir(function_arg)?;
                if let Some(Type::Function(function_ty_data)) = self.ctx.krate.types.get(function_ty)
                {
                    Ok(self
                        .ctx
                        .krate
                        .types
                        .intern(Type::Tuple(function_ty_data.params.clone())))
                } else {
                    // Not a callable this lowering can read a signature off — a
                    // host constructor with no modeled signature, or a type
                    // parameter. The honest answer is then "some arguments,
                    // shape unknown", which is what an erased array says.
                    let item = self.erase(ErasureReason::TypeLevel);
                    Ok(self.ctx.krate.types.intern(Type::List(item)))
                }
            }
            ("ReturnType", [function_arg]) => {
                let function_ty = self.ts_type_to_hir(function_arg)?;
                if let Some(Type::Function(function_ty_data)) = self.ctx.krate.types.get(function_ty)
                {
                    Ok(function_ty_data.return_ty)
                } else {
                    Ok(self.erase(ErasureReason::TypeLevel))
                }
            }
            _ => {
                let symbol = self.resolve_type_reference_symbol(&name_text);
                if let Some(interface) = self.find_interface(symbol).cloned()
                    && !self.alias_shadows_interface(symbol, &name_text)
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
                    // A constructor-only interface such as `interface
                    // MapCacheConstructor { new (): MapCache }` is a typed
                    // constructor slot, not a data record: it carries only a
                    // construct signature. Lower it to that constructor's
                    // `Type::Function` so a value of the interface type flows
                    // through the closure / `new value()` machinery and the
                    // construction keeps the concrete constructed type instead of
                    // erasing to `unknown` (issue #54).
                    if let Some(function_ty) =
                        self.interface_construct_slot_type(symbol, &substitutions)
                    {
                        return Ok(function_ty);
                    }
                    let instantiated_args = interface
                        .type_params
                        .iter()
                        .map(|param| {
                            substitutions.get(&param.name).copied().unwrap_or_else(|| {
                                self.ctx
                                    .krate
                                    .types
                                    .intern(Type::TypeParam { name: param.name })
                            })
                        })
                        .collect();
                    return Ok(self.ctx.krate.types.intern(Type::Class {
                        name: symbol,
                        args: instantiated_args,
                    }));
                }
                if let Some(alias) = self.visible_type_alias(symbol, &name_text).cloned() {
                    let lowered_args = args
                        .iter()
                        .map(|arg| self.ts_type_to_hir(arg))
                        .collect::<Result<Vec<_>, _>>()?;
                    let substitutions = self.type_argument_substitution(
                        &alias.type_params,
                        &lowered_args,
                        self.span(reference.span.start, reference.span.end),
                    )?;
                    let alias_is_union = matches!(
                        self.ctx.krate.types.get(alias.ty),
                        Some(Type::Union(_))
                    );
                    let substituted_alias_ty = self.substitute_type_params(alias.ty, &substitutions);
                    let reference_span = self.span(reference.span.start, reference.span.end);
                    let shaped = self.alias_reference_shaped_by_checker(
                        substituted_alias_ty,
                        reference.span.start,
                        reference.span.end,
                        reference_span,
                    )?;
                    if shaped != substituted_alias_ty {
                        return Ok(shaped);
                    }
                    let alias_has_fields = self.types.alias_fields(symbol)
                        .is_some_and(|fields| !fields.is_empty());
                    if alias_has_fields
                        && !alias_is_union
                        && self.function_member_type(substituted_alias_ty).is_none()
                    {
                        let instantiated_args = alias
                            .type_params
                            .iter()
                            .map(|param| {
                                substitutions.get(&param.name).copied().unwrap_or_else(|| {
                                    self.ctx.krate.types.intern(Type::TypeParam {
                                        name: param.name,
                                    })
                                })
                            })
                            .collect();
                        return Ok(self.ctx.krate.types.intern(Type::Class {
                            name: symbol,
                            args: instantiated_args,
                        }));
                    }
                    if !alias_is_union
                        && let Some(function_ty) = self.function_member_type(substituted_alias_ty)
                    {
                        if let Some(fields) = self.types.alias_fields(symbol).cloned()
                            && !fields.is_empty()
                        {
                            let fields = self.substituted_fields(&fields, &substitutions);
                            self.types.set_callable_fields(function_ty, fields.clone());
                            self.ctx.callable_fields.write().insert(function_ty, fields);
                        }
                        return Ok(function_ty);
                    }
                    return Ok(substituted_alias_ty);
                }
                if let Some(module) = self.unresolvable_type_import_module(&name_text) {
                    return Err(SmeltError::unsupported(
                        self.span(reference.span.start, reference.span.end),
                        format!(
                            "the type `{name_text}` is imported from `{module}`, which is one of this project's own sources but is not part of the crate, so nothing declares it: the reference has no shape (a nominal stand-in for it erases every union that contains it)"
                        ),
                    ));
                }
                let mut lowered_args = args
                    .iter()
                    .map(|arg| self.ts_type_to_hir(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                // A reference to a generic CLASS may omit trailing type
                // arguments the declaration defaults (`class Context<E extends
                // Env = any, P extends string = any, I extends Input = {}>`
                // referenced as `Context<E>` means `Context<E, any, {}>`), and a
                // declaration whose parameters are all defaulted may be written
                // with no argument list at all. Interfaces and type aliases
                // already take their defaults above, through
                // `type_argument_substitution`; classes fall through to this
                // arm, so without this the HIR carried a SHORT argument list and
                // every later stage — including Rust emission, where the arity
                // is checked — saw a reference of the wrong arity.
                //
                // See `type_arguments_with_defaults` for the rule and for why an
                // `any` default is a genuine dynamic boundary rather than an
                // erasure this lowering chose.
                // The prepass registry answers first: it covers every class the
                // crate declares, including ones not lowered yet. `find_class`
                // is the fallback for a class that reached the crate some other
                // way (a merged declaration file, a synthesized constructor
                // function) and therefore has an item but no prepass entry.
                if let Some(class_type_params) = self
                    .ctx
                    .class_type_params
                    .get(&symbol)
                    .cloned()
                    .or_else(|| self.find_class(symbol).map(|class| class.type_params.clone()))
                    && let Some(completed) =
                        self.type_arguments_with_defaults(&class_type_params, &lowered_args)
                {
                    lowered_args = completed;
                }
                Ok(self.ctx.krate.types.intern(Type::Class {
                    name: symbol,
                    args: lowered_args,
                }))
            }
        }
    }

    /// The module a type name was imported from when the crate cannot have it.
    ///
    /// `Some(specifier)` only for the one case that is a genuine gap rather
    /// than a modelling choice: the name came from a RELATIVE specifier that
    /// resolves to a file the manifest's own source roots contain (excludes
    /// already applied) and that the dependency closure did not reach, so no
    /// module in the crate declares it and no later module will. That is the
    /// shape that erased Hono's `ResponseHeadersInit` for weeks — the nominal
    /// `Type::Class` stand-in erases, an erased member makes the whole union
    /// non-concrete, and the resulting `SmeltUnknown` surfaces in the emitter
    /// far from the reference with nothing naming the alias.
    ///
    /// Everything else keeps the nominal fallback, deliberately:
    ///
    /// * **A cyclic type-only import of an in-crate declaration.** Hono's
    ///   `types.ts` names `Context` from `./context` while `context.ts` imports
    ///   `types.ts`; whichever lowers first sees the other's declarations
    ///   missing. The module IS in the crate, so this returns `None`.
    /// * **A types-only external package.** remeda imports `Simplify` from
    ///   `type-fest`, which is not a crate module and never will be. A bare
    ///   specifier returns `None`.
    /// * **An excluded module.** `[sources] exclude` promises that excluding a
    ///   module removes its implementation, not its type surface, so an
    ///   excluded file is not in the source-root set this consults and returns
    ///   `None`.
    fn unresolvable_type_import_module(&self, name_text: &str) -> Option<String> {
        if self.ctx.project_sources_outside_crate.is_empty() {
            return None;
        }
        let module = self.imports.import_source(name_text)?;
        if !module.starts_with('.') {
            return None;
        }
        self.resolved_module_export_keys(module)
            .into_iter()
            .any(|key| self.ctx.project_sources_outside_crate.contains(&key))
            .then(|| module.to_owned())
    }

    /// Lower a constructor-only interface to its typed constructor slot.
    ///
    /// A constructor-only interface (`interface MapCacheConstructor { new ():
    /// MapCache }`) carries a construct signature and no data fields or call
    /// signatures. At runtime it is an ordinary callable value that `new
    /// value()` invokes to produce the constructed type, so the honest lowering
    /// is the constructor's [`Type::Function`] — exactly the shape a `new (args)
    /// => T` constructor-type annotation lowers to (see `constructor_type_to_hir`).
    /// Type arguments already resolved for the reference are substituted into the
    /// signature so generic constructor slots keep their instantiated parameter
    /// and constructed types.
    ///
    /// Returns `None` when the interface is not a pure constructor slot (it has
    /// data fields, method fields, or call signatures), so a mixed
    /// callable-object interface still lowers as a class record and its member
    /// lookups are unaffected.
    pub(in crate::lowering) fn interface_construct_slot_type(
        &mut self,
        name: smelt_hir::Symbol,
        substitutions: &HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> Option<smelt_hir::TypeId> {
        let signatures = self.interfaces.construct_signatures(name).cloned()?;
        let signature = signatures.first()?;
        // Only a *pure* constructor slot lowers to a callable: an interface that
        // also carries data/method fields or a call signature is a mixed
        // callable-object surface, which stays a class record here.
        let has_other_members = self
            .find_interface(name)
            .is_some_and(|interface| !interface.fields.is_empty())
            || self.interfaces.call_signatures(name)
                .is_some_and(|call_sigs| !call_sigs.is_empty());
        if has_other_members {
            return None;
        }
        let params = signature
            .params
            .iter()
            .map(|param| self.substitute_type_params(*param, substitutions))
            .collect::<Vec<_>>();
        let return_ty = self.substitute_type_params(signature.return_ty, substitutions);
        let mutable_params = self.mutable_params_from_returned_tuple_state(&params, return_ty);
        Some(self.ctx.krate.types.intern(Type::Function(FunctionType {
            params,
            rest: signature.rest,
            required_params: signature.required_params,
            mutable_params,
            return_ty,
            is_async: false,
            may_throw: false,
        })))
    }

    /// Return the full source path for a qualified type name.
    pub(in crate::lowering) fn qualified_type_name_text(qualified: &oxc::ast::ast::TSQualifiedName<'_>) -> String {
        let left = match &qualified.left {
            TSTypeName::IdentifierReference(left) => left.name.to_string(),
            TSTypeName::QualifiedName(left) => Self::qualified_type_name_text(left),
            TSTypeName::ThisExpression(_) => "<unknown>".to_owned(),
        };
        format!("{left}.{}", qualified.right.name)
    }

    /// Resolve a source type reference through local import aliases to its declared symbol.
    ///
    /// A name this module DECLARES and that is ambiguous across the crate is
    /// answered first, with this module's own rendering of it. That has to come
    /// before every crate-wide lookup below: `find_type_alias` and the
    /// by-name item map are keyed by the bare spelling, whose entry for an
    /// ambiguous name is whichever module registered last, so without this a
    /// module's own `interface Context` annotation resolved to another module's
    /// `class Context` (the cross-kind half of H61, and Hono's 437 `E0609`).
    /// The rename map answers `None` for every unambiguous name, so nothing
    /// about a crate without a collision changes.
    pub(in crate::lowering) fn resolve_type_reference_symbol(&mut self, name_text: &str) -> smelt_hir::Symbol {
        // A lexically scoped declaration (a block-local class, alias or
        // interface reusing a name) is the one in sight.
        if let Some(symbol) = self.classes.scoped_type_name(name_text) {
            return symbol;
        }
        if let Some(symbol) = self.module_qualified_type_name(name_text) {
            return symbol;
        }
        if let Some(symbol) = self.imported_ambiguous_type_name(name_text) {
            return symbol;
        }
        let direct_symbol = self.intern_type_name(name_text);
        // TypeScript declaration merging permits a value namespace/class and a
        // type alias to share a spelling. In type position the alias is
        // authoritative; a barrel value export must not hide its union shape.
        if self.find_type_alias(direct_symbol).is_some() {
            return direct_symbol;
        }
        if let Some(symbol) = self.classes.scoped_type_name(name_text) {
            return symbol;
        }
        if let Some(item) = self.items.get(name_text).copied() {
            match self.item_ref(item) {
                Item::TypeAlias(alias) => return alias.name,
                Item::Interface(interface) => return interface.name,
                Item::Class(class) => return class.name,
                _ => {}
            }
        }
        direct_symbol
    }

    /// The EXPORTING module's rendering of an ambiguous type name reached
    /// through an import.
    ///
    /// The by-name lookups in [`Self::resolve_type_reference_symbol`] can only
    /// answer once the imported module's item exists, and in an import CYCLE it
    /// does not: Hono's `types.ts` and `context.ts` import each other, so when
    /// `types.ts` lowers first its `Context<E>` found no item and fell through
    /// to the bare spelling — which belongs to `reg-exp-router/node.ts`'s
    /// `interface Context { varIndex: number }`. The alias `NotFoundHandler<E>`
    /// baked that in, and every field typed through it read the interface's
    /// struct instead of the class: 437 `E0609` and 222 `E0107` in the whole
    /// crate, all of them downstream of one wrongly resolved name.
    ///
    /// The rename map does not have that ordering problem, because it is
    /// computed from a scan of every source before any module lowers. So the
    /// question "which module was this name imported from, and what is THAT
    /// module's rendering of it" is answerable at the moment the reference is
    /// lowered, item or no item. The lookup is by the name the module EXPORTED
    /// (`import { Node as TrieNode }` asks about `Node`), and it answers `None`
    /// for a name that is not ambiguous or whose module does not declare it —
    /// so a barrel re-export, a host module and every crate without a collision
    /// keep resolving exactly as before.
    fn imported_ambiguous_type_name(&mut self, name_text: &str) -> Option<smelt_hir::Symbol> {
        let specifier = self.imports.import_source(name_text)?.to_owned();
        let exported = self
            .imports
            .imported_name(name_text)
            .unwrap_or(name_text)
            .to_owned();
        let rendered = self.module_type_rename(&specifier, &exported)?;
        let symbol = self.ctx.krate.symbols.intern(&rendered);
        self.ctx.krate.names.record(symbol, &exported);
        Some(symbol)
    }

    /// The rendering module `specifier` gives to the ambiguous type `name`.
    ///
    /// Two passes, because the rename map is keyed by the manifest's own
    /// spelling of each source path while the candidate list is built from this
    /// module's path plus the specifier: the first pass compares those spellings
    /// directly, and the second falls back to comparing canonicalized paths so a
    /// `./`-relative or symlinked spelling still matches. The map holds one
    /// entry per (module, ambiguous name) pair and is empty for a crate with no
    /// collision, so the second pass is a scan of nothing in the common case.
    fn module_type_rename(&self, specifier: &str, name: &str) -> Option<String> {
        let candidates = self.resolved_module_export_keys(specifier);
        for candidate in &candidates {
            if let Some(rendered) = self
                .ctx
                .type_renames
                .get(candidate)
                .and_then(|renames| renames.get(name))
            {
                return Some(rendered.clone());
            }
        }
        let canonical_candidates = candidates
            .iter()
            .filter_map(|candidate| Self::canonical_module_path(candidate))
            .collect::<Vec<_>>();
        self.ctx
            .type_renames
            .iter()
            .filter_map(|(path, renames)| {
                let rendered = renames.get(name)?;
                let canonical = Self::canonical_module_path(path)?;
                canonical_candidates.contains(&canonical).then_some((path, rendered))
            })
            .min_by_key(|(path, _)| *path)
            .map(|(_, rendered)| rendered.clone())
    }

    /// Whether the module declares a type of this name.
    ///
    /// A declaration can appear after the reference that needs to know about it,
    /// so the interface registry is not yet populated when the question is asked.
    pub(in crate::lowering) fn source_declares_type(&self, name: &str) -> bool {
        self.source_facts.declares_type(name)
    }
}
