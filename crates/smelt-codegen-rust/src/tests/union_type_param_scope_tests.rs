//! A generic union enum's type parameters at use sites and inside the enum.
//!
//! `SmeltUnionN<T>` declares every free type parameter its members mention.
//! Requirements, independent of the implementation:
//!
//! * Behaviour: unchanged; these are spelling rules only.
//! * Type preservation: a use site passes one argument per declared parameter,
//!   resolved like any other type parameter at that position: spelled where
//!   the enclosing Rust item declares it, `SmeltUnknown` where it does not.
//!   A parameter bound by something no Rust item can declare (the own `<T>` of
//!   a callable TYPE stored in an `Rc<dyn Fn>`, or an item whose generics are
//!   erased) used to be spelled raw, naming a type no item declares (E0425
//!   `cannot find type T`, remeda's `lazyEmptyEvaluator`, Hono's
//!   `newRouter: <T>() => Router<T>`).
//! * Inside the enum and its impl blocks, every member renders in the enum's
//!   OWN scope: a list or callable member keeps the enum's `T`, it is not
//!   erased as if it were out of scope.

use super::*;

/// The fixture: a generic-arrow evaluator returning a union of generic
/// aliases, and a union whose callable member has its own type parameter.
const FIXTURE: &str = r#"
type Empty = { done: boolean; next?: undefined };
type Next<T> = { done: boolean; next: T };
type Many<T> = { done: boolean; next: readonly T[] };
type Outcome<T> = Empty | Next<T> | Many<T>;
const EMPTY: Empty = { done: true };
export const emptyOutcome = <T>(): Outcome<T> => EMPTY;
export const emptyRenamed = <R>(): Outcome<R> => EMPTY;

class Box<T> {
  value: T;
  constructor(value: T) { this.value = value; }
}
type Maker = <T>() => Box<T>;
export function pick(m: Maker | string[]): number {
  return Array.isArray(m) ? m.length : 1;
}
export function main(): number {
  const r = emptyOutcome<number>();
  const s = emptyRenamed<string>();
  return r.done || s.done ? 1 : 0;
}

type Init = { status: number };
type InitOrBox = Init | Box<string>;
interface Respond {
  (data: string, status?: number): number;
  (data: string, init?: InitOrBox): number;
}
export function respondWithBox(respond: Respond, box: Box<string>): number {
  return respond("x", box);
}
"#;

/// The generated source from the union enum declaring member `first_member`
/// onward.
fn union_declaration<'a>(source: &'a str, first_member: &str) -> &'a str {
    let member = source
        .find(first_member)
        .unwrap_or_else(|| panic!("a union declares `{first_member}`:\n{source}"));
    let start = source.get(..member).and_then(|head| head.rfind("pub enum ")).unwrap_or(member);
    source.get(start..).unwrap_or_default()
}

/// A use site outside any generic item erases a union's free parameter
/// instead of naming it.
#[test]
fn union_use_site_erases_a_parameter_its_item_does_not_declare() {
    let source = source_for(FIXTURE);
    let evaluator = emitted_function_body(&source, "fn empty_outcome(");
    let signature = evaluator.lines().next().unwrap_or_default();
    assert!(
        signature.contains("SmeltUnknown>") && !signature.contains("<T>"),
        "the evaluator declares no `T`, so its union return erases it: {signature}"
    );
    let pick = emitted_function_body(&source, "fn pick(");
    let signature = pick.lines().next().unwrap_or_default();
    assert!(
        signature.contains("<SmeltUnknown>") && !signature.contains("<T>"),
        "a callable type's own `<T>` is bound by no Rust item: {signature}"
    );
}

/// The erasure of `Many<R>` reads its `next` field as `SmeltList<R>`, the
/// field at the value's own type argument, not the declaration's `T[]`
/// judged against a scope that declares `R` and not `T`.
#[test]
fn union_member_fields_erase_at_the_member_type_arguments() {
    let source = source_for(FIXTURE);
    let outcome = union_declaration(&source, "M2(Many<R>)");
    let outcome = outcome.get(..outcome.find("\nimpl").and_then(|first| {
        outcome.get(first..).and_then(|rest| rest.find("\n}\n")).map(|end| first + end)
    }).unwrap_or(outcome.len())).unwrap_or(outcome);
    assert!(
        !outcome.contains(crate::ERASED_GENERIC_ARRAY_ALIAS_FN),
        "`Many<R>.next` is a `SmeltList<R>` in the enum's scope: {outcome}"
    );
}

/// A value of an exact arm of a union that is itself a member of the target
/// union (a callable interface merging `status?: number` and `init?: InitOrBox` into `number | InitOrBox`, given a `Box<string>`) is wrapped twice.
#[test]
fn value_injects_through_a_nested_union_member() {
    let source = source_for(FIXTURE);
    let caller = emitted_function_body(&source, "fn respond_with_box(");
    assert!(
        caller.contains("::M1(SmeltUnion") && caller.contains("::M1(box_"),
        "the box is wrapped in the inner union, then the outer: {caller}"
    );
}

/// Inside the enum, members keep the enum's own parameter: a callable member
/// recovers through the adapter at `T`, and a `T[]` field erases as an
/// in-scope generic list, not through the erased-generic alias helper.
#[test]
fn union_enum_members_render_in_the_enum_scope() {
    let source = source_for(FIXTURE);
    let maker = union_declaration(&source, "M0(::std::rc::Rc<dyn Fn() -> Box_<T>>)");
    assert!(
        !maker.contains("<::std::rc::Rc<dyn Fn() -> Box_<T>> as SmeltFromUnknown>"),
        "an `Rc<dyn Fn>` member has no `SmeltFromUnknown`: {maker}"
    );
    let recovery = maker.find("fn from_smelt_unknown").and_then(|at| maker.get(at..)).unwrap_or_default();
    let recovery = recovery.get(..recovery.find("\n}\n").unwrap_or(recovery.len())).unwrap_or(recovery);
    assert!(
        recovery.contains("Rc<dyn Fn() -> Box_<T>>") && !recovery.contains("Box_<SmeltUnknown>"),
        "the callable member's adapter keeps the enum's `T`: {recovery}"
    );
    let outcome = union_declaration(&source, "M2(Many<T>)");
    let outcome = outcome.get(..outcome.find("\nimpl").and_then(|first| {
        outcome.get(first..).and_then(|rest| rest.find("\n}\n")).map(|end| first + end)
    }).unwrap_or(outcome.len())).unwrap_or(outcome);
    assert!(
        !outcome.contains(crate::ERASED_GENERIC_ARRAY_ALIAS_FN),
        "`Many<T>.next` is a `SmeltList<T>` in the enum's scope: {outcome}"
    );
}
