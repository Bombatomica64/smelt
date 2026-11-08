// `JSON.stringify` with an indent argument.

// Emit `smelt_json_indent(text, indent)`: lay compact JSON text out the way
// `JSON.stringify(value, replacer, space)` does.
//
// JavaScript's indented form puts every member and element on its own line,
// one `indent` deeper than its container, writes `": "` after a key, and
// keeps an empty object or array as `{}` / `[]`. That is a pure function of
// the compact text, so the helper walks it once, tracking string literals
// (whose brackets, commas and colons are content), rather than re-serializing
// a parsed value, which could respell numbers. An empty `indent` answers the
// text unchanged.
// @smelt:item smelt_json_indent when json_indent
/// `JSON.stringify(value, replacer, space)`'s layout over compact JSON text.
#[allow(dead_code)]
fn smelt_json_indent(text: &(impl SmeltHostText + ?Sized), indent: &(impl SmeltHostText + ?Sized)) -> String { let text = text.host_text(); let text: &str = &text; let indent = indent.host_text(); let indent: &str = &indent;
    if indent.is_empty() { return text.to_owned(); }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() * 2);
    let mut depth = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut index = 0_usize;
    while index < chars.len() {
        let ch = chars[index];
        if in_string { out.push(ch); if escaped { escaped = false; } else if ch == '\\' { escaped = true; } else if ch == '"' { in_string = false; } index += 1; continue; }
        match ch {
            '"' => { in_string = true; out.push(ch); }
            '{' | '[' => { let close = if ch == '{' { '}' } else { ']' }; if chars.get(index + 1) == Some(&close) { out.push(ch); out.push(close); index += 2; continue; } depth += 1; out.push(ch); out.push('\n'); out.push_str(&indent.repeat(depth)); }
            '}' | ']' => { depth = depth.saturating_sub(1); out.push('\n'); out.push_str(&indent.repeat(depth)); out.push(ch); }
            ',' => { out.push(','); out.push('\n'); out.push_str(&indent.repeat(depth)); }
            ':' => { out.push_str(": "); }
            _ => out.push(ch),
        }
        index += 1;
    }
    out
}

// @smelt:item-end
