//! Polymorphic class handles: one shared object behind every static type of a
//! class hierarchy.
//!
//! # The defect this closes
//!
//! A reference class (`classify::reference_classes`) is a handle
//! `Name(Rc<RefCell<NameInner>>)`, and Smelt used to flatten inheritance: a
//! subclass was its own handle over its own cell carrying a copy of every
//! inherited field. Passing a `Sub` where a `Base` is expected
//! (`const b: Base = sub`, an element of a `Base[]`, a `Base` return) then
//! built a NEW `Base` cell from the subclass's fields
//! (`emitter::coercion::record_adapter`). JavaScript objects have reference
//! identity, so `sub === b` answered `false`, a write through one handle was
//! invisible through the other, and an overridden method slot bound to the
//! original read stale state.
//!
//! # The representation
//!
//! What a hand-porting team would write for a mutable class hierarchy is a
//! trait object over shared storage, so that is what a *polymorphic* class
//! becomes (see [`polymorphic_classes`]):
//!
//! ```text
//! struct Base(Rc<dyn BaseSmeltCell>);            // any instance of Base or below
//! struct BaseInner { count: f64 }
//! struct Sub(Rc<RefCell<SubInner>>);             // a leaf stays concrete
//! struct SubInner { __smelt_base: BaseInner, extra: f64 }   // + Deref to BaseInner
//! trait BaseSmeltCell {
//!     fn borrow(&self) -> Ref<'_, BaseInner>;
//!     fn borrow_mut(&self) -> RefMut<'_, BaseInner>;
//!     fn smelt_any(self: Rc<Self>) -> Rc<dyn Any>;   // checked downcasts
//!     fn smelt_class_name(&self) -> &'static str;     // `instanceof`
//!     fn into_smelt_unknown_as_concrete(self: Rc<Self>) -> SmeltUnknown; // erasure (when the crate erases)
//! }
//! impl BaseSmeltCell for RefCell<BaseInner> { .. }
//! impl BaseSmeltCell for RefCell<SubInner> { .. }  // borrows `&inner.__smelt_base`
//! ```
//!
//! - **Storage nests.** A subclass's inner record holds its base's inner
//!   record as [`BASE_FIELD`] and dereferences to it, so every existing field
//!   access spelling (`self.0.borrow().count`, `.borrow_mut().count = v`) keeps
//!   compiling unchanged on a subclass handle, and the base part of one object
//!   is one place in memory.
//! - **The trait methods are named `borrow`/`borrow_mut`**, so the same
//!   spelling on a polymorphic handle (`b.0.borrow().count`) resolves to the
//!   trait object's method and yields the base view of whatever cell it holds.
//! - **An upcast is an unsizing coercion** of the very same `Rc`
//!   (`Base(sub.0.clone())`): one allocation, so `===` (the cell address),
//!   mutation and the virtual slots the subclass bound at construction all
//!   hold through either handle. An upcast from a polymorphic intermediate
//!   class goes through its trait's `smelt_as_<ancestor>` method.
//! - **A downcast is checked** through `smelt_any` (`Rc<dyn Any>::downcast`)
//!   over the target's concrete classes, falling back to the previous
//!   field-wise conversion when the instance is not one of them (TypeScript's
//!   structural assignability lets a `Base` instance flow to a member-less
//!   `Sub`).
//! - **A leaf class stays concrete**: `Sub(Rc<RefCell<SubInner>>)`. Only a
//!   class that has subclasses pays for dynamic dispatch on field access.
//!
//! The hidden `_smelt_class` tag (`crate::class_identity`) is not needed on a
//! polymorphic hierarchy: the cell's concrete type IS the constructed class,
//! answered by `smelt_class_name`.
#![expect(
    clippy::redundant_pub_crate,
    reason = "polymorphism helpers are shared by the storage emitters and the function emitter"
)]

use std::collections::{HashMap, HashSet};

use smelt_hir::Symbol;
use smelt_mir::{Mir, MirClass, MirField};

use crate::classes::{effective_class_fields, find_class};
use crate::generic_bindings;

/// The Rust field of a nested inner record that holds its base's inner record.
pub(crate) const BASE_FIELD: &str = "__smelt_base";

/// The Rust name of the cell trait of a polymorphic class whose handle type is
/// named `type_name`; claimed with the class (`names::plan`).
pub(crate) fn cell_trait_name(type_name: &str) -> String {
    format!("{type_name}SmeltCell")
}

/// The companion name suffixes a reference class claims beside its own name.
pub(crate) const REFERENCE_COMPANIONS: &[&str] = &[
    "Inner",
    "SmeltCell",
    "SmeltStatics",
    "SmeltView",
    "SmeltViewOf",
];

/// Whether `class` is an instance class (a statics frame is the class VALUE of
/// a per-evaluation class, never an instance a hierarchy holds).
const fn is_instance_class(class: &MirClass) -> bool {
    class.statics_of.is_none()
}

/// The program base class of `class`, if it is a generated class.
fn program_base<'mir>(mir: &'mir Mir, class: &MirClass) -> Option<&'mir MirClass> {
    find_class(mir, class.base?)
}

/// The classes that become polymorphic handles.
///
/// A class is polymorphic when it is an instance reference class with at
/// least one reference subclass: only then can a value of its static type
/// hold an instance of another class. (The heritage rule of
/// `classify::reference_classes` makes a reference class's whole chain
/// reference classes, so every subclass of one is a reference class too.)
///
/// Every such hierarchy nests, whatever its subclasses do with the base:
/// - a subclass fixing a base type parameter a callable slot takes bare
///   (`class Year extends Parser<number>` over `validate: (value: T) =>
///   boolean`) stores the slot at the declaring level's ABI (`Fn(&f64)`), and
///   every value and call it builds for the slot is bridged to that ABI
///   (`emitter::record_slot_abi`);
/// - a subclass redeclaring an inherited field narrower (`declare food:
///   string` over `food: string | number`) reads the one stored property
///   through a checked downcast and writes it through an upcast
///   ([`redeclared_field_storage`], `emitter::redeclared_field`).
pub(crate) fn polymorphic_classes(mir: &Mir, references: &HashSet<Symbol>) -> HashSet<Symbol> {
    let eligible = |class: &MirClass| {
        is_instance_class(class)
            && references.contains(&class.name)
            && !crate::classify::class_has_index_store(mir, class.name)
    };
    let mut polymorphic = HashSet::new();
    for class in &mir.classes {
        if !eligible(class) {
            continue;
        }
        let mut current = class;
        let mut seen = HashSet::from([class.name]);
        while let Some(base) = program_base(mir, current) {
            if !seen.insert(base.name) || !eligible(base) {
                break;
            }
            polymorphic.insert(base.name);
            current = base;
        }
    }
    polymorphic
}

/// The base whose inner record `class`'s inner record nests, when its base is
/// polymorphic.
pub(crate) fn nested_base(
    mir: &Mir,
    polymorphic: &HashSet<Symbol>,
    class: Symbol,
) -> Option<Symbol> {
    let found = find_class(mir, class)?;
    if !is_instance_class(found) {
        return None;
    }
    let base = found.base?;
    polymorphic.contains(&base).then_some(base)
}

/// The fields `class`'s OWN inner record stores: every effective field when
/// it does not nest, else only the ones its nested base does not already
/// store (its own declarations, and the members a merged interface adds).
pub(crate) fn level_fields(
    mir: &Mir,
    polymorphic: &HashSet<Symbol>,
    class: &MirClass,
) -> Vec<MirField> {
    let effective = effective_class_fields(mir, class);
    let Some(base) =
        nested_base(mir, polymorphic, class.name).and_then(|base| find_class(mir, base))
    else {
        return effective;
    };
    let inherited: HashSet<Symbol> = effective_class_fields(mir, base)
        .into_iter()
        .map(|field| field.name)
        .collect();
    effective
        .into_iter()
        .filter(|field| !inherited.contains(&field.name))
        .collect()
}

/// The polymorphic strict ancestors of `class`, nearest first.
pub(crate) fn polymorphic_ancestors(
    mir: &Mir,
    polymorphic: &HashSet<Symbol>,
    class: Symbol,
) -> Vec<Symbol> {
    let mut ancestors = Vec::new();
    let mut current = class;
    let mut seen = HashSet::from([class]);
    while let Some(base) = nested_base(mir, polymorphic, current) {
        if !seen.insert(base) {
            break;
        }
        ancestors.push(base);
        current = base;
    }
    ancestors
}

/// `class` and every instance class below it, in declaration order: the
/// concrete cell types a handle of `class` can hold.
pub(crate) fn concrete_classes(
    mir: &Mir,
    polymorphic: &HashSet<Symbol>,
    class: Symbol,
) -> Vec<Symbol> {
    mir.classes
        .iter()
        .filter(|candidate| is_instance_class(candidate))
        .filter(|candidate| {
            candidate.name == class
                || polymorphic_ancestors(mir, polymorphic, candidate.name).contains(&class)
        })
        .map(|candidate| candidate.name)
        .collect()
}

/// How one class's reference storage is laid out under polymorphism.
pub(crate) struct ClassLayout {
    /// Whether the handle holds `Rc<dyn <Name>SmeltCell>` (the class has
    /// subclasses) rather than its own concrete cell.
    pub(crate) polymorphic: bool,
    /// The nested base inner record's type (`BaseInner<..>`), when the class
    /// nests its base.
    pub(crate) nested_base_type: Option<String>,
    /// The fields this level's inner record declares itself.
    pub(crate) level_fields: Vec<MirField>,
    /// The cell traits this class's cell implements: its own (first, when
    /// polymorphic) and each polymorphic ancestor's.
    pub(crate) cell_impls: Vec<CellImpl>,
    /// The source name `instanceof` and erasure report for this class.
    pub(crate) class_name_literal: String,
}

/// One `impl <A>SmeltCell<..> for RefCell<<C>Inner<..>>`.
pub(crate) struct CellImpl {
    /// The trait as referenced from the implementing class's scope
    /// (`BaseSmeltCell<JsString>`).
    pub(crate) trait_ref: String,
    /// The ancestor's inner record at those arguments (`BaseInner<JsString>`).
    pub(crate) inner_view: String,
    /// For each polymorphic strict ancestor `P` of the trait's class: the
    /// upcast method name and the trait object it answers, both in the
    /// implementing class's scope.
    pub(crate) upcasts: Vec<(String, String)>,
}

/// The upcast method a cell trait carries toward its polymorphic ancestor
/// whose handle type is named `ancestor_type_name`.
pub(crate) fn upcast_method_name(ancestor_type_name: &str) -> String {
    format!("smelt_as_{ancestor_type_name}")
}

/// Render `<args>` of `ancestor` (its carried positions) read in `class`'s
/// scope, with `class`'s parameters `scoped` in scope.
pub(crate) fn ancestor_args_text(
    mir: &Mir,
    context: &crate::emitter::EmitContext,
    class: Symbol,
    ancestor: &MirClass,
    scoped: &HashSet<Symbol>,
) -> Result<String, crate::EmitError> {
    let substitution = crate::classes::inherited_type_substitution(mir, class, ancestor.name);
    let mut rendered = Vec::new();
    for param in &ancestor.type_params {
        let Some(ty) = substitution.get(&param.name) else {
            return Err(crate::EmitError::new(
                "a polymorphic ancestor's type parameter has no argument in a subclass",
            ));
        };
        rendered.push(
            crate::emitter::FunctionEmitter::type_text_for_with_scoped_type_params(
                mir,
                context,
                *ty,
                &crate::type_substitution::TypeSubstitution::lexical(scoped),
            )?,
        );
    }
    let args = context
        .type_param_elision()
        .retain_carried(ancestor.name, rendered);
    Ok(if args.is_empty() {
        String::new()
    } else {
        format!("<{}>", args.join(", "))
    })
}

/// Compute `class`'s layout, or `None` when it neither is polymorphic nor
/// nests a base (its storage is the plain reference record).
pub(crate) fn class_layout(
    mir: &Mir,
    context: &crate::emitter::EmitContext,
    class: &MirClass,
) -> Result<Option<ClassLayout>, crate::EmitError> {
    let polymorphic_set = context.polymorphic_classes();
    let polymorphic = polymorphic_set.contains(&class.name);
    let nested = nested_base(mir, polymorphic_set, class.name);
    if !polymorphic && nested.is_none() {
        return Ok(None);
    }
    let scoped: HashSet<Symbol> = class.type_params.iter().map(|param| param.name).collect();
    let type_name = |symbol: Symbol| -> Result<String, crate::EmitError> {
        Ok(context.names().type_name(symbol)?.as_str().to_owned())
    };
    let nested_base_type = match nested.and_then(|base| find_class(mir, base)) {
        Some(base) => Some(format!(
            "{}Inner{}",
            type_name(base.name)?,
            ancestor_args_text(mir, context, class.name, base, &scoped)?
        )),
        None => None,
    };
    let own_args = crate::classes::class_type_args_text(mir, context, class)?;
    let mut traits: Vec<(Symbol, bool)> = Vec::new();
    if polymorphic {
        traits.push((class.name, true));
    }
    for ancestor in polymorphic_ancestors(mir, polymorphic_set, class.name) {
        traits.push((ancestor, false));
    }
    let mut cell_impls = Vec::new();
    for (trait_class, own) in traits {
        let Some(trait_mir) = find_class(mir, trait_class) else {
            continue;
        };
        let trait_args = if own {
            own_args.clone()
        } else {
            ancestor_args_text(mir, context, class.name, trait_mir, &scoped)?
        };
        let trait_type = type_name(trait_class)?;
        let mut upcasts = Vec::new();
        for upper in polymorphic_ancestors(mir, polymorphic_set, trait_class) {
            let Some(upper_mir) = find_class(mir, upper) else {
                continue;
            };
            let upper_type = type_name(upper)?;
            upcasts.push((
                upcast_method_name(&upper_type),
                format!(
                    "dyn {}{}",
                    cell_trait_name(&upper_type),
                    ancestor_args_text(mir, context, class.name, upper_mir, &scoped)?
                ),
            ));
        }
        cell_impls.push(CellImpl {
            trait_ref: format!("{}{trait_args}", cell_trait_name(&trait_type)),
            inner_view: format!("{trait_type}Inner{trait_args}"),
            upcasts,
        });
    }
    Ok(Some(ClassLayout {
        polymorphic,
        nested_base_type,
        level_fields: level_fields(mir, polymorphic_set, class),
        cell_impls,
        class_name_literal: crate::class_identity::identity_literal(mir, class.name),
    }))
}

/// The trait declaration of polymorphic `class`: the base view of any cell in
/// its hierarchy, the checked-downcast and class-name probes, the erasure
/// (when the crate erases), and an upcast toward each polymorphic ancestor.
pub(crate) fn cell_trait_declaration(
    mir: &Mir,
    context: &crate::emitter::EmitContext,
    class: &MirClass,
    type_params: &str,
    type_args: &str,
    needs_unknown: bool,
) -> Result<String, crate::EmitError> {
    let type_name = context.names().type_name(class.name)?.as_str().to_owned();
    let inner = format!("{type_name}Inner{type_args}");
    let mut methods = vec![
        format!("fn borrow(&self) -> ::std::cell::Ref<'_, {inner}>;"),
        format!("fn borrow_mut(&self) -> ::std::cell::RefMut<'_, {inner}>;"),
        format!(
            "fn try_borrow(&self) -> Result<::std::cell::Ref<'_, {inner}>, ::std::cell::BorrowError>;"
        ),
        "fn smelt_any(self: ::std::rc::Rc<Self>) -> ::std::rc::Rc<dyn ::std::any::Any>;".to_owned(),
        "fn smelt_class_name(&self) -> &'static str;".to_owned(),
    ];
    if needs_unknown {
        methods.push(
            "fn into_smelt_unknown_as_concrete(self: ::std::rc::Rc<Self>) -> SmeltUnknown;"
                .to_owned(),
        );
    }
    let scoped: HashSet<Symbol> = class.type_params.iter().map(|param| param.name).collect();
    for upper in polymorphic_ancestors(mir, context.polymorphic_classes(), class.name) {
        let Some(upper_mir) = find_class(mir, upper) else {
            continue;
        };
        let upper_type = context.names().type_name(upper)?.as_str().to_owned();
        methods.push(format!(
            "fn {}(self: ::std::rc::Rc<Self>) -> ::std::rc::Rc<dyn {}{}>;",
            upcast_method_name(&upper_type),
            cell_trait_name(&upper_type),
            ancestor_args_text(mir, context, class.name, upper_mir, &scoped)?
        ));
    }
    // A widened class's handle may hold a converting view
    // (`crate::class_widening`): its fields are reached through accessors.
    Ok(format!(
        "/// Any instance of `{type_name}` or of a class extending it, viewed as a `{type_name}`.\n#[allow(dead_code, non_snake_case)]\ntrait {}{type_params} {{ {} }}",
        cell_trait_name(&type_name),
        methods.join(" ")
    ))
}

/// One `impl` of a cell trait for `class`'s own concrete cell.
///
/// `handle` is the expression building `class`'s handle from the `Rc` named
/// `self` (`Sub(self)`), and `erase_method` the inherent method that erases
/// that handle as itself (`into_smelt_unknown`, or the own-erasure method of a
/// polymorphic class whose `IntoSmeltUnknown` dispatches back here).
///
/// With `forwards_view` (the own cell of a widened class,
/// `crate::class_widening`) the probes a placeholder cannot answer from its
/// record (the concrete cell, the class name, the erasure, the upcasts)
/// answer through its view slot.
pub(crate) fn cell_impl_text(
    cell_impl: &CellImpl,
    impl_generics: &str,
    inner: &str,
    class_name_literal: &str,
    erase: Option<&str>,
    forwards_view: bool,
) -> String {
    let forward = |call: &str| {
        if forwards_view {
            crate::class_widening::cell_forward(call)
        } else {
            String::new()
        }
    };
    let CellImpl {
        trait_ref,
        inner_view,
        upcasts,
    } = cell_impl;
    let mut methods = vec![
        format!(
            "fn borrow(&self) -> ::std::cell::Ref<'_, {inner_view}> {{ ::std::cell::Ref::map(::std::cell::RefCell::borrow(self), |smelt_inner| {{ let smelt_view: &{inner_view} = smelt_inner; smelt_view }}) }}"
        ),
        format!(
            "fn borrow_mut(&self) -> ::std::cell::RefMut<'_, {inner_view}> {{ ::std::cell::RefMut::map(::std::cell::RefCell::borrow_mut(self), |smelt_inner| {{ let smelt_view: &mut {inner_view} = smelt_inner; smelt_view }}) }}"
        ),
        format!(
            "fn try_borrow(&self) -> Result<::std::cell::Ref<'_, {inner_view}>, ::std::cell::BorrowError> {{ ::std::cell::RefCell::try_borrow(self).map(|smelt_guard| ::std::cell::Ref::map(smelt_guard, |smelt_inner| {{ let smelt_view: &{inner_view} = smelt_inner; smelt_view }})) }}"
        ),
        format!(
            "fn smelt_any(self: ::std::rc::Rc<Self>) -> ::std::rc::Rc<dyn ::std::any::Any> {{ {}self }}",
            forward("smelt_any()")
        ),
        format!(
            "fn smelt_class_name(&self) -> &'static str {{ {}{class_name_literal} }}",
            forward("smelt_class_name()")
                .replace("RefCell::borrow(&self)", "RefCell::borrow(self)")
        ),
    ];
    if let Some(erase_call) = erase {
        methods.push(format!(
            "fn into_smelt_unknown_as_concrete(self: ::std::rc::Rc<Self>) -> SmeltUnknown {{ {}{erase_call} }}",
            forward("smelt_erase()")
        ));
    }
    for (method, object) in upcasts {
        methods.push(format!(
            "fn {method}(self: ::std::rc::Rc<Self>) -> ::std::rc::Rc<{object}> {{ {}self }}",
            forward(&format!("{method}()"))
        ));
    }
    format!(
        "impl{impl_generics} {trait_ref} for ::std::cell::RefCell<{inner}> {{ {} }}",
        methods.join(" ")
    )
}

/// The `Deref`/`DerefMut` impls that let a nested inner record answer its
/// base's fields as its own.
pub(crate) fn deref_impls_text(type_params: &str, inner: &str, base_type: &str) -> String {
    format!(
        "impl{type_params} ::std::ops::Deref for {inner} {{ type Target = {base_type}; fn deref(&self) -> &Self::Target {{ &self.{BASE_FIELD} }} }}\nimpl{type_params} ::std::ops::DerefMut for {inner} {{ fn deref_mut(&mut self) -> &mut Self::Target {{ &mut self.{BASE_FIELD} }} }}"
    )
}

/// The inherent method a polymorphic class erases ITSELF through (its
/// `IntoSmeltUnknown` dispatches to the concrete cell's erasure instead).
pub(crate) const OWN_ERASURE_METHOD: &str = "__smelt_own_into_smelt_unknown";

/// How a descendant class's type arguments follow from an ancestor's, when
/// they do: each of the descendant's parameters (declaration order) is bound
/// from the ancestor argument at `param_sources[k]`, and the ancestor
/// arguments in `fixed` must be the given types (the descendant passed a
/// concrete type there: `class Sub extends Base<string>`).
pub(crate) struct DescendantBinding {
    /// The ancestor parameter position each descendant parameter comes from.
    pub(crate) param_sources: Vec<usize>,
    /// Ancestor positions the descendant fixes to a concrete type.
    pub(crate) fixed: Vec<(usize, smelt_hir::TypeId)>,
}

/// The binding of `descendant`'s type parameters from `ancestor`'s arguments,
/// or `None` when they do not follow (a parameter spelled inside another type,
/// `class Sub<U> extends Base<U[]>`, or one the ancestor never receives): a
/// handle of the ancestor type cannot then name that descendant's type.
pub(crate) fn descendant_binding(
    mir: &Mir,
    descendant: Symbol,
    ancestor: Symbol,
) -> Option<DescendantBinding> {
    let descendant_class = find_class(mir, descendant)?;
    let ancestor_class = find_class(mir, ancestor)?;
    let substitution = crate::classes::inherited_type_substitution(mir, descendant, ancestor);
    let own: Vec<Symbol> = descendant_class
        .type_params
        .iter()
        .map(|param| param.name)
        .collect();
    let mut sources: HashMap<Symbol, usize> = HashMap::new();
    let mut fixed = Vec::new();
    for (index, param) in ancestor_class.type_params.iter().enumerate() {
        let ty = *substitution.get(&param.name)?;
        match mir.types.get(ty) {
            Some(smelt_hir::Type::TypeParam { name }) if own.contains(name) => {
                if sources.insert(*name, index).is_some() {
                    return None;
                }
            }
            _ => {
                let mut mentioned = HashSet::new();
                generic_bindings::collect_type_param_occurrences(mir, ty, &mut mentioned);
                if !mentioned.is_empty() {
                    return None;
                }
                fixed.push((index, ty));
            }
        }
    }
    let param_sources = own
        .iter()
        .map(|name| sources.get(name).copied())
        .collect::<Option<Vec<_>>>()?;
    Some(DescendantBinding {
        param_sources,
        fixed,
    })
}

/// The upcast of the handle `value` of concrete-or-polymorphic class
/// `source` into the polymorphic ancestor `target`'s handle type named
/// `target_type`: the very same `Rc`, unsized (a concrete source) or
/// re-viewed through the source trait's upcast method (a polymorphic one).
pub(crate) fn upcast_handle_expr(
    context: &crate::emitter::EmitContext,
    source: Symbol,
    target: Symbol,
    value: &str,
) -> Result<String, crate::EmitError> {
    let target_type = context.names().type_name(target)?.as_str().to_owned();
    Ok(if context.is_polymorphic_class(source) {
        format!(
            "{target_type}({value}.0.clone().{}())",
            upcast_method_name(&target_type)
        )
    } else {
        format!("{target_type}({value}.0.clone())")
    })
}

/// The binder an upcast of a recovered subclass origin reads
/// (`reference_origin`).
pub(crate) const SUBCLASS_ORIGIN_BINDER: &str = "smelt_subclass_origin";

/// The subclass handle types `class`'s `SmeltFromUnknown` takes back as the
/// same object, each with the upcast of the recovered `smelt_subclass_origin`.
///
/// A subclass whose type arguments do not follow from `class`'s own
/// parameters ([`descendant_binding`], or a generic `class` the subclass
/// fixes to a concrete type) cannot be named inside the generic impl; it is
/// rebuilt from the erased snapshot as before.
pub(crate) fn subclass_origins(
    mir: &Mir,
    context: &crate::emitter::EmitContext,
    class: &MirClass,
) -> Result<Vec<(String, String)>, crate::EmitError> {
    let own_params = class
        .type_params
        .iter()
        .map(|param| {
            mir.symbols
                .get(param.name)
                .map(|name| crate::names::type_param(name).into_string())
                .ok_or_else(|| crate::EmitError::new("class type parameter has unknown symbol"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let generic = !class.type_params.is_empty();
    subclass_origins_at(mir, context, class, &own_params, &|_, _| !generic)
}

/// [`subclass_origins`] for a `class` instantiated at the argument texts
/// `args`; `fixed_matches(index, ty)` says whether a subclass that fixes the
/// argument at `index` to `ty` is an instance of that instantiation.
pub(crate) fn subclass_origins_at(
    mir: &Mir,
    context: &crate::emitter::EmitContext,
    class: &MirClass,
    args: &[String],
    fixed_matches: &dyn Fn(usize, smelt_hir::TypeId) -> bool,
) -> Result<Vec<(String, String)>, crate::EmitError> {
    let mut origins = Vec::new();
    for descendant in concrete_classes(mir, context.polymorphic_classes(), class.name) {
        if descendant == class.name {
            continue;
        }
        let Some(binding) = descendant_binding(mir, descendant, class.name) else {
            continue;
        };
        if !binding
            .fixed
            .iter()
            .all(|(index, ty)| fixed_matches(*index, *ty))
        {
            continue;
        }
        let Some(bound_args) = binding
            .param_sources
            .iter()
            .map(|index| args.get(*index).cloned())
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let descendant_args = context
            .type_param_elision()
            .retain_carried(descendant, bound_args);
        let descendant_type = context.names().type_name(descendant)?.as_str().to_owned();
        let handle = if descendant_args.is_empty() {
            descendant_type
        } else {
            format!("{descendant_type}<{}>", descendant_args.join(", "))
        };
        origins.push((
            handle,
            upcast_handle_expr(context, descendant, class.name, SUBCLASS_ORIGIN_BINDER)?,
        ));
    }
    Ok(origins)
}

/// The inner-record literal of reference class `class` from its flat field
/// initializers `parts` (`"name: value"`, in any order, as every record
/// literal of the emitter builds them).
///
/// A class that nests its base (`crate::class_polymorphism`) distributes the
/// parts over its levels: each base level's own fields (and its phantom, when
/// it declares one) go into that level's record, and the record of each
/// level is the [`BASE_FIELD`] of the one below it. Any other class's literal
/// is the flat `NameInner { .. }` it always was.
pub(crate) fn inner_literal(
    mir: &Mir,
    context: &crate::emitter::EmitContext,
    class: Symbol,
    parts: &[String],
) -> Result<String, crate::EmitError> {
    let type_name = |symbol: Symbol| -> Result<String, crate::EmitError> {
        Ok(context.names().type_name(symbol)?.as_str().to_owned())
    };
    let polymorphic = context.polymorphic_classes();
    // A widened level's record holds its view slot, empty on every record a
    // literal builds (`crate::class_widening`).
    let slot_part = format!("{}: None", crate::class_widening::VIEW_SLOT_FIELD);
    if nested_base(mir, polymorphic, class).is_none() {
        let mut flat_parts = parts.to_vec();
        if context.is_widened_class(class) {
            flat_parts.push(slot_part);
        }
        return Ok(format!(
            "{}Inner {{ {} }}",
            type_name(class)?,
            flat_parts.join(", ")
        ));
    }
    // Levels from the class itself up to the topmost nested base.
    let mut levels = vec![class];
    let mut seen = HashSet::from([class]);
    while let Some(base) = levels
        .last()
        .and_then(|last| nested_base(mir, polymorphic, *last))
    {
        if !seen.insert(base) {
            break;
        }
        levels.push(base);
    }
    let part_name = |part: &str| part.split(':').next().unwrap_or("").trim().to_owned();
    let mut level_parts: Vec<Vec<String>> = vec![Vec::new(); levels.len()];
    let mut owner_of: HashMap<String, usize> = HashMap::new();
    for (index, level) in levels.iter().enumerate() {
        let Some(level_class) = find_class(mir, *level) else {
            continue;
        };
        for field in level_fields(mir, polymorphic, level_class) {
            owner_of.insert(context.names().field_text(mir, field.name), index);
        }
        if index > 0
            && context
                .type_param_elision()
                .emits_phantom(level_class.name, level_class.type_params.len())
            && let Some(bucket) = level_parts.get_mut(index)
        {
            bucket.push("_smelt_phantom: ::std::marker::PhantomData".to_owned());
        }
        if context.is_widened_class(level_class.name)
            && let Some(bucket) = level_parts.get_mut(index)
        {
            bucket.push(slot_part.clone());
        }
    }
    for part in parts {
        let name = part_name(part);
        let index = owner_of.get(&name).copied().unwrap_or(0);
        if let Some(bucket) = level_parts.get_mut(index) {
            bucket.push(part.clone());
        }
    }
    let mut literal = String::new();
    for (index, level) in levels.iter().enumerate().rev() {
        let mut fields = Vec::new();
        if !literal.is_empty() {
            fields.push(format!("{BASE_FIELD}: {literal}"));
        }
        fields.extend(level_parts.get(index).into_iter().flatten().cloned());
        literal = format!("{}Inner {{ {} }}", type_name(*level)?, fields.join(", "));
    }
    Ok(literal)
}

/// The inherent method holding a polymorphic class's generated field
/// projection, which [`OWN_ERASURE_METHOD`] falls back to when no emitter can
/// render the inline projection.
pub(crate) const PROJECTED_ERASURE_METHOD: &str = "__smelt_project_into_smelt_unknown";

/// The [`OWN_ERASURE_METHOD`] of `class`, rendered by a function emitter
/// hosted on one of the class's own functions (any works: only the class's
/// type parameters must be in scope).
pub(crate) fn own_erasure_method(
    mir: &Mir,
    context: &crate::emitter::EmitContext,
    class: &MirClass,
) -> Result<String, crate::EmitError> {
    let fallback = if context.is_polymorphic_class(class.name) {
        format!("smelt_own.{PROJECTED_ERASURE_METHOD}()")
    } else {
        "IntoSmeltUnknown::into_smelt_unknown(smelt_own)".to_owned()
    };
    let candidate = class
        .constructor
        .into_iter()
        .chain(crate::classes::effective_class_methods(mir, class))
        .find_map(|function| mir.functions.get(usize::try_from(function.0).ok()?));
    // A generic class erases through its generated projection: the inline
    // projection renders each callable slot's erased adapter, whose
    // by-reference `&T` parameters it cannot spell for a class parameter.
    let Some(host) = candidate.filter(|_| class.type_params.is_empty()) else {
        return Ok(format!(
            "    #[allow(dead_code)]\n    fn {OWN_ERASURE_METHOD}(self) -> SmeltUnknown {{ let smelt_own = self; {fallback} }}\n"
        ));
    };
    // The class at its own parameters, as the host function's locals spell it.
    let class_ty = host
        .locals
        .iter()
        .map(|local| local.ty)
        .chain(std::iter::once(host.return_ty))
        .find(|ty| match mir.types.get(*ty) {
            Some(smelt_hir::Type::Class { name, args }) if *name == class.name => {
                args.len() == class.type_params.len()
                    && args.iter().zip(&class.type_params).all(|(arg, param)| {
                        matches!(
                            mir.types.get(*arg),
                            Some(smelt_hir::Type::TypeParam { name: arg_param }) if *arg_param == param.name
                        )
                    })
            }
            _ => false,
        });
    let emitter = crate::emitter::FunctionEmitter::new(mir, context, host)?;
    emitter.own_erasure_method_text(class_ty, &fallback)
}

/// The function implementing the method named `method` for instances of
/// `class`: its own declaration, else the nearest ancestor's.
pub(crate) fn implementing_method(
    mir: &Mir,
    class: Symbol,
    method: Symbol,
) -> Option<smelt_mir::FuncId> {
    let mut current = Some(class);
    let mut seen = HashSet::new();
    while let Some(name) = current
        && seen.insert(name)
    {
        let found = find_class(mir, name)?;
        for id in &found.methods {
            let function = mir.functions.get(usize::try_from(id.0).ok()?)?;
            if matches!(function.origin, smelt_mir::HirOrigin::ClassMethod { method: own, .. } if own == method)
            {
                return Some(*id);
            }
        }
        current = found.base;
    }
    None
}

/// Whether the method `method` of the polymorphic class `class` is
/// overridden by some instance class below it, so the class's own method
/// forwards to that override (`FunctionEmitter::override_dispatch_prologue`).
///
/// A class prototype's method (`Animal.prototype.speak.call(dog)`) runs the
/// class's own body whatever the receiver is, so such a method gets a copy
/// that does not forward, named by [`crate::classes::super_copy_name`] (the
/// copy a `super` call reaches is the same body).
pub(crate) fn method_overridden_below(
    mir: &Mir,
    polymorphic: &HashSet<Symbol>,
    class: Symbol,
    method: Symbol,
) -> bool {
    if !polymorphic.contains(&class) {
        return false;
    }
    let own = implementing_method(mir, class, method);
    concrete_classes(mir, polymorphic, class)
        .into_iter()
        .filter(|concrete| *concrete != class)
        .any(|concrete| implementing_method(mir, concrete, method) != own)
}

/// The ancestor level of a nested layout that stores `field` of `class`,
/// when it is not `class`'s own level (see [`level_fields`]).
pub(crate) fn storing_level(
    mir: &Mir,
    polymorphic: &HashSet<Symbol>,
    class: Symbol,
    field: Symbol,
) -> Option<Symbol> {
    let stores = |level: Symbol| {
        find_class(mir, level).is_some_and(|found| {
            level_fields(mir, polymorphic, found)
                .iter()
                .any(|candidate| candidate.name == field)
        })
    };
    if stores(class) {
        return None;
    }
    let mut level = class;
    let mut seen = HashSet::from([class]);
    while let Some(base) = nested_base(mir, polymorphic, level) {
        if !seen.insert(base) {
            return None;
        }
        if stores(base) {
            return Some(base);
        }
        level = base;
    }
    None
}

/// The type a nested layout stores `field` of `class` at, when a nested
/// ancestor level stores it at another type than `class`'s own redeclared
/// one (`class Dog extends Animal { declare food: string }`); `None` when
/// the field is stored at `class`'s own type.
///
/// The stored type is the storing level's declaration read at `class`'s base
/// arguments, so a callable slot that only differs by its parameters' ABI
/// (`Fn(&T)` against `Fn(f64)`) is not a redeclaration.
pub(crate) fn redeclared_field_storage(
    mir: &Mir,
    polymorphic: &HashSet<Symbol>,
    class: Symbol,
    field: Symbol,
) -> Option<smelt_hir::TypeId> {
    let storing = storing_level(mir, polymorphic, class, field)?;
    let own_ty = effective_class_fields(mir, find_class(mir, class)?)
        .into_iter()
        .find(|candidate| candidate.name == field)?
        .ty;
    let declared = effective_class_fields(mir, find_class(mir, storing)?)
        .into_iter()
        .find(|candidate| candidate.name == field)?
        .ty;
    let substitution = crate::classes::inherited_type_substitution(mir, class, storing);
    let stored = generic_bindings::substitute_existing_type(mir, declared, &substitution);
    (stored != own_ty).then_some(stored)
}
