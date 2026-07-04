use super::*;

#[test]
fn class_single_inheritance_fields_and_methods_lower() -> TestResult {
    let source = py!(r#"
class Base:
    x: int
    def value(self) -> int:
        return self.x

class Child(Base):
    y: int
    def total(self) -> int:
        return self.value() + self.y
"#);
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    Ok(())
}

#[test]
fn abstract_base_class_method_implementation_lowers() -> TestResult {
    let source = py!(r#"
from abc import ABC, abstractmethod

class Base(ABC):
    @abstractmethod
    def value(self) -> int:
        pass

class Child(Base):
    def value(self) -> int:
        return 1
"#);
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    Ok(())
}

#[test]
fn generic_marker_base_lowers_class_type_params() -> TestResult {
    let source = py!(r#"
from typing import Generic

class Box(Generic[T]):
    value: T
    def __init__(self, value: T) -> None:
        self.value = value
"#);
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
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
exclusive: set[int] = left.symmetric_difference(right)
separate: bool = left.isdisjoint(right)
subset: bool = left.issubset(right)
superset: bool = left.issuperset(right)
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
        SetBinaryOp::SymmetricDifference,
    ] {
        ensure(
            body.exprs
                .iter()
                .any(|expr| matches!(expr.kind, ExprKind::SetBinary { op, .. } if op == expected)),
            "expected set algebra lowering",
        )?;
    }
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::SetDisjoint { .. })),
        "expected set.isdisjoint() predicate lowering",
    )?;
    for expected in [SetRelationOp::IsSubset, SetRelationOp::IsSuperset] {
        ensure(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::SetRelation { op, .. } if op == expected),
            ),
            "expected set relation predicate lowering",
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
fn int_enum_members_lower_as_integer_constants() -> TestResult {
    let source = py!(r#"
from enum import IntEnum

class codes(IntEnum):
    OK = 200
    CREATED = 201, "Created"
    ALSO_OK = codes.OK

ok: int = codes.OK
also_ok: int = codes.ALSO_OK
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let module_body = body(&ctx, module.body.ok_or("expected module body")?)?;
    ensure(
        module_body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::Int(200)))),
        "expected IntEnum member lookup to lower to integer literal",
    )?;
    let class = module
        .items
        .iter()
        .find_map(|item_id| match item(&ctx, *item_id).ok()? {
            Item::Class(class) => Some(class),
            _ => None,
        })
        .ok_or("expected codes class")?;
    let base = class.base.ok_or("expected IntEnum base metadata")?;
    ensure_eq(&symbol(&ctx, base)?, &"IntEnum", "base class")?;
    Ok(())
}

#[test]
fn classmethod_binds_cls_and_lowers_class_level_call() -> TestResult {
    let source = py!(r#"
class codes:
    @classmethod
    def get_reason_phrase(cls, value: int) -> str:
        return "OK" if value == 200 else ""

reason: str = codes.get_reason_phrase(200)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let method = module
        .items
        .iter()
        .find_map(|item_id| match item(&ctx, *item_id).ok()? {
            Item::Function(function)
                if symbol(&ctx, function.name).ok()? == "get_reason_phrase" =>
            {
                Some(function)
            }
            _ => None,
        })
        .ok_or("expected classmethod item")?;
    let method_body = body(&ctx, method.body.ok_or("expected method body")?)?;
    ensure(
        method_body
            .locals
            .iter()
            .any(|local| local.name.and_then(|name| symbol(&ctx, name).ok()) == Some("cls")),
        "expected cls local binding",
    )?;
    let module_body = body(&ctx, module.body.ok_or("expected module body")?)?;
    ensure(
        module_body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Call { .. })),
        "expected class-level classmethod call",
    )?;
    Ok(())
}

#[test]
fn int_enum_new_self_reference_lowers() -> TestResult {
    let source = py!(r#"
from enum import IntEnum

class codes(IntEnum):
    def __new__(cls, value: int, phrase: str = "") -> "codes":
        obj = int.__new__(cls, value)
        obj._value_ = value
        obj.phrase = phrase
        return obj

    OK = codes.__new__(200, "OK")
"#);
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    Ok(())
}

/// Issue #98: a `@staticmethod` lowers to a `ClassStaticMethod`-owned function
/// with no `self` binding, and a class-level variable lowers to a static field.
#[test]
fn staticmethod_and_class_var_lower_to_static_members() -> TestResult {
    let source = py!(r#"
class MathUtils:
    LIMIT = 7

    @staticmethod
    def square(value: float) -> float:
        return value * value

def area(radius: float) -> float:
    return MathUtils.square(radius) * MathUtils.LIMIT
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let class = module
        .items
        .iter()
        .find_map(|item_id| match item(&ctx, *item_id).ok()? {
            Item::Class(class) if symbol(&ctx, class.name).ok()? == "MathUtils" => Some(class),
            _ => None,
        })
        .ok_or("expected MathUtils class")?;

    // The static method is kept in `static_methods` and owns no `self` binding.
    ensure_eq(&class.static_methods.len(), &1, "static method count")?;
    let static_item = class
        .static_methods
        .first()
        .copied()
        .ok_or("missing static method item")?;
    let static_method = item(&ctx, static_item)?;
    let Item::Function(function) = static_method else {
        return Err("static method item is not a function".to_owned());
    };
    ensure(
        matches!(
            function.owner,
            smelt_hir::FunctionOwner::ClassStaticMethod { .. }
        ),
        "expected ClassStaticMethod owner",
    )?;
    let method_body = body(&ctx, function.body.ok_or("expected static method body")?)?;
    ensure(
        !method_body
            .locals
            .iter()
            .any(|local| local.name.and_then(|name| symbol(&ctx, name).ok()) == Some("self")),
        "static method must not bind self",
    )?;

    // The class variable becomes a materialized static field.
    ensure_eq(&class.static_fields.len(), &1, "static field count")?;
    let static_field = class
        .static_fields
        .first()
        .ok_or("missing static field")?;
    ensure(
        matches!(static_field.value, Some(Literal::Int(7))),
        "expected concrete static field value",
    )?;
    Ok(())
}
