//! TypeScript specialization-manifest lookup and class-member materialization.

use std::path::Path;

use super::{
    Body, Expr, ExprKind, Expression, Field, FunctionType, Item, Literal, ModuleBuilder,
    SmeltError, Span, SpecializationData, Type, Visibility,
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
            static_fields.push(smelt_hir::StaticField {
                name: self.intern_source_name(materialized_member_name(&materialized.name)),
                ty: self.materialized_static_type(&materialized.ty),
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
fn materialized_member_name(name: &str) -> &str {
    name.strip_prefix('#').unwrap_or(name)
}
