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

use crate::{FileTypes, SpanKey};
use crate::convert::Converter;


/// Span-keyed types of one file: value positions and type annotations.
pub(crate) struct CollectedTypes {
    /// `getTypeAtLocation` of every value-position node.
    pub(crate) values: FileTypes,
    /// `getTypeFromTypeNode` of every type annotation node.
    pub(crate) type_nodes: FileTypes,
}

/// Collects the type of every value-position node and type node in `file`.
pub(crate) fn collect_file_types(converter: &mut Converter<'_>, file: P<SourceFile>) -> CollectedTypes {
    let mut collected = CollectedTypes { values: HashMap::new(), type_nodes: HashMap::new() };
    let text = file.text();
    let mut work: Vec<P<Node>> = vec![file.as_node()];
    while let Some(node) = work.pop() {
        if let Some(key) = span_key(text, node) {
            if is_value_position(node) && !collected.values.contains_key(&key) {
                let ty = converter.checker().get_type_at_location(node);
                collected.values.insert(key, converter.convert(ty));
            } else if is_annotation_type_node(node) && !collected.type_nodes.contains_key(&key) {
                let ty = converter.checker().get_type_from_type_node_exported(node);
                collected.type_nodes.insert(key, converter.convert(ty));
            }
        }
        push_children_in_order(node, &mut work);
    }
    collected
}

/// Whether `node` is a type annotation whose evaluated type is meaningful on its own.
///
/// Heritage clauses (`extends Base<T>`) are expressions with type arguments,
/// `infer X` only binds inside a conditional, and a type predicate
/// (`x is T`) is a boolean-returning signature form; none is a standalone type.
fn is_annotation_type_node(node: P<Node>) -> bool {
    ast::is_type_node(node)
        && !matches!(node.kind(), Kind::ExpressionWithTypeArguments)
        && !ast::is_infer_type_node(node)
        && !ast::is_type_predicate_node(node)
}

/// Whether `node` denotes a value whose type the frontend may ask about.
///
/// Two syntactic positions the `.types` baseline prints are left out because
/// the checker answers a different question there than "the type of this
/// value": a spread (`...xs` in a call or array, `...o` in an object literal)
/// reports the *iterated element* type, and the key of a non-shorthand
/// object-literal property (`{ ada: 10 }`) reports the *property's* type.
/// Shorthand properties (`{ id }`) stay: their name is also the value read.
fn is_value_position(node: P<Node>) -> bool {
    (ast::is_expression_node(node) || node.kind() == Kind::Identifier || ast::is_declaration_name(node))
        && !ast::is_part_of_type_node(node)
        && !ast::is_omitted_expression(node)
        && !matches!(node.kind(), Kind::SpreadElement | Kind::SpreadAssignment)
        && !is_object_literal_key(node)
}

/// Whether `node` is the key of a non-shorthand object-literal property.
fn is_object_literal_key(node: P<Node>) -> bool {
    node.parent()
        .is_some_and(|parent| ast::is_property_assignment(parent) && parent.name() == Some(node))
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
