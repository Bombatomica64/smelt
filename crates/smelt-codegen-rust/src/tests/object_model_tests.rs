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

#[test]
fn a_presence_test_emits_the_helper_for_its_reach() {
    // `Object.hasOwn(v, k)` and `k in v` lower to the same containment node, so
    // the node carries the reach and the emitter picks the matching prelude
    // helper. Pinning the two call sites is the cheap companion of the runtime
    // tier: while one fused disjunction served both spellings, the prototype
    // fallback that `in` needs also leaked into `Object.hasOwn`.
    let source = source_for(
        r"
export function ownProbe(value: unknown, key: string): boolean {
  return Object.hasOwn(value as object, key);
}

export function chainProbe(value: unknown, key: string): boolean {
  return key in (value as object);
}
",
    );

    let own = emitted_function_body(&source, "fn own_probe");
    assert!(
        own.contains("smelt_has_own_property("),
        "`Object.hasOwn` must ask only for own properties:\n{own}"
    );
    assert!(
        !own.contains("smelt_has_property("),
        "`Object.hasOwn` must not consult the prototype chain:\n{own}"
    );

    let chain = emitted_function_body(&source, "fn chain_probe");
    assert!(
        chain.contains("smelt_has_property("),
        "`in` must walk the prototype chain:\n{chain}"
    );

    // The prototype half is defined in terms of the own half, so a synthesized
    // own property (a boxed string's `length`, a byte view's elements) is
    // visible to both without being written twice.
    assert!(
        source.contains("fn smelt_has_own_property(value: &SmeltUnknown, key: &str) -> bool {"),
        "the prelude must define the own-property authority:\n{source}"
    );
    assert!(
        source.contains("if smelt_has_own_property(value, key) { return true; }"),
        "the prototype-chain test must be layered over the own-property one:\n{source}"
    );
    assert!(
        source.contains("smelt_boxed_string_own_property(map, key)"),
        "a boxed String's synthesized properties must count as own:\n{source}"
    );
    // The unbound `Object.prototype.hasOwnProperty` value routes through the
    // same authority rather than carrying its own narrower check.
    assert!(
        source.contains("SmeltUnknown::Bool(smelt_has_own_property(&receiver, &key))"),
        "the unbound own-key probe must share the own-property authority:\n{source}"
    );
}

#[test]
fn a_regex_is_compiled_with_a_raised_size_budget() {
    // JavaScript imposes no size limit on a pattern; the `regex` crate defaults
    // to a 10 MiB compiled program because it expects untrusted input. A bounded
    // repetition with a large upper bound (`X{0,4096}`) expands past that
    // default, and the engine reports the overflow as if the pattern were
    // malformed -- so the budget is raised, once, at the single compile site.
    let source = source_for(
        r"
const PATTERN = /a{0,4096}b/;

export function matches(text: string): boolean {
  return PATTERN.test(text);
}
",
    );

    assert!(
        source.contains("const SMELT_REGEX_SIZE_LIMIT: usize = 64 * 1024 * 1024;"),
        "the prelude must state the compiled-program budget:\n{source}"
    );
    assert!(
        source.contains(
            "fancy_regex::RegexBuilder::new(&pattern).delegate_size_limit(SMELT_REGEX_SIZE_LIMIT)"
        ),
        "the compile site must apply the raised budget:\n{source}"
    );
}

#[test]
fn a_field_read_on_a_materialized_receiver_goes_through_the_same_helper() {
    // The other erased-read emitter: `field_access_text` serves an optional
    // receiver, a `?.` chain and a union-typed local, and it used to inline an
    // OBJECT-ONLY `match` of its own. That made the two spellings of one read
    // disagree the moment the receiver was not a record — an erased ARRAY
    // (which a regex match result is, carrying `index`, `input` and `groups` as
    // named properties in the identity-keyed side table) answered a constant
    // `null` here while the place-expression path answered correctly.
    let source = source_for(
        r"
export function matchGroups(text: string): unknown {
  const found: RegExpExecArray | null = /(?<letter>[a-z])/u.exec(text);
  return found?.groups;
}
",
    );

    let body = emitted_function_body(&source, "fn match_groups");
    assert!(
        body.contains("smelt_get_unknown_field("),
        "a field read on a materialized erased receiver must use the shared helper:\n{body}"
    );
    assert!(
        !body.contains("SmeltUnknown::Object(map) => match map.get(\"groups\")"),
        "the object-only inline read must be gone, or an array receiver answers null:\n{body}"
    );
}

#[test]
fn a_class_field_typed_as_its_own_class_stays_a_field() {
    // A field whose type mentions the class it is declared in — directly, under
    // an array, or under `?` — is an ORDINARY field. A hand-writing Rust team
    // would hold `Vec<Tree>` / `Option<Tree>` through whatever handle the class
    // representation already uses rather than drop the member, so the emitted
    // storage must carry every one of them: dropping them left the constructor
    // and every `this.parent = …` store assigning to a field that did not exist.
    let source = source_for(
        r"
class Tree {
  value: number;
  children: Tree[] = [];
  parent?: Tree;
  constructor(value: number) {
    this.value = value;
  }
}

export function selfReferential(): number {
  const node = new Tree(1);
  node.parent = node;
  node.children = [node, node];
  return node.children.length;
}
",
    );

    // A recursive class is emitted through the shared-handle representation, so
    // the fields live on its `…Inner` storage struct rather than on the newtype
    // handle; the braced declaration is the one that carries them either way.
    let storage = source
        .split("struct Tree")
        .find(|part| part.starts_with(" {") || part.starts_with("Inner {"))
        .and_then(|part| part.split('}').next())
        .unwrap_or_else(|| panic!("no braced `Tree` storage struct was emitted:\n{source}"));
    for field in ["value", "children", "parent"] {
        assert!(
            storage.contains(field),
            "the `Tree` storage must keep its `{field}` field:\n{storage}"
        );
    }
}
