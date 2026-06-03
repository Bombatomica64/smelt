//! Literals emission helpers.

use super::*;
use crate::rust::RustExpr;

impl FunctionEmitter<'_> {}

/// Converts a MIR constant to Rust source text.
pub(super) fn constant_text(constant: &Constant) -> String {
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
                "SmeltUnknown::Symbol({}.to_owned())",
                RustExpr::string_literal(value).into_string()
            )
        }
        Constant::None => "()".to_owned(),
    }
}

/// Converts a HIR literal used inside callback trees to Rust source text.
pub(super) fn hir_literal_text(literal: &smelt_hir::Literal) -> String {
    match literal {
        smelt_hir::Literal::Bool(value) => value.to_string(),
        smelt_hir::Literal::Int(value) => value.to_string(),
        smelt_hir::Literal::Float(value) => {
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
        smelt_hir::Literal::String(value) => RustExpr::string_literal(value).into_string(),
        smelt_hir::Literal::Symbol(value) => {
            format!(
                "SmeltUnknown::Symbol({}.to_owned())",
                RustExpr::string_literal(value).into_string()
            )
        }
        smelt_hir::Literal::None => "()".to_owned(),
    }
}

/// Computes the set of locals that are assigned after their initial declaration.
pub(super) fn assigned_locals(mir: &Mir, function: &MirFunction) -> HashSet<LocalId> {
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
                value: Rvalue::Closure { id, .. },
                ..
            } = statement
                && let Some(callback) = mir
                    .closures
                    .get(usize::try_from(id.0).unwrap_or(usize::MAX))
                    .and_then(|closure| closure.callback_body.as_ref())
            {
                assigned_callback_locals(callback, &mut locals);
            }
            if let Statement::Assign {
                value:
                    Rvalue::ListSort {
                        comparator: Some(callback),
                        ..
                    },
                ..
            } = statement
            {
                assigned_callback_locals(callback, &mut locals);
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
                && let Some(local) = operand_local(list)
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
            && let Some(Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) =
                args.first()
        {
            locals.insert(*local);
        }
    }
    locals
}

/// Adds locals assigned by embedded callback expression trees.
fn assigned_callback_locals(callback: &smelt_hir::CallbackExpr, locals: &mut HashSet<LocalId>) {
    match &callback.kind {
        smelt_hir::CallbackExprKind::AssignCapture { target, value } => {
            locals.insert(LocalId(target.0));
            assigned_callback_locals(value, locals);
        }
        smelt_hir::CallbackExprKind::ListLit(items) => {
            for item in items {
                assigned_callback_locals(item, locals);
            }
        }
        smelt_hir::CallbackExprKind::Sequence { effects, result } => {
            for effect in effects {
                assigned_callback_locals(effect, locals);
            }
            assigned_callback_locals(result, locals);
        }
        smelt_hir::CallbackExprKind::DictLit(entries) => {
            for (_, value) in entries {
                assigned_callback_locals(value, locals);
            }
        }
        smelt_hir::CallbackExprKind::Throw { message } => {
            if let Some(thrown_message) = message {
                assigned_callback_locals(thrown_message, locals);
            }
        }
        smelt_hir::CallbackExprKind::Index { receiver, .. }
        | smelt_hir::CallbackExprKind::Field { receiver, .. }
        | smelt_hir::CallbackExprKind::HasField { receiver, .. }
        | smelt_hir::CallbackExprKind::FieldTruthy { receiver, .. } => {
            assigned_callback_locals(receiver, locals);
        }
        smelt_hir::CallbackExprKind::DynamicIndex { receiver, index } => {
            assigned_callback_locals(receiver, locals);
            assigned_callback_locals(index, locals);
        }
        smelt_hir::CallbackExprKind::HasDynamicField { receiver, field } => {
            assigned_callback_locals(receiver, locals);
            assigned_callback_locals(field, locals);
        }
        smelt_hir::CallbackExprKind::Unary { operand, .. } => {
            assigned_callback_locals(operand, locals);
        }
        smelt_hir::CallbackExprKind::UnknownIs { value, .. } => {
            assigned_callback_locals(value, locals);
        }
        smelt_hir::CallbackExprKind::TypeofValue { value } => {
            assigned_callback_locals(value, locals);
        }
        smelt_hir::CallbackExprKind::Binary { lhs, rhs, .. } => {
            assigned_callback_locals(lhs, locals);
            assigned_callback_locals(rhs, locals);
        }
        smelt_hir::CallbackExprKind::Conditional {
            cond,
            then_expr,
            else_expr,
        } => {
            assigned_callback_locals(cond, locals);
            assigned_callback_locals(then_expr, locals);
            assigned_callback_locals(else_expr, locals);
        }
        smelt_hir::CallbackExprKind::Call { callee, args } => {
            assigned_callback_locals(callee, locals);
            for arg in args {
                assigned_callback_locals(&arg.expr, locals);
            }
        }
        smelt_hir::CallbackExprKind::MethodCall { receiver, args, .. } => {
            assigned_callback_locals(receiver, locals);
            for arg in args {
                assigned_callback_locals(&arg.expr, locals);
            }
        }
        smelt_hir::CallbackExprKind::FunctionTableLookup { key, .. } => {
            assigned_callback_locals(key, locals);
        }
        smelt_hir::CallbackExprKind::Param(_)
        | smelt_hir::CallbackExprKind::Capture(_)
        | smelt_hir::CallbackExprKind::Function(_)
        | smelt_hir::CallbackExprKind::Literal(_) => {}
    }
}

/// Extracts the local base from a direct local operand.
pub(super) fn operand_local(operand: &Operand) -> Option<LocalId> {
    match operand {
        Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => Some(*local),
        Operand::Copy(_) | Operand::Move(_) | Operand::Const(_) => None,
    }
}

/// Checks if a method mutates the `this` parameter (self).
pub(super) fn method_mutates_this(function: &MirFunction) -> bool {
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
