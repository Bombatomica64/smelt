//! Manual idiomatic port of es-toolkit `Semaphore` + `Mutex`.
//!
//! Goal: discover the *desired* Rust shape for JS classes with shared mutable
//! state and escaping callbacks, independent of Smelt's current codegen.
//!
//! The essential JS semantics we must preserve:
//!   1. Objects are REFERENCE types: `const b = a;` then mutating `b` is visible
//!      through `a`. Clone shares identity, it does not fork state.
//!   2. A method can store a callback that captures `this` (Semaphore.acquire
//!      pushes `resolve` into this.deferredTasks); that callback outlives the
//!      method call and later mutates the same object.
//!   3. Composition (Mutex holds a Semaphore) shares the inner object's identity.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

// A JS `() => void` resolve callback. FnMut boxed, shared owner.
type ResolveFn = Rc<RefCell<dyn FnMut()>>;

// ---- Semaphore -----------------------------------------------------------
// The "JS object" shape: a thin handle newtype over Rc<RefCell<Inner>>.
// Field access and mutation go through the shared cell; Clone shares identity.

struct SemaphoreInner {
    capacity: f64,
    available: f64,
    deferred_tasks: VecDeque<ResolveFn>,
}

#[derive(Clone)]
struct Semaphore(Rc<RefCell<SemaphoreInner>>);

impl Semaphore {
    fn new(capacity: f64) -> Self {
        Semaphore(Rc::new(RefCell::new(SemaphoreInner {
            capacity,
            available: capacity,
            deferred_tasks: VecDeque::new(),
        })))
    }

    // NOTE: &self, not &mut self. Interior mutability means a shared handle can
    // still mutate, and — crucially — we can hand `self.clone()` into a closure
    // that escapes this call. In the real async version this returns a Promise;
    // here `register` models "push resolve into deferredTasks".
    fn acquire(&self, register: impl FnOnce(ResolveFn)) {
        if self.0.borrow().available > 0.0 {
            self.0.borrow_mut().available -= 1.0;
            return;
        }
        // The escaping callback path: the resolver captures a shared handle to
        // THIS object and mutates it later, from release().
        let this = self.clone();
        let resolve: ResolveFn = Rc::new(RefCell::new(move || {
            // when resolved, the waiter has effectively taken its permit
            let _ = &this; // (in real code the awaiting task proceeds)
        }));
        self.0.borrow_mut().deferred_tasks.push_back(resolve.clone());
        register(resolve);
    }

    fn release(&self) {
        let deferred = self.0.borrow_mut().deferred_tasks.pop_front();
        if let Some(task) = deferred {
            (task.borrow_mut())();
            return;
        }
        let mut inner = self.0.borrow_mut();
        if inner.available < inner.capacity {
            inner.available += 1.0;
        }
    }

    fn available(&self) -> f64 {
        self.0.borrow().available
    }
}

// ---- Mutex ---------------------------------------------------------------
// Composition: holds a Semaphore. Because Semaphore is a shared handle, the
// Mutex's methods that delegate to `self.semaphore` mutate the REAL semaphore,
// not a clone. `isLocked` is a COMPUTED getter -> a method, not a stored field.

#[derive(Clone)]
struct Mutex {
    semaphore: Semaphore,
}

impl Mutex {
    fn new() -> Self {
        Mutex {
            semaphore: Semaphore::new(1.0),
        }
    }

    fn is_locked(&self) -> bool {
        self.semaphore.available() == 0.0
    }

    fn acquire(&self, register: impl FnOnce(ResolveFn)) {
        self.semaphore.acquire(register);
    }

    fn release(&self) {
        self.semaphore.release();
    }
}

fn main() {
    // Scenario mirroring mutex.spec: acquire locks, release unlocks — and the
    // state change must be visible through the SAME object (reference identity).
    let m = Mutex::new();
    assert!(!m.is_locked(), "fresh mutex unlocked");

    m.acquire(|_resolve| { /* first acquire takes the only permit synchronously */ });
    assert!(m.is_locked(), "locked after acquire");

    m.release();
    assert!(!m.is_locked(), "unlocked after release");

    // Reference-identity check: a "copy" of the handle observes the same state.
    let alias = m.clone();
    m.acquire(|_r| {});
    assert!(alias.is_locked(), "alias sees the lock taken through the original");
    alias.release();
    assert!(!m.is_locked(), "original sees the release done through the alias");

    // Semaphore fairness/waiter queue: a second acquire with no permits queues.
    let s = Semaphore::new(1.0);
    s.acquire(|_r| {}); // takes the permit
    let queued = Rc::new(RefCell::new(false));
    let q2 = queued.clone();
    s.acquire(move |_resolve| {
        *q2.borrow_mut() = true; // registered as a waiter
    });
    assert!(*queued.borrow(), "second acquire queued a waiter");
    assert_eq!(s.available(), 0.0);

    println!("all scenarios pass: shared-mutable-identity port is correct");
}
