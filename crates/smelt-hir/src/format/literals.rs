//! Literal and pattern formatting helpers.

use crate::body::{Body, Pattern};
use crate::expr::Literal;
use crate::ids::PatternId;

use super::local_ref;

/// Formats a pattern as text.
pub(super) fn pattern_text(body: &Body, pattern: PatternId) -> String {
    let Some(pattern_idx) = usize::try_from(pattern.0).ok() else {
        return "<invalid-pattern>".to_owned();
    };
    let Some(pattern_value) = body.patterns.get(pattern_idx) else {
        return "<missing-pattern>".to_owned();
    };
    match pattern_value {
        Pattern::Wildcard => "_".to_owned(),
        Pattern::Binding(local) => local_ref(*local),
        Pattern::Tuple(items) => {
            let tuple_items = items
                .iter()
                .map(|item| pattern_text(body, *item))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({tuple_items})")
        }
        Pattern::Literal(literal) => literal_text(literal),
    }
}

/// Formats a literal as text.
pub(super) fn literal_text(literal: &Literal) -> String {
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
        Literal::Symbol(value) => format!("symbol({value:?})"),
        Literal::None => "none".to_owned(),
    }
}
