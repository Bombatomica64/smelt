// `itertools`: lazy, typed iterator adapters.
// Included into lowering.rs via include!, so this file uses plain comments.
//
// Each callable answers a generator (`Type::Generator`, the frontend's lazy
// iterator carrier) whose body is CPython's own definition of the adapter,
// written as a source template (`lowering/source_templates.rs`). The result
// is resumed one value at a time by whatever consumes it (`for`, `next`,
// `list`, `sum`, `zip`, a comprehension), so an infinite `count`, `cycle` or
// `repeat` ends as soon as `islice`, `zip` or a `break` stops asking, and a
// shared source is pulled exactly as far as CPython pulls it.
//
// Types: the element type of every adapter is the element type of its
// source (`chain` of `list[int]`s yields `int`), `product`, `permutations`
// and `combinations` yield tuples whose arity is static (each element typed),
// `accumulate` yields its running total's type, `groupby` yields
// `(key, group)` with the key function's return type and a generator of the
// source's elements. Nothing is erased.
//
// What CPython evaluates when the adapter is *called* is evaluated there too:
// `islice`'s argument checks raise `ValueError` at the call, and `product`,
// `permutations` and `combinations` read their whole sources into pools at
// the call, as CPython's constructors do.

/// The `ValueError` checks `islice()` makes on its arguments when called, in
/// `CPython`'s order (`itertoolsmodule.c`, `islice_new`): a `stop` of exactly
/// `-1` is the "Stop argument" message, any other negative index the
/// "Indices" one.
const ISLICE_STOP_CHECK: &str = "if __smelt_stop == -1:
    raise ValueError('Stop argument for islice() must be None or an integer: 0 <= x <= sys.maxsize.')
if __smelt_stop < -1:
    raise ValueError('Indices for islice() must be None or an integer: 0 <= x <= sys.maxsize.')
";

/// See [`ISLICE_STOP_CHECK`].
const ISLICE_START_CHECK: &str = "if __smelt_start < 0:
    raise ValueError('Indices for islice() must be None or an integer: 0 <= x <= sys.maxsize.')
";

/// See [`ISLICE_STOP_CHECK`].
const ISLICE_STEP_CHECK: &str = "if __smelt_step < 1:
    raise ValueError('Step for islice() must be a positive integer or None.')
";

/// `islice(it, start, stop, step)` with a `stop`: `CPython`'s `islice_next`.
/// Before each value it skips (pulls and drops) up to the next index, then
/// stops *without pulling* once `stop` is reached, so the source keeps every
/// value after the slice.
const ISLICE_BOUNDED: &str = "__smelt_count = 0
__smelt_next = __smelt_start
if __smelt_count >= __smelt_next and __smelt_count >= __smelt_stop:
    return
for __smelt_item in __smelt_it:
    if __smelt_count < __smelt_next:
        __smelt_count += 1
    else:
        __smelt_count += 1
        __smelt_next += __smelt_step
        if __smelt_next > __smelt_stop:
            __smelt_next = __smelt_stop
        yield __smelt_item
    if __smelt_count >= __smelt_next and __smelt_count >= __smelt_stop:
        return
";

/// `islice(it, start, None, step)`: [`ISLICE_BOUNDED`] without a stop.
const ISLICE_UNBOUNDED: &str = "__smelt_count = 0
__smelt_next = __smelt_start
for __smelt_item in __smelt_it:
    if __smelt_count < __smelt_next:
        __smelt_count += 1
    else:
        __smelt_count += 1
        __smelt_next += __smelt_step
        yield __smelt_item
";

/// `count(start, step)`.
const COUNT: &str = "__smelt_n = __smelt_start
while True:
    yield __smelt_n
    __smelt_n += __smelt_step
";

/// `repeat(object)`: the same object forever.
const REPEAT_FOREVER: &str = "while True:
    yield __smelt_object
";

/// `repeat(object, times)`: the same object `times` times (none when
/// `times <= 0`).
const REPEAT_TIMES: &str = "__smelt_done = 0
while __smelt_done < __smelt_times:
    yield __smelt_object
    __smelt_done += 1
";

/// `cycle(it)`: `CPython` saves each value of the first pass (before
/// yielding it), then repeats the saved values forever; an empty source
/// ends at once.
const CYCLE: &str = "for __smelt_item in __smelt_it:
    __smelt_saved.append(__smelt_item)
    yield __smelt_item
while __smelt_saved:
    for __smelt_item in __smelt_saved:
        yield __smelt_item
";

/// `chain.from_iterable(its)`: each inner iterable in turn, the outer one
/// pulled only when the previous inner one is exhausted.
const CHAIN_FROM_ITERABLE: &str = "for __smelt_inner in __smelt_its:
    for __smelt_item in __smelt_inner:
        yield __smelt_item
";

/// `pairwise(it)`: each overlapping pair of consecutive values; the
/// previous value is a one-slot list cell, empty before the first.
const PAIRWISE: &str = "for __smelt_item in __smelt_it:
    if __smelt_prev:
        yield (__smelt_prev[0], __smelt_item)
        __smelt_prev[0] = __smelt_item
    else:
        __smelt_prev.append(__smelt_item)
";

/// `takewhile(pred, it)`: values while the predicate holds; the first value
/// failing it is consumed and dropped, and nothing more is pulled.
const TAKEWHILE: &str = "for __smelt_item in __smelt_it:
    if __smelt_pred(__smelt_item):
        yield __smelt_item
    else:
        return
";

/// `dropwhile(pred, it)`: values from the first one failing the predicate on,
/// the predicate never called again after that.
const DROPWHILE: &str = "__smelt_dropping = True
for __smelt_item in __smelt_it:
    if __smelt_dropping:
        if __smelt_pred(__smelt_item):
            continue
        __smelt_dropping = False
    yield __smelt_item
";

/// `filterfalse(pred, it)`: the values the predicate rejects (`PRED` is the
/// predicate's call, or the value's own truth for `None`).
const FILTERFALSE: &str = "for __smelt_item in __smelt_it:
    if not PRED:
        yield __smelt_item
";

/// `iter(source)` as a generator: what `groupby` shares between its outer
/// iterator and its groups when the source is a container.
const ITERATE: &str = "for __smelt_item in __smelt_source:
    yield __smelt_item
";

/// Fill a pool with every value of a source: the tuple `product`,
/// `permutations` and `combinations` read their sources into when called.
const FILL_POOL: &str = "for __smelt_item in __smelt_source:
    __smelt_pool.append(__smelt_item)
";

/// `groupby`'s outer iterator, `CPython`'s `groupby_next`, over state shared
/// with its groups through one-slot list cells: `__smelt_vals` holds the
/// current value (`currvalue`, empty once a group took it), `__smelt_keys`
/// the current key (`currkey`), `__smelt_tgt` the key of the group last
/// yielded (`tgtkey`), and `__smelt_gen[0]` the id of the one group still
/// valid (`currgrouper`). Each advance first invalidates the current group,
/// then skips values until the key changes. `KEY` is the key function's
/// call (or the value itself).
const GROUPBY_OUTER: &str = "while True:
    __smelt_gen[0] += 1
    while True:
        if __smelt_keys:
            if not __smelt_tgt:
                break
            if __smelt_tgt[0] != __smelt_keys[0]:
                break
        for __smelt_value in __smelt_it:
            __smelt_k = KEY
            if __smelt_vals:
                __smelt_vals.pop()
            __smelt_vals.append(__smelt_value)
            if __smelt_keys:
                __smelt_keys.pop()
            __smelt_keys.append(__smelt_k)
            break
        else:
            return
    if __smelt_tgt:
        __smelt_tgt.pop()
    __smelt_tgt.append(__smelt_keys[0])
    yield (__smelt_keys[0], __smelt_grouper(ARGS))
";

/// One `groupby` group, `CPython`'s `_grouper_next`: empty once the outer
/// iterator has advanced past it (`__smelt_gen[0]` is no longer its id),
/// otherwise the current value while its key is the group's.
const GROUPBY_GROUP: &str = "while True:
    if __smelt_gen[0] != __smelt_id:
        return
    if not __smelt_vals:
        for __smelt_value in __smelt_it:
            __smelt_k = KEY
            __smelt_vals.append(__smelt_value)
            if __smelt_keys:
                __smelt_keys.pop()
            __smelt_keys.append(__smelt_k)
            break
        else:
            return
    if __smelt_keys[0] != __smelt_target:
        return
    yield __smelt_vals.pop()
";

/// The `itertools` classes whose instances the lowering answers as
/// generators: `ty` spells a value of one by the class's `__name__`.
const ITERTOOLS_ITERATOR_CLASSES: &[&str] = &[
    "chain",
    "islice",
    "count",
    "repeat",
    "cycle",
    "accumulate",
    "product",
    "permutations",
    "combinations",
    "combinations_with_replacement",
    "groupby",
    "_grouper",
    "pairwise",
    "takewhile",
    "dropwhile",
    "filterfalse",
    "starmap",
];

/// Which index tuples of a pool a combinatoric adapter yields, in
/// lexicographic order of positions (`CPython`'s order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexSelection {
    /// `permutations`: every position distinct.
    Distinct,
    /// `combinations`: strictly increasing positions.
    Increasing,
    /// `combinations_with_replacement`: non-decreasing positions.
    NonDecreasing,
}

/// The arguments of an `itertools` call bound to its parameters.
struct BoundArgs<'a> {
    /// The positional arguments.
    positional: Vec<&'a Expr>,
    /// Each keyword argument by name.
    keywords: Vec<(&'a str, &'a Expr)>,
}

impl<'a> BoundArgs<'a> {
    /// The argument for the parameter `name` at `position`, given either way.
    fn get(&self, position: usize, name: &str) -> Option<&'a Expr> {
        self.positional
            .get(position)
            .copied()
            .or_else(|| self.keywords.iter().find(|(keyword, _)| *keyword == name).map(|(_, value)| *value))
    }

    /// An argument that is absent or the literal `None`.
    fn given(&self, position: usize, name: &str) -> Option<&'a Expr> {
        self.get(position, name).filter(|expr| !matches!(expr, Expr::NoneLiteral(_)))
    }
}

/// The value of an `int` literal (`3`, `-1`), if `expr` is one.
fn int_literal(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::NumberLiteral(number) => match &number.value {
            Number::Int(value) => value.as_i64(),
            _ => None,
        },
        Expr::UnaryOp(unary) if unary.op == ruff_python_ast::UnaryOp::USub => int_literal(&unary.operand).and_then(i64::checked_neg),
        _ => None,
    }
}

impl ModuleBuilder<'_> {
    /// Lower a call of an `itertools` callable (`itertools.chain(..)`,
    /// `chain(..)` after `from itertools import chain`); `None` when the
    /// callee is not one.
    fn itertools_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(path) = self.qualified_callee(call) else {
            return Ok(None);
        };
        let Some(function) = path.strip_prefix("itertools.") else {
            return Ok(None);
        };
        let span = self.span(call.range());
        let args = self.bind_itertools_args(call, function)?;
        let outer = body.push_block(span);
        let generator = match function {
            "chain" => self.itertools_chain(&args, call.range(), body)?,
            "chain.from_iterable" => self.itertools_chain_from_iterable(&args, call.range(), body)?,
            "islice" => self.itertools_islice(&args, call.range(), body, outer)?,
            "count" => self.itertools_count(&args, call.range(), body)?,
            "repeat" => self.itertools_repeat(&args, call.range(), body)?,
            "cycle" => self.itertools_cycle(&args, call.range(), body)?,
            "accumulate" => self.itertools_accumulate(&args, call.range(), body)?,
            "product" => self.itertools_product(&args, call.range(), body, outer)?,
            "permutations" => self.itertools_combinatoric(IndexSelection::Distinct, &args, call.range(), body, outer)?,
            "combinations" => self.itertools_combinatoric(IndexSelection::Increasing, &args, call.range(), body, outer)?,
            "combinations_with_replacement" => self.itertools_combinatoric(IndexSelection::NonDecreasing, &args, call.range(), body, outer)?,
            "groupby" => self.itertools_groupby(&args, call.range(), body, outer)?,
            "pairwise" => self.itertools_pairwise(&args, call.range(), body)?,
            "takewhile" | "dropwhile" | "filterfalse" => self.itertools_predicate_filter(function, &args, call.range(), body)?,
            "starmap" => self.itertools_starmap(&args, call.range(), body)?,
            _ => return Ok(None),
        };
        Self::set_block_tail(outer, generator, span, body)?;
        let ty = Self::expr_ty(body, generator);
        Ok(Some(body.push_expr(HirExpr { kind: ExprKind::Block(outer), ty, span })))
    }

    /// Bind a call's arguments by `CPython`'s signature of `function`:
    /// whether it takes keywords and which, and its positional arity.
    fn bind_itertools_args<'a>(&self, call: &'a ruff_python_ast::ExprCall, function: &str) -> Result<BoundArgs<'a>, SmeltError> {
        let span = self.span(call.range());
        // (min positional, max positional (None: any), keyword names)
        let (min, max, keywords): (usize, Option<usize>, &[&str]) = match function {
            "chain" => (0, None, &[]),
            "chain.from_iterable" | "cycle" | "pairwise" => (1, Some(1), &[]),
            "takewhile" | "dropwhile" | "filterfalse" | "starmap" => (2, Some(2), &[]),
            "islice" => (2, Some(4), &[]),
            "count" => (0, Some(2), &["start", "step"]),
            "repeat" => (1, Some(2), &["object", "times"]),
            "accumulate" => (1, Some(2), &["iterable", "func", "initial"]),
            "product" => (0, None, &["repeat"]),
            "permutations" => (1, Some(2), &["iterable", "r"]),
            "combinations" | "combinations_with_replacement" => (2, Some(2), &["iterable", "r"]),
            "groupby" => (1, Some(2), &["iterable", "key"]),
            _ => (0, None, &[]),
        };
        if call.arguments.args.iter().any(|arg| matches!(arg, Expr::Starred(_))) {
            return Err(SmeltError::unsupported(span, format!("itertools.{function}() with *args unpacking is not supported")));
        }
        let mut bound = BoundArgs { positional: call.arguments.args.iter().collect(), keywords: Vec::new() };
        for keyword in &call.arguments.keywords {
            let Some(name) = keyword.arg.as_ref().map(ruff_python_ast::Identifier::as_str) else {
                return Err(SmeltError::unsupported(span, format!("itertools.{function}() with **kwargs unpacking is not supported")));
            };
            let position = keywords.iter().position(|candidate| *candidate == name);
            // `accumulate`'s `initial` and `product`'s `repeat` are
            // keyword-only; the others may also be given positionally.
            let keyword_only = matches!((function, name), ("accumulate", "initial") | ("product", "repeat"));
            if position.is_none() || (!keyword_only && position.is_some_and(|index| index < bound.positional.len())) {
                return Err(SmeltError::unsupported(
                    span,
                    format!("itertools.{function}() got an unexpected or repeated keyword argument '{name}'"),
                ));
            }
            bound.keywords.push((name, &keyword.value));
        }
        let given = bound.positional.len() + bound.keywords.iter().filter(|(name, _)| !matches!(*name, "initial" | "repeat")).count();
        if given < min || max.is_some_and(|max| bound.positional.len() > max) {
            return Err(SmeltError::unsupported(span, format!("itertools.{function}() got the wrong number of arguments")));
        }
        Ok(bound)
    }

    /// The element type a `for` loop over a value of type `ty` binds:
    /// `list`/`set` items, `dict` keys, `str` characters, a homogeneous
    /// tuple's items, a generator's yielded values.
    fn itertools_item_ty(&self, ty: TypeId) -> Option<TypeId> {
        match self.ctx.krate.types.get(ty)? {
            Type::List(item) | Type::Set(item) => Some(*item),
            Type::Dict(key, _) => Some(*key),
            Type::PyStr => Some(ty),
            Type::Generator { is_async: false, yield_ty, .. } => Some(*yield_ty),
            Type::Tuple(items) => {
                let first = *items.first()?;
                items.iter().all(|item| *item == first).then_some(first)
            }
            _ => None,
        }
    }

    /// Lower an iterable argument, answering it with its element type.
    fn itertools_iterable(&mut self, expr: &Expr, function: &str, body: &mut Body) -> Result<(smelt_hir::ExprId, TypeId), SmeltError> {
        self.itertools_iterable_hinted(expr, function, None, body)
    }

    /// [`Self::itertools_iterable`], an empty list display (`[]`, whose
    /// element type nothing in it states) typed `list[item_hint]`: the
    /// element type the call's other arguments give it (`chain(xs, [])`,
    /// `accumulate([], initial=0)`).
    fn itertools_iterable_hinted(
        &mut self,
        expr: &Expr,
        function: &str,
        item_hint: Option<TypeId>,
        body: &mut Body,
    ) -> Result<(smelt_hir::ExprId, TypeId), SmeltError> {
        let value = match (expr, item_hint) {
            (Expr::List(list), Some(item)) if list.elts.is_empty() => {
                let hint = self.intern_type(Type::List(item));
                self.expression_with_hint(expr, body, Some(hint))?
            }
            _ => self.expression(expr, body)?,
        };
        let ty = Self::expr_ty(body, value);
        let Some(item) = self.itertools_item_ty(ty) else {
            return Err(SmeltError::unsupported(
                self.span(expr.range()),
                format!("itertools.{function}() takes a list, set, dict, str, homogeneous tuple or generator"),
            ));
        };
        Ok((value, item))
    }

    /// Lower an `int` argument.
    fn itertools_int(&mut self, expr: &Expr, what: &str, body: &mut Body) -> Result<smelt_hir::ExprId, SmeltError> {
        let value = self.expression(expr, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, value)) != Some(&Type::PyInt) {
            return Err(SmeltError::unsupported(self.span(expr.range()), format!("{what} must be an int")));
        }
        Ok(value)
    }

    /// An `int` literal value of the caller's body.
    fn itertools_int_value(&mut self, value: i64, span: Span, body: &mut Body) -> smelt_hir::ExprId {
        let ty = self.intern_type(Type::PyInt);
        body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Int(value)), ty, span })
    }

    /// A fresh empty `list[item]`: a template's private list or state cell.
    fn itertools_empty_list(&mut self, item: TypeId, span: Span, body: &mut Body) -> smelt_hir::ExprId {
        let ty = self.intern_type(Type::List(item));
        body.push_expr(HirExpr { kind: ExprKind::ListLit(Vec::new()), ty, span })
    }

    /// A read of a local of the caller's body.
    fn itertools_read(local: smelt_hir::LocalId, span: Span, body: &mut Body) -> smelt_hir::ExprId {
        let ty = Self::local_ty(body, local);
        body.push_expr(HirExpr { kind: ExprKind::Local(local), ty, span })
    }

    /// `chain(*iterables)`: each iterable in turn, all of one element type.
    fn itertools_chain(&mut self, args: &BoundArgs<'_>, range: TextRange, body: &mut Body) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(range);
        let mut params = Vec::new();
        let mut item_ty = None;
        let mut source = String::new();
        // An empty `[]` takes the element type of the first typed iterable.
        let typed_first = args.positional.iter().position(|arg| !matches!(arg, Expr::List(list) if list.elts.is_empty()));
        let mut lowered = vec![None; args.positional.len()];
        if let Some(first) = typed_first {
            lowered[first] = Some(self.itertools_iterable(args.positional[first], "chain", body)?);
        }
        let hint = lowered.iter().flatten().next().map(|(_, item)| *item);
        for (index, arg) in args.positional.iter().enumerate() {
            let (value, item) = match lowered[index] {
                Some(done) => done,
                None => self.itertools_iterable_hinted(arg, "chain", hint, body)?,
            };
            if item_ty.is_some_and(|expected| expected != item) {
                return Err(SmeltError::unsupported(
                    self.span(arg.range()),
                    "itertools.chain() iterables must share one element type",
                ));
            }
            item_ty = Some(item);
            let name = format!("__smelt_it{index}");
            source.push_str(&format!("for __smelt_item in {name}:\n    yield __smelt_item\n"));
            params.push(TemplateParam { name, value });
        }
        let Some(item_ty) = item_ty else {
            return Err(SmeltError::unsupported(span, "itertools.chain() needs at least one iterable to type its elements"));
        };
        self.generator_template(&source, &params, item_ty, range, body, |_, _| Ok(()))
    }

    /// `chain.from_iterable(iterables)`.
    fn itertools_chain_from_iterable(&mut self, args: &BoundArgs<'_>, range: TextRange, body: &mut Body) -> Result<smelt_hir::ExprId, SmeltError> {
        let arg = args.positional.first().copied().ok_or_else(|| SmeltError::unsupported(self.span(range), "chain.from_iterable() takes one iterable"))?;
        let (value, inner_ty) = self.itertools_iterable(arg, "chain.from_iterable", body)?;
        let Some(item_ty) = self.itertools_item_ty(inner_ty) else {
            return Err(SmeltError::unsupported(self.span(arg.range()), "chain.from_iterable() takes an iterable of iterables"));
        };
        let params = [TemplateParam { name: "__smelt_its".to_owned(), value }];
        self.generator_template(CHAIN_FROM_ITERABLE, &params, item_ty, range, body, |_, _| Ok(()))
    }

    /// `islice(it, stop)` / `islice(it, start, stop[, step])`; a `None` stop
    /// is unbounded, a `None` start `0` and a `None` step `1`.
    fn itertools_islice(
        &mut self,
        args: &BoundArgs<'_>,
        range: TextRange,
        body: &mut Body,
        outer: smelt_hir::BlockId,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(range);
        let (start_arg, stop_arg, step_arg) = match args.positional.as_slice() {
            [_, stop] => (None, Some(*stop), None),
            [_, start, stop] => (Some(*start), Some(*stop), None),
            [_, start, stop, step] => (Some(*start), Some(*stop), Some(*step)),
            _ => return Err(SmeltError::unsupported(span, "islice() takes an iterable and a stop, or start, stop and step")),
        };
        let none = |arg: Option<&Expr>| arg.is_none_or(|expr| matches!(expr, Expr::NoneLiteral(_)));
        let (value, item_ty) = self.itertools_iterable(args.positional[0], "islice", body)?;
        let it = self.spill_template_value("__smelt_it", value, body, outer);
        let mut bound = Vec::new();
        let mut checks = String::new();
        let mut index = |this: &mut Self, arg: Option<&Expr>, default: i64, name: &'static str, check: &str, body: &mut Body| -> Result<smelt_hir::LocalId, SmeltError> {
            let value = match arg.filter(|_| !none(arg)) {
                Some(expr) => {
                    // A literal the check would let through needs no check.
                    if int_literal(expr).is_none_or(|literal| if name == "__smelt_step" { literal < 1 } else { literal < 0 }) {
                        checks.push_str(check);
                    }
                    this.itertools_int(expr, "an islice() index", body)?
                }
                None => this.itertools_int_value(default, span, body),
            };
            let local = this.spill_template_value(name, value, body, outer);
            bound.push((name, local));
            Ok(local)
        };
        // CPython reads stop before start and step.
        let stop = if none(stop_arg) { None } else { Some(index(self, stop_arg, 0, "__smelt_stop", ISLICE_STOP_CHECK, body)?) };
        let start = index(self, start_arg, 0, "__smelt_start", ISLICE_START_CHECK, body)?;
        let step = index(self, step_arg, 1, "__smelt_step", ISLICE_STEP_CHECK, body)?;
        if !checks.is_empty() {
            self.inline_template(&checks, &bound, range, body, outer)?;
        }
        let mut params = vec![
            TemplateParam { name: "__smelt_it".to_owned(), value: Self::itertools_read(it, span, body) },
            TemplateParam { name: "__smelt_start".to_owned(), value: Self::itertools_read(start, span, body) },
            TemplateParam { name: "__smelt_step".to_owned(), value: Self::itertools_read(step, span, body) },
        ];
        let source = if let Some(stop) = stop {
            params.push(TemplateParam { name: "__smelt_stop".to_owned(), value: Self::itertools_read(stop, span, body) });
            ISLICE_BOUNDED
        } else {
            ISLICE_UNBOUNDED
        };
        self.generator_template(source, &params, item_ty, range, body, |_, _| Ok(()))
    }

    /// `count(start=0, step=1)`: `start`, `start + step`, ... forever. Both
    /// are `int`s or both `float`s, so every value has one type.
    fn itertools_count(&mut self, args: &BoundArgs<'_>, range: TextRange, body: &mut Body) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(range);
        let numeric = |this: &mut Self, arg: Option<&Expr>, default: i64, body: &mut Body| -> Result<smelt_hir::ExprId, SmeltError> {
            match arg {
                Some(expr) => {
                    let value = this.expression(expr, body)?;
                    if !matches!(this.ctx.krate.types.get(Self::expr_ty(body, value)), Some(Type::PyInt | Type::Float)) {
                        return Err(SmeltError::unsupported(this.span(expr.range()), "count() start and step must be int or float"));
                    }
                    Ok(value)
                }
                None => Ok(this.itertools_int_value(default, span, body)),
            }
        };
        let start = numeric(self, args.get(0, "start"), 0, body)?;
        let step = numeric(self, args.get(1, "step"), 1, body)?;
        let value_ty = Self::expr_ty(body, start);
        if Self::expr_ty(body, step) != value_ty {
            return Err(SmeltError::unsupported(
                span,
                "count() with an int and a float yields an int then floats; give both as the same type",
            ));
        }
        let params = [
            TemplateParam { name: "__smelt_start".to_owned(), value: start },
            TemplateParam { name: "__smelt_step".to_owned(), value: step },
        ];
        self.generator_template(COUNT, &params, value_ty, range, body, |_, _| Ok(()))
    }

    /// `repeat(object[, times])`.
    fn itertools_repeat(&mut self, args: &BoundArgs<'_>, range: TextRange, body: &mut Body) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(range);
        let object = args.get(0, "object").ok_or_else(|| SmeltError::unsupported(span, "repeat() takes the object to repeat"))?;
        let object = self.expression(object, body)?;
        let object_ty = Self::expr_ty(body, object);
        let mut params = vec![TemplateParam { name: "__smelt_object".to_owned(), value: object }];
        let source = if let Some(times) = args.given(1, "times") {
            let times = self.itertools_int(times, "repeat() times", body)?;
            params.push(TemplateParam { name: "__smelt_times".to_owned(), value: times });
            REPEAT_TIMES
        } else {
            REPEAT_FOREVER
        };
        self.generator_template(source, &params, object_ty, range, body, |_, _| Ok(()))
    }

    /// `cycle(iterable)`.
    fn itertools_cycle(&mut self, args: &BoundArgs<'_>, range: TextRange, body: &mut Body) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(range);
        let (value, item_ty) = self.itertools_iterable(args.positional[0], "cycle", body)?;
        let saved = self.itertools_empty_list(item_ty, span, body);
        let params = [
            TemplateParam { name: "__smelt_it".to_owned(), value },
            TemplateParam { name: "__smelt_saved".to_owned(), value: saved },
        ];
        self.generator_template(CYCLE, &params, item_ty, range, body, |_, _| Ok(()))
    }

    /// `accumulate(iterable, func=operator.add, *, initial=None)`: the running
    /// total, `+` or `func(total, value)` of each value in turn, starting
    /// from the first value (or from `initial`, which is yielded first).
    fn itertools_accumulate(&mut self, args: &BoundArgs<'_>, range: TextRange, body: &mut Body) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(range);
        let iterable = args.get(0, "iterable").ok_or_else(|| SmeltError::unsupported(span, "accumulate() takes an iterable"))?;
        // Arguments evaluate in source order; only an empty `[]` (which
        // evaluates nothing) waits for `initial` to type it.
        let empty_display = matches!(iterable, Expr::List(list) if list.elts.is_empty());
        let early = if empty_display { None } else { Some(self.itertools_iterable(iterable, "accumulate", body)?) };
        let initial = args.given(usize::MAX, "initial").map(|initial| self.expression(initial, body)).transpose()?;
        let initial_ty = initial.map(|initial| Self::expr_ty(body, initial));
        let (value, item_ty) = match early {
            Some(lowered) => lowered,
            None => self.itertools_iterable_hinted(iterable, "accumulate", initial_ty, body)?,
        };
        let mut params = vec![TemplateParam { name: "__smelt_it".to_owned(), value }];
        let total_ty = initial.map_or(item_ty, |initial| Self::expr_ty(body, initial));
        let step = if let Some(func) = args.given(1, "func") {
            let callback = self.python_callback_argument(func, &[total_ty, item_ty], body)?;
            if callback.return_ty != total_ty {
                return Err(SmeltError::unsupported(
                    self.span(func.range()),
                    "accumulate() func must return the running total's type",
                ));
            }
            params.push(TemplateParam { name: "__smelt_func".to_owned(), value: callback.expr });
            "__smelt_func(TOTAL, __smelt_item)"
        } else {
            "TOTAL + __smelt_item"
        };
        let source = if let Some(initial) = initial {
            params.push(TemplateParam { name: "__smelt_initial".to_owned(), value: initial });
            format!(
                "__smelt_total = __smelt_initial\nyield __smelt_total\nfor __smelt_item in __smelt_it:\n    __smelt_total = {}\n    yield __smelt_total\n",
                step.replace("TOTAL", "__smelt_total"),
            )
        } else {
            // The total is a one-slot list: empty until the first value.
            let cell = self.itertools_empty_list(item_ty, span, body);
            params.push(TemplateParam { name: "__smelt_total".to_owned(), value: cell });
            format!(
                "for __smelt_item in __smelt_it:\n    if __smelt_total:\n        __smelt_total[0] = {}\n    else:\n        __smelt_total.append(__smelt_item)\n    yield __smelt_total[0]\n",
                step.replace("TOTAL", "__smelt_total[0]"),
            )
        };
        self.generator_template(&source, &params, total_ty, range, body, |_, _| Ok(()))
    }

    /// Read `expr`'s whole source into a fresh list local of `outer` (the
    /// pool `CPython`'s combinatoric constructors build), answering the local
    /// and the element type.
    fn itertools_pool(
        &mut self,
        expr: &Expr,
        function: &str,
        range: TextRange,
        body: &mut Body,
        outer: smelt_hir::BlockId,
    ) -> Result<(smelt_hir::LocalId, TypeId), SmeltError> {
        let span = self.span(range);
        let (value, item_ty) = self.itertools_iterable(expr, function, body)?;
        let source = self.spill_template_value("__smelt_source", value, body, outer);
        let empty = self.itertools_empty_list(item_ty, span, body);
        let pool = self.spill_template_value("__smelt_pool", empty, body, outer);
        self.inline_template(FILL_POOL, &[("__smelt_source", source), ("__smelt_pool", pool)], range, body, outer)?;
        Ok((pool, item_ty))
    }

    /// A non-negative `int` literal argument: the static arity of the tuples
    /// a combinatoric adapter yields.
    fn itertools_arity(&self, expr: &Expr, what: &str) -> Result<usize, SmeltError> {
        int_literal(expr)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| {
                SmeltError::unsupported(
                    self.span(expr.range()),
                    format!("{what} must be a non-negative int literal: it fixes the arity of the yielded tuples"),
                )
            })
    }

    /// `product(*iterables, repeat=1)`: every tuple taking one value from each
    /// pool (the pools repeated `repeat` times), the rightmost advancing
    /// fastest.
    fn itertools_product(
        &mut self,
        args: &BoundArgs<'_>,
        range: TextRange,
        body: &mut Body,
        outer: smelt_hir::BlockId,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(range);
        let repeat = match args.given(usize::MAX, "repeat") {
            Some(expr) => self.itertools_arity(expr, "product() repeat")?,
            None => 1,
        };
        let mut pools = Vec::new();
        for arg in &args.positional {
            pools.push(self.itertools_pool(arg, "product", range, body, outer)?);
        }
        let mut params = Vec::new();
        for (index, (pool, _)) in pools.iter().enumerate() {
            params.push(TemplateParam { name: format!("__smelt_pool{index}"), value: Self::itertools_read(*pool, span, body) });
        }
        let positions = pools.len().saturating_mul(repeat);
        let mut source = String::new();
        let mut element_tys = Vec::new();
        let mut names = Vec::new();
        for position in 0..positions {
            let pool = position % pools.len().max(1);
            let indent = "    ".repeat(position);
            source.push_str(&format!("{indent}for __smelt_a{position} in __smelt_pool{pool}:\n"));
            element_tys.push(pools[pool].1);
            names.push(format!("__smelt_a{position}"));
        }
        let tuple_ty = self.intern_type(Type::Tuple(element_tys));
        source.push_str(&format!("{}yield ({}{})\n", "    ".repeat(positions), names.join(", "), if names.len() == 1 { "," } else { "" }));
        self.generator_template(&source, &params, tuple_ty, range, body, |_, _| Ok(()))
    }

    /// `permutations(iterable, r=None)`, `combinations(iterable, r)` and
    /// `combinations_with_replacement(iterable, r)`: the `r`-tuples of the
    /// pool whose positions `selection` admits, in lexicographic order of
    /// positions. `r` is static (the tuple arity); `permutations` without `r`
    /// takes the source's static length.
    fn itertools_combinatoric(
        &mut self,
        selection: IndexSelection,
        args: &BoundArgs<'_>,
        range: TextRange,
        body: &mut Body,
        outer: smelt_hir::BlockId,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(range);
        let function = match selection {
            IndexSelection::Distinct => "permutations",
            IndexSelection::Increasing => "combinations",
            IndexSelection::NonDecreasing => "combinations_with_replacement",
        };
        let iterable = args.get(0, "iterable").ok_or_else(|| SmeltError::unsupported(span, format!("{function}() takes an iterable")))?;
        let arity = match args.given(1, "r") {
            Some(r) => self.itertools_arity(r, &format!("{function}() r"))?,
            None => self.static_length(iterable, body).ok_or_else(|| {
                SmeltError::unsupported(
                    span,
                    "permutations() without r yields tuples as long as its source; pass r, or a source of static length",
                )
            })?,
        };
        let (pool, item_ty) = self.itertools_pool(iterable, function, range, body, outer)?;
        let params = [TemplateParam { name: "__smelt_pool".to_owned(), value: Self::itertools_read(pool, span, body) }];
        let mut source = String::new();
        for level in 0..arity {
            let indent = "    ".repeat(level * 2);
            source.push_str(&format!("{indent}__smelt_i{level} = 0\n{indent}for __smelt_a{level} in __smelt_pool:\n"));
            let admitted = match selection {
                IndexSelection::Distinct => (0..level).map(|earlier| format!("__smelt_i{level} != __smelt_i{earlier}")).collect::<Vec<_>>(),
                IndexSelection::Increasing => (level > 0).then(|| format!("__smelt_i{level} > __smelt_i{}", level - 1)).into_iter().collect(),
                IndexSelection::NonDecreasing => (level > 0).then(|| format!("__smelt_i{level} >= __smelt_i{}", level - 1)).into_iter().collect(),
            };
            let condition = if admitted.is_empty() { "True".to_owned() } else { admitted.join(" and ") };
            source.push_str(&format!("{indent}    if {condition}:\n"));
        }
        let names = (0..arity).map(|level| format!("__smelt_a{level}")).collect::<Vec<_>>();
        source.push_str(&format!("{}yield ({}{})\n", "    ".repeat(arity * 2), names.join(", "), if arity == 1 { "," } else { "" }));
        for level in (0..arity).rev() {
            source.push_str(&format!("{}    __smelt_i{level} += 1\n", "    ".repeat(level * 2)));
        }
        let tuple_ty = self.intern_type(Type::Tuple(vec![item_ty; arity]));
        self.generator_template(&source, &params, tuple_ty, range, body, |_, _| Ok(()))
    }

    /// The number of values `expr` iterates when it is statically known: a
    /// list, set or tuple display without unpacking, a `str` literal, or a
    /// value of a fixed-size tuple type.
    fn static_length(&self, expr: &Expr, body: &Body) -> Option<usize> {
        match expr {
            Expr::List(list) if !list.elts.iter().any(|elt| matches!(elt, Expr::Starred(_))) => Some(list.elts.len()),
            Expr::Tuple(tuple) if !tuple.elts.iter().any(|elt| matches!(elt, Expr::Starred(_))) => Some(tuple.elts.len()),
            Expr::StringLiteral(string) => Some(string.value.to_str().chars().count()),
            Expr::Name(name) => {
                let local = self.scope.locals.get(name.id.as_str()).copied()?;
                match self.ctx.krate.types.get(Self::local_ty(body, local)) {
                    Some(Type::Tuple(items)) => Some(items.len()),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// `groupby(iterable, key=None)`: `(key, group)` for each run of values
    /// with equal keys, sharing one iterator of the source with its groups
    /// (see [`GROUPBY_OUTER`] and [`GROUPBY_GROUP`]).
    fn itertools_groupby(
        &mut self,
        args: &BoundArgs<'_>,
        range: TextRange,
        body: &mut Body,
        outer: smelt_hir::BlockId,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(range);
        let iterable = args.get(0, "iterable").ok_or_else(|| SmeltError::unsupported(span, "groupby() takes an iterable"))?;
        let (value, item_ty) = self.itertools_iterable(iterable, "groupby", body)?;
        // The groups pull from the same iterator as the outer one: a
        // container is iterated through one generator they share.
        let shared = if matches!(self.ctx.krate.types.get(Self::expr_ty(body, value)), Some(Type::Generator { .. })) {
            value
        } else {
            let params = [TemplateParam { name: "__smelt_source".to_owned(), value }];
            self.generator_template(ITERATE, &params, item_ty, range, body, |_, _| Ok(()))?
        };
        let it = self.spill_template_value("__smelt_it", shared, body, outer);
        let key = args.given(1, "key").map(|key| self.python_callback_argument(key, &[item_ty], body)).transpose()?;
        let key_ty = key.as_ref().map_or(item_ty, |callback| callback.return_ty);
        let int_ty = self.intern_type(Type::PyInt);
        let it_ty = Self::local_ty(body, it);
        let gen_cell_ty = self.intern_type(Type::List(int_ty));
        let vals_ty = self.intern_type(Type::List(item_ty));
        let keys_ty = self.intern_type(Type::List(key_ty));
        let key_call = if key.is_some() { "__smelt_key(__smelt_value)" } else { "__smelt_value" };
        let mut params = vec![TemplateParam { name: "__smelt_it".to_owned(), value: Self::itertools_read(it, span, body) }];
        let mut group_specs = vec![("__smelt_it".to_owned(), it_ty)];
        if let Some(callback) = &key {
            params.push(TemplateParam { name: "__smelt_key".to_owned(), value: callback.expr });
            group_specs.push(("__smelt_key".to_owned(), Self::expr_ty(body, callback.expr)));
        }
        let zero = self.itertools_int_value(0, span, body);
        let gen_cell = body.push_expr(HirExpr { kind: ExprKind::ListLit(vec![zero]), ty: gen_cell_ty, span });
        let vals = self.itertools_empty_list(item_ty, span, body);
        let keys = self.itertools_empty_list(key_ty, span, body);
        let tgt = self.itertools_empty_list(key_ty, span, body);
        params.push(TemplateParam { name: "__smelt_vals".to_owned(), value: vals });
        params.push(TemplateParam { name: "__smelt_keys".to_owned(), value: keys });
        params.push(TemplateParam { name: "__smelt_gen".to_owned(), value: gen_cell });
        params.push(TemplateParam { name: "__smelt_tgt".to_owned(), value: tgt });
        group_specs.push(("__smelt_vals".to_owned(), vals_ty));
        group_specs.push(("__smelt_keys".to_owned(), keys_ty));
        group_specs.push(("__smelt_gen".to_owned(), gen_cell_ty));
        group_specs.push(("__smelt_id".to_owned(), int_ty));
        group_specs.push(("__smelt_target".to_owned(), key_ty));
        let none_ty = self.intern_type(Type::None);
        let group_ty = self.intern_type(Type::Generator { is_async: false, yield_ty: item_ty, return_ty: none_ty, next_ty: none_ty });
        let pair_ty = self.intern_type(Type::Tuple(vec![key_ty, group_ty]));
        let group_args = group_specs
            .iter()
            .map(|(name, _)| match name.as_str() {
                "__smelt_id" => "__smelt_gen[0]".to_owned(),
                "__smelt_target" => "__smelt_keys[0]".to_owned(),
                other => other.to_owned(),
            })
            .collect::<Vec<_>>()
            .join(", ");
        let outer_source = GROUPBY_OUTER.replace("KEY", key_call).replace("ARGS", &group_args);
        let group_source = GROUPBY_GROUP.replace("KEY", key_call);
        self.generator_template(&outer_source, &params, pair_ty, range, body, |this, adapter| {
            let grouper = this.generator_template_closure(&group_source, &group_specs, item_ty, range, adapter)?;
            let grouper_ty = Self::expr_ty(adapter, grouper);
            let callback = LocalCallback { defaults: vec![None; group_specs.len()], vararg: None, kwarg: None };
            let root = adapter.root;
            this.bind_callable_local("__smelt_grouper", grouper, grouper_ty, callback, (span, adapter, root));
            Ok(())
        })
    }

    /// `pairwise(iterable)`: `(previous, value)` tuples.
    fn itertools_pairwise(&mut self, args: &BoundArgs<'_>, range: TextRange, body: &mut Body) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(range);
        let (value, item_ty) = self.itertools_iterable(args.positional[0], "pairwise", body)?;
        let prev = self.itertools_empty_list(item_ty, span, body);
        let params = [
            TemplateParam { name: "__smelt_it".to_owned(), value },
            TemplateParam { name: "__smelt_prev".to_owned(), value: prev },
        ];
        let pair_ty = self.intern_type(Type::Tuple(vec![item_ty, item_ty]));
        self.generator_template(PAIRWISE, &params, pair_ty, range, body, |_, _| Ok(()))
    }

    /// `takewhile(pred, iterable)`, `dropwhile(pred, iterable)` and
    /// `filterfalse(pred, iterable)`: the source's values the predicate
    /// (called with each value, its result's Python truth deciding) selects.
    /// `filterfalse` also takes `None`, the values' own truth.
    fn itertools_predicate_filter(
        &mut self,
        function: &str,
        args: &BoundArgs<'_>,
        range: TextRange,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let (predicate, iterable) = (args.positional[0], args.positional[1]);
        self.itertools_callable_first(predicate, function)?;
        let (value, item_ty) = self.itertools_iterable(iterable, function, body)?;
        let mut params = vec![TemplateParam { name: "__smelt_it".to_owned(), value }];
        let source = if function == "filterfalse" && matches!(predicate, Expr::NoneLiteral(_)) {
            FILTERFALSE.replace("PRED", "__smelt_item")
        } else {
            let callback = self.python_callback_argument(predicate, &[item_ty], body)?;
            params.push(TemplateParam { name: "__smelt_pred".to_owned(), value: callback.expr });
            match function {
                "takewhile" => TAKEWHILE.to_owned(),
                "dropwhile" => DROPWHILE.to_owned(),
                _ => FILTERFALSE.replace("PRED", "__smelt_pred(__smelt_item)"),
            }
        };
        self.generator_template(&source, &params, item_ty, range, body, |_, _| Ok(()))
    }

    /// `starmap(func, iterable)`: `func(*args)` for each tuple of arguments,
    /// each tuple's arity static, so the call passes its elements
    /// positionally.
    fn itertools_starmap(&mut self, args: &BoundArgs<'_>, range: TextRange, body: &mut Body) -> Result<smelt_hir::ExprId, SmeltError> {
        let (func, iterable) = (args.positional[0], args.positional[1]);
        self.itertools_callable_first(func, "starmap")?;
        let (value, item_ty) = self.itertools_iterable(iterable, "starmap", body)?;
        let Some(Type::Tuple(arg_tys)) = self.ctx.krate.types.get(item_ty).cloned() else {
            return Err(SmeltError::unsupported(self.span(iterable.range()), "starmap() takes an iterable of tuples"));
        };
        let callback = self.python_callback_argument(func, &arg_tys, body)?;
        let call_args = (0..arg_tys.len()).map(|index| format!("__smelt_item[{index}]")).collect::<Vec<_>>().join(", ");
        let source = format!("for __smelt_item in __smelt_it:\n    yield __smelt_func({call_args})\n");
        let params = [
            TemplateParam { name: "__smelt_it".to_owned(), value },
            TemplateParam { name: "__smelt_func".to_owned(), value: callback.expr },
        ];
        self.generator_template(&source, &params, callback.return_ty, range, body, |_, _| Ok(()))
    }

    /// A function argument written before the iterable whose element types
    /// type it (`takewhile(pred, it)`) is lowered after it; that keeps
    /// `CPython`'s left-to-right evaluation only when evaluating the function
    /// expression runs nothing: a lambda, a name or an attribute read. Any
    /// other expression (a call returning the function) is refused.
    fn itertools_callable_first(&self, func: &Expr, function: &str) -> Result<(), SmeltError> {
        if matches!(func, Expr::Lambda(_) | Expr::Name(_) | Expr::Attribute(_) | Expr::NoneLiteral(_)) {
            return Ok(());
        }
        Err(SmeltError::unsupported(
            self.span(func.range()),
            format!("{function}() takes its function as a lambda or a name; bind a computed function to a name first"),
        ))
    }
}
