//! Pretty-printing utilities for MIR structures.
//!
//! This module provides functions to format MIR in a human-readable compact format,
//! suitable for debugging and testing.

use std::fmt::Write as _;

use smelt_hir::op_table::{opt, regex_replace_args_text, tail};
use smelt_hir::{Type, TypeId};

use crate::{
    BuiltinFn, Callee, Constant, GlobalProjection, LocalId, LocalKind, Mir, Operand, Place,
    Rvalue, Statement,
    Terminator,
};

/// Append formatted text to the output buffer.
///
/// Writing to a `String` is infallible, so the `Result` from `write_fmt` is
/// deliberately ignored. This mirrors the HIR formatter's `push_fmt` helper and
/// lets the formatters use `write!`/`writeln!`-style assembly without tripping
/// the workspace `unwrap_used`/`expect_used` lints.
fn push_fmt(out: &mut String, args: std::fmt::Arguments<'_>) {
    let _ignored = out.write_fmt(args);
}

/// Formats a MIR module into a human-readable compact string representation.
#[must_use]
pub fn format_compact(mir: &Mir) -> String {
    let mut out = String::new();
    for class in &mir.classes {
        let name = mir.symbols.get(class.name).unwrap_or("<unknown>");
        let fields = class
            .fields
            .iter()
            .map(|field| {
                format!(
                    "{:?} {}: {}",
                    field.visibility,
                    mir.symbols.get(field.name).unwrap_or("<unknown>"),
                    type_ref(mir, field.ty)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        push_fmt(
            &mut out,
            format_args!(
                "class {name} fields [{fields}] constructor {:?} methods {:?}\n",
                class.constructor, class.methods
            ),
        );
    }
    for interface in &mir.interfaces {
        let name = mir.symbols.get(interface.name).unwrap_or("<unknown>");
        push_fmt(&mut out, format_args!("interface {name}\n"));
    }
    if !mir.classes.is_empty() || !mir.interfaces.is_empty() {
        out.push('\n');
    }
    for function in &mir.functions {
        let name = mir.symbols.get(function.name).unwrap_or("<unknown>");
        let async_prefix = if function.is_async { "async " } else { "" };
        push_fmt(
            &mut out,
            format_args!(
                "{async_prefix}fn {name} ({:?}) -> {}{}\n",
                function.id,
                type_ref(mir, function.return_ty),
                if function.can_throw { " throws" } else { "" }
            ),
        );

        if !function.locals.is_empty() {
            out.push_str("  locals\n");
            for (idx, local) in function.locals.iter().enumerate() {
                let local_id = index_to_u32(idx, "MIR local index");
                push_fmt(
                    &mut out,
                    format_args!(
                        "    {} {}: {}\n",
                        local_ref(LocalId(local_id)),
                        local_kind_text(mir, local.kind),
                        type_ref(mir, local.ty)
                    ),
                );
            }
        }

        for block in &function.blocks {
            push_fmt(&mut out, format_args!("  bb{}:\n", block.id.0));
            for statement in &block.statements {
                push_fmt(&mut out, format_args!("    {}\n", statement_text(statement)));
            }
            let terminator = block
                .terminator
                .as_ref()
                .map_or_else(|| "<missing terminator>".to_owned(), terminator_text);
            push_fmt(&mut out, format_args!("    {terminator}\n"));
        }
        out.push('\n');
    }
    for closure in &mir.closures {
        push_fmt(
            &mut out,
            format_args!(
                "closure {:?} -> {}{}\n",
                closure.id,
                type_ref(mir, closure.return_ty),
                if closure.can_throw { " throws" } else { "" }
            ),
        );
        if !closure.locals.is_empty() {
            out.push_str("  locals\n");
            for (idx, local) in closure.locals.iter().enumerate() {
                let local_id = index_to_u32(idx, "MIR closure local index");
                push_fmt(
                    &mut out,
                    format_args!(
                        "    {} {}: {}\n",
                        local_ref(LocalId(local_id)),
                        local_kind_text(mir, local.kind),
                        type_ref(mir, local.ty)
                    ),
                );
            }
        }
        for block in &closure.blocks {
            push_fmt(&mut out, format_args!("  bb{}:\n", block.id.0));
            for statement in &block.statements {
                push_fmt(&mut out, format_args!("    {}\n", statement_text(statement)));
            }
            let terminator = block
                .terminator
                .as_ref()
                .map_or_else(|| "<missing terminator>".to_owned(), terminator_text);
            push_fmt(&mut out, format_args!("    {terminator}\n"));
        }
        out.push('\n');
    }
    out
}

/// Formats a local kind to human-readable text.
fn local_kind_text(mir: &Mir, kind: LocalKind) -> String {
    match kind {
        LocalKind::Param { symbol } => symbol
            .and_then(|param_symbol| mir.symbols.get(param_symbol))
            .map_or_else(|| "param".to_owned(), |name| format!("param {name}")),
        LocalKind::Temp => "temp".to_owned(),
        LocalKind::UserBinding(symbol) => {
            format!("user {}", mir.symbols.get(symbol).unwrap_or("<unknown>"))
        }
    }
}

/// Formats a statement to human-readable text.
fn statement_text(statement: &Statement) -> String {
    match statement {
        Statement::Assign { dest, value } => {
            format!("{} = {}", local_ref(*dest), rvalue_text(value))
        }
        Statement::AssignPlace { place, value } => {
            format!("{} = {}", place_text(place), rvalue_text(value))
        }
        // `entry_update %d[key] ?? default as %cur = <rvalue>` reads as "bind the
        // entry, seeding it with `default` when absent, then store the rvalue
        // back into the same entry" — the one-probe read-modify-write formed by
        // `crate::opt::DictEntryUpdate`.
        Statement::DictEntryUpdate {
            base,
            index,
            default,
            current,
            value,
        } => format!(
            "entry_update {}[{}] ?? {} as {} = {}",
            local_ref(*base),
            operand_text(index),
            operand_text(default),
            local_ref(*current),
            rvalue_text(value)
        ),
    }
}

/// Rebinds one op field to its text; see the display grammar in
/// `smelt_hir::op_table`.
macro_rules! render_op_field {
    (one, $field:ident) => { operand_text($field) };
    (opt, $field:ident) => { $field.as_ref().map(operand_text) };
    (many, $field:ident) => { $field.iter().map(operand_text).collect::<Vec<_>>() };
    (splice, $field:ident) => {
        $field
            .iter()
            .map(|item| {
                let value = operand_text(&item.value);
                if item.spread { format!("...{value}") } else { value }
            })
            .collect::<Vec<_>>()
    };
    (symbol_pairs, $field:ident) => {
        $field
            .iter()
            .map(|(name, value)| format!("{name:?}: {}", operand_text(value)))
            .collect::<Vec<_>>()
    };
    ({ $key:ty }, $field:ident) => {
        $field
            .iter()
            .map(|(key, value)| (key, operand_text(value)))
            .collect::<Vec<_>>()
    };
    ([$data:ty], $field:ident) => { $field };
    ((sym $label:literal), $field:ident) => { format!("{}{}", $label, $field.0) };
    ((ty), $field:ident) => { format!("t{}", $field.0) };
}

/// Generates [`rvalue_text`]: the arms of the forms only MIR has, then one
/// generated arm per op-table entry.
macro_rules! define_rvalue_text {
    ($(
        $(#[$meta:meta])*
        $name:ident $({ $($(#[$field_meta:meta])* $field:ident : $kind:tt),* $(,)? })?
        => $display:tt;
    )*) => {
        /// Formats an rvalue to human-readable text.
        fn rvalue_text(rvalue: &Rvalue) -> String {
            match rvalue {
                Rvalue::Use(operand) => operand_text(operand),
                Rvalue::GeneratorYield {
                    value,
                    unwind,
                    cleanup,
                } => {
                    let mut text = format!("yield {}", operand_text(value));
                    if let Some(handler) = unwind {
                        push_fmt(&mut text, format_args!(" unwind bb{}", handler.catch_block.0));
                    }
                    if let Some(cleanup) = cleanup {
                        push_fmt(&mut text, format_args!(" cleanup bb{}", cleanup.block.0));
                    }
                    text
                }
                Rvalue::List(items) => format!(
                    "[{}]",
                    items
                        .iter()
                        .map(operand_text)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                Rvalue::Set(items) => format!(
                    "set{{{}}}",
                    items
                        .iter()
                        .map(operand_text)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                Rvalue::Dict(entries) => format!(
                    "{{{}}}",
                    entries
                        .iter()
                        .map(|(key, entry_value)| {
                            format!("{}: {}", operand_text(key), operand_text(entry_value))
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                Rvalue::Tuple(items) => format!(
                    "({})",
                    items
                        .iter()
                        .map(operand_text)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                Rvalue::Closure { id, captures } => format!(
                    "closure {:?} [{}]",
                    id,
                    captures
                        .iter()
                        .map(operand_text)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                Rvalue::Binary { op, lhs, rhs } => {
                    format!(
                        "{} {} {}",
                        operand_text(lhs),
                        smelt_hir::bin_op_text(*op),
                        operand_text(rhs)
                    )
                }
                Rvalue::Unary { op, operand } => {
                    let op_text = match op {
                        smelt_hir::UnaryOp::Not => "!",
                        smelt_hir::UnaryOp::Neg => "-",
                    };
                    format!("{op_text}{}", operand_text(operand))
                }
                Rvalue::UnionMethod {
                    receiver,
                    method,
                    args,
                } => {
                    let args_text = args.iter().map(operand_text).collect::<Vec<_>>().join(", ");
                    format!(
                        "union_method {}.method{}({args_text})",
                        operand_text(receiver),
                        method.0
                    )
                }
                Rvalue::BoundMethod {
                    receiver,
                    bound,
                    method,
                    args,
                } => {
                    let args_text = args.iter().map(operand_text).collect::<Vec<_>>().join(", ");
                    format!(
                        "bound_method<{}> {}.method{}({args_text})",
                        bound.0,
                        operand_text(receiver),
                        method.0
                    )
                }
                Rvalue::BoundField {
                    receiver,
                    bound,
                    field,
                } => format!(
                    "bound_field<{}> {}.field{}",
                    bound.0,
                    operand_text(receiver),
                    field.0
                ),
                Rvalue::Struct { class, fields } => {
                    let field_list = fields
                        .iter()
                        .map(|(field, field_value)| {
                            format!("field{}: {}", field.0, operand_text(field_value))
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("struct{} {{{field_list}}}", class.0)
                }
                Rvalue::ExternalClassInstance { class, args } => {
                    let arg_list = args.iter().map(operand_text).collect::<Vec<_>>().join(", ");
                    format!("external_new{}({arg_list})", class.0)
                }
                Rvalue::GlobalGet { global } => format!("global_get #{global}"),
                Rvalue::GlobalSet { global, value: stored } => {
                    format!("global_set #{global} = {}", operand_text(stored))
                }
                Rvalue::Await(operand) => format!("await {}", operand_text(operand)),
                $(
                    Rvalue::$name $({ $($field),* })? => {
                        $($(let $field = render_op_field!($kind, $field);)*)?
                        smelt_hir::op_mir_display!($display)
                    }
                )*
            }
        }
    };
}

smelt_hir::for_each_op!(define_rvalue_text);

/// Formats a terminator to human-readable text.
fn terminator_text(terminator: &Terminator) -> String {
    match terminator {
        Terminator::Goto(target) => format!("goto bb{}", target.0),
        Terminator::Call {
            callee,
            args,
            dest,
            target,
            unwind,
        } => {
            let arg_list = args.iter().map(operand_text).collect::<Vec<_>>().join(", ");
            let unwind_text = unwind.map_or_else(String::new, |handler| {
                format!(" unwind bb{}", handler.catch_block.0)
            });
            format!(
                "{} = call {}({}) -> bb{}{}",
                local_ref(*dest),
                callee_text(callee),
                arg_list,
                target.0,
                unwind_text
            )
        }
        Terminator::Await {
            future,
            dest,
            target,
            unwind,
        } => {
            let unwind_text = unwind.map_or_else(String::new, |handler| {
                format!(" unwind bb{}", handler.catch_block.0)
            });
            format!(
                "{} = await {} -> bb{}{}",
                local_ref(*dest),
                operand_text(future),
                target.0,
                unwind_text
            )
        }
        Terminator::Switch {
            cond,
            then_block,
            else_block,
        } => format!(
            "switch {} ? bb{} : bb{}",
            operand_text(cond),
            then_block.0,
            else_block.0
        ),
        Terminator::Match {
            scrutinee,
            arms,
            default,
        } => {
            let arm_list = arms
                .iter()
                .map(|arm| format!("{} => bb{}", constant_text(&arm.label), arm.target.0))
                .collect::<Vec<_>>()
                .join(", ");
            let default_suffix = default
                .map(|target| {
                    if arm_list.is_empty() {
                        format!("_ => bb{}", target.0)
                    } else {
                        format!(", _ => bb{}", target.0)
                    }
                })
                .unwrap_or_default();
            format!(
                "match {} {{{}{}}}",
                operand_text(scrutinee),
                arm_list,
                default_suffix
            )
        }
        Terminator::Return(operand) => format!("return {}", operand_text(operand)),
        Terminator::Throw(operand) => format!("throw {}", operand_text(operand)),
    }
}

/// Formats a callee to human-readable text.
fn callee_text(callee: &Callee) -> String {
    match callee {
        Callee::Static(func) => format!("fn{}", func.0),
        Callee::BaseInit(func) => format!("init fn{}", func.0),
        Callee::Indirect(operand) => operand_text(operand),
        // The absent spelling is part of the callee but not part of the MIR
        // dump: it is a property of the site's language, and printing it would
        // churn every fixture that logs an optional without telling a reader
        // anything the file path does not already say.
        Callee::Builtin(BuiltinFn::ConsoleLog { .. }) => "@console_log".to_owned(),
        Callee::Builtin(BuiltinFn::ConsoleWrite) => "@console_write".to_owned(),
        Callee::Builtin(BuiltinFn::ConsoleErrorWrite) => "@console_error_write".to_owned(),
        Callee::Builtin(BuiltinFn::JsonParse { target: None }) => "@json_parse".to_owned(),
        Callee::Builtin(BuiltinFn::JsonParse { target: Some(_) }) => "@json_decode".to_owned(),
        Callee::Builtin(BuiltinFn::Base64(op)) => format!("@{}", op.name()),
        Callee::Builtin(BuiltinFn::RegExpCompile) => "@regexp_compile".to_owned(),
        Callee::Builtin(BuiltinFn::UrlNew) => "@url_new".to_owned(),
        Callee::Builtin(BuiltinFn::UrlSetHref) => "@url_set_href".to_owned(),
        Callee::Builtin(BuiltinFn::BigInt(op)) => format!("@{}", op.name()),
        Callee::Builtin(BuiltinFn::HostModule(op)) => format!("@{}", op.name()),
        Callee::Builtin(BuiltinFn::DataViewAccess { write, element }) => format!(
            "@data_view_{}{}",
            if *write { "set" } else { "get" },
            element.element_name()
        ),
        Callee::Builtin(BuiltinFn::UriDecode(op)) => match op {
            smelt_hir::UriTranscodeOp::Decode => "@decode_uri".to_owned(),
            smelt_hir::UriTranscodeOp::DecodeComponent => "@decode_uri_component".to_owned(),
            // The encoding direction is infallible and never becomes a callee.
            smelt_hir::UriTranscodeOp::Encode
            | smelt_hir::UriTranscodeOp::EncodeComponent => "@encode_uri".to_owned(),
        },
    }
}

/// Formats an operand to human-readable text.
fn operand_text(operand: &Operand) -> String {
    match operand {
        Operand::Copy(place) => format!("copy {}", place_text(place)),
        Operand::Move(place) => format!("move {}", place_text(place)),
        Operand::Const(constant) => constant_text(constant),
    }
}

/// Formats a place to human-readable text.
fn place_text(place: &Place) -> String {
    match place {
        Place::Local(local) => local_ref(*local),
        Place::Field { base, field } => format!("{}.field{}", local_ref(*base), field.0),
        Place::Index { base, index, .. } => format!("{}[{}]", local_ref(*base), operand_text(index)),
        Place::Global { base, projection } => match projection {
            GlobalProjection::Field(field) => format!("global{base}.field{}", field.0),
            GlobalProjection::Index { index, .. } => {
                format!("global{base}[{}]", operand_text(index))
            }
        },
    }
}

/// Formats a constant to human-readable text.
fn constant_text(constant: &Constant) -> String {
    match constant {
        Constant::Bool(value) => value.to_string(),
        Constant::Int(value) => value.to_string(),
        Constant::Float(value) => {
            if value.fract() == 0.0 {
                format!("{value:.1}")
            } else {
                value.to_string()
            }
        }
        Constant::BigInt(digits) => format!("{digits}n"),
        Constant::String(value) => format!("\"{value}\""),
        Constant::Symbol(value) => format!("symbol({value:?})"),
        Constant::Undefined => "undefined".to_owned(),
        Constant::None => "none".to_owned(),
    }
}

/// Formats a local reference to text.
fn local_ref(local: LocalId) -> String {
    format!("%{}", local.0)
}

/// Formats a type reference to human-readable text.
fn type_ref(mir: &Mir, ty: TypeId) -> String {
    let Some(ty_value) = mir.types.get(ty) else {
        return format!("t{}", ty.0);
    };
    match ty_value {
        Type::Bool => "Bool".to_owned(),
        Type::Int => "Int".to_owned(),
        Type::Float => "Float".to_owned(),
        Type::FixedInt(width) => width.rust_name().to_owned(),
        Type::BigInt => "BigInt".to_owned(),
        Type::String => "String".to_owned(),
        Type::Unknown => "Unknown".to_owned(),
        Type::Never => "Never".to_owned(),
        Type::TypeParam { name } => mir.symbols.get(*name).unwrap_or("<unknown>").to_owned(),
        Type::None => "None".to_owned(),
        Type::List(item) => format!("List<{}>", type_ref(mir, *item)),
        Type::Set(item) => format!("Set<{}>", type_ref(mir, *item)),
        Type::Dict(key, value) => {
            format!("Dict<{}, {}>", type_ref(mir, *key), type_ref(mir, *value))
        }
        Type::JsMap(key, value) => {
            format!("Map<{}, {}>", type_ref(mir, *key), type_ref(mir, *value))
        }
        Type::Tuple(items) => {
            let item_list = items
                .iter()
                .map(|item| type_ref(mir, *item))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({item_list})")
        }
        Type::Optional(item) => format!("Optional<{}>", type_ref(mir, *item)),
        Type::Union(items) => items
            .iter()
            .map(|item| type_ref(mir, *item))
            .collect::<Vec<_>>()
            .join(" | "),
        Type::Class { name, args } => {
            let class_name = mir.symbols.get(*name).unwrap_or("<unknown>");
            if args.is_empty() {
                class_name.to_owned()
            } else {
                let type_args = args
                    .iter()
                    .map(|arg| type_ref(mir, *arg))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{class_name}<{type_args}>")
            }
        }
        Type::Function(function) => {
            let params = function
                .params
                .iter()
                .enumerate()
                .map(|(index, param)| {
                    let text = type_ref(mir, *param);
                    if function.rest == Some(index) {
                        format!("...{text}")
                    } else {
                        text
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            let async_prefix = if function.is_async { "async " } else { "" };
            format!(
                "{async_prefix}fn({params}) -> {}",
                type_ref(mir, function.return_ty)
            )
        }
        Type::Future(item) => format!("Future<{}>", type_ref(mir, *item)),
        Type::Generator {
            is_async,
            yield_ty,
            return_ty,
            next_ty,
        } => format!(
            "{}Generator<{}, {}, {}>",
            if *is_async { "Async" } else { "" },
            type_ref(mir, *yield_ty),
            type_ref(mir, *return_ty),
            type_ref(mir, *next_ty)
        ),
        Type::GeneratorResult {
            yield_ty,
            return_ty,
        } => format!(
            "GeneratorResult<{}, {}>",
            type_ref(mir, *yield_ty),
            type_ref(mir, *return_ty)
        ),
    }
}

/// Convert an index into a `u32` identifier.
fn index_to_u32(index: usize, label: &str) -> u32 {
    u32::try_from(index).unwrap_or_else(|_| {
        let _ = label;
        u32::MAX
    })
}
