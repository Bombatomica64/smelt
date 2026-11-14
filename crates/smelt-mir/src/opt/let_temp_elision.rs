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
//! * `u` has exactly one initializer (the move), so writing it one statement
//!   earlier changes no other read: the only statement in between is the move
//!   itself. A later source reassignment is a whole-local `AssignPlace`, which
//!   leaves the `let mut u = rv` binding intact and does not count.
//! * `rv` does not read `u`. A self-referencing initializer (a closure that
//!   captures its own binding) must keep reading the binding's prior state.
//! * `t` and `u` have the same type, so no conversion was implied by the move.
//! * Neither is function-typed: callable locals carry ABI decisions
//!   (`Rc<dyn Fn>` vs. borrowed callbacks) that copy propagation also leaves
//!   alone.
//! * `u` is not a closure's capture: writing a capture is a store into the
//!   shared cell, not a binding.
//!
//! The pass covers function and closure bodies alike.
//!
//! Behaviour requirement: unchanged (the same value reaches the same local).
//! Type-preservation requirement: unchanged types; the generated Rust has one
//! fewer `let` per such binding.

use smelt_hir::Type;

use std::collections::{HashMap, HashSet};

use super::Pass;
use crate::{BasicBlock, Body, LocalDecl, LocalId, Mir, Operand, Place, Rvalue, Statement};

/// The let-temporary elision pass; see the module docs.
#[derive(Debug, Default)]
pub struct LetTempElision;

impl Pass for LetTempElision {
    /// Applies the pass to `mir`, returning true if it was modified.
    fn run(&self, mir: &mut Mir) -> bool {
        let mut changed = false;
        for function in &mut mir.functions {
            changed |= elide_body(&function.locals, &mut function.blocks, &HashSet::new(), &mir.types);
        }
        for closure in &mut mir.closures {
            let captures = closure.captures.iter().filter_map(|capture| capture.target_local).collect();
            changed |= elide_body(&closure.locals, &mut closure.blocks, &captures, &mir.types);
        }
        changed
    }
}

/// Elides every qualifying temporary in one body; true if modified. `captures`
/// are the locals that never take a pair's user role.
fn elide_body(
    locals: &[LocalDecl],
    blocks: &mut [BasicBlock],
    captures: &HashSet<LocalId>,
    types: &smelt_hir::TypeInterner,
) -> bool {
    // Counted once, as in `TempCopyForwarding`: eliding a pair touches only that
    // pair's locals, and a recount per statement is quadratic in the body.
    let facts = BodyFacts::of(Body { locals, blocks }, captures, types);
    let mut changed = false;
    for block_index in 0..blocks.len() {
        let mut index: usize = 0;
        loop {
            let body = Body { locals, blocks };
            let Some((at, user)) = next_pair(body, &facts, block_index, index) else {
                break;
            };
            let Some(block) = blocks.get_mut(block_index) else {
                break;
            };
            if let Some(Statement::Assign { dest, .. }) = block.statements.get_mut(at) {
                *dest = user;
            }
            let after = at.saturating_add(1);
            if after < block.statements.len() {
                block.statements.remove(after);
            }
            changed = true;
            index = after;
        }
    }
    changed
}

/// The next elidable pair of the block at or after `index`: its statement
/// index and user local. `None` when the block has no more pairs.
fn next_pair(
    body: Body<'_>,
    facts: &BodyFacts<'_>,
    block_index: usize,
    mut index: usize,
) -> Option<(usize, LocalId)> {
    let len = body.blocks.get(block_index)?.statements.len();
    while index.saturating_add(1) < len {
        if let Some(user) = elidable_pair(body, facts, block_index, index) {
            return Some((index, user));
        }
        index = index.saturating_add(1);
    }
    None
}

/// The user local `u` when statements `index` and `index + 1` of the block
/// are an elidable `t = rv; u = move t` pair.
fn elidable_pair(
    body: Body<'_>,
    facts: &BodyFacts<'_>,
    block_index: usize,
    index: usize,
) -> Option<LocalId> {
    let statements = &body.blocks.get(block_index)?.statements;
    let Statement::Assign { dest: temp_dest, value } = statements.get(index)? else {
        return None;
    };
    let Statement::Assign { dest: user_dest, value: Rvalue::Use(moved @ (Operand::Copy(_) | Operand::Move(_))) } =
        statements.get(index.saturating_add(1))?
    else {
        return None;
    };
    let (temp, user) = (*temp_dest, *user_dest);
    let moves_temp = moved.local() == Some(temp);
    if !moves_temp || temp == user || facts.captures.contains(&user) {
        return None;
    }
    let temp_decl = body.local_decl(temp)?;
    let user_decl = body.local_decl(user)?;
    let reads_user = {
        let mut reads = false;
        value.reads(|operand| reads |= operand.reads_local(user));
        reads
    };
    let is_function = |ty| matches!(facts.types.get(ty), Some(Type::Function(_)));
    (facts.single_use.contains(&temp)
        && facts.initializers.get(&user) == Some(&1)
        && !reads_user
        && temp_decl.ty == user_decl.ty
        && !is_function(temp_decl.ty))
        .then_some(user)
}

/// What the guards ask of a whole body, computed in one walk.
struct BodyFacts<'a> {
    /// Locals that never take a pair's user role.
    captures: &'a HashSet<LocalId>,
    /// The crate's types.
    types: &'a smelt_hir::TypeInterner,
    /// Temporaries assigned once and read once.
    single_use: HashSet<LocalId>,
    /// Per local, how many definitions are not source reassignments: every
    /// definition minus whole-local `AssignPlace`s.
    initializers: HashMap<LocalId, usize>,
}

impl<'a> BodyFacts<'a> {
    /// The facts of `body`.
    fn of(body: Body<'_>, captures: &'a HashSet<LocalId>, types: &'a smelt_hir::TypeInterner) -> Self {
        let mut initializers = body.assignment_counts();
        for statement in body.blocks.iter().flat_map(|block| &block.statements) {
            if let Statement::AssignPlace { place: Place::Local(target), .. } = statement
                && let Some(count) = initializers.get_mut(target)
            {
                *count = count.saturating_sub(1);
            }
        }
        Self { captures, types, single_use: body.single_use_temps(), initializers }
    }
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
            function.push_local(LocalDecl { ty, kind, span: span(), init: crate::LocalInit::Definite });
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
            accessor_fields: Vec::default(),
            index: crate::index::MirIndex::default(),
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

    /// The fixture's body moved into a closure, with `%2` a capture when
    /// `user_is_capture` holds.
    fn closure_fixture(user_is_capture: bool) -> Mir {
        let (types, float) = floats();
        let mut mir = mir(types, [float; 3], vec![define_temp(), move_into_user()]);
        let function = mir.functions.remove(0);
        let captures = user_is_capture.then_some(crate::MirClosureCapture {
            source_local: PARAM,
            target_local: Some(USER),
            symbol: Symbol(1),
            ty: float,
            mode: smelt_hir::CaptureMode::ByRef,
        });
        mir.closures.push(crate::MirClosure {
            id: crate::ClosureId(0),
            params: vec![PARAM],
            rest: None,
            required_params: None,
            locals: function.locals,
            captures: captures.into_iter().collect(),
            return_ty: float,
            blocks: function.blocks,
            entry: function.entry,
            escapes: false,
            can_throw: false,
            is_generator: false,
            function_item_key: None,
            language: smelt_hir::Language::TypeScript,
        });
        mir
    }

    /// A closure body is a body: its let temporaries go too.
    #[test]
    fn elides_in_a_closure_body() {
        let mut mir = closure_fixture(false);
        assert!(LetTempElision.run(&mut mir));
        assert_eq!(mir.closures[0].blocks[0].statements.len(), 1);
    }

    /// `x = y + 1` in a closure stores into the cell of capture `x`.
    #[test]
    fn declines_when_user_is_a_capture() {
        assert!(!LetTempElision.run(&mut closure_fixture(true)));
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
