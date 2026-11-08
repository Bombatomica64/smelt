//! `smelt build` lowers only programs the type checkers accept (the type gate).
//!
//! An ill-typed TypeScript or Python program fails the build before anything is
//! written; `--allow-type-errors` or `[build] type-errors = "allow"` lowers it
//! anyway, printing the errors as warnings, and the frontend's checked-program
//! shortcuts then refuse what they would otherwise miscompile.

mod common;

use std::{fs, path::Path, process::Command};

use common::{TempProject, TestResult, ensure};

/// Writes a one-entry project whose entry is `entry` holding `source`, with
/// `extra` appended to the manifest.
fn project(dir: &Path, entry: &str, source: &str, extra: &str) -> TestResult {
    fs::create_dir_all(dir.join("src"))?;
    fs::write(dir.join("src").join(entry), source)?;
    fs::write(
        dir.join("Smelt.toml"),
        format!(
            "[project]\nname = \"gate\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"src/{entry}\"]\n\n[output]\ntarget = \"./dist\"\ncrate-name = \"gate\"\nbuild = false\n\n[runtime]\nclone-strategy = \"aggressive\"\n{extra}"
        ),
    )?;
    Ok(())
}

/// Outcome of one run: success and stderr.
type RunOutcome = (bool, String);

/// Runs `smelt` in `dir` with the checker on.
fn run(dir: &Path, args: &[&str]) -> TestResult<RunOutcome> {
    let output = Command::new(env!("CARGO_BIN_EXE_smelt"))
        .current_dir(dir)
        .args(args)
        .env_remove("SMELT_TS7")
        .output()?;
    Ok((output.status.success(), String::from_utf8_lossy(&output.stderr).into_owned()))
}

/// The refusal every gated failure carries.
const REFUSAL: &str = "Smelt lowers only programs that type-check";

/// A TypeScript program `tsc` rejects (TS2322 on line 2) that Smelt can lower.
const ILL_TYPED_TS: &str = "function f(x: string): void {\n  const n: number = x;\n  console.log(n);\n}\nf(\"a\");\n";

/// A TypeScript checker error fails `build` before the output exists.
#[cfg(feature = "ts7")]
#[test]
fn build_refuses_ill_typed_typescript() -> TestResult {
    let temp = TempProject::new()?;
    project(temp.path(), "main.ts", ILL_TYPED_TS, "")?;
    let (ok, stderr) = run(temp.path(), &["build"])?;
    ensure(!ok, "an ill-typed program built")?;
    ensure(stderr.contains("error[type-check]") && stderr.contains("TS2322"), stderr.clone())?;
    ensure(stderr.contains(REFUSAL), stderr)?;
    ensure(!temp.path().join("dist").exists(), "a refused build wrote its output")
}

/// `--allow-type-errors` lowers it, with the errors as warnings; so does the
/// manifest setting.
#[cfg(feature = "ts7")]
#[test]
fn allowing_type_errors_builds_ill_typed_typescript() -> TestResult {
    let temp = TempProject::new()?;
    project(temp.path(), "main.ts", ILL_TYPED_TS, "")?;
    let (ok, stderr) = run(temp.path(), &["build", "--allow-type-errors"])?;
    ensure(ok, stderr.clone())?;
    ensure(stderr.contains("warning[type-check]") && stderr.contains("TS2322"), stderr)?;
    ensure(temp.path().join("dist/src/main.rs").is_file(), "the allowed build wrote no crate")?;

    let manifest = TempProject::new()?;
    project(manifest.path(), "main.ts", ILL_TYPED_TS, "\n[build]\ntype-errors = \"allow\"\n")?;
    let (manifest_ok, manifest_stderr) = run(manifest.path(), &["build"])?;
    ensure(manifest_ok, manifest_stderr)
}

/// A well-typed program builds, and the checker says nothing.
#[cfg(feature = "ts7")]
#[test]
fn a_well_typed_program_builds() -> TestResult {
    let temp = TempProject::new()?;
    project(temp.path(), "main.ts", "const n: number = 2;\nconsole.log(n);\n", "")?;
    let (ok, stderr) = run(temp.path(), &["build"])?;
    ensure(ok, stderr.clone())?;
    ensure(!stderr.contains("type-check"), stderr)
}

/// With type errors allowed, a shortcut that relies on a checked program
/// refuses the source instead of miscompiling it: two implementations of one
/// function (TS2393), where hoisting the first would make the early call run
/// it while JavaScript runs the last.
#[cfg(feature = "ts7")]
#[test]
fn an_allowed_ill_typed_program_never_miscompiles_silently() -> TestResult {
    let temp = TempProject::new()?;
    let source = "console.log(f());\nfunction f(): number { return 1; }\nfunction f(): number { return 2; }\n";
    project(temp.path(), "main.ts", source, "")?;
    let (ok, stderr) = run(temp.path(), &["build", "--allow-type-errors"])?;
    ensure(!ok, "a duplicate implementation lowered")?;
    ensure(stderr.contains("duplicate function implementation `f`"), stderr)
}

/// A Python program ty rejects (an `int` parameter given a `str`).
const ILL_TYPED_PY: &str = "def twice(x: int) -> int:\n    return x * 2\n\n\nprint(twice(\"3\"))\n";

/// A ty error fails `build` before the output exists.
#[cfg(feature = "ty")]
#[test]
fn build_refuses_ill_typed_python() -> TestResult {
    let temp = TempProject::new()?;
    project(temp.path(), "main.py", ILL_TYPED_PY, "")?;
    let (ok, stderr) = run(temp.path(), &["build"])?;
    ensure(!ok, "an ill-typed Python program built")?;
    ensure(stderr.contains("error[type-check]") && stderr.contains("invalid-argument-type"), stderr.clone())?;
    ensure(stderr.contains("src/main.py:5:"), stderr.clone())?;
    ensure(stderr.contains(REFUSAL), stderr)?;
    ensure(!temp.path().join("dist").exists(), "a refused build wrote its output")
}

/// With type errors allowed, the ty error is a warning and the gate does not
/// refuse the Python program.
#[cfg(feature = "ty")]
#[test]
fn allowing_type_errors_passes_the_gate_for_python() -> TestResult {
    let temp = TempProject::new()?;
    project(temp.path(), "main.py", ILL_TYPED_PY, "")?;
    let (_, stderr) = run(temp.path(), &["build", "--allow-type-errors"])?;
    ensure(stderr.contains("warning[type-check]") && stderr.contains("invalid-argument-type"), stderr.clone())?;
    ensure(!stderr.contains(REFUSAL), stderr)
}

/// A well-typed Python program builds, and ty says nothing.
#[cfg(feature = "ty")]
#[test]
fn a_well_typed_python_program_builds() -> TestResult {
    let temp = TempProject::new()?;
    project(temp.path(), "main.py", "def twice(x: int) -> int:\n    return x * 2\n\n\nprint(twice(3))\n", "")?;
    let (ok, stderr) = run(temp.path(), &["build"])?;
    ensure(ok, stderr.clone())?;
    ensure(!stderr.contains("type-check"), stderr)
}

// ---------------------------------------------------------------------------
// The checker sees what Smelt resolves.
//
// Each test below is a valid program the gate used to refuse because the
// checker's program lacked a source Smelt's own resolution reaches, paired
// with an ill-typed twin through the SAME source. The twin proves the
// checker reads that source's real types, not an `any` that would let every
// program through. The typed overlay (`.sound/<pkg>/lay`, served from memory)
// is pinned by `typed_package_tests::laid_new_files_import_each_other`, and
// both directions of a cross-language import, under the default gate, by
// every case of `cross_language_matrix_tests`.
// ---------------------------------------------------------------------------

/// Writes `files` (relative path, text) under `dir`.
fn write_all(dir: &Path, files: &[(&str, &str)]) -> TestResult {
    for (relative, text) in files {
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, text)?;
    }
    Ok(())
}

/// Builds `entry` among `files` and returns the outcome; `extra` is
/// appended to the manifest.
fn build_files(files: &[(&str, &str)], entry: &str, extra: &str) -> TestResult<RunOutcome> {
    let temp = TempProject::new()?;
    let main = files
        .iter()
        .find(|(relative, _)| relative.ends_with(entry))
        .map_or("", |(_, text)| *text);
    project(temp.path(), entry, main, extra)?;
    write_all(temp.path(), files)?;
    run(temp.path(), &["build"])
}

/// Asserts a valid program builds without a checker word, and its ill-typed
/// twin is refused with `code`.
fn ensure_gate_sees(valid: RunOutcome, invalid: RunOutcome, code: &str) -> TestResult {
    let (ok, stderr) = valid;
    ensure(ok, format!("the valid program was refused:\n{stderr}"))?;
    ensure(!stderr.contains("type-check"), stderr)?;
    let (invalid_ok, invalid_stderr) = invalid;
    ensure(!invalid_ok, "the ill-typed twin built")?;
    ensure(
        invalid_stderr.contains("error[type-check]") && invalid_stderr.contains(code),
        invalid_stderr,
    )
}

/// Node's globals (`process`, from the vendored `@types/node`) type-check:
/// `process.env.HOME` is `string | undefined`, `process.argv` a `string[]`.
#[cfg(feature = "ts7")]
#[test]
fn node_globals_type_check() -> TestResult {
    let valid = "const home: string | undefined = process.env.HOME;\nconst args: string[] = process.argv.slice(2);\nconsole.log(typeof home, args.length);\n";
    let invalid = "const args: number[] = process.argv.slice(2);\nconsole.log(args.length);\n";
    ensure_gate_sees(
        build_files(&[("src/main.ts", valid)], "main.ts", "")?,
        build_files(&[("src/main.ts", invalid)], "main.ts", "")?,
        "TS2322",
    )
}

/// A bare import of a `[packages]` source type-checks against that source.
#[cfg(feature = "ts7")]
#[test]
fn declared_package_sources_type_check() -> TestResult {
    let package = "export function increment(value: number): number {\n  return value + 1;\n}\n";
    let manifest = "\n[packages.utility]\npath = \"vendor/utility\"\n";
    let main = |argument: &str| format!("import {{ increment }} from \"utility\";\nconsole.log(increment({argument}));\n");
    let (valid, invalid) = (main("4"), main("\"4\""));
    ensure_gate_sees(
        build_files(&[("vendor/utility/src/index.ts", package), ("src/main.ts", &valid)], "main.ts", manifest)?,
        build_files(&[("vendor/utility/src/index.ts", package), ("src/main.ts", &invalid)], "main.ts", manifest)?,
        "TS2345",
    )
}

/// A test file reaches the test API Smelt models (`describe`, `it`,
/// `expect`) as globals, typed; a file outside the test tier does not.
#[cfg(feature = "ts7")]
#[test]
fn test_globals_type_check_in_test_files_only() -> TestResult {
    let spec = "function add(a: number, b: number): number {\n  return a + b;\n}\n\ndescribe(\"add\", () => {\n  it(\"sums\", () => {\n    expect(add(2, 3)).toBe(5);\n  });\n});\n";
    let ill_typed_spec = spec.replace("add(2, 3)", "add(2, \"3\")");
    ensure_gate_sees(
        build_files(&[("src/math.spec.ts", spec)], "math.spec.ts", "")?,
        build_files(&[("src/math.spec.ts", &ill_typed_spec)], "math.spec.ts", "")?,
        "TS2345",
    )?;
    // The same globals in an ordinary module are not declared.
    let (ok, stderr) = build_files(&[("src/main.ts", "describe(\"x\", () => {});\n")], "main.ts", "")?;
    ensure(!ok && stderr.contains("Cannot find name 'describe'"), stderr)
}

/// TypeScript importing Python type-checks against the Python module's
/// signatures (an in-memory `.d.ts`), and Python importing TypeScript
/// against the TypeScript module's (an in-memory `.pyi`).
#[cfg(all(feature = "ts7", feature = "ty"))]
#[test]
fn cross_language_imports_type_check() -> TestResult {
    let python = "def twice(x: float) -> float:\n    return x * 2\n";
    let ts_main = |argument: &str| format!("import {{ twice }} from \"./arith\";\nconsole.log(twice({argument}));\n");
    let (valid, invalid) = (ts_main("4"), ts_main("\"4\""));
    ensure_gate_sees(
        build_files(&[("src/arith.py", python), ("src/main.ts", &valid)], "main.ts", "")?,
        build_files(&[("src/arith.py", python), ("src/main.ts", &invalid)], "main.ts", "")?,
        "TS2345",
    )?;
    let typescript = "export function twice(x: number): number {\n  return x * 2;\n}\n";
    let py_main = |argument: &str| format!("from arith import twice\n\nprint(twice({argument}))\n");
    let (valid_py, invalid_py) = (py_main("4.0"), py_main("\"4\""));
    ensure_gate_sees(
        build_files(&[("src/arith.ts", typescript), ("src/main.py", &valid_py)], "main.py", "")?,
        build_files(&[("src/arith.ts", typescript), ("src/main.py", &invalid_py)], "main.py", "")?,
        "invalid-argument-type",
    )
}

/// The test API declarations type what vitest's own types type: `it.each`
/// callbacks from the table, `vi.fn<T>()` without an
/// implementation, `vi.spyOn` call records, `expectTypeOf<T>()` chains, and
/// Jest's `jest` as vitest's `vi`. Each was an error when the API was `any`
/// (TS7006/TS7031 on untyped callbacks, TS2347 on type arguments, TS2554 on
/// the implementation-less `vi.fn`); the ill-typed twin shows the table's
/// element type really reaches the callback.
#[cfg(feature = "ts7")]
#[test]
fn test_api_declarations_type_vitest_idioms() -> TestResult {
    let spec = |callback_body: &str| {
        format!(
            "import {{ describe, it, expect, expectTypeOf, vi }} from \"vitest\";\n\
             \n\
             const target = {{ run(code: number): string {{ return String(code); }} }};\n\
             \n\
             describe(\"api\", () => {{\n\
             \x20 it.each([\"/a\", \"/b\"])(\"path %s\", (path) => {{ {callback_body} }});\n\
             \x20 it.each([[1, \"x\"], [2, \"y\"]])(\"pair %s\", (count, label) => {{ expect(count).toBe(label.length); }});\n\
             \x20 it(\"mocks\", () => {{\n\
             \x20   const quiet = vi.fn<() => void>();\n\
             \x20   quiet();\n\
             \x20   const spy = vi.spyOn(target, \"run\");\n\
             \x20   target.run(3);\n\
             \x20   const codes = spy.mock.calls.map(([code]) => code + 1);\n\
             \x20   const fake = jest.fn((value: number) => value * 2);\n\
             \x20   expect(fake(2)).toBe(4);\n\
             \x20   expectTypeOf(codes).toEqualTypeOf<number[]>();\n\
             \x20 }});\n\
             }});\n"
        )
    };
    let valid = spec("expect(path.length).toBe(2);");
    let invalid = spec("const wrong: number = path; void wrong;");
    ensure_gate_sees(
        build_files(&[("src/api.spec.ts", &valid)], "api.spec.ts", "")?,
        build_files(&[("src/api.spec.ts", &invalid)], "api.spec.ts", "")?,
        "TS2322",
    )
}

/// An unused `@ts-expect-error` is not a type error the gate counts: the
/// directive asserts an error under the project's own compiler options,
/// which Smelt's fixed profile may not raise.
#[cfg(feature = "ts7")]
#[test]
fn an_unused_expect_error_directive_does_not_refuse_a_program() -> TestResult {
    let source = "// @ts-expect-error -- an error only under the project's `noUncheckedIndexedAccess`\nconst first: number = [1, 2][0];\nconsole.log(first);\n";
    let (ok, stderr) = build_files(&[("src/main.ts", source)], "main.ts", "")?;
    ensure(ok, stderr.clone())?;
    ensure(!stderr.contains("TS2578"), stderr)
}

/// A hook's context is typed (`beforeEach(({ task, skip }) => ...)` binds
/// typed elements, as under vitest's `TestContext`): the checker reports no
/// implicit `any` for it. Asked of `check`, whose type-check diagnostics are
/// separate from lowering's (which does not lower a hook with parameters).
#[cfg(feature = "ts7")]
#[test]
fn a_hook_context_is_typed() -> TestResult {
    let temp = TempProject::new()?;
    let spec = "import { beforeEach, it } from \"vitest\";\n\nbeforeEach(({ task, skip }) => { void task; void skip; });\nit(\"runs\", () => {});\n";
    project(temp.path(), "hook.spec.ts", spec, "")?;
    let output = Command::new(env!("CARGO_BIN_EXE_smelt"))
        .current_dir(temp.path())
        .args(["check", "--message-format", "json"])
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    ensure(stdout.contains("lifecycle callbacks with parameters"), format!("expected the lowering blocker:\n{stdout}"))?;
    ensure(!stdout.contains("\"type-check\""), format!("the hook context must type-check:\n{stdout}"))
}

/// A package Smelt resolves to nothing (a test-only dev dependency not
/// installed) is an untyped module to the checker, as it is a dynamic value
/// to lowering in a test file: no
/// "cannot find module", and no implicit `any` on the parameters of a
/// callback its calls receive. Code that does not touch it is still checked.
#[cfg(feature = "ts7")]
#[test]
fn an_uninstalled_package_is_an_untyped_module() -> TestResult {
    let program = |extra: &str| {
        format!(
            "import {{ it }} from \"vitest\";\nimport fc from \"not-installed-anywhere\";\n\nit(\"property\", () => {{\n  fc.check([1, 2], (value, index) => value + index);\n  const total: number = 3;\n  {extra}\n}});\n"
        )
    };
    ensure_gate_sees(
        build_files(&[("src/main.spec.ts", &program(""))], "main.spec.ts", "")?,
        build_files(&[("src/main.spec.ts", &program("const wrong: string = total; void wrong;"))], "main.spec.ts", "")?,
        "TS2322",
    )
}

/// Node's timer handle (`ReturnType<typeof setTimeout>`, `NodeJS.Timeout`
/// under `@types/node`) is a `number`, the id Smelt's runtime hands out, so
/// the local holding one is typed, not erased.
#[cfg(feature = "ts7")]
#[test]
fn a_node_timer_handle_is_a_number() -> TestResult {
    let temp = TempProject::new()?;
    let source = "let timer: ReturnType<typeof setTimeout> | null = null;\ntimer = setTimeout(() => {}, 1);\nclearTimeout(timer);\nconsole.log(timer != null);\n";
    project(temp.path(), "main.ts", source, "")?;
    let output = Command::new(env!("CARGO_BIN_EXE_smelt"))
        .current_dir(temp.path())
        .args(["build", "--hir"])
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    ensure(output.status.success(), String::from_utf8_lossy(&output.stderr).into_owned())?;
    ensure(stdout.contains("let timer: Optional<Float>"), stdout)
}
