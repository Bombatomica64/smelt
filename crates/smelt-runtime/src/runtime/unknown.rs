// `SmeltUnknown`: the erased runtime value, and its core trait impls.

// @smelt:item SmeltUnknown
// @smelt:if unknown
pub enum SmeltUnknown {
    Null,
    Undefined,
    Bool(bool),
    Number(f64),
// @smelt:end
// @smelt:if bigint
    //~ A JavaScript `bigint` keeps its own tag in the erased carrier, so
    //~ `typeof` answers `"bigint"` and the value is never rounded to
    //~ `f64`. This is the bigint face of the existing dynamic boundary
    //~ (`unknown`, `any`, unions, generic erasure), not a new one: a
    //~ bigint only arrives here through the same erasure a number does.
    BigInt(::num_bigint::BigInt),
// @smelt:end
// @smelt:if unknown
    //~ JavaScript strings are immutable, so the erased tag shares one
    //~ heap buffer across clones instead of copying it: `Rc<str>` turns
    //~ `SmeltUnknown::clone` -- and therefore every `SmeltObject::get`
    //~ property read -- into a refcount bump rather than malloc+memcpy.
    String(::std::rc::Rc<str>),
    Symbol(::std::rc::Rc<str>),
    Array(SmeltArray),
    Object(SmeltObject),
    Function(::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>),
    Promise(SmeltPromise),
}

impl Clone for SmeltUnknown {
    fn clone(&self) -> Self {
        match self {
            Self::Null => Self::Null,
            Self::Undefined => Self::Undefined,
            Self::Bool(value) => Self::Bool(*value),
            Self::Number(value) => Self::Number(*value),
// @smelt:end
// @smelt:if bigint
            Self::BigInt(value) => Self::BigInt(value.clone()),
// @smelt:end
// @smelt:if unknown
            Self::String(value) => Self::String(value.clone()),
            Self::Symbol(value) => Self::Symbol(value.clone()),
            Self::Array(values) => Self::Array(values.clone()),
            Self::Object(values) => Self::Object(values.clone()),
            Self::Function(value) => Self::Function(value.clone()),
            Self::Promise(value) => Self::Promise(value.clone()),
        }
    }
}
// @smelt:end
// @smelt:if bigint
//~ Emits the bigint runtime helpers into the generated prelude.

// @smelt:end
// @smelt:item-end

// @smelt:item impl_std_fmt_Debug_for_SmeltUnknown
// @smelt:if unknown
impl ::std::fmt::Debug for SmeltUnknown {
    fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match self {
            Self::Null => formatter.write_str("Null"),
            Self::Undefined => formatter.write_str("Undefined"),
            Self::Bool(value) => formatter.debug_tuple("Bool").field(value).finish(),
            Self::Number(value) => formatter.debug_tuple("Number").field(value).finish(),
// @smelt:end
// @smelt:if bigint
            Self::BigInt(value) => formatter.debug_tuple("BigInt").field(value).finish(),
// @smelt:end
// @smelt:if unknown
            Self::String(value) => formatter.debug_tuple("String").field(value).finish(),
            Self::Symbol(value) => formatter.debug_tuple("Symbol").field(value).finish(),
            Self::Array(values) => formatter.debug_tuple("Array").field(values).finish(),
            Self::Object(values) => formatter.debug_tuple("Object").field(values).finish(),
            Self::Function(_) => formatter.write_str("Function(<closure>)"),
            Self::Promise(value) => formatter.debug_tuple("Promise").field(value).finish(),
        }
    }
}

// @smelt:end
// @smelt:item-end

// @smelt:item impl_PartialEq_for_SmeltUnknown when unknown
impl PartialEq for SmeltUnknown {
    fn eq(&self, other: &Self) -> bool {
        let mut smelt_seen = ::std::collections::HashSet::new();
        smelt_unknown_structural_eq(self, other, &mut smelt_seen)
    }
}

//~ `Default` is what every ABSENT erased slot falls back to: an
//~ out-of-range element read, a `resize` fill, a `new Array(n)` hole.
//~ JavaScript answers `undefined` for every one of those -- reading a
//~ missing index or a hole never produces `null`, which is a value a
//~ program has to store deliberately. Defaulting to `Null` made
//~ `[1, , 2]`-shaped holes and missing reads indistinguishable from a
//~ stored `null`, so `at(['a','b','c'], [4])` compared unequal to
//~ `[undefined]`.
impl Default for SmeltUnknown {
    fn default() -> Self {
        Self::Undefined
    }
}

// @smelt:item-end

// @smelt:item impl_SmeltUnknown when unknown
impl SmeltUnknown {
    /// Returns the JavaScript-style length for unknown string, array, and object values.
    pub fn len(&self) -> usize {
        match self {
            Self::String(value) => value.chars().count(),
            Self::Array(value) => value.len(),
            Self::Object(value) => value.len(),
            /*@if bigint*/Self::BigInt(_) => 0, /*@end*/Self::Null | Self::Undefined | Self::Bool(_) | Self::Number(_) | Self::Symbol(_) | Self::Function(_) | Self::Promise(_) => 0,
        }
    }
    /// Return a JavaScript-like weekday for erased Date-compatible numeric timestamps.
    pub fn get_day(&self) -> f64 {
        let timestamp_ms = match self { Self::Number(value) => *value, Self::Object(value) => match value.get("__smelt_date") { Some(Self::Number(value)) => value, _ => return f64::NAN }, _ => return f64::NAN };
        let days_since_epoch = (timestamp_ms / 86_400_000.0).floor() as i64;
        ((days_since_epoch + 4).rem_euclid(7)) as f64
    }
    /// Return JavaScript Date.toISOString output for erased Date-compatible values.
    pub fn to_iso_string(&self) -> String {
        if let Self::Object(value) = self {
            if let (Some(Self::Number(timestamp_ms)), Some(Self::String(timezone_name))) = (value.get("__smelt_date"), value.get("__smelt_timezone")) {
                if let (Some(mut local), Ok(timezone)) = (chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp_ms as i64).map(|date| date.naive_utc()), timezone_name.parse::<chrono_tz::Tz>()) {
                    loop {
                        match chrono::TimeZone::from_local_datetime(&timezone, &local) {
                            chrono::LocalResult::Single(date) => return date.to_rfc3339_opts(chrono::SecondsFormat::Millis, false),
                            chrono::LocalResult::Ambiguous(first, _) => return first.to_rfc3339_opts(chrono::SecondsFormat::Millis, false),
                            chrono::LocalResult::None => local += chrono::Duration::minutes(1),
                        }
                    }
                }
            }
        }
        let timestamp_ms = match self {
            Self::Number(value) => *value,
            Self::Object(value) => match value.get("__smelt_date") { Some(Self::Number(value)) => value, _ => f64::NAN },
            Self::String(value) => smelt_string_to_number(value),
            Self::Bool(value) => if *value { 1.0 } else { 0.0 },
            /*@if bigint*/Self::BigInt(value) => smelt_bigint_to_number(value), /*@end*/Self::Null | Self::Undefined | Self::Symbol(_) | Self::Array(_) | Self::Function(_) | Self::Promise(_) => f64::NAN,
        };
        chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp_ms as i64).map(|date| date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)).unwrap_or_else(|| "Invalid Date".to_owned())
    }
    /// Return JavaScript-like `includes` membership for erased strings and arrays.
    pub fn includes<T: IntoSmeltUnknown>(&self, needle: T) -> bool {
        let needle = needle.into_smelt_unknown();
        match (self, needle) {
            (Self::String(haystack), Self::String(needle)) => haystack.contains(&*needle),
            (Self::Array(values), needle) => values.iter().any(|value| value == needle),
            _ => false,
        }
    }
    /// Return JavaScript-like RegExp.test behavior for erased regex-like values.
    pub fn test<T: IntoSmeltUnknown>(&self, haystack: T) -> bool {
        let SmeltUnknown::String(haystack) = haystack.into_smelt_unknown() else { return false; };
        match self {
            Self::String(pattern) => regex::Regex::new(pattern).is_ok_and(|regex| regex.is_match(&haystack)),
            Self::Object(map) => {
                let Some(Self::String(source)) = map.get("source") else { return false; };
                let flags = match map.get("flags") { Some(Self::String(flags)) => flags.to_string(), _ => String::new() };
                SmeltRegExp::new(source.to_string(), flags).test(&haystack)
            }
            _ => false,
        }
    }
    /// No-op compatibility hook for erased callable objects with a `flush` method.
    pub fn flush(&self) {
    }
}

// @smelt:item-end

// @smelt:item impl_std_fmt_Display_for_SmeltUnknown
// @smelt:if unknown
impl ::std::fmt::Display for SmeltUnknown {
    fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match self {
            Self::Null => formatter.write_str("null"),
            Self::Undefined => formatter.write_str("undefined"),
            Self::Bool(value) => write!(formatter, "{value}"),
            Self::Number(value) => formatter.write_str(&smelt_number_to_string(*value)),
            Self::String(value) => formatter.write_str(value),
            Self::Symbol(value) => formatter.write_str(value),
// @smelt:end
// @smelt:if bigint
            Self::BigInt(value) => write!(formatter, "{value}"),
// @smelt:end
// @smelt:if unknown
            Self::Array(_) | Self::Object(_) => formatter.write_str("[object Object]"),
            Self::Function(_) => formatter.write_str("function () { [native code] }"),
            Self::Promise(_) => formatter.write_str("[object Promise]"),
        }
    }
}
//~ Emits the exception-payload ABI into the generated runtime prelude.
//~
//~ Only called from inside the prelude's `needs_unknown` region: the payload is
//~ a `SmeltUnknown`, so these items are only well-formed where that enum exists.
//~ Throw sites in a program with no erased values keep the plain string
//~ `std::io::Error` form (see `FunctionEmitter::throw_terminator_text`).

// @smelt:end
// @smelt:item-end

// @smelt:item impl_Eq_for_SmeltUnknown when unknown
impl Eq for SmeltUnknown {}

impl ::std::hash::Hash for SmeltUnknown {
    fn hash<H: ::std::hash::Hasher>(&self, state: &mut H) {
        let mut smelt_seen = ::std::collections::HashSet::new();
        smelt_unknown_structural_hash(self, state, &mut smelt_seen);
    }
}

impl PartialOrd for SmeltUnknown {
    fn partial_cmp(&self, other: &Self) -> Option<::std::cmp::Ordering> {
        match (self, other) {
            (Self::Number(left), Self::Number(right)) => left.partial_cmp(right),
            (Self::String(left), Self::String(right)) => Some(left.cmp(right)),
            (Self::Bool(left), Self::Bool(right)) => Some(left.cmp(right)),
            (Self::Object(left), Self::Object(right)) if left.contains_key("__smelt_date") || right.contains_key("__smelt_date") => smelt_unknown_date_value(self).partial_cmp(&smelt_unknown_date_value(other)),
            (Self::Null, Self::Null) => Some(::std::cmp::Ordering::Equal),
            (left, right) => Some(smelt_unknown_rank(left).cmp(&smelt_unknown_rank(right))),
        }
    }
}

impl PartialEq<f64> for SmeltUnknown {
    fn eq(&self, other: &f64) -> bool {
        matches!(self, Self::Number(value) if value == other)
    }
}

impl PartialEq<SmeltUnknown> for f64 {
    fn eq(&self, other: &SmeltUnknown) -> bool {
        other == self
    }
}

// @smelt:item-end

// @smelt:item impl_PartialOrd_f64_for_SmeltUnknown
// @smelt:if unknown
impl PartialOrd<f64> for SmeltUnknown {
    fn partial_cmp(&self, other: &f64) -> Option<::std::cmp::Ordering> {
        match self {
            Self::Number(value) => value.partial_cmp(other),
// @smelt:end
// @smelt:if bigint
            Self::BigInt(value) => smelt_bigint_compare_number(value, *other),
// @smelt:end
// @smelt:if unknown
            Self::String(value) => smelt_string_to_number(value).partial_cmp(other),
            Self::Bool(value) => (if *value { 1.0 } else { 0.0 }).partial_cmp(other),
            Self::Null | Self::Undefined | Self::Symbol(_) | Self::Array(_) | Self::Object(_) | Self::Function(_) | Self::Promise(_) => None,
        }
    }
}

// @smelt:end
// @smelt:item-end

// @smelt:item impl_PartialOrd_SmeltUnknown_for_f64 when unknown
impl PartialOrd<SmeltUnknown> for f64 {
    fn partial_cmp(&self, other: &SmeltUnknown) -> Option<::std::cmp::Ordering> {
        other.partial_cmp(self).map(::std::cmp::Ordering::reverse)
    }
}

impl PartialEq<SmeltUnknown> for Option<SmeltUnknown> {
    fn eq(&self, other: &SmeltUnknown) -> bool {
        self.as_ref().is_some_and(|value| value == other)
    }
}

impl PartialOrd<SmeltUnknown> for Option<SmeltUnknown> {
    fn partial_cmp(&self, other: &SmeltUnknown) -> Option<::std::cmp::Ordering> {
        self.as_ref().and_then(|value| value.partial_cmp(other))
    }
}

fn smelt_unknown_date_value(value: &SmeltUnknown) -> f64 {
    match value {
        SmeltUnknown::Object(object) => match object.get("__smelt_date") { Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN },
        _ => f64::NAN,
    }
}

// @smelt:item-end

// @smelt:item smelt_unknown_rank
// @smelt:if unknown
fn smelt_unknown_rank(value: &SmeltUnknown) -> u8 {
    match value {
        SmeltUnknown::Null => 0,
        SmeltUnknown::Undefined => 8,
        SmeltUnknown::Bool(_) => 1,
        SmeltUnknown::Number(_) => 2,
// @smelt:end
// @smelt:if bigint
        SmeltUnknown::BigInt(_) => 2,
// @smelt:end
// @smelt:if unknown
        SmeltUnknown::String(_) => 3,
        SmeltUnknown::Symbol(_) => 4,
        SmeltUnknown::Array(_) => 5,
        SmeltUnknown::Object(_) => 6,
        SmeltUnknown::Function(_) => 7,
        SmeltUnknown::Promise(_) => 9,
    }
}

//~ JavaScript's Abstract Relational Comparison (`<`, `<=`, `>`, `>=`) for
//~ two erased values: when *both* operands are strings it compares them
//~ lexically (byte order over the UTF-8 encoding, matching JS's UTF-16
//~ code-unit order for the BMP), otherwise it runs `ToNumber` on both and
//~ compares as `f64`. A `NaN` on either side yields `None` (an unordered
//~ result), so every relational operator reports `false` — exactly the JS
//~ outcome. Codegen routes the both-erased relational arms here so a
//~ runtime string comparison stays lexical instead of collapsing to
//~ `NaN`-vs-`NaN` under blind numeric coercion.
fn smelt_unknown_js_relational_ordering(left: &SmeltUnknown, right: &SmeltUnknown) -> Option<::std::cmp::Ordering> {
    match (left, right) {
        (SmeltUnknown::String(left), SmeltUnknown::String(right)) => Some(left.cmp(right)),
// @smelt:end
// @smelt:if bigint
        //~ A bigint compares with a bigint exactly and with any other
        //~ value by mathematical value, never through `f64`.
        (SmeltUnknown::BigInt(left), SmeltUnknown::BigInt(right)) => Some(left.cmp(right)),
        (SmeltUnknown::BigInt(left), right) => smelt_bigint_compare_number(left, smelt_unknown_to_number(right)),
        (left, SmeltUnknown::BigInt(right)) => smelt_bigint_compare_number(right, smelt_unknown_to_number(left)).map(::std::cmp::Ordering::reverse),
// @smelt:end
// @smelt:if unknown
        _ => smelt_unknown_to_number(left).partial_cmp(&smelt_unknown_to_number(right)),
    }
}

//~ `ToNumber` for an erased value, mirroring the inline coercion codegen
//~ emits when a `SmeltUnknown` flows into a numeric context: numeric
//~ strings parse to their value, non-numeric strings become `NaN`, booleans
//~ map to `0`/`1`, `__smelt_date` objects surface their timestamp, and
//~ every remaining shape is `NaN`.
fn smelt_unknown_to_number(value: &SmeltUnknown) -> f64 {
    match value {
        SmeltUnknown::Number(value) => *value,
// @smelt:end
// @smelt:if bigint
        SmeltUnknown::BigInt(value) => smelt_bigint_to_number(value),
// @smelt:end
// @smelt:if unknown
        SmeltUnknown::Object(value) => match value.get("__smelt_date") { Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN },
        SmeltUnknown::String(value) => smelt_string_to_number(value),
        SmeltUnknown::Bool(value) => if *value { 1.0 } else { 0.0 },
        SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN,
    }
}

// @smelt:end
// @smelt:item-end
