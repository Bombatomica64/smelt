//! Helpers for emitting Rust class storage, generics, and inheritance surfaces.
#![expect(
    clippy::redundant_pub_crate,
    reason = "class helpers are shared across sibling emitter modules"
)]

use smelt_mir::{Mir, MirClass, MirField, MirInterface};

use crate::{EmitError, emitter::FunctionEmitter, rust::RustIdent};

/// Return the sanitized Rust storage type name for a MIR class.
///
/// Class names can originate from TypeScript or Python identifiers, so this
/// function is the single place class storage emission applies Rust identifier
/// sanitization before combining names with generic arguments.
pub(crate) fn class_name_text(mir: &Mir, class: &MirClass) -> Result<String, EmitError> {
    Ok(RustIdent::new(
        mir.symbols
            .get(class.name)
            .ok_or_else(|| EmitError::new("class has unknown symbol"))?,
    )
    .into_string())
}

/// Render the generic parameter declaration suffix for a class, such as `<T>`.
///
/// The returned text is empty for non-generic classes so callers can append it
/// directly after a struct, trait, or impl target name without extra branching.
pub(crate) fn class_type_params_text(mir: &Mir, class: &MirClass) -> Result<String, EmitError> {
    if class.type_params.is_empty() {
        return Ok(String::new());
    }
    let params = class
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
    Ok(format!("<{params}>"))
}

/// Render the generic parameter declaration suffix for an interface.
///
/// Interface type parameters must survive into Rust storage types because
/// function signatures can refer to instantiated interface names such as
/// `ContextOptions<SmeltUnknown>`. The returned suffix mirrors class generic
/// emission and is empty for non-generic interfaces.
pub(crate) fn interface_type_params_text(
    mir: &Mir,
    interface: &MirInterface,
) -> Result<String, EmitError> {
    if interface.type_params.is_empty() {
        return Ok(String::new());
    }
    let params = interface
        .type_params
        .iter()
        .map(|param| {
            mir.symbols
                .get(param.name)
                .map(|name| RustIdent::new(name).into_string())
                .ok_or_else(|| EmitError::new("interface type parameter has unknown symbol"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format!("<{}>", params.join(", ")))
}

/// Render the generic argument suffix for a class, such as `<T>`.
///
/// This mirrors [`class_type_params_text`] for places where the generated Rust
/// references an already-declared class type rather than declaring parameters.
pub(crate) fn class_type_args_text(mir: &Mir, class: &MirClass) -> Result<String, EmitError> {
    class_type_params_text(mir, class)
}

/// Render the generic parameter suffix used on inherent impl blocks.
///
/// The helper is intentionally separate from struct rendering because impl
/// blocks are the first place bounds may be introduced as class codegen grows.
pub(crate) fn class_impl_generics_text(mir: &Mir, class: &MirClass) -> Result<String, EmitError> {
    class_type_params_text(mir, class)
}

/// Render the Rust trait name for a class inheritance surface.
///
/// Trait names currently match the source class name because Rust permits a
/// trait and struct with the same identifier in the type namespace only when
/// separated by context is not possible, so callers should avoid emitting both
/// for the same concrete class until trait emission is expanded.
pub(crate) fn class_trait_name_text(mir: &Mir, class: &MirClass) -> Result<String, EmitError> {
    class_name_text(mir, class)
}

/// Render an owned trait-object type for a class surface.
///
/// This is reserved for base-typed polymorphic storage and parameters. Current
/// emission remains concrete unless a later lowering stage marks trait objects
/// as required.
#[expect(
    dead_code,
    reason = "trait-object lowering is only emitted for later polymorphic cases"
)]
pub(crate) fn class_trait_object_type_text(
    mir: &Mir,
    class: &MirClass,
) -> Result<String, EmitError> {
    Ok(format!(
        "Box<dyn {}{}>",
        class_trait_name_text(mir, class)?,
        class_type_args_text(mir, class)?
    ))
}

/// Return the flattened field layout for a class.
///
/// Smelt stores subclass values with inherited fields first and own fields
/// after them. This helper follows the single-inheritance chain and leaves type
/// substitution to earlier lowering phases until MIR grows canonical layout
/// substitution metadata. When a subclass redeclares a field from its base
/// class, the subclass field replaces the inherited slot so Rust struct storage
/// stays valid and matches the effective source member surface.
pub(crate) fn effective_class_fields(mir: &Mir, class: &MirClass) -> Vec<MirField> {
    let mut fields = class
        .base
        .and_then(|base| mir.classes.iter().find(|candidate| candidate.name == base))
        .map(|base| effective_class_fields(mir, base))
        .unwrap_or_default();
    for field in &class.fields {
        if let Some(existing) = fields
            .iter_mut()
            .find(|candidate| candidate.name == field.name)
        {
            *existing = field.clone();
        } else {
            fields.push(field.clone());
        }
    }
    fields
}

/// Return the Rust-valid field layout for an interface.
///
/// TypeScript interface inheritance and utility-type expansion can present the
/// same source property more than once. Rust structs cannot contain duplicate
/// field identifiers, so codegen keeps the last field for each sanitized Rust
/// name. This mirrors source member lookup where later, more specific
/// declarations describe the effective surface while keeping generated storage
/// valid.
pub(crate) fn effective_interface_fields(mir: &Mir, interface: &MirInterface) -> Vec<MirField> {
    let mut fields = Vec::new();
    for field in &interface.fields {
        let field_name = mir
            .symbols
            .get(field.name)
            .map(RustIdent::new)
            .map_or_else(|| "field".to_owned(), RustIdent::into_string);
        if let Some(existing) = fields.iter_mut().find(|candidate: &&mut MirField| {
            mir.symbols
                .get(candidate.name)
                .map(RustIdent::new)
                .map_or_else(|| "field".to_owned(), RustIdent::into_string)
                == field_name
        }) {
            *existing = field.clone();
        } else {
            fields.push(field.clone());
        }
    }
    fields
}

/// Return inherited abstract method signatures required by a class.
///
/// The list walks base classes first so generated trait surfaces have stable,
/// deterministic ordering that matches the flattened field layout.
pub(crate) fn inherited_trait_methods(mir: &Mir, class: &MirClass) -> Vec<smelt_hir::MethodSig> {
    let mut methods = class
        .base
        .and_then(|base| mir.classes.iter().find(|candidate| candidate.name == base))
        .map(|base| inherited_trait_methods(mir, base))
        .unwrap_or_default();
    methods.extend(class.abstract_methods.clone());
    methods
}

/// Render a HIR class type with its Rust generic arguments.
#[expect(
    dead_code,
    reason = "standalone class type rendering is reserved for trait objects"
)]
pub(crate) fn class_type_text(
    mir: &Mir,
    name: smelt_hir::Symbol,
    args: &[smelt_hir::TypeId],
) -> Result<String, EmitError> {
    let name = RustIdent::new(
        mir.symbols
            .get(name)
            .ok_or_else(|| EmitError::new("class type has unknown symbol"))?,
    )
    .into_string();
    if args.is_empty() {
        return Ok(name);
    }
    let args = args
        .iter()
        .map(|arg| FunctionEmitter::type_text_for(mir, *arg))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    Ok(format!("{name}<{args}>"))
}
