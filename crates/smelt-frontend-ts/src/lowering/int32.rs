//! Bun's `int32`: a 32-bit integer that the build proves.
//!
//! Design adapted from Bun's `--aot` (oven-sh/bun#45631): the "32-bit
//! integers" section of `docs/bundler/executables.mdx`, the proof rules of
//! `src/sema/check/aot/int32_proofs.rs` and `integers.rs`, and the diagnostics
//! TS100031–TS100034 of `src/sema/messages.rs`.
//!
//! # The rule
//!
//! `import type { int32 } from "bun"` names a whole number from `-2^31` to
//! `2^31 - 1`. Only the type that `"bun"` declares has this meaning, under any
//! local name it is imported or aliased as; a module's own
//! `type int32 = number` is a `number`. It has this meaning only where it is the
//! WHOLE type of a variable, field, parameter or result; inside another type
//! (`int32[]`, `int32 | undefined`, `Map<string, int32>`) it is `number`. An
//! optional field or parameter `x?: int32` is an int32 or `undefined`.
//!
//! * **Type preservation.** Such a slot is an `I32` [`Type::FixedInt`] in HIR, so
//!   it is a Rust `i32` (an `Option<i32>` when optional) in the generated code:
//!   a struct field, a parameter, a return type, a local.
//! * **Behavior.** Every value STORED into an int32 slot — an initializer, an
//!   assignment, `+=`, `++`, an argument, a `return`, an object-literal property
//!   — must be provably an int32 by the rules of Bun's table (see
//!   [`Int32Prover`]); otherwise lowering stops with a diagnostic carrying Bun's
//!   wording and fix. Because every store is proven, the exact conversion
//!   `value as i32` never loses information, and nothing is checked when the
//!   program runs. Every READ of an int32 slot is converted back to a `number`
//!   (`as f64`, exact) right where it is lowered, so all arithmetic on int32
//!   values stays JavaScript `number` arithmetic: `a + b` of two int32 values is
//!   the `f64` sum, never a wrapping or panicking `i32` add. `x as int32` is
//!   `x | 0` (`ToInt32`), exactly as Bun defines it.
//!
//! # Diagnostic codes
//!
//! Smelt diagnostic codes are stable strings; the int32 family is named after
//! the Bun code it reproduces:
//!
//! | Smelt code                        | Bun code | Meaning                                         |
//! | --------------------------------- | -------- | ----------------------------------------------- |
//! | `smelt::int32-unproven`           | TS100031 | a stored value is not provably an int32         |
//! | `smelt::int32-not-int32`          | TS100032 | a stored constant is not an int32 at all        |
//! | `smelt::int32-function-as-value`  | TS100033 | a function with an int32 parameter is a value   |
//! | `smelt::int32-differs`            | TS100034 | a member is int32 in only one of two related types |

use std::collections::{HashMap, HashSet};

use oxc::ast::ast::{TSType, TSTypeName, TSTypeReference};
use smelt_hir::{
    BinOp, Body, BodyId, Expr, ExprId, ExprKind, IntWidth, Item, ItemId, Literal, LocalId,
    Pattern, PrimitiveCastOp, Span, Stmt, StringSearchOp, Symbol, Type, TypeId, UnaryOp,
};
use smelt_stdlib::DiagnosticCategory;

use super::ModuleBuilder;
use crate::SmeltError;

/// The module whose `int32` export carries the meaning.
pub(super) const BUN_MODULE: &str = "bun";
/// The export name of the type.
pub(super) const INT32_EXPORT: &str = "int32";
/// Smallest int32.
const INT32_MIN: i128 = -2_147_483_648;
/// Largest int32.
const INT32_MAX: i128 = 2_147_483_647;
/// Largest length a JavaScript string can have.
const MAX_STRING_LENGTH: i128 = INT32_MAX;
/// Largest length a JavaScript array can have.
const MAX_ARRAY_LENGTH: i128 = 4_294_967_295;
/// Source text longer than this is described rather than quoted (Bun's limit).
const MAX_QUOTED_LENGTH: usize = 48;

/// Diagnostic code for an unproven store (Bun TS100031).
pub(in crate::lowering) const UNPROVEN_CODE: &str = "smelt::int32-unproven";
/// Diagnostic code for a constant that is not an int32 (Bun TS100032).
pub(in crate::lowering) const NOT_AN_INT32_CODE: &str = "smelt::int32-not-int32";
/// Diagnostic code for a function with an int32 parameter used as a value (Bun TS100033).
pub(in crate::lowering) const USED_AS_A_VALUE_CODE: &str = "smelt::int32-function-as-value";
/// Diagnostic code for a member that is int32 in only one of two related types (Bun TS100034).
pub(in crate::lowering) const DIFFERS_CODE: &str = "smelt::int32-differs";

/// Whether `ty` is an int32 slot, and whether it is the optional form.
fn int32_slot(types: &smelt_hir::TypeInterner, ty: TypeId) -> Option<bool> {
    match types.get(ty)? {
        Type::FixedInt(IntWidth::I32) => Some(false),
        Type::Optional(inner) if types.get(*inner) == Some(&Type::FixedInt(IntWidth::I32)) => {
            Some(true)
        }
        _ => None,
    }
}

/// Builds a diagnostic of the int32 family.
fn int32_error(code: &'static str, span: Span, message: String) -> SmeltError {
    SmeltError {
        code,
        category: DiagnosticCategory::TypeConstraint,
        span,
        message,
        note: None,
    }
}

/// The slot a value is stored into, for the diagnostic's subject.
#[derive(Debug, Clone)]
enum Int32Place {
    /// A local variable: `'name'`.
    Variable(String),
    /// A parameter of a function: `Parameter 'by' of 'advance'`.
    Parameter {
        /// Parameter name.
        name: String,
        /// Function name, when it has one.
        function: Option<String>,
    },
    /// A property of a class or interface: `Property 'column' of 'Cursor'`.
    Property {
        /// Property name.
        name: String,
        /// Declaring type name, when known.
        container: Option<String>,
    },
    /// A function's result: `The result of 'advance'`.
    Result(Option<String>),
}

impl Int32Place {
    /// Bun's wording for the subject of an int32 diagnostic.
    fn describe(&self) -> String {
        match self {
            Self::Variable(name) => format!("'{name}'"),
            Self::Parameter {
                name,
                function: Some(function),
            } => format!("Parameter '{name}' of '{function}'"),
            Self::Parameter {
                name,
                function: None,
            } => format!("Parameter '{name}'"),
            Self::Property {
                name,
                container: Some(container),
            } => format!("Property '{name}' of '{container}'"),
            Self::Property {
                name,
                container: None,
            } => format!("Property '{name}'"),
            Self::Result(Some(function)) => format!("The result of '{function}'"),
            Self::Result(None) => "The result of this function".to_owned(),
        }
    }
}

/// Why a value could not be proven an int32.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Unproven {
    /// The value is a whole number in this range, which leaves the int32 range.
    Range(i128, i128),
    /// A division can leave a fraction.
    Division,
    /// A product can be too large to be exact, and it can be `-0`.
    Product,
    /// A remainder can be `-0` or `NaN`.
    Remainder,
    /// A power can be out of range or have a fraction.
    Power,
    /// The negative of `0` is `-0`.
    Negation,
    /// Nothing more is known than that it is a `number`.
    Number,
}

impl Unproven {
    /// Bun's wording for the reason.
    fn describe(&self) -> String {
        match self {
            Self::Range(minimum, maximum) => {
                format!("it can be anything from {minimum} to {maximum}")
            }
            Self::Division => "a division can leave a fraction".to_owned(),
            Self::Product => "a product can be too large to be exact, and it can be -0".to_owned(),
            Self::Remainder => "a remainder can be -0 or NaN".to_owned(),
            Self::Power => "a power can be out of range or have a fraction".to_owned(),
            Self::Negation => "the negative of 0 is -0, which is not an int32".to_owned(),
            Self::Number => "all that is known is that it is a 'number', which can have a fraction, be out of range, or be NaN or -0".to_owned(),
        }
    }
}

/// Proves values to be int32s by interval reasoning over HIR expressions.
///
/// A proven value is a whole number in `[-2^31, 2^31 - 1]` that is never `-0`
/// or `NaN`. The rules are Bun's table:
///
/// * an integer constant in range (constants are folded through `-`, `+`,
///   `-`, `*`, `/`, `%` and `**` first);
/// * another int32: a variable, a field, a parameter, a result (each read is a
///   `ToFloat` conversion of an int32 slot);
/// * `a | b`, `a & b`, `a ^ b`, `~a`, `a << n`, `a >> n` of any numbers, and
///   `a >>> n` when its range fits (`a >>> 0` reaches `2^32 - 1`);
/// * `text.length`, `text.indexOf(..)` and `text.lastIndexOf(..)` of strings
///   (an array's `.length` reaches `2^32 - 1`);
/// * arithmetic whose interval stays in range (`(a & 255) * 3 + 1`), where a
///   product is only proven when it cannot be `-0` and a remainder only when
///   the dividend is not negative and the divisor positive;
/// * `i + 1` and `i++` where a test `i < end` (or `i <= end`, `i > start`,
///   `i >= start`) bounds `i`; the caller supplies the refined intervals.
struct Int32Prover<'a> {
    /// The body holding the expressions.
    body: &'a Body,
    /// The crate's types.
    types: &'a smelt_hir::TypeInterner,
    /// Locals whose range a dominating test narrowed, with that range.
    refined: &'a HashMap<LocalId, (i128, i128)>,
}

impl Int32Prover<'_> {
    /// The expression `id`.
    fn expr(&self, id: ExprId) -> &Expr {
        &self.body.exprs[id.0 as usize]
    }

    /// The constant `number` an expression always evaluates to, if any.
    fn constant(&self, id: ExprId) -> Option<f64> {
        match &self.expr(id).kind {
            ExprKind::Literal(Literal::Float(value)) => Some(*value),
            ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToFloat,
                operand,
            } => self.constant(*operand),
            ExprKind::UnaryOp {
                op: UnaryOp::Neg,
                operand,
            } => self.constant(*operand).map(|value| -value),
            ExprKind::BinOp { op, lhs, rhs } => {
                let (lhs, rhs) = (self.constant(*lhs)?, self.constant(*rhs)?);
                match op {
                    BinOp::Add => Some(lhs + rhs),
                    BinOp::Sub => Some(lhs - rhs),
                    BinOp::Mul => Some(lhs * rhs),
                    BinOp::Div => Some(lhs / rhs),
                    BinOp::Rem => Some(lhs % rhs),
                    _ => None,
                }
            }
            ExprKind::NumericPow { base, exponent } => {
                Some(self.constant(*base)?.powf(self.constant(*exponent)?))
            }
            _ => None,
        }
    }

    /// The interval of whole numbers (never `-0` or `NaN`) an expression is
    /// proven to stay in, or why none is known.
    fn interval(&self, id: ExprId) -> Result<(i128, i128), Unproven> {
        if let Some(value) = self.constant(id) {
            return whole_number(value)
                .map(|value| (value, value))
                .ok_or(Unproven::Number);
        }
        let expr = self.expr(id);
        match &expr.kind {
            ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToFloat,
                operand,
            } if int32_slot(self.types, self.expr(*operand).ty) == Some(false)
                && is_slot_read(&self.expr(*operand).kind) =>
            {
                if let ExprKind::Local(local) = self.expr(*operand).kind
                    && let Some(range) = self.refined.get(&local)
                {
                    return Ok(*range);
                }
                Ok((INT32_MIN, INT32_MAX))
            }
            ExprKind::BinOp { op, lhs, rhs } => self.binary_interval(*op, *lhs, *rhs),
            ExprKind::UnaryOp {
                op: UnaryOp::Neg,
                operand,
            } => {
                let (lo, hi) = self.interval(*operand)?;
                if lo <= 0 && hi >= 0 {
                    return Err(Unproven::Negation);
                }
                Ok((-hi, -lo))
            }
            ExprKind::Conditional {
                then_expr,
                else_expr,
                ..
            } => {
                let (then_range, else_range) =
                    (self.interval(*then_expr)?, self.interval(*else_expr)?);
                Ok((then_range.0.min(else_range.0), then_range.1.max(else_range.1)))
            }
            ExprKind::Len { operand } => match self.types.get(self.expr(*operand).ty) {
                Some(Type::String) => Ok((0, MAX_STRING_LENGTH)),
                _ => Err(Unproven::Range(0, MAX_ARRAY_LENGTH)),
            },
            ExprKind::StringSearch {
                op: StringSearchOp::Find | StringSearchOp::RFind | StringSearchOp::Regex,
                ..
            } => Ok((-1, MAX_STRING_LENGTH - 1)),
            ExprKind::NumericPow { .. } => Err(Unproven::Power),
            ExprKind::NumericUnaryFunc {
                op: smelt_hir::NumericUnaryFuncOp::Clz32,
                ..
            } => Ok((0, 32)),
            _ => Err(Unproven::Number),
        }
    }

    /// The interval of a binary operation.
    fn binary_interval(&self, op: BinOp, lhs: ExprId, rhs: ExprId) -> Result<(i128, i128), Unproven> {
        match op {
            BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::IMul => Ok((INT32_MIN, INT32_MAX)),
            BinOp::BitAnd => {
                // A mask that is known not negative bounds the result to `[0, mask]`.
                Ok([lhs, rhs]
                    .into_iter()
                    .filter_map(|side| self.interval(side).ok())
                    .filter(|(lo, hi)| *lo >= 0 && *hi <= INT32_MAX)
                    .map(|(_, hi)| hi)
                    .min()
                    .map_or((INT32_MIN, INT32_MAX), |mask| (0, mask)))
            }
            BinOp::Shr => {
                // `a >> n` with a constant shift divides the int32 range.
                Ok(self.constant(rhs).and_then(whole_number).map_or(
                    (INT32_MIN, INT32_MAX),
                    |shift| {
                        let shift = shift.rem_euclid(32);
                        (INT32_MIN >> shift, INT32_MAX >> shift)
                    },
                ))
            }
            BinOp::UShr => {
                let top = self
                    .constant(rhs)
                    .and_then(whole_number)
                    .map_or(MAX_ARRAY_LENGTH, |shift| MAX_ARRAY_LENGTH >> shift.rem_euclid(32));
                Ok((0, top))
            }
            BinOp::Add => {
                let (left, right) = (self.interval(lhs)?, self.interval(rhs)?);
                Ok((left.0 + right.0, left.1 + right.1))
            }
            BinOp::Sub => {
                let (left, right) = (self.interval(lhs)?, self.interval(rhs)?);
                Ok((left.0 - right.1, left.1 - right.0))
            }
            BinOp::Mul => {
                let (Ok(left), Ok(right)) = (self.interval(lhs), self.interval(rhs)) else {
                    return Err(Unproven::Product);
                };
                // `0 * -n` is `-0`: a product is only an int32 when no factor
                // can be zero while the other can be negative.
                let zero_with_negative = (contains_zero(left) && right.0 < 0)
                    || (contains_zero(right) && left.0 < 0);
                if zero_with_negative {
                    return Err(Unproven::Product);
                }
                let products = [
                    left.0 * right.0,
                    left.0 * right.1,
                    left.1 * right.0,
                    left.1 * right.1,
                ];
                let lo = products.iter().copied().min().unwrap_or(0);
                let hi = products.iter().copied().max().unwrap_or(0);
                Ok((lo, hi))
            }
            BinOp::Rem => {
                let (Ok(dividend), Ok(divisor)) = (self.interval(lhs), self.interval(rhs)) else {
                    return Err(Unproven::Remainder);
                };
                if dividend.0 < 0 || divisor.0 <= 0 {
                    return Err(Unproven::Remainder);
                }
                Ok((0, dividend.1.min(divisor.1 - 1)))
            }
            BinOp::Div => Err(Unproven::Division),
            _ => Err(Unproven::Number),
        }
    }
}

/// Whether an expression reads a slot (so an int32 type on it is a proven
/// fact about the stored value) rather than computing a new value.
const fn is_slot_read(kind: &ExprKind) -> bool {
    matches!(
        kind,
        ExprKind::Local(_)
            | ExprKind::Field { .. }
            | ExprKind::OptionalField { .. }
            | ExprKind::GlobalGet { .. }
            | ExprKind::Call { .. }
            | ExprKind::Method { .. }
            | ExprKind::OptionalMethod { .. }
            | ExprKind::ClosureCall { .. }
    )
}

/// Whether an interval holds `0`.
const fn contains_zero(range: (i128, i128)) -> bool {
    range.0 <= 0 && range.1 >= 0
}

/// `value` as a whole number, when it is one and not `-0`.
fn whole_number(value: f64) -> Option<i128> {
    if !value.is_finite()
        || value.fract() != 0.0_f64
        || (value == 0.0_f64 && value.is_sign_negative())
        || value.abs() > 9_007_199_254_740_992.0_f64
    {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "value is integral and within 2^53, so it converts exactly"
    )]
    Some(value as i128)
}

/// One test known to hold (or fail) where a statement runs.
#[derive(Debug, Clone, Copy)]
struct Guard {
    /// The tested condition.
    cond: ExprId,
    /// Whether the condition is true (`then`) or false (`else`) there.
    sense: bool,
    /// The blocks the test governs, to look for other writes in.
    region: (BlockRegion, Option<BlockRegion>),
    /// Loop nesting depth of the statements the test governs directly. A
    /// store nested in a further loop runs many times per test, so the test
    /// bounds its own write only at exactly this depth.
    depth: usize,
}

/// A block subtree a guard governs.
#[derive(Debug, Clone, Copy)]
struct BlockRegion(smelt_hir::BlockId);

impl ModuleBuilder<'_> {
    /// Whether `reference` names Bun's `int32`.
    ///
    /// The test is the name's ORIGIN, not its spelling: the local name must
    /// be imported from `"bun"` as its `int32` export (under any alias), or be
    /// `int32` read through a namespace import of `"bun"`. A module's own
    /// `type int32 = number` is not imported, so it stays a `number`.
    pub(in crate::lowering) fn is_bun_int32_reference(&self, reference: &TSTypeReference<'_>) -> bool {
        if reference.type_arguments.is_some() {
            return false;
        }
        match &reference.type_name {
            TSTypeName::IdentifierReference(name) => {
                let local = name.name.as_str();
                self.imports.import_source(local) == Some(BUN_MODULE)
                    && self.imports.imported_name(local) == Some(INT32_EXPORT)
            }
            TSTypeName::QualifiedName(qualified) => {
                let TSTypeName::IdentifierReference(namespace) = &qualified.left else {
                    return false;
                };
                let namespace = namespace.name.as_str();
                qualified.right.name.as_str() == INT32_EXPORT
                    && self.imports.is_namespace(namespace)
                    && self.imports.import_source(namespace) == Some(BUN_MODULE)
            }
            TSTypeName::ThisExpression(_) => false,
        }
    }

    /// Whether a type annotation is exactly Bun's `int32`.
    pub(in crate::lowering) fn is_bun_int32_annotation(&self, ty: &TSType<'_>) -> bool {
        match ty {
            TSType::TSTypeReference(reference) => self.is_bun_int32_reference(reference),
            TSType::TSParenthesizedType(inner) => self.is_bun_int32_annotation(&inner.type_annotation),
            _ => false,
        }
    }

    /// `ty` with every int32 slot type NESTED in it replaced by `number`.
    ///
    /// `keep_top` keeps a top-level `FixedInt(I32)`: that is the "whole type"
    /// position (an aliased `type I = int32` used as a field's type). Only the
    /// shapes that can hold a type are rebuilt; everything else is returned as
    /// is.
    pub(in crate::lowering) fn int32_only_as_whole_type(&mut self, ty: TypeId, keep_top: bool) -> TypeId {
        let int32 = Type::FixedInt(IntWidth::I32);
        if !self.ctx.krate.types.all().contains(&int32) {
            return ty;
        }
        self.strip_nested_int32(ty, keep_top, 0)
    }

    /// Recursive worker of [`Self::int32_only_as_whole_type`].
    fn strip_nested_int32(&mut self, ty: TypeId, keep_top: bool, depth: usize) -> TypeId {
        if depth > 32 {
            return ty;
        }
        let Some(shape) = self.ctx.krate.types.get(ty).cloned() else {
            return ty;
        };
        let nested = |builder: &mut Self, inner: TypeId| builder.strip_nested_int32(inner, false, depth + 1);
        let rebuilt = match shape {
            Type::FixedInt(IntWidth::I32) if keep_top => return ty,
            Type::FixedInt(IntWidth::I32) => Type::Float,
            Type::List(item) => Type::List(nested(self, item)),
            Type::Set(item) => Type::Set(nested(self, item)),
            Type::Optional(item) => Type::Optional(nested(self, item)),
            Type::Future(item) => Type::Future(nested(self, item)),
            Type::Dict(key, value) => Type::Dict(nested(self, key), nested(self, value)),
            Type::JsMap(key, value) => Type::JsMap(nested(self, key), nested(self, value)),
            Type::Tuple(items) => Type::Tuple(items.into_iter().map(|item| nested(self, item)).collect()),
            Type::Union(items) => Type::Union(items.into_iter().map(|item| nested(self, item)).collect()),
            Type::Class { name, args } => Type::Class {
                name,
                args: args.into_iter().map(|arg| nested(self, arg)).collect(),
            },
            Type::Function(mut function) => {
                function.params = function
                    .params
                    .into_iter()
                    .map(|param| nested(self, param))
                    .collect();
                function.return_ty = nested(self, function.return_ty);
                Type::Function(function)
            }
            _ => return ty,
        };
        self.ctx.krate.types.intern(rebuilt)
    }

    /// Converts a read of an int32 slot into the `number` it denotes.
    ///
    /// Called on every lowered expression: an expression typed as an int32 slot
    /// (a variable, field, parameter or call result) becomes an exact `ToFloat`
    /// conversion of it, so every consumer sees an ordinary `number`. Anything
    /// else is returned unchanged.
    pub(in crate::lowering) fn int32_read_as_number(&mut self, expr: ExprId, body: &mut Body) -> ExprId {
        let ty = body.exprs[expr.0 as usize].ty;
        let Some(optional) = int32_slot(&self.ctx.krate.types, ty) else {
            return expr;
        };
        let float = self.ctx.krate.types.intern(Type::Float);
        let number = if optional {
            self.ctx.krate.types.intern(Type::Optional(float))
        } else {
            float
        };
        let span = body.exprs[expr.0 as usize].span;
        body.push_expr(Expr {
            kind: ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToFloat,
                operand: expr,
            },
            ty: number,
            span,
        })
    }

    /// Lowers JavaScript `~a`.
    ///
    /// `~a` is `ToInt32(a)` with every bit flipped, which is exactly
    /// `a ^ -1`: the existing exact `ToInt32` bitwise lowering then applies,
    /// and the result is a `number` (an int32, as Bun's table proves).
    pub(in crate::lowering) fn bitwise_not_expression(
        &mut self,
        unary: &oxc::ast::ast::UnaryExpression<'_>,
        body: &mut Body,
    ) -> Result<ExprId, SmeltError> {
        let operand = self.expression(&unary.argument, body)?;
        let float = self.ctx.krate.types.intern(Type::Float);
        let span = self.span(unary.span.start, unary.span.end);
        let all_ones = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Float(-1.0)),
            ty: float,
            span,
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: BinOp::BitXor,
                lhs: operand,
                rhs: all_ones,
            },
            ty: float,
            span,
        }))
    }

    /// TS100034 for a class member whose int32-ness differs from the member
    /// of the interface it implements, if that is why the member types differ.
    pub(in crate::lowering) fn int32_member_mismatch(
        &self,
        class: Symbol,
        interface: Symbol,
        member: Symbol,
        class_ty: TypeId,
        interface_ty: TypeId,
        span: Span,
    ) -> Option<SmeltError> {
        let types = &self.ctx.krate.types;
        if int32_slot(types, class_ty).is_some() == int32_slot(types, interface_ty).is_some() {
            return None;
        }
        Some(int32_error(
            DIFFERS_CODE,
            span,
            format!(
                "A '{}' is used as a '{}', and '{}' is an 'int32' in only one of the two. Declare it 'int32' in both, or in neither.",
                self.source_symbol(class),
                self.source_symbol(interface),
                self.source_symbol(member),
            ),
        ))
    }

    /// Lowers `value as int32`: with `--aot` Bun converts between primitives,
    /// so it is `value | 0` (`ToInt32`), a `number`.
    pub(in crate::lowering) fn int32_assertion_expression(
        &mut self,
        value: ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> ExprId {
        let float = self.ctx.krate.types.intern(Type::Float);
        let span = self.span(span.start, span.end);
        let zero = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Float(0.0)),
            ty: float,
            span,
        });
        body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: BinOp::BitOr,
                lhs: value,
                rhs: zero,
            },
            ty: float,
            span,
        })
    }

    /// Proves and converts every int32 store in the bodies lowered for this module.
    ///
    /// Runs once, after the module's items and bodies exist, so every
    /// signature a store needs (a callee's parameters, a method's, a class's
    /// fields) is known. Also reports a function with an int32 parameter used
    /// as a value (TS100033) and a member that is int32 in only one of a class
    /// and the interface or base class it is used as (TS100034).
    pub(in crate::lowering) fn finish_int32_bodies(
        &mut self,
        first_body: usize,
        module_items: &[ItemId],
    ) -> Result<(), SmeltError> {
        if !self
            .ctx
            .krate
            .types
            .all()
            .contains(&Type::FixedInt(IntWidth::I32))
        {
            return Ok(());
        }
        self.check_int32_members_agree(module_items)?;
        let results = self.body_results();
        for index in first_body..self.ctx.krate.bodies.len() {
            let span = self.ctx.krate.bodies[index].blocks[0].span;
            let mut body = std::mem::replace(&mut self.ctx.krate.bodies[index], Body::new(None, span));
            let id = body.id;
            let outcome = self.finish_int32_body(&mut body, results.get(&id));
            self.ctx.krate.bodies[index] = body;
            outcome?;
        }
        Ok(())
    }

    /// Every body's declared result type and display name.
    fn body_results(&self) -> HashMap<BodyId, (TypeId, Option<String>)> {
        let mut results = HashMap::new();
        for item in &self.ctx.krate.items {
            if let Item::Function(function) = item
                && let Some(body) = function.body
            {
                let name = self.source_symbol(function.name);
                results.insert(body, (function.return_ty, Some(name)));
            }
        }
        for body in &self.ctx.krate.bodies {
            for expr in &body.exprs {
                match &expr.kind {
                    ExprKind::Closure(closure) => {
                        results.entry(closure.body).or_insert((closure.return_ty, None));
                    }
                    ExprKind::Lambda {
                        body: lambda,
                        return_ty,
                    } => {
                        results.entry(*lambda).or_insert((*return_ty, None));
                    }
                    _ => {}
                }
            }
        }
        results
    }

    /// The source spelling of a symbol.
    fn source_symbol(&self, symbol: Symbol) -> String {
        self.ctx
            .krate
            .symbols
            .source(symbol)
            .or_else(|| self.ctx.krate.symbols.get(symbol))
            .unwrap_or("<anonymous>")
            .to_owned()
    }

    /// Proves and converts the int32 stores of one body.
    fn finish_int32_body(
        &mut self,
        body: &mut Body,
        result: Option<&(TypeId, Option<String>)>,
    ) -> Result<(), SmeltError> {
        self.check_int32_function_values(body)?;
        self.int32_arithmetic_as_number(body);
        let root = body.root;
        let mut guards = Vec::new();
        self.walk_int32_block(body, root, &mut guards, result, 0)?;
        if let Some((return_ty, name)) = result
            && int32_slot(&self.ctx.krate.types, *return_ty).is_some()
            && let Some(tail) = body.blocks[root.0 as usize].tail
        {
            let converted = self.int32_store(
                body,
                tail,
                *return_ty,
                &Int32Place::Result(name.clone()),
                &HashMap::new(),
            )?;
            body.blocks[root.0 as usize].tail = Some(converted);
        }
        self.convert_int32_expression_stores(body)?;
        self.wrap_unconverted_int32_reads(body);
        Ok(())
    }

    /// Walks a block's statements, converting statement-level stores.
    ///
    /// `guards` holds the tests that dominate the block, innermost last; a
    /// store `i = i + 1` reads `i`'s range from them.
    fn walk_int32_block(
        &mut self,
        body: &mut Body,
        block: smelt_hir::BlockId,
        guards: &mut Vec<Guard>,
        result: Option<&(TypeId, Option<String>)>,
        depth: usize,
    ) -> Result<(), SmeltError> {
        let statements = body.blocks[block.0 as usize].stmts.clone();
        for statement in statements {
            let stmt = body.stmts[statement.0 as usize].clone();
            match stmt {
                Stmt::Let {
                    pat,
                    value: Some(value),
                    ..
                } => {
                    if let Pattern::Binding(local) = body.patterns[pat.0 as usize]
                        && let Some(decl) = body.locals.get(local.0 as usize)
                    {
                        self.check_int32_record_flow(body, value, decl.ty)?;
                    }
                    if let Pattern::Binding(local) = body.patterns[pat.0 as usize]
                        && let Some(decl) = body.locals.get(local.0 as usize)
                        && int32_slot(&self.ctx.krate.types, decl.ty).is_some()
                    {
                        let ty = decl.ty;
                        let name = decl.name.map_or_else(|| "it".to_owned(), |name| self.source_symbol(name));
                        let refined = self.refinements(body, guards, statement, depth);
                        let converted =
                            self.int32_store(body, value, ty, &Int32Place::Variable(name), &refined)?;
                        if let Stmt::Let { value, .. } = &mut body.stmts[statement.0 as usize] {
                            *value = Some(converted);
                        }
                    }
                }
                Stmt::Assign { target, value } => {
                    let target = Self::int32_unwrapped_target(body, target);
                    if let Stmt::Assign { target: slot, .. } = &mut body.stmts[statement.0 as usize] {
                        *slot = target;
                    }
                    let target_ty = body.exprs[target.0 as usize].ty;
                    self.check_int32_record_flow(body, value, target_ty)?;
                    if int32_slot(&self.ctx.krate.types, target_ty).is_some() {
                        let place = self.int32_target_place(body, target);
                        let refined = self.refinements(body, guards, statement, depth);
                        let converted = self.int32_store(body, value, target_ty, &place, &refined)?;
                        if let Stmt::Assign { value, .. } = &mut body.stmts[statement.0 as usize] {
                            *value = converted;
                        }
                    }
                }
                Stmt::Return(Some(value)) => {
                    if let Some((return_ty, _)) = result {
                        self.check_int32_record_flow(body, value, *return_ty)?;
                    }
                    if let Some((return_ty, name)) = result
                        && int32_slot(&self.ctx.krate.types, *return_ty).is_some()
                    {
                        let refined = self.refinements(body, guards, statement, depth);
                        let converted = self.int32_store(
                            body,
                            value,
                            *return_ty,
                            &Int32Place::Result(name.clone()),
                            &refined,
                        )?;
                        body.stmts[statement.0 as usize] = Stmt::Return(Some(converted));
                    }
                }
                Stmt::If {
                    cond,
                    then_block,
                    else_block,
                } => {
                    guards.push(Guard {
                        cond,
                        sense: true,
                        region: (BlockRegion(then_block), None),
                        depth,
                    });
                    self.walk_int32_block(body, then_block, guards, result, depth)?;
                    guards.pop();
                    if let Some(else_block) = else_block {
                        guards.push(Guard {
                            cond,
                            sense: false,
                            region: (BlockRegion(else_block), None),
                            depth,
                        });
                        self.walk_int32_block(body, else_block, guards, result, depth)?;
                        guards.pop();
                    }
                }
                Stmt::While { cond, body: inner } => {
                    guards.push(Guard {
                        cond,
                        sense: true,
                        region: (BlockRegion(inner), None),
                        depth: depth + 1,
                    });
                    self.walk_int32_block(body, inner, guards, result, depth + 1)?;
                    guards.pop();
                }
                Stmt::WhileUpdate {
                    cond,
                    body: inner,
                    update_target,
                    update_value,
                } => {
                    guards.push(Guard {
                        cond,
                        sense: true,
                        region: (BlockRegion(inner), None),
                        depth: depth + 1,
                    });
                    self.walk_int32_block(body, inner, guards, result, depth + 1)?;
                    guards.pop();
                    let target = Self::int32_unwrapped_target(body, update_target);
                    if let Stmt::WhileUpdate { update_target, .. } = &mut body.stmts[statement.0 as usize] {
                        *update_target = target;
                    }
                    let target_ty = body.exprs[target.0 as usize].ty;
                    if int32_slot(&self.ctx.krate.types, target_ty).is_some() {
                        let place = self.int32_target_place(body, target);
                        // The update runs after the body: the test bounds the
                        // counter there only when the body never writes it.
                        let mut refined = HashMap::new();
                        if let ExprKind::Local(local) = body.exprs[target.0 as usize].kind
                            && Self::assignments_to(body, inner, local) == 0
                            && !Self::captured_in(body, local)
                        {
                            self.refine_from(body, cond, true, &mut refined);
                        }
                        let converted = self.int32_store(body, update_value, target_ty, &place, &refined)?;
                        if let Stmt::WhileUpdate { update_value, .. } = &mut body.stmts[statement.0 as usize] {
                            *update_value = converted;
                        }
                    }
                }
                Stmt::WhileUpdateBlock {
                    cond,
                    body: inner,
                    update,
                } => {
                    // The body runs between the test and the update, so a
                    // write in the update does not invalidate the test there;
                    // the update itself sees the test only if the body never
                    // writes the counter.
                    guards.push(Guard {
                        cond,
                        sense: true,
                        region: (BlockRegion(inner), None),
                        depth: depth + 1,
                    });
                    self.walk_int32_block(body, inner, guards, result, depth + 1)?;
                    guards.pop();
                    guards.push(Guard {
                        cond,
                        sense: true,
                        region: (BlockRegion(inner), Some(BlockRegion(update))),
                        depth: depth + 1,
                    });
                    self.walk_int32_block(body, update, guards, result, depth + 1)?;
                    guards.pop();
                }
                Stmt::For { pat, body: inner, .. } => {
                    if let Pattern::Binding(local) = body.patterns[pat.0 as usize]
                        && let Some(decl) = body.locals.get(local.0 as usize)
                        && int32_slot(&self.ctx.krate.types, decl.ty).is_some()
                    {
                        let name = decl.name.map_or_else(|| "it".to_owned(), |name| self.source_symbol(name));
                        return Err(int32_error(
                            UNPROVEN_CODE,
                            decl.span,
                            format!(
                                "'{name}' is an 'int32', and the compiler cannot prove that what is assigned to '{name}' is one: the compiler does not follow a value through destructuring, the head of a loop or an assertion on the target. Assign it to a 'number' first, and store that with '| 0'."
                            ),
                        ));
                    }
                    self.walk_int32_block(body, inner, guards, result, depth + 1)?;
                }
                Stmt::Match { arms, default, .. } => {
                    for arm in arms {
                        self.walk_int32_block(body, arm.body, guards, result, depth)?;
                    }
                    if let Some(default) = default {
                        self.walk_int32_block(body, default, guards, result, depth)?;
                    }
                }
                Stmt::TryCatch {
                    body: inner,
                    catch_body,
                    finally_body,
                    ..
                } => {
                    self.walk_int32_block(body, inner, guards, result, depth)?;
                    for block in [catch_body, finally_body].into_iter().flatten() {
                        self.walk_int32_block(body, block, guards, result, depth)?;
                    }
                }
                Stmt::Let { value: None, .. }
                | Stmt::Return(None)
                | Stmt::Expr(_)
                | Stmt::Throw(_)
                | Stmt::Break
                | Stmt::Continue => {}
            }
        }
        Ok(())
    }

    /// The raw slot behind an assignment target that expression lowering
    /// already converted to a `number` read.
    fn int32_unwrapped_target(body: &Body, target: ExprId) -> ExprId {
        match &body.exprs[target.0 as usize].kind {
            ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToFloat,
                operand,
            } if matches!(
                body.exprs[operand.0 as usize].kind,
                ExprKind::Local(_) | ExprKind::Field { .. } | ExprKind::GlobalGet { .. }
            ) =>
            {
                *operand
            }
            _ => target,
        }
    }

    /// Checks a value flowing into a slot whose type is a class or interface
    /// with int32 members.
    ///
    /// An object literal is projected into the record member by member, so
    /// each property stored into an int32 member is proven like any other
    /// store. Any other value of a different type would carry members the
    /// compiler never saw stored (TS100034).
    fn check_int32_record_flow(
        &self,
        body: &Body,
        value: ExprId,
        slot: TypeId,
    ) -> Result<(), SmeltError> {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(slot) else {
            return Ok(());
        };
        let value_ty = body.exprs[value.0 as usize].ty;
        if value_ty == slot {
            return Ok(());
        }
        let fields = self.record_fields(*name);
        let int32_fields: Vec<&(String, TypeId)> = fields
            .iter()
            .filter(|(_, ty)| int32_slot(&self.ctx.krate.types, *ty).is_some())
            .collect();
        if int32_fields.is_empty() {
            return Ok(());
        }
        let container = self.source_symbol(*name);
        let ExprKind::DictLit(entries) = &body.exprs[value.0 as usize].kind else {
            let shown = match self.ctx.krate.types.get(value_ty) {
                Some(Type::Class { name, .. }) => self.source_symbol(*name),
                _ => "value of another type".to_owned(),
            };
            return Err(int32_error(
                DIFFERS_CODE,
                body.exprs[value.0 as usize].span,
                format!(
                    "A '{shown}' is used as a '{container}', and '{}' is an 'int32' in only one of the two. Declare it 'int32' in both, or in neither.",
                    int32_fields[0].0
                ),
            ));
        };
        let none = HashMap::new();
        for (key, entry) in entries {
            let ExprKind::Literal(Literal::String(key_text)) = &body.exprs[key.0 as usize].kind else {
                continue;
            };
            if !int32_fields.iter().any(|(field, _)| field == key_text) {
                continue;
            }
            if matches!(self.ctx.krate.types.get(body.exprs[entry.0 as usize].ty), Some(Type::None)) {
                continue;
            }
            let place = Int32Place::Property {
                name: key_text.clone(),
                container: Some(container.clone()),
            };
            self.prove_int32(body, *entry, &place, &none)?;
        }
        Ok(())
    }

    /// The diagnostic subject for an assignment target.
    fn int32_target_place(&self, body: &Body, target: ExprId) -> Int32Place {
        match &body.exprs[target.0 as usize].kind {
            ExprKind::Local(local) => Int32Place::Variable(
                body.locals
                    .get(local.0 as usize)
                    .and_then(|decl| decl.name)
                    .map_or_else(|| "it".to_owned(), |name| self.source_symbol(name)),
            ),
            ExprKind::Field { receiver, field } => Int32Place::Property {
                name: self.source_symbol(*field),
                container: match self.ctx.krate.types.get(body.exprs[receiver.0 as usize].ty) {
                    Some(Type::Class { name, .. }) => Some(self.source_symbol(*name)),
                    _ => None,
                },
            },
            _ => Int32Place::Variable("it".to_owned()),
        }
    }

    /// The ranges the active guards give to locals at `statement`.
    ///
    /// A guard narrows a local only when nothing inside the guarded region
    /// writes it except `statement` itself, and no closure captures it: then
    /// the local still holds the value the test saw.
    fn refinements(
        &self,
        body: &Body,
        guards: &[Guard],
        statement: smelt_hir::StmtId,
        depth: usize,
    ) -> HashMap<LocalId, (i128, i128)> {
        let mut refined = HashMap::new();
        for guard in guards {
            let mut candidate = HashMap::new();
            self.refine_from(body, guard.cond, guard.sense, &mut candidate);
            for (local, range) in candidate {
                let own_write = matches!(
                    &body.stmts[statement.0 as usize],
                    Stmt::Assign { target, .. }
                        if matches!(body.exprs[Self::int32_unwrapped_target(body, *target).0 as usize].kind, ExprKind::Local(l) if l == local)
                );
                let mut writes = Self::assignments_to(body, (guard.region.0).0, local);
                if let Some(BlockRegion(update)) = guard.region.1 {
                    writes += Self::assignments_to(body, update, local);
                }
                let allowed = usize::from(own_write && guard.depth == depth);
                if writes <= allowed && !Self::captured_in(body, local) {
                    refined
                        .entry(local)
                        .and_modify(|current: &mut (i128, i128)| {
                            current.0 = current.0.max(range.0);
                            current.1 = current.1.min(range.1);
                        })
                        .or_insert(range);
                }
            }
        }
        refined
    }

    /// Ranges a test `cond == sense` gives to the int32 locals it compares.
    ///
    /// Understands `<`, `<=`, `>`, `>=` between an int32 local and an
    /// expression with a proven range, and `&&` when `sense` is true.
    fn refine_from(
        &self,
        body: &Body,
        cond: ExprId,
        sense: bool,
        refined: &mut HashMap<LocalId, (i128, i128)>,
    ) {
        let ExprKind::BinOp { op, lhs, rhs } = body.exprs[cond.0 as usize].kind else {
            if let ExprKind::UnaryOp {
                op: UnaryOp::Not,
                operand,
            } = body.exprs[cond.0 as usize].kind
            {
                self.refine_from(body, operand, !sense, refined);
            }
            return;
        };
        if op == BinOp::And && sense {
            self.refine_from(body, lhs, true, refined);
            self.refine_from(body, rhs, true, refined);
            return;
        }
        let op = if sense {
            op
        } else {
            match op {
                BinOp::Lt => BinOp::Gte,
                BinOp::Lte => BinOp::Gt,
                BinOp::Gt => BinOp::Lte,
                BinOp::Gte => BinOp::Lt,
                _ => return,
            }
        };
        let empty = HashMap::new();
        let prover = Int32Prover {
            body,
            types: &self.ctx.krate.types,
            refined: &empty,
        };
        let local_of = |side: ExprId| match &body.exprs[side.0 as usize].kind {
            ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToFloat,
                operand,
            } => match body.exprs[operand.0 as usize].kind {
                ExprKind::Local(local)
                    if int32_slot(&self.ctx.krate.types, body.exprs[operand.0 as usize].ty)
                        == Some(false) =>
                {
                    Some(local)
                }
                _ => None,
            },
            _ => None,
        };
        let mut narrow = |local: LocalId, lo: i128, hi: i128| {
            let entry = refined.entry(local).or_insert((INT32_MIN, INT32_MAX));
            entry.0 = entry.0.max(lo);
            entry.1 = entry.1.min(hi);
        };
        if let (Some(local), Ok(bound)) = (local_of(lhs), prover.interval(rhs)) {
            match op {
                BinOp::Lt => narrow(local, INT32_MIN, bound.1 - 1),
                BinOp::Lte => narrow(local, INT32_MIN, bound.1),
                BinOp::Gt => narrow(local, bound.0 + 1, INT32_MAX),
                BinOp::Gte => narrow(local, bound.0, INT32_MAX),
                _ => {}
            }
        }
        if let (Some(local), Ok(bound)) = (local_of(rhs), prover.interval(lhs)) {
            match op {
                BinOp::Gt => narrow(local, INT32_MIN, bound.1 - 1),
                BinOp::Gte => narrow(local, INT32_MIN, bound.1),
                BinOp::Lt => narrow(local, bound.0 + 1, INT32_MAX),
                BinOp::Lte => narrow(local, bound.0, INT32_MAX),
                _ => {}
            }
        }
    }

    /// Number of statements in `block` (recursively) that assign `local`.
    fn assignments_to(body: &Body, block: smelt_hir::BlockId, local: LocalId) -> usize {
        let mut count = 0;
        let mut pending = vec![block];
        let mut seen = HashSet::new();
        while let Some(block) = pending.pop() {
            if !seen.insert(block) {
                continue;
            }
            for statement in &body.blocks[block.0 as usize].stmts {
                match &body.stmts[statement.0 as usize] {
                    Stmt::Assign { target, .. } => {
                        if matches!(body.exprs[Self::int32_unwrapped_target(body, *target).0 as usize].kind, ExprKind::Local(l) if l == local)
                        {
                            count += 1;
                        }
                    }
                    Stmt::WhileUpdate {
                        body: inner,
                        update_target,
                        ..
                    } => {
                        if matches!(body.exprs[Self::int32_unwrapped_target(body, *update_target).0 as usize].kind, ExprKind::Local(l) if l == local)
                        {
                            count += 1;
                        }
                        pending.push(*inner);
                    }
                    Stmt::If {
                        then_block,
                        else_block,
                        ..
                    } => {
                        pending.push(*then_block);
                        pending.extend(*else_block);
                    }
                    Stmt::While { body: inner, .. } | Stmt::For { body: inner, .. } => pending.push(*inner),
                    Stmt::WhileUpdateBlock {
                        body: inner,
                        update,
                        ..
                    } => {
                        pending.push(*inner);
                        pending.push(*update);
                    }
                    Stmt::Match { arms, default, .. } => {
                        pending.extend(arms.iter().map(|arm| arm.body));
                        pending.extend(*default);
                    }
                    Stmt::TryCatch {
                        body: inner,
                        catch_body,
                        finally_body,
                        ..
                    } => {
                        pending.push(*inner);
                        pending.extend(*catch_body);
                        pending.extend(*finally_body);
                    }
                    Stmt::Let { .. }
                    | Stmt::Expr(_)
                    | Stmt::Return(_)
                    | Stmt::Throw(_)
                    | Stmt::Break
                    | Stmt::Continue => {}
                }
            }
        }
        count
    }

    /// Whether a closure in `body` captures `local` (and could write it unseen).
    fn captured_in(body: &Body, local: LocalId) -> bool {
        body.exprs.iter().any(|expr| match &expr.kind {
            ExprKind::Closure(closure) => closure
                .captures
                .iter()
                .any(|capture| capture.source_local == local),
            _ => false,
        })
    }

    /// Proves `value` an int32 and returns the exact conversion stored into a slot of type `slot`.
    fn int32_store(
        &mut self,
        body: &mut Body,
        value: ExprId,
        _slot: TypeId,
        place: &Int32Place,
        refined: &HashMap<LocalId, (i128, i128)>,
    ) -> Result<ExprId, SmeltError> {
        let value_ty = body.exprs[value.0 as usize].ty;
        // An absent value stored into an optional slot is not a number at all.
        if matches!(self.ctx.krate.types.get(value_ty), Some(Type::None)) {
            return Ok(value);
        }
        // Already converted (a store reached twice through a shared expression).
        if matches!(
            body.exprs[value.0 as usize].kind,
            ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToInt32,
                ..
            }
        ) {
            return Ok(value);
        }
        // A raw read of an int32 slot (built by a desugaring that bypassed
        // expression lowering) is read as a `number` first, like every read.
        let value = if int32_slot(&self.ctx.krate.types, value_ty).is_some() {
            self.int32_read_as_number(value, body)
        } else {
            value
        };
        let value_ty = body.exprs[value.0 as usize].ty;
        let optional_value = matches!(self.ctx.krate.types.get(value_ty), Some(Type::Optional(_)));
        let proven_value = match &body.exprs[value.0 as usize].kind {
            // An optional slot copied from an optional int32 slot.
            ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToFloat,
                operand,
            } if optional_value
                && int32_slot(&self.ctx.krate.types, body.exprs[operand.0 as usize].ty) == Some(true) =>
            {
                true
            }
            _ => false,
        };
        if !proven_value {
            self.prove_int32(body, value, place, refined)?;
        }
        let int32 = self.ctx.krate.types.intern(Type::FixedInt(IntWidth::I32));
        let converted_ty = if optional_value {
            self.ctx.krate.types.intern(Type::Optional(int32))
        } else {
            int32
        };
        let span = body.exprs[value.0 as usize].span;
        Ok(body.push_expr(Expr {
            kind: ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToInt32,
                operand: value,
            },
            ty: converted_ty,
            span,
        }))
    }

    /// Reports TS100031/TS100032 unless `value` is provably an int32.
    fn prove_int32(
        &self,
        body: &Body,
        value: ExprId,
        place: &Int32Place,
        refined: &HashMap<LocalId, (i128, i128)>,
    ) -> Result<(), SmeltError> {
        let prover = Int32Prover {
            body,
            types: &self.ctx.krate.types,
            refined,
        };
        let span = body.exprs[value.0 as usize].span;
        let text = self.int32_quoted_source(span);
        let what = place.describe();
        if let Some(constant) = prover.constant(value) {
            let why = if constant.is_nan() {
                Some("it is NaN")
            } else if constant.is_infinite() || constant.fract() != 0.0_f64 {
                Some("it has a fraction")
            } else if constant == 0.0_f64 && constant.is_sign_negative() {
                Some("it is -0")
            } else if !(-2_147_483_648.0_f64..=2_147_483_647.0_f64).contains(&constant) {
                Some("an int32 is a whole number from -2147483648 to 2147483647")
            } else {
                None
            };
            return why.map_or(Ok(()), |why| {
                Err(int32_error(
                    NOT_AN_INT32_CODE,
                    span,
                    format!(
                        "{what} is an 'int32', and {} is not: {why}.",
                        text.unwrap_or_else(|| "this value".to_owned())
                    ),
                ))
            });
        }
        let reason = match prover.interval(value) {
            Ok((lo, hi)) if lo >= INT32_MIN && hi <= INT32_MAX => return Ok(()),
            Ok((lo, hi)) => Unproven::Range(lo, hi),
            Err(reason) => reason,
        };
        let fix = self.int32_fix(body, value, text.as_deref());
        Err(int32_error(
            UNPROVEN_CODE,
            span,
            format!(
                "{what} is an 'int32', and the compiler cannot prove that {} is one: {}. {fix}",
                text.unwrap_or_else(|| "this value".to_owned()),
                reason.describe()
            ),
        ))
    }

    /// The quoted source text of a span, when it is short and on one line.
    fn int32_quoted_source(&self, span: Span) -> Option<String> {
        let text = self.source.get(span.start as usize..span.end as usize)?;
        (text.len() <= MAX_QUOTED_LENGTH && !text.contains('\n')).then(|| format!("'{text}'"))
    }

    /// Bun's suggested rewrite for an unproven store.
    fn int32_fix(&self, body: &Body, value: ExprId, quoted: Option<&str>) -> String {
        let Some(quoted) = quoted else {
            return "Convert it with '| 0'.".to_owned();
        };
        let text = &quoted[1..quoted.len() - 1];
        // `x++`, `x--`, `x += e`: the value's span is the whole update.
        for (operator, sign) in [("++", "+"), ("--", "-")] {
            if let Some(target) = text.strip_suffix(operator).or_else(|| text.strip_prefix(operator)) {
                let target = target.trim();
                return format!("Write '{target} = ({target} {sign} 1) | 0'.");
            }
        }
        for sign in ["+", "-", "/", "%", "**", ">>>"] {
            if let Some((target, operand)) = text.split_once(&format!(" {sign}= ")) {
                return format!("Write '{target} = ({target} {sign} {operand}) | 0'.");
            }
        }
        if let Some((target, operand)) = text.split_once(" *= ") {
            return format!("Write '{target} = Math.imul({target}, {operand})'.");
        }
        match &body.exprs[value.0 as usize].kind {
            ExprKind::BinOp {
                op: BinOp::Mul,
                lhs,
                rhs,
            } => {
                let side = |id: &ExprId| {
                    let span = body.exprs[id.0 as usize].span;
                    self.source.get(span.start as usize..span.end as usize).map(str::to_owned)
                };
                match (side(lhs), side(rhs)) {
                    (Some(lhs), Some(rhs)) => format!("Write 'Math.imul({lhs}, {rhs})'."),
                    _ => format!("Write '({text}) | 0'."),
                }
            }
            ExprKind::BinOp { .. } | ExprKind::Conditional { .. } | ExprKind::Await(_) => {
                format!("Write '({text}) | 0'.")
            }
            _ => format!("Write '{text} | 0'."),
        }
    }

    /// Converts the int32 stores that live inside expressions: arguments,
    /// constructor arguments, method arguments and object-literal properties.
    fn convert_int32_expression_stores(&mut self, body: &mut Body) -> Result<(), SmeltError> {
        let none = HashMap::new();
        for index in 0..body.exprs.len() {
            let kind = body.exprs[index].kind.clone();
            let ty = body.exprs[index].ty;
            match kind {
                ExprKind::Call { callee, args } => {
                    let callee_ty = body.exprs[callee.0 as usize].ty;
                    let Some(Type::Function(function)) = self.ctx.krate.types.get(callee_ty).cloned() else {
                        continue;
                    };
                    let name = match body.exprs[callee.0 as usize].kind {
                        ExprKind::Item(item) => self.item_function(item).map(|f| self.source_symbol(f.name)),
                        _ => None,
                    };
                    let names = match body.exprs[callee.0 as usize].kind {
                        ExprKind::Item(item) => self
                            .item_function(item)
                            .map(|f| f.params.iter().map(|p| self.source_symbol(p.name)).collect::<Vec<_>>())
                            .unwrap_or_default(),
                        _ => Vec::new(),
                    };
                    let converted = self.convert_int32_arguments(body, &args, &function.params, &names, name.as_deref(), &none)?;
                    if let ExprKind::Call { args, .. } = &mut body.exprs[index].kind {
                        *args = converted;
                    }
                }
                ExprKind::Method {
                    receiver,
                    method,
                    args,
                } => {
                    let receiver_ty = body.exprs[receiver.0 as usize].ty;
                    let Some((params, names, name)) = self.method_parameters(receiver_ty, method) else {
                        continue;
                    };
                    let converted = self.convert_int32_arguments(body, &args, &params, &names, Some(name.as_str()), &none)?;
                    if let ExprKind::Method { args, .. } = &mut body.exprs[index].kind {
                        *args = converted;
                    }
                }
                ExprKind::New { class, args } => {
                    let Some(constructor) = self.class_item(class).and_then(|class| class.constructor) else {
                        continue;
                    };
                    let Some(function) = self.item_function(constructor) else {
                        continue;
                    };
                    let params: Vec<TypeId> = function.params.iter().map(|param| param.ty).collect();
                    let names: Vec<String> = function.params.iter().map(|param| self.source_symbol(param.name)).collect();
                    let name = Some(self.source_symbol(class));
                    let converted = self.convert_int32_arguments(body, &args, &params, &names, name.as_deref(), &none)?;
                    if let ExprKind::New { args, .. } = &mut body.exprs[index].kind {
                        *args = converted;
                    }
                }
                ExprKind::DictLit(entries) => {
                    let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(ty).cloned() else {
                        continue;
                    };
                    let fields = self.record_fields(name);
                    if fields.is_empty() {
                        continue;
                    }
                    let container = self.source_symbol(name);
                    let mut converted = entries.clone();
                    for (key, value) in &mut converted {
                        let ExprKind::Literal(Literal::String(key_text)) = &body.exprs[key.0 as usize].kind else {
                            continue;
                        };
                        let Some((_, field_ty)) = fields.iter().find(|(field, _)| field == key_text) else {
                            continue;
                        };
                        if int32_slot(&self.ctx.krate.types, *field_ty).is_some() {
                            let place = Int32Place::Property {
                                name: key_text.clone(),
                                container: Some(container.clone()),
                            };
                            *value = self.int32_store(body, *value, *field_ty, &place, &none)?;
                        }
                    }
                    body.exprs[index].kind = ExprKind::DictLit(converted);
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Converts the arguments passed to int32 parameters.
    fn convert_int32_arguments(
        &mut self,
        body: &mut Body,
        args: &[ExprId],
        params: &[TypeId],
        names: &[String],
        function: Option<&str>,
        refined: &HashMap<LocalId, (i128, i128)>,
    ) -> Result<Vec<ExprId>, SmeltError> {
        let mut converted = args.to_vec();
        for (arg, param) in converted.iter().zip(params) {
            self.check_int32_record_flow(body, *arg, *param)?;
        }
        for (index, (arg, param)) in converted.iter_mut().zip(params).enumerate() {
            if int32_slot(&self.ctx.krate.types, *param).is_none() {
                continue;
            }
            let place = Int32Place::Parameter {
                name: names.get(index).cloned().unwrap_or_else(|| "it".to_owned()),
                function: function.map(str::to_owned),
            };
            *arg = self.int32_store(body, *arg, *param, &place, refined)?;
        }
        Ok(converted)
    }

    /// The function item `item`, when it is one.
    fn item_function(&self, item: ItemId) -> Option<&smelt_hir::Function> {
        match self.ctx.krate.items.get(item.0 as usize)? {
            Item::Function(function) => Some(function),
            _ => None,
        }
    }

    /// The class item named `name`.
    fn class_item(&self, name: Symbol) -> Option<&smelt_hir::Class> {
        self.ctx.krate.items.iter().find_map(|item| match item {
            Item::Class(class) if class.name == name => Some(class),
            _ => None,
        })
    }

    /// The fields (source name, type) of the class or interface named `name`,
    /// including inherited ones.
    fn record_fields(&self, name: Symbol) -> Vec<(String, TypeId)> {
        let mut fields = Vec::new();
        let mut pending = vec![name];
        let mut seen = HashSet::new();
        while let Some(name) = pending.pop() {
            if !seen.insert(name) {
                continue;
            }
            for item in &self.ctx.krate.items {
                match item {
                    Item::Class(class) if class.name == name => {
                        fields.extend(class.fields.iter().map(|field| (self.source_symbol(field.name), field.ty)));
                        pending.extend(class.base);
                    }
                    Item::Interface(interface) if interface.name == name => {
                        fields.extend(interface.fields.iter().map(|field| (self.source_symbol(field.name), field.ty)));
                        pending.extend(interface.extends.iter().map(|parent| parent.parent));
                    }
                    _ => {}
                }
            }
        }
        fields
    }

    /// Parameter types, names and the method's display name of `method` on a receiver of type `receiver`.
    fn method_parameters(&self, receiver: TypeId, method: Symbol) -> Option<(Vec<TypeId>, Vec<String>, String)> {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(receiver) else {
            return None;
        };
        let mut pending = vec![*name];
        let mut seen = HashSet::new();
        while let Some(name) = pending.pop() {
            if !seen.insert(name) {
                continue;
            }
            for item in &self.ctx.krate.items {
                match item {
                    Item::Class(class) if class.name == name => {
                        for id in &class.methods {
                            if let Some(function) = self.item_function(*id)
                                && matches!(function.owner, smelt_hir::FunctionOwner::ClassMethod { method: m, .. } if m == method)
                            {
                                return Some((
                                    function.params.iter().map(|param| param.ty).collect(),
                                    function.params.iter().map(|param| self.source_symbol(param.name)).collect(),
                                    self.source_symbol(method),
                                ));
                            }
                        }
                        pending.extend(class.base);
                    }
                    Item::Interface(interface) if interface.name == name => {
                        if let Some(signature) = interface.methods.iter().find(|signature| signature.name == method) {
                            return Some((
                                signature.params.iter().map(|param| param.ty).collect(),
                                signature.params.iter().map(|param| self.source_symbol(param.name)).collect(),
                                self.source_symbol(method),
                            ));
                        }
                        pending.extend(interface.extends.iter().map(|parent| parent.parent));
                    }
                    _ => {}
                }
            }
        }
        None
    }

    /// Reports TS100033: a function with an int32 parameter used as a value.
    ///
    /// A direct call is the only use the compiler can see every store of; a
    /// reference to the function that is not a call's callee, and a function
    /// expression or arrow with an int32 parameter (which is a value by
    /// construction), are errors.
    fn check_int32_function_values(&self, body: &Body) -> Result<(), SmeltError> {
        let callees: HashSet<ExprId> = body
            .exprs
            .iter()
            .filter_map(|expr| match &expr.kind {
                ExprKind::Call { callee, .. } => Some(*callee),
                _ => None,
            })
            .collect();
        for (index, expr) in body.exprs.iter().enumerate() {
            let (params, function, used_at): (Vec<(String, TypeId)>, Option<String>, Span) = match &expr.kind {
                ExprKind::Item(item) if !callees.contains(&ExprId(u32::try_from(index).unwrap_or(u32::MAX))) => {
                    let Some(function) = self.item_function(*item) else {
                        continue;
                    };
                    (
                        function.params.iter().map(|param| (self.source_symbol(param.name), param.ty)).collect(),
                        Some(self.source_symbol(function.name)),
                        expr.span,
                    )
                }
                ExprKind::Closure(closure) => {
                    let function = closure
                        .function_item
                        .and_then(|item| self.item_function(item))
                        .map(|function| self.source_symbol(function.name));
                    (
                        closure.params.iter().map(|param| (self.source_symbol(param.name), param.ty)).collect(),
                        function,
                        expr.span,
                    )
                }
                _ => continue,
            };
            if let Some((name, _)) = params
                .iter()
                .find(|(_, ty)| int32_slot(&self.ctx.krate.types, *ty).is_some())
            {
                let subject = Int32Place::Parameter {
                    name: name.clone(),
                    function,
                }
                .describe();
                return Err(int32_error(
                    USED_AS_A_VALUE_CODE,
                    used_at,
                    format!(
                        "{subject} is an 'int32', and the function is used as a value here, so the compiler cannot see every call. Call the function directly, or declare the parameter 'number' and convert it inside with '| 0'."
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Reports TS100034 for the classes this module declared.
    ///
    /// A class is used as the interfaces it implements and as its base class:
    /// a store through one declaration reaches the other, so a member must be
    /// an int32 in both or in neither.
    fn check_int32_members_agree(&self, module_items: &[ItemId]) -> Result<(), SmeltError> {
        for item in module_items {
            let Some(Item::Class(class)) = self.ctx.krate.items.get(item.0 as usize) else {
                continue;
            };
            let related: Vec<Symbol> = class
                .implements
                .iter()
                .map(|heritage| heritage.parent)
                .chain(class.base)
                .collect();
            for other in related {
                let other_fields = self.record_fields(other);
                for field in &class.fields {
                    let name = self.source_symbol(field.name);
                    let Some((_, other_ty)) = other_fields.iter().find(|(other, _)| *other == name) else {
                        continue;
                    };
                    let mine = int32_slot(&self.ctx.krate.types, field.ty).is_some();
                    let theirs = int32_slot(&self.ctx.krate.types, *other_ty).is_some();
                    if mine != theirs {
                        return Err(int32_error(
                            DIFFERS_CODE,
                            field.span,
                            format!(
                                "A '{}' is used as a '{}', and '{name}' is an 'int32' in only one of the two. Declare it 'int32' in both, or in neither.",
                                self.source_symbol(class.name),
                                self.source_symbol(other),
                            ),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// Makes arithmetic that a desugaring typed as an int32 slot `number` arithmetic.
    ///
    /// An update such as `i++` is built from the slot's own read and type, so
    /// it can come out as `i + 1` typed int32 with a raw `i32` operand. That
    /// would be `i32` arithmetic, which wraps or panics where JavaScript does
    /// not, and would claim an int32 range nobody proved. Each such operation is
    /// retyped to `number`, and each int32 operand is read through a fresh
    /// `ToFloat` (a fresh id: the raw read may be shared with the assignment
    /// target, which must stay the slot).
    fn int32_arithmetic_as_number(&mut self, body: &mut Body) {
        let float = self.ctx.krate.types.intern(Type::Float);
        for index in 0..body.exprs.len() {
            if int32_slot(&self.ctx.krate.types, body.exprs[index].ty).is_none() {
                continue;
            }
            let operands: Vec<ExprId> = match &body.exprs[index].kind {
                ExprKind::BinOp { lhs, rhs, .. } => vec![*lhs, *rhs],
                ExprKind::UnaryOp { operand, .. } => vec![*operand],
                _ => continue,
            };
            let converted: Vec<ExprId> = operands
                .into_iter()
                .map(|operand| {
                    // A literal the desugaring typed as the slot is a number.
                    if matches!(body.exprs[operand.0 as usize].kind, ExprKind::Literal(_)) {
                        body.exprs[operand.0 as usize].ty = float;
                        return operand;
                    }
                    self.int32_read_as_number(operand, body)
                })
                .collect();
            match &mut body.exprs[index].kind {
                ExprKind::BinOp { lhs, rhs, .. } => {
                    *lhs = converted[0];
                    *rhs = converted[1];
                }
                ExprKind::UnaryOp { operand, .. } => *operand = converted[0],
                _ => {}
            }
            body.exprs[index].ty = float;
        }
    }

    /// Converts any int32 slot read that expression lowering did not see
    /// (a read built directly by a desugaring) into its `number` value.
    ///
    /// The expression is rewritten in place — its id now holds the `ToFloat`
    /// conversion and the raw read moves to a fresh id — so every parent sees a
    /// `number` without being visited. Assignment targets, conversions' own
    /// operands, and store conversions are left alone.
    fn wrap_unconverted_int32_reads(&mut self, body: &mut Body) {
        let mut keep: HashSet<ExprId> = HashSet::new();
        for stmt in &body.stmts {
            match stmt {
                Stmt::Assign { target, .. } => {
                    keep.insert(*target);
                }
                Stmt::WhileUpdate { update_target, .. } => {
                    keep.insert(*update_target);
                }
                _ => {}
            }
        }
        for (index, expr) in body.exprs.iter().enumerate() {
            if let ExprKind::PrimitiveCast { op, operand } = &expr.kind
                && matches!(op, PrimitiveCastOp::ToFloat | PrimitiveCastOp::ToInt32)
            {
                keep.insert(*operand);
                if *op == PrimitiveCastOp::ToInt32 {
                    keep.insert(ExprId(u32::try_from(index).unwrap_or(u32::MAX)));
                }
            }
        }
        let count = body.exprs.len();
        for index in 0..count {
            let id = ExprId(u32::try_from(index).unwrap_or(u32::MAX));
            if keep.contains(&id) {
                continue;
            }
            let ty = body.exprs[index].ty;
            if int32_slot(&self.ctx.krate.types, ty).is_none() {
                continue;
            }
            let raw = body.exprs[index].clone();
            let span = raw.span;
            let moved = body.push_expr(raw);
            let number = self.int32_read_as_number(moved, body);
            let converted = body.exprs[number.0 as usize].clone();
            body.exprs[index] = Expr { span, ..converted };
        }
    }
}
