//! Property writes through an erased view of a reference-class instance.
//!
//! A reference class is one JavaScript object held by a shared cell
//! (`Rc<RefCell<Inner>>`), and its erased view (`IntoSmeltUnknown`) keeps that
//! identity: every erasure of the same cell has the same object id and carries
//! the instance's prototype members. A property WRITE through that view,
//! however, only updated the view's own field store — a snapshot — so the
//! instance never saw it. Hono's reg-exp router is the shape: `match` runs
//! with `this` erased and replaces itself with `this.match = match`, and the
//! next `router.match(..)` read the old field and rebuilt every matcher from
//! caches the first build had already released.
//!
//! The write therefore reaches the instance the view stands for: each
//! reference class gets a generated `__smelt_set_field(key, value)` method
//! that assigns an own-property field from an erased value, its erased view
//! carries that method under [`SET_FIELD_KEY`], and an erased property write
//! calls `smelt_object_write_through` after updating the view's own store.
//! The key sits under the runtime's `__smelt_method:` prefix, which key
//! enumeration, structural equality, hashing and JSON already skip.
//!
//! The machinery is pay-for-use: [`program_writes_erased_fields`] gates all
//! of it, so a program that never writes a property through an erased value
//! is emitted exactly as before.

use smelt_hir::Type;
use smelt_mir::{Mir, Place, Statement};

use super::{EmitError, FunctionEmitter};

/// Erased-view key of the write-through hook (see the module docs).
pub(crate) const SET_FIELD_KEY: &str = "__smelt_method:__smelt_set_field";

/// Name of the generated per-class field setter method.
pub(crate) const SET_FIELD_METHOD: &str = "__smelt_set_field";

/// Return whether any function or closure writes a named property through an
/// erased (`unknown`, type-parameter, or union) value.
///
/// That is the only statement that can reach [`SET_FIELD_KEY`], so a program
/// without one needs none of the write-through machinery.
pub(crate) fn program_writes_erased_fields(mir: &Mir) -> bool {
    let erased = |locals: &[smelt_mir::LocalDecl], base: smelt_mir::LocalId| {
        usize::try_from(base.0)
            .ok()
            .and_then(|index| locals.get(index))
            .is_some_and(|local| {
                matches!(
                    mir.types.get(local.ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                )
            })
    };
    let writes = |locals: &[smelt_mir::LocalDecl], blocks: &[smelt_mir::BasicBlock]| {
        blocks.iter().flat_map(|block| &block.statements).any(|statement| {
            matches!(
                statement,
                Statement::AssignPlace { place: Place::Field { base, .. }, .. }
                    if erased(locals, *base)
            )
        })
    };
    mir.functions
        .iter()
        .any(|function| writes(&function.locals, &function.blocks))
        || mir
            .closures
            .iter()
            .any(|closure| writes(&closure.locals, &closure.blocks))
}

/// The prelude helper an erased property write calls after updating the
/// view's own store.
pub(crate) const WRITE_THROUGH_PRELUDE: &str = "/// Forward a property write on an erased class view to the instance it stands for.\nfn smelt_object_write_through(map: &SmeltObject, key: &str, value: &SmeltUnknown) { if let Some(SmeltUnknown::Function(setter)) = map.get(\"__smelt_method:__smelt_set_field\") { let _ = setter(vec![SmeltUnknown::String(key.into()), value.clone()]); } }";

/// The erased-view entry that installs the write-through hook, as a statement
/// pushing onto the entry vector `entries` of a view of `receiver`.
///
/// Both erasure paths of a reference class use it: the generated
/// `into_smelt_unknown` (`self`, `__smelt_entries`) and the emitter's inline
/// struct adapter (`smelt_struct_value`, `smelt_object_entries`), so the two
/// views of one instance agree.
pub(crate) fn erased_view_entry_text(receiver: &str, entries: &str) -> String {
    format!(
        "{entries}.push(({SET_FIELD_KEY:?}.to_owned(), SmeltUnknown::Function(::std::rc::Rc::new({{ let smelt_receiver = {receiver}.clone(); move |smelt_args: Vec<SmeltUnknown>| {{ if let Some(SmeltUnknown::String(smelt_key)) = smelt_args.first() {{ smelt_receiver.{SET_FIELD_METHOD}(&smelt_key.to_string(), smelt_args.get(1).cloned().unwrap_or(SmeltUnknown::Undefined)); }} Ok(SmeltUnknown::Undefined) }} }})))); "
    )
}

impl FunctionEmitter<'_> {
    /// Render the `__smelt_set_field` method of one reference class.
    ///
    /// One arm per own-property field, keyed by the field's SOURCE spelling
    /// (the key an erased write uses), assigning the erased value converted to
    /// the field's declared type. A field whose type has no conversion from
    /// the erased carrier is left out, so a write to it keeps updating only
    /// the view, as before. `self` must be an emitter for a function of the
    /// class, so the class's type parameters are in the render scope.
    pub(crate) fn reference_field_setter_method_text(
        &self,
        fields: &[smelt_mir::MirField],
    ) -> Result<String, EmitError> {
        let unknown_ty = self.type_id(Type::Unknown)?;
        let mut arms = String::new();
        for field in fields {
            if !field.visibility.is_own_property() {
                continue;
            }
            let Ok(converted) = self.value_at_type_text(
                "value",
                unknown_ty,
                field.ty,
                &self.render_scope(),
            ) else {
                continue;
            };
            let key = self.symbol_source_name(field.name)?;
            // The same spelling the inner record's declaration uses.
            let field_name =
                crate::rust::RustIdent::new(self.mir.symbols.get(field.name).unwrap_or("field"))
                    .into_string();
            arms.push_str(&format!(
                "            {key:?} => {{ let smelt_field_value = {converted}; self.0.borrow_mut().{field_name} = smelt_field_value; }}\n"
            ));
        }
        Ok(format!(
            "    /// Assign an own-property field from a write through this instance's erased view.\n    #[allow(dead_code, unreachable_patterns)]\n    fn {SET_FIELD_METHOD}(&self, key: &str, value: SmeltUnknown) {{\n        match key {{\n{arms}            _ => {{ let _ = value; }}\n        }}\n    }}\n"
        ))
    }
}
