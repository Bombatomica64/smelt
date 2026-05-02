use std::collections::HashMap;

use smelt_hir::{
    Body, BodyId, ExprId, ExprKind, Literal as HirLiteral, LocalId as HirLocalId, Span,
    Stmt as HirStmt, Symbol, Type, TypeId,
};

use crate::{
    BasicBlock, BlockId, BuiltinFn, Callee, Constant, FuncId, HirOrigin, LocalDecl, LocalId,
    LocalKind, Mir, MirFunction, Operand, Place, Rvalue, Statement, Terminator,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LowerError {
    pub message: String,
    pub span: Option<Span>,
}

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

struct LoweringCtx<'hir> {
    krate: &'hir smelt_hir::Crate,
    item_functions: &'hir HashMap<smelt_hir::ItemId, FuncId>,
    body: &'hir Body,
    function: MirFunction,
    current_block: BlockId,
    locals: HashMap<HirLocalId, LocalId>,
    exprs: HashMap<ExprId, Operand>,
}

impl<'hir> LoweringCtx<'hir> {
    fn new(
        krate: &'hir smelt_hir::Crate,
        item_functions: &'hir HashMap<smelt_hir::ItemId, FuncId>,
        function_id: FuncId,
        body_id: BodyId,
        body: &'hir Body,
        name: Symbol,
        return_ty: TypeId,
    ) -> Self {
        let span = body.blocks[body.root.0 as usize].span;
        let mut function =
            MirFunction::new(function_id, name, HirOrigin::Body(body_id), return_ty, span);
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
        }
    }

    fn lower(mut self) -> Result<MirFunction, LowerError> {
        self.lower_block_stmts(self.body.root)?;

        let root = &self.body.blocks[self.body.root.0 as usize];
        if let Some(tail) = root.tail {
            let operand = self.lower_expr(tail)?;
            self.set_terminator(Terminator::Return(operand));
        } else if self.block().terminator.is_none() {
            self.set_terminator(Terminator::Return(Operand::Const(Constant::None)));
        }

        Ok(self.function)
    }

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
            HirStmt::While { .. } => {
                Err(self.error("while CFG lowering is not implemented yet", None))
            }
            HirStmt::For { .. } => Err(self.error("for CFG lowering is not implemented yet", None)),
            HirStmt::Throw(_) => Err(self.error("throw lowering is not implemented yet", None)),
            HirStmt::Break | HirStmt::Continue => {
                Err(self.error("loop control lowering is not implemented yet", None))
            }
        }
    }

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
            ExprKind::Method { .. }
            | ExprKind::Block(_)
            | ExprKind::Lambda { .. }
            | ExprKind::SetLit(_)
            | ExprKind::New { .. } => {
                return Err(self.error(
                    "expression kind is not implemented in MIR yet",
                    Some(expr.span),
                ));
            }
        };

        self.exprs.insert(expr_id, operand.clone());
        Ok(operand)
    }

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

    fn push_temp(&mut self, ty: TypeId, span: Span) -> LocalId {
        self.function.push_local(LocalDecl {
            ty,
            kind: LocalKind::Temp,
            span,
        })
    }

    fn local_operand(&self, operand: Operand, span: Span) -> Result<LocalId, LowerError> {
        match operand {
            Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => Ok(local),
            _ => Err(self.error(
                "field and index reads currently require a local receiver",
                Some(span),
            )),
        }
    }

    fn block(&self) -> &BasicBlock {
        &self.function.blocks[self.current_block.0 as usize]
    }

    fn block_mut(&mut self) -> &mut BasicBlock {
        &mut self.function.blocks[self.current_block.0 as usize]
    }

    fn set_terminator(&mut self, terminator: Terminator) {
        self.block_mut().terminator = Some(terminator);
    }

    fn error(&self, message: impl Into<String>, span: Option<Span>) -> LowerError {
        LowerError {
            message: message.into(),
            span,
        }
    }
}

fn lower_literal(literal: &HirLiteral) -> Constant {
    match literal {
        HirLiteral::Bool(value) => Constant::Bool(*value),
        HirLiteral::Int(value) => Constant::Int(*value),
        HirLiteral::Float(value) => Constant::Float(*value),
        HirLiteral::String(value) => Constant::String(value.clone()),
        HirLiteral::None => Constant::None,
    }
}
