//! List emission helpers.

use crate::rust::RustExpr;
use super::*;

/// The block-local recursive flattener both erased `flat` renderings emit.
///
/// Flattens nested `SmeltUnknown::Array` values up to the remaining depth and
/// keeps every other value as a leaf.
const SMELT_FLAT_VALUES_FN: &str = "fn smelt_flat_values(values: Vec<SmeltUnknown>, smelt_remaining_depth: i64) -> Vec<SmeltUnknown> { if smelt_remaining_depth <= 0 { return values; } values.into_iter().flat_map(|value| match value { SmeltUnknown::Array(items) => smelt_flat_values(items.into_vec(), smelt_remaining_depth - 1), value => vec![value] }).collect::<Vec<_>>() }";

impl FunctionEmitter<'_> {
    /// Drains a statically typed synchronous generator into a fresh `SmeltList`.
    ///
    /// The frontend lowers `Array.from(gen)` / `[...gen]` over a
    /// `Generator<Y, R, N>` to a cast into `List<T>`. The generator's items are
    /// already statically `Y`, so this collects them through the prelude's
    /// `SmeltGenerator::collect_yields` and converts each `Y` to `T` with the
    /// ordinary typed coercion (identity when `Y == T`), instead of erasing the
    /// generator to its tagged iterator-protocol object. Returns `None` when the
    /// source is not a sync generator or the target is not a list.
    pub(super) fn generator_items_expr(
        &self,
        value_text: &RustExpr,
        source: TypeId,
        target: TypeId,
    ) -> Result<Option<RustExpr>, EmitError> {
        let Some(&Type::Generator {
            is_async: false,
            yield_ty,
            ..
        }) = self.mir.types.get(source)
        else {
            return Ok(None);
        };
        let Some(&Type::List(item_ty)) = self.mir.types.get(target) else {
            return Ok(None);
        };
        let collected = format!("({value_text}).collect_yields()", value_text = value_text.delimited());
        if yield_ty == item_ty {
            return Ok(Some(RustExpr::atom(format!("SmeltList::new({collected})"))));
        }
        let item_text =
            self.rendered_at_type(&RustExpr::path("smelt_value"), yield_ty, item_ty, &self.render_scope())?;
        Ok(Some(RustExpr::atom(format!(
            "SmeltList::new({collected}.into_iter().map(|smelt_value| {item_text}).collect::<Vec<_>>())", item_text = item_text.delimited()
        ))))
    }

    /// Converts a list containment operation to Rust text.
    pub(super) fn list_contains_expr(
        &self,
        list: &Operand,
        item: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let Some((receiver_text, item_ty)) = self.list_receiver_surface(list)? else {
            return Ok(RustExpr::path("false"));
        };
        // Every use below is a `Vec` receiver (`.iter().any(..)`, `.contains(..)`),
        // so the elements are read out of the list's shared buffer.
        let list_text = list_read_expr(&receiver_text);
        let needle_ty = self.operand_ty(item)?;
        if needle_ty != item_ty {
            // A `T | undefined` needle (e.g. `array.includes(sample(array))`)
            // still checks containment against a `Vec<T>`: unwrap the optional
            // and guard on `Some`, since a `None`/`undefined` needle is never an
            // element of a list of non-optional `T`.
            if let Some(Type::Optional(opt_inner)) = self.mir.types.get(needle_ty) {
                let inner = *opt_inner;
                let compare = if inner == item_ty {
                    self.list_element_match_predicate(item_ty).to_owned()
                } else if matches!(
                    self.mir.types.get(inner),
                    Some(Type::Unknown | Type::TypeParam { .. })
                ) {
                    // An erased needle (`unknown | undefined`) compares against
                    // each element after the element is erased to the runtime
                    // `SmeltUnknown` value, matching JS `includes` semantics.
                    let erased_item = self.erase_rendered(&RustExpr::path("smelt_item").cloned_value(), item_ty)?;
                    self.carrier_format(self.carrier()?, format_args!("({erased_item}).same_js_key(&smelt_needle)", erased_item = erased_item.delimited()))
                } else {
                    return Ok(RustExpr::path("false"));
                };
                let item_text = self.operand_expr(item)?;
                return Ok(RustExpr::atom(format!(
                    "{{ match {item_text} {{ Some(smelt_needle) => {list_text}.iter().any(|smelt_item| {compare}), None => false }} }}", item_text = item_text.delimited(), list_text = list_text.tight()
                )));
            }
            // A needle that is one member of a union element type
            // (`(number | string)[]` searched for `"x"`) is injected into the
            // union and compared with SameValueZero, as `indexOf` does. A
            // constant `false` here answered every such `includes` wrongly.
            if self.union_has_member(item_ty, needle_ty) {
                let item_text =
                    self.rendered_at_type(&self.operand_expr(item)?, needle_ty, item_ty, &self.render_scope())?;
                return Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                    "{{ let smelt_needle = {item_text}; {list_text}.iter().any(|smelt_item| smelt_item.same_js_key(&smelt_needle)) }}", item_text = item_text.delimited(), list_text = list_text.tight()
                ))));
            }
            return Ok(RustExpr::path("false"));
        }
        let item_text = self.operand_expr(item)?;
        if self.list_item_uses_same_value_zero(item_ty) {
            if self.mir.types.get(item_ty) == Some(&Type::Float) {
                return Ok(RustExpr::atom(format!(
                    "{{ let smelt_needle = {item_text}; {list_text}.iter().any(|smelt_item| *smelt_item == smelt_needle || (smelt_item.is_nan() && smelt_needle.is_nan())) }}", item_text = item_text.delimited(), list_text = list_text.tight()
                )));
            }
            return Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                "{{ let smelt_needle = {item_text}; {list_text}.iter().any(|smelt_item| smelt_item.same_js_key(&smelt_needle)) }}", item_text = item_text.delimited(), list_text = list_text.tight()
            ))));
        }
        Ok(list_text.postfix(format!(".contains(&{item_text})", item_text = item_text.tight())))
    }

    /// Whether `union` is a union type with `member` among its members.
    fn union_has_member(&self, union: TypeId, member: TypeId) -> bool {
        matches!(self.mir.types.get(union), Some(Type::Union(members)) if members.contains(&member))
    }

    /// Resolve a containment receiver to `Vec`-backed Rust text plus its element
    /// type, or `None` when the operand has no single list surface.
    ///
    /// A receiver declared as a union or an *optional* union
    /// (`chars?: string | string[]`, interned
    /// `Optional(Union([String, List(String)]))`) does have one list surface once
    /// the source has narrowed it — `switch (typeof chars) { case 'object': … }` —
    /// and the frontend only emits `Rvalue::ListContains` after proving that
    /// narrowing. MIR reads the value through its *declaring* local, though, so the
    /// operand type here is still the wide one. Matching `Type::List` alone
    /// therefore answered a constant `false`, which is what made es-toolkit's
    /// `trim`/`trimStart`/`trimEnd` array-`chars` loops never remove a character —
    /// ten specs, all silently wrong rather than failing.
    ///
    /// The projection is safe precisely because the frontend established the
    /// narrowing: `project_union_value` emits an `unreachable!` guard on the
    /// other arms, so a receiver that somehow held a different member would abort
    /// rather than answer a wrong `false`. A union with more than one list member is
    /// ambiguous and still yields `None`.
    fn list_receiver_surface(
        &self,
        list: &Operand,
    ) -> Result<Option<(RustExpr, TypeId)>, EmitError> {
        let list_ty = self.operand_ty(list)?;
        // The plain-list fast path reads by borrow: the only caller,
        // `list_contains_expr`, uses that text solely as an `.iter().any(..)` /
        // `.contains(&..)` receiver. The optional/union path below keeps an owned read
        // because `Option::expect` consumes `self` and the union projection may too.
        if let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) {
            return Ok(Some((self.operand_borrow_expr(list)?, *item_ty)));
        }
        // An optional receiver is unwrapped first: the narrowing that selected the
        // list arm also proved the value present, and the `expect` message matches
        // the one the optional-narrowing paths already emit.
        let (text, inner_ty) = match self.mir.types.get(list_ty) {
            Some(Type::Optional(inner)) => {
                let optional = self.operand_expr(list)?;
                (
                    optional.postfix(".expect(\"optional value was absent after narrowing\")"),
                    *inner,
                )
            }
            _ => (self.operand_expr(list)?, list_ty),
        };
        let Some(members) = self.concrete_union_members(inner_ty) else {
            return Ok(None);
        };
        let mut surface = None;
        for member in members {
            if let Some(Type::List(item_ty)) = self.mir.types.get(*member) {
                if surface.replace((*member, *item_ty)).is_some() {
                    return Ok(None);
                }
            }
        }
        let Some((member_ty, item_ty)) = surface else {
            return Ok(None);
        };
        let Some(projected) = self.project_union_value(&text, inner_ty, member_ty)? else {
            return Ok(None);
        };
        Ok(Some((RustExpr::from(format!("({projected})", projected = projected.delimited())), item_ty)))
    }

    /// Per-element comparison predicate against a bound `smelt_needle` value.    /// Per-element comparison predicate against a bound `smelt_needle` value.
    ///
    /// Mirrors the direct-needle containment comparison so an optional-needle
    /// containment check (`list.includes(sample(list))`) reuses the same
    /// SameValueZero / float-NaN / structural-identity semantics after the
    /// optional has been narrowed to `smelt_needle`. `item` is the closure
    /// binding for each `&element`.
    fn list_element_match_predicate(&self, item_ty: TypeId) -> &'static str {
        if self.list_item_uses_same_value_zero(item_ty) {
            if self.mir.types.get(item_ty) == Some(&Type::Float) {
                "*item == smelt_needle || (item.is_nan() && smelt_needle.is_nan())"
            } else {
                "item.same_js_key(&smelt_needle)"
            }
        } else {
            "*item == smelt_needle"
        }
    }

    /// Converts a list-to-set constructor conversion to Rust text.
    pub(super) fn list_to_set_expr(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list-to-set source must be a list"));
        };
        let set_item_ty = *item_ty;
        if !matches!(self.mir.types.get(dest_ty), Some(Type::Set(dest_item)) if *dest_item == set_item_ty)
        {
            return Err(EmitError::new(
                "list-to-set destination must be set of the list item type",
            ));
        }
        // Collect into whichever container the destination `Set` uses:
        // `SmeltPrimSet` (Rust equality, hash index) for value-equality
        // primitives, or `SmeltJsSet` (SameValueZero membership) for `f64`,
        // unions, generics, and object-like elements. Both are
        // insertion-ordered and both dedup on `FromIterator`, so
        // `new Set(array)` keeps first-appearance order as JS requires.
        Ok(RustExpr::opaque(format!(
            "{}.iter().cloned().collect::<{}<_>>()",
            list_read_expr(&self.operand_expr(list)?).tight(),
            self.set_container_name(set_item_ty)
        )))
    }

    /// Converts a list of key/value pair tuples to the destination map (a
    /// Python `dict` or a source `Map`), later pairs overwriting earlier keys.
    pub(super) fn list_pairs_to_dict_expr(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("dict() pair source must be a list"));
        };
        let Some(Type::Tuple(items)) = self.mir.types.get(*item_ty) else {
            return Err(EmitError::new(
                "dict() pair source must contain tuple items",
            ));
        };
        let [key_ty, value_ty] = items.as_slice() else {
            return Err(EmitError::new(
                "dict() pair source tuples must have length two",
            ));
        };
        // A Python `dict(pairs)` collects into a `Dict`, a source
        // `new Map(entries)` into a `JsMap`; either way the destination's
        // components are the pairs' own.
        if !matches!(
            self.mir.types.get(dest_ty),
            Some(Type::Dict(dest_key, dest_value) | Type::JsMap(dest_key, dest_value))
                if dest_key == key_ty && dest_value == value_ty
        ) {
            return Err(EmitError::new(
                "dict() destination must match pair key and value types",
            ));
        }
        // Collected into the destination's own container, whichever the dict
        // renders as.
        let container = self.rendered_type(dest_ty)?;
        Ok(RustExpr::loose(format!(
            "{}.iter().cloned().collect::<{container}>()",
            list_read_expr(&self.operand_expr(list)?).tight()
        )))
    }

    /// Converts a list concatenation operation to Rust text.
    /// Converts a list concatenation operation to Rust text.
    pub(super) fn list_concat_expr(
        &self,
        left: &Operand,
        right: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let left_ty = self.operand_ty(left)?;
        let right_ty = self.operand_ty(right)?;
        let left_erased = self.has_dynamic_shape(left_ty);
        let right_erased = self.has_dynamic_shape(right_ty);
        if left_erased || right_erased {
            // Coerce both operands to the materialized result list type. When one
            // side is a concrete `List<X>` (e.g. the empty `[]` synthesized for an
            // erased `[...data]` spread), prefer `List<X>` over `List<Unknown>` so
            // the erased side's elements are unwrapped to the concrete element type
            // and `List<Unknown>` need not be interned for this function.
            let list_ty = self.concat_result_list_ty(left, right)?;
            let left_text = self.rendered_at_type(&self.operand_expr(left)?, left_ty, list_ty, &self.render_scope())?;
            let right_text =
                self.rendered_at_type(&self.operand_expr(right)?, right_ty, list_ty, &self.render_scope())?;
            let left_read = list_read_expr(&left_text);
            let right_read = list_read_expr(&right_text);
            return Ok(RustExpr::atom_on(&left_read, format!(
                "{left_read}.iter().cloned().chain({right_read}.iter().cloned()).collect::<Vec<_>>()", left_read = left_read.tight(), right_read = right_read.tight()
            )));
        }
        // A concatenation the emitter cannot type must be an error, never a
        // plausible value: `Default::default()` for a list type is the EMPTY
        // list, so the two operands were silently discarded and every caller
        // read an empty array back from a concat that had real elements.
        let (Some(Type::List(left_item)), Some(Type::List(right_item))) =
            (self.mir.types.get(left_ty), self.mir.types.get(right_ty))
        else {
            return Err(EmitError::new(format!(
                "list concatenation needs two list operands, got {:?} and {:?}",
                self.mir.types.get(left_ty),
                self.mir.types.get(right_ty)
            )));
        };
        // `concat` (and spread) always yields a NEW array, so even concatenating
        // an empty list produces a fresh reference identity, not an alias.
        if matches!(self.mir.types.get(*left_item), Some(Type::Never))
            && self.is_empty_list_operand(left)?
        {
            return Ok(self.operand_expr(right)?.postfix(".fresh_copy()"));
        }
        if matches!(self.mir.types.get(*right_item), Some(Type::Never))
            && self.is_empty_list_operand(right)?
        {
            return Ok(self.operand_expr(left)?.postfix(".fresh_copy()"));
        }
        let has_never_item = matches!(self.mir.types.get(*left_item), Some(Type::Never))
            || matches!(self.mir.types.get(*right_item), Some(Type::Never));
        if left_item != right_item && !has_never_item {
            // Mixed element types. `[...Object.keys(o), ...getSymbols(o)]` is a
            // `(string | symbol)[]`: `List<String>` chained onto `List<Unknown>`.
            // Both sides collect into one `Vec`, so when one element type is
            // already the erased `SmeltUnknown` the other side's elements are
            // erased into it. The `Default::default()` fallback below produced an
            // EMPTY list, which is how es-toolkit's `copyProperties` silently lost
            // every string key once symbols stopped arriving as bare descriptions.
            if let Some(unknown_ty) = self.find_type_id(&Type::Unknown) {
                let left_text = list_read_expr(&self.operand_expr(left)?);
                let right_text = list_read_expr(&self.operand_expr(right)?);
                if *right_item == unknown_ty {
                    let item = self.erase_rendered(&RustExpr::path("smelt_item"), *left_item)?;
                    return Ok(RustExpr::atom_on(&left_text, format!(
                        "{left_text}.iter().cloned().map(|smelt_item| {item}).chain({right_text}.iter().cloned()).collect::<Vec<_>>()", left_text = left_text.tight(), item = item.delimited(), right_text = right_text.tight()
                    )));
                }
                if *left_item == unknown_ty {
                    let item = self.erase_rendered(&RustExpr::path("smelt_item"), *right_item)?;
                    return Ok(RustExpr::atom_on(&left_text, format!(
                        "{left_text}.iter().cloned().chain({right_text}.iter().cloned().map(|smelt_item| {item})).collect::<Vec<_>>()", left_text = left_text.tight(), right_text = right_text.tight(), item = item.delimited()
                    )));
                }
            }
            // Neither element type absorbs the other directly, but the crate
            // may already name a type both convert into — typically the union
            // that covers both operands' members. `[...xs, ...ys]` over two
            // different concrete unions is exactly that shape.
            if let Some(list_ty) = self.narrowest_common_list_ty(*left_item, *right_item) {
                let left_text =
                    self.rendered_at_type(&self.operand_expr(left)?, left_ty, list_ty, &self.render_scope())?;
                let right_text =
                    self.rendered_at_type(&self.operand_expr(right)?, right_ty, list_ty, &self.render_scope())?;
                return Ok({ let receiver = list_read_expr(&left_text); RustExpr::atom_on(&receiver, format!("{receiver}.iter().cloned().chain({}.iter().cloned()).collect::<Vec<_>>()", list_read_expr(&right_text).tight(), receiver = receiver.tight())) });
            }
            // Same rule as above: neither element type can absorb the other, so
            // there is no correct text — an empty list is a wrong answer, not a
            // fallback.
            return Err(EmitError::new(format!(
                "list concatenation cannot reconcile element types {:?} and {:?}",
                self.mir.types.get(*left_item),
                self.mir.types.get(*right_item)
            )));
        }
        Ok({ let receiver = list_read_expr(&self.operand_expr(left)?); RustExpr::atom_on(&receiver, format!("{receiver}.iter().cloned().chain({}.iter().cloned()).collect::<Vec<_>>()", list_read_expr(&self.operand_expr(right)?).tight(), receiver = receiver.tight())) })
    }

    /// Find the narrowest already-interned `List<E>` whose element type `E`
    /// both `left` and `right` convert into.
    ///
    /// `[...xs, ...ys]` collects into a single `Vec`, so its element type has to
    /// be one type that covers both operands. When the two element types are
    /// different concrete unions, that cover is the union of their members —
    /// and a crate that spells both unions almost always spells their union
    /// somewhere too, so it can be looked up rather than invented (the emitter
    /// reads the type table and cannot add to it).
    ///
    /// "Narrowest" is the candidate with the fewest members, so a concat never
    /// widens further than it must; the fully erased `Unknown` is the last
    /// resort and only when the crate already interns `List<Unknown>`. Returns
    /// `None` when no interned type covers both — the caller then reports an
    /// emit error rather than guessing.
    fn narrowest_common_list_ty(&self, left: TypeId, right: TypeId) -> Option<TypeId> {
        let mut needed = self.type_union_members(left);
        needed.extend(self.type_union_members(right));
        needed.sort_unstable_by_key(|member| member.0);
        needed.dedup();
        let mut best: Option<(usize, TypeId)> = None;
        for (index, candidate) in self.mir.types.all().iter().enumerate() {
            let Type::Union(members) = candidate else {
                continue;
            };
            if !needed.iter().all(|member| members.contains(member)) {
                continue;
            }
            let Some(candidate_ty) = compact_index(index, "type index does not fit u32")
                .ok()
                .map(TypeId)
            else {
                continue;
            };
            let Some(list_ty) = self.find_type_id(&Type::List(candidate_ty)) else {
                continue;
            };
            if best.is_none_or(|(width, _)| members.len() < width) {
                best = Some((members.len(), list_ty));
            }
        }
        if let Some((_, list_ty)) = best {
            return Some(list_ty);
        }
        let unknown_ty = self.find_type_id(&Type::Unknown)?;
        self.find_type_id(&Type::List(unknown_ty))
    }

    /// Return a type's union members, or the type itself when it is not a union.
    fn type_union_members(&self, ty: TypeId) -> Vec<TypeId> {
        match self.mir.types.get(ty) {
            Some(Type::Union(members)) => members.clone(),
            _ => vec![ty],
        }
    }

    /// Compute the list type the materialized concat result carries.
    ///
    /// `[...left, ...right]` collects into a single `Vec`, so both operands must
    /// be coerced to one element type. When both are concrete `List`s with the
    /// same item (or one is an empty `List<Never>`) that shared item wins. When
    /// exactly one side is erased (`Unknown`/`TypeParam`/`Union`/erased class) and
    /// the other is a concrete `List<X>`, the concrete `List<X>` wins — the erased
    /// side's runtime `SmeltUnknown` elements are unwrapped to `X`, and `List<X>`
    /// is guaranteed interned because the concrete operand carries it. Only when no
    /// operand offers a concrete list type do we fall back to `List<Unknown>`.
    pub(super) fn concat_result_list_ty(
        &self,
        left: &Operand,
        right: &Operand,
    ) -> Result<TypeId, EmitError> {
        let left_ty = self.operand_ty(left)?;
        let right_ty = self.operand_ty(right)?;
        let left_item = match self.mir.types.get(left_ty) {
            Some(Type::List(item)) => Some(*item),
            _ => None,
        };
        let right_item = match self.mir.types.get(right_ty) {
            Some(Type::List(item)) => Some(*item),
            _ => None,
        };
        if let (Some(left_item_ty), Some(right_item_ty)) = (left_item, right_item) {
            if left_item_ty == right_item_ty
                || matches!(self.mir.types.get(right_item_ty), Some(Type::Never))
            {
                return self.type_id(Type::List(left_item_ty));
            }
            if matches!(self.mir.types.get(left_item_ty), Some(Type::Never)) {
                return self.type_id(Type::List(right_item_ty));
            }
            // Two concrete, different element types: the same rule
            // `list_concat_expr` renders with, so the type and the text agree.
            // An erased side absorbs the other; otherwise the narrowest
            // interned union covering both is the element type.
            if !self.has_dynamic_shape(left_ty) && !self.has_dynamic_shape(right_ty) {
                let unknown_ty = self.find_type_id(&Type::Unknown);
                if unknown_ty.is_some_and(|unknown| unknown == left_item_ty || unknown == right_item_ty) {
                    return self.type_id(Type::List(unknown_ty.unwrap_or(left_item_ty)));
                }
                if let Some(list_ty) = self.narrowest_common_list_ty(left_item_ty, right_item_ty) {
                    return Ok(list_ty);
                }
            }
        }
        if let Some(left_item_ty) = left_item
            && self.has_dynamic_shape(right_ty)
        {
            return self.type_id(Type::List(left_item_ty));
        }
        if let Some(right_item_ty) = right_item
            && self.has_dynamic_shape(left_ty)
        {
            return self.type_id(Type::List(right_item_ty));
        }
        let unknown_ty = self.type_id(Type::Unknown)?;
        self.type_id(Type::List(unknown_ty))
    }


    /// Return whether an operand is statically known to be an empty list.
    fn is_empty_list_operand(&self, operand: &Operand) -> Result<bool, EmitError> {
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = operand
        else {
            return Ok(false);
        };
        for block in &self.function.blocks {
            for statement in &block.statements {
                let Statement::Assign { dest, value } = statement else {
                    continue;
                };
                if dest == local && matches!(value, Rvalue::List(items) if items.is_empty()) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Inline a local list-concat temporary when it is consumed as a spread
    /// argument vector before regular statement emission preserved it.
    pub(super) fn inline_list_concat_operand_expr(
        &self,
        operand: &Operand,
    ) -> Result<Option<RustExpr>, EmitError> {
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = operand
        else {
            return Ok(None);
        };
        for block in &self.function.blocks {
            for statement in &block.statements {
                let Statement::Assign { dest, value } = statement else {
                    continue;
                };
                if dest != local {
                    continue;
                }
                if let Rvalue::ListConcat { left, right } = value {
                    return self.list_concat_expr(left, right).map(Some);
                }
            }
        }
        Ok(None)
    }

    /// Converts a list slice operation to Rust text.
    pub(super) fn list_slice_expr(
        &self,
        list: &Operand,
        start: Option<&Operand>,
        end: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let list_ty = self.operand_ty(list)?;
        if self.has_dynamic_shape(list_ty)
        {
            return self.unknown_list_slice_expr(list, start, end, dest_ty);
        }
        let Some(Type::List(source_item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list slice receiver must be a list"));
        };
        self.validate_optional_numeric_index(start, "list slice start index")?;
        self.validate_optional_numeric_index(end, "list slice end index")?;
        // Every use of `list_text` below is a `&self` receiver (`.len()`, `.iter()`),
        // so this reads the list by borrow. Cloning here copied the whole backing
        // `Vec` up to three times per slice, which inside a loop over the same list
        // turned a linear algorithm quadratic.
        let list_text = self.operand_borrow_expr(list)?;
        let len_source = list_text.postfix(".len()");
        let start_text = self.slice_start_expr(start, &len_source)?;
        let len_expr = self.slice_len_expr(&list_text, start, end)?;
        // `.len()` is the list's own inherent method (its own short-lived borrow);
        // `.iter()` needs the backing `Vec`, so it reads through the shared cell.
        // Both are shared borrows, so they may coexist inside one expression.
        let list_read = list_read_expr(&list_text);
        let Some(Type::List(dest_item_ty)) = self.mir.types.get(dest_ty) else {
            // The destination is not a concrete list (e.g. an erased
            // `SmeltUnknown` slot, as in `ary`'s rest-argument capping).
            // Materialize the slice as a real `SmeltList` with a fresh id (JS
            // `slice` returns a new array) and coerce it to the destination so
            // it is erased, rather than leaking a bare `Vec` into a
            // `SmeltUnknown` place (E0308).
            let sliced = RustExpr::atom(format!(
                "SmeltList::with_id(smelt_next_object_id(), {list_read}.iter().skip({start_text}).take({len_expr}).cloned().collect::<Vec<_>>())", list_read = list_read.tight(), start_text = start_text.delimited(), len_expr = len_expr.delimited()
            ));
            return self.rendered_at_type(&sliced, list_ty, dest_ty, &self.render_scope());
        };
        if source_item_ty == dest_item_ty {
            return Ok(RustExpr::atom_on(&list_read, format!(
                "{list_read}.iter().skip({start_text}).take({len_expr}).cloned().collect::<Vec<_>>()", list_read = list_read.tight(), start_text = start_text.delimited(), len_expr = len_expr.delimited()
            )));
        }
        let item_text = { self.rendered_at_type(&RustExpr::path("smelt_value"), *source_item_ty, *dest_item_ty, &self.render_scope())? };
        Ok(RustExpr::atom_on(&list_read, format!(
            "{list_read}.iter().skip({start_text}).take({len_expr}).cloned().map(|smelt_value| {item_text}).collect::<Vec<_>>()", list_read = list_read.tight(), start_text = start_text.delimited(), len_expr = len_expr.delimited(), item_text = item_text.delimited()
        )))
    }

    /// Converts `.slice(...)` on an erased value to Rust text.
    fn unknown_list_slice_expr(
        &self,
        list: &Operand,
        start: Option<&Operand>,
        end: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let float_ty = self.type_id(Type::Float)?;
        let list_text = self.erase(list)?;
        let start_text = start.map_or_else(
            || Ok(RustExpr::chain("0.0")),
            |operand| self.value_at_type(operand, float_ty),
        )?;
        let end_text = end.map_or_else(
            || Ok("None::<f64>".to_owned()),
            |operand| {
                self.value_at_type(operand, float_ty)
                    .map(|text| format!("Some({text})", text = text.delimited()))
            },
        )?;
        if self.has_dynamic_shape(dest_ty)
        {
            // Tag-preserving erased slice: an array receiver yields a fresh
            // `SmeltUnknown::Array` (JS `Array.prototype.slice` returns a new
            // array), a string receiver yields a `SmeltUnknown::String`, a
            // byte-backed host object (`ArrayBuffer`, `Buffer`, `DataView`) yields
            // a *fresh record of the same host identity* over the sliced bytes —
            // which is exactly what the `clone(buf)` idiom
            // (`return obj.slice(0)`) depends on to hand back a distinct object —
            // and any other value is forwarded unchanged so `.slice()` tolerates
            // it rather than panicking or ToString-coercing it.
            return Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                "{{ let smelt_slice_value = {list_text}; let smelt_slice_start = {start_text} as i64; let smelt_slice_end = {end_text}.map(|smelt_end| smelt_end as i64); if let Some(smelt_sliced) = {host_slice}(&smelt_slice_value, smelt_slice_start, smelt_slice_end) {{ smelt_sliced }} else {{ match smelt_slice_value {{ SmeltUnknown::Array(smelt_values) => {{ let smelt_values = smelt_values.into_vec(); let smelt_slice_len = smelt_values.len() as i64; let smelt_start_index = (if smelt_slice_start < 0 {{ smelt_slice_len + smelt_slice_start }} else {{ smelt_slice_start }}).clamp(0, smelt_slice_len); let smelt_end_index = smelt_slice_end.map_or(smelt_slice_len, |smelt_end| if smelt_end < 0 {{ smelt_slice_len + smelt_end }} else {{ smelt_end }}).clamp(0, smelt_slice_len); let smelt_take_len = smelt_end_index.saturating_sub(smelt_start_index) as usize; SmeltUnknown::Array(SmeltArray::with_id(smelt_next_object_id(), smelt_values.into_iter().skip(smelt_start_index as usize).take(smelt_take_len).collect::<Vec<_>>())) }}, SmeltUnknown::String(smelt_value) => {{ let smelt_slice_len = smelt_value.len() as i64; let smelt_start_index = (if smelt_slice_start < 0 {{ smelt_slice_len + smelt_slice_start }} else {{ smelt_slice_start }}).clamp(0, smelt_slice_len); let smelt_end_index = smelt_slice_end.map_or(smelt_slice_len, |smelt_end| if smelt_end < 0 {{ smelt_slice_len + smelt_end }} else {{ smelt_end }}).clamp(0, smelt_slice_len); SmeltUnknown::String(smelt_value.slice(smelt_start_index as usize, smelt_end_index as usize)) }}, smelt_other => smelt_other }} }} }}",
                host_slice = smelt_stdlib::runtime_symbols::byte_buffer::SLICE, list_text = list_text.delimited(), start_text = start_text.as_cast_operand(),
            ))));
        }
        let sliced_values_text = "smelt_slice_values.into_iter().skip(smelt_start_index as usize).take(smelt_take_len).collect::<Vec<_>>()";
        Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
            "{{ let smelt_slice_value = {list_text}; let smelt_slice_start = {start_text}; let smelt_slice_end = {end_text}; let smelt_slice_values = match smelt_slice_value {{ SmeltUnknown::Array(smelt_values) => smelt_values.into_vec(), SmeltUnknown::String(smelt_value) => smelt_value.code_point_strings().into_iter().map(SmeltUnknown::String).collect::<Vec<_>>(), _ => Vec::new() }}; let smelt_slice_len = smelt_slice_values.len() as i64; let smelt_start_index = smelt_slice_start as i64; let smelt_start_index = if smelt_start_index < 0 {{ smelt_slice_len + smelt_start_index }} else {{ smelt_start_index }}.clamp(0, smelt_slice_len); let smelt_end_index = smelt_slice_end.map_or(smelt_slice_len, |smelt_end| {{ let smelt_end = smelt_end as i64; if smelt_end < 0 {{ smelt_slice_len + smelt_end }} else {{ smelt_end }} }}).clamp(0, smelt_slice_len); let smelt_take_len = smelt_end_index.saturating_sub(smelt_start_index) as usize; {sliced_values_text} }}", list_text = list_text.delimited(), start_text = start_text.delimited()
        ))))
    }

    /// Converts an array splice or toSpliced operation to Rust text.
    pub(super) fn list_splice_expr(
        &self,
        list: &Operand,
        start: &Operand,
        delete_count: Option<&Operand>,
        items: &[MirListSpliceItem],
        mutate: bool,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("array splice receiver must be a list"));
        };
        self.validate_optional_numeric_index(Some(start), "array splice start index")?;
        self.validate_optional_numeric_index(delete_count, "array splice delete count")?;
        for item in items {
            let expected_ty = if item.spread { list_ty } else { *item_ty };
            if self.operand_ty(&item.value)? != expected_ty {
                return Err(EmitError::new(
                    "array splice replacement items must match the array element type",
                ));
            }
        }
        if dest_ty != list_ty {
            return Err(EmitError::new("array splice destination must be a list"));
        }
        let list_text = self.operand_expr(list)?;
        // Parenthesize the coerced index so a bare `x as f64` does not parse as
        // `x as f64 < 0.0` (rustc reads `<` as generic args after a type).
        let start_text = format!(
            "({})",
            self.value_at_type(start, self.type_id(Type::Float)?)?.delimited()
        );
        let delete_count_text = delete_count
            .map(|count| self.value_at_type(count, self.type_id(Type::Float)?))
            .transpose()?
            .unwrap_or_else(|| RustExpr::cast("smelt_splice_len as f64"));
        let replacement_text = self.list_splice_replacement_expr(items)?;
        let receiver_text = if mutate {
            let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = list
            else {
                return Err(EmitError::new(
                    "array splice receiver must be a mutable local for now",
                ));
            };
            list_write_expr(&self.local_value(*local)?)
        } else {
            list_text.postfix(".to_vec()")
        };
        let delete_count_float_text = format!("({delete_count_text} as f64)", delete_count_text = delete_count_text.as_cast_operand());
        if mutate {
            Ok(RustExpr::atom(format!(
                "{{ let smelt_splice_len = {receiver_text}.len(); let smelt_splice_start = if {start_text} < 0.0 {{ smelt_splice_len.saturating_sub((-{start_text}) as usize) }} else {{ ({start_text} as usize).min(smelt_splice_len) }}; let smelt_splice_delete = (({delete_count_float_text}).max(0.0) as usize).min(smelt_splice_len.saturating_sub(smelt_splice_start)); {receiver_text}.splice(smelt_splice_start..smelt_splice_start + smelt_splice_delete, {replacement_text}).collect::<Vec<_>>() }}", receiver_text = receiver_text.tight(), replacement_text = replacement_text.delimited()
            )))
        } else {
            Ok(RustExpr::atom(format!(
                "{{ let mut smelt_spliced = {receiver_text}; let smelt_splice_len = smelt_spliced.len(); let smelt_splice_start = if {start_text} < 0.0 {{ smelt_splice_len.saturating_sub((-{start_text}) as usize) }} else {{ ({start_text} as usize).min(smelt_splice_len) }}; let smelt_splice_delete = (({delete_count_float_text}).max(0.0) as usize).min(smelt_splice_len.saturating_sub(smelt_splice_start)); smelt_spliced.splice(smelt_splice_start..smelt_splice_start + smelt_splice_delete, {replacement_text}).for_each(drop); smelt_spliced }}", receiver_text = receiver_text.delimited(), replacement_text = replacement_text.delimited()
            )))
        }
    }

    /// Converts scalar and spread splice replacements into a Rust vector expression.
    fn list_splice_replacement_expr(
        &self,
        items: &[MirListSpliceItem],
    ) -> Result<RustExpr, EmitError> {
        if items.iter().all(|item| !item.spread) {
            let items_text = items
                .iter()
                .map(|item| self.operand_expr(&item.value))
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            return Ok(RustExpr::atom(format!("vec![{items_text}]")));
        }
        let mut statements = Vec::new();
        for item in items {
            let value_text = self.operand_expr(&item.value)?;
            if item.spread {
                statements.push(format!(
                    "smelt_splice_replacements.extend(({value_text}).borrow().iter().cloned());", value_text = value_text.delimited()
                ));
            } else {
                statements.push(format!("smelt_splice_replacements.push({value_text});", value_text = value_text.delimited()));
            }
        }
        Ok(RustExpr::atom(format!(
            "{{ let mut smelt_splice_replacements = Vec::new(); {} smelt_splice_replacements }}",
            statements.join(" ")
        )))
    }

    /// Converts an array fill operation to Rust text.
    pub(super) fn list_fill_expr(
        &self,
        list: &Operand,
        value: &Operand,
        start: Option<&Operand>,
        end: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("array fill receiver must be a list"));
        };
        if self.operand_ty(value)? != *item_ty || dest_ty != list_ty {
            return Err(EmitError::new(
                "array fill value and destination must match the list type",
            ));
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = list else {
            return Err(EmitError::new(
                "array fill receiver must be a mutable local for now",
            ));
        };
        let list_text = self.local_value(*local)?;
        let value_text = self.operand_expr(value)?;
        let start_text = start
            .map(|operand| self.operand_expr(operand).map(|text| format!("({text})", text = text.delimited())))
            .transpose()?
            .unwrap_or_else(|| "0.0".to_owned());
        let end_text = end
            .map(|operand| self.operand_expr(operand).map(|text| format!("({text})", text = text.delimited())))
            .transpose()?
            .unwrap_or_else(|| "(smelt_fill_len as f64)".to_owned());
        // The fill value is bound once, before the loop, so the write borrow of
        // the shared buffer never coexists with a read borrow of the same cell.
        let list_mut = list_write_expr(&list_text);
        Ok(RustExpr::atom(format!(
            "{{ let smelt_fill_len = {list_text}.len(); let smelt_fill_value = {value_text}; let smelt_fill_start = if {start_text} < 0.0 {{ smelt_fill_len.saturating_sub((-{start_text}) as usize) }} else {{ ({start_text} as usize).min(smelt_fill_len) }}; let smelt_fill_end = if {end_text} < 0.0 {{ smelt_fill_len.saturating_sub((-{end_text}) as usize) }} else {{ ({end_text} as usize).min(smelt_fill_len) }}; for smelt_fill_index in smelt_fill_start..smelt_fill_end {{ {list_mut}[smelt_fill_index] = smelt_fill_value.clone(); }} {list_text}.clone() }}", list_text = list_text.tight(), value_text = value_text.delimited(), list_mut = list_mut.tight()
        )))
    }

    /// Converts an array copyWithin operation to Rust text.
    pub(super) fn list_copy_within_expr(
        &self,
        list: &Operand,
        target: &Operand,
        start: &Operand,
        end: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let list_ty = self.operand_ty(list)?;
        if !matches!(self.mir.types.get(list_ty), Some(Type::List(_))) || dest_ty != list_ty {
            return Err(EmitError::new(
                "array copyWithin destination must match the list type",
            ));
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = list else {
            return Err(EmitError::new(
                "array copyWithin receiver must be a mutable local for now",
            ));
        };
        let list_text = self.local_value(*local)?;
        let target_text = format!("({})", self.operand_expr(target)?.delimited());
        let start_text = format!("({})", self.operand_expr(start)?.delimited());
        let end_text = end
            .map(|operand| self.operand_expr(operand).map(|text| format!("({text})", text = text.delimited())))
            .transpose()?
            .unwrap_or_else(|| "(smelt_copy_len as f64)".to_owned());
        // `copy_items` is snapshotted into its own `let`, so the read borrow of
        // the shared buffer is released before the write loop takes its own.
        let list_read = list_read_expr(&list_text);
        let list_mut = list_write_expr(&list_text);
        Ok(RustExpr::atom(format!(
            "{{ let smelt_copy_len = {list_text}.len(); let smelt_copy_target = if {target_text} < 0.0 {{ smelt_copy_len.saturating_sub((-{target_text}) as usize) }} else {{ ({target_text} as usize).min(smelt_copy_len) }}; let smelt_copy_start = if {start_text} < 0.0 {{ smelt_copy_len.saturating_sub((-{start_text}) as usize) }} else {{ ({start_text} as usize).min(smelt_copy_len) }}; let smelt_copy_end = if {end_text} < 0.0 {{ smelt_copy_len.saturating_sub((-{end_text}) as usize) }} else {{ ({end_text} as usize).min(smelt_copy_len) }}; let smelt_copy_items = {list_read}.iter().skip(smelt_copy_start).take(smelt_copy_end.saturating_sub(smelt_copy_start)).cloned().collect::<Vec<_>>(); for (smelt_offset, smelt_item) in smelt_copy_items.into_iter().enumerate() {{ if smelt_copy_target + smelt_offset < smelt_copy_len {{ {list_mut}[smelt_copy_target + smelt_offset] = smelt_item; }} }} {list_text}.clone() }}", list_text = list_text.tight(), list_read = list_read.tight(), list_mut = list_mut.tight()
        )))
    }

    /// Converts a list copy operation to Rust text.
    pub(super) fn list_copy_expr(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let list_ty = self.operand_ty(list)?;
        if !matches!(self.mir.types.get(list_ty), Some(Type::List(_))) {
            return Err(EmitError::new("list copy receiver must be a list"));
        }
        if dest_ty != list_ty {
            return Err(EmitError::new(
                "list copy destination must match the receiver list type",
            ));
        }
        // A JS array copy produces a NEW reference, so mint a fresh id rather
        // than sharing the source list's identity (`SmeltList::clone` shares it).
        Ok(self.operand_expr(list)?.postfix(".fresh_copy()"))
    }

    /// Converts an array `with` operation to Rust text.
    pub(super) fn list_with_expr(
        &self,
        list: &Operand,
        index: &Operand,
        value: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("array with receiver must be a list"));
        };
        if dest_ty != list_ty || self.operand_ty(value)? != *item_ty {
            return Err(EmitError::new(
                "array with value and destination must match the list type",
            ));
        }
        // The emitted body appends its own `.clone()` to the backing `Vec`: cloning
        // the `SmeltList` handle would share the array, and `with` leaves its input
        // alone.
        let list_text = list_read_expr(&self.operand_borrow_expr(list)?);
        let index_text = format!("({})", self.operand_expr(index)?.delimited());
        let value_text = self.operand_expr(value)?;
        Ok(RustExpr::atom(format!(
            "{{ let mut smelt_with_items = {list_text}.clone(); let smelt_with_len = smelt_with_items.len(); let smelt_with_index = if {index_text} < 0.0 {{ smelt_with_len.saturating_sub((-{index_text}) as usize) }} else {{ {index_text} as usize }}; if smelt_with_index >= smelt_with_len {{ panic!(\"array with index out of bounds\"); }} smelt_with_items[smelt_with_index] = {value_text}; smelt_with_items }}", list_text = list_text.tight(), value_text = value_text.delimited()
        )))
    }

    /// Converts JavaScript array flattening to Rust text.
    ///
    /// Nested typed lists retain the simple depth-one representation. Arrays
    /// crossing an erased `unknown` boundary need runtime recursion because
    /// their nested shape and requested depth are only available at execution.
    pub(super) fn list_flat_expr(
        &self,
        list: &Operand,
        depth: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let list_ty = self.operand_ty(list)?;
        if self.has_dynamic_shape(list_ty)
        {
            let Some(Type::List(item_ty)) = self.mir.types.get(dest_ty) else {
                return Err(EmitError::new("list flat destination must be a list"));
            };
            if self.mir.types.get(*item_ty) != Some(&Type::Unknown) {
                return Err(EmitError::new(
                    "erased array flat destination must be list[unknown]",
                ));
            }
            let depth_text = self.flat_depth_expr(depth)?;
            // Read by borrow: the emitted call appends its own
            // `.clone().into_vec()`, so cloning here made it `x.clone().clone()`.
            // The `match` below is written against a REFERENCE scrutinee
            // (`match &(..)`, arms cloning what they need) so it works whether the
            // place read is owned or is itself a borrowed callback parameter
            // (`callback_param_is_shared_reference`); moving out of the latter is
            // E0507.
            let list_text = self.operand_borrow_expr(list)?;
            return Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                "{{ {SMELT_FLAT_VALUES_FN} let smelt_flat_depth = (({depth_text}) as f64).max(0.0).floor() as i64; let smelt_flat_input = match &({list_text}) {{ SmeltUnknown::Array(smelt_values) => smelt_values.clone().into_vec(), SmeltUnknown::String(smelt_value) => smelt_value.code_point_strings().into_iter().map(SmeltUnknown::String).collect::<Vec<_>>(), _ => Vec::new() }}; smelt_flat_values(smelt_flat_input, smelt_flat_depth) }}", depth_text = depth_text.delimited(), list_text = list_text.delimited()
            ))));
        }
        let Some(Type::List(nested_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list flat receiver must be a list"));
        };
        if self.mir.types.get(*nested_ty) == Some(&Type::Unknown)
            && self.mir.types.get(dest_ty) == Some(&Type::List(*nested_ty))
        {
            let depth_text = self.flat_depth_expr(depth)?;
            return Ok(RustExpr::atom(format!(
                "{{ {SMELT_FLAT_VALUES_FN} let smelt_flat_depth = (({depth_text}) as f64).max(0.0).floor() as i64; smelt_flat_values({}.clone().into_vec(), smelt_flat_depth) }}",
                self.operand_expr(list)?.tight(), depth_text = depth_text.delimited()
            )));
        }
        if let Some(Type::Tuple(items)) = self.mir.types.get(*nested_ty) {
            if depth.is_some() {
                return Err(EmitError::new(
                    "array flat with explicit depth requires an erased nested list",
                ));
            }
            let Some(Type::List(dest_item_ty)) = self.mir.types.get(dest_ty) else {
                return Err(EmitError::new("array flat destination must be a list"));
            };
            let item_texts = items
                .iter()
                .enumerate()
                .map(|(index, item_ty)| {
                    self.rendered_at_type(
                        &RustExpr::path(format!("smelt_items.{index}")).cloned_value(),
                        *item_ty,
                        *dest_item_ty,
                        &self.render_scope(),
                    )
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            return Ok({ let receiver = list_read_expr(&self.operand_expr(list)?); RustExpr::atom_on(&receiver, format!("{receiver}.iter().flat_map(|smelt_items| vec![{item_texts}]).collect::<Vec<_>>()", receiver = receiver.tight())) });
        }
        if matches!(self.mir.types.get(*nested_ty), Some(Type::Union(_))) {
            return self.union_list_flat_expr(list, *nested_ty, depth, dest_ty);
        }
        let Some(Type::List(item_ty)) = self.mir.types.get(*nested_ty) else {
            // `flat` leaves an item that is not an array as it is.
            return self.value_at_type(list, dest_ty);
        };
        if depth.is_some() {
            return Err(EmitError::new(
                "array flat with explicit depth requires an erased nested list",
            ));
        }
        if self.mir.types.get(dest_ty) != Some(&Type::List(*item_ty)) {
            return Err(EmitError::new(
                "array flat destination must match nested item type",
            ));
        }
        // The outer list and each inner one are separate cells, and both reads
        // are shared borrows, so the nested `borrow()` is safe even when an inner
        // list aliases the outer one.
        Ok({ let receiver = list_read_expr(&self.operand_expr(list)?); RustExpr::atom_on(&receiver, format!("{receiver}.iter().flat_map(|smelt_items| smelt_items.borrow().clone().into_iter()).collect::<Vec<_>>()", receiver = receiver.tight())) })
    }

    /// `flat` over a list whose element type is a union with array arms
    /// (`(number | number[])[]`).
    ///
    /// The frontend typed the result element as TypeScript's `FlatArray` of
    /// the union (`lowering::union_flat`), so the arms nest to a depth only the
    /// runtime values decide. The flattening itself is the one an erased list
    /// takes (`smelt_flat_values`, JavaScript's `FlattenIntoArray`): each
    /// element is erased to its tagged value at the input boundary and each
    /// flattened value is checked back into the typed result element at the
    /// output boundary, so the value stays typed on both sides.
    fn union_list_flat_expr(
        &self,
        list: &Operand,
        item_ty: TypeId,
        depth: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let Some(Type::List(dest_item_ty)) = self.mir.types.get(dest_ty) else {
            return Err(EmitError::new("array flat destination must be a list"));
        };
        let unknown_ty = self.type_id(Type::Unknown)?;
        let erased_item = self.erase_rendered(&RustExpr::path("smelt_item").cloned_value(), item_ty)?;
        let typed_value = self.rendered_at_type(&RustExpr::path("smelt_value"), unknown_ty, *dest_item_ty, &self.render_scope())?;
        let depth_text = self.flat_depth_expr(depth)?;
        Ok(RustExpr::atom(format!(
            "{{ {SMELT_FLAT_VALUES_FN} let smelt_flat_depth = (({depth_text}) as f64).max(0.0).floor() as i64; let smelt_flat_input = {list_text}.iter().map(|smelt_item| {erased_item}).collect::<Vec<_>>(); smelt_flat_values(smelt_flat_input, smelt_flat_depth).into_iter().map(|smelt_value| {typed_value}).collect::<Vec<_>>() }}",
            list_text = list_read_expr(&self.operand_expr(list)?).tight(), depth_text = depth_text.delimited(), erased_item = erased_item.delimited(), typed_value = typed_value.delimited(),
        )))
    }

    /// Emits the JavaScript `Array.flat` depth expression as a runtime number.
    fn flat_depth_expr(&self, depth: Option<&Operand>) -> Result<RustExpr, EmitError> {
        match depth {
            None => Ok(RustExpr::path("1.0_f64")),
            Some(depth_operand) => {
                let text = self.operand_expr(depth_operand)?;
                match self.mir.types.get(self.operand_ty(depth_operand)?) {
                    Some(Type::Optional(inner))
                        if self.mir.types.get(*inner) == Some(&Type::Float) =>
                    {
                        Ok(text.postfix(".clone().unwrap_or(1.0)"))
                    }
                    Some(Type::Float) => Ok(text),
                    _ => Err(EmitError::new("array flat depth must be numeric")),
                }
            }
        }
    }

    /// Converts array keys, values, and entries projections to Rust text.
    pub(super) fn list_projection_expr(
        &self,
        op: smelt_hir::ListProjectionOp,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let list_ty = self.operand_ty(list)?;
        // Read by borrow: every arm below is `match {list_text}.clone() { .. }`, which
        // appends its own clone, so cloning here made it `x.clone().clone()`.
        let list_text = self.operand_borrow_expr(list)?;
        if matches!(
            self.mir.types.get(list_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) {
            return match op {
                smelt_hir::ListProjectionOp::Keys => {
                    let int_ty = self.type_id(Type::Int)?;
                    if self.mir.types.get(dest_ty) != Some(&Type::List(int_ty)) {
                        return Err(EmitError::new("array keys destination must be int list"));
                    }
                    Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                        "match {list_text}.clone() {{ SmeltUnknown::Array(smelt_values) => (0..smelt_values.len()).map(|smelt_idx| smelt_idx as i64).collect::<Vec<_>>(), _ => Vec::new() }}", list_text = list_text.tight()
                    ))))
                }
                smelt_hir::ListProjectionOp::Values => {
                    let Some(Type::List(value_ty)) = self.mir.types.get(dest_ty) else {
                        return Err(EmitError::new(
                            "dynamic array values destination must be a list",
                        ));
                    };
                    let item_text = self.extract_rendered(&RustExpr::path("smelt_item"), *value_ty, &self.render_scope())?;
                    Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                        "match {list_text}.clone() {{ SmeltUnknown::Array(smelt_values) => smelt_values.into_iter().map(|smelt_item| {item_text}).collect::<Vec<_>>(), _ => Vec::new() }}", list_text = list_text.tight(), item_text = item_text.delimited()
                    ))))
                }
                smelt_hir::ListProjectionOp::Entries => {
                    let int_ty = self.type_id(Type::Int)?;
                    let Some(Type::List(tuple_ty)) = self.mir.types.get(dest_ty) else {
                        return Err(EmitError::new("array entries destination must be a list"));
                    };
                    let Some(Type::Tuple(items)) = self.mir.types.get(*tuple_ty) else {
                        return Err(EmitError::new("array entries item must be a tuple"));
                    };
                    if items.first() != Some(&int_ty) || items.len() != 2 {
                        return Err(EmitError::new(
                            "dynamic array entries destination must contain (int, item) tuples",
                        ));
                    }
                    let Some(item_ty) = items.get(1).copied() else {
                        return Err(EmitError::new(
                            "dynamic array entries destination must contain an item type",
                        ));
                    };
                    let item_text = self.extract_rendered(&RustExpr::path("smelt_item"), item_ty, &self.render_scope())?;
                    Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                        "match {list_text}.clone() {{ SmeltUnknown::Array(smelt_values) => smelt_values.into_iter().enumerate().map(|(smelt_idx, smelt_item)| (smelt_idx as i64, {item_text})).collect::<Vec<_>>(), _ => Vec::new() }}", list_text = list_text.tight(), item_text = item_text.delimited()
                    ))))
                }
            };
        }
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("array projection receiver must be a list"));
        };
        match op {
            smelt_hir::ListProjectionOp::Keys => {
                let int_ty = self.type_id(Type::Int)?;
                if self.mir.types.get(dest_ty) != Some(&Type::List(int_ty)) {
                    return Err(EmitError::new("array keys destination must be int list"));
                }
                Ok(RustExpr::atom(format!(
                    "(0..{list_text}.len()).map(|smelt_idx| smelt_idx as i64).collect::<Vec<_>>()", list_text = list_text.tight()
                )))
            }
            smelt_hir::ListProjectionOp::Values => {
                if dest_ty != list_ty {
                    return Err(EmitError::new(
                        "array values destination must match receiver",
                    ));
                }
                Ok(list_text.cloned_value())
            }
            smelt_hir::ListProjectionOp::Entries => {
                let int_ty = self.type_id(Type::Int)?;
                let Some(Type::List(tuple_ty)) = self.mir.types.get(dest_ty) else {
                    return Err(EmitError::new("array entries destination must be a list"));
                };
                let Some(Type::Tuple(items)) = self.mir.types.get(*tuple_ty) else {
                    return Err(EmitError::new("array entries item must be a tuple"));
                };
                if items.as_slice() != [int_ty, *item_ty] {
                    return Err(EmitError::new(
                        "array entries destination must contain (int, item) tuples",
                    ));
                }
                if type_holds_function(self.mir, *item_ty, NominalStorage::Opaque) {
                    Ok(RustExpr::atom_on(&list_text, format!(
                        "{list_text}.into_iter().enumerate().map(|(smelt_idx, smelt_item)| (smelt_idx as i64, smelt_item)).collect::<Vec<_>>()", list_text = list_text.tight()
                    )))
                } else {
                    let list_read = list_read_expr(&list_text);
                    Ok(RustExpr::atom_on(&list_read, format!(
                        "{list_read}.iter().cloned().enumerate().map(|(smelt_idx, smelt_item)| (smelt_idx as i64, smelt_item)).collect::<Vec<_>>()", list_read = list_read.tight()
                    )))
                }
            }
        }
    }

    /// Converts a list-to-tuple constructor conversion to Rust text.
    pub(super) fn list_to_tuple_expr(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list-to-tuple source must be a list"));
        };
        let Some(Type::Tuple(items)) = self.mir.types.get(dest_ty) else {
            return Err(EmitError::new("list-to-tuple destination must be a tuple"));
        };
        if !items.iter().all(|tuple_item| tuple_item == item_ty) {
            return Err(EmitError::new(
                "list-to-tuple destination items must match the list item type",
            ));
        }
        let items_text = (0..items.len())
            .map(|idx| format!("smelt_tuple_items.borrow()[{idx}].clone()"))
            .collect::<Vec<_>>()
            .join(", ");
        let tuple_text = if items.len() == 1 {
            format!("({items_text},)")
        } else {
            format!("({items_text})")
        };
        Ok(RustExpr::atom(format!(
            "{{ let smelt_tuple_items = {}; if smelt_tuple_items.len() != {} {{ panic!(\"tuple() length mismatch\"); }} {tuple_text} }}",
            self.operand_expr(list)?.delimited(),
            items.len()
        )))
    }

}
