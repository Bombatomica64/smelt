//! End-to-end checks that tsrs type facts reach Smelt keyed by oxc spans.
//!
//! Each test states the TypeScript 7 type `tsc` reports for the probed node
//! (the `.types` baseline spelling) and asserts both that spelling and the
//! structured kind Smelt lowering will consume. The cases are the families the
//! hand-rolled frontend type lowering approximates or erases to
//! `Type::Unknown` today.

#![expect(
    clippy::panic_in_result_fn,
    reason = "tests assert on checker output and use `?` for setup failures"
)]

use oxc::allocator::Allocator;
use oxc::ast::ast::Expression;
use oxc::ast_visit::{Visit, walk};
use oxc::parser::Parser;
use oxc::span::{GetSpan, SourceType};
use smelt_ts_types::model::{TsTupleElementKind, TsTypeKind};
use smelt_ts_types::{SourceInput, TsType, TsTypeFacts, TsTypeId, analyze};

/// `(start, end)` byte span of a probed node.
type Span = (u32, u32);

/// `(start, end, label)` of a collected oxc expression.
type LabeledSpan = (u32, u32, String);

/// `(name, optional, readonly)` of an object property.
type PropertyShape<'facts> = (&'facts str, bool, bool);

/// File name every single-file test is analysed under.
const FILE: &str = "src/index.ts";

/// Analyses one source file and fails on any TypeScript error.
fn facts(source: &str) -> Result<TsTypeFacts, String> {
    let facts = analyze(vec![SourceInput::new(FILE, source)]).map_err(|error| error.to_string())?;
    let errors: Vec<String> =
        facts.errors().map(|diagnostic| format!("TS{}: {}", diagnostic.code, diagnostic.message)).collect();
    if errors.is_empty() { Ok(facts) } else { Err(errors.join("\n")) }
}

/// Byte span of the `nth` (0-based) occurrence of `needle` in `source`.
fn span_of(source: &str, needle: &str, nth: usize) -> Result<Span, String> {
    let offset = source
        .match_indices(needle)
        .nth(nth)
        .map(|(start, _)| start)
        .ok_or_else(|| format!("`{needle}` occurrence {nth} not found"))?;
    let start = u32::try_from(offset).map_err(|error| error.to_string())?;
    let len = u32::try_from(needle.len()).map_err(|error| error.to_string())?;
    Ok((start, start.saturating_add(len)))
}

/// The type of the `nth` occurrence of `needle`.
fn type_of<'facts>(
    facts: &'facts TsTypeFacts,
    source: &str,
    needle: &str,
    nth: usize,
) -> Result<&'facts TsType, String> {
    let (start, end) = span_of(source, needle, nth)?;
    facts.type_at(FILE, start, end).ok_or_else(|| format!("no type recorded for `{needle}` at {start}..{end}"))
}

/// The kind stored for a nested type id.
fn kind(facts: &TsTypeFacts, id: TsTypeId) -> Result<&TsTypeKind, String> {
    facts.types.get(id).map(|ty| &ty.kind).ok_or_else(|| format!("dangling type id {id:?}"))
}

#[test]
fn conditional_type_resolves_at_each_instantiation() -> Result<(), String> {
    let source = r#"
type IsStr<T> = T extends string ? "yes" : 0;
declare function probe<T>(x: T): IsStr<T>;
export const a = probe("s");
export const b = probe(1);
"#;
    let facts = facts(source)?;
    let yes = type_of(&facts, source, r#"probe("s")"#, 0)?;
    assert_eq!(yes.display, r#""yes""#, "string instantiation picks the true arm");
    assert_eq!(yes.kind, TsTypeKind::StringLiteral("yes".to_owned()), "true arm is a string literal");
    let zero = type_of(&facts, source, "probe(1)", 0)?;
    assert_eq!(zero.kind, TsTypeKind::NumberLiteral(0.0), "number instantiation picks the false arm");
    Ok(())
}

#[test]
fn callback_inference_instantiates_map_result() -> Result<(), String> {
    let source = "export const labels = [1, 2].map((n) => String(n));\n";
    let facts = facts(source)?;
    let mapped = type_of(&facts, source, "[1, 2].map((n) => String(n))", 0)?;
    assert_eq!(mapped.display, "string[]", "map infers U = string from the callback");
    let TsTypeKind::Array { element, readonly: false } = mapped.kind else {
        return Err(format!("expected a mutable array, got {:?}", mapped.kind));
    };
    assert_eq!(kind(&facts, element)?, &TsTypeKind::String, "element type is string");
    let parameter = type_of(&facts, source, "n", 1)?;
    assert_eq!(parameter.kind, TsTypeKind::Number, "callback parameter is contextually number");
    Ok(())
}

#[test]
fn typeof_guard_narrows_each_reference() -> Result<(), String> {
    let source = r#"
export function size(x: string | number): number {
  if (typeof x === "string") {
    return x.length;
  }
  return x;
}
"#;
    let facts = facts(source)?;
    assert_eq!(type_of(&facts, source, "x", 2)?.display, "string | number", "guard operand is the declared union");
    assert_eq!(type_of(&facts, source, "x", 3)?.kind, TsTypeKind::String, "true branch narrows to string");
    assert_eq!(type_of(&facts, source, "x", 4)?.kind, TsTypeKind::Number, "fallthrough narrows to number");
    Ok(())
}

#[test]
fn mapped_type_expands_to_resolved_members() -> Result<(), String> {
    let source = r"
interface Point { x: number; readonly label: string }
export const partial: Partial<Point> = {};
export const read = partial;
";
    let facts = facts(source)?;
    let ty = type_of(&facts, source, "partial", 1)?;
    assert_eq!(ty.display, "Partial<Point>", "alias spelling is preserved for display");
    let TsTypeKind::Object(object) = &ty.kind else {
        return Err(format!("expected a structural object, got {:?}", ty.kind));
    };
    let names: Vec<PropertyShape<'_>> =
        object.properties.iter().map(|property| (property.name.as_str(), property.optional, property.readonly)).collect();
    assert_eq!(names, [("x", true, false), ("label", true, true)], "Partial makes every member optional, keeps readonly");
    let x = object.properties.first().ok_or("missing x")?;
    assert_eq!(kind(&facts, x.ty)?, &TsTypeKind::Number, "optional member type excludes the added undefined");
    Ok(())
}

#[test]
fn generic_class_instantiation_is_nominal() -> Result<(), String> {
    let source = r"
export class Box<T> {
  constructor(public value: T) {}
}
export const boxed = new Box(1);
";
    let facts = facts(source)?;
    let ty = type_of(&facts, source, "new Box(1)", 0)?;
    assert_eq!(ty.display, "Box<number>", "constructor inference widens the literal");
    let TsTypeKind::Named { symbol, type_arguments } = &ty.kind else {
        return Err(format!("expected a nominal class reference, got {:?}", ty.kind));
    };
    assert_eq!(symbol.name, "Box", "class identity is the declaring symbol");
    assert!(symbol.declaration.as_ref().is_some_and(|location| location.file.ends_with(FILE)), "declared in the input");
    let arguments: Vec<&TsTypeKind> = type_arguments.iter().map(|&id| kind(&facts, id)).collect::<Result<_, _>>()?;
    assert_eq!(arguments, [&TsTypeKind::Number], "exactly one type argument, no implicit `this`");
    Ok(())
}

#[test]
fn const_tuple_keeps_literal_elements() -> Result<(), String> {
    let source = "export const pair = [1, \"a\"] as const;\nexport const copy = pair;\n";
    let facts = facts(source)?;
    let ty = type_of(&facts, source, "pair", 1)?;
    assert_eq!(ty.display, "readonly [1, \"a\"]", "as const produces a readonly literal tuple");
    let TsTypeKind::Tuple { elements, readonly: true } = &ty.kind else {
        return Err(format!("expected a readonly tuple, got {:?}", ty.kind));
    };
    let shapes: Vec<(&TsTypeKind, TsTupleElementKind)> =
        elements.iter().map(|element| Ok((kind(&facts, element.ty)?, element.kind))).collect::<Result<_, String>>()?;
    assert_eq!(
        shapes,
        [
            (&TsTypeKind::NumberLiteral(1.0), TsTupleElementKind::Required),
            (&TsTypeKind::StringLiteral("a".to_owned()), TsTupleElementKind::Required),
        ],
        "both elements are required literals",
    );
    Ok(())
}

#[test]
fn enum_member_carries_enum_identity_and_value() -> Result<(), String> {
    let source = "enum Color { Red, Green }\nexport const color = Color.Green;\n";
    let facts = facts(source)?;
    let ty = type_of(&facts, source, "Color.Green", 0)?;
    assert_eq!(ty.display, "Color.Green", "member spelling");
    let TsTypeKind::EnumMember { enum_symbol, value } = &ty.kind else {
        return Err(format!("expected an enum member, got {:?}", ty.kind));
    };
    assert_eq!(enum_symbol.name, "Color", "member knows its enum");
    assert_eq!(kind(&facts, *value)?, &TsTypeKind::NumberLiteral(1.0), "auto-numbered value");
    Ok(())
}

#[test]
fn type_errors_are_reported_in_process() -> Result<(), String> {
    let source = "export const wrong: number = \"s\";\n";
    let facts = analyze(vec![SourceInput::new(FILE, source)]).map_err(|error| error.to_string())?;
    let codes: Vec<i32> = facts.errors().map(|diagnostic| diagnostic.code).collect();
    assert_eq!(codes, [2322_i32], "string is not assignable to number");
    let (start, end) = span_of(source, "wrong", 0)?;
    let diagnostic = facts.errors().next().ok_or("missing diagnostic")?;
    assert_eq!((diagnostic.start, diagnostic.end), (Some(start), Some(end)), "diagnostic span is the declaration name");
    Ok(())
}

#[test]
fn spans_are_utf8_byte_offsets() -> Result<(), String> {
    let source = "const café = \"ünïcödé\";\nexport const size = café.length;\n";
    let facts = facts(source)?;
    assert_eq!(type_of(&facts, source, "café.length", 0)?.kind, TsTypeKind::Number, "offsets after multi-byte text line up");
    // A `const` initialised with a string literal has that literal type (`tsc` .types: `café : "ünïcödé"`).
    assert_eq!(
        type_of(&facts, source, "café", 1)?.kind,
        TsTypeKind::StringLiteral("ünïcödé".to_owned()),
        "multi-byte identifier itself resolves",
    );
    Ok(())
}

#[test]
fn types_cross_module_imports() -> Result<(), String> {
    let lib = "export interface User { id: number; name: string }\nexport function make(id: number): User { return { id, name: \"\" }; }\n";
    let main = "import { make } from \"./lib.ts\";\nexport const user = make(1);\n";
    let facts = analyze(vec![SourceInput::new("src/lib.ts", lib), SourceInput::new(FILE, main)])
        .map_err(|error| error.to_string())?;
    let errors: Vec<&str> = facts.errors().map(|diagnostic| diagnostic.message.as_str()).collect();
    assert!(errors.is_empty(), "two-file program checks cleanly: {errors:?}");
    let (start, end) = span_of(main, "make(1)", 0)?;
    let ty = facts.type_at(FILE, start, end).ok_or("no type for make(1)")?;
    let TsTypeKind::Named { symbol, .. } = &ty.kind else {
        return Err(format!("expected the imported interface, got {:?}", ty.kind));
    };
    assert!(symbol.declaration.as_ref().is_some_and(|location| location.file.ends_with("src/lib.ts")), "identity points at lib.ts");
    Ok(())
}

/// Collects the span of every oxc expression node.
struct ExpressionSpans {
    /// `(start, end, debug name)` of each expression.
    spans: Vec<LabeledSpan>,
}

impl<'a> Visit<'a> for ExpressionSpans {
    fn visit_expression(&mut self, expression: &Expression<'a>) {
        let span = expression.span();
        let label = format!("{expression:?}").chars().take(40).collect();
        self.spans.push((span.start, span.end, label));
        walk::walk_expression(self, expression);
    }
}

#[test]
fn every_oxc_expression_span_addresses_a_checker_node() -> Result<(), String> {
    let source = r#"
interface Shape { kind: "circle" | "square"; size: number }
export function area(shapes: readonly Shape[], scale = 2): number {
  let total = 0;
  for (const shape of shapes) {
    const side = shape.kind === "circle" ? Math.PI * shape.size ** 2 : shape.size * shape.size;
    total += side * scale;
  }
  const label = `${total.toFixed(1)} units`;
  const parts = label.split(" ").map((part, index) => ({ part, index }));
  return parts.length > 0 ? (total as number) : -1;
}
"#;
    let facts = facts(source)?;
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
    assert!(parsed.diagnostics.is_empty(), "oxc parses the probe");
    let mut collector = ExpressionSpans { spans: Vec::new() };
    collector.visit_program(&parsed.program);
    let missing: Vec<&LabeledSpan> =
        collector.spans.iter().filter(|(start, end, _)| facts.type_at(FILE, *start, *end).is_none()).collect();
    assert!(collector.spans.len() > 40, "probe exercises many expressions");
    assert!(missing.is_empty(), "oxc expression spans without a checker type: {missing:#?}");
    Ok(())
}
