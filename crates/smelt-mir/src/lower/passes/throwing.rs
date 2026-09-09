//! Throwing-function reachability propagation.
//!
//! Invariant: every MIR function or closure that can reach an uncaught throw,
//! an unwound async operation, or a throwing call is marked `can_throw`.

use smelt_hir::Type;

use crate::{Callee, LocalDecl, Mir, Rvalue, Statement, Terminator};
use smelt_hir::Symbol;

use super::super::{local_index, usize_from_u32};
use super::operand_local;

/// What the propagation loop knows about the crate while it mutates one
/// function at a time.
///
/// The loop takes `&mut` on the functions it marks, so everything the rules
/// read about OTHER items — the type interner, the class table, and each
/// function's name and current throwing flag — is snapshotted per iteration.
/// One struct rather than four parallel arguments: the two rules that need
/// them would otherwise take six and seven positional parameters.
struct CrateThrowFacts {
    /// Interned types, for reading local declarations.
    types: smelt_hir::TypeInterner,
    /// Class table, for resolving a method call on a class receiver.
    classes: Vec<crate::MirClass>,
    /// Each function's name, indexed by function id.
    function_names: Vec<Symbol>,
    /// Whether each function is currently known to throw, indexed by id.
    throwing: Vec<bool>,
}

/// Marks functions that can reach an uncaught throw directly or through calls.
pub(in crate::lower) fn propagate_throwing_functions(mir: &mut Mir) {
    loop {
        // A method reached through an optional chain is an `Rvalue`, not a call
        // terminator, so the statement rule below resolves it against the class
        // table and the function names itself.
        let facts = CrateThrowFacts {
            types: mir.types.clone(),
            classes: mir.classes.clone(),
            function_names: mir
                .functions
                .iter()
                .map(|function| function.name)
                .collect::<Vec<_>>(),
            throwing: mir
                .functions
                .iter()
                .map(|function| function.can_throw && !function.is_generator)
                .collect::<Vec<_>>(),
        };
        let mut changed = false;

        for function in &mut mir.functions {
            if function.can_throw {
                continue;
            }
            let can_throw = function.blocks.iter().any(|block| {
                block
                    .statements
                    .iter()
                    .any(|statement| statement_can_throw(statement, &function.locals, &facts))
                    || block
                        .terminator
                        .as_ref()
                        .is_some_and(|terminator| {
                            terminator_can_throw(terminator, &facts.throwing)
                        })
            });
            if can_throw {
                function.can_throw = true;
                changed = true;
            }
        }

        for closure in &mut mir.closures {
            if closure.can_throw {
                continue;
            }
            let can_throw = closure.blocks.iter().any(|block| {
                block
                    .statements
                    .iter()
                    .any(|statement| statement_can_throw(statement, &closure.locals, &facts))
                    || block
                        .terminator
                        .as_ref()
                        .is_some_and(|terminator| {
                            terminator_can_throw(terminator, &facts.throwing)
                        })
            });
            if can_throw {
                closure.can_throw = true;
                changed = true;
            }
        }

        if !changed {
            break;
        }
    }
}

/// Returns whether a MIR statement can throw through an expression-level call.
fn statement_can_throw(
    statement: &Statement,
    locals: &[LocalDecl],
    facts: &CrateThrowFacts,
) -> bool {
    let types = &facts.types;
    let Statement::Assign { value, .. } = statement else {
        return false;
    };
    // A method called through an optional chain (`registry?.insert(k, v)`) is an
    // `Rvalue`, so the terminator rule below never sees it and the caller went
    // unmarked. The generated Rust then put a `?` in a function returning no
    // `Result`: `registry?.insert(..)` is the same call as `registry.insert(..)`
    // for every purpose except the receiver's presence, so it propagates the
    // same way.
    if let Rvalue::OptionalMethod {
        receiver, method, ..
    } = value
        && optional_method_can_throw(receiver, *method, locals, facts)
    {
        return true;
    }
    // `EventEmitter.emit` calls every registered listener, and a listener that
    // throws leaves through the emitting function exactly as a direct call
    // would. Which listeners are registered is a run-time fact, so the
    // conservative answer is the only sound one.
    if matches!(
        value,
        Rvalue::EventEmitterOp {
            op: smelt_hir::EventEmitterOp::Emit,
            ..
        }
    ) {
        return true;
    }
    // `Server.listen` runs the listening callback in the same turn, and
    // `close`/`address` reach the same runtime that a bind error surfaces
    // through — a port already in use is a thrown `Error` in Node, not a
    // silent no-op. Every server operation is therefore throwing, on the same
    // reasoning as `emit`: what the operation reaches is decided at run time.
    if matches!(value, Rvalue::HttpServerOp { .. }) {
        return true;
    }
    let Rvalue::ClosureCall { callee, .. } = value else {
        return false;
    };
    let Some(local) = operand_local(callee) else {
        return true;
    };
    local_index(local)
        .and_then(|index| locals.get(index))
        .and_then(|decl| types.get(decl.ty))
        .is_some_and(|ty| matches!(ty, Type::Function(function) if function.may_throw))
}

/// Returns whether the method an optional chain calls is itself throwing.
///
/// Resolves the receiver's static class and looks `method` up in that class's
/// own method list — the same question, answered the same way, as when the
/// emitter decides whether to render the call with a `?`.
fn optional_method_can_throw(
    receiver: &crate::Operand,
    method: Symbol,
    locals: &[LocalDecl],
    facts: &CrateThrowFacts,
) -> bool {
    let types = &facts.types;
    let Some(local) = operand_local(receiver) else {
        return false;
    };
    let Some(decl) = local_index(local).and_then(|index| locals.get(index)) else {
        return false;
    };
    // The receiver of an optional chain is usually `Option<Class>`; either
    // spelling names the same class.
    let receiver_ty = match types.get(decl.ty) {
        Some(Type::Optional(inner)) => *inner,
        _ => decl.ty,
    };
    let Some(Type::Class { name, .. }) = types.get(receiver_ty) else {
        return false;
    };
    let Some(class) = facts.classes.iter().find(|class| class.name == *name) else {
        return false;
    };
    class.methods.iter().any(|func| {
        usize_from_u32(func.0, "MIR function index does not fit in usize")
            .ok()
            .is_some_and(|index| {
                facts.function_names.get(index).copied() == Some(method)
                    && facts.throwing.get(index).copied().unwrap_or(false)
            })
    })
}

/// Returns whether a terminator can leave through an uncaught exception path.
fn terminator_can_throw(terminator: &Terminator, throwing: &[bool]) -> bool {
    match terminator {
        Terminator::Throw(_) => true,
        Terminator::Call {
            callee: Callee::Static(func),
            unwind: None,
            ..
        } => usize_from_u32(func.0, "MIR function index does not fit in usize")
            .is_ok_and(|index| throwing.get(index).copied().unwrap_or(false)),
        Terminator::Call {
            callee: Callee::Indirect(_),
            unwind: None,
            ..
        } => true,
        Terminator::Await { unwind: None, .. } => true,
        // A fallible builtin with no handler in scope leaves the function
        // through its error channel, so the enclosing function throws. Asked of
        // the builtin itself (`BuiltinFn::is_fallible`) rather than named here:
        // this arm used to name `JsonParse` alone, so the URI decoders — added
        // later, and fallible in exactly the same way — never marked their
        // caller, and `decodeURI` outside a `try` emitted a `?` in a function
        // returning no `Result`.
        Terminator::Call {
            callee: Callee::Builtin(builtin),
            unwind: None,
            ..
        } => builtin.is_fallible(),
        Terminator::Goto(_)
        | Terminator::Call { .. }
        | Terminator::Await { .. }
        | Terminator::Switch { .. }
        | Terminator::Match { .. }
        | Terminator::Return(_)
        | Terminator::Unreachable => false,
    }
}
