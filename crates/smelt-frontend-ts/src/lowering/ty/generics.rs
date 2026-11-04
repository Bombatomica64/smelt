//! Generic type-parameter scope management and type-argument substitution.
//!
//! Handles pushing/popping type-parameter scopes, resolving parameters and
//! their constraints, building substitution maps, and applying substitutions
//! through HIR types, interface fields, and method signatures.

use std::collections::HashMap;

use oxc::ast::ast::{Declaration, Program, Statement};

use crate::lowering::ModuleBuilder;
use crate::SmeltError;
use smelt_hir::{ErasureReason, Field, FunctionType, MethodSig, Span, Type, TypeParamDef};

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
            let name = self.intern_declared_type_param(param);
            let ty = self.ctx.krate.types.intern(Type::TypeParam { name });
            scope.insert(param.name.name.to_string(), ty);
        }
        self.types.push_param_types(scope);

        // The names are in scope while the constraints and defaults lower, so
        // one may mention a sibling (`<T, K extends keyof T>`). A failure
        // there must not leave the name scope pushed without its constraint
        // scope: the push is all-or-nothing.
        let mut lowered = Vec::new();
        let mut constraints = HashMap::new();
        let mut lower_bounds = || -> Result<(), SmeltError> {
            for param in &params.params {
                let name = self.intern_declared_type_param(param);
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
            Ok(())
        };
        let bounds = lower_bounds();
        self.types.push_param_constraints(constraints);
        if let Err(error) = bounds {
            self.types.pop_param_scope();
            return Err(error);
        }
        Ok(lowered)
    }

    /// Run `lower` inside the type-parameter scope of `params`, popping it on
    /// EVERY exit path, an early `return Err(..)` or `?` inside `lower`
    /// included.
    ///
    /// This is the scoped form of [`Self::push_type_parameter_scope`] /
    /// [`Self::pop_type_parameter_scope`], in the same shape as
    /// `with_fn_ctx`: a scope left pushed by a failed declaration would
    /// otherwise stay active for every item lowered after it, so a later
    /// reference to a type with the same name as one of its parameters
    /// resolved to that stale parameter. `lower` receives the lowered
    /// parameter declarations.
    pub(in crate::lowering) fn with_type_parameter_scope<R>(
        &mut self,
        params: Option<&oxc::ast::ast::TSTypeParameterDeclaration<'_>>,
        lower: impl FnOnce(&mut Self, Vec<TypeParamDef>) -> Result<R, SmeltError>,
    ) -> Result<R, SmeltError> {
        let defs = self.push_type_parameter_scope(params)?;
        let result = lower(self, defs);
        self.pop_type_parameter_scope();
        result
    }

    /// The symbol a declared type parameter is known by: its own spelling when
    /// rendered, but an identity owned by its DECLARING ITEM.
    ///
    /// `Type::TypeParam` is identified by its symbol alone, and every
    /// substitution map in the pipeline is keyed by it. Interned by spelling,
    /// two unrelated parameters that share a name (`class Holder<Value>` and
    /// `class Parser<Value>`) were one type, so a substitution meant for one
    /// silently rewrote the other — the inherited-copy adapters of a generic
    /// abstract base read `Holder`'s `Value` as the subclass's `number`
    /// (bounded-generics plan, increment 5). The identity key therefore names
    /// the file and the owning item's path (`type_param_owners`): the overload
    /// signatures of one function, or of one method, share their parameters as
    /// TypeScript does, and every other item's parameters are distinct. The
    /// rendering stays the source spelling, so the emitted Rust is unchanged.
    pub(in crate::lowering) fn intern_declared_type_param(
        &mut self,
        param: &oxc::ast::ast::TSTypeParameter<'_>,
    ) -> smelt_hir::Symbol {
        let spelling = param.name.name.as_str();
        // Every declaration of a parsed module is in the owner map; a miss (a
        // builder that never scanned its program) keys by the declaration
        // site, which is still never shared with another item.
        let owner = self
            .type_param_owners
            .get(&param.span.start)
            .cloned()
            .unwrap_or_else(|| format!("@{}", param.span.start));
        let key = format!("{spelling}\u{1f}tp{}:{owner}", self.file_id.0);
        let name = self.ctx.krate.symbols.intern_rendered(&key, spelling);
        self.ctx.krate.names.record(name, spelling);
        name
    }

    /// Push the type-parameter scope of a generic CLOSURE value (an arrow
    /// expression lowered as a closure rather than lifted to an item).
    ///
    /// A Rust closure — and the `Rc<dyn Fn(..)>` slot it is stored in — cannot
    /// be generic over its own parameters, so a hand-written port of
    /// `<K extends string>(key: K): K => key` writes `|key: String| key`. Each
    /// own parameter with a constraint therefore resolves to that constraint
    /// inside the closure, the same erase-to-constraint rule a callable
    /// interface's generic call signature follows (`decls::call_signature`),
    /// so the closure and the slot it is stored into agree on one concrete
    /// signature. An unconstrained parameter has no single type to stand for
    /// it and stays a [`Type::TypeParam`].
    ///
    /// The declaration is first pushed as an ordinary scope so a constraint
    /// may mention a sibling parameter (`<T, K extends keyof T>`); when any
    /// parameter is constrained that scope is replaced by the erased one. Pop
    /// it with [`Self::pop_type_parameter_scope`] as usual.
    pub(in crate::lowering) fn push_closure_type_parameter_scope(
        &mut self,
        params: Option<&oxc::ast::ast::TSTypeParameterDeclaration<'_>>,
    ) -> Result<(), SmeltError> {
        let defs = self.push_type_parameter_scope(params)?;
        let Some(params) = params else {
            return Ok(());
        };
        if defs.iter().all(|def| def.constraint.is_none()) {
            return Ok(());
        }
        self.pop_type_parameter_scope();
        let mut scope = HashMap::new();
        let mut constraints = HashMap::new();
        for (param, def) in params.params.iter().zip(&defs) {
            let ty = match def.constraint {
                Some(constraint) => {
                    constraints.insert(def.name, constraint);
                    constraint
                }
                None => self.ctx.krate.types.intern(Type::TypeParam { name: def.name }),
            };
            scope.insert(param.name.name.to_string(), ty);
        }
        self.types.push_param_types(scope);
        self.types.push_param_constraints(constraints);
        Ok(())
    }

    /// Whether `left` and `right` are the same type up to a consistent
    /// renaming of type parameters that are NOT in scope here.
    ///
    /// Each generic declaration owns its parameters (`intern_declared_type_param`),
    /// so `asc: <T>(x: T, y: T) => x > y` and `desc: <T>(x: T, y: T) => x < y`
    /// have two different `T`s and two different interned function types.
    /// Seen from the code that holds both values, though, neither `T` is in
    /// scope: each is bound by its own value's signature, and the two
    /// signatures are alpha-equivalent — one type, as TypeScript reports it.
    /// A parameter that IS in scope (the enclosing function's `T`) names one
    /// type here and is never renamed.
    pub(in crate::lowering) fn alpha_equivalent_types(
        &self,
        left: smelt_hir::TypeId,
        right: smelt_hir::TypeId,
    ) -> bool {
        let mut renaming = HashMap::new();
        self.alpha_equivalent_under(left, right, &mut renaming)
    }

    /// [`Self::alpha_equivalent_types`] with the renaming built so far
    /// (`left` parameter -> `right` parameter).
    fn alpha_equivalent_under(
        &self,
        left: smelt_hir::TypeId,
        right: smelt_hir::TypeId,
        renaming: &mut HashMap<smelt_hir::TypeId, smelt_hir::TypeId>,
    ) -> bool {
        if left == right && renaming.is_empty() {
            return true;
        }
        let (Some(left_ty), Some(right_ty)) =
            (self.ctx.krate.types.get(left), self.ctx.krate.types.get(right))
        else {
            return false;
        };
        match (left_ty, right_ty) {
            (Type::TypeParam { .. }, Type::TypeParam { .. }) => {
                if !self.type_param_is_out_of_scope(left) || !self.type_param_is_out_of_scope(right) {
                    return left == right;
                }
                // A bijection: each side's parameter maps to exactly one.
                match renaming.get(&left) {
                    Some(mapped) => *mapped == right,
                    None if renaming.values().any(|mapped| *mapped == right) => false,
                    None => {
                        renaming.insert(left, right);
                        true
                    }
                }
            }
            (Type::List(left_item), Type::List(right_item))
            | (Type::Set(left_item), Type::Set(right_item))
            | (Type::Optional(left_item), Type::Optional(right_item))
            | (Type::Future(left_item), Type::Future(right_item)) => {
                self.alpha_equivalent_under(*left_item, *right_item, renaming)
            }
            (Type::Dict(left_key, left_value), Type::Dict(right_key, right_value))
            | (Type::JsMap(left_key, left_value), Type::JsMap(right_key, right_value)) => {
                self.alpha_equivalent_under(*left_key, *right_key, renaming)
                    && self.alpha_equivalent_under(*left_value, *right_value, renaming)
            }
            (Type::Tuple(left_items), Type::Tuple(right_items))
            | (Type::Union(left_items), Type::Union(right_items)) => {
                self.alpha_equivalent_slices(left_items, right_items, renaming)
            }
            (
                Type::Class { name: left_name, args: left_args },
                Type::Class { name: right_name, args: right_args },
            ) => {
                left_name == right_name
                    && self.alpha_equivalent_slices(left_args, right_args, renaming)
            }
            (Type::Function(left_fn), Type::Function(right_fn)) => {
                left_fn.rest == right_fn.rest
                    && left_fn.required_params == right_fn.required_params
                    && left_fn.mutable_params == right_fn.mutable_params
                    && left_fn.is_async == right_fn.is_async
                    && left_fn.may_throw == right_fn.may_throw
                    && self.alpha_equivalent_slices(&left_fn.params, &right_fn.params, renaming)
                    && self.alpha_equivalent_under(left_fn.return_ty, right_fn.return_ty, renaming)
            }
            (
                Type::Generator {
                    is_async: left_async,
                    yield_ty: left_yield,
                    return_ty: left_return,
                    next_ty: left_next,
                },
                Type::Generator {
                    is_async: right_async,
                    yield_ty: right_yield,
                    return_ty: right_return,
                    next_ty: right_next,
                },
            ) => {
                left_async == right_async
                    && self.alpha_equivalent_under(*left_yield, *right_yield, renaming)
                    && self.alpha_equivalent_under(*left_return, *right_return, renaming)
                    && self.alpha_equivalent_under(*left_next, *right_next, renaming)
            }
            (
                Type::GeneratorResult { yield_ty: left_yield, return_ty: left_return },
                Type::GeneratorResult { yield_ty: right_yield, return_ty: right_return },
            ) => {
                self.alpha_equivalent_under(*left_yield, *right_yield, renaming)
                    && self.alpha_equivalent_under(*left_return, *right_return, renaming)
            }
            _ => left == right,
        }
    }

    /// [`Self::alpha_equivalent_under`] position by position over two lists
    /// of the same length.
    fn alpha_equivalent_slices(
        &self,
        lefts: &[smelt_hir::TypeId],
        rights: &[smelt_hir::TypeId],
        renaming: &mut HashMap<smelt_hir::TypeId, smelt_hir::TypeId>,
    ) -> bool {
        lefts.len() == rights.len()
            && lefts
                .iter()
                .zip(rights)
                .all(|(left_item, right_item)| self.alpha_equivalent_under(*left_item, *right_item, renaming))
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
    /// * per module, from `ModuleBuilder::program`, so a standalone lowering
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
            // Interfaces, and object-shaped aliases (which lower to interfaces,
            // see `type_alias_declaration`), are referenced the same way and
            // need the same defaults: a forward reference reaches the class
            // arm of `type_reference_to_hir` before the declaration exists.
            let declared: Option<(&str, Option<&oxc::ast::ast::TSTypeParameterDeclaration<'_>>)> =
                match (statement, exported) {
                    (Statement::ClassDeclaration(class), _)
                    | (_, Some(Declaration::ClassDeclaration(class))) => class
                        .id
                        .as_ref()
                        .map(|id| (id.name.as_str(), class.type_parameters.as_deref())),
                    (Statement::TSInterfaceDeclaration(interface), _)
                    | (_, Some(Declaration::TSInterfaceDeclaration(interface))) => Some((
                        interface.id.name.as_str(),
                        interface.type_parameters.as_deref(),
                    )),
                    (Statement::TSTypeAliasDeclaration(alias), _)
                    | (_, Some(Declaration::TSTypeAliasDeclaration(alias)))
                        if matches!(alias.type_annotation, oxc::ast::ast::TSType::TSTypeLiteral(_)) =>
                    {
                        Some((alias.id.name.as_str(), alias.type_parameters.as_deref()))
                    }
                    _ => None,
                };
            let Some((name, Some(params))) = declared else { continue };
            let Ok(lowered) = self.push_type_parameter_scope(Some(params)) else {
                self.pop_type_parameter_scope();
                continue;
            };
            self.pop_type_parameter_scope();
            let symbol = self
                .module_qualified_type_name(name)
                .unwrap_or_else(|| self.intern_type_name(name));
            self.ctx.class_type_params.entry(symbol).or_insert(lowered);
        }
    }

    /// Record every class declaration's constructor parameter types (see
    /// [`crate::context::HirCtx::class_constructor_params`]).
    ///
    /// A PREPASS run beside `collect_class_type_parameter_defaults`, for the
    /// same reason: a construction can be lowered before its class item
    /// exists. Hono's `#clone()` builds `new Hono<E, ..>({ router, getPath })`
    /// inside `class Hono` itself; with no parameter types the options object
    /// had no contextual type, became an erased record, and lost its router on
    /// the way back into `HonoOptions`. Each annotation is lowered in the
    /// class's own type-parameter scope; a parameter whose annotation is
    /// missing or fails to lower records no hint (`Unknown` is not recorded),
    /// and the class itself reports any real error when it is lowered.
    pub(in crate::lowering) fn collect_class_constructor_param_types(
        &mut self,
        program: &Program<'_>,
    ) {
        for statement in &program.body {
            let class = match statement {
                Statement::ClassDeclaration(class) => class,
                Statement::ExportDeclaration(export) => match &export.declaration {
                    Declaration::ClassDeclaration(class) => class,
                    _ => continue,
                },
                _ => continue,
            };
            let Some(id) = class.id.as_ref() else { continue };
            let Some(constructor) = class.body.body.iter().find_map(|element| match element {
                oxc::ast::ast::ClassElement::MethodDefinition(method)
                    if method.kind == oxc::ast::ast::MethodDefinitionKind::Constructor =>
                {
                    Some(method)
                }
                _ => None,
            }) else {
                continue;
            };
            if self
                .push_type_parameter_scope(class.type_parameters.as_deref())
                .is_err()
            {
                self.pop_type_parameter_scope();
                continue;
            }
            let params: Option<Vec<smelt_hir::TypeId>> = constructor
                .value
                .params
                .items
                .iter()
                .map(|param| {
                    let annotation = param.type_annotation.as_ref()?;
                    let ty = self.ts_type_to_hir(&annotation.type_annotation).ok()?;
                    Some(if param.optional {
                        self.ctx.krate.types.intern(Type::Optional(ty))
                    } else {
                        ty
                    })
                })
                .collect();
            self.pop_type_parameter_scope();
            let Some(params) = params else { continue };
            let symbol = self
                .module_qualified_type_name(id.name.as_str())
                .unwrap_or_else(|| self.intern_type_name(id.name.as_str()));
            self.ctx.class_constructor_params.entry(symbol).or_insert(params);
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
    /// inference left unbound to `unknown`, so an instantiated call result
    /// never carries a raw callee `TypeParam`.
    ///
    /// A call without explicit type arguments instantiates the callee's
    /// signature from whatever `substitutions` inference produced from the
    /// arguments. A parameter no argument binds (`createNullObject<T =
    /// any>(): Record<string, T>` called with no arguments) used to stay as the
    /// raw `TypeParam { name: T }` in the call's result type. Because HIR type
    /// parameters are identified by NAME, that leaked callee `T` was then
    /// indistinguishable from the CALLER's own `T` (`class Router<T> { m =
    /// createNullObject() }` typed `m` as `Record<string, Router::T>`) or was
    /// simply unbound in the caller.
    ///
    /// Why `unknown` and not TypeScript's default / constraint / contextual
    /// type: the call's HIR type must agree with what the EMITTED callee
    /// returns, and Rust codegen binds a callee's type parameters from the
    /// call's ARGUMENTS only (`static_call_monomorphization` in
    /// `smelt-codegen-rust`). A parameter no argument binds is therefore
    /// erased at every such call site — either the definition erases it
    /// outright (`liftable_type_params` finds no inference source, e.g. a
    /// return-only `T` or one reachable only through `T | ((i) => T)`), or the
    /// site demotes because the binding is not concrete. Typing the result
    /// from the default instead (`range<T = number>(..): Generator<T>` as
    /// `Generator<number>`) described a value the definition never returns,
    /// and not every container can be converted after the fact (radash's
    /// `range` generator, E0308). That liftability decision is whole-program
    /// (it depends on every call site in the MIR), so the frontend cannot
    /// reproduce it; it instead mirrors the rule the decision is built on.
    ///
    /// DYNAMIC BOUNDARY: this `unknown` is the erased callee generic itself;
    /// it is what codegen rendered the leaked, out-of-scope parameter as
    /// before, so it adds no new erasure — it only stops aliasing an
    /// unrelated caller parameter. Recovering the concrete type needs codegen
    /// to materialize return-only generics (a turbofish at the call site).
    pub(in crate::lowering) fn complete_callee_type_substitution(
        &mut self,
        type_params: &[TypeParamDef],
        substitutions: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) {
        if type_params
            .iter()
            .all(|param| substitutions.contains_key(&param.name))
        {
            return;
        }
        let unknown = self.erase(ErasureReason::UnfilledGeneric);
        for param in type_params {
            substitutions.entry(param.name).or_insert(unknown);
        }
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

    /// The type parameters a callable item declares.
    ///
    /// A free function or lifted arrow const carries them on
    /// `Function::type_params`. Methods keep them only in
    /// `HirCtx::erased_item_type_params` (their emitted definitions render
    /// them as `SmeltUnknown`). Anything else declares none.
    pub(in crate::lowering) fn callee_own_type_params(
        &self,
        item: smelt_hir::ItemId,
    ) -> Vec<TypeParamDef> {
        if let smelt_hir::Item::Function(function) = self.item_ref(item)
            && !function.type_params.is_empty()
        {
            return function.type_params.clone();
        }
        self.ctx
            .erased_item_type_params
            .get(&item)
            .cloned()
            .unwrap_or_default()
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
            | Type::FixedInt(_)
            | Type::BigInt
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
