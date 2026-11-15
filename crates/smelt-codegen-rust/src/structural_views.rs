//! Structural views: a record handed to a slot of ANOTHER record type is the
//! same object there.
//!
//! # The defect this closes
//!
//! A reference record is `struct Point(Rc<RefCell<PointInner>>)`: one cell,
//! one JavaScript object. A slot of another record type (an interface, a
//! structural subset with mixed property types, the same properties declared
//! under another name) cannot hold that cell, so the conversion built a new
//! value of the slot's type, a copy
//! (`emitter::coercion::record_adapter`). With
//! `const a: Point = { x: 1, label: "a" }`:
//!
//! ```text
//! function rename(p: Named) { p.label = "renamed" }   rename(a); a.label   // renamed, was a
//! function keep(p: Named) { kept = p }                a.label = "z"; kept.label // z, was a
//! function same(p: Named): Named { return p }         same(a) === same(a)  // true, was false
//! ```
//!
//! # The representation
//!
//! What a hand-porting team writes for "any object that has these properties"
//! is a trait with one accessor pair per property, implemented by every record
//! that is used as one, and a handle holding a trait object. That is what a
//! *view target* becomes:
//!
//! ```text
//! struct Named(Rc<dyn NamedSmeltView>);          // any object used as a Named
//! struct NamedInner { label: JsString }          // a Named built as one
//! trait NamedSmeltView {
//!     fn label(&self) -> Ref<'_, JsString>;       // one pair per declared property,
//!     fn label_mut(&self) -> RefMut<'_, JsString>; // at the property's own type
//!     fn smelt_any(self: Rc<Self>) -> Rc<dyn Any>;       // checked casts back
//!     fn smelt_debug(&self, ..) -> fmt::Result;
//!     fn into_smelt_unknown_as_concrete(self: Rc<Self>) -> SmeltUnknown; // erasure (when the crate erases)
//!     fn smelt_as_Other(self: Rc<Self>) -> Rc<dyn OtherSmeltView>; // one per view target this one converts to
//! }
//! impl NamedSmeltView for RefCell<NamedInner> { .. }
//! impl NamedSmeltView for RefCell<PointInner> { .. }   // projects `&inner.label`
//! ```
//!
//! - **A conversion is an unsizing coercion** of the very same `Rc`
//!   (`Named(point.0.clone())`): one allocation, so identity (the cell
//!   address), every write in either direction and the object's open shape
//!   (`smelt-runtime/src/runtime/live_view.rs`, keyed by that address) hold
//!   through either handle. A view converted on to a further view goes
//!   through its trait's `smelt_as_<Target>` method.
//! - **The source keeps its concrete type** and its direct field access
//!   (`point.0.borrow().x`). Only a type that some OTHER record is converted
//!   to pays for dynamic dispatch on its property accesses, exactly as only a
//!   class with subclasses does (`crate::class_polymorphism`).
//! - **An accessor lends the property in place** (`Ref::map` over the
//!   source's cell), at the property's own type. No value is converted and
//!   none is erased: a view exists only between records that declare the
//!   property at the SAME type ([`shares`]).
//! - **Whole-object operations ask the object, not the view.** Erasure
//!   (`into_smelt_unknown_as_concrete`) hands out the concrete record's own
//!   erased view, so `Object.keys`, `JSON.stringify`, inspection and deep
//!   equality of a `Named` that is a `Point` enumerate the `Point`'s
//!   properties, as JavaScript does.
//! - **A cast back is checked** through `smelt_any`
//!   (`emitter::structural_view`): the handle of the record the object was
//!   built as, when it is one, else the field-wise conversion as before.
//!
//! # The rule
//!
//! [`StructuralViews::plan`] reads the program once. A record type `S`
//! *converts to* a record type `T` when some value flow
//! (`classify::for_each_value_flow_into_parts`) hands a value whose type holds
//! `S` to a slot whose type holds `T` at the same position (the value itself,
//! an array element, a tuple member, a dictionary value, through `Optional`),
//! and `S` [`shares`] with `T`: both are plain object shapes
//! ([`is_viewable_shape`]) and every property `T` declares is declared by `S`
//! at the same type. `T` is then a view target, every shape that reaches it
//! through such conversions implements its trait, and both are reference
//! records (`classify::reference_classes`).
//!
//! # Requirements
//!
//! Behavior, for every converted pair `S -> T`, a value `s: S` and its view
//! `t: T`:
//!
//! - a write of a property through `t` is read back from `s`, and a write to
//!   `s` is read back through `t`, however long `t` was held;
//! - `t` converted back, or two views of `s` compared, are the same object
//!   (`===`);
//! - a whole-object read of `t` (keys, JSON, inspection) enumerates the
//!   properties of `s` in `s`'s own key order.
//!
//! Type preservation: `S` keeps its struct and its fields their types; `T`'s
//! accessors are typed by `T`'s own property types; no value is erased by a
//! conversion, and the erased carrier appears only in the erasure method,
//! which is the typed-to-erased crossing the record already had.
//!
//! # What still copies
//!
//! A pair that does not [`shares`]: `T` declares a property at another type
//! than `S` does (`{ id: string | number }` over `{ id: string }`), `T`
//! declares an optional property `S` lacks, a class instance on either side
//! (its view carries prototype members, `emitter::view_write_through`), a
//! generic, callable or accessor-backed shape. Those keep the field-wise
//! conversion of `record_adapter`.
#![expect(
    clippy::redundant_pub_crate,
    reason = "view planning is shared by classification, the storage emitters and the function emitter"
)]

use std::collections::{BTreeSet, HashMap, HashSet};

use smelt_hir::{Symbol, Type, TypeId};
use smelt_mir::{Mir, MirField, MirInterface};

/// The Rust name of the view trait of a view target whose handle type is
/// named `type_name`; claimed with the type
/// (`crate::class_polymorphism::REFERENCE_COMPANIONS`).
pub(crate) fn view_trait_name(type_name: &str) -> String {
    format!("{type_name}SmeltView")
}

/// The conversion method a view trait carries toward the view target whose
/// handle type is named `target_type_name`.
pub(crate) fn upcast_method_name(target_type_name: &str) -> String {
    format!("smelt_as_{target_type_name}")
}

/// The inherent method a view target erases its OWN cell through (its
/// `IntoSmeltUnknown` dispatches to the concrete cell's erasure instead).
pub(crate) const OWN_ERASURE_METHOD: &str = "__smelt_own_into_smelt_unknown";

/// The object shape named `name`, when it is one a view can stand for or
/// over: a non-generic `interface` or object type literal that is not a
/// class, not a callable object, has no accessor-backed property and no
/// index-signature store.
///
/// The same shapes `emitter::live_view` makes live records, for the same
/// reasons: each of the excluded kinds has storage or an erased form the
/// shared cell does not spell.
pub(crate) fn is_viewable_shape(mir: &Mir, name: Symbol) -> Option<&MirInterface> {
    let interface = mir
        .interfaces
        .iter()
        .find(|interface| interface.name == name)?;
    (interface.type_params.is_empty()
        && crate::classes::find_class(mir, name).is_none()
        && !crate::classify::is_callable_shape(mir, interface)
        && !crate::classify::has_accessor_fields(mir, interface)
        && !crate::classify::class_has_index_store(mir, name))
    .then_some(interface)
}

/// Whether a value of shape `source` can be viewed as shape `target` in
/// place: they are two distinct viewable shapes, and every property `target`
/// declares is declared by `source` at the same type.
///
/// The same type, not an assignable one: an accessor lends the source's own
/// field (`Ref<'_, F>`), which is only a `target` property when the two types
/// are one. TypeScript also accepts a narrower source property
/// (`{ id: string }` as `{ id: string | number }`); such a pair keeps the
/// field-wise conversion.
pub(crate) fn shares(mir: &Mir, source: Symbol, target: Symbol) -> bool {
    if source == target {
        return false;
    }
    let (Some(source_shape), Some(target_shape)) = (
        is_viewable_shape(mir, source),
        is_viewable_shape(mir, target),
    ) else {
        return false;
    };
    let source_fields = crate::classes::effective_interface_fields(mir, source_shape);
    let target_fields = crate::classes::effective_interface_fields(mir, target_shape);
    !target_fields.is_empty()
        && target_fields.iter().all(|wanted| {
            source_fields
                .iter()
                .any(|held| held.name == wanted.name && held.ty == wanted.ty)
        })
}

/// Call `pair(source, target)` for every record type of `source` that sits at
/// the position of another record type in `slot`: the two types themselves,
/// or the same position inside matching `Optional`, array, tuple and
/// dictionary-value types.
fn for_each_record_pair(
    mir: &Mir,
    source: TypeId,
    slot: TypeId,
    depth: u8,
    pair: &mut impl FnMut(Symbol, Symbol),
) {
    if depth > 8 || source == slot {
        return;
    }
    let strip = |mut ty: TypeId| {
        while let Some(Type::Optional(inner)) = mir.types.get(ty) {
            ty = *inner;
        }
        ty
    };
    let (source, slot) = (strip(source), strip(slot));
    let deeper = depth.saturating_add(1);
    match (mir.types.get(source), mir.types.get(slot)) {
        (Some(Type::Class { name: from, args }), Some(Type::Class { name: to, args: to_args }))
            if args.is_empty() && to_args.is_empty() =>
        {
            pair(*from, *to);
        }
        (Some(Type::List(from)), Some(Type::List(to))) => {
            for_each_record_pair(mir, *from, *to, deeper, pair);
        }
        (Some(Type::Dict(_, from)), Some(Type::Dict(_, to))) => {
            for_each_record_pair(mir, *from, *to, deeper, pair);
        }
        (Some(Type::Tuple(from)), Some(Type::Tuple(to))) if from.len() == to.len() => {
            for (from_item, to_item) in from.iter().zip(to) {
                for_each_record_pair(mir, *from_item, *to_item, deeper, pair);
            }
        }
        _ => {}
    }
}

/// The structural views of a program (see the module docs).
#[derive(Debug, Default)]
pub(crate) struct StructuralViews {
    /// Each view target with the shapes some value flow converts straight
    /// to it.
    direct_sources: HashMap<Symbol, Vec<Symbol>>,
    /// Each view target with every OTHER shape whose cell implements its
    /// trait (the shapes that reach it through conversions), in declaration
    /// order.
    implementors: HashMap<Symbol, Vec<Symbol>>,
    /// Each shape with every view target it converts to (straight, or
    /// through other views), in declaration order.
    targets: HashMap<Symbol, Vec<Symbol>>,
}

impl StructuralViews {
    /// Plan the views of `mir`: one `source -> target` conversion per value
    /// flow between two shapes that [`shares`].
    pub(crate) fn plan(mir: &Mir) -> Self {
        let mut edges: BTreeSet<(usize, usize)> = BTreeSet::new();
        let position: HashMap<Symbol, usize> = mir
            .interfaces
            .iter()
            .enumerate()
            .map(|(index, interface)| (interface.name, index))
            .collect();
        crate::classify::for_each_value_flow_into_parts(mir, |source, slot| {
            for_each_record_pair(mir, source, slot, 0, &mut |from, to| {
                if shares(mir, from, to)
                    && let (Some(from_index), Some(to_index)) =
                        (position.get(&from), position.get(&to))
                {
                    edges.insert((*from_index, *to_index));
                }
            });
        });
        let name_at = |index: usize| mir.interfaces.get(index).map(|interface| interface.name);
        let mut direct_sources: HashMap<Symbol, Vec<Symbol>> = HashMap::new();
        for (from, to) in &edges {
            if let (Some(source), Some(target)) = (name_at(*from), name_at(*to)) {
                direct_sources.entry(target).or_default().push(source);
            }
        }
        // Every shape that reaches a target through conversions implements
        // the target's trait: a `Point` viewed as a `Named` and converted on
        // to a `Labelled` is still that `Point`'s cell.
        let mut implementors: HashMap<Symbol, Vec<Symbol>> = HashMap::new();
        for target in direct_sources.keys() {
            let mut reached: HashSet<Symbol> = HashSet::new();
            let mut pending = vec![*target];
            while let Some(current) = pending.pop() {
                for source in direct_sources.get(&current).into_iter().flatten() {
                    if *source != *target && reached.insert(*source) {
                        pending.push(*source);
                    }
                }
            }
            let mut ordered: Vec<Symbol> = reached.into_iter().collect();
            ordered.sort_by_key(|name| position.get(name).copied());
            implementors.insert(*target, ordered);
        }
        let mut targets: HashMap<Symbol, Vec<Symbol>> = HashMap::new();
        for (target, sources) in &implementors {
            for source in sources {
                targets.entry(*source).or_default().push(*target);
            }
        }
        for found in targets.values_mut() {
            found.sort_by_key(|name| position.get(name).copied());
        }
        Self {
            direct_sources,
            implementors,
            targets,
        }
    }

    /// Whether `name` is a view target: its handle holds a trait object.
    pub(crate) fn is_target(&self, name: Symbol) -> bool {
        self.direct_sources.contains_key(&name)
    }

    /// Whether a value of shape `source` converts to the view target
    /// `target` in place (directly or through other views).
    pub(crate) fn converts(&self, source: Symbol, target: Symbol) -> bool {
        self.implementors
            .get(&target)
            .is_some_and(|found| found.contains(&source))
    }

    /// The other shapes whose cell implements `target`'s trait, in
    /// declaration order.
    pub(crate) fn implementors(&self, target: Symbol) -> &[Symbol] {
        self.implementors.get(&target).map_or(&[], Vec::as_slice)
    }

    /// Every view target `source` converts to, in declaration order.
    pub(crate) fn targets(&self, source: Symbol) -> &[Symbol] {
        self.targets.get(&source).map_or(&[], Vec::as_slice)
    }

    /// Every shape that takes part in a view, as a source or as a target:
    /// the records `classify::reference_classes` lifts for it.
    pub(crate) fn participants(&self) -> impl Iterator<Item = Symbol> + '_ {
        self.direct_sources
            .keys()
            .chain(self.targets.keys())
            .copied()
    }
}

/// The accessor names of one property of a view trait: the shared borrow and
/// the mutable one.
pub(crate) struct AccessorNames {
    /// `fn <get>(&self) -> Ref<'_, F>`.
    pub(crate) get: String,
    /// `fn <get_mut>(&self) -> RefMut<'_, F>`.
    pub(crate) get_mut: String,
}

/// The trait methods every view trait declares besides its accessors.
const RESERVED_METHODS: &[&str] = &[
    "smelt_any",
    "smelt_debug",
    "into_smelt_unknown_as_concrete",
];

/// The accessor names of the properties `fields` of one view target, by
/// field symbol.
///
/// A property `label` is read through `label()` and written through
/// `label_mut()`. When two of those names would meet (a record declaring both
/// `label` and `label_mut`, or a property named like one of the trait's own
/// methods) every accessor of that trait takes the `smelt_field_` prefix
/// instead, which no planned field name carries.
pub(crate) fn accessor_names(
    mir: &Mir,
    context: &crate::emitter::EmitContext,
    fields: &[MirField],
) -> HashMap<Symbol, AccessorNames> {
    let plain = |field: &MirField| context.names().field_text(mir, field.name);
    let named = |prefix: &str| -> HashMap<Symbol, AccessorNames> {
        fields
            .iter()
            .map(|field| {
                let ident = plain(field);
                let bare = ident.strip_prefix("r#").unwrap_or(&ident);
                let get = if prefix.is_empty() {
                    ident.clone()
                } else {
                    format!("{prefix}{bare}")
                };
                (
                    field.name,
                    AccessorNames {
                        get_mut: format!("{prefix}{bare}_mut"),
                        get,
                    },
                )
            })
            .collect()
    };
    let direct = named("");
    let mut seen: HashSet<&str> = RESERVED_METHODS.iter().copied().collect();
    let clash = direct.values().any(|names| {
        !seen.insert(names.get.strip_prefix("r#").unwrap_or(&names.get))
            || !seen.insert(names.get_mut.as_str())
            || names.get.starts_with("smelt_as_")
            || names.get.starts_with("r#smelt_as_")
    });
    if clash {
        named("smelt_field_")
    } else {
        direct
    }
}

/// The declaration of `target`'s view trait (see the module docs).
///
/// `field_types` gives each of `target`'s properties its rendered Rust type,
/// in declaration order. `erases` is whether the crate carries the erased
/// runtime, so the trait declares the erasure method.
pub(crate) fn view_trait_declaration(
    mir: &Mir,
    context: &crate::emitter::EmitContext,
    target: Symbol,
    fields: &[MirField],
    field_types: &[String],
    erases: bool,
) -> Result<String, crate::EmitError> {
    let type_name = context.names().type_name(target)?.as_str().to_owned();
    let names = accessor_names(mir, context, fields);
    let mut methods = Vec::new();
    for (field, field_type) in fields.iter().zip(field_types) {
        let Some(accessors) = names.get(&field.name) else {
            continue;
        };
        methods.push(format!(
            "fn {}(&self) -> ::std::cell::Ref<'_, {field_type}>;",
            accessors.get
        ));
        methods.push(format!(
            "fn {}(&self) -> ::std::cell::RefMut<'_, {field_type}>;",
            accessors.get_mut
        ));
    }
    methods.push(
        "fn smelt_any(self: ::std::rc::Rc<Self>) -> ::std::rc::Rc<dyn ::std::any::Any>;".to_owned(),
    );
    methods.push(
        "fn smelt_debug(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result;"
            .to_owned(),
    );
    if erases {
        methods.push(
            "fn into_smelt_unknown_as_concrete(self: ::std::rc::Rc<Self>) -> SmeltUnknown;"
                .to_owned(),
        );
    }
    for further in context.structural_views().targets(target) {
        let further_type = context.names().type_name(*further)?.as_str().to_owned();
        methods.push(format!(
            "fn {}(self: ::std::rc::Rc<Self>) -> ::std::rc::Rc<dyn {}>;",
            upcast_method_name(&further_type),
            view_trait_name(&further_type)
        ));
    }
    Ok(format!(
        "#[allow(dead_code, non_snake_case)]\ntrait {} {{ {} }}",
        view_trait_name(&type_name),
        methods.join(" ")
    ))
}

/// `impl <Target>SmeltView for RefCell<<Concrete>Inner>`: the cell of the
/// shape `concrete` (the target itself, or a shape that converts to it)
/// answering `target`'s properties from its own fields.
pub(crate) fn view_impl_text(
    mir: &Mir,
    context: &crate::emitter::EmitContext,
    target: Symbol,
    concrete: Symbol,
    fields: &[MirField],
    field_types: &[String],
    erases: bool,
) -> Result<String, crate::EmitError> {
    let target_type = context.names().type_name(target)?.as_str().to_owned();
    let concrete_type = context.names().type_name(concrete)?.as_str().to_owned();
    let names = accessor_names(mir, context, fields);
    let mut methods = Vec::new();
    for (field, field_type) in fields.iter().zip(field_types) {
        let Some(accessors) = names.get(&field.name) else {
            continue;
        };
        // One symbol is one planned field name, so the concrete record
        // stores this property under the same Rust field.
        let stored = context.names().field_text(mir, field.name);
        methods.push(format!(
            "fn {}(&self) -> ::std::cell::Ref<'_, {field_type}> {{ ::std::cell::Ref::map(::std::cell::RefCell::borrow(self), |smelt_inner| &smelt_inner.{stored}) }}",
            accessors.get
        ));
        methods.push(format!(
            "fn {}(&self) -> ::std::cell::RefMut<'_, {field_type}> {{ ::std::cell::RefMut::map(::std::cell::RefCell::borrow_mut(self), |smelt_inner| &mut smelt_inner.{stored}) }}",
            accessors.get_mut
        ));
    }
    methods.push(
        "fn smelt_any(self: ::std::rc::Rc<Self>) -> ::std::rc::Rc<dyn ::std::any::Any> { self }"
            .to_owned(),
    );
    methods.push(
        "fn smelt_debug(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { ::std::fmt::Debug::fmt(&*::std::cell::RefCell::borrow(self), formatter) }"
            .to_owned(),
    );
    if erases {
        // The object erases as the record it was built as: that record's own
        // erased view, with all of its properties. A view target's own cell
        // erases through its inherent method, never back through the trait.
        let erase = if context.structural_views().is_target(concrete) {
            format!("{concrete_type}(self).{OWN_ERASURE_METHOD}()")
        } else {
            format!("IntoSmeltUnknown::into_smelt_unknown({concrete_type}(self))")
        };
        methods.push(format!(
            "fn into_smelt_unknown_as_concrete(self: ::std::rc::Rc<Self>) -> SmeltUnknown {{ {erase} }}"
        ));
    }
    for further in context.structural_views().targets(target) {
        let further_type = context.names().type_name(*further)?.as_str().to_owned();
        methods.push(format!(
            "fn {}(self: ::std::rc::Rc<Self>) -> ::std::rc::Rc<dyn {}> {{ self }}",
            upcast_method_name(&further_type),
            view_trait_name(&further_type)
        ));
    }
    Ok(format!(
        "#[allow(non_snake_case)]\nimpl {} for ::std::cell::RefCell<{concrete_type}Inner> {{ {} }}",
        view_trait_name(&target_type),
        methods.join(" ")
    ))
}

/// The conversion of the handle `value` of shape `source` into the view
/// target `target`: the very same `Rc`, unsized (a concrete source) or
/// re-viewed through the source trait's conversion method (a source that is
/// a view target itself).
pub(crate) fn upcast_handle_expr(
    context: &crate::emitter::EmitContext,
    source: Symbol,
    target: Symbol,
    value: &str,
) -> Result<String, crate::EmitError> {
    let target_type = context.names().type_name(target)?.as_str().to_owned();
    Ok(if context.structural_views().is_target(source) {
        format!(
            "{target_type}({value}.0.clone().{}())",
            upcast_method_name(&target_type)
        )
    } else {
        format!("{target_type}({value}.0.clone())")
    })
}

/// The shapes whose erased origin `target`'s `SmeltFromUnknown` takes back as
/// the same object: each implementor's handle type with the conversion of the
/// recovered `smelt_subclass_origin` (`reference_origin`).
pub(crate) fn implementor_origins(
    context: &crate::emitter::EmitContext,
    target: Symbol,
) -> Result<Vec<(String, String)>, crate::EmitError> {
    let views = context.structural_views();
    let mut origins = Vec::new();
    for implementor in views.implementors(target) {
        let handle = context.names().type_name(*implementor)?.as_str().to_owned();
        origins.push((
            handle,
            upcast_handle_expr(
                context,
                *implementor,
                target,
                crate::class_polymorphism::SUBCLASS_ORIGIN_BINDER,
            )?,
        ));
    }
    Ok(origins)
}
