# Python exceptions: exception classes with fields

Status: implemented. Replaces the refusal "exception class 'E': only a `pass`, `...` or docstring body is
supported (no fields or methods)". Code: `crates/smelt-frontend-py/src/lowering/exceptions.rs` (frontend),
`crates/smelt-runtime/src/runtime/py_exception.rs` (runtime), `crates/smelt-codegen-rust/src/emitter/py_exception.rs`
(emission).

## 0. The problem

Before this change every Python exception, builtin or user-defined, was one runtime value,
`SmeltPyException { classes, args }`: the class chain an `except` matches by name, and the `str`/`repr` of each
constructor argument. A user exception class was its chain and nothing else, so the idiomatic shape was refused:

```python
class HttpError(Exception):
    def __init__(self, status: int, url: str) -> None:
        super().__init__(f"{status} for {url}")
        self.status = status
        self.url = url

    def retryable(self) -> bool:
        return self.status >= 500

try:
    fetch()
except HttpError as e:
    if e.retryable():
        print("retry", e.url, e)
```

A Rust port written by hand gives `HttpError` a struct with a `status: i64` and a `url: String`, and recovers it
from the error channel by downcasting `Box<dyn Error>`. That is the bar: the fields keep their concrete types, and no
value crosses `SmeltUnknown`.

## 1. Representation

**A user exception class is an ordinary class.** It lowers through the same class lowering as any other Python
class: fields declared by `__init__` assignments or annotations, methods, properties, class variables, `super()`
calls, overriding, a polymorphic handle when it has subclasses (`crates/smelt-codegen-rust/src/class_polymorphism.rs`).
Only the builtin part of its ancestry is special:

- The topmost user class of a chain (its base is a builtin exception class, the *exception root*) has no HIR base.
  It gets what `BaseException` contributes, as real members:
  - the instance field `args`, typed as the runtime's `SmeltPyExceptionArgs` (`tuple[Any, ...]` has no static
    element type; the value only offers its `str`/`repr`, as before);
  - `__str__` and `__repr__` methods implementing `BaseException.__str__`/`__repr__`
    (`ExprKind::PyExceptionDefaultText`), unless the root defines its own. A subclass's `__str__` overrides them by
    the ordinary override rule, and `super().__str__()` in a subclass reaches them, as in CPython.
- Every exception class carries the protocol `ClassProtocol::PyException { builtin_base }`, naming the builtin
  class its root derives from. Codegen implements the runtime trait `SmeltPyExceptionObject` for the class's handle
  from it.

**The error channel is unchanged.** A raised exception is still one `SmeltPyException` crossing `Box<dyn Error>`, so
builtin exceptions, runtime errors (`IndexError` from a list read, a caught panic) and user classes are matched by one
`except` test. It gains the instance:

```rust
struct SmeltPyException {
    classes: &'static [&'static str],                     // own name first, then each base
    args: Rc<[SmeltPyExceptionArg]>,                      // builtin exceptions only
    instance: Option<Rc<dyn SmeltPyExceptionObject>>,     // a user class's instance
}
trait SmeltPyExceptionObject: Debug {
    fn smelt_any(&self) -> Rc<dyn Any>;                   // the instance's own cell, for the downcast
    fn py_classes(&self) -> &'static [&'static str];      // its RUNTIME class's chain
    fn py_args(&self) -> SmeltPyExceptionArgs;            // the live `args` field
    fn py_text(&self) -> String;                          // str(e): the class's `__str__`
    fn py_text_repr(&self) -> String;                     // repr(e): the class's `__repr__`
    fn py_default_str(&self) -> String { .. }            // BaseException.__str__ over py_args
    fn py_default_repr(&self) -> String { .. }           // Name(arg reprs)
}
```

The instance is shared (`Rc`), so `raise e` followed by `except E as caught` sees the same object, and a mutation
through either name is visible through the other. Exception classes are always reference classes
(`classify.rs`), so identity holds for leaf classes too.

## 2. Operations

| Source | HIR | Rust |
| --- | --- | --- |
| `raise E(a)` / `raise e` with `e: E` | `Throw(PyExceptionFromInstance { instance })` | `SmeltPyException::from_instance(Rc::new(e))` |
| `except E as e` (every named class a user class) | `let e: C = PyExceptionInstance { exception, class: C }`, `C` the nearest common class | checked downcast of `smelt_any()` over `C`'s concrete classes |
| `except Exception as e`, a tuple naming a builtin | `let e: SmeltPyException = caught` (as before) | unchanged |
| `super().__init__(x, y)` reaching `BaseException.__init__` | `self.args = PyExceptionArgsNew [x, y]` | `SmeltPyExceptionArgs::new(vec![arg(&x), arg(&y)])` |
| `str(e)`, `print(e)`, `repr(e)`, `[e]` with `e: E` | the class's `__str__`/`__repr__` (ordinary protocol methods) | `SmeltPyRepr` impl |
| `str(e)` with `e: SmeltPyException` | `PyStr` (as before) | the instance's `py_text()` when it has one |
| `e.args` with `e: E` | field read | `self.0.borrow().args` |

The `except` test keeps matching class names (`caught_by`): the instance's `py_classes()` is its runtime class's
chain, so `except Base` catches a `Leaf`, and clauses are tried in source order exactly as before. The downcast in
the binding cannot fail once the test matched.

## 3. `args`: CPython's two writers

CPython sets `args` twice: `BaseException.__new__` stores the constructor's positional arguments before `__init__`
runs, and `BaseException.__init__(*a)` (reached through `super().__init__(...)`) overwrites it with `a`. A class whose
`__init__` never calls `super().__init__` therefore still shows its constructor arguments in `str(e)`:

```python
class E(Exception):
    def __init__(self, code: int) -> None:
        self.code = code
print(E(5))        # 5
```

Both writers are modelled exactly, without evaluating anything twice:

- A class *keeps its constructor arguments* unless its effective `__init__` (its own, or the nearest typed
  ancestor's) calls `super().__init__(...)` as a top-level statement before any statement that mentions `args`, and
  that call reaches `BaseException.__init__`: no program class above defines `__init__`, or the parent's effective
  `__init__` recursively has the same property. A class with no `__init__` in its typed chain keeps them (that is
  `BaseException.__init__(*args)` itself).
- The constructor of a class that keeps its arguments takes a leading hidden parameter `__smelt_args`, and its
  `__init__` starts with `self.args = __smelt_args`. A call site packs its positional arguments into it
  (`PyExceptionArgsNew`), evaluating each argument once (bound to a temporary when it is also passed on to
  `__init__`). A constructor whose `args` is always overwritten takes no such parameter, so its arguments need no
  Python text: `ParseError(token)` with a `Token` that has no `__repr__` is accepted.
- `super().__init__(...)` towards a typed parent constructs the parent with the current `self.args` as its hidden
  argument when the parent keeps its arguments, then copies the parent's fields back (the ordinary Python
  super-init lowering, `statement.rs`), `args` included.

A class that keeps its arguments and is constructed with an argument that has no Python text is refused with a
message naming the argument: its `args` would have to show that value.

## 4. What stays refused

- `e.__cause__`, `e.__context__`, `e.__traceback__`, `with_traceback`, `add_note`: not modelled. `raise X from Y`
  evaluates `Y` (CPython evaluates it) and otherwise only sets `__cause__`, which only a traceback shows.
- `except E as e` reading a user attribute when `E` names a builtin class too (the binding is the untyped
  `SmeltPyException`, which only has `args`).
- An exception class nested in a function or another class, or with more than one base.
- Keyword arguments to a constructor whose class keeps its arguments without an `__init__` (CPython raises
  `TypeError`).

## 5. Evidence

`crates/smelt-transpiler/tests/fixtures/python-parity/exception_fields.py` exercises fields, methods, `__str__`
overrides, `super().__str__()`, the two `args` writers, subclass catch order, re-raise identity, tuple clauses mixing
user classes, and `raise ... from`; its stdout equals `python3`'s. Planted defects (each caught by that fixture, then
restored) are listed in the PR that landed this.
