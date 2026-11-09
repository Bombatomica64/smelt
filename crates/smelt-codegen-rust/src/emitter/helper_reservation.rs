//! Guards for the module-level helpers and expansion stacks a coercion
//! touches while it renders.
//!
//! Two coercions hoist their body into a module-level helper
//! (`FunctionEmitter::hoisted_record_conversion`,
//! `FunctionEmitter::hoisted_record_adapter_call`). The helper is RESERVED —
//! registered under its key with a placeholder body — before the body is
//! rendered, because a record that reaches its own type again converts that
//! inner value by calling the same helper and needs its ordinal. Rendering the
//! body can fail (`?`) or decline; either way the reservation must not stay
//! behind, or the crate would flush a helper whose body is the placeholder
//! `Default::default()`. Closure rendering errors ARE caught and fallen back
//! from (`closure_operand_expr_for_declared_type`), so such a leak would reach
//! emitted output.
//!
//! Removing the one entry is not enough: while the body rendered, a nested
//! helper may have been reserved AND committed, and its body may call the
//! outer helper's ordinal (the cycle is the reason for reserving early). A
//! released outer ordinal then leaves that nested helper calling a function
//! nothing defines (E0425). So a reservation is a transaction over everything
//! the render mints ([`super::render_state::MintedItems`]): it marks the
//! minted state when it is taken, and dropping it without
//! [`HelperReservation::commit`] rolls every table back to that mark, its own
//! entry and every nested helper, accessor and erasure record with it. The
//! text that called them is discarded with the failed render, so nothing that
//! survives can name a rolled-back item.
//!
//! The expansion stacks (`record_conversion_stack`, `type_expansion_stack`)
//! are kept balanced the same way: [`StackEntryGuard`] pops the entry it
//! pushed, [`SuspendedStack`] puts back the stack it set aside.

use std::cell::RefCell;
use std::collections::BTreeMap;

use super::render_state::{MintedItems, MintedMark};

/// A helper table: key -> `(ordinal, body)`.
pub(super) type HelperTable<K> = RefCell<BTreeMap<K, (usize, String)>>;

/// The body a reservation holds until it is committed.
const PLACEHOLDER_BODY: &str = "Default::default()";

/// A reserved, not yet committed, entry of a [`HelperTable`].
pub(super) struct HelperReservation<'t, K: Ord> {
    /// Everything a render mints, rolled back to `mark` on release.
    minted: &'t MintedItems,
    /// The table holding the reservation, one of `minted`'s.
    table: &'t HelperTable<K>,
    /// The reserved key; `None` once committed.
    key: Option<K>,
    /// The minted state before the reservation; `None` once committed.
    mark: Option<MintedMark>,
    /// The ordinal naming the helper.
    ordinal: usize,
}

/// Reserve `key` in `table` with a placeholder body and a fresh ordinal.
///
/// The ordinal is one past the largest ordinal in the table, not the table's
/// length: a reservation can be released after a nested helper committed a
/// higher ordinal, and reusing the length would then hand a second helper that
/// same ordinal (two functions with one name). While no reservation was ever
/// released the ordinals are dense and the two rules agree.
///
/// `table` must be one of `minted`'s tables: releasing the reservation rolls
/// all of `minted` back to the state before this call.
pub(super) fn reserve<'t, K: Ord + Clone>(
    minted: &'t MintedItems,
    table: &'t HelperTable<K>,
    key: K,
) -> HelperReservation<'t, K> {
    let mark = minted.mark();
    let mut helpers = table.borrow_mut();
    let ordinal = helpers
        .values()
        .map(|(ordinal, _)| ordinal.saturating_add(1))
        .max()
        .unwrap_or(0);
    helpers.insert(key.clone(), (ordinal, PLACEHOLDER_BODY.to_owned()));
    HelperReservation {
        minted,
        table,
        key: Some(key),
        mark: Some(mark),
        ordinal,
    }
}

impl<K: Ord> HelperReservation<'_, K> {
    /// Store the helper's rendered body and keep the entry.
    pub(super) fn commit(mut self, body: String) -> usize {
        self.mark = None;
        if let Some(key) = self.key.take()
            && let Some(entry) = self.table.borrow_mut().get_mut(&key)
        {
            entry.1 = body;
        }
        self.ordinal
    }
}

impl<K: Ord> Drop for HelperReservation<'_, K> {
    /// Release an uncommitted reservation: roll everything minted since it was
    /// taken back, its own entry and every nested item included.
    fn drop(&mut self) {
        self.key = None;
        if let Some(mark) = self.mark.take() {
            self.minted.rollback_to(&mark);
        }
    }
}

/// Pops the entry it pushed onto an expansion stack when dropped.
pub(super) struct StackEntryGuard<'s, T> {
    /// The stack the entry was pushed onto.
    stack: &'s RefCell<Vec<T>>,
}

/// Push `entry` onto `stack` for as long as the returned guard lives.
pub(super) fn push_entry<T>(stack: &RefCell<Vec<T>>, entry: T) -> StackEntryGuard<'_, T> {
    stack.borrow_mut().push(entry);
    StackEntryGuard { stack }
}

impl<T> Drop for StackEntryGuard<'_, T> {
    /// Pop this guard's entry.
    fn drop(&mut self) {
        self.stack.borrow_mut().pop();
    }
}

/// Sets a stack aside (leaving it empty) and puts it back when dropped.
pub(super) struct SuspendedStack<'s, T> {
    /// The stack that was set aside.
    stack: &'s RefCell<Vec<T>>,
    /// Its entries at suspension.
    saved: Vec<T>,
}

/// Empty `stack` for as long as the returned guard lives.
pub(super) fn suspend<T>(stack: &RefCell<Vec<T>>) -> SuspendedStack<'_, T> {
    let saved = std::mem::take(&mut *stack.borrow_mut());
    SuspendedStack { stack, saved }
}

impl<T> Drop for SuspendedStack<'_, T> {
    /// Restore the suspended entries.
    fn drop(&mut self) {
        *self.stack.borrow_mut() = std::mem::take(&mut self.saved);
    }
}

#[cfg(test)]
mod tests {
    use super::{MintedItems, reserve};

    /// A nested helper committed while an outer reservation rendered its body
    /// may call the outer ordinal, so releasing the outer reservation must take
    /// the nested helper with it: neither survives to be flushed.
    #[test]
    fn a_released_reservation_rolls_back_nested_commits() {
        let minted = MintedItems::default();
        let outer = reserve(&minted, &minted.record_conversion_helpers, "Outer".to_owned());
        let nested = reserve(
            &minted,
            &minted.record_adapter_helpers,
            ("Source".to_owned(), "Target".to_owned()),
        );
        let outer_ordinal = outer.ordinal;
        nested.commit(format!("__smelt_from_unknown_record_{outer_ordinal}(smelt_value)"));
        minted.backend_erasures.borrow_mut().clear();
        minted.console_inspect_used.set(true);
        assert_eq!(minted.record_adapter_helpers.borrow().len(), 1);
        drop(outer);
        assert!(minted.record_conversion_helpers.borrow().is_empty(), "the outer entry is released");
        assert!(
            minted.record_adapter_helpers.borrow().is_empty(),
            "the nested helper that calls the released ordinal is released with it"
        );
        assert!(!minted.console_inspect_used.get(), "flags raised by the discarded render are lowered");
    }

    /// A committed reservation keeps its entry and everything nested in it, and
    /// entries minted before it are never touched by a later release.
    #[test]
    fn a_commit_keeps_and_a_release_spares_earlier_items() {
        let minted = MintedItems::default();
        let first = reserve(&minted, &minted.record_conversion_helpers, "First".to_owned());
        let first_ordinal = first.commit("first".to_owned());
        let second = reserve(&minted, &minted.record_conversion_helpers, "Second".to_owned());
        assert_eq!(second.ordinal, first_ordinal + 1);
        drop(second);
        let helpers = minted.record_conversion_helpers.borrow();
        assert_eq!(helpers.len(), 1);
        assert_eq!(helpers.get("First").map(|(_, body)| body.as_str()), Some("first"));
    }
}
