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
use smelt_mir::{HirOrigin, Mir, MirFunction, Rvalue, Statement};

pub(crate) mod classes;
pub(crate) mod deps;
pub mod rust;
pub(crate) mod stdlib;

use deps::GeneratedDep;
mod emitter;
use classes::{
    class_impl_generics_text, class_name_text, class_type_args_text, class_type_params_text,
    effective_class_fields, effective_interface_fields, inherited_trait_methods,
    interface_impl_generics_text, interface_type_params_text,
};
use emitter::{EmitContext, FunctionEmitter};
use rust::{CodeWriter, RustIdent};

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
fn needs_timer_helpers(mir: &Mir) -> bool {
    mir.functions.iter().any(|function| {
        function.blocks.iter().any(|block| {
            block.statements.iter().any(|statement| {
                let Some(value) = (match statement {
                    Statement::Assign { value, .. } | Statement::AssignPlace { value, .. } => {
                        Some(value)
                    }
                    Statement::StorageLive(_) | Statement::StorageDead(_) => None,
                }) else {
                    return false;
                };
                matches!(
                    value,
                    Rvalue::AsyncOp {
                        op: AsyncOp::Sleep | AsyncOp::SetTimeout | AsyncOp::ClearTimeout,
                        ..
                    }
                )
            })
        })
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
    let needs_serde_json =
        stdlib::backend_dependencies(mir).contains(&smelt_stdlib::BackendDependency::SerdeJson);
    let needs_regex =
        stdlib::backend_dependencies(mir).contains(&smelt_stdlib::BackendDependency::Regex);
    let needs_unknown = stdlib::needs_unknown_type(mir);
    let needs_erased_function = needs_erased_function_runtime(mir);
    let needs_date_now = stdlib::needs_date_now_runtime(mir);
    let needs_date_timezone_offset = stdlib::needs_date_timezone_offset_runtime(mir);
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
        writer.line("thread_local! {");
        writer.line("    static SMELT_NEXT_OBJECT_ID: ::std::cell::Cell<usize> = const { ::std::cell::Cell::new(1) };");
        writer.line("}");
        writer.blank_line();
        writer.line("fn smelt_next_object_id() -> usize {");
        writer.line("    SMELT_NEXT_OBJECT_ID.with(|next| { let id = next.get(); next.set(id.saturating_add(1)); id })");
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
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K: SmeltJsKeyEq + Clone, V: Clone> SmeltJsMap<K, V> {");
        writer.line("    fn len(&self) -> usize { self.entries.len() }");
        writer.line("    fn contains_key(&self, key: &K) -> bool { self.entries.iter().any(|(existing, _)| existing.same_js_key(key)) }");
        writer.line("    fn get(&self, key: &K) -> Option<V> { self.entries.iter().find(|(existing, _)| existing.same_js_key(key)).map(|(_, value)| value.clone()) }");
        writer.line("    fn insert(&mut self, key: K, value: V) -> Option<V> { if let Some((_, existing)) = self.entries.iter_mut().find(|(existing, _)| existing.same_js_key(&key)) { Some(::std::mem::replace(existing, value)) } else { self.entries.push((key, value)); None } }");
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
        writer.line("pub trait SmeltJsKeyEq {");
        writer.line("    fn same_js_key(&self, other: &Self) -> bool;");
        writer.line("}");
        writer.blank_line();
        writer.line("impl SmeltJsKeyEq for SmeltUnknown {");
        writer.line("    fn same_js_key(&self, other: &Self) -> bool { match (self, other) { (SmeltUnknown::Number(left), SmeltUnknown::Number(right)) if left.is_nan() && right.is_nan() => true, (SmeltUnknown::Array(left), SmeltUnknown::Array(right)) => left.id == right.id, (SmeltUnknown::Object(left), SmeltUnknown::Object(right)) => left.id == right.id, (SmeltUnknown::Function(left), SmeltUnknown::Function(right)) => ::std::rc::Rc::ptr_eq(left, right), _ => self == other } }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl SmeltJsKeyEq for String { fn same_js_key(&self, other: &Self) -> bool { self == other } }");
        writer.line("impl SmeltJsKeyEq for bool { fn same_js_key(&self, other: &Self) -> bool { self == other } }");
        writer.line("impl SmeltJsKeyEq for i64 { fn same_js_key(&self, other: &Self) -> bool { self == other } }");
        writer.line("impl SmeltJsKeyEq for f64 { fn same_js_key(&self, other: &Self) -> bool { (self.is_nan() && other.is_nan()) || self == other } }");
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
        writer.line("fn smelt_is_for_in_object_key(object: &SmeltObject, key: &str) -> bool { key != \"__smelt_date\" && key != \"__smelt_timezone\" && !(object.contains_key(\"__smelt_regexp\") && matches!(key, \"__smelt_regexp\" | \"source\" | \"flags\")) }");
        writer
            .line("/// Return whether a record key is visible to JavaScript `for...in` iteration.");
        writer.line("fn smelt_is_for_in_record_key<V>(record: &SmeltRecord<String, V>, key: &str) -> bool { key != \"__smelt_date\" && key != \"__smelt_timezone\" && !(record.contains_key(\"__smelt_regexp\") && matches!(key, \"__smelt_regexp\" | \"source\" | \"flags\")) }");
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
            "    /// Consume an erased array when lowering back to statically typed list storage.",
        );
        writer.line("    fn into_vec(self) -> Vec<SmeltUnknown> { self.values }");
        writer.line("}");
        writer.line("impl From<Vec<SmeltUnknown>> for SmeltArray { fn from(values: Vec<SmeltUnknown>) -> Self { Self::new(values) } }");
        writer.line("impl ::std::iter::FromIterator<SmeltUnknown> for SmeltArray { fn from_iter<T: IntoIterator<Item = SmeltUnknown>>(iter: T) -> Self { Self::new(iter.into_iter().collect()) } }");
        writer.line("impl ::std::ops::Deref for SmeltArray { type Target = [SmeltUnknown]; fn deref(&self) -> &Self::Target { &self.values } }");
        writer.line("impl IntoIterator for SmeltArray { type Item = SmeltUnknown; type IntoIter = ::std::vec::IntoIter<SmeltUnknown>; fn into_iter(self) -> Self::IntoIter { self.values.into_iter() } }");
        writer.blank_line();
        writer.block("pub enum SmeltUnknown", |unknown_writer| {
            unknown_writer.line("Null,");
            unknown_writer.line("Bool(bool),");
            unknown_writer.line("Number(f64),");
            unknown_writer.line("String(String),");
            unknown_writer.line("Symbol(String),");
            unknown_writer.line("Array(SmeltArray),");
            unknown_writer.line("Object(SmeltObject),");
            unknown_writer.line("Function(::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>),");
        });
        writer.blank_line();
        writer.block("impl Clone for SmeltUnknown", |impl_writer| {
            impl_writer.block("fn clone(&self) -> Self", |fn_writer| {
                fn_writer.block("match self", |match_writer| {
                    match_writer.line("Self::Null => Self::Null,");
                    match_writer.line("Self::Bool(value) => Self::Bool(*value),");
                    match_writer.line("Self::Number(value) => Self::Number(*value),");
                    match_writer.line("Self::String(value) => Self::String(value.clone()),");
                    match_writer.line("Self::Symbol(value) => Self::Symbol(value.clone()),");
                    match_writer.line("Self::Array(values) => Self::Array(values.clone()),");
                    match_writer.line("Self::Object(values) => Self::Object(values.clone()),");
                    match_writer.line("Self::Function(value) => Self::Function(value.clone()),");
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
            writer.line("    fn call(&self, args: Vec<SmeltUnknown>) -> SmeltUnknown {");
            writer.line("        (self.callback)(args)");
            writer.line("    }");
            writer.line(
                "    /// Restore an erased callable value without dropping callable-object fields.",
            );
            writer.line("    fn into_smelt_unknown(self) -> SmeltUnknown {");
            writer.line("        let callback = self.callback.clone();");
            writer.line("        let callable = SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>((callback)(args))));");
            writer.line("        if let Some(object) = self.object { object.insert(\"__smelt_call\".to_owned(), callable); SmeltUnknown::Object(object) } else { callable }");
            writer.line("    }");
            writer.line("}");
        }
        writer.blank_line();
        writer.line("fn smelt_get_object_field(map: &SmeltObject, field: &str) -> SmeltUnknown {");
        writer.line("    match map.get(field).unwrap_or(SmeltUnknown::Null) {");
        writer.line("        SmeltUnknown::Object(getter) if getter.contains_key(\"__smelt_get\") => match getter.get(\"__smelt_get\") {");
        writer.line("            Some(SmeltUnknown::Function(smelt_getter)) => (smelt_getter)(Vec::new()).unwrap_or_else(|error| panic!(\"{}\", error)),");
        writer.line("            _ => SmeltUnknown::Null,");
        writer.line("        },");
        writer.line("        value => value,");
        writer.line("    }");
        writer.line("}");
        writer.blank_line();
        writer.line("fn smelt_unknown_structural_eq(left: &SmeltUnknown, right: &SmeltUnknown, seen: &mut ::std::collections::HashSet<(usize, usize)>) -> bool {");
        writer.line("    match (left, right) {");
        writer.line("        (SmeltUnknown::Null, SmeltUnknown::Null) => true,");
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
                        match_writer.line("Self::Bool(value) => formatter.debug_tuple(\"Bool\").field(value).finish(),");
                        match_writer.line("Self::Number(value) => formatter.debug_tuple(\"Number\").field(value).finish(),");
                        match_writer.line("Self::String(value) => formatter.debug_tuple(\"String\").field(value).finish(),");
                        match_writer.line("Self::Symbol(value) => formatter.debug_tuple(\"Symbol\").field(value).finish(),");
                        match_writer.line("Self::Array(values) => formatter.debug_tuple(\"Array\").field(values).finish(),");
                        match_writer.line("Self::Object(values) => formatter.debug_tuple(\"Object\").field(values).finish(),");
                        match_writer.line("Self::Function(_) => formatter.write_str(\"Function(<closure>)\"),");
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
            writer.line("}");
            writer.blank_line();
            writer.line("thread_local! {");
            writer.line("    static SMELT_NEXT_TIMER_ID: ::std::cell::Cell<u64> = const { ::std::cell::Cell::new(1) };");
            writer.line("    static SMELT_TIMER_NOW_MS: ::std::cell::Cell<u64> = const { ::std::cell::Cell::new(0) };");
            writer.line("    static SMELT_TIMERS: ::std::cell::RefCell<Vec<SmeltTimer>> = const { ::std::cell::RefCell::new(Vec::new()) };");
            writer.line("}");
            writer.blank_line();
            writer.line("fn smelt_reset_timers() {");
            writer.line("    SMELT_NEXT_TIMER_ID.with(|next| next.set(1));");
            writer.line("    SMELT_TIMER_NOW_MS.with(|now| now.set(0));");
            writer.line("    SMELT_TIMERS.with(|timers| timers.borrow_mut().clear());");
            writer.line("}");
            writer.blank_line();
            writer.line("fn smelt_set_timeout(callback: ::std::rc::Rc<::std::cell::RefCell<dyn FnMut() -> Result<(), Box<dyn std::error::Error>>>>, delay_ms: f64) -> SmeltUnknown {");
            writer.line("    let id = SMELT_NEXT_TIMER_ID.with(|next| { let id = next.get(); next.set(id.saturating_add(1)); id });");
            writer.line("    let delay_ms = if delay_ms.is_finite() && delay_ms > 0.0 { delay_ms as u64 } else { 0 };");
            writer.line("    let due_ms = SMELT_TIMER_NOW_MS.with(|now| now.get().saturating_add(delay_ms));");
            writer.line("    SMELT_TIMERS.with(|timers| timers.borrow_mut().push(SmeltTimer { id, due_ms, callback }));");
            writer.line("    SmeltUnknown::Number(id as f64)");
            writer.line("}");
            writer.blank_line();
            writer.line("fn smelt_clear_timeout<T: IntoSmeltUnknown>(handle: T) {");
            writer.line(
                "    let SmeltUnknown::Number(id) = handle.into_smelt_unknown() else { return; };",
            );
            writer.line("    let id = id as u64;");
            writer.line(
            "    SMELT_TIMERS.with(|timers| timers.borrow_mut().retain(|timer| timer.id != id));",
        );
            writer.line("}");
            writer.blank_line();
            writer.line("fn smelt_drain_due_timers() {");
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
            writer.line("        }");
            writer.line("    }");
            writer.line("}");
            writer.blank_line();
            writer.line("async fn smelt_sleep_ms(delay_ms: f64) {");
            writer.line("    let delay_ms = if delay_ms.is_finite() && delay_ms > 0.0 { delay_ms as u64 } else { 0 };");
            writer.line(
                "    SMELT_TIMER_NOW_MS.with(|now| now.set(now.get().saturating_add(delay_ms)));",
            );
            writer.line("    smelt_drain_due_timers();");
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
                    match_writer.line("Self::Null | Self::Bool(_) | Self::Number(_) | Self::Symbol(_) | Self::Function(_) => 0,");
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
                fn_writer.line("    Self::Null | Self::Symbol(_) | Self::Array(_) | Self::Function(_) => f64::NAN,");
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
                        match_writer.line("Self::Bool(value) => write!(formatter, \"{value}\"),");
                        match_writer.line("Self::Number(value) => write!(formatter, \"{value}\"),");
                        match_writer.line("Self::String(value) => formatter.write_str(value),");
                        match_writer.line("Self::Symbol(value) => formatter.write_str(value),");
                        match_writer.line("Self::Array(_) | Self::Object(_) => formatter.write_str(\"[object Object]\"),");
                        match_writer.line("Self::Function(_) => formatter.write_str(\"function () { [native code] }\"),");
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
                        match_writer.line("Self::Null | Self::Symbol(_) | Self::Array(_) | Self::Object(_) | Self::Function(_) => None,");
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
                    match_writer.line("SmeltUnknown::Bool(_) => 1,");
                    match_writer.line("SmeltUnknown::Number(_) => 2,");
                    match_writer.line("SmeltUnknown::String(_) => 3,");
                    match_writer.line("SmeltUnknown::Symbol(_) => 4,");
                    match_writer.line("SmeltUnknown::Array(_) => 5,");
                    match_writer.line("SmeltUnknown::Object(_) => 6,");
                    match_writer.line("SmeltUnknown::Function(_) => 7,");
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
                    fn_writer.line("match value { SmeltUnknown::Null => false, SmeltUnknown::Bool(value) => value, SmeltUnknown::Number(value) => value != 0.0 && !value.is_nan(), SmeltUnknown::String(value) => !value.is_empty(), SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) => true }");
                },
            );
        });
        writer.blank_line();
        writer.block("impl SmeltFromUnknown for f64", |impl_writer| {
            impl_writer.block(
                "fn smelt_from_unknown(value: SmeltUnknown) -> Self",
                |fn_writer| {
                    fn_writer.line("match value { SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") { Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN), SmeltUnknown::Bool(value) => if value { 1.0 } else { 0.0 }, SmeltUnknown::Null | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) => f64::NAN }");
                },
            );
        });
        writer.blank_line();
        writer.block("impl SmeltFromUnknown for i64", |impl_writer| {
            impl_writer.block(
                "fn smelt_from_unknown(value: SmeltUnknown) -> Self",
                |fn_writer| {
                    fn_writer.line("match value { SmeltUnknown::Number(value) => value as i64, SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") { Some(SmeltUnknown::Number(value)) => value as i64, _ => 0_i64 }, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN) as i64, SmeltUnknown::Bool(value) => if value { 1_i64 } else { 0_i64 }, SmeltUnknown::Null | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) => 0_i64 }");
                },
            );
        });
        writer.blank_line();
        writer.block("impl SmeltFromUnknown for String", |impl_writer| {
            impl_writer.block(
                "fn smelt_from_unknown(value: SmeltUnknown) -> Self",
                |fn_writer| {
                    fn_writer.line("match value { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () { [native code] }\".to_owned() }");
                },
            );
        });
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
                        "self.map_or(SmeltUnknown::Null, IntoSmeltUnknown::into_smelt_unknown)",
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
                    fn_writer.line("SmeltUnknown::Object(SmeltObject::new(self.into_iter().map(|(key, value)| { let key = match key.into_smelt_unknown() { SmeltUnknown::String(value) => value, SmeltUnknown::Symbol(value) => format!(\"__smelt_symbol:{value}\"), SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => \"null\".to_owned(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () { [native code] }\".to_owned() }; (key, value.into_smelt_unknown()) }).collect()))");
                });
            },
        );
        writer.blank_line();
        writer.block(
            "impl<K, T> IntoSmeltUnknown for SmeltRecord<K, T> where K: IntoSmeltUnknown + Eq + ::std::hash::Hash + Clone, T: IntoSmeltUnknown + Clone",
            |impl_writer| {
                impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                    fn_writer.line("SmeltUnknown::Object(SmeltObject::with_id(self.id, self.iter().into_iter().map(|(key, value)| { let key = match key.into_smelt_unknown() { SmeltUnknown::String(value) => value, SmeltUnknown::Symbol(value) => format!(\"__smelt_symbol:{value}\"), SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => \"null\".to_owned(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () { [native code] }\".to_owned() }; (key, value.into_smelt_unknown()) }).collect()))");
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
        writer.line("#[derive(Clone, Debug, PartialEq)]");
        writer.block("pub struct SmeltRegExp", |struct_writer| {
            struct_writer.line("source: String,");
            struct_writer.line("flags: String,");
            struct_writer.line("last_index: ::std::rc::Rc<::std::cell::RefCell<usize>>,");
        });
        writer.blank_line();
        writer.block("impl SmeltRegExp", |impl_writer| {
            impl_writer.line("/// Construct a JavaScript-like RegExp value with shared lastIndex state.");
            impl_writer.block("pub fn new(source: String, flags: String) -> Self", |fn_writer| {
                fn_writer.line("Self { source, flags, last_index: ::std::rc::Rc::new(::std::cell::RefCell::new(0)) }");
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
            impl_writer.line("/// Execute this RegExp and return a JavaScript-like match object.");
            impl_writer.block("pub fn exec(&self, haystack: &str) -> Option<SmeltUnknown>", |fn_writer| {
                fn_writer.line("let regex = self.compiled();");
                fn_writer.line("let start = if self.has_flag('g') || self.has_flag('y') { *self.last_index.borrow() } else { 0 };");
                fn_writer.line("let suffix = haystack.get(start..).unwrap_or(\"\");");
                fn_writer.line("let captures = regex.captures(suffix).ok().flatten()?;");
                fn_writer.line("let matched = captures.get(0)?;");
                fn_writer.line("if self.has_flag('y') && matched.start() != 0 { *self.last_index.borrow_mut() = 0; return None; }");
                fn_writer.line("if self.has_flag('g') || self.has_flag('y') { *self.last_index.borrow_mut() = start + matched.end(); }");
                fn_writer.line("let mut object = ::std::collections::HashMap::new();");
                fn_writer.line("for index in 0..captures.len() { if let Some(value) = captures.get(index) { object.insert(index.to_string(), SmeltUnknown::String(value.as_str().to_owned())); } else { object.insert(index.to_string(), SmeltUnknown::Null); } }");
                fn_writer.line("let mut groups = ::std::collections::HashMap::new();");
                fn_writer.line("for name in regex.capture_names().flatten() { let value = captures.name(name).map_or(SmeltUnknown::Null, |value| SmeltUnknown::String(value.as_str().to_owned())); groups.insert(name.to_owned(), value.clone()); let mut snake = String::new(); for (index, ch) in name.chars().enumerate() { if ch.is_ascii_uppercase() { if index > 0 { snake.push('_'); } snake.push(ch.to_ascii_lowercase()); } else { snake.push(ch); } } groups.insert(snake, value); }");
                fn_writer.line("object.insert(\"groups\".to_owned(), SmeltUnknown::Object(SmeltObject::new(groups)));");
                fn_writer.line("object.insert(\"index\".to_owned(), SmeltUnknown::Number((start + matched.start()) as f64));");
                fn_writer.line("object.insert(\"input\".to_owned(), SmeltUnknown::String(haystack.to_owned()));");
                fn_writer.line("Some(SmeltUnknown::Object(SmeltObject::new(object)))");
            });
            impl_writer.line("/// Return indexed match records for JavaScript String.prototype.matchAll.");
            impl_writer.block("pub fn match_all_indices(&self, haystack: &str) -> Vec<SmeltRecord<String, f64>>", |fn_writer| {
                fn_writer.line("let Some(regex) = self.try_compiled() else { return Vec::new(); };");
                fn_writer.line("regex.find_iter(haystack).filter_map(Result::ok).map(|matched| SmeltRecord::from([(\"index\".to_owned(), matched.start() as f64)])).collect::<Vec<_>>()");
            });
            impl_writer.line("/// Test this RegExp against a string with JavaScript lastIndex updates.");
            impl_writer.block("pub fn test(&self, haystack: &str) -> bool", |fn_writer| {
                fn_writer.line("self.exec(haystack).is_some()");
            });
        });
        writer.blank_line();
        writer.block("impl IntoSmeltUnknown for SmeltRegExp", |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                fn_writer.line(
                    "SmeltUnknown::Object(SmeltObject::new(::std::collections::HashMap::from([",
                );
                fn_writer.line("(\"source\".to_owned(), SmeltUnknown::String(self.source)),");
                fn_writer.line("(\"flags\".to_owned(), SmeltUnknown::String(self.flags)),");
                fn_writer.line("(\"__smelt_regexp\".to_owned(), SmeltUnknown::Bool(true)),");
                fn_writer.line("])))");
            });
        });
        writer.blank_line();
        writer.block("impl Default for SmeltRegExp", |impl_writer| {
            impl_writer.line("/// Construct a RegExp that matches the empty string.");
            impl_writer.block("fn default() -> Self", |fn_writer| {
                fn_writer.line("Self::new(String::new(), String::new())");
            });
        });
        writer.blank_line();
    }
    for class in &mir.classes {
        let name = class_name_text(mir, class)?;
        if !emitted_class_names.insert(name.clone()) {
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
        if has_function_field {
            writer.line("#[derive(Clone)]");
            writer.line("#[allow(dead_code)]");
        } else if needs_serde_json && class_is_json_serializable(mir, class) {
            writer.line("#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]");
        } else {
            writer.line("#[derive(Clone, Debug, Default)]");
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

    let mut out = writer.finish();

    let mut has_emitted_root_function = false;
    for function in &mir.functions {
        if matches!(
            function.origin,
            HirOrigin::ClassConstructor { .. } | HirOrigin::ClassMethod { .. }
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

/// Emit natural JSON serde support for `SmeltUnknown`.
fn emit_unknown_serde_impls(writer: &mut CodeWriter) {
    writer.block("impl serde::Serialize for SmeltUnknown", |impl_writer| {
        impl_writer.block(
            "fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer",
            |fn_writer| {
                fn_writer.block("match self", |match_writer| {
                    match_writer.line("Self::Null => serializer.serialize_none(),");
                    match_writer.line("Self::Bool(value) => serializer.serialize_bool(*value),");
                    match_writer.line("Self::Number(value) => serializer.serialize_f64(*value),");
                    match_writer.line("Self::String(value) => serializer.serialize_str(value),");
                    match_writer.line("Self::Symbol(value) => serializer.serialize_str(value),");
                    match_writer.line("Self::Array(values) => serde::Serialize::serialize(&values.values, serializer),");
                    match_writer.line("Self::Object(values) => serde::Serialize::serialize(&values.iter().collect::<::std::collections::HashMap<_, _>>(), serializer),");
                    match_writer.line("Self::Function(_) => serializer.serialize_str(\"function () { [native code] }\"),");
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
                fn_writer.line("Ok(smelt_unknown_from_json_value(value))");
            },
        );
    });
    writer.blank_line();
    writer.block(
        "fn smelt_unknown_from_json_value(value: serde_json::Value) -> SmeltUnknown",
        |fn_writer| {
            fn_writer.block("match value", |match_writer| {
                match_writer.line("serde_json::Value::Null => SmeltUnknown::Null,");
                match_writer.line("serde_json::Value::Bool(value) => SmeltUnknown::Bool(value),");
                match_writer.line("serde_json::Value::Number(value) => SmeltUnknown::Number(value.as_f64().unwrap_or_default()),");
                match_writer.line("serde_json::Value::String(value) => SmeltUnknown::String(value),");
                match_writer.line("serde_json::Value::Array(values) => SmeltUnknown::Array(values.into_iter().map(smelt_unknown_from_json_value).collect()),");
                match_writer.line("serde_json::Value::Object(values) => SmeltUnknown::Object(SmeltObject::new(values.into_iter().map(|(key, value)| (key, smelt_unknown_from_json_value(value))).collect())),");
            });
        },
    );
    writer.blank_line();
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
            format!("{value_text}.map_or(SmeltUnknown::Null, |value| {inner_text})")
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

#[cfg(test)]
/// Tests for the code generator.
#[cfg(test)]
mod tests;
