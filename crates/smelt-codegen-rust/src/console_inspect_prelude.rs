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

/// The emitted trait's name.
pub const INSPECT_TRAIT: &str = "SmeltInspect";

/// The trait method a top-level `console.log` argument is rendered through.
pub const CONSOLE_TEXT_METHOD: &str = "smelt_console_text";

/// The core runtime: context, trait, `util.inspect`'s layout helpers, and the
/// impls for the scalars, `Option` and tuples.
///
/// Every helper here only references `::std`, the console number formatter
/// (`smelt_console_number`, emitted unconditionally by
/// `number_format_prelude`) and the other items of this block.
const CORE: &str = r#"/// `util.inspect` state for one `console.log` argument (Node's `ctx`).
#[derive(Default)]
struct SmeltInspectContext {
    /// `ctx.indentationLvl`: the column the current entry starts at.
    indentation: usize,
    /// `ctx.currentDepth`: depth of the container most recently entered.
    current_depth: usize,
}

/// Node's `util.inspect`, implemented once per Rust storage type.
trait SmeltInspect {
    /// Render this value as an inspected ENTRY at `depth` (`recurseTimes`).
    fn smelt_inspect(&self, ctx: &mut SmeltInspectContext, depth: usize) -> String;
    /// Whether this value is a JavaScript number (grouped arrays right-align them).
    fn smelt_inspect_is_number(&self) -> bool { false }
    /// Render this value as a top-level `console.log` argument.
    fn smelt_console_text(&self) -> String { self.smelt_inspect(&mut SmeltInspectContext::default(), 0) }
}

/// The JavaScript `.length` of rendered text, in UTF-16 code units.
fn smelt_inspect_len(text: &str) -> usize { text.encode_utf16().count() }

/// `strEscape`: quote `value` the way `util.inspect` shows a string.
///
/// Single quotes unless the string contains one; then double quotes, then
/// backticks, and single quotes with `\'` escapes when it contains all three.
fn smelt_inspect_quote(value: &str) -> String {
    let quote = if !value.contains('\'') {
        '\''
    } else if !value.contains('"') {
        '"'
    } else if !value.contains('`') && !value.contains("${") {
        '`'
    } else {
        '\''
    };
    let mut out = String::with_capacity(value.len() + 2);
    out.push(quote);
    for ch in value.chars() {
        let code = ch as u32;
        match ch {
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            '\\' => out.push_str("\\\\"),
            '\'' if quote == '\'' => out.push_str("\\'"),
            _ if code < 0x20 || (0x7f..0xa0).contains(&code) => out.push_str(&format!("\\x{code:02X}")),
            _ => out.push(ch),
        }
    }
    out.push(quote);
    out
}

/// `formatPrimitive` for a string entry: a long multi-line string is split at
/// its newlines into `'line\n' +` pieces, one per row.
fn smelt_inspect_string(ctx: &SmeltInspectContext, value: &str) -> String {
    let length = smelt_inspect_len(value);
    if length > 16 && length + ctx.indentation + 4 > 80 {
        let lines = value.split_inclusive('\n').collect::<Vec<_>>();
        if lines.len() > 1 {
            let separator = format!(" +\n{}", " ".repeat(ctx.indentation + 2));
            return lines.iter().map(|line| smelt_inspect_quote(line)).collect::<Vec<_>>().join(&separator);
        }
    }
    smelt_inspect_quote(value)
}

/// A property key: bare when it is an identifier (`/^[a-zA-Z_][a-zA-Z_0-9]*$/`), quoted otherwise.
fn smelt_inspect_key(key: &str) -> String {
    let mut chars = key.chars();
    let identifier = chars.next().is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|rest| rest.is_ascii_alphanumeric() || rest == '_');
    if identifier { key.to_owned() } else { smelt_inspect_quote(key) }
}

/// `padStart`/`padEnd` to a JavaScript length.
fn smelt_inspect_pad(text: &str, width: usize, start: bool) -> String {
    let pad = " ".repeat(width.saturating_sub(smelt_inspect_len(text)));
    if start { format!("{pad}{text}") } else { format!("{text}{pad}") }
}

/// `groupArrayElements`: lay a long array of short entries out in columns.
///
/// `has_more` marks a trailing `... N more items` entry, which is kept out of
/// the grid; `numbers` right-aligns the cells.
fn smelt_inspect_group(ctx: &SmeltInspectContext, output: Vec<String>, numbers: bool, has_more: bool) -> Vec<String> {
    let separator_space = 2usize;
    let output_length = if has_more { output.len() - 1 } else { output.len() };
    let data_len = output[..output_length].iter().map(|entry| smelt_inspect_len(entry)).collect::<Vec<_>>();
    let total_length = data_len.iter().map(|len| len + separator_space).sum::<usize>();
    let max_length = data_len.iter().copied().max().unwrap_or(0);
    let actual_max = max_length + separator_space;
    if actual_max * 3 + ctx.indentation < 80 && (total_length as f64 / actual_max as f64 > 5.0 || max_length <= 6) {
        let average_bias = (actual_max as f64 - total_length as f64 / output.len() as f64).sqrt();
        let biased_max = (actual_max as f64 - 3.0 - average_bias).max(1.0);
        let square = ((2.5 * biased_max * output_length as f64).sqrt() / biased_max + 0.5).floor();
        let columns = square
            .min(((80 - ctx.indentation) / actual_max) as f64)
            .min(12.0)
            .min(15.0) as usize;
        if columns <= 1 {
            return output;
        }
        let max_line_length = (0..columns)
            .map(|column| {
                (column..output.len())
                    .step_by(columns)
                    .filter_map(|index| data_len.get(index).copied())
                    .max()
                    .unwrap_or(0)
                    + separator_space
            })
            .collect::<Vec<_>>();
        let mut grouped = Vec::new();
        let mut row = 0;
        while row < output_length {
            let end = (row + columns).min(output_length);
            let mut line = String::new();
            for index in row..end - 1 {
                line.push_str(&smelt_inspect_pad(&format!("{}, ", output[index]), max_line_length[index - row], numbers));
            }
            let last = end - 1;
            if numbers {
                line.push_str(&smelt_inspect_pad(&output[last], max_line_length[last - row] - separator_space, true));
            } else {
                line.push_str(&output[last]);
            }
            grouped.push(line);
            row += columns;
        }
        if has_more {
            grouped.push(output[output_length].clone());
        }
        return grouped;
    }
    output
}

/// `reduceToSingleString`: join rendered entries on one line or one per line.
///
/// `array` is `Some(numbers)` for an array (which may be grouped first) and
/// `None` for an object, map or set; `has_more` marks a trailing
/// `... N more items` entry.
fn smelt_inspect_reduce(ctx: &SmeltInspectContext, output: Vec<String>, open: &str, close: &str, depth: usize, array: Option<bool>, has_more: bool) -> String {
    let entries = output.len();
    let output = match array {
        Some(numbers) if entries > 6 => smelt_inspect_group(ctx, output, numbers, has_more),
        _ => output,
    };
    if ctx.current_depth.saturating_sub(depth) < 3 && entries == output.len() {
        let start = output.len() + ctx.indentation + smelt_inspect_len(open) + 10;
        let mut total = output.len() + start;
        let mut fits = total + output.len() <= 80;
        for entry in &output {
            if !fits {
                break;
            }
            total += smelt_inspect_len(entry);
            fits = total <= 80;
        }
        if fits {
            let joined = output.join(", ");
            if !joined.contains('\n') {
                return format!("{open} {joined} {close}");
            }
        }
    }
    let indentation = format!("\n{}", " ".repeat(ctx.indentation));
    format!("{open}{indentation}  {}{indentation}{close}", output.join(&format!(",{indentation}  ")))
}

/// The `... N more items` entry past `maxArrayLength` (100).
fn smelt_inspect_more(remaining: usize) -> String {
    format!("... {remaining} more item{}", if remaining > 1 { "s" } else { "" })
}

/// `formatArray`/`formatList`: an array (or tuple) of inspectable entries.
fn smelt_inspect_array(ctx: &mut SmeltInspectContext, depth: usize, entries: &[&dyn SmeltInspect]) -> String {
    if entries.is_empty() {
        return "[]".to_owned();
    }
    if depth > 2 {
        return "[Array]".to_owned();
    }
    let depth = depth + 1;
    ctx.current_depth = depth;
    let shown = entries.len().min(100);
    let mut output = Vec::with_capacity(shown + 1);
    ctx.indentation += 2;
    for entry in &entries[..shown] {
        output.push(entry.smelt_inspect(ctx, depth));
    }
    ctx.indentation -= 2;
    let remaining = entries.len() - shown;
    if remaining > 0 {
        output.push(smelt_inspect_more(remaining));
    }
    let numbers = entries.iter().take(output.len()).all(|entry| entry.smelt_inspect_is_number());
    smelt_inspect_reduce(ctx, output, "[", "]", depth, Some(numbers), remaining > 0)
}

/// `formatRaw` over an object's own enumerable properties.
///
/// `constructor` is the class name Node prefixes (`Point { x: 1 }`), empty for
/// a plain object.
fn smelt_inspect_object(ctx: &mut SmeltInspectContext, depth: usize, constructor: &str, entries: &[(&str, &dyn SmeltInspect)]) -> String {
    let open = if constructor.is_empty() { "{".to_owned() } else { format!("{constructor} {{") };
    if entries.is_empty() {
        return format!("{open}}}");
    }
    if depth > 2 {
        return format!("[{}]", if constructor.is_empty() { "Object" } else { constructor });
    }
    let depth = depth + 1;
    ctx.current_depth = depth;
    let mut output = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        ctx.indentation += 2;
        let text = value.smelt_inspect(ctx, depth);
        ctx.indentation -= 2;
        output.push(format!("{}: {text}", smelt_inspect_key(key)));
    }
    smelt_inspect_reduce(ctx, output, &open, "}", depth, None, false)
}

/// `formatSet`/`formatMap`: a keyed collection with its `Set(n)`/`Map(n)` prefix.
///
/// Each entry is a value (a set) or a `key => value` pair (a map).
fn smelt_inspect_collection(ctx: &mut SmeltInspectContext, depth: usize, kind: &str, entries: &[(&dyn SmeltInspect, Option<&dyn SmeltInspect>)]) -> String {
    let open = format!("{kind}({}) {{", entries.len());
    if entries.is_empty() {
        return format!("{open}}}");
    }
    if depth > 2 {
        return format!("[{kind}]");
    }
    let depth = depth + 1;
    ctx.current_depth = depth;
    let shown = entries.len().min(100);
    let mut output = Vec::with_capacity(shown + 1);
    ctx.indentation += 2;
    for (key, value) in &entries[..shown] {
        let key_text = key.smelt_inspect(ctx, depth);
        match value {
            Some(value) => {
                let value_text = value.smelt_inspect(ctx, depth);
                output.push(format!("{key_text} => {value_text}"));
            }
            None => output.push(key_text),
        }
    }
    ctx.indentation -= 2;
    let remaining = entries.len() - shown;
    if remaining > 0 {
        output.push(smelt_inspect_more(remaining));
    }
    smelt_inspect_reduce(ctx, output, &open, "}", depth, None, false)
}

impl SmeltInspect for f64 {
    fn smelt_inspect(&self, _ctx: &mut SmeltInspectContext, _depth: usize) -> String { smelt_console_number(*self) }
    fn smelt_inspect_is_number(&self) -> bool { true }
}

impl SmeltInspect for i64 {
    fn smelt_inspect(&self, _ctx: &mut SmeltInspectContext, _depth: usize) -> String { smelt_console_number(*self as f64) }
    fn smelt_inspect_is_number(&self) -> bool { true }
}

impl SmeltInspect for bool {
    fn smelt_inspect(&self, _ctx: &mut SmeltInspectContext, _depth: usize) -> String { self.to_string() }
}

impl SmeltInspect for String {
    fn smelt_inspect(&self, ctx: &mut SmeltInspectContext, _depth: usize) -> String { smelt_inspect_string(ctx, self) }
    fn smelt_console_text(&self) -> String { self.clone() }
}

impl SmeltInspect for () {
    fn smelt_inspect(&self, _ctx: &mut SmeltInspectContext, _depth: usize) -> String { "null".to_owned() }
}

impl<T: SmeltInspect> SmeltInspect for Option<T> {
    fn smelt_inspect(&self, ctx: &mut SmeltInspectContext, depth: usize) -> String {
        match self { Some(value) => value.smelt_inspect(ctx, depth), None => "undefined".to_owned() }
    }
    fn smelt_inspect_is_number(&self) -> bool { self.as_ref().is_some_and(SmeltInspect::smelt_inspect_is_number) }
    fn smelt_console_text(&self) -> String {
        match self { Some(value) => value.smelt_console_text(), None => "undefined".to_owned() }
    }
}

// A tuple is a JavaScript array.
macro_rules! smelt_inspect_tuple {
    ($($name:ident $index:tt),+) => {
        impl<$($name: SmeltInspect),+> SmeltInspect for ($($name,)+) {
            fn smelt_inspect(&self, ctx: &mut SmeltInspectContext, depth: usize) -> String {
                let entries: &[&dyn SmeltInspect] = &[$(&self.$index),+];
                smelt_inspect_array(ctx, depth, entries)
            }
        }
    };
}
smelt_inspect_tuple!(A 0);
smelt_inspect_tuple!(A 0, B 1);
smelt_inspect_tuple!(A 0, B 1, C 2);
smelt_inspect_tuple!(A 0, B 1, C 2, D 3);
smelt_inspect_tuple!(A 0, B 1, C 2, D 3, E 4);
smelt_inspect_tuple!(A 0, B 1, C 2, D 3, E 4, F 5);
smelt_inspect_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6);
smelt_inspect_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7);
smelt_inspect_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8);
smelt_inspect_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9);
smelt_inspect_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10);
smelt_inspect_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11);
"#;

/// The largest tuple arity [`CORE`] implements the trait for.
pub const MAX_TUPLE_ARITY: usize = 12;

/// `SmeltList<T>`: a JavaScript array.
const LIST_IMPL: &str = r"impl<T: SmeltInspect> SmeltInspect for SmeltList<T> {
    fn smelt_inspect(&self, ctx: &mut SmeltInspectContext, depth: usize) -> String {
        let values = self.borrow();
        let entries = values.iter().map(|value| value as &dyn SmeltInspect).collect::<Vec<_>>();
        smelt_inspect_array(ctx, depth, &entries)
    }
}
";

/// `SmeltPrimSet<T>`: a JavaScript `Set` of primitives, in insertion order.
const PRIM_SET_IMPL: &str = r#"impl<T: SmeltInspect> SmeltInspect for SmeltPrimSet<T> {
    fn smelt_inspect(&self, ctx: &mut SmeltInspectContext, depth: usize) -> String {
        let entries = self.iter().map(|value| (value as &dyn SmeltInspect, None)).collect::<Vec<_>>();
        smelt_inspect_collection(ctx, depth, "Set", &entries)
    }
}
"#;

/// The impls over the erased-carrier family: the string-keyed record, the
/// JavaScript `Map` and `Set` containers, and `SmeltUnknown` itself.
///
/// `SmeltUnknown` is inspected only when the value already WAS erased (a
/// source `unknown`, or a record value type with no static shape); nothing is
/// erased in order to reach this impl. An erased object carrying a runtime
/// marker or a function keeps its `Display` rendering, because the marker
/// stands for a host object (`Map`, `Error`, a class view) whose Node rendering
/// this carrier cannot reconstruct.
const UNKNOWN_FAMILY_IMPLS: &str = r#"impl<V: SmeltInspect + Clone> SmeltInspect for SmeltRecord<String, V> {
    fn smelt_inspect(&self, ctx: &mut SmeltInspectContext, depth: usize) -> String {
        let pairs = self.iter().filter(|(key, _)| !key.starts_with("__smelt")).collect::<Vec<_>>();
        let entries = pairs.iter().map(|(key, value)| (key.as_str(), value as &dyn SmeltInspect)).collect::<Vec<_>>();
        smelt_inspect_object(ctx, depth, "", &entries)
    }
}

impl<K: SmeltInspect + SmeltJsKeyEq + Clone, V: SmeltInspect + Clone> SmeltInspect for SmeltJsMap<K, V> {
    fn smelt_inspect(&self, ctx: &mut SmeltInspectContext, depth: usize) -> String {
        let pairs = self.iter().collect::<Vec<_>>();
        let entries = pairs.iter().map(|(key, value)| (key as &dyn SmeltInspect, Some(value as &dyn SmeltInspect))).collect::<Vec<_>>();
        smelt_inspect_collection(ctx, depth, "Map", &entries)
    }
}

impl<T: SmeltInspect + Clone + IntoSmeltUnknown> SmeltInspect for SmeltJsSet<T> {
    fn smelt_inspect(&self, ctx: &mut SmeltInspectContext, depth: usize) -> String {
        let entries = self.iter().map(|value| (value as &dyn SmeltInspect, None)).collect::<Vec<_>>();
        smelt_inspect_collection(ctx, depth, "Set", &entries)
    }
}

impl SmeltInspect for SmeltUnknown {
    fn smelt_inspect(&self, ctx: &mut SmeltInspectContext, depth: usize) -> String {
        match self {
            Self::Null => "null".to_owned(),
            Self::Undefined => "undefined".to_owned(),
            Self::Bool(value) => value.to_string(),
            Self::Number(value) => smelt_console_number(*value),
            Self::String(value) => smelt_inspect_string(ctx, value),
            Self::Array(values) => {
                let values = values.values.borrow();
                let entries = values.iter().map(|value| value as &dyn SmeltInspect).collect::<Vec<_>>();
                smelt_inspect_array(ctx, depth, &entries)
            }
            Self::Object(object) => {
                let pairs = object.iter().collect::<Vec<_>>();
                if pairs.iter().any(|(key, value)| key.starts_with("__smelt") || matches!(value, Self::Function(_))) {
                    return self.to_string();
                }
                let entries = pairs.iter().map(|(key, value)| (key.as_str(), value as &dyn SmeltInspect)).collect::<Vec<_>>();
                smelt_inspect_object(ctx, depth, "", &entries)
            }
            Self::Symbol(_) | Self::Function(_) | Self::Promise(_) => self.to_string(),
        }
    }
    fn smelt_inspect_is_number(&self) -> bool { matches!(self, Self::Number(_)) }
    fn smelt_console_text(&self) -> String {
        match self {
            Self::String(value) => value.to_string(),
            _ => self.smelt_inspect(&mut SmeltInspectContext::default(), 0),
        }
    }
}
"#;

/// Which generated containers this crate emits, so the prelude only names the
/// ones that exist.
#[derive(Debug, Clone, Copy)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is an independent pay-for-use gate of one emitted impl family"
)]
pub struct InspectContainers {
    /// `SmeltList` is emitted (`needs_smelt_list`).
    pub list: bool,
    /// `SmeltPrimSet` is emitted (`needs_prim_set`).
    pub prim_set: bool,
    /// The erased carrier family is emitted (`needs_unknown`).
    pub unknown: bool,
    /// The crate carries a JavaScript `bigint`, so `num_bigint::BigInt` and
    /// the `SmeltUnknown::BigInt` tag exist (`needs_bigint_runtime`).
    pub bigint: bool,
}

/// `util.inspect` of a bigint: its digits followed by `n` (`10n`).
///
/// Node groups and right-aligns a long array of bigints exactly as it does an
/// array of numbers, so a bigint answers `true` to the numeric question too.
const BIGINT_IMPL: &str = r#"impl SmeltInspect for ::num_bigint::BigInt {
    fn smelt_inspect(&self, _ctx: &mut SmeltInspectContext, _depth: usize) -> String { format!("{self}n") }
    fn smelt_inspect_is_number(&self) -> bool { true }
}
"#;

/// Render the runtime half of the formatter: [`CORE`] plus the container impls
/// whose containers this crate emits.
#[must_use]
pub fn runtime_text(containers: InspectContainers) -> String {
    let mut text = String::from(CORE);
    if containers.list {
        text.push('\n');
        text.push_str(LIST_IMPL);
    }
    if containers.prim_set {
        text.push('\n');
        text.push_str(PRIM_SET_IMPL);
    }
    if containers.bigint {
        text.push('\n');
        text.push_str(BIGINT_IMPL);
    }
    if containers.unknown {
        text.push('\n');
        if containers.bigint {
            // The erased carrier's own `BigInt` tag only exists in a bigint
            // crate, so its two arms are spliced in rather than spelled in the
            // shared text.
            text.push_str(
                &UNKNOWN_FAMILY_IMPLS
                    .replace(
                        "            Self::Number(value) => smelt_console_number(*value),\n",
                        "            Self::Number(value) => smelt_console_number(*value),\n            Self::BigInt(value) => format!(\"{value}n\"),\n",
                    )
                    .replace(
                        "matches!(self, Self::Number(_))",
                        "matches!(self, Self::Number(_) | Self::BigInt(_))",
                    ),
            );
        } else {
            text.push_str(UNKNOWN_FAMILY_IMPLS);
        }
    }
    text
}

/// One field of a generated record struct, as the inspect impl reads it.
pub struct InspectField {
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
pub fn struct_impl_text(
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
pub fn union_impl_text(rust_name: &str, member_count: usize) -> String {
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

    /// The core names the trait, the console entry point and the three Node
    /// layout routines it reproduces.
    #[test]
    fn core_names_the_node_layout_routines() {
        let text = runtime_text(InspectContainers { list: false, prim_set: false, unknown: false, bigint: false });
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
        let text = runtime_text(InspectContainers { list: true, prim_set: true, unknown: true, bigint: false });
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
