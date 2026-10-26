# TypeScript 7.1.0-beta type parity

Inputs: ts-pattern/Smelt.toml

**Parity: n/a** (0 of 0 statically typed expressions agree)

| verdict | count |
| --- | ---: |
| agree | 0 |
| smelt-erased | 0 |
| wider | 0 |
| mismatch | 0 |
| ts-dynamic | 0 |
| ts-deferred | 0 |
| no-checker-node | 0 |

## Smelt-erased families (HIR ← TypeScript)

None.

## Wider-type families (HIR ← TypeScript)

None.

## Mismatch families (HIR ← TypeScript)

None.

## Files (lowest parity first)

| file | parity | agree | erased | wider | mismatch | dynamic |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |

## Refused inputs (Smelt could not lower them)

- `ts-pattern/Smelt.toml`: ts-pattern/src/patterns.ts: [     ManifestDiagnostic {         file: "ts-pattern/src/patterns.ts",         category: UnsupportedLowering,         code: "smelt::unsupported-ts",         message: "too many generic type arguments",         line: Some(             150,         ),         column: Some(             26,

## TypeScript checker errors

None: every input type-checks under TypeScript 7.
