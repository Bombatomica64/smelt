
//! Shared pure support functions for TypeScript lowering.

use super::{
    ConstItem, ConstLiteral, Declaration, ExprKind, HashMap, HashSet, Item, ModuleExportName,
    Program, PropertyKey, Statement, TSAccessibility, UnknownKind, Visibility,
};

/// The statements of an arrow function's block body.
///
/// Since oxc 0.147 an arrow's body is an `ArrowFunctionBody` enum, so a concise
/// (expression) body has no statement list at all. Callers that only handle a
/// block body get an empty slice; callers that must distinguish the two forms
/// use `ArrowFunctionExpression::get_expression` instead.
pub(super) fn arrow_block_statements<'a>(
    arrow: &'a oxc::ast::ast::ArrowFunctionExpression<'a>,
) -> &'a [Statement<'a>] {
    arrow
        .get_function_body()
        .map_or_else(|| [].as_slice(), |block| block.statements.as_slice())
}

/// Return an item's original source name when available.
pub(super) fn item_name<'a>(krate: &'a smelt_hir::Crate, item: &Item) -> Option<&'a str> {
    let symbol = match item {
        Item::Function(function) => function.name,
        Item::Class(class) => class.name,
        Item::Interface(interface) => interface.name,
        Item::TypeAlias(alias) => alias.name,
        Item::Const(const_item) => const_item.name,
        Item::MutableGlobal(global_item) => global_item.name,
    };
    krate
        .names
        .get(symbol)
        .or_else(|| krate.symbols.get(symbol))
}

/// Map a JavaScript `typeof` result string to an executable unknown tag.
pub(super) fn unknown_kind_from_typeof(kind: &str) -> Option<UnknownKind> {
    match kind {
        "boolean" => Some(UnknownKind::Bool),
        "bigint" | "number" => Some(UnknownKind::Number),
        "string" => Some(UnknownKind::String),
        "undefined" => Some(UnknownKind::Undefined),
        "symbol" => Some(UnknownKind::Symbol),
        "object" => Some(UnknownKind::Object),
        "function" => Some(UnknownKind::Function),
        _ => None,
    }
}

/// Insert an item into the visible lookup tables under a source-level name.
pub(super) fn insert_visible_item(
    items: &mut HashMap<String, smelt_hir::ItemId>,
    classes: &mut HashMap<String, smelt_hir::ItemId>,
    interfaces: &mut HashMap<String, smelt_hir::ItemId>,
    name: &str,
    item_id: smelt_hir::ItemId,
    item: &Item,
) {
    items.insert(name.to_owned(), item_id);
    match item {
        Item::Class(_) => {
            classes.insert(name.to_owned(), item_id);
        }
        Item::Interface(_) => {
            interfaces.insert(name.to_owned(), item_id);
        }
        Item::Function(_)
        | Item::TypeAlias(_)
        | Item::Const(_)
        | Item::MutableGlobal(_) => {}
    }
}

/// Return a literal value from a HIR constant item when it can be inlined safely.
pub(super) fn const_literal_from_item(
    krate: &smelt_hir::Crate,
    const_item: &ConstItem,
) -> Option<ConstLiteral> {
    let body = krate.bodies.get(usize::try_from(const_item.body.0).ok()?)?;
    let expr = body.exprs.get(usize::try_from(const_item.value.0).ok()?)?;
    let ExprKind::Literal(literal) = &expr.kind else {
        return None;
    };
    Some(ConstLiteral {
        literal: literal.clone(),
        ty: const_item.ty,
    })
}

/// Sanitize a source test title into a stable Rust identifier suffix.
pub(super) fn sanitize_test_name(name: &str) -> Option<String> {
    let mut output = String::new();
    let mut previous_underscore = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
            previous_underscore = false;
        } else if !previous_underscore && !output.is_empty() {
            output.push('_');
            previous_underscore = true;
        }
    }
    while output.ends_with('_') {
        output.pop();
    }
    (!output.is_empty()).then_some(output)
}

/// Return whether a property key can be resolved without runtime evaluation.
pub(super) fn is_static_property_key(key: &PropertyKey<'_>) -> bool {
    matches!(
        key,
        PropertyKey::StaticIdentifier(_)
            | PropertyKey::PrivateIdentifier(_)
            | PropertyKey::StringLiteral(_)
    )
}

/// Extract the source text represented by a module export name.
pub(super) fn module_export_name(name: &ModuleExportName<'_>) -> String {
    match name {
        ModuleExportName::IdentifierName(ident) => ident.name.as_str().to_owned(),
        ModuleExportName::IdentifierReference(ident) => ident.name.as_str().to_owned(),
        ModuleExportName::StringLiteral(literal) => literal.value.as_str().to_owned(),
    }
}

/// Return top-level function names that have concrete implementation bodies.
pub(super) fn implemented_function_names(program: &Program<'_>) -> HashSet<String> {
    let mut names = HashSet::new();
    for statement in &program.body {
        match statement {
            Statement::FunctionDeclaration(function) if function.body.is_some() => {
                if let Some(id) = &function.id {
                    names.insert(id.name.to_string());
                }
            }
            Statement::ExportDeclaration(export) => {
                if let Declaration::FunctionDeclaration(function) = &export.declaration
                    && function.body.is_some()
                    && let Some(id) = &function.id
                {
                    names.insert(id.name.to_string());
                }
            }
            _ => {}
        }
    }
    names
}

/// Return the only argument when a pure Math constant call is unary.
pub(super) fn single_arg(args: &[f64]) -> Option<f64> {
    let [arg] = args else {
        return None;
    };
    Some(*arg)
}

/// Return both arguments when a pure Math constant call is binary.
pub(super) fn two_args(args: &[f64]) -> Option<(f64, f64)> {
    let [lhs, rhs] = args else {
        return None;
    };
    Some((*lhs, *rhs))
}

/// Return true for a TypeScript overload signature backed by an implementation.
pub(super) fn is_implemented_overload_signature(
    function: &oxc::ast::ast::Function<'_>,
    implemented_functions: &HashSet<String>,
) -> bool {
    function.body.is_none()
        && function
            .id
            .as_ref()
            .is_some_and(|id| implemented_functions.contains(id.name.as_str()))
}

/// Determine HIR visibility from TypeScript accessibility metadata.
pub(super) fn visibility(accessibility: Option<TSAccessibility>) -> Visibility {
    match accessibility {
        Some(TSAccessibility::Private) => Visibility::Private,
        Some(TSAccessibility::Protected) => Visibility::Protected,
        Some(TSAccessibility::Public) | None => Visibility::Public,
    }
}

/// Return whether a statement always terminates control flow.
pub(super) fn statement_terminates(statement: &Statement<'_>) -> bool {
    match statement {
        Statement::ReturnStatement(_) | Statement::ThrowStatement(_) => true,
        Statement::BlockStatement(block) => block.body.iter().any(statement_terminates),
        Statement::IfStatement(if_stmt) => if_stmt.alternate.as_ref().is_some_and(|alternate| {
            statement_terminates(&if_stmt.consequent) && statement_terminates(alternate)
        }),
        // `try { return ... } catch { return ... }` terminates when the try
        // block terminates and any catch handler terminates too (a missing
        // handler propagates the exception, which also leaves the enclosing
        // statement). A terminating finalizer terminates regardless.
        Statement::TryStatement(try_stmt) => {
            let finalizer_terminates = try_stmt
                .finalizer
                .as_ref()
                .is_some_and(|finalizer| finalizer.body.iter().any(statement_terminates));
            let block_terminates = try_stmt.block.body.iter().any(statement_terminates);
            let handler_terminates = try_stmt
                .handler
                .as_ref()
                .is_none_or(|handler| handler.body.body.iter().any(statement_terminates));
            finalizer_terminates || (block_terminates && handler_terminates)
        }
        Statement::BreakStatement(_)
        | Statement::ContinueStatement(_)
        | Statement::DebuggerStatement(_)
        | Statement::DoWhileStatement(_)
        | Statement::EmptyStatement(_)
        | Statement::ExpressionStatement(_)
        | Statement::ForInStatement(_)
        | Statement::ForOfStatement(_)
        | Statement::ForStatement(_)
        | Statement::LabeledStatement(_)
        | Statement::SwitchStatement(_)
        | Statement::WhileStatement(_)
        | Statement::WithStatement(_)
        | Statement::VariableDeclaration(_)
        | Statement::FunctionDeclaration(_)
        | Statement::ClassDeclaration(_)
        | Statement::TSTypeAliasDeclaration(_)
        | Statement::TSInterfaceDeclaration(_)
        | Statement::TSEnumDeclaration(_)
        | Statement::TSNamespaceDeclaration(_)
        | Statement::TSExternalModuleDeclaration(_)
        | Statement::TSGlobalDeclaration(_)
        | Statement::TSImportEqualsDeclaration(_)
        | Statement::ImportDeclaration(_)
        | Statement::ExportAllDeclaration(_)
        | Statement::ExportDeclaration(_)
        | Statement::ExportFromDeclaration(_)
        | Statement::ExportDefaultDeclaration(_)
        | Statement::ExportNamedDeclaration(_)
        | Statement::TSExportAssignment(_)
        | Statement::TSNamespaceExportDeclaration(_) => false,
    }
}
