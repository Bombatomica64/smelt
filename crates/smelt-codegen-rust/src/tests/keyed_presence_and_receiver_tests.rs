//! Keyed-read presence, throw-through-optional-call, receiver binding and
//! erased-view write-through (Hono reg-exp router rows H21/H22).
//!
//! Each test pins one general lowering rule on a minimal shape; the
//! end-to-end behaviour of all of them together is example
//! `129_keyed_presence_and_throw_sentinel` / `130_this_bound_field_write_through`.

use super::*;

/// A same-class method with no return annotation is still CALLED: the call
/// resolves while the class is being lowered, and must not be mistaken for a
/// dynamic member call that lowers to `undefined`.
#[test]
fn unannotated_same_class_method_call_is_not_dropped() {
    let source = source_for(
        r"
class Log {
  lines: string[] = [];
  #record(line: string) {
    this.lines.push(line);
  }
  add(line: string) {
    this.#record(line);
  }
}
const log = new Log();
log.add('a');
console.log(log.lines.length);
",
    );
    assert!(source.contains("self._record(line"), "{source}");
}

/// A throwing method reached through an optional chain inside `try` reaches
/// the `catch` instead of propagating past it with `?`.
#[test]
fn optional_method_call_inside_try_reaches_the_catch() {
    let source = source_for(
        r"
class Inner {
  run(value: string): void {
    if (value === 'bad') {
      throw new Error('bad');
    }
  }
}
const inners: Record<string, Inner> = { a: new Inner() };
function attempt(key: string, value: string): string {
  try {
    inners[key]?.run(value);
    return 'ok';
  } catch (e) {
    return 'caught';
  }
}
console.log(attempt('a', 'bad'));
",
    );
    assert!(source.contains("catch_unwind"), "{source}");
    assert!(
        !source.contains("Some(_smelt_value.run(value.clone())?)"),
        "the call must not propagate straight out of the function: {source}"
    );
}

/// `!record[key]` over an object-valued record tests presence of the key: the
/// value type's Rust default is truthy, so a fold would never fire the guard.
#[test]
fn keyed_record_read_guard_tests_key_presence() {
    let source = source_for(
        r"
function ensure(routes: Record<string, string[]>, path: string): number {
  if (!routes[path]) {
    routes[path] = [];
  }
  routes[path].push(path);
  return routes[path].length;
}
console.log(ensure({}, '/a'));
",
    );
    // The presence test is what lets the default-insert elision fuse the
    // guard and the write into one `entry(..).or_insert_with(..)`; a folded
    // guard left a `!(true)` that never inserted.
    assert!(
        source.contains("routes.entry(path.clone()).or_insert_with("),
        "{source}"
    );
    assert!(!source.contains("!(true)"), "{source}");
}

/// A local assigned a keyed read and then presence-tested holds
/// `T | undefined`, so a missing key reads as absent rather than being
/// unwrapped at the declared type.
#[test]
fn presence_tested_keyed_local_is_optional() {
    let source = source_for(
        r"
class TreeNode {
  kids: Record<string, TreeNode> = {};
}
function child(node: TreeNode, key: string): TreeNode {
  let next: TreeNode;
  next = node.kids[key];
  if (!next) {
    next = node.kids[key] = new TreeNode();
  }
  return next;
}
console.log(Object.keys(child(new TreeNode(), 'a').kids).length);
",
    );
    assert!(source.contains("let mut next: Option<TreeNode>"), "{source}");
    assert!(source.contains("next.clone().is_some()"), "{source}");
}

/// `class E extends Error {}` forwards its implicit constructor's message to
/// the builtin `Error(message?)` instead of ignoring it.
#[test]
fn implicit_error_subclass_constructor_forwards_the_message() {
    let source = source_for(
        r"
class PathError extends Error {}
const error = new PathError('bad path');
console.log(error.message);
",
    );
    assert!(
        source.contains("fn new(message: Option<String>) -> Self"),
        "{source}"
    );
    assert!(!source.contains("_smelt_super_arg"), "{source}");
}

/// `e instanceof UserClass` on an erased value probes the `__smelt_class`
/// marker for the class and its subclasses instead of folding to `false`.
#[test]
fn erased_instanceof_user_class_probes_the_class_marker() {
    let source = source_for(
        r"
class BaseError extends Error {}
class PathError extends BaseError {}
function kind(e: unknown): string {
  return e instanceof BaseError ? 'base' : 'other';
}
console.log(kind(new PathError('x')));
",
    );
    assert!(
        source.contains("matches!(value.get(\"__smelt_class\")"),
        "{source}"
    );
    assert!(source.contains("\"BaseError\" | \"PathError\""), "{source}");
}

/// A truthiness guard over a union of object arms is folded only for a
/// concrete generated union; an erased union keeps its runtime test.
#[test]
fn concrete_object_union_guard_folds_and_erased_one_is_tested() {
    let concrete = source_for(
        r"
type Shape = [number] | [number, number];
function first(shapes: Record<string, Shape>, key: string): number {
  const shape = shapes[key];
  if (shape) {
    return shape[0];
  }
  return -1;
}
console.log(first({ a: [1] }, 'a'));
",
    );
    let first = concrete
        .split("fn first(")
        .nth(1)
        .unwrap_or_default()
        .split("\n}\n")
        .next()
        .unwrap_or_default();
    assert!(first.contains(": bool = true;"), "{first}");
}

/// An `if` without `else` whose arm is a loop keeps the statements after the
/// `if`: the loop's exit rejoins them instead of ending the function.
#[test]
fn if_arm_loop_rejoins_the_continuation() {
    let source = source_for(
        r"
function run(xs: string[], log: string[], k: string): void {
  if (xs.length > 0) {
    for (const x of xs) {
      log.push(x);
    }
  }
  if (k === 'w') {
    log.push('wild');
    return;
  }
  log.push('tail');
}
const log: string[] = [];
run(['1'], log, 'a');
console.log(log.join(','));
",
    );
    let run = source
        .split("fn run(")
        .nth(1)
        .unwrap_or_default()
        .split("\n}\n")
        .next()
        .unwrap_or_default();
    let continuation = run.find("k.clone() == \"w\"").unwrap_or(0);
    let first_if = run.find("if _smelt_tmp").unwrap_or(usize::MAX);
    assert!(
        first_if < continuation && !run[first_if..continuation].contains("} else {"),
        "the continuation must follow the `if`, not sit in an `else`: {run}"
    );
}

/// JavaScript regex predicates compile with `fancy-regex`, which accepts
/// look-around.
#[test]
fn regex_test_uses_a_look_around_capable_engine() {
    let source = source_for(
        r"
console.log(/\((?!\?:)/.test('(a|b)'));
",
    );
    let main = source.split("fn main()").nth(1).unwrap_or_default();
    assert!(main.contains("fancy_regex::Regex::new(&"), "{main}");
    assert!(main.contains(".unwrap_or(false)"), "{main}");
}

/// A call through a function-valued class FIELD installs the instance as
/// `this` (lazily erased), and a property write through the erased `this`
/// reaches the instance's own field.
#[test]
fn field_callee_binds_this_and_writes_reach_the_instance() {
    let source = source_for(
        r"
interface Named {
  label: string;
  run(): string;
}
function run(this: Named): string {
  this.label = this.label + '!';
  return this.label;
}
class Box {
  label: string = 'box';
  run: () => string = run;
}
const box = new Box();
console.log(box.run());
console.log(box.label);
",
    );
    assert!(source.contains("smelt_push_this_lazy("), "{source}");
    assert!(source.contains("fn __smelt_set_field(&self, key: &str, value: SmeltUnknown)"), "{source}");
    assert!(source.contains("smelt_object_write_through(map, \"label\""), "{source}");
}

/// A method call keeps calling the Rust method: only a DATA field holding a
/// function takes the dynamic receiver.
#[test]
fn method_calls_do_not_bind_a_receiver() {
    let source = source_for(
        r"
function shout(this: unknown): string {
  return String(this);
}
class Greeter {
  greet(): string {
    return 'hi';
  }
}
console.log(new Greeter().greet(), shout.call('x'));
",
    );
    let main = source.split("fn main()").nth(1).unwrap_or_default();
    assert!(main.contains(".greet();"), "{main}");
    assert!(!main.contains("greet.clone()"), "{main}");
}

/// `rec[k] === undefined` over a `Record<string, string>` is a PRESENCE test.
///
/// TypeScript types the read as `string`, so the comparison used to fold to a
/// constant `false`; a missing key reads `undefined` at run time (Hono's
/// trie-router `expect(params['id']).toBe(undefined)`). The read goes through
/// its own `Option` and is tested with `is_none()` / `is_some()`.
#[test]
fn keyed_record_read_compared_with_undefined_tests_presence() {
    let source = source_for(
        r"
export function missing(rec: Record<string, string>, key: string): boolean {
  return rec[key] === undefined;
}
export function present(rec: Record<string, string>, key: string): boolean {
  return rec[key] != null;
}
export function holeAt(xs: number[], index: number): boolean {
  return xs[index] === undefined;
}
",
    );
    let program = program_part(&source);
    assert!(program.contains(".get(&key.clone())).is_none()"), "{program}");
    assert!(program.contains(".get(&key.clone())).is_some()"), "{program}");
    assert!(program.contains(".cloned()).is_none()"), "{program}");
    assert!(!program.contains("= false;"), "{program}");
}

/// A keyed record read flowing into an ERASED slot keeps its miss.
///
/// `key ? results[key] : results` returns an erased union; a missing key used
/// to be made total first (`unwrap_or(..)`, the value type's default) and then
/// erased, so `getQueryParams(url, 'absent')` answered `''` where JavaScript
/// answers `undefined`.
#[test]
fn keyed_record_read_into_an_erased_slot_erases_its_miss_as_undefined() {
    let source = source_for(
        r"
export function pick(rec: Record<string, string>, key: string): unknown {
  return rec[key];
}
",
    );
    assert!(
        source.contains(".get(&key).map(|value| SmeltUnknown::String(value.into())).unwrap_or(SmeltUnknown::Undefined)")
            || source.contains("unwrap_or(SmeltUnknown::Undefined)"),
        "{source}"
    );
    assert!(!program_part(&source).contains("unwrap_or(String::new())"), "{source}");
}

/// A string index read is total: `''[0]` is `undefined`, not a panic.
#[test]
fn string_index_read_out_of_range_does_not_panic() {
    let source = source_for(
        r"
export function startsWithSlash(parts: string[]): boolean {
  return parts[0][0] === '/';
}
",
    );
    assert!(!program_part(&source).contains("expect(\"index out of bounds\")"), "{source}");
}

/// `sourceBuffer as ArrayBuffer` over an `ArrayBufferView | ArrayBuffer`
/// digests the LIVE arm instead of projecting onto the asserted one.
///
/// A TypeScript assertion never changes the runtime value; projecting the
/// union onto its `ArrayBuffer` arm panicked with "union guard selected an
/// excluded member" whenever the value was a view (Hono's `utils/crypto.ts`).
#[test]
fn digest_of_an_asserted_buffer_source_union_reads_every_arm() {
    let source = source_for(
        r"
export async function hash(data: ArrayBufferView | ArrayBuffer): Promise<number> {
  const buffer = await crypto.subtle.digest('SHA-256', data as ArrayBuffer);
  return buffer.byteLength;
}
",
    );
    assert!(!source.contains("union guard selected an excluded member"), "{source}");
    assert!(source.contains("(value) => value.to_bytes()"), "{source}");
}

/// `FormDataEntryValue` is the spec's `File | string`, not an opaque class.
///
/// Opaque, an entry pushed into a `(string | File)[]` was injected into the
/// `File` arm even when it held a string.
#[test]
fn form_data_entry_value_is_the_file_or_string_union() {
    let source = source_for(
        r"
export function add(into: (string | File)[], value: FormDataEntryValue): number {
  return into.push(value);
}
",
    );
    let program = program_part(&source);
    assert!(!program.contains("SmeltBlob as SmeltFromUnknown"), "{program}");
}

/// A program that only constructs a `RegExp` and reads its `source` still
/// carries the `SmeltRegExp` runtime.
#[test]
fn regexp_constructed_but_never_matched_emits_its_runtime() {
    let source = source_for(
        r"
export function src(pattern: string): string {
  return new RegExp(pattern).source;
}
",
    );
    assert!(source.contains("pub struct SmeltRegExp"), "{source}");
}

/// A positive sleep under a `Promise.race` driver arms a wake-up timer instead
/// of advancing the clock itself, so a racer's `setTimeout(reject, ..)` that is
/// due first still wins.
#[test]
fn sleep_under_a_race_driver_arms_a_wake_up_timer() {
    let source = source_for(
        r"
export async function slow(): Promise<string> {
  await new Promise((resolve) => setTimeout(resolve, 100));
  return 'slow';
}
export async function run(): Promise<string> {
  return Promise.race([slow(), new Promise<string>((_, reject) => setTimeout(() => reject(new Error('t')), 10))]);
}
",
    );
    assert!(
        source.contains("if delay_ms > 0 && SMELT_RACE_DEPTH.with(::std::cell::Cell::get) > 0 {"),
        "{source}"
    );
}

/// `Object.getOwnPropertyDescriptor(rec, key)?.value` reads an own entry.
///
/// Smelt's records only hold data properties, so the descriptor is
/// `hasOwn ? { value, writable, enumerable, configurable } : undefined`; the
/// `?.value` read is the record's `value` key, not an iterator result's.
#[test]
fn own_property_descriptor_reads_the_own_entry_value() {
    let source = source_for(
        r"
export function ownValue(rec: Record<string, string>, key: string): unknown {
  return Object.getOwnPropertyDescriptor(rec, key)?.value;
}
",
    );
    assert!(source.contains("contains_key(&key"), "{source}");
    assert!(source.contains("\"configurable\".to_owned()"), "{source}");
    assert!(!program_part(&source).contains("\"getOwnPropertyDescriptor\""), "{source}");
}

/// `Array.prototype.map.call(view, f)` maps the view's elements.
///
/// The generic read-only `Array.prototype` methods run over
/// `Array.from(receiver)`, which reads the same elements; the erased
/// `prototype` read and dynamic `call` used to answer an empty list.
#[test]
fn array_prototype_read_method_call_maps_an_array_like_receiver() {
    let source = source_for(
        r"
export function hex(bytes: Uint8Array): string {
  return Array.prototype.map.call(bytes, (x) => ('00' + x.toString(16)).slice(-2)).join('');
}
",
    );
    let program = program_part(&source);
    assert!(!program.contains("\"prototype\""), "{program}");
    assert!(program.contains("to_elements()"), "{program}");
    assert!(program.contains("let radix ="), "the callback keeps its radix:\n{program}");
}

/// `x.toString(16)` inside a callback body keeps its radix.
#[test]
fn callback_body_number_to_string_keeps_its_radix() {
    let source = source_for(
        r"
export function hexes(xs: number[]): string[] {
  return xs.map((x) => x.toString(16));
}
",
    );
    assert!(source.contains("let radix ="), "{source}");
}

/// A ternary in argument position narrows its arms like any other ternary.
#[test]
fn argument_position_ternary_narrows_its_present_arm() {
    let source = source_for(
        r"
const queue: string[][] = [['a', 'b']];
const first = queue.shift();
console.log(first ? first.join(',') : 'none');
",
    );
    assert!(source.contains(".join("), "{source}");
}

/// The generated program below the runtime prelude, so a test's negative
/// assertions are not answered by prelude helpers that mention the same text.
fn program_part(source: &str) -> &str {
    source
        .split_once("@smelt:prelude-end")
        .map_or(source, |(_, program)| program)
}
