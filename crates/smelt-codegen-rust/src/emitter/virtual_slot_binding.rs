//! Binding a class's own overridable methods into its virtual method slots.
//!
//! When a subclass overrides a base method, the frontend gives the base class a
//! callable storage slot for that method (`add_overridden_base_method_fields`),
//! and a base-typed receiver calls the slot rather than the inherent method, so
//! a subclass value viewed through the base type still dispatches to its
//! override (JavaScript virtual dispatch). The subclass-to-base view fills the
//! slot with a closure over the subclass value (`virtual_method_storage_field_text`).
//!
//! An instance constructed as the base itself had nothing filling the slot: it
//! kept the field's default callback, so `base.method()` through a base-typed
//! receiver returned the default value (`""`) instead of running the base's own
//! body. The rule here closes that: at the end of a constructor, every virtual
//! slot the constructed class also implements as a method is bound to a closure
//! dispatching to that implementation on the constructed object.
//!
//! For a reference class the closure captures the handle, so dispatch is live.
//! For a value class it captures the value as it is when construction ends —
//! the same by-value contract the subclass view already has.

use super::*;

impl FunctionEmitter<'_> {
    /// Returns statements binding `this`'s virtual method slots to its own methods.
    ///
    /// `this_text` is the rendered constructor result and `this_ty` its type.
    /// Emits nothing for a type that is not a class, for a generic class (its
    /// slot closures would need the constructor's type parameters threaded
    /// through the bound call, which is not modeled yet), and for slots the
    /// class does not implement (abstract methods keep their default until a
    /// concrete subclass view fills them).
    pub(super) fn constructor_virtual_slot_bindings(
        &self,
        this_text: &str,
        this_ty: TypeId,
    ) -> Result<String, EmitError> {
        let Some(class) = self.class_for_type(this_ty) else {
            return Ok(String::new());
        };
        if !class.type_params.is_empty() {
            return Ok(String::new());
        }
        let class_name = class.name;
        let slots = class
            .fields
            .iter()
            .map(|field| field.name)
            .filter(|field| {
                self.is_virtual_method_storage_field(this_ty, *field)
                    && self.find_class_method_function(class_name, *field).is_some()
            })
            .collect::<Vec<_>>();
        let mut out = String::new();
        for slot in slots {
            let Some(bound) = self.virtual_method_storage_field_text(this_ty, this_ty, slot)?
            else {
                continue;
            };
            let field_name = sanitize_ident(self.symbol_name(slot)?);
            let place = if self.is_reference_class_type(this_ty) {
                format!("{this_text}.0.borrow_mut().{field_name}")
            } else {
                format!("{this_text}.{field_name}")
            };
            out.push_str(&format!(
                "    {{ let smelt_struct_value = {this_text}.clone(); let smelt_bound_slot = {bound}; {place} = smelt_bound_slot; }}\n"
            ));
        }
        Ok(out)
    }
}
