//! CLI pipeline helpers for AST/HIR/MIR rendering and crate emission.

use std::{
    io,
    io::Write as _,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};

use smelt_hir::format_compact;

use crate::{
    config::{Allocator, Config, OutputKind, ReleaseProfile},
    lowering,
    manifest::resolve_manifest_path,
    stubs, timing,
};

/// Parses a Python file and dumps the Ruff AST for CLI debugging.
///
/// Delegates to the [`crate::python`] module so the Ruff-backed frontend stays
/// behind the `python` feature; TypeScript-only builds report that Python
/// support was not compiled in.
pub(crate) fn dump_python_ast(file: &str) -> Result<(), Box<dyn std::error::Error>> {
    crate::python::dump_ast(file)
}

/// Prints HIR in compact or debug format.
pub(crate) fn print_hir(
    krate: &smelt_hir::Crate,
    modules: &[(String, smelt_hir::ModuleId)],
    debug: bool,
) {
    let mut stdout = io::stdout().lock();
    if debug {
        let _ignored = writeln!(stdout, "{krate:#?}");
    } else {
        let _ignored = write!(stdout, "{}", format_compact(krate, modules));
    }
}

/// Prints optimized MIR in compact format.
pub(crate) fn print_mir(krate: &smelt_hir::Crate) -> Result<(), Box<dyn std::error::Error>> {
    let mir = lower_to_optimized_mir(krate)?;
    let mut stdout = io::stdout().lock();
    write!(stdout, "{}", smelt_mir::format_compact(&mir))?;
    Ok(())
}

/// Lowers HIR to MIR, runs optimizations, and validates the result.
pub(crate) fn lower_to_optimized_mir(
    krate: &smelt_hir::Crate,
) -> Result<smelt_mir::Mir, Box<dyn std::error::Error>> {
    let mut mir = timing::measure("mir.lower_hir", || {
        smelt_mir::lower_hir(krate).map_err(|errors| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("MIR lowering failed:\n{errors:#?}"),
            )
        })
    })?;
    timing::measure("mir.optimize", || {
        smelt_mir::opt::optimize(&mut mir);
        Ok::<(), Box<dyn std::error::Error>>(())
    })?;
    let validation_errors = timing::measure("mir.validate", || {
        Ok::<_, Box<dyn std::error::Error>>(smelt_mir::validate(&mir))
    })?;
    if !validation_errors.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("MIR validation failed:\n{validation_errors:#?}"),
        )
        .into());
    }
    Ok(mir)
}

/// Builds a Rust crate from manifest configuration.
pub(crate) fn build_rust_crate(
    config: &Config,
    manifest_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let (krate, modules) = timing::measure("manifest.lower", || {
        lowering::lower_manifest_entries(config, manifest_path)
    })?;
    timing::measure("stubs.emit", || {
        stubs::emit_type_declarations(&krate, &modules)
    })?;
    let mir = lower_to_optimized_mir(&krate)?;
    let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let output_dir = resolve_manifest_path(manifest_dir, config.output_target());
    let crate_name = config
        .output_crate_name()
        .unwrap_or_else(|| config.project_name())
        .replace('-', "_");
    timing::measure("rust.emit_crate", || {
        smelt_codegen_rust::emit_crate_with_modules(
            &mir,
            &krate,
            &modules,
            &output_dir,
            &smelt_codegen_rust::EmitOptions::new(crate_name)
                .with_crate_kind(codegen_crate_kind(config.output_kind()))
                .with_allocator(codegen_allocator(config.rust_allocator()))
                .with_release_profile(codegen_release_profile(config.rust_release_profile())),
        )
    })?;

    if config.should_build_output() {
        timing::measure("rust.cargo_build", || build_generated_crate(&output_dir))?;
    }

    Ok(())
}

/// Checks manifest entries and emits declaration stubs without Rust generation.
pub(crate) fn check_manifest(
    config: &Config,
    manifest_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let (krate, modules) = timing::measure("manifest.lower", || {
        lowering::lower_manifest_entries(config, manifest_path)
    })?;
    timing::measure("stubs.emit", || {
        stubs::emit_type_declarations(&krate, &modules)
    })?;
    Ok(())
}

/// Converts manifest output kind to the Rust codegen crate kind.
fn codegen_crate_kind(output_kind: OutputKind) -> smelt_codegen_rust::CrateKind {
    match output_kind {
        OutputKind::Program => smelt_codegen_rust::CrateKind::Program,
        OutputKind::Library => smelt_codegen_rust::CrateKind::Library,
    }
}

/// Maps the manifest's allocator choice onto the codegen enum.
fn codegen_allocator(allocator: Allocator) -> smelt_codegen_rust::GeneratedAllocator {
    match allocator {
        Allocator::Mimalloc => smelt_codegen_rust::GeneratedAllocator::Mimalloc,
        Allocator::System => smelt_codegen_rust::GeneratedAllocator::System,
    }
}

/// Maps the manifest's release-profile choice onto the codegen enum.
fn codegen_release_profile(profile: ReleaseProfile) -> smelt_codegen_rust::ReleaseProfile {
    match profile {
        ReleaseProfile::Optimized => smelt_codegen_rust::ReleaseProfile::Optimized,
        ReleaseProfile::Default => smelt_codegen_rust::ReleaseProfile::Default,
    }
}

/// Builds the generated output crate by running `cargo build` in that directory.
fn build_generated_crate(output_dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let output = ProcessCommand::new("cargo")
        .arg("build")
        .current_dir(output_dir)
        .output()?;
    if output.status.success() {
        return Ok(());
    }

    Err(io::Error::other(format!(
        "generated crate failed to build\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
    .into())
}
