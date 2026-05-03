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
