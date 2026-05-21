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

use std::{collections::HashMap, fs, path::Path};

use smelt_hir::{BodyId, Type, TypeId};
use smelt_mir::{HirOrigin, Mir, MirFunction};

pub(crate) mod classes;
pub(crate) mod deps;
pub mod rust;
pub(crate) mod stdlib;

use deps::GeneratedDep;
mod emitter;
use classes::{
    class_impl_generics_text, class_name_text, class_type_args_text, class_type_params_text,
    effective_class_fields, effective_interface_fields, inherited_trait_methods,
    interface_type_params_text,
};
use emitter::{EmitContext, FunctionEmitter};
use rust::{CodeWriter, RustIdent};

/// Options for controlling code emission behavior.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct EmitOptions {
    /// The name of the Rust crate to generate.
    pub crate_name: String,
}

impl Default for EmitOptions {
    /// Returns the default emission options with crate name "smelt_app".
    fn default() -> Self {
        Self {
            crate_name: "smelt_app".to_owned(),
        }
    }
}

impl EmitOptions {
    /// Creates emission options for the given Rust crate name.
    pub fn new(crate_name: impl Into<String>) -> Self {
        Self {
            crate_name: crate_name.into(),
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
/// Creates the crate structure with Cargo.toml and main.rs files.
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
    write_if_changed(src_dir.join("main.rs"), &emit_source(mir)?)?;
    Ok(())
}

/// Emits a complete Rust crate while preserving the source module layout.
///
/// Shared generated runtime/types stay in `main.rs`. Non-entry module-level
/// functions are moved into source-shaped Rust modules and re-exported from the
/// crate root so existing flat-name call emission continues to resolve.
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
    write_if_changed(src_dir.join("main.rs"), &mapped.root)?;
    for module in mapped.modules {
        let module_path = src_dir.join(format!("{}.rs", module.name));
        write_if_changed(module_path, &module.source)?;
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

/// Emits Rust source code from the given MIR.
///
/// Returns a string containing the complete source code for the MIR, including
/// struct definitions, free functions, and impl blocks for methods.
pub fn emit_source(mir: &Mir) -> Result<String, EmitError> {
    let mut writer = CodeWriter::new();
    let needs_serde_json =
        stdlib::backend_dependencies(mir).contains(&smelt_stdlib::BackendDependency::SerdeJson);
    let needs_regex =
        stdlib::backend_dependencies(mir).contains(&smelt_stdlib::BackendDependency::Regex);
    writer.line("// @generated by smelt. Do not edit by hand.");
    writer.line("#![allow(dead_code, non_snake_case, unused_imports, unused_variables)]");
    writer.blank_line();
    if stdlib::needs_unknown_type(mir) {
        writer.block("pub enum SmeltUnknown", |unknown_writer| {
            unknown_writer.line("Null,");
            unknown_writer.line("Bool(bool),");
            unknown_writer.line("Number(f64),");
            unknown_writer.line("String(String),");
            unknown_writer.line("Array(Vec<SmeltUnknown>),");
            unknown_writer.line("Object(::std::collections::HashMap<String, SmeltUnknown>),");
            unknown_writer.line("Function(::std::rc::Rc<::std::cell::RefCell<dyn FnMut(Vec<SmeltUnknown>) -> SmeltUnknown>>),");
        });
        writer.blank_line();
        writer.block("impl Clone for SmeltUnknown", |impl_writer| {
            impl_writer.block("fn clone(&self) -> Self", |fn_writer| {
                fn_writer.block("match self", |match_writer| {
                    match_writer.line("Self::Null => Self::Null,");
                    match_writer.line("Self::Bool(value) => Self::Bool(*value),");
                    match_writer.line("Self::Number(value) => Self::Number(*value),");
                    match_writer.line("Self::String(value) => Self::String(value.clone()),");
                    match_writer.line("Self::Array(values) => Self::Array(values.clone()),");
                    match_writer.line("Self::Object(values) => Self::Object(values.clone()),");
                    match_writer.line("Self::Function(value) => Self::Function(value.clone()),");
                });
            });
        });
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
                fn_writer.block("match (self, other)", |match_writer| {
                    match_writer.line("(Self::Null, Self::Null) => true,");
                    match_writer.line("(Self::Bool(left), Self::Bool(right)) => left == right,");
                    match_writer.line("(Self::Number(left), Self::Number(right)) => left == right,");
                    match_writer.line("(Self::String(left), Self::String(right)) => left == right,");
                    match_writer.line("(Self::Array(left), Self::Array(right)) => left == right,");
                    match_writer.line("(Self::Object(left), Self::Object(right)) => left == right,");
                    match_writer.line("(Self::Function(left), Self::Function(right)) => ::std::rc::Rc::ptr_eq(left, right),");
                    match_writer.line("_ => false,");
                });
            });
        });
        writer.blank_line();
        writer.block("impl Default for SmeltUnknown", |impl_writer| {
            impl_writer.block("fn default() -> Self", |fn_writer| {
                fn_writer.line("Self::Null");
            });
        });
        writer.blank_line();
        writer.block("impl SmeltUnknown", |impl_writer| {
            impl_writer.line("/// Returns the JavaScript-style length for unknown string, array, and object values.");
            impl_writer.block("pub fn len(&self) -> usize", |fn_writer| {
                fn_writer.block("match self", |match_writer| {
                    match_writer.line("Self::String(value) => value.chars().count(),");
                    match_writer.line("Self::Array(value) => value.len(),");
                    match_writer.line("Self::Object(value) => value.len(),");
                    match_writer.line("Self::Null | Self::Bool(_) | Self::Number(_) | Self::Function(_) => 0,");
                });
            });
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
                    fn_writer.block("match self", |match_writer| {
                        match_writer.line("Self::Null => 0_u8.hash(state),");
                        match_writer.line("Self::Bool(value) => { 1_u8.hash(state); value.hash(state); }");
                        match_writer.line("Self::Number(value) => { 2_u8.hash(state); value.to_bits().hash(state); }");
                        match_writer.line("Self::String(value) => { 3_u8.hash(state); value.hash(state); }");
                        match_writer.line("Self::Array(values) => { 4_u8.hash(state); values.hash(state); }");
                        match_writer.line("Self::Object(values) => { 5_u8.hash(state); let mut entries = values.iter().collect::<Vec<_>>(); entries.sort_by(|left, right| left.0.cmp(right.0)); for (key, value) in entries { key.hash(state); value.hash(state); } }");
                        match_writer.line("Self::Function(_) => 6_u8.hash(state),");
                    });
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
                        match_writer.line("(Self::Null, Self::Null) => Some(::std::cmp::Ordering::Equal),");
                        match_writer.line("(left, right) => Some(smelt_unknown_rank(left).cmp(&smelt_unknown_rank(right))),");
                    });
                },
            );
        });
        writer.blank_line();
        writer.block(
            "fn smelt_unknown_rank(value: &SmeltUnknown) -> u8",
            |fn_writer| {
                fn_writer.block("match value", |match_writer| {
                    match_writer.line("SmeltUnknown::Null => 0,");
                    match_writer.line("SmeltUnknown::Bool(_) => 1,");
                    match_writer.line("SmeltUnknown::Number(_) => 2,");
                    match_writer.line("SmeltUnknown::String(_) => 3,");
                    match_writer.line("SmeltUnknown::Array(_) => 4,");
                    match_writer.line("SmeltUnknown::Object(_) => 5,");
                    match_writer.line("SmeltUnknown::Function(_) => 6,");
                });
            },
        );
        writer.blank_line();
        writer.block("trait IntoSmeltUnknown", |trait_writer| {
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
            "impl<A: IntoSmeltUnknown, B: IntoSmeltUnknown> IntoSmeltUnknown for (A, B)",
            |impl_writer| {
                impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                    fn_writer.line(
                        "SmeltUnknown::Array(vec![self.0.into_smelt_unknown(), self.1.into_smelt_unknown()])",
                    );
                });
            },
        );
        writer.blank_line();
        writer.block(
            "impl<K, T> IntoSmeltUnknown for ::std::collections::HashMap<K, T> where K: IntoSmeltUnknown + Eq + ::std::hash::Hash, T: IntoSmeltUnknown",
            |impl_writer| {
                impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                    fn_writer.line("SmeltUnknown::Object(self.into_iter().map(|(key, value)| { let key = match key.into_smelt_unknown() { SmeltUnknown::String(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => \"null\".to_owned(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () { [native code] }\".to_owned() }; (key, value.into_smelt_unknown()) }).collect())");
                });
            },
        );
        writer.blank_line();
        if needs_serde_json {
            emit_unknown_serde_impls(&mut writer);
        }
    }

    let mut emitted_class_names = std::collections::HashSet::new();
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
                    .map(|name| RustIdent::new(name).into_string())
                    .ok_or_else(|| EmitError::new("interface type parameter has unknown symbol"))
            })
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        writer.block(format!("struct {name}{type_params}"), |block_writer| {
            for field in &fields {
                let field_name = RustIdent::new(
                    mir.symbols
                        .get(field.name)
                        .ok_or_else(|| EmitError::new("field has unknown symbol"))
                        .unwrap_or("field"),
                )
                .into_string();
                let field_ty = FunctionEmitter::type_text_for(mir, field.ty)
                    .unwrap_or_else(|_| "SmeltUnknown".to_owned());
                block_writer.line(format!("{field_name}: {field_ty},"));
            }
            if !interface.type_params.is_empty() {
                block_writer.line(format!(
                    "_smelt_phantom: ::std::marker::PhantomData<({phantom_args})>,"
                ));
            }
        });
        writer.blank_line();
    }
    if needs_regex {
        writer.line("#[derive(Clone, Debug)]");
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
            impl_writer.block("fn compiled(&self) -> regex::Regex", |fn_writer| {
                fn_writer.line("let mut prefix = String::new();");
                fn_writer.line("if self.has_flag('i') { prefix.push('i'); }");
                fn_writer.line("if self.has_flag('m') { prefix.push('m'); }");
                fn_writer.line("if self.has_flag('s') { prefix.push('s'); }");
                fn_writer.line("let pattern = if prefix.is_empty() { self.source.clone() } else { format!(\"(?{prefix}){}\", self.source) };");
                fn_writer.line("regex::Regex::new(&pattern).expect(\"regex compile failed\")");
            });
            impl_writer.line("/// Execute this RegExp and return a JavaScript-like match object.");
            impl_writer.block("pub fn exec(&self, haystack: &str) -> Option<SmeltUnknown>", |fn_writer| {
                fn_writer.line("let regex = self.compiled();");
                fn_writer.line("let start = if self.has_flag('g') || self.has_flag('y') { *self.last_index.borrow() } else { 0 };");
                fn_writer.line("let suffix = haystack.get(start..).unwrap_or(\"\");");
                fn_writer.line("let captures = regex.captures(suffix)?;");
                fn_writer.line("let matched = captures.get(0)?;");
                fn_writer.line("if self.has_flag('y') && matched.start() != 0 { *self.last_index.borrow_mut() = 0; return None; }");
                fn_writer.line("if self.has_flag('g') || self.has_flag('y') { *self.last_index.borrow_mut() = start + matched.end(); }");
                fn_writer.line("let mut object = ::std::collections::HashMap::new();");
                fn_writer.line("for index in 0..captures.len() { if let Some(value) = captures.get(index) { object.insert(index.to_string(), SmeltUnknown::String(value.as_str().to_owned())); } else { object.insert(index.to_string(), SmeltUnknown::Null); } }");
                fn_writer.line("let mut groups = ::std::collections::HashMap::new();");
                fn_writer.line("for name in regex.capture_names().flatten() { let value = captures.name(name).map_or(SmeltUnknown::Null, |value| SmeltUnknown::String(value.as_str().to_owned())); groups.insert(name.to_owned(), value.clone()); let mut snake = String::new(); for (index, ch) in name.chars().enumerate() { if ch.is_ascii_uppercase() { if index > 0 { snake.push('_'); } snake.push(ch.to_ascii_lowercase()); } else { snake.push(ch); } } groups.insert(snake, value); }");
                fn_writer.line("object.insert(\"groups\".to_owned(), SmeltUnknown::Object(groups));");
                fn_writer.line("object.insert(\"index\".to_owned(), SmeltUnknown::Number((start + matched.start()) as f64));");
                fn_writer.line("object.insert(\"input\".to_owned(), SmeltUnknown::String(haystack.to_owned()));");
                fn_writer.line("Some(SmeltUnknown::Object(object))");
            });
            impl_writer.line("/// Test this RegExp against a string with JavaScript lastIndex updates.");
            impl_writer.block("pub fn test(&self, haystack: &str) -> bool", |fn_writer| {
                fn_writer.line("self.exec(haystack).is_some()");
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
        let _inherited_trait_methods = inherited_trait_methods(mir, class);
        let mut field_lines = Vec::new();
        let fields = effective_class_fields(mir, class);
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
        for field in fields {
            field_lines.push(format!(
                "{}: {},",
                RustIdent::new(
                    mir.symbols
                        .get(field.name)
                        .ok_or_else(|| EmitError::new("field has unknown symbol"))?
                ),
                FunctionEmitter::type_text_for(mir, field.ty)?
            ));
        }
        if !class.type_params.is_empty() {
            let phantom_args = class
                .type_params
                .iter()
                .map(|param| {
                    mir.symbols
                        .get(param.name)
                        .map(|name| RustIdent::new(name).into_string())
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
        writer.blank_line();
    }

    let context = EmitContext::new(mir)?;
    let mut out = writer.finish();

    for (idx, function) in mir.functions.iter().enumerate() {
        if matches!(
            function.origin,
            HirOrigin::ClassConstructor { .. } | HirOrigin::ClassMethod { .. }
        ) {
            continue;
        }
        if idx > 0 || !mir.classes.is_empty() {
            out.push('\n');
        }
        let mut emitter = FunctionEmitter::new(mir, &context, function)?;
        emitter.emit(&mut out)?;
    }

    let mut emitted_impl_names = std::collections::HashSet::new();
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

/// Emits source text split across Rust module files.
fn emit_mapped_sources(
    mir: &Mir,
    krate: &smelt_hir::Crate,
    modules: &[(String, smelt_hir::ModuleId)],
) -> Result<MappedSources, EmitError> {
    let mut root = emit_source(mir)?;
    let body_modules = body_module_names(krate, modules);
    let context = EmitContext::new(mir)?;
    let mut module_chunks = HashMap::<String, Vec<String>>::new();

    for function in &mir.functions {
        let HirOrigin::Body(body) = function.origin else {
            continue;
        };
        if is_root_main_function(mir, function, context.none_ty) {
            continue;
        }
        let Some(module_name) = body_modules.get(&body).cloned() else {
            continue;
        };
        let mut emitted = String::new();
        FunctionEmitter::new(mir, &context, function)?.emit(&mut emitted)?;
        if let Some(position) = root.find(&emitted) {
            root.replace_range(position..position + emitted.len(), "");
            module_chunks
                .entry(module_name)
                .or_default()
                .push(publicize_free_function(emitted));
        }
    }

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
        root = insert_after_crate_header(root, &format!("{declarations}\n\n"));
    }

    let modules = module_names
        .into_iter()
        .map(|name| {
            let chunks = module_chunks.remove(&name).unwrap_or_default();
            MappedModuleSource {
                name,
                source: format!(
                    "// @generated by smelt. Do not edit by hand.\n#![allow(dead_code, non_snake_case, unused_imports, unused_variables)]\n\nuse super::*;\n\n{}",
                    chunks.join("\n")
                ),
            }
        })
        .collect();

    Ok(MappedSources { root, modules })
}

/// Builds a body-to-Rust-module map from HIR module ownership metadata.
fn body_module_names(
    krate: &smelt_hir::Crate,
    modules: &[(String, smelt_hir::ModuleId)],
) -> HashMap<BodyId, String> {
    let mut names = HashMap::new();
    for (_path, module_id) in modules {
        let Some(module) = krate.modules.get(module_id.0 as usize) else {
            continue;
        };
        let rust_module = source_module_name(&module.name);
        if let Some(body) = module.body {
            names.insert(body, rust_module.clone());
        }
        for item in &module.items {
            if let Some(smelt_hir::Item::Function(function)) = krate.items.get(item.0 as usize)
                && let Some(body) = function.body
            {
                names.insert(body, rust_module.clone());
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
        output.replace_range(index + 1..index + 4, "pub(crate) fn ");
        return output;
    }
    source
}

/// Inserts module declarations after the generated header and crate attributes.
fn insert_after_crate_header(mut root: String, text: &str) -> String {
    let marker = "#![allow(dead_code, non_snake_case, unused_imports, unused_variables)]\n\n";
    if let Some(index) = root.find(marker) {
        root.insert_str(index + marker.len(), text);
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
                    match_writer.line("Self::Array(values) => serde::Serialize::serialize(values, serializer),");
                    match_writer.line("Self::Object(values) => serde::Serialize::serialize(values, serializer),");
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
                match_writer.line("serde_json::Value::Object(values) => SmeltUnknown::Object(values.into_iter().map(|(key, value)| (key, smelt_unknown_from_json_value(value))).collect()),");
            });
        },
    );
    writer.blank_line();
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

/// Collects the dependency list required by generated Rust code.
fn generated_deps(mir: &Mir) -> Vec<GeneratedDep> {
    let mut deps = Vec::new();
    if stdlib::needs_tokio(mir) {
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
