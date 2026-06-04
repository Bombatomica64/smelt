# Rust Diagnostics

- Cargo manifest: `/tmp/smelt_date_fns_parse_probe_20260603/dist/Cargo.toml`
- Cargo check: `failed`
- Errors: `564`
- Warnings: `295`

## Summary By Code

1. **error** `E0631` - 541 diagnostics
2. **warning** `unused_mut` - 205 diagnostics
3. **warning** `unused_parens` - 52 diagnostics
4. **warning** `unused_assignments` - 38 diagnostics
5. **error** `E0308` - 21 diagnostics
6. **error** `E0596` - 2 diagnostics

## Groups

1. **error** `E0631` - 541 occurrences
   - Message: type mismatch in closure arguments
   - Examples:
     - `src/buildFormatLongFn_index.rs:39`
     - `src/parse_index.rs:152`
     - `src/parse_index.rs:172`
     - `src/parse_index.rs:215`
     - `src/parse_index.rs:218`
2. **warning** `unused_mut` - 205 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/buildLocalizeFn_index.rs:7`
     - `src/buildMatchFn_index.rs:54`
     - `src/buildMatchFn_index.rs:100`
     - `src/buildMatchFn_index.rs:103`
     - `src/buildMatchFn_index.rs:127`
3. **warning** `unused_assignments` - 32 occurrences
   - Message: value assigned to `value` is never read
   - Examples:
     - `src/buildMatchFn_index.rs:2349`
     - `src/buildMatchFn_index.rs:2282`
     - `src/buildMatchFn_index.rs:2208`
     - `src/buildMatchFn_index.rs:2141`
     - `src/buildMatchFn_index.rs:2064`
4. **error** `E0308` - 21 occurrences
   - Message: mismatched types
   - Examples:
     - `src/parse_index.rs:372`
     - `src/parse_index.rs:373`
     - `src/parse_index.rs:373`
     - `src/parse_index.rs:373`
     - `src/parse_index.rs:373`
5. **warning** `unused_parens` - 20 occurrences
   - Message: unnecessary parentheses around type
   - Examples:
     - `src/main.rs:972`
     - `src/main.rs:1083`
     - `src/main.rs:1136`
     - `src/main.rs:1150`
     - `src/main.rs:1388`
6. **warning** `unused_parens` - 13 occurrences
   - Message: unnecessary parentheses around block return value
   - Examples:
     - `src/parse_index.rs:290`
     - `src/parse_index.rs:3424`
     - `src/parse_index.rs:3426`
     - `src/parse_index.rs:3436`
     - `src/parse_index.rs:3438`
7. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/parse_index.rs:423`
     - `src/parse_index.rs:425`
     - `src/main.rs:5239`
     - `src/main.rs:5272`
     - `src/main.rs:5279`
8. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/main.rs:3761`
     - `src/main.rs:3769`
     - `src/main.rs:3779`
     - `src/main.rs:3789`
     - `src/main.rs:3861`
9. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/parse_index.rs:3416`
     - `src/parse_index.rs:3427`
     - `src/parse_index.rs:3439`
10. **error** `E0596` - 2 occurrences
   - Message: cannot borrow `options` as mutable, as it is not declared as mutable
   - Examples:
     - `src/main.rs:3610`
     - `src/main.rs:3614`
11. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `result` is never read
   - Examples:
     - `src/formatDistance_index.rs:21`
     - `src/utils.rs:623`
12. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `values_array` is never read
   - Examples:
     - `src/buildLocalizeFn_index.rs:665`
     - `src/buildLocalizeFn_index.rs:51`
13. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `date_time_format` is never read
   - Examples:
     - `src/longFormatters_index.rs:106`
14. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `offset` is never read
   - Examples:
     - `src/test_index.rs:32`

## Cargo Stderr

```text
Updating crates.io index
     Locking 50 packages to latest Rust 1.94.1 compatible versions
      Adding fancy-regex v0.14.0 (available: v0.18.0)
   Compiling proc-macro2 v1.0.106
   Compiling autocfg v1.5.1
   Compiling unicode-ident v1.0.24
   Compiling quote v1.0.45
    Checking memchr v2.8.1
    Checking siphasher v1.0.3
    Checking regex-syntax v0.8.10
   Compiling chrono-tz v0.10.4
    Checking iana-time-zone v0.1.65
    Checking phf_shared v0.12.1
    Checking bit-vec v0.8.0
    Checking pin-project-lite v0.2.17
    Checking phf v0.12.1
    Checking bit-set v0.8.0
   Compiling num-traits v0.2.19
    Checking aho-corasick v1.1.4
   Compiling syn v2.0.117
    Checking chrono v0.4.44
    Checking regex-automata v0.4.14
    Checking regex v1.12.3
    Checking fancy-regex v0.14.0
   Compiling tokio-macros v2.7.0
    Checking tokio v1.52.3
    Checking date_fns_parse_probe v0.1.0 (/tmp/smelt_date_fns_parse_probe_20260603/dist)
error: could not compile `date_fns_parse_probe` (bin "date_fns_parse_probe") due to 564 previous errors; 295 warnings emitted
```
