//! HIR types: the `Type` enum, `FunctionType`, and the `TypeInterner` that
//! hands out `TypeId`s for them.

use serde::{Deserialize, Serialize};

use crate::UnknownKind;
use crate::ids::{Symbol, TypeId, id_index};

/// A HIR type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Type {
    /// Boolean type.
    Bool,
    /// Integer type.
    Int,
    /// Floating-point type.
    Float,
    /// A JavaScript `number` proven to be an integer of a fixed Rust width.
    ///
    /// Every value of this type is exactly representable both as the Rust
    /// integer of [`IntWidth`] and as an `f64`, so converting it to a `number`
    /// context is the exact `as f64`, and printing it matches JavaScript's
    /// `Number::toString` for that value. Two producers create it: MIR range
    /// analysis (`smelt_mir::opt::IntegerRange`), which retypes a local whose
    /// every definition it proved integral and bounded, and the TypeScript
    /// frontend for Bun's `import type { int32 } from "bun"` annotation, whose
    /// every store the frontend proves. Python's integers stay [`Type::Int`].
    FixedInt(IntWidth),
    /// A JavaScript `bigint`: an arbitrary-precision signed integer.
    ///
    /// Separate from [`Type::Int`] on purpose. `Int` is the bounded integer
    /// type (Python's `int`, list indices, timestamps) and lowers to `i64`; a
    /// `bigint` is unbounded in JavaScript, so it lowers to Rust's
    /// `num_bigint::BigInt` and every operation on it keeps JavaScript's
    /// bigint semantics (truncating division, flooring `>>`, infinite
    /// two's-complement bitwise operators) without ever passing through `f64`.
    /// Its runtime kind is [`UnknownKind::BigInt`] (`typeof` answers
    /// `"bigint"`), never `"number"`.
    BigInt,
    /// String type.
    String,
    /// TypeScript `unknown`, carried as an opaque safe boundary type.
    Unknown,
    /// TypeScript `never`, the bottom type with no runtime values.
    Never,
    /// Type parameter in a generic declaration.
    TypeParam {
        /// The type parameter name.
        name: Symbol,
    },
    /// `None`/unit type.
    None,
    /// List type.
    List(TypeId),
    /// Set type.
    Set(TypeId),
    /// Dictionary type.
    Dict(TypeId, TypeId),
    /// A JavaScript `Map`, spelled `Map<K, V>` in source.
    ///
    /// Structurally identical to [`Type::Dict`] and lowered the same way for
    /// every operation (get/set/has/entries/iteration): a `Map` and a plain
    /// object/`Record` share the internal `Dict` machinery per the frontend
    /// validation boundary. The variant exists solely to preserve the source
    /// `Map` *spelling* through interning — `Dict(String, V)` and a
    /// string-keyed `Map<String, V>` would otherwise intern to the same
    /// `TypeId` and become indistinguishable. Codegen reads this variant at the
    /// one place spelling matters: erasure to `SmeltUnknown`, where a `Map`
    /// must emit a `__smelt_map` marker object (so `isMap`/`isEqualWith` see a
    /// Map) while a `Record` stays an unmarked object. It also routes the
    /// backing container to `SmeltJsMap` so the marker round-trips.
    JsMap(TypeId, TypeId),
    /// Tuple type.
    Tuple(Vec<TypeId>),
    /// Optional type.
    Optional(TypeId),
    /// Union type.
    Union(Vec<TypeId>),
    /// User-defined class type.
    Class {
        /// The class name.
        name: Symbol,
        /// Type arguments.
        args: Vec<TypeId>,
    },
    /// Function type.
    Function(FunctionType),
    /// Future/promise type.
    Future(TypeId),
    /// Resumable generator with yielded, returned, and resumed values.
    Generator {
        /// Whether `.next()` resumes asynchronously.
        is_async: bool,
        /// Value produced at each suspension point.
        yield_ty: TypeId,
        /// Value produced when the generator completes.
        return_ty: TypeId,
        /// Value supplied by the caller when resuming the generator.
        next_ty: TypeId,
    },
    /// Result of one synchronous generator resume operation.
    GeneratorResult {
        /// Value type when the generator suspended.
        yield_ty: TypeId,
        /// Value type when the generator completed.
        return_ty: TypeId,
    },
}

impl Type {
    /// The runtime kind every value of this type has, when the type alone
    /// fixes it.
    ///
    /// The one map from a concrete type to its JavaScript runtime category. The
    /// TypeScript frontend's `typeof` folding and narrowing and Rust codegen's
    /// static tag checks and union-arm routing all read it, so a `Map` or a
    /// generator is an object on every path. (Separate copies had drifted: two
    /// of them left `Map` out of `"object"`, so `typeof v === "object"` on a
    /// `Map<string, number> | string` folded to `false`.)
    ///
    /// Each concrete type answers with the kind its erased value carries:
    /// collections, records, class instances and generators are objects, a
    /// list or tuple is an array, a future is a promise. `None` for a type that
    /// does not fix one: `unknown`, `never`, a type parameter, a union, an
    /// optional, and `None` (which is `null` or `undefined`). Callers decide
    /// those themselves, because the answer depends on the question.
    #[must_use]
    pub const fn runtime_kind(&self) -> Option<UnknownKind> {
        match self {
            Self::Bool => Some(UnknownKind::Bool),
            Self::Int | Self::Float | Self::FixedInt(_) => Some(UnknownKind::Number),
            Self::BigInt => Some(UnknownKind::BigInt),
            Self::String => Some(UnknownKind::String),
            Self::Function(_) => Some(UnknownKind::Function),
            Self::List(_) | Self::Tuple(_) => Some(UnknownKind::Array),
            Self::Dict(_, _)
            | Self::JsMap(_, _)
            | Self::Set(_)
            | Self::Class { .. }
            | Self::Generator { .. }
            | Self::GeneratorResult { .. } => Some(UnknownKind::Object),
            Self::Future(_) => Some(UnknownKind::Promise),
            Self::Unknown
            | Self::Never
            | Self::TypeParam { .. }
            | Self::None
            | Self::Optional(_)
            | Self::Union(_) => None,
        }
    }
}

/// The Rust width of a [`Type::FixedInt`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IntWidth {
    /// Rust `i32`: Bun's `int32`, a whole number from -2^31 to 2^31 - 1.
    I32,
    /// Rust `i64`: a counter proven integral and bounded that can be negative.
    I64,
    /// Rust `usize`: a counter or index proven never negative.
    Usize,
}

impl IntWidth {
    /// The Rust primitive type name for this width.
    #[must_use]
    pub const fn rust_name(self) -> &'static str {
        match self {
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::Usize => "usize",
        }
    }
}

/// A function type signature.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FunctionType {
    /// Parameter types in order.
    pub params: Vec<TypeId>,
    /// Index of the TypeScript rest parameter, when this signature has one.
    ///
    /// The rest parameter's type still appears in `params`; this flag records
    /// source call semantics so array parameters are not confused with `...args`.
    pub rest: Option<usize>,
    /// Number of leading parameters counted by JavaScript `Function.length`.
    ///
    /// This differs from `params.len()` when later parameters have defaults or
    /// when a rest parameter is present. `None` means the frontend does not know
    /// the source-level arity and consumers should fall back to `params.len()`.
    pub required_params: Option<usize>,
    /// Parameter indexes that use Rust mutable-reference ABI.
    ///
    /// This is a backend ABI detail for source object identity. It is empty for
    /// ordinary function values, and populated when codegen adapts a concrete
    /// mutating method into a structural function field.
    #[serde(default)]
    pub mutable_params: Vec<usize>,
    /// Return type.
    pub return_ty: TypeId,
    /// Whether the function is async.
    pub is_async: bool,
    /// Whether calls can leave through a source-language throw.
    pub may_throw: bool,
}

/// Interns and deduplicates types.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TypeInterner {
    /// Interned types in insertion order.
    types: Vec<Type>,
    /// `types` inverted, so interning is a hash lookup rather than a scan.
    ///
    /// A derived cache, never serialized: a deserialized interner starts with
    /// it empty and [`Self::intern`] rebuilds it on first use. The linear scan
    /// it replaces made every `intern` O(types), which is quadratic over a
    /// program -- measured by `tests/compile_scaling.rs` in
    /// `smelt-codegen-rust` on deeply nested generic types. Because `intern`
    /// is the only writer of `types` and never stores a duplicate, the index
    /// answers exactly the id the scan found (the first, and only, equal type).
    #[serde(skip)]
    index: std::collections::HashMap<Type, TypeId>,
}

impl TypeInterner {
    /// Returns the `TypeId` for `ty`, inserting it if needed.
    pub fn intern(&mut self, mut ty: Type) -> TypeId {
        // Union identity must not depend on source spelling order. Besides
        // deduplicating equivalent types, canonical member order gives Rust
        // codegen one stable tagged-enum definition for `A | B` and `B | A`.
        if let Type::Union(items) = &mut ty {
            items.sort_unstable_by_key(|item| item.0);
            items.dedup();
        }
        if self.index.len() != self.types.len() {
            // First occurrence wins, as the scan did, should a deserialized
            // table ever carry a duplicate.
            let mut index = std::collections::HashMap::with_capacity(self.types.len());
            for (idx, existing) in self.types.iter().enumerate() {
                index.entry(existing.clone()).or_insert_with(|| TypeId(id_index(idx)));
            }
            self.index = index;
        }
        if let Some(id) = self.index.get(&ty) {
            return *id;
        }
        let id = TypeId(id_index(self.types.len()));
        self.index.insert(ty.clone(), id);
        self.types.push(ty);
        id
    }

    #[must_use]
    /// Looks up a type by ID.
    pub fn get(&self, id: TypeId) -> Option<&Type> {
        self.types.get(id.0 as usize)
    }

    #[must_use]
    /// Returns all interned types in insertion order.
    pub fn all(&self) -> &[Type] {
        &self.types
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_union_member_order() {
        let mut types = TypeInterner::default();
        let string = types.intern(Type::String);
        let float = types.intern(Type::Float);

        let first = types.intern(Type::Union(vec![string, float, string]));
        let second = types.intern(Type::Union(vec![float, string]));

        assert_eq!(first, second);
        assert_eq!(types.get(first), Some(&Type::Union(vec![string, float])));
    }

    /// The shared runtime-kind map: every object-shaped concrete type is an
    /// `"object"` (a `Map` and a generator included, which drifted copies left
    /// out), a list is an array, and the types that do not fix a kind answer
    /// `None` so each caller decides them.
    #[test]
    fn runtime_kind_classifies_concrete_types() {
        let mut types = TypeInterner::default();
        let string = types.intern(Type::String);
        let float = types.intern(Type::Float);

        let object = Some(UnknownKind::Object);
        assert_eq!(Type::JsMap(string, float).runtime_kind(), object);
        assert_eq!(Type::Dict(string, float).runtime_kind(), object);
        assert_eq!(Type::Set(float).runtime_kind(), object);
        assert_eq!(
            Type::Generator {
                is_async: false,
                yield_ty: float,
                return_ty: float,
                next_ty: float,
            }
            .runtime_kind(),
            object
        );
        assert_eq!(Type::List(float).runtime_kind(), Some(UnknownKind::Array));
        assert_eq!(Type::Future(float).runtime_kind(), Some(UnknownKind::Promise));
        assert_eq!(Type::Int.runtime_kind(), Some(UnknownKind::Number));
        assert_eq!(Type::BigInt.runtime_kind(), Some(UnknownKind::BigInt));
        assert_eq!(Type::Union(vec![string, float]).runtime_kind(), None);
        assert_eq!(Type::Optional(float).runtime_kind(), None);
        assert_eq!(Type::None.runtime_kind(), None);
        assert_eq!(Type::Unknown.runtime_kind(), None);
    }

    /// `typeof x === "object"` holds for arrays, promises and `null` as well as
    /// objects; every other test is exact.
    #[test]
    fn unknown_kind_satisfies_follows_typeof() {
        for kind in [
            UnknownKind::Object,
            UnknownKind::Array,
            UnknownKind::Promise,
            UnknownKind::Null,
        ] {
            assert!(kind.satisfies(UnknownKind::Object), "{kind:?}");
            assert_eq!(kind.typeof_name(), "object");
        }
        assert!(UnknownKind::Array.satisfies(UnknownKind::Array));
        assert!(!UnknownKind::Object.satisfies(UnknownKind::Array));
        assert!(!UnknownKind::Function.satisfies(UnknownKind::Object));
        assert!(!UnknownKind::Undefined.satisfies(UnknownKind::Object));
        assert_eq!(UnknownKind::Function.typeof_name(), "function");
        assert_eq!(UnknownKind::Undefined.typeof_name(), "undefined");
    }
}
