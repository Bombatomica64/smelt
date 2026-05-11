//! Rust backend stdlib dependency discovery.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module"
)]

use smelt_hir::{AsyncOp, Type};
use smelt_mir::{Mir, Rvalue, Statement};
use smelt_stdlib::BackendDependency;

/// Collect shared backend dependencies needed by generated stdlib operations.
#[must_use]
pub(crate) fn backend_dependencies(mir: &Mir) -> Vec<BackendDependency> {
    let mut deps = Vec::new();
    if any_rvalue_needs(mir, rvalue_needs_reqwest) {
        deps.push(BackendDependency::Reqwest);
    }
    if any_rvalue_needs(mir, rvalue_needs_serde_json) {
        deps.push(BackendDependency::SerdeJson);
    }
    if any_rvalue_needs(mir, rvalue_needs_regex) {
        deps.push(BackendDependency::Regex);
    }
    if any_rvalue_needs(mir, rvalue_needs_rand) {
        deps.push(BackendDependency::Rand);
    }
    if any_rvalue_needs(mir, rvalue_needs_chrono) {
        deps.push(BackendDependency::Chrono);
    }
    if any_rvalue_needs(mir, rvalue_needs_url) {
        deps.push(BackendDependency::Url);
    }
    deps
}

/// Returns true when any MIR rvalue satisfies the dependency predicate.
///
/// Dependency detection stays rvalue-based so frontend features can add new
/// MIR operations without spreading Cargo dependency knowledge through codegen.
fn any_rvalue_needs(mir: &Mir, needs_dependency: fn(&Rvalue) -> bool) -> bool {
    rvalues(mir).any(needs_dependency)
}

/// Iterates over all rvalues that can require generated backend dependencies.
fn rvalues(mir: &Mir) -> impl Iterator<Item = &Rvalue> {
    mir.functions.iter().flat_map(|function| {
        function.blocks.iter().flat_map(|block| {
            block
                .statements
                .iter()
                .filter_map(|statement| match statement {
                    Statement::Assign { value, .. } | Statement::AssignPlace { value, .. } => {
                        Some(value)
                    }
                    Statement::StorageLive(_) | Statement::StorageDead(_) => None,
                })
        })
    })
}

/// Returns true when a MIR rvalue uses Regex APIs.
fn rvalue_needs_regex(rvalue: &Rvalue) -> bool {
    matches!(
        rvalue,
        Rvalue::RegexIsMatch { .. } | Rvalue::RegexReplace { .. } | Rvalue::RegexSplit { .. }
    )
}

/// Returns true when a MIR rvalue uses Chrono APIs.
fn rvalue_needs_chrono(rvalue: &Rvalue) -> bool {
    matches!(rvalue, Rvalue::DateNow | Rvalue::DateToIsoString { .. })
}

/// Returns true when a MIR rvalue uses Url APIs.
fn rvalue_needs_url(rvalue: &Rvalue) -> bool {
    matches!(rvalue, Rvalue::UrlField { .. })
}

/// Returns true when a MIR rvalue uses Rand APIs.
fn rvalue_needs_rand(rvalue: &Rvalue) -> bool {
    matches!(
        rvalue,
        Rvalue::NumericRandom | Rvalue::NumericRandomInt { .. } | Rvalue::ListRandomChoice { .. }
    )
}

/// Returns true when a MIR rvalue uses Serde JSON APIs.
fn rvalue_needs_serde_json(rvalue: &Rvalue) -> bool {
    matches!(
        rvalue,
        Rvalue::JsonStringify { .. } | Rvalue::JsonParse { .. }
    )
}

/// Returns true when a MIR rvalue uses Reqwest APIs.
fn rvalue_needs_reqwest(rvalue: &Rvalue) -> bool {
    matches!(
        rvalue,
        Rvalue::HttpGetText { .. }
            | Rvalue::AsyncOp {
                op: AsyncOp::HttpGetText,
                ..
            }
    )
}

/// Returns true when generated Rust needs the opaque `unknown` carrier type.
#[must_use]
pub(crate) fn needs_unknown_type(mir: &Mir) -> bool {
    mir.types.all().iter().any(|ty| matches!(ty, Type::Unknown))
}

/// Returns true when generated Rust uses Tokio APIs.
#[must_use]
pub(crate) fn needs_tokio(mir: &Mir) -> bool {
    mir.functions.iter().any(|function| {
        (function.is_async
            && mir
                .symbols
                .get(function.name)
                .is_some_and(|name| name == "main"))
            || function.blocks.iter().any(|block| {
                block.statements.iter().any(|statement| {
                    matches!(
                        statement,
                        Statement::Assign {
                            value: Rvalue::AsyncOp { .. },
                            ..
                        } | Statement::AssignPlace {
                            value: Rvalue::AsyncOp { .. },
                            ..
                        }
                    )
                })
            })
    })
}
