//! `structuredClone(value)` of a typed value: a deep copy at the value's own
//! Rust type.
//!
//! The copy goes through the runtime trait `SmeltStructuredClone`
//! (`smelt-runtime/src/runtime/structured_clone.rs`), which has an impl per
//! runtime container (`SmeltList`, `SmeltJsMap`, the sets, `SmeltRecord`,
//! `SmeltDate`, `SmeltRegExp`, typed arrays, the erased carrier) and, emitted
//! here per program, per generated record struct and concrete union enum. So
//! the copy is chosen by the value's real Rust type, as a hand port would
//! write a `DeepClone` trait, and never erases the value to copy it.
//!
//! # Requirements
//!
//! - Behaviour: the copy is a new graph. Every list, record, map and set in
//!   it is a fresh object, so writes to the copy never reach the source (they
//!   did: the frontend passed the value through, and
//!   `const c = structuredClone(item); c.tags.push("y")` grew `item.tags`). A
//!   reference that occurs twice in the source (or a cycle) occurs twice in
//!   the copy as one new object, by the memo the runtime threads through the
//!   copy. A value holding a function throws `DataCloneError`.
//! - Type preservation: the copy has the operand's type; no value crosses
//!   into `SmeltUnknown` unless it was erased already.
//!
//! # Scope
//!
//! A type the rule cannot copy faithfully is refused at the call with a
//! blocker naming it, rather than aliased: a generic record, a class with a
//! base or a subclass (their layout nests), a class with virtual method
//! slots, a structural-view target, and any type below one. A live record
//! (`emitter::live_view`) copies its declared fields: a property one of its
//! erased views added beyond them is not copied.
//! A class instance copies to an instance of its class, where the platform
//! answers a plain object without its prototype (the methods are then gone);
//! a program that clones a class instance and reads only its fields sees the
//! same values.

use std::collections::{HashMap, HashSet};

use crate::rust::RustExpr;
use smelt_hir::{Symbol, Type, TypeId};
use smelt_mir::{Mir, MirField, Operand};

use super::{EmitContext, EmitError, FunctionEmitter};

/// The most items a tuple the runtime copies has (`smelt_clone_tuple!`).
const MAX_TUPLE_ARITY: usize = 8;

/// A generated record struct the per-program impls copy.
pub(crate) struct CloneRecord {
    /// The Rust type name.
    rust_name: String,
    /// The data fields, copied one by one.
    fields: Vec<MirField>,
    /// Shared-cell storage (`struct Name(Rc<RefCell<NameInner>>)`).
    reference: bool,
}

/// The records whose struct layout the per-program impl can copy (module
/// scope), by symbol.
pub(crate) fn clone_records(mir: &Mir, context: &EmitContext) -> HashMap<Symbol, CloneRecord> {
    let mut records = HashMap::new();
    for decl in crate::names::type_claim_order(mir) {
        let symbol = decl.name();
        if context.structural_views().is_target(symbol) {
            continue;
        }
        let Ok(planned) = context.names().type_name(symbol) else {
            continue;
        };
        let (fields, eligible) = match decl {
            crate::names::TypeDecl::Interface(interface) => (
                crate::classes::effective_interface_fields(mir, interface),
                interface.type_params.is_empty(),
            ),
            crate::names::TypeDecl::Class(class) => (
                crate::classes::effective_class_fields(mir, class),
                class.type_params.is_empty()
                    && class.base.is_none()
                    && !mir.classes.iter().any(|other| other.base == Some(symbol))
                    && crate::classes::own_virtual_slot_names(mir, class).is_empty()
                    && !mir.symbols.get(symbol).is_some_and(|source| {
                        smelt_stdlib::typescript_stdlib_class(source)
                            .is_some_and(smelt_stdlib::StdlibClass::has_concrete_runtime_type)
                    }),
            ),
        };
        if !eligible {
            continue;
        }
        records.insert(
            symbol,
            CloneRecord {
                rust_name: planned.as_str().to_owned(),
                fields,
                reference: context.is_reference_class(symbol),
            },
        );
    }
    records
}

/// The per-program `SmeltStructuredClone` impls: one per copyable record
/// struct and concrete union enum. Empty unless a typed `structuredClone`
/// is in the program.
pub(crate) fn emit_structured_clone_support(
    mir: &Mir,
    context: &EmitContext,
) -> Result<String, EmitError> {
    if !context
        .demand()
        .has(crate::runtime_demand::Need::StructuredCloneTyped)
    {
        return Ok(String::new());
    }
    let Some(function) = mir.functions.first() else {
        return Ok(String::new());
    };
    let emitter = FunctionEmitter::new(mir, context, function)?;
    let records = clone_records(mir, context);
    let mut ordered: Vec<(&Symbol, &CloneRecord)> = records.iter().collect();
    ordered.sort_by(|left, right| left.1.rust_name.cmp(&right.1.rust_name));
    let mut text = String::new();
    for (symbol, record) in ordered {
        let ty = emitter.type_id(Type::Class {
            name: *symbol,
            args: Vec::new(),
        })?;
        if !emitter.structured_cloneable_in(ty, &records, &mut HashSet::new())? {
            continue;
        }
        text.push('\n');
        text.push_str(&record_impl_text(mir, context, record));
    }
    for index in 0..mir.types.all().len() {
        let ty = TypeId(crate::error::compact_index(index, "union type index does not fit u32")?);
        let Some(members) = emitter.concrete_union_members(ty) else {
            continue;
        };
        let arity = members.len();
        if !emitter.structured_cloneable_in(ty, &records, &mut HashSet::new())? {
            continue;
        }
        text.push('\n');
        text.push_str(&union_impl_text(&super::union::union_name(ty), arity));
    }
    Ok(text)
}

/// `impl SmeltStructuredClone` for one record struct.
///
/// A value struct is copied whole (`Clone`) and each data field replaced by
/// its deep copy. A shared cell is a new cell holding its `Default`, recorded
/// in the memo BEFORE its fields are copied (a cycle back to the source finds
/// the copy), then filled field by field.
fn record_impl_text(mir: &Mir, context: &EmitContext, record: &CloneRecord) -> String {
    let name = &record.rust_name;
    let mut text = format!(
        "impl SmeltStructuredClone for {name} {{\n    fn smelt_clone_in(&self, memo: &mut SmeltCloneMemo) -> Result<Self, SmeltDataCloneError> {{\n"
    );
    let field_names: Vec<String> = record
        .fields
        .iter()
        .map(|field| context.names().field(mir, field.name).into_string())
        .collect();
    if record.reference {
        text.push_str("        let key = ::std::rc::Rc::as_ptr(&self.0) as *const () as usize;\n");
        text.push_str("        if let Some(copy) = memo.get::<Self>(key) { return Ok(copy); }\n");
        text.push_str(&format!(
            "        let copy = {name}(::std::rc::Rc::new(::std::cell::RefCell::new({name}Inner::default())));\n"
        ));
        text.push_str("        memo.insert(key, &copy);\n");
        for field in &field_names {
            text.push_str(&format!(
                "        let smelt_field = self.0.borrow().{field}.smelt_clone_in(memo)?; copy.0.borrow_mut().{field} = smelt_field;\n"
            ));
        }
    } else {
        text.push_str("        let mut copy = self.clone();\n");
        for field in &field_names {
            text.push_str(&format!(
                "        copy.{field} = self.{field}.smelt_clone_in(memo)?;\n"
            ));
        }
    }
    text.push_str("        Ok(copy)\n    }\n}\n");
    text
}

/// `impl SmeltStructuredClone` for one concrete union enum: each member is
/// copied as itself.
fn union_impl_text(name: &str, arity: usize) -> String {
    let arms: String = (0..arity)
        .map(|index| {
            format!("            Self::M{index}(value) => Self::M{index}(value.smelt_clone_in(memo)?),\n")
        })
        .collect();
    format!(
        "impl SmeltStructuredClone for {name} {{\n    fn smelt_clone_in(&self, memo: &mut SmeltCloneMemo) -> Result<Self, SmeltDataCloneError> {{\n        Ok(match self {{\n{arms}        }})\n    }}\n}}\n"
    )
}

impl FunctionEmitter<'_> {
    /// `structuredClone(operand)` of a typed value. `can_throw` selects the
    /// fallible form, whose `Result` the caller propagates; otherwise the
    /// value holds no function and the copy cannot fail.
    pub(super) fn typed_structured_clone_expr(
        &self,
        operand: &Operand,
        can_throw: bool,
    ) -> Result<RustExpr, EmitError> {
        let ty = self.operand_ty(operand)?;
        let records = clone_records(self.mir, self.context);
        if !self.structured_cloneable_in(ty, &records, &mut HashSet::new())? {
            return Err(EmitError::new(format!(
                "structuredClone of a `{}` value is not modelled",
                self.blocker_type_text(ty)
            )));
        }
        let value = self.operand_expr(operand)?;
        let call = format!(
            "smelt_structured_clone_value(&{})",
            value.delimited()
        );
        if can_throw {
            return Ok(RustExpr::chain(format!(
                "{call}.map_err(smelt_data_clone_error)"
            )));
        }
        Ok(RustExpr::atom(format!(
            "match {call} {{ Ok(smelt_copy) => smelt_copy, Err(_) => unreachable!(\"a value that holds no function always clones\") }}"
        )))
    }

    /// Whether a value of type `ty` has a `SmeltStructuredClone` impl: a
    /// runtime container, or a record of `records` (or a concrete union)
    /// whose every field (member) has one. `visiting` breaks the recursion of
    /// a record that reaches itself.
    fn structured_cloneable_in(
        &self,
        ty: TypeId,
        records: &HashMap<Symbol, CloneRecord>,
        visiting: &mut HashSet<TypeId>,
    ) -> Result<bool, EmitError> {
        let Some(found) = self.mir.types.get(ty) else {
            return Ok(false);
        };
        let rendered = || -> Result<String, EmitError> {
            Ok(self.lexical_rust_type(ty, false)?.to_string())
        };
        Ok(match found {
            Type::Float | Type::Int | Type::Bool | Type::String | Type::None => true,
            // An erased value is copied by the erased helper, which the
            // program carries when it clones an erased value itself.
            Type::Unknown => self
                .context
                .demand()
                .has(crate::runtime_demand::Need::StructuredClone),
            Type::Optional(inner, _) => self.structured_cloneable_in(*inner, records, visiting)?,
            Type::List(item) => self.structured_cloneable_in(*item, records, visiting)?,
            Type::Tuple(items) => {
                (1..=MAX_TUPLE_ARITY).contains(&items.len())
                    && self.all_cloneable(items, records, visiting)?
            }
            Type::Dict(key, value) | Type::JsMap(key, value) => {
                let text = rendered()?;
                (text.starts_with("SmeltRecord<") || text.starts_with("SmeltJsMap<"))
                    && self.structured_cloneable_in(*key, records, visiting)?
                    && self.structured_cloneable_in(*value, records, visiting)?
            }
            Type::Set(item) => {
                let text = rendered()?;
                (text.starts_with("SmeltJsSet<") || text.starts_with("SmeltPrimSet<"))
                    && self.structured_cloneable_in(*item, records, visiting)?
            }
            Type::Function(_) => rendered()?.starts_with("::std::rc::Rc<"),
            Type::Union(members) if self.concrete_union_members(ty).is_some() => {
                self.all_cloneable(members, records, visiting)?
            }
            Type::Class { name, args } => {
                let text = rendered()?;
                if matches!(text.as_str(), "SmeltDate" | "SmeltRegExp" | "SmeltTypedArray") {
                    return Ok(true);
                }
                let Some(record) = records.get(name).filter(|_| args.is_empty()) else {
                    return Ok(false);
                };
                if !visiting.insert(ty) {
                    return Ok(true);
                }
                let field_tys: Vec<TypeId> = record.fields.iter().map(|field| field.ty).collect();
                self.all_cloneable(&field_tys, records, visiting)?
            }
            _ => false,
        })
    }

    /// Whether every type of `tys` is cloneable (see
    /// [`Self::structured_cloneable_in`]).
    fn all_cloneable(
        &self,
        tys: &[TypeId],
        records: &HashMap<Symbol, CloneRecord>,
        visiting: &mut HashSet<TypeId>,
    ) -> Result<bool, EmitError> {
        for ty in tys {
            if !self.structured_cloneable_in(*ty, records, visiting)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
