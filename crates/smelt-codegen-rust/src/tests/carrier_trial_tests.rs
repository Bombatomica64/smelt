//! The generic trial: a body keeps its type parameters unless the render touched
//! the erased carrier outside a real dynamic boundary.

use super::*;

#[test]
fn a_thrown_payload_does_not_cost_a_function_its_generics() {
    let source = source_for(
        r#"
export function firstOf<T>(items: T[], size: number): T[] {
    if (size <= 0) {
        throw new Error("size must be positive");
    }
    return items;
}
"#,
    );
    assert!(source.contains("fn first_of<T"), "{source}");
    assert!(source.contains("items: SmeltList<T>"), "{source}");
}

#[test]
fn inspecting_a_type_parameter_still_demotes_the_function() {
    let source = source_for(
        r"
export function isAbsent<T>(value: T): boolean {
    return value === null;
}
",
    );
    assert!(!source.contains("fn is_absent<T"), "{source}");
}

#[test]
fn a_pure_passthrough_keeps_its_generics() {
    let source = source_for(
        r"
export function identity<T>(value: T): T {
    return value;
}
",
    );
    assert!(source.contains("fn identity<T"), "{source}");
}
