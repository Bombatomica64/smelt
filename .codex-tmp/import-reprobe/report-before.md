# Remeda Consumer Import Probe

Date: 2026-05-26

## Purpose

Test Remeda from a separate Smelt entry file using normal imports, rather than
only running test modules generated from within the Remeda source tree.

## Probe Program

```ts
import { difference, flat } from "remeda";

const flattened = flat([[1, 2], [3]]);
const remaining = difference(flattened, [2]);

console.log(remaining[0]);
console.log(remaining[1]);
console.log(flat([[1], [2]], 0).length);
```

Expected output:

```text
1
3
2
```

## Ordinary Package Import Result

Import form:

```ts
import { difference, flat } from "remeda";
```

Commands:

```bash
cargo run --quiet --bin smelt -- \
  --manifest-path .codex-tmp/remeda-consumer-bare/Smelt.toml build
RUSTFLAGS='-Awarnings' cargo run --quiet \
  --manifest-path .codex-tmp/remeda-consumer-bare/dist/Cargo.toml
```

Build result: succeeded.

Actual output:

```text
null
null
0
```

Conclusion: ordinary `from "remeda"` consumption is not correct yet. The
generated entry module lowers both imported function values as empty erased
objects, so calls return `SmeltUnknown::Null` rather than invoking Remeda.

Generated Rust evidence:

```rust
let _smelt_tmp_2: SmeltRecord<String, SmeltUnknown> = SmeltRecord::from([]);
let _smelt_tmp_3: SmeltUnknown =
    SmeltUnknown::Object(SmeltObject::from_unknown_record((_smelt_tmp_2.clone()).clone()));
```

That value is subsequently used as the callable for `flat`; no generated
`flat` or `difference` module was linked into the consumer crate.

## Public Source Barrel Result

Import form:

```ts
import { difference, flat } from "../index";
```

Build result: generated Rust failed to compile.

Diagnostics summary:

```text
Cargo check: failed
Errors: 24
Primary error: E0425, missing smelt_set_timeout / smelt_clear_timeout
```

The detailed diagnostic report is in
`blocker-logs/remeda-consumer-barrel-import-diagnostics.md`.

Conclusion: importing Remeda's source barrel currently draws in modules such
as `debounce` and `funnel`, and the consumer crate does not emit the required
timer runtime helpers.

## Direct Module Control Result

Import form:

```ts
import { difference } from "../difference";
import { flat } from "../flat";
```

Commands:

```bash
cargo run --quiet --bin smelt -- \
  --manifest-path .codex-tmp/remeda-consumer-direct/Smelt.toml build
RUSTFLAGS='-Awarnings' cargo run --quiet \
  --manifest-path .codex-tmp/remeda-consumer-direct/dist/Cargo.toml
```

Actual output:

```text
1
3
2
```

Conclusion: the implementations used by this probe behave correctly once
they are linked directly. The blocking defects for normal library use are
package/public-entry resolution and public-barrel runtime emission, not this
simple `flat`/`difference` behavior.
