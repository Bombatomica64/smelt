//! Generic instantiation at a call site: which callee type parameters a call
//! binds, whether it monomorphizes, and the types its arguments render at.

use super::*;
use crate::generic_bindings::{
    CalleeTypeParamBindings, bind_class_type_params, collect_bindings_from_types,
    substituted_type_id, substitution_matches, type_param_occurs,
};
use crate::rust::RustExpr;

impl FunctionEmitter<'_> {
    /// Return the generic type parameters declared by the class that owns
    /// `function`, when `function` is a class method or constructor.
    ///
    /// Methods and constructors of a generic class are emitted inside an
    /// `impl<T> Class<T>` block, so their signatures reference the class type
    /// parameters `T` directly. Callers use this to detect arguments whose
    /// target parameter is one of those class-level type parameters.
    pub(super) fn callee_class_type_params(&self, function: &MirFunction) -> HashSet<Symbol> {
        self.callee_class_type_param_names(function).into_iter().collect()
    }

    /// The render scope for a value coerced to one of `function`'s **declared**
    /// parameter types.
    ///
    /// A callee parameter is the far side of a seam, so a type parameter may be
    /// spelled as a Rust identifier there only where BOTH sides spell it: the
    /// caller's item must declare the name (or the body cannot write it) and the
    /// callee must have emitted it as a real Rust generic (or its slot is
    /// `SmeltUnknown`). [`RenderScope::narrowed_to`] takes exactly that
    /// intersection.
    ///
    /// The callee's own emission scope is the class type parameters it is
    /// emitted inside plus the free-function type parameters the crate-wide gate
    /// let it keep — [`Self::callee_class_type_params`] and
    /// [`Self::callee_free_function_type_params`], each of which already returns
    /// the empty set for a callee that erases.
    ///
    /// Without this, the caller's lexical scope leaked across the seam: an
    /// argument of the demoted `find_middleware(middleware: SmeltRecord<String,
    /// SmeltList<SmeltUnknown>>, ..)` was rendered `SmeltRecord<String,
    /// SmeltList<T>>` because `RegExpRouter<T>::add` happens to declare a `T`
    /// (8 `E0308`s in the Hono router slice).
    pub(super) fn callee_parameter_render_scope(&self, function: &MirFunction) -> RenderScope {
        let mut emitted = self.callee_class_type_params(function);
        emitted.extend(self.callee_free_function_type_params(function));
        self.render_scope().narrowed_to(&emitted)
    }

    /// Return the generic type parameters declared by a generic free function.
    ///
    /// A generic free function (`function identity<T>(x: T): T`) is emitted as
    /// `fn identity<T: ..>(x: T) -> T`, so its parameter and return positions
    /// reference `T` directly. Callers use this to detect arguments whose target
    /// parameter is one of the callee's own type parameters, which must be
    /// passed through concretely so Rust monomorphizes the call.
    pub(super) fn callee_free_function_type_params(&self, function: &MirFunction) -> HashSet<Symbol> {
        // Only functions the crate decided to emit as real generics participate
        // in argument pass-through. Using the crate-wide decision (rather than
        // the signature-only predicate) keeps the call site in agreement with
        // the emitted definition: a function whose body forced a fall back to
        // erasure has erased (`SmeltUnknown`) parameters, so its arguments must
        // be erased here too, not passed through concretely.
        if !matches!(function.origin, HirOrigin::Body(_))
            || !self.callee_emits_generics(function.id)
        {
            return HashSet::new();
        }
        function
            .type_params
            .iter()
            .map(|param| param.name)
            .collect()
    }

    /// Returns whether a concrete `source` argument type is exactly the callee
    /// parameter `target` with its own generic `params` instantiated.
    ///
    /// A bare `TypeParam` argument (`x: T`) is passed through concretely so Rust
    /// monomorphizes the callee (see the call-site pass-through). A composite
    /// parameter that *contains* a type parameter (`arr: T[]` lowering to
    /// `SmeltList<T>`) is instantiated at the concrete argument shape at the same
    /// call site, so its argument must likewise pass through at its own concrete
    /// element type rather than be erased to `SmeltList<SmeltUnknown>` — erasing
    /// would clash with the monomorphized `SmeltList<f64>` the callee expects.
    ///
    /// The match is exact so pass-through only fires when Rust can actually bind
    /// the parameters from the argument: a `TypeParam` position accepts any
    /// source subtree (it is the binding site), every other constructor must
    /// agree in shape, and a concrete target must equal the source. When the
    /// argument has a different or erased shape (`SmeltUnknown` against
    /// `SmeltList<T>`), this returns `false` and the caller falls back to the
    /// ordinary coercion so the value is erased as before.
    ///
    /// # Relationship to [`crate::generic_bindings`]
    ///
    /// This is *not* dead duplication of the shared matcher, and it is
    /// deliberately not refactored onto it yet. The two answer different
    /// questions and diverge in both directions:
    ///
    /// - **answer shape** — this is a whole-tree conjunction over one
    ///   (source, target) pair, so a mismatch anywhere is fatal;
    ///   `generic_bindings` records evidence per *type parameter* and drops a
    ///   mismatch that lands in a type-parameter-free subtree (a `Dict` key, a
    ///   tuple slot);
    /// - **unions** — this zips equal-length unions positionally and so accepts
    ///   an identical nested union; the shared matcher refuses to bind through
    ///   unions at all, because unions erase in emitted Rust and member order is
    ///   not a sound correspondence;
    /// - **`Class` / `JsMap` / `Function` / generators** — this has no arm for
    ///   them and falls through to identity; the shared matcher descends into
    ///   all of them. Descending here would divert a callback argument off the
    ///   borrowed-callback path and change emitted bytes;
    /// - **erased sources** — a bare `TypeParam` target returns `true` here for
    ///   any source, including `SmeltUnknown`; the shared matcher records that
    ///   as `Erased`, which its only public query reports as not concrete.
    ///
    /// # Remaining scope
    ///
    /// General consumers (the composite argument decision and the return-conversion
    /// ladder) ask [`crate::generic_bindings::substitution_matches`], which is exact
    /// and unforgiving; this whole-tree conjunction is the mutable-list adapter's
    /// private rule, with [`Self::mut_list_adapter_arg`] as its one caller.
    ///
    /// It is kept rather than migrated because the adapter *needs* the
    /// divergences listed above: its `TypeParam`-target-accepts-anything arm is
    /// what classifies a `&mut SmeltList<T>` argument as `Monomorphized`, and
    /// `&mut` invariance makes the adapter's own writeback protocol, not this
    /// predicate, the thing that decides correctness there. Migrating it is a
    /// separate change with its own corpus evidence.
    pub(super) fn generic_param_instantiated_by(
        &self,
        source: TypeId,
        target: TypeId,
        params: &HashSet<Symbol>,
    ) -> bool {
        match (self.mir.types.get(target), self.mir.types.get(source)) {
            (Some(Type::TypeParam { name }), _) if params.contains(name) => true,
            (Some(Type::List(target_item)), Some(Type::List(source_item)))
            | (Some(Type::Set(target_item)), Some(Type::Set(source_item)))
            | (Some(Type::Optional(target_item)), Some(Type::Optional(source_item)))
            | (Some(Type::Future(target_item)), Some(Type::Future(source_item))) => {
                self.generic_param_instantiated_by(*source_item, *target_item, params)
            }
            (Some(Type::Dict(target_key, target_value)), Some(Type::Dict(source_key, source_value))) => {
                self.generic_param_instantiated_by(*source_key, *target_key, params)
                    && self.generic_param_instantiated_by(*source_value, *target_value, params)
            }
            (Some(Type::Tuple(target_items)), Some(Type::Tuple(source_items)))
            | (Some(Type::Union(target_items)), Some(Type::Union(source_items)))
                if target_items.len() == source_items.len() =>
            {
                target_items
                    .iter()
                    .zip(source_items.iter())
                    .all(|(target_item, source_item)| {
                        self.generic_param_instantiated_by(*source_item, *target_item, params)
                    })
            }
            _ => source == target,
        }
    }

    /// Return the generic type parameters declared by the class that owns
    /// `function`, in declaration order.
    ///
    /// [`Self::callee_class_type_params`] answers the membership question and
    /// returns a set; binding needs the order, because the receiver type's class
    /// arguments correspond to the declaration positionally.
    fn callee_class_type_param_names(&self, function: &MirFunction) -> Vec<Symbol> {
        let class_name = match function.origin {
            HirOrigin::ClassConstructor { class, .. } | HirOrigin::ClassMethod { class, .. } => {
                class
            }
            // Static methods are emitted as receiver-free associated functions and
            // do not participate in the receiver's generic instantiation, so they
            // carry no class type parameters relevant to argument pass-through here.
            HirOrigin::Body(_) | HirOrigin::ClassStaticMethod { .. } => return Vec::new(),
        };
        crate::classes::find_class(self.mir, class_name)
            .map(|class| class.type_params.iter().map(|param| param.name).collect())
            .unwrap_or_default()
    }

    /// Render a bare class-type-parameter method argument at the type the
    /// RECEIVER's class arguments pin it to.
    ///
    /// `node.insert(m, p, "x")` against `class Node<T> { insert(.., handler: T) }`
    /// declares `handler: T`, and the default method-argument path renders a
    /// concrete argument at its own type so Rust infers `T` from it. That
    /// inference is only free when nothing else has fixed `T` — but the
    /// receiver already has: its Rust type is `Node<Arg>`, so the method's `T`
    /// IS `Arg` and an argument of any other type is `E0308` (`expected
    /// SmeltUnknown, found String` at every `insert` of a `new Node()` whose `T`
    /// TypeScript infers as `unknown`, 238 errors in Hono's trie-router tests).
    ///
    /// Coerces to the receiver's argument for that parameter; a type parameter
    /// the caller cannot spell (the class's own uninstantiated `T`, which the
    /// receiver's local renders as `SmeltUnknown`) is a source-`unknown`
    /// instantiation, so the argument crosses the existing `SmeltUnknown`
    /// boundary adapter (`erase`). Returns `None` — keep the existing pass-through — when the
    /// target is not a bare class type parameter, when the receiver is not an
    /// instantiation of the callee's class, or when the receiver's argument is
    /// a type parameter the caller itself spells (a generic caller forwarding
    /// its own `T`, which Rust already unifies).
    pub(super) fn receiver_bound_class_param_argument_expr(
        &self,
        function: &MirFunction,
        receiver: &Operand,
        arg: &Operand,
        target_ty: TypeId,
    ) -> Result<Option<RustExpr>, EmitError> {
        let HirOrigin::ClassMethod { class, .. } = function.origin else {
            return Ok(None);
        };
        let Some(Type::TypeParam { name }) = self.mir.types.get(target_ty) else {
            return Ok(None);
        };
        let ordered = self.callee_class_type_param_names(function);
        let Some(position) = ordered.iter().position(|param| param == name) else {
            return Ok(None);
        };
        let Some(Type::Class {
            name: receiver_class,
            args,
        }) = self.mir.types.get(self.operand_ty(receiver)?)
        else {
            return Ok(None);
        };
        if *receiver_class != class || args.len() != ordered.len() {
            return Ok(None);
        }
        let Some(bound) = args.get(position).copied() else {
            return Ok(None);
        };
        match self.mir.types.get(bound) {
            Some(Type::TypeParam { name: bound_name }) => {
                if self.current_function_type_params().contains(bound_name) {
                    Ok(None)
                } else {
                    self.erase(arg).map(Some)
                }
            }
            Some(_) => self.value_at_type(arg, bound).map(Some),
            None => Ok(None),
        }
    }

    /// Return whether `ty` mentions any of `names` anywhere in its structure,
    /// including generic class arguments, `Map` keys and values, and generator
    /// yields (see [`type_param_occurs`]).
    ///
    /// This function's one "mentions a type parameter" walk. A narrower copy
    /// that stopped at class arguments once decided the mutable-list adapter's
    /// return: `takeHeld<T>(arr: T[]): Holder<T>` called on a `number[]` was
    /// treated as a concrete return and rebuilt field by field through a bogus
    /// `SmeltUnknown` extraction against the already-concrete `Holder<f64>`.
    pub(super) fn type_mentions_any(&self, ty: TypeId, names: &HashSet<Symbol>) -> bool {
        names
            .iter()
            .any(|name| type_param_occurs(self.mir, ty, *name))
    }

    /// Decide whether one static call really monomorphizes a generic callee at
    /// the caller's concrete types.
    ///
    /// The return shape is not an input to the decision: what licenses passing an
    /// argument through concretely is that *this call site pins every one of the
    /// callee's type parameters*, not what the callee happens to return. The
    /// substituted return type falls out of the same bindings
    /// ([`CallMonomorphization::return_ty`]), which is how a composite
    /// `T[] -> T[]` callee becomes usable at all.
    ///
    /// Accepts only when every one of the following holds; otherwise this call
    /// site demotes to the ordinary erased path, which is unchanged. Demotion is
    /// per *call site*, never per function: the callee stays generic and other
    /// sites keep instantiating it at `SmeltUnknown`, which its emitted bounds
    /// already satisfy.
    ///
    /// 1. the crate decided to emit this callee with real Rust generics
    ///    ([`Self::callee_free_function_type_params`]). A callee demoted by the
    ///    crate-wide gate — including every callback-bearing one, which the
    ///    callback gate still rejects — never reaches the rest of this check;
    /// 2. the callee packs no rest parameter, which would break the positional
    ///    correspondence between parameters and arguments;
    /// 3. no parameter position that mentions a type parameter is left to the
    ///    trailing default-argument loop, which renders the *erased* default and
    ///    would pin the parameter to `SmeltUnknown`;
    /// 4. every liftable type parameter (see `liftable_type_params`) is
    ///    `Concrete`. An `Unbound` parameter is
    ///    precisely "rustc has nothing to infer from" — passing through anyway
    ///    is E0282/E0283, so the site demotes instead;
    /// 5. every parameter position mentioning a type parameter is *exactly* its
    ///    argument's type under the bindings, so rustc reproduces the binding map
    ///    position by position;
    /// 6. the substituted return type is an interned MIR type, so the call's own
    ///    Rust type can be named rather than guessed.
    pub(super) fn static_call_monomorphization(
        &self,
        function: &MirFunction,
        args: &[Operand],
    ) -> Result<Option<CallMonomorphization>, EmitError> {
        // (1) — the crate-wide generic-emission decision.
        let own_params = self.callee_free_function_type_params(function);
        if own_params.is_empty() {
            return Ok(None);
        }
        // (2) — a packed rest parameter destroys positional alignment.
        if function.rest.is_some() {
            return Ok(None);
        }
        let emitted_params =
            self.emitted_function_param_types(&self.emitted_signature_key(function)?)?;

        // Only the parameters the callee actually LIFTS may be bound here. A
        // parameter that erases is rendered `SmeltUnknown` in the emitted
        // signature, so binding it to the type TypeScript inferred describes a
        // signature that was never emitted. es-toolkit's `countBy<T, K extends
        // PropertyKey>` erases `K`, and binding `K = string` from the call site
        // made the callback adapter declare `-> String` against a callee whose
        // bound reads `Fn(..) -> SmeltUnknown` (E0271), and the result local
        // `SmeltRecord<String, f64>` against a returned `SmeltJsMap<SmeltUnknown,
        // f64>` (E0308).
        let liftable = crate::generic_lifting::liftable_type_params(
            self.mir,
            function,
            &self.context.owned_callback_params,
            &self.context.bound_traits,
            &self.context.erased_arguments,
        );
        let type_params = function
            .type_params
            .iter()
            .map(|param| param.name)
            .filter(|name| liftable.contains(name))
            .collect::<Vec<_>>();
        if type_params.is_empty() {
            return Ok(None);
        }
        let mut declared = Vec::new();
        let mut actual = Vec::new();
        for (index, param) in function.params.iter().enumerate() {
            let local_ty = self.function_local_decl(function, *param)?.ty;
            let target_ty = emitted_params
                .as_ref()
                .and_then(|params| params.get(index).copied())
                .unwrap_or(local_ty);
            let Some(arg) = args.get(index) else {
                // (3) — an omitted argument in a generic-mentioning position
                // would be rendered as the erased default.
                if self.type_mentions_any(target_ty, &own_params) {
                    return Ok(None);
                }
                declared.push(target_ty);
                actual.push(None);
                continue;
            };
            declared.push(target_ty);
            actual.push(Some(self.operand_ty(arg)?));
        }

        let bindings = crate::generic_bindings::collect_bindings_from_types_in_scope(
            self.mir,
            &type_params,
            &declared,
            &actual,
            &self.current_function_type_params(),
        );
        // (4) — every type parameter pinned, or the site demotes.
        if !bindings.all_concrete() {
            return Ok(None);
        }
        // (5) — each generic-mentioning parameter is exactly its argument.
        for (target_ty, supplied) in declared.iter().zip(&actual) {
            let Some(actual_ty) = *supplied else {
                continue;
            };
            // A callback position is rendered through the call-site adapter,
            // which DECLARES the callee's substituted parameter types, so it
            // needs no exact match to pin anything — but only once every
            // parameter it mentions is already pinned by a value position, so
            // rustc still has a direct argument to infer from. (`sumBy(items,
            // item => ..)` inside `meanBy<T>`: the one-argument arrow is
            // adapted to the two-argument callback, and `T` comes from `items`.)
            //
            // Only its parameters may differ. A parameter accepts more than the
            // pinned type, but a return is a further candidate for `T`:
            // TypeScript widens `T` to `unknown` for a callback returning
            // `unknown`, and the adapter would convert that value to the pinned
            // type instead of carrying it.
            let adapted_callback = callback_return_fits(self.mir, *target_ty, actual_ty, &bindings)
                && type_params_pinned_by_values(self.mir, &declared, &actual, *target_ty, &bindings);
            if self.type_mentions_any(*target_ty, &own_params)
                && !adapted_callback
                && !substitution_matches(self.mir, *target_ty, actual_ty, &bindings)
            {
                return Ok(None);
            }
        }
        // (6) — the call's own Rust type must be nameable.
        let declared_return = self
            .emitted_function_return_type(&self.emitted_signature_key(function)?)
            .unwrap_or(function.return_ty);
        let Some(return_ty) = substituted_type_id(self.mir, declared_return, &bindings) else {
            return Ok(None);
        };
        Ok(Some(CallMonomorphization {
            bindings,
            return_ty,
        }))
    }

    /// Render a constructor or method argument against a callee parameter type,
    /// keeping the value concrete when the parameter is one of the callee
    /// class's own generic type parameters.
    ///
    /// A call such as `new Box<number>(3)` monomorphizes the emitted
    /// `impl<T> Box<T>` to `Box<f64>`, so the constructor parameter `value: T`
    /// resolves to `f64` at this site. Coercing the argument against the bare
    /// `TypeParam` target would erase it to `SmeltUnknown` (the calling function
    /// has no `T` in scope), producing a type mismatch against the generic
    /// parameter. Instead the argument is passed through at its own concrete
    /// type and Rust infers the class type argument from the value, which is the
    /// point of emitting real generics rather than erasing them.
    pub(super) fn callee_generic_argument_expr(
        &self,
        arg: &Operand,
        function: &MirFunction,
        target_ty: TypeId,
        class_type_params: &HashSet<Symbol>,
    ) -> Result<RustExpr, EmitError> {
        if let Some(Type::TypeParam { name }) = self.mir.types.get(target_ty)
            && class_type_params.contains(name)
        {
            let source_ty = self.operand_ty(arg)?;
            // A `TypeParam` source is already the class generic itself (e.g. one
            // generic method forwarding to another); pass it straight through.
            // Any concrete source is bound to the class type argument by Rust
            // inference, so rendering it at its own type keeps it concrete.
            let _ = function;
            return self.value_at_type(arg, source_ty);
        }
        // The same rule one level down. A constructor or method parameter can
        // MENTION the callee's type parameters without being one — `class
        // Pair<A> { constructor(left: Cell<A>) }` declares `Cell<A>` — and the
        // declared spelling is the only thing MIR hands this site: nothing
        // substituted `A`. Coercing the argument to it asks for a `Cell<A>`
        // where a `Cell<f64>` is in hand, and the record adapter dutifully
        // rebuilds the struct with its `value` field erased to `SmeltUnknown`
        // (round 32, "a record rebuilt at a substituted argument erases its
        // field").
        //
        // The argument's own type is the evidence Rust's inference would use,
        // so bind the callee's parameters from the declared/actual pair and
        // coerce to the SUBSTITUTED parameter type. Where nothing is bound, or
        // the substituted type was never interned, `substituted_type_id` fails
        // closed and the declared type stays in charge exactly as before.
        if !class_type_params.is_empty() {
            let declared_names = class_type_params.iter().copied().collect::<Vec<_>>();
            // A parameter bound to one of the CALLER's own generics is that
            // generic (`new Holder<Value>(.., this.validate)` inside
            // `Parser<Value>` binds `Holder`'s `Value` to `Parser`'s).
            let bindings = crate::generic_bindings::collect_bindings_from_types_in_scope(
                self.mir,
                &declared_names,
                &[target_ty],
                &[Some(self.operand_ty(arg)?)],
                &self.current_function_type_params(),
            );
            // A callback is adapted at this site, and the adapter DECLARES the
            // callee's parameter types: they are rendered through the bindings
            // (in the caller's scope), so they need no interned substituted
            // function type. Without them the class's own parameter renders
            // erased in the caller and the adapter takes `&SmeltUnknown` where
            // the constructor's `impl<Value>` expects `&Value` (E0631).
            if matches!(self.mir.types.get(target_ty), Some(Type::Function(_)))
                && self.type_mentions_any(target_ty, class_type_params)
                && class_type_params.iter().all(|name| {
                    !type_param_occurs(self.mir, target_ty, *name)
                        || matches!(
                            bindings.get(*name),
                            Some(crate::generic_bindings::TypeParamBinding::Concrete(_))
                        )
                })
            {
                if let Some(adapter) =
                    self.function_shape_adapter(arg, target_ty, false, Some(&bindings))?
                {
                    return Ok(adapter);
                }
                // Under this site's bindings the callback already has the
                // parameter's shape (`this.set` handed to `new ValueSetter<
                // Value>(..)` inside `Parser<Value>`: both `Value`s are the
                // caller's), so it passes as it is. Falling through instead
                // re-coerced it WITHOUT the bindings, and that adapter
                // declared the callee's `Value` erased (E0631).
                return self.value_at_type(arg, self.operand_ty(arg)?);
            }
            if let Some(substituted) = substituted_type_id(self.mir, target_ty, &bindings)
                && substituted != target_ty
            {
                return self.value_at_type(arg, substituted);
            }
        }
        self.value_at_type(arg, target_ty)
    }

    /// Resolve a method parameter type against the argument's own type when it
    /// mentions the callee class's type parameters.
    ///
    /// The declared parameter of `class Bag<T> { each(cb: (x: T) => void) }` is
    /// spelled with the class's `T`, which the calling function has no binding
    /// for. The argument's type is the evidence Rust's inference uses, so the
    /// class parameters are bound from the declared/actual pair (the same rule
    /// as [`Self::callee_generic_argument_expr`]) and the substituted type is
    /// returned. With no class type parameters, nothing bound, or no interned
    /// substitution, the declared type is returned unchanged.
    pub(super) fn method_argument_substituted_target(
        &self,
        arg: &Operand,
        target_ty: TypeId,
        class_type_params: &HashSet<Symbol>,
    ) -> Result<TypeId, EmitError> {
        if class_type_params.is_empty() {
            return Ok(target_ty);
        }
        let declared_names = class_type_params.iter().copied().collect::<Vec<_>>();
        let bindings = collect_bindings_from_types(
            self.mir,
            &declared_names,
            &[target_ty],
            &[Some(self.operand_ty(arg)?)],
        );
        Ok(substituted_type_id(self.mir, target_ty, &bindings).unwrap_or(target_ty))
    }

    /// Return a generic class method's return type with the receiver's class
    /// arguments substituted in, when this call really instantiates them.
    ///
    /// `None` — meaning "keep the ordinary erased path" — when the callee is not
    /// a method of a generic class, when its return mentions no class type
    /// parameter (there is nothing to substitute, and the emitted signature is
    /// already the answer), when the receiver does not pin every class type
    /// parameter concretely, or when the substituted type is not interned.
    pub(super) fn class_method_substituted_return_ty(
        &self,
        function: &MirFunction,
        args: &[Operand],
    ) -> Result<Option<TypeId>, EmitError> {
        if !matches!(function.origin, HirOrigin::ClassMethod { .. }) {
            return Ok(None);
        }
        let class_params = self.callee_class_type_params(function);
        if class_params.is_empty() || !self.type_mentions_any(function.return_ty, &class_params) {
            return Ok(None);
        }
        // The receiver is `args[0]` for every emitted method call; see the
        // `ClassMethod` arm of `rendered_call`, which splits it off first.
        let Some(receiver) = args.first() else {
            return Ok(None);
        };
        if crate::classes::returns_the_receiver_at_its_own_class(self.mir, function)
            && let Some(Type::Class { name, .. }) = self.mir.types.get(self.operand_ty(receiver)?)
            && !matches!(function.origin, HirOrigin::ClassMethod { class, .. } if class == *name)
        {
            return Ok(Some(self.operand_ty(receiver)?));
        }
        let ordered = self.callee_class_type_param_names(function);
        let Some(receiver_ty) = self.declaring_class_instantiation(function, self.operand_ty(receiver)?)
        else {
            return Ok(None);
        };
        let bindings = bind_class_type_params(self.mir, &ordered, receiver_ty);
        if !bindings.all_concrete() {
            return Ok(None);
        }
        Ok(substituted_type_id(self.mir, function.return_ty, &bindings))
    }

    /// The receiver of a class method call, seen as an instance of the
    /// method's DECLARING class.
    ///
    /// A subclass receiver calls the copy of a base method emitted into its own
    /// `impl` (`crate::classes::substituted_inherited_member`), where the
    /// base's parameters are the subclass's arguments: `YearParser extends
    /// Parser<number>` calls `run` at `Parser<f64>`, and `Leaf extends
    /// Middle<string>`, `Middle<T> extends Base<T[]>` calls `Base`'s methods at
    /// `Base<string[]>` (binding the receiver's own arguments positionally
    /// would have given `T = string`). A copy returning its receiver returns
    /// `Self`, i.e. the receiver's own type. `None` when a composed type is not
    /// interned.
    fn declaring_class_instantiation(
        &self,
        function: &MirFunction,
        receiver_ty: TypeId,
    ) -> Option<TypeId> {
        let HirOrigin::ClassMethod { class: declaring, .. } = function.origin else {
            return Some(receiver_ty);
        };
        let Some(Type::Class { name, args }) = self.mir.types.get(receiver_ty) else {
            return Some(receiver_ty);
        };
        if *name == declaring {
            return Some(receiver_ty);
        }
        let inherited = crate::classes::inherited_type_substitution(self.mir, *name, declaring);
        let receiver_class = crate::classes::find_class(self.mir, *name)?;
        let receiver_args: HashMap<Symbol, TypeId> = receiver_class
            .type_params
            .iter()
            .map(|param| param.name)
            .zip(args.iter().copied())
            .collect();
        let declaring_class = crate::classes::find_class(self.mir, declaring)?;
        let declaring_args = declaring_class
            .type_params
            .iter()
            .map(|param| {
                let in_receiver_scope = inherited.get(&param.name).copied()?;
                Some(crate::generic_bindings::substitute_existing_type(
                    self.mir,
                    in_receiver_scope,
                    &receiver_args,
                ))
            })
            .collect::<Option<Vec<_>>>()?;
        self.mir.types.lookup(&Type::Class {
            name: declaring,
            args: declaring_args,
        })
    }

    /// Return a monomorphized generic free function's substituted return type.
    ///
    /// `None` when this call site does not monomorphize the callee, and also
    /// when the callee's return mentions none of its own type parameters: there
    /// the emitted signature already states the return type and the ordinary
    /// ladder below is the right answer, so the argument-side decision must not
    /// perturb it.
    pub(super) fn monomorphized_free_function_return(
        &self,
        function: &MirFunction,
        args: &[Operand],
    ) -> Result<Option<TypeId>, EmitError> {
        let own_params = self.callee_free_function_type_params(function);
        if own_params.is_empty() || !self.type_mentions_any(function.return_ty, &own_params) {
            return Ok(None);
        }
        Ok(self
            .static_call_monomorphization(function, args)?
            .map(|monomorphization| monomorphization.return_ty))
    }

    /// The monomorphization decision for a static callee, for seam checking.
    ///
    /// Debug-only. Returns `None` for a non-static callee and for any failure:
    /// a check may not change emission, including by failing differently from
    /// the emission path it observes.
    #[cfg(debug_assertions)]
    pub(super) fn sampled_call_monomorphization(
        &self,
        callee: &Callee,
        args: &[Operand],
    ) -> Option<CallMonomorphization> {
        let Callee::Static(func) = callee else {
            return None;
        };
        let function = self
            .mir
            .functions
            .get(id_index(func.0, "function index does not fit usize").ok()?)?;
        self.static_call_monomorphization(function, args).ok()?
    }

    /// A callee's Rust name for a seam-assertion message.
    #[cfg(debug_assertions)]
    pub(super) fn sampled_callee_name(&self, callee: &Callee) -> String {
        let Callee::Static(func) = callee else {
            return "<indirect callee>".to_owned();
        };
        id_index(func.0, "function index does not fit usize")
            .ok()
            .and_then(|index| self.mir.functions.get(index))
            .and_then(|function| self.function_rust_name(function).ok())
            .unwrap_or_else(|| "<unnameable callee>".to_owned())
    }
}

/// The concrete instantiation one static call site pins on a generic callee.
///
/// Produced by [`FunctionEmitter::static_call_monomorphization`] and consumed by
/// both halves of the same call: the argument rendering and the type the call
/// expression is claimed to produce. Sharing one decision between them is what
/// keeps the two sides from disagreeing — a monomorphized argument with an
/// erased return claim (or the reverse) is exactly the E0308 this analysis
/// exists to prevent.
// `Debug`/`Eq` exist only for the debug-only seam assertion that compares the
// argument half of a call site against its return half; a release build derives
// nothing, exactly as before the assertions landed.
#[cfg_attr(debug_assertions, derive(Debug, Eq, PartialEq))]
pub(super) struct CallMonomorphization {
    /// Every declared type parameter of the callee, pinned to a concrete type.
    pub(super) bindings: CalleeTypeParamBindings,
    /// The callee's declared return type with `bindings` applied.
    ///
    /// This is the type the emitted Rust call really evaluates to, which is not
    /// in general the destination local's type: the frontend's destination
    /// carries the type *after* the surrounding conversion (`last<T>(xs): T |
    /// undefined` called on a `number[][]` produces `Option<SmeltList<f64>>`
    /// into a `SmeltList<f64>` destination).
    return_ty: TypeId,
}

/// Whether both types are functions and the actual callback returns exactly
/// the declared return under `bindings`.
fn callback_return_fits(
    mir: &Mir,
    declared: TypeId,
    actual: TypeId,
    bindings: &CalleeTypeParamBindings,
) -> bool {
    let (Some(Type::Function(declared)), Some(Type::Function(actual))) =
        (mir.types.get(declared), mir.types.get(actual))
    else {
        return false;
    };
    substitution_matches(mir, declared.return_ty, actual.return_ty, bindings)
}

/// Whether every type parameter `callback_ty` mentions is pinned at this call
/// site by some NON-callback position whose declared type matches its argument
/// exactly under `bindings` (see `static_call_monomorphization`, check 5).
fn type_params_pinned_by_values(
    mir: &Mir,
    declared: &[TypeId],
    actual: &[Option<TypeId>],
    callback_ty: TypeId,
    bindings: &CalleeTypeParamBindings,
) -> bool {
    let mut mentioned = HashSet::new();
    crate::generic_bindings::collect_type_param_occurrences(mir, callback_ty, &mut mentioned);
    mentioned.iter().all(|name| {
        declared.iter().zip(actual).any(|(declared_ty, supplied)| {
            let Some(actual_ty) = supplied else { return false };
            !matches!(mir.types.get(*declared_ty), Some(Type::Function(_)))
                && type_param_occurs(mir, *declared_ty, *name)
                && substitution_matches(mir, *declared_ty, *actual_ty, bindings)
        })
    })
}
