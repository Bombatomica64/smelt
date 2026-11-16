//! Regressions for IR walkers that a catch-all arm let skip a shape.
//!
//! Each test names the walker and the shape it used to miss. Both shapes are
//! reached from ordinary source: an operation placed in a call terminator
//! (`Callee::Op`) because it runs inside a `try`, and a base-constructor
//! initializer call (`Callee::BaseInit`) for `super(..)`.

use super::*;

/// `fn.apply(this, args)` inside a `try`, in an arrow nested in a function
/// expression, reads the function expression's receiver.
///
/// A callback op inside a `try` is carried by a call terminator
/// (`Callee::Op(BindOwnThis)`) rather than by a statement, and the frame-`this`
/// scan only asked call terminators about `ThisRead`. The function expression
/// then reset the receiver channel instead of taking it, the arrow captured no
/// `smelt_frame_this`, and the bind inside it named an undefined
/// `smelt_frame_this` (E0425). The same body without the `try` (a statement)
/// was right, so both spellings must take and capture the receiver alike.
#[test]
fn frame_this_is_taken_for_a_receiver_bind_inside_try() {
    let source = source_for(
        r"
export function wrap(fn: (...args: number[]) => number): (...args: number[]) => number {
  return function (this: unknown, ...args: number[]): number {
    const inner = (): number => {
      try {
        return fn.apply(this, args);
      } catch (e) {
        return 0;
      }
    };
    return inner();
  };
}
",
    );
    let wrap = rust_fn(&source, "wrap");
    assert!(
        wrap.contains("let smelt_frame_this = smelt_take_this();"),
        "the function expression must take its receiver:\n{wrap}"
    );
    assert!(
        wrap.contains("let smelt_frame_this = smelt_frame_this.clone();"),
        "the arrow must capture the enclosing receiver:\n{wrap}"
    );
}

/// A record handed to a base constructor through `super(record)` flows into
/// the base constructor's parameter exactly as `new Base(record)` does.
///
/// The record-flow scan (`classify::for_each_record_flow`) resolved parameters
/// for static and indirect callees only, so a `Callee::BaseInit` argument
/// reached the base's `unknown` parameter unseen: `Rec` stayed a by-value
/// struct, erased as a snapshot, and a write through the caller's `r` after
/// `new Derived(r)` was invisible through the stored `this.value` (JavaScript
/// stores the same object).
#[test]
fn record_passed_to_super_is_shared_like_one_passed_to_new() {
    const HANDLE: &str = "struct Rec(::std::rc::Rc<::std::cell::RefCell<RecInner>>);";
    let through_super = source_for(
        r"
interface Rec { x: number }
class Base {
  value: unknown;
  constructor(value: unknown) { this.value = value; }
}
class Derived extends Base {
  constructor(r: Rec) { super(r); }
}
export function make(r: Rec): unknown {
  const d = new Derived(r);
  return d.value;
}
",
    );
    let through_new = source_for(
        r"
interface Rec { x: number }
class Base {
  value: unknown;
  constructor(value: unknown) { this.value = value; }
}
export function make(r: Rec): unknown {
  const b = new Base(r);
  return b.value;
}
",
    );
    assert!(
        through_new.contains(HANDLE),
        "the direct construction emits `Rec` as a shared handle:\n{through_new}"
    );
    assert!(
        through_super.contains(HANDLE),
        "`super(record)` must emit `Rec` as `new Base(record)` does:\n{through_super}"
    );
}
