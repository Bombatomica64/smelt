# Radash utility namespace sort investigation

Date: 2026-07-11

## Failure family

Radash tests call the package free function as `utility.sort(list, getter, descending)`. Smelt's array instance-method interceptor claimed every static member named `sort` and rejected the third argument before establishing that the receiver was a list.

## Root cause and fix

Instance-method dispatch performed arity validation before namespace ownership resolution. `list_sort_call` now defers imported utility/object namespace receivers to the existing namespace member-call path before applying `Array.prototype.sort` rules.

No `SmeltUnknown` type or conversion was added.

## Probe result

The `array sort requires at most one comparator argument` blocker is gone. `array.test.ts` now reaches its next independent failure, a utility namespace `replace` call incorrectly claimed by regex replacement lowering. The aggregate remains 10 blocker classes across 7 files while advancing that file's failure frontier.
