# Rust Diagnostics

- Cargo manifest: `/tmp/smelt_date_fns_tests_fcIIIc/dist/Cargo.toml`
- Cargo check: `failed`
- Errors: `1`
- Warnings: `413`

## Summary By Code

1. **warning** `unused_parens` - 171 diagnostics
2. **warning** `unused_mut` - 134 diagnostics
3. **warning** `unused_assignments` - 106 diagnostics
4. **warning** `unreachable_code` - 2 diagnostics
5. **error** `E0277` - 1 diagnostic

## Groups

1. **warning** `unused_mut` - 134 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/clamp_index_1.rs:7`
     - `src/clamp_index_1.rs:8`
     - `src/clamp_index_1.rs:9`
     - `src/closestTo_index_1.rs:7`
     - `src/convertToFP_index.rs:26`
2. **warning** `unused_parens` - 123 occurrences
   - Message: unnecessary parentheses around type
   - Examples:
     - `src/main.rs:3492`
     - `src/main.rs:3521`
     - `src/main.rs:3631`
     - `src/main.rs:3742`
     - `src/main.rs:3795`
3. **warning** `unused_assignments` - 29 occurrences
   - Message: value assigned to `value` is never read
   - Examples:
     - `src/buildMatchFn_index.rs:583`
     - `src/buildMatchFn_index.rs:516`
     - `src/buildMatchFn_index.rs:442`
     - `src/buildMatchFn_index.rs:375`
     - `src/buildMatchFn_index.rs:286`
4. **warning** `unused_assignments` - 24 occurrences
   - Message: value assigned to `rtf` is never read
   - Examples:
     - `src/intlFormatDistance_index_1.rs:294`
     - `src/intlFormatDistance_index_1.rs:290`
     - `src/intlFormatDistance_index_1.rs:282`
     - `src/intlFormatDistance_index_1.rs:274`
     - `src/intlFormatDistance_index_1.rs:266`
5. **warning** `unused_parens` - 20 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/format_index_1.rs:103`
     - `src/format_index_1.rs:105`
     - `src/format_index_1.rs:109`
     - `src/format_index_1.rs:109`
     - `src/format_index_1.rs:109`
6. **warning** `unused_assignments` - 17 occurrences
   - Message: value assigned to `unit` is never read
   - Examples:
     - `src/formatDistanceStrict_index_1.rs:149`
     - `src/intlFormatDistance_index_1.rs:48`
     - `src/intlFormatDistance_index_1.rs:214`
     - `src/intlFormatDistance_index_1.rs:206`
     - `src/intlFormatDistance_index_1.rs:199`
7. **warning** `unused_parens` - 12 occurrences
   - Message: unnecessary parentheses around block return value
   - Examples:
     - `src/parse_index_1.rs:373`
     - `src/parse_index_1.rs:375`
     - `src/parse_index_1.rs:385`
     - `src/parse_index_1.rs:387`
     - `src/main.rs:18910`
8. **warning** `unused_assignments` - 8 occurrences
   - Message: value assigned to `prev` is never read
   - Examples:
     - `src/main.rs:10044`
     - `src/main.rs:10031`
     - `src/main.rs:10018`
     - `src/main.rs:10005`
     - `src/main.rs:9992`
9. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/areIntervalsOverlapping_index_1.rs:41`
     - `src/areIntervalsOverlapping_index_1.rs:53`
     - `src/getOverlappingDaysInIntervals_index_1.rs:46`
     - `src/getOverlappingDaysInIntervals_index_1.rs:56`
     - `src/isWithinInterval_index_1.rs:33`
10. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/main.rs:18482`
     - `src/main.rs:18490`
     - `src/main.rs:18500`
     - `src/main.rs:18510`
     - `src/main.rs:18572`
11. **warning** `unused_assignments` - 5 occurrences
   - Message: value assigned to `offset` is never read
   - Examples:
     - `src/formatRFC3339_index_1.rs:174`
     - `src/formatRFC3339_index_1.rs:121`
     - `src/parseISO_index_1.rs:187`
     - `src/parseISO_index_1.rs:82`
     - `src/test_index.rs:29`
12. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `result` is never read
   - Examples:
     - `src/formatDistance_index.rs:18`
     - `src/formatDistance_index_1.rs:18`
     - `src/formatDistance_index_4.rs:18`
     - `src/utils.rs:277`
13. **warning** `unreachable_code` - 2 occurrences
   - Message: unreachable statement
   - Examples:
     - `src/addBusinessDays_index_1.rs:190`
     - `src/localize_index_1.rs:39`
14. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `date` is never read
   - Examples:
     - `src/parseISO_index_1.rs:67`
     - `src/parseISO_index_1.rs:61`
15. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `day_period_enum_value` is never read
   - Examples:
     - `src/main.rs:14396`
     - `src/main.rs:14530`
16. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `localize_options` is never read
   - Examples:
     - `src/formatDistanceStrict_index_1.rs:116`
     - `src/formatDistance_index_2.rs:153`
17. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `values_array` is never read
   - Examples:
     - `src/buildLocalizeFn_index.rs:470`
     - `src/buildLocalizeFn_index.rs:54`
18. **error** `E0277` - 1 occurrence
   - Message: the trait bound `Result<Vec<SmeltUnknown>, Box<dyn std::error::Error>>: Default` is not satisfied
   - Examples:
     - `src/main.rs:12578`
19. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `date_time_format` is never read
   - Examples:
     - `src/longFormatters_index.rs:105`
20. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `flags` is never read
   - Examples:
     - `src/main.rs:18460`
21. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `format_options` is never read
   - Examples:
     - `src/intlFormat_index_1.rs:11`
22. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `full_icu_only` is never read
   - Examples:
     - `src/test_111.rs:95`
23. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `get_operation_system_locale` is never read
   - Examples:
     - `src/test_111.rs:100`
24. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `locale_options` is never read
   - Examples:
     - `src/intlFormat_index_1.rs:13`
25. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `months` is never read
   - Examples:
     - `src/formatDistance_index_2.rs:180`
26. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `spanish` is never read
   - Examples:
     - `src/test_111.rs:77`
27. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `time_string` is never read
   - Examples:
     - `src/parseISO_index_1.rs:307`
28. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `token` is never read
   - Examples:
     - `src/formatRelative_index_2.rs:78`
29. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `tz_offset` is never read
   - Examples:
     - `src/formatISO_index_1.rs:91`

## Cargo Stderr

```text
Checking date_fns_tests v0.1.0 (/tmp/smelt_date_fns_tests_fcIIIc/dist)
error: could not compile `date_fns_tests` (bin "date_fns_tests") due to 1 previous error; 413 warnings emitted
```
