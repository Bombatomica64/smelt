//! How a MIR local is declared in Rust: whether it is hoisted above its first
//! assignment, at which type, and with which default.

use super::*;
use crate::rust::{RustExpr, RustType};

impl FunctionEmitter<'_> {
    /// Returns locals that should be declared before block emission.
    ///
    /// Locals first assigned outside the entry block may be introduced inside a
    /// Rust branch scope and then reused by a sibling or follow-up block. Moving
    /// those bindings to function scope preserves MIR's function-local storage
    /// without perturbing straight-line entry-block declarations.
    pub(super) fn predeclared_locals(&self) -> HashSet<LocalId> {
        self.predeclared_locals.clone()
    }

    /// Whether a local's emitted Rust type is `&'static str` rather than `String`.
    ///
    /// True only for a local this crate's `typeof` analysis accepted; see
    /// [`typeof_str_locals`] for the whitelist and its rationale.
    pub(super) fn local_is_static_typeof_str(&self, local: LocalId) -> bool {
        self.static_typeof_str_locals.contains(&local)
    }

    /// The Rust type a local's declaration spells.
    ///
    /// The same text as [`Self::rendered_type`] for the local's MIR
    /// type, except for a local holding a `typeof` result: MIR has one string
    /// type and no borrowed one, so it types that local `String` while the value
    /// it holds is one of seven `&'static str` literals.
    pub(super) fn local_declaration_type(
        &self,
        local: LocalId,
        ty: TypeId,
    ) -> Result<RustType, EmitError> {
        if self.local_is_static_typeof_str(local) {
            return Ok(RustType::raw("&'static str"));
        }
        self.rendered_type(ty)
    }

    /// The value a local's declaration is seeded with before its first
    /// assignment, for a local that must be initialised at its declaration.
    ///
    /// `String::new()` for an owned string; the empty `&'static str` for a
    /// `typeof` local, which never holds an owned one.
    pub(super) fn local_default_value(
        &self,
        local: LocalId,
        ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        if self.local_is_static_typeof_str(local) {
            return Ok(RustExpr::chain("\"\""));
        }
        self.default_value(ty)
    }

    /// Returns whether a predeclared local should keep a concrete default.
    pub(super) fn predeclared_local_needs_default(
        &self,
        local: LocalId,
    ) -> Result<bool, EmitError> {
        let decl = self.local_decl(local)?;
        let is_callable = matches!(self.mir.types.get(decl.ty), Some(Type::Function(_)))
            || self.lexical_rust_type(decl.ty, false)?.holds_callable();
        // A callable compiler temp gets a seed only on the same evidence any
        // other local does: it is never assigned, it is read first, or the
        // definite-assignment walk (`local_may_be_used_before_assignment`,
        // consulted by every caller) finds a path that reads it unassigned. A
        // seed allocates a fresh default closure at every entry of the function,
        // even when the temp is only assigned in one branch and read there.
        Ok(is_callable
            && (self.local_assignment_count(local) == 0 || self.local_first_access_is_read(local)))
    }
}

/// Compute locals that need function-scope bindings before Rust block emission.
pub(super) fn predeclared_locals_for_function(
    mir: &Mir,
    function: &MirFunction,
) -> HashSet<LocalId> {
    let mut seen = function.params.iter().copied().collect::<HashSet<_>>();
    // A binding that can be read before its first assignment is declared up
    // front, unassigned (see `binding_storage`), wherever it is first assigned.
    let mut locals: HashSet<LocalId> = function
        .locals
        .iter()
        .enumerate()
        .filter(|(_, decl)| decl.init == smelt_mir::LocalInit::MaybeUnbound)
        .filter_map(|(index, _)| u32::try_from(index).ok().map(LocalId))
        .filter(|local| !seen.contains(local))
        .collect();
    for block in &function.blocks {
        for statement in &block.statements {
            if let Statement::Assign { dest, .. } = statement
                && !seen.insert(*dest)
            {
                locals.insert(*dest);
            }
            if let Statement::Assign {
                value: Rvalue::Closure { id, .. },
                ..
            } = statement
                && let Some(closure) = mir
                    .closures
                    .get(usize::try_from(id.0).unwrap_or(usize::MAX))
            {
                for capture in &closure.captures {
                    if !seen.contains(&capture.source_local) {
                        locals.insert(capture.source_local);
                    }
                }
            }
        }
    }
    if function.blocks.len() <= 1 {
        return locals;
    }
    for block in &function.blocks {
        if block.id == function.entry {
            continue;
        }
        for statement in &block.statements {
            if let Statement::Assign { dest, .. } = statement {
                locals.insert(*dest);
            }
        }
    }
    locals
}
