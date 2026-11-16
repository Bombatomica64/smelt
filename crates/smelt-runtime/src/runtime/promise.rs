// `SmeltPromise` and `SmeltFuture`: the promise runtime and the job queue.

// @smelt:item SmeltPromiseFuture when unknown
type SmeltPromiseFuture = ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<SmeltUnknown, Box<dyn std::error::Error>>>>>;

//~ # The job queue
//~
//~ JavaScript runs promise work on one FIFO job queue (the microtask queue):
//~ a reaction is queued when the promise it waits for settles, an `await`
//~ continuation likewise, and the queue runs to empty before any timer fires.
//~ Smelt keeps that queue here. A job is a boxed future; it is polled when it
//~ is queued and parked when it returns `Pending`, until whatever it waits for
//~ wakes it (`SmeltJobWaker`), which queues it again at the back. Nothing is
//~ ever polled to find out whether it can move: a settled promise wakes its
//~ waiters, a fired timer wakes its sleeper, so draining the queue costs one
//~ poll per step a job takes, and an idle loop costs nothing.
//~
//~ Code runs in one of two places. Inside a job's poll (`SMELT_JOB_DEPTH`
//~ above zero) an `await` of a pending promise parks the job, exactly as
//~ JavaScript suspends an async function. Outside every job (the module body,
//~ a test function, a server's connection task) there is no job to park, so
//~ an `await` runs the event loop itself (`smelt_run_until`) until the
//~ promise settles: the queue, then the timers in due order, then real I/O.
type SmeltJobFuture = ::std::pin::Pin<Box<dyn ::std::future::Future<Output = ()>>>;
struct SmeltJob {
    //~ `None` while the job is being polled.
    future: Option<SmeltJobFuture>,
    //~ Whether the job's id is in the ready queue, so a second wake does not
    //~ queue it twice.
    queued: bool,
    waker: ::std::task::Waker,
}
struct SmeltJobs {
    next_id: u64,
    //~ Every job that has not finished, by id.
    tasks: ::std::collections::HashMap<u64, SmeltJob>,
    //~ The ids of the jobs ready to run, in the order they became ready.
    ready: ::std::collections::VecDeque<u64>,
}
//~ The waker of one job: waking it queues the job. The queue is the thread's
//~ own (`SMELT_JOBS`); a generated program runs on one thread, so a wake always
//~ reaches the queue that owns the job.
struct SmeltJobWaker(u64);
impl ::std::task::Wake for SmeltJobWaker {
    fn wake(self: ::std::sync::Arc<Self>) { smelt_wake_job(self.0); }
    fn wake_by_ref(self: &::std::sync::Arc<Self>) { smelt_wake_job(self.0); }
}
thread_local! {
    static SMELT_JOBS: ::std::cell::RefCell<SmeltJobs> = ::std::cell::RefCell::new(SmeltJobs { next_id: 1, tasks: ::std::collections::HashMap::new(), ready: ::std::collections::VecDeque::new() });
    //~ How many jobs are being polled right now (a job's first step can run
    //~ inside another's poll: an async call made by an async body).
    static SMELT_JOB_DEPTH: ::std::cell::Cell<usize> = const { ::std::cell::Cell::new(0) };
    //~ In-flight I/O (an HTTP request) that will wake a job when it completes:
    //~ while any is out, an idle event loop waits for it instead of stopping.
    static SMELT_IO_IN_FLIGHT: ::std::cell::Cell<usize> = const { ::std::cell::Cell::new(0) };
    //~ Rejections settled since the last check, each with the handled flag of
    //~ its promise: reported once the job queue drains if the flag is still
    //~ unset, as Node reports an unhandled rejection.
    static SMELT_PENDING_REJECTIONS: ::std::cell::RefCell<Vec<(::std::rc::Rc<::std::cell::Cell<bool>>, SmeltUnknown)>> = const { ::std::cell::RefCell::new(Vec::new()) };
}

//~ Queue job `id` if it is alive and not queued already.
fn smelt_wake_job(id: u64) {
    let woke = SMELT_JOBS.try_with(|jobs| {
        let jobs = &mut *jobs.borrow_mut();
        match jobs.tasks.get_mut(&id) {
            Some(job) if !job.queued => { job.queued = true; jobs.ready.push_back(id); true }
            _ => false,
        }
    }).unwrap_or(false);
    /*@if timers*/if woke { smelt_loop_notify(); }/*@end*/
    let _ = woke;
}

//~ Add a job, queued (`queued`) or waiting for its first explicit run.
fn smelt_job_insert(task: SmeltJobFuture, queued: bool) -> u64 {
    let id = SMELT_JOBS.with(|jobs| {
        let jobs = &mut *jobs.borrow_mut();
        let id = jobs.next_id;
        jobs.next_id = jobs.next_id.saturating_add(1);
        let waker = ::std::task::Waker::from(::std::sync::Arc::new(SmeltJobWaker(id)));
        jobs.tasks.insert(id, SmeltJob { future: Some(task), queued, waker });
        if queued { jobs.ready.push_back(id); }
        id
    });
    /*@if timers*/if queued { smelt_loop_notify(); }/*@end*/
    id
}

//~ Queue a job at the back of the job queue.
fn smelt_spawn_promise_task(task: SmeltJobFuture) { let _ = smelt_job_insert(task, true); }

//~ Start a job now: its first step runs at once, inside the caller, and the
//~ rest when what it awaits wakes it. This is how calling an async function
//~ runs its body up to the first `await` before returning its promise.
fn smelt_spawn_now(task: SmeltJobFuture) { let id = smelt_job_insert(task, false); smelt_run_job(id); }

//~ Poll job `id` once, with its own waker. A finished job is removed and
//~ dropped outside the queue's borrow (dropping it may drop promises).
fn smelt_run_job(id: u64) {
    //~ Restores the depth on every exit from the poll.
    struct SmeltJobDepthGuard;
    impl Drop for SmeltJobDepthGuard { fn drop(&mut self) { SMELT_JOB_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1))); } }
    let taken = SMELT_JOBS.with(|jobs| {
        let mut jobs = jobs.borrow_mut();
        let job = jobs.tasks.get_mut(&id)?;
        job.queued = false;
        let future = job.future.take()?;
        Some((future, job.waker.clone()))
    });
    let Some((mut future, waker)) = taken else { return };
    SMELT_JOB_DEPTH.with(|depth| depth.set(depth.get().saturating_add(1)));
    let guard = SmeltJobDepthGuard;
    let ready = future.as_mut().poll(&mut ::std::task::Context::from_waker(&waker)).is_ready();
    drop(guard);
    if ready {
        let finished = SMELT_JOBS.with(|jobs| jobs.borrow_mut().tasks.remove(&id));
        drop(finished);
        drop(future);
    } else {
        let orphan = SMELT_JOBS.with(|jobs| match jobs.borrow_mut().tasks.get_mut(&id) { Some(job) => { job.future = Some(future); None } None => Some(future) });
        drop(orphan);
    }
}

//~ Run queued jobs, first in first out, until none is ready. Each job runs one
//~ step per wake; a job that parks is not polled again until it is woken.
fn smelt_run_jobs() {
    while let Some(id) = SMELT_JOBS.with(|jobs| jobs.borrow_mut().ready.pop_front()) { smelt_run_job(id); }
}

//~ Whether the caller is a job's poll (an `await` there parks the job).
fn smelt_in_job() -> bool { SMELT_JOB_DEPTH.with(::std::cell::Cell::get) > 0 }

//~ Drop every job and pending rejection: a test function starts from an empty
//~ event loop (`smelt_reset_timers`).
#[allow(dead_code)]
fn smelt_reset_jobs() {
    let dropped = SMELT_JOBS.with(|jobs| { let jobs = &mut *jobs.borrow_mut(); jobs.ready.clear(); ::std::mem::take(&mut jobs.tasks) });
    drop(dropped);
    SMELT_IO_IN_FLIGHT.with(|io| io.set(0));
    SMELT_PENDING_REJECTIONS.with(|pending| pending.borrow_mut().clear());
}

//~ One in-flight I/O operation, counted while it lives (`SMELT_IO_IN_FLIGHT`).
#[allow(dead_code)]
struct SmeltIoGuard;
#[allow(dead_code)]
impl SmeltIoGuard { fn new() -> Self { SMELT_IO_IN_FLIGHT.with(|io| io.set(io.get().saturating_add(1))); Self } }
impl Drop for SmeltIoGuard { fn drop(&mut self) { let _ = SMELT_IO_IN_FLIGHT.try_with(|io| io.set(io.get().saturating_sub(1))); } }

//~ A floating promise that rejects is an uncaught error, as in Node: it is
//~ reported and the process exits 1. Under `cargo test` it is only reported.
#[allow(dead_code)]
fn smelt_report_unhandled_rejection(error: &(dyn ::std::error::Error + 'static)) {
    eprintln!("{error:?}");
    if !cfg!(test) { ::std::process::exit(1); }
}

//~ Record a settled rejection and the flag that says whether anything handles
//~ it. Typed and erased promises both land here: one rule decides what is
//~ unhandled, applied when the job queue drains (`smelt_check_rejections`).
#[allow(dead_code)]
fn smelt_note_rejection(handled: ::std::rc::Rc<::std::cell::Cell<bool>>, reason: SmeltUnknown) {
    SMELT_PENDING_REJECTIONS.with(|pending| pending.borrow_mut().push((handled, reason)));
}

//~ Judge the rejections noted so far: one still unhandled is reported (and
//~ the process exits 1, as in Node); a handled one is dropped. Called only
//~ when the job queue has drained outside every job, so a handler attached
//~ later in the same turn or in a job counts, and one attached in a later
//~ timer does not.
fn smelt_check_rejections() {
    let noted = SMELT_PENDING_REJECTIONS.with(|pending| ::std::mem::take(&mut *pending.borrow_mut()));
    for (handled, reason) in noted {
        if !handled.get() {
            smelt_report_unhandled_rejection(&*smelt_throw(reason));
        }
    }
}

//~ Run the event loop from outside every job until `done()` holds: the queued
//~ jobs, then (with timers) the earliest due timers, then real I/O, in Node's
//~ order. Answers `false` when the loop is idle and `done()` still does not
//~ hold: no job is queued, no timer is pending and no I/O or open handle can
//~ wake one, so nothing can ever make it hold.
async fn smelt_run_until(done: &dyn Fn() -> bool) -> bool {
    loop {
        smelt_run_jobs();
        smelt_check_rejections();
        if done() { return true; }
// @smelt:if timers
        if smelt_fire_next_timers() {
            //~ Another task of the host runtime (a server's connection) may be
            //~ waiting for its turn; a timer step is the loop's turn boundary.
            tokio::task::yield_now().await;
            continue;
        }
        if SMELT_IO_IN_FLIGHT.with(::std::cell::Cell::get) > 0 || smelt_live_handles() > 0 {
            //~ Wait on the host runtime until a job is queued or a few
            //~ milliseconds pass: it completes the I/O, and a listening server
            //~ keeps the loop alive this way.
            let wake = SMELT_LOOP_WAKE.with(::std::rc::Rc::clone);
            tokio::select! { _ = wake.notified() => {}, _ = tokio::time::sleep(::std::time::Duration::from_millis(5)) => {} }
            continue;
        }
// @smelt:end
        return false;
    }
}

//~ An `await` outside every job of a promise nothing can settle any more. Node
//~ stops the process with exit code 13 and a warning for such a top-level
//~ `await`; under `cargo test` the awaiting test fails with this error instead.
fn smelt_unsettled_await() -> Box<dyn std::error::Error> {
    if !cfg!(test) {
        eprintln!("Warning: Detected unsettled top-level await");
        ::std::process::exit(13);
    }
    smelt_throw(SmeltUnknown::String("the awaited promise can never settle: the event loop is idle".into()))
}

//~ A promise reached through an erased value: a typed promise of the erased
//~ carrier with the object identity JavaScript gives every promise (`===`
//~ compares `id`).
#[derive(Clone)]
pub struct SmeltPromise {
    id: usize,
    //~ The settled state's error slot is the *same* exception-payload ABI
    //~ `smelt_throw`/`smelt_thrown_value` define (see `thrown.rs`), not a new
    //~ erasure: a JavaScript rejection reason is any value at all, so it has
    //~ no static type to preserve here (`Promise.reject({ status: 400 })` must
    //~ keep `status`).
    future: SmeltFuture<SmeltUnknown>,
}

// @smelt:item-end

// @smelt:item impl_SmeltPromise
// @smelt:if unknown
impl SmeltPromise {
    //~ An erased promise over `future`, with a fresh identity.
    fn of(future: SmeltFuture<SmeltUnknown>) -> Self { Self { id: smelt_next_object_id(), future } }
    //~ An already-fulfilled erased promise value.
    fn resolved(value: SmeltUnknown) -> Self { Self::of(SmeltFuture::resolved(value)) }
// @smelt:end
// @smelt:if vitest_mock
    //~ `SmeltPromise::rejected` is only referenced by the Vitest mock
    //~ runtime (`mockRejectedValue*`), so it is gated on the same flag.
    //~ An already-rejected erased promise value. The rejection reason is kept
    //~ whole and re-enters the error channel unchanged on await, so a
    //~ non-`Error` reason keeps its own properties.
    fn rejected(value: SmeltUnknown) -> Self { Self::of(SmeltFuture::settled(Err(value))) }
// @smelt:end
// @smelt:if unknown
    //~ A promise whose result `future` computes when something first awaits or
    //~ reacts to it (an adapter: an erased generator step, a body read).
    fn from_future(future: SmeltPromiseFuture) -> Self { Self::of(SmeltFuture::from_future(future)) }
    //~ The settled result now, if this promise has settled.
    #[allow(dead_code)]
    fn smelt_settled(&self) -> Option<Result<SmeltUnknown, SmeltUnknown>> { self.future.smelt_settled() }
    //~ A reaction (`then`/`catch`/`finally`) on this promise: `future` computes
    //~ the derived promise's result (`SmeltFuture::reaction`).
    #[allow(dead_code)]
    fn smelt_react(&self, future: SmeltPromiseFuture) -> SmeltUnknown { SmeltUnknown::Promise(Self::of(self.future.reaction(future))) }
    //~ `await` of this promise (it handles the promise's rejection).
    fn smelt_await(&self) -> ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<SmeltUnknown, Box<dyn std::error::Error>>> + '_>> { Box::pin(self.future.smelt_await()) }
    //~ This promise's settled result inside one of its own reactions: read
    //~ without the `await` tick, since the reaction job is that tick.
    #[allow(dead_code)]
    fn smelt_reaction_read(&self) -> ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<SmeltUnknown, Box<dyn std::error::Error>>> + '_>> { Box::pin(self.future.smelt_reaction_read()) }
}
// @smelt:end
// @smelt:item-end

// @smelt:item impl_std_fmt_Debug_for_SmeltPromise when unknown
impl ::std::fmt::Debug for SmeltPromise { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_struct("SmeltPromise").field("id", &self.id).finish() } }

//~ JavaScript `await` flattens: awaiting a value that is itself a promise
//~ resolves through the whole chain. An `async` function that returns a
//~ `Promise` erases that inner promise to `SmeltUnknown::Promise`, so a
//~ consumer that awaits the function's result must keep awaiting while the
//~ settled value is still a promise. A no-op pass-through for any
//~ non-promise value.
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
//~ `smelt_get_unknown_field` (radash's `guard`, H66). A handler's returned
//~ promise is flattened (`smelt_await_flatten`), so `then` never answers
//~ `Promise<Promise<T>>`.
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
        "then" => { let source = promise.clone(); ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| { let reader = source.clone(); let on_fulfilled = smelt_args.first().cloned(); let on_rejected = smelt_args.get(1).cloned(); Ok(source.smelt_react(Box::pin(async move { match reader.smelt_reaction_read().await { Ok(value) => smelt_promise_continue(on_fulfilled, value).await, Err(error) => match on_rejected { Some(handler) => smelt_promise_continue(Some(handler), smelt_thrown_value(&*error)).await, None => Err(error) } } }))) }) }
        "catch" => { let source = promise.clone(); ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| { let reader = source.clone(); let on_rejected = smelt_args.first().cloned(); Ok(source.smelt_react(Box::pin(async move { match reader.smelt_reaction_read().await { Ok(value) => Ok(value), Err(error) => smelt_promise_continue(on_rejected, smelt_thrown_value(&*error)).await } }))) }) }
        "finally" => { let source = promise.clone(); ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| { let reader = source.clone(); let on_settled = smelt_args.first().cloned(); Ok(source.smelt_react(Box::pin(async move { let settled = reader.smelt_reaction_read().await; if let Some(SmeltUnknown::Function(handler)) = on_settled { (handler)(::std::vec![])?; } settled }))) }) }
        _ => return SmeltUnknown::Undefined,
    };
    SmeltUnknown::Function(member)
}

//~ Generic promise-value ABI. A source `Promise<T>` / `Type::Future(T)`
//~ lowers to `SmeltFuture<T>` in *every* position (parameter, field,
//~ return, local, async-op result), so the same MIR future type renders
//~ one Rust type everywhere. The handle is shared (`Rc`), so it is cheaply
//~ `Clone` (a JavaScript promise is one object however many bindings hold
//~ it) and `Default` (a ready default value). A promise settles once; every
//~ `await` and reaction reads that one result.
#[allow(dead_code)]
enum SmeltFutureState<T> {
    //~ An adapter promise whose body has not started: it starts when
    //~ something first awaits or reacts to it.
    Pending(::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<T, Box<dyn std::error::Error>>>>>),
    //~ Running: a job, or the awaiter driving an adapter, settles it.
    Running,
    Resolved(T),
    //~ A rejection, as the thrown payload `smelt_throw`/`smelt_thrown_value`
    //~ define (see `thrown.rs`): every `await` re-observes the same reason,
    //~ so it is a value an error can be rebuilt from, not the error itself.
    Rejected(SmeltUnknown),
}
//~ What a promise does for each waiter when it settles, in registration
//~ order: a reaction's job is queued, a parked `await` is woken (which queues
//~ its job). One list keeps JavaScript's order between the two.
enum SmeltWaiter {
    Reaction(SmeltJobFuture),
    Await(::std::task::Waker),
    //~ Runs at the moment of settling: an erased view settles with its
    //~ source, being the same JavaScript promise, so its own waiters are
    //~ released in the source's turn.
    Mirror(Box<dyn FnOnce()>),
}
pub struct SmeltFuture<T> {
    state: ::std::rc::Rc<::std::cell::RefCell<SmeltFutureState<T>>>,
    waiters: ::std::rc::Rc<::std::cell::RefCell<Vec<SmeltWaiter>>>,
    //~ Whether anything handles this promise's rejection: a reaction or an
    //~ `await`. Read when the job queue drains to decide whether a rejection
    //~ that settled is unhandled (`smelt_note_rejection`).
    handled: ::std::rc::Rc<::std::cell::Cell<bool>>,
    //~ Whether a rejection here is noted for that check. An erased view of a
    //~ typed promise shares the source's `handled` flag and is not noted: one
    //~ JavaScript promise is reported once.
    notes_rejection: bool,
    //~ The erased promise this future crosses an erased boundary as, made on
    //~ the first erasure and handed out by every later one: one JavaScript
    //~ promise is one object, so `const u: unknown = p; u === (p as unknown)`
    //~ holds and both views share one settled result.
    erased: ::std::rc::Rc<::std::cell::RefCell<Option<SmeltPromise>>>,
}
impl<T> Clone for SmeltFuture<T> { fn clone(&self) -> Self { Self { state: self.state.clone(), waiters: self.waiters.clone(), handled: self.handled.clone(), notes_rejection: self.notes_rejection, erased: self.erased.clone() } } }
//~ `===` on promises is identity: two handles are equal when they are the
//~ same promise (one shared settle state).
impl<T> PartialEq for SmeltFuture<T> { fn eq(&self, other: &Self) -> bool { ::std::rc::Rc::ptr_eq(&self.state, &other.state) } }
impl<T: Default> Default for SmeltFuture<T> { fn default() -> Self { Self::resolved(T::default()) } }
impl<T> ::std::fmt::Debug for SmeltFuture<T> { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str("SmeltFuture") } }
#[allow(dead_code)]
impl<T> SmeltFuture<T> {
    //~ A handle over `state` with no waiter.
    fn with_state(state: SmeltFutureState<T>) -> Self { Self { state: ::std::rc::Rc::new(::std::cell::RefCell::new(state)), waiters: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())), handled: ::std::rc::Rc::new(::std::cell::Cell::new(false)), notes_rejection: true, erased: ::std::rc::Rc::new(::std::cell::RefCell::new(None)) } }
    //~ An adapter promise (an `await` flattening, a coercion, `Promise.all`):
    //~ its body starts when something first awaits or reacts to it.
    fn from_future(future: ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<T, Box<dyn std::error::Error>>>>>) -> Self { Self::with_state(SmeltFutureState::Pending(future)) }
    //~ The promise of an async function, method or closure call, or of an
    //~ executor's async body: its body becomes a job and runs at once up to its
    //~ first suspension, as JavaScript runs an async function synchronously to
    //~ its first `await`. A body that throws before then rejects the promise;
    //~ the throw is observed through the promise, never at the call.
    fn from_future_primed(future: ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<T, Box<dyn std::error::Error>>>>>) -> Self where T: 'static {
        let promise = Self::with_state(SmeltFutureState::Running);
        let settle = promise.smelt_detached();
        smelt_spawn_now(Box::pin(async move { let settled = future.await; settle.smelt_settle_result(settled); }));
        promise
    }
    //~ An already-fulfilled promise.
    fn resolved(value: T) -> Self { Self::with_state(SmeltFutureState::Resolved(value)) }
    //~ A promise settled now with `settled` (`Err` holds the thrown payload).
    //~ A rejection is noted for the unhandled-rejection check.
    fn settled(settled: Result<T, SmeltUnknown>) -> Self {
        let promise = Self::with_state(SmeltFutureState::Running);
        promise.smelt_settle(settled);
        promise
    }
    //~ `Promise.reject(reason)`: a promise rejected now with the thrown error.
    fn rejected(error: &(dyn std::error::Error + 'static)) -> Self { Self::settled(Err(smelt_thrown_value(error))) }
    //~ `new Promise(executor)`: a pending promise and the handle its `resolve`
    //~ and `reject` settle it through (the first settle wins).
    fn with_resolvers() -> (Self, Self) { let promise = Self::with_state(SmeltFutureState::Running); let settle = promise.smelt_detached(); (promise, settle) }
    //~ An erased value recovered as a typed promise (`u as Promise<T>`): an
    //~ erased promise is the same promise seen at `T` (`smelt_view`), each
    //~ settled value converted by `convert`, and a value settled with a
    //~ further promise flattens through it as `await` does; any other value
    //~ is a promise already fulfilled with it.
    fn from_erased(value: SmeltUnknown, convert: impl FnOnce(SmeltUnknown) -> Result<T, Box<dyn std::error::Error>> + 'static) -> Self where T: 'static {
        let SmeltUnknown::Promise(promise) = value else {
            let promise = Self::with_state(SmeltFutureState::Running);
            promise.smelt_settle_result(convert(value));
            return promise;
        };
        promise.future.smelt_view(move |settled, view: &Self| match settled {
            Ok(SmeltUnknown::Promise(inner)) => { let target = view.smelt_detached(); smelt_spawn_promise_task(Box::pin(async move { let flattened = smelt_await_flatten(SmeltUnknown::Promise(inner)).await; target.smelt_settle_result(flattened.and_then(convert)); })); }
            Ok(value) => view.smelt_settle_result(convert(value)),
            Err(reason) => view.smelt_settle(Err(reason)),
        })
    }
    //~ Another handle on this future without its erased view: what a job that
    //~ settles the future captures, so the future and its view do not hold
    //~ each other alive through the view cache.
    fn smelt_detached(&self) -> Self { Self { state: self.state.clone(), waiters: self.waiters.clone(), handled: self.handled.clone(), notes_rejection: self.notes_rejection, erased: ::std::rc::Rc::new(::std::cell::RefCell::new(None)) } }
    //~ The settled result now, if this future has settled.
    fn smelt_settled(&self) -> Option<Result<T, SmeltUnknown>> where T: Clone {
        match &*self.state.borrow() {
            SmeltFutureState::Resolved(value) => Some(Ok(value.clone())),
            SmeltFutureState::Rejected(payload) => Some(Err(payload.clone())),
            _ => None,
        }
    }
    fn smelt_is_settled(&self) -> bool { matches!(&*self.state.borrow(), SmeltFutureState::Resolved(_) | SmeltFutureState::Rejected(_)) }
    //~ Settle with `settled`, unless already settled (the first settle wins,
    //~ as a promise's `resolve`/`reject` do), and release the waiters in order.
    fn smelt_settle(&self, settled: Result<T, SmeltUnknown>) {
        if self.smelt_is_settled() { return; }
        if self.notes_rejection && let Err(reason) = &settled { smelt_note_rejection(self.handled.clone(), reason.clone()); }
        *self.state.borrow_mut() = match settled { Ok(value) => SmeltFutureState::Resolved(value), Err(reason) => SmeltFutureState::Rejected(reason) };
        let waiters = ::std::mem::take(&mut *self.waiters.borrow_mut());
        for waiter in waiters {
            match waiter {
                SmeltWaiter::Reaction(job) => smelt_spawn_promise_task(job),
                SmeltWaiter::Await(waker) => waker.wake(),
                SmeltWaiter::Mirror(settle) => settle(),
            }
        }
    }
    //~ Settle with a body's result, its error as the thrown payload.
    fn smelt_settle_result(&self, settled: Result<T, Box<dyn std::error::Error>>) { self.smelt_settle(settled.map_err(|error| smelt_thrown_value(&*error))); }
    //~ Start an adapter's body on a job of its own, if it has not started.
    fn smelt_drive(&self) where T: Clone + 'static {
        let body = { let mut state = self.state.borrow_mut(); if !matches!(&*state, SmeltFutureState::Pending(_)) { return; } match ::std::mem::replace(&mut *state, SmeltFutureState::Running) { SmeltFutureState::Pending(body) => body, _ => return } };
        let settle = self.smelt_detached();
        smelt_spawn_promise_task(Box::pin(async move { let settled = body.await; settle.smelt_settle_result(settled); }));
    }
    //~ Run `job` when this future settles: now (queued) if it has, otherwise
    //~ when it does, after the jobs queued by then. Starts an adapter's body.
    fn smelt_when_settled_run(&self, job: SmeltJobFuture) where T: Clone + 'static {
        if self.smelt_is_settled() { smelt_spawn_promise_task(job); return; }
        self.waiters.borrow_mut().push(SmeltWaiter::Reaction(job));
        self.smelt_drive();
    }
    //~ This promise seen at another type: a view that settles at the moment
    //~ this one does (`settle_view` decides its result from this one's), shares
    //~ whether the rejection is handled and is never noted itself, since it is
    //~ the same JavaScript promise. Its own waiters are therefore released in
    //~ this promise's turn, not a job later.
    fn smelt_view<U: 'static>(&self, settle_view: impl FnOnce(Result<T, SmeltUnknown>, &SmeltFuture<U>) + 'static) -> SmeltFuture<U> where T: Clone + 'static {
        let view = SmeltFuture::<U> { state: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltFutureState::Running)), waiters: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())), handled: self.handled.clone(), notes_rejection: false, erased: ::std::rc::Rc::new(::std::cell::RefCell::new(None)) };
        if let Some(settled) = self.smelt_settled() {
            settle_view(settled, &view);
        } else {
            let target = view.smelt_detached();
            let source = self.smelt_detached();
            self.waiters.borrow_mut().push(SmeltWaiter::Mirror(Box::new(move || { if let Some(settled) = source.smelt_settled() { settle_view(settled, &target); } })));
            self.smelt_drive();
        }
        view
    }
    //~ The promise this future crosses an erased boundary as, its settled value
    //~ erased by `erase`: this promise's view (`smelt_view`), made once
    //~ (`erased`), so the erased promise keeps the source promise's identity.
    fn smelt_promise_with(self, erase: impl FnOnce(T) -> SmeltUnknown + 'static) -> SmeltPromise where T: Clone + 'static {
        if let Some(view) = self.erased.borrow().as_ref() { return view.clone(); }
        let view = SmeltPromise::of(self.smelt_view(move |settled, view| view.smelt_settle(settled.map(erase))));
        *self.erased.borrow_mut() = Some(view.clone());
        view
    }
    //~ A reaction (`then`/`catch`/`finally`) on this future: `body` computes
    //~ the derived future's result, reading this one with
    //~ `smelt_reaction_read`. Its job is queued now when this future has
    //~ settled, and otherwise when it settles, after the jobs queued by then;
    //~ typed and erased reactions share the one queue. A reaction handles the
    //~ source's rejection.
    fn reaction<U: Clone + 'static>(&self, body: ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<U, Box<dyn std::error::Error>>>>>) -> SmeltFuture<U> where T: Clone + 'static {
        self.handled.set(true);
        let derived = SmeltFuture::with_state(SmeltFutureState::Running);
        let settle = derived.smelt_detached();
        self.smelt_when_settled_run(Box::pin(async move { let settled = body.await; settle.smelt_settle_result(settled); }));
        derived
    }
    //~ `await` of this promise. It handles the promise's rejection. Inside a
    //~ job it suspends the job: one tick (the job is queued again at the back)
    //~ when the promise has settled, and until it settles otherwise, as
    //~ JavaScript queues an `await` continuation as a job. An adapter whose
    //~ body has not started is driven here, by its awaiter. Outside every job
    //~ the event loop runs until the promise settles.
    async fn smelt_await(&self) -> Result<T, Box<dyn std::error::Error>> where T: Clone + 'static {
        self.handled.set(true);
        self.smelt_wait(true).await
    }
    //~ The settled result inside a reaction of this future: the reaction's own
    //~ job is the tick an `await` takes, so a settled result is read at once.
    fn smelt_reaction_read(&self) -> impl ::std::future::Future<Output = Result<T, Box<dyn std::error::Error>>> + '_ where T: Clone + 'static { self.smelt_wait(false) }
    //~ Wait for the settled result; `tick` yields one job turn first when it
    //~ has already settled.
    async fn smelt_wait(&self, tick: bool) -> Result<T, Box<dyn std::error::Error>> where T: Clone + 'static {
        let body = { let mut state = self.state.borrow_mut(); if matches!(&*state, SmeltFutureState::Pending(_)) { match ::std::mem::replace(&mut *state, SmeltFutureState::Running) { SmeltFutureState::Pending(body) => Some(body), _ => None } } else { None } };
        if let Some(body) = body {
            //~ Drive the adapter here: inside a job its own awaits park this
            //~ job, outside every job they run the event loop (the exit drain
            //~ is such an adapter, and it must run outside every job).
            let settled = body.await;
            self.smelt_settle_result(settled);
        } else if !smelt_in_job() {
            if !smelt_run_until(&|| self.smelt_is_settled()).await { return Err(smelt_unsettled_await()); }
        } else {
            let mut ticked = !tick;
            let mut parked = false;
            ::std::future::poll_fn(|cx| {
                if self.smelt_is_settled() {
                    if ticked { return ::std::task::Poll::Ready(()); }
                    ticked = true;
                    cx.waker().wake_by_ref();
                    return ::std::task::Poll::Pending;
                }
                ticked = true;
                if !parked { parked = true; self.waiters.borrow_mut().push(SmeltWaiter::Await(cx.waker().clone())); }
                ::std::task::Poll::Pending
            }).await;
        }
        self.smelt_settled().map_or_else(|| Err(smelt_unsettled_await()), |settled| settled.map_err(smelt_throw))
    }
}
//~ `SmeltFuture<T>` is `IntoFuture`, so an ordinary Rust `.await` on a
//~ promise value is a JavaScript `await` (`smelt_await`).
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
//~ erased promise is the same promise (`smelt_promise_with`).
impl<T: IntoSmeltUnknown + Clone + 'static> IntoSmeltUnknown for SmeltFuture<T> {
    fn into_smelt_unknown(self) -> SmeltUnknown { SmeltUnknown::Promise(self.smelt_promise_with(IntoSmeltUnknown::into_smelt_unknown)) }
}

//~ `Promise.race`: a promise that settles like the first of `racers` to
//~ settle. Each racer gets a reaction, as JavaScript's `race` calls `then` on
//~ each: the reaction of the racer that settles first runs first and settles
//~ the race, the rest find it settled. Losing racers keep running.
#[allow(dead_code)]
fn smelt_promise_race<T: Clone + 'static>(racers: Vec<SmeltFuture<T>>) -> SmeltFuture<T> {
    let (race, settle) = SmeltFuture::with_resolvers();
    for racer in racers {
        let settle = settle.clone();
        let source = racer.smelt_detached();
        racer.handled.set(true);
        racer.smelt_when_settled_run(Box::pin(async move { if let Some(settled) = source.smelt_settled() { settle.smelt_settle(settled); } }));
    }
    race
}

// @smelt:item-end

// @smelt:item smelt_settled_result when all_settled
//~ One entry of `Promise.allSettled`: the record JavaScript builds for a
//~ settled input, `{ status: "fulfilled", value }` or `{ status: "rejected",
//~ reason }`. The value arrives erased because the record's own type is the
//~ erased carrier wherever `PromiseSettledResult<T>` lowers to it; a program
//~ whose settled-result type is concrete converts the record to it at the call
//~ site. The rejection reason is the thrown payload (`smelt_thrown_value`),
//~ kept whole as `Promise.reject` keeps it.
#[allow(dead_code)]
fn smelt_settled_result(settled: Result<SmeltUnknown, Box<dyn std::error::Error>>) -> SmeltUnknown {
    let entries = match settled {
        Ok(value) => ::std::vec![("status", SmeltUnknown::String("fulfilled".into())), ("value", value)],
        Err(error) => ::std::vec![("status", SmeltUnknown::String("rejected".into())), ("reason", smelt_thrown_value(&*error))],
    };
    SmeltUnknown::Object(SmeltObject::new(entries))
}

// @smelt:item-end
