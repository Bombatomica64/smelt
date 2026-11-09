//! Generated Rust for regular expressions.

use super::*;

#[test]
fn coerces_string_values_to_regexp_destinations() {
    let source = source_for(
        r#"
const pattern: RegExp = "\\d+";
"#,
    );

    assert!(
        source.contains("SmeltRegExp::new(JsString::from(\"\\\\d+\"), String::new())"),
        "{source}"
    );
}

#[test]
fn emits_regex_replace_callbacks_as_closures() {
    let source = source_for(
        r#"
export function localize(enNumber: number): string {
  const suffix = "!";
  return enNumber.toString().replace(/\d/g, (match) => match + suffix);
}
"#,
    );

    assert!(source.contains("replace_all"), "{source}");
    assert!(source.contains("|closure_arg_0: JsString|"), "{source}");
    assert!(
        !source.contains("|caps: &regex::Captures<'_>| (_smelt_tmp_"),
        "{source}"
    );
    // A callback that already returns `String` is a valid `Replacer`, so it is
    // handed to `replace_all` without a ToString wrapper.
    assert!(
        !source.contains("|caps: &regex::Captures<'_>| match ("),
        "{source}"
    );
}

#[test]
fn coerces_non_string_regex_replace_callback_result() {
    // A replacer callback whose result is not a `String` is not a valid
    // `Replacer`, so the regex replacement must coerce it with the JavaScript
    // `ToString` match. Here the callback returns an `unknown` read out of an
    // erased parameter, the shape that carries no static type at all.
    //
    // This test used to spell the same idea as `htmlEscapes[match]` against a
    // module-level `Record<string, string>` constant. That read is NOT erased —
    // the constant is a concrete string-to-string map — and it only looked
    // erased because a callback could not resolve an object constant at all and
    // silently read it as `null`. Since that defect was fixed the es-toolkit
    // `escape` shape produces a `String` directly and needs no coercion, so the
    // coercion rule is pinned here on a value that is genuinely dynamic.
    let source = source_for(
        r#"
export function replaceAll(str: string, table: unknown): string {
  return str.replace(/[&<>"']/g, (match) => (table as Record<string, unknown>)[match]);
}
"#,
    );

    // `/g` replaces every match; the replacement is JavaScript ToString of the
    // erased answer.
    assert!(
        source.contains(", true, |smelt_match: &SmeltMatch| smelt_erased_to_js_string(&"),
        "{source}"
    );
    assert!(
        source.contains("SmeltUnknown::Undefined => \"undefined\".to_owned()"),
        "{source}"
    );
}

#[test]
fn emits_regex_find_with_erased_haystack_string_coercion() {
    let source = source_for(
        r"
function matchUnknown(value: unknown): string[] | undefined {
  return (value as any).match(/a+/);
}
",
    );

    assert!(
        source.contains("SmeltUnknown::String(smelt_value) => smelt_value.to_string(), SmeltUnknown::Symbol(smelt_value) => smelt_symbol_to_string(&smelt_value)"),
        "{source}"
    );
    assert!(source.contains(".match_string(&match "), "{source}");
}

#[test]
fn emits_string_match_with_regexp_flags_preserved() {
    let source = source_for(
        r"
function parts(value: string): string[] | undefined {
  const tokens = /a+|b/g;
  return value.match(tokens);
}
",
    );

    assert!(
        source.contains("SmeltRegExp::new(JsString::from(\"a+|b\"), JsString::from(\"g\"))"),
        "{source}"
    );
    assert!(source.contains("tokens.match_string(&"), "{source}");
}

/// The prelude's `match_string` answers a match value: a non-global regex is
/// `exec`, a global one is built by `SmeltMatch::from_global_matches`. A
/// statically global literal skips the match value entirely.
#[test]
fn prelude_string_match_answers_match_value_for_both_regex_kinds() {
    let source = source_for(
        r"
function words(text: string): number {
  return text.match(/[a-z]+/g)?.length ?? 0;
}
",
    );

    assert!(
        source.contains("pub fn match_string(&self, haystack: &JsStr) -> Option<SmeltMatch>"),
        "{source}"
    );
    assert!(source.contains("if !self.has_flag('g') { return self.exec(haystack); }"), "{source}");
    assert!(source.contains("fn from_global_matches(matches: Vec<JsString>, input: &JsStr) -> Self"), "{source}");
    assert!(source.contains("pub fn to_array_view(&self) -> SmeltList<Option<JsString>>"), "{source}");
    // The literal is statically global, so the call site takes the direct
    // whole-match list rather than a match value.
    assert!(source.contains(".match_all_strings(&"), "{source}");
}

#[test]
fn emits_regexp_array_elements_with_flags_preserved() {
    let source = source_for(
        r"
const patterns = [/x/u, /x/gimu];
const pattern = patterns[1];
const usesGlobal = pattern.global;
const ignoresCase = pattern.ignoreCase;
const usesMultiline = pattern.multiline;
",
    );

    assert!(
        source.contains("SmeltRegExp::new(JsString::from(\"x\"), JsString::from(\"u\"))"),
        "{source}"
    );
    assert!(
        source.contains("SmeltRegExp::new(JsString::from(\"x\"), JsString::from(\"gimu\"))"),
        "{source}"
    );
    assert!(source.contains(".has_flag('g')"), "{source}");
    assert!(source.contains(".has_flag('i')"), "{source}");
    assert!(source.contains(".has_flag('m')"), "{source}");
}

#[test]
fn emits_erased_regexp_test_with_flags_preserved() {
    let source = source_for(
        r#"
const patterns: unknown = [/^n/i];
const first = (patterns as any)[0];
const matches = first.test("Nov");
"#,
    );

    assert!(
        source.contains("SmeltRegExp::new(source.to_string(), flags).test(&haystack)"),
        "{source}"
    );
    assert!(
        source.contains("(\"flags\".to_owned(), SmeltUnknown::String(JsString::from(&*self.flags)))"),
        "{source}"
    );
}

#[test]
fn regexp_metadata_fields_resolve_to_concrete_types() {
    // A data-property read on a concrete `RegExp` receiver must type as the
    // concrete `SmeltRegExp` field (`source`/`flags` are `String`), not the
    // erased `Unknown` boundary. Otherwise passing the read into
    // `new RegExp(...)`'s `String` parameters emits a stringify-of-
    // `SmeltUnknown` match against an already-`String` scrutinee (E0308).
    let source = source_for(
        r"
export function cloneRe(obj: unknown): RegExp {
  const regExp = obj as unknown as RegExp;
  return new RegExp(regExp.source, regExp.flags);
}
",
    );

    assert!(
        source.contains("smelt_regexp_compile(JsString::from(&*reg_exp.source).clone(), JsString::from(&*reg_exp.flags).clone())?"),
        "{source}"
    );
    // The erased stringify coercion must not be applied to the concrete
    // `String` field reads.
    assert!(
        !source.contains("match reg_exp.source"),
        "{source}"
    );
}

#[test]
fn preserves_regex_arrays_inside_static_object_consts() {
    let source = source_for(
        r#"
type Args = {
  parsePatterns: Record<string, readonly RegExp[]>;
  defaultParseWidth: string;
};

function use(args: Args): unknown {
  return args.parsePatterns[args.defaultParseWidth];
}

const parseMonthPatterns = {
  narrow: [/^j/i, /^f/i] as const,
  any: [/^ja/i, /^f/i] as const,
};

const selected = use({
  parsePatterns: parseMonthPatterns,
  defaultParseWidth: "any",
});
"#,
    );

    assert!(
        source.contains("JsString::from(\"any\")")
            && source.contains("SmeltRegExp::new(JsString::from(\"^ja\"), JsString::from(\"i\"))"),
        "static regex-array records should preserve their entries: {source}"
    );
    assert!(
        !source.contains("parseMonthPatterns = SmeltRecord::from([])"),
        "static regex-array records must not collapse to an empty record: {source}"
    );
}

#[test]
fn emits_string_match_all_as_indexed_regexp_iteration() {
    let source = source_for(
        r#"
const pattern = /,|\./gu;
const matches = "a,b.c".matchAll(pattern);
for (const { index } of matches) {
  console.log(index);
}
"#,
    );

    assert!(source.contains(".match_all_indices(&"));
    assert!(source.contains("pub fn match_all_indices(&self"));
    // matchAll yields concrete `SmeltMatch` values whose consumer reads stay
    // typed: the destructured `.index` read is a typed `.index()` accessor, and
    // the list keeps its concrete `SmeltMatch` element type. The value never
    // crosses the erased `SmeltUnknown` boundary, so no `into_smelt_unknown`
    // adapter is emitted for this program.
    assert!(source.contains("pub struct SmeltMatch"));
    assert!(
        source.contains("SmeltList<SmeltMatch>"),
        "matchAll result should keep its concrete SmeltMatch element type: {source}"
    );
    assert!(
        source.contains(".index()"),
        "destructured `.index` read should use the typed accessor: {source}"
    );
    // No erasure adapter is applied at the matchAll site, and the adapter impl
    // is not emitted for this program (the only textual mention of
    // `SmeltMatch::into_smelt_unknown` is the type's docstring).
    assert!(
        !source.contains(".map(SmeltMatch::into_smelt_unknown)"),
        "matchAll must not erase its concrete result: {source}"
    );
    assert!(
        !source.contains("impl IntoSmeltUnknown for SmeltMatch"),
        "typed match reads must not require the SmeltUnknown erasure adapter: {source}"
    );
    assert!(
        source.contains("-> Vec<SmeltMatch>"),
        "match_all_indices should return concrete SmeltMatch values: {source}"
    );
}

#[test]
fn emits_regex_exec_as_concrete_smelt_match() {
    let source = source_for(
        r#"
const re = /(?<year>\d{4})-(\d{2})/;
const m = re.exec("2024-07");
console.log(m === null);
"#,
    );

    // exec returns a concrete `Option<SmeltMatch>` and the result stays typed:
    // the `m === null` check is a plain `.is_none()` on the concrete option, so
    // the match never crosses the erased `SmeltUnknown` boundary and no
    // `into_smelt_unknown` adapter is emitted for this program.
    assert!(
        source.contains("pub fn exec(&self, haystack: &JsStr) -> Option<SmeltMatch>"),
        "exec should return a concrete SmeltMatch option: {source}"
    );
    assert!(source.contains("pub struct SmeltMatch"));
    assert!(
        source.contains(".exec(&JsString::from(\"2024-07\"))")
            && !source.contains(".map(SmeltMatch::into_smelt_unknown)"),
        "typed exec result must not erase to SmeltUnknown: {source}"
    );
    assert!(
        !source.contains("impl IntoSmeltUnknown for SmeltMatch"),
        "typed exec result must not require the SmeltUnknown erasure adapter: {source}"
    );
    assert!(
        source.contains(".is_none()"),
        "`m === null` should lower to a typed `.is_none()` check: {source}"
    );
    // The numbered-group / named-group / index / input shape is modeled with
    // concrete fields, not assembled as an untyped object during exec.
    assert!(source.contains("groups: ::std::rc::Rc<Vec<Option<JsString>>>"));
    assert!(source.contains("named: ::std::rc::Rc<::std::collections::HashMap<String, Option<JsString>>>"));
    assert!(source.contains("fn from_regress(found: &regress::Match, input: &JsStr, spans: bool)"));
}

#[test]
fn types_regex_match_consumer_reads_against_smelt_match() {
    // The consumer-side reads of a match value are typed against the concrete
    // `SmeltMatch` type: numbered groups (`m[0]`, `m[2]`) read `group_owned`,
    // named groups (`m.groups.letter`) read `named_group_owned`, `m.index` and
    // `m.input` read their typed accessors, and array destructuring binds the
    // numbered groups. None of these cross the erased `SmeltUnknown` boundary.
    let source = source_for(
        r#"
const pattern = /(?<letter>[a-z])(\d)?/g;
const matches = "a1 b".matchAll(pattern);
for (const found of matches) {
  const whole = found[0];
  const letter = found.groups.letter;
  const digit = found[2];
  const [full, first] = found;
  console.log(whole);
  console.log(letter);
  console.log(digit);
  console.log(full);
  console.log(first);
  console.log(found.index);
  console.log(found.input);
}
"#,
    );

    // Numbered group reads (including the destructured bindings) route through
    // the typed `group_owned` accessor; the whole match is index 0.
    assert!(
        source.contains(".group_owned(0.0 as usize)"),
        "numbered group read should use group_owned: {source}"
    );
    assert!(
        source.contains(".group_owned(2.0 as usize)"),
        "optional numbered group read should use group_owned: {source}"
    );
    // Named group reads route through `named_group_owned`.
    assert!(
        source.contains(".named_group_owned(\"letter\")"),
        "named group read should use named_group_owned: {source}"
    );
    // `.index` / `.input` read the typed accessors.
    assert!(
        source.contains(".index()"),
        "match index read should use the typed accessor: {source}"
    );
    assert!(
        source.contains(".input_owned()"),
        "match input read should use the typed accessor: {source}"
    );
    // The match list keeps its concrete element type and the reads never erase
    // to `SmeltUnknown`.
    assert!(
        source.contains("SmeltList<SmeltMatch>"),
        "matchAll result should keep its concrete SmeltMatch element type: {source}"
    );
    assert!(
        !source.contains(".map(SmeltMatch::into_smelt_unknown)"),
        "typed match reads must not erase the matchAll result: {source}"
    );
    assert!(
        !source.contains("impl IntoSmeltUnknown for SmeltMatch"),
        "typed match reads must not require the SmeltUnknown erasure adapter: {source}"
    );
}

#[test]
fn erases_regex_match_value_only_at_a_dynamic_boundary() {
    // A match value that genuinely flows into `unknown` is erased through the
    // single explicit `into_smelt_unknown` adapter, while the sibling typed
    // reads stay concrete.
    let source = source_for(
        r#"
const re = /(?<letter>[a-z])/;
const m = re.exec("a");
if (m !== null) {
  const letter = m.groups.letter;
  const boxed: unknown = m;
  console.log(letter);
  console.log(boxed);
}
"#,
    );

    assert!(
        source.contains(".named_group_owned(\"letter\")"),
        "typed named group read should stay concrete: {source}"
    );
    assert!(
        source.contains(".clone().into_smelt_unknown()"),
        "a match value flowing into unknown should use the explicit erasure adapter: {source}"
    );
    assert!(
        source.contains("impl IntoSmeltUnknown for SmeltMatch"),
        "the match erasure adapter should be emitted when the boundary is used: {source}"
    );
}

#[test]
fn emits_javascript_any_character_regex_translation() {
    let source = source_for(
        r"
function inner(value: string): string[] | undefined {
  return value.match(/^'([^]*?)'?$/);
}
",
    );

    // `[^]` (any code unit, line terminators included) is ECMAScript syntax
    // `regress` reads itself, so the pattern reaches it untranslated. (This
    // used to pin a `[^]` -> `(?s:.)` rewrite for the UTF-8 engine.)
    assert!(source.contains("[^]*?"), "{source}");
    assert!(!source.contains("(?s:.)"), "{source}");
    assert!(source.contains(".match_string(&"), "{source}");
}

/// A `RegExp.lastIndex` write must target the backing `RefCell<usize>` through
/// `borrow_mut()`, narrowing the numeric right-hand side back to `usize`. The
/// former read-path text `(*regex.last_index.borrow() as f64)` is not a valid
/// assignment target (E0070).
#[test]
fn regexp_last_index_write_targets_borrow_mut() {
    let source = source_for(
        r"
export function run(): number {
  const regex = /a/g;
  regex.lastIndex = 10;
  return regex.lastIndex;
}
",
    );

    assert!(
        source.contains("*regex.last_index.borrow_mut() = (") && source.contains(") as usize;"),
        "lastIndex write should go through borrow_mut with a usize cast\n{source}"
    );
    assert!(
        !source.contains("borrow() as f64) = "),
        "the invalid cast-as-lvalue read form must not be used for writes\n{source}"
    );
}

/// A module-level regex `const` referenced from several functions must compile
/// its pattern exactly once.
///
/// The TypeScript frontend inlines an importable `const` initializer into every
/// referencing body, so the `SmeltRegExp::new(..)` construction is pasted at each
/// use site. That construction must stay per-use: `SmeltRegExp` carries JS
/// reference identity (`id`) and observable mutable state (`lastIndex`), and
/// `Clone` preserves both, so handing every use a clone of one shared instance
/// would fuse distinct source objects. What *is* shared is the pure half — the
/// compiled `fancy_regex` automaton, a function of the pattern text alone — which
/// the prelude memoizes in `SMELT_REGEX_CACHE`. The invariant this test pins is
/// therefore: exactly ONE `fancy_regex` compile call site exists in the whole
/// emitted crate (the memo), no matter how many times the const is inlined.
#[test]
fn module_level_regex_const_compiles_its_pattern_once() {
    let source = source_for(
        r"
const CASE_SPLIT_PATTERN = /[a-z]+|[0-9]+/g;

export function first(text: string): number {
  return text.split(CASE_SPLIT_PATTERN).length;
}

export function second(text: string): number {
  return text.split(CASE_SPLIT_PATTERN).length;
}
",
    );

    assert!(
        source.contains("static SMELT_REGRESS_CACHE:"),
        "the prelude must declare the compiled-automaton memo\n{source}"
    );
    // One compile site, behind the memo: `with_flags` for a `u`/`v` pattern
    // and `from_unicode` over code units otherwise, one call each.
    assert_eq!(
        source.matches("regress::Regex::with_flags(").count() + source.matches("regress::Regex::from_unicode(").count(),
        2,
        "the emitted crate must compile patterns only inside the memo, \
         so an inlined module-level const never recompiles its pattern\n{source}"
    );
    assert!(
        source.matches("SmeltRegExp::new(").count() >= 2,
        "each use site must still build its own `SmeltRegExp` wrapper so JS \
         reference identity and `lastIndex` stay per-object\n{source}"
    );
    assert!(
        source.contains("cache.borrow().get(&key).cloned()")
            && source.contains("cache.borrow_mut().insert(key, compiled.clone())"),
        "`try_compiled` must read through and populate the memo\n{source}"
    );
}
