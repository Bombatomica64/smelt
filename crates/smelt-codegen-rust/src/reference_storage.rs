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
    InheritedStaticSlot, NominalStorage, class_impl_generics_text, class_name_text,
    class_type_args_text, class_type_params_text, effective_class_fields, effective_class_methods,
    inherited_static_slots, materialized_static_value, type_holds_function,
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
            // A subclass statics frame flattens its base frame's static
            // fields as storage it never reads (its inherited statics are
            // accessors delegating to the base frame), so its erasure
            // projects only the statics it declares itself.
            // A method's callable slot is storage, not an own property: the
            // method is a prototype member (`__smelt_proto_entries`), so the
            // erased projection leaves it out, as the inline erasure does.
            projected_fields: if class.statics_of.is_some() {
                class.fields.clone()
            } else {
                let slots = classes::own_virtual_slot_names(mir, class);
                effective_class_fields(mir, class)
                    .into_iter()
                    .filter(|field| !slots.contains(&field.name))
                    .collect()
            },
            inherited_statics: inherited_static_slots(mir, class),
            static_fields: &class.static_fields,
            type_param_names: class.type_params.iter().map(|param| param.name).collect(),
            declared_name: class.name,
            has_proto_entries: class_proto::class_has_proto_entries(mir, context, class),
            has_field_setter: field_setter_host(mir, context, class).is_some(),
            layout: crate::class_polymorphism::class_layout(mir, context, class)?,
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
    /// The fields its erasure projects as own properties: [`Self::fields`],
    /// except for a statics frame (see `emit_reference_class_storage`).
    pub(crate) projected_fields: Vec<smelt_mir::MirField>,
    /// A subclass statics frame's inherited statics: its erasure projects
    /// each one the subclass has written as an own property, as JavaScript
    /// makes a write through the subclass an own property of it.
    pub(crate) inherited_statics: Vec<InheritedStaticSlot>,
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
    /// The polymorphic layout of a class in a polymorphic hierarchy (see
    /// [`crate::class_polymorphism`]); `None` for a shape and for every other
    /// class.
    pub(crate) layout: Option<crate::class_polymorphism::ClassLayout>,
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
        projected_fields,
        inherited_statics,
        static_fields,
        type_param_names,
        declared_name,
        has_proto_entries,
        has_field_setter,
        layout,
    } = shape;
    let inner_name = format!("{name}Inner");
    let polymorphic = layout.as_ref().is_some_and(|found| found.polymorphic);
    let nested_base_type = layout
        .as_ref()
        .and_then(|found| found.nested_base_type.clone());
    // The fields this level's inner record declares: a nested record keeps
    // its base's fields inside its base record (`crate::class_polymorphism`).
    let level_fields: &[smelt_mir::MirField] = layout
        .as_ref()
        .map_or(fields.as_slice(), |found| found.level_fields.as_slice());
    // The runtime class tag (`crate::class_identity`), on a class that carries it.
    let identity = context
        .carries_class_identity(*declared_name)
        .then(|| crate::class_identity::identity_literal(mir, *declared_name));
    // Fields holding a closure that captures this instance weakly
    // (`emitter::weak_receiver`): the handle exposes its cell to that capture,
    // and the erasure keeps the instance alive through those fields' values.
    let weak_fields: HashSet<smelt_hir::Symbol> = fields
        .iter()
        .filter(|field| {
            context
                .weak_receivers()
                .is_weak_field(*declared_name, field.name)
        })
        .map(|field| field.name)
        .collect();
    if !weak_fields.is_empty() {
        let cell = if polymorphic {
            format!(
                "dyn {}{type_args}",
                crate::class_polymorphism::cell_trait_name(name)
            )
        } else {
            format!("::std::cell::RefCell<{inner_name}{type_args}>")
        };
        writer.line(emitter::weak_receiver::handle_impl_text(
            name,
            impl_generics,
            type_args,
            &cell,
        ));
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
        writer.line(emitter::weak_receiver::drop_impl_text(
            name,
            type_params,
            type_args,
            &checked,
        ));
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
    if polymorphic {
        // Any instance of the class or of a subclass: one shared cell whose
        // concrete type is the constructed class (`crate::class_polymorphism`).
        writer.line(format!(
            "struct {name}{type_params}(::std::rc::Rc<dyn {}{type_args}>);",
            crate::class_polymorphism::cell_trait_name(name)
        ));
    } else {
        writer.line(format!(
            "struct {name}{type_params}(::std::rc::Rc<::std::cell::RefCell<{inner_name}{type_args}>>);"
        ));
    }

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
    if has_function_field || !type_params.is_empty() || identity.is_some() {
        writer.line("#[allow(dead_code)]");
        writer.block(
            format!("struct {inner_name}{type_params}"),
            |block_writer| {
                emit_reference_inner_fields(
                    block_writer,
                    mir,
                    context,
                    level_fields,
                    nested_base_type.as_deref(),
                    &scoped_type_params,
                    &phantom_args,
                    identity.is_some(),
                );
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
                derive_generics_text(&record_param_idents, classes::GENERATED_TYPE_PARAM_BOUNDS),
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
            level_fields,
            &phantom_args,
            &scoped_type_params,
            identity.as_deref(),
            nested_base_type.is_some(),
        )?;
        emit_debug_impl_for_storage_type(writer, &inner_name, &debug_generics, type_args);
    } else {
        writer.line("#[derive(Debug, Default)]");
        writer.line("#[allow(dead_code)]");
        writer.block(
            format!("struct {inner_name}{type_params}"),
            |block_writer| {
                emit_reference_inner_fields(
                    block_writer,
                    mir,
                    context,
                    level_fields,
                    nested_base_type.as_deref(),
                    &scoped_type_params,
                    &phantom_args,
                    false,
                );
            },
        );
    }

    // The polymorphic layout (`crate::class_polymorphism`): a nested record
    // answers its base's fields, a polymorphic class declares its cell trait,
    // and this class's concrete cell implements its own trait and every
    // polymorphic ancestor's.
    if let Some(class_layout) = layout {
        let inner_type = format!("{inner_name}{type_args}");
        if let Some(base_type) = &nested_base_type {
            writer.line(crate::class_polymorphism::deref_impls_text(
                type_params,
                &inner_type,
                base_type,
            ));
        }
        if class_layout.polymorphic {
            let Some(class) = classes::find_class(mir, *declared_name) else {
                return Err(EmitError::new("a polymorphic class is not declared"));
            };
            writer.line(crate::class_polymorphism::cell_trait_declaration(
                mir,
                context,
                class,
                type_params,
                type_args,
                needs_unknown,
            )?);
        }
        let turbofish = if type_args.is_empty() {
            String::new()
        } else {
            format!("::{type_args}")
        };
        let erase = needs_unknown.then(|| {
            format!(
                "{name}{turbofish}(self).{}()",
                crate::class_polymorphism::OWN_ERASURE_METHOD
            )
        });
        for cell_impl in &class_layout.cell_impls {
            writer.line(crate::class_polymorphism::cell_impl_text(
                cell_impl,
                impl_generics,
                &inner_type,
                &class_layout.class_name_literal,
                erase.as_deref(),
            ));
        }
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
                if polymorphic {
                    // The address alone: a polymorphic handle's `Rc<dyn ..>`
                    // also carries a vtable, which is no part of identity.
                    fn_writer.line("::std::ptr::addr_eq(::std::rc::Rc::as_ptr(&self.0), ::std::rc::Rc::as_ptr(&other.0))");
                } else {
                    fn_writer.line("::std::rc::Rc::ptr_eq(&self.0, &other.0)");
                }
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
                    // A mutable static's accessor reads its live storage; an
                    // inherited one asks its getter (`crate::classes`).
                    let value = if let Some(read) =
                        classes::inherited_static_read_text(mir, context, field)
                    {
                        RustExpr::chain(read)
                    } else {
                        field.global.map_or_else(
                            || {
                                materialized_static_value(
                                    field.value.as_ref(),
                                    mir.types.get(field.ty),
                                )
                            },
                            |global| RustExpr::chain(crate::globals::global_read_text(mir, global)),
                        )
                    };
                    impl_writer.block(
                        format!("fn __smelt_static_{field_name}() -> {field_ty}"),
                        |function_writer| function_writer.line(value),
                    );
                }
            },
        );
    }

    if needs_unknown && emitter::live_view::is_live_record(mir, context, *declared_name) {
        // A live record erases to a view of itself and is recovered from one
        // (see `emitter::live_view`).
        let host = mir
            .functions
            .first()
            .ok_or_else(|| EmitError::new("a live record needs a function to render in"))?;
        let host_emitter = FunctionEmitter::new(mir, context, host)?;
        writer.line(
            host_emitter
                .live_object_impl_text(*declared_name, name)?
                .trim_end(),
        );
        writer.block(
            format!("impl{impl_generics} IntoSmeltUnknown for {name}{type_args}"),
            |impl_writer| {
                impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                    fn_writer.line(
                        "SmeltUnknown::Object(SmeltObject::smelt_live_view(smelt_reference_object_identity(&self.0), self))",
                    );
                });
            },
        );
        let subclass_origins = match (layout, classes::find_class(mir, *declared_name)) {
            (Some(class_layout), Some(class)) if class_layout.polymorphic => {
                crate::class_polymorphism::subclass_origins(mir, context, class)?
            }
            _ => Vec::new(),
        };
        reference_origin::emit_reference_from_smelt_unknown_impl(
            writer,
            mir,
            context,
            name,
            impl_generics,
            type_args,
            fields,
            &subclass_origins,
            true,
        );
    } else if needs_unknown {
        emit_reference_class_into_smelt_unknown_impl(
            writer,
            mir,
            context,
            *declared_name,
            name,
            impl_generics,
            type_args,
            projected_fields,
            inherited_statics,
            &weak_fields,
            *has_proto_entries,
            *has_field_setter,
            *has_field_setter
                && emitter::view_write_through::has_live_callable_fields(
                    mir,
                    context.program_reads_this(),
                    fields,
                ),
            polymorphic,
        )?;
        // The inbound half restores the live handle (see `reference_origin`).
        let subclass_origins = match (layout, classes::find_class(mir, *declared_name)) {
            (Some(class_layout), Some(class)) if class_layout.polymorphic => {
                crate::class_polymorphism::subclass_origins(mir, context, class)?
            }
            _ => Vec::new(),
        };
        reference_origin::emit_reference_from_smelt_unknown_impl(
            writer,
            mir,
            context,
            name,
            impl_generics,
            type_args,
            fields,
            &subclass_origins,
            false,
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
    nested_base_type: Option<&str>,
    scoped_type_params: &HashSet<smelt_hir::Symbol>,
    phantom_args: &str,
    identity: bool,
) {
    // A nested record holds its base's record first
    // (`crate::class_polymorphism`).
    if let Some(base_type) = nested_base_type {
        block_writer.line(format!(
            "{}: {base_type},",
            crate::class_polymorphism::BASE_FIELD
        ));
    }
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
    if identity {
        block_writer.line(crate::class_identity::field_declaration(false));
    }
}

/// Emit `IntoSmeltUnknown` for a reference class by borrowing the shared cell.
///
/// Mirrors [`emit_record_into_smelt_unknown_impl`] but projects the public
/// fields out of `self.0.borrow()` rather than owned struct fields.
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "each flag is an independent fact about the class the caller already holds"
)]
fn emit_reference_class_into_smelt_unknown_impl(
    writer: &mut CodeWriter,
    mir: &Mir,
    context: &EmitContext,
    class: smelt_hir::Symbol,
    name: &str,
    impl_generics: &str,
    type_args: &str,
    fields: &[smelt_mir::MirField],
    inherited_statics: &[InheritedStaticSlot],
    weak_fields: &HashSet<smelt_hir::Symbol>,
    has_proto_entries: bool,
    has_field_setter: bool,
    has_live_reads: bool,
    polymorphic: bool,
) -> Result<(), EmitError> {
    // A polymorphic handle may hold an instance of a subclass, which erases as
    // that subclass (its own fields, its own class marker): the trait impl
    // dispatches to the concrete cell, and this class's own projection becomes
    // the inherent method its own cell's `into_smelt_unknown_as_concrete` calls
    // (`crate::class_polymorphism`).
    let (impl_header, fn_header) = if polymorphic {
        writer.block(
            format!("impl{impl_generics} IntoSmeltUnknown for {name}{type_args}"),
            |impl_writer| {
                impl_writer.line("fn into_smelt_unknown(self) -> SmeltUnknown { ::std::rc::Rc::clone(&self.0).into_smelt_unknown_as_concrete() }");
            },
        );
        (
            format!("impl{impl_generics} {name}{type_args}"),
            format!(
                "fn {}(self) -> SmeltUnknown",
                crate::class_polymorphism::PROJECTED_ERASURE_METHOD
            ),
        )
    } else {
        (
            format!("impl{impl_generics} IntoSmeltUnknown for {name}{type_args}"),
            "fn into_smelt_unknown(self) -> SmeltUnknown".to_owned(),
        )
    };
    // Genuine dynamic boundary: this impl IS the typed-to-erased crossing, so
    // `smelt-unknown-report` counts its body as a boundary, not as erasure.
    writer.block(
        impl_header,
        |impl_writer| {
            impl_writer.block(fn_header, |fn_writer| {
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
                fn_writer.line(
                    "if let Some(__smelt_open) = smelt_open_reference_view(__smelt_id) { return SmeltUnknown::Object(__smelt_open); }",
                );
                fn_writer.line(format!(
                    "let __smelt_view = {};",
                    reference_origin::with_origin_expr("SmeltObject::with_id(__smelt_id, Vec::<(String, SmeltUnknown)>::new())", "self.clone()").delimited()
                ));
                fn_writer.line("smelt_begin_reference_view(__smelt_id, &__smelt_view);");
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
                    // A field a subclass redeclares narrower is stored, and so
                    // read here, at its declaring level's type.
                    let stored_ty = crate::class_polymorphism::redeclared_field_storage(
                        mir,
                        context.polymorphic_classes(),
                        class,
                        field.name,
                    )
                    .unwrap_or(field.ty);
                    let value = record_field_unknown_expr(
                        mir,
                        &RustExpr::path(format!("__smelt_inner.{field_name}")).cloned_value(),
                        stored_ty,
                    )
                    .unwrap_or_else(|_| RustExpr::atom("SmeltUnknown::Null"));
                    // A closure capturing this instance weakly must hold it
                    // strongly once it is reachable from an erased view.
                    let value = if weak_fields.contains(&field.name) {
                        format!("smelt_keep_receiver_alive({value}, &self)", value = value.delimited())
                    } else {
                        value.into_string()
                    };
                    fn_writer.line(format!("({key:?}.to_owned(), {value}),"));
                }
                fn_writer.line("]);");
                for slot in inherited_statics {
                    let key = mir.names.get(slot.name).or_else(|| mir.symbols.get(slot.name)).unwrap_or("field");
                    let own = context.names().field(mir, slot.own).into_string();
                    let shadow = context.names().field(mir, slot.shadow).into_string();
                    let value = record_field_unknown_expr(
                        mir,
                        &RustExpr::path(format!("__smelt_inner.{shadow}")).cloned_value(),
                        slot.shadow_ty,
                    )
                    .unwrap_or_else(|_| RustExpr::atom("SmeltUnknown::Undefined"));
                    fn_writer.line(format!(
                        "if __smelt_inner.{own} {{ __smelt_entries.push(({key:?}.to_owned(), {value})); }}",
                        value = value.delimited()
                    ));
                }
                // As in the value-record view: an optional field holding `None`
                // has no key at all, which `Object.keys`, `in` and spread observe.
                let declared = mir.interfaces.iter().find(|interface| mir.symbols.get(interface.name) == Some(name));
                for field in fields {
                    let may_be_absent = declared.is_some_and(|interface| {
                        interface.decode_facts.iter().any(|facts| facts.name == field.name && facts.may_be_absent)
                    });
                    if field.visibility.is_own_property()
                        && may_be_absent
                        && matches!(mir.types.get(field.ty), Some(smelt_hir::Type::Optional(_)))
                    {
                        let key = mir.names.get(field.name).or_else(|| mir.symbols.get(field.name)).unwrap_or("field");
                        fn_writer.line(format!(
                            "__smelt_entries.retain(|(smelt_key, smelt_value)| !(smelt_key == {key:?} && matches!(smelt_value, SmeltUnknown::Undefined)));"
                        ));
                    }
                }
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
                fn_writer.line("smelt_finish_reference_view(__smelt_id, __smelt_view, __smelt_entries)");
            });
        },
    );
    Ok(())
}
