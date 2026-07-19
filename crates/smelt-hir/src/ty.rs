use serde::{Deserialize, Serialize};

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
        if let Some((idx, _)) = self
            .types
            .iter()
            .enumerate()
            .find(|(_, existing)| **existing == ty)
        {
            return TypeId(id_index(idx));
        }
        let id = TypeId(id_index(self.types.len()));
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
}
