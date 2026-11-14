//! The host-global tables are reached only through the resolver.
//!
//! JavaScript resolves a bare name lexically, so `function f(Map: Ctor) {
//! return new Map(5) }` constructs the PARAMETER (audit A4). The lowering
//! decides "is this the host's `Map`?" in one place,
//! `ModuleBuilder::resolve_value` (`lowering/resolve.rs`), whose `HostGlobal`
//! proof the host dispatchers take. A site that compares an identifier's
//! spelling to a host-global name itself (`object.name == "Math"`,
//! `matches!(callee.name.as_str(), "Set" | "Map")`) skips the scope and
//! reintroduces the bug, so this test fails on any such comparison in the
//! lowering sources. Compare the resolver's answer instead:
//! `self.names_host(name, "Math")`, `self.host_global(expr)` and
//! `HostGlobal::is`.

use std::path::{Path, PathBuf};

/// Host globals whose spelling a lowering site may not test directly.
const HOST_GLOBALS: &[&str] = &[
    "Math",
    "JSON",
    "Object",
    "Number",
    "Array",
    "String",
    "Boolean",
    "Date",
    "Promise",
    "Symbol",
    "RegExp",
    "Map",
    "Set",
    "WeakMap",
    "WeakSet",
    "Error",
    "AggregateError",
    "Reflect",
    "Intl",
    "console",
    "BigInt",
    "parseInt",
    "parseFloat",
    "isNaN",
    "isFinite",
    "globalThis",
    "Buffer",
    "process",
    "structuredClone",
    "URL",
    "crypto",
    "setTimeout",
    "setInterval",
    "ArrayBuffer",
    "Temporal",
    "Uint8Array",
    "Infinity",
    "NaN",
];

/// Every `.rs` file under `dir`, recursively.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Whether the code of one line compares an identifier's spelling with the
/// host global `global`: `x.name == "G"`, `x.name != "G"`,
/// `x.name.as_str() == "G"`, or `matches!(x.name.as_str(), "G" ..)`. A call
/// handing the spelling to the resolver (`names_host(x.name.as_str(), "G")`)
/// is not one.
fn line_bypasses(code: &str, global: &str) -> bool {
    let comparisons = [
        format!(".name == \"{global}\""),
        format!(".name != \"{global}\""),
        format!(".name.as_str() == \"{global}\""),
    ];
    if comparisons
        .iter()
        .any(|shape| code.contains(shape.as_str()))
    {
        return true;
    }
    let matched = format!(".name.as_str(), \"{global}\"");
    code.match_indices(matched.as_str()).any(|(at, _)| {
        code[..at]
            .trim_end_matches(|character: char| {
                character.is_ascii_alphanumeric() || character == '_'
            })
            .ends_with("matches!(")
    })
}

/// No lowering site outside the resolver decides a host global by spelling.
#[test]
fn host_globals_are_resolved_through_the_resolver() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lowering");
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    assert!(
        files.len() > 50,
        "the lowering sources were not found under {}",
        root.display()
    );
    let mut bypasses = Vec::new();
    for file in files {
        if file.ends_with("resolve.rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for (index, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            // A member's PROPERTY name (`member.property.name == "from"`) is
            // not a binding and needs no resolution.
            let code = code.replace("property.name", "property_name");
            if HOST_GLOBALS
                .iter()
                .any(|global| line_bypasses(&code, global))
            {
                bypasses.push(format!("{}:{}: {}", file.display(), index + 1, line.trim()));
            }
        }
    }
    assert!(
        bypasses.is_empty(),
        "these sites decide a host global by its spelling instead of asking the resolver \
         (`ModuleBuilder::names_host` / `host_global`), so a parameter of that name is mistaken \
         for the host's (audit A4):\n{}",
        bypasses.join("\n")
    );
}

/// The scan itself recognizes the bypass shapes it exists to forbid, and
/// not a spelling handed to the resolver.
#[test]
fn bypass_shapes_match_the_forbidden_spellings() {
    for line in [
        "if object.name == \"Map\" {",
        "if callee.name != \"Map\" {",
        "matches!(callee.name.as_str(), \"Map\" | \"Set\")",
    ] {
        assert!(line_bypasses(line, "Map"), "{line}");
    }
    assert!(!line_bypasses(
        "self.names_host(object.name.as_str(), \"Map\")",
        "Map"
    ));
}
