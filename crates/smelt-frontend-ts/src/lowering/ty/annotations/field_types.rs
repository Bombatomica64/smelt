//! Field types of user classes and interfaces: declared, inherited, getter,
//! optional and dynamic fields, and the bounds of type parameters a field read
//! goes through.

use crate::lowering::erasure::NotNewErasure;
use std::collections::{HashMap, HashSet};
use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::support::source_spelling;
use smelt_hir::{Class, ErasureReason, FunctionType, Item, Type};

impl ModuleBuilder<'_> {
    /// The type an instance read of a source getter `get name(): T` produces.
    ///
    /// A getter is not a stored field, but a member read through it is a value
    /// of the getter's declared return type — exactly what `tsc` gives
    /// `ctx.req` for `get req(): HonoRequest`. The class records each plain
    /// source accessor as a [`smelt_hir::Descriptor`] (its read type is the
    /// getter's return annotation), so the member-type resolver reads it from
    /// there instead of falling through to the `Unknown` boundary. Erasing it
    /// forced every chained access (`c.req.header(..)`) into dynamic
    /// `SmeltUnknown` dispatch. Static and setter-only
    /// descriptors are not instance reads, so they do not answer here.
    fn class_getter_read_type(
        class: &Class,
        field: smelt_hir::Symbol,
    ) -> Option<smelt_hir::TypeId> {
        class
            .descriptors
            .iter()
            .find(|descriptor| {
                descriptor.name == field && descriptor.getter.is_some() && !descriptor.is_static
            })
            .map(|descriptor| descriptor.read_ty)
    }

    /// Resolve the type of a class field.
    pub(in crate::lowering) fn class_field_type(
        &mut self,
        receiver_ty: smelt_hir::TypeId,
        field: smelt_hir::Symbol,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        let Some(receiver_type) = self.ctx.krate.types.get(receiver_ty).cloned() else {
            return Err(SmeltError::unsupported(
                self.span(0, 0),
                format!(
                    "field access is only lowered for Record<string, T>, class, and interface values for now (unknown receiver type id {receiver_ty:?})"
                ),
            ));
        };
        match receiver_type {
            Type::Dict(_, value) => Ok(value),
            Type::Optional(inner) => {
                let inner_field = self.class_field_type(inner, field)?;
                Ok(self.optional_chain_result_type(inner_field))
            }
            // not a new erasure: the receiver is already erased
            Type::Unknown => Ok(self.unknown_type(NotNewErasure::ErasedUpstream)),
            // TypeScript accepts no member read off `never`, so a `never`
            // receiver here is a contextual type inference could not resolve,
            // and its members have no static layout.
            Type::Never => Ok(self.erase(ErasureReason::TypeLevel)),
            // A member of a class- or interface-bounded parameter has the type
            // its bound declares (see `type_param_member_bound`); any other
            // parameter's members have no static layout.
            Type::TypeParam { .. } => match self.type_param_member_bound(receiver_ty) {
                Some(bound) => self.class_field_type(bound, field),
                None => Ok(self.erase(ErasureReason::ByName)),
            },
            Type::Tuple(_) => Ok(self.erase(ErasureReason::ByName)),
            Type::Bool => {
                Ok(self.erase(ErasureReason::ByName))
            }
            Type::Float | Type::Int
                if self.ctx.krate.symbols.get(field) == Some("constructor") =>
            {
                Ok(self.erase(ErasureReason::ByName))
            }
            Type::String if self.ctx.krate.symbols.get(field) == Some("length") => {
                Ok(self.ctx.krate.types.intern(Type::Float))
            }
            Type::String if self.ctx.krate.symbols.get(field) == Some("message") => {
                Ok(self.ctx.krate.types.intern(Type::String))
            }
            Type::String if self.ctx.krate.symbols.get(field) == Some("name") => {
                Ok(self.ctx.krate.types.intern(Type::String))
            }
            Type::String if self.ctx.krate.symbols.get(field) == Some("prototype") => {
                Ok(self.erase(ErasureReason::ByName))
            }
            Type::String if self.allows_unknown_index_access() => {
                Ok(self.erase(ErasureReason::ByName))
            }
            // Any remaining property read on a string value is a bound stdlib
            // method reference taken as a value — `''.slice`, `str.split`, etc.
            // (the concrete data properties `length`/`message`/`name`/`prototype`
            // are handled above). A string in valid TypeScript exposes only
            // `length` plus methods, so a property that is not one of the modeled
            // data fields is a callable member accessed without calling it.
            //
            // A method *call* (`str.slice(...)`) never reaches here: the dedicated
            // string call handlers in `dispatch_builtin_call` intercept those
            // before member-type resolution runs. Only a genuine method *value*
            // reaches this arm, and, like every other bound-method value on a
            // builtin/erased receiver, it resolves to the erased dynamic boundary
            // (`Unknown`) rather than hard-erroring. Codegen already tolerates the
            // read: `string_field_expr` emits an erased value for an unmodeled
            // string field. This keeps field resolution general for strings
            // instead of rejecting source that TypeScript accepts.
            Type::String => Ok(self.erase(ErasureReason::UntypedBuiltin)),
            Type::Function(_) => {
                if let Some(field) = self.types.callable_fields(receiver_ty)
                    .and_then(|fields| fields.iter().find(|item| item.name == field))
                {
                    return Ok(if field.optional {
                        self.ctx.krate.types.intern(Type::Optional(field.ty))
                    } else {
                        field.ty
                    });
                }
                Ok(self.erase(ErasureReason::ByName))
            }
            Type::Union(items) => {
                let matches = items
                    .iter()
                    .filter_map(|item| self.class_field_type(*item, field).ok())
                    .collect::<Vec<_>>();
                match matches.as_slice() {
                    [single] => Ok(*single),
                    [] => Ok(self.erase(ErasureReason::ByName)),
                    [first, rest @ ..] if rest.iter().all(|item| item == first) => Ok(*first),
                    _ => {
                        let mut union_items = Vec::new();
                        for item in matches {
                            if !union_items.contains(&item) {
                                union_items.push(item);
                            }
                        }
                        Ok(self.ctx.krate.types.intern(Type::Union(union_items)))
                    }
                }
            }
            Type::Class { name, args } => {
                // `constructor` and `prototype` are universal members of any
                // class/constructor value. `constructor` yields the class
                // object and `prototype` yields the prototype object; both are
                // dynamic property bags (es-toolkit mutates them through
                // `Foo.prototype.a = 1`), so they lower to the `Unknown`
                // dynamic boundary rather than a concrete shape.
                if matches!(
                    self.ctx.krate.symbols.get(field),
                    Some("constructor" | "prototype")
                ) {
                    return Ok(self.erase(ErasureReason::ByName));
                }
                // `RegExp` is a builtin stdlib class backed by the generated
                // `SmeltRegExp` runtime type, whose data properties have
                // statically known concrete field types. Resolve them here so
                // reads such as `re.source`, `re.flags`, and `re.lastIndex`
                // type as the concrete Rust fields codegen actually emits
                // (`regexp_field_expr`) instead of falling through to the
                // erased `Unknown` boundary. Erasing them would make arg
                // coercion into `RegExp::new`'s `String` params emit a
                // stringify-of-`SmeltUnknown` match against an already-`String`
                // scrutinee, and would drop the erasure adapter when the read
                // flows into a `SmeltUnknown` slot.
                if let Some(field_ty) = self.regexp_field_type_for(name, field) {
                    return Ok(field_ty);
                }
                if let Some(field_ty) = self.host_data_property_type(receiver_ty, field) {
                    return Ok(field_ty);
                }
                let class_item = self.class_by_symbol(name).cloned();
                // The receiver's arguments instantiate the class's own
                // parameters. Its item may not exist yet (an import cycle
                // through a barrel lowers a consumer first), and then the
                // declarations recorded crate-wide by the class prepass
                // (`collect_class_type_parameter_defaults`) name them: each
                // parameter is its own type, so a member spelled in `Ok<T>`'s
                // scope must be instantiated, never read as the caller's `T`.
                let class_substitutions = match &class_item {
                    Some(class) => {
                        self.type_argument_substitution(&class.type_params, &args, self.span(0, 0))?
                    }
                    None => match self.ctx.class_type_params.get(&name).cloned() {
                        Some(type_params) if !args.is_empty() => self
                            .type_argument_substitution(&type_params, &args, self.span(0, 0))?,
                        _ => HashMap::new(),
                    },
                };
                if class_item.is_none()
                    && let Some(ty) = self.in_progress_class_method_member_type(name, field)
                {
                    return Ok(self.substitute_type_params(ty, &class_substitutions));
                }
                let class_field = if let Some(class) = &class_item {
                    let substitutions =
                        self.type_argument_substitution(&class.type_params, &args, self.span(0, 0))?;
                    class
                        .fields
                        .iter()
                        .find(|item| item.name == field)
                        .map(|item| item.ty)
                        .or_else(|| Self::class_getter_read_type(class, field))
                        .map(|ty| self.substitute_type_params(ty, &substitutions))
                } else {
                    None
                };
                let class_name = source_spelling(&self.ctx.krate, name)
                    .map(str::to_owned);
                let sidecar_field = class_name
                    .as_deref()
                    .and_then(|class_name| {
                        self.classes.fields(class_name).and_then(|fields| {
                            fields
                                .iter()
                                .find(|item| item.name == field)
                                .map(|item| item.ty)
                        })
                    })
                    .map(|ty| self.substitute_type_params(ty, &class_substitutions));
                // A method's signature is spelled in its class's scope, so it is
                // instantiated at the receiver's type arguments like a field is:
                // `map(fn: (value: T) => string)` read off `Base<number>` takes
                // a `(value: number) => string` callback. Reached through the
                // base walk below, `args` are already the base's arguments in
                // the receiver's scope.
                let method_substitutions = class_substitutions.clone();
                let sidecar_method = class_name.as_deref().and_then(|class_name| {
                    let method = self.classes.methods(class_name)?
                        .iter()
                        .find(|item| item.name == field)?
                        .clone();
                    let params = method
                        .params
                        .iter()
                        .map(|param| self.substitute_type_params(param.ty, &method_substitutions))
                        .collect::<Vec<_>>();
                    let return_ty = self.substitute_type_params(method.return_ty, &method_substitutions);
                    let mutable_params =
                        self.mutable_params_from_returned_tuple_state(&params, return_ty);
                    Some(self.ctx.krate.types.intern(Type::Function(FunctionType {
                        params,
                        rest: method.rest,
                        required_params: None,
                        mutable_params,
                        return_ty,
                        is_async: method.is_async,
                        may_throw: false,
                    })))
                });
                let interface = self.find_interface(name).cloned();
                let interface_field = if let Some(interface) = interface {
                    let substitutions = self.type_argument_substitution(
                        &interface.type_params,
                        &args,
                        self.span(0, 0),
                    )?;
                    let field_data = interface
                        .fields
                        .iter()
                        .find(|item| item.name == field)
                        .map(|item| (item.ty, item.optional));
                    field_data.map(|(ty, optional)| {
                        self.instantiate_field_type(ty, optional, &substitutions)
                    }).or_else(|| self.interfaces.index_value(name))
                } else {
                    None
                };
                let interface_method = self.find_interface(name).cloned().and_then(|iface| {
                    let substitutions = self
                        .type_argument_substitution(&iface.type_params, &args, self.span(0, 0))
                        .ok()?;
                    let method = iface.methods.iter().find(|item| item.name == field)?;
                    let params = method
                        .params
                        .iter()
                        .map(|param| self.substitute_type_params(param.ty, &substitutions))
                        .collect::<Vec<_>>();
                    let return_ty = self.substitute_type_params(method.return_ty, &substitutions);
                    let mutable_params =
                        self.mutable_params_from_returned_tuple_state(&params, return_ty);
                    Some(self.ctx.krate.types.intern(Type::Function(FunctionType {
                        params,
                        rest: None,
                        required_params: None,
                        mutable_params,
                        return_ty,
                        is_async: method.is_async,
                        may_throw: false,
                    })))
                });
                let alias_fields = self.types.alias_fields(name)
                    .cloned()
                    .unwrap_or_default();
                let spelling = source_spelling(&self.ctx.krate, name).unwrap_or_default().to_owned();
                let alias_field = if let Some(alias) = self.visible_type_alias(name, &spelling).cloned() {
                    let substitutions = self.type_argument_substitution(
                        &alias.type_params,
                        &args,
                        self.span(0, 0),
                    )?;
                    let field_data = alias_fields
                        .iter()
                        .find(|item| item.name == field)
                        .map(|item| (item.ty, item.optional));
                    field_data.map(|(ty, optional)| {
                        self.instantiate_field_type(ty, optional, &substitutions)
                    })
                } else {
                    // A class's predeclared member surface, spelled in its
                    // own scope like the sidecar members above.
                    let field_data = alias_fields
                        .iter()
                        .find(|item| item.name == field)
                        .map(|item| (item.ty, item.optional));
                    field_data.map(|(ty, optional)| {
                        self.instantiate_field_type(ty, optional, &class_substitutions)
                    })
                };
                // A class string/number index signature contributes a keyed
                // store whose value type is statically known. When no declared
                // named field or method matches, resolve the access through the
                // index signature's value type instead of erasing it. Named
                // members are tried first (above) so declared fields keep their
                // concrete types; this is only the dynamic-keyed fallback.
                let class_index_field = self.classes.index_value(name);
                if let Some(ty) = class_field
                    .or(sidecar_field)
                    .or(sidecar_method)
                    .or(interface_field)
                    .or(interface_method)
                    .or(alias_field)
                    .or(class_index_field)
                {
                    return Ok(ty);
                }
                if let Some(ty) = self.builtin_class_field_type(name, field) {
                    return Ok(ty);
                }
                if let Some((base, base_args)) =
                    class_item.and_then(|class| class.base.map(|base| (base, class.base_args)))
                    // Symbol-keyed, not name-keyed: the source spelling of a
                    // class renamed for a cross-module collision is shared with
                    // the class that displaced it, so a by-name fallback made an
                    // imported base resolve back to the importing module's own
                    // subclass and the chain walk below never terminated.
                    .or_else(|| self.classes.base_of_symbol(name).cloned())
                {
                    let substitutions = self.type_argument_substitution(
                        &self
                            .class_by_symbol(name)
                            .map(|class| class.type_params.clone())
                            .unwrap_or_default(),
                        &args,
                        self.span(0, 0),
                    )?;
                    let base_args = base_args
                        .into_iter()
                        .map(|arg| self.substitute_type_params(arg, &substitutions))
                        .collect();
                    let base_ty = self.ctx.krate.types.intern(Type::Class {
                        name: base,
                        args: base_args,
                    });
                    if let Ok(ty) = self.class_field_type(base_ty, field) {
                        return Ok(ty);
                    }
                }
                let mut visited = HashSet::new();
                if let Some(ty) =
                    self.inherited_interface_field_type(name, &args, field, &mut visited)?
                {
                    return Ok(ty);
                }
                // Static resolution is exhausted: the field is not a declared
                // named field/method, an index-signature value, a builtin
                // member, or reachable through the base class or interface
                // heritage chains. In valid TypeScript, dot access still type-
                // checks in exactly one situation — the receiver's static shape
                // is *widened* or *dynamic*: an index signature on a base type,
                // an `any`-typed heritage clause, a declaration-merged member,
                // or (during isolated per-file lowering, where cross-module
                // classes expose no fields here) a class whose full member set
                // is simply not visible yet. Routing these to the `Unknown`
                // dynamic boundary is the honest lowering; hard-erroring would
                // reject source TypeScript accepts. This is not SmeltUnknown
                // *widening* of statically-typed access — every static resolver
                // above is tried first, so declared members keep their concrete
                // types and only genuinely-unresolvable dynamic access lands
                // here.
                Ok(self.erase(ErasureReason::ByName))
            }
            _ => Err(SmeltError::unsupported(
                self.span(0, 0),
                format!(
                    "field access is only lowered for Record<string, T>, class, and interface values for now (receiver: {receiver_type:?}, field: {})",
                    self.ctx.krate.symbols.get(field).unwrap_or("<unknown>")
                ),
            )),
        }
    }

    /// Resolve an in-progress class method reference as a function-valued member type.
    pub(in crate::lowering) fn in_progress_class_method_member_type(
        &mut self,
        class_name: smelt_hir::Symbol,
        method_name: smelt_hir::Symbol,
    ) -> Option<smelt_hir::TypeId> {
        let class_text = source_spelling(&self.ctx.krate, class_name)
            .map(str::to_owned)?;
        let methods = self.classes.methods(&class_text).cloned()?;
        let method = methods.into_iter().find(|item| item.name == method_name)?;
        let params = method.params.iter().map(|param| param.ty).collect::<Vec<_>>();
        let mutable_params =
            self.mutable_params_from_returned_tuple_state(&params, method.return_ty);
        Some(self.ctx.krate.types.intern(Type::Function(FunctionType {
            params,
            rest: method.rest,
            required_params: None,
            mutable_params,
            return_ty: method.return_ty,
            is_async: method.is_async,
            may_throw: false,
        })))
    }

    /// Resolve a field through stored interface heritage edges.
    ///
    /// Cyclic type-only imports can lower an interface before its parents are
    /// available. Keeping heritage edges lets member access retry inheritance
    /// against the latest declarations after the cycle has been loaded.
    pub(in crate::lowering) fn inherited_interface_field_type(
        &mut self,
        name: smelt_hir::Symbol,
        args: &[smelt_hir::TypeId],
        field: smelt_hir::Symbol,
        visited: &mut HashSet<smelt_hir::Symbol>,
    ) -> Result<Option<smelt_hir::TypeId>, SmeltError> {
        if !visited.insert(name) {
            return Ok(None);
        }
        let Some(parents) = self.interfaces.extends(name).cloned() else {
            return Ok(None);
        };
        let child_params = self
            .find_interface(name)
            .map(|interface| interface.type_params.clone())
            .unwrap_or_default();
        let child_substitutions =
            self.type_argument_substitution(&child_params, args, self.span(0, 0))?;
        for parent in parents {
            if self
                .ctx
                .krate
                .symbols
                .get(parent.parent)
                .is_some_and(|parent_name| parent_name.contains('.'))
            {
                return Ok(Some(self.erase(ErasureReason::UnmodeledType)));
            }
            let parent_args = parent
                .args
                .into_iter()
                .map(|arg| self.substitute_type_params(arg, &child_substitutions))
                .collect::<Vec<_>>();
            if let Some(ty) = self.interface_declared_field_type(parent.parent, &parent_args, field)?
            {
                return Ok(Some(ty));
            }
            if let Some(ty) =
                self.inherited_interface_field_type(parent.parent, &parent_args, field, visited)?
            {
                return Ok(Some(ty));
            }
        }
        Ok(None)
    }

    /// Resolve a directly declared interface field with generic arguments applied.
    pub(in crate::lowering) fn interface_declared_field_type(
        &mut self,
        name: smelt_hir::Symbol,
        args: &[smelt_hir::TypeId],
        field: smelt_hir::Symbol,
    ) -> Result<Option<smelt_hir::TypeId>, SmeltError> {
        let Some(interface) = self.find_interface(name).cloned() else {
            return Ok(None);
        };
        let substitutions =
            self.type_argument_substitution(&interface.type_params, args, self.span(0, 0))?;
        let field_data = interface
            .fields
            .iter()
            .find(|item| item.name == field)
            .map(|item| (item.ty, item.optional));
        Ok(field_data
            .map(|(ty, optional)| self.instantiate_field_type(ty, optional, &substitutions))
            .or_else(|| self.interfaces.index_value(name)))
    }

    /// Apply generic substitutions and optional wrapping for a structural field.
    pub(in crate::lowering) fn instantiate_field_type(
        &mut self,
        ty: smelt_hir::TypeId,
        optional: bool,
        substitutions: &HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> smelt_hir::TypeId {
        let ty = self.substitute_type_params(ty, substitutions);
        self.field_type_with_optional(ty, optional)
    }

    /// Wrap optional fields without producing nested `Optional` types.
    pub(in crate::lowering) fn field_type_with_optional(
        &mut self,
        ty: smelt_hir::TypeId,
        optional: bool,
    ) -> smelt_hir::TypeId {
        if optional && !matches!(self.ctx.krate.types.get(ty), Some(Type::Optional(_))) {
            self.ctx.krate.types.intern(Type::Optional(ty))
        } else {
            ty
        }
    }

    /// Resolve the value type for dynamic object field extraction.
    pub(in crate::lowering) fn dynamic_field_type(&mut self, receiver_ty: smelt_hir::TypeId) -> smelt_hir::TypeId {
        let receiver_ty = self.type_param_constraint_or_self(receiver_ty);
        match self.ctx.krate.types.get(receiver_ty).cloned() {
            Some(Type::Dict(_, value)) => value,
            Some(Type::Unknown) if self.allows_unknown_index_access() => {
                self.unknown_type(NotNewErasure::ErasedUpstream) // not a new erasure: receiver already erased
            }
            Some(Type::Class { name, .. }) => {
                let fields = self.types.alias_fields(name)
                    .cloned()
                    .unwrap_or_default();
                let mut value_tys = Vec::new();
                for field in fields {
                    let ty = if field.optional {
                        self.ctx.krate.types.intern(Type::Optional(field.ty))
                    } else {
                        field.ty
                    };
                    if !value_tys.contains(&ty) {
                        value_tys.push(ty);
                    }
                }
                match value_tys.as_slice() {
                    [single] => *single,
                    [] => self.erase(ErasureReason::DynamicOperator),
                    _ => self.ctx.krate.types.intern(Type::Union(value_tys)),
                }
            }
            Some(Type::Union(items)) => {
                let mut value_tys = Vec::new();
                for item in items {
                    let ty = self.dynamic_field_type(item);
                    if !value_tys.contains(&ty) {
                        value_tys.push(ty);
                    }
                }
                match value_tys.as_slice() {
                    [single] => *single,
                    [] => self.unknown_type(NotNewErasure::Unreachable), // not a new erasure: unreachable, only an empty union yields no arm
                    _ => self.ctx.krate.types.intern(Type::Union(value_tys)),
                }
            }
            // not a new erasure: the receiver is already erased
            Some(Type::Unknown) => self.unknown_type(NotNewErasure::ErasedUpstream),
            _ => self.erase(ErasureReason::DynamicOperator),
        }
    }

    /// Return the inner type for an optional receiver, or the original type otherwise.
    pub(in crate::lowering) fn optional_receiver_inner_type(&self, ty: smelt_hir::TypeId) -> smelt_hir::TypeId {
        if let Some(Type::Optional(inner)) = self.ctx.krate.types.get(ty) {
            *inner
        } else {
            ty
        }
    }

    /// Wrap a result type in `Optional`, avoiding nested optionals from optional fields.
    pub(in crate::lowering) fn optional_chain_result_type(&mut self, ty: smelt_hir::TypeId) -> smelt_hir::TypeId {
        smelt_hir::type_normalize::optional_of(&mut self.ctx.krate.types, ty)
    }

    /// Look up a class by its symbol.
    pub(in crate::lowering) fn class_by_symbol(&self, name: smelt_hir::Symbol) -> Option<&Class> {
        self.ctx.krate.items.iter().find_map(|item| {
            if let Item::Class(class) = item {
                if class.name == name {
                    return Some(class);
                }
            }
            None
        })
    }

    /// The user class or interface that bounds a type parameter, when members
    /// read through `ty` should be typed from it.
    ///
    /// `ty` qualifies when it is a [`Type::TypeParam`] whose active constraint
    /// (`<A extends Animal>`, `<L extends HasLen>`) is a NON-GENERIC class or
    /// interface declared in the program. A member read on such a receiver
    /// (`a.legs`, `a.speak()`) has exactly the type the bound declares for it,
    /// so it is typed through the bound instead of erasing; the receiver keeps its
    /// type `A`, and MIR dispatches the member by the receiver's concrete type
    /// (`Rvalue::BoundMethod`).
    ///
    /// Every other constraint answers `None` and keeps the erased member read:
    /// a builtin bound (`extends Date`, which names no class item), a generic
    /// bound (`extends Box<T>`), a structural or function constraint, or an
    /// unconstrained parameter.
    pub(in crate::lowering) fn type_param_member_bound(
        &self,
        ty: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        let Some(Type::TypeParam { name }) = self.ctx.krate.types.get(ty) else {
            return None;
        };
        let constraint = self.types.param_constraint(*name)?;
        let Some(Type::Class { name: bound_name, args }) = self.ctx.krate.types.get(constraint) else {
            return None;
        };
        if !args.is_empty() {
            return None;
        }
        let bound = *bound_name;
        let declared_class = self.class_by_symbol(bound).is_some_and(|class| class.type_params.is_empty())
            || source_spelling(&self.ctx.krate, bound)
                .is_some_and(|spelling| self.classes.item(spelling).is_some() && self.class_by_symbol(bound).is_none());
        let declared_interface = self
            .find_interface(bound)
            .is_some_and(|interface| interface.type_params.is_empty());
        (declared_class || declared_interface).then_some(constraint)
    }

    /// Return a type parameter's active constraint when expression lowering needs its shape.
    pub(in crate::lowering) fn type_param_constraint_or_self(
        &self,
        ty: smelt_hir::TypeId,
    ) -> smelt_hir::TypeId {
        match self.ctx.krate.types.get(ty) {
            Some(Type::TypeParam { name }) => self.types.param_constraint(*name).unwrap_or(ty),
            _ => ty,
        }
    }

    /// Return whether `ty` is a type parameter that is NOT in scope here.
    ///
    /// A type parameter is only meaningful inside the generic declaration that
    /// introduced it. One that reaches a *caller* — the usual way being a
    /// contextual type hint read straight off a callee's parameter annotation,
    /// before the callee's generics are instantiated — names nothing here, so
    /// any type built out of it is unrelatable to the values it describes.
    /// Callers use this to reject such a type rather than propagate it.
    pub(in crate::lowering) fn type_param_is_out_of_scope(&self, ty: smelt_hir::TypeId) -> bool {
        let Some(Type::TypeParam { name }) = self.ctx.krate.types.get(ty) else {
            return false;
        };
        let Some(spelling) = source_spelling(&self.ctx.krate, *name)
        else {
            return true;
        };
        // In scope means THIS parameter is what its spelling resolves to here:
        // a same-named parameter of another declaration (`sumBy`'s `T` seen
        // inside `meanBy<T>`) is a different type and is out of scope.
        self.types.param_type(spelling) != Some(ty)
    }
}
