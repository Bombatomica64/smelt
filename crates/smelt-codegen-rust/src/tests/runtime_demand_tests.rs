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
    RuntimeDemand::of(&mir)
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
    assert!(demand.has(Need::DepGenawaiter), "the state machine needs the crate");
    assert!(demand.has(Need::Unknown), "the runtime lives in the carrier's block");
    assert!(demand.has(Need::DepTokio), "the carrier block spells tokio types");
}

/// A program with nothing dynamic in it stays out of the carrier entirely.
#[test]
fn a_wholly_typed_program_demands_no_carrier() {
    let demand = demand_of("export function add(a: number, b: number): number { return a + b; }");
    assert!(!demand.has(Need::Unknown), "no erased value exists here");
    assert!(!demand.has(Need::SmeltList), "no list exists here");
    assert!(!demand.has(Need::ObjectIds), "no identity-bearing value exists here");
}

/// A `typeof` over a concrete value is decided statically, so it must not pull
/// the carrier into a typed program. This is why `Typeof` has no edge to `Unknown`.
#[test]
fn a_static_typeof_does_not_force_the_carrier() {
    let demand = demand_of("export function f(x: number): string { return typeof x; }");
    assert!(!demand.has(Need::Unknown), "a concrete operand needs no carrier");
}

/// The closure is transitive: a form reaches the blob runtime, the byte family
/// and the identity counter through its declared edges.
#[test]
fn the_requires_graph_closes_transitively() {
    let demand = demand_of("export function f(): FormData { return new FormData(); }");
    for need in [Need::FormData, Need::Blob, Need::ByteArray, Need::ObjectIds, Need::HostOrigins, Need::DepUrl] {
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
