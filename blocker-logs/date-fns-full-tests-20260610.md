# Generated Rust Test Report

- Cargo manifest: `/tmp/smelt_date_fns_probe_20260610/date-fns/dist/Cargo.toml`
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

<details>
<summary>Failing test inventory</summary>


</details>

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `/tmp/smelt_date_fns_probe_20260610/date-fns/dist/Cargo.toml`
- Cargo check: `failed`
- Errors: `6`
- Warnings: `1003`

## Summary By Code

1. **warning** `unused_mut` - 767 diagnostics
2. **warning** `unused_parens` - 131 diagnostics
3. **warning** `unused_assignments` - 104 diagnostics
4. **error** `E0308` - 6 diagnostics
5. **warning** `unreachable_code` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 767 occurrences
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
     - `src/main.rs:3917`
     - `src/main.rs:3946`
     - `src/main.rs:4056`
     - `src/main.rs:4167`
     - `src/main.rs:4220`
3. **warning** `unused_assignments` - 48 occurrences
   - Message: value assigned to `value` is never read
   - Examples:
     - `src/buildMatchFn_index.rs:2445`
     - `src/buildMatchFn_index.rs:2375`
     - `src/buildMatchFn_index.rs:2298`
     - `src/buildMatchFn_index.rs:2228`
     - `src/buildMatchFn_index.rs:2148`
4. **warning** `unused_assignments` - 18 occurrences
   - Message: value assigned to `rtf` is never read
   - Examples:
     - `src/intlFormatDistance_index_1.rs:246`
     - `src/intlFormatDistance_index_1.rs:242`
     - `src/intlFormatDistance_index_1.rs:234`
     - `src/intlFormatDistance_index_1.rs:226`
     - `src/intlFormatDistance_index_1.rs:218`
5. **warning** `unused_assignments` - 10 occurrences
   - Message: value assigned to `unit` is never read
   - Examples:
     - `src/intlFormatDistance_index_1.rs:50`
     - `src/intlFormatDistance_index_1.rs:167`
     - `src/intlFormatDistance_index_1.rs:159`
     - `src/intlFormatDistance_index_1.rs:152`
     - `src/intlFormatDistance_index_1.rs:139`
6. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/areIntervalsOverlapping_index_1.rs:42`
     - `src/areIntervalsOverlapping_index_1.rs:54`
     - `src/getOverlappingDaysInIntervals_index_1.rs:47`
     - `src/getOverlappingDaysInIntervals_index_1.rs:57`
     - `src/isWithinInterval_index_1.rs:34`
7. **warning** `unused_assignments` - 7 occurrences
   - Message: value assigned to `result` is never read
   - Examples:
     - `src/formatDistance_index.rs:21`
     - `src/formatDistance_index_1.rs:21`
     - `src/formatDistance_index_4.rs:21`
     - `src/formatDistance_index_5.rs:21`
     - `src/formatDistance_index_6.rs:21`
8. **error** `E0308` - 6 occurrences
   - Message: mismatched types
   - Examples:
     - `src/endOfISOWeek_index_1.rs:10`
     - `src/formatDistanceStrict_index_1.rs:427`
     - `src/formatDistance_index_2.rs:461`
     - `src/isSameISOWeek_index_1.rs:10`
     - `src/lastDayOfISOWeek_index_1.rs:10`
9. **warning** `unused_assignments` - 5 occurrences
   - Message: value assigned to `offset` is never read
   - Examples:
     - `src/formatRFC3339_index_1.rs:177`
     - `src/formatRFC3339_index_1.rs:123`
     - `src/parseISO_index_1.rs:206`
     - `src/parseISO_index_1.rs:89`
     - `src/test_index.rs:32`
10. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `day_period_enum_value` is never read
   - Examples:
     - `src/main.rs:12134`
     - `src/main.rs:12268`
11. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `transition_date` is never read
   - Examples:
     - `src/tzOffsetTransitions.rs:378`
     - `src/tzOffsetTransitions.rs:345`
12. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `values_array` is never read
   - Examples:
     - `src/buildLocalizeFn_index.rs:665`
     - `src/buildLocalizeFn_index.rs:51`
13. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/addBusinessDays_index_1.rs:187`
14. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `date_time_format` is never read
   - Examples:
     - `src/longFormatters_index.rs:106`
15. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `format_options` is never read
   - Examples:
     - `src/intlFormat_index_1.rs:12`
16. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `full_icu_only` is never read
   - Examples:
     - `src/test_111.rs:133`
17. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `get_operation_system_locale` is never read
   - Examples:
     - `src/test_111.rs:138`
18. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `locale_options` is never read
   - Examples:
     - `src/intlFormat_index_1.rs:14`
19. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `spanish` is never read
   - Examples:
     - `src/test_111.rs:115`
20. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `suffix` is never read
   - Examples:
     - `src/localize_index_3.rs:28`
21. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `time_string` is never read
   - Examples:
     - `src/parseISO_index_1.rs:348`
22. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `token` is never read
   - Examples:
     - `src/formatRelative_index_2.rs:387`
23. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `tz_offset` is never read
   - Examples:
     - `src/formatISO_index_1.rs:93`

## Cargo Stderr

```text
Compiling proc-macro2 v1.0.106
   Compiling autocfg v1.5.1
   Compiling quote v1.0.45
   Compiling unicode-ident v1.0.24
    Checking memchr v2.8.1
    Checking siphasher v1.0.3
    Checking regex-syntax v0.8.11
    Checking iana-time-zone v0.1.65
   Compiling chrono-tz v0.10.4
    Checking bit-vec v0.8.0
    Checking phf_shared v0.12.1
    Checking phf v0.12.1
    Checking pin-project-lite v0.2.17
    Checking bit-set v0.8.0
   Compiling num-traits v0.2.19
    Checking aho-corasick v1.1.4
   Compiling syn v2.0.117
    Checking chrono v0.4.45
    Checking regex-automata v0.4.14
   Compiling tokio-macros v2.7.0
    Checking regex v1.12.4
    Checking fancy-regex v0.14.0
    Checking tokio v1.52.3
    Checking date_fns_full_probe v0.1.0 (/tmp/smelt_date_fns_probe_20260610/date-fns/dist)
error: could not compile `date_fns_full_probe` (bin "date_fns_full_probe") due to 6 previous errors; 1003 warnings emitted
```
