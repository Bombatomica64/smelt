//! Lowering from HIR to MIR.
//!
//! This module contains the logic for converting a HIR crate into a MIR representation,
//! including lowering expressions, statements, and control flow.

use std::collections::HashMap;
use std::convert::TryFrom;

use smelt_hir::{
    Body, BodyId, ExprId, ExprKind, Literal as HirLiteral, LocalId as HirLocalId, Span,
    Stmt as HirStmt, Symbol, Type, TypeId,
};

use crate::{
    BasicBlock, BlockId, BuiltinFn, Callee, Constant, FuncId, HirOrigin, LocalDecl, LocalId,
    LocalKind, Mir, MirFunction, Operand, Place, Rvalue, Statement, Terminator,
};

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

/// An error encountered during HIR to MIR lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LowerError {
    /// The error message.
    pub message: String,
    /// The source span associated with the error, if available.
    pub span: Option<Span>,
}

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

    let mut mir = Mir::new(krate.types.clone(), krate.symbols.clone());
    let none = mir.types.intern(Type::None);
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
                    base: class.base,
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
                    implements: class.implements.clone(),
                });
            }
            smelt_hir::Item::Interface(interface) => {
                mir.interfaces.push(crate::MirInterface {
                    name: interface.name,
                    fields: interface
                        .fields
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
        )
        .and_then(LoweringCtx::lower)
        {
            Ok(lowered_function) => {
                mir.push_function(lowered_function);
            }
            Err(error) => errors.push(error),
        }
    }

    for module in &krate.modules {
        let Some(body_id) = module.body else {
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
        )
        .and_then(LoweringCtx::lower)
        {
            Ok(function) => {
                mir.push_function(function);
            }
            Err(error) => errors.push(error),
        }
    }

    if errors.is_empty() {
        propagate_throwing_functions(&mut mir);
        Ok(mir)
    } else {
        Err(errors)
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
        let mut locals = HashMap::new();

        for (idx, local) in body.locals.iter().enumerate() {
            let hir_local = HirLocalId(u32_from_usize(idx, "HIR local index does not fit in u32")?);
            let is_param = body.params.contains(&hir_local);
            let kind = if is_param {
                LocalKind::Param
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
            current_block: BlockId(0),
            locals,
            exprs: HashMap::new(),
            loops: Vec::new(),
            exception_targets: Vec::new(),
        })
    }

    /// Lowers the entire function body to MIR.
    fn lower(mut self) -> Result<MirFunction, LowerError> {
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
        if let Some(tail) = root.tail {
            let operand = self.lower_expr(tail)?;
            self.set_terminator(Terminator::Return(operand))?;
        } else if self.block()?.terminator.is_none() {
            if matches!(self.function.origin, HirOrigin::ClassConstructor { .. }) {
                self.set_terminator(Terminator::Return(Operand::Move(Place::Local(LocalId(0)))))?;
                return Ok(self.function);
            }
            self.set_terminator(Terminator::Return(Operand::Const(Constant::None)))?;
        }

        Ok(self.function)
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
        let header = self.current_block;
        let body_span = self.hir_block(body_hir)?.span;
        let body_mir = self.function.push_block(body_span);
        let after = self.function.push_block(self.block()?.span);
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
        let float_ty = if let Some(idx) = self
            .krate
            .types
            .all()
            .iter()
            .position(|ty| *ty == Type::Float)
        {
            TypeId(u32_from_usize(idx, "HIR type index does not fit in u32")?)
        } else {
            let local_index = usize_from_u32(hir_local.0, "HIR local index does not fit in usize")?;
            self.body
                .locals
                .get(local_index)
                .map(|local| local.ty)
                .ok_or_else(|| self.error("HIR local index should be valid", None))?
        };
        let bool_ty = if let Some(idx) = self
            .krate
            .types
            .all()
            .iter()
            .position(|ty| *ty == Type::Bool)
        {
            TypeId(u32_from_usize(idx, "HIR type index does not fit in u32")?)
        } else {
            float_ty
        };
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

        for arm in arms {
            let target = self.function.push_block(span);
            mir_arms.push(crate::MatchArm {
                label: lower_literal(&arm.label),
                target,
            });
            arm_blocks.push((target, arm.body));
        }

        let default_target = default.map(|_| self.function.push_block(span));
        self.set_terminator(Terminator::Match {
            scrutinee: lowered_scrutinee,
            arms: mir_arms,
            default: default_target,
        })?;

        for (target, hir_block) in arm_blocks {
            self.current_block = target;
            self.lower_block_stmts(hir_block)?;
            if self.block()?.terminator.is_none() {
                self.set_terminator(Terminator::Goto(join))?;
            }
        }

        if let (Some(target), Some(default_block)) = (default_target, default) {
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
                Operand::Copy(Place::Local(local_id))
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
                let base = self.local_operand(receiver_operand, expr.span)?;
                Operand::Copy(Place::Field {
                    base,
                    field: *field,
                })
            }
            ExprKind::Index { receiver, index } => {
                let receiver_operand = self.lower_expr(*receiver)?;
                let base = self.local_operand(receiver_operand, expr.span)?;
                let index_operand = self.lower_expr(*index)?;
                Operand::Copy(Place::Index {
                    base,
                    index: Box::new(index_operand),
                })
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
            } => {
                let haystack_operand = self.lower_expr(*haystack)?;
                let needle_operand = self.lower_expr(*needle)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::StringSearch {
                        op: *op,
                        haystack: haystack_operand,
                        needle: needle_operand,
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
            } => {
                let haystack_operand = self.lower_expr(*haystack)?;
                let separator_operand = self.lower_expr(*separator)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::StringSplit {
                        haystack: haystack_operand,
                        separator: separator_operand,
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
            ExprKind::HttpGetText { url } => {
                let url_operand = self.lower_expr(*url)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::HttpGetText { url: url_operand },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::Method {
                receiver,
                method,
                args,
            } => {
                let receiver_operand = self.lower_expr(*receiver)?;
                let base = self.local_operand(receiver_operand.clone(), expr.span)?;
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
                })?;
                self.current_block = target;
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::New { class, args } => {
                let callee_id = self.resolve_constructor(*class, expr.span)?;
                let lowered_args = args
                    .iter()
                    .map(|arg| self.lower_expr(*arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                let target = self.function.push_block(expr.span);
                self.set_terminator(Terminator::Call {
                    callee: Callee::Static(callee_id),
                    args: lowered_args,
                    dest,
                    target,
                })?;
                self.current_block = target;
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::Await(future) => {
                let lowered_future = self.lower_expr(*future)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::Await(lowered_future),
                });
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
            ExprKind::Block(_) | ExprKind::Lambda { .. } | ExprKind::SetLit(_) => {
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
    fn resolve_constructor(&self, class: Symbol, span: Span) -> Result<FuncId, LowerError> {
        for item in &self.krate.items {
            if let smelt_hir::Item::Class(class_item) = item
                && class_item.name == class
                && let Some(constructor) = class_item.constructor
                && let Some(func) = self.item_functions.get(&constructor)
            {
                return Ok(*func);
            }
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
        for item in &self.krate.items {
            if let smelt_hir::Item::Class(class_item) = item
                && class_item.name == *name
            {
                for method_item in &class_item.methods {
                    if let smelt_hir::Item::Function(function) = self.hir_item(*method_item)?
                        && function.name == method
                        && let Some(func) = self.item_functions.get(method_item)
                    {
                        return Ok(*func);
                    }
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
                let base = self.local_operand(receiver_operand, expr.span)?;
                Ok(Place::Field {
                    base,
                    field: *field,
                })
            }
            ExprKind::Index { receiver, index } => {
                let receiver_operand = self.lower_expr(*receiver)?;
                let base = self.local_operand(receiver_operand, expr.span)?;
                let index_operand = self.lower_expr(*index)?;
                Ok(Place::Index {
                    base,
                    index: Box::new(index_operand),
                })
            }
            ExprKind::Literal(_)
            | ExprKind::Item(_)
            | ExprKind::Call { .. }
            | ExprKind::Method { .. }
            | ExprKind::Len { .. }
            | ExprKind::NumericAbs { .. }
            | ExprKind::NumericRound { .. }
            | ExprKind::NumericExtrema { .. }
            | ExprKind::NumericHypot { .. }
            | ExprKind::NumericPredicate { .. }
            | ExprKind::NumericUnaryFunc { .. }
            | ExprKind::NumericPow { .. }
            | ExprKind::StringCase { .. }
            | ExprKind::StringTrim { .. }
            | ExprKind::StringAffix { .. }
            | ExprKind::StringSearch { .. }
            | ExprKind::StringReplace { .. }
            | ExprKind::StringRemoveAffix { .. }
            | ExprKind::StringRepeat { .. }
            | ExprKind::StringPredicate { .. }
            | ExprKind::StringCharAt { .. }
            | ExprKind::StringCharCodeAt { .. }
            | ExprKind::StringContains { .. }
            | ExprKind::StringSlice { .. }
            | ExprKind::ListContains { .. }
            | ExprKind::ListConcat { .. }
            | ExprKind::ListSearch { .. }
            | ExprKind::ListSlice { .. }
            | ExprKind::TupleContains { .. }
            | ExprKind::DictContainsKey { .. }
            | ExprKind::DictProjection { .. }
            | ExprKind::StringSplit { .. }
            | ExprKind::StringJoin { .. }
            | ExprKind::HttpGetText { .. }
            | ExprKind::BinOp { .. }
            | ExprKind::UnaryOp { .. }
            | ExprKind::Block(_)
            | ExprKind::Lambda { .. }
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

    /// Creates a lowering error with optional span information.
    fn error(&self, message: impl Into<String>, span: Option<Span>) -> LowerError {
        LowerError {
            message: message.into(),
            span,
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

/// Marks functions that can reach an uncaught throw directly or through static calls.
fn propagate_throwing_functions(mir: &mut Mir) {
    loop {
        let throwing = mir
            .functions
            .iter()
            .map(|function| function.can_throw)
            .collect::<Vec<_>>();
        let mut changed = false;

        for function in &mut mir.functions {
            if function.can_throw {
                continue;
            }
            let can_throw = function.blocks.iter().any(|block| {
                block
                    .terminator
                    .as_ref()
                    .is_some_and(|terminator| terminator_can_throw(terminator, &throwing))
            });
            if can_throw {
                function.can_throw = true;
                changed = true;
            }
        }

        if !changed {
            break;
        }
    }
}

/// Returns whether a terminator can leave through an uncaught exception path.
fn terminator_can_throw(terminator: &Terminator, throwing: &[bool]) -> bool {
    match terminator {
        Terminator::Throw(_) => true,
        Terminator::Call {
            callee: Callee::Static(func),
            ..
        } => match usize_from_u32(func.0, "MIR function index does not fit in usize") {
            Ok(index) => throwing.get(index).copied().unwrap_or(false),
            Err(_) => false,
        },
        Terminator::Goto(_)
        | Terminator::Call { .. }
        | Terminator::Switch { .. }
        | Terminator::Match { .. }
        | Terminator::Return(_)
        | Terminator::Unreachable => false,
    }
}
