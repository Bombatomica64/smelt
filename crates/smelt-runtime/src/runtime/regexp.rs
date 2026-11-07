// `SmeltRegExp`.

// Compiled-automaton memo, keyed by the translated Rust pattern text.
//
// A JavaScript `RegExp` object is constructed once (typically at module
// evaluation, from a literal) and reused; the generated equivalent
// rebuilds a `SmeltRegExp` wrapper wherever the value is referenced,
// because `SmeltRegExp` carries observable mutable state (`lastIndex`)
// and a per-object identity that must NOT be shared between distinct
// source objects. The expensive half — compiling the pattern into a
// `fancy_regex::Regex` — is pure and depends only on the pattern text,
// so it is memoized here instead. Each wrapper therefore stays a fresh,
// independently-mutable value while the automaton for a given pattern is
// built at most once per thread. `Rc` (not `Arc`) matches the rest of the
// single-threaded runtime, and the per-thread cell keeps each `#[test]`
// thread independent. `None` is cached too, so an invalid pattern is not
// recompiled on every call either.
//
// The map is unbounded: it retains one compiled automaton per distinct
// pattern the thread has ever built. That is exactly one entry per regex
// literal in the program, which is what the source itself would have kept
// alive at module scope. A program that builds regexes from *dynamic*
// strings (`new RegExp(input)`) instead retains one entry per distinct
// input, which JavaScript would not — an eviction policy belongs here if
// that shape ever shows up in a corpus, but no arbitrary cap is imposed
// before there is a case to size it against.
// @smelt:item SMELT_REGEX_CACHE when dep_regex
thread_local! { static SMELT_REGEX_CACHE: ::std::cell::RefCell<::std::collections::HashMap<String, ::std::option::Option<::std::rc::Rc<fancy_regex::Regex>>>> = ::std::cell::RefCell::new(::std::collections::HashMap::new()); }

//~ Compiled-program budget for one pattern.
//~
//~ JavaScript imposes no size limit on a `RegExp`, but the Rust `regex`
//~ crate defaults to a 10 MiB compiled program (and a 2 MiB lazy-DFA
//~ cache) because it is written to accept patterns from untrusted input.
//~ A pattern reached from generated code came out of the program's own
//~ source, so that defence buys nothing here while the limit is directly
//~ observable: a bounded repetition with a large upper bound -- `X{0,N}`,
//~ the idiomatic JavaScript way to write a repetition that cannot be made
//~ to backtrack forever -- expands to roughly `N` copies of `X` and blows
//~ the default budget, which surfaced as a pattern that "does not
//~ compile" even though it is perfectly well-formed JavaScript. The
//~ budget is raised rather than removed (`usize::MAX` would turn a
//~ genuinely pathological pattern into an OOM instead of an error) and it
//~ is stated once, as a property of the engine and not of any pattern.
const SMELT_REGEX_SIZE_LIMIT: usize = 64 * 1024 * 1024;
const SMELT_REGEX_DFA_SIZE_LIMIT: usize = 16 * 1024 * 1024;

//~ Emits the JavaScript replacement-pattern expander used by `String.replace`.
//~
//~ The second argument of `String.prototype.replace` is not literal text: ECMA-262
//~ `GetSubstitution` gives `$$`, `$&`, `` $` ``, `$'`, `$n`/`$nn` and `$<name>`
//~ meanings relative to the current match, and any other `$x` stays literal.
//~ The runtime used to push the replacement verbatim, so `'\\$&'` — the whole
//~ point of a pattern like `escapeRegExp`'s — inserted the two characters `$&`
//~ instead of the matched text.
/// Expand one JavaScript replacement pattern against a match (ECMA-262 `GetSubstitution`).
///
/// `$$` is a literal `$`, `$&` the match, `` $` `` and `$'` the text before
/// and after it, `$n`/`$nn` a numbered group (two digits preferred when that
/// group exists), `$<name>` a named group. Anything else, including a `$n`
/// past the last group, is left exactly as written.
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

#[derive(Clone, Debug)]
pub struct SmeltRegExp {
    id: usize,
    //~ `source` and `flags` are immutable for a RegExp's lifetime, and
    //~ generated code clones values freely (aggressive clone strategy):
    //~ shared `Rc<str>` keeps a clone at reference-count increments
    //~ instead of two string copies, and the compiled automaton is
    //~ memoized on the value so `exec` skips rebuilding and hashing the
    //~ translated pattern on every call
    //~ (`blocker-logs/hono-crud-perf-round2.md`).
    source: ::std::rc::Rc<str>,
    flags: ::std::rc::Rc<str>,
    last_index: ::std::rc::Rc<::std::cell::RefCell<usize>>,
    /// The compiled automaton, shared by every clone (`None` inside: the pattern does not compile).
    compiled: ::std::rc::Rc<::std::cell::OnceCell<::std::option::Option<::std::rc::Rc<fancy_regex::Regex>>>>,
}

impl PartialEq for SmeltRegExp { fn eq(&self, other: &Self) -> bool { self.source == other.source && self.flags == other.flags && *self.last_index.borrow() == *other.last_index.borrow() } }

impl SmeltRegExp {
    /// Construct a JavaScript-like RegExp value with shared lastIndex state.
    ///
    /// `source` is stored as the spec's `EscapeRegExpPattern` renders it:
    /// an unescaped `/` outside a character class reads back as `\/`, so
    /// `new RegExp('a/b').source` is `a\/b` exactly as for the literal
    /// `/a\/b/`, and the two compare equal.
    pub fn new(source: impl ::std::fmt::Display, flags: impl ::std::fmt::Display) -> Self {
        let (source, flags) = (Self::escape_pattern_source(source.to_string()), flags.to_string());
        Self { id: smelt_next_object_id(), source: source.into(), flags: flags.into(), last_index: ::std::rc::Rc::new(::std::cell::RefCell::new(0)), compiled: ::std::rc::Rc::default() }
    }
    /// Escape every unescaped `/` outside a character class (`EscapeRegExpPattern`).
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
    /// Return true when this RegExp has a flag.
    pub fn has_flag(&self, flag: char) -> bool {
        self.flags.chars().any(|value| value == flag)
    }
    /// Compile the Rust regex equivalent for this JavaScript RegExp.
    ///
    /// A pattern that does not compile is a hard error naming the original
    /// JavaScript spelling. Every RegExp operation goes through here for
    /// that reason: the alternative -- returning the haystack unchanged, as
    /// `replace`/`split`/`matchAll` used to -- turned an untranslated
    /// pattern into a silent no-op that looked like a passing program.
    fn compiled(&self) -> ::std::rc::Rc<fancy_regex::Regex> {
        match self.try_compiled() { Some(regex) => regex, None => panic!("SyntaxError: invalid regular expression: /{}/{}", self.source, self.flags) }
    }
    /// Try to compile the Rust regex equivalent for this JavaScript RegExp.
    ///
    /// The compiled automaton is memoized per translated pattern in
    /// `SMELT_REGEX_CACHE`, so repeatedly constructing the same RegExp value
    /// (for example a module-level pattern referenced from a hot function)
    /// compiles it at most once per thread. Compilation is a pure function of
    /// the pattern text, so sharing it is unobservable; the mutable
    /// `lastIndex` state stays per-`SmeltRegExp`. The result is also kept on
    /// the value itself (shared by its clones), so a hot `exec` neither
    /// rebuilds the translated pattern nor hashes it again.
    fn try_compiled(&self) -> Option<::std::rc::Rc<fancy_regex::Regex>> {
        if let Some(compiled) = self.compiled.get() { return compiled.clone(); }
        let mut prefix = String::new();
        if self.has_flag('i') { prefix.push('i'); }
        if self.has_flag('m') { prefix.push('m'); }
        if self.has_flag('s') { prefix.push('s'); }
        let translated_source = self.source.replace("[^]", "(?s:.)");
        let pattern = if prefix.is_empty() { translated_source } else { format!("(?{prefix}){translated_source}") };
        let compiled = if let Some(cached) = SMELT_REGEX_CACHE.with(|cache| cache.borrow().get(&pattern).cloned()) { cached } else { let compiled = fancy_regex::RegexBuilder::new(&pattern).delegate_size_limit(SMELT_REGEX_SIZE_LIMIT).delegate_dfa_size_limit(SMELT_REGEX_DFA_SIZE_LIMIT).build().ok().map(::std::rc::Rc::new); SMELT_REGEX_CACHE.with(|cache| { cache.borrow_mut().insert(pattern, compiled.clone()); }); compiled };
        let _ = self.compiled.set(compiled.clone());
        compiled
    }
    /// Match a string with JavaScript String.prototype.match semantics.
    ///
    /// A non-global regex answers exactly what `exec` answers (the spec's
    /// `RegExp.prototype[@@match]` IS `RegExpBuiltinExec` then), so an
    /// unmatched capture group is `None` (JavaScript `undefined`) and a
    /// sticky regex reads and advances `lastIndex`. A global regex resets
    /// `lastIndex` to 0 and answers every whole match, built by
    /// `SmeltMatch::from_global_matches`; no match at all is `None` (`null`).
    pub fn match_string(&self, haystack: &JsStr) -> Option<SmeltMatch> {
        if !self.has_flag('g') { return self.exec(haystack); }
        self.match_all_strings(haystack).map(|matches| SmeltMatch::from_global_matches(matches, haystack))
    }
    /// Every whole match of a GLOBAL String.prototype.match, or `None` (`null`).
    ///
    /// The spec resets `lastIndex` to 0 first and leaves it there. Called
    /// directly where the regex is statically known to be global, which is
    /// where the frontend types the result `string[] | null`.
    pub fn match_all_strings(&self, haystack: &JsStr) -> Option<Vec<JsString>> {
        *self.last_index.borrow_mut() = 0;
        let regex = self.compiled();
        let haystack = haystack.to_string_lossy();
        let matches = regex.find_iter(&haystack).filter_map(Result::ok).map(|value| JsString::from(value.as_str())).collect::<Vec<_>>();
        if matches.is_empty() { None } else { Some(matches) }
    }
    /// Split a string with JavaScript RegExp separator semantics.
    pub fn split_string(&self, haystack: &JsStr) -> Vec<JsString> {
        let regex = self.compiled();
        let haystack = haystack.to_string_lossy();
        regex.split(&haystack).filter_map(Result::ok).map(JsString::from).collect::<Vec<_>>()
    }
    /// Replace matches with JavaScript RegExp-aware String.prototype.replace semantics.
    ///
    /// The replacement string is a PATTERN, not literal text: `$&`, `` $` ``,
    /// `$'`, `$$`, `$n` and `$<name>` all stand for parts of the match (see
    /// `smelt_regex_substitution`). It is expanded per match, which is why
    /// this walks `captures_iter` rather than `find_iter`. The global and
    /// single-replacement cases are one loop that stops after the first
    /// match, so the two cannot disagree about the expansion.
    pub fn replace_string(&self, haystack: &JsStr, replacement: &JsStr, force_all: bool) -> JsString {
        let (haystack, replacement) = (&*haystack.to_string_lossy(), &*replacement.to_string_lossy());
        let regex = self.compiled();
        let replace_all = force_all || self.has_flag('g');
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
        JsString::from(output)
    }
    /// Execute this RegExp and return a concrete `SmeltMatch` result.
    ///
    /// The match result is a typed `SmeltMatch` value (numbered groups,
    /// named groups, `index`, `input`) instead of an erased `SmeltUnknown`
    /// property bag; callers erase it explicitly at a dynamic boundary via
    /// `SmeltMatch::into_smelt_unknown` when required.
    pub fn exec(&self, haystack: &JsStr) -> Option<SmeltMatch> {
        let haystack = &*haystack.to_string_lossy();
        let regex = self.compiled();
        //~ `lastIndex` and a match's `index` count UTF-16 code units.
        let start = if self.has_flag('g') || self.has_flag('y') { smelt_utf16_byte_of(haystack, *self.last_index.borrow()).unwrap_or_else(|byte| byte) } else { 0 };
        let suffix = haystack.get(start..).unwrap_or("");
        //~ A FAILED search on a stateful regex resets `lastIndex` to 0,
        //~ which is what makes `/a/g` over "aa" answer true, true,
        //~ false and then true AGAIN: the third call exhausts the string
        //~ and rewinds, so the fourth starts over. Returning `None`
        //~ without the reset left the regex permanently exhausted, so
        //~ every later call answered false.
        let Some(captures) = regex.captures(suffix).ok().flatten() else { if self.has_flag('g') || self.has_flag('y') { *self.last_index.borrow_mut() = 0; } return None; };
        let Some(matched) = captures.get(0) else { if self.has_flag('g') || self.has_flag('y') { *self.last_index.borrow_mut() = 0; } return None; };
        if self.has_flag('y') && matched.start() != 0 { *self.last_index.borrow_mut() = 0; return None; }
        if self.has_flag('g') || self.has_flag('y') { *self.last_index.borrow_mut() = smelt_utf16_index_of_byte(haystack, start + matched.end()); }
        Some(SmeltMatch::from_captures(&regex, &captures, haystack, start, self.has_flag('d')))
    }
    /// Return concrete `SmeltMatch` results for String.prototype.matchAll.
    pub fn match_all_indices(&self, haystack: &JsStr) -> Vec<SmeltMatch> {
        let haystack = &*haystack.to_string_lossy();
        let regex = self.compiled();
        regex.captures_iter(haystack).filter_map(Result::ok).filter_map(|captures| {
            captures.get(0)?;
            Some(SmeltMatch::from_captures(&regex, &captures, haystack, 0, self.has_flag('d')))
        }
        ).collect::<Vec<_>>()
    }
    /// `String.prototype.search`: the code unit index of the first match, if any.
    pub fn search_index(&self, haystack: &JsStr) -> Option<usize> {
        let haystack = haystack.to_string_lossy();
        let found = self.compiled().find(&haystack).ok().flatten()?;
        Some(smelt_utf16_index_of_byte(&haystack, found.start()))
    }
    /// Test this RegExp against a string with JavaScript lastIndex updates.
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
///
/// A JavaScript RegExp object owns three observable data properties:
/// `source`, `flags` and the writable `lastIndex`. All three must cross
/// the boundary, or a round trip through erased dataflow (`clone(re)`,
/// `structuredClone`, a `Record<string, unknown>` bag) silently resets
/// `lastIndex` to 0.
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
///
/// The exact inverse of the adapter above: a marker record restores
/// `source`, `flags` and `lastIndex`; a bare string is the `new
/// RegExp(str)` spelling and yields a flagless pattern. Anything else
/// answers the empty pattern, matching `new RegExp(undefined)`.
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

// @smelt:item impl_Default_for_SmeltRegExp when dep_regex
impl Default for SmeltRegExp {
    /// Construct a RegExp that matches the empty string.
    fn default() -> Self {
        Self::new(String::new(), String::new())
    }
}

// @smelt:item-end
