//! Throwing-function reachability propagation.
//!
//! Invariant: every MIR function or closure that can reach an uncaught throw,
//! an unwound async operation, or a throwing call is marked `can_throw`.

use smelt_hir::Type;

use crate::{Callee, LocalDecl, Mir, Rvalue, Statement, Terminator};
use smelt_hir::Symbol;

use super::super::{local_index, usize_from_u32};

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

/// One override family: each member's function index and whether it is an
/// abstract stub.
type Family = Vec<(usize, bool)>;

/// Override families by (hierarchy root, method name) symbol id.
type FamilyTable = std::collections::BTreeMap<(u32, u32), Family>;

/// The instance methods of each class hierarchy that share a name: one
/// override family, as function indices.
///
/// A method body is emitted once per class that has it (an inherited body is
/// re-emitted into each subclass's impl), and a call `self.f()` in it resolves
/// to that class's own `f`. Every member of a family must therefore agree on
/// whether it returns a `Result`: a body compiled against a throwing base `f`
/// applies `?` to whatever `f` the subclass has. Families never span two
/// hierarchies (a hierarchy is the classes connected through `base`).
///
/// Each member is paired with whether it is an abstract method's stub (a
/// method an abstract class declares abstract): no dispatch reaches a stub,
/// because its class is never instantiated and every concrete class overrides
/// it, so a raising stub body (Python's `raise NotImplementedError`) does not
/// make its family throw. A re-emitted body calling the stub on `self` adapts
/// to the infallible override (`emitter::call`, `inherited_self_call`).
fn override_families(mir: &Mir) -> Vec<Family> {
    let root_of = |class: &crate::MirClass| {
        // A multiple-inheritance hierarchy has no single root: one override
        // family per method name spans all of them (a conservative merge of
        // separate hierarchies, which can only add `can_throw`).
        if mir.in_multiple_inheritance_hierarchy(class.name) {
            return Symbol(u32::MAX);
        }
        let mut current = class;
        let mut seen = std::collections::HashSet::from([current.name]);
        while let Some(base) = current
            .base
            .and_then(|base| mir.classes.iter().find(|candidate| candidate.name == base))
        {
            if !seen.insert(base.name) {
                break;
            }
            current = base;
        }
        current.name
    };
    // Keyed by (hierarchy root, method name) symbol ids, in a stable order.
    let mut families: FamilyTable = std::collections::BTreeMap::new();
    for class in &mir.classes {
        let root = root_of(class);
        for method in &class.methods {
            let Some(index) = usize::try_from(method.0).ok() else {
                continue;
            };
            let Some(function) = mir.functions.get(index) else {
                continue;
            };
            let stub = class.is_abstract
                && class
                    .abstract_methods
                    .iter()
                    .any(|signature| signature.name == function.name);
            let family = families.entry((root.0, function.name.0)).or_default();
            if !family.iter().any(|(member, _)| *member == index) {
                family.push((index, stub));
            }
        }
    }
    families
        .into_values()
        .filter(|family| family.len() > 1)
        .collect()
}

/// Marks functions that can reach an uncaught throw directly or through calls.
pub(in crate::lower) fn propagate_throwing_functions(mir: &mut Mir) {
    let families = override_families(mir);
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
            // Calling an async function never throws at the call: its error
            // rejects the promise the call returns (the Rust call builds a
            // future, the `Result` lives inside it). So a caller, or a
            // callback wrapping the call (`ids.map(fetchUser)`), does not
            // inherit an async callee's error channel.
            throwing: mir
                .functions
                .iter()
                .map(|function| function.can_throw && !function.is_generator && !function.is_async)
                .collect::<Vec<_>>(),
        };
        let mut changed = false;

        for function in &mut mir.functions {
            if function.can_throw {
                continue;
            }
            let bound = projected_bind_locals(&function.blocks, &function.locals, &facts.types);
            let can_throw = function.blocks.iter().any(|block| {
                block.statements.iter().any(|statement| {
                    statement_can_throw(statement, &function.locals, &bound, &facts)
                }) || block
                    .terminator
                    .as_ref()
                    .is_some_and(|terminator| terminator_can_throw(terminator, &facts.throwing))
            });
            if can_throw {
                function.can_throw = true;
                changed = true;
            }
        }

        // One override family, one answer (see `override_families`).
        for family in &families {
            let throws = family.iter().any(|(index, stub)| {
                !stub
                    && mir
                        .functions
                        .get(*index)
                        .is_some_and(|function| function.can_throw && !function.is_generator)
            });
            if !throws {
                continue;
            }
            for (index, _) in family {
                if let Some(function) = mir.functions.get_mut(*index)
                    && !function.can_throw
                    && !function.is_generator
                {
                    function.can_throw = true;
                    changed = true;
                }
            }
        }

        for closure in &mut mir.closures {
            if closure.can_throw {
                continue;
            }
            let bound = projected_bind_locals(&closure.blocks, &closure.locals, &facts.types);
            let can_throw = closure.blocks.iter().any(|block| {
                block.statements.iter().any(|statement| {
                    statement_can_throw(statement, &closure.locals, &bound, &facts)
                }) || block
                    .terminator
                    .as_ref()
                    .is_some_and(|terminator| terminator_can_throw(terminator, &facts.throwing))
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

/// Locals holding a receiver bind of a callee read through a projection
/// (`%t = bind_this obj.field, obj`).
///
/// A call through such a local is the call `obj.field(..)`, and a call through
/// a projected callee is conservatively throwing (see the `ClosureCall` rule in
/// [`statement_can_throw`]): what the field holds is decided at run time. The
/// bind only installs `this`, so it must not change that answer.
///
/// Only a TYPED callee (a concrete `Type::Function`, the shape the frontend
/// binds at a call through a function-valued class field) is collected. An
/// erased callee's bind — an `unknown` read or the variadic erased-function
/// spelling — keeps the answer its local's type gives.
fn projected_bind_locals(
    blocks: &[crate::BasicBlock],
    locals: &[LocalDecl],
    types: &smelt_hir::TypeInterner,
) -> std::collections::HashSet<crate::LocalId> {
    let typed_callable = |local: crate::LocalId| {
        let Some(Type::Function(function)) = local_index(local)
            .and_then(|index| locals.get(index))
            .and_then(|decl| types.get(decl.ty))
        else {
            return false;
        };
        let erased_rest = function.rest == Some(0)
            && matches!(function.params.as_slice(), [param]
                if matches!(types.get(*param), Some(Type::List(item))
                    if matches!(types.get(*item), Some(Type::Unknown | Type::TypeParam { .. } | Type::Never))));
        !erased_rest
    };
    blocks
        .iter()
        .flat_map(|block| &block.statements)
        .filter_map(|statement| {
            if let Statement::Assign {
                dest,
                value: Rvalue::BindThis { callee, .. } | Rvalue::BindOwnThis { callee },
            } = statement
                && callee.local().is_none()
                && typed_callable(*dest)
            {
                Some(*dest)
            } else {
                None
            }
        })
        .collect()
}

/// Returns whether a MIR statement can throw through an expression-level call.
fn statement_can_throw(
    statement: &Statement,
    locals: &[LocalDecl],
    projected_binds: &std::collections::HashSet<crate::LocalId>,
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
    // One step of `yield*` forwards the resume to its delegate, whose throw
    // (a `throw(e)` it rethrows, a throwing `next()`) ends this generator.
    if matches!(
        value,
        Rvalue::GeneratorYield {
            delegate: Some(_),
            ..
        }
    ) {
        return true;
    }
    // A call through a type parameter's bound reaches whichever implementation
    // the receiver's concrete class carries, so it throws when any of them can.
    if let Rvalue::BoundMethod { bound, method, .. } = value
        && bound_method_can_throw(*bound, *method, facts)
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
    // Every SQLite operation can fail at run time — a missing table, a
    // constraint, an unbindable parameter — and Node throws for each.
    if matches!(value, Rvalue::SqliteOpen { .. } | Rvalue::SqliteOp { .. }) {
        return true;
    }
    let Rvalue::ClosureCall { callee, .. } = value else {
        return false;
    };
    let Some(local) = callee.local() else {
        return true;
    };
    if projected_binds.contains(&local) {
        return true;
    }
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
    let Some(local) = receiver.local() else {
        return false;
    };
    let Some(decl) = local_index(local).and_then(|index| locals.get(index)) else {
        return false;
    };
    // The receiver of an optional chain is usually `Option<Class>`; either
    // spelling names the same class.
    let receiver_ty = match types.get(decl.ty) {
        Some(Type::Optional(inner, _)) => *inner,
        _ => decl.ty,
    };
    let Some(Type::Class { name, .. }) = types.get(receiver_ty) else {
        return false;
    };
    let Some(class) = facts.classes.iter().find(|class| class.name == *name) else {
        return false;
    };
    class.methods.iter().any(|func| {
        usize_from_u32(func.0, "MIR function index does not fit in usize").is_ok_and(|index| {
            facts.function_names.get(index).copied() == Some(method)
                && facts.throwing.get(index).copied().unwrap_or(false)
        })
    })
}

/// Returns whether a method called through the class or interface `bound`
/// (`Rvalue::BoundMethod`) can throw.
///
/// The receiver may be any class that is, extends, or implements the bound, so
/// every such class is asked — through its own base chain, since an inherited
/// method is the receiver's method too. Interface-typed object literals carry
/// their methods as closure fields, which the emitter calls without a `Result`
/// (the same as a direct call on an interface value), so they add nothing.
fn bound_method_can_throw(bound: Symbol, method: Symbol, facts: &CrateThrowFacts) -> bool {
    let class_named = |name: Symbol| facts.classes.iter().find(|class| class.name == name);
    // Every class `class` derives from, itself first, through any base.
    let ancestry = |class: &crate::MirClass| {
        let mut found: Vec<Symbol> = Vec::new();
        let mut pending = vec![class.name];
        while let Some(name) = pending.pop() {
            if found.contains(&name) || found.len() > facts.classes.len() {
                continue;
            }
            found.push(name);
            if let Some(candidate) = class_named(name) {
                pending.extend(candidate.program_bases());
            }
        }
        found
    };
    let reaches_bound = |class: &crate::MirClass| {
        ancestry(class).into_iter().filter_map(class_named).any(|candidate| {
            candidate.name == bound || candidate.implements.contains(&bound)
        })
    };
    let declares_throwing = |class: &crate::MirClass| {
        class.methods.iter().any(|func| {
            usize_from_u32(func.0, "MIR function index does not fit in usize").is_ok_and(|index| {
                facts.function_names.get(index).copied() == Some(method)
                    && facts.throwing.get(index).copied().unwrap_or(false)
            })
        })
    };
    facts
        .classes
        .iter()
        .filter(|class| reaches_bound(class))
        .any(|class| ancestry(class).into_iter().filter_map(class_named).any(declares_throwing))
}

/// Returns whether a terminator can leave through an uncaught exception path.
fn terminator_can_throw(terminator: &Terminator, throwing: &[bool]) -> bool {
    match terminator {
        Terminator::Throw(_) => true,
        Terminator::Call {
            callee: Callee::Static(func) | Callee::BaseInit(func),
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
        // A fallible operation with no handler in scope leaves the function
        // through its error channel, so the enclosing function throws. Being a
        // callee at all is what says the op can throw.
        Terminator::Call {
            callee: Callee::Op(_, None),
            unwind: None,
            ..
        } => true,
        Terminator::Goto(_)
        | Terminator::Call { .. }
        | Terminator::Await { .. }
        | Terminator::Switch { .. }
        | Terminator::Match { .. }
        | Terminator::Return(_) => false,
    }
}
