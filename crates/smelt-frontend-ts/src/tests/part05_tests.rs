use super::*;

#[test]
fn lowers_set_mutation_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
let values: Set<number> = new Set([1, 2]);
const same = values.add(3);
const deleted = values.delete(2);
values.clear();
"#),
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
        ts!(r#"
const values: Set<number> = new Set([1, 2]);
const mapping: Map<string, number> = new Map();
const setSize = values.size;
const mapSize = mapping.size;
"#),
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
        ts!(r#"
const values: Set<number> = new Set([1, 2]);
const valueKeys = values.keys();
const valueList = values.values();
const valueEntries = values.entries();
const mapping: Map<string, number> = new Map();
const mapKeys = mapping.keys();
const mapValues = mapping.values();
const mapEntries = mapping.entries();
"#),
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
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
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
fn lowers_set_for_of_to_projection() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values: Set<number> = new Set([1, 2]);
let total = 0;
for (let item: number of values) {
  total = total + item;
}
"#),
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
            .any(|expr| matches!(expr.kind, ExprKind::Index { .. })),
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
