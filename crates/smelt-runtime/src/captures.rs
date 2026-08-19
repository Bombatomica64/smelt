//! Shared mutable captures for closures.
//!
//! A JavaScript closure captures its enclosing `let` by *reference*: two
//! closures created in the same scope observe each other's writes, and so does
//! the enclosing function. Generated Rust models one such variable as an
//! `Rc<RefCell<T>>` that every closure over the same scope must reach — and the
//! closures are emitted independently, so they need a way to find the *same*
//! cell at run time.
//!
//! The key is `(scope, slot)`: `scope` is a fresh id minted per scope
//! instantiation by [`smelt_next_capture_scope`], and `slot` is the address of
//! the captured local, which distinguishes the variables within one scope. The
//! registry holds `Weak` handles, so a cell is dropped with its closures instead
//! of leaking for the process lifetime.

// @smelt:item SMELT_NEXT_CAPTURE_SCOPE
thread_local! {
    static SMELT_NEXT_CAPTURE_SCOPE: ::std::cell::Cell<usize> = const { ::std::cell::Cell::new(1) };
    static SMELT_SHARED_CAPTURES: ::std::cell::RefCell<::std::collections::HashMap<(usize, usize), ::std::rc::Weak<dyn ::std::any::Any>>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());
}
// @smelt:item-end

// @smelt:item smelt_next_capture_scope
fn smelt_next_capture_scope() -> usize {
    SMELT_NEXT_CAPTURE_SCOPE.with(|next| { let id = next.get(); next.set(id.saturating_add(1)); id })
}
// @smelt:item-end

// @smelt:item smelt_shared_capture
fn smelt_shared_capture<T: Clone + 'static>(scope: usize, slot: *mut T, initial: T) -> ::std::rc::Rc<::std::cell::RefCell<T>> {
    let key = (scope, slot as usize);
    SMELT_SHARED_CAPTURES.with(|captures| {
        let mut captures = captures.borrow_mut();
        if let Some(existing) = captures.get(&key).and_then(::std::rc::Weak::upgrade) {
            return existing.downcast::<::std::cell::RefCell<T>>().expect("shared capture type mismatch");
        }
        let value: ::std::rc::Rc<::std::cell::RefCell<T>> = ::std::rc::Rc::new(::std::cell::RefCell::new(initial));
        let erased: ::std::rc::Rc<dyn ::std::any::Any> = value.clone();
        captures.insert(key, ::std::rc::Rc::downgrade(&erased));
        value
    })
}
// @smelt:item-end

#[cfg(test)]
mod tests {
    use super::{smelt_next_capture_scope, smelt_shared_capture};

    /// Scope ids are distinct, so two instantiations never share a cell.
    ///
    /// Calling a closure factory twice must produce independent state: this is
    /// what makes `makeCounter()` twice give two counters.
    #[test]
    fn capture_scopes_are_distinct() {
        assert_ne!(
            smelt_next_capture_scope(),
            smelt_next_capture_scope(),
            "each scope instantiation needs its own id"
        );
    }

    /// The same `(scope, slot)` resolves to the same cell, so writes are shared.
    #[test]
    fn the_same_scope_and_slot_share_one_cell() {
        let scope = smelt_next_capture_scope();
        let mut local: i32 = 0;
        let slot: *mut i32 = &raw mut local;
        let first = smelt_shared_capture(scope, slot, 0);
        let second = smelt_shared_capture(scope, slot, 99);
        *first.borrow_mut() += 1;
        assert_eq!(
            *second.borrow(),
            1,
            "the second lookup must see the first closure's write"
        );
        assert!(
            ::std::rc::Rc::ptr_eq(&first, &second),
            "one variable in one scope is one cell"
        );
        assert_eq!(
            local, 0,
            "the slot address is only used as a registry key, never written through"
        );
    }

    /// A different scope with the same slot address is a different cell.
    #[test]
    fn a_different_scope_is_a_different_cell() {
        let mut local: i32 = 0;
        let slot: *mut i32 = &raw mut local;
        let first = smelt_shared_capture(smelt_next_capture_scope(), slot, 0);
        let second = smelt_shared_capture(smelt_next_capture_scope(), slot, 0);
        *first.borrow_mut() = 7;
        assert_eq!(
            *second.borrow(),
            0,
            "two instantiations of one closure factory keep separate state"
        );
        assert_eq!(
            local, 0,
            "the slot address is only used as a registry key, never written through"
        );
    }

    /// Entries are weak: dropping every handle releases the registry slot.
    ///
    /// The initial value proves it — a fresh lookup after the drop re-runs the
    /// initializer instead of resurrecting the old cell.
    #[test]
    fn dropped_cells_do_not_linger() {
        let scope = smelt_next_capture_scope();
        let mut local: i32 = 0;
        let slot: *mut i32 = &raw mut local;
        {
            let cell = smelt_shared_capture(scope, slot, 1);
            *cell.borrow_mut() = 5;
        }
        let revived = smelt_shared_capture(scope, slot, 1);
        assert_eq!(
            *revived.borrow(),
            1,
            "a dropped capture must not be resurrected from the registry"
        );
        assert_eq!(
            local, 0,
            "the slot address is only used as a registry key, never written through"
        );
    }
}
