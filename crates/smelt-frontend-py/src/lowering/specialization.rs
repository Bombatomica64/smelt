/// Returns the manifest module record corresponding to `path`.
pub(crate) fn specialization_for_path(
    path: &str,
    manifest: Option<&smelt_specialize::SpecializationManifest>,
) -> Option<SpecializationData> {
    let specialization = manifest?;
    if specialization.language != smelt_specialize::HostLanguage::Python {
        return None;
    }
    let module = specialization
        .modules
        .iter()
        .find(|module| specialization_paths_match(path, &module.path))
        .or_else(|| {
            (path.is_empty() && specialization.modules.len() == 1)
                .then(|| specialization.modules.first())
                .flatten()
        })
        .cloned()?;
    let replay_effects = specialization
        .modules
        .first()
        .is_some_and(|first| first.path == module.path);
    let effects = if replay_effects {
        specialization.effects.clone()
    } else {
        Vec::new()
    };
    Some(SpecializationData {
        module,
        values: specialization.values.nodes.clone(),
        required_adapters: specialization.required_adapters.clone(),
        effects,
    })
}

/// Compares frontend and guest paths after canonicalization when possible.
fn specialization_paths_match(frontend: &str, materialized: &str) -> bool {
    if frontend == materialized {
        return true;
    }
    let frontend_path = Path::new(frontend);
    let materialized_path = Path::new(materialized);
    match (
        frontend_path.canonicalize(),
        materialized_path.canonicalize(),
    ) {
        (Ok(frontend_canonical), Ok(materialized_canonical)) => {
            frontend_canonical == materialized_canonical
        }
        _ => false,
    }
}

/// Materialized descriptor callable role for one source method.
enum DescriptorCallableRole {
    /// Getter for the named owner member.
    Getter(String),
    /// Setter for the named owner member.
    Setter(String),
}

/// Source owner receiving methods produced by definition-time execution.
struct MaterializedClassTarget<'name> {
    /// Final source class name.
    name: &'name str,
    /// Interned class symbol.
    symbol: Symbol,
    /// Concrete HIR class type.
    ty: TypeId,
    /// Class source span.
    span: Span,
}

impl ModuleBuilder<'_> {
    /// Replays deterministic output captured during definition-time execution.
    fn replay_specialization_output(&mut self, body: &mut Body) {
        let effects = self
            .specialization
            .as_ref()
            .map(|specialization| specialization.effects.clone())
            .unwrap_or_default();
        let span = Span::new(self.file_id, 0, 0);
        let none_ty = self.intern_type(Type::None);
        let string_ty = self.intern_type(Type::String);
        for effect in effects {
            let smelt_specialize::EffectReplay::Output { stderr, text } = effect else {
                continue;
            };
            let symbol = if stderr {
                smelt_hir::CONSOLE_ERROR_WRITE_SYMBOL
            } else {
                smelt_hir::CONSOLE_WRITE_SYMBOL
            };
            let item = self.ensure_specialization_output_item(symbol, span);
            let callee = body.push_expr(HirExpr {
                kind: ExprKind::Item(item),
                ty: none_ty,
                span,
            });
            let value = body.push_expr(HirExpr {
                kind: ExprKind::Literal(Literal::String(text)),
                ty: string_ty,
                span,
            });
            let call = body.push_expr(HirExpr {
                kind: ExprKind::Call {
                    callee,
                    args: vec![value],
                },
                ty: none_ty,
                span,
            });
            body.push_stmt_to_block(body.root, HirStmt::Expr(call));
        }
    }

    /// Ensures one exact output replay builtin exists in HIR.
    fn ensure_specialization_output_item(&mut self, name: &str, span: Span) -> ItemId {
        if let Some(item) = self.items.get(name) {
            return *item;
        }
        let symbol = self.intern_name(name);
        let none_ty = self.intern_type(Type::None);
        let item = self.ctx.krate.push_item(Item::Function(Function {
            name: symbol,
            span,
            // Specialized synthetic function carries no type params.
            type_params: Vec::new(),
            params: Vec::new(),
            rest: None,
            required_params: None,
            return_ty: none_ty,
            is_async: false,
            is_test: false,
            body: None,
            owner: FunctionOwner::Module,
        }));
        self.items.insert(name.to_owned(), item);
        item
    }

    /// Lifts source-defined methods installed by a metaclass or class decorator.
    fn lower_materialized_class_methods(
        &mut self,
        target: &MaterializedClassTarget<'_>,
        materialized: &smelt_specialize::ClassDefinition,
        module: &ModModule,
    ) -> Result<Vec<ItemId>, SmeltError> {
        let mut generated = Vec::new();
        for method in &materialized.methods {
            let existing = self
                .class_methods
                .get(target.name)
                .and_then(|methods| methods.get(&method.name))
                .copied();
            if existing
                .is_some_and(|item| self.materialized_method_matches_item(item, &method.callable))
            {
                continue;
            }
            let source = find_specialized_function(module, &method.callable).ok_or_else(|| {
                SmeltError::native_specialization_adapter_required(
                    target.span,
                    "python.generated-method",
                    &format!(
                        "materialized method '{}' has no transpiled source function",
                        method.callable.qualified_name
                    ),
                )
            })?;
            if let Some(original) = existing {
                let hidden = self.intern_name(&format!("__smelt_original_{}", method.name));
                self.rename_function_item(original, hidden)?;
            }
            let saved_aliases = self.install_materialized_captures(
                &method.callable.captures,
                self.span(source.range),
            )?;
            let lowered = self.class_method(target.name, target.symbol, target.ty, source);
            self.restore_capture_aliases(saved_aliases);
            let item = lowered?;
            let name = self.intern_name(&method.name);
            self.rename_function_item(item, name)?;
            self.validate_materialized_method_signature(
                item,
                &method.signature,
                target.span,
                &method.name,
            )?;
            self.class_methods
                .entry(target.name.to_owned())
                .or_default()
                .insert(method.name.clone(), item);
            generated.push(item);
        }
        Ok(generated)
    }

    /// Returns whether a final host method still points at its source class method.
    fn materialized_method_matches_item(
        &self,
        item: ItemId,
        callable: &smelt_specialize::CallableProvenance,
    ) -> bool {
        let Some(Item::Function(function)) = self
            .ctx
            .krate
            .items
            .get(usize::try_from(item.0).unwrap_or(usize::MAX))
        else {
            return false;
        };
        (function.span.start..function.span.end).contains(&callable.span.start)
    }

    /// Validates a generated instance method while treating its receiver as the owner class.
    fn validate_materialized_method_signature(
        &mut self,
        item: ItemId,
        signature: &smelt_specialize::FunctionSignature,
        span: Span,
        name: &str,
    ) -> Result<(), SmeltError> {
        let index = usize::try_from(item.0).unwrap_or(usize::MAX);
        let Some(Item::Function(function)) = self.ctx.krate.items.get(index).cloned() else {
            return Err(SmeltError::specialization_type_mismatch(
                span,
                name,
                "materialized method did not lower to a function",
            ));
        };
        if function.params.len() != signature.parameters.len() {
            return Err(SmeltError::specialization_type_mismatch(
                span,
                name,
                "materialized method parameter count differs from source",
            ));
        }
        for (source, materialized) in function
            .params
            .iter()
            .skip(1)
            .zip(signature.parameters.iter().skip(1))
        {
            let expected = self.materialized_parameter_type(materialized);
            if source.ty != expected {
                return Err(SmeltError::specialization_type_mismatch(
                    span,
                    name,
                    format!("parameter '{}' changed concrete type", materialized.name),
                ));
            }
        }
        let mut expected_return = self.materialized_static_type(&signature.return_type);
        if signature.is_async {
            expected_return = self.future_type(expected_return);
        }
        if function.return_ty != expected_return || function.is_async != signature.is_async {
            return Err(SmeltError::specialization_type_mismatch(
                span,
                name,
                "materialized method return or async shape differs from source",
            ));
        }
        Ok(())
    }

    /// Returns a cloned materialized class definition by final binding name.
    fn materialized_class_definition(
        &self,
        name: &str,
    ) -> Option<&smelt_specialize::ClassDefinition> {
        self.specialization
            .as_ref()?
            .module
            .definitions
            .iter()
            .find_map(|record| {
                (record.binding_name == name)
                    .then_some(&record.definition)
                    .and_then(|payload| match payload {
                        smelt_specialize::Definition::Class(class) => Some(class),
                        smelt_specialize::Definition::Function(_)
                        | smelt_specialize::Definition::Value { .. } => None,
                    })
            })
    }

    /// Merges metaclass/generated concrete fields into the source class shape.
    fn merge_materialized_class_fields(
        &mut self,
        class_name: &str,
        materialized: &smelt_specialize::ClassDefinition,
        fields: &mut Vec<Field>,
        span: Span,
    ) {
        for field in materialized.fields.iter().filter(|field| !field.is_static) {
            let name = self.intern_name(&field.name);
            let ty = self.materialized_static_type(&field.ty);
            // Keep the optional flag in sync with the interned type: a
            // materialized field whose type is `Type::Optional` is an optional
            // data field, matching how the ordinary `AnnAssign` path records
            // `Optional[T]` / `T | None` fields.
            let optional = matches!(self.ctx.krate.types.get(ty), Some(Type::Optional(_)));
            let lowered = Field {
                name,
                ty,
                visibility: if field.is_private {
                    Visibility::Private
                } else {
                    Visibility::Public
                },
                optional,
                span,
            };
            if let Some(existing) = fields.iter_mut().find(|existing| existing.name == name) {
                *existing = lowered.clone();
            } else {
                fields.push(lowered.clone());
            }
            self.class_fields
                .entry(class_name.to_owned())
                .or_default()
                .retain(|existing| existing.name != name);
            self.class_fields
                .entry(class_name.to_owned())
                .or_default()
                .push(lowered);
        }
    }

    /// Lowers manifest descriptor metadata and concrete primitive state.
    fn lower_materialized_descriptors(
        &mut self,
        materialized: &smelt_specialize::ClassDefinition,
        span: Span,
    ) -> Result<Vec<smelt_hir::Descriptor>, SmeltError> {
        materialized
            .descriptors
            .iter()
            .map(|descriptor| self.lower_materialized_descriptor(descriptor, span))
            .collect()
    }

    /// Lowers one materialized descriptor.
    fn lower_materialized_descriptor(
        &mut self,
        descriptor: &smelt_specialize::DescriptorDefinition,
        span: Span,
    ) -> Result<smelt_hir::Descriptor, SmeltError> {
        let getter = descriptor
            .getter
            .as_ref()
            .map(|provenance| self.source_item_for_provenance(provenance, span))
            .transpose()?;
        let setter = descriptor
            .setter
            .as_ref()
            .map(|provenance| self.source_item_for_provenance(provenance, span))
            .transpose()?;
        let value_fields = self.materialized_descriptor_value_fields(descriptor.value, span)?;
        self.merge_materialized_descriptor_state_class(descriptor.value, &value_fields, span)?;
        Ok(smelt_hir::Descriptor {
            name: self.intern_name(&descriptor.name),
            read_ty: self.materialized_static_type(&descriptor.read_type),
            write_ty: descriptor
                .write_type
                .as_ref()
                .map(|ty| self.materialized_static_type(ty)),
            getter,
            setter,
            data_descriptor: descriptor.data_descriptor,
            is_static: descriptor.is_static,
            value_fields,
        })
    }

    /// Resolves callable provenance to the nearest already-lowered source item.
    fn source_item_for_provenance(
        &self,
        provenance: &smelt_specialize::CallableProvenance,
        span: Span,
    ) -> Result<ItemId, SmeltError> {
        let expected_name = provenance
            .qualified_name
            .rsplit('.')
            .next()
            .unwrap_or(&provenance.qualified_name);
        self.source_module_items(&provenance.module, Some(&provenance.span.file))
            .into_iter()
            .filter_map(|item_id| {
                let item = self.item_ref(item_id);
                let Item::Function(function) = item else {
                    return None;
                };
                let name = self.ctx.krate.symbols.get(function.name)?;
                let owner_name = match function.owner {
                    FunctionOwner::ClassMethod { method, .. }
                    | FunctionOwner::ClassStaticMethod { method, .. } => {
                        self.ctx.krate.symbols.get(method)
                    }
                    FunctionOwner::Module | FunctionOwner::Constructor { .. } => None,
                };
                (descriptor_callable_name_matches(name, expected_name)
                    || owner_name == Some(expected_name))
                .then_some((function.span.start.abs_diff(provenance.span.start), item_id))
            })
            .min_by_key(|(distance, _)| *distance)
            .map(|(_, item_id)| item_id)
            .ok_or_else(|| {
                SmeltError::native_specialization_adapter_required(
                    span,
                    "python.descriptor-callable",
                    &format!(
                        "descriptor callable '{}' has no lowered source method",
                        provenance.qualified_name
                    ),
                )
            })
    }

    /// Extracts concrete primitive fields from a descriptor value graph node.
    fn materialized_descriptor_value_fields(
        &mut self,
        value: smelt_specialize::ValueId,
        span: Span,
    ) -> Result<Vec<smelt_hir::DescriptorValueField>, SmeltError> {
        let node = self.materialized_value(value, span)?.clone();
        let smelt_specialize::GraphValueKind::Instance { fields, .. } = node.value else {
            return Ok(Vec::new());
        };
        fields
            .into_iter()
            .map(|(name, field_value)| {
                let field_node = self.materialized_value(field_value, span)?.clone();
                let (literal, ty) = Self::materialized_primitive(&field_node.value, span)?;
                Ok(smelt_hir::DescriptorValueField {
                    name: self.intern_name(&name),
                    value: literal,
                    ty: self.intern_type(ty),
                })
            })
            .collect()
    }

    /// Adds concrete descriptor instance state to its source-defined class layout.
    fn merge_materialized_descriptor_state_class(
        &mut self,
        value: smelt_specialize::ValueId,
        value_fields: &[smelt_hir::DescriptorValueField],
        span: Span,
    ) -> Result<(), SmeltError> {
        if value_fields.is_empty() {
            return Ok(());
        }
        let node = self.materialized_value(value, span)?;
        let smelt_specialize::GraphValueKind::Instance {
            class: class_name, ..
        } = &node.value
        else {
            return Ok(());
        };
        let qualified_class = class_name.clone();
        let Some((source_module, source_name)) = qualified_class.rsplit_once('.') else {
            return Err(SmeltError::native_specialization_adapter_required(
                span,
                "python.descriptor-class",
                &format!(
                    "descriptor state class '{qualified_class}' has no transpiled source definition"
                ),
            ));
        };
        let resolved_class = self
            .source_module_binding(source_module, source_name)
            .filter(|item_id| matches!(self.item_ref(*item_id), Item::Class(_)));
        let Some(class_item) = resolved_class else {
            return Err(SmeltError::native_specialization_adapter_required(
                span,
                "python.descriptor-class",
                &format!(
                    "descriptor state class '{qualified_class}' has no transpiled source definition"
                ),
            ));
        };
        let class_index = usize::try_from(class_item.0).map_err(|_invalid_index| {
            SmeltError::unsupported(span, "resolved descriptor class item index is invalid")
        })?;
        let fields = value_fields
            .iter()
            .map(|field| Field {
                name: field.name,
                ty: field.ty,
                visibility: Visibility::Private,
                optional: false,
                span,
            })
            .collect::<Vec<_>>();
        let Some(Item::Class(descriptor_class)) = self.ctx.krate.items.get_mut(class_index) else {
            return Err(SmeltError::unsupported(
                span,
                "resolved descriptor class item is missing",
            ));
        };
        for field in &fields {
            if let Some(existing) = descriptor_class
                .fields
                .iter_mut()
                .find(|existing| existing.name == field.name)
            {
                *existing = field.clone();
            } else {
                descriptor_class.fields.push(field.clone());
            }
        }
        if self.source_module_is_current(source_module, None) {
            let known_fields = self.class_fields.entry(source_name.to_owned()).or_default();
            for field in fields {
                if let Some(existing) = known_fields
                    .iter_mut()
                    .find(|existing| existing.name == field.name)
                {
                    *existing = field;
                } else {
                    known_fields.push(field);
                }
            }
        }
        Ok(())
    }

    /// Converts one graph payload to a concrete HIR primitive.
    fn materialized_primitive(
        payload: &smelt_specialize::GraphValueKind,
        span: Span,
    ) -> Result<(Literal, Type), SmeltError> {
        match payload {
            smelt_specialize::GraphValueKind::Null => Ok((Literal::None, Type::None)),
            smelt_specialize::GraphValueKind::Bool(boolean) => {
                Ok((Literal::Bool(*boolean), Type::Bool))
            }
            smelt_specialize::GraphValueKind::Int(integer) => integer
                .parse::<i64>()
                .map(|parsed| (Literal::Int(parsed), Type::Int))
                .map_err(|error| {
                    SmeltError::unsupported(
                        span,
                        format!("materialized descriptor integer is too large: {error}"),
                    )
                }),
            smelt_specialize::GraphValueKind::Float(number) => {
                Ok((Literal::Float(*number), Type::Float))
            }
            smelt_specialize::GraphValueKind::String(text) => {
                Ok((Literal::String(text.clone()), Type::String))
            }
            smelt_specialize::GraphValueKind::Bytes(_)
            | smelt_specialize::GraphValueKind::Tuple(_)
            | smelt_specialize::GraphValueKind::List(_)
            | smelt_specialize::GraphValueKind::Set(_)
            | smelt_specialize::GraphValueKind::Dict(_)
            | smelt_specialize::GraphValueKind::Enum { .. }
            | smelt_specialize::GraphValueKind::Instance { .. }
            | smelt_specialize::GraphValueKind::ClassRef { .. }
            | smelt_specialize::GraphValueKind::FunctionRef(_) => {
                Err(SmeltError::native_specialization_adapter_required(
                    span,
                    "python.descriptor-state",
                    "descriptor state contains a non-primitive field",
                ))
            }
        }
    }

    /// Renames property getter/setter methods to collision-free Rust methods.
    fn rename_materialized_descriptor_callable(
        &mut self,
        class_name: &str,
        function: &StmtFunctionDef,
        item: ItemId,
    ) -> Result<(), SmeltError> {
        let Some(role) = self.materialized_descriptor_callable_role(class_name, function) else {
            return Ok(());
        };
        let name = match role {
            DescriptorCallableRole::Getter(field) => format!("__smelt_get_{field}"),
            DescriptorCallableRole::Setter(field) => format!("__smelt_set_{field}"),
        };
        let symbol = self.intern_name(&name);
        self.rename_function_item(item, symbol)
    }

    /// Matches a source method span to a materialized descriptor callable.
    fn materialized_descriptor_callable_role(
        &self,
        class_name: &str,
        function: &StmtFunctionDef,
    ) -> Option<DescriptorCallableRole> {
        let class = self.materialized_class_definition(class_name)?;
        let start = function.range.start().to_u32();
        class.descriptors.iter().find_map(|descriptor| {
            if descriptor.name == function.name.as_str()
                && function
                    .decorator_list
                    .iter()
                    .any(|decorator| decorator_simple_name(decorator) == Some("property"))
            {
                return Some(DescriptorCallableRole::Getter(descriptor.name.clone()));
            }
            if descriptor.name == function.name.as_str()
                && function
                    .decorator_list
                    .iter()
                    .any(|decorator| decorator_simple_name(decorator) == Some("setter"))
            {
                return Some(DescriptorCallableRole::Setter(descriptor.name.clone()));
            }
            if descriptor
                .getter
                .as_ref()
                .is_some_and(|getter| getter.span.start == start)
            {
                Some(DescriptorCallableRole::Getter(descriptor.name.clone()))
            } else if descriptor
                .setter
                .as_ref()
                .is_some_and(|setter| setter.span.start == start)
            {
                Some(DescriptorCallableRole::Setter(descriptor.name.clone()))
            } else {
                None
            }
        })
    }

    /// Returns whether a decorated method is a manifest-backed descriptor callable.
    fn is_materialized_descriptor_callable(&self, function: &StmtFunctionDef) -> bool {
        let decorator_role = function.decorator_list.iter().any(|decorator| {
            matches!(
                decorator_simple_name(decorator),
                Some("property" | "setter")
            )
        });
        self.specialization.as_ref().is_some_and(|specialization| {
            specialization
                .module
                .definitions
                .iter()
                .filter_map(|definition| match &definition.definition {
                    smelt_specialize::Definition::Class(class) => Some(class),
                    smelt_specialize::Definition::Function(_)
                    | smelt_specialize::Definition::Value { .. } => None,
                })
                .flat_map(|class| &class.descriptors)
                .any(|descriptor| {
                    (decorator_role && descriptor.name == function.name.as_str())
                        || [descriptor.getter.as_ref(), descriptor.setter.as_ref()]
                            .into_iter()
                            .flatten()
                            .any(|callable| callable.span.start == function.range.start().to_u32())
                })
        })
    }

    /// Returns whether a local function is used only as a materialized
    /// definition-time decorator factory in this module.
    fn is_materialized_decorator_factory(
        &self,
        candidate: &StmtFunctionDef,
        module: &ModModule,
    ) -> bool {
        self.specialization.is_some()
            && module.body.iter().any(|statement| match statement {
                Stmt::FunctionDef(function) => function.decorator_list.iter().any(|decorator| {
                    decorator_simple_name(decorator) == Some(candidate.name.as_str())
                }),
                Stmt::ClassDef(class) => class.decorator_list.iter().any(|decorator| {
                    decorator_simple_name(decorator) == Some(candidate.name.as_str())
                }),
                _ => false,
            })
    }

    /// Returns whether a source class is used only as a build-time metaclass.
    fn is_materialized_metaclass_definition(
        &self,
        candidate: &StmtClassDef,
        module: &ModModule,
    ) -> bool {
        self.specialization.is_some()
            && module.body.iter().any(|statement| {
                let Stmt::ClassDef(class) = statement else {
                    return false;
                };
                class.arguments.as_deref().is_some_and(|arguments| {
                    arguments.keywords.iter().any(|keyword| {
                        keyword.arg.as_ref().map(|name| name.as_str()) == Some("metaclass")
                            && expr_simple_name(&keyword.value) == Some(candidate.name.as_str())
                    })
                })
            })
    }

    /// Finds the first source definition that cannot lower without a manifest.
    fn required_specialization_definition<'module>(
        &self,
        module: &'module ModModule,
    ) -> Option<(Span, &'module str)> {
        module.body.iter().find_map(|statement| match statement {
            Stmt::FunctionDef(function) => (!function.decorator_list.is_empty()
                && !self.has_frontend_only_test_decorators(function))
            .then(|| (self.span(function.range), function.name.as_str())),
            Stmt::ClassDef(class) => Self::class_requires_specialization(class)
                .then(|| (self.span(class.range), class.name.as_str())),
            _ => None,
        })
    }

    /// Returns whether class construction invokes unsupported definition hooks.
    fn class_requires_specialization(class: &StmtClassDef) -> bool {
        let unknown_decorator = class.decorator_list.iter().any(|decorator| {
            !matches!(
                decorator_simple_name(decorator),
                Some("dataclass" | "dataclasses.dataclass")
            )
        });
        let custom_metaclass = class.arguments.as_deref().is_some_and(|arguments| {
            arguments.keywords.iter().any(|keyword| {
                keyword.arg.as_ref().map(|name| name.as_str()) == Some("metaclass")
                    && !matches!(
                        expr_simple_name(&keyword.value),
                        Some("ABCMeta" | "abc.ABCMeta")
                    )
            })
        });
        let definition_hook = class.body.iter().any(|statement| {
            matches!(
                statement,
                Stmt::FunctionDef(function)
                    if matches!(function.name.as_str(), "__set_name__" | "__init_subclass__")
            )
        });
        unknown_decorator || custom_metaclass || definition_hook
    }

    /// Lowers one top-level function using its materialized final binding.
    fn specialized_function_defs(
        &mut self,
        func: &StmtFunctionDef,
        module: &ModModule,
    ) -> Result<Vec<ItemId>, SmeltError> {
        if func.decorator_list.is_empty() || self.has_frontend_only_test_decorators(func) {
            return self.function_defs(func);
        }
        let name = func.name.as_str();
        let definition = self
            .specialization
            .as_ref()
            .and_then(|record| {
                record
                    .module
                    .definitions
                    .iter()
                    .find(|definition| definition.binding_name == name)
            })
            .cloned()
            .ok_or_else(|| SmeltError::specialization_required(self.span(func.range), name))?;
        self.reject_required_native_adapter(func)?;
        match definition.definition {
            smelt_specialize::Definition::Function(function) => {
                self.lower_materialized_wrapper(func, module, &function)
            }
            smelt_specialize::Definition::Value { value } => {
                let item = self.materialized_const_binding(name, value, self.span(func.range))?;
                Ok(vec![item])
            }
            smelt_specialize::Definition::Class(_) => {
                Err(SmeltError::native_specialization_adapter_required(
                    self.span(func.range),
                    "python.function-to-class",
                    "function decorators producing classes require materialized class lowering",
                ))
            }
        }
    }

    /// Returns whether decorators are compile-only pytest controls.
    fn has_frontend_only_test_decorators(&self, func: &StmtFunctionDef) -> bool {
        self.pytest_mode
            && func.decorator_list.iter().all(|decorator| {
                matches!(
                    decorator_simple_name(decorator),
                    Some("fixture" | "parametrize" | "skip" | "skipif" | "xfail")
                )
            })
    }

    /// Rejects the first opaque adapter requirement before HIR mutation.
    fn reject_required_native_adapter(&self, func: &StmtFunctionDef) -> Result<(), SmeltError> {
        let Some(requirement) = self
            .specialization
            .as_ref()
            .and_then(|data| data.required_adapters.first())
        else {
            return Ok(());
        };
        Err(SmeltError::native_specialization_adapter_required(
            self.span(func.range),
            &requirement.id,
            &requirement.reason,
        ))
    }

    /// Lowers an original function plus its source-defined final wrapper.
    fn lower_materialized_wrapper(
        &mut self,
        original: &StmtFunctionDef,
        module: &ModModule,
        materialized: &smelt_specialize::FunctionDefinition,
    ) -> Result<Vec<ItemId>, SmeltError> {
        let original_item = self.function_def(original)?;
        if Self::provenance_matches_function(&materialized.callable, original) {
            return Ok(vec![original_item]);
        }
        let original_name = original.name.as_str();
        let final_symbol = self.intern_name(original_name);
        let hidden_symbol = self.intern_name(&format!("__smelt_original_{original_name}"));
        self.rename_function_item(original_item, hidden_symbol)?;

        let mut provenance_chain =
            Vec::with_capacity(materialized.wrapper_chain.len().saturating_add(1));
        provenance_chain.push(&materialized.callable);
        provenance_chain.extend(&materialized.wrapper_chain);
        if provenance_chain
            .last()
            .is_some_and(|provenance| Self::provenance_matches_function(provenance, original))
        {
            provenance_chain.pop();
        }
        let mut items = vec![original_item];
        let mut inner_item = original_item;
        let mut inner_provenance = materialized.wrapper_chain.last();
        for (index, provenance) in provenance_chain.into_iter().rev().enumerate() {
            let wrapper = find_specialized_function(module, provenance).ok_or_else(|| {
                SmeltError::native_specialization_adapter_required(
                    self.span(original.range),
                    "python.callable-provenance",
                    &format!(
                        "callable '{}' did not resolve to a source function",
                        provenance.qualified_name
                    ),
                )
            })?;
            let saved_aliases = self.install_materialized_wrapper_captures(
                &provenance.captures,
                inner_item,
                inner_provenance,
                self.span(wrapper.range),
            )?;
            let wrapper_result = self.function_def(wrapper);
            self.restore_capture_aliases(saved_aliases);
            let wrapper_item = wrapper_result?;
            let wrapper_symbol =
                self.intern_name(&format!("__smelt_wrapper_{original_name}_{index}"));
            self.rename_function_item(wrapper_item, wrapper_symbol)?;
            items.push(wrapper_item);
            inner_item = wrapper_item;
            inner_provenance = Some(provenance);
        }
        self.rename_function_item(inner_item, final_symbol)?;
        self.validate_materialized_signature(
            inner_item,
            &materialized.signature,
            self.span(original.range),
            original_name,
        )?;
        self.items.insert(original_name.to_owned(), inner_item);
        self.exports.insert(original_name.to_owned(), inner_item);
        Ok(items)
    }

    /// Installs captures while binding a wrapper's inner callable explicitly.
    fn install_materialized_wrapper_captures(
        &mut self,
        captures: &std::collections::BTreeMap<String, smelt_specialize::ValueId>,
        inner_item: ItemId,
        inner_provenance: Option<&smelt_specialize::CallableProvenance>,
        span: Span,
    ) -> Result<Vec<(String, Option<ItemId>)>, SmeltError> {
        let mut saved = Vec::new();
        for (name, value) in captures {
            let node = self.materialized_value(*value, span)?.clone();
            let is_inner_callable = matches!(
                &node.value,
                smelt_specialize::GraphValueKind::FunctionRef(provenance)
                    if inner_provenance.is_none_or(|inner| {
                        provenance.qualified_name == inner.qualified_name
                            && provenance.span == inner.span
                    })
            );
            let item = if is_inner_callable {
                inner_item
            } else {
                self.materialized_capture_item(name, *value, span)?
            };
            saved.push((name.clone(), self.items.insert(name.clone(), item)));
        }
        Ok(saved)
    }

    /// Validates host callable shape against the source-lowered HIR function.
    fn validate_materialized_signature(
        &mut self,
        item: ItemId,
        signature: &smelt_specialize::FunctionSignature,
        span: Span,
        name: &str,
    ) -> Result<(), SmeltError> {
        let index = usize::try_from(item.0).unwrap_or(usize::MAX);
        let Some(Item::Function(function)) = self.ctx.krate.items.get(index).cloned() else {
            return Err(SmeltError::specialization_type_mismatch(
                span,
                name,
                "final binding is not a function",
            ));
        };
        if function.params.len() != signature.parameters.len() {
            return Err(SmeltError::specialization_type_mismatch(
                span,
                name,
                format!(
                    "source has {} parameters but host binding has {}",
                    function.params.len(),
                    signature.parameters.len()
                ),
            ));
        }
        for (source, materialized) in function.params.iter().zip(&signature.parameters) {
            let expected = self.materialized_parameter_type(materialized);
            if source.ty != expected {
                return Err(SmeltError::specialization_type_mismatch(
                    span,
                    name,
                    format!("parameter '{}' changed concrete type", materialized.name),
                ));
            }
        }
        let mut expected_return = self.materialized_static_type(&signature.return_type);
        if signature.is_async {
            expected_return = self.future_type(expected_return);
        }
        if function.return_ty != expected_return || function.is_async != signature.is_async {
            return Err(SmeltError::specialization_type_mismatch(
                span,
                name,
                "return or async shape differs from strict source typing",
            ));
        }
        Ok(())
    }

    /// Converts one materialized parameter convention to its HIR ABI type.
    fn materialized_parameter_type(&mut self, parameter: &smelt_specialize::Parameter) -> TypeId {
        let value = self.materialized_static_type(&parameter.ty);
        match parameter.kind {
            smelt_specialize::ParameterKind::VariadicPositional => {
                self.intern_type(Type::List(value))
            }
            smelt_specialize::ParameterKind::VariadicKeyword => {
                let string = self.intern_type(Type::String);
                self.intern_type(Type::Dict(string, value))
            }
            smelt_specialize::ParameterKind::Positional
            | smelt_specialize::ParameterKind::PositionalOnly
            | smelt_specialize::ParameterKind::KeywordOnly => value,
        }
    }

    /// Converts the manifest's concrete static type algebra into HIR.
    fn materialized_static_type(&mut self, ty: &smelt_specialize::StaticType) -> TypeId {
        let hir = match ty {
            smelt_specialize::StaticType::Null => Type::None,
            smelt_specialize::StaticType::Bool => Type::Bool,
            smelt_specialize::StaticType::Int => Type::Int,
            smelt_specialize::StaticType::Float => Type::Float,
            smelt_specialize::StaticType::String => Type::String,
            smelt_specialize::StaticType::Bytes => Type::Class {
                name: self.intern_name("builtins.bytes"),
                args: Vec::new(),
            },
            smelt_specialize::StaticType::List(item) => {
                Type::List(self.materialized_static_type(item))
            }
            smelt_specialize::StaticType::Set(item) => {
                Type::Set(self.materialized_static_type(item))
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
                name: self.intern_name(name),
                args: Vec::new(),
            },
            smelt_specialize::StaticType::Function(signature) => {
                let params = signature
                    .parameters
                    .iter()
                    .map(|parameter| self.materialized_parameter_type(parameter))
                    .collect();
                let mut return_ty = self.materialized_static_type(&signature.return_type);
                if signature.is_async {
                    return_ty = self.future_type(return_ty);
                }
                Type::Function(FunctionType {
                    params,
                    rest: signature.parameters.iter().position(|parameter| {
                        matches!(
                            parameter.kind,
                            smelt_specialize::ParameterKind::VariadicPositional
                        )
                    }),
                    required_params: None,
                    mutable_params: Vec::new(),
                    return_ty,
                    is_async: signature.is_async,
                    may_throw: signature.throws,
                })
            }
            smelt_specialize::StaticType::DynamicMetadata => Type::Unknown,
        };
        self.intern_type(hir)
    }

    /// Returns whether provenance points at the original top-level function.
    fn provenance_matches_function(
        provenance: &smelt_specialize::CallableProvenance,
        function: &StmtFunctionDef,
    ) -> bool {
        provenance
            .qualified_name
            .rsplit('.')
            .next()
            .is_some_and(|name| name == function.name.as_str())
            && (function.range.start().to_u32()..function.range.end().to_u32())
                .contains(&provenance.span.start)
    }

    /// Renames one HIR function item without changing its body or call identity.
    fn rename_function_item(&mut self, item: ItemId, name: Symbol) -> Result<(), SmeltError> {
        let index = usize::try_from(item.0).unwrap_or(usize::MAX);
        let Some(Item::Function(function)) = self.ctx.krate.items.get_mut(index) else {
            return Err(SmeltError::unsupported(
                Span::new(self.file_id, 0, 0),
                "materialized callable did not lower to a function item",
            ));
        };
        function.name = name;
        if let FunctionOwner::ClassMethod { class, .. } = function.owner {
            function.owner = FunctionOwner::ClassMethod {
                class,
                method: name,
            };
        }
        Ok(())
    }

    /// Installs closure captures as temporary item aliases/constants.
    fn install_materialized_captures(
        &mut self,
        captures: &std::collections::BTreeMap<String, smelt_specialize::ValueId>,
        span: Span,
    ) -> Result<Vec<(String, Option<ItemId>)>, SmeltError> {
        let mut saved = Vec::new();
        for (name, value) in captures {
            let item = self.materialized_capture_item(name, *value, span)?;
            saved.push((name.clone(), self.items.insert(name.clone(), item)));
        }
        Ok(saved)
    }

    /// Restores item bindings shadowed while lowering a lifted callable.
    fn restore_capture_aliases(&mut self, saved: Vec<(String, Option<ItemId>)>) {
        for (name, previous) in saved {
            if let Some(item) = previous {
                self.items.insert(name, item);
            } else {
                self.items.remove(&name);
            }
        }
    }

    /// Resolves one capture to a source item or concrete constant.
    fn materialized_capture_item(
        &mut self,
        name: &str,
        value_id: smelt_specialize::ValueId,
        span: Span,
    ) -> Result<ItemId, SmeltError> {
        let node = self.materialized_value(value_id, span)?.clone();
        match node.value {
            smelt_specialize::GraphValueKind::FunctionRef(provenance) => {
                self.capture_source_item(&provenance.module, &provenance.qualified_name, span)
            }
            smelt_specialize::GraphValueKind::ClassRef {
                module,
                qualified_name,
            } => self.capture_source_item(&module, &qualified_name, span),
            _ => self.materialized_const_binding(name, value_id, span),
        }
    }

    /// Finds a source item referenced by qualified callable provenance.
    fn capture_source_item(
        &self,
        module: &str,
        qualified_name: &str,
        span: Span,
    ) -> Result<ItemId, SmeltError> {
        let source_name = qualified_name.rsplit('.').next().unwrap_or(qualified_name);
        let owner_name = qualified_name
            .rsplit_once('.')
            .and_then(|(owner, _)| owner.rsplit('.').next());
        self.source_module_items(module, None)
            .into_iter()
            .find(|item_id| match self.item_ref(*item_id) {
                Item::Function(function) => match function.owner {
                    FunctionOwner::Module => {
                        self.ctx.krate.symbols.get(function.name) == Some(source_name)
                    }
                    FunctionOwner::ClassMethod { class, method }
                    | FunctionOwner::ClassStaticMethod { class, method } => {
                        self.ctx.krate.symbols.get(method) == Some(source_name)
                            && owner_name.is_some_and(|owner| {
                                self.ctx.krate.symbols.get(class) == Some(owner)
                            })
                    }
                    FunctionOwner::Constructor { class } => {
                        source_name == "__init__"
                            && owner_name.is_some_and(|owner| {
                                self.ctx.krate.symbols.get(class) == Some(owner)
                            })
                    }
                },
                Item::Class(class) => {
                    self.ctx.krate.symbols.get(class.name) == Some(source_name)
                        && owner_name.is_none()
                }
                Item::Interface(_) | Item::TypeAlias(_) | Item::Const(_) => false,
            })
            .ok_or_else(|| {
                SmeltError::native_specialization_adapter_required(
                    span,
                    "python.capture-provenance",
                    &format!("captured callable '{qualified_name}' has no lowered source item"),
                )
            })
    }

    /// Resolves a top-level binding in one exact source module.
    fn source_module_binding(&self, module: &str, name: &str) -> Option<ItemId> {
        if self.source_module_is_current(module, None) {
            return self.items.get(name).copied();
        }
        self.module_namespaces
            .get(module)
            .or_else(|| self.ctx.module_namespaces.get(module))
            .and_then(|members| members.get(name))
            .copied()
    }

    /// Returns items owned by one exact source module.
    fn source_module_items(&self, module: &str, source_file: Option<&str>) -> Vec<ItemId> {
        if self.source_module_is_current(module, source_file) {
            let mut items = self.items.values().copied().collect::<Vec<_>>();
            items.extend(
                self.class_methods
                    .values()
                    .flat_map(|methods| methods.values().copied()),
            );
            items.extend(
                self.ctx
                    .krate
                    .items
                    .iter()
                    .enumerate()
                    .filter_map(|(index, item)| {
                        let Item::Function(function) = item else {
                            return None;
                        };
                        (function.span.file == self.file_id)
                            .then(|| u32::try_from(index).ok().map(ItemId))
                            .flatten()
                    }),
            );
            items.sort_by_key(|item| item.0);
            items.dedup();
            return items;
        }
        self.ctx
            .krate
            .modules
            .iter()
            .find(|candidate| {
                source_file
                    .is_some_and(|file| specialization_paths_match(file, &candidate.source.path))
                    || module_names_from_path(&candidate.source.path)
                        .iter()
                        .any(|name| name == module)
            })
            .map_or_else(Vec::new, |candidate| candidate.items.clone())
    }

    /// Returns whether provenance belongs to the module currently being lowered.
    fn source_module_is_current(&self, module: &str, source_file: Option<&str>) -> bool {
        source_file.is_some_and(|file| specialization_paths_match(file, &self.path))
            || module_names_from_path(&self.path)
                .iter()
                .any(|name| name == module)
    }

    /// Creates a HIR constant item from one concrete materialized value.
    fn materialized_const_binding(
        &mut self,
        name: &str,
        value_id: smelt_specialize::ValueId,
        span: Span,
    ) -> Result<ItemId, SmeltError> {
        let node = self.materialized_value(value_id, span)?.clone();
        let (literal, ty) = match node.value {
            smelt_specialize::GraphValueKind::Null => (Literal::None, Type::None),
            smelt_specialize::GraphValueKind::Bool(boolean) => (Literal::Bool(boolean), Type::Bool),
            smelt_specialize::GraphValueKind::Int(integer) => (
                Literal::Int(integer.parse::<i64>().map_err(|error| {
                    SmeltError::unsupported(
                        span,
                        format!("materialized integer is too large: {error}"),
                    )
                })?),
                Type::Int,
            ),
            smelt_specialize::GraphValueKind::Float(number) => {
                (Literal::Float(number), Type::Float)
            }
            smelt_specialize::GraphValueKind::String(text) => (Literal::String(text), Type::String),
            _ => {
                return Err(SmeltError::native_specialization_adapter_required(
                    span,
                    "python.capture-value",
                    "this concrete capture shape is not yet representable as a HIR constant",
                ));
            }
        };
        let type_id = self.intern_type(ty);
        let mut body = Body::new(None, span);
        let expr = body.push_expr(HirExpr {
            kind: ExprKind::Literal(literal),
            ty: type_id,
            span,
        });
        let body_id = self.ctx.krate.push_body(body);
        let symbol = self.intern_name(name);
        let item = self.ctx.krate.push_item(Item::Const(ConstItem {
            name: symbol,
            ty: type_id,
            value: expr,
            body: body_id,
            span,
        }));
        self.items.insert(name.to_owned(), item);
        self.exports.insert(name.to_owned(), item);
        Ok(item)
    }

    /// Returns one graph value or a precise malformed-manifest diagnostic.
    fn materialized_value(
        &self,
        value: smelt_specialize::ValueId,
        span: Span,
    ) -> Result<&smelt_specialize::GraphValue, SmeltError> {
        self.specialization
            .as_ref()
            .and_then(|data| data.values.iter().find(|node| node.id == value))
            .ok_or_else(|| {
                SmeltError::unsupported(
                    span,
                    format!(
                        "materialized value {} is missing from the manifest graph",
                        value.0
                    ),
                )
            })
    }
}

/// Finds a nested source function matching callable provenance.
fn find_specialized_function<'module>(
    module: &'module ModModule,
    provenance: &smelt_specialize::CallableProvenance,
) -> Option<&'module StmtFunctionDef> {
    use ruff_python_ast::visitor::Visitor as _;

    let name = provenance
        .qualified_name
        .rsplit('.')
        .next()
        .unwrap_or(&provenance.qualified_name);
    let mut finder = SpecializedFunctionFinder {
        name,
        start: provenance.span.start,
        best_distance: u32::MAX,
        found: None,
    };
    for statement in &module.body {
        finder.visit_stmt(statement);
    }
    finder.found
}

/// Matches a callable name only at a full dotted qualified-name segment.
fn qualified_name_segment_matches(candidate: &str, expected: &str) -> bool {
    candidate == expected
        || candidate
            .strip_suffix(expected)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

/// Matches exact source names and the two collision-free descriptor helper names.
fn descriptor_callable_name_matches(candidate: &str, expected: &str) -> bool {
    qualified_name_segment_matches(candidate, expected)
        || candidate
            .strip_prefix("__smelt_get_")
            .is_some_and(|name| name == expected)
        || candidate
            .strip_prefix("__smelt_set_")
            .is_some_and(|name| name == expected)
}

/// Source-order callable finder using name plus nearest byte offset.
struct SpecializedFunctionFinder<'name, 'module> {
    /// Expected final source function name.
    name: &'name str,
    /// Guest-reported byte start.
    start: u32,
    /// Nearest candidate distance.
    best_distance: u32,
    /// Best source function.
    found: Option<&'module StmtFunctionDef>,
}

impl<'module> ruff_python_ast::visitor::Visitor<'module>
    for SpecializedFunctionFinder<'_, 'module>
{
    /// Records matching functions and continues through nested definitions.
    fn visit_stmt(&mut self, statement: &'module Stmt) {
        if let Stmt::FunctionDef(function) = statement
            && function.name.as_str() == self.name
        {
            let candidate = function.range.start().to_u32();
            let distance = candidate.abs_diff(self.start);
            if distance < self.best_distance {
                self.best_distance = distance;
                self.found = Some(function);
            }
        }
        ruff_python_ast::visitor::walk_stmt(self, statement);
    }
}

#[cfg(test)]
mod provenance_name_tests {
    use super::{descriptor_callable_name_matches, qualified_name_segment_matches};

    #[test]
    fn callable_suffixes_require_a_full_segment() {
        assert!(qualified_name_segment_matches("Owner.helper", "helper"));
        assert!(!qualified_name_segment_matches("my_helper", "helper"));
        assert!(!qualified_name_segment_matches("Owner.my_helper", "helper"));
    }

    #[test]
    fn renamed_descriptor_helpers_match_only_their_exact_member() {
        assert!(descriptor_callable_name_matches(
            "__smelt_get_value",
            "value"
        ));
        assert!(descriptor_callable_name_matches(
            "__smelt_set_value",
            "value"
        ));
        assert!(!descriptor_callable_name_matches(
            "__smelt_get_my_value",
            "value"
        ));
    }
}
