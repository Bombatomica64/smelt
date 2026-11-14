"""Section 5: MIR in one picture."""
from svg import Svg, L, H, S, esc

MR = "crates/smelt-mir/src/"
EX = "examples/typescript/end-to-end/12_while_sum/"


def model():
    g = Svg(1040, 600, "MIR data model and a real control-flow graph")
    X, W = 16, 470
    a = g.node(X, 16, W, 78, [H("Mir"), L("functions, classes, interfaces, closures, globals"),
                              L("types: TypeInterner, symbols, names")], kind="mir", href=MR + "types.rs")
    b = g.node(X, 120, W, 96, [H("MirFunction"), L("params, locals: Vec<LocalDecl>, blocks, entry"),
                               L("return_ty, type_params, language, origin"),
                               L("flags: is_async, is_generator, is_test, can_throw")], kind="mir", href=MR + "types.rs")
    c = g.node(X + 60, 242, 196, 96, [H("LocalDecl"), L("ty: TypeId"), L("kind: Param | Temp"), L("  | UserBinding")],
               kind="mir", href=MR + "types.rs")
    c2 = g.node(X + 270, 242, 200, 96, [H("LocalInit"), L("Definite"), L("MaybeUnbound →"), L("  BindingCheck read")],
                kind="mir", href=MR + "definite_assignment.rs")
    d = g.node(X, 364, W, 78, [H("BasicBlock"), L("statements: Assign | AssignPlace | DictEntryUpdate"),
                               L("terminator: Option<Terminator>")], kind="mir", href=MR + "types.rs")
    e = g.node(X, 468, W, 116, [H("Terminator"), L("Goto, Switch, Match, Return, Throw"),
                                L("Call { callee, args, dest, target, unwind }"),
                                L("Await { .., unwind }  (only these two unwind)"),
                                L("Callee: Static | Indirect | Builtin | Op | BaseInit")], kind="mir", href=MR + "types.rs")
    for p, q in ((a, b), (b, d), (d, e)):
        g.path(f"M{X + 40},{p['b']} V{q['y'] - 2}")
    g.path(f"M{X + 140},{b['b']} V{c['y'] - 2}")
    g.path(f"M{c['r']},{c['cy']:.0f} H{c2['x'] - 2}", cls="edge light")

    # real CFG
    CX = 520
    g.add(g.text(CX, 28, S("examples/…/12_while_sum: expected.mir (golden)", 13)))
    loc = g.node(CX, 38, 504, 78, [L("%0 user i: usize   %1 user sum: Float", bold=True),
                                   L("%2 temp: Bool  %3 temp: Float  %4 temp: usize"),
                                   L("%5 temp: None  %6 temp: Float")], kind="mir", href=EX + "expected.mir", cls="node cfg")
    b0 = g.node(CX, 136, 200, 78, [L("bb0", bold=True), L("%0 = 0"), L("%1 = 0.0  goto bb1")], kind="mir", cls="node cfg")
    b1 = g.node(CX, 240, 230, 78, [L("bb1", bold=True), L("%2 = copy %0 < 4"), L("switch move %2")], kind="mir", cls="node cfg")
    b2 = g.node(CX, 352, 290, 152, [L("bb2", bold=True), L("%6 = primitive_cast_float"), L("       copy %0"),
                                    L("%3 = move %1 + move %6"), L("%1 = move %3"), L("%4 = move %0 + 1"),
                                    L("%0 = move %4   goto bb1")], kind="mir", cls="node cfg")
    b3 = g.node(CX + 314, 352, 190, 78, [L("bb3", bold=True), L("%5 = call"), L("@console_log(move %1)")], kind="mir", cls="node cfg")
    b4 = g.node(CX + 314, 470, 190, 56, [L("bb4", bold=True), L("return none")], kind="mir", cls="node cfg")
    g.path(f"M{b0['x'] + 40},{b0['b']} V{b1['y'] - 2}")
    g.path(f"M{b1['x'] + 40},{b1['b']} V{b2['y'] - 2}")
    g.tag(b1["x"] + 40, (b1["b"] + b2["y"]) / 2, "true", kind="label")
    g.path(f"M{b1['r']},{b1['y'] + 50} H{b3['cx']:.0f} V{b3['y'] - 2}")
    g.tag(b3["cx"], b1["y"] + 50, "false", kind="label")
    g.path(f"M{b3['cx']:.0f},{b3['b']} V{b4['y'] - 2}")
    g.tag(b3["cx"] + 40, (b3["b"] + b4["y"]) / 2, "target", kind="label", anchor="start")
    # back edge bb2 -> bb1
    g.path(f"M{b2['r']},{b2['b'] - 20} H{b2['r'] + 12} V{b1['y'] + 20} H{b1['r'] + 2}", cls="edge back")
    g.add(g.text(CX, 556, S("usize for i: IntegerRange.  move: MoveOnLastUse (no clone).", 13)))
    g.add(g.text(CX, 576, S("Rust: loop { … if !(_smelt_tmp_2) { break; } … } (expected.rs)", 13)))
    return g.render()


FINAL = [
    ("check_unbound_bindings", "MaybeUnbound bindings and BindingCheck reads (TDZ, Python unbound names)", "lower/passes/unbound_bindings.rs"),
    ("remove_unreachable_blocks", "drops blocks unreachable from entry, so validation sees every block", "reachability.rs"),
    ("abstract_classes_as_unions", "a member-less abstract class type becomes the union of its concrete classes", "lower/passes/abstract_class_unions.rs"),
    ("intern_generic_record_instantiations", "every generic record instantiation used is in the type table", "lower/passes/generic_records.rs"),
    ("mark_escaping_closures", "sets MirClosure::escapes; escaping closures capture by value", "lower/closures.rs"),
    ("run_throwing_fixpoint (≤ 32 rounds)", "propagate_throwing_functions ⇄ widen_throwing_closure_types: can_throw", "lower/mod.rs"),
    ("promote_erased_mutated_records", "erased-and-mutated record: Dict(K, V) → Dict(K, unknown)", "erased_record_promote.rs"),
    ("normalize_operational_types", "normalizes the operational types the emitter relies on", "type_normalize.rs"),
    ("intern_inherited_member_types", "types of inherited member copies are in the table", "lower/passes/inherited_types.rs"),
]
FIX = [
    ("CopyPropagation", "x = copy y with one definition: uses of x read y (not temps, not closure-shared)", "opt/mod.rs"),
    ("MoveOnLastUse", "Copy(x) → Move(x) where x is dead afterwards; codegen drops the .clone()", "opt/move_on_last_use.rs"),
    ("LetTempElision", "initializer written straight into the user local; the temp and move go", "opt/let_temp_elision.rs"),
    ("TempCopyForwarding", "a temp read once is replaced by the value it copied", "opt/temp_copy_forwarding.rs"),
    ("RestCallFusion", "a packed rest list is folded into the call: f(rest copy %2)", "opt/rest_call_fusion.rs"),
    ("DictEntryInPlaceMutation", "d[k].push(x) copy-out/copy-in triple → one DictEntryPush", "opt/dict_entry_mutation.rs"),
    ("DictEntryUpdate", "d[k] = f(d[k] ?? seed) → one DictEntryUpdate statement", "opt/dict_entry_update.rs"),
    ("DictDefaultInsertElision", "drops a guarded default insert a following entry mutation subsumes", "opt/dict_default_insert_elision.rs"),
    ("UnobservedReceiverBind", "drops BindThis when no function reads this (no SMELT_THIS channel)", "opt/unobserved_receiver_bind.rs"),
]


def passes():
    rows = 3 + 7 + len(FINAL) + len(FIX) + 4
    RH = 30
    g = Svg(1040, 70 + rows * RH + 140, "MIR stages and passes in the order they run")
    y = 16
    NX, DX = 44, 364

    def section(title, href, kind):
        nonlocal y
        g.node(16, y, 1008, 32, [H(title)], kind=kind, href=href, cls="node band", pad=7)
        y += 40

    def prow(i, name, desc, href=None, indent=0):
        nonlocal y
        g.add(g.text(NX - 22 + indent, y + 15, L(f"{i}", cls="t2")))
        g.add(g.text(NX + indent, y + 15, L(name, href=(MR + href) if href else None)))
        assert len(desc) * 13 * 0.57 < 1024 - DX, desc
        g.add(g.text(DX, y + 15, S(desc)))
        y += RH - 4

    section("1  lower_hir  (lower/mod.rs) — HIR to MIR", MR + "lower/mod.rs", "mir")
    for i, (n, d) in enumerate([
        ("smelt_hir::validate", "HIR validation errors become LowerErrors"),
        ("assign_function_ids", "a FuncId per HIR function item, reserved before any body lowers"),
        ("lower_globals, lower_type_tables", "module mutable globals; class and interface tables"),
        ("NameIndex::new", "rejects duplicate class or interface names"),
        ("lower_item_functions", "each function body into a CFG"),
        ("lower_module_bodies", "module top-level code"),
        ("run_finalization_passes", "the fixed order below, each stating what it requires"),
    ], 1):
        prow(i, n, d)
    y += 6
    section("2  run_finalization_passes  (lower/mod.rs)", MR + "lower/mod.rs", "mir")
    for i, (n, d, h) in enumerate(FINAL, 1):
        prow(i, n, d, h)
    y += 6
    section("3  opt::try_optimize  (opt/mod.rs)", MR + "opt/mod.rs", "mir")
    top = y
    prow("", "check_after(\"lowering\")", "validate the input (debug and test builds)")
    loop_top = y
    for i, (n, d, h) in enumerate(FIX, 1):
        prow(i, n, d, h, indent=12)
    loop_bot = y
    g.add(f'<path d="M{NX - 30},{loop_bot - 6} H{NX - 34} V{loop_top + 4} H{NX - 28}" class="edge back" marker-end="url(#arr)"/>')
    prow("", "  after every pass", "remove_unreachable_blocks, sync_capture_sources, clear_index; validate in debug/test")
    prow("", "  fixpoint", "repeat the 9 passes until a whole round changes nothing; error after 64 rounds")
    prow(10, "IntegerRange (once)", "provably integral number locals: f64 → usize / i64 (JS bodies only)", "opt/integer_range.rs")
    y += 6
    section("4  validate  (validate/mod.rs) — in every build; compile fails on any error", MR + "validate/mod.rs", "mir")
    inv = [
        ("tables", "function ids are positions; member and initializer ids in range; unique class names"),
        ("closures", "closure ids are positions; one construction site; capture types agree; escaping by value"),
        ("structure", "entry, blocks, locals and terminator targets exist"),
        ("operands", "every place, global and callee a statement reads exists"),
        ("assignment", "definite assignment: each read assigned or BindingCheck'd; every block reachable"),
        ("types", "every TypeId in the MIR exists in the interner"),
    ]
    for n, d in inv:
        prow("", n, d, f"validate/{n}.rs" if n not in ("types",) else "validate/mod.rs")
    g.h = y + 10
    return g.render()
