// `isinstance`, `is None`, and the flow narrowing they prove.
// Included into lowering.rs via include!, so this file uses plain comments.
//
// `isinstance(x, T)` is answered from `x`'s static type wherever that decides
// it (`isinstance(3, str)` is `False`), and at run time only for a value of a
// concrete union or an optional, by testing which member it holds
// (`ExprKind::UnionArmTest`). Inside a branch the test proves, a read of `x`
// is the narrowed member: `if isinstance(x, int): x + 1` on an `x: int | str`
// reads the `int` arm. A branch that always leaves (`return`, `raise`,
// `continue`, `break`) proves the other side for the rest of the block.
//
// A fact names a local, so it is only ever recorded for a local bound exactly
// once in its function (a parameter never reassigned, or a single
// assignment): no later store can make a narrowed read stale. Facts are
// scoped to the block that proved them and never reach a nested function.
//
// An `is None` test on a field read through such a local (`self.tree is None`)
// narrows that field the same way, unless the function stores to the field
// through that name anywhere (`assigned_attribute_paths`): the read is the
// field at its optional's payload, as `ty` narrows a member access. As in
// `ty`, a call between the test and the read is assumed not to reset the
// field; if one does, the payload read fails at run time where CPython's
// attribute access on `None` would raise.

/// What a narrowing fact is about: a local, or one field read through a
/// local (`self.tree`), the way `ty` narrows a member access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum NarrowKey {
    /// The local itself.
    Local(smelt_hir::LocalId),
    /// The field `Symbol` of the value the local holds.
    Field(smelt_hir::LocalId, Symbol),
}

/// What an `isinstance` test proves about one local in each branch.
#[derive(Debug, Default)]
struct NarrowingFacts {
    /// Facts holding where the test is true.
    then_facts: Vec<(NarrowKey, TypeId)>,
    /// Facts holding where the test is false.
    else_facts: Vec<(NarrowKey, TypeId)>,
}

/// One class or builtin type named by `isinstance`'s second argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstanceTarget {
    /// `int` (a `bool` is one too).
    Int,
    /// `float`.
    Float,
    /// `str`.
    Str,
    /// `bool`.
    Bool,
    /// `list`, whatever its items.
    List,
    /// `dict`, whatever its keys and values.
    Dict,
    /// `set`, whatever its items.
    Set,
    /// `tuple`, whatever its items.
    Tuple,
    /// A class of the program.
    Class(Symbol),
}

/// Whether a static type is an instance of a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstanceAnswer {
    /// Every value of the type is one.
    Always,
    /// No value of the type is one.
    Never,
    /// A value of a class whose subclass is the target: only its runtime
    /// class decides.
    Subclass(Symbol),
}

impl ModuleBuilder<'_> {
    /// The names `func`'s body binds exactly once, counting its parameters as
    /// bindings: the locals an `isinstance` narrowing may name. Every store
    /// form counts (assignment, augmented assignment, `for`/`with`/walrus
    /// targets, `del`, `except ... as`, imports, nested `def`/`class`, match
    /// captures), including those inside nested functions, so a `nonlocal`
    /// rebinding is never missed; a `global` or `nonlocal` name never
    /// qualifies.
    fn single_binding_names(params: &[&str], stmts: &[Stmt]) -> HashSet<String> {
        /// Counts every store of a name.
        #[derive(Default)]
        struct Stores<'a> {
            /// Stores per name.
            counts: HashMap<&'a str, usize>,
            /// Names declared `global` or `nonlocal`.
            declared: HashSet<&'a str>,
        }
        impl<'a> Stores<'a> {
            /// Count one store of `name`.
            fn store(&mut self, name: &'a str) {
                let count = self.counts.entry(name).or_default();
                *count = count.saturating_add(1);
            }
        }
        impl<'a> ruff_python_ast::visitor::Visitor<'a> for Stores<'a> {
            fn visit_stmt(&mut self, stmt: &'a Stmt) {
                match stmt {
                    Stmt::Global(global) => self.declared.extend(global.names.iter().map(|name| name.as_str())),
                    Stmt::Nonlocal(nonlocal) => self.declared.extend(nonlocal.names.iter().map(|name| name.as_str())),
                    Stmt::FunctionDef(function) => self.store(function.name.as_str()),
                    Stmt::ClassDef(class) => self.store(class.name.as_str()),
                    Stmt::Import(import) => {
                        for alias in &import.names {
                            let bound = alias.asname.as_ref().unwrap_or(&alias.name);
                            self.store(bound.as_str().split('.').next().unwrap_or_default());
                        }
                    }
                    Stmt::ImportFrom(import) => {
                        for alias in &import.names {
                            self.store(alias.asname.as_ref().unwrap_or(&alias.name).as_str());
                        }
                    }
                    Stmt::Try(try_stmt) => {
                        for handler in &try_stmt.handlers {
                            let ExceptHandler::ExceptHandler(except) = handler;
                            if let Some(name) = &except.name {
                                self.store(name.as_str());
                            }
                        }
                    }
                    _ => {}
                }
                ruff_python_ast::visitor::walk_stmt(self, stmt);
            }

            fn visit_expr(&mut self, expr: &'a Expr) {
                if let Expr::Name(name) = expr
                    && !matches!(name.ctx, ruff_python_ast::ExprContext::Load)
                {
                    self.store(name.id.as_str());
                }
                ruff_python_ast::visitor::walk_expr(self, expr);
            }

            fn visit_pattern(&mut self, pattern: &'a ruff_python_ast::Pattern) {
                match pattern {
                    ruff_python_ast::Pattern::MatchAs(capture) => {
                        if let Some(name) = &capture.name {
                            self.store(name.as_str());
                        }
                    }
                    ruff_python_ast::Pattern::MatchStar(star) => {
                        if let Some(name) = &star.name {
                            self.store(name.as_str());
                        }
                    }
                    ruff_python_ast::Pattern::MatchMapping(mapping) => {
                        if let Some(rest) = &mapping.rest {
                            self.store(rest.as_str());
                        }
                    }
                    _ => {}
                }
                ruff_python_ast::visitor::walk_pattern(self, pattern);
            }
        }
        let mut stores = Stores::default();
        for param in params {
            stores.store(param);
        }
        for stmt in stmts {
            ruff_python_ast::visitor::Visitor::visit_stmt(&mut stores, stmt);
        }
        stores
            .counts
            .into_iter()
            .filter(|(name, count)| *count == 1 && !stores.declared.contains(name))
            .map(|(name, _)| name.to_owned())
            .collect()
    }

    /// Lower `isinstance(value, T)` (`T` a builtin type, a class, or a tuple
    /// of them) to a `bool`; see the file header.
    fn isinstance_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
        span: Span,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let ([value_ast, target_ast], []) = (call.arguments.args.as_ref(), call.arguments.keywords.as_ref()) else {
            return Err(SmeltError::unsupported(span, "isinstance() takes a value and a type"));
        };
        let targets = self.instance_targets(target_ast, span)?;
        let value = self.expression(value_ast, body)?;
        let value_ty = Self::expr_ty(body, value);
        let bool_ty = self.intern_type(Type::Bool);
        let members = self.flattened_members(value_ty);
        self.record_unlowered_ancestry(&members, &targets);
        let mut passing = Vec::new();
        let mut subclass = None;
        for &member in &members {
            match self.instance_answer(member, &targets) {
                InstanceAnswer::Always => passing.push(member),
                InstanceAnswer::Never => {}
                InstanceAnswer::Subclass(class) => subclass = Some(class),
            }
        }
        if let Some(class) = subclass {
            // A class value whose runtime class may be the target's subclass.
            if members.len() != 1 {
                return Err(SmeltError::unsupported(
                    span,
                    "isinstance() of a union member whose subclass is the target is not supported",
                ));
            }
            return Ok(body.push_expr(HirExpr { kind: ExprKind::InstanceOf { value, class }, ty: bool_ty, span }));
        }
        let answer = if passing.len() == members.len() {
            Some(true)
        } else if passing.is_empty() {
            Some(false)
        } else {
            None
        };
        let Some(constant) = answer else {
            // A union with a `None` member is erased today (no concrete enum
            // holds it), so no member test can be typed on it.
            let none_member = matches!(self.ctx.krate.types.get(value_ty), Some(Type::Union(items))
                if items.iter().any(|item| matches!(self.ctx.krate.types.get(*item), Some(Type::None))));
            if none_member {
                return Err(SmeltError::unsupported(
                    span,
                    "isinstance() on a union with a `None` member is not supported (such a union is erased)",
                ));
            }
            return Ok(body.push_expr(HirExpr {
                kind: ExprKind::UnionArmTest { value, arms: passing },
                ty: bool_ty,
                span,
            }));
        };
        // The static type decides; the operand is still evaluated first.
        let block = body.push_block(span);
        body.push_stmt_to_block(block, HirStmt::Expr(value));
        let literal = body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Bool(constant)), ty: bool_ty, span });
        let block_idx = usize::try_from(block.0).map_err(|error| SmeltError::unsupported(span, error.to_string()))?;
        body.blocks[block_idx].tail = Some(literal);
        Ok(body.push_expr(HirExpr { kind: ExprKind::Block(block), ty: bool_ty, span }))
    }

    /// The types `isinstance`'s second argument names.
    fn instance_targets(&mut self, target: &Expr, span: Span) -> Result<Vec<InstanceTarget>, SmeltError> {
        if let Expr::Tuple(tuple) = target {
            let mut targets = Vec::new();
            for item in &tuple.elts {
                targets.extend(self.instance_targets(item, span)?);
            }
            return Ok(targets);
        }
        let Expr::Name(name) = target else {
            return Err(SmeltError::unsupported(span, "isinstance() needs a type name or a tuple of them"));
        };
        let spelled = name.id.as_str();
        let builtin_target = match spelled {
            "int" => Some(InstanceTarget::Int),
            "float" => Some(InstanceTarget::Float),
            "str" => Some(InstanceTarget::Str),
            "bool" => Some(InstanceTarget::Bool),
            "list" => Some(InstanceTarget::List),
            "dict" => Some(InstanceTarget::Dict),
            "set" => Some(InstanceTarget::Set),
            "tuple" => Some(InstanceTarget::Tuple),
            _ => None,
        };
        if let Some(builtin) = builtin_target
            && !self.program_binds_name(spelled)
        {
            return Ok(vec![builtin]);
        }
        if let Some(&item) = self.items.get(spelled)
            && let Item::Class(class) = self.item_ref(item)
        {
            return Ok(vec![InstanceTarget::Class(class.name)]);
        }
        // A class of this module not lowered yet: a function body reaches a
        // class the module defines further down (pass 1 lowers declarations
        // in source order). Python resolves the name when the call runs, by
        // which time the class exists; its symbol is the one the class's own
        // lowering interns.
        if !self.program_binds_name(spelled)
            && self.module_class_bases.iter().any(|(class, _)| class == spelled)
        {
            return Ok(vec![InstanceTarget::Class(self.intern_name(spelled))]);
        }
        Err(SmeltError::unsupported(span, format!("isinstance() of `{spelled}` is not supported")))
    }

    /// The member types a value of `ty` may hold: a union's members, an
    /// optional's `None` and its payload's members, else `ty` itself.
    fn flattened_members(&mut self, ty: TypeId) -> Vec<TypeId> {
        match self.ctx.krate.types.get(ty).cloned() {
            Some(Type::Union(members)) => members,
            Some(Type::Optional(inner, _)) => {
                let mut members = self.flattened_members(inner);
                members.push(self.intern_type(Type::None));
                members
            }
            _ => vec![ty],
        }
    }

    /// Whether every value of `member` is an instance of one of `targets`.
    fn instance_answer(&self, member: TypeId, targets: &[InstanceTarget]) -> InstanceAnswer {
        let mut answer = InstanceAnswer::Never;
        for target in targets {
            let one = match (self.ctx.krate.types.get(member), *target) {
                (Some(Type::PyInt | Type::Bool), InstanceTarget::Int)
                | (Some(Type::Float), InstanceTarget::Float)
                | (Some(Type::PyStr), InstanceTarget::Str)
                | (Some(Type::Bool), InstanceTarget::Bool)
                | (Some(Type::List(_)), InstanceTarget::List)
                | (Some(Type::Dict(..)), InstanceTarget::Dict)
                | (Some(Type::Set(_)), InstanceTarget::Set)
                | (Some(Type::Tuple(_)), InstanceTarget::Tuple) => InstanceAnswer::Always,
                (Some(&Type::Class { name, .. }), InstanceTarget::Class(class)) => {
                    if self.class_extends(name, class) {
                        InstanceAnswer::Always
                    } else if self.class_extends(class, name) {
                        InstanceAnswer::Subclass(class)
                    } else {
                        InstanceAnswer::Never
                    }
                }
                _ => InstanceAnswer::Never,
            };
            match one {
                InstanceAnswer::Always => return InstanceAnswer::Always,
                InstanceAnswer::Subclass(_) => answer = one,
                InstanceAnswer::Never => {}
            }
        }
        answer
    }

    /// Whether class `sub` is `sup` or inherits from it.
    fn class_extends(&self, sub: Symbol, sup: Symbol) -> bool {
        let mut current = Some(sub);
        let mut steps = 0_usize;
        while let Some(class) = current {
            if class == sup {
                return true;
            }
            steps = steps.saturating_add(1);
            if steps > self.ctx.krate.items.len() {
                return false;
            }
            current = self.class_base(class);
        }
        false
    }

    /// The base of class `class`: the lowered class's, else the one
    /// `record_unlowered_ancestry` read from the definition of a class of
    /// this module not lowered yet.
    fn class_base(&self, class: Symbol) -> Option<Symbol> {
        let mut lowered = false;
        let base = self.ctx.krate.items.iter().find_map(|item| match item {
            Item::Class(candidate) if candidate.name == class => {
                lowered = true;
                candidate.base
            }
            _ => None,
        });
        if lowered { base } else { self.unlowered_class_bases.get(&class).copied() }
    }

    /// Record the ancestry of every class `isinstance` compares (each member
    /// of the tested value's type and each target) that this module defines
    /// further down: pass 1 lowers declarations in source order, so a function
    /// body may test against a class whose HIR item does not exist yet. Its
    /// single base, when a class of this module, is read from the definition.
    fn record_unlowered_ancestry(&mut self, members: &[TypeId], targets: &[InstanceTarget]) {
        let mut pending: Vec<Symbol> = targets
            .iter()
            .filter_map(|target| match target {
                InstanceTarget::Class(class) => Some(*class),
                _ => None,
            })
            .collect();
        pending.extend(members.iter().filter_map(|member| match self.ctx.krate.types.get(*member) {
            Some(&Type::Class { name, .. }) => Some(name),
            _ => None,
        }));
        let mut steps = 0_usize;
        while let Some(class) = pending.pop() {
            steps = steps.saturating_add(1);
            if steps > self.module_class_bases.len().saturating_add(targets.len()).saturating_add(members.len()) {
                return;
            }
            let lowered = self.ctx.krate.items.iter().any(|item| matches!(item, Item::Class(candidate) if candidate.name == class));
            if lowered || self.unlowered_class_bases.contains_key(&class) {
                continue;
            }
            let Some(name) = self.ctx.krate.symbols.get(class).map(str::to_owned) else {
                continue;
            };
            let single_base = match self.module_class_bases.iter().find(|(candidate, _)| *candidate == name) {
                Some((_, bases)) if bases.len() == 1 => bases.first().cloned(),
                _ => None,
            };
            let Some(base) = single_base.filter(|candidate_base| self.module_class_bases.iter().any(|(candidate, _)| candidate == candidate_base)) else {
                continue;
            };
            let base_symbol = self.intern_name(&base);
            self.unlowered_class_bases.insert(class, base_symbol);
            pending.push(base_symbol);
        }
    }

    /// What `test`, an `if` condition, proves about single-binding locals.
    fn narrowing_facts(&mut self, test: &Expr, body: &Body) -> NarrowingFacts {
        match test {
            Expr::UnaryOp(unary) if unary.op == ruff_python_ast::UnaryOp::Not => {
                let inner = self.narrowing_facts(&unary.operand, body);
                NarrowingFacts { then_facts: inner.else_facts, else_facts: inner.then_facts }
            }
            Expr::BoolOp(bool_op) if bool_op.op == BoolOp::And => {
                let mut facts = NarrowingFacts::default();
                for value in &bool_op.values {
                    facts.then_facts.extend(self.narrowing_facts(value, body).then_facts);
                }
                facts
            }
            Expr::Call(call) => self.isinstance_facts(call, body).unwrap_or_default(),
            Expr::Compare(compare) => self.none_test_facts(compare, body).unwrap_or_default(),
            _ => NarrowingFacts::default(),
        }
    }

    /// The facts of `name is None` / `name is not None` (either operand
    /// order) on a single-binding local of an optional type: where the local
    /// is not `None` it holds the optional's payload. The `None` side records
    /// nothing, since a read there has no payload to project.
    fn none_test_facts(&mut self, compare: &ruff_python_ast::ExprCompare, body: &Body) -> Option<NarrowingFacts> {
        let ([op], [right]) = (compare.ops.as_ref(), compare.comparators()) else {
            return None;
        };
        let ((tested, Expr::NoneLiteral(_)) | (Expr::NoneLiteral(_), tested)) = (compare.first_operand(), right) else {
            return None;
        };
        let (key, tested_ty) = match tested {
            Expr::Name(name) => {
                if !self.scope.single_binding.contains(name.id.as_str()) {
                    return None;
                }
                let local = *self.scope.locals.get(name.id.as_str())?;
                (NarrowKey::Local(local), Self::local_ty(body, local))
            }
            Expr::Attribute(attr) => {
                let Expr::Name(name) = attr.value.as_ref() else {
                    return None;
                };
                if !self.scope.single_binding.contains(name.id.as_str())
                    || self.scope.assigned_attributes.contains(&(name.id.to_string(), attr.attr.to_string()))
                {
                    return None;
                }
                let local = *self.scope.locals.get(name.id.as_str())?;
                let field = self.intern_name(attr.attr.as_str());
                let receiver_ty = self.readable_narrowing(local, body).unwrap_or_else(|| Self::local_ty(body, local));
                (NarrowKey::Field(local, field), self.field_type(receiver_ty, field).ok()?)
            }
            _ => return None,
        };
        let &Type::Optional(inner, _) = self.ctx.krate.types.get(tested_ty)? else {
            return None;
        };
        let present = vec![(key, inner)];
        match op {
            CmpOp::Is => Some(NarrowingFacts { then_facts: Vec::new(), else_facts: present }),
            CmpOp::IsNot => Some(NarrowingFacts { then_facts: present, else_facts: Vec::new() }),
            _ => None,
        }
    }

    /// The facts of `isinstance(name, T)` on a single-binding local of a
    /// union or optional type.
    fn isinstance_facts(&mut self, call: &ruff_python_ast::ExprCall, body: &Body) -> Option<NarrowingFacts> {
        let Expr::Name(callee) = call.func.as_ref() else {
            return None;
        };
        if callee.id.as_str() != "isinstance" || self.program_binds_name("isinstance") {
            return None;
        }
        let ([Expr::Name(name), target_ast], []) = (call.arguments.args.as_ref(), call.arguments.keywords.as_ref()) else {
            return None;
        };
        if !self.scope.single_binding.contains(name.id.as_str()) {
            return None;
        }
        let local = *self.scope.locals.get(name.id.as_str())?;
        let base_ty = Self::local_ty(body, local);
        // A value of a class whose subclass the test names is that subclass
        // where the test holds (`isinstance(shape, Box)` on a `shape: Shape`):
        // a read there is the checked downcast, so the value can flow where a
        // `Box` is expected, as `ty` narrows it.
        if let Some(Type::Class { .. }) = self.ctx.krate.types.get(base_ty) {
            let targets = self.instance_targets(target_ast, self.span(call.range())).ok()?;
            self.record_unlowered_ancestry(&[base_ty], &targets);
            let InstanceAnswer::Subclass(class) = self.instance_answer(base_ty, &targets) else {
                return None;
            };
            if targets.len() != 1 {
                return None;
            }
            let narrowed = self.intern_type(Type::Class { name: class, args: Vec::new() });
            return Some(NarrowingFacts { then_facts: vec![(NarrowKey::Local(local), narrowed)], else_facts: Vec::new() });
        }
        match self.ctx.krate.types.get(base_ty) {
            Some(Type::Optional(..)) => {}
            Some(Type::Union(items))
                if !items.iter().any(|item| matches!(self.ctx.krate.types.get(*item), Some(Type::None))) => {}
            _ => return None,
        }
        let targets = self.instance_targets(target_ast, self.span(call.range())).ok()?;
        // An earlier test may already have ruled members out (`elif`).
        let known_ty = self.scope.narrowed.get(&NarrowKey::Local(local)).copied().unwrap_or(base_ty);
        let members = self.flattened_members(known_ty);
        self.record_unlowered_ancestry(&members, &targets);
        let (passing, failing): (Vec<TypeId>, Vec<TypeId>) =
            members.into_iter().partition(|&member| self.instance_answer(member, &targets) == InstanceAnswer::Always);
        let mut facts = NarrowingFacts::default();
        if let Some(ty) = self.narrowed_member_type(base_ty, &passing) {
            facts.then_facts.push((NarrowKey::Local(local), ty));
        }
        if let Some(ty) = self.narrowed_member_type(base_ty, &failing) {
            facts.else_facts.push((NarrowKey::Local(local), ty));
        }
        Some(facts)
    }

    /// The type a local of `base_ty` is known to have when it holds one of
    /// `members`: a single member, an optional's payload when `members` are
    /// all its present members, else the union of `members`. That last fact
    /// only feeds later tests (`elif`); a read narrows to a single member or a
    /// payload only (see `identifier_expression`). `None` when nothing is
    /// ruled out.
    fn narrowed_member_type(&mut self, base_ty: TypeId, members: &[TypeId]) -> Option<TypeId> {
        if let [only] = members {
            return (*only != base_ty).then_some(*only);
        }
        if let Some(&Type::Optional(inner, _)) = self.ctx.krate.types.get(base_ty) {
            let present = self.flattened_members(inner);
            if members.len() == present.len() && present.iter().all(|member| members.contains(member)) {
                return Some(inner);
            }
        }
        if members.is_empty() || members.len() == self.flattened_members(base_ty).len() {
            return None;
        }
        Some(self.intern_type(Type::Union(members.to_vec())))
    }

    /// The narrowed type a read of `local` takes, if any: a fact that is
    /// still a union of several members leaves the read at its own type,
    /// unless that union is the whole payload of the local's optional.
    fn readable_narrowing(&self, local: smelt_hir::LocalId, body: &Body) -> Option<TypeId> {
        let narrowed = *self.scope.narrowed.get(&NarrowKey::Local(local))?;
        let payload = matches!(self.ctx.krate.types.get(Self::local_ty(body, local)), Some(&Type::Optional(inner, _)) if inner == narrowed);
        (payload || !matches!(self.ctx.krate.types.get(narrowed), Some(Type::Union(_)))).then_some(narrowed)
    }

    /// A read of a narrowed local: the local at its optional's payload type
    /// (the narrowing unwrap), then the union member projected out of it.
    fn narrowed_local_read(&self, local: smelt_hir::LocalId, narrowed: TypeId, span: Span, body: &mut Body) -> smelt_hir::ExprId {
        let mut base_ty = Self::local_ty(body, local);
        if let Some(&Type::Optional(inner, _)) = self.ctx.krate.types.get(base_ty) {
            base_ty = inner;
        }
        let read = body.push_expr(HirExpr { kind: ExprKind::Local(local), ty: base_ty, span });
        if base_ty == narrowed {
            return read;
        }
        body.push_expr(HirExpr { kind: ExprKind::UnknownCast { value: read, target: narrowed }, ty: narrowed, span })
    }

    /// A read of `receiver.field` whose field an enclosing `is None` test
    /// narrowed: the field read at its optional type, then its payload.
    fn narrowed_field_read(&mut self, attr: &ruff_python_ast::ExprAttribute, body: &mut Body) -> Option<smelt_hir::ExprId> {
        let Expr::Name(name) = attr.value.as_ref() else {
            return None;
        };
        let local = *self.scope.locals.get(name.id.as_str())?;
        let field = self.intern_name(attr.attr.as_str());
        let narrowed = *self.scope.narrowed.get(&NarrowKey::Field(local, field))?;
        let span = self.span(attr.range);
        let receiver_ty = self.readable_narrowing(local, body).unwrap_or_else(|| Self::local_ty(body, local));
        let receiver = match self.readable_narrowing(local, body) {
            Some(ty) => self.narrowed_local_read(local, ty, span, body),
            None => body.push_expr(HirExpr { kind: ExprKind::Local(local), ty: receiver_ty, span }),
        };
        let field_ty = self.field_type(receiver_ty, field).ok()?;
        let read = body.push_expr(HirExpr { kind: ExprKind::Field { receiver, field }, ty: field_ty, span });
        Some(body.push_expr(HirExpr { kind: ExprKind::UnknownCast { value: read, target: narrowed }, ty: narrowed, span }))
    }

    /// `(receiver, attribute)` of every attribute store `stmts` make through
    /// a name, nested blocks included (nested functions and classes aside):
    /// `x.a = v`, `x.a += v`, `x.a: T = v`, `del x.a`, and a `for`/`with`
    /// target.
    fn assigned_attribute_paths(stmts: &[Stmt]) -> HashSet<(String, String)> {
        /// Collects attribute store targets.
        struct Stores(HashSet<(String, String)>);
        impl<'a> ruff_python_ast::visitor::Visitor<'a> for Stores {
            fn visit_stmt(&mut self, stmt: &'a Stmt) {
                if matches!(stmt, Stmt::FunctionDef(_) | Stmt::ClassDef(_)) {
                    return;
                }
                ruff_python_ast::visitor::walk_stmt(self, stmt);
            }
            fn visit_expr(&mut self, expr: &'a Expr) {
                if let Expr::Attribute(attr) = expr
                    && !matches!(attr.ctx, ruff_python_ast::ExprContext::Load)
                    && let Expr::Name(name) = attr.value.as_ref()
                {
                    self.0.insert((name.id.to_string(), attr.attr.to_string()));
                }
                ruff_python_ast::visitor::walk_expr(self, expr);
            }
        }
        let mut stores = Stores(HashSet::new());
        ruff_python_ast::visitor::Visitor::visit_body(&mut stores, stmts);
        stores.0
    }

    /// Whether a block that ends with `stmts` never falls through to the
    /// statement after it.
    fn always_leaves(stmts: &[Stmt]) -> bool {
        matches!(stmts.last(), Some(Stmt::Return(_) | Stmt::Raise(_) | Stmt::Continue(_) | Stmt::Break(_)))
    }

    /// Run `lower` with `facts` added to the narrowings, restoring the
    /// enclosing narrowings afterwards.
    fn with_narrowings<T>(&mut self, facts: &[(NarrowKey, TypeId)], lower: impl FnOnce(&mut Self) -> T) -> T {
        let enclosing = self.scope.narrowed.clone();
        self.scope.narrowed.extend(facts.iter().copied());
        let result = lower(self);
        self.scope.narrowed = enclosing;
        result
    }
}
