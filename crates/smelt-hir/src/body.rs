//! Body and statement representation for HIR.

use serde::{Deserialize, Serialize};

use crate::expr::{Expr, ExprKind, Literal};
use crate::ids::{
    BlockId, BodyId, ExprId, ItemId, LocalId, PatternId, Span, StmtId, Symbol, TypeId, id_index,
};

/// A function or constant body containing expressions, statements, blocks, and local variables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Body {
    /// Unique identifier for this body.
    pub id: BodyId,
    /// The item that owns this body (function, constant, etc.).
    pub owner: Option<ItemId>,
    /// All local variable declarations in this body.
    pub locals: Vec<LocalDecl>,
    /// Parameter local IDs.
    pub params: Vec<LocalId>,
    /// All expressions in this body.
    pub exprs: Vec<Expr>,
    /// All statements in this body.
    pub stmts: Vec<Stmt>,
    /// All blocks in this body.
    pub blocks: Vec<Block>,
    /// All patterns in this body.
    pub patterns: Vec<Pattern>,
    /// Explicit async state-machine shape for async bodies.
    pub async_state_machine: Option<AsyncStateMachine>,
    /// ID of the root block.
    pub root: BlockId,
}

impl Body {
    /// Creates a new body with the given owner and root block span.
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
            async_state_machine: None,
            root: BlockId(0),
        }
    }

    /// Adds a local variable and returns its ID.
    pub fn push_local(&mut self, local: LocalDecl) -> LocalId {
        let id = LocalId(id_index(self.locals.len()));
        self.locals.push(local);
        id
    }

    /// Adds an expression and returns its ID.
    pub fn push_expr(&mut self, expr: Expr) -> ExprId {
        let id = ExprId(id_index(self.exprs.len()));
        self.exprs.push(expr);
        id
    }

    /// Adds a statement to the root block and returns its ID.
    pub fn push_stmt(&mut self, stmt: Stmt) -> StmtId {
        self.push_stmt_to_block(self.root, stmt)
    }

    /// Adds a statement to a specific block and returns its ID.
    pub fn push_stmt_to_block(&mut self, block: BlockId, stmt: Stmt) -> StmtId {
        let id = StmtId(id_index(self.stmts.len()));
        self.stmts.push(stmt);
        self.blocks[block.0 as usize].stmts.push(id);
        id
    }

    /// Adds a block and returns its ID.
    pub fn push_block(&mut self, span: Span) -> BlockId {
        let id = BlockId(id_index(self.blocks.len()));
        self.blocks.push(Block {
            stmts: Vec::new(),
            tail: None,
            span,
        });
        id
    }

    /// Adds a pattern and returns its ID.
    pub fn push_pattern(&mut self, pattern: Pattern) -> PatternId {
        let id = PatternId(id_index(self.patterns.len()));
        self.patterns.push(pattern);
        id
    }

    /// Builds async state-machine metadata from this body's await expressions.
    pub fn build_async_state_machine(&mut self) {
        let suspensions = self
            .exprs
            .iter()
            .enumerate()
            .filter_map(|(idx, expr)| {
                if let ExprKind::Await(future) = expr.kind {
                    Some(AsyncSuspensionPoint {
                        await_expr: ExprId(id_index(idx)),
                        future,
                        resume_state: AsyncStateId(id_index(idx + 1)),
                        span: expr.span,
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let state_count = suspensions.len() + 1;
        self.async_state_machine = Some(AsyncStateMachine {
            states: (0..state_count)
                .map(|idx| AsyncState {
                    id: AsyncStateId(id_index(idx)),
                })
                .collect(),
            suspensions,
        });
    }
}

/// Identifier for a state in an async body state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AsyncStateId(pub u32);

/// Explicit state-machine metadata for an async body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsyncStateMachine {
    /// States in execution order.
    pub states: Vec<AsyncState>,
    /// Suspension points that transition between states.
    pub suspensions: Vec<AsyncSuspensionPoint>,
}

/// One state in an async body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsyncState {
    /// State identifier.
    pub id: AsyncStateId,
}

/// One await-derived suspension point in an async body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsyncSuspensionPoint {
    /// Await expression that causes suspension.
    pub await_expr: ExprId,
    /// Future expression being awaited.
    pub future: ExprId,
    /// State resumed after this await completes.
    pub resume_state: AsyncStateId,
    /// Source location of the await.
    pub span: Span,
}

/// A local variable declaration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalDecl {
    /// The name of the local variable.
    pub name: Option<Symbol>,
    /// The type of the local variable.
    pub ty: TypeId,
    /// Whether this local is mutable.
    pub mutable: bool,
    /// Source location of the declaration.
    pub span: Span,
}

/// A block of statements with an optional tail expression.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// IDs of statements in this block.
    pub stmts: Vec<StmtId>,
    /// Optional tail expression of the block.
    pub tail: Option<ExprId>,
    /// Source location of the block.
    pub span: Span,
}

/// A statement in the HIR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Stmt {
    /// Let binding with a pattern, type, and optional value.
    Let {
        /// The pattern being bound.
        pat: PatternId,
        /// The type of the binding.
        ty: TypeId,
        /// Optional initializer expression.
        value: Option<ExprId>,
    },
    /// Variable assignment.
    Assign {
        /// The target expression.
        target: ExprId,
        /// The value expression.
        value: ExprId,
    },
    /// Expression statement.
    Expr(ExprId),
    /// Return statement with optional value.
    Return(Option<ExprId>),
    /// If statement with condition and optional else block.
    If {
        /// The condition expression.
        cond: ExprId,
        /// The then block.
        then_block: BlockId,
        /// Optional else block.
        else_block: Option<BlockId>,
    },
    /// While loop with condition and body.
    While {
        /// The loop condition.
        cond: ExprId,
        /// The loop body.
        body: BlockId,
    },
    /// While loop with an update assignment that runs before each next test.
    WhileUpdate {
        /// The loop condition.
        cond: ExprId,
        /// The loop body.
        body: BlockId,
        /// Assignment target updated after each iteration.
        update_target: ExprId,
        /// Assignment value evaluated after each iteration.
        update_value: ExprId,
    },
    /// For-in loop with pattern, iterator, and body.
    For {
        /// The pattern binding the iterator value.
        pat: PatternId,
        /// The iterable expression.
        iter: ExprId,
        /// The loop body.
        body: BlockId,
    },
    /// Match statement with scrutinee, arms, and optional default.
    Match {
        /// The expression being matched.
        scrutinee: ExprId,
        /// The match arms.
        arms: Vec<MatchArm>,
        /// Optional default block.
        default: Option<BlockId>,
    },
    /// Throw statement.
    Throw(ExprId),
    /// Try/catch/finally exception control flow.
    TryCatch {
        /// The protected block.
        body: BlockId,
        /// Optional local bound by the catch clause.
        catch_binding: Option<LocalId>,
        /// Optional catch block.
        catch_body: Option<BlockId>,
        /// Optional finally block.
        finally_body: Option<BlockId>,
    },
    /// Break statement.
    Break,
    /// Continue statement.
    Continue,
}

/// An arm in a match statement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchArm {
    /// The literal pattern for this arm.
    pub label: Literal,
    /// The body block for this arm.
    pub body: BlockId,
}

/// A pattern in a binding or match.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Pattern {
    /// A wildcard pattern that matches anything.
    Wildcard,
    /// A binding pattern that binds to a local variable.
    Binding(LocalId),
    /// A tuple pattern.
    Tuple(Vec<PatternId>),
    /// A literal pattern.
    Literal(Literal),
}
