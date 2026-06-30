# Codegen Quality Assessment — Remeda corpus (1789 green tests)

Status: analysis only (no compiler edits). Baseline: `main` @ `127dd72c`.

This report grades the *idiomaticness* of the Rust that Smelt currently emits for the
fully-green Remeda corpus — i.e. beyond "does it compile and pass", how close is the
output to what a human porting Remeda to Rust by hand would write? It feeds a later
dedicated codegen-quality phase.

- Raw diagnostics: [`blocker-logs/remeda-codegen-quality.md`](../blocker-logs/remeda-codegen-quality.md)
- Cross-references: [`blocker-logs/remeda-after-move-on-last-use.md`](../blocker-logs/remeda-after-move-on-last-use.md),
  memory `mir-opt-move-on-last-use.md`, `move-on-last-use-unsound.md`.

## TL;DR

`cargo check` on the generated crate is **clean: 0 errors, 269 warnings**. The warning
profile is byte-for-byte identical to the post-`move-on-last-use` baseline, so nothing
regressed. But the 269 rustc warnings *drastically understate* the idiomaticness gap.
rustc only flags syntactic dead weight (`unused_mut`, `unused_parens`, `unused_unsafe`).
The real distance from hand-written Rust is in patterns rustc is happy with but clippy
and a human reviewer would reject:

| Signal | Count | rustc warns? |
| --- | ---: | :---: |
| `.clone()` calls | 23,016 | no |
| `.clone().clone()` double-clones | 1,402 | no (clippy territory) |
| `_smelt_tmp_N` temp references | 54,342 | no |
| fully-qualified `::std::rc::Rc::new` | 4,264 | no |
| parenthesized `( x as f64 )` casts | 472 | yes (`unused_parens`, partial) |
| `unsafe {` blocks | 44 | yes (`unused_unsafe`, when empty of unsafe ops) |
| total generated LOC | 78,937 | — |

That is roughly **one `.clone()` every 3.4 lines** and **~0.7 temp bindings per line**.
The output compiles and is correct; it reads like machine-lowered SSA, not hand Rust.

## Warning classes (rustc), biggest first

From `blocker-logs/remeda-codegen-quality.md`:

| # | Code | Count | Sub-message |
| --: | --- | --: | --- |
| 1 | `unused_mut` | 135 | variable does not need to be mutable |
| 2 | `unused_parens` | 65 | unnecessary parens around method argument |
| 3 | `unused_unsafe` | 31 | unnecessary `unsafe` block |
| 4 | `unused_parens` | 24 | unnecessary parens around function argument |
| 5 | `unused_assignments` | 9 (4+1+1+1+1+1) | value assigned to X never read |
| 6 | `unreachable_code` | 1 | unreachable statement |
| 7 | `unused_must_use` | 1 | unused `Result` |
| 8 | `unused_parens` | 3 | return value / assigned value / closure body |

`unused_parens` totals **92** across its sub-buckets, making it the #1 issue by code if
the method/function/return variants are merged.

---

## Top classes: pattern → idiomatic → fix difficulty

### 1. `unused_mut` (135) — unconditional `mut` on build-then-consume temps

Source of most hits is not parameters (only 42 `mut` params total) but the **2,228
`let mut _smelt_tmp` / `let mut assigned` / `let mut smelt_callback` temps** the emitter
produces for "build a value, then read it once" helper expansions.

```rust
// generated (addProp.rs:19, allPass.rs:16/24, filter.rs, ...)
let _smelt_tmp_5 = { let mut assigned = _smelt_tmp_3.clone(); assigned.extend(...); assigned };
// and the iterate helper:
let mut smelt_callback = ::std::rc::Rc::new(move |..| { .. });
... (smelt_callback)(item, ..) ...   // never reassigned
```

```rust
// idiomatic
let assigned = { let mut a = _smelt_tmp_3.clone(); a.extend(...); a };  // mut scoped to the build only
let smelt_callback = Rc::new(move |..| { .. });                        // no mut at all
```

Idiomatic Rust only marks a binding `mut` when it is actually reassigned/mutated *after*
construction. The emitter currently emits `mut` defensively.
**Difficulty: Low.** A MIR/emitter pass that tracks whether a local is ever assigned-to
after its initializer, and drops `mut` otherwise. The move-on-last-use pass already walks
locals; this is a sibling analysis. The risk is async/`&T` interactions already documented
in `mir-opt-move-on-last-use.md` — gate the same way.

### 2/4/8. `unused_parens` (92) — parenthesized casts and arguments

```rust
// generated (filter.rs:22, purryOrderRules, randomBigInt.rs:93)
predicate(closure_arg_0.clone(), (closure_arg_1.clone() as f64), closure_arg_2.clone());
return ((result as f64).trunc() as i64);
```

```rust
// idiomatic
predicate(closure_arg_0.clone(), closure_arg_1.clone() as f64, closure_arg_2.clone());
return (result as f64).trunc() as i64;
```

The emitter wraps every `as` cast (and some return values / closure bodies) in parens for
safety against precedence, but in argument and statement position the parens are pure
noise. There are **472** parenthesized `( … as f64 )` casts; only those in tight binding
positions actually need parens.
**Difficulty: Low.** Make the cast-emission helper precedence-aware: only parenthesize a
cast when the surrounding context binds tighter than `as` (e.g. method receiver, unary,
field access). In call-argument / statement / `return` position, omit. This is a localized
change in the expression printer.

### 3. `unused_unsafe` (31) — closures/bodies wrapped in `unsafe {}`

```rust
// generated (debounce.rs:77, and 24 other files; 44 unsafe blocks total)
move || {
    unsafe {
        let timeout_id: SmeltUnknown;
        let args: SmeltList<SmeltUnknown>;
        ...
    }
}
```

```rust
// idiomatic
move || {
    let timeout_id: SmeltUnknown;
    let args: SmeltList<SmeltUnknown>;
    ...
}
```

These blocks contain no `unsafe` operations — the wrapper appears to be a scaffolding
artifact (likely for hoisted/forward `let` declarations or closure-body framing). It is
both non-idiomatic and a latent foot-gun (suppresses future real unsafe-lints inside).
**Difficulty: Low–Medium.** Find why the body-emitter wraps these blocks in `unsafe`
(25 files, concentrated in `debounce`/`funnel`/timer-style closures) and emit a plain
block instead. If the `unsafe` was once needed for a transmute/raw-pointer path that has
since been removed, this is a dead artifact — Low. If it gates something subtle, Medium.

### 5. `unused_assignments` (9) — dead initial writes before unconditional overwrite

```rust
// generated (purryOrderRules.rs:66-67, stringToPath.rs:33, evolve_test, isShallowEqual)
let mut arg: SmeltUnknown = SmeltUnknown::Undefined;        // never read
let mut arg_removed: SmeltList<...> = ...::into(SmeltList::new(...));  // never read
if cond { arg = first...; arg_removed = ...; } else { ... }
// the `rest`/`arg`/`sum`/`value`/`match_` initial values are dead.
```

```rust
// idiomatic — let the branches initialize, or use the value form
let (arg, arg_removed) = if cond { (first, ...) } else { (..., ...) };
```

These come from lowering TS `let x = init; ... x = …` where the analysis can prove `init`
is dead. They co-occur with the `unused_mut` cases.
**Difficulty: Medium.** Proper fix is dead-store elimination on the MIR (drop an
assignment whose value is overwritten on every path before any read). A cheaper partial
fix: when a `let` is immediately and unconditionally reassigned, drop the initializer.
Full branch-merge into a value-position `if` is the most idiomatic but higher effort.

### 6. `unreachable_code` (1) — `sample.rs:112`

```rust
return _smelt_tmp_29;          // returns out of the function
}
return SmeltList::new(Vec::new());   // unreachable trailing return
```

A trailing fallback `return` emitted after a branch that already returns on all paths.
**Difficulty: Medium.** Needs MIR reachability to know the prior block diverges. Single
occurrence — low priority on its own, but falls out for free if dead-code/CFG cleanup lands.

### 7. `unused_must_use` (1) — `funnel_reference_batch_test.rs:159`

A `Result`-returning call whose result is dropped. One occurrence, test-only.
**Difficulty: Low** (emit `let _ =` or `?`/`.unwrap()` per context) **but very low value.**

---

## The bigger, non-rustc-visible idiomaticness gaps

These do not show up in the 269-warning count but dominate the reviewer's impression and
would be the bulk of a clippy run.

### A. Clone explosion (23,016 clones; 1,402 double-clones) — HIGHEST IMPACT

`.clone().clone()` (1,402×) is never justifiable — the inner clone produces an owned value
that the outer immediately re-clones and drops. Patterns like
`prop.clone().clone()`, `_smelt_tmp_5.clone()` on a value about to be moved, and
`match smelt_result.clone() { … }` (clone purely to match by value) are everywhere.

```rust
// generated
match prop.clone().clone() { SmeltUnknown::String(value) => value, ... }
SmeltUnknown::Object(SmeltObject::from_unknown_record((_smelt_tmp_5).clone()));
```

```rust
// idiomatic
match prop { SmeltUnknown::String(value) => value, ... }   // match by value/ref, no clone
SmeltUnknown::Object(SmeltObject::from_unknown_record(_smelt_tmp_5));  // _smelt_tmp_5 is dead here
```

`move-on-last-use` (memory: `mir-opt-move-on-last-use.md`) already attacks the
last-use case and drove Remeda E0382 to 0; the remaining clones are (a) defensive clones
on values that *are* at last use but weren't recognized, (b) clone-to-match, and
(c) the trivially-removable `.clone().clone()` chains.
**Difficulty: Low for the double-clone collapse** (peephole: `x.clone().clone()` →
`x.clone()`), **Medium-High for the general case** (extend move-on-last-use / add a
borrow-instead-of-clone pass for match scrutinees and immutable reads; same async `&T`
hazards apply).

### B. Temp-binding explosion (54,342 `_smelt_tmp_N`)

Every sub-expression is lowered to its own SSA temp (`let _smelt_tmp_4: bool = …;
_smelt_tmp_4.clone()`), even one-line bodies. ~162 functions literally end with
`return _smelt_tmp_N;` where the temp is the immediately-preceding single-use binding.

```rust
// generated
let _smelt_tmp_4: bool = (closure_arg_0)(data.clone());
_smelt_tmp_4.clone()
```

```rust
// idiomatic
(closure_arg_0)(data.clone())
```

**Difficulty: Medium.** A temp-inlining / copy-propagation pass on MIR: a temp that is
assigned once and read once, with no intervening effects, folds into its single use. This
single pass would also erase most of the `unused_mut`, `.clone().clone()`, and
`return _smelt_tmp` noise simultaneously, because those temps disappear. High leverage.

### C. Fully-qualified paths (4,264 `::std::rc::Rc::new`)

```rust
let _smelt_tmp_2 = ::std::rc::Rc::new(move |..| { .. });
```

```rust
use std::rc::Rc;   // once per file
let _smelt_tmp_2 = Rc::new(move |..| { .. });
```

**Difficulty: Low.** Emit `use` statements at file top for the handful of hot runtime
types (`Rc`, `RefCell`, `Cell`) and print the short name. Pure readability; no semantics.

---

## Prioritized cleanup roadmap (gain-per-effort)

Ordered by idiomatic-output gain divided by effort. The top three are the recommended
focus for the codegen-quality phase.

1. **Temp inlining / copy-propagation pass (B).** *Medium effort, very high gain.*
   Folding write-once/read-once temps into their use cascades: it removes the bulk of the
   54k temps, eliminates ~all `return _smelt_tmp` noise, drops the `let mut smelt_callback`
   class of `unused_mut`, and collapses many `.clone().clone()` chains. Single MIR pass
   with the biggest readability payoff. Reuse the dataflow infra from move-on-last-use.

2. **`mut`-only-when-mutated analysis (class 1, 135 warnings) + `.clone().clone()`
   peephole (A, 1,402 hits).** *Low effort, high gain.* Two small, independent passes that
   together clear the single largest rustc warning class and the most obviously-wrong clone
   pattern. Low risk; gate `mut`-dropping behind the same async/`&T` exclusion already
   documented for move-on-last-use.

3. **Precedence-aware cast/paren printing (classes 2/4/8, 92 warnings).** *Low effort,
   medium gain.* Localized change in the expression printer; clears the entire
   `unused_parens` family and the 472 noisy `( … as f64 )` casts. No dataflow needed.

4. **`use` imports for hot runtime types (C, 4,264 paths).** *Low effort, medium-cosmetic
   gain.* Independent, mechanical, zero-semantics. Good warm-up / parallel task.

5. **Drop the spurious `unsafe {}` wrapper (class 3, 31 warnings).** *Low–Medium effort.*
   Investigate the timer/closure body-emitter; likely a dead scaffolding artifact. Also
   removes a latent lint-suppression hazard.

6. **Dead-store elimination (class 5, 9 warnings; + `unreachable_code`, 1).** *Medium
   effort, low-but-correctness-adjacent gain.* A proper MIR DSE clears these and falls out
   naturally once the CFG analysis for (1) exists.

7. **Broader borrow-instead-of-clone for match scrutinees & immutable reads (A, general
   case).** *High effort, high gain but highest risk.* Defer until (1) lands — temp
   inlining will change the clone landscape, so re-measure first. Same async-`&T` E0308
   hazards as move-on-last-use.

### Sequencing note

Passes (1) and (7) interact: do **temp inlining first**, then **re-run diagnostics**, then
scope the remaining clone work against the reduced output. (2)/(3)/(4) are independent and
can land in parallel. Re-generate and re-run `rust-diagnostics` after each landing — the
corpus regen + compile workflow is in `mir-opt-move-on-last-use.md`.

## Verification baseline

```
cargo check on third_party/remeda/dist-smelt: passed
errors: 0   warnings: 269   (identical to remeda-after-move-on-last-use baseline)
```
