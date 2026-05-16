use super::*;

#[test]
fn list_pop_method_lowers() -> TestResult {
    let source = py!(r#"
values: list[int] = [1, 2]
item: int = values.pop()
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
            .any(|expr| matches!(expr.kind, ExprKind::ListPop { .. })),
        "expected list pop lowering",
    )
}

#[test]
fn collection_clear_methods_lower() -> TestResult {
    let source = py!(r#"
values: list[int] = [1, 2]
list_result: None = values.clear()
mapping: dict[str, int] = {"a": 1}
dict_result: None = mapping.clear()
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
            .any(|expr| matches!(expr.kind, ExprKind::ListClear { .. })),
        "expected list clear lowering",
    )?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictClear { .. })),
        "expected dict clear lowering",
    )
}

#[test]
fn list_copy_method_lowers() -> TestResult {
    let source = py!(r#"
values: list[int] = [1, 2]
copied: list[int] = values.copy()
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
            .any(|expr| matches!(expr.kind, ExprKind::ListCopy { .. })),
        "expected list copy lowering",
    )
}

#[test]
fn container_constructors_lower() -> TestResult {
    let source = py!(r#"
values: list[int] = [1, 2]
copied_values: list[int] = list(values)
value_tuple: tuple[int, int] = tuple(values)
empty_values: list[int] = list()
value_set: set[int] = set(values)
items: set[int] = {1, 2}
copied_items: set[int] = set(items)
empty_items: set[int] = set()
names: dict[str, int] = {"Ada": 1}
copied_names: dict[str, int] = dict(names)
empty_names: dict[str, int] = dict()
pairs: list[tuple[str, int]] = [("Ada", 1)]
pair_names: dict[str, int] = dict(pairs)
coords: tuple[int, int] = (1, 2)
same_coords: tuple[int, int] = tuple(coords)
empty_tuple: tuple[()] = tuple()
item_list: list[int] = list(items)
name_keys: list[str] = list(names)
coord_list: list[int] = list(coords)
coord_set: set[int] = set(coords)
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
            .any(|expr| matches!(expr.kind, ExprKind::ListCopy { .. })),
        "expected list constructor copy lowering",
    )?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::SetCopy { .. })),
        "expected set constructor copy lowering",
    )?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictCopy { .. })),
        "expected dict constructor copy lowering",
    )?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListPairsToDict { .. })),
        "expected dict(list_of_pairs) conversion lowering",
    )?;
    ensure(
        body.exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                ExprKind::SetProjection {
                    op: SetProjectionOp::Values,
                    ..
                }
            )
        }),
        "expected list(set_value) to lower through set values projection",
    )?;
    ensure(
        body.exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                ExprKind::DictProjection {
                    op: DictProjectionOp::Keys,
                    ..
                }
            )
        }),
        "expected list(dict_value) to lower through dict keys projection",
    )?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListToSet { .. })),
        "expected set(list_value) conversion lowering",
    )?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListToTuple { .. })),
        "expected tuple(list_value) conversion lowering",
    )?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::TupleToList { .. })),
        "expected list(tuple_value) conversion lowering",
    )?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::TupleToSet { .. })),
        "expected set(tuple_value) conversion lowering",
    )?;
    ensure(
        body.exprs
            .iter()
            .filter(|expr| matches!(expr.kind, ExprKind::ListLit(ref items) if items.is_empty()))
            .count()
            >= 1,
        "expected empty list constructor lowering",
    )?;
    ensure(
        body.exprs
            .iter()
            .filter(|expr| matches!(expr.kind, ExprKind::SetLit(ref items) if items.is_empty()))
            .count()
            >= 1,
        "expected empty set constructor lowering",
    )?;
    ensure(
        body.exprs
            .iter()
            .filter(
                |expr| matches!(expr.kind, ExprKind::DictLit(ref entries) if entries.is_empty()),
            )
            .count()
            >= 1,
        "expected empty dict constructor lowering",
    )
}

#[test]
fn list_count_method_lowers() -> TestResult {
    let source = py!(r#"
values: list[int] = [1, 2, 1]
count: int = values.count(1)
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
            .any(|expr| matches!(expr.kind, ExprKind::ListCount { .. })),
        "expected list count lowering",
    )
}

#[test]
fn list_index_method_lowers() -> TestResult {
    let source = py!(r#"
values: list[int] = [1, 2, 1]
index: int = values.index(2)
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
            .any(|expr| matches!(expr.kind, ExprKind::ListIndex { .. })),
        "expected list index lowering",
    )
}

#[test]
fn list_remove_method_lowers() -> TestResult {
    let source = py!(r#"
values: list[int] = [1, 2, 1]
result: None = values.remove(2)
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
            .any(|expr| matches!(expr.kind, ExprKind::ListRemove { .. })),
        "expected list remove lowering",
    )
}

#[test]
fn list_sort_method_lowers() -> TestResult {
    let source = py!(r#"
ints: list[int] = [2, 1]
int_result: None = ints.sort()
int_keyword_result: None = ints.sort(reverse=False)
floats: list[float] = [2.0, 1.0]
float_result: None = floats.sort()
float_keyword_result: None = floats.sort(key=None)
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
    let sorts = body
        .exprs
        .iter()
        .filter(|expr| {
            matches!(
                expr.kind,
                ExprKind::ListSort {
                    comparator: None,
                    ..
                }
            )
        })
        .count();
    ensure_eq(&sorts, &4, "list sort count")
}

#[test]
fn dict_pop_method_lowers() -> TestResult {
    let source = py!(r#"
mapping: dict[str, int] = {"a": 1}
value: int = mapping.pop("a")
fallback: int = mapping.pop("b", 0)
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
    let pops = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::DictPop { .. }))
        .count();
    ensure_eq(&pops, &2, "dict pop count")
}

#[test]
fn dict_get_method_lowers() -> TestResult {
    let source = py!(r#"
mapping: dict[str, int] = {"a": 1}
maybe: int | None = mapping.get("a")
fallback: int = mapping.get("b", 0)
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
    let gets = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::DictGet { .. }))
        .count();
    ensure_eq(&gets, &2, "dict get count")
}
