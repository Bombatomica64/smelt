//! Temp copy forwarding.
//!
//! Lowering spills an argument into a temporary before the call that takes it:
//!
//! ```text
//! %6 = copy %0
//! %7 = closure_call copy %4(copy %5, copy %6)
//! ```
//!
//! A hand-written port names `%0` at the call. The temporary is a second binding
//! of one value, so this pass replaces its one read with the value it copied and
//! drops the definition. [`CopyPropagation`](super::CopyPropagation) cannot do it:
//! it works body-wide, so it refuses any source that is written anywhere and any
//! source that is itself a temporary, and it never sees a closure body.
//!
//! # Requirements
//!
//! Behaviour: the read sees the value the temporary held. The value of a local
//! changes only through a write, so that holds when `%s` is not written between
//! the copy and the read.
//!
//! Type preservation: the temporary and `%s` have the same type, so every use
//! sees the type it saw before; no value moves to a different Rust type and no
//! conversion is dropped. A copy between different types is a coercion and stays.
//!
//! # Correctness
//!
//! `t = copy|move s` is forwarded into a later statement or terminator of the
//! same block only when every guard holds:
//!
//! * `t` is a compiler temporary defined once and read once, and that read is a
//!   bare operand of the consumer. Being one block, the definition runs before
//!   the read on every path through it.
//! * `t` and `s` have the same type and are not function-typed: callable locals
//!   carry ABI decisions, as in [`LetTempElision`](super::LetTempElision).
//! * No statement from the copy up to and including a consuming statement writes
//!   `s`: not by definition, not through a field or index, and not as the
//!   receiver of an in-place collection operation. Including the consumer keeps
//!   `xs.push(xs)` from reading its receiver through a live mutable borrow.
//! * When `s` is [shared](super::shared_locals), no statement in between can run
//!   source code: it would be free to run a closure that writes `s`.
//! * The consumer does not mutate `t` and is not a closure construction, which
//!   snapshots `t` where forwarding would capture the cell of `s`.
//!
//! A terminator consumer writes only its destination, after reading its operands,
//! so `s = f(t)` needs no guard of its own.

use std::collections::HashSet;

use smelt_hir::{Type, TypeInterner};

use super::shared_locals::SharedLocals;
use super::Pass;
use crate::{
    BasicBlock, Body, LocalDecl, LocalId, Mir, Operand, Place, Read, ReadMut, Rvalue, Statement,
    Terminator,
};

/// The temp copy forwarding pass; see the module docs.
#[derive(Debug, Default)]
pub struct TempCopyForwarding;

impl Pass for TempCopyForwarding {
    /// Applies the pass to every function and closure body of `mir`, returning
    /// true if it was modified.
    fn run(&self, mir: &mut Mir) -> bool {
        let facts = SharedLocals::of(mir);
        let empty = HashSet::new();
        let mut changed = false;
        for (index, function) in mir.functions.iter_mut().enumerate() {
            let shared = facts.of_function(index).unwrap_or(&empty);
            changed |= forward_body(&function.locals, &mut function.blocks, shared, &mir.types);
        }
        for (index, closure) in mir.closures.iter_mut().enumerate() {
            let shared = facts.of_closure(index).unwrap_or(&empty);
            changed |= forward_body(&closure.locals, &mut closure.blocks, shared, &mir.types);
        }
        changed
    }
}

/// Where a temporary's one read sits in its block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Consumer {
    /// The statement at this index.
    Statement(usize),
    /// The block's terminator.
    Terminator,
}

/// Forwards every qualifying temporary in one body; true if modified.
fn forward_body(
    locals: &[LocalDecl],
    blocks: &mut [BasicBlock],
    shared: &HashSet<LocalId>,
    types: &TypeInterner,
) -> bool {
    // Counted once: a forwarding only removes a temporary that is dropped from
    // consideration with it, so the counts of the rest stay right, and recounting
    // per statement would make the pass quadratic in the size of the body.
    let single_use = Body { locals, blocks }.single_use_temps();
    let facts = BodyFacts { shared, single_use: &single_use, types };
    let mut changed = false;
    for block_index in 0..blocks.len() {
        let mut index: usize = 0;
        while let Some(found) = next_forward(Body { locals, blocks }, &facts, block_index, index) {
            let Some(block) = blocks.get_mut(block_index) else {
                break;
            };
            apply(block, &found);
            changed = true;
            index = found.at;
        }
    }
    changed
}

/// What the guards need beyond the body itself.
struct BodyFacts<'a> {
    /// Locals a closure can change; see [`SharedLocals`].
    shared: &'a HashSet<LocalId>,
    /// Temporaries assigned once and read once.
    single_use: &'a HashSet<LocalId>,
    /// The crate's types.
    types: &'a TypeInterner,
}

/// One proven forwarding.
#[derive(Debug)]
struct Forward {
    /// Index of the `t = copy s` statement.
    at: usize,
    /// The temporary `t`.
    temp: LocalId,
    /// The operand `t` was defined from.
    source: Operand,
    /// Where `t` is read.
    consumer: Consumer,
}

/// The first forwarding in the block at or after statement `start`.
fn next_forward(
    body: Body<'_>,
    facts: &BodyFacts<'_>,
    block_index: usize,
    start: usize,
) -> Option<Forward> {
    let block = body.blocks.get(block_index)?;
    (start..block.statements.len()).find_map(|at| forwardable(body, block, facts, at))
}

/// The forwarding of the statement at `at`, when it is a copy every guard admits.
fn forwardable(
    body: Body<'_>,
    block: &BasicBlock,
    facts: &BodyFacts<'_>,
    at: usize,
) -> Option<Forward> {
    let BodyFacts { shared, single_use, types } = *facts;
    let Statement::Assign { dest, value: Rvalue::Use(source) } = block.statements.get(at)? else {
        return None;
    };
    let (temp, from) = (*dest, source.local()?);
    let temp_decl = body.local_decl(temp)?;
    let same_type = temp_decl.ty == body.local_decl(from)?.ty;
    let callable = matches!(types.get(temp_decl.ty), Some(Type::Function(_)));
    if temp == from || !same_type || callable || !single_use.contains(&temp) {
        return None;
    }
    let after = at.saturating_add(1);
    let consumer = find_consumer(block, after, temp)?;
    let end = match consumer {
        Consumer::Statement(index) => index,
        Consumer::Terminator => block.statements.len(),
    };
    let between = block.statements.get(after..end)?;
    let is_shared = shared.contains(&from);
    let quiet = |statement: &Statement| {
        !writes(statement, from) && (!is_shared || !may_run_code(statement, body, types))
    };
    if !between.iter().all(quiet) {
        return None;
    }
    if let Consumer::Statement(index) = consumer {
        let statement = block.statements.get(index)?;
        if !accepts_forwarded_read(statement, temp) || writes(statement, from) {
            return None;
        }
    } else if !terminator_reads_bare(block.terminator.as_ref()?, temp) {
        return None;
    }
    Some(Forward { at, temp, source: source.clone(), consumer })
}

/// Where in `block`, at or after `from`, `temp` is read.
fn find_consumer(block: &BasicBlock, from: usize, temp: LocalId) -> Option<Consumer> {
    let statements = block.statements.get(from..)?;
    if let Some(offset) = statements.iter().position(|statement| statement.reads_local(temp)) {
        return Some(Consumer::Statement(from.saturating_add(offset)));
    }
    block
        .terminator
        .as_ref()
        .is_some_and(|terminator| terminator.reads_local(temp))
        .then_some(Consumer::Terminator)
}

/// Whether `statement` changes `local`, wholly or in part.
fn writes(statement: &Statement, local: LocalId) -> bool {
    if statement.defs() == Some(local) {
        return true;
    }
    let mutates = |value: &Rvalue| {
        value.mutated_receiver().is_some_and(|receiver| receiver.reads_local(local))
    };
    match statement {
        Statement::Assign { value, .. } => mutates(value),
        Statement::AssignPlace { place: Place::Field { base, .. } | Place::Index { base, .. }, value } => {
            *base == local || mutates(value)
        }
        Statement::AssignPlace { value, .. } => mutates(value),
        Statement::DictEntryUpdate { base, value, .. } => *base == local || mutates(value),
    }
}

/// Whether `statement` can run source code: it calls, constructs or suspends, or
/// it is handed a function it can call.
fn may_run_code(statement: &Statement, body: Body<'_>, types: &TypeInterner) -> bool {
    let value = match statement {
        Statement::Assign { value, .. }
        | Statement::AssignPlace { value, .. }
        | Statement::DictEntryUpdate { value, .. } => value,
    };
    if matches!(value, Rvalue::Closure { .. }) {
        return false;
    }
    if matches!(
        value,
        Rvalue::ClosureCall { .. }
            | Rvalue::ClosureCallSpread { .. }
            | Rvalue::Construct { .. }
            | Rvalue::Await { .. }
            | Rvalue::GeneratorYield { .. }
    ) {
        return true;
    }
    let mut takes_function = false;
    value.reads(|operand| {
        takes_function |= operand
            .local()
            .and_then(|local| body.local_decl(local))
            .is_some_and(|decl| matches!(types.get(decl.ty), Some(Type::Function(_))));
    });
    takes_function
}

/// Whether `statement` reads `temp` as a bare operand it does not mutate, in a
/// shape that reads the value now rather than holding on to `temp`.
fn accepts_forwarded_read(statement: &Statement, temp: LocalId) -> bool {
    let value = match statement {
        Statement::Assign { value, .. }
        | Statement::AssignPlace { value, .. }
        | Statement::DictEntryUpdate { value, .. } => value,
    };
    if matches!(value, Rvalue::Closure { .. }) {
        return false;
    }
    if value.mutated_receiver().is_some_and(|receiver| receiver.reads_local(temp)) {
        return false;
    }
    let mut bare = false;
    statement.reads(|read| {
        if let Read::Operand(operand) = read {
            bare |= operand.local() == Some(temp);
        }
    });
    bare
}

/// Whether `terminator` reads `temp` as a bare operand.
fn terminator_reads_bare(terminator: &Terminator, temp: LocalId) -> bool {
    let mut bare = false;
    terminator.reads(|operand| bare |= operand.local() == Some(temp));
    bare
}

/// Rewrites the consumer to read the source and removes the temporary's definition.
fn apply(block: &mut BasicBlock, forward: &Forward) {
    let replace = |operand: &mut Operand| {
        if operand.local() == Some(forward.temp) {
            *operand = forward.source.clone();
        }
    };
    match forward.consumer {
        Consumer::Statement(index) => {
            if let Some(statement) = block.statements.get_mut(index) {
                statement.reads_mut(|read| {
                    if let ReadMut::Operand(operand) = read {
                        replace(operand);
                    }
                });
            }
        }
        Consumer::Terminator => {
            if let Some(terminator) = &mut block.terminator {
                terminator.reads_mut(replace);
            }
        }
    }
    if forward.at < block.statements.len() {
        block.statements.remove(forward.at);
    }
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    clippy::as_conversions,
    reason = "fixed-shape hand-built MIR fixture: every index and local id is known to exist"
)]
mod tests {
    //! Unit tests for [`TempCopyForwarding`]: the rewrite, then each guard from
    //! the module docs declining on its own.
    //!
    //! ```text
    //! %0: param s
    //! %1: temp t
    //! %2: user u
    //! %3: param other
    //! bb0: %1 = copy %0; %2 = neg copy %1; return copy %2
    //! ```

    use smelt_hir::{BodyId, FileId, FunctionType, Span, Symbol, Type, TypeId, TypeInterner, UnaryOp};

    use super::TempCopyForwarding;
    use crate::opt::Pass;
    use crate::{
        ClosureId, FuncId, HirOrigin, LocalDecl, LocalId, LocalKind, Mir, MirClosureCapture,
        MirFunction, Operand, Place, Rvalue, Statement, Terminator,
    };

    const SOURCE: LocalId = LocalId(0);
    const TEMP: LocalId = LocalId(1);
    const USER: LocalId = LocalId(2);
    const OTHER: LocalId = LocalId(3);

    /// A zero-width span; spans play no part in the pass.
    fn span() -> Span {
        Span::new(FileId(0), 0, 0)
    }

    /// `copy %n`.
    fn copy(local: LocalId) -> Operand {
        Operand::Copy(Place::Local(local))
    }

    /// `%1 = copy %0`.
    fn define_temp() -> Statement {
        Statement::Assign { dest: TEMP, value: Rvalue::Use(copy(SOURCE)) }
    }

    /// `%2 = neg copy %1`.
    fn consume_temp() -> Statement {
        Statement::Assign {
            dest: USER,
            value: Rvalue::Unary { op: UnaryOp::Neg, operand: copy(TEMP) },
        }
    }

    /// `%0 = 2.0` as a source reassignment.
    fn write_source() -> Statement {
        Statement::AssignPlace {
            place: Place::Local(SOURCE),
            value: Rvalue::Use(Operand::Const(crate::Constant::Float(2.0))),
        }
    }

    /// The fixture: `statements` in the entry block of one function over a
    /// `Float` source, temporary, user local and second parameter.
    fn mir(statements: Vec<Statement>) -> Mir {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let mut function = MirFunction::new(FuncId(0), Symbol(0), HirOrigin::Body(BodyId(0)), float, span());
        let kinds = [
            LocalKind::Param { symbol: None },
            LocalKind::Temp,
            LocalKind::UserBinding(Symbol(1)),
            LocalKind::Param { symbol: None },
        ];
        for kind in kinds {
            function.push_local(LocalDecl { ty: float, kind, span: span() });
        }
        function.params = vec![SOURCE, OTHER];
        function.blocks[0].statements = statements;
        function.blocks[0].terminator = Some(Terminator::Return(copy(USER)));
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
        }
    }

    /// Asserts the pass leaves `mir` as it found it.
    fn assert_declined(mut mir: Mir) {
        let before = format!("{:?}", mir.functions[0].blocks);
        let changed = TempCopyForwarding.run(&mut mir);
        assert!(!changed, "{:?}", mir.functions[0].blocks[0].statements);
        assert_eq!(format!("{:?}", mir.functions[0].blocks), before);
    }

    /// Makes `%3` a closure's capture of `%0`, where the closure writes it, so
    /// `%0` is shared; the creating statement goes at the front of the block.
    fn share_source(mir: &mut Mir) {
        let float = mir.functions[0].locals[0].ty;
        let mut closure = crate::MirClosure {
            id: ClosureId(0),
            params: Vec::new(),
            rest: None,
            required_params: None,
            locals: vec![LocalDecl { ty: float, kind: LocalKind::UserBinding(Symbol(2)), span: span() }],
            captures: vec![MirClosureCapture {
                source_local: SOURCE,
                target_local: Some(LocalId(0)),
                symbol: Symbol(2),
                ty: float,
                mode: smelt_hir::CaptureMode::ByRef,
            }],
            return_ty: float,
            blocks: mir.functions[0].blocks.clone(),
            entry: mir.functions[0].entry,
            escapes: false,
            can_throw: false,
            is_generator: false,
            function_item_key: None,
            language: smelt_hir::Language::TypeScript,
        };
        closure.blocks[0].statements = vec![Statement::Assign {
            dest: LocalId(0),
            value: Rvalue::Use(Operand::Const(crate::Constant::Float(2.0))),
        }];
        closure.blocks[0].terminator = Some(Terminator::Return(copy(LocalId(0))));
        mir.closures.push(closure);
        mir.functions[0].blocks[0].statements.insert(
            0,
            Statement::Assign {
                dest: OTHER,
                value: Rvalue::Closure { id: ClosureId(0), captures: vec![copy(SOURCE)] },
            },
        );
    }

    /// `%0` is not written: the consumer reads it and the copy goes.
    #[test]
    fn forwards_the_source_into_the_consumer() {
        let mut mir = mir(vec![define_temp(), consume_temp()]);
        assert!(TempCopyForwarding.run(&mut mir));
        let statements = &mir.functions[0].blocks[0].statements;
        assert_eq!(statements.len(), 1, "{statements:?}");
        let Statement::Assign { dest, value: Rvalue::Unary { operand, .. } } = &statements[0] else {
            unreachable!("{statements:?}");
        };
        assert_eq!(*dest, USER);
        assert_eq!(operand.local(), Some(SOURCE));
        assert!(!TempCopyForwarding.run(&mut mir), "a second run finds nothing");
    }

    /// A terminator is a consumer too.
    #[test]
    fn forwards_into_a_terminator() {
        let mut mir = mir(vec![define_temp()]);
        mir.functions[0].blocks[0].terminator = Some(Terminator::Return(copy(TEMP)));
        assert!(TempCopyForwarding.run(&mut mir));
        assert!(mir.functions[0].blocks[0].statements.is_empty());
        let Some(Terminator::Return(operand)) = &mir.functions[0].blocks[0].terminator else {
            unreachable!();
        };
        assert_eq!(operand.local(), Some(SOURCE));
    }

    /// A statement that does not touch an unshared source does not stop it.
    #[test]
    fn forwards_across_an_unrelated_statement() {
        let unrelated = Statement::AssignPlace {
            place: Place::Local(OTHER),
            value: Rvalue::Use(Operand::Const(crate::Constant::Float(1.0))),
        };
        let mut mir = mir(vec![define_temp(), unrelated, consume_temp()]);
        assert!(TempCopyForwarding.run(&mut mir));
    }

    /// `f(a, a = 2)`: the first argument is the value before the write.
    #[test]
    fn declines_across_a_whole_local_write() {
        assert_declined(mir(vec![define_temp(), write_source(), consume_temp()]));
    }

    /// A field or index store changes the source in part.
    #[test]
    fn declines_across_a_field_write() {
        let store = Statement::AssignPlace {
            place: Place::Field { base: SOURCE, field: Symbol(9) },
            value: Rvalue::Use(Operand::Const(crate::Constant::Float(1.0))),
        };
        assert_declined(mir(vec![define_temp(), store, consume_temp()]));
    }

    /// An in-place collection operation changes its receiver.
    #[test]
    fn declines_across_a_receiver_mutation() {
        let clear = Statement::Assign { dest: OTHER, value: Rvalue::ListClear { list: copy(SOURCE) } };
        assert_declined(mir(vec![define_temp(), clear, consume_temp()]));
    }

    /// `xs.push(t)` with `t = xs`: the consumer itself mutates the source.
    #[test]
    fn declines_a_consumer_that_writes_the_source() {
        let push = Statement::Assign {
            dest: USER,
            value: Rvalue::ListPush { list: copy(SOURCE), item: copy(TEMP) },
        };
        assert_declined(mir(vec![define_temp(), push]));
    }

    /// `t.push(1)`: mutating the temporary is not a read of its value.
    #[test]
    fn declines_a_consumer_that_mutates_the_temp() {
        let clear = Statement::Assign { dest: USER, value: Rvalue::ListClear { list: copy(TEMP) } };
        assert_declined(mir(vec![define_temp(), clear]));
    }

    /// A closure that writes the source may run in any call in between.
    #[test]
    fn declines_a_shared_source_across_a_call() {
        let call = Statement::Assign {
            dest: OTHER,
            value: Rvalue::ClosureCall { callee: copy(OTHER), args: Vec::new() },
        };
        let mut mir = mir(vec![define_temp(), call, consume_temp()]);
        share_source(&mut mir);
        assert_declined(mir);
    }

    /// A statement that runs no code leaves a shared source alone.
    #[test]
    fn forwards_a_shared_source_across_a_plain_copy() {
        let plain = Statement::Assign { dest: OTHER, value: Rvalue::Use(copy(OTHER)) };
        let mut mir = mir(vec![define_temp(), plain, consume_temp()]);
        share_source(&mut mir);
        assert!(TempCopyForwarding.run(&mut mir));
    }

    /// A closure snapshotting the temporary must not start capturing the source.
    #[test]
    fn declines_a_closure_capture() {
        let capture = Statement::Assign {
            dest: USER,
            value: Rvalue::Closure { id: ClosureId(0), captures: vec![copy(TEMP)] },
        };
        assert_declined(mir(vec![define_temp(), capture]));
    }

    /// A copy between types is a coercion.
    #[test]
    fn declines_a_copy_between_types() {
        let mut mir = mir(vec![define_temp(), consume_temp()]);
        let int = mir.types.intern(Type::Int);
        mir.functions[0].locals[1].ty = int;
        assert_declined(mir);
    }

    /// Callable locals keep their ABI.
    #[test]
    fn declines_function_typed_locals() {
        let mut mir = mir(vec![define_temp(), consume_temp()]);
        let float = mir.functions[0].locals[0].ty;
        let callable: TypeId = mir.types.intern(Type::Function(FunctionType {
            params: Vec::new(),
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty: float,
            is_async: false,
            may_throw: false,
        }));
        mir.functions[0].locals[0].ty = callable;
        mir.functions[0].locals[1].ty = callable;
        assert_declined(mir);
    }

    /// A named binding is not a temporary, and a temporary read twice is not a
    /// second name for one read.
    #[test]
    fn declines_anything_but_a_single_use_temp() {
        let mut named = mir(vec![define_temp(), consume_temp()]);
        named.functions[0].locals[1].kind = LocalKind::UserBinding(Symbol(3));
        assert_declined(named);
        assert_declined(mir(vec![define_temp(), consume_temp(), consume_temp()]));
    }

    /// A read through a projection is not a bare operand.
    #[test]
    fn declines_a_projected_read() {
        let read = Statement::Assign {
            dest: USER,
            value: Rvalue::Use(Operand::Copy(Place::Field { base: TEMP, field: Symbol(9) })),
        };
        assert_declined(mir(vec![define_temp(), read]));
    }

    /// The read must follow the copy in the same block.
    #[test]
    fn declines_a_read_in_another_block() {
        let mut mir = mir(vec![define_temp()]);
        let next = mir.functions[0].push_block(span());
        mir.functions[0].blocks[0].terminator = Some(Terminator::Goto(next));
        mir.functions[0].blocks[next.0 as usize].statements = vec![consume_temp()];
        mir.functions[0].blocks[next.0 as usize].terminator = Some(Terminator::Return(copy(USER)));
        assert_declined(mir);
    }
}
