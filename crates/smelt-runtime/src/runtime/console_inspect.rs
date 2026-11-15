// `util.inspect`: the text `console.log` prints for a typed value.
//
// Per-program impls (a record's fields, a union's arms) are written by the
// emitter; everything here is fixed. The items are emitted together, after the
// program's own items, and only when a `console.log` reaches them.

// @smelt:item SmeltInspectContext
/// `util.inspect` state for one `console.log` argument (Node's `ctx`).
#[derive(Default)]
struct SmeltInspectContext {
    //~ `ctx.indentationLvl`: the column the current entry starts at.
    indentation: usize,
    //~ `ctx.currentDepth`: depth of the container most recently entered.
    current_depth: usize,
}

/// Node's `util.inspect`, implemented once per Rust storage type.
trait SmeltInspect {
    //~ Render this value as an inspected ENTRY at `depth` (`recurseTimes`).
    fn smelt_inspect(&self, ctx: &mut SmeltInspectContext, depth: usize) -> String;
    //~ Whether this value is a JavaScript number (grouped arrays right-align them).
    fn smelt_inspect_is_number(&self) -> bool { false }
    //~ Render this value as a top-level `console.log` argument.
    fn smelt_console_text(&self) -> String { self.smelt_inspect(&mut SmeltInspectContext::default(), 0) }
}

/// The JavaScript `.length` of rendered text, in UTF-16 code units.
fn smelt_inspect_len(text: &str) -> usize { text.encode_utf16().count() }

/// `strEscape`: quote `value` the way `util.inspect` shows a string.
//~
//~ Single quotes unless the string contains one; then double quotes, then
//~ backticks, and single quotes with `\'` escapes when it contains all three.
fn smelt_inspect_quote(value: &str) -> String {
    let quote = smelt_inspect_quote_char(value);
    let mut out = String::with_capacity(value.len() + 2);
    out.push(quote);
    for ch in value.chars() { smelt_inspect_escape(&mut out, ch, quote); }
    out.push(quote);
    out
}

/// The quote `strEscape` picks for `value`.
fn smelt_inspect_quote_char(value: &str) -> char {
    if !value.contains('\'') {
        '\''
    } else if !value.contains('"') {
        '"'
    } else if !value.contains('`') && !value.contains("${") {
        '`'
    } else {
        '\''
    }
}

/// Appends `ch` to a string quoted with `quote`, escaped as `strEscape` does.
fn smelt_inspect_escape(out: &mut String, ch: char, quote: char) {
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

/// `strEscape` over a JavaScript string: a lone surrogate shows as its
/// `\uXXXX` escape in lowercase hex, as Node prints it.
fn smelt_inspect_quote_js(value: &JsStr) -> String {
    let quote = smelt_inspect_quote_char(&value.to_string_lossy());
    let mut out = String::with_capacity(value.len() + 2);
    out.push(quote);
    for decoded in value.chars_lossless() {
        match decoded {
            Ok(ch) => smelt_inspect_escape(&mut out, ch, quote),
            Err(lone) => { let _ = ::std::fmt::Write::write_fmt(&mut out, format_args!("\\u{lone:04x}")); }
        }
    }
    out.push(quote);
    out
}

/// `formatPrimitive` for a string entry: a long multi-line string is split at
/// its newlines into `'line\n' +` pieces, one per row.
fn smelt_inspect_string(ctx: &SmeltInspectContext, value: &JsStr) -> String {
    let length = value.len();
    if length > 16 && length + ctx.indentation + 4 > 80 {
        let units = value.units();
        let lines = units.split_inclusive(|unit| *unit == u16::from(b'\n')).collect::<Vec<_>>();
        if lines.len() > 1 {
            let separator = format!(" +\n{}", " ".repeat(ctx.indentation + 2));
            return lines.iter().map(|line| smelt_inspect_quote_js(JsStr::from_units(line))).collect::<Vec<_>>().join(&separator);
        }
    }
    smelt_inspect_quote_js(value)
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
//~
//~ `has_more` marks a trailing `... N more items` entry, which is kept out of
//~ the grid; `numbers` right-aligns the cells.
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
//~
//~ `array` is `Some(numbers)` for an array (which may be grouped first) and
//~ `None` for an object, map or set; `has_more` marks a trailing
//~ `... N more items` entry.
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
//~
//~ `constructor` is the class name Node prefixes (`Point { x: 1 }`), empty for
//~ a plain object.
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
//~
//~ Each entry is a value (a set) or a `key => value` pair (a map).
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
    fn smelt_inspect(&self, ctx: &mut SmeltInspectContext, _depth: usize) -> String { smelt_inspect_string(ctx, &JsString::from(self.as_str())) }
    fn smelt_console_text(&self) -> String { self.clone() }
}

impl SmeltInspect for JsString {
    fn smelt_inspect(&self, ctx: &mut SmeltInspectContext, _depth: usize) -> String { smelt_inspect_string(ctx, self) }
    fn smelt_console_text(&self) -> String { self.to_string_lossy() }
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

//~ A tuple is a JavaScript array.
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
// @smelt:item-end

// @smelt:item impl_SmeltInspect_for_SmeltList when smelt_list

impl<T: SmeltInspect> SmeltInspect for SmeltList<T> {
    fn smelt_inspect(&self, ctx: &mut SmeltInspectContext, depth: usize) -> String {
        let values = self.borrow();
        //~ Named properties written through an erased view (`(list as any).x = 1`) follow the elements.
        /*@if unknown*/let named = smelt_array_named_properties(self.id()); let keyed = named.iter().map(|(key, value)| SmeltInspectKeyed { key, value }).collect::<Vec<_>>();/*@end*/
        let entries = values.iter().map(|value| value as &dyn SmeltInspect)/*@if unknown*/.chain(keyed.iter().map(|entry| entry as &dyn SmeltInspect))/*@end*/.collect::<Vec<_>>();
        smelt_inspect_array(ctx, depth, &entries)
    }
}
// @smelt:item-end

// @smelt:item impl_SmeltInspect_for_SmeltPrimSet when prim_set

impl<T: SmeltInspect> SmeltInspect for SmeltPrimSet<T> {
    fn smelt_inspect(&self, ctx: &mut SmeltInspectContext, depth: usize) -> String {
        let store = self.store.borrow();
        let entries = store.entries.iter().map(|value| (value as &dyn SmeltInspect, None)).collect::<Vec<_>>();
        smelt_inspect_collection(ctx, depth, "Set", &entries)
    }
}
// @smelt:item-end

// @smelt:item impl_SmeltInspect_for_BigInt when bigint

impl SmeltInspect for ::num_bigint::BigInt {
    fn smelt_inspect(&self, _ctx: &mut SmeltInspectContext, _depth: usize) -> String { format!("{self}n") }
    fn smelt_inspect_is_number(&self) -> bool { true }
}
// @smelt:item-end

// @smelt:item impl_SmeltInspect_for_SmeltRecord when unknown

impl<V: SmeltInspect + Clone> SmeltInspect for SmeltRecord<JsString, V> {
    fn smelt_inspect(&self, ctx: &mut SmeltInspectContext, depth: usize) -> String {
        let pairs = self.iter().filter(|(key, _)| !key.eq_str_prefix("__smelt")).collect::<Vec<_>>();
        let key_texts = pairs.iter().map(|(key, _)| key.to_string_lossy()).collect::<Vec<_>>();
                let entries = pairs.iter().zip(&key_texts).map(|((_, value), key)| (key.as_str(), value as &dyn SmeltInspect)).collect::<Vec<_>>();
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
        let members = self.iter().collect::<Vec<_>>();
        let entries = members.iter().map(|value| (value as &dyn SmeltInspect, None)).collect::<Vec<_>>();
        smelt_inspect_collection(ctx, depth, "Set", &entries)
    }
}

/// One `key: value` entry of an array's own named properties.
struct SmeltInspectKeyed<'a> {
    /// The property name.
    key: &'a str,
    /// The property value.
    value: &'a SmeltUnknown,
}

impl SmeltInspect for SmeltInspectKeyed<'_> {
    fn smelt_inspect(&self, ctx: &mut SmeltInspectContext, depth: usize) -> String {
        format!("{}: {}", smelt_inspect_key(self.key), self.value.smelt_inspect(ctx, depth))
    }
}

impl SmeltInspect for SmeltUnknown {
    fn smelt_inspect(&self, ctx: &mut SmeltInspectContext, depth: usize) -> String {
        match self {
            Self::Null => "null".to_owned(),
            Self::Undefined => "undefined".to_owned(),
            Self::Bool(value) => value.to_string(),
            Self::Number(value) => smelt_console_number(*value),
// @smelt:if bigint
            Self::BigInt(value) => format!("{value}n"),
// @smelt:end
            Self::String(value) => smelt_inspect_string(ctx, value),
            Self::Array(array) => {
                //~ An array's own named properties (a match's `index`, `input`,
                //~ `groups`, or `arr.extra = 1`) follow its elements as `key: value`.
                let named = smelt_array_named_properties(array.id);
                let values = array.clone().into_vec();
                let keyed = named.iter().map(|(key, value)| SmeltInspectKeyed { key, value }).collect::<Vec<_>>();
                let entries = values.iter().map(|value| value as &dyn SmeltInspect).chain(keyed.iter().map(|entry| entry as &dyn SmeltInspect)).collect::<Vec<_>>();
                smelt_inspect_array(ctx, depth, &entries)
            }
            Self::Object(object) => {
                /*@if live_containers*/smelt_live_container_refresh(object);/*@end*/
                if let Some(Self::Array(members)) = object.get("__smelt_set") {
                    let members = members.iter().collect::<Vec<_>>();
                    let entries = members.iter().map(|member| (member as &dyn SmeltInspect, None)).collect::<Vec<_>>();
                    return smelt_inspect_collection(ctx, depth, "Set", &entries);
                }
                if let Some(Self::Array(items)) = object.get("__smelt_map") {
                    let pairs = items.iter().filter_map(|item| match item { Self::Array(pair) => Some((pair.get(0).unwrap_or(Self::Undefined), pair.get(1).unwrap_or(Self::Undefined))), _ => None }).collect::<Vec<_>>();
                    let entries = pairs.iter().map(|(key, value)| (key as &dyn SmeltInspect, Some(value as &dyn SmeltInspect))).collect::<Vec<_>>();
                    return smelt_inspect_collection(ctx, depth, "Map", &entries);
                }
                let pairs = object.iter().filter(|(key, _)| !key.eq_str("__smelt_call")).collect::<Vec<_>>();
                if pairs.iter().any(|(key, value)| key.eq_str_prefix("__smelt") || matches!(value, Self::Function(_))) {
                    return self.to_string();
                }
                let key_texts = pairs.iter().map(|(key, _)| key.to_string_lossy()).collect::<Vec<_>>();
                let entries = pairs.iter().zip(&key_texts).map(|((_, value), key)| (key.as_str(), value as &dyn SmeltInspect)).collect::<Vec<_>>();
                smelt_inspect_object(ctx, depth, "", &entries)
            }
            Self::Symbol(_) | Self::Function(_) | Self::Promise(_) => self.to_string(),
        }
    }
    fn smelt_inspect_is_number(&self) -> bool { matches!(self, Self::Number(_)/*@if bigint*/ | Self::BigInt(_)/*@end*/) }
    fn smelt_console_text(&self) -> String {
        match self {
            Self::String(value) => value.to_string(),
            _ => self.smelt_inspect(&mut SmeltInspectContext::default(), 0),
        }
    }
}
// @smelt:item-end

// @smelt:item impl_SmeltInspect_for_SmeltMatch when dep_regress & unknown
/// A regex match prints as Node prints the match array: its elements, then
/// `index`, `input` and `groups`, which its erasure carries.
impl SmeltInspect for SmeltMatch {
    fn smelt_inspect(&self, ctx: &mut SmeltInspectContext, depth: usize) -> String {
        self.clone().into_smelt_unknown().smelt_inspect(ctx, depth)
    }
}
// @smelt:item-end
