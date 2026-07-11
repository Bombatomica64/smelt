# Radash runtime-erased never assertion investigation

Date: 2026-07-11

## Failure family

Radash deliberately passes `null as unknown as never` to exercise null handling through a generic API. Smelt rejected the assertion because it treated the target as storage that must materialize a `never` value.

## Root cause and fix

TypeScript type assertions are erased at runtime. An impossible assertion does not construct its target type; it evaluates the original operand. Assertions whose target requires `never` now preserve the operand's concrete/runtime shape. Declarations and non-empty containers whose actual storage type requires `never` remain rejected by the existing declaration/literal checks.

No `SmeltUnknown` variant or conversion was added. The existing `unknown` assertion in the source remains its explicit runtime boundary.

## Probe result

The `type assertion cannot construct a never value` blocker is gone. `array.test.ts` advances to a later independent namespace/instance `shift` arity failure. The aggregate remains 10 blocker classes across 7 files while advancing the file frontier.
