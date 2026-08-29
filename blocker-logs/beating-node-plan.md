# What stands between the generated Rust and beating Node

Written by reading each benchmarked es-toolkit function's TypeScript, writing out what a
team hand-porting that function to Rust would produce, and diffing that against what
Smelt actually emits. Counts are grepped over the generated bench crate
(`target/compat-repos/es-toolkit/dist-bench/src`, 745 files).

Measured starting point, best of three, after the allocator change:

| case | TS ops/s | Rust ops/s | gap |
| --- | ---: | ---: | ---: |
| `unique` | 3,016 | 3,084 | **1.0x** |
| `chunk` | 25,559 | 10,254 | 2.5x |
| `flatten` | 10,532 | 2,915 | 3.6x |
| `count_by` | 3,556 | 587 | 6.1x |
| `group_by` | 2,870 | 455 | 6.3x |
| `partition` | 9,206 | 1,374 | 6.7x |
| `unique_by` | 6,427 | 807 | 8.0x |
| `sum_by` | 85,638 | 1,753 | **48.9x** |

## The representation question: is `Rc<RefCell<Vec<T>>>` a smell?

Yes, but not because the `RefCell` is wrong — because it is applied unconditionally.

A JavaScript array is mutable through every alias: `const b = a; b.push(x)` is visible
through `a`. `Rc<Vec<T>>` cannot express that, so for a genuinely shared-and-mutated
array `Rc<RefCell<Vec<T>>>` is the correct lowering and a hand-writing team would write
the same thing. The smell is that **every** list gets it, including the ones that are
never aliased and the ones that are never mutated after construction.

A team hand-porting this library would use three tiers:

| tier | Rust | allocations | clone cost | when |
| --- | --- | ---: | --- | --- |
| 1 | `Vec<T>` | 1 | deep copy | never aliased — a local accumulator |
| 2 | `Rc<[T]>` | **1** (header and elements in one block) | refcount bump | built once, then only read |
| 3 | `Rc<RefCell<Vec<T>>>` | 2 (`RcBox` + `Vec` buffer) | refcount bump | genuinely shared *and* mutated |

Smelt emits tier 3 for everything. `chunk` is the clean example: it builds 1,429
seven-element output arrays per op, each of which is written once by `arr.slice(..)` and
never mutated again. That is 1,429 avoidable `RcBox` allocations per op, plus a borrow
flag touched on every read. Tier 2 halves the allocation count and removes the flag.

Picking the tier is an escape/mutation analysis over MIR: does any alias of this list
escape the function, and is it ever mutated after construction? That is the single
largest structural item on this list, and it is the one that is genuinely *about* the
north star — it is what "would a hand-writing team have written this?" answers.

`SmeltObject` is worse than a list: `{ id, values: Rc<RefCell<..>>, order: Rc<RefCell<..>> }`
is **four** heap allocations per JavaScript object.

## Waste that is visible in the emitted text

Each of these is a general rule, not a per-library fix, and each is counted over the
whole generated corpus.

### 1. Clone-then-borrow at `&T` argument positions — 178 sites

    result.contains_key(&key.clone())
    seen.remove(&value.clone())

`&x` would do. The callee takes `&T` and only reads it. This is the same rule the
callback ABI already enforces (`callback_param_is_shared_reference`), not yet applied to
runtime-method arguments. By callee: `remove` 142, `get` 11, `js_strict_eq` 10,
`contains` 7, `contains_key` 4, `same_js_key` 2. Where the value is a string-valued
`SmeltUnknown`, each clone is a malloc plus a memcpy.

### 2. Three hash lookups where one would do — `groupBy`, `countBy`, `uniqBy`

The source shape is `if (!hasOwn(result, key)) result[key] = []; result[key].push(item)`.
Emitted, per element:

    result.contains_key(&key.clone())                       // lookup 1, key clone 1
    result.insert(key.clone(), <a fresh empty SmeltList>)    // lookup 2, key clone 2, 2 allocations
    result.entry_or_insert(key.clone(), SmeltList::new(..))  // lookup 3, key clone 3, 2 more allocations

A hand-writing team writes `result.entry(key).or_default().push(item)` — one lookup, one
key move, and the empty vector only materialises for a key that is actually new. The
existing `DictEntryInPlaceMutation` MIR pass already fuses a copy-out/mutate/copy-back
triple; this is the same shape of pass for the contains/insert/entry triple.

### 3. Eagerly constructed defaults — 122 `.cloned().unwrap_or(..)`, 21 `SmeltList::new(Vec::<..>)`

`unwrap_or` takes its argument by value, so the default is built on **every** call
whether or not it is used. `entry_or_insert(key, SmeltList::new(Vec::new()))` allocates an
`Rc` per element and throws it away whenever the key already exists. `unwrap_or_else` and
an `or_insert_with`-shaped `entry_or_insert` cost nothing and remove all of it.

### 4. `.len()` re-read through a `RefCell` borrow — 243 index normalizations

Every indexed read normalizes a possibly-negative JavaScript index and re-reads `.len()`
to do it, and the enclosing `for` loop's condition reads `.len()` again. In `sum_by`'s
loop that is two borrow round-trips per element for a length the loop already has.

### 5. An indexed read clones even when the consumer only borrows

    get_value(&items.borrow().get(idx).cloned().unwrap_or(Default::default()), i)

The callback parameter is `&T`. The `.cloned()` exists only to have something to point
at. Where the item is also stored (`partition` pushes it) the clone is real work; where
it is only inspected (`sum_by`, `countBy`) it is pure waste. 122 `.cloned().unwrap_or(`
sites corpus-wide.

### 6. Redundant round trips — 53 sites

    Into::<SmeltList<_>>::into(SmeltList::from({ let items: Vec<T> = vec![]; items }))

`SmeltList::from` already produced a `SmeltList`; the `Into` is a no-op the optimiser has
to see through, and the intermediate `Vec` binding is emitted for a literal empty list.

## `sum_by` at 48.9x — the outlier

`sumBy` is nine lines of TypeScript: a loop, an index read, a callback, an add. There is
nothing in it for the emitter to get structurally wrong, which is why the ratio is so
stark — it prices the *per-element overhead* with almost no real work to hide behind.
Per element the generated loop pays: two `.len()` borrows (items 4), one indexed read
that clones an element the callback only borrows (item 5), one eagerly built default
(item 3), and then, inside the callback, a `SmeltObject::get` that hashes a constant
string and clones the value out of the map. V8 does the same loop in about four
instructions per element.

Fixing items 3, 4 and 5 lands directly on this row.

## Ordering

1. `SmeltObject::get` — 18.96% of `partition`, the single largest cost centre.
2. `SmeltUnknown::String` sharing — makes every string clone a refcount bump.
3. Items 1, 3, 4, 5, 6 above — small, general, and they compound.
4. The list representation tiering — the largest, and the one that needs a real analysis.
