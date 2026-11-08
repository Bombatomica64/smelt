//! Host type fields: the field types of built-in classes, `RegExp` and match
//! results, and which stdlib classes have `length`/`size`.

use crate::lowering::ModuleBuilder;
use crate::lowering::support::source_spelling;
use smelt_hir::Type;

impl ModuleBuilder<'_> {
    /// Return declared fields for built-in classes that Smelt does not import from lib.d.ts.
    pub(in crate::lowering) fn builtin_class_field_type(
        &mut self,
        class: smelt_hir::Symbol,
        field: smelt_hir::Symbol,
    ) -> Option<smelt_hir::TypeId> {
        // Synthetic RegExp match-result classes carry a concrete field shape.
        // A `.groups` read yields the named-group accessor class; `.index`,
        // `.input`, and `.length` map to their primitive accessor types; and any
        // named-group read on the accessor class yields an optional string.
        // `URLSearchParams.size` is the one data property the spec defines on a
        // params value, and it is a number. Resolving it here keeps the HIR type
        // honest instead of leaving the read `Unknown` for emission to recover.
        if self.stdlib_class_of_symbol(class) == Some(smelt_stdlib::StdlibClass::UrlSearchParams)
            && self.ctx.krate.symbols.get(field) == Some("size")
        {
            return Some(self.ctx.krate.types.intern(Type::Float));
        }
        if let Some(field_ty) = self.fetch_init_field_type(class, field) {
            return Some(field_ty);
        }
        match self.match_stdlib_class(class) {
            Some(smelt_stdlib::StdlibClass::Match) => {
                let field_name = self.ctx.krate.symbols.get(field)?;
                return match field_name {
                    "index" => Some(self.ctx.krate.types.intern(Type::Float)),
                    "length" => Some(self.ctx.krate.types.intern(Type::Float)),
                    "input" => Some(self.ctx.krate.types.intern(Type::String)),
                    "groups" => Some(self.match_groups_type()),
                    // `[start, end]` per group (absent group `undefined`), or
                    // `undefined` without the `d` flag.
                    "indices" => {
                        let float_ty = self.ctx.krate.types.intern(Type::Float);
                        let span_ty = self.ctx.krate.types.intern(Type::Tuple(vec![float_ty, float_ty]));
                        let group_ty = self.ctx.krate.types.intern(Type::Optional(span_ty));
                        let list_ty = self.ctx.krate.types.intern(Type::List(group_ty));
                        Some(self.ctx.krate.types.intern(Type::Optional(list_ty)))
                    }
                    _ => None,
                };
            }
            Some(smelt_stdlib::StdlibClass::MatchGroups) => {
                // Every property read on `matchResult.groups` is a named capture
                // group, which is a string when the group participated in the
                // match and `undefined` otherwise.
                let string_ty = self.ctx.krate.types.intern(Type::String);
                return Some(self.ctx.krate.types.intern(Type::Optional(string_ty)));
            }
            _ => {}
        }
        let class_name = source_spelling(&self.ctx.krate, class)?;
        if !smelt_stdlib::is_error_class_name(class_name) {
            return None;
        }
        let field_name = self.ctx.krate.symbols.get(field)?;
        match field_name {
            "name" | "message" => Some(self.ctx.krate.types.intern(Type::String)),
            "stack" => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                Some(self.ctx.krate.types.intern(Type::Optional(string_ty)))
            }
            _ => None,
        }
    }

    /// Resolve a data-property read on a builtin `RegExp` receiver to its
    /// concrete field type, or `None` when `name` is not the `RegExp` class or
    /// the field is not a modeled `RegExp` data property.
    ///
    /// The concrete types mirror the generated `SmeltRegExp` runtime shape and
    /// codegen's `regexp_field_text`/`field_access_type`: `source`/`flags` are
    /// `String`, the boolean flag accessors are `bool`, and `lastIndex` is a
    /// `number`. Callable members (`test`, `exec`, …) return `None` so the
    /// general method-resolution paths keep handling them.
    pub(super) fn regexp_field_type_for(
        &mut self,
        name: smelt_hir::Symbol,
        field: smelt_hir::Symbol,
    ) -> Option<smelt_hir::TypeId> {
        let class_name = source_spelling(&self.ctx.krate, name)?;
        if class_name != "RegExp" {
            return None;
        }
        let field_name = self.ctx.krate.symbols.get(field)?;
        let ty = match field_name {
            "source" | "flags" => Type::String,
            "lastIndex" | "last_index" => Type::Float,
            "global" | "ignoreCase" | "ignore_case" | "multiline" | "sticky" | "unicode"
            | "dotAll" | "dot_all" | "hasIndices" | "has_indices" | "unicodeSets" | "unicode_sets" => Type::Bool,
            _ => return None,
        };
        Some(self.ctx.krate.types.intern(ty))
    }

    /// Return the HIR type for a JavaScript `RegExp` value, modeled as the
    /// builtin `RegExp` stdlib class backed by the generated `SmeltRegExp`
    /// runtime type.
    pub(in crate::lowering) fn regexp_type(&mut self) -> smelt_hir::TypeId {
        let name = self.intern_type_name("RegExp");
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })
    }

    /// The lib's `PropertyKey` as a value type: `string | number | symbol`.
    ///
    /// Each arm is a concrete value kind, so a `PropertyKey` is a generated
    /// union and never erased. It is converted to the record key type
    /// (`Type::PropKey`) only where it is used as a key.
    pub(in crate::lowering) fn property_key_value_type(&mut self) -> smelt_hir::TypeId {
        let string = self.ctx.krate.types.intern(Type::String);
        let number = self.ctx.krate.types.intern(Type::Float);
        let symbol = self.ctx.krate.types.intern(Type::Symbol);
        self.ctx.krate.types.intern(Type::Union(vec![string, number, symbol]))
    }

    /// Return the concrete HIR type for a `RegExp` match result value.
    ///
    /// `RegExp.exec` and `String.matchAll` produce a JavaScript match object
    /// with a statically known shape (numbered groups, `index`, `input`,
    /// `groups`). Modeling it as the reserved synthetic class
    /// [`smelt_stdlib::MATCH_CLASS_NAME`] lets consumer reads (`m[0]`,
    /// `m.index`, `m.input`, `m.groups.name`, destructuring) resolve to typed
    /// accessors on the generated `SmeltMatch` runtime type instead of going
    /// through the erased `SmeltUnknown` boundary.
    pub(in crate::lowering) fn match_result_type(&mut self) -> smelt_hir::TypeId {
        let name = self.intern_type_name(smelt_stdlib::MATCH_CLASS_NAME);
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })
    }

    /// Return the synthetic HIR type for `matchResult.groups`.
    ///
    /// This is the same underlying `SmeltMatch` value; a subsequent named-group
    /// read (`matchResult.groups.letter`) resolves to the typed named-group
    /// accessor via [`Self::class_field_type`].
    pub(in crate::lowering) fn match_groups_type(&mut self) -> smelt_hir::TypeId {
        let name = self.intern_type_name(smelt_stdlib::MATCH_GROUPS_CLASS_NAME);
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })
    }

    /// Resolve a class symbol to its shared stdlib class identity, if any.
    pub(in crate::lowering) fn stdlib_class_of_symbol(
        &self,
        name: smelt_hir::Symbol,
    ) -> Option<smelt_stdlib::StdlibClass> {
        let class_name = source_spelling(&self.ctx.krate, name)?;
        smelt_stdlib::typescript_stdlib_class(class_name)
    }

    /// Return the stdlib class a type name denotes, keeping only the MATCH pair.
    ///
    /// Narrower than the general lookup above: a caller that only cares whether
    /// a name is a regex match result (or its groups record) gets `None` for
    /// every other stdlib class, so it need not re-filter.
    pub(in crate::lowering) fn match_stdlib_class(
        &self,
        name: smelt_hir::Symbol,
    ) -> Option<smelt_stdlib::StdlibClass> {
        let class_name = source_spelling(&self.ctx.krate, name)?;
        match smelt_stdlib::typescript_stdlib_class(class_name) {
            class @ Some(
                smelt_stdlib::StdlibClass::Match | smelt_stdlib::StdlibClass::MatchGroups,
            ) => class,
            _ => None,
        }
    }

    /// Whether `ty` is an object shape the source declares: a record or object
    /// type, or a class or interface of this crate. Its members are its own
    /// properties, never a collection's built-in ones.
    fn is_source_object_shape(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Dict(_, _)) => true,
            Some(Type::Class { name, .. }) => self
                .ctx
                .krate
                .symbols
                .get(*name)
                .is_some_and(|name| self.classes.contains(name) || self.interfaces.contains(name)),
            _ => false,
        }
    }

    /// Whether reading `member` off a value of `receiver_ty` is that value's
    /// ELEMENT COUNT, lowered to `ExprKind::Len`.
    ///
    /// Decided by the receiver's TYPE, never by the member name alone: `length`
    /// counts an array, string, tuple or host collection, and `size` counts a
    /// `Map` (`JsMap`) or `Set`. On anything else the member is an ordinary
    /// property: an object constant or `Record` whose source declares a `size`
    /// or `length` field (`DEFAULTS.size`) reads that field, which used to
    /// lower to the record's entry count because records and maps share the
    /// `Dict` shape.
    ///
    /// A type parameter keeps the count reading (a source that type-checks
    /// only reads `length`/`size` off it when every instantiation has one: an
    /// array, a string, a function's arity, a `Map`) unless its constraint is
    /// a source object shape that declares the member as a property
    /// (`T extends { size: number }`).
    pub(in crate::lowering) fn element_count_member(&self, receiver_ty: smelt_hir::TypeId, member: &str) -> bool {
        if matches!(self.ctx.krate.types.get(receiver_ty), Some(Type::TypeParam { .. })) {
            let constraint = self.type_param_constraint_or_self(receiver_ty);
            return matches!(member, "length" | "size") && !self.is_source_object_shape(constraint);
        }
        match (member, self.ctx.krate.types.get(receiver_ty)) {
            ("length", Some(Type::List(_) | Type::String | Type::Tuple(_))) => true,
            ("length", Some(Type::Class { .. })) => !self.is_source_object_shape(receiver_ty),
            ("size", Some(Type::JsMap(_, _) | Type::Set(_))) => true,
            _ => false,
        }
    }
}
