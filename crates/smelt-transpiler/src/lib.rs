//! The `smelt` transpiler: manifest handling, lowering orchestration, reports
//! and the CLI dispatch the `smelt` binary (`main.rs`) calls into.

#![expect(
    clippy::type_complexity,
    reason = "CLI command helpers currently use boxed dynamic errors directly"
)]
#![expect(
    clippy::or_fun_call,
    reason = "manifest default construction is not performance-sensitive"
)]
#![expect(
    clippy::missing_const_for_fn,
    reason = "configuration accessors use non-const Option helpers on current MSRV"
)]
#![expect(
    clippy::exhaustive_enums,
    reason = "CLI command and config enums are internal to this crate"
)]
#![expect(
    clippy::exhaustive_structs,
    reason = "CLI parser structs are internal clap data models"
)]
#![expect(
    clippy::use_debug,
    reason = "dump commands intentionally expose debug forms for development inspection"
)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "CLI submodules use crate-visible helpers even though the crate root keeps modules private"
)]
#![allow(
    clippy::too_many_lines,
    reason = "manifest lowering orchestration is intentionally kept in one helper during the active feature phase"
)]

pub mod cli;
pub mod cli_parser;
pub mod config;
pub mod config_parser;
mod diagnostics;
mod erasure;
mod list_escape_report;
mod lowering;
mod manifest;
mod package_sources;
mod pipeline;
mod probe;
mod python;
pub mod report;
mod sound;
mod sound_overlay;
mod source_files;
mod specialization;
pub mod stubs;
mod test_report;
mod timing;
mod ts_facts;
#[cfg(feature = "ts7")]
mod ts7_parity;
#[cfg(feature = "ts7")]
mod ts7_parity_report;
mod typedness_report;
mod unknown_report;

/// Result type used by CLI command handlers.
pub type CliResult<T> = Result<T, Box<dyn std::error::Error>>;
