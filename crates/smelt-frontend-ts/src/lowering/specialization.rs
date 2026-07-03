//! TypeScript specialization-manifest lookup and class-member materialization.

use std::path::Path;

use super::{
    Allocator, Body, Expr, ExprKind, Expression, Field, FunctionType, Item, Literal,
    MethodDefinitionKind, ModuleBuilder, ParseOptions, Parser, SmeltError, SourceType, Span,
    SpecializationData, Statement, Type, Visibility,
};

/// Returns specialization data for one exact TypeScript source path.
pub(super) fn specialization_for_path(
    path: &str,
    manifest: Option<&smelt_specialize::SpecializationManifest>,
) -> Option<SpecializationData> {
    let manifest = manifest?;
    if manifest.language != smelt_specialize::HostLanguage::TypeScript {
        return None;
    }
    let module = manifest
        .modules
        .iter()
        .find(|module| paths_match(path, &module.path))
        .or_else(|| {
            (path == "<memory>" && manifest.modules.len() == 1)
                .then(|| manifest.modules.first())
                .flatten()
        })
        .cloned()?;
    Some(SpecializationData {
        module,
        values: manifest.values.nodes.clone(),
        required_adapters: manifest.required_adapters.clone(),
    })
}

/// Compares source paths after canonicalization when both files exist.
fn paths_match(frontend: &str, materialized: &str) -> bool {
    if frontend == materialized {
        return true;
    }
    match (
        Path::new(frontend).canonicalize(),
        Path::new(materialized).canonicalize(),
    ) {
        (Ok(frontend), Ok(materialized)) => frontend == materialized,
        _ => false,
    }
}

impl ModuleBuilder<'_> {
    /// Returns whether provenance points at a nested function expression.
    pub(super) fn materialized_callable_needs_lift(
        &self,
        provenance: &smelt_specialize::CallableProvenance,
    ) -> bool {
        let start = usize::try_from(provenance.span.start).unwrap_or(usize::MAX);
        let end = usize::try_from(provenance.span.end).unwrap_or(usize::MAX);
        self.source
            .get(start..end)
            .is_some_and(|source| source.trim_start().starts_with("function"))
    }

    /// Lifts a source-mapped decorator callable into a hidden class method.
    pub(super) fn lift_materialized_class_callable(
        &mut self,
        provenance: &smelt_specialize::CallableProvenance,
        signature: &smelt_specialize::FunctionSignature,
        hidden_name: &str,
        class_name: smelt_hir::Symbol,
        class_ty: smelt_hir::TypeId,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let start = usize::try_from(provenance.span.start).unwrap_or(usize::MAX);
        let end = usize::try_from(provenance.span.end).unwrap_or(usize::MAX);
        let source = self.source.get(start..end).ok_or_else(|| {
            SmeltError::native_specialization_adapter_required(
                self.materialized_span(&provenance.span),
                "typescript.callable-source",
                &format!(
                    "callable '{}' has an invalid source span",
                    provenance.qualified_name
                ),
            )
        })?;
        let function_start = source.find("function").ok_or_else(|| {
            SmeltError::native_specialization_adapter_required(
                self.materialized_span(&provenance.span),
                "typescript.callable-source",
                &format!(
                    "callable '{}' does not map to a source function expression",
                    provenance.qualified_name
                ),
            )
        })?;
        let function_end = source.rfind('}').ok_or_else(|| {
            SmeltError::native_specialization_adapter_required(
                self.materialized_span(&provenance.span),
                "typescript.callable-source",
                &format!(
                    "callable '{}' has no complete source body",
                    provenance.qualified_name
                ),
            )
        })?;
        let callable_source = &source[function_start..=function_end];
        let wrapped = format!("const __smelt_lifted = {callable_source};");
        let allocator = Allocator::default();
        let parsed = Parser::new(
            &allocator,
            &wrapped,
            SourceType::default().with_typescript(true),
        )
        .with_options(ParseOptions::default())
        .parse();
        if !parsed.diagnostics.is_empty() {
            return Err(SmeltError::native_specialization_adapter_required(
                self.materialized_span(&provenance.span),
                "typescript.callable-source",
                &format!(
                    "callable '{}' could not be parsed from its source span",
                    provenance.qualified_name
                ),
            ));
        }
        let function = parsed
            .program
            .body
            .first()
            .and_then(|statement| match statement {
                Statement::VariableDeclaration(variable) => variable.declarations.first(),
                _ => None,
            })
            .and_then(|declaration| declaration.init.as_ref())
            .and_then(|expression| match expression {
                Expression::FunctionExpression(function) => Some(function.as_ref()),
                _ => None,
            })
            .ok_or_else(|| {
                SmeltError::native_specialization_adapter_required(
                    self.materialized_span(&provenance.span),
                    "typescript.callable-source",
                    &format!(
                        "callable '{}' is not a liftable function expression",
                        provenance.qualified_name
                    ),
                )
            })?;
        let hint = self.materialized_static_type(
            &smelt_specialize::StaticType::Function(Box::new(signature.clone())),
        );
        let capture_items = provenance
            .captures
            .iter()
            .map(|(name, value)| {
                let capture = self.materialized_value(*value).ok_or_else(|| {
                    SmeltError::native_specialization_adapter_required(
                        self.materialized_span(&provenance.span),
                        "typescript.callable-captures",
                        &format!("capture '{name}' is absent from the value graph"),
                    )
                })?;
                let smelt_specialize::GraphValueKind::FunctionRef(capture) = &capture.value else {
                    return Err(SmeltError::native_specialization_adapter_required(
                        self.materialized_span(&provenance.span),
                        "typescript.callable-captures",
                        &format!("capture '{name}' is not a source callable"),
                    ));
                };
                let expected = capture
                    .qualified_name
                    .rsplit('.')
                    .next()
                    .unwrap_or(&capture.qualified_name);
                let item = self
                    .ctx
                    .krate
                    .items
                    .iter()
                    .enumerate()
                    .filter_map(|(index, item)| {
                        let Item::Function(candidate_function) = item else {
                            return None;
                        };
                        let function_name =
                            self.ctx.krate.symbols.get(candidate_function.name)?;
                        let owner_name = match candidate_function.owner {
                            smelt_hir::FunctionOwner::ClassMethod { method, .. } => {
                                self.ctx.krate.symbols.get(method)
                            }
                            _ => None,
                        };
                        (function_name == expected || owner_name == Some(expected)).then_some((
                            candidate_function.span.start.abs_diff(capture.span.start),
                            index,
                        ))
                    })
                    .min_by_key(|(distance, _)| *distance)
                    .and_then(|(_, index)| u32::try_from(index).ok())
                    .map(smelt_hir::ItemId)
                    .ok_or_else(|| {
                        SmeltError::native_specialization_adapter_required(
                            self.materialized_span(&provenance.span),
                            "typescript.callable-captures",
                            &format!("capture '{name}' has no lowered source callable"),
                        )
                    })?;
                Ok((name.clone(), item))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let previous = capture_items
            .iter()
            .map(|(name, item)| (name.clone(), self.items.insert(name.clone(), *item)))
            .collect::<Vec<_>>();
        let lifted = self.function_expression_item_with_receiver(
            hidden_name,
            function,
            Some(hint),
            Some((class_name, class_ty)),
        );
        for (name, item) in previous {
            if let Some(item) = item {
                self.items.insert(name, item);
            } else {
                self.items.remove(&name);
            }
        }
        lifted.map_err(|error| {
            SmeltError::native_specialization_adapter_required(
                self.materialized_span(&provenance.span),
                "typescript.callable-lift",
                &format!(
                    "callable '{}' could not be lifted: {}",
                    provenance.qualified_name, error.message
                ),
            )
        })
    }

    /// Inserts ordered instance initializer calls at constructor entry.
    pub(super) fn prepend_materialized_initializer_calls(
        &mut self,
        constructor: smelt_hir::ItemId,
        initializers: &[(smelt_hir::ItemId, Option<String>, Option<String>)],
        class_ty: smelt_hir::TypeId,
        span: Span,
    ) -> Result<(), SmeltError> {
        let body_id = match self.item_ref(constructor) {
            Item::Function(function) => function.body,
            _ => None,
        }
        .ok_or_else(|| SmeltError::unsupported(span, "class constructor body is missing"))?;
        let methods = initializers
            .iter()
            .map(|(item, kind, name)| match self.item_ref(*item) {
                Item::Function(function) => Ok((function.name, kind.clone(), name.clone())),
                _ => Err(SmeltError::unsupported(
                    span,
                    "materialized initializer is not a function",
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let this_symbol = self.ctx.krate.symbols.intern("this");
        let body_index = usize::try_from(body_id.0).unwrap_or(usize::MAX);
        let body = self
            .ctx
            .krate
            .bodies
            .get_mut(body_index)
            .ok_or_else(|| SmeltError::unsupported(span, "class constructor body is missing"))?;
        let this_local = body
            .locals
            .iter()
            .enumerate()
            .find_map(|(index, local)| {
                local
                    .name
                    .and_then(|name| (name == this_symbol).then_some(index))
            })
            .and_then(|index| u32::try_from(index).ok())
            .map(smelt_hir::LocalId)
            .ok_or_else(|| SmeltError::unsupported(span, "class constructor receiver is missing"))?;
        let none = self.ctx.krate.types.intern(Type::None);
        let mut inserted = Vec::new();
        for (order, (method, kind, target_name)) in methods.into_iter().enumerate() {
            let receiver = body.push_expr(Expr {
                kind: ExprKind::Local(this_local),
                ty: class_ty,
                span,
            });
            let call = body.push_expr(Expr {
                kind: ExprKind::Method {
                    receiver,
                    method,
                    args: Vec::new(),
                },
                ty: none,
                span,
            });
            inserted.push((
                order,
                kind,
                target_name,
                body.push_stmt(smelt_hir::Stmt::Expr(call)),
            ));
        }
        let root_index = usize::try_from(body.root.0).unwrap_or(usize::MAX);
        let original_len = body
            .blocks
            .get(root_index)
            .map_or(0, |root| root.stmts.len().saturating_sub(inserted.len()));
        let original_stmts = body
            .blocks
            .get(root_index)
            .and_then(|root| root.stmts.get(..original_len))
            .map_or_else(Vec::new, <[smelt_hir::StmtId]>::to_vec);
        let mut placed = inserted
            .into_iter()
            .map(|(order, kind, target_name, statement)| {
                let position = if matches!(kind.as_deref(), Some("field" | "accessor")) {
                    target_name
                        .as_deref()
                        .map(materialized_member_name)
                        .and_then(|name| {
                            let symbol = self.ctx.krate.symbols.intern(name);
                            original_stmts.iter().position(|statement_id| {
                                let statement_index =
                                    usize::try_from(statement_id.0).unwrap_or(usize::MAX);
                                let Some(smelt_hir::Stmt::Assign { target, .. }) =
                                    body.stmts.get(statement_index)
                                else {
                                    return false;
                                };
                                let target_index =
                                    usize::try_from(target.0).unwrap_or(usize::MAX);
                                matches!(
                                    body.exprs.get(target_index),
                                    Some(Expr {
                                        kind: ExprKind::Field { field, .. },
                                        ..
                                    }) if *field == symbol
                                )
                            })
                        })
                        .map_or(original_len, |position| position.saturating_add(1))
                } else {
                    0
                };
                (position, order, statement)
            })
            .collect::<Vec<_>>();
        placed.sort_by_key(|(position, order, _)| (*position, *order));
        if let Some(root) = body.blocks.get_mut(root_index) {
            root.stmts.truncate(original_len);
            for (offset, (position, _, statement)) in placed.into_iter().enumerate() {
                root.stmts
                    .insert(position.saturating_add(offset), statement);
            }
        }
        Ok(())
    }

    /// Lowers a materialized primitive static member as an ordinary literal.
    pub(super) fn materialized_static_member(
        &self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(class_name) = &member.object else {
            return Ok(None);
        };
        let Some(class_item) = self.classes.get(class_name.name.as_str()).copied() else {
            return Ok(None);
        };
        let Item::Class(class) = self.item_ref(class_item) else {
            return Ok(None);
        };
        let Some(field) = class
            .static_fields
            .iter()
            .find(|field| {
                self.ctx.krate.symbols.get(field.name) == Some(member.property.name.as_str())
            })
            .cloned()
        else {
            return Ok(None);
        };
        let Some(value) = field.value else {
            return Err(SmeltError::native_specialization_adapter_required(
                self.span(member.span.start, member.span.end),
                "typescript.static-value",
                &format!(
                    "static member '{}.{}' is not a concrete primitive",
                    class_name.name, member.property.name
                ),
            ));
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(value),
            ty: field.ty,
            span: self.span(member.span.start, member.span.end),
        })))
    }

    /// Returns the materialized final class bound to `name`.
    pub(super) fn materialized_class(
        &self,
        name: &str,
    ) -> Option<&smelt_specialize::ClassDefinition> {
        self.specialization
            .as_ref()?
            .module
            .definitions
            .iter()
            .find(|definition| definition.binding_name == name)
            .and_then(|definition| match &definition.definition {
                smelt_specialize::Definition::Class(class) => Some(class),
                _ => None,
            })
    }

    /// Rejects unresolved adapter requirements before consuming a definition.
    pub(super) fn validate_specialization_adapters(&self, span: Span) -> Result<(), SmeltError> {
        let Some(adapter) = self
            .specialization
            .as_ref()
            .and_then(|data| data.required_adapters.first())
        else {
            return Ok(());
        };
        Err(SmeltError::native_specialization_adapter_required(
            span,
            &adapter.id,
            &adapter.reason,
        ))
    }

    /// Replaces source member declarations with the host-materialized class shape.
    pub(super) fn merge_materialized_class_members(
        &mut self,
        class: &smelt_specialize::ClassDefinition,
        fields: &mut Vec<Field>,
        span: Span,
    ) -> Vec<smelt_hir::StaticField> {
        for materialized in class.fields.iter().filter(|field| !field.is_static) {
            let name = self.intern_source_name(materialized_member_name(&materialized.name));
            let field = Field {
                name,
                ty: self.materialized_static_type(&materialized.ty),
                visibility: if materialized.is_private {
                    Visibility::Private
                } else {
                    Visibility::Public
                },
                optional: false,
                span: materialized
                    .span
                    .as_ref()
                    .map_or(span, |source| self.materialized_span(source)),
            };
            if let Some(existing) = fields.iter_mut().find(|existing| existing.name == name) {
                *existing = field;
            } else {
                fields.push(field);
            }
        }
        for descriptor in class
            .descriptors
            .iter()
            .filter(|descriptor| !descriptor.is_static)
        {
            let name = self.intern_source_name(materialized_member_name(&descriptor.name));
            if fields.iter().all(|field| field.name != name) {
                fields.push(Field {
                    name,
                    ty: self.materialized_static_type(&descriptor.read_type),
                    visibility: Visibility::Public,
                    optional: false,
                    span,
                });
            }
        }

        let mut static_fields = Vec::new();
        for materialized in class.fields.iter().filter(|field| field.is_static) {
            let value = class
                .static_values
                .get(&materialized.name)
                .copied()
                .or(materialized.default);
            let value_type = value
                .and_then(|value| self.materialized_value(value))
                .map(|node| node.ty.clone());
            let ty = value_type.as_ref().unwrap_or(&materialized.ty);
            static_fields.push(smelt_hir::StaticField {
                name: self.intern_source_name(materialized_member_name(&materialized.name)),
                ty: self.materialized_static_type(ty),
                visibility: if materialized.is_private {
                    Visibility::Private
                } else {
                    Visibility::Public
                },
                value: value.and_then(|value| self.materialized_literal(value)),
                span: materialized
                    .span
                    .as_ref()
                    .map_or(span, |source| self.materialized_span(source)),
            });
        }
        for (name, value) in &class.static_values {
            if static_fields
                .iter()
                .any(|field| self.ctx.krate.symbols.get(field.name) == Some(name))
            {
                continue;
            }
            let Some(node) = self.materialized_value(*value).cloned() else {
                continue;
            };
            static_fields.push(smelt_hir::StaticField {
                name: self.intern_source_name(name),
                ty: self.materialized_static_type(&node.ty),
                visibility: Visibility::Public,
                value: self.materialized_literal(*value),
                span,
            });
        }
        static_fields
    }

    /// Maps materialized instance descriptors back to source getter/setter items.
    ///
    /// Standard auto-accessors without explicit source methods remain ordinary
    /// field storage. A descriptor enters HIR only when at least one concrete
    /// accessor body was lowered from the TypeScript class declaration.
    pub(super) fn merge_materialized_class_descriptors(
        &mut self,
        class: &smelt_specialize::ClassDefinition,
        accessors: &[(smelt_hir::Symbol, MethodDefinitionKind, smelt_hir::ItemId)],
        _span: Span,
    ) -> Vec<smelt_hir::Descriptor> {
        class
            .descriptors
            .iter()
            .filter(|descriptor| !descriptor.is_static)
            .filter_map(|descriptor| {
                let name = self.intern_source_name(materialized_member_name(&descriptor.name));
                let getter = accessors
                    .iter()
                    .find(|(candidate, kind, _)| {
                        *candidate == name && *kind == MethodDefinitionKind::Get
                    })
                    .map(|(_, _, item)| *item);
                let setter = accessors
                    .iter()
                    .find(|(candidate, kind, _)| {
                        *candidate == name && *kind == MethodDefinitionKind::Set
                    })
                    .map(|(_, _, item)| *item);
                (getter.is_some() || setter.is_some()).then(|| smelt_hir::Descriptor {
                    name,
                    read_ty: self.materialized_static_type(&descriptor.read_type),
                    write_ty: descriptor
                        .write_type
                        .as_ref()
                        .map(|ty| self.materialized_static_type(ty)),
                    getter,
                    setter,
                    data_descriptor: descriptor.data_descriptor,
                    is_static: false,
                    value_fields: Vec::new(),
                })
            })
            .collect()
    }

    /// Converts one manifest type without routing concrete shapes through unknown.
    fn materialized_static_type(&mut self, ty: &smelt_specialize::StaticType) -> smelt_hir::TypeId {
        let lowered = match ty {
            smelt_specialize::StaticType::Null => Type::None,
            smelt_specialize::StaticType::Bool => Type::Bool,
            smelt_specialize::StaticType::Int | smelt_specialize::StaticType::Float => Type::Float,
            smelt_specialize::StaticType::String => Type::String,
            smelt_specialize::StaticType::Bytes => {
                Type::List(self.ctx.krate.types.intern(Type::Float))
            }
            smelt_specialize::StaticType::List(item) | smelt_specialize::StaticType::Set(item) => {
                let item = self.materialized_static_type(item);
                if matches!(ty, smelt_specialize::StaticType::Set(_)) {
                    Type::Set(item)
                } else {
                    Type::List(item)
                }
            }
            smelt_specialize::StaticType::Tuple(items) => Type::Tuple(
                items
                    .iter()
                    .map(|item| self.materialized_static_type(item))
                    .collect(),
            ),
            smelt_specialize::StaticType::Dict(key, value) => Type::Dict(
                self.materialized_static_type(key),
                self.materialized_static_type(value),
            ),
            smelt_specialize::StaticType::Named(name) => Type::Class {
                name: self.intern_type_name(name.rsplit('.').next().unwrap_or(name)),
                args: Vec::new(),
            },
            smelt_specialize::StaticType::Function(signature) => Type::Function(FunctionType {
                params: signature
                    .parameters
                    .iter()
                    .map(|parameter| self.materialized_static_type(&parameter.ty))
                    .collect(),
                rest: signature.parameters.iter().position(|parameter| {
                    matches!(
                        parameter.kind,
                        smelt_specialize::ParameterKind::VariadicPositional
                    )
                }),
                required_params: None,
                mutable_params: Vec::new(),
                return_ty: self.materialized_static_type(&signature.return_type),
                is_async: signature.is_async,
                may_throw: signature.throws,
            }),
            smelt_specialize::StaticType::DynamicMetadata => Type::Unknown,
        };
        self.ctx.krate.types.intern(lowered)
    }

    /// Converts a primitive graph node to a HIR literal.
    fn materialized_literal(&self, value: smelt_specialize::ValueId) -> Option<Literal> {
        match &self.materialized_value(value)?.value {
            smelt_specialize::GraphValueKind::Null => Some(Literal::None),
            smelt_specialize::GraphValueKind::Bool(value) => Some(Literal::Bool(*value)),
            smelt_specialize::GraphValueKind::Int(value) => {
                value.parse::<f64>().ok().map(Literal::Float)
            }
            smelt_specialize::GraphValueKind::Float(value) => Some(Literal::Float(*value)),
            smelt_specialize::GraphValueKind::String(value) => Some(Literal::String(value.clone())),
            _ => None,
        }
    }

    /// Returns one graph node by stable identity.
    fn materialized_value(
        &self,
        value: smelt_specialize::ValueId,
    ) -> Option<&smelt_specialize::GraphValue> {
        self.specialization
            .as_ref()?
            .values
            .iter()
            .find(|node| node.id == value)
    }

    /// Maps a manifest span into the current source file.
    fn materialized_span(&self, span: &smelt_specialize::SourceSpan) -> Span {
        Span::new(self.file_id, span.start, span.end)
    }
}

/// Converts standard-decorator private names to the frontend's field spelling.
pub(super) fn materialized_member_name(name: &str) -> &str {
    name.strip_prefix('#').unwrap_or(name)
}
