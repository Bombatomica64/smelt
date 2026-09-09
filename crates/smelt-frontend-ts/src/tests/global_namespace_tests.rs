//! A modeled global namespace object, used as a value.
//!
//! The registry (`smelt_stdlib::GLOBAL_NAMESPACES`) is the single source of
//! truth: an entry there is a present object wherever it is used as a value,
//! and a namespace-valued member of one (`crypto.subtle`) is likewise. These
//! tests pin the LOWERING; the observable answers (`typeof` is `"object"`,
//! truthiness, `=== undefined`) are executed end to end in
//! `crates/smelt-codegen-rust/tests/global_namespace_value_runtime.rs`.

use super::*;

/// Every registry namespace lowers to the host namespace value, `crypto` too.
///
/// `crypto` was in none of the per-path lists before, which is why Hono's
/// `if (crypto && crypto.subtle)` reported an unresolved identifier for a
/// global whose members Smelt already lowers.
#[test]
fn a_registry_namespace_lowers_to_a_host_namespace_value() -> Result<(), String> {
    for name in ["Math", "JSON", "Reflect", "Intl", "crypto"] {
        let mut ctx = HirCtx::new();
        let source = format!("export const held = {name};\n");
        lower_ok(&source, &mut ctx)?;
        ensure!(
            ctx.krate.bodies.iter().any(|body| {
                body.exprs.iter().any(|expr| matches!(
                    &expr.kind,
                    ExprKind::BuiltinNamespace { name: spelled } if spelled == name
                ))
            }),
            "{name} should lower to a host namespace value"
        );
    }
    Ok(())
}

/// A namespace-valued member lowers to the sub-namespace value, in both
/// spellings, and only for members the registry lists.
#[test]
fn a_namespace_valued_member_lowers_to_the_sub_namespace() -> Result<(), String> {
    for source in [
        "export const held = crypto.subtle;\n",
        "export const held = globalThis.crypto.subtle;\n",
    ] {
        let mut ctx = HirCtx::new();
        lower_ok(source, &mut ctx)?;
        ensure!(
            ctx.krate.bodies.iter().any(|body| {
                body.exprs.iter().any(|expr| matches!(
                    &expr.kind,
                    ExprKind::BuiltinNamespace { name } if name == "crypto.subtle"
                ))
            }),
            "`crypto.subtle` should lower to a host namespace value: {source}"
        );
    }

    // A member the registry does not list as a namespace keeps whatever the
    // ordinary property paths answer: the rule must not turn every property of
    // a namespace into a present object.
    let mut ctx = HirCtx::new();
    lower_ok("export const held = crypto.notANamespace;\n", &mut ctx)?;
    ensure!(
        !ctx.krate.bodies.iter().any(|body| {
            body.exprs.iter().any(|expr| matches!(
                &expr.kind,
                ExprKind::BuiltinNamespace { name } if name == "crypto.notANamespace"
            ))
        }),
        "an unlisted member must not become a namespace value"
    );
    Ok(())
}

/// A local binding of the same name shadows the global.
#[test]
fn a_local_binding_shadows_the_namespace() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function describe(): string {
  const crypto = { subtle: "local" };
  return crypto.subtle;
}
"#),
        &mut ctx,
    )?;
    ensure!(
        !ctx.krate.bodies.iter().any(|body| {
            body.exprs
                .iter()
                .any(|expr| matches!(&expr.kind, ExprKind::BuiltinNamespace { .. }))
        }),
        "a local `crypto` must not resolve to the global namespace"
    );
    Ok(())
}
