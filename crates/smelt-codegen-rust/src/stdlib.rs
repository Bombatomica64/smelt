//! Rust backend stdlib dependency discovery.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module"
)]

use smelt_hir::{AsyncOp, CallbackExpr, CallbackExprKind, Type};
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
    if any_rvalue_needs(mir, rvalue_needs_regex)
        || any_callback_needs_regex(mir)
        || needs_unknown_type(mir)
    {
        deps.push(BackendDependency::Regex);
    }
    if any_rvalue_needs(mir, rvalue_needs_rand) {
        deps.push(BackendDependency::Rand);
    }
    if any_rvalue_needs(mir, rvalue_needs_chrono) || needs_unknown_type(mir) {
        deps.push(BackendDependency::Chrono);
    }
    if any_rvalue_needs(mir, rvalue_needs_chrono_tz) {
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
fn any_rvalue_needs(mir: &Mir, needs_dependency: fn(&Rvalue) -> bool) -> bool {
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
fn rvalue_needs_regex(rvalue: &Rvalue) -> bool {
    matches!(
        rvalue,
        Rvalue::RegexIsMatch { .. }
            | Rvalue::RegexReplace { .. }
            | Rvalue::RegexReplaceFirstMatchUppercase { .. }
            | Rvalue::RegexSplit { .. }
            | Rvalue::RegexFind { .. }
            | Rvalue::RegexExec { .. }
    )
}

/// Returns true when a MIR rvalue uses Unicode normalization APIs.
fn rvalue_needs_unicode_normalization(rvalue: &Rvalue) -> bool {
    matches!(rvalue, Rvalue::StringNormalize { .. })
}

/// Returns true when any legacy callback-expression body uses Regex APIs.
fn any_callback_needs_regex(mir: &Mir) -> bool {
    mir.closures
        .iter()
        .filter_map(|closure| closure.callback_body.as_ref())
        .any(|callback| callback_needs_regex(callback, mir))
}

/// Returns true when one callback-expression tree uses Regex APIs.
fn callback_needs_regex(callback: &CallbackExpr, mir: &Mir) -> bool {
    match &callback.kind {
        CallbackExprKind::MethodCall {
            receiver,
            method,
            args,
        } => {
            mir.symbols.get(*method).is_some_and(|name| {
                name == "match" || name == "__smelt_replace_first_match_uppercase"
            }) || callback_needs_regex(receiver, mir)
                || args.iter().any(|arg| callback_needs_regex(&arg.expr, mir))
        }
        CallbackExprKind::Call { callee, args } => {
            callback_needs_regex(callee, mir)
                || args.iter().any(|arg| callback_needs_regex(&arg.expr, mir))
        }
        CallbackExprKind::FunctionTableLookup { key, .. } => callback_needs_regex(key, mir),
        CallbackExprKind::AssignCapture { value, .. }
        | CallbackExprKind::Unary { operand: value, .. }
        | CallbackExprKind::UnknownIs { value, .. } => callback_needs_regex(value, mir),
        CallbackExprKind::ListLit(items) => {
            items.iter().any(|item| callback_needs_regex(item, mir))
        }
        CallbackExprKind::Sequence { effects, result } => {
            effects
                .iter()
                .any(|effect| callback_needs_regex(effect, mir))
                || callback_needs_regex(result, mir)
        }
        CallbackExprKind::DictLit(entries) => entries
            .iter()
            .any(|(_, value)| callback_needs_regex(value, mir)),
        CallbackExprKind::Throw { message } => message
            .as_ref()
            .is_some_and(|panic_message| callback_needs_regex(panic_message, mir)),
        CallbackExprKind::Index { receiver, .. }
        | CallbackExprKind::Field { receiver, .. }
        | CallbackExprKind::HasField { receiver, .. }
        | CallbackExprKind::FieldTruthy { receiver, .. } => callback_needs_regex(receiver, mir),
        CallbackExprKind::DynamicIndex { receiver, index }
        | CallbackExprKind::HasDynamicField {
            receiver,
            field: index,
        }
        | CallbackExprKind::Binary {
            lhs: receiver,
            rhs: index,
            ..
        } => callback_needs_regex(receiver, mir) || callback_needs_regex(index, mir),
        CallbackExprKind::Conditional {
            cond,
            then_expr,
            else_expr,
        } => {
            callback_needs_regex(cond, mir)
                || callback_needs_regex(then_expr, mir)
                || callback_needs_regex(else_expr, mir)
        }
        CallbackExprKind::Param(_)
        | CallbackExprKind::Capture(_)
        | CallbackExprKind::Function(_)
        | CallbackExprKind::Literal(_) => false,
    }
}

/// Returns true when a MIR rvalue uses Chrono APIs.
fn rvalue_needs_chrono(rvalue: &Rvalue) -> bool {
    matches!(
        rvalue,
        Rvalue::DateNow
            | Rvalue::DateToIsoString { .. }
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
