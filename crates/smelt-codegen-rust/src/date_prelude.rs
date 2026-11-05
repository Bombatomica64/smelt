//! Generated runtime for the JavaScript `Date` class.
//!
//! # `SmeltDate`
//!
//! A `Date` is a mutable reference object: `d.setDate(1)` changes the date
//! every alias of `d` sees, `a === b` asks whether two dates are the same
//! object, and `new Date(d)` makes a fresh one. `SmeltDate` is therefore a
//! handle on one shared cell holding the time value — milliseconds since the
//! epoch as an `f64`, `NaN` for an Invalid Date, exactly the value the
//! specification stores — and cloning the handle aliases, never copies.
//!
//! The date arithmetic itself (getters, setters, constructors, formatting)
//! stays in the emitter's timestamp templates (`emitter/strings_io.rs`): they
//! read the handle's time value, and a setter writes its result back into the
//! same cell (`SmeltDate::set_time`).
//!
//! # Dynamic boundary
//!
//! An erased `Date` is the marker record `{ "__smelt_date": <time value> }`,
//! the shape the erased date paths (and a time-zone context's
//! `__smelt_timezone` metadata) already read. Erasing keeps the live handle in
//! the record's origin slot, so narrowing the record back hands out the SAME
//! date. A record that did not come from an erasure — a hand-built marker, a
//! number, a string — is rebuilt as a fresh date from the value it carries.
//! These adapters are the only place a `SmeltDate` meets `SmeltUnknown`.

#![expect(
    clippy::redundant_pub_crate,
    reason = "the prelude emitter is called from the crate root"
)]

use crate::rust::CodeWriter;

/// The marker key an erased `Date` record carries its time value under.
pub(crate) const DATE_MARKER: &str = "__smelt_date";

/// Emit the `SmeltDate` runtime type, plus its dynamic-boundary adapters when
/// the crate carries the erased value.
pub(crate) fn emit(writer: &mut CodeWriter, needs_unknown: bool) {
    writer.line("/// A JavaScript `Date`: a shared, mutable time value (ms since the epoch, `NaN`");
    writer.line("/// for an Invalid Date). Cloning aliases the same date.");
    writer.line("#[derive(Clone)]");
    writer.line("pub struct SmeltDate { id: usize, time: ::std::rc::Rc<::std::cell::Cell<f64>> }");
    writer.blank_line();
    // Structural equality over the time value is what `expect(a).toEqual(b)`
    // observes on two dates; `===` compares `id()` (see `binary_ops`).
    writer.line("impl PartialEq for SmeltDate { fn eq(&self, other: &Self) -> bool { let (left, right) = (self.get_time(), other.get_time()); left == right || (left.is_nan() && right.is_nan()) } }");
    writer.line("impl PartialOrd for SmeltDate { fn partial_cmp(&self, other: &Self) -> Option<::std::cmp::Ordering> { self.get_time().partial_cmp(&other.get_time()) } }");
    writer.line("impl ::std::fmt::Debug for SmeltDate { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str(&self.to_iso_string()) } }");
    // `Default` exists for the generated-type bounds (`T: Default`) and the
    // defaulted slots of generated structs; JavaScript has no default date,
    // so it is the Invalid Date rather than an arbitrary instant.
    writer.line("impl Default for SmeltDate { fn default() -> Self { Self::new(f64::NAN) } }");
    writer.blank_line();
    writer.line("#[allow(dead_code)]");
    writer.block("impl SmeltDate", |impl_writer| {
        impl_writer.line("/// A fresh date holding `time_ms` (any non-finite value is an Invalid Date).");
        impl_writer.line("pub fn new(time_ms: f64) -> Self { Self { id: smelt_next_object_id(), time: ::std::rc::Rc::new(::std::cell::Cell::new(Self::clip(time_ms))) } }");
        impl_writer.line("/// `TimeClip`: a non-finite or out-of-range time value is `NaN`.");
        impl_writer.line("fn clip(time_ms: f64) -> f64 { if time_ms.is_finite() && time_ms.abs() <= 8.64e15 && time_ms != i64::MIN as f64 { time_ms.trunc() + 0.0 } else { f64::NAN } }");
        impl_writer.line("/// `getTime()` / `valueOf()`: the time value.");
        impl_writer.line("pub fn get_time(&self) -> f64 { self.time.get() }");
        impl_writer.line("/// Store a new time value in place, so every alias observes it; answers");
        impl_writer.line("/// the same date, as the setters' callers re-bind their receiver to it.");
        impl_writer.line("pub fn set_time(&self, time_ms: f64) -> Self { self.time.set(Self::clip(time_ms)); self.clone() }");
        impl_writer.line("/// JavaScript reference identity of this date, shared by every alias and");
        impl_writer.line("/// by every erased view of it.");
        impl_writer.line("pub fn id(&self) -> usize { self.id }");
        impl_writer.line("/// `toISOString()`, `Invalid Date` for an invalid time value.");
        impl_writer.line("pub fn to_iso_string(&self) -> String { let time_ms = self.get_time(); if time_ms.is_finite() { chrono::DateTime::<chrono::Utc>::from_timestamp_millis(time_ms as i64).map(|date| date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)).unwrap_or_else(|| \"Invalid Date\".to_owned()) } else { \"Invalid Date\".to_owned() } }");
    });
    writer.blank_line();
    emit_date_like(writer);
    if needs_unknown {
        emit_boundary_adapters(writer);
    }
}

/// Emit `DateLike`, the trait a `D extends Date` parameter lifts to
/// (`fn f<D: DateLike + ..>(d: D)`, bounded-generics plan increment 4).
///
/// The frontend dispatches every member of a `Date`-bounded receiver as the
/// `Date` operation it is (`d.getTime()`, `d.setDate(..)`), so the generic
/// body only needs the date behind a `D`: `smelt_date()` hands out the same
/// shared handle, and the emitter's date templates run on it. Only
/// `SmeltDate` implements it; a `Date` subclass keeps the erased parameter.
fn emit_date_like(writer: &mut CodeWriter) {
    writer.line("/// What generic code reads through a `Date` bound: the date itself.");
    writer.line("#[allow(dead_code)]");
    writer.block("pub trait DateLike", |trait_writer| {
        trait_writer.line("/// The shared date handle behind this value (an alias, not a copy).");
        trait_writer.line("fn smelt_date(&self) -> SmeltDate;");
        trait_writer.line("/// `getTime()`.");
        trait_writer.line("fn get_time(&self) -> f64 { self.smelt_date().get_time() }");
    });
    writer.line("impl DateLike for SmeltDate { fn smelt_date(&self) -> SmeltDate { self.clone() } }");
    writer.blank_line();
}

/// Emit the `Date` dynamic-boundary adapters (see the module docs).
fn emit_boundary_adapters(writer: &mut CodeWriter) {
    writer.line("/// Erase a date for a dynamic boundary: its marker record, with the live");
    writer.line("/// handle kept in the origin slot.");
    writer.block("impl IntoSmeltUnknown for SmeltDate", |impl_writer| {
        impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
            fn_writer.line("let smelt_host_origin = self.clone();");
            fn_writer.line(format!(
                "smelt_with_host_origin(SmeltUnknown::Object(SmeltObject::with_id(self.id, Vec::from([(\"{DATE_MARKER}\".to_owned(), SmeltUnknown::Number(self.get_time()))]))), smelt_host_origin)"
            ));
        });
    });
    writer.blank_line();
    writer.line("/// Recover a date from an erased value: the retained handle, else a fresh");
    writer.line("/// date from the time value a marker record, a number or a string carries.");
    writer.block("impl SmeltFromUnknown for SmeltDate", |impl_writer| {
        impl_writer.block("fn smelt_from_unknown(value: SmeltUnknown) -> Self", |fn_writer| {
            fn_writer.line(
                "if let Some(origin) = smelt_restore_host_origin::<Self>(&value) { return origin; }",
            );
            fn_writer.line(format!(
                "let time_ms = match &value {{ SmeltUnknown::Number(time_ms) => *time_ms, SmeltUnknown::Object(map) => match map.get(\"{DATE_MARKER}\") {{ Some(SmeltUnknown::Number(time_ms)) => time_ms, _ => f64::NAN }}, SmeltUnknown::String(text) => chrono::DateTime::parse_from_rfc3339(text).map(|date| date.timestamp_millis() as f64).unwrap_or(f64::NAN), _ => f64::NAN }};"
            ));
            fn_writer.line("Self::new(time_ms)");
        });
    });
    writer.blank_line();
}
