//! Generated traits for class- and interface-bounded type parameters.
//!
//! A hand-written port of
//!
//! ```ts
//! function loud<A extends Animal>(a: A): A { console.log(a.speak(), a.legs); return a; }
//! ```
//!
//! keeps `A` generic and names what the body needs from it as a trait:
//!
//! ```rust,ignore
//! trait AnimalLike { fn speak(&self) -> String; fn legs(&self) -> f64; }
//! impl AnimalLike for Animal { .. }
//! impl AnimalLike for Dog { .. }      // every subclass
//! fn loud<A: AnimalLike + Clone + ..>(a: A) -> A { .. a.speak() .. a.legs() .. }
//! ```
//!
//! This module decides, from MIR alone, which such traits exist (the
//! [`BoundTraitPlan`]); `emitter::bound_dispatch` renders them and the
//! `Rvalue::BoundMethod` / `Rvalue::BoundField` reads that dispatch through
//! them. See `blocker-logs/bounded-generics-plan.md`, increments 1-3.
//!
//! # Which members a trait carries
//!
//! Exactly the members the program reads through the bound (every
//! `BoundMethod` / `BoundField` naming it, in any body), methods as methods
//! and fields as getter methods of the same name.
//!
//! # When a bound gets no trait
//!
//! A trait is planned only for a declared, non-generic class or interface
//! bound, and it carries only the members that can be implemented faithfully
//! on the bound itself and on every implementor that is kept:
//!
//! * a method is synchronous, has no rest parameter, takes only plain value
//!   parameters (no callbacks, see [`plain_member_type`]), and every
//!   implementor's own resolution of it (an override, an inherited method, or
//!   a virtual slot) has the bound's exact parameter and return types;
//! * a field is a stored field of identical type on every implementor;
//! * no member name collides with a method of the bounds every generated type
//!   parameter already carries (`clone`, `default`, ...).
//!
//! A member outside these rules (a `this`-typed result, which Smelt erases; a
//! callback parameter) is left out of the trait, and a function reading it
//! through the bound keeps the erased parameter. An implementor that cannot
//! implement a member is left out rather than
//! sinking the trait: a call that instantiates the parameter with it is then
//! rejected by `classes::liftable_type_params`, which erases the parameter at
//! that function instead. Without a trait the parameter erases exactly as it
//! did before bounds were supported.

#![expect(
    clippy::redundant_pub_crate,
    reason = "the plan is shared with the sibling emitter and class modules"
)]

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;

use smelt_hir::{Symbol, Type, TypeId};
use smelt_mir::{BasicBlock, FuncId, Mir, MirClass, MirFunction, MirInterface, Rvalue, Statement};

use crate::classes::{effective_class_fields, effective_interface_fields};
use crate::rust::RustIdent;

/// Method names every generated type parameter already has through its common
/// bounds (`Clone + Default + IntoSmeltUnknown + SmeltFromUnknown`); a trait
/// method of the same name would make `a.clone()` ambiguous (`E0034`).
const RESERVED_MEMBER_NAMES: &[&str] = &[
    "clone",
    "clone_from",
    "default",
    "into_smelt_unknown",
    "smelt_from_unknown",
];

/// The traits generated for bounded type parameters, keyed by bound name.
#[derive(Debug, Default)]
pub(crate) struct BoundTraitPlan {
    /// One planned trait per bound that has one.
    traits: IndexMap<Symbol, BoundTrait>,
}

/// One generated trait: the members read through a bound, and the types that
/// implement them.
#[derive(Debug, Clone)]
pub(crate) struct BoundTrait {
    /// Rust name of the trait (`AnimalLike`).
    pub(crate) trait_name: String,
    /// Members in a stable order (methods then fields, each by first use).
    pub(crate) members: Vec<BoundMember>,
    /// Classes and interfaces that implement every member, the bound first.
    pub(crate) implementors: Vec<Implementor>,
}

/// One trait member.
#[derive(Debug, Clone)]
pub(crate) struct BoundMember {
    /// Source member name.
    pub(crate) name: Symbol,
    /// Rust method name in the trait.
    pub(crate) rust_name: String,
    /// What the member is.
    pub(crate) kind: BoundMemberKind,
}

/// The shape of one trait member.
#[derive(Debug, Clone)]
pub(crate) enum BoundMemberKind {
    /// A method; the trait method takes `&self` and these parameters.
    Method {
        /// Parameter types, receiver excluded.
        params: Vec<TypeId>,
        /// Declared return type.
        ret: TypeId,
        /// Whether any implementation can throw, so the trait method returns
        /// a `Result` and a call through it propagates with `?`.
        throws: bool,
    },
    /// A stored field, read through a getter of the same name.
    Field {
        /// Field type.
        ty: TypeId,
    },
}

/// A type implementing a bound trait, with how it implements each member
/// (parallel to [`BoundTrait::members`]).
#[derive(Debug, Clone)]
pub(crate) struct Implementor {
    /// Class or interface name.
    pub(crate) name: Symbol,
    /// How each member is implemented, in member order.
    pub(crate) members: Vec<MemberImpl>,
}

/// How one implementor provides one member.
#[derive(Debug, Clone)]
#[expect(
    variant_size_differences,
    reason = "a handful per planned trait; boxing the function id would only add indirection"
)]
pub(crate) enum MemberImpl {
    /// Call the class's own (possibly inherited) method.
    Method {
        /// The resolved method.
        function: FuncId,
        /// Whether that method returns a `Result`.
        throws: bool,
    },
    /// Call the function stored in a field: a class's virtual slot, or an
    /// interface's method property.
    FunctionField {
        /// Whether the stored function returns a `Result`.
        throws: bool,
    },
    /// Read a stored field.
    Field,
}

impl BoundTraitPlan {
    /// Plan the trait of every bound the program reads members through.
    pub(crate) fn compute(mir: &Mir) -> Self {
        let mut uses: IndexMap<Symbol, BoundUses> = IndexMap::new();
        let bodies = mir
            .functions
            .iter()
            .map(|function| function.blocks.as_slice())
            .chain(mir.closures.iter().map(|closure| closure.blocks.as_slice()));
        for blocks in bodies {
            collect_uses(blocks, &mut uses);
        }
        let taken: HashSet<String> = mir
            .classes
            .iter()
            .map(|class| class.name)
            .chain(mir.interfaces.iter().map(|interface| interface.name))
            .filter_map(|name| mir.symbols.get(name))
            .map(|name| RustIdent::new(name).into_string())
            .collect();
        let mut traits = IndexMap::new();
        for (bound, bound_uses) in uses {
            if let Some(planned) = plan_trait(mir, bound, &bound_uses, &taken) {
                traits.insert(bound, planned);
            }
        }
        Self { traits }
    }

    /// The trait planned for `bound`, if any.
    pub(crate) fn trait_for(&self, bound: Symbol) -> Option<&BoundTrait> {
        self.traits.get(&bound)
    }

    /// Every planned trait, by bound.
    pub(crate) fn traits(&self) -> impl Iterator<Item = (Symbol, &BoundTrait)> {
        self.traits.iter().map(|(bound, planned)| (*bound, planned))
    }
}

impl BoundTrait {
    /// The member named `name`, if the trait carries it.
    pub(crate) fn member(&self, name: Symbol) -> Option<&BoundMember> {
        self.members.iter().find(|member| member.name == name)
    }

    /// Whether values of type `ty` implement the trait.
    pub(crate) fn implemented_by(&self, mir: &Mir, ty: TypeId) -> bool {
        let Some(Type::Class { name, args }) = mir.types.get(ty) else {
            return false;
        };
        args.is_empty() && self.implementors.iter().any(|implementor| implementor.name == *name)
    }
}

/// The members read through one bound, in first-use order.
#[derive(Debug, Default)]
struct BoundUses {
    /// Methods called through the bound.
    methods: Vec<Symbol>,
    /// Fields read through the bound.
    fields: Vec<Symbol>,
}

/// Record every bound member read in one body.
fn collect_uses(blocks: &[BasicBlock], uses: &mut IndexMap<Symbol, BoundUses>) {
    for block in blocks {
        for statement in &block.statements {
            let (Statement::Assign { value, .. } | Statement::AssignPlace { value, .. }) = statement
            else {
                continue;
            };
            match value {
                Rvalue::BoundMethod { bound, method, .. } => {
                    let entry = uses.entry(*bound).or_default();
                    if !entry.methods.contains(method) {
                        entry.methods.push(*method);
                    }
                }
                Rvalue::BoundField { bound, field, .. } => {
                    let entry = uses.entry(*bound).or_default();
                    if !entry.fields.contains(field) {
                        entry.fields.push(*field);
                    }
                }
                _ => {}
            }
        }
    }
}

/// The bound's own declaration: a non-generic class or interface.
enum BoundDecl<'mir> {
    /// A class bound.
    Class(&'mir MirClass),
    /// An interface bound.
    Interface(&'mir MirInterface),
}

/// Plan one bound's trait, or `None` when it cannot have one (module docs).
fn plan_trait(
    mir: &Mir,
    bound: Symbol,
    uses: &BoundUses,
    taken: &HashSet<String>,
) -> Option<BoundTrait> {
    let decl = if let Some(class) = crate::classes::find_class(mir, bound) {
        class.type_params.is_empty().then_some(BoundDecl::Class(class))?
    } else {
        let interface = mir.interface_named(bound)?;
        interface.type_params.is_empty().then_some(BoundDecl::Interface(interface))?
    };
    // A member the trait cannot carry is left out, not the whole trait: a
    // function reading it through the bound then keeps the erased parameter
    // (`classes::liftable_type_params` checks every member it reads), while
    // functions reading only representable members still lift.
    let mut members = Vec::new();
    let mut seen_names = HashSet::new();
    let mut keep = |candidate: Option<BoundMember>, kept: &mut Vec<BoundMember>| {
        if let Some(planned) = candidate
            && !RESERVED_MEMBER_NAMES.contains(&planned.rust_name.as_str())
            && seen_names.insert(planned.rust_name.clone())
        {
            kept.push(planned);
        }
    };
    for &method in &uses.methods {
        let planned = bound_method_signature(mir, &decl, method).and_then(|(params, ret)| {
            member(mir, method, BoundMemberKind::Method { params, ret, throws: false })
        });
        keep(planned, &mut members);
    }
    for &field in &uses.fields {
        let planned = bound_field_type(mir, &decl, field)
            .and_then(|ty| member(mir, field, BoundMemberKind::Field { ty }));
        keep(planned, &mut members);
    }
    if members.is_empty() {
        return None;
    }

    let candidates = match decl {
        BoundDecl::Class(_) => class_implementors(mir, bound),
        BoundDecl::Interface(_) => interface_implementors(mir, bound),
    };
    let mut implementors = Vec::new();
    for (name, is_class) in candidates {
        let resolved = members
            .iter()
            .map(|planned| implement_member(mir, name, is_class, planned))
            .collect::<Option<Vec<_>>>();
        match resolved {
            Some(implemented) => implementors.push(Implementor {
                name,
                members: implemented,
            }),
            // The bound itself must implement its own trait.
            None if name == bound => return None,
            None => {}
        }
    }
    // A method throws through the trait when any implementation does.
    for (index, planned) in members.iter_mut().enumerate() {
        if let BoundMemberKind::Method { throws, .. } = &mut planned.kind {
            *throws = implementors.iter().any(|implementor| {
                matches!(
                    implementor.members.get(index),
                    Some(MemberImpl::Method { throws: true, .. } | MemberImpl::FunctionField { throws: true })
                )
            });
        }
    }

    let base_name = format!("{}Like", RustIdent::new(mir.symbols.get(bound)?).into_string());
    let mut trait_name = base_name.clone();
    let mut suffix = 1usize;
    while taken.contains(&trait_name) {
        trait_name = format!("{base_name}{suffix}");
        suffix = suffix.checked_add(1)?;
    }
    Some(BoundTrait {
        trait_name,
        members,
        implementors,
    })
}

/// Build a member record with its sanitized Rust name.
fn member(mir: &Mir, name: Symbol, kind: BoundMemberKind) -> Option<BoundMember> {
    Some(BoundMember {
        name,
        rust_name: crate::sanitize_ident(mir.symbols.get(name)?),
        kind,
    })
}

/// Whether a type can cross a trait method boundary unchanged: a primitive, a
/// non-generic class or interface value, or a list/optional of one.
///
/// Callbacks are excluded on purpose: the emitter renders a callback parameter
/// as a borrowed `&dyn Fn` or a generated `F: Fn` bound depending on the
/// method's ownership analysis, which a shared trait signature cannot follow.
fn plain_member_type(mir: &Mir, ty: TypeId) -> bool {
    match mir.types.get(ty) {
        Some(Type::Float | Type::Int | Type::Bool | Type::String | Type::PyStr | Type::BigInt | Type::None) => true,
        Some(Type::Class { args, .. }) => args.is_empty(),
        Some(Type::List(item) | Type::Optional(item)) => plain_member_type(mir, *item),
        _ => false,
    }
}

/// The bound's own signature for `method`: parameter types (receiver
/// excluded) and return type.
fn bound_method_signature(
    mir: &Mir,
    decl: &BoundDecl<'_>,
    method: Symbol,
) -> Option<(Vec<TypeId>, TypeId)> {
    let (params, ret) = match decl {
        BoundDecl::Class(class) => if let Some(function) = resolve_class_method(mir, class, method) {
            method_function_signature(function)?
        } else {
            // An abstract method is declared by signature only.
            let signature = class_chain(mir, class)
                .into_iter()
                .flat_map(|level| level.abstract_methods.iter())
                .find(|signature| signature.name == method)?;
            if signature.is_async || signature.rest.is_some() {
                return None;
            }
            (
                signature.params.iter().map(|param| param.ty).collect(),
                signature.return_ty,
            )
        },
        BoundDecl::Interface(interface) => {
            if let Some(signature) = interface.methods.iter().find(|signature| signature.name == method) {
                if signature.is_async || signature.rest.is_some() {
                    return None;
                }
                (
                    signature.params.iter().map(|param| param.ty).collect(),
                    signature.return_ty,
                )
            } else {
                let field = effective_interface_fields(mir, interface)
                    .into_iter()
                    .find(|field| field.name == method)?;
                let Some(Type::Function(function)) = mir.types.get(field.ty) else {
                    return None;
                };
                if function.rest.is_some() {
                    return None;
                }
                (function.params.clone(), function.return_ty)
            }
        }
    };
    (params.iter().all(|param| plain_member_type(mir, *param)) && plain_member_type(mir, ret))
        .then_some((params, ret))
}

/// A synchronous method's signature with its receiver dropped, or `None` for
/// a shape the trait cannot carry (async, generator, rest parameter).
fn method_function_signature(function: &MirFunction) -> Option<(Vec<TypeId>, TypeId)> {
    if function.is_async || function.is_generator || function.rest.is_some() {
        return None;
    }
    let params = function
        .params
        .iter()
        .skip(1)
        .map(|param| {
            function
                .locals
                .get(usize::try_from(param.0).ok()?)
                .map(|local| local.ty)
        })
        .collect::<Option<Vec<_>>>()?;
    Some((params, function.return_ty))
}

/// The bound's own declared type for field `field`.
fn bound_field_type(mir: &Mir, decl: &BoundDecl<'_>, field: Symbol) -> Option<TypeId> {
    let fields = match decl {
        BoundDecl::Class(class) => effective_class_fields(mir, class),
        BoundDecl::Interface(interface) => effective_interface_fields(mir, interface),
    };
    let declared = fields.into_iter().find(|candidate| candidate.name == field)?;
    plain_member_type(mir, declared.ty).then_some(declared.ty)
}

/// `class` and its bases, nearest first.
fn class_chain<'mir>(mir: &'mir Mir, class: &'mir MirClass) -> Vec<&'mir MirClass> {
    let mut chain = vec![class];
    let mut seen = HashSet::from([class.name]);
    while let Some(base) = chain.last().and_then(|last| last.base).and_then(|base| crate::classes::find_class(mir, base)) {
        if !seen.insert(base.name) {
            break;
        }
        chain.push(base);
    }
    chain
}

/// The method `method` as `class` resolves it: its own, else the nearest
/// base's (which the emitter copies into the subclass's `impl`).
fn resolve_class_method<'mir>(
    mir: &'mir Mir,
    class: &'mir MirClass,
    method: Symbol,
) -> Option<&'mir MirFunction> {
    class_chain(mir, class).into_iter().find_map(|level| {
        level
            .methods
            .iter()
            .filter_map(|id| mir.functions.get(usize::try_from(id.0).ok()?))
            .find(|function| function.name == method)
    })
}

/// Non-generic classes that are `bound` or extend it, the bound first.
fn class_implementors(mir: &Mir, bound: Symbol) -> Vec<(Symbol, bool)> {
    let mut found = vec![(bound, true)];
    for class in &mir.classes {
        if class.name != bound
            && class.type_params.is_empty()
            && class_chain(mir, class).iter().any(|level| level.name == bound)
        {
            found.push((class.name, true));
        }
    }
    found
}

/// The interface `bound` itself, non-generic interfaces extending it, and
/// non-generic classes that (directly, through a base, or through an extending
/// interface) declare they implement it; the bound first.
fn interface_implementors(mir: &Mir, bound: Symbol) -> Vec<(Symbol, bool)> {
    // Interfaces that are `bound` or extend it, to a fixed point.
    let mut family: Vec<Symbol> = vec![bound];
    let mut changed = true;
    while changed {
        changed = false;
        for interface in &mir.interfaces {
            if !family.contains(&interface.name)
                && interface.extends.iter().any(|heritage| family.contains(&heritage.parent))
            {
                family.push(interface.name);
                changed = true;
            }
        }
    }
    let mut found: Vec<(Symbol, bool)> = family
        .iter()
        .filter(|name| {
            mir.interfaces
                .iter()
                .any(|interface| interface.name == **name && interface.type_params.is_empty())
        })
        .map(|name| (*name, false))
        .collect();
    for class in &mir.classes {
        if class.type_params.is_empty()
            && class_chain(mir, class)
                .iter()
                .any(|level| level.implements.iter().any(|name| family.contains(name)))
        {
            found.push((class.name, true));
        }
    }
    found
}

/// How implementor `name` provides `member`, or `None` when it cannot with the
/// bound's exact types.
fn implement_member(mir: &Mir, name: Symbol, is_class: bool, member: &BoundMember) -> Option<MemberImpl> {
    let fields = if is_class {
        effective_class_fields(mir, crate::classes::find_class(mir, name)?)
    } else {
        effective_interface_fields(mir, mir.interface_named(name)?)
    };
    match &member.kind {
        BoundMemberKind::Field { ty } => fields
            .iter()
            .any(|field| field.name == member.name && field.ty == *ty)
            .then_some(MemberImpl::Field),
        BoundMemberKind::Method { params, ret, .. } => {
            // A function-valued field of the member's name is a virtual slot
            // (class) or a method property (interface struct); it is what a
            // call on a value of this static type dispatches through.
            if let Some(field) = fields.iter().find(|field| field.name == member.name) {
                let Some(Type::Function(function)) = mir.types.get(field.ty) else {
                    return None;
                };
                return (function.rest.is_none()
                    && function.params == *params
                    && function.return_ty == *ret)
                    .then_some(MemberImpl::FunctionField {
                        throws: function.may_throw,
                    });
            }
            if !is_class {
                return None;
            }
            let function = resolve_class_method(mir, crate::classes::find_class(mir, name)?, member.name)?;
            let (own_params, own_ret) = method_function_signature(function)?;
            (own_params == *params && own_ret == *ret).then_some(MemberImpl::Method {
                function: function.id,
                throws: function.can_throw,
            })
        }
    }
}

/// The bound recorded on a type parameter declaration, for
/// `classes::liftable_type_params`: the symbol of a non-generic `Type::Class`
/// constraint that names a declared class or interface, or the builtin `Date`
/// (see [`constraint_is_date`]). Any other builtin bound answers `None`.
pub(crate) fn constraint_bound(mir: &Mir, constraint: Option<TypeId>) -> Option<Symbol> {
    match mir.types.get(constraint?) {
        Some(Type::Class { name, args })
            if args.is_empty()
                && (crate::classes::find_class(mir, *name).is_some()
                    || mir.has_interface(*name)) =>
        {
            Some(*name)
        }
        _ if constraint_is_date(mir, constraint) => match mir.types.get(constraint?) {
            Some(Type::Class { name, .. }) => Some(*name),
            _ => None,
        },
        _ => None,
    }
}

/// Whether `constraint` is the builtin `Date` class, the bound a `D extends
/// Date` parameter lifts through the prelude's `DateLike` trait with
/// (`date_prelude`) rather than through a planned trait.
pub(crate) fn constraint_is_date(mir: &Mir, constraint: Option<TypeId>) -> bool {
    matches!(constraint.and_then(|ty| mir.types.get(ty)), Some(Type::Class { name, args })
        if args.is_empty()
            && crate::classes::find_class(mir, *name).is_none()
            && mir.symbols.get(*name).and_then(smelt_stdlib::typescript_stdlib_class)
                == Some(smelt_stdlib::StdlibClass::Date))
}

/// Whether type parameter `name`, as seen from `function`, is bounded by
/// `Date`: declared so by `function`, or — for a closure, which declares no
/// parameters of its own — by the free function that does declare it.
pub(crate) fn type_param_is_date_bounded(mir: &Mir, function: &MirFunction, name: Symbol) -> bool {
    let declared = |params: &[smelt_hir::TypeParamDef]| {
        params.iter().find(|param| param.name == name).map(|param| constraint_is_date(mir, param.constraint))
    };
    declared(&function.type_params).unwrap_or_else(|| {
        mir.functions
            .iter()
            .filter_map(|candidate| declared(&candidate.type_params))
            .any(|is_date| is_date)
    })
}

/// Map from type-parameter name to bound for a declaration list.
pub(crate) fn declared_bounds(
    mir: &Mir,
    params: &[smelt_hir::TypeParamDef],
) -> HashMap<Symbol, Symbol> {
    params
        .iter()
        .filter_map(|param| Some((param.name, constraint_bound(mir, param.constraint)?)))
        .collect()
}
