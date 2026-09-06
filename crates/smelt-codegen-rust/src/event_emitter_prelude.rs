//! Runtime prelude for `node:events`' `EventEmitter`.
//!
//! An emitter is an insertion-ordered list of listeners, each tagged with its
//! event name, a `once` flag, and its function identity (so `off` can find it).
//! The list lives behind an `Rc<RefCell<..>>` with a `smelt_next_object_id`
//! identity, in the same shape as the other reference objects: two variables
//! holding the same emitter observe each other's registrations.
//!
//! # Why the listener store is erased
//!
//! **Dynamic boundary.** A listener's signature is not knowable from the event
//! name — `on('data', cb)` takes a chunk and `on('end', cb)` takes nothing —
//! and `emit(name, ...args)` passes an arbitrary positional list decided by the
//! emitting site, not by the emitter's type. One emitter holds listeners for
//! many events at once, so the store is heterogeneous and keyed by a runtime
//! string. No concrete type, generated union, or scoped generic can express
//! that: the callback set is only known at run time, on the branch that
//! registered it. The store is therefore the existing erased callable ABI.
//!
//! # `emit` iterates a snapshot
//!
//! Measured against Node 22 rather than read from the docs, because two of
//! these are easy to get wrong and invisible to any compile step:
//!
//! * a listener **added** during an emit does NOT run in that emit;
//! * a listener **removed** during an emit **still runs** in it.
//!
//! Together those say `emit` takes a snapshot of the matching listeners and
//! calls that, rather than walking the live list. A `once` listener is removed
//! from the live list *before* its call, so a re-entrant `emit` cannot run it
//! twice.

use crate::rust::CodeWriter;

/// Emit the `SmeltEventEmitter` runtime type.
///
/// Gated on `needs_unknown` as well as on its own use: the listener store is
/// the erased callable ABI, so an emitter cannot exist without the carrier.
pub fn emit(writer: &mut CodeWriter) {
    emit_struct(writer);
    emit_inherent_impl(writer);
}

/// Emit the listener record and the emitter struct.
fn emit_struct(writer: &mut CodeWriter) {
    writer.line("/// One registered listener: its event, its callback, and how it ends.");
    writer.line("#[derive(Clone)]");
    writer.block("pub struct SmeltEventListener", |struct_writer| {
        struct_writer.line("event: String,");
        struct_writer
            .line("callback: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn ::std::error::Error>>>,");
        struct_writer.line("/// Whether this listener is removed after one call.");
        struct_writer.line("once: bool,");
        struct_writer.line("/// Function identity, so `off(name, fn)` can find this entry.");
        struct_writer.line("identity: usize,");
    });
    writer.blank_line();
    writer.line("/// A `node:events` `EventEmitter`: listeners in registration order.");
    writer.line("#[derive(Clone)]");
    writer.block("pub struct SmeltEventEmitter", |struct_writer| {
        struct_writer.line("id: usize,");
        struct_writer
            .line("listeners: ::std::rc::Rc<::std::cell::RefCell<Vec<SmeltEventListener>>>,");
    });
    writer.blank_line();
    // Identity equality, not structural: two emitters are the same emitter only
    // when they are the same object, and closures have no structural equality
    // to compare anyway.
    writer.line(
        "impl PartialEq for SmeltEventEmitter { fn eq(&self, other: &Self) -> bool { self.id == other.id } }",
    );
    writer.line(
        "impl ::std::fmt::Debug for SmeltEventEmitter { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_struct(\"SmeltEventEmitter\").field(\"listeners\", &self.listeners.borrow().len()).finish() } }",
    );
    writer.line("impl Default for SmeltEventEmitter { fn default() -> Self { Self::new() } }");
    writer.blank_line();
}

/// Emit the emitter's operations.
fn emit_inherent_impl(writer: &mut CodeWriter) {
    writer.line("#[allow(dead_code)]");
    writer.block("impl SmeltEventEmitter", |impl_writer| {
        impl_writer.line("/// An emitter with no listeners and a fresh JS reference identity.");
        impl_writer.line(
            "pub fn new() -> Self { Self { id: smelt_next_object_id(), listeners: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())) } }",
        );
        impl_writer.line("/// JS reference identity of this emitter.");
        impl_writer.line("pub fn id(&self) -> usize { self.id }");
        impl_writer.line("/// `on`/`addListener`/`once`: append a listener.");
        impl_writer.line("///");
        impl_writer.line("/// Appending is what makes listeners fire in REGISTRATION order.");
        impl_writer.block(
            "pub fn add(&self, event: &str, callback: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn ::std::error::Error>>>, once: bool) -> Self",
            |fn_writer| {
                fn_writer.line("let identity = smelt_canonical_function_identity(&callback);");
                fn_writer.line("self.listeners.borrow_mut().push(SmeltEventListener { event: event.to_owned(), callback, once, identity });");
                // Every registration/removal answers the emitter, so
                // `e.on(..).on(..)` chains as it does in Node.
                fn_writer.line("self.clone()");
            },
        );
        impl_writer.line("/// `off`/`removeListener`: remove ONE matching instance.");
        impl_writer.line("///");
        impl_writer.line("/// The spec removes at most one, and the most recently added when the");
        impl_writer.line("/// same function was registered more than once, so the search runs");
        impl_writer.line("/// from the end. Removing a listener that was never added is not an");
        impl_writer.line("/// error.");
        impl_writer.block(
            "pub fn remove(&self, event: &str, callback: &::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn ::std::error::Error>>>) -> Self",
            |fn_writer| {
                fn_writer.line("let identity = smelt_canonical_function_identity(callback);");
                fn_writer.line("let mut listeners = self.listeners.borrow_mut();");
                fn_writer.line("if let Some(index) = listeners.iter().rposition(|listener| listener.event == event && listener.identity == identity) { listeners.remove(index); }");
                fn_writer.line("drop(listeners);");
                fn_writer.line("self.clone()");
            },
        );
        impl_writer.line("/// `removeAllListeners(name)`: drop every listener for one event.");
        impl_writer.block(
            "pub fn remove_all(&self, event: &str) -> Self",
            |fn_writer| {
                fn_writer.line("self.listeners.borrow_mut().retain(|listener| listener.event != event);");
                fn_writer.line("self.clone()");
            },
        );
        impl_writer.line("/// `listenerCount(name)`.");
        impl_writer.line(
            "pub fn listener_count(&self, event: &str) -> f64 { self.listeners.borrow().iter().filter(|listener| listener.event == event).count() as f64 }",
        );
        impl_writer.line("/// `emit(name, ...args)`: call the listeners, and answer whether any ran.");
        impl_writer.line("///");
        impl_writer.line("/// Iterates a SNAPSHOT of the matching listeners, which is what makes");
        impl_writer.line("/// a listener added during the emit wait for the next one while a");
        impl_writer.line("/// listener removed during it still runs. `once` entries leave the live");
        impl_writer.line("/// list before any call, so a re-entrant `emit` cannot run one twice.");
        impl_writer.block(
            "pub fn emit(&self, event: &str, args: Vec<SmeltUnknown>) -> Result<bool, Box<dyn ::std::error::Error>>",
            |fn_writer| {
                fn_writer.line("let snapshot: Vec<SmeltEventListener> = self.listeners.borrow().iter().filter(|listener| listener.event == event).cloned().collect();");
                fn_writer.block("if snapshot.is_empty()", |arm_writer| {
                    arm_writer.line("return Ok(false);");
                });
                fn_writer.line("self.listeners.borrow_mut().retain(|listener| !(listener.event == event && listener.once));");
                fn_writer.block("for listener in snapshot", |loop_writer| {
                    loop_writer.line("(listener.callback)(args.clone())?;");
                });
                fn_writer.line("Ok(true)");
            },
        );
    });
    writer.blank_line();
}
