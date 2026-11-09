//! Literals emission helpers.

use super::*;
use crate::rust::RustExpr;

/// Converts a MIR constant to Rust source text.
///
/// `js_text` says whether a string constant is a JavaScript string
/// (`JsString`) rather than a Python `str` (`String`).
pub(crate) fn constant_expr(constant: &Constant, js_text: bool) -> RustExpr {
    match constant {
        Constant::Bool(value) => RustExpr::path(value.to_string()),
        Constant::Int(value) => number_literal(value.to_string(), *value < 0),
        Constant::BigInt(digits) => crate::bigint_prelude::literal_expr(digits),
        Constant::Float(value) => {
            if value.is_infinite() {
                return if value.is_sign_negative() {
                    RustExpr::atom("f64::NEG_INFINITY")
                } else {
                    RustExpr::atom("f64::INFINITY")
                };
            }
            if value.is_nan() {
                return RustExpr::atom("f64::NAN");
            }
            if value.fract() == 0.0 {
                number_literal(format!("{value:.1}"), value.is_sign_negative())
            } else {
                number_literal(value.to_string(), value.is_sign_negative())
            }
        }
        Constant::String(value) if js_text => RustExpr::js_string_literal(value),
        Constant::StringUnits(units) => RustExpr::js_string_units_literal(units),
        Constant::String(value) => RustExpr::string_literal(value),
        // A symbol constant is a typed `symbol`; an erased slot takes it
        // through the coercion seam like any other typed value.
        Constant::Symbol(value) => RustExpr::chain(format!("SmeltSymbol::from_spelling({value:?})")),
        Constant::Undefined => RustExpr::unit(),
        Constant::None => RustExpr::unit(),
    }
}

/// A number literal: a primary, unless the sign makes it a unary expression.
fn number_literal(text: String, negative: bool) -> RustExpr {
    if negative { RustExpr::loose(text) } else { RustExpr::chain(text) }
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
                && let Some(local) = list.local()
                && let Ok(local_idx) = id_index(local.0, "local index does not fit usize")
                && let Some(local_decl) = function.locals.get(local_idx)
                && let Some(Type::List(item_ty)) = mir.types.get(local_decl.ty)
                && matches!(mir.types.get(*item_ty), Some(Type::Function(_)))
            {
                locals.insert(local);
            }
            if let Statement::Assign { value, .. } = statement
                && let Some(local) = value.mutated_receiver().and_then(operand_mutation_root)
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
        if let Some(Terminator::Call { callee: Callee::Op(op, Some(_)), .. }) = &block.terminator
            && let Some(local) = op.mutated_receiver().and_then(operand_mutation_root)
        {
            locals.insert(local);
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
