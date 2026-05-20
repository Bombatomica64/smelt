//! Statement formatting helpers for control-flow nodes.

use crate::body::{Body, Stmt};
use crate::krate::Crate;

use super::{expr_ref, literal_text, local_ref, pattern_text, type_ref};

/// Formats a statement as text.
pub(super) fn stmt_text(krate: &Crate, body: &Body, stmt: &Stmt) -> String {
    match stmt {
        Stmt::Let { pat, ty, value } => {
            let value_suffix = value
                .map(|expr_id| format!(" = {}", expr_ref(expr_id)))
                .unwrap_or_default();
            format!(
                "let {}: {}{}",
                pattern_text(body, *pat),
                type_ref(krate, *ty),
                value_suffix
            )
        }
        Stmt::Assign { target, value } => {
            format!("{} = {}", expr_ref(*target), expr_ref(*value))
        }
        Stmt::Expr(expr) => expr_ref(*expr),
        Stmt::Return(Some(expr)) => format!("return {}", expr_ref(*expr)),
        Stmt::Return(None) => "return".to_owned(),
        Stmt::Break => "break".to_owned(),
        Stmt::Continue => "continue".to_owned(),
        Stmt::If { .. }
        | Stmt::While { .. }
        | Stmt::WhileUpdate { .. }
        | Stmt::For { .. }
        | Stmt::Match { .. }
        | Stmt::Throw(_)
        | Stmt::TryCatch { .. } => control_stmt_text(body, stmt),
    }
}

/// Formats control-flow statements as text.
fn control_stmt_text(body: &Body, stmt: &Stmt) -> String {
    match stmt {
        Stmt::If {
            cond,
            then_block,
            else_block,
        } => {
            let else_text = else_block
                .map(|block| format!(" else {block:?}"))
                .unwrap_or_default();
            format!("if {} then {:?}{}", expr_ref(*cond), then_block, else_text)
        }
        Stmt::While {
            cond,
            body: loop_body,
        } => format!("while {} {:?}", expr_ref(*cond), loop_body),
        Stmt::WhileUpdate {
            cond,
            body: loop_body,
            update_target,
            update_value,
        } => format!(
            "while {} {:?} update {} = {}",
            expr_ref(*cond),
            loop_body,
            expr_ref(*update_target),
            expr_ref(*update_value)
        ),
        Stmt::For {
            pat,
            iter,
            body: loop_body,
        } => {
            format!(
                "for {} in {} {:?}",
                pattern_text(body, *pat),
                expr_ref(*iter),
                loop_body
            )
        }
        Stmt::Match { .. } => match_stmt_text(stmt),
        Stmt::Throw(expr) => format!("throw {}", expr_ref(*expr)),
        Stmt::TryCatch { .. } => try_catch_stmt_text(stmt),
        Stmt::Let { .. }
        | Stmt::Assign { .. }
        | Stmt::Expr(_)
        | Stmt::Return(_)
        | Stmt::Break
        | Stmt::Continue => "invalid statement".to_owned(),
    }
}

/// Formats a match statement as text.
fn match_stmt_text(stmt: &Stmt) -> String {
    let Stmt::Match {
        scrutinee,
        arms,
        default,
    } = stmt
    else {
        return "invalid match".to_owned();
    };
    let arm_text = arms
        .iter()
        .map(|arm| format!("{} => {:?}", literal_text(&arm.label), arm.body))
        .collect::<Vec<_>>()
        .join(", ");
    let default_text = default
        .map(|block| format!(" default {block:?}"))
        .unwrap_or_default();
    format!(
        "match {} {{{}}}{}",
        expr_ref(*scrutinee),
        arm_text,
        default_text
    )
}

/// Formats a try/catch/finally statement as text.
fn try_catch_stmt_text(stmt: &Stmt) -> String {
    let Stmt::TryCatch {
        body,
        catch_binding,
        catch_body,
        finally_body,
    } = stmt
    else {
        return "invalid try/catch".to_owned();
    };
    let catch = catch_body
        .map(|block| {
            let binding = catch_binding
                .map(|local| format!(" {}", local_ref(local)))
                .unwrap_or_default();
            format!(" catch{binding} {block:?}")
        })
        .unwrap_or_default();
    let finally = finally_body
        .map(|block| format!(" finally {block:?}"))
        .unwrap_or_default();
    format!("try {body:?}{catch}{finally}")
}
