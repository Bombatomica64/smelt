//! Host constructors: `new` of a built-in host class (`URLSearchParams`,
//! `FormData`, `Blob`, `File`, `Buffer`, typed byte buffers, boxed primitives,
//! `Function`, `Proxy`, `AbortController`, marker-only classes), dispatched from
//! `host_constructor_expression`.

use crate::SmeltError;
use crate::lowering::resolve::HostGlobal;
use crate::lowering::body_arena::BodyArena;
use oxc::ast::ast::{Argument, Expression, ObjectPropertyKind, PropertyKey};
use oxc::span::GetSpan;
use smelt_hir::{
    Body, ClosureExpr, ErasureReason, Expr, ExprKind, FunctionType, Literal, Span, Stmt, Type,
};
use super::super::ModuleBuilder;

impl ModuleBuilder<'_> {
    /// Lower `new URLSearchParams(init?)` into a concrete parameter list.
    ///
    /// The modeled `URLSearchParams` class: the initializer's own lowered type
    /// selects the conversion in codegen (a query string, a record, a pair list,
    /// or another parameter list).
    pub(in crate::lowering) fn url_search_params_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if new_expr.arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "URLSearchParams constructor supports at most one initializer",
            ));
        }
        let init = match new_expr.arguments.first() {
            Some(argument) => Some(self.argument(argument, body)?),
            None => None,
        };
        let ty = self.url_search_params_type();
        Ok(body.push_expr(Expr {
            kind: ExprKind::UrlSearchParamsNew { init },
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        }))
    }

    /// `new FormData()`.
    ///
    /// The modeled `FormData` class.
    ///
    /// The constructor takes no arguments here on purpose. The spec's only
    /// parameter is an optional `HTMLFormElement` (plus a `submitter`), which
    /// is DOM and outside the non-DOM profile — so rather than accept and
    /// ignore an argument, one is refused with a named blocker and a form is
    /// always built empty and filled with `append`.
    pub(in crate::lowering) fn form_data_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if !new_expr.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "FormData constructor takes no arguments: the spec's only argument is an \
                 `HTMLFormElement`, which the non-DOM profile does not model",
            ));
        }
        let ty = self.form_data_type();
        Ok(body.push_expr(Expr {
            kind: ExprKind::FormDataNew,
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        }))
    }

    /// Lower a host builtin constructor that Smelt only inspects via
    /// `instanceof` into a marker-bearing record erased to `SmeltUnknown`.
    ///
    /// JavaScript host objects such as `WeakMap`, `WeakSet`, `DataView`,
    /// `SharedArrayBuffer`, and `File` have no useful structural shape that
    /// es-toolkit reads — they are constructed and then only tested with
    /// `value instanceof X` (the `isWeakMap`/`isWeakSet`/`isTypedArray` family
    /// and the `clone` deep-clone dispatch). Rather than erase them to a
    /// shapeless `SmeltUnknown::Object` (which would make every `instanceof`
    /// false and collide each host type with the others), give each a dedicated
    /// `__smelt_<marker>` key so a later dynamic `instanceof` resolves through
    /// the marker (see `instance_of_expr`), mirroring the `ArrayBuffer`/`Blob`
    /// models. Constructor arguments are lowered for their effects/types and
    /// then discarded, since none of the retained shape is observed.
    pub(in crate::lowering) fn marker_only_builtin_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
        marker: &str,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        // Lower arguments for their effects/type checks; the marker record keeps
        // no structural shape from them.
        for argument in &new_expr.arguments {
            self.argument(argument, body)?;
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let unknown_ty = self.erase_with(ErasureReason::UntypedBuiltin, "marker builtin");
        let dict_ty = self.ctx.krate.types.intern(Type::Dict(string_ty, unknown_ty));
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let marker_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(marker.to_owned())),
            ty: string_ty,
            span,
        });
        let marker_value = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(true)),
            ty: bool_ty,
            span,
        });
        let object = body.push_expr(Expr {
            kind: ExprKind::DictLit(vec![(marker_key, marker_value)]),
            ty: dict_ty,
            span,
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: object,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span,
        }))
    }

    /// Return the dedicated identity marker key for a marker-only host builtin
    /// constructor, or `None` when the name is not such a builtin.
    ///
    /// Shared by the `new X(...)` constructor dispatch (to choose the marker to
    /// stamp) and by `instanceof X` lowering (to know the target is modeled).
    /// The mapping lives in the shared `smelt_stdlib::host_object` registry so the
    /// construct side, the `instanceof` side, and the runtime host-marker registry
    /// cannot drift. "Marker-only" here means host objects with no retained
    /// structural fields (`WeakMap`/`WeakSet`/`Request`); host objects with
    /// retained fields (the byte buffers, `Blob`, `File`, boxed primitives,
    /// `DOMException`) have their own dedicated constructors and are excluded here
    /// even though they share the registry.
    pub(crate) fn marker_only_builtin_marker(name: &str) -> Option<&'static str> {
        match name {
            // `Request` joins the marker-only set. A plain `new Request(url)` (the
            // spelling es-toolkit uses to probe host identity, as in
            // `isPlainObject(new Request(url))`) is lowered earlier by
            // `request_constructor_expression`; this entry serves the
            // `host_override` native-construction fallback and the guard
            // recognizers in `guards.rs`.
            "WeakMap" | "WeakSet" | "Request" => smelt_stdlib::host_object_marker(name),
            _ => None,
        }
    }

    /// Lower `new <ByteBufferHost>(...)` through the shared host constructor.
    ///
    /// The registered byte hosts all have concrete family types
    /// (`stdlib::typed_array`), so this path is reached for a host that is
    /// byte-backed WITHOUT being one of them — Node's `Buffer` keeps its own
    /// lowering — and for the reflected construction of an erased record.
    ///
    /// These are JavaScript's binary-data host objects. Source code constructs
    /// them, probes them with `value instanceof ArrayBuffer`, and *operates on
    /// their bytes*: `slice(0)`, `byteLength`, `byteOffset`, and indexed element
    /// reads all appear in the clone/equality paths of any library that handles
    /// binary data. So an identity-only marker record is not enough; the record
    /// needs real byte storage, which the runtime constructor gives it.
    ///
    /// Routing through `ExprKind::HostConstruct` — the same runtime constructor
    /// the reflected `new Object.getPrototypeOf(x).constructor(...)` path calls —
    /// is what makes a directly-constructed record indistinguishable from a
    /// reflectively-constructed one. es-toolkit's `clone` uses the reflected form
    /// where `cloneDeepWith` uses the direct one, and its specs compare the
    /// results against each other.
    pub(in crate::lowering) fn byte_buffer_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        class_name: &str,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let unknown_ty = self.erase_with(ErasureReason::UntypedBuiltin, "byte buffer");
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let args = new_expr
            .arguments
            .iter()
            .map(|argument| self.argument(argument, body))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(body.push_expr(Expr {
            kind: ExprKind::HostConstruct {
                class_name: class_name.to_owned(),
                args,
            },
            ty: unknown_ty,
            span,
        }))
    }

    /// Lower `new Buffer(arg)` to the concrete modeled `Buffer` byte-buffer
    /// record.
    ///
    /// The `new Buffer(...)` form is deprecated in Node in favor of
    /// `Buffer.from`/`Buffer.alloc`, but es-toolkit specs still probe
    /// `value instanceof Buffer`, which needs the constructed value to carry the
    /// `__smelt_buffer` identity. A numeric-array argument becomes the buffer's
    /// bytes; a numeric `new Buffer(size)` allocates a zero-filled length-backed
    /// list; every other argument is evaluated for its effects and backed by an
    /// empty byte list, mirroring the `Buffer.from`/`Buffer.concat` static
    /// lowerings (see `buffer_record_from_bytes`).
    pub(in crate::lowering) fn buffer_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(new_expr.span.start, new_expr.span.end);
        // `ListFromLength` fills with `list[unknown]`, so its destination type
        // must be `List<Unknown>` for the zero-filled `new Buffer(size)` case.
        let bytes = match new_expr.arguments.first() {
            None => self.buffer_empty_bytes(span, body),
            Some(argument) => {
                let value = self.argument(argument, body)?;
                let value_ty = Self::expr_ty(body, value);
                match self.ctx.krate.types.get(value_ty) {
                    // `new Buffer([1, 2, 3])` reuses the numeric source list.
                    Some(Type::List(_)) => value,
                    // `new Buffer(size)` allocates `size` zero-filled bytes.
                    Some(Type::Int | Type::Float) => {
                        let unknown_ty =
                            self.erase_with(ErasureReason::UntypedBuiltin, "new Buffer(size)");
                        let list_ty = self.ctx.krate.types.intern(Type::List(unknown_ty));
                        body.push_expr(Expr {
                            kind: ExprKind::ListFromLength { length: value },
                            ty: list_ty,
                            span,
                        })
                    }
                    // Strings / opaque sources: retain identity, empty bytes.
                    _ => self.buffer_empty_bytes(span, body),
                }
            }
        };
        // Discard any trailing constructor arguments (e.g. an encoding) after
        // evaluating them for their effects, matching the static-call handlers.
        for argument in new_expr.arguments.iter().skip(1) {
            let _ = self.argument(argument, body)?;
        }
        Ok(self.buffer_record_from_bytes(bytes, span, body))
    }

    /// Lower `new Blob(parts?, options?)` to a concrete marker-bearing record.
    ///
    /// JavaScript `Blob` is a host binary-data object: constructed, inspected
    /// via `value instanceof Blob` (the `isBlob` predicate over an erased
    /// `unknown`), and read through `.type`/`.size` (the `clone`/`cloneDeepWith`
    /// paths). Rather than erase it to a shapeless `SmeltUnknown` (which would
    /// lose its identity), model it as a record carrying a dedicated
    /// `__smelt_blob` marker plus its observable `type`, `size`, and `content`,
    /// built by the `smelt_blob_record_from_parts` runtime helper (see
    /// `ExprKind::BlobFromParts`) so a later dynamic `instanceof Blob` resolves
    /// through the marker (see `instance_of_expr`) and field reads observe real
    /// values.
    pub(in crate::lowering) fn blob_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let parts = self.blob_parts_expression(new_expr.arguments.first(), body, span)?;
        let (blob_type, _) =
            self.blob_options_expressions(new_expr.arguments.get(1), "Blob", body, span)?;
        let ty = self.blob_class_type();
        Ok(body.push_expr(Expr {
            kind: ExprKind::BlobFromParts {
                parts,
                blob_type,
                name: None,
                last_modified: None,
            },
            ty,
            span,
        }))
    }

    /// Lower `new File(parts, name, options?)` to a concrete marker-bearing record.
    ///
    /// JavaScript `File` extends `Blob`, so the modeled record carries the
    /// `__smelt_file` marker *on top of* `__smelt_blob` (stamped by the
    /// `smelt_blob_record_from_parts` runtime helper): `file instanceof Blob`
    /// and `file instanceof File` both resolve through their markers, matching
    /// the host subtype relationship that `isBlob`/`isFile` observe. The record
    /// retains `name`, `type`, `lastModified`, `size`, and `content`, so the
    /// `clone`/`cloneDeepWith` paths that rebuild a `File` from those fields
    /// round-trip real values.
    pub(in crate::lowering) fn file_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let parts = self.blob_parts_expression(new_expr.arguments.first(), body, span)?;
        let name = match new_expr.arguments.get(1) {
            Some(name_arg) => {
                let name = self.argument(name_arg, body)?;
                self.blob_string_field_expression(name, name_arg.span(), "File", "name", body)?
            }
            None => body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(String::new())),
                ty: string_ty,
                span,
            }),
        };
        let (blob_type, last_modified) =
            self.blob_options_expressions(new_expr.arguments.get(2), "File", body, span)?;
        let ty = self.file_class_type();
        Ok(body.push_expr(Expr {
            kind: ExprKind::BlobFromParts {
                parts,
                blob_type,
                name: Some(name),
                last_modified,
            },
            ty,
            span,
        }))
    }

    /// Lower a `Blob`/`File` constructor `BlobPart` array.
    ///
    /// `BlobPart` is `Blob | BufferSource | string`. Two of those three arms are
    /// modeled concretely, so a parts array whose ELEMENT TYPE is one of them
    /// keeps that type: `new Blob(["a", "b"])` is a `List<String>` and
    /// `new Blob([blob, other])` a `List<Blob>`, and codegen consumes each
    /// through its own typed constructor. Only a genuinely heterogeneous array —
    /// mixed arms, or a `BufferSource`, which is still the erased byte-backed
    /// host record family — is erased to `SmeltUnknown` and walked at runtime by
    /// `smelt_blob_parts_bytes`. That is the real dynamic boundary here, and it
    /// shrinks to nothing once the typed-array views become concrete.
    ///
    /// A missing argument (`new Blob()`) and an empty array are string parts:
    /// there is nothing heterogeneous about no bytes.
    fn blob_parts_expression(
        &mut self,
        parts_argument: Option<&Argument<'_>>,
        body: &mut Body,
        span: Span,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let Some(argument) = parts_argument else {
            let list_ty = self.ctx.krate.types.intern(Type::List(string_ty));
            return Ok(body.push_expr(Expr {
                kind: ExprKind::ListLit(Vec::new()),
                ty: list_ty,
                span,
            }));
        };
        let parts = self.argument(argument, body)?;
        let parts_ty = Self::expr_ty(body, parts);
        if self.blob_parts_type_is_concrete(parts_ty) {
            return Ok(parts);
        }
        // An empty array literal has no element type to read, so it lowers as
        // `List<Unknown>`; retype it as string parts rather than erasing an
        // array that carries nothing.
        if matches!(self.ctx.krate.types.get(parts_ty), Some(Type::List(_)))
            && matches!(
                body.expr_at(parts).map(|expr| &expr.kind),
                Some(ExprKind::ListLit(items)) if items.is_empty()
            )
        {
            let list_ty = self.ctx.krate.types.intern(Type::List(string_ty));
            return Ok(body.push_expr(Expr {
                kind: ExprKind::ListLit(Vec::new()),
                ty: list_ty,
                span,
            }));
        }
        let unknown_ty = self.erase_with(ErasureReason::UntypedBuiltin, "Blob parts");
        Ok(body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: parts,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span,
        }))
    }

    /// Return whether a `BlobPart` array's type is one codegen consumes directly.
    ///
    /// The two modeled `BlobPart` arms: a list of strings, or a list of blobs
    /// (`File` included — it is the same runtime type).
    fn blob_parts_type_is_concrete(&self, parts_ty: smelt_hir::TypeId) -> bool {
        let Some(Type::List(item)) = self.ctx.krate.types.get(parts_ty) else {
            return false;
        };
        matches!(self.ctx.krate.types.get(*item), Some(Type::String))
            || self.is_blob_class_type(*item)
    }

    /// Resolve the `type` (and `File`-only `lastModified`) expressions from a
    /// `Blob`/`File` constructor options argument.
    ///
    /// An object-literal options argument keeps the spelled `type`/`lastModified`
    /// *value expressions* — arbitrary expressions such as `valueToClone.type`,
    /// not just string literals — and lowers every other property for its
    /// effects. A missing or non-literal options argument falls back to the
    /// empty MIME string a real `Blob` reports when no type is supplied.
    fn blob_options_expressions(
        &mut self,
        options_argument: Option<&Argument<'_>>,
        class_name: &str,
        body: &mut Body,
        span: Span,
    ) -> Result<(smelt_hir::ExprId, Option<smelt_hir::ExprId>), SmeltError> {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let float_ty = self.ctx.krate.types.intern(Type::Float);
        let mut blob_type = None;
        let mut last_modified = None;
        match options_argument {
            Some(Argument::ObjectExpression(object)) => {
                for property in &object.properties {
                    let ObjectPropertyKind::ObjectProperty(property) = property else {
                        continue;
                    };
                    let PropertyKey::StaticIdentifier(key) = &property.key else {
                        let _ = self.expression(&property.value, body)?;
                        continue;
                    };
                    let value = self.expression(&property.value, body)?;
                    let value_span = property.value.span();
                    match key.name.as_str() {
                        "type" => {
                            blob_type = Some(self.blob_string_field_expression(
                                value,
                                value_span,
                                class_name,
                                "options.type",
                                body,
                            )?);
                        }
                        "lastModified" => {
                            let value_ty = Self::expr_ty(body, value);
                            let coerced = if matches!(
                                self.ctx.krate.types.get(value_ty),
                                Some(Type::Int | Type::Float)
                            ) {
                                value
                            } else if self.type_contains_unknown(value_ty) {
                                body.push_expr(Expr {
                                    kind: ExprKind::TypeAssert { value },
                                    ty: float_ty,
                                    span: self.span(value_span.start, value_span.end),
                                })
                            } else {
                                return Err(SmeltError::unsupported(
                                    self.span(value_span.start, value_span.end),
                                    format!(
                                        "{class_name} constructor options.lastModified must be a number"
                                    ),
                                ));
                            };
                            last_modified = Some(coerced);
                        }
                        // Unmodeled options (`endings`, ...) are evaluated for
                        // their effects and dropped from the record.
                        _ => {}
                    }
                }
            }
            // A non-literal options value (a variable, a call) is evaluated for
            // its effects; its fields are not recoverable statically, so the
            // record falls back to the constructor defaults.
            Some(argument) => {
                let _ = self.argument(argument, body)?;
            }
            None => {}
        }
        let blob_type = blob_type.unwrap_or_else(|| {
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(String::new())),
                ty: string_ty,
                span,
            })
        });
        Ok((blob_type, last_modified))
    }

    /// Coerce a lowered `Blob`/`File` constructor field to a string expression,
    /// mirroring the `DOMException` message idiom: strings pass through,
    /// string-compatible or erased values get a runtime `TypeAssert`, anything
    /// else is a lowering error naming the offending field.
    fn blob_string_field_expression(
        &mut self,
        value: smelt_hir::ExprId,
        value_span: oxc::span::Span,
        class_name: &str,
        field_name: &str,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let value_ty = Self::expr_ty(body, value);
        if self.ctx.krate.types.get(value_ty) == Some(&Type::String) {
            return Ok(value);
        }
        if self.is_string_compatible_type(value_ty) || self.type_contains_unknown(value_ty) {
            return Ok(body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value },
                ty: string_ty,
                span: self.span(value_span.start, value_span.end),
            }));
        }
        Err(SmeltError::unsupported(
            self.span(value_span.start, value_span.end),
            format!("{class_name} constructor {field_name} must be a string"),
        ))
    }

    /// Lower a boxed primitive wrapper (`new Number(v)`, `new Boolean(v)`,
    /// `new String(v)`) to a marker-bearing record.
    ///
    /// All three wrappers share this one rule: `new` builds an **object**, so the
    /// wrapper has reference identity of its own (`new String('a') !==
    /// new String('a')`), answers `typeof === "object"` — which is why
    /// es-toolkit's `isNumber`/`isBoolean`/`isString` (`typeof x === "number"`)
    /// must report `false` for it, and modeling it as a record erased to
    /// `SmeltUnknown::Object` is what makes the runtime narrowing correctly miss
    /// — and keeps its payload under the wrapper's own identity marker, so a
    /// later dynamic `instanceof Number`/`Boolean`/`String` resolves through that
    /// marker (mirroring the `ArrayBuffer` model) and `valueOf`/member reads
    /// unbox it.
    ///
    /// The `Number(x)` / `Boolean(x)` / `String(x)` CALLS are coercions, not
    /// constructions: they lower to primitive values on their own path and are
    /// unaffected.
    pub(in crate::lowering) fn boxed_primitive_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
        marker: &str,
        default_value: Literal,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if new_expr.arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "boxed primitive constructors support at most one value argument",
            ));
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let unknown_ty = self.erase_with(ErasureReason::UntypedBuiltin, "boxed primitive");
        let dict_ty = self.ctx.krate.types.intern(Type::Dict(string_ty, unknown_ty));
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let value = if let Some(argument) = new_expr.arguments.first() {
            self.argument(argument, body)?
        } else {
            let default_ty = self.ctx.krate.types.intern(match &default_value {
                Literal::Bool(_) => Type::Bool,
                Literal::String(_) => Type::String,
                _ => Type::Float,
            });
            body.push_expr(Expr {
                kind: ExprKind::Literal(default_value),
                ty: default_ty,
                span,
            })
        };
        let marker_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(marker.to_owned())),
            ty: string_ty,
            span,
        });
        let marker_value = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(true)),
            ty: bool_ty,
            span,
        });
        let value_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("value".to_owned())),
            ty: string_ty,
            span,
        });
        let object = body.push_expr(Expr {
            kind: ExprKind::DictLit(vec![(marker_key, marker_value), (value_key, value)]),
            ty: dict_ty,
            span,
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: object,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span,
        }))
    }

    /// Lower `new Function(...)` to a callable that throws when invoked.
    ///
    /// The `Function` constructor compiles JavaScript source at runtime —
    /// dynamic code evaluation no ahead-of-time Rust translation can honor.
    /// The construction itself succeeds (matching JS, where the error would
    /// only surface when the compiled body misbehaves) and the resulting
    /// value raises a descriptive error if it is ever actually called. The
    /// constructor arguments are lowered for their side effects and dropped.
    pub(in crate::lowering) fn function_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        for argument in &new_expr.arguments {
            let _ = self.argument(argument, body)?;
        }
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let unknown_ty = self.erase_with(ErasureReason::UntypedBuiltin, "new Function");
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let mut closure_body = Body::new(None, span);
        let message = closure_body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(
                "dynamic code evaluation via new Function(...) is not supported".to_owned(),
            )),
            ty: string_ty,
            span,
        });
        closure_body.push_stmt(Stmt::Throw(message));
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: Vec::new(),
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty: unknown_ty,
            is_async: false,
            may_throw: true,
        }));
        Ok(body.push_expr(Expr {
            kind: ExprKind::Closure(ClosureExpr {
                params: Vec::new(),
                rest: None,
                required_params: None,
                return_ty: unknown_ty,
                captures: Vec::new(),
                body: body_id,
                function_item: None,
                span,
            }),
            ty: closure_ty,
            span,
        }))
    }

    /// Lower `new Proxy(target, handler)` to its target value.
    ///
    /// JavaScript `Proxy` is transparent: `x instanceof Proxy` is a `TypeError`
    /// and a proxy reports the identity (`typeof`, `instanceof`, plain-object
    /// shape) of its target. es-toolkit only constructs `new Proxy(target, {})`
    /// in tests of `isPlainObject`, where the proxy must behave exactly like the
    /// wrapped target. There is no faithful distinct identity to invent, so the
    /// closest correct model is to lower the construct to its `target` operand
    /// (the handler is lowered for its effects/types, then discarded). This
    /// keeps the transparent semantics rather than erasing to a wrong marker.
    pub(in crate::lowering) fn proxy_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let Some(target_argument) = new_expr.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "new Proxy(target, handler) requires a target argument",
            ));
        };
        let target = self.argument(target_argument, body)?;
        if let Some(handler_argument) = new_expr.arguments.get(1) {
            let _ = self.argument(handler_argument, body)?;
            // A handler with traps is what the proxy answers, and traps have no model.
            let trapless = matches!(
                handler_argument.as_expression(),
                Some(Expression::ObjectExpression(handler)) if handler.properties.is_empty()
            );
            if !trapless {
                let ty = self.erase_with(ErasureReason::ByName, "proxy with traps");
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::UnmodeledImport,
                    ty,
                    span: self.span(new_expr.span.start, new_expr.span.end),
                }));
            }
        }
        Ok(target)
    }

    /// Lower `new AbortController()` to a concrete, marker-bearing record whose
    /// `signal` shares a mutable `aborted` flag with the controller.
    ///
    /// JavaScript `AbortController` is a host cancellation primitive used by
    /// es-toolkit's `debounce`/`throttle`: the controller exposes a `signal`,
    /// `controller.abort()` flips `signal.aborted` to `true`, and
    /// `signal.addEventListener('abort', cb)` registers callbacks fired by
    /// `abort()`. Rather than erase it to a shapeless `SmeltUnknown` (which would
    /// lose identity and shared mutability), model it as two records:
    ///
    /// - the controller carries a dedicated `__smelt_abortcontroller` marker and
    ///   a `signal` field;
    /// - the signal carries a `__smelt_abortsignal` marker, a mutable `aborted`
    ///   flag (false at construction), and a `__smelt_abort_listeners` array that
    ///   `addEventListener` appends to and `abort()` drains.
    ///
    /// Both records erase to `SmeltUnknown::Object`, whose backing storage is a
    /// shared `Rc<RefCell<..>>`; cloning the controller (or reading its `signal`)
    /// keeps the same backing store, so `controller.abort()` is observed through
    /// any binding that read `controller.signal` earlier. The method behaviors
    /// (`abort`, `addEventListener`, ...) are surfaced as runtime-helper-bound
    /// closures when those fields are read (see the erased-object field path in
    /// `place.rs` and `smelt_abort_method`); `instanceof AbortController` /
    /// `instanceof AbortSignal` use the markers (see `instance_of_expr`).
    pub(in crate::lowering) fn abort_controller_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let unknown_ty = self.erase_with(ErasureReason::UntypedBuiltin, "AbortController");
        let dict_ty = self.ctx.krate.types.intern(Type::Dict(string_ty, unknown_ty));
        let list_ty = self.ctx.krate.types.intern(Type::List(unknown_ty));
        let span = self.span(new_expr.span.start, new_expr.span.end);

        // Push a `Bool`/`String` literal expression and return its id. Kept as
        // local helpers (not methods) so the constructor reads top-to-bottom.
        let string_literal = |target: &mut Body, value: &str| {
            target.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(value.to_owned())),
                ty: string_ty,
                span,
            })
        };
        let signal_marker_key = string_literal(body, "__smelt_abortsignal");
        let aborted_key = string_literal(body, "aborted");
        let listeners_key = string_literal(body, "__smelt_abort_listeners");
        let controller_marker_key = string_literal(body, "__smelt_abortcontroller");
        let signal_key = string_literal(body, "signal");

        let bool_literal = |target: &mut Body, value: bool| {
            target.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(value)),
                ty: bool_ty,
                span,
            })
        };
        let signal_marker_value = bool_literal(body, true);
        let aborted_value = bool_literal(body, false);
        let controller_marker_value = bool_literal(body, true);

        // The shared signal record: marker, mutable `aborted` flag, listeners.
        //
        // `reason` and `__smelt_abort_follows` are deliberately NOT here, even
        // though the runtime helpers use both. An un-aborted signal's reason is
        // `undefined`, which is exactly what a read of an absent key answers,
        // and both helpers already treat an absent follow list as empty — so
        // storing either slot up front would only add an erased entry to every
        // controller a program builds. The es-toolkit corpus makes 32 of them;
        // storing the two slots eagerly cost 64 avoidable-erasure occurrences
        // for no observable difference. `smelt_abort_signal_fire` inserts the
        // reason when there IS one, and `smelt_abort_signal_follow` creates the
        // list when something actually follows.
        let listeners_value = body.push_expr(Expr {
            kind: ExprKind::ListLit(Vec::new()),
            ty: list_ty,
            span,
        });
        let signal_object = body.push_expr(Expr {
            kind: ExprKind::DictLit(vec![
                (signal_marker_key, signal_marker_value),
                (aborted_key, aborted_value),
                (listeners_key, listeners_value),
            ]),
            ty: dict_ty,
            span,
        });
        let signal_unknown = body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: signal_object,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span,
        });

        // The controller record: marker plus the shared signal.
        let controller_object = body.push_expr(Expr {
            kind: ExprKind::DictLit(vec![
                (controller_marker_key, controller_marker_value),
                (signal_key, signal_unknown),
            ]),
            ty: dict_ty,
            span,
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: controller_object,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span,
        }))
    }

    /// The host constructor `name` denotes in this module: [`HostConstructor::of`],
    /// except that a typed-array family name the program declares itself is
    /// the byte-backed host, or nothing.
    pub(in crate::lowering) fn host_constructor_of(&self, host: HostGlobal<'_>) -> Option<HostConstructor> {
        let name = host.name();
        match HostConstructor::of(name)? {
            HostConstructor::TypedArrayFamily if self.classes.contains(name) => {
                smelt_stdlib::byte_buffer_role(name).map(|_| HostConstructor::ByteBuffer)
            }
            kind => Some(kind),
        }
    }

    /// Lower `new <name>(..)` for the undeclared host constructor `name`, the
    /// dispatch over the one [`HostConstructor`] table.
    ///
    /// A name a host-module import binds (`DatabaseSync`, possibly renamed) is
    /// asked first: an imported binding is that module's export, whatever it
    /// spells.
    pub(super) fn host_constructor_expression(
        &mut self,
        host: HostGlobal<'_>,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if let Some(database) = self.sqlite_database_constructor_expression(new_expr, body)? {
            return Ok(Some(database));
        }
        let name = host.name();
        let Some(kind) = self.host_constructor_of(host) else {
            return Ok(None);
        };
        self.construct_host(kind, name, new_expr, body, type_hint).map(Some)
    }

    /// Construct one [`HostConstructor`].
    pub(in crate::lowering) fn construct_host(
        &mut self,
        kind: HostConstructor,
        name: &str,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match kind {
            HostConstructor::Date => self.new_date_expression(new_expr, body),
            HostConstructor::RegExp => self.regexp_constructor_expression(new_expr, body),
            HostConstructor::Array => self.array_constructor_expression(new_expr, body),
            HostConstructor::Object => self.object_constructor_expression(new_expr, body, type_hint),
            HostConstructor::BoxedString => self.boxed_primitive_constructor_expression(
                new_expr,
                body,
                "__smelt_string",
                Literal::String(String::new()),
            ),
            HostConstructor::BoxedNumber => self.boxed_primitive_constructor_expression(
                new_expr,
                body,
                "__smelt_number",
                Literal::Float(0.0),
            ),
            HostConstructor::BoxedBoolean => self.boxed_primitive_constructor_expression(
                new_expr,
                body,
                "__smelt_boolean",
                Literal::Bool(false),
            ),
            HostConstructor::TypedArrayFamily => {
                self.typed_array_constructor_expression(new_expr, name, body)
            }
            HostConstructor::ByteBuffer => self.byte_buffer_constructor_expression(new_expr, name, body),
            HostConstructor::Buffer => self.buffer_constructor_expression(new_expr, body),
            HostConstructor::Blob => self.blob_constructor_expression(new_expr, body),
            HostConstructor::File => self.file_constructor_expression(new_expr, body),
            HostConstructor::Proxy => self.proxy_constructor_expression(new_expr, body),
            HostConstructor::Function => self.function_constructor_expression(new_expr, body),
            HostConstructor::AbortController => self.abort_controller_constructor_expression(new_expr, body),
            HostConstructor::TextEncoder => self.text_encoder_constructor_expression(new_expr, body),
            HostConstructor::TextDecoder => self.text_decoder_constructor_expression(new_expr, body),
            HostConstructor::Headers => self.headers_constructor_expression(new_expr, body),
            HostConstructor::UrlSearchParams => self.url_search_params_constructor_expression(new_expr, body),
            HostConstructor::FormData => self.form_data_constructor_expression(new_expr, body),
            HostConstructor::Response => self.response_constructor_expression(new_expr, body),
            HostConstructor::Request => self.request_constructor_expression(new_expr, body),
            HostConstructor::EventEmitter => self.event_emitter_constructor_expression(new_expr, body),
            HostConstructor::MarkerOnly(marker) => {
                self.marker_only_builtin_constructor_expression(new_expr, body, marker)
            }
            HostConstructor::DomException => self.domexception_object_constructor_expression(new_expr, body),
            HostConstructor::Error => self.error_object_constructor_expression(new_expr, body),
            HostConstructor::Url => self.url_constructor_expression(new_expr, body),
        }
    }
}

/// Every host class `new` can name, the one table of TypeScript host
/// constructors: [`HostConstructor::of`] recognises, and
/// [`ModuleBuilder::construct_host`] constructs through an exhaustive match, so
/// a new entry fails to compile until it says how it is built.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::lowering) enum HostConstructor {
    /// `Date`.
    Date,
    /// `RegExp`.
    RegExp,
    /// `Array`.
    Array,
    /// `Object`.
    Object,
    /// `new String(..)`, an object wrapper.
    BoxedString,
    /// `new Number(..)`, an object wrapper.
    BoxedNumber,
    /// `new Boolean(..)`, an object wrapper.
    BoxedBoolean,
    /// A typed-array view, `ArrayBuffer`, `SharedArrayBuffer` or `DataView`.
    TypedArrayFamily,
    /// A byte-backed host outside that family.
    ByteBuffer,
    /// Node's `Buffer`.
    Buffer,
    /// `Blob`.
    Blob,
    /// `File`.
    File,
    /// `Proxy`.
    Proxy,
    /// `Function`.
    Function,
    /// `AbortController`.
    AbortController,
    /// `TextEncoder`.
    TextEncoder,
    /// `TextDecoder`.
    TextDecoder,
    /// `Headers`.
    Headers,
    /// `URLSearchParams`.
    UrlSearchParams,
    /// `FormData`.
    FormData,
    /// `Response`.
    Response,
    /// `Request`, which is [`Self::MarkerOnly`] when a program overrides the
    /// global.
    Request,
    /// `EventEmitter`.
    EventEmitter,
    /// A host modeled only by its identity marker (`WeakMap`, `WeakSet`).
    MarkerOnly(&'static str),
    /// `DOMException`.
    DomException,
    /// A built-in error class.
    Error,
    /// `URL`.
    Url,
}

impl HostConstructor {
    /// The host constructor spelled `name`, if there is one.
    pub(in crate::lowering) fn of(name: &str) -> Option<Self> {
        Some(match name {
            "Date" => Self::Date,
            "RegExp" => Self::RegExp,
            "Array" => Self::Array,
            "Object" => Self::Object,
            "String" => Self::BoxedString,
            "Number" => Self::BoxedNumber,
            "Boolean" => Self::BoxedBoolean,
            "Buffer" => Self::Buffer,
            "Blob" => Self::Blob,
            "File" => Self::File,
            "Proxy" => Self::Proxy,
            "Function" => Self::Function,
            "AbortController" => Self::AbortController,
            "TextEncoder" => Self::TextEncoder,
            "TextDecoder" => Self::TextDecoder,
            "Headers" => Self::Headers,
            "URLSearchParams" => Self::UrlSearchParams,
            "FormData" => Self::FormData,
            "Response" => Self::Response,
            "Request" => Self::Request,
            "EventEmitter" => Self::EventEmitter,
            "DOMException" => Self::DomException,
            "URL" => Self::Url,
            _ if matches!(
                smelt_stdlib::typescript_stdlib_class(name),
                Some(
                    smelt_stdlib::StdlibClass::TypedArray
                        | smelt_stdlib::StdlibClass::ArrayBuffer
                        | smelt_stdlib::StdlibClass::DataView
                )
            ) =>
            {
                Self::TypedArrayFamily
            }
            _ if smelt_stdlib::byte_buffer_role(name).is_some() => Self::ByteBuffer,
            _ if ModuleBuilder::marker_only_builtin_marker(name).is_some() => {
                Self::MarkerOnly(ModuleBuilder::marker_only_builtin_marker(name)?)
            }
            _ if smelt_stdlib::is_error_class_name(name) => Self::Error,
            _ => return None,
        })
    }

    /// How this host is rebuilt natively behind a program's override of its
    /// global (`host_override`), or `None` when it cannot be overridden.
    /// `Request` needs only its identity there, not its constructor's fields.
    pub(in crate::lowering) fn overridable(self, name: &str) -> Option<Self> {
        match self {
            Self::Blob
            | Self::File
            | Self::Buffer
            | Self::TypedArrayFamily
            | Self::ByteBuffer
            | Self::MarkerOnly(_) => Some(self),
            Self::Request => ModuleBuilder::marker_only_builtin_marker(name).map(Self::MarkerOnly),
            Self::Date
            | Self::RegExp
            | Self::Array
            | Self::Object
            | Self::BoxedString
            | Self::BoxedNumber
            | Self::BoxedBoolean
            | Self::Proxy
            | Self::Function
            | Self::AbortController
            | Self::TextEncoder
            | Self::TextDecoder
            | Self::Headers
            | Self::UrlSearchParams
            | Self::FormData
            | Self::Response
            | Self::EventEmitter
            | Self::DomException
            | Self::Error
            | Self::Url => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HostConstructor, ModuleBuilder};

    /// The names whose global a program can override are the ones the `typeof`
    /// guards treat as defined: a marker, a byte-backed host, `Blob` or `File`.
    #[test]
    fn overridable_hosts_are_the_known_defined_constructors() {
        for name in [
            "Blob", "File", "Buffer", "Request", "WeakMap", "WeakSet", "ArrayBuffer",
            "SharedArrayBuffer", "DataView", "Uint8Array", "BigInt64Array", "URL", "Date", "Map",
            "Set", "Error", "TypeError", "Headers", "Response", "NotAHost",
        ] {
            let table = HostConstructor::of(name).and_then(|kind| kind.overridable(name)).is_some();
            let registry = matches!(name, "Blob" | "File")
                || ModuleBuilder::marker_only_builtin_marker(name).is_some()
                || smelt_stdlib::byte_buffer_role(name).is_some();
            assert_eq!(table, registry, "{name}");
        }
    }

    /// `Request` is a real constructor directly and a marker behind an override.
    #[test]
    fn request_is_a_marker_only_behind_an_override() {
        let request = HostConstructor::of("Request").expect("a host");
        assert_eq!(request, HostConstructor::Request);
        assert!(matches!(request.overridable("Request"), Some(HostConstructor::MarkerOnly(_))));
    }
}
