// `SmeltMatch`: a regular expression match.

// Emit the concrete `SmeltMatch` runtime type for RegExp match results.
//
// `RegExp.prototype.exec` and `String.prototype.matchAll` return a JavaScript
// match result: an array-like value whose numbered entries are the capture
// groups (entry `0` is the whole match) plus the `index`, `input`, and
// `groups` properties. Historically Smelt built this shape inline as a
// `SmeltUnknown::Object` property bag, which erased its statically known
// structure. `SmeltMatch` models that structure with concrete Rust fields:
//
// * `groups` — numbered capture groups as a shared `Vec<Option<String>>` (an absent
//   optional group is `None`, matching JavaScript `undefined`).
// * `named` — named capture groups keyed by their original name (and a
//   snake_case alias so generated snake_cased field reads still resolve).
// * `match_index` — the zero-based match offset (`.index`).
// * `input` — the full searched string (`.input`).
//
// The payload (`groups`, `named`, `input`) sits behind `Rc` because a match is
// never written after `exec` builds it (writes to a match are not lowered
// through this type) while generated code clones values freely under the
// aggressive clone strategy: a deep copy of every group string per clone was
// ~17 heap allocations per request in Hono's RegExpRouter
// (`blocker-logs/hono-crud-perf-round2.md`). Sharing keeps a clone at three
// reference-count increments.
//
// Typed accessors (`group`, `named_group`, `index`, `input`, `len`) let
// callers read the shape without any dynamic tagging. When a match value has
// to cross a genuinely dynamic boundary (an `unknown`-typed sink), it is
// converted with the explicit [`IntoSmeltUnknown`] adapter — the single place where the
// concrete match is intentionally erased.
// @smelt:item SmeltMatch when dep_regress
/// A concrete JavaScript RegExp match result (numbered groups, named
/// groups, `index`, and `input`).
#[derive(Clone, Debug, Default)]
pub struct SmeltMatch {
    id: usize,
    /// Numbered capture groups; entry 0 is the whole match. An absent
    /// optional group is `None` (JavaScript `undefined`).
    groups: ::std::rc::Rc<Vec<Option<JsString>>>,
    /// Named capture groups keyed by name (plus a snake_case alias).
    named: ::std::rc::Rc<::std::collections::HashMap<String, Option<JsString>>>,
    /// Zero-based UTF-16 offset of the match within `input` (`.index`).
    match_index: usize,
    /// The full string that was searched (`.input`).
    input: JsString,
    /// Each numbered group's UTF-16 `[start, end)` (`.indices`), kept only
    /// when the regexp has the `d` flag; an absent group is `None`.
    spans: Option<::std::rc::Rc<Vec<Option<(f64, f64)>>>>,
}

//~ `id` is reference identity, not content: two matches produced by separate
//~ `exec` calls (or an original and its clone) must still compare structurally
//~ equal under `toEqual`/`isEqual`. This mirrors the hand-written
//~ `SmeltRegExp` `PartialEq`, which excludes its `id` for the same reason;
//~ the derived impl used to make every clone unequal to its source.
/// Structural equality over the match content, ignoring reference identity.
impl PartialEq for SmeltMatch { fn eq(&self, other: &Self) -> bool { self.groups == other.groups && self.named == other.named && self.match_index == other.match_index && self.input == other.input && self.spans == other.spans } }

#[allow(dead_code)]
impl SmeltMatch {
    /// Build a `SmeltMatch` from a `regress` match over `input`'s code units;
    /// `spans` keeps the group indices (the `d` flag). Every offset `regress`
    /// reports is already a code unit index.
    fn from_regress(found: &regress::Match, input: &JsStr, spans: bool) -> Self {
        let units = input.units();
        let text = |range: ::std::ops::Range<usize>| JsString::from_units(units.get(range).unwrap_or(&[]));
        let ranges = ::std::iter::once(Some(found.range())).chain(found.captures.iter().cloned()).collect::<Vec<_>>();
        let spans = spans.then(|| ::std::rc::Rc::new(ranges.iter().map(|range| range.as_ref().map(|range| (smelt_units_to_number(range.start), smelt_units_to_number(range.end)))).collect::<Vec<_>>()));
        let groups = ranges.into_iter().map(|range| range.map(text)).collect::<Vec<_>>();
        let mut named = ::std::collections::HashMap::new();
        for (name, range) in found.named_groups() {
            let value = range.map(text);
            named.insert(name.to_owned(), value.clone());
            let mut snake = String::new();
            for (index, ch) in name.chars().enumerate() {
                if ch.is_ascii_uppercase() { if index > 0 { snake.push('_'); } snake.push(ch.to_ascii_lowercase()); } else { snake.push(ch); }
            }
            named.insert(snake, value);
        }
        Self { id: smelt_next_object_id(), groups: ::std::rc::Rc::new(groups), named: ::std::rc::Rc::new(named), match_index: found.start(), input: input.to_owned(), spans }
    }
    /// Build the array a GLOBAL `String.prototype.match` answers.
    ///
    /// JavaScript returns a plain array of every whole match there, with no
    /// capture groups, `index`, `input` or `groups`. It is carried as a
    /// match value so both spellings of `match` share one type: the numbered
    /// entries are the whole matches (all present), there are no named
    /// groups, and `index` is 0 with `input` the searched string, which a
    /// global match's reader does not consult.
    fn from_global_matches(matches: Vec<JsString>, input: &JsStr) -> Self {
        Self { id: smelt_next_object_id(), groups: ::std::rc::Rc::new(matches.into_iter().map(Some).collect()), named: ::std::rc::Rc::default(), match_index: 0, input: input.to_owned(), spans: None }
    }
// @smelt:if smelt_list
    //~ The array view names `SmeltList`, which only exists when the
    //~ program uses lists; a program that only reads groups never asks for
    //~ the view, so it is emitted under the same pay-for-use gate.
    /// The match viewed as the JavaScript array it is (`[...match]`).
    ///
    /// Entry 0 is the whole match; a group that did not participate is
    /// `None` (`undefined`). The list keeps the match's identity (`===`)
    /// but copies its entries: array methods lowered against it only read
    /// (writes to a match are not lowered through it).
    pub fn to_array_view(&self) -> SmeltList<Option<JsString>> {
        SmeltList::with_id(self.id, self.groups.as_ref().clone())
    }
    /// `match.indices`: each group's `[start, end]` when the regexp has the
    /// `d` flag (an absent group `undefined`), else `undefined`.
    pub fn indices(&self) -> Option<SmeltList<Option<(f64, f64)>>> {
        self.spans.as_ref().map(|spans| SmeltList::new(spans.as_ref().clone()))
    }
// @smelt:end
    /// Read a numbered capture group (`match[n]`).
    fn group(&self, index: usize) -> Option<&JsStr> {
        self.groups.get(index).and_then(|value| value.as_deref())
    }
    /// Read a numbered capture group as an owned optional string.
    ///
    /// Consumer index reads (`match[n]`) are typed `Optional(String)`; a
    /// group that did not participate in the match is `None` (JavaScript
    /// `undefined`).
    pub fn group_owned(&self, index: usize) -> Option<JsString> {
        self.groups.get(index).cloned().flatten()
    }
    /// Read a named capture group (`match.groups.name`).
    fn named_group(&self, name: &str) -> Option<&JsStr> {
        self.named.get(name).and_then(|value| value.as_deref())
    }
    /// Read a named capture group as an owned optional string.
    ///
    /// Consumer named-group reads (`match.groups.name`) are typed
    /// `Optional(String)`; a group that did not participate is `None`.
    pub fn named_group_owned(&self, name: &str) -> Option<JsString> {
        self.named.get(name).cloned().flatten()
    }
    /// The zero-based match offset (`match.index`).
    pub fn index(&self) -> f64 {
        self.match_index as f64
    }
    /// The full searched string (`match.input`).
    fn input(&self) -> &JsStr {
        &self.input
    }
    /// The full searched string as an owned value (`match.input`).
    pub fn input_owned(&self) -> JsString {
        self.input.clone()
    }
    /// Number of numbered capture groups, including the whole match.
    fn len(&self) -> usize {
        self.groups.len()
    }
    /// Number of numbered capture groups as a JavaScript number (`match.length`).
    pub fn length(&self) -> f64 {
        self.groups.len() as f64
    }
    /// Whether there are no numbered groups (never true for a real match).
    fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
}

// @smelt:item-end

// The erasure adapter references the `SmeltUnknown` carrier and is only
// emitted when the crate genuinely erases a match into dynamic dataflow.
// Programs that keep every match read statically typed never emit
// `SmeltUnknown`, so the adapter would otherwise fail to compile.
// @smelt:item impl_IntoSmeltUnknown_for_SmeltMatch when unknown
/// Erase a concrete match into a `SmeltUnknown` at a dynamic boundary.
///
/// This reproduces the JavaScript match-array-with-properties shape:
/// numbered string keys for the groups plus `groups`, `index`, and
/// `input`. It is the single explicit adapter used when a typed
/// `SmeltMatch` must flow into erased `unknown` consumer dataflow.
///
/// A JavaScript match result IS an array — `Array.isArray(/c/.exec(s))` is
/// `true` and `Object.prototype.toString.call` reports `[object Array]` —
/// carrying `index`, `input` and `groups` as ordinary named properties. So
/// the erasure is a `SmeltUnknown::Array` over the numbered groups plus
/// three entries in the identity-keyed named-property table. That is what
/// makes a match compare equal to the plain array a spec matches it
/// against (es-toolkit `isEqualWith`'s array arm compares only `length` and
/// the elements), and, because the table is keyed by identity rather than
/// held on the `SmeltArray`, a `T[]`-typed reader that narrows the value
/// with `Array.isArray` and then reads `.index`/`.input` (es-toolkit
/// `cloneDeepWith`) still finds them through its typed list handle.
impl IntoSmeltUnknown for SmeltMatch {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        let elements = self.groups.iter().map(|value| value.clone().map_or(SmeltUnknown::Undefined, SmeltUnknown::String)).collect::<Vec<_>>();
        let groups = self.named.iter().map(|(name, value)| (name.clone(), value.clone().map_or(SmeltUnknown::Undefined, SmeltUnknown::String))).collect::<Vec<_>>();
        let array = SmeltArray::with_id(self.id, elements);
        //~ Derived from this match, and rewritten by its every erasure.
        smelt_array_store_named_properties(&array, [("index", SmeltUnknown::Number(self.match_index as f64)), ("input", SmeltUnknown::String(self.input)), ("groups", SmeltUnknown::Object(SmeltObject::new(groups)))], true);
        SmeltUnknown::Array(array)
    }
}

/// Recover a concrete match from an erased value.
///
/// The inverse of the adapter above, so a match that round-trips through
/// erased dataflow (`cloneDeep(/re/.exec(s))`, an `unknown` bag) comes back
/// with its groups, named groups, `index` and `input` intact instead of the
/// empty `Default::default()` the generic class fallback would produce.
/// A bare array (the JavaScript match value IS an array) restores the
/// numbered groups; the extra properties are then simply absent.
impl SmeltFromUnknown for SmeltMatch {
    fn smelt_from_unknown(value: SmeltUnknown) -> Self {
        let group_of = |value: SmeltUnknown| match value { SmeltUnknown::String(text) => Some(text), _ => None };
        match value {
            SmeltUnknown::Array(values) => {
                let mut named = ::std::collections::HashMap::new();
                if let Some(SmeltUnknown::Object(entries)) = values.named_property("groups") { for (name, entry) in entries.iter() { named.insert(name.to_string_lossy(), group_of(entry)); } }
                let match_index = match values.named_property("index") { Some(SmeltUnknown::Number(index)) if index.is_finite() && index >= 0.0 => index as usize, _ => 0 };
                let input = match values.named_property("input") { Some(SmeltUnknown::String(input)) => input, _ => JsString::new() };
                let id = values.id;
                Self { id, groups: ::std::rc::Rc::new(values.into_vec().into_iter().map(group_of).collect()), named: ::std::rc::Rc::new(named), match_index, input, spans: None }
            }
            SmeltUnknown::Object(map) => {
                let mut groups = Vec::new();
                while let Some(entry) = map.get(&groups.len().to_string()) { groups.push(group_of(entry)); }
                let mut named = ::std::collections::HashMap::new();
                if let Some(SmeltUnknown::Object(entries)) = map.get("groups") { for (name, entry) in entries.iter() { named.insert(name.to_string_lossy(), group_of(entry)); } }
                let match_index = match map.get("index") { Some(SmeltUnknown::Number(index)) if index.is_finite() && index >= 0.0 => index as usize, _ => 0 };
                let input = match map.get("input") { Some(SmeltUnknown::String(input)) => input, _ => JsString::new() };
                Self { id: map.id, groups: ::std::rc::Rc::new(groups), named: ::std::rc::Rc::new(named), match_index, input, spans: None }
            }
            _ => Self::default(),
        }
    }
}

// @smelt:item-end
