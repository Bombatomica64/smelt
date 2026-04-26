use smelt_hir::{Type, TypeId};

use crate::{
    BuiltinFn, Callee, Constant, LocalId, LocalKind, Mir, Operand, Place, Rvalue, Statement,
    Terminator,
};

#[must_use]
pub fn format_compact(mir: &Mir) -> String {
    let mut out = String::new();
    for function in &mir.functions {
        let name = mir.symbols.get(function.name).unwrap_or("<unknown>");
        out.push_str(&format!(
            "fn {name} ({:?}) -> {}\n",
            function.id,
            type_ref(mir, function.return_ty)
        ));

        if !function.locals.is_empty() {
            out.push_str("  locals\n");
            for (idx, local) in function.locals.iter().enumerate() {
                out.push_str(&format!(
                    "    {} {}: {}\n",
                    local_ref(LocalId(idx as u32)),
                    local_kind_text(mir, local.kind),
                    type_ref(mir, local.ty)
                ));
            }
        }

        for block in &function.blocks {
            out.push_str(&format!("  bb{}:\n", block.id.0));
            for phi in &block.phis {
                out.push_str(&format!(
                    "    {} = phi {}\n",
                    local_ref(phi.dest),
                    type_ref(mir, phi.ty)
                ));
            }
            for statement in &block.statements {
                out.push_str(&format!("    {}\n", statement_text(statement)));
            }
            let terminator = block
                .terminator
                .as_ref()
                .map(terminator_text)
                .unwrap_or_else(|| "<missing terminator>".to_owned());
            out.push_str(&format!("    {terminator}\n"));
        }
        out.push('\n');
    }
    out
}

fn local_kind_text(mir: &Mir, kind: LocalKind) -> String {
    match kind {
        LocalKind::Param => "param".to_owned(),
        LocalKind::Temp => "temp".to_owned(),
        LocalKind::UserBinding(symbol) => {
            format!("user {}", mir.symbols.get(symbol).unwrap_or("<unknown>"))
        }
    }
}

fn statement_text(statement: &Statement) -> String {
    match statement {
        Statement::Assign { dest, value } => {
            format!("{} = {}", local_ref(*dest), rvalue_text(value))
        }
        Statement::StorageLive(local) => format!("StorageLive({})", local_ref(*local)),
        Statement::StorageDead(local) => format!("StorageDead({})", local_ref(*local)),
    }
}

fn rvalue_text(value: &Rvalue) -> String {
    match value {
        Rvalue::Use(operand) => operand_text(operand),
        Rvalue::Binary { op, lhs, rhs } => {
            format!(
                "{} {} {}",
                operand_text(lhs),
                smelt_hir::bin_op_text(*op),
                operand_text(rhs)
            )
        }
    }
}

fn terminator_text(terminator: &Terminator) -> String {
    match terminator {
        Terminator::Goto(target) => format!("goto bb{}", target.0),
        Terminator::Call {
            callee,
            args,
            dest,
            target,
        } => {
            let args = args.iter().map(operand_text).collect::<Vec<_>>().join(", ");
            format!(
                "{} = call {}({}) -> bb{}",
                local_ref(*dest),
                callee_text(callee),
                args,
                target.0
            )
        }
        Terminator::Switch {
            cond,
            then_block,
            else_block,
        } => format!(
            "switch {} ? bb{} : bb{}",
            operand_text(cond),
            then_block.0,
            else_block.0
        ),
        Terminator::Match {
            scrutinee,
            arms,
            default,
        } => {
            let arms = arms
                .iter()
                .map(|arm| format!("{} => bb{}", constant_text(&arm.label), arm.target.0))
                .collect::<Vec<_>>()
                .join(", ");
            let default = default
                .map(|target| {
                    if arms.is_empty() {
                        format!("_ => bb{}", target.0)
                    } else {
                        format!(", _ => bb{}", target.0)
                    }
                })
                .unwrap_or_default();
            format!("match {} {{{}{}}}", operand_text(scrutinee), arms, default)
        }
        Terminator::Return(operand) => format!("return {}", operand_text(operand)),
        Terminator::Unreachable => "unreachable".to_owned(),
    }
}

fn callee_text(callee: &Callee) -> String {
    match callee {
        Callee::Static(func) => format!("fn{}", func.0),
        Callee::Indirect(operand) => operand_text(operand),
        Callee::Builtin(BuiltinFn::ConsoleLog) => "@console_log".to_owned(),
    }
}

fn operand_text(operand: &Operand) -> String {
    match operand {
        Operand::Copy(place) => format!("copy {}", place_text(place)),
        Operand::Move(place) => format!("move {}", place_text(place)),
        Operand::Const(constant) => constant_text(constant),
    }
}

fn place_text(place: &Place) -> String {
    match place {
        Place::Local(local) => local_ref(*local),
    }
}

fn constant_text(constant: &Constant) -> String {
    match constant {
        Constant::Bool(value) => value.to_string(),
        Constant::Int(value) => value.to_string(),
        Constant::Float(value) => {
            if value.fract() == 0.0 {
                format!("{value:.1}")
            } else {
                value.to_string()
            }
        }
        Constant::String(value) => format!("\"{value}\""),
        Constant::None => "none".to_owned(),
    }
}

fn local_ref(local: LocalId) -> String {
    format!("%{}", local.0)
}

fn type_ref(mir: &Mir, ty: TypeId) -> String {
    let Some(ty_value) = mir.types.get(ty) else {
        return format!("t{}", ty.0);
    };
    match ty_value {
        Type::Bool => "Bool".to_owned(),
        Type::Int => "Int".to_owned(),
        Type::Float => "Float".to_owned(),
        Type::String => "String".to_owned(),
        Type::None => "None".to_owned(),
        Type::List(item) => format!("List<{}>", type_ref(mir, *item)),
        Type::Set(item) => format!("Set<{}>", type_ref(mir, *item)),
        Type::Dict(key, value) => {
            format!("Dict<{}, {}>", type_ref(mir, *key), type_ref(mir, *value))
        }
        Type::Tuple(items) => {
            let items = items
                .iter()
                .map(|item| type_ref(mir, *item))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({items})")
        }
        Type::Optional(item) => format!("Optional<{}>", type_ref(mir, *item)),
        Type::Union(items) => items
            .iter()
            .map(|item| type_ref(mir, *item))
            .collect::<Vec<_>>()
            .join(" | "),
        Type::Class { name, args } => {
            let name = mir.symbols.get(*name).unwrap_or("<unknown>");
            if args.is_empty() {
                name.to_owned()
            } else {
                let args = args
                    .iter()
                    .map(|arg| type_ref(mir, *arg))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{name}<{args}>")
            }
        }
        Type::Function(function) => {
            let params = function
                .params
                .iter()
                .map(|param| type_ref(mir, *param))
                .collect::<Vec<_>>()
                .join(", ");
            let async_prefix = if function.is_async { "async " } else { "" };
            format!(
                "{async_prefix}fn({params}) -> {}",
                type_ref(mir, function.return_ty)
            )
        }
        Type::Future(item) => format!("Future<{}>", type_ref(mir, *item)),
    }
}
