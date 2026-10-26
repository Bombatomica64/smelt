//! Smelt-owned model of TypeScript 7 types.
//!
//! The tsrs checker's `Type` graph lives in a per-checker arena and is only
//! valid on the thread that built it, so nothing from tsrs crosses into this
//! model. Instead every type the frontend can ask about is copied
//! into a [`TsTypeTable`] of plain owned values. Types reference each other by
//! [`TsTypeId`], which makes recursive types (`interface Node { next: Node }`)
//! finite and lets equal tsrs types share one id.
//!
//! The model is structural where TypeScript is structural (anonymous object
//! types, unions, tuples) and nominal where Rust lowering needs identity
//! (classes, interfaces, enums: [`TsTypeKind::Named`] carries the declaring
//! symbol). Types that still depend on an uninstantiated type parameter, such
//! as a conditional type inside a generic body, stay [`TsTypeKind::Deferred`]
//! with the checker's own spelling, so callers can see exactly what TypeScript
//! could not resolve rather than a guessed approximation.

#![expect(
    clippy::exhaustive_structs,
    clippy::exhaustive_enums,
    reason = "plain data built by the checker bridge (another crate) and read by the frontend; classifiers must break on a new TypeScript type kind rather than fall into a wildcard"
)]

/// Index of a type in a [`TsTypeTable`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct TsTypeId(u32);

impl TsTypeId {
    /// Returns the table index of this id.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

/// One resolved TypeScript type.
#[derive(Clone, PartialEq, Debug)]
pub struct TsType {
    /// The structure of the type.
    pub kind: TsTypeKind,
    /// The checker's own spelling (`typeToString`), as `tsc` prints it in hovers and baselines.
    pub display: String,
    /// The type alias this type was written through (`HandlerResponse<any>`), if any.
    ///
    /// A consumer that re-spells the type in source form uses the alias name
    /// to stay identical to what the source would have written.
    pub alias: Option<TsAlias>,
}

/// A type alias reference: the alias symbol and its type arguments.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TsAlias {
    /// The declaring `type` alias.
    pub symbol: TsSymbolRef,
    /// Type arguments the alias was instantiated with.
    pub type_arguments: Vec<TsTypeId>,
}

/// Structure of a [`TsType`].
#[derive(Clone, PartialEq, Debug)]
pub enum TsTypeKind {
    /// `any`.
    Any,
    /// `unknown`.
    Unknown,
    /// `never`.
    Never,
    /// `void`.
    Void,
    /// `undefined`.
    Undefined,
    /// `null`.
    Null,
    /// `string`.
    String,
    /// `number`.
    Number,
    /// `boolean` (the checker's `true | false` union).
    Boolean,
    /// `bigint`.
    BigInt,
    /// `symbol`.
    Symbol,
    /// The non-primitive `object` type.
    NonPrimitive,
    /// A string literal type such as `"yes"`.
    StringLiteral(String),
    /// A number literal type such as `0`.
    NumberLiteral(f64),
    /// `true` or `false`.
    BooleanLiteral(bool),
    /// A bigint literal type, in source spelling without the `n` suffix.
    BigIntLiteral(String),
    /// A `unique symbol` type.
    UniqueSymbol,
    /// A member of an enum; `value` is its literal type.
    EnumMember {
        /// The enum declaring this member.
        enum_symbol: TsSymbolRef,
        /// The member's literal value type.
        value: TsTypeId,
    },
    /// A union; for an enum type `enum_symbol` names the enum.
    Union {
        /// Union members, in checker order.
        members: Vec<TsTypeId>,
        /// The enum this union is the type of, if any.
        enum_symbol: Option<TsSymbolRef>,
    },
    /// An intersection.
    Intersection(Vec<TsTypeId>),
    /// A type parameter that is still free at this location.
    TypeParameter {
        /// Source name of the parameter (for the polymorphic `this` type, the class name).
        name: String,
        /// The declared constraint, if any (for `this`, the enclosing class or interface).
        constraint: Option<TsTypeId>,
        /// Whether this is TypeScript's polymorphic `this` type.
        is_this: bool,
    },
    /// `T[]` / `Array<T>`, or `readonly T[]` when `readonly` is set.
    Array {
        /// Element type.
        element: TsTypeId,
        /// Whether this is `ReadonlyArray<T>`.
        readonly: bool,
    },
    /// A tuple type.
    Tuple {
        /// Elements in order.
        elements: Vec<TsTupleElement>,
        /// Whether this is a `readonly [...]` tuple.
        readonly: bool,
    },
    /// A class or interface type, with its type arguments (the implicit
    /// `this` argument is not included).
    Named {
        /// The declaring class or interface.
        symbol: TsSymbolRef,
        /// Instantiated type arguments, one per declared type parameter.
        type_arguments: Vec<TsTypeId>,
    },
    /// An anonymous, mapped or object-literal type, expanded to its resolved members.
    Object(TsObjectType),
    /// A type the checker keeps unevaluated because it still depends on a free type parameter.
    Deferred(TsDeferredKind),
    /// A structural type nested deeper than the expansion limit; only `display` describes it.
    Truncated,
}

/// Element of a [`TsTypeKind::Tuple`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TsTupleElement {
    /// Element type (for rest elements, the element type of the rest array).
    pub ty: TsTypeId,
    /// How the element participates in the tuple's length.
    pub kind: TsTupleElementKind,
}

/// Arity role of a tuple element.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TsTupleElementKind {
    /// `T`.
    Required,
    /// `T?`.
    Optional,
    /// `...T[]`.
    Rest,
    /// `...T` where `T` is itself a tuple-like type parameter.
    Variadic,
}

/// Members of a structural object type.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct TsObjectType {
    /// The symbol behind the type (`typeof Foo`, a function, an object literal), if any.
    pub symbol: Option<TsSymbolRef>,
    /// Properties in declaration order.
    pub properties: Vec<TsProperty>,
    /// Call signatures.
    pub call_signatures: Vec<TsSignature>,
    /// Construct signatures.
    pub construct_signatures: Vec<TsSignature>,
    /// Index signatures.
    pub index_signatures: Vec<TsIndexSignature>,
}

/// A property of an object type.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TsProperty {
    /// Property name.
    pub name: String,
    /// Property type (without the `undefined` that optionality adds).
    pub ty: TsTypeId,
    /// Whether the property is optional (`a?: T`).
    pub optional: bool,
    /// Whether the property is readonly.
    pub readonly: bool,
}

/// A call or construct signature.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TsSignature {
    /// Free type parameters of the signature.
    pub type_parameters: Vec<TsTypeId>,
    /// Parameters in order; a rest parameter is last.
    pub parameters: Vec<TsParameter>,
    /// Number of leading parameters a caller must pass.
    pub min_argument_count: usize,
    /// Whether the last parameter is a rest parameter.
    pub has_rest: bool,
    /// Return type.
    pub return_type: TsTypeId,
}

/// A signature parameter.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TsParameter {
    /// Parameter name.
    pub name: String,
    /// Parameter type; for a rest parameter, the array type.
    pub ty: TsTypeId,
}

/// An index signature such as `[key: string]: T`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TsIndexSignature {
    /// Key type (`string`, `number`, `symbol` or a template literal).
    pub key: TsTypeId,
    /// Value type.
    pub value: TsTypeId,
    /// Whether the signature is readonly.
    pub readonly: bool,
}

/// An unevaluated generic type operator.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TsDeferredKind {
    /// `T extends U ? X : Y`.
    Conditional,
    /// `T[K]`.
    IndexedAccess,
    /// `keyof T`.
    Index,
    /// A template literal type with a free type parameter.
    TemplateLiteral,
    /// `Uppercase<T>` and the other intrinsic string mappings.
    StringMapping,
    /// A substitution type the checker introduces for constrained type variables.
    Substitution,
}

/// Identity of a declared symbol.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct TsSymbolRef {
    /// Local name (`Map`, `Point`).
    pub name: String,
    /// Fully qualified name as the checker spells it (`"src/geo".Point`).
    pub qualified_name: String,
    /// Where the first declaration lives; `None` for checker-synthesized symbols.
    pub declaration: Option<TsLocation>,
}

/// A source range in one of the analysed files or a bundled lib file.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct TsLocation {
    /// File path as the program knows it.
    pub file: String,
    /// Byte offset of the first token.
    pub start: u32,
    /// Byte offset one past the end.
    pub end: u32,
}

/// Arena of resolved types.
#[derive(Clone, Debug, Default)]
pub struct TsTypeTable {
    /// Types indexed by [`TsTypeId`].
    types: Vec<TsType>,
}

impl TsTypeTable {
    /// Returns the type stored under `id`, or `None` for an id from another table.
    #[must_use]
    pub fn get(&self, id: TsTypeId) -> Option<&TsType> {
        self.types.get(usize::try_from(id.0).ok()?)
    }

    /// Number of stored types.
    #[must_use]
    pub fn len(&self) -> usize {
        self.types.len()
    }

    /// Whether the table is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }

    /// Reserves a slot for a type whose structure is filled in later, so that
    /// recursive references can point at it while it is being converted.
    ///
    /// Builder API for the checker bridge (`smelt-ts-types`); consumers only read.
    pub fn reserve(&mut self) -> TsTypeId {
        let id = TsTypeId(u32::try_from(self.types.len()).unwrap_or(u32::MAX));
        self.types.push(TsType { kind: TsTypeKind::Truncated, display: String::new(), alias: None });
        id
    }

    /// Fills a slot handed out by [`Self::reserve`].
    pub fn fill(&mut self, id: TsTypeId, ty: TsType) {
        if let Some(slot) = usize::try_from(id.0).ok().and_then(|index| self.types.get_mut(index)) {
            *slot = ty;
        }
    }
}
