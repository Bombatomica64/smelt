//! The definite-assignment pass on lowered TypeScript.
//!
//! Each test lowers a valid program and reads the optimized MIR dump: which
//! bindings became `MaybeUnbound`, which reads are checked (they raise the
//! language's error) and which are proven, and where a checked read unwinds.
//! The behaviour of the same shapes against Node and `CPython` is pinned by the
//! parity fixtures in `smelt-transpiler`.

use smelt_frontend_ts::{HirCtx, to_hir};
use smelt_hir::FileId;

use crate::{format_compact, lower_hir, opt, validate};

/// Fails the test with `message`.
fn fail(message: String) -> ! {
    std::panic::resume_unwind(Box::new(message))
}

/// Lowers and optimizes `source`, asserts the result validates, and dumps it.
fn optimized(source: &str) -> String {
    let mut ctx = HirCtx::new();
    let lowered = to_hir(source, FileId(0), &mut ctx);
    assert!(lowered.is_ok(), "HIR: {lowered:?}");
    let mut mir = lower_hir(&ctx.krate).unwrap_or_else(|errors| fail(format!("MIR: {errors:?}")));
    opt::optimize(&mut mir);
    assert_eq!(validate(&mir), Vec::new());
    format_compact(&mir)
}

/// The dump of the body headed `header` (a `fn name` or `closure ClosureId(n)`
/// line), up to the next body.
fn body<'a>(dump: &'a str, header: &str) -> &'a str {
    let start = if dump.starts_with(header) {
        0
    } else {
        dump.find(&format!("\n{header}"))
            .map_or_else(|| fail(format!("no `{header}` in:\n{dump}")), |at| at.saturating_add(1))
    };
    let rest = &dump[start..];
    let after_header = rest.get(1..).unwrap_or_default();
    let end = after_header
        .find("\nfn ")
        .or_else(|| after_header.find("\nclosure "))
        .map_or(rest.len(), |end| end.saturating_add(1));
    &rest[..end]
}

/// A closure that reads a `const` declared after it is called raises
/// `ReferenceError` there: the binding and the closure's copy can be
/// unassigned, and the closure's read is checked.
#[test]
fn a_closure_read_in_the_temporal_dead_zone_is_checked() {
    let dump = optimized(
        "function f(): number {\n  const read = (): number => late + 1\n  const first = read()\n  const late = 2\n  return first + read()\n}\nconsole.log(f())\n",
    );
    let function = body(&dump, "fn f ");
    assert!(function.contains("(maybe unbound)") && function.contains("user late: "), "{dump}");
    let closure = body(&dump, "closure ClosureId(0)");
    assert!(closure.contains("(maybe unbound)"), "{dump}");
    assert!(closure.contains("call @binding_check_tdz("), "{dump}");
    // The function never reads `late` itself, and `read` keeps plain storage.
    assert!(!function.contains("binding_check"), "{dump}");
    assert!(!function.contains("user read: fn() -> Float (maybe unbound)"), "{dump}");
}

/// A `switch` case that skips another case's declaration reads it unassigned;
/// the declaring case reads it after its assignment, proven.
#[test]
fn a_switch_case_read_of_a_skipped_declaration_is_checked() {
    let dump = optimized(
        "function f(k: number): string {\n  switch (k) {\n    case 0:\n      let v: string | undefined = 'zero'\n      return v ?? ''\n    case 1:\n      return v ?? 'one'\n  }\n  return 'none'\n}\nconsole.log(f(0))\n",
    );
    let function = body(&dump, "fn f ");
    assert!(function.contains("(maybe unbound)"), "{dump}");
    // Only the case that skipped the declaration checks it.
    assert_eq!(function.matches("call @binding_check_tdz(").count(), 1, "{dump}");
}

/// A recursive arrow captures its own binding before the binding is
/// assigned, but nothing can call the closure until it is: its storage can be
/// unassigned, and its reads are proven, so it needs no check and does not
/// throw.
#[test]
fn a_recursive_arrow_reads_itself_proven() {
    let dump = optimized(
        "function f(n: number): number {\n  const fact = (k: number): number => (k <= 1 ? 1 : k * fact(k - 1))\n  return fact(n)\n}\nconsole.log(f(5))\n",
    );
    let closure = body(&dump, "closure ClosureId(0)");
    assert!(closure.contains("fact: fn(Float) -> Float (maybe unbound)"), "{dump}");
    assert!(!dump.contains("binding_check"), "{dump}");
    assert!(!closure.contains("throws"), "{dump}");
}

/// A checked read inside a `try` unwinds to that `try`'s handler, so the
/// source's `catch` sees the error.
#[test]
fn a_checked_read_unwinds_to_the_enclosing_handler() {
    let dump = optimized(
        "function f(k: number): string {\n  switch (k) {\n    case 0:\n      let v: string | undefined = 'zero'\n      return v ?? ''\n    case 1:\n      try {\n        return v ?? 'one'\n      } catch (e) {\n        return 'caught'\n      }\n  }\n  return 'none'\n}\nconsole.log(f(1))\n",
    );
    let function = body(&dump, "fn f ");
    let read = function
        .lines()
        .find(|line| line.contains("call @binding_check_tdz("))
        .unwrap_or_else(|| fail(format!("no checked read:\n{dump}")));
    assert!(read.contains(" unwind bb"), "{read}\n{dump}");
}

/// A binding assigned before every read keeps its plain storage: no binding
/// read, no marker.
#[test]
fn a_binding_assigned_before_every_read_is_untouched() {
    let dump = optimized(
        "function f(c: boolean): number {\n  let x = 1\n  if (c) {\n    x = 2\n  }\n  const g = (): number => x\n  return g()\n}\nconsole.log(f(true))\n",
    );
    assert!(!dump.contains("maybe unbound"), "{dump}");
    assert!(!dump.contains("binding_check"), "{dump}");
}

/// `const wrapper = function () { .. this instanceof wrapper .. }`: the
/// function expression names its own binding, which the frontend gives it as a
/// provisional local while the initializer lowers. The declaration assigns
/// that local the declared value (it used to stay unassigned, and a default
/// stood in for the function), and the copy through the declared local still
/// proves the function cannot run before its binding is assigned, so its read
/// is proven and it needs no check (es-toolkit's `curry`).
#[test]
fn a_function_expression_naming_itself_reads_its_assigned_binding() {
    let dump = optimized(
        "interface Curried { (...args: any[]): any; placeholder: number }\nexport function curry(func: (...args: any[]) => any): Curried {\n  const wrapper = function (this: any, ...partialArgs: any[]) {\n    if (this instanceof wrapper) {\n      return 1\n    }\n    return func.apply(this, partialArgs)\n  }\n  wrapper.placeholder = 3\n  return wrapper\n}\n",
    );
    let function = body(&dump, "fn curry ");
    let provisional = function
        .lines()
        .find(|line| line.contains("user wrapper: Unknown (maybe unbound)"))
        .and_then(|line| line.split_whitespace().next())
        .unwrap_or_else(|| fail(format!("no provisional binding:\n{dump}")));
    assert!(function.contains(&format!("    {provisional} = copy ")), "{dump}");
    assert!(!dump.contains("binding_check"), "{dump}");
}
