//! Storage of reference records: the `Rc<RefCell<_>>` handle newtype, its inner
//! record and the impls that keep identity on the handle.
#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible items are shared with the crate root and the emitter shards"
)]

use crate::rust::RustExpr;
use std::collections::HashSet;

use smelt_mir::{Mir, MirFunction};

use crate::classes::{
    class_impl_generics_text, class_name_text, class_type_args_text, class_type_params_text,
    NominalStorage, effective_class_fields, effective_class_methods,
    materialized_static_value, type_holds_function,
};
use crate::derive_impls::{
    derive_generics_text, emit_debug_impl_for_storage_type, emit_default_impl_for_storage_type,
};
use crate::emitter::record_coercion::record_field_unknown_expr;
use crate::emitter::{self, EmitContext, FunctionEmitter};
use crate::rust::CodeWriter;
use crate::type_substitution::TypeSubstitution;
use crate::{EmitError, class_proto, classes, reference_origin};

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
pub(crate) fn emit_reference_class_storage(
    writer: &mut CodeWriter,
    mir: &Mir,
    context: &EmitContext,
    class: &smelt_mir::MirClass,
    needs_unknown: bool,
) -> Result<(), EmitError> {
    emit_reference_record_storage(
        writer,
        mir,
        context,
        &ReferenceRecordShape {
            name: class_name_text(context, class)?,
            type_params: class_type_params_text(mir, context, class)?,
            type_args: class_type_args_text(mir, context, class)?,
            impl_generics: class_impl_generics_text(mir, context, class)?,
            fields: effective_class_fields(mir, class),
            static_fields: &class.static_fields,
            type_param_names: class.type_params.iter().map(|param| param.name).collect(),
            declared_name: class.name,
            has_proto_entries: class_proto::class_has_proto_entries(mir, context, class),
            has_field_setter: field_setter_host(mir, context, class).is_some(),
        },
        needs_unknown,
    )
}

/// Everything the reference (handle) representation needs about one record type.
///
/// A reference record is emitted identically whether the source spelled it as a
/// `class` or as an object *shape* (an `interface`, or an inline object type
/// literal lowered to a synthetic one). Both are JavaScript objects, so both need the same handle newtype
/// when a field is written after construction; only where the pieces come from
/// differs, which is what this struct hides.
pub(crate) struct ReferenceRecordShape<'a> {
    /// Generated Rust type name.
    pub(crate) name: String,
    /// Declaration-site generic list, e.g. `<T>`; empty when non-generic.
    pub(crate) type_params: String,
    /// Use-site generic list, e.g. `<T>`; empty when non-generic.
    pub(crate) type_args: String,
    /// `impl` generic list including bounds.
    pub(crate) impl_generics: String,
    /// Stored fields, heritage included, in declaration order.
    pub(crate) fields: Vec<smelt_mir::MirField>,
    /// Materialized class-level fields; always empty for a shape.
    pub(crate) static_fields: &'a [smelt_mir::MirStaticField],
    /// Generic parameter symbols, for the lexical type-parameter scope.
    ///
    /// This is the FULL declared list: an elided parameter stays in lexical
    /// scope so a stray occurrence renders its name and fails loudly with
    /// `cannot find type`, rather than silently erasing to `SmeltUnknown`.
    /// What the emitted Rust declares is [`Self::declared_name`]'s carried
    /// subset (see `crate::generic_elision`).
    pub(crate) type_param_names: Vec<smelt_hir::Symbol>,
    /// Name symbol of the declaring class or interface, for the elision map.
    pub(crate) declared_name: smelt_hir::Symbol,
    /// Whether the type emits `__smelt_proto_entries` (see [`crate::class_proto`]).
    ///
    /// Only a `class` has method bodies to bind; an object *shape* has none, so
    /// it is always `false` there.
    pub(crate) has_proto_entries: bool,
    /// Whether the type emits the erased-view write-through setter (see
    /// `emitter::view_write_through`); always `false` for a shape.
    pub(crate) has_field_setter: bool,
}

/// Emit the handle newtype, inner record, and identity impls for one record type.
pub(crate) fn emit_reference_record_storage(
    writer: &mut CodeWriter,
    mir: &Mir,
    context: &EmitContext,
    shape: &ReferenceRecordShape<'_>,
    needs_unknown: bool,
) -> Result<(), EmitError> {
    let ReferenceRecordShape {
        name,
        type_params,
        type_args,
        impl_generics,
        fields,
        static_fields,
        type_param_names,
        declared_name,
        has_proto_entries,
        has_field_setter,
    } = shape;
    let inner_name = format!("{name}Inner");
    // Fields holding a closure that captures this instance weakly
    // (`emitter::weak_receiver`): the handle exposes its cell to that capture,
    // and the erasure keeps the instance alive through those fields' values.
    let weak_fields: HashSet<smelt_hir::Symbol> = fields
        .iter()
        .filter(|field| context.weak_receivers().is_weak_field(*declared_name, field.name))
        .map(|field| field.name)
        .collect();
    if !weak_fields.is_empty() {
        writer.line(emitter::weak_receiver::handle_impl_text(name, impl_generics, type_args));
        let checked = fields
            .iter()
            .filter(|field| weak_fields.contains(&field.name))
            .map(|field| {
                (
                    context.names().field(mir, field.name).into_string(),
                    emitter::weak_receiver::closure_slot(mir, field.ty).unwrap_or(""),
                )
            })
            .collect::<Vec<_>>();
        writer.line(emitter::weak_receiver::drop_impl_text(name, type_params, type_args, &checked));
        writer.blank_line();
    }
    let scoped_type_params = type_param_names.iter().copied().collect::<HashSet<_>>();
    let has_function_field = fields
        .iter()
        .any(|field| type_holds_function(mir, field.ty, NominalStorage::Opaque));
    let record_param_idents = context.type_param_elision().retain_carried(
        *declared_name,
        type_param_names
            .iter()
            .map(|param| {
                mir.symbols
                    .get(*param)
                    .map(|param_name| crate::names::type_param(param_name).into_string())
                    .ok_or_else(|| EmitError::new("record type parameter has unknown symbol"))
            })
            .collect::<Result<Vec<_>, _>>()?,
    );
    let phantom_args = record_param_idents.join(", ");

    // The handle newtype. `#[derive(Clone)]` is intentionally NOT used: a derived
    // clone would require `Inner: Clone` and clone the cell contents. We hand-
    // write `Rc::clone` below so a clone shares identity.
    writer.line("#[allow(dead_code)]");
    writer.line(format!(
        "struct {name}{type_params}(::std::rc::Rc<::std::cell::RefCell<{inner_name}{type_args}>>);"
    ));

    // The inner record. `Debug`/`Default` are derived only when the record is
    // NON-GENERIC and no callback field blocks the derives.
    //
    // A derive on a generic struct generates `impl<T: Debug> Debug` /
    // `impl<T: Default> Default` — its own bound and nothing more. That is not
    // enough whenever a field's type is another generated class, because those
    // carry the full generated bound set
    // (`Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static`, see
    // `class_impl_generics_text`). Hono's `SmartRouterInner<T>` holds a
    // `SmeltList<Router<T>>` and `NodeInner<T>` a `SmeltRecord<String, Node<T>>`,
    // so `derive(Default)` demanded `Router<T>: Default` with only `T: Default`
    // in scope: "the trait `Clone`/`Default`/`IntoSmeltUnknown`/
    // `SmeltFromUnknown` is not implemented for `T`" (H29).
    //
    // The bounds do NOT go on the struct declaration: a bounded declaration
    // propagates to every type that mentions it — starting with the handle
    // newtype whose field is `Rc<RefCell<Inner<T>>>` — and that cascade is why
    // the concrete-union emitter keeps its `pub enum` declaration bare and
    // hand-writes each impl with the bound set it needs. Same choice here, and
    // both hand-written impls already exist for the callback case.
    if has_function_field || !type_params.is_empty() {
        writer.line("#[allow(dead_code)]");
        writer.block(
            format!("struct {inner_name}{type_params}"),
            |block_writer| {
                emit_reference_inner_fields(block_writer, mir, context, fields, &scoped_type_params, &phantom_args);
            },
        );
        // Per-trait bounds. `Debug` names the struct and stops, so it needs no
        // bound at all. `Default` DOES take the full generated set
        // (`classes::GENERATED_TYPE_PARAM_BOUNDS`): its body constructs every
        // field's default explicitly, and a field default can reach another
        // generated class — Hono's `Hono_1Inner<E, S, BasePath, CurrentPath>`
        // defaults its `onError`/`notFound` slots to closures RETURNING
        // `Hono_1<E, S, BasePath, CurrentPath>`, so the body needs
        // `Hono_1: Default`, which is emitted with the full set. The
        // derive-equivalent `T: Default` cannot prove it, and no `where` clause
        // on a field's TYPE reaches it either, because the requirement comes
        // from the default EXPRESSION and not from the slot's `Rc<dyn Fn(..)>`
        // type. Since every generic item the crate emits carries the same set,
        // asking for it here cannot fail for an in-crate consumer, and a
        // parameter instantiated at a concrete generated type satisfies it by
        // construction.
        let (default_generics, debug_generics) = if type_params.is_empty() {
            (impl_generics.clone(), impl_generics.clone())
        } else {
            (
                derive_generics_text(
                    &record_param_idents,
                    classes::GENERATED_TYPE_PARAM_BOUNDS,
                ),
                type_params.clone(),
            )
        };
        emit_default_impl_for_storage_type(
            writer,
            mir,
            context,
            None,
            &inner_name,
            &default_generics,
            type_args,
            fields,
            &phantom_args,
            &scoped_type_params,
        )?;
        emit_debug_impl_for_storage_type(writer, &inner_name, &debug_generics, type_args);
    } else {
        writer.line("#[derive(Debug, Default)]");
        writer.line("#[allow(dead_code)]");
        writer.block(
            format!("struct {inner_name}{type_params}"),
            |block_writer| {
                emit_reference_inner_fields(block_writer, mir, context, fields, &scoped_type_params, &phantom_args);
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
                    if type_param_names.is_empty() {
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

    if !static_fields.is_empty() {
        writer.block(
            format!("impl{impl_generics} {name}{type_args}"),
            |impl_writer| {
                for field in *static_fields {
                    let field_name = context.names().field_text(mir, field.name);
                    let field_ty = FunctionEmitter::type_text_for_with_scoped_type_params(
                        mir,
                        context,
                        field.ty,
                        &TypeSubstitution::lexical(&scoped_type_params),
                    )
                    .unwrap_or_else(|_| "SmeltUnknown".to_owned());
                    let value = materialized_static_value(field.value.as_ref(), mir.types.get(field.ty) != Some(&smelt_hir::Type::PyStr));
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
            context,
            name,
            impl_generics,
            type_args,
            fields,
            &weak_fields,
            *has_proto_entries,
            *has_field_setter,
            *has_field_setter
                && emitter::view_write_through::has_live_callable_fields(
                    mir,
                    context.program_reads_this(),
                    fields,
                ),
        )?;
        // The inbound half restores the live handle (see `reference_origin`).
        reference_origin::emit_reference_from_smelt_unknown_impl(
            writer,
            mir,
            context,
            name,
            impl_generics,
            type_args,
            fields,
        );
    }
    writer.blank_line();
    Ok(())
}

/// The function whose emitter renders a reference class's erased-view field
/// setter (see `emitter::view_write_through`), when the class gets one.
///
/// Any function of the class works — the setter only needs the class's type
/// parameters in its render scope — so the constructor is used, falling back
/// to the first method. `None` when the program has no erased property write
/// (the machinery is pay-for-use), when the class is not a reference class
/// (a by-value instance has no identity for a view to stand for), or when the
/// class has no function at all.
pub(crate) fn field_setter_host<'mir>(
    mir: &'mir Mir,
    context: &EmitContext,
    class: &smelt_mir::MirClass,
) -> Option<&'mir MirFunction> {
    if !context.erased_view_write_through() || !context.is_reference_class(class.name) {
        return None;
    }
    class
        .constructor
        .into_iter()
        .chain(effective_class_methods(mir, class))
        .find_map(|function| mir.functions.get(usize::try_from(function.0).ok()?))
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
        let field_name = context.names().field(mir, field.name).into_string();
        let field_ty = FunctionEmitter::type_text_for_with_scoped_type_params(
            mir,
            context,
            field.ty,
            &TypeSubstitution::lexical(scoped_type_params),
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
    context: &EmitContext,
    name: &str,
    impl_generics: &str,
    type_args: &str,
    fields: &[smelt_mir::MirField],
    weak_fields: &HashSet<smelt_hir::Symbol>,
    has_proto_entries: bool,
    has_field_setter: bool,
    has_live_reads: bool,
) -> Result<(), EmitError> {
    // Genuine dynamic boundary: this impl IS the typed-to-erased crossing, so
    // `smelt-unknown-report` counts its body as a boundary, not as erasure.
    writer.block(
        format!("impl{impl_generics} IntoSmeltUnknown for {name}{type_args}"),
        |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                // One shared cell is one JavaScript object, so the erased id is
                // derived from the cell address rather than minted fresh; see
                // `smelt_reference_object_identity` in the prelude.
                fn_writer.line(
                    "let __smelt_id = smelt_reference_object_identity(&self.0);",
                );
                // The prototype members are collected BEFORE the cell is
                // borrowed: each adapter clones the handle, and taking the
                // `Ref` first would not conflict but reads worse than keeping
                // the two phases separate.
                if has_proto_entries {
                    fn_writer.line(format!(
                        "let __smelt_proto = self.{method}();",
                        method = class_proto::PROTO_ENTRIES_METHOD,
                    ));
                }
                fn_writer.line("let __smelt_inner = self.0.borrow();");
                fn_writer.line(
                    "let mut __smelt_entries: Vec<(String, SmeltUnknown)> = Vec::from([",
                );
                for field in fields {
                    if !field.visibility.is_own_property() {
                        continue;
                    }
                    // The erased key is the SOURCE spelling (`mir.names`), the
                    // Rust field is the sanitized one: `onRead` must read back
                    // as `"onRead"` from an erased view, not as `"on_read"`.
                    let field_name = context.names().field(mir, field.name).into_string();
                    let key = mir
                        .names
                        .get(field.name)
                        .or_else(|| mir.symbols.get(field.name))
                        .unwrap_or("field");
                    let value = record_field_unknown_expr(
                        mir,
                        &RustExpr::chain(format!("__smelt_inner.{field_name}.clone()")).ending_in_clone(),
                        field.ty,
                    )
                    .unwrap_or_else(|_| RustExpr::atom("SmeltUnknown::Null"));
                    // A closure capturing this instance weakly must hold it
                    // strongly once it is reachable from an erased view.
                    let value = if weak_fields.contains(&field.name) {
                        format!("smelt_keep_receiver_alive({value}, &self)")
                    } else {
                        value.into_string()
                    };
                    fn_writer.line(format!("({key:?}.to_owned(), {value}),"));
                }
                fn_writer.line("]);");
                if has_proto_entries {
                    fn_writer.line("__smelt_entries.extend(__smelt_proto);");
                }
                // A property write through this view reaches the instance
                // (see `emitter::view_write_through`).
                if has_field_setter {
                    fn_writer.line(emitter::view_write_through::erased_view_entry_expr(&RustExpr::path("self"), "__smelt_entries", has_live_reads));
                }
                // The erased view holds the live handle, so narrowing it back
                // yields this instance (see `reference_origin`).
                fn_writer.line(format!(
                    "SmeltUnknown::Object({})",
                    reference_origin::with_origin_expr("SmeltObject::with_id(__smelt_id, __smelt_entries)", "self")
                ));
            });
        },
    );
    Ok(())
}
