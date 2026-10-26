//! Classification of Smelt HIR expression types against TypeScript 7 types.
//!
//! For one HIR expression and the type the TypeScript 7 checker (tsrs)
//! resolves for the node at the same span, [`classify`] answers whether
//! Smelt's lowering kept the type TypeScript knows. The comparison is by
//! *shape*, under the frontend's documented lowering conventions rather than
//! by spelling:
//!
//! - `number` (and number literals) is `Float`, `Int` or `FixedInt`;
//!   `string`, string literals and template literals are `String`; booleans
//!   and boolean literals are `Bool`; `null`, `undefined` and `void` are
//!   `None`;
//! - `T | undefined` / `T | null` is `Optional(T)` or a union with `None`; a
//!   union whose members all agree with one non-union HIR type agrees with it
//!   (`"a" | "b"` is a `String`);
//! - `T[]` is `List(T)`; a tuple is a `Tuple` of agreeing elements, or a
//!   `List` when every element agrees with its element type (rest tuples);
//! - `Map`/`WeakMap` are `JsMap` or `Dict`, `Set`/`WeakSet` are `Set`,
//!   `Promise`/`PromiseLike` are `Future`, generators are `Generator`, and
//!   every other class or interface is `Class` with the same source name;
//! - a structural object with call signatures and no properties is a
//!   `Function`; any other structural object is a `Class` (Smelt names
//!   anonymous shapes), a `Dict`/`JsMap` (index signatures) or a `Function`
//!   (callable objects with statics);
//! - an enum member agrees with its value's shape or the enum's `Class`.
//!
//! `any`, `unknown`, `object` and `symbol` are dynamic in TypeScript itself
//! (or erased by Smelt convention, see `ErasureReason`), so they are counted
//! separately and never as a Smelt gap. Types the checker leaves deferred
//! (still generic) are likewise counted apart.

use smelt_hir::{Crate, Type, TypeId};
use smelt_ts_types::model::{TsSymbolRef, TsTupleElement, TsTupleElementKind};
use smelt_ts_types::{TsType, TsTypeFacts, TsTypeId, TsTypeKind};

/// Recursion limit for nested shape comparison.
const MAX_DEPTH: usize = 8;

/// How one HIR expression's type relates to the TypeScript 7 type at its span.
///
/// Variants are declared from closest to farthest, so when several HIR
/// expressions share one source span the minimum is the span's verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Verdict {
    /// Smelt's type has the shape TypeScript resolved.
    Agree,
    /// Smelt keeps a wider type than TypeScript's (often flow-narrowed) one:
    /// `Optional<T>` or a union with an agreeing member where TypeScript proved `T`.
    Wider,
    /// TypeScript itself says `any`/`unknown`/`object`/`symbol`.
    TsDynamic,
    /// TypeScript keeps the type deferred (a still-generic operator).
    TsDeferred,
    /// Both are concrete but the shapes differ.
    Mismatch,
    /// Smelt erased to `Unknown` a value TypeScript types concretely: the parity gap.
    SmeltErased,
}

/// Classifies HIR type `hir` against checker type `ts`.
pub(crate) fn classify(krate: &Crate, hir: TypeId, facts: &TsTypeFacts, ts: TsTypeId) -> Verdict {
    let Some(ts_type) = facts.types.get(ts) else {
        return Verdict::TsDeferred;
    };
    if is_dynamic(&ts_type.kind) {
        return Verdict::TsDynamic;
    }
    if is_deferred(&ts_type.kind) {
        return Verdict::TsDeferred;
    }
    let comparer = Comparer { krate, facts };
    match krate.types.get(hir) {
        Some(Type::Unknown) | None => Verdict::SmeltErased,
        Some(_) if comparer.agrees(hir, ts, 0) => Verdict::Agree,
        Some(_) if comparer.wider(hir, ts, 0) => Verdict::Wider,
        Some(_) => Verdict::Mismatch,
    }
}

/// Whether TypeScript's type is dynamic (or erased by Smelt's convention).
const fn is_dynamic(kind: &TsTypeKind) -> bool {
    matches!(
        kind,
        TsTypeKind::Any | TsTypeKind::Unknown | TsTypeKind::NonPrimitive | TsTypeKind::Symbol | TsTypeKind::UniqueSymbol
    )
}

/// Whether TypeScript left the type unevaluated (or beyond the model's depth).
const fn is_deferred(kind: &TsTypeKind) -> bool {
    matches!(kind, TsTypeKind::Deferred(_) | TsTypeKind::Truncated)
}

/// Renders a HIR type's shape for reports (`List<Float>`, `Class(Point)`).
pub(crate) fn hir_shape(krate: &Crate, id: TypeId) -> String {
    shape_at_depth(krate, id, 0)
}

/// [`hir_shape`] with a depth bound.
fn shape_at_depth(krate: &Crate, id: TypeId, depth: usize) -> String {
    if depth > 3 {
        return "…".to_owned();
    }
    let next = depth.saturating_add(1);
    let Some(ty) = krate.types.get(id) else {
        return "?".to_owned();
    };
    let join = |members: &[TypeId], separator: &str| {
        members.iter().map(|&member| shape_at_depth(krate, member, next)).collect::<Vec<_>>().join(separator)
    };
    let symbol = |name| krate.symbols.source(name).unwrap_or("?");
    match ty {
        Type::List(inner) => format!("List<{}>", shape_at_depth(krate, *inner, next)),
        Type::Optional(inner) => format!("Optional<{}>", shape_at_depth(krate, *inner, next)),
        Type::Future(inner) => format!("Future<{}>", shape_at_depth(krate, *inner, next)),
        Type::Set(inner) => format!("Set<{}>", shape_at_depth(krate, *inner, next)),
        Type::Union(members) => join(members, " | "),
        Type::Tuple(members) => format!("({})", join(members, ", ")),
        Type::Class { name, .. } => format!("Class({})", symbol(*name)),
        Type::TypeParam { name } => format!("TypeParam({})", symbol(*name)),
        Type::Bool => "Bool".to_owned(),
        Type::Int => "Int".to_owned(),
        Type::Float => "Float".to_owned(),
        Type::FixedInt(_) => "FixedInt".to_owned(),
        Type::BigInt => "BigInt".to_owned(),
        Type::String => "String".to_owned(),
        Type::Unknown => "Unknown".to_owned(),
        Type::Never => "Never".to_owned(),
        Type::None => "None".to_owned(),
        Type::Dict(..) => "Dict".to_owned(),
        Type::JsMap(..) => "JsMap".to_owned(),
        Type::Function(_) => "Function".to_owned(),
        Type::Generator { .. } | Type::GeneratorResult { .. } => "Generator".to_owned(),
    }
}

/// Renders a checker type's shape for grouping report rows (`Array<String>`,
/// `Named(Map)`, `Object{2 props}`), coarser than the checker's own spelling
/// so one family of gaps groups under one key.
pub(crate) fn ts_shape(facts: &TsTypeFacts, id: TsTypeId) -> String {
    ts_shape_at_depth(facts, id, 0)
}

/// [`ts_shape`] with a depth bound.
fn ts_shape_at_depth(facts: &TsTypeFacts, id: TsTypeId, depth: usize) -> String {
    if depth > 2 {
        return "…".to_owned();
    }
    let next = depth.saturating_add(1);
    let Some(ty) = facts.types.get(id) else {
        return "?".to_owned();
    };
    let join = |members: &[TsTypeId], separator: &str| {
        members.iter().map(|&member| ts_shape_at_depth(facts, member, next)).collect::<Vec<_>>().join(separator)
    };
    match &ty.kind {
        TsTypeKind::StringLiteral(_) => "string-literal".to_owned(),
        TsTypeKind::NumberLiteral(_) => "number-literal".to_owned(),
        TsTypeKind::BooleanLiteral(_) => "boolean-literal".to_owned(),
        TsTypeKind::BigIntLiteral(_) => "bigint-literal".to_owned(),
        TsTypeKind::Union { enum_symbol: Some(symbol), .. } => format!("Enum({})", symbol.name),
        TsTypeKind::EnumMember { enum_symbol, .. } => format!("EnumMember({})", enum_symbol.name),
        TsTypeKind::Union { members, enum_symbol: None } => join(members, " | "),
        TsTypeKind::Intersection(members) => join(members, " & "),
        TsTypeKind::Array { element, .. } => format!("Array<{}>", ts_shape_at_depth(facts, *element, next)),
        TsTypeKind::Tuple { elements, .. } => {
            let ids: Vec<TsTypeId> = elements.iter().map(|element| element.ty).collect();
            format!("[{}]", join(&ids, ", "))
        }
        TsTypeKind::Named { symbol, .. } => format!("Named({})", symbol.name),
        TsTypeKind::Object(object) if object.properties.is_empty() && !object.call_signatures.is_empty() => {
            "Function".to_owned()
        }
        TsTypeKind::Object(object) => format!("Object{{{} props}}", object.properties.len()),
        TsTypeKind::TypeParameter { .. } => "TypeParam".to_owned(),
        TsTypeKind::Any
        | TsTypeKind::Unknown
        | TsTypeKind::Never
        | TsTypeKind::Void
        | TsTypeKind::Undefined
        | TsTypeKind::Null
        | TsTypeKind::String
        | TsTypeKind::Number
        | TsTypeKind::Boolean
        | TsTypeKind::BigInt
        | TsTypeKind::Symbol
        | TsTypeKind::NonPrimitive
        | TsTypeKind::UniqueSymbol
        | TsTypeKind::Deferred(_)
        | TsTypeKind::Truncated => ty.display.clone(),
    }
}

/// Recursive shape comparison over one crate and one fact table.
struct Comparer<'input> {
    /// HIR types and symbols.
    krate: &'input Crate,
    /// Checker types.
    facts: &'input TsTypeFacts,
}

impl Comparer<'_> {
    /// Whether HIR type `hir` has the shape of checker type `ts`.
    fn agrees(&self, hir: TypeId, ts: TsTypeId, depth: usize) -> bool {
        if depth > MAX_DEPTH {
            return true;
        }
        let (Some(hir_type), Some(ts_type)) = (self.krate.types.get(hir), self.facts.types.get(ts)) else {
            return false;
        };
        if is_dynamic(&ts_type.kind) || is_deferred(&ts_type.kind) {
            return true;
        }
        let next = depth.saturating_add(1);
        if let Type::Optional(inner) = hir_type {
            return self.optional_agrees(*inner, ts_type, next);
        }
        self.primitive_agrees(hir_type, &ts_type.kind)
            .unwrap_or_else(|| self.composite_agrees(hir, hir_type, ts_type, next))
    }

    /// Primitive and literal kinds; `None` when `ts` is not primitive.
    fn primitive_agrees(&self, hir: &Type, ts: &TsTypeKind) -> Option<bool> {
        match ts {
            TsTypeKind::String | TsTypeKind::StringLiteral(_) => Some(matches!(hir, Type::String)),
            TsTypeKind::Number | TsTypeKind::NumberLiteral(_) => {
                Some(matches!(hir, Type::Float | Type::Int | Type::FixedInt(_)))
            }
            TsTypeKind::Boolean | TsTypeKind::BooleanLiteral(_) => Some(matches!(hir, Type::Bool)),
            TsTypeKind::BigInt | TsTypeKind::BigIntLiteral(_) => Some(matches!(hir, Type::BigInt)),
            TsTypeKind::Void | TsTypeKind::Undefined | TsTypeKind::Null => Some(matches!(hir, Type::None)),
            // `never` is the bottom type: every HIR type can hold it (an empty
            // array literal is `never[]` before its uses widen it).
            TsTypeKind::Never => Some(true),
            TsTypeKind::TypeParameter { constraint, is_this, .. } => {
                Some(matches!(hir, Type::TypeParam { .. }) || (*is_this && self.this_agrees(hir, *constraint)))
            }
            TsTypeKind::EnumMember { enum_symbol, value } => {
                Some(self.named_class(hir, &enum_symbol.name) || self.value_agrees(hir, *value))
            }
            TsTypeKind::Any
            | TsTypeKind::Unknown
            | TsTypeKind::Symbol
            | TsTypeKind::NonPrimitive
            | TsTypeKind::UniqueSymbol
            | TsTypeKind::Union { .. }
            | TsTypeKind::Intersection(_)
            | TsTypeKind::Array { .. }
            | TsTypeKind::Tuple { .. }
            | TsTypeKind::Named { .. }
            | TsTypeKind::Object(_)
            | TsTypeKind::Deferred(_)
            | TsTypeKind::Truncated => None,
        }
    }

    /// TypeScript's polymorphic `this` is a type parameter constrained to the
    /// enclosing class; lowering it to that class is the hand-port answer
    /// (Rust's `Self`), so `this` agrees with whatever agrees with its constraint.
    fn this_agrees(&self, hir: &Type, constraint: Option<TsTypeId>) -> bool {
        constraint.is_some_and(|bound| {
            self.facts.types.get(bound).is_some_and(|bound_type| self.composite_agrees_type(hir, bound_type))
        })
    }

    /// `composite_agrees` for a HIR type without its id (non-union constraints only).
    fn composite_agrees_type(&self, hir: &Type, ts: &TsType) -> bool {
        match &ts.kind {
            TsTypeKind::Named { symbol, type_arguments } => self.named_agrees(hir, &symbol.name, type_arguments, 1),
            TsTypeKind::Object(_) => matches!(hir, Type::Class { .. }),
            TsTypeKind::Any
            | TsTypeKind::Unknown
            | TsTypeKind::Never
            | TsTypeKind::Void
            | TsTypeKind::Undefined
            | TsTypeKind::Null
            | TsTypeKind::String
            | TsTypeKind::Number
            | TsTypeKind::Boolean
            | TsTypeKind::BigInt
            | TsTypeKind::Symbol
            | TsTypeKind::NonPrimitive
            | TsTypeKind::StringLiteral(_)
            | TsTypeKind::NumberLiteral(_)
            | TsTypeKind::BooleanLiteral(_)
            | TsTypeKind::BigIntLiteral(_)
            | TsTypeKind::UniqueSymbol
            | TsTypeKind::EnumMember { .. }
            | TsTypeKind::Union { .. }
            | TsTypeKind::Intersection(_)
            | TsTypeKind::TypeParameter { .. }
            | TsTypeKind::Array { .. }
            | TsTypeKind::Tuple { .. }
            | TsTypeKind::Deferred(_)
            | TsTypeKind::Truncated => false,
        }
    }

    /// Whether `hir` is a widening of `ts`: an `Optional` (at any depth) or a
    /// union one of whose members agrees with `ts`.
    fn wider(&self, hir: TypeId, ts: TsTypeId, depth: usize) -> bool {
        if depth > MAX_DEPTH {
            return false;
        }
        let next = depth.saturating_add(1);
        match self.krate.types.get(hir) {
            Some(Type::Optional(inner)) => self.agrees(*inner, ts, next) || self.wider(*inner, ts, next),
            Some(Type::Union(members)) => {
                members.iter().any(|&member| self.agrees(member, ts, next) || self.wider(member, ts, next))
            }
            Some(_) | None => false,
        }
    }

    /// Whether `hir` agrees with an enum member's literal value type.
    fn value_agrees(&self, hir: &Type, value: TsTypeId) -> bool {
        self.facts
            .types
            .get(value)
            .and_then(|ty| self.primitive_agrees(hir, &ty.kind))
            .unwrap_or(false)
    }

    /// Unions, intersections, arrays, tuples, nominal and structural objects.
    ///
    /// `hir_id` and `hir` are the same HIR type (the interner has no reverse lookup).
    fn composite_agrees(&self, hir_id: TypeId, hir: &Type, ts: &TsType, depth: usize) -> bool {
        match &ts.kind {
            TsTypeKind::Union { members, enum_symbol } => {
                self.union_agrees(hir_id, members, enum_symbol.as_ref(), depth)
            }
            TsTypeKind::Intersection(members) => {
                matches!(hir, Type::Class { .. } | Type::Dict(..))
                    || members.iter().any(|&member| self.agrees(hir_id, member, depth))
            }
            TsTypeKind::Array { element, .. } => {
                matches!(hir, Type::List(inner) if self.agrees(*inner, *element, depth))
            }
            TsTypeKind::Tuple { elements, .. } => self.tuple_agrees(hir, elements, depth),
            TsTypeKind::Named { symbol, type_arguments } => {
                self.named_agrees(hir, &symbol.name, type_arguments, depth)
            }
            TsTypeKind::Object(object) if object.properties.is_empty() && !object.call_signatures.is_empty() => {
                matches!(hir, Type::Function(_))
            }
            TsTypeKind::Object(_) => {
                matches!(hir, Type::Class { .. } | Type::Dict(..) | Type::JsMap(..) | Type::Function(_))
            }
            TsTypeKind::Any
            | TsTypeKind::Unknown
            | TsTypeKind::Never
            | TsTypeKind::Void
            | TsTypeKind::Undefined
            | TsTypeKind::Null
            | TsTypeKind::String
            | TsTypeKind::Number
            | TsTypeKind::Boolean
            | TsTypeKind::BigInt
            | TsTypeKind::Symbol
            | TsTypeKind::NonPrimitive
            | TsTypeKind::StringLiteral(_)
            | TsTypeKind::NumberLiteral(_)
            | TsTypeKind::BooleanLiteral(_)
            | TsTypeKind::BigIntLiteral(_)
            | TsTypeKind::UniqueSymbol
            | TsTypeKind::EnumMember { .. }
            | TsTypeKind::TypeParameter { .. }
            | TsTypeKind::Deferred(_)
            | TsTypeKind::Truncated => false,
        }
    }

    /// HIR `Optional(inner)` against a checker type: the checker side must be a
    /// nullable union whose non-null part agrees with `inner`.
    fn optional_agrees(&self, inner: TypeId, ts: &TsType, depth: usize) -> bool {
        let TsTypeKind::Union { members, .. } = &ts.kind else {
            return false;
        };
        let (nullish, rest) = self.split_nullish(members);
        !nullish.is_empty() && rest.iter().all(|&member| self.agrees(inner, member, depth))
    }

    /// A checker union against a non-optional HIR type.
    fn union_agrees(
        &self,
        hir_id: TypeId,
        members: &[TsTypeId],
        enum_symbol: Option<&TsSymbolRef>,
        depth: usize,
    ) -> bool {
        let Some(hir) = self.krate.types.get(hir_id) else {
            return false;
        };
        if enum_symbol.is_some_and(|declared| self.named_class(hir, &declared.name)) {
            return true;
        }
        if let Type::Union(hir_members) = hir {
            return members.iter().all(|&member| {
                hir_members.iter().any(|&hir_member| self.agrees(hir_member, member, depth))
            });
        }
        members.iter().all(|&member| self.agrees(hir_id, member, depth))
    }

    /// Splits union members into nullish (`null`/`undefined`/`void`) and the rest.
    fn split_nullish(&self, members: &[TsTypeId]) -> (Vec<TsTypeId>, Vec<TsTypeId>) {
        members.iter().partition(|&&member| {
            self.facts.types.get(member).is_some_and(|ty| {
                matches!(ty.kind, TsTypeKind::Undefined | TsTypeKind::Null | TsTypeKind::Void)
            })
        })
    }

    /// A tuple: elementwise `Tuple`, or a `List` every element agrees with.
    fn tuple_agrees(&self, hir: &Type, elements: &[TsTupleElement], depth: usize) -> bool {
        match hir {
            Type::Tuple(hir_elements) => {
                hir_elements.len() == elements.len()
                    && hir_elements
                        .iter()
                        .zip(elements)
                        .all(|(&hir_element, element)| self.agrees(hir_element, element.ty, depth))
            }
            Type::List(inner) => elements.iter().all(|element| {
                element.kind == TsTupleElementKind::Rest || self.agrees(*inner, element.ty, depth)
            }),
            Type::Bool
            | Type::Int
            | Type::Float
            | Type::FixedInt(_)
            | Type::BigInt
            | Type::String
            | Type::Unknown
            | Type::Never
            | Type::TypeParam { .. }
            | Type::None
            | Type::Set(_)
            | Type::Dict(..)
            | Type::JsMap(..)
            | Type::Optional(_)
            | Type::Union(_)
            | Type::Class { .. }
            | Type::Function(_)
            | Type::Future(_)
            | Type::Generator { .. }
            | Type::GeneratorResult { .. } => false,
        }
    }

    /// A class or interface reference, honoring the host-collection mappings.
    fn named_agrees(&self, hir: &Type, name: &str, type_arguments: &[TsTypeId], depth: usize) -> bool {
        match (name, hir) {
            ("Map" | "ReadonlyMap" | "WeakMap", Type::JsMap(..) | Type::Dict(..))
            | ("Set" | "ReadonlySet" | "WeakSet", Type::Set(_))
            | (
                "Generator" | "AsyncGenerator" | "IterableIterator" | "AsyncIterableIterator",
                Type::Generator { .. },
            ) => true,
            ("Promise" | "PromiseLike", Type::Future(inner)) => {
                type_arguments.first().is_none_or(|&argument| self.agrees(*inner, argument, depth))
            }
            _ => self.named_class(hir, name),
        }
    }

    /// Whether `hir` is a class whose source name is `name`.
    ///
    /// The recorded original name wins over the interned spelling: lowering
    /// renames classes for the Rust type system (a crate-wide duplicate
    /// `Context` becomes `Context_1`, a class expression gets an internal
    /// name) while the source name is what TypeScript reports.
    fn named_class(&self, hir: &Type, name: &str) -> bool {
        let Type::Class { name: symbol, .. } = hir else {
            return false;
        };
        let original = self.krate.names.get(*symbol).or_else(|| self.krate.symbols.source(*symbol));
        original == Some(name)
    }
}
