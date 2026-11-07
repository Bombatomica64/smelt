//! Rust backend stdlib dependency discovery.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module"
)]

use smelt_hir::Type;
use smelt_mir::{Mir, Rvalue, Statement};

/// Iterates over all rvalues that can require generated backend dependencies.
pub(crate) fn rvalues(mir: &Mir) -> impl Iterator<Item = &Rvalue> {
    let function_rvalues = mir.functions.iter().flat_map(|function| {
        function.blocks.iter().flat_map(|block| {
            block.statements.iter().map(Statement::value)
        })
    });
    let closure_rvalues = mir.closures.iter().flat_map(|closure| {
        closure.blocks.iter().flat_map(|block| {
            block.statements.iter().map(Statement::value)
        })
    });
    function_rvalues.chain(closure_rvalues)
}

/// Resolve a class symbol to its shared stdlib class identity, if any.
///
/// The one place that answers "is this `Type::Class` a modeled host class",
/// so the pay-for-use gates, the type resolver and the member emitters cannot
/// disagree about it.
///
/// Declines for a name the SOURCE declares as a class. A program that writes
/// its own `class File extends Blob` owns that name: the crate emits a real
/// `File` struct, `Type::Class { File }` means that struct, and the modeled
/// host class of the same spelling is not in play. Without the check the two
/// halves of one statement disagreed -- the declaration's type came from this
/// resolver (`SmeltBlob`) while the constructor came from the class item
/// (`File::new(..)`), which is `error[E0308]` in the generated crate -- and the
/// pay-for-use gate pulled in a runtime type the crate never uses.
///
/// A named class EXPRESSION never reaches this check: its name is bound only
/// inside the class's own body, so the frontend gives it its own type symbol
/// (`host_shadowing_class_expression_name`) rather than taking the spelling
/// away from the host class.
///
/// Interfaces are deliberately NOT consulted. A source `interface Headers`
/// declares a shape over the host value rather than replacing it, and the
/// ambient `ResponseInit`/`RequestInit` interfaces resolve through here on
/// purpose.
pub(crate) fn stdlib_class_of_class_symbol(
    mir: &Mir,
    name: smelt_hir::Symbol,
) -> Option<smelt_stdlib::StdlibClass> {
    if mir.has_class(name) {
        return None;
    }
    // The RENDERING decides this, not the recorded original name: a class
    // expression that took an internal type symbol keeps the source spelling as
    // its original name so reflection can answer `File`, and asking the
    // original name here would hand that class the host runtime type again.
    mir.symbols
        .get(name)
        .or_else(|| mir.names.get(name))
        .and_then(smelt_stdlib::typescript_stdlib_class)
}

/// Collect the distinct host constructor names that the crate reassigns, paired
/// with the sanitized upper-cased `thread_local!` slot suffix.
///
/// The returned pairs `(slot_suffix, display_name)` drive per-name slot
/// emission: one `SMELT_HOST_OVERRIDE_<slot_suffix>` slot per name, and the
/// `display_name` feeds the native-handle marker's `name` field. Sorted and
/// de-duplicated so slot emission is deterministic.
#[must_use]
pub(crate) fn host_override_slot_names(mir: &Mir) -> Vec<(String, String)> {
    let mut names = std::collections::BTreeMap::new();
    for rvalue in rvalues(mir) {
        let class = match rvalue {
            Rvalue::HostGlobalRead { class }
            | Rvalue::HostGlobalWrite { class, .. }
            | Rvalue::HostGlobalPresent { class } => *class,
            _ => continue,
        };
        if let Some(name) = mir.symbols.get(class) {
            names.insert(host_override_slot_suffix(name), name.to_owned());
        }
    }
    names.into_iter().collect()
}

/// Sanitize a host constructor name into the upper-cased `thread_local!` slot
/// suffix (`Intl.Locale` -> `INTL_LOCALE`, `Blob` -> `BLOB`).
#[must_use]
pub(crate) fn host_override_slot_suffix(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// Returns whether a set element type backs a plain Rust `HashSet`.
///
/// Mirrors [`FunctionEmitter::type_is_hash_set_key_safe`] at module scope so
/// the `needs_unknown` gate can decide whether a `Set` forces the `SmeltJsSet`
/// runtime (and therefore the unknown carrier) on. Only value-equality
/// primitives, plus optionals, futures and unions whose members all qualify, back
/// a `HashSet`; everything else uses `SmeltJsSet`.
pub(crate) fn module_hash_set_key_safe(mir: &Mir, ty: smelt_hir::TypeId) -> bool {
    match mir.types.get(ty) {
        Some(Type::Bool | Type::Int | Type::String) => true,
        Some(Type::Optional(inner) | Type::Future(inner)) => {
            module_hash_set_key_safe(mir, *inner)
        }
        Some(Type::Union(items)) => items.iter().all(|item| module_hash_set_key_safe(mir, *item)),
        _ => false,
    }
}
