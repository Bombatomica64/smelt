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

/// Erased-view key of the live callable-member read hook (see
/// [`LIVE_MEMBER_PRELUDE`]).
pub(crate) const GET_CALLABLE_KEY: &str = "__smelt_method:__smelt_get_callable_field";

/// Name of the generated per-class live callable-field getter method.
pub(crate) const GET_CALLABLE_METHOD: &str = "__smelt_get_callable_field";

/// The prelude helper that reads a member off an erased value LIVE.
///
/// An erased view of a reference-class instance is a snapshot of its fields
/// taken when it was built, while the instance itself keeps changing: Hono's
/// `SmartRouter.match` replaces its own `match` field on its first call
/// (`this.match = router.match.bind(router)`), and the NEXT call through an
/// interface view projected from an earlier erasure must reach the new
/// function, not the snapshot's. A reference class whose program reads `this`
/// therefore carries a getter for its callable own fields under
/// [`GET_CALLABLE_KEY`]; this helper asks it first and falls back to the
/// ordinary member read (own property, then prototype) when the value has no
/// such hook or the hook does not know the key.
pub(crate) const LIVE_MEMBER_PRELUDE: &str = "/// Read `key` off an erased value, asking a class view's live callable-field getter first.\n#[allow(dead_code)]\nfn smelt_live_member(value: &SmeltUnknown, key: &str) -> SmeltUnknown { if let SmeltUnknown::Object(map) = value && let Some(SmeltUnknown::Function(getter)) = map.get(\"__smelt_method:__smelt_get_callable_field\") && let Ok(live) = getter(vec![SmeltUnknown::String(key.into())]) && !matches!(live, SmeltUnknown::Undefined) { return live; } smelt_get_unknown_field(value, key) }";

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
pub(crate) fn erased_view_entry_text(receiver: &str, entries: &str, live_reads: bool) -> String {
    let setter = format!(
        "{entries}.push(({SET_FIELD_KEY:?}.to_owned(), SmeltUnknown::Function(::std::rc::Rc::new({{ let smelt_receiver = {receiver}.clone(); move |smelt_args: Vec<SmeltUnknown>| {{ if let Some(SmeltUnknown::String(smelt_key)) = smelt_args.first() {{ smelt_receiver.{SET_FIELD_METHOD}(&smelt_key.to_string(), smelt_args.get(1).cloned().unwrap_or(SmeltUnknown::Undefined)); }} Ok(SmeltUnknown::Undefined) }} }})))); "
    );
    if !live_reads {
        return setter;
    }
    // The live-read hook (see `LIVE_MEMBER_PRELUDE`).
    format!(
        "{setter}{entries}.push(({GET_CALLABLE_KEY:?}.to_owned(), SmeltUnknown::Function(::std::rc::Rc::new({{ let smelt_receiver = {receiver}.clone(); move |smelt_args: Vec<SmeltUnknown>| {{ Ok(match smelt_args.first() {{ Some(SmeltUnknown::String(smelt_key)) => smelt_receiver.{GET_CALLABLE_METHOD}(&smelt_key.to_string()), _ => SmeltUnknown::Undefined }}) }} }})))); "
    )
}

/// Whether a reference class's erased view carries the live callable-field
/// getter: the program reads `this` (so a receiver-bound slot may read a member
/// live) and the class has at least one callable own field to serve.
pub(crate) fn has_live_callable_fields(
    mir: &Mir,
    program_reads_this: bool,
    fields: &[smelt_mir::MirField],
) -> bool {
    program_reads_this
        && fields.iter().any(|field| {
            field.visibility.is_own_property()
                && matches!(mir.types.get(field.ty), Some(Type::Function(_)))
        })
}

impl FunctionEmitter<'_> {
    /// Render the `__smelt_get_callable_field` method of one reference class:
    /// the CURRENT value of each callable own-property field, erased, keyed by
    /// its source spelling (`Undefined` for any other key). Only callable
    /// fields are served: they are what a method call through a snapshot view
    /// must read live (see [`LIVE_MEMBER_PRELUDE`]), and erasing every field
    /// on each read would rebuild whole nested values for nothing.
    pub(crate) fn reference_callable_field_getter_method_text(
        &self,
        fields: &[smelt_mir::MirField],
    ) -> Result<String, EmitError> {
        let mut arms = String::new();
        for field in fields {
            if !field.visibility.is_own_property()
                || !matches!(self.mir.types.get(field.ty), Some(Type::Function(_)))
            {
                continue;
            }
            let key = self.symbol_source_name(field.name)?;
            let field_name =
                crate::rust::RustIdent::new(self.mir.symbols.get(field.name).unwrap_or("field"))
                    .into_string();
            let Ok(erased) =
                self.erase_value_text(&format!("self.0.borrow().{field_name}.clone()"), field.ty)
            else {
                continue;
            };
            arms.push_str(&format!("{key:?} => {erased}, "));
        }
        // One line on purpose: the whole method is the erased-view read
        // adapter, and `smelt-unknown-report` classifies it by the
        // `fn __smelt_get_callable_field(` marker (see its
        // `live_callable_field_getter_is_a_boundary` test).
        Ok(format!(
            "    /// The live value of a callable own field, for a read through this instance's erased view.\n    #[allow(dead_code, unreachable_patterns)]\n    fn {GET_CALLABLE_METHOD}(&self, key: &str) -> SmeltUnknown {{ match key {{ {arms}_ => SmeltUnknown::Undefined }} }}\n"
        ))
    }

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
