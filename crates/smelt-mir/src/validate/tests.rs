//! Regression tests for the invariants the validator enforces.
//!
//! Each test builds the smallest MIR that breaks one invariant from the
//! invariant audit (section C) and asserts the validator rejects it, next to a
//! well-formed twin it accepts. The audit id is in each test's doc comment.

use smelt_hir::{
    BodyId, CaptureMode, FileId, OriginalNameTable, Span, Symbol, SymbolInterner, Type, TypeId,
    TypeInterner,
};

use crate::{
    BasicBlock, BlockId, Callee, ClosureId, Constant, ExceptionHandler, FuncId, HirOrigin,
    LocalDecl, LocalId, LocalKind, Mir, MirClosure, MirClosureCapture, MirFunction, MirGlobal,
    MirGlobalInit, Operand, Place, Rvalue, Statement, Terminator,
};

use super::validate;

/// A zero-width span; spans play no part in validation.
fn span() -> Span {
    Span::new(FileId(0), 0, 0)
}

/// A program with `Float` interned as `TypeId(0)` and `String` as `TypeId(1)`.
fn program() -> Mir {
    let mut types = TypeInterner::default();
    types.intern(Type::Float);
    types.intern(Type::String);
    Mir::new(types, SymbolInterner::default(), OriginalNameTable::default())
}

/// `Float`.
const FLOAT: TypeId = TypeId(0);
/// `String`.
const STRING: TypeId = TypeId(1);

/// A local of `ty` and `kind`.
fn local(ty: TypeId, kind: LocalKind) -> LocalDecl {
    LocalDecl { ty, kind, span: span() }
}

/// `copy %local`.
fn copy(local: u32) -> Operand {
    Operand::Copy(Place::Local(LocalId(local)))
}

/// A block with `statements`, ending in `terminator`.
fn block(id: u32, statements: Vec<Statement>, terminator: Terminator) -> BasicBlock {
    BasicBlock { id: BlockId(id), statements, terminator: Some(terminator), span: span() }
}

/// A function returning `Float` with `locals` and `blocks`.
fn function(locals: Vec<LocalDecl>, blocks: Vec<BasicBlock>) -> MirFunction {
    let mut function =
        MirFunction::new(FuncId(0), Symbol(0), HirOrigin::Body(BodyId(0)), FLOAT, span());
    function.locals = locals;
    function.blocks = blocks;
    function
}

/// A closure with `locals`, `captures` and `blocks`.
fn closure(
    locals: Vec<LocalDecl>,
    captures: Vec<MirClosureCapture>,
    blocks: Vec<BasicBlock>,
) -> MirClosure {
    MirClosure {
        id: ClosureId(0),
        params: Vec::new(),
        rest: None,
        required_params: None,
        locals,
        captures,
        return_ty: FLOAT,
        blocks,
        entry: BlockId(0),
        escapes: false,
        can_throw: false,
        is_generator: false,
        function_item_key: None,
        language: smelt_hir::Language::TypeScript,
    }
}

/// Whether some error message contains `needle`.
fn reports(program: &Mir, needle: &str) -> bool {
    validate(program).iter().any(|error| error.message.contains(needle))
}

/// A closure body is validated like a function body: a block with no
/// terminator and a temp read before it is defined are both reported (C1).
#[test]
fn a_closure_body_is_validated_like_a_function_body() {
    let mut mir = program();
    mir.push_function(function(Vec::new(), vec![block(0, Vec::new(), Terminator::Return(Operand::Const(Constant::Float(0.0))))]));
    let mut bad = closure(
        vec![local(FLOAT, LocalKind::Temp)],
        Vec::new(),
        vec![block(0, Vec::new(), Terminator::Return(copy(0)))],
    );
    bad.blocks.push(BasicBlock { id: BlockId(1), statements: Vec::new(), terminator: None, span: span() });
    mir.push_closure(bad);
    assert!(reports(&mir, "closure ClosureId(0) reads local LocalId(0)"));
    assert!(reports(&mir, "Closure(ClosureId(0)) block BlockId(1) is missing a terminator"));

    let mut good = program();
    good.push_function(function(Vec::new(), vec![block(0, Vec::new(), Terminator::Return(Operand::Const(Constant::Float(0.0))))]));
    good.push_closure(closure(
        vec![local(FLOAT, LocalKind::Temp)],
        Vec::new(),
        vec![block(
            0,
            vec![Statement::Assign { dest: LocalId(0), value: Rvalue::Use(Operand::Const(Constant::Float(1.0))) }],
            Terminator::Return(copy(0)),
        )],
    ));
    assert_eq!(validate(&good), []);
}

/// A program whose function `%0: source_ty` is captured by closure 0 into
/// its `%0: target_ty`, recorded as `capture_ty` from `recorded_source`.
fn captured(
    source_ty: TypeId,
    capture_ty: TypeId,
    target_ty: TypeId,
    recorded_source: u32,
) -> Mir {
    let mut mir = program();
    mir.push_function(function(
        vec![
            local(source_ty, LocalKind::UserBinding(Symbol(1))),
            local(STRING, LocalKind::UserBinding(Symbol(2))),
            local(FLOAT, LocalKind::Temp),
        ],
        vec![block(
            0,
            vec![
                Statement::Assign { dest: LocalId(0), value: Rvalue::Use(Operand::Const(Constant::Float(1.0))) },
                Statement::Assign { dest: LocalId(1), value: Rvalue::Use(Operand::Const(Constant::String("s".into()))) },
                Statement::Assign {
                    dest: LocalId(2),
                    value: Rvalue::Closure { id: ClosureId(0), captures: vec![copy(0)] },
                },
            ],
            Terminator::Return(Operand::Const(Constant::Float(0.0))),
        )],
    ));
    mir.push_closure(closure(
        vec![local(target_ty, LocalKind::UserBinding(Symbol(1)))],
        vec![MirClosureCapture {
            source_local: LocalId(recorded_source),
            target_local: Some(LocalId(0)),
            symbol: Symbol(1),
            ty: capture_ty,
            mode: CaptureMode::ByValue,
        }],
        vec![block(0, Vec::new(), Terminator::Return(copy(0)))],
    ));
    mir
}

/// A capture's type, its source local's type and its target local's type
/// are one type (closure invariant 4, C2).
#[test]
fn capture_types_must_agree_with_both_locals() {
    assert_eq!(validate(&captured(FLOAT, FLOAT, FLOAT, 0)), []);
    assert!(reports(&captured(STRING, FLOAT, FLOAT, 0), "but its source local"));
    assert!(reports(&captured(FLOAT, FLOAT, STRING, 0), "but its target local"));
}

/// A capture's `source_local` is the local its construction site reads
/// (C4), and it must exist in the constructing body (C13).
#[test]
fn capture_source_must_match_its_construction_site() {
    assert!(reports(&captured(FLOAT, FLOAT, FLOAT, 1), "but its construction captures"));
    assert!(reports(&captured(FLOAT, FLOAT, FLOAT, 9), "is not a local of"));
}

/// `sync_capture_sources` re-derives a stale `source_local` from the site.
#[test]
fn capture_sources_are_derived_from_their_site() {
    let mut mir = captured(FLOAT, FLOAT, FLOAT, 1);
    crate::sync_capture_sources(&mut mir);
    assert_eq!(validate(&mir), []);
}

/// A trivial `return 0.0` function.
fn returning() -> MirFunction {
    function(Vec::new(), vec![block(0, Vec::new(), Terminator::Return(Operand::Const(Constant::Float(0.0))))])
}

/// `functions[i].id == i`, checked in release builds (C5).
#[test]
fn a_function_id_must_equal_its_position() {
    let mut mir = program();
    mir.push_function(returning());
    assert_eq!(validate(&mir), []);
    mir.functions.push(returning());
    assert!(reports(&mir, "function index 1 has mismatched id FuncId(0)"));
}

/// Every type position is checked, including those normalization and the
/// validator used to skip: a global's type and a `JsonParse` target (C6).
#[test]
fn every_type_position_is_validated() {
    let mut mir = program();
    mir.push_function(function(
        vec![local(STRING, LocalKind::Temp), local(FLOAT, LocalKind::Temp)],
        vec![block(
            0,
            vec![
                Statement::Assign { dest: LocalId(0), value: Rvalue::Use(Operand::Const(Constant::String("1".into()))) },
                Statement::Assign {
                    dest: LocalId(1),
                    value: Rvalue::JsonParse { text: copy(0), target: Some(TypeId(77)) },
                },
            ],
            Terminator::Return(copy(1)),
        )],
    ));
    mir.globals.push(MirGlobal {
        name: Symbol(5),
        ty: TypeId(88),
        init: MirGlobalInit::Constant(Constant::Float(0.0)),
    });
    assert!(reports(&mir, "unknown type TypeId(77)"));
    assert!(reports(&mir, "unknown type TypeId(88)"));
}

/// A function whose call's `dest` (`%0`) is read on the call's unwind edge
/// (`read_in_catch`) or on its normal edge.
fn call_with_handler(read_in_catch: bool) -> Mir {
    let mut mir = program();
    mir.push_function(returning());
    let read = Terminator::Return(copy(0));
    let other = Terminator::Return(Operand::Const(Constant::Float(0.0)));
    let (target, catch) = if read_in_catch { (other, read) } else { (read, other) };
    mir.push_function(function(
        vec![local(FLOAT, LocalKind::Temp), local(STRING, LocalKind::Temp)],
        vec![
            block(
                0,
                Vec::new(),
                Terminator::Call {
                    callee: Callee::Static(FuncId(0)),
                    args: Vec::new(),
                    dest: LocalId(0),
                    target: BlockId(1),
                    unwind: Some(ExceptionHandler { catch_block: BlockId(2), exception_local: Some(LocalId(1)) }),
                },
            ),
            block(1, Vec::new(), target),
            block(2, Vec::new(), catch),
        ],
    ));
    mir
}

/// A call's `dest` is defined on its normal edge only, never in the catch
/// block its unwind edge reaches (C7).
#[test]
fn a_call_destination_is_undefined_on_the_unwind_edge() {
    assert_eq!(validate(&call_with_handler(false)), []);
    assert!(reports(&call_with_handler(true), "reads local LocalId(0)"));
}

/// The definite-assignment contract with the emitter (C8): a user binding
/// may be read before its first write (the emitter seeds it), a temporary may
/// not, and a block no path reaches is not checked (the emitter never renders
/// it).
#[test]
fn user_bindings_and_dead_blocks_follow_the_emitter_contract() {
    let read_first = |kind| {
        let mut mir = program();
        mir.push_function(function(
            vec![local(FLOAT, kind)],
            vec![block(0, Vec::new(), Terminator::Return(copy(0)))],
        ));
        mir
    };
    assert_eq!(validate(&read_first(LocalKind::UserBinding(Symbol(1)))), []);
    assert!(reports(&read_first(LocalKind::Temp), "reads local LocalId(0)"));

    let mut dead = program();
    dead.push_function(function(
        vec![local(FLOAT, LocalKind::Temp)],
        vec![
            block(0, Vec::new(), Terminator::Return(Operand::Const(Constant::Float(0.0)))),
            block(1, Vec::new(), Terminator::Return(copy(0))),
        ],
    ));
    assert_eq!(validate(&dead), []);
}

/// Two classes of one name are rejected (C12).
#[test]
fn a_repeated_class_name_is_rejected() {
    let class = |name| crate::MirClass {
        name,
        kind: smelt_hir::ClassKind::Plain,
        type_params: Vec::new(),
        is_abstract: false,
        base: None,
        base_args: Vec::new(),
        fields: Vec::new(),
        static_fields: Vec::new(),
        descriptors: Vec::new(),
        constructor: None,
        methods: Vec::new(),
        static_methods: Vec::new(),
        abstract_methods: Vec::new(),
        implements: Vec::new(),
        protocols: Vec::new(),
    };
    let mut mir = program();
    mir.classes = vec![class(Symbol(1)), class(Symbol(2))];
    assert_eq!(validate(&mir), []);
    mir.classes.push(class(Symbol(1)));
    assert!(reports(&mir, "is declared twice"));
}

/// Global indices and class member ids are range-checked (C13).
#[test]
fn globals_and_member_ids_are_range_checked() {
    let mut mir = program();
    mir.push_function(function(
        vec![local(FLOAT, LocalKind::Temp)],
        vec![block(
            0,
            vec![Statement::Assign { dest: LocalId(0), value: Rvalue::GlobalGet { global: 3 } }],
            Terminator::Return(copy(0)),
        )],
    ));
    assert!(reports(&mir, "unknown global 3"));

    let mut with_class = program();
    with_class.push_function(returning());
    with_class.classes.push(crate::MirClass {
        name: Symbol(1),
        kind: smelt_hir::ClassKind::Plain,
        type_params: Vec::new(),
        is_abstract: false,
        base: None,
        base_args: Vec::new(),
        fields: Vec::new(),
        static_fields: Vec::new(),
        descriptors: Vec::new(),
        constructor: None,
        methods: vec![FuncId(0), FuncId(4)],
        static_methods: Vec::new(),
        abstract_methods: Vec::new(),
        implements: Vec::new(),
        protocols: Vec::new(),
    });
    assert!(reports(&with_class, "method references unknown function FuncId(4)"));
}
