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
    clippy::cast_possible_truncation,
    reason = "MIR IDs are compact u32 indexes and overflow checks will be centralized separately"
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
    clippy::assertions_on_result_states,
    reason = "test assertions are intentionally direct in the MIR smoke suite"
)]
#![expect(
    clippy::assert_without_message,
    reason = "debug assertions in ID plumbing are self-describing from the compared values"
)]
#![expect(
    clippy::map_unwrap_or,
    reason = "existing option pipelines are being preserved until the lowering refactor"
)]
#![expect(
    clippy::redundant_clone,
    reason = "clone sites in lowering will be reviewed alongside ownership cleanup"
)]
#![expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "validator helper signatures currently prefer uniform borrowed operands"
)]
#![expect(
    clippy::needless_pass_by_value,
    reason = "lowering helpers take owned operands so call sites can move or inspect them uniformly"
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

mod format;
mod lower;
pub mod opt;
mod types;
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

    #[test]
    fn lowers_top_level_let_and_console_log_to_mir() {
        let mut ctx = HirCtx::new();
        to_hir(
            "let count = 42;\nconsole.log(count);\n",
            FileId(0),
            &mut ctx,
        )
        .expect("HIR");

        let mut mir = lower_hir(&ctx.krate).expect("MIR");
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
        to_hir(
            "const sourceValue = 7;\nlet copiedValue = sourceValue;\nconsole.log(copiedValue);\n",
            FileId(0),
            &mut ctx,
        )
        .expect("HIR");

        let mut mir = lower_hir(&ctx.krate).expect("MIR");
        opt::optimize(&mut mir);
        let output = format_compact(&mir);

        assert!(output.contains("%2 = call @console_log(copy %0) -> bb1"));
    }

    #[test]
    fn while_and_for_of_lower_to_cfg() {
        let mut ctx = HirCtx::new();
        to_hir(
            "let count = 0;
while (count < 10) {
  break;
}
",
            FileId(0),
            &mut ctx,
        )
        .expect("HIR");

        let mir = lower_hir(&ctx.krate).expect("while lowers");
        assert!(validate(&mir).is_empty());
        let output = format_compact(&mir);
        assert!(output.contains("switch copy %1 ? bb1 : bb2"));
        assert!(output.contains("goto bb2"));

        let mut ctx = HirCtx::new();
        to_hir(
            "let values = 1;
for (let item: number of values) {
  continue;
}
",
            FileId(0),
            &mut ctx,
        )
        .expect("HIR");

        let mir = lower_hir(&ctx.krate).expect("for lowers");
        assert!(validate(&mir).is_empty());
        let output = format_compact(&mir);
        assert!(output.contains("len copy %0"));
    }

    #[test]
    fn throw_lowers_to_terminating_mir() {
        let mut ctx = HirCtx::new();
        to_hir(
            "function fail(): void {
  throw \"boom\";
}
fail();
",
            FileId(0),
            &mut ctx,
        )
        .expect("HIR");

        let mir = lower_hir(&ctx.krate).expect("throw lowers");
        assert!(validate(&mir).is_empty());
        let output = format_compact(&mir);

        assert!(output.contains("fn fail (FuncId(0)) -> None throws"));
        assert!(output.contains("fn main (FuncId(1)) -> None throws"));
        assert!(output.contains("throw \"boom\""));
    }

    #[test]
    fn async_await_lowers_to_mir_await_rvalue() {
        let mut ctx = HirCtx::new();
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
        )
        .expect("HIR");

        let mir = lower_hir(&ctx.krate).expect("async lowers");
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
        to_hir(
            "try {
  throw \"boom\";
} catch (err: string) {
  console.log(err);
}
",
            FileId(0),
            &mut ctx,
        )
        .expect("HIR");

        let mir = lower_hir(&ctx.krate).expect("try/catch lowers");
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
            kind: LocalKind::Param,
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
        function.blocks[0].terminator = Some(Terminator::Switch {
            cond: Operand::Copy(Place::Local(cond)),
            then_block,
            else_block,
        });
        function.blocks[then_block.0 as usize]
            .statements
            .push(Statement::Assign {
                dest: branch_only,
                value: Rvalue::Use(Operand::Const(Constant::Bool(true))),
            });
        function.blocks[then_block.0 as usize].terminator = Some(Terminator::Goto(join_block));
        function.blocks[else_block.0 as usize].terminator = Some(Terminator::Goto(join_block));
        function.blocks[join_block.0 as usize].terminator =
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
