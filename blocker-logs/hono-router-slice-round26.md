# Rust Diagnostics

- Cargo manifest: `third_party/hono/dist-routers/Cargo.toml`
- Cargo check: `failed`
- Errors: `6`
- Warnings: `222`

## Summary By Code

1. **warning** `unused_assignments` - 102 diagnostics
2. **warning** `unused_parens` - 58 diagnostics
3. **warning** `unused_mut` - 51 diagnostics
4. **warning** `unused_doc_comments` - 7 diagnostics
5. **error** `E0308` - 4 diagnostics
6. **warning** `non_camel_case_types` - 4 diagnostics
7. **error** `E0277` - 2 diagnostics

## Groups

1. **warning** `unused_mut` - 51 occurrences
   - Message: variable does not need to be mutable
   - Examples:
     - `src/matcher.rs:11`
     - `src/prepared_router.rs:135`
     - `src/prepared_router.rs:9`
     - `src/prepared_router.rs:11`
     - `src/prepared_router.rs:24`
2. **warning** `unused_parens` - 49 occurrences
   - Message: unnecessary parentheses around assigned value
   - Examples:
     - `src/matcher.rs:104`
     - `src/main.rs:28767`
     - `src/main.rs:29025`
     - `src/main.rs:29268`
     - `src/main.rs:29599`
3. **warning** `unused_assignments` - 32 occurrences
   - Message: value assigned to `offset` is never read
   - Examples:
     - `src/main.rs:41969`
     - `src/main.rs:41711`
     - `src/main.rs:41137`
     - `src/main.rs:40879`
     - `src/main.rs:40243`
4. **warning** `unused_assignments` - 32 occurrences
   - Message: value assigned to `p` is never read
   - Examples:
     - `src/main.rs:41971`
     - `src/main.rs:41713`
     - `src/main.rs:41139`
     - `src/main.rs:40881`
     - `src/main.rs:40245`
5. **warning** `unused_assignments` - 24 occurrences
   - Message: value assigned to `value` is never read
   - Examples:
     - `src/url.rs:3657`
     - `src/url.rs:3361`
     - `src/url.rs:3065`
     - `src/url.rs:2761`
     - `src/url.rs:2465`
6. **warning** `unused_parens` - 8 occurrences
   - Message: unnecessary parentheses around type
   - Examples:
     - `src/main.rs:2800`
     - `src/main.rs:3618`
     - `src/main.rs:3760`
     - `src/main.rs:3821`
     - `src/main.rs:3882`
7. **warning** `unused_doc_comments` - 7 occurrences
   - Message: unused doc comment
   - Examples:
     - `src/main.rs:1498`
     - `src/main.rs:1511`
     - `src/main.rs:1855`
     - `src/main.rs:2145`
     - `src/main.rs:2154`
8. **warning** `unused_assignments` - 5 occurrences
   - Message: value assigned to `next_node` is never read
   - Examples:
     - `src/main.rs:18857`
     - `src/main.rs:15402`
     - `src/main.rs:11945`
     - `src/main.rs:8491`
     - `src/main.rs:5036`
9. **error** `E0308` - 4 occurrences
   - Message: mismatched types
   - Examples:
     - `src/main.rs:22750`
     - `src/main.rs:22753`
     - `src/main.rs:23466`
     - `src/main.rs:24682`
10. **warning** `unused_assignments` - 3 occurrences
   - Message: value assigned to `i_1` is never read
   - Examples:
     - `src/main.rs:24864`
     - `src/main.rs:24821`
     - `src/main.rs:24800`
11. **error** `E0277` - 2 occurrences
   - Message: a value of type `SmeltRecord<String, SmeltRecord<String, SmeltList<(..., ...)>>>` cannot be built from an iterator over elements of type `(std::string::String, SmeltRecord<std::string::String, SmeltList<(T, std::string::String)>>)`
   - Examples:
     - `src/main.rs:22750`
     - `src/main.rs:22753`
12. **warning** `non_camel_case_types` - 1 occurrence
   - Message: type `Node_1Inner` should have an upper camel case name
   - Examples:
     - `src/main.rs:3671`
13. **warning** `non_camel_case_types` - 1 occurrence
   - Message: type `Node_1` should have an upper camel case name
   - Examples:
     - `src/main.rs:3668`
14. **warning** `non_camel_case_types` - 1 occurrence
   - Message: type `__smelt_anon_class_3050Inner` should have an upper camel case name
   - Examples:
     - `src/main.rs:3876`
15. **warning** `non_camel_case_types` - 1 occurrence
   - Message: type `__smelt_anon_class_3050` should have an upper camel case name
   - Examples:
     - `src/main.rs:3874`
16. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `e` is never read
   - Examples:
     - `src/main.rs:24745`
17. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `empty_params` is never read
   - Examples:
     - `src/node.rs:10`
18. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `handler_data` is never read
   - Examples:
     - `src/main.rs:23871`
19. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `indexes` is never read
   - Examples:
     - `src/main.rs:23912`
20. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `m_1` is never read
   - Examples:
     - `src/main.rs:23938`
21. **warning** `unused_assignments` - 1 occurrence
   - Message: value assigned to `map` is never read
   - Examples:
     - `src/main.rs:23929`
22. **warning** `unused_parens` - 1 occurrence
   - Message: unnecessary parentheses around method argument
   - Examples:
     - `src/main.rs:23962`

## Cargo Stderr

```text
Checking hono_routers_probe v0.1.0 (/home/user/smelt/.claude/worktrees/agent-a1dc6a2aff708b0a8/third_party/hono/dist-routers)
error: could not compile `hono_routers_probe` (bin "hono_routers_probe") due to 6 previous errors; 222 warnings emitted
```
