//! Emitted Rust for four rules about *absence* and *reference identity*.
//!
//! * A list-to-list coercion whose element types render to one Rust type is
//!   not a coercion, so a by-reference argument keeps aliasing the caller's
//!   array.
//! * Reading an erased value into a `String` slot is not the JS `String(x)`
//!   conversion: absence takes the slot's default.
//! * A container of `Type::None` recovers `undefined` vs `null` from its
//!   defining literal.
//! * A method reference read off an instance denotes the one prototype method,
//!   so every read links to one canonical identity.
//!
//! The observable values are checked against Node by the
//! `truthiness_absence_identity` fixture.

use super::*;

/// The caller's `unknown[]` and the callee's erased `T[]` are different MIR
/// types and one Rust type, so the callee receives the caller's own list.
#[test]
fn a_by_reference_list_argument_with_identical_element_rendering_is_not_rebuilt() {
    let source = source_for(
        r"
function dropFirst<T>(target: T[]): void {
  target.splice(0, 1);
}
export function useDropFirst(): number {
  const values: unknown[] = [1, 2, 3];
  dropFirst(values);
  return values.length;
}
",
    );
    assert_eq!(rust_fn(&source, "use_drop_first"), r"fn use_drop_first() -> f64 {
let _smelt_tmp_3: f64;
let mut values: SmeltList<SmeltUnknown> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<SmeltUnknown> = vec![SmeltUnknown::Number(1.0 as f64), SmeltUnknown::Number(2.0 as f64), SmeltUnknown::Number(3.0 as f64)]; smelt_list_items }));
let _ = { let smelt_mut_call_result = drop_first(&mut values); smelt_mut_call_result };
_smelt_tmp_3 = values.len() as f64;
return _smelt_tmp_3;
}");
}

/// `value as string` is a type assertion, so absence reaching a `String` slot
/// takes the type's default, as the typed side does for the same situation.
#[test]
fn extracting_absence_into_a_string_slot_is_not_the_string_conversion() {
    let source = source_for(
        r"
export function readString(value: unknown): string {
  return value as string;
}
",
    );
    assert_eq!(rust_fn(&source, "read_string"), r#"fn read_string(value: SmeltUnknown) -> JsString {
let _smelt_tmp_1: JsString = match value.clone() { SmeltUnknown::String(smelt_value) => smelt_value, smelt_other => JsString::from(match smelt_other { SmeltUnknown::String(smelt_value) => smelt_value.to_string(), SmeltUnknown::Symbol(smelt_value) => smelt_symbol_to_string(&smelt_value), SmeltUnknown::Number(smelt_value) => smelt_number_to_string(smelt_value), SmeltUnknown::Bool(smelt_value) => smelt_value.to_string(), SmeltUnknown::Null | SmeltUnknown::Undefined => String::new(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => "[object Object]".to_owned(), SmeltUnknown::Function(_) => "function () { [native code] }".to_owned(), SmeltUnknown::Promise(_) => "[object Promise]".to_owned() }) };
return _smelt_tmp_1;
}"#);
}

/// `null` and `undefined` share MIR `Type::None`, so only the defining
/// `Rvalue::Dict` knows which singleton the entries hold.
#[test]
fn an_object_literal_holding_undefined_erases_as_undefined() {
    let source = source_for(
        r"
export function erase(): unknown {
  const holder = { a: undefined };
  return holder;
}
",
    );
    assert_eq!(rust_fn(&source, "erase"), r#"fn erase() -> SmeltUnknown {
let holder: SmeltRecord<JsString, SmeltUnknown> = SmeltRecord::from([(JsString::from("a"), SmeltUnknown::Undefined)]);
return SmeltUnknown::Object(SmeltObject::from_unknown_record(holder.clone()));
}"#);
}

/// The other direction of the same recovery: a `null` literal keeps its tag.
#[test]
fn an_object_literal_holding_null_still_erases_as_null() {
    let source = source_for(
        r"
export function erase(): unknown {
  const holder = { a: null };
  return holder;
}
",
    );
    assert_eq!(rust_fn(&source, "erase"), r#"fn erase() -> SmeltUnknown {
let holder: SmeltRecord<JsString, ()> = SmeltRecord::from([(JsString::from("a"), ())]);
return { let smelt_record = holder.clone(); SmeltUnknown::Object(SmeltObject::with_id(smelt_record.id, smelt_record.iter().map(|(smelt_key, smelt_value)| (smelt_key, { let _ = smelt_value; SmeltUnknown::Null })).collect())) };
}"#);
}

/// A method reference captures its receiver, so each read is a fresh `Rc`;
/// every read links to one canonical id per (defining class, method).
#[test]
fn a_class_method_reference_links_one_canonical_identity() {
    let source = source_for(
        r"
class Greeter {
  greet(): string {
    return 'hi';
  }
}
export function methodOf(greeter: Greeter): unknown {
  return greeter.greet;
}
",
    );
    assert_eq!(rust_fn(&source, "method_of"), r#"fn method_of(greeter: Greeter) -> SmeltUnknown {
return ({ let smelt_receiver = greeter.clone(); let smelt_method: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>(SmeltUnknown::String((smelt_receiver.greet()).into()))); smelt_link_function_identity_key(&smelt_method, smelt_method_identity("Greeter::greet")); SmeltUnknown::Function(smelt_method) }).clone();
}"#);
}

/// A keyed read that resolves to no property is `undefined`.
#[test]
fn an_absent_key_on_a_class_receiver_reads_as_undefined() {
    let source = source_for(
        r"
class Holder {
  value = 1;
}
export function readMissing(holder: Holder): unknown {
  return (holder as any)['nope'];
}
",
    );
    assert_eq!(rust_fn(&source, "read_missing"), r#"fn read_missing(holder: Holder) -> SmeltUnknown {
let _smelt_tmp_1: SmeltUnknown = { let smelt_object_value = holder.clone(); let smelt_struct_value = smelt_object_value.clone(); let mut smelt_object_entries = Vec::<(String, _)>::new(); smelt_object_entries.push(("value".to_owned(), SmeltUnknown::Number(smelt_object_value.value as f64))); smelt_object_entries.push(("__smelt_class".to_owned(), SmeltUnknown::String("Holder".into()))); SmeltUnknown::Object(SmeltObject::new(smelt_object_entries)) };
return match _smelt_tmp_1.clone() {
SmeltUnknown::String(smelt_value) => {
let smelt_key = JsString::from("nope").clone();
if smelt_key == "length" {
SmeltUnknown::Number(smelt_value.len() as f64)
} else {
smelt_key.to_string().parse::<usize>().ok().and_then(|smelt_position| smelt_value.char_at(smelt_position).map(|smelt_ch| SmeltUnknown::String(smelt_ch.into()))).unwrap_or(SmeltUnknown::Undefined)
}
}
SmeltUnknown::Array(smelt_values) => smelt_get_array_field(&smelt_values, &JsString::from("nope").clone()),
SmeltUnknown::Object(smelt_values) => smelt_get_object_field(&smelt_values, &JsString::from("nope").clone()),
_ => SmeltUnknown::Undefined,
};
}"#);
}
