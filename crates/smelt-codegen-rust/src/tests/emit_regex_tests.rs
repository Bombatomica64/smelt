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

/// A callback that returns a string is handed to the replacement as a closure.
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

    assert_eq!(rust_fn(&source, "localize"), r#"fn localize(en_number: f64) -> JsString {
let suffix: JsString = JsString::from("!");
let _smelt_tmp_2: SmeltRegExp = SmeltRegExp::new(JsString::from("\\d"), JsString::from("g"));
let _smelt_tmp_3: JsString = JsString::from(smelt_number_to_string(en_number));
let _smelt_tmp_5: JsString = _smelt_tmp_2.replace_with(&_smelt_tmp_3, true, |smelt_match: &SmeltMatch| ({
let suffix = suffix.clone();
move |closure_arg_0: JsString| {
let _smelt_tmp_2: JsString = closure_arg_0.clone() + &suffix.clone();
_smelt_tmp_2.clone()
}
})(smelt_match.group_owned(0).unwrap_or_default()));
return _smelt_tmp_5;
}"#);
}

/// A replacer whose result is dynamic is converted with JavaScript `ToString`.
#[test]
fn coerces_non_string_regex_replace_callback_result() {
    let source = source_for(
        r#"
export function replaceAll(str: string, table: unknown): string {
  return str.replace(/[&<>"']/g, (match) => (table as Record<string, any>)[match]);
}
"#,
    );

    assert_eq!(rust_fn(&source, "replace_all"), r#"fn replace_all(str: JsString, table: SmeltUnknown) -> JsString {
let _smelt_tmp_2: SmeltRegExp = SmeltRegExp::new(JsString::from("[&<>\"']"), JsString::from("g"));
let _smelt_tmp_4: JsString = _smelt_tmp_2.replace_with(&str.clone(), true, |smelt_match: &SmeltMatch| smelt_erased_to_js_string(&(({
let table = table.clone();
move |closure_arg_0: JsString| {
let _smelt_tmp_2: SmeltRecord<JsString, SmeltUnknown> = match (table.clone()).into_smelt_unknown() { SmeltUnknown::Object(smelt_value) => smelt_value.smelt_shared_record(), SmeltUnknown::Array(smelt_values) => smelt_values.own_entries().into_iter().map(|(smelt_key, smelt_value)| (JsString::from(smelt_key), smelt_value)).collect(), SmeltUnknown::String(smelt_value) => smelt_value.units().iter().enumerate().map(|(smelt_index, smelt_unit)| (JsString::from(smelt_index.to_string()), SmeltUnknown::String(JsString::from_units(&[*smelt_unit])))).collect(), SmeltUnknown::Function(smelt_value) => SmeltRecord::from([(JsString::from("__smelt_call"), SmeltUnknown::Function(smelt_value))]), _ => SmeltRecord::new() };
smelt_record_property(&_smelt_tmp_2, &closure_arg_0.clone())
}
})(smelt_match.group_owned(0).unwrap_or_default()))));
return _smelt_tmp_4;
}"#);
}

/// `match` on an erased receiver converts the haystack from its runtime tag.
#[test]
fn emits_regex_find_with_erased_haystack_string_coercion() {
    let source = source_for(
        r"
function matchUnknown(value: unknown): string[] | undefined {
  return (value as any).match(/a+/);
}
",
    );

    assert_eq!(rust_fn(&source, "match_unknown"), r#"fn match_unknown(value: SmeltUnknown) -> Option<SmeltList<JsString>> {
let _smelt_tmp_1: SmeltRegExp = SmeltRegExp::new(JsString::from("a+"), JsString::new());
let _smelt_tmp_2: Option<SmeltMatch> = _smelt_tmp_1.match_string(&match value.clone() { SmeltUnknown::String(smelt_text) => smelt_text, smelt_value => JsString::from(match smelt_value {   SmeltUnknown::Object(smelt_value) if smelt_host_buffer_is_view(&SmeltUnknown::Object(smelt_value.clone())) => smelt_host_buffer_elements(&SmeltUnknown::Object(smelt_value)).unwrap_or_default().into_iter().map(|smelt_element| match smelt_element { SmeltUnknown::Number(smelt_element) => smelt_element.to_string(), _ => String::new() }).collect::<Vec<_>>().join(","), SmeltUnknown::Null => "null".to_owned(), SmeltUnknown::Undefined => "undefined".to_owned(), SmeltUnknown::Bool(smelt_value) => smelt_value.to_string(), SmeltUnknown::Number(smelt_value) => smelt_number_to_string(smelt_value), SmeltUnknown::String(smelt_value) => smelt_value.to_string(), SmeltUnknown::Symbol(smelt_value) => smelt_symbol_to_string(&smelt_value), SmeltUnknown::Object(smelt_value) if smelt_value.contains_key("__smelt_regexp") => smelt_regexp_literal(&smelt_value), SmeltUnknown::Object(value) if value.contains_key("__smelt_error") => { let smelt_error_name = match value.get("name") { Some(SmeltUnknown::String(name)) => name.to_string(), _ => match value.get("__smelt_error") { Some(SmeltUnknown::String(class)) => class.to_string(), _ => "Error".to_owned() } }; let smelt_error_message = match value.get("message") { Some(SmeltUnknown::String(message)) => message.to_string(), _ => String::new() }; if smelt_error_message.is_empty() { smelt_error_name } else if smelt_error_name.is_empty() { smelt_error_message } else { format!("{smelt_error_name}: {smelt_error_message}") } }, SmeltUnknown::Array(smelt_values) => smelt_erased_to_js_string(&SmeltUnknown::Array(smelt_values)).to_string_lossy(), SmeltUnknown::Object(_) => "[object Object]".to_owned(), SmeltUnknown::Function(_) => "function () { [native code] }".to_owned(), SmeltUnknown::Promise(_) => "[object Promise]".to_owned() }) });
return _smelt_tmp_2.map(|smelt_value| SmeltList::new((smelt_value).to_array_view().into_iter().map(|smelt_group| smelt_group.unwrap_or_default()).collect::<Vec<_>>()));
}"#);
}

/// A regex held in a local keeps its flags through `match`.
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

    assert_eq!(rust_fn(&source, "parts"), r#"fn parts(value: JsString) -> Option<SmeltList<JsString>> {
let tokens: SmeltRegExp = SmeltRegExp::new(JsString::from("a+|b"), JsString::from("g"));
let _smelt_tmp_3: Option<SmeltMatch> = tokens.match_string(&value.clone());
return _smelt_tmp_3.map(|smelt_value| SmeltList::new((smelt_value).to_array_view().into_iter().map(|smelt_group| smelt_group.unwrap_or_default()).collect::<Vec<_>>()));
}"#);
}

/// A statically global literal takes the whole-match list directly.
#[test]
fn prelude_string_match_answers_match_value_for_both_regex_kinds() {
    let source = source_for(
        r"
function words(text: string): number {
  return text.match(/[a-z]+/g)?.length ?? 0;
}
",
    );

    assert_eq!(rust_fn(&source, "words"), r#"fn words(text: JsString) -> f64 {
let mut _smelt_tmp_5: Option<f64>;
let _smelt_tmp_6: SmeltList<JsString>;
let _smelt_tmp_7: f64;
let _smelt_tmp_8: f64;
let _smelt_tmp_1: SmeltRegExp = SmeltRegExp::new(JsString::from("[a-z]+"), JsString::from("g"));
let _smelt_tmp_2: Option<SmeltList<JsString>> = _smelt_tmp_1.match_all_strings(&text.clone()).map(SmeltList::from);
let _smelt_tmp_3: bool = _smelt_tmp_2.clone().is_none();
let _smelt_tmp_4: bool = !(_smelt_tmp_3);
let mut _smelt_tmp_5: Option<f64> = None::<f64>;
if _smelt_tmp_4 {
_smelt_tmp_6 = Into::<SmeltList<_>>::into(_smelt_tmp_2.clone().expect("optional value was absent after narrowing"));
_smelt_tmp_7 = _smelt_tmp_6.len() as f64;
_smelt_tmp_5 = Some(_smelt_tmp_7);
} else {
_smelt_tmp_5 = None::<f64>;
}
_smelt_tmp_8 = _smelt_tmp_5.clone().unwrap_or(0.0);
return _smelt_tmp_8;
}"#);
}

/// Each array element keeps its own flags and reads them with `has_flag`.
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

    assert_eq!(rust_fn(&source, "main"), r#"fn main() {
let _smelt_tmp_5: SmeltRegExp = SmeltRegExp::new(JsString::from("x"), JsString::from("u"));
let _smelt_tmp_6: SmeltRegExp = SmeltRegExp::new(JsString::from("x"), JsString::from("gimu"));
let patterns: SmeltList<SmeltRegExp> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<SmeltRegExp> = vec![_smelt_tmp_5.clone(), _smelt_tmp_6.clone()]; smelt_list_items }));
let pattern: SmeltRegExp = patterns.borrow().get({ let smelt_normalized = 1.0 as i64; usize::try_from(smelt_normalized).unwrap_or(usize::MAX) }).cloned().unwrap_or_else(|| SmeltRegExp::new(String::new(), String::new()));
let uses_global: bool = pattern.has_flag('g');
let ignores_case: bool = pattern.has_flag('i');
let uses_multiline: bool = pattern.has_flag('m');
return;
}"#);
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

/// `source` and `flags` of a concrete `RegExp` are typed fields, not erased reads.
#[test]
fn regexp_metadata_fields_resolve_to_concrete_types() {
    let source = source_for(
        r"
export function cloneRe(obj: unknown): RegExp {
  const regExp = obj as unknown as RegExp;
  return new RegExp(regExp.source, regExp.flags);
}
",
    );

    assert_eq!(rust_fn(&source, "clone_re"), r#"fn clone_re(obj: SmeltUnknown) -> Result<SmeltRegExp, Box<dyn std::error::Error>> {
let reg_exp: SmeltRegExp = <SmeltRegExp as SmeltFromUnknown>::smelt_from_unknown(obj.clone());
let _smelt_tmp_3: SmeltRegExp = smelt_regexp_compile(JsString::from(&*reg_exp.source).clone(), JsString::from(&*reg_exp.flags).clone())?;
return Ok(_smelt_tmp_3);
}"#);
}

/// A record of regex arrays keeps its entries at their concrete type.
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

    assert_eq!(rust_fn(&source, "main"), r#"fn main() {
let selected: SmeltUnknown;
let _smelt_tmp_2: SmeltRegExp = SmeltRegExp::new(JsString::from("^j"), JsString::from("i"));
let _smelt_tmp_3: SmeltRegExp = SmeltRegExp::new(JsString::from("^f"), JsString::from("i"));
let _smelt_tmp_4: SmeltList<SmeltRegExp> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<SmeltRegExp> = vec![_smelt_tmp_2.clone(), _smelt_tmp_3.clone()]; smelt_list_items }));
let _smelt_tmp_5: SmeltRegExp = SmeltRegExp::new(JsString::from("^ja"), JsString::from("i"));
let _smelt_tmp_6: SmeltRegExp = SmeltRegExp::new(JsString::from("^f"), JsString::from("i"));
let _smelt_tmp_7: SmeltList<SmeltRegExp> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<SmeltRegExp> = vec![_smelt_tmp_5.clone(), _smelt_tmp_6.clone()]; smelt_list_items }));
let parse_month_patterns: SmeltRecord<JsString, SmeltList<SmeltRegExp>> = SmeltRecord::from([(JsString::from("narrow"), _smelt_tmp_4), (JsString::from("any"), _smelt_tmp_7)]);
let _smelt_tmp_9: Args = Args { parse_patterns: parse_month_patterns, default_parse_width: JsString::from("any") };
let _smelt_tmp_10: SmeltUnknown = use_(_smelt_tmp_9);
selected = _smelt_tmp_10;
return;
}"#);
}

/// `matchAll` yields concrete `SmeltMatch` values and the erasure adapter is not emitted.
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

    assert_eq!(rust_fn(&source, "main"), r#"fn main() {
let mut __smelt_for_item: SmeltMatch;
let mut index: f64;
let mut _smelt_tmp_7: usize;
let mut _smelt_tmp_8: bool;
let pattern: SmeltRegExp = SmeltRegExp::new(JsString::from(",|\\."), JsString::from("gu"));
let matches: SmeltList<SmeltMatch> = Into::<SmeltList<_>>::into(pattern.match_all_indices(&JsString::from("a,b.c")));
let mut _smelt_tmp_6: usize = 0;
loop {
_smelt_tmp_7 = matches.len();
_smelt_tmp_8 = _smelt_tmp_6 < _smelt_tmp_7;
if !(_smelt_tmp_8) { break; }
__smelt_for_item = matches.borrow().get(_smelt_tmp_6).cloned().unwrap_or_else(|| SmeltMatch::default());
index = __smelt_for_item.index();
let _ = { println!("{}", smelt_console_number(index)); };
_smelt_tmp_6 = _smelt_tmp_6 + 1;
}
return;
}"#);
    assert!(!source.contains("impl IntoSmeltUnknown for SmeltMatch"), "{source}");
}

/// `exec` stays a concrete `Option<SmeltMatch>` and the erasure adapter is not emitted.
#[test]
fn emits_regex_exec_as_concrete_smelt_match() {
    let source = source_for(
        r#"
const re = /(?<year>\d{4})-(\d{2})/;
const m = re.exec("2024-07");
console.log(m === null);
"#,
    );

    assert_eq!(rust_fn(&source, "main"), r#"fn main() {
let re: SmeltRegExp = SmeltRegExp::new(JsString::from("(?<year>\\d{4})-(\\d{2})"), JsString::new());
let m: Option<SmeltMatch> = re.exec(&JsString::from("2024-07"));
let _smelt_tmp_4: bool = m.is_none();
let _ = { println!("{}", _smelt_tmp_4); };
return;
}"#);
    assert!(!source.contains("impl IntoSmeltUnknown for SmeltMatch"), "{source}");
}

/// Numbered groups, named groups, `index`, `input` and destructuring all read typed accessors.
#[test]
fn types_regex_match_consumer_reads_against_smelt_match() {
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

    assert_eq!(rust_fn(&source, "main"), r#"fn main() {
let mut found: SmeltMatch;
let mut whole: Option<JsString>;
let mut letter: Option<JsString>;
let mut digit: Option<JsString>;
let mut full: Option<JsString>;
let mut first: Option<JsString>;
let mut _smelt_tmp_11: usize;
let mut _smelt_tmp_12: bool;
let mut _smelt_tmp_14: SmeltMatch;
let pattern: SmeltRegExp = SmeltRegExp::new(JsString::from("(?<letter>[a-z])(\\d)?"), JsString::from("g"));
let matches: SmeltList<SmeltMatch> = Into::<SmeltList<_>>::into(pattern.match_all_indices(&JsString::from("a1 b")));
let mut _smelt_tmp_10: usize = 0;
loop {
_smelt_tmp_11 = matches.len();
_smelt_tmp_12 = _smelt_tmp_10 < _smelt_tmp_11;
if !(_smelt_tmp_12) { break; }
found = matches.borrow().get(_smelt_tmp_10).cloned().unwrap_or_else(|| SmeltMatch::default());
whole = found.clone().group_owned(0.0 as usize);
_smelt_tmp_14 = found.clone();
letter = _smelt_tmp_14.named_group_owned("letter").clone();
digit = found.clone().group_owned(2.0 as usize);
full = found.clone().group_owned(0.0 as usize);
first = found.clone().group_owned(1.0 as usize);
let _ = { println!("{}", match &whole { Some(smelt_value) => format!("{}", smelt_value), None => "undefined".to_owned() }); };
let _ = { println!("{}", match &letter { Some(smelt_value) => format!("{}", smelt_value), None => "undefined".to_owned() }); };
let _ = { println!("{}", match &digit { Some(smelt_value) => format!("{}", smelt_value), None => "undefined".to_owned() }); };
let _ = { println!("{}", match &full { Some(smelt_value) => format!("{}", smelt_value), None => "undefined".to_owned() }); };
let _ = { println!("{}", match &first { Some(smelt_value) => format!("{}", smelt_value), None => "undefined".to_owned() }); };
let _ = { println!("{}", smelt_console_number(found.index())); };
let _ = { println!("{}", found.input_owned().clone()); };
_smelt_tmp_10 = _smelt_tmp_10 + 1;
}
return;
}"#);
    assert!(!source.contains("impl IntoSmeltUnknown for SmeltMatch"), "{source}");
}

/// Only a match flowing into `unknown` crosses the explicit adapter; the adjacent typed read stays concrete.
#[test]
fn erases_regex_match_value_only_at_a_dynamic_boundary() {
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

    assert_eq!(rust_fn(&source, "main"), r#"fn main() {
let letter: Option<JsString>;
let boxed: SmeltUnknown;
let _smelt_tmp_7: SmeltMatch;
let _smelt_tmp_8: SmeltMatch;
let _smelt_tmp_9: SmeltMatch;
let re: SmeltRegExp = SmeltRegExp::new(JsString::from("(?<letter>[a-z])"), JsString::new());
let m: Option<SmeltMatch> = re.exec(&JsString::from("a"));
let _smelt_tmp_6: bool = !(m.clone().is_none());
if _smelt_tmp_6 {
_smelt_tmp_7 = m.clone().expect("optional value was absent after narrowing");
_smelt_tmp_8 = _smelt_tmp_7.clone();
letter = _smelt_tmp_8.named_group_owned("letter").clone();
_smelt_tmp_9 = m.clone().expect("optional value was absent after narrowing");
boxed = _smelt_tmp_9.clone().into_smelt_unknown();
let _ = { println!("{}", match &letter { Some(smelt_value) => format!("{}", smelt_value), None => "undefined".to_owned() }); };
let _ = { println!("{}", SmeltInspect::smelt_console_text(&boxed)); };
}
return;
}"#);
    assert!(source.contains("impl IntoSmeltUnknown for SmeltMatch"), "{source}");
}

/// `[^]` is ECMAScript syntax the engine reads itself, so the pattern reaches it untranslated.
#[test]
fn emits_javascript_any_character_regex_translation() {
    let source = source_for(
        r"
function inner(value: string): string[] | undefined {
  return value.match(/^'([^]*?)'?$/);
}
",
    );

    assert_eq!(rust_fn(&source, "inner"), r#"fn inner(value: JsString) -> Option<SmeltList<JsString>> {
let _smelt_tmp_1: SmeltRegExp = SmeltRegExp::new(JsString::from("^'([^]*?)'?$"), JsString::new());
let _smelt_tmp_2: Option<SmeltMatch> = _smelt_tmp_1.match_string(&value.clone());
return _smelt_tmp_2.map(|smelt_value| SmeltList::new((smelt_value).to_array_view().into_iter().map(|smelt_group| smelt_group.unwrap_or_default()).collect::<Vec<_>>()));
}"#);
}

/// A `lastIndex` write targets the backing cell through `borrow_mut`, narrowing the number to `usize`.
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

    assert_eq!(rust_fn(&source, "run"), r#"fn run() -> f64 {
let mut regex: SmeltRegExp = SmeltRegExp::new(JsString::from("a"), JsString::from("g"));
*regex.last_index.borrow_mut() = (10.0) as usize;
return (*regex.last_index.borrow() as f64);
}"#);
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
