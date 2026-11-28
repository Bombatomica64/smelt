# Python `collections` containers and `typing.NamedTuple`

## What a hand port would write

| Python | Generated Rust |
| --- | --- |
| `deque[T]` | `SmeltPyDeque<T>`: a shared `VecDeque<T>` plus an optional bound |
| `Counter[K]` | `SmeltPyCounter<K>`: a shared `IndexMap<K, SmeltPyInt>` |
| `defaultdict[K, V]` | `SmeltPyDefaultDict<K, V>`: a shared `IndexMap<K, V>` plus the stored factory |
| `OrderedDict[K, V]` | `SmeltPyOrderedDict<K, V>`: a shared `IndexMap<K, V>` |
| `class P(NamedTuple)` | a struct with one typed field per position (the frozen-dataclass lowering) |

Every element, key and value type is the one the source proves, so none of these values is
erased. The containers have Python's reference semantics (`b = a; b.append(x)` changes `a`), so
each is an `Rc<RefCell<..>>` handle, as `SmeltList` and `SmeltPrimSet` are; `.copy()`, `c1 + c2`
and the like build a new store.

## Layers

* **Frontend** (`crates/smelt-frontend-py/src/lowering/stdlib_py_collections.rs`,
  `named_tuple.rs`): resolves the classes by binding (`collections.deque`, `from collections import
  Counter`, `typing.Deque`, ...), types every operand against the container's type arguments, and
  lowers each operation to `PyCollectionNew` / `PyCollectionOp`. Iteration, `len`, `in`, truth,
  `==`, `print`/`repr`, subscripts, `del` and augmented assignment all go through the same table.
* **HIR/MIR** (`crates/smelt-hir/src/expr/py_collection_ops.rs`, two op-table entries): a variant
  names the runtime method and its calling convention (which operand is borrowed, whether it can
  raise, whether it changes the container).
* **Stdlib registry** (`StdlibClass::PyDeque`, ...): the reserved class names
  (`__SmeltPyDeque`, ...) the backend recognizes.
* **Backend** (`crates/smelt-codegen-rust/src/emitter/py_collections.rs`): one method call per op.
* **Runtime** (`crates/smelt-runtime/src/runtime/py_collections.rs`): the four types, with
  `CPython`'s error messages and `repr`s, behind the `py_collections` gate.

## Requirements the parity fixture pins (`collections_basics.py`)

* a `defaultdict` read of a missing key inserts and returns the factory's value; `.get` and `in`
  never insert;
* a `Counter` read of a missing key is `0` and does not insert; `most_common`, `repr` and
  `elements` use `CPython`'s orders (count descending, ties in insertion order);
* a bounded `deque` drops from the opposite end; `rotate` follows the sign convention;
* `OrderedDict.move_to_end`/`popitem(last=)` act on either end; equality between two is
  order-sensitive;
* a named tuple's `p[i]` (constant `i`), unpacking, `len`, `_replace` and `_asdict` answer what the
  tuple answers, each position keeping its declared type.

## Refused, with a diagnostic

* a container built without element types from its context (`q = deque()`): annotate the target;
* a container annotation without type arguments (`x: deque = ...`);
* `collections.namedtuple(...)`, whose fields have no types: declare a `typing.NamedTuple` class;
* a `defaultdict` factory that can raise, a deque slice, `del d[i]` on a deque, a named tuple
  indexed by a non-constant position (its positions have different types).

A `defaultdict` whose factory is a lambda or a function prints `<function <lambda>>` /
`<function f>` where `CPython` adds the function's address, which no port can reproduce.
