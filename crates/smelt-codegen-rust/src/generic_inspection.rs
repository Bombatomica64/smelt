//! Scalar inspections of a value of a lifted type parameter.
//!
//! # The rule
//!
//! A generic free function may ask a value typed as its own bare type
//! parameter (`x: T`, or `x?: T`) one of the questions below and keep `T` a
//! real Rust generic. The question is answered on an erased COPY of the value,
//! obtained through the `IntoSmeltUnknown` bound every lifted parameter
//! declares (`classes::GENERATED_TYPE_PARAM_BOUNDS`):
//!
//! - whether it is absent: `x == null`, `x != null`, `x === undefined`,
//!   `x === null` and their negations;
//! - its run-time kind: `typeof x`, and a `typeof x === "string"` guard;
//! - its truthiness: `!x`, `Boolean(x)`, `if (x)`, `x ? a : b`.
//!
//! A hand port writes the same thing: the function stays generic and the one
//! place that needs the dynamic answer converts a clone.
//!
//! # Requirements
//!
//! - Behaviour: each question answers what JavaScript answers for the value
//!   the caller passed. The copy is only read: it is never written, stored,
//!   returned or handed on, so no alias of the original can observe it and no
//!   write can be lost to it. The original `T` value keeps flowing typed.
//! - Type preservation: the function's `T` stays a Rust generic; only the
//!   boolean or string ANSWER leaves the erased copy.
//!
//! # What is not a scalar inspection
//!
//! - A member read or write off a `T` (`obj[key]`, `obj.name = v`,
//!   [`crate::generic_lifting`]'s `place_projects_into_type_param`): the member
//!   must come out of, or go into, the caller's object itself, which an erased
//!   copy is not.
//! - A question asked of a CONTAINER of `T` (`items: T[]`): erasing a list
//!   copies its buffer, and the existing presence rules already answer
//!   `items == null` and `!items` without reading into it.
//! - A comparison of two `T` values, an ordering, arithmetic, `x ?? y`: the
//!   answer is not a property of one value's erased copy alone, or the value
//!   itself is the result.
//!
//! # A guard does not license a reinterpretation
//!
//! `Array.isArray(item)` on an `item: T` is a scalar inspection, but what the
//! body then does with the narrowed value is judged on its own. Handing it to
//! a callback that reads it as a container of `T` ([`reinterprets_type_param`])
//! treats one `T` as many, which only the erased carrier can do, so such a
//! body keeps `T` erased. Uses at a `T`-free type (the `string` a
//! `typeof x === "string"` guard proves) were never moves and stay rejected
//! by [`crate::generic_lifting`]'s destination rules.
#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible items are shared with the lifting analysis and the emitter"
)]

use std::collections::HashMap;

use smelt_hir::{BinOp, PrimitiveCastOp, Symbol, Type, UnaryOp};
use smelt_mir::{
    BasicBlock, Constant, LocalDecl, LocalId, LocalKind, Mir, Operand, Place, Rvalue, Statement,
    Terminator,
};

use crate::id_index;

/// Whether `operand` reads a whole local typed as the bare type parameter
/// `name`, or as an optional of it.
pub(crate) fn operand_is_bare_type_param(
    mir: &Mir,
    locals: &[LocalDecl],
    operand: &Operand,
    name: Symbol,
) -> bool {
    let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = operand else {
        return false;
    };
    let Some(decl) = id_index(local.0, "local index does not fit usize")
        .ok()
        .and_then(|index| locals.get(index))
    else {
        return false;
    };
    let payload = match mir.types.get(decl.ty) {
        Some(Type::Optional(inner)) => *inner,
        _ => decl.ty,
    };
    matches!(mir.types.get(payload), Some(Type::TypeParam { name: found }) if *found == name)
}

/// Which absent value a comparison tests for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Absent {
    /// `null`.
    Null,
    /// `undefined`.
    Undefined,
}

/// The temporaries of a body that only ever hold an absent value: exactly
/// one definition in the whole body, and that definition is the `null` or
/// `undefined` constant. This is what a lowered `x === undefined` compares
/// against (`%1 = undefined; %2 = copy %0 == move %1`).
///
/// A parameter or a user binding never qualifies (a caller, or a later
/// assignment through a closure, can give it a real value), and neither does
/// a temporary joined from two branches.
pub(crate) fn absent_only_locals(
    locals: &[LocalDecl],
    blocks: &[BasicBlock],
) -> HashMap<LocalId, Absent> {
    let mut definitions: HashMap<LocalId, usize> = HashMap::new();
    let mut absent: HashMap<LocalId, Absent> = HashMap::new();
    for block in blocks {
        for statement in &block.statements {
            let Some(dest) = statement.defs() else {
                continue;
            };
            let count = definitions.entry(dest).or_default();
            *count = count.saturating_add(1);
            if let Statement::Assign {
                value: Rvalue::Use(Operand::Const(constant)),
                ..
            } = statement
            {
                if matches!(constant, Constant::None) {
                    absent.insert(dest, Absent::Null);
                } else if matches!(constant, Constant::Undefined) {
                    absent.insert(dest, Absent::Undefined);
                }
            }
        }
        if let Some(dest) = block.terminator.as_ref().and_then(Terminator::defs) {
            let count = definitions.entry(dest).or_default();
            *count = count.saturating_add(1);
        }
    }
    absent.retain(|local, _| {
        definitions.get(local) == Some(&1)
            && id_index(local.0, "local index does not fit usize")
                .ok()
                .and_then(|index| locals.get(index))
                .is_some_and(|decl| matches!(decl.kind, LocalKind::Temp))
    });
    absent
}

/// The absent value `operand` is: a `null`/`undefined` constant, or a
/// temporary that only ever holds one (`absent_locals`, from
/// [`absent_only_locals`]).
pub(crate) fn operand_absence(
    operand: &Operand,
    absent_locals: &HashMap<LocalId, Absent>,
) -> Option<Absent> {
    if matches!(operand, Operand::Const(Constant::None)) {
        return Some(Absent::Null);
    }
    if matches!(operand, Operand::Const(Constant::Undefined)) {
        return Some(Absent::Undefined);
    }
    if let Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) = operand {
        return absent_locals.get(local).copied();
    }
    None
}

/// Whether `op` is an equality or inequality test.
pub(crate) const fn is_equality(op: BinOp) -> bool {
    matches!(
        op,
        BinOp::Eq
            | BinOp::NotEq
            | BinOp::StrictEq
            | BinOp::StrictNotEq
            | BinOp::JsStrictEq
            | BinOp::JsStrictNotEq
    )
}

/// Whether `rvalue` is a scalar inspection (see the module docs) of a value of
/// the bare type parameter `name`: every operand of it that could carry a `T`
/// is a whole local of type `T` (or `T | undefined`), and the operation is
/// answered from an erased copy alone.
pub(crate) fn is_scalar_inspection(
    mir: &Mir,
    locals: &[LocalDecl],
    rvalue: &Rvalue,
    name: Symbol,
    absent_locals: &HashMap<LocalId, Absent>,
) -> bool {
    let bare = |operand: &Operand| operand_is_bare_type_param(mir, locals, operand, name);
    if let Rvalue::Binary { op, lhs, rhs } = rvalue {
        return is_equality(*op)
            && ((bare(lhs) && operand_absence(rhs, absent_locals).is_some())
                || (bare(rhs) && operand_absence(lhs, absent_locals).is_some()));
    }
    if let Rvalue::TypeofValue { value } | Rvalue::UnknownIs { value, .. } = rvalue {
        return bare(value);
    }
    if let Rvalue::PrimitiveCast {
        op: PrimitiveCastOp::ToBool,
        operand,
    }
    | Rvalue::Unary {
        op: UnaryOp::Not,
        operand,
    } = rvalue
    {
        return bare(operand);
    }
    false
}

/// Whether `rvalue` hands a value of the bare type parameter `name` to a
/// callback parameter that reads it as something else built from `name`
/// (`recursive(item)` with `item: T` narrowed by `Array.isArray` and
/// `recursive: (arr: T[]) => void`).
///
/// TypeScript accepts the call after the guard; under erasure it works
/// because the value is whatever it is at run time. A lifted `T` instantiated
/// with `number[]` would have its elements recovered as `T` (`number[]`
/// again), which they are not: es-toolkit's `flatMap` printed nested empty
/// lists that way. Such a body reinterprets its `T`, so `name` does not lift.
pub(crate) fn reinterprets_type_param(
    mir: &Mir,
    locals: &[LocalDecl],
    rvalue: &Rvalue,
    name: Symbol,
) -> bool {
    let Rvalue::ClosureCall { callee, args } = rvalue else {
        return false;
    };
    let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = callee else {
        return false;
    };
    let Some(decl) = id_index(local.0, "local index does not fit usize")
        .ok()
        .and_then(|index| locals.get(index))
    else {
        return false;
    };
    let signature = match mir.types.get(decl.ty) {
        Some(Type::Optional(inner)) => mir.types.get(*inner),
        other => other,
    };
    let Some(Type::Function(function)) = signature else {
        return false;
    };
    let is_bare = |ty: smelt_hir::TypeId| {
        let payload = match mir.types.get(ty) {
            Some(Type::Optional(inner)) => *inner,
            _ => ty,
        };
        matches!(mir.types.get(payload), Some(Type::TypeParam { name: found }) if *found == name)
    };
    args.iter().zip(&function.params).any(|(arg, param)| {
        operand_is_bare_type_param(mir, locals, arg, name)
            && crate::generic_bindings::type_param_occurs(mir, *param, name)
            && !is_bare(*param)
    })
}
