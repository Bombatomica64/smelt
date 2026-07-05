//! Tests for `ty`-backed type resolution (issue #93).
//!
//! These are gated on the crate's `ty` feature because they depend on Astral's
//! `ty` resolving Python types from source that omits (or mis-declares) type
//! annotations. Without the feature the frontend still requires explicit
//! annotations, which is covered by `category_tests`.
//!
//! Run with: `cargo test -p smelt-frontend-py --features ty ty_resolution`

use super::{ensure, ensure_eq, item, lower_module};
use crate::HirCtx;
use smelt_hir::{Item, Type};

/// A function with no return annotation lowers, with the return type `ty`
/// inferred from the body (`x + 1` → `int`).
#[test]
fn function_without_return_annotation_lowers() -> Result<(), String> {
    let source = "def inc(x: int):\n    return x + 1\n";
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = crate::tests::module(&ctx, module_id)?;
    let item_id = module
        .items
        .first()
        .copied()
        .ok_or_else(|| "expected function item".to_owned())?;
    let Item::Function(func) = item(&ctx, item_id)? else {
        return Err("expected Function item".to_owned());
    };
    ensure_eq(
        &ctx.krate.types.get(func.return_ty),
        &Some(&Type::Int),
        "resolved return type",
    )?;
    Ok(())
}

/// Several unannotated functions in one module all resolve their return types
/// from their bodies and lower together.
///
/// This exercises the dominant blocker family from the library probes: plain
/// `def`s with no `-> T`. (Unannotated *parameters* are a separate, deferred
/// case: `ty` reports an unannotated parameter as dynamic, so those still
/// require a source annotation — see the module-level note in
/// `smelt-py-types`.)
#[test]
fn multiple_unannotated_functions_lower() -> Result<(), String> {
    let source = concat!(
        "def inc(x: int):\n    return x + 1\n\n",
        "def label(name: str):\n    return \"hi \" + name\n\n",
        "def flag(b: bool):\n    return not b\n",
    );
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = crate::tests::module(&ctx, module_id)?;
    ensure_eq(&module.items.len(), &3, "item count")?;

    let expected = [Type::Int, Type::String, Type::Bool];
    for (index, want) in expected.iter().enumerate() {
        let Item::Function(func) = item(&ctx, module.items[index])? else {
            return Err(format!("item {index} is not a function"));
        };
        ensure_eq(
            &ctx.krate.types.get(func.return_ty),
            &Some(want),
            "resolved return type",
        )?;
    }
    Ok(())
}

/// A method with no return annotation lowers, with the return type resolved by
/// `ty` from the body.
#[test]
fn method_without_return_annotation_lowers() -> Result<(), String> {
    let source = "class Counter:\n    def __init__(self, n: int):\n        self.n = n\n\n    def snapshot(self, extra: int):\n        return extra + 1\n";
    let mut ctx = HirCtx::new();
    // `snapshot` has no `-> T`; without the `ty` feature this raises
    // "method '...' must have an explicit return type annotation".
    lower_module(source, &mut ctx)?;
    Ok(())
}

/// A valueless function (no `return <expr>`) resolves to `None`.
#[test]
fn function_returning_none_lowers() -> Result<(), String> {
    let source = "def noop(x: int):\n    pass\n";
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = crate::tests::module(&ctx, module_id)?;
    let item_id = module
        .items
        .first()
        .copied()
        .ok_or_else(|| "expected function item".to_owned())?;
    let Item::Function(func) = item(&ctx, item_id)? else {
        return Err("expected Function item".to_owned());
    };
    ensure_eq(
        &ctx.krate.types.get(func.return_ty),
        &Some(&Type::None),
        "resolved None return type",
    )?;
    Ok(())
}

/// When a declared return annotation diverges from the actual returned type,
/// lowering follows `ty`'s inferred type, not the stale annotation.
///
/// Here the body returns an `int` (`x + 1`) but the source annotates `-> float`.
/// `ty` reports the actual `int`, and the frontend uses it.
#[test]
fn divergent_return_annotation_prefers_inferred() -> Result<(), String> {
    let source = "def inc(x: int) -> float:\n    return x + 1\n";
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = crate::tests::module(&ctx, module_id)?;
    let item_id = module
        .items
        .first()
        .copied()
        .ok_or_else(|| "expected function item".to_owned())?;
    let Item::Function(func) = item(&ctx, item_id)? else {
        return Err("expected Function item".to_owned());
    };
    // The actual returned type is `int`; the annotation said `float`.
    ensure(
        matches!(ctx.krate.types.get(func.return_ty), Some(Type::Int)),
        "expected inferred int return type to override float annotation",
    )?;
    Ok(())
}

/// An explicit `-> float` annotation is preserved even when `ty` infers a wider
/// type for the body (issue #93 regression).
///
/// `value * value` for `value: float` is inferred by `ty` as a broader numeric
/// type (`int | float`) than the declared `float`. The declared `float` is a
/// valid, equal-or-narrower description of that inference, so lowering must keep
/// the annotation and emit `f64` rather than degrading into an erased union.
#[test]
fn annotated_return_kept_when_ty_infers_wider() -> Result<(), String> {
    let source = "def square(value: float) -> float:\n    return value * value\n";
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = crate::tests::module(&ctx, module_id)?;
    let item_id = module
        .items
        .first()
        .copied()
        .ok_or_else(|| "expected function item".to_owned())?;
    let Item::Function(func) = item(&ctx, item_id)? else {
        return Err("expected Function item".to_owned());
    };
    // The explicit `-> float` contract must survive `ty`'s wider inference.
    ensure_eq(
        &ctx.krate.types.get(func.return_ty),
        &Some(&Type::Float),
        "annotated float return type must be preserved",
    )?;
    Ok(())
}

/// A `@staticmethod` annotated `-> float` keeps `float`, mirroring the codegen
/// regression where `square` degraded into an erased union under ty-default.
#[test]
fn annotated_staticmethod_return_kept_when_ty_infers_wider() -> Result<(), String> {
    let source = concat!(
        "class MathUtils:\n",
        "    @staticmethod\n",
        "    def square(value: float) -> float:\n",
        "        return value * value\n",
    );
    let mut ctx = HirCtx::new();
    // Lowering succeeds and does not panic; the static method's `-> float`
    // contract is honoured (verified end to end by the codegen `f64` test).
    lower_module(source, &mut ctx)?;
    Ok(())
}

/// An UNANNOTATED function still resolves its return type via `ty` after the
/// annotation-respecting reconciliation change: the fix must only affect the
/// annotated path, leaving the #93 inference-for-unannotated behaviour intact.
#[test]
fn unannotated_return_still_resolves_via_ty() -> Result<(), String> {
    let source = "def to_text(x: int):\n    return str(x)\n";
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = crate::tests::module(&ctx, module_id)?;
    let item_id = module
        .items
        .first()
        .copied()
        .ok_or_else(|| "expected function item".to_owned())?;
    let Item::Function(func) = item(&ctx, item_id)? else {
        return Err("expected Function item".to_owned());
    };
    // No annotation was present; `ty` resolved the body's `str(x)` to `str`.
    ensure_eq(
        &ctx.krate.types.get(func.return_ty),
        &Some(&Type::String),
        "ty-inferred return type for unannotated function",
    )?;
    Ok(())
}
