# Radash first-class Array.isArray investigation

Date: 2026-07-11

## Failure family

Radash exports `Array.isArray` through a constant and calls it through the package namespace:

```ts
export const isArray = Array.isArray
_.isArray(value)
```

Direct `Array.isArray(value)` calls were modeled, but the bare static member did not become a function value. The exported constant therefore reached namespace call lowering as a non-callable item.

## Root cause and fix

Static builtin member lowering lacked a first-class `Array.isArray` adapter. The frontend now synthesizes a one-argument closure returning `UnknownIs(Array)`, matching the existing direct-call runtime probe.

The closure parameter is intentionally `Unknown`: `Array.isArray` accepts and inspects arbitrary JavaScript runtime values. This is a genuine dynamic boundary, not type-level helper plumbing. No new `SmeltUnknown` conversion or runtime variant was added.

## Probe result

The `namespace member isArray is not callable` blocker is gone from `typed.test.ts`. The scan now reaches the next independent unsupported construct in that file, a sequence expression, so the aggregate remains 11 blocker classes across 7 files while advancing the file's failure frontier.
