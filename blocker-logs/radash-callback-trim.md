# Radash callback trim investigation

Date: 2026-07-11

## Failure family

Radash filters path segments with `path.split(...).filter(x => !!x.trim())`. Direct `String.prototype.trim` calls were supported, but the callback-body method table rejected the identical operation.

## Root cause and fix

Callback closure conversion maintained a separate set of supported method expressions and omitted zero-argument `trim`. It now emits `StringTrim(Both)` for string receivers and applies the existing explicit string coercion for erased or scoped-generic callback receivers.

No new `SmeltUnknown` type, conversion, or boundary was introduced.

## Probe result

The radash-only probe moves from 11 to 10 blocker classes. The callback-method diagnostic is absent, while the scan continues to report the two independent remaining failures in `src/object.ts`.
