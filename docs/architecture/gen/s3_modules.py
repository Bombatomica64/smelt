"""Section 3: module maps of the big crates."""
import subprocess, collections
from svg import Svg, L, H, S, esc, link, tw

ROOT = __import__("subprocess").check_output(["git", "rev-parse", "--show-toplevel"], cwd=__import__("os").path.dirname(__import__("os").path.abspath(__file__))).decode().strip()
DEEP = {"smelt-frontend-ts": "lowering", "smelt-codegen-rust": "emitter", "smelt-runtime": "runtime",
        "smelt-frontend-py": "lowering", "smelt-mir": "lower", "smelt-transpiler": None}
KIND = {"smelt-frontend-ts": "front", "smelt-frontend-py": "front", "smelt-codegen-rust": "codegen",
        "smelt-mir": "mir", "smelt-runtime": "runtime", "smelt-transpiler": "driver"}
REST = "*"

FT = "crates/smelt-frontend-ts/src/"
CG = "crates/smelt-codegen-rust/src/"
MR = "crates/smelt-mir/src/"
RT = "crates/smelt-runtime/src/"
TR = "crates/smelt-transpiler/src/"
FP = "crates/smelt-frontend-py/src/"

# (group, entry function, entry path, [module keys])
GROUPS = {
    "smelt-frontend-ts": [
        ("Host and stdlib mapping", "ModuleBuilder::call_expression_with_hint", FT + "lowering/stdlib/call_dispatch.rs",
         ["lowering/stdlib", "lowering/stdlib_dispatch", "lowering/host_override", "lowering/ambient_globals",
          "lowering/bigint", "lowering/promise_resolution"]),
        ("Expressions", "ModuleBuilder::expression_with_hint", FT + "lowering/expr/dispatch.rs",
         ["lowering/expr", "lowering/new_expr", "lowering/spread_arguments", "lowering/typed_spread",
          "lowering/union_member_read", "lowering/asserted_member_read", "lowering/js_text", "lowering/conditional_base",
          "lowering/postfix_deferral", "lowering/super_method", "lowering/peel"]),
        ("Types and checker facts", "ModuleBuilder::ts_type_to_hir", FT + "lowering/ty/annotations.rs",
         ["lowering/ty", "lowering/checker_types", "lowering/int32", "lowering/type_param_owners", "lowering/union_flat",
          "lowering/tuple_rest", "lowering/open_array_hint", "lowering/erasure"]),
        ("Declarations", "ModuleBuilder::class_declaration", FT + "lowering/decls/classes.rs",
         ["lowering/decls", "lowering/hoisting", "lowering/function_statics", "lowering/symbol_consts"]),
        ("Statements", "ModuleBuilder::statement_in_block", FT + "lowering/stmt/statements.rs", ["lowering/stmt"]),
        ("Module init", "ModuleBuilder::program", FT + "lowering/module_init.rs", ["lowering/module_init"]),
        ("Test harness", "test_function_from_arrow", FT + "lowering/testing/suites.rs", ["lowering/testing"]),
        ("Callbacks", "ModuleBuilder::callback_argument", FT + "lowering/callbacks/dispatch.rs",
         ["lowering/callbacks", "lowering/arguments_forwarding"]),
        ("Narrowing and guards", "ModuleBuilder::guard_narrowing", FT + "lowering/narrowing.rs",
         ["lowering/narrowing", "lowering/guards", "lowering/type_predicates", "lowering/presence_tested_locals",
          "lowering/null_absent", "lowering/assertion_views", "lowering/unchecked_returns"]),
        ("Builder state and entry", "to_hir_with_options", FT + "lowering.rs",
         ["lowering", "lowering/state", "lowering/support", "lowering/resolve", "lowering/scoped_references", "lowering/own_this",
          "lowering/source_facts", "lowering/speculation", "lowering/body_arena", "lowering/specialization",
          "context", "untyped_imports", "error", "lib", "test_support", "ident"]),
        ("Unit tests", None, FT + "tests", ["tests"]),
    ],
    "smelt-codegen-rust": [
        ("Coercion seam", "FunctionEmitter::value_at_type", CG + "emitter/coercion.rs",
         ["emitter/coercion", "emitter/callable_object", "emitter/callback_handle", "emitter/seam_assertions",
          "emitter/record_slot_abi", "emitter/primitive_cast"]),
        ("Calls and closures", "FunctionEmitter::closure_expr", CG + "emitter/closures.rs",
         ["emitter/call", "emitter/call_generics", "emitter/closure_call", "emitter/closures", "emitter/capture_analysis",
          "emitter/bound_dispatch", "emitter/static_call_args", "emitter/weak_receiver", "emitter/virtual_slots",
          "emitter/receiver_bound_slot", "emitter/callback_signature", "emitter/construct"]),
        ("Values, places, operators", "FunctionEmitter::place_expr", CG + "emitter/place.rs",
         ["emitter/place", "emitter/rvalue", "emitter/field_access", "emitter/optional_access",
          "emitter/aggregate_literal", "emitter/binary_ops", "emitter/instance_of", "emitter/local_analysis",
          "emitter/truthiness", "emitter/default_value", "emitter/operand", "emitter/place_type", "emitter/tuple",
          "emitter/literals", "emitter/local_use", "emitter/local_binding", "emitter/local_declaration",
          "emitter/binding_storage", "emitter/class_keyed_write", "emitter/typeof_str_locals"]),
        ("Collections", "FunctionEmitter::dict_get_expr", CG + "emitter/map.rs",
         ["emitter/list", "emitter/list_query", "emitter/list_mutation", "emitter/list_ordering", "emitter/map",
          "emitter/set", "emitter/mut_list_call", "emitter/typed_array", "emitter/dict_entry_update"]),
        ("Control flow and effects", "FunctionEmitter::emit_block", CG + "emitter/control_flow.rs",
         ["emitter/control_flow", "emitter/control_flow_match", "emitter/flow_state", "emitter/post_dominance",
          "emitter/cfg_queries", "emitter/throwing_join", "emitter/fallible_op", "emitter/throw", "emitter/generator",
          "emitter/async_op"]),
        ("Strings and host APIs", "FunctionEmitter::string_case_expr", CG + "emitter/strings.rs",
         ["emitter/strings", "emitter/strings_io", "emitter/string_units", "emitter/fetch_types", "emitter/json_decode",
          "emitter/console_inspect", "emitter/http_server", "emitter/form_data", "emitter/crypto",
          "emitter/text_codec", "emitter/sqlite", "emitter/url", "emitter/blob", "emitter/abort_signal",
          "emitter/date_clock", "emitter/host_interop", "emitter/host_module", "emitter/host_member_read",
          "emitter/vitest_mock", "emitter/py_exception"]),
        ("Types and unions", "FunctionEmitter::rust_type", CG + "emitter/types.rs",
         ["emitter/types", "emitter/union", "emitter/type_facts", "emitter/typedness", "emitter/backend_erasure",
          "emitter/numeric", "emitter/bigint", "emitter/fixed_int"]),
        ("Emitter core and state", "FunctionEmitter::emit_free_function", CG + "emitter/function_item.rs", ["emitter", "emitter/" + REST]),
        ("Crate assembly and names", "emit_crate_with_modules_reporting", CG + "crate_writer.rs",
         ["crate_writer", "crate_builder", "startup", "module_map", "deps", "options", "lib", "error", "contract", "gates",
          "rust", "names"]),
        ("Runtime selection", "RuntimeDemand::of", CG + "runtime_demand.rs",
         ["runtime_demand", "runtime_prelude", "runtime_symbols_check", "accessor_prelude", "bigint_prelude",
          "console_inspect_prelude", "reflection_prelude", "typed_array_prelude", "stdlib"]),
        ("Program types and classes", "emit_program_types", CG + "program_types.rs",
         ["program_types", "classes", "class_proto", "classify", "derive_impls", "bound_traits", "base_init",
          "reference_storage", "reference_origin", "lazy_view", "erased_list_alias", "globals", "thrown"]),
        ("Generics", "function_emits_rust_generics", CG + "generic_lifting.rs",
         ["generic_bindings", "generic_lifting", "generic_elision", "type_substitution"]),
        ("Unit tests", None, CG + "tests", ["tests"]),
    ],
    "smelt-mir": [
        ("HIR → MIR lowering", "lower_hir", MR + "lower/mod.rs", ["lower/" + REST, "lower"]),
        ("Optimization passes", "opt::try_optimize", MR + "opt/mod.rs", ["opt"]),
        ("Data model and printing", "compile_observed", MR + "compile.rs",
         ["types", "traversal", "type_positions", "lib", "format", "compile"]),
        ("Analyses", "list_escape, definite_assignment", MR + "list_escape.rs",
         ["list_escape", "definite_assignment", "reachability", "closure_sites", "index"]),
        ("Finalization passes", "run_finalization_passes (lower/mod.rs)", MR + "lower/mod.rs",
         ["lower/passes", "erased_record_promote", "type_normalize"]),
        ("Validation", "validate", MR + "validate/mod.rs", ["validate"]),
    ],
    "smelt-runtime": [
        ("Erased value model", "SmeltUnknown (runtime/unknown.rs)", RT + "runtime/unknown.rs",
         ["runtime/unknown", "runtime/property_access", "runtime/erased_method", "runtime/callable_registry",
          "runtime/function_object", "runtime/field_store", "runtime/object_model", "runtime/object", "runtime/record",
          "runtime/host_value_erasure", "runtime/boxing", "runtime/builtin_member", "runtime/structural_eq",
          "runtime/key_eq", "runtime/identity", "runtime/this_channel", "runtime/erased_function", "runtime/value",
          "runtime/binding", "runtime/accessor", "runtime/host_method", "runtime/host_override", "runtime/reflection",
          "runtime/captures", "runtime/for_in", "runtime/to_string_tag", "runtime/view_write_through",
          "runtime/symbol", "runtime/symbol_keys"]),
        ("Host APIs (web, node)", "runtime/fetch_types.rs", RT + "runtime/fetch_types.rs",
         ["runtime/fetch_types", "runtime/http_server", "runtime/host_module", "runtime/form_data", "runtime/blob",
          "runtime/blob_record", "runtime/url", "runtime/crypto", "runtime/text_codec", "runtime/sqlite",
          "runtime/timers", "runtime/promise", "runtime/generator", "runtime/event_emitter", "runtime/abort",
          "runtime/console_inspect"]),
        ("JS values and text", "runtime/js_string.rs", RT + "runtime/js_string.rs",
         ["runtime/js_string", "runtime/strings", "runtime/number_format", "runtime/string_to_number",
          "runtime/conversions", "runtime/bigint", "runtime/math", "runtime/json_serde", "runtime/json_indent",
          "runtime/regexp", "runtime/regexp_match", "runtime/date", "runtime/clock", "runtime/uri", "runtime/thrown"]),
        ("Collections", "runtime/list.rs", RT + "runtime/list.rs",
         ["runtime/array", "runtime/list", "runtime/js_set", "runtime/js_map", "runtime/prim_set",
          "runtime/typed_array", "runtime/byte_buffer"]),
        ("Text machinery (not emitted)", "source::render_item, order::ORDER", RT + "source.rs",
         ["source", "order", "lib", "runtime_tests"]),
        ("Test support", "runtime/vitest_mock.rs", RT + "runtime/vitest_mock.rs",
         ["runtime/fast_check", "runtime/vitest_mock", "runtime/test_checks"]),
        ("Python values", "runtime/py_repr.rs", RT + "runtime/py_repr.rs", ["runtime/py_repr", "runtime/py_exception"]),
    ],
    "smelt-transpiler": [
        ("Reports and probes", "classify_line (unknown_report/classify.rs)", TR + "unknown_report/classify.rs",
         ["unknown_report", "typedness_report", "list_escape_report", "report", "probe", "test_report", "ts7_parity",
          "ts7_parity_report"]),
        ("Driver and type gate", "ManifestLowering::check", TR + "lowering.rs",
         ["lowering", "type_gate", "ts_facts", "check_program", "python", "stubs", "specialization", "sound",
          "sound_overlay"]),
        ("Manifest and sources", "manifest::dependency_closure", TR + "manifest.rs",
         ["config", "config_parser", "manifest", "source_files", "package_sources"]),
        ("Build pipeline and erasure", "build_rust_crate_with_erasure", TR + "pipeline.rs",
         ["pipeline", "diagnostics", "erasure"]),
        ("CLI", "cli::run", TR + "cli.rs", ["main", "cli", "cli_parser", "lib", "timing", "scaffold", "clean"]),
    ],
    "smelt-frontend-py": [
        ("Expressions and calls", "call_expression_with_hint", FP + "lowering/call.rs",
         ["lowering/call", "lowering/literals", "lowering/member", "lowering/format", "lowering/list", "lowering/map"]),
        ("Statements and modules", "statement_in_block", FP + "lowering/statement.rs",
         ["lowering/statement", "lowering/control_flow", "lowering/exceptions", "lowering/match",
          "lowering/module_variables", "lowering/module_imports"]),
        ("Classes and functions", "lower_class_def", FP + "lowering/class.rs",
         ["lowering/class", "lowering/class_init", "lowering/function", "lowering/closure", "lowering/default_params"]),
        ("Stdlib mapping", "stdlib_dispatch::call_rule", FP + "lowering/stdlib_dispatch.rs",
         ["lowering/stdlib_types", "lowering/stdlib_collections", "lowering/stdlib_call", "lowering/stdlib_slice",
          "lowering/math", "lowering/stdlib_dispatch", "lowering/stdlib"]),
        ("Specialization", "specialization_for_path", FP + "lowering/specialization.rs", ["lowering/specialization"]),
        ("Types and ty", "annotation_to_hir", FP + "lowering/types.rs",
         ["lowering/types", "lowering/type_helpers", "lowering/validate", "ty_resolve"]),
        ("Entry and names", "to_hir_with_options", FP + "lib.rs",
         ["lowering", "lib", "name_resolution", "helpers", "test_support", "lowering/pytest"]),
        ("Unit tests", None, FP + "tests", ["tests"]),
    ],
}


def keyed(crate):
    base = f"crates/{crate}/src/"
    files = [f for f in subprocess.check_output(["git", "ls-files", base], cwd=ROOT).decode().split() if f.endswith(".rs")]
    deep = DEEP[crate]
    out = collections.defaultdict(lambda: [0, 0, []])
    for f in files:
        parts = f[len(base):].split("/")
        strip = lambda p: p[:-3] if p.endswith(".rs") else p
        if deep and parts[0] == deep and len(parts) > 1:
            k = deep + "/" + strip(parts[1])
        else:
            k = strip(parts[0])
        n = sum(1 for _ in open(f"{ROOT}/{f}", errors="ignore"))
        out[k][0] += n
        out[k][1] += 1
        out[k][2].append((n, f))
    return out


def top_files(n=10):
    files = [f for f in subprocess.check_output(["git", "ls-files", "crates"], cwd=ROOT).decode().split() if f.endswith(".rs")]
    sized = sorted(((sum(1 for _ in open(f"{ROOT}/{f}", errors="ignore")), f) for f in files), reverse=True)
    def is_test(f):
        return "/tests/" in f or f.endswith("_tests.rs") or f.endswith("/tests.rs") or "test_support" in f
    return sized[:n], [x for x in sized if not is_test(x[1])][:n], len(files), sum(s for s, _ in sized)


def assign(crate):
    mods = keyed(crate)
    groups = []
    taken = set()
    rest_group = None
    for gname, entry, epath, keys in GROUPS[crate]:
        mlist = []
        for k in keys:
            if k.endswith(REST):
                rest_group = (len(groups), k[:-1])
                continue
            if k in mods:
                mlist.append((k, *mods[k][:2]))
                taken.add(k)
        groups.append([gname, entry, epath, mlist])
    left = [k for k in mods if k not in taken]
    if rest_group:
        gi, prefix = rest_group
        for k in left[:]:
            if k.startswith(prefix):
                groups[gi][3].append((k, *mods[k][:2]))
                left.remove(k)
    if left:
        raise SystemExit(f"{crate}: unassigned modules {left}")
    total = sum(v[0] for v in mods.values())
    assert sum(m[1] for g in groups for m in g[3]) == total
    for g in groups:
        g[3].sort(key=lambda m: -m[1])
    groups.sort(key=lambda g: (g[0] == "Unit tests", -sum(m[1] for m in g[3])))
    return groups, total, mods


SCALE = 760 / 40000


def build(crate, prod_rank):
    groups, total, mods = assign(crate)
    kind = KIND[crate]
    RH = 80
    g = Svg(1040, 16 + RH * len(groups), f"Module map of {crate}")
    y = 8
    # file -> rank for production top files
    rank_of = {f: i + 1 for i, (n, f) in enumerate(prod_rank)}
    for gname, entry, epath, mlist in groups:
        gl = sum(m[1] for m in mlist)
        tests = gname == "Unit tests"
        g.add(g.text(8, y + 34, H(gname)))
        info = L(f"{gl / 1000:.1f}k lines · {len(mlist)} module{'s' if len(mlist) != 1 else ''}", cls="t2")
        g.add(g.text(8, y + 54, info))
        assert tw(gname, 14, False, True) < 248, gname
        x = 262
        for i, (k, n, files) in enumerate(mlist):
            w = n * SCALE
            seg_cls = f"seg k-{kind} {'alt' if i % 2 else ''} {'tests' if tests else ''}"
            short = k.split("/")[-1] or k
            label = short + (".rs" if files == 1 and k == (DEEP.get(crate) or "") else "")
            g.add(f'<rect class="{seg_cls}" x="{x:.1f}" y="{y + 24}" width="{max(w - 2, 1):.1f}" height="24" rx="2">'
                  f'<title>{esc(k)}: {n} lines in {files} file{"s" if files != 1 else ""}</title></rect>')
            lw = tw(short, 13)
            if lw + 10 < w - 2:
                g.add(g.text(x + 5, y + 41, L(short)))
            # badges for production top-10 files in this module
            base = f"crates/{crate}/src/"
            for f, r in rank_of.items():
                fk = f[len(base):] if f.startswith(base) else None
                if not fk:
                    continue
                parts = fk.split("/")
                strip = lambda p: p[:-3] if p.endswith(".rs") else p
                deep = DEEP[crate]
                fkey = deep + "/" + strip(parts[1]) if deep and parts[0] == deep and len(parts) > 1 else strip(parts[0])
                if fkey == k:
                    bx = x + min(12, max(w / 2, 10))
                    g.add(f'<g class="badge"><title>#{r} largest non-test file: {esc(f)}</title>'
                          f'<circle cx="{bx:.1f}" cy="{y + 12}" r="10"/>'
                          f'<text x="{bx:.1f}" y="{y + 16.5}" font-size="13" class="m b" text-anchor="middle">{r}</text></g>')
            x += w
        assert x <= 1030, (crate, gname, x)
        if entry:
            ent = L(f"entry  {entry}  ·  {epath.split('/src/', 1)[1]}", href=epath)
        else:
            ent = L(f"inline unit tests  ·  {epath.split('/src/', 1)[1]}/", href=epath, cls="t2")
        assert ent.width < 770, ent.text
        g.add(g.text(262, y + 68, ent))
        y += RH
    # complete table for click-to-expand
    rows = []
    for gname, entry, epath, mlist in groups:
        for k, n, files in mlist:
            rows.append((gname, k, n, files))
    return g.render(), total, rows, groups
