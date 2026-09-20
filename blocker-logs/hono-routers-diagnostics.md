# Rust Diagnostics

- Cargo manifest: `third_party/hono/dist-routers/Cargo.toml`
- Cargo check: `failed`
- Errors: `497`
- Warnings: `99`

## Summary By Code

1. **error** `E0308` - 429 diagnostics
2. **warning** `unused_parens` - 59 diagnostics
3. **warning** `unused_mut` - 27 diagnostics
4. **error** `E0061` - 19 diagnostics
5. **error** `E0277` - 19 diagnostics
6. **error** `E0609` - 13 diagnostics
7. **error** `E0599` - 7 diagnostics
8. **warning** `unused_assignments` - 7 diagnostics
9. **error** `E0560` - 6 diagnostics
10. **warning** `unused_doc_comments` - 5 diagnostics
11. **error** `E0615` - 3 diagnostics
12. **error** `E0369` - 1 diagnostic
13. **warning** `non_camel_case_types` - 1 diagnostic

## Groups

1. **error** `E0308` - 429 occurrences
   - Message: mismatched types
   - Examples:
     - `src/matcher.rs:37`
     - `src/matcher.rs:69`
     - `src/prepared_router.rs:349`
     - `src/prepared_router.rs:351`
     - `src/prepared_router.rs:357`
2. **warning** `unused_parens` - 51 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/matcher.rs:104`
     - `src/main.rs:4113`
     - `src/main.rs:4246`
     - `src/main.rs:10478`
     - `src/main.rs:10719`
3. **warning** `unused_mut` - 27 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/router_3.rs:78`
     - `src/url.rs:26`
     - `src/url.rs:83`
     - `src/url.rs:88`
     - `src/url.rs:89`
4. **error** `E0061` - 19 occurrences
   - Message: this method takes 5 arguments but 4 arguments were supplied
   - Examples:
     - `src/main.rs:10358`
     - `src/main.rs:10360`
     - `src/main.rs:10382`
     - `src/main.rs:10406`
     - `src/main.rs:12008`
5. **error** `E0609` - 9 occurrences
   - Message: no field `0` on type `&__smelt_anon_class_3050<T>`
   - Examples:
     - `src/main.rs:5761`
     - `src/main.rs:5893`
     - `src/main.rs:5894`
     - `src/main.rs:5903`
     - `src/main.rs:5975`
6. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around type
   - Examples:
     - `src/main.rs:2641`
     - `src/main.rs:3402`
     - `src/main.rs:3446`
     - `src/main.rs:3506`
     - `src/main.rs:3552`
7. **error** `E0277` - 5 occurrences
   - Message: the trait bound `T: IntoSmeltUnknown` is not satisfied
   - Examples:
     - `src/main.rs:3597`
     - `src/main.rs:3640`
     - `src/main.rs:3681`
     - `src/main.rs:3681`
     - `src/main.rs:3681`
8. **error** `E0277` - 5 occurrences
   - Message: the trait bound `T: SmeltFromUnknown` is not satisfied
   - Examples:
     - `src/main.rs:3597`
     - `src/main.rs:3640`
     - `src/main.rs:3681`
     - `src/main.rs:3681`
     - `src/main.rs:3681`
9. **error** `E0599` - 5 occurrences
   - Message: no method named `into_smelt_unknown` found for tuple `(SmeltRegExp, SmeltList<SmeltList<(T, SmeltRecord<..., f64>)>>, ...)` in the current scope
   - Examples:
     - `src/main.rs:5348`
     - `src/main.rs:5349`
     - `src/main.rs:5738`
     - `src/main.rs:6590`
     - `src/main.rs:6591`
10. **warning** `unused_doc_comments` - 5 occurrences
   - Message: unused doc comment
   - Examples:
     - `src/main.rs:1401`
     - `src/main.rs:1414`
     - `src/main.rs:1993`
     - `src/main.rs:2002`
     - `src/main.rs:2011`
11. **error** `E0277` - 4 occurrences
   - Message: the trait bound `T: Clone` is not satisfied
   - Examples:
     - `src/main.rs:3597`
     - `src/main.rs:3640`
     - `src/main.rs:3681`
     - `src/main.rs:3681`
12. **error** `E0277` - 4 occurrences
   - Message: the trait bound `T: Default` is not satisfied
   - Examples:
     - `src/main.rs:3597`
     - `src/main.rs:3640`
     - `src/main.rs:3681`
     - `src/main.rs:3681`
13. **error** `E0560` - 2 occurrences
   - Message: struct `PatternRouter<SmeltUnknown>` has no field named `_routes`
   - Examples:
     - `src/main.rs:3396`
     - `src/main.rs:3396`
14. **error** `E0560` - 2 occurrences
   - Message: struct `PatternRouter<SmeltUnknown>` has no field named `_smelt_phantom`
   - Examples:
     - `src/main.rs:3396`
     - `src/main.rs:3396`
15. **error** `E0560` - 2 occurrences
   - Message: struct `PatternRouter<SmeltUnknown>` has no field named `name`
   - Examples:
     - `src/main.rs:3396`
     - `src/main.rs:3396`
16. **error** `E0609` - 2 occurrences
   - Message: no field `_routes` on type `&PatternRouter<T>`
   - Examples:
     - `src/main.rs:4113`
     - `src/main.rs:4246`
17. **error** `E0609` - 2 occurrences
   - Message: no field `name` on type `&PatternRouter<T>`
   - Examples:
     - `src/main.rs:4113`
     - `src/main.rs:4246`
18. **error** `E0615` - 2 occurrences
   - Message: attempted to take value of method `match_` on type `&SmartRouter<T>`
   - Examples:
     - `src/main.rs:6685`
     - `src/main.rs:6733`
19. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `encoded` is never read
   - Examples:
     - `src/url.rs:243`
     - `src/url_1.rs:248`
20. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `key_index_1` is never read
   - Examples:
     - `src/url.rs:245`
     - `src/url_1.rs:250`
21. **warning** `unused_assignments` - 2 occurrences
   - Message: value assigned to `results` is never read
   - Examples:
     - `src/url.rs:238`
     - `src/url_1.rs:243`
22. **error** `E0277` - 1 occurrence
   - Message: the trait bound `(T, SmeltRecord<std::string::String, f64>): SmeltFromUnknown` is not satisfied
   - Examples:
     - `src/main.rs:5735`
23. **error** `E0369` - 1 occurrence
   - Message: binary operation `==` cannot be applied to type `Node<T>`
   - Examples:
     - `src/main.rs:3681`
24. **error** `E0599` - 1 occurrence
   - Message: no method named `into_smelt_unknown` found for tuple `(SmeltList<(T, SmeltRecord<std::string::String, std::string::String>)>,)` in the current scope
   - Examples:
     - `src/main.rs:23483`
25. **error** `E0599` - 1 occurrence
   - Message: the method `clone` exists for enum `Result<Router<T>, Box<(dyn StdError + 'static)>>`, but its trait bounds were not satisfied
   - Examples:
     - `src/main.rs:6758`
26. **error** `E0615` - 1 occurrence
   - Message: attempted to take value of method `build_all_matchers` on type `&__smelt_anon_class_3050<T>`
   - Examples:
     - `src/main.rs:6577`
27. **warning** `non_camel_case_types` - 1 occurrence
   - Message: type `__smelt_anon_class_3050` should have an upper camel case name
   - Examples:
     - `src/main.rs:3546`
28. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `empty_params` is never read
   - Examples:
     - `src/node.rs:10`

## Cargo Stderr

```text
Updating crates.io index
     Locking 89 packages to latest Rust 1.96.1 compatible versions
      Adding fancy-regex v0.14.0 (available: v0.19.1)
   Compiling proc-macro2 v1.0.107
   Compiling unicode-ident v1.0.24
   Compiling quote v1.0.47
    Checking stable_deref_trait v1.2.1
    Checking writeable v0.6.4
    Checking litemap v0.8.3
    Checking utf8_iter v1.0.4
   Compiling icu_properties_data v2.3.0
   Compiling icu_normalizer_data v2.3.0
   Compiling autocfg v1.5.1
   Compiling libc v0.2.189
    Checking memchr v2.8.3
   Compiling num-traits v0.2.19
   Compiling serde_core v1.0.229
    Checking smallvec v1.16.0
    Checking aho-corasick v1.1.5
   Compiling syn v3.0.5
   Compiling syn v2.0.119
    Checking siphasher v1.0.3
   Compiling zmij v1.0.23
    Checking regex-syntax v0.8.11
    Checking phf_shared v0.12.1
   Compiling synstructure v0.13.2
    Checking regex-automata v0.4.18
   Compiling zerofrom-derive v0.1.7
    Checking zerofrom v0.1.8
   Compiling yoke-derive v0.8.2
   Compiling serde v1.0.229
    Checking iana-time-zone v0.1.65
    Checking bit-vec v0.8.0
   Compiling serde_json v1.0.151
    Checking percent-encoding v2.3.2
    Checking yoke v0.8.3
   Compiling chrono-tz v0.10.4
    Checking form_urlencoded v1.2.2
    Checking bit-set v0.8.0
    Checking chrono v0.4.45
    Checking phf v0.12.1
   Compiling zerovec-derive v0.11.6
   Compiling displaydoc v0.2.7
   Compiling tokio-macros v2.7.2
   Compiling serde_derive v1.0.229
    Checking zerotrie v0.2.5
    Checking mio v1.2.3
    Checking zerovec v0.11.8
    Checking socket2 v0.6.5
    Checking itoa v1.0.18
    Checking pin-project-lite v0.2.17
    Checking tokio v1.53.1
    Checking tinystr v0.8.4
    Checking icu_locale_core v2.3.0
    Checking potential_utf v0.1.6
    Checking icu_collections v2.3.0
    Checking icu_provider v2.3.1
    Checking fancy-regex v0.14.0
    Checking icu_properties v2.3.0
    Checking icu_normalizer v2.3.0
    Checking regex v1.13.1
    Checking idna_adapter v1.2.2
    Checking idna v1.1.0
    Checking url v2.5.8
    Checking hono_routers_probe v0.1.0 (/home/user/smelt/.claude/worktrees/agent-a1dc6a2aff708b0a8/third_party/hono/dist-routers)
error: could not compile `hono_routers_probe` (bin "hono_routers_probe") due to 497 previous errors; 99 warnings emitted
```
