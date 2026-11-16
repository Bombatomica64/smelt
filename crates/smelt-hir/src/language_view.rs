//! The view one source language has of a type from the other.
//!
//! A mixed crate stores text two ways: a JavaScript string is UTF-16
//! ([`Type::String`]) and a Python `str` is UTF-8 ([`Type::PyStr`]). A value
//! that crosses between the languages (an argument passed to a function the
//! other language defines, a result, a field or a constant read from it) has
//! one type on each side. [`view_in`] answers what that type is in a given
//! language, and [`crosses_into`] whether a type carries the other language's
//! text, which is exactly when a crossing needs a conversion. A frontend marks
//! each crossing with an `ExprKind::TypeAssert` to the view, and the Rust
//! backend's coercion converts the representation, element by element through
//! containers.
//!
//! Integers cross one way too: a Python `int` ([`Type::PyInt`], unbounded) is
//! a TypeScript `number` ([`Type::Float`]) on the TypeScript side.
//!
//! Neither frontend produces the other language's text type itself, so the
//! presence of [`Type::String`] in a Python expression's type (or
//! [`Type::PyStr`] in a TypeScript one) is the crossing: no module bookkeeping
//! is needed to find it, and no crossing can be missed by a rule that forgot
//! to ask.

use crate::{FunctionType, Type, TypeId, TypeInterner};

/// A source language of a mixed crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceLanguage {
    /// TypeScript, whose text is [`Type::String`].
    TypeScript,
    /// Python, whose text is [`Type::PyStr`].
    Python,
}

impl SourceLanguage {
    /// Each type of the other language this language has its own spelling
    /// for, paired with that spelling.
    ///
    /// Text crosses both ways. A Python `int` ([`Type::PyInt`], unbounded)
    /// is a `number` once TypeScript holds it, as it is when Node reads a
    /// Python integer; a TypeScript `number` is already a Python `float`, so
    /// nothing maps the other way.
    const fn foreign_types(self) -> &'static [(Type, Type)] {
        match self {
            Self::TypeScript => &[(Type::PyStr, Type::String), (Type::PyInt, Type::Float)],
            Self::Python => &[(Type::String, Type::PyStr)],
        }
    }

    /// This language's spelling of `found`, when it is a type of the other
    /// language.
    fn own_spelling(self, found: &Type) -> Option<Type> {
        self.foreign_types()
            .iter()
            .find(|(foreign, _)| foreign == found)
            .map(|(_, own)| own.clone())
    }
}

/// Whether `ty` carries a type of the other language anywhere, seen from
/// `language`.
#[must_use]
pub fn crosses_into(types: &TypeInterner, ty: TypeId, language: SourceLanguage) -> bool {
    language
        .foreign_types()
        .iter()
        .any(|(foreign, _)| contains(types, ty, foreign, 0))
}

/// `ty` as `language` sees it: every type of the other language replaced by
/// `language`'s own spelling of it, through every container, union and
/// signature.
pub fn view_in(types: &mut TypeInterner, ty: TypeId, language: SourceLanguage) -> TypeId {
    if !crosses_into(types, ty, language) {
        return ty;
    }
    map(types, ty, language, 0)
}

/// Recursion bound; a deeper type is left as it is.
const MAX_DEPTH: usize = 32;

/// Whether `ty` contains `needle`.
fn contains(types: &TypeInterner, ty: TypeId, needle: &Type, depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    let Some(found) = types.get(ty) else {
        return false;
    };
    if found == needle {
        return true;
    }
    children(found)
        .into_iter()
        .any(|child| contains(types, child, needle, depth + 1))
}

/// The types directly inside `ty`.
fn children(ty: &Type) -> Vec<TypeId> {
    match ty {
        Type::List(item) | Type::Set(item) | Type::Optional(item) | Type::Future(item) => {
            vec![*item]
        }
        Type::Dict(key, value) | Type::JsMap(key, value) => vec![*key, *value],
        Type::Tuple(items) | Type::Union(items) => items.clone(),
        Type::Class { args, .. } => args.clone(),
        Type::Function(function) => function
            .params
            .iter()
            .copied()
            .chain([function.return_ty])
            .collect(),
        Type::Generator {
            yield_ty,
            return_ty,
            next_ty,
            ..
        } => vec![*yield_ty, *return_ty, *next_ty],
        Type::GeneratorResult {
            yield_ty,
            return_ty,
        } => vec![*yield_ty, *return_ty],
        Type::IndexedAccess { object, index } => vec![*object, *index],
        Type::Bool
        | Type::Int
        | Type::PyInt
        | Type::Float
        | Type::FixedInt(_)
        | Type::BigInt
        | Type::String
        | Type::PyStr
        | Type::Symbol
        | Type::PropKey
        | Type::Unknown
        | Type::Never
        | Type::TypeParam { .. }
        | Type::None => Vec::new(),
    }
}

/// [`view_in`] of a type known to cross.
fn map(types: &mut TypeInterner, ty: TypeId, language: SourceLanguage, depth: usize) -> TypeId {
    if depth > MAX_DEPTH {
        return ty;
    }
    let Some(found) = types.get(ty).cloned() else {
        return ty;
    };
    if let Some(own) = language.own_spelling(&found) {
        return types.intern(own);
    }
    let each = |types: &mut TypeInterner, child: TypeId| map(types, child, language, depth + 1);
    let mapped = match found {
        Type::List(item) => Type::List(each(types, item)),
        Type::Set(item) => Type::Set(each(types, item)),
        Type::Optional(item) => Type::Optional(each(types, item)),
        Type::Future(item) => Type::Future(each(types, item)),
        Type::Dict(key, value) => Type::Dict(each(types, key), each(types, value)),
        Type::JsMap(key, value) => Type::JsMap(each(types, key), each(types, value)),
        Type::Tuple(items) => {
            Type::Tuple(items.into_iter().map(|item| each(types, item)).collect())
        }
        Type::Union(items) => {
            Type::Union(items.into_iter().map(|item| each(types, item)).collect())
        }
        Type::Class { name, args } => Type::Class {
            name,
            args: args.into_iter().map(|arg| each(types, arg)).collect(),
        },
        Type::Function(function) => Type::Function(FunctionType {
            params: function
                .params
                .iter()
                .map(|param| each(types, *param))
                .collect(),
            return_ty: each(types, function.return_ty),
            ..function
        }),
        Type::Generator {
            is_async,
            yield_ty,
            return_ty,
            next_ty,
        } => Type::Generator {
            is_async,
            yield_ty: each(types, yield_ty),
            return_ty: each(types, return_ty),
            next_ty: each(types, next_ty),
        },
        Type::GeneratorResult {
            yield_ty,
            return_ty,
        } => Type::GeneratorResult {
            yield_ty: each(types, yield_ty),
            return_ty: each(types, return_ty),
        },
        Type::IndexedAccess { object, index } => Type::IndexedAccess {
            object: each(types, object),
            index: each(types, index),
        },
        leaf @ (Type::Bool
        | Type::Int
        | Type::PyInt
        | Type::Float
        | Type::FixedInt(_)
        | Type::BigInt
        | Type::String
        | Type::PyStr
        | Type::Symbol
        | Type::PropKey
        | Type::Unknown
        | Type::Never
        | Type::TypeParam { .. }
        | Type::None) => leaf,
    };
    types.intern(mapped)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Python view replaces JavaScript text through containers, and leaves
    /// a type with no foreign text as it is.
    #[test]
    fn views_replace_the_other_languages_text() {
        let mut types = TypeInterner::default();
        let js = types.intern(Type::String);
        let py = types.intern(Type::PyStr);
        let float = types.intern(Type::Float);
        let list = types.intern(Type::List(js));
        let optional = types.intern(Type::Optional(list));
        let viewed = view_in(&mut types, optional, SourceLanguage::Python);
        let py_list = types.intern(Type::List(py));
        assert_eq!(types.get(viewed), Some(&Type::Optional(py_list)));
        assert!(crosses_into(&types, optional, SourceLanguage::Python));
        assert!(!crosses_into(&types, optional, SourceLanguage::TypeScript));
        assert_eq!(view_in(&mut types, float, SourceLanguage::Python), float);
        assert_eq!(
            view_in(&mut types, py_list, SourceLanguage::TypeScript),
            list
        );
    }
}
