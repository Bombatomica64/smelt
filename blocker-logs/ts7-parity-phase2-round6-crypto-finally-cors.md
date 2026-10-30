# TS7 parity, phase 2, round 6: WebCrypto keys, `finally`, JSON layout, CORS

Branch `feat/hono-round6`, stacked on `feat/hono-url-model` (PR #286).

Hono v5 `0f5ad7b6`: **446 passed / 8 failed** of 454. Round 5 ended at
421 / 33. No test that passed before fails. The four failure families a
build-free research pass traced (pretty-json, timeout, CORS, compose) and the
signed-cookie family are below. Example 181 covers the new rules; its output
matches Node and it holds zero avoidable `SmeltUnknown`.

## Signed cookies (14 tests)

- **`crypto.subtle.importKey`/`sign`/`verify`.**
  - `raw` HMAC keys over SHA-1/256/384/512, with the `hmac` crate.
  - A `CryptoKey` is the concrete `SmeltCryptoKey` (a new `StdlibClass`), with
    boundary adapters and an `instanceof` marker.
  - `verify` compares in constant time.
  - Other formats and algorithms throw the spec's branded `NotSupportedError`.
  - The `importKey` algorithm object is reduced to name and hash strings in
    the frontend.
  - The `CRYPTO_KEY_REASON` blocker for these three members is lifted.
- **`String.fromCharCode(...codes)`.** One spread of a code list, a typed
  array (through its elements) or an erased value (read as a number list).
  Before, a spread declined to an empty string.
- **`Date.prototype.toUTCString`.** The `IMF-fixdate` form, a new
  `DateToUtcString` node; `toGMTString` is an alias.
- **`BufferSource` / `ArrayBufferView`.** These lower to the unions of the byte
  classes they name, which Smelt reads through the live arm. Before, they
  were opaque classes.
- **Conditional joins.** A concrete stdlib class arm no longer joins to
  `String`: `typeof s === 'string' ? encode(s) : s` was typed `String`.
- **Programs with only a `Date`.** A crate whose only erased value is a `Date`
  emits the `SmeltUnknown` carrier (`classes::class_name_renders_erased`,
  shared with the emitter).
- **A user function named `main`.** It no longer collides with the binary
  entry point. It renames to `main_` in MIR and in `sanitize_ident`. The
  module file for `main.ts` stays `source_main.rs`.

## Pretty JSON (7 tests)

- **`JSON.stringify(value, replacer, space)`.** `space` lays the compact text
  out with JavaScript's indentation, through a new `JsonIndent` node and the
  `smelt_json_indent` text re-indenter: a number is clamped to 10, a string
  cut to 10 characters. Arguments are now evaluated in source order (the
  value first). A replacer function is still not modeled.
- **`clone()` of a used body.** The clone is itself used, so reading it throws
  `TypeError` and `try { await res.clone().json() } catch {}` takes the catch.
  The spec throws at `clone()` itself; here the throw comes at the first read.

## `finally` and exceptions (general)

- **`finally` runs on the exceptional exit.** Before, it ran only on the
  normal and `return` exits, so `try { await race } finally { clearTimeout(t)
  }` skipped the cleanup on a rejection.
  - A `try` with a `finally` installs a handler around the `try` body (when
    there is no `catch`) and around the `catch` body. The handler stores the
    thrown value, raises a `pending_throw` flag and joins the one clause block
    the normal exit uses. The clause's end rethrows when the flag is up, into
    an enclosing `try`'s handler or out of the function.
  - The clause is emitted once. A first version re-lowered it in the handler,
    which duplicated its statements (es-toolkit's `isEqualWith` gained 5
    erased calls).
  - The thrown value is a genuine dynamic boundary: any value can be thrown,
    and example 181 rethrows through one clause on two paths.
    It is held in a local named `smelt_pending_thrown`, which only a crate
    with a `finally` gets. The SmeltUnknown report classifies that local with
    the exception-payload adapters (`smelt_throw`, `smelt_thrown_value`), and
    `pending_thrown_value_is_a_boundary` tests the classification.
  - A function with a `try`/`finally` and no `catch` now returns `Result`:
    the rethrow is an explicit throw out of it.
  - MIR validation now treats the exception edge as defining the handler's
    local. The emitter marks the local `mut` when a statement also writes it.
- **`'m' in value` on an erased class instance.** It sees the methods stored
  as `__smelt_method:`/`__smelt_proto:` entries.
- **Dotted reads off an erased record.** A read that misses falls back to
  those prototype entries (`smelt_record_inherited`), so `err.getResponse()`
  after `'getResponse' in err` works.
- **Thrown class instances keep their identity.** A class whose instances are
  thrown is a reference class, so `expect(context.error).toBe(error)` holds
  across erasure (compose, 1 test).

## CORS (3 tests)

`origin.includes(o)` over an untyped IIFE parameter that `typeof` checks
narrowed to `string[]` was lowered as `String.prototype.includes` over
`"[object Object]"`.

- When the TS7 checker types the receiver as an array, the string lowering
  declines (`checker_types_as_array`).
- The compact callback IR's nodes carry no source span, so its classifier
  hands such an arrow to closure-body lowering.
- The call then dispatches `includes` on the erased value at run time.

## Still failing (8)

- **Timeout (3).**
  - Calling `next()` primes the downstream handler eagerly. A primed caller
    that awaits it re-polls it within the same synchronous prime, which lets
    its sleep advance the virtual clock past the `Promise.race` timeout
    armed before it. `race` then picks the handler (status 200, not 504).
  - Making the sleep wait out the whole prime (`while` instead of one yield)
    did not fix it and made `body.test`'s nesting-limit test hang, so it was
    reverted.
  - The fix belongs in how priming and the race driver share the clock.
- **Mount (5).** Not investigated this round. See round 4 for their shapes.

## Typedness: closure calls are counted as calls

Inside a guarded `try`, a call through a closure value (`a.entries()` on an
erased `a`) is lowered as a `Call` terminator with an unwind edge. Outside
one, it is a `closure_call` statement, which the typedness report did not
count. Now that a `finally` also guards its body, those calls moved from the
uncounted form to the counted one. The metric showed that as 5 new erased
calls in `isEqualWith` and 1 in `isMatchWith`, though the calls were just as
erased before.

The report now counts `ClosureCall`/`ClosureCallSpread` statements as calls,
so a call cannot appear in or vanish from the metric when lowering picks a
form. To separate the methodology change from the code change, both the
pre-round-6 compiler (`feat/hono-url-model` plus the counting patch) and this
branch were measured with the new counting:

- es-toolkit: the only differences are +1 erased value in `limitAsync`,
  `isMatchWith` and `isEqualWith`, each the `finally`'s thrown value above.
- Examples: no program is less typed.

The baselines are re-snapshotted from this branch's measurement, with that
justification.

## Ratchets

- es-toolkit avoidable `SmeltUnknown`: 22183 -> 22157 (-26), re-snapshotted.
  An intermediate build of this round (the clause duplicated in the handler)
  measured 22092. The shared clause respells many lines of functions with a
  `finally`, since they now return `Result` and propagate with `?`. That
  respelling accounts for the difference, though it is not attributed line
  by line.
- remeda (advisory) typedness re-snapshotted for the new call counting: its
  erased calls rise in files whose callbacks are called through
  `closure_call`, which the metric now counts.
- Examples avoidable `SmeltUnknown`: 0.
- es-toolkit tests: see the commit message.
