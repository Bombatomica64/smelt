use super::*;

#[test]
fn print_call_lowers() -> TestResult {
    let source = py!(r#"
x: int = 1
print(x)
"#);
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    Ok(())
}

#[test]
fn requests_get_lowers_to_http_get_text() -> TestResult {
    let source = py!(r#"
import requests

def load() -> str:
    return requests.get("https://example.com")
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let load_id = module
        .items
        .first()
        .copied()
        .ok_or_else(|| "expected load function".to_owned())?;
    let Item::Function(load) = item(&ctx, load_id)? else {
        return Err("expected function item for load".to_owned());
    };
    let load_body_id = load.body.ok_or_else(|| "expected load body".to_owned())?;
    let load_body = body(&ctx, load_body_id)?;

    ensure(
        load_body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::HttpGetText { .. })),
        "expected requests.get lowering",
    )
}

#[test]
fn json_dumps_lowers_to_stringify() -> TestResult {
    let source = py!(r#"
import json
values: list[int] = [1, 2]
text: str = json.dumps(values)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::JsonStringify { .. })),
        "expected json.dumps lowering",
    )
}

#[test]
fn json_loads_lowers_to_parse() -> TestResult {
    let source = py!(r#"
import json
text: str = "[1,2]"
values: list[int] = json.loads(text)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::JsonParse { .. })),
        "expected json.loads lowering",
    )
}

#[test]
fn re_module_calls_lower_to_regex_matches() -> TestResult {
    let source = py!(r#"
import re
text: str = "abc123"
pattern: str = "\\d+"
found: bool = re.search(pattern, text)
starts: bool = re.match(pattern, text)
full: bool = re.fullmatch(pattern, text)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;

    for expected in [
        RegexMatchOp::Search,
        RegexMatchOp::Match,
        RegexMatchOp::FullMatch,
    ] {
        ensure(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::RegexIsMatch { op, .. } if op == expected),
            ),
            "expected re module regex lowering",
        )?;
    }
    Ok(())
}

#[test]
fn unsupported_re_module_forms_reject() -> TestResult {
    let mut ctx = HirCtx::new();
    let keyword = lower_errors(
        py!(r#"
import re
text: str = "abc123"
found: bool = re.search("\\d+", text, flags=0)
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&keyword)?
            .message
            .contains("pattern and text arguments only"),
        "expected re keyword diagnostic",
    )?;

    let mut ctx = HirCtx::new();
    let non_string = lower_errors(
        py!(r#"
import re
text: str = "abc123"
found: bool = re.search(1, text)
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&non_string)?
            .message
            .contains("string pattern and text"),
        "expected re type diagnostic",
    )
}

#[test]
fn unsupported_json_dumps_forms_reject() -> TestResult {
    let mut ctx = HirCtx::new();
    let keyword = lower_errors(
        py!(r#"
import json
values: list[int] = [1, 2]
text: str = json.dumps(values, indent=2)
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&keyword)?.message.contains("exactly one value"),
        "expected json.dumps keyword diagnostic",
    )?;

    let mut ctx = HirCtx::new();
    let unsupported_key = lower_errors(
        py!(r#"
import json
values: dict[int, str] = {1: "a"}
text: str = json.dumps(values)
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&unsupported_key)?
            .message
            .contains("JSON-serializable"),
        "expected json.dumps type diagnostic",
    )
}

#[test]
fn unsupported_json_loads_forms_reject() -> TestResult {
    let mut ctx = HirCtx::new();
    let extra_arg = lower_errors(
        py!(r#"
import json
text: str = "[1,2]"
values: list[int] = json.loads(text, None)
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&extra_arg)?
            .message
            .contains("exactly one text argument"),
        "expected json.loads arity diagnostic",
    )
}

#[test]
fn len_call_lowers() -> TestResult {
    let source = py!(r#"
values: list[int] = [1, 2, 3]
count: int = len(values)
word: str = "smelt"
letters: int = len(word)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body_id = module
        .body
        .ok_or_else(|| "expected module body".to_owned())?;
    let body = body(&ctx, body_id)?;

    let len_count = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::Len { .. }))
        .count();
    ensure_eq(&len_count, &2, "len call count")?;
    Ok(())
}

#[test]
fn abs_call_lowers() -> TestResult {
    let source = py!(r#"
value: int = -5
positive: int = abs(value)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body_id = module
        .body
        .ok_or_else(|| "expected module body".to_owned())?;
    let body = body(&ctx, body_id)?;

    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::NumericAbs { .. })),
        "expected abs() lowering",
    )?;
    Ok(())
}

#[test]
fn string_case_methods_lower() -> TestResult {
    let source = py!(r#"
word: str = "Smelt"
lower: str = word.lower()
upper: str = word.upper()
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body_id = module
        .body
        .ok_or_else(|| "expected module body".to_owned())?;
    let body = body(&ctx, body_id)?;

    ensure(
        body.exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                ExprKind::StringCase {
                    op: StringCaseOp::Lower,
                    ..
                }
            )
        }),
        "expected lower() lowering",
    )?;
    ensure(
        body.exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                ExprKind::StringCase {
                    op: StringCaseOp::Upper,
                    ..
                }
            )
        }),
        "expected upper() lowering",
    )?;
    Ok(())
}

#[test]
fn string_trim_method_lowers() -> TestResult {
    let source = py!(r#"
word: str = " Smelt "
trimmed: str = word.strip()
left: str = word.lstrip()
right: str = word.rstrip()
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body_id = module
        .body
        .ok_or_else(|| "expected module body".to_owned())?;
    let body = body(&ctx, body_id)?;

    for expected in [
        StringTrimSide::Both,
        StringTrimSide::Start,
        StringTrimSide::End,
    ] {
        ensure(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::StringTrim { side, .. } if side == expected),
            ),
            "expected string trim side lowering",
        )?;
    }
    Ok(())
}

#[test]
fn string_prefix_suffix_methods_lower() -> TestResult {
    let source = py!(r#"
word: str = "Smelt"
starts: bool = word.startswith("Sm")
ends: bool = word.endswith("lt")
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;

    for expected in [StringAffixOp::StartsWith, StringAffixOp::EndsWith] {
        ensure(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::StringAffix { op, .. } if op == expected),
            ),
            "expected string affix lowering",
        )?;
    }
    Ok(())
}

#[test]
fn string_search_methods_lower() -> TestResult {
    let source = py!(r#"
word: str = "Smelt"
first: int = word.find("m")
last: int = word.rfind("t")
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;

    for expected in [StringSearchOp::Find, StringSearchOp::RFind] {
        ensure(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::StringSearch { op, .. } if op == expected),
            ),
            "expected string search lowering",
        )?;
    }
    Ok(())
}

#[test]
fn list_and_string_slices_lower() -> TestResult {
    let source = py!(r#"
values: list[int] = [1, 2, 3, 4]
all_values: list[int] = values[:]
tail_values: list[int] = values[1:]
mid_values: list[int] = values[1:3]
last_values: list[int] = values[-2:]
word: str = "smelting"
all_text: str = word[:]
tail_text: str = word[1:]
mid_text: str = word[1:4]
last_text: str = word[-3:]
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;

    let list_slices = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::ListSlice { .. }))
        .count();
    let string_slices = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::StringSlice { .. }))
        .count();
    ensure_eq(&list_slices, &4, "list slice count")?;
    ensure_eq(&string_slices, &4, "string slice count")?;
    Ok(())
}

#[test]
fn list_append_method_lowers() -> TestResult {
    let source = py!(r#"
values: list[int] = [1, 2]
result: None = values.append(3)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;

    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListPush { .. })),
        "expected list append lowering",
    )
}

#[test]
fn list_extend_method_lowers() -> TestResult {
    let source = py!(r#"
left: list[int] = [1, 2]
right: list[int] = [3, 4]
result: None = left.extend(right)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListExtend { .. })),
        "expected list extend lowering",
    )
}

#[test]
fn list_insert_method_lowers() -> TestResult {
    let source = py!(r#"
values: list[int] = [1, 2]
result: None = values.insert(1, 0)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListInsert { .. })),
        "expected list insert lowering",
    )
}

#[test]
fn list_reverse_method_lowers() -> TestResult {
    let source = py!(r#"
values: list[int] = [1, 2]
result: None = values.reverse()
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListReverse { .. })),
        "expected list reverse lowering",
    )
}
