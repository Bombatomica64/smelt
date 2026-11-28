//! Function identity on records built from a function value (design decision
//! C1, `docs/design/record-identity.md` §5).
//!
//! # The rule
//!
//! TypeScript accepts a function where an object type of `Function.prototype`
//! members is expected:
//!
//! ```ts
//! interface Named { name: string; length: number }
//! function hello(a: number, b: number): number { return a + b; }
//! const n: Named = hello;
//! n === hello;                 // true: one object
//! ```
//!
//! The emitter builds the record from the function's own `name` and `length`
//! (`emitter::function_name::function_record_expr`), so the record is a second
//! Rust value. To keep `===` and `Object.is` agreeing with JavaScript, such a
//! record carries a hidden token ([`FUNCTION_IDENTITY_FIELD`], runtime type
//! `SmeltFunctionToken`) holding the identity key of the function it was built
//! from:
//!
//! * the function side keeps its exact representation (a plain `Rc<dyn Fn>`,
//!   a borrowed `&dyn Fn`, a function item's shared value): its key is the one
//!   two function values already compare by (`Rc::as_ptr`);
//! * the record's identity is the token's key when a function built it, and
//!   the record's own identity otherwise. `record === fn`, `fn === record` and
//!   `record === record` compare those identities, so two records built from
//!   one function are equal and a record built from a function equals it;
//! * the token pins the function's allocation with a `Weak` (no strong handle,
//!   so no `Rc` cycle and no captured value kept alive), so a record that
//!   outlives every other handle on its function never matches a later
//!   function allocated at the reused address.
//!
//! # Pay for use
//!
//! Only the records some MIR statement converts a function value into carry
//! the token ([`token_records`]): a program that never wraps a function emits
//! exactly what it emitted before. The token is not an own property (keys,
//! JSON, inspection and deep equality never see it), and an erased record does
//! not carry it.
//!
//! # Limits
//!
//! * A property written through the record is not read back through the bare
//!   function, and the other way round (C1's documented limit).
//! * Only non-generic interface records carry the token: a class type or a
//!   generic record a function is converted to keeps the record's own
//!   identity, as before.
//! * A record built from a borrowed function parameter (`&dyn Fn`) keys it by
//!   address with no pin: no `Rc` handle is in reach there.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are called from the struct emitters and the function emitter"
)]

use std::collections::HashSet;

use smelt_hir::{Symbol, Type, TypeId};
use smelt_mir::{
    BasicBlock, Callee, LocalDecl, Mir, Operand, Place, Rvalue, Statement, Terminator,
};

/// The Rust name of the hidden function-identity field.
///
/// A `_smelt` name, reserved for generated members (`names::escape`), and
/// never an own property: no enumeration, JSON or inspection view lists it.
pub(crate) const FUNCTION_IDENTITY_FIELD: &str = "_smelt_function";

/// The runtime type of the field.
pub(crate) const FUNCTION_TOKEN_TYPE: &str = "SmeltFunctionToken";

/// The struct field line declaring the token. A record that derives serde
/// skips it: the token is not an own property.
pub(crate) fn field_declaration(serde: bool) -> String {
    let skip = if serde { "#[serde(skip)] " } else { "" };
    format!("{skip}{FUNCTION_IDENTITY_FIELD}: {FUNCTION_TOKEN_TYPE},")
}

/// The records whose values carry the function-identity token: every
/// non-generic interface record of `Function.prototype`-compatible shape that
/// some statement converts a function value into (an assignment, a call
/// argument, a returned value).
pub(crate) fn token_records(mir: &Mir) -> HashSet<Symbol> {
    let mut found = HashSet::new();
    let mut visit = |locals: &[LocalDecl], blocks: &[BasicBlock], return_ty: TypeId| {
        for block in blocks {
            for statement in &block.statements {
                let (dest, value) = if let Statement::Assign { dest, value } = statement {
                    (*dest, value)
                } else if let Statement::AssignPlace {
                    place: Place::Local(dest),
                    value,
                } = statement
                {
                    (*dest, value)
                } else {
                    continue;
                };
                let Some(dest_ty) = local_ty(locals, dest) else {
                    continue;
                };
                if let Rvalue::Use(operand) = value {
                    note(mir, &mut found, locals, operand, dest_ty);
                } else if let Rvalue::List(items) = value
                    && let Some(Type::List(item_ty)) = mir.types.get(dest_ty)
                {
                    // A list literal's elements convert to its element type.
                    for item in items {
                        note(mir, &mut found, locals, item, *item_ty);
                    }
                }
            }
            match &block.terminator {
                Some(Terminator::Call {
                    callee: Callee::Static(function),
                    args,
                    ..
                }) => {
                    let Some(callee) = mir.function(*function) else {
                        continue;
                    };
                    for (arg, param) in args.iter().zip(&callee.params) {
                        if let Some(param_ty) = local_ty(&callee.locals, *param) {
                            note(mir, &mut found, locals, arg, param_ty);
                        }
                    }
                }
                Some(Terminator::Return(operand)) => {
                    note(mir, &mut found, locals, operand, return_ty);
                }
                _ => {}
            }
        }
    };
    for function in &mir.functions {
        visit(&function.locals, &function.blocks, function.return_ty);
    }
    for closure in &mir.closures {
        visit(&closure.locals, &closure.blocks, closure.return_ty);
    }
    found
}

/// Record `dest_ty`'s record when `operand` holds a function value and the
/// record is one a function converts into.
fn note(
    mir: &Mir,
    found: &mut HashSet<Symbol>,
    locals: &[LocalDecl],
    operand: &Operand,
    dest_ty: TypeId,
) {
    let Some(Type::Class { name, args }) = mir.types.get(dest_ty) else {
        return;
    };
    if !args.is_empty() || found.contains(name) {
        return;
    }
    let holds_function = operand_ty(mir, locals, operand)
        .is_some_and(|ty| matches!(mir.types.get(ty), Some(Type::Function(_))));
    if holds_function && is_function_shaped_record(mir, *name) {
        found.insert(*name);
    }
}

/// Whether `name` is a non-generic interface record whose members a function
/// value supplies: `name: string`, `length: number`, or optional members
/// (the shapes `function_record_expr` builds).
fn is_function_shaped_record(mir: &Mir, name: Symbol) -> bool {
    let Some(interface) = mir
        .interfaces
        .iter()
        .find(|interface| interface.name == name)
    else {
        return false;
    };
    if !interface.type_params.is_empty()
        || !interface.methods.is_empty()
        || crate::classify::is_callable_shape(mir, interface)
    {
        return false;
    }
    let fields = crate::classes::effective_interface_fields(mir, interface);
    !fields.is_empty()
        && fields.iter().all(|field| {
            let source = mir
                .names
                .get(field.name)
                .or_else(|| mir.symbols.get(field.name));
            matches!(
                (source, mir.types.get(field.ty)),
                (_, Some(Type::Optional(..)))
                    | (Some("name"), Some(Type::String))
                    | (Some("length"), Some(Type::Float | Type::Int))
            )
        })
}

/// The declared type of local `local`.
fn local_ty(locals: &[LocalDecl], local: smelt_mir::LocalId) -> Option<TypeId> {
    locals
        .get(usize::try_from(local.0).ok()?)
        .map(|decl| decl.ty)
}

/// The type `operand` reads: a local's, or a list element's.
fn operand_ty(mir: &Mir, locals: &[LocalDecl], operand: &Operand) -> Option<TypeId> {
    let place = operand.place()?;
    if let Place::Local(local) = place {
        return local_ty(locals, *local);
    }
    if let Place::Index { base, .. } = place
        && let Some(Type::List(item)) = mir.types.get(local_ty(locals, *base)?)
    {
        return Some(*item);
    }
    None
}
