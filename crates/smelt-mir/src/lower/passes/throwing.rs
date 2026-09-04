//! Throwing-function reachability propagation.
//!
//! Invariant: every MIR function or closure that can reach an uncaught throw,
//! an unwound async operation, or a throwing call is marked `can_throw`.

use smelt_hir::Type;

use crate::{BuiltinFn, Callee, LocalDecl, Mir, Rvalue, Statement, Terminator};

use super::super::{local_index, usize_from_u32};
use super::operand_local;

/// Marks functions that can reach an uncaught throw directly or through calls.
pub(in crate::lower) fn propagate_throwing_functions(mir: &mut Mir) {
    loop {
        let types = mir.types.clone();
        let throwing = mir
            .functions
            .iter()
            .map(|function| function.can_throw && !function.is_generator)
            .collect::<Vec<_>>();
        let mut changed = false;

        for function in &mut mir.functions {
            if function.can_throw {
                continue;
            }
            let can_throw = function.blocks.iter().any(|block| {
                block
                    .statements
                    .iter()
                    .any(|statement| statement_can_throw(statement, &function.locals, &types))
                    || block
                        .terminator
                        .as_ref()
                        .is_some_and(|terminator| terminator_can_throw(terminator, &throwing))
            });
            if can_throw {
                function.can_throw = true;
                changed = true;
            }
        }

        for closure in &mut mir.closures {
            if closure.can_throw {
                continue;
            }
            let can_throw = closure.blocks.iter().any(|block| {
                block
                    .statements
                    .iter()
                    .any(|statement| statement_can_throw(statement, &closure.locals, &types))
                    || block
                        .terminator
                        .as_ref()
                        .is_some_and(|terminator| terminator_can_throw(terminator, &throwing))
            });
            if can_throw {
                closure.can_throw = true;
                changed = true;
            }
        }

        if !changed {
            break;
        }
    }
}

/// Returns whether a MIR statement can throw through an expression-level call.
fn statement_can_throw(
    statement: &Statement,
    locals: &[LocalDecl],
    types: &smelt_hir::TypeInterner,
) -> bool {
    let Statement::Assign {
        value: Rvalue::ClosureCall { callee, .. },
        ..
    } = statement
    else {
        return false;
    };
    let Some(local) = operand_local(callee) else {
        return true;
    };
    local_index(local)
        .and_then(|index| locals.get(index))
        .and_then(|decl| types.get(decl.ty))
        .is_some_and(|ty| matches!(ty, Type::Function(function) if function.may_throw))
}

/// Returns whether a terminator can leave through an uncaught exception path.
fn terminator_can_throw(terminator: &Terminator, throwing: &[bool]) -> bool {
    match terminator {
        Terminator::Throw(_) => true,
        Terminator::Call {
            callee: Callee::Static(func),
            unwind: None,
            ..
        } => usize_from_u32(func.0, "MIR function index does not fit in usize")
            .is_ok_and(|index| throwing.get(index).copied().unwrap_or(false)),
        Terminator::Call {
            callee: Callee::Indirect(_),
            unwind: None,
            ..
        } => true,
        Terminator::Await { unwind: None, .. } => true,
        // A fallible builtin with no handler in scope leaves the function
        // through its error channel, so the enclosing function throws.
        Terminator::Call {
            callee: Callee::Builtin(BuiltinFn::JsonParse),
            unwind: None,
            ..
        } => true,
        Terminator::Goto(_)
        | Terminator::Call { .. }
        | Terminator::Await { .. }
        | Terminator::Switch { .. }
        | Terminator::Match { .. }
        | Terminator::Return(_)
        | Terminator::Unreachable => false,
    }
}
