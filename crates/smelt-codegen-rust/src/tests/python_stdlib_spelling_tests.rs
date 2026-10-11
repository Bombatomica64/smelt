//! Emitted Rust for every import spelling of the standard-library names the
//! Python frontend models: `dataclasses.dataclass`, `enum.IntEnum`,
//! `typing.Generic`, and `abc.ABC` with `abc.abstractmethod`.
//!
//! The frontend resolves these names through the module's imports, so the
//! spelling must not matter: each program is lowered once per spelling (dotted,
//! aliased module, from-import, from-import alias), the generated Rust must be
//! identical for all four, and it must carry the concrete types the source
//! declares.

use super::*;

/// The four import spellings of `names` from `module`, as the import line and
/// the expression each name is spelled with afterwards.
fn spellings(module: &str, names: &[&str]) -> Vec<(String, Vec<String>)> {
    let aliases: Vec<String> = (0..names.len()).map(|index| format!("Alias{index}")).collect();
    let alias_imports: Vec<String> = names
        .iter()
        .zip(&aliases)
        .map(|(name, alias)| format!("{name} as {alias}"))
        .collect();
    vec![
        (
            format!("import {module}"),
            names.iter().map(|name| format!("{module}.{name}")).collect(),
        ),
        (
            format!("import {module} as m"),
            names.iter().map(|name| format!("m.{name}")).collect(),
        ),
        (
            format!("from {module} import {}", names.join(", ")),
            names.iter().map(|name| (*name).to_owned()).collect(),
        ),
        (
            format!("from {module} import {}", alias_imports.join(", ")),
            aliases,
        ),
    ]
}

/// Emit `program` once per spelling of `names` and assert every spelling
/// produces the same Rust; returns that Rust.
fn emitted_for_every_spelling(
    module: &str,
    names: &[&str],
    program: impl Fn(&[String]) -> String,
) -> String {
    let mut reference: Option<(String, String)> = None;
    for (import, spelled) in spellings(module, names) {
        let source = source_for_py(&format!("{import}\n{}", program(&spelled)));
        match &reference {
            Some((reference_import, reference_source)) => assert_eq!(
                &source, reference_source,
                "`{import}` emits different Rust than `{reference_import}`"
            ),
            None => reference = Some((import, source)),
        }
    }
    reference.map(|(_, source)| source).unwrap_or_default()
}

#[test]
fn dataclass_emits_the_same_struct_for_every_spelling() {
    for frozen in ["", "(frozen=True)"] {
        let source = emitted_for_every_spelling("dataclasses", &["dataclass"], |names| {
            let [dataclass] = names else {
                return String::new();
            };
            format!(
                "
@{dataclass}{frozen}
class Point:
    x: int
    y: str


def same() -> bool:
    return Point(1, \"a\") == Point(1, \"a\")
",
            )
        });
        assert!(
            source.contains("struct Point {\n    x: i64,\n    y: String,\n}"),
            "{source}"
        );
        assert!(
            emitted_function_body(&source, "fn same() -> bool").contains("Point::new(1, "),
            "{source}"
        );
    }
}

#[test]
fn int_enum_members_emit_as_integers_for_every_spelling() {
    let source = emitted_for_every_spelling("enum", &["IntEnum"], |names| {
        let [int_enum] = names else {
            return String::new();
        };
        format!(
            "
class Color({int_enum}):
    RED = 1
    GREEN = 2


def next_after_green() -> int:
    return Color.GREEN + 1
",
        )
    });
    let body = emitted_function_body(&source, "fn next_after_green() -> i64");
    assert!(body.contains("2 + 1"), "{source}");
}

/// Only spelling-independence is asserted here. The class's type parameter
/// reaches HIR for every spelling (see the frontend's
/// `generic_marker_lowers_type_params_for_every_import_spelling`), but Rust
/// emission currently erases the field to `SmeltUnknown` and drops `<T>` for
/// every spelling alike, including the plain from-import. Pinning that erased
/// shape would make fixing it look like a regression.
#[test]
fn generic_marker_emits_the_same_class_for_every_spelling() {
    let source = emitted_for_every_spelling("typing", &["Generic"], |names| {
        let [generic] = names else {
            return String::new();
        };
        format!(
            "from typing import TypeVar

T = TypeVar(\"T\")


class Box({generic}[T]):
    value: T

    def __init__(self, value: T) -> None:
        self.value = value


def unbox(b: Box[int]) -> int:
    return b.value
",
        )
    });
    assert!(source.contains("struct Box"), "{source}");
    assert!(source.contains("fn unbox(b: Box"), "{source}");
}

#[test]
fn abstract_base_emits_the_same_classes_for_every_spelling() {
    let source = emitted_for_every_spelling("abc", &["ABC", "abstractmethod"], |names| {
        let [abc, abstractmethod] = names else {
            return String::new();
        };
        format!(
            "
class Shape({abc}):
    @{abstractmethod}
    def area(self) -> float:
        pass


class Square(Shape):
    def __init__(self, side: float) -> None:
        self.side = side

    def area(self) -> float:
        return self.side * self.side


def square_area() -> float:
    return Square(2.0).area()
",
        )
    });
    assert!(source.contains("fn area(&self) -> f64"), "{source}");
    assert!(source.contains("fn square_area() -> f64"), "{source}");
}
