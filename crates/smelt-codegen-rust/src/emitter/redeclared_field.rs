//! A field a subclass redeclares at a narrower type, in a nested class layout.
//!
//! A polymorphic hierarchy (`crate::class_polymorphism`) stores every field
//! once, in the inner record of the level that first declares it, so the one
//! JavaScript property is one place in memory whichever static type reads
//! it. A subclass may redeclare that property at a narrower type
//! (`class Dog extends Animal { declare food: string }` over
//! `food: string | number`, or `value: number` over `value: unknown`). The
//! storage keeps the declaring level's type; the subclass reads it through a
//! checked downcast to its own type and writes its own type back through an
//! upcast, the conversions every other seam between the two types already
//! renders (`rendered_at_type`).

use super::FunctionEmitter;
use crate::EmitError;
use crate::rust::RustExpr;
use smelt_hir::{Symbol, TypeId};
use smelt_mir::MirField;

impl FunctionEmitter<'_> {
    /// The type the nested layout stores `field` of the record `record_ty`
    /// at, when a nested ancestor level stores it at another type than the
    /// record's own (redeclared) one; `None` when the record stores it at its
    /// own type.
    ///
    /// The stored type is the storing level's declaration read at the
    /// record's base arguments, so a slot that only differs by its
    /// parameters' ABI (`Fn(&T)` against `Fn(f64)`, `record_slot_abi`) is not
    /// a redeclaration.
    pub(super) fn redeclared_field_storage(
        &self,
        record_ty: TypeId,
        field: Symbol,
    ) -> Option<TypeId> {
        let smelt_hir::Type::Class { name, .. } = self.mir.types.get(record_ty)? else {
            return None;
        };
        crate::class_polymorphism::redeclared_field_storage(
            self.mir,
            self.context.polymorphic_classes(),
            *name,
            field,
        )
    }

    /// The record type of the class the function being emitted belongs to,
    /// at its own type parameters (`Parser<V>` inside `impl<V> Parser<V>`).
    pub(super) fn own_class_record_ty(&self) -> Option<TypeId> {
        let declaring = match self.function.origin {
            smelt_mir::HirOrigin::ClassMethod { class, .. }
            | smelt_mir::HirOrigin::ClassConstructor { class, .. }
            | smelt_mir::HirOrigin::ClassStaticMethod { class, .. } => class,
            smelt_mir::HirOrigin::Body(_) => return None,
        };
        let owner = self.role.inherited_impl_owner().unwrap_or(declaring);
        crate::classes::class_self_type(self.mir, owner)
    }

    /// `value`, rendered at `field`'s own type on `record_ty`, converted to
    /// the type the nested layout stores it at (an upcast; unchanged when the
    /// field is not redeclared).
    pub(super) fn field_value_at_storage(
        &self,
        value: RustExpr,
        record_ty: TypeId,
        field: &MirField,
    ) -> Result<RustExpr, EmitError> {
        match self.redeclared_field_storage(record_ty, field.name) {
            Some(stored) => self.rendered_at_type(&value, field.ty, stored, &self.render_scope()),
            None => Ok(value),
        }
    }
}
