//! Class prototype methods are a dynamic boundary that only reflection pays for.
//!
//! `class_proto::class_prototype_methods_fn` gives a class's prototype object
//! its methods as `SmeltUnknown`-typed, `this`-taking functions
//! (docs/design/class-prototypes.md, section 7). These tests pin both sides of
//! the SmeltUnknown policy for it:
//!
//! * a program that reaches a class prototype by reflection, and then names a
//!   method only by string on an erased value, gets the builder: the method set
//!   it reads is a run-time fact of the value, which no concrete type, union or
//!   scoped generic can spell;
//! * a program where the class is statically known gets none of it: no builder,
//!   no table row, and no `SmeltUnknown` at all;
//! * a program that carries the `this` channel but never reaches a prototype
//!   does not pay for the builder either.

use super::*;

/// The classes both programs declare.
const CLASSES: &str = r"
class Animal {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
  speak(): string {
    return `${this.name} speaks`;
  }
}
";

#[test]
fn prototype_methods_exist_only_for_reflection() {
    // Reflection: the prototype of a value whose class is not known
    // statically, and a method named only by string through it.
    let reflective = source_for(&format!(
        "{CLASSES}
export function speakThrough(value: unknown): unknown {{
  const proto: any = Object.getPrototypeOf(value);
  return proto.speak;
}}
export function make(): unknown {{
  return new Animal('a');
}}
"
    ));
    assert!(
        reflective.contains("fn __smelt_prototype_methods() -> Vec<(String, SmeltUnknown)>"),
        "a program that reaches a class prototype by reflection carries its methods:\n{reflective}"
    );
    assert!(
        reflective.contains("(\"Animal\", \"\", Some(Animal::__smelt_prototype_methods)),"),
        "the runtime's class table names the builder:\n{reflective}"
    );

    // Typed: the class is statically known, so the call is a typed method call
    // and nothing is erased.
    let typed = source_for(&format!(
        "{CLASSES}
export function speakTyped(): string {{
  const animal = new Animal('a');
  return animal.speak();
}}
"
    ));
    assert!(
        !typed.contains("__smelt_prototype_methods"),
        "a statically typed class does not pay for prototype methods:\n{typed}"
    );
    assert!(
        !typed.contains("SmeltUnknown"),
        "the typed path emits no SmeltUnknown at all:\n{typed}"
    );

    // A function reading `this` carries the channel, but reaches no class
    // prototype.
    let erased_call = source_for(&format!(
        "{CLASSES}
function nameOf(this: any): unknown {{
  return this.name;
}}
export function viaThis(value: any): unknown {{
  return nameOf.call(value);
}}
export function make(): unknown {{
  return new Animal('a');
}}
"
    ));
    assert!(
        erased_call.contains("fn smelt_this("),
        "the program carries the `this` channel (the case the gate must not key on):\n{erased_call}"
    );
    assert!(
        !erased_call.contains("fn __smelt_prototype_methods"),
        "the builder is gated on reflection, not on the `this` channel:\n{erased_call}"
    );
}
