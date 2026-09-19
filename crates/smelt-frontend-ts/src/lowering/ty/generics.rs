//! Generic type-parameter scope management and type-argument substitution.
//!
//! Handles pushing/popping type-parameter scopes, resolving parameters and
//! their constraints, building substitution maps, and applying substitutions
//! through HIR types, interface fields, and method signatures.

use std::collections::HashMap;

use oxc::ast::ast::{Class as TSClass, Declaration, Program, Statement};

use crate::lowering::ModuleBuilder;
use crate::SmeltError;
use smelt_hir::{Field, FunctionType, MethodSig, Span, Type, TypeParamDef};

impl ModuleBuilder<'_> {
    /// Lower a TypeScript type parameter declaration and push its symbols as an active scope.
    pub(in crate::lowering) fn push_type_parameter_scope(
        &mut self,
        params: Option<&oxc::ast::ast::TSTypeParameterDeclaration<'_>>,
    ) -> Result<Vec<TypeParamDef>, SmeltError> {
        let Some(params) = params else {
            self.types.push_param_types(HashMap::new());
            self.types.push_param_constraints(HashMap::new());
            return Ok(Vec::new());
        };

        let mut scope = HashMap::new();
        for param in &params.params {
            let name = self.intern_type_name(param.name.name.as_str());
            let ty = self.ctx.krate.types.intern(Type::TypeParam { name });
            scope.insert(param.name.name.to_string(), ty);
        }
        self.types.push_param_types(scope);

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
        self.types.push_param_constraints(constraints);
        Ok(lowered)
    }

    /// Record the type-parameter declarations of every class in this program.
    ///
    /// A PREPASS, run before any body is lowered. TypeScript hoists class TYPES:
    /// a type reference may name a class declared later in the same file, and a
    /// signature collected by an earlier prepass (forward function types,
    /// predeclared methods) is lowered before any class item exists at all. Such
    /// a reference still has to take the declaration's type-parameter DEFAULTS,
    /// so the defaults have to be readable before the class is an HIR item —
    /// which is what this map provides. Classes declared by EARLIER modules need
    /// no entry: they are already `Item::Class` and `find_class` reads their
    /// parameters off the crate.
    ///
    /// Each declaration's parameters are lowered in the declaration's OWN
    /// parameter scope, because a default may mention an earlier parameter of
    /// the same list (`class Pair<A, B = A[]>`). A class whose parameters fail
    /// to lower is skipped rather than reported: this pass exists only to
    /// enrich later references, and the real diagnostic is raised when the class
    /// itself is lowered.
    pub(in crate::lowering) fn collect_class_type_parameter_defaults(
        &mut self,
        program: &Program<'_>,
    ) {
        for statement in &program.body {
            let exported = match statement {
                Statement::ExportDeclaration(export) => Some(&export.declaration),
                _ => None,
            };
            let class: Option<&TSClass<'_>> = match (statement, exported) {
                (Statement::ClassDeclaration(class), _) => Some(class.as_ref()),
                (_, Some(Declaration::ClassDeclaration(class))) => Some(class.as_ref()),
                _ => None,
            };
            let Some(class) = class else { continue };
            let Some(id) = class.id.as_ref() else { continue };
            let Some(params) = class.type_parameters.as_deref() else {
                continue;
            };
            let Ok(lowered) = self.push_type_parameter_scope(Some(params)) else {
                self.pop_type_parameter_scope();
                continue;
            };
            self.pop_type_parameter_scope();
            let symbol = self.resolve_type_reference_symbol(id.name.as_str());
            self.types.set_class_type_params(symbol, lowered);
        }
    }

    /// Pop the current TypeScript type parameter scope.
    pub(in crate::lowering) fn pop_type_parameter_scope(&mut self) {
        self.types.pop_param_scope();
    }

    /// Resolve a type parameter by source name from innermost to outermost scope.
    pub(in crate::lowering) fn type_parameter_type(&self, name: &str) -> Option<smelt_hir::TypeId> {
        self.types.param_type(name)
    }

    /// Resolve a lowered type parameter's active constraint, if any.
    pub(in crate::lowering) fn type_parameter_constraint(
        &self,
        name: smelt_hir::Symbol,
    ) -> Option<smelt_hir::TypeId> {
        self.types.param_constraint(name)
    }

    /// Builds a substitution map from generic parameter symbols to actual argument types.
    pub(in crate::lowering) fn type_argument_substitution(
        &mut self,
        type_params: &[TypeParamDef],
        args: &[smelt_hir::TypeId],
        span: Span,
    ) -> Result<HashMap<smelt_hir::Symbol, smelt_hir::TypeId>, SmeltError> {
        if args.len() > type_params.len() {
            if type_params.is_empty() {
                return Ok(HashMap::new());
            }
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

    /// Complete a short type-argument list from the declaration's defaults.
    ///
    /// TypeScript lets a type reference omit TRAILING type arguments whenever
    /// every omitted parameter declares a default: `class C<A, B = string,
    /// C = number>` referenced as `C<X>` means `C<X, string, number>`, and a
    /// declaration whose parameters ALL have defaults may be referenced with no
    /// argument list at all (`C` where every parameter is defaulted). A default
    /// is itself lowered in the declaration's own parameter scope, so an
    /// earlier parameter may appear in a later default (`<A, B = A[]>`); the
    /// substitution map is therefore built left to right and applied to each
    /// default as it is taken.
    ///
    /// Returns `None` when the list cannot be completed — some omitted
    /// parameter has no default, which `tsc` rejects except where inference
    /// supplies the argument. Callers keep whatever shorter list they already
    /// had in that case rather than inventing arguments.
    ///
    /// DYNAMIC BOUNDARY: a parameter defaulted to `any` (`E extends Env = any`)
    /// lowers to [`Type::Unknown`], because source `any` in a type position IS
    /// a dynamic boundary under the project's `SmeltUnknown` rules — the
    /// declaration itself says "whatever the instantiation supplies, unchecked".
    /// Taking the default is still strictly better than dropping the argument:
    /// a dropped argument leaves the reference with the WRONG ARITY, which no
    /// later stage can repair, whereas the default is the type the source
    /// actually means.
    pub(in crate::lowering) fn type_arguments_with_defaults(
        &mut self,
        type_params: &[TypeParamDef],
        args: &[smelt_hir::TypeId],
    ) -> Option<Vec<smelt_hir::TypeId>> {
        if type_params.is_empty() || args.len() >= type_params.len() {
            return None;
        }
        let mut substitutions = HashMap::new();
        let mut completed = Vec::with_capacity(type_params.len());
        for (idx, param) in type_params.iter().enumerate() {
            let actual = if let Some(arg) = args.get(idx) {
                *arg
            } else {
                let default = param.default?;
                self.substitute_type_params(default, &substitutions)
            };
            substitutions.insert(param.name, actual);
            completed.push(actual);
        }
        Some(completed)
    }

    /// Substitute generic type parameters within a previously lowered HIR type.
    pub(in crate::lowering) fn substitute_type_params(
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
            Type::JsMap(key, value) => {
                let key = self.substitute_type_params(key, substitutions);
                let value = self.substitute_type_params(value, substitutions);
                self.ctx.krate.types.intern(Type::JsMap(key, value))
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
                    rest: function.rest,
                    required_params: function.required_params,
                    mutable_params: Vec::new(),
                    return_ty,
                    is_async: function.is_async,
                    may_throw: false,
                }))
            }
            Type::Future(item) => {
                let item = self.substitute_type_params(item, substitutions);
                self.ctx.krate.types.intern(Type::Future(item))
            }
            Type::Generator {
                is_async,
                yield_ty,
                return_ty,
                next_ty,
            } => {
                let yield_ty = self.substitute_type_params(yield_ty, substitutions);
                let return_ty = self.substitute_type_params(return_ty, substitutions);
                let next_ty = self.substitute_type_params(next_ty, substitutions);
                self.ctx.krate.types.intern(Type::Generator {
                    is_async,
                    yield_ty,
                    return_ty,
                    next_ty,
                })
            }
            Type::GeneratorResult {
                yield_ty,
                return_ty,
            } => {
                let yield_ty = self.substitute_type_params(yield_ty, substitutions);
                let return_ty = self.substitute_type_params(return_ty, substitutions);
                self.ctx.krate.types.intern(Type::GeneratorResult {
                    yield_ty,
                    return_ty,
                })
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
    pub(in crate::lowering) fn substituted_fields(
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
    pub(in crate::lowering) fn substituted_methods(
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
