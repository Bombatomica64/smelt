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
//! from (`closure_operand_text_for_declared_type`), so such a leak would reach
//! emitted output. [`HelperReservation`] removes the entry on drop unless
//! [`HelperReservation::commit`] stored the real body.
//!
//! The expansion stacks (`record_conversion_stack`, `type_expansion_stack`)
//! are kept balanced the same way: [`StackEntryGuard`] pops the entry it
//! pushed, [`SuspendedStack`] puts back the stack it set aside.

use std::cell::RefCell;
use std::collections::BTreeMap;

/// A helper table: key -> `(ordinal, body)`.
pub(super) type HelperTable<K> = RefCell<BTreeMap<K, (usize, String)>>;

/// The body a reservation holds until it is committed.
const PLACEHOLDER_BODY: &str = "Default::default()";

/// A reserved, not yet committed, entry of a [`HelperTable`].
pub(super) struct HelperReservation<'t, K: Ord> {
    /// The table holding the reservation.
    table: &'t HelperTable<K>,
    /// The reserved key; `None` once committed.
    key: Option<K>,
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
pub(super) fn reserve<K: Ord + Clone>(table: &HelperTable<K>, key: K) -> HelperReservation<'_, K> {
    let mut helpers = table.borrow_mut();
    let ordinal = helpers
        .values()
        .map(|(ordinal, _)| ordinal.saturating_add(1))
        .max()
        .unwrap_or(0);
    helpers.insert(key.clone(), (ordinal, PLACEHOLDER_BODY.to_owned()));
    HelperReservation {
        table,
        key: Some(key),
        ordinal,
    }
}

impl<K: Ord> HelperReservation<'_, K> {
    /// Store the helper's rendered body and keep the entry.
    pub(super) fn commit(mut self, body: String) -> usize {
        if let Some(key) = self.key.take()
            && let Some(entry) = self.table.borrow_mut().get_mut(&key)
        {
            entry.1 = body;
        }
        self.ordinal
    }
}

impl<K: Ord> Drop for HelperReservation<'_, K> {
    /// Release an uncommitted reservation.
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            self.table.borrow_mut().remove(&key);
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
