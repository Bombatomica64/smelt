//! Widened generic class handles: one object behind every instantiation it is
//! seen at.
//!
//! # The defect this closes
//!
//! TypeScript lets a generic class instance flow to an instantiation with
//! other type arguments: `const wide: Parser<unknown> = new Parser<number>()`,
//! a `Parser<number>` element of a `Parser<any>[]`. JavaScript has one object:
//! `wide === p`, a field written through either handle is seen through the
//! other, and a method called through `wide` runs on `p`'s state (and on its
//! subclass's override). The cell of a generated generic class is typed at the
//! arguments it was built with (`ParserInner<f64>`), so a `Parser<SmeltUnknown>`
//! handle cannot hold it, and Smelt used to rebuild a NEW `Parser<SmeltUnknown>`
//! from the fields: a copy.
//!
//! # The representation
//!
//! A *widened* class ([`widened_classes`]) keeps its handle exactly as before
//! (`Parser<V>(Rc<RefCell<ParserInner<V>>>)`, or its hierarchy's trait object),
//! so a handle at known, unchanged arguments keeps the direct
//! `Ref<ParserInner<f64>>` path. Its record gains one hidden slot,
//! `__smelt_view: Option<Rc<dyn ParserSmeltView<V>>>`, `None` on every cell a
//! constructor builds. A handle seen at other arguments is a *placeholder*
//! cell whose slot holds a converting view of the original:
//!
//! ```text
//! trait ParserSmeltView<V> { fn smelt_read_items(&self) -> SmeltList<V>; fn smelt_write_items(&self, value: SmeltList<V>); .. }
//! struct ParserSmeltViewOf<V, SmeltWV> { source: Parser<SmeltWV>, root: Rc<dyn Any>, .. }
//! ```
//!
//! - **A conversion between instantiations keeps the object**
//!   (`Parser::<V>::smelt_retarget`): a source already at `V` is that very
//!   handle, a view whose root handle is at `V` unwraps to the root
//!   (narrowing back hands out the original), anything else becomes a
//!   placeholder over a view of it.
//! - **Identity, erasure, class name and checked downcasts** answer the
//!   source's: `===` and `PartialEq` compare `smelt_identity`, erasure and a
//!   hierarchy's cell-trait probes forward through the slot, so `instanceof`
//!   and override dispatch see the constructed class.
//! - **Fields** are read and written through the handle's accessors
//!   (`smelt_read_<f>`/`smelt_write_<f>`) on every handle that may be a
//!   placeholder: its arguments mention `SmeltUnknown` or a type parameter
//!   ([`handle_may_be_view`]). On a real cell an accessor is the record's own
//!   `Ref`/`RefMut` after one inline slot test; on a placeholder it converts
//!   the source's field through the erased carrier with the class's own
//!   per-field adapters, which keep `null` and `undefined` apart, and the write
//!   guard stores the value back when the statement ends. A list field is
//!   spliced into the source's array, so it keeps its identity
//!   (`wide.items.push(x)` reaches `p.items`).
//! - **Methods** begin with a forward (`emitter::widened_view`): on a
//!   placeholder the call runs, out of line, as the source instantiation's
//!   erased prototype adapter (`__smelt_method:<name>`), so a method body (and
//!   every `self.0.borrow()` in it) only ever runs on a real cell.
//!
//! A callable field over the class's parameters (a virtual method slot
//! `parse: Rc<dyn Fn(JsString) -> Result_<T>>`) is handed out behind an
//! adapter that converts the arguments and the answer that mention them (a
//! `&T` argument through a converted local, a `&mut T` one converted back
//! after the call). A slot of any other shape has no such adapter; reading it
//! through a placeholder reports the gap (`smelt_widened_view_storage`)
//! instead of reading the placeholder.

#![expect(
    clippy::redundant_pub_crate,
    reason = "widening helpers are shared by the storage emitters and the function emitter"
)]

use std::collections::HashSet;

use smelt_hir::{Symbol, Type, TypeId};
use smelt_mir::{Mir, MirClass};

use crate::classes::find_class;

/// The companion name of a widened class's converting view type.
pub(crate) fn view_type_name(type_name: &str) -> String {
    format!("{type_name}SmeltView")
}

/// The accessor reading `field` (its Rust name) through a widened handle.
pub(crate) fn read_accessor(field: &str) -> String {
    format!("smelt_read_{}", field.trim_start_matches("r#"))
}

/// The accessor writing `field` (its Rust name) through a widened handle.
pub(crate) fn write_accessor(field: &str) -> String {
    format!("smelt_write_{}", field.trim_start_matches("r#"))
}

/// The inherent function erasing `field` (its Rust name) of a widened class at
/// the class's own type parameters.
pub(crate) fn erase_field_fn(field: &str) -> String {
    format!("smelt_erase_field_{}", field.trim_start_matches("r#"))
}

/// The inherent function rebuilding `field` (its Rust name) of a widened class
/// from the erased carrier at the class's own type parameters.
pub(crate) fn extract_field_fn(field: &str) -> String {
    format!("smelt_extract_field_{}", field.trim_start_matches("r#"))
}

/// The prototype key a method named `name` (its source spelling) is erased
/// under; the view forwards a call through it.
pub(crate) fn proto_method_key(name: &str) -> String {
    format!("__smelt_method:{name}")
}

/// Whether `ty` mentions the erased type anywhere.
fn mentions_unknown(mir: &Mir, ty: TypeId, seen: &mut HashSet<TypeId>) -> bool {
    if !seen.insert(ty) {
        return false;
    }
    match mir.types.get(ty) {
        Some(Type::Unknown) => true,
        Some(found) => found
            .children()
            .any(|child| mentions_unknown(mir, child, seen)),
        None => false,
    }
}

/// Whether `class` can be widened: a generic instance class stored as a
/// reference class with a plain field record (`classify`), whose handle
/// carries at least one type parameter (`crate::generic_elision`; a class
/// whose parameters are all elided has one handle type for every
/// instantiation).
fn widenable(
    mir: &Mir,
    references: &HashSet<Symbol>,
    elision: &crate::generic_elision::TypeParamElision,
    class: &MirClass,
) -> bool {
    class.statics_of.is_none()
        && !elision
            .retain_carried(class.name, class.type_params.iter().collect())
            .is_empty()
        && references.contains(&class.name)
        && !crate::classify::class_has_index_store(mir, class.name)
}

/// The generic classes whose instances may be seen at other type arguments
/// than they were built with.
///
/// A class is widened when the program names it at carried arguments
/// mentioning the erased type (`Parser<unknown>`, `Parser<any>`, `Box<unknown[]>`): that is
/// the instantiation any other one can flow to without a cast. Every generic
/// ancestor of a widened class is widened too, so a view can answer its
/// upcasts with views of its own.
pub(crate) fn widened_classes(
    mir: &Mir,
    references: &HashSet<Symbol>,
    elision: &crate::generic_elision::TypeParamElision,
) -> HashSet<Symbol> {
    let mut widened = HashSet::new();
    for ty in mir.types.all() {
        let Type::Class { name, args } = ty else {
            continue;
        };
        if args.is_empty() || widened.contains(name) {
            continue;
        }
        let Some(class) = find_class(mir, *name) else {
            continue;
        };
        if !widenable(mir, references, elision, class) {
            continue;
        }
        let mut seen = HashSet::new();
        if elision
            .retain_carried(*name, args.iter().collect())
            .into_iter()
            .any(|arg| mentions_unknown(mir, *arg, &mut seen))
        {
            widened.insert(*name);
        }
    }
    let mut pending: Vec<Symbol> = widened.iter().copied().collect();
    while let Some(name) = pending.pop() {
        let Some(base) = find_class(mir, name)
            .and_then(|class| class.base)
            .and_then(|base| find_class(mir, base))
        else {
            continue;
        };
        if widenable(mir, references, elision, base) && widened.insert(base.name) {
            pending.push(base.name);
        }
    }
    widened
}

/// Whether a handle of widened class type `ty` may hold a converting view:
/// its arguments mention the erased type or a type parameter (a generic body
/// may be instantiated at an erased argument). A handle at known arguments
/// (`Parser<f64>`) holds a cell built at them, or the root a narrowing
/// unwrapped.
pub(crate) fn handle_may_be_view(
    mir: &Mir,
    context: &crate::emitter::EmitContext,
    ty: TypeId,
) -> bool {
    let Some(Type::Class { name, args }) = mir.types.get(ty) else {
        return false;
    };
    if !context.is_widened_class(*name) {
        return false;
    }
    let mut seen = HashSet::new();
    context
        .type_param_elision()
        .retain_carried(*name, args.iter().collect())
        .into_iter()
        .any(|arg| {
            let mut params = HashSet::new();
            crate::generic_bindings::collect_type_param_occurrences(mir, *arg, &mut params);
            !params.is_empty() || mentions_unknown(mir, *arg, &mut seen)
        })
}

/// One field of a widened class as its accessors spell it.
pub(crate) struct WidenedField {
    /// The Rust field name.
    pub(crate) rust: String,
    /// The field's Rust type at the class's own type parameters.
    pub(crate) ty_text: String,
    /// Whether the field is an array, which a view splices back in place.
    pub(crate) is_list: bool,
    /// Whether the field's type mentions the class's type parameters. A field
    /// that does not is the same type at every instantiation, so a view hands
    /// out its source's own guards.
    pub(crate) generic: bool,
    /// A callable field over the class's parameters whose slot takes its
    /// arguments by value: each parameter's and the answer's Rust type, with
    /// whether it mentions the class's parameters. A view hands out the
    /// source's callable behind an adapter converting exactly those.
    pub(crate) callable: Option<(Vec<SlotParam>, (String, bool))>,
    /// The erasure of the field's value bound to `smelt_value` (a reference),
    /// or `None` for a field the erased carrier cannot take back (a callable
    /// slot): a view leaves its accessors to the record's own, which no view
    /// has, so such a field is never read through a view.
    pub(crate) erase: Option<String>,
}

/// How a callable slot takes one parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotPassing {
    /// By value (`T`).
    Value,
    /// By shared reference (`&T`, the slot ABI of a borrowed record).
    Shared,
    /// By mutable reference (`&mut T`, a parameter the callee mutates).
    Mutable,
}

/// One parameter of a callable slot a view adapts.
#[derive(Debug, Clone)]
pub(crate) struct SlotParam {
    /// The parameter's type without its reference (`Value`).
    pub(crate) ty_text: String,
    /// How the slot takes it.
    pub(crate) passing: SlotPassing,
    /// Whether the type mentions the class's parameters (only those convert).
    pub(crate) generic: bool,
}

impl SlotParam {
    /// The parameter as the slot's signature spells it (`&mut SmeltDate`).
    pub(crate) fn spelled(&self) -> String {
        match self.passing {
            SlotPassing::Value => self.ty_text.clone(),
            SlotPassing::Shared => format!("&{}", self.ty_text),
            SlotPassing::Mutable => format!("&mut {}", self.ty_text),
        }
    }
}

/// The top-level comma-separated parameter list of the rendered slot type
/// `::std::rc::Rc<dyn Fn(A, B<C, D>) -> R>`, and its return text (`()` when the
/// signature has none); `None` for any other shape.
fn slot_signature(ty_text: &str) -> Option<(Vec<String>, String)> {
    let inner = ty_text
        .strip_prefix("::std::rc::Rc<dyn Fn(")?
        .strip_suffix('>')?;
    let mut depth = 0usize;
    let mut params = Vec::new();
    let mut current = String::new();
    let mut tail = None;
    for (index, ch) in inner.char_indices() {
        match ch {
            '<' | '(' | '[' => depth = depth.checked_add(1)?,
            '>' | ']' => depth = depth.checked_sub(1)?,
            ')' if depth == 0 => {
                tail = Some(inner.get(index.checked_add(1)?..)?);
                break;
            }
            ')' => depth = depth.checked_sub(1)?,
            ',' if depth == 0 => {
                params.push(current.trim().to_owned());
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }
    if !current.trim().is_empty() {
        params.push(current.trim().to_owned());
    }
    let after = tail?.trim();
    let ret = if after.is_empty() {
        "()".to_owned()
    } else {
        after.strip_prefix("->")?.trim().to_owned()
    };
    Some((params, ret))
}

/// The fields of widened `class` (its own and every inherited one, each at the
/// type its declaring level stores it at).
pub(crate) fn widened_fields(
    mir: &Mir,
    context: &crate::emitter::EmitContext,
    class: &MirClass,
) -> Result<Vec<WidenedField>, crate::EmitError> {
    let scoped: HashSet<Symbol> = class.type_params.iter().map(|param| param.name).collect();
    let mut fields = Vec::new();
    for field in crate::classes::effective_class_fields(mir, class) {
        let stored_ty = crate::class_polymorphism::redeclared_field_storage(
            mir,
            context.polymorphic_classes(),
            class.name,
            field.name,
        )
        .unwrap_or(field.ty);
        let ty_text = crate::emitter::FunctionEmitter::type_text_for_with_scoped_type_params(
            mir,
            context,
            stored_ty,
            &crate::type_substitution::TypeSubstitution::lexical(&scoped),
        )?;
        let erase = if crate::classes::type_holds_function(
            mir,
            stored_ty,
            crate::classes::NominalStorage::Opaque,
        ) {
            None
        } else {
            crate::emitter::record_coercion::record_field_unknown_expr(
                mir,
                &crate::rust::RustExpr::path("smelt_value").cloned_value(),
                stored_ty,
            )
            .ok()
            .map(crate::rust::RustExpr::into_string)
        };
        let mentions = |ty: TypeId| {
            let mut found = HashSet::new();
            crate::generic_bindings::collect_type_param_occurrences(mir, ty, &mut found);
            found.iter().any(|param| scoped.contains(param))
        };
        let render = |ty: TypeId| {
            crate::emitter::FunctionEmitter::type_text_for_with_scoped_type_params(
                mir,
                context,
                ty,
                &crate::type_substitution::TypeSubstitution::lexical(&scoped),
            )
        };
        // A callable slot whose rendered signature spells exactly its
        // parameters (each by value, `&` or `&mut`) and its answer.
        let callable = match (mir.types.get(stored_ty), slot_signature(&ty_text)) {
            (Some(Type::Function(function)), Some((spelled, ret_text)))
                if function.rest.is_none() && spelled.len() == function.params.len() =>
            {
                let mut params = Vec::new();
                for (param, slot_text) in function.params.iter().zip(&spelled) {
                    let rendered = render(*param)?;
                    let passing = if *slot_text == rendered {
                        SlotPassing::Value
                    } else if *slot_text == format!("&mut {rendered}") {
                        SlotPassing::Mutable
                    } else if *slot_text == format!("&{rendered}") {
                        SlotPassing::Shared
                    } else {
                        params.clear();
                        break;
                    };
                    params.push(SlotParam {
                        ty_text: rendered,
                        passing,
                        generic: mentions(*param),
                    });
                }
                let ret = render(function.return_ty)?;
                (params.len() == function.params.len() && ret == ret_text)
                    .then(|| (params, (ret, mentions(function.return_ty))))
            }
            _ => None,
        };
        let mut mentioned = HashSet::new();
        crate::generic_bindings::collect_type_param_occurrences(mir, stored_ty, &mut mentioned);
        fields.push(WidenedField {
            rust: context.names().field(mir, field.name).into_string(),
            ty_text,
            is_list: matches!(mir.types.get(stored_ty), Some(Type::List(_))),
            generic: mentioned.iter().any(|param| scoped.contains(param)),
            callable,
            erase,
        });
    }
    Ok(fields)
}

/// `text` with every whole-word occurrence of a parameter in `idents` renamed
/// to its source-side spelling (`SmeltW<ident>`), the view's second set of
/// parameters.
pub(crate) fn source_side(text: &str, idents: &[String]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    let flush = |pending: &mut String, written: &mut String| {
        if idents.iter().any(|ident| ident == pending) {
            written.push_str("SmeltW");
        }
        written.push_str(pending);
        pending.clear();
    };
    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            word.push(ch);
        } else {
            flush(&mut word, &mut out);
            out.push(ch);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// What [`widened_items_text`] needs about the class being emitted.
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent facts about the emitted class, each read by one part of the text"
)]
pub(crate) struct WidenedClass<'a> {
    /// The handle type's Rust name (`Parser`).
    pub(crate) name: &'a str,
    /// The carried type parameter identifiers (`["V"]`).
    pub(crate) idents: &'a [String],
    /// The bounded generics of the class's impls (`<V: Clone + ..>`).
    pub(crate) impl_generics: &'a str,
    /// The class's type arguments at its own parameters (`<V>`).
    pub(crate) type_args: &'a str,
    /// The class's source name, for the gap report.
    pub(crate) source_name: &'a str,
    /// Whether the class is a polymorphic handle (`crate::class_polymorphism`):
    /// the view then also answers the cell-trait probes.
    pub(crate) polymorphic: bool,
    /// Whether the class has a cell trait of its own: a polymorphic class
    /// upcasts through its trait's `smelt_as_<ancestor>`, a leaf subclass by
    /// unsizing its concrete cell.
    pub(crate) own_trait: bool,
    /// The cell-trait upcasts toward polymorphic ancestors: the method name,
    /// the ancestor's handle type name and its arguments at this class's
    /// parameters (`""` for a non-generic ancestor).
    pub(crate) upcasts: Vec<(String, String, String)>,
    /// Whether the crate erases (the view forwards erasure and methods).
    pub(crate) needs_unknown: bool,
    /// Whether the class emits `__smelt_proto_entries`, which the view
    /// forwards method calls through.
    pub(crate) has_proto_entries: bool,
    /// The instantiations a placeholder can exist at ([`viewable_instantiations`]).
    pub(crate) viewable: Vec<String>,
}

/// The hidden view slot's Rust type for widened class `name` at `type_args`.
pub(crate) fn view_slot_type(name: &str, type_args: &str) -> String {
    format!(
        "Option<::std::rc::Rc<dyn {}{type_args}>>",
        view_type_name(name)
    )
}

/// The Rust field holding a widened record's view slot.
pub(crate) const VIEW_SLOT_FIELD: &str = "__smelt_view";

/// The view-slot forward a cell-trait method of a widened class's own cell
/// begins with: a placeholder answers through its view.
pub(crate) fn cell_forward(call: &str) -> String {
    format!(
        "if let Some(smelt_view) = ::std::cell::RefCell::borrow(&self).{VIEW_SLOT_FIELD}.clone() {{ return smelt_view.{call}; }} "
    )
}

/// The view trait, the view type and its impl, and the class's inherent
/// accessors, per-field adapters, identity and `smelt_retarget`
/// (`crate::class_widening`).
pub(crate) fn widened_items_text(class: &WidenedClass<'_>, fields: &[WidenedField]) -> String {
    let WidenedClass {
        name,
        idents,
        impl_generics,
        type_args,
        source_name,
        polymorphic,
        own_trait,
        upcasts,
        needs_unknown,
        has_proto_entries,
        viewable,
    } = class;
    let may_view = if viewable.is_empty() {
        "false".to_owned()
    } else {
        viewable
            .iter()
            .map(|handle| format!("smelt_self == ::std::any::TypeId::of::<{handle}>()"))
            .collect::<Vec<_>>()
            .join(" || ")
    };
    let view_trait = view_type_name(name);
    let view = format!("{view_trait}Of");
    let source_args = source_side(type_args, idents);
    let source_idents: Vec<String> = idents
        .iter()
        .map(|ident| format!("SmeltW{ident}"))
        .collect();
    let view_params = format!("<{}, {}>", idents.join(", "), source_idents.join(", "));
    let source_bounds = source_side(impl_generics, idents);
    let view_generics = format!(
        "<{}, {}>",
        impl_generics.trim_start_matches('<').trim_end_matches('>'),
        source_bounds.trim_start_matches('<').trim_end_matches('>')
    );
    let source_generics = format!(
        "<{}>",
        source_bounds.trim_start_matches('<').trim_end_matches('>')
    );
    let gap = format!("smelt_widened_view_storage({source_name:?})");
    // The view trait (object-safe, per instantiation) and its impl over a
    // source at other arguments.
    let mut declared = vec![
        "fn smelt_identity(&self) -> *const ();".to_owned(),
        "fn smelt_root(&self) -> ::std::rc::Rc<dyn ::std::any::Any>;".to_owned(),
    ];
    let mut implemented = vec![
        "fn smelt_identity(&self) -> *const () { self.source.smelt_identity() }".to_owned(),
        "fn smelt_root(&self) -> ::std::rc::Rc<dyn ::std::any::Any> { ::std::rc::Rc::clone(&self.root) }".to_owned(),
    ];
    if *needs_unknown {
        declared.push("fn smelt_erase(&self) -> SmeltUnknown;".to_owned());
        implemented.push(
            "fn smelt_erase(&self) -> SmeltUnknown { IntoSmeltUnknown::into_smelt_unknown(self.source.clone()) }".to_owned(),
        );
        if *has_proto_entries {
            declared.push("fn smelt_method(&self, name: &str) -> Option<SmeltUnknown>;".to_owned());
            implemented.push(
                "fn smelt_method(&self, name: &str) -> Option<SmeltUnknown> { self.source.__smelt_proto_entries().into_iter().find_map(|(key, value)| (key == name).then_some(value)) }".to_owned(),
            );
        }
    }
    if *polymorphic {
        declared.push("fn smelt_any(&self) -> ::std::rc::Rc<dyn ::std::any::Any>;".to_owned());
        implemented.push("fn smelt_any(&self) -> ::std::rc::Rc<dyn ::std::any::Any> { ::std::rc::Rc::clone(&self.source.0).smelt_any() }".to_owned());
        declared.push("fn smelt_class_name(&self) -> &'static str;".to_owned());
        implemented.push(
            "fn smelt_class_name(&self) -> &'static str { self.source.0.smelt_class_name() }"
                .to_owned(),
        );
        for (method, ancestor, ancestor_args) in upcasts {
            let ancestor_cell = crate::class_polymorphism::cell_trait_name(ancestor);
            declared.push(format!(
                "fn {method}(&self) -> ::std::rc::Rc<dyn {ancestor_cell}{ancestor_args}>;"
            ));
            let source_cell = if *own_trait {
                format!("::std::rc::Rc::clone(&self.source.0).{method}()")
            } else {
                "self.source.0.clone()".to_owned()
            };
            let body = if ancestor_args.is_empty() {
                source_cell
            } else {
                format!(
                    "{ancestor}::{ancestor_args}::smelt_retarget({ancestor}::{}({source_cell})).0",
                    source_side(ancestor_args, idents)
                )
            };
            implemented.push(format!(
                "fn {method}(&self) -> ::std::rc::Rc<dyn {ancestor_cell}{ancestor_args}> {{ {body} }}"
            ));
        }
    }
    // The handle's accessors and the class's per-field adapters.
    let mut inherent = String::new();
    for field in fields {
        let WidenedField {
            rust,
            ty_text,
            is_list,
            generic,
            callable,
            erase,
        } = field;
        let read = read_accessor(rust);
        let write = write_accessor(rust);
        let (view_read, view_write) = (format!("{read}_view"), format!("{write}_view"));
        // What a placeholder answers: the source's own value for a field typed
        // alike at every instantiation, a conversion through the erased
        // carrier otherwise, the gap for a callable over the parameters.
        let forwarded = if !generic {
            declared.push(format!("fn {view_read}(&self) -> {ty_text};"));
            declared.push(format!("fn {view_write}(&self, value: {ty_text});"));
            implemented.push(format!(
                "fn {view_read}(&self) -> {ty_text} {{ self.source.{read}().clone() }}"
            ));
            implemented.push(format!(
                "fn {view_write}(&self, value: {ty_text}) {{ *self.source.{write}() = value; }}"
            ));
            true
        } else if let Some(erase_text) = erase {
            // Genuine dynamic boundary (`WIDENED_VIEW_MARKERS`): the field is
            // read at type arguments the object was not built with, and the
            // erased carrier is the only value both instantiations name. The
            // adapters are the class's own erasure and recovery, so `null`
            // and `undefined` stay apart.
            let erase_fn = erase_field_fn(rust);
            let extract_fn = extract_field_fn(rust);
            inherent.push_str(&format!(
                "    #[allow(dead_code)]\n    fn {erase_fn}(smelt_value: &{ty_text}) -> SmeltUnknown {{ {erase_text} }}\n    #[allow(dead_code)]\n    fn {extract_fn}(smelt_value: SmeltUnknown) -> {ty_text} {{ SmeltFromUnknown::smelt_from_unknown(smelt_value) }}\n"
            ));
            let converted = format!(
                "{name}::{type_args}::{extract_fn}({name}::{source_args}::{erase_fn}(&*smelt_source))"
            );
            let read_value = if *is_list {
                format!("SmeltList::with_id(smelt_source.id(), {converted}.to_vec())")
            } else {
                converted
            };
            let stored = format!(
                "{name}::{source_args}::{extract_fn}({name}::{type_args}::{erase_fn}(&value))"
            );
            let write_back = if *is_list {
                format!("self.source.{write}().replace_all({stored}.to_vec());")
            } else {
                format!("*self.source.{write}() = {stored};")
            };
            declared.push(format!("fn {view_read}(&self) -> {ty_text};"));
            declared.push(format!("fn {view_write}(&self, value: {ty_text});"));
            implemented.push(format!(
                "fn {view_read}(&self) -> {ty_text} {{ let smelt_source = self.source.{read}(); {read_value} }}"
            ));
            implemented.push(format!(
                "fn {view_write}(&self, value: {ty_text}) {{ {write_back} }}"
            ));
            true
        } else if let Some((params, ret)) = callable {
            // The source's callable behind an adapter: each argument at this
            // view's arguments goes to the source's, the answer comes back.
            let convert =
                "SmeltFromUnknown::smelt_from_unknown(IntoSmeltUnknown::into_smelt_unknown(VALUE))";
            // `to_source`: the closure is spelled at this view's arguments and
            // calls the source's slot; otherwise the reverse.
            let adapter = |to_source: bool| {
                let side = |text: &str| {
                    if to_source {
                        text.to_owned()
                    } else {
                        source_side(text, idents)
                    }
                };
                let mut declared_params = Vec::new();
                let mut before = String::new();
                let mut passed = Vec::new();
                let mut after = String::new();
                for (index, param) in params.iter().enumerate() {
                    let arg = format!("smelt_arg{index}");
                    declared_params.push(format!("{arg}: {}", side(&param.spelled())));
                    if !param.generic {
                        passed.push(arg);
                        continue;
                    }
                    let local = format!("smelt_converted{index}");
                    match param.passing {
                        SlotPassing::Value => passed.push(convert.replace("VALUE", &arg)),
                        SlotPassing::Shared => {
                            before.push_str(&format!(
                                "let {local} = {}; ",
                                convert.replace("VALUE", &format!("(*{arg}).clone()"))
                            ));
                            passed.push(format!("&{local}"));
                        }
                        SlotPassing::Mutable => {
                            before.push_str(&format!(
                                "let mut {local} = {}; ",
                                convert.replace("VALUE", &format!("(*{arg}).clone()"))
                            ));
                            passed.push(format!("&mut {local}"));
                            after.push_str(&format!(
                                "*{arg} = {}; ",
                                convert.replace("VALUE", &local)
                            ));
                        }
                    }
                }
                let call = format!("smelt_slot({})", passed.join(", "));
                let answer = if ret.1 {
                    convert.replace("VALUE", "smelt_answer")
                } else {
                    "smelt_answer".to_owned()
                };
                format!(
                    "::std::rc::Rc::new(move |{}| -> {} {{ {before}let smelt_answer = {call}; {after}{answer} }})",
                    declared_params.join(", "),
                    side(&ret.0)
                )
            };
            declared.push(format!("fn {view_read}(&self) -> {ty_text};"));
            declared.push(format!("fn {view_write}(&self, value: {ty_text});"));
            implemented.push(format!(
                "fn {view_read}(&self) -> {ty_text} {{ let smelt_slot = self.source.{read}().clone(); {} }}",
                adapter(true)
            ));
            implemented.push(format!(
                "fn {view_write}(&self, value: {ty_text}) {{ let smelt_slot = value; *self.source.{write}() = {}; }}",
                adapter(false)
            ));
            true
        } else {
            false
        };
        let (on_read, on_write) = if forwarded {
            (
                format!("return SmeltFieldRead::Owned(smelt_view.{view_read}());"),
                format!(
                    "let smelt_value = smelt_view.{view_read}(); return SmeltFieldWrite::converted(smelt_value, move |smelt_value| smelt_view.{view_write}(smelt_value));"
                ),
            )
        } else {
            (
                format!("let _ = smelt_view; {gap}"),
                format!("let _ = smelt_view; {gap}"),
            )
        };
        inherent.push_str(&format!(
            "    #[allow(dead_code)]\n    fn {read}(&self) -> SmeltFieldRead<'_, {ty_text}> {{ if let Some(smelt_view) = self.smelt_view() {{ {on_read} }} SmeltFieldRead::Cell(::std::cell::Ref::map(self.0.borrow(), |smelt_inner| &smelt_inner.{rust})) }}\n    #[allow(dead_code)]\n    fn {write}(&self) -> SmeltFieldWrite<'_, {ty_text}> {{ if let Some(smelt_view) = self.smelt_view() {{ {on_write} }} SmeltFieldWrite::Cell(::std::cell::RefMut::map(self.0.borrow_mut(), |smelt_inner| &mut smelt_inner.{rust})) }}\n"
        ));
    }
    let mut out = String::new();
    out.push_str(&format!(
        "/// A `{name}` seen at other type arguments than its cell was built with: the same object, converted at each access.\n#[allow(dead_code)]\ntrait {view_trait}{} {{ {} }}\n",
        static_params(type_args),
        declared.join(" ")
    ));
    out.push_str(&format!(
        "/// The view of a `{name}` built at other arguments.\n#[allow(dead_code)]\nstruct {view}{view_params} {{ source: {name}{source_args}, root: ::std::rc::Rc<dyn ::std::any::Any>, _smelt_phantom: ::std::marker::PhantomData<({},)> }}\n",
        idents.join(", ")
    ));
    out.push_str(&format!(
        "impl{view_generics} {view_trait}{type_args} for {view}{view_params} {{ {} }}\n",
        implemented.join(" ")
    ));
    out.push_str(&format!(
        "impl{impl_generics} {name}{type_args} {{\n\
         {inherent}\
         \x20   /// Whether a placeholder can exist at this instantiation: one the program names with erased arguments. A constant once monomorphized, so every other instantiation skips the slot test.\n\
         \x20   #[allow(dead_code)]\n\
         \x20   #[inline]\n\
         \x20   fn smelt_may_view() -> bool {{ let smelt_self = ::std::any::TypeId::of::<Self>(); let _ = smelt_self; {may_view} }}\n\
         \x20   /// The converting view this handle stands for, when it is a placeholder.\n\
         \x20   #[allow(dead_code)]\n\
         \x20   #[inline]\n\
         \x20   fn smelt_view(&self) -> {} {{ if !Self::smelt_may_view() {{ return None; }} self.0.borrow().{VIEW_SLOT_FIELD}.clone() }}\n\
         \x20   /// Whether this handle is a placeholder for an object built at other arguments.\n\
         \x20   #[allow(dead_code)]\n\
         \x20   #[inline]\n\
         \x20   fn smelt_is_view(&self) -> bool {{ Self::smelt_may_view() && self.0.borrow().{VIEW_SLOT_FIELD}.is_some() }}\n\
         \x20   /// The object's identity: the cell it was built in.\n\
         \x20   #[allow(dead_code)]\n\
         \x20   fn smelt_identity(&self) -> *const () {{ match self.smelt_view() {{ Some(smelt_view) => smelt_view.smelt_identity(), None => ::std::rc::Rc::as_ptr(&self.0).cast::<()>() }} }}\n\
         \x20   /// This handle, which must be the object's own cell (an upcast of a placeholder has no record to view).\n\
         \x20   #[allow(dead_code)]\n\
         \x20   fn smelt_real(&self) -> &Self {{ if self.smelt_is_view() {{ {gap} }} self }}\n\
         \x20   /// This object as a `{name}{type_args}`: the same handle when it is (or the placeholder roots at) one at these arguments, else a placeholder over a converting view of it.\n\
         \x20   #[allow(dead_code)]\n\
         \x20   fn smelt_retarget{source_generics}(source: {name}{source_args}) -> Self {{ let smelt_any: ::std::rc::Rc<dyn ::std::any::Any> = ::std::rc::Rc::new(source.clone()); if let Some(smelt_same) = smelt_any.downcast_ref::<Self>() {{ return smelt_same.clone(); }} let smelt_root = source.smelt_view().map_or(smelt_any, |smelt_view| smelt_view.smelt_root()); if let Some(smelt_found) = smelt_root.downcast_ref::<Self>() {{ return smelt_found.clone(); }} if !Self::smelt_may_view() {{ {gap} }} let mut smelt_inner = {name}Inner::{type_args}::default(); smelt_inner.{VIEW_SLOT_FIELD} = Some(::std::rc::Rc::new({view}::{view_params} {{ source, root: smelt_root, _smelt_phantom: ::std::marker::PhantomData }})); {name}(::std::rc::Rc::new(::std::cell::RefCell::new(smelt_inner))) }}\n\
         }}\n",
        view_slot_type(name, type_args)
    ));
    out
}

/// The instantiations of widened `class` a placeholder can exist at: every
/// concrete one the program names whose carried arguments mention the erased
/// type (`Parser<SmeltUnknown>`). A conversion into any other instantiation
/// finds the object's own cell or reports the gap; it never builds a view.
pub(crate) fn viewable_instantiations(
    mir: &Mir,
    context: &crate::emitter::EmitContext,
    class: Symbol,
) -> Result<Vec<String>, crate::EmitError> {
    let none = HashSet::new();
    let mut handles = Vec::new();
    for (index, ty) in mir.types.all().iter().enumerate() {
        let Type::Class { name, args } = ty else {
            continue;
        };
        if *name != class {
            continue;
        }
        let carried = context
            .type_param_elision()
            .retain_carried(*name, args.iter().collect());
        let mut params = HashSet::new();
        let mut seen = HashSet::new();
        let mut erased = false;
        for arg in &carried {
            crate::generic_bindings::collect_type_param_occurrences(mir, **arg, &mut params);
            erased |= mentions_unknown(mir, **arg, &mut seen);
        }
        if !erased || !params.is_empty() {
            continue;
        }
        let Ok(id) = u32::try_from(index) else {
            continue;
        };
        let handle = crate::emitter::FunctionEmitter::type_text_for_with_scoped_type_params(
            mir,
            context,
            TypeId(id),
            &crate::type_substitution::TypeSubstitution::lexical(&none),
        )?;
        if !renders_placeholder(&handle) && !handles.contains(&handle) {
            handles.push(handle);
        }
    }
    Ok(handles)
}

/// The bare parameter list `params` (`<A, B>`) with each parameter bounded by
/// `'static`; a list that already carries bounds is kept as it is.
pub(crate) fn static_params(params: &str) -> String {
    let inner = params.trim_start_matches('<').trim_end_matches('>');
    if inner.is_empty() || inner.contains(':') {
        return params.to_owned();
    }
    let bounded: Vec<String> = inner
        .split(',')
        .map(|param| format!("{}: 'static", param.trim()))
        .collect();
    format!("<{}>", bounded.join(", "))
}

/// The handle types widened `class`'s `SmeltFromUnknown` also takes back as
/// the same object, each with the expression retargeting the recovered
/// `smelt_subclass_origin`: every concrete instantiation of `class` or of a
/// subclass the program names (`Box<f64>`, `NumBox`). An erasure of a
/// `Box<f64>` read back as a `Box<unknown>` is the same box, not a copy.
///
/// `target` is the path of the handle type retargeted to (`Self` inside the
/// class's own impls).
pub(crate) fn instantiation_origins(
    mir: &Mir,
    context: &crate::emitter::EmitContext,
    class: &MirClass,
    target: &str,
) -> Result<Vec<(String, String)>, crate::EmitError> {
    let binder = crate::class_polymorphism::SUBCLASS_ORIGIN_BINDER;
    let none = HashSet::new();
    let mut seen = HashSet::new();
    let mut origins = Vec::new();
    for (index, ty) in mir.types.all().iter().enumerate() {
        let Type::Class { name, args } = ty else {
            continue;
        };
        let descends = *name == class.name
            || crate::class_polymorphism::polymorphic_ancestors(
                mir,
                context.polymorphic_classes(),
                *name,
            )
            .contains(&class.name);
        if !descends || !context.is_reference_class(*name) {
            continue;
        }
        let mut params = HashSet::new();
        for arg in args {
            crate::generic_bindings::collect_type_param_occurrences(mir, *arg, &mut params);
        }
        if !params.is_empty() {
            continue;
        }
        let Ok(id) = u32::try_from(index) else {
            continue;
        };
        let handle = crate::emitter::FunctionEmitter::type_text_for_with_scoped_type_params(
            mir,
            context,
            TypeId(id),
            &crate::type_substitution::TypeSubstitution::lexical(&none),
        )?;
        // An argument rendered as the inference placeholder names no type.
        if renders_placeholder(&handle) || !seen.insert(handle.clone()) {
            continue;
        }
        let viewed = if *name == class.name {
            binder.to_owned()
        } else {
            crate::class_polymorphism::upcast_handle_expr(context, *name, class.name, binder)?
        };
        origins.push((handle, format!("{target}::smelt_retarget({viewed})")));
    }
    Ok(origins)
}

/// Whether the rendered type `text` leaves an argument to inference (`_`):
/// it names no instantiation of its own, so it is neither an origin to take
/// back nor a different instantiation to retarget from.
pub(crate) fn renders_placeholder(text: &str) -> bool {
    text.split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
        .any(|word| word == "_")
}
