use serde::{Deserialize, Serialize};

use crate::ids::{Symbol, TypeId};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Type {
    Bool,
    Int,
    Float,
    String,
    None,
    List(TypeId),
    Set(TypeId),
    Dict(TypeId, TypeId),
    Tuple(Vec<TypeId>),
    Optional(TypeId),
    Union(Vec<TypeId>),
    Class { name: Symbol, args: Vec<TypeId> },
    Function(FunctionType),
    Future(TypeId),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FunctionType {
    pub params: Vec<TypeId>,
    pub return_ty: TypeId,
    pub is_async: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TypeInterner {
    types: Vec<Type>,
}

impl TypeInterner {
    pub fn intern(&mut self, ty: Type) -> TypeId {
        if let Some((idx, _)) = self
            .types
            .iter()
            .enumerate()
            .find(|(_, existing)| **existing == ty)
        {
            return TypeId(idx as u32);
        }
        let id = TypeId(self.types.len() as u32);
        self.types.push(ty);
        id
    }

    #[must_use]
    pub fn get(&self, id: TypeId) -> Option<&Type> {
        self.types.get(id.0 as usize)
    }

    #[must_use]
    pub fn all(&self) -> &[Type] {
        &self.types
    }
}
