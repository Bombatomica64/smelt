use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use smelt_hir::{
    Body, BodyId, ExprId, ExprKind, Literal as HirLiteral, LocalId as HirLocalId, Span,
    Stmt as HirStmt, Symbol, Type, TypeId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FuncId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LocalId(pub u32);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mir {
    pub functions: Vec<MirFunction>,
    pub types: smelt_hir::TypeInterner,
    pub symbols: smelt_hir::SymbolInterner,
}

impl Mir {
    #[must_use]
    pub fn new(types: smelt_hir::TypeInterner, symbols: smelt_hir::SymbolInterner) -> Self {
        Self {
            functions: Vec::new(),
            types,
            symbols,
        }
    }

    pub fn push_function(&mut self, mut function: MirFunction) -> FuncId {
        let id = FuncId(self.functions.len() as u32);
        function.id = id;
        self.functions.push(function);
        id
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirFunction {
    pub id: FuncId,
    pub name: Symbol,
    pub origin: HirOrigin,
    pub is_async: bool,
    pub params: Vec<LocalId>,
    pub return_ty: TypeId,
    pub locals: Vec<LocalDecl>,
    pub blocks: Vec<BasicBlock>,
    pub entry: BlockId,
}

impl MirFunction {
    fn new(name: Symbol, origin: HirOrigin, return_ty: TypeId, span: Span) -> Self {
        Self {
            id: FuncId(u32::MAX),
            name,
            origin,
            is_async: false,
            params: Vec::new(),
            return_ty,
            locals: Vec::new(),
            blocks: vec![BasicBlock {
                id: BlockId(0),
                phis: Vec::new(),
                statements: Vec::new(),
                terminator: None,
                span,
            }],
            entry: BlockId(0),
        }
    }

    fn push_local(&mut self, local: LocalDecl) -> LocalId {
        let id = LocalId(self.locals.len() as u32);
        self.locals.push(local);
        id
    }

    fn push_block(&mut self, span: Span) -> BlockId {
        let id = BlockId(self.blocks.len() as u32);
        self.blocks.push(BasicBlock {
            id,
            phis: Vec::new(),
            statements: Vec::new(),
            terminator: None,
            span,
        });
        id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirOrigin {
    Body(BodyId),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalDecl {
    pub ty: TypeId,
    pub kind: LocalKind,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocalKind {
    Param,
    Temp,
    UserBinding(Symbol),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicBlock {
    pub id: BlockId,
    pub phis: Vec<Phi>,
    pub statements: Vec<Statement>,
    pub terminator: Option<Terminator>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phi {
    pub dest: LocalId,
    pub ty: TypeId,
    pub incoming: Vec<(BlockId, Operand)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Place {
    Local(LocalId),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Operand {
    Copy(Place),
    Move(Place),
    Const(Constant),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Constant {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Rvalue {
    Use(Operand),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Statement {
    Assign { dest: LocalId, value: Rvalue },
    StorageLive(LocalId),
    StorageDead(LocalId),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Terminator {
    Goto(BlockId),
    Call {
        callee: Callee,
        args: Vec<Operand>,
        dest: LocalId,
        target: BlockId,
    },
    Return(Operand),
    Unreachable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Callee {
    Static(FuncId),
    Indirect(Operand),
    Builtin(BuiltinFn),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuiltinFn {
    ConsoleLog,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LowerError {
    pub message: String,
    pub span: Option<Span>,
}

#[must_use]
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

    for module in &krate.modules {
        let Some(body_id) = module.body else {
            continue;
        };
        let body = &krate.bodies[body_id.0 as usize];
        let name = mir.symbols.intern(&module.name);
        match LoweringCtx::new(krate, body_id, body, name, none).lower() {
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
    body_id: BodyId,
    body: &'hir Body,
    function: MirFunction,
    current_block: BlockId,
    locals: HashMap<HirLocalId, LocalId>,
    exprs: HashMap<ExprId, Operand>,
}

impl<'hir> LoweringCtx<'hir> {
    fn new(
        krate: &'hir smelt_hir::Crate,
        body_id: BodyId,
        body: &'hir Body,
        name: Symbol,
        return_ty: TypeId,
    ) -> Self {
        let span = body.blocks[body.root.0 as usize].span;
        let mut function = MirFunction::new(name, HirOrigin::Body(body_id), return_ty, span);
        let mut locals = HashMap::new();

        for (idx, local) in body.locals.iter().enumerate() {
            let hir_local = HirLocalId(idx as u32);
            let kind = local.name.map_or(LocalKind::Temp, LocalKind::UserBinding);
            let mir_local = function.push_local(LocalDecl {
                ty: local.ty,
                kind,
                span: local.span,
            });
            locals.insert(hir_local, mir_local);
        }

        Self {
            krate,
            body_id,
            body,
            function,
            current_block: BlockId(0),
            locals,
            exprs: HashMap::new(),
        }
    }

    fn lower(mut self) -> Result<MirFunction, LowerError> {
        let root = &self.body.blocks[self.body.root.0 as usize];
        for stmt_id in &root.stmts {
            let stmt = &self.body.stmts[stmt_id.0 as usize];
            self.lower_stmt(stmt)?;
        }

        if let Some(tail) = root.tail {
            let operand = self.lower_expr(tail)?;
            self.set_terminator(Terminator::Return(operand));
        } else if self.block().terminator.is_none() {
            self.set_terminator(Terminator::Return(Operand::Const(Constant::None)));
        }

        Ok(self.function)
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
            HirStmt::Throw(_) => Err(self.error("throw lowering is not implemented yet", None)),
            HirStmt::Break | HirStmt::Continue => {
                Err(self.error("loop control lowering is not implemented yet", None))
            }
        }
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
            ExprKind::Method { .. }
            | ExprKind::Field { .. }
            | ExprKind::Index { .. }
            | ExprKind::BinOp { .. }
            | ExprKind::UnaryOp { .. }
            | ExprKind::Block(_)
            | ExprKind::Lambda { .. }
            | ExprKind::ListLit(_)
            | ExprKind::SetLit(_)
            | ExprKind::DictLit(_)
            | ExprKind::TupleLit(_)
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
        if name == "console_log" {
            Ok(Callee::Builtin(BuiltinFn::ConsoleLog))
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
        let _ = self.body_id;
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

pub mod opt {
    use super::*;

    pub trait Pass {
        fn name(&self) -> &'static str;
        fn run(&self, mir: &mut Mir) -> bool;
    }

    #[derive(Debug, Default)]
    pub struct CopyPropagation;

    impl Pass for CopyPropagation {
        fn name(&self) -> &'static str {
            "copy-propagation"
        }

        fn run(&self, mir: &mut Mir) -> bool {
            let mut changed = false;
            for function in &mut mir.functions {
                changed |= propagate_function(function);
            }
            changed
        }
    }

    #[must_use]
    pub fn default_passes() -> Vec<Box<dyn Pass>> {
        vec![Box::<CopyPropagation>::default()]
    }

    pub fn optimize(mir: &mut Mir) {
        for pass in default_passes() {
            pass.run(mir);
        }
    }

    fn propagate_function(function: &mut MirFunction) -> bool {
        let mut aliases = HashMap::new();

        for block in &function.blocks {
            for stmt in &block.statements {
                if let Statement::Assign {
                    dest,
                    value: Rvalue::Use(Operand::Copy(Place::Local(source))),
                } = stmt
                {
                    aliases.insert(*dest, resolve_alias(&aliases, *source));
                }
            }
        }

        if aliases.is_empty() {
            return false;
        }

        let mut changed = false;
        for block in &mut function.blocks {
            for phi in &mut block.phis {
                for (_, operand) in &mut phi.incoming {
                    changed |= rewrite_operand(operand, &aliases);
                }
            }
            for stmt in &mut block.statements {
                if let Statement::Assign { dest, value } = stmt {
                    changed |= rewrite_rvalue(value, &aliases, Some(*dest));
                }
            }
            if let Some(terminator) = &mut block.terminator {
                changed |= rewrite_terminator(terminator, &aliases);
            }
        }

        changed
    }

    fn resolve_alias(aliases: &HashMap<LocalId, LocalId>, local: LocalId) -> LocalId {
        let mut current = local;
        let mut seen = HashSet::new();
        while let Some(next) = aliases.get(&current).copied() {
            if !seen.insert(current) {
                break;
            }
            current = next;
        }
        current
    }

    fn rewrite_rvalue(
        value: &mut Rvalue,
        aliases: &HashMap<LocalId, LocalId>,
        dest: Option<LocalId>,
    ) -> bool {
        match value {
            Rvalue::Use(operand) => rewrite_operand_except(operand, aliases, dest),
        }
    }

    fn rewrite_terminator(
        terminator: &mut Terminator,
        aliases: &HashMap<LocalId, LocalId>,
    ) -> bool {
        match terminator {
            Terminator::Goto(_) | Terminator::Unreachable => false,
            Terminator::Return(operand) => rewrite_operand(operand, aliases),
            Terminator::Call {
                callee, args, dest, ..
            } => {
                let mut changed = match callee {
                    Callee::Indirect(operand) => rewrite_operand(operand, aliases),
                    Callee::Static(_) | Callee::Builtin(_) => false,
                };
                for arg in args {
                    changed |= rewrite_operand_except(arg, aliases, Some(*dest));
                }
                changed
            }
        }
    }

    fn rewrite_operand(operand: &mut Operand, aliases: &HashMap<LocalId, LocalId>) -> bool {
        rewrite_operand_except(operand, aliases, None)
    }

    fn rewrite_operand_except(
        operand: &mut Operand,
        aliases: &HashMap<LocalId, LocalId>,
        except: Option<LocalId>,
    ) -> bool {
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = operand
        else {
            return false;
        };
        if Some(*local) == except {
            return false;
        }
        let resolved = resolve_alias(aliases, *local);
        if resolved == *local {
            return false;
        }
        *local = resolved;
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub message: String,
}

#[must_use]
pub fn validate(mir: &Mir) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    for function in &mir.functions {
        validate_function(mir, function, &mut errors);
    }
    errors
}

fn validate_function(mir: &Mir, function: &MirFunction, errors: &mut Vec<ValidationError>) {
    if function.blocks.get(function.entry.0 as usize).is_none() {
        errors.push(error(format!(
            "function {:?} has an unknown entry block {:?}",
            function.id, function.entry
        )));
    }

    let mut definitions = HashSet::new();
    for param in &function.params {
        definitions.insert(*param);
    }

    for (block_idx, block) in function.blocks.iter().enumerate() {
        if block.id.0 as usize != block_idx {
            errors.push(error(format!(
                "function {:?} block index {block_idx} has mismatched id {:?}",
                function.id, block.id
            )));
        }
        if block.terminator.is_none() {
            errors.push(error(format!(
                "function {:?} block {:?} is missing a terminator",
                function.id, block.id
            )));
        }

        for phi in &block.phis {
            validate_type(mir, phi.ty, errors);
            for (_, operand) in &phi.incoming {
                validate_operand(function, &definitions, operand, errors);
            }
            define_local(function, &mut definitions, phi.dest, errors);
        }

        for stmt in &block.statements {
            match stmt {
                Statement::Assign { dest, value } => {
                    validate_rvalue(function, &definitions, value, errors);
                    define_local(function, &mut definitions, *dest, errors);
                }
                Statement::StorageLive(local) | Statement::StorageDead(local) => {
                    validate_local_exists(function, *local, errors);
                }
            }
        }

        if let Some(terminator) = &block.terminator {
            match terminator {
                Terminator::Goto(target) => validate_block_exists(function, *target, errors),
                Terminator::Call {
                    callee,
                    args,
                    dest,
                    target,
                } => {
                    validate_callee(mir, function, &definitions, callee, errors);
                    for arg in args {
                        validate_operand(function, &definitions, arg, errors);
                    }
                    define_local(function, &mut definitions, *dest, errors);
                    validate_block_exists(function, *target, errors);
                }
                Terminator::Return(operand) => {
                    validate_operand(function, &definitions, operand, errors);
                }
                Terminator::Unreachable => {}
            }
        }
    }

    for local in &function.locals {
        validate_type(mir, local.ty, errors);
    }
}

fn define_local(
    function: &MirFunction,
    definitions: &mut HashSet<LocalId>,
    local: LocalId,
    errors: &mut Vec<ValidationError>,
) {
    validate_local_exists(function, local, errors);
    if !definitions.insert(local) {
        errors.push(error(format!(
            "function {:?} local {:?} is defined more than once",
            function.id, local
        )));
    }
}

fn validate_rvalue(
    function: &MirFunction,
    definitions: &HashSet<LocalId>,
    value: &Rvalue,
    errors: &mut Vec<ValidationError>,
) {
    match value {
        Rvalue::Use(operand) => validate_operand(function, definitions, operand, errors),
    }
}

fn validate_operand(
    function: &MirFunction,
    definitions: &HashSet<LocalId>,
    operand: &Operand,
    errors: &mut Vec<ValidationError>,
) {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => {
            validate_place(function, definitions, place, errors);
        }
        Operand::Const(_) => {}
    }
}

fn validate_place(
    function: &MirFunction,
    definitions: &HashSet<LocalId>,
    place: &Place,
    errors: &mut Vec<ValidationError>,
) {
    match place {
        Place::Local(local) => {
            validate_local_exists(function, *local, errors);
            if !definitions.contains(local) {
                errors.push(error(format!(
                    "function {:?} reads local {:?} before it is defined",
                    function.id, local
                )));
            }
        }
    }
}

fn validate_callee(
    mir: &Mir,
    function: &MirFunction,
    definitions: &HashSet<LocalId>,
    callee: &Callee,
    errors: &mut Vec<ValidationError>,
) {
    match callee {
        Callee::Static(func) => {
            if mir.functions.get(func.0 as usize).is_none() {
                errors.push(error(format!("call references unknown function {func:?}")));
            }
        }
        Callee::Indirect(operand) => {
            validate_operand(function, definitions, operand, errors);
        }
        Callee::Builtin(_) => {}
    }
}

fn validate_local_exists(
    function: &MirFunction,
    local: LocalId,
    errors: &mut Vec<ValidationError>,
) {
    if function.locals.get(local.0 as usize).is_none() {
        errors.push(error(format!(
            "function {:?} references unknown local {:?}",
            function.id, local
        )));
    }
}

fn validate_block_exists(
    function: &MirFunction,
    block: BlockId,
    errors: &mut Vec<ValidationError>,
) {
    if function.blocks.get(block.0 as usize).is_none() {
        errors.push(error(format!(
            "function {:?} references unknown block {:?}",
            function.id, block
        )));
    }
}

fn validate_type(mir: &Mir, ty: TypeId, errors: &mut Vec<ValidationError>) {
    if mir.types.get(ty).is_none() {
        errors.push(error(format!("MIR references unknown type {ty:?}")));
    }
}

fn error(message: String) -> ValidationError {
    ValidationError { message }
}

#[must_use]
pub fn format_compact(mir: &Mir) -> String {
    let mut out = String::new();
    for function in &mir.functions {
        let name = mir.symbols.get(function.name).unwrap_or("<unknown>");
        out.push_str(&format!(
            "fn {name} ({:?}) -> {}\n",
            function.id,
            type_ref(mir, function.return_ty)
        ));

        if !function.locals.is_empty() {
            out.push_str("  locals\n");
            for (idx, local) in function.locals.iter().enumerate() {
                out.push_str(&format!(
                    "    {} {}: {}\n",
                    local_ref(LocalId(idx as u32)),
                    local_kind_text(mir, local.kind),
                    type_ref(mir, local.ty)
                ));
            }
        }

        for block in &function.blocks {
            out.push_str(&format!("  bb{}:\n", block.id.0));
            for phi in &block.phis {
                out.push_str(&format!(
                    "    {} = phi {}\n",
                    local_ref(phi.dest),
                    type_ref(mir, phi.ty)
                ));
            }
            for statement in &block.statements {
                out.push_str(&format!("    {}\n", statement_text(statement)));
            }
            let terminator = block
                .terminator
                .as_ref()
                .map(terminator_text)
                .unwrap_or_else(|| "<missing terminator>".to_owned());
            out.push_str(&format!("    {terminator}\n"));
        }
        out.push('\n');
    }
    out
}

fn local_kind_text(mir: &Mir, kind: LocalKind) -> String {
    match kind {
        LocalKind::Param => "param".to_owned(),
        LocalKind::Temp => "temp".to_owned(),
        LocalKind::UserBinding(symbol) => {
            format!("user {}", mir.symbols.get(symbol).unwrap_or("<unknown>"))
        }
    }
}

fn statement_text(statement: &Statement) -> String {
    match statement {
        Statement::Assign { dest, value } => {
            format!("{} = {}", local_ref(*dest), rvalue_text(value))
        }
        Statement::StorageLive(local) => format!("StorageLive({})", local_ref(*local)),
        Statement::StorageDead(local) => format!("StorageDead({})", local_ref(*local)),
    }
}

fn rvalue_text(value: &Rvalue) -> String {
    match value {
        Rvalue::Use(operand) => operand_text(operand),
    }
}

fn terminator_text(terminator: &Terminator) -> String {
    match terminator {
        Terminator::Goto(target) => format!("goto bb{}", target.0),
        Terminator::Call {
            callee,
            args,
            dest,
            target,
        } => {
            let args = args.iter().map(operand_text).collect::<Vec<_>>().join(", ");
            format!(
                "{} = call {}({}) -> bb{}",
                local_ref(*dest),
                callee_text(callee),
                args,
                target.0
            )
        }
        Terminator::Return(operand) => format!("return {}", operand_text(operand)),
        Terminator::Unreachable => "unreachable".to_owned(),
    }
}

fn callee_text(callee: &Callee) -> String {
    match callee {
        Callee::Static(func) => format!("fn{}", func.0),
        Callee::Indirect(operand) => operand_text(operand),
        Callee::Builtin(BuiltinFn::ConsoleLog) => "@console_log".to_owned(),
    }
}

fn operand_text(operand: &Operand) -> String {
    match operand {
        Operand::Copy(place) => format!("copy {}", place_text(place)),
        Operand::Move(place) => format!("move {}", place_text(place)),
        Operand::Const(constant) => constant_text(constant),
    }
}

fn place_text(place: &Place) -> String {
    match place {
        Place::Local(local) => local_ref(*local),
    }
}

fn constant_text(constant: &Constant) -> String {
    match constant {
        Constant::Bool(value) => value.to_string(),
        Constant::Int(value) => value.to_string(),
        Constant::Float(value) => {
            if value.fract() == 0.0 {
                format!("{value:.1}")
            } else {
                value.to_string()
            }
        }
        Constant::String(value) => format!("\"{value}\""),
        Constant::None => "none".to_owned(),
    }
}

fn local_ref(local: LocalId) -> String {
    format!("%{}", local.0)
}

fn type_ref(mir: &Mir, ty: TypeId) -> String {
    let Some(ty_value) = mir.types.get(ty) else {
        return format!("t{}", ty.0);
    };
    match ty_value {
        Type::Bool => "Bool".to_owned(),
        Type::Int => "Int".to_owned(),
        Type::Float => "Float".to_owned(),
        Type::String => "String".to_owned(),
        Type::None => "None".to_owned(),
        Type::List(item) => format!("List<{}>", type_ref(mir, *item)),
        Type::Set(item) => format!("Set<{}>", type_ref(mir, *item)),
        Type::Dict(key, value) => {
            format!("Dict<{}, {}>", type_ref(mir, *key), type_ref(mir, *value))
        }
        Type::Tuple(items) => {
            let items = items
                .iter()
                .map(|item| type_ref(mir, *item))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({items})")
        }
        Type::Optional(item) => format!("Optional<{}>", type_ref(mir, *item)),
        Type::Union(items) => items
            .iter()
            .map(|item| type_ref(mir, *item))
            .collect::<Vec<_>>()
            .join(" | "),
        Type::Class { name, args } => {
            let name = mir.symbols.get(*name).unwrap_or("<unknown>");
            if args.is_empty() {
                name.to_owned()
            } else {
                let args = args
                    .iter()
                    .map(|arg| type_ref(mir, *arg))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{name}<{args}>")
            }
        }
        Type::Function(function) => {
            let params = function
                .params
                .iter()
                .map(|param| type_ref(mir, *param))
                .collect::<Vec<_>>()
                .join(", ");
            let async_prefix = if function.is_async { "async " } else { "" };
            format!(
                "{async_prefix}fn({params}) -> {}",
                type_ref(mir, function.return_ty)
            )
        }
        Type::Future(item) => format!("Future<{}>", type_ref(mir, *item)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smelt_frontend_ts::{HirCtx, to_hir};
    use smelt_hir::FileId;

    #[test]
    fn lowers_top_level_let_and_console_log_to_mir() {
        let mut ctx = HirCtx::new();
        to_hir(
            "let count = 42;\nconsole.log(count);\n",
            FileId(0),
            &mut ctx,
        )
        .expect("HIR");

        let mut mir = lower_hir(&ctx.krate).expect("MIR");
        opt::optimize(&mut mir);

        assert!(validate(&mir).is_empty());
        let output = format_compact(&mir);
        assert!(output.contains("fn main (FuncId(0)) -> None"));
        assert!(output.contains("%0 user count: Float"));
        assert!(output.contains("%0 = 42.0"));
        assert!(output.contains("%1 = call @console_log(copy %0) -> bb1"));
        assert!(output.contains("return none"));
    }

    #[test]
    fn copy_propagation_rewrites_alias_uses() {
        let mut ctx = HirCtx::new();
        to_hir(
            "const sourceValue = 7;\nlet copiedValue = sourceValue;\nconsole.log(copiedValue);\n",
            FileId(0),
            &mut ctx,
        )
        .expect("HIR");

        let mut mir = lower_hir(&ctx.krate).expect("MIR");
        opt::optimize(&mut mir);
        let output = format_compact(&mir);

        assert!(output.contains("%2 = call @console_log(copy %0) -> bb1"));
    }
}
