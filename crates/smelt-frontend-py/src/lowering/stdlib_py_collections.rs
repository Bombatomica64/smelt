// Python's `collections` containers: `deque`, `Counter`, `defaultdict` and
// `OrderedDict`.
//
// Each is a typed generic value, never an erased one: `deque[T]`,
// `Counter[K]`, `defaultdict[K, V]` and `OrderedDict[K, V]` are HIR classes
// with reserved names (`smelt_stdlib::PY_DEQUE_CLASS_NAME`, ...) whose type
// arguments are the element, key and value types, and which the backend
// renders as the runtime's concrete `SmeltPyDeque<T>`, `SmeltPyCounter<K>`,
// `SmeltPyDefaultDict<K, V>` and `SmeltPyOrderedDict<K, V>`. Every operation
// is a `PyCollectionNew` or `PyCollectionOp` over operands whose types this
// module checks against those arguments, so a wrong-typed key is refused here
// rather than becoming a Rust type error.
//
// The classes are found by BINDING, never by spelling: `deque` after
// `from collections import deque`, `collections.deque`, `c.deque` after
// `import collections as c`, and the `typing` aliases (`typing.Deque`,
// `typing.Counter`, `typing.DefaultDict`, `typing.OrderedDict`) in annotation
// position. A program's own `deque` is its own class.
//
// Behavioural requirements, each pinned by `collections_basics.py`:
// * a `defaultdict` read of a missing key inserts the factory's value and
//   returns it (so `d[k].append(x)` appends to the stored list), while `.get`
//   and `in` never insert;
// * a `Counter` read of a missing key is `0` and does not insert;
// * `repr` is `CPython`'s for each (`deque([1, 2], maxlen=3)`,
//   `Counter({'a': 2})` in `most_common` order, `defaultdict(<class 'list'>,
//   {...})`, `OrderedDict({'a': 1})`);
// * iteration walks a snapshot of the elements (a deque) or keys (the
//   mappings) in order.
//
// Type preservation: every result is typed from the receiver's arguments
// (`d.popleft()` of a `deque[int]` is an `int`, `c.most_common()` of a
// `Counter[str]` a `list[tuple[str, int]]`), the same table the backend's
// `py_collection_result_ty` answers from.

/// One of Python's `collections` container classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PyCollectionClass {
    /// `collections.deque[T]`.
    Deque,
    /// `collections.Counter[K]`.
    Counter,
    /// `collections.defaultdict[K, V]`.
    DefaultDict,
    /// `collections.OrderedDict[K, V]`.
    OrderedDict,
}

impl PyCollectionClass {
    /// The class a fully qualified Python name denotes.
    fn from_qualified(path: &str) -> Option<Self> {
        match path {
            "collections.deque" | "typing.Deque" => Some(Self::Deque),
            "collections.Counter" | "typing.Counter" => Some(Self::Counter),
            "collections.defaultdict" | "typing.DefaultDict" => Some(Self::DefaultDict),
            "collections.OrderedDict" | "typing.OrderedDict" => Some(Self::OrderedDict),
            _ => None,
        }
    }

    /// The reserved HIR class name the backend renders as the runtime type.
    const fn class_name(self) -> &'static str {
        match self {
            Self::Deque => smelt_stdlib::PY_DEQUE_CLASS_NAME,
            Self::Counter => smelt_stdlib::PY_COUNTER_CLASS_NAME,
            Self::DefaultDict => smelt_stdlib::PY_DEFAULT_DICT_CLASS_NAME,
            Self::OrderedDict => smelt_stdlib::PY_ORDERED_DICT_CLASS_NAME,
        }
    }

    /// The class from its reserved HIR name.
    fn from_class_name(name: &str) -> Option<Self> {
        [Self::Deque, Self::Counter, Self::DefaultDict, Self::OrderedDict]
            .into_iter()
            .find(|class| class.class_name() == name)
    }

    /// The Python spelling, for diagnostics.
    const fn python_name(self) -> &'static str {
        match self {
            Self::Deque => "deque",
            Self::Counter => "Counter",
            Self::DefaultDict => "defaultdict",
            Self::OrderedDict => "OrderedDict",
        }
    }

    /// How many type arguments the class takes.
    const fn arity(self) -> usize {
        match self {
            Self::Deque | Self::Counter => 1,
            Self::DefaultDict | Self::OrderedDict => 2,
        }
    }

    /// Whether the class is a key/value mapping (`defaultdict`, `OrderedDict`).
    const fn is_mapping(self) -> bool {
        matches!(self, Self::DefaultDict | Self::OrderedDict)
    }
}

/// A container type, split into its class and type arguments.
#[derive(Debug, Clone)]
struct PyCollectionType {
    /// The container class.
    class: PyCollectionClass,
    /// The element type (a deque) or key type (the others).
    first: TypeId,
    /// The mapped value type: a `Counter`'s `int`, a mapping's `V`; the
    /// element type for a deque.
    value: TypeId,
}

impl ModuleBuilder<'_> {
    /// The `collections` class a name or attribute chain denotes through the
    /// program's imports.
    fn py_collection_class_of(&self, expr: &Expr) -> Option<PyCollectionClass> {
        let path = match expr {
            Expr::Name(name) => self.qualified_import_path(name.id.as_str(), &[])?,
            Expr::Attribute(attribute) => self.qualified_attribute(attribute)?,
            _ => return None,
        };
        PyCollectionClass::from_qualified(&path)
    }

    /// The HIR type of a `class` container over `args`.
    fn py_collection_type(&mut self, class: PyCollectionClass, args: Vec<TypeId>) -> TypeId {
        let name = self.intern_name(class.class_name());
        self.intern_type(Type::Class { name, args })
    }

    /// `ty` as a container type, when it is one.
    fn py_collection_of(&mut self, ty: TypeId) -> Option<PyCollectionType> {
        let Some(Type::Class { name, args }) = self.ctx.krate.types.get(ty).cloned() else {
            return None;
        };
        let class = PyCollectionClass::from_class_name(self.ctx.krate.symbols.get(name)?)?;
        let first = *args.first()?;
        let value = match class {
            PyCollectionClass::Deque => first,
            PyCollectionClass::Counter => self.intern_type(Type::PyInt),
            PyCollectionClass::DefaultDict | PyCollectionClass::OrderedDict => *args.get(1)?,
        };
        Some(PyCollectionType { class, first, value })
    }

    /// The type a `for` loop over a container value of type `ty` binds: a
    /// deque's elements, the others' keys.
    fn py_collection_item_type(&self, ty: TypeId) -> Option<TypeId> {
        let Some(Type::Class { name, args }) = self.ctx.krate.types.get(ty) else {
            return None;
        };
        PyCollectionClass::from_class_name(self.ctx.krate.symbols.get(*name)?)?;
        args.first().copied()
    }

    /// `annotation[args]` when `value` names a container class: its type over
    /// the lowered arguments, which must be as many as the class takes.
    fn py_collection_annotation(
        &mut self,
        sub: &ruff_python_ast::ExprSubscript,
    ) -> Result<Option<TypeId>, SmeltError> {
        let Some(class) = self.py_collection_class_of(&sub.value) else {
            return Ok(None);
        };
        let elements: Vec<&Expr> = match sub.slice.as_ref() {
            Expr::Tuple(tuple) => tuple.elts.iter().collect(),
            single => vec![single],
        };
        if elements.len() != class.arity() {
            return Err(SmeltError::unsupported(
                self.span(sub.range),
                format!(
                    "`{}` takes {} type argument(s)",
                    class.python_name(),
                    class.arity()
                ),
            ));
        }
        let args = elements
            .into_iter()
            .map(|element| self.annotation_to_hir(element))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(self.py_collection_type(class, args)))
    }

    /// A container class named in annotation position without its type
    /// arguments: refused, because the element types are what make it typed.
    fn bare_py_collection_annotation(&self, annotation: &Expr) -> Option<SmeltError> {
        let class = self.py_collection_class_of(annotation)?;
        Some(SmeltError::unsupported(
            self.span(annotation.range()),
            format!(
                "`{name}` needs its type arguments in an annotation, e.g. `{name}[{example}]`",
                name = class.python_name(),
                example = if class.arity() == 1 { "int" } else { "str, int" }
            ),
        ))
    }

    /// Push one container method call typed `ty`.
    fn py_collection_op(
        &mut self,
        method: smelt_hir::PyCollectionMethod,
        receiver: smelt_hir::ExprId,
        args: Vec<smelt_hir::ExprId>,
        ty: TypeId,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        body.push_expr(HirExpr { kind: ExprKind::PyCollectionOp { method, receiver, args }, ty, span })
    }

    /// Lower `expr` and require it to have type `expected`, naming `what` in
    /// the refusal.
    fn py_collection_operand(
        &mut self,
        expr: &Expr,
        expected: TypeId,
        what: &str,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let value = self.expression_with_hint(expr, body, Some(expected))?;
        if Self::expr_ty(body, value) != expected {
            return Err(SmeltError::unsupported(
                self.span(expr.range()),
                format!("{what} must have the container's declared type"),
            ));
        }
        Ok(value)
    }

    /// `iterable` as a `list[item]` (a list as it is, a `str`'s characters, a
    /// set's members, a dict's or container's keys, a homogeneous tuple's
    /// elements), with `item` its element type when the caller knows it.
    fn py_collection_items_list(
        &mut self,
        iterable: &Expr,
        item: Option<TypeId>,
        body: &mut Body,
    ) -> Result<(smelt_hir::ExprId, TypeId), SmeltError> {
        let span = self.span(iterable.range());
        let hint = item.map(|item| self.intern_type(Type::List(item)));
        let value = self.expression_with_hint(iterable, body, hint)?;
        let value = self.iterable_as_list(value, body);
        let value_ty = Self::expr_ty(body, value);
        let list = match self.ctx.krate.types.get(value_ty).cloned() {
            Some(Type::List(_)) => value,
            Some(Type::Tuple(items))
                if items.first().is_some_and(|first| items.iter().all(|other| other == first)) =>
            {
                let ty = self.intern_type(Type::List(items[0]));
                body.push_expr(HirExpr { kind: ExprKind::TupleToList { tuple: value }, ty, span })
            }
            _ => {
                return Err(SmeltError::unsupported(
                    span,
                    "a collections container is built from a list, tuple, set, str, dict or container",
                ));
            }
        };
        let Some(Type::List(element)) = self.ctx.krate.types.get(Self::expr_ty(body, list)).cloned() else {
            return Err(SmeltError::unsupported(span, "a collections container is built from an iterable"));
        };
        if let Some(expected) = item
            && element != expected
        {
            return Err(SmeltError::unsupported(
                span,
                "the iterable's items must have the container's declared element type",
            ));
        }
        Ok((list, element))
    }

    /// An empty `list[item]` literal.
    fn py_collection_empty_list(&mut self, item: TypeId, span: Span, body: &mut Body) -> smelt_hir::ExprId {
        let ty = self.intern_type(Type::List(item));
        body.push_expr(HirExpr { kind: ExprKind::ListLit(Vec::new()), ty, span })
    }

    /// The `(key, value)` pairs a mapping argument holds, as a list: a dict's
    /// entries, a container's items, or a list of pairs.
    fn py_collection_pairs_list(
        &mut self,
        mapping: &Expr,
        key: Option<TypeId>,
        value: Option<TypeId>,
        body: &mut Body,
    ) -> Result<(smelt_hir::ExprId, TypeId, TypeId), SmeltError> {
        let span = self.span(mapping.range());
        let hint = match (key, value) {
            (Some(key), Some(value)) => Some(self.intern_type(Type::Dict(key, value))),
            _ => None,
        };
        let lowered = self.expression_with_hint(mapping, body, hint)?;
        let lowered_ty = Self::expr_ty(body, lowered);
        let pairs = match self.ctx.krate.types.get(lowered_ty).cloned() {
            Some(Type::Dict(dict_key, dict_value)) => {
                let entry = self.intern_type(Type::Tuple(vec![dict_key, dict_value]));
                let ty = self.intern_type(Type::List(entry));
                body.push_expr(HirExpr {
                    kind: ExprKind::DictProjection { op: DictProjectionOp::Entries, dict: lowered },
                    ty,
                    span,
                })
            }
            Some(Type::List(_)) => lowered,
            _ => match self.py_collection_of(lowered_ty) {
                Some(container) if container.class != PyCollectionClass::Deque => {
                    let entry = self.intern_type(Type::Tuple(vec![container.first, container.value]));
                    let ty = self.intern_type(Type::List(entry));
                    self.py_collection_op(smelt_hir::PyCollectionMethod::Items, lowered, Vec::new(), ty, span, body)
                }
                _ => {
                    return Err(SmeltError::unsupported(
                        span,
                        "a mapping container is built from a dict, a list of pairs, or another mapping",
                    ));
                }
            },
        };
        let pair_ty = match self.ctx.krate.types.get(Self::expr_ty(body, pairs)).cloned() {
            Some(Type::List(pair)) => self.ctx.krate.types.get(pair).cloned(),
            _ => None,
        };
        let Some(Type::Tuple(pair)) = pair_ty else {
            return Err(SmeltError::unsupported(span, "a mapping container is built from (key, value) pairs"));
        };
        let [pair_key, pair_value] = pair.as_slice() else {
            return Err(SmeltError::unsupported(span, "a mapping container is built from (key, value) pairs"));
        };
        if key.is_some_and(|key| key != *pair_key) || value.is_some_and(|value| value != *pair_value) {
            return Err(SmeltError::unsupported(
                span,
                "the mapping's keys and values must have the container's declared types",
            ));
        }
        Ok((pairs, *pair_key, *pair_value))
    }

    /// The container type a constructor call is expected to build, from its
    /// context's type hint, when that hint is the same class.
    fn py_collection_hint(&mut self, class: PyCollectionClass, hint: Option<TypeId>) -> Option<PyCollectionType> {
        let container = self.py_collection_of(hint?)?;
        (container.class == class).then_some(container)
    }

    /// Lower a call that builds a container, or calls one of its methods.
    fn py_collection_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
        type_hint: Option<TypeId>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if let Some(class) = self.py_collection_class_of(&call.func) {
            return self.py_collection_new(class, call, body, type_hint).map(Some);
        }
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        // A receiver with no static name is accepted when it is itself a
        // container construction (`deque([1]).index(5)`): lowered once, here.
        let (receiver_ty, prelowered) = match self.static_receiver_type(&attr.value, body) {
            Some(ty) => (ty, None),
            None => match attr.value.as_ref() {
                Expr::Call(inner) if self.py_collection_class_of(&inner.func).is_some() => {
                    let lowered = self.expression(&attr.value, body)?;
                    (Self::expr_ty(body, lowered), Some(lowered))
                }
                _ => return Ok(None),
            },
        };
        let Some(container) = self.py_collection_of(receiver_ty) else {
            return Ok(None);
        };
        self.py_collection_method(&container, attr, call, prelowered, body).map(Some)
    }

    /// `deque(..)`, `Counter(..)`, `defaultdict(..)` or `OrderedDict(..)`.
    fn py_collection_new(
        &mut self,
        class: PyCollectionClass,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
        type_hint: Option<TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(call.range());
        let expected = self.py_collection_hint(class, type_hint);
        let args = call.arguments.args.as_ref();
        if args.iter().any(|arg| matches!(arg, Expr::Starred(_))) {
            return Err(SmeltError::unsupported(span, format!("`{}(*args)` is not supported", class.python_name())));
        }
        let annotate = || {
            SmeltError::unsupported(
                span,
                format!(
                    "`{name}(..)` needs its element types: annotate the target, e.g. `x: {name}[{example}] = {name}(..)`",
                    name = class.python_name(),
                    example = if class.arity() == 1 { "int" } else { "str, int" }
                ),
            )
        };
        match class {
            PyCollectionClass::Deque => {
                let mut maxlen_expr = None;
                for keyword in &call.arguments.keywords {
                    match keyword.arg.as_ref().map(ruff_python_ast::Identifier::as_str) {
                        Some("maxlen") => maxlen_expr = Some(&keyword.value),
                        _ => return Err(SmeltError::unsupported(span, "deque() takes an iterable and `maxlen`")),
                    }
                }
                if args.len() > 2 || (args.len() == 2 && maxlen_expr.is_some()) {
                    return Err(SmeltError::unsupported(span, "deque() takes an iterable and `maxlen`"));
                }
                maxlen_expr = maxlen_expr.or_else(|| args.get(1));
                let element = expected.as_ref().map(|container| container.first);
                let (items, element) = match args.first() {
                    Some(iterable) => self.py_collection_items_list(iterable, element, body)?,
                    None => {
                        let element = element.ok_or_else(annotate)?;
                        (self.py_collection_empty_list(element, span, body), element)
                    }
                };
                let maxlen = match maxlen_expr {
                    Some(bound) if !matches!(bound, Expr::NoneLiteral(_)) => {
                        let int_ty = self.intern_type(Type::PyInt);
                        let bound = self.py_collection_operand(bound, int_ty, "`maxlen`", body)?;
                        self.py_int_as_size(bound, body)
                    }
                    _ => {
                        let none_ty = self.intern_type(Type::None);
                        body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::None), ty: none_ty, span })
                    }
                };
                let ty = self.py_collection_type(class, vec![element]);
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::PyCollectionNew { ctor: smelt_hir::PyCollectionCtor::Deque, args: vec![items, maxlen] },
                    ty,
                    span,
                }))
            }
            PyCollectionClass::Counter => {
                if !call.arguments.keywords.is_empty() || args.len() > 1 {
                    return Err(SmeltError::unsupported(span, "Counter() takes one iterable or mapping"));
                }
                let key = expected.as_ref().map(|container| container.first);
                let (ctor, operand, key) = match args.first() {
                    None => {
                        let key = key.ok_or_else(annotate)?;
                        (smelt_hir::PyCollectionCtor::Counter, self.py_collection_empty_list(key, span, body), key)
                    }
                    Some(source @ Expr::Dict(_)) => {
                        let int_ty = self.intern_type(Type::PyInt);
                        let (pairs, key, _) = self.py_collection_pairs_list(source, key, Some(int_ty), body)?;
                        (smelt_hir::PyCollectionCtor::CounterFromCounts, pairs, key)
                    }
                    Some(source) => {
                        let (items, key) = self.py_collection_items_list(source, key, body)?;
                        (smelt_hir::PyCollectionCtor::Counter, items, key)
                    }
                };
                let ty = self.py_collection_type(class, vec![key]);
                Ok(body.push_expr(HirExpr { kind: ExprKind::PyCollectionNew { ctor, args: vec![operand] }, ty, span }))
            }
            PyCollectionClass::DefaultDict => {
                if !call.arguments.keywords.is_empty() || args.is_empty() || args.len() > 2 {
                    return Err(SmeltError::unsupported(
                        span,
                        "defaultdict() takes a default factory and an optional mapping",
                    ));
                }
                let expected_value = expected.as_ref().map(|container| container.value);
                let (factory, factory_repr, value) = self.default_factory(&args[0], expected_value, body)?;
                let (pairs, key) = match args.get(1) {
                    Some(mapping) => {
                        let key = expected.as_ref().map(|container| container.first);
                        let (pairs, key, _) = self.py_collection_pairs_list(mapping, key, Some(value), body)?;
                        (pairs, key)
                    }
                    None => {
                        let key = expected.as_ref().map(|container| container.first).ok_or_else(annotate)?;
                        let entry = self.intern_type(Type::Tuple(vec![key, value]));
                        (self.py_collection_empty_list(entry, span, body), key)
                    }
                };
                let str_ty = self.intern_type(Type::PyStr);
                let repr = body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::String(factory_repr)), ty: str_ty, span });
                let ty = self.py_collection_type(class, vec![key, value]);
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::PyCollectionNew { ctor: smelt_hir::PyCollectionCtor::DefaultDict, args: vec![factory, repr, pairs] },
                    ty,
                    span,
                }))
            }
            PyCollectionClass::OrderedDict => {
                if !call.arguments.keywords.is_empty() || args.len() > 1 {
                    return Err(SmeltError::unsupported(span, "OrderedDict() takes one mapping or list of pairs"));
                }
                let key = expected.as_ref().map(|container| container.first);
                let value = expected.as_ref().map(|container| container.value);
                let (pairs, key, value) = match args.first() {
                    Some(mapping) => self.py_collection_pairs_list(mapping, key, value, body)?,
                    None => {
                        let (Some(key), Some(value)) = (key, value) else {
                            return Err(annotate());
                        };
                        let entry = self.intern_type(Type::Tuple(vec![key, value]));
                        (self.py_collection_empty_list(entry, span, body), key, value)
                    }
                };
                let ty = self.py_collection_type(class, vec![key, value]);
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::PyCollectionNew { ctor: smelt_hir::PyCollectionCtor::OrderedDict, args: vec![pairs] },
                    ty,
                    span,
                }))
            }
        }
    }

    /// A `defaultdict`'s factory as a zero-argument closure, its `repr`, and
    /// the value type it produces.
    ///
    /// A builtin type (`list`, `int`, `set`, `str`, `float`, `dict`, `bool`)
    /// is the closure returning that type's empty value, typed by the value
    /// type the context expects (`list` needs to know its element type); a
    /// program class is its zero-argument construction; any other callable
    /// (a lambda, a function) is called with no arguments. `CPython` writes a
    /// class factory as `<class 'list'>` / `<class '__main__.C'>`; a
    /// function's `repr` carries its address, which no port reproduces, so it
    /// is written without one.
    fn default_factory(
        &mut self,
        factory: &Expr,
        expected_value: Option<TypeId>,
        body: &mut Body,
    ) -> Result<(smelt_hir::ExprId, String, TypeId), SmeltError> {
        let span = self.span(factory.range());
        let (source, repr) = match factory {
            Expr::Name(name) if !self.program_binds_name(name.id.as_str()) => {
                let empty = match name.id.as_str() {
                    "list" => "[]",
                    "int" => "0",
                    "float" => "0.0",
                    "str" => "''",
                    "set" => "set()",
                    "dict" => "{}",
                    "bool" => "False",
                    other => {
                        return Err(SmeltError::unsupported(
                            span,
                            format!("`{other}` is not a supported defaultdict factory"),
                        ));
                    }
                };
                (format!("lambda: {empty}"), format!("<class '{}'>", name.id.as_str()))
            }
            Expr::Name(name) if matches!(self.items.get(name.id.as_str()).map(|item| self.item_ref(*item)), Some(Item::Class(_))) => {
                let module = if self.is_entry { "__main__".to_owned() } else { module_name_from_path(&self.path) };
                (format!("lambda: {}()", name.id.as_str()), format!("<class '{module}.{}'>", name.id.as_str()))
            }
            Expr::Lambda(lambda) => {
                let closure = self.lambda_closure(lambda, &[], expected_value, body)?;
                return Ok((closure.expr, "<function <lambda>>".to_owned(), closure.return_ty));
            }
            Expr::Name(name) => (format!("lambda: {}()", name.id.as_str()), format!("<function {}>", name.id.as_str())),
            _ => {
                return Err(SmeltError::unsupported(
                    span,
                    "a defaultdict factory is a builtin type, a class, a function name or a lambda",
                ));
            }
        };
        let parsed = ruff_python_parser::parse_expression(&source)
            .map_err(|error| SmeltError::unsupported(span, error.to_string()))?;
        let mut expr = *parsed.into_syntax().body;
        ruff_python_ast::relocate::relocate_expr(&mut expr, factory.range());
        let Expr::Lambda(lambda) = expr else {
            return Err(SmeltError::unsupported(span, "internal: the synthesized factory is not a lambda"));
        };
        let closure = self.lambda_closure(&lambda, &[], expected_value, body)?;
        if expected_value.is_some_and(|expected| expected != closure.return_ty) {
            return Err(SmeltError::unsupported(
                span,
                "the defaultdict factory's value must have the container's declared value type",
            ));
        }
        Ok((closure.expr, repr, closure.return_ty))
    }

    /// One method call on a container value.
    #[expect(
        clippy::too_many_lines,
        reason = "one table of every container method; splitting it scatters the per-class rules"
    )]
    fn py_collection_method(
        &mut self,
        container: &PyCollectionType,
        attr: &ruff_python_ast::ExprAttribute,
        call: &ruff_python_ast::ExprCall,
        prelowered: Option<smelt_hir::ExprId>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        use smelt_hir::PyCollectionMethod as Method;
        let span = self.span(call.range());
        let class = container.class;
        let name = attr.attr.as_str();
        let args = call.arguments.args.as_ref();
        let keyword = |wanted: &str| {
            call.arguments
                .keywords
                .iter()
                .find(|keyword| keyword.arg.as_ref().is_some_and(|arg| arg.as_str() == wanted))
                .map(|keyword| &keyword.value)
        };
        let known_keywords: &[&str] = match (class, name) {
            (PyCollectionClass::OrderedDict, "popitem" | "move_to_end") => &["last"],
            _ => &[],
        };
        if call.arguments.keywords.iter().any(|keyword| {
            keyword.arg.as_ref().is_none_or(|arg| !known_keywords.contains(&arg.as_str()))
        }) || args.iter().any(|arg| matches!(arg, Expr::Starred(_)))
        {
            return Err(SmeltError::unsupported(
                span,
                format!("`{}.{name}()` does not take these arguments", class.python_name()),
            ));
        }
        let arity_error = |count: &str| {
            SmeltError::unsupported(span, format!("`{}.{name}()` takes {count}", class.python_name()))
        };
        let receiver = match prelowered {
            Some(receiver) => receiver,
            None => self.expression(&attr.value, body)?,
        };
        let receiver_ty = Self::expr_ty(body, receiver);
        let none_ty = self.intern_type(Type::None);
        let bool_ty = self.intern_type(Type::Bool);
        let int_ty = self.intern_type(Type::PyInt);
        let first = container.first;
        let value = container.value;
        let list_of = |this: &mut Self, item: TypeId| this.intern_type(Type::List(item));
        let pair_list = |this: &mut Self| {
            let pair = this.intern_type(Type::Tuple(vec![first, value]));
            this.intern_type(Type::List(pair))
        };
        // `(method, operands, result type)`, or a finished expression.
        let (method, operands, ty) = match (class, name, args.len()) {
            (PyCollectionClass::Deque, "append" | "appendleft", 1) => {
                let item = self.py_collection_operand(&args[0], first, "the appended item", body)?;
                let method = if name == "append" { Method::Append } else { Method::AppendLeft };
                (method, vec![item], none_ty)
            }
            (PyCollectionClass::Deque, "pop", 0) => (Method::DequePop, Vec::new(), first),
            (PyCollectionClass::Deque, "popleft", 0) => (Method::PopLeft, Vec::new(), first),
            (PyCollectionClass::Deque, "extend" | "extendleft", 1) => {
                let (items, _) = self.py_collection_items_list(&args[0], Some(first), body)?;
                let method = if name == "extend" { Method::Extend } else { Method::ExtendLeft };
                (method, vec![items], none_ty)
            }
            (PyCollectionClass::Deque, "rotate", 0 | 1) => {
                let steps = match args.first() {
                    Some(steps) => {
                        let steps = self.py_collection_operand(steps, int_ty, "the rotation", body)?;
                        self.py_int_as_index(steps, body)
                    }
                    None => {
                        let machine = self.intern_type(Type::Int);
                        body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Int(1)), ty: machine, span })
                    }
                };
                (Method::Rotate, vec![steps], none_ty)
            }
            (PyCollectionClass::Deque, "count" | "index" | "remove", 1) => {
                let item = self.py_collection_operand(&args[0], first, "the item", body)?;
                match name {
                    "count" => (Method::Count, vec![item], int_ty),
                    "index" => (Method::Index, vec![item], int_ty),
                    _ => (Method::Remove, vec![item], none_ty),
                }
            }
            (PyCollectionClass::Deque, "reverse", 0) => (Method::Reverse, Vec::new(), none_ty),
            (_, "clear", 0) => (Method::Clear, Vec::new(), none_ty),
            (_, "copy", 0) => (Method::Copy, Vec::new(), receiver_ty),
            (PyCollectionClass::Counter, "update" | "subtract", 1) => {
                let source = &args[0];
                let source_ty = self.static_receiver_type(source, body);
                if source_ty == Some(receiver_ty) {
                    let other = self.expression(source, body)?;
                    let method = if name == "update" { Method::UpdateCounts } else { Method::SubtractCounts };
                    (method, vec![other], none_ty)
                } else {
                    let (items, _) = self.py_collection_items_list(source, Some(first), body)?;
                    let method = if name == "update" { Method::Update } else { Method::Subtract };
                    (method, vec![items], none_ty)
                }
            }
            (PyCollectionClass::Counter, "most_common", 0 | 1) => {
                let limit = match args.first() {
                    Some(limit) => {
                        let limit = self.py_collection_operand(limit, int_ty, "the count", body)?;
                        self.py_int_as_size(limit, body)
                    }
                    None => body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::None), ty: none_ty, span }),
                };
                let ty = pair_list(self);
                (Method::MostCommon, vec![limit], ty)
            }
            (PyCollectionClass::Counter, "elements", 0) => {
                let ty = list_of(self, first);
                (Method::Elements, Vec::new(), ty)
            }
            (PyCollectionClass::Counter, "total", 0) => (Method::Total, Vec::new(), int_ty),
            (PyCollectionClass::Counter | PyCollectionClass::DefaultDict | PyCollectionClass::OrderedDict, "keys", 0) => {
                let ty = list_of(self, first);
                (Method::Keys, Vec::new(), ty)
            }
            (PyCollectionClass::Counter | PyCollectionClass::DefaultDict | PyCollectionClass::OrderedDict, "values", 0) => {
                let ty = list_of(self, value);
                (Method::Values, Vec::new(), ty)
            }
            (PyCollectionClass::Counter | PyCollectionClass::DefaultDict | PyCollectionClass::OrderedDict, "items", 0) => {
                let ty = pair_list(self);
                (Method::Items, Vec::new(), ty)
            }
            (_, "get", 1 | 2) if class != PyCollectionClass::Deque => {
                let key = self.py_collection_operand(&args[0], first, "the key", body)?;
                match args.get(1) {
                    Some(default) => {
                        let default = self.py_collection_operand(default, value, "the default", body)?;
                        (Method::GetOr, vec![key, default], value)
                    }
                    None if class == PyCollectionClass::Counter => {
                        // `Counter.get(k)` is `dict.get`: `None` when missing,
                        // unlike `c[k]`.
                        let ty = smelt_hir::type_normalize::optional_of(&mut self.ctx.krate.types, value);
                        let present = self.py_collection_op(Method::Contains, receiver, vec![key], bool_ty, span, body);
                        let count = self.py_collection_op(Method::Get, receiver, vec![key], value, span, body);
                        let none = body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::None), ty: none_ty, span });
                        return Ok(body.push_expr(HirExpr {
                            kind: ExprKind::Conditional { cond: present, then_expr: count, else_expr: none },
                            ty,
                            span,
                        }));
                    }
                    None => {
                        let ty = smelt_hir::type_normalize::optional_of(&mut self.ctx.krate.types, value);
                        (Method::Get, vec![key], ty)
                    }
                }
            }
            (_, "pop", 1 | 2) if class != PyCollectionClass::Deque => {
                let key = self.py_collection_operand(&args[0], first, "the key", body)?;
                match args.get(1) {
                    Some(default) if class.is_mapping() => {
                        let default = self.py_collection_operand(default, value, "the default", body)?;
                        (Method::PopOr, vec![key, default], value)
                    }
                    Some(_) => return Err(arity_error("one key")),
                    None => (Method::MappingPop, vec![key], value),
                }
            }
            (PyCollectionClass::DefaultDict | PyCollectionClass::OrderedDict, "setdefault", 2) => {
                let key = self.py_collection_operand(&args[0], first, "the key", body)?;
                let default = self.py_collection_operand(&args[1], value, "the default", body)?;
                (Method::SetDefault, vec![key, default], value)
            }
            (PyCollectionClass::OrderedDict, "popitem", 0) => {
                let last = self.ordered_dict_last_flag(keyword("last"), span, body)?;
                let ty = self.intern_type(Type::Tuple(vec![first, value]));
                (Method::PopItem, vec![last], ty)
            }
            (PyCollectionClass::OrderedDict, "move_to_end", 1) => {
                let key = self.py_collection_operand(&args[0], first, "the key", body)?;
                let last = self.ordered_dict_last_flag(keyword("last"), span, body)?;
                (Method::MoveToEnd, vec![key, last], none_ty)
            }
            _ => {
                return Err(SmeltError::unsupported(
                    span,
                    format!("`{}.{name}()` with {} argument(s) is not supported", class.python_name(), args.len()),
                ));
            }
        };
        Ok(self.py_collection_op(method, receiver, operands, ty, span, body))
    }

    /// The `last=` flag of `OrderedDict.popitem`/`move_to_end`, `True` when
    /// not given.
    fn ordered_dict_last_flag(
        &mut self,
        last: Option<&Expr>,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let bool_ty = self.intern_type(Type::Bool);
        match last {
            Some(flag) => self.py_collection_operand(flag, bool_ty, "`last`", body),
            None => Ok(body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Bool(true)), ty: bool_ty, span })),
        }
    }

    /// `container[index]` as a read: a deque's element, a `Counter`'s count
    /// (`0` when missing, never inserted), a `defaultdict`'s value (inserting
    /// the factory's for a missing key), an `OrderedDict`'s value (`KeyError`
    /// when missing).
    fn py_collection_subscript_read(
        &mut self,
        receiver: smelt_hir::ExprId,
        index: &Expr,
        span: Span,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        use smelt_hir::PyCollectionMethod as Method;
        let Some(container) = self.py_collection_of(Self::expr_ty(body, receiver)) else {
            return Ok(None);
        };
        let (method, operand) = self.py_collection_index_operand(&container, index, body)?;
        let method = match container.class {
            PyCollectionClass::Deque => Method::GetIndex,
            PyCollectionClass::Counter => Method::Get,
            PyCollectionClass::DefaultDict => Method::GetOrInsert,
            PyCollectionClass::OrderedDict => method,
        };
        Ok(Some(self.py_collection_op(method, receiver, vec![operand], container.value, span, body)))
    }

    /// The lowered subscript of a container: a machine position for a deque,
    /// a key of the key type for the others, with the read method of an
    /// `OrderedDict` (the only one whose read is the class's own).
    fn py_collection_index_operand(
        &mut self,
        container: &PyCollectionType,
        index: &Expr,
        body: &mut Body,
    ) -> Result<(smelt_hir::PyCollectionMethod, smelt_hir::ExprId), SmeltError> {
        if matches!(index, Expr::Slice(_)) {
            return Err(SmeltError::unsupported(
                self.span(index.range()),
                format!("slicing a `{}` raises TypeError in CPython", container.class.python_name()),
            ));
        }
        if container.class == PyCollectionClass::Deque {
            let int_ty = self.intern_type(Type::PyInt);
            let position = self.py_collection_operand(index, int_ty, "a deque index", body)?;
            return Ok((smelt_hir::PyCollectionMethod::GetIndex, self.py_int_as_index(position, body)));
        }
        let key = self.py_collection_operand(index, container.first, "the key", body)?;
        Ok((smelt_hir::PyCollectionMethod::GetItem, key))
    }

    /// `container[index] = value` as a statement, when `target` subscripts a
    /// container. Answers whether it was one.
    fn py_collection_subscript_assign(
        &mut self,
        target: &Expr,
        value: &Expr,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        let Expr::Subscript(sub) = target else {
            return Ok(false);
        };
        let Some(receiver_ty) = self.static_receiver_type(&sub.value, body) else {
            return Ok(false);
        };
        let Some(container) = self.py_collection_of(receiver_ty) else {
            return Ok(false);
        };
        let span = self.span(target.range());
        let receiver = self.expression(&sub.value, body)?;
        let (_, index) = self.py_collection_index_operand(&container, &sub.slice, body)?;
        let stored = self.py_collection_operand(value, container.value, "the stored value", body)?;
        let method = if container.class == PyCollectionClass::Deque {
            smelt_hir::PyCollectionMethod::SetIndex
        } else {
            smelt_hir::PyCollectionMethod::Set
        };
        let none_ty = self.intern_type(Type::None);
        let store = self.py_collection_op(method, receiver, vec![index, stored], none_ty, span, body);
        body.push_stmt_to_block(block, HirStmt::Expr(store));
        Ok(true)
    }

    /// `container[index] op= value`: the read the subscript's class does (a
    /// `defaultdict` inserting its default, a `Counter` reading `0`), the
    /// operator, then the store, as `CPython` runs `__getitem__` then
    /// `__setitem__`. The receiver and index are evaluated once. Answers
    /// whether the target subscripted a container.
    fn py_collection_aug_assign(
        &mut self,
        aug: &StmtAugAssign,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        let Expr::Subscript(sub) = aug.target.as_ref() else {
            return Ok(false);
        };
        let Some(receiver_ty) = self.static_receiver_type(&sub.value, body) else {
            return Ok(false);
        };
        let Some(container) = self.py_collection_of(receiver_ty) else {
            return Ok(false);
        };
        let span = self.span(aug.range);
        let receiver = self.expression(&sub.value, body)?;
        let receiver = self.py_collection_temporary(receiver, span, body, block);
        let (_, index) = self.py_collection_index_operand(&container, &sub.slice, body)?;
        let index = self.py_collection_temporary(index, span, body, block);
        let Some(current) = self.py_collection_subscript_read_lowered(&container, receiver, index, span, body) else {
            return Ok(false);
        };
        let rhs = self.expression(&aug.value, body)?;
        let op = Self::python_arithmetic_op(aug.op).ok_or_else(|| {
            SmeltError::unsupported(span, format!("augmented assignment operator '{:?}' is not supported", aug.op))
        })?;
        let updated = self.python_arithmetic(op, current, rhs, span, body)?;
        if Self::expr_ty(body, updated) != container.value {
            return Err(SmeltError::unsupported(
                span,
                "the updated value must keep the container's declared value type",
            ));
        }
        let method = if container.class == PyCollectionClass::Deque {
            smelt_hir::PyCollectionMethod::SetIndex
        } else {
            smelt_hir::PyCollectionMethod::Set
        };
        let none_ty = self.intern_type(Type::None);
        let store = self.py_collection_op(method, receiver, vec![index, updated], none_ty, span, body);
        body.push_stmt_to_block(block, HirStmt::Expr(store));
        Ok(true)
    }

    /// The read `container[index]` does, over already lowered operands.
    fn py_collection_subscript_read_lowered(
        &mut self,
        container: &PyCollectionType,
        receiver: smelt_hir::ExprId,
        index: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        use smelt_hir::PyCollectionMethod as Method;
        let method = match container.class {
            PyCollectionClass::Deque => Method::GetIndex,
            PyCollectionClass::Counter => Method::Get,
            PyCollectionClass::DefaultDict => Method::GetOrInsert,
            PyCollectionClass::OrderedDict => Method::GetItem,
        };
        Some(self.py_collection_op(method, receiver, vec![index], container.value, span, body))
    }

    /// `value` bound once to a fresh immutable temporary in `block`, read back.
    fn py_collection_temporary(
        &mut self,
        value: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> smelt_hir::ExprId {
        let ty = Self::expr_ty(body, value);
        let local = body.push_local(LocalDecl { name: None, ty, mutable: false, span });
        let pat = body.push_pattern(HirPattern::Binding(local));
        body.push_stmt_to_block(block, HirStmt::Let { pat, ty, value: Some(value) });
        body.push_expr(HirExpr { kind: ExprKind::Local(local), ty, span })
    }

    /// `del container[key]` as a statement. Answers whether `sub` subscripted
    /// a container. A deque position is refused: `CPython` supports it, but
    /// a keyed delete is what the mappings and `Counter` are used for.
    fn py_collection_delete(
        &mut self,
        sub: &ruff_python_ast::ExprSubscript,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        let Some(receiver_ty) = self.static_receiver_type(&sub.value, body) else {
            return Ok(false);
        };
        let Some(container) = self.py_collection_of(receiver_ty) else {
            return Ok(false);
        };
        let span = self.span(sub.range);
        if container.class == PyCollectionClass::Deque {
            return Err(SmeltError::unsupported(span, "`del d[i]` on a deque is not supported; use `d.remove(x)`"));
        }
        let receiver = self.expression(&sub.value, body)?;
        let (_, key) = self.py_collection_index_operand(&container, &sub.slice, body)?;
        let method = if container.class == PyCollectionClass::Counter {
            smelt_hir::PyCollectionMethod::CounterDelete
        } else {
            smelt_hir::PyCollectionMethod::MappingDelete
        };
        let none_ty = self.intern_type(Type::None);
        let delete = self.py_collection_op(method, receiver, vec![key], none_ty, span, body);
        body.push_stmt_to_block(block, HirStmt::Expr(delete));
        Ok(true)
    }

    /// A container as the list a loop or a materializing builtin walks: a
    /// deque's elements, the others' keys, in order.
    fn py_collection_iteration_list(
        &mut self,
        iter: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let container = self.py_collection_of(Self::expr_ty(body, iter))?;
        let ty = self.intern_type(Type::List(container.first));
        let method = if container.class == PyCollectionClass::Deque {
            smelt_hir::PyCollectionMethod::Items
        } else {
            smelt_hir::PyCollectionMethod::Keys
        };
        Some(self.py_collection_op(method, iter, Vec::new(), ty, span, body))
    }

    /// `len(container)`, a machine integer.
    fn py_collection_len(&mut self, operand: smelt_hir::ExprId, span: Span, body: &mut Body) -> Option<smelt_hir::ExprId> {
        self.py_collection_of(Self::expr_ty(body, operand))?;
        let ty = self.intern_type(Type::Int);
        Some(self.py_collection_op(smelt_hir::PyCollectionMethod::Len, operand, Vec::new(), ty, span, body))
    }

    /// `needle in container`: a deque's element test, the others' key test.
    fn py_collection_contains(
        &mut self,
        needle: smelt_hir::ExprId,
        haystack: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(container) = self.py_collection_of(Self::expr_ty(body, haystack)) else {
            return Ok(None);
        };
        if Self::expr_ty(body, needle) != container.first {
            return Err(SmeltError::unsupported(
                span,
                format!("`in` on a `{}` needs an operand of its element or key type", container.class.python_name()),
            ));
        }
        let ty = self.intern_type(Type::Bool);
        Ok(Some(self.py_collection_op(smelt_hir::PyCollectionMethod::Contains, haystack, vec![needle], ty, span, body)))
    }

    /// `a == b` / `a != b` of two containers of one type: the class's own
    /// equality (order-sensitive for a deque and an `OrderedDict`, a missing
    /// `Counter` key counting `0`).
    fn py_collection_equality(
        &mut self,
        negated: bool,
        lhs: smelt_hir::ExprId,
        rhs: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let ty = Self::expr_ty(body, lhs);
        if ty != Self::expr_ty(body, rhs) {
            return None;
        }
        self.py_collection_of(ty)?;
        let bool_ty = self.intern_type(Type::Bool);
        let equal = self.py_collection_op(smelt_hir::PyCollectionMethod::Equals, lhs, vec![rhs], bool_ty, span, body);
        if !negated {
            return Some(equal);
        }
        Some(body.push_expr(HirExpr { kind: ExprKind::UnaryOp { op: UnaryOp::Not, operand: equal }, ty: bool_ty, span }))
    }

    /// `c1 + c2` / `c1 - c2` of two `Counter`s of one type: a new counter
    /// keeping only positive counts.
    fn py_counter_arithmetic(
        &mut self,
        op: PythonArithmetic,
        lhs: smelt_hir::ExprId,
        rhs: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let ty = Self::expr_ty(body, lhs);
        if ty != Self::expr_ty(body, rhs) || self.py_collection_of(ty)?.class != PyCollectionClass::Counter {
            return None;
        }
        let method = match op {
            PythonArithmetic::Plain(BinOp::Add) => smelt_hir::PyCollectionMethod::Plus,
            PythonArithmetic::Plain(BinOp::Sub) => smelt_hir::PyCollectionMethod::Minus,
            _ => return None,
        };
        Some(self.py_collection_op(method, lhs, vec![rhs], ty, span, body))
    }

    /// `d.maxlen` of a deque: its bound, or `None`.
    fn deque_maxlen_expression(
        &mut self,
        attr: &ruff_python_ast::ExprAttribute,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if attr.attr.as_str() != "maxlen" {
            return Ok(None);
        }
        let Some(receiver_ty) = self.static_receiver_type(&attr.value, body) else {
            return Ok(None);
        };
        if self.py_collection_of(receiver_ty).is_none_or(|container| container.class != PyCollectionClass::Deque) {
            return Ok(None);
        }
        let span = self.span(attr.range);
        let receiver = self.expression(&attr.value, body)?;
        let int_ty = self.intern_type(Type::PyInt);
        let ty = smelt_hir::type_normalize::optional_of(&mut self.ctx.krate.types, int_ty);
        Ok(Some(self.py_collection_op(smelt_hir::PyCollectionMethod::MaxLen, receiver, Vec::new(), ty, span, body)))
    }

    /// `dict(container)` of a `defaultdict` or `OrderedDict`: a plain dict of
    /// its entries.
    fn py_collection_to_dict(&mut self, value: smelt_hir::ExprId, span: Span, body: &mut Body) -> Option<smelt_hir::ExprId> {
        let container = self.py_collection_of(Self::expr_ty(body, value))?;
        if !container.class.is_mapping() {
            return None;
        }
        let ty = self.intern_type(Type::Dict(container.first, container.value));
        Some(self.py_collection_op(smelt_hir::PyCollectionMethod::ToDict, value, Vec::new(), ty, span, body))
    }
}
