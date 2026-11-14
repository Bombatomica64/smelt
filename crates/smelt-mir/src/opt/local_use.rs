//! Local lookups and temp inlining shared by the MIR optimization passes. The
//! read and definition counts themselves live on [`MirFunction`].

use crate::{Constant, LocalId, MirFunction, Operand, Rvalue, Statement};

/// Inline every single-assignment temp in `candidates` that only ever holds
/// an absent constant (`undefined`/`None`) into the statement operands that
/// read it, and drop its assignment.
///
/// Lowering spills an absent literal into a temp typed at its context: in a
/// generic body `v !== undefined` becomes `%t: T = undefined; v != %t`, while
/// the same comparison on a concrete `number` keeps the literal inline
/// (`n != undefined`). When the temp's type is later substituted with a
/// concrete type (an inherited copy, see codegen's
/// `classes::substituted_inherited_member`), a concrete `f64` cannot hold
/// `undefined`, and the spill would read as `0.0`. Inlining restores the
/// concrete spelling. A temp read anywhere other than a statement's rvalue
/// (a terminator, a projected place) is left alone.
pub fn inline_absent_constant_temps(function: &mut MirFunction, candidates: &[LocalId]) {
    for &local in candidates {
        if !function.is_temp(local) || function.assignment_count(local) != 1 {
            continue;
        }
        let mut found = None;
        for block in &function.blocks {
            for statement in &block.statements {
                if let Statement::Assign {
                    dest,
                    value:
                        Rvalue::Use(Operand::Const(value @ (Constant::Undefined | Constant::None))),
                } = statement
                    && *dest == local
                {
                    found = Some(value.clone());
                }
            }
        }
        let Some(constant) = found else {
            continue;
        };
        let mut statement_reads: usize = 0;
        for statement in function.blocks.iter().flat_map(|block| &block.statements) {
            if let Statement::Assign { value, .. } = statement {
                value.reads(|operand| {
                    statement_reads =
                        statement_reads.saturating_add(usize::from(operand.local() == Some(local)));
                });
            }
        }
        if statement_reads != function.read_count(local) {
            continue;
        }
        for block in &mut function.blocks {
            block.statements.retain(
                |statement| !matches!(statement, Statement::Assign { dest, .. } if *dest == local),
            );
            for statement in &mut block.statements {
                if let Statement::Assign { value, .. } = statement {
                    value.reads_mut(|operand| {
                        if operand.local() == Some(local) {
                            *operand = Operand::Const(constant.clone());
                        }
                    });
                }
            }
        }
    }
}
