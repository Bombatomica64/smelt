//! Rust emission for the two `AbortSignal` statics.
//!
//! Both build the erased signal record the abort subsystem owns — a marker, the
//! `aborted` flag, the `reason`, the listener list and the dependent-signal
//! list — through `smelt_abort_signal_record`, so a signal from a static and a
//! signal from `new AbortController().signal` are the same shape and the same
//! runtime helpers act on both.
//!
//! The instance members are not here: they are read off the record and bound by
//! `smelt_abort_method` (see `crate::lib`'s abort helpers), which is what lets
//! `signal?.addEventListener('abort', ..)` — the spelling every
//! `AbortSignal`-aware helper uses, because the signal arrives optional —
//! resolve them through the ordinary optional-chained member read.

use super::*;

impl FunctionEmitter<'_> {
    /// Emit one `AbortSignal` static call.
    pub(super) fn abort_signal_op_text(
        &self,
        op: smelt_hir::AbortSignalOp,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let unknown_ty = self.type_id(Type::Unknown)?;
        let call = match op {
            // `AbortSignal.abort(reason?)` is already aborted, so its record is
            // born with a reason. The argument goes through the same
            // absent-means-default rule `controller.abort(reason?)` uses, and
            // through the same helper, so the two spellings cannot disagree
            // about what `abort()` with no argument means.
            smelt_hir::AbortSignalOp::Abort => {
                let reason = match args.first() {
                    Some(reason) => format!(
                        "smelt_abort_reason_argument(&[{}])",
                        self.erase(reason)?
                    ),
                    None => "smelt_abort_default_reason()".to_owned(),
                };
                format!("SmeltUnknown::Object(smelt_abort_signal_record(Some({reason})))")
            }
            // `AbortSignal.timeout(ms)` answers a NOT-yet-aborted signal and
            // schedules the abort. The scheduled callback holds a clone of the
            // record, which is `Rc`-shared, so the abort it performs later is
            // observed through the signal the caller is holding — and it fires
            // with the spec's `TimeoutError`, which is the only thing
            // distinguishing a timed-out signal from `AbortSignal.abort()`.
            //
            // It is armed as a real TIMER (`smelt_set_timeout`), the same queue
            // `setTimeout` uses, and not as a promise task that sleeps. WHATWG
            // says "start a timeout", and on Smelt's virtual clock the two are
            // not interchangeable: the clock advances to the earliest TIMER
            // deadline, so a sleeping task's own deadline does not hold time
            // back. `AbortSignal.timeout(20)` racing a `setTimeout(.., 100)`
            // let the 100 ms timer win — the wait it was meant to cancel ran to
            // completion, and the signal only reported `aborted` afterwards.
            // The handle is discarded because the source has no way to name it:
            // `clearTimeout` cannot cancel a timeout signal.
            smelt_hir::AbortSignalOp::Timeout => {
                let Some(delay) = args.first() else {
                    return Err(EmitError::new(
                        "AbortSignal.timeout requires a delay in milliseconds",
                    ));
                };
                let float_ty = self.type_id(Type::Float)?;
                format!(
                    "{{ let smelt_signal = smelt_abort_signal_record(None); \
                     let smelt_handle = smelt_signal.clone(); \
                     let _ = {set_timeout}(::std::rc::Rc::new(::std::cell::RefCell::new(move || {{ \
                     smelt_abort_signal_fire(&smelt_handle, smelt_abort_timeout_reason()); \
                     Ok(()) }})), {delay} as f64); \
                     SmeltUnknown::Object(smelt_signal) }}",
                    set_timeout = smelt_stdlib::runtime_symbols::timers::SET_TIMEOUT,
                    delay = self.value_at_type(delay, float_ty)?,
                )
            }
        };
        self.value_at_type_text(&call, unknown_ty, dest_ty)
    }
}
