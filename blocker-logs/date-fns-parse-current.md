# Generated Rust Test Report

- Cargo manifest: `/tmp/smelt_date_fns_parse_probe_20260527/dist/Cargo.toml`
- Focused runs: `0`
- Guard runs: `0`
- Full suite executed: `true`

## Full Suite

- Status: `failed`
- Result: `test result: FAILED. 38 passed; 209 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.91s`
- Failing tests: `209`

### Largest Failing Groups

| Failures | Test group |
| ---: | --- |
| 209 | `__smelt_module_test` |

<details>
<summary>Failing test inventory</summary>

- `__smelt_module_test::test_parse_accepts_a_timestamp_as_referencedate`
- `__smelt_module_test::test_parse_accepts_new_line_character`
- `__smelt_module_test::test_parse_am_pm_12_am`
- `__smelt_module_test::test_parse_am_pm_12_pm`
- `__smelt_module_test::test_parse_am_pm_abbreviated`
- `__smelt_module_test::test_parse_am_pm_narrow`
- `__smelt_module_test::test_parse_am_pm_noon_midnight_abbreviated`
- `__smelt_module_test::test_parse_am_pm_noon_midnight_narrow`
- `__smelt_module_test::test_parse_am_pm_noon_midnight_wide`
- `__smelt_module_test::test_parse_am_pm_wide`
- `__smelt_module_test::test_parse_calendar_year_four_digit_zero_padding`
- `__smelt_module_test::test_parse_calendar_year_numeric`
- `__smelt_module_test::test_parse_calendar_year_ordinal`
- `__smelt_module_test::test_parse_calendar_year_specified_amount_of_digits`
- `__smelt_module_test::test_parse_calendar_year_three_digit_zero_padding`
- `__smelt_module_test::test_parse_calendar_year_two_digit_numeric_year_gets_the_100_year_range_from_referencedate`
- `__smelt_module_test::test_parse_calendar_year_two_digit_numeric_year_works_as_expected`
- `__smelt_module_test::test_parse_common_formats_date_prototype_toisostring`
- `__smelt_module_test::test_parse_common_formats_iso_8601`
- `__smelt_module_test::test_parse_common_formats_iso_day_of_year_date`
- `__smelt_module_test::test_parse_common_formats_iso_week_numbering_date`
- `__smelt_module_test::test_parse_common_formats_little_endian`
- `__smelt_module_test::test_parse_common_formats_middle_endian`
- `__smelt_module_test::test_parse_context_allows_to_specify_the_context`
- `__smelt_module_test::test_parse_custom_locale_allows_to_pass_a_custom_locale`
- `__smelt_module_test::test_parse_day_of_month_numeric`
- `__smelt_module_test::test_parse_day_of_month_ordinal`
- `__smelt_module_test::test_parse_day_of_month_zero_padding`
- `__smelt_module_test::test_parse_day_of_week_formatting_abbreviated`
- `__smelt_module_test::test_parse_day_of_week_formatting_allows_to_specify_which_day_is_the_first_day_of_the_week`
- `__smelt_module_test::test_parse_day_of_week_formatting_narrow`
- `__smelt_module_test::test_parse_day_of_week_formatting_short`
- `__smelt_module_test::test_parse_day_of_week_formatting_wide`
- `__smelt_module_test::test_parse_day_of_year_numeric`
- `__smelt_module_test::test_parse_day_of_year_ordinal`
- `__smelt_module_test::test_parse_day_of_year_specified_amount_of_digits`
- `__smelt_module_test::test_parse_day_of_year_three_digit_zero_padding`
- `__smelt_module_test::test_parse_day_of_year_two_digit_zero_padding`
- `__smelt_module_test::test_parse_edge_cases_parses_normally_if_the_remaining_input_is_just_whitespace`
- `__smelt_module_test::test_parse_edge_cases_throws_rangeerror_exception_if_the_format_string_contains_an_unescaped_latin_alphabet_character`
- `__smelt_module_test::test_parse_era_abbreviated`
- `__smelt_module_test::test_parse_era_narrow`
- `__smelt_module_test::test_parse_era_parses_stand_alone_ad`
- `__smelt_module_test::test_parse_era_parses_stand_alone_bc`
- `__smelt_module_test::test_parse_era_wide`
- `__smelt_module_test::test_parse_era_with_week_numbering_year`
- `__smelt_module_test::test_parse_escapes_characters_between_the_single_quote_characters`
- `__smelt_module_test::test_parse_extended_year_four_digit_zero_padding`
- `__smelt_module_test::test_parse_extended_year_numeric`
- `__smelt_module_test::test_parse_extended_year_specified_amount_of_digits`
- `__smelt_module_test::test_parse_extended_year_three_digit_zero_padding`
- `__smelt_module_test::test_parse_extended_year_two_digit_zero_padding`
- `__smelt_module_test::test_parse_failure_returns_referencedate_if_datestring_and_formatstring_are_empty_strings`
- `__smelt_module_test::test_parse_failure_returns_referencedate_if_no_tokens_in_formatstring_are_provided`
- `__smelt_module_test::test_parse_flexible_day_period_abbreviated`
- `__smelt_module_test::test_parse_flexible_day_period_narrow`
- `__smelt_module_test::test_parse_flexible_day_period_wide`
- `__smelt_module_test::test_parse_fraction_of_second_1_100_of_second`
- `__smelt_module_test::test_parse_fraction_of_second_1_10_of_second`
- `__smelt_module_test::test_parse_fraction_of_second_millisecond`
- `__smelt_module_test::test_parse_fraction_of_second_specified_amount_of_digits`
- `__smelt_module_test::test_parse_hour_0_11_numeric`
- `__smelt_module_test::test_parse_hour_0_11_ordinal`
- `__smelt_module_test::test_parse_hour_0_11_zero_padding`
- `__smelt_module_test::test_parse_hour_0_23_numeric`
- `__smelt_module_test::test_parse_hour_0_23_ordinal`
- `__smelt_module_test::test_parse_hour_0_23_zero_padding`
- `__smelt_module_test::test_parse_hour_1_12_numeric`
- `__smelt_module_test::test_parse_hour_1_12_ordinal`
- `__smelt_module_test::test_parse_hour_1_12_zero_padding`
- `__smelt_module_test::test_parse_hour_1_24_numeric`
- `__smelt_module_test::test_parse_hour_1_24_ordinal`
- `__smelt_module_test::test_parse_hour_1_24_zero_padding`
- `__smelt_module_test::test_parse_iso_day_of_week_formatting_abbreviated`
- `__smelt_module_test::test_parse_iso_day_of_week_formatting_narrow`
- `__smelt_module_test::test_parse_iso_day_of_week_formatting_numeric`
- `__smelt_module_test::test_parse_iso_day_of_week_formatting_ordinal`
- `__smelt_module_test::test_parse_iso_day_of_week_formatting_short`
- `__smelt_module_test::test_parse_iso_day_of_week_formatting_wide`
- `__smelt_module_test::test_parse_iso_day_of_week_formatting_zero_padding`
- `__smelt_module_test::test_parse_iso_week_numbering_year_four_digit_zero_padding`
- `__smelt_module_test::test_parse_iso_week_numbering_year_numeric`
- `__smelt_module_test::test_parse_iso_week_numbering_year_specified_amount_of_digits`
- `__smelt_module_test::test_parse_iso_week_numbering_year_three_digit_zero_padding`
- `__smelt_module_test::test_parse_iso_week_numbering_year_two_digit_zero_padding`
- `__smelt_module_test::test_parse_iso_week_of_year_numeric`
- `__smelt_module_test::test_parse_iso_week_of_year_ordinal`
- `__smelt_module_test::test_parse_iso_week_of_year_zero_padding`
- `__smelt_module_test::test_parse_local_day_of_week_formatting_abbreviated`
- `__smelt_module_test::test_parse_local_day_of_week_formatting_allows_to_specify_which_day_is_the_first_day_of_the_week`
- `__smelt_module_test::test_parse_local_day_of_week_formatting_narrow`
- `__smelt_module_test::test_parse_local_day_of_week_formatting_numeric`
- `__smelt_module_test::test_parse_local_day_of_week_formatting_ordinal`
- `__smelt_module_test::test_parse_local_day_of_week_formatting_short`
- `__smelt_module_test::test_parse_local_day_of_week_formatting_wide`
- `__smelt_module_test::test_parse_local_day_of_week_formatting_zero_padding`
- `__smelt_module_test::test_parse_local_day_of_week_stand_alone_abbreviated`
- `__smelt_module_test::test_parse_local_day_of_week_stand_alone_allows_to_specify_which_day_is_the_first_day_of_the_week`
- `__smelt_module_test::test_parse_local_day_of_week_stand_alone_narrow`
- `__smelt_module_test::test_parse_local_day_of_week_stand_alone_numeric`
- `__smelt_module_test::test_parse_local_day_of_week_stand_alone_ordinal`
- `__smelt_module_test::test_parse_local_day_of_week_stand_alone_short`
- `__smelt_module_test::test_parse_local_day_of_week_stand_alone_wide`
- `__smelt_module_test::test_parse_local_day_of_week_stand_alone_zero_padding`
- `__smelt_module_test::test_parse_local_week_numbering_year_allows_to_specify_weekstartson_and_firstweekcontainsdate_in_options`
- `__smelt_module_test::test_parse_local_week_numbering_year_four_digit_zero_padding`
- `__smelt_module_test::test_parse_local_week_numbering_year_numeric`
- `__smelt_module_test::test_parse_local_week_numbering_year_ordinal`
- `__smelt_module_test::test_parse_local_week_numbering_year_specified_amount_of_digits`
- `__smelt_module_test::test_parse_local_week_numbering_year_three_digit_zero_padding`
- `__smelt_module_test::test_parse_local_week_numbering_year_two_digit_numeric_year_gets_the_100_year_range_from_referencedate`
- `__smelt_module_test::test_parse_local_week_numbering_year_two_digit_numeric_year_works_as_expected`
- `__smelt_module_test::test_parse_local_week_of_year_allows_to_specify_weekstartson_and_firstweekcontainsdate_in_options`
- `__smelt_module_test::test_parse_local_week_of_year_numeric`
- `__smelt_module_test::test_parse_local_week_of_year_ordinal`
- `__smelt_module_test::test_parse_local_week_of_year_zero_padding`
- `__smelt_module_test::test_parse_long_format_full_date`
- `__smelt_module_test::test_parse_long_format_full_date_short_time_420`
- `__smelt_module_test::test_parse_long_format_full_date_short_time_424`
- `__smelt_module_test::test_parse_long_format_long_date`
- `__smelt_module_test::test_parse_long_format_long_date_short_time_419`
- `__smelt_module_test::test_parse_long_format_long_date_short_time_423`
- `__smelt_module_test::test_parse_long_format_medium_date`
- `__smelt_module_test::test_parse_long_format_medium_date_short_time_418`
- `__smelt_module_test::test_parse_long_format_medium_date_short_time_422`
- `__smelt_module_test::test_parse_long_format_medium_time`
- `__smelt_module_test::test_parse_long_format_short_date`
- `__smelt_module_test::test_parse_long_format_short_date_short_time_417`
- `__smelt_module_test::test_parse_long_format_short_date_short_time_421`
- `__smelt_module_test::test_parse_long_format_short_time`
- `__smelt_module_test::test_parse_milliseconds_timestamp_numeric`
- `__smelt_module_test::test_parse_milliseconds_timestamp_specified_amount_of_digits`
- `__smelt_module_test::test_parse_milliseconds_timestamp_throws_an_error_when_it_is_used_after_any_token`
- `__smelt_module_test::test_parse_minute_numeric`
- `__smelt_module_test::test_parse_minute_ordinal`
- `__smelt_module_test::test_parse_minute_zero_padding`
- `__smelt_module_test::test_parse_month_formatting_abbreviated`
- `__smelt_module_test::test_parse_month_formatting_narrow`
- `__smelt_module_test::test_parse_month_formatting_numeric`
- `__smelt_module_test::test_parse_month_formatting_ordinal`
- `__smelt_module_test::test_parse_month_formatting_wide`
- `__smelt_module_test::test_parse_month_formatting_zero_padding`
- `__smelt_module_test::test_parse_month_stand_alone_abbreviated`
- `__smelt_module_test::test_parse_month_stand_alone_narrow`
- `__smelt_module_test::test_parse_month_stand_alone_numeric`
- `__smelt_module_test::test_parse_month_stand_alone_ordinal`
- `__smelt_module_test::test_parse_month_stand_alone_wide`
- `__smelt_module_test::test_parse_month_stand_alone_zero_padding`
- `__smelt_module_test::test_parse_priority_units_of_lower_priority_don_t_overwrite_values_of_higher_priority`
- `__smelt_module_test::test_parse_quarter_formatting_abbreviated`
- `__smelt_module_test::test_parse_quarter_formatting_narrow`
- `__smelt_module_test::test_parse_quarter_formatting_numeric`
- `__smelt_module_test::test_parse_quarter_formatting_ordinal`
- `__smelt_module_test::test_parse_quarter_formatting_wide`
- `__smelt_module_test::test_parse_quarter_formatting_zero_padding`
- `__smelt_module_test::test_parse_quarter_stand_alone_abbreviated`
- `__smelt_module_test::test_parse_quarter_stand_alone_narrow`
- `__smelt_module_test::test_parse_quarter_stand_alone_numeric`
- `__smelt_module_test::test_parse_quarter_stand_alone_ordinal`
- `__smelt_module_test::test_parse_quarter_stand_alone_wide`
- `__smelt_module_test::test_parse_quarter_stand_alone_zero_padding`
- `__smelt_module_test::test_parse_quarter_with_following_year_first_quarter`
- `__smelt_module_test::test_parse_quarter_with_following_year_fourth_quarter`
- `__smelt_module_test::test_parse_quarter_with_following_year_second_quarter`
- `__smelt_module_test::test_parse_quarter_with_following_year_third_quarter`
- `__smelt_module_test::test_parse_second_numeric`
- `__smelt_module_test::test_parse_second_ordinal`
- `__smelt_module_test::test_parse_second_zero_padding`
- `__smelt_module_test::test_parse_seconds_timestamp_numeric`
- `__smelt_module_test::test_parse_seconds_timestamp_specified_amount_of_digits`
- `__smelt_module_test::test_parse_seconds_timestamp_throws_an_error_when_it_is_used_after_any_token`
- `__smelt_module_test::test_parse_time_zones_properly_parses_dates_around_dst_transitions`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_o_z_x_gmt`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_o_z_x_hours`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_o_z_x_hours_and_minutes`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_o_z_xx_gmt`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_o_z_xx_hours_and_minutes`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_o_z_xxx_gmt`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_o_z_xxx_hours_and_minutes`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_o_z_xxxx_gmt`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_o_z_xxxx_hours_and_minutes`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_o_z_xxxx_hours_minutes_and_seconds`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_o_z_xxxxx_gmt`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_o_z_xxxxx_hours_and_minutes`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_o_z_xxxxx_hours_minutes_and_seconds`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_z_x_gmt`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_z_x_hours`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_z_x_hours_and_minutes`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_z_xx_gmt`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_z_xx_hours_and_minutes`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_z_xxx_gmt`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_z_xxx_hours_and_minutes`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_z_xxxx_gmt`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_z_xxxx_hours_and_minutes`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_z_xxxx_hours_minutes_and_seconds`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_z_xxxxx_gmt`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_z_xxxxx_hours_and_minutes`
- `__smelt_module_test::test_parse_timezone_iso_8601_w_z_xxxxx_hours_minutes_and_seconds`
- `__smelt_module_test::test_parse_two_single_quote_characters_are_transformed_into_a_real_single_quote`
- `__smelt_module_test::test_parse_useadditionalweekyeartokens_and_useadditionaldayofyeartokens_options_allows_d_token_if_useadditionaldayofyeartokens_is_set_to_true`
- `__smelt_module_test::test_parse_useadditionalweekyeartokens_and_useadditionaldayofyeartokens_options_allows_dd_token_if_useadditionaldayofyeartokens_is_set_to_true`
- `__smelt_module_test::test_parse_useadditionalweekyeartokens_and_useadditionaldayofyeartokens_options_allows_yy_token_if_useadditionalweekyeartokens_is_set_to_true`
- `__smelt_module_test::test_parse_useadditionalweekyeartokens_and_useadditionaldayofyeartokens_options_allows_yyyy_token_if_useadditionalweekyeartokens_is_set_to_true`
- `__smelt_module_test::test_parse_useadditionalweekyeartokens_and_useadditionaldayofyeartokens_options_throws_an_error_if_yy_token_is_used`
- `__smelt_module_test::test_parse_useadditionalweekyeartokens_and_useadditionaldayofyeartokens_options_throws_an_error_if_yyyy_token_is_used`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_calendar_year_works_correctly_for_two_digit_year_zero`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_day_of_month_parses_29th_of_february_of_leap_year`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_day_of_year_parses_366th_day_of_leap_year`
- `__smelt_module_test::test_parse_with_options_strictvalidation_true_local_week_numbering_year_works_correctly_for_two_digit_year_zero`

</details>

## Compiler Diagnostics

### Rust Diagnostics

- Cargo manifest: `/tmp/smelt_date_fns_parse_probe_20260527/dist/Cargo.toml`
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
     - `src/main.rs:934`
     - `src/main.rs:1045`
     - `src/main.rs:1098`
     - `src/main.rs:1112`
     - `src/main.rs:1350`
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
     - `src/main.rs:4791`
     - `src/main.rs:4824`
     - `src/main.rs:4831`
6. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around function argument
   - Examples:
     - `src/main.rs:3313`
     - `src/main.rs:3321`
     - `src/main.rs:3331`
     - `src/main.rs:3341`
     - `src/main.rs:3413`
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
     - `src/buildLocalizeFn_index.rs:682`
     - `src/buildLocalizeFn_index.rs:52`
10. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `date_time_format` is never read
   - Examples:
     - `src/longFormatters_index.rs:106`
11. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `flags` is never read
   - Examples:
     - `src/main.rs:3291`
12. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `offset` is never read
   - Examples:
     - `src/test_index.rs:30`

## Cargo Stderr

```text
Compiling proc-macro2 v1.0.106
   Compiling quote v1.0.45
   Compiling unicode-ident v1.0.24
   Compiling autocfg v1.5.1
    Checking memchr v2.8.1
    Checking siphasher v1.0.3
    Checking regex-syntax v0.8.10
    Checking bit-vec v0.8.0
   Compiling chrono-tz v0.10.4
    Checking phf_shared v0.12.1
    Checking iana-time-zone v0.1.65
    Checking bit-set v0.8.0
    Checking phf v0.12.1
    Checking pin-project-lite v0.2.17
   Compiling num-traits v0.2.19
    Checking aho-corasick v1.1.4
   Compiling syn v2.0.117
    Checking chrono v0.4.44
    Checking regex-automata v0.4.14
   Compiling tokio-macros v2.7.0
    Checking fancy-regex v0.14.0
    Checking regex v1.12.3
    Checking tokio v1.52.3
    Checking date_fns_parse_probe v0.1.0 (/tmp/smelt_date_fns_parse_probe_20260527/dist)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 01s
```
