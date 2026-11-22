//! Valid Python whose generated Rust must print what `CPython` prints.
//!
//! Each `fixtures/python-parity/<name>.py` runs through a full `smelt build` and a
//! compile of the generated crate, and its stdout is compared with the committed
//! `<name>.stdout` and with `python3` itself. The Python examples contain no
//! lambda, so the byte oracle and example goldens cannot see what these pin.

mod common;

use std::fs;
use std::process::Command;

use common::{
    PYTHON_ORACLE, TempProject, TestResult, cargo_run_manifest, ensure, ensure_eq, output_within,
    program_run_limit, python_runs_corpus, smelt_in, workspace_root,
};
use smelt_specialize::prereq::missing_prerequisite;

/// Build fixture `name` and check its stdout against the golden and `CPython`.
fn verify_python_parity(name: &str) -> TestResult {
    let fixtures = workspace_root()?.join("crates/smelt-transpiler/tests/fixtures/python-parity");
    let script = fixtures.join(format!("{name}.py"));
    let expected = fs::read_to_string(fixtures.join(format!("{name}.stdout")))?;
    if python_runs_corpus() {
        let output = output_within(
            Command::new("python3").arg("-I").arg(&script),
            program_run_limit(),
            &format!("python3 on {name}"),
        )?;
        let python_stdout = String::from_utf8(output.stdout)?;
        ensure(
            python_stdout == expected,
            format!(
                "{name}: the golden stdout is not what `python3` prints for the same source\n--- python3\n{python_stdout}--- golden\n{expected}"
            ),
        )?;
    } else {
        missing_prerequisite::<Box<dyn std::error::Error>>(name, PYTHON_ORACLE)?;
    }
    let project = TempProject::new()?;
    let root = project.path();
    fs::create_dir_all(root.join("src"))?;
    fs::copy(&script, root.join("src/main.py"))?;
    fs::write(
        root.join("Smelt.toml"),
        r#"[project]
name = "python-parity"
version = "0.1.0"

[sources]
entries = ["src/main.py"]

[output]
target = "./dist"
crate-name = "python_parity"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    smelt_in(root, &["build"])?;
    let stdout = cargo_run_manifest(&root.join("dist/Cargo.toml"))?;
    ensure_eq(&stdout, &expected, format!("{name}: generated Rust stdout"))
}

/// `print` spells booleans and `None` the Python way.
#[test]
fn print_spellings() -> TestResult {
    verify_python_parity("print_spellings")
}

/// A lambda body is an ordinary expression, and `/`, `//` and `%` are Python's.
#[test]
fn lambda_arithmetic() -> TestResult {
    verify_python_parity("lambda_arithmetic")
}

/// A name read before an assignment bound it raises: `UnboundLocalError` for a
/// function local, `NameError` for a free variable or a module-level name. Each
/// is caught by an `except` naming it (or `NameError`, its base), propagates
/// out of a function that does not catch it, and a path that did assign the
/// name reads its value (C8).
#[test]
fn unbound_names() -> TestResult {
    verify_python_parity("unbound_names")
}

/// A raise from a `map`, `filter` or `sorted` callback reaches the enclosing `try`.
#[test]
fn try_around_raising_callback() -> TestResult {
    verify_python_parity("try_around_raising_callback")
}

/// `xs[i]` raises `IndexError`, `print` draws `repr`, and `x /= 2` makes a `float`.
#[test]
fn list_semantics() -> TestResult {
    verify_python_parity("list_semantics")
}

/// A `str` index counts code points, in `len`, `find`, `rfind` and slicing.
#[test]
fn str_code_point_indices() -> TestResult {
    verify_python_parity("str_code_point_indices")
}

/// A class with `__iter__`/`__next__` drives `for` until `StopIteration`.
#[test]
fn iterator_protocol() -> TestResult {
    verify_python_parity("iterator_protocol")
}

/// A `def` or parameter spelled like a builtin shadows it.
#[test]
fn builtin_names_shadowed() -> TestResult {
    verify_python_parity("builtin_names_shadowed")
}

/// A user method spelled like a builtin collection method is the user's.
#[test]
fn builtin_method_names_on_classes() -> TestResult {
    verify_python_parity("builtin_method_names_on_classes")
}

/// `print` of a container writes each element's Python `repr`.
#[test]
fn print_container_repr() -> TestResult {
    verify_python_parity("print_container_repr")
}

/// A `try` around a call that cannot raise defines the recovery helpers it
/// emits, runs its body and its `finally`.
#[test]
fn try_around_infallible_call() -> TestResult {
    verify_python_parity("try_around_infallible_call")
}

/// A user `open` is the user's function, not the builtin.
#[test]
fn user_open_shadows_builtin() -> TestResult {
    verify_python_parity("user_open_shadows_builtin")
}

/// A dict iterates in insertion order through every view of it, and a key
/// deleted and inserted again moves to the end.
#[test]
fn dict_insertion_order() -> TestResult {
    verify_python_parity("dict_insertion_order")
}

/// User and builtin exception classes: `raise`, `except` by class, tuple and
/// base class, bare `except` and `raise`, `else`, `finally`, `e.args`, and an
/// iterator whose `__next__` raises something other than `StopIteration`.
#[test]
fn exception_classes() -> TestResult {
    verify_python_parity("exception_classes")
}

/// A dict is a reference value: an alias, a list element and a parameter all
/// reach the same container; `dict(d)` copies it.
#[test]
fn dict_aliasing() -> TestResult {
    verify_python_parity("dict_aliasing")
}

/// `import asyncio` with `async def`, `await`, `asyncio.run`, `asyncio.gather`
/// and `asyncio.sleep` builds, and tasks resume in the event loop's order.
#[test]
fn asyncio_programs() -> TestResult {
    verify_python_parity("asyncio_programs")
}

/// `str()`, `repr()` and f-strings write Python text for containers,
/// exceptions and primitives.
#[test]
fn python_text() -> TestResult {
    verify_python_parity("python_text")
}

/// Negative indexes, clamped and stepped slices, and the `IndexError` and
/// `ValueError` `CPython` raises.
#[test]
fn sequence_indexing() -> TestResult {
    verify_python_parity("sequence_indexing")
}

/// `x /= n` rebinds an `int` name to a `float` in straight-line code.
#[test]
fn int_rebinds_to_float() -> TestResult {
    verify_python_parity("int_rebinds_to_float")
}

/// Destructuring `for` targets, tuple unpacking, `dict.pop` and `popitem`.
#[test]
fn dict_destructuring_and_pop() -> TestResult {
    verify_python_parity("dict_destructuring_and_pop")
}

/// Module variables written through `global`, defaulted parameters, generic
/// functions (`TypeVar` and PEP 695) and `str * int` repetition.
#[test]
fn module_state_and_defaults() -> TestResult {
    verify_python_parity("module_state_and_defaults")
}

/// A write through a class-typed or list-typed module global (a field, a
/// property setter, an augmented assignment, an index) lands in the global,
/// from module level and from a function.
#[test]
fn writes_through_module_globals() -> TestResult {
    verify_python_parity("writes_through_module_globals")
}

/// `set` mutation (`add`, `discard`, `remove`, `copy`, `clear`) and algebra run as Python runs them.
#[test]
fn python_set_methods() -> TestResult {
    verify_python_parity("python_set_methods")
}

/// List, dict, set, tuple and str methods and the math helpers behave as
/// Python's, including the exceptions for missing items.
#[test]
fn collection_methods() -> TestResult {
    verify_python_parity("collection_methods")
}

/// Searches, slices, negative indexes, tuple access and list mutation behave
/// as Python's, including the exceptions for out-of-range reads.
#[test]
fn sequence_operations() -> TestResult {
    verify_python_parity("sequence_operations")
}

/// Comprehensions, inheritance, abstract bases, generics, static forward
/// references, constructor-declared fields and every type-alias spelling run
/// as Python runs them.
#[test]
fn frontend_lowering_shapes() -> TestResult {
    verify_python_parity("frontend_lowering_shapes")
}

/// `list.pop()` on an empty list raises a catchable `IndexError`.
#[test]
fn list_pop_empty() -> TestResult {
    verify_python_parity("list_pop_empty")
}

/// Builtins, closures, JSON, regex and sequence operations behave like Python.
#[test]
fn builtins_and_sequences() -> TestResult {
    verify_python_parity("builtins_and_sequences")
}

/// Inherited methods, overrides, `super()` chains, properties and static members run as Python runs them.
#[test]
fn class_inheritance() -> TestResult {
    verify_python_parity("class_inheritance")
}

/// `json.dumps` with `CPython`'s default separators, float repr and
/// `ensure_ascii`, called from a function a module-level statement calls,
/// whose output prints once.
#[test]
fn json_dumps_defaults() -> TestResult {
    verify_python_parity("json_dumps_defaults")
}

/// The entry module's `__name__` is `"__main__"`, and output a decorator wrote
/// at definition time prints once, where its statement stands.
#[test]
fn module_entry_and_definition_output() -> TestResult {
    verify_python_parity("module_entry_and_definition_output")
}

/// A Python `int` never overflows: products, powers, shifts and literals past
/// 64 bits are exact; `//` and `%` floor, `/` is correctly rounded, an `int`
/// meets a `float` by exact value, and every failure (`ZeroDivisionError`,
/// `OverflowError`, `IndexError`, a negative shift count) is an exception a
/// `try` catches. Requirements: the printed values are `CPython`'s, and every
/// `int` in the generated Rust is a `SmeltPyInt`, never an `i64` that wraps.
#[test]
fn python_int_precision() -> TestResult {
    verify_python_parity("python_int_precision")
}

/// `int(text)`, `int(text, base)`, `int(x)` of a float and `float(text)`
/// follow `CPython`'s grammars, and a malformed or out-of-range input raises
/// the `ValueError`/`OverflowError` `CPython` raises, with its message, into
/// the enclosing `except` (never a panic).
#[test]
fn python_numeric_conversions() -> TestResult {
    verify_python_parity("python_numeric_conversions")
}

/// `list + list`, `list * n`, `n * list`, `tuple + tuple` and `tuple * n` build
/// new sequences that share their elements (`[[0]] * 3` holds one inner list
/// three times), and `+=`/`*=` mutate a list in place so its aliases see the
/// change. Requirements: `CPython`'s output, and the results keep their list
/// and tuple element types (`SmeltList<SmeltPyInt>`, typed tuples).
#[test]
fn sequence_arithmetic() -> TestResult {
    verify_python_parity("sequence_arithmetic")
}

/// A class's `__str__`/`__repr__` is its text everywhere `CPython` uses it:
/// `print`, `str`, `repr`, f-strings (`{x}` and `{x!r}`), inherited methods,
/// `str` falling back to `__repr__`, and each element of a printed container;
/// a raising `__str__` raises into the enclosing `except`.
#[test]
fn class_str_repr() -> TestResult {
    verify_python_parity("class_str_repr")
}

/// A `for`/`while` loop's `else` runs exactly when the loop ends without a
/// `break` (nested loops keep their own); a chained comparison evaluates each
/// middle operand once and stops at the first false link; `and`/`or` evaluate
/// their right operand only when the left one does not decide (a call there
/// runs only when reached).
#[test]
fn loop_else_and_chained_comparisons() -> TestResult {
    verify_python_parity("loop_else_and_chained_comparisons")
}

/// `str` methods with `CPython`'s rules over code points: title case through
/// Unicode's title mapping (`ß` → `Ss`), padding, bounded `count`/`find`/
/// `index`, whitespace and `maxsplit` splitting, counted replacement and
/// character-set stripping, with their `ValueError`/`TypeError`.
#[test]
fn python_str_methods() -> TestResult {
    verify_python_parity("python_str_methods")
}

/// `round` (half to even; correctly rounded `ndigits`; an `int` stays an
/// `int`), `divmod`, `pow` with and without a modulus, `chr`, `ord` and
/// `list.pop(i)`, with `CPython`'s exceptions and messages.
#[test]
fn python_numeric_builtins() -> TestResult {
    verify_python_parity("python_numeric_builtins")
}

/// f-string format specs follow the format-spec mini-language for `int`,
/// `float`, `str` and `bool` (fill, alignment, sign, `z`, `#`, zero padding,
/// width, grouping, precision, type); `{x=}` writes its own source text; a
/// spec the value's type does not take raises `ValueError`.
#[test]
fn fstring_format_specs() -> TestResult {
    verify_python_parity("fstring_format_specs")
}

/// A `@dataclass` has the generated `__repr__` (`Point(x=1, label='a')`)
/// wherever `CPython` shows it, unless the class defines its own.
#[test]
fn dataclass_repr() -> TestResult {
    verify_python_parity("dataclass_repr")
}

/// A comprehension binds a (nested) tuple target as a `for` statement does,
/// and tuples order lexicographically in `sorted` (both directions), `min` and
/// `max`, floats included.
#[test]
fn comprehension_targets_and_tuple_order() -> TestResult {
    verify_python_parity("comprehension_targets_and_tuple_order")
}

/// `min`/`max` of several arguments mixing `int` and `float` return one of
/// the arguments unchanged: `max(3, 2.5)` is the `int` `3`, the first of equal
/// values wins (`max(4, 4.0)` is `4`), an `int` meets a `float` by exact value
/// (`max(10**30, 1e30)` is the float), and a NaN compares as `CPython`
/// compares it. A module-level NaN reaches the program intact. Requirements:
/// `CPython`'s output, and the result is the `int | float` union
/// (`SmeltPyInt` or `f64`), printed through its live member's `str`/`repr`.
#[test]
fn min_max_mixed_numbers() -> TestResult {
    verify_python_parity("min_max_mixed_numbers")
}

/// `sorted` and `list.sort` over floats holding NaNs leave the order
/// `CPython`'s `listsort` leaves (every comparison with a NaN is false, so the
/// order is the algorithm's), in both directions and with a key, for lists
/// long enough to merge and gallop; a key runs once per item, in list order,
/// and a raising key leaves the list untouched. Requirements: `CPython`'s
/// output, never a panic, and the lists keep their `SmeltList<f64>` type.
#[test]
fn sort_nan_cpython_order() -> TestResult {
    verify_python_parity("sort_nan_cpython_order")
}

/// `json.dumps(indent=..)` lays the text out as `CPython`'s encoder does (an
/// `int` is that many spaces, `0` or a negative still breaks lines, a `str`
/// is used as given, empty containers stay `[]`/`{}`, strings are copied
/// untouched), and `json.loads` decodes at the annotated type: an `int`
/// stays exact past 64 bits, `NaN`/`Infinity` are floats, a later duplicate
/// key wins, and a malformed document raises `json.JSONDecodeError` (a
/// `ValueError`) with `CPython`'s message and position. Requirements:
/// `CPython`'s output, and the decoded values keep their static types
/// (`SmeltPyInt`, `SmeltList<SmeltPyInt>`, ...), never the erased carrier.
#[test]
fn json_indent_and_exact_loads() -> TestResult {
    verify_python_parity("json_indent_and_exact_loads")
}

/// `"template" % args` with a literal template formats each conversion as
/// `CPython`'s printf-style formatting does: `d i u o x X e E f F g G c s r
/// a %`, the `- + space # 0` flags, width and precision, `%d` of a `float`
/// truncating, `%s`/`%r` writing `str`/`repr` of any value with Python text,
/// a tuple supplying one value per conversion and any other value the only
/// one. Requirements: `CPython`'s output, and the result is a `str` built
/// from the values' own typed formatting (no erased value).
#[test]
fn percent_formatting() -> TestResult {
    verify_python_parity("percent_formatting")
}

/// `ascii(x)`, `f"{x!a}"` and `%a` are `repr(x)` with every non-ASCII code
/// point escaped as `\xhh`, `\uhhhh` or `\Uhhhhhhhh`, for strings and for
/// containers alike, and a format spec after `!a` pads the escaped text.
#[test]
fn ascii_conversion() -> TestResult {
    verify_python_parity("ascii_conversion")
}

/// `del xs[a:b]` and `del xs[a:b:c]` remove, in place, exactly the items the
/// same slice reads (`slice.indices` clamping, negative bounds and steps), so
/// an alias and a caller's list see the shorter list; a zero step raises a
/// catchable `ValueError`. Requirements: `CPython`'s output, and the list
/// keeps its `SmeltList<T>` element type.
#[test]
fn del_slice() -> TestResult {
    verify_python_parity("del_slice")
}

/// An unannotated list or dict literal whose items have different types is
/// typed as the union of their types, as `ty` infers it: `[1, [2, 3]]` is a
/// `list[int | list[int]]` and `[1, 2.5]` keeps `2.5` a float (it used to be
/// truncated to `2` under the first item's `int`). Requirements: `CPython`'s
/// output, and every item keeps its own type inside the union.
#[test]
fn list_literal_mixed_types() -> TestResult {
    verify_python_parity("list_literal_mixed_types")
}

/// `del xs[i]` removes one item in place (negative from the end), seen through
/// aliases and a caller's list; an index outside the list, an empty list
/// included, raises `IndexError("list assignment index out of range")`.
/// Requirements: `CPython`'s output, and the list keeps its element type.
#[test]
fn del_list_index() -> TestResult {
    verify_python_parity("del_list_index")
}

/// `%(key)s` conversions read their value from the mapping operand, evaluated
/// once before formatting: a dict literal's values keep their own types (so
/// `%(n)d` formats an `int`), a `dict[str, V]` value raises `KeyError` for a
/// missing key, flags and widths apply as for positional conversions.
/// Requirements: `CPython`'s output.
#[test]
fn percent_mapping_keys() -> TestResult {
    verify_python_parity("percent_mapping_keys")
}

/// A module-level function passed directly as a callback (`sorted(xs,
/// key=neg)`, `xs.sort(key=f)`, `map(f, xs)`) behaves like the lambda
/// `lambda x: neg(x)`: it is wrapped in a forwarding closure with the
/// function's own parameter and return types, and an exception it raises
/// propagates out of the sort. Requirements: `CPython`'s output, and the
/// callback keeps the function's concrete signature (no erased callable).
#[test]
fn callback_named_function() -> TestResult {
    verify_python_parity("callback_named_function")
}

/// An unannotated module-level `NAME = value` whose value is not a literal
/// (`x = float("2.5")`, `items = list(range(3))`, `n = helper()`) is a module
/// global typed by `ty`'s inference of the value: every function reads it, and
/// one declaring `global x` writes it. Requirements: `CPython`'s output, and
/// the global keeps the concrete inferred type (`f64`, `SmeltList<SmeltPyInt>`).
#[test]
fn module_global_inferred() -> TestResult {
    verify_python_parity("module_global_inferred")
}

/// `isinstance(x, T)` for builtin types, classes and tuples of them: decided
/// from `x`'s static type where that settles it (`isinstance(True, int)`),
/// a member test on a concrete union or an optional, and a class test for a
/// base-class value. The branch a test proves reads `x` as the narrowed
/// member (`if`, `elif` chains, `and`, `not`, and the rest of the block after
/// a branch that returns or continues). Requirements: `CPython`'s output, and
/// a narrowed read is the concrete member (`SmeltPyInt` out of the union
/// enum), never an erased value.
#[test]
fn isinstance_narrowing() -> TestResult {
    verify_python_parity("isinstance_narrowing")
}
