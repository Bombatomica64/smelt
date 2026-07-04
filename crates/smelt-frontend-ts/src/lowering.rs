//! TypeScript AST lowering into Smelt HIR.

mod ambient_globals;
mod specialization;
mod stdlib;
mod stdlib_dispatch;
mod support;
mod ty;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};
use support::{
    is_static_property_key, unknown_kind_from_typeof,
};

use crate::{
    HirCtx, ObjectConst,
    OverloadSignature, SmeltError,
};
use oxc::allocator::Allocator;
use oxc::ast::ast::{
    Argument, ArrayExpressionElement, AssignmentTarget, BindingPattern, ChainElement,
    Declaration, Expression, ForStatementInit, ForStatementLeft, MethodDefinitionKind, ModuleExportName,
    ObjectPropertyKind, Program, PropertyKey, PropertyKind, SimpleAssignmentTarget, Statement,
    TSAccessibility,
};
use oxc::parser::{ParseOptions, Parser};
use oxc::span::SourceType;
use oxc::syntax::operator::{
    AssignmentOperator, BinaryOperator, LogicalOperator, UnaryOperator,
};
use smelt_hir::{
    AsyncOp, BinOp, Body, CallbackCallArg, CallbackExpr, CallbackExprKind, CaptureMode, Class,
    ClosureCapture, ConstItem, DictProjectionOp, Expr, ExprKind, Field, FileId, Function,
    FunctionOwner, FunctionType, Item, Language, ListCallbackOp, ListSearchOp,
    Literal, LocalDecl, MethodSig, Module, ModuleId, Param, ParamSig, Pattern,
    PrimitiveCastOp, SetProjectionOp, SourceFile, Span, Stmt, StringAffixOp,
    StringCaseOp, StringPadOp, Type, UnaryOp, UnknownKind, Visibility,
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
    /// Local variable bindings in current scope.
    locals: HashMap<String, smelt_hir::LocalId>,
    /// Local values statically known to retain JavaScript `Date` identity.
    date_value_locals: HashSet<smelt_hir::LocalId>,
    /// Typed top-level mutable bindings visible from nested function bodies.
    module_globals: HashMap<String, smelt_hir::TypeId>,
    /// Declared and imported items (functions, classes, interfaces).
    items: HashMap<String, smelt_hir::ItemId>,
    /// Class definitions by name.
    classes: HashMap<String, smelt_hir::ItemId>,
    /// Class names declared later in the current module.
    pending_class_names: HashSet<String>,
    /// Interface definitions by name.
    interfaces: HashMap<String, smelt_hir::ItemId>,
    /// Fields for each class.
    class_fields: HashMap<String, Vec<Field>>,
    /// Method signatures for classes that are visible before their item is fully emitted.
    class_methods: HashMap<String, Vec<MethodSig>>,
    /// Base class metadata for the class currently being lowered.
    class_bases: HashMap<String, (smelt_hir::Symbol, Vec<smelt_hir::TypeId>)>,
    /// Fields carried by structural type aliases.
    type_alias_fields: HashMap<smelt_hir::Symbol, Vec<Field>>,
    /// Interface heritage clauses for resolving fields after cyclic type imports settle.
    interface_extends: HashMap<smelt_hir::Symbol, Vec<InterfaceHeritageRef>>,
    /// Value types declared by interface string index signatures.
    interface_index_values: HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    /// Value types declared by class string index signatures (`[k: string]: T`).
    ///
    /// Mirrors `interface_index_values` for classes: keyed by class name symbol,
    /// this records the value type of a class's `[key: string]: T` index
    /// signature so member and computed access can fall back to it when no
    /// declared named field or method matches.
    class_index_values: HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    /// Interface call signatures for callable interface types.
    interface_call_signatures: HashMap<smelt_hir::Symbol, Vec<FunctionType>>,
    /// Interface construct signatures (`new (): T`) for constructor-slot types.
    ///
    /// A constructor-interface such as `interface MapCacheConstructor { new
    /// (): MapCache }` is, at runtime, an ordinary callable value: `new
    /// value()` invokes it to produce the constructed type. Each construct
    /// signature is stored as the equivalent [`FunctionType`] (its parameters
    /// and constructed return type) so a reference to the interface can lower to
    /// a typed constructor slot instead of an erased dictionary.
    interface_construct_signatures: HashMap<smelt_hir::Symbol, Vec<FunctionType>>,
    /// Fields attached to callable intersection types.
    callable_fields: HashMap<smelt_hir::TypeId, Vec<Field>>,
    /// Type aliases whose source surface is a callable object intersection.
    callable_object_aliases: HashSet<smelt_hir::Symbol>,
    /// Namespace path currently qualifying type-only declarations.
    type_namespace_prefix: Vec<String>,
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
    /// Test-framework API names imported from Vitest-compatible modules.
    test_builtins: HashSet<String>,
    /// Local names statically known to alias the ambient global object.
    ///
    /// Populated for `const g = globalThis;` style bindings so that global-path
    /// normalization and feature-probe erasure recognize `g.Object.keys(x)` and
    /// `"Map" in g` as global references. Used only for preserving known member
    /// types and stdlib dispatch; dynamic correctness would come from a shared
    /// runtime object if Phase 2/3 lands.
    global_object_aliases: HashSet<String>,
    /// Local names bound by namespace imports such as `import * as MathApi from "./math"`.
    namespace_imports: HashSet<String>,
    /// Local names imported only for TypeScript type positions.
    type_only_imports: HashSet<String>,
    /// Local names imported as runtime values.
    value_imports: HashSet<String>,
    /// Local names bound to `tz` from the `@date-fns/tz` package.
    date_fns_timezone_factories: HashSet<String>,
    /// Object constants that act as namespace-like API surfaces.
    object_namespaces: HashMap<String, HashMap<String, smelt_hir::ItemId>>,
    /// Literal constant items visible from already-lowered modules.
    const_literals: HashMap<String, ConstLiteral>,
    /// Const-folded TypeScript `enum` member values keyed by enum name then
    /// member name.
    ///
    /// Populated from `enum` declarations in the current module and seeded from
    /// [`HirCtx::enum_members`] for enums declared in earlier modules. Lets
    /// `EnumName.Member` reads and `case EnumName.Member:` labels inline the
    /// member's constant literal.
    enum_member_literals: HashMap<String, HashMap<String, ConstLiteral>>,
    /// `RegExp` literal constants visible from nested function bodies.
    const_regexps: HashMap<String, (String, String, smelt_hir::TypeId)>,
    /// Object literal constants visible from current and already-lowered modules.
    const_objects: HashMap<String, ObjectConst>,
    /// Array/set constants visible from nested function bodies.
    const_collections: HashMap<String, ConstCollection>,
    /// Object constants whose values can be projected by `Object.values`.
    const_object_value_collections: HashMap<String, ConstCollection>,
    /// User assertion functions declared with `asserts value is T`.
    assertion_functions: HashMap<String, AssertionNarrowing>,
    /// User predicate functions declared with `value is T`.
    predicate_functions: HashMap<String, AssertionNarrowing>,
    /// Active local narrowings from guards and assertion calls.
    narrowed_locals: Vec<HashMap<String, smelt_hir::TypeId>>,
    /// Active generic type parameter scopes.
    type_param_scopes: Vec<HashMap<String, smelt_hir::TypeId>>,
    /// Constraints for active generic type parameters keyed by HIR type parameter symbol.
    type_param_constraint_scopes: Vec<HashMap<smelt_hir::Symbol, smelt_hir::TypeId>>,
    /// Local closure values available to non-escaping callback consumers.
    local_callbacks: HashMap<String, LocalCallback>,
    /// Rest-parameter metadata for top-level function declarations.
    function_rests: HashMap<String, RestParam>,
    /// Forward-visible function declaration signatures for hoisted callback calls.
    forward_function_types: HashMap<String, (smelt_hir::Symbol, smelt_hir::TypeId)>,
    /// Function item slots reserved for local hoisted declarations.
    local_function_items: HashMap<String, smelt_hir::ItemId>,
    /// TypeScript overload signatures keyed by implementation name.
    function_overloads: HashMap<String, Vec<OverloadSignature>>,
    /// Materialized final definitions for this source module.
    specialization: Option<SpecializationData>,
}

/// Synthetic list used to materialize a synchronous generator body.
#[derive(Debug, Clone, Copy)]
struct GeneratorYieldAccumulator {
    /// Local that stores yielded values in source order.
    local: smelt_hir::LocalId,
    /// HIR type of the accumulator list.
    list_ty: smelt_hir::TypeId,
    /// HIR type of each erased yielded value.
    item_ty: smelt_hir::TypeId,
}

// Lowering builder implementation split into small include files.
mod module_init;
mod new_expr;
mod guards;
mod callbacks;
mod decls;
mod expr;
mod stmt;
mod testing;
