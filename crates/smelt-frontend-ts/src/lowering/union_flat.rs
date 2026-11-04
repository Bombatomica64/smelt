//! `Array.prototype.flat` over a list whose element type is a union.
//!
//! `[1, [2, [3]]]` is `(number | (number | number[])[])[]` in TypeScript, and
//! `.flat()` of it is `(number | (number | number[]))[]` = `(number |
//! number[])[]`: TypeScript's `FlatArray<A, D>` unwraps one array level from
//! every union arm per unit of depth and keeps the non-array arms as they are.
//! A hand port computes the same element type. The rule here mirrors
//! `FlatArray`:
//!
//! - depth `0` keeps the element type;
//! - an array arm `T[]` at depth `d > 0` contributes `FlatArray<T, d - 1>`;
//! - any other arm contributes itself;
//! - the arms' results are joined into one union (deduplicated).
//!
//! The depth is the call's argument: none means `1`, a numeric literal means
//! its floor (a negative literal means `0`). Any other depth, `Infinity`
//! included, has type `number` in TypeScript, so like `FlatArray<A, number>`
//! the result is the union over every depth up to the element's nesting.
//!
//! Behaviour requirement: unchanged; the runtime flattening is the same
//! helper an erased list uses (`list_flat_text` in the Rust emitter). Type
//! preservation requirement: the result keeps the element type TypeScript
//! proves instead of erasing the whole value.

use oxc::ast::ast::{Argument, Expression, UnaryOperator};
use smelt_hir::{Type, TypeId};

use super::ModuleBuilder;

/// Deepest nesting the walk unwraps; beyond it the remaining arm is kept.
const MAX_FLAT_DEPTH: u32 = 16;

/// The depth a `flat` call flattens to, as far as its argument fixes it.
#[derive(Clone, Copy)]
enum FlatDepth {
    /// A depth fixed by a literal (capped at [`MAX_FLAT_DEPTH`]).
    Known(u32),
    /// A depth only known at runtime.
    Dynamic,
}

impl ModuleBuilder<'_> {
    /// The element type of `list.flat(depth)` for a list of `element_ty`,
    /// when `element_ty` is a union with at least one array arm; `None`
    /// otherwise (the caller's other rules apply).
    pub(in crate::lowering) fn union_flat_item_type(
        &mut self,
        element_ty: TypeId,
        depth: Option<&Argument<'_>>,
    ) -> Option<TypeId> {
        let Some(Type::Union(arms)) = self.ctx.krate.types.get(element_ty) else {
            return None;
        };
        if !arms.iter().any(|arm| matches!(self.ctx.krate.types.get(*arm), Some(Type::List(_)))) {
            return None;
        }
        let members = match flat_depth(depth) {
            FlatDepth::Known(depth) => self.flat_members(element_ty, depth),
            FlatDepth::Dynamic => {
                let mut members = Vec::new();
                for level in 0..=MAX_FLAT_DEPTH {
                    for member in self.flat_members(element_ty, level) {
                        if !members.contains(&member) {
                            members.push(member);
                        }
                    }
                }
                members
            }
        };
        Some(self.union_of_types_or_unknown(members))
    }

    /// The flat union members `FlatArray<ty, depth>` resolves to.
    fn flat_members(&mut self, ty: TypeId, depth: u32) -> Vec<TypeId> {
        if depth == 0 {
            return self.flatten_union_member_types(ty);
        }
        let mut members = Vec::new();
        for arm in self.flatten_union_member_types(ty) {
            let flattened = match self.ctx.krate.types.get(arm) {
                Some(&Type::List(item_ty)) => self.flat_members(item_ty, depth.saturating_sub(1)),
                _ => vec![arm],
            };
            for member in flattened {
                if !members.contains(&member) {
                    members.push(member);
                }
            }
        }
        members
    }
}

/// Reads the depth argument of a `flat` call.
fn flat_depth(argument: Option<&Argument<'_>>) -> FlatDepth {
    argument.map_or(FlatDepth::Known(1), |argument| {
        argument.as_expression().map_or(FlatDepth::Dynamic, flat_depth_of_expression)
    })
}

/// [`flat_depth`] for the depth expression itself.
fn flat_depth_of_expression(expression: &Expression<'_>) -> FlatDepth {
    match expression {
        Expression::NumericLiteral(literal) => {
            let value = literal.value;
            if value.is_nan() || value <= 0.0 {
                FlatDepth::Known(0)
            } else if value >= f64::from(MAX_FLAT_DEPTH) {
                FlatDepth::Known(MAX_FLAT_DEPTH)
            } else {
                // `value` is in `(0, MAX_FLAT_DEPTH)`, so its floor is one of
                // the depths below the cap.
                (0..MAX_FLAT_DEPTH)
                    .rev()
                    .find(|&depth| f64::from(depth) <= value)
                    .map_or(FlatDepth::Known(0), FlatDepth::Known)
            }
        }
        // `-n` for a numeric literal `n` is negative (or `-0`/`NaN`): depth 0.
        Expression::UnaryExpression(unary)
            if unary.operator == UnaryOperator::UnaryNegation
                && matches!(unary.argument, Expression::NumericLiteral(_)) =>
        {
            FlatDepth::Known(0)
        }
        Expression::ParenthesizedExpression(inner) => flat_depth_of_expression(&inner.expression),
        _ => FlatDepth::Dynamic,
    }
}
