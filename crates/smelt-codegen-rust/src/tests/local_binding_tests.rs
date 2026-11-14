//! Shapes of emitted local bindings that behaviour cannot see: what a capture
//! renders as when the running program would print the same either way. The
//! captures' meaning is exercised by the `function_value_semantics` Node-parity
//! fixture.

use super::*;

/// A self-recursive closure holds itself weakly, so it does not keep itself alive.
///
/// The cell starts as an unassigned `SmeltBinding` and is filled by `bind`
/// (C8). Old requirement: it was seeded with a default callback answering
/// `0.0`, a value that stood in for the function until the assignment
/// replaced it. New requirement: no default stands in for a binding before
/// its assignment; MIR proves this closure cannot run first, so its read is
/// the unchecked `bound()`.
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
    assert_eq!(
        rust_fn(&source, "run"),
        r#"fn run() -> f64 {
let smelt_capture_walk: ::std::rc::Rc<::std::cell::RefCell<SmeltBinding<::std::rc::Rc<dyn Fn(f64) -> f64>>>> = ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltBinding::unbound()));
let _smelt_tmp_1 = ::std::rc::Rc::new({
let smelt_capture_walk = ::std::rc::Rc::downgrade(&smelt_capture_walk);
move |closure_arg_0: f64| {
let mut _smelt_tmp_3: f64;
let _smelt_tmp_4: f64;
let _smelt_tmp_5: f64;
let _smelt_tmp_2: bool = closure_arg_0 <= 0.0;
if _smelt_tmp_2 {
_smelt_tmp_3 = 0.0;
} else {
_smelt_tmp_4 = closure_arg_0 - 1.0;
_smelt_tmp_5 = ((*(*smelt_capture_walk.upgrade().expect("self-recursive closure called after its defining scope returned").borrow()).bound()))(_smelt_tmp_4);
_smelt_tmp_3 = 1.0 + _smelt_tmp_5;
}
_smelt_tmp_3
}
});
{ let smelt_next_value = _smelt_tmp_1.clone(); (*smelt_capture_walk.borrow_mut()).bind(smelt_next_value); }
let _smelt_tmp_2: f64 = { let smelt_callable = ::std::clone::Clone::clone(&(*(*smelt_capture_walk.borrow()).bound())); (smelt_callable)(3.0) };
return _smelt_tmp_2;
}"#
    );
}

/// An async method's future owns its receiver only when the body reads it.
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
    assert_eq!(
        rust_fn(&source, "uses"),
        r"fn uses(&self) -> SmeltFuture<f64> {
let self_owned = self.clone();
SmeltFuture::<f64>::from_future_primed(Box::pin(async move {
return Ok(self_owned.value);
}))
}"
    );
    assert_eq!(
        rust_fn(&source, "ignores"),
        r"fn ignores(&self) -> SmeltFuture<f64> {
SmeltFuture::<f64>::from_future_primed(Box::pin(async move {
return Ok(2.0);
}))
}"
    );
}
