//! Which unannotated object-literal bindings may lower to a record struct.
//!
//! `const point = { x: 1, label: "a" }` has no annotation, so its values have
//! no common type and the literal used to lower to a map of erased values
//! (`Dict<String, SmeltUnknown>`, a `dynamic-operator` erasure). The checker
//! knows the binding is `{ x: number; label: string }`, and
//! `checker_literal_binding_type` can lower it as the struct the same type
//! literal written in source gives. This module decides, from the uses of the
//! binding, whether that is safe; the one question it cannot answer from
//! syntax (what a callee's parameter does to an argument) it hands to the
//! checker.
//!
//! # What a struct honours, and what it does not
//!
//! A record struct is one JavaScript object held by one shared cell. At its
//! own type it is that object: one identity, writes through aliases and
//! closures, key order as declared (which is the literal's).
//!
//! Handed to a slot of ANOTHER type it used to be converted, and the
//! conversion was a copy. That is no longer so for the slots that hold a view
//! of it: `unknown`, `any`, a type parameter, `Record<K, V>`
//! (`smelt-codegen-rust/src/emitter/live_view.rs`). There the callee writes,
//! extends, keeps, compares and returns the caller's object, and a value a
//! callee rebuilt comes back with the key order it was given. So passing a
//! literal to a call is not, by itself, a reason to keep the map.
//!
//! A slot of a DIFFERENT record type (a parameter typed by an interface or by
//! an object type with properties) holds a view of it too when the two
//! records share: every property the slot's type declares is a property of
//! the value's type, at the same type
//! (`smelt-codegen-rust/src/structural_views.rs`). And a literal whose
//! declared slots name a record type declaring exactly its properties
//! (optional ones after them allowed) takes that type and is not converted
//! there at all (`checker_literal_binding_type`). A literal whose values
//! share one type, landing in record types it does not match exactly, is
//! built as its own record, which they all view. A record type that does
//! neither (a class, a property at a type the view cannot convert) keeps the
//! map, which the record built from it adopts
//! (`SmeltRecord::smelt_adopt_typed`), so the two stay one object. A literal
//! kept as a map for a key-set use goes to every record slot as its own
//! record (`ModuleBuilder::kept_map_literal_record`), so one record adopts it
//! and the slots view that record.
//!
//! # The rule
//!
//! A binding `x` declared without annotation from an object literal, or from
//! an array literal holding object literals, takes the record struct only when
//! ALL hold; otherwise it keeps the map lowering it has always had, with the
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
//! 2. **Every use keeps the value typed or passes it on**
//!    ([`Analysis::classify`]). A *flow* is a reference to `x`, a read of a
//!    record- or array-valued part of a flow (`x.inner`, `xs[i]`), or a
//!    reference to a `const`/`let` alias or `for..of` element bound to a flow.
//!    Each flow must be one of:
//!    - a read or write of a declared property with a static key, or of an
//!      array element or `length` (a write only of a primitive property);
//!    - the initializer of an unannotated, never reassigned alias, or the
//!      iterable of a `for..of` over an array flow;
//!    - a value landing in a slot of a declared type ([`SlotUse`], decided by
//!      3): an argument of a call or a `new`, the initializer of an annotated
//!      alias or an element of an annotated binding's array literal, the
//!      operand of `as T`, the operand of a `return` from a function with a
//!      declared return type (`satisfies` hands the value on unchanged);
//!    - an equality comparison (`===`, `!==`, `==`, `!=`), `typeof`, or a
//!      statement of its own.
//! 3. **Every declared slot it lands in shares it**
//!    (`ModuleBuilder::literal_slot_targets`): the checker's type for the slot
//!    holds a view of the record, cannot hold a record at all, or lowers to
//!    the record type the binding is declared at or one it shares with
//!    (`ModuleBuilder::record_types_share`). No callee is named:
//!    `console.log`, `JSON.stringify`, `Object.keys` and the test framework's
//!    matchers pass because their parameters are `any` or generic, exactly
//!    like a user function's.
//!
//! Anything else disqualifies the binding, classified for reporting as a
//! [`Disqualifier`]: a key-set or key-order operation spelled on the typed
//! value, a replaced record-valued property, a slot of a type that still
//! copies (a class, a generic record) or that the checker cannot type, an
//! escape whose slot is not inspected (assigned into another object, thrown,
//! captured by a closure, exported, reassigned), or a use this module does
//! not model.
//!
//! # Requirements
//!
//! - **Behavior**: a program in which a literal takes the struct prints what
//!   it prints with the map lowering, which is what Node prints. Identity
//!   (`===` between aliases and across a call), key order
//!   (`JSON.stringify`, `console.log`, `Object.keys`) and the visibility of
//!   writes through aliases, closures and callees are preserved: at its own
//!   type the struct is the object, and every other slot the rule admits holds
//!   a view of it.
//! - **Type preservation**: a literal that takes the struct has every
//!   property at the type the checker inferred, with no erased part; one that
//!   keeps the map is typed exactly as before this rule existed.
//! - **Conservative**: a use that cannot be proved safe keeps the map, so the
//!   rule can lose typing and never behavior.
//!
//! Checked by `examples/typescript/end-to-end/197_object_literal_takes_its_checker_record`
//! (the literal that takes the struct: goldens of the HIR, MIR and Rust, no
//! erased value), by `smelt-transpiler/tests/literal_record_cli_tests.rs` (one
//! fixture per family of uses: what each literal lowers to, and that the
//! program prints what Node prints) and by the tests below.
//!
//! # Measuring
//!
//! With `SMELT_LITERAL_RECORD_TRACE` set, `checker_literal_binding_type`
//! prints one line per candidate binding to stderr: its decision, and whether
//! the checker's type was representable. That is how the cost of each
//! disqualifier is counted.

use std::cell::RefCell;
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
    /// The key set or key order is observed or changed by an operation
    /// spelled on the typed value: a computed key, `delete`, `in`, `for..in`,
    /// a property the literal does not declare, a spread.
    Keys,
    /// A record- or array-valued property is replaced (`x.inner = other`),
    /// which would convert the assigned value.
    PartWrite,
    /// The value is an argument of a call or constructor whose parameter
    /// there does not share the caller's object: a record type it does not
    /// share with, or a callee the checker cannot type
    /// (`checker_argument_shares`, `literal_slot_targets`).
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
            Self::Keys => "keys",
            Self::PartWrite => "part-write",
            Self::Call => "call",
            Self::Escape => "escape",
            Self::Other => "other",
        }
    }
}

/// A flow handed to a call or a `new` as an argument.
///
/// The callee's parameter decides what happens to it, and that is the
/// checker's knowledge, not this syntactic pass's: a parameter that holds a
/// view of the record (`unknown`, `any`, a type parameter, `Record<K, V>`),
/// the record itself, or another record type it shares with, shares the
/// object; any other record type converts it. `checker_literal_binding_type`
/// asks, per use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::lowering) struct ArgumentUse {
    /// Start of the callee expression (the function called or constructed).
    pub(in crate::lowering) callee_start: u32,
    /// End of the callee expression.
    pub(in crate::lowering) callee_end: u32,
    /// Position of the argument in the call.
    pub(in crate::lowering) index: usize,
    /// `new callee(..)` rather than `callee(..)`.
    pub(in crate::lowering) construct: bool,
    /// Start of the argument expression: the flow as it is spelled there (the
    /// binding, a part read from it, an alias), whose own type a parameter of
    /// another record type is compared with.
    pub(in crate::lowering) value_start: u32,
    /// End of the argument expression.
    pub(in crate::lowering) value_end: u32,
}

/// A slot of a declared type the value lands in, whose type the checker
/// knows and this syntactic pass does not.
///
/// A slot that holds a view of the record shares the object, and so does a
/// slot of another plain object type: the conversion there links the two
/// records into one object (`smelt-codegen-rust/src/linked_records.rs`). A
/// slot of the binding's own record type, or of one with exactly its
/// properties, is where the binding takes its type from
/// (`checker_literal_binding_type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::lowering) enum SlotUse {
    /// An argument of a call or a `new` (see [`ArgumentUse`]).
    Argument(ArgumentUse),
    /// The initializer of the annotated binding at `start..end`
    /// (`const f: Flags = x`), or, with `element`, an element of the array
    /// literal that initializes it (`const all: Flags[] = [x]`).
    Annotated {
        /// Start of the annotated binding identifier.
        start: u32,
        /// End of the annotated binding identifier.
        end: u32,
        /// The value is an element of the binding's array literal.
        element: bool,
    },
    /// The operand of the type assertion `x as T` spanning `start..end`: the
    /// value goes on at `T`, a view of the record when `T` holds one
    /// (`unknown`, `object`), a linked record when `T` is another plain
    /// object type.
    Asserted {
        /// Start of the assertion expression.
        start: u32,
        /// End of the assertion expression.
        end: u32,
    },
    /// The operand of a `return` in the function, with a declared return
    /// type, named by the binding identifier at `start..end`.
    Returned {
        /// Start of the function's name.
        start: u32,
        /// End of the function's name.
        end: u32,
    },
}

/// The decision for one candidate binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::lowering) enum LiteralDecision {
    /// Every use keeps the value typed or hands it to a slot of a declared
    /// type: lower the checker's record type (or the slots' common one),
    /// provided every listed slot shares the object (see [`SlotUse`]).
    Struct(Vec<SlotUse>),
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
    /// The record slots of the bindings that keep their dictionary for a
    /// key-set use ([`Self::kept_map_slots`]).
    kept_map_slots: HashMap<u32, Vec<(SlotUse, (u32, u32))>>,
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
            arguments: RefCell::new(Vec::new()),
            values: RefCell::new(Vec::new()),
            ignored: ignored_disqualifiers(),
        };
        let mut decisions = HashMap::new();
        let mut kept_map_slots = HashMap::new();
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
            analysis.arguments.borrow_mut().clear();
            analysis.values.borrow_mut().clear();
            let decision = match analysis.decide(node.id(), binding.symbol_id.get(), init) {
                Ok(()) => {
                    analysis.values.borrow_mut().clear();
                    LiteralDecision::Struct(analysis.arguments.take())
                }
                Err(disqualifier) => {
                    // A binding that stays a dictionary only because its key
                    // set is observed still lands in record slots; those
                    // uses are kept so each can go through the literal's own
                    // record (`ModuleBuilder::kept_map_literal_record`).
                    let slots = analysis.arguments.take();
                    let values = analysis.values.take();
                    if disqualifier == Disqualifier::Keys && !slots.is_empty() {
                        kept_map_slots
                            .insert(binding.span.start, slots.into_iter().zip(values).collect());
                    }
                    LiteralDecision::Map(disqualifier)
                }
            };
            decisions.insert(binding.span.start, decision);
        }
        Self {
            decisions,
            kept_map_slots,
        }
    }

    /// The declared slots, each with the span of the value that lands in it,
    /// of the binding identifier starting at `binding_start` when it keeps
    /// its dictionary only because its key set is observed
    /// ([`Disqualifier::Keys`]); empty otherwise.
    pub(in crate::lowering) fn kept_map_slots(
        &self,
        binding_start: u32,
    ) -> &[(SlotUse, (u32, u32))] {
        self.kept_map_slots
            .get(&binding_start)
            .map_or(&[], Vec::as_slice)
    }

    /// The decision for the binding identifier starting at `binding_start`;
    /// `None` when it is not a candidate (annotated, or its initializer is not
    /// a literal holding an object literal), which keeps its lowering as is.
    pub(in crate::lowering) fn decision(&self, binding_start: u32) -> Option<LiteralDecision> {
        self.decisions.get(&binding_start).cloned()
    }
}

/// The disqualifiers named in `SMELT_LITERAL_RECORD_IGNORE` (a comma-separated
/// list of [`Disqualifier::name`]s), which the analysis then does not apply.
///
/// A measurement switch, the companion of `SMELT_LITERAL_RECORD_TRACE`: it is
/// how a disqualifier is shown to be required (a fixture prints something
/// else than Node with it off) or not, and how its cost is counted. A build
/// made with it set is for that comparison, not for shipping. `shape` cannot
/// be switched off: a literal with no part structure has no uses to follow.
fn ignored_disqualifiers() -> Vec<Disqualifier> {
    let Some(names) = std::env::var_os("SMELT_LITERAL_RECORD_IGNORE") else {
        return Vec::new();
    };
    let names = names.to_string_lossy();
    [
        Disqualifier::Keys,
        Disqualifier::PartWrite,
        Disqualifier::Call,
        Disqualifier::Escape,
        Disqualifier::Other,
    ]
    .into_iter()
    .filter(|disqualifier| {
        names
            .split(',')
            .any(|name| name.trim() == disqualifier.name())
    })
    .collect()
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

/// One module's semantic model, and the classification of flows over it.
struct Analysis<'model, 'ast> {
    /// Every AST node, with its parent.
    nodes: &'model AstNodes<'ast>,
    /// Symbols and their resolved references.
    scoping: &'model Scoping,
    /// The declared slots the binding being decided lands in.
    arguments: RefCell<Vec<SlotUse>>,
    /// The span of the value at each of `arguments`, in the same order: the
    /// flow as it is spelled where it lands in the slot.
    values: RefCell<Vec<(u32, u32)>>,
    /// Disqualifiers switched off for a measurement
    /// ([`ignored_disqualifiers`]); empty in every ordinary build.
    ignored: Vec<Disqualifier>,
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
        // A key-set use keeps the dictionary, but the other uses are still
        // classified: any other disqualifier wins, and the record slots they
        // land in are what a kept dictionary is shared through.
        let mut keys = false;
        for reference in self.scoping.get_resolved_references(symbol) {
            if reference.is_write() {
                // Reassigned: the binding would hold a converted value.
                return Err(Disqualifier::Escape);
            }
            if reference.is_type() || reference.flags().is_value_as_type() {
                // `typeof x` in a type position names the type, not the value.
                continue;
            }
            match self.classify(reference.node_id(), shape, visiting) {
                // Measurement only: the use is admitted as if it kept the
                // value typed ([`ignored_disqualifiers`]).
                Err(disqualifier) if self.ignored.contains(&disqualifier) => {}
                Err(Disqualifier::Keys) => keys = true,
                other => other?,
            }
        }
        if keys {
            return Err(Disqualifier::Keys);
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
                    // The annotated alias holds the value at its declared
                    // type, which the checker judges (`SlotUse::Annotated`).
                    self.slot(
                        SlotUse::Annotated {
                            start: alias.span.start,
                            end: alias.span.end,
                            element: false,
                        },
                        span,
                    );
                    return Ok(());
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
                self.argument(call, span)
            }
            AstKind::NewExpression(new) => {
                if new.callee.span() == span {
                    return Err(Disqualifier::Other);
                }
                let index = new
                    .arguments
                    .iter()
                    .position(|argument| argument.span() == span)
                    .ok_or(Disqualifier::Call)?;
                self.passed(new.callee.span(), index, true, span);
                Ok(())
            }
            AstKind::BinaryExpression(binary) => {
                match binary.operator {
                    // A record struct is one object: `===` between two of
                    // them, or against a view of one, compares identities as
                    // JavaScript does.
                    BinaryOperator::StrictEquality
                    | BinaryOperator::StrictInequality
                    | BinaryOperator::Equality
                    | BinaryOperator::Inequality => Ok(()),
                    BinaryOperator::In => Err(Disqualifier::Keys),
                    _ => Err(Disqualifier::Other),
                }
            }
            AstKind::UnaryExpression(unary) => match unary.operator {
                UnaryOperator::Typeof => Ok(()),
                UnaryOperator::Delete => Err(Disqualifier::Keys),
                _ => Err(Disqualifier::Other),
            },
            // `x as T` / `x satisfies T`: the asserted type is a slot like an
            // annotation (`SlotUse::Annotated` on the assertion's own span);
            // `as const` re-reads the literal and is not modelled.
            AstKind::TSAsExpression(assertion) if assertion.expression.span() == span => {
                if matches!(
                    &assertion.type_annotation,
                    oxc::ast::ast::TSType::TSTypeReference(reference)
                        if matches!(&reference.type_name, oxc::ast::ast::TSTypeName::IdentifierReference(name) if name.name == "const")
                ) {
                    return Err(Disqualifier::Other);
                }
                self.slot(
                    SlotUse::Asserted {
                        start: assertion.span.start,
                        end: assertion.span.end,
                    },
                    span,
                );
                Ok(())
            }
            AstKind::TSSatisfiesExpression(assertion) if assertion.expression.span() == span => {
                // `satisfies` checks and hands the value on unchanged.
                self.classify(parent, shape, visiting)
            }
            AstKind::ExpressionStatement(_) => Ok(()),
            // `() => x` returns the value.
            AstKind::ArrowFunctionExpression(_) | AstKind::FunctionBody(_) => {
                Err(Disqualifier::Escape)
            }
            AstKind::SpreadElement(_) => Err(Disqualifier::Keys),
            AstKind::ReturnStatement(_) => {
                let (start, end) = self
                    .returning_function(parent)
                    .ok_or(Disqualifier::Escape)?;
                self.slot(SlotUse::Returned { start, end }, span);
                Ok(())
            }
            AstKind::ArrayExpression(_) => {
                let (start, end) = self
                    .annotated_array_binding(parent)
                    .ok_or(Disqualifier::Escape)?;
                self.slot(
                    SlotUse::Annotated {
                        start,
                        end,
                        element: true,
                    },
                    span,
                );
                Ok(())
            }
            AstKind::ThrowStatement(_)
            | AstKind::YieldExpression(_)
            | AstKind::AwaitExpression(_)
            | AstKind::ObjectProperty(_)
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

    /// Record that the flow being classified, spelled at `value`, is argument
    /// `index` of the call (or `new`) whose callee spans `callee`.
    fn passed(&self, callee: Span, index: usize, construct: bool, value: Span) {
        self.slot(
            SlotUse::Argument(ArgumentUse {
                callee_start: callee.start,
                callee_end: callee.end,
                index,
                construct,
                value_start: value.start,
                value_end: value.end,
            }),
            value,
        );
    }

    /// Record that the flow being classified, spelled at `value`, lands in
    /// the declared `slot`.
    fn slot(&self, slot: SlotUse, value: Span) {
        self.arguments.borrow_mut().push(slot);
        self.values.borrow_mut().push((value.start, value.end));
    }

    /// The name of the function a `return` at `statement` returns from, when
    /// that function declares its return type and is named by a binding
    /// identifier: a function declaration or expression's own name, or the
    /// unannotated `const`/`let` an arrow or function expression initializes.
    fn returning_function(&self, statement: NodeId) -> Option<(u32, u32)> {
        let mut node = statement;
        loop {
            let parent = self.nodes.parent_id(node);
            if parent == node {
                return None;
            }
            node = parent;
            match self.nodes.kind(node) {
                AstKind::Function(function) => {
                    function.return_type.as_ref()?;
                    if let Some(id) = &function.id {
                        return Some((id.span.start, id.span.end));
                    }
                    return self.initialized_binding(node);
                }
                AstKind::ArrowFunctionExpression(arrow) => {
                    arrow.return_type.as_ref()?;
                    return self.initialized_binding(node);
                }
                AstKind::Class(_) | AstKind::StaticBlock(_) => return None,
                _ => {}
            }
        }
    }

    /// The unannotated binding identifier the expression at `node` (through
    /// parentheses) initializes.
    fn initialized_binding(&self, node: NodeId) -> Option<(u32, u32)> {
        let node = self.unwrapped(node);
        match self.nodes.parent_kind(node) {
            AstKind::VariableDeclarator(declarator) if declarator.type_annotation.is_none() => {
                match &declarator.id {
                    BindingPattern::BindingIdentifier(id) => Some((id.span.start, id.span.end)),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// The annotated binding identifier the array literal at `array` (through
    /// parentheses) initializes.
    fn annotated_array_binding(&self, array: NodeId) -> Option<(u32, u32)> {
        let array = self.unwrapped(array);
        match self.nodes.parent_kind(array) {
            AstKind::VariableDeclarator(declarator) if declarator.type_annotation.is_some() => {
                match &declarator.id {
                    BindingPattern::BindingIdentifier(id) => Some((id.span.start, id.span.end)),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Classify a flow at `span` passed as an argument of `call`.
    ///
    /// Passing the value is not a disqualifier by itself: a callee whose
    /// parameter holds a view of the record, or the record, shares the
    /// caller's object, whatever its body does with it. Which parameters do
    /// is the checker's knowledge, so the position is recorded
    /// ([`ArgumentUse`]) and decided where the checker is at hand.
    fn argument(&self, call: &CallExpression<'ast>, span: Span) -> Result<(), Disqualifier> {
        let index = call
            .arguments
            .iter()
            .position(|argument| argument.span() == span)
            .ok_or(Disqualifier::Call)?;
        self.passed(call.callee.span(), index, false, span);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! Each test names the binding `x` and asserts its decision.

    use oxc::allocator::Allocator;
    use oxc::parser::Parser;
    use oxc::span::SourceType;

    use super::{ArgumentUse, Disqualifier, LiteralDecision, LiteralRecordUses, SlotUse};

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

    /// Assert `x` takes the struct, with no argument left to the checker.
    fn assert_struct(source: &str) {
        assert_eq!(
            decide(source),
            Some(LiteralDecision::Struct(Vec::new())),
            "{source}"
        );
    }

    /// Assert `x` takes the struct provided the checker accepts `count`
    /// argument positions, and return them.
    fn assert_passed(source: &str, count: usize) -> Vec<ArgumentUse> {
        let decision = decide(source);
        assert!(
            matches!(decision, Some(LiteralDecision::Struct(_))),
            "`x` must be a struct candidate: {source}"
        );
        let arguments = match decision {
            Some(LiteralDecision::Struct(slots)) => slots
                .into_iter()
                .filter_map(|slot| match slot {
                    SlotUse::Argument(argument) => Some(argument),
                    SlotUse::Annotated { .. }
                    | SlotUse::Asserted { .. }
                    | SlotUse::Returned { .. } => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        assert_eq!(
            arguments.len(),
            count,
            "`x` must be a struct candidate with {count} argument use(s): {source}"
        );
        arguments
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
        assert_struct("const x = { a: 1, t: ['p', 'q'] }; for (const t of x.t) { f(t); }");
        assert_struct("const x = { a: 1, s: 'v' }; type T = typeof x; if (x !== undefined) {}");
    }

    /// A flow handed to a call or a `new` is recorded for the checker, with
    /// the callee's span and the argument's position; no callee is exempt or
    /// excluded by its name.
    #[test]
    fn arguments_are_left_to_the_checker() {
        let start_of = |source: &str, callee: &str| {
            source
                .find(callee)
                .and_then(|start| u32::try_from(start).ok())
        };
        let plain = "const x = { a: 1, s: 'v' }; f(0, x);";
        let plain_start = start_of(plain, "f(").unwrap_or(u32::MAX);
        assert_eq!(
            assert_passed(plain, 1),
            [ArgumentUse {
                callee_start: plain_start,
                callee_end: plain_start.saturating_add(1),
                index: 1,
                construct: false,
                value_start: plain_start.saturating_add(5),
                value_end: plain_start.saturating_add(6),
            }]
        );
        let constructed = "const x = { a: 1, s: 'v' }; new Box(x);";
        assert!(
            assert_passed(constructed, 1)
                .first()
                .is_some_and(|use_| use_.construct)
        );
        let member = "const x = { a: 1, s: 'v' }; console.log(x);";
        let member_start = start_of(member, "console.log").unwrap_or(u32::MAX);
        assert_eq!(
            assert_passed(member, 1)
                .first()
                .map(|use_| (use_.callee_start, use_.callee_end)),
            Some((member_start, member_start.saturating_add(11)))
        );
        for (source, count) in [
            (
                "const x = { a: 1, s: 'v' }; console.log(x); JSON.stringify(x, null, 2);",
                2,
            ),
            (
                "const x = { a: 1, s: 'v' }; JSON.stringify(x, (k, v) => v);",
                1,
            ),
            (
                "const x = { a: 1, s: 'v' }; expect(x).toEqual({}); expect(f()).not.toStrictEqual(x);",
                2,
            ),
            (
                "const x = { a: 1, s: 'v' }; expect(y).toBe(x); expect(x).toBeDefined();",
                2,
            ),
            (
                "const x = { a: 1, s: 'v' }; Object.is(x, y); seen.has(x);",
                2,
            ),
            (
                "const x = { a: 1, s: 'v' }; Object.keys(x); Object.assign(x, y);",
                2,
            ),
            (
                "const x = { a: 1, in: { b: 1, c: 's' } }; f(x.in); list.includes(x.in);",
                2,
            ),
            (
                "const x = [{ a: 1, s: 'v' }]; for (const e of x) { f(e); }",
                1,
            ),
            (
                "const x = { a: 1, s: 'v' }; const y = x; f(y); expect(f(x)).toEqual(1);",
                2,
            ),
        ] {
            assert_passed(source, count);
        }
        // A spread argument takes the value apart; a call ON the value is not
        // an argument at all.
        assert_map("const x = [{ a: 1, s: 'v' }]; f(...x);", Disqualifier::Keys);
        assert_map("const x = { a: 1, s: 'v' }; x();", Disqualifier::Other);
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

    /// Comparing the value by identity is a typed use: a record struct is
    /// one object, and `===` compares its identity.
    #[test]
    fn identity_comparisons_keep_the_struct() {
        assert_struct("const x = { a: 1, s: 'v' }; x === y; if (x != y) {}");
        assert_struct("const x = { a: 1, s: 'v' }; if (x !== undefined && x != null) {}");
        assert_map("const x = { a: 1, s: 'v' }; x + 1;", Disqualifier::Other);
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
            "const x = { a: 1, s: 'v' }; for (const k in x) {}",
            Disqualifier::Keys,
        );
    }

    #[test]
    fn escapes_disqualify() {
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
    }

    /// A value that lands in a slot of a declared type (an annotated alias,
    /// an element of an annotated binding's array literal, the operand of a
    /// `return` from a function with a declared return type) is recorded for
    /// the checker with the slot's binding; one whose slot has no declared
    /// type to judge stays an escape.
    #[test]
    fn declared_slots_are_left_to_the_checker() {
        let slots = |source: &str| match decide(source) {
            Some(LiteralDecision::Struct(slots)) => slots,
            _ => Vec::new(),
        };
        let span_of = |source: &str, name: &str| {
            let start = source
                .find(name)
                .and_then(|at| u32::try_from(at).ok())
                .unwrap_or(0);
            (start, start + u32::try_from(name.len()).unwrap_or(0))
        };
        let annotated = "const x = { a: 1, s: 'v' }; const held: object = x;";
        let (start, end) = span_of(annotated, "held");
        assert_eq!(
            slots(annotated),
            [SlotUse::Annotated {
                start,
                end,
                element: false
            }]
        );
        let element = "const x = { a: 1, s: 'v' }; const all: T[] = [x, x];";
        let (all_start, all_end) = span_of(element, "all");
        let in_all = SlotUse::Annotated {
            start: all_start,
            end: all_end,
            element: true,
        };
        assert_eq!(slots(element), [in_all, in_all]);
        let returned = "function make(): T { const x = { a: 1, s: 'v' }; return x; }";
        let (make_start, make_end) = span_of(returned, "make");
        assert_eq!(
            slots(returned),
            [SlotUse::Returned {
                start: make_start,
                end: make_end
            }]
        );
        let arrow = "const make = (): T => { const x = { a: 1, s: 'v' }; return x; };";
        let (arrow_start, arrow_end) = span_of(arrow, "make");
        assert_eq!(
            slots(arrow),
            [SlotUse::Returned {
                start: arrow_start,
                end: arrow_end
            }]
        );
        assert_map(
            "const f = () => { const x = { a: 1, s: 'v' }; return x; };",
            Disqualifier::Escape,
        );
        assert_map(
            "const x = { a: 1, s: 'v' }; const all = [[x]];",
            Disqualifier::Escape,
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
            "const x = { a: 1, s: 'v' }; const y = x as const;",
            Disqualifier::Other,
        );

        assert_map(
            "const x = { a: 1, s: 'v' }; const { a } = x;",
            Disqualifier::Other,
        );
    }
}
