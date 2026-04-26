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
    fn while_and_for_lowering_fail_without_panicking() {
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

        let errors = lower_hir(&ctx.krate).expect_err("while lowering is deferred");
        assert!(errors[0].message.contains("while CFG lowering"));

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

        let errors = lower_hir(&ctx.krate).expect_err("for lowering is deferred");
        assert!(errors[0].message.contains("for CFG lowering"));
    }
}
