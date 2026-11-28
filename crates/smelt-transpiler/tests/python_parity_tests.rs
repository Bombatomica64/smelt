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

/// A literal `None` argument to a parameter that admits `None` (`x: int |
/// None`, defaulted or not, positional or keyword) is that parameter's absent
/// value, and `x is None` / `x is not None` narrow a single-binding optional
/// local to its payload: in `if`/`elif`/`else` branches, after a branch that
/// returns, in a conditional expression, and in the later operands of `and`
/// / `or`. Also pins two shapes that used to be refused (a list literal to a
/// union parameter, an `int` to an `int | float` parameter keeping its `int`
/// text). Requirements: `CPython`'s output; the narrowed read is the
/// payload type (`SmeltPyInt`, `SmeltList<SmeltPyInt>`), never erased.
#[test]
fn optional_none_argument() -> TestResult {
    verify_python_parity("optional_none_argument")
}

/// `min(xs, key=f)` / `max(xs, key=f)` with a named function, a lambda, a
/// builtin (`len`, `abs`, `sum`) or `key=None`: the key runs once per item in
/// list order, the first of equal keys wins, a NaN key compares as `CPython`
/// compares it, an empty list raises `ValueError`, and a raising key
/// propagates. Builtins also work as `sorted`/`map` callbacks, including over
/// list items (borrowed by the callback ABI). Requirements: `CPython`'s
/// output, and the result keeps the item type (no erased value).
#[test]
fn extrema_key() -> TestResult {
    verify_python_parity("extrema_key")
}

/// Class variables (`count = 0` in a class body): read through the class, an
/// instance or `cls`; stored through the class or `cls` (`+=`, `*=`, `=`) as
/// one value every instance sees; inherited reads through a subclass; and an
/// instance attribute bound in `__init__` shadowing a class variable of the
/// same name while `Class.name` keeps the class value. Requirements:
/// `CPython`'s output; a stored class variable is typed storage of its
/// literal's type (`SmeltPyInt`, `f64`, `String`), an unstored one a constant.
#[test]
fn class_variables() -> TestResult {
    verify_python_parity("class_variables")
}

/// Python truth tests: `if`, `elif`, `while`, `assert`, `not`, conditional
/// expressions, comprehension `if`s, `bool(x)` and `filter(None, xs)` read an
/// empty `list`/`dict`/`set`/`str`, `0`, `0.0` and `None` as false, an
/// `Optional` container as present-and-non-empty, and a class through its
/// `__bool__`, else `__len__`, else true. Requirements: `CPython`'s output;
/// the test is a `bool` over the typed value (`len != 0`, the payload's own
/// test), never JavaScript truthiness, which makes every object true.
#[test]
fn python_truth_tests() -> TestResult {
    verify_python_parity("python_truth_tests")
}

/// `a or b` / `a and b` return an operand (`b if a else a` for `and`), the
/// left one evaluated once and the right one only when it is the result, and
/// `opt or default` over `T | None` and `T` is a `T`. Requirements: `CPython`'s
/// output and evaluation order; the result keeps the operands' type.
#[test]
fn value_returning_and_or() -> TestResult {
    verify_python_parity("value_returning_and_or")
}

/// `"...".format(...)` with automatic, numbered and keyword fields,
/// conversions and format specs, every argument evaluated once in source
/// order; `hex`/`bin`/`oct`/`format`; and unbound builtin methods as
/// callbacks (`key=str.lower`, `map(str.upper, xs)`). Requirements:
/// `CPython`'s output.
#[test]
fn str_format_and_radix() -> TestResult {
    verify_python_parity("str_format_and_radix")
}

/// Starred assignment targets (`first, *rest`, `*init, last`, `a, *mid, z`,
/// an empty star, and the `ValueError` of too few items), `enumerate(xs,
/// start)`, and tuple sort keys (`key=lambda p: (p.a, -p.b)`, with
/// `reverse=True`). Requirements: `CPython`'s output; the starred target is a
/// list of the value's item type.
#[test]
fn starred_targets_enumerate_sort_keys() -> TestResult {
    verify_python_parity("starred_targets_enumerate_sort_keys")
}

/// An `ABC` with `@abstractmethod`s (stub and docstring bodies) called through
/// `list[Shape]` dispatches to each subclass's override, and
/// `type(x).__name__` / `x.__class__.__name__` name the runtime class (a
/// subclass through a base-typed value, the builtin types by name).
/// Requirements: `CPython`'s output.
#[test]
fn abstract_classes_and_type_names() -> TestResult {
    verify_python_parity("abstract_classes_and_type_names")
}

/// Dataclass `field(default_factory=f)` runs `f()` for every instance that
/// leaves the field out (no sharing), `field(default=v)` is an ordinary
/// default, and `==` compares a dataclass's fields even when it is a
/// reference class (mutated through an alias). Requirements: `CPython`'s
/// output.
#[test]
fn dataclass_field_defaults() -> TestResult {
    verify_python_parity("dataclass_field_defaults")
}

/// User exception classes with fields, methods and `__str__` overrides
/// (`docs/design/python-exceptions.md`): `except C as e` binds the typed
/// instance (`e.status`, `e.retryable()`), a base clause catches a subclass
/// and keeps its class, a re-raise keeps the same instance, `args` follows
/// `CPython`'s two writers (`BaseException.__new__` and `__init__`), and an
/// untyped `except Exception` shows the class's own `str`/`repr`.
/// Requirements: `CPython`'s output.
#[test]
fn exception_fields() -> TestResult {
    verify_python_parity("exception_fields")
}

/// A method or constructor parameter's default is the callee's: an omitted
/// argument takes it (silent-wrong before: the type's zero value), and a
/// mutable default is shared by every call omitting it. Requirements:
/// `CPython`'s output.
#[test]
fn method_default_params() -> TestResult {
    verify_python_parity("method_default_params")
}

/// One fallibility per override family: a raising base method with a
/// non-raising override (and the reverse, and an ABC's stub with a concrete
/// override) called from an inherited method compiles for every class and
/// raises exactly where `CPython` does (E0277 before: `?` on a non-`Result`).
/// Requirements: `CPython`'s output.
#[test]
fn override_fallibility() -> TestResult {
    verify_python_parity("override_fallibility")
}

/// A class named like a runtime tuple impl's type parameter (`A`..`F`) or a
/// prelude type compiles (E0530 before: the impls bound tuple elements to
/// patterns spelled like the class), and `type(x).__name__` / `isinstance`
/// four levels deep name the runtime class (E0034 before: a leaf's cell
/// implements several ancestors' `smelt_class_name`). Requirements:
/// `CPython`'s output.
#[test]
fn short_class_names() -> TestResult {
    verify_python_parity("short_class_names")
}

/// `str.partition`/`rpartition` (an empty separator raises `ValueError`),
/// `splitlines` at every `CPython` line boundary, `expandtabs`, `zip` and
/// `enumerate` over a `str`, and f-string specs with nested replacement
/// fields (`{x:{w}.{p}f}`, evaluated after the value). Requirements:
/// `CPython`'s output.
#[test]
fn str_sweep() -> TestResult {
    verify_python_parity("str_sweep")
}

/// Closures late-bind: a lambda or nested `def` made in a loop reads the loop
/// variable's last value (silent-wrong before: each kept its iteration's
/// value), while a defaulted parameter (`lambda x, i=i:`) freezes the value
/// the default had when the lambda was made. Requirements: `CPython`'s output.
#[test]
fn closure_binding() -> TestResult {
    verify_python_parity("closure_binding")
}

/// `match` guards, captures, `|` and `as` patterns: cases tried in order,
/// each guard evaluated only when its pattern matched, a capture bound before
/// its guard (and kept when the guard fails). Requirements: `CPython`'s
/// output.
#[test]
fn match_guards() -> TestResult {
    verify_python_parity("match_guards")
}

/// Generator functions (`Iterator[T]`, `Generator[T, None, None]`): a `for`
/// loop resumes the generator once per iteration, so their effects
/// interleave; `break` leaves the rest unproduced; `list`/`sum`/`sorted`/
/// `max` drain one; a raise inside reaches the consumer's `try`.
/// `any`/`all` over a generator expression stop at the first deciding
/// element (silent-wrong before: every element was computed). Requirements:
/// `CPython`'s output.
#[test]
fn generators() -> TestResult {
    verify_python_parity("generators")
}

/// A container whose element type is a union of classes (`[A(1), B()]`,
/// nested lists of them, dict values) prints each element's `__repr__`, as
/// `CPython` does (Rust `Debug` of the erased carrier before); `repr(str)`
/// escapes exactly what `str.isprintable()` rejects (`\xa0`, `​`,
/// `\U0010ffff`), printable non-ASCII text stays itself. Requirements:
/// `CPython`'s output; the union keeps its concrete enum.
#[test]
fn union_repr_and_escapes() -> TestResult {
    verify_python_parity("union_repr_and_escapes")
}

/// `isinstance` against a class the module defines further down, over its
/// subclass chain; a subclass instance passed to a parameter of its base; a
/// lambda called where it is written (`(lambda x: x + 1)(2)`), and an
/// unannotated `name = lambda ...` typed by each of its calls, reading
/// enclosing variables when called (late binding). Requirements: `CPython`'s
/// output.
#[test]
fn forward_classes_and_lambdas() -> TestResult {
    verify_python_parity("forward_classes_and_lambdas")
}

/// The generator protocol: `send()`, `return value` (`StopIteration`'s
/// `args`, `yield from`'s result), `yield from` of a generator, a list and a
/// `str`, `next(g[, default])`, and `zip`/`enumerate`/`any`/`all` resuming a
/// generator object one value at a time (refused before). Requirements:
/// `CPython`'s output, the interleaving of producer and consumer prints
/// included.
#[test]
fn generator_protocol() -> TestResult {
    verify_python_parity("generator_protocol")
}

/// Operations on a union value dispatch on the member it holds: arithmetic,
/// comparisons, `abs`, `round` and unary `-` on `int | float` (refused
/// before), `isinstance`/`is None` on `int | str | None` (an erased union
/// before, now `Option<union>`). Requirements: `CPython`'s output, with
/// `int` results staying exact.
#[test]
fn numeric_unions() -> TestResult {
    verify_python_parity("numeric_unions")
}

/// A function or method body constructs, annotates, calls and reads the
/// members of classes and functions written after it (refused before: bodies
/// lowered in source order), mutual recursion between module functions, and
/// string annotations spelling an expression (`"Entry | None"`). Requirements:
/// `CPython`'s output, with every forward-referenced value at its concrete
/// class type.
#[test]
fn forward_references() -> TestResult {
    verify_python_parity("forward_references")
}

/// `g.throw(exc)` raises at the suspended `yield` (or ends a just-started
/// generator), `g.close()` runs `finally` blocks and `except GeneratorExit`
/// handlers and refuses a generator that yields again, `send()` of a
/// non-`None` value to a just-started generator raises `TypeError`, and
/// `except StopIteration as e` reads `e.value` (all refused before).
/// Requirements: `CPython`'s output, every handler and `finally` print
/// included, in order.
#[test]
fn generator_throw_close() -> TestResult {
    verify_python_parity("generator_throw_close")
}

/// `super()` chains through three and four levels of single inheritance,
/// from overriding and from inherited bodies, `super().__str__()` included
/// (a three-level chain recursed forever before: an inherited body's
/// `super()` alias resolved to itself). Requirements: `CPython`'s output,
/// each call reaching the definition after its own class in the MRO.
#[test]
fn super_chains() -> TestResult {
    verify_python_parity("super_chains")
}

/// An `is None` test on `self.field` narrows the field (refused before), an
/// `isinstance` test on a base-class value narrows it to the subclass as a
/// checked downcast of the same object (a miscompile or refusal before), a
/// conditional expression with branches of different types is their union,
/// a list of sibling classes dispatches an inherited method, and a user
/// method spelled like a builtin one (`find`, `items`, `count`) is the
/// user's. Requirements: `CPython`'s output, identity of the narrowed object
/// included.
#[test]
fn narrowing_and_dispatch() -> TestResult {
    verify_python_parity("narrowing_and_dispatch")
}
