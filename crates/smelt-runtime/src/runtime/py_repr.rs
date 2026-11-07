// Python's `repr`, for what Python's `print` shows inside a container.
//
// `print(x)` writes `str(x)`, and `str` of a list, tuple, dict or set is
// built from the `repr` of each element: strings quoted (`'a'`), booleans
// `True`/`False`, `None`, floats as `repr(float)`. Rust's `Debug` spells all
// of those differently (`"a"`, `true`, `Some(..)`), so the emitter routes a
// printed container through this trait instead.

// @smelt:item SmeltPyRepr when py_repr
/// Python's `repr` of a value.
#[allow(dead_code)]
trait SmeltPyRepr {
    /// The text `repr(self)` answers.
    fn py_repr(&self) -> String;
}

impl SmeltPyRepr for i64 { fn py_repr(&self) -> String { self.to_string() } }
impl SmeltPyRepr for f64 { fn py_repr(&self) -> String { smelt_py_float_repr(*self) } }
impl SmeltPyRepr for bool { fn py_repr(&self) -> String { if *self { "True".to_owned() } else { "False".to_owned() } } }
impl SmeltPyRepr for () { fn py_repr(&self) -> String { "None".to_owned() } }
impl<T: SmeltPyRepr> SmeltPyRepr for Option<T> { fn py_repr(&self) -> String { self.as_ref().map_or_else(|| "None".to_owned(), SmeltPyRepr::py_repr) } }
impl<T: SmeltPyRepr + ?Sized> SmeltPyRepr for ::std::rc::Rc<T> { fn py_repr(&self) -> String { (**self).py_repr() } }

impl SmeltPyRepr for String {
    fn py_repr(&self) -> String {
        //~ Single quotes, unless the text holds a `'` and no `"`.
        let quote = if self.contains('\'') && !self.contains('"') { '"' } else { '\'' };
        let mut out = Self::with_capacity(self.len() + 2);
        out.push(quote);
        for ch in self.chars() {
            match ch {
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                ch if ch == quote => { out.push('\\'); out.push(ch); }
                ch if (ch as u32) < 0x20 || ch as u32 == 0x7f => { let code = u32::from(ch); out.push_str("\\x"); out.extend([code >> 4, code & 0xf].map(|digit| char::from_digit(digit, 16).unwrap_or('0'))); }
                ch => out.push(ch),
            }
        }
        out.push(quote);
        out
    }
}

/// `[a, b]` from the `repr` of each item.
#[allow(dead_code)]
fn smelt_py_repr_items<'a, T: SmeltPyRepr + 'a>(items: impl IntoIterator<Item = &'a T>) -> String {
    items.into_iter().map(SmeltPyRepr::py_repr).collect::<Vec<_>>().join(", ")
}

impl<T: SmeltPyRepr> SmeltPyRepr for Vec<T> { fn py_repr(&self) -> String { format!("[{}]", smelt_py_repr_items(self)) } }

impl<K: SmeltPyRepr, V: SmeltPyRepr> SmeltPyRepr for ::std::collections::HashMap<K, V> {
    fn py_repr(&self) -> String {
        format!("{{{}}}", self.iter().map(|(key, value)| format!("{}: {}", key.py_repr(), value.py_repr())).collect::<Vec<_>>().join(", "))
    }
}

/// A tuple's `repr`: `(a,)` for one element, `(a, b)` for more.
macro_rules! smelt_py_repr_tuple {
    ($($name:ident),+) => {
        impl<$($name: SmeltPyRepr),+> SmeltPyRepr for ($($name,)+) {
            #[allow(non_snake_case)]
            fn py_repr(&self) -> String {
                let ($($name,)+) = self;
                let items = [$($name.py_repr()),+];
                if items.len() == 1 { format!("({},)", items[0]) } else { format!("({})", items.join(", ")) }
            }
        }
    };
}
smelt_py_repr_tuple!(A);
smelt_py_repr_tuple!(A, B);
smelt_py_repr_tuple!(A, B, C);
smelt_py_repr_tuple!(A, B, C, D);
smelt_py_repr_tuple!(A, B, C, D, E);
smelt_py_repr_tuple!(A, B, C, D, E, F);

// @smelt:if smelt_list
impl<T: SmeltPyRepr> SmeltPyRepr for SmeltList<T> { fn py_repr(&self) -> String { format!("[{}]", smelt_py_repr_items(self.borrow().iter())) } }
// @smelt:end
// @smelt:if prim_set
impl<T: SmeltPyRepr> SmeltPyRepr for SmeltPrimSet<T> {
    fn py_repr(&self) -> String {
        let items = smelt_py_repr_items(self.iter());
        if items.is_empty() { "set()".to_owned() } else { format!("{{{items}}}") }
    }
}
// @smelt:end

// @smelt:item-end
