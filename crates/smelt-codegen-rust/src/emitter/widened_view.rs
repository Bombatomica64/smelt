//! Field access and method calls through widened class handles
//! (`crate::class_widening`).
//!
//! A handle of a widened class whose type arguments mention the erased type
//! or a type parameter may hold a converting view of a cell built at other
//! arguments. Such a view has no record to borrow, so:
//!
//! - a declared field is read through `recv.smelt_read_<f>()` and written
//!   through `(*recv.smelt_write_<f>())` (`SmeltFieldRead`/`SmeltFieldWrite`
//!   guards, which on a real cell are the record's own `Ref`/`RefMut`);
//! - every method that has an erased prototype adapter begins with a forward:
//!   a view answers the call from the source instantiation's adapter, so the
//!   body only ever runs on a real cell. Inside such a method `self` keeps the
//!   direct record path.

use super::*;
use crate::class_widening::{handle_may_be_view, proto_method_key, read_accessor, write_accessor};
use crate::rust::RustExpr;

impl FunctionEmitter<'_> {
    /// Whether this function is a method of a widened class that forwards a
    /// call on a converting view to the source instantiation
    /// ([`Self::widened_view_prologue`]), so its own receiver is never a view.
    fn forwards_widened_views(&self) -> bool {
        if !matches!(self.function.origin, HirOrigin::ClassMethod { .. }) {
            return false;
        }
        // The class whose impl the method is emitted into: an inherited copy
        // in a subclass's impl runs on that subclass's handles.
        let Some(class) = self.method_impl_owner() else {
            return false;
        };
        self.method_name.is_none()
            && !self.function.is_async
            && !self.function.is_generator
            && self.function.rest.is_none()
            && self.context.is_widened_class(class)
            && self.context.demand().has(Need::Unknown)
            && crate::classes::find_class(self.mir, class).is_some_and(|found| {
                crate::class_proto::class_has_proto_entries(self.mir, self.context, found)
            })
            && crate::class_proto::method_is_proto_eligible(
                self.mir,
                self.context,
                class,
                self.function,
            )
    }

    /// Whether `base` is this method's own receiver.
    fn is_method_receiver(&self, base: LocalId) -> bool {
        matches!(self.function.origin, HirOrigin::ClassMethod { .. })
            && self.function.params.first() == Some(&base)
    }

    /// Whether `base` is the instance a constructor builds: a fresh cell at
    /// the class's own arguments, never a view.
    fn is_constructed_instance(&self, base: LocalId) -> bool {
        matches!(self.function.origin, HirOrigin::ClassConstructor { .. })
            && self.local_name(base).is_ok_and(|name| name == "this")
    }

    /// Whether a declared field of `base` (a reference class handle of type
    /// `base_ty`) is reached through the widened accessors: the handle may be
    /// a converting view, and it is neither the receiver of a method that
    /// forwards views away nor the instance a constructor builds.
    pub(super) fn field_goes_through_accessors(&self, base: LocalId, base_ty: TypeId) -> bool {
        handle_may_be_view(self.mir, self.context, base_ty)
            && !(self.is_method_receiver(base) && self.forwards_widened_views())
            && !self.is_constructed_instance(base)
    }

    /// The suffix reading declared field `field` off the handle `base` of type
    /// `base_ty`: `.0.borrow().f`, or the accessor guard `.smelt_read_f()`
    /// (which dereferences to the field, so `.clone()` and method calls read
    /// through it).
    pub(super) fn reference_field_read_suffix(
        &self,
        base: LocalId,
        base_ty: TypeId,
        field: Symbol,
    ) -> String {
        let field_name = self.context.names().field_text(self.mir, field);
        if self.field_goes_through_accessors(base, base_ty) {
            format!(".{}()", read_accessor(&field_name))
        } else {
            format!(".0.borrow().{field_name}")
        }
    }

    /// The place writing declared field `field` of the handle rendered as
    /// `base_text` (the local `base`, of type `base_ty`): `base.0.borrow_mut().f`,
    /// or the dereferenced accessor guard `(*base.smelt_write_f())`, which a
    /// view stores back when the statement ends.
    pub(super) fn reference_field_write_text(
        &self,
        base_text: &str,
        base: LocalId,
        base_ty: TypeId,
        field: Symbol,
    ) -> String {
        let field_name = self.context.names().field_text(self.mir, field);
        if self.field_goes_through_accessors(base, base_ty) {
            format!("(*{base_text}.{}())", write_accessor(&field_name))
        } else {
            format!("{base_text}.0.borrow_mut().{field_name}")
        }
    }

    /// The forward a method of a widened class begins with: on a converting
    /// view, the call runs as the source instantiation's erased prototype
    /// adapter (`__smelt_method:<name>`), with each argument erased at this
    /// method's parameter type and the answer recovered at its return type.
    /// Empty for a method that does not forward.
    pub(super) fn widened_view_prologue(&self) -> Result<String, EmitError> {
        if !self.forwards_widened_views() {
            return Ok(String::new());
        }
        let HirOrigin::ClassMethod { method, .. } = self.function.origin else {
            return Ok(String::new());
        };
        let Some(spelling) = self.mir.symbols.get(method) else {
            return Ok(String::new());
        };
        let key = proto_method_key(spelling);
        let mut args = Vec::new();
        for param in self.function.params.iter().skip(1) {
            let ty = self.local_decl(*param)?.ty;
            let name = self.local_name(*param)?.into_owned();
            args.push(
                self.erase_rendered(&RustExpr::path(name).cloned_value(), ty)?
                    .into_string(),
            );
        }
        // The erased adapter answers `Result<SmeltUnknown, Box<dyn Error>>`:
        // a throwing method passes the error on, a non-throwing one never
        // receives one.
        let throws = self.call_is_throwing(self.function);
        let returns_unit = matches!(
            self.mir.types.get(self.function.return_ty),
            Some(Type::None)
        );
        let recovered = |text: &str| -> Result<String, EmitError> {
            if returns_unit {
                return Ok(format!("{{ let _ = {text}; }}"));
            }
            let scope = self.render_scope();
            Ok(self
                .extract_rendered(&RustExpr::atom(text), self.function.return_ty, &scope)?
                .into_string())
        };
        let answer_value = if throws {
            format!(
                "smelt_view_call.map(|smelt_view_value| {})",
                recovered("smelt_view_value")?
            )
        } else {
            recovered("smelt_view_call.unwrap_or_else(|smelt_error| panic!(\"{smelt_error}\"))")?
        };
        Ok(format!(
            "    if self.smelt_is_view() && let Some(smelt_view_answer) = smelt_widened_cold(|| {{ let Some(SmeltUnknown::Function(smelt_view_method)) = self.smelt_view()?.smelt_method({key:?}) else {{ return None; }}; let smelt_view_call = smelt_view_method(vec![{}]); Some({answer_value}) }}) {{ return smelt_view_answer; }}\n",
            args.join(", ")
        ))
    }
}
