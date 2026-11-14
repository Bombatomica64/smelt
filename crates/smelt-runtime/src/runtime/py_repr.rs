// Python's `repr`, for what Python's `print` shows inside a container, and
// Python's `json.dumps` text (the same per-type dispatch, see below).
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
    //~ The text `repr(self)` answers.
    fn py_repr(&self) -> String;
    //~ The text `str(self)` answers, which is the `repr` for every value but a
    //~ string and an exception.
    fn py_str(&self) -> String { self.py_repr() }
}

impl SmeltPyRepr for i64 { fn py_repr(&self) -> String { self.to_string() } }
impl SmeltPyRepr for f64 { fn py_repr(&self) -> String { smelt_py_float_repr(*self) } }
impl SmeltPyRepr for bool { fn py_repr(&self) -> String { if *self { "True".to_owned() } else { "False".to_owned() } } }
impl SmeltPyRepr for () { fn py_repr(&self) -> String { "None".to_owned() } }
impl<T: SmeltPyRepr> SmeltPyRepr for Option<T> { fn py_repr(&self) -> String { self.as_ref().map_or_else(|| "None".to_owned(), SmeltPyRepr::py_repr) } }
impl<T: SmeltPyRepr + ?Sized> SmeltPyRepr for ::std::rc::Rc<T> { fn py_repr(&self) -> String { (**self).py_repr() } }

impl SmeltPyRepr for String {
    fn py_str(&self) -> String { self.clone() }
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

// @smelt:if record
impl<K: SmeltPyRepr + Eq + ::std::hash::Hash + Clone + SmeltPropertyKey, V: SmeltPyRepr + Clone> SmeltPyRepr for SmeltRecord<K, V> {
    fn py_repr(&self) -> String {
        format!("{{{}}}", self.iter().map(|(key, value)| format!("{}: {}", key.py_repr(), value.py_repr())).collect::<Vec<_>>().join(", "))
    }
}
// @smelt:end
// @smelt:if dep_indexmap
impl<K: SmeltPyRepr, V: SmeltPyRepr> SmeltPyRepr for ::indexmap::IndexMap<K, V> {
    fn py_repr(&self) -> String {
        format!("{{{}}}", self.iter().map(|(key, value)| format!("{}: {}", key.py_repr(), value.py_repr())).collect::<Vec<_>>().join(", "))
    }
}
// @smelt:end

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
        let items = smelt_py_repr_items(self.store.borrow().entries.iter());
        if items.is_empty() { "set()".to_owned() } else { format!("{{{items}}}") }
    }
}
// @smelt:end

// Python's `json.dumps(value)` with its default arguments, by type.
//
// CPython's encoder differs from `JSON.stringify` in every default: items are
// separated by `", "` and keys by `": "`, a float keeps its `repr` (`2.0`, not
// `2`), a non-finite float is `NaN`/`Infinity`/`-Infinity`, and
// `ensure_ascii=True` escapes everything outside printable ASCII as `\uXXXX`
// (a surrogate pair above the BMP). The value's static Python type decides the
// form, so an `int` and a `float` stay distinct where an erased number could
// not tell them apart.

/// Python's `json.dumps` encoding of a value.
#[allow(dead_code)]
trait SmeltPyJson {
    //~ Append the JSON text of `self` to `out`.
    fn py_json(&self, out: &mut String);
}

/// A dict key `json.dumps` accepts: its JSON object key text, quoted.
#[allow(dead_code)]
trait SmeltPyJsonKey {
    //~ Append the quoted key `json.dumps` writes for `self`.
    fn py_json_key(&self, out: &mut String);
}

/// `json.dumps(value)` with the default arguments.
#[allow(dead_code)]
fn smelt_py_json_dumps<T: SmeltPyJson + ?Sized>(value: &T) -> String {
    let mut out = String::new();
    value.py_json(&mut out);
    out
}

/// Append `text` as a JSON string the way `CPython`'s `ensure_ascii` encoder
/// writes it.
#[allow(dead_code)]
fn smelt_py_json_string(text: &str, out: &mut String) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(ch),
            ch => {
                let mut units = [0_u16; 2];
                for unit in ch.encode_utf16(&mut units) {
                    let _ = ::std::fmt::Write::write_fmt(out, format_args!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
}

/// Append the items of a sequence as `[a, b]`.
#[allow(dead_code)]
fn smelt_py_json_items<'a, T: SmeltPyJson + 'a>(items: impl IntoIterator<Item = &'a T>, out: &mut String) {
    out.push('[');
    for (index, item) in items.into_iter().enumerate() {
        if index > 0 { out.push_str(", "); }
        item.py_json(out);
    }
    out.push(']');
}

/// Append the entries of a dict as `{"k": v}`.
#[allow(dead_code)]
fn smelt_py_json_entries<'a, K: SmeltPyJsonKey + 'a, V: SmeltPyJson + 'a>(entries: impl IntoIterator<Item = (&'a K, &'a V)>, out: &mut String) {
    out.push('{');
    for (index, (key, value)) in entries.into_iter().enumerate() {
        if index > 0 { out.push_str(", "); }
        key.py_json_key(out);
        out.push_str(": ");
        value.py_json(out);
    }
    out.push('}');
}

/// A float the way `json.dumps` writes it: its `repr`, or `CPython`'s spelling
/// of a non-finite value.
#[allow(dead_code)]
fn smelt_py_json_float(value: f64) -> String {
    if value.is_nan() { "NaN".to_owned() }
    else if value.is_infinite() { if value > 0.0 { "Infinity".to_owned() } else { "-Infinity".to_owned() } }
    else { smelt_py_float_repr(value) }
}

impl SmeltPyJson for i64 { fn py_json(&self, out: &mut String) { out.push_str(&self.to_string()); } }
impl SmeltPyJson for f64 { fn py_json(&self, out: &mut String) { out.push_str(&smelt_py_json_float(*self)); } }
impl SmeltPyJson for bool { fn py_json(&self, out: &mut String) { out.push_str(if *self { "true" } else { "false" }); } }
impl SmeltPyJson for () { fn py_json(&self, out: &mut String) { out.push_str("null"); } }
impl SmeltPyJson for String { fn py_json(&self, out: &mut String) { smelt_py_json_string(self, out); } }
impl<T: SmeltPyJson> SmeltPyJson for Option<T> { fn py_json(&self, out: &mut String) { match self { Some(value) => value.py_json(out), None => out.push_str("null") } } }
impl<T: SmeltPyJson + ?Sized> SmeltPyJson for ::std::rc::Rc<T> { fn py_json(&self, out: &mut String) { (**self).py_json(out); } }
impl<T: SmeltPyJson> SmeltPyJson for Vec<T> { fn py_json(&self, out: &mut String) { smelt_py_json_items(self, out); } }

impl SmeltPyJsonKey for String { fn py_json_key(&self, out: &mut String) { smelt_py_json_string(self, out); } }
impl SmeltPyJsonKey for i64 { fn py_json_key(&self, out: &mut String) { out.push('"'); out.push_str(&self.to_string()); out.push('"'); } }
impl SmeltPyJsonKey for f64 { fn py_json_key(&self, out: &mut String) { out.push('"'); out.push_str(&smelt_py_json_float(*self)); out.push('"'); } }
impl SmeltPyJsonKey for bool { fn py_json_key(&self, out: &mut String) { out.push_str(if *self { "\"true\"" } else { "\"false\"" }); } }

// @smelt:if record
impl<K: SmeltPyJsonKey + Eq + ::std::hash::Hash + Clone + SmeltPropertyKey, V: SmeltPyJson + Clone> SmeltPyJson for SmeltRecord<K, V> {
    fn py_json(&self, out: &mut String) {
        let store = self.store.borrow();
        smelt_py_json_entries(store.entries().iter().map(|entry| (&entry.key, &entry.value)), out);
    }
}
// @smelt:end
// @smelt:if dep_indexmap
impl<K: SmeltPyJsonKey, V: SmeltPyJson> SmeltPyJson for ::indexmap::IndexMap<K, V> { fn py_json(&self, out: &mut String) { smelt_py_json_entries(self.iter(), out); } }
// @smelt:end

/// A tuple is a JSON array.
macro_rules! smelt_py_json_tuple {
    ($($name:ident),+) => {
        impl<$($name: SmeltPyJson),+> SmeltPyJson for ($($name,)+) {
            #[allow(non_snake_case)]
            fn py_json(&self, out: &mut String) {
                let ($($name,)+) = self;
                out.push('[');
                let mut first = true;
                $(
                    if !first { out.push_str(", "); }
                    first = false;
                    $name.py_json(out);
                )+
                let _ = first;
                out.push(']');
            }
        }
    };
}
smelt_py_json_tuple!(A);
smelt_py_json_tuple!(A, B);
smelt_py_json_tuple!(A, B, C);
smelt_py_json_tuple!(A, B, C, D);
smelt_py_json_tuple!(A, B, C, D, E);
smelt_py_json_tuple!(A, B, C, D, E, F);

// @smelt:if smelt_list
impl<T: SmeltPyJson> SmeltPyJson for SmeltList<T> { fn py_json(&self, out: &mut String) { smelt_py_json_items(self.borrow().iter(), out); } }
// @smelt:end

// @smelt:item-end
