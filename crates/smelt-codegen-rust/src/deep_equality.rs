//! Typed deep equality: which records compare field with field at their own
//! type.
//!
//! # The defect this closes
//!
//! `toEqual` and the structural `==` compare own properties. A generated
//! record's `PartialEq` answers another question where the record is a
//! JavaScript object with identity: a reference record compares its cell's
//! address (`===`), and a by-value struct's derived `PartialEq` asks that of
//! every record it holds. A *live* record already compares through its views
//! (`emitter::live_view`, `smelt_live_deep_eq`). These did not:
//!
//! ```text
//! interface Box<T> { value: T }          // generic, written after construction
//! expect([box(1)]).toEqual([box(1)])     // vitest: equal; was: unequal (two cells)
//! interface Team { lead: Person }        // a by-value struct holding a shared record
//! expect([team()]).toEqual([team()])     // vitest: equal; was: unequal (two `lead` cells)
//! ```
//!
//! # The rule
//!
//! A record that a deep comparison reaches gets a generated
//! `impl SmeltDeepEq` (`smelt-runtime/src/runtime/deep_eq.rs`,
//! [`crate::emitter::deep_equality`]): the trait a hand-porting team would
//! derive for "equal property by property", at the record's own type.
//!
//! - **Reached** ([`DeepEquality::plan`]): the type of both operands of a deep
//!   `==`/`!=` ([`BinOp::Eq`], [`BinOp::NotEq`]: `toEqual` and the structural
//!   comparisons, never `===`), followed through the containers that
//!   comparison walks (list, optional, tuple, string- or property-keyed
//!   dictionary), through a reached record's own fields, and through a record
//!   type's arguments.
//! - **A by-value struct** compares field with field.
//! - **A reference record that is not live** (a generic one) is one object:
//!   equal to itself without being read, otherwise field with field, with the
//!   pair of cells entered in the comparison's in-progress set. A cell met
//!   again while it is being compared is equal exactly when it is paired with
//!   the cell it is being compared to: the rule of vitest's `equals`, under
//!   which two cycles are equal when they close at the same depth.
//! - **A live record** keeps the comparison of its views, which also sees a
//!   property a view added or deleted. It gets an impl (delegating to that
//!   comparison) only where it is a record type's ARGUMENT, since a generic
//!   record's impl can only compare a field of type `T` through the trait.
//!   That includes a record seen as another record type (a view target,
//!   [`crate::structural_views`]): its erasure is the concrete object's, so
//!   two handles compare all the properties of the objects they stand for.
//! - **A callable record** compares by identity, as JavaScript compares
//!   functions: the cell of a reference one, the function it holds of a
//!   by-value one (which has no `PartialEq` at all, so a typed list of them
//!   could not be compared before).
//!
//! # Requirements
//!
//! Behavior: for two values of one type, the comparison answers what vitest's
//! `toEqual` answers for the same source values, for every case the fixture
//! `assertion-parity/typed_deep_equality.test.ts` holds (each `T_`/`F_` case
//! is checked against vitest). A number leaf follows the rule of every nested
//! number (`NaN` equals `NaN`).
//!
//! Type preservation: both operands keep their types. No operand, field or
//! type argument is erased to be compared; the only erased comparison is the
//! live record's own view comparison, which predates this module.
//!
//! # What is not covered (each keeps its `PartialEq`, as before)
//!
//! - a record with a field this module does not compare at its type: a
//!   function-typed field (identity of a callback has no single spelling
//!   across its representations), a promise or generator, a dictionary keyed
//!   by a type parameter, or a type parameter inside a container the walk
//!   does not enter ([`DeepEquality::field_compares`]);
//! - a generic record at a type argument the trait has no impl for
//!   ([`DeepEquality::argument_compares`]): anything but a number, string,
//!   boolean, a list or optional of those, or a record with an impl. That
//!   includes a type parameter of an enclosing generic FUNCTION, which carries
//!   no bound to compare through;
//! - a class instance: its own properties are not its declared fields alone
//!   (accessors and methods live on the prototype), and a polymorphic handle
//!   would need dynamic dispatch.
//!
//! [`BinOp::Eq`]: smelt_hir::BinOp::Eq
//! [`BinOp::NotEq`]: smelt_hir::BinOp::NotEq

use std::collections::HashSet;

use smelt_hir::{Symbol, Type, TypeId};
use smelt_mir::{Mir, MirInterface, Operand, Place, Rvalue, Statement};

/// Where a comparison expression is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scope {
    /// A function body: a type parameter there carries no `SmeltDeepEq` bound.
    Body,
    /// A record's own `impl SmeltDeepEq`, which bounds the record's type
    /// parameters by the trait.
    Impl,
}

/// The records of a program that get an `impl SmeltDeepEq` (see the module
/// docs).
#[derive(Debug, Default)]
pub(crate) struct DeepEquality {
    /// The records with an impl.
    records: HashSet<Symbol>,
    /// Whether a deep comparison reaches, through a record's field or a type
    /// argument, a shape that compares through its views.
    reaches_view_record: bool,
}

/// The object shape named `name`: an interface or lowered type literal, not a
/// class.
fn shape(mir: &Mir, name: Symbol) -> Option<&MirInterface> {
    if mir.has_class(name) {
        return None;
    }
    mir.interfaces
        .iter()
        .find(|interface| interface.name == name)
}

/// Whether `interface` compares through its views: the shapes
/// `emitter::live_view::live_records` makes live records in a program that
/// carries the erased runtime.
fn compares_through_views(mir: &Mir, references: &HashSet<Symbol>, interface: &MirInterface) -> bool {
    interface.type_params.is_empty()
        && references.contains(&interface.name)
        && !crate::classify::is_callable_shape(mir, interface)
        && !crate::classify::has_accessor_fields(mir, interface)
}

/// Whether `ty` names a type parameter anywhere inside it.
fn mentions_type_param(mir: &Mir, ty: TypeId, depth: u8) -> bool {
    if depth > 32 {
        return true;
    }
    match mir.types.get(ty) {
        Some(Type::TypeParam { .. }) => true,
        Some(other) => other
            .children()
            .any(|child| mentions_type_param(mir, child, depth.saturating_add(1))),
        None => false,
    }
}

/// Whether the comparison walks into a dictionary keyed by `key`: a string- or
/// property-keyed one (`binary_ops::structural_leaf_equality_expr`).
fn walks_dict_keyed_by(mir: &Mir, key: TypeId) -> bool {
    mir.types
        .get(key)
        .is_some_and(|key| key.is_text() || *key == Type::PropKey)
}

/// The shapes the deep comparisons of a program reach, before any is checked
/// for whether its fields can be compared.
#[derive(Default)]
struct Reached {
    /// Shapes compared field with field.
    fieldwise: HashSet<Symbol>,
    /// Shapes compared through their views that are a record type's argument.
    view_arguments: HashSet<Symbol>,
    /// Callable shapes, compared by identity.
    callable: HashSet<Symbol>,
    /// Whether a shape compared through its views was reached below a record.
    view_record_below_a_record: bool,
}

impl Reached {
    /// Follow `ty` as the comparison does. `below_record` is whether the walk
    /// is inside a reached record's field or argument, and `as_argument`
    /// whether it is inside a record type's argument.
    fn reach(
        &mut self,
        mir: &Mir,
        references: &HashSet<Symbol>,
        ty: TypeId,
        below_record: bool,
        as_argument: bool,
        depth: u8,
    ) {
        if depth > 32 {
            return;
        }
        let deeper = depth.saturating_add(1);
        match mir.types.get(ty) {
            Some(Type::List(item) | Type::Optional(item)) => {
                self.reach(mir, references, *item, below_record, as_argument, deeper);
            }
            Some(Type::Tuple(items)) => {
                for item in items {
                    self.reach(mir, references, *item, below_record, as_argument, deeper);
                }
            }
            Some(Type::Dict(key, value)) if walks_dict_keyed_by(mir, *key) => {
                self.reach(mir, references, *value, below_record, as_argument, deeper);
            }
            Some(Type::Class { name, args }) => {
                let Some(interface) = shape(mir, *name) else {
                    return;
                };
                if compares_through_views(mir, references, interface) {
                    self.view_record_below_a_record |= below_record;
                    if as_argument {
                        self.view_arguments.insert(*name);
                    }
                } else if crate::classify::is_callable_shape(mir, interface) {
                    self.callable.insert(*name);
                } else if self.fieldwise.insert(*name) {
                    for field in crate::classes::effective_interface_fields(mir, interface) {
                        self.reach(mir, references, field.ty, true, false, deeper);
                    }
                }
                for arg in args {
                    self.reach(mir, references, *arg, true, true, deeper);
                }
            }
            _ => {}
        }
    }
}

impl DeepEquality {
    /// Plans the impls of `mir`, whose reference shapes and classes are
    /// `references` (see the module docs).
    pub(crate) fn plan(mir: &Mir, references: &HashSet<Symbol>) -> Self {
        let mut reached = Reached::default();
        let mut scan = |locals: &[smelt_mir::LocalDecl], blocks: &[smelt_mir::BasicBlock]| {
            let ty_of = |operand: &Operand| match operand {
                Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => {
                    usize::try_from(local.0)
                        .ok()
                        .and_then(|index| locals.get(index))
                        .map(|decl| decl.ty)
                }
                _ => None,
            };
            for statement in blocks.iter().flat_map(|block| &block.statements) {
                let Statement::Assign {
                    value:
                        Rvalue::Binary {
                            op: smelt_hir::BinOp::Eq | smelt_hir::BinOp::NotEq,
                            lhs,
                            rhs,
                        },
                    ..
                } = statement
                else {
                    continue;
                };
                // Only two operands of ONE type compare at that type
                // (`binary_ops::nested_float_equality_expr`).
                if let (Some(left), Some(right)) = (ty_of(lhs), ty_of(rhs))
                    && left == right
                {
                    reached.reach(mir, references, left, false, false, 0);
                }
            }
        };
        for function in &mir.functions {
            scan(&function.locals, &function.blocks);
        }
        for closure in &mir.closures {
            scan(&closure.locals, &closure.blocks);
        }
        let mut plan = Self {
            records: reached
                .fieldwise
                .iter()
                .chain(&reached.view_arguments)
                .chain(&reached.callable)
                .copied()
                .collect(),
            reaches_view_record: reached.view_record_below_a_record,
        };
        // A record with a field that cannot be compared at its type keeps its
        // `PartialEq`. Dropping one can leave another's field without an impl
        // to call, so this repeats until nothing changes.
        loop {
            let dropped = reached
                .fieldwise
                .iter()
                .copied()
                .filter(|name| plan.records.contains(name))
                .filter(|name| {
                    shape(mir, *name).is_none_or(|interface| {
                        !crate::classes::effective_interface_fields(mir, interface)
                            .iter()
                            .all(|field| plan.field_compares(mir, references, field.ty, 0))
                    })
                })
                .collect::<Vec<_>>();
            if dropped.is_empty() {
                break;
            }
            for name in dropped {
                plan.records.remove(&name);
            }
        }
        plan
    }

    /// Whether the record `name` has an `impl SmeltDeepEq`.
    pub(crate) fn has_impl(&self, name: Symbol) -> bool {
        self.records.contains(&name)
    }

    /// Whether any record has an impl: the program then carries the trait.
    pub(crate) fn has_impls(&self) -> bool {
        !self.records.is_empty()
    }

    /// Whether a deep comparison reaches, through a record's field or a type
    /// argument, a shape that compares through its views. Views exist only in
    /// a program that carries the erased runtime, so the runtime demand scan
    /// asks this to carry it, as it does for such a shape compared directly
    /// (`classify::deep_compares_a_reference_shape`).
    pub(crate) fn reaches_view_record(&self) -> bool {
        self.reaches_view_record
    }

    /// Whether two values of type `ty` compare through `SmeltDeepEq` in
    /// `scope`: a record with an impl whose type arguments all have one.
    pub(crate) fn compares_through_trait(&self, mir: &Mir, ty: TypeId, scope: Scope) -> bool {
        matches!(
            mir.types.get(ty),
            Some(Type::Class { name, args })
                if self.has_impl(*name)
                    && args.iter().all(|arg| self.argument_compares(mir, *arg, scope, 0))
        )
    }

    /// Whether `ty` holds, inside the containers the comparison walks, a
    /// record that compares through `SmeltDeepEq` in a function body.
    pub(crate) fn holds_trait_record(&self, mir: &Mir, ty: TypeId, depth: u8) -> bool {
        if depth > 16 {
            return false;
        }
        let deeper = depth.saturating_add(1);
        match mir.types.get(ty) {
            Some(Type::Class { .. }) => self.compares_through_trait(mir, ty, Scope::Body),
            Some(Type::List(item) | Type::Optional(item)) => {
                self.holds_trait_record(mir, *item, deeper)
            }
            Some(Type::Tuple(items)) => items
                .iter()
                .any(|item| self.holds_trait_record(mir, *item, deeper)),
            Some(Type::Dict(key, value)) if walks_dict_keyed_by(mir, *key) => {
                self.holds_trait_record(mir, *value, deeper)
            }
            _ => false,
        }
    }

    /// Whether the runtime implements `SmeltDeepEq` for the rendering of the
    /// type argument `ty`: the leaf impls of `runtime/deep_eq.rs` and the
    /// generated ones. A type parameter has the bound only inside a record's
    /// own impl.
    fn argument_compares(&self, mir: &Mir, ty: TypeId, scope: Scope, depth: u8) -> bool {
        if depth > 16 {
            return false;
        }
        match mir.types.get(ty) {
            Some(Type::Float | Type::Bool | Type::String) => true,
            Some(Type::TypeParam { .. }) => scope == Scope::Impl,
            Some(Type::List(item) | Type::Optional(item)) => {
                self.argument_compares(mir, *item, scope, depth.saturating_add(1))
            }
            Some(Type::Class { .. }) => self.compares_through_trait(mir, ty, scope),
            _ => false,
        }
    }

    /// Whether a record field of type `ty` can be compared at that type by
    /// the expression `binary_ops::structural_leaf_equality_expr` writes for
    /// it inside the record's impl.
    fn field_compares(
        &self,
        mir: &Mir,
        references: &HashSet<Symbol>,
        ty: TypeId,
        depth: u8,
    ) -> bool {
        if depth > 16 {
            return false;
        }
        let deeper = depth.saturating_add(1);
        match mir.types.get(ty) {
            // The record's own parameter, compared through its bound.
            Some(Type::TypeParam { .. }) => true,
            Some(Type::List(item) | Type::Optional(item)) => {
                self.field_compares(mir, references, *item, deeper)
            }
            Some(Type::Tuple(items)) => items
                .iter()
                .all(|item| self.field_compares(mir, references, *item, deeper)),
            Some(Type::Dict(key, value)) if walks_dict_keyed_by(mir, *key) => {
                self.field_compares(mir, references, *value, deeper)
            }
            Some(Type::Class { name, .. })
                if self.compares_through_trait(mir, ty, Scope::Impl)
                    || shape(mir, *name).is_some_and(|interface| {
                        compares_through_views(mir, references, interface)
                    }) =>
            {
                true
            }
            // Everything else is compared with its own `==`, which a type
            // parameter is not bounded for.
            Some(_) => {
                !mentions_type_param(mir, ty, 0)
                    && crate::derive_impls::type_supports_partial_eq_where(
                        mir,
                        &|name| references.contains(&name),
                        ty,
                        &mut Vec::new(),
                    )
            }
            None => false,
        }
    }
}
