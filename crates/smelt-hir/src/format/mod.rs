//! Pretty-printing utilities for HIR.

mod call;
mod control_flow;
mod list;
mod literals;
mod map;
mod types;

use call::expr_text;
use control_flow::stmt_text;
use crate::body::Body;
use crate::ids::{ExprId, LocalId, ModuleId, id_index};
use crate::krate::Crate;
use list::{collection_text, expr_list_text};
use literals::{literal_text, pattern_text};
use map::dict_lit_text;
use types::{expr_ref, item_ref, item_text, local_ref, optional_expr_ref, type_ref, type_text};
use std::fmt::Write as _;

/// Append formatted text to the output buffer.
fn push_fmt(out: &mut String, args: std::fmt::Arguments<'_>) {
    let _ignored = out.write_fmt(args);
}

/// Formats the HIR of the given modules in a compact, human-readable form.
#[must_use]
pub fn format_compact(krate: &Crate, modules: &[(String, ModuleId)]) -> String {
    let mut out = String::new();

    for (path, module_id) in modules {
        let Some(module_idx) = usize::try_from(module_id.0).ok() else {
            push_fmt(
                &mut out,
                format_args!("module {path} (ModuleId({}))\n", module_id.0),
            );
            out.push_str("  <invalid module>\n\n");
            continue;
        };
        let Some(module) = krate.modules.get(module_idx) else {
            push_fmt(
                &mut out,
                format_args!("module {path} (ModuleId({}))\n", module_id.0),
            );
            out.push_str("  <missing module>\n\n");
            continue;
        };
        push_fmt(
            &mut out,
            format_args!("module {path} (ModuleId({}))\n", module_id.0),
        );

        let Some(body_id) = module.body else {
            out.push_str("  <no body>\n\n");
            continue;
        };

        push_fmt(&mut out, format_args!("  body BodyId({})\n", body_id.0));

        if !module.items.is_empty() {
            out.push_str("  items\n");
            for item in &module.items {
                out.push_str("    ");
                out.push_str(&item_text(krate, *item));
                out.push('\n');
            }
        }

        let Some(body_idx) = usize::try_from(body_id.0).ok() else {
            out.push_str("  <invalid body>\n\n");
            continue;
        };
        let Some(body) = krate.bodies.get(body_idx) else {
            out.push_str("  <missing body>\n\n");
            continue;
        };
        format_body_sections(krate, body, &mut out);
        out.push('\n');
    }

    out.push_str("interned types\n");
    for (idx, ty) in krate.types.all().iter().enumerate() {
        push_fmt(
            &mut out,
            format_args!("  t{} = {}\n", idx, type_text(krate, ty)),
        );
    }

    out
}

/// Appends the locals/exprs/stmts sections of a body to the output string.
fn format_body_sections(krate: &Crate, body: &Body, out: &mut String) {
    if !body.locals.is_empty() {
        out.push_str("  locals\n");
        for (idx, local) in body.locals.iter().enumerate() {
            let local_id = LocalId(id_index(idx));
            let mutability = if local.mutable { "let" } else { "const" };
            let name = local
                .name
                .and_then(|sym| krate.names.get(sym).or_else(|| krate.symbols.get(sym)))
                .unwrap_or("_");
            push_fmt(
                out,
                format_args!(
                    "    {} {} {}: {}\n",
                    local_ref(local_id),
                    mutability,
                    name,
                    type_ref(krate, local.ty)
                ),
            );
        }
    }

    if !body.exprs.is_empty() {
        out.push_str("  exprs\n");
        for (idx, expr) in body.exprs.iter().enumerate() {
            let expr_id = ExprId(id_index(idx));
            push_fmt(
                out,
                format_args!(
                    "    {}: {} = {}\n",
                    expr_ref(expr_id),
                    type_ref(krate, expr.ty),
                    expr_text(krate, expr)
                ),
            );
        }
    }

    if !body.stmts.is_empty() {
        out.push_str("  stmts\n");
        for (idx, stmt) in body.stmts.iter().enumerate() {
            push_fmt(
                out,
                format_args!("    s{}: {}\n", idx, stmt_text(krate, body, stmt)),
            );
        }
    }

    if let Some(machine) = &body.async_state_machine {
        out.push_str("  async state machine\n");
        let states = machine
            .states
            .iter()
            .map(|state| format!("state{}", state.id.0))
            .collect::<Vec<_>>()
            .join(", ");
        push_fmt(out, format_args!("    states [{states}]\n"));
        for suspension in &machine.suspensions {
            push_fmt(
                out,
                format_args!(
                    "    suspend {} on {} -> state{}\n",
                    expr_ref(suspension.await_expr),
                    expr_ref(suspension.future),
                    suspension.resume_state.0
                ),
            );
        }
    }
}
