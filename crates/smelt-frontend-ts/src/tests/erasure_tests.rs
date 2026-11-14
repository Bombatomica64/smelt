//! Erasure reasons recorded at the lowering decision.
//!
//! Design adapted from Bun's `--aot` (oven-sh/bun#45631),
//! `src/sema/aot_rules.rs`. Each test lowers a small valid TypeScript program
//! and pins the reason and the 1-based `line:col` of the record the deciding
//! site appends to `Crate::erasures`, so a site that drops its record, or
//! records the wrong reason or position, fails here.

use super::*;
use smelt_hir::ErasureReason;

/// Every record of a lowering, as `(reason, line, col, detail)`.
fn records(source: &str) -> Result<Vec<(ErasureReason, u32, u32, String)>, String> {
    let mut ctx = HirCtx::new();
    lower_ok(source, &mut ctx)?;
    Ok(ctx
        .krate
        .erasures
        .iter()
        .map(|record| {
            let (line, col) = smelt_hir::line_col(source, record.span.start).unwrap_or((0, 0));
            (record.reason, line, col, record.detail.clone())
        })
        .collect())
}

/// Assert that lowering `source` records `reason` at `line:col`.
fn assert_recorded(source: &str, reason: ErasureReason, line: u32, col: u32) -> Result<(), String> {
    let found = records(source)?;
    ensure!(
        found
            .iter()
            .any(|(at_reason, at_line, at_col, _)| *at_reason == reason
                && *at_line == line
                && *at_col == col),
        "expected {reason:?} at {line}:{col}, recorded {found:?}"
    );
    Ok(())
}

/// Assert that lowering `source` records nothing for `reason`.
fn assert_not_recorded(source: &str, reason: ErasureReason) -> Result<(), String> {
    let found = records(source)?;
    ensure!(
        found.iter().all(|(at_reason, ..)| *at_reason != reason),
        "expected no {reason:?}, recorded {found:?}"
    );
    Ok(())
}

/// `any` and `unknown` annotations are distinct reasons, at the annotation.
#[test]
fn any_and_unknown_annotations_are_split() -> Result<(), String> {
    let source = ts!("function f(a: any, b: unknown): void {\n  console.log(a, b);\n}\nf(1, 2);\n");
    assert_recorded(source, ErasureReason::ExplicitAny, 1, 15)?;
    assert_recorded(source, ErasureReason::SourceUnknown, 1, 23)
}

/// `object` is an open layout, not an opt-out.
#[test]
fn object_keyword_is_open_layout() -> Result<(), String> {
    let source = ts!("function f(a: object): void {\n  console.log(a);\n}\nf({});\n");
    assert_recorded(source, ErasureReason::OpenLayout, 1, 15)?;
    assert_not_recorded(source, ErasureReason::ExplicitAny)
}

/// An index signature over `unknown` values is an open layout.
#[test]
fn unknown_index_signature_is_open_layout() -> Result<(), String> {
    let source = ts!("function f(a: {}): void {\n  console.log(a);\n}\nf({});\n");
    assert_recorded(source, ErasureReason::OpenLayout, 1, 15)
}

/// A type literal whose every member `Object.prototype` supplies
/// (`{ valueOf: () => number }`) states no layout: every object satisfies it,
/// a `Date` included, so it is the open layout `object` is. Lowered as a
/// record it rebuilt a `Date` from its own entries and lost the date (remeda
/// `Comparable`, `firstBy`/`sortBy` over dates compared `NaN`s). A literal
/// with any other member keeps its record shape.
#[test]
fn object_prototype_surface_is_open_layout() -> Result<(), String> {
    let source = ts!(
        "function f(a: { valueOf: () => number }): void {\n  console.log(a);\n}\nf(new Date());\n"
    );
    assert_recorded(source, ErasureReason::OpenLayout, 1, 15)?;
    let typed = ts!(
        "function g(a: { valueOf(): number; label: string }): void {\n  console.log(a);\n}\ng({ valueOf: () => 1, label: 'x' });\n"
    );
    assert_not_recorded(typed, ErasureReason::OpenLayout)
}

/// An empty array literal without an annotation is a missing annotation.
#[test]
fn empty_array_without_annotation_is_missing_annotation() -> Result<(), String> {
    let source = ts!("const xs = [];\nconsole.log(xs.length);\n");
    assert_recorded(source, ErasureReason::MissingAnnotation, 1, 12)
}

/// Annotating the same array removes the record: the decision is not made.
#[test]
fn annotated_array_records_nothing() -> Result<(), String> {
    let source = ts!("const xs: number[] = [];\nconsole.log(xs.length);\n");
    let found = records(source)?;
    ensure!(found.is_empty(), "expected no erasure, recorded {found:?}");
    Ok(())
}

/// `JSON.parse` without a typed target is external data.
#[test]
fn untyped_json_parse_is_external_type() -> Result<(), String> {
    let source = ts!("const data = JSON.parse('{\"a\":1}');\nconsole.log(data);\n");
    assert_recorded(source, ErasureReason::ExternalType, 1, 14)
}

/// A `symbol` annotation, a `Symbol()` call and a `PropertyKey` are typed:
/// none of them records an erasure.
///
/// Old requirement: `symbol` was a type Smelt did not model, recorded as
/// `UnmodeledType` at the annotation. `Type::Symbol` now carries it (and
/// `PropertyKey` is `string | number | symbol`), so the same program must
/// record nothing.
#[test]
fn symbol_keyword_is_a_modeled_type() -> Result<(), String> {
    let source = ts!(
        "function f(s: symbol, k: PropertyKey): void {\n  console.log(typeof s, typeof k);\n}\nf(Symbol('x'), Symbol.for('y'));\n"
    );
    let found = records(source)?;
    ensure!(
        found.is_empty(),
        "a symbol program must record no erasure, recorded {found:?}"
    );
    Ok(())
}

/// A conditional type is a type-level construct Smelt does not evaluate.
#[test]
fn conditional_type_is_type_level() -> Result<(), String> {
    let source = ts!(
        "type Pick1<T> = T extends string ? number : boolean;\nfunction f<T>(x: Pick1<T>): void {\n  console.log(x);\n}\nf<string>(1);\n"
    );
    let found = records(source)?;
    ensure!(
        found
            .iter()
            .any(|(reason, ..)| *reason == ErasureReason::TypeLevel),
        "expected a TypeLevel record, recorded {found:?}"
    );
    Ok(())
}

/// A timer id is a builtin result Smelt does not model yet.
#[test]
fn timer_id_is_untyped_builtin() -> Result<(), String> {
    let source = ts!("const t = setTimeout(() => {}, 1);\nconsole.log(t);\n");
    assert_recorded(source, ErasureReason::UntypedBuiltin, 1, 11)
}

/// `this` in a free function is resolved by name.
#[test]
fn free_function_this_is_by_name() -> Result<(), String> {
    let source = ts!("function g(this: any) { return this; }\nconsole.log(g);\n");
    assert_recorded(source, ErasureReason::ByName, 1, 32)
}

/// A mixed array literal widens its element type at the literal.
#[test]
fn mixed_array_literal_is_dynamic_operator() -> Result<(), String> {
    let source = ts!("const xs = [1, 'a'];\nconsole.log(xs);\n");
    assert_recorded(source, ErasureReason::DynamicOperator, 1, 12)
}

/// `new Map()` without type arguments or a hint is a missing annotation.
#[test]
fn bare_new_map_is_missing_annotation() -> Result<(), String> {
    let source = ts!("const m = new Map();\nconsole.log(m.size);\n");
    assert_recorded(source, ErasureReason::MissingAnnotation, 1, 11)
}

/// A `vi.fn()` mock is a test-harness value; its import is an untyped package.
#[test]
fn vitest_mock_is_test_harness() -> Result<(), String> {
    let source = ts!("import { vi } from 'vitest';\nconst f = vi.fn();\nconsole.log(f);\n");
    assert_recorded(source, ErasureReason::TestHarness, 2, 11)?;
    assert_recorded(source, ErasureReason::UntypedPackage, 1, 1)
}

/// `as any` erases at the cast's annotation.
#[test]
fn as_any_is_explicit_any() -> Result<(), String> {
    let source = ts!(
        "function f(x: number | string): void { const y = x as any; console.log(y); }\nf(1);\n"
    );
    assert_recorded(source, ErasureReason::ExplicitAny, 1, 55)
}

/// A join that drops a concrete arm's shape is a recorded decision (audit
/// finding A12): `c ? n : x` with `n: number` and `x: unknown` erases the
/// number arm, which used to widen without a record.
#[test]
fn a_join_that_erases_a_concrete_arm_is_recorded() -> Result<(), String> {
    let source = ts!(
        "function pick(c: boolean, n: number, x: unknown): void {\n  const v = c ? n : x;\n  console.log(v);\n}\npick(true, 1, 2);\n"
    );
    let found = records(source)?;
    ensure!(
        found.iter().any(
            |(reason, line, _, detail)| *reason == ErasureReason::DynamicOperator
                && *line == 2
                && detail == "join with an erased operand"
        ),
        "expected the join to record DynamicOperator on line 2, recorded {found:?}"
    );
    Ok(())
}

/// A join of arms that are all already erased drops nothing and records
/// nothing new.
#[test]
fn a_join_of_erased_arms_records_nothing_new() -> Result<(), String> {
    let source = ts!(
        "function pick(c: boolean, x: unknown, y: unknown): void {\n  const v = c ? x : y;\n  console.log(v);\n}\npick(true, 1, 2);\n"
    );
    assert_not_recorded(source, ErasureReason::DynamicOperator)
}

/// A discarded probe leaves no erasure record behind (audit finding A10).
///
/// An unannotated module constant is lowered twice: once in a probe that only
/// asks for its type (`infer_module_global_initializer_type`), then for real.
/// Only the real lowering's decision may remain: one record per site.
#[test]
fn a_discarded_probe_leaves_no_erasure_record() -> Result<(), String> {
    let source = ts!("const parsed = JSON.parse(\"{}\");\nconsole.log(parsed);\n");
    let found = records(source)?;
    let mut sites = found
        .iter()
        .map(|(reason, line, col, _)| (*reason, *line, *col))
        .collect::<Vec<_>>();
    let all = sites.len();
    sites.sort_unstable();
    sites.dedup();
    ensure!(!found.is_empty(), "the initializer must record its erasure");
    ensure_eq!(all, sites.len());
    Ok(())
}
