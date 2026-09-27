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
    /// A PREPASS. It runs twice over the crate for two different reasons, and
    /// both are needed:
    ///
    /// * crate-wide, from `predeclare_type_declarations_with_path`, before ANY
    ///   module body is lowered — a dependency cycle through a barrel file
    ///   routinely lowers a consumer before the module that declares the class
    ///   it references, and such a reference must still take the declaration's
    ///   type-parameter DEFAULTS;
    /// * per module, from `ModuleBuilder::module`, so a standalone lowering
    ///   (`dump-hir`, the frontend's own tests) that never runs the crate-wide
    ///   pass still sees its own classes. TypeScript hoists class TYPES, and the
    ///   forward-function-type and predeclaration passes lower signatures before
    ///   any class item exists, so "the class is already an `Item::Class`" is
    ///   not a condition this rule can depend on even within one file.
    ///
    /// Recording is keyed by the symbol the DECLARATION itself gets, which is
    /// `module_qualified_type_name` where the crate declares the spelling more
    /// than once and the bare interned spelling otherwise — the same two-step
    /// `class_declaration` and `resolve_type_reference_symbol` both take. Keying
    /// by the bare spelling instead would merge two unrelated declarations: a
    /// crate with a generic `class Context<E, P, I>` in one module and a plain
    /// `class Context` in another renames one of them, and a reference to the
    /// plain one must not pick up the generic one's defaults. The map is on
    /// [`crate::context::HirCtx`] so it outlives the module.
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
            let symbol = self
                .module_qualified_type_name(id.name.as_str())
                .unwrap_or_else(|| self.intern_type_name(id.name.as_str()));
            self.ctx.class_type_params.insert(symbol, lowered);
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

    /// Bind every one of a callee's own type parameters that argument
    /// inference left unbound, so an instantiated call result never carries a
    /// raw callee `TypeParam`.
    ///
    /// A call without explicit type arguments instantiates the callee's
    /// signature from whatever `substitutions` inference produced from the
    /// arguments. A parameter no argument mentions (`createNullObject<T =
    /// any>(): Record<string, T>` called with no arguments) used to stay as the
    /// raw `TypeParam { name: T }` in the call's result type. Because HIR type
    /// parameters are identified by NAME, that leaked callee `T` was then
    /// indistinguishable from the CALLER's own `T` (`class Router<T> { m =
    /// createNullObject() }` typed `m` as `Record<string, Router::T>`) or was
    /// simply unbound in the caller.
    ///
    /// Each unbound parameter takes, in TypeScript's own priority order:
    ///
    /// 1. the binding implied by the call's contextual (expected) result type
    ///    `result_hint`, matched against the declared `return_ty` — `const d:
    ///    Record<string, number> = makeBag()` binds `T := number`. A hint
    ///    binding that is itself `unknown` carries no evidence and is skipped;
    /// 2. the declared default (`= string`), instantiated through the bindings
    ///    chosen so far (a default may name an earlier parameter);
    /// 3. the declared `extends` constraint, instantiated the same way (what
    ///    `tsc` infers for a parameter with no inference candidate);
    /// 4. `unknown`.
    ///
    /// Defaults and constraints are instantiated in a SINGLE substitution pass
    /// over a map in which every still-unbound callee parameter is `unknown`,
    /// so a caller-scope parameter that happens to share a callee parameter's
    /// name is never re-substituted: every value in the finished map is already
    /// expressed purely in caller-scope types.
    ///
    /// `materialized` says whether the callee's definition really is generic
    /// over these parameters in the emitted Rust (a free function's
    /// `Function::type_params`). A method or lifted arrow whose generics are
    /// recorded only in `HirCtx::erased_item_type_params` is emitted with each
    /// parameter as `SmeltUnknown`, so there steps 1-3 would promise a concrete
    /// type the definition does not return: every unbound parameter goes
    /// straight to step 4.
    ///
    /// DYNAMIC BOUNDARY: steps 2 (for an `= any` default, which lowers to
    /// [`Type::Unknown`]) and 4 produce `unknown`. That is exactly the source's
    /// meaning — the declaration says "whatever, unchecked", supplies no
    /// information at all, or (not `materialized`) the definition itself is
    /// erased — and it is what codegen already rendered a leaked, out-of-scope
    /// type parameter as; the difference is that the erasure is now explicit
    /// instead of aliasing an unrelated caller parameter.
    pub(in crate::lowering) fn complete_callee_type_substitution(
        &mut self,
        type_params: &[TypeParamDef],
        materialized: bool,
        substitutions: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
        return_ty: smelt_hir::TypeId,
        result_hint: Option<smelt_hir::TypeId>,
    ) {
        if type_params
            .iter()
            .all(|param| substitutions.contains_key(&param.name))
        {
            return;
        }
        let unknown = self.ctx.krate.types.intern(Type::Unknown);
        if !materialized {
            for param in type_params {
                substitutions.entry(param.name).or_insert(unknown);
            }
            return;
        }
        let mut hinted = HashMap::new();
        if let Some(hint) = result_hint {
            let mut candidate = substitutions.clone();
            if self.infer_callee_type_bindings(type_params, return_ty, hint, &mut candidate) {
                hinted = candidate;
            }
        }
        for param in type_params {
            if substitutions.contains_key(&param.name) {
                continue;
            }
            let from_hint = hinted.get(&param.name).copied().filter(|ty| {
                !matches!(self.ctx.krate.types.get(*ty), Some(Type::Unknown))
                    && self.type_params_all_in_active_scope(*ty)
            });
            let chosen = match (from_hint, param.default.or(param.constraint)) {
                (Some(ty), _) => ty,
                (None, Some(declared)) => {
                    let mut scope = substitutions.clone();
                    for other in type_params {
                        scope.entry(other.name).or_insert(unknown);
                    }
                    self.substitute_type_params(declared, &scope)
                }
                (None, None) => unknown,
            };
            substitutions.insert(param.name, chosen);
        }
    }

    /// Whether every type parameter `ty` mentions is one the code being lowered
    /// can name — i.e. is declared by an ACTIVE type-parameter scope.
    ///
    /// A call's contextual result type is not always a caller-scope type: in
    /// argument position it is the OUTER callee's declared parameter type
    /// (`outer<U>(bag: Record<string, U>)` hints `outer(makeBag())` with the
    /// outer callee's raw `U`). Binding the inner callee from such a hint would
    /// move one callee's parameter into the caller — the very leak
    /// [`Self::complete_callee_type_substitution`] exists to prevent — so such
    /// a binding is rejected and the default chain decides instead.
    fn type_params_all_in_active_scope(&self, ty: smelt_hir::TypeId) -> bool {
        let Some(value) = self.ctx.krate.types.get(ty).cloned() else {
            return true;
        };
        let children = match value {
            Type::TypeParam { name } => {
                return self
                    .ctx
                    .krate
                    .symbols
                    .get(name)
                    .is_some_and(|text| self.type_parameter_type(text) == Some(ty));
            }
            Type::List(item) | Type::Set(item) | Type::Optional(item) | Type::Future(item) => {
                vec![item]
            }
            Type::Dict(key, value) | Type::JsMap(key, value) => vec![key, value],
            Type::Tuple(items) | Type::Union(items) => items,
            Type::Class { args, .. } => args,
            Type::Function(function) => {
                let mut children = function.params;
                children.push(function.return_ty);
                children
            }
            Type::Generator {
                yield_ty,
                return_ty,
                next_ty,
                ..
            } => vec![yield_ty, return_ty, next_ty],
            Type::GeneratorResult {
                yield_ty,
                return_ty,
            } => vec![yield_ty, return_ty],
            Type::Bool
            | Type::Int
            | Type::Float
            | Type::String
            | Type::Unknown
            | Type::Never
            | Type::None => Vec::new(),
        };
        children
            .into_iter()
            .all(|child| self.type_params_all_in_active_scope(child))
    }

    /// Infer callee type-parameter bindings from one `expected` (declared) /
    /// `actual` pair, binding only the callee's `own` parameters.
    ///
    /// Wraps [`Self::infer_overload_type`] with the one case it cannot see:
    /// HIR type parameters are identified by NAME, so when a caller's `T` flows
    /// into a callee's same-named `T` (`function tally<T>(x: T) { ident(x) }`)
    /// the declared and actual types are the SAME interned type, the matcher
    /// answers "compatible" at once, and nothing is bound. The call's result
    /// would then fall through to the callee's default or `unknown`. After the
    /// matcher succeeds, a structural walk binds each still-unbound own
    /// parameter that sits at a position where `actual` holds that identical
    /// type — the binding is the caller's type, which is what inference means.
    ///
    /// The identity walk binds only `own` names; bindings the matcher itself
    /// makes are left as they are (callers that must not rebind caller-scope
    /// names filter the map to `own`). Returns the matcher's verdict.
    pub(in crate::lowering) fn infer_callee_type_bindings(
        &mut self,
        own: &[TypeParamDef],
        expected: smelt_hir::TypeId,
        actual: smelt_hir::TypeId,
        substitutions: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> bool {
        let matched = self.infer_overload_type(expected, actual, substitutions);
        if matched {
            self.bind_identical_own_type_params(own, expected, actual, substitutions);
        }
        matched
    }

    /// Bind each unbound `own` type parameter in `expected` whose counterpart
    /// in `actual` is the identical type (see
    /// [`Self::infer_callee_type_bindings`]). Walks only positions where both
    /// sides have the same type constructor and arity; anything else is left
    /// to the ordinary matcher.
    fn bind_identical_own_type_params(
        &self,
        own: &[TypeParamDef],
        expected: smelt_hir::TypeId,
        actual: smelt_hir::TypeId,
        substitutions: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) {
        let (Some(expected_ty), Some(actual_ty)) = (
            self.ctx.krate.types.get(expected).cloned(),
            self.ctx.krate.types.get(actual).cloned(),
        ) else {
            return;
        };
        let walk = |pairs: Vec<(smelt_hir::TypeId, smelt_hir::TypeId)>,
                    bindings: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>| {
            for (declared_part, given_part) in pairs {
                self.bind_identical_own_type_params(own, declared_part, given_part, bindings);
            }
        };
        match (expected_ty, actual_ty) {
            (Type::TypeParam { name }, _) => {
                if expected == actual
                    && own.iter().any(|param| param.name == name)
                    && !substitutions.contains_key(&name)
                {
                    substitutions.insert(name, actual);
                }
            }
            (Type::List(declared), Type::List(given))
            | (Type::Set(declared), Type::Set(given))
            | (Type::Optional(declared), Type::Optional(given))
            | (Type::Future(declared), Type::Future(given)) => {
                walk(vec![(declared, given)], substitutions);
            }
            (Type::Dict(declared_key, declared_value), Type::Dict(given_key, given_value))
            | (
                Type::JsMap(declared_key, declared_value),
                Type::JsMap(given_key, given_value),
            ) => {
                walk(
                    vec![(declared_key, given_key), (declared_value, given_value)],
                    substitutions,
                );
            }
            (Type::Tuple(declared), Type::Tuple(given))
            | (Type::Union(declared), Type::Union(given))
                if declared.len() == given.len() =>
            {
                walk(declared.into_iter().zip(given).collect(), substitutions);
            }
            (
                Type::Class {
                    name: declared_name,
                    args: declared,
                },
                Type::Class {
                    name: given_name,
                    args: given,
                },
            ) if declared_name == given_name && declared.len() == given.len() => {
                walk(declared.into_iter().zip(given).collect(), substitutions);
            }
            (Type::Function(declared), Type::Function(given))
                if declared.params.len() == given.params.len() =>
            {
                let mut pairs = declared
                    .params
                    .into_iter()
                    .zip(given.params)
                    .collect::<Vec<_>>();
                pairs.push((declared.return_ty, given.return_ty));
                walk(pairs, substitutions);
            }
            _ => {}
        }
    }

    /// The type parameters a callable item declares, and whether its emitted
    /// definition is actually generic over them.
    ///
    /// A free function carries them on `Function::type_params` (materialized as
    /// Rust generics). Methods and lifted arrows keep them only in
    /// `HirCtx::erased_item_type_params` (emitted as `SmeltUnknown`). Anything
    /// else declares none.
    pub(in crate::lowering) fn callee_own_type_params(
        &self,
        item: smelt_hir::ItemId,
    ) -> (Vec<TypeParamDef>, bool) {
        if let smelt_hir::Item::Function(function) = self.item_ref(item)
            && !function.type_params.is_empty()
        {
            return (function.type_params.clone(), true);
        }
        self.ctx
            .erased_item_type_params
            .get(&item)
            .map_or_else(|| (Vec::new(), true), |params| (params.clone(), false))
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
