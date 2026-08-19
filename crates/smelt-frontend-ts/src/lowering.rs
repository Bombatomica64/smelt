//! TypeScript AST lowering into Smelt HIR.

mod ambient_globals;
mod specialization;
mod state;
mod stdlib;
mod stdlib_dispatch;
mod support;
mod ty;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};
use support::unknown_kind_from_typeof;

use crate::{HirCtx, OverloadSignature, SmeltError};
use oxc::allocator::Allocator;
use oxc::ast::ast::{
    Argument, ArrayExpressionElement, AssignmentTarget, BindingPattern, ChainElement, Declaration,
    Expression, ForStatementInit, ForStatementLeft, MethodDefinitionKind, ModuleExportName,
    ObjectPropertyKind, Program, PropertyKey, PropertyKind, SimpleAssignmentTarget, Statement,
    TSAccessibility,
};
use oxc::parser::{ParseOptions, Parser};
use oxc::span::SourceType;
use oxc::syntax::operator::{AssignmentOperator, BinaryOperator, LogicalOperator, UnaryOperator};
use smelt_hir::{
    AsyncOp, BinOp, Body, CallbackCallArg, CallbackExpr, CallbackExprKind, CaptureMode, Class,
    ClosureCapture, ConstItem, DictProjectionOp, Expr, ExprKind, Field, FileId, Function,
    FunctionOwner, FunctionType, Item, Language, ListCallbackOp, ListSearchOp, Literal, LocalDecl,
    MethodSig, Module, ModuleId, Param, ParamSig, Pattern, PrimitiveCastOp, SetProjectionOp,
    SourceFile, Span, Stmt, StringAffixOp, StringCaseOp, StringPadOp, Type, UnaryOp, UnknownKind,
    Visibility,
};

/// Vitest expectation matchers that can lower to direct HIR checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestMatcher {
    /// `expect(actual).toBe(expected)`.
    Be,
    /// `expect(actual).toEqual(expected)`.
    Equal,
    /// `expect(actual).toStrictEqual(expected)`.
    StrictEqual,
    /// `expect(actual).toContain(expected)`.
    Contain,
    /// `expect(actual).toHaveLength(expected)`.
    HaveLength,
    /// `expect(actual).toHaveProperty(key)`.
    HaveProperty,
    /// `expect(actual).toBeInstanceOf(Ctor)`.
    BeInstanceOf,
}

/// Constant expression exported from another TypeScript module.
///
/// Also used to carry const-folded TypeScript `enum` member values across
/// modules (see [`HirCtx::enum_members`]): each enum member resolves to one of
/// these so `EnumName.Member` reads and `case EnumName.Member:` labels inline
/// the same literal. Public so the sibling [`crate::context`] module can store
/// the cross-module enum-member map, mirroring `ConstCollection`.
#[derive(Debug, Clone)]
pub struct ConstLiteral {
    /// Literal expression to inline at import use sites.
    literal: Literal,
    /// HIR type of the literal.
    ty: smelt_hir::TypeId,
}

impl ConstLiteral {
    /// Return the JavaScript property-member name this constant would name when
    /// used as a computed key (`{ [K]: v }` / `class C { [K]: T }`).
    ///
    /// JavaScript coerces a computed key to a string member name, so string and
    /// numeric constants resolve to a stable static identifier here. A whole
    /// finite number is rendered without a fractional part (`0` -> "0") to match
    /// the numeric-literal key spelling.
    ///
    /// A `Symbol.for(<description>)` registry symbol also folds: registry symbols
    /// are globally interned by description, so every reference names the same
    /// member (issue #115). It maps to the stable synthetic spelling shared with
    /// inline `[Symbol.for(...)]` keys via
    /// [`crate::lowering::ty::computed_key_symbols::registry_symbol_key`]. A
    /// *unique* `Symbol(...)` value has fresh identity each time and never folds.
    ///
    /// Boolean/null constants have no well-defined static member spelling for
    /// Smelt's named-field model and return `None`, leaving genuinely dynamic
    /// keys on the runtime-keyed path.
    fn computed_member_name(&self) -> Option<String> {
        match &self.literal {
            Literal::String(value) => Some(value.clone()),
            Literal::Int(value) => Some(value.to_string()),
            Literal::Float(value) => Some(ModuleBuilder::numeric_property_key_name(*value)),
            Literal::Symbol(value) => {
                ty::computed_key_symbols::registry_description_of_symbol_literal(value)
                    .map(ty::computed_key_symbols::registry_symbol_key)
            }
            Literal::Bool(_) | Literal::Undefined | Literal::None => None,
        }
    }

    /// Return the synthetic registry member key when this constant is a
    /// `Symbol.for(<description>)` registry symbol.
    ///
    /// Registry-backed keys are interned verbatim (they are synthetic member
    /// spellings, not source names), so computed-key resolution uses this to
    /// decide whether a folded const key is a symbol key. Returns `None` for
    /// every non-registry-symbol constant.
    fn symbol_registry_name(&self) -> Option<String> {
        match &self.literal {
            Literal::Symbol(value) => {
                ty::computed_key_symbols::registry_description_of_symbol_literal(value)
                    .map(ty::computed_key_symbols::registry_symbol_key)
            }
            _ => None,
        }
    }
}

/// Constant collection element visible to nested function bodies.
#[derive(Debug, Clone)]
#[expect(
    clippy::redundant_pub_crate,
    reason = "split lowering modules share this internal type through the private parent module"
)]
pub(crate) struct ConstCollectionItem {
    /// HIR expression template to recreate at each read site.
    value: ConstCollectionValue,
    /// HIR type of the recreated expression.
    ty: smelt_hir::TypeId,
}

/// Reusable collection element value.
#[derive(Debug, Clone)]
#[expect(
    clippy::redundant_pub_crate,
    reason = "split lowering modules share this internal enum through the private parent module"
)]
pub(crate) enum ConstCollectionValue {
    /// A self-contained expression kind with no references to body-local expr ids.
    Expr(ExprKind),
    /// A JavaScript `null` represented as erased unknown.
    UnknownNull,
    /// A JavaScript array represented as erased unknown.
    UnknownArray,
    /// A JavaScript object-like value represented as erased unknown.
    UnknownObject,
    /// A JavaScript function represented as erased unknown.
    UnknownFunction,
}

/// Literal collection value visible to nested function bodies in the same module.
#[derive(Debug, Clone)]
pub struct ConstCollection {
    /// Literal elements in source order.
    items: Vec<ConstCollectionItem>,
    /// HIR type of the collection value.
    ty: smelt_hir::TypeId,
    /// Whether the collection should lower as a `Set` instead of an array.
    is_set: bool,
}

/// Optional build-time inputs for TypeScript frontend lowering.
#[derive(Debug, Clone, Copy, Default)]
pub struct FrontendOptions<'manifest> {
    /// Materialized definition-time structure for this source graph.
    pub specialization: Option<&'manifest smelt_specialize::SpecializationManifest>,
}

/// Materialized specialization data owned by one source module builder.
#[derive(Debug, Clone)]
struct SpecializationData {
    /// Module-local materialized definitions.
    module: smelt_specialize::ModuleRecord,
    /// Shared reference-preserving value graph.
    values: Vec<smelt_specialize::GraphValue>,
    /// Required opaque adapters.
    required_adapters: Vec<smelt_specialize::AdapterRequirement>,
}

/// A function that narrows one argument after it returns successfully.
#[derive(Debug, Clone, Copy)]
struct AssertionNarrowing {
    /// Positional parameter index narrowed by the assertion.
    param_index: usize,
    /// Type proven for that argument.
    target: smelt_hir::TypeId,
}

/// A local arrow/function callback value that has not escaped its defining body.
#[derive(Debug, Clone)]
struct LocalCallback {
    /// Root span identifying the lexical body that owns the materialized closure.
    defining_body_span: Option<Span>,
    /// Lowered callback expression tree.
    callback: CallbackExpr,
    /// Parameter types in source order.
    params: Vec<smelt_hir::TypeId>,
    /// Default argument expressions in source order.
    defaults: Vec<Option<LocalCallbackDefault>>,
    /// Rest parameter metadata when the source closure uses `...args`.
    rest: Option<RestParam>,
    /// Number of leading parameters counted by JavaScript `Function.length`.
    required_params: Option<usize>,
    /// Return type produced by the callback.
    return_ty: smelt_hir::TypeId,
}

/// Source value bound into a `test.each` callback parameter.
#[derive(Debug, Clone, Copy)]
enum TableBindingValue<'a> {
    /// A whole row element, used for ordinary identifier parameters.
    Element(&'a ArrayExpressionElement<'a>),
    /// A property expression extracted from an object-literal row.
    ObjectField(&'a Expression<'a>),
}

/// A default argument expression stored for a local callback.
#[derive(Debug, Clone)]
enum LocalCallbackDefault {
    /// The default references callback parameters and must be instantiated at each call site.
    Callback(CallbackExpr),
}

/// A JavaScript/TypeScript rest parameter represented as one list argument.
#[derive(Debug, Clone, Copy)]
pub struct RestParam {
    /// Parameter index of the packed rest list in the lowered closure.
    index: usize,
    /// Element type accepted by each extra source-language argument.
    item_ty: smelt_hir::TypeId,
}

/// A lowered `extends Parent<Args...>` edge kept for lazy interface field lookup.
#[derive(Debug, Clone)]
pub struct InterfaceHeritageRef {
    /// Parent interface symbol.
    parent: smelt_hir::Symbol,
    /// Lowered type arguments supplied to the parent interface.
    args: Vec<smelt_hir::TypeId>,
}

/// A closure expression prepared for a callback-consuming API.
#[derive(Debug, Clone, Copy)]
struct ClosureCallback {
    /// HIR expression id of the closure value.
    expr: smelt_hir::ExprId,
    /// Return type produced by the closure.
    return_ty: smelt_hir::TypeId,
}

impl TestMatcher {
    /// Parse a source matcher name into a supported test matcher.
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "toBe" => Some(Self::Be),
            "toEqual" => Some(Self::Equal),
            "toStrictEqual" => Some(Self::StrictEqual),
            "toContain" => Some(Self::Contain),
            "toHaveLength" => Some(Self::HaveLength),
            "toHaveProperty" => Some(Self::HaveProperty),
            "toBeInstanceOf" => Some(Self::BeInstanceOf),
            _ => None,
        }
    }

    /// Return the source API spelling for diagnostics.
    const fn source_name(self) -> &'static str {
        match self {
            Self::Be => "toBe",
            Self::Equal => "toEqual",
            Self::StrictEqual => "toStrictEqual",
            Self::Contain => "toContain",
            Self::HaveLength => "toHaveLength",
            Self::HaveProperty => "toHaveProperty",
            Self::BeInstanceOf => "toBeInstanceOf",
        }
    }
}

/// Check whether an actual class field type satisfies an interface field.
fn field_type_satisfies(
    krate: &smelt_hir::Crate,
    actual: smelt_hir::TypeId,
    required: &Field,
) -> bool {
    if actual == required.ty {
        return true;
    }
    if !required.optional {
        return false;
    }
    matches!(
        krate.types.get(required.ty),
        Some(Type::Optional(inner)) if *inner == actual
    )
}

/// Parse TypeScript source code and lower it to HIR.
///
/// # Errors
///
/// Returns a vector of errors if parsing or lowering fails.
pub fn to_hir(
    source: &str,
    file_id: FileId,
    ctx: &mut HirCtx,
) -> Result<ModuleId, Vec<SmeltError>> {
    to_hir_with_options(source, file_id, "<memory>", ctx, FrontendOptions::default())
}

/// Parse TypeScript source code from `path` and lower it to HIR.
///
/// # Errors
///
/// Returns a vector of errors if parsing or lowering fails.
pub fn to_hir_with_path(
    source: &str,
    file_id: FileId,
    path: &str,
    ctx: &mut HirCtx,
) -> Result<ModuleId, Vec<SmeltError>> {
    to_hir_with_options(source, file_id, path, ctx, FrontendOptions::default())
}

/// Parse TypeScript source with option-aware specialization inputs.
///
/// # Errors
///
/// Returns parse, specialization, or lowering diagnostics.
pub fn to_hir_with_options(
    source: &str,
    file_id: FileId,
    path: &str,
    ctx: &mut HirCtx,
    options: FrontendOptions<'_>,
) -> Result<ModuleId, Vec<SmeltError>> {
    if is_generated_declaration_file(path, source) {
        return Ok(ctx.krate.push_module(Module::new(
            "main",
            SourceFile {
                path: path.to_owned(),
                language: Language::TypeScript,
            },
        )));
    }
    let allocator = Allocator::default();
    let source_type = if is_typescript_declaration_path(path) {
        SourceType::d_ts()
    } else {
        SourceType::default().with_typescript(true)
    };
    let parsed = Parser::new(&allocator, source, source_type)
        .with_options(ParseOptions::default())
        .parse();

    if !parsed.diagnostics.is_empty() {
        return Err(parsed
            .diagnostics
            .into_iter()
            .map(|error| {
                SmeltError::parse(
                    Span::new(file_id, 0, u32::try_from(source.len()).unwrap_or(u32::MAX)),
                    error.to_string(),
                )
            })
            .collect());
    }

    let specialization = specialization::specialization_for_path(path, options.specialization);
    let mut builder = ModuleBuilder::new(
        file_id,
        path.to_owned(),
        source.to_owned(),
        ctx,
        specialization,
    );
    builder.program(&parsed.program)
}

/// Predeclare TypeScript aliases and class method surfaces before manifest body lowering.
///
/// Dependency cycles through barrel files can otherwise cause a consumer to be
/// lowered before the module that defines an imported alias. This declaration
/// pass preserves the alias's concrete union/generic shape for every member of
/// the cycle without evaluating module bodies or emitting duplicate modules.
///
/// # Errors
///
/// Returns parse diagnostics when the source is not valid TypeScript.
pub fn predeclare_type_declarations_with_path(
    source: &str,
    file_id: FileId,
    path: &str,
    ctx: &mut HirCtx,
) -> Result<(), Vec<SmeltError>> {
    if is_generated_declaration_file(path, source) {
        return Ok(());
    }
    let allocator = Allocator::default();
    let source_type = if is_typescript_declaration_path(path) {
        SourceType::d_ts()
    } else {
        SourceType::default().with_typescript(true)
    };
    let parsed = Parser::new(&allocator, source, source_type)
        .with_options(ParseOptions::default())
        .parse();
    if !parsed.diagnostics.is_empty() {
        return Err(parsed
            .diagnostics
            .into_iter()
            .map(|error| {
                SmeltError::parse(
                    Span::new(file_id, 0, u32::try_from(source.len()).unwrap_or(u32::MAX)),
                    error.to_string(),
                )
            })
            .collect());
    }
    let mut builder = ModuleBuilder::new(
        file_id,
        path.to_owned(),
        source.to_owned(),
        ctx,
        None,
    );
    builder.predeclare_class_method_fields(&parsed.program);
    builder.predeclare_type_alias_items(&parsed.program);
    Ok(())
}

/// Scan one TypeScript source for modeled host constructors it reassigns via
/// `globalThis.<Name> = ...` (or `global.` / `self.`, static or computed).
///
/// This is the per-file half of the crate-level host-global override pre-pass:
/// the transpiler unions these results across every source *before* lowering
/// begins, so a write in a spec file activates the dynamic override machinery
/// (presence guards, `new` dispatch, reads) in the predicate module that lowers
/// first. Only names in the modeled host-object registry are recorded; a write
/// to any other `globalThis` member keeps today's behavior. Parse failures
/// yield an empty set so scanning never blocks a build.
#[must_use]
pub fn scan_written_host_globals(source: &str, path: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    if is_generated_declaration_file(path, source) {
        return names;
    }
    let allocator = Allocator::default();
    let source_type = if is_typescript_declaration_path(path) {
        SourceType::d_ts()
    } else {
        SourceType::default().with_typescript(true)
    };
    let parsed = Parser::new(&allocator, source, source_type)
        .with_options(ParseOptions::default())
        .parse();
    if !parsed.diagnostics.is_empty() {
        return names;
    }
    let mut collector = WrittenHostGlobalCollector { names: &mut names };
    oxc::ast_visit::Visit::visit_program(&mut collector, &parsed.program);
    names
}

/// Return whether an assignment-target member names a reassigned modeled host
/// global (`globalThis.<Name>` / `global.<Name>` / `self.<Name>`, static or a
/// string-literal computed key), yielding the modeled host constructor name.
fn assignment_target_host_global_name<'a>(target: &'a AssignmentTarget<'a>) -> Option<&'a str> {
    let (object, property) = match target {
        AssignmentTarget::StaticMemberExpression(member) if !member.optional => {
            (&member.object, member.property.name.as_str())
        }
        AssignmentTarget::ComputedMemberExpression(member) if !member.optional => {
            let Expression::StringLiteral(key) = &member.expression else {
                return None;
            };
            (&member.object, key.value.as_str())
        }
        _ => return None,
    };
    let Expression::Identifier(base) = object else {
        return None;
    };
    if !ambient_globals::is_global_alias_name(base.name.as_str()) {
        return None;
    }
    (smelt_stdlib::host_object_by_class(property).is_some()).then_some(property)
}

/// AST collector for `globalThis.<HostName> = ...` writes anywhere in a program.
///
/// Walking continues into nested nodes so writes inside function/method bodies,
/// `beforeAll` hooks, and nested test closures are all recorded.
struct WrittenHostGlobalCollector<'names> {
    /// Accumulates each modeled host constructor name written via `globalThis`.
    names: &'names mut HashSet<String>,
}

impl<'a> oxc::ast_visit::Visit<'a> for WrittenHostGlobalCollector<'_> {
    fn visit_assignment_expression(&mut self, assign: &oxc::ast::ast::AssignmentExpression<'a>) {
        if let Some(name) = assignment_target_host_global_name(&assign.left) {
            self.names.insert(name.to_owned());
        }
        oxc::ast_visit::walk::walk_assignment_expression(self, assign);
    }
}

/// Return whether a declaration file was generated by Smelt itself.
fn is_generated_declaration_file(path: &str, source: &str) -> bool {
    is_typescript_declaration_path(path)
        && source.starts_with("// Generated by smelt. Do not edit.")
}

/// Return whether a path names a TypeScript declaration file.
fn is_typescript_declaration_path(path: &str) -> bool {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            let name = name.to_ascii_lowercase();
            name.ends_with(".d.ts") || name.ends_with(".d.mts") || name.ends_with(".d.cts")
        })
}

/// Builder for lowering TypeScript module to HIR.
///
/// Accumulates scoping information, items, and local variables during module construction.
struct ModuleBuilder<'ctx> {
    /// File ID for error reporting.
    file_id: FileId,
    /// Source path for module metadata.
    path: String,
    /// Source text for cheap declaration-order probes.
    source: String,
    /// Mutable reference to the HIR context.
    ctx: &'ctx mut HirCtx,
    /// Per-body local state: name bindings, `Date`-identity and explicit-`any`
    /// facts, callable-local property writes, local callbacks, hoisted local
    /// function items, and the flow-narrowing stack.
    ///
    /// Owns the body-scoping and last-write-wins invariants documented on
    /// [`state::local_scope::LocalScope`].
    scope: state::local_scope::LocalScope,
    /// Typed top-level mutable bindings visible from nested function bodies.
    module_globals: HashMap<String, smelt_hir::TypeId>,
    /// Module-level `let`/`var` bindings lifted to mutable globals, mapped to
    /// their HIR item id. Reads of these names lower to `GlobalGet` and writes
    /// to `GlobalSet` instead of the const-inline or local-assignment paths.
    mutable_global_items: HashMap<String, smelt_hir::ItemId>,
    /// Declared and imported items (functions, classes, interfaces).
    items: HashMap<String, smelt_hir::ItemId>,
    /// Class shapes visible to this module: items, fields, methods, bases,
    /// index-signature value types, lexically scoped type symbols, and the
    /// pending / constructor-function name sets.
    ///
    /// Owns the constructor-function invariant documented on
    /// [`state::class_registry::ClassRegistry`].
    classes: state::class_registry::ClassRegistry,
    /// Interface shapes visible to this module: items, heritage clauses, call and
    /// construct signatures, index-signature value types, and the pending /
    /// locally-lowered name sets.
    ///
    /// Owns the registration invariant documented on
    /// [`state::interface_registry::InterfaceRegistry`].
    interfaces: state::interface_registry::InterfaceRegistry,
    /// Type-level surface: structural alias fields, callable intersection
    /// fields, callable-object aliases, the active namespace path, and the
    /// generic type-parameter scopes.
    ///
    /// Owns the paired type-parameter stack invariant documented on
    /// [`state::type_scope::TypeScope`].
    types: state::type_scope::TypeScope,
    /// Currently processing class name, if any.
    current_class: Option<String>,
    /// Whether the current lowered function body is async.
    current_async: bool,
    /// Declared return type for the current lowered function body.
    current_return_ty: Option<smelt_hir::TypeId>,
    /// Synthetic yield accumulator for the current generator function body.
    current_generator_yields: Option<GeneratorYieldAccumulator>,
    /// Active JavaScript `arguments` object arities for function bodies.
    current_arguments_arities: Vec<usize>,
    /// HIR block that owns side-effect statements emitted while lowering an expression.
    current_statement_block: Option<smelt_hir::BlockId>,
    /// Postfix updates waiting for the variable initializer that reads their original value.
    deferred_postfix_updates: Option<Vec<Stmt>>,
    /// Whether type-test-only lowering may index erased unknown metadata.
    allow_unknown_index_access: bool,
    /// Whether a lifted specialization callable keeps its concrete `this` type through assertions.
    preserve_specialization_receiver: bool,
    /// Provenance of source names this module did not declare: value,
    /// type-only and namespace imports, test builtins, `@date-fns/tz`
    /// factories, and `globalThis` aliases.
    ///
    /// See [`state::import_scope::ImportScope`] for what it does and does not
    /// guarantee about those sets.
    imports: state::import_scope::ImportScope,
    /// Object constants that act as namespace-like API surfaces.
    object_namespaces: HashMap<String, HashMap<String, smelt_hir::ItemId>>,
    /// Folded constant values visible to this module: literals, `enum` members,
    /// `RegExp` literals, object constants, collections and object-value
    /// projections.
    ///
    /// Owns the single-kind and import-rebinding invariants documented on
    /// [`state::const_registry::ConstRegistry`].
    consts: state::const_registry::ConstRegistry,
    /// User assertion functions declared with `asserts value is T`.
    assertion_functions: HashMap<String, AssertionNarrowing>,
    /// User predicate functions declared with `value is T`.
    predicate_functions: HashMap<String, AssertionNarrowing>,
    /// Rest-parameter metadata for top-level function declarations.
    function_rests: HashMap<String, RestParam>,
    /// Forward-visible function declaration signatures for hoisted callback calls.
    forward_function_types: HashMap<String, (smelt_hir::Symbol, smelt_hir::TypeId)>,
    /// TypeScript overload signatures keyed by implementation name.
    function_overloads: HashMap<String, Vec<OverloadSignature>>,
    /// Materialized final definitions for this source module.
    specialization: Option<SpecializationData>,
}

/// Concrete types active while lowering a generator body.
#[derive(Debug, Clone, Copy)]
struct GeneratorYieldAccumulator {
    /// Type exposed by each `yield` suspension point.
    yield_ty: smelt_hir::TypeId,
    /// Type accepted from the caller when the suspension is resumed.
    next_ty: smelt_hir::TypeId,
    /// Whether delegated iterators may use the async iterator protocol.
    is_async: bool,
}

// Lowering builder implementation split into small include files.
mod callbacks;
mod decls;
mod expr;
mod guards;
mod host_override;
mod module_init;
mod new_expr;
mod stmt;
mod testing;
