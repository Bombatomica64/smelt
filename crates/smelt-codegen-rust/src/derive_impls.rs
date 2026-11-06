//! Hand-written and gated `derive` impls of generated storage types, and the
//! predicates deciding which of them a type gets.

use crate::rust::RustExpr;
use std::collections::HashSet;

use smelt_hir::{Type, TypeId};
use smelt_mir::Mir;

use crate::classes::{
    NominalStorage, effective_class_fields, effective_interface_fields, type_holds_function,
};
use crate::emitter::{EmitContext, FunctionEmitter};
use crate::rust::{CodeWriter, RustIdent};
use crate::type_substitution::TypeSubstitution;
use crate::{EmitError, accessor_prelude};

/// Emits a manual `Default` impl for generated storage structs with callbacks.
///
/// Rust cannot derive `Default` for fields that contain `dyn FnMut`, but Smelt
/// still needs defaultable generated option bags and locale structs. This
/// helper reuses the function emitter's canonical default expression for each
/// field so callback defaults remain callable and ordinary fields match local
/// initialization behavior.
pub(crate) fn emit_default_impl_for_storage_type(
    writer: &mut CodeWriter,
    mir: &Mir,
    context: &EmitContext,
    accessor_owner: Option<smelt_hir::Symbol>,
    name: &str,
    impl_generics: &str,
    type_args: &str,
    fields: &[smelt_mir::MirField],
    phantom_args: &str,
    scoped_type_params: &HashSet<smelt_hir::Symbol>,
    type_param_idents: &[String],
) -> Result<(), EmitError> {
    // Each field's default expression is computed ONCE and used twice: for the
    // body, and to decide the impl's `where` clause.
    //
    // The clause has to follow the body rather than the field list. Most field
    // defaults are spelled out and need nothing of their type — a callback field
    // gets a constructed no-op closure, and `Rc<dyn Fn(..)>` is not `Default`, so
    // naming every field type demanded something false. The defaults that DELEGATE
    // (`Default::default()` for a bare `T`, `Node::default()` for a
    // reference-class field) are exactly the ones whose type must be `Default`,
    // and those cannot be covered by bounding `T` either:
    // `impl<T: Default> Default for TrieRouter<T>` could not prove
    // `Node<T>: Default`, whose own impl carries the full generated bound set.
    let mut field_defaults = Vec::with_capacity(fields.len());
    for field in fields {
        let field_name =
            RustIdent::new(mir.symbols.get(field.name).unwrap_or("field")).into_string();
        let default_value = FunctionEmitter::default_value_for_with_scoped_type_params(
            mir,
            context,
            field.ty,
            &TypeSubstitution::lexical(scoped_type_params),
        )
        .unwrap_or_else(|_| RustExpr::atom("Default::default()"));
        let field_type_text = FunctionEmitter::type_text_for_with_scoped_type_params(
            mir,
            context,
            field.ty,
            &TypeSubstitution::lexical(scoped_type_params),
        )?;
        // A CALLABLE slot is never a delegation, whatever its default text
        // looks like. The emitter always spells such a field's default out as a
        // constructed no-op closure, and `Rc<dyn Fn(..)>` implements no
        // `Default` for the clause to name — but the closure BODY may itself
        // delegate, because the slot's return type has to be produced from
        // somewhere. Hono's `Hono_1Inner` defaults `onError` to
        // `Rc::new(move |..| -> Hono_1<..> { Default::default() })`, whose
        // `default()` belongs to the RETURN type and not to the field type, so
        // a purely textual search sees a delegation and asks for
        // `Rc<dyn Fn(..)>: Default` — an unsatisfiable clause that makes the
        // whole `Default` impl unusable (E0599 at every `Inner::default()`).
        // The nested requirement is carried by the impl's own type-parameter
        // bounds instead (`classes::GENERATED_TYPE_PARAM_BOUNDS`), which is what
        // the return type's `Default` impl asks for.
        let is_callable_slot = type_holds_function(mir, field.ty, NominalStorage::Opaque);
        // An accessor field answers its type's default through a constant
        // getter; its storage is never a delegation.
        if accessor_owner.is_some_and(|owner| accessor_prelude::is_accessor_field(mir, owner, field.name)) {
            field_defaults.push((
                field_name,
                accessor_prelude::storage_type_text(&field_type_text),
                accessor_prelude::constant_expr(&default_value),
                true,
            ));
            continue;
        }
        field_defaults.push((field_name, field_type_text, default_value, is_callable_slot));
    }
    let delegating_field_types = field_defaults
        .iter()
        .filter(|(_, _, default_value, is_callable_slot)| {
            !is_callable_slot && default_expression_delegates(default_value)
        })
        .map(|(_, field_type_text, _, _)| field_type_text.clone())
        .collect::<Vec<_>>();
    let where_clause = if impl_generics.is_empty() {
        String::new()
    } else {
        trait_where_clause_for_field_types(&delegating_field_types, "Default", type_param_idents)
    };
    writer.block(
        format!("impl{impl_generics} Default for {name}{type_args}{where_clause}"),
        |impl_writer| {
            impl_writer.block("fn default() -> Self", |fn_writer| {
                fn_writer.block("Self", |self_writer| {
                    for (field_name, _, default_value, _) in &field_defaults {
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

/// Whether a rendered field default resolves through the field type's `Default`.
///
/// `Default::default()` (a bare type parameter) and `Type::default()` (a
/// generated class) both do; a spelled-out constructor such as
/// `SmeltList::new(Vec::<T>::new())` or a no-op closure does not. Only the
/// former need the field type bounded on the impl.
///
/// The call can be NESTED — a promise field defaults to
/// `SmeltFuture::resolved(SmeltUnion9::M0(Default::default()))`, whose `T:
/// Default` requirement reaches the impl through the field type all the same —
/// so the whole expression is searched rather than only its tail.
fn default_expression_delegates(default_value: &str) -> bool {
    default_value.contains("default()")
}

/// Emits a conservative `Debug` impl for storage structs that contain callbacks.
///
/// Callback trait objects cannot be formatted structurally, but parent option
/// bags still often derive `Debug` and therefore need nested storage records to
/// satisfy Rust bounds. The generated implementation names the storage type and
/// intentionally omits callable field internals.
pub(crate) fn emit_debug_impl_for_storage_type(
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

/// Build ` where <field type>: Trait, …` for a field-by-field impl.
///
/// A field-by-field body needs exactly one thing: every field's type implements
/// the trait. Bounding the FIELD TYPES says that and nothing else, where
/// bounding `T` says either too little (a `T`-typed field) or too much (a field
/// whose type is another generated class, which needs the whole generated bound
/// set). Duplicate field types are emitted once; an empty list yields no clause.
fn trait_where_clause_for_field_types(
    field_type_texts: &[String],
    bound: &str,
    type_param_idents: &[String],
) -> String {
    let mut seen = Vec::new();
    for field_type_text in field_type_texts {
        // A field type that mentions no type parameter is decided already:
        // `where String: Clone` is noise, and if such a bound were unsatisfied
        // the impl body would report it directly. Only a type whose
        // satisfiability depends on `T` needs saying.
        if !mentions_type_param(field_type_text, type_param_idents) {
            continue;
        }
        if !seen.contains(field_type_text) {
            seen.push(field_type_text.clone());
        }
    }
    if seen.is_empty() {
        return String::new();
    }
    let clauses = seen
        .iter()
        .map(|field_type_text| format!("{field_type_text}: {bound}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(" where {clauses}")
}

/// Render `<T: Bound, …>` — the bound `#[derive(Bound)]` would have imposed.
///
/// Used together with a field-type `where` clause, never instead of it. The
/// clause alone is not enough: a field default can require the bound through an
/// expression the field's TYPE does not mention, as a promise field defaulting
/// to `SmeltFuture::resolved(SmeltUnion9::M0(Default::default()))` does when the
/// union renders without its argument. Since a derive would have imposed exactly
/// this bound, adding it can never leave a consumer worse off than the derive it
/// replaces.
pub(crate) fn derive_generics_text(type_param_idents: &[String], bound: &str) -> String {
    if type_param_idents.is_empty() {
        return String::new();
    }
    let params = type_param_idents
        .iter()
        .map(|ident| format!("{ident}: {bound}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("<{params}>")
}

/// Whether a rendered type mentions one of these type parameters as a whole
/// identifier, so `T` matches in `Node<T>` but not inside `Trie` or `TABLE`.
fn mentions_type_param(type_text: &str, type_param_idents: &[String]) -> bool {
    type_param_idents.iter().any(|ident| {
        type_text
            .match_indices(ident.as_str())
            .any(|(at, matched)| {
                let before = type_text[..at].chars().next_back();
                let after = type_text[at + matched.len()..].chars().next();
                let boundary = |ch: Option<char>| {
                    ch.is_none_or(|ch| !ch.is_alphanumeric() && ch != '_')
                };
                boundary(before) && boundary(after)
            })
    })
}

/// Emit a field-by-field `Clone` for a generic value class.
///
/// `#[derive(Clone)]` would generate `impl<T: Clone> Clone`, whose single bound
/// cannot satisfy a field whose type is another generated class — those carry
/// the full generated bound set (see `class_impl_generics_text`). The impl is
/// spelled out with bare parameters plus a field-type `where` clause instead.
pub(crate) fn emit_clone_impl_for_value_class(
    writer: &mut CodeWriter,
    name: &str,
    impl_generics: &str,
    type_args: &str,
    field_names: &[String],
    field_type_texts: &[String],
    type_param_idents: &[String],
    has_phantom: bool,
) {
    let where_clause = trait_where_clause_for_field_types(field_type_texts, "Clone", type_param_idents);
    writer.block(
        format!("impl{impl_generics} Clone for {name}{type_args}{where_clause}"),
        |impl_writer| {
            impl_writer.block("fn clone(&self) -> Self", |fn_writer| {
                fn_writer.block(name, |init_writer| {
                    for field_name in field_names {
                        init_writer.line(format!("{field_name}: self.{field_name}.clone(),"));
                    }
                    if has_phantom {
                        init_writer.line("_smelt_phantom: ::std::marker::PhantomData,");
                    }
                });
            });
        },
    );
}

/// Emit a field-by-field `PartialEq` for a generic value class.
///
/// The `Clone` reasoning above applies unchanged: a derived `PartialEq` carries
/// only `T: PartialEq`, which a generated-class field does not satisfy. The
/// phantom field is skipped — `PhantomData` compares equal to itself and says
/// nothing about the value.
pub(crate) fn emit_partial_eq_impl_for_value_class(
    writer: &mut CodeWriter,
    name: &str,
    impl_generics: &str,
    type_args: &str,
    field_names: &[String],
    field_type_texts: &[String],
    type_param_idents: &[String],
) {
    let where_clause = trait_where_clause_for_field_types(field_type_texts, "PartialEq", type_param_idents);
    writer.block(
        format!("impl{impl_generics} PartialEq for {name}{type_args}{where_clause}"),
        |impl_writer| {
            impl_writer.block("fn eq(&self, other: &Self) -> bool", |fn_writer| {
                if field_names.is_empty() {
                    fn_writer.line("true");
                    return;
                }
                let comparisons = field_names
                    .iter()
                    .map(|field_name| format!("self.{field_name} == other.{field_name}"))
                    .collect::<Vec<_>>()
                    .join(" && ");
                fn_writer.line(comparisons);
            });
        },
    );
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
pub(crate) fn type_supports_partial_eq(
    mir: &Mir,
    context: &EmitContext,
    ty: TypeId,
    seen: &mut Vec<smelt_hir::Symbol>,
) -> bool {
    match mir.types.get(ty) {
        Some(
            Type::Function(_)
            | Type::Future(_)
            | Type::Generator { .. }
            | Type::GeneratorResult { .. },
        ) => false,
        Some(Type::List(item) | Type::Set(item) | Type::Optional(item)) => {
            type_supports_partial_eq(mir, context, *item, seen)
        }
        // A map keyed by a TYPE PARAMETER is a `SmeltJsMap<P, _>`, whose
        // `PartialEq` needs `P: SmeltJsKeyEq`. A `#[derive(PartialEq)]` only
        // adds `P: PartialEq`, so the derive does not compile (E0369); the
        // record is not derivably comparable.
        Some(Type::Dict(key, _) | Type::JsMap(key, _))
            if matches!(mir.types.get(*key), Some(Type::TypeParam { .. })) =>
        {
            false
        }
        Some(Type::Dict(key, value) | Type::JsMap(key, value)) => {
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
            let Some(class) = crate::classes::find_class(mir, name) else {
                // A shape (interface / synthetic object-literal shape) is a
                // by-value struct that derives `PartialEq` under this same rule,
                // so a field holding one is comparable exactly when the shape's
                // own fields are. Recursing here is what keeps the two derives in
                // agreement: a shape storing a callback derives no `PartialEq`,
                // and a record storing THAT shape must not derive one either.
                if let Some(interface) = mir
                    .interfaces
                    .iter()
                    .find(|candidate| candidate.name == name)
                {
                    if seen.contains(&name) {
                        return true;
                    }
                    seen.push(name);
                    let comparable = effective_interface_fields(mir, interface)
                        .iter()
                        .all(|field| type_supports_partial_eq(mir, context, field.ty, seen));
                    seen.pop();
                    return comparable;
                }
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
            | Type::FixedInt(_)
            | Type::BigInt
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
pub(crate) fn class_is_json_serializable(mir: &Mir, class: &smelt_mir::MirClass) -> bool {
    let mut seen = Vec::new();
    effective_class_fields(mir, class)
        .iter()
        .all(|field| type_is_json_serializable(mir, field.ty, &mut seen))
}

/// Return whether all stored fields in an interface are JSON-compatible.
pub(crate) fn interface_is_json_serializable(mir: &Mir, interface: &smelt_mir::MirInterface) -> bool {
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
                if let Some(class) = crate::classes::find_class(mir, *name) {
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
