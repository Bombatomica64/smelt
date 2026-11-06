//! Reading a `Body`'s arenas by id.

use smelt_hir::{Block, BlockId, Body, Expr, ExprId, LocalDecl, LocalId, Stmt, StmtId};

/// Lookups in a `Body`'s arenas. An id from another body answers `None`.
pub(in crate::lowering) trait BodyArena {
    /// The expression `id` names.
    fn expr_at(&self, id: ExprId) -> Option<&Expr>;
    /// The expression `id` names, for rewriting in place.
    fn expr_at_mut(&mut self, id: ExprId) -> Option<&mut Expr>;
    /// The local `id` names.
    fn local_at(&self, id: LocalId) -> Option<&LocalDecl>;
    /// The local `id` names, for rewriting in place.
    fn local_at_mut(&mut self, id: LocalId) -> Option<&mut LocalDecl>;
    /// The statement `id` names.
    fn stmt_at(&self, id: StmtId) -> Option<&Stmt>;
    /// The block `id` names.
    fn block_at(&self, id: BlockId) -> Option<&Block>;
    /// The block `id` names, for rewriting in place.
    fn block_at_mut(&mut self, id: BlockId) -> Option<&mut Block>;
}

impl BodyArena for Body {
    fn expr_at(&self, id: ExprId) -> Option<&Expr> {
        self.exprs.get(usize::try_from(id.0).ok()?)
    }

    fn expr_at_mut(&mut self, id: ExprId) -> Option<&mut Expr> {
        self.exprs.get_mut(usize::try_from(id.0).ok()?)
    }

    fn local_at(&self, id: LocalId) -> Option<&LocalDecl> {
        self.locals.get(usize::try_from(id.0).ok()?)
    }

    fn local_at_mut(&mut self, id: LocalId) -> Option<&mut LocalDecl> {
        self.locals.get_mut(usize::try_from(id.0).ok()?)
    }

    fn stmt_at(&self, id: StmtId) -> Option<&Stmt> {
        self.stmts.get(usize::try_from(id.0).ok()?)
    }

    fn block_at(&self, id: BlockId) -> Option<&Block> {
        self.blocks.get(usize::try_from(id.0).ok()?)
    }

    fn block_at_mut(&mut self, id: BlockId) -> Option<&mut Block> {
        self.blocks.get_mut(usize::try_from(id.0).ok()?)
    }
}
