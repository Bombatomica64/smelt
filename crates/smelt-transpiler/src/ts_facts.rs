//! TypeScript 7 checker facts for a manifest's TypeScript sources.
//!
//! Before the frontend lowers a crate, the transpiler type-checks every
//! TypeScript source of the dependency closure as one program with the
//! embedded TypeScript 7 checker (`smelt-ts-types`, behind the `ts7`
//! feature) and hands the resulting span-keyed facts to each module's
//! lowering through `FrontendOptions::ts_facts`. Each source is analysed
//! under exactly the path string it is lowered under, so the frontend looks
//! its own spans up with its own `path`.
//!
//! The facts are advisory input to lowering: when the checker is unavailable
//! (feature off, `SMELT_TS7=0`, or a checker failure) lowering proceeds on
//! annotations alone, exactly as before the checker was embedded.

use smelt_ts_model::TsTypeFacts;

use crate::lowering::SourceLang;
use crate::manifest::ManifestSource;

/// Environment switch: `SMELT_TS7=0` lowers without checker facts (A/B runs).
const DISABLE_VAR: &str = "SMELT_TS7";

/// Type-checks the TypeScript sources among `sources`; `None` when unavailable.
pub(crate) fn analyze_sources(sources: &[&ManifestSource]) -> Option<TsTypeFacts> {
    analyze_files(
        sources
            .iter()
            .filter(|source| matches!(source.lang, SourceLang::TypeScript | SourceLang::TypeScriptDeclaration))
            .map(|source| (source.path.display().to_string(), source.source.clone()))
            .collect(),
    )
}

/// Type-checks `(path, text)` TypeScript files as one program.
///
/// Facts are filed under each `path` exactly as given (the string the
/// frontend lowers the file under), while the file is placed in the program
/// at its absolute location so relative imports resolve as on disk.
#[cfg(feature = "ts7")]
pub(crate) fn analyze_files(files: Vec<(String, String)>) -> Option<TsTypeFacts> {
    if files.is_empty() || std::env::var(DISABLE_VAR).is_ok_and(|value| value == "0") {
        return None;
    }
    let inputs: Vec<smelt_ts_types::SourceInput> = files
        .into_iter()
        .map(|(path, text)| {
            let located = std::path::absolute(&path).map(|location| location.display().to_string());
            let input = smelt_ts_types::SourceInput::new(path, text);
            match located {
                Ok(location) => input.located_at(location),
                Err(_) => input,
            }
        })
        .collect();
    crate::timing::measure("manifest.ts7_check", || smelt_ts_types::analyze(inputs)).ok()
}

/// Without the `ts7` feature there is no checker: lowering uses annotations only.
#[cfg(not(feature = "ts7"))]
pub(crate) fn analyze_files(_files: Vec<(String, String)>) -> Option<TsTypeFacts> {
    let _ = DISABLE_VAR;
    None
}
