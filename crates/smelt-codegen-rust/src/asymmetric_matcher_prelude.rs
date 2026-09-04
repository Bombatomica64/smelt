//! Runtime prelude for vitest *asymmetric matchers*.
//!
//! An asymmetric matcher — `expect.any(Number)`, `expect.arrayContaining([..])`,
//! `expect.objectContaining({..})`, `expect.stringContaining("x")`,
//! `expect.stringMatching(/x/)`, `expect.closeTo(n, p)`, `expect.anything()`,
//! and every one of those under `expect.not` — is a **value**, not an
//! assertion. Vitest types these factories `any`; what makes the value a
//! matcher is that the deep equality behind `toEqual` / `toStrictEqual` /
//! `toHaveBeenCalledWith` *asks* it whether the actual value matches, instead
//! of comparing it structurally. So a matcher can be stored in a variable,
//! nested inside an expected object or array literal, or handed over as one
//! argument of `toHaveBeenCalledWith`.
//!
//! The frontend lowers one to a branded record — `{ __smelt_asymmetric:
//! <kind>, sample: [<factory arguments>], inverted: <bool> }` — using the same
//! convention as `__smelt_vitest_mock`, `__smelt_map` and `__smelt_set`: the
//! value's kind travels with it, so a runtime helper recognizes it wherever it
//! ended up. This module emits the two halves that give the record meaning:
//!
//! * [`MATCH_FN`] — the per-kind predicate.
//! * [`EQUALS_FN`] — the deep-equality walk that consults the brand on **either**
//!   side, at **every** level, which is what makes a nested matcher work.
//!
//! # Why this is not `PartialEq for SmeltUnknown`
//!
//! `smelt_unknown_structural_eq` backs `PartialEq for SmeltUnknown`, and
//! transpiled library code observes that operator — es-toolkit's own `isEqual`
//! among other things. Teaching it about `__smelt_asymmetric` would let a
//! test-harness marker change what a transpiled predicate answers about an
//! ordinary object. The walk here is therefore a separate function, and when
//! neither operand holds a matcher anywhere it delegates to the structural one
//! unchanged, so an ordinary `toEqual` compares exactly as it did before.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]

use crate::rust::CodeWriter;

/// Reads the brand, factory arguments and inversion off a matcher record.
const MARKER_FN: &str = r#"fn smelt_asymmetric_marker(value: &SmeltUnknown) -> Option<(String, Vec<SmeltUnknown>, bool)> { let SmeltUnknown::Object(object) = value else { return None; }; let Some(SmeltUnknown::String(kind)) = object.get("__smelt_asymmetric") else { return None; }; let sample = match object.get("sample") { Some(SmeltUnknown::Array(values)) => values.into_vec(), _ => Vec::new() }; let inverted = matches!(object.get("inverted"), Some(SmeltUnknown::Bool(true))); Some((kind.to_string(), sample, inverted)) }"#;

/// Whether a value, or anything nested in it, is a matcher.
const PRESENT_FN: &str = r"fn smelt_asymmetric_present(value: &SmeltUnknown, depth: usize) -> bool { if depth == 0 { return false; } if smelt_asymmetric_marker(value).is_some() { return true; } match value { SmeltUnknown::Array(values) => values.iter().any(|item| smelt_asymmetric_present(&item, depth.saturating_sub(1))), SmeltUnknown::Object(map) => map.iter().any(|(_, item)| smelt_asymmetric_present(&item, depth.saturating_sub(1))), _ => false } }";

/// The constructor name an `expect.any(..)` argument denotes.
const CONSTRUCTOR_NAME_FN: &str = r#"fn smelt_asymmetric_constructor_name(value: &SmeltUnknown) -> Option<String> { match value { SmeltUnknown::String(name) => Some(name.to_string()), SmeltUnknown::Object(map) => match map.get("name") { Some(SmeltUnknown::String(name)) => Some(name.to_string()), _ => match map.get("__smelt_class") { Some(SmeltUnknown::String(name)) => Some(name.to_string()), _ => None } }, _ => None } }"#;

/// `expect.any(Ctor)`: whether a value is of the constructor's runtime kind.
const ANY_FN: &str = r#"fn smelt_asymmetric_any(actual: &SmeltUnknown, constructor: Option<&SmeltUnknown>) -> bool { let Some(name) = constructor.and_then(smelt_asymmetric_constructor_name) else { return false; }; match name.as_str() { "Number" => matches!(actual, SmeltUnknown::Number(_)), "String" => matches!(actual, SmeltUnknown::String(_)), "Boolean" => matches!(actual, SmeltUnknown::Bool(_)), "Symbol" => matches!(actual, SmeltUnknown::Symbol(_)), "Function" => matches!(actual, SmeltUnknown::Function(_)), "Array" => matches!(actual, SmeltUnknown::Array(_)), "Object" => matches!(actual, SmeltUnknown::Object(_) | SmeltUnknown::Array(_)), "Promise" => matches!(actual, SmeltUnknown::Promise(_)), other => smelt_object_to_string_tag(actual) == format!("[object {other}]") || matches!(actual, SmeltUnknown::Object(map) if matches!(map.get("__smelt_class"), Some(SmeltUnknown::String(class)) if &*class == other) || matches!(map.get("__smelt_error"), Some(SmeltUnknown::String(class)) if &*class == other)) } }"#;

/// The per-kind matcher predicate, one emitted line per source line.
const MATCH_FN: &[&str] = &[
    r"fn smelt_asymmetric_match(actual: &SmeltUnknown, kind: &str, sample: &[SmeltUnknown], seen: &mut ::std::collections::HashSet<(usize, usize)>) -> bool {",
    r"    match kind {",
    r#"        "anything" => !matches!(actual, SmeltUnknown::Null | SmeltUnknown::Undefined),"#,
    r#"        "any" => smelt_asymmetric_any(actual, sample.first()),"#,
    r#"        "arrayContaining" => { let SmeltUnknown::Array(values) = actual else { return false; }; let items = values.clone().into_vec(); let wanted = match sample.first() { Some(SmeltUnknown::Array(expected)) => expected.clone().into_vec(), _ => Vec::new() }; wanted.iter().all(|want| items.iter().any(|have| smelt_vitest_asymmetric_equals(have, want, seen))) }"#,
    r#"        "objectContaining" => { let SmeltUnknown::Object(map) = actual else { return false; }; let Some(SmeltUnknown::Object(expected)) = sample.first() else { return false; }; expected.iter().all(|(key, want)| map.get(&key).is_some_and(|have| smelt_vitest_asymmetric_equals(&have, &want, seen))) }"#,
    r#"        "stringContaining" => { let (SmeltUnknown::String(text), Some(SmeltUnknown::String(part))) = (actual, sample.first()) else { return false; }; text.contains(&**part) }"#,
    r#"        "stringMatching" => { let SmeltUnknown::String(text) = actual else { return false; }; match sample.first() { Some(SmeltUnknown::String(pattern)) => SmeltRegExp::new(pattern.to_string(), String::new()).test(text), Some(SmeltUnknown::Object(map)) if map.contains_key("__smelt_regexp") => { let source = match map.get("source") { Some(SmeltUnknown::String(source)) => source.to_string(), _ => String::new() }; let flags = match map.get("flags") { Some(SmeltUnknown::String(flags)) => flags.to_string(), _ => String::new() }; SmeltRegExp::new(source, flags).test(text) } _ => false } }"#,
    r#"        "closeTo" => { let (SmeltUnknown::Number(value), Some(SmeltUnknown::Number(target))) = (actual, sample.first()) else { return false; }; let precision = match sample.get(1) { Some(SmeltUnknown::Number(precision)) => *precision, _ => 2.0 }; (value - target).abs() < 10f64.powf(-precision) / 2.0 }"#,
    r"        _ => false,",
    r"    }",
    r"}",
];

/// The matcher-aware deep-equality walk, one emitted line per source line.
const EQUALS_FN: &[&str] = &[
    r"fn smelt_vitest_asymmetric_equals(left: &SmeltUnknown, right: &SmeltUnknown, seen: &mut ::std::collections::HashSet<(usize, usize)>) -> bool {",
    r"    if let Some((kind, sample, inverted)) = smelt_asymmetric_marker(right) { return smelt_asymmetric_match(left, &kind, &sample, seen) != inverted; }",
    r"    if let Some((kind, sample, inverted)) = smelt_asymmetric_marker(left) { return smelt_asymmetric_match(right, &kind, &sample, seen) != inverted; }",
    r"    if !smelt_asymmetric_present(left, 32) && !smelt_asymmetric_present(right, 32) { return smelt_unknown_structural_eq(left, right, seen); }",
    r"    match (left, right) {",
    r"        (SmeltUnknown::Array(left), SmeltUnknown::Array(right)) => left.len() == right.len() && left.iter().zip(right.iter()).all(|(left, right)| smelt_vitest_asymmetric_equals(&left, &right, seen)),",
    r"        (SmeltUnknown::Object(left), SmeltUnknown::Object(right)) => { let left_keys = smelt_own_object_keys(left).len(); let right_keys = smelt_own_object_keys(right).len(); left_keys == right_keys && left.iter().filter(|(key, _)| smelt_is_for_in_object_key(left, key)).all(|(key, value)| right.get(&key).is_some_and(|other| smelt_vitest_asymmetric_equals(&value, &other, seen))) }",
    r"        _ => smelt_unknown_structural_eq(left, right, seen),",
    r"    }",
    r"}",
];

/// Emits the whole asymmetric-matcher runtime into the generated prelude.
///
/// Only called from the vitest-mock region of the prelude: every helper here is
/// written in terms of `SmeltUnknown`, `SmeltRegExp` and the own-key view, all
/// of which a program carrying an erased test value already emits.
pub(crate) fn emit(writer: &mut CodeWriter) {
    writer.blank_line();
    writer.line("/// Read the brand, arguments and inversion off an asymmetric-matcher record.");
    writer.line(MARKER_FN);
    writer.blank_line();
    writer.line("/// Whether a value, or anything nested in it, is an asymmetric matcher.");
    writer.line("///");
    writer.line("/// The depth bound both terminates on a cyclic value and keeps the check");
    writer.line("/// cheap: a matcher is written literally in the assertion, so it is never");
    writer.line("/// nested deeper than the expected literal it sits in.");
    writer.line(PRESENT_FN);
    writer.blank_line();
    writer.line("/// The constructor NAME an `expect.any(..)` argument denotes.");
    writer.line(CONSTRUCTOR_NAME_FN);
    writer.blank_line();
    writer.line("/// `expect.any(Ctor)`: whether a value is of the constructor's runtime kind.");
    writer.line("///");
    writer.line("/// The primitive wrappers name a tag; every other constructor is matched by");
    writer.line("/// the value's own class identity, through the same view");
    writer.line("/// `Object.prototype.toString` reports.");
    writer.line(ANY_FN);
    writer.blank_line();
    writer.line("/// Whether `actual` satisfies one asymmetric matcher.");
    for line in MATCH_FN {
        writer.line(*line);
    }
    writer.blank_line();
    writer.line("/// Vitest deep equality, consulting an asymmetric matcher on either side.");
    writer.line("///");
    writer.line("/// With no matcher anywhere in either operand this is exactly");
    writer.line("/// `smelt_unknown_structural_eq`, which backs `PartialEq for SmeltUnknown`");
    writer.line("/// and must stay blind to the harness's markers. With one, the walk recurses");
    writer.line("/// itself, so a matcher NESTED in an expected array or object is asked about");
    writer.line("/// the corresponding part of the actual value.");
    for line in EQUALS_FN {
        writer.line(*line);
    }
}
