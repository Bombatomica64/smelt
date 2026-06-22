use super::*;

#[test]
fn unsupported_dict_get_forms_reject() -> TestResult {
    let mut ctx = HirCtx::new();
    let wrong_key = lower_errors(
        py!(r#"
mapping: dict[str, int] = {"a": 1}
value: int | None = mapping.get(1)
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&wrong_key)?.message.contains("key type"),
        "expected dict get key diagnostic",
    )?;

    let mut ctx = HirCtx::new();
    let wrong_default = lower_errors(
        py!(r#"
mapping: dict[str, int] = {"a": 1}
value: int = mapping.get("a", "fallback")
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&wrong_default)?.message.contains("value type"),
        "expected dict get default diagnostic",
    )?;

    let mut ctx = HirCtx::new();
    let wrong_arity = lower_errors(
        py!(r#"
mapping: dict[str, int] = {"a": 1}
value: int | None = mapping.get()
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&wrong_arity)?
            .message
            .contains("key and optional default"),
        "expected dict get arity diagnostic",
    )
}

#[test]
fn unsupported_dict_setdefault_forms_reject() -> TestResult {
    let mut ctx = HirCtx::new();
    let wrong_key = lower_errors(
        py!(r#"
mapping: dict[str, int] = {"a": 1}
value: int = mapping.setdefault(1, 2)
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&wrong_key)?.message.contains("key type"),
        "expected dict setdefault key diagnostic",
    )?;

    let mut ctx = HirCtx::new();
    let wrong_default = lower_errors(
        py!(r#"
mapping: dict[str, int] = {"a": 1}
value: int = mapping.setdefault("b", "fallback")
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&wrong_default)?.message.contains("value type"),
        "expected dict setdefault default diagnostic",
    )?;

    let mut ctx = HirCtx::new();
    let wrong_arity = lower_errors(
        py!(r#"
mapping: dict[str, int] = {"a": 1}
value: int = mapping.setdefault("b")
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&wrong_arity)?
            .message
            .contains("key and default"),
        "expected dict setdefault arity diagnostic",
    )
}

#[test]
fn unsupported_builtin_sum_forms_reject() -> TestResult {
    let mut ctx = HirCtx::new();
    let wrong_arity = lower_errors(
        py!(r#"
total: int = sum()
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&wrong_arity)?
            .message
            .contains("exactly one list"),
        "expected sum arity diagnostic",
    )?;

    let mut ctx = HirCtx::new();
    let non_numeric = lower_errors(
        py!(r#"
values: list[str] = ["a", "b"]
total: str = sum(values)
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&non_numeric)?
            .message
            .contains("int or float list"),
        "expected sum type diagnostic",
    )
}

#[test]
fn unsupported_builtin_all_any_forms_reject() -> TestResult {
    let mut ctx = HirCtx::new();
    let wrong_arity = lower_errors(
        py!(r#"
result: bool = all()
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&wrong_arity)?
            .message
            .contains("exactly one bool list"),
        "expected all arity diagnostic",
    )?;

    let mut ctx = HirCtx::new();
    let non_bool = lower_errors(
        py!(r#"
values: list[int] = [1, 2]
result: bool = any(values)
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&non_bool)?.message.contains("bool list"),
        "expected any type diagnostic",
    )
}

#[test]
fn unsupported_builtin_sorted_forms_reject() -> TestResult {
    let mut ctx = HirCtx::new();
    let wrong_arity = lower_errors(
        py!(r#"
values: list[int] = [1, 2]
ordered: list[int] = sorted(values, values)
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&wrong_arity)?
            .message
            .contains("exactly one list"),
        "expected sorted arity diagnostic",
    )?;

    let mut ctx = HirCtx::new();
    let non_sortable = lower_errors(
        py!(r#"
values: list[list[int]] = [[1], [2]]
ordered: list[list[int]] = sorted(values)
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&non_sortable)?
            .message
            .contains("sortable list"),
        "expected sorted type diagnostic",
    )?;

    let mut ctx = HirCtx::new();
    let non_literal_reverse = lower_errors(
        py!(r#"
flag: bool = True
values: list[int] = [1, 2]
ordered: list[int] = sorted(values, reverse=flag)
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&non_literal_reverse)?
            .message
            .contains("reverse must be a boolean literal"),
        "expected sorted non-literal reverse diagnostic",
    )
}

#[test]
fn unsupported_dict_pop_forms_reject() -> TestResult {
    let mut ctx = HirCtx::new();
    let missing_key = lower_errors(
        py!(r#"
mapping: dict[str, int] = {"a": 1}
value: int = mapping.pop()
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&missing_key)?
            .message
            .contains("key and optional default"),
        "expected missing key diagnostic",
    )?;

    let mut ctx = HirCtx::new();
    let wrong_key = lower_errors(
        py!(r#"
mapping: dict[str, int] = {"a": 1}
value: int = mapping.pop(1)
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&wrong_key)?.message.contains("key type"),
        "expected wrong key diagnostic",
    )?;

    let mut ctx = HirCtx::new();
    let wrong_default = lower_errors(
        py!(r#"
mapping: dict[str, int] = {"a": 1}
value: int = mapping.pop("a", "x")
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&wrong_default)?.message.contains("value type"),
        "expected wrong default diagnostic",
    )
}

#[test]
fn unsupported_dict_update_forms_reject() -> TestResult {
    let mut ctx = HirCtx::new();
    let missing = lower_errors(
        py!(r#"
left: dict[str, int] = {"a": 1}
result: None = left.update()
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&missing)?
            .message
            .contains("exactly one dict argument"),
        "expected dict update arity diagnostic",
    )?;

    let mut ctx = HirCtx::new();
    let wrong_type = lower_errors(
        py!(r#"
left: dict[str, int] = {"a": 1}
right: dict[str, str] = {"b": "x"}
result: None = left.update(right)
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&wrong_type)?
            .message
            .contains("receiver dict type"),
        "expected dict update type diagnostic",
    )
}

#[test]
fn unsupported_dict_copy_forms_reject() -> TestResult {
    let mut ctx = HirCtx::new();
    let errors = lower_errors(
        py!(r#"
mapping: dict[str, int] = {"a": 1}
copied: dict[str, int] = mapping.copy(1)
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&errors)?
            .message
            .contains("requires no arguments"),
        "expected dict copy arity diagnostic",
    )
}

#[test]
fn unsupported_slice_forms_reject() -> TestResult {
    let mut ctx = HirCtx::new();
    let step = lower_errors(
        py!(r#"
values: list[int] = [1, 2, 3]
bad: list[int] = values[0:2:1]
"#),
        &mut ctx,
    )?;
    let error = first_error(&step)?;
    ensure(
        error.message.contains("steps"),
        "expected slice step diagnostic",
    )
}

#[test]
fn string_replace_method_lowers() -> TestResult {
    let source = py!(r#"
word: str = "hello hello"
replaced: str = word.replace("hello", "hi")
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
        body.exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                ExprKind::StringReplace {
                    op: StringReplaceOp::All,
                    ..
                }
            )
        }),
        "expected string replace lowering",
    )?;
    Ok(())
}

#[test]
fn string_remove_affix_methods_lower() -> TestResult {
    let source = py!(r#"
word: str = "pre-value-suf"
without_prefix: str = word.removeprefix("pre-")
without_suffix: str = word.removesuffix("-suf")
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
                |expr| matches!(expr.kind, ExprKind::StringRemoveAffix { op, .. } if op == expected),
            ),
            "expected string remove-affix lowering",
        )?;
    }
    Ok(())
}

#[test]
fn string_predicate_methods_lower() -> TestResult {
    let source = py!(r#"
word: str = "abc123"
digits: bool = word.isdigit()
letters: bool = word.isalpha()
alnum: bool = word.isalnum()
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
        StringPredicateOp::IsDigit,
        StringPredicateOp::IsAlpha,
        StringPredicateOp::IsAlnum,
    ] {
        ensure(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::StringPredicate { op, .. } if op == expected),
            ),
            "expected string predicate lowering",
        )?;
    }
    Ok(())
}

#[test]
fn string_join_method_lowers() -> TestResult {
    let source = py!(r#"
parts: list[str] = ["a", "b", "c"]
joined: str = "-".join(parts)
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
            .any(|expr| matches!(expr.kind, ExprKind::StringJoin { .. })),
        "expected string join lowering",
    )?;
    Ok(())
}
