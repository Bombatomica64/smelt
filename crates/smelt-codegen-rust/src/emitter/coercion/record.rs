//! Record-shaped coercion: a generated record's crossings to and from the
//! erased carrier, field by field.

use crate::rust::RustExpr;
use std::collections::HashSet;

use smelt_hir::{Type, TypeId};
use smelt_mir::Mir;

use crate::emitter::{EmitContext, FunctionEmitter};
use crate::rust::{self, CodeWriter, erased_string};
use crate::thrown::ERROR_BOX;
use crate::type_substitution::TypeSubstitution;
use crate::{EmitError, accessor_prelude};

/// Emits `IntoSmeltUnknown` for a generated record storage type.
///
/// Some lowered object operations temporarily recover string-keyed object
/// shapes from typed class/interface records. The implementation keeps that
/// conversion available without requiring every call site to know the concrete
/// record type that produced the object.
#[expect(clippy::too_many_arguments, reason = "the record impl needs its emission context and lexical type parameters beside the existing record facts")]
pub(crate) fn emit_record_into_smelt_unknown_impl(
    writer: &mut CodeWriter,
    mir: &Mir,
    context: &EmitContext,
    accessor_owner: Option<smelt_hir::Symbol>,
    scoped_type_params: &HashSet<smelt_hir::Symbol>,
    name: &str,
    impl_generics: &str,
    type_args: &str,
    fields: &[smelt_mir::MirField],
    decode_facts: &[smelt_hir::FieldDecodeFacts],
) -> Result<(), EmitError> {
    // Genuine dynamic boundary: this impl IS the typed-to-erased crossing, so
    // `smelt-unknown-report` counts its body as a boundary, not as erasure.
    writer.block(
        format!("impl{impl_generics} IntoSmeltUnknown for {name}{type_args}"),
        |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                // An interface field that may be absent (`p?: T`) and holds
                // `None` IS absent: the erased object has no such key, which
                // is what an object spread, `in` and `Object.keys` observe. A
                // spread of `{ ...defaults, ...options }` used to copy an
                // `undefined` over every default the caller left out.
                let absent_keys = fields
                    .iter()
                    .filter(|field| {
                        field.visibility.is_own_property()
                            && matches!(mir.types.get(field.ty), Some(Type::Optional(_)))
                            && decode_facts
                                .iter()
                                .any(|facts| facts.name == field.name && facts.may_be_absent)
                    })
                    .map(|field| {
                        mir.names
                            .get(field.name)
                            .or_else(|| mir.symbols.get(field.name))
                            .unwrap_or("field")
                            .to_owned()
                    })
                    .collect::<Vec<_>>();
                if !absent_keys.is_empty() {
                    fn_writer.line(format!(
                        "let smelt_absent_keys: [&str; {}] = {:?};",
                        absent_keys.len(),
                        absent_keys
                    ));
                }
                fn_writer.line(
                    "SmeltUnknown::Object(SmeltObject::new(Vec::<(String, _)>::from([",
                );
                for field in fields {
                    if !field.visibility.is_own_property() {
                        continue;
                    }
                    // The erased key is the SOURCE spelling (`mir.names`), the
                    // Rust field is the sanitized one: `onRead` must read back
                    // as `"onRead"` from an erased view, not as `"on_read"`.
                    let field_name = context.names().field(mir, field.name).into_string();
                    let key = mir
                        .names
                        .get(field.name)
                        .or_else(|| mir.symbols.get(field.name))
                        .unwrap_or("field");
                    // A field declared `p: T | null` (it admits `null` and can
                    // never be absent) holds `null` when its `Option` is empty,
                    // and its erased view says so: `JSON.stringify` prints
                    // `"p":null` rather than omitting the key.
                    let absent_is_null = decode_facts.iter().any(|facts| {
                        facts.name == field.name && facts.admits_null && !facts.may_be_absent
                    });
                    // An accessor is read by running its getter now: the erased
                    // view holds the value the property answers at erase time.
                    let field_source = if accessor_owner
                        .is_some_and(|owner| accessor_prelude::is_accessor_field(mir, owner, field.name))
                    {
                        RustExpr::atom(format!("({})", accessor_prelude::read_expr(&RustExpr::from(format!("self.{field_name}"))).delimited()))
                    } else {
                        RustExpr::path(format!("self.{field_name}"))
                    };
                    let value = match mir.types.get(field.ty) {
                        Some(Type::Optional(inner)) if absent_is_null => {
                            record_field_unknown_expr(mir, &RustExpr::path("smelt_present"), *inner).map(|present| {
                                format!(
                                    "{field_source}.map_or(SmeltUnknown::Null, |smelt_present| {present})", field_source = field_source.tight(), present = present.delimited()
                                )
                            })
                        }
                        _ => erased_function_field_expr(
                            mir,
                            context,
                            scoped_type_params,
                            &field_source,
                            field.ty,
                        )
                        .map_or_else(
                            || record_field_unknown_expr(mir, &field_source, field.ty),
                            Ok,
                        ).map(RustExpr::into_string),
                    }
                    .unwrap_or_else(|_| "SmeltUnknown::Null".to_owned());
                    fn_writer.line(format!("({key:?}.to_owned(), {value}),"));
                }
                if absent_keys.is_empty() {
                    fn_writer.line("])))");
                } else {
                    fn_writer.line("]).into_iter().filter(|(smelt_key, smelt_value)| !(matches!(smelt_value, SmeltUnknown::Undefined) && smelt_absent_keys.contains(&smelt_key.as_str()))).collect::<Vec<_>>()))");
                }
            });
        },
    );
    Ok(())
}

/// Whether a value of `ty` can be rebuilt from its erased `SmeltUnknown` view.
///
/// The whitelist behind [`emit_record_from_smelt_unknown_impl`]. Recoverable
/// shapes are the primitives, the erased carrier itself, the generated
/// containers, and generated records/unions (which now emit their own
/// `SmeltFromUnknown`). Everything else keeps the record's `Default`: callbacks,
/// futures and generators have no inbound impl in the prelude, and runtime
/// builtin classes (a compiled `RegExp` among them, even though the prelude does
/// emit `SmeltFromUnknown for SmeltRegExp`) are left out of this whitelist
/// because the program does not generate them.
pub(crate) fn type_supports_from_unknown(
    mir: &Mir,
    context: &EmitContext,
    ty: TypeId,
) -> bool {
    match mir.types.get(ty) {
        Some(
            Type::Bool
            | Type::Int
            | Type::Float
            | Type::String | Type::PyStr
            | Type::Unknown
            | Type::TypeParam { .. },
        ) => true,
        // A class is recoverable only when *this* program generates it, because
        // that is what now emits `SmeltFromUnknown`. `Type::Class` also names
        // runtime builtins, which this program does not generate, so they are
        // excluded.
        //
        // A *reference* class is generated too, as an `Rc<RefCell<Inner>>`
        // newtype by `emit_reference_class_storage`. It now carries an inbound
        // impl as well (`reference_origin::emit_reference_from_smelt_unknown_impl`,
        // which restores the live handle an erasure stamped), so it satisfies
        // the generated `SmeltFromUnknown` type-parameter bound. This whitelist
        // still leaves it out: it decides where the EMITTER reaches for the
        // impl (record fields, prototype-method parameters), and widening those
        // sites is a separate change from making identity round-trip.
        Some(Type::Class { name, .. }) => {
            let name = *name;
            !context.is_reference_class(name)
                && (mir.has_class(name)
                    || mir.has_interface(name))
        }
        Some(Type::List(item) | Type::Set(item) | Type::Optional(item)) => {
            type_supports_from_unknown(mir, context, *item)
        }
        Some(Type::Dict(key, value) | Type::JsMap(key, value)) => {
            type_supports_from_unknown(mir, context, *key)
                && type_supports_from_unknown(mir, context, *value)
        }
        Some(Type::Tuple(items) | Type::Union(items)) => items
            .iter()
            .all(|item| type_supports_from_unknown(mir, context, *item)),
        _ => false,
    }
}

/// Emits `SmeltFromUnknown` for a generated record storage type.
///
/// The inbound mirror of [`emit_record_into_smelt_unknown_impl`]. Every lifted
/// type parameter is bounded by `SmeltFromUnknown`, so a class that lacks the
/// impl cannot be used as a generic argument at all: es-toolkit's `meanBy`/
/// `medianBy` specs call generic helpers with `Person[]` and failed with "the
/// trait bound `Person: SmeltFromUnknown` is not satisfied". Only the outbound
/// half was ever emitted, which is why concrete class values could flow out to
/// erased code but never back.
///
/// The recovery is total rather than fallible: a non-object input, a missing
/// key, or a field the projection skips all fall back to `Default`, which every
/// generated record derives. Private fields are not part of the erased object
/// view, so they take that default too.
pub(crate) fn emit_record_from_smelt_unknown_impl(
    writer: &mut CodeWriter,
    mir: &Mir,
    context: &EmitContext,
    name: &str,
    impl_generics: &str,
    type_args: &str,
    fields: &[smelt_mir::MirField],
    scoped_type_params: &HashSet<smelt_hir::Symbol>,
) -> Result<(), EmitError> {
    writer.block(
        format!("impl{impl_generics} SmeltFromUnknown for {name}{type_args}"),
        |impl_writer| {
            impl_writer.block(
                "fn smelt_from_unknown(value: SmeltUnknown) -> Self",
                |fn_writer| {
                    fn_writer.line("let mut result = Self::default();");
                    fn_writer.block("if let SmeltUnknown::Object(object) = value", |body| {
                        for field in fields {
                            if !field.visibility.is_own_property() {
                                continue;
                            }
                            // Only rebuild fields whose type can actually be
                            // recovered. This is a whitelist rather than a
                            // blacklist: a field the erased view cannot restore
                            // (a callback handle, a compiled regex) keeps its
                            // `Default`, and a type added later has to opt in
                            // rather than silently emit a call to a
                            // `SmeltFromUnknown` impl that does not exist.
                            let field_name =
                                context.names().field(mir, field.name).into_string();
                            let key = mir
                                .names
                                .get(field.name)
                                .or_else(|| mir.symbols.get(field.name))
                                .unwrap_or("field");
                            if matches!(mir.types.get(field.ty), Some(Type::Function(_))) {
                                emit_function_field_origin_restore(
                                    body,
                                    mir,
                                    context,
                                    scoped_type_params,
                                    &field_name,
                                    key,
                                    field.ty,
                                );
                                continue;
                            }
                            if !type_supports_from_unknown(mir, context, field.ty) {
                                continue;
                            }
                            // Read the SOURCE spelling out of the erased object;
                            // write the sanitized Rust field. See the outbound
                            // impl above.
                            body.block(
                                format!("if let Some(smelt_field) = object.get({key:?})"),
                                |assign| {
                                    assign.line(format!(
                                        "result.{field_name} = SmeltFromUnknown::smelt_from_unknown(smelt_field);"
                                    ));
                                },
                            );
                        }
                    });
                    fn_writer.line("result");
                },
            );
        },
    );
    Ok(())
}

/// Restores a function-typed record field from an erased object, by identity.
///
/// A record that crosses an erased boundary and comes back (an options bag
/// passed as a generic `T`, then `Object.assign(this, options)`) carries its
/// callable members as erased functions. Leaving the field at its `Default`
/// replaces a live method with a no-op: Hono's `basePath()`/`route()` build a
/// clone whose `router` came back through here, and every route added to the
/// clone went into a router whose `add` did nothing, so mounted routes 404'd.
///
/// The erasure of a typed callback registers that callback as the erased
/// function's origin (`smelt_register_function_origin`), so the field is
/// restored to the SAME typed `Rc` — the identity JavaScript preserves — when
/// the origin has the field's exact type. A function that never had a typed
/// origin of this type keeps the `Default` it had before; adapting an arbitrary
/// erased function to a typed signature needs the per-call-site coercion the
/// function emitter owns.
fn emit_function_field_origin_restore(
    body: &mut CodeWriter,
    mir: &Mir,
    context: &EmitContext,
    scoped_type_params: &HashSet<smelt_hir::Symbol>,
    field_name: &str,
    key: &str,
    field_ty: TypeId,
) {
    let Ok(field_ty_rendered) = FunctionEmitter::rust_type_with_scoped_type_params(
        mir,
        context,
        field_ty,
        &TypeSubstitution::lexical(scoped_type_params),
    ) else {
        return;
    };
    // Origins are recorded only for typed `Rc` callables (the erasure in
    // `emitter/coercion.rs`), so a field spelled any other way (an erased
    // `SmeltErasedFunction`) has no origin to find and keeps its `Default`.
    if field_ty_rendered.shape() != rust::RustShape::Callable {
        return;
    }
    body.block(format!("if let Some(smelt_field) = object.get({key:?})"), |assign| {
        assign.line(
            "let smelt_function = match smelt_field { SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get(\"__smelt_call\") { Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function), _ => None }, _ => None };",
        );
        assign.line(format!(
            "if let Some(smelt_origin) = smelt_function.and_then(|smelt_function| smelt_restore_function_origin::<{field_ty_rendered}>(&smelt_function)) {{ result.{field_name} = smelt_origin; }}"
        ));
    });
}

/// Recovers a value of type `ty` from a `SmeltUnknown` expression.
///
/// The mirror of [`record_field_unknown_expr`], and it exists for the same
/// reason: a Rust TUPLE of an arity without an emitted `SmeltFromUnknown` impl has none, so the blanket
/// `SmeltFromUnknown::smelt_from_unknown(…)` an erased adapter reaches for does
/// not compile against a tuple parameter. `RegExpRouter#add`'s
/// `HandlerWithMetadata<T> = [T, Record<string, number>]` parameter reported
/// "the trait `SmeltFromUnknown` is not implemented for
/// `(T, SmeltRecord<String, f64>)`".
///
/// A tuple is recovered ELEMENT-WISE from the erased array it was written as,
/// recursing so a tuple of tuples works too. Everything else keeps the blanket
/// impl, which is what its type provides.
///
/// The `panic!` on a non-array matches the spelling the emitter's other
/// tuple-extraction sites already use, rather than inventing a second answer for
/// the same impossible input.
pub(crate) fn record_field_from_unknown_expr(
    mir: &Mir,
    value_text: &RustExpr,
    ty: TypeId,
) -> Result<RustExpr, EmitError> {
    let Some(Type::Tuple(items)) = mir.types.get(ty) else {
        return Ok(RustExpr::atom(format!(
            "SmeltFromUnknown::smelt_from_unknown({value_text})", value_text = value_text.delimited()
        )));
    };
    let elements = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            record_field_from_unknown_expr(
                mir,
                &RustExpr::chain(format!(
                    "smelt_tuple_values.get({index}).cloned().unwrap_or(SmeltUnknown::Undefined)"
                )),
                *item,
            )
        })
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    Ok(RustExpr::atom(format!(
        "(if let SmeltUnknown::Array(smelt_tuple_values) = {value_text} {{ ({elements}) }} else {{ panic!(\"unknown is not tuple\") }})", value_text = value_text.delimited()
    )))
}

/// Erases a FUNCTION-typed record field (`p: (a: A) => R` or `p?: ..`) to a
/// callable `SmeltUnknown`, or `None` for a signature this adapter declines.
///
/// [`record_field_unknown_expr`] spells a function field `SmeltUnknown::Null`
/// because it has no emitter to render the adapter. The erased view of an
/// interface struct then lost its methods: Hono's `Object.assign(this, rest)`
/// copied a `Router` through such a view, and the router it read back had a
/// `match` that answered `null` for every request. The adapter converts each
/// argument with `SmeltFromUnknown` (its target type is inferred from the
/// callable itself, so the interface's own type parameters need no
/// rendering), erases the result with `IntoSmeltUnknown`, and registers the
/// typed callable as its origin so converting back yields that same callable.
///
/// A parameter the rendered signature borrows (`&T`) gets a reference to the
/// converted value. Declined, keeping the old spelling: rest, async and
/// mutable-parameter signatures, an `impl` or `&mut` in the rendered type, and
/// a parameter type that cannot be produced from an erased value.
fn erased_function_field_expr(
    mir: &Mir,
    context: &EmitContext,
    scoped_type_params: &HashSet<smelt_hir::Symbol>,
    value_text: &RustExpr,
    ty: TypeId,
) -> Option<RustExpr> {
    let (optional, function_ty) = match mir.types.get(ty)? {
        Type::Optional(inner) => (true, *inner),
        _ => (false, ty),
    };
    let Type::Function(function) = mir.types.get(function_ty)? else {
        return None;
    };
    if function.rest.is_some() || function.is_async || !function.mutable_params.is_empty() {
        return None;
    }
    let rendered = FunctionEmitter::rust_type_with_scoped_type_params(
        mir,
        context,
        function_ty,
        &TypeSubstitution::lexical(scoped_type_params),
    )
    .ok()?;
    if rendered.shape() != rust::RustShape::Callable {
        return None;
    }
    if !function
        .params
        .iter()
        .all(|param| type_supports_from_unknown(mir, context, *param))
    {
        return None;
    }
    let emitter = FunctionEmitter::type_renderer(mir, context, function_ty).ok()?;
    let borrowed = function
        .params
        .iter()
        .enumerate()
        .map(|(index, param)| emitter.callback_param_is_shared_reference(function, index, *param))
        .collect::<Vec<_>>();
    let args = borrowed
        .iter()
        .enumerate()
        .map(|(index, borrowed)| {
            let reference = if *borrowed { "&" } else { "" };
            format!(
                "{reference}SmeltFromUnknown::smelt_from_unknown(smelt_args.get({index}).cloned().unwrap_or(SmeltUnknown::Undefined))"
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let call = if function.may_throw {
        format!("(smelt_function_value)({args})?")
    } else {
        format!("(smelt_function_value)({args})")
    };
    let adapter = |callable: &str| {
        format!(
            "{{ let smelt_function_value: {rendered} = {callable}; let smelt_function_origin = smelt_function_value.clone(); let smelt_erased_function: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, {ERROR_BOX}>> = ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, {ERROR_BOX}>(({call}).into_smelt_unknown())); smelt_register_function_origin(&smelt_erased_function, smelt_function_origin); SmeltUnknown::Function(smelt_erased_function) }}"
        )
    };
    Some(if optional {
        value_text.postfix(format!(
            ".map_or(SmeltUnknown::Undefined, |smelt_present| {})",
            adapter("smelt_present")
        ))
    } else {
        RustExpr::from(adapter(&format!("{value_text}.clone()", value_text = value_text.tight())))
    })
}

/// Renders a generated record field as a `SmeltUnknown` expression.
pub(crate) fn record_field_unknown_expr(mir: &Mir, value_text: &RustExpr, ty: TypeId) -> Result<RustExpr, EmitError> {
    Ok(match mir.types.get(ty) {
        Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. }) => {
            RustExpr::atom(format!("({value_text}).into_smelt_unknown()", value_text = value_text.delimited()))
        }
        Some(
            Type::None
            | Type::Never
            | Type::Future(_)
            | Type::Generator { .. }
            | Type::GeneratorResult { .. },
        )
        | None => {
            RustExpr::atom("SmeltUnknown::Null")
        }
        Some(Type::Bool) => RustExpr::atom(format!("SmeltUnknown::Bool({value_text})", value_text = value_text.delimited())),
        Some(Type::Int | Type::FixedInt(_) | Type::Float) => RustExpr::atom(format!("SmeltUnknown::Number({value_text} as f64)", value_text = value_text.as_cast_operand())),
        Some(Type::BigInt) => RustExpr::atom(format!("SmeltUnknown::BigInt(({value_text}).clone())", value_text = value_text.delimited())),
        Some(Type::String | Type::PyStr) => erased_string(value_text),
        Some(Type::Optional(inner)) => {
            let inner_text = record_field_unknown_expr(mir, &RustExpr::path("smelt_value"), *inner)?;
            value_text.postfix(format!(".map_or(SmeltUnknown::Undefined, |smelt_value| {inner_text})", inner_text = inner_text.delimited()))
        }
        Some(Type::List(item) | Type::Set(item)) => {
            let item_text = record_field_unknown_expr(mir, &RustExpr::path("smelt_value"), *item)?;
            RustExpr::atom(format!(
                "SmeltUnknown::Array({value_text}.into_iter().map(|smelt_value| {item_text}).collect())", value_text = value_text.tight(), item_text = item_text.delimited()
            ))
        }
        Some(Type::Dict(key, item)) if matches!(mir.types.get(*key), Some(Type::String | Type::PyStr)) => {
            let item_text = record_field_unknown_expr(mir, &RustExpr::path("smelt_value"), *item)?;
            RustExpr::atom(format!(
                "SmeltUnknown::Object(SmeltObject::new({value_text}.into_iter().map(|(smelt_key, smelt_value)| (smelt_key, {item_text})).collect()))", value_text = value_text.tight(), item_text = item_text.delimited()
            ))
        }
        // A tuple erases ELEMENT-WISE to a JS array, like every other structural
        // shape above it. It was grouped with the shapes that have an
        // `IntoSmeltUnknown` impl, but a Rust tuple has none — the prototype
        // carrier for `RegExpRouter#buildAllMatchers`, whose return type is
        // `[RegExp, HandlerParamsSet<T>[][], …]`, asked for
        // `(value).into_smelt_unknown()` and got "no method named
        // `into_smelt_unknown` found for tuple" (H29, second half).
        //
        // The value is bound once: `value_text` is an arbitrary expression here,
        // and indexing it per element would evaluate it once per element.
        Some(Type::Tuple(items)) => {
            let elements = items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    record_field_unknown_expr(mir, &RustExpr::chain(format!("smelt_tuple.{index}")).indexed_place(), *item)
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            RustExpr::atom(format!(
                "{{ let smelt_tuple = {value_text}; SmeltUnknown::Array(vec![{elements}].into()) }}", value_text = value_text.delimited()
            ))
        }
        Some(Type::Dict(_, _) | Type::JsMap(_, _) | Type::Class { .. }) => {
            RustExpr::atom(format!("({value_text}).into_smelt_unknown()", value_text = value_text.delimited()))
        }
        Some(Type::Function(_)) => RustExpr::atom("SmeltUnknown::Null"),
    })
}
