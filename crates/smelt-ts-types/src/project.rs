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

/// Path of the declaration of the test frameworks Smelt models itself.
const TEST_FRAMEWORK_DECLARATIONS: &str = "/smelt/__smelt_test_frameworks.d.ts";

/// The test-framework surface Smelt lowers natively, as the checker sees it.
///
/// The program holds only the project's own sources, so `vitest` would resolve to
/// nothing and `vi.fn(impl)` would be `any`, losing the signature of every mock:
/// `memoize(vi.fn((a: number[]) => 0), { getCacheKey: (x) => x.join() })` could
/// not type `x`. `vi.fn` keeps the implementation's type. Everything else stays
/// `any`, which is what an unresolved import answers.
const TEST_FRAMEWORK_TEXT: &str = r#"
type SmeltMock<F> = F & { mock: any; [member: string]: any };
interface SmeltVi {
  fn<F extends (...args: any[]) => any>(implementation: F): SmeltMock<F>;
  fn(): SmeltMock<(...args: any[]) => any>;
  [member: string]: any;
}
declare module "vitest" {
  export const vi: SmeltVi;
  export const describe: any, it: any, test: any, expect: any, expectTypeOf: any, assertType: any;
  export const beforeAll: any, beforeEach: any, afterAll: any, afterEach: any;
}
declare module "@effect/vitest" {
  export const vi: SmeltVi;
  export const describe: any, it: any, test: any, expect: any, expectTypeOf: any, assertType: any;
  export const beforeAll: any, beforeEach: any, afterAll: any, afterEach: any;
}
declare module "@fast-check/vitest" {
  export const vi: SmeltVi;
  export const describe: any, it: any, test: any, expect: any, expectTypeOf: any, assertType: any;
  export const beforeAll: any, beforeEach: any, afterAll: any, afterEach: any;
}
"#;

/// A whole program for the checker: the sources it walks, declarations it
/// only reads, and how bare specifiers resolve.
///
/// The transpiler builds one from the module graph it lowers, so each bare
/// specifier names the very file Smelt resolved it to (`module_paths`), and a
/// module Smelt provides without TypeScript source (a Python module imported
/// from TypeScript) is a `declarations` entry at the path the import names.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct CheckProgram {
    /// Files whose types are collected, as for [`crate::analyze`].
    pub sources: Vec<SourceInput>,
    /// Files in the program that are only read (generated declarations).
    pub declarations: Vec<SourceInput>,
    /// Bare specifier → the location (or path) of the source or declaration
    /// it resolves to.
    pub module_paths: std::collections::BTreeMap<String, String>,
}

impl CheckProgram {
    /// A program of `sources` with nothing else.
    #[must_use]
    pub fn of_sources(sources: Vec<SourceInput>) -> Self {
        Self { sources, ..Self::default() }
    }

    /// This program with `declarations` read alongside its sources.
    #[must_use]
    pub fn with_declarations(mut self, declarations: Vec<SourceInput>) -> Self {
        self.declarations = declarations;
        self
    }

    /// This program with bare specifiers resolving through `module_paths`.
    #[must_use]
    pub fn with_module_paths(mut self, module_paths: std::collections::BTreeMap<String, String>) -> Self {
        self.module_paths = module_paths;
        self
    }
}

/// The program path of a file located at `location`, as [`SourceInput`] places it.
fn program_path_of(location: &str) -> String {
    format!("{ROOT}/{}", normalize_segments(&location.replace('\\', "/")))
}

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
        program_path_of(self.location.as_ref().unwrap_or(&self.path))
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

/// Renders the `tsconfig.json` for `program`.
///
/// The options are the strict profile Smelt's check pipeline requires
/// (`specs/check-pipeline.md`): `strict` (exact optional properties off, as
/// in `tsc --strict`), the newest ECMAScript lib (Smelt models ES2024 APIs
/// such as `String.prototype.isWellFormed`; one it does not model is refused
/// by lowering, not by the checker) with the DOM libs that type the web
/// globals Smelt models (`URL`, `fetch`, `TextEncoder`) and Node's own
/// declarations (`types: ["node"]`, the vendored `@types/node`) for the host
/// globals and `node:` modules, bundler resolution so extensionless and `.ts`
/// relative imports between inputs resolve, `paths` for every bare specifier
/// the transpiler resolved, and `moduleDetection: force` because Smelt
/// treats every file as a module (an import-free file is not a global script
/// whose declarations collide).
fn tsconfig_json(program: &CheckProgram) -> String {
    let mut files = format!("\"{}\"", TEST_FRAMEWORK_DECLARATIONS.trim_start_matches("/smelt/"));
    for input in program.sources.iter().chain(&program.declarations) {
        let _ = write!(files, ",\"{}\"", input.root_relative_path().replace('"', "\\\""));
    }
    let paths = program
        .module_paths
        .iter()
        .map(|(specifier, location)| {
            format!("{}: [{}]", json_string(specifier), json_string(&program_path_of(location)))
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"{{
  "compilerOptions": {{
    "strict": true,
    "target": "esnext",
    "lib": ["esnext", "dom", "dom.iterable"],
    "module": "esnext",
    "moduleDetection": "force",
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "noEmit": true,
    "skipLibCheck": true,
    "types": ["node"],
    "paths": {{ {paths} }}
  }},
  "files": [{files}]
}}"#
    )
}

/// `text` as a JSON string literal.
fn json_string(text: &str) -> String {
    let mut quoted = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
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
pub(crate) fn build_program(program: &CheckProgram) -> Result<&'static Program, TsTypesError> {
    let config_path = format!("{ROOT}/tsconfig.json");
    let mut files: Vec<(String, String)> = program
        .sources
        .iter()
        .chain(&program.declarations)
        .map(|input| (input.program_path(), input.text.clone()))
        .collect();
    files.push((TEST_FRAMEWORK_DECLARATIONS.to_owned(), TEST_FRAMEWORK_TEXT.to_owned()));
    files.extend(
        crate::node_types::NODE_TYPE_FILES
            .iter()
            .map(|(path, text)| (format!("{ROOT}/{path}"), (*text).to_owned())),
    );
    files.push((config_path.clone(), tsconfig_json(program)));
    let memory: Arc<dyn FS> = Arc::new(vfstest::from_map(files, true));
    let fs: Arc<dyn FS> = Arc::new(bundled::wrap_fs(crate::layered_fs::LayeredFs::new(memory)));
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
