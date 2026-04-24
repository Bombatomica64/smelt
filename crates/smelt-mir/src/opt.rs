use std::collections::{HashMap, HashSet};

use crate::{Callee, LocalId, Mir, MirFunction, Operand, Place, Rvalue, Statement, Terminator};

pub trait Pass {
    fn name(&self) -> &'static str;
    fn run(&self, mir: &mut Mir) -> bool;
}

#[derive(Debug, Default)]
pub struct CopyPropagation;

impl Pass for CopyPropagation {
    fn name(&self) -> &'static str {
        "copy-propagation"
    }

    fn run(&self, mir: &mut Mir) -> bool {
        let mut changed = false;
        for function in &mut mir.functions {
            changed |= propagate_function(function);
        }
        changed
    }
}

#[must_use]
pub fn default_passes() -> Vec<Box<dyn Pass>> {
    vec![Box::<CopyPropagation>::default()]
}

pub fn optimize(mir: &mut Mir) {
    let passes = default_passes();
    loop {
        let mut changed = false;
        for pass in &passes {
            changed |= pass.run(mir);
        }
        if !changed {
            break;
        }
    }
}

fn propagate_function(function: &mut MirFunction) -> bool {
    let mut aliases = HashMap::new();

    for block in &function.blocks {
        for stmt in &block.statements {
            if let Statement::Assign {
                dest,
                value: Rvalue::Use(Operand::Copy(Place::Local(source))),
            } = stmt
            {
                aliases.insert(*dest, resolve_alias(&aliases, *source));
            }
        }
    }

    if aliases.is_empty() {
        return false;
    }

    let mut changed = false;
    for block in &mut function.blocks {
        for phi in &mut block.phis {
            for (_, operand) in &mut phi.incoming {
                changed |= rewrite_operand(operand, &aliases);
            }
        }
        for stmt in &mut block.statements {
            if let Statement::Assign { dest, value } = stmt {
                changed |= rewrite_rvalue(value, &aliases, Some(*dest));
            }
        }
        if let Some(terminator) = &mut block.terminator {
            changed |= rewrite_terminator(terminator, &aliases);
        }
    }

    changed
}

fn resolve_alias(aliases: &HashMap<LocalId, LocalId>, local: LocalId) -> LocalId {
    let mut current = local;
    let mut seen = HashSet::new();
    while let Some(next) = aliases.get(&current).copied() {
        if !seen.insert(current) {
            break;
        }
        current = next;
    }
    current
}

fn rewrite_rvalue(
    value: &mut Rvalue,
    aliases: &HashMap<LocalId, LocalId>,
    dest: Option<LocalId>,
) -> bool {
    match value {
        Rvalue::Use(operand) => rewrite_operand_except(operand, aliases, dest),
    }
}

fn rewrite_terminator(terminator: &mut Terminator, aliases: &HashMap<LocalId, LocalId>) -> bool {
    match terminator {
        Terminator::Goto(_) | Terminator::Unreachable => false,
        Terminator::Return(operand) => rewrite_operand(operand, aliases),
        Terminator::Call {
            callee, args, dest, ..
        } => {
            let mut changed = match callee {
                Callee::Indirect(operand) => rewrite_operand(operand, aliases),
                Callee::Static(_) | Callee::Builtin(_) => false,
            };
            for arg in args {
                changed |= rewrite_operand_except(arg, aliases, Some(*dest));
            }
            changed
        }
    }
}

fn rewrite_operand(operand: &mut Operand, aliases: &HashMap<LocalId, LocalId>) -> bool {
    rewrite_operand_except(operand, aliases, None)
}

fn rewrite_operand_except(
    operand: &mut Operand,
    aliases: &HashMap<LocalId, LocalId>,
    except: Option<LocalId>,
) -> bool {
    let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = operand else {
        return false;
    };
    if Some(*local) == except {
        return false;
    }
    let resolved = resolve_alias(aliases, *local);
    if resolved == *local {
        return false;
    }
    *local = resolved;
    true
}
