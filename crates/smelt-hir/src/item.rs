use serde::{Deserialize, Serialize};

use crate::ids::{BodyId, ExprId, ItemId, LocalId, Span, Symbol, TypeId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Item {
    Function(Function),
    Class(Class),
    Interface(Interface),
    TypeAlias(TypeAlias),
    Const(ConstItem),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    Public,
    Private,
    Protected,
}

/// Distinguishes how a class gets its shape.
///
/// Used by the Python frontend (and by codegen) to pick the right derived
/// traits and constructor strategy.  TypeScript classes always produce
/// `ClassKind::Plain`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClassKind {
    /// A plain class with an explicit constructor (`__init__` / `constructor`).
    Plain,
    /// A `@dataclass`-annotated class (or any class whose metaclass implements
    /// PEP 681 `dataclass_transform`).  The frontend synthesizes an explicit
    /// `__init__` into HIR so MIR sees no difference from a plain class.
    /// `frozen` mirrors the `frozen=True` parameter.
    DataclassLike { frozen: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Function {
    pub name: Symbol,
    pub span: Span,
    pub params: Vec<Param>,
    pub return_ty: TypeId,
    pub is_async: bool,
    pub body: Option<BodyId>,
    pub owner: FunctionOwner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FunctionOwner {
    Module,
    ClassMethod { class: Symbol, method: Symbol },
    Constructor { class: Symbol },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Param {
    pub name: Symbol,
    pub local: LocalId,
    pub ty: TypeId,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Class {
    pub name: Symbol,
    pub span: Span,
    /// Whether this is a plain or dataclass-like class.
    pub kind: ClassKind,
    /// Single base class (Python `class C(B):` / TS `class C extends B`).
    /// Multiple inheritance is rejected by both frontends.
    pub base: Option<Symbol>,
    pub fields: Vec<Field>,
    pub constructor: Option<ItemId>,
    pub methods: Vec<ItemId>,
    pub implements: Vec<Symbol>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interface {
    pub name: Symbol,
    pub span: Span,
    pub fields: Vec<Field>,
    pub methods: Vec<MethodSig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Field {
    pub name: Symbol,
    pub ty: TypeId,
    pub visibility: Visibility,
    pub optional: bool,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodSig {
    pub name: Symbol,
    pub params: Vec<ParamSig>,
    pub return_ty: TypeId,
    pub visibility: Visibility,
    pub is_async: bool,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamSig {
    pub name: Symbol,
    pub ty: TypeId,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeAlias {
    pub name: Symbol,
    pub ty: TypeId,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstItem {
    pub name: Symbol,
    pub ty: TypeId,
    pub value: ExprId,
    pub body: BodyId,
    pub span: Span,
}
