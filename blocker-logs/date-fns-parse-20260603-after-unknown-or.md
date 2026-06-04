# Generated Rust Test Report

- Cargo manifest: `/tmp/smelt_date_fns_parse_probe_20260603/dist/Cargo.toml`
- Focused runs: `0`
- Guard runs: `0`
- Full suite executed: `true`

## Full Suite

- Status: `failed`
- Result: `test result: FAILED. 158 passed; 89 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.38s`
- Failing tests: `89`

### Largest Failing Groups

| Failures | Test group |
| ---: | --- |
| 89 | `__smelt_module_test` |

### Delta From Baseline

- Baseline report: `blocker-logs/date-fns-parse-20260603-after-default-options.md`
- Resolved tests: `0`
- Newly failing tests: `0`

<details>
<summary>Failing test inventory</summary>

- `__smelt_module_test::test_parse_accepts_a_timestamp_as_referencedate`
- `__smelt_module_test::test_parse_am_pm_12_pm`
- `__smelt_module_test::test_parse_am_pm_noon_midnight_abbreviated`
- `__smelt_module_test::test_parse_am_pm_wide`
- `__smelt_module_test::test_parse_common_formats_middle_endian`
- `__smelt_module_test::test_parse_context_allows_to_specify_the_context`
- `__smelt_module_test::test_parse_custom_locale_allows_to_pass_a_custom_locale`
- `__smelt_module_test::test_parse_day_of_week_formatting_abbreviated`
- `__smelt_module_test::test_parse_day_of_week_formatting_allows_to_specify_which_day_is_the_first_day_of_the_week`
- `__smelt_module_test::test_parse_day_of_week_formatting_narrow`
- `__smelt_module_test::test_parse_day_of_week_formatting_short`
- `__smelt_module_test::test_parse_day_of_week_formatting_wide`
- `__smelt_module_test::test_parse_era_abbreviated`
- `__smelt_module_test::test_parse_era_narrow`
- `__smelt_module_test::test_parse_era_parses_stand_alone_ad`
- `__smelt_module_test::test_parse_era_parses_stand_alone_bc`
- `__smelt_module_test::test_parse_era_wide`
- `__smelt_module_test::test_parse_era_with_week_numbering_year`
- `__smelt_module_test::test_parse_escapes_characters_between_the_single_quote_characters`
- `__smelt_module_test::test_parse_flexible_day_period_narrow`
- `__smelt_module_test::test_parse_flexible_day_period_wide`
- `__smelt_module_test::test_parse_iso_day_of_week_formatting_abbreviated`
- `__smelt_module_test::test_parse_iso_day_of_week_formatting_narrow`
- `__smelt_module_test::test_parse_iso_day_of_week_formatting_short`
- `__smelt_module_test::test_parse_iso_day_of_week_formatting_wide`
- `__smelt_module_test::test_parse_local_day_of_week_formatting_abbreviated`
- `__smelt_module_test::test_parse_local_day_of_week_formatting_narrow`
- `__smelt_module_test::test_parse_local_day_of_week_formatting_short`
- `__smelt_module_test::test_parse_local_day_of_week_formatting_wide`
- `__smelt_module_test::test_parse_local_day_of_week_stand_alone_abbreviated`
- `__smelt_module_test::test_parse_local_day_of_week_stand_alone_narrow`
- `__smelt_module_test::test_parse_local_day_of_week_stand_alone_short`
- `__smelt_module_test::test_parse_local_day_of_week_stand_alone_wide`
- `__smelt_module_test::test_parse_long_format_full_date`
- `__smelt_module_test::test_parse_long_format_full_date_short_time_420`
- `__smelt_module_test::test_parse_long_format_full_date_short_time_424`
- `__smelt_module_test::test_parse_long_format_long_date`
- `__smelt_module_test::test_parse_long_format_long_date_short_time_419`
- `__smelt_module_test::test_parse_long_format_long_date_short_time_423`
- `__smelt_module_test::test_parse_long_format_medium_date`
- `__smelt_module_test::test_parse_long_format_medium_date_short_time_418`
- `__smelt_module_test::test_parse_long_format_medium_date_short_time_422`
- `__smelt_module_test::test_parse_month_formatting_abbreviated`
- `__smelt_module_test::test_parse_month_formatting_narrow`
- `__smelt_module_test::test_parse_month_formatting_wide`
- `__smelt_module_test::test_parse_month_stand_alone_abbreviated`
- `__smelt_module_test::test_parse_month_stand_alone_narrow`
- `__smelt_module_test::test_parse_month_stand_alone_wide`
- `__smelt_module_test::test_parse_quarter_formatting_abbreviated`
- `__smelt_module_test::test_parse_quarter_formatting_narrow`
- `__smelt_module_test::test_parse_quarter_formatting_wide`
- `__smelt_module_test::test_parse_quarter_stand_alone_abbreviated`
- `__smelt_module_test::test_parse_quarter_stand_alone_narrow`
- `__smelt_module_test::test_parse_quarter_stand_alone_wide`
- `__smelt_module_test::test_parse_quarter_with_following_year_first_quarter`
- `__smelt_module_test::test_parse_quarter_with_following_year_fourth_quarter`
- `__smelt_module_test::test_parse_quarter_with_following_year_second_quarter`
- `__smelt_module_test::test_parse_quarter_with_following_year_third_quarter`
- `__smelt_module_test::test_parse_time_zones_properly_parses_dates_around_dst_transitions`
- `__smelt_module_test::test_parse_useadditionalweekyeartokens_and_useadditionaldayofyeartokens_options_throws_an_error_if_d_token_is_used`
- `__smelt_module_test::test_parse_useadditionalweekyeartokens_and_useadditionaldayofyeartokens_options_throws_an_error_if_dd_token_is_used`
- `__smelt_module_test::test_parse_useadditionalweekyeartokens_and_useadditionaldayofyeartokens_options_throws_an_error_if_yy_token_is_used`
- `__smelt_module_test::test_parse_useadditionalweekyeartokens_and_useadditionaldayofyeartokens_options_throws_an_error_if_yyyy_token_is_used`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_calendar_year_returns_invalid_date_for_year_zero`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_day_of_month_returns_invalid_date_for_29th_of_february_of_non_leap_year`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_day_of_month_returns_invalid_date_for_invalid_day_of_the_month`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_day_of_year_returns_invalid_date_for_366th_day_of_non_leap_year`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_day_of_year_returns_invalid_date_for_invalid_day_of_the_year`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_hour_0_11_returns_invalid_date_for_invalid_hour`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_hour_0_23_returns_invalid_date_for_invalid_hour`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_hour_1_12_returns_invalid_date_for_hour_zero`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_hour_1_12_returns_invalid_date_for_invalid_hour`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_hour_1_24_returns_invalid_date_for_hour_zero`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_hour_1_24_returns_invalid_date_for_invalid_hour`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_iso_day_of_week_formatting_returns_invalid_date_for_day_zero`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_iso_day_of_week_formatting_returns_invalid_date_for_eight_day_of_week`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_iso_week_of_year_returns_invalid_date_for_invalid_week`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_local_day_of_week_formatting_returns_invalid_date_for_day_zero`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_local_day_of_week_formatting_returns_invalid_date_for_eight_day_of_week`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_local_day_of_week_stand_alone_returns_invalid_date_for_day_zero`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_local_day_of_week_stand_alone_returns_invalid_date_for_eight_day_of_week`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_local_week_numbering_year_returns_invalid_date_for_year_zero`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_local_week_of_year_returns_invalid_date_for_invalid_week`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_minute_returns_invalid_date_for_invalid_minute`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_month_formatting_returns_invalid_date_for_invalid_month`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_month_stand_alone_returns_invalid_date_for_invalid_month`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_quarter_formatting_returns_invalid_date_for_invalid_quarter`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_quarter_stand_alone_returns_invalid_date_for_invalid_quarter`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_second_returns_invalid_date_for_invalid_second`

</details>

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `/tmp/smelt_date_fns_parse_probe_20260603/dist/Cargo.toml`
- Cargo check: `passed`
- Errors: `0`
- Warnings: `311`

## Summary By Code

1. **warning** `unused_mut` - 220 diagnostics
2. **warning** `unused_parens` - 52 diagnostics
3. **warning** `unused_assignments` - 39 diagnostics

## Groups

1. **warning** `unused_mut` - 220 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/buildFormatLongFn_index.rs:10`
     - `src/buildFormatLongFn_index.rs:7`
     - `src/buildLocalizeFn_index.rs:7`
     - `src/buildMatchFn_index.rs:54`
     - `src/buildMatchFn_index.rs:100`
2. **warning** `unused_assignments` - 32 occurrences
   - Message: value assigned to `value` is never read
   - Examples:
     - `src/buildMatchFn_index.rs:2349`
     - `src/buildMatchFn_index.rs:2282`
     - `src/buildMatchFn_index.rs:2208`
     - `src/buildMatchFn_index.rs:2141`
     - `src/buildMatchFn_index.rs:2064`
3. **warning** `unused_parens` - 20 occurrences
   - Message: unnecessary parentheses around type
   - Examples:
     - `src/main.rs:972`
     - `src/main.rs:1083`
     - `src/main.rs:1136`
     - `src/main.rs:1150`
     - `src/main.rs:1388`
4. **warning** `unused_parens` - 13 occurrences
   - Message: unnecessary parentheses around block return value
   - Examples:
     - `src/parse_index.rs:290`
     - `src/parse_index.rs:3424`
     - `src/parse_index.rs:3426`
     - `src/parse_index.rs:3436`
     - `src/parse_index.rs:3438`
5. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around `if` condition
   - Examples:
     - `src/parse_index.rs:423`
     - `src/parse_index.rs:425`
     - `src/main.rs:5239`
     - `src/main.rs:5272`
     - `src/main.rs:5279`
6. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/main.rs:3761`
     - `src/main.rs:3769`
     - `src/main.rs:3779`
     - `src/main.rs:3789`
     - `src/main.rs:3861`
7. **warning** `unused_parens` - 3 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/parse_index.rs:3416`
     - `src/parse_index.rs:3427`
     - `src/parse_index.rs:3439`
8. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `result` is never read
   - Examples:
     - `src/formatDistance_index.rs:21`
     - `src/utils.rs:623`
9. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `values_array` is never read
   - Examples:
     - `src/buildLocalizeFn_index.rs:665`
     - `src/buildLocalizeFn_index.rs:51`
10. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `date_time_format` is never read
   - Examples:
     - `src/longFormatters_index.rs:106`
11. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `flags` is never read
   - Examples:
     - `src/main.rs:3739`
12. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `offset` is never read
   - Examples:
     - `src/test_index.rs:32`

## Cargo Stderr

```text
Checking date_fns_parse_probe v0.1.0 (/tmp/smelt_date_fns_parse_probe_20260603/dist)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3m 39s
```
