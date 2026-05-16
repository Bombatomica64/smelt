use super::*;

#[test]
fn dict_setdefault_method_lowers() -> TestResult {
    let source = py!(r#"
mapping: dict[str, int] = {"a": 1}
value: int = mapping.setdefault("b", 2)
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
            .any(|expr| matches!(expr.kind, ExprKind::DictSetDefault { .. })),
        "expected dict setdefault lowering",
    )
}

#[test]
fn dict_update_method_lowers() -> TestResult {
    let source = py!(r#"
left: dict[str, int] = {"a": 1}
right: dict[str, int] = {"b": 2}
result: None = left.update(right)
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
            .any(|expr| matches!(expr.kind, ExprKind::DictUpdate { .. })),
        "expected dict update lowering",
    )
}

#[test]
fn dict_copy_method_lowers() -> TestResult {
    let source = py!(r#"
mapping: dict[str, int] = {"a": 1}
copied: dict[str, int] = mapping.copy()
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
            .any(|expr| matches!(expr.kind, ExprKind::DictCopy { .. })),
        "expected dict copy lowering",
    )
}

#[test]
fn unsupported_list_append_forms_reject() -> TestResult {
    let mut ctx = HirCtx::new();
    let wrong_type = lower_errors(
        py!(r#"
values: list[int] = [1, 2]
result: None = values.append("x")
"#),
        &mut ctx,
    )?;
    let error = first_error(&wrong_type)?;
    ensure(
        error.message.contains("argument must match"),
        "expected append type diagnostic",
    )?;

    let mut ctx = HirCtx::new();
    let too_many = lower_errors(
        py!(r#"
values: list[int] = [1, 2]
result: None = values.append(3, 4)
"#),
        &mut ctx,
    )?;
    let error = first_error(&too_many)?;
    ensure(
        error.message.contains("exactly one item"),
        "expected append arity diagnostic",
    )
}

#[test]
fn unsupported_list_extend_forms_reject() -> TestResult {
    let mut ctx = HirCtx::new();
    let wrong_type = lower_errors(
        py!(r#"
left: list[int] = [1, 2]
right: list[str] = ["x"]
result: None = left.extend(right)
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&wrong_type)?
            .message
            .contains("receiver list type"),
        "expected list extend type diagnostic",
    )?;

    let mut ctx = HirCtx::new();
    let wrong_arity = lower_errors(
        py!(r#"
left: list[int] = [1, 2]
result: None = left.extend()
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&wrong_arity)?
            .message
            .contains("exactly one list"),
        "expected list extend arity diagnostic",
    )
}

#[test]
fn unsupported_list_pop_forms_reject() -> TestResult {
    let mut ctx = HirCtx::new();
    let errors = lower_errors(
        py!(r#"
values: list[int] = [1, 2]
item: int = values.pop(0)
"#),
        &mut ctx,
    )?;
    let error = first_error(&errors)?;
    ensure(
        error.message.contains("index arguments"),
        "expected pop index diagnostic",
    )
}

#[test]
fn unsupported_collection_clear_forms_reject() -> TestResult {
    let mut ctx = HirCtx::new();
    let errors = lower_errors(
        py!(r#"
values: list[int] = [1, 2]
result: None = values.clear(1)
"#),
        &mut ctx,
    )?;
    let error = first_error(&errors)?;
    ensure(
        error.message.contains("requires no arguments"),
        "expected clear arity diagnostic",
    )
}

#[test]
fn unsupported_list_copy_forms_reject() -> TestResult {
    let mut ctx = HirCtx::new();
    let errors = lower_errors(
        py!(r#"
values: list[int] = [1, 2]
copied: list[int] = values.copy(1)
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&errors)?
            .message
            .contains("requires no arguments"),
        "expected list copy arity diagnostic",
    )
}

#[test]
fn unsupported_list_count_forms_reject() -> TestResult {
    let mut ctx = HirCtx::new();
    let wrong_type = lower_errors(
        py!(r#"
values: list[int] = [1, 2]
count: int = values.count("x")
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&wrong_type)?.message.contains("element type"),
        "expected list count type diagnostic",
    )?;

    let mut ctx = HirCtx::new();
    let wrong_arity = lower_errors(
        py!(r#"
values: list[int] = [1, 2]
count: int = values.count()
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&wrong_arity)?
            .message
            .contains("exactly one item"),
        "expected list count arity diagnostic",
    )
}

#[test]
fn unsupported_list_index_forms_reject() -> TestResult {
    let mut ctx = HirCtx::new();
    let wrong_type = lower_errors(
        py!(r#"
values: list[int] = [1, 2]
index: int = values.index("x")
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&wrong_type)?.message.contains("element type"),
        "expected list index type diagnostic",
    )?;

    let mut ctx = HirCtx::new();
    let unsupported_bounds = lower_errors(
        py!(r#"
values: list[int] = [1, 2]
index: int = values.index(1, 0)
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&unsupported_bounds)?
            .message
            .contains("exactly one item"),
        "expected list index bounds diagnostic",
    )
}

#[test]
fn unsupported_list_remove_forms_reject() -> TestResult {
    let mut ctx = HirCtx::new();
    let wrong_type = lower_errors(
        py!(r#"
values: list[int] = [1, 2]
result: None = values.remove("x")
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&wrong_type)?.message.contains("element type"),
        "expected list remove type diagnostic",
    )?;

    let mut ctx = HirCtx::new();
    let wrong_arity = lower_errors(
        py!(r#"
values: list[int] = [1, 2]
result: None = values.remove()
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&wrong_arity)?
            .message
            .contains("exactly one item"),
        "expected list remove arity diagnostic",
    )
}

#[test]
fn unsupported_list_sort_forms_reject() -> TestResult {
    let mut ctx = HirCtx::new();
    let positional_arg = lower_errors(
        py!(r#"
values: list[int] = [1, 2]
result: None = values.sort(True)
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&positional_arg)?
            .message
            .contains("no positional arguments"),
        "expected list sort positional arg diagnostic",
    )?;

    let mut ctx = HirCtx::new();
    let keyword_arg = lower_errors(
        py!(r#"
values: list[int] = [1, 2]
result: None = values.sort(reverse=True)
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&keyword_arg)?
            .message
            .contains("key=None and reverse=False"),
        "expected list sort keyword diagnostic",
    )
}

#[test]
fn unsupported_list_insert_forms_reject() -> TestResult {
    let mut ctx = HirCtx::new();
    let wrong_index = lower_errors(
        py!(r#"
values: list[int] = [1, 2]
result: None = values.insert("1", 0)
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&wrong_index)?
            .message
            .contains("index must be int"),
        "expected list insert index diagnostic",
    )?;

    let mut ctx = HirCtx::new();
    let wrong_item = lower_errors(
        py!(r#"
values: list[int] = [1, 2]
result: None = values.insert(1, "x")
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&wrong_item)?.message.contains("element type"),
        "expected list insert item diagnostic",
    )?;

    let mut ctx = HirCtx::new();
    let wrong_arity = lower_errors(
        py!(r#"
values: list[int] = [1, 2]
result: None = values.insert(1)
"#),
        &mut ctx,
    )?;
    ensure(
        first_error(&wrong_arity)?
            .message
            .contains("index and item"),
        "expected list insert arity diagnostic",
    )
}
