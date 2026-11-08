//! Declaration stub emission for TypeScript and Python LSPs.
//!
//! Every lowered source module gets a `.d.ts` and a `.pyi` beside it, so an
//! editor resolving the module from either language sees its lowered surface.
//! Those files sit in the user's source tree, so writing one is guarded by
//! ownership: a stub is written only over nothing or over a file that starts
//! with Smelt's own stub header ([`ClaimedStub`]), never over a hand-written
//! declaration; it is rewritten only when its bytes change, so an editor or a
//! build tool watching mtimes sees no churn; and two sources whose stubs would
//! land on one path (`a.ts` and `a.mts` both declare `a.d.ts`) are refused
//! before anything is written.

use smelt_hir::{Crate, Function, Item, ModuleId, Type};
use std::collections::BTreeMap;
use std::io::Write as _;
use std::{fs, io, path::{Path, PathBuf}};

use crate::report::push_fmt;

/// The two stub languages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StubKind {
    /// A TypeScript declaration file (`.d.ts`).
    TypeScript,
    /// A Python stub (`.pyi`).
    Python,
}

impl StubKind {
    /// Both kinds, in the order they are written.
    const ALL: [Self; 2] = [Self::TypeScript, Self::Python];

    /// The extension replacing the source's.
    const fn extension(self) -> &'static str {
        match self {
            Self::TypeScript => "d.ts",
            Self::Python => "pyi",
        }
    }

    /// The first line of every stub of this kind; it marks the file as Smelt's.
    const fn header(self) -> &'static str {
        match self {
            Self::TypeScript => smelt_hir::TS_STUB_HEADER,
            Self::Python => smelt_hir::PY_STUB_HEADER,
        }
    }

    /// The stub of `module`, starting with [`Self::header`].
    fn render(self, krate: &Crate, module: ModuleId) -> String {
        match self {
            Self::TypeScript => typescript_declaration(krate, module),
            Self::Python => python_declaration(krate, module),
        }
    }
}

/// The TypeScript declaration of `module`, as the TypeScript checker reads a
/// Python module that TypeScript imports.
pub(crate) fn typescript_stub(krate: &Crate, module: ModuleId) -> String {
    StubKind::TypeScript.render(krate, module)
}

/// The Python stub of `module`, as ty reads a TypeScript module that Python imports.
pub(crate) fn python_stub(krate: &Crate, module: ModuleId) -> String {
    StubKind::Python.render(krate, module)
}

/// A stub destination Smelt may write: nothing is there, or the file there
/// carries Smelt's stub header of the same kind.
///
/// Only [`Self::claim`] builds one, and it reads the file to decide, so a
/// hand-written declaration can never be overwritten.
#[derive(Debug)]
struct ClaimedStub {
    /// Where the stub goes.
    path: PathBuf,
    /// What kind of stub may go there.
    kind: StubKind,
    /// The bytes there now, if any.
    existing: Option<String>,
}

impl ClaimedStub {
    /// Claims `path` for a `kind` stub, or answers `None` when the file there
    /// is not Smelt's.
    ///
    /// # Errors
    ///
    /// Returns an error when the file exists but cannot be read.
    fn claim(path: PathBuf, kind: StubKind) -> io::Result<Option<Self>> {
        match fs::read(&path) {
            Ok(bytes) => Ok(String::from_utf8(bytes)
                .ok()
                .filter(|text| text.starts_with(kind.header()))
                .map(|existing| Self { path, kind, existing: Some(existing) })),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Some(Self { path, kind, existing: None })),
            Err(error) => Err(error),
        }
    }

    /// Writes `module`'s stub here, unless the bytes are already there.
    fn write(self, krate: &Crate, module: ModuleId) -> io::Result<()> {
        let text = self.kind.render(krate, module);
        if self.existing.as_deref() == Some(text.as_str()) {
            return Ok(());
        }
        fs::write(&self.path, text)
    }
}

/// Emit `.d.ts` and `.pyi` files next to each lowered source module.
///
/// A module inside an installed package (a path with a `node_modules`
/// component) gets no stub: Smelt never writes into installed packages. Such a
/// module is a file a package's typed version lays over the installed package,
/// in memory only (see `crate::sound`), and its stub would overwrite the
/// package's own `index.d.ts` or add files to it. A declaration file gets none
/// either: it is already one.
///
/// A destination holding a file Smelt did not write is left alone, with a
/// warning; see the module docs.
///
/// # Errors
///
/// Returns an error, before writing anything, when two modules' stubs would
/// land on one path; or when a declaration file cannot be read or written.
pub fn emit_type_declarations(
    krate: &Crate,
    modules: &[(String, ModuleId)],
) -> Result<(), Box<dyn std::error::Error>> {
    let planned = plan_stubs(modules)?;
    for (destination, (kind, module, _source)) in planned {
        match ClaimedStub::claim(destination.clone(), kind)? {
            Some(claimed) => claimed.write(krate, module)?,
            None => {
                let _ignored = writeln!(
                    io::stderr().lock(),
                    "warning[stubs]: left `{}` alone: it was not written by smelt",
                    destination.display()
                );
            }
        }
    }
    Ok(())
}

/// Every stub to write, by destination: its kind, module and source path.
///
/// # Errors
///
/// Two sources whose stubs share a destination.
fn plan_stubs(
    modules: &[(String, ModuleId)],
) -> Result<BTreeMap<PathBuf, (StubKind, ModuleId, &str)>, String> {
    let mut planned = BTreeMap::new();
    for (path, module_id) in modules {
        if is_declaration_path(path) || is_installed_package_path(path) {
            continue;
        }
        for kind in StubKind::ALL {
            let mut destination = Path::new(path).to_path_buf();
            destination.set_extension(kind.extension());
            if let Some((_, _, other)) = planned.insert(destination.clone(), (kind, *module_id, path.as_str())) {
                return Err(format!(
                    "the declaration stubs of `{other}` and `{path}` would both be written to `{}`; \
                     rename one of the sources",
                    destination.display()
                ));
            }
        }
    }
    Ok(planned)
}

/// Return whether a path lies inside an installed package (`node_modules`).
fn is_installed_package_path(path: &str) -> bool {
    Path::new(path)
        .components()
        .any(|component| component.as_os_str() == "node_modules")
}

/// Return whether a path is already a declaration/stub file.
fn is_declaration_path(path: &str) -> bool {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|file_name| {
            let lower_name = file_name.to_ascii_lowercase();
            lower_name.ends_with(".d.ts")
                || lower_name.ends_with(".d.mts")
                || lower_name.ends_with(".d.cts")
        })
        || Path::new(path)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pyi"))
}

/// Format a TypeScript declaration file for one HIR module.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "declaration formatting keeps the TypeScript item cases together"
)]
fn typescript_declaration(krate: &Crate, module_id: ModuleId) -> String {
    let Some(module_idx) = usize::try_from(module_id.0).ok() else {
        return String::new();
    };
    let Some(module) = krate.modules.get(module_idx) else {
        return String::new();
    };
    let mut out = String::new();
    out.push_str(smelt_hir::TS_STUB_HEADER);
    out.push_str("\n\n");
    for item_id in &module.items {
        let Some(item_idx) = usize::try_from(item_id.0).ok() else {
            continue;
        };
        let Some(item) = krate.items.get(item_idx) else {
            continue;
        };
        match item {
            Item::Function(function)
                if matches!(function.owner, smelt_hir::FunctionOwner::Module) =>
            {
                let params = function
                    .params
                    .iter()
                    .map(|param| {
                        format!(
                            "{}: {}",
                            symbol_name(krate, param.name),
                            ts_type(krate, param.ty)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                push_fmt(
                    &mut out,
                    format_args!(
                        "export declare function {}({params}): {};\n",
                        symbol_name(krate, function.name),
                        ts_type(krate, function.return_ty)
                    ),
                );
            }
            Item::Class(class) => {
                push_fmt(
                    &mut out,
                    format_args!(
                        "export declare class {} {{\n",
                        symbol_name(krate, class.name)
                    ),
                );
                for field in &class.fields {
                    push_fmt(
                        &mut out,
                        format_args!(
                            "  {}: {};\n",
                            ts_property_name(symbol_name(krate, field.name)),
                            ts_type(krate, field.ty)
                        ),
                    );
                }
                out.push_str("}\n");
            }
            Item::Interface(interface) => {
                push_fmt(
                    &mut out,
                    format_args!(
                        "export interface {} {{\n",
                        symbol_name(krate, interface.name)
                    ),
                );
                for field in &interface.fields {
                    push_fmt(
                        &mut out,
                        format_args!(
                            "  {}: {};\n",
                            ts_property_name(symbol_name(krate, field.name)),
                            ts_type(krate, field.ty)
                        ),
                    );
                }
                for method in &interface.methods {
                    let params = method
                        .params
                        .iter()
                        .map(|param| {
                            format!(
                                "{}: {}",
                                symbol_name(krate, param.name),
                                ts_type(krate, param.ty)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    push_fmt(
                        &mut out,
                        format_args!(
                            "  {}({params}): {};\n",
                            ts_property_name(symbol_name(krate, method.name)),
                            ts_type(krate, method.return_ty)
                        ),
                    );
                }
                out.push_str("}\n");
            }
            Item::TypeAlias(alias) => {
                let type_params = if alias.type_params.is_empty() {
                    String::new()
                } else {
                    format!(
                        "<{}>",
                        alias
                            .type_params
                            .iter()
                            .map(|param| symbol_name(krate, param.name))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                push_fmt(
                    &mut out,
                    format_args!(
                        "export type {}{} = {};\n",
                        symbol_name(krate, alias.name),
                        type_params,
                        ts_type(krate, alias.ty)
                    ),
                );
            }
            Item::Function(_) | Item::Const(_) | Item::MutableGlobal(_) => {}
        }
    }
    out
}

/// Format a Python stub file for one HIR module.
#[must_use]
fn python_declaration(krate: &Crate, module_id: ModuleId) -> String {
    let Some(module_idx) = usize::try_from(module_id.0).ok() else {
        return String::new();
    };
    let Some(module) = krate.modules.get(module_idx) else {
        return String::new();
    };
    let mut out = String::new();
    out.push_str(smelt_hir::PY_STUB_HEADER);
    out.push_str("\n\n");
    for item_id in &module.items {
        let Some(item_idx) = usize::try_from(item_id.0).ok() else {
            continue;
        };
        let Some(item) = krate.items.get(item_idx) else {
            continue;
        };
        match item {
            Item::Function(function)
                if matches!(function.owner, smelt_hir::FunctionOwner::Module) =>
            {
                write_python_function(krate, function, &mut out);
            }
            Item::Class(class) => {
                push_fmt(
                    &mut out,
                    format_args!("class {}:\n", symbol_name(krate, class.name)),
                );
                if class.fields.is_empty() && class.methods.is_empty() {
                    out.push_str("    ...\n");
                } else {
                    for field in &class.fields {
                        push_fmt(
                            &mut out,
                            format_args!(
                                "    {}: {}\n",
                                symbol_name(krate, field.name),
                                py_type(krate, field.ty)
                            ),
                        );
                    }
                }
            }
            Item::Function(_)
            | Item::Interface(_)
            | Item::TypeAlias(_)
            | Item::Const(_)
            | Item::MutableGlobal(_) => {}
        }
    }
    out
}

/// Append a Python function signature to a stub file.
fn write_python_function(krate: &Crate, function: &Function, out: &mut String) {
    let params = function
        .params
        .iter()
        .map(|param| {
            format!(
                "{}: {}",
                symbol_name(krate, param.name),
                py_type(krate, param.ty)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    push_fmt(
        out,
        format_args!(
            "def {}({params}) -> {}: ...\n",
            symbol_name(krate, function.name),
            py_type(krate, function.return_ty)
        ),
    );
}

/// Resolve a symbol to display text.
fn symbol_name(krate: &Crate, symbol: smelt_hir::Symbol) -> &str {
    krate
        .names
        .get(symbol)
        .or_else(|| krate.symbols.get(symbol))
        .unwrap_or("_")
}

/// Render a TypeScript class/interface member name safely.
fn ts_property_name(name: &str) -> String {
    if is_ts_identifier_name(name) && !is_ts_reserved_property_name(name) {
        return name.to_owned();
    }
    format!("{name:?}")
}

/// Return whether text can be emitted as a bare TypeScript identifier.
fn is_ts_identifier_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first == '$' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch == '$' || ch.is_ascii_alphanumeric())
}

/// Return whether a property should be quoted in declaration output.
fn is_ts_reserved_property_name(name: &str) -> bool {
    matches!(
        name,
        "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "export"
            | "extends"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "import"
            | "in"
            | "instanceof"
            | "new"
            | "return"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
            | "yield"
    )
}

/// Render a HIR type as TypeScript.
fn ts_type(krate: &Crate, ty: smelt_hir::TypeId) -> String {
    match krate.types.get(ty) {
        Some(Type::Bool) => "boolean".to_owned(),
        Some(Type::Int | Type::FixedInt(_) | Type::Float) => "number".to_owned(),
        Some(Type::BigInt) => "bigint".to_owned(),
        Some(Type::String | Type::PyStr) => "string".to_owned(),
        Some(Type::Unknown) | None => "unknown".to_owned(),
        Some(Type::Never) => "never".to_owned(),
        Some(Type::TypeParam { name } | Type::Class { name, .. }) => {
            symbol_name(krate, *name).to_owned()
        }
        Some(Type::None) => "void".to_owned(),
        Some(Type::List(item) | Type::Set(item)) => {
            format!("Array<{}>", ts_type(krate, *item))
        }
        Some(Type::Dict(key, value)) => {
            format!(
                "Record<{}, {}>",
                ts_type(krate, *key),
                ts_type(krate, *value)
            )
        }
        Some(Type::JsMap(key, value)) => {
            format!("Map<{}, {}>", ts_type(krate, *key), ts_type(krate, *value))
        }
        Some(Type::Tuple(items)) => {
            let tuple_items = items
                .iter()
                .map(|item| ts_type(krate, *item))
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{tuple_items}]")
        }
        Some(Type::Optional(item)) => format!("{} | null", ts_union_member_type(krate, *item)),
        Some(Type::Union(items)) => items
            .iter()
            .map(|item| ts_union_member_type(krate, *item))
            .collect::<Vec<_>>()
            .join(" | "),
        Some(Type::Function(function)) => {
            let params = function
                .params
                .iter()
                .map(|param| ts_type(krate, *param))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({params}) => {}", ts_type(krate, function.return_ty))
        }
        Some(Type::Future(item)) => format!("Promise<{}>", ts_type(krate, *item)),
        Some(Type::Generator {
            is_async,
            yield_ty,
            return_ty,
            next_ty,
        }) => format!(
            "{}Generator<{}, {}, {}>",
            if *is_async { "Async" } else { "" },
            ts_type(krate, *yield_ty),
            ts_type(krate, *return_ty),
            ts_type(krate, *next_ty)
        ),
        Some(Type::GeneratorResult {
            yield_ty,
            return_ty,
        }) => format!(
            "IteratorResult<{}, {}>",
            ts_type(krate, *yield_ty),
            ts_type(krate, *return_ty)
        ),
    }
}

/// Render a TypeScript type in a union member position.
fn ts_union_member_type(krate: &Crate, ty: smelt_hir::TypeId) -> String {
    match krate.types.get(ty) {
        Some(Type::Function(_)) => format!("({})", ts_type(krate, ty)),
        _ => ts_type(krate, ty),
    }
}

/// Render a HIR type as Python.
fn py_type(krate: &Crate, ty: smelt_hir::TypeId) -> String {
    match krate.types.get(ty) {
        Some(Type::Bool) => "bool".to_owned(),
        // Python's `int` is already arbitrary-precision.
        Some(Type::Int | Type::FixedInt(_) | Type::BigInt) => "int".to_owned(),
        Some(Type::Float) => "float".to_owned(),
        Some(Type::String | Type::PyStr) => "str".to_owned(),
        Some(Type::Unknown) | None => "typing.Any".to_owned(),
        Some(Type::Never) => "typing.NoReturn".to_owned(),
        Some(Type::TypeParam { name } | Type::Class { name, .. }) => {
            symbol_name(krate, *name).to_owned()
        }
        Some(Type::None) => "None".to_owned(),
        Some(Type::List(item)) => format!("list[{}]", py_type(krate, *item)),
        Some(Type::Set(item)) => format!("set[{}]", py_type(krate, *item)),
        Some(Type::Dict(key, value) | Type::JsMap(key, value)) => {
            format!("dict[{}, {}]", py_type(krate, *key), py_type(krate, *value))
        }
        Some(Type::Tuple(items)) => {
            let tuple_items = items
                .iter()
                .map(|item| py_type(krate, *item))
                .collect::<Vec<_>>()
                .join(", ");
            format!("tuple[{tuple_items}]")
        }
        Some(Type::Optional(item)) => format!("{} | None", py_type(krate, *item)),
        Some(Type::Union(items)) => items
            .iter()
            .map(|item| py_type(krate, *item))
            .collect::<Vec<_>>()
            .join(" | "),
        Some(Type::Function(_)) => "typing.Callable".to_owned(),
        Some(Type::Future(item)) => format!("typing.Awaitable[{}]", py_type(krate, *item)),
        Some(Type::Generator {
            is_async,
            yield_ty,
            return_ty,
            next_ty,
        }) => format!(
            "typing.{}Generator[{}, {}, {}]",
            if *is_async { "Async" } else { "" },
            py_type(krate, *yield_ty),
            py_type(krate, *next_ty),
            py_type(krate, *return_ty)
        ),
        Some(Type::GeneratorResult {
            yield_ty,
            return_ty,
        }) => format!(
            "typing.Union[{}, {}]",
            py_type(krate, *yield_ty),
            py_type(krate, *return_ty)
        ),
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "a test fails fast on fixture setup")]
mod tests {
    //! Stub ownership, collisions and write-only-when-changed.

    use super::*;

    /// A crate with one empty module lowered from `path`.
    fn one_module(path: &str) -> (Crate, Vec<(String, ModuleId)>) {
        let mut krate = Crate::new();
        let module = krate.push_module(smelt_hir::Module::new(
            "main",
            smelt_hir::SourceFile {
                path: path.to_owned(),
                language: smelt_hir::Language::TypeScript,
                file: smelt_hir::FileId(0),
            },
        ));
        (krate, vec![(path.to_owned(), module)])
    }

    /// A hand-written stub beside a source is never overwritten; a missing one
    /// is written with Smelt's header.
    #[test]
    fn a_hand_written_stub_is_left_alone() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let source = dir.path().join("lib.ts");
        fs::write(dir.path().join("lib.pyi"), "def mine() -> int: ...\n").expect("write fixture");
        let (krate, modules) = one_module(&source.display().to_string());
        emit_type_declarations(&krate, &modules).expect("emit");
        assert_eq!(
            fs::read_to_string(dir.path().join("lib.pyi")).expect("read"),
            "def mine() -> int: ...\n",
            "a hand-written .pyi must survive"
        );
        let declaration = fs::read_to_string(dir.path().join("lib.d.ts")).expect("read");
        assert!(declaration.starts_with(smelt_hir::TS_STUB_HEADER), "{declaration}");
    }

    /// A stub whose bytes did not change keeps its mtime; a stale one of
    /// Smelt's is replaced.
    #[test]
    fn an_unchanged_stub_is_not_rewritten() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let source = dir.path().join("lib.ts");
        let (krate, modules) = one_module(&source.display().to_string());
        let stub = dir.path().join("lib.d.ts");
        fs::write(&stub, format!("{}\nstale\n", smelt_hir::TS_STUB_HEADER)).expect("write fixture");
        emit_type_declarations(&krate, &modules).expect("emit");
        let fresh = fs::read_to_string(&stub).expect("read");
        assert!(!fresh.contains("stale"), "Smelt's own stale stub is replaced: {fresh}");
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        fs::File::options().write(true).open(&stub).expect("open").set_modified(old).expect("set mtime");
        emit_type_declarations(&krate, &modules).expect("emit again");
        assert_eq!(fs::metadata(&stub).expect("stat").modified().expect("mtime"), old);
    }

    /// Two sources whose stubs share a path are refused before anything is written.
    #[test]
    fn colliding_stems_are_refused() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let mut krate = Crate::new();
        let modules = ["a.ts", "a.mts"]
            .into_iter()
            .map(|name| {
                let path = dir.path().join(name).display().to_string();
                let module = krate.push_module(smelt_hir::Module::new(
                    "m",
                    smelt_hir::SourceFile {
                        path: path.clone(),
                        language: smelt_hir::Language::TypeScript,
                        file: smelt_hir::FileId(0),
                    },
                ));
                (path, module)
            })
            .collect::<Vec<_>>();
        let error = emit_type_declarations(&krate, &modules).expect_err("a collision is refused").to_string();
        assert!(error.contains("a.d.ts"), "{error}");
        assert!(!dir.path().join("a.d.ts").exists(), "nothing is written");
    }
}
