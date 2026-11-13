//! Python AST to smelt HIR lowering.
#![allow(
    clippy::redundant_pub_crate,
    reason = "the crate root constructs the lowering builder"
)]

mod stdlib;
mod stdlib_dispatch;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use indexmap::IndexMap;

use ruff_python_ast::{
    BoolOp, CmpOp, ElifElseClause, ExceptHandler, Expr, ModModule, Number, Operator,
    Pattern as RuffPattern, PatternMatchAs, Singleton, Stmt, StmtAnnAssign, StmtAssert, StmtAssign,
    StmtAugAssign, StmtClassDef, StmtFor, StmtFunctionDef, StmtIf, StmtImport, StmtImportFrom,
    StmtMatch, StmtTry, StmtWith, UnaryOp as RuffUnaryOp,
};
use ruff_text_size::{Ranged, TextRange};
use smelt_hir::{
    AsyncOp, BinOp, Body, CaptureMode, Class, ClassKind,
    ClassProtocol,
    ClosureCapture, ConstItem, DictProjectionOp, Expr as HirExpr, ExprKind, Field, FileId,
    Function, FunctionOwner, FunctionType, Import, Item, ItemId, Language, ListCallbackOp, Literal,
    LocalDecl, MatchArm, MethodSig, Module, ModuleId, NumericExtremaOp, NumericPredicateOp,
    NumericRoundOp, NumericUnaryFuncOp, Param, ParamSig, Pattern as HirPattern, PrimitiveCastOp,
    PropertyLookup, PythonDivisionOp, RegexMatchOp, SetProjectionOp, SourceFile, Span, Stmt as HirStmt, StringAffixOp, StringCaseOp,
    StringPredicateOp, StringReplaceOp, StringSearchOp, StringTrimSide, Symbol, Type, TypeId,
    TypeParamDef, UnaryOp, Visibility,
};
use smelt_stdlib::RuleId;

use crate::helpers::{
    collect_bitor_parts, decorator_frozen_kwarg, decorator_simple_name, dotted_name, is_stub_body,
    expr_kind_name, expr_simple_name, expr_type_name, is_django_model_base, range_to_span,
    stmt_kind_name, two_type_args,
};
use crate::name_resolution::{EvalScope, ModuleBindings, StdlibMatch, StdlibName};
use crate::{HirCtx, SmeltError, test_support};

/// Option-owned specialization data for one source module.
#[derive(Debug, Clone)]
pub(crate) struct SpecializationData {
    /// Materialized module definitions.
    module: smelt_specialize::ModuleRecord,
    /// Shared reference-preserving value graph.
    values: Vec<smelt_specialize::GraphValue>,
    /// Opaque behavior that requires explicit adapters.
    required_adapters: Vec<smelt_specialize::AdapterRequirement>,
    /// Output captured while this module was imported at definition time, in
    /// write order (see [`OutputReplay`]).
    outputs: Vec<OutputReplay>,
}

/// One write captured during definition-time execution, placed in the module.
#[derive(Debug, Clone)]
pub(crate) struct OutputReplay {
    /// Whether the text went to standard error.
    stderr: bool,
    /// The captured text.
    text: String,
    /// Byte offset, in this module's source, of the line of the top-level
    /// statement that wrote it; `None` when the write is not attributed to one
    /// (an older guest, or a write outside any top-level statement), which
    /// replays at the start of the module body.
    offset: Option<u32>,
}

/// A module-level type alias and the parameters it is generic over.
#[derive(Debug, Clone)]
pub(crate) struct TypeAliasDef {
    /// The aliased type, with its parameters left as placeholders.
    ty: TypeId,
    /// Type-parameter names the alias abstracts over, in first-appearance
    /// order — the order a subscripted use (`Result[int, str]`) supplies them.
    params: Vec<String>,
}

/// What the builder knows about a module's source before lowering it.
pub(crate) struct ModuleAnalysis {
    /// `ty`-resolved types (empty unless the `ty` feature is on).
    pub(crate) resolved_types: crate::ty_resolve::ResolvedTypes,
    /// Module-scope and class-body bindings (see [`ModuleBindings`]).
    pub(crate) bindings: ModuleBindings,
}

impl ModuleAnalysis {
    /// Analyse `module`, whose `ty` resolution is `resolved_types`.
    pub(crate) fn new(resolved_types: crate::ty_resolve::ResolvedTypes, module: &ModModule) -> Self {
        Self {
            resolved_types,
            bindings: ModuleBindings::collect(&module.body),
        }
    }
}

/// Stateful Python-module lowering context.
pub(crate) struct ModuleBuilder<'ctx> {
    /// Source file ID for spans created while lowering.
    file_id: FileId,
    /// Source path used for module metadata and diagnostics.
    path: String,
    /// Whether this module is the program entry (`__name__ == "__main__"`).
    pub(crate) is_entry: bool,
    /// Shared HIR context being populated.
    ctx: &'ctx mut HirCtx,
    /// State owned by the function body being lowered; swapped wholesale by
    /// [`Self::with_fn_scope`].
    scope: FnScope,
    /// Module-level items (functions / classes) for call resolution.
    items: HashMap<String, ItemId>,
    /// Names exported by the module currently being lowered.
    exports: HashMap<String, ItemId>,
    /// The type parameters of the function whose signature and body are being
    /// lowered (its PEP 695 `[T]`, and the module `TypeVar`s its annotations
    /// name): inside it, such a name is a [`Type::TypeParam`].
    function_type_params: Vec<String>,
    /// The type-parameter solution of the last generic call whose arguments
    /// were lowered (see `callable_call_args`), for its result type.
    last_call_substitutions: HashMap<String, TypeId>,
    /// Top-level bindings lowered as module globals, by name; see
    /// `lowering/module_variables.rs`.
    module_variables: HashMap<String, ModuleVariable>,
    /// Items synthesized while lowering the module body (global
    /// initializers), added to the module when it is done.
    pending_module_items: Vec<ItemId>,
    /// Integer enum members keyed by class name, then member name.
    enum_members: HashMap<String, HashMap<String, i64>>,
    /// Instance-method items keyed by class name, then method name.
    ///
    /// Populated by a per-class pre-pass ([`Self::reserve_class_method_slots`])
    /// *before* any method body is lowered, so a method that calls an instance
    /// sibling declared later (`self.helper()`) can still resolve it.
    class_methods: HashMap<String, HashMap<String, ItemId>>,
    /// Class `@staticmethod` and `@classmethod` items keyed by class name, then
    /// method name.
    ///
    /// Both dispatch receiver-free (`Class::method(args)`), so they are kept
    /// separate from the instance-method [`Self::class_methods`] registry. This
    /// also lets a forward `cls.helper()` / `Class.helper()` call resolve the
    /// method before the class item's `static_methods` field is finalized.
    class_static_methods: HashMap<String, HashMap<String, ItemId>>,
    /// Class fields keyed by class name while a class body is being lowered.
    class_fields: HashMap<String, Vec<Field>>,
    /// Module-level type aliases (`Result: TypeAlias = Ok | Err`) by source name.
    type_aliases: HashMap<String, TypeAliasDef>,
    /// Names declared by `TypeVar`/`ParamSpec`/`TypeVarTuple`, in source order.
    type_param_names: Vec<String>,
    /// Imported module/package namespaces available by local module name.
    module_namespaces: HashMap<String, HashMap<String, ItemId>>,
    /// What each imported local name is bound to, as a fully qualified name.
    import_aliases: HashMap<String, String>,
    /// Pytest fixture functions available to tests in this module.
    pytest_fixtures: IndexMap<String, PytestFixture>,
    /// Whether this module should use pytest-specific test discovery rules.
    pytest_mode: bool,
    /// Variadic parameter metadata for top-level Python function items.
    function_variadics: HashMap<String, FunctionVariadics>,
    /// Materialized final bindings for this source module.
    specialization: Option<SpecializationData>,
    /// `ty`-resolved types for this module (empty unless the `ty` feature is on).
    resolved_types: crate::ty_resolve::ResolvedTypes,
    /// Module-scope and class-body bindings, used to resolve what a
    /// decorator, base class or metaclass expression denotes.
    bindings: ModuleBindings,
    /// User exception classes of this module, each with its chain: its own
    /// name, then each base up to `BaseException`.
    exception_classes: HashMap<String, Vec<String>>,
    /// The caught-exception local of each enclosing `except` clause, innermost
    /// last, which a bare `raise` re-raises.
    handled_exceptions: Vec<smelt_hir::LocalId>,
    /// Whether the module body awaits (`asyncio.run`), which makes it async.
    module_awaits: bool,
}

/// What kind of body the lowering cursor is in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum FnKind {
    /// Module top level (and class bodies): no enclosing function.
    #[default]
    Module,
    /// A `def` body.
    Sync,
    /// An `async def` body: the only place `await` is legal.
    Async,
}

impl FnKind {
    /// The kind of a `def` (`is_async == false`) or `async def` body.
    const fn of(is_async: bool) -> Self {
        if is_async { Self::Async } else { Self::Sync }
    }
}

/// Lowering state that belongs to ONE function body.
///
/// Every function, method and synthesized constructor is lowered inside
/// [`ModuleBuilder::with_fn_scope`], which installs a fresh `FnScope` and puts
/// the enclosing one back on every exit, error included. Before, each site
/// saved and restored the fields by hand and an early `?` (a failed parameter
/// annotation, a rejected statement) skipped the restore: the failed
/// function's locals and async-ness leaked into every item and module
/// statement lowered after it -- a module-level `print(x)` after
/// `def f(x: int, y: <bad>)` resolved `x` to the dead function's local and
/// panicked indexing the module body.
#[derive(Debug, Default)]
struct FnScope {
    /// In-scope locals by source name.
    locals: HashMap<String, smelt_hir::LocalId>,
    /// The kind of body being lowered.
    kind: FnKind,
    /// Call-packing metadata of callable locals, by name.
    local_callbacks: HashMap<String, LocalCallback>,
    /// Module variables this function declared `global`: its writes of them
    /// store the module globals.
    globals: HashSet<String>,
}

/// How a call site packs its arguments for a callable local: what a `Callable`
/// type alone cannot say about a lambda or nested `def` bound to that name.
#[derive(Debug, Clone)]
struct LocalCallback {
    /// Default argument expressions in source order, in the defining body.
    defaults: Vec<Option<smelt_hir::ExprId>>,
    /// Positional vararg metadata when the source closure uses `*args`.
    vararg: Option<VarArgParam>,
    /// Keyword vararg metadata when the source closure uses `**kwargs`.
    kwarg: Option<KwArgParam>,
}

/// What a Python arithmetic operator lowers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PythonArithmetic {
    /// An operator whose meaning a plain HIR binary op already has.
    Plain(BinOp),
    /// `/`, `//` or `%`, with Python's division rules.
    Division(PythonDivisionOp),
}

/// A Python `*args` parameter represented as one list argument.
#[derive(Debug, Clone, Copy)]
struct VarArgParam {
    /// Parameter index of the packed positional list in the lowered closure.
    index: usize,
    /// Element type accepted by each extra source-language positional argument.
    item_ty: TypeId,
}

/// A Python `**kwargs` parameter represented as one dictionary argument.
#[derive(Debug, Clone, Copy)]
struct KwArgParam {
    /// Parameter index of the packed keyword dictionary in the lowered closure.
    index: usize,
    /// Value type accepted by each source-language keyword argument.
    value_ty: TypeId,
}

/// Python top-level function variadic parameters represented after lowering.
#[derive(Debug, Clone, Copy, Default)]
struct FunctionVariadics {
    /// Positional `*args` packing metadata.
    vararg: Option<VarArgParam>,
    /// Keyword `**kwargs` packing metadata.
    kwarg: Option<KwArgParam>,
}

/// Static Python callable signature used for call argument lowering.
struct CallableCallSignature<'a> {
    /// Parameter types in lowered call order.
    params: &'a [TypeId],
    /// Source parameter names when the callable declaration is available.
    param_names: Option<&'a [String]>,
    /// Positional vararg metadata when source `*args` are packed.
    vararg: Option<VarArgParam>,
    /// Keyword vararg metadata when source `**kwargs` are packed.
    kwarg: Option<KwArgParam>,
    /// Default argument expressions in lowered parameter order.
    defaults: &'a [Option<smelt_hir::ExprId>],
    /// Diagnostic label for this kind of callable.
    label: &'a str,
    /// The callee's own type parameters, by name, which each call solves
    /// from its arguments (`first([1])` binds `T` to `int`); empty when the
    /// callee is not generic.
    type_params: &'a [String],
}

/// A closure expression prepared for a callback-consuming Python API.
#[derive(Debug, Clone, Copy)]
pub(super) struct ClosureCallback {
    /// HIR expression id of the closure value.
    pub(super) expr: smelt_hir::ExprId,
    /// Return type produced by the closure.
    pub(super) return_ty: TypeId,
}

/// A simple pytest parametrization table.
#[derive(Debug, Clone)]
struct PytestParametrize {
    /// Parameter names in order.
    names: Vec<String>,
    /// Literal rows in order.
    rows: Vec<Vec<PytestLiteral>>,
}

/// Lowered metadata for a pytest fixture function.
#[derive(Debug, Clone, Copy)]
struct PytestFixture {
    /// HIR item for the fixture function.
    item: ItemId,
    /// Return type produced by the fixture.
    ty: TypeId,
    /// Whether this fixture should be called for every pytest test in the module.
    autouse: bool,
}

/// Arguments needed to lower a statically-known protocol method call.
struct ProtocolMethodCall {
    /// Expression used as the method receiver.
    receiver: smelt_hir::ExprId,
    /// Python method name to call.
    method: String,
    /// Already-lowered call arguments.
    args: Vec<smelt_hir::ExprId>,
}

/// Literal values supported in first-pass pytest parametrization.
#[derive(Debug, Clone)]
enum PytestLiteral {
    /// Boolean literal.
    Bool(bool),
    /// Integer literal.
    Int(i64),
    /// Floating-point literal.
    Float(f64),
    /// String literal.
    String(String),
    /// `None`.
    None,
    /// Tuple literal.
    Tuple(Vec<Self>),
    /// List literal.
    List(Vec<Self>),
}

impl<'ctx> ModuleBuilder<'ctx> {
    /// Build a lowering context for one Python source module.
    ///
    /// Everything the builder reads about the module before lowering it -- the
    /// specialization data and the `analysis` of its source -- is an argument
    /// here, so no field is filled in after construction.
    pub(crate) fn new(
        file_id: FileId,
        path: String,
        ctx: &'ctx mut HirCtx,
        specialization: Option<SpecializationData>,
        analysis: ModuleAnalysis,
    ) -> Self {
        let items = visible_items(ctx);
        let enum_members = ctx.enum_members.clone();
        let class_methods = visible_class_methods(ctx);
        let class_static_methods = visible_class_static_methods(ctx);
        let module_namespaces = ctx.module_namespaces.clone();
        Self {
            file_id,
            pytest_mode: test_support::is_pytest_file(&path),
            path,
            is_entry: false,
            ctx,
            scope: FnScope::default(),
            items,
            exports: HashMap::new(),
            module_variables: HashMap::new(),
            last_call_substitutions: HashMap::new(),
            function_type_params: Vec::new(),
            pending_module_items: Vec::new(),
            enum_members,
            class_methods,
            class_static_methods,
            class_fields: HashMap::new(),
            type_aliases: HashMap::new(),
            type_param_names: Vec::new(),
            module_namespaces,
            import_aliases: HashMap::new(),
            pytest_fixtures: IndexMap::new(),
            function_variadics: HashMap::new(),
            specialization,
            resolved_types: analysis.resolved_types,
            bindings: analysis.bindings,
            exception_classes: HashMap::new(),
            handled_exceptions: Vec::new(),
            module_awaits: false,
        }
    }

    /// Lower one function body in a fresh [`FnScope`] of `kind`.
    ///
    /// The enclosing scope is restored when `lower` returns, whatever it
    /// returns, so a failing body cannot leak its locals, its async-ness or its
    /// callbacks into what is lowered next.
    fn with_fn_scope<T>(&mut self, kind: FnKind, lower: impl FnOnce(&mut Self) -> T) -> T {
        let enclosing = std::mem::replace(
            &mut self.scope,
            FnScope {
                kind,
                ..FnScope::default()
            },
        );
        let result = lower(self);
        self.scope = enclosing;
        result
    }

    /// Lower a nested block whose bindings must not outlive it (an `except`
    /// handler, a comprehension): the visible locals are restored when `lower`
    /// returns, whatever it returns.
    fn with_block_scope<T>(&mut self, lower: impl FnOnce(&mut Self) -> T) -> T {
        let enclosing = self.scope.locals.clone();
        let result = lower(self);
        self.scope.locals = enclosing;
        result
    }

    // -----------------------------------------------------------------------
    // Module — two-pass lowering
    // -----------------------------------------------------------------------

    /// Lower a parsed Python module into HIR.
    pub(crate) fn module(&mut self, module: &ModModule) -> Result<ModuleId, Vec<SmeltError>> {
        let source = SourceFile {
            path: self.path.clone(),
            language: Language::Python,
            file: self.file_id,
        };
        let mut hir_module = Module::new("main", source);
        let module_span = self.span(module.range);
        let mut body = Body::new(None, module_span);
        let mut errors: Vec<SmeltError> = Vec::new();

        if self.specialization.is_none()
            && let Some((span, name)) = self.required_specialization_definition(module)
        {
            return Err(vec![SmeltError::specialization_required(span, name)]);
        }
        self.replay_unattributed_output(&mut body);
        self.collect_exception_classes(module);

        // Pass 1 — collect top-level function/class declarations so later
        // statements can reference them in calls. Each statement pass 1 takes
        // care of (successfully or not) is recorded, and pass 2 skips exactly
        // those: a second predicate re-deciding "was this declared?" against
        // the *final* item table could disagree with what pass 1 actually did.
        // Pass 0 — module variables, so functions lowered in pass 1 read and
        // write them as module globals.
        self.declare_module_variables(module, &mut hir_module);
        let mut declared = vec![false; module.body.len()];
        for (slot, stmt) in declared.iter_mut().zip(&module.body) {
            *slot = self.declare_module_stmt(stmt, module, &mut hir_module, &mut errors);
        }
        self.register_module_namespace(&hir_module);

        // Pass 2 — lower the remaining module-level statements into the
        // module body.
        for (stmt, is_declared) in module.body.iter().zip(declared) {
            if is_declared {
                // A constructed binding Pass 1 lifted to a module global is
                // touched here, so its initializer runs in source order (and
                // prints its own output). Any other declared statement is
                // materialized, not run, so what it printed at definition time
                // is replayed in its place.
                if let Some(touch) = self.module_global_touch(stmt, &mut body) {
                    body.push_stmt(HirStmt::Expr(touch));
                } else {
                    self.replay_statement_output(stmt.range(), &mut body);
                }
                continue;
            }
            if let Err(err) = self.statement(stmt, &mut body) {
                errors.push(err);
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }
        hir_module.items.append(&mut self.pending_module_items);

        if self.module_awaits {
            hir_module.is_async = true;
            // MIR requires an async body's await-point metadata.
            body.finish_async();
        }
        let body_id = self.ctx.krate.push_body(body);
        hir_module.body = Some(body_id);
        Ok(self.ctx.krate.push_module(hir_module))
    }
}

impl ModuleBuilder<'_> {
    /// Pass 1 of [`Self::module`] for one top-level statement: lower it if it
    /// is a declaration later statements may reference, recording a failure in
    /// `errors`.
    ///
    /// Returns whether the statement is fully handled by this pass (whether or
    /// not lowering it succeeded), i.e. whether pass 2 must skip it.
    fn declare_module_stmt(
        &mut self,
        stmt: &Stmt,
        module: &ModModule,
        hir_module: &mut Module,
        errors: &mut Vec<SmeltError>,
    ) -> bool {
        match stmt {
            Stmt::FunctionDef(func) => {
                if !self.is_materialized_decorator_factory(func, module) {
                    match self.specialized_function_defs(func, module) {
                        Ok(item_ids) => hir_module.items.extend(item_ids),
                        Err(err) => errors.push(err),
                    }
                }
                true
            }
            Stmt::ClassDef(class) if self.exception_classes.contains_key(class.name.as_str()) => {
                if let Err(err) = self.exception_class_def(class) {
                    errors.push(err);
                }
                true
            }
            Stmt::ClassDef(class) => {
                if !self.is_materialized_metaclass_definition(class, module)
                    && let Err(err) = self.class_def(class, module, hir_module)
                {
                    errors.push(err);
                }
                true
            }
            Stmt::Import(import) => {
                self.import_stmt(import, hir_module);
                true
            }
            Stmt::ImportFrom(import) => {
                self.import_from_stmt(import, hir_module);
                true
            }
            // `__all__` is export metadata; it lowers to nothing.
            _ if is_module_all_assignment(stmt) => true,
            _ if let Some(name) = type_param_declaration_name(stmt) => {
                // Recorded so a parameterised alias can order its parameters.
                if !self.type_param_names.iter().any(|known| known == name) {
                    self.type_param_names.push(name.to_owned());
                }
                true
            }
            // A type alias declares no runtime binding; see
            // `register_type_alias_statement`.
            _ if self.register_type_alias_statement(stmt) => true,
            Stmt::Assign(assign) => match self.constructed_constant_assign(assign, hir_module) {
                Ok(handled) => handled,
                Err(err) => {
                    errors.push(err);
                    true
                }
            },
            _ => false,
        }
    }
}

include!("lowering/module_imports.rs");
// Top-level bindings as module globals.
include!("lowering/module_variables.rs");
// Default parameter values, applied by the callee.
include!("lowering/default_params.rs");
// Host-runtime specialization manifest consumption.
include!("lowering/specialization.rs");
include!("lowering/class.rs");
// Class initializer synthesis helpers.
include!("lowering/class_init.rs");
// Function signature and pytest parametrization helpers.
include!("lowering/function.rs");
// Pytest-specific lowering helpers.
include!("lowering/pytest.rs");
// Type annotation lowering helpers.
include!("lowering/types.rs");
include!("lowering/statement.rs");
include!("lowering/control_flow.rs");
// Exception classes, `raise` and `try`/`except`.
include!("lowering/exceptions.rs");
// Match statement and block lowering helpers.
include!("lowering/match.rs");
// Literal and base expression lowering helpers.
include!("lowering/literals.rs");
include!("lowering/call.rs");
// Collection constructor and list/set/dict helpers.
include!("lowering/list.rs");

// Python lambdas and nested `def`s as closures.
include!("lowering/closure.rs");
// Numeric/math operation lowering helpers.
include!("lowering/math.rs");
// String formatting and manipulation lowering helpers.
include!("lowering/format.rs");
// Map/dict and module-call lowering helpers.
include!("lowering/map.rs");
// Member access and constant import lowering helpers.
include!("lowering/member.rs");
// Type inference, built-in item, and interning helpers.
include!("lowering/type_helpers.rs");
// Validation and module visibility helpers.
include!("lowering/validate.rs");
