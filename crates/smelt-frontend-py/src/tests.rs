//! Unit tests for the Python frontend.

use crate::{HirCtx, to_hir};
use smelt_hir::{FileId, Language};

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
        smelt_hir::Item::Function(f) => {
            assert_eq!(ctx.krate.symbols.get(f.name).unwrap(), "add");
            assert_eq!(f.params.len(), 2);
            assert!(f.body.is_some());
        }
        _ => panic!("expected Function item"),
    }
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
        smelt_hir::Item::Class(c) => {
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
        smelt_hir::Item::Class(c) => {
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
        smelt_hir::Item::Class(c) => {
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
