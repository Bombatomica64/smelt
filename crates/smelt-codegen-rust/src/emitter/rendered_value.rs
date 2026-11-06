//! Typed "rendered value" abstraction for the Rust codegen emitter.
//!
//! A [`RustExpr`] knows how its text binds; a [`RenderedValue`] adds the MIR type
//! it evaluates to, which is what the coercion seam needs. A value crossing the
//! seam carries both, so no verb re-derives the type or guesses at parentheses.

use smelt_hir::TypeId;

use crate::rust::{Formed, Precedence, RustExpr};

/// A rendered Rust expression bundled with its MIR type.
#[derive(Clone, Debug)]
pub(super) struct RenderedValue {
    /// The rendered expression.
    expr: RustExpr,
    /// The MIR type the rendered expression evaluates to.
    ty: TypeId,
}

impl RenderedValue {
    /// `expr`, known to evaluate to `ty`.
    pub(super) fn new(expr: RustExpr, ty: TypeId) -> Self {
        Self { expr, ty }
    }

    /// The MIR type this rendered expression evaluates to.
    pub(super) fn ty(&self) -> TypeId {
        self.ty
    }

    /// The rendered expression.
    pub(super) fn expr(&self) -> &RustExpr {
        &self.expr
    }

    /// The text, safe to drop into a tighter-binding position.
    pub(super) fn parenthesized_if_needed(&self) -> String {
        self.expr.parenthesized_if_needed()
    }
}

impl Formed for RenderedValue {
    fn precedence(&self) -> Precedence {
        self.expr.binding()
    }
}

#[cfg(test)]
mod tests {
    use super::RenderedValue;
    use crate::rust::RustExpr;
    use smelt_hir::TypeId;

    /// A throwaway type id; `RenderedValue` never interprets it in these tests.
    const TY: TypeId = TypeId(0);

    /// A chain is inlined verbatim in every position.
    #[test]
    fn a_chain_is_never_parenthesized() {
        let value = RenderedValue::new(RustExpr::chain("smelt_value.to_lowercase()"), TY);
        assert_eq!(value.parenthesized_if_needed(), "smelt_value.to_lowercase()");
    }

    /// A loose operand must be wrapped before it is dropped into a tighter
    /// position: `SmeltUnknown::Number(a + b as f64)` parses as `a + (b as f64)`.
    #[test]
    fn a_loose_value_is_wrapped_for_tight_contexts() {
        let value = RenderedValue::new(RustExpr::loose("a + b"), TY);
        assert_eq!(value.parenthesized_if_needed(), "(a + b)");
        assert_eq!(value.expr().as_str(), "a + b");
    }

    /// The cast operand shape the `erase` Int/Float arm builds.
    #[test]
    fn cast_operand_wrapping_matches_erase_number_arm() {
        let chain = RenderedValue::new(RustExpr::path("value"), TY);
        assert_eq!(
            format!("SmeltUnknown::Number({} as f64)", chain.parenthesized_if_needed()),
            "SmeltUnknown::Number(value as f64)"
        );
        let loose = RenderedValue::new(RustExpr::loose("a + b"), TY);
        assert_eq!(
            format!("SmeltUnknown::Number({} as f64)", loose.parenthesized_if_needed()),
            "SmeltUnknown::Number((a + b) as f64)"
        );
    }
}
