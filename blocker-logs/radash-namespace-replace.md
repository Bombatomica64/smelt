# Radash utility namespace replace investigation

Date: 2026-07-11

## Failure family

Radash tests call `utility.replace(list, replacement, predicate)`. Both regex replacement and literal string replacement interceptors claimed every static member named `replace` and rejected the collection-helper signature before resolving the namespace receiver.

## Root cause and fix

Both string/regex replacement paths now defer imported utility/object namespace receivers before instance-method arity validation. The existing namespace member-call path remains responsible for lowering the free function and all its arguments.

No `SmeltUnknown` type or conversion was added.

## Probe result

The replacement arity blockers are gone. `array.test.ts` advances to a later independent `never` assertion failure. The aggregate remains 10 blocker classes across 7 files while moving that file's failure frontier.
