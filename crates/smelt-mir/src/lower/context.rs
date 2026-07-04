//! The [`LoweringCtx`] state machine and its construction.
//!
//! A [`LoweringCtx`] carries every piece of mutable state needed to lower one
//! HIR body (a function, method, constructor, module top level, or closure)
//! into a single [`MirFunction`]. It owns the in-progress function, the HIR/MIR
//! local and expression maps, the block cursor, and the loop/exception target
//! stacks. Expression, statement, place, and closure lowering are implemented
//! as methods on this type in sibling modules; this module owns only the shared
//! state, its two constructors, and the small accessor helpers that every other
//! module builds on (`block`, `hir_expr`, `error`, and friends).

use std::collections::HashMap;

use smelt_hir::{
    Body, BodyId, ExprId, LocalId as HirLocalId, Span, Stmt as HirStmt, Symbol, TypeId,
};

use crate::{
    BasicBlock, BlockId, ExceptionHandler, FuncId, HirOrigin, LocalDecl, LocalId, LocalKind,
    MirClosure, MirFunction, Operand, Terminator,
};

use super::{LowerError, function_item_for_body, u32_from_usize, usize_from_u32};

/// Context for lowering a function body from HIR to MIR.
pub(super) struct LoweringCtx<'hir> {
    /// Reference to the HIR crate.
    pub(super) krate: &'hir smelt_hir::Crate,
    /// Mapping of HIR item IDs to MIR function IDs.
    pub(super) item_functions: &'hir HashMap<smelt_hir::ItemId, FuncId>,
    /// The HIR body being lowered.
    pub(super) body: &'hir Body,
    /// The MIR function being constructed.
    pub(super) function: MirFunction,
    /// MIR closures constructed while lowering this function.
    pub(super) closures: Vec<MirClosure>,
    /// Global closure id offset for closures constructed by this context.
    pub(super) closure_base: usize,
    /// The current block being generated.
    pub(super) current_block: BlockId,
    /// Mapping of HIR local IDs to MIR local IDs.
    pub(super) locals: HashMap<HirLocalId, LocalId>,
    /// Mapping of HIR expression IDs to lowered operands.
    pub(super) exprs: HashMap<ExprId, Operand>,
    /// Stack of loop targets for break/continue.
    pub(super) loops: Vec<LoopTargets>,
    /// Stack of lexical exception targets for throws inside try blocks.
    pub(super) exception_targets: Vec<ExceptionTarget>,
    /// Numeric type used for generated index-based loop counters.
    pub(super) loop_index_ty: TypeId,
    /// Boolean type used for generated loop conditions.
    pub(super) loop_bool_ty: TypeId,
}

/// Target blocks for break and continue statements.
#[derive(Debug, Clone, Copy)]
pub(super) struct LoopTargets {
    /// Target block for break statements.
    pub(super) break_target: BlockId,
    /// Target block for continue statements.
    pub(super) continue_target: BlockId,
}

/// Target block and optional binding for a lexical catch clause.
#[derive(Debug, Clone, Copy)]
pub(super) struct ExceptionTarget {
    /// Target block for caught throw statements.
    pub(super) catch_block: BlockId,
    /// Local receiving the thrown value when the catch has a binding.
    pub(super) exception_local: Option<LocalId>,
}

/// Crate-wide inputs shared by every body lowered in one `lower_hir` run.
///
/// Grouping these into a single value keeps the [`LoweringCtx`] constructors
/// under a manageable argument count: they carry the interned HIR crate, the
/// item-to-`FuncId` map, the two synthetic loop helper types, and the global
/// closure-id offset that the constructed context should start numbering from.
#[derive(Clone, Copy)]
pub(super) struct LoweringShared<'hir> {
    /// Reference to the HIR crate being lowered.
    pub(super) krate: &'hir smelt_hir::Crate,
    /// Mapping of HIR item IDs to MIR function IDs.
    pub(super) item_functions: &'hir HashMap<smelt_hir::ItemId, FuncId>,
    /// Numeric type used for generated index-based loop counters.
    pub(super) loop_index_ty: TypeId,
    /// Boolean type used for generated loop conditions.
    pub(super) loop_bool_ty: TypeId,
    /// Global closure id offset for closures this context constructs.
    pub(super) closure_base: usize,
}

/// Per-function identity and signature inputs for [`LoweringCtx::new`].
///
/// These are the fields that distinguish a top-level or item body from the
/// shared crate frame in [`LoweringShared`]: the target `FuncId`, the source
/// body, its name, return type, structural owner, and async-ness.
#[derive(Clone, Copy)]
pub(super) struct FunctionSpec {
    /// The MIR function id to assign to the lowered body.
    pub(super) function_id: FuncId,
    /// The HIR body id backing this function.
    pub(super) body_id: BodyId,
    /// The function's interned name symbol.
    pub(super) name: Symbol,
    /// The (already future-unwrapped) MIR return type.
    pub(super) return_ty: TypeId,
    /// Structural owner (module, constructor, or class method).
    pub(super) owner: smelt_hir::FunctionOwner,
    /// Whether the source function is `async`.
    pub(super) is_async: bool,
}

/// Mapping from HIR local ids to their allocated MIR local ids.
type LocalMap = HashMap<HirLocalId, LocalId>;

/// Build the HIR-to-MIR local map for `body`, pushing param locals in order.
///
/// Both constructors need the same walk: every HIR local becomes a MIR local,
/// parameters are tagged [`LocalKind::Param`] and appended to `function.params`,
/// and the HIR-to-MIR id mapping is returned for later expression lowering.
fn build_locals(function: &mut MirFunction, body: &Body) -> Result<LocalMap, LowerError> {
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
    Ok(locals)
}

impl<'hir> LoweringCtx<'hir> {
    /// Creates a new lowering context for a function body.
    pub(super) fn new(
        shared: LoweringShared<'hir>,
        spec: FunctionSpec,
        body: &'hir Body,
    ) -> Result<Self, LowerError> {
        let FunctionSpec {
            function_id,
            body_id,
            name,
            return_ty,
            owner,
            is_async,
        } = spec;
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
            smelt_hir::FunctionOwner::ClassStaticMethod { class, method } => {
                HirOrigin::ClassStaticMethod {
                    class,
                    method,
                    body: body_id,
                }
            }
        };
        let mut function = MirFunction::new(function_id, name, origin, return_ty, span);
        function.is_async = is_async;
        if let Some(hir_function) = function_item_for_body(shared.krate, body_id) {
            function.is_test = hir_function.is_test;
            function.rest = hir_function.rest;
        }
        let locals = build_locals(&mut function, body)?;

        Ok(Self::from_parts(shared, body, function, locals))
    }

    /// Creates a lowering context for a nested closure body.
    pub(super) fn new_closure(
        shared: LoweringShared<'hir>,
        body_id: BodyId,
        body: &'hir Body,
        return_ty: TypeId,
        span: Span,
    ) -> Result<Self, LowerError> {
        let mut function = MirFunction::new(
            FuncId(u32::MAX),
            Symbol(u32::MAX),
            HirOrigin::Body(body_id),
            return_ty,
            span,
        );
        let locals = build_locals(&mut function, body)?;

        Ok(Self::from_parts(shared, body, function, locals))
    }

    /// Assemble a context from a prepared function and local map.
    ///
    /// Both constructors converge here once they have built the `MirFunction`
    /// skeleton and the HIR-to-MIR local map, so the shared-frame fields are
    /// unpacked in exactly one place.
    fn from_parts(
        shared: LoweringShared<'hir>,
        body: &'hir Body,
        function: MirFunction,
        locals: LocalMap,
    ) -> Self {
        Self {
            krate: shared.krate,
            item_functions: shared.item_functions,
            body,
            function,
            closures: Vec::new(),
            closure_base: shared.closure_base,
            current_block: BlockId(0),
            locals,
            exprs: HashMap::new(),
            loops: Vec::new(),
            exception_targets: Vec::new(),
            loop_index_ty: shared.loop_index_ty,
            loop_bool_ty: shared.loop_bool_ty,
        }
    }

    /// Allocates a new temporary local variable.
    pub(super) fn push_temp(&mut self, ty: TypeId, span: Span) -> LocalId {
        self.function.push_local(LocalDecl {
            ty,
            kind: LocalKind::Temp,
            span,
        })
    }

    /// Returns a reference to the current block.
    pub(super) fn block(&self) -> Result<&BasicBlock, LowerError> {
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
    pub(super) fn block_mut(&mut self) -> Result<&mut BasicBlock, LowerError> {
        let block_index = usize_from_u32(
            self.current_block.0,
            "MIR block index does not fit in usize",
        )?;
        self.function
            .blocks
            .get_mut(block_index)
            .ok_or_else(|| LowerError {
                message: "MIR block index should be valid".to_owned(),
                span: None,
            })
    }

    /// Returns a reference to a MIR local declaration.
    pub(super) fn mir_local(&self, local: LocalId) -> Result<&LocalDecl, LowerError> {
        let local_index = usize_from_u32(local.0, "MIR local index does not fit in usize")?;
        self.function
            .locals
            .get(local_index)
            .ok_or_else(|| self.error("MIR local index should be valid", None))
    }

    /// Returns a HIR item by ID.
    pub(super) fn hir_item(&self, item_id: smelt_hir::ItemId) -> Result<&smelt_hir::Item, LowerError> {
        let item_index = usize_from_u32(item_id.0, "HIR item index does not fit in usize")?;
        self.krate
            .items
            .get(item_index)
            .ok_or_else(|| self.error("HIR item index should be valid", None))
    }

    /// Returns a HIR block by ID.
    pub(super) fn hir_block(&self, block_id: smelt_hir::BlockId) -> Result<&smelt_hir::Block, LowerError> {
        let block_index = usize_from_u32(block_id.0, "HIR block index does not fit in usize")?;
        self.body
            .blocks
            .get(block_index)
            .ok_or_else(|| self.error("HIR block index should be valid", None))
    }

    /// Returns a HIR statement by ID.
    pub(super) fn hir_stmt(&self, stmt_id: smelt_hir::StmtId) -> Result<&HirStmt, LowerError> {
        let stmt_index = usize_from_u32(stmt_id.0, "HIR statement index does not fit in usize")?;
        self.body
            .stmts
            .get(stmt_index)
            .ok_or_else(|| self.error("HIR statement index should be valid", None))
    }

    /// Returns a HIR expression by ID.
    pub(super) fn hir_expr(&self, expr_id: ExprId) -> Result<&smelt_hir::Expr, LowerError> {
        let expr_index = usize_from_u32(expr_id.0, "HIR expr index does not fit in usize")?;
        self.body
            .exprs
            .get(expr_index)
            .ok_or_else(|| self.error("HIR expr index should be valid", None))
    }

    /// Returns a HIR pattern by ID.
    pub(super) fn hir_pattern(
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
    pub(super) fn set_terminator(&mut self, terminator: Terminator) -> Result<(), LowerError> {
        self.block_mut()?.terminator = Some(terminator);
        Ok(())
    }

    /// Returns the active source-language exception handler for a throwing call.
    pub(super) fn current_exception_handler(&self) -> Option<ExceptionHandler> {
        self.exception_targets
            .last()
            .map(|target| ExceptionHandler {
                catch_block: target.catch_block,
                exception_local: target.exception_local,
            })
    }

    /// Creates a lowering error with optional span information.
    pub(super) fn error(&self, message: impl Into<String>, error_span: Option<Span>) -> LowerError {
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
