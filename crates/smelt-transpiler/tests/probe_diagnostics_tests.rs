//! Integration tests for recoverable, categorized diagnostics
//! (`smelt check --message-format json`).

mod common;

use std::fs;

use common::{TempProject, TestResult, ensure, smelt, utf8_path};

/// `check --message-format json` keeps lowering past the first failing file and
/// reports every module's diagnostics, each tagged with a structured category.
#[test]
fn check_json_recovers_and_categorizes_across_files() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "diag-recovery"
version = "0.1.0"

[sources]
entries = ["src/a.ts", "src/b.ts"]

[output]
target = "./dist"
crate-name = "diag_recovery"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    // First file references an unknown user symbol; second references a known
    // builtin Smelt does not model. A fail-fast pass would only report the first.
    // (`Reflect`/`Math`/`JSON` now resolve as namespace values, so this uses
    // `structuredClone`, whose bare value reference is not lowered yet.)
    fs::write(
        project_path.join("src/a.ts"),
        "console.log(missingAlpha);\n",
    )?;
    fs::write(
        project_path.join("src/b.ts"),
        "const f = structuredClone;\n",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    let json = smelt(&[
        "--manifest-path",
        &manifest_arg,
        "check",
        "--message-format",
        "json",
    ])?;

    // Both files surface (recovery past the first failure).
    ensure(
        json.contains("missingAlpha"),
        "missing first-file diagnostic",
    )?;
    ensure(
        json.contains("structuredClone"),
        "missing second-file diagnostic",
    )?;
    // Categories are decided in the frontend and serialized in kebab-case.
    ensure(
        json.contains("\"unresolved-reference\""),
        "expected unresolved-reference category for unknown symbol",
    )?;
    ensure(
        json.contains("\"missing-stdlib\""),
        "expected missing-stdlib category for known builtin",
    )?;
    Ok(())
}

/// `[sources] exclude` prunes the dependency CLOSURE, not only the root set, and
/// a *value* imported from an excluded module reports the exclusion by name.
///
/// This is the shape root-only filtering cannot express: `src/main.ts` reaches
/// the excluded module directly. Before closure pruning the excluded module was pulled in
/// transitively and lowered as if it were in scope, which is why an exclusion
/// could be written in the manifest and silently have no effect.
#[test]
fn exclude_prunes_the_dependency_closure_and_names_value_imports() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src/client"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "exclude-closure"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]
exclude = ["src/client/**"]

[output]
target = "./dist"
crate-name = "exclude_closure"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    // The excluded module. `makeClient` is a value; `ClientOptions` a type.
    fs::write(
        project_path.join("src/client/index.ts"),
        "export type ClientOptions = { base: string };\nexport const makeClient = (base: string): string => base;\n",
    )?;
    // A type-only re-export from the excluded module must stay free: this is
    // how a barrel keeps publishing an out-of-scope module's type surface.
    fs::write(
        project_path.join("src/types.ts"),
        "export type { ClientOptions } from './client';\n",
    )?;
    // Using the excluded module's VALUE must block, and name the exclusion.
    fs::write(
        project_path.join("src/main.ts"),
        "import { makeClient } from './client';\nexport const base = makeClient('x');\n",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    let json = smelt(&[
        "--manifest-path",
        &manifest_arg,
        "check",
        "--message-format",
        "json",
    ])?;

    ensure(
        json.contains("makeClient") && json.contains("which the manifest excludes"),
        format!("expected a named exclusion blocker for the value import, got {json}"),
    )?;
    // The excluded file must not have been lowered as a module of the crate.
    ensure(
        !json.contains("client/index.ts"),
        format!("excluded module should be pruned from the closure, got {json}"),
    )?;

    Ok(())
}

/// A type-only import from an excluded module is free, and a type-only
/// re-export through a barrel is too.
///
/// This is the half that must NOT block: excluding a module removes its
/// implementation from the crate, not its type surface, and a barrel that
/// re-exports `export type { X } from './excluded'` stays usable. Hono's
/// `src/index.ts` is exactly this shape.
#[test]
fn exclude_keeps_type_only_imports_and_reexports_free() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src/client"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "exclude-type-only"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]
exclude = ["src/client/**"]

[output]
target = "./dist"
crate-name = "exclude_type_only"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/client/index.ts"),
        "export type ClientOptions = { base: string };\nexport const makeClient = (base: string): string => base;\n",
    )?;
    fs::write(
        project_path.join("src/types.ts"),
        "export type { ClientOptions } from './client';\n",
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "import type { ClientOptions } from './types';\nexport const base = (options: ClientOptions): string => options.base;\n",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    let json = smelt(&[
        "--manifest-path",
        &manifest_arg,
        "check",
        "--message-format",
        "json",
    ])?;

    ensure(
        !json.contains("which the manifest excludes"),
        format!("a type-only import from an excluded module must not block, got {json}"),
    )?;

    Ok(())
}

/// Builds a one-file Python project from `source` and answers the stderr of
/// the `smelt build` that must reject it.
fn rejected_python_build(source: &str) -> TestResult<String> {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "py-diagnostics"
version = "0.1.0"

[sources]
entries = ["src/main.py"]

[output]
target = "./dist"
crate-name = "py_diagnostics"
build = false
"#,
    )?;
    fs::write(project_path.join("src/main.py"), source)?;
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_smelt"))
        .current_dir(project_path)
        .arg("build")
        .output()?;
    ensure(
        !output.status.success(),
        "the build must reject the program",
    )?;
    Ok(String::from_utf8(output.stderr)?)
}

/// A rejected Python source reports each diagnostic as
/// `file:line:column: error[code]: message`, at the expression it is about:
/// a type helper's diagnostic takes the position of the expression that
/// asked, never line 1, column 1, and the text is not a `Debug` dump.
///
/// The probe only needs some `str` method Smelt still rejects. It used
/// `expandtabs`, which #567 made supported (the build then succeeded and the
/// test failed); `casefold` is still rejected, so the requirement under test
/// (position and form of the diagnostic) is unchanged.
#[test]
fn python_diagnostics_point_at_the_rejected_expression() -> TestResult {
    let stderr = rejected_python_build("s: str = \"x\"\n\n\nprint(s.casefold())\n")?;
    ensure(
        stderr.contains("src/main.py:4:7: error[smelt::unsupported-py]: "),
        format!("diagnostic at line 4, column 7 in compiler form:\n{stderr}"),
    )?;
    ensure(
        !stderr.contains("ManifestDiagnostic") && !stderr.contains(":1:1:"),
        format!("no Debug dump and no 1:1 position:\n{stderr}"),
    )
}

/// A use of an imported API Smelt has no mapping for names the API at the
/// use, through `import m`, `from m import x` and an attribute read, and a
/// later statement that fails only because it uses the rejected definition
/// is not reported a second time.
#[test]
fn unmapped_python_imports_are_named_once() -> TestResult {
    let from_import = rejected_python_build(
        "from collections import deque\n\n\ndef main() -> None:\n    d: deque[int] = deque()\n    print(d)\n\n\nmain()\n",
    )?;
    ensure(
        from_import.contains(
            "src/main.py:5:21: error[smelt::unsupported-py]: `collections.deque` is not supported",
        ),
        format!("from-import use names the API:\n{from_import}"),
    )?;
    ensure(
        from_import
            .lines()
            .filter(|line| line.contains("error["))
            .count()
            == 1,
        format!("the call of the failed `main` is not a second diagnostic:\n{from_import}"),
    )?;
    let attribute = rejected_python_build("import sys\n\nprint(sys.argv)\n")?;
    ensure(
        attribute
            .contains("src/main.py:3:7: error[smelt::unsupported-py]: `sys.argv` is not supported"),
        format!("module attribute read names the API:\n{attribute}"),
    )?;
    let module_variable = rejected_python_build(
        "import collections\n\nq = collections.deque()\nq.append(1)\nprint(len(q))\n",
    )?;
    // An untyped module variable is materialized by running the module in the
    // specialization guest, which rejects a runtime-dynamic `deque`: one
    // diagnostic naming the API, not the guest's Python traceback.
    ensure(
        module_variable.contains("error[smelt::dynamic-attribute-access]: 'collections.deque'")
            && !module_variable.contains("Traceback")
            && !module_variable.contains("Guest {"),
        format!("guest rejection is a diagnostic:\n{module_variable}"),
    )?;
    ensure(
        module_variable
            .lines()
            .filter(|line| line.contains("error["))
            .count()
            == 1,
        format!(
            "uses of a module variable whose initializer failed do not cascade:\n{module_variable}"
        ),
    )?;
    let typed_variable = rejected_python_build(
        "from collections import deque\n\nq: deque[int] = deque()\nq.append(1)\nprint(len(q))\n",
    )?;
    ensure(
        typed_variable.lines().filter(|line| line.contains("error[")).count() == 1
            && typed_variable.contains("src/main.py:3:17: error[smelt::unsupported-py]: `collections.deque` is not supported"),
        format!("uses of a module variable whose initializer failed do not cascade:\n{typed_variable}"),
    )
}

/// A statement that fails only because it uses a definition whose own
/// lowering failed (a function, at module level or inside another function)
/// is not reported a second time.
#[test]
fn failed_definitions_do_not_cascade() -> TestResult {
    let in_function = rejected_python_build(
        "def gen(n: int) -> int:\n    yield n\n    return n\n\n\ndef main() -> None:\n    print(gen(1))\n\n\nmain()\n",
    )?;
    ensure(
        in_function
            .lines()
            .filter(|line| line.contains("error["))
            .count()
            == 1,
        format!(
            "a call of a failed function inside another function is not a second diagnostic:\n{in_function}"
        ),
    )?;
    Ok(())
}
