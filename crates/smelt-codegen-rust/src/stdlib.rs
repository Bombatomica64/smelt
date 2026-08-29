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
pub(crate) fn rvalues(mir: &Mir) -> impl Iterator<Item = &Rvalue> {
    let function_rvalues = mir.functions.iter().flat_map(|function| {
        function.blocks.iter().flat_map(|block| {
            block
                .statements
                .iter()
                .filter_map(|statement| match statement {
                    Statement::Assign { value, .. }
                    | Statement::AssignPlace { value, .. }
                    | Statement::DictEntryUpdate { value, .. } => Some(value),
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
                    Statement::Assign { value, .. }
                    | Statement::AssignPlace { value, .. }
                    | Statement::DictEntryUpdate { value, .. } => Some(value),
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

/// Returns true when generated Rust needs the `smelt_encode_uri` runtime helper
/// backing JavaScript `encodeURI`.
pub(crate) fn needs_uri_encode_runtime(mir: &Mir) -> bool {
    any_rvalue_needs(mir, |rvalue| matches!(rvalue, Rvalue::UriEncode { .. }))
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

/// Returns true when generated Rust constructs or asserts a stateful Vitest
/// mock and therefore needs the `SmeltVitestMock*` runtime prelude (the
/// per-mock state registry, chainable configuration methods, and the
/// `SmeltPromise::rejected` constructor for `mockRejectedValue*`).
#[must_use]
pub(crate) fn needs_vitest_mock_runtime(mir: &Mir) -> bool {
    any_rvalue_needs(mir, |rvalue| {
        matches!(
            rvalue,
            Rvalue::VitestMockFn { .. }
                | Rvalue::VitestMockCalledTimes { .. }
                | Rvalue::VitestMockCalledWith { .. }
                | Rvalue::VitestMockLastResolvedWith { .. }
        )
    })
}

/// Returns true when generated Rust reads/writes/probes a host-global override
/// slot and therefore needs the `SmeltHostOverride` enum + helper prelude.
///
/// This is the *pay-for-use* gate: it is `true` only when the crate actually
/// contains a `globalThis.<Name> =` write somewhere (the frontend only lowers
/// reads/presence to these rvalues for such written names), so a crate that
/// never reassigns a host global emits byte-identical output to before.
#[must_use]
pub(crate) fn needs_host_override_runtime(mir: &Mir) -> bool {
    any_rvalue_needs(mir, |rvalue| {
        matches!(
            rvalue,
            Rvalue::HostGlobalRead { .. }
                | Rvalue::HostGlobalWrite { .. }
                | Rvalue::HostGlobalPresent { .. }
        )
    })
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

/// Returns whether a set element type backs a plain Rust `HashSet`.
///
/// Mirrors [`FunctionEmitter::type_is_hash_set_key_safe`] at module scope so
/// the `needs_unknown` gate can decide whether a `Set` forces the `SmeltJsSet`
/// runtime (and therefore the unknown carrier) on. Only value-equality
/// primitives back a `HashSet`; everything else uses `SmeltJsSet`.
fn module_hash_set_key_safe(mir: &Mir, ty: smelt_hir::TypeId) -> bool {
    match mir.types.get(ty) {
        Some(Type::Bool | Type::Int | Type::String) => true,
        Some(Type::Optional(inner) | Type::Future(inner)) => {
            module_hash_set_key_safe(mir, *inner)
        }
        Some(Type::Union(items)) => items.iter().all(|item| module_hash_set_key_safe(mir, *item)),
        _ => false,
    }
}

/// Returns true when generated Rust needs the opaque `unknown` carrier type.
#[must_use]
pub(crate) fn needs_unknown_type(mir: &Mir) -> bool {
    // `Type::Union` values are emitted as `SmeltUnknown` (see the type-text
    // helper) and their members are erased into that carrier, so a union
    // anywhere in the type table means the generated crate references
    // `SmeltUnknown` even when no explicit `UnknownCast`/`UnknownIs` statement
    // is present. A union-valued `Map`/dict literal is exactly such a case: the
    // dict-literal emitter erases each entry inline without materializing a
    // `Type::Unknown` in the table. Emit the carrier for unions too.
    // A generic class or interface emits inherent/impl blocks whose type
    // parameters are bounded by `IntoSmeltUnknown + SmeltFromUnknown` (see
    // `classes::class_impl_generics_text`). A generic free function emits the
    // same bounds on its own `fn name<T: ..>` signature (see
    // `classes::function_impl_generics_list`). Those bounds reference the
    // erasure traits, so the carrier and its traits must be emitted even when a
    // program has no explicit `unknown`/union value of its own — otherwise the
    // generic signature names traits that were never declared.
    mir.classes
        .iter()
        .any(|class| !class.type_params.is_empty())
        || mir
            .interfaces
            .iter()
            .any(|interface| !interface.type_params.is_empty())
        || mir
            .functions
            .iter()
            .any(|function| !function.type_params.is_empty())
        || mir
            .types
            .all()
            .iter()
            .any(|ty| matches!(ty, Type::Unknown | Type::Never | Type::Union(_)))
        // A `Set` whose element type is not a value-equality primitive is
        // emitted as the `SmeltJsSet` runtime container, whose SameValueZero
        // membership projects elements through `IntoSmeltUnknown`. That carrier
        // and its traits live in the `needs_unknown` prelude block, so such a
        // set forces the unknown runtime on.
        || mir
            .types
            .all()
            .iter()
            .any(|ty| matches!(ty, Type::Set(item) if !module_hash_set_key_safe(mir, *item)))
        // A source `Map` (`Type::JsMap`) always lowers to the `SmeltJsMap`
        // runtime container — even when string-keyed — because its
        // `IntoSmeltUnknown` stamps the `__smelt_map` identity marker. That
        // container and its `SmeltJsKeyEq`/erasure traits live in the
        // `needs_unknown` prelude block, so any `JsMap` forces the unknown
        // runtime on (a string-keyed `Map` would otherwise reference an
        // unemitted `SmeltJsMap`).
        || mir
            .types
            .all()
            .iter()
            .any(|ty| matches!(ty, Type::JsMap(_, _)))
        // A source `Promise<T>` (`Type::Future`) lowers to `SmeltFuture<T>`, and
        // the whole promise/future runtime — `SmeltFuture`, `SmeltPromise`,
        // `smelt_await_flatten`, and the eager-prefix priming waker — is emitted
        // inside this `needs_unknown` prelude block. A fully-typed async program
        // (no union/unknown/generic of its own) would otherwise reference
        // `SmeltFuture` without it being defined, so any future forces the runtime
        // on. Corpus programs never hit this because their erased values already
        // set `needs_unknown`; it only surfaces for wholly-typed async code.
        || mir
            .types
            .all()
            .iter()
            .any(|ty| matches!(ty, Type::Future(_)))
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

/// Returns true when generated Rust needs the identity-bearing `SmeltList<T>`
/// type. That is any program using a `Type::List` (which lowers to `SmeltList`),
/// plus every program needing `SmeltUnknown` (its erase/extract seam and the
/// `From<SmeltList>`/`IntoSmeltUnknown` impls reference `SmeltList`).
#[must_use]
pub(crate) fn needs_smelt_list(mir: &Mir) -> bool {
    needs_unknown_type(mir) || mir.types.all().iter().any(|ty| matches!(ty, Type::List(_)))
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
