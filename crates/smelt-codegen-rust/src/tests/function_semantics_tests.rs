//! Codegen regression tests for JavaScript function-value semantics.
//!
//! * A method call supplies a receiver. `recv.m(args)` is a call PLUS a
//!   receiver, so a callable stored as an ordinary property sees the object it
//!   was read from as its `this`. The bind is installed on the member READ,
//!   before the call's own coercion re-types it — a read typed `unknown` is
//!   coerced to a function type synthesized from the argument types, so asking
//!   the coerced operand which ABI it uses always answered "a concrete
//!   function" and no ordinary method call carried a receiver at all.
//! * A receiver that nothing can read is not installed. `this` is only
//!   observable through a read, so a program containing no `this` carries no
//!   `SMELT_THIS` channel even though every erased method call binds one.
//! * An erased `Map` receiver still mutates the map. `Map.prototype.set` on a
//!   `SmeltUnknown` receiver used to resolve to `undefined`, and the call of an
//!   `undefined` callee collapses to a null callback, so the write vanished.

use super::*;

#[test]
fn a_method_call_installs_the_object_it_was_read_from_as_the_receiver() {
    let source = source_for(
        r"
export function run(): unknown {
  const callee = function (a: number) { return (a + (this as any).b) as number; };
  const object = { m: callee, b: 2 };
  return (object as any).m(1);
}
",
    );

    // One receiver temporary feeds both the member read and the bind, so the
    // object is evaluated once.
    assert!(
        source.contains(
            "smelt_bind_this(smelt_get_unknown_field(&_smelt_tmp_5.clone(), \"m\").clone(), _smelt_tmp_5.clone())"
        ),
        "a method call must bind the read receiver:\n{source}"
    );
}

#[test]
fn a_program_without_this_carries_no_receiver_channel() {
    let source = source_for(
        r"
export function run(): unknown {
  const object = { m: (a: number) => a + 1 };
  return (object as any).m(1);
}
",
    );

    assert!(
        !source.contains("SMELT_THIS"),
        "a program that never mentions `this` must not carry the channel:\n{source}"
    );
    assert!(
        !source.contains("smelt_bind_this"),
        "an unobservable receiver bind must be dropped:\n{source}"
    );
}

#[test]
fn an_erased_map_carries_its_mutating_members() {
    let source = source_for(
        r"
export function run(cache: Map<string, number> | Record<string, number>): void {
  (cache as any).set('a', 1);
}
",
    );

    assert!(
        source.contains("\"set\" => { let store = entry_store.clone();"),
        "an erased Map must synthesize `set`:\n{source}"
    );
}
