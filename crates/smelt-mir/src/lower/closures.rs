//! Closure ABI ownership for HIR-to-MIR lowering.
//!
//! This module is the single home for every closure-specific concern in MIR
//! lowering. It owns:
//!
//! * **Construction** — [`LoweringCtx::lower_closure_expr`] turns an
//!   `ExprKind::Closure` into a [`MirClosure`] table entry plus a
//!   [`Rvalue::Closure`] in the owning body, recording captured operands and
//!   capture metadata.
//! * **Escape analysis** — [`mark_escaping_closures`] marks every closure that
//!   can be reached from a returned value as escaping and rewrites its captures
//!   to by-value ownership.
//! * **Throwing ABI widening** — [`widen_throwing_closure_types`] re-types the
//!   locals and capture slots that hold throwing closures so they carry the
//!   `Result` ABI everywhere they flow.
//!
//! General (non-closure) throwing-ness propagation lives in
//! [`super::passes::throwing`]; this module only consumes its `can_throw`
//! results to widen closure-typed locals.
//!
//! # Invariants published for the codegen-rust emitter
//!
//! After [`lower_hir`](super::lower_hir) runs its closure passes, the Rust
//! source emitter may assume all of the following without re-deriving them:
//!
//! 1. **Escaping closures capture by value.** Every closure with
//!    `MirClosure::escapes == true` has all of its captures set to
//!    [`CaptureMode::ByValue`](smelt_hir::CaptureMode::ByValue). The emitter can
//!    therefore generate an owning `move` closure and never borrows a stack
//!    local that outlives its owner function. This is the invariant
//!    [`crate::validate`] enforces in `validate_closures`.
//! 2. **Escape reachability is transitive.** A closure is escaping if it is
//!    returned directly, returned through local aliases, returned inside an
//!    aggregate (`list`/`set`/`tuple`/`dict`), or captured by another escaping
//!    closure. The emitter does not need to chase aliases itself.
//! 3. **Throwing closures use a throwing function ABI uniformly.** When a
//!    closure body `can_throw`, every MIR local that stores, aliases, or
//!    captures that closure is typed `Type::Function { may_throw: true, .. }`
//!    with the closure's return type. The emitter can pick the `Result`-based
//!    closure signature from any local's type, not just the construction site.
//! 4. **Capture metadata matches captured-local types.** After widening, each
//!    `MirClosureCapture::ty` and the closure-body local it targets share the
//!    (possibly widened) type of the captured source local, so the emitter's
//!    capture-struct field types agree with the owner's local types.

use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;

use smelt_hir::Type;

use crate::{
    BasicBlock, ClosureId, LocalDecl, LocalId, Mir, MirClosure, MirClosureCapture, MirFunction,
    Operand, Place, Rvalue, Statement, Terminator,
};

use super::passes::operand_local;
use super::{
    LowerError, LoweringCtx, closure_id_index, local_index, u32_from_usize, usize_from_u32,
};

impl LoweringCtx<'_> {
    /// Lower an `ExprKind::Closure` into a MIR closure entry and `Rvalue::Closure`.
    ///
    /// This recursively lowers the closure body in a nested [`LoweringCtx`],
    /// records the captured operands and their capture metadata, appends the
    /// resulting [`MirClosure`] (plus any nested closures) to the closure table,
    /// and emits the closure construction assignment into the current block. The
    /// `escapes` flag starts as `false`; [`mark_escaping_closures`] later promotes
    /// closures that escape their owner.
    pub(super) fn lower_closure_expr(
        &mut self,
        closure: &smelt_hir::ClosureExpr,
        expr_ty: smelt_hir::TypeId,
        expr_span: smelt_hir::Span,
    ) -> Result<Operand, LowerError> {
        let closure_index = self
            .closure_base
            .checked_add(self.closures.len())
            .ok_or_else(|| self.error("MIR closure index overflowed usize", Some(expr_span)))?;
        let closure_id = ClosureId(u32_from_usize(
            closure_index,
            "MIR closure index does not fit in u32",
        )?);
        let captures = closure
            .captures
            .iter()
            .map(|capture| {
                self.locals
                    .get(&capture.source_local)
                    .copied()
                    .map(|local| Operand::Copy(Place::Local(local)))
                    .ok_or_else(|| {
                        self.error(
                            "closure captures a local that is not available in MIR",
                            Some(closure.span),
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mir_captures = closure
            .captures
            .iter()
            .map(|capture| {
                self.locals
                    .get(&capture.source_local)
                    .copied()
                    .map(|source_local| MirClosureCapture {
                        source_local,
                        target_local: capture.body_local.map(|local| LocalId(local.0)),
                        symbol: capture.symbol,
                        ty: capture.ty,
                        mode: capture.mode,
                    })
                    .ok_or_else(|| {
                        self.error(
                            "closure captures a local that is not available in MIR",
                            Some(closure.span),
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let closure_body = self
            .krate
            .bodies
            .get(usize_from_u32(
                closure.body.0,
                "HIR closure body index does not fit in usize",
            )?)
            .ok_or_else(|| self.error("closure references an unknown body", Some(closure.span)))?;
        let closure_ctx = LoweringCtx::new_closure(
            self.krate,
            self.item_functions,
            closure.body,
            closure_body,
            closure.return_ty,
            closure.span,
            self.loop_index_ty,
            self.loop_bool_ty,
            closure_index
                .checked_add(1)
                .ok_or_else(|| self.error("MIR closure index overflowed usize", Some(expr_span)))?,
        )?;
        let (function, nested_closures) = closure_ctx.lower()?;
        self.closures.push(MirClosure {
            id: closure_id,
            params: function.params,
            rest: closure.rest,
            required_params: closure.required_params,
            locals: function.locals,
            captures: mir_captures,
            return_ty: closure.return_ty,
            blocks: function.blocks,
            entry: function.entry,
            escapes: false,
            can_throw: function.can_throw,
        });
        self.closures.extend(nested_closures);
        let dest = self.push_temp(expr_ty, expr_span);
        self.block_mut()?.statements.push(Statement::Assign {
            dest,
            value: Rvalue::Closure {
                id: closure_id,
                captures,
            },
        });
        Ok(Operand::Copy(Place::Local(dest)))
    }
}

// ===========================================================================
// Escape analysis (invariants 1 and 2)
// ===========================================================================

/// Mark closures returned from their creating function as escaping.
///
/// This is the MIR-level equivalent of Rust's closure escape analysis for the
/// subset Smelt supports today: closure values may be created into temporaries,
/// moved through local aliases, and returned. Once a closure escapes, its
/// captures are promoted to by-value so Rust emission uses an owning `move`
/// closure instead of borrowing stack locals that are about to disappear.
///
/// Publishes invariants 1 and 2 in the module docstring.
pub(super) fn mark_escaping_closures(mir: &mut Mir) {
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

// ===========================================================================
// Throwing ABI widening (invariants 3 and 4)
// ===========================================================================

/// Widen MIR locals that hold throwing closures to the throwing function ABI.
///
/// HIR closure expressions initially carry the syntactic function type. MIR can
/// later discover additional throwing behavior through lowered calls inside the
/// closure body (propagated by [`super::passes::throwing`]), so the local
/// receiving `Rvalue::Closure` must be updated after throw propagation. Keeping
/// this in MIR avoids frontend guesses about which callees will eventually use a
/// `Result` ABI.
///
/// Publishes invariants 3 and 4 in the module docstring.
pub(super) fn widen_throwing_closure_types(mir: &mut Mir) {
    for function_index in 0..mir.functions.len() {
        let mut changed = true;
        while changed {
            changed = false;
            let assignments = {
                let Some(function) = mir.functions.get(function_index) else {
                    continue;
                };
                local_alias_assignments(&function.blocks)
            };
            for (dest, source) in assignments {
                let Some(function) = mir.functions.get_mut(function_index) else {
                    continue;
                };
                if widen_function_local_from_source(
                    &mut mir.types,
                    &mut function.locals,
                    dest,
                    source,
                ) {
                    changed = true;
                }
            }
        }
        let updates = {
            let Some(function) = mir.functions.get(function_index) else {
                continue;
            };
            let mut updates = Vec::new();
            for block in &function.blocks {
                for statement in &block.statements {
                    let Statement::Assign {
                        dest,
                        value: Rvalue::Closure { id, .. },
                    } = statement
                    else {
                        continue;
                    };
                    let Some(closure) =
                        closure_id_index(*id).and_then(|index| mir.closures.get(index))
                    else {
                        continue;
                    };
                    if !closure.can_throw {
                        continue;
                    }
                    updates.push((*dest, closure.return_ty));
                }
            }
            updates
        };
        for (local, return_ty) in updates {
            let Some(local_decl) = local_index(local)
                .and_then(|index| {
                    mir.functions
                        .get(function_index)
                        .and_then(|function| function.locals.get(index))
                })
                .cloned()
            else {
                continue;
            };
            let Some(Type::Function(mut function_ty)) = mir.types.get(local_decl.ty).cloned()
            else {
                continue;
            };
            if function_ty.may_throw {
                continue;
            }
            function_ty.may_throw = true;
            function_ty.return_ty = return_ty;
            let widened = mir.types.intern(Type::Function(function_ty));
            if let Some(local_decl) = local_index(local).and_then(|index| {
                mir.functions
                    .get_mut(function_index)
                    .and_then(|function| function.locals.get_mut(index))
            }) {
                local_decl.ty = widened;
            }
        }
        let Some(function) = mir.functions.get_mut(function_index) else {
            continue;
        };
        let assignments = local_alias_assignments(&function.blocks);
        propagate_throwing_function_aliases(&mut mir.types, &mut function.locals, &assignments);
        let owner_locals = function.locals.clone();
        let closure_ids = closure_ids_in_blocks(&function.blocks);
        synchronize_closure_capture_types_from_locals(
            &mut mir.closures,
            &owner_locals,
            closure_ids,
        );
    }
    for closure_index in 0..mir.closures.len() {
        let mut changed = true;
        while changed {
            changed = false;
            let assignments = {
                let Some(closure) = mir.closures.get(closure_index) else {
                    continue;
                };
                closure
                    .blocks
                    .iter()
                    .flat_map(|block| block.statements.iter())
                    .filter_map(|statement| {
                        let Statement::Assign {
                            dest,
                            value: Rvalue::Use(operand),
                        } = statement
                        else {
                            return None;
                        };
                        let source = operand_local(operand)?;
                        Some((*dest, source))
                    })
                    .collect::<Vec<_>>()
            };
            for (dest, source) in assignments {
                let Some(closure) = mir.closures.get_mut(closure_index) else {
                    continue;
                };
                if widen_function_local_from_source(
                    &mut mir.types,
                    &mut closure.locals,
                    dest,
                    source,
                ) {
                    changed = true;
                }
            }
        }
        let updates = {
            let Some(closure) = mir.closures.get(closure_index) else {
                continue;
            };
            let mut updates = Vec::new();
            for block in &closure.blocks {
                for statement in &block.statements {
                    let Statement::Assign {
                        dest,
                        value: Rvalue::Closure { id, .. },
                    } = statement
                    else {
                        continue;
                    };
                    let Some(nested) =
                        closure_id_index(*id).and_then(|index| mir.closures.get(index))
                    else {
                        continue;
                    };
                    if nested.can_throw {
                        updates.push((*dest, nested.return_ty));
                    }
                }
            }
            updates
        };
        for (local, return_ty) in updates {
            let Some(local_decl) = local_index(local)
                .and_then(|index| {
                    mir.closures
                        .get(closure_index)
                        .and_then(|closure| closure.locals.get(index))
                })
                .cloned()
            else {
                continue;
            };
            let Some(Type::Function(mut function_ty)) = mir.types.get(local_decl.ty).cloned()
            else {
                continue;
            };
            if function_ty.may_throw {
                continue;
            }
            function_ty.may_throw = true;
            function_ty.return_ty = return_ty;
            let widened = mir.types.intern(Type::Function(function_ty));
            if let Some(local_decl) = local_index(local).and_then(|index| {
                mir.closures
                    .get_mut(closure_index)
                    .and_then(|closure| closure.locals.get_mut(index))
            }) {
                local_decl.ty = widened;
            }
        }
        let Some(closure) = mir.closures.get_mut(closure_index) else {
            continue;
        };
        let assignments = local_alias_assignments(&closure.blocks);
        propagate_throwing_function_aliases(&mut mir.types, &mut closure.locals, &assignments);
        let owner_locals = closure.locals.clone();
        let closure_ids = closure_ids_in_blocks(&closure.blocks);
        synchronize_closure_capture_types_from_locals(
            &mut mir.closures,
            &owner_locals,
            closure_ids,
        );
    }
}

/// Return closure IDs constructed by assignments in a block list.
fn closure_ids_in_blocks(blocks: &[BasicBlock]) -> Vec<ClosureId> {
    blocks
        .iter()
        .flat_map(|block| block.statements.iter())
        .filter_map(|statement| {
            let Statement::Assign {
                value: Rvalue::Closure { id, .. },
                ..
            } = statement
            else {
                return None;
            };
            Some(*id)
        })
        .collect()
}

/// Keep capture metadata and captured locals aligned with widened source locals.
fn synchronize_closure_capture_types_from_locals(
    closures: &mut [MirClosure],
    owner_locals: &[LocalDecl],
    closure_ids: Vec<ClosureId>,
) {
    for closure_id in closure_ids {
        let Some(closure) = closure_id_index(closure_id).and_then(|index| closures.get_mut(index))
        else {
            continue;
        };
        for capture in &mut closure.captures {
            let Some(source_ty) = local_index(capture.source_local)
                .and_then(|index| owner_locals.get(index))
                .map(|decl| decl.ty)
            else {
                continue;
            };
            capture.ty = source_ty;
            if let Some(target_local) = capture.target_local
                && let Some(local) =
                    local_index(target_local).and_then(|index| closure.locals.get_mut(index))
            {
                local.ty = source_ty;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Throwing-function ABI propagation through local aliases.
//
// Invariant: after these helpers run for a local set, every function-typed
// local that aliases a throwing function-typed source is also typed with the
// throwing ABI and the source return type.
// ---------------------------------------------------------------------------

/// Return local alias assignments from a block list.
fn local_alias_assignments(blocks: &[BasicBlock]) -> Vec<(LocalId, LocalId)> {
    blocks
        .iter()
        .flat_map(|block| block.statements.iter())
        .filter_map(|statement| {
            let Statement::Assign {
                dest,
                value: Rvalue::Use(operand),
            } = statement
            else {
                return None;
            };
            let source = operand_local(operand)?;
            Some((*dest, source))
        })
        .collect()
}

/// Propagate throwing function ABI through local-to-local aliases in blocks.
fn propagate_throwing_function_aliases(
    types: &mut smelt_hir::TypeInterner,
    locals: &mut [LocalDecl],
    assignments: &[(LocalId, LocalId)],
) {
    let mut changed = true;
    while changed {
        changed = false;
        for (dest, source) in assignments.iter().copied() {
            if widen_function_local_from_source(types, locals, dest, source) {
                changed = true;
            }
        }
    }
}

/// Widen a function-typed destination local when it aliases a throwing source.
fn widen_function_local_from_source(
    types: &mut smelt_hir::TypeInterner,
    locals: &mut [LocalDecl],
    dest: LocalId,
    source: LocalId,
) -> bool {
    let Some(source_ty) = local_index(source)
        .and_then(|index| locals.get(index))
        .map(|decl| decl.ty)
    else {
        return false;
    };
    let Some(Type::Function(source_fn)) = types.get(source_ty).cloned() else {
        return false;
    };
    if !source_fn.may_throw {
        return false;
    }
    let Some(dest_ty) = local_index(dest)
        .and_then(|index| locals.get(index))
        .map(|decl| decl.ty)
    else {
        return false;
    };
    let Some(Type::Function(mut dest_fn)) = types.get(dest_ty).cloned() else {
        return false;
    };
    if dest_fn.may_throw {
        return false;
    }
    dest_fn.may_throw = true;
    dest_fn.return_ty = source_fn.return_ty;
    let widened = types.intern(Type::Function(dest_fn));
    if let Some(local) = local_index(dest).and_then(|index| locals.get_mut(index)) {
        local.ty = widened;
        true
    } else {
        false
    }
}
