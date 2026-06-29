//! Items (functions, classes, interfaces, type aliases, and constants).

use serde::{Deserialize, Serialize};

use crate::{
    expr::Literal,
    ids::{BodyId, ExprId, ItemId, LocalId, Span, Symbol, TypeId},
};

/// A top-level item in the HIR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Item {
    /// A function item.
    Function(Function),
    /// A class item.
    Class(Class),
    /// An interface item.
    Interface(Interface),
    /// A type alias item.
    TypeAlias(TypeAlias),
    /// A constant item.
    Const(ConstItem),
}

/// Visibility modifier for items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    /// Public visibility.
    Public,
    /// Private visibility.
    Private,
    /// Protected visibility.
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
    /// An abstract class. It may contribute fields and method signatures, but
    /// cannot be constructed directly.
    Abstract,
    /// A `@dataclass`-annotated class (or any class whose metaclass implements
    /// PEP 681 `dataclass_transform`).  The frontend synthesizes an explicit
    /// `__init__` into HIR so MIR sees no difference from a plain class.
    /// `frozen` mirrors the `frozen=True` parameter.
    DataclassLike {
        /// Whether the dataclass is frozen.
        frozen: bool,
    },
}

/// A function item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Function {
    /// The name of the function.
    pub name: Symbol,
    /// Source location of the function declaration.
    pub span: Span,
    /// Parameters of the function.
    pub params: Vec<Param>,
    /// Index of the rest parameter, if this function declares one.
    pub rest: Option<usize>,
    /// Number of leading parameters counted by JavaScript `Function.length`.
    pub required_params: Option<usize>,
    /// Return type of the function.
    pub return_ty: TypeId,
    /// Whether this function is async.
    pub is_async: bool,
    /// Whether this function should be emitted as a native Rust test.
    pub is_test: bool,
    /// Optional body of the function.
    pub body: Option<BodyId>,
    /// The owner of this function (module or class).
    pub owner: FunctionOwner,
}

/// The owner context of a function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FunctionOwner {
    /// Function is a module-level function.
    Module,
    /// Function is a method of a class.
    ClassMethod {
        /// Owning class.
        class: Symbol,
        /// Method name.
        method: Symbol,
    },
    /// Function is a constructor of a class.
    Constructor {
        /// Owning class.
        class: Symbol,
    },
}

/// A parameter of a function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Param {
    /// The name of the parameter.
    pub name: Symbol,
    /// The local variable ID for this parameter.
    pub local: LocalId,
    /// The type of the parameter.
    pub ty: TypeId,
    /// Source location of the parameter.
    pub span: Span,
}

/// A class item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Class {
    /// The name of the class.
    pub name: Symbol,
    /// Source location of the class declaration.
    pub span: Span,
    /// Whether this is a plain or dataclass-like class.
    pub kind: ClassKind,
    /// Generic type parameters declared by the class.
    pub type_params: Vec<TypeParamDef>,
    /// Single base class (Python `class C(B):` / TS `class C extends B`).
    /// Multiple inheritance is rejected by both frontends.
    pub base: Option<Symbol>,
    /// Type arguments applied to the base class.
    pub base_args: Vec<TypeId>,
    /// Fields of the class.
    pub fields: Vec<Field>,
    /// Materialized descriptor-backed members.
    pub descriptors: Vec<Descriptor>,
    /// Optional constructor method ID.
    pub constructor: Option<ItemId>,
    /// Method IDs of the class.
    pub methods: Vec<ItemId>,
    /// Abstract method signatures required by this class.
    pub abstract_methods: Vec<MethodSig>,
    /// Interfaces implemented by this class.
    pub implements: Vec<Symbol>,
}

/// A typed materialized descriptor member.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Descriptor {
    /// Bound class member name.
    pub name: Symbol,
    /// Concrete type produced by descriptor reads.
    pub read_ty: TypeId,
    /// Concrete type accepted by writes, when this is a data descriptor.
    pub write_ty: Option<TypeId>,
    /// Source-defined getter function item.
    pub getter: Option<ItemId>,
    /// Source-defined setter function item.
    pub setter: Option<ItemId>,
    /// Whether data-descriptor precedence applies.
    pub data_descriptor: bool,
    /// Concrete descriptor instance fields used to construct static state.
    pub value_fields: Vec<DescriptorValueField>,
}

/// One concrete primitive field on a materialized descriptor instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescriptorValueField {
    /// Descriptor object field name.
    pub name: Symbol,
    /// Concrete primitive value.
    pub value: Literal,
    /// Concrete value type.
    pub ty: TypeId,
}

/// An interface item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interface {
    /// The name of the interface.
    pub name: Symbol,
    /// Source location of the interface declaration.
    pub span: Span,
    /// Generic type parameters declared by the interface.
    pub type_params: Vec<TypeParamDef>,
    /// Interfaces extended by this interface.
    pub extends: Vec<InterfaceHeritage>,
    /// Fields of the interface.
    pub fields: Vec<Field>,
    /// Method signatures of the interface.
    pub methods: Vec<MethodSig>,
}

/// A lowered `extends Parent<Args...>` edge on an interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceHeritage {
    /// Parent interface symbol.
    pub parent: Symbol,
    /// Type arguments supplied to the parent interface.
    pub args: Vec<TypeId>,
}

/// A generic type parameter declaration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeParamDef {
    /// The type parameter name.
    pub name: Symbol,
    /// Optional upper bound or structural constraint.
    pub constraint: Option<TypeId>,
    /// Optional default type argument.
    pub default: Option<TypeId>,
    /// Source location of the type parameter.
    pub span: Span,
}

/// A field in a class or interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Field {
    /// The name of the field.
    pub name: Symbol,
    /// The type of the field.
    pub ty: TypeId,
    /// The visibility of the field.
    pub visibility: Visibility,
    /// Whether this field is optional.
    pub optional: bool,
    /// Source location of the field.
    pub span: Span,
}

/// A method signature in an interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodSig {
    /// The name of the method.
    pub name: Symbol,
    /// Parameters of the method.
    pub params: Vec<ParamSig>,
    /// Index of the rest parameter, if this method declares one.
    pub rest: Option<usize>,
    /// Number of leading parameters counted by JavaScript `Function.length`.
    pub required_params: Option<usize>,
    /// Return type of the method.
    pub return_ty: TypeId,
    /// The visibility of the method.
    pub visibility: Visibility,
    /// Whether this method is async.
    pub is_async: bool,
    /// Source location of the method signature.
    pub span: Span,
}

/// A parameter signature in an interface method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamSig {
    /// The name of the parameter.
    pub name: Symbol,
    /// The type of the parameter.
    pub ty: TypeId,
    /// Source location of the parameter.
    pub span: Span,
}

/// A type alias item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeAlias {
    /// The name of the type alias.
    pub name: Symbol,
    /// Generic type parameters declared by the alias.
    pub type_params: Vec<TypeParamDef>,
    /// The aliased type.
    pub ty: TypeId,
    /// Source location of the type alias declaration.
    pub span: Span,
}

/// A constant item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstItem {
    /// The name of the constant.
    pub name: Symbol,
    /// The type of the constant.
    pub ty: TypeId,
    /// The value expression ID.
    pub value: ExprId,
    /// The body containing the constant's value.
    pub body: BodyId,
    /// Source location of the constant declaration.
    pub span: Span,
}
