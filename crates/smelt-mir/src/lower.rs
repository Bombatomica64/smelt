//! Lowering from HIR to MIR.
//!
//! This module contains the logic for converting a HIR crate into a MIR representation,
//! including lowering expressions, statements, and control flow.

use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;

use smelt_hir::{
    Body, BodyId, ExprId, ExprKind, Literal as HirLiteral, LocalId as HirLocalId, Span,
    Stmt as HirStmt, Symbol, Type, TypeId,
};

use crate::{
    BasicBlock, BlockId, BuiltinFn, Callee, ClosureId, Constant, ExceptionHandler, FuncId,
    HirOrigin, LocalDecl, LocalId, LocalKind, Mir, MirClosure, MirFunction, MirListSpliceItem,
    Operand, Place, Rvalue, Statement, Terminator,
};

/// Closure ABI ownership: construction, escape analysis, and throwing widening.
mod closures;
/// Whole-program MIR analyses that run after core HIR lowering.
mod passes;

/// Converts a `usize` into `u32`, panicking if it does not fit.
///
/// # Panics
///
/// Panics when `value` does not fit in `u32`.
fn u32_from_usize(value: usize, context: &str) -> Result<u32, LowerError> {
    u32::try_from(value).map_err(|error| LowerError {
        message: format!("{context}: {error}"),
        span: None,
    })
}

/// Converts a `u32` into `usize`, panicking if it does not fit.
///
/// # Panics
///
/// Panics when `value` does not fit in `usize`.
fn usize_from_u32(value: u32, context: &str) -> Result<usize, LowerError> {
    usize::try_from(value).map_err(|error| LowerError {
        message: format!("{context}: {error}"),
        span: None,
    })
}

/// Converts a local ID into an optional slice index.
fn local_index(local: LocalId) -> Option<usize> {
    usize::try_from(local.0).ok()
}

/// Converts a closure ID into an optional slice index.
fn closure_id_index(closure: ClosureId) -> Option<usize> {
    usize::try_from(closure.0).ok()
}

/// Finds the HIR function item that owns `body_id`, if any.
fn function_item_for_body(
    krate: &smelt_hir::Crate,
    body_id: BodyId,
) -> Option<&smelt_hir::Function> {
    krate.items.iter().find_map(|item| {
        let smelt_hir::Item::Function(function) = item else {
            return None;
        };
        (function.body == Some(body_id)).then_some(function)
    })
}

/// An error encountered during HIR to MIR lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LowerError {
    /// The error message.
    pub message: String,
    /// The source span associated with the error, if available.
    pub span: Option<Span>,
}

/// Function lowering output with any closures discovered inside the function body.
type LoweredFunction = (MirFunction, Vec<MirClosure>);

/// Original place and temporary local used for collection mutation writeback.
type MutationWriteback = Option<(Place, LocalId)>;

/// Lowered collection mutation receiver and its optional writeback.
type LoweredMutationReceiver = (Operand, MutationWriteback);

/// Lowers a HIR crate to MIR, or returns lowering errors if the conversion fails.
pub fn lower_hir(krate: &smelt_hir::Crate) -> Result<Mir, Vec<LowerError>> {
    let hir_errors = smelt_hir::validate(krate);
    if !hir_errors.is_empty() {
        return Err(hir_errors
            .into_iter()
            .map(|error| LowerError {
                message: error.message,
                span: None,
            })
            .collect());
    }

    let mut mir = Mir::new(
        krate.types.clone(),
        krate.symbols.clone(),
        krate.names.clone(),
    );
    let none = mir.types.intern(Type::None);
    let loop_index_ty = mir.types.intern(Type::Float);
    let loop_bool_ty = mir.types.intern(Type::Bool);
    let mut errors = Vec::new();
    let mut item_functions = HashMap::new();

    for (idx, hir_item) in krate.items.iter().enumerate() {
        let item_id = smelt_hir::ItemId(
            match u32_from_usize(idx, "HIR item index does not fit in u32") {
                Ok(value) => value,
                Err(error) => return Err(vec![error]),
            },
        );
        if let smelt_hir::Item::Function(function) = hir_item
            && function.body.is_some()
        {
            let function_id = match u32_from_usize(
                item_functions.len(),
                "MIR function index does not fit in u32",
            ) {
                Ok(value) => value,
                Err(error) => return Err(vec![error]),
            };
            item_functions.insert(item_id, FuncId(function_id));
        }
    }

    for hir_item in &krate.items {
        match hir_item {
            smelt_hir::Item::Class(class) => {
                mir.classes.push(crate::MirClass {
                    name: class.name,
                    kind: class.kind.clone(),
                    type_params: class.type_params.clone(),
                    is_abstract: matches!(class.kind, smelt_hir::ClassKind::Abstract),
                    base: class.base,
                    base_args: class.base_args.clone(),
                    fields: class
                        .fields
                        .iter()
                        .map(|field| crate::MirField {
                            name: field.name,
                            ty: field.ty,
                            visibility: field.visibility,
                        })
                        .collect(),
                    constructor: class
                        .constructor
                        .and_then(|item_id| item_functions.get(&item_id).copied()),
                    methods: class
                        .methods
                        .iter()
                        .filter_map(|method_item| item_functions.get(method_item).copied())
                        .collect(),
                    abstract_methods: class.abstract_methods.clone(),
                    implements: class.implements.clone(),
                });
            }
            smelt_hir::Item::Interface(interface) => {
                let fields = lower_effective_interface_fields(
                    krate,
                    &mut mir,
                    interface,
                    &HashMap::new(),
                    &mut HashSet::new(),
                );
                mir.interfaces.push(crate::MirInterface {
                    name: interface.name,
                    type_params: interface.type_params.clone(),
                    extends: interface.extends.clone(),
                    fields: fields
                        .iter()
                        .map(|field| crate::MirField {
                            name: field.name,
                            ty: field.ty,
                            visibility: field.visibility,
                        })
                        .collect(),
                    methods: interface.methods.clone(),
                });
            }
            smelt_hir::Item::Function(_)
            | smelt_hir::Item::TypeAlias(_)
            | smelt_hir::Item::Const(_) => {}
        }
    }

    for (idx, item) in krate.items.iter().enumerate() {
        let item_id = smelt_hir::ItemId(
            match u32_from_usize(idx, "HIR item index does not fit in u32") {
                Ok(value) => value,
                Err(error) => {
                    errors.push(error);
                    continue;
                }
            },
        );
        let smelt_hir::Item::Function(function) = item else {
            continue;
        };
        let Some(body_id) = function.body else {
            continue;
        };
        let Some(body) = krate.bodies.get(
            match usize_from_u32(body_id.0, "HIR body index does not fit in usize") {
                Ok(value) => value,
                Err(error) => {
                    errors.push(error);
                    continue;
                }
            },
        ) else {
            continue;
        };
        let return_ty = if function.is_async {
            future_inner_type(krate, function.return_ty).unwrap_or(function.return_ty)
        } else {
            function.return_ty
        };
        match LoweringCtx::new(
            krate,
            &item_functions,
            match item_functions.get(&item_id).copied() {
                Some(function_id) => function_id,
                None => continue,
            },
            body_id,
            body,
            function.name,
            return_ty,
            function.owner,
            function.is_async,
            loop_index_ty,
            loop_bool_ty,
            mir.closures.len(),
        )
        .and_then(LoweringCtx::lower)
        {
            Ok((lowered_function, closures)) => {
                mir.closures.extend(closures);
                mir.push_function(lowered_function);
            }
            Err(error) => {
                errors.push(error);
                break;
            }
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    for module in &krate.modules {
        let Some(body_id) = module.body else {
            continue;
        };
        if module_has_test_items(krate, module) && body_is_empty(krate, body_id) {
            continue;
        }
        let Some(body) = krate.bodies.get(
            match usize_from_u32(body_id.0, "HIR body index does not fit in usize") {
                Ok(value) => value,
                Err(error) => {
                    errors.push(error);
                    continue;
                }
            },
        ) else {
            continue;
        };
        let name = mir.symbols.intern(&module.name);
        let function_id = mir.next_function_id();
        match LoweringCtx::new(
            krate,
            &item_functions,
            function_id,
            body_id,
            body,
            name,
            none,
            smelt_hir::FunctionOwner::Module,
            false,
            loop_index_ty,
            loop_bool_ty,
            mir.closures.len(),
        )
        .and_then(LoweringCtx::lower)
        {
            Ok((function, closures)) => {
                mir.closures.extend(closures);
                mir.push_function(function);
            }
            Err(error) => errors.push(error),
        }
    }

    if errors.is_empty() {
        // Closure ABI finalization. `closures` owns escape analysis and the
        // throwing-closure type widening; the general function-throwing
        // propagation in `passes::throwing` feeds the widening, which is why the
        // two interleave. See `closures` for the invariants this publishes.
        closures::mark_escaping_closures(&mut mir);
        passes::throwing::propagate_throwing_functions(&mut mir);
        closures::widen_throwing_closure_types(&mut mir);
        passes::throwing::propagate_throwing_functions(&mut mir);
        closures::widen_throwing_closure_types(&mut mir);
        crate::normalize_operational_types(&mut mir);
        Ok(mir)
    } else {
        Err(errors)
    }
}

/// Return interface fields with inherited parent fields substituted and flattened.
///
/// TypeScript interface inheritance is erased at runtime, but generated Rust
/// storage needs a concrete field layout. MIR expands parent fields while
/// lowering HIR so codegen can treat interfaces as regular records without
/// retaining frontend-only heritage lookup tables.
fn lower_effective_interface_fields(
    krate: &smelt_hir::Crate,
    mir: &mut Mir,
    interface: &smelt_hir::Interface,
    substitutions: &HashMap<Symbol, TypeId>,
    seen: &mut HashSet<Symbol>,
) -> Vec<smelt_hir::Field> {
    if !seen.insert(interface.name) {
        return Vec::new();
    }
    let mut fields = Vec::new();
    for heritage in &interface.extends {
        let Some(parent) = find_hir_interface(krate, heritage.parent) else {
            continue;
        };
        let parent_args = heritage
            .args
            .iter()
            .copied()
            .map(|arg| substitute_type_id(mir, arg, substitutions))
            .collect::<Vec<_>>();
        let parent_substitutions = parent
            .type_params
            .iter()
            .zip(parent_args.iter().copied())
            .map(|(param, arg)| (param.name, arg))
            .collect::<HashMap<_, _>>();
        fields.extend(lower_effective_interface_fields(
            krate,
            mir,
            parent,
            &parent_substitutions,
            seen,
        ));
    }
    for source_field in &interface.fields {
        let mut lowered_field = source_field.clone();
        lowered_field.ty = substitute_type_id(mir, lowered_field.ty, substitutions);
        if let Some(existing) = fields
            .iter_mut()
            .find(|candidate: &&mut smelt_hir::Field| candidate.name == lowered_field.name)
        {
            *existing = lowered_field;
        } else {
            fields.push(lowered_field);
        }
    }
    seen.remove(&interface.name);
    fields
}

/// Find a HIR interface item by symbol.
fn find_hir_interface(krate: &smelt_hir::Crate, name: Symbol) -> Option<&smelt_hir::Interface> {
    krate.items.iter().find_map(|item| {
        if let smelt_hir::Item::Interface(interface) = item
            && interface.name == name
        {
            Some(interface)
        } else {
            None
        }
    })
}

/// Apply interface generic substitutions to a type, interning rebuilt shapes.
fn substitute_type_id(
    mir: &mut Mir,
    ty: TypeId,
    substitutions: &HashMap<Symbol, TypeId>,
) -> TypeId {
    match mir.types.get(ty).cloned() {
        Some(Type::TypeParam { name }) => substitutions.get(&name).copied().unwrap_or(ty),
        Some(Type::List(item)) => {
            let substituted_item = substitute_type_id(mir, item, substitutions);
            mir.types.intern(Type::List(substituted_item))
        }
        Some(Type::Set(item)) => {
            let substituted_item = substitute_type_id(mir, item, substitutions);
            mir.types.intern(Type::Set(substituted_item))
        }
        Some(Type::Dict(key, value)) => {
            let substituted_key = substitute_type_id(mir, key, substitutions);
            let substituted_value = substitute_type_id(mir, value, substitutions);
            mir.types
                .intern(Type::Dict(substituted_key, substituted_value))
        }
        Some(Type::Tuple(items)) => {
            let substituted_items = items
                .into_iter()
                .map(|item| substitute_type_id(mir, item, substitutions))
                .collect();
            mir.types.intern(Type::Tuple(substituted_items))
        }
        Some(Type::Optional(item)) => {
            let substituted_item = substitute_type_id(mir, item, substitutions);
            mir.types.intern(Type::Optional(substituted_item))
        }
        Some(Type::Union(items)) => {
            let substituted_items = items
                .into_iter()
                .map(|item| substitute_type_id(mir, item, substitutions))
                .collect();
            mir.types.intern(Type::Union(substituted_items))
        }
        Some(Type::Class { name, args }) => {
            let substituted_args = args
                .into_iter()
                .map(|arg| substitute_type_id(mir, arg, substitutions))
                .collect();
            mir.types.intern(Type::Class {
                name,
                args: substituted_args,
            })
        }
        Some(Type::Function(mut function)) => {
            function.params = function
                .params
                .into_iter()
                .map(|param| substitute_type_id(mir, param, substitutions))
                .collect();
            function.return_ty = substitute_type_id(mir, function.return_ty, substitutions);
            mir.types.intern(Type::Function(function))
        }
        Some(Type::Future(item)) => {
            let substituted_item = substitute_type_id(mir, item, substitutions);
            mir.types.intern(Type::Future(substituted_item))
        }
        Some(
            Type::Bool
            | Type::Int
            | Type::Float
            | Type::String
            | Type::Unknown
            | Type::Never
            | Type::None,
        )
        | None => ty,
    }
}

/// Context for lowering a function body from HIR to MIR.
struct LoweringCtx<'hir> {
    /// Reference to the HIR crate.
    krate: &'hir smelt_hir::Crate,
    /// Mapping of HIR item IDs to MIR function IDs.
    item_functions: &'hir HashMap<smelt_hir::ItemId, FuncId>,
    /// The HIR body being lowered.
    body: &'hir Body,
    /// The MIR function being constructed.
    function: MirFunction,
    /// MIR closures constructed while lowering this function.
    closures: Vec<MirClosure>,
    /// Global closure id offset for closures constructed by this context.
    closure_base: usize,
    /// The current block being generated.
    current_block: BlockId,
    /// Mapping of HIR local IDs to MIR local IDs.
    locals: HashMap<HirLocalId, LocalId>,
    /// Mapping of HIR expression IDs to lowered operands.
    exprs: HashMap<ExprId, Operand>,
    /// Stack of loop targets for break/continue.
    loops: Vec<LoopTargets>,
    /// Stack of lexical exception targets for throws inside try blocks.
    exception_targets: Vec<ExceptionTarget>,
    /// Numeric type used for generated index-based loop counters.
    loop_index_ty: TypeId,
    /// Boolean type used for generated loop conditions.
    loop_bool_ty: TypeId,
}

/// Return whether `module` contains any function marked as a source test.
fn module_has_test_items(krate: &smelt_hir::Crate, module: &smelt_hir::Module) -> bool {
    module.items.iter().any(|item_id| {
        krate
            .items
            .get(usize::try_from(item_id.0).unwrap_or(usize::MAX))
            .is_some_and(
                |item| matches!(item, smelt_hir::Item::Function(function) if function.is_test),
            )
    })
}

/// Return whether a HIR body has no executable statements or tail expression.
fn body_is_empty(krate: &smelt_hir::Crate, body_id: BodyId) -> bool {
    krate
        .bodies
        .get(usize::try_from(body_id.0).unwrap_or(usize::MAX))
        .is_some_and(|body| {
            body.blocks
                .get(usize::try_from(body.root.0).unwrap_or(usize::MAX))
                .is_some_and(|block| block.stmts.is_empty() && block.tail.is_none())
        })
}

/// Target blocks for break and continue statements.
#[derive(Debug, Clone, Copy)]
struct LoopTargets {
    /// Target block for break statements.
    break_target: BlockId,
    /// Target block for continue statements.
    continue_target: BlockId,
}

/// Target block and optional binding for a lexical catch clause.
#[derive(Debug, Clone, Copy)]
struct ExceptionTarget {
    /// Target block for caught throw statements.
    catch_block: BlockId,
    /// Local receiving the thrown value when the catch has a binding.
    exception_local: Option<LocalId>,
}

impl<'hir> LoweringCtx<'hir> {
    /// Creates a new lowering context for a function body.
    fn new(
        krate: &'hir smelt_hir::Crate,
        item_functions: &'hir HashMap<smelt_hir::ItemId, FuncId>,
        function_id: FuncId,
        body_id: BodyId,
        body: &'hir Body,
        name: Symbol,
        return_ty: TypeId,
        owner: smelt_hir::FunctionOwner,
        is_async: bool,
        loop_index_ty: TypeId,
        loop_bool_ty: TypeId,
        closure_base: usize,
    ) -> Result<Self, LowerError> {
        let span = body
            .blocks
            .get(usize_from_u32(
                body.root.0,
                "HIR block index does not fit in usize",
            )?)
            .map(|block| block.span)
            .ok_or_else(|| LowerError {
                message: "HIR block index should be valid".to_owned(),
                span: None,
            })?;
        let origin = match owner {
            smelt_hir::FunctionOwner::Module => HirOrigin::Body(body_id),
            smelt_hir::FunctionOwner::Constructor { class } => HirOrigin::ClassConstructor {
                class,
                body: body_id,
            },
            smelt_hir::FunctionOwner::ClassMethod { class, method } => HirOrigin::ClassMethod {
                class,
                method,
                body: body_id,
            },
        };
        let mut function = MirFunction::new(function_id, name, origin, return_ty, span);
        function.is_async = is_async;
        if let Some(hir_function) = function_item_for_body(krate, body_id) {
            function.is_test = hir_function.is_test;
            function.rest = hir_function.rest;
        }
        let mut locals = HashMap::new();

        for (idx, local) in body.locals.iter().enumerate() {
            let hir_local = HirLocalId(u32_from_usize(idx, "HIR local index does not fit in u32")?);
            let is_param = body.params.contains(&hir_local);
            let kind = if is_param {
                LocalKind::Param { symbol: local.name }
            } else {
                local.name.map_or(LocalKind::Temp, LocalKind::UserBinding)
            };
            let mir_local = function.push_local(LocalDecl {
                ty: local.ty,
                kind,
                span: local.span,
            });
            if is_param {
                function.params.push(mir_local);
            }
            locals.insert(hir_local, mir_local);
        }

        Ok(Self {
            krate,
            item_functions,
            body,
            function,
            closures: Vec::new(),
            closure_base,
            current_block: BlockId(0),
            locals,
            exprs: HashMap::new(),
            loops: Vec::new(),
            exception_targets: Vec::new(),
            loop_index_ty,
            loop_bool_ty,
        })
    }

    /// Creates a lowering context for a nested closure body.
    fn new_closure(
        krate: &'hir smelt_hir::Crate,
        item_functions: &'hir HashMap<smelt_hir::ItemId, FuncId>,
        body_id: BodyId,
        body: &'hir Body,
        return_ty: TypeId,
        span: Span,
        loop_index_ty: TypeId,
        loop_bool_ty: TypeId,
        closure_base: usize,
    ) -> Result<Self, LowerError> {
        let mut function = MirFunction::new(
            FuncId(u32::MAX),
            Symbol(u32::MAX),
            HirOrigin::Body(body_id),
            return_ty,
            span,
        );
        let mut locals = HashMap::new();
        for (idx, local) in body.locals.iter().enumerate() {
            let hir_local = HirLocalId(u32_from_usize(idx, "HIR local index does not fit in u32")?);
            let is_param = body.params.contains(&hir_local);
            let kind = if is_param {
                LocalKind::Param { symbol: local.name }
            } else {
                local.name.map_or(LocalKind::Temp, LocalKind::UserBinding)
            };
            let mir_local = function.push_local(LocalDecl {
                ty: local.ty,
                kind,
                span: local.span,
            });
            if is_param {
                function.params.push(mir_local);
            }
            locals.insert(hir_local, mir_local);
        }

        Ok(Self {
            krate,
            item_functions,
            body,
            function,
            closures: Vec::new(),
            closure_base,
            current_block: BlockId(0),
            locals,
            exprs: HashMap::new(),
            loops: Vec::new(),
            exception_targets: Vec::new(),
            loop_index_ty,
            loop_bool_ty,
        })
    }

    /// Lowers the entire function body to MIR.
    fn lower(mut self) -> Result<LoweredFunction, LowerError> {
        if let HirOrigin::ClassConstructor { class, .. } = self.function.origin {
            let Some(this) = self.function.locals.first().map(|_| LocalId(0)) else {
                return Err(self.error("constructor is missing synthetic this local", None));
            };
            self.block_mut()?.statements.push(Statement::Assign {
                dest: this,
                value: Rvalue::Struct {
                    class,
                    fields: Vec::new(),
                },
            });
        }
        self.lower_block_stmts(self.body.root)?;

        let root = self.hir_block(self.body.root)?;
        if self.block()?.terminator.is_some() {
            return Ok((self.function, self.closures));
        }

        if let Some(tail) = root.tail {
            let operand = self.lower_expr(tail)?;
            self.set_terminator(Terminator::Return(operand))?;
        } else {
            if matches!(self.function.origin, HirOrigin::ClassConstructor { .. }) {
                self.set_terminator(Terminator::Return(Operand::Move(Place::Local(LocalId(0)))))?;
                return Ok((self.function, self.closures));
            }
            self.set_terminator(Terminator::Return(Operand::Const(Constant::None)))?;
        }

        Ok((self.function, self.closures))
    }

    /// Lowers all statements in a HIR block.
    fn lower_block_stmts(&mut self, block_id: smelt_hir::BlockId) -> Result<(), LowerError> {
        let block = self.hir_block(block_id)?;
        let stmt_ids = block.stmts.clone();
        for stmt_id in stmt_ids {
            if self.block()?.terminator.is_some() {
                break;
            }
            let stmt = self.hir_stmt(stmt_id)?.clone();
            self.lower_stmt(&stmt)?;
        }
        Ok(())
    }

    /// Lowers a single HIR statement to MIR.
    fn lower_stmt(&mut self, stmt: &HirStmt) -> Result<(), LowerError> {
        match stmt {
            HirStmt::Let { pat, value, .. } => {
                let hir_local = match self.hir_pattern(*pat)? {
                    smelt_hir::Pattern::Binding(hir_local) => *hir_local,
                    smelt_hir::Pattern::Wildcard
                    | smelt_hir::Pattern::Tuple(_)
                    | smelt_hir::Pattern::Literal(_) => {
                        return Err(
                            self.error("only binding let patterns can lower to MIR yet", None)
                        );
                    }
                };
                let Some(dest) = self.locals.get(&hir_local).copied() else {
                    return Err(self.error("let pattern references an unknown local", None));
                };
                let lowered_value = value
                    .map(|expr| self.lower_expr(expr))
                    .transpose()?
                    .unwrap_or(Operand::Const(Constant::None));
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::Use(lowered_value),
                });
                Ok(())
            }
            HirStmt::Assign { target, value } => {
                let place = self.lower_place(*target)?;
                let lowered_value = self.lower_expr(*value)?;
                self.block_mut()?.statements.push(Statement::AssignPlace {
                    place,
                    value: Rvalue::Use(lowered_value),
                });
                Ok(())
            }
            HirStmt::Expr(expr) => {
                self.lower_expr(*expr)?;
                Ok(())
            }
            HirStmt::Return(Some(expr)) => {
                let lowered_operand = self.lower_expr(*expr)?;
                self.set_terminator(Terminator::Return(lowered_operand))?;
                Ok(())
            }
            HirStmt::Return(None) => {
                self.set_terminator(Terminator::Return(Operand::Const(Constant::None)))?;
                Ok(())
            }
            HirStmt::If {
                cond,
                then_block,
                else_block,
            } => self.lower_if(*cond, *then_block, *else_block),
            HirStmt::Match {
                scrutinee,
                arms,
                default,
            } => self.lower_match(*scrutinee, arms, *default),
            HirStmt::While { cond, body } => self.lower_while(*cond, *body),
            HirStmt::WhileUpdate {
                cond,
                body,
                update_target,
                update_value,
            } => self.lower_while_update(*cond, *body, *update_target, *update_value),
            HirStmt::For { pat, iter, body } => self.lower_for(*pat, *iter, *body),
            HirStmt::Throw(expr) => {
                let lowered_operand = self.lower_expr(*expr)?;
                if let Some(target) = self.exception_targets.last().copied() {
                    if let Some(exception_local) = target.exception_local {
                        self.block_mut()?.statements.push(Statement::Assign {
                            dest: exception_local,
                            value: Rvalue::Use(lowered_operand),
                        });
                    }
                    self.set_terminator(Terminator::Goto(target.catch_block))?;
                } else {
                    self.set_terminator(Terminator::Throw(lowered_operand))?;
                }
                Ok(())
            }
            HirStmt::TryCatch {
                body,
                catch_binding,
                catch_body,
                finally_body,
            } => self.lower_try_catch(*body, *catch_binding, *catch_body, *finally_body),
            HirStmt::Break => {
                let Some(targets) = self.loops.last().copied() else {
                    return Err(self.error("break used outside a loop", None));
                };
                self.set_terminator(Terminator::Goto(targets.break_target))?;
                Ok(())
            }
            HirStmt::Continue => {
                let Some(targets) = self.loops.last().copied() else {
                    return Err(self.error("continue used outside a loop", None));
                };
                self.set_terminator(Terminator::Goto(targets.continue_target))?;
                Ok(())
            }
        }
    }

    /// Lowers try/catch/finally into explicit catch and cleanup blocks.
    fn lower_try_catch(
        &mut self,
        body_hir: smelt_hir::BlockId,
        catch_binding: Option<HirLocalId>,
        catch_body_hir: Option<smelt_hir::BlockId>,
        finally_body_hir: Option<smelt_hir::BlockId>,
    ) -> Result<(), LowerError> {
        let try_span = self.hir_block(body_hir)?.span;
        let try_block = self.function.push_block(try_span);
        let after = self.function.push_block(self.block()?.span);
        let finally_mir_block = finally_body_hir
            .map(|block| {
                let span = self.hir_block(block)?.span;
                Ok(self.function.push_block(span))
            })
            .transpose()?;
        let normal_exit = finally_mir_block.unwrap_or(after);
        let catch_mir_block = catch_body_hir
            .map(|block| {
                let span = self.hir_block(block)?.span;
                Ok(self.function.push_block(span))
            })
            .transpose()?;

        self.set_terminator(Terminator::Goto(try_block))?;

        if let Some(catch_block) = catch_mir_block {
            let exception_local = catch_binding
                .map(|local| {
                    self.locals.get(&local).copied().ok_or_else(|| {
                        self.error("catch binding references an unknown local", None)
                    })
                })
                .transpose()?;
            self.exception_targets.push(ExceptionTarget {
                catch_block,
                exception_local,
            });
        }

        self.current_block = try_block;
        self.lower_block_stmts(body_hir)?;
        if self.block()?.terminator.is_none() {
            self.set_terminator(Terminator::Goto(normal_exit))?;
        }

        if catch_mir_block.is_some() {
            self.exception_targets.pop();
        }

        if let (Some(catch_hir), Some(catch_block)) = (catch_body_hir, catch_mir_block) {
            self.current_block = catch_block;
            self.lower_block_stmts(catch_hir)?;
            if self.block()?.terminator.is_none() {
                self.set_terminator(Terminator::Goto(normal_exit))?;
            }
        }

        if let (Some(finally_hir), Some(finally_block)) = (finally_body_hir, finally_mir_block) {
            self.current_block = finally_block;
            self.lower_block_stmts(finally_hir)?;
            if self.block()?.terminator.is_none() {
                self.set_terminator(Terminator::Goto(after))?;
            }
        }

        self.current_block = after;
        Ok(())
    }

    /// Lowers an if statement to MIR with switch terminator.
    fn lower_if(
        &mut self,
        cond: ExprId,
        then_hir: smelt_hir::BlockId,
        else_hir: Option<smelt_hir::BlockId>,
    ) -> Result<(), LowerError> {
        let lowered_cond = self.lower_expr(cond)?;
        let then_span = self.hir_block(then_hir)?.span;
        let else_span = if let Some(block) = else_hir {
            self.hir_block(block)?.span
        } else {
            self.block()?.span
        };
        let then_mir = self.function.push_block(then_span);
        let else_mir = self.function.push_block(else_span);
        self.set_terminator(Terminator::Switch {
            cond: lowered_cond,
            then_block: then_mir,
            else_block: else_mir,
        })?;

        self.current_block = then_mir;
        self.lower_block_stmts(then_hir)?;

        if let Some(else_block_hir) = else_hir {
            let join = self.function.push_block(self.block()?.span);
            if self.block()?.terminator.is_none() {
                self.set_terminator(Terminator::Goto(join))?;
            }

            self.current_block = else_mir;
            self.lower_block_stmts(else_block_hir)?;
            if self.block()?.terminator.is_none() {
                self.set_terminator(Terminator::Goto(join))?;
            }
            self.current_block = join;
        } else {
            if self.block()?.terminator.is_none() {
                self.set_terminator(Terminator::Goto(else_mir))?;
            }
            self.current_block = else_mir;
        }

        Ok(())
    }

    /// Lowers a while loop to MIR with switch terminator.
    fn lower_while(
        &mut self,
        cond: ExprId,
        body_hir: smelt_hir::BlockId,
    ) -> Result<(), LowerError> {
        let preheader_span = self.block()?.span;
        let header = self.function.push_block(preheader_span);
        self.set_terminator(Terminator::Goto(header))?;
        self.current_block = header;
        let body_span = self.hir_block(body_hir)?.span;
        let body_mir = self.function.push_block(body_span);
        let after = self.function.push_block(preheader_span);
        let lowered_cond = self.lower_expr(cond)?;
        self.set_terminator(Terminator::Switch {
            cond: lowered_cond,
            then_block: body_mir,
            else_block: after,
        })?;

        self.loops.push(LoopTargets {
            break_target: after,
            continue_target: header,
        });
        self.current_block = body_mir;
        self.lower_block_stmts(body_hir)?;
        if self.block()?.terminator.is_none() {
            self.set_terminator(Terminator::Goto(header))?;
        }
        self.loops.pop();
        self.current_block = after;
        Ok(())
    }

    /// Lowers a while loop whose next-condition update must run on `continue`.
    fn lower_while_update(
        &mut self,
        cond: ExprId,
        body_hir: smelt_hir::BlockId,
        update_target: ExprId,
        update_value: ExprId,
    ) -> Result<(), LowerError> {
        let preheader_span = self.block()?.span;
        let header = self.function.push_block(preheader_span);
        self.set_terminator(Terminator::Goto(header))?;
        self.current_block = header;
        let body_span = self.hir_block(body_hir)?.span;
        let body_mir = self.function.push_block(body_span);
        let latch = self.function.push_block(body_span);
        let after = self.function.push_block(preheader_span);
        let lowered_cond = self.lower_expr(cond)?;
        self.set_terminator(Terminator::Switch {
            cond: lowered_cond,
            then_block: body_mir,
            else_block: after,
        })?;

        self.loops.push(LoopTargets {
            break_target: after,
            continue_target: latch,
        });
        self.current_block = body_mir;
        self.lower_block_stmts(body_hir)?;
        if self.block()?.terminator.is_none() {
            self.set_terminator(Terminator::Goto(latch))?;
        }
        self.loops.pop();

        self.current_block = latch;
        let place = self.lower_place(update_target)?;
        let value = self.lower_expr(update_value)?;
        self.block_mut()?.statements.push(Statement::AssignPlace {
            place,
            value: Rvalue::Use(value),
        });
        if self.block()?.terminator.is_none() {
            self.set_terminator(Terminator::Goto(header))?;
        }
        self.current_block = after;
        Ok(())
    }

    /// Lowers a for loop to MIR with index-based iteration.
    fn lower_for(
        &mut self,
        pat: smelt_hir::PatternId,
        iter: ExprId,
        body_hir: smelt_hir::BlockId,
    ) -> Result<(), LowerError> {
        let hir_local = match self.hir_pattern(pat)? {
            smelt_hir::Pattern::Binding(hir_local) => *hir_local,
            smelt_hir::Pattern::Wildcard
            | smelt_hir::Pattern::Tuple(_)
            | smelt_hir::Pattern::Literal(_) => {
                return Err(self.error("only binding for patterns can lower to MIR yet", None));
            }
        };
        let item_local = self
            .locals
            .get(&hir_local)
            .copied()
            .ok_or_else(|| self.error("for pattern references an unknown local", None))?;
        let iter_operand = self.lower_expr(iter)?;
        let iter_span = self.hir_expr(iter)?.span;
        let iter_local = self.local_operand(iter_operand, iter_span)?;
        let float_ty = self.loop_index_ty;
        let bool_ty = self.loop_bool_ty;
        let idx = self.push_temp(float_ty, iter_span);
        self.block_mut()?.statements.push(Statement::Assign {
            dest: idx,
            value: Rvalue::Use(Operand::Const(Constant::Float(0.0))),
        });

        let header = self.function.push_block(self.block()?.span);
        self.set_terminator(Terminator::Goto(header))?;
        self.current_block = header;
        let len = self.push_temp(float_ty, self.block()?.span);
        self.block_mut()?.statements.push(Statement::Assign {
            dest: len,
            value: Rvalue::Len(Operand::Copy(Place::Local(iter_local))),
        });
        let cond = self.push_temp(bool_ty, self.block()?.span);
        self.block_mut()?.statements.push(Statement::Assign {
            dest: cond,
            value: Rvalue::Binary {
                op: smelt_hir::BinOp::Lt,
                lhs: Operand::Copy(Place::Local(idx)),
                rhs: Operand::Copy(Place::Local(len)),
            },
        });
        let body_mir = self.function.push_block(self.hir_block(body_hir)?.span);
        let update_block = self.function.push_block(self.block()?.span);
        let after = self.function.push_block(self.block()?.span);
        self.set_terminator(Terminator::Switch {
            cond: Operand::Copy(Place::Local(cond)),
            then_block: body_mir,
            else_block: after,
        })?;

        self.loops.push(LoopTargets {
            break_target: after,
            continue_target: update_block,
        });
        self.current_block = body_mir;
        self.block_mut()?.statements.push(Statement::Assign {
            dest: item_local,
            value: Rvalue::Use(Operand::Copy(Place::Index {
                base: iter_local,
                index: Box::new(Operand::Copy(Place::Local(idx))),
            })),
        });
        self.lower_block_stmts(body_hir)?;
        if self.block()?.terminator.is_none() {
            self.set_terminator(Terminator::Goto(update_block))?;
        }
        self.loops.pop();
        self.current_block = update_block;
        let one = Operand::Const(Constant::Float(1.0));
        self.block_mut()?.statements.push(Statement::AssignPlace {
            place: Place::Local(idx),
            value: Rvalue::Binary {
                op: smelt_hir::BinOp::Add,
                lhs: Operand::Copy(Place::Local(idx)),
                rhs: one,
            },
        });
        self.set_terminator(Terminator::Goto(header))?;
        self.current_block = after;
        Ok(())
    }

    /// Lowers a match expression to MIR with match terminator.
    fn lower_match(
        &mut self,
        scrutinee: ExprId,
        arms: &[smelt_hir::MatchArm],
        default: Option<smelt_hir::BlockId>,
    ) -> Result<(), LowerError> {
        let lowered_scrutinee = self.lower_expr(scrutinee)?;
        let span = self.block()?.span;
        let join = self.function.push_block(span);
        let mut mir_arms = Vec::new();
        let mut arm_blocks = Vec::new();
        let mut targets_by_hir_block = HashMap::new();

        for arm in arms {
            let target = *targets_by_hir_block
                .entry(arm.body)
                .or_insert_with(|| self.function.push_block(span));
            mir_arms.push(crate::MatchArm {
                label: lower_literal(&arm.label),
                target,
            });
            if !arm_blocks
                .iter()
                .any(|(_, hir_block): &(BlockId, smelt_hir::BlockId)| *hir_block == arm.body)
            {
                arm_blocks.push((target, arm.body));
            }
        }

        let default_target = default
            .map(|default_body| {
                *targets_by_hir_block
                    .entry(default_body)
                    .or_insert_with(|| self.function.push_block(span))
            })
            .or_else(|| {
                let target = self.function.push_block(span);
                let target_index = usize::try_from(target.0).unwrap_or(usize::MAX);
                if let Some(block) = self.function.blocks.get_mut(target_index) {
                    block.terminator = Some(Terminator::Goto(join));
                }
                Some(target)
            });
        self.set_terminator(Terminator::Match {
            scrutinee: lowered_scrutinee,
            arms: mir_arms,
            default: default_target,
        })?;

        let default_already_lowered = default.is_some_and(|default_block| {
            arm_blocks
                .iter()
                .any(|(_, hir_block): &(BlockId, smelt_hir::BlockId)| *hir_block == default_block)
        });

        for (target, hir_block) in arm_blocks {
            self.current_block = target;
            self.lower_block_stmts(hir_block)?;
            if self.block()?.terminator.is_none() {
                self.set_terminator(Terminator::Goto(join))?;
            }
        }

        if let (Some(target), Some(default_block)) = (default_target, default)
            && !default_already_lowered
        {
            self.current_block = target;
            self.lower_block_stmts(default_block)?;
            if self.block()?.terminator.is_none() {
                self.set_terminator(Terminator::Goto(join))?;
            }
        }

        self.current_block = join;
        Ok(())
    }

    /// Lowers a HIR expression to an operand, allocating temporaries as needed.
    fn lower_expr(&mut self, expr_id: ExprId) -> Result<Operand, LowerError> {
        if let Some(operand) = self.exprs.get(&expr_id) {
            return Ok(operand.clone());
        }

        let expr = self.hir_expr(expr_id)?.clone();
        let operand = match &expr.kind {
            ExprKind::Literal(literal) => Operand::Const(lower_literal(literal)),
            ExprKind::Local(local) => {
                let local_id = self.locals.get(local).copied().ok_or_else(|| {
                    self.error(
                        "local expression references an unknown local",
                        Some(expr.span),
                    )
                })?;
                let operand = Operand::Copy(Place::Local(local_id));
                let local_ty = self.mir_local(local_id)?.ty;
                if matches!(self.krate.types.get(local_ty), Some(Type::Optional(inner)) if *inner == expr.ty)
                {
                    let dest = self.push_temp(expr.ty, expr.span);
                    self.block_mut()?.statements.push(Statement::Assign {
                        dest,
                        value: Rvalue::Use(operand),
                    });
                    Operand::Copy(Place::Local(dest))
                } else {
                    operand
                }
            }
            ExprKind::Call { callee, args } => {
                let callee_id = self.lower_callee(*callee)?;
                let lowered_args = args
                    .iter()
                    .map(|arg| self.lower_expr(*arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                let target = self.function.push_block(expr.span);
                self.set_terminator(Terminator::Call {
                    callee: callee_id,
                    args: lowered_args,
                    dest,
                    target,
                    unwind: self.current_exception_handler(),
                })?;
                self.current_block = target;
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::Item(_) => {
                return Err(self.error(
                    "item expressions can only be used as callees",
                    Some(expr.span),
                ));
            }
            ExprKind::BinOp { op, lhs, rhs } => {
                let lhs_operand = self.lower_expr(*lhs)?;
                let rhs_operand = self.lower_expr(*rhs)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::Binary {
                        op: *op,
                        lhs: lhs_operand,
                        rhs: rhs_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::UnaryOp { op, operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::Unary {
                        op: *op,
                        operand: lowered_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            } => {
                let cond_operand = self.lower_expr(*cond)?;
                let dest = self.push_temp(expr.ty, expr.span);
                let then_block = self.function.push_block(expr.span);
                let else_block = self.function.push_block(expr.span);
                let join_block = self.function.push_block(expr.span);
                self.set_terminator(Terminator::Switch {
                    cond: cond_operand,
                    then_block,
                    else_block,
                })?;

                self.current_block = then_block;
                let then_operand = self.lower_expr(*then_expr)?;
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::Use(then_operand),
                });
                if self.block()?.terminator.is_none() {
                    self.set_terminator(Terminator::Goto(join_block))?;
                }

                self.current_block = else_block;
                let else_operand = self.lower_expr(*else_expr)?;
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::Use(else_operand),
                });
                if self.block()?.terminator.is_none() {
                    self.set_terminator(Terminator::Goto(join_block))?;
                }

                self.current_block = join_block;
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::FunctionTableLookup { key, cases } => {
                let lowered_key = self.lower_expr(*key)?;
                let lowered_cases = cases
                    .iter()
                    .map(|(case_key, case_expr)| {
                        self.lower_expr(*case_expr)
                            .map(|operand| (case_key.clone(), operand))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::FunctionTableLookup {
                        key: lowered_key,
                        cases: lowered_cases,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::InstanceOf { value, class } => {
                let lowered_value = self.lower_expr(*value)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::InstanceOf {
                        value: lowered_value,
                        class: *class,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::UnknownIs { value, kind } => {
                let lowered_value = self.lower_expr(*value)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::UnknownIs {
                        value: lowered_value,
                        kind: *kind,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::TypeofValue { value } => {
                let lowered_value = self.lower_expr(*value)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::TypeofValue {
                        value: lowered_value,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::UnknownCast { value, target } => {
                let lowered_value = self.lower_expr(*value)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::UnknownCast {
                        value: lowered_value,
                        target: *target,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListLit(items) => {
                let lowered_items = items
                    .iter()
                    .map(|item| self.lower_expr(*item))
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::List(lowered_items),
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::SetLit(items) => {
                let lowered_items = items
                    .iter()
                    .map(|item| self.lower_expr(*item))
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::Set(lowered_items),
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DictLit(entries) => {
                let lowered_entries = entries
                    .iter()
                    .map(|(key, value)| Ok((self.lower_expr(*key)?, self.lower_expr(*value)?)))
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::Dict(lowered_entries),
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::TupleLit(items) => {
                let lowered_items = items
                    .iter()
                    .map(|item| self.lower_expr(*item))
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::Tuple(lowered_items),
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::Field { receiver, field } => {
                let receiver_operand = self.lower_expr(*receiver)?;
                let receiver_ty = self.hir_expr(*receiver)?.ty;
                let base =
                    self.materialize_operand_local(receiver_operand, receiver_ty, expr.span)?;
                Operand::Copy(Place::Field {
                    base,
                    field: *field,
                })
            }
            ExprKind::OptionalField { receiver, field } => {
                let receiver_operand = self.lower_expr(*receiver)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::OptionalField {
                        receiver: receiver_operand,
                        field: *field,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::Index { receiver, index } => {
                let receiver_operand = self.lower_expr(*receiver)?;
                let index_operand = self.lower_expr(*index)?;
                if matches!(self.krate.types.get(expr.ty), Some(Type::Optional(_))) {
                    let dest = self.push_temp(expr.ty, expr.span);
                    self.block_mut()?.statements.push(Statement::Assign {
                        dest,
                        value: Rvalue::OptionalIndex {
                            receiver: receiver_operand,
                            index: index_operand,
                        },
                    });
                    return Ok(Operand::Copy(Place::Local(dest)));
                }
                let receiver_ty = self.hir_expr(*receiver)?.ty;
                let base =
                    self.materialize_operand_local(receiver_operand, receiver_ty, expr.span)?;
                Operand::Copy(Place::Index {
                    base,
                    index: Box::new(index_operand),
                })
            }
            ExprKind::OptionalIndex { receiver, index } => {
                let receiver_operand = self.lower_expr(*receiver)?;
                let index_operand = self.lower_expr(*index)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::OptionalIndex {
                        receiver: receiver_operand,
                        index: index_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::OptionalMethod {
                receiver,
                method,
                args,
            } => {
                let receiver_operand = self.lower_expr(*receiver)?;
                let lowered_args = args
                    .iter()
                    .map(|arg| self.lower_expr(*arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::OptionalMethod {
                        receiver: receiver_operand,
                        method: *method,
                        args: lowered_args,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::OptionalCoalesce { optional, fallback } => {
                let lowered_optional = self.lower_expr(*optional)?;
                let lowered_fallback = self.lower_expr(*fallback)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::OptionalCoalesce {
                        optional: lowered_optional,
                        fallback: lowered_fallback,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::TypeAssert { value } => {
                let lowered_value = self.lower_expr(*value)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::Use(lowered_value),
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::Len { operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::Len(lowered_operand),
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::NumericAbs { operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::NumericAbs(lowered_operand),
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::NumericRound { op, operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::NumericRound {
                        op: *op,
                        operand: lowered_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::NumericExtrema { op, args } => {
                let lowered_args = args
                    .iter()
                    .map(|arg| self.lower_expr(*arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::NumericExtrema {
                        op: *op,
                        args: lowered_args,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::NumericHypot { args } => {
                let lowered_args = args
                    .iter()
                    .map(|arg| self.lower_expr(*arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::NumericHypot { args: lowered_args },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::NumericPredicate { op, operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::NumericPredicate {
                        op: *op,
                        operand: lowered_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::NumericUnaryFunc { op, operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::NumericUnaryFunc {
                        op: *op,
                        operand: lowered_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::NumericPow { base, exponent } => {
                let base_operand = self.lower_expr(*base)?;
                let exponent_operand = self.lower_expr(*exponent)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::NumericPow {
                        base: base_operand,
                        exponent: exponent_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::NumericAtan2 { y, x } => {
                let y_operand = self.lower_expr(*y)?;
                let x_operand = self.lower_expr(*x)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::NumericAtan2 {
                        y: y_operand,
                        x: x_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::NumericRandom => {
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::NumericRandom,
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::NumericRandomInt { start, end } => {
                let start_operand = self.lower_expr(*start)?;
                let end_operand = self.lower_expr(*end)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::NumericRandomInt {
                        start: start_operand,
                        end: end_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::NumericToStringRadix { operand, radix } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let lowered_radix = self.lower_expr(*radix)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::NumericToStringRadix {
                        operand: lowered_operand,
                        radix: lowered_radix,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::PrimitiveCast { op, operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::PrimitiveCast {
                        op: *op,
                        operand: lowered_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::StringCase { op, operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::StringCase {
                        op: *op,
                        operand: lowered_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::StringNormalize { form, operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::StringNormalize {
                        form: *form,
                        operand: lowered_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::StringTrim { side, operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::StringTrim {
                        side: *side,
                        operand: lowered_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::StringAffix {
                op,
                haystack,
                needle,
            } => {
                let haystack_operand = self.lower_expr(*haystack)?;
                let needle_operand = self.lower_expr(*needle)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::StringAffix {
                        op: *op,
                        haystack: haystack_operand,
                        needle: needle_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::StringSearch {
                op,
                haystack,
                needle,
                from_index,
            } => {
                let haystack_operand = self.lower_expr(*haystack)?;
                let needle_operand = self.lower_expr(*needle)?;
                let from_index_operand =
                    from_index.map(|value| self.lower_expr(value)).transpose()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::StringSearch {
                        op: *op,
                        haystack: haystack_operand,
                        needle: needle_operand,
                        from_index: from_index_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::StringReplace {
                op,
                haystack,
                pattern,
                replacement,
            } => {
                let haystack_operand = self.lower_expr(*haystack)?;
                let pattern_operand = self.lower_expr(*pattern)?;
                let replacement_operand = self.lower_expr(*replacement)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::StringReplace {
                        op: *op,
                        haystack: haystack_operand,
                        pattern: pattern_operand,
                        replacement: replacement_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::StringRemoveAffix {
                op,
                haystack,
                affix,
            } => {
                let haystack_operand = self.lower_expr(*haystack)?;
                let affix_operand = self.lower_expr(*affix)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::StringRemoveAffix {
                        op: *op,
                        haystack: haystack_operand,
                        affix: affix_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::StringRepeat { operand, count } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let count_operand = self.lower_expr(*count)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::StringRepeat {
                        operand: lowered_operand,
                        count: count_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::StringPad {
                op,
                operand,
                target_len,
                pad,
            } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let target_len_operand = self.lower_expr(*target_len)?;
                let pad_operand = self.lower_expr(*pad)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::StringPad {
                        op: *op,
                        operand: lowered_operand,
                        target_len: target_len_operand,
                        pad: pad_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::StringPredicate { op, operand } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::StringPredicate {
                        op: *op,
                        operand: lowered_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::RegexIsMatch {
                op,
                pattern,
                haystack,
            } => {
                let pattern_operand = self.lower_expr(*pattern)?;
                let haystack_operand = self.lower_expr(*haystack)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::RegexIsMatch {
                        op: *op,
                        pattern: pattern_operand,
                        haystack: haystack_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::RegexReplace {
                op,
                pattern,
                haystack,
                replacement,
            } => {
                let pattern_operand = self.lower_expr(*pattern)?;
                let haystack_operand = self.lower_expr(*haystack)?;
                let replacement_operand = self.lower_expr(*replacement)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::RegexReplace {
                        op: *op,
                        pattern: pattern_operand,
                        haystack: haystack_operand,
                        replacement: replacement_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::RegexReplaceCallback {
                op,
                pattern,
                haystack,
                callback,
            } => {
                let pattern_operand = self.lower_expr(*pattern)?;
                let haystack_operand = self.lower_expr(*haystack)?;
                let callback_operand = self.lower_expr(*callback)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::RegexReplaceCallback {
                        op: *op,
                        pattern: pattern_operand,
                        haystack: haystack_operand,
                        callback: callback_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::RegexReplaceFirstMatchUppercase { pattern, haystack } => {
                let pattern_operand = self.lower_expr(*pattern)?;
                let haystack_operand = self.lower_expr(*haystack)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::RegexReplaceFirstMatchUppercase {
                        pattern: pattern_operand,
                        haystack: haystack_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::RegexSplit { pattern, haystack } => {
                let pattern_operand = self.lower_expr(*pattern)?;
                let haystack_operand = self.lower_expr(*haystack)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::RegexSplit {
                        pattern: pattern_operand,
                        haystack: haystack_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::RegexFind { pattern, haystack } => {
                let pattern_operand = self.lower_expr(*pattern)?;
                let haystack_operand = self.lower_expr(*haystack)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::RegexFind {
                        pattern: pattern_operand,
                        haystack: haystack_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::RegexExec { regex, haystack } => {
                let regex_operand = self.lower_expr(*regex)?;
                let haystack_operand = self.lower_expr(*haystack)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::RegexExec {
                        regex: regex_operand,
                        haystack: haystack_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::RegexMatchAll { regex, haystack } => {
                let regex_operand = self.lower_expr(*regex)?;
                let haystack_operand = self.lower_expr(*haystack)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::RegexMatchAll {
                        regex: regex_operand,
                        haystack: haystack_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::StringCharAt { operand, index } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let index_operand = self.lower_expr(*index)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::StringCharAt {
                        operand: lowered_operand,
                        index: index_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::StringCharCodeAt { operand, index } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let index_operand = self.lower_expr(*index)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::StringCharCodeAt {
                        operand: lowered_operand,
                        index: index_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::StringContains { haystack, needle } => {
                let haystack_operand = self.lower_expr(*haystack)?;
                let needle_operand = self.lower_expr(*needle)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::StringContains {
                        haystack: haystack_operand,
                        needle: needle_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::StringSlice {
                operand,
                start,
                end,
            } => {
                let lowered_operand = self.lower_expr(*operand)?;
                let start_operand = start.map(|bound| self.lower_expr(bound)).transpose()?;
                let end_operand = end.map(|bound| self.lower_expr(bound)).transpose()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::StringSlice {
                        operand: lowered_operand,
                        start: start_operand,
                        end: end_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListContains { list, item } => {
                let list_operand = self.lower_expr(*list)?;
                let item_operand = self.lower_expr(*item)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListContains {
                        list: list_operand,
                        item: item_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::SetContains { set, item } => {
                let set_operand = self.lower_expr(*set)?;
                let item_operand = self.lower_expr(*item)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::SetContains {
                        set: set_operand,
                        item: item_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::SetDisjoint { left, right } => {
                let left_operand = self.lower_expr(*left)?;
                let right_operand = self.lower_expr(*right)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::SetDisjoint {
                        left: left_operand,
                        right: right_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::SetRelation { op, left, right } => {
                let left_operand = self.lower_expr(*left)?;
                let right_operand = self.lower_expr(*right)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::SetRelation {
                        op: *op,
                        left: left_operand,
                        right: right_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::SetAdd { set, item } => {
                let set_operand = self.lower_expr(*set)?;
                let item_operand = self.lower_expr(*item)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::SetAdd {
                        set: set_operand,
                        item: item_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::SetRemove { op, set, item } => {
                let set_operand = self.lower_expr(*set)?;
                let item_operand = self.lower_expr(*item)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::SetRemove {
                        op: *op,
                        set: set_operand,
                        item: item_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::SetClear { set } => {
                let set_operand = self.lower_expr(*set)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::SetClear { set: set_operand },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::SetCopy { set } => {
                let set_operand = self.lower_expr(*set)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::SetCopy { set: set_operand },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListToSet { list } => {
                let list_operand = self.lower_expr(*list)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListToSet { list: list_operand },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListPairsToDict { list } => {
                let list_operand = self.lower_expr(*list)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListPairsToDict { list: list_operand },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::SetBinary { op, left, right } => {
                let left_operand = self.lower_expr(*left)?;
                let right_operand = self.lower_expr(*right)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::SetBinary {
                        op: *op,
                        left: left_operand,
                        right: right_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::SetProjection { op, set } => {
                let set_operand = self.lower_expr(*set)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::SetProjection {
                        op: *op,
                        set: set_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListConcat { left, right } => {
                let left_operand = self.lower_expr(*left)?;
                let right_operand = self.lower_expr(*right)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListConcat {
                        left: left_operand,
                        right: right_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListSearch { op, list, item } => {
                let list_operand = self.lower_expr(*list)?;
                let item_operand = self.lower_expr(*item)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListSearch {
                        op: *op,
                        list: list_operand,
                        item: item_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListCallback { op, list, callback } => {
                let list_operand = self.lower_expr(*list)?;
                let callback_operand = self.lower_expr(*callback)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListCallback {
                        op: *op,
                        list: list_operand,
                        callback: callback_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListFromLength { length } => {
                let length_operand = self.lower_expr(*length)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListFromLength {
                        length: length_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListRepeat { value, count } => {
                let value_operand = self.lower_expr(*value)?;
                let count_operand = self.lower_expr(*count)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListRepeat {
                        value: value_operand,
                        count: count_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListFromLengthMap { length, callback } => {
                let length_operand = self.lower_expr(*length)?;
                let callback_operand = self.lower_expr(*callback)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListFromLengthMap {
                        length: length_operand,
                        callback: callback_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListReduce {
                list,
                initial,
                callback,
            } => {
                let list_operand = self.lower_expr(*list)?;
                let initial_operand = initial.map(|expr| self.lower_expr(expr)).transpose()?;
                let callback_operand = self.lower_expr(*callback)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListReduce {
                        list: list_operand,
                        initial: initial_operand,
                        callback: callback_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListSlice { list, start, end } => {
                let list_operand = self.lower_expr(*list)?;
                let start_operand = start.map(|bound| self.lower_expr(bound)).transpose()?;
                let end_operand = end.map(|bound| self.lower_expr(bound)).transpose()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListSlice {
                        list: list_operand,
                        start: start_operand,
                        end: end_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListSplice {
                list,
                start,
                delete_count,
                items,
                mutate,
            } => {
                let list_operand = self.lower_expr(*list)?;
                let start_operand = self.lower_expr(*start)?;
                let delete_count_operand = delete_count
                    .map(|count| self.lower_expr(count))
                    .transpose()?;
                let item_operands = items
                    .iter()
                    .map(|item| {
                        Ok(MirListSpliceItem {
                            value: self.lower_expr(item.value)?,
                            spread: item.spread,
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListSplice {
                        list: list_operand,
                        start: start_operand,
                        delete_count: delete_count_operand,
                        items: item_operands,
                        mutate: *mutate,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListFill {
                list,
                value,
                start,
                end,
            } => {
                let list_operand = self.lower_expr(*list)?;
                let value_operand = self.lower_expr(*value)?;
                let start_operand = start.map(|bound| self.lower_expr(bound)).transpose()?;
                let end_operand = end.map(|bound| self.lower_expr(bound)).transpose()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListFill {
                        list: list_operand,
                        value: value_operand,
                        start: start_operand,
                        end: end_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListCopyWithin {
                list,
                target,
                start,
                end,
            } => {
                let list_operand = self.lower_expr(*list)?;
                let target_operand = self.lower_expr(*target)?;
                let start_operand = self.lower_expr(*start)?;
                let end_operand = end.map(|bound| self.lower_expr(bound)).transpose()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListCopyWithin {
                        list: list_operand,
                        target: target_operand,
                        start: start_operand,
                        end: end_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListWith { list, index, value } => {
                let list_operand = self.lower_expr(*list)?;
                let index_operand = self.lower_expr(*index)?;
                let value_operand = self.lower_expr(*value)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListWith {
                        list: list_operand,
                        index: index_operand,
                        value: value_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListFlat { list, depth } => {
                let list_operand = self.lower_expr(*list)?;
                let depth_operand = depth
                    .map(|depth_expr| self.lower_expr(depth_expr))
                    .transpose()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListFlat {
                        list: list_operand,
                        depth: depth_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListProjection { op, list } => {
                let list_operand = self.lower_expr(*list)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListProjection {
                        op: *op,
                        list: list_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListPush { list, item } => {
                let (list_operand, writeback) = self.lower_mutation_receiver(*list)?;
                let item_operand = self.lower_expr(*item)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListPush {
                        list: list_operand,
                        item: item_operand,
                    },
                });
                self.write_back_mutation_receiver(writeback)?;
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListExtend { list, other } => {
                let (list_operand, writeback) = self.lower_mutation_receiver(*list)?;
                let other_operand = self.lower_expr(*other)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListExtend {
                        list: list_operand,
                        other: other_operand,
                    },
                });
                self.write_back_mutation_receiver(writeback)?;
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListInsert { list, index, item } => {
                let (list_operand, writeback) = self.lower_mutation_receiver(*list)?;
                let index_operand = self.lower_expr(*index)?;
                let item_operand = self.lower_expr(*item)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListInsert {
                        list: list_operand,
                        index: index_operand,
                        item: item_operand,
                    },
                });
                self.write_back_mutation_receiver(writeback)?;
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListUnshift { list, items } => {
                let (list_operand, writeback) = self.lower_mutation_receiver(*list)?;
                let item_operands = items
                    .iter()
                    .map(|item| self.lower_expr(*item))
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListUnshift {
                        list: list_operand,
                        items: item_operands,
                    },
                });
                self.write_back_mutation_receiver(writeback)?;
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListReverse { list } => {
                let (list_operand, writeback) = self.lower_mutation_receiver(*list)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListReverse { list: list_operand },
                });
                self.write_back_mutation_receiver(writeback)?;
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListClear { list } => {
                let (list_operand, writeback) = self.lower_mutation_receiver(*list)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListClear { list: list_operand },
                });
                self.write_back_mutation_receiver(writeback)?;
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListCopy { list } => {
                let list_operand = self.lower_expr(*list)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListCopy { list: list_operand },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::TupleToList { tuple } => {
                let tuple_operand = self.lower_expr(*tuple)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::TupleToList {
                        tuple: tuple_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListToTuple { list } => {
                let list_operand = self.lower_expr(*list)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListToTuple { list: list_operand },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::TupleToSet { tuple } => {
                let tuple_operand = self.lower_expr(*tuple)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::TupleToSet {
                        tuple: tuple_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListCount { list, item } => {
                let list_operand = self.lower_expr(*list)?;
                let item_operand = self.lower_expr(*item)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListCount {
                        list: list_operand,
                        item: item_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListSum { list } => {
                let list_operand = self.lower_expr(*list)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListSum { list: list_operand },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListBoolFold { op, list } => {
                let list_operand = self.lower_expr(*list)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListBoolFold {
                        op: *op,
                        list: list_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListSorted { list, key, reverse } => {
                let list_operand = self.lower_expr(*list)?;
                let key_operand = match key {
                    Some(key) => Some(self.lower_expr(*key)?),
                    None => None,
                };
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListSorted {
                        list: list_operand,
                        key: key_operand,
                        reverse: *reverse,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListReversed { list } => {
                let list_operand = self.lower_expr(*list)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListReversed { list: list_operand },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListEnumerate { list } => {
                let list_operand = self.lower_expr(*list)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListEnumerate { list: list_operand },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListZip { left, right } => {
                let left_operand = self.lower_expr(*left)?;
                let right_operand = self.lower_expr(*right)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListZip {
                        left: left_operand,
                        right: right_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListRange { start, end, step } => {
                let start_operand = self.lower_expr(*start)?;
                let end_operand = self.lower_expr(*end)?;
                let step_operand = self.lower_expr(*step)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListRange {
                        start: start_operand,
                        end: end_operand,
                        step: step_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListRandomChoice { list } => {
                let list_operand = self.lower_expr(*list)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListRandomChoice { list: list_operand },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListIndex { list, item } => {
                let list_operand = self.lower_expr(*list)?;
                let item_operand = self.lower_expr(*item)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListIndex {
                        list: list_operand,
                        item: item_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListRemove { list, item } => {
                let (list_operand, writeback) = self.lower_mutation_receiver(*list)?;
                let item_operand = self.lower_expr(*item)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListRemove {
                        list: list_operand,
                        item: item_operand,
                    },
                });
                self.write_back_mutation_receiver(writeback)?;
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListSort {
                list,
                comparator,
                key,
                reverse,
            } => {
                let (list_operand, writeback) = self.lower_mutation_receiver(*list)?;
                let comparator_operand = match comparator {
                    Some(comparator) => Some(self.lower_expr(*comparator)?),
                    None => None,
                };
                let key_operand = match key {
                    Some(key) => Some(self.lower_expr(*key)?),
                    None => None,
                };
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListSort {
                        list: list_operand,
                        comparator: comparator_operand,
                        key: key_operand,
                        reverse: *reverse,
                    },
                });
                self.write_back_mutation_receiver(writeback)?;
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListPop { list } => {
                let (list_operand, writeback) = self.lower_mutation_receiver(*list)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListPop { list: list_operand },
                });
                self.write_back_mutation_receiver(writeback)?;
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListShift { list } => {
                let (list_operand, writeback) = self.lower_mutation_receiver(*list)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListShift { list: list_operand },
                });
                self.write_back_mutation_receiver(writeback)?;
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListNext { list } => {
                let list_operand = self.lower_expr(*list)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ListNext { list: list_operand },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::IteratorDone { result } => {
                let result_operand = self.lower_expr(*result)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::IteratorDone {
                        result: result_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::IteratorValue { result } => {
                let result_operand = self.lower_expr(*result)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::IteratorValue {
                        result: result_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::TupleContains { tuple, item } => {
                let tuple_operand = self.lower_expr(*tuple)?;
                let item_operand = self.lower_expr(*item)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::TupleContains {
                        tuple: tuple_operand,
                        item: item_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::TupleIndex { tuple, index } => {
                let tuple_operand = self.lower_expr(*tuple)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::TupleIndex {
                        tuple: tuple_operand,
                        index: *index,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::TupleSlice { tuple, start, end } => {
                let tuple_operand = self.lower_expr(*tuple)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::TupleSlice {
                        tuple: tuple_operand,
                        start: *start,
                        end: *end,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DictContainsKey { dict, key } => {
                let dict_operand = self.lower_expr(*dict)?;
                let key_operand = self.lower_expr(*key)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::DictContainsKey {
                        dict: dict_operand,
                        key: key_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DictSet { dict, key, value } => {
                let dict_operand = self.lower_expr(*dict)?;
                let key_operand = self.lower_expr(*key)?;
                let value_operand = self.lower_expr(*value)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::DictSet {
                        dict: dict_operand,
                        key: key_operand,
                        value: value_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DictRemoveKey { dict, key } => {
                let dict_operand = self.lower_expr(*dict)?;
                let key_operand = self.lower_expr(*key)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::DictRemoveKey {
                        dict: dict_operand,
                        key: key_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DictGet { dict, key, default } => {
                let dict_operand = self.lower_expr(*dict)?;
                let key_operand = self.lower_expr(*key)?;
                let default_operand = default
                    .map(|default_expr| self.lower_expr(default_expr))
                    .transpose()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::DictGet {
                        dict: dict_operand,
                        key: key_operand,
                        default: default_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DictSetDefault { dict, key, default } => {
                let dict_operand = self.lower_expr(*dict)?;
                let key_operand = self.lower_expr(*key)?;
                let default_operand = self.lower_expr(*default)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::DictSetDefault {
                        dict: dict_operand,
                        key: key_operand,
                        default: default_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DictClear { dict } => {
                let dict_operand = self.lower_expr(*dict)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::DictClear { dict: dict_operand },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DictPop { dict, key, default } => {
                let dict_operand = self.lower_expr(*dict)?;
                let key_operand = self.lower_expr(*key)?;
                let default_operand = default
                    .as_ref()
                    .map(|default_expr| self.lower_expr(*default_expr))
                    .transpose()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::DictPop {
                        dict: dict_operand,
                        key: key_operand,
                        default: default_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DictUpdate { dict, other } => {
                let dict_operand = self.lower_expr(*dict)?;
                let other_operand = self.lower_expr(*other)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::DictUpdate {
                        dict: dict_operand,
                        other: other_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DictAssign { target, sources } => {
                let target_operand = self.lower_expr(*target)?;
                let source_operands = sources
                    .iter()
                    .map(|source| self.lower_expr(*source))
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::DictAssign {
                        target: target_operand,
                        sources: source_operands,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::CallableObjectAssign { callable, props } => {
                let callable_operand = self.lower_expr(*callable)?;
                let prop_operands = props
                    .iter()
                    .map(|(name, value)| Ok((*name, self.lower_expr(*value)?)))
                    .collect::<Result<Vec<_>, LowerError>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::CallableObjectAssign {
                        callable: callable_operand,
                        props: prop_operands,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DictCopy { dict } => {
                let dict_operand = self.lower_expr(*dict)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::DictCopy { dict: dict_operand },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DictProjection { op, dict } => {
                let dict_operand = self.lower_expr(*dict)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::DictProjection {
                        op: *op,
                        dict: dict_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::StringSplit {
                haystack,
                separator,
                limit,
            } => {
                let haystack_operand = self.lower_expr(*haystack)?;
                let separator_operand = self.lower_expr(*separator)?;
                let limit_operand = limit
                    .map(|limit_expr| self.lower_expr(limit_expr))
                    .transpose()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::StringSplit {
                        haystack: haystack_operand,
                        separator: separator_operand,
                        limit: limit_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::StringChars { haystack } => {
                let haystack_operand = self.lower_expr(*haystack)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::StringChars {
                        haystack: haystack_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::StringJoin { items, separator } => {
                let items_operand = self.lower_expr(*items)?;
                let separator_operand = self.lower_expr(*separator)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::StringJoin {
                        items: items_operand,
                        separator: separator_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::JsonStringify { value } => {
                let value_operand = self.lower_expr(*value)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::JsonStringify {
                        value: value_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::JsonParse { text } => {
                let text_operand = self.lower_expr(*text)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::JsonParse { text: text_operand },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::HttpGetText { url } => {
                let url_operand = self.lower_expr(*url)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::HttpGetText { url: url_operand },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DateNow => {
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::DateNow,
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DateSetNow { timestamp } => {
                let timestamp_operand = self.lower_expr(*timestamp)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::DateSetNow {
                        timestamp: timestamp_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DateResetNow => {
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::DateResetNow,
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DateTimezoneOffset => {
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::DateTimezoneOffset,
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DateSetTimezoneOffset { offset } => {
                let offset_operand = self.lower_expr(*offset)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::DateSetTimezoneOffset {
                        offset: offset_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DateResetTimezoneOffset => {
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::DateResetTimezoneOffset,
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DateTimezoneContext { timezone } => {
                let timezone_operand = self.lower_expr(*timezone)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::DateTimezoneContext {
                        timezone: timezone_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DateToIsoString { timestamp_ms } => {
                let timestamp_operand = self.lower_expr(*timestamp_ms)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::DateToIsoString {
                        timestamp_ms: timestamp_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DateToString { timestamp_ms } => {
                let timestamp_operand = self.lower_expr(*timestamp_ms)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::DateToString {
                        timestamp_ms: timestamp_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DateFromParts { parts } => {
                let part_operands = parts
                    .iter()
                    .map(|part| self.lower_expr(*part))
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::DateFromParts {
                        parts: part_operands,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DateFromValue { value } => {
                let value_operand = self.lower_expr(*value)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::DateFromValue {
                        value: value_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DateGetPart { part, timestamp_ms } => {
                let timestamp_operand = self.lower_expr(*timestamp_ms)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::DateGetPart {
                        part: *part,
                        timestamp_ms: timestamp_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DateSetPart {
                part,
                timestamp_ms,
                values,
            } => {
                let timestamp_operand = self.lower_expr(*timestamp_ms)?;
                let value_operands = values
                    .iter()
                    .map(|value| self.lower_expr(*value))
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::DateSetPart {
                        part: *part,
                        timestamp_ms: timestamp_operand,
                        values: value_operands,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::UrlField { field, url } => {
                let url_operand = self.lower_expr(*url)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::UrlField {
                        field: *field,
                        url: url_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::FileReadText { path } => {
                let path_operand = self.lower_expr(*path)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::FileReadText { path: path_operand },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::FileWriteText { path, text } => {
                let path_operand = self.lower_expr(*path)?;
                let text_operand = self.lower_expr(*text)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::FileWriteText {
                        path: path_operand,
                        text: text_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::Method {
                receiver,
                method,
                args,
            } => {
                let receiver_operand = self.lower_expr(*receiver)?;
                let receiver_ty = self.hir_expr(*receiver)?.ty;
                let base = self.materialize_operand_local(
                    receiver_operand.clone(),
                    receiver_ty,
                    expr.span,
                )?;
                let receiver_ty = self.mir_local(base)?.ty;
                let callee_id = self.resolve_method(receiver_ty, *method, expr.span)?;
                let mut lowered_args = vec![receiver_operand];
                lowered_args.extend(
                    args.iter()
                        .map(|arg| self.lower_expr(*arg))
                        .collect::<Result<Vec<_>, _>>()?,
                );
                let dest = self.push_temp(expr.ty, expr.span);
                let target = self.function.push_block(expr.span);
                self.set_terminator(Terminator::Call {
                    callee: Callee::Static(callee_id),
                    args: lowered_args,
                    dest,
                    target,
                    unwind: self.current_exception_handler(),
                })?;
                self.current_block = target;
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::New { class, args } => {
                let lowered_args = args
                    .iter()
                    .map(|arg| self.lower_expr(*arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let Some(callee_id) = self.resolve_constructor(*class, expr.span)? else {
                    let dest = self.push_temp(expr.ty, expr.span);
                    self.block_mut()?.statements.push(Statement::Assign {
                        dest,
                        value: Rvalue::ExternalClassInstance {
                            class: *class,
                            args: lowered_args,
                        },
                    });
                    return Ok(Operand::Copy(Place::Local(dest)));
                };
                let dest = self.push_temp(expr.ty, expr.span);
                let target = self.function.push_block(expr.span);
                self.set_terminator(Terminator::Call {
                    callee: Callee::Static(callee_id),
                    args: lowered_args,
                    dest,
                    target,
                    unwind: self.current_exception_handler(),
                })?;
                self.current_block = target;
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::Await(future) => {
                let lowered_future = self.lower_expr(*future)?;
                let dest = self.push_temp(expr.ty, expr.span);
                if self.current_exception_handler().is_some() {
                    let target = self.function.push_block(expr.span);
                    self.set_terminator(Terminator::Await {
                        future: lowered_future,
                        dest,
                        target,
                        unwind: self.current_exception_handler(),
                    })?;
                    self.current_block = target;
                } else {
                    self.block_mut()?.statements.push(Statement::Assign {
                        dest,
                        value: Rvalue::Await(lowered_future),
                    });
                }
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::AsyncOp { op, args } => {
                let lowered_args = args
                    .iter()
                    .map(|arg| self.lower_expr(*arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::AsyncOp {
                        op: *op,
                        args: lowered_args,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::Closure(closure) => self.lower_closure_expr(closure, expr.ty, expr.span)?,
            ExprKind::ClosureCall { callee, args } => {
                let callee_ty = self.hir_expr(*callee)?.ty;
                let callee_operand = self.lower_expr(*callee)?;
                let lowered_args = args
                    .iter()
                    .map(|arg| self.lower_expr(*arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                if matches!(
                    self.krate.types.get(callee_ty),
                    Some(Type::Function(function)) if function.may_throw
                ) {
                    let target = self.function.push_block(expr.span);
                    self.set_terminator(Terminator::Call {
                        callee: Callee::Indirect(callee_operand),
                        args: lowered_args,
                        dest,
                        target,
                        unwind: self.current_exception_handler(),
                    })?;
                    self.current_block = target;
                    return Ok(Operand::Copy(Place::Local(dest)));
                }
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ClosureCall {
                        callee: callee_operand,
                        args: lowered_args,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ClosureCallSpread { callee, args } => {
                let callee_operand = self.lower_expr(*callee)?;
                let args_operand = self.lower_expr(*args)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::ClosureCallSpread {
                        callee: callee_operand,
                        args: args_operand,
                    },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::Block(block_id) => {
                // Execute the block's statements in the current control flow,
                // then yield the block's tail expression (or `None`) as the
                // block-expression value. Used by comprehension lowering.
                let tail = self.hir_block(*block_id)?.tail;
                self.lower_block_stmts(*block_id)?;
                match tail {
                    Some(tail_expr) => self.lower_expr(tail_expr)?,
                    None => Operand::Const(Constant::None),
                }
            }
            ExprKind::Lambda { .. } => {
                return Err(self.error(
                    "expression kind is not implemented in MIR yet",
                    Some(expr.span),
                ));
            }
        };

        self.exprs.insert(expr_id, operand.clone());
        Ok(operand)
    }

    /// Lowers a callee expression to a MIR callee.
    fn lower_callee(&self, expr_id: ExprId) -> Result<Callee, LowerError> {
        let expr = self.hir_expr(expr_id)?.clone();
        let ExprKind::Item(item_id) = expr.kind else {
            return Err(self.error(
                "only direct item calls can lower to MIR yet",
                Some(expr.span),
            ));
        };
        let item = self.hir_item(item_id)?;
        let smelt_hir::Item::Function(function) = item else {
            return Err(self.error(
                "only function item calls can lower to MIR yet",
                Some(expr.span),
            ));
        };
        let Some(name) = self.krate.symbols.get(function.name) else {
            return Err(self.error("callee has an unknown symbol", Some(expr.span)));
        };
        if name == smelt_hir::CONSOLE_LOG_SYMBOL {
            Ok(Callee::Builtin(BuiltinFn::ConsoleLog))
        } else if let Some(function_id) = self.item_functions.get(&item_id).copied() {
            Ok(Callee::Static(function_id))
        } else {
            Err(self.error(
                format!("function `{name}` is not resolvable to MIR yet"),
                Some(expr.span),
            ))
        }
    }

    /// Resolves a class constructor to its function ID.
    fn resolve_constructor(&self, class: Symbol, span: Span) -> Result<Option<FuncId>, LowerError> {
        for item in &self.krate.items {
            if let smelt_hir::Item::Class(class_item) = item
                && class_item.name == class
            {
                if matches!(class_item.kind, smelt_hir::ClassKind::Abstract) {
                    let name = self.krate.symbols.get(class).unwrap_or("<unknown>");
                    return Err(self.error(
                        format!("abstract class `{name}` cannot be constructed"),
                        Some(span),
                    ));
                }
                if let Some(constructor) = class_item.constructor
                    && let Some(func) = self.item_functions.get(&constructor)
                {
                    return Ok(Some(*func));
                }
            }
        }
        if self
            .krate
            .types
            .all()
            .iter()
            .any(|ty| matches!(ty, Type::Class { name, .. } if *name == class))
        {
            return Ok(None);
        }
        let name = self.krate.symbols.get(class).unwrap_or("<unknown>");
        Err(self.error(
            format!("class `{name}` has no resolvable constructor"),
            Some(span),
        ))
    }

    /// Resolves a method call to its function ID based on the receiver type.
    fn resolve_method(
        &self,
        receiver_ty: TypeId,
        method: Symbol,
        span: Span,
    ) -> Result<FuncId, LowerError> {
        let Some(Type::Class { name, .. }) = self.krate.types.get(receiver_ty) else {
            return Err(self.error("method receiver must be a class value", Some(span)));
        };
        self.resolve_method_on_class(*name, method, span)
    }

    /// Resolves a method call by walking the single-inheritance chain.
    fn resolve_method_on_class(
        &self,
        class: Symbol,
        method: Symbol,
        span: Span,
    ) -> Result<FuncId, LowerError> {
        for item in &self.krate.items {
            if let smelt_hir::Item::Class(class_item) = item
                && class_item.name == class
            {
                for method_item in &class_item.methods {
                    if let smelt_hir::Item::Function(function) = self.hir_item(*method_item)?
                        && function.name == method
                        && let Some(func) = self.item_functions.get(method_item)
                    {
                        return Ok(*func);
                    }
                }
                if let Some(base) = class_item.base {
                    return self.resolve_method_on_class(base, method, span);
                }
            }
        }
        let method_name = self.krate.symbols.get(method).unwrap_or("<unknown>");
        Err(self.error(
            format!("class method `{method_name}` is not resolvable"),
            Some(span),
        ))
    }

    /// Allocates a new temporary local variable.
    fn push_temp(&mut self, ty: TypeId, span: Span) -> LocalId {
        self.function.push_local(LocalDecl {
            ty,
            kind: LocalKind::Temp,
            span,
        })
    }

    /// Lowers a mutable collection receiver and records non-local place writeback.
    fn lower_mutation_receiver(
        &mut self,
        expr_id: ExprId,
    ) -> Result<LoweredMutationReceiver, LowerError> {
        let expr = self.hir_expr(expr_id)?.clone();
        if !matches!(
            expr.kind,
            ExprKind::Field { .. }
                | ExprKind::Index { .. }
                | ExprKind::TupleIndex { .. }
                | ExprKind::TypeAssert { .. }
                | ExprKind::UnknownCast { .. }
        ) {
            return Ok((self.lower_expr(expr_id)?, None));
        }

        let place = self.lower_place(expr_id)?;
        if matches!(place, Place::Local(_)) {
            return Ok((Operand::Copy(place), None));
        }

        let local = self.push_temp(expr.ty, expr.span);
        self.block_mut()?.statements.push(Statement::Assign {
            dest: local,
            value: Rvalue::Use(Operand::Copy(place.clone())),
        });
        Ok((Operand::Copy(Place::Local(local)), Some((place, local))))
    }

    /// Writes a mutated temporary collection back through its original place.
    fn write_back_mutation_receiver(
        &mut self,
        writeback: MutationWriteback,
    ) -> Result<(), LowerError> {
        if let Some((place, local)) = writeback {
            self.block_mut()?.statements.push(Statement::AssignPlace {
                place,
                value: Rvalue::Use(Operand::Copy(Place::Local(local))),
            });
        }
        Ok(())
    }

    /// Lowers an lvalue expression to a MIR place for assignment targets.
    fn lower_place(&mut self, expr_id: ExprId) -> Result<Place, LowerError> {
        let expr = self.hir_expr(expr_id)?.clone();
        match &expr.kind {
            ExprKind::Local(local) => {
                let local_id = self.locals.get(local).copied().ok_or_else(|| {
                    self.error("assignment references an unknown local", Some(expr.span))
                })?;
                Ok(Place::Local(local_id))
            }
            ExprKind::Field { receiver, field } => {
                let receiver_operand = self.lower_expr(*receiver)?;
                let receiver_ty = self.hir_expr(*receiver)?.ty;
                let base =
                    self.materialize_operand_local(receiver_operand, receiver_ty, expr.span)?;
                Ok(Place::Field {
                    base,
                    field: *field,
                })
            }
            ExprKind::Index { receiver, index } => {
                let receiver_operand = self.lower_expr(*receiver)?;
                let receiver_ty = self.hir_expr(*receiver)?.ty;
                let base =
                    self.materialize_operand_local(receiver_operand, receiver_ty, expr.span)?;
                let index_operand = self.lower_expr(*index)?;
                Ok(Place::Index {
                    base,
                    index: Box::new(index_operand),
                })
            }
            ExprKind::TupleIndex { tuple, index } => {
                let tuple_operand = self.lower_expr(*tuple)?;
                let tuple_ty = self.hir_expr(*tuple)?.ty;
                let base = self.materialize_operand_local(tuple_operand, tuple_ty, expr.span)?;
                let tuple_index = i64::try_from(*index).map_err(|_error| {
                    self.error("tuple index does not fit in MIR integer", Some(expr.span))
                })?;
                Ok(Place::Index {
                    base,
                    index: Box::new(Operand::Const(Constant::Int(tuple_index))),
                })
            }
            ExprKind::TypeAssert { value } | ExprKind::UnknownCast { value, .. } => {
                self.lower_place(*value)
            }
            ExprKind::Literal(_)
            | ExprKind::Item(_)
            | ExprKind::Call { .. }
            | ExprKind::ClosureCall { .. }
            | ExprKind::ClosureCallSpread { .. }
            | ExprKind::Method { .. }
            | ExprKind::OptionalField { .. }
            | ExprKind::OptionalIndex { .. }
            | ExprKind::OptionalMethod { .. }
            | ExprKind::OptionalCoalesce { .. }
            | ExprKind::Len { .. }
            | ExprKind::NumericAbs { .. }
            | ExprKind::NumericRound { .. }
            | ExprKind::NumericExtrema { .. }
            | ExprKind::NumericHypot { .. }
            | ExprKind::NumericPredicate { .. }
            | ExprKind::NumericUnaryFunc { .. }
            | ExprKind::NumericPow { .. }
            | ExprKind::NumericAtan2 { .. }
            | ExprKind::NumericRandom
            | ExprKind::NumericRandomInt { .. }
            | ExprKind::NumericToStringRadix { .. }
            | ExprKind::PrimitiveCast { .. }
            | ExprKind::StringCase { .. }
            | ExprKind::StringNormalize { .. }
            | ExprKind::StringTrim { .. }
            | ExprKind::StringAffix { .. }
            | ExprKind::StringSearch { .. }
            | ExprKind::StringReplace { .. }
            | ExprKind::StringRemoveAffix { .. }
            | ExprKind::StringRepeat { .. }
            | ExprKind::StringPad { .. }
            | ExprKind::StringPredicate { .. }
            | ExprKind::RegexIsMatch { .. }
            | ExprKind::RegexReplace { .. }
            | ExprKind::RegexReplaceCallback { .. }
            | ExprKind::RegexReplaceFirstMatchUppercase { .. }
            | ExprKind::RegexSplit { .. }
            | ExprKind::RegexFind { .. }
            | ExprKind::RegexExec { .. }
            | ExprKind::RegexMatchAll { .. }
            | ExprKind::StringCharAt { .. }
            | ExprKind::StringCharCodeAt { .. }
            | ExprKind::StringContains { .. }
            | ExprKind::StringSlice { .. }
            | ExprKind::ListContains { .. }
            | ExprKind::SetContains { .. }
            | ExprKind::SetDisjoint { .. }
            | ExprKind::SetRelation { .. }
            | ExprKind::SetAdd { .. }
            | ExprKind::SetRemove { .. }
            | ExprKind::SetClear { .. }
            | ExprKind::SetCopy { .. }
            | ExprKind::ListToSet { .. }
            | ExprKind::ListPairsToDict { .. }
            | ExprKind::SetBinary { .. }
            | ExprKind::SetProjection { .. }
            | ExprKind::ListConcat { .. }
            | ExprKind::ListSearch { .. }
            | ExprKind::ListCallback { .. }
            | ExprKind::ListFromLength { .. }
            | ExprKind::ListRepeat { .. }
            | ExprKind::ListFromLengthMap { .. }
            | ExprKind::ListReduce { .. }
            | ExprKind::ListSlice { .. }
            | ExprKind::ListSplice { .. }
            | ExprKind::ListFill { .. }
            | ExprKind::ListCopyWithin { .. }
            | ExprKind::ListWith { .. }
            | ExprKind::ListFlat { .. }
            | ExprKind::ListProjection { .. }
            | ExprKind::ListPush { .. }
            | ExprKind::ListExtend { .. }
            | ExprKind::ListInsert { .. }
            | ExprKind::ListUnshift { .. }
            | ExprKind::ListReverse { .. }
            | ExprKind::ListClear { .. }
            | ExprKind::ListCopy { .. }
            | ExprKind::TupleToList { .. }
            | ExprKind::ListToTuple { .. }
            | ExprKind::TupleToSet { .. }
            | ExprKind::ListCount { .. }
            | ExprKind::ListSum { .. }
            | ExprKind::ListBoolFold { .. }
            | ExprKind::ListSorted { .. }
            | ExprKind::ListReversed { .. }
            | ExprKind::ListEnumerate { .. }
            | ExprKind::ListZip { .. }
            | ExprKind::ListRange { .. }
            | ExprKind::ListRandomChoice { .. }
            | ExprKind::ListIndex { .. }
            | ExprKind::ListRemove { .. }
            | ExprKind::ListSort { .. }
            | ExprKind::ListPop { .. }
            | ExprKind::ListShift { .. }
            | ExprKind::ListNext { .. }
            | ExprKind::IteratorDone { .. }
            | ExprKind::IteratorValue { .. }
            | ExprKind::TupleContains { .. }
            | ExprKind::TupleSlice { .. }
            | ExprKind::DictContainsKey { .. }
            | ExprKind::DictSet { .. }
            | ExprKind::DictRemoveKey { .. }
            | ExprKind::DictGet { .. }
            | ExprKind::DictSetDefault { .. }
            | ExprKind::DictClear { .. }
            | ExprKind::DictPop { .. }
            | ExprKind::DictUpdate { .. }
            | ExprKind::DictAssign { .. }
            | ExprKind::CallableObjectAssign { .. }
            | ExprKind::DictCopy { .. }
            | ExprKind::DictProjection { .. }
            | ExprKind::StringSplit { .. }
            | ExprKind::StringChars { .. }
            | ExprKind::StringJoin { .. }
            | ExprKind::JsonStringify { .. }
            | ExprKind::JsonParse { .. }
            | ExprKind::HttpGetText { .. }
            | ExprKind::DateNow
            | ExprKind::DateSetNow { .. }
            | ExprKind::DateResetNow
            | ExprKind::DateTimezoneOffset
            | ExprKind::DateSetTimezoneOffset { .. }
            | ExprKind::DateResetTimezoneOffset
            | ExprKind::DateTimezoneContext { .. }
            | ExprKind::DateToIsoString { .. }
            | ExprKind::DateToString { .. }
            | ExprKind::DateFromParts { .. }
            | ExprKind::DateFromValue { .. }
            | ExprKind::DateGetPart { .. }
            | ExprKind::DateSetPart { .. }
            | ExprKind::UrlField { .. }
            | ExprKind::FileReadText { .. }
            | ExprKind::FileWriteText { .. }
            | ExprKind::BinOp { .. }
            | ExprKind::UnaryOp { .. }
            | ExprKind::Conditional { .. }
            | ExprKind::FunctionTableLookup { .. }
            | ExprKind::InstanceOf { .. }
            | ExprKind::UnknownIs { .. }
            | ExprKind::TypeofValue { .. }
            | ExprKind::Block(_)
            | ExprKind::Lambda { .. }
            | ExprKind::Closure(_)
            | ExprKind::ListLit(_)
            | ExprKind::SetLit(_)
            | ExprKind::DictLit(_)
            | ExprKind::TupleLit(_)
            | ExprKind::New { .. }
            | ExprKind::Await(_)
            | ExprKind::AsyncOp { .. } => Err(self.error(
                "only local, field, and index expressions can be assigned",
                Some(expr.span),
            )),
        }
    }

    /// Extracts a local variable ID from an operand or returns an error.
    fn local_operand(&self, operand: Operand, span: Span) -> Result<LocalId, LowerError> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => match place {
                Place::Local(local) => Ok(local),
                Place::Field { .. } | Place::Index { .. } => Err(self.error(
                    "field and index reads currently require a local receiver",
                    Some(span),
                )),
            },
            Operand::Const(_) => Err(self.error(
                "field and index reads currently require a local receiver",
                Some(span),
            )),
        }
    }

    /// Materializes an operand into a local so it can be used as a MIR place base.
    fn materialize_operand_local(
        &mut self,
        operand: Operand,
        ty: TypeId,
        span: Span,
    ) -> Result<LocalId, LowerError> {
        if let Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) = operand {
            return Ok(local);
        }
        let local = self.push_temp(ty, span);
        self.block_mut()?.statements.push(Statement::Assign {
            dest: local,
            value: Rvalue::Use(operand),
        });
        Ok(local)
    }

    /// Returns a reference to the current block.
    fn block(&self) -> Result<&BasicBlock, LowerError> {
        let block_index = usize_from_u32(
            self.current_block.0,
            "MIR block index does not fit in usize",
        )?;
        self.function
            .blocks
            .get(block_index)
            .ok_or_else(|| self.error("MIR block index should be valid", None))
    }

    /// Returns a mutable reference to the current block.
    fn block_mut(&mut self) -> Result<&mut BasicBlock, LowerError> {
        let block_index = usize_from_u32(
            self.current_block.0,
            "MIR block index does not fit in usize",
        )?;
        match self.function.blocks.get_mut(block_index) {
            Some(block) => Ok(block),
            None => Err(LowerError {
                message: "MIR block index should be valid".to_owned(),
                span: None,
            }),
        }
    }

    /// Returns a reference to a MIR local declaration.
    fn mir_local(&self, local: LocalId) -> Result<&LocalDecl, LowerError> {
        let local_index = usize_from_u32(local.0, "MIR local index does not fit in usize")?;
        self.function
            .locals
            .get(local_index)
            .ok_or_else(|| self.error("MIR local index should be valid", None))
    }

    /// Returns a HIR item by ID.
    fn hir_item(&self, item_id: smelt_hir::ItemId) -> Result<&smelt_hir::Item, LowerError> {
        let item_index = usize_from_u32(item_id.0, "HIR item index does not fit in usize")?;
        self.krate
            .items
            .get(item_index)
            .ok_or_else(|| self.error("HIR item index should be valid", None))
    }

    /// Returns a HIR block by ID.
    fn hir_block(&self, block_id: smelt_hir::BlockId) -> Result<&smelt_hir::Block, LowerError> {
        let block_index = usize_from_u32(block_id.0, "HIR block index does not fit in usize")?;
        self.body
            .blocks
            .get(block_index)
            .ok_or_else(|| self.error("HIR block index should be valid", None))
    }

    /// Returns a HIR statement by ID.
    fn hir_stmt(&self, stmt_id: smelt_hir::StmtId) -> Result<&HirStmt, LowerError> {
        let stmt_index = usize_from_u32(stmt_id.0, "HIR statement index does not fit in usize")?;
        self.body
            .stmts
            .get(stmt_index)
            .ok_or_else(|| self.error("HIR statement index should be valid", None))
    }

    /// Returns a HIR expression by ID.
    fn hir_expr(&self, expr_id: ExprId) -> Result<&smelt_hir::Expr, LowerError> {
        let expr_index = usize_from_u32(expr_id.0, "HIR expr index does not fit in usize")?;
        self.body
            .exprs
            .get(expr_index)
            .ok_or_else(|| self.error("HIR expr index should be valid", None))
    }

    /// Returns a HIR pattern by ID.
    fn hir_pattern(
        &self,
        pattern_id: smelt_hir::PatternId,
    ) -> Result<&smelt_hir::Pattern, LowerError> {
        let pattern_index =
            usize_from_u32(pattern_id.0, "HIR pattern index does not fit in usize")?;
        self.body
            .patterns
            .get(pattern_index)
            .ok_or_else(|| self.error("HIR pattern index should be valid", None))
    }

    /// Sets the terminator for the current block.
    fn set_terminator(&mut self, terminator: Terminator) -> Result<(), LowerError> {
        self.block_mut()?.terminator = Some(terminator);
        Ok(())
    }

    /// Returns the active source-language exception handler for a throwing call.
    fn current_exception_handler(&self) -> Option<ExceptionHandler> {
        self.exception_targets
            .last()
            .map(|target| ExceptionHandler {
                catch_block: target.catch_block,
                exception_local: target.exception_local,
            })
    }

    /// Creates a lowering error with optional span information.
    fn error(&self, message: impl Into<String>, error_span: Option<Span>) -> LowerError {
        let mut message_text = message.into();
        if let Some(span_value) = error_span
            && let Some(module) = usize::try_from(span_value.file.0)
                .ok()
                .and_then(|index| self.krate.modules.get(index))
        {
            message_text = format!("{message_text} at {}", module.source.path);
        }
        LowerError {
            message: message_text,
            span: error_span,
        }
    }
}

/// Converts a HIR literal to a MIR constant.
fn lower_literal(literal: &HirLiteral) -> Constant {
    match literal {
        HirLiteral::Bool(value) => Constant::Bool(*value),
        HirLiteral::Int(value) => Constant::Int(*value),
        HirLiteral::Float(value) => Constant::Float(*value),
        HirLiteral::String(value) => Constant::String(value.clone()),
        HirLiteral::Symbol(value) => Constant::Symbol(value.clone()),
        HirLiteral::None => Constant::None,
    }
}

/// Returns the output type of a Future type.
fn future_inner_type(krate: &smelt_hir::Crate, ty: TypeId) -> Option<TypeId> {
    match krate.types.get(ty) {
        Some(Type::Future(inner)) => Some(*inner),
        _ => None,
    }
}
