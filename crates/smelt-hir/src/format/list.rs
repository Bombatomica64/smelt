//! List-style formatting helpers.

use crate::ids::ExprId;

use super::expr_ref;

/// Formats a comma-separated expression ID list.
pub(super) fn expr_list_text(items: &[ExprId]) -> String {
    items
        .iter()
        .map(|arg| expr_ref(*arg))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Formats a collection literal as text.
pub(super) fn collection_text(open: &str, close: &str, items: &[ExprId]) -> String {
    let item_text = items
        .iter()
        .map(|item| expr_ref(*item))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{open}{item_text}{close}")
}
