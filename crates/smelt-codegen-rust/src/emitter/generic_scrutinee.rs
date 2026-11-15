//! The erased copy a scalar inspection of a lifted type parameter reads.
//!
//! The emitter half of [`crate::generic_inspection`]: `x == null`,
//! `typeof x`, `!x` and `if (x)` on a value typed as a type parameter the
//! emitted Rust carries as a real generic. A `T` is opaque to Rust, so each
//! question is asked of `x.clone().into_smelt_unknown()`, the conversion the
//! `IntoSmeltUnknown` bound of every lifted parameter supplies.
//!
//! Requirements: the copy is a temporary of the one expression that reads it
//! (never bound, stored or returned), so the original value keeps its type
//! and its identity; and the answer is the one the erased rendering of the
//! same function gives, because the erased function receives exactly this
//! conversion of the caller's argument.

use super::*;
use crate::generic_inspection::{Absent, absent_only_locals, is_equality, operand_absence};
use crate::rust::RustExpr;

impl FunctionEmitter<'_> {
    /// Whether `ty` is a type parameter this scope spells as a Rust generic.
    fn is_lifted_type_param(&self, ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(ty),
            Some(Type::TypeParam { name }) if self.current_function_has_type_param(*name)
        )
    }

    /// The erased copy of `operand` when it reads a value (a local, or a
    /// field or element of one) typed as a type parameter this scope spells
    /// as a Rust generic (a free function's own, or its class's), or an
    /// optional of one;
    /// `None` for every other operand, the erased spelling of a type
    /// parameter included (that one already is the carrier).
    pub(super) fn lifted_param_copy(
        &self,
        operand: &Operand,
    ) -> Result<Option<RustExpr>, EmitError> {
        if !self.operand_is_lifted_value(operand)? {
            return Ok(None);
        }
        Ok(Some(self.erase(operand)?))
    }

    /// Whether `operand` reads a value typed as a lifted type parameter, or
    /// an optional of one.
    ///
    /// Asked of the scope, not of a rendering: rendering an erased type
    /// parameter records its erasure, and this is only a question.
    fn operand_is_lifted_value(&self, operand: &Operand) -> Result<bool, EmitError> {
        if matches!(operand, Operand::Const(_)) {
            return Ok(false);
        }
        let ty = self.operand_ty(operand)?;
        let payload = match self.mir.types.get(ty) {
            Some(Type::Optional(inner)) => *inner,
            _ => ty,
        };
        Ok(self.is_lifted_type_param(payload))
    }

    /// `x == null`, `x === undefined` and their negations where `x` is a
    /// lifted type parameter value ([`Self::lifted_param_copy`]) and the other
    /// side is the absent value: a constant, or a temporary that only ever
    /// holds one.
    ///
    /// A strict test matches the one absent value it names; a loose one
    /// matches both, as the erased rendering does.
    pub(super) fn lifted_param_absence_expr(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
    ) -> Result<Option<RustExpr>, EmitError> {
        if !is_equality(op) {
            return Ok(None);
        }
        // Cheap exit before the body scan below: no side is a lifted value.
        if !self.operand_is_lifted_value(lhs)? && !self.operand_is_lifted_value(rhs)? {
            return Ok(None);
        }
        // Find the absent side first: its temporary is typed `T` too.
        let absent_locals = absent_only_locals(&self.function.locals, &self.function.blocks);
        let (absent, value) = if let Some(absent) = operand_absence(rhs, &absent_locals) {
            (absent, lhs)
        } else if let Some(absent) = operand_absence(lhs, &absent_locals) {
            (absent, rhs)
        } else {
            return Ok(None);
        };
        let Some(copy) = self.lifted_param_copy(value)? else {
            return Ok(None);
        };
        let pattern = match (op, absent) {
            (smelt_hir::BinOp::Eq | smelt_hir::BinOp::NotEq, _) => {
                "SmeltUnknown::Null | SmeltUnknown::Undefined"
            }
            (_, Absent::Null) => "SmeltUnknown::Null",
            (_, Absent::Undefined) => "SmeltUnknown::Undefined",
        };
        let negated = matches!(
            op,
            smelt_hir::BinOp::NotEq
                | smelt_hir::BinOp::StrictNotEq
                | smelt_hir::BinOp::JsStrictNotEq
        );
        let bang = if negated { "!" } else { "" };
        Ok(Some(RustExpr::atom(self.carrier_format(
            self.carrier()?,
            format_args!("{bang}matches!({copy}, {pattern})", copy = copy.delimited()),
        ))))
    }

    /// Whether `dest` is a temporary that exists only to be the absent side
    /// of absence tests of lifted type parameter values
    /// ([`Self::lifted_param_absence_expr`]): typed as the lifted parameter,
    /// defined once by the absent constant, and read by nothing but equality
    /// tests.
    ///
    /// Such a temporary is never read by the emitted Rust (the test matches
    /// the erased copy against the absent tag), and a real `T` cannot hold
    /// the absent value: recovering one from `undefined` panics for a class
    /// instantiation. Its declaration is therefore filled from the `Default`
    /// bound instead ([`Self::absence_probe_placeholder`]).
    pub(super) fn is_absence_probe_temp(&self, dest: LocalId) -> Result<bool, EmitError> {
        let ty = self.local_decl(dest)?.ty;
        if !self.is_lifted_type_param(ty) {
            return Ok(false);
        }
        if !absent_only_locals(&self.function.locals, &self.function.blocks).contains_key(&dest) {
            return Ok(false);
        }
        for block in &self.function.blocks {
            for statement in &block.statements {
                if !statement.reads_local(dest) {
                    continue;
                }
                // Only a test `lifted_param_absence_expr` renders leaves the
                // temporary unread: an equality whose other side has an
                // erased copy.
                let Statement::Assign {
                    value: Rvalue::Binary { op, lhs, rhs },
                    ..
                } = statement
                else {
                    return Ok(false);
                };
                let reads_dest = |operand: &Operand| {
                    matches!(
                        operand,
                        Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))
                            if *local == dest
                    )
                };
                let other = if reads_dest(lhs) { rhs } else { lhs };
                if !is_equality(*op)
                    || reads_dest(other)
                    || self.lifted_param_copy(other)?.is_none()
                {
                    return Ok(false);
                }
            }
            let mut read = false;
            if let Some(terminator) = &block.terminator {
                terminator.reads(|operand| operand.read_locals(&mut |local| read |= local == dest));
            }
            if read {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The value an absence-probe temporary ([`Self::is_absence_probe_temp`])
    /// is declared with: never read, so any `T` does.
    pub(super) fn absence_probe_placeholder() -> &'static str {
        "Default::default()"
    }
}
