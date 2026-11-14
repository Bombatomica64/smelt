//! Which unannotated object-literal bindings may lower to a record struct.
//!
//! `const point = { x: 1, label: "a" }` has no annotation, so its values have
//! no common type and the literal used to lower to a map of erased values
//! (`Dict<String, SmeltUnknown>`, a `dynamic-operator` erasure). The checker
//! knows the binding is `{ x: number; label: string }`, and
//! `checker_literal_binding_type` can lower it as the struct the same type
//! literal written in source gives. This module decides, from the uses of the
//! binding alone, whether that is safe.
//!
//! # Why a struct is not always safe
//!
//! The two representations agree while a value stays at its own type, and
//! differ once it is viewed at another one:
//!
//! - the map IS the JavaScript object: one identity, an insertion-ordered
//!   open key set, passed to every callee as itself;
//! - the struct keeps its identity too (`===` between aliases, and into an
//!   erased slot and back through a cast), and its key order is its
//!   declaration order, which is the literal's. But handing it to a slot of
//!   another type CONVERTS it, and the conversion is a copy:
//!   - a write made through the other view does not reach the struct: a
//!     callee taking `Record<string, unknown>`, `any`, a type parameter or
//!     another record type and assigning a property, or a write through an
//!     element of a `Record<..>[]` the struct was stored in
//!     (fixture `passed-or-stored-stays-a-map`: the caller reads the stale
//!     value);
//!   - a value a callee returns as its `T` is rebuilt as the struct, in the
//!     struct's key order, whatever order the callee built it in (es-toolkit's
//!     `sortKeys`: `Object.keys(sortKeys(obj))` came back unsorted);
//!   - the test framework's identity matcher on a value that went through a
//!     callee and back (es-toolkit's `cloneDeepWith` spec, `toBe(obj)`).
//!
//! So the rule is about where the value goes, not about what it is. Whether a
//! given callee writes, rebuilds or compares is a property of its body, which
//! this analysis does not read: every call that is not a known observer
//! disqualifies.
//!
//! Two families are disqualifiers as a PRECAUTION, not because a failing
//! program was found: identity observations ([`Disqualifier::Identity`]) and
//! key-set or key-order operations ([`Disqualifier::Keys`]). On the programs
//! probed (the fixtures `identity-stays-a-map` and `keys-stay-a-map` lowered
//! with the rule switched off) the struct printed what Node prints. They stay disqualifiers because only those
//! instances were checked, and they cost almost nothing: see
//! `blocker-logs/typedness-literal-records.md`.
//!
//! # The rule
//!
//! A binding `x` declared without annotation from an object literal, or from
//! an array literal holding object literals, takes the record struct only when
//! BOTH hold; otherwise it keeps the map lowering it has always had, with the
//! erasure reason that lowering already records.
//!
//! 1. **The literal is plain data spelled in place** ([`literal_shape`]):
//!    every property is `key: value` with an identifier or string key that
//!    JavaScript enumerates in source order, and every value is a primitive
//!    literal or another such literal. No spread, computed key, method,
//!    accessor, integer-like key (those enumerate first, in numeric order) or
//!    `__proto__`, and no value produced elsewhere (an identifier, a call, a
//!    function): such a value has its own representation and identity, which
//!    a typed field would convert.
//! 2. **Every use keeps the value typed** ([`Analysis::classify`]). A *flow*
//!    is a reference to `x`, a read of a record- or array-valued part of a
//!    flow (`x.inner`, `xs[i]`), or a reference to a `const`/`let` alias or
//!    `for..of` element bound to a flow. Each flow must be one of:
//!    - a read or write of a declared property with a static key, or of an
//!      array element or `length` (a write only of a primitive property);
//!    - the initializer of an unannotated, never reassigned alias, or the
//!      iterable of a `for..of` over an array flow;
//!    - an argument the callee only observes by value, at once: `console.*`,
//!      `JSON.stringify`, the subject or expected value of the test
//!      framework's deep-equality matchers (`toEqual`, `toStrictEqual`);
//!    - a comparison with `null`/`undefined`, `typeof`, or a statement of its
//!      own.
//!
//! Anything else disqualifies the binding, classified for reporting as a
//! [`Disqualifier`]: an identity observation, a key-set or key-order
//! operation, an argument to any other call (which covers every parameter
//! typed `unknown`, `any`, `object`, `Record<..>` or a type parameter: the
//! callee may compare, enumerate, mutate or return it, and its body is not
//! inspected), an escape (returned, stored, assigned, exported, reassigned),
//! or a use this module does not model.
//!
//! # Requirements
//!
//! - **Behavior**: a program in which a literal takes the struct prints what
//!   it prints with the map lowering, which is what Node prints. Identity
//!   (`===` between aliases), key order (`JSON.stringify`, `console.log`)
//!   and the visibility of writes through
//!   aliases and closures are preserved, because every use the rule admits is
//!   one both representations answer alike and no copy is ever made.
//! - **Type preservation**: a literal that takes the struct has every
//!   property at the type the checker inferred, with no erased part; one that
//!   keeps the map is typed exactly as before this rule existed.
//! - **Conservative**: the analysis is syntactic and per module. A use it
//!   cannot prove safe keeps the map, so it can lose typing and never
//!   behavior.
//!
//! Checked by `examples/typescript/end-to-end/196_object_literal_takes_its_checker_record`
//! (the literal that takes the struct: goldens of the HIR, MIR and Rust, no
//! erased value), by `smelt-transpiler/tests/literal_record_cli_tests.rs` (one
//! fixture per disqualifier family: the literal stays a map and the program
//! prints what Node prints) and by the tests below.
//!
//! # Measuring
//!
//! With `SMELT_LITERAL_RECORD_TRACE` set, `checker_literal_binding_type`
//! prints one line per candidate binding to stderr: its decision, and whether
//! the checker's type was representable. That is how the cost of each
//! disqualifier is counted (`blocker-logs/typedness-literal-records.md`).

use std::collections::{HashMap, HashSet};

use oxc::ast::AstKind;
use oxc::ast::ast::{
    ArrayExpressionElement, BinaryOperator, BindingPattern, CallExpression, Expression,
    ForStatementLeft, ObjectPropertyKind, Program, PropertyKey, PropertyKind, UnaryOperator,
    VariableDeclarationKind,
};
use oxc::semantic::{AstNodes, NodeId, Scoping, SemanticBuilder, SymbolId};
use oxc::span::{GetSpan, Span};

/// Why a literal binding keeps its map lowering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(in crate::lowering) enum Disqualifier {
    /// The literal is not plain data spelled in place: a spread, a computed
    /// or integer-like key, a method or accessor, or a property value that is
    /// not itself a literal.
    Shape,
    /// The value's identity is observed: `===`/`!==` against another value,
    /// `Object.is`, the `toBe` matcher, membership in a `Set`/`Map` key or an
    /// array (`has`, `add`, `get`, `set`, `includes`, `indexOf`).
    Identity,
    /// The key set or key order is observed beyond what a struct fixes, or
    /// changed: a computed key, `delete`, `in`, `for..in`, a property the
    /// literal does not declare, a spread, `Object.keys`/`values`/`entries`/
    /// `assign`/...
    Keys,
    /// A record- or array-valued property is replaced (`x.inner = other`),
    /// which would convert the assigned value.
    PartWrite,
    /// The value is an argument of a call or constructor that is not a known
    /// by-value observer, whatever the parameter's type.
    Call,
    /// The value leaves the binding: returned, thrown, stored in a container
    /// or another variable, exported, or the binding is reassigned.
    Escape,
    /// A use this module does not model: a method call on the value, a type
    /// assertion, an operator.
    Other,
}

impl Disqualifier {
    /// The stable name printed by the trace and used in reports.
    pub(in crate::lowering) const fn name(self) -> &'static str {
        match self {
            Self::Shape => "shape",
            Self::Identity => "identity",
            Self::Keys => "keys",
            Self::PartWrite => "part-write",
            Self::Call => "call",
            Self::Escape => "escape",
            Self::Other => "other",
        }
    }
}

/// The decision for one candidate binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::lowering) enum LiteralDecision {
    /// Every use keeps the value typed: lower the checker's record type.
    Struct,
    /// Some use needs the map; the first one found, in source order of the
    /// binding's references.
    Map(Disqualifier),
}

/// The decision for every candidate binding of one module, keyed by the start
/// offset of the binding identifier.
#[derive(Debug, Default)]
pub(in crate::lowering) struct LiteralRecordUses {
    /// Bindings declared from a literal that holds an object literal.
    decisions: HashMap<u32, LiteralDecision>,
}

impl LiteralRecordUses {
    /// Decide every candidate binding of `program`.
    pub(in crate::lowering) fn collect(program: &Program<'_>) -> Self {
        let semantic = SemanticBuilder::new()
            .with_build_nodes(true)
            .build(program)
            .semantic;
        let analysis = Analysis {
            nodes: semantic.nodes(),
            scoping: semantic.scoping(),
        };
        let mut decisions = HashMap::new();
        for node in analysis.nodes.iter() {
            let AstKind::VariableDeclarator(declarator) = node.kind() else {
                continue;
            };
            let (BindingPattern::BindingIdentifier(binding), Some(init)) =
                (&declarator.id, &declarator.init)
            else {
                continue;
            };
            if declarator.type_annotation.is_some() || !holds_object_literal(init) {
                continue;
            }
            let decision = match analysis.decide(node.id(), binding.symbol_id.get(), init) {
                Ok(()) => LiteralDecision::Struct,
                Err(disqualifier) => LiteralDecision::Map(disqualifier),
            };
            decisions.insert(binding.span.start, decision);
        }
        Self { decisions }
    }

    /// The decision for the binding identifier starting at `binding_start`;
    /// `None` when it is not a candidate (annotated, or its initializer is not
    /// a literal holding an object literal), which keeps its lowering as is.
    pub(in crate::lowering) fn decision(&self, binding_start: u32) -> Option<LiteralDecision> {
        self.decisions.get(&binding_start).copied()
    }
}

/// Whether `expression` is an object literal, or an array literal holding one
/// at any depth. Only these can become records; an array of primitives is
/// outside this rule.
fn holds_object_literal(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::ObjectExpression(_) => true,
        Expression::ArrayExpression(array) => array.elements.iter().any(|element| {
            element
                .as_expression()
                .is_some_and(|element| holds_object_literal(element))
        }),
        _ => false,
    }
}

/// The part structure of a plain-data literal: what each read of it yields.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Shape {
    /// A value with no identity or parts: a number, string, boolean, `null`.
    Primitive,
    /// An object literal: its properties in source order.
    Record(Vec<(String, Self)>),
    /// An array literal: the shape every element shares.
    Array(Box<Self>),
}

impl Shape {
    /// The shape of the declared property `name` of a record.
    fn property(&self, name: &str) -> Option<&Self> {
        match self {
            Self::Record(properties) => properties
                .iter()
                .find(|(property, _)| property == name)
                .map(|(_, shape)| shape),
            Self::Primitive | Self::Array(_) => None,
        }
    }
}

/// The shape of `expression` when it is plain data spelled in place (rule 1 of
/// the module docs), else [`Disqualifier::Shape`].
fn literal_shape(expression: &Expression<'_>) -> Result<Shape, Disqualifier> {
    match expression {
        Expression::ObjectExpression(object) => {
            let mut properties: Vec<(String, Shape)> = Vec::new();
            for property in &object.properties {
                let ObjectPropertyKind::ObjectProperty(property) = property else {
                    return Err(Disqualifier::Shape);
                };
                if property.kind != PropertyKind::Init || property.method || property.computed {
                    return Err(Disqualifier::Shape);
                }
                let name = match &property.key {
                    PropertyKey::StaticIdentifier(name) => name.name.as_str(),
                    PropertyKey::StringLiteral(name) => {
                        name.value.as_str().ok_or(Disqualifier::Shape)?
                    }
                    _ => return Err(Disqualifier::Shape),
                };
                if name == "__proto__"
                    || is_integer_key(name)
                    || properties.iter().any(|(seen, _)| seen == name)
                {
                    return Err(Disqualifier::Shape);
                }
                properties.push((name.to_owned(), literal_shape(&property.value)?));
            }
            Ok(Shape::Record(properties))
        }
        Expression::ArrayExpression(array) => {
            let mut shared: Option<Shape> = None;
            for element in &array.elements {
                let element = match element {
                    ArrayExpressionElement::SpreadElement(_)
                    | ArrayExpressionElement::Elision(_) => return Err(Disqualifier::Shape),
                    other => other.as_expression().ok_or(Disqualifier::Shape)?,
                };
                let shape = literal_shape(element)?;
                match &shared {
                    None => shared = Some(shape),
                    // Elements of different part structure have no one shape
                    // an indexed read could be judged by.
                    Some(seen) if *seen != shape => return Err(Disqualifier::Shape),
                    Some(_) => {}
                }
            }
            // An empty array has no element to judge: its uses are judged as
            // if every element were a primitive, which no read contradicts.
            Ok(Shape::Array(Box::new(shared.unwrap_or(Shape::Primitive))))
        }
        Expression::ParenthesizedExpression(inner) => literal_shape(&inner.expression),
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_) => Ok(Shape::Primitive),
        Expression::TemplateLiteral(_) => Ok(Shape::Primitive),
        Expression::UnaryExpression(unary)
            if matches!(
                unary.operator,
                UnaryOperator::UnaryNegation
                    | UnaryOperator::UnaryPlus
                    | UnaryOperator::LogicalNot
                    | UnaryOperator::BitwiseNot
            ) =>
        {
            match literal_shape(&unary.argument)? {
                Shape::Primitive => Ok(Shape::Primitive),
                Shape::Record(_) | Shape::Array(_) => Err(Disqualifier::Shape),
            }
        }
        _ => Err(Disqualifier::Shape),
    }
}

/// Whether `name` is a canonical array index (`"0"`, `"12"`), which JavaScript
/// enumerates before every string key, in numeric order, whatever the source
/// order: a struct's declaration order could not reproduce it.
fn is_integer_key(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| byte.is_ascii_digit())
        && (name == "0" || !name.starts_with('0'))
}

/// The test framework's deep-equality matchers: they compare by value, at
/// once, and ignore key order and identity (`testing/matchers.rs`).
const BY_VALUE_MATCHERS: &[&str] = &["toEqual", "toStrictEqual"];

/// Methods whose argument is compared or stored by identity.
const IDENTITY_METHODS: &[&str] = &[
    "toBe",
    "has",
    "add",
    "get",
    "set",
    "delete",
    "includes",
    "indexOf",
    "lastIndexOf",
];

/// One module's semantic model, and the classification of flows over it.
struct Analysis<'model, 'ast> {
    /// Every AST node, with its parent.
    nodes: &'model AstNodes<'ast>,
    /// Symbols and their resolved references.
    scoping: &'model Scoping,
}

impl<'ast> Analysis<'_, 'ast> {
    /// Decide the binding declared by the declarator `declarator_id` from the
    /// literal `init`.
    fn decide(
        &self,
        declarator_id: NodeId,
        symbol: Option<SymbolId>,
        init: &Expression<'ast>,
    ) -> Result<(), Disqualifier> {
        let shape = literal_shape(init)?;
        // No symbol means the semantic model did not bind the name: nothing
        // about its uses is known.
        let symbol = symbol.ok_or(Disqualifier::Other)?;
        if self.is_exported(declarator_id) {
            // Another module's uses are not visible from here.
            return Err(Disqualifier::Escape);
        }
        let mut visiting = HashSet::new();
        self.symbol_flows(symbol, &shape, &mut visiting)
    }

    /// Whether the declarator `declarator_id` sits in an `export` declaration.
    fn is_exported(&self, declarator_id: NodeId) -> bool {
        let declaration = self.nodes.parent_id(declarator_id);
        matches!(
            self.nodes.parent_kind(declaration),
            AstKind::ExportDeclaration(_)
                | AstKind::ExportNamedDeclaration(_)
                | AstKind::ExportDefaultDeclaration(_)
        )
    }

    /// Classify every reference to `symbol`, a binding whose value is a flow
    /// of `shape`.
    fn symbol_flows(
        &self,
        symbol: SymbolId,
        shape: &Shape,
        visiting: &mut HashSet<SymbolId>,
    ) -> Result<(), Disqualifier> {
        if !visiting.insert(symbol) {
            return Ok(());
        }
        for reference in self.scoping.get_resolved_references(symbol) {
            if reference.is_write() {
                // Reassigned: the binding would hold a converted value.
                return Err(Disqualifier::Escape);
            }
            if reference.is_type() || reference.flags().is_value_as_type() {
                // `typeof x` in a type position names the type, not the value.
                continue;
            }
            self.classify(reference.node_id(), shape, visiting)?;
        }
        Ok(())
    }

    /// The node whose parent decides what happens to the flow at `node`:
    /// `node` itself, or the outermost parenthesis, non-null assertion or
    /// optional chain that only wraps it.
    fn unwrapped(&self, mut node: NodeId) -> NodeId {
        loop {
            let parent = self.nodes.parent_id(node);
            if parent == node {
                return node;
            }
            match self.nodes.kind(parent) {
                AstKind::ParenthesizedExpression(_)
                | AstKind::TSNonNullExpression(_)
                | AstKind::ChainExpression(_) => node = parent,
                _ => return node,
            }
        }
    }

    /// Classify the use of the flow at `node`, a value of `shape` (rule 2 of
    /// the module docs).
    fn classify(
        &self,
        node: NodeId,
        shape: &Shape,
        visiting: &mut HashSet<SymbolId>,
    ) -> Result<(), Disqualifier> {
        let node = self.unwrapped(node);
        let span = self.nodes.kind(node).span();
        let parent = self.nodes.parent_id(node);
        match self.nodes.kind(parent) {
            AstKind::StaticMemberExpression(member) if member.object.span() == span => {
                self.member(parent, shape, Some(member.property.name.as_str()), visiting)
            }
            AstKind::ComputedMemberExpression(member) if member.object.span() == span => {
                let key = match &member.expression {
                    Expression::StringLiteral(key) => key.value.as_str(),
                    _ => None,
                };
                self.member(parent, shape, key, visiting)
            }
            AstKind::VariableDeclarator(declarator) => {
                // An alias shares the handle; an annotation or a pattern
                // would convert or take the value apart.
                let BindingPattern::BindingIdentifier(alias) = &declarator.id else {
                    return Err(Disqualifier::Other);
                };
                if declarator.type_annotation.is_some() {
                    return Err(Disqualifier::Other);
                }
                let alias = alias.symbol_id.get().ok_or(Disqualifier::Other)?;
                self.symbol_flows(alias, shape, visiting)
            }
            AstKind::ForOfStatement(statement) if statement.right.span() == span => {
                let Shape::Array(element) = shape else {
                    return Err(Disqualifier::Other);
                };
                self.loop_binding(&statement.left, element, visiting)
            }
            // Enumerating keys goes through the map view of the value.
            AstKind::ForInStatement(_) => Err(Disqualifier::Keys),
            AstKind::CallExpression(call) => {
                if call.callee.span() == span {
                    return Err(Disqualifier::Other);
                }
                self.argument(parent, call, span)
            }
            AstKind::NewExpression(_) => Err(Disqualifier::Call),
            AstKind::BinaryExpression(binary) => {
                let other = if binary.left.span() == span {
                    &binary.right
                } else {
                    &binary.left
                };
                match binary.operator {
                    BinaryOperator::StrictEquality
                    | BinaryOperator::StrictInequality
                    | BinaryOperator::Equality
                    | BinaryOperator::Inequality => {
                        if is_nullish_literal(other) {
                            Ok(())
                        } else {
                            Err(Disqualifier::Identity)
                        }
                    }
                    BinaryOperator::In => Err(Disqualifier::Keys),
                    _ => Err(Disqualifier::Other),
                }
            }
            AstKind::UnaryExpression(unary) => match unary.operator {
                UnaryOperator::Typeof => Ok(()),
                UnaryOperator::Delete => Err(Disqualifier::Keys),
                _ => Err(Disqualifier::Other),
            },
            AstKind::ExpressionStatement(_) => Ok(()),
            // `() => x` returns the value.
            AstKind::ArrowFunctionExpression(_) | AstKind::FunctionBody(_) => {
                Err(Disqualifier::Escape)
            }
            AstKind::SpreadElement(_) => Err(Disqualifier::Keys),
            AstKind::ReturnStatement(_)
            | AstKind::ThrowStatement(_)
            | AstKind::YieldExpression(_)
            | AstKind::AwaitExpression(_)
            | AstKind::ObjectProperty(_)
            | AstKind::ArrayExpression(_)
            | AstKind::AssignmentExpression(_)
            | AstKind::ExportSpecifier(_)
            | AstKind::ExportDefaultDeclaration(_) => Err(Disqualifier::Escape),
            _ => Err(Disqualifier::Other),
        }
    }

    /// Classify the member read or write `member` on a flow of `shape`; `key`
    /// is its static key, `None` for a computed one.
    fn member(
        &self,
        member: NodeId,
        shape: &Shape,
        key: Option<&str>,
        visiting: &mut HashSet<SymbolId>,
    ) -> Result<(), Disqualifier> {
        let part = match shape {
            Shape::Record(_) => {
                // A computed key reads or writes a property chosen at run
                // time; an undeclared one (`x.toString`, `x.added`) is not a
                // field of the struct.
                let key = key.ok_or(Disqualifier::Keys)?;
                shape.property(key).ok_or(Disqualifier::Keys)?
            }
            Shape::Array(element) => match key {
                Some("length") => &Shape::Primitive,
                // Any other name is an array method.
                Some(_) => return Err(Disqualifier::Other),
                None => element.as_ref(),
            },
            Shape::Primitive => return Ok(()),
        };
        let member = self.unwrapped(member);
        let span = self.nodes.kind(member).span();
        let parent = self.nodes.parent_kind(member);
        if let AstKind::UnaryExpression(unary) = parent
            && unary.operator == UnaryOperator::Delete
        {
            return Err(Disqualifier::Keys);
        }
        let written = match parent {
            AstKind::AssignmentExpression(assignment) => assignment.left.span() == span,
            AstKind::UpdateExpression(_) => true,
            AstKind::ArrayAssignmentTarget(_)
            | AstKind::ObjectAssignmentTarget(_)
            | AstKind::AssignmentTargetRest(_)
            | AstKind::AssignmentTargetWithDefault(_)
            | AstKind::AssignmentTargetPropertyIdentifier(_)
            | AstKind::AssignmentTargetPropertyProperty(_) => true,
            _ => false,
        };
        match part {
            // A primitive has no identity and no parts: any use of the read
            // value is its own, and a write stores a primitive in the field.
            // `length` is the exception: writing it truncates the array.
            Shape::Primitive if written && matches!(shape, Shape::Array(_)) && key.is_some() => {
                Err(Disqualifier::Other)
            }
            Shape::Primitive => Ok(()),
            Shape::Record(_) | Shape::Array(_) if written => Err(Disqualifier::PartWrite),
            // A part is a flow of its own shape.
            Shape::Record(_) | Shape::Array(_) => self.classify(member, part, visiting),
        }
    }

    /// Classify the binding a `for..of` over an array flow declares: each
    /// iteration binds an element, a flow of `element`.
    fn loop_binding(
        &self,
        left: &ForStatementLeft<'ast>,
        element: &Shape,
        visiting: &mut HashSet<SymbolId>,
    ) -> Result<(), Disqualifier> {
        let ForStatementLeft::VariableDeclaration(declaration) = left else {
            return Err(Disqualifier::Other);
        };
        let [declarator] = declaration.declarations.as_slice() else {
            return Err(Disqualifier::Other);
        };
        let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
            return Err(Disqualifier::Other);
        };
        if declarator.type_annotation.is_some() || declaration.kind == VariableDeclarationKind::Var
        {
            return Err(Disqualifier::Other);
        }
        if *element == Shape::Primitive {
            // A primitive element has no identity or parts to follow.
            return Ok(());
        }
        let symbol = binding.symbol_id.get().ok_or(Disqualifier::Other)?;
        self.symbol_flows(symbol, element, visiting)
    }

    /// Classify a flow at `span` passed as an argument of `call`.
    fn argument(
        &self,
        call_id: NodeId,
        call: &CallExpression<'ast>,
        span: Span,
    ) -> Result<(), Disqualifier> {
        let position = call
            .arguments
            .iter()
            .position(|argument| argument.span() == span);
        let callee = call.callee.without_parentheses();
        if self.is_expect(callee) && position == Some(0) {
            return self.expectation_subject(call_id);
        }
        let Expression::StaticMemberExpression(member) = callee else {
            return Err(Disqualifier::Call);
        };
        let method = member.property.name.as_str();
        if let Some(namespace) = self.global_name(&member.object) {
            match (namespace, method) {
                // Prints or serializes the value in its own key order.
                ("console", _) => return Ok(()),
                // A replacer function would be handed the value's parts.
                ("JSON", "stringify")
                    if position == Some(0)
                        && call.arguments.iter().skip(1).all(|argument| {
                            argument.as_expression().is_some_and(|argument| {
                                matches!(
                                    argument.without_parentheses(),
                                    Expression::NullLiteral(_)
                                        | Expression::NumericLiteral(_)
                                        | Expression::StringLiteral(_)
                                )
                            })
                        }) =>
                {
                    return Ok(());
                }
                ("Object", "is") => return Err(Disqualifier::Identity),
                ("Object", _) => return Err(Disqualifier::Keys),
                _ => {}
            }
        }
        if self.is_expectation(&member.object) && BY_VALUE_MATCHERS.contains(&method) {
            return Ok(());
        }
        if IDENTITY_METHODS.contains(&method) {
            return Err(Disqualifier::Identity);
        }
        Err(Disqualifier::Call)
    }

    /// Classify `expect(flow)` by the matcher applied to it:
    /// `expect(x).toEqual(..)`, `expect(x).not.toStrictEqual(..)`.
    fn expectation_subject(&self, expect_call: NodeId) -> Result<(), Disqualifier> {
        let mut node = self.unwrapped(expect_call);
        loop {
            let parent = self.nodes.parent_id(node);
            let AstKind::StaticMemberExpression(member) = self.nodes.kind(parent) else {
                return Err(Disqualifier::Other);
            };
            let name = member.property.name.as_str();
            if name == "not" {
                node = self.unwrapped(parent);
                continue;
            }
            return if BY_VALUE_MATCHERS.contains(&name) {
                Ok(())
            } else if IDENTITY_METHODS.contains(&name) {
                Err(Disqualifier::Identity)
            } else {
                Err(Disqualifier::Other)
            };
        }
    }

    /// Whether `expression` names the test framework's `expect`: a global or
    /// an import, never a function the module declares itself (whose body
    /// could do anything with its argument).
    fn is_expect(&self, expression: &Expression<'ast>) -> bool {
        let Expression::Identifier(identifier) = expression.without_parentheses() else {
            return false;
        };
        if identifier.name != "expect" {
            return false;
        }
        let Some(reference) = identifier.reference_id.get() else {
            return false;
        };
        self.scoping
            .get_reference(reference)
            .symbol_id()
            .is_none_or(|symbol| self.scoping.symbol_flags(symbol).is_import())
    }

    /// Whether `expression` is an expectation chain: `expect(..)`,
    /// `expect(..).not`.
    fn is_expectation(&self, expression: &Expression<'ast>) -> bool {
        match expression.without_parentheses() {
            Expression::CallExpression(call) => self.is_expect(&call.callee),
            Expression::StaticMemberExpression(member) => {
                member.property.name == "not" && self.is_expectation(&member.object)
            }
            _ => false,
        }
    }

    /// The name of `expression` when it is a reference to a global (no
    /// declaration in the module binds it): `console`, `JSON`, `Object`.
    fn global_name<'expr>(&self, expression: &'expr Expression<'ast>) -> Option<&'expr str> {
        let Expression::Identifier(identifier) = expression.without_parentheses() else {
            return None;
        };
        let reference = self.scoping.get_reference(identifier.reference_id.get()?);
        reference
            .symbol_id()
            .is_none()
            .then(|| identifier.name.as_str())
    }
}

/// Whether `expression` is `null` or `undefined` spelled out: comparing with
/// them asks about presence, not identity.
fn is_nullish_literal(expression: &Expression<'_>) -> bool {
    match expression.without_parentheses() {
        Expression::NullLiteral(_) => true,
        Expression::Identifier(identifier) => identifier.name == "undefined",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    //! Each test names the binding `x` and asserts its decision.

    use oxc::allocator::Allocator;
    use oxc::parser::Parser;
    use oxc::span::SourceType;

    use super::{Disqualifier, LiteralDecision, LiteralRecordUses};

    /// The decision for the first binding named `x` in `source`; `None` when
    /// it is not a candidate.
    fn decide(source: &str) -> Option<LiteralDecision> {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let uses = LiteralRecordUses::collect(&parsed.program);
        let start = source.find(" x")? + 1;
        uses.decision(u32::try_from(start).ok()?)
    }

    /// Assert `x` takes the struct.
    fn assert_struct(source: &str) {
        assert_eq!(decide(source), Some(LiteralDecision::Struct), "{source}");
    }

    /// Assert `x` keeps the map because of `disqualifier`.
    fn assert_map(source: &str, disqualifier: Disqualifier) {
        assert_eq!(
            decide(source),
            Some(LiteralDecision::Map(disqualifier)),
            "{source}"
        );
    }

    #[test]
    fn typed_uses_take_the_struct() {
        assert_struct("const x = { a: 1, s: 'v' }; console.log(x.a + 1, x.s.length);");
        assert_struct("const x = { a: 1, s: 'v' }; x.a = 2; x.a += 1; x.a++;");
        assert_struct("const x = { a: 1, in: { b: 'v', c: [1, 2] } }; x.in.b = 'w'; x.in.c[0];");
        assert_struct("const x = { a: 1, s: 'v' }; const y = x; y.a = 3; const f = () => x.s;");
        assert_struct(
            "const x = [{ a: 1, s: 'v' }]; for (const e of x) { e.a += 1; } x[0].s; x.length;",
        );
        assert_struct("const x = { a: 1, s: 'v' }; console.log(x); JSON.stringify(x, null, 2);");
        assert_struct("const x = { a: 1, t: ['p', 'q'] }; for (const t of x.t) { f(t); }");
        assert_struct(
            "import { expect } from 'vitest'; const x = { a: 1, s: 'v' }; expect(x).toEqual(1);",
        );
        assert_struct(
            "const x = { a: 1, s: 'v' }; expect(x).toEqual({}); expect(f()).not.toStrictEqual(x);",
        );
        assert_struct("const x = { a: 1, s: 'v' }; type T = typeof x; if (x !== undefined) {}");
    }

    #[test]
    fn non_candidates_are_left_alone() {
        assert_eq!(decide("const x = [1, 2, 3];"), None);
        assert_eq!(decide("const x: { a: number } = { a: 1 };"), None);
        assert_eq!(decide("const x = make();"), None);
    }

    #[test]
    fn shape_disqualifies() {
        assert_map("const x = { a: 1, ...rest };", Disqualifier::Shape);
        assert_map("const x = { [k]: 1, s: 'v' };", Disqualifier::Shape);
        assert_map("const x = { 1: 'a', s: 2 };", Disqualifier::Shape);
        assert_map("const x = { a: 1, m() {} };", Disqualifier::Shape);
        assert_map("const x = { a: 1, o: other };", Disqualifier::Shape);
        assert_map("const x = { a: 1, f: () => 1 };", Disqualifier::Shape);
        assert_map("const x = [{ a: 1 }, { b: 's' }];", Disqualifier::Shape);
    }

    #[test]
    fn identity_disqualifies() {
        assert_map(
            "const x = { a: 1, s: 'v' }; x === y;",
            Disqualifier::Identity,
        );
        assert_map(
            "const x = { a: 1, s: 'v' }; Object.is(x, y);",
            Disqualifier::Identity,
        );
        assert_map(
            "const x = { a: 1, s: 'v' }; expect(y).toBe(x);",
            Disqualifier::Identity,
        );
        assert_map(
            "const x = { a: 1, s: 'v' }; expect(x).not.toBe(y);",
            Disqualifier::Identity,
        );
        assert_map(
            "const x = { a: 1, s: 'v' }; seen.has(x);",
            Disqualifier::Identity,
        );
        assert_map(
            "const x = { a: 1, in: { b: 1, c: 's' } }; list.includes(x.in);",
            Disqualifier::Identity,
        );
    }

    #[test]
    fn key_operations_disqualify() {
        assert_map("const x = { a: 1, s: 'v' }; x[k];", Disqualifier::Keys);
        assert_map("const x = { a: 1, s: 'v' }; x[k] = 2;", Disqualifier::Keys);
        assert_map(
            "const x = { a: 1, s: 'v' }; delete x.a;",
            Disqualifier::Keys,
        );
        assert_map("const x = { a: 1, s: 'v' }; x.added;", Disqualifier::Keys);
        assert_map("const x = { a: 1, s: 'v' }; 'a' in x;", Disqualifier::Keys);
        assert_map(
            "const x = { a: 1, s: 'v' }; const y = { ...x };",
            Disqualifier::Keys,
        );
        assert_map(
            "const x = { a: 1, s: 'v' }; Object.entries(x);",
            Disqualifier::Keys,
        );
        assert_map(
            "const x = { a: 1, s: 'v' }; Object.assign(x, y);",
            Disqualifier::Keys,
        );
        assert_map(
            "const x = { a: 1, s: 'v' }; Object.keys(x);",
            Disqualifier::Keys,
        );
        assert_map(
            "const x = { a: 1, s: 'v' }; for (const k in x) {}",
            Disqualifier::Keys,
        );
    }

    #[test]
    fn calls_and_escapes_disqualify() {
        assert_map("const x = { a: 1, s: 'v' }; f(x);", Disqualifier::Call);
        assert_map(
            "function expect(v: unknown) { return v; } const x = { a: 1, s: 'v' }; expect(x).toEqual(1);",
            Disqualifier::Call,
        );
        assert_map(
            "const x = { a: 1, s: 'v' }; JSON.stringify(x, (k, v) => v);",
            Disqualifier::Call,
        );
        assert_map(
            "const x = { a: 1, s: 'v' }; new Box(x);",
            Disqualifier::Call,
        );
        assert_map(
            "const x = { a: 1, s: 'v' }; expect(f(x)).toEqual(1);",
            Disqualifier::Call,
        );
        assert_map(
            "const x = { a: 1, in: { b: 1, c: 's' } }; f(x.in);",
            Disqualifier::Call,
        );
        assert_map(
            "const x = [{ a: 1, s: 'v' }]; for (const e of x) { f(e); }",
            Disqualifier::Call,
        );
        assert_map(
            "function f() { const x = { a: 1, s: 'v' }; return x; }",
            Disqualifier::Escape,
        );
        assert_map(
            "const x = { a: 1, s: 'v' }; const f = () => x;",
            Disqualifier::Escape,
        );
        assert_map(
            "const x = { a: 1, s: 'v' }; const y = [x];",
            Disqualifier::Escape,
        );
        assert_map(
            "const x = { a: 1, s: 'v' }; const y = { x };",
            Disqualifier::Escape,
        );
        assert_map(
            "let x = { a: 1, s: 'v' }; x = { a: 2, s: 'w' };",
            Disqualifier::Escape,
        );
        assert_map("export const x = { a: 1, s: 'v' };", Disqualifier::Escape);
        assert_map(
            "const x = { a: 1, s: 'v' }; const y = x; f(y);",
            Disqualifier::Call,
        );
    }

    #[test]
    fn unmodelled_uses_disqualify() {
        assert_map(
            "const x = { a: 1, in: { b: 1, c: 's' } }; x.in = y;",
            Disqualifier::PartWrite,
        );
        assert_map(
            "const x = [{ a: 1, s: 'v' }]; x.push(y);",
            Disqualifier::Other,
        );
        assert_map(
            "const x = [{ a: 1, s: 'v' }]; x.length = 0;",
            Disqualifier::Other,
        );
        assert_map(
            "const x = { a: 1, s: 'v' }; const y = x as unknown;",
            Disqualifier::Other,
        );
        assert_map(
            "const x = { a: 1, s: 'v' }; const y: object = x;",
            Disqualifier::Other,
        );
        assert_map(
            "const x = { a: 1, s: 'v' }; expect(x).toBeDefined();",
            Disqualifier::Other,
        );
        assert_map(
            "const x = { a: 1, s: 'v' }; const { a } = x;",
            Disqualifier::Other,
        );
    }
}
