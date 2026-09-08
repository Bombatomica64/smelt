//! Precedence tests for the static-call argument ladder.
//!
//! `emitter::static_call_args` makes the ladder a total function: one classifier
//! assigns each argument exactly one `StaticArgumentKind`, and the rungs are
//! tried in a fixed, documented order. Two of that order's constraints are
//! load-bearing, and each is a defect the callback-generics campaign actually
//! shipped:
//!
//! * a borrowed callback outranks the monomorphization passthrough, and
//! * a `&mut` parameter outranks the monomorphization passthrough.
//!
//! Neither constraint is observable unless an argument satisfies BOTH competing
//! rungs at once, so each test below builds exactly that overlap: a callee that
//! really emits Rust generics (so the passthrough rung is live), called at a
//! site that pins those generics to concrete types (so `substitution_matches`
//! succeeds for the contested argument). Reversing either constraint changes the
//! asserted text — verified by reversing it — so neither test passes vacuously.

use super::*;

/// A `&mut` parameter that is ALSO a monomorphizing composite must be rendered
/// by the mutable-reference rung, not by the passthrough.
///
/// `xs: T[]` on a callee that mutates it lowers to `&mut SmeltList<T>`, and this
/// call site pins `T = f64`, so `substitution_matches` holds for the argument:
/// the passthrough rung would claim it and render it BY VALUE against a `&mut`
/// parameter (E0308). The mutable-reference rung runs first and renders the
/// borrow.
///
/// The callback parameter is not decoration. A `&mut` LIST argument is normally
/// emitted by a THIRD ladder — `call::static_call_mut_list_adapter_text`, the
/// convert-in-place adapter, recognisable by its `smelt_mut_call_result`
/// wrapper — which never reaches the classifier at all. That adapter declines
/// for a callee whose callback parameter mentions one of the callee's own
/// emitted type parameters, which is exactly what `make: (index: number) => T`
/// does, so this call is emitted by the main ladder and the contested `&mut`
/// argument really is classified here.
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

    // The callee emits real generics behind a `&mut` list parameter, which is
    // what makes the argument a candidate for BOTH rungs. Without this the test
    // would pass for the wrong reason.
    assert!(
        source.contains("(mut xs: &mut SmeltList<T>, make: &F0)"),
        "callee did not emit a generic `&mut` list parameter, so the two rungs do not \
         overlap here:\n{source}"
    );
    // The caller's local is the concrete `SmeltList<f64>` the call site pins
    // `T` to, so the passthrough's `substitution_matches` really does hold.
    assert!(
        source.contains("let mut nums: SmeltList<f64>"),
        "caller local is not the concrete list that pins `T`:\n{source}"
    );
    // Not the convert-in-place adapter: this call went through the classifier.
    assert!(
        !source.contains("smelt_mut_call_result"),
        "the mutable-list adapter emitted this call, so the main ladder never classified \
         its arguments:\n{source}"
    );
    // The mutable-reference rung won: a borrow, not a value.
    assert!(
        source.contains("fill_with(&mut nums, "),
        "expected the `&mut` rung to render a borrow:\n{source}"
    );
    assert!(
        !source.contains("fill_with(nums"),
        "the passthrough claimed a `&mut` parameter and rendered it by value:\n{source}"
    );
}

/// A borrowed callback that is ALSO a monomorphizing passthrough candidate must
/// be rendered by the borrowed-callback rung, not by the passthrough.
///
/// Once a callback-bearing callee emits real generics, a pinned call site
/// satisfies `substitution_matches` for its `Fn(T) -> T` parameter too, so the
/// passthrough rung would claim the callback and render it as an owned
/// `Rc<closure>` against a borrowed `&F0` parameter (E0308). The
/// borrowed-callback rung runs first and reborrows it.
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

    // The callee emits real generics AND takes its callback by reference behind
    // an `F{n}` bound: that is the overlap this test exists for.
    assert!(
        source.contains("fn apply_to<T:") && source.contains("F0: Fn(&T) -> T + ?Sized>"),
        "callee did not emit a generic borrowed callback parameter, so the two rungs do not \
         overlap here:\n{source}"
    );
    // The sibling list argument passes through concretely, which is the direct
    // evidence that this call site DID monomorphize the callee — the state in
    // which the passthrough rung competes for the callback.
    assert!(
        source.contains("apply_to(nums, "),
        "call site did not monomorphize the callee, so the passthrough rung is not in \
         play:\n{source}"
    );
    // The borrowed-callback rung won: the callback is reborrowed at the call,
    // not handed over as the owned `Rc` handle the caller holds.
    assert!(
        source.contains("apply_to(nums, &mut { let _smelt_adapted_callback ="),
        "expected the borrowed-callback rung to reborrow the callback:\n{source}"
    );
    assert!(
        !source.contains("apply_to(nums, _smelt_tmp"),
        "the passthrough claimed a borrowed callback and rendered it as an owned \
         handle:\n{source}"
    );
}


/// H25: a CLASS METHOD's parameters carry the same ABI as every other callee's.
///
/// A parameter the callee mutates in place is emitted `&mut T` by the very
/// predicate the call site can consult (`parameter_needs_mutable_reference_in`),
/// and a parameter the call omits is padded with its default. Both were already
/// true for free functions, static methods and constructors; the instance-method
/// arm asked neither question, so Hono's `#pushHandlerSets(handlerSets, node,
/// method, nodeParams, params?)` -- a private method that pushes into its first
/// parameter and is called both with and without the optional tail -- produced
/// `expected &mut SmeltList<_>, found SmeltList<_>` and `takes 5 arguments but 4
/// were supplied`: together 419 of the router slice's 498 errors.
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

    // The signature side: the mutated parameter really is by-reference here, so
    // the call-site assertions below are not vacuous.
    assert!(
        source.contains("into: &mut SmeltList<"),
        "the mutated parameter did not lower to `&mut`, so this fixture does not \
         exercise the ABI:\n{source}"
    );
    // The call with the optional argument omitted passes `None` for it rather
    // than being short one argument.
    assert!(
        source.contains("self._add_row(&mut sink,"),
        "a method call passed a `&mut` parameter by value:\n{source}"
    );
    assert!(
        !source.contains("self._add_row(sink.clone()"),
        "the mutated argument is still cloned by value:\n{source}"
    );
    // The two calls differ only in the optional tail: the one that omits it
    // pads `None`, the one that writes it passes `Some(..)`. Both must be
    // present, so neither the padding nor the ordinary argument regressed.
    assert!(
        source.contains(", None::<String>);"),
        "the omitted optional parameter was not padded:\n{source}"
    );
    assert!(
        source.contains("Some(\"twice\".to_owned()));"),
        "the supplied optional argument no longer reaches the call:\n{source}"
    );
}
