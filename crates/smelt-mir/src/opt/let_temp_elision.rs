//! Let-temporary elision.
//!
//! HIR-to-MIR lowering evaluates every initializer into a fresh temporary and
//! then moves it into the declared local:
//!
//! ```text
//! %5 = list_flat copy %1, _
//! %2 = move %5          // %2 is `transforms`
//! ```
//!
//! A hand-written port has no such temporary (`let transforms =
//! _transforms.flat();`). [`CopyPropagation`](super::CopyPropagation) never
//! removes it on purpose: aliasing the user local to the temporary would
//! replace the source name with a synthetic one. This pass removes it in the
//! other direction: the defining statement writes straight into the user local
//! and the move disappears, so the source name is kept.
//!
//! # Correctness
//!
//! A pair `t = rv; u = move|copy t` in one block, as adjacent statements, is
//! rewritten to `u = rv` only when every guard holds:
//!
//! * `t` is a compiler temporary defined exactly once and read exactly once
//!   (by the move), so no other statement observes it.
//! * `u` is defined exactly once (the move), so `u` is a single-assignment
//!   binding and writing it one statement earlier changes no other read: the
//!   only statement in between is the move itself.
//! * `rv` does not read `u`. A self-referencing initializer (a closure that
//!   captures its own binding) must keep reading the binding's prior state.
//! * `t` and `u` have the same type, so no conversion was implied by the move.
//! * Neither is function-typed: callable locals carry ABI decisions
//!   (`Rc<dyn Fn>` vs. borrowed callbacks) that copy propagation also leaves
//!   alone.
//! * `t` appears in no `StorageLive`/`StorageDead` marker.
//!
//! Behaviour requirement: unchanged (the same value reaches the same local).
//! Type-preservation requirement: unchanged types; the generated Rust has one
//! fewer `let` per such binding.

use smelt_hir::Type;

use super::local_use::{local_assignment_count, local_decl, local_is_temp, local_read_count, operand_local};
use super::Pass;
use crate::{Mir, MirFunction, Operand, Rvalue, Statement};

/// The let-temporary elision pass; see the module docs.
#[derive(Debug, Default)]
pub struct LetTempElision;

impl Pass for LetTempElision {
    /// Returns the stable identifier of this pass.
    fn name(&self) -> &'static str {
        "let-temp-elision"
    }

    /// Applies the pass to `mir`, returning true if it was modified.
    fn run(&self, mir: &mut Mir) -> bool {
        let mut changed = false;
        for function in &mut mir.functions {
            changed |= elide_function(function, &mir.types);
        }
        changed
    }
}

/// Elides every qualifying temporary in one function; true if modified.
fn elide_function(function: &mut MirFunction, types: &smelt_hir::TypeInterner) -> bool {
    let mut changed = false;
    for block_index in 0..function.blocks.len() {
        let mut index: usize = 0;
        while let Some(user) = next_pair(function, types, block_index, &mut index) {
            let Some(block) = function.blocks.get_mut(block_index) else {
                break;
            };
            if let Some(Statement::Assign { dest, .. }) = block.statements.get_mut(index) {
                *dest = user;
            }
            let after = index.saturating_add(1);
            if after < block.statements.len() {
                block.statements.remove(after);
            }
            changed = true;
            index = after;
        }
    }
    changed
}

/// Advances `index` to the next elidable pair in the block at or after it,
/// returning the pair's user local; `None` when the block has no more pairs.
fn next_pair(
    function: &MirFunction,
    types: &smelt_hir::TypeInterner,
    block_index: usize,
    index: &mut usize,
) -> Option<crate::LocalId> {
    let len = function.blocks.get(block_index)?.statements.len();
    while index.saturating_add(1) < len {
        if let Some(user) = elidable_pair(function, types, block_index, *index) {
            return Some(user);
        }
        *index = index.saturating_add(1);
    }
    None
}

/// The user local `u` when statements `index` and `index + 1` of the block
/// are an elidable `t = rv; u = move t` pair.
fn elidable_pair(
    function: &MirFunction,
    types: &smelt_hir::TypeInterner,
    block_index: usize,
    index: usize,
) -> Option<crate::LocalId> {
    let statements = &function.blocks.get(block_index)?.statements;
    let Statement::Assign { dest: temp_dest, value } = statements.get(index)? else {
        return None;
    };
    let Statement::Assign { dest: user_dest, value: Rvalue::Use(moved @ (Operand::Copy(_) | Operand::Move(_))) } =
        statements.get(index.saturating_add(1))?
    else {
        return None;
    };
    let (temp, user) = (*temp_dest, *user_dest);
    let moves_temp = operand_local(moved) == Some(temp);
    if !moves_temp || temp == user {
        return None;
    }
    let temp_decl = local_decl(function, temp)?;
    let user_decl = local_decl(function, user)?;
    let reads_user = {
        let mut reads = false;
        value.for_each_operand(|operand| reads |= super::local_use::operand_reads_local(operand, user));
        reads
    };
    let has_storage_marker = function.blocks.iter().flat_map(|block| &block.statements).any(|statement| {
        matches!(statement, Statement::StorageLive(local) | Statement::StorageDead(local) if *local == temp)
    });
    let is_function = |ty| matches!(types.get(ty), Some(Type::Function(_)));
    (local_is_temp(function, temp)
        && local_assignment_count(function, temp) == 1
        && local_read_count(function, temp) == 1
        && local_assignment_count(function, user) == 1
        && !reads_user
        && temp_decl.ty == user_decl.ty
        && !is_function(temp_decl.ty)
        && !has_storage_marker)
        .then_some(user)
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    clippy::panic,
    clippy::as_conversions,
    reason = "fixed-shape hand-built MIR fixture: every index and local id is known to exist"
)]
mod tests {
    //! Unit tests for [`LetTempElision`]: the happy path rewrites, and each
    //! guard from the module docs declines on its own.
    //!
    //! Each test builds one function by hand:
    //!
    //! ```text
    //! %0: param a
    //! %1: temp t
    //! %2: user u
    //! bb0: %1 = copy %0 + copy %0; %2 = move %1; return copy %2
    //! ```
    //!
    //! and breaks exactly one guard.

    use smelt_hir::{BinOp, BodyId, FileId, FunctionType, Span, Symbol, Type, TypeId, TypeInterner};

    use super::LetTempElision;
    use crate::opt::Pass;
    use crate::{
        FuncId, HirOrigin, LocalDecl, LocalId, LocalKind, Mir, MirFunction, Operand, Place, Rvalue,
        Statement, Terminator,
    };

    const PARAM: LocalId = LocalId(0);
    const TEMP: LocalId = LocalId(1);
    const USER: LocalId = LocalId(2);

    /// A zero-width span; spans play no part in the pass.
    fn span() -> Span {
        Span::new(FileId(0), 0, 0)
    }

    /// `copy %n`.
    fn copy(local: LocalId) -> Operand {
        Operand::Copy(Place::Local(local))
    }

    /// The fixture: one function whose locals have types `tys` (param, temp,
    /// user) and whose entry block runs `statements` then returns `copy %2`.
    fn mir(types: TypeInterner, tys: [TypeId; 3], statements: Vec<Statement>) -> Mir {
        let mut function = MirFunction::new(FuncId(0), Symbol(0), HirOrigin::Body(BodyId(0)), tys[2], span());
        let kinds = [LocalKind::Param { symbol: None }, LocalKind::Temp, LocalKind::UserBinding(Symbol(1))];
        for (ty, kind) in tys.into_iter().zip(kinds) {
            function.push_local(LocalDecl { ty, kind, span: span() });
        }
        function.params = vec![PARAM];
        let entry = &mut function.blocks[0];
        entry.statements = statements;
        entry.terminator = Some(Terminator::Return(copy(USER)));
        Mir {
            functions: vec![function],
            classes: Vec::new(),
            interfaces: Vec::new(),
            closures: Vec::new(),
            globals: Vec::new(),
            file_paths: std::collections::HashMap::new(),
            types,
            symbols: smelt_hir::SymbolInterner::default(),
            names: smelt_hir::OriginalNameTable::default(),
        }
    }

    /// `%1 = copy %0 + copy %0`.
    fn define_temp() -> Statement {
        Statement::Assign {
            dest: TEMP,
            value: Rvalue::Binary {
                op: BinOp::Add,
                lhs: copy(PARAM),
                rhs: copy(PARAM),
            },
        }
    }

    /// `%2 = move %1`.
    fn move_into_user() -> Statement {
        Statement::Assign {
            dest: USER,
            value: Rvalue::Use(Operand::Move(Place::Local(TEMP))),
        }
    }

    /// Interner plus a `Float` type.
    fn floats() -> (TypeInterner, TypeId) {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        (types, float)
    }

    /// The fixture with every guard satisfied, before `edit` breaks one.
    fn run(edit: impl FnOnce(&mut Mir)) -> (bool, Mir) {
        let (types, float) = floats();
        let mut mir = mir(types, [float; 3], vec![define_temp(), move_into_user()]);
        edit(&mut mir);
        let changed = LetTempElision.run(&mut mir);
        (changed, mir)
    }

    /// Asserts the pass left the function's statements as they were.
    fn assert_declined(edit: impl FnOnce(&mut Mir)) {
        let (types, float) = floats();
        let mut expected = mir(types, [float; 3], vec![define_temp(), move_into_user()]);
        let (changed, mir) = run(|mir| {
            edit(mir);
            expected = mir.clone();
        });
        assert!(!changed, "{:?}", mir.functions[0].blocks[0].statements);
        assert_eq!(
            format!("{:?}", mir.functions[0].blocks),
            format!("{:?}", expected.functions[0].blocks)
        );
    }

    /// Every guard holds: the temp's definition writes the user local and the
    /// move disappears. Same value (`a + a`), same local, same type.
    #[test]
    fn happy_path_writes_user_local_directly() {
        let (changed, mir) = run(|_| {});
        assert!(changed);
        let function = &mir.functions[0];
        let statements = &function.blocks[0].statements;
        assert_eq!(statements.len(), 1, "{statements:?}");
        let Statement::Assign { dest, value } = &statements[0] else {
            panic!("{statements:?}");
        };
        assert_eq!(*dest, USER);
        assert!(matches!(value, Rvalue::Binary { op: BinOp::Add, .. }), "{value:?}");
        // The user binding keeps its declared type.
        assert_eq!(function.locals[USER.0 as usize].ty, function.locals[TEMP.0 as usize].ty);
        // A fixpoint run finds nothing more to do.
        let mut again = mir;
        assert!(!LetTempElision.run(&mut again));
    }

    /// `t` must be a compiler temporary, not a named binding.
    #[test]
    fn declines_when_source_is_not_temp() {
        assert_declined(|mir| mir.functions[0].locals[1].kind = LocalKind::UserBinding(Symbol(2)));
    }

    /// `t` defined twice: the second definition would lose its destination.
    #[test]
    fn declines_when_temp_defined_twice() {
        assert_declined(|mir| mir.functions[0].blocks[0].statements.insert(0, define_temp()));
    }

    /// `t` read again after the move: that read would see no value.
    #[test]
    fn declines_when_temp_read_twice() {
        assert_declined(|mir| {
            mir.functions[0].blocks[0].terminator = Some(Terminator::Return(copy(TEMP)));
        });
    }

    /// `u` assigned elsewhere: writing it a statement early is not single-assignment.
    #[test]
    fn declines_when_user_defined_twice() {
        assert_declined(|mir| {
            mir.functions[0].blocks[0].statements.insert(
                0,
                Statement::Assign {
                    dest: USER,
                    value: Rvalue::Use(copy(PARAM)),
                },
            );
        });
    }

    /// `rv` reads `u`: it must keep reading the binding's prior state.
    #[test]
    fn declines_when_initializer_reads_user() {
        assert_declined(|mir| {
            mir.functions[0].blocks[0].statements[0] = Statement::Assign {
                dest: TEMP,
                value: Rvalue::Binary {
                    op: BinOp::Add,
                    lhs: copy(PARAM),
                    rhs: copy(USER),
                },
            };
        });
    }

    /// Different types: the move implied a conversion.
    #[test]
    fn declines_when_types_differ() {
        assert_declined(|mir| {
            let int = mir.types.intern(Type::Int);
            mir.functions[0].locals[1].ty = int;
        });
    }

    /// Function-typed locals keep their callable ABI decisions.
    #[test]
    fn declines_when_function_typed() {
        assert_declined(|mir| {
            let float = mir.functions[0].locals[0].ty;
            let callable = mir.types.intern(Type::Function(FunctionType {
                params: Vec::new(),
                rest: None,
                required_params: None,
                mutable_params: Vec::new(),
                return_ty: float,
                is_async: false,
                may_throw: false,
            }));
            mir.functions[0].locals[1].ty = callable;
            mir.functions[0].locals[2].ty = callable;
        });
    }

    /// A storage marker on `t` scopes its lifetime; leave it alone.
    #[test]
    fn declines_when_temp_has_storage_marker() {
        assert_declined(|mir| {
            mir.functions[0].blocks[0].statements.push(Statement::StorageDead(TEMP));
        });
    }

    /// The pair must be adjacent: a statement between them may observe `u`.
    #[test]
    fn declines_when_not_adjacent() {
        assert_declined(|mir| {
            mir.functions[0].blocks[0].statements.insert(
                1,
                Statement::AssignPlace {
                    place: Place::Local(PARAM),
                    value: Rvalue::Use(Operand::Const(crate::Constant::Float(1.0))),
                },
            );
        });
    }

    /// The move must read `t`, not some other local.
    #[test]
    fn declines_when_move_reads_another_local() {
        assert_declined(|mir| {
            mir.functions[0].blocks[0].statements[1] = Statement::Assign {
                dest: USER,
                value: Rvalue::Use(copy(PARAM)),
            };
            mir.functions[0].blocks[0].terminator = Some(Terminator::Return(copy(TEMP)));
        });
    }
}
