//! Interning of the field types a generic record's instantiations need.
//!
//! A generated storage struct keeps its Rust generic parameters, but almost
//! every adapter the backend emits works against an *instantiated* record type
//! such as `InitLike<f64>`: a union arm's payload, a record-to-struct
//! projection, a field read through a narrowed union arm. To emit those the
//! backend substitutes the instantiation's type arguments into the declared
//! field types (`status?: T` at `T = f64` becomes `Option<f64>`), and it does
//! that substitution by *looking up* the substituted type in the MIR type
//! table — the emitter holds `&Mir` and cannot intern.
//!
//! Nothing guaranteed that lookup would succeed. `Optional<Float>` exists in
//! the table only if some other part of the program happened to spell it, so a
//! generic interface whose only instantiated field type was unique to that
//! instantiation silently kept its declaration-time `Type::TypeParam`: the
//! enum arm rendered `InitLike<f64>` while the adapter that builds it rendered
//! the field at `SmeltUnknown`, and the generated crate did not compile
//! (E0308, `expected Option<f64>, found Option<SmeltUnknown>`).
//!
//! This pass closes that gap once, in the one place that owns the type table:
//! for every instantiated record type in the program it interns the
//! substituted form of each of its field types, following base classes and
//! extended interfaces so an inherited field is instantiated too. It adds
//! types and never rewrites one, so no existing id changes meaning.

use crate::Mir;
use smelt_hir::{FunctionType, Symbol, Type, TypeId};
use std::collections::{HashMap, HashSet};

/// One record instantiation: the record's name and its type arguments.
type Instantiation = (Symbol, Vec<TypeId>);

/// What one named record declares, as this pass needs to see it.
struct RecordShape {
    /// The names of the record's declared type parameters, in order.
    type_params: Vec<Symbol>,
    /// The declared types of the record's fields.
    field_types: Vec<TypeId>,
    /// Each parent record and the type arguments applied to it.
    heritage: Vec<Instantiation>,
}

/// Intern the substituted field types of every instantiated generic record.
///
/// Runs to a fixpoint: substituting a field type can mint a *new* record
/// instantiation (`Wrapper<T>` inside `Outer<T>` at `T = f64`), whose own
/// fields then need instantiating as well. Each `(name, args)` pair is
/// processed once, so a self-referential generic record terminates.
pub(in crate::lower) fn intern_generic_record_instantiations(mir: &mut Mir) {
    let mut done: HashSet<Instantiation> = HashSet::new();
    loop {
        let pending = pending_instantiations(mir, &done);
        if pending.is_empty() {
            return;
        }
        for instantiation in pending {
            done.insert(instantiation.clone());
            instantiate_record(mir, &instantiation);
        }
    }
}

/// Collect the instantiated record types in the table that are not done yet.
fn pending_instantiations(mir: &Mir, done: &HashSet<Instantiation>) -> Vec<Instantiation> {
    let mut pending = Vec::new();
    for ty in mir.types.all() {
        if let Type::Class { name, args } = ty
            && !args.is_empty()
        {
            let key = (*name, args.clone());
            if !done.contains(&key) && !pending.contains(&key) {
                pending.push(key);
            }
        }
    }
    pending
}

/// Intern one record instantiation's substituted field and heritage types.
fn instantiate_record(mir: &mut Mir, (name, args): &Instantiation) {
    let Some(shape) = record_shape(mir, *name) else {
        return;
    };
    let substitutions: HashMap<Symbol, TypeId> = shape
        .type_params
        .into_iter()
        .zip(args.iter().copied())
        .collect();
    if substitutions.is_empty() {
        return;
    }
    for field_ty in shape.field_types {
        substitute_and_intern(mir, field_ty, &substitutions);
    }
    // A base class or extended interface contributes fields to the same
    // record, and codegen folds them in against the *parent's* instantiation
    // (`extends Base<T>` at `T = f64` is `Base<f64>`). Interning the parent
    // instantiation puts it in the table, and the fixpoint loop then
    // instantiates the parent's own fields.
    for (parent, parent_args) in shape.heritage {
        let substituted = parent_args
            .into_iter()
            .map(|arg| substitute_and_intern(mir, arg, &substitutions))
            .collect();
        mir.types.intern(Type::Class {
            name: parent,
            args: substituted,
        });
    }
}

/// The declared type parameters, field types and heritage of a named record.
///
/// Interfaces are checked first because a name resolves to at most one of the
/// two tables; the values are cloned so the caller can mutate the type table.
fn record_shape(mir: &Mir, name: Symbol) -> Option<RecordShape> {
    if let Some(interface) = mir.interfaces.iter().find(|item| item.name == name) {
        return Some(RecordShape {
            type_params: interface
                .type_params
                .iter()
                .map(|param| param.name)
                .collect(),
            field_types: interface.fields.iter().map(|field| field.ty).collect(),
            heritage: interface
                .extends
                .iter()
                .map(|parent| (parent.parent, parent.args.clone()))
                .collect(),
        });
    }
    let class = mir.classes.iter().find(|item| item.name == name)?;
    Some(RecordShape {
        type_params: class.type_params.iter().map(|param| param.name).collect(),
        field_types: class.fields.iter().map(|field| field.ty).collect(),
        heritage: class
            .base
            .map(|base| vec![(base, class.base_args.clone())])
            .unwrap_or_default(),
    })
}

/// Substitute type parameters through `ty`, interning every rewritten layer.
///
/// Mirrors the emitter's read-only `substitute_type_params_in_type`, except
/// that this side may intern: a substituted layer that does not exist yet is
/// added rather than silently answering the unsubstituted type.
fn substitute_and_intern(
    mir: &mut Mir,
    ty: TypeId,
    substitutions: &HashMap<Symbol, TypeId>,
) -> TypeId {
    let Some(kind) = mir.types.get(ty).cloned() else {
        return ty;
    };
    let substituted = match kind {
        Type::TypeParam { name } => return substitutions.get(&name).copied().unwrap_or(ty),
        Type::Optional(inner) => Type::Optional(substitute_and_intern(mir, inner, substitutions)),
        Type::List(item) => Type::List(substitute_and_intern(mir, item, substitutions)),
        Type::Set(item) => Type::Set(substitute_and_intern(mir, item, substitutions)),
        Type::Future(item) => Type::Future(substitute_and_intern(mir, item, substitutions)),
        Type::Dict(key, value) => Type::Dict(
            substitute_and_intern(mir, key, substitutions),
            substitute_and_intern(mir, value, substitutions),
        ),
        Type::Tuple(items) => Type::Tuple(
            items
                .into_iter()
                .map(|item| substitute_and_intern(mir, item, substitutions))
                .collect(),
        ),
        Type::Union(items) => Type::Union(
            items
                .into_iter()
                .map(|item| substitute_and_intern(mir, item, substitutions))
                .collect(),
        ),
        Type::Class { name, args } => Type::Class {
            name,
            args: args
                .into_iter()
                .map(|arg| substitute_and_intern(mir, arg, substitutions))
                .collect(),
        },
        Type::Function(function) => Type::Function(FunctionType {
            params: function
                .params
                .iter()
                .map(|param| substitute_and_intern(mir, *param, substitutions))
                .collect(),
            return_ty: substitute_and_intern(mir, function.return_ty, substitutions),
            ..function
        }),
        other => other,
    };
    mir.types.intern(substituted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MirField, MirInterface};
    use smelt_hir::{
        OriginalNameTable, Span, SymbolInterner, TypeInterner, TypeParamDef, Visibility,
    };

    /// A generic interface instantiated at a concrete argument must leave its
    /// substituted field types in the table.
    ///
    /// Regression: the emitter's field-type substitution can only *look up* a
    /// substituted type, so `status?: T` at `T = f64` silently stayed
    /// `Option<TypeParam>` while the instantiated struct rendered
    /// `Option<f64>`. Concrete types, unions and scoped generics cannot stand
    /// in for each other here — the two renderings of the *same* field must
    /// agree, so the substituted type has to exist.
    #[test]
    fn interns_the_substituted_field_types_of_an_instantiation() {
        let mut types = TypeInterner::default();
        let mut symbols = SymbolInterner::default();
        let float = types.intern(Type::Float);
        let param_name = symbols.intern("T");
        let param = types.intern(Type::TypeParam { name: param_name });
        let declared_field = types.intern(Type::Optional(param));
        let interface_name = symbols.intern("InitLike");
        types.intern(Type::Class {
            name: interface_name,
            args: vec![float],
        });
        let mut mir = Mir::new(types, symbols, OriginalNameTable::default());
        mir.interfaces.push(MirInterface {
            name: interface_name,
            type_params: vec![TypeParamDef {
                name: param_name,
                constraint: None,
                default: None,
                span: Span::new(smelt_hir::FileId(0), 0, 0),
            }],
            extends: Vec::new(),
            fields: vec![MirField {
                name: mir.symbols.intern("status"),
                ty: declared_field,
                visibility: Visibility::Public,
            }],
            methods: Vec::new(),
        });

        assert!(
            !mir.types
                .all()
                .contains(&Type::Optional(float)),
            "the substituted field type must be absent before the pass runs"
        );

        intern_generic_record_instantiations(&mut mir);

        assert!(
            mir.types.all().contains(&Type::Optional(float)),
            "Optional<Float> must be interned for InitLike<f64>"
        );
    }
}
