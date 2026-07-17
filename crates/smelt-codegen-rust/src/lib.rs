//! Code generator for emitting Rust source code from the MIR.
//!
//! This crate provides functionality to transform a MIR (Middle Intermediate Representation)
//! into valid Rust source code, including struct definitions and function implementations.

#![expect(
    clippy::doc_markdown,
    reason = "codegen docs mention generated language tokens that are not fully marked up yet"
)]
#![expect(
    clippy::missing_errors_doc,
    reason = "public codegen entrypoint docs need a focused polish pass"
)]
#![expect(
    clippy::type_complexity,
    reason = "control-flow pattern recognizers currently return structured tuples"
)]
#![expect(
    clippy::format_push_string,
    reason = "Rust code emission is still written as direct string assembly"
)]
#![expect(
    clippy::too_many_arguments,
    reason = "structured codegen emitters pass explicit control-flow context"
)]
#![expect(
    clippy::too_many_lines,
    reason = "large emitters will be split after behavior stabilizes"
)]
#![expect(
    clippy::literal_string_with_formatting_args,
    reason = "generated Rust format strings intentionally contain braces"
)]
#![expect(
    clippy::wildcard_enum_match_arm,
    reason = "operand formatting groups future constants with existing constants"
)]
#![expect(
    clippy::unused_self,
    reason = "helper methods stay on the emitter for future shared state"
)]
#![expect(
    clippy::needless_pass_by_value,
    reason = "type lookup helpers accept owned HIR type values for call-site readability"
)]
#![expect(
    clippy::missing_const_for_fn,
    reason = "utility const qualification will be handled after behavior cleanup"
)]
#![expect(
    clippy::needless_pass_by_ref_mut,
    reason = "emitter entrypoints reserve mutability for incremental generation state"
)]
#![allow(
    clippy::manual_let_else,
    clippy::match_same_arms,
    clippy::needless_question_mark,
    clippy::option_if_let_else,
    clippy::unnested_or_patterns,
    clippy::unnecessary_wraps,
    reason = "Remeda fallback emission currently favors explicit conservative branches over stylistic rewrites"
)]
#![cfg_attr(
    test,
    expect(
        clippy::manual_let_else,
        clippy::match_wild_err_arm,
        clippy::option_if_let_else,
        clippy::panic,
        reason = "codegen tests keep fixture setup compact and fail fast on invalid test inputs"
    )
)]

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

use smelt_hir::{AsyncOp, BodyId, Type, TypeId};
use smelt_mir::{HirOrigin, Mir, MirFunction, Rvalue};

pub(crate) mod classes;
pub(crate) mod classify;
pub(crate) mod deps;
pub mod rust;
pub(crate) mod stdlib;

use deps::GeneratedDep;
mod emitter;
use classes::{
    class_impl_generics_text, class_name_text, class_type_args_text, class_type_params_text,
    effective_class_fields, effective_interface_fields, inherited_trait_methods,
    interface_impl_generics_text, interface_type_params_text, materialized_static_value_text,
};
use emitter::{EmitContext, FunctionEmitter};
use rust::{CodeWriter, RustIdent};

/// Sentinel comment emitted between the fixed runtime prelude and the generated
/// program body.
///
/// Every emitted crate root contains this line exactly once. It is an inert Rust
/// comment (so it never affects compilation) and the single source of truth for
/// where the shared runtime scaffolding ends. Tooling that measures
/// `SmeltUnknown` usage reads it to attribute occurrences above it to the
/// prelude and occurrences below it to program code. Because it is exported,
/// that tooling references this constant instead of hard-coding the string, so
/// the marker and its consumers can never drift apart.
pub const PRELUDE_END_MARKER: &str = "// @smelt:prelude-end — generated program below";

/// Options for controlling code emission behavior.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct EmitOptions {
    /// The name of the Rust crate to generate.
    pub crate_name: String,
    /// The Rust crate target kind to generate.
    pub crate_kind: CrateKind,
}

impl Default for EmitOptions {
    /// Returns the default emission options with crate name "smelt_app".
    fn default() -> Self {
        Self {
            crate_name: "smelt_app".to_owned(),
            crate_kind: CrateKind::Program,
        }
    }
}

impl EmitOptions {
    /// Creates program emission options for the given Rust crate name.
    pub fn new(crate_name: impl Into<String>) -> Self {
        Self {
            crate_name: crate_name.into(),
            ..Self::default()
        }
    }

    /// Sets whether Smelt emits a program or library crate root.
    #[must_use]
    pub fn with_crate_kind(mut self, crate_kind: CrateKind) -> Self {
        self.crate_kind = crate_kind;
        self
    }
}

/// Rust crate target kind emitted by Smelt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CrateKind {
    /// Generate an executable Rust program rooted at `src/main.rs`.
    Program,
    /// Generate a Rust library crate rooted at `src/lib.rs`.
    Library,
}

impl CrateKind {
    /// Returns the Rust source root file name for this crate kind.
    #[must_use]
    fn root_file_name(self) -> &'static str {
        match self {
            Self::Program => "main.rs",
            Self::Library => "lib.rs",
        }
    }

    /// Returns the Rust source root file name for the other crate kind.
    #[must_use]
    fn stale_root_file_name(self) -> &'static str {
        match self {
            Self::Program => "lib.rs",
            Self::Library => "main.rs",
        }
    }
}

/// An error encountered during code emission.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EmitError {
    /// The error message.
    pub message: String,
}

impl std::fmt::Display for EmitError {
    /// Formats the error message for display.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for EmitError {}

impl EmitError {
    /// Creates a new emit error with the given message.
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Converts a compact MIR identifier into a `usize` for indexing.
fn id_index(index: u32, context: &'static str) -> Result<usize, EmitError> {
    usize::try_from(index).map_err(|_err| EmitError::new(context))
}

/// Render the Rust array literal of host-object identity markers hidden from
/// JavaScript `for...in` / `Object.keys` enumeration.
///
/// The host-object identity markers (`__smelt_arraybuffer`, `__smelt_weakmap`,
/// the boxed-primitive markers, ...) come from the shared
/// `smelt_stdlib::host_object` registry so this runtime filter, the frontend
/// construction path, and the `instanceof` codegen path share one source of
/// truth. Appended to that are the markers owned by other runtime subsystems
/// (abort controllers/signals, builtin namespaces, the global object) whose
/// records must equally hide their internal keys but which are not part of the
/// host-object registry proper.
fn host_marker_registry_array() -> String {
    let markers = smelt_stdlib::host_object_markers()
        .chain([
            "__smelt_abortcontroller",
            "__smelt_abortsignal",
            "__smelt_builtin_namespace",
            "__smelt_global_object",
        ])
        .map(|marker| format!("\"{marker}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{markers}]")
}

/// Converts a `usize` into a compact MIR identifier.
fn compact_index(index: usize, context: &'static str) -> Result<u32, EmitError> {
    u32::try_from(index).map_err(|_err| EmitError::new(context))
}

/// Emits a complete Rust crate from the given MIR.
///
/// Creates the crate structure with Cargo.toml and the configured crate root.
pub fn emit_crate(
    mir: &Mir,
    output_path: impl AsRef<Path>,
    options: &EmitOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = output_path.as_ref();
    let src_dir = output_dir.join("src");
    fs::create_dir_all(&src_dir)?;
    write_if_changed(
        output_dir.join("Cargo.toml"),
        &deps::cargo_toml(&options.crate_name, &generated_deps(mir)),
    )?;
    write_crate_root(&src_dir, options.crate_kind, &emit_source(mir)?)?;
    Ok(())
}

/// Emits a complete Rust crate while preserving the source module layout.
///
/// Shared generated runtime/types stay in the configured crate root. Non-entry
/// module-level functions are moved into source-shaped Rust modules and
/// re-exported from the crate root so existing flat-name call emission
/// continues to resolve.
pub fn emit_crate_with_modules(
    mir: &Mir,
    krate: &smelt_hir::Crate,
    modules: &[(String, smelt_hir::ModuleId)],
    output_path: impl AsRef<Path>,
    options: &EmitOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = output_path.as_ref();
    let src_dir = output_dir.join("src");
    fs::create_dir_all(&src_dir)?;
    write_if_changed(
        output_dir.join("Cargo.toml"),
        &deps::cargo_toml(&options.crate_name, &generated_deps(mir)),
    )?;

    let mapped = emit_mapped_sources(mir, krate, modules)?;
    write_crate_root(&src_dir, options.crate_kind, &mapped.root)?;
    for module in mapped.modules {
        let module_path = src_dir.join(format!("{}.rs", module.name));
        write_if_changed(module_path, &module.source)?;
    }
    Ok(())
}

/// Writes the configured crate root and removes the stale opposite root.
///
/// Cargo infers program and library targets from `src/main.rs` and `src/lib.rs`.
/// Removing the other generated root keeps manifest kind changes from silently
/// producing both targets in the same output directory.
fn write_crate_root(
    src_dir: &Path,
    crate_kind: CrateKind,
    contents: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    write_if_changed(src_dir.join(crate_kind.root_file_name()), contents)?;
    let stale_root = src_dir.join(crate_kind.stale_root_file_name());
    if stale_root.exists() {
        fs::remove_file(stale_root)?;
    }
    Ok(())
}

/// Writes generated text only when the destination bytes actually change.
///
/// Cargo uses file mtimes as part of its freshness checks. Rewriting every
/// generated Rust module on each Smelt build forces Cargo to recompile modules
/// whose source is byte-for-byte identical, which dominates large projects such
/// as Remeda. Keeping identical files untouched preserves incremental build
/// artifacts without changing Smelt's lowering or codegen semantics.
fn write_if_changed(
    path: impl AsRef<Path>,
    contents: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let destination = path.as_ref();
    if fs::read_to_string(destination).is_ok_and(|existing| existing == contents) {
        return Ok(());
    }
    fs::write(destination, contents)?;
    Ok(())
}

/// Returns whether generated Rust uses JavaScript timer helpers.
///
/// Scans function *and* closure rvalues: a timer op can live inside a
/// synthesized closure body (e.g. the first-class `setTimeout` value form),
/// and the prelude must still define the timer queue for it.
fn needs_timer_helpers(mir: &Mir) -> bool {
    stdlib::rvalues(mir).any(|value| {
        matches!(
            value,
            Rvalue::AsyncOp {
                op: AsyncOp::Sleep
                    | AsyncOp::SetTimeout
                    | AsyncOp::ClearTimeout
                    | AsyncOp::SetInterval
                    | AsyncOp::ClearInterval
                    | AsyncOp::Promise
                    | AsyncOp::Then
                    | AsyncOp::Catch
                    | AsyncOp::SpawnLocal,
                ..
            }
        )
    })
}

/// Emits Rust source code from the given MIR.
///
/// Returns a string containing the complete source code for the MIR, including
/// struct definitions, free functions, and impl blocks for methods.
pub fn emit_source(mir: &Mir) -> Result<String, EmitError> {
    emit_source_with_free_function_router(mir, |_function, _context, source| Ok(Some(source)))
}

/// Emits Rust source code while allowing callers to route free functions.
///
/// The router receives each already-emitted free function exactly once. Returning
/// `Some(source)` keeps the function in the crate root; returning `None` lets the
/// caller store it elsewhere, such as in source-shaped sibling modules.
fn emit_source_with_free_function_router(
    mir: &Mir,
    mut route_free_function: impl FnMut(
        &MirFunction,
        &EmitContext,
        String,
    ) -> Result<Option<String>, EmitError>,
) -> Result<String, EmitError> {
    let mut writer = CodeWriter::new();
    let context = EmitContext::new(mir)?;
    // Decide, once, which free functions emit real Rust generics (signature
    // safety plus a body-cleanliness trial) so the definition and every call
    // site agree on the generic-vs-erased shape.
    context.populate_generic_functions(mir)?;
    let needs_serde_json =
        stdlib::backend_dependencies(mir).contains(&smelt_stdlib::BackendDependency::SerdeJson);
    let needs_regex =
        stdlib::backend_dependencies(mir).contains(&smelt_stdlib::BackendDependency::Regex);
    let needs_unknown = stdlib::needs_unknown_type(mir);
    let needs_smelt_list = stdlib::needs_smelt_list(mir);
    let needs_erased_function = needs_erased_function_runtime(mir);
    let needs_date_now = stdlib::needs_date_now_runtime(mir);
    let needs_date_timezone_offset = stdlib::needs_date_timezone_offset_runtime(mir);
    let needs_blob_record = stdlib::needs_blob_record_runtime(mir);
    let needs_host_override = stdlib::needs_host_override_runtime(mir);
    let needs_shared_captures = mir
        .closures
        .iter()
        .any(|closure| !closure.captures.is_empty());
    let needs_timer_helpers = needs_timer_helpers(mir);
    writer.line("// @generated by smelt. Do not edit by hand.");
    writer.line("#![allow(dead_code, non_snake_case, unused_imports, unused_variables)]");
    writer.blank_line();
    if needs_date_now {
        writer.line("thread_local! {");
        writer.line("    static SMELT_DATE_NOW: ::std::cell::Cell<Option<i64>> = const { ::std::cell::Cell::new(None) };");
        writer.line("}");
        writer.blank_line();
    }
    if needs_date_timezone_offset {
        writer.line("thread_local! {");
        writer.line("    static SMELT_DATE_TIMEZONE_OFFSET: ::std::cell::Cell<f64> = const { ::std::cell::Cell::new(0.0) };");
        writer.line("}");
        writer.blank_line();
    }
    if needs_host_override {
        // Bounded host-global override support: a fixed override-state enum, one
        // `thread_local!` slot per host name the crate reassigns (fresh `Native`
        // per test thread), and the three fixed helpers. Gated on any
        // `globalThis.<Name> =` write, so crates that never reassign a host
        // global emit byte-identical output.
        writer.line("/// Override state of a modeled host constructor's global slot.");
        writer.line("///");
        writer.line("/// `Native` is the unmodified builtin; `Absent` is an explicit");
        writer.line("/// `globalThis.X = undefined`; `Ctor` holds a reassigned constructor value.");
        writer.line(format!(
            "#[derive(Clone)] enum {enum_name} {{ Native, Absent, Ctor(SmeltUnknown) }}",
            enum_name = smelt_stdlib::runtime_symbols::host_override::OVERRIDE_ENUM,
        ));
        writer.blank_line();
        writer.line("thread_local! {");
        for (suffix, _name) in stdlib::host_override_slot_names(mir) {
            writer.line(format!(
                "    static {prefix}{suffix}: ::std::cell::RefCell<{enum_name}> = const {{ ::std::cell::RefCell::new({enum_name}::Native) }};",
                prefix = smelt_stdlib::runtime_symbols::host_override::SLOT_PREFIX,
                enum_name = smelt_stdlib::runtime_symbols::host_override::OVERRIDE_ENUM,
            ));
        }
        writer.line("}");
        writer.blank_line();
        writer.line("/// Read a host override slot: native-handle marker for `Native`, the");
        writer.line("/// stored constructor for `Ctor`, JS `undefined` for `Absent`.");
        writer.line(format!(
            "fn {read}(slot: &::std::cell::RefCell<{enum_name}>, name: &str) -> SmeltUnknown {{",
            read = smelt_stdlib::runtime_symbols::host_override::READ,
            enum_name = smelt_stdlib::runtime_symbols::host_override::OVERRIDE_ENUM,
        ));
        writer.line(format!(
            "    match &*slot.borrow() {{ {enum_name}::Native => {{ let mut entries = ::std::collections::HashMap::new(); entries.insert({marker:?}.to_owned(), SmeltUnknown::Bool(true)); entries.insert(\"name\".to_owned(), SmeltUnknown::String(name.to_owned())); SmeltUnknown::Object(SmeltObject::new(entries)) }}, {enum_name}::Absent => SmeltUnknown::Undefined, {enum_name}::Ctor(value) => value.clone() }}",
            enum_name = smelt_stdlib::runtime_symbols::host_override::OVERRIDE_ENUM,
            marker = smelt_stdlib::runtime_symbols::host_override::NATIVE_CTOR_MARKER,
        ));
        writer.line("}");
        writer.blank_line();
        writer.line("/// Classify and store a written host-global value; returns the stored value.");
        writer.line("///");
        writer.line("/// JS `undefined` -> `Absent`; a native-handle marker record -> `Native`");
        writer.line("/// (the save/restore round trip); any function/class value -> `Ctor`.");
        writer.line(format!(
            "fn {write}(slot: &::std::cell::RefCell<{enum_name}>, value: SmeltUnknown) -> SmeltUnknown {{",
            write = smelt_stdlib::runtime_symbols::host_override::WRITE,
            enum_name = smelt_stdlib::runtime_symbols::host_override::OVERRIDE_ENUM,
        ));
        writer.line(format!(
            "    let state = match &value {{ SmeltUnknown::Undefined => {enum_name}::Absent, SmeltUnknown::Object(entries) if entries.contains_key({marker:?}) => {enum_name}::Native, _ => {enum_name}::Ctor(value.clone()) }}; *slot.borrow_mut() = state; value",
            enum_name = smelt_stdlib::runtime_symbols::host_override::OVERRIDE_ENUM,
            marker = smelt_stdlib::runtime_symbols::host_override::NATIVE_CTOR_MARKER,
        ));
        writer.line("}");
        writer.blank_line();
        writer.line("/// Whether a host override slot is present (`false` only when `Absent`).");
        writer.line(format!(
            "fn {present}(slot: &::std::cell::RefCell<{enum_name}>) -> bool {{ !matches!(&*slot.borrow(), {enum_name}::Absent) }}",
            present = smelt_stdlib::runtime_symbols::host_override::PRESENT,
            enum_name = smelt_stdlib::runtime_symbols::host_override::OVERRIDE_ENUM,
        ));
        writer.blank_line();
    }
    if needs_shared_captures {
        writer.line("thread_local! {");
        writer.line("    static SMELT_NEXT_CAPTURE_SCOPE: ::std::cell::Cell<usize> = const { ::std::cell::Cell::new(1) };");
        writer.line("    static SMELT_SHARED_CAPTURES: ::std::cell::RefCell<::std::collections::HashMap<(usize, usize), ::std::rc::Weak<dyn ::std::any::Any>>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());");
        writer.line("}");
        writer.blank_line();
        writer.line("fn smelt_next_capture_scope() -> usize {");
        writer.line("    SMELT_NEXT_CAPTURE_SCOPE.with(|next| { let id = next.get(); next.set(id.saturating_add(1)); id })");
        writer.line("}");
        writer.blank_line();
        writer.line("fn smelt_shared_capture<T: Clone + 'static>(scope: usize, slot: *mut T, initial: T) -> ::std::rc::Rc<::std::cell::RefCell<T>> {");
        writer.line("    let key = (scope, slot as usize);");
        writer.line("    SMELT_SHARED_CAPTURES.with(|captures| {");
        writer.line("        let mut captures = captures.borrow_mut();");
        writer.line("        if let Some(existing) = captures.get(&key).and_then(::std::rc::Weak::upgrade) {");
        writer.line("            return existing.downcast::<::std::cell::RefCell<T>>().expect(\"shared capture type mismatch\");");
        writer.line("        }");
        writer.line("        let value: ::std::rc::Rc<::std::cell::RefCell<T>> = ::std::rc::Rc::new(::std::cell::RefCell::new(initial));");
        writer.line("        let erased: ::std::rc::Rc<dyn ::std::any::Any> = value.clone();");
        writer.line("        captures.insert(key, ::std::rc::Rc::downgrade(&erased));");
        writer.line("        value");
        writer.line("    })");
        writer.line("}");
        writer.blank_line();
    }
    // `smelt_next_object_id` mints fresh JavaScript object reference ids. It is
    // emitted in the `needs_smelt_list` block below (a list mints ids), but a
    // regex/match program without any list still needs it for
    // `SmeltMatch::from_captures` (a match carries an id so it keeps a stable
    // identity when later erased to `SmeltUnknown`). Emit it standalone only in
    // that regex-without-list case so list-using programs keep byte-identical
    // output. `needs_smelt_list` already subsumes `needs_unknown`.
    if stdlib::needs_uri_encode_runtime(mir) {
        writer.line("/// JavaScript `encodeURI`: percent-encode `value` as a full URI.");
        writer.line("///");
        writer.line("/// Leaves the ECMA-262 `encodeURI` unescaped set intact — ASCII alphanumerics,");
        writer.line("/// the unreserved marks `- _ . ! ~ * ' ( )`, the URI reserved separators");
        writer.line("/// `; / ? : @ & = + $ ,`, and `#` — and percent-encodes every other character's");
        writer.line("/// UTF-8 bytes as uppercase `%XX` triplets. Rust `&str` is always valid UTF-8,");
        writer.line("/// so ECMA-262's lone-surrogate `URIError` case cannot occur here.");
        writer.line(format!(
            "fn {encode_uri}(value: &str) -> String {{",
            encode_uri = smelt_stdlib::runtime_symbols::strings::ENCODE_URI,
        ));
        writer.line("    use ::std::fmt::Write as _;");
        writer.line("    let mut encoded = String::with_capacity(value.len());");
        writer.line("    for ch in value.chars() {");
        writer.line("        let unescaped = ch.is_ascii_alphanumeric()");
        writer.line("            || matches!(ch, '-' | '_' | '.' | '!' | '~' | '*' | '\\'' | '(' | ')'");
        writer.line("                | ';' | '/' | '?' | ':' | '@' | '&' | '=' | '+' | '$' | ',' | '#');");
        writer.line("        if unescaped {");
        writer.line("            encoded.push(ch);");
        writer.line("        } else {");
        writer.line("            let mut buffer = [0u8; 4];");
        writer.line("            for byte in ch.encode_utf8(&mut buffer).as_bytes() {");
        writer.line("                let _ = write!(encoded, \"%{byte:02X}\");");
        writer.line("            }");
        writer.line("        }");
        writer.line("    }");
        writer.line("    encoded");
        writer.line("}");
        writer.blank_line();
    }
    if needs_regex && !needs_smelt_list {
        writer.line("thread_local! {");
        writer.line("    static SMELT_NEXT_OBJECT_ID: ::std::cell::Cell<usize> = const { ::std::cell::Cell::new(1) };");
        writer.line("}");
        writer.blank_line();
        writer.line("#[allow(dead_code)]");
        writer.line("fn smelt_next_object_id() -> usize {");
        writer.line("    SMELT_NEXT_OBJECT_ID.with(|next| { let id = next.get(); next.set(id.saturating_add(1)); id })");
        writer.line("}");
        writer.blank_line();
    }
    if needs_smelt_list {
        // Identity-bearing statically-typed list — `Type::List` lowers to this.
        // `Deref`s to its backing `Vec<T>`; `Clone` shares the JS reference id
        // (so internal value-copies keep identity) while deep-cloning values.
        // Emitted whenever a list is used, independent of `SmeltUnknown`; the
        // `SmeltUnknown`-dependent impls (erase/From<SmeltArray>/serde) live in
        // the `needs_unknown` block. `smelt_next_object_id` lives here too because
        // `SmeltList::new` mints a fresh id (and `needs_unknown` implies this gate).
        writer.line("thread_local! {");
        writer.line("    static SMELT_NEXT_OBJECT_ID: ::std::cell::Cell<usize> = const { ::std::cell::Cell::new(1) };");
        writer.line("}");
        writer.blank_line();
        writer.line("#[allow(dead_code)]");
        writer.line("fn smelt_next_object_id() -> usize {");
        writer.line("    SMELT_NEXT_OBJECT_ID.with(|next| { let id = next.get(); next.set(id.saturating_add(1)); id })");
        writer.line("}");
        writer.blank_line();
        writer.line("pub struct SmeltList<T> {");
        writer.line("    id: usize,");
        writer.line("    values: Vec<T>,");
        writer.line("}");
        // Debug forwards to the backing Vec so `console.log([1,2,3])` prints
        // `[1.0, 2.0, 3.0]`, not the `SmeltList { .. }` wrapper.
        writer.line("impl<T: ::std::fmt::Debug> ::std::fmt::Debug for SmeltList<T> { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { self.values.fmt(formatter) } }");
        writer.line("impl<T: Clone> Clone for SmeltList<T> { fn clone(&self) -> Self { Self { id: self.id, values: self.values.clone() } } }");
        writer.line("#[allow(dead_code)]");
        writer.line("impl<T> SmeltList<T> {");
        writer.line(
            "    /// Create an identity-bearing typed list with a fresh JS reference identity.",
        );
        writer.line(
            "    fn new(values: Vec<T>) -> Self { Self { id: smelt_next_object_id(), values } }",
        );
        writer.line("    /// Reuse a caller-supplied identity so an erase/extract round-trip stays `===` equal.");
        writer.line("    fn with_id(id: usize, values: Vec<T>) -> Self { Self { id, values } }");
        writer.line(
            "    /// A JS array copy (`[...a]`, `slice`): same contents, a NEW reference identity.",
        );
        writer.line(
            "    fn fresh_copy(&self) -> Self where T: Clone { Self::new(self.values.clone()) }",
        );
        writer.line("    /// JS reference identity of this list.");
        writer.line("    fn id(&self) -> usize { self.id }");
        writer.line("    /// Consume the list, yielding the backing storage.");
        writer.line("    fn into_vec(self) -> Vec<T> { self.values }");
        writer.line("}");
        writer.line("impl<T> From<Vec<T>> for SmeltList<T> { fn from(values: Vec<T>) -> Self { Self::new(values) } }");
        writer.line("impl<T> ::std::iter::FromIterator<T> for SmeltList<T> { fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self { Self::new(iter.into_iter().collect()) } }");
        writer.line("impl<T> ::std::ops::Deref for SmeltList<T> { type Target = Vec<T>; fn deref(&self) -> &Vec<T> { &self.values } }");
        writer.line("impl<T> ::std::ops::DerefMut for SmeltList<T> { fn deref_mut(&mut self) -> &mut Vec<T> { &mut self.values } }");
        writer.line("impl<T> IntoIterator for SmeltList<T> { type Item = T; type IntoIter = ::std::vec::IntoIter<T>; fn into_iter(self) -> Self::IntoIter { self.values.into_iter() } }");
        writer.line("impl<'smelt_list, T> IntoIterator for &'smelt_list SmeltList<T> { type Item = &'smelt_list T; type IntoIter = ::std::slice::Iter<'smelt_list, T>; fn into_iter(self) -> Self::IntoIter { self.values.iter() } }");
        writer.line("impl<T: PartialEq> PartialEq for SmeltList<T> { fn eq(&self, other: &Self) -> bool { self.values == other.values } }");
        writer.line("impl<T: PartialEq> Eq for SmeltList<T> {}");
        writer.line("impl<T: ::std::hash::Hash> ::std::hash::Hash for SmeltList<T> { fn hash<H: ::std::hash::Hasher>(&self, state: &mut H) { self.values.hash(state); } }");
        writer.line(
            "impl<T> Default for SmeltList<T> { fn default() -> Self { Self::new(Vec::new()) } }",
        );
        writer.line("impl<T> From<SmeltList<T>> for Vec<T> { fn from(list: SmeltList<T>) -> Self { list.values } }");
        writer.blank_line();
    }

    if needs_unknown {
        writer.line("use ::std::hash::Hash;");
        writer.blank_line();
        writer.line("#[derive(Debug)]");
        writer.line("pub struct SmeltRecord<K, V> {");
        writer.line("    id: usize,");
        writer.line(
            "    values: ::std::rc::Rc<::std::cell::RefCell<::std::collections::HashMap<K, V>>>,",
        );
        writer.line("    order: ::std::rc::Rc<::std::cell::RefCell<Vec<K>>>,");
        writer.line("}");
        writer.blank_line();
        // `smelt_next_object_id` is emitted in the `needs_smelt_list` block above
        // (which `needs_unknown` always implies), so it is in scope here.
        writer.line("thread_local! {");
        writer.line("    static SMELT_PROMISE_IDENTITIES: ::std::cell::RefCell<::std::collections::HashMap<usize, usize>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());");
        writer.line("}");
        writer.blank_line();
        writer.line("/// Return a stable erased promise id for a source future local.");
        writer.line("fn smelt_promise_identity(source_key: usize) -> usize {");
        writer.line("    SMELT_PROMISE_IDENTITIES.with(|identities| {");
        writer.line("        let mut identities = identities.borrow_mut();");
        writer.line("        if let Some(id) = identities.get(&source_key) { return *id; }");
        writer.line("        let id = smelt_next_object_id();");
        writer.line("        identities.insert(source_key, id);");
        writer.line("        id");
        writer.line("    })");
        writer.line("}");
        writer.blank_line();
        writer.line("thread_local! {");
        writer
            .line("    /// Map a source list's storage address to a stable erased-array identity.");
        writer.line("    static SMELT_LIST_IDENTITIES: ::std::cell::RefCell<::std::collections::HashMap<usize, usize>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());");
        writer.line("}");
        writer.blank_line();
        writer.line(
            "/// Return a stable erased-array id for a source list keyed on its `Vec` address.",
        );
        writer.line("///");
        writer.line("/// Erasing the SAME source list local twice must yield arrays that compare");
        writer.line("/// `===` equal (arrays compare by id). Keying on the live `Vec`'s storage");
        writer.line("/// address lets every erasure of one binding reuse a single id, while a");
        writer.line("/// fresh list (literal or transform result) still gets a fresh id from");
        writer.line("/// `SmeltArray::new`. KNOWN LIMITATION: `Vec::as_ptr` on an EMPTY `Vec`");
        writer.line("/// returns a shared dangling sentinel, so distinct empty list bindings can");
        writer.line("/// collide on one id; acceptable here because the targeted cases are");
        writer.line("/// non-empty.");
        writer.line("fn smelt_list_identity(source_key: usize) -> usize {");
        writer.line("    SMELT_LIST_IDENTITIES.with(|identities| {");
        writer.line("        let mut identities = identities.borrow_mut();");
        writer.line("        if let Some(id) = identities.get(&source_key) { return *id; }");
        writer.line("        let id = smelt_next_object_id();");
        writer.line("        identities.insert(source_key, id);");
        writer.line("        id");
        writer.line("    })");
        writer.line("}");
        writer.blank_line();
        writer.line("thread_local! {");
        writer.line("    static SMELT_FUNCTION_ORIGINS: ::std::cell::RefCell<::std::collections::HashMap<usize, Box<dyn ::std::any::Any>>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());");
        writer.line("}");
        writer.blank_line();
        writer.line("/// Return the stable key for an erased callback wrapper.");
        writer.line("fn smelt_erased_function_key(function: &::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>) -> usize {");
        writer.line("    ::std::rc::Rc::as_ptr(function) as *const () as usize");
        writer.line("}");
        writer.blank_line();
        writer.line("/// Retain typed callback identity while it crosses an erased ABI.");
        writer.line("fn smelt_register_function_origin<T: Clone + 'static>(function: &::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>, origin: T) {");
        writer.line("    SMELT_FUNCTION_ORIGINS.with(|origins| { origins.borrow_mut().insert(smelt_erased_function_key(function), Box::new(origin)); });");
        writer.line("}");
        writer.blank_line();
        writer.line("/// Recover a typed callback previously passed through an erased ABI.");
        writer.line("fn smelt_restore_function_origin<T: Clone + 'static>(function: &::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>) -> Option<T> {");
        writer.line("    SMELT_FUNCTION_ORIGINS.with(|origins| origins.borrow().get(&smelt_erased_function_key(function)).and_then(|origin| origin.downcast_ref::<T>()).cloned())");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K, V> Clone for SmeltRecord<K, V> {");
        writer.line(
            "    fn clone(&self) -> Self { Self { id: self.id, values: self.values.clone(), order: self.order.clone() } }",
        );
        writer.line("}");
        writer.blank_line();
        writer.line("trait SmeltOwnedOptionCloned<T> {");
        writer.line("    /// Return owned optional values unchanged when generated shared lookup code calls `.cloned()`.");
        writer.line("    fn cloned(self) -> Option<T>;");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<T> SmeltOwnedOptionCloned<T> for Option<T> {");
        writer.line("    fn cloned(self) -> Option<T> { self }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K: Eq + ::std::hash::Hash + Clone, V> SmeltRecord<K, V> {");
        writer.line("    fn new() -> Self { Self { id: smelt_next_object_id(), values: ::std::rc::Rc::new(::std::cell::RefCell::new(::std::collections::HashMap::new())), order: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())) } }");
        writer.line("    fn with_id(id: usize, values: ::std::collections::HashMap<K, V>) -> Self { let order = values.keys().cloned().collect::<Vec<_>>(); Self { id, values: ::std::rc::Rc::new(::std::cell::RefCell::new(values)), order: ::std::rc::Rc::new(::std::cell::RefCell::new(order)) } }");
        writer.line("    fn with_id_from_entries<I: IntoIterator<Item = (K, V)>>(id: usize, iter: I) -> Self { let record = Self { id, values: ::std::rc::Rc::new(::std::cell::RefCell::new(::std::collections::HashMap::new())), order: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())) }; record.extend(iter); record }");
        writer.line("    fn len(&self) -> usize { self.values.borrow().len() }");
        writer.line("    fn contains_key<Q>(&self, key: &Q) -> bool where K: ::std::borrow::Borrow<Q>, Q: Eq + ::std::hash::Hash + ?Sized { self.values.borrow().contains_key(key) }");
        writer.line("    fn insert(&self, key: K, value: V) -> Option<V> { if !self.values.borrow().contains_key(&key) { self.order.borrow_mut().push(key.clone()); } self.values.borrow_mut().insert(key, value) }");
        writer.line("    fn remove<Q>(&self, key: &Q) -> Option<V> where K: ::std::borrow::Borrow<Q>, Q: Eq + ::std::hash::Hash + ?Sized { let removed = self.values.borrow_mut().remove(key); if removed.is_some() { self.order.borrow_mut().retain(|existing| <K as ::std::borrow::Borrow<Q>>::borrow(existing) != key); } removed }");
        writer.line("    fn get<Q>(&self, key: &Q) -> Option<V> where K: ::std::borrow::Borrow<Q>, Q: Eq + ::std::hash::Hash + ?Sized, V: Clone { self.values.borrow().get(key).cloned() }");
        writer.line("    fn iter(&self) -> ::std::vec::IntoIter<(K, V)> where V: Clone { let values = self.values.borrow(); self.order.borrow().iter().filter_map(|key| values.get(key).map(|value| (key.clone(), value.clone()))).collect::<Vec<_>>().into_iter() }");
        writer.line("    fn keys(&self) -> ::std::vec::IntoIter<K> { self.order.borrow().clone().into_iter() }");
        writer.line("    fn values(&self) -> ::std::vec::IntoIter<V> where V: Clone { let values = self.values.borrow(); self.order.borrow().iter().filter_map(|key| values.get(key).cloned()).collect::<Vec<_>>().into_iter() }");
        writer.line("    fn extend<I: IntoIterator<Item = (K, V)>>(&self, iter: I) { for (key, value) in iter { self.insert(key, value); } }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K: Eq + ::std::hash::Hash + Clone, V> Default for SmeltRecord<K, V> {");
        writer.line("    fn default() -> Self { Self::new() }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K: Eq + ::std::hash::Hash + Clone, V, const N: usize> From<[(K, V); N]> for SmeltRecord<K, V> {");
        writer.line("    fn from(values: [(K, V); N]) -> Self { values.into_iter().collect() }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K: Eq + ::std::hash::Hash + Clone, V> ::std::iter::FromIterator<(K, V)> for SmeltRecord<K, V> {");
        writer.line("    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self { let record = Self::new(); record.extend(iter); record }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K: Eq + ::std::hash::Hash + Clone, V: Clone> IntoIterator for SmeltRecord<K, V> {");
        writer.line("    type Item = (K, V);");
        writer.line("    type IntoIter = ::std::vec::IntoIter<(K, V)>;");
        writer.line("    fn into_iter(self) -> Self::IntoIter { self.iter() }");
        writer.line("}");
        writer.blank_line();
        writer.line(
            "impl<K: Eq + ::std::hash::Hash, V: PartialEq> PartialEq for SmeltRecord<K, V> {",
        );
        writer.line("    fn eq(&self, other: &Self) -> bool { *self.values.borrow() == *other.values.borrow() }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K: Eq + ::std::hash::Hash, V: Eq> Eq for SmeltRecord<K, V> {}");
        writer.blank_line();
        writer.line("impl<K, V> PartialEq<::std::collections::HashMap<K, V>> for SmeltRecord<K, V> where K: Eq + ::std::hash::Hash, V: PartialEq {");
        writer.line("    fn eq(&self, other: &::std::collections::HashMap<K, V>) -> bool { self.values.borrow().eq(other) }");
        writer.line("}");
        writer.blank_line();
        writer.line("#[derive(Clone, Debug)]");
        writer.line("pub struct SmeltJsMap<K, V> {");
        writer.line("    entries: Vec<(K, V)>,");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K, V> SmeltJsMap<K, V> {");
        writer.line("    fn new() -> Self { Self { entries: Vec::new() } }");
        // `clear` removes every entry without comparing keys, so it needs no
        // `SmeltJsKeyEq`/`Clone` bounds and lives on the unbounded impl block.
        writer.line("    fn clear(&mut self) { self.entries.clear(); }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K: SmeltJsKeyEq + Clone, V: Clone> SmeltJsMap<K, V> {");
        writer.line("    fn len(&self) -> usize { self.entries.len() }");
        writer.line("    fn contains_key(&self, key: &K) -> bool { self.entries.iter().any(|(existing, _)| existing.same_js_key(key)) }");
        writer.line("    fn get(&self, key: &K) -> Option<V> { self.entries.iter().find(|(existing, _)| existing.same_js_key(key)).map(|(_, value)| value.clone()) }");
        writer.line("    fn insert(&mut self, key: K, value: V) -> Option<V> { if let Some((_, existing)) = self.entries.iter_mut().find(|(existing, _)| existing.same_js_key(&key)) { Some(::std::mem::replace(existing, value)) } else { self.entries.push((key, value)); None } }");
        writer.line("    fn remove(&mut self, key: &K) -> Option<V> { if let Some(index) = self.entries.iter().position(|(existing, _)| existing.same_js_key(key)) { Some(self.entries.remove(index).1) } else { None } }");
        writer.line("    fn iter(&self) -> ::std::vec::IntoIter<(K, V)> { self.entries.clone().into_iter() }");
        writer.line("    fn keys(&self) -> ::std::vec::IntoIter<K> { self.entries.iter().map(|(key, _)| key.clone()).collect::<Vec<_>>().into_iter() }");
        writer.line("    fn values(&self) -> ::std::vec::IntoIter<V> { self.entries.iter().map(|(_, value)| value.clone()).collect::<Vec<_>>().into_iter() }");
        writer.line("    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) { for (key, value) in iter { self.insert(key, value); } }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K, V> Default for SmeltJsMap<K, V> {");
        writer.line("    fn default() -> Self { Self::new() }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K, V, const N: usize> From<[(K, V); N]> for SmeltJsMap<K, V> {");
        writer.line(
            "    fn from(entries: [(K, V); N]) -> Self { Self { entries: Vec::from(entries) } }",
        );
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K, V> ::std::iter::FromIterator<(K, V)> for SmeltJsMap<K, V> {");
        writer.line("    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self { Self { entries: iter.into_iter().collect() } }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K, V> IntoIterator for SmeltJsMap<K, V> {");
        writer.line("    type Item = (K, V);");
        writer.line("    type IntoIter = ::std::vec::IntoIter<(K, V)>;");
        writer.line("    fn into_iter(self) -> Self::IntoIter { self.entries.into_iter() }");
        writer.line("}");
        writer.blank_line();
        writer.line(
            "impl<K: SmeltJsKeyEq + Clone, V: PartialEq + Clone> PartialEq for SmeltJsMap<K, V> {",
        );
        writer.line("    fn eq(&self, other: &Self) -> bool { self.entries.len() == other.entries.len() && self.entries.iter().all(|(key, value)| other.get(key).is_some_and(|other_value| other_value == *value)) }");
        writer.line("}");
        writer.line("impl<K: SmeltJsKeyEq + Clone, V: Eq + Clone> Eq for SmeltJsMap<K, V> {}");
        writer.blank_line();
        // JS `Set` container with SameValueZero membership and insertion order.
        //
        // Rust `HashSet` demands `Eq + Hash`, which is impossible for `f64`,
        // generated unions, and generic type parameters, and is the wrong
        // equality besides: JavaScript `Set` compares objects/functions by
        // reference identity and treats `NaN` as equal to itself. Rather than
        // require a per-element `Eq + Hash`, membership projects each element
        // through its `IntoSmeltUnknown` erasure and compares the resulting
        // runtime values with `SmeltJsKeyEq::same_js_key` (the same erased-key
        // projection `SmeltJsMap` uses for keys). This makes one uniform,
        // JS-correct container work for every element type that can be erased.
        // Elements are stored in a `Vec` so iteration preserves insertion order
        // like a real JS `Set`.
        writer.line("#[derive(Clone, Debug)]");
        writer.line("pub struct SmeltJsSet<T> {");
        writer.line("    entries: Vec<T>,");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<T> SmeltJsSet<T> {");
        writer.line("    fn new() -> Self { Self { entries: Vec::new() } }");
        writer.line("    fn clear(&mut self) { self.entries.clear(); }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<T: Clone + IntoSmeltUnknown> SmeltJsSet<T> {");
        writer.line("    /// SameValueZero equality via each element's erased runtime value.");
        writer.line("    fn same_member(left: &T, right: &T) -> bool { left.clone().into_smelt_unknown().same_js_key(&right.clone().into_smelt_unknown()) }");
        writer.line("    fn len(&self) -> usize { self.entries.len() }");
        writer.line("    fn is_empty(&self) -> bool { self.entries.is_empty() }");
        writer.line("    fn contains(&self, value: &T) -> bool { self.entries.iter().any(|existing| Self::same_member(existing, value)) }");
        writer.line("    fn insert(&mut self, value: T) -> bool { if self.contains(&value) { false } else { self.entries.push(value); true } }");
        writer.line("    fn remove(&mut self, value: &T) -> bool { if let Some(index) = self.entries.iter().position(|existing| Self::same_member(existing, value)) { self.entries.remove(index); true } else { false } }");
        writer.line("    fn iter(&self) -> ::std::slice::Iter<'_, T> { self.entries.iter() }");
        writer.line("    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) { for value in iter { self.insert(value); } }");
        writer.line("    fn is_disjoint(&self, other: &Self) -> bool { self.entries.iter().all(|value| !other.contains(value)) }");
        writer.line("    fn is_subset(&self, other: &Self) -> bool { self.entries.iter().all(|value| other.contains(value)) }");
        writer.line("    fn is_superset(&self, other: &Self) -> bool { other.is_subset(self) }");
        writer.line("    fn union<'smelt_set>(&'smelt_set self, other: &'smelt_set Self) -> ::std::vec::IntoIter<&'smelt_set T> { let mut out: Vec<&T> = self.entries.iter().collect(); for value in other.entries.iter() { if !out.iter().any(|existing| Self::same_member(existing, value)) { out.push(value); } } out.into_iter() }");
        writer.line("    fn intersection<'smelt_set>(&'smelt_set self, other: &'smelt_set Self) -> ::std::vec::IntoIter<&'smelt_set T> { self.entries.iter().filter(|value| other.contains(value)).collect::<Vec<_>>().into_iter() }");
        writer.line("    fn difference<'smelt_set>(&'smelt_set self, other: &'smelt_set Self) -> ::std::vec::IntoIter<&'smelt_set T> { self.entries.iter().filter(|value| !other.contains(value)).collect::<Vec<_>>().into_iter() }");
        writer.line("    fn symmetric_difference<'smelt_set>(&'smelt_set self, other: &'smelt_set Self) -> ::std::vec::IntoIter<&'smelt_set T> { let mut out: Vec<&T> = self.entries.iter().filter(|value| !other.contains(value)).collect(); for value in other.entries.iter() { if !self.contains(value) { out.push(value); } } out.into_iter() }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<T> Default for SmeltJsSet<T> { fn default() -> Self { Self::new() } }");
        writer.line("impl<T: Clone + IntoSmeltUnknown, const N: usize> From<[T; N]> for SmeltJsSet<T> { fn from(values: [T; N]) -> Self { values.into_iter().collect() } }");
        writer.line("impl<T: Clone + IntoSmeltUnknown> ::std::iter::FromIterator<T> for SmeltJsSet<T> { fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self { let mut set = Self::new(); set.extend(iter); set } }");
        writer.line("impl<T> IntoIterator for SmeltJsSet<T> { type Item = T; type IntoIter = ::std::vec::IntoIter<T>; fn into_iter(self) -> Self::IntoIter { self.entries.into_iter() } }");
        writer.line("impl<'smelt_set, T> IntoIterator for &'smelt_set SmeltJsSet<T> { type Item = &'smelt_set T; type IntoIter = ::std::slice::Iter<'smelt_set, T>; fn into_iter(self) -> Self::IntoIter { self.entries.iter() } }");
        writer.line("impl<T: Clone + IntoSmeltUnknown> PartialEq for SmeltJsSet<T> { fn eq(&self, other: &Self) -> bool { self.entries.len() == other.entries.len() && self.entries.iter().all(|value| other.contains(value)) } }");
        writer.line("impl<T: IntoSmeltUnknown> IntoSmeltUnknown for SmeltJsSet<T> { fn into_smelt_unknown(self) -> SmeltUnknown { let mut values = self.entries.into_iter().map(IntoSmeltUnknown::into_smelt_unknown).collect::<Vec<_>>(); values.sort_by_key(smelt_unknown_stable_hash_key); SmeltUnknown::Array(values.into()) } }");
        writer.blank_line();
        writer.line("pub trait SmeltJsKeyEq {");
        writer.line("    fn same_js_key(&self, other: &Self) -> bool;");
        writer.line("}");
        writer.blank_line();
        writer.line("impl SmeltJsKeyEq for SmeltUnknown {");
        writer.line("    fn same_js_key(&self, other: &Self) -> bool { match (self, other) { (SmeltUnknown::Number(left), SmeltUnknown::Number(right)) if left.is_nan() && right.is_nan() => true, (SmeltUnknown::Array(left), SmeltUnknown::Array(right)) => left.id == right.id, (SmeltUnknown::Object(left), SmeltUnknown::Object(right)) => left.id == right.id, (SmeltUnknown::Function(left), SmeltUnknown::Function(right)) => ::std::rc::Rc::ptr_eq(left, right), (SmeltUnknown::Promise(left), SmeltUnknown::Promise(right)) => left.id == right.id, _ => self == other } }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl SmeltJsKeyEq for String { fn same_js_key(&self, other: &Self) -> bool { self == other } }");
        writer.line("impl SmeltJsKeyEq for bool { fn same_js_key(&self, other: &Self) -> bool { self == other } }");
        writer.line("impl SmeltJsKeyEq for i64 { fn same_js_key(&self, other: &Self) -> bool { self == other } }");
        writer.line("impl SmeltJsKeyEq for f64 { fn same_js_key(&self, other: &Self) -> bool { (self.is_nan() && other.is_nan()) || self == other } }");
        // A record/object used as a collection key compares by JavaScript
        // reference identity (its stable object `id`), matching `same_js_key`'s
        // object arm on the erased value. This lets a `Set`/`Map`/cache keyed by
        // a concrete `SmeltRecord` resolve `SmeltJsKeyEq` without erasing to
        // `SmeltUnknown` (was E0599: unsatisfied `SmeltJsKeyEq` bound).
        writer.line("impl<K, V> SmeltJsKeyEq for SmeltRecord<K, V> { fn same_js_key(&self, other: &Self) -> bool { self.id == other.id } }");
        writer.blank_line();
        // The fourth equality kind: JavaScript strict equality (`===`/`!==`).
        // Distinct from the other three — objects/arrays/functions compare by
        // REFERENCE identity (id/ptr), primitives by value, and `NaN !== NaN`
        // (unlike `same_js_key`'s SameValueZero and `Object.is`'s SameValue, which
        // both treat NaN as equal). `+0 === -0` holds because `f64 ==` is true for
        // them. Unlike `PartialEq` (`smelt_unknown_structural_eq`, intentionally
        // deep for `isDeepEqual`), this never recurses into object/array contents.
        // A trait (not an inherent method) with primitive impls so erased operands
        // that lower to a concrete `String`/`bool`/number still resolve the method.
        writer.line("pub trait SmeltJsStrictEq {");
        writer.line("    fn js_strict_eq(&self, other: &Self) -> bool;");
        writer.line("}");
        writer.line("impl SmeltJsStrictEq for SmeltUnknown {");
        writer.line("    fn js_strict_eq(&self, other: &Self) -> bool { match (self, other) { (SmeltUnknown::Null, SmeltUnknown::Null) => true, (SmeltUnknown::Undefined, SmeltUnknown::Undefined) => true, (SmeltUnknown::Bool(left), SmeltUnknown::Bool(right)) => left == right, (SmeltUnknown::Number(left), SmeltUnknown::Number(right)) => left == right, (SmeltUnknown::String(left), SmeltUnknown::String(right)) => left == right, (SmeltUnknown::Symbol(left), SmeltUnknown::Symbol(right)) => left == right, (SmeltUnknown::Array(left), SmeltUnknown::Array(right)) => left.id == right.id, (SmeltUnknown::Object(left), SmeltUnknown::Object(right)) => left.id == right.id, (SmeltUnknown::Function(left), SmeltUnknown::Function(right)) => ::std::rc::Rc::ptr_eq(left, right), (SmeltUnknown::Promise(left), SmeltUnknown::Promise(right)) => left.id == right.id, _ => false } }");
        writer.line("}");
        writer.line("impl SmeltJsStrictEq for String { fn js_strict_eq(&self, other: &Self) -> bool { self == other } }");
        writer.line("impl SmeltJsStrictEq for bool { fn js_strict_eq(&self, other: &Self) -> bool { self == other } }");
        writer.line("impl SmeltJsStrictEq for i64 { fn js_strict_eq(&self, other: &Self) -> bool { self == other } }");
        writer.line("impl SmeltJsStrictEq for f64 { fn js_strict_eq(&self, other: &Self) -> bool { self == other } }");
        writer.blank_line();
        writer.line("#[derive(Debug)]");
        writer.line("pub struct SmeltObject {");
        writer.line("    id: usize,");
        writer.line("    values: ::std::rc::Rc<::std::cell::RefCell<::std::collections::HashMap<String, SmeltUnknown>>>,");
        writer.line("    order: ::std::rc::Rc<::std::cell::RefCell<Vec<String>>>,");
        writer.line("}");
        writer.blank_line();
        writer.line("impl Clone for SmeltObject { fn clone(&self) -> Self { Self { id: self.id, values: self.values.clone(), order: self.order.clone() } } }");
        writer.line("impl SmeltObject {");
        writer.line("    fn new(values: ::std::collections::HashMap<String, SmeltUnknown>) -> Self { let mut order = values.keys().cloned().collect::<Vec<_>>(); order.sort(); Self { id: smelt_next_object_id(), values: ::std::rc::Rc::new(::std::cell::RefCell::new(values)), order: ::std::rc::Rc::new(::std::cell::RefCell::new(order)) } }");
        writer.line("    fn with_id(id: usize, values: ::std::collections::HashMap<String, SmeltUnknown>) -> Self { let mut order = values.keys().cloned().collect::<Vec<_>>(); order.sort(); Self { id, values: ::std::rc::Rc::new(::std::cell::RefCell::new(values)), order: ::std::rc::Rc::new(::std::cell::RefCell::new(order)) } }");
        writer.line("    fn from_unknown_record(record: SmeltRecord<String, SmeltUnknown>) -> Self { Self { id: record.id, values: record.values, order: record.order } }");
        writer.line("    fn len(&self) -> usize { self.values.borrow().len() }");
        writer.line("    fn contains_key(&self, key: &str) -> bool { self.values.borrow().contains_key(key) }");
        writer.line("    fn get(&self, key: &str) -> Option<SmeltUnknown> { self.values.borrow().get(key).cloned() }");
        writer.line("    fn insert(&self, key: String, value: SmeltUnknown) -> Option<SmeltUnknown> { if !self.values.borrow().contains_key(&key) { self.order.borrow_mut().push(key.clone()); } self.values.borrow_mut().insert(key, value) }");
        writer.line("    fn remove(&self, key: &str) -> Option<SmeltUnknown> { let removed = self.values.borrow_mut().remove(key); if removed.is_some() { self.order.borrow_mut().retain(|existing| existing != key); } removed }");
        writer.line("    fn iter(&self) -> ::std::vec::IntoIter<(String, SmeltUnknown)> { let values = self.values.borrow(); self.order.borrow().iter().filter_map(|key| values.get(key).map(|value| (key.clone(), value.clone()))).collect::<Vec<_>>().into_iter() }");
        writer.line("    fn keys(&self) -> Vec<String> { self.order.borrow().clone() }");
        writer.line("    fn values(&self) -> Vec<SmeltUnknown> { let values = self.values.borrow(); self.order.borrow().iter().filter_map(|key| values.get(key).cloned()).collect() }");
        writer.line("}");
        writer.blank_line();
        writer.line(
            "/// Return whether an erased object key is visible to JavaScript `for...in` iteration.",
        );
        let host_marker_array = host_marker_registry_array();
        writer.line(format!("fn smelt_object_has_host_marker(object: &SmeltObject) -> bool {{ {host_marker_array}.iter().any(|marker| object.contains_key(marker)) }}"));
        writer.line(format!("fn smelt_record_has_host_marker<V>(record: &SmeltRecord<String, V>) -> bool {{ {host_marker_array}.iter().any(|marker| record.contains_key(*marker)) }}"));
        writer.line("fn smelt_is_for_in_object_key(object: &SmeltObject, key: &str) -> bool { if smelt_object_has_host_marker(object) { return false; } key != \"__smelt_date\" && key != \"__smelt_timezone\" && key != \"__smelt_class\" && !(object.contains_key(\"__smelt_regexp\") && matches!(key, \"__smelt_regexp\" | \"source\" | \"flags\")) && !(object.contains_key(\"__smelt_error\") && matches!(key, \"__smelt_error\" | \"message\" | \"cause\" | \"errors\")) }");
        writer
            .line("/// Return whether a record key is visible to JavaScript `for...in` iteration.");
        writer.line("fn smelt_is_for_in_record_key<V>(record: &SmeltRecord<String, V>, key: &str) -> bool { if smelt_record_has_host_marker(record) { return false; } key != \"__smelt_date\" && key != \"__smelt_timezone\" && key != \"__smelt_class\" && !(record.contains_key(\"__smelt_regexp\") && matches!(key, \"__smelt_regexp\" | \"source\" | \"flags\")) && !(record.contains_key(\"__smelt_error\") && matches!(key, \"__smelt_error\" | \"message\" | \"cause\" | \"errors\")) }");
        writer.line("/// Return the opaque `Object.getPrototypeOf` sentinel for an erased value.");
        writer.line(
            "/// Class instances carry a hidden `__smelt_class` marker and map to a distinct",
        );
        writer.line(
            "/// `\"__smelt_proto:class\"` sentinel so they are not treated as plain objects; arrays,",
        );
        writer.line("/// `null`, and plain objects keep their existing sentinels.");
        writer.line("fn smelt_prototype_sentinel(value: &SmeltUnknown) -> SmeltUnknown { match value { SmeltUnknown::Null => SmeltUnknown::Null, SmeltUnknown::Array(_) => SmeltUnknown::String(\"__smelt_proto:array\".to_owned()), SmeltUnknown::Promise(_) => SmeltUnknown::String(\"__smelt_proto:promise\".to_owned()), SmeltUnknown::Object(map) if map.contains_key(\"__smelt_class\") => SmeltUnknown::String(\"__smelt_proto:class\".to_owned()), _ => SmeltUnknown::String(\"__smelt_proto:object\".to_owned()) } }");
        writer.blank_line();
        writer.line("/// Resolve the JavaScript `Object.prototype.toString.call(x)` tag for an erased value.");
        writer.line("///");
        writer.line("/// Primitive and function variants map to their spec tags. Object records");
        writer.line("/// resolve through Smelt's host identity markers: dates, regexps, errors, the");
        writer.line("/// global object, abort records, registry host objects (WeakMap, Blob, boxed");
        writer.line("/// primitives, Intl instances, ...), and builtin namespace records (whose");
        writer.line("/// `@@toStringTag`-bearing `name` becomes the tag, matching `[object JSON]` /");
        writer.line("/// `[object Math]`). Class instances and unmarked records are plain");
        writer.line("/// `[object Object]`, exactly like JavaScript objects without a custom tag.");
        {
            let host_tag_arms = smelt_stdlib::HOST_OBJECTS.iter().fold(
                String::new(),
                |mut arms, entry| {
                    use ::std::fmt::Write as _;
                    let _ = write!(
                        arms,
                        "if map.contains_key(\"{marker}\") {{ return \"[object {tag}]\".to_owned(); }} ",
                        marker = entry.marker,
                        tag = entry.class_name,
                    );
                    arms
                },
            );
            writer.line(format!(
                "fn smelt_object_to_string_tag(value: &SmeltUnknown) -> String {{ match value {{ SmeltUnknown::Null => \"[object Null]\".to_owned(), SmeltUnknown::Undefined => \"[object Undefined]\".to_owned(), SmeltUnknown::Bool(_) => \"[object Boolean]\".to_owned(), SmeltUnknown::Number(_) => \"[object Number]\".to_owned(), SmeltUnknown::String(_) => \"[object String]\".to_owned(), SmeltUnknown::Symbol(_) => \"[object Symbol]\".to_owned(), SmeltUnknown::Array(_) => \"[object Array]\".to_owned(), SmeltUnknown::Function(_) => \"[object Function]\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned(), SmeltUnknown::Object(map) => {{ if map.contains_key(\"__smelt_date\") {{ return \"[object Date]\".to_owned(); }} if map.contains_key(\"__smelt_regexp\") {{ return \"[object RegExp]\".to_owned(); }} if map.contains_key(\"__smelt_error\") {{ return \"[object Error]\".to_owned(); }} if map.contains_key(\"__smelt_global_object\") {{ return \"[object global]\".to_owned(); }} if map.contains_key(\"__smelt_abortcontroller\") {{ return \"[object AbortController]\".to_owned(); }} if map.contains_key(\"__smelt_abortsignal\") {{ return \"[object AbortSignal]\".to_owned(); }} {host_tag_arms}if map.contains_key(\"__smelt_builtin_namespace\") {{ if let Some(SmeltUnknown::String(name)) = map.get(\"name\") {{ return format!(\"[object {{name}}]\"); }} }} \"[object Object]\".to_owned() }} }} }}",
            ));
        }
        writer.blank_line();
        writer.line("impl PartialEq for SmeltObject { fn eq(&self, other: &Self) -> bool { let mut smelt_seen = ::std::collections::HashSet::new(); smelt_object_structural_eq(self, other, &mut smelt_seen) } }");
        writer.line("impl Eq for SmeltObject {}");
        writer.line("impl ::std::hash::Hash for SmeltObject { fn hash<H: ::std::hash::Hasher>(&self, state: &mut H) { let mut smelt_seen = ::std::collections::HashSet::new(); smelt_object_structural_hash(self, state, &mut smelt_seen); } }");
        writer.line("impl IntoIterator for SmeltObject { type Item = (String, SmeltUnknown); type IntoIter = ::std::vec::IntoIter<(String, SmeltUnknown)>; fn into_iter(self) -> Self::IntoIter { self.iter() } }");
        writer.blank_line();
        writer.line("#[derive(Debug)]");
        writer.line("pub struct SmeltArray {");
        writer.line("    id: usize,");
        writer.line("    values: Vec<SmeltUnknown>,");
        writer.line("}");
        writer.blank_line();
        writer.line("impl Clone for SmeltArray { fn clone(&self) -> Self { Self { id: self.id, values: self.values.clone() } } }");
        writer.line("impl SmeltArray {");
        writer.line("    /// Create an identity-bearing erased JavaScript array.");
        writer.line("    fn new(values: Vec<SmeltUnknown>) -> Self { Self { id: smelt_next_object_id(), values } }");
        writer.line(
            "    /// Reuse a caller-supplied identity so repeated erasures of one source list compare `===` equal.",
        );
        writer.line(
            "    fn with_id(id: usize, values: Vec<SmeltUnknown>) -> Self { Self { id, values } }",
        );
        writer.line(
            "    /// Consume an erased array when lowering back to statically typed list storage.",
        );
        writer.line("    fn into_vec(self) -> Vec<SmeltUnknown> { self.values }");
        writer.line(
            "    /// Set the element at a numeric index, extending with `undefined` holes to match JS `arr[i] = v`.",
        );
        writer.line("    fn set_index(&mut self, index: usize, value: SmeltUnknown) { if index >= self.values.len() { self.values.resize(index.saturating_add(1), SmeltUnknown::Undefined); } self.values[index] = value; }");
        writer.line("}");
        writer.line("impl From<Vec<SmeltUnknown>> for SmeltArray { fn from(values: Vec<SmeltUnknown>) -> Self { Self::new(values) } }");
        writer.line("impl ::std::iter::FromIterator<SmeltUnknown> for SmeltArray { fn from_iter<T: IntoIterator<Item = SmeltUnknown>>(iter: T) -> Self { Self::new(iter.into_iter().collect()) } }");
        writer.line("impl ::std::ops::Deref for SmeltArray { type Target = [SmeltUnknown]; fn deref(&self) -> &Self::Target { &self.values } }");
        writer.line("impl IntoIterator for SmeltArray { type Item = SmeltUnknown; type IntoIter = ::std::vec::IntoIter<SmeltUnknown>; fn into_iter(self) -> Self::IntoIter { self.values.into_iter() } }");
        writer.blank_line();
        // `SmeltList<T>` itself is defined in the `needs_smelt_list` block above.
        // These impls depend on `SmeltArray`/`SmeltUnknown`, so they live here.
        // Erasing a typed list to a `SmeltUnknown::Array` preserves its JS reference identity.
        writer.line("impl From<SmeltList<SmeltUnknown>> for SmeltArray { fn from(list: SmeltList<SmeltUnknown>) -> Self { SmeltArray::with_id(list.id, list.values) } }");
        // serde impls only when the crate actually links serde (JSON contexts).
        if needs_serde_json {
            writer.line("impl<T: serde::Serialize> serde::Serialize for SmeltList<T> { fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> { serde::Serialize::serialize(&self.values, serializer) } }");
            writer.line("impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for SmeltList<T> { fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> { <Vec<T> as serde::Deserialize>::deserialize(deserializer).map(SmeltList::new) } }");
        }
        writer.line("type SmeltPromiseFuture = ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<SmeltUnknown, Box<dyn std::error::Error>>>>>;");
        writer.blank_line();
        writer.line("#[derive(Clone)]");
        writer.line("pub struct SmeltPromise {");
        writer.line("    id: usize,");
        writer.line(
            "    state: ::std::rc::Rc<::std::cell::RefCell<Option<Result<SmeltUnknown, String>>>>,",
        );
        writer.line("    future: ::std::rc::Rc<::std::cell::RefCell<Option<SmeltPromiseFuture>>>,");
        writer.line("}");
        writer.blank_line();
        writer.line("impl SmeltPromise {");
        writer.line("    /// Create a pending erased promise identity with shared settle state.");
        writer.line("    fn pending() -> Self { Self { id: smelt_next_object_id(), state: ::std::rc::Rc::new(::std::cell::RefCell::new(None)), future: ::std::rc::Rc::new(::std::cell::RefCell::new(None)) } }");
        writer.line("    /// Create a pending promise with a preassigned identity.");
        writer.line("    fn pending_with_id(id: usize) -> Self { Self { id, state: ::std::rc::Rc::new(::std::cell::RefCell::new(None)), future: ::std::rc::Rc::new(::std::cell::RefCell::new(None)) } }");
        writer.line("    /// Create an already-fulfilled erased promise value.");
        writer.line("    fn resolved(value: SmeltUnknown) -> Self { Self { id: smelt_next_object_id(), state: ::std::rc::Rc::new(::std::cell::RefCell::new(Some(Ok(value)))), future: ::std::rc::Rc::new(::std::cell::RefCell::new(None)) } }");
        writer.line("    /// Store a live future behind a cloneable erased promise handle.");
        writer.line("    fn from_future(future: SmeltPromiseFuture) -> Self { Self { id: smelt_next_object_id(), state: ::std::rc::Rc::new(::std::cell::RefCell::new(None)), future: ::std::rc::Rc::new(::std::cell::RefCell::new(Some(future))) } }");
        writer
            .line("    /// Await the stored future once and share its settled result with clones.");
        writer.line(
            "    async fn smelt_await(&self) -> Result<SmeltUnknown, Box<dyn std::error::Error>> {",
        );
        writer.line("        if self.state.borrow().is_none() {");
        // Bind the taken future to a local before awaiting so no `RefMut` from
        // `borrow_mut()` stays alive across the `.await` (pre-2024-edition
        // temporary-scope rules would otherwise keep it borrowed and panic on
        // re-entry through the shared cell).
        writer.line("            let taken = self.future.borrow_mut().take();");
        writer.line("            if let Some(future) = taken {");
        writer
            .line("                let settled = future.await.map_err(|error| error.to_string());");
        writer.line("                *self.state.borrow_mut() = Some(settled);");
        writer.line("            }");
        writer.line("        }");
        writer.line("        loop {");
        writer.line("            if let Some(result) = self.state.borrow().clone() {");
        writer.line("                return result.map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error).into());");
        writer.line("            }");
        // The result cell is still empty: another task (or a timer callback) must
        // settle it. When timer helpers exist, drive the cooperative scheduler and
        // virtual clock the same way the spin-wait futures do (H1/H3) so a future
        // handed to the detached promise-task queue, or one whose resolution
        // depends on a pending timer, cannot spin forever with nothing advancing
        // time. Without timer helpers there is no timer queue or detached-task
        // queue to drive, so a bare cooperative yield is the only (and correct)
        // driver, and the helper symbols are not emitted to reference.
        if needs_timer_helpers {
            writer.line(format!(
                "            {sleep_ms}(0.0).await;",
                sleep_ms = smelt_stdlib::runtime_symbols::timers::SLEEP_MS,
            ));
        }
        writer.line("            tokio::task::yield_now().await;");
        writer.line("        }");
        writer.line("    }");
        writer.line("}");
        writer.line("impl ::std::fmt::Debug for SmeltPromise { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_struct(\"SmeltPromise\").field(\"id\", &self.id).finish() } }");
        writer.blank_line();
        // JavaScript `await` flattens: awaiting a value that is itself a promise
        // resolves through the whole chain. An `async` function that returns a
        // `Promise` erases that inner promise to `SmeltUnknown::Promise`, so a
        // consumer that awaits the function's result must keep awaiting while the
        // settled value is still a promise. This helper drives the shared cell of
        // each erased promise (which in turn drives the cooperative scheduler),
        // and is a no-op pass-through for any non-promise value.
        writer.line("#[allow(dead_code)]");
        writer.line("async fn smelt_await_flatten(value: SmeltUnknown) -> Result<SmeltUnknown, Box<dyn std::error::Error>> {");
        writer.line("    let mut current = value;");
        writer.line("    while let SmeltUnknown::Promise(promise) = current {");
        writer.line("        current = promise.smelt_await().await?;");
        writer.line("    }");
        writer.line("    Ok(current)");
        writer.line("}");
        writer.blank_line();
        // Generic promise-value ABI. A source `Promise<T>` / `Type::Future(T)`
        // lowers to `SmeltFuture<T>` in *every* position (parameter, field,
        // return, local, async-op result), so the same MIR future type renders
        // one Rust type everywhere instead of a bare
        // `Pin<Box<dyn Future>>` in some positions and a value in others. The
        // handle is a shared `Rc<RefCell<..>>` so it is cheaply `Clone`
        // (matching JS promise-value copy semantics) and `Default` (a ready
        // default value). `smelt_await` drives the stored future once, caches the
        // resolved value, and serves later awaits from that cache — JS promises
        // may be awaited multiple times, and each await after the first resolves
        // from the cached value (single-consumer of the underlying future).
        writer.line("#[allow(dead_code)]");
        writer.line("enum SmeltFutureState<T> {");
        writer.line("    Pending(::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<T, Box<dyn std::error::Error>>>>>),");
        writer.line("    Resolved(T),");
        writer.line("    Taken,");
        writer.line("}");
        writer.line("pub struct SmeltFuture<T> {");
        writer.line("    state: ::std::rc::Rc<::std::cell::RefCell<SmeltFutureState<T>>>,");
        writer.line("}");
        writer.line("impl<T> Clone for SmeltFuture<T> { fn clone(&self) -> Self { Self { state: self.state.clone() } } }");
        writer.line("impl<T: Default> Default for SmeltFuture<T> { fn default() -> Self { Self::resolved(T::default()) } }");
        writer.line("impl<T> ::std::fmt::Debug for SmeltFuture<T> { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str(\"SmeltFuture\") } }");
        writer.line("#[allow(dead_code)]");
        writer.line("impl<T> SmeltFuture<T> {");
        writer.line("    /// Wrap a live future behind a cloneable promise-value handle.");
        writer.line("    fn from_future(future: ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<T, Box<dyn std::error::Error>>>>>) -> Self { Self { state: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltFutureState::Pending(future))) } }");
        writer.line("    /// Build an already-resolved promise-value handle.");
        writer.line("    fn resolved(value: T) -> Self { Self { state: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltFutureState::Resolved(value))) } }");
        writer.line("    /// Drive the stored future once, cache its resolved value, and serve");
        writer.line("    /// later awaits from that cache (single-consumer of the future).");
        writer.line("    async fn smelt_await(&self) -> Result<T, Box<dyn std::error::Error>> where T: Clone {");
        writer.line("        let pending = {");
        writer.line("            let mut guard = self.state.borrow_mut();");
        writer.line("            if matches!(&*guard, SmeltFutureState::Pending(_)) {");
        writer.line("                match ::std::mem::replace(&mut *guard, SmeltFutureState::Taken) {");
        writer.line("                    SmeltFutureState::Pending(future) => Some(future),");
        writer.line("                    _ => None,");
        writer.line("                }");
        writer.line("            } else { None }");
        writer.line("        };");
        writer.line("        if let Some(future) = pending {");
        writer.line("            let value = future.await?;");
        writer.line("            *self.state.borrow_mut() = SmeltFutureState::Resolved(value.clone());");
        writer.line("            return Ok(value);");
        writer.line("        }");
        writer.line("        let guard = self.state.borrow();");
        writer.line("        match &*guard {");
        writer.line("            SmeltFutureState::Resolved(value) => Ok(value.clone()),");
        writer.line("            _ => Err(std::io::Error::new(std::io::ErrorKind::Other, \"future already consumed\").into()),");
        writer.line("        }");
        writer.line("    }");
        writer.line("}");
        // `SmeltFuture<T>` is `IntoFuture`, so an ordinary Rust `.await` on a
        // promise value drives it through `smelt_await` — every generated `.await`
        // site works on the wrapper without a bespoke call form. The underlying
        // futures are `'static` (the prior `Pin<Box<dyn Future>>` ABI was too), so
        // `T: 'static` here matches existing constraints.
        writer.line("impl<T: Clone + 'static> ::std::future::IntoFuture for SmeltFuture<T> {");
        writer.line("    type Output = Result<T, Box<dyn std::error::Error>>;");
        writer.line("    type IntoFuture = ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<T, Box<dyn std::error::Error>>>>>;");
        writer.line("    fn into_future(self) -> Self::IntoFuture { Box::pin(async move { self.smelt_await().await }) }");
        writer.line("}");
        writer.blank_line();
        writer.line("/// Return an erased JavaScript `Array.prototype.sort` method bound to an erased array.");
        writer.line("fn smelt_array_sort_method(values: SmeltArray) -> SmeltUnknown { SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let mut sorted = values.clone().into_vec(); if let Some(SmeltUnknown::Function(compare)) = args.get(0).cloned() { sorted.sort_by(|left, right| { let result = compare(vec![left.clone(), right.clone()]).unwrap_or(SmeltUnknown::Number(0.0)); let ordering = match result { SmeltUnknown::Number(value) => value, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(0.0), SmeltUnknown::Bool(value) => if value { 1.0 } else { 0.0 }, _ => 0.0 }; if ordering < 0.0 { ::std::cmp::Ordering::Less } else if ordering > 0.0 { ::std::cmp::Ordering::Greater } else { ::std::cmp::Ordering::Equal } }); } else { sorted.sort_by(|left, right| left.to_string().cmp(&right.to_string())); } Ok(SmeltUnknown::Array(sorted.into())) })) }");
        writer.blank_line();
        // `AbortController`/`AbortSignal` cancellation model. Both erase to
        // marker-bearing `SmeltObject`s whose shared `Rc<RefCell<..>>` storage
        // makes `controller.abort()` observable through any binding that read
        // `controller.signal`. These helpers resolve the signal record behind a
        // controller or signal, flip the shared `aborted` flag, and fire (then
        // clear) registered `'abort'` listeners.
        writer.line("/// Resolve the AbortSignal record behind an abort controller or signal object.");
        writer.line("fn smelt_abort_signal_object(object: &SmeltObject) -> Option<SmeltObject> { if object.contains_key(\"__smelt_abortsignal\") { return Some(object.clone()); } match object.get(\"signal\") { Some(SmeltUnknown::Object(signal)) if signal.contains_key(\"__smelt_abortsignal\") => Some(signal), _ => None } }");
        writer.line("/// Mark an AbortSignal aborted and fire (then clear) its registered `'abort'` listeners.");
        writer.line("fn smelt_abort_signal_fire(signal: &SmeltObject) { if matches!(signal.get(\"aborted\"), Some(SmeltUnknown::Bool(true))) { return; } signal.insert(\"aborted\".to_owned(), SmeltUnknown::Bool(true)); let listeners = match signal.get(\"__smelt_abort_listeners\") { Some(SmeltUnknown::Array(values)) => values.clone().into_vec(), _ => Vec::new() }; signal.insert(\"__smelt_abort_listeners\".to_owned(), SmeltUnknown::Array(Vec::new().into())); for listener in listeners { if let SmeltUnknown::Function(callback) = listener { let event = SmeltObject::new(::std::collections::HashMap::from([(\"type\".to_owned(), SmeltUnknown::String(\"abort\".to_owned()))])); let _ = callback(vec![SmeltUnknown::Object(event)]); } } }");
        writer.line("/// Return an erased AbortController/AbortSignal method bound to its shared record.");
        writer.line("fn smelt_abort_method(object: SmeltObject, method: &str) -> SmeltUnknown { let method = method.to_owned(); SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let signal = smelt_abort_signal_object(&object); match method.as_str() { \"abort\" | \"dispatchEvent\" => { if let Some(signal) = signal { smelt_abort_signal_fire(&signal); } Ok(if method == \"dispatchEvent\" { SmeltUnknown::Bool(true) } else { SmeltUnknown::Undefined }) } \"addEventListener\" => { if let Some(signal) = signal { let event_type = match args.first() { Some(SmeltUnknown::String(value)) => value.clone(), _ => String::new() }; if event_type == \"abort\" { if let Some(listener @ SmeltUnknown::Function(_)) = args.get(1).cloned() { let mut listeners = match signal.get(\"__smelt_abort_listeners\") { Some(SmeltUnknown::Array(values)) => values.into_vec(), _ => Vec::new() }; listeners.push(listener); signal.insert(\"__smelt_abort_listeners\".to_owned(), SmeltUnknown::Array(listeners.into())); } } } Ok(SmeltUnknown::Undefined) } \"removeEventListener\" => { if let Some(signal) = signal { if let Some(target @ SmeltUnknown::Function(_)) = args.get(1).cloned() { let listeners: Vec<SmeltUnknown> = match signal.get(\"__smelt_abort_listeners\") { Some(SmeltUnknown::Array(values)) => values.into_vec(), _ => Vec::new() }.into_iter().filter(|listener| !listener.js_strict_eq(&target)).collect(); signal.insert(\"__smelt_abort_listeners\".to_owned(), SmeltUnknown::Array(listeners.into())); } } Ok(SmeltUnknown::Undefined) } _ => Ok(SmeltUnknown::Undefined) } })) }");
        writer.blank_line();
        writer.block("pub enum SmeltUnknown", |unknown_writer| {
            unknown_writer.line("Null,");
            unknown_writer.line("Undefined,");
            unknown_writer.line("Bool(bool),");
            unknown_writer.line("Number(f64),");
            unknown_writer.line("String(String),");
            unknown_writer.line("Symbol(String),");
            unknown_writer.line("Array(SmeltArray),");
            unknown_writer.line("Object(SmeltObject),");
            unknown_writer.line("Function(::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>),");
            unknown_writer.line("Promise(SmeltPromise),");
        });
        writer.blank_line();
        writer.block("impl Clone for SmeltUnknown", |impl_writer| {
            impl_writer.block("fn clone(&self) -> Self", |fn_writer| {
                fn_writer.block("match self", |match_writer| {
                    match_writer.line("Self::Null => Self::Null,");
                    match_writer.line("Self::Undefined => Self::Undefined,");
                    match_writer.line("Self::Bool(value) => Self::Bool(*value),");
                    match_writer.line("Self::Number(value) => Self::Number(*value),");
                    match_writer.line("Self::String(value) => Self::String(value.clone()),");
                    match_writer.line("Self::Symbol(value) => Self::Symbol(value.clone()),");
                    match_writer.line("Self::Array(values) => Self::Array(values.clone()),");
                    match_writer.line("Self::Object(values) => Self::Object(values.clone()),");
                    match_writer.line("Self::Function(value) => Self::Function(value.clone()),");
                    match_writer.line("Self::Promise(value) => Self::Promise(value.clone()),");
                });
            });
        });
        if needs_erased_function {
            writer.blank_line();
            writer.line("#[derive(Clone)]");
            writer.line("pub struct SmeltErasedFunction {");
            writer.line("    callback: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> SmeltUnknown>,");
            writer.line("    length: f64,");
            writer.line("    object: Option<SmeltObject>,");
            writer.line("}");
            writer.blank_line();
            writer.line("impl SmeltErasedFunction {");
            writer.line("    /// Invoke an erased JavaScript callable through a reentrant handle.");
            writer.line("    fn call(&self, args: impl Into<Vec<SmeltUnknown>>) -> SmeltUnknown {");
            writer.line("        (self.callback)(args.into())");
            writer.line("    }");
            writer.line(
                "    /// Restore an erased callable value without dropping callable-object fields.",
            );
            writer.line("    fn into_smelt_unknown(self) -> SmeltUnknown {");
            writer.line(
                "        // Erasing the SAME callback twice must yield `SmeltUnknown::Function`",
            );
            writer.line("        // values that share one OUTER `Rc`, because reference identity");
            writer.line(
                "        // (`Rc::ptr_eq` in `same_js_key`/`smelt_unknown_structural_eq`) compares",
            );
            writer.line(
                "        // that outer `Rc`. A nullary function-item constant (`doNothing()`)",
            );
            writer.line(
                "        // routes through one cached `SmeltErasedFunction`, so two calls share",
            );
            writer.line(
                "        // the inner callback `Rc`; key a `Weak` outer cache on its address so",
            );
            writer.line(
                "        // both erasures resolve to one `SmeltUnknown::Function` while both are",
            );
            writer.line(
                "        // alive (e.g. inside one `toStrictEqual`). A `Weak` avoids pinning the",
            );
            writer.line(
                "        // callback alive; a successful upgrade proves the address is still the",
            );
            writer.line("        // same callback, so it is a true hit. Callable objects");
            writer.line("        // (`object: Some(_)`) are per-instance and skip the cache.");
            writer.line("        if self.object.is_none() {");
            writer.line("            let key = ::std::rc::Rc::as_ptr(&self.callback) as *const () as usize;");
            writer.line("            if let Some(callable) = SMELT_ERASED_FUNCTION_VALUES.with(|cache| cache.borrow().get(&key).and_then(::std::rc::Weak::upgrade)) {");
            writer.line("                return SmeltUnknown::Function(callable);");
            writer.line("            }");
            writer.line("            let callback = self.callback.clone();");
            writer.line("            let callable: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = ::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>((callback)(args)));");
            writer.line("            SMELT_ERASED_FUNCTION_VALUES.with(|cache| { cache.borrow_mut().insert(key, ::std::rc::Rc::downgrade(&callable)); });");
            writer.line("            return SmeltUnknown::Function(callable);");
            writer.line("        }");
            writer.line("        let callback = self.callback.clone();");
            writer.line("        let callable = SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>((callback)(args))));");
            writer.line("        if let Some(object) = self.object { object.insert(\"__smelt_call\".to_owned(), callable); SmeltUnknown::Object(object) } else { callable }");
            writer.line("    }");
            writer.line("}");
            writer.blank_line();
            writer.line("thread_local! {");
            writer.line(
                "    /// Cache the OUTER `SmeltUnknown::Function` `Rc` derived from each erased",
            );
            writer
                .line("    /// callback, keyed on the inner callback `Rc` address, as a `Weak` so");
            writer.line(
                "    /// repeated erasures of one shared `SmeltErasedFunction` keep reference",
            );
            writer.line("    /// identity while alive without pinning transient callbacks.");
            writer.line("    static SMELT_ERASED_FUNCTION_VALUES: ::std::cell::RefCell<::std::collections::HashMap<usize, ::std::rc::Weak<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());");
            writer.line("}");
        }
        writer.blank_line();
        if needs_blob_record {
            writer.line("/// Build the modeled host `Blob`/`File` record for `new Blob(...)` / `new File(...)`.");
            writer.line("///");
            writer.line("/// Concatenates BlobPart contents (strings verbatim; nested Blob/File records");
            writer.line("/// contribute their stored `content`; other parts stringify like JavaScript)");
            writer.line("/// and stores the UTF-8 byte length as `size`. Passing a file name stamps the");
            writer.line("/// `__smelt_file` marker on top of `__smelt_blob`, so `file instanceof Blob`");
            writer.line("/// observes the host subtype relationship; `lastModified` defaults to `0.0`");
            writer.line("/// for determinism instead of the wall clock.");
            writer.line(format!(
                "fn {}(parts: SmeltUnknown, blob_type: String, file_name: Option<String>, last_modified: Option<f64>) -> SmeltUnknown {{",
                smelt_stdlib::runtime_symbols::host::BLOB_RECORD_FROM_PARTS,
            ));
            writer.line("    let mut content = String::new();");
            writer.line("    if let SmeltUnknown::Array(items) = parts {");
            writer.line("        for item in items.iter() {");
            writer.line("            match item {");
            writer.line("                SmeltUnknown::String(text) => content.push_str(text),");
            writer.line("                SmeltUnknown::Object(map) if map.contains_key(\"__smelt_blob\") => {");
            writer.line("                    if let Some(SmeltUnknown::String(text)) = map.get(\"content\") { content.push_str(&text); }");
            writer.line("                }");
            writer.line("                other => content.push_str(&other.to_string()),");
            writer.line("            }");
            writer.line("        }");
            writer.line("    }");
            writer.line("    let record = ::std::collections::HashMap::from([");
            writer.line("        (\"__smelt_blob\".to_owned(), SmeltUnknown::Bool(true)),");
            writer.line("        (\"type\".to_owned(), SmeltUnknown::String(blob_type)),");
            writer.line("        (\"size\".to_owned(), SmeltUnknown::Number(content.len() as f64)),");
            writer.line("        (\"content\".to_owned(), SmeltUnknown::String(content)),");
            writer.line("    ]);");
            writer.line("    let record = SmeltObject::new(record);");
            writer.line("    if let Some(name) = file_name {");
            writer.line("        record.insert(\"__smelt_file\".to_owned(), SmeltUnknown::Bool(true));");
            writer.line("        record.insert(\"name\".to_owned(), SmeltUnknown::String(name));");
            writer.line("        record.insert(\"lastModified\".to_owned(), SmeltUnknown::Number(last_modified.unwrap_or(0.0)));");
            writer.line("    }");
            writer.line("    SmeltUnknown::Object(record)");
            writer.line("}");
            writer.blank_line();
        }
        // Erased dynamic indexed assignment `target[key] = value`. Mirrors JS:
        // an object gets a string property; an array with a numeric index sets
        // (and extends) that element; any other value (or an array with a
        // non-index key) becomes a fresh object holding the single property.
        // Centralizing this keeps each call site a single call instead of an
        // inline `match` that repeats `SmeltUnknown` at every assignment.
        writer.line("fn smelt_index_assign(target: &mut SmeltUnknown, key: String, value: SmeltUnknown) {");
        writer.line("    match target {");
        writer.line("        SmeltUnknown::Object(map) => { map.insert(key, value); }");
        writer.line("        SmeltUnknown::Array(array) => {");
        writer.line("            if let Ok(index) = key.parse::<usize>() { array.set_index(index, value); }");
        writer.line("            else { let mut map = ::std::collections::HashMap::new(); map.insert(key, value); *target = SmeltUnknown::Object(SmeltObject::new(map)); }");
        writer.line("        }");
        writer.line("        other => { let mut map = ::std::collections::HashMap::new(); map.insert(key, value); *other = SmeltUnknown::Object(SmeltObject::new(map)); }");
        writer.line("    }");
        writer.line("}");
        writer.blank_line();
        writer.line("fn smelt_get_object_field(map: &SmeltObject, field: &str) -> SmeltUnknown {");
        writer.line("    // A missing property reads as JS `undefined`, distinct from an");
        writer.line("    // explicit `null` value (`obj.missing === undefined`, `!== null`).");
        writer.line("    match map.get(field).unwrap_or(SmeltUnknown::Undefined) {");
        writer.line("        SmeltUnknown::Object(getter) if getter.contains_key(\"__smelt_get\") => match getter.get(\"__smelt_get\") {");
        writer.line("            Some(SmeltUnknown::Function(smelt_getter)) => (smelt_getter)(Vec::new()).unwrap_or_else(|error| panic!(\"{}\", error)),");
        writer.line("            _ => SmeltUnknown::Null,");
        writer.line("        },");
        writer.line("        value => value,");
        writer.line("    }");
        writer.line("}");
        writer.blank_line();
        writer.line("fn smelt_unknown_is_nullish(value: &SmeltUnknown) -> bool { matches!(value, SmeltUnknown::Null | SmeltUnknown::Undefined) }");
        writer.line("fn smelt_unknown_is_undefined(value: &SmeltUnknown) -> bool { matches!(value, SmeltUnknown::Undefined) }");
        writer
            .line("fn smelt_missing_property_value() -> SmeltUnknown { SmeltUnknown::Undefined }");
        writer.line("fn smelt_missing_index_value() -> SmeltUnknown { SmeltUnknown::Undefined }");
        writer.blank_line();
        writer.line("fn smelt_unknown_structural_eq(left: &SmeltUnknown, right: &SmeltUnknown, seen: &mut ::std::collections::HashSet<(usize, usize)>) -> bool {");
        writer.line("    match (left, right) {");
        writer.line("        (SmeltUnknown::Null, SmeltUnknown::Null) => true,");
        writer.line("        (SmeltUnknown::Undefined, SmeltUnknown::Undefined) => true,");
        writer.line(
            "        (SmeltUnknown::Bool(left), SmeltUnknown::Bool(right)) => left == right,",
        );
        writer.line("        (SmeltUnknown::Number(left), SmeltUnknown::Number(right)) => left == right || (left.is_nan() && right.is_nan()),");
        writer.line(
            "        (SmeltUnknown::String(left), SmeltUnknown::String(right)) => left == right,",
        );
        writer.line(
            "        (SmeltUnknown::Symbol(left), SmeltUnknown::Symbol(right)) => left == right,",
        );
        writer.line("        (SmeltUnknown::Array(left), SmeltUnknown::Array(right)) => left.len() == right.len() && left.iter().zip(right.iter()).all(|(left, right)| smelt_unknown_structural_eq(left, right, seen)),");
        writer.line("        (SmeltUnknown::Object(left), SmeltUnknown::Object(right)) => smelt_object_structural_eq(left, right, seen),");
        writer.line("        (SmeltUnknown::Function(left), SmeltUnknown::Function(right)) => ::std::rc::Rc::ptr_eq(left, right),");
        writer.line("        (SmeltUnknown::Promise(left), SmeltUnknown::Promise(right)) => left.id == right.id,");
        writer.line("        _ => false,");
        writer.line("    }");
        writer.line("}");
        writer.blank_line();
        writer.line("fn smelt_object_structural_eq(left: &SmeltObject, right: &SmeltObject, seen: &mut ::std::collections::HashSet<(usize, usize)>) -> bool {");
        writer.line("    if left.contains_key(\"__smelt_date\") || right.contains_key(\"__smelt_date\") { let left_date = smelt_unknown_date_value(&SmeltUnknown::Object(left.clone())); let right_date = smelt_unknown_date_value(&SmeltUnknown::Object(right.clone())); return left_date == right_date || (left_date.is_nan() && right_date.is_nan()); }");
        writer.line("    if left.id == right.id { return true; }");
        writer.line("    let key = (left.id, right.id);");
        writer.line("    if !seen.insert(key) { return true; }");
        writer.line("    let left_entries = left.iter().collect::<Vec<_>>();");
        writer.line("    let right_values = right.values.borrow();");
        writer.line("    if left_entries.len() != right_values.len() { return false; }");
        writer.line("    left_entries.into_iter().all(|(key, left_value)| right_values.get(&key).is_some_and(|right_value| smelt_unknown_structural_eq(&left_value, right_value, seen)))");
        writer.line("}");
        writer.blank_line();
        writer.line("fn smelt_unknown_structural_hash<H: ::std::hash::Hasher>(value: &SmeltUnknown, state: &mut H, seen: &mut ::std::collections::HashSet<usize>) {");
        writer.line("    match value {");
        writer.line("        SmeltUnknown::Null => 0_u8.hash(state),");
        writer.line("        SmeltUnknown::Undefined => 8_u8.hash(state),");
        writer
            .line("        SmeltUnknown::Bool(value) => { 1_u8.hash(state); value.hash(state); }");
        writer.line("        SmeltUnknown::Number(value) => { 2_u8.hash(state); if value.is_nan() { f64::NAN.to_bits().hash(state); } else { value.to_bits().hash(state); } }");
        writer.line(
            "        SmeltUnknown::String(value) => { 3_u8.hash(state); value.hash(state); }",
        );
        writer.line(
            "        SmeltUnknown::Symbol(value) => { 4_u8.hash(state); value.hash(state); }",
        );
        writer.line("        SmeltUnknown::Array(values) => { 5_u8.hash(state); values.len().hash(state); for value in values.iter() { smelt_unknown_structural_hash(value, state, seen); } }");
        writer.line("        SmeltUnknown::Object(values) => { 6_u8.hash(state); smelt_object_structural_hash(values, state, seen); }");
        writer.line("        SmeltUnknown::Function(function) => { 7_u8.hash(state); ::std::rc::Rc::as_ptr(function).hash(state); }");
        writer.line("        SmeltUnknown::Promise(promise) => { 9_u8.hash(state); promise.id.hash(state); }");
        writer.line("    }");
        writer.line("}");
        writer.blank_line();
        writer.line("fn smelt_unknown_stable_hash_key(value: &SmeltUnknown) -> u64 {");
        writer.line("    let mut hasher = ::std::collections::hash_map::DefaultHasher::new();");
        writer.line("    let mut seen = ::std::collections::HashSet::new();");
        writer.line("    smelt_unknown_structural_hash(value, &mut hasher, &mut seen);");
        writer.line("    ::std::hash::Hasher::finish(&hasher)");
        writer.line("}");
        writer.blank_line();
        writer.line("fn smelt_object_structural_hash<H: ::std::hash::Hasher>(object: &SmeltObject, state: &mut H, seen: &mut ::std::collections::HashSet<usize>) {");
        writer.line("    if !seen.insert(object.id) { 255_u8.hash(state); return; }");
        writer.line("    let mut entries = object.iter().collect::<Vec<_>>();");
        writer.line("    entries.sort_by(|left, right| left.0.cmp(&right.0));");
        writer.line("    entries.len().hash(state);");
        writer.line("    for (key, value) in entries { key.hash(state); smelt_unknown_structural_hash(&value, state, seen); }");
        writer.line("}");
        writer.blank_line();
        writer.block("impl ::std::fmt::Debug for SmeltUnknown", |impl_writer| {
            impl_writer.block(
                "fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result",
                |fn_writer| {
                    fn_writer.block("match self", |match_writer| {
                        match_writer.line("Self::Null => formatter.write_str(\"Null\"),");
                        match_writer.line("Self::Undefined => formatter.write_str(\"Undefined\"),");
                        match_writer.line("Self::Bool(value) => formatter.debug_tuple(\"Bool\").field(value).finish(),");
                        match_writer.line("Self::Number(value) => formatter.debug_tuple(\"Number\").field(value).finish(),");
                        match_writer.line("Self::String(value) => formatter.debug_tuple(\"String\").field(value).finish(),");
                        match_writer.line("Self::Symbol(value) => formatter.debug_tuple(\"Symbol\").field(value).finish(),");
                        match_writer.line("Self::Array(values) => formatter.debug_tuple(\"Array\").field(values).finish(),");
                        match_writer.line("Self::Object(values) => formatter.debug_tuple(\"Object\").field(values).finish(),");
                        match_writer.line("Self::Function(_) => formatter.write_str(\"Function(<closure>)\"),");
                        match_writer.line("Self::Promise(value) => formatter.debug_tuple(\"Promise\").field(value).finish(),");
                    });
                },
            );
        });
        writer.blank_line();
        writer.block("impl PartialEq for SmeltUnknown", |impl_writer| {
            impl_writer.block("fn eq(&self, other: &Self) -> bool", |fn_writer| {
                fn_writer.line("let mut smelt_seen = ::std::collections::HashSet::new();");
                fn_writer.line("smelt_unknown_structural_eq(self, other, &mut smelt_seen)");
            });
        });
        writer.blank_line();
        writer.block("impl Default for SmeltUnknown", |impl_writer| {
            impl_writer.block("fn default() -> Self", |fn_writer| {
                fn_writer.line("Self::Null");
            });
        });
        writer.blank_line();
        if needs_timer_helpers {
            writer.line("struct SmeltTimer {");
            writer.line("    id: u64,");
            writer.line("    due_ms: u64,");
            writer.line("    callback: ::std::rc::Rc<::std::cell::RefCell<dyn FnMut() -> Result<(), Box<dyn std::error::Error>>>>,");
            writer.line("    // `Some(period)` marks a repeating `setInterval` timer that re-arms");
            writer.line("    // itself `period` ms after each fire; `None` is a one-shot `setTimeout`.");
            writer.line("    period_ms: Option<u64>,");
            writer.line("}");
            writer.blank_line();
            writer.line("thread_local! {");
            writer.line("    static SMELT_NEXT_TIMER_ID: ::std::cell::Cell<u64> = const { ::std::cell::Cell::new(1) };");
            writer.line("    static SMELT_TIMER_NOW_MS: ::std::cell::Cell<u64> = const { ::std::cell::Cell::new(0) };");
            writer.line("    static SMELT_TIMERS: ::std::cell::RefCell<Vec<SmeltTimer>> = const { ::std::cell::RefCell::new(Vec::new()) };");
            writer.line("    static SMELT_PROMISE_TASKS: ::std::cell::RefCell<Vec<::std::pin::Pin<Box<dyn ::std::future::Future<Output = ()>>>>> = const { ::std::cell::RefCell::new(Vec::new()) };");
            writer.line("}");
            writer.blank_line();
            writer.line(format!(
                "fn {reset_timers}() {{",
                reset_timers = smelt_stdlib::runtime_symbols::timers::RESET_TIMERS,
            ));
            writer.line("    SMELT_NEXT_TIMER_ID.with(|next| next.set(1));");
            writer.line("    SMELT_TIMER_NOW_MS.with(|now| now.set(0));");
            writer.line("    SMELT_TIMERS.with(|timers| timers.borrow_mut().clear());");
            writer.line("    SMELT_PROMISE_TASKS.with(|tasks| tasks.borrow_mut().clear());");
            writer.line("}");
            writer.blank_line();
            writer.line(format!(
                "fn {noop_waker}() -> ::std::task::Waker {{",
                noop_waker = smelt_stdlib::runtime_symbols::timers::NOOP_WAKER,
            ));
            writer.line(
                "    unsafe fn clone(_: *const ()) -> ::std::task::RawWaker { smelt_raw_waker() }",
            );
            writer.line("    unsafe fn wake(_: *const ()) {}");
            writer.line("    unsafe fn wake_by_ref(_: *const ()) {}");
            writer.line("    unsafe fn drop(_: *const ()) {}");
            writer.line("    fn smelt_raw_waker() -> ::std::task::RawWaker { ::std::task::RawWaker::new(::std::ptr::null(), &::std::task::RawWakerVTable::new(clone, wake, wake_by_ref, drop)) }");
            writer.line("    unsafe { ::std::task::Waker::from_raw(smelt_raw_waker()) }");
            writer.line("}");
            writer.blank_line();
            writer.line(format!(
                "fn {spawn_promise_task}(task: ::std::pin::Pin<Box<dyn ::std::future::Future<Output = ()>>>) {{",
                spawn_promise_task = smelt_stdlib::runtime_symbols::timers::SPAWN_PROMISE_TASK,
            ));
            writer.line("    SMELT_PROMISE_TASKS.with(|tasks| tasks.borrow_mut().push(task));");
            writer.line("}");
            writer.blank_line();
            writer.line(format!(
                "async fn {drain_promise_tasks}() {{",
                drain_promise_tasks = smelt_stdlib::runtime_symbols::timers::DRAIN_PROMISE_TASKS,
            ));
            writer.line("    for _ in 0..64 {");
            writer.line("        let mut tasks = SMELT_PROMISE_TASKS.with(|tasks| ::std::mem::take(&mut *tasks.borrow_mut()));");
            writer.line("        if tasks.is_empty() { break; }");
            writer.line(format!(
                "        let waker = {noop_waker}();",
                noop_waker = smelt_stdlib::runtime_symbols::timers::NOOP_WAKER,
            ));
            writer.line("        let mut cx = ::std::task::Context::from_waker(&waker);");
            writer.line("        let mut pending = Vec::new();");
            writer.line("        for mut task in tasks.drain(..) {");
            writer.line(
                "            if task.as_mut().poll(&mut cx).is_pending() { pending.push(task); }",
            );
            writer.line("        }");
            writer.line("        let had_pending = !pending.is_empty();");
            writer.line(
                "        SMELT_PROMISE_TASKS.with(|tasks| tasks.borrow_mut().extend(pending));",
            );
            writer.line("        if !had_pending { break; }");
            writer.line("        tokio::task::yield_now().await;");
            writer.line("    }");
            writer.line("}");
            writer.blank_line();
            writer.line(format!(
                "fn {set_timeout}(callback: ::std::rc::Rc<::std::cell::RefCell<dyn FnMut() -> Result<(), Box<dyn std::error::Error>>>>, delay_ms: f64) -> SmeltUnknown {{",
                set_timeout = smelt_stdlib::runtime_symbols::timers::SET_TIMEOUT,
            ));
            writer.line("    let id = SMELT_NEXT_TIMER_ID.with(|next| { let id = next.get(); next.set(id.saturating_add(1)); id });");
            writer.line("    let delay_ms = if delay_ms.is_finite() && delay_ms > 0.0 { delay_ms as u64 } else { 0 };");
            writer.line("    let due_ms = SMELT_TIMER_NOW_MS.with(|now| now.get().saturating_add(delay_ms));");
            writer.line("    SMELT_TIMERS.with(|timers| timers.borrow_mut().push(SmeltTimer { id, due_ms, callback, period_ms: None }));");
            writer.line("    SmeltUnknown::Number(id as f64)");
            writer.line("}");
            writer.blank_line();
            writer.line(format!(
                "fn {set_interval}(callback: ::std::rc::Rc<::std::cell::RefCell<dyn FnMut() -> Result<(), Box<dyn std::error::Error>>>>, period_ms: f64) -> SmeltUnknown {{",
                set_interval = smelt_stdlib::runtime_symbols::timers::SET_INTERVAL,
            ));
            writer.line("    let id = SMELT_NEXT_TIMER_ID.with(|next| { let id = next.get(); next.set(id.saturating_add(1)); id });");
            writer.line("    // Clamp non-positive periods to 1 ms so an interval still advances virtual");
            writer.line("    // time and cannot busy-loop the drain at the current instant.");
            writer.line("    let period_ms = if period_ms.is_finite() && period_ms > 0.0 { period_ms as u64 } else { 1 };");
            writer.line("    let due_ms = SMELT_TIMER_NOW_MS.with(|now| now.get().saturating_add(period_ms));");
            writer.line("    SMELT_TIMERS.with(|timers| timers.borrow_mut().push(SmeltTimer { id, due_ms, callback, period_ms: Some(period_ms) }));");
            writer.line("    SmeltUnknown::Number(id as f64)");
            writer.line("}");
            writer.blank_line();
            writer.line(format!(
                "fn {clear_timeout}<T: IntoSmeltUnknown>(handle: T) {{",
                clear_timeout = smelt_stdlib::runtime_symbols::timers::CLEAR_TIMEOUT,
            ));
            writer.line(
                "    let SmeltUnknown::Number(id) = handle.into_smelt_unknown() else { return; };",
            );
            writer.line("    let id = id as u64;");
            writer.line(
            "    SMELT_TIMERS.with(|timers| timers.borrow_mut().retain(|timer| timer.id != id));",
        );
            writer.line("}");
            writer.blank_line();
            // `clearInterval` cancels by id exactly like `clearTimeout`; both share
            // the single timer queue. Emit it as a thin alias so the generated
            // `async_clear_interval` call site resolves to a real helper.
            writer.line(format!(
                "fn {clear_interval}<T: IntoSmeltUnknown>(handle: T) {{ {clear_timeout}(handle) }}",
                clear_interval = smelt_stdlib::runtime_symbols::timers::CLEAR_INTERVAL,
                clear_timeout = smelt_stdlib::runtime_symbols::timers::CLEAR_TIMEOUT,
            ));
            writer.blank_line();
            writer.line(format!(
                "fn {drain_due_timers}() {{",
                drain_due_timers = smelt_stdlib::runtime_symbols::timers::DRAIN_DUE_TIMERS,
            ));
            writer.line("    loop {");
            writer.line("        let now = SMELT_TIMER_NOW_MS.with(|now| now.get());");
            writer.line("        let due = SMELT_TIMERS.with(|timers| {");
            writer.line("            let mut timers = timers.borrow_mut();");
            writer.line("            let mut due = Vec::new();");
            writer.line("            let mut pending = Vec::new();");
            writer.line("            for timer in timers.drain(..) {");
            writer.line("                if timer.due_ms <= now { due.push(timer); } else { pending.push(timer); }");
            writer.line("            }");
            writer.line("            *timers = pending;");
            writer.line("            due");
            writer.line("        });");
            writer.line("        if due.is_empty() { break; }");
            writer.line("        for timer in due {");
            writer.line("            (&mut *timer.callback.borrow_mut())().unwrap_or_else(|error| panic!(\"{}\", error));");
            writer.line("            // Re-arm repeating `setInterval` timers for their next period. The");
            writer.line("            // next fire is scheduled `period` ms from the current virtual time, so");
            writer.line("            // it is strictly in the future and cannot re-fire within this drain pass.");
            writer.line("            if let Some(period_ms) = timer.period_ms {");
            writer.line("                let next_due = now.saturating_add(period_ms);");
            writer.line("                SMELT_TIMERS.with(|timers| timers.borrow_mut().push(SmeltTimer { id: timer.id, due_ms: next_due, callback: timer.callback.clone(), period_ms: Some(period_ms) }));");
            writer.line("            }");
            writer.line("        }");
            writer.line("    }");
            writer.line("}");
            writer.blank_line();
            writer.line(format!(
                "async fn {sleep_ms}(delay_ms: f64) {{",
                sleep_ms = smelt_stdlib::runtime_symbols::timers::SLEEP_MS,
            ));
            writer.line(format!(
                "    {drain_promise_tasks}().await;",
                drain_promise_tasks = smelt_stdlib::runtime_symbols::timers::DRAIN_PROMISE_TASKS,
            ));
            writer.line("    let delay_ms = if delay_ms.is_finite() && delay_ms > 0.0 { delay_ms as u64 } else { 0 };");
            writer.line(
                "    let target_ms = SMELT_TIMER_NOW_MS.with(|now| now.get().saturating_add(delay_ms));",
            );
            // Fire every timer due within the requested window, advancing virtual
            // time to each in turn and draining the microtask queue between fires.
            writer.line("    loop {");
            writer.line("        let next_due = SMELT_TIMERS.with(|timers| timers.borrow().iter().filter(|timer| timer.due_ms <= target_ms).map(|timer| timer.due_ms).min());");
            writer.line("        let Some(next_due) = next_due else { break; };");
            writer.line("        SMELT_TIMER_NOW_MS.with(|now| if next_due > now.get() { now.set(next_due); });");
            writer.line(format!(
                "        {drain_due_timers}();",
                drain_due_timers = smelt_stdlib::runtime_symbols::timers::DRAIN_DUE_TIMERS,
            ));
            writer.line(format!(
                "        {drain_promise_tasks}().await;",
                drain_promise_tasks = smelt_stdlib::runtime_symbols::timers::DRAIN_PROMISE_TASKS,
            ));
            writer.line("    }");
            writer.line("    SMELT_TIMER_NOW_MS.with(|now| if target_ms > now.get() { now.set(target_ms); });");
            // Node-style run-until-idle. A zero-delay sleep is what the generated
            // promise-executor spin loops await while waiting for a result cell to
            // be settled (e.g. by a `setTimeout(resolve, 100)` timer). With no
            // window to fire into, virtual time would never reach that timer and
            // the spin loop would starve forever. When the requested window has no
            // due timer AND no detached promise task is still runnable (the
            // microtask queue is drained, so nothing else can make progress), the
            // event loop is idle: advance virtual time to the earliest pending
            // timer and fire it, exactly as Node advances to its timer phase once
            // microtasks settle. Repeat until either the queue has runnable work
            // again, a timer settles the awaited state, or no timers remain.
            //
            // This applies only to a zero-delay sleep. A positive-delay sleep has
            // a real deadline: it must fire exactly the timers due within its
            // window (handled above) and must not jump the clock to a later timer,
            // or a bounded `await delay(35)` would over-fire a `setInterval`.
            writer.line("    'idle: {");
            writer.line("        if delay_ms != 0 { break 'idle; }");
            writer.line("        let tasks_pending = SMELT_PROMISE_TASKS.with(|tasks| !tasks.borrow().is_empty());");
            writer.line("        if tasks_pending { break 'idle; }");
            writer.line("        let earliest = SMELT_TIMERS.with(|timers| timers.borrow().iter().map(|timer| timer.due_ms).min());");
            writer.line("        let Some(earliest) = earliest else { break 'idle; };");
            writer.line("        SMELT_TIMER_NOW_MS.with(|now| if earliest > now.get() { now.set(earliest); });");
            writer.line(format!(
                "        {drain_due_timers}();",
                drain_due_timers = smelt_stdlib::runtime_symbols::timers::DRAIN_DUE_TIMERS,
            ));
            writer.line(format!(
                "        {drain_promise_tasks}().await;",
                drain_promise_tasks = smelt_stdlib::runtime_symbols::timers::DRAIN_PROMISE_TASKS,
            ));
            writer.line("    }");
            writer.line(format!(
                "    {drain_promise_tasks}().await;",
                drain_promise_tasks = smelt_stdlib::runtime_symbols::timers::DRAIN_PROMISE_TASKS,
            ));
            writer.line("    tokio::task::yield_now().await;");
            writer.line("}");
            writer.blank_line();
        }
        writer.block("impl SmeltUnknown", |impl_writer| {
            impl_writer.line("/// Returns the JavaScript-style length for unknown string, array, and object values.");
            impl_writer.block("pub fn len(&self) -> usize", |fn_writer| {
                fn_writer.block("match self", |match_writer| {
                    match_writer.line("Self::String(value) => value.chars().count(),");
                    match_writer.line("Self::Array(value) => value.len(),");
                    match_writer.line("Self::Object(value) => value.len(),");
                    match_writer.line("Self::Null | Self::Undefined | Self::Bool(_) | Self::Number(_) | Self::Symbol(_) | Self::Function(_) | Self::Promise(_) => 0,");
                });
            });
            impl_writer.line("/// Return a JavaScript-like weekday for erased Date-compatible numeric timestamps.");
            impl_writer.block("pub fn get_day(&self) -> f64", |fn_writer| {
                fn_writer.line("let timestamp_ms = match self { Self::Number(value) => *value, Self::Object(value) => match value.get(\"__smelt_date\") { Some(Self::Number(value)) => value, _ => return f64::NAN }, _ => return f64::NAN };");
                fn_writer.line("let days_since_epoch = (timestamp_ms / 86_400_000.0).floor() as i64;");
                fn_writer.line("((days_since_epoch + 4).rem_euclid(7)) as f64");
            });
            impl_writer.line("/// Return JavaScript Date.toISOString output for erased Date-compatible values.");
            impl_writer.block("pub fn to_iso_string(&self) -> String", |fn_writer| {
                fn_writer.line("if let Self::Object(value) = self {");
                fn_writer.line("    if let (Some(Self::Number(timestamp_ms)), Some(Self::String(timezone_name))) = (value.get(\"__smelt_date\"), value.get(\"__smelt_timezone\")) {");
                fn_writer.line("        if let (Some(mut local), Ok(timezone)) = (chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp_ms as i64).map(|date| date.naive_utc()), timezone_name.parse::<chrono_tz::Tz>()) {");
                fn_writer.line("            loop {");
                fn_writer.line("                match chrono::TimeZone::from_local_datetime(&timezone, &local) {");
                fn_writer.line("                    chrono::LocalResult::Single(date) => return date.to_rfc3339_opts(chrono::SecondsFormat::Millis, false),");
                fn_writer.line("                    chrono::LocalResult::Ambiguous(first, _) => return first.to_rfc3339_opts(chrono::SecondsFormat::Millis, false),");
                fn_writer.line("                    chrono::LocalResult::None => local += chrono::Duration::minutes(1),");
                fn_writer.line("                }");
                fn_writer.line("            }");
                fn_writer.line("        }");
                fn_writer.line("    }");
                fn_writer.line("}");
                fn_writer.line("let timestamp_ms = match self {");
                fn_writer.line("    Self::Number(value) => *value,");
                fn_writer.line("    Self::Object(value) => match value.get(\"__smelt_date\") { Some(Self::Number(value)) => value, _ => f64::NAN },");
                fn_writer.line("    Self::String(value) => value.parse::<f64>().unwrap_or(f64::NAN),");
                fn_writer.line("    Self::Bool(value) => if *value { 1.0 } else { 0.0 },");
                fn_writer.line("    Self::Null | Self::Undefined | Self::Symbol(_) | Self::Array(_) | Self::Function(_) | Self::Promise(_) => f64::NAN,");
                fn_writer.line("};");
                fn_writer.line("chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp_ms as i64).map(|date| date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)).unwrap_or_else(|| \"Invalid Date\".to_owned())");
            });
            impl_writer.line("/// Return JavaScript-like `includes` membership for erased strings and arrays.");
            impl_writer.block(
                "pub fn includes<T: IntoSmeltUnknown>(&self, needle: T) -> bool",
                |fn_writer| {
                    fn_writer.line("let needle = needle.into_smelt_unknown();");
                    fn_writer.block("match (self, needle)", |match_writer| {
                        match_writer.line("(Self::String(haystack), Self::String(needle)) => haystack.contains(&needle),");
                        match_writer.line("(Self::Array(values), needle) => values.iter().any(|value| value == &needle),");
                        match_writer.line("_ => false,");
                    });
                },
            );
            impl_writer.line("/// Return JavaScript-like RegExp.test behavior for erased regex-like values.");
            impl_writer.block(
                "pub fn test<T: IntoSmeltUnknown>(&self, haystack: T) -> bool",
                |fn_writer| {
                    fn_writer.line("let SmeltUnknown::String(haystack) = haystack.into_smelt_unknown() else { return false; };");
                    fn_writer.block("match self", |match_writer| {
                        match_writer.line("Self::String(pattern) => regex::Regex::new(pattern).is_ok_and(|regex| regex.is_match(&haystack)),");
                        match_writer.line("Self::Object(map) => {");
                        match_writer.line("    let Some(Self::String(source)) = map.get(\"source\") else { return false; };");
                        match_writer.line("    let flags = match map.get(\"flags\") { Some(Self::String(flags)) => flags.clone(), _ => String::new() };");
                        match_writer.line("    SmeltRegExp::new(source.clone(), flags).test(&haystack)");
                        match_writer.line("}");
                        match_writer.line("_ => false,");
                    });
                },
            );
            impl_writer.line("/// No-op compatibility hook for erased callable objects with a `flush` method.");
            impl_writer.block("pub fn flush(&self)", |_fn_writer| {});
        });
        writer.blank_line();
        writer.block("impl ::std::fmt::Display for SmeltUnknown", |impl_writer| {
            impl_writer.block(
                "fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result",
                |fn_writer| {
                    fn_writer.block("match self", |match_writer| {
                        match_writer.line("Self::Null => formatter.write_str(\"null\"),");
                        match_writer.line("Self::Undefined => formatter.write_str(\"undefined\"),");
                        match_writer.line("Self::Bool(value) => write!(formatter, \"{value}\"),");
                        match_writer.line("Self::Number(value) => write!(formatter, \"{value}\"),");
                        match_writer.line("Self::String(value) => formatter.write_str(value),");
                        match_writer.line("Self::Symbol(value) => formatter.write_str(value),");
                        match_writer.line("Self::Array(_) | Self::Object(_) => formatter.write_str(\"[object Object]\"),");
                        match_writer.line("Self::Function(_) => formatter.write_str(\"function () { [native code] }\"),");
                        match_writer.line("Self::Promise(_) => formatter.write_str(\"[object Promise]\"),");
                    });
                },
            );
        });
        writer.blank_line();
        writer.line("impl Eq for SmeltUnknown {}");
        writer.blank_line();
        writer.block("impl ::std::hash::Hash for SmeltUnknown", |impl_writer| {
            impl_writer.block(
                "fn hash<H: ::std::hash::Hasher>(&self, state: &mut H)",
                |fn_writer| {
                    fn_writer.line("let mut smelt_seen = ::std::collections::HashSet::new();");
                    fn_writer.line("smelt_unknown_structural_hash(self, state, &mut smelt_seen);");
                },
            );
        });
        writer.blank_line();
        writer.block("impl PartialOrd for SmeltUnknown", |impl_writer| {
            impl_writer.block(
                "fn partial_cmp(&self, other: &Self) -> Option<::std::cmp::Ordering>",
                |fn_writer| {
                    fn_writer.block("match (self, other)", |match_writer| {
                        match_writer.line("(Self::Number(left), Self::Number(right)) => left.partial_cmp(right),");
                        match_writer.line("(Self::String(left), Self::String(right)) => Some(left.cmp(right)),");
                        match_writer.line("(Self::Bool(left), Self::Bool(right)) => Some(left.cmp(right)),");
                        match_writer.line("(Self::Object(left), Self::Object(right)) if left.contains_key(\"__smelt_date\") || right.contains_key(\"__smelt_date\") => smelt_unknown_date_value(self).partial_cmp(&smelt_unknown_date_value(other)),");
                        match_writer.line("(Self::Null, Self::Null) => Some(::std::cmp::Ordering::Equal),");
                        match_writer.line("(left, right) => Some(smelt_unknown_rank(left).cmp(&smelt_unknown_rank(right))),");
                    });
                },
            );
        });
        writer.blank_line();
        writer.block("impl PartialEq<f64> for SmeltUnknown", |impl_writer| {
            impl_writer.block("fn eq(&self, other: &f64) -> bool", |fn_writer| {
                fn_writer.line("matches!(self, Self::Number(value) if value == other)");
            });
        });
        writer.blank_line();
        writer.block("impl PartialEq<SmeltUnknown> for f64", |impl_writer| {
            impl_writer.block("fn eq(&self, other: &SmeltUnknown) -> bool", |fn_writer| {
                fn_writer.line("other == self");
            });
        });
        writer.blank_line();
        writer.block("impl PartialOrd<f64> for SmeltUnknown", |impl_writer| {
            impl_writer.block(
                "fn partial_cmp(&self, other: &f64) -> Option<::std::cmp::Ordering>",
                |fn_writer| {
                    fn_writer.block("match self", |match_writer| {
                        match_writer.line("Self::Number(value) => value.partial_cmp(other),");
                        match_writer.line("Self::String(value) => value.parse::<f64>().ok().and_then(|number| number.partial_cmp(other)),");
                        match_writer.line("Self::Bool(value) => (if *value { 1.0 } else { 0.0 }).partial_cmp(other),");
                        match_writer.line("Self::Null | Self::Undefined | Self::Symbol(_) | Self::Array(_) | Self::Object(_) | Self::Function(_) | Self::Promise(_) => None,");
                    });
                },
            );
        });
        writer.blank_line();
        writer.block("impl PartialOrd<SmeltUnknown> for f64", |impl_writer| {
            impl_writer.block(
                "fn partial_cmp(&self, other: &SmeltUnknown) -> Option<::std::cmp::Ordering>",
                |fn_writer| {
                    fn_writer.line("other.partial_cmp(self).map(::std::cmp::Ordering::reverse)");
                },
            );
        });
        writer.blank_line();
        writer.block(
            "impl PartialEq<SmeltUnknown> for Option<SmeltUnknown>",
            |impl_writer| {
                impl_writer.block("fn eq(&self, other: &SmeltUnknown) -> bool", |fn_writer| {
                    fn_writer.line("self.as_ref().is_some_and(|value| value == other)");
                });
            },
        );
        writer.blank_line();
        writer.block(
            "impl PartialOrd<SmeltUnknown> for Option<SmeltUnknown>",
            |impl_writer| {
                impl_writer.block(
                    "fn partial_cmp(&self, other: &SmeltUnknown) -> Option<::std::cmp::Ordering>",
                    |fn_writer| {
                        fn_writer.line("self.as_ref().and_then(|value| value.partial_cmp(other))");
                    },
                );
            },
        );
        writer.blank_line();
        writer.block(
            "fn smelt_unknown_date_value(value: &SmeltUnknown) -> f64",
            |fn_writer| {
                fn_writer.block("match value", |match_writer| {
                    match_writer.line("SmeltUnknown::Object(object) => match object.get(\"__smelt_date\") { Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN },");
                    match_writer.line("_ => f64::NAN,");
                });
            },
        );
        writer.blank_line();
        writer.block(
            "fn smelt_unknown_rank(value: &SmeltUnknown) -> u8",
            |fn_writer| {
                fn_writer.block("match value", |match_writer| {
                    match_writer.line("SmeltUnknown::Null => 0,");
                    match_writer.line("SmeltUnknown::Undefined => 8,");
                    match_writer.line("SmeltUnknown::Bool(_) => 1,");
                    match_writer.line("SmeltUnknown::Number(_) => 2,");
                    match_writer.line("SmeltUnknown::String(_) => 3,");
                    match_writer.line("SmeltUnknown::Symbol(_) => 4,");
                    match_writer.line("SmeltUnknown::Array(_) => 5,");
                    match_writer.line("SmeltUnknown::Object(_) => 6,");
                    match_writer.line("SmeltUnknown::Function(_) => 7,");
                    match_writer.line("SmeltUnknown::Promise(_) => 9,");
                });
            },
        );
        writer.blank_line();
        // JavaScript's Abstract Relational Comparison (`<`, `<=`, `>`, `>=`) for
        // two erased values: when *both* operands are strings it compares them
        // lexically (byte order over the UTF-8 encoding, matching JS's UTF-16
        // code-unit order for the BMP), otherwise it runs `ToNumber` on both and
        // compares as `f64`. A `NaN` on either side yields `None` (an unordered
        // result), so every relational operator reports `false` — exactly the JS
        // outcome. Codegen routes the both-erased relational arms here so a
        // runtime string comparison stays lexical instead of collapsing to
        // `NaN`-vs-`NaN` under blind numeric coercion.
        writer.block(
            "fn smelt_unknown_js_relational_ordering(left: &SmeltUnknown, right: &SmeltUnknown) -> Option<::std::cmp::Ordering>",
            |fn_writer| {
                fn_writer.block("match (left, right)", |match_writer| {
                    match_writer.line("(SmeltUnknown::String(left), SmeltUnknown::String(right)) => Some(left.cmp(right)),");
                    match_writer.line("_ => smelt_unknown_to_number(left).partial_cmp(&smelt_unknown_to_number(right)),");
                });
            },
        );
        writer.blank_line();
        // `ToNumber` for an erased value, mirroring the inline coercion codegen
        // emits when a `SmeltUnknown` flows into a numeric context: numeric
        // strings parse to their value, non-numeric strings become `NaN`, booleans
        // map to `0`/`1`, `__smelt_date` objects surface their timestamp, and
        // every remaining shape is `NaN`.
        writer.block(
            "fn smelt_unknown_to_number(value: &SmeltUnknown) -> f64",
            |fn_writer| {
                fn_writer.block("match value", |match_writer| {
                    match_writer.line("SmeltUnknown::Number(value) => *value,");
                    match_writer.line("SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") { Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN },");
                    match_writer.line("SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN),");
                    match_writer.line("SmeltUnknown::Bool(value) => if *value { 1.0 } else { 0.0 },");
                    match_writer.line("SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN,");
                });
            },
        );
        writer.blank_line();
        writer.block("pub trait IntoSmeltUnknown", |trait_writer| {
            trait_writer.line("fn into_smelt_unknown(self) -> SmeltUnknown;");
        });
        writer.blank_line();
        writer.block("impl IntoSmeltUnknown for SmeltUnknown", |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                fn_writer.line("self");
            });
        });
        writer.blank_line();
        writer.block("impl IntoSmeltUnknown for bool", |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                fn_writer.line("SmeltUnknown::Bool(self)");
            });
        });
        writer.blank_line();
        writer.block("impl IntoSmeltUnknown for f64", |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                fn_writer.line("SmeltUnknown::Number(self)");
            });
        });
        writer.blank_line();
        writer.block("impl IntoSmeltUnknown for i64", |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                fn_writer.line("SmeltUnknown::Number(self as f64)");
            });
        });
        writer.blank_line();
        writer.block("impl IntoSmeltUnknown for String", |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                fn_writer.line("SmeltUnknown::String(self)");
            });
        });
        writer.blank_line();
        writer.block("impl IntoSmeltUnknown for ()", |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                fn_writer.line("SmeltUnknown::Null");
            });
        });
        writer.blank_line();
        // Erasing a typed list yields an identity-bearing `SmeltUnknown::Array`,
        // carrying the list's reference id and erasing each element in turn.
        writer.line("impl<T: IntoSmeltUnknown> IntoSmeltUnknown for SmeltList<T> { fn into_smelt_unknown(self) -> SmeltUnknown { SmeltUnknown::Array(SmeltArray::with_id(self.id, self.values.into_iter().map(IntoSmeltUnknown::into_smelt_unknown).collect())) } }");
        writer.blank_line();
        writer.block("pub trait SmeltFromUnknown", |trait_writer| {
            trait_writer.line("fn smelt_from_unknown(value: SmeltUnknown) -> Self;");
        });
        writer.blank_line();
        writer.block("impl SmeltFromUnknown for SmeltUnknown", |impl_writer| {
            impl_writer.block(
                "fn smelt_from_unknown(value: SmeltUnknown) -> Self",
                |fn_writer| {
                    fn_writer.line("value");
                },
            );
        });
        writer.blank_line();
        writer.block("impl SmeltFromUnknown for bool", |impl_writer| {
            impl_writer.block(
                "fn smelt_from_unknown(value: SmeltUnknown) -> Self",
                |fn_writer| {
                    fn_writer.line("match value { SmeltUnknown::Null | SmeltUnknown::Undefined => false, SmeltUnknown::Bool(value) => value, SmeltUnknown::Number(value) => value != 0.0 && !value.is_nan(), SmeltUnknown::String(value) => !value.is_empty(), SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => true }");
                },
            );
        });
        writer.blank_line();
        writer.block("impl SmeltFromUnknown for f64", |impl_writer| {
            impl_writer.block(
                "fn smelt_from_unknown(value: SmeltUnknown) -> Self",
                |fn_writer| {
                    fn_writer.line("match value { SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") { Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN), SmeltUnknown::Bool(value) => if value { 1.0 } else { 0.0 }, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }");
                },
            );
        });
        writer.blank_line();
        writer.block("impl SmeltFromUnknown for i64", |impl_writer| {
            impl_writer.block(
                "fn smelt_from_unknown(value: SmeltUnknown) -> Self",
                |fn_writer| {
                    fn_writer.line("match value { SmeltUnknown::Number(value) => value as i64, SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") { Some(SmeltUnknown::Number(value)) => value as i64, _ => 0_i64 }, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN) as i64, SmeltUnknown::Bool(value) => if value { 1_i64 } else { 0_i64 }, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => 0_i64 }");
                },
            );
        });
        writer.blank_line();
        writer.block("impl SmeltFromUnknown for String", |impl_writer| {
            impl_writer.block(
                "fn smelt_from_unknown(value: SmeltUnknown) -> Self",
                |fn_writer| {
                    fn_writer.line("match value { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Undefined => \"undefined\".to_owned(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () { [native code] }\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned() }");
                },
            );
        });
        writer.blank_line();
        // Un-erasing a `SmeltUnknown` back into statically typed collections is the
        // mirror of the `IntoSmeltUnknown` impls above: a returned generic value
        // (e.g. `identity<T>(x): T` inferred at `SmeltRecord<String, String>`) is
        // erased to `SmeltUnknown` inside the generic body and must be converted
        // back element-wise at the typed binding site. Each impl reuses the source
        // container's reference id so round-tripping preserves JS identity, and
        // converts each element/entry through the element type's own
        // `SmeltFromUnknown`. A non-matching `SmeltUnknown` variant yields an empty
        // container (the JS-coercion fallback, matching the scalar impls above).
        writer.line("impl<T: SmeltFromUnknown> SmeltFromUnknown for SmeltList<T> { fn smelt_from_unknown(value: SmeltUnknown) -> Self { match value { SmeltUnknown::Array(array) => SmeltList::with_id(array.id, array.values.into_iter().map(T::smelt_from_unknown).collect()), _ => SmeltList::new(Vec::new()) } } }");
        writer.blank_line();
        writer.line("impl<K: SmeltFromUnknown + Eq + ::std::hash::Hash + Clone, V: SmeltFromUnknown + Clone> SmeltFromUnknown for SmeltRecord<K, V> { fn smelt_from_unknown(value: SmeltUnknown) -> Self { match value { SmeltUnknown::Object(object) => SmeltRecord::with_id_from_entries(object.id, object.iter().map(|(key, value)| (K::smelt_from_unknown(SmeltUnknown::String(key)), V::smelt_from_unknown(value)))), _ => SmeltRecord::with_id_from_entries(smelt_next_object_id(), ::std::iter::empty()) } } }");
        writer.blank_line();
        writer.line("impl<K: SmeltFromUnknown + SmeltJsKeyEq + Clone, V: SmeltFromUnknown + Clone> SmeltFromUnknown for SmeltJsMap<K, V> { fn smelt_from_unknown(value: SmeltUnknown) -> Self { match value { SmeltUnknown::Object(object) => object.iter().map(|(key, value)| (K::smelt_from_unknown(SmeltUnknown::String(key)), V::smelt_from_unknown(value))).collect(), _ => SmeltJsMap::default() } } }");
        writer.blank_line();
        writer.block("trait SmeltIntoF64", |trait_writer| {
            trait_writer.line("fn smelt_into_f64(self) -> f64;");
        });
        writer.blank_line();
        writer.block("impl SmeltIntoF64 for f64", |impl_writer| {
            impl_writer.block("fn smelt_into_f64(self) -> f64", |fn_writer| {
                fn_writer.line("self");
            });
        });
        writer.blank_line();
        writer.block("impl SmeltIntoF64 for SmeltUnknown", |impl_writer| {
            impl_writer.block("fn smelt_into_f64(self) -> f64", |fn_writer| {
                fn_writer.block("match self", |match_writer| {
                    match_writer.line("SmeltUnknown::Number(value) => value,");
                    match_writer.line("_ => 0.0,");
                });
            });
        });
        writer.blank_line();
        writer.block(
            "impl<T: IntoSmeltUnknown> IntoSmeltUnknown for Option<T>",
            |impl_writer| {
                impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                    fn_writer.line(
                        // An absent optional erases to JS `undefined` (missing
                        // element / optional / `?.`), distinct from explicit `null`.
                        "self.map_or(SmeltUnknown::Undefined, IntoSmeltUnknown::into_smelt_unknown)",
                    );
                });
            },
        );
        writer.blank_line();
        writer.block("impl<T: IntoSmeltUnknown> IntoSmeltUnknown for Vec<T>", |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                fn_writer.line("SmeltUnknown::Array(self.into_iter().map(IntoSmeltUnknown::into_smelt_unknown).collect())");
            });
        });
        writer.blank_line();
        writer.block(
            "impl<T: IntoSmeltUnknown + Eq + ::std::hash::Hash> IntoSmeltUnknown for ::std::collections::HashSet<T>",
            |impl_writer| {
                impl_writer.line("/// Erase JavaScript Set values as iterable array-like values.");
                impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                    fn_writer.line("let mut values = self.into_iter().map(IntoSmeltUnknown::into_smelt_unknown).collect::<Vec<_>>();");
                    fn_writer.line("values.sort_by_key(smelt_unknown_stable_hash_key);");
                    fn_writer.line("SmeltUnknown::Array(values.into())");
                });
            },
        );
        writer.blank_line();
        writer.block("trait SmeltArrayExt<T>", |trait_writer| {
            trait_writer
                .line("/// Return the first JavaScript-style index of a value, or -1 when absent.");
            trait_writer.line("fn index_of(&self, needle: T) -> SmeltUnknown where T: PartialEq;");
        });
        writer.blank_line();
        writer.block("impl<T> SmeltArrayExt<T> for Vec<T>", |impl_writer| {
            impl_writer.line("/// Implement `Array.prototype.indexOf` for generated Rust vectors.");
            impl_writer.block("fn index_of(&self, needle: T) -> SmeltUnknown where T: PartialEq", |fn_writer| {
                fn_writer.line("self.iter().position(|value| value == &needle).map_or(SmeltUnknown::Number(-1.0), |index| SmeltUnknown::Number(index as f64))");
            });
        });
        writer.blank_line();
        writer.block("trait SmeltUnknownArrayExt", |trait_writer| {
            trait_writer
                .line("/// Concatenate erased array values with JavaScript-style spreading.");
            trait_writer.line("fn concat(self, other: Vec<SmeltUnknown>) -> Vec<SmeltUnknown>;");
        });
        writer.blank_line();
        writer.block(
            "impl SmeltUnknownArrayExt for Vec<SmeltUnknown>",
            |impl_writer| {
                impl_writer.line(
                    "/// Implement the generated rest-argument concat helper for erased arrays.",
                );
                impl_writer.block(
                    "fn concat(mut self, other: Vec<SmeltUnknown>) -> Vec<SmeltUnknown>",
                    |fn_writer| {
                        fn_writer.line("self.extend(other);");
                        fn_writer.line("self");
                    },
                );
            },
        );
        writer.blank_line();
        writer.block("trait SmeltUnitExt", |trait_writer| {
            trait_writer.line(
                "/// Return false for generated calls against unreachable unit placeholders.",
            );
            trait_writer.line("fn includes<T>(&self, _needle: T) -> bool;");
        });
        writer.blank_line();
        writer.block("impl SmeltUnitExt for ()", |impl_writer| {
            impl_writer.line("/// Keep unreachable JavaScript membership calls type-checkable.");
            impl_writer.block("fn includes<T>(&self, _needle: T) -> bool", |fn_writer| {
                fn_writer.line("false");
            });
        });
        writer.blank_line();
        writer.block(
            "impl<A: IntoSmeltUnknown, B: IntoSmeltUnknown> IntoSmeltUnknown for (A, B)",
            |impl_writer| {
                impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                    fn_writer.line(
                        "SmeltUnknown::Array(vec![self.0.into_smelt_unknown(), self.1.into_smelt_unknown()].into())",
                    );
                });
            },
        );
        writer.blank_line();
        writer.block(
            "impl<K, T> IntoSmeltUnknown for ::std::collections::HashMap<K, T> where K: IntoSmeltUnknown + Eq + ::std::hash::Hash, T: IntoSmeltUnknown",
            |impl_writer| {
                impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                    fn_writer.line("SmeltUnknown::Object(SmeltObject::new(self.into_iter().map(|(key, value)| { let key = match key.into_smelt_unknown() { SmeltUnknown::String(value) => value, SmeltUnknown::Symbol(value) => format!(\"__smelt_symbol:{value}\"), SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => \"null\".to_owned(), SmeltUnknown::Undefined => \"undefined\".to_owned(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () { [native code] }\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned() }; (key, value.into_smelt_unknown()) }).collect()))");
                });
            },
        );
        writer.blank_line();
        writer.block(
            "impl<K, T> IntoSmeltUnknown for SmeltRecord<K, T> where K: IntoSmeltUnknown + Eq + ::std::hash::Hash + Clone, T: IntoSmeltUnknown + Clone",
            |impl_writer| {
                impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                    fn_writer.line("SmeltUnknown::Object(SmeltObject::with_id(self.id, self.iter().into_iter().map(|(key, value)| { let key = match key.into_smelt_unknown() { SmeltUnknown::String(value) => value, SmeltUnknown::Symbol(value) => format!(\"__smelt_symbol:{value}\"), SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => \"null\".to_owned(), SmeltUnknown::Undefined => \"undefined\".to_owned(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () { [native code] }\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned() }; (key, value.into_smelt_unknown()) }).collect()))");
                });
            },
        );
        writer.blank_line();
        if needs_serde_json {
            emit_unknown_serde_impls(&mut writer);
        }
    }

    let mut emitted_class_names = HashSet::new();
    for interface in &mir.interfaces {
        let name = RustIdent::new(
            mir.symbols
                .get(interface.name)
                .ok_or_else(|| EmitError::new("interface has unknown symbol"))?,
        )
        .into_string();
        if !emitted_class_names.insert(name.clone()) {
            continue;
        }
        let type_params = interface_type_params_text(mir, interface)?;
        let impl_generics = interface_impl_generics_text(mir, interface)?;
        let fields = effective_interface_fields(mir, interface);
        let has_function_field = fields
            .iter()
            .any(|field| type_contains_function(mir, field.ty));
        if has_function_field {
            writer.line("#[derive(Clone)]");
            writer.line("#[allow(dead_code)]");
        } else if needs_serde_json && interface_is_json_serializable(mir, interface) {
            writer.line("#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]");
        } else {
            writer.line("#[derive(Clone, Debug, Default)]");
        }
        let phantom_args = interface
            .type_params
            .iter()
            .map(|param| {
                mir.symbols
                    .get(param.name)
                    .map(|param_name| RustIdent::new(param_name).into_string())
                    .ok_or_else(|| EmitError::new("interface type parameter has unknown symbol"))
            })
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let scoped_type_params = interface
            .type_params
            .iter()
            .map(|param| param.name)
            .collect::<HashSet<_>>();
        writer.block(format!("struct {name}{type_params}"), |block_writer| {
            for field in &fields {
                let field_name = RustIdent::new(
                    mir.symbols
                        .get(field.name)
                        .ok_or_else(|| EmitError::new("field has unknown symbol"))
                        .unwrap_or("field"),
                )
                .into_string();
                let field_ty = FunctionEmitter::type_text_for_with_scoped_type_params(
                    mir,
                    &context,
                    field.ty,
                    &scoped_type_params,
                )
                .unwrap_or_else(|_| "SmeltUnknown".to_owned());
                block_writer.line(format!("{field_name}: {field_ty},"));
            }
            if !interface.type_params.is_empty() {
                block_writer.line(format!(
                    "_smelt_phantom: ::std::marker::PhantomData<({phantom_args})>,"
                ));
            }
        });
        if has_function_field {
            emit_default_impl_for_storage_type(
                &mut writer,
                mir,
                &context,
                &name,
                &impl_generics,
                &type_params,
                &fields,
                &phantom_args,
                &scoped_type_params,
            )?;
            emit_debug_impl_for_storage_type(&mut writer, &name, &impl_generics, &type_params);
        }
        if needs_unknown {
            emit_record_into_smelt_unknown_impl(
                &mut writer,
                mir,
                &name,
                &impl_generics,
                &type_params,
                &fields,
            )?;
        }
        writer.blank_line();
    }
    if needs_regex {
        writer.line("#[derive(Clone, Debug)]");
        writer.block("pub struct SmeltRegExp", |struct_writer| {
            struct_writer.line("id: usize,");
            struct_writer.line("source: String,");
            struct_writer.line("flags: String,");
            struct_writer.line("last_index: ::std::rc::Rc<::std::cell::RefCell<usize>>,");
        });
        writer.blank_line();
        writer.line("impl PartialEq for SmeltRegExp { fn eq(&self, other: &Self) -> bool { self.source == other.source && self.flags == other.flags && *self.last_index.borrow() == *other.last_index.borrow() } }");
        writer.blank_line();
        writer.block("impl SmeltRegExp", |impl_writer| {
            impl_writer.line("/// Construct a JavaScript-like RegExp value with shared lastIndex state.");
            impl_writer.block("pub fn new(source: String, flags: String) -> Self", |fn_writer| {
                fn_writer.line("Self { id: smelt_next_object_id(), source, flags, last_index: ::std::rc::Rc::new(::std::cell::RefCell::new(0)) }");
            });
            impl_writer.line("/// Return true when this RegExp has a flag.");
            impl_writer.block("pub fn has_flag(&self, flag: char) -> bool", |fn_writer| {
                fn_writer.line("self.flags.chars().any(|value| value == flag)");
            });
            impl_writer.line("/// Compile the Rust regex equivalent for this JavaScript RegExp.");
            impl_writer.block("fn compiled(&self) -> fancy_regex::Regex", |fn_writer| {
                fn_writer.line("self.try_compiled().expect(\"regex compile failed\")");
            });
            impl_writer.line("/// Try to compile the Rust regex equivalent for this JavaScript RegExp.");
            impl_writer.block("fn try_compiled(&self) -> Option<fancy_regex::Regex>", |fn_writer| {
                fn_writer.line("let mut prefix = String::new();");
                fn_writer.line("if self.has_flag('i') { prefix.push('i'); }");
                fn_writer.line("if self.has_flag('m') { prefix.push('m'); }");
                fn_writer.line("if self.has_flag('s') { prefix.push('s'); }");
                fn_writer.line("let translated_source = self.source.replace(\"[^]\", \"(?s:.)\");");
                fn_writer.line("let pattern = if prefix.is_empty() { translated_source } else { format!(\"(?{prefix}){translated_source}\") };");
                fn_writer.line("fancy_regex::Regex::new(&pattern).ok()");
            });
            impl_writer.line("/// Match a string with JavaScript String.prototype.match semantics.");
            impl_writer.block("pub fn match_string(&self, haystack: &str) -> Option<Vec<String>>", |fn_writer| {
                fn_writer.line("let regex = self.try_compiled()?;");
                fn_writer.block("if self.has_flag('g')", |if_writer| {
                    if_writer.line("let matches = regex.find_iter(haystack).filter_map(Result::ok).map(|value| value.as_str().to_owned()).collect::<Vec<_>>();");
                    if_writer.line("if matches.is_empty() { None } else { Some(matches) }");
                });
                fn_writer.block("else", |else_writer| {
                    else_writer.line("let captures = regex.captures(haystack).ok().flatten()?;");
                    else_writer.line("Some((0..captures.len()).map(|index| captures.get(index).map_or(String::new(), |value| value.as_str().to_owned())).collect::<Vec<_>>())");
                });
            });
            impl_writer.line("/// Split a string with JavaScript RegExp separator semantics.");
            impl_writer.block("pub fn split_string(&self, haystack: &str) -> Vec<String>", |fn_writer| {
                fn_writer.line("let Some(regex) = self.try_compiled() else { return vec![haystack.to_owned()]; };");
                fn_writer.line("regex.split(haystack).filter_map(Result::ok).map(str::to_owned).collect::<Vec<_>>()");
            });
            impl_writer.line("/// Replace matches with JavaScript RegExp-aware String.prototype.replace semantics.");
            impl_writer.block("pub fn replace_string(&self, haystack: &str, replacement: &str, force_all: bool) -> String", |fn_writer| {
                fn_writer.line("let Some(regex) = self.try_compiled() else { return haystack.to_owned(); };");
                fn_writer.line("let replace_all = force_all || self.has_flag('g');");
                fn_writer.block("if replace_all", |if_writer| {
                    if_writer.line("let mut output = String::new();");
                    if_writer.line("let mut last_end = 0usize;");
                    if_writer.block("for matched in regex.find_iter(haystack).filter_map(Result::ok)", |for_writer| {
                        for_writer.line("output.push_str(&haystack[last_end..matched.start()]);");
                        for_writer.line("output.push_str(replacement);");
                        for_writer.line("last_end = matched.end();");
                    });
                    if_writer.line("output.push_str(&haystack[last_end..]);");
                    if_writer.line("output");
                });
                fn_writer.line("else if let Ok(Some(matched)) = regex.find(haystack) {");
                fn_writer.line("    format!(\"{}{}{}\", &haystack[..matched.start()], replacement, &haystack[matched.end()..])");
                fn_writer.line("} else {");
                fn_writer.line("    haystack.to_owned()");
                fn_writer.line("}");
            });
            impl_writer.line("/// Execute this RegExp and return a concrete `SmeltMatch` result.");
            impl_writer.line("///");
            impl_writer.line("/// The match result is a typed `SmeltMatch` value (numbered groups,");
            impl_writer.line("/// named groups, `index`, `input`) instead of an erased `SmeltUnknown`");
            impl_writer.line("/// property bag; callers erase it explicitly at a dynamic boundary via");
            impl_writer.line("/// `SmeltMatch::into_smelt_unknown` when required.");
            impl_writer.block("pub fn exec(&self, haystack: &str) -> Option<SmeltMatch>", |fn_writer| {
                fn_writer.line("let regex = self.compiled();");
                fn_writer.line("let start = if self.has_flag('g') || self.has_flag('y') { *self.last_index.borrow() } else { 0 };");
                fn_writer.line("let suffix = haystack.get(start..).unwrap_or(\"\");");
                fn_writer.line("let captures = regex.captures(suffix).ok().flatten()?;");
                fn_writer.line("let matched = captures.get(0)?;");
                fn_writer.line("if self.has_flag('y') && matched.start() != 0 { *self.last_index.borrow_mut() = 0; return None; }");
                fn_writer.line("if self.has_flag('g') || self.has_flag('y') { *self.last_index.borrow_mut() = start + matched.end(); }");
                fn_writer.line("Some(SmeltMatch::from_captures(&regex, &captures, start + matched.start(), haystack))");
            });
            impl_writer.line("/// Return concrete `SmeltMatch` results for String.prototype.matchAll.");
            impl_writer.block("pub fn match_all_indices(&self, haystack: &str) -> Vec<SmeltMatch>", |fn_writer| {
                fn_writer.line("let Some(regex) = self.try_compiled() else { return Vec::new(); };");
                fn_writer.block("regex.captures_iter(haystack).filter_map(Result::ok).filter_map(|captures|", |map_writer| {
                    map_writer.line("let matched = captures.get(0)?;");
                    map_writer.line("Some(SmeltMatch::from_captures(&regex, &captures, matched.start(), haystack))");
                });
                fn_writer.line(").collect::<Vec<_>>()");
            });
            impl_writer.line("/// Test this RegExp against a string with JavaScript lastIndex updates.");
            impl_writer.block("pub fn test(&self, haystack: &str) -> bool", |fn_writer| {
                fn_writer.line("self.exec(haystack).is_some()");
            });
        });
        writer.blank_line();
        // The `IntoSmeltUnknown` erasure adapters reference the `SmeltUnknown`
        // carrier, which is only emitted when the crate genuinely crosses a
        // dynamic boundary. A regex/match program that keeps every value
        // statically typed does not emit `SmeltUnknown`, so the adapters are
        // gated on that need rather than on regex presence alone.
        if needs_unknown {
            writer.block("impl IntoSmeltUnknown for SmeltRegExp", |impl_writer| {
                impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                    fn_writer.line("SmeltUnknown::Object(SmeltObject::with_id(self.id, ::std::collections::HashMap::from([");
                    fn_writer.line("(\"source\".to_owned(), SmeltUnknown::String(self.source)),");
                    fn_writer.line("(\"flags\".to_owned(), SmeltUnknown::String(self.flags)),");
                    fn_writer.line("(\"__smelt_regexp\".to_owned(), SmeltUnknown::Bool(true)),");
                    fn_writer.line("])))");
                });
            });
            writer.blank_line();
        }
        writer.block("impl Default for SmeltRegExp", |impl_writer| {
            impl_writer.line("/// Construct a RegExp that matches the empty string.");
            impl_writer.block("fn default() -> Self", |fn_writer| {
                fn_writer.line("Self::new(String::new(), String::new())");
            });
        });
        writer.blank_line();
        emit_smelt_match(&mut writer, needs_unknown);
    }
    for class in &mir.classes {
        let name = class_name_text(mir, class)?;
        if !emitted_class_names.insert(name.clone()) {
            continue;
        }
        if context.is_reference_class(class.name) {
            emit_reference_class_storage(&mut writer, mir, &context, class, needs_unknown)?;
            continue;
        }
        let type_params = class_type_params_text(mir, class)?;
        let impl_generics = class_impl_generics_text(mir, class)?;
        let _inherited_trait_methods = inherited_trait_methods(mir, class);
        let mut field_lines = Vec::new();
        let fields = effective_class_fields(mir, class);
        let scoped_type_params = class
            .type_params
            .iter()
            .map(|param| param.name)
            .collect::<HashSet<_>>();
        let has_function_field = fields
            .iter()
            .any(|field| type_contains_function(mir, field.ty));
        // A value class supports structural equality (JS `==`/`===`/`toBe` on the
        // by-value representation, and derived comparisons in generated specs)
        // only when every stored field is itself `PartialEq`. Function fields
        // (`dyn Fn`) and promise fields (`SmeltPromise`, a `Clone`-only shared
        // future handle) are the two prelude shapes without `PartialEq`, so the
        // derive is gated on their absence.
        let supports_partial_eq = fields.iter().all(|field| {
            type_supports_partial_eq(mir, &context, field.ty, &mut Vec::new())
        });
        let partial_eq_derive = if supports_partial_eq { ", PartialEq" } else { "" };
        if has_function_field {
            writer.line("#[derive(Clone)]");
            writer.line("#[allow(dead_code)]");
        } else if needs_serde_json && class_is_json_serializable(mir, class) {
            writer.line(format!(
                "#[derive(Clone, Debug, Default{partial_eq_derive}, serde::Serialize, serde::Deserialize)]"
            ));
        } else {
            writer.line(format!("#[derive(Clone, Debug, Default{partial_eq_derive})]"));
        }
        for field in &fields {
            field_lines.push(format!(
                "{}: {},",
                RustIdent::new(
                    mir.symbols
                        .get(field.name)
                        .ok_or_else(|| EmitError::new("field has unknown symbol"))?
                ),
                FunctionEmitter::type_text_for_with_scoped_type_params(
                    mir,
                    &context,
                    field.ty,
                    &scoped_type_params,
                )?
            ));
        }
        if !class.type_params.is_empty() {
            let phantom_args = class
                .type_params
                .iter()
                .map(|param| {
                    mir.symbols
                        .get(param.name)
                        .map(|param_name| RustIdent::new(param_name).into_string())
                        .ok_or_else(|| EmitError::new("class type parameter has unknown symbol"))
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            field_lines.push(format!(
                "_smelt_phantom: ::std::marker::PhantomData<({phantom_args})>,"
            ));
        }
        writer.block(format!("struct {name}{type_params}"), |block_writer| {
            for field_line in field_lines {
                block_writer.line(field_line);
            }
        });
        if has_function_field {
            let phantom_args = class
                .type_params
                .iter()
                .map(|param| {
                    mir.symbols
                        .get(param.name)
                        .map(|param_name| RustIdent::new(param_name).into_string())
                        .ok_or_else(|| EmitError::new("class type parameter has unknown symbol"))
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            emit_default_impl_for_storage_type(
                &mut writer,
                mir,
                &context,
                &name,
                &impl_generics,
                &type_params,
                &fields,
                &phantom_args,
                &scoped_type_params,
            )?;
            emit_debug_impl_for_storage_type(&mut writer, &name, &impl_generics, &type_params);
        }
        if !class.static_fields.is_empty() {
            writer.block(
                format!("impl{impl_generics} {name}{type_params}"),
                |impl_writer| {
                    for field in &class.static_fields {
                        let field_name = mir
                            .symbols
                            .get(field.name)
                            .map(RustIdent::new)
                            .map_or_else(|| "field".to_owned(), RustIdent::into_string);
                        let field_ty = FunctionEmitter::type_text_for_with_scoped_type_params(
                            mir,
                            &context,
                            field.ty,
                            &scoped_type_params,
                        )
                        .unwrap_or_else(|_| "SmeltUnknown".to_owned());
                        let value = materialized_static_value_text(field.value.as_ref());
                        impl_writer.block(
                            format!("fn __smelt_static_{field_name}() -> {field_ty}"),
                            |function_writer| function_writer.line(value),
                        );
                    }
                },
            );
        }
        if needs_unknown {
            emit_record_into_smelt_unknown_impl(
                &mut writer,
                mir,
                &name,
                &impl_generics,
                &type_params,
                &fields,
            )?;
        }
        writer.blank_line();
    }

    let union_definitions = emitter::emit_union_definitions(mir, &context)?;
    if !union_definitions.is_empty() {
        for line in union_definitions.lines() {
            writer.line(line);
        }
    }

    let mut out = writer.finish();

    // Stable sentinel marking the end of the fixed runtime prelude and the start
    // of program-specific code. Tooling (`smelt smelt-unknown-report`) keys on
    // this exact comment to separate shared runtime scaffolding — the
    // `SmeltUnknown` enum, its impls, and the `smelt_*` helpers above — from the
    // generated program below, so `SmeltUnknown` occurrences in the prelude are
    // never mistaken for program-level erasure. Keep the text byte-for-byte
    // stable; changing it silently reclassifies every prelude occurrence.
    //
    // Normalize the trailing whitespace first so the marker always sits after
    // exactly one blank line, regardless of how the last prelude block ended.
    while out.ends_with('\n') {
        out.pop();
    }
    out.push_str(&format!("\n\n{PRELUDE_END_MARKER}\n"));

    // Emit one thread-local cell per module-level mutable global. These are
    // program-specific mangled names (not fixed runtime symbols): each `#[test]`
    // thread observes fresh module state initialized from the source literal,
    // deterministic and mirroring vitest's per-file module isolation. Copy
    // primitives use a `const`-initialized `Cell`; strings use the non-const
    // `RefCell` form because a `.to_owned()` initializer cannot be `const`.
    if !mir.globals.is_empty() {
        out.push('\n');
        out.push_str(&emit_mutable_globals(mir)?);
    }

    let mut has_emitted_root_function = false;
    for function in &mir.functions {
        if matches!(
            function.origin,
            HirOrigin::ClassConstructor { .. }
                | HirOrigin::ClassMethod { .. }
                | HirOrigin::ClassStaticMethod { .. }
        ) {
            continue;
        }
        let mut emitter = FunctionEmitter::new(mir, &context, function)?;
        let mut emitted_function_source = String::new();
        emitter.emit(&mut emitted_function_source)?;
        let routed_function_source =
            route_free_function(function, &context, emitted_function_source)?;
        let Some(root_function_source) = routed_function_source else {
            continue;
        };
        if has_emitted_root_function || !mir.classes.is_empty() {
            out.push('\n');
        }
        out.push_str(&root_function_source);
        has_emitted_root_function = true;
    }

    // Flush the per-function-item value accessors collected while erasing
    // function-item-as-value wrappers to `SmeltUnknown`. Each accessor lazily
    // builds and caches ONE erased `SmeltUnknown::Function` so that every
    // reference to the same named function value shares one inner `Rc`, keeping
    // JavaScript reference identity (`===`) stable across references. Sorted by
    // item key via the `BTreeMap` for deterministic golden output.
    for (key, body) in context.function_item_accessors.borrow().iter() {
        out.push_str(&format!(
            "\nfn __smelt_fn_value_{key}() -> SmeltUnknown {{\n    thread_local! {{ static SMELT_FN_VALUE: ::std::cell::OnceCell<SmeltUnknown> = ::std::cell::OnceCell::new(); }}\n    SMELT_FN_VALUE.with(|cell| cell.get_or_init(|| {body}).clone())\n}}\n"
        ));
    }

    // Flush the per-function-item accessors for the CONCRETE `SmeltErasedFunction`
    // type. Each caches ONE `SmeltErasedFunction` so repeated calls of a nullary
    // function-item constant (`doNothing()`/`constant()`) return clones sharing
    // one inner callback `Rc`, keeping JavaScript function-singleton identity.
    // A non-empty collector means a closure was lowered to `SmeltErasedFunction`,
    // so `needs_erased_function` is necessarily true and the struct is emitted.
    for (key, body) in context.function_item_erased_fn_accessors.borrow().iter() {
        out.push_str(&format!(
            "\nfn __smelt_fn_erased_{key}() -> SmeltErasedFunction {{\n    thread_local! {{ static SMELT_FN_ERASED: ::std::cell::OnceCell<SmeltErasedFunction> = ::std::cell::OnceCell::new(); }}\n    SMELT_FN_ERASED.with(|cell| cell.get_or_init(|| {body}).clone())\n}}\n"
        ));
    }

    let mut emitted_impl_names = HashSet::new();
    for class in &mir.classes {
        let name = class_name_text(mir, class)?;
        if !emitted_impl_names.insert(name.clone()) {
            continue;
        }
        let impl_generics = class_impl_generics_text(mir, class)?;
        let type_args = class_type_args_text(mir, class)?;
        out.push_str(&format!("\nimpl{impl_generics} {name}{type_args} {{\n"));
        if !class.is_abstract
            && let Some(constructor) = class.constructor
            && let Some(function) = mir.functions.get(id_index(
                constructor.0,
                "constructor index does not fit usize",
            )?)
        {
            let mut emitter = FunctionEmitter::new(mir, &context, function)?;
            emitter.emit_method(&mut out)?;
        }
        for method in &class.methods {
            if let Some(function) = mir
                .functions
                .get(id_index(method.0, "method index does not fit usize")?)
            {
                let mut emitter = FunctionEmitter::new(mir, &context, function)?;
                emitter.emit_method(&mut out)?;
            }
        }
        for static_method in &class.static_methods {
            if let Some(function) = mir.functions.get(id_index(
                static_method.0,
                "static method index does not fit usize",
            )?) {
                let mut emitter = FunctionEmitter::new(mir, &context, function)?;
                emitter.emit_method(&mut out)?;
            }
        }
        out.push_str("}\n");
    }

    if !has_main_function(mir)? {
        out.push_str("\nfn main() {}\n");
    }

    Ok(out)
}

/// Rust sources split by original source module.
struct MappedSources {
    /// Crate root source.
    root: String,
    /// Generated sibling module sources.
    modules: Vec<MappedModuleSource>,
}

/// One generated Rust module file.
struct MappedModuleSource {
    /// Source-shaped Rust file stem.
    name: String,
    /// Module source text.
    source: String,
}

/// Generated module metadata for a HIR body.
#[derive(Clone)]
struct BodyModuleInfo {
    /// Source-shaped Rust file stem.
    name: String,
    /// Original source path that owns the body.
    source_path: String,
}

/// Emits source text split across Rust module files.
fn emit_mapped_sources(
    mir: &Mir,
    krate: &smelt_hir::Crate,
    modules: &[(String, smelt_hir::ModuleId)],
) -> Result<MappedSources, EmitError> {
    let body_modules = body_module_names(krate, modules);
    let mut module_chunks = HashMap::<String, Vec<String>>::new();
    let mut module_paths = HashMap::<String, String>::new();

    let mut root =
        emit_source_with_free_function_router(mir, |function, context, function_source| {
            let HirOrigin::Body(body) = function.origin else {
                return Ok(Some(function_source));
            };
            if is_root_main_function(mir, function, context.none_ty) {
                return Ok(Some(function_source));
            }
            let Some(module_info) = body_modules.get(&body).cloned() else {
                return Ok(Some(function_source));
            };
            let module_name = module_info.name;
            module_paths
                .entry(module_name.clone())
                .or_insert(module_info.source_path);
            module_chunks
                .entry(module_name)
                .or_default()
                .push(publicize_free_function(function_source));
            Ok(None)
        })?;

    let mut module_names = module_chunks.keys().cloned().collect::<Vec<_>>();
    module_names.sort();
    let declarations = module_names
        .iter()
        .map(|name| {
            let module_ident = generated_source_module_ident(name);
            format!(
                "#[path = \"{name}.rs\"]\nmod {module_ident};\npub(crate) use {module_ident}::*;"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    if !declarations.is_empty() {
        root = insert_after_crate_header(root, &format!("{declarations}\n\n\n"));
    }

    let mapped_modules = module_names
        .into_iter()
        .map(|name| {
            let chunks = module_chunks.remove(&name).unwrap_or_default();
            let source_path = module_paths.remove(&name).unwrap_or_else(|| name.clone());
            MappedModuleSource {
                name,
                source: format!(
                    "// @generated by smelt. Do not edit by hand.\n// source: {source_path}\n#![allow(dead_code, non_snake_case, unused_imports, unused_variables)]\n\nuse super::*;\n\n{}",
                    chunks.join("\n")
                ),
            }
        })
        .collect();

    Ok(MappedSources {
        root,
        modules: mapped_modules,
    })
}

/// Builds a body-to-Rust-module map from HIR module ownership metadata.
fn body_module_names(
    krate: &smelt_hir::Crate,
    modules: &[(String, smelt_hir::ModuleId)],
) -> HashMap<BodyId, BodyModuleInfo> {
    let mut names = HashMap::new();
    for (path, module_id) in modules {
        let Some(module) = usize::try_from(module_id.0)
            .ok()
            .and_then(|index| krate.modules.get(index))
        else {
            continue;
        };
        let rust_module = source_module_name(&module.name);
        let module_info = BodyModuleInfo {
            name: rust_module.clone(),
            source_path: path.clone(),
        };
        if let Some(body) = module.body {
            names.insert(body, module_info.clone());
        }
        for item in &module.items {
            if let Some(smelt_hir::Item::Function(function)) = usize::try_from(item.0)
                .ok()
                .and_then(|index| krate.items.get(index))
                && let Some(body) = function.body
            {
                names.insert(body, module_info.clone());
            }
        }
    }
    names
}

/// Returns the source-shaped Rust file stem for one lowered source module.
fn source_module_name(name: &str) -> String {
    let sanitized = sanitize_ident(name);
    if sanitized == "main" || sanitized == "lib" {
        format!("source_{sanitized}")
    } else {
        sanitized
    }
}

/// Returns the private Rust module identifier used to include a source file.
///
/// The generated file stem intentionally tracks the source module name for
/// readable artifacts. The Rust module item needs a separate internal name
/// because Rust puts `mod Foo;` and `struct Foo` in the same type namespace.
/// Date-fns has source modules such as `Setter.ts` that also define `Setter`,
/// so declaring them through an internal identifier avoids double emission
/// collisions while keeping `Setter.rs` as the artifact file.
fn generated_source_module_ident(file_stem: &str) -> String {
    sanitize_ident(&format!("__smelt_module_{file_stem}"))
}

/// Returns whether this generated function must remain the Rust crate root.
fn is_root_main_function(mir: &Mir, function: &MirFunction, none_ty: TypeId) -> bool {
    !function.is_test
        && function.return_ty == none_ty
        && mir
            .symbols
            .get(function.name)
            .is_some_and(|name| name == "main")
}

/// Makes a top-level generated function visible to the crate root.
fn publicize_free_function(source: String) -> String {
    if source.starts_with("async fn ") {
        return source.replacen("async fn ", "pub(crate) async fn ", 1);
    }
    if source.starts_with("fn ") {
        return source.replacen("fn ", "pub(crate) fn ", 1);
    }
    if source.starts_with("#[")
        && let Some(index) = source.find("\nfn ")
    {
        let mut output = source;
        let start = index.checked_add(1).unwrap_or(index);
        let end = index.checked_add(4).unwrap_or(index);
        output.replace_range(start..end, "pub(crate) fn ");
        return output;
    }
    source
}

/// Inserts module declarations after the generated header and crate attributes.
fn insert_after_crate_header(mut root: String, text: &str) -> String {
    let marker = "#![allow(dead_code, non_snake_case, unused_imports, unused_variables)]\n\n";
    if let Some(index) = root.find(marker) {
        let insertion_index = index.checked_add(marker.len()).unwrap_or(index);
        root.insert_str(insertion_index, text);
        return root;
    }
    root.insert_str(0, text);
    root
}

/// Emit the concrete `SmeltMatch` runtime type for RegExp match results.
///
/// `RegExp.prototype.exec` and `String.prototype.matchAll` return a JavaScript
/// match result: an array-like value whose numbered entries are the capture
/// groups (entry `0` is the whole match) plus the `index`, `input`, and
/// `groups` properties. Historically Smelt built this shape inline as a
/// `SmeltUnknown::Object` property bag, which erased its statically known
/// structure. `SmeltMatch` models that structure with concrete Rust fields:
///
/// * `groups` — numbered capture groups as `Vec<Option<String>>` (an absent
///   optional group is `None`, matching JavaScript `undefined`).
/// * `named` — named capture groups keyed by their original name (and a
///   snake_case alias so generated snake_cased field reads still resolve).
/// * `match_index` — the zero-based match offset (`.index`).
/// * `input` — the full searched string (`.input`).
///
/// Typed accessors (`group`, `named_group`, `index`, `input`, `len`) let
/// callers read the shape without any dynamic tagging. When a match value has
/// to cross a genuinely dynamic boundary (the erased `unknown` result type the
/// frontend still assigns for `exec`/`matchAll` consumers), it is converted
/// with the explicit [`IntoSmeltUnknown`] adapter — the single place where the
/// concrete match is intentionally erased.
fn emit_smelt_match(writer: &mut CodeWriter, needs_unknown: bool) {
    writer.line("/// A concrete JavaScript RegExp match result (numbered groups, named");
    writer.line("/// groups, `index`, and `input`).");
    writer.line("#[derive(Clone, Debug, Default, PartialEq)]");
    writer.block("pub struct SmeltMatch", |struct_writer| {
        struct_writer.line("id: usize,");
        struct_writer.line("/// Numbered capture groups; entry 0 is the whole match. An absent");
        struct_writer.line("/// optional group is `None` (JavaScript `undefined`).");
        struct_writer.line("groups: Vec<Option<String>>,");
        struct_writer.line("/// Named capture groups keyed by name (plus a snake_case alias).");
        struct_writer.line("named: ::std::collections::HashMap<String, Option<String>>,");
        struct_writer.line("/// Zero-based offset of the match within `input` (`.index`).");
        struct_writer.line("match_index: usize,");
        struct_writer.line("/// The full string that was searched (`.input`).");
        struct_writer.line("input: String,");
    });
    writer.blank_line();
    writer.line("#[allow(dead_code)]");
    writer.block("impl SmeltMatch", |impl_writer| {
        impl_writer.line("/// Build a `SmeltMatch` from a compiled regex and its captures.");
        impl_writer.block(
            "fn from_captures(regex: &fancy_regex::Regex, captures: &fancy_regex::Captures<'_>, match_index: usize, input: &str) -> Self",
            |fn_writer| {
                fn_writer.line("let groups = (0..captures.len()).map(|index| captures.get(index).map(|value| value.as_str().to_owned())).collect::<Vec<_>>();");
                fn_writer.line("let mut named = ::std::collections::HashMap::new();");
                fn_writer.block("for name in regex.capture_names().flatten()", |for_writer| {
                    for_writer.line("let value = captures.name(name).map(|value| value.as_str().to_owned());");
                    for_writer.line("named.insert(name.to_owned(), value.clone());");
                    for_writer.line("let mut snake = String::new();");
                    for_writer.block("for (index, ch) in name.chars().enumerate()", |snake_writer| {
                        snake_writer.line("if ch.is_ascii_uppercase() { if index > 0 { snake.push('_'); } snake.push(ch.to_ascii_lowercase()); } else { snake.push(ch); }");
                    });
                    for_writer.line("named.insert(snake, value);");
                });
                fn_writer.line("Self { id: smelt_next_object_id(), groups, named, match_index, input: input.to_owned() }");
            },
        );
        impl_writer.line("/// Read a numbered capture group (`match[n]`).");
        impl_writer.block("fn group(&self, index: usize) -> Option<&str>", |fn_writer| {
            fn_writer.line("self.groups.get(index).and_then(|value| value.as_deref())");
        });
        impl_writer.line("/// Read a numbered capture group as an owned optional string.");
        impl_writer.line("///");
        impl_writer.line("/// Consumer index reads (`match[n]`) are typed `Optional(String)`; a");
        impl_writer.line("/// group that did not participate in the match is `None` (JavaScript");
        impl_writer.line("/// `undefined`).");
        impl_writer.block("pub fn group_owned(&self, index: usize) -> Option<String>", |fn_writer| {
            fn_writer.line("self.groups.get(index).cloned().flatten()");
        });
        impl_writer.line("/// Read a named capture group (`match.groups.name`).");
        impl_writer.block("fn named_group(&self, name: &str) -> Option<&str>", |fn_writer| {
            fn_writer.line("self.named.get(name).and_then(|value| value.as_deref())");
        });
        impl_writer.line("/// Read a named capture group as an owned optional string.");
        impl_writer.line("///");
        impl_writer.line("/// Consumer named-group reads (`match.groups.name`) are typed");
        impl_writer.line("/// `Optional(String)`; a group that did not participate is `None`.");
        impl_writer.block("pub fn named_group_owned(&self, name: &str) -> Option<String>", |fn_writer| {
            fn_writer.line("self.named.get(name).cloned().flatten()");
        });
        impl_writer.line("/// The zero-based match offset (`match.index`).");
        impl_writer.block("pub fn index(&self) -> f64", |fn_writer| {
            fn_writer.line("self.match_index as f64");
        });
        impl_writer.line("/// The full searched string (`match.input`).");
        impl_writer.block("fn input(&self) -> &str", |fn_writer| {
            fn_writer.line("&self.input");
        });
        impl_writer.line("/// The full searched string as an owned value (`match.input`).");
        impl_writer.block("pub fn input_owned(&self) -> String", |fn_writer| {
            fn_writer.line("self.input.clone()");
        });
        impl_writer.line("/// Number of numbered capture groups, including the whole match.");
        impl_writer.block("fn len(&self) -> usize", |fn_writer| {
            fn_writer.line("self.groups.len()");
        });
        impl_writer.line("/// Number of numbered capture groups as a JavaScript number (`match.length`).");
        impl_writer.block("pub fn length(&self) -> f64", |fn_writer| {
            fn_writer.line("self.groups.len() as f64");
        });
        impl_writer.line("/// Whether there are no numbered groups (never true for a real match).");
        impl_writer.block("fn is_empty(&self) -> bool", |fn_writer| {
            fn_writer.line("self.groups.is_empty()");
        });
    });
    writer.blank_line();
    // The erasure adapter references the `SmeltUnknown` carrier and is only
    // emitted when the crate genuinely erases a match into dynamic dataflow.
    // Programs that keep every match read statically typed never emit
    // `SmeltUnknown`, so the adapter would otherwise fail to compile.
    if needs_unknown {
        writer.line("/// Erase a concrete match into a `SmeltUnknown` at a dynamic boundary.");
        writer.line("///");
        writer.line("/// This reproduces the JavaScript match-array-with-properties shape:");
        writer.line("/// numbered string keys for the groups plus `groups`, `index`, and");
        writer.line("/// `input`. It is the single explicit adapter used when a typed");
        writer.line("/// `SmeltMatch` must flow into erased `unknown` consumer dataflow.");
        writer.block("impl IntoSmeltUnknown for SmeltMatch", |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                fn_writer.line("let mut object = ::std::collections::HashMap::new();");
                fn_writer.line("for (index, value) in self.groups.iter().enumerate() { object.insert(index.to_string(), value.clone().map_or(SmeltUnknown::Undefined, SmeltUnknown::String)); }");
                fn_writer.line("let groups = self.named.into_iter().map(|(name, value)| (name, value.map_or(SmeltUnknown::Undefined, SmeltUnknown::String))).collect::<::std::collections::HashMap<_, _>>();");
                fn_writer.line("object.insert(\"groups\".to_owned(), SmeltUnknown::Object(SmeltObject::new(groups)));");
                fn_writer.line("object.insert(\"index\".to_owned(), SmeltUnknown::Number(self.match_index as f64));");
                fn_writer.line("object.insert(\"input\".to_owned(), SmeltUnknown::String(self.input));");
                fn_writer.line("SmeltUnknown::Object(SmeltObject::with_id(self.id, object))");
            });
        });
        writer.blank_line();
    }
}

/// Emit natural JSON serde support for `SmeltUnknown`.
fn emit_unknown_serde_impls(writer: &mut CodeWriter) {
    writer.block("impl serde::Serialize for SmeltUnknown", |impl_writer| {
        impl_writer.block(
            "fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer",
            |fn_writer| {
                fn_writer.block("match self", |match_writer| {
                    match_writer.line("Self::Null => serializer.serialize_none(),");
                    match_writer.line("Self::Undefined => serializer.serialize_none(),");
                    match_writer.line("Self::Bool(value) => serializer.serialize_bool(*value),");
                    match_writer.line("Self::Number(value) => serializer.serialize_f64(*value),");
                    match_writer.line("Self::String(value) => serializer.serialize_str(value),");
                    match_writer.line("Self::Symbol(value) => serializer.serialize_str(value),");
                    match_writer.line("Self::Array(values) => serde::Serialize::serialize(&values.values, serializer),");
                    match_writer.line("Self::Object(values) => serde::Serialize::serialize(&values.iter().filter(|(key, _)| key != \"__smelt_class\").collect::<::std::collections::HashMap<_, _>>(), serializer),");
                    match_writer.line("Self::Function(_) => serializer.serialize_str(\"function () { [native code] }\"),");
                    match_writer.line("Self::Promise(_) => serializer.serialize_str(\"[object Promise]\"),");
                });
            },
        );
    });
    writer.blank_line();
    writer.block("impl<'de> serde::Deserialize<'de> for SmeltUnknown", |impl_writer| {
        impl_writer.block(
            "fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: serde::Deserializer<'de>",
            |fn_writer| {
                fn_writer.line("let value = <serde_json::Value as serde::Deserialize>::deserialize(deserializer)?;");
                fn_writer.line(format!(
                    "Ok({from_json}(value))",
                    from_json = smelt_stdlib::runtime_symbols::json::UNKNOWN_FROM_JSON_VALUE,
                ));
            },
        );
    });
    writer.blank_line();
    let from_json = smelt_stdlib::runtime_symbols::json::UNKNOWN_FROM_JSON_VALUE;
    writer.block(
        format!("fn {from_json}(value: serde_json::Value) -> SmeltUnknown"),
        |fn_writer| {
            fn_writer.block("match value", |match_writer| {
                match_writer.line("serde_json::Value::Null => SmeltUnknown::Null,");
                match_writer.line("serde_json::Value::Bool(value) => SmeltUnknown::Bool(value),");
                match_writer.line("serde_json::Value::Number(value) => SmeltUnknown::Number(value.as_f64().unwrap_or_default()),");
                match_writer.line("serde_json::Value::String(value) => SmeltUnknown::String(value),");
                match_writer.line(format!(
                    "serde_json::Value::Array(values) => SmeltUnknown::Array(values.into_iter().map({from_json}).collect()),"
                ));
                match_writer.line(format!(
                    "serde_json::Value::Object(values) => SmeltUnknown::Object(SmeltObject::new(values.into_iter().map(|(key, value)| (key, {from_json}(value))).collect())),"
                ));
            });
        },
    );
    writer.blank_line();
}

/// Emit storage for a reference class as a handle newtype over `Rc<RefCell<_>>`.
///
/// A reference class `Name` becomes a thin handle `struct Name(Rc<RefCell<
/// NameInner>>)` whose fields live in `NameInner`. Identity lives only in the
/// wrapper, so:
/// - `Clone` is hand-written as `Rc::clone` — a clone shares the SAME cell, which
///   is JavaScript reference-identity and the fix for the silent "mutate a
///   throwaway clone" miscompile;
/// - `Default`/`Debug` delegate through the cell;
/// - `IntoSmeltUnknown` (only when the crate crosses a dynamic boundary) borrows
///   the cell and projects the public fields into an object.
///
/// The inner struct carries the same concrete field types the value struct would
/// have used, so field access stays strongly typed; only identity concentrates
/// in the outer `Rc`.
fn emit_reference_class_storage(
    writer: &mut CodeWriter,
    mir: &Mir,
    context: &EmitContext,
    class: &smelt_mir::MirClass,
    needs_unknown: bool,
) -> Result<(), EmitError> {
    let name = class_name_text(mir, class)?;
    let inner_name = format!("{name}Inner");
    let type_params = class_type_params_text(mir, class)?;
    let type_args = class_type_args_text(mir, class)?;
    let impl_generics = class_impl_generics_text(mir, class)?;
    let fields = effective_class_fields(mir, class);
    let scoped_type_params = class
        .type_params
        .iter()
        .map(|param| param.name)
        .collect::<HashSet<_>>();
    let has_function_field = fields
        .iter()
        .any(|field| type_contains_function(mir, field.ty));
    let phantom_args = class
        .type_params
        .iter()
        .map(|param| {
            mir.symbols
                .get(param.name)
                .map(|param_name| RustIdent::new(param_name).into_string())
                .ok_or_else(|| EmitError::new("class type parameter has unknown symbol"))
        })
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");

    // The handle newtype. `#[derive(Clone)]` is intentionally NOT used: a derived
    // clone would require `Inner: Clone` and clone the cell contents. We hand-
    // write `Rc::clone` below so a clone shares identity.
    writer.line("#[allow(dead_code)]");
    writer.line(format!(
        "struct {name}{type_params}(::std::rc::Rc<::std::cell::RefCell<{inner_name}{type_args}>>);"
    ));

    // The inner record. Debug/Default are derived unless a callback field blocks
    // the derives, matching the value-struct rules.
    if has_function_field {
        writer.line("#[allow(dead_code)]");
        writer.block(
            format!("struct {inner_name}{type_params}"),
            |block_writer| {
                emit_reference_inner_fields(block_writer, mir, context, &fields, &scoped_type_params, &phantom_args);
            },
        );
        emit_default_impl_for_storage_type(
            writer,
            mir,
            context,
            &inner_name,
            &impl_generics,
            &type_args,
            &fields,
            &phantom_args,
            &scoped_type_params,
        )?;
        emit_debug_impl_for_storage_type(writer, &inner_name, &impl_generics, &type_args);
    } else {
        writer.line("#[derive(Debug, Default)]");
        writer.line("#[allow(dead_code)]");
        writer.block(
            format!("struct {inner_name}{type_params}"),
            |block_writer| {
                emit_reference_inner_fields(block_writer, mir, context, &fields, &scoped_type_params, &phantom_args);
            },
        );
    }

    // Hand-written identity `Clone`: share the SAME cell.
    writer.block(
        format!("impl{impl_generics} Clone for {name}{type_args}"),
        |impl_writer| {
            impl_writer.block("fn clone(&self) -> Self", |fn_writer| {
                fn_writer.line(format!("{name}(::std::rc::Rc::clone(&self.0))"));
            });
        },
    );

    // Hand-written identity `PartialEq`: a reference class has JavaScript object
    // identity, so `==`/`===`/`toBe` compare whether two handles share the same
    // cell (`Rc::ptr_eq`), never the cell contents. This also avoids requiring
    // `Inner: PartialEq`, which a callback-storing inner record could not satisfy.
    writer.block(
        format!("impl{impl_generics} PartialEq for {name}{type_args}"),
        |impl_writer| {
            impl_writer.block("fn eq(&self, other: &Self) -> bool", |fn_writer| {
                fn_writer.line("::std::rc::Rc::ptr_eq(&self.0, &other.0)");
            });
        },
    );

    // `Default` wraps a defaulted inner record in a fresh cell.
    writer.block(
        format!("impl{impl_generics} Default for {name}{type_args}"),
        |impl_writer| {
            impl_writer.block("fn default() -> Self", |fn_writer| {
                fn_writer.line(format!(
                    "{name}(::std::rc::Rc::new(::std::cell::RefCell::new({inner_name}::default())))"
                ));
            });
        },
    );

    // `Debug` delegates through the cell to the inner record for a non-generic
    // class. A generic class's declared `T` bound does not include `Debug`, so a
    // delegating body would not satisfy `Inner<T>: Debug`; it falls back to a
    // non-exhaustive struct debug that needs no extra bound.
    writer.block(
        format!("impl{impl_generics} ::std::fmt::Debug for {name}{type_args}"),
        |impl_writer| {
            impl_writer.block(
                "fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result",
                |fn_writer| {
                    if class.type_params.is_empty() {
                        fn_writer.line("::std::fmt::Debug::fmt(&*self.0.borrow(), formatter)");
                    } else {
                        fn_writer.line(format!(
                            "formatter.debug_struct({name:?}).finish_non_exhaustive()"
                        ));
                    }
                },
            );
        },
    );

    if !class.static_fields.is_empty() {
        writer.block(
            format!("impl{impl_generics} {name}{type_args}"),
            |impl_writer| {
                for field in &class.static_fields {
                    let field_name = mir
                        .symbols
                        .get(field.name)
                        .map(RustIdent::new)
                        .map_or_else(|| "field".to_owned(), RustIdent::into_string);
                    let field_ty = FunctionEmitter::type_text_for_with_scoped_type_params(
                        mir,
                        context,
                        field.ty,
                        &scoped_type_params,
                    )
                    .unwrap_or_else(|_| "SmeltUnknown".to_owned());
                    let value = materialized_static_value_text(field.value.as_ref());
                    impl_writer.block(
                        format!("fn __smelt_static_{field_name}() -> {field_ty}"),
                        |function_writer| function_writer.line(value),
                    );
                }
            },
        );
    }

    if needs_unknown {
        emit_reference_class_into_smelt_unknown_impl(
            writer,
            mir,
            &name,
            &impl_generics,
            &type_args,
            &fields,
        )?;
    }
    writer.blank_line();
    Ok(())
}

/// Emit the field lines of a reference class's inner record.
fn emit_reference_inner_fields(
    block_writer: &mut CodeWriter,
    mir: &Mir,
    context: &EmitContext,
    fields: &[smelt_mir::MirField],
    scoped_type_params: &HashSet<smelt_hir::Symbol>,
    phantom_args: &str,
) {
    for field in fields {
        let field_name = RustIdent::new(mir.symbols.get(field.name).unwrap_or("field")).into_string();
        let field_ty = FunctionEmitter::type_text_for_with_scoped_type_params(
            mir,
            context,
            field.ty,
            scoped_type_params,
        )
        .unwrap_or_else(|_| "SmeltUnknown".to_owned());
        block_writer.line(format!("{field_name}: {field_ty},"));
    }
    if !phantom_args.is_empty() {
        block_writer.line(format!(
            "_smelt_phantom: ::std::marker::PhantomData<({phantom_args})>,"
        ));
    }
}

/// Emit `IntoSmeltUnknown` for a reference class by borrowing the shared cell.
///
/// Mirrors [`emit_record_into_smelt_unknown_impl`] but projects the public
/// fields out of `self.0.borrow()` rather than owned struct fields.
fn emit_reference_class_into_smelt_unknown_impl(
    writer: &mut CodeWriter,
    mir: &Mir,
    name: &str,
    impl_generics: &str,
    type_args: &str,
    fields: &[smelt_mir::MirField],
) -> Result<(), EmitError> {
    writer.block(
        format!("impl{impl_generics} IntoSmeltUnknown for {name}{type_args}"),
        |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                fn_writer.line("let __smelt_inner = self.0.borrow();");
                fn_writer.line(
                    "SmeltUnknown::Object(SmeltObject::new(::std::collections::HashMap::from([",
                );
                for field in fields {
                    if matches!(field.visibility, smelt_hir::Visibility::Private) {
                        continue;
                    }
                    let key = mir.symbols.get(field.name).unwrap_or("field");
                    let field_name = RustIdent::new(key).into_string();
                    let value = record_field_unknown_text(
                        mir,
                        &format!("__smelt_inner.{field_name}.clone()"),
                        field.ty,
                    )
                    .unwrap_or_else(|_| "SmeltUnknown::Null".to_owned());
                    fn_writer.line(format!("({key:?}.to_owned(), {value}),"));
                }
                fn_writer.line("])))");
            });
        },
    );
    Ok(())
}

/// Emits a manual `Default` impl for generated storage structs with callbacks.
///
/// Rust cannot derive `Default` for fields that contain `dyn FnMut`, but Smelt
/// still needs defaultable generated option bags and locale structs. This
/// helper reuses the function emitter's canonical default expression for each
/// field so callback defaults remain callable and ordinary fields match local
/// initialization behavior.
fn emit_default_impl_for_storage_type(
    writer: &mut CodeWriter,
    mir: &Mir,
    context: &EmitContext,
    name: &str,
    impl_generics: &str,
    type_args: &str,
    fields: &[smelt_mir::MirField],
    phantom_args: &str,
    scoped_type_params: &HashSet<smelt_hir::Symbol>,
) -> Result<(), EmitError> {
    writer.block(
        format!("impl{impl_generics} Default for {name}{type_args}"),
        |impl_writer| {
            impl_writer.block("fn default() -> Self", |fn_writer| {
                fn_writer.block("Self", |self_writer| {
                    for field in fields {
                        let field_name =
                            RustIdent::new(mir.symbols.get(field.name).unwrap_or("field"))
                                .into_string();
                        let default_value =
                            FunctionEmitter::default_value_for_with_scoped_type_params(
                                mir,
                                context,
                                field.ty,
                                scoped_type_params,
                            )
                            .unwrap_or_else(|_| "Default::default()".to_owned());
                        self_writer.line(format!("{field_name}: {default_value},"));
                    }
                    if !phantom_args.is_empty() {
                        self_writer.line("_smelt_phantom: ::std::marker::PhantomData,");
                    }
                });
            });
        },
    );
    Ok(())
}

/// Emits a conservative `Debug` impl for storage structs that contain callbacks.
///
/// Callback trait objects cannot be formatted structurally, but parent option
/// bags still often derive `Debug` and therefore need nested storage records to
/// satisfy Rust bounds. The generated implementation names the storage type and
/// intentionally omits callable field internals.
fn emit_debug_impl_for_storage_type(
    writer: &mut CodeWriter,
    name: &str,
    impl_generics: &str,
    type_args: &str,
) {
    writer.block(
        format!("impl{impl_generics} ::std::fmt::Debug for {name}{type_args}"),
        |impl_writer| {
            impl_writer.block(
                "fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result",
                |fn_writer| {
                    fn_writer.line(format!(
                        "formatter.debug_struct({name:?}).finish_non_exhaustive()"
                    ));
                },
            );
        },
    );
}

/// Return whether a type contains a callable value in a stored position.
///
/// Rust cannot derive `Clone` or `Debug` for `dyn FnMut` trait objects. Class
/// structs that store function fields therefore opt out of those derives until
/// callable storage grows an explicit cloneable wrapper.
fn type_contains_function(mir: &Mir, ty: TypeId) -> bool {
    match mir.types.get(ty) {
        Some(Type::Function(_)) => true,
        Some(Type::List(item) | Type::Set(item) | Type::Optional(item) | Type::Future(item)) => {
            type_contains_function(mir, *item)
        }
        Some(Type::Dict(key, value)) => {
            type_contains_function(mir, *key) || type_contains_function(mir, *value)
        }
        Some(Type::Tuple(items) | Type::Union(items)) => {
            items.iter().any(|item| type_contains_function(mir, *item))
        }
        Some(
            Type::Bool
            | Type::Int
            | Type::Float
            | Type::String
            | Type::None
            | Type::Unknown
            | Type::Never
            | Type::TypeParam { .. }
            | Type::Class { .. },
        )
        | None => false,
    }
}

/// Return whether a type supports Rust `PartialEq` in generated code.
///
/// A value class derives `PartialEq` for JavaScript structural comparison
/// (`==`/`===`/`toBe` on the by-value representation and derived comparisons in
/// generated specs) only when every stored field is itself comparable. The two
/// prelude shapes without `PartialEq` are `dyn Fn` callbacks and the
/// `Clone`-only `SmeltPromise` shared-future handle, so any type transitively
/// reaching one is rejected. Class fields are followed through their effective
/// field layout — a reference class always compares by identity (`Rc::ptr_eq`)
/// and is therefore always comparable, while a value class is comparable only
/// when its own fields are. `seen` breaks recursive class cycles by treating an
/// in-progress class as comparable (its fields are validated by the outer call).
fn type_supports_partial_eq(
    mir: &Mir,
    context: &EmitContext,
    ty: TypeId,
    seen: &mut Vec<smelt_hir::Symbol>,
) -> bool {
    match mir.types.get(ty) {
        Some(Type::Function(_) | Type::Future(_)) => false,
        Some(Type::List(item) | Type::Set(item) | Type::Optional(item)) => {
            type_supports_partial_eq(mir, context, *item, seen)
        }
        Some(Type::Dict(key, value)) => {
            type_supports_partial_eq(mir, context, *key, seen)
                && type_supports_partial_eq(mir, context, *value, seen)
        }
        // A concrete union lowers to a generated enum with a hand-written
        // `PartialEq` (and an erased union falls back to `SmeltUnknown`, which is
        // also `PartialEq`), so a union field is always comparable regardless of
        // its members.
        Some(Type::Union(_)) => true,
        Some(Type::Tuple(items)) => items
            .iter()
            .all(|item| type_supports_partial_eq(mir, context, *item, seen)),
        Some(Type::Class { name, .. }) => {
            let name = *name;
            // A reference class compares by identity (`Rc::ptr_eq`) with no
            // constraint on its inner fields, so it is always comparable.
            if context.is_reference_class(name) {
                return true;
            }
            let Some(class) = mir.classes.iter().find(|candidate| candidate.name == name) else {
                // An external/builtin class surface (e.g. a runtime prelude type)
                // is assumed comparable; only user classes gate the derive.
                return true;
            };
            if seen.contains(&name) {
                return true;
            }
            seen.push(name);
            let comparable = effective_class_fields(mir, class)
                .iter()
                .all(|field| type_supports_partial_eq(mir, context, field.ty, seen));
            seen.pop();
            comparable
        }
        Some(
            Type::Bool
            | Type::Int
            | Type::Float
            | Type::String
            | Type::None
            | Type::Unknown
            | Type::Never
            | Type::TypeParam { .. },
        )
        | None => true,
    }
}

/// Return whether all stored fields in a class are JSON-compatible.
fn class_is_json_serializable(mir: &Mir, class: &smelt_mir::MirClass) -> bool {
    let mut seen = Vec::new();
    effective_class_fields(mir, class)
        .iter()
        .all(|field| type_is_json_serializable(mir, field.ty, &mut seen))
}

/// Return whether all stored fields in an interface are JSON-compatible.
fn interface_is_json_serializable(mir: &Mir, interface: &smelt_mir::MirInterface) -> bool {
    let mut seen = vec![interface.name];
    effective_interface_fields(mir, interface)
        .iter()
        .all(|field| type_is_json_serializable(mir, field.ty, &mut seen))
}

/// Emits `IntoSmeltUnknown` for a generated record storage type.
///
/// Some lowered object operations temporarily recover string-keyed object
/// shapes from typed class/interface records. The implementation keeps that
/// conversion available without requiring every call site to know the concrete
/// record type that produced the object.
fn emit_record_into_smelt_unknown_impl(
    writer: &mut CodeWriter,
    mir: &Mir,
    name: &str,
    impl_generics: &str,
    type_args: &str,
    fields: &[smelt_mir::MirField],
) -> Result<(), EmitError> {
    writer.block(
        format!("impl{impl_generics} IntoSmeltUnknown for {name}{type_args}"),
        |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                fn_writer.line(
                    "SmeltUnknown::Object(SmeltObject::new(::std::collections::HashMap::from([",
                );
                for field in fields {
                    if matches!(field.visibility, smelt_hir::Visibility::Private) {
                        continue;
                    }
                    let key = mir.symbols.get(field.name).unwrap_or("field");
                    let field_name = RustIdent::new(key).into_string();
                    let value =
                        record_field_unknown_text(mir, &format!("self.{field_name}"), field.ty)
                            .unwrap_or_else(|_| "SmeltUnknown::Null".to_owned());
                    fn_writer.line(format!("({key:?}.to_owned(), {value}),"));
                }
                fn_writer.line("])))");
            });
        },
    );
    Ok(())
}

/// Renders a generated record field as a `SmeltUnknown` expression.
fn record_field_unknown_text(mir: &Mir, value_text: &str, ty: TypeId) -> Result<String, EmitError> {
    Ok(match mir.types.get(ty) {
        Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. }) => {
            format!("({value_text}).into_smelt_unknown()")
        }
        Some(Type::None | Type::Never | Type::Future(_)) | None => "SmeltUnknown::Null".to_owned(),
        Some(Type::Bool) => format!("SmeltUnknown::Bool({value_text})"),
        Some(Type::Int | Type::Float) => format!("SmeltUnknown::Number({value_text} as f64)"),
        Some(Type::String) => format!("SmeltUnknown::String({value_text})"),
        Some(Type::Optional(inner)) => {
            let inner_text = record_field_unknown_text(mir, "value", *inner)?;
            format!("{value_text}.map_or(SmeltUnknown::Undefined, |value| {inner_text})")
        }
        Some(Type::List(item) | Type::Set(item)) => {
            let item_text = record_field_unknown_text(mir, "value", *item)?;
            format!(
                "SmeltUnknown::Array({value_text}.into_iter().map(|value| {item_text}).collect())"
            )
        }
        Some(Type::Dict(key, item)) if matches!(mir.types.get(*key), Some(Type::String)) => {
            let item_text = record_field_unknown_text(mir, "value", *item)?;
            format!(
                "SmeltUnknown::Object(SmeltObject::new({value_text}.into_iter().map(|(key, value)| (key, {item_text})).collect()))"
            )
        }
        Some(Type::Dict(_, _) | Type::Tuple(_) | Type::Class { .. }) => {
            format!("({value_text}).into_smelt_unknown()")
        }
        Some(Type::Function(_)) => "SmeltUnknown::Null".to_owned(),
    })
}

/// Return whether generated serde can represent a type as natural JSON.
fn type_is_json_serializable(mir: &Mir, ty: TypeId, seen: &mut Vec<smelt_hir::Symbol>) -> bool {
    match mir.types.get(ty) {
        Some(Type::Bool | Type::Int | Type::Float | Type::String | Type::Unknown) => true,
        Some(Type::List(item) | Type::Set(item) | Type::Optional(item)) => {
            type_is_json_serializable(mir, *item, seen)
        }
        Some(Type::Tuple(items)) => items
            .iter()
            .all(|item| type_is_json_serializable(mir, *item, seen)),
        Some(Type::Dict(key, value)) => {
            matches!(mir.types.get(*key), Some(Type::String))
                && type_is_json_serializable(mir, *value, seen)
        }
        Some(Type::Class { name, .. }) => {
            if seen.contains(name) {
                return true;
            }
            seen.push(*name);
            let serializable =
                if let Some(class) = mir.classes.iter().find(|class| class.name == *name) {
                    effective_class_fields(mir, class)
                        .iter()
                        .all(|field| type_is_json_serializable(mir, field.ty, seen))
                } else if let Some(interface) = mir
                    .interfaces
                    .iter()
                    .find(|interface| interface.name == *name)
                {
                    effective_interface_fields(mir, interface)
                        .iter()
                        .all(|field| type_is_json_serializable(mir, field.ty, seen))
                } else {
                    false
                };
            seen.pop();
            serializable
        }
        _ => false,
    }
}

/// Return whether the MIR already contains a Rust entrypoint.
fn has_main_function(mir: &Mir) -> Result<bool, EmitError> {
    let none_ty = mir
        .types
        .all()
        .iter()
        .enumerate()
        .find_map(|(id, ty)| {
            (*ty == Type::None)
                .then(|| compact_index(id, "type index does not fit u32").map(TypeId))
        })
        .transpose()?
        .unwrap_or(TypeId(u32::MAX));
    Ok(mir.functions.iter().any(|function| {
        mir.symbols
            .get(function.name)
            .is_some_and(|name| name == "main")
            && function.return_ty == none_ty
    }))
}

/// Return whether generated code needs the first-class erased callback runtime.
fn needs_erased_function_runtime(mir: &Mir) -> bool {
    mir.types.all().iter().any(|ty| {
        let Type::Function(function) = ty else {
            return false;
        };
        if function.is_async || matches!(mir.types.get(function.return_ty), Some(Type::Future(_))) {
            return false;
        }
        let Some(0) = function.rest else {
            return false;
        };
        let [param] = function.params.as_slice() else {
            return false;
        };
        matches!(
            mir.types.get(*param),
            Some(Type::List(item))
                if matches!(
                    mir.types.get(*item),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Never)
                )
        )
    })
}

/// Collects the dependency list required by generated Rust code.
fn generated_deps(mir: &Mir) -> Vec<GeneratedDep> {
    let mut deps = Vec::new();
    if stdlib::needs_tokio(mir) || stdlib::needs_unknown_type(mir) {
        deps.push(GeneratedDep::Tokio);
    }
    deps.extend(
        stdlib::backend_dependencies(mir)
            .into_iter()
            .map(GeneratedDep::Stdlib),
    );
    deps
}

/// Helper struct for emitting Rust code from a MirFunction.
fn sanitize_ident(name: &str) -> String {
    RustIdent::new(name).into_string()
}

/// Emit the `thread_local!` block declaring every mutable global's cell.
///
/// Each `#[test]` runs on its own thread, so a `thread_local!` cell gives every
/// test a fresh copy of module state initialized from the source literal —
/// deterministic, mirroring vitest's per-file module isolation. Copy primitives
/// (`f64`/`i64`/`bool`) use a `const`-initialized [`std::cell::Cell`]; strings
/// use a [`std::cell::RefCell`] with the non-const initializer form because a
/// `.to_owned()` initializer cannot appear in a `const` block.
fn emit_mutable_globals(mir: &Mir) -> Result<String, EmitError> {
    let mut writer = CodeWriter::new();
    writer.line("thread_local! {");
    for (index, global) in mir.globals.iter().enumerate() {
        let name = global_static_name(mir, compact_index(index, "global index")?);
        let init = emitter::literals::constant_text(&global.init);
        match mir.types.get(global.ty) {
            Some(Type::String) => {
                // `init` is already the owned-string expression (`"…".to_owned()`),
                // which cannot appear in a `const` block, hence the non-const form.
                writer.line(format!(
                    "    static {name}: ::std::cell::RefCell<String> = ::std::cell::RefCell::new({init});"
                ));
            }
            Some(Type::Float) => {
                writer.line(format!(
                    "    static {name}: ::std::cell::Cell<f64> = const {{ ::std::cell::Cell::new({init}) }};"
                ));
            }
            Some(Type::Int) => {
                writer.line(format!(
                    "    static {name}: ::std::cell::Cell<i64> = const {{ ::std::cell::Cell::new({init}) }};"
                ));
            }
            Some(Type::Bool) => {
                writer.line(format!(
                    "    static {name}: ::std::cell::Cell<bool> = const {{ ::std::cell::Cell::new({init}) }};"
                ));
            }
            _ => {
                return Err(EmitError::new(
                    "mutable global has a non-primitive type; only Float/Int/Bool/String are supported",
                ));
            }
        }
    }
    writer.line("}");
    Ok(writer.finish())
}

/// Compute the thread-local static name for a mutable global by index.
///
/// The name is derived from the binding's source symbol (sanitized and
/// uppercased) with the global's dense `Mir::globals` index appended, prefixed
/// `SMELT_GLOBAL_`. Including the index disambiguates cross-module bindings
/// that share a source name so each mutable global gets a unique per-program
/// static. These names are program-specific and never enter the fixed
/// runtime-symbol registry.
pub(crate) fn global_static_name(mir: &Mir, index: u32) -> String {
    let base = usize::try_from(index)
        .ok()
        .and_then(|idx| mir.globals.get(idx))
        .and_then(|global| mir.symbols.get(global.name))
        .map_or_else(|| "global".to_owned(), sanitize_ident);
    format!("SMELT_GLOBAL_{}_{index}", base.to_uppercase())
}

#[cfg(test)]
/// Tests for the code generator.
#[cfg(test)]
mod tests;
