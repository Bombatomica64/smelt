# Rust Diagnostics

- Cargo manifest: `/tmp/smelt_date_fns_full_tests_20260604/dist/Cargo.toml`
- Cargo check: `failed`
- Errors: `2`
- Warnings: `754`

## Summary By Code

1. **warning** `unused_mut` - 467 diagnostics
2. **warning** `unused_parens` - 181 diagnostics
3. **warning** `unused_assignments` - 105 diagnostics
4. **error** `E0308` - 1 diagnostic
5. **error** `E0425` - 1 diagnostic
6. **warning** `unreachable_code` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 467 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/addWithOptions_index.rs:7`
     - `src/add_index.rs:7`
     - `src/add_index_1.rs:7`
     - `src/areIntervalsOverlappingWithOptions_index.rs:7`
     - `src/areIntervalsOverlappingWithOptions_index.rs:7`
2. **warning** `unused_parens` - 123 occurrences
   - Message: unnecessary parentheses around type
   - Examples:
     - `src/main.rs:3884`
     - `src/main.rs:3913`
     - `src/main.rs:4023`
     - `src/main.rs:4134`
     - `src/main.rs:4187`
3. **warning** `unused_assignments` - 48 occurrences
   - Message: value assigned to `value` is never read
   - Examples:
     - `src/buildMatchFn_index.rs:2349`
     - `src/buildMatchFn_index.rs:2282`
     - `src/buildMatchFn_index.rs:2208`
     - `src/buildMatchFn_index.rs:2141`
     - `src/buildMatchFn_index.rs:2064`
4. **warning** `unused_parens` - 24 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/format_index_1.rs:410`
     - `src/format_index_1.rs:412`
     - `src/format_index_1.rs:416`
     - `src/format_index_1.rs:416`
     - `src/format_index_1.rs:416`
5. **warning** `unused_assignments` - 18 occurrences
   - Message: value assigned to `rtf` is never read
   - Examples:
     - `src/intlFormatDistance_index_1.rs:246`
     - `src/intlFormatDistance_index_1.rs:242`
     - `src/intlFormatDistance_index_1.rs:234`
     - `src/intlFormatDistance_index_1.rs:226`
     - `src/intlFormatDistance_index_1.rs:218`
6. **warning** `unused_parens` - 18 occurrences
   - Message: unnecessary parentheses around block return value
   - Examples:
     - `src/formatDistanceStrict_index_1.rs:276`
     - `src/formatDistance_index_2.rs:309`
     - `src/formatDuration_index_1.rs:206`
     - `src/formatRelative_index_2.rs:228`
     - `src/format_index_1.rs:238`
7. **warning** `unused_assignments` - 11 occurrences
   - Message: value assigned to `unit` is never read
   - Examples:
     - `src/formatDistanceStrict_index_1.rs:457`
     - `src/intlFormatDistance_index_1.rs:50`
     - `src/intlFormatDistance_index_1.rs:167`
     - `src/intlFormatDistance_index_1.rs:159`
     - `src/intlFormatDistance_index_1.rs:152`
8. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/areIntervalsOverlapping_index_1.rs:42`
     - `src/areIntervalsOverlapping_index_1.rs:54`
     - `src/getOverlappingDaysInIntervals_index_1.rs:47`
     - `src/getOverlappingDaysInIntervals_index_1.rs:57`
     - `src/isWithinInterval_index_1.rs:34`
9. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/main.rs:12815`
     - `src/main.rs:12823`
     - `src/main.rs:12833`
     - `src/main.rs:12843`
     - `src/main.rs:12911`
10. **warning** `unused_assignments` - 7 occurrences
   - Message: value assigned to `result` is never read
   - Examples:
     - `src/formatDistance_index.rs:21`
     - `src/formatDistance_index_1.rs:21`
     - `src/formatDistance_index_4.rs:21`
     - `src/formatDistance_index_5.rs:21`
     - `src/formatDistance_index_6.rs:21`
11. **warning** `unused_assignments` - 5 occurrences
   - Message: value assigned to `offset` is never read
   - Examples:
     - `src/formatRFC3339_index_1.rs:177`
     - `src/formatRFC3339_index_1.rs:123`
     - `src/parseISO_index_1.rs:206`
     - `src/parseISO_index_1.rs:89`
     - `src/test_index.rs:32`
12. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `day_period_enum_value` is never read
   - Examples:
     - `src/main.rs:12103`
     - `src/main.rs:12237`
13. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `transition_date` is never read
   - Examples:
     - `src/tzOffsetTransitions.rs:380`
     - `src/tzOffsetTransitions.rs:347`
14. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `values_array` is never read
   - Examples:
     - `src/buildLocalizeFn_index.rs:665`
     - `src/buildLocalizeFn_index.rs:51`
15. **error** `E0308` - 1 occurrence
   - Message: mismatched types
   - Examples:
     - `src/localize_index_3.rs:43`
16. **error** `E0425` - 1 occurrence
   - Message: cannot find type `ResultDate` in this scope
   - Examples:
     - `src/isMatch_index_1.rs:9`
17. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/addBusinessDays_index_1.rs:187`
18. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `date_time_format` is never read
   - Examples:
     - `src/longFormatters_index.rs:106`
19. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `format_options` is never read
   - Examples:
     - `src/intlFormat_index_1.rs:12`
20. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `full_icu_only` is never read
   - Examples:
     - `src/test_111.rs:133`
21. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `get_operation_system_locale` is never read
   - Examples:
     - `src/test_111.rs:138`
22. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `locale_options` is never read
   - Examples:
     - `src/intlFormat_index_1.rs:14`
23. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `months` is never read
   - Examples:
     - `src/formatDistance_index_2.rs:483`
24. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `spanish` is never read
   - Examples:
     - `src/test_111.rs:115`
25. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `time_string` is never read
   - Examples:
     - `src/parseISO_index_1.rs:348`
26. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `token` is never read
   - Examples:
     - `src/formatRelative_index_2.rs:381`
27. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `tz_offset` is never read
   - Examples:
     - `src/formatISO_index_1.rs:93`

## Cargo Stderr

```text
Updating crates.io index
     Locking 50 packages to latest Rust 1.94.1 compatible versions
      Adding fancy-regex v0.14.0 (available: v0.18.0)
   Compiling proc-macro2 v1.0.106
   Compiling quote v1.0.45
   Compiling unicode-ident v1.0.24
   Compiling autocfg v1.5.1
    Checking memchr v2.8.1
    Checking siphasher v1.0.3
    Checking regex-syntax v0.8.10
    Checking bit-vec v0.8.0
    Checking iana-time-zone v0.1.65
    Checking phf_shared v0.12.1
   Compiling chrono-tz v0.10.4
    Checking bit-set v0.8.0
    Checking phf v0.12.1
    Checking pin-project-lite v0.2.17
   Compiling num-traits v0.2.19
    Checking aho-corasick v1.1.4
   Compiling syn v2.0.117
    Checking chrono v0.4.45
    Checking regex-automata v0.4.14
   Compiling tokio-macros v2.7.0
    Checking fancy-regex v0.14.0
    Checking regex v1.12.3
    Checking tokio v1.52.3
    Checking date_fns_full_tests v0.1.0 (/tmp/smelt_date_fns_full_tests_20260604/dist)
error: could not compile `date_fns_full_tests` (bin "date_fns_full_tests") due to 2 previous errors; 754 warnings emitted
```
