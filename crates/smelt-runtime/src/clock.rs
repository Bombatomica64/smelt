//! The clock shared by JavaScript timers and `Date.now()`.
//!
//! JS code routinely measures elapsed time with `Date.now()` while scheduling
//! work with `setTimeout`, and expects the two to agree (a debounce reads
//! `Date.now()` to size its `maxWait` timeout, then waits for that timeout to
//! fire). Generated Rust runs deterministically on a virtual clock —
//! `sleep`/timer draining fast-forwards time instead of really blocking — so
//! both readings must come from one timeline.
//!
//! `SMELT_VIRTUAL_MS` is the accumulated fast-forward; real wall time keeps
//! advancing on top of it so a synchronous busy-loop such as
//! `while (Date.now() - start < 320) { ... }` (no `await` to fast-forward) still
//! terminates on real elapsed time rather than spinning forever.
//!
//! `SMELT_DATE_NOW` and `SMELT_DATE_TIMEZONE_OFFSET` are the two test-visible
//! overrides (`vi.setSystemTime`, timezone pinning); they are separate items
//! because their gates are separate — a program that only reads `Date.now()`
//! must not emit the timer queue.

// @smelt:item SMELT_DATE_NOW
thread_local! {
    static SMELT_DATE_NOW: ::std::cell::Cell<Option<i64>> = const { ::std::cell::Cell::new(None) };
}
// @smelt:item-end

// @smelt:item SMELT_DATE_TIMEZONE_OFFSET
thread_local! {
    static SMELT_DATE_TIMEZONE_OFFSET: ::std::cell::Cell<f64> = const { ::std::cell::Cell::new(0.0) };
}
// @smelt:item-end

// @smelt:item SMELT_VIRTUAL_MS
thread_local! {
    static SMELT_VIRTUAL_MS: ::std::cell::Cell<u64> = const { ::std::cell::Cell::new(0) };
    static SMELT_TIMER_EPOCH: ::std::cell::Cell<Option<::std::time::Instant>> = const { ::std::cell::Cell::new(None) };
}
// @smelt:item-end

// @smelt:item smelt_mono_ms
/// Monotonic virtual + wall clock (ms) shared by JS timers and `Date.now()`.
///
/// Returns real elapsed wall time since a fixed epoch plus the virtual
/// fast-forward accumulated by `sleep`/timer draining, so `setTimeout`
/// deadlines and `Date.now()` measurements share one timeline.
fn smelt_mono_ms() -> u64 {
    let epoch = SMELT_TIMER_EPOCH.with(|epoch| match epoch.get() {
        Some(instant) => instant,
        None => { let instant = ::std::time::Instant::now(); epoch.set(Some(instant)); instant }
    });
    let real_ms = ::std::time::Instant::now().saturating_duration_since(epoch).as_millis() as u64;
    real_ms.saturating_add(SMELT_VIRTUAL_MS.with(::std::cell::Cell::get))
}
// @smelt:item-end

// @smelt:item smelt_virtual_advance_to
/// Fast-forward the virtual clock so `smelt_mono_ms()` reaches `target_ms`.
fn smelt_virtual_advance_to(target_ms: u64) {
    let now = smelt_mono_ms();
    if target_ms > now {
        SMELT_VIRTUAL_MS.with(|virtual_ms| virtual_ms.set(virtual_ms.get().saturating_add(target_ms - now)));
    }
}
// @smelt:item-end

#[cfg(test)]
mod tests {
    use super::{SMELT_DATE_NOW, SMELT_DATE_TIMEZONE_OFFSET, smelt_mono_ms, smelt_virtual_advance_to};

    /// The clock never runs backwards, whatever the mix of real and virtual time.
    #[test]
    fn the_clock_is_monotonic() {
        let first = smelt_mono_ms();
        let second = smelt_mono_ms();
        assert!(
            second >= first,
            "clock went backwards: {first} then {second}"
        );
    }

    /// Fast-forwarding to a future deadline makes the clock read at least it.
    ///
    /// This is what lets a drained `setTimeout(fn, 5000)` observe
    /// `Date.now() - start >= 5000` without the test really waiting five
    /// seconds.
    #[test]
    fn advancing_reaches_the_target() {
        let target = smelt_mono_ms().saturating_add(5_000);
        smelt_virtual_advance_to(target);
        assert!(
            smelt_mono_ms() >= target,
            "virtual advance must reach the requested deadline"
        );
    }

    /// Advancing to an already-passed deadline is a no-op, not a rewind.
    #[test]
    fn advancing_to_the_past_does_not_rewind() {
        let before = smelt_mono_ms();
        smelt_virtual_advance_to(0);
        assert!(
            smelt_mono_ms() >= before,
            "a stale deadline must not move the clock back"
        );
    }

    /// The `Date` override slots start unset, so `Date.now()` reads the clock.
    #[test]
    fn date_overrides_default_to_unset() {
        assert_eq!(
            SMELT_DATE_NOW.with(::std::cell::Cell::get),
            None,
            "no pinned system time until a test sets one"
        );
        assert!(
            SMELT_DATE_TIMEZONE_OFFSET
                .with(::std::cell::Cell::get)
                .abs()
                < f64::EPSILON,
            "the default timezone offset is UTC"
        );
    }
}
