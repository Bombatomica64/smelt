//! Spike: evaluate Astral's `ty` as a Python type source for Smelt.
//!
//! Smelt's Python frontend (`smelt-frontend-py`) currently uses ruff *only as a
//! parser* and recovers types from source annotations + its own lowering. Real
//! Python type inference (call-result types, narrowed unions, generics, stdlib
//! stubs) is deferred. This binary checks whether `ty`'s semantic engine can be
//! embedded as a library to supply that information.
//!
//! It demonstrates the two capabilities Smelt would need:
//!   1. **Diagnostics** — run the checker over a file (`check_file`).
//!   2. **Inferred types** — query the type of arbitrary expressions through the
//!      `SemanticModel` (`HasType::inferred_type`), including types that come
//!      from bundled typeshed stubs rather than source annotations.
//!
//! Run with: `cargo run -p smelt-py-ty-spike`

use anyhow::{Context, Result, anyhow};
use ruff_db::files::system_path_to_file;
use ruff_db::parsed::parsed_module;
use ruff_db::system::{OsSystem, SystemPathBuf};
use ruff_python_ast::{Expr, Stmt};
use ty_project::{ProjectDatabase, ProjectMetadata};
use ty_python_semantic::{HasType, SemanticModel, check_file};

/// Python that mixes annotated bindings with values whose types must be
/// *inferred* (literals, arithmetic, a stdlib call returning a stub type, and a
/// deliberate type error to exercise diagnostics).
const SAMPLE_PY: &str = r#"
import math

count: int = 3
label = "hello"
ratio = count / 2
root = math.sqrt(count)
mixed = [1, 2, 3]

bad: int = "not an int"
"#;

fn main() -> Result<()> {
    // With two args, check a real project: <project-root> <file-relative-to-root>.
    // With none, fall back to the built-in SAMPLE_PY one-file project.
    let mut args = std::env::args().skip(1);
    let (dir, sample) = match (args.next(), args.next()) {
        // `<root> <file>`: check one file. `<root>` alone: scan the whole tree.
        (Some(root), maybe_rel) => {
            let root = std::path::PathBuf::from(root);
            let target = maybe_rel.map_or_else(|| root.clone(), |rel| root.join(rel));
            (root, target)
        }
        _ => {
            let dir = std::env::temp_dir().join("smelt-ty-spike");
            std::fs::create_dir_all(&dir).context("create spike project dir")?;
            let sample = dir.join("sample.py");
            std::fs::write(&sample, SAMPLE_PY).context("write sample.py")?;
            (dir, sample)
        }
    };

    let root = to_system_path(dir)?;
    let system = OsSystem::new(&root);
    let metadata =
        ProjectMetadata::discover(&root, &system).context("discover ty project metadata")?;
    let db = ProjectDatabase::use_defaults(metadata, system);

    // Directory mode: scan every `.py` file under the sample's directory and
    // aggregate diagnostics by ty rule id, so genuine type errors can be told
    // apart from environment noise (e.g. `unresolved-import` when third-party
    // dependencies are not installed).
    if sample.is_dir() {
        return scan_directory(&db, &sample);
    }

    let sample_path = to_system_path(sample)?;
    let file = system_path_to_file(&db, &sample_path).context("resolve file in ty db")?;

    println!("== ty diagnostics ==");
    match check_file(&db, file) {
        Ok(diagnostics) if diagnostics.is_empty() => println!("(none — file type-checks clean)"),
        Ok(diagnostics) => {
            println!("{} diagnostic(s):", diagnostics.len());
            for diagnostic in diagnostics.iter().take(12) {
                println!("- {}", diagnostic.primary_message());
            }
            if diagnostics.len() > 12 {
                println!("  … {} more", diagnostics.len() - 12);
            }
        }
        Err(fatal) => println!("checker error: {}", fatal.primary_message()),
    }

    println!("\n== inferred types (top-level bindings) ==");
    let model = SemanticModel::new(&db, file);
    let parsed = parsed_module(&db, file).load(&db);
    for statement in &parsed.syntax().body {
        match statement {
            Stmt::Assign(assign) => {
                let names: Vec<String> = assign.targets.iter().filter_map(name_of).collect();
                if let Some(ty) = assign.value.inferred_type(&model) {
                    println!("{} : {}", names.join(", "), ty.display(&db));
                }
            }
            Stmt::AnnAssign(annotated) => {
                if let (Some(name), Some(value)) =
                    (name_of(&annotated.target), annotated.value.as_deref())
                    && let Some(ty) = value.inferred_type(&model)
                {
                    println!("{name} : {}", ty.display(&db));
                }
            }
            _ => {}
        }
    }

    Ok(())
}

/// Scan every `.py` file under `root`, aggregating ty diagnostics by rule id.
fn scan_directory(db: &ProjectDatabase, root: &std::path::Path) -> Result<()> {
    let mut py_files = Vec::new();
    collect_py_files(root, &mut py_files);
    py_files.sort();

    let mut by_rule: std::collections::BTreeMap<String, (usize, String)> =
        std::collections::BTreeMap::new();
    let mut checked = 0usize;
    let mut total = 0usize;

    for path in &py_files {
        let Ok(system_path) = to_system_path(path.clone()) else {
            continue;
        };
        let Ok(file) = system_path_to_file(db, &system_path) else {
            continue;
        };
        let Ok(diagnostics) = check_file(db, file) else {
            continue;
        };
        checked += 1;
        for diagnostic in diagnostics.iter() {
            total += 1;
            let rule = diagnostic.id().to_string();
            let entry = by_rule
                .entry(rule)
                .or_insert_with(|| (0, diagnostic.primary_message().to_string()));
            entry.0 += 1;
        }
    }

    println!(
        "== ty over {} ({checked} .py files checked) ==",
        root.file_name().and_then(|n| n.to_str()).unwrap_or("?")
    );
    println!("{total} diagnostics, grouped by rule:\n");
    // `unresolved-import` (and friends) are environment noise when third-party
    // dependencies are not installed; everything else is a real finding.
    let noise = ["unresolved-import", "unresolved-attribute", "unresolved-reference"];
    let mut real = 0usize;
    for (rule, (count, sample)) in &by_rule {
        let tag = if noise.contains(&rule.as_str()) {
            "env-noise"
        } else {
            real += count;
            "REAL"
        };
        println!("[{tag:>9}] {count:>5}  {rule}");
        println!("            e.g. {sample}");
    }
    println!("\nreal type findings (excluding unresolved-import noise): {real}");
    Ok(())
}

/// Recursively gather `.py` files, skipping vendored / virtualenv directories.
fn collect_py_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let skip = matches!(
                path.file_name().and_then(|n| n.to_str()),
                Some(".git" | ".venv" | "venv" | "node_modules" | "__pycache__" | "site-packages")
            );
            if !skip {
                collect_py_files(&path, out);
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("py") {
            out.push(path);
        }
    }
}

/// Read the source-language identifier of a simple `Name` assignment target.
fn name_of(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Name(name) => Some(name.id.to_string()),
        _ => None,
    }
}

/// Convert a filesystem path into ty's UTF-8 `SystemPath`.
fn to_system_path(path: std::path::PathBuf) -> Result<SystemPathBuf> {
    SystemPathBuf::from_path_buf(path).map_err(|original| anyhow!("non-UTF-8 path: {original:?}"))
}
