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
//! type parameters kept as Rust generics, host classes (`Request`, `Error`, …) whose Node rendering
//! is not a field listing, a class whose base chain leaves the program, and a
//! string-keyed `Record` stored in a `HashMap` (its iteration order is not
//! JavaScript's insertion order, so no faithful rendering exists).
//!
//! Requirements: see the behavioral and type-preservation statement on
//! [`crate::console_inspect_prelude`].

use super::*;
use crate::console_inspect_prelude::{self, InspectField};
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
    /// `console_arg`).
    fn console_inspects(&self, ty: TypeId, absent: AbsentSpelling) -> bool {
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
                | Type::Unknown
                | Type::Union(_),
            ) => true,
            Some(Type::Optional(inner)) => {
                !matches!(
                    self.mir.types.get(self.flatten_optional_inner(*inner)),
                    Some(
                        Type::Bool
                            | Type::Int
                            | Type::Float
                            | Type::String | Type::PyStr
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
    fn type_is_console_inspectable(&self, ty: TypeId) -> bool {
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
        let needs_unknown = self.context.demand().has(Need::Unknown);
        let result = match self.mir.types.get(ty) {
            Some(Type::Bool | Type::Int | Type::Float | Type::BigInt | Type::String | Type::PyStr | Type::None) => {
                true
            }
            Some(Type::Unknown) => needs_unknown,
            // A type parameter this body renders as the erased carrier holds what
            // the carrier holds; one kept as a Rust generic has no `SmeltInspect`.
            Some(Type::TypeParam { .. }) => {
                needs_unknown && self.renders_erased(ty).unwrap_or(false)
            }
            Some(Type::Optional(inner)) => {
                self.inspectable_in(self.flatten_optional_inner(*inner), records, visiting)
            }
            Some(Type::List(item)) => {
                self.context.demand().has(Need::SmeltList)
                    && self.inspectable_in(*item, records, visiting)
            }
            Some(Type::Set(item)) => {
                let container_emitted = if self.set_container_name(*item) == "SmeltPrimSet" {
                    self.context.demand().has(Need::PrimSet)
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

/// Every generated record struct, keyed by its MIR symbol, read off the
/// crate's one type claim order ([`crate::names::type_claim_order`]) and named
/// by its plan, so the inspector names exactly the struct `emit_source` wrote.
fn inspect_records(mir: &Mir, context: &EmitContext) -> HashMap<Symbol, InspectRecord> {
    let mut records = HashMap::new();
    for decl in crate::names::type_claim_order(mir) {
        let symbol = decl.name();
        let Some(source) = mir.symbols.get(symbol) else {
            continue;
        };
        if host_class_name(source) {
            continue;
        }
        let Ok(planned) = context.names().type_name(symbol) else {
            continue;
        };
        let rust_name = planned.as_str().to_owned();
        let record = match decl {
            crate::names::TypeDecl::Interface(interface) => InspectRecord {
                rust_name,
                constructor: String::new(),
                fields: crate::classes::effective_interface_fields(mir, interface),
                reference: context.is_reference_class(symbol),
                is_shape: true,
                eligible: interface.type_params.is_empty(),
            },
            crate::names::TypeDecl::Class(class) => InspectRecord {
                rust_name,
                constructor: mir.names.get(symbol).unwrap_or(source).to_owned(),
                fields: crate::classes::effective_class_fields(mir, class),
                reference: context.is_reference_class(symbol),
                is_shape: false,
                eligible: class.type_params.is_empty() && base_chain_is_local(mir, class),
            },
        };
        records.insert(symbol, record);
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
        let Some(next) = crate::classes::find_class(mir, base) else {
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
    let mut writer = crate::rust::CodeWriter::new();
    crate::runtime_prelude::emit_entries(&mut writer, smelt_runtime::order::CONSOLE_INSPECT, mir, context)?;
    let mut text = writer.finish();
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
                rust_name: context.names().field(mir, field.name).into_string(),
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

impl FunctionEmitter<'_> {
    /// Whether a value of type `ty` has a `SmeltPyRepr` impl
    /// (`runtime/py_repr.rs`): Python's scalars, `None`, and lists, tuples,
    /// hash-map dicts and primitive sets of such values.
    pub(super) fn type_has_py_repr(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Int | Type::Float | Type::Bool | Type::String | Type::PyStr | Type::None) => true,
            Some(Type::Optional(inner) | Type::List(inner)) => self.type_has_py_repr(*inner),
            Some(Type::Tuple(items)) => !items.is_empty() && items.len() <= 6 && items.iter().all(|item| self.type_has_py_repr(*item)),
            Some(Type::Dict(key, value)) => {
                !self.dict_uses_js_key_map(*key)
                    && self.type_has_py_repr(*key)
                    && self.type_has_py_repr(*value)
            }
            Some(Type::Set(item)) => self.set_container_name(*item) == "SmeltPrimSet" && self.type_has_py_repr(*item),
            _ => self.is_py_exception_type(ty) || self.is_py_exception_args_type(ty),
        }
    }

    /// Converts an operand to console.log argument format and returns format string and value.
    /// Python's `repr` of the value `reference` borrows, as a `String`, for a type
    /// built of primitives, lists, tuples and options; `None` for any other type.
    fn python_repr_expr(&self, reference: &RustExpr, ty: TypeId) -> Option<RustExpr> {
        let item = RustExpr::path("smelt_item");
        Some(match self.mir.types.get(ty)? {
            Type::String | Type::PyStr => RustExpr::chain(format!("smelt_py_str_repr({reference})", reference = reference.delimited())),
            Type::Int => RustExpr::chain(format!("({reference}).to_string()", reference = reference.delimited())),
            Type::Float => RustExpr::chain(format!(
                "{}({reference})",
                smelt_stdlib::runtime_symbols::number_text::PY_FLOAT_REPR_FN, reference = reference.delimited()
            )),
            Type::Bool => RustExpr::atom(format!(
                "(if *{reference} {{ \"True\" }} else {{ \"False\" }}).to_owned()", reference = reference.tight()
            )),
            Type::Optional(inner) => {
                let inner_repr = self.python_repr_expr(&item, *inner)?;
                RustExpr::atom(format!(
                    "match {reference} {{ Some(smelt_item) => {inner_repr}, None => \"None\".to_owned() }}", reference = reference.delimited(), inner_repr = inner_repr.delimited()
                ))
            }
            Type::List(inner) => {
                let inner_repr = self.python_repr_expr(&item, *inner)?;
                RustExpr::chain(format!(
                    "format!(\"[{{}}]\", ({reference}).borrow().iter().map(|smelt_item| {inner_repr}).collect::<Vec<String>>().join(\", \"))", reference = reference.delimited(), inner_repr = inner_repr.delimited()
                ))
            }
            Type::Tuple(members) => {
                let parts = members
                    .iter()
                    .enumerate()
                    .map(|(index, member)| {
                        self.python_repr_expr(&RustExpr::atom(format!("&({reference}).{index}", reference = reference.delimited())), *member)
                            .map(RustExpr::into_string)
                    })
                    .collect::<Option<Vec<_>>>()?;
                let trailing = if parts.len() == 1 { "," } else { "" };
                RustExpr::chain(format!(
                    "format!(\"({{}}{trailing})\", [{}].join(\", \"))",
                    parts.join(", ")
                ))
            }
            _ => return None,
        })
    }

    pub(super) fn console_arg(
        &self,
        operand: &Operand,
        absent: AbsentSpelling,
    ) -> Result<(&'static str, String), EmitError> {
        let ty = self.operand_ty(operand)?;
        if ty == self.none_ty {
            // The language's own word for the null value: `console.log(null)`
            // is `null`, Python's `print(None)` is `None`.
            Ok(("{}", format!("{:?}", absent.null_text())))
        } else if absent == AbsentSpelling::None
            && matches!(self.mir.types.get(ty), Some(Type::Bool))
        {
            // Python's `print` writes `str(value)`, and `str(True)` is `True`;
            // Rust's `Display` (and JavaScript) write `true`. `AbsentSpelling`
            // is the console's language: `None` is Python's `print`.
            Ok((
                "{}",
                format!(
                    "if {} {{ \"True\" }} else {{ \"False\" }}",
                    self.operand_expr(operand)?.delimited()
                ),
            ))
        } else if self.is_py_exception_type(ty) || self.is_py_exception_args_type(ty) {
            // `print(e)` writes `str(e)`, the exception's `Display`.
            Ok(("{}", self.operand_expr(operand)?.into_string()))
        } else if absent == AbsentSpelling::None
            && matches!(
                self.mir.types.get(ty),
                Some(Type::List(_) | Type::Tuple(_) | Type::Dict(_, _) | Type::Set(_))
            )
            && self.type_has_py_repr(ty)
        {
            // Python's `print` of a container writes `str(container)`, built
            // from each element's `repr` (`['a', True, None]`), not Rust's
            // `Debug` (`["a", true, ()]`).
            Ok((
                "{}",
                format!("SmeltPyRepr::py_repr(&{})", self.operand_expr(operand)?.tight()),
            ))
        } else if self.console_inspects(ty, absent) {
            // A container, record or union prints the way Node's
            // `util.inspect` draws it (`[ 1, 2 ]`, `{ x: 1 }`, `Map(1) { .. }`),
            // through the trait impl of the value's own Rust type; see
            // `console_inspect_prelude`.
            Ok((
                "{}",
                format!(
                    "{}::{}(&{})",
                    console_inspect_prelude::INSPECT_TRAIT,
                    console_inspect_prelude::CONSOLE_TEXT_METHOD,
                    self.operand_expr(operand)?.tight()
                ),
            ))
        } else if absent == AbsentSpelling::Undefined
            && matches!(self.mir.types.get(ty), Some(Type::GeneratorResult { .. }))
        {
            // An iterator result is the `{ value, done }` object, and the erased
            // carrier is what draws an object.
            self.context.mark_console_inspect_used();
            Ok((
                "{}",
                format!(
                    "{}::{}(&{})",
                    console_inspect_prelude::INSPECT_TRAIT,
                    console_inspect_prelude::CONSOLE_TEXT_METHOD,
                    self.erase(operand)?.tight()
                ),
            ))
        } else if absent == AbsentSpelling::Undefined
            && (matches!(self.mir.types.get(ty), Some(Type::TypeParam { .. }))
                || matches!(
                    self.mir.types.get(ty),
                    Some(Type::List(_) | Type::Dict(_, _) | Type::Tuple(_))
                ) && self.type_mentions_any(ty, &self.current_function_type_params()))
        {
            // A container of a Rust generic has no `SmeltInspect` impl: its
            // elements print through the erased carrier their bound provides.
            self.context.mark_console_inspect_used();
            Ok((
                "{}",
                format!(
                    "{}::{}(&{})",
                    console_inspect_prelude::INSPECT_TRAIT,
                    console_inspect_prelude::CONSOLE_TEXT_METHOD,
                    self.erase(operand)?.tight()
                ),
            ))
        } else if absent == AbsentSpelling::None
            && matches!(self.mir.types.get(ty), Some(Type::List(_) | Type::Tuple(_)))
            && let Some(repr) = self.python_repr_expr(&RustExpr::atom(format!("&{}", self.operand_expr(operand)?.tight())), ty)
        {
            Ok(("{}", repr.into_string()))
        } else if matches!(
            self.mir.types.get(ty),
            Some(Type::List(_) | Type::Dict(_, _) | Type::Tuple(_))
        ) {
            Ok(("{:?}", self.operand_expr(operand)?.into_string()))
        } else if matches!(self.mir.types.get(ty), Some(Type::Optional(_))) {
            // An `Optional<T>` prints the value INSIDE it, never the Rust
            // wrapper. `{:?}` on an `Option` put `Some("ada")` / `None` into
            // program output, which is a shape no JavaScript runtime prints.
            Ok(("{}", self.console_optional_expr(operand, absent)?.into_string()))
        } else if matches!(self.mir.types.get(ty), Some(Type::BigInt)) {
            // `console.log(10n)` prints `10n`: the console shows a bigint with
            // its suffix, where `String(10n)` (and a template) is `"10"`.
            Ok(("{}n", self.operand_expr(operand)?.into_string()))
        } else if absent == AbsentSpelling::None && matches!(self.mir.types.get(ty), Some(Type::Int))
        {
            // Python's `int` is an integer, not a JavaScript number: `print`
            // writes its decimal digits, exactly (no `f64` round trip).
            Ok(("{}", self.operand_expr(operand)?.into_string()))
        } else if absent == AbsentSpelling::None
            && matches!(self.mir.types.get(ty), Some(Type::Float))
        {
            // Python's `print(12.0)` writes `repr(12.0)`, `12.0`.
            Ok((
                "{}",
                format!(
                    "{fn_name}({})",
                    self.operand_expr(operand)?.delimited(),
                    fn_name = smelt_stdlib::runtime_symbols::number_text::PY_FLOAT_REPR_FN,
                ),
            ))
        } else if matches!(self.mir.types.get(ty), Some(Type::Int | Type::Float)) {
            // A NUMBER goes through the console's own formatter, not Rust's
            // `Display`: JavaScript switches to exponential notation at both
            // ends of the range (`1e+21`, `1e-7`), prints `Infinity` where Rust
            // prints `inf`, and — this being `util.inspect` rather than
            // `String()` — shows a negative zero as `-0`.
            let width = if matches!(self.mir.types.get(ty), Some(Type::Int)) {
                " as f64"
            } else {
                ""
            };
            Ok((
                "{}",
                format!(
                    "{fn_name}({}{width})",
                    self.operand_expr(operand)?.delimited(),
                    fn_name = smelt_stdlib::runtime_symbols::number_text::CONSOLE_NUMBER_FN,
                ),
            ))
        } else if matches!(self.mir.types.get(ty), Some(Type::Bool | Type::String | Type::PyStr | Type::Unknown)) {
            // These render through their own `Display` impl, which matches
            // JavaScript: a bool and a string print as themselves, and
            // `Display for SmeltUnknown` routes its number tag through the
            // spec's rule.
            //
            // An erased value takes the SPEC's rule rather than the console's,
            // which differs for exactly one value: `console.log(-0)` prints
            // `-0` in Node and this prints `0`. Distinguishing them here is not
            // possible — an operand typed `Unknown` does not always emit a
            // `SmeltUnknown` expression (a typed-array element read is an
            // `f64`), so the console formatter cannot be applied on the static
            // type alone. A `SmeltConsoleFormat` trait with one impl per
            // reachable Rust type would close it; see
            // `blocker-logs/hono-fetch-demand.md`.
            Ok(("{}", self.operand_expr(operand)?.into_string()))
        } else {
            // A class instance, generic type parameter, union, function, or set
            // has no Rust `Display` impl, so `{}` would fail to compile (E0277).
            // Erase to the runtime `SmeltUnknown` form, which does implement
            // `Display`, matching how the same values stringify everywhere
            // else. NOT wrapped in the console formatter: `erase_rendered`
            // is the identity for a type that is already erased at runtime, so
            // the text here is not always a `SmeltUnknown` (a typed-array
            // element read is an `f64`), and none of the shapes that reach this
            // arm is a number needing the `-0` spelling.
            Ok(("{}", self.erase_rendered(&self.operand_expr(operand)?, ty)?.into_string()))
        }
    }

    /// Render an `Optional<T>` console argument as a `String` expression.
    ///
    /// The present arm renders the inner value the way `console.log` renders
    /// that type on its own, so the wrapper is invisible: a `string | undefined`
    /// holding `"ada"` prints `ada`, and a `number[] | undefined` holding
    /// `[1, 2]` prints through the container's `{:?}`.
    ///
    /// # Why the absent arm prints `undefined`
    ///
    /// TypeScript's `null` and `undefined` both intern to `Type::None` (see the
    /// annotation lowering for `TSNullKeyword`/`TSUndefinedKeyword`), so
    /// `string | null` and `string | undefined` are the *same*
    /// `Optional(String)` here. Node prints `null` for the first and
    /// `undefined` for the second, and this layer cannot tell them apart, so
    /// one word has to be chosen and it is wrong for the other spelling.
    ///
    /// `undefined` is chosen because it is what nearly every operation that
    /// *produces* an `Optional` in TypeScript returns: `find`, `pop`,
    /// `Map.get`, an optional property or parameter, an index read, `?.`, and
    /// `process.env.X`. `null` arrives mostly from annotations spelled `T |
    /// null` (and from `headers.get`), and a value annotated as plain `null`
    /// keeps printing `null` through the `Type::None` branch above. The
    /// end-to-end fixture `33_console_optional_value` pins this against Node
    /// 22, whose `find()` miss prints `undefined`.
    ///
    /// Printing the right word for both spellings needs a distinct
    /// `Type::Undefined` carried down from the annotation — a type-table
    /// change, not a console change.
    fn console_optional_expr(
        &self,
        operand: &Operand,
        absent: AbsentSpelling,
    ) -> Result<RustExpr, EmitError> {
        let ty = self.operand_ty(operand)?;
        let Some(&Type::Optional(inner)) = self.mir.types.get(ty) else {
            return Err(EmitError::new(
                "console optional rendering requires an optional operand",
            ));
        };
        let present = self.console_value(&RustExpr::path("smelt_value"), inner, absent)?;
        Ok(RustExpr::atom(format!(
            "match &{} {{ Some(smelt_value) => {present}, None => {:?}.to_owned() }}",
            self.operand_expr(operand)?.tight(),
            absent.text(), present = present.delimited()
        )))
    }

    /// Render `value_text` of type `ty` as a `String` expression, console-style.
    ///
    /// The same three cases as [`Self::console_arg`], but producing an
    /// owned `String` rather than a format-spec pair, so it can be used inside a
    /// match arm. `value_text` names a *reference* to the value.
    fn console_value(
        &self,
        value_text: &RustExpr,
        ty: TypeId,
        absent: AbsentSpelling,
    ) -> Result<RustExpr, EmitError> {
        if ty == self.none_ty {
            return Ok(RustExpr::chain("\"null\".to_owned()"));
        }
        if matches!(
            self.mir.types.get(ty),
            Some(Type::List(_) | Type::Dict(_, _) | Type::Tuple(_))
        ) {
            return Ok(RustExpr::atom(format!("format!(\"{{:?}}\", {value_text})", value_text = value_text.delimited())));
        }
        if matches!(
            self.mir.types.get(ty),
            Some(Type::Bool | Type::Int | Type::Float | Type::String | Type::PyStr | Type::Unknown)
        ) {
            return Ok(RustExpr::atom(format!("format!(\"{{}}\", {value_text})", value_text = value_text.delimited())));
        }
        // A nested optional recurses, so `Array<string | undefined>`'s element
        // prints its own inner value rather than a wrapper.
        if let Some(&Type::Optional(inner)) = self.mir.types.get(ty) {
            let present = self.console_value(value_text, inner, absent)?;
            return Ok(RustExpr::atom(format!(
                "match {value_text} {{ Some(smelt_value) => {present}, None => {:?}.to_owned() }}",
                absent.text(), value_text = value_text.delimited(), present = present.delimited()
            )));
        }
        // Every remaining non-`Display` type takes the same erasure route the
        // top-level argument takes.
        let erased = self.erase_rendered(&value_text.cloned_value(), ty)?;
        Ok(RustExpr::atom(format!("format!(\"{{}}\", {erased})", erased = erased.delimited())))
    }
}
