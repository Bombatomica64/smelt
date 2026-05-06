//! Unit tests for the Python frontend.

use crate::{HirCtx, SmeltError, to_hir};
use smelt_hir::{
    AsyncOp, Body, BodyId, ExprKind, FileId, Item, ItemId, Language, Module, ModuleId, Pattern,
    PatternId, Stmt, StringCaseOp, Symbol, Type,
};
use std::convert::TryFrom;

type TestResult = Result<(), String>;

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
    let module_id = lower_module("", &mut ctx)?;
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
    let errors = lower_errors("x = \"oops", &mut ctx)?;
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
    let source = r#"
def add(x: int, y: int) -> int:
    return x + y
"#;
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
    let source = r#"
async def lift(value: int) -> int:
    return value

async def run() -> int:
    return await lift(5)
"#;
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
    let source = r#"
async def lift(value: int) -> int:
    return value

def run() -> int:
    return await lift(5)
"#;
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
    let source = r#"
import asyncio

async def lift(value: int) -> int:
    await asyncio.sleep(0)
    return value

async def run() -> tuple[int, int]:
    return await asyncio.gather(lift(1), lift(2))
"#;
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
    let source = r#"
import asyncio

def run() -> None:
    asyncio.get_event_loop()
"#;
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
    let source = r#"
import asyncio

async def lift(value: int) -> int:
    return value

async def run() -> int:
    task: Awaitable[int] = asyncio.create_task(lift(1))
    return await asyncio.wait_for(task, 10)
"#;
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;

    let queue_source = r#"
import asyncio

def run() -> None:
    asyncio.Queue()
"#;
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
    let source = "x: int = 42\n";
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
    let source = r#"
def process(items: list[str], counts: dict[str, int]) -> bool:
    return True
"#;
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    Ok(())
}

#[test]
fn optional_annotation_lowered() -> TestResult {
    let source = r#"
def find(x: int) -> str | None:
    return None
"#;
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    Ok(())
}

#[test]
fn missing_return_annotation_is_error() -> TestResult {
    let source = "def bad(x: int):\n    return x\n";
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
    let source = r#"
x: int = 1
print(x)
"#;
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    Ok(())
}

#[test]
fn len_call_lowers() -> TestResult {
    let source = r#"
values: list[int] = [1, 2, 3]
count: int = len(values)
word: str = "smelt"
letters: int = len(word)
"#;
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
fn string_case_methods_lower() -> TestResult {
    let source = r#"
word: str = "Smelt"
lower: str = word.lower()
upper: str = word.upper()
"#;
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
fn string_contains_comparison_lowers() -> TestResult {
    let source = r#"
word: str = "Smelt"
has: bool = "mel" in word
missing: bool = "xyz" not in word
"#;
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
fn string_split_method_lowers() -> TestResult {
    let source = r#"
word: str = "a,b,c"
parts: list[str] = word.split(",")
"#;
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
    let source = r#"
left, right = (1, "two")
"#;
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
    let source = r#"
pairs: list[tuple[int, str]] = [(1, "one")]
for key, label in pairs:
    print(label)
"#;
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
    let source = r#"
class Point:
    x: int
    y: int
    def __init__(self, x: int, y: int) -> None:
        self.x = x
        self.y = y
"#;
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
    let source = r#"
from dataclasses import dataclass

@dataclass
class Point:
    x: int
    y: int
"#;
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
    let source = r#"
@dataclass(frozen=True)
class Immutable:
    value: int
"#;
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
    let source = r#"
class Dog:
    name: str
    def __init__(self, name: str) -> None:
        self.name = name

d: Dog = Dog("Rex")
"#;
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    Ok(())
}

#[test]
fn django_model_rejected() -> TestResult {
    let source = r#"
class MyModel(models.Model):
    name: str
"#;
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    let error = first_error(&errors)?;
    ensure_eq(&error.code, &"smelt::django-unsupported", "error code")?;
    Ok(())
}

#[test]
fn metaclass_rejected() -> TestResult {
    let source = r#"
class Meta(metaclass=ABCMeta):
    pass
"#;
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    let error = first_error(&errors)?;
    ensure_eq(&error.code, &"smelt::no-metaclass", "error code")?;
    Ok(())
}

#[test]
fn multiple_inheritance_rejected() -> TestResult {
    let source = r#"
class C(A, B):
    pass
"#;
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    let error = first_error(&errors)?;
    ensure_eq(&error.code, &"smelt::no-multiple-inheritance", "error code")?;
    Ok(())
}

#[test]
fn unknown_decorator_rejected() -> TestResult {
    let source = r#"
@some_decorator
class Foo:
    x: int
"#;
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    let error = first_error(&errors)?;
    ensure_eq(&error.code, &"smelt::unsupported-py", "error code")?;
    Ok(())
}
