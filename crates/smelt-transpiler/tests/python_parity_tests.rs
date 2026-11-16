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
