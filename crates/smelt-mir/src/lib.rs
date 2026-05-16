//! MIR (Medium-level Intermediate Representation) crate for the Smelt compiler.
//!
//! This crate provides the MIR IR for functions, types, and control flow,
//! as well as utilities for lowering HIR to MIR, optimizing MIR, validating MIR,
//! and formatting MIR for debugging.

#![expect(
    clippy::too_many_lines,
    reason = "MIR lowering and validation functions will be split during the next architecture pass"
)]
#![expect(
    clippy::too_many_arguments,
    reason = "lowering context construction mirrors the MIR function shape for now"
)]
#![expect(
    clippy::format_push_string,
    reason = "debug formatters favor straightforward string assembly until the formatter module is rewritten"
)]
#![expect(
    clippy::option_if_let_else,
    reason = "current control-flow validation is clearer with explicit branches"
)]
#![expect(
    clippy::match_same_arms,
    reason = "separate MIR variants are kept visually distinct in validators"
)]
#![expect(
    clippy::missing_errors_doc,
    reason = "public compiler entrypoints need documentation polish as a separate pass"
)]
#![expect(
    clippy::missing_const_for_fn,
    reason = "const qualification will be applied once constructors are stabilized"
)]
#![expect(
    clippy::map_unwrap_or,
    reason = "existing option pipelines are being preserved until the lowering refactor"
)]
#![expect(
    clippy::unused_self,
    reason = "lowering helpers remain methods to keep the API shape stable during refactors"
)]
#![expect(
    clippy::semicolon_if_nothing_returned,
    reason = "validator match arms keep expression style for consistency"
)]
#![expect(
    clippy::similar_names,
    reason = "MIR lowering uses related HIR/MIR block names that differ only by role"
)]
#![expect(
    clippy::exhaustive_enums,
    reason = "MIR is the internal compiler data model and codegen/validators match variants directly"
)]
#![expect(
    clippy::exhaustive_structs,
    reason = "MIR structs are constructed across workspace crates during lowering and tests"
)]
#![expect(
    clippy::shadow_unrelated,
    reason = "optimizer match arms reuse domain variable names in independent scopes"
)]

/// Compact MIR formatting utilities.
mod format;
/// HIR-to-MIR lowering pipeline.
mod lower;
/// MIR optimization passes.
pub mod opt;
/// MIR core data types.
mod types;
/// MIR validation and diagnostics.
mod validate;

pub use format::format_compact;
pub use lower::{LowerError, lower_hir};
pub use types::*;
pub use validate::{ValidationError, validate};

#[cfg(test)]
mod tests {
    use super::*;
    use smelt_frontend_ts::{HirCtx, to_hir};
    use smelt_hir::FileId;
    use std::convert::TryFrom;

    fn ok_or_panic<T, E: std::fmt::Debug>(result: Result<T, E>, context: &str) -> T {
        result.unwrap_or_else(|error| {
            std::panic::resume_unwind(Box::new(format!("{context}: {error:?}")))
        })
    }

    fn block_mut(function: &mut MirFunction, block: BlockId) -> &mut BasicBlock {
        let index = usize::try_from(block.0).ok().unwrap_or_else(|| {
            std::panic::resume_unwind(Box::new(format!(
                "block id {block:?} does not fit in usize"
            )))
        });
        function.blocks.get_mut(index).unwrap_or_else(|| {
            std::panic::resume_unwind(Box::new(format!("block index {index} is out of bounds")))
        })
    }

    #[test]
    fn lowers_top_level_let_and_console_log_to_mir() {
        let mut ctx = HirCtx::new();
        ok_or_panic(
            to_hir(
                "let count = 42;\nconsole.log(count);\n",
                FileId(0),
                &mut ctx,
            ),
            "HIR",
        );

        let mut mir = ok_or_panic(lower_hir(&ctx.krate), "MIR");
        opt::optimize(&mut mir);

        assert!(validate(&mir).is_empty());
        let output = format_compact(&mir);
        assert!(output.contains("fn main (FuncId(0)) -> None"));
        assert!(output.contains("%0 user count: Float"));
        assert!(output.contains("%0 = 42.0"));
        assert!(output.contains("%1 = call @console_log(copy %0) -> bb1"));
        assert!(output.contains("return none"));
    }

    #[test]
    fn copy_propagation_rewrites_alias_uses() {
        let mut ctx = HirCtx::new();
        ok_or_panic(
            to_hir(
                "const sourceValue = 7;\nlet copiedValue = sourceValue;\nconsole.log(copiedValue);\n",
                FileId(0),
                &mut ctx,
            ),
            "HIR",
        );

        let mut mir = ok_or_panic(lower_hir(&ctx.krate), "MIR");
        opt::optimize(&mut mir);
        let output = format_compact(&mir);

        assert!(output.contains("%2 = call @console_log(copy %0) -> bb1"));
    }

    #[test]
    fn while_and_for_of_lower_to_cfg() {
        let mut ctx = HirCtx::new();
        ok_or_panic(
            to_hir(
                "let count = 0;
while (count < 10) {
  break;
}
",
                FileId(0),
                &mut ctx,
            ),
            "HIR",
        );

        let while_mir = ok_or_panic(lower_hir(&ctx.krate), "while lowers");
        assert!(validate(&while_mir).is_empty());
        let while_output = format_compact(&while_mir);
        assert!(while_output.contains("goto bb1"));
        assert!(while_output.contains("switch copy %1 ? bb2 : bb3"));
        assert!(while_output.contains("goto bb3"));

        let mut for_ctx = HirCtx::new();
        ok_or_panic(
            to_hir(
                "let values = 1;
for (let item: number of values) {
  continue;
}
",
                FileId(0),
                &mut for_ctx,
            ),
            "HIR",
        );

        let for_mir = ok_or_panic(lower_hir(&for_ctx.krate), "for lowers");
        assert!(validate(&for_mir).is_empty());
        let for_output = format_compact(&for_mir);
        assert!(for_output.contains("len copy %0"));
    }

    #[test]
    fn throw_lowers_to_terminating_mir() {
        let mut ctx = HirCtx::new();
        ok_or_panic(
            to_hir(
                "function fail(): void {
  throw \"boom\";
}
fail();
",
                FileId(0),
                &mut ctx,
            ),
            "HIR",
        );

        let mir = ok_or_panic(lower_hir(&ctx.krate), "throw lowers");
        assert!(validate(&mir).is_empty());
        let output = format_compact(&mir);

        assert!(output.contains("fn fail (FuncId(0)) -> None throws"));
        assert!(output.contains("fn main (FuncId(1)) -> None throws"));
        assert!(output.contains("throw \"boom\""));
    }

    #[test]
    fn async_await_lowers_to_mir_await_rvalue() {
        let mut ctx = HirCtx::new();
        ok_or_panic(
            to_hir(
                "async function lift(value: number): Promise<number> {
  return value;
}

async function run(): Promise<number> {
  return await lift(5);
}
",
                FileId(0),
                &mut ctx,
            ),
            "HIR",
        );

        let mir = ok_or_panic(lower_hir(&ctx.krate), "async lowers");
        assert!(validate(&mir).is_empty());
        let output = format_compact(&mir);

        assert!(output.contains("async fn lift (FuncId(0)) -> Float"));
        assert!(output.contains("async fn run (FuncId(1)) -> Float"));
        assert!(output.contains("%0 = call fn0(5.0) -> bb1"));
        assert!(output.contains("%1 = await copy %0"));
    }

    #[test]
    fn try_catch_lowers_caught_throw_to_cfg() {
        let mut ctx = HirCtx::new();
        ok_or_panic(
            to_hir(
                "try {
  throw \"boom\";
} catch (err: string) {
  console.log(err);
}
",
                FileId(0),
                &mut ctx,
            ),
            "HIR",
        );

        let mir = ok_or_panic(lower_hir(&ctx.krate), "try/catch lowers");
        assert!(validate(&mir).is_empty());
        let output = format_compact(&mir);

        assert!(output.contains("fn main (FuncId(0)) -> None\n"));
        assert!(output.contains("%0 = \"boom\""));
        assert!(!output.contains("throw \"boom\""));
    }

    #[test]
    fn validation_is_cfg_aware_for_definite_assignment() {
        let mut types = smelt_hir::TypeInterner::default();
        let mut symbols = smelt_hir::SymbolInterner::default();
        let bool_ty = types.intern(smelt_hir::Type::Bool);
        let none_ty = types.intern(smelt_hir::Type::None);
        let name = symbols.intern("main");
        let mut mir = Mir::new(types, symbols);
        let mut function = MirFunction::new(
            FuncId(0),
            name,
            HirOrigin::Body(smelt_hir::BodyId(0)),
            none_ty,
            smelt_hir::Span::new(FileId(0), 0, 0),
        );
        let cond = function.push_local(LocalDecl {
            ty: bool_ty,
            kind: LocalKind::Param { symbol: None },
            span: smelt_hir::Span::new(FileId(0), 0, 0),
        });
        let branch_only = function.push_local(LocalDecl {
            ty: bool_ty,
            kind: LocalKind::Temp,
            span: smelt_hir::Span::new(FileId(0), 0, 0),
        });
        function.params.push(cond);
        let then_block = function.push_block(smelt_hir::Span::new(FileId(0), 0, 0));
        let else_block = function.push_block(smelt_hir::Span::new(FileId(0), 0, 0));
        let join_block = function.push_block(smelt_hir::Span::new(FileId(0), 0, 0));
        let entry = function.entry;
        block_mut(&mut function, entry).terminator = Some(Terminator::Switch {
            cond: Operand::Copy(Place::Local(cond)),
            then_block,
            else_block,
        });
        block_mut(&mut function, then_block)
            .statements
            .push(Statement::Assign {
                dest: branch_only,
                value: Rvalue::Use(Operand::Const(Constant::Bool(true))),
            });
        block_mut(&mut function, then_block).terminator = Some(Terminator::Goto(join_block));
        block_mut(&mut function, else_block).terminator = Some(Terminator::Goto(join_block));
        block_mut(&mut function, join_block).terminator =
            Some(Terminator::Return(Operand::Copy(Place::Local(branch_only))));
        mir.push_function(function);

        let errors = validate(&mir);
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("definitely defined")),
            "expected definite-assignment error, got {errors:?}"
        );
    }
}
