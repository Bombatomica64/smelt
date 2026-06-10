//! Closure escape analysis for lowered MIR.
//!
//! Invariant: every closure reachable from a returned closure value is marked
//! as escaping, and escaping closures capture by value so Rust codegen does not
//! borrow locals after their owner function returns.

use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;

use crate::{ClosureId, LocalId, Mir, MirFunction, Operand, Rvalue, Statement, Terminator};

use super::operand_local;

/// Mark closures returned from their creating function as escaping.
///
/// This is the MIR-level equivalent of Rust's closure escape analysis for the
/// subset Smelt supports today: closure values may be created into temporaries,
/// moved through local aliases, and returned. Once a closure escapes, its
/// captures are promoted to by-value so Rust emission uses an owning `move`
/// closure instead of borrowing stack locals that are about to disappear.
pub(in crate::lower) fn mark_escaping_closures(mir: &mut Mir) {
    let closure_defs_by_function = mir
        .functions
        .iter()
        .map(closure_definitions)
        .collect::<Vec<_>>();
    let local_rvalues_by_function = mir.functions.iter().map(local_rvalues).collect::<Vec<_>>();
    let mut escaping = HashSet::new();
    for ((function, definitions), local_rvalues) in mir
        .functions
        .iter()
        .zip(&closure_defs_by_function)
        .zip(&local_rvalues_by_function)
    {
        for block in &function.blocks {
            let Some(Terminator::Return(operand)) = &block.terminator else {
                continue;
            };
            mark_operand_escaping_closures(
                operand,
                definitions,
                local_rvalues,
                &mut HashSet::new(),
                &mut escaping,
            );
        }
    }
    let closure_function_index = closure_defs_by_function
        .iter()
        .enumerate()
        .flat_map(|(function_index, definitions)| {
            definitions
                .values()
                .filter_map(move |definition| match definition {
                    ClosureLocalDef::Closure(id) => Some((*id, function_index)),
                    ClosureLocalDef::Alias(_) => None,
                })
        })
        .collect::<HashMap<_, _>>();
    let mut changed = true;
    while changed {
        changed = false;
        let escaping_ids = escaping.iter().copied().collect::<Vec<_>>();
        for id in escaping_ids {
            let Some(closure) = mir
                .closures
                .get(usize::try_from(id.0).unwrap_or(usize::MAX))
            else {
                continue;
            };
            let Some(function_index) = closure_function_index.get(&id).copied() else {
                continue;
            };
            for capture in &closure.captures {
                let before = escaping.len();
                let Some(definitions) = closure_defs_by_function.get(function_index) else {
                    continue;
                };
                let Some(local_rvalues) = local_rvalues_by_function.get(function_index) else {
                    continue;
                };
                mark_local_escaping_closures(
                    capture.source_local,
                    definitions,
                    local_rvalues,
                    &mut HashSet::new(),
                    &mut escaping,
                );
                changed |= escaping.len() != before;
            }
        }
    }
    for id in escaping {
        if let Some(closure) = mir
            .closures
            .get_mut(usize::try_from(id.0).unwrap_or(usize::MAX))
        {
            closure.escapes = true;
            for capture in &mut closure.captures {
                capture.mode = smelt_hir::CaptureMode::ByValue;
            }
        }
    }
}

/// Return local assignments in a function for escape analysis.
fn local_rvalues(function: &MirFunction) -> HashMap<LocalId, Rvalue> {
    let mut rvalues = HashMap::new();
    for block in &function.blocks {
        for statement in &block.statements {
            if let Statement::Assign { dest, value } = statement {
                rvalues.insert(*dest, value.clone());
            }
        }
    }
    rvalues
}

/// Mark closures reachable from a returned operand as escaping.
fn mark_operand_escaping_closures(
    operand: &Operand,
    definitions: &HashMap<LocalId, ClosureLocalDef>,
    local_rvalues: &HashMap<LocalId, Rvalue>,
    seen_locals: &mut HashSet<LocalId>,
    escaping: &mut HashSet<ClosureId>,
) {
    let Some(local) = operand_local(operand) else {
        return;
    };
    mark_local_escaping_closures(local, definitions, local_rvalues, seen_locals, escaping);
}

/// Mark closures reachable from a local as escaping.
fn mark_local_escaping_closures(
    local: LocalId,
    definitions: &HashMap<LocalId, ClosureLocalDef>,
    local_rvalues: &HashMap<LocalId, Rvalue>,
    seen_locals: &mut HashSet<LocalId>,
    escaping: &mut HashSet<ClosureId>,
) {
    if !seen_locals.insert(local) {
        return;
    }
    if let Some(id) = resolve_closure_local(local, definitions) {
        escaping.insert(id);
    }
    let Some(value) = local_rvalues.get(&local) else {
        return;
    };
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "closure escape analysis only descends into aggregate operands that can carry closures"
    )]
    match value {
        Rvalue::Use(source) => {
            mark_operand_escaping_closures(
                source,
                definitions,
                local_rvalues,
                seen_locals,
                escaping,
            );
        }
        Rvalue::List(items) | Rvalue::Set(items) | Rvalue::Tuple(items) => {
            for item in items {
                mark_operand_escaping_closures(
                    item,
                    definitions,
                    local_rvalues,
                    seen_locals,
                    escaping,
                );
            }
        }
        Rvalue::Dict(entries) => {
            for (key, value) in entries {
                mark_operand_escaping_closures(
                    key,
                    definitions,
                    local_rvalues,
                    seen_locals,
                    escaping,
                );
                mark_operand_escaping_closures(
                    value,
                    definitions,
                    local_rvalues,
                    seen_locals,
                    escaping,
                );
            }
        }
        _ => {}
    }
}

/// Return the closure and alias definitions inside one MIR function.
fn closure_definitions(function: &MirFunction) -> HashMap<LocalId, ClosureLocalDef> {
    let mut definitions = HashMap::new();
    for block in &function.blocks {
        for statement in &block.statements {
            let Statement::Assign { dest, value } = statement else {
                continue;
            };
            #[expect(
                clippy::wildcard_enum_match_arm,
                reason = "closure escape analysis only tracks closure definitions and aliases"
            )]
            match value {
                Rvalue::Closure { id, .. } => {
                    definitions.insert(*dest, ClosureLocalDef::Closure(*id));
                }
                Rvalue::Use(operand) => {
                    if let Some(source) = operand_local(operand) {
                        definitions.insert(*dest, ClosureLocalDef::Alias(source));
                    }
                }
                _ => {}
            }
        }
    }
    definitions
}

/// One local definition relevant to closure escape analysis.
#[derive(Debug, Clone, Copy)]
enum ClosureLocalDef {
    /// The local directly stores a closure construction.
    Closure(ClosureId),
    /// The local aliases another closure-typed local.
    Alias(LocalId),
}

/// Resolve a local through closure aliases into the constructed closure ID.
fn resolve_closure_local(
    local: LocalId,
    definitions: &HashMap<LocalId, ClosureLocalDef>,
) -> Option<ClosureId> {
    let mut current = local;
    let mut seen = HashSet::new();
    loop {
        if !seen.insert(current) {
            return None;
        }
        match definitions.get(&current).copied()? {
            ClosureLocalDef::Closure(id) => return Some(id),
            ClosureLocalDef::Alias(next) => current = next,
        }
    }
}
