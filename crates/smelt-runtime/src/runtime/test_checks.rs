// Assertion checks answered over erased values, and snapshot text.
//
// **Dynamic boundary.** These back `chai`, `node:assert` and vitest matchers
// whose operand may be anything at the source level (`expect(x: any)`); which
// rule applies depends on the operand's runtime tag, as it does under Node.
// Typed operands are erased only to reach the check, and only in test code.

// @smelt:item smelt_test_check when vitest_mock
/// The answer of the assertion rule `check` on `actual` and its expected arguments.
///
/// An unknown rule name is false, so a mismatch between the frontend's table and
/// this one fails the test instead of passing it.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_test_check(check: &str, args: Vec<SmeltUnknown>) -> bool {
    let mut args = args.into_iter();
    let actual = args.next().unwrap_or(SmeltUnknown::Undefined);
    let first = args.next().unwrap_or(SmeltUnknown::Undefined);
    let second = args.next();
    match check {
        "exists" => !matches!(actual, SmeltUnknown::Null | SmeltUnknown::Undefined),
        "is_nan" => matches!(actual, SmeltUnknown::Number(value) if value.is_nan()),
        "is_empty" => smelt_test_is_empty(&actual),
        "type_is" => smelt_test_type_name(&actual) == smelt_erased_to_js_string(&first).to_string_lossy().to_lowercase(),
        "include" => smelt_test_includes(&actual, &first, false),
        "deep_include" => smelt_test_includes(&actual, &first, true),
        "length_is" => smelt_get_unknown_field(&actual, "length") == first || smelt_get_unknown_field(&actual, "size") == first,
        "has_property" => smelt_test_has_property(&actual, &first, second.as_ref()),
        "keys_are" => smelt_test_keys_are(&actual, first),
        "same_members" => smelt_test_same_members(&actual, &first),
        "includes_members" => smelt_test_elements(&first).iter().all(|item| smelt_test_includes(&actual, item, false)),
        "one_of" => smelt_test_elements(&first).iter().any(|item| smelt_test_strict_eq(item, &actual)),
        "satisfies" => smelt_callable_of(&first).is_some_and(|predicate| predicate(vec![actual]).is_ok_and(|answer| smelt_test_truthy(&answer))),
        "close_to" => smelt_test_close_to(&actual, &first, second.as_ref(), true),
        "within" => matches!((&actual, &first, second.as_ref()), (SmeltUnknown::Number(value), SmeltUnknown::Number(low), Some(SmeltUnknown::Number(high))) if value >= low && value <= high),
        "to_be_close_to" => {
            let digits = match second { Some(SmeltUnknown::Number(digits)) => digits, _ => 2.0 };
            smelt_test_close_to(&actual, &first, Some(&SmeltUnknown::Number(10f64.powf(-digits) / 2.0)), false)
        }
        "snapshot" => smelt_test_snapshot_matches(&actual, &first),
        _ => false,
    }
}

/// JavaScript truthiness of an erased value.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_test_truthy(value: &SmeltUnknown) -> bool {
    match value {
        SmeltUnknown::Null | SmeltUnknown::Undefined => false,
        SmeltUnknown::Bool(value) => *value,
        SmeltUnknown::Number(value) => *value != 0.0 && !value.is_nan(),
        SmeltUnknown::String(text) => !text.is_empty(),
        _ => true,
    }
}

/// `===`: primitives by value, everything with an identity by that identity.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_test_strict_eq(left: &SmeltUnknown, right: &SmeltUnknown) -> bool {
    match (left, right) {
        (SmeltUnknown::Number(left), SmeltUnknown::Number(right)) => left == right,
        (SmeltUnknown::Object(left), SmeltUnknown::Object(right)) => left.id == right.id,
        (SmeltUnknown::Array(left), SmeltUnknown::Array(right)) => left.id == right.id,
        (SmeltUnknown::Function(left), SmeltUnknown::Function(right)) => ::std::rc::Rc::ptr_eq(left, right),
        _ => left == right,
    }
}

/// The lowercase `Object.prototype.toString` tag chai's type names use.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_test_type_name(value: &SmeltUnknown) -> String {
    if smelt_callable_of(value).is_some() { return "function".to_owned(); }
    let tag = smelt_object_to_string_tag(value);
    tag.trim_start_matches("[object ").trim_end_matches(']').to_lowercase()
}

/// The elements of an erased array; any other value has none.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_test_elements(value: &SmeltUnknown) -> Vec<SmeltUnknown> {
    match value { SmeltUnknown::Array(values) => values.iter().collect(), _ => Vec::new() }
}

/// The source-visible own keys of an erased record.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_test_own_keys(object: &SmeltObject) -> Vec<String> {
    object.keys().iter().map(|key| key.to_string_lossy()).filter(|key| !key.starts_with("__smelt")).collect()
}

/// Whether a string, array, `Map`, `Set` or record has nothing in it.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_test_is_empty(value: &SmeltUnknown) -> bool {
    match value {
        SmeltUnknown::String(text) => text.is_empty(),
        SmeltUnknown::Array(values) => values.iter().next().is_none(),
        SmeltUnknown::Object(object) if object.contains_key("__smelt_map") || object.contains_key("__smelt_set") => smelt_get_unknown_field(value, "size") == SmeltUnknown::Number(0.0),
        SmeltUnknown::Object(object) => smelt_test_own_keys(object).is_empty(),
        _ => false,
    }
}

/// chai's `include`: a substring, an array element, or a subset of a record.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_test_includes(container: &SmeltUnknown, item: &SmeltUnknown, deep: bool) -> bool {
    let same = |left: &SmeltUnknown, right: &SmeltUnknown| if deep { left == right } else { smelt_test_strict_eq(left, right) };
    match (container, item) {
        (SmeltUnknown::String(text), SmeltUnknown::String(part)) => text.to_string_lossy().contains(&part.to_string_lossy()),
        (SmeltUnknown::Array(values), _) => values.iter().any(|value| same(&value, item)),
        (SmeltUnknown::Object(object), SmeltUnknown::Object(wanted)) => wanted.iter().all(|(key, value)| object.get(&key).is_some_and(|found| same(&found, &value))),
        _ => false,
    }
}

/// chai's `property(name[, value])`.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_test_has_property(value: &SmeltUnknown, name: &SmeltUnknown, expected: Option<&SmeltUnknown>) -> bool {
    let key = smelt_erased_to_js_string(name);
    let SmeltUnknown::Object(object) = value else { return match expected { Some(expected) => smelt_get_unknown_field(value, key.to_string_lossy().as_str()) == *expected, None => false } };
    match (object.get(&key), expected) { (Some(found), Some(expected)) => found == *expected, (Some(_), None) => true, (None, _) => false }
}

/// chai's `keys`: exactly these keys, given as an array or a record.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_test_keys_are(value: &SmeltUnknown, wanted: SmeltUnknown) -> bool {
    let SmeltUnknown::Object(object) = value else { return false };
    let mut wanted: Vec<String> = match wanted {
        SmeltUnknown::Object(record) => smelt_test_own_keys(&record),
        SmeltUnknown::Array(values) => values.iter().map(|key| smelt_erased_to_js_string(&key).to_string_lossy()).collect(),
        other => vec![smelt_erased_to_js_string(&other).to_string_lossy()],
    };
    let mut keys = smelt_test_own_keys(object);
    wanted.sort();
    keys.sort();
    keys == wanted
}

/// chai's `members`: the same elements, by `===`, in any order.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_test_same_members(actual: &SmeltUnknown, expected: &SmeltUnknown) -> bool {
    let mut remaining = smelt_test_elements(expected);
    let actual = smelt_test_elements(actual);
    if actual.len() != remaining.len() { return false; }
    actual.iter().all(|item| match remaining.iter().position(|other| smelt_test_strict_eq(other, item)) { Some(found) => { remaining.remove(found); true } None => false })
}

/// Whether `actual` is within `delta` of `expected`; an equal `Infinity` is, a `NaN` never is.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_test_close_to(actual: &SmeltUnknown, expected: &SmeltUnknown, delta: Option<&SmeltUnknown>, inclusive: bool) -> bool {
    let (SmeltUnknown::Number(actual), SmeltUnknown::Number(expected), Some(SmeltUnknown::Number(delta))) = (actual, expected, delta) else { return false };
    if actual == expected { return true; }
    let distance = (actual - expected).abs();
    if inclusive { distance <= *delta } else { distance < *delta }
}

/// `value` as vitest serializes it into an inline snapshot (`pretty-format`).
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_test_snapshot_text(value: &SmeltUnknown, indent: usize) -> String {
    let inner = "  ".repeat(indent + 1);
    let outer = "  ".repeat(indent);
    match value {
        SmeltUnknown::Null => "null".to_owned(),
        SmeltUnknown::Undefined => "undefined".to_owned(),
        SmeltUnknown::Bool(value) => value.to_string(),
        SmeltUnknown::Number(value) if *value == 0.0 && value.is_sign_negative() => "-0".to_owned(),
        SmeltUnknown::String(text) => format!("\"{}\"", text.to_string_lossy()),
        SmeltUnknown::Array(values) => {
            let items: Vec<String> = values.iter().map(|item| format!("{inner}{},\n", smelt_test_snapshot_text(&item, indent + 1))).collect();
            if items.is_empty() { "[]".to_owned() } else { format!("[\n{}{outer}]", items.concat()) }
        }
        SmeltUnknown::Promise(_) => "Promise {}".to_owned(),
        SmeltUnknown::Object(object) if object.contains_key("__smelt_date") => value.to_iso_string(),
        SmeltUnknown::Object(object) if object.contains_key("__smelt_regexp") => format!("/{}/{}", smelt_erased_to_js_string(&smelt_get_unknown_field(value, "source")).to_string_lossy(), smelt_erased_to_js_string(&smelt_get_unknown_field(value, "flags")).to_string_lossy()),
        SmeltUnknown::Object(object) if object.contains_key("__smelt_map") || object.contains_key("__smelt_set") => {
            let name = if object.contains_key("__smelt_map") { "Map" } else { "Set" };
            if smelt_test_is_empty(value) { return format!("{name} {{}}"); }
            let inner = "  ".repeat(indent + 1);
            let lines: Vec<String> = match (object.get("__smelt_map"), object.get("__smelt_set")) {
                (_, Some(SmeltUnknown::Array(members))) => members.iter().map(|member| format!("{inner}{},\n", smelt_test_snapshot_text(&member, indent + 1))).collect(),
                (Some(SmeltUnknown::Array(entries)), _) => entries.iter().map(|entry| match &entry { SmeltUnknown::Array(pair) => format!("{inner}{} => {},\n", smelt_test_snapshot_text(&pair.get(0).unwrap_or(SmeltUnknown::Undefined), indent + 1), smelt_test_snapshot_text(&pair.get(1).unwrap_or(SmeltUnknown::Undefined), indent + 1)), other => format!("{inner}{},\n", smelt_test_snapshot_text(other, indent + 1)) }).collect(),
                _ => Vec::new(),
            };
            format!("{name} {{\n{}{}}}", lines.concat(), "  ".repeat(indent))
        }
        SmeltUnknown::Object(object) if smelt_object_to_string_tag(value).ends_with("Array]") => {
            let tag = smelt_object_to_string_tag(value);
            let name = tag.trim_start_matches("[object ").trim_end_matches(']').to_owned();
            let length = match smelt_get_unknown_field(value, "length") { SmeltUnknown::Number(length) => length as usize, _ => 0 };
            let items: Vec<String> = (0..length).map(|index| format!("{inner}{},\n", smelt_test_snapshot_text(&smelt_get_unknown_field(value, index.to_string().as_str()), indent + 1))).collect();
            if items.is_empty() { format!("{name} []") } else { format!("{name} [\n{}{outer}]", items.concat()) }
        }
        SmeltUnknown::Object(object) if object.contains_key("__smelt_error") => {
            let name = match object.get("name") { Some(SmeltUnknown::String(name)) => name.to_string_lossy(), _ => "Error".to_owned() };
            format!("[{name}: {}]", smelt_thrown_message(value))
        }
        SmeltUnknown::Object(object) if smelt_callable_of(value).is_none() => {
            let mut keys = smelt_test_own_keys(object);
            keys.sort();
            let items: Vec<String> = keys.iter().map(|key| format!("{inner}\"{key}\": {},\n", smelt_test_snapshot_text(&object.get(key.as_str()).unwrap_or(SmeltUnknown::Undefined), indent + 1))).collect();
            let prefix = match object.get("__smelt_class") { Some(SmeltUnknown::String(name)) => format!("{} ", name.to_string_lossy()), _ => String::new() };
            if items.is_empty() { format!("{prefix}{{}}") } else { format!("{prefix}{{\n{}{outer}}}", items.concat()) }
        }
        SmeltUnknown::Function(_) | SmeltUnknown::Object(_) => "[Function anonymous]".to_owned(),
        other => smelt_erased_to_js_string(other).to_string_lossy(),
    }
}

/// Drop the common indent an inline snapshot's source layout adds.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_test_snapshot_dedent(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let indent = lines.iter().filter(|line| !line.trim().is_empty()).map(|line| line.len() - line.trim_start().len()).min().unwrap_or(0);
    lines.iter().map(|line| line.get(indent..).unwrap_or("")).collect::<Vec<_>>().join("\n").trim().to_owned()
}

/// Whether `actual` serializes to the stored inline snapshot.
#[allow(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::float_cmp, clippy::option_if_let_else, clippy::suboptimal_flops, clippy::too_many_lines, clippy::unreadable_literal, reason = "JavaScript numbers and a PRNG convert between f64 and integers on purpose")]
fn smelt_test_snapshot_matches(actual: &SmeltUnknown, snapshot: &SmeltUnknown) -> bool {
    let SmeltUnknown::String(snapshot) = snapshot else { return false };
    smelt_test_snapshot_text(actual, 0).trim() == smelt_test_snapshot_dedent(&snapshot.to_string_lossy())
}
// @smelt:item-end
