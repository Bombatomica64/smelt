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
| `s.charAt(n)` | `StringCharAt` | `StringCharAt` | `s.chars().nth(n as usize).map(...).unwrap_or_default()` | One number argument | Default index coercion, negative/infinite edge parity | Uses Rust Unicode scalar indexes, not JS UTF-16 code-unit indexes. |
| `s.charCodeAt(n)` | `StringCharCodeAt` | `StringCharCodeAt` | `s.chars().nth(n as usize).map_or(f64::NAN, ...)` | One number argument | Default index coercion, negative/infinite edge parity | Uses Rust Unicode scalar values, not JS UTF-16 code units. |
| `s.slice()` / `s.slice(start, end)` | `StringSlice` | `StringSlice` | `s.chars().skip(...).take(...).collect::<String>()` | Omitted, start, or start/end numeric args | Negative indexes, third arg, `substring` | Uses Rust Unicode scalar indexes, not JS UTF-16 code-unit indexes; dynamic negative values are not guarded at runtime yet. |
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
| `s[:]` / `s[start:end]` | `StringSlice` | `StringSlice` | `s.chars().skip(...).take(...).collect::<String>()` | Omitted, lower, or lower/upper integer bounds | Negative indexes, step, non-integer bounds | Uses Rust Unicode scalar indexes, not Python code-point indexes; dynamic negative values are not guarded at runtime yet. |
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
| `Number.isFinite(x)` | `NumericPredicate::IsFinite` | `NumericPredicate::IsFinite` | `x.is_finite()` | One number | Non-number or wrong arity | Static TypeScript `number` inputs only; JS non-number false behavior is rejected. |
| `Number.isNaN(x)` | `NumericPredicate::IsNaN` | `NumericPredicate::IsNaN` | `x.is_nan()` | One number | Non-number or wrong arity | Static TypeScript `number` inputs only; JS non-number false behavior is rejected. |

## Python Math And Builtins

| Source API | HIR expression | MIR rvalue | Rust output | Supported arguments | Unsupported arguments | Known semantic differences |
| --- | --- | --- | --- | --- | --- | --- |
| `abs(x)` | `NumericAbs` | `NumericAbs` | `x.abs()` | One `int` or `float` | Non-numeric or wrong arity | Uses Rust numeric methods. |
| `math.sqrt(x)` | `NumericUnaryFunc::Sqrt` | `NumericUnaryFunc::Sqrt` | `x.sqrt()` | One numeric argument | Non-numeric or wrong arity | Codegen currently emits direct floating-point Rust. |
| `math.pow(x, y)` | `NumericPow` | `NumericPow` | `x.powf(y)` | Two numeric arguments | Non-numeric or wrong arity | Codegen currently emits direct floating-point Rust. |
| `math.trunc(x)` | `NumericRound::Trunc` | `NumericRound::Trunc` | `x.trunc() as i64` | One float argument | Non-float or wrong arity | Python arbitrary-size integer behavior is not modeled. |
| `max(...)` | `NumericExtrema::Max` | `NumericExtrema::Max` | Chained `.max(...)` | At least one all-int or all-float argument list | Zero args, mixed numeric types, iterables, key/default | Python ordering and keyword arguments are not modeled. |
| `min(...)` | `NumericExtrema::Min` | `NumericExtrema::Min` | Chained `.min(...)` | At least one all-int or all-float argument list | Zero args, mixed numeric types, iterables, key/default | Python ordering and keyword arguments are not modeled. |

## HTTP

| Source API | HIR expression | MIR rvalue | Rust output | Supported arguments | Unsupported arguments | Known semantic differences |
| --- | --- | --- | --- | --- | --- | --- |
| `fetch(url)` | `AsyncOp::HttpGetText` | `AsyncOp::HttpGetText` | `reqwest::get(url).await...text().await...` | One string URL | Options object, response object APIs | Returns response text directly. |
| `requests.get(url)` | `HttpGetText` | `HttpGetText` | `reqwest::blocking::get(url)...text()...` | One string URL | Headers/options, response object APIs | Returns response text directly. |

## Contains

| Source API | HIR expression | MIR rvalue | Rust output | Supported arguments | Unsupported arguments | Known semantic differences |
| --- | --- | --- | --- | --- | --- | --- |
| `array.includes(x)` | `ListContains` | `ListContains` | `array.contains(&x)` | One item matching element type | Optional `fromIndex` | Rust equality semantics are used. |
| `array.indexOf(x)` | `ListSearch::Find` | `ListSearch::Find` | `array.iter().position(...).map_or(-1.0, ...)` | One item matching element type | Optional `fromIndex` | Rust equality semantics are used. |
| `array.lastIndexOf(x)` | `ListSearch::RFind` | `ListSearch::RFind` | `array.iter().rposition(...).map_or(-1.0, ...)` | One item matching element type | Optional `fromIndex` | Rust equality semantics are used. |
| `array.slice()` / `array.slice(start, end)` | `ListSlice` | `ListSlice` | `array.iter().skip(...).take(...).cloned().collect()` | Omitted, start, or start/end numeric args | Negative indexes, third arg | Rust `usize` slice positions are used; dynamic negative values are not guarded at runtime yet. |
| `list[:]` / `list[start:end]` | `ListSlice` | `ListSlice` | `list.iter().skip(...).take(...).cloned().collect()` | Omitted, lower, or lower/upper integer bounds | Negative indexes, step, non-integer bounds | Rust `usize` slice positions are used; dynamic negative values are not guarded at runtime yet. |
| `array.push(x)` | `ListPush` | `ListPush` | `{ array.push(x); array.len() as f64 }` | One item matching element type on a local array | Multiple args, mismatched item type, non-local receiver | JS returns the new length; generated Rust mutates the local `Vec`. |
| `array.unshift(...items)` | `ListUnshift` | `ListUnshift` | `{ array.insert(0, item_n); ...; array.len() as f64 }` | Zero or more same-typed items on a local array | Mismatched item type, non-local receiver | Items are inserted in reverse evaluation order after arguments are evaluated, preserving final JS order; front insertion on a Rust `Vec` is linear time. |
| `list.append(x)` | `ListPush` | `ListPush` | `{ list.push(x); () }` | One item matching element type | Multiple args, mismatched item type | Python returns `None`; generated Rust mutates the local `Vec`. |
| `list.clear()` | `ListClear` | `ListClear` | `{ list.clear(); () }` | No arguments | Arguments | Python returns `None`; generated Rust mutates the local `Vec`. |
| `array.pop()` | `ListPop` | `ListPop` | `array.pop()` | No args | Arguments | TypeScript `undefined` on empty arrays is represented as `Option<T>`. |
| `array.shift()` | `ListShift` | `ListShift` | `if array.is_empty() { None } else { Some(array.remove(0)) }` | No args | Arguments | TypeScript `undefined` on empty arrays is represented as `Option<T>`; Rust removal from the front of a `Vec` is linear time. |
| `list.pop()` | `ListPop` | `ListPop` | `list.pop().expect("pop from empty list")` | No args | Index argument | Empty-list `IndexError` is modeled as a generated panic for now. |
| `array.reverse()` | `ListReverse` | `ListReverse` | `{ array.reverse(); array.clone() }` | No args on a local array | Arguments, non-local receiver | JS returns the same array object; generated Rust returns a clone because alias identity is not modeled. |
| `list.reverse()` | `ListReverse` | `ListReverse` | `{ list.reverse(); () }` | No args | Arguments | Python returns `None`; generated Rust mutates the local `Vec`. |
| `array.concat(other)` | `ListConcat` | `ListConcat` | `array.iter().cloned().chain(other.iter().cloned()).collect()` | One same-typed array argument | Multiple arrays and non-array values | JS sparse-array and value-spreading semantics are not modeled. |
| `stringArray.join()` | `StringJoin` | `StringJoin` | `string_array.join(&",".to_owned())` | No arguments on `string[]` | Non-string arrays | Default comma separator is used. |
| `stringArray.join(separator)` | `StringJoin` | `StringJoin` | `string_array.join(&separator)` | One string separator on `string[]` | Non-string separator or non-string arrays | JS element stringification is not modeled. |
| `Array.isArray(x)` | `Literal(bool)` | `Use(Constant::Bool)` | `true` or `false` | One statically typed argument | Runtime structural checks for erased/dynamic values | Decided from static HIR type; no runtime guard or narrowing is emitted. |
| `x in list` / `x not in list` | `ListContains` plus optional `UnaryOp::Not` | `ListContains` plus optional `Unary` | `list.contains(&x)` | Item matching element type | Mismatched types | Rust equality semantics are used. |
| `x in tuple` / `x not in tuple` | `TupleContains` plus optional `UnaryOp::Not` | `TupleContains` plus optional `Unary` | Equality chain over tuple fields | Item matching at least one tuple element type | Mismatched types | Rust equality semantics are used. |
| `k in dict` / `k not in dict` | `DictContainsKey` plus optional `UnaryOp::Not` | `DictContainsKey` plus optional `Unary` | `dict.contains_key(&k)` | Key matching dict key type | Mismatched key type | Rust `HashMap` key semantics are used. |

## Dictionary Projections

| Source API | HIR expression | MIR rvalue | Rust output | Supported arguments | Unsupported arguments | Known semantic differences |
| --- | --- | --- | --- | --- | --- | --- |
| `Object.keys(record)` | `DictProjection::Keys` | `DictProjection::Keys` | `record.keys().cloned().collect()` | One record argument | Non-record objects | Rust `HashMap` iteration order is used. |
| `Object.values(record)` | `DictProjection::Values` | `DictProjection::Values` | `record.values().cloned().collect()` | One record argument | Non-record objects | Rust `HashMap` iteration order is used. |
| `Object.entries(record)` | `DictProjection::Entries` | `DictProjection::Entries` | `record.iter().map(...).collect()` | One record argument | Non-record objects | Rust `HashMap` iteration order is used. |
| `dict.keys()` | `DictProjection::Keys` | `DictProjection::Keys` | `dict.keys().cloned().collect()` | No arguments | View object behavior | Returns a list, not a live Python view. |
| `dict.values()` | `DictProjection::Values` | `DictProjection::Values` | `dict.values().cloned().collect()` | No arguments | View object behavior | Returns a list, not a live Python view. |
| `dict.items()` | `DictProjection::Entries` | `DictProjection::Entries` | `dict.iter().map(...).collect()` | No arguments | View object behavior | Returns a list of tuples, not a live Python view. |
| `dict.clear()` | `DictClear` | `DictClear` | `{ dict.clear(); () }` | No arguments | Arguments | Python returns `None`; generated Rust mutates the local `HashMap`. |
