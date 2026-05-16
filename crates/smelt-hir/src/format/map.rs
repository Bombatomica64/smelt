//! Map/dictionary formatting helpers.

use crate::ids::ExprId;

use super::expr_ref;

/// Formats a dictionary literal as text.
pub(super) fn dict_lit_text(items: &[(ExprId, ExprId)]) -> String {
    let item_text = items
        .iter()
        .map(|(key, value)| format!("{}: {}", expr_ref(*key), expr_ref(*value)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{{item_text}}}")
}
