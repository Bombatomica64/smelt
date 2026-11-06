//! Codegen coverage for a class instance's callable FIELD viewed through an
//! interface record (`emitter/receiver_bound_slot.rs`).
//!
//! The interface slot must dispatch through the instance on every call: it
//! installs the instance as `this` and reads the field at call time, so a
//! function stored in the field that reads `this` (and reassigns the field)
//! behaves as `iface.m(..)` does in JavaScript.

use super::*;

/// The shared fixture: `match` reads `this` and is installed as a field.
const FIELD_VIEWED_AS_INTERFACE: &str = r"
interface Router {
  name: string;
  match(path: string): string;
}
function match<R extends Router>(this: R, path: string): string {
  return (this as any).name + ':' + path;
}
class R1 implements Router {
  name: string = 'r1';
  match: typeof match = match;
}
function viaInterface(routers: Router[]): string {
  return routers[0].match('p');
}
console.log(viaInterface([new R1()]));
";

#[test]
fn interface_slot_of_callable_field_installs_the_instance_as_receiver() {
    let source = source_for(FIELD_VIEWED_AS_INTERFACE);
    assert!(
        source.contains("let smelt_slot_receiver = smelt_struct_value.clone();"),
        "the slot must capture the source instance:\n{source}"
    );
    assert!(
        source.contains("smelt_push_this_lazy"),
        "the slot must install the instance as `this`:\n{source}"
    );
}

#[test]
fn interface_slot_of_callable_field_is_plain_copy_without_this_reads() {
    // Without any `this` read the receiver is unobservable: the slot stays a
    // plain copy of the field value.
    let source = source_for(
        r"
interface Router {
  match(path: string): string;
}
function match(path: string): string {
  return path + '!';
}
class R1 implements Router {
  match: (path: string) => string = match;
}
function viaInterface(routers: Router[]): string {
  return routers[0].match('p');
}
console.log(viaInterface([new R1()]));
",
    );
    assert!(
        !source.contains("smelt_slot_receiver"),
        "no receiver binding without a `this` read:\n{source}"
    );
}

#[test]
fn class_to_class_conversion_keeps_callable_field_values() {
    // Converting between two CLASSES copies member values; wrapping a callable
    // field in a receiver-bound slot there would give the copy a different
    // function (`copy.d === original.d` false).
    let source = source_for(
        r"
class Base {
  d: (x: string) => string;
  constructor(d: (x: string) => string) {
    this.d = d;
  }
}
class Derived extends Base {}
function reads(this: any): unknown {
  return this;
}
function asBase(b: Base): string {
  return b.d('x');
}
const probe = { f: reads };
probe.f();
console.log(asBase(new Derived((x: string) => x + '!')));
",
    );
    assert!(
        !source.contains("smelt_slot_receiver") && !source.contains("smelt_slot_source"),
        "class-to-class conversion must not wrap callable fields:\n{source}"
    );
}

#[test]
fn computed_key_write_into_class_dispatches_on_the_key() {
    // `this[method] = f` installs a handler on the field the runtime key
    // names; it used to be discarded (`let _ = ..`).
    let source = source_for(
        r"
const METHODS = ['get', 'post'] as const;
class App {
  get!: (path: string) => string;
  post!: (path: string) => string;
  constructor() {
    const all = [...METHODS];
    all.forEach((method) => {
      this[method] = (path: string) => method + path;
    });
  }
}
console.log(new App().get('/a'));
",
    );
    assert!(
        source.contains("match smelt_key.as_str() {") && source.contains("\"get\" => {"),
        "the keyed write must dispatch over the class fields:\n{source}"
    );
}
