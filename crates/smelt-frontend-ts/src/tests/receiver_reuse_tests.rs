//! A member call lowers its receiver once.
//!
//! The probe that asks whether a receiver declares the called member lowers the
//! receiver; every later route of the same call reuses that lowering. Lowering
//! it again at each level of a method chain `make().with(..).with(..)` would make
//! the work exponential in the chain's length and leave a copy of every closure
//! in the chain behind as an unreachable body.

use super::*;

/// A builder class whose `with` returns the builder, and a chain of `links`
/// `with` calls on a call receiver, each passing one closure.
fn chain_source(links: usize) -> String {
    let chain: String = std::iter::once("make()".to_owned())
        .chain((0..links).map(|index| format!(".with(() => {index})")))
        .collect();
    format!(
        "class Builder {{\n  total = 0;\n  with(step: () => number): Builder {{ this.total += step(); return this; }}\n}}\n\
         function make(): Builder {{ return new Builder(); }}\n\
         export const built = {chain};\n\
         console.log(built.total);\n"
    )
}

/// `(bodies, expressions)` the crate holds after lowering a chain of `links` calls.
fn lowered_size(links: usize) -> Result<(usize, usize), String> {
    let mut ctx = HirCtx::new();
    lower_ok(&chain_source(links), &mut ctx)?;
    let exprs = ctx.krate.bodies.iter().map(|body| body.exprs.len()).sum();
    Ok((ctx.krate.bodies.len(), exprs))
}

#[test]
fn each_link_of_a_method_chain_adds_one_closure_body_and_constant_expressions() -> Result<(), String>
{
    let (bodies_2, exprs_2) = lowered_size(2)?;
    let (bodies_4, exprs_4) = lowered_size(4)?;
    let (bodies_6, exprs_6) = lowered_size(6)?;
    ensure!(
        bodies_4 - bodies_2 == 2,
        "one body per closure: {bodies_2} {bodies_4}"
    );
    ensure!(
        bodies_6 - bodies_4 == 2,
        "one body per closure: {bodies_4} {bodies_6}"
    );
    ensure!(
        exprs_6 - exprs_4 == exprs_4 - exprs_2,
        "expressions grow linearly: {exprs_2} {exprs_4} {exprs_6}"
    );
    Ok(())
}
