//! Element reads on a generated union stay on the union's arms.
//!
//! A value typed as a union of tuples, lists or records is a generated
//! `SmeltUnion…` enum. Reading `value[i]` or `value.length` used to erase it
//! (`into_smelt_unknown`), read the erased array dynamically and narrow the
//! answer back; these tests pin the typed per-arm reads that replaced that, and
//! the `Optional` read type of a tuple element some arm does not have.

use super::{emitted_function_body, source_for};

/// The router-result shape: two tuple arms of different arity.
const OUTCOME: &str = r#"
type Indexed = Record<string, number>;
type Named = Record<string, string>;
type Outcome = [[string, Indexed][], string[]] | [[string, Named][]];
export function describe(outcome: Outcome): string {
  const handlers = outcome[0];
  const head = handlers[0];
  const raw = head[1]['id'];
  const direct = outcome[1] ? outcome[1][raw as number] : (raw as string);
  return `${handlers.length} ${head[0]} ${direct}`;
}
"#;

#[test]
fn tuple_and_list_element_reads_project_each_arm() {
    let source = source_for(OUTCOME);
    let body = emitted_function_body(&source, "fn describe(");
    // `outcome[0]`: both tuple arms hold element 0, each at its own list type.
    assert!(
        body.contains("::M0(smelt_union_arm.0.clone()), ")
            && body.contains("::M1(smelt_union_arm.0.clone()) }"),
        "outcome[0] should read element 0 of each tuple arm: {body}"
    );
    // `handlers[0]` and `handlers.length` read each list arm directly.
    assert!(body.contains("smelt_union_arm.borrow().get("), "{body}");
    assert!(body.contains("(smelt_union_arm.len() as f64)"), "{body}");
    // `head[1]['id']`: each record arm is read by key.
    assert!(body.contains("smelt_union_arm.get(&"), "{body}");
    // None of these reads goes through the erased carrier.
    assert!(
        !body.contains("into_smelt_unknown()"),
        "an element read erased the union: {body}"
    );
}

#[test]
fn tuple_element_missing_from_an_arm_reads_optional() {
    let source = source_for(OUTCOME);
    let body = emitted_function_body(&source, "fn describe(");
    // `outcome[1]` exists on the first arm only: the second arm answers
    // `None`, so the read is `Option<..>` and the truthiness test is a real
    // branch. Typed as the first arm's `string[]`, the test folded to `true`
    // and the one-element arm read the stash it does not have.
    assert!(
        body.contains("::M1(smelt_union_arm) => None"),
        "the arm without element 1 should answer None: {body}"
    );
    assert!(!body.contains("if true"), "the stash test folded: {body}");
}
