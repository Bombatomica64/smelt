//! Storage of a source binding that can be read before its first assignment.
//!
//! MIR marks such a binding [`LocalInit::MaybeUnbound`] and guarantees (the
//! validator enforces it) that it is written only by a whole-local
//! `Statement::Assign` and read only where it is definitely assigned. So the
//! four things this module renders are the only ways the binding reaches Rust
//! text:
//!
//! * its declaration, as `SmeltBinding<T>` (in a shared closure cell when a
//!   closure captures it), starting `SmeltBinding::unbound()`: no default value
//!   of `T` stands in for "not assigned yet";
//! * an assignment, `binding.bind(value)`;
//! * a check, `binding.checked(|| error(name))?`, which MIR puts before a read
//!   it cannot prove assigned: it raises the source language's error through
//!   the ordinary error channel, so a `try` the source wrote catches it;
//! * a read, `(*binding.bound())` (`bound_mut` for a write through it), which
//!   MIR only emits where the binding is assigned, past an assignment or a
//!   check.
//!
//! Every other binding keeps its plain `T`, with no flag and no check.

use super::*;
use crate::rust::{RustExpr, RustType};
use smelt_hir::UnboundRead;
use smelt_mir::LocalInit;

impl FunctionEmitter<'_> {
    /// Whether `local` is stored as a `SmeltBinding<T>`.
    pub(super) fn local_is_maybe_unbound(&self, local: LocalId) -> Result<bool, EmitError> {
        Ok(self.local_decl(local)?.init == LocalInit::MaybeUnbound)
    }

    /// `SmeltBinding<T>` for a local of type `ty`.
    pub(super) fn binding_storage_type(&self, ty: TypeId) -> Result<RustType, EmitError> {
        Ok(RustType::raw(format!("SmeltBinding<{}>", self.rendered_type(ty)?)))
    }

    /// Declares a possibly-unbound local before any block: as a plain
    /// `SmeltBinding<T>`, or as the shared cell holding one when a closure
    /// captures it. Both start unassigned.
    pub(super) fn emit_binding_declaration(&self, local: LocalId, out: &mut String) -> Result<(), EmitError> {
        let decl = self.local_decl(local)?;
        let storage = self.binding_storage_type(decl.ty)?;
        let name = self.local_name(local)?;
        if self.local_uses_shared_capture_storage(local) {
            out.push_str(&format!(
                "    let smelt_capture_{name}: ::std::rc::Rc<::std::cell::RefCell<{storage}>> = ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltBinding::unbound()));\n"
            ));
        } else {
            out.push_str(&format!("    let mut {name}: {storage} = SmeltBinding::unbound();\n"));
        }
        self.mark_local_declared(local);
        Ok(())
    }

    /// Assigns a possibly-unbound local. The value is evaluated before the
    /// binding's storage is borrowed, so a value that reads the same shared
    /// cell has released it first.
    pub(super) fn emit_binding_assignment(
        &self,
        local: LocalId,
        value: &RustExpr,
        out: &mut String,
    ) -> Result<(), EmitError> {
        let storage = self.binding_storage(local, true)?;
        out.push_str(&format!(
            "    {{ let smelt_next_value = {}; {}.bind(smelt_next_value); }}\n",
            value.delimited(),
            storage.tight()
        ));
        Ok(())
    }

    /// The checked read of `binding`: raises `unbound` when the binding is not
    /// assigned, and answers `()` otherwise. A binding a pass narrowed to plain
    /// storage is always assigned, so its check answers `Ok(())`.
    pub(super) fn binding_check_call(
        &self,
        binding: &Operand,
        unbound: UnboundRead,
    ) -> Result<(call::RenderedCall, TypeId), EmitError> {
        let local = binding_local(binding)?;
        if !self.local_is_maybe_unbound(local)? {
            return Ok((
                call::RenderedCall::propagating(RustExpr::atom("Ok::<(), Box<dyn ::std::error::Error>>(())")),
                self.none_ty,
            ));
        }
        let name = self.binding_source_name(local)?;
        let error = match unbound {
            UnboundRead::JsTemporalDeadZone => "smelt_js_tdz_error",
            UnboundRead::PyLocal => "smelt_py_unbound_local_error",
            UnboundRead::PyFreeVariable => "smelt_py_unbound_free_error",
            UnboundRead::PyGlobal => "smelt_py_name_error",
        };
        let storage = self.binding_storage(local, false)?;
        Ok((
            call::RenderedCall::propagating(
                storage.postfix(format!(".checked(|| {error}({name:?})).map(|_| ())")),
            ),
            self.none_ty,
        ))
    }

    /// The `SmeltBinding` a possibly-unbound local is stored in, as a place:
    /// its own name, or its shared cell borrowed (mutably when `mutable`).
    fn binding_storage(&self, local: LocalId, mutable: bool) -> Result<RustExpr, EmitError> {
        Ok(match self.local_shared_cell(local)? {
            Some(cell) if mutable => {
                self.binding_uses.record_mut_borrow(&cell);
                cell.write_expr()
            }
            Some(cell) => cell.read_expr(),
            None => self.local_expr(local)?,
        })
    }

    /// The value of a possibly-unbound local, as a place: `(*storage.bound())`,
    /// or `(*storage.bound_mut())` for a write through it. Only reads MIR
    /// proved assigned reach here (the validator rejects any other), so
    /// `bound` never finds the binding empty.
    pub(super) fn binding_value(&self, local: LocalId, mutable: bool) -> Result<RustExpr, EmitError> {
        let storage = self.binding_storage(local, mutable)?;
        let accessor = if mutable { "bound_mut" } else { "bound" };
        Ok(RustExpr::atom(format!("(*{}.{accessor}())", storage.tight())))
    }

    /// The name the source gave `local`, which the raised error quotes.
    fn binding_source_name(&self, local: LocalId) -> Result<String, EmitError> {
        match self.local_decl(local)?.kind {
            LocalKind::UserBinding(symbol) | LocalKind::Param { symbol: Some(symbol) } => {
                Ok(self.symbol_source_name(symbol)?.to_owned())
            }
            LocalKind::Param { symbol: None } | LocalKind::Temp => {
                Err(EmitError::new("a checked binding read names no source binding"))
            }
        }
    }
}

/// The local a binding read reads.
fn binding_local(binding: &Operand) -> Result<LocalId, EmitError> {
    binding
        .local()
        .ok_or_else(|| EmitError::new("a binding read must read a local"))
}
