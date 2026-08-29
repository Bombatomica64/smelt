//! MIR (Medium-level Intermediate Representation) crate for the Smelt compiler.
//!
//! This crate provides the MIR IR for functions, types, and control flow,
//! as well as utilities for lowering HIR to MIR, optimizing MIR, validating MIR,
//! and formatting MIR for debugging.

#![expect(
    clippy::too_many_lines,
    reason = "the remaining long functions are single exhaustive matches over the full \
              ExprKind/Rvalue surface (lower_expr, the for_each_operand walkers, the \
              format/opt dispatchers); compile-time exhaustiveness keeps them in sync \
              and splitting them per variant family would hide that guarantee"
)]
#![expect(
    clippy::match_same_arms,
    reason = "exhaustive Rvalue/ExprKind walkers in validators and optimizers keep \
              per-variant arms distinct so new variants get an explicit review site \
              instead of silently joining a merged arm"
)]
#![expect(
    clippy::similar_names,
    reason = "MIR lowering pairs HIR and MIR blocks with names that differ only by \
              role (then_hir/then_mir, body_hir/body_mir); the pairing is the point"
)]
#![expect(
    clippy::exhaustive_enums,
    reason = "MIR is the internal compiler data model and codegen/validators match variants directly"
)]
#![expect(
    clippy::exhaustive_structs,
    reason = "MIR structs are constructed across workspace crates during lowering and tests"
)]

/// Promote erased-and-mutated record locals to an erased value representation.
mod erased_record_promote;
/// Compact MIR formatting utilities.
mod format;
/// HIR-to-MIR lowering pipeline.
mod lower;
/// MIR optimization passes.
pub mod opt;
/// MIR operational type normalization.
mod type_normalize;
/// MIR core data types.
mod types;
/// MIR validation and diagnostics.
mod validate;

pub use erased_record_promote::promote_erased_mutated_records;
pub use format::format_compact;
pub use lower::{LowerError, lower_hir};
pub use type_normalize::normalize_operational_types;
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
        // `count` is dead after the log call, so move-on-last-use rewrites the
        // final `copy %0` into a `move` (dropping a defensive clone in codegen).
        assert!(output.contains("%1 = call @console_log(move %0) -> bb1"));
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

        // Copy propagation resolves the alias to `%0`; move-on-last-use then
        // turns the final use into a `move` since `%0` is dead afterwards.
        assert!(output.contains("%2 = call @console_log(move %0) -> bb1"));
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
    fn c_style_for_continue_reruns_update_before_header() {
        // Regression: a `continue` inside a C-style `for` must re-run the update
        // clause before re-testing the condition. The desugaring lowers the
        // update into a dedicated latch block that is the `continue` target, so
        // `continue` cannot skip the update and spin forever.
        let mut ctx = HirCtx::new();
        ok_or_panic(
            to_hir(
                "let total = 0;
for (let i = 0; i < 10; i++) {
  if (i === 3) {
    continue;
  }
  total = total + i;
}
",
                FileId(0),
                &mut ctx,
            ),
            "HIR",
        );

        let mir = ok_or_panic(lower_hir(&ctx.krate), "c-for lowers");
        assert!(validate(&mir).is_empty());
        let output = format_compact(&mir);

        // Header (bb1) tests the condition; the latch (bb3) increments `i` and
        // jumps back to the header. Crucially, the `continue` arm (bb5) routes
        // through the latch (`goto bb3`), NOT straight to the header (bb1), so
        // the increment always runs.
        assert!(
            output.contains("bb3:"),
            "expected a distinct latch block:\n{output}"
        );
        assert!(
            output.contains("%1 = copy %5"),
            "latch should apply the `i++` update:\n{output}"
        );
        assert!(
            output.contains("bb5:\n    goto bb3"),
            "continue must jump to the update latch (bb3), not the header:\n{output}"
        );
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
        let mut mir = Mir::new(types, symbols, smelt_hir::OriginalNameTable::default());
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

    /// Optimize a TypeScript snippet and return its formatted MIR.
    fn optimized_mir(source: &str) -> String {
        let mut ctx = HirCtx::new();
        ok_or_panic(to_hir(source, FileId(0), &mut ctx), "HIR");
        let mut mir = ok_or_panic(lower_hir(&ctx.krate), "MIR");
        opt::optimize(&mut mir);
        assert!(validate(&mir).is_empty());
        format_compact(&mir)
    }

    #[test]
    fn move_on_last_use_keeps_earlier_uses_as_copy() {
        // `value` is read by two separate calls; only the final read may move.
        let output = optimized_mir("const value = \"hi\";\nconsole.log(value);\nconsole.log(value);\n");
        assert!(
            output.contains("@console_log(copy %0)"),
            "first use must stay a copy:\n{output}"
        );
        assert!(
            output.contains("@console_log(move %0)"),
            "final use must become a move:\n{output}"
        );
    }

    #[test]
    fn move_on_last_use_preserves_loop_carried_values() {
        // `index` is live across the loop back-edge, so its read inside the exit
        // condition must remain a copy. Only the dead boolean condition
        // temporary may move into the switch (the Rust emitter reconstructs the
        // structured loop from either `switch copy` or `switch move`).
        let output = optimized_mir(
            "let total = 0;\nlet index = 0;\nwhile (index < 3) {\n  total = total + index;\n  index = index + 1;\n}\nconsole.log(total);\n",
        );
        assert!(
            output.contains("copy %1 < 3.0"),
            "the loop-carried index read in the condition must stay a copy:\n{output}"
        );
        assert!(
            output.contains("switch move"),
            "the dead boolean condition temporary moves into the switch:\n{output}"
        );
    }

    #[test]
    fn move_on_last_use_does_not_move_function_parameters() {
        // Parameters are excluded; the final read of `a` keeps its copy so a
        // later borrow pass (not this one) can decide the calling convention.
        let output = optimized_mir(
            "function identity(a: string): string {\n  return a;\n}\nconsole.log(identity(\"x\"));\n",
        );
        assert!(
            output.contains("return copy"),
            "a returned parameter must remain a copy:\n{output}"
        );
    }

    #[test]
    fn propagates_generic_class_type_params_into_mir() {
        // Issue #99: HIR generic class type parameters survive into `MirClass`
        // so codegen can emit `struct Container<T>` and `impl<T> Container<T>`.
        let mut ctx = HirCtx::new();
        ok_or_panic(
            to_hir(
                "class Container<T> {\n  value: T;\n  constructor(value: T) { this.value = value; }\n  get(): T { return this.value; }\n}\n",
                FileId(0),
                &mut ctx,
            ),
            "HIR",
        );

        let mir = ok_or_panic(lower_hir(&ctx.krate), "MIR");
        let container = mir
            .classes
            .iter()
            .find(|class| mir.symbols.get(class.name) == Some("Container"))
            .unwrap_or_else(|| {
                std::panic::resume_unwind(Box::new("Container class lowered to MIR".to_owned()))
            });
        assert_eq!(container.type_params.len(), 1);
        let type_param = container.type_params.first().unwrap_or_else(|| {
            std::panic::resume_unwind(Box::new("Container has a type parameter".to_owned()))
        });
        assert_eq!(
            mir.symbols.get(type_param.name),
            Some("T"),
            "the class type parameter name is preserved in MIR"
        );
    }

    #[test]
    fn propagates_generic_free_function_type_params_into_mir() {
        // Issue #99: HIR generic free-function type parameters survive into
        // `MirFunction` so codegen can emit `fn identity<T>(x: T) -> T`.
        let mut ctx = HirCtx::new();
        ok_or_panic(
            to_hir(
                "export function identity<T>(x: T): T {\n  return x;\n}\n",
                FileId(0),
                &mut ctx,
            ),
            "HIR",
        );

        let mir = ok_or_panic(lower_hir(&ctx.krate), "MIR");
        let identity = mir
            .functions
            .iter()
            .find(|function| mir.symbols.get(function.name) == Some("identity"))
            .unwrap_or_else(|| {
                std::panic::resume_unwind(Box::new("identity function lowered to MIR".to_owned()))
            });
        assert_eq!(identity.type_params.len(), 1);
        let type_param = identity.type_params.first().unwrap_or_else(|| {
            std::panic::resume_unwind(Box::new("identity has a type parameter".to_owned()))
        });
        assert_eq!(
            mir.symbols.get(type_param.name),
            Some("T"),
            "the free-function type parameter name is preserved in MIR"
        );
    }

    /// The `groupBy` accumulator shape: a guarded default insert followed by a
    /// push through the same entry.
    const GUARDED_GROUP_BY: &str = "\
export function groupBy<T, K extends PropertyKey>(
  arr: readonly T[],
  getKeyFromItem: (item: T, index: number) => K
): Record<K, T[]> {
  const result = {} as Record<K, T[]>;
  for (let i = 0; i < arr.length; i++) {
    const item = arr[i];
    const key = getKeyFromItem(item, i);
    if (!Object.hasOwn(result, key)) {
      result[key] = [];
    }
    result[key].push(item);
  }
  return result;
}
";

    /// Lowers TypeScript, runs every default optimization pass, and formats the
    /// result. Panics if lowering or validation fails.
    fn optimized_mir_text(source: &str) -> String {
        let mut ctx = HirCtx::new();
        ok_or_panic(to_hir(source, FileId(0), &mut ctx), "HIR");
        let mut mir = ok_or_panic(lower_hir(&ctx.krate), "MIR");
        opt::optimize(&mut mir);
        assert!(validate(&mir).is_empty(), "optimized MIR validates");
        format_compact(&mir)
    }

    #[test]
    fn dict_default_insert_elision_drops_the_guarded_seed() {
        let output = optimized_mir_text(GUARDED_GROUP_BY);

        // The probe and its negation are gone, and the guard block falls
        // straight through to the join that mutates the entry.
        assert!(!output.contains("dict_contains_key"), "{output}");
        assert!(!output.contains(" = !"), "{output}");
        assert!(
            output.contains("%5 = move %9\n    goto bb6\n"),
            "the guard block ends in a goto at the join: {output}"
        );
        assert!(
            output.contains("%14 = list_push copy %2[copy %5], move %4"),
            "the entry mutation still inserts the key: {output}"
        );
        // The seeding block survives as an unreachable block: block ids are
        // positional, so nothing renumbers it away.
        assert!(output.contains("%2[copy %5] = move %12"), "{output}");
    }

    /// Appends a second reader of the `dict_contains_key` temporary to the join
    /// block, without disturbing the guard's statement adjacency. Returns the
    /// probe temporary that gained the extra reader.
    fn add_second_contains_reader(mir: &mut Mir) -> LocalId {
        for function in &mut mir.functions {
            let Some((probe, join)) = find_contains_probe(function) else {
                continue;
            };
            let decl = function
                .locals
                .get(usize::try_from(probe.0).unwrap_or(usize::MAX))
                .unwrap_or_else(|| {
                    std::panic::resume_unwind(Box::new("probe local is declared".to_owned()))
                })
                .clone();
            let extra = LocalId(u32::try_from(function.locals.len()).unwrap_or(u32::MAX));
            function.locals.push(decl);
            block_mut(function, join).statements.push(Statement::Assign {
                dest: extra,
                value: Rvalue::Unary {
                    op: smelt_hir::UnaryOp::Not,
                    operand: Operand::Copy(Place::Local(probe)),
                },
            });
            return probe;
        }
        std::panic::resume_unwind(Box::new("a dict_contains_key probe was lowered".to_owned()))
    }

    /// Finds a `dict_contains_key` destination and the block its guard's switch
    /// falls through to when the key is present.
    fn find_contains_probe(function: &MirFunction) -> Option<(LocalId, BlockId)> {
        function.blocks.iter().find_map(|block| {
            let probe = block.statements.iter().find_map(|statement| match statement {
                Statement::Assign {
                    dest,
                    value: Rvalue::DictContainsKey { .. },
                } => Some(*dest),
                _ => None,
            })?;
            let Some(Terminator::Switch { else_block, .. }) = block.terminator else {
                return None;
            };
            Some((probe, else_block))
        })
    }

    #[test]
    fn dict_default_insert_elision_keeps_a_guard_with_a_second_reader() {
        let mut ctx = HirCtx::new();
        ok_or_panic(to_hir(GUARDED_GROUP_BY, FileId(0), &mut ctx), "HIR");
        let mut mir = ok_or_panic(lower_hir(&ctx.krate), "MIR");
        add_second_contains_reader(&mut mir);
        opt::optimize(&mut mir);

        let output = format_compact(&mir);
        assert!(
            output.contains("dict_contains_key"),
            "a second reader of the probe observes a value the elision deletes: {output}"
        );
    }

    #[test]
    fn dict_default_insert_elision_keeps_a_non_empty_seed() {
        // `[item]` is not the default `entry_or_insert` would synthesize, so the
        // guarded store is not redundant.
        let output = optimized_mir_text(&GUARDED_GROUP_BY.replace("= [];", "= [item];"));

        assert!(output.contains("dict_contains_key"), "{output}");
    }

    #[test]
    fn dict_default_insert_elision_keeps_a_read_only_join() {
        // The join only READS the entry, which the backend renders as a
        // non-inserting lookup. Deleting the guard would leave the key absent.
        let output = optimized_mir_text(
            "\
export function groupBy<T, K extends PropertyKey>(
  arr: readonly T[],
  getKeyFromItem: (item: T, index: number) => K
): Record<K, T[]> {
  const result = {} as Record<K, T[]>;
  const sizes: number[] = [];
  for (let i = 0; i < arr.length; i++) {
    const item = arr[i];
    const key = getKeyFromItem(item, i);
    if (!Object.hasOwn(result, key)) {
      result[key] = [];
    }
    const group = result[key];
    sizes.push(group.length);
  }
  return result;
}
",
        );

        assert!(output.contains("dict_contains_key"), "{output}");
    }

    #[test]
    fn dict_default_insert_elision_keeps_a_guard_on_another_key() {
        let output = optimized_mir_text(
            "\
export function groupBy<T, K extends PropertyKey>(
  arr: readonly T[],
  getKeyFromItem: (item: T, index: number) => K
): Record<K, T[]> {
  const result = {} as Record<K, T[]>;
  for (let i = 0; i < arr.length; i++) {
    const item = arr[i];
    const key = getKeyFromItem(item, i);
    const other = getKeyFromItem(item, i + 1);
    if (!Object.hasOwn(result, key)) {
      result[key] = [];
    }
    result[other].push(item);
  }
  return result;
}
",
        );

        assert!(output.contains("dict_contains_key"), "{output}");
    }


    /// The `countBy` accumulator shape: a read-modify-write through one entry.
    const COUNT_BY: &str = "\
export function countBy<T, K extends PropertyKey>(
  arr: readonly T[],
  mapper: (item: T) => K
): Record<K, number> {
  const result = {} as Record<K, number>;
  for (let i = 0; i < arr.length; i++) {
    const key = mapper(arr[i]);
    result[key] = (result[key] ?? 0) + 1;
  }
  return result;
}
";

    #[test]
    fn dict_entry_update_fuses_the_count_by_accumulator() {
        let output = optimized_mir_text(COUNT_BY);

        assert!(
            output.contains("entry_update %2[copy %4] ?? 0.0 as %9 = move %9 + 1.0"),
            "the triple becomes one entry update: {output}"
        );
        // Neither the separate entry read nor the write-back survives.
        assert!(!output.contains("%9 = copy %2[copy %4]"), "{output}");
        assert!(!output.contains("%2[copy %4] = "), "{output}");
    }

    /// The block index and statement index of the entry-read/compute/write-back
    /// triple `DictEntryUpdate` looks for, plus the container it writes.
    fn find_entry_triple(function: &MirFunction) -> Option<(BlockId, usize, LocalId)> {
        function.blocks.iter().find_map(|block| {
            block.statements.iter().enumerate().find_map(|(index, _)| {
                let [
                    Statement::Assign {
                        value: Rvalue::OptionalCoalesce { .. },
                        ..
                    },
                    Statement::Assign { .. },
                    Statement::AssignPlace {
                        place: Place::Index { base, .. },
                        ..
                    },
                ] = block.statements.get(index..index.saturating_add(3))?
                else {
                    return None;
                };
                Some((block.id, index, *base))
            })
        })
    }

    /// Rewrites the triple's compute step so it also reads the container local,
    /// which is the borrow hazard the pass must decline on.
    fn make_update_read_the_container(mir: &mut Mir) {
        for function in &mut mir.functions {
            let Some((block, index, base)) = find_entry_triple(function) else {
                continue;
            };
            let Some(Statement::Assign {
                value: Rvalue::Binary { rhs, .. },
                ..
            }) = block_mut(function, block)
                .statements
                .get_mut(index.saturating_add(1))
            else {
                continue;
            };
            *rhs = Operand::Copy(Place::Local(base));
            return;
        }
        std::panic::resume_unwind(Box::new("a fusable entry triple was lowered".to_owned()))
    }

    /// Appends a second reader of the entry-read temporary to the triple's block.
    fn add_second_entry_read_reader(mir: &mut Mir) {
        for function in &mut mir.functions {
            let Some((block, index, _)) = find_entry_triple(function) else {
                continue;
            };
            let Some(Statement::Assign { dest, .. }) = block_mut(function, block)
                .statements
                .get(index)
                .cloned()
            else {
                continue;
            };
            let decl = function
                .locals
                .get(usize::try_from(dest.0).unwrap_or(usize::MAX))
                .unwrap_or_else(|| {
                    std::panic::resume_unwind(Box::new("entry read local is declared".to_owned()))
                })
                .clone();
            let extra = LocalId(u32::try_from(function.locals.len()).unwrap_or(u32::MAX));
            function.locals.push(decl);
            block_mut(function, block).statements.push(Statement::Assign {
                dest: extra,
                value: Rvalue::Use(Operand::Copy(Place::Local(dest))),
            });
            return;
        }
        std::panic::resume_unwind(Box::new("a fusable entry triple was lowered".to_owned()))
    }

    /// Lowers TypeScript, runs every default pass, and formats the result
    /// WITHOUT validating, after `mutate` has broken one correctness condition.
    fn optimized_mir_text_after(source: &str, mutate: impl FnOnce(&mut Mir)) -> String {
        let mut ctx = HirCtx::new();
        ok_or_panic(to_hir(source, FileId(0), &mut ctx), "HIR");
        let mut mir = ok_or_panic(lower_hir(&ctx.krate), "MIR");
        mutate(&mut mir);
        opt::optimize(&mut mir);
        format_compact(&mir)
    }

    #[test]
    fn dict_entry_update_keeps_an_update_that_reads_the_container() {
        // The stored value is evaluated while the backend holds a `RefMut`
        // guard into the container's store, so an update that reads the
        // container itself would panic with `already borrowed` at runtime.
        let output = optimized_mir_text_after(COUNT_BY, make_update_read_the_container);

        assert!(
            !output.contains("entry_update"),
            "an update reading the container must keep the two-probe form: {output}"
        );
    }

    #[test]
    fn dict_entry_update_keeps_a_triple_with_a_second_reader() {
        // A second reader observes the entry value the fused form folds into
        // the update, so the read must stay a statement of its own.
        let output = optimized_mir_text_after(COUNT_BY, add_second_entry_read_reader);

        assert!(
            !output.contains("entry_update"),
            "a second reader of the entry read blocks the fusion: {output}"
        );
    }

    #[test]
    fn dict_entry_update_keeps_a_write_to_another_key() {
        let output = optimized_mir_text(
            "\
export function countBy<T, K extends PropertyKey>(
  arr: readonly T[],
  mapper: (item: T) => K
): Record<K, number> {
  const result = {} as Record<K, number>;
  for (let i = 0; i < arr.length; i++) {
    const key = mapper(arr[i]);
    const other = mapper(arr[0]);
    result[other] = (result[key] ?? 0) + 1;
  }
  return result;
}
",
        );

        assert!(!output.contains("entry_update"), "{output}");
    }

    #[test]
    fn dict_entry_update_keeps_a_write_to_another_dict() {
        let output = optimized_mir_text(
            "\
export function countBy<T, K extends PropertyKey>(
  arr: readonly T[],
  mapper: (item: T) => K
): Record<K, number> {
  const result = {} as Record<K, number>;
  const mirror = {} as Record<K, number>;
  for (let i = 0; i < arr.length; i++) {
    const key = mapper(arr[i]);
    mirror[key] = (result[key] ?? 0) + 1;
  }
  return result;
}
",
        );

        assert!(!output.contains("entry_update"), "{output}");
    }

    #[test]
    fn dict_entry_update_keeps_an_update_that_reads_another_local() {
        // Conservative operand rule: anything but constants and the bound entry
        // value is refused, because a container clone SHARES its store and a
        // second local can name the same cell.
        let output = optimized_mir_text(
            "\
export function countBy<T, K extends PropertyKey>(
  arr: readonly T[],
  mapper: (item: T) => K,
  bump: number
): Record<K, number> {
  const result = {} as Record<K, number>;
  for (let i = 0; i < arr.length; i++) {
    const key = mapper(arr[i]);
    result[key] = (result[key] ?? 0) + bump;
  }
  return result;
}
",
        );

        assert!(!output.contains("entry_update"), "{output}");
    }

    #[test]
    fn dict_entry_update_keeps_a_non_constant_seed() {
        // `SmeltJsMap::entry_or_insert` calls the seed closure while it holds
        // `store.borrow_mut()`, so only a constant seed is accepted.
        let output = optimized_mir_text(
            "\
export function countBy<T, K extends PropertyKey>(
  arr: readonly T[],
  mapper: (item: T) => K,
  seed: number
): Record<K, number> {
  const result = {} as Record<K, number>;
  for (let i = 0; i < arr.length; i++) {
    const key = mapper(arr[i]);
    result[key] = (result[key] ?? seed) + 1;
  }
  return result;
}
",
        );

        assert!(!output.contains("entry_update"), "{output}");
    }

    #[test]
    fn dict_default_insert_elision_keeps_a_guard_on_another_dict() {

        let output = optimized_mir_text(
            "\
export function groupBy<T, K extends PropertyKey>(
  arr: readonly T[],
  getKeyFromItem: (item: T, index: number) => K
): Record<K, T[]> {
  const result = {} as Record<K, T[]>;
  const mirror = {} as Record<K, T[]>;
  for (let i = 0; i < arr.length; i++) {
    const item = arr[i];
    const key = getKeyFromItem(item, i);
    if (!Object.hasOwn(result, key)) {
      result[key] = [];
    }
    mirror[key].push(item);
  }
  return result;
}
",
        );

        assert!(output.contains("dict_contains_key"), "{output}");
    }
}
