//! Every edition a generated crate may declare is one its code is valid in.
//!
//! The runtime is inlined into every generated crate and, with every flag on,
//! is the largest text the emitter ever writes; it uses let-chains and the
//! 2024 keyword rules. `rustc` parses and expands it under each accepted
//! [`RustEdition`] and the test fails on any error that is not a name the
//! standalone text cannot resolve (the generated crate's dependencies and its
//! program items are absent here): a syntax error, an edition gate, a feature
//! gate. Adding `2021` to [`RustEdition::ALL`] makes it fail.

#![expect(clippy::expect_used, reason = "a test fails fast on fixture setup")]

use std::process::Command;

use crate::runtime_prelude::rendered_text;
use crate::RustEdition;

/// Error codes of names a standalone runtime text cannot resolve, and of what
/// rustc reports only because of them.
/// (E0220/E0223: an associated type of a trait that did not resolve.)
const UNRESOLVED_CODES: &[&str] =
    &["E0220", "E0223", "E0405", "E0412", "E0422", "E0423", "E0425", "E0432", "E0433", "E0531"];

/// The errors `rustc` reports for `text` as a library under `edition`, as
/// `(code, message)`; a parse error has no code.
fn rustc_errors(text: &str, edition: RustEdition) -> Vec<(Option<String>, String)> {
    let dir = tempfile::tempdir().expect("temporary directory");
    let source = dir.path().join("runtime.rs");
    std::fs::write(&source, text).expect("write runtime");
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let output = Command::new(rustc)
        .args(["--edition", edition.as_str(), "--crate-type", "lib", "--emit", "metadata", "--error-format", "short"])
        .arg("-o")
        .arg(dir.path().join("out.rmeta"))
        .arg(&source)
        .output()
        .expect("run rustc");
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .filter_map(|line| {
            // `path:line:col: error[E0425]: message`, or `error: message`.
            let (_, rest) = line.split_once("error")?;
            let (code, message) = match rest.strip_prefix('[') {
                Some(coded) => {
                    let (code, message) = coded.split_once("]: ")?;
                    (Some(code.to_owned()), message)
                }
                None => (None, rest.strip_prefix(": ")?),
            };
            (!message.starts_with("aborting due to")).then(|| (code, message.to_owned()))
        })
        .collect()
}

/// The all-flags-on runtime is valid code under every accepted edition.
#[test]
fn every_accepted_edition_parses_the_runtime() {
    let runtime = rendered_text(&|_| true);
    for edition in RustEdition::ALL {
        let invalid = rustc_errors(&runtime, edition)
            .into_iter()
            .filter(|(code, _)| !code.as_deref().is_some_and(|known| UNRESOLVED_CODES.contains(&known)))
            .collect::<Vec<_>>();
        assert!(invalid.is_empty(), "the runtime is not valid {} code: {invalid:#?}", edition.as_str());
    }
}
