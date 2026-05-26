# Rust Diagnostics

- Cargo manifest: `/tmp/smelt_date_fns_full_tests_20260526/dist/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `662`

## Summary By Code

1. **warning** `unused_mut` - 373 diagnostics
2. **warning** `unused_parens` - 177 diagnostics
3. **warning** `unused_assignments` - 111 diagnostics
4. **warning** `unreachable_code` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 373 occurrences
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
     - `src/main.rs:3755`
     - `src/main.rs:3784`
     - `src/main.rs:3894`
     - `src/main.rs:4005`
     - `src/main.rs:4058`
3. **warning** `unused_assignments` - 29 occurrences
   - Message: value assigned to `value` is never read
   - Examples:
     - `src/buildMatchFn_index.rs:584`
     - `src/buildMatchFn_index.rs:517`
     - `src/buildMatchFn_index.rs:443`
     - `src/buildMatchFn_index.rs:376`
     - `src/buildMatchFn_index.rs:287`
4. **warning** `unused_assignments` - 24 occurrences
   - Message: value assigned to `rtf` is never read
   - Examples:
     - `src/intlFormatDistance_index_1.rs:295`
     - `src/intlFormatDistance_index_1.rs:291`
     - `src/intlFormatDistance_index_1.rs:283`
     - `src/intlFormatDistance_index_1.rs:275`
     - `src/intlFormatDistance_index_1.rs:267`
5. **warning** `unused_parens` - 20 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/format_index_1.rs:260`
     - `src/format_index_1.rs:262`
     - `src/format_index_1.rs:266`
     - `src/format_index_1.rs:266`
     - `src/format_index_1.rs:266`
6. **warning** `unused_parens` - 18 occurrences
   - Message: unnecessary parentheses around block return value
   - Examples:
     - `src/formatDistanceStrict_index_1.rs:198`
     - `src/formatDistance_index_2.rs:234`
     - `src/formatDuration_index_1.rs:131`
     - `src/formatRelative_index_2.rs:153`
     - `src/format_index_1.rs:161`
7. **warning** `unused_assignments` - 17 occurrences
   - Message: value assigned to `unit` is never read
   - Examples:
     - `src/formatDistanceStrict_index_1.rs:304`
     - `src/intlFormatDistance_index_1.rs:49`
     - `src/intlFormatDistance_index_1.rs:215`
     - `src/intlFormatDistance_index_1.rs:207`
     - `src/intlFormatDistance_index_1.rs:200`
8. **warning** `unused_assignments` - 8 occurrences
   - Message: value assigned to `prev` is never read
   - Examples:
     - `src/main.rs:10352`
     - `src/main.rs:10339`
     - `src/main.rs:10326`
     - `src/main.rs:10313`
     - `src/main.rs:10300`
9. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/areIntervalsOverlapping_index_1.rs:42`
     - `src/areIntervalsOverlapping_index_1.rs:54`
     - `src/getOverlappingDaysInIntervals_index_1.rs:47`
     - `src/getOverlappingDaysInIntervals_index_1.rs:57`
     - `src/isWithinInterval_index_1.rs:34`
10. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/main.rs:18855`
     - `src/main.rs:18863`
     - `src/main.rs:18873`
     - `src/main.rs:18883`
     - `src/main.rs:18945`
11. **warning** `unused_assignments` - 7 occurrences
   - Message: value assigned to `result` is never read
   - Examples:
     - `src/formatDistance_index.rs:20`
     - `src/formatDistance_index_1.rs:20`
     - `src/formatDistance_index_4.rs:20`
     - `src/formatDistance_index_5.rs:20`
     - `src/formatDistance_index_6.rs:20`
12. **warning** `unused_assignments` - 6 occurrences
   - Message: value assigned to `base_tz_offset` is never read
   - Examples:
     - `src/main.rs:12881`
     - `src/main.rs:12873`
     - `src/main.rs:12850`
     - `src/main.rs:12842`
     - `src/main.rs:12812`
13. **warning** `unused_assignments` - 5 occurrences
   - Message: value assigned to `offset` is never read
   - Examples:
     - `src/formatRFC3339_index_1.rs:177`
     - `src/formatRFC3339_index_1.rs:123`
     - `src/parseISO_index_1.rs:188`
     - `src/parseISO_index_1.rs:83`
     - `src/test_index.rs:30`
14. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `day_period_enum_value` is never read
   - Examples:
     - `src/main.rs:14766`
     - `src/main.rs:14900`
15. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `values_array` is never read
   - Examples:
     - `src/buildLocalizeFn_index.rs:368`
     - `src/buildLocalizeFn_index.rs:52`
16. **warning** `unreachable_code` - 1 occurrence
   - Message: unreachable statement
   - Examples:
     - `src/addBusinessDays_index_1.rs:191`
17. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `date_time_format` is never read
   - Examples:
     - `src/longFormatters_index.rs:106`
18. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `flags` is never read
   - Examples:
     - `src/main.rs:18833`
19. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `format_options` is never read
   - Examples:
     - `src/intlFormat_index_1.rs:12`
20. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `full_icu_only` is never read
   - Examples:
     - `src/test_111.rs:96`
21. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `get_operation_system_locale` is never read
   - Examples:
     - `src/test_111.rs:101`
22. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `locale_options` is never read
   - Examples:
     - `src/intlFormat_index_1.rs:14`
23. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `months` is never read
   - Examples:
     - `src/formatDistance_index_2.rs:335`
24. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `spanish` is never read
   - Examples:
     - `src/test_111.rs:78`
25. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `time_string` is never read
   - Examples:
     - `src/parseISO_index_1.rs:308`
26. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `token` is never read
   - Examples:
     - `src/formatRelative_index_2.rs:233`
27. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `tz_offset` is never read
   - Examples:
     - `src/formatISO_index_1.rs:93`

## Cargo Stderr

```text
Checking date_fns_full_tests v0.1.0 (/tmp/smelt_date_fns_full_tests_20260526/dist)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 45.32s
```
