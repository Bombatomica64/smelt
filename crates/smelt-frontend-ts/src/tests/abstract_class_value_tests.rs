//! An abstract class used as a value constructs nothing.
//!
//! TypeScript rejects `new` on an abstract class, through an `abstract new (..)`
//! constructor type as well, so its class value carries no constructor closure
//! (a closure would build an instance of a class that has none).

use super::*;

/// Every `ClassValue` expression of the module body, with whether it carries a constructor.
fn class_values(source: &str) -> Result<Vec<bool>, String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    Ok(body
        .exprs
        .iter()
        .filter_map(|expr| match &expr.kind {
            ExprKind::ClassValue { constructor, .. } => Some(constructor.is_some()),
            _ => None,
        })
        .collect())
}

#[test]
fn an_abstract_class_value_has_no_constructor() -> Result<(), String> {
    let values = class_values(
        "abstract class Shape {}\nclass Square extends Shape {}\n\
         const kinds: unknown[] = [Shape, Square];\nconsole.log(kinds.length);\n",
    )?;
    ensure!(values == vec![false, true], "constructors: {values:?}");
    Ok(())
}
