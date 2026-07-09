//! Regression tests for reference-class modeling.
//!
//! Covers the classification pass (each rule clause plus the value-class
//! negative), the reference-class emitter shape (handle newtype, identity
//! `Clone`, getter-as-method, narrow borrows), and the escaping-`this` capture.

use super::*;

/// Return the set of reference-class names classified for TypeScript `ts`.
fn reference_class_names(ts: &str) -> Vec<String> {
    let mut ctx = HirCtx::new();
    assert!(to_hir(ts, FileId(0), &mut ctx).is_ok(), "HIR");
    let mut mir = smelt_mir::lower_hir(&ctx.krate).expect("MIR lowering");
    smelt_mir::opt::optimize(&mut mir);
    let mut names: Vec<String> = crate::classify::reference_classes(&mir)
        .into_iter()
        .map(|symbol| mir.symbols.get(symbol).unwrap_or("?").to_owned())
        .collect();
    names.sort();
    names
}

// ---- Classification: each rule clause ------------------------------------

#[test]
fn classifies_method_self_field_mutation_as_reference() {
    // Rule: a non-constructor method assigns to `this.<field>`.
    let names = reference_class_names(
        r"
class Counter {
  count: number;
  constructor() { this.count = 0; }
  increment(): void { this.count = this.count + 1; }
}
function run(): void { const c = new Counter(); c.increment(); }
",
    );
    assert_eq!(names, vec!["Counter".to_owned()]);
}

#[test]
fn classifies_external_field_write_as_reference() {
    // Rule: a source path writes an instance field after construction.
    let names = reference_class_names(
        r"
class Bag {
  value: number;
  constructor() { this.value = 0; }
}
function mutate(bag: Bag): void { bag.value = 5; }
",
    );
    assert_eq!(names, vec!["Bag".to_owned()]);
}

#[test]
fn classifies_escaping_this_capture_as_reference() {
    // Rule: `this` is captured by a closure that escapes the method.
    let names = reference_class_names(
        r"
type Fn = () => void;
class Widget {
  tasks: Fn[];
  constructor() { this.tasks = []; }
  defer(): void {
    const task: Fn = () => { const w = this; };
    this.tasks.push(task);
  }
}
function run(): void { const w = new Widget(); w.defer(); }
",
    );
    assert!(names.contains(&"Widget".to_owned()), "{names:?}");
}

#[test]
fn value_class_constructed_and_read_is_not_reference() {
    // Negative: a class that is only constructed, aliased, and read stays a
    // value class — pure aliasing without mutation is not a trigger.
    let names = reference_class_names(
        r"
class Point {
  x: number;
  y: number;
  constructor(x: number, y: number) { this.x = x; this.y = y; }
  sum(): number { return this.x + this.y; }
}
function run(): number {
  const p = new Point(1, 2);
  const q = p;
  return p.sum() + q.sum();
}
",
    );
    assert!(names.is_empty(), "value class must not be lifted: {names:?}");
}

#[test]
fn constructor_field_init_alone_is_not_reference() {
    // Negative: a constructor initializing its own instance is construction,
    // not post-construction mutation, so it does not lift the class.
    let names = reference_class_names(
        r"
class Config {
  name: string;
  constructor(name: string) { this.name = name; }
  label(): string { return this.name; }
}
function run(): string { return new Config('a').label(); }
",
    );
    assert!(names.is_empty(), "{names:?}");
}

#[test]
fn structural_interface_field_write_is_not_a_reference_class() {
    // Negative: interfaces are structurally typed value records, never handles,
    // even when a field is written through a parameter.
    let names = reference_class_names(
        r"
interface Flags { era?: number; }
function setEra(flags: Flags, value: number): void { flags.era = value; }
function run(): number { const f: Flags = {}; setEra(f, 1); return f.era!; }
",
    );
    assert!(names.is_empty(), "{names:?}");
}

// ---- Emitter shape --------------------------------------------------------

#[test]
fn reference_class_emits_handle_newtype_and_identity_clone() {
    let source = source_for(
        r"
class Counter {
  count: number;
  constructor() { this.count = 0; }
  increment(): void { this.count = this.count + 1; }
}
function run(): void { const c = new Counter(); c.increment(); }
",
    );
    // Handle newtype over a shared cell, with a separate inner record.
    assert!(
        source.contains("struct Counter(::std::rc::Rc<::std::cell::RefCell<CounterInner>>);"),
        "{source}"
    );
    assert!(source.contains("struct CounterInner"), "{source}");
    // Hand-written identity `Clone` shares the cell (`Rc::clone`).
    assert!(
        source.contains("fn clone(&self) -> Self")
            && source.contains("Counter(::std::rc::Rc::clone(&self.0))"),
        "{source}"
    );
    // `Default`/`Debug` delegate through the cell.
    assert!(
        source.contains(
            "Counter(::std::rc::Rc::new(::std::cell::RefCell::new(CounterInner::default())))"
        ),
        "{source}"
    );
    assert!(
        source.contains("::std::fmt::Debug::fmt(&*self.0.borrow(), formatter)"),
        "{source}"
    );
}

#[test]
fn reference_class_methods_take_self_and_use_narrow_borrows() {
    let source = source_for(
        r"
class Counter {
  count: number;
  constructor() { this.count = 0; }
  increment(): void { this.count = this.count + 1; }
}
function run(): void { const c = new Counter(); c.increment(); }
",
    );
    // `&self` uniformly (interior mutability), never `&mut self`.
    assert!(source.contains("fn increment(&self)"), "{source}");
    assert!(!source.contains("fn increment(&mut self)"), "{source}");
    // Field reads borrow-and-clone; writes go through a narrow `borrow_mut()`.
    assert!(source.contains("self.0.borrow().count.clone()"), "{source}");
    assert!(source.contains("self.0.borrow_mut().count ="), "{source}");
}

#[test]
fn reference_class_getter_lowers_to_a_method_not_a_field() {
    let source = source_for(
        r"
class Counter {
  count: number;
  constructor() { this.count = 0; }
  increment(): void { this.count = this.count + 1; }
  get value(): number { return this.count; }
}
function run(): number { const c = new Counter(); c.increment(); return c.value; }
",
    );
    // The getter is a computed `&self` method; the read dispatches to it.
    assert!(source.contains("fn __smelt_get_value(&self) -> f64"), "{source}");
    assert!(source.contains(".__smelt_get_value()"), "{source}");
    // The getter is NOT a stored inner field.
    assert!(
        !source.contains("value: f64,"),
        "getter must not become a stored field\n{source}"
    );
}

#[test]
fn value_class_keeps_by_value_struct_emission() {
    let source = source_for(
        r"
class Point {
  x: number;
  y: number;
  constructor(x: number, y: number) { this.x = x; this.y = y; }
  sum(): number { return this.x + this.y; }
}
function run(): number { const p = new Point(1, 2); return p.sum(); }
",
    );
    // A value class keeps the derived by-value struct: no handle, no cell.
    assert!(source.contains("#[derive(Clone, Debug, Default)]"), "{source}");
    assert!(source.contains("struct Point {"), "{source}");
    assert!(!source.contains("struct Point("), "{source}");
    assert!(!source.contains("PointInner"), "{source}");
    assert!(!source.contains("self.0.borrow()"), "{source}");
}

// ---- Escaping-`this` capture ---------------------------------------------

#[test]
fn escaping_this_binds_self_capture_as_cloned_handle() {
    let source = source_for(
        r"
type Fn = () => void;
class Widget {
  tasks: Fn[];
  count: number;
  constructor() { this.tasks = []; this.count = 0; }
  defer(): void {
    const task: Fn = () => { this.count = this.count + 1; };
    this.tasks.push(task);
  }
}
function run(): void { const w = new Widget(); w.defer(); }
",
    );
    // The receiver is bound once as a cloned handle (never an unresolved
    // `smelt_capture_self`), and the escaping closure captures it by clone.
    assert!(
        source.contains(
            "let smelt_capture_self = ::std::rc::Rc::new(::std::cell::RefCell::new(self.clone()));"
        ),
        "{source}"
    );
    assert!(
        source.contains("let smelt_capture_self = smelt_capture_self.clone();"),
        "{source}"
    );
}
