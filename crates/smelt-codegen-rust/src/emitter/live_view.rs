//! Live records: a reference record whose map-shaped views read and write it.
//!
//! # The defect this closes
//!
//! A record that crosses into a slot of another type — `any`, `unknown`, an
//! erased type parameter, `Record<string, V>` — was handed over as a snapshot
//! of its fields. JavaScript hands over the object. With
//! `const a: Point = { x: 1, label: "a" }`:
//!
//! ```text
//! function bump(p: any) { p.x = 200 }      bump(a); a.x            // 200, was 1
//! function keep(p: unknown) { held = p }   a.x = 5; (held as any).x // 5, was 1
//! function tag(p: any) { p.extra = true }  tag(a); Object.keys(a)   // x,label,extra
//! ```
//!
//! # The rule
//!
//! A *live record* is a non-generic object shape (an `interface` or an object
//! type literal) emitted as a reference record. It gets an
//! `impl SmeltLiveObject` ([`FunctionEmitter::live_object_impl_text`]), and
//! every erasure of it is `SmeltObject::smelt_live_view`: an erased object
//! whose store is linked to the record's own cell
//! (`smelt-runtime/src/runtime/live_view.rs`). The record keeps its concrete
//! Rust type and stays the single source of truth; typed code reads and writes
//! its fields exactly as before.
//!
//! # Requirements
//!
//! Behavior, for every live record `r` and every erased or map view `v` of it:
//!
//! - a write of a declared property through `v` is read back from `r`, and a
//!   write to `r` is read back through `v`, however long `v` was held;
//! - a write through `v` of a value the property's declared type cannot hold
//!   is read back through every view as written (JavaScript does not check a
//!   write); `r`'s typed field keeps its last typed value, since no typed
//!   read of it is valid TypeScript any more;
//! - `v` narrowed back to the record type is `r` (`===`), and two views of `r`
//!   are `===`;
//! - a property `v` adds that the record type does not declare, a declared
//!   property `v` deletes, and the resulting own-key order are properties of
//!   the object: every later view of `r` enumerates them as JavaScript does.
//!
//! Type preservation: the record's Rust type, its fields' types and every
//! typed access are unchanged. The erased carrier appears only inside the
//! `SmeltLiveObject` impl, which is the typed-to-erased crossing itself.
//!
//! # What is not a live record
//!
//! - A **class** keeps its snapshot view with the write-through hook of
//!   `emitter::view_write_through`: its erased view also carries prototype
//!   members, which the record's fields do not describe.
//! - A **callable object** (a shape with a call signature) crosses an erased
//!   slot as a function value carrying its properties
//!   (`emitter::callable_object`), not as an object view.
//! - A shape with a **getter-backed property** keeps its by-value struct,
//!   the only representation that stores an accessor.
//! - A **generic** record keeps the snapshot view: its impl would have to
//!   erase fields of the declaration's type parameters, which only a use site
//!   can render.
//! - A **value** record (never written, compared or shared) has no identity to
//!   share and stays a by-value struct; `classify` lifts it when one of its
//!   values reaches a slot that could observe one.

use smelt_hir::{Symbol, Type, TypeId};
use smelt_mir::Mir;

use super::{EmitContext, EmitError, FunctionEmitter, helper_reservation};
use crate::rust::RustExpr;

/// The live records of a program (see the module docs): every non-generic
/// object shape among `reference_classes`, in a program that carries the
/// erased runtime and JavaScript strings its views are written in.
pub(crate) fn live_records(
    mir: &Mir,
    reference_classes: &std::collections::HashSet<Symbol>,
    demand: &crate::runtime_demand::RuntimeDemand,
) -> std::collections::HashSet<Symbol> {
    use crate::runtime_demand::Need;
    if !demand.has(Need::Unknown) || !demand.has(Need::JsString) || mir.functions.is_empty() {
        return std::collections::HashSet::new();
    }
    mir.interfaces
        .iter()
        .filter(|interface| {
            interface.type_params.is_empty()
                && reference_classes.contains(&interface.name)
                && crate::classes::find_class(mir, interface.name).is_none()
                && !crate::classify::is_callable_shape(mir, interface)
                && !crate::classify::has_accessor_fields(mir, interface)
        })
        .map(|interface| interface.name)
        .collect()
}

/// Whether the record named `name` is a live record (see the module docs).
pub(crate) fn is_live_record(_mir: &Mir, context: &EmitContext, name: Symbol) -> bool {
    context.is_live_record(name)
}

/// A predicate over the erased `value` that holds exactly when it is of the
/// kind a field of type `ty` stores, or `None` when every erased value is
/// taken (an erased, generic or union field, and the types whose erased form
/// this does not pin down).
///
/// JavaScript does not check a write, so an `any` view can store an object
/// where the record type says `string`. The typed field cannot hold that
/// value, and converting it would store something nobody wrote
/// (`"[object Object]"`): a live record's setter refuses the write instead,
/// and the runtime keeps the value on the object (`SmeltOpenShape::extras`).
fn erased_value_fits(mir: &Mir, ty: TypeId) -> Option<String> {
    let tag = |variant: &str| Some(format!("matches!(value, SmeltUnknown::{variant}(_))"));
    match mir.types.get(ty)? {
        Type::Float | Type::Int | Type::FixedInt(_) => tag("Number"),
        Type::String => tag("String"),
        Type::Bool => tag("Bool"),
        Type::BigInt => tag("BigInt"),
        Type::List(_) | Type::Tuple(_) => tag("Array"),
        Type::Dict(_, _) => tag("Object"),
        Type::Function(_) => tag("Function"),
        Type::Class { name, .. } => {
            // A callable object crosses as a function value.
            let callable = mir.interfaces.iter().any(|interface| {
                interface.name == *name && crate::classify::is_callable_shape(mir, interface)
            });
            (!callable).then(|| "matches!(value, SmeltUnknown::Object(_))".to_owned())
        }
        Type::Optional(inner) => erased_value_fits(mir, *inner).map(|fits| {
            format!("(matches!(value, SmeltUnknown::Null | SmeltUnknown::Undefined) || {fits})")
        }),
        _ => None,
    }
}

/// A field statement of the snapshot view, with its entry keyed by the
/// property name's literal instead of an owned copy of it: the live impl's
/// sinks take `(&'static str, SmeltUnknown)`, so a read allocates no key.
fn static_key_entry(key: &str, statement: &str) -> String {
    statement.replace(
        &format!("smelt_object_entries.push(({key:?}.to_owned(), "),
        &format!("smelt_object_entries.push(({key:?}, "),
    )
}

impl FunctionEmitter<'_> {
    /// Whether `ty` is a live record type.
    pub(in crate::emitter) fn is_live_record_type(&self, ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(ty),
            Some(Type::Class { name, .. }) if is_live_record(self.mir, self.context, *name)
        )
    }

    /// The field statements of [`Self::record_view_field_statements`] built
    /// from each field's type alone (`record_field_unknown_expr`), which
    /// cannot fail: the erasure a reference record's generated
    /// `IntoSmeltUnknown` impl used before it had a live view.
    fn type_directed_field_statements(
        &self,
        name: Symbol,
        target: TypeId,
    ) -> Result<Vec<(String, String)>, EmitError> {
        self.class_or_interface_fields(name)
            .iter()
            .filter(|field| field.visibility.is_own_property())
            .map(|field| {
                let key = self.symbol_source_name(field.name)?.to_owned();
                let value = super::record_coercion::record_field_unknown_expr(
                    self.mir,
                    &RustExpr::chain(self.reference_field_read_place(
                        "smelt_object_value",
                        target,
                        field.name,
                    )?)
                    .cloned_value(),
                    field.ty,
                )
                .unwrap_or_else(|_| RustExpr::atom("SmeltUnknown::Null"));
                let statement = format!(
                    "smelt_object_entries.push(({key:?}.to_owned(), {value}));",
                    value = value.delimited()
                );
                Ok((key, statement))
            })
            .collect()
    }

    /// Render `impl SmeltLiveObject` for the live record `name`, whose Rust
    /// type is `rust_name`.
    ///
    /// Reads reuse the per-field erase statements of the snapshot view
    /// ([`Self::record_view_field_statements`]), so a live read and a snapshot
    /// agree on every field, an absent optional included. A write of a value
    /// of the field's kind ([`erased_value_fits`]) converts it to the field's
    /// declared type with the same checked conversion a typed assignment from
    /// an erased value uses. Any other value, and a field whose type has no
    /// such conversion, is handed back: the runtime keeps it on the object,
    /// where every view reads it.
    ///
    /// `self` is an emitter for any function of the program: a live record is
    /// non-generic, so its field types need no type parameter in scope.
    pub(crate) fn live_object_impl_text(
        &self,
        name: Symbol,
        rust_name: &str,
    ) -> Result<String, EmitError> {
        let target = self.type_id(Type::Class {
            name,
            args: Vec::new(),
        })?;
        let unknown_ty = self.type_id(Type::Unknown)?;
        let entry =
            helper_reservation::push_entry(&self.conversions.record_conversion_stack, target);
        let rendered = self.record_view_field_statements(target, name);
        drop(entry);
        // A field the emitter cannot erase in this scope must not sink the
        // record: it falls back to the type-directed erasure the record's
        // `IntoSmeltUnknown` impl has always used, field by field.
        let statements = match rendered {
            Ok(statements) => statements,
            Err(_) => self.type_directed_field_statements(name, target)?,
        };
        let names = statements
            .iter()
            .map(|(key, _)| format!("{key:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        // A single-key read runs that key's statement into a one-value slot.
        let get_arms = statements
            .iter()
            .map(|(key, statement)| {
                format!(
                    "if *key == {key:?} {{ {} }}",
                    static_key_entry(key, statement)
                )
            })
            .collect::<Vec<_>>()
            .join(" else ");
        let all = statements
            .iter()
            .map(|(key, statement)| static_key_entry(key, statement))
            .collect::<Vec<_>>()
            .join(" ");
        let mut set_arms = String::new();
        for field in self.class_or_interface_fields(name) {
            if !field.visibility.is_own_property() {
                continue;
            }
            let Ok(converted) = self.rendered_at_type(
                &RustExpr::path("value"),
                unknown_ty,
                field.ty,
                &self.render_scope(),
            ) else {
                continue;
            };
            let key = self.symbol_source_name(field.name)?;
            let place = self.reference_field_write_place("self", target, field.name)?;
            // A value of another type than the field's is handed back, not
            // coerced: the runtime keeps it on the object, shadowing the field.
            let guard = erased_value_fits(self.mir, field.ty)
                .map(|fits| format!("if !{fits} {{ return Err(value); }} "))
                .unwrap_or_default();
            set_arms.push_str(&format!(
                "        if *key == {key:?} {{ {guard}let smelt_field_value = {converted}; {place} = smelt_field_value; return Ok(()); }}\n",
                converted = converted.delimited()
            ));
        }
        // The cell the object is: a view target's handle holds it as its
        // trait object (`crate::structural_views`).
        let live_cell = if self.is_view_target_type(target) {
            "self.0.clone().smelt_any()"
        } else {
            "self.0.clone()"
        };
        // The whole impl is the typed-to-erased crossing of this record, which
        // `smelt-unknown-report` classifies by the `impl SmeltLiveObject for`
        // marker.
        Ok(self.carrier_format(
            self.carrier()?,
            format_args!(
                "/// `{rust_name}` as its erased and map views read and write it (`SmeltLiveObject`).\n\
                 impl SmeltLiveObject for {rust_name} {{\n\
                 \x20   fn smelt_field_names(&self) -> &'static [&'static str] {{ &[{names}] }}\n\
                 \x20   #[allow(unused_mut, unused_variables)]\n\
                 \x20   fn smelt_field_get(&self, key: &JsStr) -> Option<SmeltUnknown> {{\n\
                 \x20       let smelt_object_value = self;\n\
                 \x20       let smelt_struct_value = self;\n\
                 \x20       let mut smelt_object_entries = SmeltLiveSlot(None);\n\
                 \x20       {get_arms}\n\
                 \x20       smelt_object_entries.0\n\
                 \x20   }}\n\
                 \x20   #[allow(unused_variables)]\n\
                 \x20   fn smelt_field_set(&self, key: &JsStr, value: SmeltUnknown) -> Result<(), SmeltUnknown> {{\n\
                 {set_arms}\
                 \x20       Err(value)\n\
                 \x20   }}\n\
                 \x20   #[allow(unused_mut, unused_variables)]\n\
                 \x20   fn smelt_field_entries(&self) -> Vec<(&'static str, SmeltUnknown)> {{\n\
                 \x20       let smelt_object_value = self;\n\
                 \x20       let smelt_struct_value = self;\n\
                 \x20       let mut smelt_object_entries = Vec::<(&'static str, SmeltUnknown)>::new();\n\
                 \x20       {all}\n\
                 \x20       smelt_object_entries\n\
                 \x20   }}\n\
                 \x20   fn smelt_live_cell(&self) -> ::std::rc::Rc<dyn ::std::any::Any> {{ {live_cell} }}\n\
                 \x20   fn smelt_live_address(&self) -> usize {{ ::std::rc::Rc::as_ptr(&self.0) as *const () as usize }}\n\
                 \x20   fn smelt_live_handle(&self) -> &dyn ::std::any::Any {{ self }}\n\
                 }}\n"
            ),
        ))
    }
}
