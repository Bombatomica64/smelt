# Rust Diagnostics

- Cargo manifest: `/tmp/smelt_date_fns_full_tests_20260526/dist/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `754`

## Summary By Code

1. **warning** `unused_mut` - 466 diagnostics
2. **warning** `unused_parens` - 181 diagnostics
3. **warning** `unused_assignments` - 106 diagnostics
4. **warning** `unreachable_code` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 466 occurrences
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
     - `src/main.rs:3760`
     - `src/main.rs:3789`
     - `src/main.rs:3899`
     - `src/main.rs:4010`
     - `src/main.rs:4063`
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
     - `src/format_index_1.rs:310`
     - `src/format_index_1.rs:312`
     - `src/format_index_1.rs:316`
     - `src/format_index_1.rs:316`
     - `src/format_index_1.rs:316`
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
     - `src/formatDistanceStrict_index_1.rs:226`
     - `src/formatDistance_index_2.rs:259`
     - `src/formatDuration_index_1.rs:156`
     - `src/formatRelative_index_2.rs:178`
     - `src/format_index_1.rs:188`
7. **warning** `unused_assignments` - 11 occurrences
   - Message: value assigned to `unit` is never read
   - Examples:
     - `src/formatDistanceStrict_index_1.rs:357`
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
     - `src/main.rs:16168`
     - `src/main.rs:16176`
     - `src/main.rs:16186`
     - `src/main.rs:16196`
     - `src/main.rs:16268`
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
     - `src/parseISO_index_1.rs:212`
     - `src/parseISO_index_1.rs:91`
     - `src/test_index.rs:30`
12. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `day_period_enum_value` is never read
   - Examples:
     - `src/main.rs:12078`
     - `src/main.rs:12212`
13. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `transition_date` is never read
   - Examples:
     - `src/tzOffsetTransitions.rs:383`
     - `src/tzOffsetTransitions.rs:350`
14. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `values_array` is never read
   - Examples:
     - `src/buildLocalizeFn_index.rs:682`
     - `src/buildLocalizeFn_index.rs:52`
15. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/addBusinessDays_index_1.rs:202`
16. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `date_time_format` is never read
   - Examples:
     - `src/longFormatters_index.rs:106`
17. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `flags` is never read
   - Examples:
     - `src/main.rs:16146`
18. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `format_options` is never read
   - Examples:
     - `src/intlFormat_index_1.rs:12`
19. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `full_icu_only` is never read
   - Examples:
     - `src/test_111.rs:103`
20. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `get_operation_system_locale` is never read
   - Examples:
     - `src/test_111.rs:108`
21. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `locale_options` is never read
   - Examples:
     - `src/intlFormat_index_1.rs:14`
22. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `months` is never read
   - Examples:
     - `src/formatDistance_index_2.rs:383`
23. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `spanish` is never read
   - Examples:
     - `src/test_111.rs:85`
24. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `time_string` is never read
   - Examples:
     - `src/parseISO_index_1.rs:358`
25. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `token` is never read
   - Examples:
     - `src/formatRelative_index_2.rs:281`
26. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `tz_offset` is never read
   - Examples:
     - `src/formatISO_index_1.rs:93`

## Cargo Stderr

```text
Checking date_fns_full_tests v0.1.0 (/tmp/smelt_date_fns_full_tests_20260526/dist)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 39s
```
