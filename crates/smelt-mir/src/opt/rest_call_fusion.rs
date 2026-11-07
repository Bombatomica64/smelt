//! Rest call fusion.
//!
//! A frontend packs the arguments of a call through a function value, so a
//! callee that is all rest receives one list:
//!
//! ```text
//! %14 = [copy %2]
//! %15 = closure_call copy %13(move %14)
//! ```
//!
//! A hand-written port spells the list inside the call (`f.call(vec![x])`); it is
//! not a value of the program. This pass folds the list into the call, so packing
//! stays the frontend's decision and the temporary goes:
//!
//! ```text
//! %15 = closure_call copy %13(rest copy %2)
//! ```
//!
//! The same call fed a list of another element type (`%14 = copy %0`, a
//! `List<T>` into a `List<Unknown>` parameter) is a spread of `%0`:
//! [`Rvalue::ClosureCallSpread`] converts its list itself, so no second binding
//! is needed.
//!
//! # Requirements
//!
//! Behaviour: the callee receives the same elements in the same order, read at
//! the same point: the two statements are adjacent, so no write falls between
//! the reads of the list's items and the call.
//!
//! Type preservation: the fused form renders the list at the type of the
//! temporary it replaces, the callee's erased rest parameter. No typed value
//! moves to an erased type: a temporary this pass removes was already
//! `List<Unknown>`.
//!
//! # Correctness
//!
//! `t = rv; x = closure_call f(t)`, as adjacent statements of one block, is
//! fused only when every guard holds:
//!
//! * `t` is a compiler temporary assigned once and read once, by the call.
//! * `t` is the only argument, and a bare operand of the call.
//! * The callee is a function value whose whole signature is one rest parameter
//!   of type `List<Unknown>`, the type of `t`.
//! * The callee neither throws nor is asynchronous: those calls carry an error
//!   or a future the spread form does not render.
//! * `rv` is a list literal, or a copy of a local holding a list of a different
//!   type. A same-type copy belongs to
//!   [`TempCopyForwarding`](super::TempCopyForwarding).

use std::collections::HashSet;

use smelt_hir::{FunctionType, Type, TypeInterner};

use super::Pass;
use crate::{BasicBlock, Body, LocalDecl, LocalId, Mir, Operand, Place, Rvalue, Statement};

/// The rest call fusion pass; see the module docs.
#[derive(Debug, Default)]
pub struct RestCallFusion;

impl Pass for RestCallFusion {
    /// Applies the pass to every function and closure body of `mir`, returning
    /// true if it was modified.
    fn run(&self, mir: &mut Mir) -> bool {
        let mut changed = false;
        for function in &mut mir.functions {
            changed |= fuse_body(&function.locals, &mut function.blocks, &mir.types);
        }
        for closure in &mut mir.closures {
            changed |= fuse_body(&closure.locals, &mut closure.blocks, &mir.types);
        }
        changed
    }
}

/// Fuses every qualifying pair in one body; true if modified.
fn fuse_body(locals: &[LocalDecl], blocks: &mut [BasicBlock], types: &TypeInterner) -> bool {
    // Counted once: fusing removes a temporary and leaves every other count alone.
    let single_use = Body { locals, blocks }.single_use_temps();
    let mut changed = false;
    for block in blocks.iter_mut() {
        let mut index: usize = 0;
        while index.saturating_add(1) < block.statements.len() {
            let pair = block.statements.get(index..index.saturating_add(2));
            let fused = pair.and_then(|statements| match statements {
                [definition, call] => fused_call(definition, call, locals, &single_use, types),
                _ => None,
            });
            if let Some(statement) = fused {
                block.statements.splice(index..index.saturating_add(2), [statement]);
                changed = true;
            }
            index = index.saturating_add(1);
        }
    }
    changed
}

/// The call that replaces `definition; call`, when every guard admits the pair.
fn fused_call(
    definition: &Statement,
    call: &Statement,
    locals: &[LocalDecl],
    single_use: &HashSet<LocalId>,
    types: &TypeInterner,
) -> Option<Statement> {
    let Statement::Assign { dest: temp, value } = definition else {
        return None;
    };
    let Statement::Assign { dest, value: Rvalue::ClosureCall { callee, args } } = call else {
        return None;
    };
    let [argument] = args.as_slice() else {
        return None;
    };
    if !single_use.contains(temp) || argument.local() != Some(*temp) {
        return None;
    }
    let body = Body { locals, blocks: &[] };
    let temp_ty = body.local_decl(*temp)?.ty;
    let callee_ty = body.local_decl(callee.local()?)?.ty;
    let Some(Type::Function(function)) = types.get(callee_ty) else {
        return None;
    };
    if !takes_only_an_erased_rest(function, temp_ty, types) {
        return None;
    }
    let fused = match value {
        Rvalue::List(items) => {
            Rvalue::ClosureCallRest { callee: callee.clone(), items: items.clone() }
        }
        Rvalue::Use(list @ (Operand::Copy(Place::Local(source)) | Operand::Move(Place::Local(source)))) => {
            let source_ty = body.local_decl(*source)?.ty;
            if source_ty == temp_ty || !matches!(types.get(source_ty), Some(Type::List(_))) {
                return None;
            }
            Rvalue::ClosureCallSpread { callee: callee.clone(), args: list.clone() }
        }
        _ => return None,
    };
    Some(Statement::Assign { dest: *dest, value: fused })
}

/// Whether `function` is one synchronous, non-throwing rest parameter of type
/// `list_ty`, itself `List<Unknown>`.
fn takes_only_an_erased_rest(
    function: &FunctionType,
    list_ty: smelt_hir::TypeId,
    types: &TypeInterner,
) -> bool {
    let is_erased_list = matches!(
        types.get(list_ty),
        Some(Type::List(item)) if matches!(types.get(*item), Some(Type::Unknown))
    );
    is_erased_list
        && function.rest == Some(0)
        && function.params.as_slice() == [list_ty]
        && function.mutable_params.is_empty()
        && !function.may_throw
        && !function.is_async
        && !matches!(types.get(function.return_ty), Some(Type::Future(_)))
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    clippy::panic,
    reason = "fixed-shape hand-built MIR fixture: its one function and block are known to exist"
)]
mod tests {
    use smelt_hir::{BodyId, FileId, Span, Symbol};

    use super::*;
    use crate::{FuncId, HirOrigin, LocalKind, MirFunction, Terminator};

    /// The callee, a parameter.
    const CALLEE: LocalId = LocalId(0);
    /// A value parameter, the list's item.
    const ITEM: LocalId = LocalId(1);
    /// The temporary under test.
    const TEMP: LocalId = LocalId(2);
    /// The call's destination.
    const RESULT: LocalId = LocalId(3);
    /// A `List<Float>` parameter.
    const FLOATS: LocalId = LocalId(4);

    /// A span nothing reads.
    fn span() -> Span {
        Span::new(FileId(0), 0, 0)
    }

    /// `copy %n`.
    fn copy(local: LocalId) -> Operand {
        Operand::Copy(Place::Local(local))
    }

    /// `%3 = closure_call copy %0(move %2)`.
    fn call() -> Statement {
        Statement::Assign {
            dest: RESULT,
            value: Rvalue::ClosureCall {
                callee: copy(CALLEE),
                args: vec![Operand::Move(Place::Local(TEMP))],
            },
        }
    }

    /// `%2 = [copy %1]`.
    fn list() -> Statement {
        Statement::Assign { dest: TEMP, value: Rvalue::List(vec![copy(ITEM)]) }
    }

    /// Builds the callee's function type from the erased list type and the element type.
    type MakeFunction = fn(smelt_hir::TypeId, smelt_hir::TypeId) -> FunctionType;

    /// One function holding `statements`, whose callee is `make_function(list)`.
    fn mir(statements: Vec<Statement>, make_function: MakeFunction) -> Mir {
        let mut types = TypeInterner::default();
        let unknown = types.intern(Type::Unknown);
        let float = types.intern(Type::Float);
        let erased = types.intern(Type::List(unknown));
        let floats = types.intern(Type::List(float));
        let callee = types.intern(Type::Function(make_function(erased, unknown)));
        let mut function = MirFunction::new(FuncId(0), Symbol(0), HirOrigin::Body(BodyId(0)), unknown, span());
        for (ty, kind) in [
            (callee, LocalKind::Param { symbol: None }),
            (unknown, LocalKind::Param { symbol: None }),
            (erased, LocalKind::Temp),
            (unknown, LocalKind::UserBinding(Symbol(1))),
            (floats, LocalKind::Param { symbol: None }),
        ] {
            function.push_local(LocalDecl { ty, kind, span: span() });
        }
        function.blocks[0].statements = statements;
        function.blocks[0].terminator = Some(Terminator::Return(copy(RESULT)));
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

    /// A synchronous, non-throwing function type.
    fn function(params: Vec<smelt_hir::TypeId>, rest: Option<usize>, return_ty: smelt_hir::TypeId) -> FunctionType {
        FunctionType {
            params,
            rest,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty,
            is_async: false,
            may_throw: false,
        }
    }

    /// `(...args: unknown[]) => unknown`.
    fn all_rest(list: smelt_hir::TypeId, unknown: smelt_hir::TypeId) -> FunctionType {
        function(vec![list], Some(0), unknown)
    }

    /// Asserts the pass leaves `mir` as it found it.
    fn assert_declined(mut mir: Mir) {
        let before = format!("{:?}", mir.functions[0].blocks);
        assert!(!RestCallFusion.run(&mut mir));
        assert_eq!(format!("{:?}", mir.functions[0].blocks), before);
    }

    /// A packed list goes into the call and the temporary with it.
    #[test]
    fn folds_a_packed_list_into_the_call() {
        let mut mir = mir(vec![list(), call()], all_rest);
        assert!(RestCallFusion.run(&mut mir));
        let [Statement::Assign { dest, value: Rvalue::ClosureCallRest { items, .. } }] =
            mir.functions[0].blocks[0].statements.as_slice()
        else {
            panic!("{:?}", mir.functions[0].blocks[0].statements);
        };
        assert_eq!(*dest, RESULT);
        assert_eq!(items, &vec![copy(ITEM)]);
    }

    /// A list of another element type is a spread of that list.
    #[test]
    fn a_converting_copy_becomes_a_spread() {
        let define = Statement::Assign { dest: TEMP, value: Rvalue::Use(copy(FLOATS)) };
        let mut mir = mir(vec![define, call()], all_rest);
        assert!(RestCallFusion.run(&mut mir));
        let [Statement::Assign { value: Rvalue::ClosureCallSpread { args, .. }, .. }] =
            mir.functions[0].blocks[0].statements.as_slice()
        else {
            panic!("{:?}", mir.functions[0].blocks[0].statements);
        };
        assert_eq!(*args, copy(FLOATS));
    }

    /// A statement between the two could change what the list holds.
    #[test]
    fn declines_a_pair_that_is_not_adjacent() {
        let between = Statement::AssignPlace {
            place: Place::Local(ITEM),
            value: Rvalue::Use(copy(ITEM)),
        };
        assert_declined(mir(vec![list(), between, call()], all_rest));
    }

    /// A callee with a positional parameter takes the list as one of two.
    #[test]
    fn declines_a_callee_with_positional_parameters() {
        fn positional(list: smelt_hir::TypeId, unknown: smelt_hir::TypeId) -> FunctionType {
            function(vec![unknown, list], Some(1), unknown)
        }
        assert_declined(mir(vec![list(), call()], positional));
    }

    /// A throwing callee stays on the path that renders its error.
    #[test]
    fn declines_a_throwing_callee() {
        fn throwing(list: smelt_hir::TypeId, unknown: smelt_hir::TypeId) -> FunctionType {
            FunctionType { may_throw: true, ..all_rest(list, unknown) }
        }
        assert_declined(mir(vec![list(), call()], throwing));
    }

    /// A list the program reads again is a value of the program.
    #[test]
    fn declines_a_list_read_twice() {
        let again = Statement::Assign { dest: RESULT, value: Rvalue::Use(copy(TEMP)) };
        assert_declined(mir(vec![list(), call(), again], all_rest));
    }

    /// A same-type copy is not this pass's to take.
    #[test]
    fn declines_a_copy_of_the_same_type() {
        let define = Statement::Assign { dest: TEMP, value: Rvalue::Use(copy(ITEM)) };
        assert_declined(mir(vec![define, call()], all_rest));
    }
}
