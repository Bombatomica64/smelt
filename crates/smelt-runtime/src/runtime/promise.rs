// `SmeltPromise` and `SmeltFuture`: the promise runtime.

// @smelt:item SmeltPromiseFuture when unknown
type SmeltPromiseFuture = ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<SmeltUnknown, Box<dyn std::error::Error>>>>>;

//~ JS eager-async-prefix semantics. Calling an `async` function (or running
//~ a `new Promise` executor) executes SYNCHRONOUSLY up to the first `await`;
//~ only the continuation after that suspension point is deferred to a later
//~ microtask. Smelt models a promise value as a lazily-stored future that is
//~ not driven until `smelt_await`, which would instead defer the ENTIRE body
//~ (including its synchronous prefix). That breaks any code whose observable
//~ ordering depends on all prefixes running before the event loop turns —
//~ e.g. `Promise.all([api.call(), api.call(), api.call()])` where each
//~ `call` synchronously registers a request with a batch/funnel scheduler:
//~ lazily, the first `await` inside `Promise.all` drains a timer after only
//~ the first prefix ran, splitting one batch into three.
//~
//~ `smelt_eager_poll_waker` is a no-op `Waker` used to poll a freshly
//~ constructed future exactly once at construction time, advancing it
//~ through its synchronous prefix up to (and not past) the first real
//~ suspension. The poll's result is folded into the promise's shared settle
//~ state (see `SmeltFuture::from_future_primed`); a
//~ still-`Pending` future is kept and later resumed by `smelt_await` under
//~ the real executor waker. Because the no-op waker never schedules a wake,
//~ it is only ever valid for this single priming poll.
//~
//~ Error observability contract: a synchronous prefix that throws lowers to
//~ a future that resolves `Poll::Ready(Err(_))` on its first poll. Per JS,
//~ an exception thrown before the first `await` of an `async` function
//~ becomes a REJECTED promise, not a synchronous throw, and its rejection is
//~ observable only through the normal await/handler path in microtask order.
//~ The priming poll therefore CAPTURES a `Ready(Err)` into the shared
//~ rejected settle state and NEVER propagates it out of `from_future`;
//~ `smelt_await` surfaces it exactly when a consumer awaits, preserving when
//~ rejections become observable relative to other continuations and timers.
fn smelt_eager_poll_waker() -> ::std::task::Waker {
    unsafe fn clone(_: *const ()) -> ::std::task::RawWaker { raw() }
    unsafe fn wake(_: *const ()) {}
    unsafe fn wake_by_ref(_: *const ()) {}
    unsafe fn drop(_: *const ()) {}
    fn raw() -> ::std::task::RawWaker { ::std::task::RawWaker::new(::std::ptr::null(), &::std::task::RawWakerVTable::new(clone, wake, wake_by_ref, drop)) }
    unsafe { ::std::task::Waker::from_raw(raw()) }
}

#[derive(Clone)]
pub struct SmeltPromise {
    id: usize,
    //~ The settled state's error slot is the *same* exception-payload ABI
    //~ `smelt_throw`/`smelt_thrown_value` define (see `thrown.rs`), not a new
    //~ erasure: a JavaScript rejection reason is any value at all, so it has
    //~ no static type to preserve here (`Promise.reject({ status: 400 })` must
    //~ keep `status`). Since the payload arrives as a `SmeltUnknown` and leaves
    //~ as one, storing it as a `SmeltUnknown` keeps it whole across the settle
    //~ boundary.
    state: ::std::rc::Rc<::std::cell::RefCell<Option<Result<SmeltUnknown, SmeltUnknown>>>>,
    future: ::std::rc::Rc<::std::cell::RefCell<Option<SmeltPromiseFuture>>>,
    //~ Whether anything handles this promise's rejection: a `then`/`catch`/
    //~ `finally` reaction or an `await`. Shared by every handle of the promise,
    //~ read when the job queue drains to decide whether a rejection that
    //~ settled is unhandled (`smelt_note_rejection`).
    handled: ::std::rc::Rc<::std::cell::Cell<bool>>,
    //~ Reaction jobs waiting for this promise to settle, in registration
    //~ order: released into the job queue when it settles (`smelt_settle`), so
    //~ a reaction on a pending promise runs after the jobs already queued, as
    //~ in JavaScript's single FIFO job queue.
    reactions: ::std::rc::Rc<::std::cell::RefCell<Vec<::std::pin::Pin<Box<dyn ::std::future::Future<Output = ()>>>>>>,
    //~ Whether a job is already driving this promise's future, so a reaction
    //~ registered on it does not start a second driver.
    driven: ::std::rc::Rc<::std::cell::Cell<bool>>,
    //~ Its reaction job is waiting for its source to settle: until the job
    //~ runs, another awaiter waits for it rather than driving the future out
    //~ of the queue's order.
    parked: ::std::rc::Rc<::std::cell::Cell<bool>>,
}

// @smelt:item-end

// @smelt:item impl_SmeltPromise
// @smelt:if unknown
impl SmeltPromise {
    //~ Create a pending erased promise identity with shared settle state.
    fn pending() -> Self { Self { id: smelt_next_object_id(), state: ::std::rc::Rc::new(::std::cell::RefCell::new(None)), future: ::std::rc::Rc::new(::std::cell::RefCell::new(None)), handled: ::std::rc::Rc::new(::std::cell::Cell::new(false)), reactions: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())), driven: ::std::rc::Rc::new(::std::cell::Cell::new(false)), parked: ::std::rc::Rc::new(::std::cell::Cell::new(false)) } }
    //~ Create a pending promise with a preassigned identity.
    fn pending_with_id(id: usize) -> Self { Self { id, state: ::std::rc::Rc::new(::std::cell::RefCell::new(None)), future: ::std::rc::Rc::new(::std::cell::RefCell::new(None)), handled: ::std::rc::Rc::new(::std::cell::Cell::new(false)), reactions: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())), driven: ::std::rc::Rc::new(::std::cell::Cell::new(false)), parked: ::std::rc::Rc::new(::std::cell::Cell::new(false)) } }
    //~ Create an already-fulfilled erased promise value.
    fn resolved(value: SmeltUnknown) -> Self { Self { id: smelt_next_object_id(), state: ::std::rc::Rc::new(::std::cell::RefCell::new(Some(Ok(value)))), future: ::std::rc::Rc::new(::std::cell::RefCell::new(None)), handled: ::std::rc::Rc::new(::std::cell::Cell::new(false)), reactions: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())), driven: ::std::rc::Rc::new(::std::cell::Cell::new(false)), parked: ::std::rc::Rc::new(::std::cell::Cell::new(false)) } }
// @smelt:end
// @smelt:if vitest_mock
    //~ `SmeltPromise::rejected` is only referenced by the Vitest mock
    //~ runtime (`mockRejectedValue*`), so it is gated on the same flag to
    //~ keep every non-mock crate's prelude byte-identical.
    //~ Create an already-rejected erased promise value. The rejection
    //~ reason is kept whole in the shared settle state and re-enters the
    //~ error channel unchanged on await, so a non-`Error` reason keeps its
    //~ own properties (JavaScript rejects with any value, not a message).
    fn rejected(value: SmeltUnknown) -> Self { Self { id: smelt_next_object_id(), state: ::std::rc::Rc::new(::std::cell::RefCell::new(Some(Err(value)))), future: ::std::rc::Rc::new(::std::cell::RefCell::new(None)), handled: ::std::rc::Rc::new(::std::cell::Cell::new(false)), reactions: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())), driven: ::std::rc::Rc::new(::std::cell::Cell::new(false)), parked: ::std::rc::Rc::new(::std::cell::Cell::new(false)) } }
// @smelt:end
// @smelt:if unknown
    //~ Store a live future behind a cloneable erased promise handle. This
    //~ is the lazy constructor used by derived/adapter promises (await
    //~ flattening, `.then`/`.catch` chains, coercions): the future is not
    //~ driven until `smelt_await`, matching JS deferral of those.
    fn from_future(future: SmeltPromiseFuture) -> Self { Self { id: smelt_next_object_id(), state: ::std::rc::Rc::new(::std::cell::RefCell::new(None)), future: ::std::rc::Rc::new(::std::cell::RefCell::new(Some(future))), handled: ::std::rc::Rc::new(::std::cell::Cell::new(false)), reactions: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())), driven: ::std::rc::Rc::new(::std::cell::Cell::new(false)), parked: ::std::rc::Rc::new(::std::cell::Cell::new(false)) } }
    //~ A promise whose future runs now, as JavaScript runs an async function
    //~ and a promise reaction whether or not anything awaits the result: the
    //~ future is queued as a promise task (the event loop and the exit drain
    //~ run those), which settles it without handling it, so a rejection
    //~ nobody handles is reported. Every handle shares the settled result.
    //~ Without an event loop (no `timers` helpers) it stays lazy, run by the
    //~ code that awaits it.
    #[allow(dead_code)]
    fn scheduled(future: SmeltPromiseFuture) -> Self {
        let promise = Self::from_future(future);
        /*@if timers*/promise.smelt_drive();/*@end*/
        promise
    }
    //~ A promise already rejected with `reason` (an erased view of a typed
    //~ future that settled by rejecting). Nothing handles it yet: it is noted
    //~ for the check when the job queue drains, like any settled rejection.
    #[allow(dead_code)]
    fn settled_rejection(reason: SmeltUnknown) -> Self {
        let promise = Self::pending();
        /*@if timers*/smelt_note_rejection(promise.handled.clone(), reason.clone());/*@end*/
        *promise.state.borrow_mut() = Some(Err(reason));
        promise
    }
    //~ Queue one job that drives this promise's future to its settled result,
    //~ unless one is already queued or the promise has no future of its own.
    #[allow(dead_code)]
    fn smelt_drive(&self) {
        if self.driven.get() || self.future.borrow().is_none() { return; }
        self.driven.set(true);
        let driver = self.clone();
        /*@if timers*/smelt_spawn_promise_task(Box::pin(async move { let _ = driver.smelt_settle().await; }));/*@end*/
    }
    //~ A reaction (`then`/`catch`/`finally`) on this promise: `future` computes
    //~ the derived promise's result. Its job is queued now when this promise
    //~ has settled, and otherwise when it settles, after the jobs queued by
    //~ then, which is JavaScript's order. Without an event loop the derived
    //~ promise stays lazy and `future` drives this one when awaited.
    #[allow(dead_code)]
    fn smelt_react(&self, future: SmeltPromiseFuture) -> SmeltUnknown {
        let derived = Self::from_future(future);
        /*@if timers*/{ derived.driven.set(true); let driver = derived.clone(); let job: ::std::pin::Pin<Box<dyn ::std::future::Future<Output = ()>>> = Box::pin(async move { driver.parked.set(false); let _ = driver.smelt_settle().await; }); if self.state.borrow().is_some() { smelt_spawn_promise_task(job); } else { derived.parked.set(true); self.reactions.borrow_mut().push(job); self.smelt_drive(); } }/*@end*/
        SmeltUnknown::Promise(derived)
    }
    //~ Await the stored future once and share its settled result with clones.
    //~ A consumer awaiting the promise handles its rejection: the flag is set
    //~ when the await is set up, then the promise is driven to its result.
    fn smelt_await(&self) -> ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<SmeltUnknown, Box<dyn std::error::Error>>> + '_>> {
        self.handled.set(true);
        Box::pin(self.smelt_settle())
    }
    //~ Drive the promise to its settled result without handling it: the
    //~ runtime's own scheduler task uses this, so a rejection nobody else
    //~ handles stays unhandled. A rejection it settles is noted for the check
    //~ when the job queue drains.
    async fn smelt_settle(&self) -> Result<SmeltUnknown, Box<dyn std::error::Error>> {
        if self.state.borrow().is_none() && !self.parked.get() {
            //~ Bind the taken future to a local before awaiting so no `RefMut` from
            //~ `borrow_mut()` stays alive across the `.await` (pre-2024-edition
            //~ temporary-scope rules would otherwise keep it borrowed and panic on
            //~ re-entry through the shared cell).
            let taken = self.future.borrow_mut().take();
            if let Some(future) = taken {
                let settled = future.await.map_err(|error| smelt_thrown_value(&*error));
                /*@if timers*/if let Err(reason) = &settled { smelt_note_rejection(self.handled.clone(), reason.clone()); }/*@end*/
                *self.state.borrow_mut() = Some(settled);
                /*@if timers*/{ let released = ::std::mem::take(&mut *self.reactions.borrow_mut()); for job in released { smelt_spawn_promise_task(job); } }/*@end*/
            }
        }
        loop {
            if let Some(result) = self.state.borrow().clone() {
                return result.map_err(smelt_throw);
            }
// @smelt:end
// @smelt:if timers
            //~ The result cell is still empty: another task (or a timer callback) must
            //~ settle it. When timer helpers exist, drive the cooperative scheduler and
            //~ virtual clock the same way the spin-wait futures do (H1/H3) so a future
            //~ handed to the detached promise-task queue, or one whose resolution
            //~ depends on a pending timer, cannot spin forever with nothing advancing
            //~ time. Without timer helpers there is no timer queue or detached-task
            //~ queue to drive, so a bare cooperative yield is the only (and correct)
            //~ driver, and the helper symbols are not emitted to reference.
            //~ Inside a promise task's poll the outer drain is still running the
            //~ queue: only yield to it (below) rather than advancing time, so no
            //~ timer fires before the queue has drained.
            if SMELT_PROMISE_TASK_DEPTH.with(::std::cell::Cell::get) == 0 { smelt_sleep_ms(0.0).await; }
// @smelt:end
// @smelt:if unknown
            tokio::task::yield_now().await;
        }
    }
}
// @smelt:end
// @smelt:item-end

// @smelt:item impl_std_fmt_Debug_for_SmeltPromise when unknown
impl ::std::fmt::Debug for SmeltPromise { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_struct("SmeltPromise").field("id", &self.id).finish() } }

//~ JavaScript `await` flattens: awaiting a value that is itself a promise
//~ resolves through the whole chain. An `async` function that returns a
//~ `Promise` erases that inner promise to `SmeltUnknown::Promise`, so a
//~ consumer that awaits the function's result must keep awaiting while the
//~ settled value is still a promise. This helper drives the shared cell of
//~ each erased promise (which in turn drives the cooperative scheduler),
//~ and is a no-op pass-through for any non-promise value.
#[allow(dead_code)]
async fn smelt_await_flatten(value: SmeltUnknown) -> Result<SmeltUnknown, Box<dyn std::error::Error>> {
    let mut current = value;
    while let SmeltUnknown::Promise(promise) = current {
        current = promise.smelt_await().await?;
    }
    Ok(current)
}

//~ `promise.then(..)`, `.catch(..)` and `.finally(..)` reached through an
//~ ERASED receiver. The typed spelling lowers to an `AsyncOp` in the
//~ frontend, but a promise that arrives as `SmeltUnknown` — the result of
//~ calling an erased callable, which is what a generic
//~ `TFunction extends () => any` parameter is — reads its member through
//~ `smelt_get_unknown_field`, and that answered `undefined`: the
//~ continuation silently vanished and the chain's value became a default
//~ (radash's `guard`, whose `func()` result is erased, answered its
//~ fallback for every input; H66).
//~
//~ The derived promise is lazy exactly like every other one here: the
//~ future is not driven until `smelt_await`, which is what JavaScript
//~ does too. A handler's returned promise is flattened
//~ (`smelt_await_flatten`), so `then` never answers `Promise<Promise<T>>`.
/// Run one promise continuation handler over a settled value.
//~
//~ A missing or non-callable handler passes the value through, which is
//~ what `promise.then(undefined)` does in JavaScript.
#[allow(dead_code)]
async fn smelt_promise_continue(handler: Option<SmeltUnknown>, value: SmeltUnknown) -> Result<SmeltUnknown, Box<dyn std::error::Error>> {
    match handler {
        Some(SmeltUnknown::Function(handler)) => smelt_await_flatten((handler)(::std::vec![value])?).await,
        _ => Ok(value),
    }
}


/// Read a modeled member off an erased promise (`then`/`catch`/`finally`).
#[allow(dead_code)]
fn smelt_promise_member(promise: &SmeltPromise, field: &str) -> SmeltUnknown {
    let member: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = match field {
        "then" => { promise.handled.set(true); let source = promise.clone(); let reaction_source = promise.clone(); ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| { let source = source.clone(); let on_fulfilled = smelt_args.first().cloned(); let on_rejected = smelt_args.get(1).cloned(); Ok(reaction_source.smelt_react(Box::pin(async move { match source.smelt_await().await { Ok(value) => smelt_promise_continue(on_fulfilled, value).await, Err(error) => match on_rejected { Some(handler) => smelt_promise_continue(Some(handler), smelt_thrown_value(&*error)).await, None => Err(error) } } }))) }) }
        "catch" => { promise.handled.set(true); let source = promise.clone(); let reaction_source = promise.clone(); ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| { let source = source.clone(); let on_rejected = smelt_args.first().cloned(); Ok(reaction_source.smelt_react(Box::pin(async move { match source.smelt_await().await { Ok(value) => Ok(value), Err(error) => smelt_promise_continue(on_rejected, smelt_thrown_value(&*error)).await } }))) }) }
        "finally" => { promise.handled.set(true); let source = promise.clone(); let reaction_source = promise.clone(); ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| { let source = source.clone(); let on_settled = smelt_args.first().cloned(); Ok(reaction_source.smelt_react(Box::pin(async move { let settled = source.smelt_await().await; if let Some(SmeltUnknown::Function(handler)) = on_settled { (handler)(::std::vec![])?; } settled }))) }) }
        _ => return SmeltUnknown::Undefined,
    };
    SmeltUnknown::Function(member)
}

//~ Generic promise-value ABI. A source `Promise<T>` / `Type::Future(T)`
//~ lowers to `SmeltFuture<T>` in *every* position (parameter, field,
//~ return, local, async-op result), so the same MIR future type renders
//~ one Rust type everywhere instead of a bare
//~ `Pin<Box<dyn Future>>` in some positions and a value in others. The
//~ handle is a shared `Rc<RefCell<..>>` so it is cheaply `Clone`
//~ (matching JS promise-value copy semantics) and `Default` (a ready
//~ default value). `smelt_await` drives the stored future once, caches the
//~ resolved value, and serves later awaits from that cache — JS promises
//~ may be awaited multiple times, and each await after the first resolves
//~ from the cached value (single-consumer of the underlying future).
#[allow(dead_code)]
enum SmeltFutureState<T> {
    Pending(::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<T, Box<dyn std::error::Error>>>>>),
    Resolved(T),
    //~ A synchronous prefix that threw during eager priming (see
    //~ `smelt_eager_poll_waker`) is stored as a rejection. JS promises may be
    //~ awaited more than once and each await re-observes the SAME rejection,
    //~ so the state has to hold something `smelt_await` can rebuild an error
    //~ from on every call — which is why it is a value and not the
    //~ `Box<dyn Error>` itself. It holds the thrown payload
    //~ `smelt_throw`/`smelt_thrown_value` already define (see `thrown.rs`),
    //~ not a new erasure: `async () => { throw "oops"; }` hands its `catch` the
    //~ string.
    Rejected(SmeltUnknown),
    Taken,
}
//~ A priming poll must not own the virtual clock. `from_future_primed`
//~ runs an async body's synchronous prefix at call time; in JavaScript
//~ that prefix only *schedules* its timers, it does not make time pass.
//~ The virtual-clock sleep helper advances the clock to its own deadline
//~ when it is driven, so without this marker priming
//~ `withTimeout(() => delay(1000), 50)`'s `run()` jumped the clock 1000ms
//~ before the 50ms deadline was even armed and the timeout could never
//~ win. This is the same rule `SMELT_RACE_DEPTH` already states for a
//~ `Promise.race` driver, applied to the other place that polls a future
//~ out of band.
thread_local! {
    //~ Non-zero while `SmeltFuture::from_future_primed` is running its
    //~ eager prefix poll; see `smelt_sleep_ms`.
    static SMELT_PRIME_DEPTH: ::std::cell::Cell<usize> = const { ::std::cell::Cell::new(0) };
}
pub struct SmeltFuture<T> {
    state: ::std::rc::Rc<::std::cell::RefCell<SmeltFutureState<T>>>,
    //~ Reaction jobs waiting for this future to settle, released into the job
    //~ queue in registration order when it does (see `SmeltPromise`'s field
    //~ of the same name): typed and erased reactions share one FIFO queue.
    reactions: ::std::rc::Rc<::std::cell::RefCell<Vec<::std::pin::Pin<Box<dyn ::std::future::Future<Output = ()>>>>>>,
    //~ Whether a job is already driving this future.
    driven: ::std::rc::Rc<::std::cell::Cell<bool>>,
    //~ An `await` of it yields one microtask tick before reading the settled
    //~ value (`Promise.resolve(x)`): settled for its reactions at once, as in
    //~ JavaScript, while `await Promise.resolve()` still lets queued jobs run.
    tick: ::std::rc::Rc<::std::cell::Cell<bool>>,
    //~ Its reaction job is waiting for its source to settle: until the job
    //~ runs, another awaiter waits for it rather than driving the future out
    //~ of the queue's order.
    parked: ::std::rc::Rc<::std::cell::Cell<bool>>,
}
impl<T> Clone for SmeltFuture<T> { fn clone(&self) -> Self { Self { state: self.state.clone(), reactions: self.reactions.clone(), driven: self.driven.clone(), tick: self.tick.clone(), parked: self.parked.clone() } } }
impl<T: Default> Default for SmeltFuture<T> { fn default() -> Self { Self::resolved(T::default()) } }
impl<T> ::std::fmt::Debug for SmeltFuture<T> { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str("SmeltFuture") } }
#[allow(dead_code)]
impl<T> SmeltFuture<T> {
    //~ Wrap a live future behind a cloneable promise-value handle, lazily:
    //~ the future is not driven until `smelt_await`. Used by derived and
    //~ adapter promises (await flattening, `.then`/`.catch` chains, callback
    //~ coercions, `Promise.all` collection) whose bodies JS also defers,
    //~ so priming them would run continuations/handlers out of microtask
    //~ order — see `from_future_primed` for the eager-prefix constructor.
    fn from_future(future: ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<T, Box<dyn std::error::Error>>>>>) -> Self { Self::with_state(SmeltFutureState::Pending(future)) }
    //~ A handle over `state` with no reaction waiting and no driver.
    fn with_state(state: SmeltFutureState<T>) -> Self { Self { state: ::std::rc::Rc::new(::std::cell::RefCell::new(state)), reactions: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())), driven: ::std::rc::Rc::new(::std::cell::Cell::new(false)), tick: ::std::rc::Rc::new(::std::cell::Cell::new(false)), parked: ::std::rc::Rc::new(::std::cell::Cell::new(false)) } }
    //~ Wrap an ASYNC-FUNCTION-BODY future, priming it with a single no-op-
    //~ waker poll so its synchronous prefix runs at call time (JS
    //~ eager-async-prefix semantics; see `smelt_eager_poll_waker`). Only
    //~ genuine async function / async closure / async method bodies use this,
    //~ so a call like `api.call()` registers its request synchronously before
    //~ the event loop turns — the difference that keeps
    //~ `Promise.all([call(), call(), call()])` from splitting a funnel/batch.
    //~ A prefix that completes resolves now; one that throws is captured as a
    //~ rejection surfaced only via `smelt_await` (JS: a throw before the first
    //~ await becomes a rejected promise, observable only through the
    //~ await/handler path); one that suspends keeps the advanced future for
    //~ later resume. Derived/adapter promises deliberately do NOT use this,
    //~ preserving when their continuations and rejections become observable.
    fn from_future_primed(mut future: ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<T, Box<dyn std::error::Error>>>>>) -> Self {
        struct SmeltPrimeGuard;
        impl Drop for SmeltPrimeGuard { fn drop(&mut self) { SMELT_PRIME_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1))); } }
        SMELT_PRIME_DEPTH.with(|depth| depth.set(depth.get().saturating_add(1)));
        let _smelt_prime_guard = SmeltPrimeGuard;
        let waker = smelt_eager_poll_waker();
        let mut cx = ::std::task::Context::from_waker(&waker);
        let state = match ::std::future::Future::poll(future.as_mut(), &mut cx) {
            ::std::task::Poll::Ready(Ok(value)) => SmeltFutureState::Resolved(value),
            ::std::task::Poll::Ready(Err(error)) => SmeltFutureState::Rejected(smelt_thrown_value(&*error)),
            ::std::task::Poll::Pending => SmeltFutureState::Pending(future),
        };
        Self::with_state(state)
    }
    //~ Build an already-resolved promise-value handle.
    fn resolved(value: T) -> Self { Self::with_state(SmeltFutureState::Resolved(value)) }
    //~ `Promise.resolve(value)`: settled now, so a reaction on it is queued at
    //~ once; an `await` of it still yields one microtask tick first.
    fn resolved_after_tick(value: T) -> Self { let future = Self::with_state(SmeltFutureState::Resolved(value)); future.tick.set(true); future }
    //~ The settled result now, if this future has settled: what an erased
    //~ view of it starts from, and what decides whether a reaction is queued
    //~ at once or waits.
    fn smelt_settled(&self) -> Option<Result<T, SmeltUnknown>> where T: Clone {
        match &*self.state.borrow() {
            SmeltFutureState::Resolved(value) => Some(Ok(value.clone())),
            SmeltFutureState::Rejected(payload) => Some(Err(payload.clone())),
            _ => None,
        }
    }
    //~ The promise this future crosses an erased boundary as, its
    //~ settled value erased by `erase`. A future that has already settled (an
    //~ async body that finished synchronously) gives an already-settled
    //~ promise, so a reaction registered on it is queued at once, as in
    //~ JavaScript; a pending one gives a promise driven by its own job.
    fn smelt_promise_with(self, erase: impl FnOnce(T) -> SmeltUnknown + 'static) -> SmeltPromise where T: Clone + 'static {
        match self.smelt_settled() {
            Some(Ok(value)) => SmeltPromise::resolved(erase(value)),
            Some(Err(reason)) => SmeltPromise::settled_rejection(reason),
            None => SmeltPromise::scheduled(Box::pin(async move { let value = self.smelt_await().await?; Ok::<SmeltUnknown, Box<dyn std::error::Error>>(erase(value)) })),
        }
    }
    //~ Queue one job that drives this future to its settled result, unless one
    //~ is already queued or it has no future left to drive.
    fn smelt_drive(&self) where T: Clone + 'static {
        if self.driven.get() || !matches!(&*self.state.borrow(), SmeltFutureState::Pending(_)) { return; }
        self.driven.set(true);
        let driver = self.clone();
        /*@if timers*/smelt_spawn_promise_task(Box::pin(async move { let _ = driver.smelt_await().await; }));/*@end*/
    }
    //~ Wait for this future to settle without taking it: its own driver job
    //~ runs it (`smelt_drive`), and this only reads the result. A
    //~ `Promise.race` waits on its racers this way, so dropping the wait of a
    //~ losing racer leaves that promise running, as in JavaScript, and a racer
    //~ settles on its own job when its timer releases it.
    fn smelt_when_settled(self) -> ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<T, Box<dyn std::error::Error>>>>> where T: Clone + 'static {
        self.smelt_drive();
        Box::pin(async move {
            loop {
                match &*self.state.borrow() {
                    SmeltFutureState::Resolved(value) => return Ok(value.clone()),
                    SmeltFutureState::Rejected(payload) => return Err(smelt_throw(payload.clone())),
                    _ => {}
                }
                tokio::task::yield_now().await;
            }
        })
    }
    //~ A reaction (`then`/`catch`/`finally`) on this future: `body` computes
    //~ the derived future's result (awaiting this one). Its job is queued now
    //~ when this future has settled, and otherwise when it settles, after the
    //~ jobs queued by then; typed and erased reactions share the one queue.
    //~ Without an event loop the derived future stays lazy, driven by whoever
    //~ awaits it.
    fn reaction<U: Clone + 'static>(&self, body: ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<U, Box<dyn std::error::Error>>>>>) -> SmeltFuture<U> where T: Clone + 'static {
        let derived = SmeltFuture::from_future(body);
        //~ The reaction reads its settled source directly: the `await` tick of
        //~ `Promise.resolve` is for code awaiting it, not for its reactions.
        self.tick.set(false);
        /*@if timers*/{ derived.driven.set(true); let driver = derived.clone(); let job: ::std::pin::Pin<Box<dyn ::std::future::Future<Output = ()>>> = Box::pin(async move { driver.parked.set(false); let _ = driver.smelt_await().await; }); if self.smelt_settled().is_some() { smelt_spawn_promise_task(job); } else { derived.parked.set(true); self.reactions.borrow_mut().push(job); self.smelt_drive(); } }/*@end*/
        derived
    }
    //~ Drive the stored future once, cache its resolved value, and serve
    //~ later awaits from that cache (single-consumer of the future).
    async fn smelt_await(&self) -> Result<T, Box<dyn std::error::Error>> where T: Clone {
        /*@if timers*/if self.tick.replace(false) { smelt_sleep_ms(0.0).await; }/*@end*/
        let pending = {
            let mut guard = self.state.borrow_mut();
            if matches!(&*guard, SmeltFutureState::Pending(_)) && !self.parked.get() {
                match ::std::mem::replace(&mut *guard, SmeltFutureState::Taken) {
                    SmeltFutureState::Pending(future) => Some(future),
                    _ => None,
                }
            } else { None }
        };
        if let Some(future) = pending {
            //~ The settled result is stored either way (a rejection too, so a
            //~ later await re-observes it) and the waiting reactions released.
            let settled = future.await;
            *self.state.borrow_mut() = match &settled { Ok(value) => SmeltFutureState::Resolved(value.clone()), Err(error) => SmeltFutureState::Rejected(smelt_thrown_value(&**error)) };
            /*@if timers*/{ let released = ::std::mem::take(&mut *self.reactions.borrow_mut()); for job in released { smelt_spawn_promise_task(job); } }/*@end*/
            return settled;
        }
        loop {
            match &*self.state.borrow() {
                SmeltFutureState::Resolved(value) => return Ok(value.clone()),
                SmeltFutureState::Rejected(payload) => return Err(smelt_throw(payload.clone())),
                _ => {}
            }
            //~ Another job is driving the future (`Taken`): wait for it to
            //~ settle. Inside a promise task's poll that is a yield back to the
            //~ drain; at the top level, one tick of the event loop.
            /*@if timers*/if SMELT_PROMISE_TASK_DEPTH.with(::std::cell::Cell::get) == 0 { smelt_sleep_ms(0.0).await; }/*@end*/
            tokio::task::yield_now().await;
        }
    }
}
//~ `SmeltFuture<T>` is `IntoFuture`, so an ordinary Rust `.await` on a
//~ promise value drives it through `smelt_await` — every generated `.await`
//~ site works on the wrapper without a bespoke call form. The underlying
//~ futures are `'static` (the prior `Pin<Box<dyn Future>>` ABI was too), so
//~ `T: 'static` here matches existing constraints.
impl<T: Clone + 'static> ::std::future::IntoFuture for SmeltFuture<T> {
    type Output = Result<T, Box<dyn std::error::Error>>;
    type IntoFuture = ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<T, Box<dyn std::error::Error>>>>>;
    fn into_future(self) -> Self::IntoFuture { Box::pin(async move { self.smelt_await().await }) }
}
//~ Erasing a typed promise value (`SmeltFuture<T>`) to the dynamic carrier
//~ yields a `SmeltUnknown::Promise`: a JS `Promise<T>` flowing into an
//~ `unknown`/erased position is still a promise object, not its resolved
//~ value. This is a genuine dynamic boundary — the erased consumer only
//~ knows the promise/thenable protocol, so no concrete `T` survives. The
//~ adapter defers exactly like JS: it wraps a fresh `SmeltPromise` whose
//~ body awaits this future and erases the settled value through
//~ `IntoSmeltUnknown`, matching the `SmeltUnknown::Promise(SmeltPromise::
//~ from_future(..))` shape the future-recovery coercion emits. This impl
//~ must exist whenever generated code can call `.into_smelt_unknown()` on a
//~ future — e.g. the erased-callback promise adapter and the recover-erased-
//~ promise-on-await coercion both do — and it lives in this same
//~ `SmeltUnknown`/`SmeltPromise` prelude region so the gate is shared.
impl<T: IntoSmeltUnknown + Clone + 'static> IntoSmeltUnknown for SmeltFuture<T> {
    fn into_smelt_unknown(self) -> SmeltUnknown { SmeltUnknown::Promise(self.smelt_promise_with(IntoSmeltUnknown::into_smelt_unknown)) }
}

// @smelt:item-end
