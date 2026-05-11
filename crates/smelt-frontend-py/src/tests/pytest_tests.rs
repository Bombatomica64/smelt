use super::*;

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
fn pytest_test_function_without_return_annotation_lowers_to_none() -> TestResult {
    let source = py!("def test_smoke():\n    pass\n");
    let mut ctx = HirCtx::new();
    let module_id = lower_path_module(source, "tests/test_smoke.py", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let item_id = *module
        .items
        .first()
        .ok_or_else(|| "expected lowered pytest test function".to_owned())?;
    let item = ctx
        .krate
        .items
        .get(usize::try_from(item_id.0).map_err(|err| err.to_string())?)
        .ok_or_else(|| "missing lowered pytest test function item".to_owned())?;
    let Item::Function(function) = item else {
        return Err("expected pytest test function item".to_owned());
    };
    ensure(
        matches!(ctx.krate.types.get(function.return_ty), Some(Type::None)),
        "pytest test functions without return annotations lower as None",
    )?;
    Ok(())
}

#[test]
fn pytest_suffix_test_function_without_return_annotation_lowers_to_none() -> TestResult {
    let source = py!("def test_smoke():\n    pass\n");
    let mut ctx = HirCtx::new();
    lower_path_module(source, "tests/smoke_test.py", &mut ctx)?;
    Ok(())
}

#[test]
fn pytest_plain_assert_lowers_to_conditional_failure() -> TestResult {
    let source = py!("def test_truth():\n    assert True\n");
    let mut ctx = HirCtx::new();
    let module_id = lower_path_module(source, "tests/test_truth.py", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let item_id = *module
        .items
        .first()
        .ok_or_else(|| "expected lowered pytest test function".to_owned())?;
    let Item::Function(function) = ctx
        .krate
        .items
        .get(usize::try_from(item_id.0).map_err(|err| err.to_string())?)
        .ok_or_else(|| "missing lowered pytest test function item".to_owned())?
    else {
        return Err("expected pytest test function item".to_owned());
    };
    ensure(
        function.is_test,
        "pytest test function should be marked as test",
    )?;
    let body_id = function
        .body
        .ok_or_else(|| "expected pytest test body".to_owned())?;
    let body = body(&ctx, body_id)?;
    ensure(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::If { .. })),
        "expected assert to lower to if/throw shape",
    )?;
    Ok(())
}

#[test]
fn pytest_assert_not_and_identity_comparisons_lower() -> TestResult {
    let source = py!(r#"
def test_identity():
    value: None = None
    assert not False
    assert value is None
    assert value is not None
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_module(source, "tests/test_identity.py", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let item_id = *module
        .items
        .first()
        .ok_or_else(|| "expected lowered pytest test function".to_owned())?;
    let Item::Function(function) = ctx
        .krate
        .items
        .get(usize::try_from(item_id.0).map_err(|err| err.to_string())?)
        .ok_or_else(|| "missing lowered pytest test function item".to_owned())?
    else {
        return Err("expected pytest test function item".to_owned());
    };
    let body_id = function
        .body
        .ok_or_else(|| "expected pytest test body".to_owned())?;
    let body = body(&ctx, body_id)?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::UnaryOp { .. })),
        "expected assert not to lower through unary not",
    )?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::BinOp { .. })),
        "expected is/is not to lower through equality comparisons",
    )?;
    Ok(())
}

#[test]
fn pytest_raises_context_manager_lowers_to_try_catch() -> TestResult {
    let source = py!(r#"
import pytest

def test_raises():
    with pytest.raises(Exception):
        raise "boom"
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_module(source, "tests/test_raises.py", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let item_id = *module
        .items
        .first()
        .ok_or_else(|| "expected lowered pytest test function".to_owned())?;
    let Item::Function(function) = ctx
        .krate
        .items
        .get(usize::try_from(item_id.0).map_err(|err| err.to_string())?)
        .ok_or_else(|| "missing lowered pytest test function item".to_owned())?
    else {
        return Err("expected pytest test function item".to_owned());
    };
    let body_id = function
        .body
        .ok_or_else(|| "expected pytest test body".to_owned())?;
    let body = body(&ctx, body_id)?;
    ensure(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::TryCatch { .. })),
        "expected pytest.raises to lower to try/catch",
    )?;
    Ok(())
}

#[test]
fn pytest_raises_match_excinfo_and_callable_form_lower() -> TestResult {
    let source = py!(r#"
import pytest

def boom() -> None:
    raise "boom"

def test_raises_forms():
    with pytest.raises(Exception, match="boom") as excinfo:
        boom()
    pytest.raises(Exception, boom, match="boom")
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_module(source, "tests/test_raises_forms.py", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let Item::Function(function) = item(&ctx, module.items[1])? else {
        return Err("expected pytest test function item".to_owned());
    };
    let body = body(&ctx, function.body.ok_or("expected test body")?)?;
    ensure(
        body.stmts
            .iter()
            .filter(|stmt| matches!(stmt, Stmt::TryCatch { .. }))
            .count()
            == 2,
        "expected context and callable pytest.raises forms to lower to try/catch",
    )?;
    ensure(
        body.locals.iter().any(|local| {
            local
                .name
                .is_some_and(|name| symbol(&ctx, name).is_ok_and(|name| name == "excinfo"))
        }),
        "expected pytest.raises as excinfo to bind a catch local",
    )?;
    Ok(())
}

#[test]
fn pytest_parametrize_expands_literal_rows_to_test_functions() -> TestResult {
    let source = py!(r#"
import pytest

@pytest.mark.parametrize("value, expected, pair, items", [(1, 2, (1, "a"), [1, 2]), (3, 4, (3, "b"), [3, 4])])
def test_increment(value, expected):
    assert value + 1 == expected
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_module(source, "tests/test_parametrize.py", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    ensure(
        module.items.len() == 2,
        "expected one generated test function per parameter row",
    )?;
    for (index, item_id) in module.items.iter().enumerate() {
        let Item::Function(function) = ctx
            .krate
            .items
            .get(usize::try_from(item_id.0).map_err(|err| err.to_string())?)
            .ok_or_else(|| "missing lowered pytest test function item".to_owned())?
        else {
            return Err("expected pytest test function item".to_owned());
        };
        ensure(
            function.is_test,
            "generated parametrized case should be a test",
        )?;
        ensure(
            function.params.is_empty(),
            "generated parametrized case should bind parameters as locals",
        )?;
        let expected_name = format!("test_increment__case_{index}");
        ensure(
            symbol(&ctx, function.name)? == expected_name,
            "generated parametrized case should have a stable name",
        )?;
    }
    Ok(())
}

#[test]
fn pytest_parametrize_accepts_ids_and_pytest_param_rows() -> TestResult {
    let source = py!(r#"
import pytest

@pytest.mark.parametrize("value, expected", [pytest.param(1, 2, id="one"), pytest.param(3, 4, marks=pytest.mark.xfail)], ids=["a", "b"])
def test_increment(value, expected):
    assert value + 1 == expected
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_module(source, "tests/test_parametrize_ids.py", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    ensure(
        module.items.len() == 2,
        "expected pytest.param rows to expand into generated tests",
    )?;
    Ok(())
}

#[test]
fn pytest_simple_fixture_binds_test_parameter() -> TestResult {
    let source = py!(r#"
import pytest

@pytest.fixture
def answer() -> int:
    return 41

def test_answer(answer):
    assert answer + 1 == 42
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_module(source, "tests/test_fixture.py", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    ensure(
        module.items.len() == 2,
        "expected fixture function and generated test function",
    )?;
    let Item::Function(function) = item(&ctx, module.items[1])? else {
        return Err("expected pytest test function item".to_owned());
    };
    ensure(function.is_test, "fixture consumer should be a test")?;
    ensure(
        function.params.is_empty(),
        "fixture parameters should be bound as locals, not Rust test parameters",
    )?;
    Ok(())
}

#[test]
fn pytest_fixture_accepts_autouse_and_non_default_scope_syntax() -> TestResult {
    let source = py!(r#"
import pytest

@pytest.fixture(autouse=True)
def autouse_answer() -> int:
    return 41

@pytest.fixture(scope="module")
def scoped_answer() -> int:
    return 41

def test_answer(scoped_answer):
    assert scoped_answer == 41
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_module(source, "tests/test_fixture_scope.py", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    ensure(
        module.items.len() == 3,
        "expected both fixtures and the consuming test to lower",
    )?;
    Ok(())
}

#[test]
fn pytest_autouse_fixture_is_called_in_tests() -> TestResult {
    let source = py!(r#"
import pytest

@pytest.fixture(autouse=True)
def setup() -> int:
    return 1

def test_uses_autouse():
    assert True
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_module(source, "tests/test_autouse.py", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let Item::Function(function) = item(&ctx, module.items[1])? else {
        return Err("expected pytest test function item".to_owned());
    };
    let body = body(&ctx, function.body.ok_or("expected test body")?)?;
    ensure(
        body.stmts.iter().any(|stmt| matches!(stmt, Stmt::Expr(_))),
        "expected autouse fixture call to be injected before test body",
    )?;
    Ok(())
}

#[test]
fn pytest_skip_skipif_and_xfail_do_not_emit_runnable_tests() -> TestResult {
    let source = py!(r#"
import pytest

@pytest.mark.skip
def test_skip():
    assert False

@pytest.mark.skipif(True)
def test_skipif():
    assert False

@pytest.mark.xfail
def test_xfail():
    assert False

@pytest.mark.skipif(False)
def test_runs():
    assert True
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_module(source, "tests/test_skip.py", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    ensure(
        module.items.len() == 1,
        "expected only non-skipped test to lower",
    )?;
    let Item::Function(function) = item(&ctx, module.items[0])? else {
        return Err("expected pytest test function item".to_owned());
    };
    ensure(
        symbol(&ctx, function.name)? == "test_runs",
        "expected skip/xfail tests to be omitted",
    )?;
    Ok(())
}
