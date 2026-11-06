//! Writing the runtime into a generated crate.
//!
//! The runtime is real, compiled, unit-tested Rust in `smelt-runtime` (see that
//! crate's docs). This module is the emitter's half of the arrangement: it answers
//! the flags the runtime's markers name, from the program's
//! [`RuntimeDemand`](crate::runtime_demand::RuntimeDemand), and copies the rendered
//! text into the crate root in [`ORDER`]. Generated crates do not depend on
//! `smelt-runtime`: the text is inlined, so they stay self-contained and cannot
//! skew against a runtime version.
//!
//! # Failing loudly
//!
//! An order entry with no region, or a marker naming a flag this module does not
//! answer, is a build-breaking mistake rather than a silent omission:
//! [`emit_entries`] returns an [`EmitError`], and the tests in
//! `src/tests/runtime_manifest_tests.rs` walk the whole manifest, so a typo fails
//! the test suite rather than a user's build.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]

use smelt_mir::Mir;
use smelt_runtime::order::Entry;
#[cfg(test)]
use smelt_runtime::order::ORDER;
use smelt_runtime::source::{RenderedLine, holds, item_enabled, render_item};

use crate::emitter::EmitContext;
use crate::runtime_demand::Need;
use crate::{EmitError, rust::CodeWriter, stdlib};

/// Writes `entries` under the program's flags.
///
/// # Errors
///
/// Returns an [`EmitError`] when an entry names an item the runtime does not
/// define.
pub(crate) fn emit_entries(
    writer: &mut CodeWriter,
    entries: &[Entry],
    mir: &Mir,
    context: &EmitContext,
) -> Result<(), EmitError> {
    let flags = |name: &str| flag(context, name).unwrap_or(false);
    for entry in entries {
        match entry {
            Entry::Item(name) if item_enabled(name, &flags) => {
                let lines = render_item(name, &flags).ok_or_else(|| {
                    EmitError::new(format!("the runtime order names `{name}`, which smelt-runtime does not define"))
                })?;
                for line in lines {
                    match line {
                        RenderedLine::Text(text) => writer.line(text),
                        RenderedLine::Slot(slot) => emit_slot(writer, slot, mir)?,
                        _ => return Err(EmitError::new("unknown rendered runtime line")),
                    }
                }
            }
            Entry::Blank(expr) if holds(expr, &flags) => writer.blank_line(),
            _ => {}
        }
    }
    Ok(())
}

/// Renders the runtime under `flags`, for tests that read its text.
#[cfg(test)]
pub(crate) fn rendered_text(flags: &dyn Fn(&str) -> bool) -> String {
    rendered_entries(ORDER, flags)
}

/// Renders `entries` under `flags`, for tests that read their text.
///
/// Slots hold program text, so they render as nothing here.
#[cfg(test)]
pub(crate) fn rendered_entries(entries: &[Entry], flags: &dyn Fn(&str) -> bool) -> String {
    let mut text = String::new();
    for entry in entries {
        match entry {
            Entry::Item(name) if item_enabled(name, flags) => {
                for line in render_item(name, flags).unwrap_or_default() {
                    if let RenderedLine::Text(line) = line {
                        text.push_str(&line);
                        text.push('\n');
                    }
                }
            }
            Entry::Blank(expr) if holds(expr, flags) => text.push('\n'),
            _ => {}
        }
    }
    text
}

/// Writes the lines of a program-dependent `slot`.
fn emit_slot(writer: &mut CodeWriter, slot: &str, mir: &Mir) -> Result<(), EmitError> {
    match slot {
        "host_override_slots" => {
            use smelt_stdlib::runtime_symbols::host_override::{OVERRIDE_ENUM, SLOT_PREFIX};
            for (suffix, _name) in stdlib::host_override_slot_names(mir) {
                writer.line(format!(
                    "    static {SLOT_PREFIX}{suffix}: ::std::cell::RefCell<{OVERRIDE_ENUM}> = const {{ ::std::cell::RefCell::new({OVERRIDE_ENUM}::Native) }};",
                ));
            }
            Ok(())
        }
        _ => Err(EmitError::new(format!("the runtime names an unknown slot `{slot}`"))),
    }
}

/// Answers a flag named by a runtime marker, or `None` for a name nothing answers.
pub(crate) fn flag(context: &EmitContext, name: &str) -> Option<bool> {
    let demand = context.demand();
    match name {
        // A generated object keeps its origin slot for two different reasons.
        "reference_origins" => Some(context.has_reference_classes() || demand.has(Need::HostOrigins)),
        // Timers and `Date.now()` share one clock.
        "virtual_clock" => Some(demand.has(Need::Timers) || demand.has(Need::DateNow)),
        "crypto_any" => Some(
            demand.has(Need::CryptoRandomValues)
                || demand.has(Need::CryptoDigest)
                || demand.has(Need::CryptoKey),
        ),
        // `[[Construct]]` and the `instanceof` walk share one callable lookup.
        "callable_of" => Some(demand.has(Need::Construct) || demand.has(Need::InstanceOfValue)),
        "write_through" => Some(context.erased_view_write_through()),
        "reads_this" => Some(context.program_reads_this()),
        _ => need_of_flag(name).map(|need| demand.has(need)),
    }
}

/// The [`Need`] a marker's flag spells.
fn need_of_flag(name: &str) -> Option<Need> {
    Some(match name {
        "unknown" => Need::Unknown,
        "smelt_list" => Need::SmeltList,
        "object_ids" => Need::ObjectIds,
        "prim_set" => Need::PrimSet,
        "panic_route" => Need::PanicRoute,
        "bigint" => Need::BigInt,
        "generator" => Need::Generator,
        "iterator_result" => Need::IteratorResult,
        "timers" => Need::Timers,
        "date_now" => Need::DateNow,
        "date_timezone_offset" => Need::DateTimezoneOffset,
        "date" => Need::Date,
        "math_round" => Need::MathRound,
        "shared_captures" => Need::SharedCaptures,
        "uri_encode" => Need::UriEncode,
        "uri_decode" => Need::UriDecode,
        "json_indent" => Need::JsonIndent,
        "json_parse" => Need::JsonParse,
        "locale_compare" => Need::LocaleCompare,
        "headers" => Need::Headers,
        "response" => Need::Response,
        "request" => Need::Request,
        "body" => Need::Body,
        "body_host_method" => Need::BodyHostMethod,
        "form_data" => Need::FormData,
        "blob" => Need::Blob,
        "url_search_params" => Need::UrlSearchParams,
        "url" => Need::Url,
        "url_throwing" => Need::UrlThrowing,
        "text_encoder" => Need::TextEncoder,
        "text_decoder" => Need::TextDecoder,
        "byte_array" => Need::ByteArray,
        "data_view" => Need::DataView,
        "data_view_access" => Need::DataViewAccess,
        "crypto_random_values" => Need::CryptoRandomValues,
        "crypto_digest" => Need::CryptoDigest,
        "crypto_key" => Need::CryptoKey,
        "node_path" => Need::NodePath,
        "node_hash" => Need::NodeHash,
        "event_emitter" => Need::EventEmitter,
        "http_server" => Need::HttpServer,
        "fetch_server" => Need::FetchServer,
        "sqlite" => Need::Sqlite,
        "base64" => Need::Base64,
        "global_object" => Need::GlobalObject,
        "unknown_assign" => Need::UnknownAssign,
        "reg_exp_compile" => Need::RegExpCompile,
        "vitest_mock" => Need::VitestMock,
        "structured_clone" => Need::StructuredClone,
        "typeof" => Need::Typeof,
        "concat_spread" => Need::ConcatSpread,
        "define_properties" => Need::DefineProperties,
        "this_channel" => Need::ThisChannel,
        "this_read" => Need::ThisRead,
        "construct" => Need::Construct,
        "instance_of_value" => Need::InstanceOfValue,
        "host_override" => Need::HostOverride,
        "host_origins" => Need::HostOrigins,
        "erased_function" => Need::ErasedFunction,
        "erased_list_alias" => Need::ErasedListAlias,
        "optional_unknown_read" => Need::OptionalUnknownRead,
        "accessors" => Need::Accessors,
        "python_number_format" => Need::PythonNumberFormat,
        "dep_tokio" => Need::DepTokio,
        "dep_genawaiter" => Need::DepGenawaiter,
        "dep_reqwest" => Need::DepReqwest,
        "dep_serde_json" => Need::DepSerdeJson,
        "dep_regex" => Need::DepRegex,
        "dep_rand" => Need::DepRand,
        "dep_chrono" => Need::DepChrono,
        "dep_chrono_tz" => Need::DepChronoTz,
        "dep_url" => Need::DepUrl,
        "dep_unicode_normalization" => Need::DepUnicodeNormalization,
        "dep_uuid" => Need::DepUuid,
        "dep_sha" => Need::DepSha,
        "dep_hmac" => Need::DepHmac,
        "dep_md5" => Need::DepMd5,
        _ => return None,
    })
}
