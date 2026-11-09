// A raised Python exception.
//
// Python's `raise` hands any instance of a `BaseException` subclass to the
// nearest `except` clause naming one of its classes. The generated crate carries
// every such instance, builtin or user-defined, as one `SmeltPyException`: the
// names of its class and of every base up to `BaseException`, which is what
// `except` matches against, and the `str`/`repr` of each constructor argument,
// which is all `str(e)`, `repr(e)` and `e.args` show. It crosses the error
// channel as a `Box<dyn Error>` and is recovered by downcasting.

// @smelt:item SmeltPyException when py_exception
/// A raised Python exception: its class chain and its constructor arguments.
/// The default is a placeholder an unassigned binding starts as: no class and
/// no arguments.
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
struct SmeltPyException {
    /// The class's own name first, then each base up to `BaseException`. Empty
    /// for an error the runtime raised without naming a Python class.
    classes: &'static [&'static str],
    /// `str` and `repr` of each constructor argument, in order.
    args: ::std::rc::Rc<[SmeltPyExceptionArg]>,
}

/// The two texts a constructor argument of an exception is ever shown as.
#[derive(Clone, Debug)]
#[allow(dead_code)]
struct SmeltPyExceptionArg {
    /// `str(arg)`.
    text: String,
    /// `repr(arg)`.
    repr: String,
}

#[allow(dead_code)]
impl SmeltPyException {
    /// An instance of the class whose chain is `classes`, built from `args`.
    fn new(classes: &'static [&'static str], args: Vec<SmeltPyExceptionArg>) -> Self {
        Self { classes, args: args.into() }
    }

    /// One constructor argument, kept as its `str` and its `repr`.
    fn arg<T: SmeltPyRepr + ?Sized>(value: &T) -> SmeltPyExceptionArg {
        SmeltPyExceptionArg { text: value.py_str(), repr: value.py_repr() }
    }

    /// The exception a caught error carries. An error raised without a Python
    /// class keeps its message as its one argument and has no class.
    fn from_error(error: &(dyn ::std::error::Error + 'static)) -> Self {
        error.downcast_ref::<Self>().cloned().unwrap_or_else(|| Self::from_message(error.to_string()))
    }

    /// The exception a caught panic carries: an error the runtime raised
    /// without a Python class, whose one argument is the panic's message.
    fn from_message(text: String) -> Self {
        let repr = text.py_repr();
        Self { classes: &[], args: vec![SmeltPyExceptionArg { text, repr }].into() }
    }

    /// Whether an `except` clause naming `classes` catches this exception.
    ///
    /// An error with no class is caught by a clause naming only builtin
    /// classes exactly when `catches_unclassified` says so: it is some builtin
    /// exception, but which one the runtime did not record.
    fn caught_by(&self, classes: &[&str], catches_unclassified: bool) -> bool {
        if self.classes.is_empty() {
            return catches_unclassified;
        }
        classes.iter().any(|class| self.classes.contains(class))
    }

    /// This exception as the error a `raise` propagates.
    fn into_error(self) -> Box<dyn ::std::error::Error> {
        Box::new(self)
    }

    /// `e.args`.
    fn args(&self) -> SmeltPyExceptionArgs {
        SmeltPyExceptionArgs(self.args.clone())
    }

    /// `repr(e.args)`: the tuple of the arguments' `repr`s.
    fn args_repr(&self) -> String {
        self.args().py_repr()
    }
}

/// `e.args` of an exception: a tuple whose elements have no static type, so
/// it is shown, and offers nothing else.
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
struct SmeltPyExceptionArgs(::std::rc::Rc<[SmeltPyExceptionArg]>);

impl SmeltPyRepr for SmeltPyExceptionArgs {
    fn py_repr(&self) -> String {
        match &*self.0 {
            [only] => format!("({},)", only.repr),
            args => format!("({})", args.iter().map(|arg| arg.repr.as_str()).collect::<Vec<_>>().join(", ")),
        }
    }
}

impl ::std::fmt::Display for SmeltPyExceptionArgs {
    fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        formatter.write_str(&self.py_repr())
    }
}

/// The `IndexError` of a sequence read past either end.
#[allow(dead_code)]
fn smelt_py_index_error(message: &str) -> Box<dyn ::std::error::Error> {
    SmeltPyException::new(&["IndexError", "LookupError", "Exception", "BaseException"], vec![SmeltPyException::arg(&message.to_owned())]).into_error()
}

/// A `ValueError` with one message argument (`min()` of an empty iterable).
#[allow(dead_code)]
fn smelt_py_value_error(message: &str) -> Box<dyn ::std::error::Error> {
    SmeltPyException::new(&["ValueError", "Exception", "BaseException"], vec![SmeltPyException::arg(&message.to_owned())]).into_error()
}

/// The positions `xs[start:end:step]` reads from a sequence of `len`
/// elements: `CPython`'s `slice.indices` clamping, then its `range`. A zero
/// step raises `ValueError`.
#[allow(dead_code)]
fn smelt_py_slice_positions(len: usize, start: Option<i64>, end: Option<i64>, step: i64) -> Result<Vec<usize>, Box<dyn ::std::error::Error>> {
    if step == 0 {
        return Err(SmeltPyException::new(&["ValueError", "Exception", "BaseException"], vec![SmeltPyException::arg(&"slice step cannot be zero".to_owned())]).into_error());
    }
    let len = i64::try_from(len).unwrap_or(i64::MAX);
    let (lower, upper) = if step < 0 { (-1, len - 1) } else { (0, len) };
    let clamp = |bound: i64| if bound < 0 { (bound + len).max(lower) } else { bound.min(upper) };
    let mut position = start.map_or(if step < 0 { upper } else { lower }, clamp);
    let limit = end.map_or(if step < 0 { lower } else { upper }, clamp);
    let mut positions = Vec::new();
    while (step > 0 && position < limit) || (step < 0 && position > limit) {
        positions.extend(usize::try_from(position).ok());
        position += step;
    }
    Ok(positions)
}

impl ::std::fmt::Display for SmeltPyException {
    /// `str(e)`: nothing for no argument, the argument's `str` for one (its
    /// `repr` for a `KeyError`, whose `__str__` says so), the arguments'
    /// tuple otherwise.
    fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match &*self.args {
            [] => Ok(()),
            [only] if self.classes.contains(&"KeyError") => formatter.write_str(&only.repr),
            [only] => formatter.write_str(&only.text),
            _ => formatter.write_str(&self.args_repr()),
        }
    }
}

impl ::std::error::Error for SmeltPyException {}

impl SmeltPyRepr for SmeltPyException {
    fn py_repr(&self) -> String {
        let name = self.classes.first().copied().unwrap_or("Exception");
        format!("{name}({})", self.args.iter().map(|arg| arg.repr.as_str()).collect::<Vec<_>>().join(", "))
    }
    fn py_str(&self) -> String { self.to_string() }
}

// @smelt:item-end
