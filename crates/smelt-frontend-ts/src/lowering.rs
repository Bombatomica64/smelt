//! TypeScript AST lowering into Smelt HIR.

mod stdlib;
mod stdlib_dispatch;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use crate::{
    HirCtx, ObjectConst, ObjectConstEntry, ObjectConstValue, OverloadSignature, SmeltError,
    camel_to_snake, test_support,
};
use oxc::allocator::Allocator;
use oxc::ast::ast::{
    Argument, ArrayExpressionElement, AssignmentTarget, BindingPattern, ChainElement, ClassElement,
    Declaration, Expression, ForStatementInit, ForStatementLeft, ImportDeclarationSpecifier,
    ImportOrExportKind, MethodDefinitionKind, MethodDefinitionType, ModuleExportName,
    ObjectPropertyKind, Program, PropertyKey, SimpleAssignmentTarget, Statement, TSAccessibility,
    TSModuleDeclarationBody, TSModuleDeclarationName, TSSignature, TSTupleElement, TSType,
    TSTypeName, TSTypeQueryExprName,
};
use oxc::parser::{ParseOptions, Parser};
use oxc::span::{GetSpan, SourceType};
use oxc::syntax::operator::{
    AssignmentOperator, BinaryOperator, LogicalOperator, UnaryOperator, UpdateOperator,
};
use smelt_hir::{
    AsyncOp, BinOp, Body, CallbackCallArg, CallbackExpr, CallbackExprKind, CaptureMode, Class,
    ClosureCapture, ConstItem, DatePart, DictProjectionOp, Expr, ExprKind, Field, FileId, Function,
    FunctionOwner, FunctionType, Import, Interface, Item, Language, ListCallbackOp, ListSearchOp,
    Literal, LocalDecl, MatchArm, MethodSig, Module, ModuleId, NumericExtremaOp,
    NumericPredicateOp, NumericRoundOp, NumericUnaryFuncOp, Param, ParamSig, Pattern,
    PrimitiveCastOp, SetProjectionOp, SetRemoveOp, SourceFile, Span, Stmt, StringAffixOp,
    StringCaseOp, StringNormalizeForm, StringPadOp, StringReplaceOp, StringSearchOp,
    StringTrimSide, Type, TypeParamDef, UnaryOp, UnknownKind, UrlField, Visibility,
};
use smelt_stdlib::RuleId;

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
}

/// Constant expression exported from another TypeScript module.
#[derive(Debug, Clone)]
struct ConstLiteral {
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
struct RestParam {
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
    to_hir_with_path(source, file_id, "<memory>", ctx)
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

    if !parsed.errors.is_empty() {
        return Err(parsed
            .errors
            .into_iter()
            .map(|error| {
                SmeltError::parse(
                    Span::new(file_id, 0, u32::try_from(source.len()).unwrap_or(u32::MAX)),
                    error.to_string(),
                )
            })
            .collect());
    }

    let mut builder = ModuleBuilder::new(file_id, path.to_owned(), source.to_owned(), ctx);
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
    /// Interface call signatures for callable interface types.
    interface_call_signatures: HashMap<smelt_hir::Symbol, Vec<FunctionType>>,
    /// Fields attached to callable intersection types.
    callable_fields: HashMap<smelt_hir::TypeId, Vec<Field>>,
    /// Namespace path currently qualifying type-only declarations.
    type_namespace_prefix: Vec<String>,
    /// Currently processing class name, if any.
    current_class: Option<String>,
    /// Whether the current lowered function body is async.
    current_async: bool,
    /// Declared return type for the current lowered function body.
    current_return_ty: Option<smelt_hir::TypeId>,
    /// HIR block that owns side-effect statements emitted while lowering an expression.
    current_statement_block: Option<smelt_hir::BlockId>,
    /// Whether type-test-only lowering may index erased unknown metadata.
    allow_unknown_index_access: bool,
    /// Test-framework API names imported from Vitest-compatible modules.
    test_builtins: HashSet<String>,
    /// Local names bound by namespace imports such as `import * as MathApi from "./math"`.
    namespace_imports: HashSet<String>,
    /// Local names imported only for TypeScript type positions.
    type_only_imports: HashSet<String>,
    /// Local names imported as runtime values.
    value_imports: HashSet<String>,
    /// Object constants that act as namespace-like API surfaces.
    object_namespaces: HashMap<String, HashMap<String, smelt_hir::ItemId>>,
    /// Literal constant items visible from already-lowered modules.
    const_literals: HashMap<String, ConstLiteral>,
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
}

// Lowering builder implementation split into small include files.
include!("lowering/builder_part01.rs");
include!("lowering/builder_part02.rs");
include!("lowering/builder_part03.rs");
include!("lowering/builder_part04.rs");
include!("lowering/builder_part05.rs");
include!("lowering/builder_part06.rs");
include!("lowering/builder_part07.rs");
include!("lowering/builder_part08.rs");
include!("lowering/call.rs");
include!("lowering/builder_part09.rs");
include!("lowering/builder_part10.rs");
include!("lowering/builder_part11.rs");
include!("lowering/builder_part12.rs");
include!("lowering/builder_part13.rs");
include!("lowering/builder_part14.rs");
include!("lowering/builder_part15.rs");
include!("lowering/builder_part16.rs");
include!("lowering/builder_part17.rs");
include!("lowering/builder_part18.rs");

// Top-level lowering helper functions split into include files.
include!("lowering/helpers_part01.rs");
