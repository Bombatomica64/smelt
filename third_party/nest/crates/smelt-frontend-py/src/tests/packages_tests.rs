use super::*;

#[test]
fn httpx_like_status_code_classmethod_lowers_through_package_namespace() -> TestResult {
    let mut ctx = HirCtx::new();
    lower_path_module(
        py!(r#"
from enum import IntEnum

class codes(IntEnum):
    OK = 200, "OK"
    NOT_FOUND = 404, "Not Found"

    @classmethod
    def get_reason_phrase(cls, value: int) -> str:
        return "OK" if value == codes.OK else "Not Found"
"#),
        "src/httpx/_status_codes.py",
        &mut ctx,
    )?;
    lower_path_module(
        py!(r#"
from ._status_codes import codes
"#),
        "src/httpx/__init__.py",
        &mut ctx,
    )?;
    let module_id = lower_path_module(
        py!(r#"
import httpx

def test_status_code_reason():
    assert httpx.codes.OK == 200
    assert httpx.codes.get_reason_phrase(200) == "OK"
"#),
        "tests/test_status_codes.py",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let test = module
        .items
        .iter()
        .find_map(|item_id| match item(&ctx, *item_id).ok()? {
            Item::Function(function) if function.is_test => Some(function),
            _ => None,
        })
        .ok_or("expected lowered pytest test")?;
    let test_body = body(&ctx, test.body.ok_or("expected test body")?)?;
    ensure(
        test_body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::Int(200)))),
        "expected httpx.codes.OK to lower as integer literal",
    )?;
    ensure(
        test_body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Call { .. })),
        "expected httpx.codes.get_reason_phrase(...) call",
    )?;
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
fn abcmeta_metaclass_lowers_as_abstract_metadata() -> TestResult {
    let source = py!(r#"
class Meta(metaclass=ABCMeta):
    pass
"#);
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    Ok(())
}

#[test]
fn package_import_namespace_members_lower() -> TestResult {
    let mut ctx = HirCtx::new();
    lower_path_module(
        py!(r#"
def add(a: int, b: int) -> int:
    return a + b
"#),
        "src/httpx/__init__.py",
        &mut ctx,
    )?;
    ensure(
        ctx.module_namespaces
            .get("httpx")
            .is_some_and(|members| members.contains_key("add")),
        "expected httpx package namespace to export add",
    )?;
    let module_id = lower_path_module(
        py!(r#"
import httpx

result: int = httpx.add(2, 3)
"#),
        "tests/test_status_codes.py",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let module_body = body(&ctx, module.body.ok_or("expected module body")?)?;
    ensure(
        module_body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Call { .. })),
        "expected package namespace member call",
    )?;
    Ok(())
}

#[test]
fn constructed_module_constant_exports_class_instance() -> TestResult {
    let source = py!(r#"
class NullFile:
    def __enter__(self) -> NullFile:
        return self

NULL_FILE = NullFile()
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    ensure(
        module.items.iter().any(|item_id| {
            matches!(
                item(&ctx, *item_id),
                Ok(Item::Const(const_item)) if symbol(&ctx, const_item.name) == Ok("NULL_FILE")
            )
        }),
        "expected constructed NULL_FILE constant item",
    )?;
    ensure(
        ctx.module_namespaces
            .values()
            .any(|members| members.contains_key("NULL_FILE")),
        "expected constructed constant in module namespace",
    )?;
    Ok(())
}

#[test]
fn rich_package_import_aliases_export_null_file_members() -> TestResult {
    let mut ctx = HirCtx::new();
    lower_path_module(
        py!(r#"
class NullFile:
    def __enter__(self) -> NullFile:
        return self
    def __exit__(self, *_args: object) -> None:
        pass

NULL_FILE = NullFile()
"#),
        "src/rich/_null_file.py",
        &mut ctx,
    )?;
    lower_path_module(
        py!(r#"
from ._null_file import NULL_FILE, NullFile

__all__ = ["NULL_FILE", "NullFile"]
"#),
        "src/rich/__init__.py",
        &mut ctx,
    )?;
    lower_path_module(
        py!(r#"
from rich import NULL_FILE, NullFile

value: NullFile = NULL_FILE
made: NullFile = NullFile()
"#),
        "tests/test_rich_imports.py",
        &mut ctx,
    )?;
    let rich = ctx
        .module_namespaces
        .get("rich")
        .ok_or("expected rich package namespace")?;
    ensure(
        rich.contains_key("NULL_FILE") && rich.contains_key("NullFile"),
        "expected rich package to re-export NULL_FILE and NullFile",
    )?;
    Ok(())
}

#[test]
fn rich_null_file_protocol_methods_lower() -> TestResult {
    let source = py!(r#"
class NullFile:
    def write(self, text: str) -> int:
        return 0
    def __enter__(self) -> NullFile:
        return self
    def __exit__(self, *_args: object) -> None:
        pass
    def __iter__(self) -> NullFile:
        return self
    def __next__(self) -> str:
        raise StopIteration
    def __str__(self) -> str:
        return ""

NULL_FILE = NullFile()
text: str = ""
with NULL_FILE as handle:
    text = str(handle)
    count: int = handle.write("ignored")
for line in NULL_FILE:
    text = line
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let module_body = body(&ctx, module.body.ok_or("expected module body")?)?;
    ensure(
        module_body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Method { .. })),
        "expected protocol/class method calls",
    )?;
    ensure(
        !module_body
            .stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::For { .. })),
        "expected NullFile direct iteration to lower as an empty loop",
    )?;
    Ok(())
}

#[test]
fn generic_base_class_lowers_as_base_metadata() -> TestResult {
    let source = py!(r#"
class NullFile(IO[str]):
    def write(self, text: str) -> int:
        return 0
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let class = module
        .items
        .iter()
        .find_map(|item_id| match item(&ctx, *item_id).ok()? {
            Item::Class(class) => Some(class),
            _ => None,
        })
        .ok_or("expected class item")?;
    let base = class.base.ok_or("expected base class metadata")?;
    ensure_eq(&symbol(&ctx, base)?, &"IO", "base class")?;
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

#[test]
fn deferred_native_and_io_apis_have_targeted_diagnostics() -> TestResult {
    let mut ctx = HirCtx::new();
    let numpy_errors = lower_errors(
        py!(r#"
import numpy as np

def run() -> int:
    return np.array([1, 2, 3])
"#),
        &mut ctx,
    )?;
    let error = first_error(&numpy_errors)?;
    ensure_eq(&error.code, &"smelt::unsupported-py", "error code")?;
    ensure(
        error.message.contains("NumPy is deferred from Phase 6"),
        "expected NumPy decision diagnostic",
    )?;

    let mut ctx = HirCtx::new();
    let pandas_errors = lower_errors(
        py!(r#"
import pandas as pd

def run() -> int:
    return pd.DataFrame()
"#),
        &mut ctx,
    )?;
    let error = first_error(&pandas_errors)?;
    ensure(
        error.message.contains("pandas is out of scope for Phase 6"),
        "expected pandas decision diagnostic",
    )?;

    let mut ctx = HirCtx::new();
    let module_id = lower_module(
        py!(r#"
def run() -> str:
    return open("input.txt").read()
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function_id = *module.items.first().ok_or("expected function item")?;
    let Item::Function(function) = item(&ctx, function_id)? else {
        return Err("expected function item".to_owned());
    };
    let function_body = body(&ctx, function.body.ok_or("expected function body")?)?;
    ensure(
        function_body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::FileReadText { .. })),
        "expected Python open().read() to lower to file_read_text",
    )?;
    Ok(())
}
