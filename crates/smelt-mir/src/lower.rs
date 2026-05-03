//! Lowering from HIR to MIR.
//!
//! This module contains the logic for converting a HIR crate into a MIR representation,
//! including lowering expressions, statements, and control flow.

use std::collections::HashMap;

use smelt_hir::{
    Body, BodyId, ExprId, ExprKind, Literal as HirLiteral, LocalId as HirLocalId, Span,
    Stmt as HirStmt, Symbol, Type, TypeId,
};

use crate::{
    BasicBlock, BlockId, BuiltinFn, Callee, Constant, FuncId, HirOrigin, LocalDecl, LocalId,
    LocalKind, Mir, MirFunction, Operand, Place, Rvalue, Statement, Terminator,
};

/// An error encountered during HIR to MIR lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
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

    for (idx, item) in krate.items.iter().enumerate() {
        let item_id = smelt_hir::ItemId(idx as u32);
        if let smelt_hir::Item::Function(function) = item
            && function.body.is_some()
        {
            item_functions.insert(item_id, FuncId(item_functions.len() as u32));
        }
    }

    for item in &krate.items {
        match item {
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
                        .and_then(|item| item_functions.get(&item).copied()),
                    methods: class
                        .methods
                        .iter()
                        .filter_map(|item| item_functions.get(item).copied())
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
            _ => {}
        }
    }

    for (idx, item) in krate.items.iter().enumerate() {
        let item_id = smelt_hir::ItemId(idx as u32);
        let smelt_hir::Item::Function(function) = item else {
            continue;
        };
        let Some(body_id) = function.body else {
            continue;
        };
        let body = &krate.bodies[body_id.0 as usize];
        match LoweringCtx::new(
            krate,
            &item_functions,
            item_functions[&item_id],
            body_id,
            body,
            function.name,
            function.return_ty,
            function.owner,
        )
        .lower()
        {
            Ok(function) => {
                mir.push_function(function);
            }
            Err(error) => errors.push(error),
        }
    }

    for module in &krate.modules {
        let Some(body_id) = module.body else {
            continue;
        };
        let body = &krate.bodies[body_id.0 as usize];
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
        )
        .lower()
        {
            Ok(function) => {
                mir.push_function(function);
            }
            Err(error) => errors.push(error),
        }
    }

    if errors.is_empty() {
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
}

/// Target blocks for break and continue statements.
#[derive(Debug, Clone, Copy)]
struct LoopTargets {
    /// Target block for break statements.
    break_target: BlockId,
    /// Target block for continue statements.
    continue_target: BlockId,
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
    ) -> Self {
        let span = body.blocks[body.root.0 as usize].span;
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
        let mut locals = HashMap::new();

        for (idx, local) in body.locals.iter().enumerate() {
            let hir_local = HirLocalId(idx as u32);
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

        Self {
            krate,
            item_functions,
            body,
            function,
            current_block: BlockId(0),
            locals,
            exprs: HashMap::new(),
            loops: Vec::new(),
        }
    }

    /// Lowers the entire function body to MIR.
    fn lower(mut self) -> Result<MirFunction, LowerError> {
        if let HirOrigin::ClassConstructor { class, .. } = self.function.origin {
            let Some(this) = self.function.locals.first().map(|_| LocalId(0)) else {
                return Err(self.error("constructor is missing synthetic this local", None));
            };
            self.block_mut().statements.push(Statement::Assign {
                dest: this,
                value: Rvalue::Struct {
                    class,
                    fields: Vec::new(),
                },
            });
        }
        self.lower_block_stmts(self.body.root)?;

        let root = &self.body.blocks[self.body.root.0 as usize];
        if let Some(tail) = root.tail {
            let operand = self.lower_expr(tail)?;
            self.set_terminator(Terminator::Return(operand));
        } else if self.block().terminator.is_none() {
            if matches!(self.function.origin, HirOrigin::ClassConstructor { .. }) {
                self.set_terminator(Terminator::Return(Operand::Move(Place::Local(LocalId(0)))));
                return Ok(self.function);
            }
            self.set_terminator(Terminator::Return(Operand::Const(Constant::None)));
        }

        Ok(self.function)
    }

    /// Lowers all statements in a HIR block.
    fn lower_block_stmts(&mut self, block_id: smelt_hir::BlockId) -> Result<(), LowerError> {
        let block = &self.body.blocks[block_id.0 as usize];
        for stmt_id in &block.stmts {
            if self.block().terminator.is_some() {
                break;
            }
            let stmt = &self.body.stmts[stmt_id.0 as usize];
            self.lower_stmt(stmt)?;
        }
        Ok(())
    }

    /// Lowers a single HIR statement to MIR.
    fn lower_stmt(&mut self, stmt: &HirStmt) -> Result<(), LowerError> {
        match stmt {
            HirStmt::Let { pat, value, .. } => {
                let smelt_hir::Pattern::Binding(hir_local) = self.body.patterns[pat.0 as usize]
                else {
                    return Err(self.error("only binding let patterns can lower to MIR yet", None));
                };
                let Some(dest) = self.locals.get(&hir_local).copied() else {
                    return Err(self.error("let pattern references an unknown local", None));
                };
                let value = value
                    .map(|expr| self.lower_expr(expr))
                    .transpose()?
                    .unwrap_or(Operand::Const(Constant::None));
                self.block_mut().statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::Use(value),
                });
                Ok(())
            }
            HirStmt::Assign { target, value } => {
                let place = self.lower_place(*target)?;
                let value = self.lower_expr(*value)?;
                self.block_mut().statements.push(Statement::AssignPlace {
                    place,
                    value: Rvalue::Use(value),
                });
                Ok(())
            }
            HirStmt::Expr(expr) => {
                self.lower_expr(*expr)?;
                Ok(())
            }
            HirStmt::Return(Some(expr)) => {
                let operand = self.lower_expr(*expr)?;
                self.set_terminator(Terminator::Return(operand));
                Ok(())
            }
            HirStmt::Return(None) => {
                self.set_terminator(Terminator::Return(Operand::Const(Constant::None)));
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
            HirStmt::Throw(_) => Err(self.error("throw lowering is not implemented yet", None)),
            HirStmt::Break => {
                let Some(targets) = self.loops.last().copied() else {
                    return Err(self.error("break used outside a loop", None));
                };
                self.set_terminator(Terminator::Goto(targets.break_target));
                Ok(())
            }
            HirStmt::Continue => {
                let Some(targets) = self.loops.last().copied() else {
                    return Err(self.error("continue used outside a loop", None));
                };
                self.set_terminator(Terminator::Goto(targets.continue_target));
                Ok(())
            }
        }
    }

    /// Lowers an if statement to MIR with switch terminator.
    fn lower_if(
        &mut self,
        cond: ExprId,
        then_hir: smelt_hir::BlockId,
        else_hir: Option<smelt_hir::BlockId>,
    ) -> Result<(), LowerError> {
        let cond = self.lower_expr(cond)?;
        let then_span = self.body.blocks[then_hir.0 as usize].span;
        let else_span = else_hir
            .map(|block| self.body.blocks[block.0 as usize].span)
            .unwrap_or_else(|| self.block().span);
        let then_mir = self.function.push_block(then_span);
        let else_mir = self.function.push_block(else_span);
        self.set_terminator(Terminator::Switch {
            cond,
            then_block: then_mir,
            else_block: else_mir,
        });

        self.current_block = then_mir;
        self.lower_block_stmts(then_hir)?;

        if let Some(else_hir) = else_hir {
            let join = self.function.push_block(self.block().span);
            if self.block().terminator.is_none() {
                self.set_terminator(Terminator::Goto(join));
            }

            self.current_block = else_mir;
            self.lower_block_stmts(else_hir)?;
            if self.block().terminator.is_none() {
                self.set_terminator(Terminator::Goto(join));
            }
            self.current_block = join;
        } else {
            if self.block().terminator.is_none() {
                self.set_terminator(Terminator::Goto(else_mir));
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
        let body_span = self.body.blocks[body_hir.0 as usize].span;
        let body_mir = self.function.push_block(body_span);
        let after = self.function.push_block(self.block().span);
        let cond = self.lower_expr(cond)?;
        self.set_terminator(Terminator::Switch {
            cond,
            then_block: body_mir,
            else_block: after,
        });

        self.loops.push(LoopTargets {
            break_target: after,
            continue_target: header,
        });
        self.current_block = body_mir;
        self.lower_block_stmts(body_hir)?;
        if self.block().terminator.is_none() {
            self.set_terminator(Terminator::Goto(header));
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
        let smelt_hir::Pattern::Binding(hir_local) = self.body.patterns[pat.0 as usize] else {
            return Err(self.error("only binding for patterns can lower to MIR yet", None));
        };
        let item_local = self
            .locals
            .get(&hir_local)
            .copied()
            .ok_or_else(|| self.error("for pattern references an unknown local", None))?;
        let iter_operand = self.lower_expr(iter)?;
        let iter_local =
            self.local_operand(iter_operand.clone(), self.body.exprs[iter.0 as usize].span)?;
        let float_ty = self
            .krate
            .types
            .all()
            .iter()
            .position(|ty| *ty == Type::Float)
            .map(|idx| TypeId(idx as u32))
            .unwrap_or(self.body.locals[hir_local.0 as usize].ty);
        let bool_ty = self
            .krate
            .types
            .all()
            .iter()
            .position(|ty| *ty == Type::Bool)
            .map(|idx| TypeId(idx as u32))
            .unwrap_or(float_ty);
        let idx = self.push_temp(float_ty, self.body.exprs[iter.0 as usize].span);
        self.block_mut().statements.push(Statement::Assign {
            dest: idx,
            value: Rvalue::Use(Operand::Const(Constant::Float(0.0))),
        });

        let header = self.function.push_block(self.block().span);
        self.set_terminator(Terminator::Goto(header));
        self.current_block = header;
        let len = self.push_temp(float_ty, self.block().span);
        self.block_mut().statements.push(Statement::Assign {
            dest: len,
            value: Rvalue::Len(Operand::Copy(Place::Local(iter_local))),
        });
        let cond = self.push_temp(bool_ty, self.block().span);
        self.block_mut().statements.push(Statement::Assign {
            dest: cond,
            value: Rvalue::Binary {
                op: smelt_hir::BinOp::Lt,
                lhs: Operand::Copy(Place::Local(idx)),
                rhs: Operand::Copy(Place::Local(len)),
            },
        });
        let body_mir = self
            .function
            .push_block(self.body.blocks[body_hir.0 as usize].span);
        let update_block = self.function.push_block(self.block().span);
        let after = self.function.push_block(self.block().span);
        self.set_terminator(Terminator::Switch {
            cond: Operand::Copy(Place::Local(cond)),
            then_block: body_mir,
            else_block: after,
        });

        self.loops.push(LoopTargets {
            break_target: after,
            continue_target: update_block,
        });
        self.current_block = body_mir;
        self.block_mut().statements.push(Statement::Assign {
            dest: item_local,
            value: Rvalue::Use(Operand::Copy(Place::Index {
                base: iter_local,
                index: Box::new(Operand::Copy(Place::Local(idx))),
            })),
        });
        self.lower_block_stmts(body_hir)?;
        if self.block().terminator.is_none() {
            self.set_terminator(Terminator::Goto(update_block));
        }
        self.loops.pop();
        self.current_block = update_block;
        let one = Operand::Const(Constant::Float(1.0));
        self.block_mut().statements.push(Statement::AssignPlace {
            place: Place::Local(idx),
            value: Rvalue::Binary {
                op: smelt_hir::BinOp::Add,
                lhs: Operand::Copy(Place::Local(idx)),
                rhs: one,
            },
        });
        self.set_terminator(Terminator::Goto(header));
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
        let scrutinee = self.lower_expr(scrutinee)?;
        let span = self.block().span;
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
            scrutinee,
            arms: mir_arms,
            default: default_target,
        });

        for (target, hir_block) in arm_blocks {
            self.current_block = target;
            self.lower_block_stmts(hir_block)?;
            if self.block().terminator.is_none() {
                self.set_terminator(Terminator::Goto(join));
            }
        }

        if let (Some(target), Some(default_block)) = (default_target, default) {
            self.current_block = target;
            self.lower_block_stmts(default_block)?;
            if self.block().terminator.is_none() {
                self.set_terminator(Terminator::Goto(join));
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

        let expr = &self.body.exprs[expr_id.0 as usize];
        let operand = match &expr.kind {
            ExprKind::Literal(literal) => Operand::Const(lower_literal(literal)),
            ExprKind::Local(local) => {
                let local = self.locals.get(local).copied().ok_or_else(|| {
                    self.error(
                        "local expression references an unknown local",
                        Some(expr.span),
                    )
                })?;
                Operand::Copy(Place::Local(local))
            }
            ExprKind::Call { callee, args } => {
                let callee = self.lower_callee(*callee)?;
                let args = args
                    .iter()
                    .map(|arg| self.lower_expr(*arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                let target = self.function.push_block(expr.span);
                self.set_terminator(Terminator::Call {
                    callee,
                    args,
                    dest,
                    target,
                });
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
                let lhs = self.lower_expr(*lhs)?;
                let rhs = self.lower_expr(*rhs)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut().statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::Binary { op: *op, lhs, rhs },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::UnaryOp { op, operand } => {
                let operand = self.lower_expr(*operand)?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut().statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::Unary { op: *op, operand },
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::ListLit(items) => {
                let items = items
                    .iter()
                    .map(|item| self.lower_expr(*item))
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut().statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::List(items),
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::DictLit(entries) => {
                let entries = entries
                    .iter()
                    .map(|(key, value)| Ok((self.lower_expr(*key)?, self.lower_expr(*value)?)))
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut().statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::Dict(entries),
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::TupleLit(items) => {
                let items = items
                    .iter()
                    .map(|item| self.lower_expr(*item))
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                self.block_mut().statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::Tuple(items),
                });
                Operand::Copy(Place::Local(dest))
            }
            ExprKind::Field { receiver, field } => {
                let receiver = self.lower_expr(*receiver)?;
                let base = self.local_operand(receiver, expr.span)?;
                Operand::Copy(Place::Field {
                    base,
                    field: *field,
                })
            }
            ExprKind::Index { receiver, index } => {
                let receiver = self.lower_expr(*receiver)?;
                let base = self.local_operand(receiver, expr.span)?;
                let index = self.lower_expr(*index)?;
                Operand::Copy(Place::Index {
                    base,
                    index: Box::new(index),
                })
            }
            ExprKind::Method { .. } => {
                if let ExprKind::Method {
                    receiver,
                    method,
                    args,
                } = &expr.kind
                {
                    let receiver = self.lower_expr(*receiver)?;
                    let base = self.local_operand(receiver.clone(), expr.span)?;
                    let receiver_ty = self.function.locals[base.0 as usize].ty;
                    let callee = self.resolve_method(receiver_ty, *method, expr.span)?;
                    let mut lowered_args = vec![receiver];
                    lowered_args.extend(
                        args.iter()
                            .map(|arg| self.lower_expr(*arg))
                            .collect::<Result<Vec<_>, _>>()?,
                    );
                    let dest = self.push_temp(expr.ty, expr.span);
                    let target = self.function.push_block(expr.span);
                    self.set_terminator(Terminator::Call {
                        callee: Callee::Static(callee),
                        args: lowered_args,
                        dest,
                        target,
                    });
                    self.current_block = target;
                    Operand::Copy(Place::Local(dest))
                } else {
                    unreachable!()
                }
            }
            ExprKind::New { class, args } => {
                let callee = self.resolve_constructor(*class, expr.span)?;
                let lowered_args = args
                    .iter()
                    .map(|arg| self.lower_expr(*arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let dest = self.push_temp(expr.ty, expr.span);
                let target = self.function.push_block(expr.span);
                self.set_terminator(Terminator::Call {
                    callee: Callee::Static(callee),
                    args: lowered_args,
                    dest,
                    target,
                });
                self.current_block = target;
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
        let expr = &self.body.exprs[expr_id.0 as usize];
        let ExprKind::Item(item_id) = expr.kind else {
            return Err(self.error(
                "only direct item calls can lower to MIR yet",
                Some(expr.span),
            ));
        };
        let item = &self.krate.items[item_id.0 as usize];
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
                    if let smelt_hir::Item::Function(function) =
                        &self.krate.items[method_item.0 as usize]
                        && function.name == method
                        && let Some(func) = self.item_functions.get(method_item)
                    {
                        return Ok(*func);
                    }
                }
            }
        }
        let name = self.krate.symbols.get(method).unwrap_or("<unknown>");
        Err(self.error(
            format!("class method `{name}` is not resolvable"),
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
        let expr = &self.body.exprs[expr_id.0 as usize];
        match &expr.kind {
            ExprKind::Local(local) => {
                let local = self.locals.get(local).copied().ok_or_else(|| {
                    self.error("assignment references an unknown local", Some(expr.span))
                })?;
                Ok(Place::Local(local))
            }
            ExprKind::Field { receiver, field } => {
                let receiver = self.lower_expr(*receiver)?;
                let base = self.local_operand(receiver, expr.span)?;
                Ok(Place::Field {
                    base,
                    field: *field,
                })
            }
            ExprKind::Index { receiver, index } => {
                let receiver = self.lower_expr(*receiver)?;
                let base = self.local_operand(receiver, expr.span)?;
                let index = self.lower_expr(*index)?;
                Ok(Place::Index {
                    base,
                    index: Box::new(index),
                })
            }
            _ => Err(self.error(
                "only local, field, and index expressions can be assigned",
                Some(expr.span),
            )),
        }
    }

    /// Extracts a local variable ID from an operand or returns an error.
    fn local_operand(&self, operand: Operand, span: Span) -> Result<LocalId, LowerError> {
        match operand {
            Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => Ok(local),
            _ => Err(self.error(
                "field and index reads currently require a local receiver",
                Some(span),
            )),
        }
    }

    /// Returns a reference to the current block.
    fn block(&self) -> &BasicBlock {
        &self.function.blocks[self.current_block.0 as usize]
    }

    /// Returns a mutable reference to the current block.
    fn block_mut(&mut self) -> &mut BasicBlock {
        &mut self.function.blocks[self.current_block.0 as usize]
    }

    /// Sets the terminator for the current block.
    fn set_terminator(&mut self, terminator: Terminator) {
        self.block_mut().terminator = Some(terminator);
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
