# Resumable Generator and Delegation Plan

**Goal:** Lower TypeScript generators as genuinely resumable values so `yield`,
`.next()`, completion returns, throws after suspension, async generators, and
`yield*` preserve JavaScript execution order without `SmeltUnknown` erasure.

**Architecture:** Add distinct HIR/MIR generator and iterator-result types plus
generator metadata on functions. Emit a named `SmeltGenerator<Y, R>` wrapper
whose hidden producer is a `genawaiter::rc::Gen`; generator bodies render inside
the producer's async closure, and a generator-yield rvalue renders as
`co.yield_(value).await`. The wrapper type-erases only the producer closure, not
`Y` or `R`. Async generators use a separate wrapper whose `next()` returns a
typed future. Delegation is a dedicated rvalue that resumes the selected iterator
protocol until completion, forwarding each yielded value through the outer
producer and evaluating to the delegate's return value.

## Constraints

- No neverthrow or function-name special cases.
- Keep `Y`, `R`, and `N` concrete/generic; no `SmeltUnknown` compiler shortcut.
- A generator call must not execute its body.
- Each `.next()` advances at most one suspension or completion.
- A throw after a yield occurs only if the generator is resumed again.
- Sync generators reject async-only delegates; async generators may delegate to
  sync or async iterables.
- Built-in and symbol-based iterables use the same typed adapters.

## Task 1: Resumable synchronous generator core

- [ ] Add failing frontend/codegen runtime tests for call laziness, sequential
  `.next()`, completion return values, and yield-then-throw timing.
- [ ] Add distinct `Type::Generator` and `Type::GeneratorResult` shapes and
  generator metadata through HIR and MIR.
- [ ] Add generator-yield HIR/MIR operation and generator-aware return typing.
- [ ] Add `genawaiter` only to generated crates that contain generators.
- [ ] Emit the typed `SmeltGenerator` runtime wrapper and producer body.
- [ ] Lower typed `.next()`, `.done`, and `.value` reads.
- [ ] Pass focused frontend, MIR, generated Rust, and runtime tests.

## Task 2: Synchronous `yield*`

- [ ] Add failing tests for expression return values, exact-once operand
  evaluation, nested delegation, yield covariance, heterogeneous union returns,
  built-in iterables, and symbol iterator methods.
- [ ] Add typed delegation HIR/MIR and emit the resume/forward/complete loop.
- [ ] Reconcile compatible yield/return arms through assignability and unions.
- [ ] Pass focused runtime tests including neverthrow's Err-yield-then-throw
  shape.

## Task 3: Async generators and async delegation

- [ ] Add failing tests for unannotated/annotated async generators, async
  `.next()`, async iterator methods, sync fallback delegates, and effect order.
- [ ] Emit a typed async-generator wrapper whose `next()` returns a future.
- [ ] Lower async delegation without allowing async carriers in sync generators.
- [ ] Pass focused runtime tests.

## Task 4: Reprobe and ship

- [ ] Refresh neverthrow probe and generated Rust test report.
- [ ] Run `smelt-unknown-report`; avoidable erasure must not increase.
- [ ] Run repository tight loop and full pre-commit suite.
- [ ] Request independent review, fix all Critical/Important findings, commit,
  and push.
