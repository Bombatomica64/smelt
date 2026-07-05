/// Collect items already present in the shared crate for cross-module references.
fn visible_items(ctx: &HirCtx) -> HashMap<String, ItemId> {
    let mut items = HashMap::new();
    for (idx, item) in ctx.krate.items.iter().enumerate() {
        let Ok(item_idx) = u32::try_from(idx) else {
            continue;
        };
        let item_id = ItemId(item_idx);
        let Some(name) = item_name(&ctx.krate, item) else {
            continue;
        };
        items.insert(name.to_owned(), item_id);
    }
    items
}

/// Collect class method items already present in the shared crate.
fn visible_class_methods(ctx: &HirCtx) -> HashMap<String, HashMap<String, ItemId>> {
    let mut class_methods: HashMap<String, HashMap<String, ItemId>> = HashMap::new();
    for (idx, item) in ctx.krate.items.iter().enumerate() {
        let Item::Function(function) = item else {
            continue;
        };
        let FunctionOwner::ClassMethod { class, method } = function.owner else {
            continue;
        };
        let Ok(item_idx) = u32::try_from(idx) else {
            continue;
        };
        let Some(class_name) = ctx.krate.symbols.get(class) else {
            continue;
        };
        let Some(method_name) = ctx.krate.symbols.get(method) else {
            continue;
        };
        class_methods
            .entry(class_name.to_owned())
            .or_default()
            .insert(method_name.to_owned(), ItemId(item_idx));
    }
    class_methods
}

/// Collect class `@staticmethod` items already present in the shared crate.
fn visible_class_static_methods(ctx: &HirCtx) -> HashMap<String, HashMap<String, ItemId>> {
    let mut class_static_methods: HashMap<String, HashMap<String, ItemId>> = HashMap::new();
    for (idx, item) in ctx.krate.items.iter().enumerate() {
        let Item::Function(function) = item else {
            continue;
        };
        let FunctionOwner::ClassStaticMethod { class, method } = function.owner else {
            continue;
        };
        let Ok(item_idx) = u32::try_from(idx) else {
            continue;
        };
        let Some(class_name) = ctx.krate.symbols.get(class) else {
            continue;
        };
        let Some(method_name) = ctx.krate.symbols.get(method) else {
            continue;
        };
        class_static_methods
            .entry(class_name.to_owned())
            .or_default()
            .insert(method_name.to_owned(), ItemId(item_idx));
    }
    class_static_methods
}

/// Return true for module-level `__all__` metadata assignments.
fn is_module_all_assignment(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Assign(assign) => {
            let [target] = assign.targets.as_slice() else {
                return false;
            };
            matches!(target, Expr::Name(name) if name.id.as_str() == "__all__")
        }
        Stmt::AnnAssign(assign) => {
            matches!(assign.target.as_ref(), Expr::Name(name) if name.id.as_str() == "__all__")
        }
        _ => false,
    }
}

/// Return whether a raise statement is the simple `StopIteration` sentinel form.
fn is_stop_iteration_raise(raise_stmt: &ruff_python_ast::StmtRaise) -> bool {
    match raise_stmt.exc.as_deref() {
        Some(Expr::Name(name)) => name.id.as_str() == "StopIteration",
        Some(Expr::Call(call)) => {
            matches!(call.func.as_ref(), Expr::Name(name) if name.id.as_str() == "StopIteration")
        }
        _ => false,
    }
}

/// Derive a stable Python module name from a source path.
fn module_name_from_path(path: &str) -> String {
    let source_path = Path::new(path);
    if source_path.file_name().and_then(|name| name.to_str()) == Some("__init__.py")
        && let Some(parent) = source_path.parent().and_then(Path::file_name)
    {
        return parent.to_string_lossy().into_owned();
    }
    source_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| "__main__".to_owned())
}

/// Derive all import keys that can refer to a Python source path.
fn module_names_from_path(path: &str) -> Vec<String> {
    let source_path = Path::new(path);
    let mut names = Vec::new();
    if source_path.file_name().and_then(|name| name.to_str()) == Some("__init__.py") {
        if let Some(package) = package_name_from_path(path) {
            names.push(package);
        }
    } else if let Some(dotted) = dotted_module_name_from_path(path) {
        names.push(dotted);
    }
    if let Some(stem) = source_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|stem| !stem.is_empty())
        && !names.iter().any(|name| name == &stem)
    {
        names.push(stem);
    }
    if names.is_empty() {
        names.push("__main__".to_owned());
    }
    names
}

/// Return the package key for a source path when it is inside a package.
fn package_name_from_path(path: &str) -> Option<String> {
    let source_path = Path::new(path);
    if source_path.file_name().and_then(|name| name.to_str()) == Some("__init__.py") {
        return source_path
            .parent()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned());
    }
    source_path
        .parent()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
}

/// Return a dotted `package.module` key for package member files.
fn dotted_module_name_from_path(path: &str) -> Option<String> {
    let source_path = Path::new(path);
    let package = source_path.parent().and_then(Path::file_name)?;
    let stem = source_path.file_stem()?;
    Some(format!(
        "{}.{}",
        package.to_string_lossy(),
        stem.to_string_lossy()
    ))
}

/// Return the base class name for plain and parameterized base expressions.
fn base_class_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Subscript(sub) => expr_simple_name(&sub.value),
        _ => expr_simple_name(expr),
    }
}

/// Return an item's source name when available.
fn item_name<'a>(krate: &'a smelt_hir::Crate, hir_item: &Item) -> Option<&'a str> {
    let symbol = match hir_item {
        Item::Function(function) => function.name,
        Item::Class(class) => class.name,
        Item::Interface(interface) => interface.name,
        Item::TypeAlias(alias) => alias.name,
        Item::Const(const_item) => const_item.name,
    };
    krate.symbols.get(symbol)
}
