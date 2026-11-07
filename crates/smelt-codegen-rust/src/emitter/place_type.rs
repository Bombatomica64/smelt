//! The MIR type a place reads at, and the member types of the host classes a
//! place can project through.

use super::*;
use crate::rust::RustType;

impl FunctionEmitter<'_> {
    /// Converts a blocking HTTP GET operation to Rust text.
    /// Gets the type of a place.
    pub(super) fn place_ty(&self, place: &Place) -> Result<TypeId, EmitError> {
        match place {
            Place::Local(local) => Ok(self.local_decl(*local)?.ty),
            // `Place::Global` is only ever an assignment target, and the
            // assignment path reads the global's declared type directly rather
            // than asking for a place type. Reaching here means a global place
            // was used as a value, which is a compiler bug.
            Place::Global { .. } => Err(EmitError::new(
                "internal: a mutable-global place has no read type",
            )),
            Place::Field { base, field } => self.field_place_ty(*base, *field),
            Place::Index { base, index, negative } => self.index_place_ty(*base, index, *negative),
        }
    }

    /// Find the interned stdlib `RegExp` class type, if the program uses it.
    ///
    /// The frontend interns it whenever it types a `RegExp` value, so a
    /// run-time `new RegExp(p)` call (whose builtin callee carries no type of
    /// its own) can name its result type through this lookup.
    pub(super) fn regexp_class_ty(&self) -> Option<TypeId> {
        self.mir
            .types
            .all()
            .iter()
            .position(|ty| {
                matches!(
                    ty,
                    Type::Class { name, args } if args.is_empty()
                        && self.is_regexp_class_symbol(*name) == Ok(true)
                )
            })
            .and_then(|index| compact_index(index, "type index does not fit u32").ok())
            .map(TypeId)
    }

    /// Find the interned `__SmeltMatchGroups` class type, if the program uses it.
    ///
    /// The frontend interns this synthetic class whenever it types a `.groups`
    /// read, so it is present in the type table by the time codegen runs.
    fn match_groups_class_ty(&self) -> Option<TypeId> {
        self.mir
            .types
            .all()
            .iter()
            .position(|ty| {
                matches!(
                    ty,
                    Type::Class { name, .. }
                        if self.match_class_kind(*name)
                            == Ok(Some(smelt_stdlib::StdlibClass::MatchGroups))
                )
            })
            .and_then(|index| compact_index(index, "type index does not fit u32").ok())
            .map(TypeId)
    }

    /// Resolve the result type of a field read on a synthetic match-result class.
    ///
    /// Mirrors the frontend `builtin_class_field_type` typing so `place_ty` and
    /// the emitted accessor agree: a `.groups` read yields the named-group
    /// accessor class, `.index`/`.length` are floats, `.input` is a string, and
    /// every named-group read on the accessor class is an optional string. The
    /// concrete types are already interned by the frontend that typed the read,
    /// so `find_type_id` resolves them; an unmodeled field on the match value
    /// itself falls back to the erased boundary only if the program interned it.
    pub(super) fn match_field_ty(
        &self,
        kind: smelt_stdlib::StdlibClass,
        field: Symbol,
    ) -> Result<TypeId, EmitError> {
        let string_ty = self.type_id(Type::String)?;
        match kind {
            smelt_stdlib::StdlibClass::MatchGroups => self.type_id(Type::Optional(string_ty)),
            _ => match self.symbol_source_name(field)? {
                "index" | "length" => self.type_id(Type::Float),
                "input" => Ok(string_ty),
                "groups" => self.match_groups_class_ty().ok_or_else(|| {
                    EmitError::new("match groups accessor class type is not interned")
                }),
                other => self.erased_fallback_ty(|| {
                    format!("the read of `{other}` on a regular-expression match result")
                }),
            },
        }
    }

    /// The type a read of a `RegExp` member yields.
    pub(super) fn regexp_member_ty(
        &self,
        member: smelt_stdlib::RegExpMember,
    ) -> Result<TypeId, EmitError> {
        self.type_id(match member {
            smelt_stdlib::RegExpMember::Text => Type::String,
            smelt_stdlib::RegExpMember::Flag => Type::Bool,
            smelt_stdlib::RegExpMember::LastIndex => Type::Float,
        })
    }

    /// The type a field place reads at.
    fn field_place_ty(&self, base: LocalId, field: Symbol) -> Result<TypeId, EmitError> {
        let base_ty = self.local_decl(base)?.ty;
        // A `.length` read on a CONCRETE collection (typed list or set)
        // renders as `({base}.len() as f64)` (see the matching arm in
        // `place::field_read_text`), so its value type is `Float`, not the
        // erased `Unknown` the struct-field fallback would otherwise
        // report. Keeping the resolved type aligned with the rendered
        // expression lets callers coerce from the concrete `f64` instead
        // of treating an already-concrete value as erased.
        // A tuple's `.length` renders as its constant arity.
        if matches!(
            self.mir.types.get(base_ty),
            Some(Type::List(_) | Type::Set(_) | Type::Tuple(_))
        ) && smelt_stdlib::typescript_field_rule(self.symbol_source_name(field)?)
            == Some(smelt_stdlib::FieldRule::TsLength)
        {
            return self.type_id(Type::Float);
        }
        // Likewise for a `String` base: `place::field_read_text` routes
        // it to `string_field_expr`, which renders `.length` as a
        // character count and `.source` as a `String`. Reporting
        // `Unknown` here made callers coerce an ALREADY concrete
        // expression as if it were erased -- a `${s.length}`
        // interpolation ran the `SmeltUnknown` ToString match over an
        // `i64` and did not compile. `string_field_read` decides the
        // text and the type together; ask it for the type.
        if matches!(self.mir.types.get(base_ty), Some(Type::String)) {
            return Ok(self.string_field_read(&RustExpr::loose(""), field)?.1);
        }
        // A GENERATED union receiver whose every arm declares the field
        // at one type reads at that type: `place::place_expr` dispatches
        // it through the union's own arms, so reporting the erased
        // fallback here would make callers re-coerce an already concrete
        // value. The text and the type are decided by the same rule.
        if smelt_stdlib::typescript_field_rule(self.symbol_source_name(field)?)
            == Some(smelt_stdlib::FieldRule::TsLength)
            && self.concrete_union_length_read_expr(base)?.is_some()
        {
            return self.type_id(Type::Float);
        }
        if let Some(field_ty) = self.concrete_union_field_ty(base_ty, field) {
            return Ok(field_ty);
        }
        if let Some((_, descriptor)) = self.descriptor_for_field(base_ty, field) {
            return Ok(descriptor.read_ty);
        }
        if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
            && let Some(kind) = self.match_class_kind(*name)?
        {
            return self.match_field_ty(kind, field);
        }
        // A concrete builtin `RegExp` receiver exposes data properties
        // with statically known concrete field types backed by the
        // generated `SmeltRegExp` runtime shape. Resolve them here so
        // the field operand's type agrees with `regexp_field_expr` and
        // `field_access_type`; without this the generic class arm below
        // (which only knows user classes/interfaces) would erase the
        // read to `Unknown` and make callers re-coerce an already
        // concrete value.
        // `URLSearchParams.size` is the one data property the spec
        // defines on a concrete params value; it is a number.
        if self.is_url_search_params_class_type(base_ty)?
            && self.symbol_name(field)? == "size"
        {
            return self.type_id(Type::Float);
        }
        if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
            && self.is_regexp_class_symbol(*name)?
        {
            let member = self.symbol_name(field)?;
            return match smelt_stdlib::regexp_member(member) {
                Some(kind) => self.regexp_member_ty(kind),
                None => self.erased_fallback_ty(|| {
                    format!("the read of `{member}` on a `RegExp`")
                }),
            };
        }
        match self.mir.types.get(base_ty) {
            Some(Type::Dict(_, value)) => Ok(*value),
            Some(Type::Optional(inner)) => {
                if let Some(Type::Dict(_, value)) = self.mir.types.get(*inner) {
                    return self.type_id(Type::Optional(*value));
                }
                let Some(fields) = self.structural_record_fields(*inner) else {
                    return self.erased_fallback_ty(|| {
                        format!(
                            "the read of `{}` on the optional receiver `{}`, which has no record fields",
                            self.symbol_name(field).unwrap_or("<unnamed>"),
                            self.blocker_type_text(*inner)
                        )
                    });
                };
                fields
                    .into_iter()
                    .find(|record_field| record_field.name == field)
                    .map(|record_field| {
                        if matches!(
                            self.mir.types.get(record_field.ty),
                            Some(Type::Optional(_))
                        ) {
                            record_field.ty
                        } else {
                            self.type_id(Type::Optional(record_field.ty))
                                .unwrap_or(record_field.ty)
                        }
                    })
                    .ok_or_else(|| {
                        // Name the field and the receiver: the site
                        // annotation says which function, and this says
                        // which read inside it.
                        EmitError::new(format!(
                            "optional record field `{}` is unknown on {}",
                            self.symbol_name(field).unwrap_or("<unnamed>"),
                            self.rendered_type(*inner)
                                .unwrap_or_else(|_| RustType::from(format!("{:?}", self.mir.types.get(*inner)))),
                        ))
                    })
            }
            // The read's type is the field type of *this*
            // instantiation, so the record's type arguments have to be
            // substituted into it: `status?: T` off an
            // `InitLike<f64>` receiver is an `Option<f64>`, and the
            // generated struct's field is rendered at exactly that
            // type. Reporting the declaration-time `T` instead made
            // callers treat an already-concrete field as erased and
            // coerce `SmeltUnknown` into `f64` (E0308). This is the
            // same substitution `structural_record_fields` performs;
            // the lookup order (class, then interface) is kept as it
            // was.
            Some(Type::Class { name, args }) => {
                let field_ty = if let Some(class) =
                    crate::classes::find_class(self.mir, *name)
                {
                    self.substitute_record_field_type_params(
                        &class.type_params,
                        args,
                        crate::classes::effective_class_fields(self.mir, class),
                    )
                    .into_iter()
                    .find(|class_field| class_field.name == field)
                    .map(|class_field| class_field.ty)
                } else if let Some(interface) = self
                    .mir
                    .interfaces
                    .iter()
                    .find(|interface| interface.name == *name)
                {
                    self.substitute_record_field_type_params(
                        &interface.type_params,
                        args,
                        crate::classes::effective_interface_fields(self.mir, interface),
                    )
                    .into_iter()
                    .find(|interface_field| interface_field.name == field)
                    .map(|interface_field| interface_field.ty)
                } else {
                    None
                };
                match field_ty {
                    Some(ty) => Ok(ty),
                    // An undeclared member on an index-signature class is
                    // a keyed store read; its type is the store's value
                    // type `T` (issue #84), not an erased `Unknown`.
                    None => match self.class_index_store_types(base_ty) {
                        Some((_key_ty, value_ty)) => Ok(value_ty),
                        None => self.erased_fallback_ty(|| {
                            format!(
                                "the read of `{}` on `{}`, which declares no such member",
                                self.symbol_name(field).unwrap_or("<unnamed>"),
                                self.blocker_type_text(base_ty)
                            )
                        }),
                    },
                }
            }
            _ => self.erased_fallback_ty(|| {
                format!(
                    "the read of `{}` on the receiver `{}`",
                    self.symbol_name(field).unwrap_or("<unnamed>"),
                    self.blocker_type_text(base_ty)
                )
            }),
        }
    }

    /// The type an index place reads at.
    fn index_place_ty(
        &self,
        base: LocalId,
        index: &Operand,
        negative: NegativeIndex,
    ) -> Result<TypeId, EmitError> {
        let base_ty = self.local_decl(base)?.ty;
        // Decided by the same rule that renders the read
        // (`place::place_expr`), so callers coerce from the arm-typed
        // value rather than re-narrowing an erased one.
        if let Some((read_ty, _)) =
            self.concrete_union_index_reads(base_ty, index, negative)?
        {
            return Ok(read_ty);
        }
        // An indexed read on a typed-array view decodes ONE element at
        // the view's own width and renders as an `f64` (see
        // `typed_array::typed_array_index_read_expr`), so its value type
        // is `Float`, not the erased fallback the class arm below would
        // report. Same rule as the concrete `.length` read above: the
        // text and the type are decided together, so a caller coerces
        // FROM the concrete value instead of treating an already
        // concrete `f64` as erased. Without this, `Number(view[i])` ran
        // the `SmeltUnknown` numeric-conversion match over an `f64`
        // scrutinee and did not compile (E0308).
        if self.is_typed_array_view_class_type(base_ty)? {
            // ...and a bigint view's element renders as a `BigInt`.
            if self.is_bigint_view_class_type(base_ty)? {
                return self.type_id(Type::BigInt);
            }
            return self.type_id(Type::Float);
        }
        if let Some(Type::Class { name, .. }) = self.mir.types.get(base_ty)
            && self.is_match_class_symbol(*name)?
        {
            // A numbered group read is an optional string.
            let string_ty = self.type_id(Type::String)?;
            return self.type_id(Type::Optional(string_ty));
        }
        match self.mir.types.get(base_ty) {
            Some(Type::List(item)) => Ok(*item),
            Some(Type::Optional(inner)) => {
                if let Some(Type::List(item)) = self.mir.types.get(*inner) {
                    return if matches!(self.mir.types.get(*item), Some(Type::Optional(_))) {
                        Ok(*item)
                    } else {
                        self.type_id(Type::Optional(*item))
                    };
                }
                self.erased_fallback_ty(|| {
                    format!(
                        "the indexed read on the optional receiver `{}`, which is not a list",
                        self.blocker_type_text(*inner)
                    )
                })
            }
            Some(Type::Dict(_, value) | Type::JsMap(_, value)) => Ok(*value),
            Some(Type::String) => self.type_id(Type::String),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => Ok(base_ty),
            Some(Type::Tuple(items)) => {
                let tuple_index = self.tuple_index(index, items.len())?;
                items
                    .get(tuple_index)
                    .copied()
                    .ok_or_else(|| EmitError::new("tuple index is out of bounds"))
            }
            _ => self.erased_fallback_ty(|| {
                format!(
                    "the indexed read on the receiver `{}`",
                    self.blocker_type_text(base_ty)
                )
            }),
        }
    }
}
