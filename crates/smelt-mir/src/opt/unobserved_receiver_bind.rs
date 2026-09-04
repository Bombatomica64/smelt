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
        let function_blocks = mir
            .functions
            .iter_mut()
            .flat_map(|function| function.blocks.iter_mut());
        let closure_blocks = mir
            .closures
            .iter_mut()
            .flat_map(|closure| closure.blocks.iter_mut());
        let mut changed = false;
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
