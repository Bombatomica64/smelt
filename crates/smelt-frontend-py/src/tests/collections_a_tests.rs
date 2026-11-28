//! Print, requests/json/re, string, slice, list-mutation and map-lambda lowering tests.
use super::*;

#[test]
fn print_call_lowers() -> TestResult {
    let source = py!(r"
x: int = 1
print(x)
");
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module_body = body(
        &ctx,
        module(&ctx, module_id)?
            .body
            .ok_or("expected module body")?,
    )?;
    // `print(x)` is one call of the runtime print item with the one argument.
    let print_arg_counts: Vec<usize> = module_body
        .exprs
        .iter()
        .filter_map(|expr| match &expr.kind {
            ExprKind::Call { callee, args }
                if usize::try_from(callee.0)
                    .ok()
                    .and_then(|index| module_body.exprs.get(index))
                    .is_some_and(|callee| matches!(callee.kind, ExprKind::Item(_))) =>
            {
                Some(args.len())
            }
            _ => None,
        })
        .collect();
    ensure_eq(
        &print_arg_counts,
        &vec![1],
        "print calls and their argument counts",
    )?;
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

/// `json.dumps` lowers to its own Python op, not `JSON.stringify`: `CPython`'s
/// defaults (`", "`/`": "` separators, a float's repr, `ensure_ascii`) differ
/// from JavaScript's, and the `python-parity/json_dumps_defaults` fixture runs
/// the output against `python3`.
#[test]
fn json_dumps_lowers_to_py_json_dumps() -> TestResult {
    let source = py!(r"
import json
values: list[int] = [1, 2]
text: str = json.dumps(values)
");
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let code = module_code(&ctx, module)?;
    ensure(
        code.exprs()
            .any(|expr| matches!(expr.kind, ExprKind::PyJsonDumps { .. }))
            && !code
                .exprs()
                .any(|expr| matches!(expr.kind, ExprKind::JsonStringify { .. })),
        "expected json.dumps to lower to PyJsonDumps, not JSON.stringify",
    )
}

/// `json.loads` lowers to the typed `PyJsonLoads`, which decodes at the
/// annotated type, and never to JavaScript's `JsonParse`. The old requirement
/// (`JsonParse`) parsed into the erased carrier, whose numbers are `f64`, so a
/// JSON integer past 2**53 lost digits where `CPython` keeps every one
/// (`json_indent_and_exact_loads` in the Python parity suite).
#[test]
fn json_loads_lowers_to_typed_decode() -> TestResult {
    let source = py!(r#"
import json
text: str = "[1,2]"
values: list[int] = json.loads(text)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let code = module_code(&ctx, module)?;
    ensure(
        code.exprs()
            .any(|expr| matches!(expr.kind, ExprKind::PyJsonLoads { .. }))
            && !code
                .exprs()
                .any(|expr| matches!(expr.kind, ExprKind::JsonParse { .. })),
        "expected json.loads to lower to PyJsonLoads, not JsonParse",
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
    let code = module_code(&ctx, module)?;

    for expected in [
        RegexMatchOp::Search,
        RegexMatchOp::Match,
        RegexMatchOp::FullMatch,
    ] {
        ensure(
            code.exprs().any(
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
        py!(r"
import json
values: list[int] = [1, 2]
text: str = json.dumps(values, sort_keys=True)
"),
        &mut ctx,
    )?;
    // `indent=` is supported (`json_indent_and_exact_loads`); a keyword the
    // lowering does not model is still refused.
    ensure(
        first_error(&keyword)?
            .message
            .contains("the `indent` keyword"),
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
    let code = module_code(&ctx, module)?;

    let len_count = code
        .exprs()
        .filter(|expr| matches!(expr.kind, ExprKind::Len { .. }))
        .count();
    ensure_eq(&len_count, &2, "len call count")?;
    Ok(())
}

#[test]
fn abs_call_lowers() -> TestResult {
    let source = py!(r"
value: int = -5
positive: int = abs(value)
");
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let code = module_code(&ctx, module)?;

    ensure(
        code.exprs()
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
    let code = module_code(&ctx, module)?;

    ensure(
        code.exprs().any(|expr| {
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
        code.exprs().any(|expr| {
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
    let code = module_code(&ctx, module)?;

    for expected in [
        StringTrimSide::Both,
        StringTrimSide::Start,
        StringTrimSide::End,
    ] {
        ensure(
            code.exprs().any(
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
    let code = module_code(&ctx, module)?;

    for expected in [StringAffixOp::StartsWith, StringAffixOp::EndsWith] {
        ensure(
            code.exprs().any(
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
    let code = module_code(&ctx, module)?;

    for expected in [StringSearchOp::Find, StringSearchOp::RFind] {
        ensure(
            code.exprs().any(
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
    let code = module_code(&ctx, module)?;

    let list_slices = code
        .exprs()
        .filter(|expr| matches!(expr.kind, ExprKind::ListSlice { .. }))
        .count();
    let string_slices = code
        .exprs()
        .filter(|expr| matches!(expr.kind, ExprKind::StringSlice { .. }))
        .count();
    ensure_eq(&list_slices, &4, "list slice count")?;
    ensure_eq(&string_slices, &4, "string slice count")?;
    Ok(())
}

#[test]
fn list_append_method_lowers() -> TestResult {
    let source = py!(r"
values: list[int] = [1, 2]
result: None = values.append(3)
");
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let code = module_code(&ctx, module)?;

    ensure(
        code.exprs()
            .any(|expr| matches!(expr.kind, ExprKind::ListPush { .. })),
        "expected list append lowering",
    )
}

#[test]
fn list_extend_method_lowers() -> TestResult {
    let source = py!(r"
left: list[int] = [1, 2]
right: list[int] = [3, 4]
result: None = left.extend(right)
");
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let code = module_code(&ctx, module)?;
    ensure(
        code.exprs()
            .any(|expr| matches!(expr.kind, ExprKind::ListExtend { .. })),
        "expected list extend lowering",
    )
}

#[test]
fn list_insert_method_lowers() -> TestResult {
    let source = py!(r"
values: list[int] = [1, 2]
result: None = values.insert(1, 0)
");
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let code = module_code(&ctx, module)?;
    ensure(
        code.exprs()
            .any(|expr| matches!(expr.kind, ExprKind::ListInsert { .. })),
        "expected list insert lowering",
    )
}

#[test]
fn list_reverse_method_lowers() -> TestResult {
    let source = py!(r"
values: list[int] = [1, 2]
result: None = values.reverse()
");
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let code = module_code(&ctx, module)?;
    ensure(
        code.exprs()
            .any(|expr| matches!(expr.kind, ExprKind::ListReverse { .. })),
        "expected list reverse lowering",
    )
}

#[test]
fn del_dict_key_lowers_to_dict_remove_key() -> TestResult {
    let source = py!(r#"
def drop(scores: dict[str, int]) -> None:
    del scores["a"]
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let drop_id = module
        .items
        .first()
        .copied()
        .ok_or_else(|| "expected drop function".to_owned())?;
    let Item::Function(drop) = item(&ctx, drop_id)? else {
        return Err("expected function item for drop".to_owned());
    };
    let drop_body_id = drop.body.ok_or_else(|| "expected drop body".to_owned())?;
    let drop_body = body(&ctx, drop_body_id)?;
    ensure(
        drop_body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictRemoveKey { .. })),
        "expected `del dict[key]` to lower to DictRemoveKey",
    )
}

/// `del receiver[key]` on a receiver that is neither a dict nor a list is refused.
///
/// Old requirement: `del values[0]` on a `list[int]` was refused ("requires a
/// dict receiver"), because no helper raised `CPython`'s "list assignment index
/// out of range". New requirement: a list receiver lowers to `PyListDeleteAt`
/// (`del_list_index` parity fixture checks the behaviour against `python3`),
/// and only other receivers, such as a `str` (a `TypeError` in `CPython`),
/// are still refused.
#[test]
fn del_non_dict_subscript_is_rejected() -> TestResult {
    let mut ctx = HirCtx::new();
    let module_id = lower_module(
        py!(r"
def drop(values: list[int]) -> None:
    del values[0]
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let drop_id = module
        .items
        .first()
        .copied()
        .ok_or_else(|| "expected drop function".to_owned())?;
    let Item::Function(drop) = item(&ctx, drop_id)? else {
        return Err("expected function item for drop".to_owned());
    };
    let drop_body = body(
        &ctx,
        drop.body.ok_or_else(|| "expected drop body".to_owned())?,
    )?;
    ensure(
        drop_body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::PyListDeleteAt { .. })),
        "expected `del list[i]` to lower to PyListDeleteAt",
    )?;
    let source = py!(r"
def drop(text: str) -> None:
    del text[0]
");
    let mut str_ctx = HirCtx::new();
    let errors = lower_errors(source, &mut str_ctx)?;
    let first = first_error(&errors)?;
    ensure(
        first.message.contains("requires a dict or list receiver"),
        format!(
            "expected dict-or-list-receiver rejection, got {:?}",
            first.message
        ),
    )
}

#[test]
fn del_bare_name_is_rejected() -> TestResult {
    let source = py!(r"
x: int = 1
del x
");
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    let first = first_error(&errors)?;
    ensure(
        first.message.contains("only `del dict[key]`"),
        format!("expected del-form rejection, got {:?}", first.message),
    )
}

#[test]
fn map_lambda_with_unary_negation_lowers() -> TestResult {
    let source = py!(r"
values: list[int] = [1, 2, 3]
negated: list[int] = list(map(lambda value: -value, values))
");
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let code = module_code(&ctx, module)?;
    ensure(
        code.exprs().any(|expr| {
            matches!(
                &expr.kind,
                ExprKind::ListCallback {
                    op: smelt_hir::ListCallbackOp::Map,
                    ..
                }
            )
        }),
        "expected map lambda with unary negation to lower",
    )
}

#[test]
fn map_lambda_with_conditional_body_lowers() -> TestResult {
    let source = py!(r"
values: list[int] = [1, -2, 3]
clamped: list[int] = list(map(lambda value: value if value > 0 else 0, values))
");
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let code = module_code(&ctx, module)?;
    ensure(
        code.exprs().any(|expr| {
            matches!(
                &expr.kind,
                ExprKind::ListCallback {
                    op: smelt_hir::ListCallbackOp::Map,
                    ..
                }
            )
        }),
        "expected map lambda with conditional body to lower",
    )
}

#[test]
fn map_lambda_capture_order_does_not_depend_on_the_hash_seed() -> TestResult {
    let source = py!(r"
def combine(values: list[int], a: int, b: int, c: int, d: int, e: int, f: int) -> list[int]:
    return list(map(lambda value: value + f + e + d + c + b + a, values))
");
    let render = || -> Result<String, String> {
        let mut ctx = HirCtx::new();
        lower_module(source, &mut ctx)?;
        Ok(format!("{:?}", ctx.krate.bodies))
    };
    let first = render()?;
    for _ in 0_i32..24_i32 {
        ensure(
            render()? == first,
            "capture order changed between lowerings",
        )?;
    }
    Ok(())
}

/// A set item or dict key literal mixing item types is refused before codegen:
/// the union enum has no `Hash`/`Eq`, and `CPython` equates `1` with `1.0`.
/// A list or dict value mixing types is typed as the union instead
/// (`list_literal_mixed_types` parity fixture).
#[test]
fn mixed_hashed_literals_reject() -> TestResult {
    for source in [
        py!(r#"items = {1, "a"}"#),
        py!(r#"table = {1: "a", "b": "c"}"#),
    ] {
        let mut ctx = HirCtx::new();
        let errors = lower_errors(source, &mut ctx)?;
        ensure(
            first_error(&errors)?.message.contains("must have one type"),
            "expected a mixed hashed literal diagnostic",
        )?;
    }
    Ok(())
}

/// `%(key)s` templates refuse what `CPython` raises for: a positional
/// conversion beside a keyed one, a key the dict literal lacks, and a mapping
/// operand that is not a `dict[str, V]`.
#[test]
fn percent_mapping_misuse_rejects() -> TestResult {
    for (source, expected) in [
        (
            py!(r#"text = "%(a)s %s" % {"a": 1}"#),
            "positional conversion with a mapping",
        ),
        (py!(r#"text = "%(b)s" % {"a": 1}"#), "is not in the dict"),
        (py!(r#"text = "%(a)s" % (1,)"#), "dict with str keys"),
    ] {
        let mut ctx = HirCtx::new();
        let errors = lower_errors(source, &mut ctx)?;
        ensure(
            first_error(&errors)?.message.contains(expected),
            format!("expected a `%` mapping diagnostic containing {expected:?}"),
        )?;
    }
    Ok(())
}

/// `isinstance` refuses a target that is neither a builtin type nor a class.
///
/// Corrected requirement: this test also required the refusal of
/// `isinstance` on an `int | str | None` value ("union with a `None`
/// member"), because that union was erased (its canonical form, a union with a
/// `None` arm, has no concrete enum). Python's `A | B | None` now lowers to
/// `Optional(Union[A, B])`, a Rust `Option<SmeltUnionN>`
/// (`docs/design/python-numeric-tower.md` §3), so its members are testable and
/// `CPython` accepts the call; `isinstance_on_optional_union` checks it.
#[test]
fn isinstance_unsupported_forms_reject() -> TestResult {
    let source = py!(r#"
def f(x: int) -> bool:
    return isinstance(x, bytes)
"#);
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    ensure(
        first_error(&errors)?
            .message
            .contains("isinstance() of `bytes`"),
        "expected an isinstance diagnostic naming `bytes`",
    )
}

/// `int | str | None` is an optional of the concrete union `int | str` (never
/// the erased union with a `None` arm), and `isinstance` on it tests the
/// union's members (`UnionArmTest`).
#[test]
fn isinstance_on_optional_union() -> TestResult {
    let source = py!(r#"
def f(x: int | str | None) -> bool:
    return isinstance(x, int)
"#);
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    let function = function_named(&ctx, "f")?;
    let param_ty = function
        .params
        .first()
        .map(|param| param.ty)
        .ok_or("f has no parameter")?;
    let optional_union = matches!(ctx.krate.types.get(param_ty), Some(Type::Optional(inner, _))
        if matches!(ctx.krate.types.get(*inner), Some(Type::Union(members)) if members.len() == 2
            && members.iter().all(|member| matches!(ctx.krate.types.get(*member), Some(Type::PyInt | Type::PyStr)))));
    ensure(
        optional_union,
        "`int | str | None` must lower to Optional(Union[int, str])",
    )?;
    let tested = ctx.krate.bodies.iter().any(|body| {
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::UnionArmTest { .. }))
    });
    ensure(
        tested,
        "isinstance on the optional union must test its members",
    )
}

/// A literal `None` argument takes the optional parameter's type (as the
/// `None` of an omitted defaulted argument does), and a read after
/// `if x is None: return` is the payload: `x * 2` multiplies a `PyInt`.
#[test]
fn none_argument_and_is_none_narrowing() -> TestResult {
    let source = py!(r"
def f(x: int | None = None) -> int:
    if x is None:
        return -1
    return x * 2

def run() -> int:
    return f(None)
");
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    let optional_int = |ty| matches!(ctx.krate.types.get(ty), Some(Type::Optional(inner, _)) if ctx.krate.types.get(*inner) == Some(&Type::PyInt));
    let typed_none_arg = ctx.krate.bodies.iter().any(|body| {
        body.exprs.iter().any(|expr| match &expr.kind {
            ExprKind::Call { args, .. } => args.iter().any(|arg| {
                body.exprs
                    .get(usize::try_from(arg.0).unwrap_or(usize::MAX))
                    .is_some_and(|arg_expr| {
                        matches!(arg_expr.kind, ExprKind::Literal(Literal::None))
                            && optional_int(arg_expr.ty)
                    })
            }),
            _ => false,
        })
    });
    ensure(
        typed_none_arg,
        "`f(None)` passes a `None` typed `int | None`",
    )?;
    let narrowed_read = ctx.krate.bodies.iter().any(|body| {
        body.exprs.iter().any(|expr| {
            matches!(expr.kind, ExprKind::Local(_))
                && ctx.krate.types.get(expr.ty) == Some(&Type::PyInt)
                && body.locals.iter().any(|local| optional_int(local.ty))
        })
    });
    ensure(
        narrowed_read,
        "the read after `if x is None: return` is the `int` payload",
    )
}
