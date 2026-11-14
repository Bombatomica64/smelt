//! Casts between the classes of a polymorphic hierarchy
//! (`crate::class_polymorphism`): an upcast re-views the same shared cell, a
//! downcast checks the cell's concrete class.

use super::*;
use crate::class_polymorphism::{concrete_classes, descendant_binding, polymorphic_ancestors};
use crate::rust::RustExpr;

impl FunctionEmitter<'_> {
    /// The handle type `ty` (a class type) as a constructor path with its
    /// arguments as a turbofish (`Base::<JsString>`), so building it from a
    /// coerced `Rc` never leaves the arguments to inference.
    fn handle_constructor_path(&self, ty: TypeId) -> Result<String, EmitError> {
        let rendered = self.rendered_type(ty)?.as_str().to_owned();
        Ok(match rendered.find('<') {
            Some(index) => format!("{}::{}", &rendered[..index], &rendered[index..]),
            None => rendered,
        })
    }

    /// The source and target class names of a class-to-class conversion
    /// between two distinct reference classes.
    fn reference_class_pair(&self, source: TypeId, target: TypeId) -> Option<(Symbol, Symbol)> {
        let (Some(Type::Class { name: from, .. }), Some(Type::Class { name: to, .. })) =
            (self.mir.types.get(source), self.mir.types.get(target))
        else {
            return None;
        };
        (from != to
            && self.context.is_reference_class(*from)
            && self.context.is_reference_class(*to))
        .then_some((*from, *to))
    }

    /// `value` (of class type `source`) viewed as its polymorphic ancestor
    /// `target`: the same `Rc`, unsized into the ancestor's cell trait object
    /// (or re-viewed through a polymorphic source's upcast method). `None`
    /// when `target` is not a polymorphic strict ancestor of `source`.
    pub(super) fn polymorphic_upcast_expr(
        &self,
        value: &RustExpr,
        source: TypeId,
        target: TypeId,
    ) -> Result<Option<RustExpr>, EmitError> {
        let Some((from, to)) = self.reference_class_pair(source, target) else {
            return Ok(None);
        };
        if !self.context.is_polymorphic_class(to)
            || !polymorphic_ancestors(self.mir, self.context.polymorphic_classes(), from)
                .contains(&to)
        {
            return Ok(None);
        }
        let constructor = self.handle_constructor_path(target)?;
        let cell = if self.context.is_polymorphic_class(from) {
            let target_type = self.type_rust_name(to)?;
            format!(
                "{}.0.clone().{}()",
                value.tight(),
                crate::class_polymorphism::upcast_method_name(&target_type)
            )
        } else {
            format!("{}.0.clone()", value.tight())
        };
        Ok(Some(RustExpr::atom(format!("{constructor}({cell})"))))
    }

    /// The checked downcast of the handle bound to `smelt_struct_value` (of
    /// polymorphic class type `source`) to its descendant `target`: an
    /// `Option` holding the same object when its cell is one of `target`'s
    /// concrete classes. `None` (no expression) when the pair is not a
    /// downcast of a polymorphic handle.
    ///
    /// A concrete class whose type arguments do not follow from `target`'s
    /// (`descendant_binding`), or that fixes one of them to a type other than
    /// `target`'s, cannot be the class of a `target` instance and is skipped.
    pub(super) fn polymorphic_downcast_expr(
        &self,
        source: TypeId,
        target: TypeId,
    ) -> Result<Option<String>, EmitError> {
        let Some((from, to)) = self.reference_class_pair(source, target) else {
            return Ok(None);
        };
        let polymorphic = self.context.polymorphic_classes();
        if !self.context.is_polymorphic_class(from)
            || !polymorphic_ancestors(self.mir, polymorphic, to).contains(&from)
        {
            return Ok(None);
        }
        let Some(Type::Class { args, .. }) = self.mir.types.get(target) else {
            return Ok(None);
        };
        let target_args = args
            .iter()
            .map(|arg| Ok(self.rendered_type(*arg)?.as_str().to_owned()))
            .collect::<Result<Vec<_>, EmitError>>()?;
        let constructor = self.handle_constructor_path(target)?;
        let mut arms = Vec::new();
        for concrete in concrete_classes(self.mir, polymorphic, to) {
            let concrete_args = if concrete == to {
                self.context
                    .type_param_elision()
                    .retain_carried(to, target_args.clone())
            } else {
                let Some(binding) = descendant_binding(self.mir, concrete, to) else {
                    continue;
                };
                if binding
                    .fixed
                    .iter()
                    .any(|(index, ty)| args.get(*index) != Some(ty))
                {
                    continue;
                }
                let Some(bound) = binding
                    .param_sources
                    .iter()
                    .map(|index| target_args.get(*index).cloned())
                    .collect::<Option<Vec<_>>>()
                else {
                    continue;
                };
                self.context
                    .type_param_elision()
                    .retain_carried(concrete, bound)
            };
            let generics = if concrete_args.is_empty() {
                String::new()
            } else {
                format!("<{}>", concrete_args.join(", "))
            };
            let inner = format!("{}Inner{generics}", self.type_rust_name(concrete)?);
            arms.push(format!(
                "match smelt_any.downcast::<::std::cell::RefCell<{inner}>>() {{ Ok(smelt_cell) => return Some({constructor}(smelt_cell)), Err(smelt_other) => smelt_other }}"
            ));
        }
        if arms.is_empty() {
            return Ok(None);
        }
        let mut body = String::from("let smelt_any = smelt_struct_value.0.clone().smelt_any(); ");
        for arm in arms {
            body.push_str(&format!("let smelt_any = {arm}; "));
        }
        body.push_str("let _ = smelt_any; None");
        Ok(Some(format!("(|| {{ {body} }})()")))
    }

    /// The concrete class name of the polymorphic handle `value` (an
    /// `&'static str`), which `instanceof` compares.
    pub(super) fn polymorphic_class_name_expr(value: &str) -> String {
        format!("{value}.0.smelt_class_name()")
    }
}

impl FunctionEmitter<'_> {
    /// The inherent method a class of a polymorphic hierarchy erases itself
    /// through (`crate::class_polymorphism::OWN_ERASURE_METHOD`), rendered by
    /// the same field-wise projection an erasure of a value of exactly that
    /// class gets inline, so an instance erases identically whatever static
    /// type it was held at.
    ///
    /// `class_ty` is the class at its own type parameters; `fallback` is the
    /// body used when no such type is interned.
    pub(crate) fn own_erasure_method_text(
        &self,
        class_ty: Option<TypeId>,
        fallback: &str,
    ) -> Result<String, EmitError> {
        let body = match class_ty {
            Some(ty) => self
                .concrete_class_unknown_object_expr(&RustExpr::path("smelt_own"), ty)?
                .into_string(),
            None => fallback.to_owned(),
        };
        Ok(format!(
            "    #[allow(dead_code)]\n    fn {}(self) -> SmeltUnknown {{ let smelt_own = self; let _ = &smelt_own; {body} }}\n",
            crate::class_polymorphism::OWN_ERASURE_METHOD
        ))
    }
}

/// The function implementing the method named `method` for instances of
/// `class`: its own declaration, else the nearest ancestor's.
fn implementing_method(mir: &Mir, class: Symbol, method: Symbol) -> Option<FuncId> {
    let mut current = Some(class);
    let mut seen = HashSet::new();
    while let Some(name) = current
        && seen.insert(name)
    {
        let found = crate::classes::find_class(mir, name)?;
        for id in &found.methods {
            let function = mir.functions.get(usize::try_from(id.0).ok()?)?;
            if matches!(function.origin, HirOrigin::ClassMethod { method: own, .. } if own == method)
            {
                return Some(*id);
            }
        }
        current = found.base;
    }
    None
}

impl FunctionEmitter<'_> {
    /// The class whose `impl` this method emitter renders into.
    fn method_impl_owner(&self) -> Option<Symbol> {
        self.role.inherited_impl_owner().or(match self.function.origin {
            HirOrigin::ClassMethod { class, .. } => Some(class),
            _ => None,
        })
    }

    /// The dispatch prologue of a GENERIC method of a polymorphic class
    /// (`crate::class_polymorphism`) that a subclass overrides.
    ///
    /// A `dyn` trait method cannot be generic over the method's own type
    /// parameters, so a generic override is not a cell-trait method or a
    /// callable slot (which erased it to `SmeltUnknown`): the base method
    /// instead checks the cell's concrete class and forwards to the
    /// override, monomorphized at the caller's own instantiation
    /// (`return Sub(cell).map(f)` inside `fn map<U>(&self, f) -> SmeltList<U>`).
    /// The body that follows is the class's own implementation.
    ///
    /// An override is forwarded to when its class's type arguments follow
    /// from the owner's (`descendant_binding`); one that fixes them to a
    /// concrete type is forwarded to only when the method's signature does not
    /// mention the owner's type parameters, since the forwarded arguments are
    /// typed at the owner's parameters. Empty when nothing is forwarded.
    pub(super) fn generic_override_dispatch_prologue(&self) -> Result<String, EmitError> {
        let HirOrigin::ClassMethod { method, .. } = self.function.origin else {
            return Ok(String::new());
        };
        let Some(owner) = self.method_impl_owner() else {
            return Ok(String::new());
        };
        if self.function.type_params.is_empty()
            || self.function.is_async
            || self.function.is_generator
            || !self.context.is_polymorphic_class(owner)
            || self.method_own_generics_declared().is_empty()
        {
            return Ok(String::new());
        }
        let Some(owner_class) = crate::classes::find_class(self.mir, owner) else {
            return Ok(String::new());
        };
        let own_implementation = implementing_method(self.mir, owner, method);
        let owner_params: Vec<Symbol> = owner_class
            .type_params
            .iter()
            .map(|param| param.name)
            .collect();
        let signature_mentions_owner_params = {
            let mut mentioned = HashSet::new();
            for ty in self
                .function
                .params
                .iter()
                .skip(1)
                .filter_map(|param| self.function.locals.get(usize::try_from(param.0).ok()?))
                .map(|local| local.ty)
                .chain(std::iter::once(self.function.return_ty))
            {
                crate::generic_bindings::collect_type_param_occurrences(
                    self.mir,
                    ty,
                    &mut mentioned,
                );
            }
            owner_params.iter().any(|param| mentioned.contains(param))
        };
        let owner_param_idents = owner_params
            .iter()
            .map(|param| {
                self.mir
                    .symbols
                    .get(*param)
                    .map(|name| crate::names::type_param(name).into_string())
                    .ok_or_else(|| EmitError::new("class type parameter has unknown symbol"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let args = self
            .function
            .params
            .iter()
            .skip(1)
            .map(|param| self.local_name(*param))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let method_name = self.method_rust_name(method)?;
        let polymorphic = self.context.polymorphic_classes();
        let mut arms = String::new();
        for concrete in concrete_classes(self.mir, polymorphic, owner) {
            if concrete == owner {
                continue;
            }
            let Some(implementation) = implementing_method(self.mir, concrete, method) else {
                continue;
            };
            if Some(implementation) == own_implementation
                || !self.context.is_generic_function(implementation)
            {
                continue;
            }
            let Some(binding) = descendant_binding(self.mir, concrete, owner) else {
                continue;
            };
            if !binding.fixed.is_empty() && signature_mentions_owner_params {
                continue;
            }
            let Some(concrete_args) = binding
                .param_sources
                .iter()
                .map(|index| owner_param_idents.get(*index).cloned())
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let concrete_args = self
                .context
                .type_param_elision()
                .retain_carried(concrete, concrete_args);
            let generics = if concrete_args.is_empty() {
                String::new()
            } else {
                format!("<{}>", concrete_args.join(", "))
            };
            let concrete_type = self.type_rust_name(concrete)?;
            let turbofish = if generics.is_empty() {
                String::new()
            } else {
                format!("::{generics}")
            };
            arms.push_str(&format!(
                "        let smelt_any = match smelt_any.downcast::<::std::cell::RefCell<{concrete_type}Inner{generics}>>() {{ Ok(smelt_cell) => return {concrete_type}{turbofish}(smelt_cell).{method_name}({args}), Err(smelt_other) => smelt_other }};\n"
            ));
        }
        if arms.is_empty() {
            return Ok(String::new());
        }
        Ok(format!(
            "    {{\n        let smelt_any = self.0.clone().smelt_any();\n{arms}        let _ = smelt_any;\n    }}\n"
        ))
    }
}
