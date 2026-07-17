# Generated Rust Test Report

> Probe note: Radash declares Chai's test globals ambiently. To refresh the
> previously generated test crate with the current frontend, this run added a
> temporary `vitest` import for `describe`/`test` to the probe source and removed
> it immediately after generation; the third-party source is unchanged.

- Cargo manifest: `/home/lollo/Playground/smelt/target/library-probes/radash/dist-smelt/Cargo.toml`
- Focused runs: `0`
- Guard runs: `0`
- Full suite executed: `true`

## Full Suite

- Status: `failed`
- Result: `no test-result line`
- Failing tests: `0`

### Largest Failing Groups

| Failures | Test group |
| ---: | --- |

### Delta From Baseline

- Baseline report: `blocker-logs/radash-dict-field-write.md`
- Resolved tests: `0`
- Newly failing tests: `0`

<details>
<summary>Failing test inventory</summary>


</details>

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `/home/lollo/Playground/smelt/target/library-probes/radash/dist-smelt/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `103`

## Summary By Code

1. **warning** `unused_mut` - 60 diagnostics
2. **warning** `unused_parens` - 21 diagnostics
3. **warning** `unused_assignments` - 12 diagnostics
4. **warning** `unreachable_code` - 8 diagnostics
5. **warning** `noop_method_call` - 2 diagnostics

## Groups

1. **warning** `unused_mut` - 60 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/async_.rs:86`
     - `src/async_.rs:142`
     - `src/array.rs:463`
     - `src/array.rs:509`
     - `src/array.rs:688`
2. **warning** `unused_parens` - 20 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/array.rs:92`
     - `src/array.rs:139`
     - `src/array.rs:168`
     - `src/array.rs:357`
     - `src/array.rs:539`
3. **warning** `unreachable_code` - 8 occurrences
   - Message: unreachable expression
   - Examples:
     - `src/async_.rs:444`
     - `src/async_test.rs:503`
     - `src/async_test.rs:540`
     - `src/async_test.rs:578`
     - `src/async_test.rs:682`
4. **warning** `noop_method_call` - 2 occurrences
   - Message: call to `.clone()` on a reference in this situation does nothing
   - Examples:
     - `src/array.rs:900`
     - `src/array.rs:910`
5. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `fn_` is never read
   - Examples:
     - `src/async_.rs:342`
     - `src/async_test.rs:685`
6. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `result` is never read
   - Examples:
     - `src/array_test.rs:517`
     - `src/array_test.rs:442`
7. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `asc` is never read
   - Examples:
     - `src/array.rs:435`
8. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `composed` is never read
   - Examples:
     - `src/curry_test.rs:172`
9. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `decomposed` is never read
   - Examples:
     - `src/curry_test.rs:178`
10. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `dsc` is never read
   - Examples:
     - `src/array.rs:442`
11. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `func` is never read
   - Examples:
     - `src/async_test.rs:1348`
12. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `return_num` is never read
   - Examples:
     - `src/curry_test.rs:258`
13. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `s` is never read
   - Examples:
     - `src/curry_test.rs:1405`
14. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `x` is never read
   - Examples:
     - `src/curry_test.rs:1418`
15. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `match` scrutinee expression
   - Examples:
     - `src/array.rs:1116`

## Cargo Stderr

```text
Checking radash_probe v0.1.0 (/home/lollo/Playground/smelt/target/library-probes/radash/dist-smelt)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 10.10s
```
