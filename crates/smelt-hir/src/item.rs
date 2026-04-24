use serde::{Deserialize, Serialize};

use crate::ids::{BodyId, ExprId, ItemId, LocalId, Span, Symbol, TypeId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Item {
    Function(Function),
    Class(Class),
    TypeAlias(TypeAlias),
    Const(ConstItem),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Function {
    pub name: Symbol,
    pub span: Span,
    pub params: Vec<Param>,
    pub return_ty: TypeId,
    pub is_async: bool,
    pub body: Option<BodyId>,
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
    pub fields: Vec<Field>,
    pub methods: Vec<ItemId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Field {
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
