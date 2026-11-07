// Runtime prelude for `node:events`' `EventEmitter`.
//
// An emitter is an insertion-ordered list of listeners, each tagged with its
// event name, a `once` flag, and its function identity (so `off` can find it).
// The list lives behind an `Rc<RefCell<..>>` with a `smelt_next_object_id`
// identity, in the same shape as the other reference objects: two variables
// holding the same emitter observe each other's registrations.
//
// # Why the listener store is erased
//
// **Dynamic boundary.** A listener's signature is not knowable from the event
// name — `on('data', cb)` takes a chunk and `on('end', cb)` takes nothing —
// and `emit(name, ...args)` passes an arbitrary positional list decided by the
// emitting site, not by the emitter's type. One emitter holds listeners for
// many events at once, so the store is heterogeneous and keyed by a runtime
// string. No concrete type, generated union, or scoped generic can express
// that: the callback set is only known at run time, on the branch that
// registered it. The store is therefore the existing erased callable ABI.
//
// # `emit` iterates a snapshot
//
// Measured against Node 22 rather than read from the docs, because two of
// these are easy to get wrong and invisible to any compile step:
//
// * a listener **added** during an emit does NOT run in that emit;
// * a listener **removed** during an emit **still runs** in it.
//
// Together those say `emit` takes a snapshot of the matching listeners and
// calls that, rather than walking the live list. A `once` listener is removed
// from the live list *before* its call, so a re-entrant `emit` cannot run it
// twice.

// Emit the listener record and the emitter struct.
// @smelt:item SmeltEventListener when event_emitter
/// One registered listener: its event, its callback, and how it ends.
#[derive(Clone)]
pub struct SmeltEventListener {
    event: String,
    callback: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn ::std::error::Error>>>,
    /// Whether this listener is removed after one call.
    once: bool,
    /// Function identity, so `off(name, fn)` can find this entry.
    identity: SmeltFunctionIdentity,
}

/// A `node:events` `EventEmitter`: listeners in registration order.
#[derive(Clone)]
pub struct SmeltEventEmitter {
    id: usize,
    listeners: ::std::rc::Rc<::std::cell::RefCell<Vec<SmeltEventListener>>>,
}

//~ Identity equality, not structural: two emitters are the same emitter only
//~ when they are the same object, and closures have no structural equality
//~ to compare anyway.
impl PartialEq for SmeltEventEmitter { fn eq(&self, other: &Self) -> bool { self.id == other.id } }
impl ::std::fmt::Debug for SmeltEventEmitter { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_struct("SmeltEventEmitter").field("listeners", &self.listeners.borrow().len()).finish() } }
impl Default for SmeltEventEmitter { fn default() -> Self { Self::new() } }

//~ Emit the emitter's operations.
#[allow(dead_code)]
impl SmeltEventEmitter {
    /// An emitter with no listeners and a fresh JS reference identity.
    pub fn new() -> Self { Self { id: smelt_next_object_id(), listeners: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())) } }
    /// JS reference identity of this emitter.
    pub fn id(&self) -> usize { self.id }
    /// `on`/`addListener`/`once`: append a listener.
    ///
    /// Appending is what makes listeners fire in REGISTRATION order.
    pub fn add(&self, event: &(impl SmeltHostText + ?Sized), callback: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn ::std::error::Error>>>, once: bool) -> Self { let event = event.host_text(); let event: &str = &event;
        let identity = smelt_canonical_function_identity(&callback);
        self.listeners.borrow_mut().push(SmeltEventListener { event: event.to_owned(), callback, once, identity });
        //~ Every registration/removal answers the emitter, so
        //~ `e.on(..).on(..)` chains as it does in Node.
        self.clone()
    }
    /// `off`/`removeListener`: remove ONE matching instance.
    ///
    /// The spec removes at most one, and the most recently added when the
    /// same function was registered more than once, so the search runs
    /// from the end. Removing a listener that was never added is not an
    /// error.
    pub fn remove(&self, event: &(impl SmeltHostText + ?Sized), callback: &::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn ::std::error::Error>>>) -> Self { let event = event.host_text(); let event: &str = &event;
        let identity = smelt_canonical_function_identity(callback);
        let mut listeners = self.listeners.borrow_mut();
        if let Some(index) = listeners.iter().rposition(|listener| listener.event == event && listener.identity == identity) { listeners.remove(index); }
        drop(listeners);
        self.clone()
    }
    /// `removeAllListeners(name)`: drop every listener for one event.
    pub fn remove_all(&self, event: &(impl SmeltHostText + ?Sized)) -> Self { let event = event.host_text(); let event: &str = &event;
        self.listeners.borrow_mut().retain(|listener| listener.event != event);
        self.clone()
    }
    /// `listenerCount(name)`.
    pub fn listener_count(&self, event: &(impl SmeltHostText + ?Sized)) -> f64 { let event = event.host_text(); let event: &str = &event; self.listeners.borrow().iter().filter(|listener| listener.event == event).count() as f64 }
    /// `emit(name, ...args)`: call the listeners, and answer whether any ran.
    ///
    /// Iterates a SNAPSHOT of the matching listeners, which is what makes
    /// a listener added during the emit wait for the next one while a
    /// listener removed during it still runs. `once` entries leave the live
    /// list before any call, so a re-entrant `emit` cannot run one twice.
    pub fn emit(&self, event: &(impl SmeltHostText + ?Sized), args: Vec<SmeltUnknown>) -> Result<bool, Box<dyn ::std::error::Error>> { let event = event.host_text(); let event: &str = &event;
        let snapshot: Vec<SmeltEventListener> = self.listeners.borrow().iter().filter(|listener| listener.event == event).cloned().collect();
        if snapshot.is_empty() {
            return Ok(false);
        }
        self.listeners.borrow_mut().retain(|listener| !(listener.event == event && listener.once));
        for listener in snapshot {
            (listener.callback)(args.clone())?;
        }
        Ok(true)
    }
}

// @smelt:item-end

// Emit [`WEAK_HANDLE`].
// @smelt:item SmeltEmitterHandle when event_emitter
/// An emitter a listener can hold without owning it.
trait SmeltEmitterHandle: Sized {
    /// The handle with its listener list held weakly.
    type Weak;
    /// Downgrade to the weak handle.
    fn smelt_downgrade(&self) -> Self::Weak;
    /// The emitter again, while it is alive.
    fn smelt_upgrade(weak: &Self::Weak) -> Option<Self>;
}

/// `SmeltEventEmitter` with its listener list held weakly.
#[derive(Clone)]
struct SmeltWeakEventEmitter { id: usize, listeners: ::std::rc::Weak<::std::cell::RefCell<Vec<SmeltEventListener>>> }

impl SmeltEmitterHandle for SmeltEventEmitter {
    type Weak = SmeltWeakEventEmitter;
    fn smelt_downgrade(&self) -> Self::Weak { SmeltWeakEventEmitter { id: self.id, listeners: ::std::rc::Rc::downgrade(&self.listeners) } }
    fn smelt_upgrade(weak: &Self::Weak) -> Option<Self> { Some(Self { id: weak.id, listeners: weak.listeners.upgrade()? }) }
}

/// The emitter a listener registered on it captures, held weakly.
struct SmeltWeakEmitter<H: SmeltEmitterHandle>(H::Weak);

impl<H: SmeltEmitterHandle> SmeltWeakEmitter<H> {
    /// Capture `emitter` weakly.
    fn new(emitter: &H) -> Self { Self(emitter.smelt_downgrade()) }
    /// The emitter: alive whenever its listener runs.
    fn smelt_emitter(&self) -> H { H::smelt_upgrade(&self.0).expect("a listener ran after the emitter it was registered on was dropped") }
}

// @smelt:item-end
