//! Span-keyed type collection over the analysed source files.
//!
//! This is the walk behind TypeScript's `.types` conformance baselines
//! (`tsrs_testrunner::type_symbol_baseline`): every expression, identifier and
//! declaration name that is not itself part of a type annotation is asked for
//! `getTypeAtLocation`. The result is keyed by the node's token span, which is
//! `skip_trivia(pos)..end`: the same byte range oxc reports for the node, so
//! the frontend can look a type up with the span it already holds.
//!
//! When several nodes share one span (an identifier that is also a whole
//! expression statement's expression, a parenthesised node), the outermost
//! node wins because the walk is pre-order and keeps the first entry.

#![expect(
    clippy::redundant_pub_crate,
    reason = "the span walk and its key types are used by the crate root"
)]

use std::collections::HashMap;

use tsrs_ast::{self as ast, Kind, Node, SourceFile};
use tsrs_core::P;

use crate::convert::Converter;
use crate::model::TsTypeId;

/// Byte span `(start, end)` of a node's tokens.
pub(crate) type SpanKey = (u32, u32);

/// Type of each value-position node in one file, keyed by token span.
pub(crate) type FileTypes = HashMap<SpanKey, TsTypeId>;

/// Collects the type of every value-position node in `file`.
pub(crate) fn collect_file_types(
    converter: &mut Converter<'_>,
    file: P<SourceFile>,
) -> FileTypes {
    let mut types = HashMap::new();
    let text = file.text();
    let mut work: Vec<P<Node>> = vec![file.as_node()];
    while let Some(node) = work.pop() {
        if let Some(key) = span_key(text, node)
            && is_value_position(node)
            && !types.contains_key(&key)
        {
            let ty = converter.checker().get_type_at_location(node);
            types.insert(key, converter.convert(ty));
        }
        push_children_in_order(node, &mut work);
    }
    types
}

/// Whether `node` denotes a value whose type the frontend may ask about.
fn is_value_position(node: P<Node>) -> bool {
    (ast::is_expression_node(node) || node.kind() == Kind::Identifier || ast::is_declaration_name(node))
        && !ast::is_part_of_type_node(node)
        && !ast::is_omitted_expression(node)
}

/// The node's token span, or `None` for synthesized nodes without source positions.
fn span_key(text: &str, node: P<Node>) -> Option<SpanKey> {
    let pos = node.pos().source_offset()?;
    let end = node.end().source_offset()?;
    Some((tsrs_scanner::skip_trivia(text, pos), end))
}

/// Pushes the children of `node` so they pop in source order (keeping the walk pre-order).
fn push_children_in_order(node: P<Node>, work: &mut Vec<P<Node>>) {
    let mut children = Vec::new();
    node.for_each_child(&mut |child| {
        children.push(child);
        false
    });
    children.reverse();
    work.append(&mut children);
}
