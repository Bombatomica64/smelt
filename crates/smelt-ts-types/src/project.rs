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
/// not type `x`. `vi.fn` keeps the implementation's type, which is optional
/// as in vitest (`vi.fn<() => void>()` names the type alone).
///
/// The assertion API (`expect`, `expectTypeOf`, `describe`, ...) is
/// [`SmeltTestApi`]: callable with type arguments and chainable through any
/// member, as vitest's own types are. `any` refused a type argument
/// (`expectTypeOf(x).toEqualTypeOf<T>()` was TS2347, "untyped function calls
/// may not accept type arguments"), an error vitest's declarations never give.
const TEST_FRAMEWORK_TEXT: &str = r#"
type SmeltProcedure = (...args: any[]) => any;
interface SmeltMockState<F extends SmeltProcedure> {
  calls: Parameters<F>[];
  lastCall: Parameters<F> | undefined;
  results: { type: string; value: any }[];
  instances: any[];
  invocationCallOrder: number[];
  [member: string]: any;
}
type SmeltMock<F extends SmeltProcedure> = F & { mock: SmeltMockState<F>; [member: string]: any };
type SmeltMethod<T> = T extends SmeltProcedure ? T : SmeltProcedure;
interface SmeltVi {
  fn<F extends SmeltProcedure = SmeltProcedure>(implementation?: F): SmeltMock<F>;
  spyOn<T, K extends keyof T>(object: T, method: K, accessType?: "get" | "set"): SmeltMock<SmeltMethod<T[K]>>;
  [member: string]: any;
}
interface SmeltTestApi {
  <A = any, B = any, C = any>(...args: any[]): SmeltTestApi;
  [member: string]: SmeltTestApi;
}
interface SmeltTestContext {
  task: any;
  skip: (...args: any[]) => any;
  signal: AbortSignal;
  expect: SmeltTestApi;
  onTestFailed: (handler: SmeltProcedure) => void;
  onTestFinished: (handler: SmeltProcedure) => void;
  [member: string]: any;
}
type SmeltTestName = string | number | Function;
interface SmeltEach<Context> {
  (strings: TemplateStringsArray, ...values: any[]): (name: SmeltTestName, fn: (context: any) => any, options?: any) => void;
  <T extends any[] | [any]>(cases: ReadonlyArray<T>): (name: SmeltTestName, fn: (...args: T) => any, options?: any) => void;
  <T>(cases: ReadonlyArray<T>): (name: SmeltTestName, fn: (...args: T[]) => any, options?: any) => void;
}
interface SmeltCaseApi<Context> {
  (name: SmeltTestName, fn?: (context: Context) => any, options?: any): void;
  (name: SmeltTestName, options: object, fn?: (context: Context) => any): void;
  each: SmeltEach<Context>;
  for: SmeltEach<Context>;
  skip: SmeltCaseApi<Context>;
  only: SmeltCaseApi<Context>;
  todo: SmeltCaseApi<Context>;
  concurrent: SmeltCaseApi<Context>;
  sequential: SmeltCaseApi<Context>;
  shuffle: SmeltCaseApi<Context>;
  fails: SmeltCaseApi<Context>;
  skipIf(condition: any): SmeltCaseApi<Context>;
  runIf(condition: any): SmeltCaseApi<Context>;
  extend<T = {}>(fixtures: any): SmeltCaseApi<Context & T>;
}
type SmeltHook<Context> = (fn: (context: Context) => any, timeout?: number) => void;
declare module "vitest" {
  export const vi: SmeltVi;
  export const describe: SmeltCaseApi<void>, suite: SmeltCaseApi<void>;
  export const it: SmeltCaseApi<SmeltTestContext>, test: SmeltCaseApi<SmeltTestContext>, bench: SmeltCaseApi<void>;
  export const expect: SmeltTestApi, expectTypeOf: SmeltTestApi, assertType: SmeltTestApi, assert: SmeltTestApi;
  export const beforeAll: SmeltHook<any>, afterAll: SmeltHook<any>;
  export const beforeEach: SmeltHook<SmeltTestContext>, afterEach: SmeltHook<SmeltTestContext>;
  export const onTestFailed: (handler: SmeltProcedure) => void, onTestFinished: (handler: SmeltProcedure) => void;
  export type Mock<F extends SmeltProcedure = SmeltProcedure> = SmeltMock<F>;
  export type MockInstance<F extends SmeltProcedure = SmeltProcedure> = SmeltMock<F>;
  export type MockedFunction<F extends SmeltProcedure> = SmeltMock<F>;
  export type Mocked<T> = T & { [member: string]: any };
  export type TestContext = SmeltTestContext;
  export type TaskContext = SmeltTestContext;
  export type RunnerTestSuite = any;
  export type RunnerTestCase = any;
  export type RunnerTask = any;
  export type Suite = any;
  export type Test = any;
  export type ExpectStatic = SmeltTestApi;
  export type Assertion<T = any> = SmeltTestApi;
}
declare module "@effect/vitest" {
  export * from "vitest";
}
declare module "@fast-check/vitest" {
  export * from "vitest";
}
"#;

/// Whether every program already declares the module `specifier`: a test
/// framework Smelt models, or a module of the vendored `@types/node`
/// (`"fs"`, `"node:fs"`, `"timers/promises"`).
#[must_use]
pub fn declares_module(specifier: &str) -> bool {
    let double = format!("declare module \"{specifier}\"");
    let single = format!("declare module '{specifier}'");
    TEST_FRAMEWORK_TEXT.contains(&double)
        || crate::node_types::NODE_TYPE_FILES
            .iter()
            .any(|(_, text)| text.contains(&double) || text.contains(&single))
}

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
