use super::*;

#[test]
fn lowers_set_mutation_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
let values: Set<number> = new Set([1, 2]);
const same = values.add(3);
const deleted = values.delete(2);
values.clear();
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::SetAdd { .. })),
        "expected Set.add lowering",
    );
    ensure!(
        body.exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                ExprKind::SetRemove {
                    op: SetRemoveOp::Delete,
                    ..
                }
            )
        }),
        "expected Set.delete lowering",
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::SetClear { .. })),
        "expected Set.clear lowering",
    );
    Ok(())
}

#[test]
fn lowers_map_and_set_size_properties() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const values: Set<number> = new Set([1, 2]);
const mapping: Map<string, number> = new Map();
const setSize = values.size;
const mapSize = mapping.size;
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure_eq!(
        body.exprs
            .iter()
            .filter(|expr| matches!(expr.kind, ExprKind::Len { .. }))
            .count(),
        2,
    );
    Ok(())
}

#[test]
fn lowers_map_and_set_projection_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const values: Set<number> = new Set([1, 2]);
const valueKeys = values.keys();
const valueList = values.values();
const valueEntries = values.entries();
const mapping: Map<string, number> = new Map();
const mapKeys = mapping.keys();
const mapValues = mapping.values();
const mapEntries = mapping.entries();
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure_eq!(
        body.exprs
            .iter()
            .filter(|expr| matches!(expr.kind, ExprKind::DictProjection { .. }))
            .count(),
        3,
    );
    ensure_eq!(
        body.exprs
            .iter()
            .filter(|expr| matches!(
                expr.kind,
                ExprKind::SetProjection {
                    op: SetProjectionOp::Values,
                    ..
                }
            ))
            .count(),
        2,
    );
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            expr.kind,
            ExprKind::SetProjection {
                op: SetProjectionOp::Entries,
                ..
            }
        )),
        "expected Set.entries lowering",
    );
    Ok(())
}

#[test]
fn lowers_map_constructor_has_and_get_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const mapping: Map<string, number> = new Map();
const literal = new Map([["a", 1], ["b", 2]]);
const has = mapping.has("a");
const value = mapping.get("a");
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    let dict_lit_entries = body
        .exprs
        .iter()
        .filter_map(|expr| match &expr.kind {
            ExprKind::DictLit(entries) => Some(entries.len()),
            _ => None,
        })
        .collect::<Vec<_>>();
    ensure!(
        dict_lit_entries.contains(&0) && dict_lit_entries.contains(&2),
        "Map constructors did not lower to expected DictLit entries"
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictContainsKey { .. })),
        "Map.has did not lower to DictContainsKey"
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictGet { .. })),
        "Map.get did not lower to DictGet"
    );
    Ok(())
}

#[test]
fn lowers_untyped_map_has_with_string_key() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
class Container {
  private serviceMap = new Map();

  get(name: string) {
    if (this.serviceMap.has(name)) {
      return this.serviceMap.get(name);
    }
    this.serviceMap.set(name, {});
    return this.serviceMap.get(name);
  }
}
"),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_map_mutation_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
let mapping: Map<string, number> = new Map();
const same = mapping.set("a", 1);
const deleted = mapping.delete("a");
mapping.clear();
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictSet { .. })),
        "expected Map.set lowering",
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictRemoveKey { .. })),
        "expected Map.delete lowering",
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictClear { .. })),
        "expected Map.clear lowering",
    );
    Ok(())
}

#[test]
fn lowers_string_split_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "a,b,c";
const parts = word.split(",");
const limited = word.split(",", 2);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::StringSplit { .. }))
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::StringSplit { limit: Some(_), .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn preserves_regexp_separator_from_static_object_for_string_split() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function parts(value: string) {
  return value.split(patterns.separator);
}
const patterns = { separator: /[T ]/i };
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = function_item(&ctx, module, 0).and_then(|function| function_body(&ctx, function))?;
    let split_separator = body.exprs.iter().find_map(|expr| match &expr.kind {
        ExprKind::StringSplit { separator, .. } => Some(*separator),
        _ => None,
    });
    let separator = split_separator.ok_or_else(|| "expected StringSplit expression".to_owned())?;
    let separator_ty = body
        .exprs
        .get(usize::try_from(separator.0).map_err(|error| error.to_string())?)
        .ok_or_else(|| "missing split separator expression".to_owned())?
        .ty;
    ensure!(
        matches!(
            ctx.krate.types.get(separator_ty),
            Some(Type::Class { name, .. })
                if ctx.krate.symbols.get(*name).is_some_and(|name| name == "RegExp")
        ),
        "expected object-held regex separator to retain RegExp type"
    );
    Ok(())
}

#[test]
fn rejects_unknown_identifier() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(ts!("console.log(x);"), &mut ctx)?;
    assert_unsupported_ts(&errors, "unresolved identifier")
}

#[test]
fn formats_compact_hir() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("let count = 42;
console.log(count);
"),
        &mut ctx,
    )?;

    let output = smelt_hir::format_compact(&ctx.krate, &[("sample.ts".to_owned(), module_id)]);

    ensure_eq!(
        output,
        "module sample.ts (ModuleId(0))\n  body BodyId(0)\n  locals\n    %0 let count: Float\n  exprs\n    #0: Float = 42.0\n    #1: Float = %0\n    #2: None = @0(console_log)\n    #3: None = call #2(#1)\n  stmts\n    s0: let %0: Float = #0\n    s1: #3\n\ninterned types\n  t0 = Float\n  t1 = None\n"
    );
    Ok(())
}

#[test]
fn normalizes_camel_case() -> Result<(), String> {
    ensure_eq!(camel_to_snake("myFunction"), "my_function");
    ensure_eq!(camel_to_snake("URLParser"), "url_parser");
    ensure_eq!(camel_to_snake("IPAddr"), "ip_addr");
    ensure_eq!(camel_to_snake("_internal"), "_internal");
    Ok(())
}

#[test]
fn lowers_function_declaration_and_direct_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("function add(a: number, b: number): number {
  return a + b;
}
const result = add(2, 3);
console.log(result);
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;

    ensure_eq!(module.items.len(), 1);
    ensure_eq!(ctx.krate.items.len(), 2);
    ensure_eq!(ctx.krate.bodies.len(), 2);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_generator_yields_into_materialized_unknown_array() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function* values(limit: number): Generator<number> {
  for (let i = 0; i < limit; i += 1) {
    yield i;
  }
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure_eq!(module.items.len(), 1);
    let function = ctx
        .krate
        .items
        .iter()
        .find_map(|item| match item {
            Item::Function(function) if ctx.krate.symbols.get(function.name) == Some("values") => {
                Some(function)
            }
            _ => None,
        })
        .ok_or_else(|| "missing generator function".to_owned())?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListPush { .. })),
        "yield should append to the synthetic generator list"
    );
    ensure!(
        body.stmts.iter().any(|stmt| matches!(
            stmt,
            Stmt::Return(Some(value))
                if matches!(
                    body.exprs.get(value.0 as usize).map(|expr| &expr.kind),
                    Some(ExprKind::UnknownCast { .. })
                )
        )),
        "generator should return an erased iterable value"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_if_else_while_and_for_of_to_hir() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("let count = 0;
if (count < 10) {
  console.log(count);
} else {
  console.log(count);
}
while (count < 10) {
  break;
}
for (let item: number of count) {
  continue;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::If { .. }))
    );
    ensure!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::While { .. }))
    );
    ensure!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::For { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn c_style_for_lowers_update_into_separate_block_not_body() -> Result<(), String> {
    // Regression: a C-style `for (init; test; update)` with a `continue` in the
    // body must NOT append the update to the loop body — otherwise `continue`
    // (which jumps to the loop header) skips the update and spins forever. The
    // update must live in its own block (`WhileUpdateBlock.update`) so MIR can
    // make it the `continue` target.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
for (let i = 0; i < 10; i++) {
  if (i === 3) {
    continue;
  }
  console.log(i);
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    let Some(Stmt::WhileUpdateBlock {
        body: loop_body,
        update,
        ..
    }) = body
        .stmts
        .iter()
        .find(|stmt| matches!(stmt, Stmt::WhileUpdateBlock { .. }))
    else {
        return Err("expected C-style for to lower to WhileUpdateBlock".to_owned());
    };

    let is_assign = |block_id: &smelt_hir::BlockId| -> Result<bool, String> {
        let block = body
            .blocks
            .get(usize::try_from(block_id.0).map_err(|err| err.to_string())?)
            .ok_or_else(|| "expected block".to_owned())?;
        Ok(block.stmts.iter().any(|stmt| {
            usize::try_from(stmt.0)
                .is_ok_and(|index| matches!(body.stmts.get(index), Some(Stmt::Assign { .. })))
        }))
    };

    // The update assignment (`i++` -> `i = i + 1`) must be in the update block,
    // and NOT duplicated into the loop body.
    ensure!(
        is_assign(update)?,
        "update block should contain the loop-update assignment",
    );
    ensure!(
        !is_assign(loop_body)?,
        "loop body must not contain the update assignment (would be skipped by continue)",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_set_for_of_to_projection() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const values: Set<number> = new Set([1, 2]);
let total = 0;
for (let item: number of values) {
  total = total + item;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs.iter().any(|expr| matches!(
            expr.kind,
            ExprKind::SetProjection {
                op: SetProjectionOp::Values,
                ..
            }
        )),
        "expected set for-of projection",
    );
    ensure!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::For { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_map_for_of_to_entries_projection() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const mapping: Map<string, number> = new Map([["a", 1], ["b", 2]]);
let last: [string, number] = ["", 0];
for (const entry: [string, number] of mapping) {
  last = entry;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs.iter().any(|expr| matches!(
            expr.kind,
            ExprKind::DictProjection {
                op: DictProjectionOp::Entries,
                ..
            }
        )),
        "expected Map for-of to project entries",
    );
    ensure!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::For { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_destructured_for_of_bindings() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const pairs: [string, number][] = [["a", 1]];
let total: number = 0;
for (const [key, value] of pairs) {
  total = total + value;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::TupleIndex { .. })),
        "expected destructured for-of binding to lower tuple indexes",
    );
    ensure!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::For { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_static_tuple_index() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const pair: [string, number] = ["Ada", 1];
const name = pair[0];
const count = pair[1];
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    let indexes = body
        .exprs
        .iter()
        .filter_map(|expr| match expr.kind {
            ExprKind::TupleIndex { index, .. } => Some(index),
            _ => None,
        })
        .collect::<Vec<_>>();
    ensure!(
        indexes == [0, 1],
        "expected static tuple index lowering for both tuple fields"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_negative_array_bracket_read_to_undefined() -> Result<(), String> {
    // `arr[-1]` is a JavaScript property lookup that never names an element, so
    // it lowers to an honest optional `None` (undefined) instead of rejecting or
    // wrapping like `.at(-1)`.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const values: number[] = [1, 2, 3];
const missing = values[-1];
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    let none_index_read = body.exprs.iter().find(|expr| {
        matches!(expr.kind, ExprKind::Literal(Literal::None))
            && matches!(ctx.krate.types.get(expr.ty), Some(Type::Optional(_)))
    });
    ensure!(
        none_index_read.is_some(),
        "expected negative array bracket read to lower to an optional None literal",
    );
    // No element `Index` read should be emitted for the negative access.
    ensure!(
        !body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Index { .. })),
        "negative array bracket read must not emit an element Index access",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_negative_string_bracket_read_to_undefined() -> Result<(), String> {
    // `str[-1]` mirrors the array case: an out-of-range property lookup yields
    // `undefined`, lowered to an optional `None`.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const text = "hello";
const missing = text[-1];
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs.iter().any(|expr| {
            matches!(expr.kind, ExprKind::Literal(Literal::None))
                && matches!(ctx.krate.types.get(expr.ty), Some(Type::Optional(_)))
        }),
        "expected negative string bracket read to lower to an optional None literal",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_negative_tuple_bracket_read_to_undefined() -> Result<(), String> {
    // A negative tuple bracket index is a property lookup too, so it lowers to
    // undefined rather than a `TupleIndex` field access.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const pair: [string, number] = ["Ada", 1];
const missing = pair[-1];
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs.iter().any(|expr| {
            matches!(expr.kind, ExprKind::Literal(Literal::None))
                && matches!(ctx.krate.types.get(expr.ty), Some(Type::Optional(_)))
        }),
        "expected negative tuple bracket read to lower to an optional None literal",
    );
    ensure!(
        !body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::TupleIndex { .. })),
        "negative tuple bracket read must not emit a TupleIndex field access",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_negative_array_bracket_write_as_noop() -> Result<(), String> {
    // `arr[-1] = value` sets a string-keyed property that does not change the
    // array's elements, so the write is a no-op on the collection. The
    // right-hand side is still evaluated (as a discarded expression statement)
    // to preserve its side effects and no element assignment is produced.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const values: number[] = [1, 2, 3];
values[-1] = 99;
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        !body.stmts.iter().any(|stmt| matches!(
            stmt,
            Stmt::Assign { target, .. }
                if usize::try_from(target.0)
                    .ok()
                    .and_then(|index| body.exprs.get(index))
                    .is_some_and(|expr| matches!(expr.kind, ExprKind::Index { .. }))
        )),
        "negative array bracket write must not emit an element assignment",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn rejects_dynamic_tuple_index() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r#"
const pair: [string, number] = ["Ada", 1];
const index = 1;
const value = pair[index];
"#),
        &mut ctx,
    )?;

    assert_unsupported_ts(
        &errors,
        "tuple indexing requires a static non-negative integer index",
    )
}

#[test]
fn lowers_try_catch_finally_to_hir() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("try {
  throw 'x';
} catch (error) {
  console.log(error);
} finally {
  console.log('done');
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    let Some(Stmt::TryCatch {
        body: try_body,
        catch_binding: Some(_),
        catch_body: Some(catch_body),
        finally_body: Some(finally_body),
    }) = body
        .stmts
        .iter()
        .find(|stmt| matches!(stmt, Stmt::TryCatch { .. }))
    else {
        return Err("expected try/catch/finally to lower to HIR".to_owned());
    };
    let try_block = body
        .blocks
        .get(
            usize::try_from(try_body.0)
                .map_err(|err| format!("try block id {try_body:?} does not fit in usize: {err}"))?,
        )
        .ok_or_else(|| format!("missing try block {try_body:?}"))?;
    let catch_block =
        body.blocks
            .get(usize::try_from(catch_body.0).map_err(|err| {
                format!("catch block id {catch_body:?} does not fit in usize: {err}")
            })?)
            .ok_or_else(|| format!("missing catch block {catch_body:?}"))?;
    let finally_block = body
        .blocks
        .get(usize::try_from(finally_body.0).map_err(|err| {
            format!("finally block id {finally_body:?} does not fit in usize: {err}")
        })?)
        .ok_or_else(|| format!("missing finally block {finally_body:?}"))?;
    ensure!(try_block.stmts.iter().any(|stmt| {
        usize::try_from(stmt.0)
            .is_ok_and(|stmt_index| matches!(body.stmts.get(stmt_index), Some(Stmt::Throw(_))))
    }));
    ensure!(!catch_block.stmts.is_empty());
    ensure!(!finally_block.stmts.is_empty());
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn rejects_missing_implemented_interface_field() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!("interface Named { name: string; }
class User implements Named {
  constructor() {}
}
"),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "field `name`")
}

#[test]
fn rejects_implemented_method_signature_mismatch() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!("interface Named { label(prefix: string): string; }
class User implements Named {
  label(prefix: number): string { return \"x\"; }
}
"),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "mismatched signature")
}

/// Return whether the lowered crate contains a test function with the given
/// sanitized Rust name.
fn has_test_named(ctx: &HirCtx, name: &str) -> bool {
    ctx.krate.items.iter().any(|item| {
        matches!(item, Item::Function(function)
            if function.is_test && ctx.krate.symbols.get(function.name) == Some(name))
    })
}

#[test]
fn describe_body_class_declaration_registers_suite_helper_type() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import { describe, expect, it } from "vitest";

describe("pairs", () => {
  class Pair {
    constructor(
      public a: number,
      public b: number,
    ) {}
  }

  it("builds a pair", () => {
    const pair = new Pair(1, 2);
    expect(pair.a).toBe(1);
  });
});
"#),
        &mut ctx,
    )?;
    ensure!(
        ctx.krate.items.iter().any(|item| matches!(item, Item::Class(class)
            if ctx.krate.symbols.get(class.name) == Some("Pair"))),
        "a class declared in a describe body should register as a suite-level class",
    );
    ensure!(
        has_test_named(&ctx, "test_pairs_builds_a_pair"),
        "the suite test case should still lower alongside the helper class",
    );
    Ok(())
}

#[test]
fn test_title_folds_suite_const_string_interpolation() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import { describe, expect, it } from "vitest";

describe("pull", () => {
  const methodName = "pull";

  it(`\`_.${methodName}\` should work`, () => {
    expect(1).toBe(1);
  });
});
"#),
        &mut ctx,
    )?;
    ensure!(
        has_test_named(&ctx, "test_pull_pull_should_work"),
        "a suite const-string interpolation should fold into the test name",
    );
    Ok(())
}

#[test]
fn test_title_folds_loop_conditional_interpolation() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import { describe, expect, it } from "vitest";

describe("throttle", () => {
  [0, 1].forEach(index => {
    it(`should call${index ? " and reset" : ""}`, () => {
      expect(index).toBe(index);
    });
  });
});
"#),
        &mut ctx,
    )?;
    ensure!(
        has_test_named(&ctx, "test_throttle_case_0_should_call"),
        "the falsy-index iteration should fold the empty conditional branch",
    );
    ensure!(
        has_test_named(&ctx, "test_throttle_case_1_should_call_and_reset"),
        "the truthy-index iteration should fold the non-empty conditional branch",
    );
    Ok(())
}

#[test]
fn test_title_folds_const_array_index_interpolation() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import { describe, expect, it } from "vitest";

describe("findLast", () => {
  const expected = [1, undefined, 3];

  it(`returns \`${expected[1]}\` if missing`, () => {
    expect(1).toBe(1);
  });
});
"#),
        &mut ctx,
    )?;
    ensure!(
        has_test_named(&ctx, "test_findlast_returns_undefined_if_missing"),
        "indexing a const array literal by a literal index should fold into the title",
    );
    Ok(())
}

#[test]
fn expect_to_throw_accepts_erased_callable_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    // `makeThrower` resolves to an erased callable here (no concrete function
    // type), as a cross-module helper would; `toThrow` must still lower.
    lower_path_ok(
        ts!(r#"
import { describe, expect, it } from "vitest";
import { makeThrower } from "./makeThrower";

describe("once", () => {
  it("throws", () => {
    const resultFunc = makeThrower();
    expect(resultFunc).toThrow();
  });
});
"#),
        "src/once.spec.ts",
        &mut ctx,
    )?;
    ensure!(
        has_test_named(&ctx, "test_once_throws"),
        "expect(value).toThrow() over an erased callable should lower the test",
    );
    Ok(())
}

#[test]
fn expect_to_contain_accepts_erased_expected_in_collection() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    // `pick(array)` resolves to an erased value; toContain over a concrete list
    // actual must still lower via the runtime containment check.
    lower_path_ok(
        ts!(r#"
import { describe, expect, it } from "vitest";
import { pick } from "./pick";

describe("sample", () => {
  const array = [1, 2, 3, 4, 5];

  it("contains the picked element", () => {
    const actual = pick(array);
    expect(array).toContain(actual);
  });
});
"#),
        "src/sample.spec.ts",
        &mut ctx,
    )?;
    ensure!(
        has_test_named(&ctx, "test_sample_contains_the_picked_element"),
        "expect(collection).toContain(erased) should lower the test",
    );
    Ok(())
}

#[test]
fn expect_to_contain_accepts_erased_actual_collection() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    // `keysIn(value)` resolves to an erased value when the helper's module is
    // not part of this lowering unit; toContain must project the erased actual
    // to an erased list and run the containment check at runtime instead of
    // rejecting the matcher.
    lower_path_ok(
        ts!(r#"
import { describe, expect, it } from "vitest";
import { keysIn } from "./keysIn";

describe("sample", () => {
  it("does not expose buffer keys", () => {
    const actual = keysIn({ a: 1 });
    expect(actual).not.toContain("offset");
  });
});
"#),
        "src/sample.spec.ts",
        &mut ctx,
    )?;
    ensure!(
        has_test_named(&ctx, "test_sample_does_not_expose_buffer_keys"),
        "expect(erased).toContain(value) should lower the test",
    );
    ensure!(
        ctx.krate
            .bodies
            .iter()
            .flat_map(|body| body.exprs.iter())
            .any(|expr| matches!(expr.kind, ExprKind::ListContains { .. })),
        "erased actual should lower to a runtime list containment check",
    );
    Ok(())
}

#[test]
fn lowers_new_map_with_declared_union_value_type() -> Result<(), String> {
    // A `Map<K, V>` annotation whose value type is a union should accept
    // heterogeneous `[key, value]` entries: each entry coerces to the declared
    // union value type instead of requiring a single homogeneous value type.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function tags(): Map<string, string | number> {
  return new Map<string, string | number>([["a", 1], ["b", "two"]]);
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;

    let dict_ty = body
        .exprs
        .iter()
        .find_map(|expr| match &expr.kind {
            ExprKind::DictLit(entries) if entries.len() == 2 => Some(expr.ty),
            _ => None,
        })
        .ok_or("expected a two-entry Map DictLit")?;
    let Some(Type::Dict(key_ty, value_ty)) = ctx.krate.types.get(dict_ty) else {
        return Err("Map literal type must be a Dict".to_owned());
    };
    ensure!(
        ctx.krate.types.get(*key_ty) == Some(&Type::String),
        "declared Map key type should stay String",
    );
    ensure!(
        matches!(ctx.krate.types.get(*value_ty), Some(Type::Union(_))),
        "declared Map value type should preserve the union annotation",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_new_map_with_inferred_union_value_type() -> Result<(), String> {
    // Without a `Map<K, V>` annotation, mixed entry value types are widened to
    // the union of the observed types, mirroring array-literal inference rather
    // than being rejected as non-homogeneous.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const mapping = new Map([["a", 1], ["b", "two"]]);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    let dict_ty = body
        .exprs
        .iter()
        .find_map(|expr| match &expr.kind {
            ExprKind::DictLit(entries) if entries.len() == 2 => Some(expr.ty),
            _ => None,
        })
        .ok_or("expected a two-entry Map DictLit")?;
    let Some(Type::Dict(key_ty, value_ty)) = ctx.krate.types.get(dict_ty) else {
        return Err("Map literal type must be a Dict".to_owned());
    };
    ensure!(
        ctx.krate.types.get(*key_ty) == Some(&Type::String),
        "homogeneous string keys should infer a String key type",
    );
    ensure!(
        matches!(
            ctx.krate.types.get(*value_ty),
            Some(Type::Union(_) | Type::Unknown)
        ),
        "mixed entry values should infer a union (or erased) value type",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_new_map_with_homogeneous_entries() -> Result<(), String> {
    // Regression: homogeneous entries still infer a single shared value type.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const mapping = new Map([["a", 1], ["b", 2]]);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    let dict_ty = body
        .exprs
        .iter()
        .find_map(|expr| match &expr.kind {
            ExprKind::DictLit(entries) if entries.len() == 2 => Some(expr.ty),
            _ => None,
        })
        .ok_or("expected a two-entry Map DictLit")?;
    let Some(Type::Dict(key_ty, value_ty)) = ctx.krate.types.get(dict_ty) else {
        return Err("Map literal type must be a Dict".to_owned());
    };
    ensure!(
        ctx.krate.types.get(*key_ty) == Some(&Type::String),
        "homogeneous string keys should infer a String key type",
    );
    ensure!(
        ctx.krate.types.get(*value_ty) == Some(&Type::Float),
        "homogeneous numeric values should infer a single Float value type",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// `sample(...)`-style helpers return `T | undefined`, so
/// `expect(collection).toContain(sample(collection))` passes an optional needle
/// whose inner type matches the collection element type. JavaScript containment
/// compares the needle against each element regardless of nullability, so the
/// optional expected is accepted (the emitter guards the `undefined` at runtime)
/// instead of being rejected with "requires a string, array, set, or tuple
/// actual value with a matching expected value". es-toolkit's `sample` spec
/// exercises this over both list and tuple actuals.
#[test]
fn expect_to_contain_accepts_optional_expected_in_collection() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r#"
import { describe, expect, it } from "vitest";

function firstNum(items: number[]): number | undefined {
  if (items.length === 0) {
    return undefined;
  }
  return items[0];
}

describe("sample", () => {
  it("list actual with optional needle", () => {
    const values: number[] = [10, 20, 30];
    const picked = firstNum(values);
    expect(values).toContain(picked);
  });

  it("tuple actual with optional needle", () => {
    const picked = firstNum([1, 2, 3]);
    expect([1, 2, 3]).toContain(picked);
  });
});
"#),
        "src/sample.spec.ts",
        &mut ctx,
    )?;
    ensure!(
        has_test_named(&ctx, "test_sample_list_actual_with_optional_needle"),
        "list toContain with an optional needle should lower the test",
    );
    ensure!(
        has_test_named(&ctx, "test_sample_tuple_actual_with_optional_needle"),
        "tuple toContain with an optional needle should lower the test",
    );
    Ok(())
}
