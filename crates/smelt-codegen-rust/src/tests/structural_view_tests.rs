//! Structural views (`crate::structural_views`): which record conversions are
//! planned to share the object, and the Rust the pair is emitted as.

use super::*;
use crate::structural_views::StructuralViews;

/// The `source -> target` pairs planned for TypeScript `ts`, by source name,
/// sorted, and the reference-record names.
fn planned_views(ts: &str) -> (Vec<String>, Vec<String>) {
    let mut ctx = HirCtx::new();
    assert!(to_hir(ts, FileId(0), &mut ctx).is_ok(), "HIR");
    let lowered = smelt_mir::lower_hir(&ctx.krate);
    assert!(lowered.is_ok(), "MIR lowering");
    let Ok(mut mir) = lowered else {
        return (Vec::new(), Vec::new());
    };
    smelt_mir::opt::optimize(&mut mir);
    let views = StructuralViews::plan(&mir);
    let name = |symbol| mir.symbols.get(symbol).unwrap_or("?").to_owned();
    let mut pairs = Vec::new();
    for target in &mir.interfaces {
        for source in views.implementors(target.name) {
            pairs.push(format!("{} -> {}", name(*source), name(target.name)));
        }
    }
    pairs.sort();
    let mut references: Vec<String> = classify::reference_classes(&mir)
        .into_iter()
        .map(name)
        .collect();
    references.sort();
    (pairs, references)
}

/// Rule: a shape handed to a slot of another shape that declares a subset of
/// its properties at the same types is a view of it, and both are reference
/// records, although neither is written in the program: the slot holds the
/// caller's object, and what happens to it there is not visible here.
#[test]
fn plans_a_view_for_each_flow_between_shapes_that_share() {
    for (slot, flow) in [
        (
            "interface Named { label: string }",
            "function take(p: Named): string { return p.label; }\nfunction run(): string { const a: Point = { x: 1, label: 'a' }; return take(a); }",
        ),
        (
            "type Named = { label: string; x: number }",
            "function run(): number { const a: Point = { x: 1, label: 'a' }; const b: Named = a; return b.x; }",
        ),
        (
            "interface Named { label: string }",
            "function run(): Named { const a: Point = { x: 1, label: 'a' }; return a; }",
        ),
        (
            "interface Named { label: string }",
            "function take(all: Named[]): number { return all.length; }\nfunction run(): number { const all: Point[] = [{ x: 1, label: 'a' }]; return take(all); }",
        ),
        (
            "interface Named { label: string }",
            "type Holder = { item: Named };\nfunction run(): Holder { const a: Point = { x: 1, label: 'a' }; return { item: a }; }",
        ),
    ] {
        let (pairs, references) = planned_views(&format!(
            "type Point = {{ x: number; label: string }};\n{slot};\n{flow}\n"
        ));
        assert_eq!(pairs, vec!["Point -> Named".to_owned()], "{slot} / {flow}");
        assert!(
            references.contains(&"Point".to_owned()) && references.contains(&"Named".to_owned()),
            "{slot} / {flow}: {references:?}"
        );
    }
}

/// A view converted on to a further view: the record's cell implements every
/// trait it reaches.
#[test]
fn a_record_implements_every_view_it_reaches() {
    let (pairs, _) = planned_views(
        "type Point = { x: number; label: string };\ninterface Named { label: string }\ninterface Titled { label: string }\n\
         function last(p: Titled): string { return p.label; }\nfunction middle(p: Named): string { return last(p); }\n\
         function run(): string { const a: Point = { x: 1, label: 'a' }; return middle(a); }\n",
    );
    assert_eq!(
        pairs,
        vec![
            "Named -> Titled".to_owned(),
            "Point -> Named".to_owned(),
            "Point -> Titled".to_owned(),
        ]
    );
}

/// Not a view, so the field-wise conversion is kept: the slot declares a
/// property at another type, an optional property the value lacks, or either
/// side is a class or a generic shape. None of these lifts the records.
#[test]
fn a_pair_that_does_not_share_plans_no_view() {
    for (slot, call) in [
        ("type Slot = { x: number | string }", "take(a)"),
        ("type Slot = { x: number; note?: string }", "take(a)"),
        ("class Slot { x: number = 0; }", "take(a)"),
        ("type Slot<T = number> = { x: T }", "take(a)"),
    ] {
        let (pairs, references) = planned_views(&format!(
            "type Point = {{ x: number; label: string }};\n{slot};\nfunction take(p: Slot): void {{}}\n\
             function run(): void {{ const a: Point = {{ x: 1, label: 'a' }}; {call}; }}\n"
        ));
        assert!(pairs.is_empty(), "{slot}: {pairs:?}");
        assert!(references.is_empty(), "{slot}: {references:?}");
    }
    // The same shape on both sides is no conversion at all.
    let (pairs, references) = planned_views(
        "type Point = { x: number; label: string };\nfunction take(p: Point): number { return p.x; }\n\
         function run(): number { const a: Point = { x: 1, label: 'a' }; return take(a); }\n",
    );
    assert!(
        pairs.is_empty() && references.is_empty(),
        "{pairs:?} {references:?}"
    );
}

/// The emitted pair: the source keeps its concrete cell, the target's handle
/// holds the view trait object, the conversion unsizes the same `Rc`, and a
/// property of the target is reached through its accessors.
#[test]
fn a_view_target_is_a_trait_object_over_the_source_cell() {
    let source = source_for(
        "type Point = { x: number; label: string };\ninterface Named { label: string }\n\
         function rename(p: Named): void { p.label = p.label + '!'; }\n\
         function run(): string { const a: Point = { x: 1, label: 'a' }; rename(a); return a.label; }\n",
    );
    let body = program_body(&source);
    for expected in [
        "struct Point(::std::rc::Rc<::std::cell::RefCell<PointInner>>);",
        "struct Named(::std::rc::Rc<dyn NamedSmeltView>);",
        "fn label(&self) -> ::std::cell::Ref<'_, JsString>;",
        "fn label_mut(&self) -> ::std::cell::RefMut<'_, JsString>;",
        "impl NamedSmeltView for ::std::cell::RefCell<NamedInner>",
        "impl NamedSmeltView for ::std::cell::RefCell<PointInner>",
        "rename(Named(a.clone().0.clone()))",
        "(*p.0.label_mut()) = ",
        "(*p.0.label()).clone()",
    ] {
        assert!(body.contains(expected), "missing `{expected}`:\n{body}");
    }
    // No conversion builds a second `Named` cell from the point's fields.
    assert!(
        !body.contains("NamedInner { label: smelt_struct_value"),
        "the conversion copies:\n{body}"
    );
    // Typed code on the source still reads its own cell directly.
    assert!(
        rust_fn(&source, "run").contains("a.0.borrow().label"),
        "{body}"
    );
}
