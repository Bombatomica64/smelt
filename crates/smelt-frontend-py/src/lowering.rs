//! Python AST to smelt HIR lowering.
#![allow(
    clippy::redundant_pub_crate,
    reason = "the crate root constructs the lowering builder"
)]

mod stdlib;
mod stdlib_dispatch;

use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::path::Path;

use ruff_python_ast::{
    BoolOp, CmpOp, ElifElseClause, Expr, ModModule, Number, Operator, Pattern as RuffPattern,
    PatternMatchAs, Singleton, Stmt, StmtAnnAssign, StmtAssert, StmtAugAssign, StmtClassDef,
    StmtFor, StmtFunctionDef, StmtIf, StmtImport, StmtImportFrom, StmtMatch, StmtWith,
    UnaryOp as RuffUnaryOp,
};
use ruff_text_size::{Ranged, TextRange};
use smelt_hir::{
    AsyncOp, BinOp, Body, CallbackExpr, CallbackExprKind, CaptureMode, Class, ClassKind,
    ClosureCapture, ConstItem, DictProjectionOp, Expr as HirExpr, ExprKind, Field, FileId,
    Function, FunctionOwner, FunctionType, Import, Item, ItemId, Language, ListCallbackOp, Literal,
    LocalDecl, MatchArm, MethodSig, Module, ModuleId, NumericExtremaOp, NumericPredicateOp,
    NumericRoundOp, NumericUnaryFuncOp, Param, ParamSig, Pattern as HirPattern, PrimitiveCastOp,
    RegexMatchOp, SetProjectionOp, SourceFile, Span, Stmt as HirStmt, StringAffixOp, StringCaseOp,
    StringPredicateOp, StringReplaceOp, StringSearchOp, StringTrimSide, Symbol, Type, TypeId,
    TypeParamDef, UnaryOp, Visibility,
};
use smelt_stdlib::RuleId;

use crate::helpers::{
    collect_bitor_parts, decorator_frozen_kwarg, decorator_simple_name, expr_kind_name,
    expr_simple_name, expr_type_name, is_django_model_base, range_to_span, stmt_kind_name,
    two_type_args,
};
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
    /// Ordered deterministic effects replayed once by the first manifest module.
    effects: Vec<smelt_specialize::EffectReplay>,
}

/// Stateful Python-module lowering context.
pub(crate) struct ModuleBuilder<'ctx> {
    /// Source file ID for spans created while lowering.
    file_id: FileId,
    /// Source path used for module metadata and diagnostics.
    path: String,
    /// Shared HIR context being populated.
    ctx: &'ctx mut HirCtx,
    /// In-scope locals while lowering a body.
    locals: HashMap<String, smelt_hir::LocalId>,
    /// Module-level items (functions / classes) for call resolution.
    items: HashMap<String, ItemId>,
    /// Names exported by the module currently being lowered.
    exports: HashMap<String, ItemId>,
    /// Integer enum members keyed by class name, then member name.
    enum_members: HashMap<String, HashMap<String, i64>>,
    /// Class method items keyed by class name, then method name.
    class_methods: HashMap<String, HashMap<String, ItemId>>,
    /// Class fields keyed by class name while a class body is being lowered.
    class_fields: HashMap<String, Vec<Field>>,
    /// Imported module/package namespaces available by local module name.
    module_namespaces: HashMap<String, HashMap<String, ItemId>>,
    /// Pytest fixture functions available to tests in this module.
    pytest_fixtures: HashMap<String, PytestFixture>,
    /// Whether the current lowered function body is async.
    current_async: bool,
    /// Whether this module should use pytest-specific test discovery rules.
    pytest_mode: bool,
    /// Local lambda callbacks with explicit callable annotations.
    local_callbacks: HashMap<String, LocalCallback>,
    /// Variadic parameter metadata for top-level Python function items.
    function_variadics: HashMap<String, FunctionVariadics>,
    /// Materialized final bindings for this source module.
    specialization: Option<SpecializationData>,
    /// `ty`-resolved types for this module (empty unless the `ty` feature is on).
    resolved_types: crate::ty_resolve::ResolvedTypes,
}

/// A local Python lambda value that can be consumed by callback APIs without escaping.
#[derive(Debug, Clone)]
struct LocalCallback {
    /// Lowered callback expression tree.
    callback: CallbackExpr,
    /// Parameter types in source order.
    params: Vec<TypeId>,
    /// Default argument expressions in source order.
    defaults: Vec<Option<smelt_hir::ExprId>>,
    /// Positional vararg metadata when the source closure uses `*args`.
    vararg: Option<VarArgParam>,
    /// Keyword vararg metadata when the source closure uses `**kwargs`.
    kwarg: Option<KwArgParam>,
    /// Return type produced by the callback.
    return_ty: TypeId,
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
    /// Positional vararg metadata when source `*args` are packed.
    vararg: Option<VarArgParam>,
    /// Keyword vararg metadata when source `**kwargs` are packed.
    kwarg: Option<KwArgParam>,
    /// Default argument expressions in lowered parameter order.
    defaults: &'a [Option<smelt_hir::ExprId>],
    /// Diagnostic label for this kind of callable.
    label: &'a str,
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
    pub(crate) fn new(
        file_id: FileId,
        path: String,
        ctx: &'ctx mut HirCtx,
        specialization: Option<SpecializationData>,
    ) -> Self {
        let items = visible_items(ctx);
        let enum_members = ctx.enum_members.clone();
        let class_methods = visible_class_methods(ctx);
        let module_namespaces = ctx.module_namespaces.clone();
        Self {
            file_id,
            pytest_mode: test_support::is_pytest_file(&path),
            path,
            ctx,
            locals: HashMap::new(),
            items,
            exports: HashMap::new(),
            enum_members,
            class_methods,
            class_fields: HashMap::new(),
            module_namespaces,
            pytest_fixtures: HashMap::new(),
            current_async: false,
            local_callbacks: HashMap::new(),
            function_variadics: HashMap::new(),
            specialization,
            resolved_types: crate::ty_resolve::ResolvedTypes::disabled(),
        }
    }

    /// Install the `ty`-resolved module types used to fill in missing
    /// annotations. Called once, immediately after construction.
    pub(crate) fn set_resolved_types(&mut self, resolved: crate::ty_resolve::ResolvedTypes) {
        self.resolved_types = resolved;
    }

    // -----------------------------------------------------------------------
    // Module — two-pass lowering
    // -----------------------------------------------------------------------

    /// Lower a parsed Python module into HIR.
    pub(crate) fn module(&mut self, module: &ModModule) -> Result<ModuleId, Vec<SmeltError>> {
        let source = SourceFile {
            path: self.path.clone(),
            language: Language::Python,
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
        self.replay_specialization_output(&mut body);

        // Pass 1 — collect top-level function/class declarations so later
        // statements can reference them in calls.
        for stmt in &module.body {
            if let Stmt::FunctionDef(func) = stmt {
                if self.is_materialized_decorator_factory(func, module) {
                    continue;
                }
                match self.specialized_function_defs(func, module) {
                    Ok(item_ids) => hir_module.items.extend(item_ids),
                    Err(err) => errors.push(err),
                }
            } else if let Stmt::ClassDef(class) = stmt {
                if self.is_materialized_metaclass_definition(class, module) {
                    continue;
                }
                match self.class_def(class, module, &mut hir_module) {
                    Ok(()) => {}
                    Err(err) => errors.push(err),
                }
            } else if let Stmt::Import(import) = stmt {
                self.import_stmt(import, &mut hir_module);
            } else if let Stmt::ImportFrom(import) = stmt {
                self.import_from_stmt(import, &mut hir_module);
            } else if let Stmt::Assign(assign) = stmt {
                match self.constructed_constant_assign(assign, &mut hir_module) {
                    Ok(()) => {}
                    Err(err) => errors.push(err),
                }
            }
        }
        self.register_module_namespace(&hir_module);

        // Pass 2 — lower module-level statements into the module body.
        for stmt in &module.body {
            if matches!(
                stmt,
                Stmt::FunctionDef(_) | Stmt::ClassDef(_) | Stmt::Import(_) | Stmt::ImportFrom(_)
            ) || is_module_all_assignment(stmt)
                || self.is_constructed_constant_assignment(stmt)
            {
                continue; // already lowered in Pass 1
            }
            if let Err(err) = self.statement(stmt, &mut body) {
                errors.push(err);
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        let body_id = self.ctx.krate.push_body(body);
        hir_module.body = Some(body_id);
        Ok(self.ctx.krate.push_module(hir_module))
    }
}

// Module/import lowering helpers.
include!("lowering/module_imports.rs");
// Host-runtime specialization manifest consumption.
include!("lowering/specialization.rs");
// Class lowering helpers.
include!("lowering/class.rs");
// Class initializer synthesis helpers.
include!("lowering/class_init.rs");
// Function signature and pytest parametrization helpers.
include!("lowering/function.rs");
// Pytest-specific lowering helpers.
include!("lowering/pytest.rs");
// Type annotation lowering helpers.
include!("lowering/types.rs");
// Statement lowering helpers.
include!("lowering/statement.rs");
// Control-flow statement lowering helpers.
include!("lowering/control_flow.rs");
// Match statement and block lowering helpers.
include!("lowering/match.rs");
// Literal and base expression lowering helpers.
include!("lowering/literals.rs");
// Call lowering helpers.
include!("lowering/call.rs");
// Collection constructor and list/set/dict helpers.
include!("lowering/list.rs");
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
