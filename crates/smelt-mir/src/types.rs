//! Core MIR type definitions and structures.
//!
//! This module defines the fundamental types and structures used in the MIR representation,
//! including functions, basic blocks, locals, and various statements and expressions.

use serde::{Deserialize, Serialize};
use smelt_hir::{BodyId, Span, Symbol, TypeId, Visibility};

/// Unique identifier for a function in MIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FuncId(pub u32);

/// Unique identifier for a basic block in MIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockId(pub u32);

/// Unique identifier for a local variable in MIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LocalId(pub u32);

/// The MIR representation of a crate, containing all functions, classes, and interfaces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mir {
    /// All functions in the crate.
    pub functions: Vec<MirFunction>,
    /// All classes in the crate.
    pub classes: Vec<MirClass>,
    /// All interfaces in the crate.
    pub interfaces: Vec<MirInterface>,
    /// Type interner for interned types.
    pub types: smelt_hir::TypeInterner,
    /// Symbol interner for interned identifiers.
    pub symbols: smelt_hir::SymbolInterner,
}

impl Mir {
    /// Creates a new empty MIR crate with the given type and symbol interners.
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

    /// Returns the next available function ID.
    #[must_use]
    pub fn next_function_id(&self) -> FuncId {
        FuncId(self.functions.len() as u32)
    }

    /// Adds a function to the MIR and returns its ID.
    pub fn push_function(&mut self, function: MirFunction) -> FuncId {
        let id = FuncId(self.functions.len() as u32);
        debug_assert_eq!(function.id, id);
        self.functions.push(function);
        id
    }
}

/// MIR representation of a class definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirClass {
    /// Name of the class.
    pub name: Symbol,
    /// Propagated from HIR for codegen (e.g. emit `#[derive(PartialEq, Eq)]`
    /// for frozen dataclasses).
    pub kind: smelt_hir::ClassKind,
    /// Single base class, if any (multiple inheritance is rejected upstream).
    pub base: Option<Symbol>,
    /// Fields defined in the class.
    pub fields: Vec<MirField>,
    /// Constructor function ID, if any.
    pub constructor: Option<FuncId>,
    /// Method function IDs.
    pub methods: Vec<FuncId>,
    /// Interfaces this class implements.
    pub implements: Vec<Symbol>,
}

/// MIR representation of an interface definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirInterface {
    /// Name of the interface.
    pub name: Symbol,
    /// Fields defined in the interface.
    pub fields: Vec<MirField>,
    /// Method signatures in the interface.
    pub methods: Vec<smelt_hir::MethodSig>,
}

/// MIR representation of a field in a class or interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirField {
    /// Name of the field.
    pub name: Symbol,
    /// Type of the field.
    pub ty: TypeId,
    /// Visibility of the field.
    pub visibility: Visibility,
}

/// MIR representation of a function with basic blocks and locals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirFunction {
    /// Unique identifier of this function.
    pub id: FuncId,
    /// Name of the function.
    pub name: Symbol,
    /// Origin information (from HIR).
    pub origin: HirOrigin,
    /// Whether this is an async function.
    pub is_async: bool,
    /// Parameter local IDs.
    pub params: Vec<LocalId>,
    /// Return type of the function.
    pub return_ty: TypeId,
    /// All local variables in the function.
    pub locals: Vec<LocalDecl>,
    /// All basic blocks in the function.
    pub blocks: Vec<BasicBlock>,
    /// Entry block ID.
    pub entry: BlockId,
}

impl MirFunction {
    /// Creates a new MIR function with an empty entry block.
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

    /// Adds a local variable to the function and returns its ID.
    pub(crate) fn push_local(&mut self, local: LocalDecl) -> LocalId {
        let id = LocalId(self.locals.len() as u32);
        self.locals.push(local);
        id
    }

    /// Adds a basic block to the function and returns its ID.
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

/// Information about the origin of a MIR function in the HIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirOrigin {
    /// A module-level function body.
    Body(BodyId),
    /// A class constructor.
    ClassConstructor {
        /// The class being constructed.
        class: Symbol,
        /// The constructor body.
        body: BodyId,
    },
    /// A class method.
    ClassMethod {
        /// The class containing the method.
        class: Symbol,
        /// The method name.
        method: Symbol,
        /// The method body.
        body: BodyId,
    },
}

/// Declaration of a local variable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalDecl {
    /// Type of the local.
    pub ty: TypeId,
    /// Kind of the local (parameter, temporary, or user binding).
    pub kind: LocalKind,
    /// Source span of the local.
    pub span: Span,
}

/// The kind of a local variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocalKind {
    /// A function parameter.
    Param,
    /// A compiler-generated temporary.
    Temp,
    /// A user-defined binding (variable).
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
