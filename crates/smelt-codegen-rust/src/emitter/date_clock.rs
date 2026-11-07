//! The emitted clock: `Date.now()` and the timezone offset, their mock overrides
//! and the timezone context.

use super::*;
use crate::rust::RustExpr;

impl FunctionEmitter<'_> {
    /// `Date.now()`.
    pub(super) fn date_now_expr(&self, dest_ty: TypeId) -> Result<RustExpr, EmitError> {
        // `Date.now()` shares the timer timeline: real wall time plus the
        // virtual fast-forward accumulated by `sleep`/timer draining
        // (`SMELT_VIRTUAL_MS`, emitted alongside the date runtime). This
        // keeps elapsed-time measurements consistent with `setTimeout`
        // deadlines under the deterministic virtual clock. An explicit
        // `vi.setSystemTime(...)` override (`SMELT_DATE_NOW`) still wins.
        let text = RustExpr::chain("SMELT_DATE_NOW.with(::std::cell::Cell::get).unwrap_or_else(|| chrono::Utc::now().timestamp_millis().saturating_add(SMELT_VIRTUAL_MS.with(::std::cell::Cell::get) as i64))");
        self.date_timestamp_result_expr(&text, dest_ty)
    }

    /// Pin `Date.now()` to a timestamp.
    pub(super) fn date_set_now_expr(
        &self,
        timestamp: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        Ok(RustExpr::atom(format!(
            "{{ SMELT_DATE_NOW.with(|value| value.set(Some(({}) as i64))); {} }}",
            self.date_timestamp_expr(timestamp)?,
            self.default_value(dest_ty)?
        )))
    }

    /// Remove a pinned `Date.now()`.
    pub(super) fn date_reset_now_expr(&self, dest_ty: TypeId) -> Result<RustExpr, EmitError> {
        Ok(RustExpr::atom(format!(
            "{{ SMELT_DATE_NOW.with(|value| value.set(None)); {} }}",
            self.default_value(dest_ty)?
        )))
    }

    /// Pin the timezone offset.
    pub(super) fn date_set_timezone_offset_expr(
        &self,
        offset: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        Ok(RustExpr::atom(format!(
            "{{ SMELT_DATE_TIMEZONE_OFFSET.with(|value| value.set({})); {} }}",
            self.value_at_type(offset, self.type_id(Type::Float)?)?,
            self.default_value(dest_ty)?
        )))
    }

    /// Remove a pinned timezone offset.
    pub(super) fn date_reset_timezone_offset_expr(
        &self,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        Ok(RustExpr::atom(format!(
            "{{ SMELT_DATE_TIMEZONE_OFFSET.with(|value| value.set(0.0)); {} }}",
            self.default_value(dest_ty)?
        )))
    }
}
