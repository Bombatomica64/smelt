//! Emission of the fused dictionary-entry read-modify-write statement.
//!
//! [`Statement::DictEntryUpdate`] is formed by
//! `smelt_mir::opt::DictEntryUpdate` out of the triple a source
//! `result[key] = (result[key] ?? 0) + 1` lowers to. This module renders it as
//! the single-probe form a hand-written Rust port uses:
//!
//! ```text
//! { let mut smelt_slot = result.entry_or_insert(key, || 0.0);
//!   let current = (*smelt_slot).clone();
//!   *smelt_slot = current + 1.0; }
//! ```
//!
//! The container's entry accessor is the same one `list_push_text` reaches for,
//! chosen the same way: `SmeltJsMap`/`SmeltRecord` expose `entry_or_insert`,
//! which hands back a `RefMut<V>` guard, while a plain `HashMap` backing uses
//! `entry(..).or_insert_with(..)` and hands back a `&mut V`. The `RefMut` guard
//! is a live borrow of the container's shared store, so the MIR pass has already
//! proved that the stored rvalue reads nothing but `current` and constants —
//! nothing evaluated inside the block can touch the container.
//!
//! `current` is bound with a `let` INSIDE the block rather than assigned into
//! the local's function-scope predeclaration. The MIR pass requires `current` to
//! be a single-assignment, single-read temporary whose only reader is the stored
//! rvalue, so a block-local binding is exactly as visible as it needs to be, and
//! it keeps the emission independent of whether the local analysis chose to
//! predeclare the temporary.

use super::*;

impl FunctionEmitter<'_> {
    /// Whether the container of a fused entry update needs a mutable binding.
    ///
    /// `SmeltRecord` is a reference value with interior mutability, so its
    /// `entry_or_insert` takes `&self` and a `mut` binding would raise
    /// `unused_mut`. `SmeltJsMap::entry_or_insert` and `HashMap::entry` both
    /// take `&mut self`. Same split as [`Self::rvalue_borrows_local_mutably`]
    /// applies to `DictAssign`.
    pub(super) fn dict_entry_update_needs_mut_base(&self, base: LocalId) -> bool {
        let Ok(decl) = self.local_decl(base) else {
            return true;
        };
        let Some(Type::Dict(key_ty, _)) = self.mir.types.get(decl.ty) else {
            return true;
        };
        !self.dict_uses_smelt_record(*key_ty)
    }

    /// Emits a fused dict-entry update as one entry probe.
    ///
    /// `base` must be a `Dict`-typed local; the MIR pass guarantees that, and a
    /// mismatch is an internal error rather than a fallback, because there is no
    /// unfused form left to emit.
    pub(super) fn emit_dict_entry_update_statement(
        &self,
        base: LocalId,
        index: &Operand,
        default: &Operand,
        current: LocalId,
        value: &Rvalue,
        out: &mut String,
    ) -> Result<(), EmitError> {
        let base_ty = self.local_decl(base)?.ty;
        let Some(Type::Dict(dict_key_ty, dict_entry_ty)) = self.mir.types.get(base_ty) else {
            return Err(EmitError::new(
                "fused dict entry update base must be a dictionary",
            ));
        };
        let (key_ty, entry_ty) = (*dict_key_ty, *dict_entry_ty);
        let base_text = self.local_mut_value_text(base)?;
        // The key is coerced exactly as every other dict write coerces it: a
        // string-keyed container takes a JavaScript property-key string, any
        // other key type takes the container's key type directly.
        let key_text = if self.mir.types.get(key_ty) == Some(&Type::String) {
            let source_key = self.operand_ty(index)?;
            let index_text = self.operand_text(index)?;
            self.property_key_to_string_text(&index_text, source_key)?
        } else {
            self.value_at_type(index, key_ty)?
        };
        let default_text = self.value_at_type(default, entry_ty)?;
        // `SmeltJsMap` and `SmeltRecord` both expose `entry_or_insert`, which
        // seeds the entry from a CLOSURE so the seed is built only on the absent
        // path. A plain `HashMap` backing uses the standard-library spelling of
        // the same thing. Selected by the same predicate pair `list_push_text`
        // uses, so the two entry-mutating emitters cannot disagree about which
        // container is in play.
        let guarded = self.dict_uses_js_key_map(key_ty) || self.dict_uses_smelt_record(key_ty);
        let accessor = if guarded {
            format!("{base_text}.entry_or_insert({key_text}, || {default_text})")
        } else {
            format!("{base_text}.entry({key_text}).or_insert_with(|| {default_text})")
        };
        // A `RefMut<V>` needs a mutable binding to be assigned THROUGH (its
        // `DerefMut` takes `&mut self`); a `&mut V` does not, and marking it
        // `mut` would raise `unused_mut` in the generated crate.
        let slot = if guarded {
            "let mut smelt_slot"
        } else {
            "let smelt_slot"
        };
        let current_name = self.local_name(current)?.to_owned();
        let current_ty = self.type_text_with_impl_trait(self.local_decl(current)?.ty, false)?;
        let stored = self.rvalue_text_for_dest(value, entry_ty)?;
        out.push_str(&format!(
            "    {{ {slot} = {accessor}; let {current_name}: {current_ty} = (*smelt_slot).clone(); *smelt_slot = {stored}; }}\n"
        ));
        Ok(())
    }
}
