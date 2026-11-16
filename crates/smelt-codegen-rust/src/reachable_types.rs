//! The types a program can actually spell: every type position of its MIR,
//! closed over the types those types are built from.
//!
//! The MIR type table ([`smelt_hir::TypeInterner`]) is not that set. The
//! frontends intern types they only compare against (`unknown`, `never`, the
//! union a narrowing step computed and then refined away), and erasure
//! bookkeeping interns `Type::Unknown` in nearly every program. Scanning the
//! whole table made the runtime demand pull the `SmeltUnknown` carrier and the
//! `JsString` runtime into wholly typed programs.
//!
//! Union enums are still declared from the whole table: the emitter derives
//! unions no position holds (the union of a tuple union's element types at
//! `pair[0]`) and looks them up there. Those enums are written against the
//! carrier, so they are declared only in a program that carries it (any
//! reachable union demands it), and such a program's demand also covers the
//! types inside every union of the table ([`ReachableTypes::of_with_unions`]).
//!
//! The seeds are [`Mir::visit_types`], the single list of every MIR position
//! that holds a type (locals, signatures, class and interface fields, globals,
//! op render types, casts and decode targets). The closure adds each type's
//! [`smelt_hir::Type::children`], so the item of a list, the arms of a union,
//! a class's type arguments and a function type's parameters are included, and
//! a generic class or interface spelled with arguments adds its fields' types at
//! those arguments (`ValueSetter<number>`'s `value: number | string`), which its
//! constructor and field reads render.
#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible items are shared with the runtime demand and the emitter"
)]

use smelt_hir::TypeId;
use smelt_mir::Mir;

/// Which type ids of `mir`'s type table the program can spell, indexed by
/// [`TypeId`]. See the module docs.
#[derive(Debug, Clone, Default)]
pub(crate) struct ReachableTypes {
    /// One flag per entry of the type table.
    reachable: Vec<bool>,
}

impl ReachableTypes {
    /// The reachable set of `mir`.
    #[must_use]
    pub(crate) fn of(mir: &Mir) -> Self {
        let mut pending = Vec::new();
        mir.visit_types(&mut |ty| pending.push(ty));
        Self::closed(mir, pending)
    }

    /// [`Self::of`], with every union in the type table as a seed too: what a
    /// program that carries the erased runtime declares enums for (see the
    /// module docs).
    pub(crate) fn of_with_unions(mir: &Mir) -> Self {
        let mut pending = Vec::new();
        mir.visit_types(&mut |ty| pending.push(ty));
        pending.extend(
            mir.types
                .all()
                .iter()
                .enumerate()
                .filter(|(_, ty)| matches!(ty, smelt_hir::Type::Union(_)))
                .filter_map(|(at, _)| u32::try_from(at).ok().map(TypeId)),
        );
        Self::closed(mir, pending)
    }

    /// The closure of `pending` over child types and generic instantiations.
    fn closed(mir: &Mir, mut pending: Vec<TypeId>) -> Self {
        let mut reachable = vec![false; mir.types.all().len()];
        while let Some(ty) = pending.pop() {
            let Some(slot) = index(ty).and_then(|at| reachable.get_mut(at)) else {
                continue;
            };
            if *slot {
                continue;
            }
            *slot = true;
            if let Some(resolved) = mir.types.get(ty) {
                pending.extend(resolved.children());
                if let smelt_hir::Type::Class { name, args } = resolved
                    && !args.is_empty()
                {
                    pending.extend(instantiated_field_types(mir, *name, args));
                }
            }
        }
        Self { reachable }
    }

    /// The reachable type ids, in type-table order.
    pub(crate) fn ids(&self) -> impl Iterator<Item = TypeId> + '_ {
        self.reachable
            .iter()
            .enumerate()
            .filter(|(_, reachable)| **reachable)
            .filter_map(|(at, _)| u32::try_from(at).ok().map(TypeId))
    }
}

/// The field types of class or interface `name` with its type parameters bound
/// to `args`, as the table already interned them (codegen holds `&Mir` and
/// renders an instantiation through the same lookup). A field type whose
/// instantiation was never interned is its declaration, already reachable.
fn instantiated_field_types(mir: &Mir, name: smelt_hir::Symbol, args: &[TypeId]) -> Vec<TypeId> {
    let (params, fields) = if let Some(class) = crate::classes::find_class(mir, name) {
        (&class.type_params, &class.fields)
    } else if let Some(interface) = mir.interface_named(name) {
        (&interface.type_params, &interface.fields)
    } else {
        return Vec::new();
    };
    let substitutions = params
        .iter()
        .zip(args)
        .map(|(param, arg)| (param.name, *arg))
        .collect::<std::collections::HashMap<_, _>>();
    fields
        .iter()
        .map(|field| {
            crate::generic_bindings::substitute_existing_type(mir, field.ty, &substitutions)
        })
        .collect()
}

/// The table index of `ty`.
fn index(ty: TypeId) -> Option<usize> {
    usize::try_from(ty.0).ok()
}
