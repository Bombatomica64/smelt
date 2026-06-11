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
    if any_rvalue_needs(mir, |rvalue| rvalue_needs_regex(rvalue, mir)) || needs_unknown_type(mir) {
        deps.push(BackendDependency::Regex);
    }
    if any_rvalue_needs(mir, rvalue_needs_rand) {
        deps.push(BackendDependency::Rand);
    }
    if any_rvalue_needs(mir, rvalue_needs_chrono) || needs_unknown_type(mir) {
        deps.push(BackendDependency::Chrono);
    }
    if any_rvalue_needs(mir, rvalue_needs_chrono_tz) || needs_unknown_type(mir) {
        deps.push(BackendDependency::ChronoTz);
    }
    if any_rvalue_needs(mir, rvalue_needs_url) {
        deps.push(BackendDependency::Url);
    }
    if any_rvalue_needs(mir, rvalue_needs_unicode_normalization) {
        deps.push(BackendDependency::UnicodeNormalization);
    }
    deps
}

/// Returns true when any MIR rvalue satisfies the dependency predicate.
///
/// Dependency detection stays rvalue-based so frontend features can add new
/// MIR operations without spreading Cargo dependency knowledge through codegen.
fn any_rvalue_needs(mir: &Mir, needs_dependency: impl Fn(&Rvalue) -> bool) -> bool {
    rvalues(mir).any(needs_dependency)
}

/// Iterates over all rvalues that can require generated backend dependencies.
fn rvalues(mir: &Mir) -> impl Iterator<Item = &Rvalue> {
    let function_rvalues = mir.functions.iter().flat_map(|function| {
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
    });
    let closure_rvalues = mir.closures.iter().flat_map(|closure| {
        closure.blocks.iter().flat_map(|block| {
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
    });
    function_rvalues.chain(closure_rvalues)
}

/// Returns true when a MIR rvalue uses Regex APIs.
///
/// Sort comparators reach codegen as closure operands, so any regex usage
/// inside a comparator body is detected through the closure rvalues iterated by
/// [`rvalues`]; no comparator-tree scan is needed here.
fn rvalue_needs_regex(rvalue: &Rvalue, _mir: &Mir) -> bool {
    matches!(
        rvalue,
        Rvalue::RegexIsMatch { .. }
            | Rvalue::RegexReplace { .. }
            | Rvalue::RegexReplaceFirstMatchUppercase { .. }
            | Rvalue::RegexSplit { .. }
            | Rvalue::RegexFind { .. }
            | Rvalue::RegexExec { .. }
            | Rvalue::RegexMatchAll { .. }
            | Rvalue::StringSplit { .. }
    )
}

/// Returns true when a MIR rvalue uses Unicode normalization APIs.
fn rvalue_needs_unicode_normalization(rvalue: &Rvalue) -> bool {
    matches!(rvalue, Rvalue::StringNormalize { .. })
}

/// Returns true when a MIR rvalue uses Chrono APIs.
fn rvalue_needs_chrono(rvalue: &Rvalue) -> bool {
    matches!(
        rvalue,
        Rvalue::DateNow
            | Rvalue::DateSetNow { .. }
            | Rvalue::DateResetNow
            | Rvalue::DateToIsoString { .. }
            | Rvalue::DateToString { .. }
            | Rvalue::DateFromParts { .. }
            | Rvalue::DateFromValue { .. }
            | Rvalue::DateGetPart { .. }
            | Rvalue::DateSetPart { .. }
            | Rvalue::DateTimezoneContext { .. }
    )
}

/// Returns true when a MIR rvalue converts timestamps in an IANA time zone.
fn rvalue_needs_chrono_tz(rvalue: &Rvalue) -> bool {
    matches!(rvalue, Rvalue::DateTimezoneContext { .. })
}

/// Returns true when generated Rust needs mutable `Date.now()` mock-clock state.
#[must_use]
pub(crate) fn needs_date_now_runtime(mir: &Mir) -> bool {
    any_rvalue_needs(mir, |rvalue| {
        matches!(
            rvalue,
            Rvalue::DateNow | Rvalue::DateSetNow { .. } | Rvalue::DateResetNow
        )
    })
}

/// Returns true when generated Rust needs mutable `Date.getTimezoneOffset()` state.
#[must_use]
pub(crate) fn needs_date_timezone_offset_runtime(mir: &Mir) -> bool {
    any_rvalue_needs(mir, |rvalue| {
        matches!(
            rvalue,
            Rvalue::DateTimezoneOffset
                | Rvalue::DateSetTimezoneOffset { .. }
                | Rvalue::DateResetTimezoneOffset
        )
    })
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
    mir.types
        .all()
        .iter()
        .any(|ty| matches!(ty, Type::Unknown | Type::Never))
        || mir.functions.iter().any(|function| {
            function.blocks.iter().any(|block| {
                block.statements.iter().any(|statement| {
                    matches!(
                        statement,
                        Statement::Assign {
                            value: Rvalue::UnknownCast { .. } | Rvalue::UnknownIs { .. },
                            ..
                        } | Statement::AssignPlace {
                            value: Rvalue::UnknownCast { .. } | Rvalue::UnknownIs { .. },
                            ..
                        }
                    )
                })
            })
        })
}

/// Returns true when generated Rust uses Tokio APIs.
#[must_use]
pub(crate) fn needs_tokio(mir: &Mir) -> bool {
    mir.functions.iter().any(|function| {
        (function.is_async
            && (function.is_test
                || mir
                    .symbols
                    .get(function.name)
                    .is_some_and(|name| name == "main")))
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
