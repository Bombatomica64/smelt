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
fn optional_dataclass_field_lowers_as_optional() -> TestResult {
    // A dataclass field annotated `Optional[int]` (or `int | None`) must record
    // `optional: true` and intern its type as `Type::Optional`, while a plain
    // required field stays non-optional. This mirrors how TypeScript class fields
    // carry the `?` spelling and lets Rust codegen emit `Option<T>` for the
    // optional slot only.
    let source = py!(r#"
from dataclasses import dataclass
from typing import Optional

@dataclass
class Point:
    x: int
    y: Optional[int] = None
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
        ensure_eq(&c.fields.len(), &2, "field count")?;
        let x = c
            .fields
            .iter()
            .find(|field| symbol(&ctx, field.name) == Ok("x"))
            .ok_or_else(|| "missing field x".to_owned())?;
        let y = c
            .fields
            .iter()
            .find(|field| symbol(&ctx, field.name) == Ok("y"))
            .ok_or_else(|| "missing field y".to_owned())?;
        ensure(!x.optional, "required field x must stay non-optional")?;
        ensure(
            !matches!(ctx.krate.types.get(x.ty), Some(Type::Optional(_))),
            "required field x must not be Type::Optional",
        )?;
        ensure(y.optional, "field y must be marked optional")?;
        ensure(
            matches!(ctx.krate.types.get(y.ty), Some(Type::Optional(_))),
            "field y must intern as Type::Optional",
        )?;
    } else {
        return Err("expected Class item".to_owned());
    }
    Ok(())
}

#[test]
fn pep604_optional_dataclass_field_lowers_as_optional() -> TestResult {
    // The PEP 604 `int | None` spelling must lower the same way as
    // `Optional[int]`: the field is optional and its type is `Type::Optional`.
    let source = py!(r#"
from dataclasses import dataclass

@dataclass
class Config:
    name: str
    retries: int | None = None
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
        let retries = c
            .fields
            .iter()
            .find(|field| symbol(&ctx, field.name) == Ok("retries"))
            .ok_or_else(|| "missing field retries".to_owned())?;
        ensure(retries.optional, "field retries must be marked optional")?;
        ensure(
            matches!(ctx.krate.types.get(retries.ty), Some(Type::Optional(_))),
            "field retries must intern as Type::Optional",
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

// ---------------------------------------------------------------------------
// Issue #94: method and non-top-level calls.
// ---------------------------------------------------------------------------

/// Return the body of the named function/method item in `module`.
fn method_body_named<'a>(
    ctx: &'a HirCtx,
    module: &Module,
    name: &str,
) -> Result<&'a Body, String> {
    let item_id = module
        .items
        .iter()
        .copied()
        .find(|item_id| match item(ctx, *item_id) {
            Ok(Item::Function(function)) => symbol(ctx, function.name).ok() == Some(name),
            _ => false,
        })
        .ok_or_else(|| format!("missing method '{name}'"))?;
    let Item::Function(function) = item(ctx, item_id)? else {
        return Err(format!("item '{name}' is not a function"));
    };
    body(ctx, function.body.ok_or("expected method body")?)
}

/// An instance method may call a sibling method declared *later* in the class
/// body: method items are pre-registered before any body is lowered, so the
/// forward reference resolves and lowers to a receiver method call.
#[test]
fn instance_method_forward_reference_lowers() -> TestResult {
    let source = py!(r#"
class Counter:
    value: int
    def __init__(self, value: int) -> None:
        self.value = value
    def total(self) -> int:
        return self.doubled()
    def doubled(self) -> int:
        return self.value * 2
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let total = method_body_named(&ctx, module, "total")?;
    ensure(
        total
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Method { .. })),
        "expected self.doubled() to lower as a receiver method call",
    )?;
    Ok(())
}

/// Inside a `@classmethod`, `cls(args)` constructs the owning class and
/// `cls.helper()` dispatches to a sibling classmethod/staticmethod as a
/// receiver-free associated call.
#[test]
fn classmethod_cls_construction_and_dispatch_lower() -> TestResult {
    let source = py!(r#"
class Counter:
    value: int
    def __init__(self, value: int) -> None:
        self.value = value
    @classmethod
    def make(cls, start: int) -> "Counter":
        base = cls(start)
        return cls(base.value + cls.origin())
    @staticmethod
    def origin() -> int:
        return 0
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let make = method_body_named(&ctx, module, "make")?;
    // `cls(...)` lowers to class construction, not a receiver method call.
    ensure(
        make.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::New { .. })),
        "expected cls(args) to lower as class construction",
    )?;
    // `cls.origin()` (staticmethod) lowers to a receiver-free associated call.
    ensure(
        make.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Call { .. })),
        "expected cls.origin() to lower as an associated call",
    )?;
    // No `cls.origin()` receiver method call was emitted for the associated call.
    // The classmethod itself dispatches receiver-free, so its owner is static.
    let Item::Function(make_fn) = item(
        &ctx,
        module
            .items
            .iter()
            .copied()
            .find(|item_id| matches!(
                item(&ctx, *item_id),
                Ok(Item::Function(function)) if symbol(&ctx, function.name).ok() == Some("make")
            ))
            .ok_or("missing make item")?,
    )?
    else {
        return Err("make is not a function".to_owned());
    };
    ensure(
        matches!(
            make_fn.owner,
            smelt_hir::FunctionOwner::ClassStaticMethod { .. }
        ),
        "expected @classmethod to lower with a receiver-free (static) owner",
    )?;
    Ok(())
}

/// A `@classmethod` called qualified (`Class.make(..)`) lowers to a
/// receiver-free associated call, and a subsequent instance method on its
/// result chains correctly.
#[test]
fn classmethod_qualified_call_and_chain_lower() -> TestResult {
    let source = py!(r#"
class Counter:
    value: int
    def __init__(self, value: int) -> None:
        self.value = value
    def total(self) -> int:
        return self.value
    @classmethod
    def make(cls, start: int) -> "Counter":
        return cls(start)

def via_factory(start: int) -> int:
    return Counter.make(start).total()
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let factory = method_body_named(&ctx, module, "via_factory")?;
    // `Counter.make(start)` is an associated call; `.total()` is a method call.
    ensure(
        factory
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Call { .. })),
        "expected Counter.make(..) associated call",
    )?;
    ensure(
        factory
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Method { .. })),
        "expected chained .total() receiver method call",
    )?;
    Ok(())
}

/// A `@staticmethod` may call a sibling declared later via `Class.helper()`.
#[test]
fn staticmethod_forward_reference_lowers() -> TestResult {
    let source = py!(r#"
class Ops:
    @staticmethod
    def start() -> int:
        return Ops.base() + 1
    @staticmethod
    def base() -> int:
        return 10
"#);
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    Ok(())
}

/// A method call to a name the receiver class does not declare is still
/// rejected with the unsupported-call diagnostic.
#[test]
fn unknown_instance_method_rejects() -> TestResult {
    let source = py!(r#"
class C:
    def m(self) -> int:
        return 1

def f(c: C) -> int:
    return c.nope()
"#);
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    let error = first_error(&errors)?;
    ensure(
        error.message.contains("only calls to top-level functions"),
        "expected the unsupported-call diagnostic for an unknown method",
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Instance fields declared by assignment in `__init__` (issue #94).
// ---------------------------------------------------------------------------

/// A field assigned from an `__init__` parameter is declared with that
/// parameter's type, so a method call through the field dispatches statically.
///
/// Without the implicit declaration the class lowers with no fields at all, and
/// `field_type`'s fieldless-class fallback types `self.inner` as `B` itself —
/// so `self.inner.a()` looked for `a` on `B`, found nothing, and fell through
/// to the unsupported-call diagnostic.
#[test]
fn constructor_assigned_field_supports_method_dispatch() -> TestResult {
    let source = py!(r#"
class A:
    def a(self) -> int:
        return 1

class B:
    def __init__(self, inner: A) -> None:
        self.inner = inner
    def b(self) -> int:
        return self.inner.a()
"#);
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    Ok(())
}

/// The implicitly declared field carries the parameter's real type, not the
/// enclosing class's. Passing it to a function typed for the field's class must
/// type-check.
#[test]
fn constructor_assigned_field_has_the_parameter_type() -> TestResult {
    let source = py!(r#"
class A:
    def a(self) -> int:
        return 1

def take(x: A) -> int:
    return x.a()

class B:
    def __init__(self, inner: A) -> None:
        self.inner = inner
    def b(self) -> int:
        return take(self.inner)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let class_item_id = module
        .items
        .iter()
        .rev()
        .find(|&&item_id| matches!(item(&ctx, item_id), Ok(Item::Class(_))))
        .copied()
        .ok_or("expected a class item")?;
    let Item::Class(class) = item(&ctx, class_item_id)? else {
        return Err("expected Class item".to_owned());
    };
    ensure_eq(&class.fields.len(), &1, "implicit field count")?;
    ensure_eq(
        &symbol(&ctx, class.fields[0].name)?,
        &"inner",
        "implicit field name",
    )?;
    Ok(())
}

/// An explicit class-level annotation still wins: the field is declared once,
/// with the annotated type, and the `__init__` assignment does not duplicate it.
#[test]
fn class_level_annotation_wins_over_constructor_assignment() -> TestResult {
    let source = py!(r#"
class Point:
    x: int
    def __init__(self, x: int) -> None:
        self.x = x
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let class_item_id = module
        .items
        .iter()
        .rev()
        .find(|&&item_id| matches!(item(&ctx, item_id), Ok(Item::Class(_))))
        .copied()
        .ok_or("expected a class item")?;
    let Item::Class(class) = item(&ctx, class_item_id)? else {
        return Err("expected Class item".to_owned());
    };
    ensure_eq(&class.fields.len(), &1, "field count (no duplicate)")?;
    Ok(())
}

/// `self.<name>: T = <value>` inside `__init__` declares the field from its own
/// annotation, even when the value is not a plain parameter reference.
#[test]
fn annotated_constructor_assignment_declares_field() -> TestResult {
    let source = py!(r#"
class A:
    def a(self) -> int:
        return 1

class B:
    def __init__(self) -> None:
        self.inner: A = A()
    def b(self) -> int:
        return self.inner.a()
"#);
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// `super().__init__(...)` in a derived constructor (issue #94).
// ---------------------------------------------------------------------------

/// A derived `__init__` may run its base's initialization via `super()`.
///
/// Rust has no class inheritance, so the call has no callee to defer to: it
/// lowers to constructing the base and copying its (flattened) slots onto
/// `self`, matching the TypeScript frontend's `super(...)` lowering.
#[test]
fn super_init_call_lowers_in_a_derived_constructor() -> TestResult {
    let source = py!(r#"
class A:
    def __init__(self, x: int) -> None:
        self.x = x

class B(A):
    def __init__(self, x: int, y: int) -> None:
        super().__init__(x)
        self.y = y
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let has_base_construction = ctx.krate.bodies.iter().any(|candidate| {
        candidate
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::New { .. }))
    });
    ensure(
        has_base_construction,
        "expected super().__init__() to construct the base class",
    )?;
    let _ = module;
    Ok(())
}

/// `super()` outside a class that declares a base is rejected with a specific
/// message rather than the generic unsupported-call diagnostic.
#[test]
fn super_init_without_a_base_class_is_rejected() -> TestResult {
    let source = py!(r#"
class A:
    def __init__(self, x: int) -> None:
        super().__init__()
        self.x = x
"#);
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    ensure(
        first_error(&errors)?.message.contains("base class"),
        "expected a specific diagnostic when the class declares no base",
    )?;
    Ok(())
}

/// `super().<method>()` on an ordinary method is refused explicitly.
///
/// Under flattening an override *replaces* the inherited slot in the derived
/// impl, so the base body is not present to call; dispatching back through
/// `self` would recurse forever. The limit is stated rather than silently
/// mis-lowered.
#[test]
fn super_method_call_is_refused_with_a_specific_message() -> TestResult {
    let source = py!(r#"
class A:
    def greet(self) -> int:
        return 1

class B(A):
    def greet(self) -> int:
        return super().greet() + 1
"#);
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    ensure(
        first_error(&errors)?
            .message
            .contains("only super().__init__() is lowered"),
        "expected the specific super()-method diagnostic",
    )?;
    Ok(())
}

/// A subclass without an explicit `__init__` inherits its base constructor.
///
/// The synthesized derived constructor must keep the base parameter list;
/// replacing it with the ordinary zero-argument default makes the valid
/// `Child(7)` call lower to a non-existent `Child::new(7)` overload.
#[test]
fn subclass_without_init_inherits_base_constructor_signature() -> TestResult {
    let source = py!(r#"
class Base:
    def __init__(self, value: int) -> None:
        self.value = value

class Child(Base):
    pass

def make() -> Child:
    return Child(7)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let child = module
        .items
        .iter()
        .find_map(|&item_id| match item(&ctx, item_id) {
            Ok(Item::Class(class)) if symbol(&ctx, class.name).ok() == Some("Child") => Some(class),
            _ => None,
        })
        .ok_or("expected Child class")?;
    let constructor = child.constructor.ok_or("expected Child constructor")?;
    let Item::Function(function) = item(&ctx, constructor)? else {
        return Err("expected Child constructor function".to_owned());
    };
    ensure_eq(
        &function.params.len(),
        &1,
        "inherited constructor parameter count",
    )?;
    ensure_eq(
        &symbol(&ctx, function.params[0].name)?,
        &"value",
        "inherited constructor parameter name",
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Type-parameter declarations and `@property` getters.
// ---------------------------------------------------------------------------

/// `T = TypeVar("T")` is a type-system declaration, not a runtime call.
///
/// Python *requires* the statement — `Generic[T]` will not compile without it —
/// so rejecting it made the only correct spelling of a generic class an error.
#[test]
fn typevar_declaration_lowers_with_a_generic_class() -> TestResult {
    let source = py!(r#"
from typing import Generic, TypeVar

T = TypeVar("T")

class Box(Generic[T]):
    value: T
    def __init__(self, value: T) -> None:
        self.value = value
"#);
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    Ok(())
}

/// `ParamSpec` and `TypeVarTuple`, and the `typing.`-qualified spelling, are
/// the same kind of declaration and are skipped the same way.
#[test]
fn param_spec_and_qualified_type_param_declarations_lower() -> TestResult {
    let source = py!(r#"
import typing
from typing import ParamSpec, TypeVar, TypeVarTuple

P = ParamSpec("P")
Ts = TypeVarTuple("Ts")
U = typing.TypeVar("U")
V = TypeVar("V", bound=int)

def identity(value: V) -> V:
    return value
"#);
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    Ok(())
}

/// A `@property` getter lowers as an instance method and registers a read-only
/// descriptor, so the property is readable through field syntax.
#[test]
fn property_getter_lowers_as_a_readable_descriptor() -> TestResult {
    let source = py!(r#"
class Ok:
    def __init__(self, value: int) -> None:
        self._value = value

    @property
    def ok_value(self) -> int:
        return self._value

def read(o: Ok) -> int:
    return o.ok_value
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let class_item_id = module
        .items
        .iter()
        .find(|&&item_id| matches!(item(&ctx, item_id), Ok(Item::Class(_))))
        .copied()
        .ok_or("expected a class item")?;
    let Item::Class(class) = item(&ctx, class_item_id)? else {
        return Err("expected Class item".to_owned());
    };
    ensure_eq(&class.descriptors.len(), &1, "property descriptor count")?;
    ensure_eq(
        &symbol(&ctx, class.descriptors[0].name)?,
        &"ok_value",
        "descriptor name",
    )?;
    ensure(
        class.descriptors[0].getter.is_some(),
        "the descriptor must carry the source getter",
    )?;
    ensure(
        class.descriptors[0].write_ty.is_none() && !class.descriptors[0].data_descriptor,
        "a bare @property is read-only",
    )?;
    Ok(())
}

/// `__slots__` and `__match_args__` are `CPython` metadata, not class variables.
///
/// Both are tuples, so the class-variable rule (a single name bound to a
/// literal) rejected them and blocked any class that declares its layout the
/// idiomatic way.
#[test]
fn class_metadata_assignments_are_skipped() -> TestResult {
    let source = py!(r#"
class Ok:
    __match_args__ = ("ok_value",)
    __slots__ = ("_value",)

    def __init__(self, value: int) -> None:
        self._value = value

    @property
    def ok_value(self) -> int:
        return self._value
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let class_item_id = module
        .items
        .iter()
        .find(|&&item_id| matches!(item(&ctx, item_id), Ok(Item::Class(_))))
        .copied()
        .ok_or("expected a class item")?;
    let Item::Class(class) = item(&ctx, class_item_id)? else {
        return Err("expected Class item".to_owned());
    };
    ensure(
        class.static_fields.is_empty(),
        "class metadata must not materialize as static fields",
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Type aliases and union-receiver method dispatch.
// ---------------------------------------------------------------------------

/// All three type-alias spellings resolve to the type they name.
#[test]
fn type_alias_spellings_resolve() -> TestResult {
    for source in [
        // PEP 613: the explicit `TypeAlias` annotation.
        py!(r#"
from typing import TypeAlias, Union

class Ok:
    def is_ok(self) -> bool:
        return True

class Err:
    def is_ok(self) -> bool:
        return False

Result: TypeAlias = Union[Ok, Err]

def check(r: Result) -> bool:
    return r.is_ok()
"#),
        // Pre-3.12 idiom: a bare assignment of a type expression.
        py!(r#"
class Ok:
    def is_ok(self) -> bool:
        return True

class Err:
    def is_ok(self) -> bool:
        return False

Result = Ok | Err

def check(r: Result) -> bool:
    return r.is_ok()
"#),
        // PEP 695 statement form.
        py!(r#"
class Ok:
    def is_ok(self) -> bool:
        return True

class Err:
    def is_ok(self) -> bool:
        return False

type Result = Ok | Err

def check(r: Result) -> bool:
    return r.is_ok()
"#),
    ] {
        let mut ctx = HirCtx::new();
        lower_module(source, &mut ctx)?;
    }
    Ok(())
}

/// An ordinary value assignment is not mistaken for a type alias.
///
/// A type alias is *skipped* during module-body lowering, so a false match would
/// silently delete the assignment. The module body must still bind the value.
#[test]
fn value_assignment_is_not_a_type_alias() -> TestResult {
    let source = py!(r#"
LIMIT = 10
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let module_body = body(&ctx, module.body.ok_or("expected a module body")?)?;
    ensure(
        module_body
            .stmts
            .iter()
            .any(|stmt| matches!(stmt, smelt_hir::Stmt::Let { .. })),
        "a value assignment must still lower as a binding, not be skipped as an alias",
    )?;
    Ok(())
}

/// A parameterised alias substitutes its arguments positionally, so
/// `Pair[int, str]` really is `(int, str)` and not the declared placeholders.
#[test]
fn parameterised_type_alias_substitutes_arguments() -> TestResult {
    let source = py!(r#"
from typing import Tuple, TypeAlias, TypeVar

A = TypeVar("A")
B = TypeVar("B")

Pair: TypeAlias = Tuple[A, B]

def first(p: Pair[int, str]) -> int:
    return p[0]
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let Item::Function(function) = item(
        &ctx,
        *module.items.last().ok_or("expected a function item")?,
    )?
    else {
        return Err("expected Function item".to_owned());
    };
    let param_ty = function
        .params
        .first()
        .map(|param| param.ty)
        .ok_or("expected a parameter")?;
    let Some(Type::Tuple(items)) = ctx.krate.types.get(param_ty) else {
        return Err("expected the alias to lower to a tuple".to_owned());
    };
    ensure_eq(&items.len(), &2, "tuple arity")?;
    ensure(
        matches!(ctx.krate.types.get(items[0]), Some(Type::Int))
            && matches!(ctx.krate.types.get(items[1]), Some(Type::String)),
        "alias parameters must be replaced by the supplied arguments",
    )?;
    Ok(())
}

/// A union receiver dispatches when every arm declares the method, and is
/// refused when one does not.
#[test]
fn union_receiver_requires_every_arm_to_declare_the_method() -> TestResult {
    let source = py!(r#"
class Ok:
    def is_ok(self) -> bool:
        return True

class Err:
    def other(self) -> bool:
        return False

def check(r: Ok | Err) -> bool:
    return r.is_ok()
"#);
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    ensure(
        !errors.is_empty(),
        "a union arm missing the method must not dispatch",
    )?;
    Ok(())
}
