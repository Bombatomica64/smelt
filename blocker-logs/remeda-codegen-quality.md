# Rust Diagnostics

- Cargo manifest: `third_party/remeda/dist-smelt/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `269`

## Summary By Code

1. **warning** `unused_mut` - 135 diagnostics
2. **warning** `unused_parens` - 92 diagnostics
3. **warning** `unused_unsafe` - 31 diagnostics
4. **warning** `unused_assignments` - 9 diagnostics
5. **warning** `unreachable_code` - 1 diagnostic
6. **warning** `unused_must_use` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 135 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/addProp.rs:19`
     - `src/allPass.rs:16`
     - `src/allPass.rs:24`
     - `src/anyPass.rs:16`
     - `src/anyPass.rs:24`
2. **warning** `unused_parens` - 65 occurrences
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/funnel.rs:194`
     - `src/funnel.rs:279`
     - `src/funnel.rs:367`
     - `src/funnel.rs:452`
     - `src/funnel.rs:547`
3. **warning** `unused_unsafe` - 31 occurrences
   - Message: unnecessary `unsafe` block
   - Examples:
     - `src/binarySearchCutoffIndex_test.rs:13`
     - `src/debounce.rs:77`
     - `src/debounce.rs:127`
     - `src/debounce.rs:159`
     - `src/debounce.rs:200`
4. **warning** `unused_parens` - 24 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/countBy.rs:52`
     - `src/dropWhile.rs:39`
     - `src/filter.rs:22`
     - `src/filter.rs:26`
     - `src/find.rs:23`
5. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `rest` is never read
   - Examples:
     - `src/funnel_lodash_debounce_test.rs:111`
     - `src/funnel_lodash_debounce_test.rs:98`
     - `src/funnel_lodash_throttle_test.rs:92`
     - `src/funnel_lodash_throttle_test.rs:79`
6. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/sample.rs:112`
7. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg_removed` is never read
   - Examples:
     - `src/purryOrderRules.rs:67`
8. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `arg` is never read
   - Examples:
     - `src/purryOrderRules.rs:66`
9. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `match_` is never read
   - Examples:
     - `src/stringToPath.rs:33`
10. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `sum` is never read
   - Examples:
     - `src/evolve_test.rs:850`
11. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `value` is never read
   - Examples:
     - `src/isShallowEqual.rs:259`
12. **warning** `unused_must_use` - 1 occurrence
   - Message: unused `Result` that must be used
   - Examples:
     - `src/funnel_reference_batch_test.rs:159`
13. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around `return` value
   - Examples:
     - `src/randomBigInt.rs:93`
14. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/dropWhile.rs:42`
15. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around closure body
   - Examples:
     - `src/sample_test.rs:25`

## Cargo Stderr

```text
Compiling proc-macro2 v1.0.106
    Checking libc v0.2.186
    Checking zerocopy v0.8.48
    Checking serde_core v1.0.228
    Checking num-traits v0.2.19
    Checking zmij v1.0.21
   Compiling quote v1.0.45
    Checking chrono v0.4.44
   Compiling syn v2.0.117
    Checking getrandom v0.3.4
    Checking rand_core v0.9.5
    Checking chrono-tz v0.10.4
    Checking serde_json v1.0.149
    Checking ppv-lite86 v0.2.21
   Compiling serde_derive v1.0.228
   Compiling tokio-macros v2.7.0
    Checking rand_chacha v0.9.0
    Checking rand v0.9.4
    Checking tokio v1.52.3
    Checking serde v1.0.228
    Checking remeda_smelt_probe v0.1.0 (/home/lollo/Playground/smelt/.claude/worktrees/agent-a63d35b2ab716fc6e/third_party/remeda/dist-smelt)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2m 08s
```
