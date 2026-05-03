use serde::{Deserialize, Serialize};
use smelt_hir::{BodyId, Span, Symbol, TypeId, Visibility};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FuncId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LocalId(pub u32);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mir {
    pub functions: Vec<MirFunction>,
    pub classes: Vec<MirClass>,
    pub interfaces: Vec<MirInterface>,
    pub types: smelt_hir::TypeInterner,
    pub symbols: smelt_hir::SymbolInterner,
}

impl Mir {
    #[must_use]
    pub fn new(types: smelt_hir::TypeInterner, symbols: smelt_hir::SymbolInterner) -> Self {
        Self {
            functions: Vec::new(),
            classes: Vec::new(),
            interfaces: Vec::new(),
            types,
            symbols,
        }
    }

    #[must_use]
    pub fn next_function_id(&self) -> FuncId {
        FuncId(self.functions.len() as u32)
    }

    pub fn push_function(&mut self, function: MirFunction) -> FuncId {
        let id = FuncId(self.functions.len() as u32);
        debug_assert_eq!(function.id, id);
        self.functions.push(function);
        id
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirClass {
    pub name: Symbol,
    /// Propagated from HIR for codegen (e.g. emit `#[derive(PartialEq, Eq)]`
    /// for frozen dataclasses).
    pub kind: smelt_hir::ClassKind,
    /// Single base class, if any (multiple inheritance is rejected upstream).
    pub base: Option<Symbol>,
    pub fields: Vec<MirField>,
    pub constructor: Option<FuncId>,
    pub methods: Vec<FuncId>,
    pub implements: Vec<Symbol>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirInterface {
    pub name: Symbol,
    pub fields: Vec<MirField>,
    pub methods: Vec<smelt_hir::MethodSig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirField {
    pub name: Symbol,
    pub ty: TypeId,
    pub visibility: Visibility,
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
    pub(crate) fn new(
        id: FuncId,
        name: Symbol,
        origin: HirOrigin,
        return_ty: TypeId,
        span: Span,
    ) -> Self {
        Self {
            id,
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

    pub(crate) fn push_local(&mut self, local: LocalDecl) -> LocalId {
        let id = LocalId(self.locals.len() as u32);
        self.locals.push(local);
        id
    }

    pub(crate) fn push_block(&mut self, span: Span) -> BlockId {
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
    ClassConstructor {
        class: Symbol,
        body: BodyId,
    },
    ClassMethod {
        class: Symbol,
        method: Symbol,
        body: BodyId,
    },
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
    Field { base: LocalId, field: Symbol },
    Index { base: LocalId, index: Box<Operand> },
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
    List(Vec<Operand>),
    Dict(Vec<(Operand, Operand)>),
    Tuple(Vec<Operand>),
    Binary {
        op: smelt_hir::BinOp,
        lhs: Operand,
        rhs: Operand,
    },
    Unary {
        op: smelt_hir::UnaryOp,
        operand: Operand,
    },
    Struct {
        class: Symbol,
        fields: Vec<(Symbol, Operand)>,
    },
    Len(Operand),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Statement {
    Assign { dest: LocalId, value: Rvalue },
    AssignPlace { place: Place, value: Rvalue },
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
    Switch {
        cond: Operand,
        then_block: BlockId,
        else_block: BlockId,
    },
    Match {
        scrutinee: Operand,
        arms: Vec<MatchArm>,
        default: Option<BlockId>,
    },
    Return(Operand),
    Unreachable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchArm {
    pub label: Constant,
    pub target: BlockId,
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
