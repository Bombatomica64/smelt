//! Astral `ty`-backed Python type resolution for Smelt's Python frontend.
//!
//! # Why this crate exists
//!
//! Smelt's Python frontend (`smelt-frontend-py`) uses Ruff only as a *parser*
//! and, historically, required every function/method/parameter to carry an
//! explicit type annotation — otherwise it emitted a hard
//! `must have an explicit ... annotation` error. Idiomatic Python omits most of
//! those annotations, so that policy blocked nearly every real library before
//! any lowering could happen (see issue #93).
//!
//! Rather than build a bespoke Python inference engine, this crate embeds
//! **`ty`** (Astral's Rust type checker, formerly red-knot) as a *library* and
//! consumes the types it resolves. `ty` already understands call-result types,
//! unions/narrowing, generics, and — crucially — types that live in the
//! **vendored typeshed stubs** rather than user source. The feasibility of
//! embedding it was proven by `crates/smelt-py-ty-spike`; this crate
//! productionizes that spike into a small, stable query surface.
//!
//! # The stable query surface
//!
//! `ty`'s internal `Type` lattice is large and almost entirely `pub(crate)` —
//! there is no public way to walk a function's inferred signature struct. The
//! one stable, public capability `ty` exposes is
//! [`ty_python_semantic::HasType::inferred_type`] on AST nodes, plus
//! `Type::display(db)`, which renders a resolved type in **canonical Python
//! spelling** (`int`, `list[int]`, `int | float`, `str | None`, …). That
//! spelling is exactly the grammar the frontend's existing
//! `annotation_to_hir` already parses. So this crate's contract is
//! deliberately narrow: it hands the frontend *type strings*, not `ty`'s
//! internal types, insulating the frontend (and the rest of the workspace)
//! from `ty`'s unstable internal API.
//!
//! # Parameter types (a note on limits)
//!
//! `ty` resolves a parameter's type from its *annotation* (or a resolvable
//! binding); it reports an **unannotated** parameter as dynamic/`Unknown` — it
//! does not infer a parameter type from a default value or from call sites.
//! Such dynamic results are dropped by [`displayable_type`], so a genuinely
//! unannotated parameter stays an explicit boundary and the frontend keeps
//! requiring an annotation for it. The productive win from `ty` is therefore
//! **return-type resolution**, which is the dominant blocker family; broader
//! parameter inference is a deferred follow-up (see issue #93).
//!
//! # Return type: actual vs. declared
//!
//! Issue #93 stresses that a function's *actual returned* type may differ from
//! its declared annotation, and that lowering should follow the accurate type.
//! This crate therefore derives a function's return type from the inferred
//! types of its `return <expr>` statements (unioned across all return sites),
//! not from the annotation. A function with no value-returning `return` is
//! reported as `None`.
//!
//! # Batch model
//!
//! Smelt is a batch compiler; `ty` is incremental. The simplest correct
//! integration — and the one used here — is a fresh `ProjectDatabase` per
//! resolved module, mirroring the spike. The source is materialized into a
//! temporary directory and checked through an `OsSystem`, the exact path the
//! spike proved works in this environment. Reuse/caching is a later
//! optimization, not a correctness concern.

#![expect(
    clippy::wildcard_enum_match_arm,
    reason = "Ruff AST statement matches group all non-block variants in one fallback arm"
)]

use std::collections::HashMap;

use anyhow::{Context, Result, anyhow};
use ruff_db::files::system_path_to_file;
use ruff_db::parsed::parsed_module;
use ruff_db::system::{OsSystem, SystemPathBuf};
use ruff_python_ast::{Stmt, StmtFunctionDef};
use ruff_text_size::{Ranged, TextSize};
use ty_project::{ProjectDatabase, ProjectMetadata};
use ty_python_semantic::types::Type;
use ty_python_semantic::{HasType, SemanticModel};

/// Types resolved by `ty` for one Python module, keyed by AST byte offset.
///
/// Keys are the source byte offset of the *start* of the relevant AST node —
/// a `StmtFunctionDef` for a return type, or a `Parameter` for a parameter
/// type. Byte offsets are stable and unique within a parse, so the frontend
/// can look up a node's resolved type using the same Ruff AST it already
/// walks (via `ruff_text_size::Ranged::start`), without name-based matching
/// that would break on nesting, overloads, or shadowing.
///
/// Every stored value is a **canonical Python type string** as produced by
/// `ty`'s `Type::display` (e.g. `"int"`, `"list[int]"`, `"str | None"`). The
/// frontend feeds these back through its existing annotation parser.
#[derive(Debug, Clone, Default)]
pub struct ResolvedModuleTypes {
    /// Function-definition start offset → resolved return type spelling.
    return_types: HashMap<u32, String>,
    /// Parameter start offset → resolved parameter type spelling.
    param_types: HashMap<u32, String>,
}

impl ResolvedModuleTypes {
    /// Return the resolved return-type spelling for the function whose
    /// definition starts at `offset`, if `ty` resolved one.
    #[must_use]
    pub fn return_type_at(&self, offset: TextSize) -> Option<&str> {
        self.return_types.get(&offset.to_u32()).map(String::as_str)
    }

    /// Return the resolved type spelling for the parameter whose declaration
    /// starts at `offset`, if `ty` resolved one.
    #[must_use]
    pub fn param_type_at(&self, offset: TextSize) -> Option<&str> {
        self.param_types.get(&offset.to_u32()).map(String::as_str)
    }

    /// Number of resolved return types (used by tests / diagnostics).
    #[must_use]
    pub fn resolved_return_count(&self) -> usize {
        self.return_types.len()
    }

    /// Number of resolved parameter types (used by tests / diagnostics).
    #[must_use]
    pub fn resolved_param_count(&self) -> usize {
        self.param_types.len()
    }
}

/// Resolve, via `ty`, the return and parameter types of every function/method
/// in `source`.
///
/// `module_stem` is the file stem used for the temporary `.py` file (e.g.
/// `"main"`); it only affects `ty`'s module naming, not the result keys.
///
/// The source is written into a fresh temporary directory and checked with a
/// `ty` `ProjectDatabase` over an `OsSystem`. A resolved type is recorded only
/// when `ty` produces a *representable, concrete* spelling — dynamic/unknown
/// results (`Unknown`, `Any`, `Never`, unresolved imports) are intentionally
/// dropped so the frontend keeps them as an explicit boundary rather than
/// silently widening to a catch-all.
///
/// # Errors
/// Returns an error if the temporary project cannot be created or `ty` cannot
/// open the file. A `ty` *checker* error on the file is not fatal: it yields an
/// empty result so the caller falls back to annotation-based lowering.
pub fn resolve_module_types(source: &str, module_stem: &str) -> Result<ResolvedModuleTypes> {
    let dir = unique_temp_dir();
    std::fs::create_dir_all(&dir).context("create ty resolution project dir")?;
    // Clean up defensively even if a later step fails; ignore removal errors.
    // Constructed before any fallible step so the directory is always removed.
    let guard = TempDirGuard(dir.clone());

    let stem = if module_stem.is_empty() {
        "smelt_module"
    } else {
        module_stem
    };
    let file_path = dir.join(format!("{stem}.py"));
    std::fs::write(&file_path, source).context("write ty resolution source")?;

    let root = SystemPathBuf::from_path_buf(dir)
        .map_err(|original| anyhow!("non-UTF-8 temp dir path: {}", original.display()))?;
    let system = OsSystem::new(&root);
    let metadata =
        ProjectMetadata::discover(&root, &system).context("discover ty project metadata")?;
    let db = ProjectDatabase::use_defaults(metadata, system);

    let file_system_path = SystemPathBuf::from_path_buf(file_path)
        .map_err(|original| anyhow!("non-UTF-8 source path: {}", original.display()))?;
    let file = system_path_to_file(&db, &file_system_path).context("resolve file in ty db")?;

    let model = SemanticModel::new(&db, file);
    let parsed = parsed_module(&db, file).load(&db);

    let mut collector = TypeCollector {
        db: &db,
        model: &model,
        resolved: ResolvedModuleTypes::default(),
    };
    collector.collect_stmts(&parsed.syntax().body);

    let resolved = collector.resolved;
    drop(guard);
    Ok(resolved)
}

/// Best-effort temporary directory removal on drop.
struct TempDirGuard(std::path::PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        // Best-effort cleanup; a leftover temp dir is harmless.
        drop(std::fs::remove_dir_all(&self.0));
    }
}

/// Build a unique temporary directory path for one resolution.
///
/// The process id plus a monotonic counter keeps concurrent resolutions (and
/// repeated runs in the same process) from colliding.
fn unique_temp_dir() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    std::env::temp_dir().join(format!("smelt-ty-resolve-{pid}-{seq}"))
}

/// Walks a module's statements, recording `ty`-resolved return/parameter types.
///
/// Nested functions and class methods are visited too, so closures/methods
/// defined inside other definitions are also resolved; but a function's return
/// type is derived only from the `return` statements in *its own* body, not
/// those of nested functions (see [`FunctionReturnVisitor`]).
struct TypeCollector<'a, 'db> {
    /// `ty` project database used to display resolved types.
    db: &'db ProjectDatabase,
    /// `ty` semantic model used to query inferred node types.
    model: &'a SemanticModel<'db>,
    /// Accumulated resolved return/parameter type spellings.
    resolved: ResolvedModuleTypes,
}

impl TypeCollector<'_, '_> {
    /// Record every function/method reachable from `stmts`, descending into
    /// nested function, class, and control-flow blocks.
    fn collect_stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            if let Stmt::FunctionDef(func) = stmt {
                self.record_function(func);
            }
            // Recurse into every block child so nested defs/methods are found.
            self.collect_stmts_of(stmt);
        }
    }

    /// Recurse into the block children of one statement, including the bodies
    /// of nested function/class definitions (unlike [`child_stmts`], which is
    /// used for a single function's own-return search).
    fn collect_stmts_of(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::FunctionDef(func) => self.collect_stmts(&func.body),
            Stmt::ClassDef(class) => self.collect_stmts(&class.body),
            other => {
                for child in child_stmts(other) {
                    if let Stmt::FunctionDef(func) = child {
                        self.record_function(func);
                    }
                    self.collect_stmts_of(child);
                }
            }
        }
    }

    /// Record `ty`'s resolved parameter and return types for one function.
    fn record_function(&mut self, func: &StmtFunctionDef) {
        // Parameters (including `*args` / `**kwargs`): use ty's binding type.
        // `AnyParameterRef::as_parameter` yields the underlying `Parameter` node
        // for both variadic and non-variadic parameters, and that node's start
        // offset is the key the frontend uses to look the type back up.
        for param in &func.parameters {
            let parameter = param.as_parameter();
            if let Some(ty) = parameter.inferred_type(self.model)
                && let Some(spelling) = displayable_type(self.db, ty)
            {
                self.resolved
                    .param_types
                    .insert(parameter.start().to_u32(), spelling);
            }
        }

        // Return type: union of the inferred types of this body's own
        // value-returning `return` statements. No such statement => `None`.
        let mut returns = FunctionReturnVisitor {
            db: self.db,
            model: self.model,
            spellings: Vec::new(),
        };
        for statement in &func.body {
            returns.visit_body_stmt(statement);
        }
        if let Some(spelling) = returns.into_return_spelling() {
            self.resolved
                .return_types
                .insert(func.start().to_u32(), spelling);
        }
    }
}

/// Collects the inferred spellings of the value-returning `return` statements
/// belonging to a single function body.
///
/// It deliberately does **not** descend into nested function or lambda bodies:
/// those returns belong to the inner callable, which is recorded separately by
/// the enclosing [`TypeCollector`].
struct FunctionReturnVisitor<'a, 'db> {
    /// `ty` project database used to display resolved types.
    db: &'db ProjectDatabase,
    /// `ty` semantic model used to query inferred expression types.
    model: &'a SemanticModel<'db>,
    /// Canonical spellings gathered from each `return <expr>` site, in order.
    spellings: Vec<String>,
}

impl<'db> FunctionReturnVisitor<'_, 'db> {
    /// Visit one statement of the owning function body without crossing into a
    /// nested function/class scope.
    fn visit_body_stmt(&mut self, stmt: &'db Stmt) {
        match stmt {
            // Do not descend into nested callables: their `return`s are not ours.
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {}
            Stmt::Return(ret) => {
                if let Some(value) = ret.value.as_deref() {
                    if let Some(ty) = value.inferred_type(self.model)
                        && let Some(spelling) = displayable_type(self.db, ty)
                    {
                        self.spellings.push(spelling);
                    } else {
                        // A return whose value type we cannot represent makes the
                        // whole return type unresolved; mark it and stop trusting
                        // a partial union.
                        self.spellings.push(UNRESOLVED_MARKER.to_owned());
                    }
                } else {
                    self.spellings.push("None".to_owned());
                }
            }
            other => self.walk_children(other),
        }
    }

    /// Recurse into the control-flow children of `stmt` (if/for/while/with/try/
    /// match blocks) so nested `return`s are found, still without entering a
    /// nested function scope.
    fn walk_children(&mut self, stmt: &'db Stmt) {
        for child in child_stmts(stmt) {
            self.visit_body_stmt(child);
        }
    }

    /// Fold the collected return spellings into one canonical return type, or
    /// `None` when the function has no value-returning `return` (a bare `pass`
    /// / fall-through function), or when any return was unrepresentable.
    fn into_return_spelling(self) -> Option<String> {
        if self.spellings.is_empty() {
            // No `return <expr>` at all — the function returns `None`.
            return Some("None".to_owned());
        }
        if self.spellings.iter().any(|s| s == UNRESOLVED_MARKER) {
            return None;
        }
        let mut unique: Vec<String> = Vec::new();
        for spelling in self.spellings {
            if !unique.contains(&spelling) {
                unique.push(spelling);
            }
        }
        if unique.len() == 1 {
            return unique.into_iter().next();
        }
        // Multiple distinct return types → a PEP 604 union spelling, which the
        // frontend's `annotation_to_hir` already understands.
        Some(unique.join(" | "))
    }
}

/// Sentinel pushed for a `return` whose value type could not be represented.
const UNRESOLVED_MARKER: &str = "\u{0}unresolved";

/// Return the direct control-flow child statements of `stmt`, excluding the
/// bodies of nested function/class definitions.
///
/// This is a small, explicit traversal (rather than the generic AST visitor)
/// so it is obvious that lambda/def scopes are never crossed while searching
/// for a function's own `return` statements.
fn child_stmts(stmt: &Stmt) -> Vec<&Stmt> {
    let mut out: Vec<&Stmt> = Vec::new();
    match stmt {
        Stmt::If(node) => {
            out.extend(&node.body);
            for clause in &node.elif_else_clauses {
                out.extend(&clause.body);
            }
        }
        Stmt::For(node) => {
            out.extend(&node.body);
            out.extend(&node.orelse);
        }
        Stmt::While(node) => {
            out.extend(&node.body);
            out.extend(&node.orelse);
        }
        Stmt::With(node) => out.extend(&node.body),
        Stmt::Try(node) => {
            out.extend(&node.body);
            for handler in &node.handlers {
                let ruff_python_ast::ExceptHandler::ExceptHandler(except) = handler;
                out.extend(&except.body);
            }
            out.extend(&node.orelse);
            out.extend(&node.finalbody);
        }
        Stmt::Match(node) => {
            for case in &node.cases {
                out.extend(&case.body);
            }
        }
        _ => {}
    }
    out
}

/// Render `ty`'s resolved `Type` to a canonical Python type spelling, or
/// `None` if the type is dynamic/unrepresentable and should be left as an
/// explicit frontend boundary.
///
/// Dynamic (`Unknown`/`Any`), `Never`, and other non-value forms are dropped
/// on purpose: per Smelt's `SmeltUnknown` philosophy, unresolved cases must stay
/// an explicit boundary rather than be routed through a catch-all. Everything
/// else is displayed and lightly normalized into the annotation subset the
/// frontend understands.
fn displayable_type(db: &ProjectDatabase, ty: Type<'_>) -> Option<String> {
    // Drop dynamic / bottom types outright: these are genuine boundaries.
    if matches!(ty, Type::Dynamic(_) | Type::Never | Type::Divergent(_)) {
        return None;
    }
    let spelling = ty.display(db).to_string();
    normalize_spelling(&spelling)
}

/// Normalize a `ty` display string into the type-annotation subset the Python
/// frontend can lower, or reject it (returning `None`) when it clearly falls
/// outside that subset.
///
/// `ty` displays a *value's* narrow type using literal spellings such as
/// `Literal[3]`, `Literal["hi"]`, or `bool`-narrowed `Literal[True]`. Smelt's
/// frontend widens those to the underlying primitive, so we do too. Anything
/// containing `ty`-internal markers (`Unknown`, `@Todo`, `<...>` special
/// forms) is treated as unresolved.
fn normalize_spelling(spelling: &str) -> Option<String> {
    let trimmed = spelling.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Reject clearly-dynamic or ty-internal renderings.
    if trimmed.contains("Unknown")
        || trimmed.contains("@Todo")
        || trimmed.contains('<')
        || trimmed.contains("Any")
    {
        return None;
    }
    // Widen literal spellings to their underlying primitive.
    if let Some(widened) = widen_literal(trimmed) {
        return Some(widened);
    }
    Some(trimmed.to_owned())
}

/// Widen a `Literal[...]` (or `bool` literal) spelling to its primitive type.
///
/// `ty` reports the narrowest type it can prove (`Literal[3]` for the constant
/// `3`); the frontend's HIR carries primitive types (`int`), so a literal
/// spelling is widened to the matching primitive. Returns `None` when the
/// spelling is not a recognized literal form, so the caller keeps it as-is.
fn widen_literal(spelling: &str) -> Option<String> {
    let inner = spelling.strip_prefix("Literal[")?.strip_suffix(']')?;
    // A `Literal[...]` may pack several members (`Literal[1, 2]`); every member
    // of one literal set shares a base type in practice for our inputs, so key
    // off the first member's shape.
    let first = inner.split(',').next()?.trim();
    let base = if first == "True" || first == "False" {
        "bool"
    } else if first.starts_with("b\"") || first.starts_with("b'") {
        // bytes literal — outside the lowering subset.
        return None;
    } else if first.starts_with('"') || first.starts_with('\'') {
        "str"
    } else if first.parse::<i128>().is_ok() {
        "int"
    } else if first.parse::<f64>().is_ok() {
        "float"
    } else {
        // Enum members and other exotic literals: leave unresolved.
        return None;
    };
    Some(base.to_owned())
}

#[cfg(test)]
#[expect(
    clippy::panic_in_result_fn,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests assert against fixed source and index known-good parse results"
)]
mod tests {
    use super::*;

    #[test]
    fn resolves_return_from_body_not_annotation() -> Result<()> {
        // No return annotation: ty infers `int` from `x + 1`.
        let resolved = resolve_module_types("def inc(x: int):\n    return x + 1\n", "m")?;
        let parsed = ruff_python_parser::parse_module("def inc(x: int):\n    return x + 1\n")?;
        let func = parsed.syntax().body[0].as_function_def_stmt().unwrap();
        assert_eq!(resolved.return_type_at(func.start()), Some("int"));
        Ok(())
    }

    #[test]
    fn resolves_none_for_valueless_function() -> Result<()> {
        let resolved = resolve_module_types("def noop():\n    pass\n", "m")?;
        let parsed = ruff_python_parser::parse_module("def noop():\n    pass\n")?;
        let func = parsed.syntax().body[0].as_function_def_stmt().unwrap();
        assert_eq!(resolved.return_type_at(func.start()), Some("None"));
        Ok(())
    }

    #[test]
    fn resolves_parameter_type() -> Result<()> {
        let src = "def f(x: int, y: str):\n    return x\n";
        let resolved = resolve_module_types(src, "m")?;
        let parsed = ruff_python_parser::parse_module(src)?;
        let func = parsed.syntax().body[0].as_function_def_stmt().unwrap();
        let x = func.parameters.iter().next().unwrap().as_parameter();
        assert_eq!(resolved.param_type_at(x.start()), Some("int"));
        Ok(())
    }

    #[test]
    fn widens_literals() {
        assert_eq!(widen_literal("Literal[3]").as_deref(), Some("int"));
        assert_eq!(widen_literal("Literal[\"hi\"]").as_deref(), Some("str"));
        assert_eq!(widen_literal("Literal[True]").as_deref(), Some("bool"));
        assert_eq!(widen_literal("int"), None);
    }

    #[test]
    fn rejects_dynamic_spellings() {
        assert_eq!(normalize_spelling("Unknown"), None);
        assert_eq!(normalize_spelling("int | Unknown"), None);
        assert_eq!(normalize_spelling("int"), Some("int".to_owned()));
        assert_eq!(normalize_spelling("str | None"), Some("str | None".to_owned()));
    }
}
