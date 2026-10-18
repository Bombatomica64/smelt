//! Emission tests for closures stored on the object they capture
//! (`emitter::weak_receiver`): which closures take the weak capture, and which
//! must keep the strong one because something else can reach them.

use super::source_for;

/// A class-field arrow is stored only on its instance: it captures it weakly,
/// and the class gets the handle support and the drop-time escape check.
#[test]
fn a_field_arrow_captures_its_instance_weakly() {
    let source = source_for(
        r"
class Ctx {
  status = 200;
  json = (body: string): string => `${this.status}:${body}`;
  constructor(code: number) { this.status = code; }
}
function run(): string { const c = new Ctx(1); c.status = 2; return c.json('a'); }
console.log(run());
",
    );
    assert!(
        source.contains("let smelt_capture_this = SmeltWeakReceiver::new(&"),
        "{source}"
    );
    assert!(
        source.contains("impl SmeltReferenceHandle for Ctx"),
        "{source}"
    );
    assert!(
        source.contains(
            "impl Drop for Ctx { fn drop(&mut self) { if ::std::rc::Rc::strong_count(&self.0) == 1"
        ),
        "{source}"
    );
    assert!(
        source.contains("inner.json.smelt_closure_shared()"),
        "{source}"
    );
}

/// A closure that is stored AND handed elsewhere is not cycle-only: it keeps
/// the strong capture, and no weak-receiver support is emitted.
#[test]
fn a_stored_closure_that_also_escapes_keeps_the_strong_capture() {
    let source = source_for(
        r"
const keep: Array<() => number> = [];
class Ctx {
  status = 200;
  read: () => number;
  constructor(code: number) {
    const read = (): number => this.status;
    this.read = read;
    keep.push(read);
    this.status = code;
  }
}
function run(): number { const c = new Ctx(1); c.status = 2; return c.read(); }
console.log(run(), keep.length);
",
    );
    assert!(!source.contains("SmeltWeakReceiver::new("), "{source}");
    assert!(!source.contains("impl Drop for Ctx"), "{source}");
}

/// A field that something other than the constructor writes can have its
/// closure replaced while shared, so it is not a weak-receiver field.
#[test]
fn a_field_written_outside_the_constructor_keeps_the_strong_capture() {
    let source = source_for(
        r"
class Ctx {
  status = 200;
  json = (body: string): string => `${this.status}:${body}`;
  constructor(code: number) { this.status = code; }
}
function run(): string { const c = new Ctx(1); c.json = (body: string): string => body; return c.json('a'); }
console.log(run());
",
    );
    assert!(!source.contains("SmeltWeakReceiver::new("), "{source}");
}

/// A listener registered on the emitter it captures holds the emitter weakly;
/// one whose body builds a closure of its own keeps the strong capture.
#[test]
fn a_listener_captures_its_own_emitter_weakly_unless_it_builds_closures() {
    let weak = source_for(
        r#"
import { EventEmitter } from "node:events";
function attach(): EventEmitter {
  const e = new EventEmitter();
  e.on("ping", () => { e.emit("pong"); });
  return e;
}
attach().emit("ping");
"#,
    );
    assert!(
        weak.contains("let smelt_weak_e = SmeltWeakEmitter::new(&e);"),
        "{weak}"
    );
    assert!(weak.contains("smelt_weak_e.smelt_emitter()"), "{weak}");

    let strong = source_for(
        r#"
import { EventEmitter } from "node:events";
const later: Array<() => void> = [];
function attach(): EventEmitter {
  const e = new EventEmitter();
  e.on("ping", () => { later.push(() => { e.emit("pong"); }); });
  return e;
}
attach().emit("ping");
"#,
    );
    assert!(!strong.contains("SmeltWeakEmitter::new("), "{strong}");
}
