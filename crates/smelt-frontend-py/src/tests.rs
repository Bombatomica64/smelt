//! Unit tests for the Python frontend.

use crate::{HirCtx, SmeltError, to_hir};
use smelt_hir::{
    AsyncOp, Body, BodyId, BoolFoldOp, DictProjectionOp, ExprKind, FileId, Item, ItemId, Language,
    Module, ModuleId, NumericExtremaOp, NumericPredicateOp, NumericRoundOp, NumericUnaryFuncOp,
    Pattern, PatternId, RegexMatchOp, SetBinaryOp, SetRemoveOp, Stmt, StringAffixOp, StringCaseOp,
    StringPredicateOp, StringReplaceOp, StringSearchOp, StringTrimSide, Symbol, Type,
};
use std::convert::TryFrom;

type TestResult = Result<(), String>;

/// Marks a test fixture string as Python source code.
macro_rules! py {
    ($source:literal $(,)?) => {
        $source
    };
}

/// Lowers `source` into HIR and returns the module ID.
fn lower_module(source: &str, ctx: &mut HirCtx) -> Result<ModuleId, String> {
    to_hir(source, FileId(0), ctx)
        .map_err(|errors| format!("expected successful lowering, got {errors:?}"))
}

/// Lowers `source` and returns the diagnostics produced by the frontend.
fn lower_errors(source: &str, ctx: &mut HirCtx) -> Result<Vec<SmeltError>, String> {
    match to_hir(source, FileId(0), ctx) {
        Ok(module_id) => Err(format!(
            "expected lowering to fail, got module {module_id:?}"
        )),
        Err(errors) => Ok(errors),
    }
}

/// Returns the first diagnostic from `errors`.
fn first_error(errors: &[SmeltError]) -> Result<&SmeltError, String> {
    errors
        .first()
        .ok_or_else(|| "expected at least one diagnostic".to_owned())
}

/// Fails the test if `condition` is false.
fn ensure(condition: bool, message: impl Into<String>) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

/// Fails the test if `left` and `right` are not equal.
fn ensure_eq<T>(left: &T, right: &T, context: &str) -> Result<(), String>
where
    T: PartialEq + std::fmt::Debug,
{
    if left == right {
        Ok(())
    } else {
        Err(format!("{context}: left={left:?}, right={right:?}"))
    }
}

/// Looks up a module by ID.
fn module(ctx: &HirCtx, module_id: ModuleId) -> Result<&Module, String> {
    let idx = usize::try_from(module_id.0)
        .map_err(|error| format!("missing module {module_id:?}: {error}"))?;
    ctx.krate
        .modules
        .get(idx)
        .ok_or_else(|| format!("missing module {module_id:?}"))
}

/// Looks up an item by ID.
fn item(ctx: &HirCtx, item_id: ItemId) -> Result<&Item, String> {
    let idx =
        usize::try_from(item_id.0).map_err(|error| format!("missing item {item_id:?}: {error}"))?;
    ctx.krate
        .items
        .get(idx)
        .ok_or_else(|| format!("missing item {item_id:?}"))
}

/// Looks up a body by ID.
fn body(ctx: &HirCtx, body_id: BodyId) -> Result<&Body, String> {
    let idx =
        usize::try_from(body_id.0).map_err(|error| format!("missing body {body_id:?}: {error}"))?;
    ctx.krate
        .bodies
        .get(idx)
        .ok_or_else(|| format!("missing body {body_id:?}"))
}

/// Looks up a pattern by ID within `body`.
fn pattern(body: &Body, pattern_id: PatternId) -> Result<&Pattern, String> {
    let idx = usize::try_from(pattern_id.0)
        .map_err(|error| format!("missing pattern {pattern_id:?}: {error}"))?;
    body.patterns
        .get(idx)
        .ok_or_else(|| format!("missing pattern {pattern_id:?}"))
}

/// Resolves a symbol back to its interned name.
fn symbol(ctx: &HirCtx, symbol: Symbol) -> Result<&str, String> {
    ctx.krate
        .symbols
        .get(symbol)
        .ok_or_else(|| format!("missing symbol {symbol:?}"))
}

#[test]
fn empty_module_lowers_to_empty_hir() -> TestResult {
    let mut ctx = HirCtx::new();
    let module_id = lower_module(py!(""), &mut ctx)?;
    let module = module(&ctx, module_id)?;
    ensure_eq(
        &module.source.language,
        &Language::Python,
        "module language",
    )?;
    ensure(module.items.is_empty(), "expected empty module")?;
    Ok(())
}

#[test]
fn parse_error_is_reported() -> TestResult {
    let mut ctx = HirCtx::new();
    let errors = lower_errors(py!("x = \"oops"), &mut ctx)?;
    ensure(!errors.is_empty(), "expected parse error")?;
    ensure_eq(
        &first_error(&errors)?.code,
        &"smelt::parse-error-py",
        "parse error code",
    )?;
    Ok(())
}

#[test]
fn simple_function_lowers() -> TestResult {
    let source = py!(r#"
def add(x: int, y: int) -> int:
    return x + y
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    ensure_eq(&module.items.len(), &1, "item count")?;

    let item_id = module
        .items
        .first()
        .copied()
        .ok_or_else(|| "expected function item".to_owned())?;
    if let Item::Function(f) = item(&ctx, item_id)? {
        ensure_eq(&symbol(&ctx, f.name)?, &"add", "function name")?;
        ensure_eq(&f.params.len(), &2, "parameter count")?;
        ensure(f.body.is_some(), "expected function body")?;
    } else {
        return Err("expected Function item".to_owned());
    }
    Ok(())
}

#[test]
fn async_function_and_await_lower() -> TestResult {
    let source = py!(r#"
async def lift(value: int) -> int:
    return value

async def run() -> int:
    return await lift(5)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;

    ensure_eq(&module.items.len(), &2, "item count")?;

    let lift_id = module
        .items
        .first()
        .copied()
        .ok_or_else(|| "expected lift function".to_owned())?;
    let Item::Function(lift) = item(&ctx, lift_id)? else {
        return Err("expected function item for lift".to_owned());
    };
    ensure(lift.is_async, "lift should be async")?;
    ensure(
        matches!(ctx.krate.types.get(lift.return_ty), Some(Type::Future(_))),
        "lift return type should be Future",
    )?;

    let run_id = module
        .items
        .get(1)
        .copied()
        .ok_or_else(|| "expected run function".to_owned())?;
    let Item::Function(run) = item(&ctx, run_id)? else {
        return Err("expected function item for run".to_owned());
    };
    ensure(run.is_async, "run should be async")?;
    let body_id = run.body.ok_or_else(|| "expected run body".to_owned())?;
    let body = body(&ctx, body_id)?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Await(_))),
        "expected await expression",
    )?;
    let state_machine = body
        .async_state_machine
        .as_ref()
        .ok_or_else(|| "async body should record suspension metadata".to_owned())?;
    ensure_eq(&state_machine.suspensions.len(), &1, "suspension count")?;
    Ok(())
}

#[test]
fn await_outside_async_function_is_rejected() -> TestResult {
    let source = py!(r#"
async def lift(value: int) -> int:
    return value

def run() -> int:
    return await lift(5)
"#);
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    let error = first_error(&errors)?;
    ensure_eq(&error.code, &"smelt::unsupported-py", "error code")?;
    ensure(
        error.message.contains("inside async functions"),
        "expected async-only message",
    )?;
    Ok(())
}

#[test]
fn asyncio_gather_and_sleep_lower_to_async_ops() -> TestResult {
    let source = py!(r#"
import asyncio

async def lift(value: int) -> int:
    await asyncio.sleep(0)
    return value

async def run() -> tuple[int, int]:
    return await asyncio.gather(lift(1), lift(2))
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;

    let lift_id = module
        .items
        .first()
        .copied()
        .ok_or_else(|| "expected lift function".to_owned())?;
    let Item::Function(lift) = item(&ctx, lift_id)? else {
        return Err("expected function item for lift".to_owned());
    };
    let lift_body_id = lift.body.ok_or_else(|| "expected lift body".to_owned())?;
    let lift_body = body(&ctx, lift_body_id)?;
    ensure(
        lift_body.exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                ExprKind::AsyncOp {
                    op: AsyncOp::Sleep,
                    ..
                }
            )
        }),
        "expected asyncio.sleep lowering",
    )?;

    let run_id = module
        .items
        .get(1)
        .copied()
        .ok_or_else(|| "expected run function".to_owned())?;
    let Item::Function(run) = item(&ctx, run_id)? else {
        return Err("expected function item for run".to_owned());
    };
    let run_body_id = run.body.ok_or_else(|| "expected run body".to_owned())?;
    let run_body = body(&ctx, run_body_id)?;
    ensure(
        run_body.exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                ExprKind::AsyncOp {
                    op: AsyncOp::All,
                    ..
                }
            )
        }),
        "expected asyncio.gather lowering",
    )?;
    Ok(())
}

#[test]
fn lower_level_asyncio_loop_apis_are_rejected() -> TestResult {
    let source = py!(r#"
import asyncio

def run() -> None:
    asyncio.get_event_loop()
"#);
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    let error = first_error(&errors)?;
    ensure_eq(&error.code, &"smelt::unsupported-py", "error code")?;
    ensure(
        error.message.contains("lower-level event-loop API"),
        "expected lower-level event-loop message",
    )?;
    Ok(())
}

#[test]
fn asyncio_task_wait_for_and_runtime_objects_are_classified() -> TestResult {
    let source = py!(r#"
import asyncio

async def lift(value: int) -> int:
    return value

async def run() -> int:
    task: Awaitable[int] = asyncio.create_task(lift(1))
    return await asyncio.wait_for(task, 10)
"#);
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;

    let queue_source = py!(r#"
import asyncio

def run() -> None:
    asyncio.Queue()
"#);
    let mut queue_ctx = HirCtx::new();
    let errors = lower_errors(queue_source, &mut queue_ctx)?;
    ensure(
        first_error(&errors)?
            .message
            .contains("runtime object support"),
        "expected runtime object support message",
    )?;
    Ok(())
}

#[test]
fn annotated_assignment_lowers() -> TestResult {
    let source = py!("x: int = 42\n");
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body_id = module
        .body
        .ok_or_else(|| "expected module body".to_owned())?;
    let body = body(&ctx, body_id)?;
    ensure(!body.stmts.is_empty(), "expected body statements")?;
    Ok(())
}

#[test]
fn type_annotations_lowered() -> TestResult {
    let source = py!(r#"
def process(items: list[str], counts: dict[str, int]) -> bool:
    return True
"#);
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    Ok(())
}

#[test]
fn optional_annotation_lowered() -> TestResult {
    let source = py!(r#"
def find(x: int) -> str | None:
    return None
"#);
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    Ok(())
}

#[test]
fn missing_return_annotation_is_error() -> TestResult {
    let source = py!("def bad(x: int):\n    return x\n");
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    ensure_eq(
        &first_error(&errors)?.code,
        &"smelt::unsupported-py",
        "error code",
    )?;
    Ok(())
}

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
empty_values: list[int] = list()
items: set[int] = {1, 2}
copied_items: set[int] = set(items)
empty_items: set[int] = set()
names: dict[str, int] = {"Ada": 1}
copied_names: dict[str, int] = dict(names)
empty_names: dict[str, int] = dict()
coords: tuple[int, int] = (1, 2)
same_coords: tuple[int, int] = tuple(coords)
empty_tuple: tuple[()] = tuple()
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
floats: list[float] = [2.0, 1.0]
float_result: None = floats.sort()
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
        .filter(|expr| matches!(expr.kind, ExprKind::ListSort { .. }))
        .count();
    ensure_eq(&sorts, &2, "list sort count")
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
            .contains("no arguments"),
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
        first_error(&keyword_arg)?.message.contains("no arguments"),
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
ordered: list[int] = sorted(values, reverse=True)
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

#[test]
fn dict_projection_methods_lower() -> TestResult {
    let source = py!(r#"
mapping: dict[str, int] = {"a": 1, "b": 2}
keys: list[str] = mapping.keys()
values: list[int] = mapping.values()
items: list[tuple[str, int]] = mapping.items()
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
        DictProjectionOp::Keys,
        DictProjectionOp::Values,
        DictProjectionOp::Entries,
    ] {
        ensure(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::DictProjection { op, .. } if op == expected),
            ),
            "expected dict projection lowering",
        )?;
    }
    Ok(())
}

#[test]
fn math_numeric_functions_lower() -> TestResult {
    let source = py!(r#"
import math
value: float = 4.0
root: float = math.sqrt(value)
sin_value: float = math.sin(value)
cos_value: float = math.cos(value)
tan_value: float = math.tan(value)
asin_value: float = math.asin(value)
acos_value: float = math.acos(value)
atan_value: float = math.atan(value)
atan2_value: float = math.atan2(value, 2.0)
log_value: float = math.log(value)
log10_value: float = math.log10(value)
log2_value: float = math.log2(value)
exp_value: float = math.exp(value)
raised: float = math.pow(value, 2.0)
floored: int = math.floor(value)
ceiled: int = math.ceil(value)
whole: int = math.trunc(value)
finite: bool = math.isfinite(value)
nan_value: bool = math.isnan(value)
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
        NumericUnaryFuncOp::Sqrt,
        NumericUnaryFuncOp::Sin,
        NumericUnaryFuncOp::Cos,
        NumericUnaryFuncOp::Tan,
        NumericUnaryFuncOp::Asin,
        NumericUnaryFuncOp::Acos,
        NumericUnaryFuncOp::Atan,
        NumericUnaryFuncOp::Log,
        NumericUnaryFuncOp::Log10,
        NumericUnaryFuncOp::Log2,
        NumericUnaryFuncOp::Exp,
    ] {
        ensure(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::NumericUnaryFunc { op, .. } if op == expected),
            ),
            "expected math unary lowering",
        )?;
    }
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::NumericPow { .. })),
        "expected math.pow lowering",
    )?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::NumericAtan2 { .. })),
        "expected math.atan2 lowering",
    )?;
    for expected in [
        NumericRoundOp::Floor,
        NumericRoundOp::Ceil,
        NumericRoundOp::Trunc,
    ] {
        ensure(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::NumericRound { op, .. } if op == expected),
            ),
            "expected math rounding lowering",
        )?;
    }
    for expected in [NumericPredicateOp::IsFinite, NumericPredicateOp::IsNaN] {
        ensure(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::NumericPredicate { op, .. } if op == expected),
            ),
            "expected math predicate lowering",
        )?;
    }
    Ok(())
}

#[test]
fn random_module_functions_lower() -> TestResult {
    let source = py!(r#"
import random
sample: float = random.random()
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
            .any(|expr| matches!(expr.kind, ExprKind::NumericRandom)),
        "expected random.random lowering",
    )?;
    Ok(())
}

#[test]
fn builtin_min_max_lower() -> TestResult {
    let source = py!(r#"
first: int = 1
second: int = 2
highest: int = max(first, second)
lowest: int = min(first, second)
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

    for expected in [NumericExtremaOp::Max, NumericExtremaOp::Min] {
        ensure(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::NumericExtrema { op, .. } if op == expected),
            ),
            "expected Python min/max lowering",
        )?;
    }
    Ok(())
}

#[test]
fn builtin_sum_lower() -> TestResult {
    let source = py!(r#"
ints: list[int] = [1, 2]
int_total: int = sum(ints)
floats: list[float] = [1.0, 2.0]
float_total: float = sum(floats)
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
    let sums = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::ListSum { .. }))
        .count();
    ensure_eq(&sums, &2, "list sum count")
}

#[test]
fn string_contains_comparison_lowers() -> TestResult {
    let source = py!(r#"
word: str = "Smelt"
has: bool = "mel" in word
missing: bool = "xyz" not in word
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body_id = module
        .body
        .ok_or_else(|| "expected module body".to_owned())?;
    let body = body(&ctx, body_id)?;

    let contains_count = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::StringContains { .. }))
        .count();
    ensure_eq(&contains_count, &2, "string contains count")?;
    Ok(())
}

#[test]
fn builtin_all_any_lower() -> TestResult {
    let source = py!(r#"
values: list[bool] = [True, False]
all_values: bool = all(values)
any_values: bool = any(values)
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

    for expected in [BoolFoldOp::All, BoolFoldOp::Any] {
        ensure(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::ListBoolFold { op, .. } if op == expected),
            ),
            "expected bool fold lowering",
        )?;
    }
    Ok(())
}

#[test]
fn builtin_sorted_lower() -> TestResult {
    let source = py!(r#"
values: list[int] = [2, 1]
ordered: list[int] = sorted(values)
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
            .any(|expr| matches!(expr.kind, ExprKind::ListSorted { .. })),
        "expected sorted lowering",
    )?;
    Ok(())
}

#[test]
fn list_contains_comparison_lowers() -> TestResult {
    let source = py!(r#"
values: list[int] = [1, 2, 3]
has: bool = 2 in values
missing: bool = 4 not in values
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body_id = module
        .body
        .ok_or_else(|| "expected module body".to_owned())?;
    let body = body(&ctx, body_id)?;

    let contains_count = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::ListContains { .. }))
        .count();
    ensure_eq(&contains_count, &2, "list contains count")?;
    Ok(())
}

#[test]
fn set_literal_and_contains_comparison_lowers() -> TestResult {
    let source = py!(r#"
values: set[int] = {1, 2, 3}
has: bool = 2 in values
missing: bool = 4 not in values
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
            .any(|expr| matches!(expr.kind, ExprKind::SetLit(_))),
        "expected set literal lowering",
    )?;
    ensure_eq(
        &body
            .exprs
            .iter()
            .filter(|expr| matches!(expr.kind, ExprKind::SetContains { .. }))
            .count(),
        &2,
        "set contains count",
    )?;
    Ok(())
}

#[test]
fn set_mutation_methods_lower() -> TestResult {
    let source = py!(r#"
values: set[int] = {1, 2}
values.add(3)
values.discard(2)
values.remove(1)
copy: set[int] = values.copy()
values.clear()
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
            .any(|expr| matches!(expr.kind, ExprKind::SetAdd { .. })),
        "expected set.add lowering",
    )?;
    for expected in [SetRemoveOp::Discard, SetRemoveOp::Remove] {
        ensure(
            body.exprs
                .iter()
                .any(|expr| matches!(expr.kind, ExprKind::SetRemove { op, .. } if op == expected)),
            "expected set remove/discard lowering",
        )?;
    }
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::SetCopy { .. })),
        "expected set.copy lowering",
    )?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::SetClear { .. })),
        "expected set.clear lowering",
    )
}

#[test]
fn set_algebra_methods_lower() -> TestResult {
    let source = py!(r#"
left: set[int] = {1, 2}
right: set[int] = {2, 3}
merged: set[int] = left.union(right)
common: set[int] = left.intersection(right)
only_left: set[int] = left.difference(right)
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
        SetBinaryOp::Union,
        SetBinaryOp::Intersection,
        SetBinaryOp::Difference,
    ] {
        ensure(
            body.exprs
                .iter()
                .any(|expr| matches!(expr.kind, ExprKind::SetBinary { op, .. } if op == expected)),
            "expected set algebra lowering",
        )?;
    }
    Ok(())
}

#[test]
fn tuple_contains_comparison_lowers() -> TestResult {
    let source = py!(r#"
values: tuple[int, int] = (1, 2)
has: bool = 2 in values
missing: bool = 4 not in values
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

    ensure_eq(
        &body
            .exprs
            .iter()
            .filter(|expr| matches!(expr.kind, ExprKind::TupleContains { .. }))
            .count(),
        &2,
        "tuple contains count",
    )?;
    Ok(())
}

#[test]
fn tuple_index_and_slice_lower() -> TestResult {
    let source = py!(r#"
pair: tuple[str, int] = ("Ada", 1)
name: str = pair[0]
rank: int = pair[-1]
tail: tuple[int] = pair[1:]
empty: tuple[()] = pair[:0]
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
            .filter(|expr| matches!(expr.kind, ExprKind::TupleIndex { .. }))
            .count()
            >= 2,
        "expected tuple index lowering",
    )?;
    ensure(
        body.exprs
            .iter()
            .filter(|expr| matches!(expr.kind, ExprKind::TupleSlice { .. }))
            .count()
            >= 2,
        "expected tuple slice lowering",
    )
}

#[test]
fn unsupported_dynamic_tuple_index_rejects() -> TestResult {
    let mut ctx = HirCtx::new();
    let errors = lower_errors(
        py!(r#"
pair: tuple[str, int] = ("Ada", 1)
i: int = 0
bad: str = pair[i]
"#),
        &mut ctx,
    )?;
    let error = first_error(&errors)?;
    ensure(
        error.message.contains("static integer index"),
        "expected dynamic tuple index diagnostic",
    )
}

#[test]
fn dict_key_contains_comparison_lowers() -> TestResult {
    let source = py!(r#"
values: dict[str, int] = {"a": 1}
has: bool = "a" in values
missing: bool = "b" not in values
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

    ensure_eq(
        &body
            .exprs
            .iter()
            .filter(|expr| matches!(expr.kind, ExprKind::DictContainsKey { .. }))
            .count(),
        &2,
        "dict key contains count",
    )?;
    Ok(())
}

#[test]
fn string_split_method_lowers() -> TestResult {
    let source = py!(r#"
word: str = "a,b,c"
parts: list[str] = word.split(",")
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
            .any(|expr| matches!(expr.kind, ExprKind::StringSplit { .. })),
        "expected string split lowering",
    )?;
    Ok(())
}

#[test]
fn tuple_destructuring_assignment_lowers_to_pattern() -> TestResult {
    let source = py!(r#"
left, right = (1, "two")
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body_id = module
        .body
        .ok_or_else(|| "expected module body".to_owned())?;
    let body = body(&ctx, body_id)?;

    let stmt = body
        .stmts
        .first()
        .ok_or_else(|| "expected destructuring let statement".to_owned())?;
    let Stmt::Let { pat, ty, value } = stmt else {
        return Err("expected destructuring let".to_owned());
    };
    ensure(value.is_some(), "expected initializer value")?;
    ensure(
        matches!(pattern(body, *pat)?, Pattern::Tuple(_)),
        "expected tuple pattern",
    )?;
    ensure(
        matches!(ctx.krate.types.get(*ty), Some(Type::Tuple(items)) if items.len() == 2),
        "expected tuple type of length 2",
    )?;
    Ok(())
}

#[test]
fn for_tuple_destructuring_target_lowers_to_pattern() -> TestResult {
    let source = py!(r#"
pairs: list[tuple[int, str]] = [(1, "one")]
for key, label in pairs:
    print(label)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body_id = module
        .body
        .ok_or_else(|| "expected module body".to_owned())?;
    let body = body(&ctx, body_id)?;

    let for_pattern_id = body.stmts.iter().find_map(|stmt| {
        if let Stmt::For { pat, .. } = stmt {
            Some(*pat)
        } else {
            None
        }
    });
    let pattern_id = for_pattern_id.ok_or_else(|| "expected for statement".to_owned())?;
    ensure(
        matches!(pattern(body, pattern_id)?, Pattern::Tuple(_)),
        "expected tuple pattern",
    )?;
    Ok(())
}

#[test]
fn plain_class_lowers() -> TestResult {
    let source = py!(r#"
class Point:
    x: int
    y: int
    def __init__(self, x: int, y: int) -> None:
        self.x = x
        self.y = y
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let class_item_id = module
        .items
        .last()
        .copied()
        .ok_or_else(|| "expected class item".to_owned())?;
    if let Item::Class(c) = item(&ctx, class_item_id)? {
        ensure_eq(&symbol(&ctx, c.name)?, &"Point", "class name")?;
        ensure_eq(&c.fields.len(), &2, "field count")?;
        ensure(
            matches!(c.kind, smelt_hir::ClassKind::Plain),
            "expected plain class",
        )?;
        ensure(c.constructor.is_some(), "expected constructor")?;
    } else {
        return Err("expected Class item".to_owned());
    }
    Ok(())
}

#[test]
fn dataclass_lowers() -> TestResult {
    let source = py!(r#"
from dataclasses import dataclass

@dataclass
class Point:
    x: int
    y: int
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let class_item_id = module
        .items
        .last()
        .copied()
        .ok_or_else(|| "expected class item".to_owned())?;
    if let Item::Class(c) = item(&ctx, class_item_id)? {
        ensure(
            matches!(
                c.kind,
                smelt_hir::ClassKind::DataclassLike { frozen: false }
            ),
            "expected dataclass-like class",
        )?;
        ensure(c.constructor.is_some(), "should have synthesized __init__")?;
    } else {
        return Err("expected Class item".to_owned());
    }
    Ok(())
}

#[test]
fn frozen_dataclass_lowers() -> TestResult {
    let source = py!(r#"
@dataclass(frozen=True)
class Immutable:
    value: int
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let class_item_id = module
        .items
        .last()
        .copied()
        .ok_or_else(|| "expected class item".to_owned())?;
    if let Item::Class(c) = item(&ctx, class_item_id)? {
        ensure(
            matches!(c.kind, smelt_hir::ClassKind::DataclassLike { frozen: true }),
            "expected frozen dataclass-like class",
        )?;
    } else {
        return Err("expected Class item".to_owned());
    }
    Ok(())
}

#[test]
fn class_constructor_call_lowers() -> TestResult {
    let source = py!(r#"
class Dog:
    name: str
    def __init__(self, name: str) -> None:
        self.name = name

d: Dog = Dog("Rex")
"#);
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    Ok(())
}

#[test]
fn django_model_rejected() -> TestResult {
    let source = py!(r#"
class MyModel(models.Model):
    name: str
"#);
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    let error = first_error(&errors)?;
    ensure_eq(&error.code, &"smelt::django-unsupported", "error code")?;
    Ok(())
}

#[test]
fn metaclass_rejected() -> TestResult {
    let source = py!(r#"
class Meta(metaclass=ABCMeta):
    pass
"#);
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    let error = first_error(&errors)?;
    ensure_eq(&error.code, &"smelt::no-metaclass", "error code")?;
    Ok(())
}

#[test]
fn multiple_inheritance_rejected() -> TestResult {
    let source = py!(r#"
class C(A, B):
    pass
"#);
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    let error = first_error(&errors)?;
    ensure_eq(&error.code, &"smelt::no-multiple-inheritance", "error code")?;
    Ok(())
}

#[test]
fn unknown_decorator_rejected() -> TestResult {
    let source = py!(r#"
@some_decorator
class Foo:
    x: int
"#);
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    let error = first_error(&errors)?;
    ensure_eq(&error.code, &"smelt::unsupported-py", "error code")?;
    Ok(())
}
