//! Codegen regression tests for the es-toolkit "tail round 7" fixes.
//!
//! * A returned closure whose contextual function type spells a parameter as
//!   optional (`(target?: V) => boolean`) while the closure body declares the
//!   same parameter flat (`function (target?: unknown)` lowering to bare
//!   `SmeltUnknown`) must emit the closure parameter at the contextual
//!   `Option<…>` type and rebind it to the flat local type in the body. Without
//!   this the `Rc<{closure}>` fails to coerce to the target
//!   `Rc<dyn Fn(Option<SmeltUnknown>) -> _>` (was E0631 in `matchesProperty`).

use super::*;

#[test]
fn returned_closure_param_follows_dropped_optional_target_type() {
    // The declared return type's parameter is optional over a type parameter
    // (`Option<SmeltUnknown>` once erased), while the returned closure declares
    // its own parameter as bare `unknown` (`SmeltUnknown`). The emitted closure
    // signature must adopt the `Option<SmeltUnknown>` contextual type so the
    // `Rc<{closure}>` coerces to the target `dyn Fn`.
    let source = source_for(
        r"
function id2<T>(x: T): T {
  return x;
}
export function make<V>(source: V): (target?: V) => boolean {
  source = id2(source);
  return function (target?: unknown) {
    return target === source;
  };
}
",
    );
    assert!(
        source.contains("closure_arg_0: Option<SmeltUnknown>"),
        "expected closure param to adopt the optional contextual type, got:\n{source}"
    );
    // The body must rebind the parameter back to its flat local type via a
    // lossless `unwrap_or` so the closure body keeps seeing `SmeltUnknown`.
    assert!(
        source.contains("let closure_arg_0: SmeltUnknown ="),
        "expected a rebind of the closure param to its flat local type, got:\n{source}"
    );
}
