//! JavaScript binary-operator semantics: optional/unknown/erased arithmetic, equality, and relational comparison text emission.

use super::*;

impl FunctionEmitter<'_> {
    /// Emits binary operations involving `Option<T>` by coercing through the
    /// optional's inner type instead of letting Rust compare unrelated shapes.
    pub(super) fn optional_binary_text(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
        dest_ty: TypeId,
    ) -> Result<Option<String>, EmitError> {
        let lhs_ty = self.operand_ty(lhs)?;
        let rhs_ty = self.operand_ty(rhs)?;
        let lhs_inner = self.optional_inner_ty(lhs_ty);
        let rhs_inner = self.optional_inner_ty(rhs_ty);
        if lhs_inner.is_none() && rhs_inner.is_none() {
            return Ok(None);
        }

        if matches!(
            op,
            smelt_hir::BinOp::Eq
                | smelt_hir::BinOp::NotEq
                | smelt_hir::BinOp::StrictEq
                | smelt_hir::BinOp::StrictNotEq
                | smelt_hir::BinOp::JsStrictEq
                | smelt_hir::BinOp::JsStrictNotEq
        ) {
            return self.optional_equality_text(op, lhs, rhs, lhs_inner, rhs_inner);
        }

        // JS relational operators (`<`, `>`, `<=`, `>=`) with an optional operand
        // whose held value is erased (`unknown`, a concrete/erased union, a type
        // param) coerce BOTH sides with `ToNumber` and compare as `f64` — an
        // absent/`undefined`/non-numeric value becomes `NaN`, so every comparison
        // against it is `false`, which `f64`'s own `NaN` ordering already yields.
        // Without this arm the comparison falls through to a raw `Option<…> < f64`
        // (E0277). Skip when a side is statically a `String` (JS does lexical
        // string comparison there) so this stays a numeric-coercion path only.
        if matches!(
            op,
            smelt_hir::BinOp::Lt
                | smelt_hir::BinOp::Lte
                | smelt_hir::BinOp::Gt
                | smelt_hir::BinOp::Gte
        ) {
            let side_is_string = |emitter: &Self, ty: TypeId, inner: Option<TypeId>| {
                matches!(emitter.mir.types.get(ty), Some(Type::String))
                    || inner.is_some_and(|inner_ty| {
                        matches!(emitter.mir.types.get(inner_ty), Some(Type::String))
                    })
            };
            // A side counts as erased when its held value is erased: the inner
            // type for an optional operand, or the operand's own type when it is
            // a bare (non-optional) erased value. The latter covers comparing an
            // optional number against an erased `SmeltUnknown` such as a
            // `.length` read (es-toolkit `includes`).
            let side_erased = |emitter: &Self, ty: TypeId, inner: Option<TypeId>| match inner {
                Some(inner_ty) => emitter.is_erased_relational(inner_ty),
                None => emitter.is_erased_relational(ty),
            };
            let lhs_erased_inner = side_erased(self, lhs_ty, lhs_inner);
            let rhs_erased_inner = side_erased(self, rhs_ty, rhs_inner);
            if (lhs_erased_inner || rhs_erased_inner)
                && !side_is_string(self, lhs_ty, lhs_inner)
                && !side_is_string(self, rhs_ty, rhs_inner)
            {
                let float_ty = self.type_id(Type::Float)?;
                let unknown_ty = self.type_id(Type::Unknown)?;
                // Coerce one operand to an `f64`. An optional side is first
                // projected to a bare `SmeltUnknown` (`erase()` maps absence to
                // `SmeltUnknown::Undefined`), then `ToNumber`-extracted; a numeric
                // optional keeps the existing unwrap-with-default extraction; a
                // non-optional side extracts directly at its own type.
                let side_text = |emitter: &Self, operand: &Operand, inner: Option<TypeId>| {
                    match inner {
                        Some(inner_ty) if emitter.is_erased_relational(inner_ty) => {
                            let erased = emitter.erase(operand)?;
                            emitter.value_at_type_text(&erased, unknown_ty, float_ty)
                        }
                        Some(inner_ty) => {
                            emitter.option_value_as_type_text(operand, inner_ty, float_ty)
                        }
                        None => emitter.value_at_type(operand, float_ty),
                    }
                };
                let lhs_text = side_text(self, lhs, lhs_inner)?;
                let rhs_text = side_text(self, rhs, rhs_inner)?;
                return Ok(Some(format!(
                    "({lhs_text}) {} ({rhs_text})",
                    smelt_hir::bin_op_text(op)
                )));
            }
        }

        if let Some(inner) = lhs_inner
            && rhs_inner.is_none()
            && self.is_numeric_type(inner)
            && self.is_numeric_type(rhs_ty)
        {
            let common_ty = self.common_numeric_type(inner, rhs_ty, dest_ty)?;
            return Ok(Some(format!(
                "{} {} {}",
                self.option_value_as_type_text(lhs, inner, common_ty)?,
                smelt_hir::bin_op_text(op),
                self.value_at_type(rhs, common_ty)?
            )));
        }

        if let Some(inner) = rhs_inner
            && lhs_inner.is_none()
            && self.is_numeric_type(lhs_ty)
            && self.is_numeric_type(inner)
        {
            let common_ty = self.common_numeric_type(lhs_ty, inner, dest_ty)?;
            return Ok(Some(format!(
                "{} {} {}",
                self.value_at_type(lhs, common_ty)?,
                smelt_hir::bin_op_text(op),
                self.option_value_as_type_text(rhs, inner, common_ty)?
            )));
        }

        Ok(None)
    }

    /// Emits equality checks for erased `SmeltUnknown` operands.
    pub(super) fn unknown_binary_text(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
    ) -> Result<Option<String>, EmitError> {
        if !matches!(
            op,
            smelt_hir::BinOp::Eq
                | smelt_hir::BinOp::NotEq
                | smelt_hir::BinOp::StrictEq
                | smelt_hir::BinOp::StrictNotEq
                | smelt_hir::BinOp::JsStrictEq
                | smelt_hir::BinOp::JsStrictNotEq
        ) {
            return Ok(None);
        }
        let lhs_ty = self.operand_ty(lhs)?;
        let rhs_ty = self.operand_ty(rhs)?;
        let lhs_is_erased = matches!(
            self.mir.types.get(lhs_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(lhs_ty);
        let rhs_is_erased = matches!(
            self.mir.types.get(rhs_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(rhs_ty);
        let lhs_is_none = self.mir.types.get(lhs_ty) == Some(&Type::None);
        let rhs_is_none = self.mir.types.get(rhs_ty) == Some(&Type::None);
        let strict = matches!(
            op,
            smelt_hir::BinOp::StrictEq | smelt_hir::BinOp::StrictNotEq
        );
        let js_strict = matches!(
            op,
            smelt_hir::BinOp::JsStrictEq | smelt_hir::BinOp::JsStrictNotEq
        );
        let strict_nullish = strict || js_strict;
        // Pick the erased nullish tag for a `None`-typed literal: the `undefined`
        // literal matches `Undefined`, every other `None`-typed operand (the
        // `null` literal or a unit value) matches `Null`. Used by the
        // erased-vs-none arms where the none side is essentially always a literal.
        let nullish_pattern = |operand: &Operand| {
            if strict_nullish {
                if matches!(operand, Operand::Const(Constant::Undefined)) {
                    "SmeltUnknown::Undefined"
                } else {
                    "SmeltUnknown::Null"
                }
            } else {
                "SmeltUnknown::Null | SmeltUnknown::Undefined"
            }
        };
        // `null` and `undefined` are byte-identical in MIR (both `Type::None`), so
        // the JS nullish kind can only be proven from an explicit literal:
        // `Constant::None` is `null`, `Constant::Undefined` is `undefined`. Two
        // `None`-typed operands therefore strict-differ ONLY when one is provably
        // the `null` literal and the other provably the `undefined` literal. In
        // every other case — including a unit temporary produced by an
        // `undefined`-valued expression such as `clone(undefined)` compared
        // against the `undefined` literal — they share the same nullish value and
        // strict-equal. (The old code keyed on "is undefined?", so a non-literal
        // unit actual wrongly differed from the `undefined` literal.)
        let provably_null =
            |operand: &Operand| matches!(operand, Operand::Const(Constant::None));
        let provably_undefined =
            |operand: &Operand| matches!(operand, Operand::Const(Constant::Undefined));
        let text = if lhs_is_none && rhs_is_none {
            if strict_nullish {
                let distinct = (provably_null(lhs) && provably_undefined(rhs))
                    || (provably_undefined(lhs) && provably_null(rhs));
                (!distinct).to_string()
            } else {
                "true".to_owned()
            }
        } else if lhs_is_erased && rhs_is_none {
            format!(
                "matches!({}, {})",
                self.operand_text(lhs)?,
                nullish_pattern(rhs)
            )
        } else if rhs_is_erased && lhs_is_none {
            format!(
                "matches!({}, {})",
                self.operand_text(rhs)?,
                nullish_pattern(lhs)
            )
        } else if lhs_is_none || rhs_is_none {
            "false".to_owned()
        } else if lhs_is_erased || rhs_is_erased {
            // Both sides are compared through `SmeltUnknown`'s JS-equality impls,
            // so both operands must be genuine `SmeltUnknown` values. `erase()` is
            // idempotent for operands whose Rust representation is already
            // `SmeltUnknown` (`Type::Unknown`, generic type params, erased class
            // types, non-concrete unions), but a CONCRETE union renders as a
            // tagged enum (`SmeltUnionN`) and must be projected via
            // `into_smelt_unknown()` before `same_js_key`/`js_strict_eq`/`==`
            // resolve. Routing both sides through `erase()` handles that case
            // without special-casing the union arm.
            let lhs_text = self.erase(lhs)?;
            // `js_strict_eq`/`same_js_key` take `&SmeltUnknown` and only read it,
            // so an already-erased right-hand side is BORROWED rather than cloned
            // and then borrowed. `erase` on such an operand is the ordinary value
            // read, which appends a `.clone()`; for a string-valued unknown that
            // clone was a refcount bump the comparison never kept.
            let rhs_reference_text = if matches!(
                self.mir.types.get(self.operand_ty(rhs)?),
                Some(Type::Unknown)
            ) {
                format!("&{}", self.operand_borrow_text(rhs)?)
            } else {
                format!("&{}", self.erase(rhs)?)
            };
            let rhs_text = self.erase(rhs)?;
            if js_strict {
                // JavaScript `===`/`!==` on erased values: reference identity for
                // objects/arrays/functions, value for primitives, NaN-unequal.
                format!("{lhs_text}.js_strict_eq({rhs_reference_text})")
            } else if strict {
                // `Object.is` / `a === b || Object.is(a,b)` SameValueZero idiom:
                // NaN-equal, reference objects.
                format!("{lhs_text}.same_js_key({rhs_reference_text})")
            } else {
                // Loose/structural `==`/`!=` on erased values: SmeltUnknown's
                // structural `Eq` (deep), which the `toEqual`/`toStrictEqual`
                // matchers and `isDeepEqual` depend on.
                format!("{lhs_text} == {rhs_text}")
            }
        } else {
            return Ok(None);
        };
        Ok(Some(
            if matches!(
                op,
                smelt_hir::BinOp::NotEq
                    | smelt_hir::BinOp::StrictNotEq
                    | smelt_hir::BinOp::JsStrictNotEq
            ) {
                format!("!({text})")
            } else {
                text
            },
        ))
    }

    /// Emits equality and inequality for optional operands.
    fn optional_equality_text(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
        lhs_inner: Option<TypeId>,
        rhs_inner: Option<TypeId>,
    ) -> Result<Option<String>, EmitError> {
        let negate = matches!(
            op,
            smelt_hir::BinOp::NotEq
                | smelt_hir::BinOp::StrictNotEq
                | smelt_hir::BinOp::JsStrictNotEq
        );
        let strict = matches!(
            op,
            smelt_hir::BinOp::StrictEq | smelt_hir::BinOp::StrictNotEq
        );
        let js_strict = matches!(
            op,
            smelt_hir::BinOp::JsStrictEq | smelt_hir::BinOp::JsStrictNotEq
        );
        let strict_nullish = strict || js_strict;
        // `Optional` of a reference type keeps JavaScript identity semantics on
        // the present values while absence stays comparable: two absent values
        // are the same (`undefined === undefined`), and present-vs-absent never
        // is. Applies to every identity-bearing container, not just the
        // string-keyed record this arm originally covered.
        if strict
            && let (Some(left_inner), Some(right_inner)) = (lhs_inner, rhs_inner)
            && let (Some(left_identity), Some(right_identity)) = (
                self.reference_identity_text("left", left_inner),
                self.reference_identity_text("right", right_inner),
            )
        {
            let text = format!(
                "match ({}.as_ref(), {}.as_ref()) {{ (Some(left), Some(right)) => {left_identity} == {right_identity}, (None, None) => true, _ => false }}",
                self.operand_text(lhs)?,
                self.operand_text(rhs)?
            );
            return Ok(Some(if negate { format!("!({text})") } else { text }));
        }
        // Exactly one side is optional and both hold reference values: an absent
        // optional is `undefined`, which is never the same reference as a
        // present object, so only the present case compares identities.
        if strict
            && let Some(pair) = match (lhs_inner, rhs_inner) {
                (Some(inner), None) => Some((lhs, inner, rhs)),
                (None, Some(inner)) => Some((rhs, inner, lhs)),
                _ => None,
            }
        {
            let (option_operand, inner, bare_operand) = pair;
            let bare_ty = self.operand_ty(bare_operand)?;
            if let (Some(inner_identity), Some(bare_identity)) = (
                self.reference_identity_text("value", inner),
                self.reference_identity_text(&self.operand_text(bare_operand)?, bare_ty),
            ) {
                let text = format!(
                    "{}.as_ref().is_some_and(|value| {inner_identity} == {bare_identity})",
                    self.operand_text(option_operand)?
                );
                return Ok(Some(if negate { format!("!({text})") } else { text }));
            }
        }
        let text = if let Some(inner) = lhs_inner
            && self.operand_ty(rhs)? == self.none_ty
        {
            let lhs_text = self.operand_text(lhs)?;
            if self.optional_inner_preserves_erased_singletons(inner) {
                self.optional_erased_singleton_equality_text(&lhs_text, rhs, strict_nullish, inner)?
            } else {
                format!("{lhs_text}.is_none()")
            }
        } else if let Some(inner) = rhs_inner
            && self.operand_ty(lhs)? == self.none_ty
        {
            let rhs_text = self.operand_text(rhs)?;
            if self.optional_inner_preserves_erased_singletons(inner) {
                self.optional_erased_singleton_equality_text(&rhs_text, lhs, strict_nullish, inner)?
            } else {
                format!("{rhs_text}.is_none()")
            }
        } else if let Some(inner) = lhs_inner
            && rhs_inner.is_none()
        {
            format!(
                "{} == Some({})",
                self.operand_text(lhs)?,
                self.value_at_type(rhs, inner)?
            )
        } else if let Some(inner) = rhs_inner
            && lhs_inner.is_none()
        {
            format!(
                "Some({}) == {}",
                self.value_at_type(lhs, inner)?,
                self.operand_text(rhs)?
            )
        } else {
            return Ok(None);
        };
        Ok(Some(if negate { format!("!({text})") } else { text }))
    }

    /// Emits equality between `Option<SmeltUnknown>`-like storage and a JS
    /// nullish singleton.
    fn optional_erased_singleton_equality_text(
        &self,
        option_text: &str,
        singleton: &Operand,
        strict_nullish: bool,
        inner: TypeId,
    ) -> Result<String, EmitError> {
        let pattern = if strict_nullish {
            if matches!(singleton, Operand::Const(Constant::Undefined)) {
                "SmeltUnknown::Undefined"
            } else {
                "SmeltUnknown::Null"
            }
        } else {
            "SmeltUnknown::Null | SmeltUnknown::Undefined"
        };
        // A concrete-union `Option` payload stores a tagged enum; project each
        // present value to `SmeltUnknown` before the nullish tag match. A present
        // union value never holds `null`/`undefined` (those are the `None`), so
        // this preserves the exact loose/strict comparison semantics.
        let scrutinee = if self.concrete_union_members(inner).is_some() {
            "value.clone().into_smelt_unknown()"
        } else {
            "value"
        };
        let missing_matches =
            !strict_nullish || matches!(singleton, Operand::Const(Constant::Undefined));
        if missing_matches {
            Ok(format!(
                "{option_text}.as_ref().map_or(true, |value| matches!({scrutinee}, {pattern}))"
            ))
        } else {
            Ok(format!(
                "{option_text}.as_ref().is_some_and(|value| matches!({scrutinee}, {pattern}))"
            ))
        }
    }

    /// Emits numeric comparisons after coercing both operands to a shared scalar.
    ///
    /// HIR preserves TypeScript numeric intent even when one side has been
    /// inferred as `number` and the other as an integer-like literal. Rust does
    /// not compare `i64` and `f64` directly, so this keeps generated assertions
    /// and branch predicates type-directed instead of relying on literal shape.
    /// Emits a JS relational comparison (`<`, `<=`, `>`, `>=`) where at least one
    /// bare (non-optional) operand is erased for relational purposes — an
    /// `unknown`, a concrete/erased union (e.g. `string | number`), a type param,
    /// or an erased class — and neither operand is statically a `String`.
    ///
    /// JavaScript's relational algorithm runs `ToNumber` on both operands unless
    /// *both* are strings (the lexical case, left to other arms). A comparison of
    /// a number against a `string | number` union therefore coerces the union side
    /// with `ToNumber` and compares as `f64`; a non-numeric value becomes `NaN`, so
    /// every comparison against it is `false`, which `f64`'s own `NaN` ordering
    /// already yields. Without this arm such a comparison reaches a raw
    /// `f64 {op} SmeltUnion…` which does not type-check (E0277).
    ///
    /// This mirrors the erased-relational coercion in `optional_binary_text`, but
    /// for operands whose erased value is held directly rather than behind an
    /// `Option`. It only fires when every side is either numeric or erased, so the
    /// coercion to `f64` is always well-defined.
    pub(super) fn bare_erased_relational_text(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
    ) -> Result<Option<String>, EmitError> {
        if !matches!(
            op,
            smelt_hir::BinOp::Lt
                | smelt_hir::BinOp::Lte
                | smelt_hir::BinOp::Gt
                | smelt_hir::BinOp::Gte
        ) {
            return Ok(None);
        }
        let lhs_ty = self.operand_ty(lhs)?;
        let rhs_ty = self.operand_ty(rhs)?;
        // Optional operands are handled earlier by `optional_binary_text`; a
        // statically `String` side is handled by the lexical / mixed arms.
        if self.optional_inner_ty(lhs_ty).is_some() || self.optional_inner_ty(rhs_ty).is_some() {
            return Ok(None);
        }
        if matches!(self.mir.types.get(lhs_ty), Some(Type::String))
            || matches!(self.mir.types.get(rhs_ty), Some(Type::String))
        {
            return Ok(None);
        }
        let lhs_erased = self.is_erased_relational(lhs_ty);
        let rhs_erased = self.is_erased_relational(rhs_ty);
        if !lhs_erased && !rhs_erased {
            return Ok(None);
        }
        // Every side must be either numeric or erased so `ToNumber` coercion is
        // well-defined; anything else falls through to the remaining arms.
        if !(lhs_erased || self.is_numeric_type(lhs_ty))
            || !(rhs_erased || self.is_numeric_type(rhs_ty))
        {
            return Ok(None);
        }
        // When BOTH operands are erased, either could hold a string at runtime.
        // JavaScript's relational algorithm compares two strings *lexically*, so
        // blind `ToNumber` coercion (`NaN` for non-numeric text) would make every
        // string comparison `false` and break generic string-keyed sorts / binary
        // searches (remeda's `sortedIndex`, `firstBy`, `sortBy`, …). Route these
        // through the runtime abstract-relational helper, which stays lexical for
        // two strings and falls back to `ToNumber` for every other pairing.
        if lhs_erased && rhs_erased {
            let lhs_text = self.erase(lhs)?;
            let rhs_text = self.erase(rhs)?;
            // The guard at the top of this function restricts `op` to the four
            // relational operators, so the `Gte` wildcard is the only remaining
            // case (`match` cannot see through that earlier guard).
            let ordering_pattern = match op {
                smelt_hir::BinOp::Lt => "::std::cmp::Ordering::Less",
                smelt_hir::BinOp::Lte => {
                    "::std::cmp::Ordering::Less | ::std::cmp::Ordering::Equal"
                }
                smelt_hir::BinOp::Gt => "::std::cmp::Ordering::Greater",
                _ => "::std::cmp::Ordering::Greater | ::std::cmp::Ordering::Equal",
            };
            return Ok(Some(format!(
                "matches!(smelt_unknown_js_relational_ordering(&({lhs_text}), &({rhs_text})), Some({ordering_pattern}))"
            )));
        }
        // Exactly one side is erased and the other is statically numeric: JS keeps
        // this on the `ToNumber` path (only the both-strings case is lexical), so
        // coerce both operands to `f64` and compare directly.
        let float_ty = self.type_id(Type::Float)?;
        let lhs_text = self.value_at_type(lhs, float_ty)?;
        let rhs_text = self.value_at_type(rhs, float_ty)?;
        Ok(Some(format!(
            "({lhs_text}) {} ({rhs_text})",
            smelt_hir::bin_op_text(op)
        )))
    }

    /// Emits an equality or relational comparison between two statically numeric
    /// operands, coercing both to a shared scalar type (`i64`/`f64`) first so Rust
    /// does not reject a mixed `i64`-vs-`f64` comparison.
    pub(super) fn numeric_comparison_text(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
        dest_ty: TypeId,
    ) -> Result<Option<String>, EmitError> {
        if !matches!(
            op,
            smelt_hir::BinOp::Eq
                | smelt_hir::BinOp::NotEq
                | smelt_hir::BinOp::JsStrictEq
                | smelt_hir::BinOp::JsStrictNotEq
                | smelt_hir::BinOp::Lt
                | smelt_hir::BinOp::Lte
                | smelt_hir::BinOp::Gt
                | smelt_hir::BinOp::Gte
        ) {
            return Ok(None);
        }
        let lhs_ty = self.operand_ty(lhs)?;
        let rhs_ty = self.operand_ty(rhs)?;
        if !self.is_numeric_type(lhs_ty) || !self.is_numeric_type(rhs_ty) {
            return Ok(None);
        }
        let lhs_text = self.operand_text(lhs)?;
        let rhs_text = self.operand_text(rhs)?;
        let mut common_ty = self.common_numeric_type(lhs_ty, rhs_ty, dest_ty)?;
        if matches!(self.mir.types.get(common_ty), Some(Type::Int))
            && (lhs_text.contains(" as f64") || rhs_text.contains(" as f64"))
        {
            common_ty = self.type_id(Type::Float)?;
        }
        Ok(Some(format!(
            "{} {} {}",
            self.value_at_type_text(&lhs_text, lhs_ty, common_ty)?,
            smelt_hir::bin_op_text(op),
            self.value_at_type_text(&rhs_text, rhs_ty, common_ty)?
        )))
    }

    /// Emits a JS relational comparison (`<`, `<=`, `>`, `>=`) where exactly one
    /// operand is statically a `String` and the other is numeric.
    ///
    /// JavaScript's relational algorithm runs `ToNumber` on both operands unless
    /// *both* are strings (then it compares lexically). A `String`-vs-number
    /// comparison therefore coerces the string side with `ToNumber`: numeric
    /// strings parse to their value and non-numeric strings become `NaN`, so
    /// every comparison against them is `false` — matching JS. Without this arm
    /// the operands reach a raw `String {op} f64` which does not type-check
    /// (E0308). String-vs-string lexical comparison is left untouched (handled by
    /// the default path), and cases where a side is optional are handled earlier
    /// by `optional_binary_text`.
    pub(super) fn mixed_string_number_relational_text(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
    ) -> Result<Option<String>, EmitError> {
        if !matches!(
            op,
            smelt_hir::BinOp::Lt
                | smelt_hir::BinOp::Lte
                | smelt_hir::BinOp::Gt
                | smelt_hir::BinOp::Gte
        ) {
            return Ok(None);
        }
        let lhs_ty = self.operand_ty(lhs)?;
        let rhs_ty = self.operand_ty(rhs)?;
        let lhs_is_string = matches!(self.mir.types.get(lhs_ty), Some(Type::String));
        let rhs_is_string = matches!(self.mir.types.get(rhs_ty), Some(Type::String));
        // Only a genuine string-vs-number mix; string-vs-string stays lexical and
        // number-vs-number is handled by `numeric_comparison_text`.
        if lhs_is_string == rhs_is_string {
            return Ok(None);
        }
        let numeric_side_ty = if lhs_is_string { rhs_ty } else { lhs_ty };
        if !self.is_numeric_type(numeric_side_ty) {
            return Ok(None);
        }
        // `ToNumber` a string operand: parse to `f64`, non-numeric text -> `NaN`,
        // mirroring the `SmeltUnknown` ToNumber path used elsewhere in codegen.
        let string_to_number = |text: String| format!("({text}).parse::<f64>().unwrap_or(f64::NAN)");
        let float_ty = self.type_id(Type::Float)?;
        let lhs_text = if lhs_is_string {
            string_to_number(self.operand_text(lhs)?)
        } else {
            self.value_at_type(lhs, float_ty)?
        };
        let rhs_text = if rhs_is_string {
            string_to_number(self.operand_text(rhs)?)
        } else {
            self.value_at_type(rhs, float_ty)?
        };
        Ok(Some(format!(
            "({lhs_text}) {} ({rhs_text})",
            smelt_hir::bin_op_text(op)
        )))
    }

    /// Emits equality for first-class callback values.
    ///
    /// Stored callbacks lower to `Rc<dyn Fn...>`, which has no
    /// structural equality. JavaScript compares function values by identity, so
    /// matching callback shapes can use `Rc::ptr_eq`; mismatched shapes are
    /// unequal and compile to a constant.
    pub(super) fn function_equality_text(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
    ) -> Result<Option<String>, EmitError> {
        if !matches!(
            op,
            smelt_hir::BinOp::Eq
                | smelt_hir::BinOp::NotEq
                | smelt_hir::BinOp::StrictEq
                | smelt_hir::BinOp::StrictNotEq
                | smelt_hir::BinOp::JsStrictEq
                | smelt_hir::BinOp::JsStrictNotEq
        ) {
            return Ok(None);
        }
        let lhs_ty = self.operand_ty(lhs)?;
        let rhs_ty = self.operand_ty(rhs)?;
        let lhs_contains_function = self.type_contains_function(lhs_ty);
        let rhs_contains_function = self.type_contains_function(rhs_ty);
        if !lhs_contains_function && !rhs_contains_function {
            return Ok(None);
        }
        let equal_text = if lhs_ty == rhs_ty && lhs_contains_function && rhs_contains_function {
            self.function_bearing_equality_text(
                &self.operand_text(lhs)?,
                &self.operand_text(rhs)?,
                lhs_ty,
            )?
        } else {
            "false".to_owned()
        };
        Ok(Some(
            if matches!(
                op,
                smelt_hir::BinOp::NotEq
                    | smelt_hir::BinOp::StrictNotEq
                    | smelt_hir::BinOp::JsStrictNotEq
            ) {
                format!("!({equal_text})")
            } else {
                equal_text
            },
        ))
    }

    /// Emits structural equality recursively while comparing function leaves by identity.
    fn function_bearing_equality_text(
        &self,
        left: &str,
        right: &str,
        ty: TypeId,
    ) -> Result<String, EmitError> {
        Ok(match self.mir.types.get(ty) {
            Some(Type::Function(_)) => self.function_ptr_eq_text(left, right, ty)?,
            Some(Type::List(item)) => {
                let item_equal =
                    self.function_bearing_equality_text("left_item", "right_item", *item)?;
                // `.len()` is the list's own inherent method; `.iter()` reads
                // the shared buffer. Both are shared borrows, so comparing a
                // list against itself is fine.
                let left_read = list_read_text(left);
                let right_read = list_read_text(right);
                format!(
                    "{left}.len() == {right}.len() && {left_read}.iter().zip({right_read}.iter()).all(|(left_item, right_item)| {item_equal})"
                )
            }
            Some(Type::Dict(key, value)) if self.mir.types.get(*key) == Some(&Type::String) => {
                let value_equal =
                    self.function_bearing_equality_text("left_value", "right_value", *value)?;
                format!(
                    "{left}.len() == {right}.len() && {left}.iter().all(|(key, left_value)| {right}.get(&key).is_some_and(|right_value| {value_equal}))"
                )
            }
            Some(Type::Optional(inner)) => {
                let item_equal =
                    self.function_bearing_equality_text("left_value", "right_value", *inner)?;
                format!(
                    "match ({left}.as_ref(), {right}.as_ref()) {{ (Some(left_value), Some(right_value)) => {item_equal}, (None, None) => true, _ => false }}"
                )
            }
            _ => format!("{left} == {right}"),
        })
    }

    /// Emits an identity comparison for two function values.
    ///
    /// Both operands share the same MIR `Type::Function`, but their emitted
    /// Rust types can differ: a value produced by a combinator such as
    /// `flow(...)` is already the erased `Rc<dyn Fn(...) -> _>` trait object,
    /// while a locally bound closure keeps its concrete `Rc<{closure}>` type.
    /// `Rc::ptr_eq` requires both references to have the identical `Rc<T>`, so
    /// each operand is first coerced to the common `Rc<dyn Fn(...)>` type
    /// (an unsizing coercion that preserves the shared allocation, keeping
    /// pointer identity intact). The clone is cheap and does not affect the
    /// `ptr_eq` result.
    fn function_ptr_eq_text(
        &self,
        left: &str,
        right: &str,
        ty: TypeId,
    ) -> Result<String, EmitError> {
        let fn_ty = self.type_text_with_impl_trait(ty, false)?;
        Ok(format!(
            "smelt_same_function_identity(&{{ let smelt_lhs_fn: {fn_ty} = {left}.clone(); smelt_lhs_fn }}, &{{ let smelt_rhs_fn: {fn_ty} = {right}.clone(); smelt_rhs_fn }})"
        ))
    }

    /// Returns whether a MIR type is a JavaScript *reference* type — a value
    /// that `===` compares by object identity rather than by contents.
    ///
    /// This is the type-level half of the identity rule; whether the chosen Rust
    /// representation can actually *observe* that identity is a separate
    /// question answered by [`Self::reference_identity_text`].
    fn is_reference_type(&self, ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(ty),
            Some(
                Type::List(_)
                    | Type::Dict(_, _)
                    | Type::JsMap(_, _)
                    | Type::Set(_)
                    | Type::Tuple(_)
                    | Type::Class { .. }
                    | Type::Function(_)
            )
        )
    }

    /// Returns the Rust expression reading a typed reference value's JavaScript
    /// object identity, or `None` when its Rust representation carries none.
    ///
    /// Reference identity is only available where the emitted container keeps a
    /// stable object id, and each such container is listed here so the rule
    /// stays keyed on the *representation* rather than on any library spelling:
    ///
    /// * `SmeltList` (`Type::List`) exposes `id()`.
    /// * `SmeltRecord` (a string-keyed `Type::Dict`, when the `SmeltUnknown`
    ///   prelude is emitted) and `SmeltJsMap` (an object-keyed `Type::Dict`, or
    ///   any source-spelled `Type::JsMap`) both carry an `id` field.
    /// * `SmeltJsSet` (a `Type::Set` whose element cannot key a Rust `HashSet`)
    ///   carries an `id` field.
    ///
    /// Every id comes from the one `smelt_next_object_id` counter, so ids from
    /// different containers are directly comparable. In each of those
    /// containers Rust `Clone` deliberately *shares* the id (codegen inserts
    /// `.clone()` freely, and a JS value copy would otherwise lose its
    /// identity), so an identity comparison survives codegen's own clones.
    ///
    /// The representations deliberately left without identity are the plain
    /// `HashMap`/`HashSet` dicts and sets, Rust tuples (`Type::Tuple`) and
    /// generated class structs (`Type::Class`): none of them stores an object
    /// id today, so there is no identity to read and this returns `None` rather
    /// than inventing one.
    fn reference_identity_text(&self, text: &str, ty: TypeId) -> Option<String> {
        match self.mir.types.get(ty)? {
            Type::List(_) => Some(format!("{text}.id()")),
            Type::Dict(key, _)
                if self.dict_uses_smelt_record(*key) || self.dict_uses_js_key_map(*key) =>
            {
                Some(format!("{text}.id"))
            }
            Type::JsMap(_, _) => Some(format!("{text}.id")),
            Type::Set(item) if !self.type_is_hash_set_key_safe(*item) => {
                Some(format!("{text}.id"))
            }
            _ => None,
        }
    }

    /// Emits JavaScript SameValue checks for numeric and reference values.
    ///
    /// Reference-typed operands compare by JavaScript object identity, matching
    /// what the erased `SmeltUnknown` path (`js_strict_eq` / `same_js_key`) does
    /// for the same values: two distinct objects with identical contents are
    /// `!==`. Structural `PartialEq` stays reserved for the loose/deep
    /// comparisons (`toEqual`, `toStrictEqual`, `isDeepEqual`), which this
    /// function never handles — it only fires for `StrictEq`/`StrictNotEq`.
    ///
    /// When a reference operand's representation carries no observable identity
    /// (see [`Self::reference_identity_text`]) the result stays the pre-existing
    /// constant `false`: two such values cannot be proven to be the same
    /// reference.
    pub(super) fn strict_identity_text(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
    ) -> Result<Option<String>, EmitError> {
        if !matches!(
            op,
            smelt_hir::BinOp::StrictEq | smelt_hir::BinOp::StrictNotEq
        ) {
            return Ok(None);
        }
        let lhs_ty = self.operand_ty(lhs)?;
        let rhs_ty = self.operand_ty(rhs)?;
        let equal_text = match (self.mir.types.get(lhs_ty), self.mir.types.get(rhs_ty)) {
            (Some(Type::Int | Type::Float), Some(Type::Int | Type::Float)) => {
                let lhs_text = self.float_operand_text(lhs)?;
                let rhs_text = self.float_operand_text(rhs)?;
                format!(
                    "{{ let lhs: f64 = {lhs_text}; let rhs: f64 = {rhs_text}; (lhs.is_nan() && rhs.is_nan()) || (lhs == rhs && (lhs != 0.0 || lhs.is_sign_negative() == rhs.is_sign_negative())) }}"
                )
            }
            (Some(Type::Function(_)), Some(Type::Function(_))) if lhs_ty == rhs_ty => {
                self.function_ptr_eq_text(
                    &self.operand_text(lhs)?,
                    &self.operand_text(rhs)?,
                    lhs_ty,
                )?
            }
            _ if self.is_reference_type(lhs_ty) || self.is_reference_type(rhs_ty) => {
                let lhs_identity =
                    self.reference_identity_text(&self.operand_text(lhs)?, lhs_ty);
                let rhs_identity =
                    self.reference_identity_text(&self.operand_text(rhs)?, rhs_ty);
                match (lhs_identity, rhs_identity) {
                    (Some(left), Some(right)) => format!("{left} == {right}"),
                    // Either a reference value whose representation has no id,
                    // or a reference compared against a non-reference: neither
                    // can be the same JS reference.
                    _ => "false".to_owned(),
                }
            }
            _ => return Ok(None),
        };
        Ok(Some(if op == smelt_hir::BinOp::StrictNotEq {
            format!("!({equal_text})")
        } else {
            equal_text
        }))
    }

    /// Emits equality for structurally compatible values with erased members.
    ///
    /// Test assertions often compare a concrete expected value with a result
    /// whose item type was erased by a generic Remeda entrypoint, for example
    /// `Vec<SmeltUnknown>` against `Vec<f64>`. Rust needs both sides to have the
    /// same `PartialEq` shape, so this coerces the concrete side into the erased
    /// container before comparing.
    pub(super) fn heterogeneous_equality_text(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
    ) -> Result<Option<String>, EmitError> {
        if !matches!(
            op,
            smelt_hir::BinOp::Eq
                | smelt_hir::BinOp::NotEq
                | smelt_hir::BinOp::StrictEq
                | smelt_hir::BinOp::StrictNotEq
                | smelt_hir::BinOp::JsStrictEq
                | smelt_hir::BinOp::JsStrictNotEq
        ) {
            return Ok(None);
        }
        let lhs_ty = self.operand_ty(lhs)?;
        let rhs_ty = self.operand_ty(rhs)?;
        if lhs_ty == rhs_ty {
            return Ok(None);
        }
        if self.equality_shapes_are_definitely_incompatible(lhs_ty, rhs_ty) {
            let text = "false".to_owned();
            return Ok(Some(
                if matches!(
                    op,
                    smelt_hir::BinOp::NotEq
                        | smelt_hir::BinOp::StrictNotEq
                        | smelt_hir::BinOp::JsStrictNotEq
                ) {
                    format!("!({text})")
                } else {
                    text
                },
            ));
        }
        let lhs_needs_erased = self.type_contains_unknown(lhs_ty)
            || matches!(
                self.mir.types.get(lhs_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            )
            || self.is_erased_class_type(lhs_ty);
        let rhs_needs_erased = self.type_contains_unknown(rhs_ty)
            || matches!(
                self.mir.types.get(rhs_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            )
            || self.is_erased_class_type(rhs_ty);
        let text = if lhs_needs_erased && !rhs_needs_erased {
            format!(
                "{} == {}",
                self.operand_text(lhs)?,
                self.value_at_type(rhs, lhs_ty)?
            )
        } else if rhs_needs_erased && !lhs_needs_erased {
            if matches!(self.mir.types.get(lhs_ty), Some(Type::Tuple(_)))
                && matches!(self.mir.types.get(rhs_ty), Some(Type::List(_)))
            {
                format!("{} == {}", self.erase(lhs)?, self.erase(rhs)?)
            } else {
                format!(
                    "{} == {}",
                    self.value_at_type(lhs, rhs_ty)?,
                    self.operand_text(rhs)?
                )
            }
        } else {
            return Ok(None);
        };
        Ok(Some(if op == smelt_hir::BinOp::NotEq {
            format!("!({text})")
        } else {
            text
        }))
    }

    /// Returns whether two static types cannot be equal under JavaScript-style deep equality.
    fn equality_shapes_are_definitely_incompatible(&self, left: TypeId, right: TypeId) -> bool {
        match (self.mir.types.get(left), self.mir.types.get(right)) {
            (Some(Type::Int | Type::Float), Some(Type::Int | Type::Float)) => false,
            (Some(Type::Optional(left_inner)), Some(Type::Optional(right_inner))) => {
                self.equality_shapes_are_definitely_incompatible(*left_inner, *right_inner)
            }
            (Some(Type::Optional(inner)), _) => {
                self.equality_shapes_are_definitely_incompatible(*inner, right)
            }
            (_, Some(Type::Optional(inner))) => {
                self.equality_shapes_are_definitely_incompatible(left, *inner)
            }
            (Some(Type::List(_)), Some(Type::List(_)))
            | (Some(Type::Dict(_, _)), Some(Type::Dict(_, _)))
            | (Some(Type::Set(_)), Some(Type::Set(_))) => false,
            (Some(Type::Tuple(left_items)), Some(Type::Tuple(right_items))) => {
                left_items.len() != right_items.len()
                    || left_items
                        .iter()
                        .zip(right_items.iter())
                        .any(|(left_item, right_item)| {
                            self.equality_shapes_are_definitely_incompatible(
                                *left_item,
                                *right_item,
                            )
                        })
            }
            // A tuple (e.g. `splitAt`'s `[T[], T[]]`) and a list literal of the
            // same shape are structurally comparable once both are erased to
            // `SmeltUnknown::Array`; they are NOT definitely incompatible. Let the
            // erase-both equality path handle them instead of folding to `false`.
            (Some(Type::Tuple(_)), Some(Type::List(_)))
            | (Some(Type::List(_)), Some(Type::Tuple(_))) => false,
            (
                Some(
                    Type::List(_)
                    | Type::Dict(_, _)
                    | Type::Set(_)
                    | Type::Tuple(_)
                    | Type::Function(_),
                ),
                Some(
                    Type::List(_)
                    | Type::Dict(_, _)
                    | Type::Set(_)
                    | Type::Tuple(_)
                    | Type::Function(_),
                ),
            ) => true,
            (
                Some(Type::Bool | Type::Int | Type::Float | Type::String | Type::None),
                Some(Type::Bool | Type::Int | Type::Float | Type::String | Type::None),
            ) => {
                !matches!(
                    (self.mir.types.get(left), self.mir.types.get(right)),
                    (Some(Type::Int | Type::Float), Some(Type::Int | Type::Float))
                        | (Some(Type::None), Some(Type::None))
                ) && self.mir.types.get(left) != self.mir.types.get(right)
            }
            (
                Some(Type::Class { .. }),
                Some(Type::Bool | Type::Int | Type::Float | Type::String | Type::None),
            )
            | (
                Some(Type::Bool | Type::Int | Type::Float | Type::String | Type::None),
                Some(Type::Class { .. }),
            ) => true,
            _ => false,
        }
    }

    /// Returns the inner type for `Option<T>`.
    fn optional_inner_ty(&self, ty: TypeId) -> Option<TypeId> {
        match self.mir.types.get(ty) {
            Some(Type::Optional(inner)) => Some(*inner),
            _ => None,
        }
    }

    /// Emits arithmetic involving erased operands through JavaScript-like numbers.
    pub(super) fn erased_arithmetic_text(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
        dest_ty: TypeId,
    ) -> Result<Option<String>, EmitError> {
        if !matches!(
            op,
            smelt_hir::BinOp::Add
                | smelt_hir::BinOp::Sub
                | smelt_hir::BinOp::Mul
                | smelt_hir::BinOp::Div
                | smelt_hir::BinOp::Rem
        ) {
            return Ok(None);
        }
        let lhs_ty = self.operand_ty(lhs)?;
        let rhs_ty = self.operand_ty(rhs)?;
        let lhs_erased = matches!(
            self.mir.types.get(lhs_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(lhs_ty);
        let rhs_erased = matches!(
            self.mir.types.get(rhs_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(rhs_ty);
        let has_non_add_string = op != smelt_hir::BinOp::Add
            && matches!(
                (self.mir.types.get(lhs_ty), self.mir.types.get(rhs_ty)),
                (Some(Type::String), _) | (_, Some(Type::String))
            );
        if !lhs_erased && !rhs_erased && !has_non_add_string {
            return Ok(None);
        }
        let float_ty = self.type_id(Type::Float)?;
        let lhs_text = self.value_at_type(lhs, float_ty)?;
        let rhs_text = self.value_at_type(rhs, float_ty)?;
        // The combined `lhs <op> rhs` is a binary expression: a loose operand
        // that must be parenthesized before it becomes a cast operand
        // (`(...) as f64`) or a method receiver (`(...).to_string()`). Carrying
        // its precedence in a `RenderedValue` lets the value wrap itself instead
        // of relying on each arm below to remember the parentheses by hand.
        let numeric = RenderedValue::with_precedence(
            format!("{lhs_text} {} {rhs_text}", smelt_hir::bin_op_text(op)),
            float_ty,
            Precedence::NeedsParens,
        );
        Ok(Some(match self.mir.types.get(dest_ty) {
            Some(Type::Int) => format!(
                "({} as f64).trunc() as i64",
                numeric.parenthesized_if_needed()
            ),
            Some(Type::Float) => numeric.into_text(),
            Some(Type::String) => {
                format!("{}.to_string()", numeric.parenthesized_if_needed())
            }
            // The erased target is already an `f64` value in argument position
            // (no reassociation); the coercion seam owns the `Number` tag text.
            _ => self.erase_f64_text(&numeric.into_text()),
        }))
    }

    /// Returns whether a type is a Rust numeric scalar.
    fn is_numeric_type(&self, ty: TypeId) -> bool {
        matches!(self.mir.types.get(ty), Some(Type::Int | Type::Float))
    }

    /// Returns whether a type is erased for the purpose of a JS relational
    /// comparison — an `unknown`, a union (concrete or erased), a generic type
    /// param, or an erased class type. Such values must be coerced with
    /// `ToNumber` before an `f64` `<`/`>`/`<=`/`>=` comparison resolves.
    fn is_erased_relational(&self, ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(ty)
    }

    /// Chooses the Rust scalar type for mixed numeric arithmetic.
    fn common_numeric_type(
        &self,
        lhs: TypeId,
        rhs: TypeId,
        dest_ty: TypeId,
    ) -> Result<TypeId, EmitError> {
        if self.is_numeric_type(dest_ty) {
            return Ok(dest_ty);
        }
        if matches!(self.mir.types.get(lhs), Some(Type::Float))
            || matches!(self.mir.types.get(rhs), Some(Type::Float))
        {
            return self.type_id(Type::Float);
        }
        self.type_id(Type::Int)
    }

    /// Emits an `Option<T>` value expression with a default for arithmetic.
    fn option_value_text(&self, operand: &Operand, inner: TypeId) -> Result<String, EmitError> {
        let fallback = match self.mir.types.get(inner) {
            Some(Type::Int) => "0_i64",
            Some(Type::Float) => "0.0",
            _ => return Ok(self.operand_text(operand)?),
        };
        Ok(format!(
            "{}.unwrap_or({fallback})",
            self.operand_text(operand)?
        ))
    }

    /// Emits an unwrapped optional arithmetic operand coerced to `target`.
    fn option_value_as_type_text(
        &self,
        operand: &Operand,
        inner: TypeId,
        target: TypeId,
    ) -> Result<String, EmitError> {
        let value = self.option_value_text(operand, inner)?;
        self.value_at_type_text(&value, inner, target)
    }
}
