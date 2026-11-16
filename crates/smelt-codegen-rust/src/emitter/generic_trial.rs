//! The generic trial: rendering a function once with its own type parameters
//! kept, to decide whether its Rust definition can stay generic.

use super::*;
use std::collections::HashSet;

impl<'mir> FunctionEmitter<'mir> {
    /// An emitter for a generic free function's body-cleanliness trial (see
    /// [`Self::renders_real_generics`] and [`emit_role::OwnGenerics::Trial`]).
    pub(crate) fn for_generic_trial(
        mir: &'mir Mir,
        context: &'mir EmitContext,
        function: &'mir MirFunction,
    ) -> Result<Self, EmitError> {
        let mut emitter = Self::new(mir, context, function)?;
        emitter.role = emit_role::EmitRole::Own {
            generics: emit_role::OwnGenerics::Trial,
        };
        Ok(emitter)
    }

    /// Apply the crate-wide generic decision ([`emit_role::crate_generic_decision`])
    /// to an emitter in the [`emit_role::EmitRole::Own`] role; other roles have
    /// no own generics to decide.
    pub(crate) fn apply_crate_generic_decision(&mut self) {
        if let emit_role::EmitRole::Own { generics } = &mut self.role {
            *generics = emit_role::crate_generic_decision(self.context, self.function);
        }
    }

    /// Force an own item's type parameters out of scope, whatever the crate
    /// decision says (the typedness measurement of closures with no owner).
    pub(crate) fn erase_own_generics(&mut self) {
        self.role = emit_role::EmitRole::Own {
            generics: emit_role::OwnGenerics::Erased,
        };
    }

    /// The free function currently on body-cleanliness trial: this emitter's
    /// own function in the trial role, or the one a closure inherited.
    pub(super) const fn trial_generic_function(&self) -> Option<FuncId> {
        match &self.role {
            emit_role::EmitRole::Own {
                generics: emit_role::OwnGenerics::Trial,
            } => Some(self.function.id),
            emit_role::EmitRole::Closure(scope) => scope.trial_function,
            emit_role::EmitRole::Own { .. }
            | emit_role::EmitRole::InheritedCopy { .. }
            | emit_role::EmitRole::BaseInitializerCopy { .. } => None,
        }
    }

    /// Return whether this generic free function's body keeps every type
    /// parameter opaque, so the function can emit real Rust generics.
    ///
    /// Trial-renders the body with the type parameters in scope and reads whether
    /// the render touched the erased carrier ([`EmitContext::note_carrier_use`]).
    /// A clean body (pure passthrough) touches nothing and keeps its generics; a
    /// body that inspects, compares, or erases a `T`-typed value falls back to
    /// full erasure. A thrown payload and a mutable-list adapter are real dynamic
    /// boundaries and do not count ([`EmitContext::carrier_boundary`]). The scratch
    /// declared-locals state is reset afterwards so the real emit renders from a
    /// clean slate. The caller only invokes this for signature-generic-safe
    /// functions, so the type parameters are in scope during the trial.
    pub(crate) fn renders_real_generics(&self) -> Result<bool, EmitError> {
        let mut body = String::new();
        // The crate-wide set grows from empty, so a self-recursive function
        // would never see itself as generic and its recursive call would erase
        // its own `T` (`mk(x.into_smelt_unknown())` inside `mk<T>`), failing
        // its own trial forever. The trial asks "does the body stay clean IF
        // this function is generic", so it assumes exactly that.
        // That assumption is this emitter's role (`OwnGenerics::Trial`, see
        // `FunctionEmitter::for_generic_trial`).
        //
        // The rendered text is never emitted, so everything the render mints into the shared context (accessors, hoisted helpers,
        // erasure records) is rolled back when the checkpoint drops.
        {
            let _trial = self.context.checkpoint();
            self.context.take_carrier_touched();
            self.emit_body(&mut body)?;
        }
        let touched_carrier = self.context.take_carrier_touched();
        self.reset_body_emission_state();
        // A partial lift is governed by `classes::type_param_only_moved`, which
        // asks MIR whether each *lifted* parameter is ever inspected. The carrier
        // record cannot be used there: the parameters that did not lift render as
        // `SmeltUnknown` by design, so they touch the carrier legitimately and
        // would demote the whole function, which is the all-or-nothing behaviour
        // per-parameter lifting exists to remove.
        let liftable = crate::generic_lifting::liftable_type_params(
            self.mir,
            self.function,
            &self.context.owned_callback_params,
            &self.context.bound_traits,
            self.context.type_param_elision(),
            &self.context.erased_arguments,
        );
        if liftable.len() < self.function.type_params.len() {
            return Ok(!liftable.is_empty());
        }
        Ok(!touched_carrier)
    }

    /// Whether a call to the free function `id` renders against its generic
    /// signature: the crate-wide decision, or the function currently on trial
    /// (see [`Self::trial_generic_function`]).
    pub(super) fn callee_emits_generics(&self, id: FuncId) -> bool {
        self.trial_generic_function() == Some(id) || self.context.is_generic_function(id)
    }

    /// Whether every type parameter of this function is spelled by one of its
    /// emitted parameter types, so a caller's arguments pin it.
    ///
    /// A parameter MIR still mentions can vanish from the rendered signature
    /// (an alias whose own parameter is elided), and a generic no argument
    /// names is `E0282` at every call site.
    ///
    /// A return type MIR spells with a parameter must also render with it: a
    /// union return that collapses to `SmeltUnknown` would erase the `T` the
    /// caller expects back, so such a function stays erased whole.
    pub(crate) fn signature_spells_own_type_params(&self) -> Result<bool, EmitError> {
        let params = self.param_decls_text(0)?;
        let returned = self.return_type(self.function.return_ty)?.into_string();
        let words_of = |text: &str| -> HashSet<String> {
            text.split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
                .map(str::to_owned)
                .collect()
        };
        let param_words = words_of(&params);
        let return_words = words_of(&returned);
        Ok(self.function.type_params.iter().all(|type_param| {
            let Some(name) = self.mir.symbols.get(type_param.name) else {
                return false;
            };
            let ident = crate::names::type_param(name);
            let return_mentions = crate::generic_bindings::type_param_occurs(
                self.mir,
                self.function.return_ty,
                type_param.name,
            );
            param_words.contains(ident.as_str())
                && (!return_mentions || return_words.contains(ident.as_str()))
        }))
    }

    /// Whether the body stores or returns `null` at `T | null` of one of its
    /// own type parameters, anywhere (its closures included).
    ///
    /// The erased `Option<SmeltUnknown>` keeps `null` apart from `undefined`
    /// (`Some(SmeltUnknown::Null)`); an `Option<T>` has one absent value, so a
    /// lifted function would read `null` back as `undefined` (radash's
    /// `draw([]) === null`). Such a function stays erased whichever trial
    /// would otherwise lift it.
    pub(crate) fn stores_null_at_own_optional(&self) -> bool {
        let own: Vec<Symbol> = self
            .function
            .type_params
            .iter()
            .map(|type_param| type_param.name)
            .collect();
        let null_into_own_optional = |ty: TypeId| {
            matches!(
                self.mir.types.get(ty),
                Some(Type::Optional(inner))
                    if matches!(self.mir.types.get(*inner), Some(Type::TypeParam { name }) if own.contains(name))
            )
        };
        let return_ty = self.body_return_ty();
        let mut pending: Vec<(&[LocalDecl], &[BasicBlock], bool)> =
            vec![(&self.function.locals, &self.function.blocks, true)];
        let mut seen: HashSet<smelt_mir::ClosureId> = HashSet::new();
        while let Some((locals, blocks, top_level)) = pending.pop() {
            for block in blocks {
                for statement in &block.statements {
                    let Statement::Assign { dest, value } = statement else {
                        continue;
                    };
                    if let Rvalue::Closure { id, .. } = value
                        && seen.insert(*id)
                        && let Some(closure) = self
                            .mir
                            .closures
                            .iter()
                            .find(|candidate| candidate.id == *id)
                    {
                        pending.push((&closure.locals, &closure.blocks, false));
                    }
                    if matches!(value, Rvalue::Use(Operand::Const(Constant::None)))
                        && usize::try_from(dest.0)
                            .ok()
                            .and_then(|index| locals.get(index))
                            .is_some_and(|decl| null_into_own_optional(decl.ty))
                    {
                        return true;
                    }
                }
                if top_level
                    && matches!(
                        &block.terminator,
                        Some(Terminator::Return(Operand::Const(Constant::None)))
                    )
                    && null_into_own_optional(return_ty)
                {
                    return true;
                }
            }
        }
        false
    }

    /// Whether every seam where an own type parameter meets the erased
    /// carrier keeps the value's identity and presence, the precondition for
    /// lifting a body whose render touched the carrier
    /// ([`crate::generic_lifting::type_params_stay_opaque`] answers the rest).
    ///
    /// Only a bare `T` (or an optional one) crosses into the carrier whole,
    /// through `IntoSmeltUnknown`. A container of `T` does not: erasing a
    /// `SmeltList<T>` builds a new `SmeltArray` of erased elements (same id,
    /// separate storage), so a push through the erased copy is lost to the
    /// caller, while the erased function shares the one buffer. So these seams
    /// keep the function erased:
    /// - a `T`-container returned, stored or computed into a type that holds
    ///   the carrier (`T[]` returned as `unknown` or `unknown[]`);
    /// - a `T`-container handed to an erased callee's parameter that holds
    ///   the carrier or the callee's own (erased) type parameter, unless the
    ///   callee only reads it ([`smelt_mir::ReadOnlyArguments`]);
    /// - an erased container recovered where a `T`-container is wanted
    ///   (`key(group)` of a `new Array(n)` handed to a `(vals: T[]) => K`
    ///   callback): `SmeltFromUnknown` builds a new buffer, which only a list
    ///   this body minted itself and never names again cannot tell apart;
    /// - a whole erased value returned at, or stored into a handed-on local
    ///   of, a type that structures an own `T` (`new Impl(x) as any` returned
    ///   as `Matcher<T>`): the rebuilt structure is a snapshot, not the object;
    /// - `null` stored or returned at `T | null` (`Option<T>` has one absent
    ///   value, so `null` would read back as `undefined`);
    /// - an optional `T` read as a bare `T` (`defaultValue as T` of an absent
    ///   optional): an erased `T` carries the absence as `undefined`, a real
    ///   `T` cannot hold it at all.
    pub(crate) fn erased_seams_keep_identity(&self) -> Result<bool, EmitError> {
        let own: Vec<Symbol> = self
            .function
            .type_params
            .iter()
            .map(|type_param| type_param.name)
            .collect();
        let mentions_own = |ty: TypeId| {
            own.iter()
                .any(|name| crate::generic_bindings::type_param_occurs(self.mir, ty, *name))
        };
        // A value whose type mentions an own parameter other than as a bare
        // (possibly optional) `T`: a container that erasing would copy.
        let own_container = |ty: TypeId| mentions_own(ty) && !whole_erased_slot(self.mir, ty);
        let holds_carrier =
            |ty: TypeId| crate::generic_lifting::type_mentions_unknown(self.mir, ty);
        let return_ty = self.body_return_ty();
        // `null` stored at `T | null` of an own `T`: the erased
        // `Option<SmeltUnknown>` keeps it apart from `undefined`
        // (`Some(SmeltUnknown::Null)`), an `Option<T>` cannot.
        let null_into_own_optional = |ty: TypeId| {
            matches!(
                self.mir.types.get(ty),
                Some(Type::Optional(inner))
                    if matches!(self.mir.types.get(*inner), Some(Type::TypeParam { name }) if own.contains(name))
            )
        };
        let mut pending: Vec<(&[LocalDecl], &[BasicBlock], bool)> =
            vec![(&self.function.locals, &self.function.blocks, true)];
        let mut seen: HashSet<smelt_mir::ClosureId> = HashSet::new();
        while let Some((locals, blocks, top_level)) = pending.pop() {
            let local_ty = |local: LocalId| -> Option<TypeId> {
                usize::try_from(local.0)
                    .ok()
                    .and_then(|index| locals.get(index))
                    .map(|decl| decl.ty)
            };
            let operand_ty =
                |operand: &Operand| -> Option<TypeId> { operand.local().and_then(local_ty) };
            // An erased container (`unknown[]`) local, and whether this body
            // minted every buffer it holds and names it whole at most once:
            // then a recovered copy of it at that one use is unobservable.
            // Only a list has a buffer to share; an erased callable recovered
            // at a typed signature is wrapped, not copied.
            let erased_container = |local: LocalId| {
                local_ty(local).is_some_and(|ty| {
                    let list = match self.mir.types.get(ty) {
                        Some(Type::Optional(inner)) => *inner,
                        _ => ty,
                    };
                    matches!(self.mir.types.get(list), Some(Type::List(_))) && holds_carrier(list)
                })
            };
            let mut fresh_defs: HashMap<LocalId, bool> = HashMap::new();
            let mut whole_reads: HashMap<LocalId, usize> = HashMap::new();
            // Locals some site reads whole other than a fresh copy or a length:
            // a site that can keep, mutate or compare the buffer itself.
            let mut handed_on: HashSet<LocalId> = HashSet::new();
            let whole_local = Operand::local;
            for block in blocks {
                for statement in &block.statements {
                    // An equality test or a negation reads presence or
                    // identity, which an identity-keeping copy (`with_id`)
                    // answers the same.
                    let only_copies = match statement {
                        Statement::Assign { value, .. } => {
                            smelt_mir::rvalue_mints_fresh_list(value)
                                || matches!(
                                    value,
                                    Rvalue::Len { .. }
                                        | Rvalue::Unary {
                                            op: smelt_hir::UnaryOp::Not,
                                            ..
                                        }
                                        | Rvalue::Binary {
                                            op: smelt_hir::BinOp::Eq
                                                | smelt_hir::BinOp::NotEq
                                                | smelt_hir::BinOp::StrictEq
                                                | smelt_hir::BinOp::StrictNotEq
                                                | smelt_hir::BinOp::JsStrictEq
                                                | smelt_hir::BinOp::JsStrictNotEq,
                                            ..
                                        }
                                )
                        }
                        // A store into a container or a whole-binding rebind
                        // keeps what it reads; a fused entry update reads only
                        // scalars, which hold no buffer.
                        Statement::AssignPlace { .. }
                        | Statement::DictEntryUpdate { .. }
                        | Statement::StorageLive(_) => false,
                    };
                    statement.reads(|read| {
                        if let Read::Operand(operand) = read
                            && let Some(local) = whole_local(operand)
                        {
                            let count = whole_reads.entry(local).or_default();
                            *count = count.saturating_add(1);
                            if !only_copies {
                                handed_on.insert(local);
                            }
                        }
                    });
                    match statement {
                        Statement::Assign { dest, value } => {
                            let fresh = smelt_mir::rvalue_mints_fresh_list(value);
                            let entry = fresh_defs.entry(*dest).or_insert(true);
                            *entry &= fresh;
                        }
                        Statement::AssignPlace {
                            place: Place::Local(dest),
                            ..
                        }
                        | Statement::DictEntryUpdate { current: dest, .. } => {
                            fresh_defs.insert(*dest, false);
                        }
                        // A projected write defines no local, and a fresh binding
                        // cell defines no value.
                        Statement::AssignPlace { .. } | Statement::StorageLive(_) => {}
                    }
                }
                if let Some(terminator) = &block.terminator {
                    // A branch reads only truthiness, and a callee that only
                    // reads an argument keeps and changes nothing of it.
                    let branches = matches!(
                        terminator,
                        Terminator::Switch { .. } | Terminator::Match { .. }
                    );
                    let read_only_args: HashSet<usize> = if let Terminator::Call {
                        callee: Callee::Static(callee),
                        args,
                        ..
                    } = terminator
                    {
                        (0..args.len())
                            .filter(|index| {
                                self.context
                                    .read_only_arguments(self.mir)
                                    .argument_is_read_only(*callee, *index, args.len())
                            })
                            .collect()
                    } else {
                        HashSet::new()
                    };
                    let call_args: Vec<&Operand> = if let Terminator::Call { args, .. } = terminator
                    {
                        args.iter().collect()
                    } else {
                        Vec::new()
                    };
                    terminator.reads(|operand| {
                        if let Some(local) = whole_local(operand) {
                            let count = whole_reads.entry(local).or_default();
                            *count = count.saturating_add(1);
                            let read_only = call_args.iter().enumerate().any(|(index, arg)| {
                                std::ptr::eq(*arg, operand) && read_only_args.contains(&index)
                            });
                            if !branches && !read_only {
                                handed_on.insert(local);
                            }
                        }
                    });
                    if let Terminator::Call { dest, .. } | Terminator::Await { dest, .. } =
                        terminator
                    {
                        fresh_defs.insert(*dest, false);
                    }
                }
            }
            // A destination this body defines (not a parameter or capture,
            // which the caller still names).
            let dest_is_local_copy = |local: LocalId| fresh_defs.contains_key(&local);
            // The values stored into each local by element or field writes
            // (`result[i] = row`).
            let mut stored_into: HashMap<LocalId, Vec<Operand>> = HashMap::new();
            for block in blocks {
                for statement in &block.statements {
                    if let Statement::AssignPlace {
                        place: Place::Index { base, .. } | Place::Field { base, .. },
                        value,
                    } = statement
                    {
                        value.reads(|operand| {
                            stored_into.entry(*base).or_default().push(operand.clone());
                        });
                    }
                }
            }
            // A list nothing but this body ever names: every buffer it holds
            // was minted here, it is named whole at most once (the use being
            // judged), and every list stored into it is unshared too. A copy
            // of it at that one use (a `SmeltFromUnknown` recovery) cannot be
            // told from the list itself; element VALUES that are not lists
            // cross whole, as every bare `T` does.
            let is_list_local = |local: LocalId| {
                local_ty(local).is_some_and(|ty| {
                    let list = match self.mir.types.get(ty) {
                        Some(Type::Optional(inner)) => *inner,
                        _ => ty,
                    };
                    matches!(self.mir.types.get(list), Some(Type::List(_)))
                })
            };
            let unshared = |root: LocalId| {
                let mut worklist = vec![root];
                let mut visited: HashSet<LocalId> = HashSet::new();
                while let Some(local) = worklist.pop() {
                    if !visited.insert(local) {
                        continue;
                    }
                    if !fresh_defs.get(&local).copied().unwrap_or(false)
                        || whole_reads.get(&local).copied().unwrap_or(0) > 1
                    {
                        return false;
                    }
                    for operand in stored_into.get(&local).into_iter().flatten() {
                        if let Some(element) = whole_local(operand)
                            && is_list_local(element)
                        {
                            worklist.push(element);
                        }
                    }
                }
                true
            };
            // A whole read of an erased container at a site that also carries
            // an own `T` (its destination, or another operand) may recover it
            // into a `T`-container.
            let recovers_shared = |operands: &[Operand], dest: Option<LocalId>| {
                // A recovered copy only ever copied again (`Array.from` of it)
                // or measured is never observed as a buffer of its own.
                if let Some(copy) = dest
                    && dest_is_local_copy(copy)
                    && !handed_on.contains(&copy)
                {
                    return false;
                }
                let involves_own = dest.and_then(local_ty).is_some_and(mentions_own)
                    || operands
                        .iter()
                        .any(|operand| operand_ty(operand).is_some_and(mentions_own));
                involves_own
                    && operands.iter().any(|operand| {
                        let (Operand::Copy(Place::Local(local))
                        | Operand::Move(Place::Local(local))) = operand
                        else {
                            return false;
                        };
                        erased_container(*local) && !unshared(*local)
                    })
            };
            for block in blocks {
                if let Some(terminator) = &block.terminator {
                    let mut operands: Vec<Operand> = Vec::new();
                    terminator.reads(|operand| operands.push(operand.clone()));
                    let dest = terminator.defs();
                    if !matches!(terminator, Terminator::Return(_))
                        && recovers_shared(&operands, dest)
                    {
                        return Ok(false);
                    }
                }
                for statement in &block.statements {
                    let Statement::Assign { dest, value } = statement else {
                        continue;
                    };
                    // A rvalue minting a fresh list (`Array.from`, a spread
                    // copy) copies in JavaScript too, so recovering into its
                    // result hides nothing.
                    // A whole erased value (`unknown`, `any`) read into a
                    // structure mentioning an own `T` (`x as T[]`, an object
                    // `as Matcher<T>`) is rebuilt by `SmeltFromUnknown`; only
                    // a local copy nothing hands on cannot tell.
                    if let Some(dest_ty) = local_ty(*dest)
                        && own_container(dest_ty)
                        && !matches!(value, Rvalue::Len { .. } | Rvalue::Closure { .. })
                        && !smelt_mir::rvalue_mints_fresh_list(value)
                        && handed_on.contains(dest)
                    {
                        let mut recovers_whole = false;
                        value.reads(|operand| {
                            recovers_whole |=
                                whole_local(operand).and_then(local_ty).is_some_and(|ty| {
                                    crate::generic_lifting::type_is_erased_whole(self.mir, ty)
                                });
                        });
                        if recovers_whole {
                            return Ok(false);
                        }
                    }
                    if !matches!(value, Rvalue::Len { .. } | Rvalue::Closure { .. })
                        && !smelt_mir::rvalue_mints_fresh_list(value)
                    {
                        let mut operands: Vec<Operand> = Vec::new();
                        value.reads(|operand| operands.push(operand.clone()));
                        if recovers_shared(&operands, Some(*dest)) {
                            return Ok(false);
                        }
                    }
                    if let Rvalue::Closure { id, .. } = value
                        && seen.insert(*id)
                        && let Some(closure) = self
                            .mir
                            .closures
                            .iter()
                            .find(|candidate| candidate.id == *id)
                    {
                        pending.push((&closure.locals, &closure.blocks, false));
                    }
                    let Some(dest_ty) = local_ty(*dest) else {
                        continue;
                    };
                    if matches!(value, Rvalue::Use(Operand::Const(Constant::None)))
                        && null_into_own_optional(dest_ty)
                    {
                        return Ok(false);
                    }
                    if let Rvalue::Use(operand) = value
                        && let Some(source) = operand_ty(operand)
                        && let Some(Type::Optional(inner)) = self.mir.types.get(source)
                        && *inner == dest_ty
                        && matches!(self.mir.types.get(dest_ty), Some(Type::TypeParam { .. }))
                        && mentions_own(dest_ty)
                    {
                        return Ok(false);
                    }
                    // A length or a closure capture reads the container
                    // itself, never converting it.
                    if matches!(value, Rvalue::Len { .. } | Rvalue::Closure { .. })
                        || !holds_carrier(dest_ty)
                        || mentions_own(dest_ty)
                    {
                        continue;
                    }
                    let mut copies = false;
                    value.reads(|operand| {
                        copies |= operand_ty(operand).is_some_and(own_container);
                    });
                    if copies {
                        return Ok(false);
                    }
                }
                match &block.terminator {
                    Some(Terminator::Return(Operand::Const(Constant::None)))
                        if top_level && null_into_own_optional(return_ty) =>
                    {
                        return Ok(false);
                    }
                    Some(Terminator::Return(operand))
                        if top_level
                            && holds_carrier(return_ty)
                            && !mentions_own(return_ty)
                            && operand_ty(operand).is_some_and(own_container) =>
                    {
                        return Ok(false);
                    }
                    // The reverse: an erased value returned at a return type
                    // that structures an own `T` is rebuilt for the caller, who
                    // no longer shares it with whatever else holds it.
                    Some(Terminator::Return(operand))
                        if top_level
                            && own_container(return_ty)
                            && operand_ty(operand).is_some_and(holds_carrier)
                            && !whole_local(operand).is_some_and(unshared) =>
                    {
                        return Ok(false);
                    }
                    Some(Terminator::Call {
                        callee: Callee::Static(callee),
                        args,
                        dest,
                        ..
                    }) => {
                        let Some(callee_fn) = usize::try_from(callee.0)
                            .ok()
                            .and_then(|index| self.mir.functions.get(index))
                        else {
                            continue;
                        };
                        // A generic callee is instantiated at this site unless
                        // the site demotes (a packed rest, an unpinned
                        // parameter), which passes erased values exactly as an
                        // erased callee takes them.
                        if self.callee_emits_generics(*callee)
                            && self
                                .static_call_monomorphization(callee_fn, args)?
                                .is_some()
                        {
                            continue;
                        }
                        let read_only = self.context.read_only_arguments(self.mir);
                        for (index, (arg, param)) in args.iter().zip(&callee_fn.params).enumerate()
                        {
                            let Some(param_ty) = usize::try_from(param.0)
                                .ok()
                                .and_then(|slot| callee_fn.locals.get(slot))
                                .map(|decl| decl.ty)
                            else {
                                continue;
                            };
                            let erases = holds_carrier(param_ty)
                                || callee_fn.type_params.iter().any(|type_param| {
                                    crate::generic_bindings::type_param_occurs(
                                        self.mir,
                                        param_ty,
                                        type_param.name,
                                    )
                                });
                            // A callee that only reads the argument (keeps no
                            // handle, never writes through it) cannot tell the
                            // list from its identity-keeping erased copy.
                            // The copy is unobservable when nothing else names
                            // the list, when the callee only reads it, or when
                            // the callee hands it back only as its result and
                            // that result goes nowhere that could keep or
                            // change it (`headToolkit(toArray(arr))`).
                            let unobservable = whole_local(arg).is_some_and(unshared)
                                || read_only.argument_is_read_only(*callee, index, args.len())
                                || (read_only.argument_only_returned_unmutated(
                                    *callee,
                                    index,
                                    args.len(),
                                ) && !handed_on.contains(dest));
                            if erases && operand_ty(arg).is_some_and(own_container) && !unobservable
                            {
                                return Ok(false);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(true)
    }
}

/// Whether a type renders as the erased carrier itself once its type
/// parameters erase: `unknown`, a type parameter, or an optional of either.
/// A value crosses such a slot whole, through `IntoSmeltUnknown`.
fn whole_erased_slot(mir: &Mir, ty: TypeId) -> bool {
    let payload = match mir.types.get(ty) {
        Some(Type::Optional(inner)) => *inner,
        _ => ty,
    };
    matches!(
        mir.types.get(payload),
        Some(Type::Unknown | Type::TypeParam { .. })
    )
}
