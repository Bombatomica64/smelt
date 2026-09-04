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
//! * A local read by an EARLIER closure is predeclared and shared. Statements
//!   lower in source order, but a closure body runs when it is called, so it may
//!   legitimately read a `const` written below it. The read used to fall through
//!   to the module-global fallback, which fabricates an empty object for an
//!   `unknown` type.

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

#[test]
fn a_local_read_by_an_earlier_closure_is_shared_not_fabricated() {
    // es-toolkit's `timeout`: the abort handler is written before the timer
    // handle it clears. The handle must reach the handler, so the local is
    // reserved before the statement list is lowered and captured through the
    // shared cell -- the value is assigned after the closure exists.
    let source = source_for(
        r"
export function arm(signal: AbortSignal, ms: number): void {
  const onAbort = () => { clearTimeout(timeoutId); };
  const timeoutId = setTimeout(() => { (globalThis as any).fired = true; }, ms);
  signal.addEventListener('abort', onAbort, { once: true });
}
",
    );

    assert!(
        source.contains("smelt_clear_timeout((*smelt_capture_timeout_id.borrow()).clone())"),
        "the forward-read handle must be captured through the shared cell:\n{source}"
    );
    assert!(
        source.contains("(*smelt_capture_timeout_id.borrow_mut()) = "),
        "the declaration must write into the reserved local:\n{source}"
    );
    assert!(
        !source.contains("smelt_clear_timeout(SmeltUnknown::Object("),
        "a forward-referenced local must never be fabricated as an object:\n{source}"
    );
}

#[test]
fn new_through_a_function_value_emits_the_construct_operation() {
    // JavaScript `new f(args)` on a function value is `[[Construct]]`, not a
    // call: it allocates an object linked to `f.prototype`, runs `f` with that
    // object as its receiver, and keeps it unless `f` returned an object. The
    // lowering used to reuse `ExprKind::ClosureCall`, which expresses none of
    // that, so `new f()` was indistinguishable from `f()`.
    let source = source_for(
        r"
export function run(make: (...args: unknown[]) => unknown): unknown {
  return new (make as any)(1);
}
",
    );

    assert!(
        source.contains("smelt_construct("),
        "`new` through a function value must construct, not call:\n{source}"
    );
    assert!(
        source.contains("fn smelt_construct("),
        "the construction helper must be emitted:\n{source}"
    );
}

#[test]
fn instanceof_a_function_value_walks_the_prototype_chain() {
    // Every non-arrow JavaScript function is a constructor, so `x instanceof f`
    // is a runtime prototype-chain walk. It used to fold to a compile-time
    // `false` for every function-valued target, on the premise that Smelt's
    // runtime never constructs closure values with `new`.
    let source = source_for(
        r"
export function run(value: unknown): boolean {
  function C() {}
  return value instanceof C;
}
",
    );

    assert!(
        source.contains("smelt_instance_of_value("),
        "`instanceof` a function value must be answered at runtime:\n{source}"
    );
    assert!(
        source.contains("fn smelt_instance_of_value("),
        "the prototype-chain walk must be emitted:\n{source}"
    );
}

#[test]
fn a_property_write_onto_a_function_value_lands_in_its_property_bag() {
    // A JavaScript function is an object, so an undeclared property write onto
    // one is a real store. The write place had no slot for it and the statement
    // was emitted as a discard (`let _ = value;`), which is how
    // `wrapper.prototype = Object.create(func.prototype)` silently vanished.
    let source = source_for(
        r"
export function run(func: (...args: unknown[]) => unknown): unknown {
  const wrapper = function (...args: unknown[]) { return func(...args); };
  (wrapper as any).prototype = Object.create((func as any).prototype);
  return wrapper;
}
",
    );

    assert!(
        source.contains("smelt_set_property(\"prototype\", ")
            || source.contains("smelt_set_function_property(&wrapper"),
        "a property write onto a function value must land:\n{source}"
    );
    assert!(
        source.contains("fn smelt_set_function_property"),
        "the function-property store must be emitted:\n{source}"
    );
}

#[test]
fn a_property_read_on_a_function_value_reaches_its_property_bag() {
    // The read side of the same rule: `smelt_get_unknown_field` answered
    // `undefined` for every field of a `SmeltUnknown::Function`, so
    // `if (func.prototype)` never took its branch.
    let source = source_for(
        r"
export function run(func: unknown): boolean {
  return Boolean((func as any).prototype);
}
",
    );

    assert!(
        source.contains("SmeltUnknown::Function(function) => match smelt_function_value_property("),
        "an erased property read must consult the function property bag:\n{source}"
    );
}

#[test]
fn object_create_records_a_link_to_its_prototype() {
    // `Object.create(p)` copies `p`'s own members under the `__smelt_proto:`
    // prefix so they read as inherited. A copy cannot answer an identity
    // question, so the prototype itself is also recorded by reference — the
    // slot an `instanceof` chain walk and `Object.getPrototypeOf` follow.
    let source = source_for(
        r"
export function run(prototype: unknown): unknown {
  return Object.create(prototype as object);
}
",
    );

    assert!(
        source.contains("\"__smelt_proto:__proto__\""),
        "`Object.create` must record a link to its prototype:\n{source}"
    );
}
