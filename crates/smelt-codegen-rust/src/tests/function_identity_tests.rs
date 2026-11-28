//! Function identity on records built from a function value
//! (`crate::function_identity`). Behavior against Node is the fixture
//! `smelt-transpiler/tests/fixtures/node-parity/function_identity_records.ts`.

use super::*;

/// Rule: a record a function value is converted into carries the function's
/// token, and `===` between the two compares identities instead of folding to
/// `false`.
#[test]
fn a_record_built_from_a_function_carries_its_identity() {
    let source = source_for(
        "interface Named { name: string; length: number }\n\
         function hello(a: number, b: number): number { return a + b; }\n\
         const n: Named = hello;\n\
         console.log(n === hello, n.name);\n",
    );
    assert!(
        source.contains("_smelt_function: SmeltFunctionToken,"),
        "{source}"
    );
    assert!(
        source.contains("_smelt_function: SmeltFunctionToken::of(&"),
        "{source}"
    );
    assert!(
        source.contains("smelt_left.is_some() && smelt_left == smelt_right"),
        "{source}"
    );
}

/// Rule (pay for use): a program that never converts a function into a
/// record emits no token, even for a record of the same shape.
#[test]
fn a_record_no_function_builds_carries_no_token() {
    let source = source_for(
        "interface Named { name: string; length: number }\n\
         const n: Named = { name: 'hello', length: 2 };\n\
         const m: Named = { name: 'other', length: 1 };\n\
         console.log(n === m, n.name);\n",
    );
    assert!(!source.contains("_smelt_function"), "{source}");
    assert!(!source.contains("SmeltFunctionToken"), "{source}");
}
