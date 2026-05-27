# Rust Diagnostics

- Cargo manifest: `.codex-tmp/remeda-consumer-barrel/dist/Cargo.toml`
- Cargo check: `failed`
- Errors: `24`
- Warnings: `122`

## Summary By Code

1. **warning** `unused_mut` - 70 diagnostics
2. **warning** `unused_parens` - 39 diagnostics
3. **error** `E0425` - 24 diagnostics
4. **warning** `unused_assignments` - 6 diagnostics
5. **warning** `unused_unsafe` - 6 diagnostics
6. **warning** `unreachable_code` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 70 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/addProp.rs:19`
     - `src/clamp.rs:8`
     - `src/clone.rs:16`
     - `src/clone.rs:16`
     - `src/clone.rs:90`
2. **warning** `unused_parens` - 20 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/countBy.rs:52`
     - `src/dropWhile.rs:39`
     - `src/filter.rs:22`
     - `src/find.rs:23`
     - `src/findIndex.rs:18`
3. **error** `E0425` - 13 occurrences
   - Message: cannot find function `smelt_clear_timeout` in this scope
   - Examples:
     - `src/debounce.rs:76`
     - `src/debounce.rs:125`
     - `src/debounce.rs:212`
     - `src/debounce.rs:222`
     - `src/debounce.rs:251`
4. **error** `E0425` - 11 occurrences
   - Message: cannot find function `smelt_set_timeout` in this scope
   - Examples:
     - `src/debounce.rs:152`
     - `src/debounce.rs:192`
     - `src/debounce.rs:201`
     - `src/debounce.rs:215`
     - `src/debounce.rs:225`
5. **warning** `unused_parens` - 9 occurrences
   - Message: unnecessary parentheses around block return value
   - Examples:
     - `src/purryOrderRules.rs:148`
     - `src/purryOrderRules.rs:149`
     - `src/purryOrderRules.rs:296`
     - `src/purryOrderRules.rs:297`
     - `src/range.rs:49`
6. **warning** `unused_unsafe` - 6 occurrences
   - Message: unnecessary `unsafe` block
   - Examples:
     - `src/difference.rs:55`
     - `src/drop.rs:36`
     - `src/intersection.rs:55`
     - `src/unique.rs:22`
     - `src/uniqueBy.rs:25`
7. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/range.rs:49`
     - `src/toCamelCase.rs:48`
     - `src/toCamelCase.rs:80`
8. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/dropWhile.rs:42`
     - `src/sample.rs:71`
     - `src/take.rs:35`
9. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/funnel.rs:186`
     - `src/funnel.rs:251`
     - `src/splitAt.rs:38`
10. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/sample.rs:86`
11. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:60`
12. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:59`
13. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `global_separator` is never read
   - Examples:
     - `src/truncate.rs:122`
14. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `item` is never read
   - Examples:
     - `src/mergeAll.rs:20`
15. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `key` is never read
   - Examples:
     - `src/omit.rs:122`
16. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:33`
17. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:93`

## Cargo Stderr

```text
Compiling libc v0.2.186
   Compiling proc-macro2 v1.0.106
   Compiling unicode-ident v1.0.24
   Compiling getrandom v0.3.4
   Compiling quote v1.0.45
   Compiling zerocopy v0.8.48
    Checking cfg-if v1.0.4
    Checking memchr v2.8.0
   Compiling autocfg v1.5.1
    Checking regex-syntax v0.8.10
    Checking bit-vec v0.8.0
    Checking iana-time-zone v0.1.65
    Checking aho-corasick v1.1.4
    Checking pin-project-lite v0.2.17
   Compiling num-traits v0.2.19
    Checking bit-set v0.8.0
   Compiling syn v2.0.117
    Checking chrono v0.4.44
    Checking regex-automata v0.4.14
    Checking rand_core v0.9.5
    Checking regex v1.12.3
    Checking fancy-regex v0.14.0
   Compiling tokio-macros v2.7.0
    Checking ppv-lite86 v0.2.21
    Checking tokio v1.52.3
    Checking rand_chacha v0.9.0
    Checking rand v0.9.4
    Checking remeda_consumer_barrel_probe v0.1.0 (/home/lollo/Playground/smelt/.codex-tmp/remeda-consumer-barrel/dist)
error: could not compile `remeda_consumer_barrel_probe` (bin "remeda_consumer_barrel_probe") due to 24 previous errors; 122 warnings emitted
```
