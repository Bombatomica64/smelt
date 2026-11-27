//! Class-body variables (`count = 0` in a class): constants, live class-level
//! storage, and the stores whose `CPython` meaning Smelt refuses.
use super::*;

/// Whether static field `name` of class `class` has live storage.
fn has_storage(ctx: &HirCtx, class_name: &str, name: &str) -> Result<bool, String> {
    let class = class_named(ctx, class_name)?;
    let field = class
        .static_fields
        .iter()
        .find(|field| symbol(ctx, field.name) == Ok(name))
        .ok_or_else(|| format!("missing class variable `{name}`"))?;
    Ok(field.storage.is_some())
}

/// The first diagnostic lowering `source` produces.
fn refusal(source: &str) -> Result<String, String> {
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    Ok(first_error(&errors)?.message.clone())
}

#[test]
fn class_variable_stored_through_its_class_gets_storage_and_others_stay_constant() -> TestResult {
    let source = py!(r"
class Counter:
    count = 0
    label = 'counter'

    def __init__(self) -> None:
        Counter.count += 1

    def text(self) -> str:
        return self.label + str(self.count)
");
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    ensure(
        has_storage(&ctx, "Counter", "count")?,
        "`Counter.count += 1` makes `count` live storage",
    )?;
    ensure(
        !has_storage(&ctx, "Counter", "label")?,
        "a never-stored class variable stays a constant",
    )
}

#[test]
fn class_variable_stored_through_cls_gets_storage() -> TestResult {
    let source = py!(r"
class Counter:
    count = 0

    @classmethod
    def bump(cls) -> None:
        cls.count += 1
");
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    ensure(
        has_storage(&ctx, "Counter", "count")?,
        "`cls.count += 1` makes `count` live storage",
    )
}

#[test]
fn store_through_a_subclass_is_refused() -> TestResult {
    let message = refusal(py!(r"
class Base:
    count = 0

class Child(Base):
    pass

def main() -> None:
    Child.count = 3
"))?;
    ensure(
        message.contains("storing `count` through `Child`"),
        format!("expected the per-subclass diagnostic, got {message}"),
    )
}

#[test]
fn store_through_cls_of_a_subclassed_class_is_refused() -> TestResult {
    let message = refusal(py!(r"
class Base:
    count = 0

    @classmethod
    def bump(cls) -> None:
        cls.count += 1

class Child(Base):
    pass
"))?;
    ensure(
        message.contains("`Base` has subclasses"),
        format!("expected the subclassed-cls diagnostic, got {message}"),
    )
}

#[test]
fn instance_store_shadowing_a_class_variable_outside_init_is_refused() -> TestResult {
    let message = refusal(py!(r"
class Counter:
    count = 0

    def reset(self) -> None:
        self.count = 5
"))?;
    ensure(
        message.contains("shadowing the class variable"),
        format!("expected the instance-shadowing diagnostic, got {message}"),
    )
}

#[test]
fn init_assignment_shadows_a_class_variable_with_an_instance_field() -> TestResult {
    let source = py!(r"
class Named:
    name = 'default'

    def __init__(self, given: str) -> None:
        self.name = given.upper()
");
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    ensure_class_shape(&ctx, "Named", "Named { name: PyStr }")
}
