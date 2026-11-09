// `SmeltRegExp`.

// @smelt:item SMELT_REGEX_CACHE when dep_regex
thread_local! { static SMELT_REGEX_CACHE: ::std::cell::RefCell<::std::collections::HashMap<String, ::std::option::Option<::std::rc::Rc<fancy_regex::Regex>>>> = ::std::cell::RefCell::new(::std::collections::HashMap::new()); }

//~ Compiled-program budget for one pattern.
//~
//~ The Rust `regex` crate defaults to a 10 MiB compiled program (and a 2 MiB
//~ lazy-DFA cache) because it is written to accept patterns from untrusted
//~ input. A pattern reached from generated code came out of the program's
//~ own source, so that defence buys nothing here while the limit is
//~ directly observable: a bounded repetition with a large upper bound
//~ (`X{0,N}`) expands to roughly `N` copies of `X` and blows the default
//~ budget. The budget is raised rather than removed (`usize::MAX` would
//~ turn a genuinely pathological pattern into an OOM instead of an error).
const SMELT_REGEX_SIZE_LIMIT: usize = 64 * 1024 * 1024;
const SMELT_REGEX_DFA_SIZE_LIMIT: usize = 16 * 1024 * 1024;

/// Compile a Python `re` pattern once per thread (`None`: it does not compile).
fn smelt_py_regex(pattern: &str) -> Option<::std::rc::Rc<fancy_regex::Regex>> {
    if let Some(cached) = SMELT_REGEX_CACHE.with(|cache| cache.borrow().get(pattern).cloned()) { return cached; }
    let compiled = fancy_regex::RegexBuilder::new(pattern).delegate_size_limit(SMELT_REGEX_SIZE_LIMIT).delegate_dfa_size_limit(SMELT_REGEX_DFA_SIZE_LIMIT).build().ok().map(::std::rc::Rc::new);
    SMELT_REGEX_CACHE.with(|cache| { cache.borrow_mut().insert(pattern.to_owned(), compiled.clone()); });
    compiled
}

/// Expand one replacement pattern against a `fancy_regex` match.
//~
//~ `$$` is a literal `$`, `$&` the match, `` $` `` and `$'` the text before
//~ and after it, `$n`/`$nn` a numbered group (two digits preferred when that
//~ group exists), `$<name>` a named group. Anything else, including a `$n`
//~ past the last group, is left exactly as written.
fn smelt_regex_substitution(replacement: &str, haystack: &str, captures: &fancy_regex::Captures<'_>) -> String {
    let Some(whole) = captures.get(0) else { return replacement.to_owned(); };
    let chars: Vec<char> = replacement.chars().collect();
    let mut out = String::with_capacity(replacement.len());
    let mut index = 0usize;
    while let Some(&ch) = chars.get(index) {
        if ch != '$' { out.push(ch); index = index.saturating_add(1); continue; }
        match chars.get(index.saturating_add(1)) {
            Some('$') => { out.push('$'); index = index.saturating_add(2); }
            Some('&') => { out.push_str(whole.as_str()); index = index.saturating_add(2); }
            Some('`') => { out.push_str(haystack.get(..whole.start()).unwrap_or("")); index = index.saturating_add(2); }
            Some('\'') => { out.push_str(haystack.get(whole.end()..).unwrap_or("")); index = index.saturating_add(2); }
            Some('<') => {
                let open = index.saturating_add(2);
                match (open..chars.len()).find(|position| chars.get(*position) == Some(&'>')) {
                    Some(close) => {
                        let name: String = chars.get(open..close).unwrap_or_default().iter().collect();
                        if let Some(group) = captures.name(&name) { out.push_str(group.as_str()); }
                        index = close.saturating_add(1);
                    }
                    None => { out.push('$'); index = index.saturating_add(1); }
                }
            }
            Some(digit) if digit.is_ascii_digit() => {
                let first = digit.to_digit(10).unwrap_or(0) as usize;
                let second = chars.get(index.saturating_add(2)).and_then(|next| next.to_digit(10));
                let two_digit = second.map(|value| first.saturating_mul(10).saturating_add(value as usize));
                let total = captures.len();
                let selected = match two_digit { Some(group) if group >= 1 && group < total => Some((group, 3usize)), _ => if first >= 1 && first < total { Some((first, 2usize)) } else { None } };
                match selected {
                    Some((group, width)) => { if let Some(matched) = captures.get(group) { out.push_str(matched.as_str()); } index = index.saturating_add(width); }
                    None => { out.push('$'); index = index.saturating_add(1); }
                }
            }
            _ => { out.push('$'); index = index.saturating_add(1); }
        }
    }
    out
}

/// Python `re.sub` / `str.replace` over a pattern: every (or the first)
/// match replaced by the expanded replacement.
//~ A pattern that does not
//~ compile is a hard error naming it.
fn smelt_py_regex_replace(pattern: &str, haystack: &str, replacement: &str, replace_all: bool) -> String {
    let Some(regex) = smelt_py_regex(pattern) else { panic!("re.error: invalid regular expression: {pattern}") };
    let mut output = String::new();
    let mut last_end = 0usize;
    for captures in regex.captures_iter(haystack).filter_map(Result::ok) {
        let Some(matched) = captures.get(0) else { continue; };
        output.push_str(haystack.get(last_end..matched.start()).unwrap_or(""));
        output.push_str(&smelt_regex_substitution(replacement, haystack, &captures));
        last_end = matched.end();
        if !replace_all { break; }
    }
    output.push_str(haystack.get(last_end..).unwrap_or(""));
    output
}
// @smelt:item-end

// A JavaScript `RegExp`, on the `regress` engine.
//
// `regress` implements ECMAScript regular expression syntax and semantics
// and matches over UTF-16 code units directly, so a `JsString` is searched
// as it is stored: every index (`lastIndex`, a match's `index`, a group's
// span) is a code unit index with no conversion, and a lone surrogate in the
// subject or the pattern is an ordinary unit. With the `u` or `v` flag a
// surrogate pair is one character (`find_from_utf16`); without them every
// unit is a character (`find_from_ucs2`), as ECMA-262 specifies.
//
// The compiled automaton is memoized per (pattern, flags) in a per-thread
// map and on the value itself (shared by its clones), so a hot `exec`
// neither recompiles nor rehashes. The map holds one entry per distinct
// pattern the thread has built: one per regex literal, which the source
// itself keeps alive; a program that builds regexes from dynamic strings
// keeps one per distinct input.
// @smelt:item SMELT_REGRESS_CACHE when dep_regress
thread_local! { static SMELT_REGRESS_CACHE: ::std::cell::RefCell<::std::collections::HashMap<String, ::std::option::Option<::std::rc::Rc<regress::Regex>>>> = ::std::cell::RefCell::new(::std::collections::HashMap::new()); }

/// Expand one JavaScript replacement pattern against a match (ECMA-262 `GetSubstitution`).
//~
//~ `$$` is a literal `$`, `$&` the match, `` $` `` and `$'` the text before
//~ and after it, `$n`/`$nn` a numbered group (two digits preferred when that
//~ group exists), `$<name>` a named group. Anything else, including a `$n`
//~ past the last group, is left exactly as written. Every position is a
//~ code unit of `haystack`.
fn smelt_js_regex_substitution(replacement: &[u16], haystack: &[u16], found: &regress::Match, out: &mut Vec<u16>) {
    let dollar = u16::from(b'$');
    let range_units = |range: Option<::std::ops::Range<usize>>| range.and_then(|range| haystack.get(range)).unwrap_or(&[]);
    let mut index = 0usize;
    while let Some(&unit) = replacement.get(index) {
        if unit != dollar { out.push(unit); index = index.saturating_add(1); continue; }
        let next = replacement.get(index.saturating_add(1)).and_then(|next| u8::try_from(*next).ok());
        match next {
            Some(b'$') => { out.push(dollar); index = index.saturating_add(2); }
            Some(b'&') => { out.extend_from_slice(range_units(Some(found.range()))); index = index.saturating_add(2); }
            Some(b'`') => { out.extend_from_slice(haystack.get(..found.start()).unwrap_or(&[])); index = index.saturating_add(2); }
            Some(b'\'') => { out.extend_from_slice(haystack.get(found.end()..).unwrap_or(&[])); index = index.saturating_add(2); }
            Some(b'<') => {
                let open = index.saturating_add(2);
                if let Some(close) = (open..replacement.len()).find(|position| replacement.get(*position) == Some(&u16::from(b'>'))) {
                    let name = String::from_utf16_lossy(replacement.get(open..close).unwrap_or(&[]));
                    out.extend_from_slice(range_units(found.named_group(&name)));
                    index = close.saturating_add(1);
                } else {
                    out.push(dollar);
                    index = index.saturating_add(1);
                }
            }
            Some(digit) if digit.is_ascii_digit() => {
                let first = usize::from(digit - b'0');
                let second = replacement.get(index.saturating_add(2)).and_then(|next| u8::try_from(*next).ok()).filter(u8::is_ascii_digit);
                let two_digit = second.map(|value| first.saturating_mul(10).saturating_add(usize::from(value - b'0')));
                let total = found.captures.len().saturating_add(1);
                let selected = match two_digit { Some(group) if group >= 1 && group < total => Some((group, 3usize)), _ => if first >= 1 && first < total { Some((first, 2usize)) } else { None } };
                if let Some((group, width)) = selected {
                    out.extend_from_slice(range_units(found.group(group)));
                    index = index.saturating_add(width);
                } else {
                    out.push(dollar);
                    index = index.saturating_add(1);
                }
            }
            _ => { out.push(dollar); index = index.saturating_add(1); }
        }
    }
}

#[derive(Clone, Debug)]
pub struct SmeltRegExp {
    id: usize,
    //~ `source` and `flags` are immutable for a RegExp's lifetime, and
    //~ generated code clones values freely: shared `Rc<str>` keeps a clone
    //~ at reference-count increments instead of two string copies.
    source: ::std::rc::Rc<str>,
    flags: ::std::rc::Rc<str>,
    last_index: ::std::rc::Rc<::std::cell::RefCell<usize>>,
    //~ The compiled automaton, shared by every clone (`None` inside: the pattern does not compile).
    compiled: ::std::rc::Rc<::std::cell::OnceCell<::std::option::Option<::std::rc::Rc<regress::Regex>>>>,
}

impl PartialEq for SmeltRegExp { fn eq(&self, other: &Self) -> bool { self.source == other.source && self.flags == other.flags && *self.last_index.borrow() == *other.last_index.borrow() } }

#[allow(dead_code)]
impl SmeltRegExp {
    //~ Construct a JavaScript-like RegExp value with shared lastIndex state.
    //~
    //~ `source` is stored as the spec's `EscapeRegExpPattern` renders it:
    //~ an unescaped `/` outside a character class reads back as `\/`, so
    //~ `new RegExp('a/b').source` is `a\/b` exactly as for the literal
    //~ `/a\/b/`, and the two compare equal.
    pub fn new(source: impl ::std::fmt::Display, flags: impl ::std::fmt::Display) -> Self {
        let (source, flags) = (Self::escape_pattern_source(source.to_string()), flags.to_string());
        Self { id: smelt_next_object_id(), source: source.into(), flags: flags.into(), last_index: ::std::rc::Rc::new(::std::cell::RefCell::new(0)), compiled: ::std::rc::Rc::default() }
    }
    //~ Escape every unescaped `/` outside a character class (`EscapeRegExpPattern`).
    fn escape_pattern_source(source: String) -> String {
        if !source.contains('/') { return source; }
        let mut escaped = String::with_capacity(source.len() + 2);
        let (mut in_class, mut after_backslash) = (false, false);
        for ch in source.chars() {
            if after_backslash { escaped.push(ch); after_backslash = false; continue; }
            match ch { '\\' => after_backslash = true, '[' => in_class = true, ']' => in_class = false, '/' if !in_class => escaped.push('\\'), _ => {} }
            escaped.push(ch);
        }
        escaped
    }
    //~ Return true when this RegExp has a flag.
    pub fn has_flag(&self, flag: char) -> bool {
        self.flags.chars().any(|value| value == flag)
    }
    //~ Whether a surrogate pair is one character (`u` or `v`).
    fn is_unicode(&self) -> bool {
        self.has_flag('u') || self.has_flag('v')
    }
    //~ Whether the search reads and writes `lastIndex` (`g` or `y`).
    fn is_stateful(&self) -> bool {
        self.has_flag('g') || self.has_flag('y')
    }
    //~ The compiled automaton.
    //~
    //~ A pattern that does not compile is a hard error naming the original
    //~ JavaScript spelling, so an unsupported pattern can never pass as a
    //~ silent no-op.
    fn compiled(&self) -> ::std::rc::Rc<regress::Regex> {
        match self.try_compiled() { Some(regex) => regex, None => panic!("SyntaxError: invalid regular expression: /{}/{}", self.source, self.flags) }
    }
    //~ The compiled automaton, or `None` when the pattern does not compile.
    //~
    //~ The flags `regress` reads are `i`, `m`, `s`, `u` and `v`; `g`, `y`
    //~ and `d` change how a search is driven and what it records, which
    //~ this type does itself.
    fn try_compiled(&self) -> Option<::std::rc::Rc<regress::Regex>> {
        if let Some(compiled) = self.compiled.get() { return compiled.clone(); }
        let engine_flags: String = self.flags.chars().filter(|flag| matches!(flag, 'i' | 'm' | 's' | 'u' | 'v')).collect();
        let key = format!("{engine_flags}/{}", self.source);
        let compiled = if let Some(cached) = SMELT_REGRESS_CACHE.with(|cache| cache.borrow().get(&key).cloned()) { cached } else {
            //~ Without `u`/`v` a pattern is a sequence of code units, so an
            //~ astral character in it is its two surrogate halves, matching
            //~ the halves of the subject one by one; with them it is one
            //~ code point. `from_unicode` takes the pattern as either.
            let compiled = if self.is_unicode() {
                regress::Regex::with_flags(&self.source, engine_flags.as_str())
            } else {
                regress::Regex::from_unicode(self.source.encode_utf16().map(u32::from), engine_flags.as_str())
            }
            .ok()
            .map(::std::rc::Rc::new);
            SMELT_REGRESS_CACHE.with(|cache| { cache.borrow_mut().insert(key, compiled.clone()); });
            compiled
        };
        let _ = self.compiled.set(compiled.clone());
        compiled
    }
    //~ The first match starting at or after code unit `start`.
    fn find_from(&self, regex: &regress::Regex, units: &[u16], start: usize) -> Option<regress::Match> {
        if start > units.len() { return None; }
        if self.is_unicode() { regex.find_from_utf16(units, start).next() } else { regex.find_from_ucs2(units, start).next() }
    }
    //~ Every match, left to right, as a global search finds them.
    fn find_all(&self, regex: &regress::Regex, units: &[u16]) -> Vec<regress::Match> {
        if self.is_unicode() { regex.find_from_utf16(units, 0).collect() } else { regex.find_from_ucs2(units, 0).collect() }
    }
    //~ Match a string with JavaScript String.prototype.match semantics.
    //~
    //~ A non-global regex answers exactly what `exec` answers (the spec's
    //~ `RegExp.prototype[@@match]` IS `RegExpBuiltinExec` then). A global
    //~ regex resets `lastIndex` to 0 and answers every whole match; no match
    //~ at all is `None` (`null`).
    pub fn match_string(&self, haystack: &JsStr) -> Option<SmeltMatch> {
        if !self.has_flag('g') { return self.exec(haystack); }
        self.match_all_strings(haystack).map(|matches| SmeltMatch::from_global_matches(matches, haystack))
    }
    //~ Every whole match of a GLOBAL String.prototype.match, or `None` (`null`).
    //~
    //~ The spec resets `lastIndex` to 0 first and leaves it there.
    pub fn match_all_strings(&self, haystack: &JsStr) -> Option<Vec<JsString>> {
        *self.last_index.borrow_mut() = 0;
        let regex = self.compiled();
        let units = haystack.units();
        let matches = self.find_all(&regex, units).into_iter().map(|found| JsString::from_units(units.get(found.range()).unwrap_or(&[]))).collect::<Vec<_>>();
        if matches.is_empty() { None } else { Some(matches) }
    }
    //~ `String.prototype.split` with a RegExp separator (ECMA-262 `@@split`).
    //~
    //~ Each match ends a piece and contributes its capture groups (an absent
    //~ group is `undefined`, which a `string[]` result cannot hold, so it is
    //~ the empty string); an empty match at the start of a piece is skipped,
    //~ so `"ab".split(/(?:)/)` is `["a", "b"]`. An empty subject is `[""]`
    //~ unless the regex matches it, then `[]`.
    pub fn split_string(&self, haystack: &JsStr) -> Vec<JsString> {
        let regex = self.compiled();
        let units = haystack.units();
        let piece = |range: ::std::ops::Range<usize>| JsString::from_units(units.get(range).unwrap_or(&[]));
        if units.is_empty() { return if self.find_from(&regex, units, 0).is_some() { Vec::new() } else { vec![JsString::new()] }; }
        let (mut pieces, mut start, mut cursor) = (Vec::new(), 0usize, 0usize);
        while cursor < units.len() {
            let Some(found) = self.find_from(&regex, units, cursor) else { break; };
            let (match_start, match_end) = (found.start(), found.end().min(units.len()));
            if match_start >= units.len() { break; }
            if match_end == start { cursor = match_start.saturating_add(1); continue; }
            pieces.push(piece(start..match_start));
            pieces.extend(found.captures.iter().map(|group| group.clone().map_or_else(JsString::new, piece)));
            start = match_end;
            cursor = start;
        }
        pieces.push(piece(start..units.len()));
        pieces
    }
    //~ Replace matches with JavaScript RegExp-aware String.prototype.replace semantics.
    //~
    //~ The replacement string is a PATTERN (`$&`, `` $` ``, `$'`, `$$`, `$n`,
    //~ `$<name>`; see `smelt_js_regex_substitution`), expanded per match.
    pub fn replace_string(&self, haystack: &JsStr, replacement: &JsStr, force_all: bool) -> JsString {
        let units = haystack.units();
        let regex = self.compiled();
        let replace_all = force_all || self.has_flag('g');
        let mut output = Vec::with_capacity(units.len());
        let mut last_end = 0usize;
        let found = if replace_all { self.find_all(&regex, units) } else { self.find_from(&regex, units, 0).into_iter().collect() };
        for matched in &found {
            output.extend_from_slice(units.get(last_end..matched.start()).unwrap_or(&[]));
            smelt_js_regex_substitution(replacement.units(), units, matched, &mut output);
            last_end = matched.end();
        }
        output.extend_from_slice(units.get(last_end..).unwrap_or(&[]));
        JsString::from_units(&output)
    }
    //~ `String.prototype.replace` with a replacer function: each (or the
    //~ first) match is replaced by what `replacer` answers for it.
    pub fn replace_with(&self, haystack: &JsStr, force_all: bool, mut replacer: impl FnMut(&SmeltMatch) -> JsString) -> JsString {
        let units = haystack.units();
        let regex = self.compiled();
        let replace_all = force_all || self.has_flag('g');
        let found = if replace_all { self.find_all(&regex, units) } else { self.find_from(&regex, units, 0).into_iter().collect() };
        let mut output = Vec::with_capacity(units.len());
        let mut last_end = 0usize;
        for matched in &found {
            output.extend_from_slice(units.get(last_end..matched.start()).unwrap_or(&[]));
            output.extend_from_slice(replacer(&SmeltMatch::from_regress(matched, haystack, false)).units());
            last_end = matched.end();
        }
        output.extend_from_slice(units.get(last_end..).unwrap_or(&[]));
        JsString::from_units(&output)
    }
    //~ Execute this RegExp and return a concrete `SmeltMatch` result.
    //~
    //~ A `g` or `y` regex starts at `lastIndex` and moves it past the match;
    //~ a FAILED stateful search resets it to 0, which is what makes `/a/g`
    //~ over "aa" answer true, true, false and then true again. A `y` match
    //~ must start exactly at `lastIndex`.
    pub fn exec(&self, haystack: &JsStr) -> Option<SmeltMatch> {
        let units = haystack.units();
        let regex = self.compiled();
        let stateful = self.is_stateful();
        let start = if stateful { *self.last_index.borrow() } else { 0 };
        let found = self.find_from(&regex, units, start).filter(|found| !self.has_flag('y') || found.start() == start);
        let Some(found) = found else { if stateful { *self.last_index.borrow_mut() = 0; } return None; };
        if stateful { *self.last_index.borrow_mut() = found.end(); }
        Some(SmeltMatch::from_regress(&found, haystack, self.has_flag('d')))
    }
    //~ Return concrete `SmeltMatch` results for String.prototype.matchAll.
    pub fn match_all_indices(&self, haystack: &JsStr) -> Vec<SmeltMatch> {
        let regex = self.compiled();
        self.find_all(&regex, haystack.units()).iter().map(|found| SmeltMatch::from_regress(found, haystack, self.has_flag('d'))).collect::<Vec<_>>()
    }
    //~ `String.prototype.search`: the code unit index of the first match, if any.
    pub fn search_index(&self, haystack: &JsStr) -> Option<usize> {
        let regex = self.compiled();
        self.find_from(&regex, haystack.units(), 0).map(|found| found.start())
    }
    //~ Whether the pattern matches anywhere, ignoring `lastIndex`: `test` on
    //~ a regex without `g` or `y`, whose `lastIndex` stays 0.
    pub fn is_match(&self, haystack: &JsStr) -> bool {
        self.search_index(haystack).is_some()
    }
    //~ Test this RegExp against a string with JavaScript lastIndex updates.
    pub fn test(&self, haystack: &JsStr) -> bool {
        self.exec(haystack).is_some()
    }
}

// @smelt:item-end

// The `IntoSmeltUnknown` erasure adapters reference the `SmeltUnknown`
// carrier, which is only emitted when the crate genuinely crosses a
// dynamic boundary. A regex/match program that keeps every value
// statically typed does not emit `SmeltUnknown`, so the adapters are
// gated on that need rather than on regex presence alone.
// @smelt:item impl_IntoSmeltUnknown_for_SmeltRegExp when unknown
/// Erase a concrete RegExp into a `SmeltUnknown` at a dynamic boundary.
//~
//~ A JavaScript RegExp object owns three observable data properties:
//~ `source`, `flags` and the writable `lastIndex`. All three must cross
//~ the boundary, or a round trip through erased dataflow (`clone(re)`,
//~ `structuredClone`, a `Record<string, unknown>` bag) silently resets
//~ `lastIndex` to 0.
impl IntoSmeltUnknown for SmeltRegExp {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        let last_index = *self.last_index.borrow() as f64;
        SmeltUnknown::Object(SmeltObject::with_id(self.id, Vec::from([
        ("source".to_owned(), SmeltUnknown::String(JsString::from(&*self.source))),
        ("flags".to_owned(), SmeltUnknown::String(JsString::from(&*self.flags))),
        ("lastIndex".to_owned(), SmeltUnknown::Number(last_index)),
        ("__smelt_regexp".to_owned(), SmeltUnknown::Bool(true)),
        ])))
    }
}

/// Recover a concrete RegExp from an erased value.
//~
//~ The exact inverse of the adapter above: a marker record restores
//~ `source`, `flags` and `lastIndex`; a bare string is the `new
//~ RegExp(str)` spelling and yields a flagless pattern. Anything else
//~ answers the empty pattern, matching `new RegExp(undefined)`.
impl SmeltFromUnknown for SmeltRegExp {
    fn smelt_from_unknown(value: SmeltUnknown) -> Self {
        match value {
            SmeltUnknown::String(source) => Self::new(source.to_string(), String::new()),
            SmeltUnknown::Object(map) => {
                let source = match map.get("source") { Some(SmeltUnknown::String(source)) => source.to_string(), _ => String::new() };
                let flags = match map.get("flags") { Some(SmeltUnknown::String(flags)) => flags.to_string(), _ => String::new() };
                let regexp = Self::new(source, flags);
                if let Some(SmeltUnknown::Number(last_index)) = map.get("lastIndex") { if last_index.is_finite() && last_index >= 0.0 { *regexp.last_index.borrow_mut() = last_index as usize; } }
                regexp
            }
            _ => Self::new(String::new(), String::new()),
        }
    }
}

// @smelt:item-end

// @smelt:item impl_Default_for_SmeltRegExp when dep_regress
impl Default for SmeltRegExp {
    //~ Construct a RegExp that matches the empty string.
    fn default() -> Self {
        Self::new(String::new(), String::new())
    }
}

// @smelt:item-end
