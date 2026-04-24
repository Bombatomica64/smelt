use serde::{Deserialize, Serialize};

use crate::expr::{Expr, Literal};
use crate::ids::{
    BlockId, BodyId, ExprId, ItemId, LocalId, PatternId, Span, StmtId, Symbol, TypeId,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Body {
    pub id: BodyId,
    pub owner: Option<ItemId>,
    pub locals: Vec<LocalDecl>,
    pub params: Vec<LocalId>,
    pub exprs: Vec<Expr>,
    pub stmts: Vec<Stmt>,
    pub blocks: Vec<Block>,
    pub patterns: Vec<Pattern>,
    pub root: BlockId,
}

impl Body {
    #[must_use]
    pub fn new(owner: Option<ItemId>, span: Span) -> Self {
        Self {
            id: BodyId(u32::MAX),
            owner,
            locals: Vec::new(),
            params: Vec::new(),
            exprs: Vec::new(),
            stmts: Vec::new(),
            blocks: vec![Block {
                stmts: Vec::new(),
                tail: None,
                span,
            }],
            patterns: Vec::new(),
            root: BlockId(0),
        }
    }

    pub fn push_local(&mut self, local: LocalDecl) -> LocalId {
        let id = LocalId(self.locals.len() as u32);
        self.locals.push(local);
        id
    }

    pub fn push_expr(&mut self, expr: Expr) -> ExprId {
        let id = ExprId(self.exprs.len() as u32);
        self.exprs.push(expr);
        id
    }

    pub fn push_stmt(&mut self, stmt: Stmt) -> StmtId {
        let id = StmtId(self.stmts.len() as u32);
        self.stmts.push(stmt);
        self.blocks[self.root.0 as usize].stmts.push(id);
        id
    }

    pub fn push_pattern(&mut self, pattern: Pattern) -> PatternId {
        let id = PatternId(self.patterns.len() as u32);
        self.patterns.push(pattern);
        id
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalDecl {
    pub name: Option<Symbol>,
    pub ty: TypeId,
    pub mutable: bool,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub stmts: Vec<StmtId>,
    pub tail: Option<ExprId>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Stmt {
    Let {
        pat: PatternId,
        ty: TypeId,
        value: Option<ExprId>,
    },
    Expr(ExprId),
    Return(Option<ExprId>),
    Throw(ExprId),
    Break,
    Continue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Pattern {
    Wildcard,
    Binding(LocalId),
    Tuple(Vec<PatternId>),
    Literal(Literal),
}
