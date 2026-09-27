//! Drops receiver binds no function in the program can observe.
//!
//! `this` is a dynamically scoped channel: [`Rvalue::BindThis`] installs a
//! receiver for the duration of one call and [`Rvalue::ThisRead`] reads
//! whatever the innermost active call installed. A bind is therefore observable
//! only through a read — if the whole program contains no [`Rvalue::ThisRead`],
//! installing a receiver cannot change what any function computes, and the bind
//! is a pure cost: it wraps the callable and, more visibly, it is what makes
//! codegen emit the `SMELT_THIS` channel at all.
//!
//! That matters because a receiver is bound at every ordinary method call whose
//! callee is invoked through the erased call ABI (see
//! `bind_member_call_receiver` in `smelt-frontend-ts`) — JavaScript supplies
//! `this` from the CALL, so the frontend cannot know whether the callee reads
//! it. This pass answers that question once the whole program is in MIR: with
//! no reader anywhere, every bind becomes a plain use of its callee and the
//! channel disappears from programs that never mention `this`.

use crate::{Mir, Rvalue, Statement, opt::Pass};

/// Replaces every [`Rvalue::BindThis`] with a use of its callee when the
/// program contains no [`Rvalue::ThisRead`].
#[derive(Debug, Default)]
pub struct UnobservedReceiverBind;

impl Pass for UnobservedReceiverBind {
    fn name(&self) -> &'static str {
        "unobserved-receiver-bind"
    }

    fn run(&self, mir: &mut Mir) -> bool {
        if program_reads_this(mir) {
            return false;
        }
        let mut changed = false;
        for function in &mut mir.functions {
            changed |= forward_bound_callees(function);
        }
        let function_blocks = mir
            .functions
            .iter_mut()
            .flat_map(|function| function.blocks.iter_mut());
        let closure_blocks = mir
            .closures
            .iter_mut()
            .flat_map(|closure| closure.blocks.iter_mut());
        for block in function_blocks.chain(closure_blocks) {
            for statement in &mut block.statements {
                let Some(value) = statement_rvalue_mut(statement) else {
                    continue;
                };
                let Rvalue::BindThis { callee, .. } = value else {
                    continue;
                };
                *value = Rvalue::Use(callee.clone());
                changed = true;
            }
        }
        changed
    }
}

/// Call a dropped bind's callee directly where the bound temporary was only
/// the callee of one call.
///
/// A receiver bind at an ordinary `obj.field(..)` call is
/// `%t = bind_this obj.field, obj` followed by `closure_call %t(..)`. With
/// the bind gone the temporary is a pure copy of the callee, so the call reads
/// the callee in place again and the copy is deleted: the function is exactly
/// what it was before the frontend offered the receiver, rather than carrying
/// a detour through a temporary the program never needed. Only the simple,
/// provably equivalent shape is forwarded -- a compiler temporary assigned
/// once and read once, by a later call in the same block; anything else keeps
/// the plain `Use` the caller installs.
fn forward_bound_callees(function: &mut crate::MirFunction) -> bool {
    use super::local_use::{local_assignment_count, local_is_temp, local_read_count, operand_local};
    // Decide every forwarding first (the use counts read the whole function),
    // then apply them block by block.
    let forwardable = |body: &crate::MirFunction, dest: crate::LocalId| {
        local_is_temp(body, dest)
            && local_assignment_count(body, dest) == 1
            && local_read_count(body, dest) == 1
    };
    let candidates = function
        .blocks
        .iter()
        .flat_map(|block| &block.statements)
        .filter_map(|statement| match statement {
            Statement::Assign {
                dest,
                value: Rvalue::BindThis { .. },
            } => Some(*dest),
            Statement::Assign { .. }
            | Statement::AssignPlace { .. }
            | Statement::DictEntryUpdate { .. }
            | Statement::StorageLive(_)
            | Statement::StorageDead(_) => None,
        })
        .filter(|dest| forwardable(function, *dest))
        .collect::<Vec<_>>();
    let mut changed = false;
    for block in &mut function.blocks {
        for bound in &candidates {
            let Some(bind_at) = block.statements.iter().position(|statement| {
                matches!(statement, Statement::Assign { dest, value: Rvalue::BindThis { .. } } if dest == bound)
            }) else {
                continue;
            };
            let Some(Statement::Assign {
                value: Rvalue::BindThis { callee: bound_callee, .. },
                ..
            }) = block.statements.get(bind_at).cloned()
            else {
                continue;
            };
            let call = block
                .statements
                .iter_mut()
                .skip(bind_at.saturating_add(1))
                .find_map(|statement| match statement {
                    Statement::Assign {
                        value: Rvalue::ClosureCall { callee: call_callee, .. },
                        ..
                    } if operand_local(call_callee) == Some(*bound) => Some(call_callee),
                    Statement::Assign { .. }
                    | Statement::AssignPlace { .. }
                    | Statement::DictEntryUpdate { .. }
                    | Statement::StorageLive(_)
                    | Statement::StorageDead(_) => None,
                });
            let Some(call_callee) = call else {
                continue;
            };
            *call_callee = bound_callee;
            block.statements.remove(bind_at);
            changed = true;
        }
    }
    changed
}

/// Returns the rvalue a statement evaluates, when it evaluates one.
const fn statement_rvalue_mut(statement: &mut Statement) -> Option<&mut Rvalue> {
    match statement {
        Statement::Assign { value, .. }
        | Statement::AssignPlace { value, .. }
        | Statement::DictEntryUpdate { value, .. } => Some(value),
        Statement::StorageLive(_) | Statement::StorageDead(_) => None,
    }
}

/// Returns whether any function or closure in the program reads `this`.
fn program_reads_this(mir: &Mir) -> bool {
    let function_blocks = mir
        .functions
        .iter()
        .flat_map(|function| function.blocks.iter());
    let closure_blocks = mir.closures.iter().flat_map(|closure| closure.blocks.iter());
    function_blocks
        .chain(closure_blocks)
        .flat_map(|block| block.statements.iter())
        .any(|statement| match statement {
            Statement::Assign { value, .. }
            | Statement::AssignPlace { value, .. }
            | Statement::DictEntryUpdate { value, .. } => matches!(value, Rvalue::ThisRead),
            Statement::StorageLive(_) | Statement::StorageDead(_) => false,
        })
}
