# Verification contracts and audit evidence

The full checklist behind the "Verification contracts and audit evidence" rules in
`AGENTS.md`. Read it before correcting an expectation, adding or correcting a general
lowering rule, running or answering an audit, or doing optimization work. The rules in
`AGENTS.md` are binding; this file states them in full, with examples.

## Verification contracts and generated output

Treat expected source behavior and type preservation as requirements independent of the
implementation. Never make a fix pass by weakening assertions, deleting coverage,
broadening erasure classifications, or relaxing a baseline. If an expectation is wrong,
document the old and new requirement, the source-language evidence, and why the correction
preserves the intended guarantee. Follow the specific baseline-update rules in `AGENTS.md`
("SmeltUnknown enforcement", "Typedness ratchet").

Fix lowering, emission, or runtime helpers at their source. Edits to generated Rust may be
used to investigate a failure, but are not a durable fix. When a change affects committed
generated artifacts, regenerate the affected artifacts and verify that they match the
committed output. Preserve unchanged file mtimes; use temporary output for comparisons
where appropriate. Validate the final tree being committed, and rerun affected checks if it
changes afterward.

For a new or corrected general lowering rule, state both its behavioral and
type-preservation requirements. Use valid source examples and compare source-runtime
behavior with generated Rust where practical; also check the emitted concrete or generic
types. Successful compilation alone does not establish either requirement.

For high-risk lowering rules or suspected coverage gaps, temporarily introduce a plausible
defect and verify that the relevant check fails. Examples include dropping a namespace
qualifier, erasing a generic parameter, or breaking a union-narrowing branch. Restore the
implementation before committing. Record which defect the check detects; a passing suite
alone does not demonstrate that it would catch that defect.

## Audit evidence

Review behavioral equivalence, type preservation, and weakened checks or stale generated
output as separate questions. Include shared lowering and runtime helpers, not only the
changed call sites. When running repeated independent audits, give each auditor a fresh
context with the requirements and relevant artifacts, and require concrete evidence for
findings.

Prioritize findings reproducible through accepted TypeScript or Python source or a
supported public API. For an internal-state finding, establish how a caller can reach that
state; record unreachable states separately rather than treating them as demonstrated
source-level failures. This does not replace frontend validation or rejection tests.

When changing allocation, copying, recursion, size arithmetic, or runtime adapters, check
resource behavior with representative and boundary inputs as well as returned values. For
optimization work, define the benchmark and acceptance criteria before changing the
implementation; accept improvements only while behavioral and type-preservation checks
still pass.

Report exactly what the evidence establishes and what remains unchecked. Tests cover their
exercised cases; proofs cover their stated properties under their assumptions. Neither a
clean audit round nor successful compilation establishes complete correctness. Any future
formal verification must review the specification and proof statements themselves,
including assumptions about compilation and runtime behavior.
