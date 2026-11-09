//! Module-level mutable globals: their thread-local cells and static names.
#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible items are shared with the crate root and the emitter shards"
)]

use crate::rust::RustExpr;
use smelt_hir::Type;
use smelt_mir::{Mir, MirGlobalInit};

use crate::emitter::{self, EmitContext, FunctionEmitter};
use crate::rust::CodeWriter;
use crate::thrown::ERROR_BOX;
use crate::{EmitError, compact_index};

/// Emit the `thread_local!` block declaring every mutable global's cell.
///
/// Each `#[test]` runs on its own thread, so a `thread_local!` cell gives every
/// test a fresh copy of module state initialized from the source literal —
/// deterministic, mirroring vitest's per-file module isolation. Copy primitives
/// (`f64`/`i64`/`bool`) use a `const`-initialized [`std::cell::Cell`]; strings
/// use a [`std::cell::RefCell`] with the non-const initializer form because a
/// `.to_owned()` initializer cannot appear in a `const` block.
pub(crate) fn emit_mutable_globals(mir: &Mir, context: &EmitContext) -> Result<String, EmitError> {
    let mut writer = CodeWriter::new();
    let mut any_fallible_init = false;
    writer.line("thread_local! {");
    for (index, global) in mir.globals.iter().enumerate() {
        let name = global_static_name(mir, compact_index(index, "global index")?);
        // The cell's initial value. A constant is emitted inline; an expression
        // initializer is a CALL to the nullary function the frontend
        // synthesized from it, which is emitted like any other function. A
        // `thread_local!` initializer is an arbitrary expression evaluated once
        // per thread on first access, so a call is as legal there as a literal
        // — and that timing is exactly JavaScript's "module state is
        // initialized before any consumer runs", per generated test thread.
        let init = match &global.init {
            MirGlobalInit::Constant(constant) => emitter::literals::constant_expr(
                constant,
                !matches!(mir.types.get(global.ty), Some(Type::PyStr)),
            ),
            MirGlobalInit::Call(func_id) => {
                let function = mir
                    .functions
                    .get(usize::try_from(func_id.0).unwrap_or(usize::MAX))
                    .ok_or_else(|| {
                        EmitError::new(
                            "mutable global initializer references an unknown function",
                        )
                    })?;
                let function_name = context
                    .names()
                    .function(function.id)
                    .ok_or_else(|| EmitError::new("mutable global initializer has no planned name"))?
                    .as_str()
                    .to_owned();
                // An initializer that can throw (`const db = new
                // DatabaseSync(path)`, a call to a throwing function) returns a
                // `Result`, and a `thread_local!` initializer has nowhere to
                // propagate it. The cell is touched at the binding's own
                // declaration (see `module_init`), so a throw surfaces there,
                // after the output of the statements before it — and a throw
                // during module evaluation is always uncaught in JavaScript.
                if function.can_throw {
                    any_fallible_init = true;
                    RustExpr::atom(function_name).postfix("().unwrap_or_else(|error| smelt_module_init_failed(error))")
                } else {
                    RustExpr::atom(function_name).postfix("()")
                }
            }
        };
        // A Copy primitive uses `Cell`, which needs no borrow bookkeeping and
        // can be `const`-initialized from a literal. Everything else — a
        // `String`, a record, a list, a map — is not `Copy`, so its cell is a
        // `RefCell`; an owned or computed initializer cannot be `const` either.
        let is_constant_init = matches!(global.init, MirGlobalInit::Constant(_));
        match mir.types.get(global.ty) {
            Some(Type::Float | Type::Int | Type::Bool) if is_constant_init => {
                let rust_ty = match mir.types.get(global.ty) {
                    Some(Type::Float) => "f64",
                    Some(Type::Int) => "i64",
                    _ => "bool",
                };
                writer.line(format!(
                    "    static {name}: ::std::cell::Cell<{rust_ty}> = const {{ ::std::cell::Cell::new({init}) }};", init = init.delimited()
                ));
            }
            Some(Type::Float | Type::Int | Type::Bool) => {
                let rust_ty = match mir.types.get(global.ty) {
                    Some(Type::Float) => "f64",
                    Some(Type::Int) => "i64",
                    _ => "bool",
                };
                writer.line(format!(
                    "    static {name}: ::std::cell::Cell<{rust_ty}> = ::std::cell::Cell::new({init});", init = init.delimited()
                ));
            }
            _ => {
                let value_ty = FunctionEmitter::type_text_for_with_context(mir, context, global.ty)?;
                writer.line(format!(
                    "    static {name}: ::std::cell::RefCell<{value_ty}> = ::std::cell::RefCell::new({init});", init = init.delimited()
                ));
            }
        }
    }
    writer.line("}");
    if any_fallible_init {
        emit_module_init_failed(&mut writer);
    }
    Ok(writer.finish())
}

/// Emit `smelt_module_init_failed`, the report for a module global whose
/// initializer threw.
///
/// It does what a throwing top-level statement does through `main`'s
/// `Result` return — prints `Error: {error:?}` to stderr and exits with status
/// 1 — because the initializer runs at the same point of module evaluation and
/// no `catch` can enclose a module-level declaration. Not a panic: a panic
/// prints a different message and exits 101, so the two spellings of the same
/// source failure would disagree. Stdout is flushed first, since `exit` runs
/// no destructors and a partial line would otherwise be lost.
fn emit_module_init_failed(writer: &mut CodeWriter) {
    writer.line("/// A module global's initializer threw during module evaluation: report it as `main` does and exit 1.");
    writer.block(
        format!("fn smelt_module_init_failed(error: {ERROR_BOX}) -> !"),
        |fn_writer| {
            fn_writer.line("let _ = ::std::io::Write::flush(&mut ::std::io::stdout());");
            fn_writer.line("eprintln!(\"Error: {error:?}\");");
            fn_writer.line("::std::process::exit(1)");
        },
    );
}

/// The expression reading mutable global `index`'s current value: a `Cell`
/// read for the `Copy` primitives, a cloned borrow otherwise (the cell kinds
/// `emit_mutable_globals` declares).
pub(crate) fn global_read_text(mir: &Mir, index: u32) -> String {
    let name = global_static_name(mir, index);
    let copy = usize::try_from(index)
        .ok()
        .and_then(|idx| mir.globals.get(idx))
        .is_some_and(|global| matches!(mir.types.get(global.ty), Some(Type::Float | Type::Int | Type::Bool)));
    if copy {
        format!("{name}.with(::std::cell::Cell::get)")
    } else {
        format!("{name}.with(|smelt_value| smelt_value.borrow().clone())")
    }
}

/// Compute the thread-local static name for a mutable global by index.
///
/// The name is derived from the binding's source symbol (sanitized and
/// uppercased) with the global's dense `Mir::globals` index appended, prefixed
/// `SMELT_GLOBAL_`. Including the index disambiguates cross-module bindings
/// that share a source name so each mutable global gets a unique per-program
/// static. These names are program-specific and never enter the fixed
/// runtime-symbol registry.
pub(crate) fn global_static_name(mir: &Mir, index: u32) -> String {
    let base = usize::try_from(index)
        .ok()
        .and_then(|idx| mir.globals.get(idx))
        .and_then(|global| mir.symbols.get(global.name))
        .map_or_else(|| "global".to_owned(), crate::names::fragment);
    format!("SMELT_GLOBAL_{}_{index}", base.to_uppercase())
}
