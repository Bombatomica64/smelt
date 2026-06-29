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
    Some(SpecializationData {
        module,
        values: specialization.values.nodes.clone(),
        required_adapters: specialization.required_adapters.clone(),
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

impl ModuleBuilder<'_> {
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
        let wrapper =
            find_specialized_function(module, &materialized.callable).ok_or_else(|| {
                SmeltError::native_specialization_adapter_required(
                    self.span(original.range),
                    "python.callable-provenance",
                    &format!(
                        "callable '{}' did not resolve to a source function",
                        materialized.callable.qualified_name
                    ),
                )
            })?;
        let original_name = original.name.as_str();
        let final_symbol = self.intern_name(original_name);
        let hidden_symbol = self.intern_name(&format!("__smelt_original_{original_name}"));
        self.rename_function_item(original_item, hidden_symbol)?;

        let saved_aliases = self.install_materialized_captures(
            &materialized.callable.captures,
            self.span(wrapper.range),
        )?;
        let wrapper_result = self.function_def(wrapper);
        self.restore_capture_aliases(saved_aliases);
        let wrapper_item = wrapper_result?;
        self.rename_function_item(wrapper_item, final_symbol)?;
        self.validate_materialized_signature(
            wrapper_item,
            &materialized.signature,
            self.span(original.range),
            original_name,
        )?;
        self.items.insert(original_name.to_owned(), wrapper_item);
        self.exports.insert(original_name.to_owned(), wrapper_item);
        if wrapper.name.as_str() != original_name {
            self.items.remove(wrapper.name.as_str());
            self.exports.remove(wrapper.name.as_str());
        }
        Ok(vec![original_item, wrapper_item])
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
        provenance.qualified_name == function.name.as_str()
            && provenance.span.start == function.range.start().to_u32()
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
                self.capture_source_item(&provenance.qualified_name, span)
            }
            smelt_specialize::GraphValueKind::ClassRef { qualified_name, .. } => {
                self.capture_source_item(&qualified_name, span)
            }
            _ => self.materialized_const_binding(name, value_id, span),
        }
    }

    /// Finds a source item referenced by qualified callable provenance.
    fn capture_source_item(&self, qualified_name: &str, span: Span) -> Result<ItemId, SmeltError> {
        let source_name = qualified_name.rsplit('.').next().unwrap_or(qualified_name);
        self.items.get(source_name).copied().ok_or_else(|| {
            SmeltError::native_specialization_adapter_required(
                span,
                "python.capture-provenance",
                &format!("captured callable '{qualified_name}' has no lowered source item"),
            )
        })
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
