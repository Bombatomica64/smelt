//! In-memory tsrs program construction.
//!
//! Smelt hands the checker the exact source text it parsed with oxc, so the
//! program is built over an in-memory file system rooted at [`ROOT`] instead
//! of the real disk. The TypeScript lib files come from tsrs's bundled copies
//! (`include_str!`-embedded), which keeps type facts hermetic: the same input
//! text always produces the same types, whatever is installed on the machine.
//!
//! The program is single-threaded. tsrs, like TypeScript, may split a program
//! across several checkers, and types from different checkers cannot be
//! compared. Smelt needs one consistent type identity across every file, so
//! everything goes through one checker.

#![expect(
    clippy::redundant_pub_crate,
    reason = "program construction is called from the crate root"
)]

use std::fmt::Write as _;
use std::sync::Arc;

use tsrs_compiler::{CompilerHost, Program, ProgramOptions, new_compiler_host, new_program};
use tsrs_core::{P, Tristate};
use tsrs_tsoptions::{self as tsoptions, ParseConfigHost};
use tsrs_vfs::{FS, bundled, vfstest};

use crate::TsTypesError;

/// Virtual directory holding the analysed sources and the generated `tsconfig.json`.
pub(crate) const ROOT: &str = "/smelt";

/// One source file to analyse.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct SourceInput {
    /// Path relative to the project root, with `/` separators (`src/index.ts`).
    pub path: String,
    /// Exact source text; byte offsets into it are the span keys of the results.
    pub text: String,
    /// Where the file lives, when `path` (the key results are filed under)
    /// is not itself a path that keeps relative imports resolvable, such as a
    /// working-directory-relative `../src/lib.ts`. Absolute paths are best.
    pub location: Option<String>,
}

impl SourceInput {
    /// Creates an input from a relative path and its source text.
    #[must_use]
    pub fn new(path: impl Into<String>, text: impl Into<String>) -> Self {
        Self { path: path.into(), text: text.into(), location: None }
    }

    /// Places the file at `location` in the program while keeping `path` as its key.
    #[must_use]
    pub fn located_at(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }

    /// Path of this file relative to [`ROOT`].
    ///
    /// Relative inputs keep their layout; absolute inputs (`/home/me/pkg/a.ts`)
    /// are re-rooted under [`ROOT`] with their full path, so relative imports
    /// between inputs resolve exactly as they do on disk.
    pub(crate) fn root_relative_path(&self) -> String {
        normalize_segments(&self.location.as_ref().unwrap_or(&self.path).replace('\\', "/"))
    }

    /// Absolute path of this file inside the virtual program.
    pub(crate) fn program_path(&self) -> String {
        format!("{ROOT}/{}", self.root_relative_path())
    }
}

/// Resolves `.` and `..` segments and drops leading `/` and `./`.
///
/// The in-memory file system keys files by their exact path, so a dependency
/// recorded as `pkg/app/../src/lib.ts` must be stored as `pkg/src/lib.ts` for
/// the relative import `../src/lib` to find it.
fn normalize_segments(path: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    segments.join("/")
}

/// Supplies the config parser with the virtual file system.
struct ConfigHost {
    /// File system the config file is read from.
    fs: Arc<dyn FS>,
}

impl ParseConfigHost for ConfigHost {
    fn fs(&self) -> &dyn FS {
        &*self.fs
    }

    fn get_current_directory(&self) -> &'static str {
        ROOT
    }
}

/// Renders the `tsconfig.json` for `inputs`.
///
/// The options are the strict profile Smelt's check pipeline requires
/// (`specs/check-pipeline.md`): `strict` (exact optional properties off, as
/// in `tsc --strict`), a modern target with the DOM libs that type the web
/// globals Smelt models (`URL`, `fetch`, `TextEncoder`), bundler resolution
/// so extensionless and `.ts` relative imports between inputs resolve, and
/// `moduleDetection: force` because Smelt treats every file as a module (an
/// import-free file is not a global script whose declarations collide).
fn tsconfig_json(inputs: &[SourceInput]) -> String {
    let mut files = String::new();
    for (index, input) in inputs.iter().enumerate() {
        if index > 0 {
            files.push(',');
        }
        let _ = write!(files, "\"{}\"", input.root_relative_path().replace('"', "\\\""));
    }
    format!(
        r#"{{
  "compilerOptions": {{
    "strict": true,
    "target": "es2023",
    "lib": ["es2023", "dom", "dom.iterable"],
    "module": "esnext",
    "moduleDetection": "force",
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "noEmit": true,
    "skipLibCheck": true,
    "types": []
  }},
  "files": [{files}]
}}"#
    )
}

/// Builds the program for `inputs`.
///
/// The program, its config host and its file system are leaked: tsrs hands out
/// `&'static` programs whose arena outlives every type and node it produced.
/// Smelt builds one program per transpile run, so the leak is bounded.
///
/// # Errors
///
/// Returns [`TsTypesError::Config`] when the generated `tsconfig.json` does
/// not parse; option diagnostics are reported as part of the error.
pub(crate) fn build_program(inputs: &[SourceInput]) -> Result<&'static Program, TsTypesError> {
    let config_path = format!("{ROOT}/tsconfig.json");
    let mut files: Vec<(String, String)> =
        inputs.iter().map(|input| (input.program_path(), input.text.clone())).collect();
    files.push((config_path.clone(), tsconfig_json(inputs)));
    let fs: Arc<dyn FS> = Arc::new(bundled::wrap_fs(vfstest::from_map(files, true)));
    let config_host: &'static ConfigHost =
        Box::leak(Box::new(ConfigHost { fs: Arc::clone(&fs) }));
    let (config, diagnostics) =
        tsoptions::get_parsed_command_line_of_config_file(&config_path, None, None, config_host, None);
    let Some(parsed) = config.filter(|_| diagnostics.is_empty()) else {
        let messages: Vec<&str> = diagnostics.iter().map(|diagnostic| diagnostic.message_text()).collect();
        return Err(TsTypesError::Config(messages.join("; ")));
    };
    let host: Arc<dyn CompilerHost> = new_compiler_host(ROOT, fs, &bundled::lib_path(), None, None, None);
    let mut options = ProgramOptions::new(P::new(parsed), host);
    options.single_threaded = Tristate::True;
    Ok(new_program(options))
}
