//! Writes an emitted crate to disk.
//!
//! Everything here is filesystem policy: the crate layout, the stale-root cleanup
//! and the write-only-when-changed rule Cargo's incremental reuse depends on.

use std::{fs, path::Path};

use smelt_mir::Mir;

use crate::crate_builder::{FreeFunctionHome, emit_root};
use crate::gates::assert_runtime_symbols_defined;
use crate::module_map::emit_mapped_sources;
use crate::{CrateKind, EmitOptions, deps};

/// Emits a complete Rust crate from the given MIR.
///
/// Creates the crate structure with Cargo.toml and the configured crate root.
pub fn emit_crate(
    mir: &Mir,
    output_path: impl AsRef<Path>,
    options: &EmitOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = output_path.as_ref();
    let src_dir = output_dir.join("src");
    fs::create_dir_all(&src_dir)?;
    let allocator = options.effective_allocator();
    let root = emit_root(mir, allocator, |_function, _context| FreeFunctionHome::Root)?;
    let source = root.source();
    assert_runtime_symbols_defined([source.as_str()])?;
    write_if_changed(
        output_dir.join("Cargo.toml"),
        &deps::cargo_toml(
            &options.crate_name,
            &root.dependencies,
            allocator,
            options,
        ),
    )?;
    write_crate_root(&src_dir, options.crate_kind, &source)?;
    Ok(())
}

/// Emits a complete Rust crate while preserving the source module layout.
///
/// Shared generated runtime/types stay in the configured crate root. Non-entry
/// module-level functions are moved into source-shaped Rust modules and
/// re-exported from the crate root so existing flat-name call emission
/// continues to resolve.
pub fn emit_crate_with_modules(
    mir: &Mir,
    krate: &smelt_hir::Crate,
    modules: &[(String, smelt_hir::ModuleId)],
    output_path: impl AsRef<Path>,
    options: &EmitOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    emit_crate_with_modules_reporting(mir, krate, modules, output_path, options).map(|_| ())
}

/// [`emit_crate_with_modules`], also returning the backend's erasures.
///
/// These are the erasures decided while rendering types (erased generics and
/// unions, unresolved classes, `never`). See `emitter::backend_erasure` for the
/// span each record carries. The records are unsorted and may repeat;
/// normalize them with [`smelt_hir::normalize_erasures`].
pub fn emit_crate_with_modules_reporting(
    mir: &Mir,
    krate: &smelt_hir::Crate,
    modules: &[(String, smelt_hir::ModuleId)],
    output_path: impl AsRef<Path>,
    options: &EmitOptions,
) -> Result<Vec<smelt_hir::ErasureRecord>, Box<dyn std::error::Error>> {
    let output_dir = output_path.as_ref();
    let src_dir = output_dir.join("src");
    fs::create_dir_all(&src_dir)?;
    let allocator = options.effective_allocator();
    let mapped = emit_mapped_sources(mir, krate, modules, allocator)?;
    assert_runtime_symbols_defined(
        std::iter::once(mapped.root.as_str()).chain(mapped.modules.iter().map(|module| module.source.as_str())),
    )?;
    write_if_changed(
        output_dir.join("Cargo.toml"),
        &deps::cargo_toml(
            &options.crate_name,
            &mapped.dependencies,
            allocator,
            options,
        ),
    )?;
    write_crate_root(&src_dir, options.crate_kind, &mapped.root)?;
    for module in mapped.modules {
        let module_path = src_dir.join(format!("{}.rs", module.name));
        write_if_changed(module_path, &module.source)?;
    }
    Ok(mapped.erasures)
}

/// Writes the configured crate root and removes the stale opposite root.
///
/// Cargo infers program and library targets from `src/main.rs` and `src/lib.rs`.
/// Removing the other generated root keeps manifest kind changes from silently
/// producing both targets in the same output directory.
fn write_crate_root(
    src_dir: &Path,
    crate_kind: CrateKind,
    contents: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    write_if_changed(src_dir.join(crate_kind.root_file_name()), contents)?;
    let stale_root = src_dir.join(crate_kind.stale_root_file_name());
    if stale_root.exists() {
        fs::remove_file(stale_root)?;
    }
    Ok(())
}

/// Writes generated text only when the destination bytes actually change.
///
/// Cargo uses file mtimes as part of its freshness checks. Rewriting every
/// generated Rust module on each Smelt build forces Cargo to recompile modules
/// whose source is byte-for-byte identical, which dominates large projects such
/// as Remeda. Keeping identical files untouched preserves incremental build
/// artifacts without changing Smelt's lowering or codegen semantics.
fn write_if_changed(
    path: impl AsRef<Path>,
    contents: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let destination = path.as_ref();
    if fs::read_to_string(destination).is_ok_and(|existing| existing == contents) {
        return Ok(());
    }
    fs::write(destination, contents)?;
    Ok(())
}
