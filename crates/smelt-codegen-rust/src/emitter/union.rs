//! Concrete Rust representation and conversions for HIR union types.
//!
//! A union whose members all retain concrete Rust storage lowers to one
//! canonical tagged enum. Truly dynamic members (`unknown`, unscoped type
//! parameters, or erased host shapes) continue to use `SmeltUnknown`.

use crate::rust::RustType;
use crate::rust::RustExpr;
use super::*;

/// Return the stable generated Rust name for an interned union type.
pub(crate) fn union_name(ty: TypeId) -> String {
    format!("SmeltUnion{}", ty.0)
}

impl FunctionEmitter<'_> {
    /// Render a concrete union storage type at a use site of the current
    /// function, with its generic arguments resolved in this function's
    /// lexical type-parameter scope (see [`Self::union_type_text_in`]).
    pub(super) fn union_rust_type(&self, ty: TypeId) -> Result<RustType, EmitError> {
        self.union_type_text_in(ty, &self.render_scope().substitution()).map(RustType::from)
    }

    /// Render a concrete union storage type at a use site, resolving each of
    /// the generated enum's type parameters through `substitution`.
    ///
    /// The enum `SmeltUnionN<T, ..>` declares every free type parameter its
    /// members mention (in [`Self::union_type_params`] order), and a use site
    /// passes one argument per declared parameter. Each argument is the
    /// parameter rendered like any other `Type::TypeParam` at this position:
    /// spelled when the enclosing item declares it, bound when a call site
    /// pinned it, and `SmeltUnknown` when it is free here. A parameter is free
    /// at a use site when it is bound by something the Rust item cannot
    /// declare: the own type parameter of a callable TYPE
    /// (`newRouter: <T>() => Router<T>`, a polymorphic value no `Rc<dyn Fn>`
    /// can carry) or of an item whose generics are erased (a lifted generic
    /// arrow, a method). Spelling the raw name there referenced a type the
    /// item never declares (E0425 `cannot find type T`).
    pub(super) fn union_type_text_in(
        &self,
        ty: TypeId,
        substitution: &TypeSubstitution<'_>,
    ) -> Result<String, EmitError> {
        self.union_rust_type_in(ty, substitution).map(RustType::into_string)
    }

    /// The generated enum of a concrete union, with its arguments resolved
    /// through `substitution`.
    pub(super) fn union_rust_type_in(
        &self,
        ty: TypeId,
        substitution: &TypeSubstitution<'_>,
    ) -> Result<RustType, EmitError> {
        let params = self.union_type_params(ty)?;
        if params.is_empty() {
            return Ok(RustType::raw(union_name(ty)));
        }
        let args = params
            .iter()
            .map(|param| {
                let param_ty = self
                    .mir
                    .types
                    .lookup(&Type::TypeParam { name: *param })
                    .ok_or_else(|| EmitError::new("union type parameter is not interned"))?;
                self.rust_type(param_ty, false, substitution)
            })
            .collect::<Result<Vec<_>, EmitError>>()?;
        let text = args.iter().map(RustType::as_str).collect::<Vec<_>>().join(", ");
        Ok(RustType::composed(format!("{}<{text}>", union_name(ty)), &args))
    }

    /// Spell a concrete union with the generated enum's OWN declared
    /// parameter names (`SmeltUnionN<T>`), for the enum's definition and impl
    /// blocks, where those names are in scope by construction.
    pub(super) fn union_declared_type(&self, ty: TypeId) -> Result<RustType, EmitError> {
        let params = self.union_type_params(ty)?;
        if params.is_empty() {
            return Ok(RustType::from(union_name(ty)));
        }
        let args = params
            .iter()
            .map(|param| self.union_type_param_name(*param))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        Ok(RustType::raw(format!("{}<{args}>", union_name(ty))))
    }

    /// Collect free type parameters retained by a concrete union's members.
    pub(super) fn union_type_params(&self, ty: TypeId) -> Result<Vec<Symbol>, EmitError> {
        let Some(members) = self.concrete_union_members(ty) else {
            return Ok(Vec::new());
        };
        let mut params = Vec::new();
        for member in members {
            self.collect_union_type_params(*member, &mut params)?;
        }
        Ok(params)
    }

    /// Walk one union member type and retain each free parameter once.
    fn collect_union_type_params(
        &self,
        ty: TypeId,
        params: &mut Vec<Symbol>,
    ) -> Result<(), EmitError> {
        let Some(kind) = self.mir.types.get(ty) else {
            return Err(EmitError::new("union member references an unknown type"));
        };
        match kind {
            Type::TypeParam { name } => {
                if !params.contains(name) {
                    params.push(*name);
                }
            }
            other => {
                for child in other.children() {
                    self.collect_union_type_params(child, params)?;
                }
            }
        }
        Ok(())
    }

    /// Render one union type parameter as a Rust identifier.
    fn union_type_param_name(&self, param: Symbol) -> Result<String, EmitError> {
        Ok(crate::names::type_param(self.symbol_name(param)?).into_string())
    }

    /// Emit a statically typed method call shared by every concrete union arm.
    ///
    /// TypeScript permits a union method call when every member exposes that
    /// method. Concrete unions are represented as generated tagged enums, so
    /// dispatch must project the active payload before invoking the Rust
    /// method; erasing the receiver would lose useful generic return shape.
    pub(super) fn union_method_expr(
        &self,
        receiver: &Operand,
        method: Symbol,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let receiver_ty = self.operand_ty(receiver)?;
        let Some(members) = self.concrete_union_members(receiver_ty) else {
            return Err(EmitError::new(
                "union method call requires a concrete tagged union receiver",
            ));
        };
        let receiver_text = self.operand_expr(receiver)?;
        let method_name = self.method_rust_name(method)?;
        let args_text = args
            .iter()
            .map(|arg| self.operand_expr(arg))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let union = union_name(receiver_ty);
        let mut arms = Vec::with_capacity(members.len());
        for (index, member) in members.iter().copied().enumerate() {
            let (return_ty, can_throw) = self.union_member_method_return_type(member, method)?;
            // An arm's method that can throw returns `Result`. Its error leaves
            // as a panic, which the enclosing `try` catches (the call has an
            // unwind edge there) or the caller's channel carries on.
            let thrown = if can_throw { ".unwrap_or_else(|error| smelt_panic_throw(error))" } else { "" };
            let call = RustExpr::chain(format!("value.{method_name}({args_text}){thrown}"));
            let converted = self.rendered_at_type(&call, return_ty, dest_ty, &self.render_scope())?;
            arms.push(format!("{union}::M{index}(value) => {converted}", converted = converted.delimited()));
        }
        Ok(RustExpr::atom(format!("match {receiver_text} {{ {} }}", arms.join(", "), receiver_text = receiver_text.delimited())))
    }

    /// Resolve and instantiate one union member's declared method return type,
    /// with whether that method can throw.
    fn union_member_method_return_type(
        &self,
        member_ty: TypeId,
        method: Symbol,
    ) -> Result<(TypeId, bool), EmitError> {
        let Some(Type::Class { name, args }) = self.mir.types.get(member_ty) else {
            return Err(EmitError::new("union method member must be a class value"));
        };
        let class = crate::classes::find_class(self.mir, *name)
            .ok_or_else(|| EmitError::new("union method class is not materialized"))?;
        let function = class
            .methods
            .iter()
            .filter_map(|id| self.mir.functions.get(id.0 as usize))
            .find(|function| function.name == method)
            .ok_or_else(|| EmitError::new("union member does not implement method"))?;
        let substitutions = class
            .type_params
            .iter()
            .zip(args.iter().copied())
            .map(|(param, arg)| (param.name, arg))
            .collect::<HashMap<_, _>>();
        Ok((
            crate::generic_bindings::substitute_existing_type(self.mir, function.return_ty, &substitutions),
            function.can_throw,
        ))
    }

    /// Return canonical members when a union has fully concrete Rust storage.
    pub(super) fn concrete_union_members(&self, ty: TypeId) -> Option<&[TypeId]> {
        let Type::Union(items) = self.mir.types.get(ty)? else {
            return None;
        };
        let concrete = self.context.concrete_unions.borrow().get(&ty).copied();
        let concrete = concrete.unwrap_or_else(|| {
            let concrete = items.len() >= 2
                && items.iter().all(|item| self.union_member_is_concrete(*item));
            self.context.concrete_unions.borrow_mut().insert(ty, concrete);
            concrete
        });
        concrete.then_some(items)
    }

    /// Determine whether a union member can be stored without runtime erasure.
    fn union_member_is_concrete(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Unknown | Type::Never | Type::TypeParam { .. }) | None => false,
            // A union nested under a container arm (`number | (number |
            // number[])[]`, the element type of `[1, [2, [3]]]`) is stored as
            // its own generated enum when that union is concrete too. A bare
            // union directly in a union is flattened away by normalization.
            Some(Type::Union(_)) => self.concrete_union_members(ty).is_some(),
            Some(Type::Class { .. }) => !self.is_erased_class_type(ty),
            Some(
                container @ (Type::List(_)
                | Type::Set(_)
                | Type::Future(_)
                | Type::Dict(_, _)
                | Type::JsMap(_, _)
                | Type::Tuple(_)
                | Type::Function(_)
                | Type::GeneratorResult { .. }),
            ) => container.children().all(|child| self.union_member_is_concrete(child)),
            Some(Type::Generator {
                yield_ty,
                return_ty,
                ..
            }) => {
                self.union_member_is_concrete(*yield_ty)
                    && self.union_member_is_concrete(*return_ty)
                // The caller-provided `.next(value)` type is not stored in the
                // generated wrapper, so a dynamic next-input boundary does not
                // make the generator value itself an erased runtime object.
            }
            Some(Type::Bool | Type::Int | Type::FixedInt(_) | Type::BigInt | Type::Float | Type::String | Type::PyStr | Type::Symbol | Type::PropKey) => true,
            // Nullish unions remain `Option`/erased-boundary work. Keeping them
            // out of tagged unions preserves missing-property/default semantics;
            // the concrete-union feature targets distinct value-bearing arms.
            Some(Type::None | Type::Optional(_)) => false,
        }
    }

    /// Erase `value_text` to `SmeltUnknown` when its type is a concrete union.
    ///
    /// A concrete union stores a tagged `SmeltUnion…` enum, but JavaScript value
    /// operations (string coercion, structural `match`/`matches!`, `typeof`,
    /// property-key stringification) are emitted against `SmeltUnknown`. The
    /// generated `into_smelt_unknown()` conversion projects the tagged enum back
    /// to the erased value those operations expect.
    ///
    /// The projection is cloned first because every caller erases a value in
    /// order to INSPECT it, and inspecting must not consume the source.
    /// `into_smelt_unknown` takes `self` by value, so erasing a place read moved
    /// out of it: `const first = table['a']` read twice — once indexed, once for
    /// `.length` — emitted `first.into_smelt_unknown()` at both sites and the
    /// second was a use-after-move (E0382). A `SmeltUnion…` is not `Copy`, and
    /// the source expression is a place read as often as it is a temporary, so
    /// the clone belongs here rather than at each call site. Cloning a
    /// temporary is redundant but harmless; moving a place is a hard error.
    pub(super) fn erase_concrete_union_expr(&self, value_text: &RustExpr, ty: TypeId) -> Result<RustExpr, EmitError> {
        if self.concrete_union_members(ty).is_none() {
            return Ok(value_text.to_owned());
        }
        // A caller that already owns a clone needs no second one; `.clone().clone()`
        // is not what a hand-writing Rust team would produce.
        if value_text.has_clone_tail() {
            return Ok(value_text.postfix(self.carrier_format(self.carrier()?, format_args!(".into_smelt_unknown()"))));
        }
        Ok(value_text.postfix(self.carrier_format(self.carrier()?, format_args!(".clone().into_smelt_unknown()"))))
    }

    /// Wrap a concrete value in the matching variant of a target union.
    pub(super) fn inject_union_value(
        &self,
        value_text: &RustExpr,
        source: TypeId,
        target: TypeId,
        scope: &RenderScope,
    ) -> Result<Option<RustExpr>, EmitError> {
        let Some(members) = self.concrete_union_members(target) else {
            return Ok(None);
        };
        if let Some(index) = members.iter().position(|member| *member == source) {
            return Ok(Some(RustExpr::opaque(format!("{}::M{index}({value_text})", union_name(target), value_text = value_text.delimited()))));
        }
        // A member that is itself a generated union (`number | ResponseOrInit`
        // where `ResponseOrInit = ResponseInit | Response`, kept as its own
        // enum) holds the source as one of ITS exact arms: wrap twice. Only an
        // exact nested arm counts, and only when exactly one member union has
        // it, so the choice stays as unambiguous as the direct match above.
        let nested = members
            .iter()
            .enumerate()
            .filter(|(_, member)| {
                self.concrete_union_members(**member)
                    .is_some_and(|inner| inner.contains(&source))
            })
            .map(|(index, member)| (index, *member))
            .collect::<Vec<_>>();
        if let [(index, member)] = nested.as_slice()
            && let Some(inner) = self.inject_union_value(value_text, source, *member, scope)?
        {
            return Ok(Some(RustExpr::opaque(format!("{}::M{index}({inner})", union_name(target), inner = inner.delimited()))));
        }
        // Structural injection: the source is not an exact member but shares a
        // single member's collection shape (for example an empty
        // `SmeltList<SmeltUnknown>` default flowing into a `SmeltList<String>`
        // union arm). Coerce the value into that member's element types, then
        // wrap it in the variant. Requiring a unique shape-compatible member
        // keeps the choice unambiguous.
        let mut structural: Vec<usize> = members
            .iter()
            .enumerate()
            .filter(|(_, member)| self.union_member_shape_matches_source(source, **member))
            .map(|(index, _)| index)
            .collect();
        // A union of object shapes (`{ a: number } | { b: string }`) leaves
        // several members shape-compatible, which the uniqueness rule above
        // would reject outright. Narrow by *field* compatibility first: keep
        // only the members every one of whose required fields the source can
        // actually populate. An object literal typed `Dict(String, number)`
        // fits `{ a: number }` and not `{ b: string }`, so the choice becomes
        // unambiguous again on the same "one compatible member" rule.
        if structural.len() > 1 {
            let fitted: Vec<usize> = structural
                .iter()
                .copied()
                .filter_map(|index| members.get(index).copied().map(|member| (index, member)))
                .filter(|(_, member)| self.record_member_accepts_source_fields(source, *member))
                .map(|(index, _)| index)
                .collect();
            if !fitted.is_empty() {
                structural = fitted;
            }
        }
        if let [index] = structural.as_slice() {
            let coerced = self.rendered_at_type(value_text, source, members[*index], scope)?;
            return Ok(Some(RustExpr::opaque(format!("{}::M{index}({coerced})", union_name(target), coerced = coerced.delimited()))));
        }
        // A string-keyed record that fits several object arms (`{ done,
        // hasNext: false }` against `LazyEmpty | LazyNext<T>`, whose literal
        // field types HIR stores as `Bool`) cannot pick its arm statically: the
        // deciding values live in the record. When the arms' structural guards
        // tell them apart (required keys, literal-typed fields; see
        // `interface_member_structural_guard`), the record is routed through
        // the enum's runtime recovery, which tests exactly those fields — the
        // decision TypeScript's discriminated-union check makes. Without
        // guards the choice would be a guess, so the site keeps declining.
        if structural.len() > 1
            && matches!(self.mir.types.get(source), Some(Type::Dict(key, _)) if matches!(self.mir.types.get(*key), Some(Type::String | Type::PyStr)))
            && structural
                .iter()
                .filter(|index| self.union_member_structural_guard(members[**index], members, **index).is_none())
                .count()
                <= 1
        {
            let erased = self.erase_rendered(value_text, source)?;
            return Ok(Some({ let receiver = union_name(target); RustExpr::atom_on(&receiver, format!("{receiver}::from_smelt_unknown({erased})", erased = erased.delimited())) }));
        }
        // An erased source (object-field read, erased return, nullish default)
        // carries a `SmeltUnknown`; reconstruct the concrete union by routing the
        // runtime value to its matching variant. `value_text` must be an owned
        // `SmeltUnknown` expression (the reconstruction consumes it).
        if matches!(
            self.mir.types.get(source),
            Some(Type::Unknown | Type::TypeParam { .. })
        ) {
            return Ok(Some({ let receiver = union_name(target); RustExpr::atom_on(&receiver, format!("{receiver}::from_smelt_unknown({value_text})", value_text = value_text.delimited())) }));
        }
        Ok(None)
    }

    /// The single union arm a LIST LITERAL of `len` elements can be built at.
    ///
    /// A list literal fits a `List` arm (any element type — the elements coerce
    /// into it) and a `Tuple` arm of the same arity. `None` when the target is
    /// not a concrete union, when no arm is a collection, or when more than one
    /// arm is: the literal's own element types cannot break that tie, so the
    /// caller keeps its erased path rather than guessing an arm.
    ///
    /// Answering here rather than in `inject_union_value` is deliberate: a
    /// literal has no single source type to inject FROM, so the arm has to be
    /// chosen before the value is rendered, and the render then happens at the
    /// arm's own type.
    pub(super) fn union_collection_arm_for_list_literal(
        &self,
        union_ty: TypeId,
        len: usize,
    ) -> Option<(usize, TypeId)> {
        let members = self.concrete_union_members(union_ty)?;
        let candidates = members
            .iter()
            .enumerate()
            .filter(|(_, member)| match self.mir.types.get(**member) {
                Some(Type::List(_)) => true,
                Some(Type::Tuple(items)) => items.len() == len,
                _ => false,
            })
            .map(|(index, member)| (index, *member))
            .collect::<Vec<_>>();
        match candidates.as_slice() {
            [single] => Some(*single),
            _ => None,
        }
    }

    /// Whether every required field of an object-shaped union `member` can be
    /// populated from `source`.
    ///
    /// Used only to break a tie between several shape-compatible members (see
    /// [`Self::inject_union_value`]). A member with no known fields, or a
    /// source that is not an object shape, answers `false` so the tie-break
    /// simply does not fire and the previous "unique shape match" rule stands.
    fn record_member_accepts_source_fields(&self, source: TypeId, member: TypeId) -> bool {
        let Some(fields) = self.structural_record_fields(member) else {
            return false;
        };
        if fields.is_empty() {
            return false;
        }
        // A string-keyed dictionary offers ONE value type for every key, so a
        // member fits when that value type can render at each required field.
        // A record/class source instead offers its own named fields, which the
        // field-wise adapter already checks.
        match self.mir.types.get(source) {
            Some(Type::Dict(key, value)) if matches!(self.mir.types.get(*key), Some(Type::String | Type::PyStr)) => {
                let value_ty = *value;
                fields.iter().all(|field| {
                    matches!(self.mir.types.get(field.ty), Some(Type::Optional(_)))
                        || self.dict_value_fits_field(value_ty, field.ty)
                })
            }
            Some(Type::Class { .. }) => self.structural_record_adapter_available(source, member),
            _ => false,
        }
    }

    /// Whether `source` shares `member`'s collection shape while differing only
    /// in element types.
    ///
    /// Used to inject a structurally-compatible value (such as an empty list
    /// default whose element type erased to `SmeltUnknown`) into a concrete
    /// union arm by coercing its elements, rather than requiring an exact type
    /// match. Only collection, object, callback and promise shapes are considered so
    /// that unrelated scalar members are never selected.
    fn union_member_shape_matches_source(&self, source: TypeId, member: TypeId) -> bool {
        if source == member {
            return false;
        }
        matches!(
            (self.mir.types.get(source), self.mir.types.get(member)),
            (Some(Type::List(_)), Some(Type::List(_)))
                | (Some(Type::Set(_)), Some(Type::Set(_)))
                | (Some(Type::Dict(_, _)), Some(Type::Dict(_, _)))
                // An object literal (a string-keyed `Dict`/`SmeltRecord`, or a
                // structural record) injected into a union arm that is itself an
                // object-shaped type (a declared `Class`/interface such as
                // `RetryOptions`, or another record). The field-wise coercion is
                // emitted by `rendered_at_type`'s record adapters. Requiring a
                // single shape-compatible member keeps the arm choice
                // unambiguous, so a `number | RetryOptions` union routes the
                // options object to `RetryOptions` rather than the scalar arm.
                | (
                    Some(Type::Dict(_, _) | Type::Class { .. }),
                    Some(Type::Class { .. }),
                )
                | (Some(Type::Class { .. }), Some(Type::Dict(_, _)))
                // A callback injected into a union's function arm (`ExFn | Ex`
                // receiving `() => new Ex(..)`). TypeScript admits a callback
                // with FEWER parameters than the arm declares, so the source is
                // a different function type; the ordinary function-to-function
                // coercion adapts the arity once the arm is chosen, and the
                // unique-shape rule keeps a union of two callbacks ambiguous.
                | (Some(Type::Function(_)), Some(Type::Function(_)))
                // A promise injected into a union's promise arm (an `async`
                // handler returning `Promise<unknown>` into a
                // `Response | Promise<Response>` result). The future-to-future
                // coercion re-types the awaited output once the arm is chosen.
                | (Some(Type::Future(_)), Some(Type::Future(_)))
        )
    }

    /// The single arm of `targets` that instantiates the same named class or
    /// interface as `member` with different type arguments, if exactly one
    /// does.
    fn union_member_same_nominal(&self, member: TypeId, targets: &[TypeId]) -> Option<(usize, TypeId)> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(member) else {
            return None;
        };
        let mut found = targets.iter().enumerate().filter(|(_, target)| {
            **target != member
                && matches!(self.mir.types.get(**target), Some(Type::Class { name: other, .. }) if other == name)
        });
        let (index, target) = found.next()?;
        found.next().is_none().then_some((index, *target))
    }

    /// Project a guarded union value into a concrete member or narrower union.
    pub(super) fn project_union_value(
        &self,
        value_text: &RustExpr,
        source: TypeId,
        target: TypeId,
    ) -> Result<Option<RustExpr>, EmitError> {
        if source == target {
            return Ok(None);
        }
        let Some(source_members) = self.concrete_union_members(source) else {
            return Ok(None);
        };
        let source_name = union_name(source);
        // `await` over `T | Promise<T>`: every arm becomes a future of the same
        // awaited type, the promise arms as themselves and the value arms as an
        // already-resolved handle. Falling through to the single-member
        // projection below would pick the promise arm and `unreachable!` the
        // value arm, which is a wrong value for exactly the half of the union
        // that needs no waiting -- Hono's `await this.#dispatch(..)` on
        // `Response | Promise<Response>` is that shape.
        if let Some(&Type::Future(awaited)) = self.mir.types.get(target)
            && source_members.iter().all(|member| {
                *member == target
                    || *member == awaited
                    || matches!(self.mir.types.get(*member), Some(Type::Future(inner)) if *inner == awaited)
            })
        {
            let arms = source_members
                .iter()
                .enumerate()
                .map(|(index, member)| {
                    if *member == awaited {
                        let awaited_text = self.rendered_type(awaited)?;
                        Ok(format!(
                            "{source_name}::M{index}(value) => SmeltFuture::<{awaited_text}>::resolved(value)"
                        ))
                    } else {
                        Ok(format!("{source_name}::M{index}(value) => value"))
                    }
                })
                .collect::<Result<Vec<_>, EmitError>>()?;
            return Ok(Some(RustExpr::atom(format!(
                "match {value_text} {{ {} }}",
                arms.join(", "), value_text = value_text.delimited()
            ))));
        }
        if let Some(target_members) = self.concrete_union_members(target) {
            let target_name = union_name(target);
            let mut arms = Vec::new();
            for (source_index, member) in source_members.iter().enumerate() {
                if let Some(target_index) =
                    target_members.iter().position(|target_member| target_member == member)
                {
                    arms.push(format!(
                        "{source_name}::M{source_index}(value) => {target_name}::M{target_index}(value)"
                    ));
                    continue;
                }
                // The same generic record at another instantiation
                // (`LazyNext<SmeltUnknown>` from an erased generic call into
                // the caller's `LazyNext<f64>`): the arm is the same source
                // type, so the value converts into it instead of being
                // treated as excluded.
                if let Some((target_index, target_member)) =
                    self.union_member_same_nominal(*member, target_members)
                {
                    let converted =
                        self.rendered_at_type(&RustExpr::path("value"), *member, target_member, &self.render_scope())?;
                    arms.push(format!(
                        "{source_name}::M{source_index}(value) => {target_name}::M{target_index}({converted})", converted = converted.delimited()
                    ));
                    continue;
                }
                // A string-keyed RECORD arm (`cond ? { .. } : SKIP_ITEM`) is
                // an object literal the target can still hold: it injects the
                // way a lone record would, through the target arms' literal
                // discriminants. Any other unmatched arm is one the program
                // narrowed away and stays excluded.
                if matches!(self.mir.types.get(*member), Some(Type::Dict(key, _)) if matches!(self.mir.types.get(*key), Some(Type::String | Type::PyStr)))
                    && let Some(injected) =
                        self.inject_union_value(&RustExpr::path("value"), *member, target, &self.render_scope())?
                {
                    arms.push(format!("{source_name}::M{source_index}(value) => {injected}", injected = injected.delimited()));
                }
            }
            if arms.is_empty() {
                return Ok(None);
            }
            let fallback = if arms.len() == source_members.len() {
                String::new()
            } else {
                ", _ => unreachable!(\"union guard selected an excluded member\")".to_owned()
            };
            return Ok(Some(RustExpr::atom(format!(
                "match {value_text} {{ {}{fallback} }}",
                arms.join(", "), value_text = value_text.delimited()
            ))));
        }
        let Some(index) = source_members.iter().position(|member| *member == target) else {
            return Ok(None);
        };
        Ok(Some(RustExpr::atom(format!(
            "match {value_text} {{ {source_name}::M{index}(value) => value, _ => unreachable!(\"union guard selected an excluded member\") }}", value_text = value_text.delimited()
        ))))
    }

    /// Emit a discriminant check for concrete union members of one JS kind.
    pub(super) fn concrete_union_tag_check(
        &self,
        value_text: &RustExpr,
        union_ty: TypeId,
        kind: smelt_hir::UnknownKind,
    ) -> Option<String> {
        let members = self.concrete_union_members(union_ty)?;
        let union_enum_name = union_name(union_ty);
        let patterns = members
            .iter()
            .enumerate()
            .filter(|(_, member)| self.union_member_matches_kind(**member, kind))
            .map(|(index, _)| format!("{union_enum_name}::M{index}(_)"))
            .collect::<Vec<_>>();
        Some(if patterns.is_empty() {
            "false".to_owned()
        } else {
            format!("matches!({value_text}, {})", patterns.join(" | "), value_text = value_text.delimited())
        })
    }

    /// Emit an `instanceof` discriminant test for concrete class union arms.
    pub(super) fn concrete_union_class_check(
        &self,
        value_text: &RustExpr,
        union_ty: TypeId,
        class: Symbol,
    ) -> Option<String> {
        let patterns = self.concrete_union_class_patterns(union_ty, class)?;
        Some(if patterns.is_empty() {
            "false".to_owned()
        } else {
            format!("matches!({value_text}, {})", patterns.join(" | "), value_text = value_text.delimited())
        })
    }

    /// Emit the same test for an OPTIONAL union.
    ///
    /// `null instanceof File` is `false` in JavaScript, so an absent value is
    /// simply not one of the accepted patterns: the check is the arm patterns
    /// wrapped in `Some(..)`, which answers `false` for `None` by construction.
    ///
    /// Its own method rather than a flag on the check above, because the
    /// patterns are the same and only their context differs — and because this
    /// shape had no answer at all before. `form.get(name) instanceof File` is
    /// `Optional<string | File>`, and the marker probe that claimed it emitted
    /// `matches!(x.clone(), Some(SmeltUnknown::Object(value)) if ..)` against a
    /// value of type `Option<SmeltUnion…>`, which does not compile.
    pub(super) fn optional_union_class_check(
        &self,
        value_text: &RustExpr,
        optional_ty: TypeId,
        class: Symbol,
    ) -> Option<String> {
        let Some(&Type::Optional(inner)) = self.mir.types.get(optional_ty) else {
            return None;
        };
        let patterns = self.concrete_union_class_patterns(inner, class)?;
        Some(if patterns.is_empty() {
            "false".to_owned()
        } else {
            format!(
                "matches!({value_text}, {})",
                patterns
                    .iter()
                    .map(|pattern| format!("Some({pattern})"))
                    .collect::<Vec<_>>()
                    .join(" | "), value_text = value_text.delimited()
            )
        })
    }

    /// The generated-union arm patterns that satisfy `instanceof <class>`.
    ///
    /// Empty when no arm is that class, which is a real `false` rather than a
    /// missing answer — the union cannot hold one. `None` only when the type is
    /// not a concrete generated union at all.
    fn concrete_union_class_patterns(&self, union_ty: TypeId, class: Symbol) -> Option<Vec<String>> {
        let members = self.concrete_union_members(union_ty)?;
        let union_enum_name = union_name(union_ty);
        Some(
            members
                .iter()
                .enumerate()
                .filter(|(_, member)| self.union_member_is_class(**member, class))
                .map(|(index, _)| format!("{union_enum_name}::M{index}(_)"))
                .collect::<Vec<_>>(),
        )
    }

    /// Whether a union arm's type IS the class `instanceof` names.
    ///
    /// A nominal answer is not enough. Several JavaScript classes lower to a
    /// dedicated `Type` variant rather than to `Type::Class`, because Smelt
    /// models their data directly: a `Promise<T>` is `Type::Future`, an array
    /// is `Type::List`, a `Map` is `Type::JsMap`, a `Set` is `Type::Set`. Those
    /// arms are still that class, and `value instanceof Promise` on a
    /// `string | Promise<string>` is the discriminant test for the future arm.
    ///
    /// Asking only "is this arm `Type::Class { name: Promise }`?" answered no
    /// for every one of them, and an empty pattern list is emitted as a
    /// constant `false` — a real answer for a union that genuinely cannot hold
    /// the class, but here it made the promise branch DEAD CODE and the
    /// function returned its string-formatted promise instead of awaiting it.
    /// That is silently wrong output, not a compile error, which is why it
    /// survived so long.
    ///
    /// Stated over the representation, so any class Smelt models as its own
    /// `Type` variant is answered here rather than by naming spellings.
    fn union_member_is_class(&self, member: TypeId, class: Symbol) -> bool {
        let Ok(class_name) = self.symbol_name(class) else {
            return false;
        };
        let rule = smelt_stdlib::instance_of_rule(class_name);
        match self.mir.types.get(member) {
            Some(Type::Class { name, .. }) => self.class_extends_or_equals(*name, class),
            Some(Type::Future(_)) => rule == Some(smelt_stdlib::InstanceOfRule::Promise),
            Some(Type::List(_) | Type::Tuple(_)) => rule == Some(smelt_stdlib::InstanceOfRule::Array),
            Some(Type::JsMap(_, _)) => rule == Some(smelt_stdlib::InstanceOfRule::Map),
            Some(Type::Set(_)) => rule == Some(smelt_stdlib::InstanceOfRule::Set),
            _ => false,
        }
    }

    /// Return the `SmeltUnknown` variant pattern a concrete member reconstructs from.
    ///
    /// Reconstruction (`from_smelt_unknown`) routes an erased value to the union
    /// arm whose runtime JavaScript category matches, so each member maps to the
    /// `SmeltUnknown` discriminant its values carry at runtime.
    fn union_member_unknown_pattern(&self, member: TypeId) -> &'static str {
        use smelt_hir::UnknownKind as Kind;

        match self.mir.types.get(member).and_then(Type::runtime_kind) {
            Some(Kind::Bool) => "SmeltUnknown::Bool(_)",
            Some(Kind::Number) => "SmeltUnknown::Number(_)",
            Some(Kind::BigInt) => "SmeltUnknown::BigInt(_)",
            Some(Kind::String) => "SmeltUnknown::String(_)",
            Some(Kind::Function) => "SmeltUnknown::Function(_)",
            Some(Kind::Array) => "SmeltUnknown::Array(_)",
            Some(Kind::Promise) => "SmeltUnknown::Promise(_)",
            Some(Kind::Object) => "SmeltUnknown::Object(_)",
            Some(Kind::Symbol | Kind::Null | Kind::Undefined) | None => {
                "SmeltUnknown::Null | SmeltUnknown::Undefined"
            }
        }
    }

    /// Build the body of a union's `from_smelt_unknown` reconstruction.
    ///
    /// Erased values (object-field reads, erased returns, nullish-coalescing
    /// defaults) reach a concrete-union destination and must be projected back
    /// into the matching tagged variant. Each present value is routed by its
    /// runtime discriminant to the member sharing that JavaScript category, then
    /// extracted into the member's concrete Rust storage. The final member is the
    /// total fallback so the reconstruction is exhaustive (tsc has already proven
    /// the value inhabits the union).
    /// The runtime tag is only a discriminator when the arms disagree about it.
    /// Two object-shaped arms share `SmeltUnknown::Object(_)`, and in source
    /// order the FIRST of them therefore claimed every object -- and its
    /// extraction then retyped the recovered value's leaves to that arm's field
    /// types, so an `A & B`-shaped value forced into arm `A` came back with
    /// `age: 36` rewritten as the string `"36"`. Silent data corruption, not a
    /// mis-typing.
    ///
    /// So ambiguous arms are ordered by how much evidence their guard actually
    /// checks: an arm with a structural key check (a class, whose declared
    /// fields are known) is tried before one with only the shared tag, and
    /// among the rest an arm whose extraction is lossless -- a record or list of
    /// erased values, which keeps every leaf as it arrived -- is preferred over
    /// one that would retype. That makes "no arm matched" resolve to keeping the
    /// erased value rather than to rewriting it.
    /// Build the `from_smelt_unknown` body with the enum's own type parameters
    /// spellable.
    ///
    /// A member that mentions one of them cannot be rebuilt field by field here:
    /// the recovery is written against concrete field types, and `T` has none
    /// until the enum is instantiated. It delegates to that member's own
    /// `SmeltFromUnknown` instead, which the enum's `impl` bound already
    /// requires of every declared parameter — so `ResponseInit<T>` recovers
    /// through `ResponseInit`'s implementation with `T`'s recovery inside it,
    /// rather than through a field-by-field rebuild that assumed
    /// `Option<SmeltUnknown>` where the struct declares `Option<T>` (E0308).
    /// Members that mention no parameter keep the field-by-field recovery
    /// unchanged, so every non-generic union emits exactly the bytes it did --
    /// a non-generic union declares no parameters, so `substitution.spells`
    /// answers `false` for every member and this reduces to the original body.
    pub(super) fn union_from_smelt_unknown_body_in_scope(
        &self,
        members: &[TypeId],
        member_scope: &HashSet<Symbol>,
    ) -> Result<String, EmitError> {
        let substitution = &TypeSubstitution::callee_emission(member_scope);
        let order = self.union_recovery_order(members);
        let mut body = String::new();
        // A modeled host class (a `ReadableStream` body, a `Blob`, a typed
        // array) erases to an object whose ORIGIN slot holds the live value,
        // and all of them share the `Object` tag, so the tag guards below cannot
        // tell them apart: the first object arm caught every one, and Hono's
        // `new Response(res.body)` rebuilt the body as an empty typed array.
        // The origin's Rust type decides exactly, so those arms are asked first
        // and hand back the original value. Anything without an origin falls
        // through to the guards unchanged.
        for (index, &member) in members.iter().enumerate() {
            let Some(Type::Class { name, .. }) = self.mir.types.get(member) else {
                continue;
            };
            let host_adapter = crate::stdlib::stdlib_class_of_class_symbol(self.mir, *name)
                .is_some_and(smelt_stdlib::StdlibClass::erases_through_adapter);
            let shares_tag = members.iter().enumerate().any(|(other, ty)| {
                other != index
                    && self.union_member_unknown_pattern(*ty)
                        == self.union_member_unknown_pattern(member)
            });
            if !host_adapter || !shares_tag {
                continue;
            }
            let member_text = self.rust_type(member, false, substitution)?.into_string();
            body.push_str(&self.carrier_format(self.carrier()?, format_args!(
                "        if let SmeltUnknown::Object(smelt_host) = &value {{ if let Some(smelt_origin) = smelt_host.smelt_origin::<{member_text}>() {{ return Self::M{index}(smelt_origin); }} }}\n"
            )));
        }
        for (position, &index) in order.iter().enumerate() {
            let Some(&member) = members.get(index) else {
                continue;
            };
            let mut member_params = Vec::new();
            self.collect_union_type_params(member, &mut member_params)?;
            // A callable member has no `SmeltFromUnknown` impl (an
            // `Rc<dyn Fn>` is rebuilt as an adapter closure around the erased
            // function), so it takes the value-extraction path rendered in the
            // enum's own scope, which spells its parameters like the trait
            // path does.
            let is_callable = matches!(self.mir.types.get(member), Some(Type::Function(_)));
            let extracted = if is_callable {
                self.extract_rendered(&RustExpr::path("value"), member, &RenderScope::lexical(member_scope.clone()))?
            } else if member_params
                .iter()
                .any(|param| substitution.spells(*param))
            {
                let member_text = self.rust_type(member, false, substitution)?.into_string();
                RustExpr::loose(self.carrier_format(self.carrier()?, format_args!("<{member_text} as SmeltFromUnknown>::smelt_from_unknown(value)")))
            } else {
                self.extract_rendered(&RustExpr::path("value"), member, &self.render_scope())?
            };
            if Some(position) == order.len().checked_sub(1) {
                body.push_str(&format!("        Self::M{index}({extracted})\n", extracted = extracted.delimited()));
            } else {
                let pattern = self.union_member_unknown_pattern(member);
                let guard = match self.union_member_structural_guard(member, members, index) {
                    Some(structural) => format!("matches!(value, {pattern}) && {structural}"),
                    None => format!("matches!(value, {pattern})"),
                };
                body.push_str(&format!(
                    "        if {guard} {{ return Self::M{index}({extracted}); }}\n", extracted = extracted.delimited()
                ));
            }
        }
        Ok(body)
    }

    /// Order the member indexes by how much evidence their recovery guard has.
    ///
    /// Arms whose runtime tag is unique to them keep source order — the tag
    /// alone decides, so nothing is ambiguous. Arms that share a tag with
    /// another arm are ranked: structurally checkable first, then lossless
    /// (nothing is retyped when this arm is the one that catches the value),
    /// then the remainder. The last entry becomes the unconditional fallback.
    fn union_recovery_order(&self, members: &[TypeId]) -> Vec<usize> {
        let mut order: Vec<usize> = (0..members.len()).collect();
        let tag_of = |index: usize| {
            members
                .get(index)
                .map(|member| self.union_member_unknown_pattern(*member))
        };
        let shares_tag = |index: usize| {
            (0..members.len()).any(|other| other != index && tag_of(other) == tag_of(index))
        };
        order.sort_by_key(|&index| {
            let Some(&member) = members.get(index) else {
                return (u8::MAX, index);
            };
            if !shares_tag(index) {
                return (0_u8, index);
            }
            let rank = if self
                .union_member_structural_guard(member, members, index)
                .is_some()
            {
                1
            } else if self.union_member_extraction_is_lossless(member) {
                2
            } else {
                3
            };
            (rank, index)
        });
        order
    }

    /// Renders one test of a union arm's structural guard.
    ///
    /// A guard's text both decides (does this arm have a guard at all?) and is
    /// emitted inside the union's `from_smelt_unknown` recovery, whose own
    /// emitter holds the carrier. So it records the carrier use for the generic
    /// trial, as [`Self::carrier_format`] does, without asking for the
    /// `CarrierUse` proof a decision-only render has no reason to hold.
    fn guard_format(&self, template: std::fmt::Arguments<'_>) -> String {
        self.context.note_carrier_use();
        template.to_string()
    }

    /// Build a structural guard that separates `member` from the arms it shares
    /// a runtime tag with, if its declared shape allows one.
    ///
    /// Two shapes carry enough static evidence to discriminate:
    ///
    /// * a TUPLE states its arity, so a length test decides — used when no
    ///   same-tag sibling has the same arity;
    /// * a CLASS states its field names, so key presence decides — the guard
    ///   asserts every field this arm declares that at least one same-tag
    ///   sibling does not.
    ///
    /// Everything else returns `None`: a record is `Dict(String, V)` and states
    /// no keys, a list states no length, so there is nothing to test. Those arms
    /// still recover on the tag alone, which is all their type supports.
    fn union_member_structural_guard(
        &self,
        member: TypeId,
        members: &[TypeId],
        index: usize,
    ) -> Option<String> {
        let tag = self.union_member_unknown_pattern(member);
        let siblings = members
            .iter()
            .enumerate()
            .filter(|(other, ty)| {
                *other != index && self.union_member_unknown_pattern(**ty) == tag
            })
            .map(|(_, ty)| *ty)
            .collect::<Vec<_>>();
        if siblings.is_empty() {
            return None;
        }
        // A TUPLE's arity is part of its type, so when tag-sharing siblings
        // disagree on length the length IS the discriminant — the same kind of
        // static evidence the class branch below gets from field names.
        //
        // Without this, `Pair | Triple` (both `[string, number, ...]`, so both
        // tagged `SmeltUnknown::Array`) recovered on the tag alone: the first
        // arm won every time, and `table['b'] = ['y', 2, true]` came back as the
        // 2-tuple with its third element silently dropped —
        // `['y',2,true].length` answered 2 where Node answers 3, with nothing
        // reported. Only emitted when no tag-sharing sibling has the same
        // arity, so the check is decisive rather than merely narrowing.
        if let Some(Type::Tuple(items)) = self.mir.types.get(member) {
            let arity = items.len();
            let ambiguous = siblings.iter().any(|sibling| {
                matches!(self.mir.types.get(*sibling), Some(Type::Tuple(other)) if other.len() == arity)
            });
            return if ambiguous {
                None
            } else {
                Some(self.guard_format(format_args!(
                    "(matches!(&value, SmeltUnknown::Array(smelt_arms) if smelt_arms.len() == {arity}))"
                )))
            };
        }
        let Some(Type::Class { name, .. }) = self.mir.types.get(member) else {
            return None;
        };
        if let Some(interface) = self.mir.interface_named(*name) {
            return self.interface_member_structural_guard(interface, &siblings);
        }
        let fields = crate::classes::find_class(self.mir, *name)
            .map(|class| {
                class
                    .fields
                    .iter()
                    .filter_map(|field| self.symbol_name(field.name).ok())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })?;
        let distinguishing = fields
            .into_iter()
            .filter(|field| {
                siblings
                    .iter()
                    .any(|sibling| !self.union_member_has_field(*sibling, field))
            })
            .map(|field| {
                self.guard_format(format_args!(
                    "matches!(&value, SmeltUnknown::Object(smelt_map) if smelt_map.contains_key({field:?}))"
                ))
            })
            .collect::<Vec<_>>();
        if distinguishing.is_empty() {
            None
        } else {
            Some(format!("({})", distinguishing.join(" && ")))
        }
    }

    /// The structural guard of an object-shaped (interface) union arm.
    ///
    /// An interface states two kinds of evidence a runtime object can be
    /// tested against:
    ///
    /// * a REQUIRED field that some same-tag sibling does not declare (key
    ///   presence, as for classes);
    /// * a required field whose type is a LITERAL (`hasNext: false`,
    ///   `kind: "circle"`), recorded by the frontend as
    ///   [`smelt_hir::FieldDecodeFacts::literals`] because HIR stores the field
    ///   as its primitive. It discriminates against a sibling that declares
    ///   the same field with disjoint literals.
    ///
    /// The guard is the conjunction of every such test. Each one is a
    /// necessary condition for a value of this arm, so a value that fails it
    /// is not this arm; this is what TypeScript's discriminated-union
    /// narrowing decides from the same declarations.
    fn interface_member_structural_guard(
        &self,
        interface: &smelt_mir::MirInterface,
        siblings: &[TypeId],
    ) -> Option<String> {
        let fields = crate::classes::effective_interface_fields(self.mir, interface);
        let mut tests = Vec::new();
        for field in &fields {
            let Ok(key) = self.symbol_source_name(field.name) else {
                continue;
            };
            if matches!(self.mir.types.get(field.ty), Some(Type::Optional(_))) {
                continue;
            }
            let literals = interface
                .decode_facts
                .iter()
                .find(|facts| facts.name == field.name && !facts.may_be_absent)
                .map(|facts| facts.literals.as_slice())
                .unwrap_or_default();
            let absent_somewhere = siblings
                .iter()
                .any(|sibling| !self.union_member_declares_field(*sibling, field.name));
            let literal_disjoint = !literals.is_empty()
                && siblings.iter().any(|sibling| {
                    self.union_member_field_literals(*sibling, field.name)
                        .is_some_and(|other| other.iter().all(|value| !literals.contains(value)))
                });
            if absent_somewhere {
                tests.push(self.guard_format(format_args!(
                    "matches!(&value, SmeltUnknown::Object(smelt_map) if smelt_map.contains_key({key:?}))"
                )));
            }
            if literal_disjoint
                && let Some(test) = Self::literal_field_test(key, literals)
            {
                tests.push(test);
            }
        }
        (!tests.is_empty()).then(|| format!("({})", tests.join(" && ")))
    }

    /// Whether the object-shaped union arm `ty` declares field `name`.
    fn union_member_declares_field(&self, ty: TypeId, name: Symbol) -> bool {
        let Some(Type::Class { name: owner, .. }) = self.mir.types.get(ty) else {
            return false;
        };
        if let Some(interface) = self.mir.interface_named(*owner) {
            return crate::classes::effective_interface_fields(self.mir, interface)
                .iter()
                .any(|field| field.name == name);
        }
        self.symbol_name(name)
            .is_ok_and(|field| self.union_member_has_field(ty, field))
    }

    /// The literal values an interface arm's field `name` admits when present
    /// (an optional `hasMany?: false` admits `false`, or is absent; neither
    /// passes a `hasMany === true` test); `None` when the arm is not an
    /// interface or the field is not literal-typed.
    fn union_member_field_literals(&self, ty: TypeId, name: Symbol) -> Option<&[smelt_hir::Literal]> {
        let Some(Type::Class { name: owner, .. }) = self.mir.types.get(ty) else {
            return None;
        };
        let interface = self.mir.interface_named(*owner)?;
        interface
            .decode_facts
            .iter()
            .find(|facts| facts.name == name && !facts.literals.is_empty())
            .map(|facts| facts.literals.as_slice())
    }

    /// `value.key` is one of `literals`, as a runtime test on an erased object;
    /// `None` when a literal has no erased spelling to compare against.
    fn literal_field_test(key: &str, literals: &[smelt_hir::Literal]) -> Option<String> {
        let arms = literals
            .iter()
            .map(|literal| match literal {
                smelt_hir::Literal::Bool(value) => {
                    Some(format!("matches!(smelt_map.get({key:?}), Some(SmeltUnknown::Bool({value})))"))
                }
                smelt_hir::Literal::String(text) => Some(format!(
                    "matches!(smelt_map.get({key:?}), Some(SmeltUnknown::String(smelt_text)) if smelt_text.to_string() == {text:?})"
                )),
                smelt_hir::Literal::Float(number) if number.is_finite() => Some(format!(
                    "matches!(smelt_map.get({key:?}), Some(SmeltUnknown::Number(smelt_number)) if smelt_number == {number:?})"
                )),
                smelt_hir::Literal::Int(number) => Some(format!(
                    "matches!(smelt_map.get({key:?}), Some(SmeltUnknown::Number(smelt_number)) if smelt_number == {number}.0)"
                )),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        Some(format!(
            "matches!(&value, SmeltUnknown::Object(smelt_map) if {})",
            arms.join(" || ")
        ))
    }

    /// Whether recovering into this arm leaves every leaf of the erased value
    /// exactly as it arrived.
    ///
    /// A container of `Unknown` stores the runtime values verbatim; a container
    /// of a concrete element type re-extracts every leaf into that type, which
    /// is where `36` became `"36"`.
    fn union_member_extraction_is_lossless(&self, member: TypeId) -> bool {
        match self.mir.types.get(member) {
            Some(Type::Unknown) => true,
            Some(Type::Dict(_, value) | Type::List(value) | Type::Set(value)) => {
                matches!(self.mir.types.get(*value), Some(Type::Unknown))
            }
            _ => false,
        }
    }

    /// Emit a structural `field in value` check over concrete union arms.
    ///
    /// A `"field" in value` guard on a concrete union does not need to erase the
    /// value and inspect a runtime object map: the set of arms that expose
    /// `field` is known statically, so the check compiles to a tagged-enum
    /// discriminant test. Returns `None` when the receiver is not a concrete
    /// union (the caller then falls back to the erased object lookup), keeping
    /// dynamic boundaries explicit. When no arm carries the field the check is a
    /// constant `false`; when every arm carries it, a constant `true`.
    pub(super) fn concrete_union_field_check(
        &self,
        value_text: &RustExpr,
        union_ty: TypeId,
        field: &str,
    ) -> Option<String> {
        let members = self.concrete_union_members(union_ty)?;
        let union_enum_name = union_name(union_ty);
        let patterns = members
            .iter()
            .enumerate()
            .filter(|(_, member)| self.type_has_property_statically(**member, field))
            .map(|(index, _)| format!("{union_enum_name}::M{index}(_)"))
            .collect::<Vec<_>>();
        Some(if patterns.is_empty() {
            "false".to_owned()
        } else if patterns.len() == members.len() {
            "true".to_owned()
        } else {
            format!("matches!({value_text}, {})", patterns.join(" | "), value_text = value_text.delimited())
        })
    }

    /// Emit a dotted property write dispatched through a generated union's arms.
    ///
    /// A value whose static type is a concrete union is a tagged `SmeltUnion…`
    /// enum, so the write is dispatched on the tag and each arm performs its own
    /// member's typed struct-field assignment — the value is rendered at that
    /// arm's declared field type, so nothing is erased on the way in.
    ///
    /// Returns `None` when at least one arm does not declare the property as a
    /// named field. TypeScript only accepts such a write after narrowing the
    /// receiver to the one arm that has it, and the tag alone cannot reproduce a
    /// narrowing the MIR did not record, so the caller falls back to the erased
    /// boundary adapter (documented at the call site in `control_flow`).
    pub(super) fn concrete_union_field_assign_expr(
        &self,
        base: LocalId,
        field: Symbol,
        value: &Rvalue,
    ) -> Result<Option<RustExpr>, EmitError> {
        let base_ty = self.local_decl(base)?.ty;
        let Some(members) = self.concrete_union_members(base_ty) else {
            return Ok(None);
        };
        let resolved: Option<Vec<TypeId>> = members
            .iter()
            .map(|member| self.class_named_field_ty(*member, field))
            .collect();
        let Some(field_types) = resolved else {
            return Ok(None);
        };
        let union_enum_name = union_name(base_ty);
        let field_name = self.context.names().field_text(self.mir, field);
        let mut arms = Vec::with_capacity(field_types.len());
        for (index, (field_ty, member)) in field_types.into_iter().zip(members).enumerate() {
            let rendered_value = self.rvalue_expr_for_dest(value, field_ty)?;
            // A reference class keeps its fields in its shared cell.
            let slot = if self.is_reference_class_type(*member) {
                format!("smelt_union_arm.0.borrow_mut().{field_name}")
            } else {
                format!("smelt_union_arm.{field_name}")
            };
            arms.push(format!(
                "{union_enum_name}::M{index}(smelt_union_arm) => {{ {slot} = {rendered_value}; }}", rendered_value = rendered_value.delimited()
            ));
        }
        Ok(Some(RustExpr::atom(format!(
            "match &mut {} {{ {} }}",
            self.local_mut_value(base)?.tight(),
            arms.join(", ")
        ))))
    }

    /// Emit a dotted property read dispatched through a generated union's arms.
    ///
    /// The mirror of [`Self::concrete_union_field_assign_expr`] on the read
    /// side: each arm projects its own member's struct field, so the value keeps
    /// the type the source gave it instead of being erased and looked up by
    /// name at run time.
    ///
    /// Returns `None` unless every arm declares the property at ONE type. Arms
    /// that disagree on the field's type would need the field's own union, which
    /// this rule does not mint; the caller keeps its existing path for those.
    pub(super) fn concrete_union_field_read_expr(
        &self,
        base: LocalId,
        field: Symbol,
    ) -> Result<Option<RustExpr>, EmitError> {
        let base_ty = self.local_decl(base)?.ty;
        if self.concrete_union_field_ty(base_ty, field).is_none() {
            return Ok(None);
        }
        let Some(members) = self.concrete_union_members(base_ty) else {
            return Ok(None);
        };
        let union_enum_name = union_name(base_ty);
        let field_name = self.context.names().field_text(self.mir, field);
        let arms = members
            .iter()
            .enumerate()
            .map(|(index, member)| {
                // A reference class keeps its fields in its shared cell.
                let cell = if self.is_reference_class_type(*member) { ".0.borrow()" } else { "" };
                format!(
                    "{union_enum_name}::M{index}(smelt_union_arm) => smelt_union_arm{cell}.{field_name}.clone()"
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        Ok(Some(RustExpr::atom(format!(
            "match &{} {{ {arms} }}",
            self.local_value(base)?.tight()
        ))))
    }

    /// `value.length` on a concrete union whose every arm has a length: each
    /// list arm answers its own `len()`, a tuple arm its fixed arity, a string
    /// arm its character count (the same count a `String` `.length` reads).
    /// `None` keeps the erased read for any other arm.
    pub(super) fn concrete_union_length_read_expr(
        &self,
        base: LocalId,
    ) -> Result<Option<RustExpr>, EmitError> {
        let base_ty = self.local_decl(base)?.ty;
        let Some(members) = self.concrete_union_members(base_ty) else {
            return Ok(None);
        };
        let mut arms = Vec::with_capacity(members.len());
        for (position, member) in members.iter().enumerate() {
            let length = match self.mir.types.get(*member) {
                Some(Type::List(_)) => "(smelt_union_arm.len() as f64)".to_owned(),
                Some(Type::Tuple(items)) => format!("{}.0", items.len()),
                Some(Type::String | Type::PyStr) => format!("({} as f64)", self.string_len_text("smelt_union_arm")),
                _ => return Ok(None),
            };
            arms.push(format!("{}::M{position}(smelt_union_arm) => {length}", union_name(base_ty)));
        }
        Ok(Some(RustExpr::atom(format!(
            "match &{} {{ {} }}",
            self.local_value(base)?.tight(),
            arms.join(", ")
        ))))
    }

    /// The static type and per-arm reads of `value[index]` on a concrete union.
    ///
    /// A union whose arms are tuples and lists (Hono's router answers
    /// `[[T, ParamIndexMap][], ParamStash] | [[T, Params][]]`) is typed all the
    /// way through HIR. A hand port matches on the enum and reads each arm at its
    /// own type, and so does this:
    ///
    /// * a tuple arm with a constant index reads that element, and reads
    ///   `undefined` past its end (`result[1]` on the one-element arm);
    /// * a list arm reads the element with the typed list read's semantics
    ///   (an out-of-range index answers the element type's missing value);
    /// * a dictionary arm reads the key as a typed dictionary read does.
    ///
    /// The read's type is the arms' common type, else the interned union of
    /// their types (the HIR types `result[0]` as that union, so it exists),
    /// made `Optional` when an arm reads `undefined`. `None` keeps the erased
    /// path: another arm shape, a non-constant tuple index, a negative tuple
    /// index, or a joined type that is not interned or not itself concrete.
    pub(super) fn concrete_union_index_reads(
        &self,
        union_ty: TypeId,
        index: &Operand,
        negative: NegativeIndex,
    ) -> Result<Option<(TypeId, Vec<UnionArmIndexRead>)>, EmitError> {
        let Some(members) = self.concrete_union_members(union_ty) else {
            return Ok(None);
        };
        let constant = match index {
            Operand::Const(Constant::Int(value)) => usize::try_from(*value).ok(),
            Operand::Const(Constant::Float(value))
                if value.fract() == 0.0_f64 && *value >= 0.0_f64 && *value < 4_294_967_296.0_f64 =>
            {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "the guard proves a whole number in 0..2^32, exact in usize"
                )]
                let index = *value as usize;
                Some(index)
            }
            _ => None,
        };
        let mut reads = Vec::with_capacity(members.len());
        for member in members {
            let read = match self.mir.types.get(*member) {
                Some(Type::Tuple(items)) => {
                    let Some(position) = constant else {
                        return Ok(None);
                    };
                    match items.get(position) {
                        Some(item) => UnionArmIndexRead::Element(
                            *item,
                            format!("smelt_union_arm.{position}.clone()"),
                        ),
                        None => UnionArmIndexRead::Absent,
                    }
                }
                Some(Type::List(item)) => {
                    let index_text = self.normalized_read_index_expr(
                        &RustExpr::chain("smelt_union_arm.len()"),
                        index,
                        negative,
                    )?;
                    let missing = self.element_missing_value(*item)?;
                    UnionArmIndexRead::Element(
                        *item,
                        format!(
                            "{}.get({index_text}).cloned().unwrap_or_else(|| {missing})",
                            list_read_expr(&RustExpr::opaque("smelt_union_arm")).tight(), index_text = index_text.delimited(), missing = missing.delimited()
                        ),
                    )
                }
                Some(Type::Dict(key, value)) => UnionArmIndexRead::Element(
                    *value,
                    self.dict_index_read_expr(&RustExpr::path("smelt_union_arm"), *key, *value, index)?.into_string(),
                ),
                _ => return Ok(None),
            };
            reads.push(read);
        }
        let mut element_tys: Vec<TypeId> = Vec::new();
        for read in &reads {
            if let UnionArmIndexRead::Element(ty, _) = read
                && !element_tys.contains(ty)
            {
                element_tys.push(*ty);
            }
        }
        let joined = match element_tys.as_slice() {
            [] => return Ok(None),
            [single] => *single,
            many => {
                let Some(joined) = self.find_type_id(&Type::Union(many.to_vec())) else {
                    return Ok(None);
                };
                if self.concrete_union_members(joined).is_none() {
                    return Ok(None);
                }
                joined
            }
        };
        let read_ty = if reads.iter().any(|read| matches!(read, UnionArmIndexRead::Absent)) {
            if matches!(self.mir.types.get(joined), Some(Type::Optional(_))) {
                joined
            } else {
                let Some(optional) = self.find_type_id(&Type::Optional(joined)) else {
                    return Ok(None);
                };
                optional
            }
        } else {
            joined
        };
        Ok(Some((read_ty, reads)))
    }

    /// Render `value[index]` on a concrete union through its arms (see
    /// [`Self::concrete_union_index_reads`]), or `None` to keep the erased read.
    pub(super) fn concrete_union_index_read_expr(
        &self,
        base: LocalId,
        index: &Operand,
        negative: NegativeIndex,
    ) -> Result<Option<RustExpr>, EmitError> {
        let base_ty = self.local_decl(base)?.ty;
        let Some((read_ty, reads)) = self.concrete_union_index_reads(base_ty, index, negative)? else {
            return Ok(None);
        };
        let (optional, joined) = match self.mir.types.get(read_ty) {
            Some(Type::Optional(inner))
                if reads.iter().any(|read| matches!(read, UnionArmIndexRead::Absent)) =>
            {
                (true, *inner)
            }
            _ => (false, read_ty),
        };
        let joined_members = self.concrete_union_members(joined).map(<[TypeId]>::to_vec);
        let base_enum = union_name(base_ty);
        let mut arms = Vec::with_capacity(reads.len());
        for (position, read) in reads.iter().enumerate() {
            let value = match read {
                UnionArmIndexRead::Absent => "None".to_owned(),
                UnionArmIndexRead::Element(ty, text) => {
                    let injected = match &joined_members {
                        Some(members) if *ty != joined => {
                            let Some(arm) = members.iter().position(|member| member == ty) else {
                                return Ok(None);
                            };
                            format!("{}::M{arm}({text})", union_name(joined))
                        }
                        _ => text.clone(),
                    };
                    if optional { format!("Some({injected})") } else { injected }
                }
            };
            arms.push(format!("{base_enum}::M{position}(smelt_union_arm) => {value}"));
        }
        Ok(Some(RustExpr::atom(format!(
            "match &{} {{ {} }}",
            self.local_value(base)?.tight(),
            arms.join(", ")
        ))))
    }

    /// `value?.[index]` (or an element read flowing into an optional slot) on
    /// a concrete union, answering `Option<inner>`: each arm reads its element
    /// as [`Self::concrete_union_index_reads`] does, an arm without the element
    /// answers `None`, and an arm whose element is already `Option<inner>`
    /// passes it through. `None` keeps the erased read.
    pub(super) fn concrete_union_optional_index_expr(
        &self,
        base_text: &RustExpr,
        base_ty: TypeId,
        index: &Operand,
        inner: TypeId,
    ) -> Result<Option<RustExpr>, EmitError> {
        let Some((_, reads)) =
            self.concrete_union_index_reads(base_ty, index, NegativeIndex::OutOfRange)?
        else {
            return Ok(None);
        };
        let inner_members = self.concrete_union_members(inner).map(<[TypeId]>::to_vec);
        let mut arms = Vec::with_capacity(reads.len());
        for (position, read) in reads.iter().enumerate() {
            let value = match read {
                UnionArmIndexRead::Absent => "None".to_owned(),
                UnionArmIndexRead::Element(ty, text) if *ty == inner => format!("Some({text})"),
                UnionArmIndexRead::Element(ty, text)
                    if self.mir.types.get(*ty) == Some(&Type::Optional(inner)) =>
                {
                    text.clone()
                }
                UnionArmIndexRead::Element(ty, text) => {
                    let Some(arm) = inner_members
                        .as_ref()
                        .and_then(|members| members.iter().position(|member| member == ty))
                    else {
                        return Ok(None);
                    };
                    format!("Some({}::M{arm}({text}))", union_name(inner))
                }
            };
            arms.push(format!("{}::M{position}(smelt_union_arm) => {value}", union_name(base_ty)));
        }
        Ok(Some(RustExpr::atom(format!("match &{base_text} {{ {} }}", arms.join(", "), base_text = base_text.tight()))))
    }

    /// The static type and per-arm reads of `receiver.field` on a concrete
    /// union whose arms do NOT all declare the field at one type (the case
    /// [`Self::concrete_union_field_read_expr`] declines).
    ///
    /// Each arm answers on its own: a record arm reads its declared field, a
    /// modeled host class arm reads its data property (`Response.status`), and
    /// a `number`/`boolean` arm answers `undefined`, which is what JavaScript
    /// reads for a property neither primitive has (`(5).status`). The read's
    /// type is the arms' common base type, made `Optional` when any arm can
    /// answer `undefined`: `(number | { status?: number } | Response)?.status`
    /// reads `Option<f64>`, typed per arm, as a hand port would write the
    /// `match`.
    ///
    /// `None` (keep the erased path) when an arm is any other shape without the
    /// field, when the arms disagree on the base type, or when the joined
    /// type is not interned.
    pub(super) fn concrete_union_member_reads(
        &self,
        union_ty: TypeId,
        field: Symbol,
    ) -> Result<Option<(TypeId, Vec<UnionArmRead>)>, EmitError> {
        let Some(members) = self.concrete_union_members(union_ty) else {
            return Ok(None);
        };
        let mut reads = Vec::with_capacity(members.len());
        let mut base: Option<TypeId> = None;
        let mut optional = false;
        for member in members {
            let read = if let Some(field_ty) = self.class_named_field_ty(*member, field) {
                UnionArmRead::Field(field_ty)
            } else if let Some(property) = self.host_data_property(*member, field)? {
                let primitive = match property.ty {
                    smelt_stdlib::HostPrimitive::Number => Type::Float,
                    smelt_stdlib::HostPrimitive::Bool => Type::Bool,
                    smelt_stdlib::HostPrimitive::String => Type::String,
                    _ => return Ok(None),
                };
                let Some(ty) = self.find_type_id(&primitive) else {
                    return Ok(None);
                };
                UnionArmRead::Host(ty, property)
            } else if matches!(self.mir.types.get(*member), Some(Type::Float | Type::Bool)) {
                UnionArmRead::Absent
            } else {
                return Ok(None);
            };
            if let UnionArmRead::Field(ty) | UnionArmRead::Host(ty, _) = read {
                let arm_base = match self.mir.types.get(ty) {
                    Some(Type::Optional(inner)) => {
                        optional = true;
                        *inner
                    }
                    _ => ty,
                };
                match base {
                    Some(previous) if previous != arm_base => return Ok(None),
                    _ => base = Some(arm_base),
                }
            } else {
                optional = true;
            }
            reads.push(read);
        }
        let Some(base) = base else {
            return Ok(None);
        };
        let read_ty = if optional {
            match self.find_type_id(&Type::Optional(base)) {
                Some(ty) => ty,
                None => return Ok(None),
            }
        } else {
            base
        };
        Ok(Some((read_ty, reads)))
    }

    /// Render `receiver.field` on a concrete union through
    /// [`Self::concrete_union_member_reads`]: one `match` arm per member, each
    /// converted to the joined read type.
    pub(super) fn concrete_union_member_read_expr(
        &self,
        receiver_text: &RustExpr,
        union_ty: TypeId,
        field: Symbol,
    ) -> Result<Option<RustExpr>, EmitError> {
        let Some((read_ty, reads)) = self.concrete_union_member_reads(union_ty, field)? else {
            return Ok(None);
        };
        let read_is_optional = matches!(self.mir.types.get(read_ty), Some(Type::Optional(_)));
        let union_enum_name = union_name(union_ty);
        let field_name = self.context.names().field_text(self.mir, field);
        let arms = reads
            .iter()
            .enumerate()
            .map(|(index, read)| {
                let (value, ty) = match read {
                    UnionArmRead::Absent => {
                        return format!("{union_enum_name}::M{index}(_) => None");
                    }
                    UnionArmRead::Field(ty) => {
                        (format!("smelt_union_arm.{field_name}.clone()"), *ty)
                    }
                    UnionArmRead::Host(ty, property) => {
                        (field_access::host_data_property_read(&RustExpr::path("smelt_union_arm"), property).into_string(), *ty)
                    }
                };
                let value = if read_is_optional
                    && !matches!(self.mir.types.get(ty), Some(Type::Optional(_)))
                {
                    format!("Some({value})")
                } else {
                    value
                };
                format!("{union_enum_name}::M{index}(smelt_union_arm) => {value}")
            })
            .collect::<Vec<_>>()
            .join(", ");
        Ok(Some(RustExpr::atom(format!("match &{receiver_text} {{ {arms} }}", receiver_text = receiver_text.tight()))))
    }

    /// Resolve the one type every arm of a concrete union gives a named field.
    ///
    /// `None` when the receiver is not a concrete union, when an arm does not
    /// declare the field, or when the arms disagree about its type — the three
    /// cases in which the tag alone cannot decide a single static read type.
    pub(super) fn concrete_union_field_ty(&self, union_ty: TypeId, field: Symbol) -> Option<TypeId> {
        let members = self.concrete_union_members(union_ty)?;
        let mut resolved: Option<TypeId> = None;
        for member in members {
            let field_ty = self.class_named_field_ty(*member, field)?;
            match resolved {
                Some(previous) if previous != field_ty => return None,
                _ => resolved = Some(field_ty),
            }
        }
        resolved
    }

    /// Resolve the declared type of one named struct field on a record type.
    ///
    /// A class and an interface (or a synthesized object-literal shape) are the
    /// same kind of record here — both emit a Rust struct with the same field
    /// spelling — so both field tables answer, exactly as
    /// [`Self::class_has_named_field`] consults both.
    ///
    /// Returns `None` when the type is not a materialized record or the field is
    /// not one of its declared members, which is how the union write dispatch
    /// above decides that an arm cannot take a typed write.
    fn class_named_field_ty(&self, ty: TypeId, field: Symbol) -> Option<TypeId> {
        if !self.class_has_named_field(ty, field) {
            return None;
        }
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return None;
        };
        let fields = crate::classes::find_class(self.mir, *name)
            .map(|class| crate::classes::effective_class_fields(self.mir, class))
            .or_else(|| {
                self.mir
                    .interfaces
                    .iter()
                    .find(|interface| interface.name == *name)
                    .map(|interface| {
                        crate::classes::effective_interface_fields(self.mir, interface)
                    })
            })?;
        fields
            .into_iter()
            .find(|candidate| candidate.name == field)
            .map(|candidate| candidate.ty)
    }

    /// Return whether a union member's erased runtime OBJECT carries `field` as
    /// a data key.
    ///
    /// The erased-arm recognizer tests `SmeltUnknown::Object` maps, which hold
    /// only data fields, so this is deliberately narrower than
    /// [`Self::type_has_property_statically`] (no methods, accessors or
    /// prototype members).
    fn union_member_has_field(&self, ty: TypeId, field: &str) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::String | Type::PyStr | Type::List(_) | Type::Tuple(_)) => field == "length",
            Some(Type::Dict(_, _)) => true,
            Some(Type::Class { name, .. }) => self.mir.classes.iter().any(|class| {
                class.name == *name
                    && class
                        .fields
                        .iter()
                        .any(|candidate| self.symbol_name(candidate.name) == Ok(field))
            }),
            _ => false,
        }
    }

    /// Return whether `'field' in value` holds for a concrete union member type
    /// (or a single concrete class instance).
    ///
    /// The emitted discriminant check keeps exactly the arms whose type answers
    /// `true`. JavaScript's `in` walks the prototype chain, so a class arm
    /// answers for its whole instance surface, not only its data fields:
    ///
    /// * a modeled HOST class (`Request`, `Response`, `Headers`, ...) answers
    ///   from the shared stdlib registry of its spec surface
    ///   (`stdlib_class_instance_has_property`) — it has no MIR class entry,
    ///   so before this every host arm answered `false` and Hono's
    ///   `'headers' in request` over `HonoRequest | Request` was constant
    ///   `false`;
    /// * a user class answers for its fields, methods and accessors,
    ///   including inherited ones ([`Self::mir_class_has_member`]), and an
    ///   interface-shaped arm for its declared fields;
    /// * every object arm also has the `Object.prototype` members.
    pub(super) fn type_has_property_statically(&self, ty: TypeId, field: &str) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::String | Type::PyStr | Type::List(_) | Type::Tuple(_)) => field == "length",
            Some(Type::Dict(_, _)) => true,
            Some(Type::Class { name, .. }) => {
                if let Ok(Some(host)) = self.stdlib_class_of_symbol(*name)
                    && let Some(answer) =
                        smelt_stdlib::stdlib_class_instance_has_property(host, field)
                {
                    return answer;
                }
                self.mir_class_has_member(*name, field, 0)
                    || self.mir_interface_has_field(*name, field)
                    || smelt_stdlib::is_object_prototype_member(field)
            }
            _ => false,
        }
    }

    /// Return whether a named MIR class declares `member` as an instance field,
    /// method or accessor, itself or through its base-class chain.
    ///
    /// `depth` bounds the base walk so a malformed (cyclic) chain terminates.
    fn mir_class_has_member(&self, name: Symbol, member: &str, depth: usize) -> bool {
        if depth > 64 {
            return false;
        }
        let Some(class) = crate::classes::find_class(self.mir, name) else {
            return false;
        };
        let named = |symbol: Symbol| self.symbol_name(symbol) == Ok(member);
        let has_field = class.fields.iter().any(|candidate| named(candidate.name));
        let has_accessor = class
            .descriptors
            .iter()
            .any(|descriptor| !descriptor.is_static && named(descriptor.name));
        let has_method = class.methods.iter().any(|func| {
            usize::try_from(func.0)
                .ok()
                .and_then(|index| self.mir.functions.get(index))
                .is_some_and(|function| {
                    matches!(function.origin, HirOrigin::ClassMethod { method: method_name, .. } if named(method_name))
                })
        });
        has_field
            || has_accessor
            || has_method
            || class
                .base
                .is_some_and(|base| self.mir_class_has_member(base, member, depth.saturating_add(1)))
    }

    /// Return whether a named MIR interface declares a field (own or inherited).
    fn mir_interface_has_field(&self, name: Symbol, field: &str) -> bool {
        self.mir
            .interfaces
            .iter()
            .find(|interface| interface.name == name)
            .is_some_and(|interface| {
                crate::classes::effective_interface_fields(self.mir, interface)
                    .iter()
                    .any(|candidate| self.symbol_name(candidate.name) == Ok(field))
            })
    }

    /// Whether a union arm of type `ty` can hold a value that passes a runtime
    /// kind test (`typeof`, `Array.isArray`) for `kind`.
    ///
    /// A `None` arm is `null`/`undefined` and an optional arm can be absent;
    /// every other arm answers through the shared concrete-type map
    /// (`Type::runtime_kind` / `UnknownKind::satisfies`), the same one
    /// `static_tag_check` uses for a value that is not a union.
    fn union_member_matches_kind(&self, ty: TypeId, kind: smelt_hir::UnknownKind) -> bool {
        use smelt_hir::UnknownKind as Kind;

        match self.mir.types.get(ty) {
            Some(Type::None) => matches!(kind, Kind::Null | Kind::Undefined),
            Some(Type::Optional(_)) => kind == Kind::Undefined,
            Some(member) => member
                .runtime_kind()
                .is_some_and(|actual| actual.satisfies(kind)),
            None => false,
        }
    }
}

/// Emit every canonical concrete union enum used by the MIR type table.
pub(crate) fn emit_union_definitions(
    mir: &Mir,
    context: &EmitContext,
) -> Result<String, EmitError> {
    let Some(function) = mir.functions.first() else {
        return Ok(String::new());
    };
    let emitter = FunctionEmitter::new(mir, context, function)?;
    let mut output = String::new();
    for (index, _) in mir.types.all().iter().enumerate() {
        let type_id = TypeId(compact_index(index, "union type index does not fit u32")?);
        let Some(members) = emitter.concrete_union_members(type_id) else {
            continue;
        };
        let name = union_name(type_id);
        let params = emitter.union_type_params(type_id)?;
        // Everything inside the enum and its impl blocks renders in the enum's
        // own scope: exactly the parameters its headers declare. The probing
        // emitter above renders in the first function's scope, which neither
        // declares the enum's parameters nor may leak its own into the enum.
        let scoped = FunctionEmitter::for_generated_item(
            mir,
            context,
            function,
            params.iter().copied().collect(),
        )?;
        let emitter = &scoped;
        let declaration_generics = if params.is_empty() {
            String::new()
        } else {
            format!(
                "<{}>",
                params
                    .iter()
                    .map(|param| emitter.union_type_param_name(*param))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ")
            )
        };
        let impl_generics = if params.is_empty() {
            String::new()
        } else {
            format!(
                "<{}>",
                params
                    .iter()
                    .map(|param| {
                        Ok(format!(
                            "{}: {}",
                            emitter.union_type_param_name(*param)?,
                            crate::classes::GENERATED_TYPE_PARAM_BOUNDS
                        ))
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?
                    .join(", ")
            )
        };
        // `Default`'s body constructs the FIRST member's default, and a member
        // is routinely another generated type whose own `Default` carries the
        // crate bound set (`classes::GENERATED_TYPE_PARAM_BOUNDS`) — a
        // `Result<T> = Ok<T> | Err<T>` union defaults to `Ok<T>::default()`.
        // `T: Default` alone cannot prove that, so this takes the same set every
        // other generic item the crate emits takes. `Debug` shares the binding
        // and is not weakened by it: it routes through the erased view, which
        // needs `IntoSmeltUnknown` anyway.
        let default_generics = impl_generics.clone();
        let target = emitter.union_declared_type(type_id)?;
        // Render the members in the enum's OWN type-parameter environment. The
        // default environment is the first function's lexical scope, which does
        // not declare the union's parameters, so a member mentioning one erased
        // it: `type ResponseOrInit<T> = ResponseInit<T> | Response` emitted
        // `enum SmeltUnion4<T> { M0(ResponseInit<SmeltUnknown>), M1(SmeltResponse) }`
        // -- `T` declared and used by nothing (E0392), the argument the MIR type
        // carried discarded, and `T` uninferable at any construction site
        // (E0282). `declaration_generics` is derived from exactly these
        // parameters, so spelling them here is what makes the declaration and its
        // members agree. Use sites pass one argument per parameter, resolved
        // in their own scope (`FunctionEmitter::union_type_text_in`).
        let member_scope: ::std::collections::HashSet<Symbol> = params.iter().copied().collect();
        let member_substitution = TypeSubstitution::callee_emission(&member_scope);
        output.push_str("#[derive(Clone)]\n");
        output.push_str(&format!("pub enum {name}{declaration_generics} {{\n"));
        for (member_index, member) in members.iter().enumerate() {
            let member_text = emitter
                .rust_type(*member, false, &member_substitution)?
                .into_string();
            output.push_str(&format!("    M{member_index}({member_text}),\n"));
        }
        output.push_str("}\n");
        if members
            .iter()
            .any(|member| matches!(mir.types.get(*member), Some(Type::Generator { .. })))
        {
            // Generator carriers deliberately cannot cross into
            // `SmeltUnknown`: doing so would erase suspension and the selected
            // sync/async resume protocol. Generator unions therefore remain a
            // typed tagged enum consumed by `yield*` arm dispatch and omit the
            // generic erased conversion/equality implementations below.
            output.push_str(&format!(
                "impl{default_generics} ::std::fmt::Debug for {target} {{\n"
            ));
            output.push_str(
                "    fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str(\"SmeltGeneratorUnion\") }\n",
            );
            output.push_str("}\n\n");
            continue;
        }
        output.push_str(&format!("impl{impl_generics} IntoSmeltUnknown for {target} {{\n"));
        output.push_str("    fn into_smelt_unknown(self) -> SmeltUnknown {\n");
        output.push_str("        match self {\n");
        for (member_index, member) in members.iter().enumerate() {
            let erased = emitter.erase_rendered(&RustExpr::path("value"), *member)?;
            output.push_str(&format!(
                "            Self::M{member_index}(value) => {erased},\n", erased = erased.delimited()
            ));
        }
        output.push_str("        }\n    }\n}\n");
        output.push_str(&format!("impl{impl_generics} {target} {{\n"));
        // Genuine dynamic boundary: recovering a union from an erased value is
        // the adapter's whole job, and `smelt-unknown-report` counts it as one.
        output.push_str("    fn from_smelt_unknown(value: SmeltUnknown) -> Self {\n");
        output.push_str(
            &emitter.union_from_smelt_unknown_body_in_scope(members, &member_scope)?,
        );
        output.push_str("    }\n}\n");
        output.push_str(&format!("impl{impl_generics} PartialEq for {target} {{\n"));
        output.push_str("    fn eq(&self, other: &Self) -> bool {\n");
        output.push_str(
            "        self.clone().into_smelt_unknown() == other.clone().into_smelt_unknown()\n",
        );
        output.push_str("    }\n}\n");
        // `SmeltFromUnknown` is the trait form of the inherent
        // `from_smelt_unknown` emitted just above. Without it a union cannot be
        // used as a generic argument, because every lifted type parameter is
        // bounded by it — es-toolkit's specs instantiate generic helpers at
        // union types and failed with "the trait bound `SmeltUnionN:
        // SmeltFromUnknown` is not satisfied".
        output.push_str(&format!(
            "impl{impl_generics} SmeltFromUnknown for {target} {{\n"
        ));
        output.push_str(
            "    const HOLDS_RECORD: bool = true;\n    fn smelt_from_unknown(value: SmeltUnknown) -> Self { Self::from_smelt_unknown(value) }\n",
        );
        output.push_str("}\n");
        // A union used as a map key needs JavaScript key equality. Both halves
        // route through the erased `SmeltUnknown` view, exactly like the
        // `PartialEq` impl above, so the union agrees with `SmeltUnknown` on key
        // identity — which matters because the same map is often compared
        // against an erased one.
        output.push_str(&format!(
            "impl{impl_generics} SmeltJsKeyEq for {target} {{\n"
        ));
        output.push_str(
            "    fn same_js_key(&self, other: &Self) -> bool { self.clone().into_smelt_unknown().same_js_key(&other.clone().into_smelt_unknown()) }\n",
        );
        output.push_str(
            "    fn js_key_hash(&self) -> Option<u64> { self.clone().into_smelt_unknown().js_key_hash() }\n",
        );
        output.push_str("}\n");
        // Concrete unions surface as fields of generated classes, which derive
        // `Debug` and `Default`. A data-carrying enum can derive neither, and a
        // `#[derive(Debug)]` would additionally demand that every member type be
        // `Debug` (excluding `SmeltJsMap`, function members, and other runtime
        // carriers). Emit both by hand: `Debug` reuses the erased `SmeltUnknown`
        // view (always present, since the union already emits `IntoSmeltUnknown`)
        // exactly like the `PartialEq` impl above, and `Default` selects the
        // first arm, matching how `default_value` builds a concrete-union default.
        output.push_str(&format!("impl{impl_generics} ::std::fmt::Debug for {target} {{\n"));
        output.push_str(
            "    fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {\n",
        );
        output.push_str(
            "        ::std::fmt::Debug::fmt(&self.clone().into_smelt_unknown(), formatter)\n",
        );
        output.push_str("    }\n}\n");
        let first_member = *members
            .first()
            .ok_or_else(|| EmitError::new("concrete union has no members"))?;
        let first_default = emitter.default_value(first_member)?;
        output.push_str(&format!("impl{default_generics} Default for {target} {{\n"));
        output.push_str("    fn default() -> Self {\n");
        output.push_str(&format!("        Self::M0({first_default})\n", first_default = first_default.delimited()));
        output.push_str("    }\n}\n\n");
    }
    Ok(output)
}

/// How one arm of a concrete union answers an element read `value[index]`;
/// see [`FunctionEmitter::concrete_union_index_reads`].
pub(super) enum UnionArmIndexRead {
    /// The arm holds the element: its type and the read, spelled against the
    /// arm binding `smelt_union_arm`.
    Element(TypeId, String),
    /// The arm has no element there (a tuple read past its end): JavaScript
    /// reads `undefined`.
    Absent,
}

/// How one arm of a concrete union answers a named property read; see
/// [`FunctionEmitter::concrete_union_member_reads`].
#[derive(Clone, Copy)]
#[expect(
    variant_size_differences,
    reason = "a 24-byte `Copy` value; boxing `Host` would cost `Copy` and an allocation to save ~16 bytes"
)]
pub(super) enum UnionArmRead {
    /// A record arm's declared field, of this type.
    Field(TypeId),
    /// A modeled host class's data property: its type and how it is read.
    Host(TypeId, smelt_stdlib::HostDataProperty),
    /// A `number`/`boolean` arm: JavaScript reads `undefined`.
    Absent,
}
