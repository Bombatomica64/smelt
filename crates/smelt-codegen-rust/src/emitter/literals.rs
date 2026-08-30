//! Literals emission helpers.

use super::*;
use crate::rust::RustExpr;

/// Converts a MIR constant to Rust source text.
pub(crate) fn constant_text(constant: &Constant) -> String {
    match constant {
        Constant::Bool(value) => value.to_string(),
        Constant::Int(value) => value.to_string(),
        Constant::Float(value) => {
            if value.is_infinite() {
                return if value.is_sign_negative() {
                    "f64::NEG_INFINITY".to_owned()
                } else {
                    "f64::INFINITY".to_owned()
                };
            }
            if value.is_nan() {
                return "f64::NAN".to_owned();
            }
            if value.fract() == 0.0 {
                format!("{value:.1}")
            } else {
                value.to_string()
            }
        }
        Constant::String(value) => RustExpr::string_literal(value).into_string(),
        Constant::Symbol(value) => {
            format!(
                "SmeltUnknown::Symbol({}.into())",
                RustExpr::string_literal(value).into_string()
            )
        }
        Constant::Undefined => "()".to_owned(),
        Constant::None => "()".to_owned(),
    }
}

/// Computes the set of locals that are assigned after their initial declaration.
pub(super) fn assigned_locals(
    mir: &Mir,
    context: &EmitContext,
    function: &MirFunction,
) -> HashSet<LocalId> {
    let mut locals = HashSet::new();
    let mut assigned_once = function.params.iter().copied().collect::<HashSet<_>>();
    for block in &function.blocks {
        for statement in &block.statements {
            if let Statement::Assign { dest, .. } = statement
                && !assigned_once.insert(*dest)
            {
                locals.insert(*dest);
            }
            if let Statement::Assign {
                value: Rvalue::ListCallback { list, .. },
                ..
            } = statement
                && let Some(local) = operand_local(list)
                && let Ok(local_idx) = id_index(local.0, "local index does not fit usize")
                && let Some(local_decl) = function.locals.get(local_idx)
                && let Some(Type::List(item_ty)) = mir.types.get(local_decl.ty)
                && matches!(mir.types.get(*item_ty), Some(Type::Function(_)))
            {
                locals.insert(local);
            }
            if let Statement::Assign {
                value:
                    Rvalue::ListPush { list, .. }
                    | Rvalue::ListExtend { list, .. }
                    | Rvalue::ListInsert { list, .. }
                    | Rvalue::ListUnshift { list, .. }
                    | Rvalue::ListReverse { list }
                    | Rvalue::ListSplice {
                        list, mutate: true, ..
                    }
                    | Rvalue::ListFill { list, .. }
                    | Rvalue::ListCopyWithin { list, .. }
                    | Rvalue::ListClear { list }
                    | Rvalue::ListRemove { list, .. }
                    | Rvalue::ListSort { list, .. }
                    | Rvalue::ListPop { list }
                    | Rvalue::ListShift { list }
                    | Rvalue::ListNext { list }
                    | Rvalue::SetAdd { set: list, .. }
                    | Rvalue::SetRemove { set: list, .. }
                    | Rvalue::SetClear { set: list }
                    | Rvalue::DictClear { dict: list }
                    | Rvalue::DictPop { dict: list, .. }
                    | Rvalue::DictSet { dict: list, .. }
                    | Rvalue::DictRemoveKey { dict: list, .. }
                    | Rvalue::DictSetDefault { dict: list, .. }
                    | Rvalue::DictUpdate { dict: list, .. },
                ..
            } = statement
                && let Some(local) = operand_mutation_root(list)
            {
                locals.insert(local);
            }
            if let Statement::AssignPlace {
                place: Place::Local(local),
                ..
            } = statement
            {
                locals.insert(*local);
            }
            if let Statement::AssignPlace {
                place: Place::Field { base, .. },
                ..
            } = statement
            {
                locals.insert(*base);
            }
            if let Statement::AssignPlace {
                place: Place::Index { base, .. },
                ..
            } = statement
            {
                locals.insert(*base);
            }
        }
        if let Some(Terminator::Call {
            callee: Callee::Static(func),
            args,
            ..
        }) = &block.terminator
            && let Ok(function_index) = id_index(func.0, "function index does not fit usize")
            && let Some(callee) = mir.functions.get(function_index)
            && method_mutates_this(callee)
            && !callee_is_reference_class_method(context, callee)
            && let Some(Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) =
                args.first()
        {
            locals.insert(*local);
        }
    }
    locals
}

/// Extracts the local a place-rooted mutation writes through.
///
/// A collection mutation whose receiver is a bare local mutates that local; one
/// whose receiver is an INDEX place (`base[key].push(x)`, produced by
/// `smelt_mir::opt::DictEntryInPlaceMutation`) mutates the CONTAINER, because the
/// stored entry is mutated in place through it. This keeps the container's Rust
/// binding mutable now that the fused form no longer writes it back with an
/// explicit `AssignPlace`, which is what used to mark it.
pub(super) fn operand_mutation_root(operand: &Operand) -> Option<LocalId> {
    match operand {
        Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => Some(*local),
        Operand::Copy(Place::Index { base, .. }) | Operand::Move(Place::Index { base, .. }) => {
            Some(*base)
        }
        Operand::Copy(_) | Operand::Move(_) | Operand::Const(_) => None,
    }
}

/// Extracts the local base from a direct local operand.
pub(super) fn operand_local(operand: &Operand) -> Option<LocalId> {
    match operand {
        Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => Some(*local),
        Operand::Copy(_) | Operand::Move(_) | Operand::Const(_) => None,
    }
}

/// Returns whether a callee is a method of a reference class.
///
/// Reference-class methods take `&self` and mutate through the shared cell, so
/// the caller's receiver never needs a defensive mutable binding — this prevents
/// the throwaway-clone-then-mutate miscompile.
fn callee_is_reference_class_method(context: &EmitContext, callee: &MirFunction) -> bool {
    match callee.origin {
        HirOrigin::ClassMethod { class, .. } | HirOrigin::ClassConstructor { class, .. } => {
            context.is_reference_class(class)
        }
        HirOrigin::ClassStaticMethod { .. } | HirOrigin::Body(_) => false,
    }
}

/// Checks if a method mutates the `this` parameter (self).
pub(crate) fn method_mutates_this(function: &MirFunction) -> bool {
    function.blocks.iter().any(|block| {
        block.statements.iter().any(|statement| {
            matches!(
                statement,
                Statement::AssignPlace {
                    place: Place::Field {
                        base: LocalId(0),
                        ..
                    },
                    ..
                }
            )
        })
    })
}

// Identifier sanitizing lives in the parent codegen module.
