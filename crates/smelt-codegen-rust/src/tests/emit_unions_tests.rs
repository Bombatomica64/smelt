//! Generated Rust for generated unions and narrowing.

use super::*;

#[test]
fn emits_concrete_union_enum_and_projects_typeof_narrowed_local() {
    let source = source_for(
        r#"
function resolvePath(path: string | (() => string)): string {
  if (typeof path === "string" && path.includes(".")) {
    return path;
  }
  if (typeof path === "string") {
    return path + ".ts";
  }
  return path();
}
"#,
    );

    assert!(source.contains("pub enum SmeltUnion"), "{source}");
    assert!(source.contains("M0(") && source.contains("M1("), "{source}");
    assert!(
        source.contains("matches!(path.clone(), SmeltUnion"),
        "{source}"
    );
    assert!(
        source.contains("union guard selected an excluded member"),
        "{source}"
    );
}

#[test]
fn narrows_concrete_union_locals_with_in_array_and_instanceof_guards() {
    let source = source_for(
        r#"
interface Named { name: string; }
interface LengthBearing { length: number; }
function lengthOf(value: Named | LengthBearing): number {
  if ("length" in value) return value.length;
  return 0;
}

function values(source: number[] | Record<string, number>): number[] {
  return Array.isArray(source) ? source : Object.values(source);
}

class Left { left: string = "left"; }
class Right { right: string = "right"; }
function read(value: Left | Right): string {
  if (value instanceof Left) return value.left;
  return "right";
}
"#,
    );

    assert!(source.matches("pub enum SmeltUnion").count() >= 3, "{source}");
    assert!(
        source.contains("union guard selected an excluded member"),
        "{source}"
    );
    assert!(source.contains("matches!(value.clone(), SmeltUnion"), "{source}");
}

/// A class field typed as a concrete union (`string | number`) lowers the field
/// to a tagged `SmeltUnion*` enum. The class struct derives `Clone, Debug,
/// Default`, so the union enum must supply `Debug` and `Default` as well — a
/// data-carrying enum can derive neither. The enum keeps `#[derive(Clone)]` and
/// gains a hand-written `Debug` (reusing the erased `SmeltUnknown` view, exactly
/// like its `PartialEq`) and a hand-written `Default` selecting the first arm.
/// Without these impls the struct's `Debug`/`Default` derives fail to compile.
#[test]
fn emits_debug_and_default_for_union_typed_class_field() {
    let source = source_for(
        r"
class Holder {
  value: string | number = 0;
}
export function getVal(h: Holder): string | number {
  return h.value;
}
",
    );

    // The class struct keeps deriving the standard traits (plus `PartialEq`,
    // which the union field's hand-written `PartialEq` satisfies).
    assert!(
        source.contains("#[derive(Clone, Debug, Default, PartialEq)]"),
        "{source}"
    );
    // The union enum carries hand-written Debug and Default impls.
    assert!(source.contains("pub enum SmeltUnion"), "{source}");
    assert!(
        source.contains("impl ::std::fmt::Debug for SmeltUnion"),
        "{source}"
    );
    assert!(
        source.contains("impl Default for SmeltUnion"),
        "{source}"
    );
    // Default selects the first union arm.
    assert!(source.contains("Self::M0("), "{source}");
}

#[test]
fn emits_union_equality_against_concrete_values() {
    let source = source_for(
        r#"
function isEmpty(value: string | number): boolean {
  return value === "";
}
"#,
    );

    assert!(
        source.contains("SmeltUnknown::String(\"\".into())"),
        "{source}"
    );
}

#[test]
fn emits_list_literal_for_union_destination_as_unknown_array() {
    let source = source_for(
        r#"
function empty(flag: boolean): unknown[] | unknown {
  return flag ? [] : "none";
}
"#,
    );

    assert!(
        source.contains("SmeltUnknown::Array(vec![].into())"),
        "{source}"
    );
}

#[test]
fn emits_union_element_push_as_concrete_union_injections() {
    // Mixed-literal pushes into an `Array<number | string>` inject each argument
    // into the concrete union enum instead of routing through SmeltUnknown.
    let source = source_for(
        r#"
export function mixed(): Array<number | string> {
  const xs: Array<number | string> = [];
  xs.push(1);
  xs.push("a");
  return xs;
}
"#,
    );

    assert!(source.contains("pub enum SmeltUnion"), "{source}");
    assert!(
        source.contains("let smelt_push_item = SmeltUnion") && source.contains("::M0(1.0)"),
        "numeric push should inject the concrete union member: {source}"
    );
    assert!(
        source.contains("::M1(JsString::from(\"a\"))"),
        "string push should inject the concrete union member: {source}"
    );
}

#[test]
fn emits_union_receiver_concat_by_extracting_to_unknown_list() {
    // `concat` on an erased/union array-like receiver extracts the receiver to a
    // concrete `SmeltList<SmeltUnknown>` and chains the appended values — a
    // genuine dynamic boundary, not a silent widening of statically-typed data.
    let source = source_for(
        r#"
export function widen(a: number[] | string[]): unknown[] {
  return (a as unknown[]).concat(1, "x");
}
"#,
    );

    assert!(
        source.contains("SmeltList<SmeltUnknown>"),
        "erased concat receiver should extract to an unknown list: {source}"
    );
    assert!(
        source.contains(".iter().cloned().chain("),
        "concat should chain the appended values onto the receiver: {source}"
    );
    assert!(
        source.contains("SmeltUnknown::Number(1.0")
            && source.contains("SmeltUnknown::String(\"x\".into())"),
        "appended scalars should be boxed into the unknown element type: {source}"
    );
}

#[test]
fn narrows_a_typeof_only_local_to_a_static_str() {
    let source = source_for(
        r"
export function sameKind(a: unknown, b: unknown): boolean {
  if (typeof a !== typeof b) {
    return false;
  }
  switch (typeof a) {
    case 'string':
    case 'boolean':
      return true;
    default:
      return false;
  }
}
",
    );

    // A local defined only by a `typeof`, and read only by an equality and a
    // string `switch`, carries the `&'static str` the helper returns -- no
    // owned copy, and no `.as_str()` on a value that is already a `&str`.
    assert!(
        source.contains("let _smelt_tmp_2: &'static str = smelt_typeof(&a);"),
        "{source}"
    );
    assert!(
        source.contains("let _smelt_tmp_3: &'static str = smelt_typeof(&b);"),
        "{source}"
    );
    assert!(
        source.contains("let _smelt_tmp_5: &'static str;")
            && source.contains("_smelt_tmp_5 = smelt_typeof(&a);"),
        "{source}"
    );
    assert!(!source.contains("smelt_typeof(&a).to_owned()") && !source.contains("JsString::from(smelt_typeof(&a))"), "{source}");
    // The equality compares the two borrowed spellings directly, and the
    // `switch` matches the binding rather than an `.as_str()` it does not have.
    assert!(
        source.contains("_smelt_tmp_2 != _smelt_tmp_3") && source.contains("match _smelt_tmp_5 {"),
        "{source}"
    );
}

#[test]
fn emits_union_typeof_as_borrowed_static_str_match() {
    let source = source_for(
        r"
function describe(value: string | number[]): string {
  return typeof value;
}
",
    );

    // A concrete union decides its arms per member type, so the match stays at
    // the site -- but it selects a `&'static str` and borrows its scrutinee.
    assert!(!source.contains("\"string\".to_owned(), ") && !source.contains("JsString::from(\"string\"), "), "{source}");
    assert!(
        source.contains("=> match smelt_builtin_receiver_elements(&receiver) { Some(values) => { let len = values.len(); let start = smelt_builtin_relative_index(&first, len, 0); let end = smelt_builtin_relative_index(&second, len, len); SmeltUnknown::Array(values.into_iter().skip(start).take(end.saturating_sub(start)).collect::<Vec<_>>().into()) } Non")
            && source.contains("::M0(_) => \"string\", ")
            && source.contains("::M1(_) => \"object\" });"),
        "{source}"
    );
}

#[test]
fn a_map_union_arm_is_an_object_on_every_runtime_kind_path() {
    // Regression (haiku audit F0310): four sites mapped a concrete type to its
    // JavaScript runtime category and the copies had drifted, three of them
    // leaving `Map` out of "object". All now read `Type::runtime_kind`.
    let source = source_for(
        r"
export function isObj(v: Map<string, number> | string): boolean {
  return typeof v === 'object';
}
export function kindName(v: Map<string, number> | string): string {
  return typeof v;
}
export function fromErased(u: unknown): Map<string, number> | number[] | boolean {
  return u as Map<string, number> | number[] | boolean;
}
",
    );

    // The frontend no longer folds `typeof v === 'object'` to `false`, and the
    // concrete-union arm filter keeps the Map arm.
    let is_obj = source
        .split("fn is_obj(")
        .nth(1)
        .and_then(|rest| rest.split("\n}").next())
        .unwrap_or_default();
    assert!(
        is_obj.contains("matches!(v.clone(), SmeltUnion") && !is_obj.contains("false"),
        "{source}"
    );
    // The union `typeof` string names the Map arm "object", not "undefined".
    assert!(
        source.contains("(_) => \"object\" }") && !source.contains("(_) => \"undefined\""),
        "{source}"
    );
    // No member of the reconstructed union is nullish, so a `Null | Undefined`
    // routing pattern could only be the Map arm misfiled.
    assert!(
        source.contains("fn from_smelt_unknown")
            && !source.contains(
                "matches!(value, SmeltUnknown::Null | SmeltUnknown::Undefined) { return Self::"
            ),
        "{source}"
    );
}

#[test]
fn reads_narrowed_unknown_object_length_property() {
    let source = source_for(
        r#"
function lengthOf(value: unknown): number {
  if (typeof value === "object" && value !== null && "length" in value && typeof value.length === "number") {
    return value.length;
  }
  return -1;
}
"#,
    );

    assert!(
        source.contains("smelt_get_object_field(&smelt_map, \"length\")"),
        "{source}"
    );
    assert!(
        !source.contains("SmeltUnknown::Object(value) => value.len()"),
        "{source}"
    );
}

#[test]
fn structural_in_guard_projects_to_concrete_union_discriminant() {
    // Issue #55: a `"field" in value` guard over a concrete class union must
    // lower to a tagged-enum discriminant check, not an erased runtime object
    // lookup. Inside the true branch the value projects into the matching arm.
    let source = source_for(
        r#"
class Circle { radius: number = 1; }
class Square { side: number = 2; }
function describe(shape: Circle | Square): number {
  if ("radius" in shape) {
    return shape.radius;
  }
  return 0;
}
"#,
    );

    assert!(source.contains("pub enum SmeltUnion"), "{source}");
    // The `in` check is a concrete discriminant test, never a SmeltUnknown map.
    assert!(
        source.contains("matches!(shape.clone(), SmeltUnion"),
        "structural `in` should emit a concrete tag check: {source}"
    );
    assert!(
        !source.contains("SmeltUnknown::Object(values) => values.contains_key"),
        "structural `in` on a concrete union must not erase to an object lookup: {source}"
    );
    // The narrowed read projects into the concrete arm.
    assert!(
        source.contains("union guard selected an excluded member"),
        "{source}"
    );
}

#[test]
fn reassigning_narrowed_union_local_stays_within_narrowed_arm() {
    // Issue #55 invalidation rule: writing a value that still inhabits the
    // narrowed arm refines the narrowing rather than dropping it, so the write
    // re-injects the concrete union variant and later reads still project it.
    let source = source_for(
        r#"
function resolve(path: string | (() => string)): string {
  if (typeof path === "string") {
    path = path + "x";
    return path;
  }
  return path();
}
"#,
    );

    assert!(source.contains("pub enum SmeltUnion"), "{source}");
    // The assignment re-injects the concrete arm, proving the fact survived the
    // widening-compatible write.
    assert!(
        source.contains("path = SmeltUnion"),
        "assignment of a narrowed-compatible value should re-inject the arm: {source}"
    );
    assert!(
        source.contains("union guard selected an excluded member"),
        "{source}"
    );
}

#[test]
fn erases_concrete_union_operand_before_truthiness_extraction() {
    // A destructuring default over a `boolean | number` field lowers `flag` to a
    // concrete `SmeltUnion…` enum. Using it in a boolean position must first
    // project the tagged enum back to its erased value (`into_smelt_unknown()`)
    // because the truthiness `match` operates over `SmeltUnknown` discriminants.
    // Regression for the concrete-union boundary: without the erase the emitted
    // `match flag.clone() { SmeltUnknown::… }` would not type-check against the
    // `SmeltUnion…` storage.
    let source = source_for(
        r"
function pick(opts: { flag?: boolean | number }): boolean {
  const { flag = false } = opts;
  return !flag;
}
",
    );

    assert!(source.contains("pub enum SmeltUnion"), "{source}");
    assert!(
        source.contains("let flag: SmeltUnion"),
        "flag should keep its concrete-union storage: {source}"
    );
    assert!(
        source.contains("match flag.clone().into_smelt_unknown()"),
        "a concrete-union operand must be erased before the truthiness match: {source}"
    );
}

#[test]
fn injects_concrete_union_at_nullish_default_sink() {
    // An erased object-field read defaulted with `??` flows into a concrete
    // `boolean | number` sink. The coalesced `SmeltUnknown` value must be
    // reconstructed into the tagged union (`SmeltUnion…::from_smelt_unknown`)
    // rather than left erased. Regression for the concrete-union boundary: the
    // sink stores `SmeltUnion…`, so leaving the value as `SmeltUnknown` fails to
    // type-check.
    let source = source_for(
        r#"
function opt(record: Record<string, unknown>): boolean | number {
  const value: boolean | number = (record["k"] as boolean | number) ?? false;
  return value;
}
"#,
    );

    assert!(source.contains("pub enum SmeltUnion"), "{source}");
    assert!(
        source.contains("::from_smelt_unknown("),
        "an erased value flowing into a concrete-union sink must be reconstructed: {source}"
    );
}

#[test]
fn indexes_concrete_union_list_with_union_missing_default() {
    // Out-of-bounds element access on a `SmeltList<SmeltUnion…>` must default to
    // a union value, not the erased `SmeltUnknown::Undefined`. The list element
    // is a tagged `SmeltUnion…`, so `.get(idx).cloned().unwrap_or(default)`
    // requires the default to be an actual union value (E0308 otherwise).
    let source = source_for(
        r"
export function pick(keys: Array<string | number>, i: number): string | number {
  return keys[i];
}
",
    );

    assert!(source.contains("pub enum SmeltUnion"), "{source}");
    assert!(
        source.contains(".cloned().unwrap_or_else(|| SmeltUnion"),
        "missing element default must be a concrete union value: {source}"
    );
    assert!(
        !source.contains(".cloned().unwrap_or(SmeltUnknown::Undefined)"),
        "concrete-union element default must not be the erased undefined: {source}"
    );
}

#[test]
fn recovers_concrete_union_tuple_arms_by_arity() {
    // Two tuple arms of different length carry the SAME runtime tag
    // (`SmeltUnknown::Array`), so recovery on the tag alone always chose the
    // first arm: `['y', 2, true]` came back as the 2-tuple with its third
    // element silently dropped, and `.length` answered 2 where Node answers 3.
    //
    // A tuple's arity is part of its type, so it is the discriminant. This is
    // the tuple counterpart of the class arm's field-presence guard.
    let source = source_for(
        r"
type Pair = [string, number];
type Triple = [string, number, boolean];
export function put(table: Record<string, Pair | Triple>): void {
  table['b'] = ['y', 2, true];
}
",
    );

    assert!(source.contains("pub enum SmeltUnion"), "{source}");
    // The enabling condition: both arms really do share the `Array` runtime tag,
    // so the tag test alone must NOT be the whole guard. That tag-only spelling
    // is exactly what chose the wrong arm, so asserting its absence is what
    // makes this test non-vacuous.
    assert!(
        !source.contains("if matches!(value, SmeltUnknown::Array(_)) { return"),
        "a tag-only guard cannot separate two Array-tagged tuple arms: {source}"
    );
    assert!(
        source.contains("smelt_arms.len() == 2"),
        "the 2-tuple arm must be guarded by its arity: {source}"
    );
}

#[test]
fn erasing_a_concrete_union_for_inspection_does_not_move_it() {
    // `into_smelt_unknown` takes `self` by value and `SmeltUnion…` is not
    // `Copy`, so erasing a concrete union in order to INSPECT it moved out of
    // the source place. Reading the same local twice — once indexed, once for
    // `.length` — then emitted `first.into_smelt_unknown()` at both sites and
    // the second was a use-after-move (E0382).
    let source = source_for(
        r"
type Pair = [string, number];
type Triple = [string, number, boolean];
export function read(table: Record<string, Pair | Triple>): string {
  const first = table['a'];
  return first[0] + ',' + first.length;
}
",
    );

    assert!(source.contains("pub enum SmeltUnion"), "{source}");
    // The enabling condition is that the local is INSPECTED at two sites, which
    // is what made a move observable — one use could never fail to compile.
    // Both reads used to erase it (`into_smelt_unknown`); since element and
    // `.length` reads project the union's arms
    // (`concrete_union_index_reads`, `concrete_union_length_read_expr`), both
    // inspect it through a borrow (`match &first`) and nothing erases it. The
    // guarantee is unchanged and now holds by construction: neither read
    // consumes `first`.
    assert!(
        source.matches("match &first {").count() >= 2,
        "the union local must be inspected at both sites, by reference: {source}"
    );
    assert!(
        !source.contains("first.into_smelt_unknown()"),
        "inspecting a union place must not move it: {source}"
    );
}

/// Issue #113: a named `reduce` callback whose declared return type is not
/// identical to the initial value's type but statically reconciles with it must
/// not be rejected with "array reduce callback returns an unsupported type". The
/// accumulator widens to the reconciled type (`string | number`), the initial
/// `0` is coerced into that concrete union, and the emitted `fold` invokes the
/// callback with only the two arguments it declares (not the four the runtime
/// supplies) so the closure call type-checks.
#[test]
fn reduce_named_callback_reconciles_union_return_type() {
    let source = source_for(
        r"
function step(acc: string | number, value: number): string | number {
  return acc;
}
export function run(values: number[]): string | number {
  return values.reduce(step, 0);
}
",
    );
    assert!(source.contains(".iter().enumerate().fold("), "{source}");
    // The named callback declares two parameters, so the emitted fold calls it
    // with exactly two arguments rather than the runtime's `(acc, item, index,
    // array)` four-argument shape.
    assert!(
        source.contains("(smelt_callback)(smelt_acc, smelt_item)"),
        "reduce should call the two-parameter callback with two args: {source}"
    );
    assert!(source.contains("fn run("), "{source}");
}

#[test]
fn emits_string_coercion_default_sort_for_union_elements() {
    // A comparator-less `sort()` on a union-element list follows JavaScript's
    // default ordering: elements compare by their `ToString` coercion. The
    // concrete union projects through `into_smelt_unknown` before the coercion
    // match, and the sort itself is a stable `sort_by` over the coerced keys.
    let source = source_for(
        r"
function sortMixed(values: Array<string | number>): Array<string | number> {
  return values.sort();
}
",
    );

    assert!(
        source.contains(".sort_by(|smelt_left, smelt_right| (match smelt_left.clone().into_smelt_unknown()"),
        "union default sort should compare erased string coercions\n{source}"
    );
    assert!(
        source.contains("SmeltUnknown::Number(value) => smelt_number_to_string(value)"),
        "the coercion match should stringify numeric elements\n{source}"
    );
}

#[test]
fn branch_join_narrows_optional_reassigned_in_both_arms() {
    // Regression (es-toolkit includes): when both arms of an if/else reassign an
    // optional local to a non-null value, the local is non-null after the join
    // and should narrow to its inner type so later arithmetic/indexing type-checks
    // against the concrete type instead of the declared `Optional<T>`.
    let source = source_for(
        r"
export function clampFrom(guard: boolean, fromIndex?: number): number {
  if (guard || !fromIndex) {
    fromIndex = 0;
  } else {
    fromIndex = fromIndex + 1;
  }
  return fromIndex + 1;
}
",
    );
    // The post-join read of `fromIndex` must be NARROWED to `f64`. Narrowing an
    // optional local is rendered as the checked read
    // `x.clone().expect("optional value was absent after narrowing")` (the same
    // read the else arm's `fromIndex + 1` gets from its truthiness guard); the
    // un-narrowed read defaults the absent case instead
    // (`from_index.clone().unwrap_or(0.0)`).
    //
    // Corrected requirement. This test used to assert the ABSENCE of
    // `from_index.clone().expect`, but that only held because the narrowed read
    // was emitted with a redundant second clone (`.clone().clone().expect`):
    // with the join narrowing disabled the old assertion still passed, so it
    // never detected the defect it names. The guarantee from the change that
    // added it (86856594: the read type-checks as the inner type instead of
    // `Option<f64>`, clearing an E0308) is what is asserted now: both reads are
    // narrowed and none defaults. Disabling the join narrowing in
    // `types_iface.rs` turns the post-join read into `unwrap_or(0.0)` and fails
    // both checks.
    let narrowed_reads = source
        .matches("from_index.clone().expect(\"optional value was absent after narrowing\")")
        .count();
    assert!(
        narrowed_reads == 2 && !source.contains("from_index.clone().unwrap_or("),
        "branch-join narrowing should read fromIndex as a narrowed f64 after the join: {source}"
    );
}

/// An object literal passed where a `number | Options` union is expected must
/// be injected into the record-shaped union arm. Previously only collection
/// shapes were shape-matched, so a `Dict`/record source against a class arm
/// found no member and the raw record was passed (E0308). Mirrors es-toolkit's
/// `retry(fn, { retries })`.
#[test]
fn object_literal_injects_into_record_union_arm() {
    let source = source_for(
        r"
interface Options {
  retries?: number;
}

function retry(fn: () => number, options: number | Options): number {
  return fn();
}

export function run(): number {
  return retry(() => 1, { retries: 3 });
}
",
    );

    assert!(
        source.contains("::M1("),
        "an object literal must be injected into the record-shaped union arm\n{source}"
    );
}

#[test]
fn array_containment_projects_an_optional_union_receiver() {
    // `chars?: string | string[]` interns as `Optional(Union([String, List(String)]))`.
    // The source narrows it with `switch (typeof chars) { case 'object': … }` and the
    // frontend only emits `Rvalue::ListContains` after that narrowing holds, but MIR
    // reads the value through its DECLARING local, so the operand type at emission
    // is still the wide one. `list_contains_expr` matched `Type::List` alone and
    // answered a constant `false` for anything else, so es-toolkit's
    // `trim`/`trimStart`/`trimEnd` array-`chars` loops never removed a character —
    // ten specs, silently wrong rather than failing.
    let generated = source_for(
        r"
export function trimIt(str: string, chars?: string | string[]): string {
  if (chars === undefined) {
    return str;
  }
  let startIndex = 0;
  switch (typeof chars) {
    case 'string': {
      while (startIndex < str.length && str[startIndex] === chars) {
        startIndex++;
      }
      break;
    }
    case 'object': {
      while (startIndex < str.length && chars.includes(str[startIndex])) {
        startIndex++;
      }
    }
  }
  return str.substring(startIndex);
}
",
    );
    assert!(
        generated.contains("union guard selected an excluded member"),
        "the containment receiver must project to its list arm:\n{generated}"
    );
    let program = generated
        .split_once("@smelt:prelude-end")
        .map_or(generated.as_str(), |(_, program)| program);
    assert!(
        program.contains(".contains(&str.char_at("),
        "containment must compare against the projected `Vec`:\n{generated}"
    );
}

/// A method call on a union receiver dispatches by matching the tagged enum,
/// with no dynamic erasure.
///
/// The emitter has always been able to do this (`union_method_expr`); what was
/// missing was the Python frontend emitting an ordinary `Method` expression for
/// a union-typed receiver instead of rejecting it. The arms are concrete
/// classes, so the call must stay concrete — routing it through `SmeltUnknown`
/// would be exactly the "reconcile concrete union arms" erasure the project
/// forbids.
#[test]
fn python_union_receiver_method_dispatches_statically() {
    let source = source_for_py(
        r"
class Ok:
    def is_ok(self) -> bool:
        return True

class Err:
    def is_ok(self) -> bool:
        return False

def check(r: Ok | Err) -> bool:
    return r.is_ok()
",
    );

    // Cut at the end of `check` itself: the tail of the file holds the class
    // `impl` blocks, whose erased-prototype adapters legitimately mention
    // `SmeltUnknown`, and the assertion below is about this body only.
    let check_body = source
        .split("fn check(")
        .nth(1)
        .and_then(|tail| tail.split_once("\n}\n").map(|(body, _)| body))
        .unwrap_or_else(|| panic!("expected a generated `check`:\n{source}"));
    assert!(
        check_body.contains("M0(value) => value.is_ok()")
            && check_body.contains("M1(value) => value.is_ok()"),
        "each union arm must call its own concrete method:\n{source}"
    );
    assert!(
        !check_body.contains("SmeltUnknown"),
        "a concrete union receiver must not erase to a dynamic value:\n{source}"
    );
}

/// A TypeScript method call on a union receiver dispatches by matching the
/// tagged enum instead of erasing the receiver.
///
/// `resolve_method` has always computed the per-arm result type for a union and
/// documented that "MIR retains the typed call and dispatches over the concrete
/// tagged-union arms". What defeated it was the callable-field path running
/// first: its concreteness guard demanded a single method item, which a union
/// deliberately never has, so the call was lowered as a dynamic field read and
/// invoked through the `SmeltUnknown` call ABI.
#[test]
fn typescript_union_receiver_method_dispatches_statically() {
    let source = source_for(
        r"
class Ok { isOk(): boolean { return true; } }
class Err { isOk(): boolean { return false; } }
function check(r: Ok | Err): boolean { return r.isOk(); }
const v = check(new Ok());
",
    );

    let check_body = source
        .split("fn check(")
        .nth(1)
        .and_then(|rest| rest.split("\nfn ").next())
        .unwrap_or_else(|| panic!("expected a generated `check`:\n{source}"));
    assert!(
        check_body.contains("M0(value) => value.is_ok()")
            && check_body.contains("M1(value) => value.is_ok()"),
        "each union arm must call its own concrete method:\n{source}"
    );
    assert!(
        !check_body.contains("SmeltUnknown"),
        "a concrete union receiver must not erase to a dynamic value:\n{source}"
    );
    // The dynamic call ABI is fallible, so erasing also infected the signature
    // with a `Result`. Static dispatch cannot throw.
    assert!(
        check_body.starts_with("r: SmeltUnion") && check_body.contains(") -> bool {"),
        "static dispatch must not wrap the return type in `Result`:\n{source}"
    );
}

/// A conditional whose branches are two *declared* classes unifies to the
/// generated tagged union, not to `String`.
///
/// `Type::Class` spells both a declared class and an unresolved opaque name, so
/// `is_string_compatible_type` accepts any class — a JS value can always be
/// coerced to a string. Applying that to *unification* made
/// `flag ? new A() : new B()` come out as `String`, and the emitter then
/// declared a `String` local and assigned the class values into it: output that
/// does not compile. Two declared classes do have a concrete common
/// representation, so they unify to their union.
#[test]
fn conditional_over_declared_classes_unifies_to_a_union() {
    let source = source_for(
        r"
class A { v(): number { return 1; } }
class B { v(): number { return 2; } }
function pick(flag: boolean): A | B { return flag ? new A() : new B(); }
function use1(x: A | B): number { return x.v(); }
const r = use1(pick(true));
",
    );

    let pick_body = source
        .split("fn pick(")
        .nth(1)
        .and_then(|rest| rest.split("\nfn ").next())
        .unwrap_or_else(|| panic!("expected a generated `pick`:\n{source}"));
    assert!(
        !pick_body.contains(": String") && !pick_body.contains(": JsString"),
        "two declared classes must not unify to `String`:\n{source}"
    );
    assert!(
        pick_body.contains("::M0(") && pick_body.contains("::M1("),
        "each branch must be wrapped into its union arm:\n{source}"
    );
}

/// A conditional with ONE declared-class branch unifies to the union too.
///
/// The sibling test above required BOTH branches to be declared classes, and
/// that is what the rule required as well — so `flag ? new Doc() : "text"` at a
/// `string | Doc` type still fell into the string-compatible test, still
/// unified to `String`, and still emitted `let mut _smelt_tmp: String;` with a
/// `Doc` assigned into it. That is the commoner shape of the two: a union of a
/// class and a primitive is how a "value or message" result is usually spelled.
///
/// Every closer unification is tried before this arm is reached, so a declared
/// class here means the two branches are genuinely two union members.
#[test]
fn conditional_over_a_class_and_a_string_unifies_to_a_union() {
    let source = source_for(
        r#"
class Doc { name(): string { return "d"; } }
function pick(flag: boolean): string | Doc { return flag ? new Doc() : "text"; }
function show(value: string | Doc): string {
  return typeof value === "string" ? value : value.name();
}
const r = show(pick(true));
"#,
    );

    let pick_body = source
        .split("fn pick(")
        .nth(1)
        .and_then(|rest| rest.split("\nfn ").next())
        .unwrap_or_else(|| panic!("expected a generated `pick`:\n{source}"));
    assert!(
        !pick_body.contains("let mut _smelt_tmp_1: String;") && !pick_body.contains("let mut _smelt_tmp_1: JsString;"),
        "a class branch must not be assigned into a `String` local:\n{source}"
    );
    assert!(
        pick_body.contains("::M0(") && pick_body.contains("::M1("),
        "each branch must be wrapped into its own union arm:\n{source}"
    );
}

/// `instanceof` narrows an OPTIONAL union by matching the arm under `Some`.
///
/// `form.get(name)` is `string | File | null`, so `entry instanceof File` asks
/// about an `Option<SmeltUnion…>`. The marker probe claimed it — every
/// `Optional` operand counted as dynamic — and emitted a `SmeltUnknown::Object`
/// pattern against a value of that type, which does not compile. The union arm
/// answers it now, one optional deep: an absent value is simply not one of the
/// accepted patterns, which is also the right answer, since `null instanceof
/// File` is `false` in JavaScript.
#[test]
fn instanceof_narrows_an_optional_union_through_its_arm() {
    let source = source_for(
        r#"
const form = new FormData();
form.append("doc", new File(["hi"], "note.txt"));
const entry = form.get("doc");
if (entry instanceof File) {
  console.log(entry.name);
} else {
  console.log("not a file");
}
"#,
    );

    assert!(
        !source.contains("Some(SmeltUnknown::Object(value))"),
        "an optional union must not be probed as an erased record:\n{source}"
    );
    assert!(
        source.contains("matches!(entry.clone(), Some(SmeltUnion"),
        "the check must match the union arm under `Some`:\n{source}"
    );
}

/// A generated union emits `SmeltJsKeyEq`, so it can be used as a map key.
///
/// Map keys are compared through the erased JavaScript key-equality projection.
/// Without the impl, any generated map keyed by a union fails with "the trait
/// bound `SmeltUnionN: SmeltJsKeyEq` is not satisfied" — which is what
/// es-toolkit's `keyBy` specs hit.
#[test]
fn generated_unions_emit_js_key_equality() {
    let source = source_for(
        r"
function pick(flag: boolean): number | string { return flag ? 1 : 'a'; }
const m = new Map<number | string, number>();
m.set(pick(true), 1);
",
    );

    assert!(
        source.contains("SmeltJsKeyEq for SmeltUnion"),
        "a generated union must support JavaScript key equality:\n{source}"
    );
    assert!(
        source.contains("SmeltFromUnknown for SmeltUnion"),
        "a generated union must be recoverable from its erased view:\n{source}"
    );
}


/// Narrowing a callable object to a callable interface that declares fewer
/// members keeps the dropped members readable as own properties of the
/// underlying function value, the way JavaScript does.
#[test]
fn narrowed_callable_object_keeps_its_own_properties() {
    let source = source_for(&format!(
        "{CALLABLE_OVERLOAD_PRELUDE}
const curried = curry2((a: number, b: number) => a + b);
// @ts-ignore
const marker = curried.placeholder;
"
    ));

    assert!(
        source.contains("smelt_with_properties(vec![(\"placeholder\".to_owned()"),
        "the dropped property must be carried onto the erased callable:\n{source}"
    );
    assert!(
        source.contains(".__smelt_call.smelt_property(\"placeholder\")"),
        "the undeclared member read must resolve through the callable's properties:\n{source}"
    );
}

/// H14: a `place_ty` fallback must degrade AT THE SITE, never by interning
/// `Type::Unknown`.
///
/// The `_ =>` arms of `place_ty` answer "this read has no static type", which
/// the emitter spells as the erased carrier. Asking for it through `type_id`
/// made that answer depend on something unrelated to the read — a crate with no
/// erased value has no `Type::Unknown` entry at all — so the blocker read
/// `type table does not contain literal operand type Unknown at
/// emitter/types.rs:<line>`, naming a line of the COMPILER rather than the read
/// in the user's program. The obvious repair is rejected on purpose: interning
/// `Unknown` there would flip `Need::Unknown` for the whole crate
/// and emit the entire erased prelude for a program that has no erased value.
///
/// This is a SHAPE GUARD, not a reproduction: the shape recorded for H14 (a
/// `200 | 404` literal union reaching a generic interface and a generic
/// function) transpiles cleanly on this head, and four variations of it were
/// tried without reaching a fallback. It is pinned so a future change that
/// routes this shape into an unresolved field read gets a blocker naming the
/// read, and so the "just intern `Unknown`" repair cannot land silently.
#[test]
fn a_literal_union_through_a_generic_interface_still_emits() {
    let source = source_for(
        r#"
type Code = 200 | 404;

interface Envelope<T> {
  code: Code;
  body: T;
}

function report<T>(row: Envelope<T>): string {
  const code: Code = row.code;
  return `${code}`;
}

const table: Envelope<string>[] = [
  { code: 200, body: "ok" },
  { code: 404, body: "missing" },
];
console.log(report(table[0]), report(table[1]));
"#,
    );

    assert!(source.contains("fn report"), "{source}");
    // The blocker this item is about names a compiler line rather than the
    // program. It must never appear again, for any shape.
    assert!(
        !source.contains("type table does not contain literal operand type"),
        "{source}"
    );
}

/// A `??` whose fallback is a NARROWER callback keeps the declared callback
/// type instead of interning a two-arm union.
///
/// TypeScript reduces `NonNullable<A> | B` to `A` when `B` is assignable to it,
/// and a callback that ignores trailing arguments is: `() => boolean` inhabits
/// `(attempt: number) => boolean`. Interning the union instead produced a
/// generated union whose call site knew only the declared arm, so selecting the
/// fallback panicked at runtime with "union guard selected an excluded member".
#[test]
fn a_narrower_callback_fallback_keeps_the_declared_callback_type() {
    let source = source_for(
        r"
interface Options {
  shouldRetry?: (attempt: number) => boolean;
}

const DEFAULT_SHOULD_RETRY = () => true;

export function pick(options?: Options): boolean {
  const shouldRetry = options?.shouldRetry ?? DEFAULT_SHOULD_RETRY;
  return shouldRetry(0);
}
",
    );

    assert!(
        !source.contains("union guard selected an excluded member"),
        "the fallback must be adapted, not routed through a union guard:\n{source}"
    );
}
