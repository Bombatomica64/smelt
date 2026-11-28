//! Settling the implicit type parameters the frontend gives `any` parameters.
//!
//! The TypeScript frontend declares, for a module function's plain `x: any`
//! parameter, a fresh type parameter whose
//! [`smelt_hir::TypeParamDef::implicit_param`] names that parameter
//! (`docs/design/any-as-generic.md`); the parameter's local stays erased
//! through MIR lowering. Such a parameter is a real generic only where the
//! body passes the value along without inspecting it. This pass decides that
//! from MIR, once, before any emitter or measurement reads the program:
//!
//! * a parameter whose every use passes the value along ([`passes_through`])
//!   and that every caller fills with a value the lifted parameter carries
//!   as the erased carrier did ([`callers_pass_carriable_values`]) has its
//!   local retyped to the type parameter, and the ordinary lifting decision
//!   (`EmitContext::populate_generic_functions`) treats it like a declared one;
//! * every other one is dropped from the function's type parameters, so that
//!   function is the program it was before this rule existed, byte for byte.
//!   Its erasure is recorded as `explicit-any` at the annotation, the record
//!   the frontend retracted when it declared the parameter.
//!
//! Requirements (checked by `implicit_generics` tests and the corpora): a
//! settled program compiles and behaves as the source does, a lifted implicit
//! parameter carries the caller's static type into the callee up to the
//! first dynamic boundary in its body (where it crosses `IntoSmeltUnknown`),
//! and a function this rule does not lift emits exactly what it emitted before.

#![expect(
    clippy::redundant_pub_crate,
    reason = "`settled` is shared with the crate's emission entry points"
)]

use std::{borrow::Cow, collections::HashMap};

use smelt_hir::{ErasureReason, ErasureRecord, Symbol, Type, TypeId};
use smelt_mir::{Callee, LocalId, Mir, MirFunction, Operand, Place, Rvalue, Statement, Terminator};

/// Settle every implicit type parameter of `mir` (see the module docs),
/// returning the `explicit-any` records of the parameters left erased.
/// Idempotent: a settled program has no unsettled implicit parameter.
pub fn settle_implicit_generics(mir: &mut Mir) -> Vec<ErasureRecord> {
    let mut records = Vec::new();
    for index in 0..mir.functions.len() {
        let Some(function) = mir.functions.get(index) else {
            continue;
        };
        let mut lifted: Vec<(LocalId, TypeId)> = Vec::new();
        let mut dropped: Vec<Symbol> = Vec::new();
        for param in &function.type_params {
            let Some(position) = param.implicit_param else {
                continue;
            };
            match settle_one(mir, function, param.name, position) {
                Ok(Some(retype)) => lifted.push(retype),
                Ok(None) => {}
                Err(refusal) => {
                    dropped.push(param.name);
                    let name = mir.symbols.get(param.name).unwrap_or("?");
                    let owner = mir.symbols.get(function.name).unwrap_or("<anonymous>");
                    records.push(ErasureRecord::new(
                        ErasureReason::ExplicitAny,
                        param.span,
                        format!("{name} of {owner}: {refusal}"),
                    ));
                }
            }
        }
        let Some(settled) = mir.functions.get_mut(index) else {
            continue;
        };
        settled
            .type_params
            .retain(|param| !dropped.contains(&param.name));
        for (local, ty) in lifted {
            if let Some(decl) = usize::try_from(local.0)
                .ok()
                .and_then(|slot| settled.locals.get_mut(slot))
            {
                decl.ty = ty;
            }
        }
    }
    records
}

/// `mir` with every implicit type parameter settled: borrowed when it
/// already is (the transpiler settles once, in `lower_to_emittable_mir`), a
/// settled copy otherwise, so a caller that hands the emitter MIR straight
/// from lowering (codegen's own tests, embedders) emits the same program.
/// The copy's records are dropped: only the transpiler writes a sidecar.
pub(crate) fn settled(mir: &Mir) -> Cow<'_, Mir> {
    let unsettled = mir.functions.iter().any(|function| {
        function.type_params.iter().any(|param| {
            param.implicit_param.is_some_and(|position| {
                !function
                    .params
                    .get(position)
                    .and_then(|local| usize::try_from(local.0).ok())
                    .and_then(|slot| function.locals.get(slot))
                    .is_some_and(|decl| {
                        mir.types.get(decl.ty) == Some(&Type::TypeParam { name: param.name })
                    })
            })
        })
    });
    if !unsettled {
        return Cow::Borrowed(mir);
    }
    let mut copy = mir.clone();
    let _records = settle_implicit_generics(&mut copy);
    Cow::Owned(copy)
}

/// Decide the implicit parameter `name`, standing for parameter `position`
/// of `function`: `Ok(Some((local, ty)))` to retype that local to `ty`,
/// `Ok(None)` when it already is (the pass is idempotent), or `Err` with the
/// refusal the `explicit-any` record names.
fn settle_one(
    mir: &Mir,
    function: &MirFunction,
    name: Symbol,
    position: usize,
) -> Result<Option<(LocalId, TypeId)>, &'static str> {
    const NOT_ERASED: &str = "the parameter is not the erased carrier";
    let local = function.params.get(position).copied().ok_or(NOT_ERASED)?;
    let decl = usize::try_from(local.0)
        .ok()
        .and_then(|slot| function.locals.get(slot))
        .ok_or(NOT_ERASED)?;
    let wanted = Type::TypeParam { name };
    if mir.types.get(decl.ty) == Some(&wanted) {
        return Ok(None);
    }
    if !matches!(mir.types.get(decl.ty), Some(Type::Unknown)) {
        return Err(NOT_ERASED);
    }
    let ty = mir
        .types
        .lookup_like(decl.ty, &wanted)
        .unwrap_or_else(|| mir.types.intern_late(wanted));
    if !passes_through(mir, function, local, ty) {
        return Err("the body does more than pass the value along");
    }
    if !callers_pass_carriable_values(mir, function, position) {
        return Err("a caller passes a function, a future or a nullish value");
    }
    Ok(Some((local, ty)))
}

/// Whether every use of the erased parameter `param` of `function` passes
/// the value along, judged as if `param` had the type parameter type
/// `param_ty`:
///
/// * copied whole into an erased local (the `IntoSmeltUnknown` crossing);
/// * passed whole to a statically called function at a parameter that is
///   erased or one of that callee's own unbounded bare type parameters;
/// * returned whole from a function whose return is erased;
/// * asked a scalar question answered on an erased copy (`x == null`,
///   `typeof x`, `!x`; [`crate::generic_inspection`]).
///
/// Anything else that reads it (a member or index read, an operator, a
/// branch, a store into a container, a closure capture, an indirect call) is
/// an inspection, and any write to it (a reassigned parameter) refills the
/// slot from elsewhere: both reject the parameter. The list is an allow-list
/// on purpose, so a new MIR form cannot make an inspected `any` generic by
/// default.
fn passes_through(mir: &Mir, function: &MirFunction, param: LocalId, param_ty: TypeId) -> bool {
    let mut locals = function.locals.clone();
    if let Some(decl) = usize::try_from(param.0)
        .ok()
        .and_then(|slot| locals.get_mut(slot))
    {
        decl.ty = param_ty;
    }
    let Some(Type::TypeParam { name }) = mir.types.get(param_ty).cloned() else {
        return false;
    };
    let local_ty = |local: LocalId| {
        usize::try_from(local.0)
            .ok()
            .and_then(|index| locals.get(index))
            .map(|decl| decl.ty)
    };
    let is_erased = |found: Option<TypeId>| {
        matches!(found.and_then(|ty| mir.types.get(ty)), Some(Type::Unknown))
    };
    let mentions = |operand: &Operand| {
        operand
            .place()
            .is_some_and(|place| place_mentions(place, param))
    };
    let whole = |operand: &Operand| matches!(operand, Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) if *local == param);
    let absent_only = crate::generic_inspection::absent_only_locals(&locals, &function.blocks);
    for block in &function.blocks {
        for statement in &block.statements {
            match statement {
                Statement::Assign { dest, value } => {
                    if *dest == param {
                        return false;
                    }
                    if let Rvalue::Use(operand) = value
                        && mentions(operand)
                    {
                        if !(whole(operand) && is_erased(local_ty(*dest))) {
                            return false;
                        }
                        continue;
                    }
                    if crate::generic_inspection::is_scalar_inspection(
                        mir,
                        &locals,
                        value,
                        name,
                        &absent_only,
                    ) {
                        continue;
                    }
                    let mut reads = false;
                    value.reads(|operand| reads |= mentions(operand));
                    if reads {
                        return false;
                    }
                }
                Statement::AssignPlace { .. } | Statement::DictEntryUpdate { .. } => {
                    let mut reads = false;
                    statement.reads(|read| {
                        reads |= match read {
                            smelt_mir::Read::Operand(operand) => mentions(operand),
                            smelt_mir::Read::Base(base) => base == param,
                        };
                    });
                    let writes = matches!(
                        statement,
                        Statement::AssignPlace { place: Place::Local(local), .. } if *local == param
                    );
                    if reads || writes {
                        return false;
                    }
                }
                Statement::StorageLive(_) => {}
            }
        }
        let Some(terminator) = &block.terminator else {
            continue;
        };
        match terminator {
            Terminator::Call {
                callee: Callee::Static(target),
                args,
                dest,
                ..
            } => {
                if *dest == param {
                    return false;
                }
                let callee = mir.function(*target);
                for (position, arg) in args.iter().enumerate() {
                    if !mentions(arg) {
                        continue;
                    }
                    let accepts = whole(arg)
                        && callee.is_some_and(|target_fn| {
                            callee_param_accepts_any(mir, target_fn, position)
                        });
                    if !accepts {
                        return false;
                    }
                }
            }
            Terminator::Return(operand) => {
                if mentions(operand) && !(whole(operand) && is_erased(Some(function.return_ty))) {
                    return false;
                }
            }
            Terminator::Goto(_) => {}
            Terminator::Call {
                callee, args, dest, ..
            } => {
                if *dest == param || args.iter().any(mentions) {
                    return false;
                }
                let reads = match callee {
                    Callee::Indirect(operand) => mentions(operand),
                    Callee::Op(rvalue, _) => {
                        let mut reads = false;
                        rvalue.reads(|operand| reads |= mentions(operand));
                        reads
                    }
                    Callee::Static(_) | Callee::Builtin(_) | Callee::BaseInit(_) => false,
                };
                if reads {
                    return false;
                }
            }
            Terminator::Await {
                future: operand,
                dest,
                ..
            } => {
                if *dest == param || mentions(operand) {
                    return false;
                }
            }
            Terminator::Switch { cond: operand, .. }
            | Terminator::Match {
                scrutinee: operand, ..
            }
            | Terminator::Throw(operand) => {
                if mentions(operand) {
                    return false;
                }
            }
        }
    }
    true
}

/// Whether `place` reads `param`: as its base, or anywhere in an index
/// operand (`globalThis[name]` keys a lookup with the parameter, which reads
/// its value as a property key).
fn place_mentions(place: &Place, param: LocalId) -> bool {
    match place {
        Place::Local(local) | Place::Field { base: local, .. } => *local == param,
        Place::Index { base, index, .. } => {
            *base == param
                || index
                    .place()
                    .is_some_and(|inner| place_mentions(inner, param))
        }
        Place::Global { projection, .. } => match projection {
            smelt_mir::GlobalProjection::Index { index, .. } => index
                .place()
                .is_some_and(|inner| place_mentions(inner, param)),
            smelt_mir::GlobalProjection::Field(_) => false,
        },
    }
}

/// Whether parameter `position` of `callee` takes an `any` value as it is:
/// it is erased, or it is one of the callee's own bare type parameters (a
/// generic the argument instantiates).
fn callee_param_accepts_any(mir: &Mir, callee: &MirFunction, position: usize) -> bool {
    if callee.rest.is_some_and(|rest| position >= rest) {
        return false;
    }
    let Some(ty) = callee
        .params
        .get(position)
        .and_then(|local| usize::try_from(local.0).ok())
        .and_then(|index| callee.locals.get(index))
        .map(|decl| decl.ty)
    else {
        return false;
    };
    match mir.types.get(ty) {
        Some(Type::Unknown) => true,
        Some(Type::TypeParam { name }) => callee
            .type_params
            .iter()
            .any(|param| param.name == *name && param.constraint.is_none()),
        _ => false,
    }
}

/// Whether every static caller of `function` passes, at parameter
/// `position`, a value the lifted parameter carries exactly as the erased
/// carrier did. A function value and a future have no `IntoSmeltUnknown` /
/// `SmeltFromUnknown` conversion. A nullish value (`null`, `undefined`, an
/// optional, or a container of one) renders as `()` or `Option<_>`, which
/// keep one absent value where the erased carrier keeps `null` apart from
/// `undefined`: `isUndefined(undefined)` would read back `null`. A parameter
/// some caller fills with such a value keeps the erased carrier, whose
/// call-site adapter converts the value with its absence kind.
fn callers_pass_carriable_values(mir: &Mir, function: &MirFunction, position: usize) -> bool {
    let mut memo = HashMap::new();
    mir.static_callers(function.id).all(|caller| {
        caller.blocks.iter().all(|block| {
            let Some(Terminator::Call {
                callee: Callee::Static(target),
                args,
                ..
            }) = &block.terminator
            else {
                return true;
            };
            if *target != function.id {
                return true;
            }
            let Some(arg) = args.get(position) else {
                return true;
            };
            crate::generic_bindings::operand_type(mir, caller, arg)
                .is_ok_and(|ty| !holds_uncarriable(mir, ty, &mut memo, 0))
        })
    })
}

/// Whether `ty` is, or contains, a function, a future, a nullish value or
/// `never` (see [`callers_pass_carriable_values`]), memoized per type.
fn holds_uncarriable(
    mir: &Mir,
    ty: TypeId,
    memo: &mut HashMap<TypeId, bool>,
    depth: usize,
) -> bool {
    if depth > 32 {
        return true;
    }
    if let Some(known) = memo.get(&ty) {
        return *known;
    }
    let answer = match mir.types.get(ty) {
        None
        | Some(
            Type::Function(_) | Type::Future(_) | Type::None | Type::Optional(..) | Type::Never,
        ) => true,
        Some(kind) => kind
            .children()
            .into_iter()
            .any(|child| holds_uncarriable(mir, child, memo, depth.saturating_add(1))),
    };
    memo.insert(ty, answer);
    answer
}
