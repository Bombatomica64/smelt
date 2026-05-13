impl ModuleBuilder<'_> {
    /// Lower a TypeScript type parameter declaration and push its symbols as an active scope.
    fn push_type_parameter_scope(
        &mut self,
        params: Option<&oxc::ast::ast::TSTypeParameterDeclaration<'_>>,
    ) -> Result<Vec<TypeParamDef>, SmeltError> {
        let Some(params) = params else {
            self.type_param_scopes.push(HashMap::new());
            self.type_param_constraint_scopes.push(HashMap::new());
            return Ok(Vec::new());
        };

        let mut scope = HashMap::new();
        for param in &params.params {
            let name = self.intern_type_name(param.name.name.as_str());
            let ty = self.ctx.krate.types.intern(Type::TypeParam { name });
            scope.insert(param.name.name.to_string(), ty);
        }
        self.type_param_scopes.push(scope);

        let mut lowered = Vec::new();
        let mut constraints = HashMap::new();
        for param in &params.params {
            let name = self.intern_type_name(param.name.name.as_str());
            let constraint = param
                .constraint
                .as_ref()
                .map(|constraint| self.ts_type_to_hir(constraint))
                .transpose()?;
            if let Some(constraint) = constraint {
                constraints.insert(name, constraint);
            }
            let default = param
                .default
                .as_ref()
                .map(|default| self.ts_type_to_hir(default))
                .transpose()?;
            lowered.push(TypeParamDef {
                name,
                constraint,
                default,
                span: self.span(param.span.start, param.span.end),
            });
        }
        self.type_param_constraint_scopes.push(constraints);
        Ok(lowered)
    }

    /// Pop the current TypeScript type parameter scope.
    fn pop_type_parameter_scope(&mut self) {
        self.type_param_scopes.pop();
        self.type_param_constraint_scopes.pop();
    }

    /// Resolve a type parameter by source name from innermost to outermost scope.
    fn type_parameter_type(&self, name: &str) -> Option<smelt_hir::TypeId> {
        self.type_param_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    /// Resolve a lowered type parameter's active constraint, if any.
    fn type_parameter_constraint(
        &self,
        name: smelt_hir::Symbol,
    ) -> Option<smelt_hir::TypeId> {
        self.type_param_constraint_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(&name).copied())
    }

    /// Builds a substitution map from generic parameter symbols to actual argument types.
    fn type_argument_substitution(
        &mut self,
        type_params: &[TypeParamDef],
        args: &[smelt_hir::TypeId],
        span: Span,
    ) -> Result<HashMap<smelt_hir::Symbol, smelt_hir::TypeId>, SmeltError> {
        if args.len() > type_params.len() {
            return Err(SmeltError::unsupported(
                span,
                "too many generic type arguments",
            ));
        }
        let mut substitutions = HashMap::new();
        for (idx, param) in type_params.iter().enumerate() {
            let actual = if let Some(arg) = args.get(idx) {
                *arg
            } else if let Some(default) = param.default {
                self.substitute_type_params(default, &substitutions)
            } else {
                self.ctx.krate.types.intern(Type::TypeParam { name: param.name })
            };
            substitutions.insert(param.name, actual);
        }
        Ok(substitutions)
    }

    /// Substitute generic type parameters within a previously lowered HIR type.
    fn substitute_type_params(
        &mut self,
        ty: smelt_hir::TypeId,
        substitutions: &HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> smelt_hir::TypeId {
        let Some(value) = self.ctx.krate.types.get(ty).cloned() else {
            return ty;
        };
        match value {
            Type::TypeParam { name } => substitutions.get(&name).copied().unwrap_or(ty),
            Type::List(item) => {
                let item = self.substitute_type_params(item, substitutions);
                self.ctx.krate.types.intern(Type::List(item))
            }
            Type::Set(item) => {
                let item = self.substitute_type_params(item, substitutions);
                self.ctx.krate.types.intern(Type::Set(item))
            }
            Type::Dict(key, value) => {
                let key = self.substitute_type_params(key, substitutions);
                let value = self.substitute_type_params(value, substitutions);
                self.ctx.krate.types.intern(Type::Dict(key, value))
            }
            Type::Tuple(items) => {
                let items = items
                    .into_iter()
                    .map(|item| self.substitute_type_params(item, substitutions))
                    .collect();
                self.ctx.krate.types.intern(Type::Tuple(items))
            }
            Type::Optional(item) => {
                let item = self.substitute_type_params(item, substitutions);
                self.ctx.krate.types.intern(Type::Optional(item))
            }
            Type::Union(items) => {
                let items = items
                    .into_iter()
                    .map(|item| self.substitute_type_params(item, substitutions))
                    .collect();
                self.ctx.krate.types.intern(Type::Union(items))
            }
            Type::Class { name, args } => {
                let args = args
                    .into_iter()
                    .map(|arg| self.substitute_type_params(arg, substitutions))
                    .collect();
                self.ctx.krate.types.intern(Type::Class { name, args })
            }
            Type::Function(function) => {
                let params = function
                    .params
                    .into_iter()
                    .map(|param| self.substitute_type_params(param, substitutions))
                    .collect();
                let return_ty = self.substitute_type_params(function.return_ty, substitutions);
                self.ctx.krate.types.intern(Type::Function(FunctionType {
                    params,
                    return_ty,
                    is_async: function.is_async,
                }))
            }
            Type::Future(item) => {
                let item = self.substitute_type_params(item, substitutions);
                self.ctx.krate.types.intern(Type::Future(item))
            }
            Type::Bool
            | Type::Int
            | Type::Float
            | Type::String
            | Type::Unknown
            | Type::Never
            | Type::None => ty,
        }
    }

    /// Substitute type parameters through interface fields.
    fn substituted_fields(
        &mut self,
        fields: &[Field],
        substitutions: &HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> Vec<Field> {
        fields
            .iter()
            .cloned()
            .map(|mut field| {
                field.ty = self.substitute_type_params(field.ty, substitutions);
                field
            })
            .collect()
    }

    /// Substitute type parameters through interface method signatures.
    fn substituted_methods(
        &mut self,
        methods: &[MethodSig],
        substitutions: &HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> Vec<MethodSig> {
        methods
            .iter()
            .cloned()
            .map(|mut method| {
                method.return_ty = self.substitute_type_params(method.return_ty, substitutions);
                for param in &mut method.params {
                    param.ty = self.substitute_type_params(param.ty, substitutions);
                }
                method
            })
            .collect()
    }
}
