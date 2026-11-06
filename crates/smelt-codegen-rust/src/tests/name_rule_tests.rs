//! Lowering must not depend on what a source name happens to be spelled: each
//! test renders the same program under two names and compares the results.

use super::*;

/// `source` with `name` substituted for the `NAME` placeholder, the name itself
/// then folded back out of the output so only structure is compared.
fn structure_under(source: &str, name: &str) -> String {
    source_for(&source.replace("NAME", name)).replace(name, "NAME")
}

#[test]
fn a_loop_counter_is_seeded_the_same_whatever_it_is_called() {
    let source = r"
export function run(items: number[]): number {
    let total = 0;
    let NAME = 0;
    for (NAME = 0; NAME < items.length; NAME++) {
        total = total + items[NAME];
    }
    return total;
}
";
    assert_eq!(structure_under(source, "index_1"), structure_under(source, "cursor_1"));
}

#[test]
fn a_returned_callable_adapts_the_same_whatever_its_factory_is_called() {
    let source = r"
function NAME(): (x: number) => unknown {
    return (x: number) => x + 1;
}
export const add: (x: number) => number = NAME() as (x: number) => number;
";
    assert_eq!(structure_under(source, "piped"), structure_under(source, "factory"));
}

#[test]
fn a_method_named_like_an_array_builtin_is_still_a_method() {
    let source = r"
class Bag {
    items: number[] = [];
    NAME(other: number[]): number[] {
        return other;
    }
}
export function run(bag: Bag): number[] {
    return bag.NAME([1]);
}
";
    // The runtime prelude spells `concat` itself, so compare only the program.
    let program = |rendered: String| {
        let start = rendered.find("struct Bag").expect("the class is emitted");
        rendered[start..].to_owned()
    };
    assert_eq!(
        program(structure_under(source, "concat")),
        program(structure_under(source, "gather"))
    );
}
