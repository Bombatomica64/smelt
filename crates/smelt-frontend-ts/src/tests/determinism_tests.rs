//! Lowering must not depend on the per-map hash seed.
//!
//! Every `HashMap` built in one process draws fresh `RandomState` keys, so
//! lowering the same source repeatedly in one test exercises the seed variation
//! that otherwise only shows between `smelt` runs.

use super::*;

/// How many lowerings of one source are compared; an order that leaks from a
/// hash map survives this many fresh seeds with negligible probability.
const REPEATS: usize = 24;

/// Lower `source` afresh and render everything downstream stages read.
///
/// The type interner renders entry by entry: its lookup index is a hash map that
/// nothing iterates, so its `Debug` form differs between otherwise equal crates.
fn lowered_crate(source: &str) -> Result<String, String> {
    let mut ctx = HirCtx::new();
    lower_ok(source, &mut ctx)?;
    let krate = &ctx.krate;
    let types = (0..)
        .map_while(|index| krate.types.get(smelt_hir::TypeId(index)))
        .collect::<Vec<_>>();
    Ok(format!("{:?} {:?} {:?} {types:?}", krate.modules, krate.items, krate.bodies))
}

/// Assert `REPEATS` lowerings of `source` render byte-identically.
fn ensure_stable(source: &str) -> Result<(), String> {
    let first = lowered_crate(source)?;
    for _ in 1..REPEATS {
        ensure!(lowered_crate(source)? == first);
    }
    Ok(())
}

#[test]
fn object_namespace_entries_keep_source_order() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
type Formatter = (value: string) => string;

export const table: Record<string, Formatter> = {
  zulu: (value) => value + "z",
  yankee: (value) => value + "y",
  alpha: (value) => value + "a",
  papa: (value) => value + "p",
  Papa: (value) => value + "P",
  bravo: (value) => value + "b",
  mike: (value) => value + "m",
  kilo: (value) => value + "k",
};
"#),
        &mut ctx,
    )?;
    let namespace = ctx
        .object_namespaces
        .exported("table")
        .ok_or_else(|| "expected table namespace metadata".to_owned())?;
    let keys = namespace.keys().map(String::as_str).collect::<Vec<_>>();
    ensure_eq!(keys, ["zulu", "yankee", "alpha", "papa", "Papa", "bravo", "mike", "kilo"]);
    Ok(())
}

#[test]
fn callback_function_table_arms_do_not_depend_on_the_hash_seed() -> Result<(), String> {
    ensure_stable(ts!(r#"
type Formatter = (value: string) => string;

const zulu: Formatter = (value) => value + "z";
const alpha: Formatter = (value) => value + "a";
const papa: Formatter = (value) => value + "p";
const kilo: Formatter = (value) => value + "k";

export const table: Record<string, Formatter> = { zulu, alpha, papa, kilo, Papa: papa, mike: kilo };

export function apply(values: string[]): string[] {
  return values.map((value) => {
    const formatter = table[value];
    return formatter(value);
  });
}
"#))
}

#[test]
fn deferred_export_renames_resolve_their_locals_before_publishing_aliases() -> Result<(), String> {
    for _ in 0..REPEATS {
        let mut ctx = HirCtx::new();
        lower_ok(
            ts!(r"
class First {}
class Second {}
export { First as Second, Second as Third };
"),
            &mut ctx,
        )?;
        let name = |alias: &str| -> Result<String, String> {
            let item = ctx
                .export_aliases
                .iter()
                .find(|(key, _)| key.name == alias)
                .map(|(_, item)| *item)
                .ok_or_else(|| format!("no alias {alias}"))?;
            match ctx.krate.items.get(item.0 as usize) {
                Some(Item::Class(class)) => ctx
                    .krate
                    .symbols
                    .get(class.name)
                    .map(str::to_owned)
                    .ok_or_else(|| "unnamed class".to_owned()),
                _ => Err(format!("alias {alias} is not a class")),
            }
        };
        ensure_eq!(name("Second")?, "First");
        ensure_eq!(name("Third")?, "Second");
    }
    Ok(())
}
