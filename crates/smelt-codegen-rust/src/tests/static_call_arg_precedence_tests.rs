//! Precedence tests for the static-call argument ladder.
//!
//! `emitter::static_call_args` makes the ladder a total function: one classifier
//! assigns each argument exactly one `StaticArgumentKind`, and the rungs are
//! tried in a fixed, documented order:
//!
//! * a borrowed callback outranks the monomorphization passthrough, and
//! * a `&mut` parameter outranks the monomorphization passthrough.
//!
//! Neither order is observable unless an argument satisfies BOTH competing
//! rungs at once, so each test builds that overlap: a callee that really emits
//! Rust generics, called at a site that pins those generics to concrete types.
//! Each caller is compared whole because the rendered argument forms are the
//! requirement here; what the programs compute is in the
//! `function_value_semantics` Node-parity fixture.

use super::*;

/// A `&mut` list parameter a generic callee mutates is passed as a borrow.
///
/// The callback parameter names the callee's own type parameter, which keeps
/// this call out of the convert-in-place list adapter, so the classifier is the
/// code that renders it.
#[test]
fn mutable_reference_outranks_monomorphizing_passthrough() {
    let source = source_for(
        r"
function fillWith<T>(xs: T[], make: (index: number) => T): void {
  xs.push(make(0));
}
export function useFill(): number[] {
  const nums: number[] = [1, 2];
  fillWith(nums, (i: number) => i + 10);
  return nums;
}
",
    );

    assert_eq!(rust_fn(&source, "fill_with"), r#"fn fill_with<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, F0: Fn(f64) -> T + ?Sized>(mut xs: &mut SmeltList<T>, make: &F0) -> () {
let _smelt_tmp_2: T = make(0.0);
let _smelt_tmp_3: f64 = { let smelt_push_item = _smelt_tmp_2; xs.borrow_mut().push(smelt_push_item); xs.len() as f64 };
return;
}"#);
    assert_eq!(rust_fn(&source, "use_fill"), r#"fn use_fill() -> SmeltList<f64> {
let mut nums: SmeltList<f64> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<f64> = vec![1.0, 2.0]; smelt_list_items }));
let _smelt_tmp_2 = ::std::rc::Rc::new(|closure_arg_0: f64| {
let _smelt_tmp_1: f64 = closure_arg_0 + 10.0;
_smelt_tmp_1
});
let _ = fill_with(&mut nums, &*_smelt_tmp_2);
return nums;
}"#);
}

/// A borrowed callback that could also pass through is reborrowed at the call.
#[test]
fn borrowed_callback_outranks_monomorphizing_passthrough() {
    let source = source_for(
        r"
function applyTo<T>(xs: T[], fn: (item: T) => T): T[] {
  return xs.map(fn);
}
export function useApply(): number[] {
  const nums: number[] = [1, 2, 3];
  return applyTo(nums, (n: number) => n + 1);
}
",
    );

    assert_eq!(rust_fn(&source, "apply_to"), r#"fn apply_to<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, F0: Fn(&T) -> T + ?Sized>(xs: SmeltList<T>, fn_: &F0) -> SmeltList<T> {
let _smelt_tmp_2: SmeltList<T> = Into::<SmeltList<_>>::into({ let smelt_callback = fn_; xs.borrow().iter().enumerate().map(|(smelt_index, smelt_item)| { (smelt_callback)(smelt_item) }).collect::<Vec<_>>() });
return _smelt_tmp_2;
}"#);
    assert_eq!(rust_fn(&source, "use_apply"), r#"fn use_apply() -> SmeltList<f64> {
let nums: SmeltList<f64> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<f64> = vec![1.0, 2.0, 3.0]; smelt_list_items }));
let _smelt_tmp_2 = ::std::rc::Rc::new(|closure_arg_0: f64| {
let _smelt_tmp_1: f64 = closure_arg_0 + 1.0;
_smelt_tmp_1
});
let _smelt_tmp_3: SmeltList<f64> = apply_to(nums, &mut { let _smelt_adapted_callback = _smelt_tmp_2.clone(); move |smelt_arg0: &f64| -> f64 { (_smelt_adapted_callback)(smelt_arg0.clone()) } });
return _smelt_tmp_3;
}"#);
}

/// An instance method borrows a parameter it mutates and pads one the call omits.
#[test]
fn a_method_call_borrows_a_mutated_parameter_and_pads_an_omitted_one() {
    let source = source_for(
        r"
type Row = { id: number };

class Collector {
  rows: Row[] = [];

  #addRow(into: Row[], row: Row, tag?: string): void {
    into.push(row);
    if (tag) {
      into.push({ id: row.id + 1 });
    }
  }

  collect(): number {
    const sink: Row[] = [];
    this.#addRow(sink, { id: 1 });
    this.#addRow(sink, { id: 5 }, 'twice');
    return sink.length;
  }
}

const total = new Collector().collect();
console.log(total);
",
    );

    assert_eq!(rust_fn(&source, "collect"), r#"fn collect(&self) -> f64 {
let _smelt_tmp_5: SmeltRecord<JsString, f64>;
let _smelt_tmp_7: f64;
let mut sink: SmeltList<Row> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<Row> = vec![]; smelt_list_items }));
let _smelt_tmp_3: SmeltRecord<JsString, f64> = SmeltRecord::from([(JsString::from("id"), 1.0)]);
let _ = self._add_row(&mut sink, { let smelt_record_map = _smelt_tmp_3.clone(); Row { id: smelt_record_map.get_text("id").cloned().map_or(0.0, |smelt_value| smelt_value) } }, None::<JsString>);
_smelt_tmp_5 = SmeltRecord::from([(JsString::from("id"), 5.0)]);
let _ = self._add_row(&mut sink, { let smelt_record_map = _smelt_tmp_5.clone(); Row { id: smelt_record_map.get_text("id").cloned().map_or(0.0, |smelt_value| smelt_value) } }, Some(JsString::from("twice")));
_smelt_tmp_7 = sink.len() as f64;
return _smelt_tmp_7;
}"#);
}
