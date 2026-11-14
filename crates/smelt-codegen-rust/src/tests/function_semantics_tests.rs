//! Codegen tests for JavaScript function-value semantics.
//!
//! Receivers, property bags, construction and forward reads are run in the
//! `function_value_semantics` Node-parity fixture. These tests pin what the
//! running program cannot show: the receiver channel is pay-for-use, and a
//! join or union read keeps its concrete type.

use super::*;

/// `this` is only observable through a read, so a program without one carries no
/// receiver channel, and a program with one does.
#[test]
fn a_program_without_this_carries_no_receiver_channel() {
    let without = source_for(
        r"
export function run(): unknown {
  const object = { m: (a: number) => a + 1 };
  return (object as any).m(1);
}
",
    );
    assert!(
        !without.contains("SMELT_THIS"),
        "a program that never mentions `this` must not carry the channel:\n{without}"
    );
    assert!(
        !without.contains("smelt_bind_this"),
        "an unobservable receiver bind must be dropped:\n{without}"
    );

    let with = source_for(
        r"
export function run(): unknown {
  const callee = function (this: any, a: number) { return (a + this.b) as number; };
  const object = { m: callee, b: 2 };
  return (object as any).m(1);
}
",
    );
    assert!(with.contains("SMELT_THIS"), "{with}");
    assert!(with.contains("smelt_bind_this"), "{with}");
}

/// `never` is the bottom type of a function join: the joined slot returns what
/// the other arm returns, and is fallible because the `never` arm throws.
#[test]
fn never_returning_arm_does_not_type_a_function_join() {
    let source = source_for(
        r"
class Failure {
  message: string
  constructor(message: string) {
    this.message = message
  }
}
const rethrow = (err: Failure): never => {
  throw err
}
const handle = (err: Failure): number => err.message.length
export function chooseHandler(strict: boolean): (err: Failure) => number {
  const handler = strict ? rethrow : (err: Failure) => handle(err)
  return handler
}
",
    );
    let handler = rust_fn(&source, "choose_handler");
    assert!(
        handler.starts_with(
            "fn choose_handler(strict: bool) -> ::std::rc::Rc<dyn Fn(Failure) -> f64> {\nlet handler: ::std::rc::Rc<dyn Fn(Failure) -> Result<f64, Box<dyn std::error::Error>>>;"
        ),
        "the joined handler must return `handle`'s number, fallibly:\n{handler}"
    );
}

/// A union property read answers per arm, typed, never through the erased path.
#[test]
fn union_property_read_is_typed_per_arm() {
    let source = source_for(
        r"
interface PlainInit {
  status?: number;
}
export function pickStatus(arg?: number | PlainInit | Response): number {
  return typeof arg === 'number' ? arg : (arg?.status ?? 200);
}
",
    );
    assert_eq!(
        rust_fn(&source, "pick_status"),
        r#"fn pick_status(arg: Option<SmeltUnion3>) -> f64 {
let mut _smelt_tmp_2: f64;
let _smelt_tmp_3: Option<f64>;
let _smelt_tmp_1: bool = arg.clone().as_ref().is_some_and(|smelt_value| matches!(smelt_value, SmeltUnion3::M0(_)));
let mut _smelt_tmp_2: f64 = 0.0;
if _smelt_tmp_1 {
_smelt_tmp_2 = arg.clone().clone().map_or(0.0, |smelt_value| match smelt_value { SmeltUnion3::M0(value) => value, _ => unreachable!("union guard selected an excluded member") });
} else {
_smelt_tmp_3 = arg.clone().as_ref().and_then(|_smelt_value| match &_smelt_value { SmeltUnion3::M0(_) => None, SmeltUnion3::M1(smelt_union_arm) => smelt_union_arm.status.clone(), SmeltUnion3::M2(smelt_union_arm) => Some(smelt_union_arm.status()) });
_smelt_tmp_2 = _smelt_tmp_3.clone().unwrap_or(200.0);
}
return _smelt_tmp_2;
}"#
    );
}
