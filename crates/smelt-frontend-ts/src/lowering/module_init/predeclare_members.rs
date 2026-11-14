//! Class and interface member predeclaration: method fields of module classes
//! and call signatures of callable interfaces, declared before bodies read them.

use super::super::ModuleBuilder;
use oxc::ast::ast::{Declaration, Program, Statement};
use smelt_hir::{Field, FunctionType, Type, Visibility};

impl ModuleBuilder<'_> {
    /// Predeclare instance-method member types for classes in a manifest-wide type pass.
    ///
    /// The metadata is stored with structural type fields so cyclic importers
    /// can type method calls before the class's runtime item is lowered.
    pub(in crate::lowering) fn predeclare_class_method_fields(&mut self, program: &Program<'_>) {
        for statement in &program.body {
            let class = match statement {
                Statement::ClassDeclaration(class) => Some(class),
                Statement::ExportDeclaration(export) => match &export.declaration {
                    Declaration::ClassDeclaration(class) => Some(class),
                    _ => None,
                },
                _ => None,
            };
            let Some(class) = class else { continue };
            let Some(id) = &class.id else { continue };
            // Same symbol the declaration will use, so a crate-ambiguous class
            // name attaches its predeclared method surface to ITS class rather
            // than to the other module's class of the same spelling (see
            // `module_qualified_type_name`).
            let name = self
                .module_qualified_type_name(id.name.as_str())
                .unwrap_or_else(|| self.intern_type_name(id.name.as_str()));
            if self
                .push_type_parameter_scope(class.type_parameters.as_deref())
                .is_err()
            {
                continue;
            }
            let signatures = self.class_method_signatures(&class.body.body);
            let members = self.predeclared_class_member_fields(&class.body.body);
            self.pop_type_parameter_scope();
            let Ok(signatures) = signatures else { continue };
            let mut fields = signatures
                .into_iter()
                .map(|method| {
                    let ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
                        params: method.params.iter().map(|param| param.ty).collect(),
                        rest: method.rest,
                        required_params: method.required_params,
                        mutable_params: Vec::new(),
                        return_ty: method.return_ty,
                        is_async: method.is_async,
                        may_throw: false,
                    }));
                    Field {
                        name: method.name,
                        ty,
                        visibility: Visibility::Public,
                        optional: false,
                        span: method.span,
                    }
                })
                .collect::<Vec<_>>();
            let members = members
                .into_iter()
                .filter(|member| fields.iter().all(|method| method.name != member.name))
                .collect::<Vec<_>>();
            fields.extend(members);
            if !fields.is_empty() {
                self.ctx.type_alias_fields.write().insert(name, fields);
            }
        }
    }

    /// Predeclare the call signatures of this module's callable interfaces
    /// (`interface TextRespond { (text: string): Response }`) crate-wide.
    ///
    /// A value of a callable-interface type is called through the interface's
    /// call signatures (`function_member_type`), which are otherwise recorded
    /// only when the interface's module lowers. An import cycle can lower a
    /// caller first — `c.text('..')` in a module ordered before the one that
    /// declares `get text(): TextRespond` — and the call then had no callable
    /// shape and was lowered as a method the class does not have.
    ///
    /// Only interfaces with no `extends` clause are recorded here, because
    /// inherited call signatures need the parents lowered; the interface's own
    /// lowering replaces the entry with the complete set either way.
    pub(in crate::lowering) fn predeclare_interface_call_signatures(
        &mut self,
        program: &Program<'_>,
    ) {
        for statement in &program.body {
            let interface = match statement {
                Statement::TSInterfaceDeclaration(interface) => interface,
                Statement::ExportDeclaration(export) => match &export.declaration {
                    Declaration::TSInterfaceDeclaration(interface) => interface,
                    _ => continue,
                },
                _ => continue,
            };
            if !interface.extends.is_empty() {
                continue;
            }
            let signatures = interface
                .body
                .body
                .iter()
                .filter_map(|member| match member {
                    oxc::ast::ast::TSSignature::TSCallSignatureDeclaration(signature) => {
                        Some(signature)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            if signatures.is_empty() {
                continue;
            }
            let name_text = interface.id.name.as_str();
            let name = self
                .module_qualified_type_name(name_text)
                .unwrap_or_else(|| self.intern_type_name(name_text));
            if self.ctx.interface_call_signatures.contains_key(&name)
                || self
                    .push_type_parameter_scope(interface.type_parameters.as_deref())
                    .is_err()
            {
                continue;
            }
            let lowered = signatures
                .into_iter()
                .map(|signature| self.call_signature_to_hir(signature))
                .collect::<Result<Vec<_>, _>>();
            self.pop_type_parameter_scope();
            if let Ok(lowered) = lowered {
                self.ctx
                    .interface_call_signatures
                    .write()
                    .insert(name, lowered);
            }
        }
    }

    /// Whether `ty` names a callable interface that this module knows ONLY by
    /// the call signatures [`Self::predeclare_interface_call_signatures`]
    /// recorded — the interface itself is not lowered yet.
    ///
    /// Such a predeclared signature can be less precise than the interface's
    /// own lowering will be (a parameter type computed from a const object in
    /// another module is not known before that module lowers), so a call
    /// through a value of this type must not fix the callee's Rust type from
    /// it. The caller then calls the RECORD, whose `__smelt_call` slot the
    /// emitter invokes with the slot's final signature.
    pub(in crate::lowering) fn callable_interface_known_only_by_predeclaration(
        &self,
        ty: smelt_hir::TypeId,
    ) -> bool {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(ty) else {
            return false;
        };
        self.interfaces.call_signatures(*name).is_some() && self.find_interface(*name).is_none()
    }

    /// The non-method half of a class's predeclared member surface: every
    /// instance getter and own data property whose type the source annotates.
    ///
    /// An import cycle can lower a module that reads `c.req` or `c[KEY]` before
    /// the module declaring `c`'s class; without these entries the read had no
    /// declared type and fell to the erased `Unknown` path even though the
    /// annotation fixes it. Each member is recorded as a plain field of its
    /// declared READ type — exactly what HIR records for an accessor read once
    /// the class is lowered (the emitter picks the getter call from the class's
    /// descriptors).
    ///
    /// Left out, so the surface never claims more than it knows:
    /// * static members and `#private` names (not readable from another module);
    /// * members with no annotation (their type comes from the initializer or
    ///   body, which this pass does not lower);
    /// * keys that are not static, including a computed key no const resolves;
    /// * callable members (a function type, or a callable interface such as
    ///   `get text(): TextRespond`): a predeclared callable entry is how a
    ///   cycle-early consumer recognizes a METHOD (`callbacks::closures`, the
    ///   member-call path), and a callable property or getter is not one — its
    ///   call must read the member first, which only the lowered class knows
    ///   how to do.
    fn predeclared_class_member_fields(
        &mut self,
        elements: &[oxc::ast::ast::ClassElement<'_>],
    ) -> Vec<Field> {
        use oxc::ast::ast::{ClassElement, MethodDefinitionKind, PropertyKey};
        let mut fields = Vec::new();
        for element in elements {
            let (key, annotation, optional, span) = match element {
                ClassElement::MethodDefinition(method)
                    if method.kind == MethodDefinitionKind::Get && !method.r#static =>
                {
                    let Some(annotation) = method.value.return_type.as_ref() else {
                        continue;
                    };
                    (&method.key, annotation, false, method.span)
                }
                ClassElement::PropertyDefinition(property) if !property.r#static => {
                    let Some(annotation) = property.type_annotation.as_ref() else {
                        continue;
                    };
                    (&property.key, annotation, property.optional, property.span)
                }
                _ => continue,
            };
            if matches!(key, PropertyKey::PrivateIdentifier(_))
                || !self.is_resolvable_property_key(key)
            {
                continue;
            }
            let Ok(name) = self.property_key_symbol(key) else {
                continue;
            };
            let Ok(ty) = self.ts_type_to_hir(&annotation.type_annotation) else {
                continue;
            };
            if matches!(self.ctx.krate.types.get(ty), Some(Type::Function(_))) {
                continue;
            }
            if fields.iter().any(|field: &Field| field.name == name) {
                continue;
            }
            fields.push(Field {
                name,
                ty,
                visibility: Visibility::Public,
                optional,
                span: self.span(span.start, span.end),
            });
        }
        fields
    }
}
