---
name: Standard language feature
about: Track a TypeScript/Python stdlib or built-in language feature that should lower through HIR/MIR
title: "stdlib: support <feature>"
labels: ["stdlib", "frontend", "hir"]
assignees: ""
---

## Feature

Name the source feature and API surface.

Examples:
- TypeScript: `console.log`, `Array.prototype.map`, `Promise.all`
- Python: `print`, `list.append`, `dict.get`

## Source Semantics

Describe the behavior we need to preserve at the HIR boundary.

- Accepted call/property forms:
- Return type:
- Side effects:
- Error/exception behavior:
- Async behavior:

## HIR Shape

Describe the language-neutral HIR representation.

- Expression/statement shape:
- Required `Type` mappings:
- Symbol/original-name behavior:
- Rejected cases:

## MIR Shape

Describe the target-neutral MIR lowering.

- `BuiltinFn` variants needed:
- Operand/result types:
- Pure SSA behavior:
- CFG/terminator implications:

## Rust Codegen Sketch

Describe likely Rust output or runtime helper.

- Direct Rust/std call:
- Runtime helper needed:
- Ownership/clone expectations:

## Tests

Add fixtures for the supported and rejected forms.

- Positive TypeScript fixtures:
- Positive Python fixtures:
- Negative fixtures:
- HIR compact output expectations:
- MIR validation expectations:

## Open Questions

List semantic ambiguities or compatibility choices.
