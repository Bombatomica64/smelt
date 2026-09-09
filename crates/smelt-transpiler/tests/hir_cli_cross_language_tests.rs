//! Cross-language and fixture-sweep integration tests.

mod common;

use std::fs;

use common::{
    TempProject, TestResult, cargo_run_manifest, ensure, ensure_eq, example_dir,
    example_is_selected, smelt, utf8_path, verify_end_to_end_example,
    verify_python_end_to_end_example,
};

#[test]
fn build_runs_python_entry_importing_typescript_function() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "cross-run"
version = "0.1.0"

[sources]
entries = ["src/math.ts", "src/main.py"]

[output]
target = "./dist"
crate-name = "cross_run"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/math.ts"),
        "export function add(a: number, b: number): number {\n  return a + b;\n}\n",
    )?;
    fs::write(
        project_path.join("src/main.py"),
        "from math import add\nresult: float = add(2.0, 3.0)\nprint(result)\n",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"5\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_keeps_one_main_when_python_entry_imports_typescript_main_module() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src/lib"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "cross-main-name"
version = "0.1.0"

[sources]
entries = ["src/main.py"]

[output]
target = "./dist"
crate-name = "cross_main_name"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/lib/main.ts"),
        "export function add(a: number, b: number): number {\n  return a + b;\n}\n",
    )?;
    fs::write(
        project_path.join("src/main.py"),
        "from lib.main import add\nresult: float = add(4.0, 6.0)\nprint(result)\n",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let generated = fs::read_to_string(project_path.join("dist/src/main.rs"))?;
    ensure_eq(
        &generated.matches("fn main(").count(),
        &1,
        "generated Rust should contain one executable main function",
    )?;
    ensure(
        generated.contains("#[path = \"main_1.rs\"]\nmod __smelt_module_main_1;"),
        "dependency module body should be emitted in its source module",
    )?;
    let dependency_module = fs::read_to_string(project_path.join("dist/src/main_1.rs"))?;
    ensure(
        dependency_module.contains("fn main_1()"),
        "dependency module body should be renamed away from main",
    )?;
    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"10\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_resolves_python_package_init_imports() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src/lib"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "py-package-import"
version = "0.1.0"

[sources]
entries = ["src/main.py", "src/lib/__init__.py"]

[output]
target = "./dist"
crate-name = "py_package_import"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/lib/__init__.py"),
        "def add(a: int, b: int) -> int:\n    return a + b\n",
    )?;
    fs::write(
        project_path.join("src/main.py"),
        "from lib import add\nresult: int = add(7, 8)\nprint(result)\n",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"15\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_resolves_python_package_namespace_imports() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src/httpx"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "py-package-namespace-import"
version = "0.1.0"

[sources]
entries = ["src/main.py", "src/httpx/__init__.py"]

[output]
target = "./dist"
crate-name = "py_package_namespace_import"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/httpx/__init__.py"),
        "def add(a: int, b: int) -> int:\n    return a + b\n",
    )?;
    fs::write(
        project_path.join("src/main.py"),
        "import httpx\nresult: int = httpx.add(7, 8)\nprint(result)\n",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"15\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_discovers_python_ast_import_forms() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src/pkg/sub"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "py-ast-import-discovery"
version = "0.1.0"

[sources]
entries = ["src/main.py"]

[output]
target = "./dist"
crate-name = "py_ast_import_discovery"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(project_path.join("src/pkg/__init__.py"), "")?;
    fs::write(project_path.join("src/pkg/sub/__init__.py"), "")?;
    fs::write(
        project_path.join("src/alpha.py"),
        "def first() -> int:\n    return 2\n",
    )?;
    fs::write(
        project_path.join("src/beta.py"),
        "def second() -> int:\n    return 3\n",
    )?;
    fs::write(
        project_path.join("src/pkg/util.py"),
        "def bonus() -> int:\n    return 4\n",
    )?;
    fs::write(
        project_path.join("src/pkg/sub/helper.py"),
        "from ..util import bonus\n\ndef compute(value: int) -> int:\n    return bonus() + value\n",
    )?;
    fs::write(
        project_path.join("src/main.py"),
        r"import alpha, beta
from pkg.sub.helper import (
    compute,
)

result: int = alpha.first() + beta.second() + compute(5)
print(result)
",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"14\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_runs_typescript_entry_importing_python_function() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ts-imports-py"
version = "0.1.0"

[sources]
entries = ["src/main.ts", "src/math.py"]

[output]
target = "./dist"
crate-name = "ts_imports_py"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/math.py"),
        "def add(a: float, b: float) -> float:\n    return a + b\n",
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "import { add } from './math';\nconst result = add(9, 4);\nconsole.log(result);\n",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"13\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

/// A forward-referenced module-level arrow const, whose lifted Rust item name
/// is qualified by its module.
const LIFTED_ARROW_MAIN: &str = r"function run(n: number): number {
  return helper(n) + 1;
}
const helper = (n: number): number => n * 2;
console.log(run(3));
console.log(helper(5));
";

/// The generated Rust for one source is the same wherever it is built.
///
/// A module-private helper's Rust item name has to stay unique once every
/// module is emitted into one crate, and it was qualified with `self.path` —
/// the path the compiler was handed, absolute in a manifest build. So the same
/// TypeScript emitted `helper__module__tmp_xyz_src_main_ts` in one checkout and
/// a different name in another: generated output was not reproducible, and no
/// golden could cover the shape (H55, found while writing H50's fixture).
///
/// The qualifier is now the module IDENTITY the transpiler already computes for
/// module bodies (`manifest_module_names`), so this test builds the same source
/// in two different directories and compares the emitted crate byte for byte.
/// Two builds rather than a golden with a name in it, because the property is
/// equality between builds, not any particular spelling.
#[test]
fn build_emits_identical_rust_from_two_directories() -> TestResult {
    let mut emitted = Vec::new();
    for _ in 0_u8..2_u8 {
        let project = TempProject::new()?;
        let project_path = project.path();
        fs::create_dir_all(project_path.join("src"))?;
        fs::write(
            project_path.join("Smelt.toml"),
            r#"[project]
name = "reproducible-names"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "reproducible_names"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
        )?;
        fs::write(project_path.join("src/main.ts"), LIFTED_ARROW_MAIN)?;
        let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
        smelt(&["--manifest-path", &manifest_arg, "build"])?;
        emitted.push(fs::read_to_string(project_path.join("dist/src/main.rs"))?);
    }
    let [first, second] = emitted.as_slice() else {
        return Err("expected two emitted crates".into());
    };
    ensure_eq(
        first,
        second,
        "the same source built in two directories must emit the same Rust",
    )?;
    // The lifted helper is qualified by its module, not by a path: the shape
    // this test exists for would otherwise pass vacuously if the qualification
    // were dropped altogether (which would reintroduce the cross-module
    // collision it prevents).
    ensure(
        first.contains("helper__module_main"),
        "the lifted arrow should be qualified by its module identity",
    )?;
    ensure(
        !first.contains("__module__"),
        "no generated name should embed an absolute path",
    )?;

    Ok(())
}

/// A predicate bound to a const, used both directly and by name.
const NAMED_LOCAL_CALLBACK_MAIN: &str = r#"type Predicate = (value: string) => boolean;

const words = ["a", "abc", "ab", "abcd"];

const isShort = (value: string): boolean => value.length < 3;
const isShortAnnotated: Predicate = (value: string): boolean => value.length < 3;
const upper = (value: string): string => value.toUpperCase();

function countMatching(values: string[], predicate: Predicate): number {
  let total = 0;
  for (const value of values) {
    if (predicate(value)) {
      total += 1;
    }
  }
  return total;
}

console.log(isShort("ab"));
console.log(words.filter(isShort).join(","));
console.log(words.filter(isShortAnnotated).join(","));
console.log(words.some(isShort));
console.log(words.every(isShort));
console.log(words.find(isShort) ?? "none");
console.log(words.map(upper).join(","));
console.log(countMatching(words, isShort));
"#;

#[test]
fn build_runs_named_local_callback_passed_by_value() -> TestResult {
    // A callback declaration's local is only a DECLARATION: calls to the name
    // stay concrete by inlining the callback's body, so the binding is never
    // assigned a closure unless something needs it as a value. The
    // array-callback path captured that local anyway, and an unassigned function
    // local renders as a placeholder default callback (`|_| false`) — so
    // `words.filter(isShort)` kept NOTHING while the direct call `isShort('ab')`
    // one line above answered correctly. It compiled, nothing threw, and an
    // empty result looks like an answer, which is what made it worth fixing over
    // a blocker.
    //
    // A build-and-RUN test rather than an `examples/` fixture, for a reason
    // worth recording: referencing a module-level arrow as a value lifts it to a
    // named function whose Rust name embeds the SOURCE PATH it was compiled
    // from, so the generated Rust is not stable across build directories and
    // cannot be a golden. See `blocker-logs/hono-h55-path-mangled-lifted-name.md`.
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "named-local-callback"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "named_local_callback"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(project_path.join("src/main.ts"), NAMED_LOCAL_CALLBACK_MAIN)?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    // Every observer has to agree with the direct call on the first line.
    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(
        &actual_stdout,
        &"true\na,ab\na,ab\ntrue\nfalse\na\nA,ABC,AB,ABCD\n2\n".to_owned(),
        "unexpected stdout",
    )?;

    Ok(())
}

/// The first of two modules that both export a class named `Node`.
const DUPLICATE_CLASS_ALPHA: &str = r"export class Node {
  #items: string[] = [];

  push(value: string): void {
    this.#items.push(value);
  }

  search(prefix: string): string[] {
    return this.#items.filter((item) => item.startsWith(prefix));
  }
}
";

/// The second module exporting `Node`, with members disjoint from the first's.
const DUPLICATE_CLASS_BETA: &str = r"export class Node {
  index = 0;

  bump(): number {
    this.index += 1;
    return this.index;
  }
}
";

/// An importer that aliases both `Node` classes and calls each one's members.
const DUPLICATE_CLASS_MAIN: &str = r"import { Node as AlphaNode } from './alpha';
import { Node as BetaNode } from './beta';

const alpha = new AlphaNode();
alpha.push('abc');
alpha.push('bcd');
console.log(alpha.search('a').join(','));

const beta = new BetaNode();
console.log(beta.bump());
console.log(beta.bump());
";

#[test]
fn build_runs_two_modules_exporting_a_same_named_class() -> TestResult {
    // Class identity in HIR is the class's name symbol, and method resolution
    // goes from that symbol back to the class item. Two modules exporting a
    // class of the same name therefore interned ONE symbol for two different
    // classes, and every method of the loser reported "unknown class method" —
    // Hono's two `Node` classes (`router/trie-router/node.ts` and
    // `router/reg-exp-router/node.ts`), which stopped the router slice
    // transpiling outright once the import-scanner fix put both in the crate.
    //
    // A crate-wide pre-pass now gives an ambiguous class name the same ordinal
    // suffix scheme `manifest_module_names` uses for module bodies: the last
    // declaring module keeps the bare name and earlier ones become `Node_1`,
    // `Node_2`, ... A name declared by exactly one module is untouched, which
    // is what keeps every existing golden byte-identical.
    //
    // This is a multi-module test rather than an `examples/` fixture because
    // the golden harness copies one `input.ts` into a single-file project and
    // the whole point here is TWO modules. Building and RUNNING it is what
    // proves the fix: the failure mode was a wrong method resolution, so the
    // observable symptom is the value each class answers, not a diagnostic.
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "duplicate-class-name"
version = "0.1.0"

[sources]
roots = ["src"]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "duplicate_class_name"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    // Both modules export a class named `Node`, with disjoint members: if the
    // two collapse onto one identity, at least one call cannot resolve. The
    // importer aliases them, so this also shows the problem was never import
    // scope — distinct local names still collided.
    fs::write(project_path.join("src/alpha.ts"), DUPLICATE_CLASS_ALPHA)?;
    fs::write(project_path.join("src/beta.ts"), DUPLICATE_CLASS_BETA)?;
    fs::write(project_path.join("src/main.ts"), DUPLICATE_CLASS_MAIN)?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    // Each class answers with its OWN members. Before the fix this failed to
    // lower at all ("unknown class method `bump`"); a rename that lost the
    // declaring module's field metadata instead printed an empty first line.
    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"abc\n1\n2\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

/// A renamed generic class that constructs and stores ITSELF.
///
/// Trie-shaped, like `router/trie-router/node.ts`: a private record of children
/// keyed by string, each child a `Node<T>` this method constructs.
const SELF_CONSTRUCTING_CLASS_TRIE: &str = r#"type Pattern = readonly [string, string, boolean] | "*";

export class Node<T> {
  #children: Record<string, Node<T>> = {};
  #pattern?: Pattern | string;
  #values: T[] = [];

  insert(key: string, pattern: Pattern | string, value: T): void {
    let child = this.#children[key];
    if (!child) {
      child = new Node<T>();
      this.#children[key] = child;
    }
    if (pattern && !child.#pattern) {
      child.#pattern = pattern;
    }
    child.#values.push(value);
  }

  describe(key: string): string {
    const child = this.#children[key];
    if (!child) {
      return "none";
    }
    return child.#pattern === undefined ? "unset" : "set";
  }
}
"#;

/// The same-named class that forces the rename, in another module.
const SELF_CONSTRUCTING_CLASS_OTHER: &str = r"export class Node {
  index = 0;

  bump(): number {
    this.index += 1;
    return this.index;
  }
}
";

/// An importer that aliases both and exercises the self-constructing one.
const SELF_CONSTRUCTING_CLASS_MAIN: &str = r"import { Node as TrieNode } from './trie';
import { Node as RegExpNode } from './regexp';

const trie = new TrieNode<string>();
trie.insert('a', '*', 'handler');
console.log(trie.describe('a'));
console.log(trie.describe('b'));

const regexp = new RegExpNode();
console.log(regexp.bump());
";

#[test]
fn build_runs_a_renamed_class_constructing_itself() -> TestResult {
    // A `new Node()` inside `Node`'s OWN method resolved to the other module's
    // `Node`. `ClassRegistry::item` reads a map seeded with every class item in
    // the crate, and a class's own item is registered only AFTER its members
    // are lowered, so the seeded entry was the only match while the class was
    // in progress. While both classes shared one name symbol that was
    // invisible; giving them distinct symbols (the duplicate-class-name fix
    // above) turned it into a generated-crate type mismatch,
    // `expected Option<Node_1<T>>, found Node`.
    //
    // A class name bound in the module's own lexical scope now wins over a
    // crate-wide item of the same spelling, which is what makes a
    // self-reference resolve to the class being declared.
    //
    // Multi-module and build-and-run for the same reasons as the test above:
    // one module cannot produce a rename, and the symptom is a value.
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "self-constructing-class"
version = "0.1.0"

[sources]
roots = ["src"]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "self_constructing_class"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(project_path.join("src/trie.ts"), SELF_CONSTRUCTING_CLASS_TRIE)?;
    fs::write(
        project_path.join("src/regexp.ts"),
        SELF_CONSTRUCTING_CLASS_OTHER,
    )?;
    fs::write(project_path.join("src/main.ts"), SELF_CONSTRUCTING_CLASS_MAIN)?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    // `describe('a')` sees the child the insert stored; `describe('b')` sees
    // nothing. Before the fix the generated crate did not compile at all.
    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(&actual_stdout, &"set
none
1
".to_owned(), "unexpected stdout")?;

    Ok(())
}

/// The module that LOSES the bare spelling of an ambiguous class name.
const AMBIGUOUS_CLASS_LOSER: &str = r"export class Node {
  #tag = 'first';

  label(): string {
    return this.#tag;
  }
}
";

/// The module that KEEPS the bare spelling, and refers to its own class.
const AMBIGUOUS_CLASS_WINNER: &str = r"export class Node {
  #children: Record<string, Node> = {};
  #pattern?: string;

  add(key: string, pattern: string): void {
    const child: Node = new Node();
    this.#children[key] = child;
    child.#pattern = pattern;
  }

  describe(key: string): string {
    const child = this.#children[key];
    if (!child) {
      return 'none';
    }
    return child.#pattern === undefined ? 'unset' : 'set';
  }
}
";

/// An importer that aliases both and exercises the bare-name winner.
const AMBIGUOUS_CLASS_MAIN: &str = r"import { Node as FirstNode } from './first';
import { Node as SecondNode } from './second';

console.log(new FirstNode().label());

const second = new SecondNode();
second.add('a', '*');
console.log(second.describe('a'));
console.log(second.describe('b'));
";

#[test]
fn build_runs_a_module_referring_to_its_own_ambiguous_class_name() -> TestResult {
    // The module that keeps the BARE spelling of an ambiguous class name had no
    // binding of its own: `resolve_type_reference_symbol` fell through to the
    // crate-wide by-name item map, whose entry for an ambiguous spelling is
    // whichever module registered last. So `second.ts`'s own `Node`
    // annotations, and its `new Node()`, resolved to `first.ts`'s class, and
    // reads of its own fields went looking on the wrong class — Hono's trie
    // router read the reg-exp router's `#children` and stopped the router slice
    // with "optional record field `#pattern` is unknown on Node_1".
    //
    // Every module that declares an ambiguous name now gets an entry in the
    // rename map, the winner's mapping the name to itself: the Rust name is
    // unchanged (so goldens are byte-identical) but the frontend binds the name
    // in the module's own scope, which is what makes a module's own class win
    // for its own spelling.
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "ambiguous-class-name"
version = "0.1.0"

[sources]
roots = ["src"]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "ambiguous_class_name"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(project_path.join("src/first.ts"), AMBIGUOUS_CLASS_LOSER)?;
    fs::write(project_path.join("src/second.ts"), AMBIGUOUS_CLASS_WINNER)?;
    fs::write(project_path.join("src/main.ts"), AMBIGUOUS_CLASS_MAIN)?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    // Each class answers with its own members, and the winner's record holds
    // what its own method stored. Before the fix this did not lower at all.
    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(
        &actual_stdout,
        &"first
set
none
".to_owned(),
        "unexpected stdout",
    )?;

    Ok(())
}

/// Every TypeScript end-to-end example the golden suite checks.
///
/// A list rather than a directory scan: an example is only checked once it
/// is named here, so adding a fixture directory without its entry silently
/// checks nothing. Kept out of the test body so the list can grow without
/// pushing the function past the line limit.
const END_TO_END_EXAMPLES: &[&str] = &[
    "01_number",
    "02_string",
    "03_boolean",
    "04_null",
    "05_alias",
    "06_array_literal",
    "07_tuple_literal",
    "08_record_literal",
    "09_index_access",
    "10_unary_logical",
    "11_console_log_expressions",
    "12_while_sum",
    "13_for_of_sum",
    "14_c_for_loop",
    "15_break_continue",
    "16_switch_break_no_fallthrough",
    "17_mutating_array",
    "18_class_fields",
    "19_constructor",
    "20_method_call",
    "21_this_field",
    "22_mutating_method",
    "23_interface_shape",
    "24_interface_method_signature",
    "25_private_protected_metadata",
    "26_interface_inheritance_optional_computed",
    "27_optional_chains",
    "28_regex_match_result",
    "29_callable_object",
    "30_nullish_union_join",
    "31_headers_fetch_type",
    "32_url_search_params",
    "33_console_optional_value",
    "34_optional_field_interface_literal",
    "35_top_level_await",
    "36_floating_promise_drained",
    "38_interface_literal_key_spellings",
    "39_module_scope_reassignment",
    "40_computed_method_over_known_members",
    "41_symbol_keyed_class_members",
    "42_module_arrow_shared_capture",
    "43_await_value_or_promise_union",
    "44_node_http_echo",
    "45_text_codec",
    "46_blob_file",
    "47_module_const_host_value",
    "48_form_data",
    "49_sibling_method_call",
    "50_union_arm_narrowing",
    "51_web_crypto",
    "53_top_level_try_tail",
    "54_regex_test_predicate",
    "55_headers_init_union",
    "56_union_member_read",
    "57_reassigned_method",
    "58_absent_value_stringify",
    "59_ambient_response_init",
    "60_response_body_handle",
    "61_tuple_spread_arguments",
    "63_generic_init_union_arm",
    "64_error_subclass_optional_message",
    "65_typeof_keyof_alias_union",
    "66_json_stringify_shapes",
    "67_compound_bitwise_assignment",
    "68_switch_continue_in_loop",
    "69_asserted_callback_name",
    "70_set_from_iterable",
    "71_const_arrow_literal_hint",
    "72_form_data_for_each",
    "73_set_insertion_order",
    "74_typed_array_views",
    "75_callback_block_return_type",
    "76_base64_globals",
    "77_body_init_buffer_source",
    "78_request_input_forms",
    "79_logical_assignment_store",
    "80_optional_chain_union_and_throw",
    "81_receiver_capture_in_callback",
    "82_data_view_shared_buffer",
    "83_number_to_string",
    "84_data_view_range_error",
];

/// The generated-Rust goldens cover EVERY emitted file, not just `main.rs`.
///
/// Before this, a program whose lowering split into modules had its user code
/// in `source_<entry>.rs` while the golden held `main.rs` — the runtime prelude
/// and a `mod` declaration — so 34 of the corpus's fixtures golden-checked the
/// prelude and nothing they were written to exercise. A feature could change
/// its own emitted Rust with every golden still green.
///
/// This asserts the shape that closed it: a multi-file golden exists, its
/// sections are introduced by the `// ==== <file>` header the harness writes,
/// and the first section is unheaded — which is what keeps a single-file
/// program's golden byte-for-byte its `main.rs`.
#[test]
fn end_to_end_rust_goldens_cover_every_generated_file() -> TestResult {
    let mut split = Vec::new();
    for name in END_TO_END_EXAMPLES {
        let golden = fs::read_to_string(example_dir(name)?.join("expected.rs"))?;
        ensure(
            !golden.starts_with("// ==== "),
            format!("{name}: the first golden section must be unheaded `main.rs`"),
        )?;
        for line in golden.lines().filter(|line| line.starts_with("// ==== ")) {
            let file = line.trim_start_matches("// ==== ");
            ensure(
                std::path::Path::new(file)
                    .extension()
                    .is_some_and(|extension| extension == "rs")
                    && file != "main.rs",
                format!("{name}: `{line}` does not name a generated module file"),
            )?;
            split.push(name);
        }
    }
    ensure(
        !split.is_empty(),
        "no golden holds a second generated file: the corpus would not notice \
         a harness that compared `main.rs` alone again",
    )?;

    Ok(())
}

#[test]
fn end_to_end_examples_match_expected_outputs() -> TestResult {
    for name in END_TO_END_EXAMPLES {
        // `example_is_selected` is unset in a normal run, so this verifies the
        // whole corpus; the regeneration script uses it to narrow a rewrite to
        // the fixtures it was asked for.
        if example_is_selected(name) {
            verify_end_to_end_example(name)?;
        }
    }

    Ok(())
}

/// Golden-checks the Python example corpus, which no test read before.
///
/// These fixtures shipped with only `input.py` and a `Smelt.toml`, so a Python
/// lowering or formatting regression could not fail anything. Only the HIR and
/// MIR tiers are asserted here: the corpus has no generated-Rust or runtime
/// goldens yet, and adding those needs a `smelt build` per case.
#[test]
fn python_end_to_end_examples_match_expected_dumps() -> TestResult {
    for name in [
        "01_number",
        "02_string",
        "03_boolean",
        "04_none",
        "05_while_sum",
        "06_function",
        "07_if_else",
        "08_match",
    ] {
        verify_python_end_to_end_example(name)?;
    }

    Ok(())
}

#[test]
fn build_runs_nested_compound_condition_while_loop() -> TestResult {
    // A compound-condition inner `while` nested inside an outer loop must lower
    // its back-edge so each iteration re-evaluates the FULL `&&` condition. If
    // the back-edge instead `continue`s the outer loop, `combinations` never
    // advances `indices` and the program infinite-loops. Building and RUNNING
    // the program is the only way to prove the hang is gone; the `combinations`
    // shape mirrors the es-toolkit case that first exposed the bug.
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "nested-compound-while"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "nested_compound_while"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        r"function combinations(n: number, r: number): number[][] {
  const result: number[][] = [];
  const indices: number[] = [];
  for (let k = 0; k < r; k++) indices.push(k);
  while (true) {
    const tuple: number[] = [];
    for (let j = 0; j < r; j++) tuple.push(indices[j]);
    result.push(tuple);
    let i = r - 1;
    while (i >= 0 && indices[i] === i + n - r) i--;
    if (i < 0) break;
    indices[i]++;
    for (let j = i + 1; j < r; j++) indices[j] = indices[j - 1] + 1;
  }
  return result;
}
const c = combinations(4, 2);
console.log(c.length);
console.log(c[0][0]);
console.log(c[0][1]);
console.log(c[5][0]);
console.log(c[5][1]);
",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    // 4 choose 2 = 6 tuples; first is [0,1] and last is [2,3].
    ensure_eq(&actual_stdout, &"6\n0\n1\n2\n3\n".to_owned(), "unexpected stdout")?;

    Ok(())
}

#[test]
fn build_runs_loop_body_switch_with_nested_loop() -> TestResult {
    // A `for` loop whose body is a `switch` (with an inner `for` inside one arm)
    // must lower as a real loop whose arms route their back-edge to `continue`
    // and whose inner loop is preserved. This mirrors the es-toolkit `omit`
    // shape (outer `for` over keys, `switch` on the key kind, inner `for`
    // deleting each nested key). Before the fix the outer loop was either
    // mis-recognized as a compound `while` header — dropping the inner loop body
    // and switching on an uninitialized temp (E0381) — or emitted as a run-once
    // straight-line block that never iterated. Building and RUNNING is the only
    // way to prove both the loop iterates and the inner body executes.
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "loop-body-switch"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "loop_body_switch"
build = true

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        r"interface Item { tag: string; vals: number[]; }
function f(items: Item[]): number {
  let total = 0;
  for (let i = 0; i < items.length; i++) {
    const it = items[i];
    switch (it.tag) {
      case 'sum': {
        for (let j = 0; j < it.vals.length; j++) {
          total = total + it.vals[j];
        }
        break;
      }
      case 'one': {
        total = total + 1;
        break;
      }
    }
  }
  return total;
}
const items: Item[] = [
  { tag: 'sum', vals: [1, 2, 3] },
  { tag: 'one', vals: [] },
  { tag: 'sum', vals: [10, 20] },
];
console.log(f(items));
",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    // (1+2+3) + 1 + (10+20) = 37; a dropped inner loop or non-iterating outer
    // loop would produce a smaller number.
    ensure_eq(&actual_stdout, &"37\n".to_owned(), "unexpected stdout")?;

    Ok(())
}
