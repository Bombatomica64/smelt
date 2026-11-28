//! Typed deep equality (`crate::deep_equality`): which records a program's
//! deep comparisons give an `impl SmeltDeepEq`, and the Rust they are emitted
//! as. Behavior against vitest is the fixture
//! `smelt-transpiler/tests/fixtures/assertion-parity/typed_deep_equality.test.ts`.

use super::*;
use crate::deep_equality::DeepEquality;
use crate::runtime_demand::{Need, RuntimeDemand};

/// The vitest import every program here starts with.
const VITEST: &str = "import { test, expect } from 'vitest';\n";

/// The names of the records TypeScript `ts` plans an impl for, sorted.
fn planned_impls(ts: &str) -> Vec<String> {
    let mut ctx = HirCtx::new();
    assert!(to_hir(ts, FileId(0), &mut ctx).is_ok(), "HIR");
    let lowered = smelt_mir::lower_hir(&ctx.krate);
    assert!(lowered.is_ok(), "MIR lowering");
    let Ok(mut mir) = lowered else {
        return Vec::new();
    };
    smelt_mir::opt::optimize(&mut mir);
    let plan = DeepEquality::plan(&mir, &classify::reference_classes(&mir));
    let mut names: Vec<String> = mir
        .interfaces
        .iter()
        .filter(|interface| plan.has_impl(interface.name))
        .map(|interface| mir.symbols.get(interface.name).unwrap_or("?").to_owned())
        .collect();
    names.sort();
    names
}

/// The runtime demand of TypeScript `ts`.
fn demand_of(ts: &str) -> RuntimeDemand {
    let mut ctx = HirCtx::new();
    assert!(to_hir(ts, FileId(0), &mut ctx).is_ok(), "HIR");
    let mut mir = match smelt_mir::lower_hir(&ctx.krate) {
        Ok(mir) => mir,
        Err(_) => panic!("MIR lowering failed"),
    };
    smelt_mir::opt::optimize(&mut mir);
    RuntimeDemand::of(&mir, &classify::reference_classes(&mir))
}

/// A generic record written through a second reference, and a list of them
/// compared.
const GENERIC: &str = "interface Box<T> { value: T; label: string }\n\
     function fill<T>(box: Box<T>, value: T): Box<T> { box.value = value; return box; }\n\
     function boxes(first: number): Box<number>[] { const list: Box<number>[] = [{ value: 0, label: 'a' }]; fill(list[0], first); return list; }\n";

/// Rule: a record a deep comparison reaches through a list gets an impl; the
/// same program without the comparison plans none.
#[test]
fn a_generic_record_in_a_compared_list_is_planned() {
    let compared = format!(
        "{VITEST}{GENERIC}test('same', () => {{ expect(boxes(1)).toEqual(boxes(1)); }});\n"
    );
    assert_eq!(planned_impls(&compared), vec!["Box".to_owned()]);
    let uncompared = format!(
        "{VITEST}{GENERIC}test('count', () => {{ expect(boxes(1).length).toEqual(1); }});\n"
    );
    assert_eq!(planned_impls(&uncompared), Vec::<String>::new());
}

/// Rule: a by-value struct is compared field with field, and a shared record
/// it holds keeps the comparison of its views (no impl of its own): it is not
/// a record type's argument.
#[test]
fn a_struct_holding_a_shared_record_is_planned_and_the_record_is_not() {
    let ts = format!(
        "{VITEST}interface Person {{ name: string; age: number }}\ninterface Team {{ lead: Person; size: number }}\n\
         function birthday(person: Person): Person {{ person.age += 1; return person; }}\n\
         function teams(age: number): Team[] {{ const lead: Person = {{ name: 'a', age }}; birthday(lead); const team: Team = {{ lead, size: 3 }}; return [team]; }}\n\
         test('same', () => {{ expect(teams(1)).toEqual(teams(1)); }});\n"
    );
    assert_eq!(planned_impls(&ts), vec!["Team".to_owned()]);
    // The shared record is reached below a record, so the program carries the
    // erased runtime its views live in.
    assert!(demand_of(&ts).has(Need::Unknown));
    assert!(demand_of(&ts).has(Need::DeepEq));
}

/// Rule: a shared record that is a record type's ARGUMENT gets an impl, since
/// the generic record's impl compares a field of type `T` through the trait.
#[test]
fn a_shared_record_that_is_a_type_argument_is_planned() {
    let ts = format!(
        "{VITEST}{GENERIC}interface Person {{ name: string; age: number }}\n\
         function birthday(person: Person): Person {{ person.age += 1; return person; }}\n\
         function people(age: number): Box<Person>[] {{ const person: Person = {{ name: 'a', age }}; birthday(person); const list: Box<Person>[] = [{{ value: person, label: 'p' }}]; return list; }}\n\
         test('same', () => {{ expect(people(1)).toEqual(people(1)); }});\n"
    );
    assert_eq!(
        planned_impls(&ts),
        vec!["Box".to_owned(), "Person".to_owned()]
    );
}

/// What is not planned keeps its `PartialEq`: a record with a function-typed
/// field, and the record holding it (whose field would have no impl to call
/// and no `PartialEq` to fall back on).
#[test]
fn a_record_with_a_callback_field_is_not_planned() {
    let ts = format!(
        "{VITEST}interface Handler {{ run: (value: number) => number; name: string }}\ninterface Slot {{ handler: Handler; index: number }}\n\
         function slots(): Slot[] {{ const handler: Handler = {{ run: value => value + 1, name: 'h' }}; const slot: Slot = {{ handler, index: 0 }}; return [slot]; }}\n\
         test('same', () => {{ const all = slots(); expect(all).toEqual(all); }});\n"
    );
    assert_eq!(planned_impls(&ts), Vec::<String>::new());
}

/// The emitted impls: a by-value struct compares field with field (a number
/// with the nested-number rule, a shared record through its views), a generic
/// reference record is one object, then enters the pair, then compares its
/// fields through its parameter's bound; and the function body compares the
/// list's items through the trait, with neither operand erased.
#[test]
fn the_impls_compare_field_with_field_at_the_records_own_types() {
    let source = source_for(&format!(
        "{VITEST}{GENERIC}interface Person {{ name: string; age: number }}\ninterface Team {{ lead: Person; size: number }}\n\
         function birthday(person: Person): Person {{ person.age += 1; return person; }}\n\
         function teams(age: number): Team[] {{ const lead: Person = {{ name: 'a', age }}; birthday(lead); const team: Team = {{ lead, size: 3 }}; return [team]; }}\n\
         test('boxes', () => {{ expect(boxes(1)).toEqual(boxes(1)); }});\n\
         test('teams', () => {{ expect(teams(1)).toEqual(teams(1)); }});\n"
    ));
    // The trait is part of the runtime prelude of a program that plans an impl.
    assert!(source.contains("pub trait SmeltDeepEq {"), "no trait");
    let body = program_body(&source);
    for expected in [
        "impl SmeltDeepEq for Team {",
        "let (smelt_left, smelt_right) = (self, other);",
        "smelt_live_deep_eq(smelt_left.lead.clone(), smelt_right.lead.clone()) && (smelt_left.size == smelt_right.size || (smelt_left.size.is_nan() && smelt_right.size.is_nan()))",
        "SmeltDeepEq for Box_<T> where T: SmeltDeepEq {",
        "if ::std::rc::Rc::ptr_eq(&self.0, &other.0) { return true; }",
        "if let Some(smelt_answer) = smelt_seen.enter(::std::rc::Rc::as_ptr(&self.0).cast::<()>() as usize, ::std::rc::Rc::as_ptr(&other.0).cast::<()>() as usize) { return smelt_answer; }",
        "let smelt_equal = SmeltDeepEq::smelt_deep_eq(&smelt_left.value, &smelt_right.value, smelt_seen) && (smelt_left.label == smelt_right.label);",
        "smelt_seen.leave();",
        ".all(|(smelt_left_item, smelt_right_item)| smelt_deep_eq(&smelt_left_item, &smelt_right_item))",
    ] {
        assert!(body.contains(expected), "missing `{expected}`:\n{body}");
    }
    // The shared record is compared through its views where it is a field,
    // and has no impl of its own: nothing takes it as a type argument.
    assert!(!body.contains("impl SmeltDeepEq for Person"), "{body}");
    // Neither comparison erases an operand.
    for name in ["test_boxes", "test_teams"] {
        let test = rust_fn(&source, name);
        assert!(!test.contains("into_smelt_unknown"), "{test}");
        assert!(!test.contains("SmeltUnknown::Object"), "{test}");
    }
}

/// A generic record at a type argument the trait has no impl for keeps its
/// `PartialEq` at that use (a union here), and is not compared through the
/// trait there.
#[test]
fn a_type_argument_without_an_impl_keeps_the_handles_equality() {
    let source = source_for(&format!(
        "{VITEST}{GENERIC}\
         function mixed(first: number | string): Box<number | string>[] {{ const list: Box<number | string>[] = [{{ value: 0, label: 'a' }}]; fill(list[0], first); return list; }}\n\
         test('mixed', () => {{ const all = mixed(1); expect(all).toEqual(all); }});\n"
    ));
    let test = rust_fn(&source, "test_mixed");
    assert!(!test.contains("smelt_deep_eq"), "{test}");
    // The positive half: the matcher still compares, through `PartialEq`. A
    // `toEqual` dropped or folded to `true` also has no `smelt_deep_eq`.
    assert!(
        test.contains(" != "),
        "the handles' own equality decides: {test}"
    );
}

/// Rule: a callable record compares by identity, as JavaScript compares
/// functions: a by-value one by the function in its call slot.
#[test]
fn a_callable_record_compares_by_the_identity_of_its_function() {
    let ts = format!(
        "{VITEST}interface Counter {{ (step: number): number; count: number }}\n\
         function counter(count: number): Counter {{ const call = (step: number): number => step + count; call.count = count; return call; }}\n\
         function counters(count: number): Counter[] {{ return [counter(count)]; }}\n\
         test('same', () => {{ const all = counters(1); expect(all).toEqual(all); }});\n"
    );
    assert_eq!(planned_impls(&ts), vec!["Counter".to_owned()]);
    let source = source_for(&ts);
    let body = program_body(&source);
    assert!(body.contains("impl SmeltDeepEq for Counter {"), "{body}");
    assert!(
        body.contains("smelt_same_function_identity(&self.__smelt_call, &other.__smelt_call)")
            || body.contains("::std::ptr::addr_eq(::std::rc::Rc::as_ptr(&self.__smelt_call), ::std::rc::Rc::as_ptr(&other.__smelt_call))"),
        "{body}"
    );
    // The property the function carries is not what makes two of them equal.
    assert!(!body.contains("smelt_left.count"), "{body}");
}
