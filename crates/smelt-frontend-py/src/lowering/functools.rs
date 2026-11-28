// `functools`: `reduce`, `partial`, and the `lru_cache` / `cache` decorators.
// Included into lowering.rs via include!, so this file uses plain comments.
//
// Each API is recognized by what its name denotes through the module's
// imports (`functools.reduce`, `from functools import reduce as fold`), never
// by spelling, and lowers to ordinary typed HIR:
//
// * `reduce(f, xs[, init])` is a typed fold (`ExprKind::ListReduce`) whose
//   callback is typed by the accumulator and the element type.
// * `partial(f, *args, **kwargs)` evaluates `f`'s bound arguments once, where
//   the partial is created, and answers a typed closure over the parameters
//   left unbound; a partial bound to a name also keeps its call shape, so a
//   call through the name may still pass keywords and override a bound one.
// * `@lru_cache` / `@cache` on a module-level function is rewritten, before
//   lowering, into the undecorated function, a module-global typed `dict`
//   keyed by the argument values, and a wrapper with the function's own name
//   and signature that consults the dict (`memoized_module`). A recursive
//   call names the wrapper, as it does in Python, so recursion is memoized.
//
// `functools.wraps` needs nothing here: a decorator's wrapper chain is
// materialized from the build-time run (`lowering/specialization.rs`), which
// already maps each `@wraps(f)` layer back to its source function.

/// The fully qualified name of `functools.partial`.
const FUNCTOOLS_PARTIAL: &str = "functools.partial";
/// The fully qualified name of `functools.reduce`.
const FUNCTOOLS_REDUCE: &str = "functools.reduce";
/// CPython's `lru_cache` default `maxsize`, used by a bare `@lru_cache` and
/// by `@lru_cache()`.
const LRU_CACHE_DEFAULT_MAXSIZE: u64 = 128;

/// How many entries a memoizing decorator keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemoBound {
    /// `@cache`, `@lru_cache(maxsize=None)`: every result is kept.
    Unbounded,
    /// `@lru_cache(maxsize=n)` with `n > 0`: least recently used eviction.
    Lru(u64),
    /// `maxsize <= 0`: CPython calls straight through and caches nothing.
    Disabled,
}

/// A `name = functools.partial(func, *args, **kwargs)` bound to a name of the
/// scope that binds it exactly once.
///
/// Python evaluates `func` and every bound argument once, when `partial` is
/// called; each lives in a hidden local of the scope (named with the
/// `__smelt_partial` prefix no Python source uses), so a call through the
/// name re-reads the stored values, never the source expressions.
#[derive(Debug, Clone)]
struct PartialBinding {
    /// The name the partial calls: a module function, another partial
    /// binding, or the hidden local holding a callable value.
    callee: String,
    /// The hidden locals holding the bound positional arguments, in order.
    positional: Vec<String>,
    /// Each bound keyword with the hidden local holding its value.
    keywords: Vec<(String, String)>,
    /// The local holding the partial as a closure value, which a read of the
    /// name as a value (a callback, a nested function) sees. `None` when the
    /// remaining parameters cannot be passed positionally: then the name can
    /// only be called.
    value: Option<smelt_hir::LocalId>,
}

/// One parameter a partial leaves for its callers.
#[derive(Debug, Clone)]
struct PartialParam {
    /// The parameter's source name, when the callee is a `def`.
    name: Option<String>,
    /// The type a caller passes.
    ty: TypeId,
    /// Whether a caller must pass it (no default).
    required: bool,
}

/// What a partial calls, as far as its value closure's signature is concerned.
#[derive(Debug, Clone)]
enum PartialTarget {
    /// A module function, with each parameter's name, type and requiredness.
    Function {
        /// The function's parameters.
        params: Vec<PartialParam>,
        /// The function's return type.
        return_ty: TypeId,
        /// Whether the function is generic, whose solution depends on each call.
        generic: bool,
    },
    /// A callable value (a closure, another partial's closure): positional only.
    Value(FunctionType),
}

/// Relocates every statement and expression of a synthesized fragment to one
/// source range, so its diagnostics point at the code it stands for and no
/// offset of the template text is mistaken for a real source position.
struct RelocateFragment(TextRange);

impl ruff_python_ast::visitor::transformer::Transformer for RelocateFragment {
    fn visit_stmt(&self, stmt: &mut Stmt) {
        match stmt {
            Stmt::Assign(node) => node.range = self.0,
            Stmt::AnnAssign(node) => node.range = self.0,
            Stmt::If(node) => {
                node.range = self.0;
                for clause in &mut node.elif_else_clauses {
                    clause.range = self.0;
                }
            }
            Stmt::Return(node) => node.range = self.0,
            Stmt::Delete(node) => node.range = self.0,
            Stmt::Raise(node) => node.range = self.0,
            Stmt::Expr(node) => node.range = self.0,
            _ => {}
        }
        ruff_python_ast::visitor::transformer::walk_stmt(self, stmt);
    }

    fn visit_expr(&self, expr: &mut Expr) {
        ruff_python_ast::relocate::relocate_expr(expr, self.0);
    }
}

/// Replaces each `Name` a fragment spells as a placeholder with an expression.
struct SubstituteNames<'a>(&'a HashMap<String, Expr>);

impl ruff_python_ast::visitor::transformer::Transformer for SubstituteNames<'_> {
    fn visit_expr(&self, expr: &mut Expr) {
        if let Expr::Name(name) = expr
            && let Some(replacement) = self.0.get(name.id.as_str())
        {
            *expr = replacement.clone();
            return;
        }
        ruff_python_ast::visitor::transformer::walk_expr(self, expr);
    }
}

/// Parse a synthesized statement fragment and place it at `range`.
fn synthesized_statements(source: &str, range: TextRange) -> Vec<Stmt> {
    use ruff_python_ast::visitor::transformer::Transformer as _;
    // The templates are fixed text plus source identifiers, which always parse.
    let Ok(parsed) = ruff_python_parser::parse_module(source) else {
        return Vec::new();
    };
    let mut stmts: Vec<Stmt> = parsed.into_syntax().body.into_iter().collect();
    let relocate = RelocateFragment(range);
    for stmt in &mut stmts {
        relocate.visit_stmt(stmt);
    }
    stmts
}

/// Parse a synthesized expression and place it at `range`.
fn synthesized_expression(source: &str, range: TextRange) -> Option<Expr> {
    let parsed = ruff_python_parser::parse_expression(source).ok()?;
    let mut expr = *parsed.into_syntax().body;
    ruff_python_ast::relocate::relocate_expr(&mut expr, range);
    Some(expr)
}

/// A bare `Name` expression at `range`.
fn name_expr(id: &str, range: TextRange) -> Expr {
    Expr::Name(ruff_python_ast::ExprName {
        node_index: ruff_python_ast::AtomicNodeIndex::default(),
        range,
        id: ruff_python_ast::name::Name::new(id),
        ctx: ruff_python_ast::ExprContext::Load,
    })
}

impl ModuleBuilder<'_> {
    // -----------------------------------------------------------------------
    // `@lru_cache` / `@cache`
    // -----------------------------------------------------------------------

    /// The module with every memoized module-level function rewritten (see
    /// [`Self::memoized_function`]), or `None` when no function is memoized.
    ///
    /// The rewrite runs before any lowering, so the memoized function is
    /// declared, specialized and lowered like any undecorated `def`: the
    /// build-time materialization never sees the decorator it replaces.
    fn memoized_module(&self, module: &ModModule) -> Result<Option<ModModule>, Vec<SmeltError>> {
        let mut stmts: Vec<Stmt> = Vec::with_capacity(module.body.len());
        let mut rewritten = false;
        let mut errors = Vec::new();
        for stmt in &module.body {
            let Stmt::FunctionDef(func) = stmt else {
                stmts.push(stmt.clone());
                continue;
            };
            match self.memo_decorator(func).and_then(|bound| {
                bound.map(|bound| self.memoized_function(func, bound)).transpose()
            }) {
                Ok(Some(replacement)) => {
                    stmts.extend(replacement);
                    rewritten = true;
                }
                Ok(None) => stmts.push(stmt.clone()),
                Err(error) => errors.push(error),
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        if !rewritten {
            return Ok(None);
        }
        let mut module = module.clone();
        module.body = stmts.into_iter().collect();
        Ok(Some(module))
    }

    /// The memoizing decorator on a module-level function, if it has one.
    ///
    /// A decorator matches when every binding reaching it denotes
    /// `functools.lru_cache` or `functools.cache` (bare, called, dotted or
    /// aliased alike). Memoization composes with no other decorator here, so
    /// one stacked with another is refused, as is any `maxsize` that is not a
    /// literal integer or `None`.
    fn memo_decorator(&self, func: &StmtFunctionDef) -> Result<Option<MemoBound>, SmeltError> {
        let scope = EvalScope::Module { at: func.range.start() };
        let mut found = None;
        for decorator in &func.decorator_list {
            let (callee, call) = match &decorator.expression {
                Expr::Call(call) => (call.func.as_ref(), Some(call)),
                other => (other, None),
            };
            let Some(resolution) = self.bindings.resolve(callee, scope) else {
                continue;
            };
            let qualified = resolution.qualified_name();
            let is_lru = qualified == Some("functools.lru_cache");
            let is_cache = qualified == Some("functools.cache");
            if !is_lru && !is_cache {
                let names_memo = resolution.describe().contains("'functools.lru_cache'")
                    || resolution.describe().contains("'functools.cache'");
                if names_memo {
                    return Err(SmeltError::ambiguous_binding(
                        self.span(decorator.range),
                        func.name.as_str(),
                        &dotted_name(callee).unwrap_or_default(),
                        &resolution.describe(),
                    ));
                }
                continue;
            }
            let span = self.span(decorator.range);
            let bound = match (is_cache, call) {
                (true, None) => MemoBound::Unbounded,
                (true, Some(_)) => {
                    return Err(SmeltError::unsupported(
                        span,
                        "`functools.cache` takes the function itself; write `@cache` without a call",
                    ));
                }
                (false, None) => MemoBound::Lru(LRU_CACHE_DEFAULT_MAXSIZE),
                (false, Some(call)) => Self::lru_cache_bound(call, span)?,
            };
            if found.is_some() {
                return Err(SmeltError::unsupported(span, "a function memoized twice is not supported"));
            }
            found = Some(bound);
        }
        if found.is_some() && func.decorator_list.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(func.range),
                format!(
                    "`{}`: `lru_cache`/`cache` stacked with another decorator is not supported; memoize an undecorated function",
                    func.name
                ),
            ));
        }
        Ok(found)
    }

    /// The bound an `@lru_cache(...)` call states: `maxsize` positionally or
    /// by keyword (a literal integer or `None`), and `typed` as a literal
    /// bool. Parameters are statically typed, so `typed=True` (which keeps
    /// `1` and `1.0` apart) changes nothing a parameter can receive.
    fn lru_cache_bound(call: &ruff_python_ast::ExprCall, span: Span) -> Result<MemoBound, SmeltError> {
        let mut maxsize: Option<&Expr> = None;
        match call.arguments.args.as_ref() {
            [] => {}
            [only] => maxsize = Some(only),
            _ => return Err(SmeltError::unsupported(span, "`lru_cache` takes at most one positional argument")),
        }
        for keyword in &call.arguments.keywords {
            match keyword.arg.as_ref().map(ruff_python_ast::Identifier::as_str) {
                Some("maxsize") if maxsize.is_none() => maxsize = Some(&keyword.value),
                Some("typed") if matches!(keyword.value, Expr::BooleanLiteral(_)) => {}
                _ => {
                    return Err(SmeltError::unsupported(
                        span,
                        "`lru_cache` supports `maxsize=` (an integer literal or None) and `typed=` (a bool literal)",
                    ));
                }
            }
        }
        let Some(maxsize) = maxsize else {
            return Ok(MemoBound::Lru(LRU_CACHE_DEFAULT_MAXSIZE));
        };
        match maxsize {
            Expr::NoneLiteral(_) => Ok(MemoBound::Unbounded),
            Expr::NumberLiteral(number) => match &number.value {
                Number::Int(int) => Ok(int.as_u64().map_or(MemoBound::Disabled, |size| {
                    if size == 0 { MemoBound::Disabled } else { MemoBound::Lru(size) }
                })),
                _ => Err(SmeltError::unsupported(span, "`lru_cache` maxsize must be an integer or None")),
            },
            Expr::UnaryOp(unary) if matches!(unary.op, RuffUnaryOp::USub) && matches!(unary.operand.as_ref(), Expr::NumberLiteral(_)) => {
                        // A negative size is treated as 0: no caching.
                Ok(MemoBound::Disabled)
            }
            _ => Err(SmeltError::unsupported(
                span,
                "`lru_cache` maxsize must be an integer literal or None",
            )),
        }
    }

    /// Rewrite one memoized module-level function into the statements that
    /// implement it:
    ///
    /// ```text
    /// def __smelt_uncached_f(a: A, b: B) -> R: <f's body>
    /// __smelt_lru_cache_f: dict[tuple[A, B], R] = {}
    /// def f(a: A, b: B = default) -> R:
    ///     __smelt_key = (a, b)
    ///     if __smelt_key in __smelt_lru_cache_f:
    ///         return __smelt_lru_cache_f[__smelt_key]
    ///     __smelt_value = __smelt_uncached_f(a, b)
    ///     __smelt_lru_cache_f[__smelt_key] = __smelt_value
    ///     return __smelt_value
    /// ```
    ///
    /// Behaviour: the body runs once per distinct argument tuple (until it
    /// is evicted), exactly when CPython's `lru_cache` runs it. A recursive
    /// call in the body names `f`, the wrapper, so it is memoized too; a
    /// key a recursive call inserted while the body ran is overwritten with
    /// the equal value the unbounded cache computes, and kept as is by the
    /// bounded one (CPython's `bounded_lru_cache_wrapper` does the same).
    /// With a bound the dict's insertion order is the recency order: a hit
    /// moves its entry to the end, and a miss on a full cache evicts the
    /// first, least recently used, entry before inserting.
    ///
    /// Types: the cache is `dict[K, R]` with `K` the parameter type for one
    /// parameter, the tuple of them for several, and `bool` (one key, `True`)
    /// for none, and `R` the declared return type, so every lookup is typed.
    /// Parameters must key that dict by value: `int`, `str`, `bool` and
    /// tuples of them (`annotation_is_value_hashable`).
    fn memoized_function(&self, func: &StmtFunctionDef, bound: MemoBound) -> Result<Vec<Stmt>, SmeltError> {
        let name = func.name.as_str();
        let span = self.span(func.range);
        let refuse = |why: &str| Err(SmeltError::unsupported(span, format!("`{name}` cannot be memoized: {why}")));
        let mut undecorated = func.clone();
        undecorated.decorator_list = Vec::new().into_iter().collect();
        if bound == MemoBound::Disabled {
            // `maxsize=0` caches nothing: the function is called as written.
            return Ok(vec![Stmt::FunctionDef(undecorated)]);
        }
        if func.is_async {
            return refuse("an `async def` caches its coroutine, which can be awaited once");
        }
        let params = &func.parameters;
        if params.vararg.is_some() || params.kwarg.is_some() {
            return refuse("`*args` / `**kwargs` parameters are not supported");
        }
        let Some(returns) = func.returns.as_deref() else {
            return refuse("it needs a return annotation, the type of its cached values");
        };
        let positional: Vec<&ruff_python_ast::ParameterWithDefault> =
            params.posonlyargs.iter().chain(&params.args).collect();
        let all: Vec<&ruff_python_ast::ParameterWithDefault> =
            positional.iter().copied().chain(&params.kwonlyargs).collect();
        let scope = EvalScope::Module { at: func.range.start() };
        for param in &all {
            let Some(annotation) = param.parameter.annotation.as_deref() else {
                return refuse(&format!("parameter `{}` needs an annotation, the type of its cache key", param.parameter.name));
            };
            if !self.annotation_is_value_hashable(annotation, scope) {
                return refuse(&format!(
                    "parameter `{}` cannot key the cache (int, str, bool and tuples of them can)",
                    param.parameter.name
                ));
            }
        }

        let range = TextRange::empty(func.range.start());
        let uncached = format!("__smelt_uncached_{name}");
        let cache = format!("__smelt_lru_cache_{name}");

        // The body under a hidden name. Its own defaults are never used (the
        // wrapper always passes every argument), so it declares none.
        let mut inner = undecorated.clone();
        inner.name = ruff_python_ast::Identifier::new(uncached.as_str(), func.name.range);
        for param in inner.parameters.posonlyargs.iter_mut().chain(inner.parameters.args.iter_mut()).chain(inner.parameters.kwonlyargs.iter_mut()) {
            param.default = None;
        }

        // `dict[K, R]`, with placeholders for each annotation.
        let key_ty = match all.len() {
            0 => "bool".to_owned(),
            1 => "__smelt_key_ty_0".to_owned(),
            count => format!(
                "tuple[{}]",
                (0..count).map(|index| format!("__smelt_key_ty_{index}")).collect::<Vec<_>>().join(", ")
            ),
        };
        let mut placeholders: HashMap<String, Expr> = HashMap::new();
        for (index, param) in all.iter().enumerate() {
            if let Some(annotation) = param.parameter.annotation.as_deref() {
                placeholders.insert(format!("__smelt_key_ty_{index}"), annotation.clone());
            }
        }
        placeholders.insert("__smelt_return_ty".to_owned(), returns.clone());
        let mut cache_decl = synthesized_statements(&format!("{cache}: dict[{key_ty}, __smelt_return_ty] = {{}}\n"), range);
        {
            use ruff_python_ast::visitor::transformer::Transformer as _;
            let substitute = SubstituteNames(&placeholders);
            for stmt in &mut cache_decl {
                substitute.visit_stmt(stmt);
            }
        }

        let names: Vec<&str> = all.iter().map(|param| param.parameter.name.as_str()).collect();
        let key = match names.as_slice() {
            [] => "True".to_owned(),
            [only] => (*only).to_owned(),
            several => format!("({})", several.join(", ")),
        };
        let call_args = positional
            .iter()
            .map(|param| param.parameter.name.as_str().to_owned())
            .chain(params.kwonlyargs.iter().map(|param| format!("{0}={0}", param.parameter.name)))
            .collect::<Vec<_>>()
            .join(", ");
        let wrapper_body = match bound {
            MemoBound::Unbounded => format!(
                "__smelt_key = {key}\n\
                 if __smelt_key in {cache}:\n    return {cache}[__smelt_key]\n\
                 __smelt_value = {uncached}({call_args})\n\
                 {cache}[__smelt_key] = __smelt_value\n\
                 return __smelt_value\n"
            ),
            MemoBound::Lru(maxsize) => format!(
                "__smelt_key = {key}\n\
                 if __smelt_key in {cache}:\n    __smelt_value = {cache}.pop(__smelt_key)\n    {cache}[__smelt_key] = __smelt_value\n    return __smelt_value\n\
                 __smelt_value = {uncached}({call_args})\n\
                 if __smelt_key not in {cache}:\n    if len({cache}) >= {maxsize}:\n        del {cache}[list({cache})[0]]\n    {cache}[__smelt_key] = __smelt_value\n\
                 return __smelt_value\n"
            ),
            MemoBound::Disabled => String::new(),
        };
        let mut wrapper = undecorated;
        wrapper.body = synthesized_statements(&wrapper_body, range).into_iter().collect();
        Ok(vec![Stmt::FunctionDef(inner), cache_decl.into_iter().next().ok_or_else(|| {
            SmeltError::unsupported(span, "internal error: the cache declaration did not parse")
        })?, Stmt::FunctionDef(wrapper)])
    }

    /// Whether an annotation names a type whose values hash and compare by
    /// value and key the shared, insertion-ordered dict a cache must be (a
    /// module global every call mutates in place): `int`, `str`, `bool`, and
    /// tuples of up to six of them (`dict_key_uses_smelt_record` in
    /// `smelt-codegen-rust`). `float` keys need NaN's identity rules and
    /// optional keys a `None` member, neither of which that dict models, so
    /// they are refused rather than cached wrongly.
    ///
    /// Names are resolved by binding at the `def` (`EvalScope::Module`), so a
    /// program class that happens to be called `str` is never mistaken for
    /// the builtin, and `typing.Tuple` is recognized under any spelling.
    fn annotation_is_value_hashable(&self, annotation: &Expr, scope: EvalScope) -> bool {
        let denotes = |expr: &Expr, qualified: &[&str]| {
            self.bindings
                .resolve(expr, scope)
                .and_then(|resolution| resolution.qualified_name().map(str::to_owned))
                .is_some_and(|name| qualified.contains(&name.as_str()))
        };
        match annotation {
            Expr::Name(_) | Expr::Attribute(_) => {
                denotes(annotation, &["builtins.int", "builtins.str", "builtins.bool"])
            }
            Expr::Subscript(subscript) if denotes(&subscript.value, &["builtins.tuple", "typing.Tuple"]) => {
                let members: Vec<&Expr> = match subscript.slice.as_ref() {
                    Expr::Tuple(tuple) => tuple.elts.iter().collect(),
                    other => vec![other],
                };
                (1..=6).contains(&members.len())
                    && members.iter().all(|member| self.annotation_is_value_hashable(member, scope))
            }
            _ => false,
        }
    }

    // -----------------------------------------------------------------------
    // `functools.reduce`
    // -----------------------------------------------------------------------

    /// Lower `functools.reduce(function, iterable[, initial])`, or `None` when
    /// `call` is not one.
    ///
    /// Behaviour: the accumulator starts at `initial` (or the first item, the
    /// fold then running over the rest), and each item in order replaces it
    /// with `function(accumulator, item)`; an empty iterable without
    /// `initial` raises `TypeError("reduce() of empty iterable with no
    /// initial value")`, catchable as Python's. Types: the accumulator is the
    /// type of `initial` (the element type without one), `function` is typed
    /// `(accumulator, element) -> accumulator`, and a function returning
    /// another type is refused rather than widened.
    fn functools_reduce_call(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if self.qualified_callee(call).as_deref() != Some(FUNCTOOLS_REDUCE) {
            return Ok(None);
        }
        let span = self.span(call.range());
        if !call.arguments.keywords.is_empty() || call.arguments.args.iter().any(|arg| matches!(arg, Expr::Starred(_))) {
            return Err(SmeltError::unsupported(span, "reduce() takes its arguments positionally"));
        }
        let (function, iterable, initial) = match call.arguments.args.as_ref() {
            [function, iterable] => (function, iterable, None),
            [function, iterable, initial] => (function, iterable, Some(initial)),
            _ => return Err(SmeltError::unsupported(span, "reduce() takes a function, an iterable and an optional initial value")),
        };
        let mut list = self.expression(iterable, body)?;
        if let Some(collected) = self.generator_as_list(list, body) {
            list = collected;
        }
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(item_ty)) = self.ctx.krate.types.get(list_ty).cloned() else {
            return Err(SmeltError::unsupported(
                self.span(iterable.range()),
                "reduce() iterates a list or a generator here; convert the iterable with list(...)",
            ));
        };
        let initial = initial.map(|initial| self.expression(initial, body)).transpose()?;
        let acc_ty = initial.map_or(item_ty, |initial| Self::expr_ty(body, initial));
        let callback = self.python_callback_argument(function, &[acc_ty, item_ty], body)?;
        if callback.return_ty != acc_ty {
            return Err(SmeltError::unsupported(
                self.span(function.range()),
                format!(
                    "reduce() function returns {} where the accumulator is {}; give the initial value the result's type",
                    self.type_display(callback.return_ty),
                    self.type_display(acc_ty)
                ),
            ));
        }
        if initial.is_some() {
            return Ok(Some(body.push_expr(HirExpr {
                kind: ExprKind::ListReduce { list, initial, callback: callback.expr, from_right: false },
                ty: acc_ty,
                span,
            })));
        }
        // No initial value: an empty list raises Python's `TypeError` before
        // the fold, which then always has a first item to start from.
        let block = body.push_block(span);
        let items = body.push_local(self.synthetic_local("__smelt_reduce_items", list_ty, false, span));
        let pat = body.push_pattern(HirPattern::Binding(items));
        body.push_stmt_to_block(block, HirStmt::Let { pat, ty: list_ty, value: Some(list) });
        let guard = synthesized_statements(
            "if not __smelt_reduce_items:\n    raise TypeError(\"reduce() of empty iterable with no initial value\")\n",
            call.range(),
        );
        self.with_block_scope(|this| {
            this.scope.locals.insert("__smelt_reduce_items".to_owned(), items);
            guard.iter().try_for_each(|stmt| this.statement_in_block(stmt, body, block))
        })?;
        let read = body.push_expr(HirExpr { kind: ExprKind::Local(items), ty: list_ty, span });
        let fold = body.push_expr(HirExpr {
            kind: ExprKind::ListReduce { list: read, initial: None, callback: callback.expr, from_right: false },
            ty: acc_ty,
            span,
        });
        let block_index = usize::try_from(block.0).map_err(|error| SmeltError::unsupported(span, error.to_string()))?;
        body.blocks[block_index].tail = Some(fold);
        Ok(Some(body.push_expr(HirExpr { kind: ExprKind::Block(block), ty: acc_ty, span })))
    }

    /// A type as a Python programmer reads it, for diagnostics.
    fn type_display(&self, ty: TypeId) -> String {
        match self.ctx.krate.types.get(ty) {
            Some(Type::PyInt | Type::Int) => "int".to_owned(),
            Some(Type::Float) => "float".to_owned(),
            Some(Type::String) => "str".to_owned(),
            Some(Type::Bool) => "bool".to_owned(),
            Some(other) => format!("{other:?}"),
            None => "an unknown type".to_owned(),
        }
    }

    // -----------------------------------------------------------------------
    // `functools.partial`
    // -----------------------------------------------------------------------

    /// Record `name = functools.partial(func, *args, **kwargs)` as a
    /// [`PartialBinding`] when this scope binds `name` exactly once (else the
    /// assignment lowers as any other, its value being the partial's closure).
    ///
    /// Behaviour: `func` (unless it names a module function, which cannot be
    /// rebound in place) and the bound arguments are evaluated once, here, in
    /// source order, into hidden locals (`bind_partial`). The name itself is
    /// bound to the closure a read of it as a value answers. Answers whether
    /// the statement was recorded.
    fn record_partial_binding(
        &mut self,
        assign: &StmtAssign,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        let ([Expr::Name(target)], Expr::Call(call)) = (assign.targets.as_slice(), assign.value.as_ref()) else {
            return Ok(false);
        };
        if self.qualified_callee(call).as_deref() != Some(FUNCTOOLS_PARTIAL) {
            return Ok(false);
        }
        let name = target.id.as_str();
        let single = if self.scope.is_module {
            self.module_single_binding.contains(name) && !self.items.contains_key(name)
        } else {
            self.scope.single_binding.contains(name)
        };
        if !single || self.scope.locals.contains_key(name) || self.scope.globals.contains(name) {
            return Ok(false);
        }
        let span = self.span(assign.range);
        let mut binding = self.bind_partial(call, body, block)?;
        if let Some((lambda, param_tys, return_ty)) = self.partial_value_lambda(&binding, body, call.range()) {
            let closure = self.lambda_closure(&lambda, &param_tys, return_ty, body)?;
            let ty = Self::expr_ty(body, closure.expr);
            let local = body.push_local(LocalDecl { name: Some(self.intern_name(name)), ty, mutable: false, span });
            let pat = body.push_pattern(HirPattern::Binding(local));
            body.push_stmt_to_block(block, HirStmt::Let { pat, ty, value: Some(closure.expr) });
            self.scope.locals.insert(name.to_owned(), local);
            binding.value = Some(local);
        }
        self.scope.partials.insert(name.to_owned(), binding);
        Ok(true)
    }

    /// Evaluate a `partial(func, *args, **kwargs)` call's operands into
    /// hidden immutable locals of `block`, in source order, and answer the
    /// binding that reads them. A `func` naming a module function or a
    /// partial binding is called by name instead of stored.
    fn bind_partial(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<PartialBinding, SmeltError> {
        let span = self.span(call.range());
        let Some((func, args)) = call.arguments.args.split_first() else {
            return Err(SmeltError::unsupported(span, "partial() needs the function to bind"));
        };
        if call.arguments.args.iter().any(|arg| matches!(arg, Expr::Starred(_))) {
            return Err(SmeltError::unsupported(span, "partial() does not support `*args` unpacking"));
        }
        // Unique within the body: no two partials share a hidden name.
        let slot = body.locals.len();
        let hidden = |this: &mut Self, body: &mut Body, label: String, value: &Expr| -> Result<String, SmeltError> {
            let lowered = this.expression(value, body)?;
            let ty = Self::expr_ty(body, lowered);
            let hidden_name = format!("__smelt_partial_{slot}_{label}");
            // Named as the closures capturing it name it (`closure_captures`).
            let symbol = this.intern_name(&hidden_name);
            let local = body.push_local(LocalDecl { name: Some(symbol), ty, mutable: false, span });
            let pat = body.push_pattern(HirPattern::Binding(local));
            body.push_stmt_to_block(block, HirStmt::Let { pat, ty, value: Some(lowered) });
            this.scope.locals.insert(hidden_name.clone(), local);
            Ok(hidden_name)
        };
        let callee = match self.direct_partial_callee(func) {
            Some(callee) => callee,
            None => {
                let stored = hidden(self, body, "func".to_owned(), func)?;
                let ty = self.scope.locals.get(&stored).map(|local| Self::local_ty(body, *local));
                if !ty.is_some_and(|ty| matches!(self.ctx.krate.types.get(ty), Some(Type::Function(_)))) {
                    return Err(SmeltError::unsupported(self.span(func.range()), "partial() needs a callable"));
                }
                stored
            }
        };
        let mut positional = Vec::with_capacity(args.len());
        for (index, arg) in args.iter().enumerate() {
            positional.push(hidden(self, body, format!("arg{index}"), arg)?);
        }
        let mut keywords = Vec::new();
        for keyword in &call.arguments.keywords {
            let Some(arg) = keyword.arg.as_ref() else {
                return Err(SmeltError::unsupported(span, "partial() does not support `**kwargs` unpacking"));
            };
            let hidden_name = hidden(self, body, format!("kw_{arg}"), &keyword.value)?;
            keywords.push((arg.as_str().to_owned(), hidden_name));
        }
        Ok(PartialBinding { callee, positional, keywords, value: None })
    }

    /// The name a partial can call `func` by without storing it: a module
    /// function not shadowed here or a partial binding of this scope. `None`
    /// for any other callable, which the partial stores when it is created.
    fn direct_partial_callee(&self, func: &Expr) -> Option<String> {
        let Expr::Name(name) = func else {
            return None;
        };
        let spelled = name.id.as_str();
        if let Some(binding) = self.scope.partials.get(spelled)
            && self.scope.locals.get(spelled).copied() == binding.value
        {
            return Some(spelled.to_owned());
        }
        if self.scope.locals.contains_key(spelled) {
            return None;
        }
        let item = self.items.get(spelled)?;
        matches!(self.item_ref(*item), Item::Function(_)).then(|| spelled.to_owned())
    }

    /// What `callee` (a [`PartialBinding::callee`]) calls, for the signature
    /// of a value closure over it.
    fn partial_target(&mut self, callee: &str, body: &Body) -> Option<PartialTarget> {
        let value_local = match self.scope.partials.get(callee) {
            Some(binding) => Some(binding.value?),
            None => self.scope.locals.get(callee).copied(),
        };
        if let Some(local) = value_local {
            let Some(Type::Function(function)) = self.ctx.krate.types.get(Self::local_ty(body, local)).cloned() else {
                return None;
            };
            return Some(PartialTarget::Value(function));
        }
        let Item::Function(function) = self.item_ref(*self.items.get(callee)?).clone() else {
            return None;
        };
        let required = function.required_params.unwrap_or(function.params.len());
        let mut type_params = Vec::new();
        for param in &function.params {
            self.collect_type_param_names(param.ty, &mut type_params);
        }
        let params = function
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| PartialParam {
                name: self.ctx.krate.symbols.get(param.name).map(str::to_owned),
                ty: param.ty,
                required: index < required,
            })
            .collect();
        Some(PartialTarget::Function {
            params,
            return_ty: function.return_ty,
            generic: !function.type_params.is_empty() || !type_params.is_empty(),
        })
    }

    /// The lambda a partial is as a value, with its parameter types and
    /// return type: it takes the callee's parameters the partial leaves
    /// unbound and required, in order, and calls the callee with the bound
    /// arguments followed by its own, plus the bound keywords. A parameter
    /// with a default is left to its default, as a positional caller that
    /// stops before it does.
    ///
    /// Types: each parameter keeps the callee's declared type and the result
    /// is the callee's return type, so the closure is as typed as the `def`.
    ///
    /// `None` when no positional signature exists: a required parameter
    /// after a keyword-bound one can only be passed by keyword, a stored
    /// callable called with keywords has no parameter names here, and a
    /// generic callee's parameter types are only solved per call.
    fn partial_value_lambda(
        &mut self,
        binding: &PartialBinding,
        body: &Body,
        range: TextRange,
    ) -> Option<(ruff_python_ast::ExprLambda, Vec<TypeId>, Option<TypeId>)> {
        let (remaining, return_ty): (Vec<TypeId>, TypeId) = match self.partial_target(&binding.callee, body)? {
            PartialTarget::Function { params, return_ty, generic } => {
                if generic {
                    return None;
                }
                let mut open = Vec::new();
                let mut after_keyword = false;
                for param in params.into_iter().skip(binding.positional.len()) {
                    let bound = param
                        .name
                        .as_ref()
                        .is_some_and(|name| binding.keywords.iter().any(|(keyword, _)| keyword == name));
                    if bound {
                        after_keyword = true;
                    } else if param.required {
                        if after_keyword {
                            return None;
                        }
                        open.push(param.ty);
                    }
                }
                (open, return_ty)
            }
            PartialTarget::Value(function) => {
                if !binding.keywords.is_empty() || function.rest.is_some() {
                    return None;
                }
                let required = function.required_params.unwrap_or(function.params.len());
                let open = function
                    .params
                    .iter()
                    .enumerate()
                    .skip(binding.positional.len())
                    .filter(|(index, _)| *index < required)
                    .map(|(_, ty)| *ty)
                    .collect();
                (open, function.return_ty)
            }
        };
        let own: Vec<String> = (0..remaining.len()).map(|index| format!("__smelt_partial_param_{index}")).collect();
        let mut args = binding.positional.clone();
        args.extend(own.iter().cloned());
        args.extend(binding.keywords.iter().map(|(keyword, hidden)| format!("{keyword}={hidden}")));
        let source = format!("lambda {}: {}({})", own.join(", "), binding.callee, args.join(", "));
        let Some(Expr::Lambda(lambda)) = synthesized_expression(&source, range) else {
            return None;
        };
        Some((lambda, remaining, Some(return_ty)))
    }

    /// `functools.partial(...)` as an expression (not bound to a name of its
    /// own): a block that evaluates `func` and the bound arguments once, when
    /// the expression runs (`bind_partial`), and answers the closure over the
    /// parameters left unbound (`partial_value_lambda`). The hidden locals
    /// are visible to the closure only.
    fn functools_partial_call(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if self.qualified_callee(call).as_deref() != Some(FUNCTOOLS_PARTIAL) {
            return Ok(None);
        }
        let span = self.span(call.range());
        let block = body.push_block(span);
        let closure = self.with_block_scope(|this| {
            let binding = this.bind_partial(call, body, block)?;
            let Some((lambda, param_tys, return_ty)) = this.partial_value_lambda(&binding, body, call.range()) else {
                return Err(SmeltError::unsupported(
                    span,
                    "this partial() cannot be used as a value: a parameter it leaves unbound can only be passed by keyword, or the function is generic; bind it to a name and call it",
                ));
            };
            this.lambda_closure(&lambda, &param_tys, return_ty, body)
        })?;
        let ty = Self::expr_ty(body, closure.expr);
        let block_index = usize::try_from(block.0).map_err(|error| SmeltError::unsupported(span, error.to_string()))?;
        body.blocks[block_index].tail = Some(closure.expr);
        Ok(Some(body.push_expr(HirExpr { kind: ExprKind::Block(block), ty, span })))
    }

    /// A call through a [`PartialBinding`] name: the callee called with the
    /// bound positional arguments, then the call's own, and the bound
    /// keywords merged under the call's (a keyword the call passes overrides
    /// the bound one, as `partial` does). `None` when `call` does not call a
    /// partial binding of this scope.
    ///
    /// The synthesized call lowers as a written one, so argument checking,
    /// defaults and coercions are the callee's own.
    fn partial_binding_call(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
        type_hint: Option<TypeId>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Name(callee) = call.func.as_ref() else {
            return Ok(None);
        };
        let Some(binding) = self.scope.partials.get(callee.id.as_str()).cloned() else {
            return Ok(None);
        };
        if self.scope.locals.get(callee.id.as_str()).copied() != binding.value {
            return Ok(None);
        }
        let range = call.func.range();
        let mut synthesized = call.clone();
        synthesized.func = Box::new(name_expr(&binding.callee, range));
        let mut args: Vec<Expr> = binding.positional.iter().map(|hidden| name_expr(hidden, range)).collect();
        args.extend(call.arguments.args.iter().cloned());
        synthesized.arguments.args = args.into_iter().collect();
        let passed: HashSet<&str> = call
            .arguments
            .keywords
            .iter()
            .filter_map(|keyword| keyword.arg.as_ref().map(ruff_python_ast::Identifier::as_str))
            .collect();
        let mut keywords: Vec<ruff_python_ast::Keyword> = binding
            .keywords
            .iter()
            .filter(|(keyword, _)| !passed.contains(keyword.as_str()))
            .map(|(keyword, hidden)| ruff_python_ast::Keyword {
                range,
                node_index: ruff_python_ast::AtomicNodeIndex::default(),
                arg: Some(ruff_python_ast::Identifier::new(keyword.as_str(), range)),
                value: name_expr(hidden, range),
            })
            .collect();
        keywords.extend(call.arguments.keywords.iter().cloned());
        synthesized.arguments.keywords = keywords.into_iter().collect();
        self.call_expression_with_hint(&synthesized, body, type_hint).map(Some)
    }
}
