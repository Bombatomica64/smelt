use crate::body::{Body, Pattern, Stmt};
use crate::expr::{Expr, ExprKind, Literal};
use crate::ids::{ExprId, ItemId, LocalId, ModuleId, PatternId, TypeId};
use crate::item::Item;
use crate::krate::Crate;
use crate::ty::Type;

pub fn format_compact(krate: &Crate, modules: &[(String, ModuleId)]) -> String {
    let mut out = String::new();

    for (path, module_id) in modules {
        let module = &krate.modules[module_id.0 as usize];
        out.push_str(&format!("module {path} ({module_id:?})\n"));

        let Some(body_id) = module.body else {
            out.push_str("  <no body>\n\n");
            continue;
        };

        let body = &krate.bodies[body_id.0 as usize];
        out.push_str(&format!("  body {body_id:?}\n"));

        if !body.locals.is_empty() {
            out.push_str("  locals\n");
            for (idx, local) in body.locals.iter().enumerate() {
                let local_id = LocalId(idx as u32);
                let mutability = if local.mutable { "let" } else { "const" };
                let name = local
                    .name
                    .and_then(|symbol| {
                        krate
                            .names
                            .get(symbol)
                            .or_else(|| krate.symbols.get(symbol))
                    })
                    .unwrap_or("_");
                out.push_str(&format!(
                    "    {} {} {}: {}\n",
                    local_ref(local_id),
                    mutability,
                    name,
                    type_ref(krate, local.ty)
                ));
            }
        }

        if !body.exprs.is_empty() {
            out.push_str("  exprs\n");
            for (idx, expr) in body.exprs.iter().enumerate() {
                let expr_id = ExprId(idx as u32);
                out.push_str(&format!(
                    "    {}: {} = {}\n",
                    expr_ref(expr_id),
                    type_ref(krate, expr.ty),
                    expr_text(krate, expr)
                ));
            }
        }

        if !body.stmts.is_empty() {
            out.push_str("  stmts\n");
            for (idx, stmt) in body.stmts.iter().enumerate() {
                out.push_str(&format!("    s{}: {}\n", idx, stmt_text(krate, body, stmt)));
            }
        }

        out.push('\n');
    }

    out.push_str("interned types\n");
    for (idx, ty) in krate.types.all().iter().enumerate() {
        out.push_str(&format!("  t{} = {}\n", idx, type_text(krate, ty)));
    }

    out
}

fn stmt_text(krate: &Crate, body: &Body, stmt: &Stmt) -> String {
    match stmt {
        Stmt::Let { pat, ty, value } => {
            let value = value
                .map(|value| format!(" = {}", expr_ref(value)))
                .unwrap_or_default();
            format!(
                "let {}: {}{}",
                pattern_text(body, *pat),
                type_ref(krate, *ty),
                value
            )
        }
        Stmt::Expr(expr) => expr_ref(*expr),
        Stmt::Return(Some(expr)) => format!("return {}", expr_ref(*expr)),
        Stmt::Return(None) => "return".to_owned(),
        Stmt::Throw(expr) => format!("throw {}", expr_ref(*expr)),
        Stmt::Break => "break".to_owned(),
        Stmt::Continue => "continue".to_owned(),
    }
}

fn expr_text(krate: &Crate, expr: &Expr) -> String {
    match &expr.kind {
        ExprKind::Literal(literal) => literal_text(literal),
        ExprKind::Local(local) => local_ref(*local),
        ExprKind::Item(item) => item_ref(krate, *item),
        ExprKind::Call { callee, args } => {
            let args = args
                .iter()
                .map(|arg| expr_ref(*arg))
                .collect::<Vec<_>>()
                .join(", ");
            format!("call {}({})", expr_ref(*callee), args)
        }
        ExprKind::Method {
            receiver,
            method,
            args,
        } => {
            let method = krate.symbols.get(*method).unwrap_or("<unknown>");
            let args = args
                .iter()
                .map(|arg| expr_ref(*arg))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{}.{}({})", expr_ref(*receiver), method, args)
        }
        ExprKind::Field { receiver, field } => {
            let field = krate.symbols.get(*field).unwrap_or("<unknown>");
            format!("{}.{}", expr_ref(*receiver), field)
        }
        ExprKind::Index { receiver, index } => {
            format!("{}[{}]", expr_ref(*receiver), expr_ref(*index))
        }
        ExprKind::BinOp { op, lhs, rhs } => {
            format!("{op:?} {}, {}", expr_ref(*lhs), expr_ref(*rhs))
        }
        ExprKind::UnaryOp { op, operand } => format!("{op:?} {}", expr_ref(*operand)),
        ExprKind::Block(block) => format!("block {block:?}"),
        ExprKind::Lambda { body, return_ty } => {
            format!("lambda {body:?} -> {}", type_ref(krate, *return_ty))
        }
        ExprKind::ListLit(items) => collection_text("[", "]", items),
        ExprKind::SetLit(items) => collection_text("set{", "}", items),
        ExprKind::DictLit(items) => {
            let items = items
                .iter()
                .map(|(key, value)| format!("{}: {}", expr_ref(*key), expr_ref(*value)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{items}}}")
        }
        ExprKind::TupleLit(items) => collection_text("(", ")", items),
        ExprKind::New { class, args } => {
            let class = krate.symbols.get(*class).unwrap_or("<unknown>");
            let args = args
                .iter()
                .map(|arg| expr_ref(*arg))
                .collect::<Vec<_>>()
                .join(", ");
            format!("new {class}({args})")
        }
    }
}

fn collection_text(open: &str, close: &str, items: &[ExprId]) -> String {
    let items = items
        .iter()
        .map(|item| expr_ref(*item))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{open}{items}{close}")
}

fn pattern_text(body: &Body, pattern: PatternId) -> String {
    match &body.patterns[pattern.0 as usize] {
        Pattern::Wildcard => "_".to_owned(),
        Pattern::Binding(local) => local_ref(*local),
        Pattern::Tuple(items) => {
            let items = items
                .iter()
                .map(|item| pattern_text(body, *item))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({items})")
        }
        Pattern::Literal(literal) => literal_text(literal),
    }
}

fn literal_text(literal: &Literal) -> String {
    match literal {
        Literal::Bool(value) => value.to_string(),
        Literal::Int(value) => value.to_string(),
        Literal::Float(value) => {
            if value.fract() == 0.0 {
                format!("{value:.1}")
            } else {
                value.to_string()
            }
        }
        Literal::String(value) => format!("\"{value}\""),
        Literal::None => "none".to_owned(),
    }
}

fn item_ref(krate: &Crate, item: ItemId) -> String {
    let Some(item_value) = krate.items.get(item.0 as usize) else {
        return format!("item{}", item.0);
    };

    let name = match item_value {
        Item::Function(function) => krate.symbols.get(function.name),
        Item::Class(class) => krate.symbols.get(class.name),
        Item::TypeAlias(alias) => krate.symbols.get(alias.name),
        Item::Const(item) => krate.symbols.get(item.name),
    }
    .unwrap_or("<unknown>");

    format!("@{}({})", item.0, name)
}

fn type_ref(krate: &Crate, ty: TypeId) -> String {
    let Some(ty_value) = krate.types.get(ty) else {
        return format!("t{}", ty.0);
    };
    type_text(krate, ty_value)
}

fn type_text(krate: &Crate, ty: &Type) -> String {
    match ty {
        Type::Bool => "Bool".to_owned(),
        Type::Int => "Int".to_owned(),
        Type::Float => "Float".to_owned(),
        Type::String => "String".to_owned(),
        Type::None => "None".to_owned(),
        Type::List(item) => format!("List<{}>", type_ref(krate, *item)),
        Type::Set(item) => format!("Set<{}>", type_ref(krate, *item)),
        Type::Dict(key, value) => {
            format!(
                "Dict<{}, {}>",
                type_ref(krate, *key),
                type_ref(krate, *value)
            )
        }
        Type::Tuple(items) => {
            let items = items
                .iter()
                .map(|item| type_ref(krate, *item))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({items})")
        }
        Type::Optional(item) => format!("Optional<{}>", type_ref(krate, *item)),
        Type::Union(items) => items
            .iter()
            .map(|item| type_ref(krate, *item))
            .collect::<Vec<_>>()
            .join(" | "),
        Type::Class { name, args } => {
            let name = krate.symbols.get(*name).unwrap_or("<unknown>");
            if args.is_empty() {
                name.to_owned()
            } else {
                let args = args
                    .iter()
                    .map(|arg| type_ref(krate, *arg))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{name}<{args}>")
            }
        }
        Type::Function(function) => {
            let params = function
                .params
                .iter()
                .map(|param| type_ref(krate, *param))
                .collect::<Vec<_>>()
                .join(", ");
            let async_prefix = if function.is_async { "async " } else { "" };
            format!(
                "{async_prefix}fn({params}) -> {}",
                type_ref(krate, function.return_ty)
            )
        }
        Type::Future(item) => format!("Future<{}>", type_ref(krate, *item)),
    }
}

fn local_ref(local: LocalId) -> String {
    format!("%{}", local.0)
}

fn expr_ref(expr: ExprId) -> String {
    format!("#{}", expr.0)
}
