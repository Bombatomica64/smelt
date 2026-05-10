# Stdlib Mapping

This document lists direct stdlib mappings currently lowered through HIR/MIR and emitted as inline Rust.

## TypeScript Strings

| Source API | HIR expression | MIR rvalue | Rust output | Supported arguments | Unsupported arguments | Known semantic differences |
| --- | --- | --- | --- | --- | --- | --- |
| `s.toLowerCase()` | `StringCase::Lower` | `StringCase::Lower` | `s.to_lowercase()` | No arguments | Any arguments | Locale-sensitive JS casing is not modeled. |
| `s.toUpperCase()` | `StringCase::Upper` | `StringCase::Upper` | `s.to_uppercase()` | No arguments | Any arguments | Locale-sensitive JS casing is not modeled. |
| `s.trim()` | `StringTrim::Both` | `StringTrim::Both` | `s.trim().to_owned()` | No arguments | Any arguments | Rust whitespace semantics are used. |
| `s.trimStart()` | `StringTrim::Start` | `StringTrim::Start` | `s.trim_start().to_owned()` | No arguments | Any arguments | Rust whitespace semantics are used. |
| `s.trimEnd()` | `StringTrim::End` | `StringTrim::End` | `s.trim_end().to_owned()` | No arguments | Any arguments | Rust whitespace semantics are used. |
| `s.includes(x)` | `StringContains` | `StringContains` | `s.contains(&x)` | One string argument | Optional start index | Rust substring matching is used. |
| `s.startsWith(x)` | `StringAffix::StartsWith` | `StringAffix::StartsWith` | `s.starts_with(&x)` | One string argument | Optional position | Rust prefix matching is used. |
| `s.endsWith(x)` | `StringAffix::EndsWith` | `StringAffix::EndsWith` | `s.ends_with(&x)` | One string argument | Optional length | Rust suffix matching is used. |
| `s.indexOf(x)` | `StringSearch::Find` | `StringSearch::Find` | `s.find(&x).map_or(-1.0, ...)` | One string argument | Optional `fromIndex` | Returns Rust byte offsets for now, not JS UTF-16 code-unit indexes. |
| `s.lastIndexOf(x)` | `StringSearch::RFind` | `StringSearch::RFind` | `s.rfind(&x).map_or(-1.0, ...)` | One string argument | Optional `fromIndex` | Returns Rust byte offsets for now, not JS UTF-16 code-unit indexes. |
| `s.replace(x, y)` | `StringReplace::First` | `StringReplace::First` | `s.replacen(&x, &y, 1)` | Two string arguments | Regex patterns and replacement callbacks | JS replacement-substitution patterns like `$1` are not modeled. |
| `s.repeat(n)` | `StringRepeat` | `StringRepeat` | `s.repeat(n as usize)` | One number argument | Negative, infinite, or range-error parity | Count is cast directly to `usize`; JS range/error semantics are deferred. |
| `s.padStart(len, pad?)` | `StringPad::Start` | `StringPad::Start` | Build padding with repeated pad chars before `s` | Numeric length and optional string pad | Non-string pad, wrong arity | Counts Rust chars rather than JS UTF-16 code units. |
| `s.padEnd(len, pad?)` | `StringPad::End` | `StringPad::End` | Build padding with repeated pad chars after `s` | Numeric length and optional string pad | Non-string pad, wrong arity | Counts Rust chars rather than JS UTF-16 code units. |
| `s.charAt(n)` | `StringCharAt` | `StringCharAt` | `s.chars().nth(n as usize).map(...).unwrap_or_default()` | One number argument | Default index coercion, negative/infinite edge parity | Uses Rust Unicode scalar indexes, not JS UTF-16 code-unit indexes. |
| `s.charCodeAt(n)` | `StringCharCodeAt` | `StringCharCodeAt` | `s.chars().nth(n as usize).map_or(f64::NAN, ...)` | One number argument | Default index coercion, negative/infinite edge parity | Uses Rust Unicode scalar values, not JS UTF-16 code units. |
| `s.at(n)` | `Index` | `Place::Index` read | `s.chars().nth(normalized_index).expect(...)` | One number argument, including negative indexes | Optional/coercive argument forms | Lowers through Python-style HIR indexing; out-of-bounds is a generated panic instead of JS `undefined`. Uses Rust Unicode scalar indexes. |
| `s.slice()` / `s.slice(start, end)` | `StringSlice` | `StringSlice` | `s.chars().skip(normalized_start).take(...).collect::<String>()` | Omitted, start, or start/end numeric args, including negative bounds | Third arg, `substring` | Bounds use Python-style negative-index normalization, matching JS `slice` for this supported subset. Uses Rust Unicode scalar indexes, not JS UTF-16 code-unit indexes. |
| `s.split(x)` | `StringSplit` | `StringSplit` | `s.split(&x).map(str::to_owned).collect()` | One string separator | Regex separators and limit | Rust split semantics are used. |

## Python Strings

| Source API | HIR expression | MIR rvalue | Rust output | Supported arguments | Unsupported arguments | Known semantic differences |
| --- | --- | --- | --- | --- | --- | --- |
| `s.lower()` | `StringCase::Lower` | `StringCase::Lower` | `s.to_lowercase()` | No arguments | Any arguments | Python Unicode casing parity is not complete. |
| `s.upper()` | `StringCase::Upper` | `StringCase::Upper` | `s.to_uppercase()` | No arguments | Any arguments | Python Unicode casing parity is not complete. |
| `s.strip()` | `StringTrim::Both` | `StringTrim::Both` | `s.trim().to_owned()` | No arguments | Character-set argument | Rust whitespace semantics are used. |
| `s.lstrip()` | `StringTrim::Start` | `StringTrim::Start` | `s.trim_start().to_owned()` | No arguments | Character-set argument | Rust whitespace semantics are used. |
| `s.rstrip()` | `StringTrim::End` | `StringTrim::End` | `s.trim_end().to_owned()` | No arguments | Character-set argument | Rust whitespace semantics are used. |
| `x in s` | `StringContains` | `StringContains` | `s.contains(&x)` | String item and string receiver | Non-string operands | Rust substring matching is used. |
| `s.startswith(x)` | `StringAffix::StartsWith` | `StringAffix::StartsWith` | `s.starts_with(&x)` | One string argument | Tuple prefixes, start/end arguments | Rust prefix matching is used. |
| `s.endswith(x)` | `StringAffix::EndsWith` | `StringAffix::EndsWith` | `s.ends_with(&x)` | One string argument | Tuple suffixes, start/end arguments | Rust suffix matching is used. |
| `s.find(x)` | `StringSearch::Find` | `StringSearch::Find` | `s.find(&x).map_or(-1, ...)` | One string argument | Optional start/end arguments | Returns Rust byte offsets for now, not Python code-point indexes. |
| `s.rfind(x)` | `StringSearch::RFind` | `StringSearch::RFind` | `s.rfind(&x).map_or(-1, ...)` | One string argument | Optional start/end arguments | Returns Rust byte offsets for now, not Python code-point indexes. |
| `s[i]` | `Index` | `Place::Index` read | `s.chars().nth(normalized_index).expect(...)` | Integer indexes, including negative indexes | Slices with step | Uses Rust Unicode scalar indexes, not Python code-point indexes. |
| `s[:]` / `s[start:end]` | `StringSlice` | `StringSlice` | `s.chars().skip(...).take(...).collect::<String>()` | Omitted, lower, or lower/upper integer bounds, including negative bounds | Step, non-integer bounds | Uses Rust Unicode scalar indexes, not Python code-point indexes. |
| `s.replace(x, y)` | `StringReplace::All` | `StringReplace::All` | `s.replace(&x, &y)` | Two string arguments | Optional count argument | Rust literal replacement semantics are used. |
| `s.removeprefix(x)` | `StringRemoveAffix::StartsWith` | `StringRemoveAffix::StartsWith` | `s.strip_prefix(&x).unwrap_or(&s).to_owned()` | One string argument | Non-string argument | Rust prefix matching is used. |
| `s.removesuffix(x)` | `StringRemoveAffix::EndsWith` | `StringRemoveAffix::EndsWith` | `s.strip_suffix(&x).unwrap_or(&s).to_owned()` | One string argument | Non-string argument | Rust suffix matching is used. |
| `s.isdigit()` | `StringPredicate::IsDigit` | `StringPredicate::IsDigit` | `!s.is_empty() && s.chars().all(char::is_ascii_digit)` | No arguments | Any arguments | ASCII digit semantics are used for now. |
| `s.isalpha()` | `StringPredicate::IsAlpha` | `StringPredicate::IsAlpha` | `!s.is_empty() && s.chars().all(char::is_alphabetic)` | No arguments | Any arguments | Rust Unicode alphabetic semantics are used. |
| `s.isalnum()` | `StringPredicate::IsAlnum` | `StringPredicate::IsAlnum` | `!s.is_empty() && s.chars().all(char::is_alphanumeric)` | No arguments | Any arguments | Rust Unicode alphanumeric semantics are used. |
| `separator.join(items)` | `StringJoin` | `StringJoin` | `items.join(&separator)` | `list[str]` argument and string receiver | Arbitrary iterable arguments | Rust `Vec<String>` join semantics are used. |
| `s.split(x)` | `StringSplit` | `StringSplit` | `s.split(&x).map(str::to_owned).collect()` | One string separator | Default whitespace split and maxsplit | Rust split semantics are used. |

## TypeScript Math

| Source API | HIR expression | MIR rvalue | Rust output | Supported arguments | Unsupported arguments | Known semantic differences |
| --- | --- | --- | --- | --- | --- | --- |
| `Math.abs(x)` | `NumericAbs` | `NumericAbs` | `x.abs()` | One number | Non-number or wrong arity | Uses Rust `f64::abs`. |
| `Math.floor(x)` | `NumericRound::Floor` | `NumericRound::Floor` | `x.floor()` | One number | Non-number or wrong arity | Uses Rust `f64`. |
| `Math.ceil(x)` | `NumericRound::Ceil` | `NumericRound::Ceil` | `x.ceil()` | One number | Non-number or wrong arity | Uses Rust `f64`. |
| `Math.round(x)` | `NumericRound::Round` | `NumericRound::Round` | `x.round()` | One number | Non-number or wrong arity | Rust midpoint behavior may differ from JS in edge cases. |
| `Math.trunc(x)` | `NumericRound::Trunc` | `NumericRound::Trunc` | `x.trunc()` | One number | Non-number or wrong arity | Uses Rust `f64`. |
| `Math.max(...)` | `NumericExtrema::Max` | `NumericExtrema::Max` | Chained `.max(...)` or `f64::NEG_INFINITY` | Any number of number args | Non-number args | Zero args use JS-compatible identity. |
| `Math.min(...)` | `NumericExtrema::Min` | `NumericExtrema::Min` | Chained `.min(...)` or `f64::INFINITY` | Any number of number args | Non-number args | Zero args use JS-compatible identity. |
| `Math.sqrt(x)` | `NumericUnaryFunc::Sqrt` | `NumericUnaryFunc::Sqrt` | `x.sqrt()` | One number | Non-number or wrong arity | Uses Rust `f64`. |
| `Math.cbrt(x)` | `NumericUnaryFunc::Cbrt` | `NumericUnaryFunc::Cbrt` | `x.cbrt()` | One number | Non-number or wrong arity | Uses Rust `f64`. |
| `Math.pow(x, y)` | `NumericPow` | `NumericPow` | `x.powf(y)` | Two numbers | Non-number or wrong arity | Uses Rust `f64`. |
| `Math.hypot(...)` | `NumericHypot` | `NumericHypot` | `0.0f64.hypot(x).hypot(y)` | Any number of number args | Non-number args | Uses a left fold over Rust `f64::hypot`; this may not exactly match JS overflow/underflow edge handling. |
| `Math.sign(x)` | `NumericUnaryFunc::Sign` | `NumericUnaryFunc::Sign` | `x.signum()` | One number | Non-number or wrong arity | JS `-0` and `NaN` edge semantics are not modeled yet. |
| `Math.sin/cos/tan/asin/acos/atan(x)` | `NumericUnaryFunc` | `NumericUnaryFunc` | `x.sin()` / equivalent | One number | Non-number or wrong arity | Uses Rust `f64` behavior directly. |
| `Math.atan2(y, x)` | `NumericAtan2` | `NumericAtan2` | `y.atan2(x)` | Two numbers | Non-number or wrong arity | Uses Rust `f64` behavior directly. |
| `Math.log/log10/log2/exp(x)` | `NumericUnaryFunc` | `NumericUnaryFunc` | `x.ln()` / equivalent | One number | Non-number or wrong arity | Uses Rust `f64` behavior directly. |
| `Math.random()` | `NumericRandom` | `NumericRandom` | `rand::random::<f64>()` | No arguments | Any arguments | Uses the Rust `rand` crate; exact JS PRNG behavior is not modeled. |
| `Number.isFinite(x)` | `NumericPredicate::IsFinite` | `NumericPredicate::IsFinite` | `x.is_finite()` | One number | Non-number or wrong arity | Static TypeScript `number` inputs only; JS non-number false behavior is rejected. |
| `Number.isNaN(x)` | `NumericPredicate::IsNaN` | `NumericPredicate::IsNaN` | `x.is_nan()` | One number | Non-number or wrong arity | Static TypeScript `number` inputs only; JS non-number false behavior is rejected. |

## Python Math And Builtins

| Source API | HIR expression | MIR rvalue | Rust output | Supported arguments | Unsupported arguments | Known semantic differences |
| --- | --- | --- | --- | --- | --- | --- |
| `abs(x)` | `NumericAbs` | `NumericAbs` | `x.abs()` | One `int` or `float` | Non-numeric or wrong arity | Uses Rust numeric methods. |
| `math.sqrt(x)` | `NumericUnaryFunc::Sqrt` | `NumericUnaryFunc::Sqrt` | `x.sqrt()` | One numeric argument | Non-numeric or wrong arity | Codegen currently emits direct floating-point Rust. |
| `math.pow(x, y)` | `NumericPow` | `NumericPow` | `x.powf(y)` | Two numeric arguments | Non-numeric or wrong arity | Codegen currently emits direct floating-point Rust. |
| `math.floor(x)` | `NumericRound::Floor` | `NumericRound::Floor` | `x.floor() as i64` | One float argument | Non-float or wrong arity | Python arbitrary-size integer behavior is not modeled. |
| `math.ceil(x)` | `NumericRound::Ceil` | `NumericRound::Ceil` | `x.ceil() as i64` | One float argument | Non-float or wrong arity | Python arbitrary-size integer behavior is not modeled. |
| `math.trunc(x)` | `NumericRound::Trunc` | `NumericRound::Trunc` | `x.trunc() as i64` | One float argument | Non-float or wrong arity | Python arbitrary-size integer behavior is not modeled. |
| `math.sin/cos/tan/asin/acos/atan(x)` | `NumericUnaryFunc` | `NumericUnaryFunc` | `x.sin()` / equivalent | One numeric argument | Non-numeric or wrong arity | Uses Rust `f64` behavior directly. |
| `math.atan2(y, x)` | `NumericAtan2` | `NumericAtan2` | `y.atan2(x)` | Two numeric arguments | Non-numeric or wrong arity | Uses Rust `f64` behavior directly. |
| `math.log/log10/log2/exp(x)` | `NumericUnaryFunc` | `NumericUnaryFunc` | `x.ln()` / equivalent | One numeric argument | Non-numeric or wrong arity | Uses Rust `f64` behavior directly. |
| `math.isfinite(x)` | `NumericPredicate::IsFinite` | `NumericPredicate::IsFinite` | `x.is_finite()` | One float argument | Non-float or wrong arity | Python accepts ints too; this direct mapping currently requires float. |
| `math.isnan(x)` | `NumericPredicate::IsNaN` | `NumericPredicate::IsNaN` | `x.is_nan()` | One float argument | Non-float or wrong arity | Python accepts ints too; this direct mapping currently requires float. |
| `random.random()` | `NumericRandom` | `NumericRandom` | `rand::random::<f64>()` | No arguments | Any arguments | Uses the Rust `rand` crate; exact CPython PRNG behavior is not modeled. |
| `max(...)` | `NumericExtrema::Max` | `NumericExtrema::Max` | Chained `.max(...)` | At least one all-int or all-float argument list | Zero args, mixed numeric types, iterables, key/default | Python ordering and keyword arguments are not modeled. |
| `min(...)` | `NumericExtrema::Min` | `NumericExtrema::Min` | Chained `.min(...)` | At least one all-int or all-float argument list | Zero args, mixed numeric types, iterables, key/default | Python ordering and keyword arguments are not modeled. |
| `sum(values)` | `ListSum` | `ListSum` | `values.iter().copied().sum::<i64/f64>()` | One `list[int]` or `list[float]` argument | Start argument, iterables other than lists, non-numeric lists | Empty numeric lists use Rust's numeric zero identity. |
| `all(values)` | `ListBoolFold::All` | `ListBoolFold::All` | `values.iter().copied().all(|value| value)` | One `list[bool]` argument | Iterables other than lists, non-bool lists | Python truthiness for arbitrary item types is not modeled. |
| `any(values)` | `ListBoolFold::Any` | `ListBoolFold::Any` | `values.iter().copied().any(|value| value)` | One `list[bool]` argument | Iterables other than lists, non-bool lists | Python truthiness for arbitrary item types is not modeled. |
| `sorted(values)` | `ListSorted` | `ListSorted` | Clone, sort, and return the clone | One sortable list argument | `key`, `reverse`, non-list iterables, nested/record items | Float sorting panics on unordered values such as NaN until Python edge semantics are modeled. |
| `reversed(values)` | `ListReversed` | `ListReversed` | `values.iter().rev().cloned().collect()` | One list argument | Iterator object identity/laziness, non-list iterables | Returns a materialized list rather than Python's lazy reverse iterator. |
| `enumerate(values)` | `ListEnumerate` | `ListEnumerate` | `values.iter().cloned().enumerate().map(...)` | One list argument, plus dict/set after key/value projection | `start`, lazy iterator identity, arbitrary iterables | Returns a materialized list of `(i64, item)` tuples. Set/dict order follows Rust `HashSet`/`HashMap` iteration order. |
| `zip(left, right)` | `ListZip` | `ListZip` | `left.iter().cloned().zip(right.iter().cloned()).collect()` | Two list arguments, plus dict/set after key/value projection | More than two inputs, lazy iterator identity, arbitrary iterables | Returns a materialized list of pair tuples and truncates to the shorter input like Python. Set/dict order follows Rust `HashSet`/`HashMap` iteration order. |
| `range(stop)` / `range(start, stop[, step])` | `ListRange` | `ListRange` | Materialized `Vec<i64>` built with a loop | One to three integer arguments | Keyword args and lazy range object identity | `range()` is represented as `list[int]`; zero step panics in generated Rust. |

## HTTP

| Source API | HIR expression | MIR rvalue | Rust output | Supported arguments | Unsupported arguments | Known semantic differences |
| --- | --- | --- | --- | --- | --- | --- |
| `fetch(url)` | `AsyncOp::HttpGetText` | `AsyncOp::HttpGetText` | `reqwest::get(url).await...text().await...` | One string URL | Options object, response object APIs | Returns response text directly. |
| `requests.get(url)` | `HttpGetText` | `HttpGetText` | `reqwest::blocking::get(url)...text()...` | One string URL | Headers/options, response object APIs | Returns response text directly. |

## JSON

| Source API | HIR expression | MIR rvalue | Rust output | Supported arguments | Unsupported arguments | Known semantic differences |
| --- | --- | --- | --- | --- | --- | --- |
| `JSON.stringify(value)` | `JsonStringify` | `JsonStringify` | `serde_json::to_string(&value).expect(...)` | One JSON-compatible primitive/list/tuple/string-keyed dict value | Replacer, spacing, class values, non-string dict keys | Serde JSON is isolated behind codegen dependency injection; class/interface serialization is not modeled yet. |
| `JSON.parse<T>(text)` | `JsonParse` | `JsonParse` | `serde_json::from_str::<T>(&text).expect(...)` | One string argument and explicit JSON-compatible type argument | Reviver, omitted type argument, class/interface targets | Serde JSON is isolated behind codegen dependency injection; failures currently panic. |
| `json.dumps(value)` | `JsonStringify` | `JsonStringify` | `serde_json::to_string(&value).expect(...)` | One JSON-compatible primitive/list/tuple/string-keyed dict value | `indent`, `default`, other keyword args, class values, non-string dict keys | Python encoder customization and non-string key coercion are not modeled yet. |
| `json.loads(text)` | `JsonParse` | `JsonParse` | `serde_json::from_str::<T>(&text).expect(...)` | One string argument with annotated destination type | Hooks, keyword args, unannotated destination, class targets | Serde JSON is isolated behind codegen dependency injection; failures currently panic. |

## Regex

| Source API | HIR expression | MIR rvalue | Rust output | Supported arguments | Unsupported arguments | Known semantic differences |
| --- | --- | --- | --- | --- | --- | --- |
| `new RegExp(pattern).test(text)` | `RegexIsMatch::Search` | `RegexIsMatch::Search` | `regex::Regex::new(&pattern).expect(...).is_match(&text)` | One string pattern and one string text argument | Flags, regex literals, `String.match`, captures, replacement | Rust `regex` syntax is used; JavaScript features such as lookaround and backreferences are not supported by the Rust crate. |
| `re.search(pattern, text)` | `RegexIsMatch::Search` | `RegexIsMatch::Search` | `regex::Regex::new(&pattern).expect(...).is_match(&text)` | String pattern and text arguments | Flags, compiled patterns, captures, match object access | Returns `bool` directly instead of a Python match object. Rust `regex` syntax is used. |
| `re.match(pattern, text)` | `RegexIsMatch::Match` | `RegexIsMatch::Match` | `regex::Regex::new(&pattern).expect(...).find(&text).is_some_and(...)` | String pattern and text arguments | Flags, compiled patterns, captures, match object access | Returns `bool` directly and requires the first match to start at byte offset 0. |
| `re.fullmatch(pattern, text)` | `RegexIsMatch::FullMatch` | `RegexIsMatch::FullMatch` | `regex::Regex::new(&pattern).expect(...).find(&text).is_some_and(...)` | String pattern and text arguments | Flags, compiled patterns, captures, match object access | Returns `bool` directly and requires the match to cover the full byte length. |

## Contains

| Source API | HIR expression | MIR rvalue | Rust output | Supported arguments | Unsupported arguments | Known semantic differences |
| --- | --- | --- | --- | --- | --- | --- |
| `array.includes(x)` | `ListContains` | `ListContains` | `array.contains(&x)` | One item matching element type | Optional `fromIndex` | Rust equality semantics are used. |
| `array.indexOf(x)` | `ListSearch::Find` | `ListSearch::Find` | `array.iter().position(...).map_or(-1.0, ...)` | One item matching element type | Optional `fromIndex` | Rust equality semantics are used. |
| `array.lastIndexOf(x)` | `ListSearch::RFind` | `ListSearch::RFind` | `array.iter().rposition(...).map_or(-1.0, ...)` | One item matching element type | Optional `fromIndex` | Rust equality semantics are used. |
| `array.at(n)` | `Index` | `Place::Index` read | `array.get(normalized_index).cloned().expect(...)` | One number argument, including negative indexes | Optional/coercive argument forms | Lowers through Python-style HIR indexing; out-of-bounds is a generated panic instead of JS `undefined`. Negative bracket indexes are rejected because JS treats them as property lookups. |
| `array.slice()` / `array.slice(start, end)` | `ListSlice` | `ListSlice` | `array.iter().skip(normalized_start).take(...).cloned().collect()` | Omitted, start, or start/end numeric args, including negative bounds | Third arg | Bounds use Python-style negative-index normalization, matching JS `slice` for this supported subset. |
| `tuple[n]` | `TupleIndex` | `TupleIndex` | `tuple.n.clone()` | Static non-negative integer indexes | Dynamic indexes, negative bracket indexes | TypeScript negative bracket indexes are property lookups, so they are rejected instead of using Python-style HIR negative indexing. |
| `list[i]` | `Index` | `Place::Index` read/write | `list.get(normalized_index).cloned().expect(...)` / `list[normalized_index] = ...` | Integer indexes, including negative indexes | Non-list iterable indexing | Element indexes use Python negative-index normalization and raise via generated panic when out of bounds. |
| `list[:]` / `list[start:end]` | `ListSlice` | `ListSlice` | `list.iter().skip(normalized_start).take(...).cloned().collect()` | Omitted, lower, or lower/upper integer bounds, including negative bounds | Step, non-integer bounds | Bounds use Python slice normalization. |
| `array.push(x)` | `ListPush` | `ListPush` | `{ array.push(x); array.len() as f64 }` | One item matching element type on a local array | Multiple args, mismatched item type, non-local receiver | JS returns the new length; generated Rust mutates the local `Vec`. |
| `array.unshift(...items)` | `ListUnshift` | `ListUnshift` | `{ array.insert(0, item_n); ...; array.len() as f64 }` | Zero or more same-typed items on a local array | Mismatched item type, non-local receiver | Items are inserted in reverse evaluation order after arguments are evaluated, preserving final JS order; front insertion on a Rust `Vec` is linear time. |
| `list.append(x)` | `ListPush` | `ListPush` | `{ list.push(x); () }` | One item matching element type | Multiple args, mismatched item type | Python returns `None`; generated Rust mutates the local `Vec`. |
| `list.extend(other)` | `ListExtend` | `ListExtend` | `{ list.extend(other.iter().cloned()); () }` | One same-typed list argument | Non-list iterables, mismatched element types | Python returns `None`; generated Rust mutates the local `Vec`. |
| `list.insert(index, item)` | `ListInsert` | `ListInsert` | `{ let insert_index = usize::try_from(index).expect(...); list.insert(insert_index, item); () }` | Integer index and same-typed item | Wrong arity, non-int index, mismatched item type | Negative-index parity is not modeled yet; generated Rust rejects negative indexes at runtime. |
| `list.clear()` | `ListClear` | `ListClear` | `{ list.clear(); () }` | No arguments | Arguments | Python returns `None`; generated Rust mutates the local `Vec`. |
| `list.copy()` | `ListCopy` | `ListCopy` | `list.clone()` | No arguments | Arguments | Produces a shallow `Vec` clone. |
| `list.count(item)` | `ListCount` | `ListCount` | `list.iter().filter(...).count() as i64` | One item matching element type | Wrong arity, mismatched item type | Rust equality semantics are used. |
| `list.index(item)` | `ListIndex` | `ListIndex` | `list.iter().position(...).expect(...) as i64` | One item matching element type | Start/stop bounds, mismatched item type | Missing-value `ValueError` is modeled as a generated panic for now. |
| `list.remove(item)` | `ListRemove` | `ListRemove` | `{ let remove_index = list.iter().position(...).expect(...); list.remove(remove_index); () }` | One item matching element type | Wrong arity, mismatched item type | Missing-value `ValueError` is modeled as a generated panic for now. |
| `list.sort()` | `ListSort` | `ListSort` | `list.sort()` or `list.sort_by(...partial_cmp...)` | No arguments on bool, int, float, or str lists | `key`, `reverse`, positional args, non-scalar item types | Float `NaN` ordering is modeled as a generated panic for now. |
| `array.pop()` | `ListPop` | `ListPop` | `array.pop()` | No args | Arguments | TypeScript `undefined` on empty arrays is represented as `Option<T>`. |
| `array.shift()` | `ListShift` | `ListShift` | `if array.is_empty() { None } else { Some(array.remove(0)) }` | No args | Arguments | TypeScript `undefined` on empty arrays is represented as `Option<T>`; Rust removal from the front of a `Vec` is linear time. |
| `list.pop()` | `ListPop` | `ListPop` | `list.pop().expect("pop from empty list")` | No args | Index argument | Empty-list `IndexError` is modeled as a generated panic for now. |
| `array.reverse()` | `ListReverse` | `ListReverse` | `{ array.reverse(); array.clone() }` | No args on a local array | Arguments, non-local receiver | JS returns the same array object; generated Rust returns a clone because alias identity is not modeled. |
| `list.reverse()` | `ListReverse` | `ListReverse` | `{ list.reverse(); () }` | No args | Arguments | Python returns `None`; generated Rust mutates the local `Vec`. |
| `array.concat(other)` | `ListConcat` | `ListConcat` | `array.iter().cloned().chain(other.iter().cloned()).collect()` | One same-typed array argument | Multiple arrays and non-array values | JS sparse-array and value-spreading semantics are not modeled. |
| `array.map(value => expr)` | `ListCallback::Map` | `ListCallback::Map` | `array.iter().map(|item| ...).collect()` | One capture-free expression arrow callback | Captures, function callbacks, statement-heavy callbacks, index/array callback params | Callback lowering is intentionally expression-only until closure capture support lands. |
| `array.filter(value => pred)` | `ListCallback::Filter` | `ListCallback::Filter` | `array.iter().filter(|item| ...).cloned().collect()` | One capture-free expression arrow callback returning bool | Captures, function callbacks, non-bool predicates, index/array callback params | Rust iterator predicate semantics are used; sparse-array behavior is not modeled. |
| `array.reduce((acc, value) => expr, initial)` | `ListReduce` | `ListReduce` | `array.iter().fold(initial, |acc, item| ...)` | Capture-free expression arrow callback and explicit initial value | Missing initial value, captures, index/array callback params | The callback result must match the initial value type. |
| `array.forEach(value => expr)` | `ListCallback::ForEach` | `ListCallback::ForEach` | `array.iter().for_each(|item| { let _ = ...; })` | One capture-free expression arrow callback | Captures, side-effecting statement callbacks, index/array callback params | Only the expression is evaluated and discarded; general mutation side effects are not modeled. |
| `array.find(value => pred)` | `ListCallback::Find` | `ListCallback::Find` | `array.iter().find(...).cloned()` | One capture-free expression arrow callback returning bool | Captures, function callbacks, index/array callback params | TS `undefined` is represented as `Option<T>`. |
| `array.findIndex(value => pred)` | `ListCallback::FindIndex` | `ListCallback::FindIndex` | `array.iter().position(...).map_or(-1.0, ...)` | One capture-free expression arrow callback returning bool | Captures, function callbacks, index/array callback params | Returns a Rust iterator position as `f64`. |
| `array.some(value => pred)` | `ListCallback::Some` | `ListCallback::Some` | `array.iter().any(...)` | One capture-free expression arrow callback returning bool | Captures, function callbacks, index/array callback params | Rust iterator short-circuiting is used. |
| `array.every(value => pred)` | `ListCallback::Every` | `ListCallback::Every` | `array.iter().all(...)` | One capture-free expression arrow callback returning bool | Captures, function callbacks, index/array callback params | Rust iterator short-circuiting is used. |
| `stringArray.join()` | `StringJoin` | `StringJoin` | `string_array.join(&",".to_owned())` | No arguments on `string[]` | Non-string arrays | Default comma separator is used. |
| `stringArray.join(separator)` | `StringJoin` | `StringJoin` | `string_array.join(&separator)` | One string separator on `string[]` | Non-string separator or non-string arrays | JS element stringification is not modeled. |
| `Array.isArray(x)` | `Literal(bool)` | `Use(Constant::Bool)` | `true` or `false` | One statically typed argument | Runtime structural checks for erased/dynamic values | Decided from static HIR type; no runtime guard or narrowing is emitted. |
| `new Set([a, b])` | `SetLit` | `Set` | `HashSet::from([a, b])` | One array literal argument, optionally with `Set<T>` annotation | Iterable variables, spread, mixed element types | Rust `HashSet` uniqueness/order semantics are used. |
| `new Set()` | `SetLit` | `Set` | `HashSet::from([])` | `Set<T>` annotated target | Unannotated empty constructors | Type annotation supplies the missing element type. |
| `set.has(x)` | `SetContains` | `SetContains` | `set.contains(&x)` | One item matching element type | Wrong arity or mismatched item type | Rust `HashSet` equality and hashing semantics are used. |
| `set.add(x)` | `SetAdd` | `SetAdd` | `{ set.insert(x); set.clone() }` | One item matching element type on a local set | Wrong arity, mismatched item type, non-local receiver | JS returns the same set object; generated Rust mutates the local and returns a clone because alias identity is not modeled. |
| `set.delete(x)` | `SetRemove::Delete` | `SetRemove::Delete` | `set.remove(&x)` | One item matching element type on a local set | Wrong arity, mismatched item type, non-local receiver | Rust `HashSet::remove` bool return matches the supported surface. |
| `set.clear()` | `SetClear` | `SetClear` | `{ set.clear(); () }` | No arguments on a local set | Arguments, non-local receiver | JS returns `undefined`, represented as `None`. |
| `set.size` | `Len` | `Len` | `set.len() as f64` | Set receiver | Runtime dynamic receivers | Static set size only. |
| `set.keys()` / `set.values()` | `SetProjection::Values` | `SetProjection::Values` | `set.iter().cloned().collect()` | No arguments | Iterator object identity/laziness | Returns a list rather than a JS iterator. |
| `set.entries()` | `SetProjection::Entries` | `SetProjection::Entries` | `set.iter().map(|v| (v.clone(), v.clone())).collect()` | No arguments | Iterator object identity/laziness | Returns a list of tuples rather than a JS iterator. |
| `for (... of set)` | `SetProjection::Values` plus `For` | `SetProjection::Values` plus indexed `For` | Project set values into a `Vec` and iterate by index | Typed loop binding over `Set<T>` | Direct JS iterator object semantics | Rust `HashSet` iteration order is nondeterministic. |
| `new Map()` | `DictLit` | `Dict` | `HashMap::from([])` | `Map<K, V>` annotated target | Entry iterable construction | TypeScript `Map` is currently represented with the shared dictionary path. |
| `new Map([[k, v], ...])` | `DictLit` | `Dict` | `HashMap::from([(k, v), ...])` | One array literal of homogeneous `[key, value]` array pairs, optionally with `Map<K, V>` annotation | Non-array iterables, spread, mixed key/value types | TypeScript `Map` is currently represented with the shared dictionary path. |
| `map.has(k)` | `DictContainsKey` | `DictContainsKey` | `map.contains_key(&k)` | One key matching key type | Wrong arity or mismatched key type | Rust `HashMap` key semantics are used. |
| `map.get(k)` | `DictGet` | `DictGet` | `map.get(&k).cloned()` | One key matching key type | Wrong arity or mismatched key type | TypeScript `undefined` is represented as `Option<V>`. |
| `map.set(k, v)` | `DictSet` | `DictSet` | `{ map.insert(k, v); map.clone() }` | Key and value matching map type on a local map | Wrong arity, mismatched types, non-local receiver | JS returns the same map object; generated Rust mutates the local and returns a clone because alias identity is not modeled. |
| `map.delete(k)` | `DictRemoveKey` | `DictRemoveKey` | `map.remove(&k).is_some()` | One key matching key type on a local map | Wrong arity, mismatched key type, non-local receiver | Rust `HashMap::remove` bool result matches the supported surface. |
| `map.clear()` | `DictClear` | `DictClear` | `{ map.clear(); () }` | No arguments on a local map | Arguments, non-local receiver | JS returns `undefined`, represented as `None`. |
| `map.size` | `Len` | `Len` | `map.len() as f64` | Map receiver | Runtime dynamic receivers | Static map size only. |
| `map.keys()` | `DictProjection::Keys` | `DictProjection::Keys` | `map.keys().cloned().collect()` | No arguments | Iterator object identity/laziness | Returns a list rather than a JS iterator. |
| `map.values()` | `DictProjection::Values` | `DictProjection::Values` | `map.values().cloned().collect()` | No arguments | Iterator object identity/laziness | Returns a list rather than a JS iterator. |
| `map.entries()` | `DictProjection::Entries` | `DictProjection::Entries` | `map.iter().map(...).collect()` | No arguments | Iterator object identity/laziness | Returns a list of tuples rather than a JS iterator. |
| `for (... of map)` | `DictProjection::Entries` plus `For` | `DictProjection::Entries` plus indexed `For` | Project map entries into a `Vec` and iterate by index | Typed loop binding over `Map<K, V>` entries | Direct JS iterator object semantics and destructured loop bindings | Rust `HashMap` iteration order is nondeterministic. |
| `x in list` / `x not in list` | `ListContains` plus optional `UnaryOp::Not` | `ListContains` plus optional `Unary` | `list.contains(&x)` | Item matching element type | Mismatched types | Rust equality semantics are used. |
| `x in set` / `x not in set` | `SetContains` plus optional `UnaryOp::Not` | `SetContains` plus optional `Unary` | `set.contains(&x)` | Item matching element type | Mismatched types | Rust `HashSet` equality and hashing semantics are used. |
| Python set literal `{a, b}` | `SetLit` | `Set` | `HashSet::from([a, b])` | Same-typed literal elements or annotated target type | Mixed element types, empty set literal syntax | Python preserves unique values; generated Rust uses `HashSet`. |
| `set.add(x)` | `SetAdd` | `SetAdd` | `{ set.insert(x); () }` | One item matching element type on a local set | Wrong arity, mismatched item type, non-local receiver | Python returns `None`; generated Rust mutates the local `HashSet`. |
| `set.discard(x)` | `SetRemove::Discard` | `SetRemove::Discard` | `{ set.remove(&x); () }` | One item matching element type on a local set | Wrong arity, mismatched item type, non-local receiver | Missing values are ignored. |
| `set.remove(x)` | `SetRemove::Remove` | `SetRemove::Remove` | `if !set.remove(&x) { panic!(...) }` | One item matching element type on a local set | Wrong arity, mismatched item type, non-local receiver | Missing-value `KeyError` is modeled as a generated panic for now. |
| `set.copy()` | `SetCopy` | `SetCopy` | `set.clone()` | No arguments | Arguments | Produces a shallow `HashSet` clone. |
| `set.clear()` | `SetClear` | `SetClear` | `{ set.clear(); () }` | No arguments on a local set | Arguments, non-local receiver | Python returns `None`; generated Rust mutates the local `HashSet`. |
| `set.union(other)` | `SetBinary::Union` | `SetBinary::Union` | `set.union(&other).cloned().collect()` | One same-typed set argument | Multiple arguments, arbitrary iterables | Rust `HashSet` order is not deterministic. |
| `set.intersection(other)` | `SetBinary::Intersection` | `SetBinary::Intersection` | `set.intersection(&other).cloned().collect()` | One same-typed set argument | Multiple arguments, arbitrary iterables | Rust `HashSet` order is not deterministic. |
| `set.difference(other)` | `SetBinary::Difference` | `SetBinary::Difference` | `set.difference(&other).cloned().collect()` | One same-typed set argument | Multiple arguments, arbitrary iterables | Rust `HashSet` order is not deterministic. |
| `set.symmetric_difference(other)` | `SetBinary::SymmetricDifference` | `SetBinary::SymmetricDifference` | `set.symmetric_difference(&other).cloned().collect()` | One same-typed set argument | Multiple arguments, arbitrary iterables | Rust `HashSet` order is not deterministic. |
| `set.isdisjoint(other)` | `SetDisjoint` | `SetDisjoint` | `set.is_disjoint(&other)` | One same-typed set argument | Multiple arguments, arbitrary iterables | Rust `HashSet` equality and hashing semantics are used. |
| `set.issubset(other)` / `set.issuperset(other)` | `SetRelation` | `SetRelation` | `set.is_subset(&other)` / `set.is_superset(&other)` | One same-typed set argument | Multiple arguments, arbitrary iterables | Rust `HashSet` equality and hashing semantics are used. |
| `x in tuple` / `x not in tuple` | `TupleContains` plus optional `UnaryOp::Not` | `TupleContains` plus optional `Unary` | Equality chain over tuple fields | Item matching at least one tuple element type | Mismatched types | Rust equality semantics are used. |
| `tuple[i]` | `TupleIndex` | `TupleIndex` | `tuple.i.clone()` | Static integer index, including negative indexes | Dynamic indexes | Heterogeneous tuple typing requires static indexes; out-of-range indexes are rejected while lowering. |
| `tuple[start:end]` | `TupleSlice` | `TupleSlice` | Tuple of cloned fields | Static integer bounds or omitted bounds, including negative bounds | Dynamic bounds and step | Bounds are normalized with Python slice clamping during frontend lowering. |
| `k in dict` / `k not in dict` | `DictContainsKey` plus optional `UnaryOp::Not` | `DictContainsKey` plus optional `Unary` | `dict.contains_key(&k)` | Key matching dict key type | Mismatched key type | Rust `HashMap` key semantics are used. |

## Dictionary Projections

| Source API | HIR expression | MIR rvalue | Rust output | Supported arguments | Unsupported arguments | Known semantic differences |
| --- | --- | --- | --- | --- | --- | --- |
| `Object.keys(record)` | `DictProjection::Keys` | `DictProjection::Keys` | `record.keys().cloned().collect()` | One record argument | Non-record objects | Rust `HashMap` iteration order is used. |
| `Object.values(record)` | `DictProjection::Values` | `DictProjection::Values` | `record.values().cloned().collect()` | One record argument | Non-record objects | Rust `HashMap` iteration order is used. |
| `Object.entries(record)` | `DictProjection::Entries` | `DictProjection::Entries` | `record.iter().map(...).collect()` | One record argument | Non-record objects | Rust `HashMap` iteration order is used. |
| `Object.hasOwn(record, key)` / `record.hasOwnProperty(key)` | `DictContainsKey` | `DictContainsKey` | `record.contains_key(&key)` | Record plus matching key type | Prototype-chain/object semantics | Static record key containment only. |
| `dict.keys()` | `DictProjection::Keys` | `DictProjection::Keys` | `dict.keys().cloned().collect()` | No arguments | View object behavior | Returns a list, not a live Python view. |
| `dict.values()` | `DictProjection::Values` | `DictProjection::Values` | `dict.values().cloned().collect()` | No arguments | View object behavior | Returns a list, not a live Python view. |
| `dict.items()` | `DictProjection::Entries` | `DictProjection::Entries` | `dict.iter().map(...).collect()` | No arguments | View object behavior | Returns a list of tuples, not a live Python view. |
| `for x in set_value` / `for k in dict_value` | `SetProjection::Values` / `DictProjection::Keys` plus `For` | Projection rvalue plus indexed `For` | Project into a `Vec` and iterate by index | Typed loop binding over set values or dict keys | Iterator object identity/laziness | Rust `HashSet`/`HashMap` iteration order is nondeterministic. |
| `dict.get(key[, default])` | `DictGet` | `DictGet` | `dict.get(&key).cloned()` or `.unwrap_or(default)` | Key plus optional default matching value type | Wrong arity, wrong key type, wrong default type | Without a default, missing keys return `None`; with a default, the result has the dictionary value type. |
| `dict.setdefault(key, default)` | `DictSetDefault` | `DictSetDefault` | `dict.entry(key).or_insert(default).clone()` | Key and default matching dict key/value types | Missing default, wrong arity, wrong key type, wrong default type | The no-default `None` form is unsupported unless optional value semantics are modeled explicitly. |
| `dict.update(other)` | `DictUpdate` | `DictUpdate` | `dict.extend(other.iter().map(...)); ()` | One same-typed dict argument | Keyword arguments, iterable pairs, mismatched key/value types | Python returns `None`; generated Rust mutates the local `HashMap`. |
| `dict.copy()` | `DictCopy` | `DictCopy` | `dict.clone()` | No arguments | Arguments | Produces a shallow `HashMap` clone. |
| `dict.pop(key[, default])` | `DictPop` | `DictPop` | `dict.remove(&key).expect(...)` or `dict.remove(&key).unwrap_or(default)` | Key plus optional default matching value type | Wrong arity, wrong key type, wrong default type | Missing-key `KeyError` without a default is modeled as a generated panic for now. |
| `dict.clear()` | `DictClear` | `DictClear` | `{ dict.clear(); () }` | No arguments | Arguments | Python returns `None`; generated Rust mutates the local `HashMap`. |
| `list()` / `set()` / `dict()` / `tuple()` | Literal constructors | Aggregate rvalues | Empty `Vec`, `HashSet`, `HashMap`, or tuple | Empty constructor with target type annotation | Unannotated empty mutable containers | Type annotation supplies missing element, key, and value types. |
| `list(x)` / `set(x)` / `dict(x)` / `tuple(x)` | Copy or identity expression | Copy rvalues or use | Clone same-container values | One same-container argument | Cross-iterable conversion forms and `dict` iterable pairs | Only direct same-container copies are modeled in this slice. |
| `list(set_value)` / `list(dict_value)` | `SetProjection::Values` / `DictProjection::Keys` | Projection rvalues | `set.iter().cloned().collect()` / `dict.keys().cloned().collect()` | One set or dict argument | Iterator ordering guarantees | Rust `HashSet`/`HashMap` iteration order is nondeterministic, matching the already documented projection behavior. |
| `list(tuple_value)` / `set(list_value)` / `set(tuple_value)` | `TupleToList` / `ListToSet` / `TupleToSet` | Matching conversion rvalues | `vec![tuple fields]`, `list.iter().cloned().collect()`, or `HashSet::from([tuple fields])` | Homogeneous tuple or list inputs | Heterogeneous tuple conversion, `tuple(list_value)`, `dict(iterable_pairs)` | Tuple conversions require statically-known homogeneous tuple item types. |
