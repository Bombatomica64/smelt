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

use std::{fs, path::Path};

use smelt_hir::{Type, TypeId};
use smelt_mir::{HirOrigin, Mir};

pub(crate) mod classes;
pub(crate) mod deps;
pub mod rust;
pub(crate) mod stdlib;

use deps::GeneratedDep;
mod emitter;
use classes::{
    class_impl_generics_text, class_name_text, class_type_args_text, class_type_params_text,
    effective_class_fields, inherited_trait_methods,
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
    fs::write(
        output_dir.join("Cargo.toml"),
        deps::cargo_toml(&options.crate_name, &generated_deps(mir)),
    )?;
    fs::write(src_dir.join("main.rs"), emit_source(mir)?)?;
    Ok(())
}

/// Emits Rust source code from the given MIR.
///
/// Returns a string containing the complete source code for the MIR, including
/// struct definitions, free functions, and impl blocks for methods.
pub fn emit_source(mir: &Mir) -> Result<String, EmitError> {
    let mut writer = CodeWriter::new();
    writer.line("// @generated by smelt. Do not edit by hand.");
    writer.line("#![allow(dead_code, unused_variables)]");
    writer.blank_line();
    if stdlib::needs_unknown_type(mir) {
        writer.line("#[derive(Clone, Debug, PartialEq)]");
        writer.block("pub enum SmeltUnknown", |unknown_writer| {
            unknown_writer.line("Null,");
            unknown_writer.line("Bool(bool),");
            unknown_writer.line("Number(f64),");
            unknown_writer.line("String(String),");
            unknown_writer.line("Array(Vec<SmeltUnknown>),");
            unknown_writer.line("Object(::std::collections::HashMap<String, SmeltUnknown>),");
        });
        writer.blank_line();
        writer.block("impl Default for SmeltUnknown", |impl_writer| {
            impl_writer.block("fn default() -> Self", |fn_writer| {
                fn_writer.line("Self::Null");
            });
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
                    fn_writer.line("SmeltUnknown::Object(self.into_iter().map(|(key, value)| { let key = match key.into_smelt_unknown() { SmeltUnknown::String(value) => value, SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => \"null\".to_owned(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned() }; (key, value.into_smelt_unknown()) }).collect())");
                });
            },
        );
        writer.blank_line();
    }

    let mut emitted_class_names = std::collections::HashSet::new();
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
            writer.line("#[allow(dead_code)]");
        } else {
            writer.line("#[derive(Clone, Debug)]");
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
