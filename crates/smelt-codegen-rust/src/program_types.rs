//! Declarations of the program's own types: interfaces, classes, unions and
//! JSON decoders.
#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible items are shared with the crate root and the emitter shards"
)]

use std::collections::HashSet;

use smelt_mir::Mir;

use crate::classes::{
    self, NominalStorage, class_impl_generics_text, class_name_text, class_type_params_text,
    effective_class_fields, effective_interface_fields, interface_impl_generics_text,
    interface_type_params_text, materialized_static_value, type_holds_function,
};
use crate::derive_impls::{
    class_is_json_serializable, derive_generics_text, emit_clone_impl_for_value_class,
    emit_debug_impl_for_storage_type, emit_default_impl_for_storage_type,
    emit_partial_eq_impl_for_value_class, interface_is_json_serializable, type_supports_partial_eq,
};
use crate::emitter::record_coercion::{
    emit_record_from_smelt_unknown_impl, emit_record_into_smelt_unknown_impl,
};
use crate::emitter::{self, EmitContext, FunctionEmitter};
use crate::reference_storage::{
    ReferenceRecordShape, emit_reference_class_storage, emit_reference_record_storage,
};
use crate::runtime_demand::Need;
use crate::rust::CodeWriter;
use crate::type_substitution::TypeSubstitution;
use crate::{EmitError, accessor_prelude};

/// Emit the program's own types: interfaces, classes, unions and JSON decoders.
///
/// These are written after [`PRELUDE_END_MARKER`], because the report that reads
/// the marker counts everything above it as runtime, and a user's type is not.
pub(crate) fn emit_program_types(mir: &Mir, context: &EmitContext) -> Result<String, EmitError> {
    let mut writer = CodeWriter::new();
    // One declaration per identity, in the crate's one claim order (see
    // `names::type_claim_order`); two symbols that render alike are two
    // structs, renamed apart by the name plan, never one shared struct.
    for decl in crate::names::type_claim_order(mir) {
        match decl {
            crate::names::TypeDecl::Interface(interface) => {
                emit_interface_type(&mut writer, mir, context, interface)?;
            }
            crate::names::TypeDecl::Class(class) => emit_class_type(&mut writer, mir, context, class)?,
        }
    }

    let union_definitions = emitter::emit_union_definitions(mir, context)?;
    if !union_definitions.is_empty() {
        for line in union_definitions.lines() {
            writer.line(line);
        }
    }

    // Strict JSON entry decoders (`emitter::json_decode`): boundary adapters
    // from the parsed JSON value into each source-declared target's concrete
    // type. Emitted beside the union definitions they construct.
    let json_decoders = emitter::emit_json_decoders(mir, context)?;
    if !json_decoders.is_empty() {
        for line in json_decoders.lines() {
            writer.line(line);
        }
    }

    Ok(writer.finish())
}

/// Emit one interface's storage: a reference record when its fields are written
/// after construction, otherwise a by-value struct with its derives and erasure
/// impls.
fn emit_interface_type(
    writer: &mut CodeWriter,
    mir: &Mir,
    context: &EmitContext,
    interface: &smelt_mir::MirInterface,
) -> Result<(), EmitError> {
    let demand = context.demand();
    let needs_serde_json = demand.has(Need::DepSerdeJson);
    let needs_unknown = demand.has(Need::Unknown);
    let name = context.names().type_name(interface.name)?.as_str().to_owned();
    let type_params = interface_type_params_text(mir, context, interface)?;
    let impl_generics = interface_impl_generics_text(mir, context, interface)?;
    let fields = effective_interface_fields(mir, interface);
    // A shape whose fields are written after construction is a reference
    // record: it needs the shared-cell handle so aliases observe the write,
    // exactly as a mutated class does. See `classify::reference_classes`.
    if context.is_reference_class(interface.name) {
        emit_reference_record_storage(
            writer,
            mir,
            context,
            &ReferenceRecordShape {
                name,
                type_params: type_params.clone(),
                type_args: type_params,
                impl_generics,
                fields,
                static_fields: &[],
                type_param_names: interface
                    .type_params
                    .iter()
                    .map(|param| param.name)
                    .collect(),
                declared_name: interface.name,
                // An object shape has no method bodies to bind.
                has_proto_entries: false,
                has_field_setter: false,
            },
            needs_unknown,
        )?;
        return Ok(());
    }
    let has_function_field = fields
        .iter()
        .any(|field| type_holds_function(mir, field.ty, NominalStorage::Opaque));
    // An interface-backed record is a by-value struct, exactly like a value
    // class, so it supports structural equality (JS `==`/`===`/`toBe`, and
    // derived comparisons in generated specs) under the same rule: every
    // stored field must itself be `PartialEq`.
    let interface_supports_partial_eq = fields
        .iter()
        .all(|field| type_supports_partial_eq(mir, context, field.ty, &mut Vec::new()));
    let interface_partial_eq_derive = if interface_supports_partial_eq {
        ", PartialEq"
    } else {
        ""
    };
    if has_function_field {
        writer.line("#[derive(Clone)]");
        writer.line("#[allow(dead_code)]");
    } else if needs_serde_json
        && interface_is_json_serializable(mir, interface)
        && !fields
            .iter()
            .any(|field| accessor_prelude::is_accessor_field(mir, interface.name, field.name))
    {
        writer.line(format!(
            "#[derive(Clone, Debug, Default{interface_partial_eq_derive}, serde::Serialize, serde::Deserialize)]"
        ));
    } else {
        writer.line(format!(
            "#[derive(Clone, Debug, Default{interface_partial_eq_derive})]"
        ));
    }
    // Only the parameters the emitted Rust declares: a position
    // `generic_elision` dropped is spelled nowhere, so it must not appear in
    // the `PhantomData` filler or in any hand-written impl's generic list
    // either (see `crate::generic_elision`).
    let interface_param_idents = context.type_param_elision().retain_carried(
        interface.name,
        interface
            .type_params
            .iter()
            .map(|param| {
                mir.symbols
                    .get(param.name)
                    .map(|param_name| crate::names::type_param(param_name).into_string())
                    .ok_or_else(|| {
                        EmitError::new("interface type parameter has unknown symbol")
                    })
            })
            .collect::<Result<Vec<_>, _>>()?,
    );
    let phantom_args = interface_param_idents.join(", ");
    let scoped_type_params = interface
        .type_params
        .iter()
        .map(|param| param.name)
        .collect::<HashSet<_>>();
    writer.block(format!("struct {name}{type_params}"), |block_writer| {
        for field in &fields {
            let field_name = context.names().field(mir, field.name).into_string();
            let field_ty = FunctionEmitter::type_text_for_with_scoped_type_params(
                mir,
                context,
                field.ty,
                &TypeSubstitution::lexical(&scoped_type_params),
            )
            .unwrap_or_else(|_| "SmeltUnknown".to_owned());
            let field_ty = if accessor_prelude::is_accessor_field(mir, interface.name, field.name) {
                accessor_prelude::storage_type_text(&field_ty)
            } else {
                field_ty
            };
            block_writer.line(format!("{field_name}: {field_ty},"));
        }
        if !interface_param_idents.is_empty() {
            block_writer.line(format!(
                "_smelt_phantom: ::std::marker::PhantomData<({phantom_args})>,"
            ));
        }
    });
    if has_function_field {
        emit_default_impl_for_storage_type(
            writer,
            mir,
            context,
            Some(interface.name),
            &name,
            &impl_generics,
            &type_params,
            &fields,
            &phantom_args,
            &scoped_type_params,
        )?;
        emit_debug_impl_for_storage_type(writer, &name, &impl_generics, &type_params);
    }
    if needs_unknown {
        emit_record_into_smelt_unknown_impl(
            writer,
            mir,
            context,
            Some(interface.name),
            &scoped_type_params,
            &name,
            &impl_generics,
            &type_params,
            &fields,
            &interface.decode_facts,
        )?;
        emit_record_from_smelt_unknown_impl(
            writer,
            mir,
            context,
            &name,
            &impl_generics,
            &type_params,
            &fields,
            &scoped_type_params,
        )?;
    }
    writer.blank_line();
    Ok(())
}

/// Emit one class's storage: the reference handle for a reference class,
/// otherwise a value struct with its derives or hand-written bounded impls, its
/// static accessors and erasure impls.
fn emit_class_type(
    writer: &mut CodeWriter,
    mir: &Mir,
    context: &EmitContext,
    class: &smelt_mir::MirClass,
) -> Result<(), EmitError> {
    let demand = context.demand();
    let needs_serde_json = demand.has(Need::DepSerdeJson);
    let needs_unknown = demand.has(Need::Unknown);
    let name = class_name_text(context, class)?;
    if context.is_reference_class(class.name) {
        emit_reference_class_storage(writer, mir, context, class, needs_unknown)?;
        return Ok(());
    }
    let type_params = class_type_params_text(mir, context, class)?;
    let impl_generics = class_impl_generics_text(mir, context, class)?;
    let mut field_lines = Vec::new();
    let fields = effective_class_fields(mir, class);
    let scoped_type_params = class
        .type_params
        .iter()
        .map(|param| param.name)
        .collect::<HashSet<_>>();
    // Only the parameters the emitted Rust declares; see the sibling
    // comment on the interface path and `crate::generic_elision`.
    let class_param_idents = context.type_param_elision().retain_carried(
        class.name,
        class
            .type_params
            .iter()
            .map(|param| {
                mir.symbols
                    .get(param.name)
                    .map(|param_name| crate::names::type_param(param_name).into_string())
                    .ok_or_else(|| EmitError::new("class type parameter has unknown symbol"))
            })
            .collect::<Result<Vec<_>, _>>()?,
    );
    let has_function_field = fields
        .iter()
        .any(|field| type_holds_function(mir, field.ty, NominalStorage::Opaque));
    // A value class supports structural equality (JS `==`/`===`/`toBe` on the
    // by-value representation, and derived comparisons in generated specs)
    // only when every stored field is itself `PartialEq`. Function fields
    // (`dyn Fn`) and promise fields (`SmeltPromise`, a `Clone`-only shared
    // future handle) are the two prelude shapes without `PartialEq`, so the
    // derive is gated on their absence.
    let supports_partial_eq = fields.iter().all(|field| {
        type_supports_partial_eq(mir, context, field.ty, &mut Vec::new())
    });
    let partial_eq_derive = if supports_partial_eq { ", PartialEq" } else { "" };
    // A GENERIC value class cannot use these derives at all. Every one of
    // them generates an impl bounded by its own trait and nothing more
    // (`impl<T: Clone> Clone`, `impl<T: Default> Default`, …), which is not
    // enough as soon as a field's type is another generated class: those
    // carry the full generated bound set
    // (`Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static`,
    // see `class_impl_generics_text`). Hono's
    // `struct TrieRouter<T> { root: Node<T> }` reported "the trait
    // `Clone`/`Default`/`IntoSmeltUnknown`/`SmeltFromUnknown` is not
    // implemented for `T`" once per derive (H29).
    //
    // The bounds are NOT moved onto the struct declaration: a bounded
    // declaration propagates to every generated type that mentions this one,
    // and that cascade is why the concrete-union emitter keeps its `pub enum`
    // bare and spells out each impl with the bound set it needs. Same choice
    // here — the impls are emitted by hand just below with `impl_generics`.
    // A class every one of whose parameters was elided is NOT generic in
    // Rust: it declares no parameters, so the ordinary derives apply and no
    // hand-written bounded impl is needed.
    let is_generic = !class_param_idents.is_empty();
    if has_function_field {
        writer.line("#[derive(Clone)]");
        writer.line("#[allow(dead_code)]");
    } else if is_generic {
        writer.line("#[allow(dead_code)]");
    } else if needs_serde_json && class_is_json_serializable(mir, class) {
        writer.line(format!(
            "#[derive(Clone, Debug, Default{partial_eq_derive}, serde::Serialize, serde::Deserialize)]"
        ));
    } else {
        writer.line(format!("#[derive(Clone, Debug, Default{partial_eq_derive})]"));
    }
    // The field types are kept as text as well: a hand-written `Clone` or
    // `PartialEq` bounds them in a `where` clause, which is the exact
    // requirement its body has (see the derive-gating comment above).
    let mut field_types = Vec::with_capacity(fields.len());
    for field in &fields {
        let field_type = FunctionEmitter::rust_type_with_scoped_type_params(
            mir,
            context,
            field.ty,
            &TypeSubstitution::lexical(&scoped_type_params),
        )?;
        field_lines.push(format!(
            "{}: {field_type},",
            context.names().field(mir, field.name),
        ));
        field_types.push(field_type);
    }
    if !class_param_idents.is_empty() {
        let phantom_args = class_param_idents.join(", ");
        field_lines.push(format!(
            "_smelt_phantom: ::std::marker::PhantomData<({phantom_args})>,"
        ));
    }
    writer.block(format!("struct {name}{type_params}"), |block_writer| {
        for field_line in field_lines {
            block_writer.line(field_line);
        }
    });
    if has_function_field || is_generic {
        let phantom_args = class_param_idents.join(", ");
        // The generated bound set, plus the field-type `where` clause the
        // emitter adds. See `derive_generics_text` for why both are needed,
        // and `classes::GENERATED_TYPE_PARAM_BOUNDS` for why the set is the
        // full one rather than the derive-equivalent `T: Default`.
        let default_generics = if is_generic {
            derive_generics_text(
                &class_param_idents,
                classes::GENERATED_TYPE_PARAM_BOUNDS,
            )
        } else {
            impl_generics.clone()
        };
        emit_default_impl_for_storage_type(
            writer,
            mir,
            context,
            None,
            &name,
            &default_generics,
            &type_params,
            &fields,
            &phantom_args,
            &scoped_type_params,
        )?;
        // `Debug` names the struct and stops, so it needs no bound at all.
        let debug_generics = if is_generic {
            type_params.clone()
        } else {
            impl_generics.clone()
        };
        emit_debug_impl_for_storage_type(writer, &name, &debug_generics, &type_params);
    }
    // A generic value class also loses its `Clone` and `PartialEq` derives
    // for the same reason, so those are spelled out too. The callback case
    // keeps `#[derive(Clone)]` (its `dyn Fn` field is an `Rc`, and it has no
    // `PartialEq` to begin with), so this is the generic case only.
    //
    // Both bound their FIELD TYPES in a `where` clause rather than bounding
    // `T`. That is the exact requirement of a field-by-field body, and it is
    // the only formulation that is right for both shapes at once:
    // `Ok<T> { value: T }` needs `T: Clone` (so a union deriving `Clone`
    // over an `Ok<T>` arm can still satisfy it), while
    // `TrieRouter<T> { root: Node<T> }` needs `Node<T>: Clone`, which pulls
    // the whole generated bound set — a requirement of `Node`, not of
    // `TrieRouter`, and one only its own use sites can discharge.
    if is_generic && !has_function_field {
        let field_names = fields
            .iter()
            .map(|field| context.names().field_text(mir, field.name))
            .collect::<Vec<_>>();
        emit_clone_impl_for_value_class(
            writer,
            &name,
            &type_params,
            &type_params,
            &field_names,
            &field_types,
            true,
        );
        if supports_partial_eq {
            emit_partial_eq_impl_for_value_class(
                writer,
                &name,
                &type_params,
                &type_params,
                &field_names,
                &field_types,
            );
        }
    }
    if !class.static_fields.is_empty() {
        writer.block(
            format!("impl{impl_generics} {name}{type_params}"),
            |impl_writer| {
                for field in &class.static_fields {
                    let field_name = context.names().field_text(mir, field.name);
                    let field_ty = FunctionEmitter::type_text_for_with_scoped_type_params(
                        mir,
                        context,
                        field.ty,
                        &TypeSubstitution::lexical(&scoped_type_params),
                    )
                    .unwrap_or_else(|_| "SmeltUnknown".to_owned());
                    // A mutable static's accessor reads its live storage.
                    let value = field.global.map_or_else(
                        || materialized_static_value(field.value.as_ref(), mir.types.get(field.ty) != Some(&smelt_hir::Type::PyStr)),
                        |global| crate::rust::RustExpr::chain(crate::globals::global_read_text(mir, global)),
                    );
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
            writer,
            mir,
            context,
            None,
            &scoped_type_params,
            &name,
            &impl_generics,
            &type_params,
            &fields,
            &[],
        )?;
        emit_record_from_smelt_unknown_impl(
            writer,
            mir,
            context,
            &name,
            &impl_generics,
            &type_params,
            &fields,
            &scoped_type_params,
        )?;
    }
    writer.blank_line();
    Ok(())
}
