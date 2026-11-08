//! CLI argument parsing and command definitions.

use clap::{ArgGroup, Args as ClapArgs, Parser, Subcommand, ValueEnum};

use crate::report::ReportFormat;

/// Top-level CLI arguments for the smelt command.
#[derive(Parser, Debug)]
#[command(
    name = "smelt",
    version,
    about = "Smelt your TypeScript and Python into Rust",
    long_about = "Smelt transpiles TypeScript and Python into a Rust crate.\n\n\
                  Beyond `build` and `check`, the tooling subcommands drive the \
                  porting campaign: `probe` reports how far a project transpiles \
                  and groups the remaining blockers; `rust-diagnostics` and \
                  `rust-test-report` summarize a generated crate's `cargo \
                  check`/`cargo test` output as stable Markdown for hand-off."
)]
pub struct Args {
    /// Path to Smelt.toml (defaults to `./Smelt.toml`).
    #[arg(long, global = true, value_name = "PATH")]
    pub manifest_path: Option<String>,

    /// Command to execute.
    #[command(subcommand)]
    pub command: Command,
}

/// Available CLI subcommands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Read manifest, transpile, and emit a Rust crate
    Build {
        /// `--hir` / `--hir-debug`: print HIR after frontend lowering and stop.
        #[command(flatten)]
        hir: HirFlags,

        /// `--define KEY=VALUE`.
        #[command(flatten)]
        define: DefineArgs,

        /// Set an erasure reason's severity: `<reason>[:<glob>]=<allow|warn|deny>`
        /// (`all` for every reason). Repeatable; overrides `[erasure]`.
        #[arg(long = "erasure", value_name = "REASON=LEVEL")]
        erasure: Vec<String>,

        /// Lower the program even when the type checkers (TypeScript's, ty for
        /// Python) report errors, or cannot run. By default `build` refuses:
        /// Smelt's lowering relies on a program that type-checks. Same as
        /// `[check] type-errors = "allow"`; errors print as warnings.
        #[arg(long)]
        allow_type_errors: bool,
    },
    /// Type-check and validate without generating Rust
    ///
    /// The default `human` format also writes `.d.ts` and `.pyi` declaration
    /// stubs next to each source file; the `json` format writes nothing to disk.
    Check {
        /// Diagnostic output: `human` (default, fail-fast) or `json` (collect
        /// every module's categorized diagnostics in one recoverable pass).
        #[arg(long = "message-format", value_enum, default_value_t = MessageFormat::Human)]
        message_format: MessageFormat,

        /// `--define KEY=VALUE`.
        #[command(flatten)]
        define: DefineArgs,

        /// Set an erasure reason's severity: `<reason>[:<glob>]=<allow|warn|deny>`
        /// (`all` for every reason). Repeatable; overrides `[erasure]`.
        #[arg(long = "erasure", value_name = "REASON=LEVEL")]
        erasure: Vec<String>,

        /// Fail when a type checker (TypeScript's, or ty for Python) reports an
        /// error or cannot run, as `build` does by default. Off by default for
        /// `check`, which reports: the checker sees only the dependency
        /// closure, so a project without `@types/node` or with ambient test
        /// globals gets errors its real `tsc` setup does not.
        #[arg(long)]
        deny_type_errors: bool,
    },
    /// Scaffold a new smelt project in a directory of that name
    New {
        /// Project name
        name: String,

        /// Use Python as the entry language instead of TypeScript
        #[arg(long)]
        python: bool,
    },

    /// Print compact HIR for a single source file
    DumpHir {
        /// Print the full debug representation instead of compact HIR
        #[arg(long)]
        debug: bool,

        /// Input source file path.
        file: String,
    },

    /// Print the MIR for a single source file (debug)
    DumpMir {
        /// Input source file path.
        file: String,
    },

    /// Print the raw parser AST for a single source file (Python only for now)
    DumpAst {
        /// Input source file path.
        file: String,
    },

    /// Run cargo check for a Rust crate and summarize diagnostics as Markdown
    #[command(long_about = "Run `cargo check --message-format=json` for a \
        generated crate and render a Markdown report. Diagnostics are grouped by \
        rustc code and first message line and sorted by count (desc), then \
        level, then code, so reports diff cleanly between runs and the biggest \
        warning/error classes appear first.")]
    RustDiagnostics {
        /// Path to the generated crate `Cargo.toml`.
        #[arg(long = "cargo-manifest", value_name = "PATH")]
        cargo_manifest: String,

        /// Write the Markdown report to this file instead of stdout.
        #[arg(long, value_name = "PATH")]
        output: Option<String>,
    },

    /// Run generated Rust tests and write a compact Markdown investigation report
    #[command(long_about = "Run focused, guarded, and/or full generated `cargo \
        test` runs and render a compact Markdown report. Use `--focus` to drill \
        into a failing family, `--guard` to prove a prior fix still passes, and \
        `--full` for the whole-suite inventory and largest failing groups. \
        Requires at least one of `--focus`, `--guard`, or `--full`.")]
    #[command(group(
        ArgGroup::new("runs").args(["focus", "guard", "full"]).required(true).multiple(true)
    ))]
    RustTestReport {
        /// Path to the generated Rust crate `Cargo.toml`.
        #[arg(long = "cargo-manifest", value_name = "PATH")]
        cargo_manifest: String,

        /// Smelt.toml to transpile/build before running the generated tests.
        #[arg(long = "build-manifest", value_name = "PATH")]
        build_manifest: Option<String>,

        /// Let the `--build-manifest` build lower a program the type checkers
        /// reject; see `build --allow-type-errors`.
        #[arg(long, requires = "build_manifest")]
        allow_type_errors: bool,

        /// Test filter to investigate; repeat for independent focused runs.
        #[arg(long, value_name = "FILTER")]
        focus: Vec<String>,

        /// Regression test filter to protect while investigating; repeat as needed.
        #[arg(long, value_name = "FILTER")]
        guard: Vec<String>,

        /// Run the complete generated Rust test suite after focused runs.
        #[arg(long)]
        full: bool,

        /// Previous JSON report (`--json-output`) used to compute resolved and
        /// newly failing tests.
        #[arg(long = "baseline-report", value_name = "PATH")]
        baseline_report: Option<String>,

        /// Include grouped `cargo check` diagnostics in the Markdown report.
        #[arg(long)]
        diagnostics: bool,

        /// Suppress generated Rust warnings (RUSTFLAGS=-Awarnings) while testing.
        #[arg(long)]
        suppress_warnings: bool,

        /// Format and destination of the report.
        #[command(flatten)]
        report: ReportArgs,

        /// Also write the report as JSON to this file, from the same run.
        #[arg(long = "json-output", value_name = "PATH")]
        json_output: Option<String>,
    },

    /// Probe how far a manifest transpiles and enumerate blockers by category
    #[command(long_about = "Attempt a whole-crate build, then lower every source \
        file with shared manifest declarations and group the remaining blockers. On success, optionally \
        run the generated test suite (`--run-tests`). Blocker classes are sorted \
        by occurrences (desc), then affected files, then class name; long `oxc` \
        AST dumps in messages are elided to a short class key in the table, with \
        the full text kept in a collapsible detail section.")]
    Probe {
        /// Also run the generated `cargo test` suite when the crate transpiles.
        #[arg(long = "run-tests")]
        run_tests: bool,

        /// `--define KEY=VALUE`.
        #[command(flatten)]
        define: DefineArgs,

        /// Let the probe's whole-crate build lower a program the type checkers
        /// reject; see `build --allow-type-errors`.
        #[arg(long)]
        allow_type_errors: bool,

        /// Format and destination of the report.
        #[command(flatten)]
        report: ReportArgs,
    },

    /// Measure and gate `SmeltUnknown` usage in generated Rust output
    #[command(
        name = "smelt-unknown-report",
        long_about = "Scan generated `.rs` files and count every `SmeltUnknown` \
        occurrence, classifying each into runtime-prelude scaffolding, a \
        legitimate dynamic boundary (erased closures/interop, promises, boundary \
        adapters, runtime narrowing), or avoidable internal erasure (program \
        storage/signatures a concrete type, union, or generic could carry). The \
        scan is textual and deterministic — grouped by occurrence shape, sorted \
        by count — so numbers diff cleanly and can be stored as a checked-in \
        baseline. Pass `--baseline` to surface avoidable-erasure regressions; by \
        default the command stays advisory (exit 0) so intentional boundary \
        additions never block CI. Add `--fail-on-regression` to make a rise above \
        the baseline exit nonzero for a CI gate."
    )]
    SmeltUnknownReport {
        /// Roots to scan; each is a `.rs` file or a directory walked for `.rs`
        /// files. Repeat to scan several corpora at once.
        #[arg(value_name = "PATH", required = true)]
        roots: Vec<String>,

        /// Format, destination and baseline gate of the report.
        #[command(flatten)]
        gate: GateArgs,

        /// A `smelt-erasure.json` sidecar to explain avoidable erasure by
        /// reason and source line. Repeatable. A sidecar beside a scanned
        /// `src/` directory is read automatically.
        #[arg(long = "erasure", value_name = "PATH")]
        erasure: Vec<String>,
    },

    /// Compare Smelt's TypeScript types with the TypeScript 7 checker's
    #[command(
        name = "ts7-parity-report",
        long_about = "Lower each input to HIR as `smelt build` does, type-check the \
        same TypeScript sources with the embedded TypeScript 7 checker (tsrs), and \
        compare every HIR expression's type with the checker's type for the node at \
        the same span. Reports parity (the share of statically typed expressions \
        whose shape Smelt kept), the families of values TypeScript types concretely \
        that Smelt erased to Unknown, shape mismatches, and the checker's own errors. \
        Inputs are `Smelt.toml` manifests or `.ts` files."
    )]
    Ts7ParityReport {
        /// `Smelt.toml` manifests or `.ts` files to measure.
        #[arg(value_name = "PATH", required = true)]
        inputs: Vec<String>,

        /// `--define KEY=VALUE`.
        #[command(flatten)]
        define: DefineArgs,

        /// Format and destination of the report.
        #[command(flatten)]
        report: ReportArgs,
    },

    /// Measure and ratchet how much of a program reaches Rust typed
    #[command(
        name = "typedness-report",
        long_about = "Measure, on optimized MIR and through the Rust emitter's own \
        type rendering, how many values (locals), property reads and calls are \
        concrete, generic or erased (`SmeltUnknown`), per program. Inputs are \
        `smelt-typedness.json` sidecars (or directories holding one), `Smelt.toml` \
        manifests (lowered in-process) or `.ts`/`.py` files (each lowered \
        in-process as one program). With `--baseline`, a program whose erased \
        count rose for any kind is reported (the typed share is shown, not judged); \
        `--fail-on-regression` makes that exit nonzero. Design adapted from Bun's \
        `--aot` typedness tests (oven-sh/bun#45631)."
    )]
    TypednessReport {
        /// Sidecars, directories, manifests or source files to measure.
        #[arg(value_name = "PATH", required = true)]
        paths: Vec<String>,

        /// `--define KEY=VALUE`.
        #[command(flatten)]
        define: DefineArgs,

        /// Format, destination and baseline gate of the report.
        #[command(flatten)]
        gate: GateArgs,

        /// A baseline program that is gone on purpose: it stops being judged,
        /// and `--update-baseline` removes its record. Repeatable. Any other
        /// baseline program that is not measured fails the gate.
        #[arg(long, value_name = "PROGRAM", requires = "baseline")]
        retire_program: Vec<String>,
    },

    /// Classify list-typed MIR locals as escaping, aliased, or frame-local
    #[command(
        name = "list-escape-report",
        long_about = "Lower a Smelt manifest to optimized MIR and classify every \
        list-typed local in every function and closure body: `escaping` (a handle \
        on the buffer can be observed outside the frame), `aliased` (confined but \
        named by more than one live local), or `local-mutated`/`local-immutable` \
        (confined and singly named). `Type::List` lowers to `SmeltList<T>` = \
        `Rc<RefCell<Vec<T>>>`, which costs two heap allocations per list where a \
        plain `Vec<T>` costs one, so the confined population is what a tiered \
        representation could win. The analysis is conservative in one direction \
        only: anything it cannot prove confined is reported as escaping. Use \
        `--function` to expand named functions into a per-local table."
    )]
    ListEscapeReport {
        /// Smelt manifest to lower (`Smelt.toml`).
        #[arg(value_name = "PATH")]
        manifest: String,

        /// `--define KEY=VALUE`.
        #[command(flatten)]
        define: DefineArgs,

        /// Format and destination of the report.
        #[command(flatten)]
        report: ReportArgs,

        /// Expand this function into a per-local table; repeat for several.
        #[arg(long, value_name = "NAME")]
        function: Vec<String>,

        /// Rows kept in the "top bodies by confined list locals" table.
        #[arg(long, default_value_t = 25, value_name = "N")]
        top: usize,
    },

    /// Remove the output target directory
    Clean {
        /// Remove files smelt did not write without asking
        #[arg(long)]
        force: bool,
    },
    /// Print the JSON Schema for Smelt.toml
    DumpSchema,
}

/// How `smelt build` prints HIR instead of building.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HirMode {
    /// Compact HIR (`--hir`).
    Compact,
    /// Full debug HIR (`--hir-debug`).
    Debug,
}

/// The two spellings of [`HirMode`] on `smelt build`; at most one is given.
#[derive(ClapArgs, Debug, Clone, Copy)]
pub struct HirFlags {
    /// Print compact HIR after frontend lowering and stop
    #[arg(long, conflicts_with = "hir_debug")]
    hir: bool,

    /// Print full debug HIR after frontend lowering and stop
    #[arg(long = "hir-debug")]
    hir_debug: bool,
}

impl HirFlags {
    /// The requested HIR mode, or `None` to build normally.
    #[must_use]
    pub const fn mode(self) -> Option<HirMode> {
        match (self.hir, self.hir_debug) {
            (_, true) => Some(HirMode::Debug),
            (true, false) => Some(HirMode::Compact),
            (false, false) => None,
        }
    }
}

/// The `--format` and `--output` every report takes.
#[derive(ClapArgs, Debug)]
pub struct ReportArgs {
    /// Report format: `md` (default) or `json`.
    #[arg(long, value_enum, default_value_t = ReportFormat::Markdown, value_name = "FORMAT")]
    pub format: ReportFormat,

    /// Write the report to this file instead of stdout.
    #[arg(long, value_name = "PATH")]
    pub output: Option<String>,
}

/// The arguments of a report gated by a checked-in baseline.
#[derive(ClapArgs, Debug)]
pub struct GateArgs {
    /// Report format: `md` (default) or `json` (also the baseline format).
    #[arg(long, value_enum, default_value_t = ReportFormat::Markdown, value_name = "FORMAT")]
    pub format: ReportFormat,

    /// Prior JSON report to compare against.
    #[arg(long, value_name = "PATH")]
    pub baseline: Option<String>,

    /// Exit nonzero on a regression against `--baseline`. Off by default: the
    /// report stays advisory unless a gate opts in.
    #[arg(long, requires = "baseline")]
    pub fail_on_regression: bool,

    /// Rewrite `--baseline` with what was measured, keeping the old record
    /// wherever the measurement regressed.
    #[arg(long, requires = "baseline")]
    pub update_baseline: bool,

    /// Write the report to this file instead of stdout.
    #[arg(long, value_name = "PATH")]
    pub output: Option<String>,
}

/// Diagnostic output of `smelt check` (`--message-format`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum MessageFormat {
    /// Fail fast with human-readable errors, writing declaration stubs.
    Human,
    /// Collect every module's categorized diagnostics as JSON.
    Json,
}

/// `--define`, on every command that lowers a manifest.
#[derive(ClapArgs, Debug, Clone, Default)]
pub struct DefineArgs {
    /// Define a build-time value, like Bun's `--define`: `KEY=VALUE` where
    /// VALUE is JSON text, e.g. `--define 'process.env.NODE_ENV="production"'`.
    /// Repeatable; overrides `[build] define` in Smelt.toml. Smelt consults
    /// defines only to decide whether a package's typed version
    /// (`.sound/`, `@sound/*`) applies; it does not substitute them.
    #[arg(long = "define", value_name = "KEY=VALUE", value_parser = parse_define)]
    pub values: Vec<(String, String)>,
}

/// Parse one `--define KEY=VALUE` argument, splitting at the first `=`.
fn parse_define(argument: &str) -> Result<(String, String), String> {
    let (key, value) = argument
        .split_once('=')
        .ok_or_else(|| format!("`--define {argument}` is not KEY=VALUE"))?;
    if key.is_empty() {
        return Err(format!("`--define {argument}` has an empty KEY"));
    }
    Ok((key.to_owned(), value.to_owned()))
}

#[cfg(test)]
mod tests {
    //! Parsing of the typed CLI options, in their existing spellings.

    use super::{Args, Command, HirMode, MessageFormat, ReportFormat};
    use clap::Parser as _;

    /// Parse a command line, panicking with clap's message on failure.
    fn parse(argv: &[&str]) -> Command {
        Args::try_parse_from(argv).map(|args| args.command).expect("parses")
    }

    #[test]
    fn hir_flags_map_to_one_mode() {
        // `None` when the command is not `build` at all.
        let mode = |argv: &[&str]| {
            if let Command::Build { hir, .. } = parse(argv) { Some(hir.mode()) } else { None }
        };
        assert_eq!(mode(&["smelt", "build"]), Some(None));
        assert_eq!(mode(&["smelt", "build", "--hir"]), Some(Some(HirMode::Compact)));
        assert_eq!(mode(&["smelt", "build", "--hir-debug"]), Some(Some(HirMode::Debug)));
        assert!(Args::try_parse_from(["smelt", "build", "--hir", "--hir-debug"]).is_err());
    }

    #[test]
    fn formats_keep_their_spellings() {
        for (spelling, expected) in
            [("md", ReportFormat::Markdown), ("markdown", ReportFormat::Markdown), ("json", ReportFormat::Json)]
        {
            assert!(matches!(
                parse(&["smelt", "probe", "--format", spelling]),
                Command::Probe { report, .. } if report.format == expected
            ));
        }
        assert!(Args::try_parse_from(["smelt", "probe", "--format", "yaml"]).is_err());
        assert!(matches!(
            parse(&["smelt", "check", "--message-format", "json"]),
            Command::Check { message_format: MessageFormat::Json, .. }
        ));
        assert!(matches!(
            parse(&["smelt", "check"]),
            Command::Check { message_format: MessageFormat::Human, .. }
        ));
    }

    /// Every command that lowers a manifest takes `--define`, so it can agree with
    /// `build` on which typed versions apply; one that lowers no manifest does not.
    #[test]
    fn define_is_on_every_manifest_command() {
        let define = ["--define", "K=1"];
        for command in [
            &["smelt", "build"][..],
            &["smelt", "check"],
            &["smelt", "probe"],
            &["smelt", "list-escape-report", "Smelt.toml"],
            &["smelt", "typedness-report", "Smelt.toml"],
            &["smelt", "ts7-parity-report", "Smelt.toml"],
        ] {
            let arguments = [command, &define[..]].concat();
            assert!(Args::try_parse_from(&arguments).is_ok(), "{command:?}");
        }
        assert!(Args::try_parse_from(["smelt", "dump-hir", "a.ts", "--define", "K=1"]).is_err());
    }

    /// A gate with nothing to compare against would pass vacuously, and an
    /// update with nowhere to write would do nothing, so clap refuses both.
    #[test]
    fn gate_flags_require_a_baseline() {
        for report in [["smelt", "smelt-unknown-report", "src"], ["smelt", "typedness-report", "src"]] {
            for flag in ["--fail-on-regression", "--update-baseline"] {
                let mut argv = report.to_vec();
                argv.push(flag);
                let error = Args::try_parse_from(&argv).expect_err("no baseline");
                assert_eq!(error.kind(), clap::error::ErrorKind::MissingRequiredArgument, "{argv:?}");
                argv.extend(["--baseline", "base.json"]);
                assert!(Args::try_parse_from(&argv).is_ok(), "{argv:?}");
            }
        }
    }

    #[test]
    fn rust_test_report_needs_a_run() {
        assert!(Args::try_parse_from(["smelt", "rust-test-report", "--cargo-manifest", "C"]).is_err());
        for run in [&["--full"][..], &["--focus", "f"], &["--guard", "g", "--full"]] {
            let mut argv = vec!["smelt", "rust-test-report", "--cargo-manifest", "C"];
            argv.extend_from_slice(run);
            assert!(Args::try_parse_from(argv).is_ok(), "{run:?}");
        }
    }
}
