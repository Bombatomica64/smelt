//! Source facts a strict JSON entry decoder needs beyond a property's HIR type.
//!
//! HIR erases literal types to their primitive (`"a" | "b"` lowers to
//! `String`) and folds `T | null`, `T | undefined` and `p?: T` into one
//! `Optional<T>`. That is the right storage answer, but a typed entry boundary
//! (`const v: T = JSON.parse(s)`) has to answer exactly what TypeScript would:
//! does `{"mode":"c"}` match `mode: "a" | "b"`, does `null` match
//! `p?: number`? This module reads those answers off the TypeScript annotation
//! while it is still in hand and records them as
//! [`smelt_hir::FieldDecodeFacts`] on the interface.
//!
//! Design adapted from Bun's `--aot` typed validation (`Bun.validate<T>`,
//! oven-sh/bun#45631, `docs/bundler/executables.mdx` "Validate data by its
//! type"): literal types are checked by value, and `null` passes only where the
//! type admits it.
//!
//! Aliases (`type Mode = "a" | "b"`) carry their own facts on the lowered
//! `smelt_hir::TypeAlias`, so a property typed `Mode` decodes exactly as one
//! spelled `"a" | "b"`. A reference to an alias that has not been lowered yet
//! contributes no literal constraint: the decoder then checks the primitive
//! kind only, never rejecting more than TypeScript would.

use oxc::ast::ast::{Expression, TSLiteral, TSType, TSTypeName, UnaryOperator};
use std::collections::HashSet;

use smelt_hir::{FieldDecodeFacts, Literal, TypeDecodeFacts};

use crate::lowering::ModuleBuilder;

/// Facts of a type that admits no nullish value and no literal constraint.
const fn open_facts() -> TypeDecodeFacts {
    TypeDecodeFacts {
        admits_null: false,
        admits_undefined: false,
        literals: None,
    }
}

/// The identity of [`union_facts`]: a union with no arm seen yet.
const fn empty_union_facts() -> TypeDecodeFacts {
    TypeDecodeFacts {
        admits_null: false,
        admits_undefined: false,
        literals: Some(Vec::new()),
    }
}

/// A hashable identity for a literal value, equal exactly when the values are
/// equal under `Literal`'s `PartialEq` (so `0` and `-0` share one key), or
/// `None` for a value equal to nothing (`NaN`).
fn literal_key(value: &Literal) -> Option<(u8, u64, &str)> {
    match value {
        Literal::Bool(flag) => Some((0, u64::from(*flag), "")),
        Literal::Int(number) => Some((1, u64::from_ne_bytes(number.to_ne_bytes()), "")),
        Literal::Float(number) if number.is_nan() => None,
        Literal::Float(number) => Some((2, (if *number == 0.0 { 0.0_f64 } else { *number }).to_bits(), "")),
        Literal::String(text) => Some((3, 0, text.as_str())),
        Literal::Symbol(text) => Some((4, 0, text.as_str())),
        Literal::BigInt(digits) => Some((7, 0, digits.as_str())),
        Literal::Undefined => Some((5, 0, "")),
        Literal::None => Some((6, 0, "")),
    }
}

/// Combine the facts of two union arms: nullish arms accumulate, and the
/// literal list survives only while every arm is a literal (or nullish).
///
/// `seen` holds the keys of the literals already in `left`, so a union of `n`
/// literal arms deduplicates in linear time (a `Vec::contains` per arm made
/// `type U = "a0" | … | "aN"` quadratic -- caught by the `compile_scaling`
/// string-literal-union shape).
fn union_facts(
    left: TypeDecodeFacts,
    right: TypeDecodeFacts,
    seen: &mut HashSet<(u8, u64, String)>,
) -> TypeDecodeFacts {
    TypeDecodeFacts {
        admits_null: left.admits_null || right.admits_null,
        admits_undefined: left.admits_undefined || right.admits_undefined,
        literals: match (left.literals, right.literals) {
            (Some(mut values), Some(more)) => {
                for value in more {
                    let fresh = literal_key(&value).is_none_or(|(tag, bits, text)| {
                        seen.insert((tag, bits, text.to_owned()))
                    });
                    if fresh {
                        values.push(value);
                    }
                }
                Some(values)
            }
            _ => None,
        },
    }
}

impl ModuleBuilder<'_> {
    /// Compute the decode facts of a TypeScript type annotation.
    ///
    /// Walks unions, parentheses and alias references; any other type is open
    /// (no literal constraint, no nullish arm).
    pub(in crate::lowering) fn ts_type_decode_facts(&mut self, ty: &TSType<'_>) -> TypeDecodeFacts {
        match ty {
            TSType::TSNullKeyword(_) => TypeDecodeFacts {
                admits_null: true,
                ..empty_union_facts()
            },
            TSType::TSUndefinedKeyword(_) | TSType::TSVoidKeyword(_) => TypeDecodeFacts {
                admits_undefined: true,
                ..empty_union_facts()
            },
            TSType::TSLiteralType(literal) => {
                let value = match &literal.literal {
                    TSLiteral::StringLiteral(value) => Some(Literal::String(value.value.to_string())),
                    TSLiteral::NumericLiteral(value) => Some(Literal::Float(value.value)),
                    TSLiteral::BooleanLiteral(value) => Some(Literal::Bool(value.value)),
                    TSLiteral::UnaryExpression(unary)
                        if unary.operator == UnaryOperator::UnaryNegation =>
                    {
                        match &unary.argument {
                            Expression::NumericLiteral(value) => Some(Literal::Float(-value.value)),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                value.map_or_else(open_facts, |value| TypeDecodeFacts {
                    literals: Some(vec![value]),
                    ..open_facts()
                })
            }
            TSType::TSUnionType(union) => {
                let mut seen = HashSet::new();
                union
                    .types
                    .iter()
                    .map(|member| self.ts_type_decode_facts(member))
                    .fold(empty_union_facts(), |left, right| union_facts(left, right, &mut seen))
            }
            TSType::TSParenthesizedType(inner) => self.ts_type_decode_facts(&inner.type_annotation),
            TSType::TSTypeReference(reference) if reference.type_arguments.is_none() => {
                let TSTypeName::IdentifierReference(name) = &reference.type_name else {
                    return open_facts();
                };
                // The alias was lowered before this reference (its facts were
                // computed then), so the recursion stops at an alias that
                // refers to itself: its own facts are not yet recorded.
                let symbol = self.intern_type_name(name.name.as_str());
                self.find_type_alias(symbol)
                    .and_then(|alias| alias.decode_facts.clone())
                    .unwrap_or_else(open_facts)
            }
            _ => open_facts(),
        }
    }

    /// The facts a lowered type alias records, or `None` when the aliased type
    /// says nothing its lowered HIR type does not.
    pub(in crate::lowering) fn alias_decode_facts_of(
        &mut self,
        annotation: &TSType<'_>,
    ) -> Option<TypeDecodeFacts> {
        let facts = self.ts_type_decode_facts(annotation);
        (facts != open_facts()).then_some(facts)
    }

    /// Build the per-property facts for one interface/type-literal property.
    ///
    /// `optional` is the `?` marker. A literal list is kept only when it is
    /// non-empty: a property typed `null` alone has nothing to compare by value,
    /// and its storage type already pins it.
    pub(in crate::lowering) fn field_decode_facts(
        &mut self,
        name: smelt_hir::Symbol,
        optional: bool,
        annotation: &TSType<'_>,
    ) -> FieldDecodeFacts {
        let facts = self.ts_type_decode_facts(annotation);
        FieldDecodeFacts {
            name,
            may_be_absent: optional || facts.admits_undefined,
            admits_null: facts.admits_null,
            literals: facts.literals.unwrap_or_default(),
        }
    }
}
