//! User exception classes as ordinary classes
//! (`docs/design/python-exceptions.md`): the typed `except` binding, the
//! members an exception root gets from `BaseException`, the hidden packed
//! `args` parameter, and the refusals.

use super::*;

/// The class item named `name` in the lowered crate.
fn class_named<'a>(ctx: &'a HirCtx, name: &str) -> Result<&'a smelt_hir::Class, String> {
    ctx.krate
        .items
        .iter()
        .find_map(|item| match item {
            Item::Class(class) if ctx.krate.symbols.get(class.name) == Some(name) => Some(class),
            _ => None,
        })
        .ok_or_else(|| format!("no class named `{name}`"))
}

#[test]
fn except_naming_user_classes_binds_the_typed_instance() -> TestResult {
    let source = py!(r#"
class AppError(Exception):
    pass

class HttpError(AppError):
    def __init__(self, status: int) -> None:
        super().__init__("http")
        self.status = status

class NotFound(HttpError):
    pass

def run() -> int:
    try:
        raise NotFound(404)
    except (NotFound, HttpError) as e:
        return e.status
"#);
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    // The tuple's nearest common class is `HttpError`: the binding is the
    // instance recovered as that class, so `e.status` is an ordinary field.
    let bound = ctx
        .krate
        .bodies
        .iter()
        .flat_map(|body| body.exprs.iter())
        .find_map(|expr| match expr.kind {
            ExprKind::PyExceptionInstance { class, .. } => Some(class),
            _ => None,
        })
        .ok_or("expected a typed `except` binding")?;
    ensure(
        symbol(&ctx, bound)? == "HttpError",
        "expected the binding at the common class `HttpError`",
    )?;
    let raised = ctx
        .krate
        .bodies
        .iter()
        .flat_map(|body| body.exprs.iter())
        .any(|expr| matches!(expr.kind, ExprKind::PyExceptionFromInstance { .. }));
    ensure(raised, "expected the raise to carry the instance")?;
    Ok(())
}

#[test]
fn exception_root_gets_base_exception_members() -> TestResult {
    let source = py!(r#"
class AppError(Exception):
    def __str__(self) -> str:
        return "custom"

class Child(AppError):
    pass
"#);
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    let root = class_named(&ctx, "AppError")?;
    ensure(root.base.is_none(), "an exception root has no program base")?;
    let fields: Vec<&str> = root
        .fields
        .iter()
        .filter_map(|field| ctx.krate.symbols.get(field.name))
        .collect();
    ensure(
        fields == ["args"],
        format!("expected the root's only field to be `args`, got {fields:?}"),
    )?;
    // The root defines `__str__`, so only the default `__repr__` is synthesized.
    let method_names: Vec<&str> = root
        .methods
        .iter()
        .filter_map(|id| match item(&ctx, *id) {
            Ok(Item::Function(function)) => ctx.krate.symbols.get(function.name),
            _ => None,
        })
        .collect();
    ensure(
        method_names
            .iter()
            .filter(|name| **name == "__str__")
            .count()
            == 1
            && method_names.contains(&"__repr__"),
        format!("expected the user's `__str__` and a synthesized `__repr__`, got {method_names:?}"),
    )?;
    for name in ["AppError", "Child"] {
        let class = class_named(&ctx, name)?;
        let builtin = class.protocols.iter().find_map(|protocol| match protocol {
            ClassProtocol::PyException { builtin_base, .. } => Some(*builtin_base),
            _ => None,
        });
        ensure(
            builtin.and_then(|symbol| ctx.krate.symbols.get(symbol)) == Some("Exception"),
            format!("expected `{name}` to carry the exception protocol over `Exception`"),
        )?;
    }
    Ok(())
}

#[test]
fn constructor_keeps_args_only_when_base_exception_init_never_overwrites_them() -> TestResult {
    let source = py!(r#"
class Keeps(Exception):
    def __init__(self, code: int) -> None:
        self.code = code

class Overwrites(Exception):
    def __init__(self, code: int) -> None:
        super().__init__("message")
        self.code = code
"#);
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    let param_count = |name: &str| -> Result<usize, String> {
        let class = class_named(&ctx, name)?;
        let constructor = class.constructor.ok_or("expected a constructor")?;
        match item(&ctx, constructor)? {
            Item::Function(function) => Ok(function.params.len()),
            _ => Err("constructor is not a function".to_owned()),
        }
    };
    // `Keeps` takes its positional arguments packed first (`args` shows
    // them); `Overwrites` replaces `args` before anything can read it.
    ensure(
        param_count("Keeps")? == 2,
        "expected `Keeps` to take the hidden packed `args` first",
    )?;
    ensure(
        param_count("Overwrites")? == 1,
        "expected `Overwrites` to take only `code`",
    )?;
    Ok(())
}

#[test]
fn kept_args_need_python_text() -> TestResult {
    let source = py!(r#"
class Token:
    def __init__(self, text: str) -> None:
        self.text = text

class Keeps(Exception):
    def __init__(self, token: Token) -> None:
        self.token = token

def run() -> None:
    raise Keeps(Token("x"))
"#);
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    ensure(
        first_error(&errors)?
            .message
            .contains("keeps this argument in `args`"),
        "expected a constructor argument `args` must show to need Python text",
    )
}
