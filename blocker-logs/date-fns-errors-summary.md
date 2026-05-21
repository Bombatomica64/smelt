# Rust Diagnostics

- Cargo manifest: `/tmp/smelt_date_fns_compat_mZz3tt/dist/Cargo.toml`
- Cargo check: `failed`
- Errors: `2270`
- Warnings: `678`

## Summary By Code

1. **error** `E0308` - 2074 diagnostics
2. **warning** `unused_mut` - 284 diagnostics
3. **warning** `unused_parens` - 193 diagnostics
4. **warning** `unused_assignments` - 182 diagnostics
5. **error** `E0277` - 96 diagnostics
6. **error** `E0384` - 29 diagnostics
7. **error** `E0599` - 25 diagnostics
8. **warning** `unreachable_code` - 18 diagnostics
9. **error** `E0369` - 16 diagnostics
10. **error** `E0600` - 9 diagnostics
11. **error** `E0282` - 6 diagnostics
12. **error** `E0609` - 5 diagnostics
13. **error** `E0425` - 3 diagnostics
14. **error** `E0689` - 3 diagnostics
15. **error** `E0057` - 1 diagnostic
16. **error** `E0382` - 1 diagnostic
17. **error** `E0615` - 1 diagnostic
18. **error** `E0618` - 1 diagnostic
19. **warning** `non_camel_case_types` - 1 diagnostic

## Groups

1. **error** `E0308` - 2071 occurrences
   - Message: mismatched types
   - Examples:
     - `src/addBusinessDays_index_1.rs:59`
     - `src/addBusinessDays_index_1.rs:59`
     - `src/addBusinessDays_index_1.rs:62`
     - `src/addBusinessDays_index_1.rs:62`
     - `src/addBusinessDays_index_1.rs:66`
2. **warning** `unused_mut` - 284 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/addLeadingZeros_index.rs:7`
     - `src/addLeadingZeros_index.rs:8`
     - `src/closestIndexTo_index_1.rs:7`
     - `src/closestTo_index_1.rs:7`
     - `src/closestTo_index_1.rs:8`
3. **warning** `unused_parens` - 123 occurrences
   - Message: unnecessary parentheses around type
   - Examples:
     - `src/main.rs:4780`
     - `src/main.rs:4787`
     - `src/main.rs:4829`
     - `src/main.rs:4879`
     - `src/main.rs:4902`
4. **error** `E0277` - 66 occurrences
   - Message: the trait bound `Localize: Default` is not satisfied
   - Examples:
     - `src/format_index_1.rs:133`
     - `src/format_index_1.rs:133`
     - `src/format_index_1.rs:133`
     - `src/format_index_1.rs:133`
     - `src/format_index_1.rs:133`
5. **warning** `unused_assignments` - 64 occurrences
   - Message: value assigned to `prev` is never read
   - Examples:
     - `src/main.rs:8994`
     - `src/main.rs:8981`
     - `src/main.rs:8968`
     - `src/main.rs:8955`
     - `src/main.rs:8942`
6. **warning** `unused_assignments` - 61 occurrences
   - Message: value assigned to `transition_date` is never read
   - Examples:
     - `src/main.rs:9126`
     - `src/main.rs:9208`
     - `src/main.rs:9290`
     - `src/main.rs:9372`
     - `src/main.rs:9454`
7. **error** `E0384` - 29 occurrences
   - Message: cannot assign to immutable argument `date`
   - Examples:
     - `src/main.rs:19466`
     - `src/main.rs:19469`
     - `src/main.rs:19559`
     - `src/main.rs:19562`
     - `src/main.rs:19652`
8. **warning** `unused_parens` - 25 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/formatRelative_index_65.rs:9`
     - `src/formatRelative_index_65.rs:23`
     - `src/formatRelative_index_66.rs:9`
     - `src/formatRelative_index_66.rs:23`
     - `src/format_index_1.rs:105`
9. **warning** `unused_parens` - 21 occurrences
   - Message: unnecessary parentheses around block return value
   - Examples:
     - `src/parse_index_1.rs:254`
     - `src/parse_index_1.rs:256`
     - `src/parse_index_1.rs:384`
     - `src/parse_index_1.rs:386`
     - `src/parse_index_1.rs:521`
10. **warning** `unreachable_code` - 18 occurrences
   - Message: unreachable statement
   - Examples:
     - `src/addBusinessDays_index_1.rs:193`
     - `src/formatRelative_index_13.rs:51`
     - `src/formatRelative_index_13.rs:107`
     - `src/formatRelative_index_14.rs:51`
     - `src/formatRelative_index_14.rs:107`
11. **warning** `unused_assignments` - 17 occurrences
   - Message: value passed to `date` is never read
   - Examples:
     - `src/main.rs:19463`
     - `src/main.rs:19554`
     - `src/main.rs:19647`
     - `src/main.rs:19747`
     - `src/main.rs:19845`
12. **warning** `unused_parens` - 16 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/areIntervalsOverlapping_index_1.rs:37`
     - `src/areIntervalsOverlapping_index_1.rs:47`
     - `src/basic.rs:41`
     - `src/basic_1.rs:24`
     - `src/basic_2.rs:26`
13. **error** `E0369` - 15 occurrences
   - Message: cannot add `Option<f64>` to `Option<f64>`
   - Examples:
     - `src/add_index_1.rs:43`
     - `src/add_index_1.rs:50`
     - `src/add_index_1.rs:55`
     - `src/add_index_1.rs:58`
     - `src/add_index_1.rs:72`
14. **error** `E0277` - 12 occurrences
   - Message: the trait bound `Locale: Default` is not satisfied
   - Examples:
     - `src/formatDistanceStrict_index_1.rs:102`
     - `src/formatDistanceStrict_index_1.rs:106`
     - `src/formatDistance_index_2.rs:138`
     - `src/formatDistance_index_2.rs:142`
     - `src/formatDuration_index_1.rs:35`
15. **error** `E0599` - 11 occurrences
   - Message: no method named `get_day` found for enum `SmeltUnknown` in the current scope
   - Examples:
     - `src/formatRelative_index_20.rs:27`
     - `src/formatRelative_index_54.rs:28`
     - `src/formatRelative_index_54.rs:32`
     - `src/formatRelative_index_65.rs:9`
     - `src/formatRelative_index_65.rs:9`
16. **error** `E0600` - 9 occurrences
   - Message: cannot apply unary operator `!` to type `f64`
   - Examples:
     - `src/addDays_index_1.rs:27`
     - `src/addMonths_index_1.rs:44`
     - `src/eachDayOfInterval_index_1.rs:68`
     - `src/eachHourOfInterval_index_1.rs:66`
     - `src/eachMinuteOfInterval_index_1.rs:61`
17. **warning** `unused_assignments` - 9 occurrences
   - Message: value assigned to `date` is never read
   - Examples:
     - `src/main.rs:14055`
     - `src/main.rs:19466`
     - `src/main.rs:19559`
     - `src/main.rs:19652`
     - `src/main.rs:19750`
18. **error** `E0277` - 8 occurrences
   - Message: the trait bound `FormatLong: Default` is not satisfied
   - Examples:
     - `src/format_index_1.rs:105`
     - `src/format_index_1.rs:105`
     - `src/format_index_1.rs:107`
     - `src/format_index_1.rs:107`
     - `src/parse_index_1.rs:212`
19. **error** `E0599` - 8 occurrences
   - Message: no method named `includes` found for enum `SmeltUnknown` in the current scope
   - Examples:
     - `src/parse_index_1.rs:254`
     - `src/parse_index_1.rs:256`
     - `src/parse_index_1.rs:384`
     - `src/parse_index_1.rs:386`
     - `src/parse_index_1.rs:521`
20. **warning** `unused_assignments` - 8 occurrences
   - Message: value assigned to `day_period_enum_value` is never read
   - Examples:
     - `src/main.rs:7531`
     - `src/main.rs:7666`
     - `src/main.rs:7628`
     - `src/main.rs:7590`
     - `src/main.rs:7801`
21. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/main.rs:19241`
     - `src/main.rs:19249`
     - `src/main.rs:19259`
     - `src/main.rs:19269`
     - `src/main.rs:19331`
22. **error** `E0282` - 6 occurrences
   - Message: type annotations needed
   - Examples:
     - `src/formatRelative_index_20.rs:9`
     - `src/formatRelative_index_40.rs:7`
     - `src/formatRelative_index_54.rs:9`
     - `src/formatRelative_index_54.rs:10`
     - `src/format_index_1.rs:133`
23. **warning** `unused_assignments` - 5 occurrences
   - Message: value assigned to `date_` is never read
   - Examples:
     - `src/closestIndexTo_index_1.rs:49`
     - `src/transpose_index_1.rs:64`
     - `src/transpose_index_1.rs:55`
     - `src/transpose_index_1.rs:39`
     - `src/transpose_index_1.rs:30`
24. **error** `E0277` - 4 occurrences
   - Message: the trait bound `IsSameWeekOptions: Default` is not satisfied
   - Examples:
     - `src/formatRelative_index_54.rs:9`
     - `src/formatRelative_index_54.rs:10`
     - `src/formatRelative_index_54.rs:28`
     - `src/formatRelative_index_54.rs:32`
25. **error** `E0609` - 4 occurrences
   - Message: no field `full_token` on type `Option<HashMap<String, String>>`
   - Examples:
     - `src/parse_index_1.rs:260`
     - `src/parse_index_1.rs:390`
     - `src/parse_index_1.rs:527`
     - `src/parse_index_1.rs:657`
26. **warning** `unused_assignments` - 4 occurrences
   - Message: value assigned to `base_tz_offset` is never read
   - Examples:
     - `src/main.rs:14041`
     - `src/main.rs:14033`
     - `src/main.rs:14005`
     - `src/main.rs:13997`
27. **error** `E0277` - 3 occurrences
   - Message: `Locale` doesn't implement `Debug`
   - Examples:
     - `src/main.rs:4878`
     - `src/main.rs:5568`
     - `src/main.rs:5805`
28. **error** `E0689` - 3 occurrences
   - Message: can't call method `powf` on ambiguous numeric type `{float}`
   - Examples:
     - `src/formatRFC3339_index_1.rs:114`
     - `src/main.rs:6819`
     - `src/main.rs:21187`
29. **error** `E0277` - 2 occurrences
   - Message: a value of type `Vec<SmeltUnknown>` cannot be built from an iterator over elements of type `FormatPart`
   - Examples:
     - `src/format_index_1.rs:115`
     - `src/format_index_1.rs:119`
30. **error** `E0308` - 2 occurrences
   - Message: `if` and `else` have incompatible types
   - Examples:
     - `src/buildMatchFn_index.rs:57`
     - `src/buildMatchPatternFn_index.rs:45`
31. **error** `E0425` - 2 occurrences
   - Message: cannot find value `_smelt_tmp_2` in this scope
   - Examples:
     - `src/localize_index_14.rs:46`
     - `src/localize_index_35.rs:16`
32. **error** `E0599` - 2 occurrences
   - Message: no method named `index_of` found for struct `Vec<f64>` in the current scope
   - Examples:
     - `src/parse_index_1.rs:779`
     - `src/parse_index_1.rs:780`
33. **error** `E0599` - 2 occurrences
   - Message: no method named `test` found for enum `SmeltUnknown` in the current scope
   - Examples:
     - `src/buildMatchFn_index.rs:60`
     - `src/buildMatchFn_index.rs:127`
34. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `first_week_of_year` is never read
   - Examples:
     - `src/main.rs:19432`
     - `src/main.rs:19435`
35. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `result` is never read
   - Examples:
     - `src/formatDistance_index_50.rs:32`
     - `src/utils.rs:277`
36. **error** `E0057` - 1 occurrence
   - Message: this function takes 1 argument but 3 arguments were supplied
   - Examples:
     - `src/parse_index_1.rs:801`
37. **error** `E0277` - 1 occurrence
   - Message: cannot add `bool` to `f64`
   - Examples:
     - `src/parseISO_index_1.rs:637`
38. **error** `E0308` - 1 occurrence
   - Message: arguments to this function are incorrect
   - Examples:
     - `src/basic_3.rs:14`
39. **error** `E0369` - 1 occurrence
   - Message: cannot multiply `bool` by `{float}`
   - Examples:
     - `src/parseISO_index_1.rs:636`
40. **error** `E0382` - 1 occurrence
   - Message: use of moved value: `translate_seconds`
   - Examples:
     - `src/formatDistance_index_53.rs:227`
41. **error** `E0425` - 1 occurrence
   - Message: cannot find value `_smelt_tmp_3` in this scope
   - Examples:
     - `src/localize_index_35.rs:8`
42. **error** `E0599` - 1 occurrence
   - Message: no method named `includes` found for unit type `()` in the current scope
   - Examples:
     - `src/main.rs:16601`
43. **error** `E0599` - 1 occurrence
   - Message: the method `concat` exists for struct `Vec<SmeltUnknown>`, but its trait bounds were not satisfied
   - Examples:
     - `src/convertToFP_index.rs:28`
44. **error** `E0609` - 1 occurrence
   - Message: no field `set` on type `&Parser<Value>`
   - Examples:
     - `src/main.rs:19133`
45. **error** `E0615` - 1 occurrence
   - Message: attempted to take value of method `validate` on type `&Parser<Value>`
   - Examples:
     - `src/main.rs:19133`
46. **error** `E0618` - 1 occurrence
   - Message: expected function, found `Vec<String>`
   - Examples:
     - `src/formatDuration_index_1.rs:57`
47. **warning** `non_camel_case_types` - 1 occurrence
   - Message: type `hiLocaleNumberValuesType` should have an upper camel case name
   - Examples:
     - `src/main.rs:6331`
48. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `d` is never read
   - Examples:
     - `src/main.rs:8997`
49. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `date_time_format` is never read
   - Examples:
     - `src/longFormatters_index.rs:105`
50. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `era` is never read
   - Examples:
     - `src/main.rs:6844`
51. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `format_relative_locale` is never read
   - Examples:
     - `src/formatRelative_index_40.rs:34`
52. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `full_icu_only` is never read
   - Examples:
     - `src/test_111.rs:95`
53. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `get_operation_system_locale` is never read
   - Examples:
     - `src/test_111.rs:100`
54. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `hours` is never read
   - Examples:
     - `src/main.rs:7855`
55. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `offset` is never read
   - Examples:
     - `src/test_index.rs:29`
56. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `spanish` is never read
   - Examples:
     - `src/test_111.rs:77`
57. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `year` is never read
   - Examples:
     - `src/main.rs:6902`

## Cargo Stderr

```text
Checking date_fns_full_check v0.1.0 (/tmp/smelt_date_fns_compat_mZz3tt/dist)
error: could not compile `date_fns_full_check` (bin "date_fns_full_check") due to 2270 previous errors; 678 warnings emitted
```
