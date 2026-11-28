//! Python's `collections` containers and `typing.NamedTuple` classes: typed
//! lowering, resolution by binding, and the refusals of shapes with no types.
use super::*;

/// Whether `ctx` holds a `PyCollectionOp` calling `method`.
fn has_collection_op(code: &ModuleCode<'_>, method: smelt_hir::PyCollectionMethod) -> bool {
    code.exprs().any(|expr| matches!(&expr.kind, ExprKind::PyCollectionOp { method: found, .. } if *found == method))
}

#[test]
fn counter_of_a_str_is_a_typed_counter_of_str() -> TestResult {
    let source = py!(r#"
from collections import Counter
letters = Counter("abca")
first: int = letters["a"]
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let code = module_code(&ctx, module)?;
    let typed = code.exprs().any(|expr| {
        matches!(expr.kind, ExprKind::PyCollectionNew { ctor: smelt_hir::PyCollectionCtor::Counter, .. })
            && matches!(
                ctx.krate.types.get(expr.ty),
                Some(Type::Class { name, args })
                    if ctx.krate.symbols.get(*name) == Some(smelt_stdlib::PY_COUNTER_CLASS_NAME)
                        && args.len() == 1
                        && ctx.krate.types.get(args[0]) == Some(&Type::PyStr)
            )
    });
    ensure(typed, "expected `Counter(str)` to build a Counter[str]")?;
    // `c[k]` reads the count without inserting.
    ensure(has_collection_op(&code, smelt_hir::PyCollectionMethod::Get), "expected a non-inserting count read")
}

#[test]
fn defaultdict_subscript_inserts_and_get_does_not() -> TestResult {
    let source = py!(r#"
import collections
groups: collections.defaultdict[str, list[int]] = collections.defaultdict(list)
groups["a"].append(1)
missing: list[int] | None = groups.get("b")
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let code = module_code(&ctx, module)?;
    ensure(
        has_collection_op(&code, smelt_hir::PyCollectionMethod::GetOrInsert),
        "expected `d[k]` to insert the factory's value",
    )?;
    ensure(has_collection_op(&code, smelt_hir::PyCollectionMethod::Get), "expected `d.get(k)` not to insert")
}

#[test]
fn a_program_class_named_deque_is_its_own_class() -> TestResult {
    let source = py!(r#"
class deque:
    def __init__(self) -> None:
        self.size = 0

queue = deque()
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let code = module_code(&ctx, module)?;
    ensure(
        !code.exprs().any(|expr| matches!(expr.kind, ExprKind::PyCollectionNew { .. })),
        "a program's own `deque` must not become collections.deque",
    )
}

#[test]
fn collection_shapes_without_types_are_refused() -> TestResult {
    let cases = [
        ("from collections import deque\nqueue = deque()\n", "annotate the target"),
        ("from collections import deque\nqueue: deque = deque([1])\n", "type arguments"),
        (
            "from collections import Counter\nc: Counter[str] = Counter(\"ab\")\nn: int = c[1]\n",
            "declared type",
        ),
        (
            "from collections import namedtuple\nPoint = namedtuple(\"Point\", [\"x\", \"y\"])\n",
            "typing.NamedTuple",
        ),
    ];
    for (source, expected) in cases {
        let mut ctx = HirCtx::new();
        let errors = lower_errors(source, &mut ctx)?;
        let message = &first_error(&errors)?.message;
        ensure(message.contains(expected), format!("expected `{expected}` in the diagnostic, got `{message}`"))?;
    }
    Ok(())
}

#[test]
fn named_tuple_index_reads_the_declared_field() -> TestResult {
    let source = py!(r#"
from typing import NamedTuple

class Point(NamedTuple):
    x: int
    label: str

def describe(p: Point) -> str:
    x, label = p
    return label + str(p[0] + x) + p[-1]
"#);
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    ensure(
        ctx.krate.bodies.iter().flat_map(|code| code.exprs.iter()).any(|expr| matches!(expr.kind, ExprKind::Field { field, .. } if ctx.krate.symbols.get(field) == Some("label"))
            && ctx.krate.types.get(expr.ty) == Some(&Type::PyStr)),
        "expected `p[-1]` to read the `label: str` field",
    )
}
