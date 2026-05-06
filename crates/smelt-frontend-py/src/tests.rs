//! Unit tests for the Python frontend.

use crate::{HirCtx, to_hir};
use smelt_hir::{AsyncOp, ExprKind, FileId, Item, Language, Pattern, Stmt, StringCaseOp, Type};

#[test]
fn empty_module_lowers_to_empty_hir() {
    let mut ctx = HirCtx::new();
    let module_id = to_hir("", FileId(0), &mut ctx).expect("empty module is valid");
    let module = &ctx.krate.modules[module_id.0 as usize];
    assert_eq!(module.source.language, Language::Python);
    assert!(module.items.is_empty());
}

#[test]
fn parse_error_is_reported() {
    let mut ctx = HirCtx::new();
    let errors = to_hir("x = \"oops", FileId(0), &mut ctx).expect_err("should fail");
    assert!(!errors.is_empty());
    assert_eq!(errors[0].code, "smelt::parse-error-py");
}

#[test]
fn simple_function_lowers() {
    let source = r#"
def add(x: int, y: int) -> int:
    return x + y
"#;
    let mut ctx = HirCtx::new();
    let module_id = to_hir(source, FileId(0), &mut ctx).expect("valid module");
    let module = &ctx.krate.modules[module_id.0 as usize];
    assert_eq!(module.items.len(), 1);
    let item = &ctx.krate.items[module.items[0].0 as usize];
    match item {
        Item::Function(f) => {
            assert_eq!(ctx.krate.symbols.get(f.name).unwrap(), "add");
            assert_eq!(f.params.len(), 2);
            assert!(f.body.is_some());
        }
        _ => panic!("expected Function item"),
    }
}

#[test]
fn async_function_and_await_lower() {
    let source = r#"
async def lift(value: int) -> int:
    return value

async def run() -> int:
    return await lift(5)
"#;
    let mut ctx = HirCtx::new();
    let module_id = to_hir(source, FileId(0), &mut ctx).expect("async module should lower");
    let module = &ctx.krate.modules[module_id.0 as usize];

    assert_eq!(module.items.len(), 2);
    let Item::Function(lift) = &ctx.krate.items[module.items[0].0 as usize] else {
        panic!("expected function item");
    };
    assert!(lift.is_async);
    assert!(matches!(
        ctx.krate.types.get(lift.return_ty),
        Some(Type::Future(_))
    ));

    let Item::Function(run) = &ctx.krate.items[module.items[1].0 as usize] else {
        panic!("expected function item");
    };
    assert!(run.is_async);
    let body = &ctx.krate.bodies[run.body.expect("run body").0 as usize];
    assert!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Await(_)))
    );
    let state_machine = body
        .async_state_machine
        .as_ref()
        .expect("async body should record suspension metadata");
    assert_eq!(state_machine.suspensions.len(), 1);
}

#[test]
fn await_outside_async_function_is_rejected() {
    let source = r#"
async def lift(value: int) -> int:
    return value

def run() -> int:
    return await lift(5)
"#;
    let mut ctx = HirCtx::new();
    let errors = to_hir(source, FileId(0), &mut ctx).expect_err("await requires async def");

    assert_eq!(errors[0].code, "smelt::unsupported-py");
    assert!(errors[0].message.contains("inside async functions"));
}

#[test]
fn asyncio_gather_and_sleep_lower_to_async_ops() {
    let source = r#"
import asyncio

async def lift(value: int) -> int:
    await asyncio.sleep(0)
    return value

async def run() -> tuple[int, int]:
    return await asyncio.gather(lift(1), lift(2))
"#;
    let mut ctx = HirCtx::new();
    let module_id = to_hir(source, FileId(0), &mut ctx).expect("asyncio calls should lower");
    let module = &ctx.krate.modules[module_id.0 as usize];

    let Item::Function(lift) = &ctx.krate.items[module.items[0].0 as usize] else {
        panic!("expected function item");
    };
    let lift_body = &ctx.krate.bodies[lift.body.expect("lift body").0 as usize];
    assert!(lift_body.exprs.iter().any(|expr| {
        matches!(
            expr.kind,
            ExprKind::AsyncOp {
                op: AsyncOp::Sleep,
                ..
            }
        )
    }));

    let Item::Function(run) = &ctx.krate.items[module.items[1].0 as usize] else {
        panic!("expected function item");
    };
    let run_body = &ctx.krate.bodies[run.body.expect("run body").0 as usize];
    assert!(run_body.exprs.iter().any(|expr| {
        matches!(
            expr.kind,
            ExprKind::AsyncOp {
                op: AsyncOp::All,
                ..
            }
        )
    }));
}

#[test]
fn lower_level_asyncio_loop_apis_are_rejected() {
    let source = r#"
import asyncio

def run() -> None:
    asyncio.get_event_loop()
"#;
    let mut ctx = HirCtx::new();
    let errors = to_hir(source, FileId(0), &mut ctx).expect_err("loop API should be rejected");

    assert_eq!(errors[0].code, "smelt::unsupported-py");
    assert!(errors[0].message.contains("lower-level event-loop API"));
}

#[test]
fn asyncio_task_wait_for_and_runtime_objects_are_classified() {
    let source = r#"
import asyncio

async def lift(value: int) -> int:
    return value

async def run() -> int:
    task: Awaitable[int] = asyncio.create_task(lift(1))
    return await asyncio.wait_for(task, 10)
"#;
    let mut ctx = HirCtx::new();
    to_hir(source, FileId(0), &mut ctx).expect("task and wait_for should lower");

    let queue_source = r#"
import asyncio

def run() -> None:
    asyncio.Queue()
"#;
    let mut ctx = HirCtx::new();
    let errors = to_hir(queue_source, FileId(0), &mut ctx).expect_err("Queue should be classified");
    assert!(errors[0].message.contains("runtime object support"));
}

#[test]
fn annotated_assignment_lowers() {
    let source = "x: int = 42\n";
    let mut ctx = HirCtx::new();
    let module_id = to_hir(source, FileId(0), &mut ctx).expect("valid module");
    let module = &ctx.krate.modules[module_id.0 as usize];
    let body = &ctx.krate.bodies[module.body.unwrap().0 as usize];
    assert!(!body.stmts.is_empty());
}

#[test]
fn type_annotations_lowered() {
    let source = r#"
def process(items: list[str], counts: dict[str, int]) -> bool:
    return True
"#;
    let mut ctx = HirCtx::new();
    to_hir(source, FileId(0), &mut ctx).expect("type annotations should lower");
}

#[test]
fn optional_annotation_lowered() {
    let source = r#"
def find(x: int) -> str | None:
    return None
"#;
    let mut ctx = HirCtx::new();
    to_hir(source, FileId(0), &mut ctx).expect("PEP 604 Optional annotation should lower");
}

#[test]
fn missing_return_annotation_is_error() {
    let source = "def bad(x: int):\n    return x\n";
    let mut ctx = HirCtx::new();
    let errors = to_hir(source, FileId(0), &mut ctx).expect_err("should require return type");
    assert_eq!(errors[0].code, "smelt::unsupported-py");
}

#[test]
fn print_call_lowers() {
    let source = r#"
x: int = 1
print(x)
"#;
    let mut ctx = HirCtx::new();
    to_hir(source, FileId(0), &mut ctx).expect("print() should lower");
}

#[test]
fn len_call_lowers() {
    let source = r#"
values: list[int] = [1, 2, 3]
count: int = len(values)
word: str = "smelt"
letters: int = len(word)
"#;
    let mut ctx = HirCtx::new();
    let module_id = to_hir(source, FileId(0), &mut ctx).expect("len() should lower");
    let module = &ctx.krate.modules[module_id.0 as usize];
    let body = &ctx.krate.bodies[module.body.expect("module body").0 as usize];

    let len_count = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::Len { .. }))
        .count();
    assert_eq!(len_count, 2);
}

#[test]
fn string_case_methods_lower() {
    let source = r#"
word: str = "Smelt"
lower: str = word.lower()
upper: str = word.upper()
"#;
    let mut ctx = HirCtx::new();
    let module_id = to_hir(source, FileId(0), &mut ctx).expect("string case methods should lower");
    let module = &ctx.krate.modules[module_id.0 as usize];
    let body = &ctx.krate.bodies[module.body.expect("module body").0 as usize];

    assert!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::StringCase {
            op: StringCaseOp::Lower,
            ..
        }
    )));
    assert!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::StringCase {
            op: StringCaseOp::Upper,
            ..
        }
    )));
}

#[test]
fn string_contains_comparison_lowers() {
    let source = r#"
word: str = "Smelt"
has: bool = "mel" in word
missing: bool = "xyz" not in word
"#;
    let mut ctx = HirCtx::new();
    let module_id =
        to_hir(source, FileId(0), &mut ctx).expect("string contains comparisons should lower");
    let module = &ctx.krate.modules[module_id.0 as usize];
    let body = &ctx.krate.bodies[module.body.expect("module body").0 as usize];

    let contains_count = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::StringContains { .. }))
        .count();
    assert_eq!(contains_count, 2);
}

#[test]
fn tuple_destructuring_assignment_lowers_to_pattern() {
    let source = r#"
left, right = (1, "two")
"#;
    let mut ctx = HirCtx::new();
    let module_id = to_hir(source, FileId(0), &mut ctx).expect("destructuring should lower");
    let module = &ctx.krate.modules[module_id.0 as usize];
    let body = &ctx.krate.bodies[module.body.expect("module body").0 as usize];

    let Stmt::Let { pat, ty, value } = body.stmts.first().expect("let stmt") else {
        panic!("expected destructuring let");
    };
    assert!(value.is_some());
    assert!(matches!(body.patterns[pat.0 as usize], Pattern::Tuple(_)));
    assert!(matches!(ctx.krate.types.get(*ty), Some(Type::Tuple(items)) if items.len() == 2));
}

#[test]
fn for_tuple_destructuring_target_lowers_to_pattern() {
    let source = r#"
pairs: list[tuple[int, str]] = [(1, "one")]
for key, label in pairs:
    print(label)
"#;
    let mut ctx = HirCtx::new();
    let module_id = to_hir(source, FileId(0), &mut ctx).expect("for destructuring should lower");
    let module = &ctx.krate.modules[module_id.0 as usize];
    let body = &ctx.krate.bodies[module.body.expect("module body").0 as usize];

    let for_pat = body.stmts.iter().find_map(|stmt| match stmt {
        Stmt::For { pat, .. } => Some(*pat),
        _ => None,
    });
    let pat = for_pat.expect("for statement");
    assert!(matches!(body.patterns[pat.0 as usize], Pattern::Tuple(_)));
}

#[test]
fn plain_class_lowers() {
    let source = r#"
class Point:
    x: int
    y: int
    def __init__(self, x: int, y: int) -> None:
        self.x = x
        self.y = y
"#;
    let mut ctx = HirCtx::new();
    let module_id = to_hir(source, FileId(0), &mut ctx).expect("plain class should lower");
    let module = &ctx.krate.modules[module_id.0 as usize];
    let class_item_id = module.items.last().copied().expect("class item");
    match &ctx.krate.items[class_item_id.0 as usize] {
        Item::Class(c) => {
            assert_eq!(ctx.krate.symbols.get(c.name).unwrap(), "Point");
            assert_eq!(c.fields.len(), 2);
            assert!(matches!(c.kind, smelt_hir::ClassKind::Plain));
            assert!(c.constructor.is_some());
        }
        _ => panic!("expected Class item"),
    }
}

#[test]
fn dataclass_lowers() {
    let source = r#"
from dataclasses import dataclass

@dataclass
class Point:
    x: int
    y: int
"#;
    let mut ctx = HirCtx::new();
    let module_id = to_hir(source, FileId(0), &mut ctx).expect("dataclass should lower");
    let module = &ctx.krate.modules[module_id.0 as usize];
    let class_item_id = module.items.last().copied().expect("class item");
    match &ctx.krate.items[class_item_id.0 as usize] {
        Item::Class(c) => {
            assert!(matches!(
                c.kind,
                smelt_hir::ClassKind::DataclassLike { frozen: false }
            ));
            assert!(c.constructor.is_some(), "should have synthesized __init__");
        }
        _ => panic!("expected Class item"),
    }
}

#[test]
fn frozen_dataclass_lowers() {
    let source = r#"
@dataclass(frozen=True)
class Immutable:
    value: int
"#;
    let mut ctx = HirCtx::new();
    let module_id = to_hir(source, FileId(0), &mut ctx).expect("frozen dataclass should lower");
    let module = &ctx.krate.modules[module_id.0 as usize];
    let class_item_id = module.items.last().copied().expect("class item");
    match &ctx.krate.items[class_item_id.0 as usize] {
        Item::Class(c) => {
            assert!(matches!(
                c.kind,
                smelt_hir::ClassKind::DataclassLike { frozen: true }
            ));
        }
        _ => panic!("expected Class item"),
    }
}

#[test]
fn class_constructor_call_lowers() {
    let source = r#"
class Dog:
    name: str
    def __init__(self, name: str) -> None:
        self.name = name

d: Dog = Dog("Rex")
"#;
    let mut ctx = HirCtx::new();
    to_hir(source, FileId(0), &mut ctx).expect("constructor call should lower");
}

#[test]
fn django_model_rejected() {
    let source = r#"
class MyModel(models.Model):
    name: str
"#;
    let mut ctx = HirCtx::new();
    let errors = to_hir(source, FileId(0), &mut ctx).expect_err("django model should be rejected");
    assert!(errors[0].code == "smelt::django-unsupported");
}

#[test]
fn metaclass_rejected() {
    let source = r#"
class Meta(metaclass=ABCMeta):
    pass
"#;
    let mut ctx = HirCtx::new();
    let errors = to_hir(source, FileId(0), &mut ctx).expect_err("metaclass should be rejected");
    assert_eq!(errors[0].code, "smelt::no-metaclass");
}

#[test]
fn multiple_inheritance_rejected() {
    let source = r#"
class C(A, B):
    pass
"#;
    let mut ctx = HirCtx::new();
    let errors =
        to_hir(source, FileId(0), &mut ctx).expect_err("multiple inheritance should be rejected");
    assert_eq!(errors[0].code, "smelt::no-multiple-inheritance");
}

#[test]
fn unknown_decorator_rejected() {
    let source = r#"
@some_decorator
class Foo:
    x: int
"#;
    let mut ctx = HirCtx::new();
    let errors =
        to_hir(source, FileId(0), &mut ctx).expect_err("unknown decorator should be rejected");
    assert_eq!(errors[0].code, "smelt::unsupported-py");
}
