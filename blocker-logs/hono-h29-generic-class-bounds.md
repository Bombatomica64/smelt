# H29 — a generic generated class cannot use `#[derive]`

Round 13, item 4. **58 → 39 slice errors**, and all 12 `is not implemented for
`T`` reports are gone.

## The rule

`#[derive(Trait)]` on a generic struct generates an impl bounded by that ONE
trait and nothing more: `impl<T: Clone> Clone`, `impl<T: Default> Default`. That
is not enough the moment a field's type is another generated class, because
those carry the full generated bound set — `Clone + Default + IntoSmeltUnknown +
SmeltFromUnknown + 'static`, from `class_impl_generics_text`.

Three shapes in the hono slice:

```rust
struct SmartRouterInner<T> { _routers: SmeltList<Router<T>>, … }   // derive(Debug, Default)
struct NodeInner<T>        { _children: SmeltRecord<String, Node<T>>, … }
struct TrieRouter<T>       { root: Node<T> }                        // derive(Clone, Debug, Default, PartialEq)
```

`derive(Default)` on the first demanded `Router<T>: Default` with only
`T: Default` in scope — one error per derive per missing bound, 12 in total.

## What the fix is NOT

**Not bounds on the struct declaration.** A bounded declaration propagates to
every generated type that mentions it, starting with the reference handle
newtype whose field is `Rc<RefCell<Inner<T>>>`. That cascade is why the
concrete-union emitter keeps its `pub enum` declaration bare and spells out each
impl instead; this follows the same precedent.

**Not the full generated bound set on the impls either** — that was my first
attempt and the test suite caught it. `impl<T: Clone + Default + IntoSmeltUnknown
+ SmeltFromUnknown + 'static> Clone for Ok<T>` compiles, but then
`#[derive(Clone)] pub enum SmeltUnion9<T> { M0(Ok<T>), … }` can no longer satisfy
`Ok<T>: Clone` from its own `T: Clone`. Over-constraining an impl breaks its
consumers. The same attempt also UNDER-constrained `PartialEq`, whose
`self.value == other.value` body needs the `T: PartialEq` the derive supplied.

So the bound differs per trait and per class, and neither extreme is right.

## What it is

Each impl is spelled out with the requirement its own BODY has:

| impl | generics | where |
| --- | --- | --- |
| `Clone` | bare `<T>` | each field type: `Clone` |
| `PartialEq` | bare `<T>` | each field type: `PartialEq` |
| `Default` | `<T: Default>` (derive-equivalent) | each *delegating* field type: `Default` |
| `Debug` | bare `<T>` | — (the body names the struct and stops) |

```rust
impl<T> Clone for TrieRouter<T> where Node<T>: Clone { … }
impl<T> PartialEq for TrieRouter<T> where Node<T>: PartialEq { … }
impl<T: Default> Default for TrieRouter<T> where Node<T>: Default { … }
```

A field-by-field body needs exactly one thing: every field's type implements the
trait. Bounding the FIELD TYPES says that and only that — `Ok<T> { value: T }`
yields `T: Clone`, so the union deriving `Clone` over it is satisfied, while
`TrieRouter<T> { root: Node<T> }` yields `Node<T>: Clone`, a requirement of
`Node` that only `TrieRouter`'s use sites can discharge. Clauses naming no type
parameter are dropped: `where String: Clone` is noise, and if such a bound were
unsatisfied the body would say so directly.

`Default` needs two refinements. Only the *delegating* defaults are bounded:
most field defaults are spelled out and need nothing of their type — a callback
field gets a constructed no-op closure, and `Rc<dyn Fn(…)>` is not `Default`, so
naming every field type demanded something false. And it keeps the
derive-equivalent `<T: Default>` on top of the clause, because a default can
require the bound through an expression the field's TYPE does not mention: a
promise field defaulting to
`SmeltFuture::resolved(SmeltUnion9::M0(Default::default()))` needs `T: Default`
while the union renders without its argument. Since a derive would have imposed
that bound anyway, keeping it can never leave a consumer worse off.

## A latent bug this exposed

`emit_default_impl_for_storage_type` ran only for callback-bearing classes
before, and none of those were generic — so this was never reachable:

```rust
_routers: SmeltList::new(Vec::<Router<SmeltUnknown>>::new()),   // in Default for SmartRouterInner<T>
```

A list default annotates its element type, and the arm doing it reached for
`type_text_with_impl_trait`, which rebuilds a substitution from the CURRENT
FUNCTION's type parameters. There is no current function here —
`default_value_for_with_scoped_type_params` hosts itself on `functions[0]`
purely to have an emitter — so a class type parameter was not in scope and
erased to `SmeltUnknown` (E0308). `default_value_with_scoped_type_params` now
has its own `List` arm that spells the element type through the substitution it
was given.

## Coverage

`a_generic_class_spells_its_derived_impls_instead_of_deriving_them` in
`part_7_tests.rs`: asserts the class really is generic (the enabling condition),
that the derives are gone, that `Clone`/`PartialEq` are spelled out, and that
`Default` keeps `impl<T: Default>`. Verified to FAIL with the emitter change
reverted.

The suite is the real review here, and it did its job — it is what rejected the
full-bound-set version twice, at
`build_predeclares_union_generator_methods_across_barrel_cycle`.

## The slice after this

| | errors |
| --- | ---: |
| round 12 end | 58 |
| after the reference-record half | 53 |
| after the value-class half | 42 |
| after the list-default fix | **39** |

Remaining, by count — H29's own second half is now the largest single item:

| errors | shape |
| ---: | --- |
| 8 + 8 | a generic-`T` record rebuilt from an erased iterator (`SmeltRecord<String, SmeltList<T>>` from `Iterator<Item=(String, SmeltList<SmeltUnknown>)>`) |
| 5 + 1 | **H29's second half**: `into_smelt_unknown` not found for a tuple — a tuple has no erasure adapter |
| 3 | E0615 `match_` used as a field, not a method |
| 2 | `expected SmeltUnknown, found SmeltRecord<String, SmeltRegExp>` |
| 2 | `SmeltList<(T, …)>` vs `SmeltList<(SmeltUnknown, …)>` |
| 1 each | `Node<T>` has no `==`; a `Result` receiver needing `?`; `expected bool, found String`; `expected String, found (String, String, T)`; two `due to unsatisfied trait bounds` |

## Gates

cargo test 105 result lines / 0 failures / **2661 passed** (frontend 1069,
codegen 1026 + the new test); clippy `--lib` clean and `--all-targets` clean
outside `smelt-specialize`; end-to-end goldens byte-identical; examples
SmeltUnknown avoidable 0 (+0); es-toolkit compiles with 0 errors, generated tests
**1055/4**, ratchet +0 (32517); remeda 1789/0 with the advisory unchanged at the
merge's +3.
