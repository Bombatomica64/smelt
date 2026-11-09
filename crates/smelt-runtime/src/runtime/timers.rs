// The timer queue, promise task queue and exit drain.

// @smelt:item SmeltTimer when timers
struct SmeltTimer {
    id: u64,
    due_ms: u64,
    callback: ::std::rc::Rc<::std::cell::RefCell<dyn FnMut() -> Result<(), Box<dyn std::error::Error>>>>,
    //~ `Some(period)` marks a repeating `setInterval` timer that re-arms
    //~ itself `period` ms after each fire; `None` is a one-shot `setTimeout`.
    period_ms: Option<u64>,
}

thread_local! {
    static SMELT_NEXT_TIMER_ID: ::std::cell::Cell<u64> = const { ::std::cell::Cell::new(1) };
    static SMELT_TIMERS: ::std::cell::RefCell<Vec<SmeltTimer>> = const { ::std::cell::RefCell::new(Vec::new()) };
    //~ Whether a detached promise task finished since the event loop last checked for idleness.
    static SMELT_PROMISE_TASK_SETTLED: ::std::cell::Cell<bool> = const { ::std::cell::Cell::new(false) };
    static SMELT_PROMISE_TASKS: ::std::cell::RefCell<Vec<::std::pin::Pin<Box<dyn ::std::future::Future<Output = ()>>>>> = const { ::std::cell::RefCell::new(Vec::new()) };
    //~ Non-zero while a `Promise.race` driver owns the event loop; see
    //~ `smelt_promise_race`.
    static SMELT_RACE_DEPTH: ::std::cell::Cell<usize> = const { ::std::cell::Cell::new(0) };
    //~ Wakes the exit loop (`smelt_run_until_exit`) when work is queued
    //~ while the program is only being kept alive by an open handle.
    static SMELT_LOOP_WAKE: ::std::rc::Rc<tokio::sync::Notify> = ::std::rc::Rc::new(tokio::sync::Notify::new());
}

fn smelt_reset_timers() {
    SMELT_NEXT_TIMER_ID.with(|next| next.set(1));
    SMELT_VIRTUAL_MS.with(|virtual_ms| virtual_ms.set(0));
    SMELT_TIMER_EPOCH.with(|epoch| epoch.set(None));
    SMELT_TIMERS.with(|timers| timers.borrow_mut().clear());
    SMELT_PROMISE_TASKS.with(|tasks| tasks.borrow_mut().clear());
    SMELT_RACE_DEPTH.with(|depth| depth.set(0));
    SMELT_PRIME_DEPTH.with(|depth| depth.set(0));
}

fn smelt_noop_waker() -> ::std::task::Waker {
    unsafe fn clone(_: *const ()) -> ::std::task::RawWaker { smelt_raw_waker() }
    //~ A promise task the drain left pending is polled again on the next
    //~ loop turn. While the program is serving (`smelt_run_until_exit`
    //~ waits on `SMELT_LOOP_WAKE`), a task that becomes ready has to say
    //~ so, or it sits until the 5 ms tick: under load that idled the
    //~ server between requests even though no request needed the time.
    //~ Waking only notifies the serving loop; outside it nothing awaits
    //~ the notification, so the drain's own order is unchanged.
    unsafe fn wake(_: *const ()) { let _ = SMELT_LOOP_WAKE.try_with(|wake| wake.notify_one()); }
    unsafe fn wake_by_ref(_: *const ()) { let _ = SMELT_LOOP_WAKE.try_with(|wake| wake.notify_one()); }
    unsafe fn drop(_: *const ()) {}
    fn smelt_raw_waker() -> ::std::task::RawWaker { ::std::task::RawWaker::new(::std::ptr::null(), &::std::task::RawWakerVTable::new(clone, wake, wake_by_ref, drop)) }
    unsafe { ::std::task::Waker::from_raw(smelt_raw_waker()) }
}

fn smelt_spawn_promise_task(task: ::std::pin::Pin<Box<dyn ::std::future::Future<Output = ()>>>) {
    SMELT_PROMISE_TASKS.with(|tasks| tasks.borrow_mut().push(task));
    SMELT_LOOP_WAKE.with(|wake| wake.notify_one());
}

async fn smelt_drain_promise_tasks() {
    for _ in 0..64 {
        let mut tasks = SMELT_PROMISE_TASKS.with(|tasks| ::std::mem::take(&mut *tasks.borrow_mut()));
        if tasks.is_empty() { break; }
        let waker = smelt_noop_waker();
        let mut cx = ::std::task::Context::from_waker(&waker);
        let mut pending = Vec::new();
        for mut task in tasks.drain(..) {
            if task.as_mut().poll(&mut cx).is_pending() { pending.push(task); } else { SMELT_PROMISE_TASK_SETTLED.with(|settled| settled.set(true)); }
        }
        let had_pending = !pending.is_empty();
        SMELT_PROMISE_TASKS.with(|tasks| tasks.borrow_mut().extend(pending));
        if !had_pending { break; }
        tokio::task::yield_now().await;
    }
}

fn smelt_set_timeout(callback: ::std::rc::Rc<::std::cell::RefCell<dyn FnMut() -> Result<(), Box<dyn std::error::Error>>>>, delay_ms: f64) -> SmeltUnknown {
    let id = SMELT_NEXT_TIMER_ID.with(|next| { let id = next.get(); next.set(id.saturating_add(1)); id });
    let delay_ms = if delay_ms.is_finite() && delay_ms > 0.0 { delay_ms as u64 } else { 0 };
    let due_ms = smelt_mono_ms().saturating_add(delay_ms);
    SMELT_TIMERS.with(|timers| timers.borrow_mut().push(SmeltTimer { id, due_ms, callback, period_ms: None }));
    SmeltUnknown::Number(id as f64)
}

fn smelt_set_interval(callback: ::std::rc::Rc<::std::cell::RefCell<dyn FnMut() -> Result<(), Box<dyn std::error::Error>>>>, period_ms: f64) -> SmeltUnknown {
    let id = SMELT_NEXT_TIMER_ID.with(|next| { let id = next.get(); next.set(id.saturating_add(1)); id });
    //~ Clamp non-positive periods to 1 ms so an interval still advances virtual
    //~ time and cannot busy-loop the drain at the current instant.
    let period_ms = if period_ms.is_finite() && period_ms > 0.0 { period_ms as u64 } else { 1 };
    let due_ms = smelt_mono_ms().saturating_add(period_ms);
    SMELT_TIMERS.with(|timers| timers.borrow_mut().push(SmeltTimer { id, due_ms, callback, period_ms: Some(period_ms) }));
    SmeltUnknown::Number(id as f64)
}

fn smelt_clear_timeout<T: IntoSmeltUnknown>(handle: T) {
    let SmeltUnknown::Number(id) = handle.into_smelt_unknown() else { return; };
    let id = id as u64;
    SMELT_TIMERS.with(|timers| timers.borrow_mut().retain(|timer| timer.id != id));
}

//~ `clearInterval` cancels by id exactly like `clearTimeout`; both share
//~ the single timer queue. Emit it as a thin alias so the generated
//~ `smelt_clear_interval` (the CLEAR_INTERVAL symbol) call site resolves to a real helper.
fn smelt_clear_interval<T: IntoSmeltUnknown>(handle: T) { smelt_clear_timeout(handle) }

//~ `id_barrier` defers timers created *after* the current drain began:
//~ a timer whose id is at or above the barrier was (re)scheduled during
//~ this drain pass and, like a Node timer scheduled inside the timer
//~ phase, must wait for a later tick rather than firing again now. This
//~ stops a self-rearming `setTimeout(cb, 0)` (e.g. a funnel's 0 ms
//~ interval) from firing repeatedly within one `sleep`.
fn smelt_drain_due_timers(id_barrier: u64) {
    loop {
        let now = smelt_mono_ms();
        let due = SMELT_TIMERS.with(|timers| {
            let mut timers = timers.borrow_mut();
            let mut due = Vec::new();
            let mut pending = Vec::new();
            for timer in timers.drain(..) {
                if timer.due_ms <= now && timer.id < id_barrier { due.push(timer); } else { pending.push(timer); }
            }
            *timers = pending;
            due
        });
        if due.is_empty() { break; }
        for timer in due {
            (&mut *timer.callback.borrow_mut())().unwrap_or_else(|error| smelt_panic_throw(error));
            //~ Re-arm repeating `setInterval` timers for their next period. The
            //~ next fire is scheduled `period` ms from the current virtual time, so
            //~ it is strictly in the future and cannot re-fire within this drain pass.
            if let Some(period_ms) = timer.period_ms {
                let next_due = now.saturating_add(period_ms);
                SMELT_TIMERS.with(|timers| timers.borrow_mut().push(SmeltTimer { id: timer.id, due_ms: next_due, callback: timer.callback.clone(), period_ms: Some(period_ms) }));
            }
        }
    }
}

async fn smelt_sleep_ms(delay_ms: f64) {
    //~ Suspend immediately under an eager prefix poll, before touching the
    //~ clock. `from_future_primed` runs an async body's synchronous prefix
    //~ at call time; in JavaScript that prefix schedules its timers but
    //~ does not make time pass, whereas this helper advances virtual time
    //~ to its own deadline as soon as it is driven. Yielding here leaves
    //~ the prefix's effects in place and defers all timekeeping to the
    //~ first real poll, so a long sleep started inside a primed body
    //~ cannot outrun deadlines armed after it (see `SMELT_PRIME_DEPTH`).
    //~
    //~ It stays suspended for as long as ANY prime is running, not for
    //~ one poll. Primes nest (an async body calls an async function that
    //~ calls another), and each enclosing prime re-polls the futures it
    //~ awaits within the same synchronous pass, so a single yield was
    //~ spent by the second layer and the sleep advanced the clock inside
    //~ the prefix. Hono's `timeout` middleware armed its 1000ms deadline,
    //~ called `next()`, and the six-deep prime under it slept the
    //~ handler's 1100ms, firing the deadline before `Promise.race`
    //~ existed; the race then saw the handler resolved and returned 200.
    while SMELT_PRIME_DEPTH.with(::std::cell::Cell::get) > 0 { tokio::task::yield_now().await; }
    smelt_drain_promise_tasks().await;
    let delay_ms = if delay_ms.is_finite() && delay_ms > 0.0 { delay_ms as u64 } else { 0 };
    let target_ms = smelt_mono_ms().saturating_add(delay_ms);
    //~ Under a `Promise.race` driver a positive sleep must not move the
    //~ clock itself: firing every timer due within its own window would
    //~ also fire the OTHER racers' timers (a `setTimeout(reject, 50)`
    //~ timeout loses to the `sleep(110)` racer it should beat, because
    //~ both settle in one poll and the earlier-listed racer is read
    //~ first). Instead the sleep arms a no-op wake-up timer at its
    //~ deadline, so the driver sees it among the pending timers and steps
    //~ the clock to it in order, and suspends until virtual time has
    //~ reached the deadline. This is Node's model: a sleep IS a timer.
    if delay_ms > 0 && SMELT_RACE_DEPTH.with(::std::cell::Cell::get) > 0 {
        let id = SMELT_NEXT_TIMER_ID.with(|next| { let id = next.get(); next.set(id.saturating_add(1)); id });
        let callback: ::std::rc::Rc<::std::cell::RefCell<dyn FnMut() -> Result<(), Box<dyn std::error::Error>>>> = ::std::rc::Rc::new(::std::cell::RefCell::new(|| Ok(())));
        SMELT_TIMERS.with(|timers| timers.borrow_mut().push(SmeltTimer { id, due_ms: target_ms, callback, period_ms: None }));
        while smelt_mono_ms() < target_ms { tokio::task::yield_now().await; }
        smelt_drain_promise_tasks().await;
        return;
    }
    //~ For a zero-delay sleep (one event-loop tick), only timers that
    //~ already exist when it begins may fire; anything (re)scheduled while
    //~ draining is deferred to a later tick, exactly as Node runs a timer
    //~ scheduled inside the timer phase on the next turn. A positive-delay
    //~ sleep spans a real window and must fire every timer that becomes due
    //~ within it, including ones scheduled mid-window (e.g. a recursive
    //~ debounce re-arming itself), so it uses no barrier.
    let id_barrier = if delay_ms == 0 { SMELT_NEXT_TIMER_ID.with(::std::cell::Cell::get) } else { u64::MAX };
    //~ Fire every timer due within the requested window, advancing virtual
    //~ time to each in turn and draining the microtask queue between fires.
    let mut fired_any = false;
    loop {
        let next_due = SMELT_TIMERS.with(|timers| timers.borrow().iter().filter(|timer| timer.due_ms <= target_ms && timer.id < id_barrier).map(|timer| timer.due_ms).min());
        let Some(next_due) = next_due else { break; };
        fired_any = true;
        smelt_virtual_advance_to(next_due);
        smelt_drain_due_timers(id_barrier);
        smelt_drain_promise_tasks().await;
    }
    smelt_virtual_advance_to(target_ms);
    //~ Node-style run-until-idle. A zero-delay sleep is what the generated
    //~ promise-executor spin loops await while waiting for a result cell to
    //~ be settled (e.g. by a `setTimeout(resolve, 100)` timer). With no
    //~ window to fire into, virtual time would never reach that timer and
    //~ the spin loop would starve forever. When the requested window has no
    //~ due timer AND no detached promise task is still runnable (the
    //~ microtask queue is drained, so nothing else can make progress), the
    //~ event loop is idle: advance virtual time to the earliest pending
    //~ timer and fire it, exactly as Node advances to its timer phase once
    //~ microtasks settle. Repeat until either the queue has runnable work
    //~ again, a timer settles the awaited state, or no timers remain.
    //~
    //~ This applies only to a zero-delay sleep. A positive-delay sleep has
    //~ a real deadline: it must fire exactly the timers due within its
    //~ window (handled above) and must not jump the clock to a later timer,
    //~ or a bounded `await delay(35)` would over-fire a `setInterval`.
    //~
    //~ It also applies only when the window fired nothing. If the window
    //~ already ran a due timer, progress was made this tick and control
    //~ must return so the caller's spin loop can re-check its awaited
    //~ state; jumping ahead to fire a timer that was (re)scheduled during
    //~ this drain — e.g. a `setInterval(_, 0)` re-arming itself — would
    //~ over-fire it a tick early and, for a funnel, collapse the burst to
    //~ idle before the next `call`.
    'idle: {
        //~ A `Promise.race` driver owns the clock while it is running: if a
        //~ racer advanced time here, polling one racer could fire ANOTHER
        //~ racer's timer, so both settle in the same round and the winner
        //~ stops being the one that finished first. Yield instead and let
        //~ `smelt_promise_race` take exactly one timer step per round.
        if delay_ms != 0 || fired_any || SMELT_RACE_DEPTH.with(::std::cell::Cell::get) > 0 { break 'idle; }
        //~ A queued task only blocks the clock while tasks are finishing. Tasks
        //~ that a whole drain left pending are themselves waiting on a timer or
        //~ on each other, and nothing but the clock can release them.
        let tasks_pending = SMELT_PROMISE_TASKS.with(|tasks| !tasks.borrow().is_empty());
        let tasks_settled = SMELT_PROMISE_TASK_SETTLED.with(|settled| settled.replace(false));
        if tasks_pending && tasks_settled { break 'idle; }
        let earliest = SMELT_TIMERS.with(|timers| timers.borrow().iter().filter(|timer| timer.id < id_barrier).map(|timer| timer.due_ms).min());
        let Some(earliest) = earliest else { break 'idle; };
        smelt_virtual_advance_to(earliest);
        smelt_drain_due_timers(id_barrier);
        smelt_drain_promise_tasks().await;
    }
    smelt_drain_promise_tasks().await;
    //~ The trailing cooperative yield keeps a spin-waiting caller from
    //~ monopolising the executor. Under a `Promise.race` driver it instead
    //~ costs a whole extra round to observe a settled racer: the spin loop
    //~ already yielded once this iteration, so this second suspension lands
    //~ BEFORE the re-check of the result cell, and the driver would take
    //~ another timer step in the meantime — settling a later racer and
    //~ handing it the win. The driver is the scheduler while it runs, so it
    //~ supplies the yield itself.
    if SMELT_RACE_DEPTH.with(::std::cell::Cell::get) == 0 { tokio::task::yield_now().await; }
}

//~ Emit the live-handle counter and the event loop's exit drain.
//~
//~ # Why the exit drain is not `sleep(0)`
//~
//~ It was, and that made every async program's last act "drain the microtask
//~ queue and the due timers, then return". Node does that too — and then keeps
//~ going while a REFERENCED HANDLE is open. A listening `http.Server` is such a
//~ handle, which is the whole reason `createServer(..).listen(3000)` serves in
//~ Node rather than exiting immediately. With the drain spelled as `sleep(0)`,
//~ the generated program returned from `main` the instant the queue was empty
//~ and the server task was dropped mid-flight.
//~
//~ A mid-program `await sleep(0)` must NOT wait on handles — it would never
//~ return while a server was up — so the two cannot be the same operation.
//~ Hence [`smelt_hir::AsyncOp::ExitDrain`], which is what the module body's
//~ last statement lowers to.
//~
//~ The counter lives here rather than in the `node:http` prelude because the
//~ drain that reads it is emitted for EVERY async program, while the server
//~ prelude is emitted only for programs that serve. Anything else that gains a
//~ process-keeping handle later (a listening socket, a watched file) increments
//~ the same counter.
thread_local! {
    //~ Open handles that keep the program alive, in Node's sense.
    static SMELT_LIVE_HANDLES: ::std::cell::Cell<usize> = const { ::std::cell::Cell::new(0) };
}
/// Register a handle that must keep the program from exiting.
#[allow(dead_code)]
fn smelt_retain_handle() { SMELT_LIVE_HANDLES.with(|handles| handles.set(handles.get().saturating_add(1))); }
/// Release a handle registered by `smelt_retain_handle`.
#[allow(dead_code)]
fn smelt_release_handle() { SMELT_LIVE_HANDLES.with(|handles| handles.set(handles.get().saturating_sub(1))); }
/// Run the event loop until the program is allowed to exit.
//~
//~ First the ordinary run-until-idle drain, then Node's ref'd-handle
//~ rule: stay alive while any handle is open. Polling (rather than a
//~ notification) is deliberate -- this loop runs once, at the very end of
//~ the program, and only while a handle really is open, so its cost is a
//~ wakeup every few milliseconds in a process that is otherwise just
//~ serving.
#[allow(dead_code)]
async fn smelt_run_until_exit() {
    smelt_sleep_ms(0.0).await;
    //~ While a handle keeps the program alive, the loop keeps RUNNING
    //~ the event loop, not just waiting: work a handle produces (a
    //~ `node:http` request whose handler starts a promise, a timer it
    //~ sets) is queued for this loop to drive. Sleeping alone left a
    //~ served request's handler task queued forever, so the server
    //~ accepted every connection and answered none. A queued task wakes
    //~ the loop at once (`SMELT_LOOP_WAKE`); the 5 ms tick remains for
    //~ due timers.
    while SMELT_LIVE_HANDLES.with(::std::cell::Cell::get) > 0 {
        smelt_sleep_ms(0.0).await;
        let smelt_wake = SMELT_LOOP_WAKE.with(::std::rc::Rc::clone);
        tokio::select! { _ = smelt_wake.notified() => {}, _ = tokio::time::sleep(::std::time::Duration::from_millis(5)) => {} }
    }
}

//~ `Promise.race` on the virtual clock. Every generated promise value
//~ is a spin-loop future that, when polled, advances virtual time by
//~ at most one timer step and then yields. So within a single poll
//~ round each racer can fire its own timer, and two racers whose
//~ timers are due at different virtual instants both become settled
//~ before anyone observes either. `tokio::select!` picks a branch in
//~ randomized order and returns the first one that reports `Ready`,
//~ which made the winner a coin flip: `withTimeout(() => delay(1000),
//~ 50)` resolved with the 1000 ms work about half the time instead of
//~ rejecting with the 50 ms timeout.
//~
//~ Polling the racers in source order fixes that without touching the
//~ clock: a racer settles only on the poll AFTER the step that fired
//~ its timer, so the racer whose timer was due earlier is always
//~ `Ready` in an earlier round, and ties inside one timer instant fall
//~ to the earlier-listed racer exactly as JS resolves same-tick ties by
//~ registration order. Losers are dropped when the vector goes out of
//~ scope, matching `select!`'s cancellation.
async fn smelt_promise_race<T>(mut racers: Vec<::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<T, Box<dyn std::error::Error>>>>>>) -> Result<T, Box<dyn std::error::Error>> {
    //~ The depth guard is a `Drop` type so the count is restored on every
    //~ exit path, including the early `return` that a settled racer takes.
    struct SmeltRaceGuard;
    impl Drop for SmeltRaceGuard { fn drop(&mut self) { SMELT_RACE_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1))); } }
    SMELT_RACE_DEPTH.with(|depth| depth.set(depth.get().saturating_add(1)));
    let _smelt_race_guard = SmeltRaceGuard;
    loop {
        let waker = smelt_noop_waker();
        let mut cx = ::std::task::Context::from_waker(&waker);
        for racer in racers.iter_mut() {
            if let ::std::task::Poll::Ready(result) = ::std::future::Future::poll(racer.as_mut(), &mut cx) { return result; }
        }
        smelt_drain_promise_tasks().await;
        //~ A race reached during an async function's EAGER PREFIX poll
        //~ (`SmeltFuture::from_future_primed`) is not idle when every racer is
        //~ pending: under priming a racer yields before it has run its own
        //~ synchronous prefix (`smelt_sleep_ms`), so it may not have scheduled
        //~ its timer yet. Advancing the clock then fired the OTHER racer's
        //~ timer first — `Promise.race([work(), timeout])` inside a function
        //~ called from a `.then` callback rejected with the timeout even when
        //~ the work was due long before it. The race yields instead, which
        //~ ends the single prime poll; it resumes as an ordinary await.
        if SMELT_PRIME_DEPTH.with(::std::cell::Cell::get) > 0 { tokio::task::yield_now().await; continue; }
        //~ Every racer is pending, so the event loop is idle: take exactly ONE
        //~ timer step — advance to the earliest pending due time and fire the
        //~ timers due at that instant — then re-poll everyone. Timers scheduled
        //~ by those callbacks are held back by the id barrier so they run on the
        //~ next round, the same deferral a zero-delay sleep applies.
        let id_barrier = SMELT_NEXT_TIMER_ID.with(::std::cell::Cell::get);
        let earliest = SMELT_TIMERS.with(|timers| timers.borrow().iter().filter(|timer| timer.id < id_barrier).map(|timer| timer.due_ms).min());
        if let Some(earliest) = earliest {
            smelt_virtual_advance_to(earliest);
            smelt_drain_due_timers(id_barrier);
            smelt_drain_promise_tasks().await;
        }
        tokio::task::yield_now().await;
    }
}

// @smelt:item-end
