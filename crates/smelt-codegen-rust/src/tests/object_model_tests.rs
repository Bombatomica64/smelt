//! Codegen regression tests for the JavaScript object model rules that used to
//! be modeled as something simpler.
//!
//! These are the cheap emitter-level companions of the
//! `object_model_runtime` tier: they pin the emitted SHAPE (which helper a seam
//! calls, which table a key comes from), while the runtime tier proves the
//! answers.
//!
//! * A named write to an erased array stores into the array's side table
//!   instead of replacing the array with a one-property object.
//! * An erased property read goes through the one prelude helper that knows
//!   about object records, array named properties and the `Object.prototype`
//!   fallback, so `v.k` and `v[k]` cannot answer differently.
//! * `Object.prototype`'s members are a lookup fallback with one cached
//!   identity per member, never stored entries.
//! * A well-known `Symbol.<name>` is a symbol VALUE, while the member it
//!   indexes keeps the shared storage-key spelling.

use super::*;

#[test]
fn a_named_write_to_an_erased_array_keeps_the_array() {
    // Both store seams: the dotted static-member store emitted inline, and the
    // computed store that goes through `smelt_index_assign`.
    let source = source_for(
        r"
export function tagArray(): unknown {
  const values: any = ['1'];
  values.tag = 2;
  return values;
}
",
    );

    let body = emitted_function_body(&source, "fn tag_array");
    assert!(
        body.contains("SmeltUnknown::Array(values) => { values.set_named_property(\"tag\".to_owned()"),
        "a named write on an array receiver must store into the array:\n{body}"
    );
    assert!(
        source.contains("else { array.set_named_property(key, value); }"),
        "the runtime index-assign helper must keep the array for a non-index key"
    );
    assert!(
        !source.contains("else { *target = SmeltUnknown::Object(SmeltObject::new(Vec::from([(key, value)]))); }"),
        "the array arm must no longer replace the array with an object"
    );
}

#[test]
fn an_erased_property_read_goes_through_one_helper() {
    // One helper for every receiver shape. The dotted read used to inline an
    // object-only `match`, which answered `undefined` for an array's named
    // property and for a member of the `Object.prototype` sentinel.
    let source = source_for(
        r"
export function readTag(value: unknown): unknown {
  return (value as any).tag;
}
",
    );

    let body = emitted_function_body(&source, "fn read_tag");
    assert!(
        body.contains("smelt_get_unknown_field("),
        "an erased field read must go through the shared helper:\n{body}"
    );
    assert!(
        source.contains("SmeltUnknown::Array(values) => smelt_get_array_field(values, field)"),
        "the shared helper must read an array's own properties"
    );
    assert!(
        source.contains("__smelt_proto:object\" => smelt_object_prototype_member(field)"),
        "the shared helper must resolve members of the prototype sentinel"
    );
}

#[test]
fn object_prototype_members_are_a_fallback_with_one_identity_each() {
    // The members must be produced by a cached table linked into the function
    // identity registry -- that is what makes two reads `===` -- and they must
    // be reached only after the own and `__smelt_proto:` lookups miss.
    let source = source_for(
        r"
export function readToString(value: Record<string, unknown>): unknown {
  return value['toString'];
}
",
    );

    assert!(
        source.contains(
            "smelt_link_function_identity_key(&function, smelt_method_identity(key)); SmeltUnknown::Function(function)"
        ),
        "each prototype member needs one canonical identity"
    );
    assert!(
        source.contains(
            "SmeltUnknown::Object(map) => match smelt_get_object_field(map, field) { SmeltUnknown::Undefined => smelt_object_prototype_member(field)"
        ),
        "the prototype table must be consulted only after the own/proto lookups miss"
    );
}

#[test]
fn a_well_known_symbol_is_a_value_and_its_key_comes_from_the_shared_table() {
    // The value spelling and the storage key are two different strings for one
    // symbol, and the emitted program has to carry both: `Literal::Symbol` for
    // the value, the `__smelt_symbol_*` member for the key it indexes.
    let source = source_for(
        r"
export function tagged(): unknown {
  const value: any = { [Symbol.toStringTag]: 'x' };
  const key: any = Symbol.toStringTag;
  return value[key];
}
",
    );

    let body = emitted_function_body(&source, "fn tagged");
    assert!(
        body.contains("SmeltUnknown::Symbol(\"Symbol.toStringTag\""),
        "a well-known symbol in value position must be a symbol:\n{body}"
    );
    assert!(
        body.contains("__smelt_symbol_to_string_tag"),
        "the declared member must keep the shared storage-key spelling:\n{body}"
    );
    assert!(
        source.contains("\"Symbol.toStringTag\" => \"__smelt_symbol_to_string_tag\".to_owned()"),
        "the runtime property-key coercion must map the value spelling to that same key"
    );
    assert!(
        source.contains(
            "if let Some(SmeltUnknown::String(tag)) = map.get(\"__smelt_symbol_to_string_tag\")"
        ),
        "a string `@@toStringTag` must win over the builtin object tag"
    );
}

#[test]
fn a_class_extending_a_builtin_error_erases_with_that_error_marker() {
    // The erasure records the NEAREST BUILTIN base, not the user class name, so
    // the marker answers `instanceof TypeError` for a `TypeError` subclass and
    // `instanceof Error` for both, while the user class keeps resolving through
    // `__smelt_class`.
    let source = source_for(
        r"
class CustomTypeError extends TypeError {}
export function erase(): unknown {
  return new CustomTypeError('x');
}
",
    );

    let body = emitted_function_body(&source, "fn erase");
    assert!(
        body.contains("(\"__smelt_error\".to_owned(), SmeltUnknown::String(\"TypeError\".into()))"),
        "the erased subclass instance must carry its builtin error base:\n{body}"
    );
    assert!(
        body.contains("(\"__smelt_class\".to_owned(), SmeltUnknown::String(\"CustomTypeError\".into()))"),
        "the user class identity must survive alongside it:\n{body}"
    );
}
