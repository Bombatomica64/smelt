//! TypeScript AST lowering into Smelt HIR.

mod stdlib;
mod stdlib_dispatch;
use std::collections::{HashMap, HashSet};

use crate::{HirCtx, SmeltError, camel_to_snake, test_support};
use oxc::allocator::Allocator;
use oxc::ast::ast::{
    Argument, ArrayExpressionElement, AssignmentTarget, BindingPattern, ChainElement, ClassElement,
    Declaration, Expression, ForStatementInit, ForStatementLeft, ImportDeclarationSpecifier,
    ImportOrExportKind, MethodDefinitionKind, ModuleExportName, ObjectPropertyKind, Program,
    PropertyKey, SimpleAssignmentTarget, Statement, TSAccessibility, TSSignature, TSTupleElement,
    TSType, TSTypeName,
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
    StringCaseOp, StringPadOp, StringReplaceOp, StringSearchOp, StringTrimSide, Type, TypeParamDef,
    UnaryOp, UnknownKind, UrlField, Visibility,
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

/// Literal value exported from another TypeScript module.
#[derive(Debug, Clone)]
struct ConstLiteral {
    /// Literal expression to inline at import use sites.
    literal: Literal,
    /// HIR type of the literal.
    ty: smelt_hir::TypeId,
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
    defaults: Vec<Option<smelt_hir::ExprId>>,
    /// Rest parameter metadata when the source closure uses `...args`.
    rest: Option<RestParam>,
    /// Return type produced by the callback.
    return_ty: smelt_hir::TypeId,
}

/// A JavaScript/TypeScript rest parameter represented as one list argument.
#[derive(Debug, Clone, Copy)]
struct RestParam {
    /// Parameter index of the packed rest list in the lowered closure.
    index: usize,
    /// Element type accepted by each extra source-language argument.
    item_ty: smelt_hir::TypeId,
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
    let allocator = Allocator::default();
    let source_type = SourceType::default().with_typescript(true);
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

    let mut builder = ModuleBuilder::new(file_id, path.to_owned(), ctx);
    builder.program(&parsed.program)
}

/// Builder for lowering TypeScript module to HIR.
///
/// Accumulates scoping information, items, and local variables during module construction.
struct ModuleBuilder<'ctx> {
    /// File ID for error reporting.
    file_id: FileId,
    /// Source path for module metadata.
    path: String,
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
    /// Interface definitions by name.
    interfaces: HashMap<String, smelt_hir::ItemId>,
    /// Fields for each class.
    class_fields: HashMap<String, Vec<Field>>,
    /// Currently processing class name, if any.
    current_class: Option<String>,
    /// Whether the current lowered function body is async.
    current_async: bool,
    /// Test-framework API names imported from Vitest-compatible modules.
    test_builtins: HashSet<String>,
    /// Local names bound by namespace imports such as `import * as MathApi from "./math"`.
    namespace_imports: HashSet<String>,
    /// Local names imported only for TypeScript type positions.
    type_only_imports: HashSet<String>,
    /// Object constants that act as namespace-like API surfaces.
    object_namespaces: HashMap<String, HashMap<String, smelt_hir::ItemId>>,
    /// Literal constant items visible from already-lowered modules.
    const_literals: HashMap<String, ConstLiteral>,
    /// User assertion functions declared with `asserts value is T`.
    assertion_functions: HashMap<String, AssertionNarrowing>,
    /// Active local narrowings from guards and assertion calls.
    narrowed_locals: Vec<HashMap<String, smelt_hir::TypeId>>,
    /// Active generic type parameter scopes.
    type_param_scopes: Vec<HashMap<String, smelt_hir::TypeId>>,
    /// Local closure values available to non-escaping callback consumers.
    local_callbacks: HashMap<String, LocalCallback>,
    /// Rest-parameter metadata for top-level function declarations.
    function_rests: HashMap<String, RestParam>,
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
