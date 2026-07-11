# Radash parseFloat investigation

Date: 2026-07-11

## Selected failure family

`radash/src/number.ts` called the global JavaScript `parseFloat` with a value typed as `any`. Smelt rejected the call because both global `parseFloat` and `Number.parseFloat` required an already-string operand.

## Root cause

JavaScript applies string coercion before parsing. Smelt represented the operation as the generic `ToFloat` primitive cast and therefore had no place to preserve that coercion step or JavaScript's non-throwing invalid-input result.

## Implemented lowering

- Both call spellings share `parse_float_operand`.
- Existing string operands pass through unchanged.
- Other supported primitive or erased operands lower through an explicit `ToString` cast.
- A distinct `ParseFloat` cast emits a non-panicking parse result (`NaN` on invalid complete input), leaving generic/Python `ToFloat` behavior unchanged.
- First-class global `parseFloat` values use the same distinct cast when synthesized as callbacks.
- No `SmeltUnknown` variants or conversions were added. The tested `any` value uses the pre-existing erased-source boundary.

This slice does not yet implement ECMAScript's longest-valid-prefix parsing for strings such as `"12px"`; Rust's complete-string parser returns `NaN` for that case. That broader runtime semantic gap is independent of the radash coercion blocker addressed here.

## Probe comparison

The radash-only probe used the same job fixture and Smelt binary before and after the change.

| Metric | Before | After |
| --- | ---: | ---: |
| Blocker classes | 12 | 11 |
| Files with blockers | 8 | 7 |

The `parseFloat requires a string argument` diagnostic is absent after the change. Remaining blockers belong to other failure families such as update-expression arguments, Promise executors, local callable values, dynamic Date construction, and callback methods.
