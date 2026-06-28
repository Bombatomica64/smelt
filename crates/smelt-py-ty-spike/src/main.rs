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
        (Some(root), Some(rel)) => {
            let root = std::path::PathBuf::from(root);
            let file = root.join(rel);
            (root, file)
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
