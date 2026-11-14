//! Program start-up: running the module bodies from `main`.
//!
//! A JavaScript or Python program evaluates its module graph once at start:
//! each module body runs exactly once, a module's imports before its own
//! body, the entries in the order the manifest lists them. The transpiler
//! numbers the modules that run so (`smelt_hir::Module::run_order`), and the
//! backend turns that order into calls:
//!
//! * One entry: its body is the crate's `main` (with the async runtime and the
//!   error return the body needs), and `main` first calls the bodies of the
//!   modules it reaches, in order (`FunctionEmitter`'s `main` emission).
//! * Several entries, or an entry not lowered as `main`: the crate's `main`
//!   calls every body in order (`CrateBuilder::emit_main_fallback`).
//!
//! A body that runs on the async runtime can only be `main` itself: any other
//! async body in the order is a build error, never a silently skipped body.
#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible items are shared with the crate root and the emitter"
)]

use crate::emitter::EmitContext;
use crate::error::EmitError;
use smelt_mir::{FuncId, Mir, MirFunction};

/// One module body `main` runs at start.
#[derive(Debug, Clone)]
pub(crate) struct StartupBody {
    /// The body's function.
    pub(crate) function: FuncId,
    /// The Rust module the body is defined in, when not the crate root. The
    /// call names it (`__smelt_module_Setter::Setter()`): through the root's
    /// glob re-export, a body named like a root item (the tuple struct of a
    /// class `Setter`) would resolve to that item instead.
    pub(crate) module: Option<String>,
}

/// The module body functions to run at start, in order (see the module docs).
pub(crate) fn startup_functions(
    krate: &smelt_hir::Crate,
    mir: &Mir,
    module_of: impl Fn(smelt_hir::BodyId) -> Option<String>,
) -> Vec<StartupBody> {
    let mut ordered = krate
        .modules
        .iter()
        .filter_map(|module| Some((module.run_order?, module.body?)))
        .collect::<Vec<_>>();
    ordered.sort_by_key(|(order, _)| *order);
    ordered
        .into_iter()
        .filter_map(|(_, body)| {
            mir.functions
                .iter()
                .find(|function| {
                    !function.is_test
                        && matches!(function.origin, smelt_mir::HirOrigin::Body(owner) if owner == body)
                })
                .map(|function| StartupBody {
                    function: function.id,
                    module: module_of(body),
                })
        })
        .collect()
}

/// The statements calling the start-up bodies of `context` other than
/// `skip`, for a caller that returns `Result` when `caller_throws`.
///
/// A throwing body propagates with `?` from a throwing caller; from one that
/// cannot return an error it reports the uncaught exception and exits with
/// status 1, as an uncaught exception ends a Node process.
pub(crate) fn startup_calls(
    mir: &Mir,
    context: &EmitContext,
    skip: Option<FuncId>,
    caller_throws: bool,
) -> Result<String, EmitError> {
    let mut out = String::new();
    for body in context.startup() {
        if Some(body.function) == skip {
            continue;
        }
        let Some(function) = mir
            .functions
            .iter()
            .find(|function| function.id == body.function)
        else {
            continue;
        };
        let bare = startup_function_name(context, function)?;
        let name = match &body.module {
            Some(module) => format!("{module}::{bare}"),
            None => bare.clone(),
        };
        if function.is_async {
            return Err(EmitError::new(format!(
                "module `{bare}` awaits at top level but is not the program's single entry; \
                 Smelt runs top-level await only in the entry module"
            )));
        }
        if !function.can_throw {
            out.push_str(&format!("{name}();\n"));
        } else if caller_throws {
            out.push_str(&format!("{name}()?;\n"));
        } else {
            out.push_str(&format!(
                "if let Err(smelt_error) = {name}() {{ eprintln!(\"Uncaught {{smelt_error}}\"); ::std::process::exit(1); }}\n"
            ));
        }
    }
    Ok(out)
}

/// Whether any start-up body other than `skip` can throw.
pub(crate) fn startup_throws(mir: &Mir, context: &EmitContext, skip: Option<FuncId>) -> bool {
    context.startup().iter().any(|body| {
        Some(body.function) != skip
            && mir
                .functions
                .iter()
                .any(|function| function.id == body.function && function.can_throw)
    })
}

/// The planned Rust name of a start-up body.
fn startup_function_name(
    context: &EmitContext,
    function: &MirFunction,
) -> Result<String, EmitError> {
    context
        .names()
        .function(function.id)
        .map(|name| name.as_str().to_owned())
        .ok_or_else(|| EmitError::new("module body has no planned Rust name"))
}
