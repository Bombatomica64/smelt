//! Equality between a function and a record built from it
//! ([`crate::function_identity`], design decision C1).
//!
//! `const n: Named = hello; n === hello` is `true` in JavaScript: the record is
//! the function. A record carrying the function-identity token answers for
//! the function it was built from, so every equality operator (`===`, `==`,
//! `Object.is` and their negations; on objects they all compare references)
//! between such a record and a function, or between two such records,
//! compares identities:
//!
//! * a function's identity is its key (`function_identity_key_text`);
//! * a token record's identity is its token's key, else (a record no function
//!   built) the record's own: its cell address for a reference record, none
//!   for a by-value record, which then equals nothing but is never compared
//!   by identity otherwise either.
//!
//! With the identity registry present (`Need::Unknown`), function keys go
//! through `smelt_function_identity_of`, so an adapter of the function
//! (`smelt_link_function_identity`) is the same function here too.

use super::binary_ops::FunctionKeyForm;
use super::*;
use crate::rust::RustExpr;

impl FunctionEmitter<'_> {
    /// `lhs op rhs` by function identity when one side is a record carrying
    /// the function-identity token and the other is a function value or
    /// another such record. `None` for every other pair.
    pub(super) fn function_token_equality_expr(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
    ) -> Result<Option<RustExpr>, EmitError> {
        if !op.is_equality() {
            return Ok(None);
        }
        let lhs_ty = self.operand_ty(lhs)?;
        let rhs_ty = self.operand_ty(rhs)?;
        let (lhs_record, rhs_record) = (self.token_record(lhs_ty), self.token_record(rhs_ty));
        let is_function = |ty: TypeId| matches!(self.mir.types.get(ty), Some(Type::Function(_)));
        let applies = match (lhs_record, rhs_record) {
            (Some(_), Some(_)) => true,
            (Some(_), None) => is_function(rhs_ty),
            (None, Some(_)) => is_function(lhs_ty),
            (None, None) => false,
        };
        if !applies {
            return Ok(None);
        }
        let left = self.function_identity_side(lhs, lhs_record)?;
        let right = self.function_identity_side(rhs, rhs_record)?;
        let equal = format!(
            "{{ let (smelt_left, smelt_right): (Option<usize>, Option<usize>) = ({left}, {right}); smelt_left.is_some() && smelt_left == smelt_right }}"
        );
        Ok(Some(if op.is_negated_equality() {
            RustExpr::loose(format!("!{equal}"))
        } else {
            RustExpr::atom(equal)
        }))
    }

    /// The record `ty` names when its values carry the function-identity
    /// token, or `None`.
    fn token_record(&self, ty: TypeId) -> Option<Symbol> {
        match self.mir.types.get(ty) {
            Some(Type::Class { name, .. }) if self.context.carries_function_identity(*name) => {
                Some(*name)
            }
            _ => None,
        }
    }

    /// The identity of `operand` as an `Option<usize>` expression: a
    /// function's key, or a token record's (see the module docs).
    fn function_identity_side(
        &self,
        operand: &Operand,
        record: Option<Symbol>,
    ) -> Result<String, EmitError> {
        let canonical = |key: &str| {
            if self.context.demand().has(Need::Unknown) {
                format!("smelt_function_identity_of({key})")
            } else {
                key.to_owned()
            }
        };
        let field = crate::function_identity::FUNCTION_IDENTITY_FIELD;
        let Some(record_name) = record else {
            let key = self.function_identity_key_text(operand, FunctionKeyForm::Key)?;
            return Ok(format!("Some({})", canonical(&key)));
        };
        let text = self.operand_expr(operand)?;
        let mapped = canonical("smelt_key");
        Ok(if self.context.is_reference_class(record_name) {
            format!(
                "{{ let smelt_record = &{text}.0; Some(smelt_record.borrow().{field}.key().map_or(::std::rc::Rc::as_ptr(smelt_record) as *const () as usize, |smelt_key| {mapped})) }}",
                text = text.tight()
            )
        } else {
            format!(
                "{text}.{field}.key().map(|smelt_key| {mapped})",
                text = text.tight()
            )
        })
    }
}
