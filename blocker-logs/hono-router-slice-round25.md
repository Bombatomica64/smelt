# Rust Diagnostics

- Cargo manifest: `third_party/hono/dist-routers/Cargo.toml`
- Cargo check: `failed`
- Errors: `56`
- Warnings: `217`

## Summary By Code

1. **warning** `unused_assignments` - 102 diagnostics
2. **warning** `unused_parens` - 58 diagnostics
3. **error** `E0308` - 50 diagnostics
4. **warning** `unused_mut` - 46 diagnostics
5. **warning** `unused_doc_comments` - 7 diagnostics
6. **warning** `non_camel_case_types` - 4 diagnostics
7. **error** `E0277` - 2 diagnostics
8. **error** `E0424` - 2 diagnostics
9. **error** `E0283` - 1 diagnostic
10. **error** `E0615` - 1 diagnostic

## Groups

1. **error** `E0308` - 50 occurrences
   - Message: mismatched types
   - Examples:
     - `src/main.rs:21520`
     - `src/main.rs:21523`
     - `src/main.rs:21541`
     - `src/main.rs:22113`
     - `src/main.rs:22119`
2. **warning** `unused_parens` - 49 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/matcher.rs:104`
     - `src/main.rs:27384`
     - `src/main.rs:27626`
     - `src/main.rs:27853`
     - `src/main.rs:28168`
3. **warning** `unused_mut` - 46 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/matcher.rs:11`
     - `src/router_3.rs:64`
     - `src/url.rs:26`
     - `src/url.rs:86`
     - `src/url.rs:87`
4. **warning** `unused_assignments` - 32 occurrences
   - Message: value assigned to `offset` is never read
   - Examples:
     - `src/main.rs:39850`
     - `src/main.rs:39608`
     - `src/main.rs:39066`
     - `src/main.rs:38824`
     - `src/main.rs:38220`
5. **warning** `unused_assignments` - 32 occurrences
   - Message: value assigned to `p` is never read
   - Examples:
     - `src/main.rs:39852`
     - `src/main.rs:39610`
     - `src/main.rs:39068`
     - `src/main.rs:38826`
     - `src/main.rs:38222`
6. **warning** `unused_assignments` - 24 occurrences
   - Message: value assigned to `value` is never read
   - Examples:
     - `src/url.rs:3586`
     - `src/url.rs:3296`
     - `src/url.rs:3006`
     - `src/url.rs:2708`
     - `src/url.rs:2418`
7. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around type
   - Examples:
     - `src/main.rs:2788`
     - `src/main.rs:3606`
     - `src/main.rs:3748`
     - `src/main.rs:3809`
     - `src/main.rs:3870`
8. **warning** `unused_doc_comments` - 7 occurrences
   - Message: unused doc comment
   - Examples:
     - `src/main.rs:1486`
     - `src/main.rs:1499`
     - `src/main.rs:1843`
     - `src/main.rs:2133`
     - `src/main.rs:2142`
9. **warning** `unused_assignments` - 5 occurrences
   - Message: value assigned to `next_node` is never read
   - Examples:
     - `src/main.rs:17870`
     - `src/main.rs:14658`
     - `src/main.rs:11444`
     - `src/main.rs:8233`
     - `src/main.rs:5021`
10. **warning** `unused_assignments` - 3 occurrences
   - Message: value assigned to `i_1` is never read
   - Examples:
     - `src/main.rs:23630`
     - `src/main.rs:23587`
     - `src/main.rs:23566`
11. **error** `E0277` - 2 occurrences
   - Message: a value of type `SmeltRecord<String, SmeltRecord<String, SmeltList<(..., ...)>>>` cannot be built from an iterator over elements of type `(std::string::String, SmeltRecord<std::string::String, SmeltList<(T, std::string::String)>>)`
   - Examples:
     - `src/main.rs:21520`
     - `src/main.rs:21523`
12. **error** `E0424` - 2 occurrences
   - Message: expected unit struct, unit variant or constant, found local variable `self`
   - Examples:
     - `src/main.rs:21114`
     - `src/main.rs:21169`
13. **error** `E0283` - 1 occurrence
   - Message: type annotations needed for `__smelt_anon_class_3050<_>`
   - Examples:
     - `src/prepared_router.rs:120`
14. **error** `E0615` - 1 occurrence
   - Message: attempted to take value of method `build_all_matchers` on type `&__smelt_anon_class_3050<T>`
   - Examples:
     - `src/main.rs:23471`
15. **warning** `non_camel_case_types` - 1 occurrence
   - Message: type `Node_1Inner` should have an upper camel case name
   - Examples:
     - `src/main.rs:3659`
16. **warning** `non_camel_case_types` - 1 occurrence
   - Message: type `Node_1` should have an upper camel case name
   - Examples:
     - `src/main.rs:3656`
17. **warning** `non_camel_case_types` - 1 occurrence
   - Message: type `__smelt_anon_class_3050Inner` should have an upper camel case name
   - Examples:
     - `src/main.rs:3864`
18. **warning** `non_camel_case_types` - 1 occurrence
   - Message: type `__smelt_anon_class_3050` should have an upper camel case name
   - Examples:
     - `src/main.rs:3862`
19. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `e` is never read
   - Examples:
     - `src/main.rs:23511`
20. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `empty_params` is never read
   - Examples:
     - `src/node.rs:10`
21. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `handler_data` is never read
   - Examples:
     - `src/main.rs:22639`
22. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `indexes` is never read
   - Examples:
     - `src/main.rs:22680`
23. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `m_1` is never read
   - Examples:
     - `src/main.rs:22706`
24. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `map` is never read
   - Examples:
     - `src/main.rs:22697`
25. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/main.rs:22730`

## Cargo Stderr

```text
Checking writeable v0.6.4
    Checking litemap v0.8.3
    Checking zerofrom v0.1.8
    Checking memchr v2.8.3
    Checking yoke v0.8.3
    Checking icu_properties_data v2.3.0
    Checking icu_normalizer_data v2.3.0
    Checking smallvec v1.16.0
    Checking libc v0.2.189
    Checking zerovec v0.11.8
    Checking zerotrie v0.2.5
    Checking aho-corasick v1.1.5
    Checking phf_shared v0.12.1
    Checking num-traits v0.2.19
    Checking tinystr v0.8.4
    Checking icu_locale_core v2.3.0
    Checking potential_utf v0.1.6
    Checking regex-automata v0.4.18
    Checking icu_collections v2.3.0
    Checking serde_core v1.0.229
    Checking bit-vec v0.8.0
    Checking icu_provider v2.3.1
    Checking percent-encoding v2.3.2
    Checking iana-time-zone v0.1.65
    Checking chrono v0.4.45
    Checking icu_normalizer v2.3.0
    Checking icu_properties v2.3.0
    Checking form_urlencoded v1.2.2
    Checking bit-set v0.8.0
    Checking socket2 v0.6.5
    Checking mio v1.2.3
    Checking phf v0.12.1
    Checking idna_adapter v1.2.2
    Checking idna v1.1.0
    Checking zmij v1.0.23
    Checking tokio v1.53.1
    Checking url v2.5.8
    Checking serde_json v1.0.151
    Checking fancy-regex v0.14.0
    Checking regex v1.13.1
    Checking serde v1.0.229
    Checking chrono-tz v0.10.4
    Checking hono_routers_probe v0.1.0 (/home/user/smelt/.claude/worktrees/agent-a1dc6a2aff708b0a8/third_party/hono/dist-routers)
error: could not compile `hono_routers_probe` (bin "hono_routers_probe") due to 56 previous errors; 217 warnings emitted
```
