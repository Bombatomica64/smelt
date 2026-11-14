//! The exact Rust the exception-payload ABI (see `crate::thrown`) emits at a
//! throw site. What a throw delivers to its catch is checked by running programs:
//! the `thrown_payload_channel` Node-parity fixture and the
//! `try_around_infallible_call` Python-parity fixture.

use super::*;

/// One function's error channel carries two structurally unrelated payloads,
/// chosen at run time, each entering it at its own erased representation.
///
/// This is the proof the `SmeltUnknown` payload needs under `CLAUDE.md`:
///
/// * No concrete Rust type spans a two-field record and a string.
/// * No generated union arm can be selected, because the arm is picked by a
///   run-time branch and the `catch` is not in the throwing function's signature
///   (JavaScript has no throws-clause, so nothing propagates the choice).
/// * No function-scoped type parameter works either: the channel type is fixed by
///   `Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn Error>>>`,
///   shared by every erased callback in the crate, so it cannot mention a
///   parameter belonging to one caller.
#[test]
fn one_channel_carries_structurally_unrelated_payloads() {
    let source = source_for(
        r#"
function boom(asRecord: boolean): void {
  if (asRecord) {
    throw { code: 42, tag: "record" };
  }
  throw "a bare string";
}
"#,
    );

    assert_eq!(
        rust_fn(&source, "boom"),
        r#"fn boom(as_record: bool) -> Result<(), Box<dyn std::error::Error>> {
if as_record {
return Err::<_, Box<dyn std::error::Error>>(smelt_throw(SmeltUnknown::Object(SmeltObject::from_unknown_record((SmeltRecord::from([(JsString::from("code"), SmeltUnknown::Number(42.0 as f64)), (JsString::from("tag"), SmeltUnknown::String("record".into()))])).clone()))));
} else {
return Err::<_, Box<dyn std::error::Error>>(smelt_throw(SmeltUnknown::String("a bare string".into())));
}
}"#
    );
}

/// The fold of a throw payload looks only at the throwing function's own
/// locals. A capturing closure elsewhere reuses the same local numbers, so the
/// fixture occupies a spread of them.
#[test]
fn a_foreign_closure_capture_does_not_suppress_the_fold() {
    let source = source_for(
        r"
export function manyClosures(values: number[]): number[] {
  const a = 1;
  const b = 2;
  const c = 3;
  const d = 4;
  const e = 5;
  const f = 6;
  const g = 7;
  const h = 8;
  return values
    .map(value => value * a + b)
    .map(value => value * c + d)
    .map(value => value * e + f)
    .map(value => value * g + h);
}

export function thrower(size: number): number {
  if (!Number.isInteger(size) || size <= 0) {
    throw new Error('Size must be an integer greater than zero.');
  }
  return size;
}
",
    );

    assert_eq!(
        rust_fn(&source, "thrower"),
        r#"fn thrower(size: f64) -> Result<f64, Box<dyn std::error::Error>> {
let mut _smelt_tmp_3: bool;
let _smelt_tmp_4: bool;
let _smelt_tmp_1: bool = size.fract() == 0.0;
let _smelt_tmp_2: bool = !(_smelt_tmp_1);
let mut _smelt_tmp_3: bool = false;
if _smelt_tmp_2 {
_smelt_tmp_3 = true;
} else {
_smelt_tmp_4 = size <= 0.0;
_smelt_tmp_3 = _smelt_tmp_4;
}
if _smelt_tmp_3 {
return Err::<_, Box<dyn std::error::Error>>(smelt_throw(SmeltUnknown::Object(SmeltObject::from_unknown_record((SmeltRecord::from([(JsString::from("__smelt_error"), SmeltUnknown::String("Error".into())), (JsString::from("message"), SmeltUnknown::String("Size must be an integer greater than zero.".into())), (JsString::from("stack"), SmeltUnknown::Undefined), (JsString::from("cause"), SmeltUnknown::Undefined)])).clone()))));
} else {
return Ok(size);
}
}"#
    );
}

/// `throw new Error(m)` is one expression at the throw site: no `let` stages
/// the record or its erasure, because a hand-written crate would not.
#[test]
fn thrown_error_payload_is_built_at_the_throw_site() {
    let source = source_for(
        r"
export function chunk(size: number): number {
  if (size <= 0) {
    throw new Error('Size must be an integer greater than zero.');
  }
  return size;
}
",
    );

    assert_eq!(
        rust_fn(&source, "chunk"),
        r#"fn chunk(size: f64) -> Result<f64, Box<dyn std::error::Error>> {
let _smelt_tmp_1: bool = size <= 0.0;
if _smelt_tmp_1 {
return Err::<_, Box<dyn std::error::Error>>(smelt_throw(SmeltUnknown::Object(SmeltObject::from_unknown_record((SmeltRecord::from([(JsString::from("__smelt_error"), SmeltUnknown::String("Error".into())), (JsString::from("message"), SmeltUnknown::String("Size must be an integer greater than zero.".into())), (JsString::from("stack"), SmeltUnknown::Undefined), (JsString::from("cause"), SmeltUnknown::Undefined)])).clone()))));
} else {
return Ok(size);
}
}"#
    );
}

/// The fold is keyed on the shape of the MIR, not on the operand being an
/// `Error`: a thrown string and a thrown object each enter the channel whole.
#[test]
fn throwing_a_plain_value_keeps_its_own_payload() {
    let source = source_for(
        r"
export function keep(value: unknown): unknown {
  return value;
}

export function reject(flag: boolean): number {
  if (flag) {
    throw 'negative';
  }
  throw { code: 1 };
}
",
    );

    assert_eq!(
        rust_fn(&source, "reject"),
        r#"fn reject(flag: bool) -> Result<f64, Box<dyn std::error::Error>> {
if flag {
return Err::<_, Box<dyn std::error::Error>>(smelt_throw(SmeltUnknown::String("negative".into())));
} else {
return Err::<_, Box<dyn std::error::Error>>(smelt_throw({ let smelt_record = (SmeltRecord::from([(JsString::from("code"), 1.0)])).clone(); SmeltUnknown::Object(SmeltObject::with_id(smelt_record.id, smelt_record.iter().map(|(smelt_key, smelt_value)| (smelt_key, SmeltUnknown::Number(smelt_value as f64))).collect())) }));
}
}"#
    );
}
