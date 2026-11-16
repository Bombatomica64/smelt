// The timer queue and the event loop's exit drain.

// @smelt:item SmeltTimer when timers
struct SmeltTimer {
    //~ The heap entry that is this timer's current schedule: an interval
    //~ re-armed for its next period gets a new one, and an entry whose sequence
    //~ number is not this one is stale and skipped.
    seq: u64,
    callback: ::std::rc::Rc<::std::cell::RefCell<dyn FnMut() -> Result<(), Box<dyn std::error::Error>>>>,
    //~ `Some(period)` marks a repeating `setInterval` timer that re-arms
    //~ itself `period` ms after each fire; `None` is a one-shot `setTimeout`.
    period_ms: Option<u64>,
}

thread_local! {
    static SMELT_NEXT_TIMER_ID: ::std::cell::Cell<u64> = const { ::std::cell::Cell::new(1) };
    //~ Scheduling order: timers due at the same instant fire in the order they
    //~ were scheduled, as in Node.
    static SMELT_NEXT_TIMER_SEQ: ::std::cell::Cell<u64> = const { ::std::cell::Cell::new(1) };
    //~ The live timers by id: `clearTimeout` removes one in O(1), and its heap
    //~ entry is skipped when it comes up.
    static SMELT_TIMERS: ::std::cell::RefCell<::std::collections::HashMap<u64, SmeltTimer>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());
    //~ The schedule, earliest first: `(due, seq, id)`.
    static SMELT_TIMER_HEAP: ::std::cell::RefCell<::std::collections::BinaryHeap<::std::cmp::Reverse<(u64, u64, u64)>>> = const { ::std::cell::RefCell::new(::std::collections::BinaryHeap::new()) };
    //~ Wakes an event loop waiting on real I/O or an open handle when a job is
    //~ queued (`smelt_run_until`).
    static SMELT_LOOP_WAKE: ::std::rc::Rc<tokio::sync::Notify> = ::std::rc::Rc::new(tokio::sync::Notify::new());
    //~ Open handles that keep the program alive, in Node's sense.
    static SMELT_LIVE_HANDLES: ::std::cell::Cell<usize> = const { ::std::cell::Cell::new(0) };
}

//~ Start a test function from an empty event loop.
fn smelt_reset_timers() {
    SMELT_NEXT_TIMER_ID.with(|next| next.set(1));
    SMELT_NEXT_TIMER_SEQ.with(|next| next.set(1));
    SMELT_VIRTUAL_MS.with(|virtual_ms| virtual_ms.set(0));
    SMELT_TIMER_EPOCH.with(|epoch| epoch.set(None));
    let dropped = SMELT_TIMERS.with(|timers| ::std::mem::take(&mut *timers.borrow_mut()));
    drop(dropped);
    SMELT_TIMER_HEAP.with(|heap| heap.borrow_mut().clear());
    smelt_reset_jobs();
}

//~ Tell an event loop waiting on I/O that a job was queued.
fn smelt_loop_notify() { let _ = SMELT_LOOP_WAKE.try_with(|wake| wake.notify_one()); }

//~ How many open handles keep the program alive.
fn smelt_live_handles() -> usize { SMELT_LIVE_HANDLES.with(::std::cell::Cell::get) }

/// Register a handle that must keep the program from exiting.
#[allow(dead_code)]
fn smelt_retain_handle() { SMELT_LIVE_HANDLES.with(|handles| handles.set(handles.get().saturating_add(1))); }
/// Release a handle registered by `smelt_retain_handle`.
#[allow(dead_code)]
fn smelt_release_handle() { SMELT_LIVE_HANDLES.with(|handles| handles.set(handles.get().saturating_sub(1))); }

//~ Schedule timer `id` to fire at `due_ms`.
fn smelt_schedule_timer(id: u64, due_ms: u64, callback: ::std::rc::Rc<::std::cell::RefCell<dyn FnMut() -> Result<(), Box<dyn std::error::Error>>>>, period_ms: Option<u64>) {
    let seq = SMELT_NEXT_TIMER_SEQ.with(|next| { let seq = next.get(); next.set(seq.saturating_add(1)); seq });
    SMELT_TIMERS.with(|timers| timers.borrow_mut().insert(id, SmeltTimer { seq, callback, period_ms }));
    SMELT_TIMER_HEAP.with(|heap| heap.borrow_mut().push(::std::cmp::Reverse((due_ms, seq, id))));
}

fn smelt_next_timer_id() -> u64 { SMELT_NEXT_TIMER_ID.with(|next| { let id = next.get(); next.set(id.saturating_add(1)); id }) }

fn smelt_set_timeout(callback: ::std::rc::Rc<::std::cell::RefCell<dyn FnMut() -> Result<(), Box<dyn std::error::Error>>>>, delay_ms: f64) -> SmeltUnknown {
    let id = smelt_next_timer_id();
    let delay_ms = if delay_ms.is_finite() && delay_ms > 0.0 { delay_ms as u64 } else { 0 };
    smelt_schedule_timer(id, smelt_mono_ms().saturating_add(delay_ms), callback, None);
    SmeltUnknown::Number(id as f64)
}

fn smelt_set_interval(callback: ::std::rc::Rc<::std::cell::RefCell<dyn FnMut() -> Result<(), Box<dyn std::error::Error>>>>, period_ms: f64) -> SmeltUnknown {
    let id = smelt_next_timer_id();
    //~ Clamp non-positive periods to 1 ms so an interval still advances virtual
    //~ time and cannot fire again at the instant it fired.
    let period_ms = if period_ms.is_finite() && period_ms > 0.0 { period_ms as u64 } else { 1 };
    smelt_schedule_timer(id, smelt_mono_ms().saturating_add(period_ms), callback, Some(period_ms));
    SmeltUnknown::Number(id as f64)
}

fn smelt_clear_timeout<T: IntoSmeltUnknown>(handle: T) {
    let SmeltUnknown::Number(id) = handle.into_smelt_unknown() else { return; };
    let cleared = SMELT_TIMERS.with(|timers| timers.borrow_mut().remove(&(id as u64)));
    drop(cleared);
}

//~ `clearInterval` cancels by id exactly like `clearTimeout`; both share
//~ the single timer queue.
fn smelt_clear_interval<T: IntoSmeltUnknown>(handle: T) { smelt_clear_timeout(handle) }

//~ The earliest live schedule entry, dropping stale ones (cleared timers and
//~ an interval's superseded periods) from the top of the heap.
fn smelt_peek_timer() -> Option<(u64, u64, u64)> {
    loop {
        let top = SMELT_TIMER_HEAP.with(|heap| heap.borrow().peek().map(|entry| entry.0))?;
        let live = SMELT_TIMERS.with(|timers| timers.borrow().get(&top.2).is_some_and(|timer| timer.seq == top.1));
        if live { return Some(top); }
        SMELT_TIMER_HEAP.with(|heap| heap.borrow_mut().pop());
    }
}

//~ One timer phase of the event loop, run when the job queue is empty: move
//~ the virtual clock to the earliest due timer (or keep it, when that timer
//~ is already due) and fire, in due order, every timer due by then that was
//~ scheduled before the phase began, running the job queue after each
//~ callback as Node does. A timer a callback schedules waits for a later
//~ phase, even at the same instant. Answers whether any timer is pending.
fn smelt_fire_next_timers() -> bool {
    let Some((earliest, _, _)) = smelt_peek_timer() else { return false };
    smelt_virtual_advance_to(earliest);
    let now = smelt_mono_ms();
    let barrier = SMELT_NEXT_TIMER_SEQ.with(::std::cell::Cell::get);
    while let Some((due, seq, id)) = smelt_peek_timer() {
        if due > now || seq >= barrier { break; }
        SMELT_TIMER_HEAP.with(|heap| heap.borrow_mut().pop());
        let fired = SMELT_TIMERS.with(|timers| {
            let mut timers = timers.borrow_mut();
            let timer = timers.get(&id)?;
            let callback = timer.callback.clone();
            let period = timer.period_ms;
            if period.is_none() { timers.remove(&id); }
            Some((callback, period))
        });
        let Some((callback, period)) = fired else { continue };
        //~ Re-armed before the callback runs, so a `clearInterval` from inside
        //~ it finds the timer; the next fire is a period after this instant.
        if let Some(period_ms) = period { smelt_schedule_timer(id, now.saturating_add(period_ms), callback.clone(), Some(period_ms)); }
        (&mut *callback.borrow_mut())().unwrap_or_else(|error| smelt_panic_throw(error));
        smelt_run_jobs();
        smelt_check_rejections();
    }
    true
}

//~ A timer as a future: settles `delay_ms` from now on the virtual clock, in
//~ its turn among the other timers. Awaited inside a job it parks the job
//~ until the timer phase fires it; this is Node's model, where a sleep IS a
//~ timer, so a sleep never moves the clock past a timer due before it.
async fn smelt_sleep_ms(delay_ms: f64) {
    let fired = ::std::rc::Rc::new(::std::cell::Cell::new(false));
    let waker: ::std::rc::Rc<::std::cell::RefCell<Option<::std::task::Waker>>> = ::std::rc::Rc::new(::std::cell::RefCell::new(None));
    let delay_ms = if delay_ms.is_finite() && delay_ms > 0.0 { delay_ms as u64 } else { 0 };
    let (on_fire, wake_on_fire) = (fired.clone(), waker.clone());
    let callback: ::std::rc::Rc<::std::cell::RefCell<dyn FnMut() -> Result<(), Box<dyn std::error::Error>>>> = ::std::rc::Rc::new(::std::cell::RefCell::new(move || { on_fire.set(true); if let Some(waker) = wake_on_fire.borrow_mut().take() { waker.wake(); } Ok(()) }));
    smelt_schedule_timer(smelt_next_timer_id(), smelt_mono_ms().saturating_add(delay_ms), callback, None);
    if !smelt_in_job() {
        //~ Outside every job, the sleeper runs the event loop to its deadline.
        let _ = smelt_run_until(&|| fired.get()).await;
        return;
    }
    ::std::future::poll_fn(|cx| {
        if fired.get() { return ::std::task::Poll::Ready(()); }
        *waker.borrow_mut() = Some(cx.waker().clone());
        ::std::task::Poll::Pending
    }).await;
}

//~ Run the event loop until the program is allowed to exit, as Node does:
//~ the job queue, then every pending timer in due order (a never-cleared
//~ interval keeps the program running), then, while a handle is open (a
//~ listening `http.Server`), the I/O it produces. A mid-program `await`
//~ never waits on handles alone; only this exit drain does (see
//~ [`smelt_hir::AsyncOp::ExitDrain`], which the module body's last
//~ statement lowers to).
#[allow(dead_code)]
async fn smelt_run_until_exit() {
    let _ = smelt_run_until(&|| false).await;
}

// @smelt:item-end
