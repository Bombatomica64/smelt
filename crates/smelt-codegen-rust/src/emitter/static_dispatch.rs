//! Emission of a static method's receiver class (`crate::static_receiver`):
//! the receiver generic on the method, the statics trait and its impls, and
//! each `Rvalue::StaticDispatch`.

use smelt_hir::StaticMember;

use super::call::RenderedCall;
use super::callback_signature::ParameterPassing;
use super::*;
use crate::rust::RustExpr;
use crate::static_receiver::{RECEIVER_GENERIC, RECEIVER_PARAM, receiver_argument};

/// The error type every statics-trait member answers in, as a generated
/// throwing function does.
const MEMBER_ERROR: &str = "Box<dyn std::error::Error>";

/// One statics-trait member's signature, in the declaring class's scope.
struct MemberSignature {
    /// The associated function's name.
    name: String,
    /// Its own generics list (`<F0: Fn(..)>`), or `""`.
    generics: String,
    /// Each parameter's name and Rust type.
    params: Vec<(String, String)>,
    /// The parameters' MIR types, which a dispatch renders its arguments at.
    param_tys: Vec<TypeId>,
    /// How each parameter is passed, which decides how its argument is borrowed.
    param_passing: Vec<ParameterPassing>,
    /// The value type it answers (inside the `Result`).
    ret_ty: TypeId,
    /// That type's Rust text.
    ret_text: String,
    /// Whether it answers in a `Result` (a constructor or static method can
    /// throw; a static field and the class value cannot).
    fallible: bool,
}

/// Where a static field of a class is read and written.
struct StaticFieldAccess {
    /// The read of its current value.
    read: RustExpr,
    /// The store of a new value, when the class can write it: its own mutable
    /// static, or an inherited one through its setter.
    store: Option<StaticStoreTarget>,
    /// The field's type.
    ty: TypeId,
}

/// How a write of a static field lands.
enum StaticStoreTarget {
    /// The class's own mutable static's global.
    Global(u32),
    /// An inherited static's setter (it makes the value the class's own).
    Inherited(smelt_mir::MirStaticField),
}

impl FunctionEmitter<'_> {
    /// The receiver generic and parameter of this function when it is a
    /// static method that uses `this` (`crate::static_receiver`): `SmeltThis:
    /// BaseSmeltStatics` and `_smelt_this: PhantomData<SmeltThis>`.
    pub(super) fn receiver_generic_parts(&self) -> Result<Option<(String, String)>, EmitError> {
        let Some(class) = self
            .context
            .static_receivers()
            .receiver_class(self.function.id)
        else {
            return Ok(None);
        };
        let trait_name = crate::static_receiver::statics_trait_name(&self.type_rust_name(class)?);
        Ok(Some((
            format!("{RECEIVER_GENERIC}: {trait_name}"),
            format!("{RECEIVER_PARAM}: ::std::marker::PhantomData<{RECEIVER_GENERIC}>"),
        )))
    }

    /// The static method implementing `method` for `class`: its own, else
    /// the nearest ancestor's.
    fn implementing_static_method(&self, class: Symbol, method: Symbol) -> Option<&MirFunction> {
        let method_name = self.symbol_source_name(method).ok()?;
        let mut current = Some(class);
        let mut seen = HashSet::new();
        while let Some(name) = current
            && seen.insert(name)
        {
            let found = crate::classes::find_class(self.mir, name)?;
            for id in &found.static_methods {
                let function = self.function_by_id(*id)?;
                if let HirOrigin::ClassStaticMethod { method: own, .. } = function.origin
                    && self.symbol_source_name(own).ok() == Some(method_name)
                {
                    return Some(function);
                }
            }
            current = found.base;
        }
        None
    }

    /// Where the static field named `field` of `class` lives: an inherited
    /// static's accessor pair on `class`, else the nearest declaration (whose
    /// storage `class` writes only when it is `class`'s own).
    fn static_field_access(
        &self,
        class: Symbol,
        field: Symbol,
    ) -> Result<Option<StaticFieldAccess>, EmitError> {
        let key = self.symbol_source_name(field)?.to_owned();
        let mut current = Some(class);
        let mut own = true;
        let mut seen = HashSet::new();
        while let Some(name) = current
            && seen.insert(name)
        {
            let Some(found) = crate::classes::find_class(self.mir, name) else {
                break;
            };
            for declared in &found.static_fields {
                if self.symbol_source_name(declared.name)? != key {
                    continue;
                }
                if declared.inherited.is_some() {
                    if !own {
                        continue;
                    }
                    let read = crate::classes::inherited_static_read_text(
                        self.mir,
                        self.context,
                        declared,
                    )
                    .ok_or_else(|| EmitError::new("inherited static has no getter"))?;
                    return Ok(Some(StaticFieldAccess {
                        read: RustExpr::chain(read),
                        store: Some(StaticStoreTarget::Inherited(declared.clone())),
                        ty: declared.ty,
                    }));
                }
                let read = match declared.global {
                    Some(global) => self.global_get_expr(global)?,
                    None if declared.value.is_some() => crate::classes::materialized_static_value(
                        declared.value.as_ref(),
                        self.mir.types.get(declared.ty) != Some(&Type::PyStr),
                    ),
                    None => continue,
                };
                let store = (own && declared.mutable)
                    .then_some(declared.global)
                    .flatten()
                    .map(StaticStoreTarget::Global);
                return Ok(Some(StaticFieldAccess {
                    read,
                    store,
                    ty: declared.ty,
                }));
            }
            own = false;
            current = found.base;
        }
        Ok(None)
    }

    /// The signature of `member` of `class`'s statics trait.
    fn member_signature(
        &self,
        class: Symbol,
        member: StaticMember,
    ) -> Result<MemberSignature, EmitError> {
        let field_text = |field: Symbol| self.context.names().field_text(self.mir, field);
        match member {
            StaticMember::Get(field) | StaticMember::Set(field) => {
                let access = self.static_field_access(class, field)?.ok_or_else(|| {
                    EmitError::new("a static reached through `this` is not declared")
                })?;
                let ty_text = self.rendered_type(access.ty)?.as_str().to_owned();
                let (name, params, param_tys) = if matches!(member, StaticMember::Get(_)) {
                    (
                        format!("__smelt_static_get_{}", field_text(field)),
                        Vec::new(),
                        Vec::new(),
                    )
                } else {
                    (
                        format!("__smelt_static_set_{}", field_text(field)),
                        vec![("smelt_value".to_owned(), ty_text.clone())],
                        vec![access.ty],
                    )
                };
                Ok(MemberSignature {
                    name,
                    generics: String::new(),
                    params,
                    param_passing: vec![ParameterPassing::Owned; param_tys.len()],
                    param_tys,
                    ret_ty: access.ty,
                    ret_text: ty_text,
                    fallible: false,
                })
            }
            StaticMember::Construct => {
                let constructor = crate::classes::find_class(self.mir, class)
                    .and_then(|found| found.constructor)
                    .and_then(|id| self.function_by_id(id))
                    .ok_or_else(|| EmitError::new("`new this()` needs a constructor"))?;
                self.function_member_signature("__smelt_static_construct".to_owned(), constructor)
            }
            StaticMember::Call(method) => {
                let target = self
                    .implementing_static_method(class, method)
                    .ok_or_else(|| {
                        EmitError::new("a static method called through `this` is not declared")
                    })?;
                let name = format!(
                    "__smelt_static_call_{}",
                    static_method_rust_name(self.context, target.id)?
                );
                self.function_member_signature(name, target)
            }
            StaticMember::ClassValue => Ok(MemberSignature {
                name: "__smelt_static_class_value".to_owned(),
                generics: String::new(),
                params: Vec::new(),
                param_tys: Vec::new(),
                param_passing: Vec::new(),
                ret_ty: self.type_id(Type::Unknown)?,
                ret_text: "SmeltUnknown".to_owned(),
                fallible: false,
            }),
        }
    }

    /// A member signature mirroring `function`'s own parameters, generics and
    /// return value (a constructor's or a static method's).
    fn function_member_signature(
        &self,
        name: String,
        function: &MirFunction,
    ) -> Result<MemberSignature, EmitError> {
        let mut emitter = FunctionEmitter::new(self.mir, self.context, function)?;
        emitter.apply_crate_generic_decision();
        let generics = emitter.method_own_generics_text()?;
        let mut params = Vec::new();
        let mut param_tys = Vec::new();
        let mut param_passing = Vec::new();
        for param in &function.params {
            param_passing.push(emitter.parameter_passing(*param)?);
            params.push((
                emitter.local_name(*param)?.into_owned(),
                emitter.parameter_decl_type(*param)?.as_str().to_owned(),
            ));
            param_tys.push(emitter.function_local_decl(function, *param)?.ty);
        }
        let ret_text = emitter
            .rendered_type(function.return_ty)?
            .as_str()
            .to_owned();
        Ok(MemberSignature {
            name,
            generics,
            params,
            param_tys,
            param_passing,
            ret_ty: function.return_ty,
            ret_text,
            fallible: true,
        })
    }

    /// The statics trait of `class` and its impl for every receiver class.
    pub(crate) fn statics_trait_text(
        &self,
        class: Symbol,
        members: &[StaticMember],
    ) -> Result<String, EmitError> {
        let trait_name = crate::static_receiver::statics_trait_name(&self.type_rust_name(class)?);
        let signatures = members
            .iter()
            .map(|member| Ok((*member, self.member_signature(class, *member)?)))
            .collect::<Result<Vec<_>, EmitError>>()?;
        let header = |signature: &MemberSignature| {
            let params = signature
                .params
                .iter()
                .map(|(name, ty)| format!("{name}: {ty}"))
                .collect::<Vec<_>>()
                .join(", ");
            if signature.fallible {
                format!(
                    "fn {}{}({params}) -> Result<{}, {MEMBER_ERROR}>",
                    signature.name, signature.generics, signature.ret_text
                )
            } else {
                format!(
                    "fn {}{}({params}) -> {}",
                    signature.name, signature.generics, signature.ret_text
                )
            }
        };
        let class_source = self.symbol_source_name(class)?;
        let mut out = format!(
            "/// The statics of `{class_source}` or a class extending it, as a static method's `this` reaches them.\n#[allow(dead_code, non_snake_case)]\ntrait {trait_name} {{\n"
        );
        for (_, signature) in &signatures {
            out.push_str(&format!("    {};\n", header(signature)));
        }
        out.push_str("}\n");
        for receiver in crate::static_receiver::receiver_classes(self.mir, class) {
            let receiver_type = self.type_rust_name(receiver)?;
            out.push_str(&format!(
                "#[allow(unused_variables, unreachable_code, clippy::needless_return)]\nimpl {trait_name} for {receiver_type} {{\n"
            ));
            for (member, signature) in &signatures {
                let body = self.member_body(class, receiver, *member, signature)?;
                out.push_str(&format!("    {} {{ {body} }}\n", header(signature)));
            }
            out.push_str("}\n");
        }
        Ok(out)
    }

    /// The body of `member` in `receiver`'s impl of `class`'s statics trait.
    fn member_body(
        &self,
        class: Symbol,
        receiver: Symbol,
        member: StaticMember,
        signature: &MemberSignature,
    ) -> Result<String, EmitError> {
        let scope = self.render_scope();
        let arg_names = signature
            .params
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        Ok(match member {
            StaticMember::Get(field) => match self.static_field_access(receiver, field)? {
                Some(access) => self
                    .rendered_at_type(&access.read, access.ty, signature.ret_ty, &scope)?
                    .into_string(),
                None => self.default_value(signature.ret_ty)?.into_string(),
            },
            StaticMember::Set(field) => {
                let store = match self.static_field_access(receiver, field)? {
                    Some(StaticFieldAccess {
                        store: Some(target),
                        ty,
                        ..
                    }) => {
                        let value = self.rendered_at_type(
                            &RustExpr::path("smelt_value").cloned_value(),
                            signature.ret_ty,
                            ty,
                            &scope,
                        )?;
                        match target {
                            StaticStoreTarget::Global(global) => {
                                format!("{};", self.global_store_text(global, &value)?)
                            }
                            StaticStoreTarget::Inherited(entry) => format!(
                                "let _ = {};",
                                crate::classes::inherited_static_write_text(
                                    self.mir,
                                    self.context,
                                    &entry,
                                    value.delimited(),
                                )
                                .ok_or_else(|| EmitError::new("inherited static has no setter"))?
                            ),
                        }
                    }
                    _ => String::new(),
                };
                format!("{store} smelt_value")
            }
            StaticMember::Construct => {
                let receiver_class = crate::classes::find_class(self.mir, receiver);
                let Some(constructor) = receiver_class
                    .and_then(|found| found.constructor)
                    .and_then(|id| self.function_by_id(id))
                else {
                    return Ok(format!(
                        "Ok({})",
                        self.default_value(signature.ret_ty)?.delimited()
                    ));
                };
                // Arguments by position, as JavaScript passes them: an
                // argument the receiver's constructor does not declare is
                // dropped, a parameter no argument fills takes its default.
                let mut args = Vec::new();
                for (index, param) in constructor.params.iter().enumerate() {
                    let param_ty = self.function_local_decl(constructor, *param)?.ty;
                    args.push(
                        match (arg_names.get(index), signature.param_tys.get(index)) {
                            (Some(name), Some(arg_ty)) => self
                                .rendered_at_type(
                                    &RustExpr::path(name.clone()),
                                    *arg_ty,
                                    param_ty,
                                    &scope,
                                )?
                                .into_string(),
                            _ => self.missing_argument_value(param_ty)?.into_string(),
                        },
                    );
                }
                let throwing = if self.call_is_throwing(constructor) {
                    "?"
                } else {
                    ""
                };
                let built = RustExpr::atom(format!(
                    "{}::new({}){throwing}",
                    self.type_rust_name(receiver)?,
                    args.join(", ")
                ));
                // The receiver's own instance type: a class without its own
                // constructor shares its base's, whose return type is the base.
                let built_ty = self
                    .existing_type_id(Type::Class {
                        name: receiver,
                        args: Vec::new(),
                    })
                    .unwrap_or(constructor.return_ty);
                let converted =
                    self.rendered_at_type(&built, built_ty, signature.ret_ty, &scope)?;
                format!("Ok({})", converted.delimited())
            }
            StaticMember::Call(method) => {
                let target = self
                    .implementing_static_method(receiver, method)
                    .ok_or_else(|| {
                        EmitError::new("a static method called through `this` is not declared")
                    })?;
                let HirOrigin::ClassStaticMethod { class: owner, .. } = target.origin else {
                    return Err(EmitError::new("a static method is not a static method"));
                };
                let mut args = Vec::new();
                if self
                    .context
                    .static_receivers()
                    .receiver_class(target.id)
                    .is_some()
                {
                    args.push(receiver_argument(&self.type_rust_name(receiver)?));
                }
                args.extend(arg_names.iter().cloned());
                let call = format!(
                    "{}::{}({})",
                    self.type_rust_name(owner)?,
                    static_method_rust_name(self.context, target.id)?,
                    args.join(", ")
                );
                let _ = class;
                if self.call_is_throwing(target) {
                    call
                } else {
                    format!("Ok({call})")
                }
            }
            StaticMember::ClassValue => self.class_value(receiver, None)?.into_string(),
        })
    }

    /// One `Rvalue::StaticDispatch`, as the fallible call it is.
    pub(super) fn static_dispatch_call(
        &self,
        receiver: Option<Symbol>,
        class: Symbol,
        member: StaticMember,
        args: &[Operand],
    ) -> Result<(RenderedCall, TypeId), EmitError> {
        if let Some(named) = receiver {
            // A static method reached through a class that inherits it.
            let StaticMember::Call(method) = member else {
                return Err(EmitError::new(
                    "a static member other than a call is reached through a named class",
                ));
            };
            let target = self
                .implementing_static_method(named, method)
                .ok_or_else(|| {
                    EmitError::new("a static method called through a class is not declared")
                })?;
            let HirOrigin::ClassStaticMethod { class: owner, .. } = target.origin else {
                return Err(EmitError::new("a static method is not a static method"));
            };
            let call = self.class_static_method_call(target, owner, args, Some(named))?;
            // A static method with its own generics answers its type at this
            // call's instantiation, as a direct call does.
            let source_ty = self
                .static_call_monomorphization(target, args)?
                .map_or(target.return_ty, |pinned| pinned.return_ty);
            return Ok((call, source_ty));
        }
        // A class no other class extends is its static methods' only
        // receiver (`StaticReceiverPlan::has_trait`): the member is the
        // class's own, called directly.
        if !self.context.static_receivers().has_trait(class) {
            return match member {
                StaticMember::Call(method) => {
                    let target =
                        self.implementing_static_method(class, method)
                            .ok_or_else(|| {
                                EmitError::new(
                                    "a static method called through `this` is not declared",
                                )
                            })?;
                    let HirOrigin::ClassStaticMethod { class: owner, .. } = target.origin else {
                        return Err(EmitError::new("a static method is not a static method"));
                    };
                    let call = self.class_static_method_call(target, owner, args, None)?;
                    let source_ty = self
                        .static_call_monomorphization(target, args)?
                        .map_or(target.return_ty, |pinned| pinned.return_ty);
                    Ok((call, source_ty))
                }
                StaticMember::Construct => {
                    let constructor = crate::classes::find_class(self.mir, class)
                        .and_then(|found| found.constructor)
                        .and_then(|id| self.function_by_id(id))
                        .ok_or_else(|| EmitError::new("`new this()` needs a constructor"))?;
                    Ok((
                        self.static_call(constructor.id, args)?,
                        constructor.return_ty,
                    ))
                }
                _ => Err(EmitError::new(
                    "a static field or class value reached the fallible dispatch path",
                )),
            };
        }
        let signature = self.member_signature(class, member)?;
        let rendered = self.member_arguments(&signature, args)?;
        let trait_name = crate::static_receiver::statics_trait_name(&self.type_rust_name(class)?);
        let call = RustExpr::atom(format!(
            "<{RECEIVER_GENERIC} as {trait_name}>::{}({})",
            signature.name,
            rendered.join(", ")
        ));
        let source_ty = match member {
            StaticMember::Call(method) => match self.implementing_static_method(class, method) {
                Some(target) => self
                    .static_call_monomorphization(target, args)?
                    .map_or(signature.ret_ty, |pinned| pinned.return_ty),
                None => signature.ret_ty,
            },
            _ => signature.ret_ty,
        };
        Ok((RenderedCall::propagating(call), source_ty))
    }

    /// `args` rendered at `signature`'s parameters (a borrowed callback
    /// reborrowed, a missing trailing argument defaulted).
    fn member_arguments(
        &self,
        signature: &MemberSignature,
        args: &[Operand],
    ) -> Result<Vec<String>, EmitError> {
        let mut rendered = Vec::new();
        for (index, arg) in args.iter().enumerate() {
            let Some(target_ty) = signature.param_tys.get(index).copied() else {
                continue;
            };
            let borrowed_callback =
                matches!(
                    signature.param_passing.get(index),
                    Some(ParameterPassing::BorrowedCallback | ParameterPassing::MutableReference)
                ) && matches!(self.mir.types.get(target_ty), Some(Type::Function(_)));
            rendered.push(if borrowed_callback {
                self.borrowed_function_argument_expr(arg, target_ty, None)?
                    .into_string()
            } else {
                self.value_at_type(arg, target_ty)?.into_string()
            });
        }
        for target_ty in signature.param_tys.iter().skip(args.len()) {
            rendered.push(self.missing_argument_value(*target_ty)?.into_string());
        }
        Ok(rendered)
    }

    /// One `Rvalue::StaticDispatch` that cannot throw (a static field read or
    /// write, or the class value), as the value it answers at `dest_ty`.
    pub(super) fn static_dispatch_value(
        &self,
        class: Symbol,
        member: StaticMember,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let scope = self.render_scope();
        let (value, value_ty) = if self.context.static_receivers().has_trait(class) {
            let signature = self.member_signature(class, member)?;
            let rendered = self.member_arguments(&signature, args)?;
            let trait_name =
                crate::static_receiver::statics_trait_name(&self.type_rust_name(class)?);
            (
                RustExpr::atom(format!(
                    "<{RECEIVER_GENERIC} as {trait_name}>::{}({})",
                    signature.name,
                    rendered.join(", ")
                )),
                signature.ret_ty,
            )
        } else {
            // The class's own static, read and written directly.
            match member {
                StaticMember::Get(field) | StaticMember::Set(field) => {
                    let access = self.static_field_access(class, field)?.ok_or_else(|| {
                        EmitError::new("a static reached through `this` is not declared")
                    })?;
                    if matches!(member, StaticMember::Get(_)) {
                        (access.read, access.ty)
                    } else {
                        let Some(arg) = args.first() else {
                            return Err(EmitError::new("a static write has no value"));
                        };
                        let value = self.value_at_type(arg, access.ty)?;
                        let store = match access.store {
                            Some(StaticStoreTarget::Global(global)) => format!(
                                "{};",
                                self.global_store_text(
                                    global,
                                    &RustExpr::path("smelt_value").cloned_value()
                                )?
                            ),
                            Some(StaticStoreTarget::Inherited(entry)) => format!(
                                "let _ = {};",
                                crate::classes::inherited_static_write_text(
                                    self.mir,
                                    self.context,
                                    &entry,
                                    "smelt_value.clone()",
                                )
                                .ok_or_else(|| EmitError::new("inherited static has no setter"))?
                            ),
                            None => String::new(),
                        };
                        (
                            RustExpr::atom(format!(
                                "{{ let smelt_value = {}; {store} smelt_value }}",
                                value.delimited()
                            )),
                            access.ty,
                        )
                    }
                }
                StaticMember::ClassValue => {
                    (self.class_value(class, None)?, self.type_id(Type::Unknown)?)
                }
                StaticMember::Construct | StaticMember::Call(_) => {
                    return Err(EmitError::new(
                        "a static construction or call reached the assignment path",
                    ));
                }
            }
        };
        self.rendered_at_type(&value, value_ty, dest_ty, &scope)
    }
}
