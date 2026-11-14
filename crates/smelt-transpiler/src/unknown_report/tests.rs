//! Unit tests for the `SmeltUnknown` report: classification, scanning, baseline
//! gates and rendering.

use super::classify::*;
use super::scan::*;
use super::*;

/// A callable interface's erased `__smelt_call` slot is a boundary.
///
/// An overloaded callable interface stores every overload in one slot, so
/// the slot's type cannot be any single overload's concrete `Fn` shape —
/// which overload runs is a runtime decision made from the argument values.
/// Ordinary erased program storage with no such slot stays avoidable.
#[test]
fn erased_callable_object_slot_is_a_boundary() {
    let slot = "    _smelt_tmp_3 = { let smelt_callback = curried.__smelt_call.clone(); let smelt_adapted: ::std::rc::Rc<dyn Fn(&SmeltUnknown, &SmeltUnknown) -> SmeltUnknown> = ::std::rc::Rc::new(move |arg0: &SmeltUnknown, arg1: &SmeltUnknown| smelt_callback.call(vec![arg0.clone(), arg1.clone()])); smelt_adapted };";
    assert_eq!(
        classify_line(slot, false),
        Category::LegitimateBoundary,
        "the erased callable-object call ABI is a runtime-dispatch boundary"
    );
    let storage = "    let values: SmeltList<SmeltUnknown> = SmeltList::new(Vec::new());";
    assert_eq!(
        classify_line(storage, false),
        Category::AvoidableErasure,
        "ordinary erased storage must stay avoidable"
    );
}

/// A property read through the erased chain is a boundary.
///
/// The receiver is `unknown`, so the member is resolved by runtime
/// narrowing on the value's variant; an ordinary erased local next to it
/// stays avoidable, so the marker does not widen past the read.
#[test]
fn erased_property_read_is_a_boundary() {
    let read = "    let groups: SmeltUnknown = smelt_get_unknown_field(::std::borrow::Borrow::borrow(&smelt_match), \"groups\");";
    assert_eq!(
        classify_line(read, false),
        Category::LegitimateBoundary,
        "a member looked up by name on an erased receiver is runtime narrowing"
    );
    let storage = "    let mut matches: Vec<SmeltUnknown> = Vec::new();";
    assert_eq!(
        classify_line(storage, false),
        Category::AvoidableErasure,
        "ordinary erased storage must stay avoidable"
    );
}

/// An error record's `cause` slot is a boundary.
///
/// `cause?: unknown` is the source-level `unknown` spelling itself, so the
/// slot and the value written into it are a genuine dynamic edge. The
/// marker is field-shaped, so ordinary erased storage and an erased
/// parameter beside it stay avoidable — it does not widen to every line
/// that happens to mention a cause.
#[test]
fn error_cause_slot_is_a_boundary() {
    let slot = "    cause: SmeltUnknown,";
    assert_eq!(
        classify_line(slot, false),
        Category::LegitimateBoundary,
        "the ES2022 `cause?: unknown` slot is the canonical source-level unknown"
    );
    let written = "    let mut this: Self = HTTPException { name: String::new(), message: String::new(), stack: None::<String>, cause: SmeltUnknown::Null, status: 0.0 };";
    assert_eq!(
        classify_line(written, false),
        Category::LegitimateBoundary,
        "the value written into the cause slot is the same boundary as the slot"
    );
    let storage = "    let causes: Vec<SmeltUnknown> = Vec::new();";
    assert_eq!(
        classify_line(storage, false),
        Category::AvoidableErasure,
        "ordinary erased storage must stay avoidable"
    );
    let parameter = "fn describe(cause_of: SmeltUnknown) -> String {";
    assert_eq!(
        classify_line(parameter, false),
        Category::AvoidableErasure,
        "an erased parameter whose name merely contains `cause` must stay avoidable"
    );
}

/// The erased-view write-through setter converts an erased write back to
/// a field's declared type; its erased parameter is that boundary.
#[test]
fn view_write_through_setter_is_a_boundary() {
    assert_eq!(
        classify_line(
            "    fn __smelt_set_field(&self, key: &str, value: SmeltUnknown) {",
            false
        ),
        Category::LegitimateBoundary,
        "the setter adapts an erased property write to the field's type"
    );
    assert_eq!(
        classify_line("    fn set_label(&self, value: SmeltUnknown) {", false),
        Category::AvoidableErasure,
        "an ordinary erased parameter must stay avoidable"
    );
}

/// The erased-view live getter is the read mirror of the setter: a key
/// chosen at runtime selects one of several differently typed fields.
#[test]
fn live_callable_field_getter_is_a_boundary() {
    assert_eq!(
        classify_line(
            "    fn __smelt_get_callable_field(&self, key: &str) -> SmeltUnknown { match key { _ => SmeltUnknown::Undefined } }",
            false
        ),
        Category::LegitimateBoundary,
        "the getter erases one live field on demand for an erased read"
    );
    assert_eq!(
        classify_line(
            "    fn get_label(&self, key: &str) -> SmeltUnknown {",
            false
        ),
        Category::AvoidableErasure,
        "an ordinary erased return must stay avoidable"
    );
}

/// `instanceof` a SOURCE class over an erased operand probes the hidden
/// `__smelt_class` marker — runtime narrowing of a value whose static type
/// says nothing (a `catch` binding), so it is a boundary; an erased local
/// holding the same value stays avoidable.
#[test]
fn user_class_instanceof_guard_is_a_boundary() {
    let guard = "    _smelt_tmp_7 = matches!(e.clone().clone(), SmeltUnknown::Object(smelt_value) if matches!(smelt_value.get(\"__smelt_class\"), Some(SmeltUnknown::String(smelt_class_name)) if matches!(&*smelt_class_name, \"UnsupportedPathError\")));";
    assert_eq!(
        classify_line(guard, false),
        Category::LegitimateBoundary,
        "a caught source-class error recovers its class from the value's marker"
    );
    let optional = "    let _smelt_tmp_2: bool = matches!(input.clone(), Some(SmeltUnknown::Object(smelt_value)) if matches!(smelt_value.get(\"__smelt_class\"), Some(SmeltUnknown::String(smelt_class_name)) if matches!(&*smelt_class_name, \"Base\" | \"Derived\")));";
    assert_eq!(
        classify_line(optional, false),
        Category::LegitimateBoundary,
        "the `Option` receiver spelling of the same guard is the same boundary"
    );
    assert_eq!(
        classify_line("    let caught: SmeltUnknown = e;", false),
        Category::AvoidableErasure,
        "an ordinary erased local must stay avoidable"
    );
}

/// `instanceof HostClass` over an erased operand is a boundary.
///
/// The guard reads a `__smelt_*` brand key back off a value whose static
/// type says only "some erased value" — the canonical case being a `catch`
/// binding, which TypeScript types `unknown` by its own rule. That is
/// runtime narrowing, the same edge `tag_check` and `smelt_unknown_is_`
/// name for the primitive tags; the host brands were the one family whose
/// guard was still inlined at the call site.
///
/// The marker is the guard's own shape, so it covers both spellings the
/// emitter produces (plain and `Option` receiver) while an erased local, an
/// erased parameter, and a `contains_key` that is not a `value`-bound brand
/// guard all stay avoidable. A brand probe that is NOT a narrowing guard is
/// not what classifies its line: where the line also carries a boundary
/// adapter (`into_smelt_unknown`), the adapter does, so it is not avoidable.
#[test]
fn host_brand_instanceof_guard_is_a_boundary() {
    let guard = "    _smelt_tmp_3 = matches!(error.clone(), SmeltUnknown::Object(smelt_value) if smelt_value.contains_key(\"__smelt_domexception\"));";
    assert_eq!(
        classify_line(guard, false),
        Category::LegitimateBoundary,
        "a caught host error recovers its class from the value's brand"
    );
    let optional = "    let _smelt_tmp_2: bool = matches!(input.clone(), Some(SmeltUnknown::Object(smelt_value)) if smelt_value.contains_key(\"__smelt_request\"));";
    assert_eq!(
        classify_line(optional, false),
        Category::LegitimateBoundary,
        "the `Option` receiver spelling of the same guard is the same boundary"
    );
    let multi = "    _smelt_tmp_4 = matches!(buffer.clone(), SmeltUnknown::Object(smelt_value) if smelt_value.contains_key(\"__smelt_arraybuffer\") || value.contains_key(\"__smelt_sharedarraybuffer\"));";
    assert_eq!(
        classify_line(multi, false),
        Category::LegitimateBoundary,
        "a class with several brands probes each of them in one guard"
    );
    let storage = "    let failure: SmeltUnknown;";
    assert_eq!(
        classify_line(storage, false),
        Category::AvoidableErasure,
        "an ordinary erased local must stay avoidable"
    );
    let parameter = "pub(crate) fn width_of(value: SmeltUnknown) -> f64 {";
    assert_eq!(
        classify_line(parameter, false),
        Category::AvoidableErasure,
        "an erased parameter named `value` must stay avoidable"
    );
    let field =
        "    let tag: SmeltUnknown = fields.contains_key(\"__smelt_class\").into_smelt_unknown();";
    assert_ne!(
        classify_line(field, false),
        Category::AvoidableErasure,
        "the adapter on this line is what classifies it, not the brand probe"
    );
    let unbranded = "    let seen: Vec<SmeltUnknown> = Vec::from([SmeltUnknown::Bool(entries.contains_key(\"id\"))]);";
    assert_eq!(
        classify_line(unbranded, false),
        Category::AvoidableErasure,
        "a `contains_key` that is not a `value`-bound brand guard must stay avoidable"
    );
}

/// `instanceof ErrorClass` over an erased operand is a boundary.
///
/// The sibling of `host_brand_instanceof_guard_is_a_boundary`, and the case
/// a `catch` binding is: the guard reads the thrown value's `__smelt_error`
/// brand slot back to recover its class. TypeScript types a `catch` binding
/// `unknown`, and the exception channel it arrives on carries payloads of
/// unrelated shape on a run-time branch, so no concrete type can stand in.
///
/// The marker is the brand probe itself, so an erased local beside the
/// guard, and an ordinary read of an error record's own `message` field,
/// both stay classified as they were.
#[test]
fn error_brand_instanceof_guard_is_a_boundary() {
    let guard = "    _smelt_tmp_3 = matches!(error.clone(), SmeltUnknown::Object(smelt_value) if matches!(smelt_value.get(\"__smelt_error\"), Some(SmeltUnknown::String(smelt_error_class)) if &*smelt_error_class == \"RangeError\"));";
    assert_eq!(
        classify_line(guard, false),
        Category::LegitimateBoundary,
        "a caught error recovers its class from the thrown value's brand"
    );
    let storage = "    let _smelt_tmp_4: SmeltUnknown;";
    assert_eq!(
        classify_line(storage, false),
        Category::AvoidableErasure,
        "an ordinary erased local must stay avoidable"
    );
    let bag = "    let thrown: SmeltRecord<String, SmeltUnknown> = SmeltRecord::new();";
    assert_eq!(
        classify_line(bag, false),
        Category::AvoidableErasure,
        "an erased record beside the guard must stay avoidable"
    );
}

/// Erasing a value to hand it to the JSON serializer is a boundary.
///
/// `JSON.stringify`'s output is ECMA-262's, and every one of its rules
/// lives in `Serialize for SmeltUnknown`, so the value crosses at this call
/// and nowhere else. Ordinary erased storage beside it stays avoidable, so
/// the marker does not widen to every line that mentions serde.
#[test]
fn json_serializer_argument_is_a_boundary() {
    let stringify = "    _smelt_tmp_3 = smelt_json_stringify(&SmeltUnknown::Number(4.0 as f64)).expect(\"JSON serialization failed\");";
    assert_eq!(
        classify_line(stringify, false),
        Category::LegitimateBoundary,
        "the JSON serializer's argument is the JSON value boundary"
    );
    let storage = "    let payloads: Vec<SmeltUnknown> = Vec::new();";
    assert_eq!(
        classify_line(storage, false),
        Category::AvoidableErasure,
        "ordinary erased storage must stay avoidable"
    );
    let parse =
        "    let parsed: SmeltUnknown = serde_json::from_str(text).unwrap_or(SmeltUnknown::Null);";
    assert_eq!(
        classify_line(parse, false),
        Category::AvoidableErasure,
        "the marker is the stringify argument, not every serde call"
    );
}

/// A class's erased prototype slot is a boundary.
///
/// The class itself stays concrete — this is only the adapter that lets an
/// already-erased instance answer a method looked up by string name. An
/// ordinary erased local is unaffected, so the marker does not widen past
/// the slot.
#[test]
fn erased_class_prototype_slot_is_a_boundary() {
    for line in [
        "    fn __smelt_proto_entries(&self) -> Vec<(String, SmeltUnknown)> {",
        "        let mut smelt_proto_entries: Vec<(String, SmeltUnknown)> = Vec::new();",
        "        smelt_object_entries.extend(smelt_struct_value.__smelt_proto_entries());",
        "    pub(crate) fn __smelt_prototype_methods() -> Vec<(String, SmeltUnknown)> {",
    ] {
        assert_eq!(
            classify_line(line, false),
            Category::LegitimateBoundary,
            "a method reached by runtime name on an erased instance has no \
             concrete, union, or generic spelling: {line}"
        );
    }
    assert_eq!(
        classify_line(
            "    let entries: Vec<(String, SmeltUnknown)> = Vec::new();",
            false
        ),
        Category::AvoidableErasure,
        "an ordinary erased entry list is not the prototype slot"
    );
}

#[test]
fn erased_typeof_call_is_a_boundary() {
    // `typeof` on an erased value asks for that value's runtime tag. The
    // inlined nine-arm `match` this call replaced was already classified as
    // a boundary (its `SmeltUnknown::Function` arm matched), so the helper
    // call must classify the same way -- otherwise factoring an identical
    // table out of 20-odd sites would read as an erasure regression.
    let call = "    _smelt_tmp_11 = smelt_typeof(&a).to_owned();";
    assert_eq!(
        classify_line(call, false),
        Category::LegitimateBoundary,
        "reading an erased value's runtime tag is runtime narrowing"
    );
    // The residual erased value on a line that ALSO calls `smelt_typeof` is
    // covered by the same boundary, exactly as it was when the tag match was
    // spelled out inline on that line.
    let nested = "    _smelt_tmp_4 = smelt_typeof(&(match nested.clone() { SmeltUnknown::Object(map) => smelt_get_object_field(&map, \"k\"), _ => SmeltUnknown::Undefined })).to_owned();";
    assert_eq!(
        classify_line(nested, false),
        Category::LegitimateBoundary,
        "the tag read covers the erased operand it narrows"
    );
    // A concrete union's `typeof` never mentions `SmeltUnknown`, so it is
    // not counted at all; ordinary erased storage stays avoidable.
    let storage = "    let tag: SmeltUnknown = value.clone();";
    assert_eq!(
        classify_line(storage, false),
        Category::AvoidableErasure,
        "ordinary erased storage must stay avoidable"
    );
}

#[test]
fn this_receiver_channel_is_a_boundary() {
    // One erased callable reached through two structurally unrelated
    // receivers. `capped` is created once, with one fixed Rust signature,
    // and is then invoked as a method of two objects that share no field.
    // Whatever type the receiver read could have been given at creation
    // time would be wrong for at least one of these calls, so no concrete
    // struct, generated union arm, or scoped generic can represent it --
    // only the dynamic channel can, and the value is recovered by narrowing
    // the tag.
    for line in [
        "    let _smelt_tmp_1: SmeltUnknown = smelt_this();",
        "    let _smelt_tmp_2: SmeltUnknown = smelt_frame_this.clone();",
        "    let _smelt_tmp_4 = capped.smelt_bind_this(SmeltUnknown::Object(left.clone()));",
        "    let _smelt_tmp_5 = capped.smelt_bind_this(SmeltUnknown::Object(right.clone()));",
        "        let _smelt_this_guard = smelt_push_this(receiver.clone());",
    ] {
        assert_eq!(
            classify_line(line, false),
            Category::LegitimateBoundary,
            "the `this` receiver is supplied by the call site, not by any \
             statically known type: {line}"
        );
    }
    // Storing an ordinary program value as `SmeltUnknown` is still
    // avoidable erasure -- the marker must not widen past the channel.
    assert_eq!(
        classify_line(
            "    let total: SmeltUnknown = SmeltUnknown::Number(1.0);",
            false
        ),
        Category::AvoidableErasure,
        "an ordinary erased local is not the `this` channel"
    );
}

/// The panic-recovery exception payload is a dynamic boundary, and the
/// marker does not swallow ordinary error-record construction.
///
/// A `catch` binding has no static type — TypeScript types it `unknown` —
/// and the payload is rebuilt from a recovered panic message, so no concrete
/// struct, generated union arm, or scoped generic can represent it: the
/// branch that produced the panic is only known at run time. Only the
/// recovery site's `__smelt_error` *message binding* is matched, so an
/// `Error` record built from program values keeps whatever classification it
/// had.
#[test]
fn panic_recovery_payload_is_a_boundary() {
    let recovery = "            let e = SmeltUnknown::Object(SmeltObject::new(Vec::from([(\"__smelt_error\".to_owned(), SmeltUnknown::String(\"Error\".into())), (\"message\".to_owned(), SmeltUnknown::String(__smelt_error.into()))])));";
    assert_eq!(
        classify_line(recovery, false),
        Category::LegitimateBoundary,
        "the recovered panic payload is the exception ABI, not program storage"
    );
    // The same record shape built from a program value (a source
    // `new Error(message)`) is NOT reclassified by this marker.
    let program_error = "            let e = SmeltUnknown::Object(SmeltObject::new(Vec::from([(\"__smelt_error\".to_owned(), SmeltUnknown::String(\"Error\".into())), (\"message\".to_owned(), SmeltUnknown::String(message.clone().into()))])));";
    assert_eq!(
        classify_line(program_error, false),
        Category::AvoidableErasure,
        "an Error record built from program values keeps its classification"
    );
}

/// The inverse direction of the same boundary: a stored property key handed
/// back to the program as a symbol VALUE.
///
/// `Object.getOwnPropertySymbols` / `Reflect.ownKeys` / erased-Map
/// enumeration produce symbols, and round 10 moved that conversion behind
/// one prelude helper so the frontend and the runtime derive a symbol's key
/// the same way. The `SmeltUnknown::Symbol` tag therefore sits inside the
/// helper rather than at the call site, so the call site carries its own
/// marker and keeps the classification the inline spelling had.
#[test]
fn symbol_key_inverse_is_a_boundary() {
    let own_symbols = "    let keys: SmeltList<SmeltUnknown> = Into::<SmeltList<_>>::into(record.keys().filter_map(|key| smelt_own_symbol_key_value(&key)).collect::<Vec<_>>());";
    assert_eq!(
        classify_line(own_symbols, false),
        Category::LegitimateBoundary,
        "recovering a symbol value from its key is the same boundary as minting one"
    );
    // A list of erased values built any other way keeps its classification:
    // the marker names one helper, not "a list of SmeltUnknown".
    let plain_list = "    let keys: SmeltList<SmeltUnknown> = Into::<SmeltList<_>>::into(record.keys().map(|key| SmeltUnknown::String(key.into())).collect::<Vec<_>>());";
    assert_eq!(
        classify_line(plain_list, false),
        Category::AvoidableErasure,
        "a string-keyed projection is still avoidable erasure"
    );
}

/// The hoisted erased-record-to-class conversion keeps the boundary
/// classification its inline spelling had.
#[test]
fn record_conversion_helper_is_a_boundary() {
    let signature = "fn __smelt_from_unknown_record_0(smelt_value: SmeltUnknown) -> Context {";
    assert_eq!(
        classify_line(signature, false),
        Category::LegitimateBoundary,
        "the helper is the erased-to-class adapter that used to be inlined"
    );
    let inline = "    _smelt_tmp_10 = match (err).into_smelt_unknown() { SmeltUnknown::Object(values) => { let smelt_record_map = values.smelt_shared_record(); }, _ => Default::default() };";
    assert_eq!(
        classify_line(inline, false),
        Category::LegitimateBoundary,
        "the inline spelling it replaces was a boundary"
    );
    let ordinary = "fn helper(value: SmeltUnknown) -> Context {";
    assert_eq!(
        classify_line(ordinary, false),
        Category::AvoidableErasure,
        "an ordinary function taking SmeltUnknown is still avoidable erasure"
    );
}

/// An untyped `JSON.parse` result is a run-time JSON value.
#[test]
fn untyped_json_parse_is_a_boundary() {
    let line = r#"    let _smelt_tmp_14: SmeltUnknown = smelt_json_parse(&"{\"maxAge\": 12.7}".to_owned())?;"#;
    assert_eq!(
        classify_line(line, false),
        Category::LegitimateBoundary,
        "a parsed JSON document's shape exists only at run time"
    );
    // One parse yields different SHAPES from different texts; no concrete
    // type, union arm or scoped generic chosen at compile time can hold
    // `null`, `3` and `{..}` from the same call site except the erased
    // carrier (runtime fixture `json_parse_any_value` runs all of them).
    let typed = "    let _smelt_tmp_3: SmeltRecord<String, SmeltUnknown> = Default::default();";
    assert_eq!(classify_line(typed, false), Category::AvoidableErasure);
}

/// A `node:sqlite` row read is a run-time document, like an untyped
/// `JSON.parse`; its parameters are not.
#[test]
fn sqlite_row_read_is_a_boundary() {
    let all = "    let _smelt_tmp_4: SmeltList<SmeltUnknown> = statement.smelt_sqlite_all(&[])?;";
    let get = "    let _smelt_tmp_5: SmeltUnknown = statement.smelt_sqlite_get(&[&id])?;";
    for line in [all, get] {
        assert_eq!(
            classify_line(line, false),
            Category::LegitimateBoundary,
            "a row's columns are chosen by the SQL text at run time: {line}"
        );
    }
    // One statement yields different column sets from the same call site
    // when the schema it reads changes under it (`SELECT *` after an
    // `ALTER TABLE ... ADD COLUMN`); no struct, union arm or scoped generic
    // fixed at compile time holds both (runtime fixture
    // `node_sqlite_database` reads rows with INTEGER, REAL, TEXT and NULL
    // columns from one statement). A parameter erased by hand is NOT
    // covered: parameters bind at their own types.
    let erased_parameter = "    let changes = statement.run(&[&SmeltUnknown::Number(id as f64)])?;";
    assert_eq!(
        classify_line(erased_parameter, false),
        Category::AvoidableErasure
    );
}

/// An optional erased value read at the erased carrier is an adapter between
/// two erased spellings, not new erasure.
#[test]
fn optional_unknown_read_is_a_boundary() {
    let line = "    _smelt_tmp_3 = smelt_optional_unknown_or_undefined(value.clone());";
    assert_eq!(
        classify_line(line, false),
        Category::LegitimateBoundary,
        "an optional erased value read at the erased carrier adds no erasure"
    );
    assert!(line.contains(smelt_codegen_rust::OPTIONAL_UNKNOWN_OR_UNDEFINED_FN));
    // Spelling the same absence by hand is not a marker: only the named
    // adapter, whose input is already erased, is classified as a boundary.
    let by_hand = "    _smelt_tmp_3 = value.clone().unwrap_or(SmeltUnknown::Undefined);";
    assert_eq!(classify_line(by_hand, false), Category::AvoidableErasure);
}

/// An erased generic array handed to another erased slot is a boundary
/// adapter; the general list conversion and a source `unknown[]` wrap are
/// not.
///
/// The lines are the generated spellings of the two alias helpers
/// (`smelt_codegen_rust::erased_list_alias`). Before #250 the same
/// conversions rebuilt the array through `into_smelt_unknown`, which this
/// classifier already counted as a boundary; the helper names keep that
/// classification for the aliasing spelling.
#[test]
fn erased_generic_list_alias_is_a_boundary() {
    let list_alias = "            let _smelt_tmp_3: SmeltList<SmeltUnknown> = smelt_alias_erased_generic_list(items.clone());";
    assert_eq!(
        classify_line(list_alias, false),
        Category::LegitimateBoundary,
        "an erased generic list handed to an erased-list slot adds no erasure"
    );
    let array_alias = "    _smelt_tmp_4 = !(SmeltUnknown::Array(smelt_alias_erased_generic_array(a.clone())) == SmeltUnknown::Array(smelt_alias_erased_generic_array(b.clone())));";
    assert_eq!(
        classify_line(array_alias, false),
        Category::LegitimateBoundary,
        "an erased generic list wrapped as an `unknown` array adds no erasure"
    );
    // The names are the codegen crate's own constants, so a rename on that
    // side cannot silently drop the classification.
    assert!(list_alias.contains(smelt_codegen_rust::ERASED_GENERIC_LIST_ALIAS_FN));
    assert!(array_alias.contains(smelt_codegen_rust::ERASED_GENERIC_ARRAY_ALIAS_FN));
    // The general list conversion is not a marker, and neither is the
    // direct wrap a source `unknown[]` keeps: both stay avoidable.
    let general = "            let _smelt_tmp_3: SmeltList<SmeltUnknown> = Into::<SmeltList<_>>::into(items.clone());";
    assert_eq!(
        classify_line(general, false),
        Category::AvoidableErasure,
        "the general list conversion must not hide erasure"
    );
    let source_unknown = "    _smelt_tmp_4 = !(SmeltUnknown::Array(a.clone().into()) == SmeltUnknown::Array(b.clone().into()));";
    assert_eq!(
        classify_line(source_unknown, false),
        Category::AvoidableErasure,
        "a source `unknown[]` wrap keeps its avoidable classification"
    );
}

/// A JavaScript symbol VALUE has no static counterpart; its use as a member
/// KEY has one and must stay unaffected.
/// A `finally`'s pending thrown value is the exception payload: any
/// JavaScript value, held only between the handler and the rethrow. An
/// ordinary erased temporary next to it stays avoidable.
#[test]
fn pending_thrown_value_is_a_boundary() {
    for line in [
        "    let mut smelt_pending_thrown: SmeltUnknown;",
        "    smelt_pending_thrown = SmeltUnknown::Undefined;",
        "    let mut smelt_pending_thrown_1: SmeltUnknown;",
    ] {
        assert_eq!(
            classify_line(line, false),
            Category::LegitimateBoundary,
            "{line}"
        );
    }
    assert_eq!(
        classify_line("    let _smelt_tmp_4: SmeltUnknown;", false),
        Category::AvoidableErasure
    );
}

#[test]
fn symbol_value_is_a_boundary() {
    let symbol_value =
        "    let key: SmeltUnknown = SmeltUnknown::Symbol(\"Symbol()@21\".to_owned().into());";
    assert_eq!(
        classify_line(symbol_value, false),
        Category::LegitimateBoundary,
        "a symbol's identity is a run-time property with no static carrier"
    );
    // The KEY side is static and carries no erasure at all: a module-level
    // `const K = Symbol()` folds to a synthetic member name and the read is
    // an ordinary field access. Nothing here should reclassify a line that
    // merely mentions the folded name.
    let member_read = "    let value: f64 = holder.__smelt_get___smelt_symbol_unique_symbol_21();";
    assert_eq!(
        count_occurrences(member_read, "SmeltUnknown"),
        0,
        "a symbol-keyed member read is not an erasure site at all"
    );
}

/// The global object and the in-place `Object.assign` onto it are
/// boundaries: the members exist only because some earlier statement (or
/// the host) put them there, so a read's shape is a run-time fact.
///
/// The contrast lines stay avoidable: an erased LOCAL declared next to the
/// global object, and a typed record merge, carry no boundary marker.
#[test]
fn global_object_members_are_run_time_facts() {
    let read = "    let manifest: SmeltUnknown = smelt_get_unknown_field(&smelt_global_object(), \"__MANIFEST\").clone();";
    assert_eq!(classify_line(read, false), Category::LegitimateBoundary);
    let write = "    let _smelt_tmp_3: SmeltUnknown = { let mut smelt_target = smelt_global_object(); smelt_unknown_assign(&mut smelt_target, &SmeltUnknown::Object(SmeltObject::new(Vec::new()))); smelt_target };";
    assert_eq!(classify_line(write, false), Category::LegitimateBoundary);
    let helper = "fn smelt_global_object() -> SmeltUnknown { thread_local! { static SMELT_GLOBAL_OBJECT: SmeltObject = SmeltObject::new(Vec::new()); } SmeltUnknown::Object(SMELT_GLOBAL_OBJECT.with(Clone::clone)) }";
    assert_eq!(classify_line(helper, false), Category::RuntimePrelude);
    let erased_local = "    let mut _smelt_tmp_4: SmeltUnknown;";
    assert_eq!(
        classify_line(erased_local, false),
        Category::AvoidableErasure
    );
}

/// Counting is non-overlapping and handles repeats on one line.
#[test]
fn counts_occurrences_per_line() {
    assert_eq!(
        count_occurrences("SmeltUnknown SmeltUnknown", "SmeltUnknown"),
        2
    );
    assert_eq!(count_occurrences("nothing here", "SmeltUnknown"), 0);
    assert_eq!(count_occurrences("aaa", "aa"), 1);
}

/// Fallibility-wrapping annotation tokens are excluded from the value count.
#[test]
fn count_value_erasures_excludes_annotation_spellings() {
    // The `Ok::<...>` turbofish annotates the Result Ok arm; wrapping a bare
    // return value in it must not raise the erased-value count.
    assert_eq!(
        count_value_erasures("        Ok::<SmeltUnknown, Box<dyn std::error::Error>>(value)"),
        count_value_erasures("        value"),
        "the Ok turbofish adds no erased value versus the unwrapped return"
    );
    // A genuine erased value inside the Ok turbofish is still counted once.
    assert_eq!(
        count_value_erasures(
            "        Ok::<SmeltUnknown, Box<dyn std::error::Error>>(SmeltUnknown::Undefined)"
        ),
        1
    );

    // A closure signature made fallible must not inflate versus the erased
    // literal it replaced (which carried no `SmeltUnknown` spelling).
    assert_eq!(
        count_value_erasures(
            "    let cb: Rc<dyn Fn(SmeltList<i64>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = f;"
        ),
        count_value_erasures("    let cb: SmeltErasedFunction = f;"),
        "a fallible closure return annotation must be count-neutral"
    );

    // A bare `-> SmeltUnknown` return still counts its value.
    assert_eq!(count_value_erasures("fn f() -> SmeltUnknown {"), 1);
}

/// Prelude definition starts and single-line helpers toggle state correctly.
#[test]
fn tracks_prelude_helper_regions() {
    let mut in_prelude = false;
    update_prelude_helper_state("pub enum SmeltUnknown {", &mut in_prelude);
    assert!(in_prelude, "enum def opens a prelude region");
    update_prelude_helper_state("    Number(f64),", &mut in_prelude);
    assert!(in_prelude, "field lines stay inside the region");
    update_prelude_helper_state("}", &mut in_prelude);
    assert!(!in_prelude, "left-margin close ends the region");

    // A one-line helper opens and closes on the same line.
    update_prelude_helper_state(
        "fn smelt_missing() -> SmeltUnknown { SmeltUnknown::Undefined }",
        &mut in_prelude,
    );
    assert!(!in_prelude, "one-line helper does not leak state");
}

/// The enum definition and helper bodies classify as runtime prelude.
#[test]
fn classifies_runtime_prelude() {
    assert_eq!(
        classify_line("pub enum SmeltUnknown {", false),
        Category::RuntimePrelude
    );
    assert_eq!(
        classify_line("    SmeltUnknown::Number(*value)", true),
        Category::RuntimePrelude,
        "inside a helper body everything is prelude"
    );
}

/// Erased callables, promises, and adapters are legitimate boundaries.
#[test]
fn classifies_legitimate_boundaries() {
    assert_eq!(
        classify_line("let f = SmeltUnknown::Function(rc);", false),
        Category::LegitimateBoundary
    );
    assert_eq!(
        classify_line("SmeltUnknown::Promise(promise)", false),
        Category::LegitimateBoundary
    );
    assert_eq!(
        classify_line("value.into_smelt_unknown()", false),
        Category::LegitimateBoundary
    );
}

/// Program storage and signatures classify as avoidable erasure.
#[test]
fn classifies_avoidable_erasure() {
    assert_eq!(
        classify_line("fn my_program_fn(x: SmeltUnknown) -> SmeltUnknown {", false),
        Category::AvoidableErasure
    );
    assert_eq!(
        classify_line("let values: Vec<SmeltUnknown> = Vec::new();", false),
        Category::AvoidableErasure
    );
}

/// A `__smelt_update_tmp` snapshot whose type is `SmeltUnknown` only because
/// its update target is an already-erased dynamic value (e.g. an erased
/// `unknown` callback parameter in `(v) => v++`) is a boundary adapter, not
/// avoidable program storage: no concrete type can represent it because the
/// underlying target is erased. Array-padding index uses that merely
/// reference the temp stay avoidable.
#[test]
fn classifies_update_snapshot_at_erased_target_as_boundary() {
    assert_eq!(
        classify_line(
            "    let __smelt_update_tmp: SmeltUnknown = _smelt_tmp_2.clone();",
            false
        ),
        Category::LegitimateBoundary
    );
    assert_eq!(
        classify_line(
            "    let __smelt_update_tmp_1: SmeltUnknown = closure_arg_0.clone();",
            false
        ),
        Category::LegitimateBoundary
    );
    // Referencing the temp as an array index is not the boundary: array
    // padding with `SmeltUnknown::Null`/`Undefined` remains avoidable.
    assert_eq!(
        classify_line(
            "    arr.resize(smelt_assign_index, SmeltUnknown::Null); let i = __smelt_update_tmp.clone();",
            false
        ),
        Category::AvoidableErasure
    );
}

/// Normalization collapses literals and digits but keeps identifiers.
#[test]
fn normalize_collapses_literals_keeps_identifiers() {
    assert_eq!(
        normalize_line("    let x: Vec<SmeltUnknown> = vec![42, 7];"),
        "let x: Vec<SmeltUnknown> = vec![N, N];"
    );
    assert_eq!(
        normalize_line("SmeltUnknown::String(\"hello\".to_owned())"),
        "SmeltUnknown::String(\"S\".to_owned())"
    );
}

/// Whether each line of `text` reads as inside a generated adapter.
fn adapter_lines(text: &str) -> Vec<bool> {
    let mut adapter = AdapterBody::default();
    text.lines().map(|line| adapter.advance(line)).collect()
}

/// Adapter bodies are boundary code, and the rule stops at their closing
/// brace: a program type has no concrete form for `SmeltUnknown::Object`,
/// because producing the erased object is what the adapter is for.
#[test]
fn adapter_bodies_are_boundary_and_end_at_their_brace() {
    let text = "impl IntoSmeltUnknown for Route {\n    fn into_smelt_unknown(self) -> SmeltUnknown {\n        SmeltUnknown::Object(object)\n    }\n}\nfn program() -> SmeltUnknown {\n}\n";
    assert_eq!(
        adapter_lines(text),
        [true, true, true, true, true, false, false]
    );
}

/// Program methods beside `from_smelt_unknown` in one `impl` stay judged,
/// so erasure in them still counts as avoidable.
#[test]
fn program_methods_beside_an_adapter_method_stay_avoidable() {
    let text = "impl Route {\n    fn from_smelt_unknown(value: SmeltUnknown) -> Self {\n        Self::default()\n    }\n    fn path(&self) -> SmeltUnknown {\n        SmeltUnknown::Null\n    }\n}\n";
    assert_eq!(
        adapter_lines(text),
        [false, true, true, true, false, false, false, false]
    );
}

/// Naming the trait in a bound does not make an item an adapter.
#[test]
fn a_generic_bound_is_not_an_adapter() {
    let text = "impl<T: Clone + IntoSmeltUnknown + SmeltFromUnknown> Default for Cell<T> {\n    fn default() -> Self {\n    }\n}\n";
    assert_eq!(adapter_lines(text), [false, false, false, false]);
}

/// Record recovery and JSON decoders are adapters; a one-line item closes
/// on its own line rather than swallowing what follows.
#[test]
fn decoders_are_adapters_and_one_liners_close_themselves() {
    let text = "fn smelt_decode_6(value: &SmeltUnknown) -> bool { true }\nfn program(value: SmeltUnknown) {\n}\nfn __smelt_from_unknown_record_1(smelt_value: SmeltUnknown) -> Raw {\n    decode(smelt_value)\n}\n";
    assert_eq!(adapter_lines(text), [true, false, false, true, true, true]);
}

/// A full scan groups by shape, tallies categories, and stays deterministic.
#[test]
fn scan_groups_and_categorizes() {
    let dir =
        std::env::temp_dir().join(format!("smelt-unknown-report-test-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("gen.rs");
    std::fs::write(
        &file,
        concat!(
            "// @generated by smelt.\n",
            "pub enum SmeltUnknown {\n",
            "    Function(Rc<dyn Fn(Vec<SmeltUnknown>)>),\n",
            "}\n",
            "fn program(x: SmeltUnknown) -> SmeltUnknown {\n",
            "    let cb = SmeltUnknown::Function(rc);\n",
            "    x\n",
            "}\n",
        ),
    )
    .unwrap();

    let report = scan_files(&[file]).unwrap().report;
    assert_eq!(report.files_scanned, 1);
    // enum line (1) + Function field (1) inside enum = 2 prelude;
    // signature line has 2; Function construction has 1 legitimate.
    assert!(report.category_total(Category::RuntimePrelude) >= 2);
    assert_eq!(report.category_total(Category::LegitimateBoundary), 1);
    assert_eq!(report.category_total(Category::AvoidableErasure), 2);

    // Re-scanning yields identical output (determinism).
    let json_a = serde_json::to_string(&report).unwrap();
    let report_b = scan_files(&[dir.join("gen.rs")]).unwrap().report;
    let json_b = serde_json::to_string(&report_b).unwrap();
    assert_eq!(json_a, json_b);

    drop(std::fs::remove_dir_all(&dir));
}

/// When the emitter's prelude sentinel is present, everything above it is
/// classified as prelude regardless of the per-line heuristics — so a program
/// signature accidentally sitting above the marker would still be prelude,
/// and a boundary-looking line below it is judged on its own merits.
#[test]
fn marker_defines_authoritative_prelude_boundary() {
    let dir = std::env::temp_dir().join(format!(
        "smelt-unknown-report-marker-{}",
        std::process::id()
    ));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("gen.rs");
    // The pre-marker `fn helper` signature looks like avoidable program
    // erasure, but because it sits above the sentinel the boundary makes it
    // prelude; the post-marker `fn program` signature stays avoidable.
    std::fs::write(
        &file,
        format!(
            concat!(
                "// @generated by smelt.\n",
                "fn helper(x: SmeltUnknown) -> SmeltUnknown {{ x }}\n",
                "{marker}\n",
                "fn program(x: SmeltUnknown) -> SmeltUnknown {{ x }}\n",
            ),
            marker = smelt_codegen_rust::PRELUDE_END_MARKER,
        ),
    )
    .unwrap();

    let report = scan_files(&[file]).unwrap().report;
    // The pre-marker signature (2 occurrences) is prelude; the post-marker
    // program signature (2 occurrences) is avoidable erasure.
    assert_eq!(report.category_total(Category::RuntimePrelude), 2);
    assert_eq!(report.category_total(Category::AvoidableErasure), 2);
    assert_eq!(report.category_total(Category::LegitimateBoundary), 0);

    drop(std::fs::remove_dir_all(&dir));
}

/// A module file names its source in its header, so it holds program code
/// only: a program item the emitter named with its own prefix (a module
/// global's initializer) is measured like any other program code instead of
/// being taken for a runtime helper by its name. Without the header (output
/// that predates it) the name heuristic still decides.
#[test]
fn a_module_file_measures_its_minted_program_items() {
    let dir = std::env::temp_dir().join(format!("smelt-unknown-module-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    assert!(std::fs::create_dir_all(&dir).is_ok(), "temp dir");
    let init =
        "fn smelt_global_init__table__0() -> SmeltUnknown {\n    SmeltUnknown::Number(1.0)\n}\n";
    let module = dir.join("source_main.rs");
    let written = std::fs::write(
        &module,
        format!(
            "// @generated by smelt. Do not edit by hand.\n{header}src/main.ts\nuse super::*;\n\n{init}",
            header = smelt_codegen_rust::MODULE_SOURCE_HEADER,
        ),
    );
    assert!(written.is_ok(), "module file written");
    let report = scan_files(std::slice::from_ref(&module))
        .ok()
        .map(|scan| scan.report);
    let total = |category| {
        report
            .as_ref()
            .map(|scanned| scanned.category_total(category))
    };
    assert_eq!(
        total(Category::RuntimePrelude),
        Some(0),
        "a module file holds no prelude"
    );
    assert_eq!(
        total(Category::AvoidableErasure),
        Some(2),
        "the initializer is program code"
    );

    let legacy = dir.join("legacy.rs");
    assert!(std::fs::write(&legacy, init).is_ok(), "legacy file written");
    let legacy_prelude = scan_files(&[legacy])
        .ok()
        .map(|scan| scan.report.category_total(Category::RuntimePrelude));
    assert_eq!(
        legacy_prelude,
        Some(2),
        "no header: the name heuristic decides"
    );
    drop(std::fs::remove_dir_all(&dir));
}

/// Markdown rendering emits the category table and flags regressions.
#[test]
fn markdown_flags_avoidable_regression() {
    let baseline = UnknownReport {
        version: SchemaVersion::CURRENT,
        files_scanned: 1,
        total: 2,
        category_totals: BTreeMap::from([(Category::AvoidableErasure.as_str().to_owned(), 2usize)]),
        shapes: Vec::new(),
        erasure: None,
    };
    let current = UnknownReport {
        version: SchemaVersion::CURRENT,
        files_scanned: 1,
        total: 5,
        category_totals: BTreeMap::from([(Category::AvoidableErasure.as_str().to_owned(), 5usize)]),
        shapes: Vec::new(),
        erasure: None,
    };
    let markdown = render_markdown(&current, Some(&baseline));
    assert!(markdown.contains("## Occurrences by category"));
    assert!(markdown.contains("Regression to review"));
    assert!(markdown.contains("rose by 3"));
}

/// No regression note when avoidable erasure holds steady or drops.
#[test]
fn markdown_reports_no_regression_when_flat() {
    let baseline = UnknownReport {
        version: SchemaVersion::CURRENT,
        files_scanned: 1,
        total: 4,
        category_totals: BTreeMap::from([(Category::AvoidableErasure.as_str().to_owned(), 4usize)]),
        shapes: Vec::new(),
        erasure: None,
    };
    let current = baseline.clone();
    let markdown = render_markdown(&current, Some(&baseline));
    assert!(markdown.contains("did not increase"));
}

/// Build a [`ScanResult`] fixture with the given avoidable total and per-file
/// tally, for exercising [`detect_regression`] without touching disk.
fn fixture_scan(avoidable: usize, avoidable_by_file: &[(&str, usize)]) -> ScanResult {
    ScanResult {
        report: UnknownReport {
            version: SchemaVersion::CURRENT,
            files_scanned: avoidable_by_file.len(),
            total: avoidable,
            category_totals: BTreeMap::from([(
                Category::AvoidableErasure.as_str().to_owned(),
                avoidable,
            )]),
            shapes: Vec::new(),
            erasure: None,
        },
        avoidable_by_file: avoidable_by_file
            .iter()
            .map(|(file, count)| ((*file).to_owned(), *count))
            .collect(),
    }
}

/// A fabricated baseline report carrying just an avoidable total.
fn fixture_baseline(avoidable: usize) -> UnknownReport {
    UnknownReport {
        version: SchemaVersion::CURRENT,
        files_scanned: 1,
        total: avoidable,
        category_totals: BTreeMap::from([(
            Category::AvoidableErasure.as_str().to_owned(),
            avoidable,
        )]),
        shapes: Vec::new(),
        erasure: None,
    }
}

/// A scan of nothing cannot judge, with or without a baseline to compare.
#[test]
fn coverage_loss_flags_a_scan_of_no_files() {
    let empty = fixture_scan(0, &[]).report;
    assert!(coverage_loss(&empty, None).is_some());
    assert!(coverage_loss(&empty, Some(&fixture_baseline(0))).is_some());
}

/// Losing up to a tenth of the baseline's files passes; losing more fails,
/// though the smaller scan reports lower erasure and passes [`detect_regression`].
#[test]
fn coverage_loss_flags_a_large_drop_in_files_scanned() {
    let mut baseline = fixture_baseline(4);
    baseline.files_scanned = 100;
    let mut scan = fixture_scan(0, &[("a.rs", 0)]);

    scan.report.files_scanned = 90;
    assert_eq!(coverage_loss(&scan.report, Some(&baseline)), None);
    scan.report.files_scanned = 89;
    let message = coverage_loss(&scan.report, Some(&baseline)).expect("a large drop cannot judge");
    assert!(
        message.contains("89") && message.contains("100"),
        "{message}"
    );
    assert!(detect_regression(&scan, &baseline).is_none());
    scan.report.files_scanned = 150;
    assert_eq!(coverage_loss(&scan.report, Some(&baseline)), None);
}

/// A rise above the baseline is a regression naming the delta and the top
/// offending files, ranked by avoidable count.
#[test]
fn detect_regression_flags_increase_with_top_files() {
    let scan = fixture_scan(10, &[("a.rs", 2), ("b.rs", 7), ("c.rs", 1)]);
    let baseline = fixture_baseline(4);
    let regression = detect_regression(&scan, &baseline).expect("increase is a regression");
    assert_eq!(regression.baseline, 4);
    assert_eq!(regression.current, 10);
    assert_eq!(regression.delta, 6);
    // Ranked by count, most first.
    assert_eq!(regression.top_files[0], ("b.rs".to_owned(), 7));
    assert_eq!(regression.top_files[1], ("a.rs".to_owned(), 2));
    assert_eq!(regression.top_files[2], ("c.rs".to_owned(), 1));
}

/// A flat or falling avoidable count is never a regression.
#[test]
fn detect_regression_ignores_flat_and_decrease() {
    let flat = fixture_scan(4, &[("a.rs", 4)]);
    assert!(detect_regression(&flat, &fixture_baseline(4)).is_none());
    let lower = fixture_scan(2, &[("a.rs", 2)]);
    assert!(detect_regression(&lower, &fixture_baseline(9)).is_none());
}

/// A gate needs a baseline to compare against.
///
/// Old requirement: `--fail-on-regression` without `--baseline` was accepted
/// and exited 0, which let a gate pass without measuring anything. New
/// requirement: clap rejects that command line, and [`detect_regression`]
/// takes a baseline by value, so no path reaches it without one.
#[test]
fn detect_regression_requires_baseline() {
    use clap::Parser as _;
    let gate = [
        "smelt",
        "smelt-unknown-report",
        "src",
        "--fail-on-regression",
    ];
    let error = crate::cli_parser::Args::try_parse_from(gate).expect_err("no baseline");
    assert_eq!(
        error.kind(),
        clap::error::ErrorKind::MissingRequiredArgument
    );
    assert!(error.to_string().contains("--baseline"), "{error}");
}

/// A scanned tree holding `avoidable` erased struct fields.
fn scanned_tree(avoidable: usize) -> tempfile::TempDir {
    let tree = tempfile::tempdir().expect("temp dir");
    let fields = "    value: SmeltUnknown,\n".repeat(avoidable);
    std::fs::write(
        tree.path().join("a.rs"),
        format!("struct S {{\n{fields}}}\n"),
    )
    .expect("source");
    tree
}

/// Run `--update-baseline` over `tree`, returning the gate's verdict.
fn update(tree: &Path, baseline: &Path) -> GatedOutcome {
    unknown_report(&UnknownReportOptions {
        roots: &[tree.to_path_buf()],
        format: ReportFormat::Json,
        baseline: Some(baseline.to_path_buf()),
        update_baseline: true,
        erasure_sidecars: &[],
    })
    .expect("report")
}

fn avoidable_in(baseline: &Path) -> usize {
    report::load_json::<UnknownReport>(baseline, "")
        .expect("baseline")
        .category_total(Category::AvoidableErasure)
}

/// A rise in avoidable erasure can never be snapshotted: the old baseline
/// stays byte for byte and the regression is still reported.
#[test]
fn update_baseline_refuses_a_regression() {
    let dir = tempfile::tempdir().expect("temp dir");
    let baseline = dir.path().join("baseline.json");
    update(scanned_tree(1).path(), &baseline);
    assert_eq!(
        avoidable_in(&baseline),
        1,
        "a first update creates the baseline"
    );
    let before = std::fs::read(&baseline).expect("baseline");

    let outcome = update(scanned_tree(3).path(), &baseline);
    assert_eq!(std::fs::read(&baseline).expect("baseline"), before);
    assert!(outcome.regression.is_some());
}

/// A flat or falling count is taken, as the policy's re-snapshot requires.
#[test]
fn update_baseline_takes_a_fall() {
    let dir = tempfile::tempdir().expect("temp dir");
    let baseline = dir.path().join("baseline.json");
    update(scanned_tree(3).path(), &baseline);
    let outcome = update(scanned_tree(1).path(), &baseline);
    assert_eq!(avoidable_in(&baseline), 1);
    assert!(outcome.regression.is_none());
}

/// The top-offenders list is capped so a huge corpus yields a readable message.
#[test]
fn detect_regression_caps_top_files() {
    let files: Vec<(String, usize)> = (0..25)
        .map(|index| (format!("file{index:02}.rs"), index + 1))
        .collect();
    let refs: Vec<(&str, usize)> = files
        .iter()
        .map(|(name, count)| (name.as_str(), *count))
        .collect();
    let scan = fixture_scan(1000, &refs);
    let regression =
        detect_regression(&scan, &fixture_baseline(0)).expect("increase is a regression");
    assert_eq!(regression.top_files.len(), TOP_OFFENDER_LIMIT);
    // The single largest contributor leads.
    assert_eq!(regression.top_files[0].1, 25);
}
