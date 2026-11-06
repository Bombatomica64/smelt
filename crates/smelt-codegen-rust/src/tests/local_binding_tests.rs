//! Typed local bindings: each test states what a capture must render as, so a
//! body that fell back to the source name or the wrong cell form fails here.

use super::*;

#[test]
fn a_captured_receiver_binds_under_its_own_name_in_the_body() {
    let source = source_for(
        r"
class Counter {
    value = 1;
    read(): number {
        const get = () => this.value;
        return get();
    }
}
console.log(new Counter().read());
",
    );
    assert!(source.contains("let smelt_self = self.clone();"), "{source}");
    assert!(source.contains("smelt_self.0.borrow().value"), "{source}");
    assert!(!source.contains("let self ="), "{source}");
}

#[test]
fn a_nested_closure_reaches_the_same_shared_cell() {
    let source = source_for(
        r"
function run(): number {
    let count = 0;
    const outer = () => {
        const inner = () => { count = count + 1; };
        inner();
    };
    outer();
    return count;
}
console.log(run());
",
    );
    assert!(source.contains("let smelt_capture_count = smelt_capture_count.clone();"), "{source}");
    assert!(source.contains("(*smelt_capture_count.borrow_mut()) ="), "{source}");
    assert!(!source.contains("smelt_capture_(*"), "{source}");
}

#[test]
fn a_self_recursive_closure_upgrades_its_weak_cell_before_borrowing() {
    let source = source_for(
        r"
function run(): number {
    const walk = (n: number): number => (n <= 0 ? 0 : 1 + walk(n - 1));
    return walk(3);
}
console.log(run());
",
    );
    assert!(source.contains("::std::rc::Rc::downgrade(&smelt_capture_walk)"), "{source}");
    assert!(source.contains("smelt_capture_walk.upgrade().expect("), "{source}");
}

#[test]
fn an_async_closure_owns_a_clone_of_the_cell_handle() {
    let source = source_for(
        r"
async function run(): Promise<number> {
    let count = 0;
    const bump = async () => {
        const again = async () => { count = count + 1; };
        await again();
    };
    await bump();
    return count;
}
run();
",
    );
    assert!(source.contains("let smelt_capture_count = smelt_capture_count.clone();"), "{source}");
    assert!(!source.contains("let (*smelt_capture_count"), "{source}");
}

#[test]
fn an_async_method_clones_its_receiver_only_when_the_body_names_it() {
    let source = source_for(
        r"
class Box1 {
    value = 1;
    async uses(): Promise<number> { return this.value; }
    async ignores(): Promise<number> { return 2; }
}
new Box1().uses();
new Box1().ignores();
",
    );
    assert_eq!(source.matches("let self_owned = self.clone();").count(), 1, "{source}");
}
