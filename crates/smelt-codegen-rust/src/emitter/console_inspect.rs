//! Which values `console.log` renders through `util.inspect`, and the
//! per-program half of that formatter (struct and union impls).
//!
//! The runtime half — the `SmeltInspect` trait, Node's layout routines and the
//! container impls — lives in [`crate::console_inspect_prelude`]. This module
//! decides, from the MIR type of a `console.log` argument, whether every Rust
//! type the value can hold implements that trait, and emits the impls for the
//! program's own record structs and union enums.
//!
//! Design adapted from Bun's `--aot` (oven-sh/bun#45631): compiled output is
//! judged against the JavaScript runtime running the same source
//! (`docs/bundler/aot-debugging.mdx`), which is how the Rust-`Debug` console
//! spelling this replaces was found.
//!
//! # The decision is a proof, not a guess
//!
//! A console argument is routed through the trait only when
//! [`FunctionEmitter::type_is_console_inspectable`] holds, and that predicate
//! mirrors the impl set exactly: a type is inspectable when its Rust storage
//! has an impl and every type it contains is inspectable. A record struct gets
//! an impl precisely when the same predicate holds for all its fields, so the
//! console site and the emitted impls cannot disagree — an argument the
//! predicate rejects keeps its previous rendering instead of producing Rust
//! that fails to compile (E0277).
//!
//! The predicate rejects: generic records and unions (their impls would need
//! bounds the generic-elision pass decides), functions, futures, generators,
//! type parameters, host classes (`Request`, `Error`, …) whose Node rendering
//! is not a field listing, a class whose base chain leaves the program, and a
//! string-keyed `Record` stored in a `HashMap` (its iteration order is not
//! JavaScript's insertion order, so no faithful rendering exists).
//!
//! Requirements: see the behavioral and type-preservation statement on
//! [`crate::console_inspect_prelude`].

use super::*;
use crate::console_inspect_prelude::{self, InspectContainers, InspectField};
use crate::rust::RustIdent;
use super::union::union_name;

/// A generated record struct, as the inspect impl needs to see it.
struct InspectRecord {
    /// The emitted Rust type name.
    rust_name: String,
    /// The source class name Node prints, or empty for an object shape.
    constructor: String,
    /// Every stored field, base fields first.
    fields: Vec<MirField>,
    /// Shared-cell storage (`struct Name(Rc<RefCell<NameInner>>)`).
    reference: bool,
    /// An object shape (interface / type literal) rather than a class.
    is_shape: bool,
    /// The record has no type parameters and its base chain stays in the
    /// program, so a field listing is its whole Node rendering.
    eligible: bool,
}

impl FunctionEmitter<'_> {
    /// Whether a `console.log` argument of type `ty` renders through the
    /// `SmeltInspect` trait.
    ///
    /// Only the JavaScript console takes this route (`absent` is the
    /// language's spelling of an absent value: Python's `print` formats with
    /// `repr`, a different rule). Scalars keep their direct formatting, which
    /// already matches Node and keeps the generated text free of a trait call
    /// for the common case; a `SmeltUnknown` keeps `Display`, because an
    /// operand typed `unknown` does not always emit the erased carrier (see
    /// `console_arg_text`).
    pub(super) fn console_inspects(&self, ty: TypeId, absent: AbsentSpelling) -> bool {
        if absent != AbsentSpelling::Undefined {
            return false;
        }
        let composite = match self.mir.types.get(ty) {
            Some(
                Type::List(_)
                | Type::Set(_)
                | Type::Dict(_, _)
                | Type::JsMap(_, _)
                | Type::Tuple(_)
                | Type::Class { .. }
                | Type::Union(_),
            ) => true,
            Some(Type::Optional(inner)) => {
                !matches!(
                    self.mir.types.get(self.flatten_optional_inner(*inner)),
                    Some(
                        Type::Bool
                            | Type::Int
                            | Type::Float
                            | Type::String
                            | Type::Unknown
                            | Type::None
                    )
                )
            }
            _ => false,
        };
        let inspectable = composite && self.type_is_console_inspectable(ty);
        if inspectable {
            self.context.mark_console_inspect_used();
        }
        inspectable
    }

    /// Whether every Rust type a value of `ty` can hold implements
    /// `SmeltInspect`. See the module docs for what is rejected and why.
    pub(super) fn type_is_console_inspectable(&self, ty: TypeId) -> bool {
        let records = inspect_records(self.mir, self.context);
        self.inspectable_in(ty, &records, &mut HashSet::new())
    }

    /// [`Self::type_is_console_inspectable`] with the record table built and a
    /// set of types already being proven (a recursive type is inspectable when
    /// the rest of its structure is: the trait impls recurse the same way).
    fn inspectable_in(
        &self,
        ty: TypeId,
        records: &HashMap<Symbol, InspectRecord>,
        visiting: &mut HashSet<TypeId>,
    ) -> bool {
        if !visiting.insert(ty) {
            return true;
        }
        let needs_unknown = crate::stdlib::needs_unknown_type(self.mir);
        let result = match self.mir.types.get(ty) {
            Some(Type::Bool | Type::Int | Type::Float | Type::String | Type::None) => true,
            Some(Type::Unknown) => needs_unknown,
            Some(Type::Optional(inner)) => {
                self.inspectable_in(self.flatten_optional_inner(*inner), records, visiting)
            }
            Some(Type::List(item)) => {
                crate::stdlib::needs_smelt_list(self.mir)
                    && self.inspectable_in(*item, records, visiting)
            }
            Some(Type::Set(item)) => {
                let container_emitted = if self.set_container_name(*item) == "SmeltPrimSet" {
                    crate::stdlib::needs_prim_set(self.mir)
                } else {
                    needs_unknown
                };
                container_emitted && self.inspectable_in(*item, records, visiting)
            }
            Some(Type::JsMap(key, value)) => {
                needs_unknown
                    && self.inspectable_in(*key, records, visiting)
                    && self.inspectable_in(*value, records, visiting)
            }
            // Only the `SmeltRecord` backing keeps JavaScript's own-key order;
            // a `HashMap` or a `SmeltJsMap`-backed record has no faithful
            // object rendering.
            Some(Type::Dict(key, value)) => {
                self.dict_uses_smelt_record(*key)
                    && self.inspectable_in(*value, records, visiting)
            }
            Some(Type::Tuple(items)) => {
                (1..=console_inspect_prelude::MAX_TUPLE_ARITY).contains(&items.len())
                    && items
                        .iter()
                        .all(|item| self.inspectable_in(*item, records, visiting))
            }
            Some(Type::Class { name, args }) => {
                args.is_empty()
                    && records.get(name).is_some_and(|record| {
                        record.eligible
                            && record
                                .fields
                                .iter()
                                .filter(|field| field.visibility.is_own_property())
                                .all(|field| self.inspectable_in(field.ty, records, visiting))
                    })
            }
            Some(Type::Union(_)) => self.union_is_inspectable(ty, records, visiting),
            _ => false,
        };
        visiting.remove(&ty);
        result
    }

    /// Whether a union has a non-generic generated enum whose every member is
    /// inspectable — the exact condition under which its impl is emitted.
    fn union_is_inspectable(
        &self,
        ty: TypeId,
        records: &HashMap<Symbol, InspectRecord>,
        visiting: &mut HashSet<TypeId>,
    ) -> bool {
        let Some(member_slice) = self.concrete_union_members(ty) else {
            return false;
        };
        let members = member_slice.to_vec();
        self.union_type_params(ty).is_ok_and(|params| params.is_empty())
            && members.iter().all(|member| {
                !matches!(self.mir.types.get(*member), Some(Type::Generator { .. }))
                    && self.inspectable_in(*member, records, visiting)
            })
    }
}

/// Every generated record struct, keyed by its MIR symbol, in the order the
/// crate emitter claims Rust names: interfaces first, then classes, a name
/// already taken skipping the later declaration (as `emit_source` does).
fn inspect_records(mir: &Mir, context: &EmitContext) -> HashMap<Symbol, InspectRecord> {
    let mut records = HashMap::new();
    let mut claimed = HashSet::new();
    for interface in &mir.interfaces {
        let Some(source) = mir.symbols.get(interface.name) else {
            continue;
        };
        let rust_name = RustIdent::new(source).into_string();
        if !claimed.insert(rust_name.clone()) || host_class_name(source) {
            continue;
        }
        records.insert(
            interface.name,
            InspectRecord {
                rust_name,
                constructor: String::new(),
                fields: crate::classes::effective_interface_fields(mir, interface),
                reference: context.is_reference_class(interface.name),
                is_shape: true,
                eligible: interface.type_params.is_empty(),
            },
        );
    }
    for class in &mir.classes {
        let Some(source) = mir.symbols.get(class.name) else {
            continue;
        };
        let rust_name = RustIdent::new(source).into_string();
        if !claimed.insert(rust_name.clone()) || host_class_name(source) {
            continue;
        }
        let constructor = mir.names.get(class.name).unwrap_or(source).to_owned();
        records.insert(
            class.name,
            InspectRecord {
                rust_name,
                constructor,
                fields: crate::classes::effective_class_fields(mir, class),
                reference: context.is_reference_class(class.name),
                is_shape: false,
                eligible: class.type_params.is_empty() && base_chain_is_local(mir, class),
            },
        );
    }
    records
}

/// Whether a source class name is a modeled host class with its own runtime
/// type (`Request`, `Blob`, …): such a class is not a generated record.
fn host_class_name(name: &str) -> bool {
    smelt_stdlib::typescript_stdlib_class(name)
        .is_some_and(smelt_stdlib::StdlibClass::has_concrete_runtime_type)
}

/// Whether every base of `class` is a class this program declares.
///
/// A class extending a host class (`extends Error`) is not rendered by Node as
/// a field listing — an error prints its stack — so it is not inspectable.
fn base_chain_is_local(mir: &Mir, class: &MirClass) -> bool {
    let mut current = class;
    // A base chain longer than the class list is a cycle, which no accepted
    // program has; bounding the walk keeps a malformed one from looping.
    for _ in 0..=mir.classes.len() {
        let Some(base) = current.base else {
            return true;
        };
        let Some(next) = mir.classes.iter().find(|candidate| candidate.name == base) else {
            return false;
        };
        current = next;
    }
    false
}

/// Emit the `SmeltInspect` runtime and every per-program impl, or nothing when
/// no `console.log` argument took the inspect route.
///
/// Called after every function body is emitted, so the context flag set by
/// [`FunctionEmitter::console_inspects`] is final.
pub(crate) fn emit_console_inspect_support(
    mir: &Mir,
    context: &EmitContext,
) -> Result<String, EmitError> {
    if !context.console_inspect_used() {
        return Ok(String::new());
    }
    let Some(function) = mir.functions.first() else {
        return Ok(String::new());
    };
    let emitter = FunctionEmitter::new(mir, context, function)?;
    let needs_unknown = crate::stdlib::needs_unknown_type(mir);
    let mut text = console_inspect_prelude::runtime_text(InspectContainers {
        list: crate::stdlib::needs_smelt_list(mir),
        prim_set: crate::stdlib::needs_prim_set(mir),
        unknown: needs_unknown,
    });
    let records = inspect_records(mir, context);
    let mut ordered = records.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.1.rust_name.cmp(&right.1.rust_name));
    for (symbol, record) in ordered {
        let Some(record_index) = mir.types.all().iter().position(|ty| {
            matches!(ty, Type::Class { name, args } if name == symbol && args.is_empty())
        }) else {
            continue;
        };
        let record_ty = TypeId(compact_index(record_index, "record type index does not fit u32")?);
        if !emitter.inspectable_in(record_ty, &records, &mut HashSet::new()) {
            continue;
        }
        let fields = record
            .fields
            .iter()
            .filter(|field| field.visibility.is_own_property())
            .map(|field| InspectField {
                key: mir
                    .names
                    .get(field.name)
                    .or_else(|| mir.symbols.get(field.name))
                    .unwrap_or("field")
                    .to_owned(),
                rust_name: RustIdent::new(mir.symbols.get(field.name).unwrap_or("field"))
                    .into_string(),
                skip_when_absent: record.is_shape
                    && matches!(mir.types.get(field.ty), Some(Type::Optional(_))),
            })
            .collect::<Vec<_>>();
        text.push('\n');
        text.push_str(&console_inspect_prelude::struct_impl_text(
            &record.rust_name,
            &record.constructor,
            &fields,
            record.reference,
        ));
    }
    for index in 0..mir.types.all().len() {
        let ty = TypeId(compact_index(index, "union type index does not fit u32")?);
        let Some(members) = emitter.concrete_union_members(ty) else {
            continue;
        };
        let member_count = members.len();
        if !emitter.union_is_inspectable(ty, &records, &mut HashSet::new()) {
            continue;
        }
        text.push('\n');
        text.push_str(&console_inspect_prelude::union_impl_text(
            &union_name(ty),
            member_count,
        ));
    }
    Ok(text)
}
