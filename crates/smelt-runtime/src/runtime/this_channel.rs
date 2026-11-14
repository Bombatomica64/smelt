// The dynamically scoped `this` channel.

// JavaScript `this` is supplied by the CALL, not by the definition site:
// the same plain function sees a different receiver depending on whether
// it was reached as `object.method()`, `fn.call(thisArg, ..)`, or a bare
// `fn()`. That is a genuinely dynamic binding, so it is modeled as a
// dynamically scoped channel rather than as a value threaded through the
// erased call ABI -- threading it would change the signature of every
// erased callable in the program for a feature only a handful of them
// read. `smelt_bind_this` installs a receiver for exactly one call and
// the guard restores the previous binding on scope exit (including on
// unwind), so the channel is always balanced.
// A receiver is either an erased value or a DEFERRED erasure of a
// concrete one: a call through a typed function-valued field
// (`router.match(..)`) installs a thunk, so the instance's erased view
// is built only if the callee actually reads `this`.
// @smelt:item SmeltThisBinding when this_channel
/// What `this` is bound to: an erased receiver, or the erasure of a concrete one, run on first read.
#[derive(Clone)]
enum SmeltThisBinding { Value(SmeltUnknown), Deferred(::std::rc::Rc<dyn Fn() -> SmeltUnknown>) }

thread_local! {
    //~ Receiver installed by the innermost active call, `undefined` when none.
    static SMELT_THIS: ::std::cell::RefCell<SmeltThisBinding> = ::std::cell::RefCell::new(SmeltThisBinding::Value(SmeltUnknown::Undefined));
}

/// Restores the previously installed `this` binding when dropped.
struct SmeltThisGuard { previous: SmeltThisBinding }

impl Drop for SmeltThisGuard {
    fn drop(&mut self) {
        let previous = ::std::mem::replace(&mut self.previous, SmeltThisBinding::Value(SmeltUnknown::Undefined));
        SMELT_THIS.with(|slot| { *slot.borrow_mut() = previous; });
    }
}

/// Install `receiver` as `this` until the returned guard is dropped.
fn smelt_push_this(receiver: SmeltUnknown) -> SmeltThisGuard {
    SmeltThisGuard { previous: SMELT_THIS.with(|slot| slot.replace(SmeltThisBinding::Value(receiver))) }
}

/// Install `undefined` as `this` for a plain call (`f(..)`) until the guard is dropped.
fn smelt_push_no_this() -> SmeltThisGuard { smelt_push_this(SmeltUnknown::Undefined) }

/// Install a concrete receiver as `this`, erased by `erase` only when first read.
#[allow(dead_code)]
fn smelt_push_this_lazy(erase: ::std::rc::Rc<dyn Fn() -> SmeltUnknown>) -> SmeltThisGuard {
    SmeltThisGuard { previous: SMELT_THIS.with(|slot| slot.replace(SmeltThisBinding::Deferred(erase))) }
}

/// Read the `this` receiver installed by the innermost active call.
fn smelt_this() -> SmeltUnknown {
    let binding = SMELT_THIS.with(|slot| slot.borrow().clone());
    match binding {
        SmeltThisBinding::Value(receiver) => receiver,
        SmeltThisBinding::Deferred(erase) => {
            let receiver = erase();
            SMELT_THIS.with(|slot| { *slot.borrow_mut() = SmeltThisBinding::Value(receiver.clone()); });
            receiver
        }
    }
}

/// Bind a receiver to an erased callable, as `Function.prototype.bind` does.
fn smelt_bind_this(callee: SmeltUnknown, receiver: SmeltUnknown) -> SmeltUnknown {
    match callee {
        SmeltUnknown::Function(function) => SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| {
            let _smelt_this_guard = smelt_push_this(receiver.clone());
            function(args)
        })),
        //~ A callable OBJECT (`__smelt_call`) keeps its property bag: rebuilding
        //~ the object with a bound `__smelt_call` preserves every other member,
        //~ which is what `throttle`/`debounce` return and then invoke as a method.
        SmeltUnknown::Object(object) => match object.get("__smelt_call") {
            Some(callable @ SmeltUnknown::Function(_)) => {
                let bound = object.clone();
                bound.insert("__smelt_call".to_owned(), smelt_bind_this(callable, receiver));
                SmeltUnknown::Object(bound)
            }
            _ => SmeltUnknown::Object(object),
        },
        other => other,
    }
}
// @smelt:item-end
