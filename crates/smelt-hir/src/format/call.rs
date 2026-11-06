//! Expression formatting helpers, including call-like nodes.

use crate::expr::{Expr, ExprKind};
use crate::krate::Crate;
use crate::op_table::{opt, regex_replace_args_text, tail};

use super::{
    collection_text, dict_lit_text, expr_list_text, expr_ref, item_ref, literal_text, local_ref,
    type_ref,
};

/// Rebinds one op field to its text; see the display grammar in
/// [`crate::op_table`].
macro_rules! render_op_field {
    ($krate:ident, one, $field:ident) => { expr_ref(*$field) };
    ($krate:ident, opt, $field:ident) => { $field.map(expr_ref) };
    ($krate:ident, many, $field:ident) => {
        $field.iter().map(|child| expr_ref(*child)).collect::<Vec<_>>()
    };
    ($krate:ident, splice, $field:ident) => {
        $field
            .iter()
            .map(|item| {
                let value = expr_ref(item.value);
                if item.spread { format!("...{value}") } else { value }
            })
            .collect::<Vec<_>>()
    };
    ($krate:ident, symbol_pairs, $field:ident) => {
        $field
            .iter()
            .map(|(name, value)| {
                let name = $krate.symbols.get(*name).unwrap_or("<unknown>");
                format!("{name}: {}", expr_ref(*value))
            })
            .collect::<Vec<_>>()
    };
    ($krate:ident, { $key:ty }, $field:ident) => {
        $field
            .iter()
            .map(|(key, value)| (key, expr_ref(*value)))
            .collect::<Vec<_>>()
    };
    ($krate:ident, [$data:ty], $field:ident) => { $field };
    ($krate:ident, (sym $label:literal), $field:ident) => {
        $krate.symbols.get(*$field).unwrap_or("<unknown>").to_owned()
    };
    ($krate:ident, (ty), $field:ident) => { type_ref($krate, *$field) };
}

/// Generates [`expr_text`]: the arms of the forms only HIR has, then one
/// generated arm per op-table entry.
macro_rules! define_expr_text {
    ($(
        $(#[$meta:meta])*
        $name:ident $({ $($(#[$field_meta:meta])* $field:ident : $kind:tt),* $(,)? })?
        $($fact:ident $fact_value:tt)*
        => $display:tt;
    )*) => {
        /// Formats an expression as text.
        pub(super) fn expr_text(krate: &Crate, expr: &Expr) -> String {
            match &expr.kind {
                ExprKind::Literal(literal) => literal_text(literal),
                ExprKind::Local(local) => local_ref(*local),
                ExprKind::Item(item) => item_ref(krate, *item),
                ExprKind::GlobalGet { item } => format!("global_get {}", item_ref(krate, *item)),
                ExprKind::GlobalSet { item, value } => {
                    format!("global_set {} = {}", item_ref(krate, *item), expr_ref(*value))
                }
                ExprKind::GeneratorYield { value } => format!("yield {}", expr_ref(*value)),
                ExprKind::Call { .. }
                | ExprKind::Method { .. }
                | ExprKind::New { .. }
                | ExprKind::BaseConstructorInit { .. } => call_like_expr_text(krate, expr),
                ExprKind::Field { receiver, field } => {
                    let field_name = krate.symbols.get(*field).unwrap_or("<unknown>");
                    format!("{}.{}", expr_ref(*receiver), field_name)
                }
                ExprKind::Index { receiver, index } => {
                    format!("{}[{}]", expr_ref(*receiver), expr_ref(*index))
                }
                ExprKind::TypeAssert { value } => format!("assert_type {}", expr_ref(*value)),
                ExprKind::BinOp { op, lhs, rhs } => {
                    format!("{op:?} {}, {}", expr_ref(*lhs), expr_ref(*rhs))
                }
                ExprKind::UnaryOp { op, operand } => format!("{op:?} {}", expr_ref(*operand)),
                ExprKind::Conditional {
                    cond,
                    then_expr,
                    else_expr,
                } => {
                    format!(
                        "if {} then {} else {}",
                        expr_ref(*cond),
                        expr_ref(*then_expr),
                        expr_ref(*else_expr)
                    )
                }
                ExprKind::Block(block) => format!("block {block:?}"),
                ExprKind::Lambda { body, return_ty } => {
                    format!("lambda {body:?} -> {}", type_ref(krate, *return_ty))
                }
                ExprKind::Closure(closure) => {
                    format!(
                        "closure {:?} captures {} -> {}",
                        closure.body,
                        closure.captures.len(),
                        type_ref(krate, closure.return_ty)
                    )
                }
                ExprKind::ListLit(items) => collection_text("[", "]", items),
                ExprKind::SetLit(items) => collection_text("set{", "}", items),
                ExprKind::DictLit(items) => dict_lit_text(items),
                ExprKind::TupleLit(items) => collection_text("(", ")", items),
                ExprKind::Await(await_expr) => format!("await {}", expr_ref(*await_expr)),
                $(
                    ExprKind::$name $({ $($field),* })? => {
                        $($(let $field = render_op_field!(krate, $kind, $field);)*)?
                        $crate::op_hir_display!($display)
                    }
                )*
            }
        }
    };
}

crate::for_each_op!(define_expr_text);

/// Formats call-like expressions as text.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "Fallback arm preserves previous invalid-call behavior while keeping formatter split concise"
)]
fn call_like_expr_text(krate: &Crate, expr: &Expr) -> String {
    match &expr.kind {
        ExprKind::Call { callee, args } => {
            let arg_text = expr_list_text(args);
            format!("call {}({arg_text})", expr_ref(*callee))
        }
        ExprKind::Method {
            receiver,
            method,
            args,
        } => {
            let method_name = krate.symbols.get(*method).unwrap_or("<unknown>");
            let arg_text = expr_list_text(args);
            format!("{}.{}({arg_text})", expr_ref(*receiver), method_name)
        }
        ExprKind::New { class, args } => {
            let class_name = krate.symbols.get(*class).unwrap_or("<unknown>");
            let arg_text = expr_list_text(args);
            format!("new {class_name}({arg_text})")
        }
        ExprKind::BaseConstructorInit {
            base,
            receiver,
            args,
        } => {
            let base_name = krate.symbols.get(*base).unwrap_or("<unknown>");
            let arg_text = expr_list_text(args);
            format!("{base_name}.init({}; {arg_text})", expr_ref(*receiver))
        }
        _ => "invalid call".to_owned(),
    }
}

