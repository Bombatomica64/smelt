//! Tests for the runtime demand scan and its `requires` graph.
//!
//! Each test states a program whose emitted text names a runtime family, and
//! holds the demand to it. `source_for` fails on an undefined runtime symbol in
//! a debug build, so a missing edge fails these tests by name.

use super::*;

/// Returns the demand of TypeScript `source`.
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

/// A wholly typed generator has no erased value of its own, yet the generator
/// runtime lives in the carrier's block and its throw arm unwraps the panic route.
///
/// Without the `Generator -> Unknown` edge this program emits `SmeltGenerator`
/// against a prelude that never declares it.
#[test]
fn a_wholly_typed_generator_carries_the_generator_runtime() {
    let source = source_for(
        r"
function* g(): Generator<number, void, void> { yield 1; yield 2; }
function main(): void { for (const x of g()) { console.log(x); } }
main();
",
    );
    assert!(
        source.contains("struct SmeltGenerator<"),
        "a generator must define its runtime:\n{source}"
    );
}

/// The manifest and the prelude read the same demand, so a generator program
/// links `genawaiter` exactly when it emits the state machine.
#[test]
fn a_generator_demands_its_crate_and_the_carrier() {
    let demand = demand_of("function* g(): Generator<number, void, void> { yield 1; }");
    assert!(demand.has(Need::Generator), "the function is a generator");
    assert!(
        demand.has(Need::DepGenawaiter),
        "the state machine needs the crate"
    );
    assert!(
        demand.has(Need::Unknown),
        "the runtime lives in the carrier's block"
    );
    assert!(
        demand.has(Need::DepTokio),
        "the carrier block spells tokio types"
    );
}

/// A program with nothing dynamic in it stays out of the carrier entirely.
#[test]
fn a_wholly_typed_program_demands_no_carrier() {
    let demand = demand_of("export function add(a: number, b: number): number { return a + b; }");
    assert!(!demand.has(Need::Unknown), "no erased value exists here");
    assert!(!demand.has(Need::SmeltList), "no list exists here");
    assert!(
        !demand.has(Need::ObjectIds),
        "no identity-bearing value exists here"
    );
}

/// A `typeof` over a concrete value is decided statically, so it must not pull
/// the carrier into a typed program. This is why `Typeof` has no edge to `Unknown`.
#[test]
fn a_static_typeof_does_not_force_the_carrier() {
    let demand = demand_of("export function f(x: number): string { return typeof x; }");
    assert!(
        !demand.has(Need::Unknown),
        "a concrete operand needs no carrier"
    );
}

/// The closure is transitive: a form reaches the blob runtime, the byte family
/// and the identity counter through its declared edges.
#[test]
fn the_requires_graph_closes_transitively() {
    let demand = demand_of("export function f(): FormData { return new FormData(); }");
    for need in [
        Need::FormData,
        Need::Blob,
        Need::ByteArray,
        Need::ObjectIds,
        Need::HostOrigins,
        Need::DepUrl,
    ] {
        assert!(demand.has(need), "a form must reach {need:?}");
    }
}

/// A narrowing read of an optional at an erased type parameter calls the helper
/// `Option<SmeltUnknown>` reads share, even though `unknown` itself is absent
/// from the type table.
#[test]
fn an_erased_type_parameter_read_defines_its_optional_helper() {
    let source = source_for(
        r"
export function orDefault<TDefault = unknown>(value: any, defaultValue?: TDefault): TDefault {
  if (value === null) return defaultValue as TDefault
  return value
}
",
    );
    assert!(
        source.contains(&format!("fn {}(", crate::OPTIONAL_UNKNOWN_OR_UNDEFINED_FN)),
        "the read must find its helper defined:\n{source}"
    );
}

/// Every need, closed over the `requires` graph ON ITS OWN, renders a runtime
/// whose every emitter-prefixed name is defined.
///
/// A program's demand is a union of needs, and the scan is free to predict any
/// of them alone (a late erasure can add `Unknown` to a program that had
/// nothing else). So each need's closure has to be self-contained: an item
/// that names a family it did not declare an edge to shows up here, by need,
/// instead of as an E0425 in the one program that happens to demand it alone.
/// The program facts a flag can also ask are checked both off and on.
///
/// The `util.inspect` items any `console.log` splices in are rendered too. When
/// any TypeScript function demanded `JsString`, an item written against it
/// behind another flag (the inspect core's string entries) never showed.
#[test]
fn every_need_closes_to_a_self_contained_runtime() {
    use crate::runtime_prelude::{ProgramFacts, flag_for, rendered_entries, rendered_text};
    let all_facts = ProgramFacts {
        reference_classes: true,
        erased_view_write_through: true,
        reads_this: true,
        live_records: true,
    };
    let mut failures = Vec::new();
    for need in Need::ALL {
        for facts in [ProgramFacts::default(), all_facts] {
            let demand = RuntimeDemand::closed_over(&[*need]);
            let flags = |name: &str| flag_for(&demand, facts, name).unwrap_or(false);
            // The `util.inspect` items are spliced in by any `console.log` of
            // a typed value, whatever else the program demands.
            let mut text = rendered_text(&flags);
            text.push_str(&rendered_entries(
                smelt_runtime::order::CONSOLE_INSPECT,
                &flags,
            ));
            let undefined = runtime_symbols_check::undefined_runtime_symbols([text.as_str()]);
            if !undefined.is_empty() {
                failures.push(format!("{need:?} with {facts:?}: {}", undefined.join(", ")));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "needs whose closure names undefined runtime symbols:\n{}",
        failures.join("\n")
    );
}

/// `Need::ALL` holds every need once, at its own discriminant: the bit a need
/// sets in the demand is its position, so two needs can never share one.
#[test]
fn every_need_has_its_own_bit() {
    for (index, need) in Need::ALL.iter().enumerate() {
        let bit = RuntimeDemand::closed_over(&[]);
        assert!(!bit.has(*need), "an empty demand holds nothing");
        let alone = Need::ALL.iter().position(|other| other == need);
        assert_eq!(alone, Some(index), "{need:?} is listed twice in Need::ALL");
    }
    // Distinct needs set distinct bits: each need alone demands no other.
    for need in Need::ALL {
        let mut only = RuntimeDemand::default();
        only.add_for_test(*need);
        let shared = Need::ALL.iter().filter(|other| only.has(**other)).count();
        assert_eq!(shared, 1, "{need:?} shares its bit with another need");
    }
}

/// A runtime use the scan did not predict is refused at the site that asks for
/// it, naming the family, instead of reaching the finished crate.
#[test]
fn an_unpredicted_runtime_use_is_refused_by_name() {
    let typed = demand_of("export function add(a: number, b: number): number { return a + b; }");
    let refused = typed
        .carrier()
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(refused.contains("`Unknown` runtime family"), "{refused}");
    assert!(
        RuntimeDemand::closed_over(&[Need::Unknown])
            .carrier()
            .is_ok()
    );
}

/// A deep comparison of reference records reads them through their views
/// (`smelt_live_deep_eq`), so a program with no erased value of its own still
/// carries the erased runtime; the same program without the comparison, and
/// the same comparison of records nobody shares, do not.
#[test]
fn a_deep_comparison_of_reference_records_carries_the_erased_runtime() {
    let shared = r"
import { test, expect } from 'vitest';
interface Person { name: string; age: number }
function birthday(person: Person): Person { person.age += 1; return person; }
function people(): Person[] { const list: Person[] = [{ name: 'a', age: 1 }]; birthday(list[0]); return list; }
";
    let compared =
        format!("{shared}test('same', () => {{ expect(people()).toEqual(people()); }});\n");
    assert!(demand_of(&compared).has(Need::Unknown), "compared");
    let uncompared =
        format!("{shared}test('count', () => {{ expect(people().length).toEqual(1); }});\n");
    assert!(!demand_of(&uncompared).has(Need::Unknown), "not compared");
    let values = r"
import { test, expect } from 'vitest';
interface Point { x: number }
function points(): Point[] { return [{ x: 1 }]; }
test('same', () => { expect(points()).toEqual(points()); });
";
    assert!(!demand_of(values).has(Need::Unknown), "value records");
}

/// A generic class whose bodies never convert a `T` does not pull the erased
/// carrier into a program that has no other erased value: its parameters are
/// bounded `Clone + Default + 'static` (`classes::generated_type_param_bounds`).
///
/// Before, every generic item demanded the carrier because its bounds named
/// `IntoSmeltUnknown + SmeltFromUnknown`, and the program carried the whole
/// runtime for nothing.
#[test]
fn a_generic_item_that_converts_nothing_carries_no_carrier() {
    let source = source_for(
        r"
class Box<T> {
  value: T;
  constructor(value: T) { this.value = value; }
  get(): T { return this.value; }
}
function first<T>(items: T[]): T { return items[0]; }
const box = new Box<number>(4);
console.log(box.get() + first([1, 2]));
",
    );
    assert!(
        !source.contains("enum SmeltUnknown"),
        "no erased value, no carrier:\n{source}"
    );
    assert!(
        !runtime_symbols_check::names_the_carrier([source.as_str()]),
        "a carrier-free crate names no part of the carrier:\n{source}"
    );
    assert!(
        source.contains("T: Clone + Default + 'static"),
        "the generated bounds are the carrier-free set:\n{source}"
    );
}

/// A generic body that DOES convert a `T` fails the carrier-free speculation,
/// and the program is emitted with the carrier and the full bound set, as
/// before. Two ways to fail it: a conversion the emitter writes through the
/// carrier proof (`console.log` of a `T`) errors in the speculative emission;
/// a `typeof` of a `T` emits without error but names the carrier, which only
/// the check of the emitted text (`EmittedRoot::is_carrier_free`) sees.
#[test]
fn a_generic_item_that_converts_its_parameter_keeps_the_carrier() {
    for program in [
        "function show<T>(value: T): void { console.log(value); }\nshow<number>(1);\n",
        "function kind<T>(value: T): string { return typeof value; }\nconsole.log(kind<number>(1));\n",
    ] {
        let source = source_for(program);
        assert!(
            source.contains("enum SmeltUnknown"),
            "a converted `T` needs the carrier:\n{source}"
        );
    }
}
