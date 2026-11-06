//! `console.log`'s value formatter — Node's `util.inspect` — as emitted Rust.
//!
//! `console.log(value)` does not stringify a non-string argument with
//! `String(value)`; it renders it with `util.inspect`, whose output is a
//! JavaScript-literal-like picture of the value:
//!
//! | source | Node | before this module |
//! | --- | --- | --- |
//! | `console.log([1, 2, 3])` | `[ 1, 2, 3 ]` | `[1.0, 2.0, 3.0]` (Rust `Debug`) |
//! | `console.log(["age", 42] as [string, number])` | `[ 'age', 42 ]` | `("age", 42.0)` |
//! | `console.log({ x: 1 })` (an interface record) | `{ x: 1 }` | `[object Object]` |
//! | `console.log(new Map([["a", 1]]))` | `Map(1) { 'a' => 1 }` | `[object Object]` |
//!
//! Design adapted from Bun's `--aot` (oven-sh/bun#45631), whose debugging
//! guide (`docs/bundler/aot-debugging.mdx`) checks compiled output by comparing
//! it with the same program run by the JavaScript runtime: this formatter is
//! what made Smelt's `console.log` output comparable with Node's at all.
//!
//! # The rule
//!
//! The formatter is a trait, [`INSPECT_TRAIT`], with one impl per Rust storage
//! type a TypeScript value can have: the scalars, `Option`, tuples, the
//! generated containers (`SmeltList`, `SmeltRecord`, `SmeltJsMap`, the two set
//! containers), the erased carrier, and — emitted per program — every
//! non-generic generated record struct and concrete union enum. So the impl is
//! chosen by the value's REAL Rust type, the shape a hand-written Rust port
//! would give it (a `Display`-like trait per type), never by erasing the value
//! into `SmeltUnknown` to format it.
//!
//! Each impl follows `lib/internal/util/inspect.js` (Node 24):
//!
//! * `formatValue`/`formatRaw`: strings are quoted with `strEscape` inside a
//!   container (single quotes, falling back to `"` and then `` ` `` when the
//!   string contains the quote), numbers use the console number rule (`-0`),
//!   containers past depth 2 collapse to `[Array]` / `[Object]` /
//!   `[ClassName]` / `[Map]` / `[Set]`, and at most 100 entries are shown
//!   (`... N more items`).
//! * `reduceToSingleString`: entries go on one line (`[ a, b ]`, `{ k: v }`)
//!   when the innermost nesting below this container is shallower than 3 and
//!   the line fits in 80 columns, and one entry per line otherwise.
//! * `groupArrayElements`: an array of more than 6 short entries is laid out
//!   in aligned columns, numbers right-aligned and anything else left-aligned.
//!
//! A top-level `console.log` STRING argument prints raw, not quoted, which is
//! the trait's `smelt_console_text` entry point (a union holding a string
//! prints it raw too).
//!
//! # Behavioral and type-preservation requirements
//!
//! * Behavior: for every value this formatter accepts, the printed text is
//!   byte-identical to `node` 24's `console.log` of the same source value. The
//!   end-to-end corpus checks the generated program's stdout against the golden
//!   AND the golden against `node` itself (`verify_end_to_end_example`).
//! * Types: no value is erased to format it. A list of numbers is inspected as
//!   a `SmeltList<f64>`, a record as its generated struct; only a value that
//!   was ALREADY a `SmeltUnknown` (a source `unknown`) is inspected through the
//!   erased carrier's own impl.
//!
//! # Known gaps (documented, not silently wrong)
//!
//! * A shape the emitter cannot prove inspectable — a generic class, a record
//!   holding a function, a `HashMap`-backed `Record` whose iteration order is
//!   not JavaScript's — keeps the previous rendering; see
//!   `FunctionEmitter::type_is_console_inspectable`.
//! * An erased object carrying runtime markers (`__smelt_*`, a `Map`/`Set`/
//!   `Error` view, a class view with prototype methods) or a function prints
//!   through `Display for SmeltUnknown` as before.
//! * An absent optional inside a container prints `undefined`; `T | null`
//!   and `T | undefined` share one Rust `Option` (see
//!   `console_optional_text`).
//! * Record struct fields print in DECLARATION order. A literal written in a
//!   different key order than its interface declares prints in the
//!   interface's order, where Node prints the literal's.
//! * Entry widths are counted in `char`s; Node counts display width, which
//!   differs for East-Asian wide characters.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]

/// The emitted trait's name.
pub(crate) const INSPECT_TRAIT: &str = "SmeltInspect";

/// The trait method a top-level `console.log` argument is rendered through.
pub(crate) const CONSOLE_TEXT_METHOD: &str = "smelt_console_text";

/// The largest tuple arity [`CORE`] implements the trait for.
pub(crate) const MAX_TUPLE_ARITY: usize = 12;

/// One field of a generated record struct, as the inspect impl reads it.
pub(crate) struct InspectField {
    /// The JavaScript property key (the SOURCE spelling).
    pub key: String,
    /// The Rust field identifier.
    pub rust_name: String,
    /// The field is an optional property of an object SHAPE (an interface or
    /// type literal): `None` means the property is absent, so it is skipped
    /// instead of printed as `undefined`.
    pub skip_when_absent: bool,
}

/// Render `impl SmeltInspect` for one non-generic generated record struct.
///
/// `constructor` is the source class name Node prints before the braces, or
/// empty for an object shape (Node shows a plain object). `reference` selects
/// the shared-cell storage (`struct Name(Rc<RefCell<NameInner>>)`), whose
/// fields are read through one borrow of the cell.
#[must_use]
pub(crate) fn struct_impl_text(
    rust_name: &str,
    constructor: &str,
    fields: &[InspectField],
    reference: bool,
) -> String {
    let receiver = if reference { "__smelt_inner" } else { "self" };
    let mut text = format!(
        "impl {INSPECT_TRAIT} for {rust_name} {{\n    fn smelt_inspect(&self, ctx: &mut SmeltInspectContext, depth: usize) -> String {{\n"
    );
    if reference {
        text.push_str("        let __smelt_inner = self.0.borrow();\n");
    }
    text.push_str("        let mut entries: Vec<(&str, &dyn SmeltInspect)> = Vec::new();\n");
    for field in fields {
        let key = &field.key;
        let name = &field.rust_name;
        if field.skip_when_absent {
            text.push_str(&format!(
                "        if let Some(value) = &{receiver}.{name} {{ entries.push(({key:?}, value)); }}\n"
            ));
        } else {
            text.push_str(&format!("        entries.push(({key:?}, &{receiver}.{name}));\n"));
        }
    }
    text.push_str(&format!(
        "        smelt_inspect_object(ctx, depth, {constructor:?}, &entries)\n    }}\n}}\n"
    ));
    text
}

/// Render `impl SmeltInspect` for one non-generic concrete union enum: every
/// method delegates to the member the value holds.
#[must_use]
pub(crate) fn union_impl_text(rust_name: &str, member_count: usize) -> String {
    let arms = |call: &str| {
        (0..member_count)
            .map(|index| format!("Self::M{index}(value) => value.{call}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "impl {INSPECT_TRAIT} for {rust_name} {{\n    fn smelt_inspect(&self, ctx: &mut SmeltInspectContext, depth: usize) -> String {{ match self {{ {} }} }}\n    fn smelt_inspect_is_number(&self) -> bool {{ match self {{ {} }} }}\n    fn {CONSOLE_TEXT_METHOD}(&self) -> String {{ match self {{ {} }} }}\n}}\n",
        arms("smelt_inspect(ctx, depth)"),
        arms("smelt_inspect_is_number()"),
        arms(&format!("{CONSOLE_TEXT_METHOD}()")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use smelt_runtime::order::CONSOLE_INSPECT;

    /// The core names the trait, the console entry point and the three Node
    /// layout routines it reproduces.
    #[test]
    fn core_names_the_node_layout_routines() {
        let text = crate::runtime_prelude::rendered_entries(CONSOLE_INSPECT, &|_| false);
        for expected in [
            "trait SmeltInspect",
            "fn smelt_console_text(&self) -> String",
            "fn smelt_inspect_quote(",
            "fn smelt_inspect_group(",
            "fn smelt_inspect_reduce(",
            "smelt_inspect_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11);",
        ] {
            assert!(text.contains(expected), "missing `{expected}`");
        }
        // A container impl names its container, so it must not be emitted
        // without it.
        assert!(!text.contains("SmeltList<T>"));
        assert!(!text.contains("SmeltPrimSet<T>"));
        assert!(!text.contains("for SmeltUnknown"));
    }

    /// Each container impl is gated on its own container.
    #[test]
    fn container_impls_follow_their_gates() {
        let text = crate::runtime_prelude::rendered_entries(CONSOLE_INSPECT, &|flag| matches!(flag, "smelt_list" | "prim_set" | "unknown"));
        assert!(text.contains("SmeltInspect for SmeltList<T>"));
        assert!(text.contains("SmeltInspect for SmeltPrimSet<T>"));
        assert!(text.contains("SmeltInspect for SmeltJsMap<K, V>"));
        assert!(text.contains("SmeltInspect for SmeltUnknown"));
    }

    /// A shape skips an absent optional property; a class prints every field.
    #[test]
    fn struct_impl_distinguishes_absent_shape_properties() {
        let text = struct_impl_text(
            "Point",
            "",
            &[
                InspectField { key: "x".to_owned(), rust_name: "x".to_owned(), skip_when_absent: false },
                InspectField { key: "label".to_owned(), rust_name: "label".to_owned(), skip_when_absent: true },
            ],
            false,
        );
        assert!(text.contains("entries.push((\"x\", &self.x));"));
        assert!(text.contains("if let Some(value) = &self.label { entries.push((\"label\", value)); }"));
        assert!(text.contains("smelt_inspect_object(ctx, depth, \"\", &entries)"));
        let reference = struct_impl_text(
            "Node",
            "Node",
            &[InspectField { key: "next".to_owned(), rust_name: "next".to_owned(), skip_when_absent: false }],
            true,
        );
        assert!(reference.contains("let __smelt_inner = self.0.borrow();"));
        assert!(reference.contains("entries.push((\"next\", &__smelt_inner.next));"));
        assert!(reference.contains("smelt_inspect_object(ctx, depth, \"Node\", &entries)"));
    }
}
