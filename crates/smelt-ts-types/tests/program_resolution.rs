//! The checker program resolves what Smelt resolves: Node's host globals and
//! `node:` modules from the vendored `@types/node`, bare specifiers through
//! `module_paths`, and modules provided only as declarations.

use std::collections::BTreeMap;

use smelt_ts_types::{CheckProgram, SourceInput, Spellings, TsTypeFacts, analyze_program};

/// The program's errors, as `TSnnnn: message`.
fn errors(program: CheckProgram) -> Result<Vec<String>, String> {
    let facts: TsTypeFacts = analyze_program(program, Spellings::Omitted).map_err(|error| error.to_string())?;
    Ok(facts.errors().map(|diagnostic| format!("TS{}: {}", diagnostic.code, diagnostic.message)).collect())
}

/// `process`, `Buffer`, `console`, timers and `node:` modules type-check.
#[test]
fn node_globals_and_modules_type_check() -> Result<(), String> {
    let source = r#"
import { EventEmitter } from "node:events";
const port: string | undefined = process.env.PORT;
const bytes: number = Buffer.from("hi").length;
const handle = setTimeout(() => console.log(port, bytes), 1);
clearTimeout(handle);
new EventEmitter().emit("x");
"#;
    let found = errors(CheckProgram::of_sources(vec![SourceInput::new("src/main.ts", source)]))?;
    if found.is_empty() { Ok(()) } else { Err(found.join("\n")) }
}

/// A bare specifier resolves to the file `module_paths` names, and a module
/// given only as a declaration is typed by it.
#[test]
fn bare_specifiers_and_declarations_resolve() -> Result<(), String> {
    let main = "import { twice } from \"utility\";\nimport { add } from \"./math\";\nconst n: number = twice(add(1, 2));\nconsole.log(n);\n";
    let program = CheckProgram::of_sources(vec![
        SourceInput::new("app/src/main.ts", main),
        SourceInput::new("vendor/utility/src/index.ts", "export function twice(x: number): number { return x * 2; }\n"),
    ])
    .with_declarations(vec![SourceInput::new(
        "app/src/math.d.ts",
        "export declare function add(a: number, b: number): number;\n",
    )])
    .with_module_paths(BTreeMap::from([("utility".to_owned(), "vendor/utility/src/index.ts".to_owned())]));
    let found = errors(program)?;
    if found.is_empty() { Ok(()) } else { Err(found.join("\n")) }
}

/// Without the mapping the same program does not resolve: the test above is
/// not passing by accident.
#[test]
fn an_unmapped_bare_specifier_is_unresolved() -> Result<(), String> {
    let main = "import { twice } from \"utility\";\nconsole.log(twice(1));\n";
    let found = errors(CheckProgram::of_sources(vec![SourceInput::new("app/src/main.ts", main)]))?;
    if found.iter().any(|error| error.starts_with("TS2307")) { Ok(()) } else { Err(format!("{found:?}")) }
}
