//! Map emission helpers.
//!
//! Several projections here inline the same own-string-key predicate:
//! `__smelt_symbol:` names a symbol-keyed property (never a string key),
//! `__smelt_proto:` / `__smelt_method:` carry inherited members, and
//! `__smelt_class` records which prototype an instance came from. JavaScript
//! exposes none of the four as an own string-keyed property, so `Object.keys` /
//! `values` / `entries` must skip every one of them. Keep the list in sync with
//! `smelt_is_for_in_object_key` and `smelt_is_own_structural_key` in the runtime
//! prelude when a marker is added.

use crate::rust::RustExpr;
use super::*;

impl FunctionEmitter<'_> {
    /// Converts a dictionary key containment operation to Rust text.
    pub(super) fn dict_contains_key_expr(
        &self,
        dict: &Operand,
        key: &Operand,
        lookup: PropertyLookup,
    ) -> Result<RustExpr, EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        // A `"field" in value` test over a concrete union projects to a static
        // discriminant check instead of a runtime object lookup. The frontend
        // keeps such receivers at their union type precisely so this fast path
        // can fire; `concrete_union_field_check` returns `None` when the union is
        // not fully concrete, leaving the erased fallback below to run.
        if matches!(self.mir.types.get(dict_ty), Some(Type::Union(_)))
            && let Some(field) = self.operand_string_literal(key)
            && let Some(check) =
                self.concrete_union_field_check(&self.operand_expr(dict)?, dict_ty, &field)
        {
            return Ok(RustExpr::from(check));
        }
        // The one-arm case: a concrete (non-erased) class instance answers a
        // literal key from its declared instance surface, the same rule a
        // union arm uses. An erased class is a runtime object and keeps the
        // live lookup below.
        if matches!(self.mir.types.get(dict_ty), Some(Type::Class { .. }))
            && !self.is_erased_class_type(dict_ty)
            && let Some(field) = self.operand_string_literal(key)
        {
            return Ok(RustExpr::path(self.type_has_property_statically(dict_ty, &field).to_string()));
        }
        let Some(Type::Dict(key_ty, _) | Type::JsMap(key_ty, _)) = self.mir.types.get(dict_ty)
        else {
            if self.dict_contains_key_uses_erased_object(dict_ty) {
                // Read by borrow: the emitted helper takes its receiver by
                // reference, so cloning here copied the whole record for a
                // membership test.
                let dict_text = self.operand_borrow_expr(dict)?;
                let key_text = self.operand_expr(key)?;
                let key_value = match self.mir.types.get(self.operand_ty(key)?) {
                    Some(Type::String) => key_text,
                    Some(Type::Bool) => RustExpr::postfix_on(&key_text, format!("{key_text}.to_string()")),
                    // The same number rule as every other property key.
                    Some(Type::Int | Type::Float) => {
                        self.property_key_to_string_expr(&key_text, self.operand_ty(key)?)?
                    }
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => {
                        self.property_key_to_string_expr(&key_text, self.operand_ty(key)?)?
                    }
                    _ => return Ok(RustExpr::path("false")),
                };
                // The two JavaScript presence tests over an erased value are two
                // different helpers, not one: `Object.hasOwn` stops at the
                // value's own properties while `in` continues onto the prototype
                // chain, so `Object.hasOwn({}, 'toString')` is `false` where
                // `'toString' in {}` is `true`. Emitting a single disjunction for
                // both spellings made whichever reach it picked wrong for the
                // other -- and before `Rvalue::DictContainsKey` carried `lookup`
                // there was nothing here to pick with. Both helpers live in the
                // prelude beside the property-READ chain they must agree with,
                // which is what keeps a synthesized own property (a boxed
                // string's `length` and character indices, a byte view's
                // elements) visible to the presence tests and not only to reads.
                let helper = match lookup {
                    PropertyLookup::Own => "smelt_has_own_property",
                    PropertyLookup::PrototypeChain => "smelt_has_property",
                };
                return Ok(RustExpr::chain(format!("{helper}(&({dict_text}), &({key_value}))")));
            }
            return Ok(RustExpr::path("false"));
        };
        // A concrete key flowing into a `SmeltUnknown`-keyed Map (`key_ty`
        // erased) must be erased to its runtime `SmeltUnknown` form here, exactly
        // as `set`/`get` do via `dict_key_operand_expr`. Comparing the concrete
        // key type against the erased `key_ty` for exact equality would fold this
        // to the literal `false`, silently corrupting the lookup for object keys.
        if !self.dict_key_operand_is_compatible(key, *key_ty)? {
            return Ok(RustExpr::path("false"));
        }
        // `contains_key` takes `&self`, so read the receiver by borrow. Cloning it
        // copied the whole map for a membership test; in es-toolkit's `groupBy` that
        // ran once per element (`result.clone().contains_key(..)` inside the loop),
        // making an O(n) pass O(n^2) in the accumulated map.
        Ok({ let receiver = self.operand_borrow_expr(dict)?; RustExpr::postfix_on(&receiver, format!("{receiver}.contains_key({})", self.dict_key_reference_expr(key, *key_ty)?)) })
    }

    /// Whether a string-keyed dict's values are the erased dynamic type.
    ///
    /// The byte-buffer record helpers are defined over
    /// `SmeltRecord<String, SmeltUnknown>` only — that is the sole shape a
    /// marker-bearing host record can arrive as through the structural ABI — so the
    /// projections consult this before delegating to them, and keep their ordinary
    /// key/value filters for every concretely-typed record.
    fn dict_holds_erased_values(&self, ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(ty),
            Some(Type::Dict(_, value_ty) | Type::JsMap(_, value_ty))
                if self.mir.types.get(*value_ty) == Some(&Type::Unknown)
        )
    }

    /// Return the string value when an operand is a constant string literal.
    ///
    /// Used to recognize a static discriminant key in `"field" in value` so the
    /// concrete-union field check can be emitted from a literal property name.
    fn operand_string_literal(&self, operand: &Operand) -> Option<String> {
        match operand {
            Operand::Const(Constant::String(value)) => Some(value.clone()),
            _ => None,
        }
    }

    /// Return whether a dictionary containment check must inspect an erased
    /// JavaScript object value at runtime.
    ///
    /// Class-shaped types without a local declaration (ambient interfaces such
    /// as `IArguments`) are represented as `SmeltUnknown` at runtime — see
    /// [`Self::is_erased_class_type`] — so their containment checks go through
    /// the same live-object inspection as `unknown` values.
    fn dict_contains_key_uses_erased_object(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Unknown | Type::Union(_)) => true,
            Some(Type::TypeParam { name }) => !self.current_function_has_type_param(*name),
            Some(Type::Class { .. }) => self.is_erased_class_type(ty),
            _ => false,
        }
    }

    /// Converts a dictionary get operation to Rust text.
    pub(super) fn dict_get_expr(
        &self,
        dict: &Operand,
        key: &Operand,
        default: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        let Some(Type::Dict(key_ty, value_ty) | Type::JsMap(key_ty, value_ty)) =
            self.mir.types.get(dict_ty)
        else {
            return Err(EmitError::new("dict get receiver must be a dict"));
        };
        if !self.dict_key_operand_is_compatible(key, *key_ty)? {
            return Err(EmitError::new("dict key does not fit the receiver"));
        }
        // Every `get` below hands the key to a `&K` parameter and keeps nothing,
        // so borrow the place where no coercion is needed.
        let key_text = self.dict_key_reference_expr(key, *key_ty)?;
        if let Some(default_operand) = default {
            if self.operand_ty(default_operand)? != *value_ty {
                return Err(EmitError::new(
                    "dict get default must match the dict value type",
                ));
            }
            if dest_ty != *value_ty {
                return Err(EmitError::new(
                    "dict get destination with default must match the dict value type",
                ));
            }
            let get_text =
                if self.map_op_uses_smelt_record(self.operand_ty(dict)?, *key_ty)
                    || self.map_op_uses_js_key_map(self.operand_ty(dict)?, *key_ty) {
                    RustExpr::postfix_on(&self.operand_borrow_expr(dict)?, format!("{}.get({})", self.operand_borrow_expr(dict)?, key_text))
                } else {
                    RustExpr::postfix_on(&self.operand_borrow_expr(dict)?, format!("{}.get({}).cloned()", self.operand_borrow_expr(dict)?, key_text))
                };
            return Ok(RustExpr::postfix_on(&get_text, format!(
                "{get_text}.unwrap_or({})",
                self.operand_expr(default_operand)?
            )));
        }
        if dest_ty == *value_ty
            && self.mir.types.get(*key_ty) == Some(&Type::String)
            && self.renders_erased(*value_ty)?
        {
            // An absent key reads as `undefined`, matching JavaScript's
            // dynamic property semantics; `null` is a value only a store puts
            // there, and the two differ under `===`.
            return Ok(RustExpr::postfix_on(&self.operand_expr(dict)?, self.carrier_format(format_args!(
                "{}.get({}).unwrap_or(SmeltUnknown::Undefined)",
                self.operand_expr(dict)?,
                key_text
            ))));
        }
        match (self.mir.types.get(*value_ty), self.mir.types.get(dest_ty)) {
            (Some(Type::Optional(value_inner)), Some(Type::Optional(dest_inner)))
                if value_inner == dest_inner =>
            {
                let get_text =
                    if self.map_op_uses_smelt_record(self.operand_ty(dict)?, *key_ty)
                    || self.map_op_uses_js_key_map(self.operand_ty(dict)?, *key_ty) {
                        RustExpr::postfix_on(&self.operand_borrow_expr(dict)?, format!("{}.get({})", self.operand_borrow_expr(dict)?, key_text))
                    } else {
                        RustExpr::postfix_on(&self.operand_borrow_expr(dict)?, format!("{}.get({}).cloned()", self.operand_borrow_expr(dict)?, key_text))
                    };
                Ok(RustExpr::postfix_on(&get_text, format!("{get_text}.flatten()")))
            }
            (_, Some(Type::Optional(dest_inner))) if dest_inner == value_ty => {
                if self.map_op_uses_smelt_record(self.operand_ty(dict)?, *key_ty)
                    || self.map_op_uses_js_key_map(self.operand_ty(dict)?, *key_ty) {
                    Ok({ let receiver = self.operand_borrow_expr(dict)?; RustExpr::postfix_on(&receiver, format!("{receiver}.get({})", key_text)) })
                } else {
                    Ok({ let receiver = self.operand_expr(dict)?; RustExpr::postfix_on(&receiver, format!("{receiver}.get({}).cloned()", key_text)) })
                }
            }
            _ => Err(EmitError::new(
                "dict get destination without default must be optional value",
            )),
        }
    }

    /// Converts a dictionary setdefault operation to Rust text.
    ///
    /// This mapping supports the explicit-default form only, so generated Rust
    /// can preserve the dictionary value type without inventing a `None`
    /// default for non-optional values.
    pub(super) fn dict_setdefault_expr(
        &self,
        dict: &Operand,
        key: &Operand,
        default: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        let Some(Type::Dict(key_ty, value_ty) | Type::JsMap(key_ty, value_ty)) =
            self.mir.types.get(dict_ty)
        else {
            return Err(EmitError::new("dict setdefault receiver must be a dict"));
        };
        if self.operand_ty(key)? != *key_ty {
            return Err(EmitError::new(
                "dict setdefault key must match the dict key type",
            ));
        }
        if self.operand_ty(default)? != *value_ty {
            return Err(EmitError::new(
                "dict setdefault default must match the dict value type",
            ));
        }
        if dest_ty != *value_ty {
            return Err(EmitError::new(
                "dict setdefault destination must match the dict value type",
            ));
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = dict else {
            return Err(EmitError::new(
                "dict setdefault receiver must be a mutable local for now",
            ));
        };
        let dict_text = self.local_mut_value(*local)?;
        let key_text = self.operand_expr(key)?;
        let default_text = self.operand_expr(default)?;
        if self.map_op_uses_smelt_record(self.operand_ty(dict)?, *key_ty)
                    || self.map_op_uses_js_key_map(self.operand_ty(dict)?, *key_ty) {
            Ok(RustExpr::atom(format!(
                "{{ if let Some(value) = {dict_text}.get(&{key_text}) {{ value }} else {{ {dict_text}.insert({key_text}, {default_text}.clone()); {default_text} }} }}"
            )))
        } else {
            Ok(RustExpr::atom(format!(
                "{{ {dict_text}.entry({key_text}).or_insert({default_text}).clone() }}"
            )))
        }
    }

    /// Converts a dictionary insertion operation to Rust text.
    pub(super) fn dict_set_expr(
        &self,
        dict: &Operand,
        key: &Operand,
        value: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        let Some(Type::Dict(key_ty, value_ty) | Type::JsMap(key_ty, value_ty)) =
            self.mir.types.get(dict_ty)
        else {
            return Err(EmitError::new("dict merge receiver must be a dict"));
        };
        if !self.dict_key_operand_is_compatible(key, *key_ty)? {
            return Ok(self.operand_expr(dict)?);
        }
        if !self.dict_value_operand_is_compatible(value, *value_ty)? {
            return Ok(self.operand_expr(dict)?);
        }
        if dest_ty != dict_ty {
            return Err(EmitError::new(
                "dict set destination must match the receiver dict type",
            ));
        }
        // The receiver may be a plain local or a place rooted in one (a class
        // field holding a `Map`, e.g. `this.__data.set(k, v)`). Render the
        // assignable lvalue for the place so the insertion mutates the stored
        // dict in place rather than a temporary copy.
        let (Operand::Copy(dict_place) | Operand::Move(dict_place)) = dict else {
            return Err(EmitError::new(
                "dict set receiver must be a place operand",
            ));
        };
        let dict_text = self.assignment_place_expr(dict_place)?;
        let key_text = self.dict_key_operand_expr(key, *key_ty)?;
        let value_text = self.value_at_type(value, *value_ty)?;
        Ok(RustExpr::atom(format!(
            "{{ {dict_text}.insert({key_text}, {value_text}); {dict_text}.clone() }}"
        )))
    }

    /// Return whether a key operand can be used for a dictionary operation.
    fn dict_key_operand_is_compatible(
        &self,
        key: &Operand,
        key_ty: TypeId,
    ) -> Result<bool, EmitError> {
        let operand_ty = self.operand_ty(key)?;
        if operand_ty == key_ty {
            return Ok(true);
        }
        // A `SmeltUnknown`-keyed Map (`Map<object, _>`, `Map<unknown, _>`) accepts
        // any concrete key: the key is erased to its runtime `SmeltUnknown` form
        // at the operation site by `dict_key_operand_expr`, the same erasure the
        // correctly-lowered `Map<unknown, _>` path already relies on. Without
        // this, a concrete object key (`const a = { v: 1 }` used as a Map key)
        // reads as incompatible and the whole operation is folded to a constant
        // default — dropping inserts and returning `false`/`None` at runtime.
        if self.key_ty_is_erased(key_ty) {
            return Ok(true);
        }
        Ok(matches!(
            self.mir.types.get(operand_ty),
            Some(Type::Unknown | Type::TypeParam { .. })
        ) || matches!(self.mir.types.get(operand_ty), Some(Type::Optional(inner)) if *inner == key_ty))
    }

    /// Return whether a value operand can be stored into a dict/Map value slot.
    ///
    /// Mirrors [`Self::dict_key_operand_is_compatible`] for the value position.
    /// Crucially, this must not silently reject a storable value: a rejection
    /// here folds the whole `map.set(k, v)` to a plain read of the receiver
    /// (`dict_set_expr`'s bail arm), which drops the insert — an observable
    /// side effect — from the generated code even though `map.set` was written
    /// in statement position. An erased value slot (`Map<_, any>`,
    /// `Map<_, unknown>`, erased union/class) accepts any concrete value because
    /// [`Self::value_at_type`] erases it into `SmeltUnknown` at the store site;
    /// without this a `Map<any, any>` insert of a concrete number/string is
    /// dropped and both maps compare empty at runtime (es-toolkit `isEqualWith`
    /// map comparisons). Coercible operand shapes (`Unknown`/`TypeParam`
    /// sources, an optional wrapping the exact slot type, a concrete-membered
    /// union target) are likewise accepted since `value_at_type` materializes
    /// them. A genuinely irreconcilable concrete-to-concrete mismatch still
    /// bails to keep the generated Rust well-typed.
    fn dict_value_operand_is_compatible(
        &self,
        value: &Operand,
        value_ty: TypeId,
    ) -> Result<bool, EmitError> {
        let operand_ty = self.operand_ty(value)?;
        if operand_ty == value_ty {
            return Ok(true);
        }
        if self.key_ty_is_erased(value_ty) {
            return Ok(true);
        }
        if self.concrete_union_members(value_ty).is_some() {
            return Ok(true);
        }
        Ok(matches!(
            self.mir.types.get(operand_ty),
            Some(Type::Unknown | Type::TypeParam { .. })
        ) || matches!(self.mir.types.get(operand_ty), Some(Type::Optional(inner)) if *inner == value_ty))
    }

    /// Return whether a dict/Map key type is erased to `SmeltUnknown` at runtime.
    ///
    /// When it is, a concrete key operand must be erased into it (via
    /// [`Self::dict_key_operand_expr`]) rather than matched by exact type. This
    /// mirrors the erased-target test in [`Self::value_at_type`]: bare `unknown`,
    /// a non-concrete union, an out-of-scope type parameter, or an erased class
    /// (ambient interface) all lower to `SmeltUnknown`. A union with fully
    /// concrete members is deliberately excluded so its keys keep exact typing.
    fn key_ty_is_erased(&self, key_ty: TypeId) -> bool {
        (matches!(
            self.mir.types.get(key_ty),
            Some(Type::Unknown | Type::Union(_))
        ) && self.concrete_union_members(key_ty).is_none())
            || matches!(self.mir.types.get(key_ty), Some(Type::TypeParam { name }) if !self.current_function_has_type_param(*name))
            || self.is_erased_class_type(key_ty)
    }

    /// Render a dictionary key, unwrapping optional keys after frontend narrowing.
    fn dict_key_operand_expr(&self, key: &Operand, key_ty: TypeId) -> Result<RustExpr, EmitError> {
        let operand_ty = self.operand_ty(key)?;
        if matches!(self.mir.types.get(operand_ty), Some(Type::Optional(inner)) if *inner == key_ty)
        {
            let key_text = self.operand_expr(key)?;
            let default = self.default_value(key_ty)?;
            return Ok(RustExpr::postfix_on(&key_text, format!("{key_text}.clone().unwrap_or({default})")));
        }
        if operand_ty == key_ty {
            return self.value_at_type(key, key_ty);
        }
        if self.mir.types.get(key_ty) == Some(&Type::String) {
            let key_text = self.operand_expr(key)?;
            return self.property_key_to_string_expr(&key_text, operand_ty);
        }
        self.value_at_type(key, key_ty)
    }

    /// Render a dictionary key for an argument position spelled `&K`.
    ///
    /// `remove`/`get`/`contains_key` only read the key, so where no coercion is
    /// needed the caller hands over the place rather than a copy of it. Every
    /// other key shape — an optional unwrapped after narrowing, a property key
    /// coerced to `String` — genuinely builds a value, and a borrow of that
    /// temporary lives to the end of the statement.
    pub(super) fn dict_key_reference_expr(&self, key: &Operand, key_ty: TypeId) -> Result<RustExpr, EmitError> {
        if self.operand_ty(key)? == key_ty {
            return self.shared_reference_argument_expr(key, key_ty);
        }
        Ok(RustExpr::loose(format!("&({})", self.dict_key_operand_expr(key, key_ty)?)))
    }

    /// Converts a dictionary key removal operation to Rust text.
    pub(super) fn dict_remove_key_expr(
        &self,
        dict: &Operand,
        key: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        let Some(Type::Dict(key_ty, _) | Type::JsMap(key_ty, _)) = self.mir.types.get(dict_ty)
        else {
            if matches!(
                self.mir.types.get(dict_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Never | Type::Union(_))
            ) || self.is_erased_class_type(dict_ty)
            {
                if !matches!(self.mir.types.get(dest_ty), Some(Type::Bool)) {
                    return Err(EmitError::new("dict remove destination must be bool"));
                }
                let rendered_key = self.operand_expr(key)?;
                let key_text =
                    self.property_key_to_string_expr(&rendered_key, self.operand_ty(key)?)?;
                if let Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) =
                    dict
                {
                    return Ok(RustExpr::atom(self.carrier_format(format_args!(
                        "match &mut {} {{ SmeltUnknown::Object(map) => map.remove(&{key_text}).is_some(), _ => true }}",
                        self.local_mut_value(*local)?
                    ))));
                }
                return Ok(RustExpr::path("true"));
            }
            // JavaScript `delete list[i]` punches a hole while preserving
            // length. Smelt lists are dense `Vec<T>` with no hole
            // representation, so the delete lowers to a successful no-op —
            // the same explicit deferral style as no-op list `length` growth.
            // Subsequent reads observe the retained element instead of a hole.
            if matches!(self.mir.types.get(dict_ty), Some(Type::List(_) | Type::Tuple(_))) {
                if !matches!(self.mir.types.get(dest_ty), Some(Type::Bool)) {
                    return Err(EmitError::new("dict remove destination must be bool"));
                }
                return Ok(RustExpr::path("true"));
            }
            return Err(EmitError::new(format!(
                "dict remove receiver must be a dict, got {}",
                Self::type_text_for(self.mir, dict_ty)
                    .unwrap_or_else(|_error| format!("{dict_ty:?}"))
            )));
        };
        // Coerce a dynamically-typed (`Unknown`/optional/union) key to the dict's
        // key type instead of dropping the removal — mirrors `dict_set_expr`.
        // `delete out[key]` lowers here with `key` erased even though `out` is
        // keyed by `String` (omit) or a union (intersection's `Map.delete`).
        if !self.dict_key_operand_is_compatible(key, *key_ty)? {
            return Ok(RustExpr::path("false"));
        }
        if !matches!(self.mir.types.get(dest_ty), Some(Type::Bool)) {
            return Err(EmitError::new("dict remove destination must be bool"));
        }
        // Accept a place-rooted receiver (a class field holding a `Map`, e.g.
        // `this.__data.delete(k)`) as well as a plain local, rendering the
        // assignable lvalue so the removal mutates the stored dict in place.
        let (Operand::Copy(dict_place) | Operand::Move(dict_place)) = dict else {
            return Err(EmitError::new(
                "dict remove receiver must be a place operand",
            ));
        };
        Ok({ let receiver = self.assignment_place_expr(dict_place)?; RustExpr::postfix_on(&receiver, format!("{receiver}.remove({}).is_some()", self.dict_key_reference_expr(key, *key_ty)?)) })
    }

    /// Converts a dictionary pop operation to Rust text.
    pub(super) fn dict_pop_expr(
        &self,
        dict: &Operand,
        key: &Operand,
        default: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        let Some(Type::Dict(key_ty, value_ty) | Type::JsMap(key_ty, value_ty)) =
            self.mir.types.get(dict_ty)
        else {
            return Err(EmitError::new("dict pop receiver must be a dict"));
        };
        if self.operand_ty(key)? != *key_ty {
            return Err(EmitError::new("dict pop key must match the dict key type"));
        }
        if dest_ty != *value_ty {
            return Err(EmitError::new(
                "dict pop destination must match the dict value type",
            ));
        }
        if let Some(default_operand) = default
            && self.operand_ty(default_operand)? != *value_ty
        {
            return Err(EmitError::new(
                "dict pop default must match the dict value type",
            ));
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = dict else {
            return Err(EmitError::new(
                "dict pop receiver must be a mutable local for now",
            ));
        };
        let dict_text = self.local_mut_value(*local)?;
        let key_text = self.operand_expr(key)?;
        if let Some(default_operand) = default {
            let default_text = self.operand_expr(default_operand)?;
            Ok(RustExpr::postfix_on(&dict_text, format!(
                "{dict_text}.remove(&{key_text}).unwrap_or({default_text})"
            )))
        } else {
            Ok(RustExpr::postfix_on(&dict_text, format!(
                "{dict_text}.remove(&{key_text}).expect(\"dict pop missing key\")"
            )))
        }
    }

    /// Converts a dictionary update operation to Rust text.
    pub(super) fn dict_update_expr(
        &self,
        dict: &Operand,
        other: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        if !matches!(self.mir.types.get(dict_ty), Some(Type::Dict(_, _))) {
            return Err(EmitError::new("dict update receiver must be a dict"));
        }
        if self.operand_ty(other)? != dict_ty {
            return Err(EmitError::new(
                "dict update argument must match the receiver dict type",
            ));
        }
        if !matches!(self.mir.types.get(dest_ty), Some(Type::None)) {
            return Err(EmitError::new("dict update destination must be None"));
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = dict else {
            return Err(EmitError::new(
                "dict update receiver must be a mutable local for now",
            ));
        };
        let dict_text = self.local_mut_value(*local)?;
        let other_text = self.operand_expr(other)?;
        Ok(RustExpr::atom(format!(
            "{{ {dict_text}.extend({other_text}.iter().map(|(key, value)| (key.clone(), value.clone()))); () }}"
        )))
    }

    /// Emit `Object.assign(target, ...sources)` onto an ERASED target.
    ///
    /// The target is a shared erased object, so each source (erased at this
    /// boundary) is copied onto the SAME object by the runtime helper and the
    /// call evaluates to that object, as JavaScript's does. The frontend only
    /// builds this shape for a target whose static type is `unknown` — the
    /// global object above all — where there is no typed record to merge into.
    fn unknown_assign_expr(
        &self,
        target: &Operand,
        sources: &[Operand],
    ) -> Result<RustExpr, EmitError> {
        let mut steps = Vec::with_capacity(sources.len());
        for source in sources {
            steps.push(format!(
                "{}(&mut smelt_target, &{});",
                crate::thrown::UNKNOWN_ASSIGN_FN,
                self.erase(source)?
            ));
        }
        Ok(RustExpr::atom(format!(
            "{{ let mut smelt_target = {}; {} smelt_target }}",
            self.operand_expr(target)?,
            steps.join(" ")
        )))
    }

    /// Converts a dictionary assign operation to Rust text.
    pub(super) fn dict_assign_expr(
        &self,
        target: &Operand,
        sources: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let target_ty = self.operand_ty(target)?;
        if matches!(self.mir.types.get(target_ty), Some(Type::Unknown)) {
            return self.unknown_assign_expr(target, sources);
        }
        if !matches!(self.mir.types.get(target_ty), Some(Type::Dict(_, _))) {
            return Err(EmitError::new("dict assign target must be a dict"));
        }
        if dest_ty != target_ty {
            return Err(EmitError::new(
                "dict assign destination must match the target dict type",
            ));
        }
        // JavaScript `Object.assign(target, ...sources)` mutates `target` in
        // place and returns it. When `target` is a mutable local, extend that
        // local directly so the caller's binding observes the merge (a plain
        // discarded `Object.assign(result, src)` must still update `result`),
        // then evaluate to the mutated dict. When `target` is not a place
        // (e.g. `Object.assign({}, a, b)`), fall back to merging into a fresh
        // clone since there is nothing to write back.
        let target_place = match target {
            Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => Some(*local),
            _ => None,
        };
        let (accumulator, mut steps, final_value) = if let Some(local) = target_place {
            let target_text = self.local_mut_value(local)?;
            let final_value = format!("{target_text}.clone()");
            (target_text.into_string(), Vec::new(), final_value)
        } else {
            let target_text = self.operand_expr(target)?;
            (
                "assigned".to_owned(),
                vec![format!("let mut assigned = {target_text}.clone();")],
                "assigned".to_owned(),
            )
        };
        for source in sources {
            let source_ty = self.operand_ty(source)?;
            let source_text = if source_ty == target_ty {
                self.operand_expr(source)?
            } else if !matches!(self.mir.types.get(source_ty), Some(Type::Dict(_, _))) {
                self.object_spread_unknown_source_expr(source, target_ty)?
            } else if let Some(converted) = self.dict_spread_value_conversion_expr(source, target_ty)? {
                // A record whose VALUE type differs from the target's (a
                // `{ weight: number }` spread into an erased merge) is still
                // copied, each value converted to the target value type. It
                // used to be skipped, silently losing every field it carried.
                steps.push(format!("{accumulator}.extend({converted});"));
                continue;
            } else {
                continue;
            };
            steps.push(format!(
                "{accumulator}.extend({source_text}.iter().map(|(key, value)| (key.clone(), value.clone())));"
            ));
        }
        steps.push(final_value);
        Ok(RustExpr::atom(format!("{{ {} }}", steps.join(" "))))
    }

    /// Converts a string-keyed record source into `(key, value)` pairs of the
    /// target record's value type, for an object spread whose source and
    /// target records differ only in value type.
    ///
    /// Returns `None` when either side is not a string-keyed `Dict`, leaving
    /// the caller's other arms to decide.
    fn dict_spread_value_conversion_expr(
        &self,
        source: &Operand,
        target_ty: TypeId,
    ) -> Result<Option<RustExpr>, EmitError> {
        let source_ty = self.operand_ty(source)?;
        let (Some(Type::Dict(source_key, source_value)), Some(Type::Dict(target_key, target_value))) =
            (self.mir.types.get(source_ty), self.mir.types.get(target_ty))
        else {
            return Ok(None);
        };
        if self.mir.types.get(*source_key) != Some(&Type::String)
            || self.mir.types.get(*target_key) != Some(&Type::String)
        {
            return Ok(None);
        }
        let value_text =
            self.rendered_at_type(&RustExpr::chain("value.clone()").ending_in_clone(), *source_value, *target_value, &self.render_scope())?;
        Ok(Some({ let receiver = self.operand_expr(source)?; RustExpr::postfix_on(&receiver, format!("{receiver}.iter().map(|(key, value)| (key.clone(), {value_text}))")) }))
    }

    /// Converts an unknown object-spread source into the typed record target.
    ///
    /// JavaScript object spread copies enumerable object fields at runtime even
    /// when the source is statically opaque. The resulting object can still
    /// have a narrower static surface from later explicit properties, so each
    /// copied value is converted to the target record value type as it is
    /// inserted.
    fn object_spread_unknown_source_expr(
        &self,
        source: &Operand,
        target_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let Some(Type::Dict(key_ty, value_ty)) = self.mir.types.get(target_ty) else {
            return self.rendered_at_type(
                &self.operand_expr(source)?,
                self.operand_ty(source)?,
                target_ty,
                &self.render_scope(),
            );
        };
        if self.mir.types.get(*key_ty) != Some(&Type::String) {
            return self.rendered_at_type(
                &self.operand_expr(source)?,
                self.operand_ty(source)?,
                target_ty,
                &self.render_scope(),
            );
        }
        let value_text = self.extract_rendered(&RustExpr::path("value"), *value_ty, &self.render_scope())?;
        let source_text = self.operand_expr(source)?;
        // The spread-merge match inspects a dynamic `SmeltUnknown::Object`, so a
        // source that still carries a concrete static shape (a typed options
        // struct, an `Option<Struct>`, a type parameter, etc.) must first be
        // erased to `SmeltUnknown` through its boundary adapter. Sources already
        // typed as `unknown` are left untouched so the common dynamic spread
        // keeps its previous byte-identical emission.
        let source_ty = self.operand_ty(source)?;
        let scrutinee_text = if matches!(self.mir.types.get(source_ty), Some(Type::Unknown)) {
            format!("{source_text}.clone()")
        } else {
            self.carrier_format(format_args!("IntoSmeltUnknown::into_smelt_unknown({source_text}.clone())"))
        };
        Ok(RustExpr::atom(self.carrier_format(format_args!(
            "match {scrutinee_text} {{ SmeltUnknown::Object(map) => SmeltRecord::with_id_from_entries(map.id, map.into_iter().map(|(key, value)| (key, {value_text}))), _ => SmeltRecord::new() }}"
        ))))
    }

    /// Converts a dictionary copy operation to Rust text.
    pub(super) fn dict_copy_expr(
        &self,
        dict: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let dict_ty = self.operand_ty(dict)?;
        if !matches!(self.mir.types.get(dict_ty), Some(Type::Dict(_, _))) {
            // An object rest pattern (`const { a, ...rest } = source`) lowers the
            // `...rest` copy to `DictCopy` of the source. When the source is not a
            // native `Dict` but an object-like value that erases to `SmeltUnknown`
            // — a bare `unknown`, a type parameter, a union, or a named object
            // type / interface / type alias such as a library's `Funnel<Args>` —
            // route the copy through its erased `SmeltUnknown::Object` form so the
            // remaining fields survive. Otherwise the rest object would be empty
            // and any spread-out members (e.g. `cancel`/`flush`) would be lost.
            //
            // A NOMINAL record (an interface or object-alias struct, a class)
            // takes the same route: its `IntoSmeltUnknown` view is the object
            // of its own enumerable properties, with an absent optional field
            // left out, which is exactly what `...rest` copies. Without it the
            // rest of `const { strict, ...rest } = options` was empty once
            // `type Options = { .. }` became a struct.
            let source_is_unknown_like =
                self.renders_erased(dict_ty)?;
            let source_is_record = matches!(self.mir.types.get(dict_ty), Some(Type::Class { .. }));
            if (source_is_unknown_like || source_is_record)
                && matches!(
                    self.mir.types.get(dest_ty),
                    Some(Type::Dict(key_ty, value_ty))
                        if self.mir.types.get(*key_ty) == Some(&Type::String)
                            && self.mir.types.get(*value_ty) == Some(&Type::Unknown)
                )
            {
                // Read by borrow: the emitted expression appends its own
                // `.clone()`, so cloning here made it `x.clone().clone()`.
                let dict_text = self.operand_borrow_expr(dict)?;
                // A nominal record erases through the emitter's own field-wise
                // object builder, not its generic `IntoSmeltUnknown` impl: that
                // impl has no ABI for a callable field and renders it `null`,
                // so `const { strict, ...rest } = options` dropped Hono's
                // `options.router` methods and every route `app.route(..)`
                // mounted was added to a router that answered nothing.
                let erased = if source_is_record {
                    self.erase_rendered(&RustExpr::postfix_on(&dict_text, format!("{dict_text}.clone()")).ending_in_clone(), dict_ty)?
                } else {
                    RustExpr::atom(self.carrier_format(format_args!("({dict_text}.clone()).into_smelt_unknown()")))
                };
                return Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "match {erased} {{ SmeltUnknown::Object(map) => SmeltRecord::with_id_from_entries(map.id, map.into_iter()), _ => Default::default() }}"
                ))));
            }
            return Err(EmitError::new("dict copy receiver must be a dict"));
        }
        if dest_ty != dict_ty {
            return Err(EmitError::new(
                "dict copy destination must match the receiver dict type",
            ));
        }
        // The copy itself is the clone; reading by borrow keeps it at one.
        Ok({ let receiver = self.operand_borrow_expr(dict)?; RustExpr::postfix_on(&receiver, format!("{receiver}.clone()")) }.ending_in_clone())
    }

    /// Converts a dictionary projection operation to Rust text.
    pub(super) fn dict_projection_expr(
        &self,
        op: smelt_hir::DictProjectionOp,
        dict: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let dict_text = self.operand_expr(dict)?;
        if matches!(
            self.mir.types.get(self.operand_ty(dict)?),
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
        ) {
            return match op {
                smelt_hir::DictProjectionOp::FromEntries => Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "match {dict_text} {{ SmeltUnknown::Array(entries) => entries.into_iter().filter_map(|entry| match entry {{ SmeltUnknown::Array(values) if values.len() >= 2 => {{ let mut values = values.into_iter(); let key = match values.next()? {{ SmeltUnknown::String(value) => value.to_string(), SmeltUnknown::Number(value) => smelt_number_to_string(value), SmeltUnknown::Bool(value) => value.to_string(), _ => return None }}; Some((key, values.next()?)) }}, _ => None }}).collect::<SmeltRecord<String, SmeltUnknown>>(), _ => SmeltRecord::new() }}"
                )))),
                // A byte-backed host record's own enumerable properties are its
                // *element indices*: `Object.keys(new Uint8Array(1))` is `['0']`,
                // and `bytes`/`byteLength`/`length`/`byteOffset`/`buffer` are
                // internal storage or prototype accessors that never enumerate.
                // Leaking them would make a deep-equality walk over two views
                // compare storage keys instead of elements, and would recurse
                // through `buffer` back into the view.
                // An erased ARRAY's own enumerable keys are its element indices
                // followed by the named properties in its side table, which is
                // the order `Object.keys(['a'])` -> `["0"]` and
                // `Object.keys(withNamedProp)` -> `["0", "x"]` report.
                smelt_hir::DictProjectionOp::Keys => Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "match {dict_text} {{ SmeltUnknown::Object(map) => {index_keys}(&SmeltUnknown::Object(map.clone())).unwrap_or_else(|| map.keys().into_iter().filter(|key| !key.starts_with(\"{SMELT_SYMBOL}\") && !key.starts_with(\"{SMELT_PROTO}:\") && !key.starts_with(\"{SMELT_METHOD}:\") && key != \"{SMELT_CLASS}\").collect()), SmeltUnknown::Array(values) => values.own_keys(), _ => Vec::new() }}",
                    index_keys = smelt_stdlib::runtime_symbols::byte_buffer::INDEX_KEYS,
                )))),
                smelt_hir::DictProjectionOp::ForInKeys => Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "match {dict_text} {{ SmeltUnknown::Object(map) => {index_keys}(&SmeltUnknown::Object(map.clone())).unwrap_or_else(|| smelt_for_in_object_keys(&map)), SmeltUnknown::Array(values) => values.own_keys(), _ => Vec::new() }}",
                    index_keys = smelt_stdlib::runtime_symbols::byte_buffer::INDEX_KEYS,
                )))),
                // `Reflect.ownKeys` reports the string keys AND the symbol keys,
                // so unlike `Keys` above it cannot filter `__smelt_symbol:` out;
                // the projection maps those storage keys back to symbol values.
                // An array's own keys are its indices and named properties, all
                // strings.
                smelt_hir::DictProjectionOp::OwnKeys => Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "match {dict_text} {{ SmeltUnknown::Object(map) => {index_keys}(&SmeltUnknown::Object(map.clone())).map_or_else(|| smelt_own_object_keys(&map), |keys| keys.into_iter().map(|key| SmeltUnknown::String(key.as_str().into())).collect()), SmeltUnknown::Array(values) => values.own_keys().into_iter().map(|key| SmeltUnknown::String(key.as_str().into())).collect(), _ => Vec::new() }}",
                    index_keys = smelt_stdlib::runtime_symbols::byte_buffer::INDEX_KEYS,
                )))),
                // `Object.getOwnPropertySymbols` yields symbol VALUES. The stored
                // key is `__smelt_symbol:<description>`; handing back the bare
                // description (a `String`) made the caller lose the symbol tag, so
                // `source[symbols[i]]` read the *string* key and missed, and
                // `target[symbols[i]] = v` wrote a plain string key. Re-tagging the
                // description as `SmeltUnknown::Symbol` keeps the round trip: the
                // property-key mapping turns that tag back into the prefixed key.
                smelt_hir::DictProjectionOp::Symbols => Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "match {dict_text} {{ SmeltUnknown::Object(map) => map.keys().into_iter().filter_map(|key| smelt_own_symbol_key_value(&key)).collect(), _ => Vec::new() }}"
                )))),
                // `Object.values`/`Object.entries` of a byte-backed view are its
                // decoded elements, paired with their index keys — the same own-key
                // set `Object.keys` reports above.
                smelt_hir::DictProjectionOp::Values => Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "match {dict_text} {{ SmeltUnknown::Object(map) => {elements}(&SmeltUnknown::Object(map.clone())).unwrap_or_else(|| map.iter().filter(|(key, _)| !key.starts_with(\"{SMELT_SYMBOL}\") && !key.starts_with(\"{SMELT_PROTO}:\") && !key.starts_with(\"{SMELT_METHOD}:\") && key != \"{SMELT_CLASS}\").map(|(_, value)| value).collect()), SmeltUnknown::Array(values) => values.own_entries().into_iter().map(|(_, value)| value).collect(), _ => Vec::new() }}",
                    elements = smelt_stdlib::runtime_symbols::byte_buffer::OWN_ELEMENTS,
                )))),
                smelt_hir::DictProjectionOp::Entries => Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "match {dict_text} {{ SmeltUnknown::Object(map) => {elements}(&SmeltUnknown::Object(map.clone())).map_or_else(|| map.clone().into_iter().filter(|(key, _)| !key.starts_with(\"{SMELT_SYMBOL}\") && !key.starts_with(\"{SMELT_PROTO}:\") && !key.starts_with(\"{SMELT_METHOD}:\") && key != \"{SMELT_CLASS}\").collect::<Vec<_>>(), |values| values.into_iter().enumerate().map(|(index, value)| (index.to_string(), value)).collect::<Vec<_>>()), SmeltUnknown::Array(values) => values.own_entries(), _ => Vec::new() }}",
                    elements = smelt_stdlib::runtime_symbols::byte_buffer::OWN_ELEMENTS,
                )))),
            };
        }
        if !matches!(
            self.mir.types.get(self.operand_ty(dict)?),
            Some(Type::Dict(_, _) | Type::JsMap(_, _))
        ) {
            return Err(EmitError::new("dict projection receiver must be a dict"));
        }
        match op {
            smelt_hir::DictProjectionOp::FromEntries => {
                Err(EmitError::new("fromEntries receiver must be erased"))
            }
            smelt_hir::DictProjectionOp::Keys => {
                let Some(Type::Dict(key_ty, _) | Type::JsMap(key_ty, _)) =
                    self.mir.types.get(self.operand_ty(dict)?)
                else {
                    return Err(EmitError::new("dict projection receiver must be a dict"));
                };
                if self.mir.types.get(*key_ty) == Some(&Type::String) {
                    // `Object.keys` returns own enumerable string keys; exclude
                    // both symbol keys and Smelt's internal marker keys
                    // (`__smelt_date`, `__smelt_regexp`/`source`/`flags`, ...),
                    // which are representation details, not real JS properties.
                    // This is why e.g. `isShallowEqual(/a/, /b/)` (no own keys) is
                    // equal. `SmeltRecord` gets the filter through
                    // `smelt_is_for_in_record_key`, which is defined over that
                    // backing; a `SmeltUnknown`-keyed `SmeltJsMap` gets the same
                    // rule from `smelt_own_js_map_entries` in the non-`String`
                    // key branch below. A `SmeltJsMap` DOES carry the markers —
                    // the erased-object coercion copies them in verbatim — so the
                    // symbol-only filter used to leak an inherited key here.
                    if self.map_op_uses_smelt_record(self.operand_ty(dict)?, *key_ty) {
                        // A byte-backed host record's own keys are its element
                        // indices, not the storage keys the marker filter hides.
                        if self.dict_holds_erased_values(self.operand_ty(dict)?) {
                            return Ok(RustExpr::chain(format!(
                                "{helper}(&{dict_text}).unwrap_or_else(|| {dict_text}.keys().filter(|key| !key.starts_with(\"{SMELT_SYMBOL}\") && smelt_is_for_in_record_key(&{dict_text}, key)).collect::<Vec<_>>())",
                                helper = smelt_stdlib::runtime_symbols::byte_buffer::RECORD_INDEX_KEYS,
                            )));
                        }
                        Ok(RustExpr::atom_on(&dict_text, format!(
                            "{dict_text}.keys().filter(|key| !key.starts_with(\"{SMELT_SYMBOL}\") && smelt_is_for_in_record_key(&{dict_text}, key)).collect::<Vec<_>>()"
                        )))
                    } else if self.map_op_uses_js_key_map(self.operand_ty(dict)?, *key_ty) {
                        Ok(RustExpr::atom_on(&dict_text, format!(
                            "{dict_text}.keys().filter(|key| !key.starts_with(\"{SMELT_SYMBOL}\")).collect::<Vec<_>>()"
                        )))
                    } else {
                        Ok(RustExpr::atom_on(&dict_text, format!(
                            "{dict_text}.keys().filter(|key| !key.starts_with(\"{SMELT_SYMBOL}\")).cloned().collect::<Vec<_>>()"
                        )))
                    }
                } else if self.map_op_uses_js_key_map(self.operand_ty(dict)?, *key_ty) {
                    // Own string keys only: the prelude helper strips the
                    // representation markers and restores symbol tags, then the
                    // symbol filter drops the symbol-keyed properties
                    // `Object.keys` never reports.
                    Ok(RustExpr::atom(self.carrier_format(format_args!(
                        "smelt_own_js_map_entries(&{dict_text}).into_iter().filter(|(key, _)| !matches!(key, SmeltUnknown::Symbol(_))).map(|(key, _)| key).collect::<Vec<_>>()"
                    ))))
                } else {
                    Ok(RustExpr::atom_on(&dict_text, format!("{dict_text}.keys().cloned().collect::<Vec<_>>()")))
                }
            }
            // `Reflect.ownKeys`: the string keys in own-key order, then the
            // symbol keys. One helper per backing, mirroring the `ForInKeys`
            // split -- every string-keyed backing derefs to
            // `&SmeltRecord<String, _>`, and a `SmeltUnknown`-keyed one goes
            // through the own-entry helper that restores symbol tags.
            smelt_hir::DictProjectionOp::OwnKeys => {
                let Some(Type::Dict(key_ty, _) | Type::JsMap(key_ty, _)) =
                    self.mir.types.get(self.operand_ty(dict)?)
                else {
                    return Err(EmitError::new("dict projection receiver must be a dict"));
                };
                if self.mir.types.get(*key_ty) == Some(&Type::String) {
                    Ok(RustExpr::chain(format!("smelt_own_keys(&{dict_text})")))
                } else if self.map_op_uses_js_key_map(self.operand_ty(dict)?, *key_ty) {
                    Ok(RustExpr::chain(format!("smelt_own_js_map_keys(&{dict_text})")))
                } else {
                    Err(EmitError::new(
                        "own-key projection needs a string-keyed or erased-keyed record",
                    ))
                }
            }
            smelt_hir::DictProjectionOp::ForInKeys => {
                let Some(Type::Dict(key_ty, _) | Type::JsMap(key_ty, _)) =
                    self.mir.types.get(self.operand_ty(dict)?)
                else {
                    return Err(EmitError::new("dict projection receiver must be a dict"));
                };
                if self.mir.types.get(*key_ty) == Some(&Type::String) {
                    // Every string-keyed backing goes through the same helper: it
                    // takes `&SmeltRecord<String, _>`, which each of them derefs to,
                    // and it owns the own-key filtering plus the prototype-chain
                    // walk that `for...in` needs (see `smelt_for_in_record_keys`).
                    // The `SmeltRecord` / `SmeltJsMap` / plain-dict split the
                    // sibling `Keys` arm needs therefore does not apply here, so
                    // this is one branch rather than three identical ones.
                    Ok(RustExpr::chain(format!("smelt_for_in_record_keys(&{dict_text})")))
                } else if self.map_op_uses_js_key_map(self.operand_ty(dict)?, *key_ty) {
                    // `for...in` also walks the enumerable `__smelt_proto:`
                    // members, so it uses the prototype-chain helper rather than
                    // the own-entries one; see `smelt_for_in_js_map_keys`.
                    Ok(RustExpr::chain(format!("smelt_for_in_js_map_keys(&{dict_text})")))
                } else {
                    Ok(RustExpr::atom_on(&dict_text, format!("{dict_text}.keys().cloned().collect::<Vec<_>>()")))
                }
            }
            smelt_hir::DictProjectionOp::Symbols => {
                let Some(Type::Dict(key_ty, _) | Type::JsMap(key_ty, _)) =
                    self.mir.types.get(self.operand_ty(dict)?)
                else {
                    return Err(EmitError::new("dict projection receiver must be a dict"));
                };
                // Symbol VALUES, not descriptions — see the erased arm above. A
                // `String`-keyed backing stores them prefixed, a `SmeltUnknown`-keyed
                // one stores the tag directly, and both hand back the tag.
                if self.mir.types.get(*key_ty) == Some(&Type::String) {
                    Ok(RustExpr::atom_on(&dict_text, format!(
                        "{dict_text}.keys().filter_map(|key| smelt_own_symbol_key_value(&key)).collect::<Vec<_>>()"
                    )))
                } else if self.map_op_uses_js_key_map(self.operand_ty(dict)?, *key_ty) {
                    // The mirror image of the `Keys` arm: the same own-entry
                    // helper restores a `__smelt_symbol:` storage key to its tag,
                    // and this arm keeps exactly the keys that filter drops.
                    Ok(RustExpr::atom(self.carrier_format(format_args!(
                        "smelt_own_js_map_entries(&{dict_text}).into_iter().map(|(key, _)| key).filter(|key| matches!(key, SmeltUnknown::Symbol(_))).collect::<Vec<_>>()"
                    ))))
                } else {
                    Ok(self.carrier_expr(&RustExpr::atom("Vec::<SmeltUnknown>::new()")))
                }
            }
            smelt_hir::DictProjectionOp::Values => {
                let Some(Type::Dict(key_ty, _) | Type::JsMap(key_ty, _)) =
                    self.mir.types.get(self.operand_ty(dict)?)
                else {
                    return Err(EmitError::new("dict projection receiver must be a dict"));
                };
                if self.mir.types.get(*key_ty) == Some(&Type::String) {
                    if self.map_op_uses_smelt_record(self.operand_ty(dict)?, *key_ty)
                    || self.map_op_uses_js_key_map(self.operand_ty(dict)?, *key_ty) {
                        // A byte-backed view's values are its decoded elements, not
                        // its internal storage fields; see the `Keys` arm above.
                        if self.dict_holds_erased_values(self.operand_ty(dict)?) {
                            return Ok(RustExpr::chain(format!(
                                "{helper}(&{dict_text}).unwrap_or_else(|| {dict_text}.iter().filter(|(key, _)| !key.starts_with(\"{SMELT_SYMBOL}\") && !key.starts_with(\"{SMELT_PROTO}:\") && !key.starts_with(\"{SMELT_METHOD}:\") && key != \"{SMELT_CLASS}\").map(|(_, value)| value).collect::<Vec<_>>())",
                                helper = smelt_stdlib::runtime_symbols::byte_buffer::RECORD_ELEMENTS,
                            )));
                        }
                        Ok(RustExpr::atom_on(&dict_text, format!(
                            "{dict_text}.iter().filter(|(key, _)| !key.starts_with(\"{SMELT_SYMBOL}\") && !key.starts_with(\"{SMELT_PROTO}:\") && !key.starts_with(\"{SMELT_METHOD}:\") && key != \"{SMELT_CLASS}\").map(|(_, value)| value).collect::<Vec<_>>()"
                        )))
                    } else {
                        Ok(RustExpr::atom_on(&dict_text, format!(
                            "{dict_text}.iter().filter(|(key, _)| !key.starts_with(\"{SMELT_SYMBOL}\") && !key.starts_with(\"{SMELT_PROTO}:\") && !key.starts_with(\"{SMELT_METHOD}:\") && key != \"{SMELT_CLASS}\").map(|(_, value)| value.clone()).collect::<Vec<_>>()"
                        )))
                    }
                } else if self.map_op_uses_js_key_map(self.operand_ty(dict)?, *key_ty) {
                    // Own, non-symbol entries only -- same rule as the `Keys` arm.
                    Ok(RustExpr::atom(self.carrier_format(format_args!(
                        "smelt_own_js_map_entries(&{dict_text}).into_iter().filter(|(key, _)| !matches!(key, SmeltUnknown::Symbol(_))).map(|(_, value)| value).collect::<Vec<_>>()"
                    ))))
                } else {
                    Ok(RustExpr::atom_on(&dict_text, format!("{dict_text}.values().cloned().collect::<Vec<_>>()")))
                }
            }
            smelt_hir::DictProjectionOp::Entries => {
                let Some(Type::Dict(key_ty, _) | Type::JsMap(key_ty, _)) =
                    self.mir.types.get(self.operand_ty(dict)?)
                else {
                    return Err(EmitError::new("dict projection receiver must be a dict"));
                };
                if self.mir.types.get(*key_ty) == Some(&Type::String) {
                    if self.map_op_uses_smelt_record(self.operand_ty(dict)?, *key_ty)
                    || self.map_op_uses_js_key_map(self.operand_ty(dict)?, *key_ty) {
                        // A byte-backed view's entries pair its element indices with
                        // its decoded elements; see the `Keys` arm above.
                        if self.dict_holds_erased_values(self.operand_ty(dict)?) {
                            return Ok(RustExpr::chain(format!(
                                "{helper}(&{dict_text}).map_or_else(|| {dict_text}.iter().filter(|(key, _)| !key.starts_with(\"{SMELT_SYMBOL}\") && !key.starts_with(\"{SMELT_PROTO}:\") && !key.starts_with(\"{SMELT_METHOD}:\") && key != \"{SMELT_CLASS}\").collect::<Vec<_>>(), |values| values.into_iter().enumerate().map(|(index, value)| (index.to_string(), value)).collect::<Vec<_>>())",
                                helper = smelt_stdlib::runtime_symbols::byte_buffer::RECORD_ELEMENTS,
                            )));
                        }
                        Ok(RustExpr::atom_on(&dict_text, format!(
                            "{dict_text}.iter().filter(|(key, _)| !key.starts_with(\"{SMELT_SYMBOL}\") && !key.starts_with(\"{SMELT_PROTO}:\") && !key.starts_with(\"{SMELT_METHOD}:\") && key != \"{SMELT_CLASS}\").collect::<Vec<_>>()"
                        )))
                    } else {
                        Ok(RustExpr::atom_on(&dict_text, format!(
                            "{dict_text}.iter().filter(|(key, _)| !key.starts_with(\"{SMELT_SYMBOL}\") && !key.starts_with(\"{SMELT_PROTO}:\") && !key.starts_with(\"{SMELT_METHOD}:\") && key != \"{SMELT_CLASS}\").map(|(key, value)| (key.clone(), value.clone())).collect::<Vec<_>>()"
                        )))
                    }
                } else if self.map_op_uses_js_key_map(self.operand_ty(dict)?, *key_ty) {
                    // Own, non-symbol entries only -- same rule as the `Keys` arm.
                    Ok(RustExpr::atom(self.carrier_format(format_args!(
                        "smelt_own_js_map_entries(&{dict_text}).into_iter().filter(|(key, _)| !matches!(key, SmeltUnknown::Symbol(_))).collect::<Vec<_>>()"
                    ))))
                } else {
                    Ok(RustExpr::atom_on(&dict_text, format!(
                        "{dict_text}.iter().map(|(key, value)| (key.clone(), value.clone())).collect::<Vec<_>>()"
                    )))
                }
            }
        }
    }

    /// Converts a JSON serialization operation to Rust text.
    ///
    /// The Serde JSON backend is intentionally confined to this helper and
    /// Cargo dependency injection, making it replaceable without changing HIR
    /// or frontend lowering.
    pub(super) fn json_stringify_expr(
        &self,
        value: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        if !matches!(self.mir.types.get(dest_ty), Some(Type::String)) {
            return Err(EmitError::new("JSON stringify destination must be string"));
        }
        if !self.is_json_serializable_type(self.operand_ty(value)?) {
            // The blocker names the TYPE; the enclosing function and its source
            // span come from the site the emit entrypoints attach
            // (`EmitError::with_site`), which is the one mechanism every
            // function-scoped blocker uses. Naming the function here as well
            // printed it twice and without the span.
            return Err(EmitError::new(format!(
                "JSON stringify value must be JSON-serializable, got {}",
                self.rendered_type(self.operand_ty(value)?)?,
            )));
        }
        // Serialize through the erased carrier, ALWAYS. `JSON.stringify` is
        // specified by ECMA-262, not by serde: an integral number has no
        // fraction (`1`, not `1.0`), a non-finite number is `null`, an
        // `undefined` property is omitted, key order is insertion order, and a
        // byte-backed view serializes as its element indices. Every one of
        // those rules already lives in `Serialize for SmeltUnknown`, so routing
        // the value through it is what makes one rule decide the output for
        // every shape — including a UNION, whose arm is only known at run time
        // and whose per-arm answer is exactly its tag's arm in that impl.
        //
        // Serializing a concretely typed value directly was serde's format,
        // not JavaScript's: `JSON.stringify({a: 1})` printed `{"a":1.0}` for
        // every integral number in the crate, in records, lists, class
        // instances and nested objects alike.
        let value_ty = self.operand_ty(value)?;
        let value_text = self.erase_rendered(&self.operand_expr(value)?, value_ty)?;
        Ok(RustExpr::atom(format!(
            "serde_json::to_string(&{value_text}).expect(\"JSON serialization failed\")"
        )))
    }

}
