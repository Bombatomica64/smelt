// The JavaScript number-to-string rule, as one emitted runtime helper.
//
// Rust's `f64` `Display` and JavaScript's `Number::toString` agree on most
// values and disagree at both ends of the range, because Rust never switches
// to exponential notation and JavaScript does:
//
// | value | Node | Rust `Display` |
// | --- | --- | --- |
// | `1e21` | `1e+21` | `1000000000000000000000` |
// | `1e-7` | `1e-7` | `0.0000001` |
// | `3.13984e-319` | `3.13984e-319` | 321 characters of zeros and digits |
//
// So every program that printed a very large or very small number printed a
// different string from Node — a general formatting rule, wrong in one place,
// independent of every type that reaches it. It was found while making
// `DataView` concrete (a `getFloat64` over unrelated bytes lands on
// subnormals) and recorded in `blocker-logs/hono-fetch-demand.md`.
//
// ## The spec's rule, and why the digits come from `{:e}`
//
// ECMA-262 `Number::toString(x, 10)` picks integers `s`, `k`, `n` with
// `10^(k-1) <= s < 10^k` and `s * 10^(n-k) = x`, choosing the SMALLEST `k` —
// that is, the shortest decimal digit string that round-trips — and then lays
// those digits out by comparing `n` against 21 and -6.
//
// Rust's `{:e}` already produces exactly that digit string: `LowerExp` uses
// the same shortest-round-trip algorithm as `Display`, printed as
// `d[.ddd]e<exp>`. So the helper reads `s` and `k` off the mantissa and `n`
// off the exponent (`n = exp + 1`, since the mantissa carries one digit before
// the point) rather than reimplementing digit generation, which is the part
// that would be easy to get subtly wrong.

// Emit `smelt_number_to_string(f64) -> String`.
//
// Emitted unconditionally: it is core coercion machinery, reached by
// `Display for SmeltUnknown`, every static `String(x)`/template coercion, a
// property key, an array join and `console.log`. A crate that never
// stringifies a number carries it unused, which is inert under the generated
// crate's `#![allow(dead_code)]` — the same trade the `typeof` helper makes.
// @smelt:item smelt_number_to_string
/// The bits a hole of a `number[]` is stored as: a signaling NaN.
//~
//~ A slot JavaScript leaves empty reads as `undefined`, which a `number`
//~ cannot hold. A signaling NaN is a value no arithmetic produces: every
//~ operation on it answers a quiet NaN, exactly what `undefined` becomes in a
//~ JS number context, so only an untouched slot keeps these bits. Its text is
//~ `undefined`, `console.log` shows the slot as an empty item, and JSON writes
//~ `null` as it does for any NaN.
const SMELT_NUMBER_HOLE_BITS: u64 = 0x7FF4_0000_0000_0000;

/// A hole of a `number[]`; see `SMELT_NUMBER_HOLE_BITS`.
const fn smelt_number_hole() -> f64 { f64::from_bits(SMELT_NUMBER_HOLE_BITS) }

/// Whether `value` is a hole of a `number[]`.
const fn smelt_is_number_hole(value: f64) -> bool { value.to_bits() == SMELT_NUMBER_HOLE_BITS }

/// The value a JavaScript read that finds no slot yields (an out-of-range
/// index, an `Array(n)` hole), in the carrier of a generic element `T`.
//~
//~ JavaScript answers `undefined` whatever the element type, and each carrier
//~ spells `undefined` its own way: `number` is the hole above, an optional is
//~ `None` and the erased carrier is `SmeltUnknown::Undefined` (both their
//~ `Default`), and a carrier with no `undefined` (a `string`, a record) keeps
//~ its `Default`, as the typed read of the same element does. A generic body
//~ asks this function, not `Default::default()`, so `at<T>` instantiated at
//~ `number` returns the same hole the typed `number[]` read returns; the
//~ `Any` test is resolved per instantiation, so the typed path and the generic
//~ one cannot disagree. Needs only `Default + 'static`, which every generated
//~ type parameter carries with or without the erased carrier.
fn smelt_missing<T: Default + 'static>() -> T {
    let mut value = T::default();
    if let Some(number) = (&mut value as &mut dyn ::std::any::Any).downcast_mut::<f64>() {
        *number = smelt_number_hole();
    }
    value
}

/// `Number::toString(value, 10)`: JavaScript's number formatting.
//~
//~ Exponential notation at `n > 21` and `n <= -6`, where `n` is the
//~ decimal exponent of the shortest round-trip digit string — which is
//~ what Rust's `{:e}` already produces.
//~ `Borrow<f64>` rather than `f64`, so the one helper serves both call
//~ shapes the emitter produces: a by-value number, and the `&f64` a
//~ `match` over a BORROWED erased value binds (an erased list's items are
//~ iterated by reference, so `SmeltUnknown::Number(value)` there is a
//~ `&f64`). The alternative was for every emit site to know which shape its
//~ scrutinee had, which is exactly the kind of guess that compiles in the
//~ examples and fails on the first library.
fn smelt_number_to_string<N: ::std::borrow::Borrow<f64>>(value: N) -> String {
    let value = *value.borrow();
    if value.is_nan() { return if smelt_is_number_hole(value) { "undefined" } else { "NaN" }.to_owned(); }
    //~ `-0.0 == 0.0` is true, so negative zero takes this arm and prints
    //~ `0`, which is what `String(-0)` is in JavaScript.
    if value == 0.0 { return "0".to_owned(); }
    if value < 0.0 { return format!("-{}", smelt_number_to_string(-value)); }
    if value.is_infinite() { return "Infinity".to_owned(); }
    let exponential = format!("{value:e}");
    let (mantissa, exponent) = exponential.split_once('e').unwrap_or((exponential.as_str(), "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let k = i32::try_from(digits.len()).unwrap_or(i32::MAX);
    let n = exponent.parse::<i32>().unwrap_or(0) + 1;
    //~ The four layouts of the spec's step 5, in its order: an integer
    //~ with trailing zeros, a decimal point inside the digits, a leading
    //~ `0.` with padding, and exponential form.
    if k <= n && n <= 21 {
        let zeros = usize::try_from(n - k).unwrap_or(0);
        return digits + &"0".repeat(zeros);
    }
    if 0 < n && n <= 21 {
        let split = usize::try_from(n).unwrap_or(0);
        return format!("{}.{}", &digits[..split], &digits[split..]);
    }
    if -6 < n && n <= 0 {
        let zeros = usize::try_from(-n).unwrap_or(0);
        return format!("0.{}{}", "0".repeat(zeros), digits);
    }
    let sign = if n - 1 < 0 { '-' } else { '+' };
    let magnitude = (n - 1).abs();
    if k == 1 { return format!("{digits}e{sign}{magnitude}"); }
    format!("{}.{}e{sign}{magnitude}", &digits[..1], &digits[1..])
}

//~ `console.log` is `util.inspect`, not `String()`, and the two disagree
//~ about exactly one value.
/// `console.log`'s number formatting: the spec's rule, but `-0` prints `-0`.
fn smelt_console_number<N: ::std::borrow::Borrow<f64>>(value: N) -> String { let value = *value.borrow(); if value == 0.0 && value.is_sign_negative() { return "-0".to_owned(); } smelt_number_to_string(value) }

// @smelt:item-end

// @smelt:item smelt_py_str_repr when python_number_format
//~ The table is `not str.isprintable()` over every code point, generated by
//~ CPython 3.14.8 (Unicode 16.0.0), the parity oracle:
//~ [i for i in range(0x110000) if not chr(i).isprintable()]
//~ collapsed into inclusive ranges. CPython's rule: categories Cc, Cf, Cs, Co,
//~ Cn, Zl, Zp and Zs, except the ASCII space, are not printable.
/// The code points `CPython`'s `repr` escapes (`not str.isprintable()`), as
/// sorted inclusive ranges.
#[allow(clippy::unreadable_literal)]
const SMELT_PY_UNPRINTABLE: &[(u32, u32)] = &[
    (0x0, 0x1f), (0x7f, 0xa0), (0xad, 0xad), (0x378, 0x379), (0x380, 0x383), (0x38b, 0x38b), (0x38d, 0x38d), (0x3a2, 0x3a2),
    (0x530, 0x530), (0x557, 0x558), (0x58b, 0x58c), (0x590, 0x590), (0x5c8, 0x5cf), (0x5eb, 0x5ee), (0x5f5, 0x605), (0x61c, 0x61c),
    (0x6dd, 0x6dd), (0x70e, 0x70f), (0x74b, 0x74c), (0x7b2, 0x7bf), (0x7fb, 0x7fc), (0x82e, 0x82f), (0x83f, 0x83f), (0x85c, 0x85d),
    (0x85f, 0x85f), (0x86b, 0x86f), (0x88f, 0x896), (0x8e2, 0x8e2), (0x984, 0x984), (0x98d, 0x98e), (0x991, 0x992), (0x9a9, 0x9a9),
    (0x9b1, 0x9b1), (0x9b3, 0x9b5), (0x9ba, 0x9bb), (0x9c5, 0x9c6), (0x9c9, 0x9ca), (0x9cf, 0x9d6), (0x9d8, 0x9db), (0x9de, 0x9de),
    (0x9e4, 0x9e5), (0x9ff, 0xa00), (0xa04, 0xa04), (0xa0b, 0xa0e), (0xa11, 0xa12), (0xa29, 0xa29), (0xa31, 0xa31), (0xa34, 0xa34),
    (0xa37, 0xa37), (0xa3a, 0xa3b), (0xa3d, 0xa3d), (0xa43, 0xa46), (0xa49, 0xa4a), (0xa4e, 0xa50), (0xa52, 0xa58), (0xa5d, 0xa5d),
    (0xa5f, 0xa65), (0xa77, 0xa80), (0xa84, 0xa84), (0xa8e, 0xa8e), (0xa92, 0xa92), (0xaa9, 0xaa9), (0xab1, 0xab1), (0xab4, 0xab4),
    (0xaba, 0xabb), (0xac6, 0xac6), (0xaca, 0xaca), (0xace, 0xacf), (0xad1, 0xadf), (0xae4, 0xae5), (0xaf2, 0xaf8), (0xb00, 0xb00),
    (0xb04, 0xb04), (0xb0d, 0xb0e), (0xb11, 0xb12), (0xb29, 0xb29), (0xb31, 0xb31), (0xb34, 0xb34), (0xb3a, 0xb3b), (0xb45, 0xb46),
    (0xb49, 0xb4a), (0xb4e, 0xb54), (0xb58, 0xb5b), (0xb5e, 0xb5e), (0xb64, 0xb65), (0xb78, 0xb81), (0xb84, 0xb84), (0xb8b, 0xb8d),
    (0xb91, 0xb91), (0xb96, 0xb98), (0xb9b, 0xb9b), (0xb9d, 0xb9d), (0xba0, 0xba2), (0xba5, 0xba7), (0xbab, 0xbad), (0xbba, 0xbbd),
    (0xbc3, 0xbc5), (0xbc9, 0xbc9), (0xbce, 0xbcf), (0xbd1, 0xbd6), (0xbd8, 0xbe5), (0xbfb, 0xbff), (0xc0d, 0xc0d), (0xc11, 0xc11),
    (0xc29, 0xc29), (0xc3a, 0xc3b), (0xc45, 0xc45), (0xc49, 0xc49), (0xc4e, 0xc54), (0xc57, 0xc57), (0xc5b, 0xc5c), (0xc5e, 0xc5f),
    (0xc64, 0xc65), (0xc70, 0xc76), (0xc8d, 0xc8d), (0xc91, 0xc91), (0xca9, 0xca9), (0xcb4, 0xcb4), (0xcba, 0xcbb), (0xcc5, 0xcc5),
    (0xcc9, 0xcc9), (0xcce, 0xcd4), (0xcd7, 0xcdc), (0xcdf, 0xcdf), (0xce4, 0xce5), (0xcf0, 0xcf0), (0xcf4, 0xcff), (0xd0d, 0xd0d),
    (0xd11, 0xd11), (0xd45, 0xd45), (0xd49, 0xd49), (0xd50, 0xd53), (0xd64, 0xd65), (0xd80, 0xd80), (0xd84, 0xd84), (0xd97, 0xd99),
    (0xdb2, 0xdb2), (0xdbc, 0xdbc), (0xdbe, 0xdbf), (0xdc7, 0xdc9), (0xdcb, 0xdce), (0xdd5, 0xdd5), (0xdd7, 0xdd7), (0xde0, 0xde5),
    (0xdf0, 0xdf1), (0xdf5, 0xe00), (0xe3b, 0xe3e), (0xe5c, 0xe80), (0xe83, 0xe83), (0xe85, 0xe85), (0xe8b, 0xe8b), (0xea4, 0xea4),
    (0xea6, 0xea6), (0xebe, 0xebf), (0xec5, 0xec5), (0xec7, 0xec7), (0xecf, 0xecf), (0xeda, 0xedb), (0xee0, 0xeff), (0xf48, 0xf48),
    (0xf6d, 0xf70), (0xf98, 0xf98), (0xfbd, 0xfbd), (0xfcd, 0xfcd), (0xfdb, 0xfff), (0x10c6, 0x10c6), (0x10c8, 0x10cc), (0x10ce, 0x10cf),
    (0x1249, 0x1249), (0x124e, 0x124f), (0x1257, 0x1257), (0x1259, 0x1259), (0x125e, 0x125f), (0x1289, 0x1289), (0x128e, 0x128f), (0x12b1, 0x12b1),
    (0x12b6, 0x12b7), (0x12bf, 0x12bf), (0x12c1, 0x12c1), (0x12c6, 0x12c7), (0x12d7, 0x12d7), (0x1311, 0x1311), (0x1316, 0x1317), (0x135b, 0x135c),
    (0x137d, 0x137f), (0x139a, 0x139f), (0x13f6, 0x13f7), (0x13fe, 0x13ff), (0x1680, 0x1680), (0x169d, 0x169f), (0x16f9, 0x16ff), (0x1716, 0x171e),
    (0x1737, 0x173f), (0x1754, 0x175f), (0x176d, 0x176d), (0x1771, 0x1771), (0x1774, 0x177f), (0x17de, 0x17df), (0x17ea, 0x17ef), (0x17fa, 0x17ff),
    (0x180e, 0x180e), (0x181a, 0x181f), (0x1879, 0x187f), (0x18ab, 0x18af), (0x18f6, 0x18ff), (0x191f, 0x191f), (0x192c, 0x192f), (0x193c, 0x193f),
    (0x1941, 0x1943), (0x196e, 0x196f), (0x1975, 0x197f), (0x19ac, 0x19af), (0x19ca, 0x19cf), (0x19db, 0x19dd), (0x1a1c, 0x1a1d), (0x1a5f, 0x1a5f),
    (0x1a7d, 0x1a7e), (0x1a8a, 0x1a8f), (0x1a9a, 0x1a9f), (0x1aae, 0x1aaf), (0x1acf, 0x1aff), (0x1b4d, 0x1b4d), (0x1bf4, 0x1bfb), (0x1c38, 0x1c3a),
    (0x1c4a, 0x1c4c), (0x1c8b, 0x1c8f), (0x1cbb, 0x1cbc), (0x1cc8, 0x1ccf), (0x1cfb, 0x1cff), (0x1f16, 0x1f17), (0x1f1e, 0x1f1f), (0x1f46, 0x1f47),
    (0x1f4e, 0x1f4f), (0x1f58, 0x1f58), (0x1f5a, 0x1f5a), (0x1f5c, 0x1f5c), (0x1f5e, 0x1f5e), (0x1f7e, 0x1f7f), (0x1fb5, 0x1fb5), (0x1fc5, 0x1fc5),
    (0x1fd4, 0x1fd5), (0x1fdc, 0x1fdc), (0x1ff0, 0x1ff1), (0x1ff5, 0x1ff5), (0x1fff, 0x200f), (0x2028, 0x202f), (0x205f, 0x206f), (0x2072, 0x2073),
    (0x208f, 0x208f), (0x209d, 0x209f), (0x20c1, 0x20cf), (0x20f1, 0x20ff), (0x218c, 0x218f), (0x242a, 0x243f), (0x244b, 0x245f), (0x2b74, 0x2b75),
    (0x2b96, 0x2b96), (0x2cf4, 0x2cf8), (0x2d26, 0x2d26), (0x2d28, 0x2d2c), (0x2d2e, 0x2d2f), (0x2d68, 0x2d6e), (0x2d71, 0x2d7e), (0x2d97, 0x2d9f),
    (0x2da7, 0x2da7), (0x2daf, 0x2daf), (0x2db7, 0x2db7), (0x2dbf, 0x2dbf), (0x2dc7, 0x2dc7), (0x2dcf, 0x2dcf), (0x2dd7, 0x2dd7), (0x2ddf, 0x2ddf),
    (0x2e5e, 0x2e7f), (0x2e9a, 0x2e9a), (0x2ef4, 0x2eff), (0x2fd6, 0x2fef), (0x3000, 0x3000), (0x3040, 0x3040), (0x3097, 0x3098), (0x3100, 0x3104),
    (0x3130, 0x3130), (0x318f, 0x318f), (0x31e6, 0x31ee), (0x321f, 0x321f), (0xa48d, 0xa48f), (0xa4c7, 0xa4cf), (0xa62c, 0xa63f), (0xa6f8, 0xa6ff),
    (0xa7ce, 0xa7cf), (0xa7d2, 0xa7d2), (0xa7d4, 0xa7d4), (0xa7dd, 0xa7f1), (0xa82d, 0xa82f), (0xa83a, 0xa83f), (0xa878, 0xa87f), (0xa8c6, 0xa8cd),
    (0xa8da, 0xa8df), (0xa954, 0xa95e), (0xa97d, 0xa97f), (0xa9ce, 0xa9ce), (0xa9da, 0xa9dd), (0xa9ff, 0xa9ff), (0xaa37, 0xaa3f), (0xaa4e, 0xaa4f),
    (0xaa5a, 0xaa5b), (0xaac3, 0xaada), (0xaaf7, 0xab00), (0xab07, 0xab08), (0xab0f, 0xab10), (0xab17, 0xab1f), (0xab27, 0xab27), (0xab2f, 0xab2f),
    (0xab6c, 0xab6f), (0xabee, 0xabef), (0xabfa, 0xabff), (0xd7a4, 0xd7af), (0xd7c7, 0xd7ca), (0xd7fc, 0xf8ff), (0xfa6e, 0xfa6f), (0xfada, 0xfaff),
    (0xfb07, 0xfb12), (0xfb18, 0xfb1c), (0xfb37, 0xfb37), (0xfb3d, 0xfb3d), (0xfb3f, 0xfb3f), (0xfb42, 0xfb42), (0xfb45, 0xfb45), (0xfbc3, 0xfbd2),
    (0xfd90, 0xfd91), (0xfdc8, 0xfdce), (0xfdd0, 0xfdef), (0xfe1a, 0xfe1f), (0xfe53, 0xfe53), (0xfe67, 0xfe67), (0xfe6c, 0xfe6f), (0xfe75, 0xfe75),
    (0xfefd, 0xff00), (0xffbf, 0xffc1), (0xffc8, 0xffc9), (0xffd0, 0xffd1), (0xffd8, 0xffd9), (0xffdd, 0xffdf), (0xffe7, 0xffe7), (0xffef, 0xfffb),
    (0xfffe, 0xffff), (0x1000c, 0x1000c), (0x10027, 0x10027), (0x1003b, 0x1003b), (0x1003e, 0x1003e), (0x1004e, 0x1004f), (0x1005e, 0x1007f), (0x100fb, 0x100ff),
    (0x10103, 0x10106), (0x10134, 0x10136), (0x1018f, 0x1018f), (0x1019d, 0x1019f), (0x101a1, 0x101cf), (0x101fe, 0x1027f), (0x1029d, 0x1029f), (0x102d1, 0x102df),
    (0x102fc, 0x102ff), (0x10324, 0x1032c), (0x1034b, 0x1034f), (0x1037b, 0x1037f), (0x1039e, 0x1039e), (0x103c4, 0x103c7), (0x103d6, 0x103ff), (0x1049e, 0x1049f),
    (0x104aa, 0x104af), (0x104d4, 0x104d7), (0x104fc, 0x104ff), (0x10528, 0x1052f), (0x10564, 0x1056e), (0x1057b, 0x1057b), (0x1058b, 0x1058b), (0x10593, 0x10593),
    (0x10596, 0x10596), (0x105a2, 0x105a2), (0x105b2, 0x105b2), (0x105ba, 0x105ba), (0x105bd, 0x105bf), (0x105f4, 0x105ff), (0x10737, 0x1073f), (0x10756, 0x1075f),
    (0x10768, 0x1077f), (0x10786, 0x10786), (0x107b1, 0x107b1), (0x107bb, 0x107ff), (0x10806, 0x10807), (0x10809, 0x10809), (0x10836, 0x10836), (0x10839, 0x1083b),
    (0x1083d, 0x1083e), (0x10856, 0x10856), (0x1089f, 0x108a6), (0x108b0, 0x108df), (0x108f3, 0x108f3), (0x108f6, 0x108fa), (0x1091c, 0x1091e), (0x1093a, 0x1093e),
    (0x10940, 0x1097f), (0x109b8, 0x109bb), (0x109d0, 0x109d1), (0x10a04, 0x10a04), (0x10a07, 0x10a0b), (0x10a14, 0x10a14), (0x10a18, 0x10a18), (0x10a36, 0x10a37),
    (0x10a3b, 0x10a3e), (0x10a49, 0x10a4f), (0x10a59, 0x10a5f), (0x10aa0, 0x10abf), (0x10ae7, 0x10aea), (0x10af7, 0x10aff), (0x10b36, 0x10b38), (0x10b56, 0x10b57),
    (0x10b73, 0x10b77), (0x10b92, 0x10b98), (0x10b9d, 0x10ba8), (0x10bb0, 0x10bff), (0x10c49, 0x10c7f), (0x10cb3, 0x10cbf), (0x10cf3, 0x10cf9), (0x10d28, 0x10d2f),
    (0x10d3a, 0x10d3f), (0x10d66, 0x10d68), (0x10d86, 0x10d8d), (0x10d90, 0x10e5f), (0x10e7f, 0x10e7f), (0x10eaa, 0x10eaa), (0x10eae, 0x10eaf), (0x10eb2, 0x10ec1),
    (0x10ec5, 0x10efb), (0x10f28, 0x10f2f), (0x10f5a, 0x10f6f), (0x10f8a, 0x10faf), (0x10fcc, 0x10fdf), (0x10ff7, 0x10fff), (0x1104e, 0x11051), (0x11076, 0x1107e),
    (0x110bd, 0x110bd), (0x110c3, 0x110cf), (0x110e9, 0x110ef), (0x110fa, 0x110ff), (0x11135, 0x11135), (0x11148, 0x1114f), (0x11177, 0x1117f), (0x111e0, 0x111e0),
    (0x111f5, 0x111ff), (0x11212, 0x11212), (0x11242, 0x1127f), (0x11287, 0x11287), (0x11289, 0x11289), (0x1128e, 0x1128e), (0x1129e, 0x1129e), (0x112aa, 0x112af),
    (0x112eb, 0x112ef), (0x112fa, 0x112ff), (0x11304, 0x11304), (0x1130d, 0x1130e), (0x11311, 0x11312), (0x11329, 0x11329), (0x11331, 0x11331), (0x11334, 0x11334),
    (0x1133a, 0x1133a), (0x11345, 0x11346), (0x11349, 0x1134a), (0x1134e, 0x1134f), (0x11351, 0x11356), (0x11358, 0x1135c), (0x11364, 0x11365), (0x1136d, 0x1136f),
    (0x11375, 0x1137f), (0x1138a, 0x1138a), (0x1138c, 0x1138d), (0x1138f, 0x1138f), (0x113b6, 0x113b6), (0x113c1, 0x113c1), (0x113c3, 0x113c4), (0x113c6, 0x113c6),
    (0x113cb, 0x113cb), (0x113d6, 0x113d6), (0x113d9, 0x113e0), (0x113e3, 0x113ff), (0x1145c, 0x1145c), (0x11462, 0x1147f), (0x114c8, 0x114cf), (0x114da, 0x1157f),
    (0x115b6, 0x115b7), (0x115de, 0x115ff), (0x11645, 0x1164f), (0x1165a, 0x1165f), (0x1166d, 0x1167f), (0x116ba, 0x116bf), (0x116ca, 0x116cf), (0x116e4, 0x116ff),
    (0x1171b, 0x1171c), (0x1172c, 0x1172f), (0x11747, 0x117ff), (0x1183c, 0x1189f), (0x118f3, 0x118fe), (0x11907, 0x11908), (0x1190a, 0x1190b), (0x11914, 0x11914),
    (0x11917, 0x11917), (0x11936, 0x11936), (0x11939, 0x1193a), (0x11947, 0x1194f), (0x1195a, 0x1199f), (0x119a8, 0x119a9), (0x119d8, 0x119d9), (0x119e5, 0x119ff),
    (0x11a48, 0x11a4f), (0x11aa3, 0x11aaf), (0x11af9, 0x11aff), (0x11b0a, 0x11bbf), (0x11be2, 0x11bef), (0x11bfa, 0x11bff), (0x11c09, 0x11c09), (0x11c37, 0x11c37),
    (0x11c46, 0x11c4f), (0x11c6d, 0x11c6f), (0x11c90, 0x11c91), (0x11ca8, 0x11ca8), (0x11cb7, 0x11cff), (0x11d07, 0x11d07), (0x11d0a, 0x11d0a), (0x11d37, 0x11d39),
    (0x11d3b, 0x11d3b), (0x11d3e, 0x11d3e), (0x11d48, 0x11d4f), (0x11d5a, 0x11d5f), (0x11d66, 0x11d66), (0x11d69, 0x11d69), (0x11d8f, 0x11d8f), (0x11d92, 0x11d92),
    (0x11d99, 0x11d9f), (0x11daa, 0x11edf), (0x11ef9, 0x11eff), (0x11f11, 0x11f11), (0x11f3b, 0x11f3d), (0x11f5b, 0x11faf), (0x11fb1, 0x11fbf), (0x11ff2, 0x11ffe),
    (0x1239a, 0x123ff), (0x1246f, 0x1246f), (0x12475, 0x1247f), (0x12544, 0x12f8f), (0x12ff3, 0x12fff), (0x13430, 0x1343f), (0x13456, 0x1345f), (0x143fb, 0x143ff),
    (0x14647, 0x160ff), (0x1613a, 0x167ff), (0x16a39, 0x16a3f), (0x16a5f, 0x16a5f), (0x16a6a, 0x16a6d), (0x16abf, 0x16abf), (0x16aca, 0x16acf), (0x16aee, 0x16aef),
    (0x16af6, 0x16aff), (0x16b46, 0x16b4f), (0x16b5a, 0x16b5a), (0x16b62, 0x16b62), (0x16b78, 0x16b7c), (0x16b90, 0x16d3f), (0x16d7a, 0x16e3f), (0x16e9b, 0x16eff),
    (0x16f4b, 0x16f4e), (0x16f88, 0x16f8e), (0x16fa0, 0x16fdf), (0x16fe5, 0x16fef), (0x16ff2, 0x16fff), (0x187f8, 0x187ff), (0x18cd6, 0x18cfe), (0x18d09, 0x1afef),
    (0x1aff4, 0x1aff4), (0x1affc, 0x1affc), (0x1afff, 0x1afff), (0x1b123, 0x1b131), (0x1b133, 0x1b14f), (0x1b153, 0x1b154), (0x1b156, 0x1b163), (0x1b168, 0x1b16f),
    (0x1b2fc, 0x1bbff), (0x1bc6b, 0x1bc6f), (0x1bc7d, 0x1bc7f), (0x1bc89, 0x1bc8f), (0x1bc9a, 0x1bc9b), (0x1bca0, 0x1cbff), (0x1ccfa, 0x1ccff), (0x1ceb4, 0x1ceff),
    (0x1cf2e, 0x1cf2f), (0x1cf47, 0x1cf4f), (0x1cfc4, 0x1cfff), (0x1d0f6, 0x1d0ff), (0x1d127, 0x1d128), (0x1d173, 0x1d17a), (0x1d1eb, 0x1d1ff), (0x1d246, 0x1d2bf),
    (0x1d2d4, 0x1d2df), (0x1d2f4, 0x1d2ff), (0x1d357, 0x1d35f), (0x1d379, 0x1d3ff), (0x1d455, 0x1d455), (0x1d49d, 0x1d49d), (0x1d4a0, 0x1d4a1), (0x1d4a3, 0x1d4a4),
    (0x1d4a7, 0x1d4a8), (0x1d4ad, 0x1d4ad), (0x1d4ba, 0x1d4ba), (0x1d4bc, 0x1d4bc), (0x1d4c4, 0x1d4c4), (0x1d506, 0x1d506), (0x1d50b, 0x1d50c), (0x1d515, 0x1d515),
    (0x1d51d, 0x1d51d), (0x1d53a, 0x1d53a), (0x1d53f, 0x1d53f), (0x1d545, 0x1d545), (0x1d547, 0x1d549), (0x1d551, 0x1d551), (0x1d6a6, 0x1d6a7), (0x1d7cc, 0x1d7cd),
    (0x1da8c, 0x1da9a), (0x1daa0, 0x1daa0), (0x1dab0, 0x1deff), (0x1df1f, 0x1df24), (0x1df2b, 0x1dfff), (0x1e007, 0x1e007), (0x1e019, 0x1e01a), (0x1e022, 0x1e022),
    (0x1e025, 0x1e025), (0x1e02b, 0x1e02f), (0x1e06e, 0x1e08e), (0x1e090, 0x1e0ff), (0x1e12d, 0x1e12f), (0x1e13e, 0x1e13f), (0x1e14a, 0x1e14d), (0x1e150, 0x1e28f),
    (0x1e2af, 0x1e2bf), (0x1e2fa, 0x1e2fe), (0x1e300, 0x1e4cf), (0x1e4fa, 0x1e5cf), (0x1e5fb, 0x1e5fe), (0x1e600, 0x1e7df), (0x1e7e7, 0x1e7e7), (0x1e7ec, 0x1e7ec),
    (0x1e7ef, 0x1e7ef), (0x1e7ff, 0x1e7ff), (0x1e8c5, 0x1e8c6), (0x1e8d7, 0x1e8ff), (0x1e94c, 0x1e94f), (0x1e95a, 0x1e95d), (0x1e960, 0x1ec70), (0x1ecb5, 0x1ed00),
    (0x1ed3e, 0x1edff), (0x1ee04, 0x1ee04), (0x1ee20, 0x1ee20), (0x1ee23, 0x1ee23), (0x1ee25, 0x1ee26), (0x1ee28, 0x1ee28), (0x1ee33, 0x1ee33), (0x1ee38, 0x1ee38),
    (0x1ee3a, 0x1ee3a), (0x1ee3c, 0x1ee41), (0x1ee43, 0x1ee46), (0x1ee48, 0x1ee48), (0x1ee4a, 0x1ee4a), (0x1ee4c, 0x1ee4c), (0x1ee50, 0x1ee50), (0x1ee53, 0x1ee53),
    (0x1ee55, 0x1ee56), (0x1ee58, 0x1ee58), (0x1ee5a, 0x1ee5a), (0x1ee5c, 0x1ee5c), (0x1ee5e, 0x1ee5e), (0x1ee60, 0x1ee60), (0x1ee63, 0x1ee63), (0x1ee65, 0x1ee66),
    (0x1ee6b, 0x1ee6b), (0x1ee73, 0x1ee73), (0x1ee78, 0x1ee78), (0x1ee7d, 0x1ee7d), (0x1ee7f, 0x1ee7f), (0x1ee8a, 0x1ee8a), (0x1ee9c, 0x1eea0), (0x1eea4, 0x1eea4),
    (0x1eeaa, 0x1eeaa), (0x1eebc, 0x1eeef), (0x1eef2, 0x1efff), (0x1f02c, 0x1f02f), (0x1f094, 0x1f09f), (0x1f0af, 0x1f0b0), (0x1f0c0, 0x1f0c0), (0x1f0d0, 0x1f0d0),
    (0x1f0f6, 0x1f0ff), (0x1f1ae, 0x1f1e5), (0x1f203, 0x1f20f), (0x1f23c, 0x1f23f), (0x1f249, 0x1f24f), (0x1f252, 0x1f25f), (0x1f266, 0x1f2ff), (0x1f6d8, 0x1f6db),
    (0x1f6ed, 0x1f6ef), (0x1f6fd, 0x1f6ff), (0x1f777, 0x1f77a), (0x1f7da, 0x1f7df), (0x1f7ec, 0x1f7ef), (0x1f7f1, 0x1f7ff), (0x1f80c, 0x1f80f), (0x1f848, 0x1f84f),
    (0x1f85a, 0x1f85f), (0x1f888, 0x1f88f), (0x1f8ae, 0x1f8af), (0x1f8bc, 0x1f8bf), (0x1f8c2, 0x1f8ff), (0x1fa54, 0x1fa5f), (0x1fa6e, 0x1fa6f), (0x1fa7d, 0x1fa7f),
    (0x1fa8a, 0x1fa8e), (0x1fac7, 0x1facd), (0x1fadd, 0x1fade), (0x1faea, 0x1faef), (0x1faf9, 0x1faff), (0x1fb93, 0x1fb93), (0x1fbfa, 0x1ffff), (0x2a6e0, 0x2a6ff),
    (0x2b73a, 0x2b73f), (0x2b81e, 0x2b81f), (0x2cea2, 0x2ceaf), (0x2ebe1, 0x2ebef), (0x2ee5e, 0x2f7ff), (0x2fa1e, 0x2ffff), (0x3134b, 0x3134f), (0x323b0, 0xe00ff),
    (0xe01f0, 0x10ffff),
];

/// Whether `repr` shows `character` as itself (`str.isprintable`).
fn smelt_py_is_printable(character: char) -> bool {
    let code = u32::from(character);
    if code < 0x7f { return code >= 0x20; }
    SMELT_PY_UNPRINTABLE.binary_search_by(|&(low, high)| if high < code { ::std::cmp::Ordering::Less } else if low > code { ::std::cmp::Ordering::Greater } else { ::std::cmp::Ordering::Equal }).is_err()
}

/// Python's `repr(str)`: single quotes, unless the text holds one and no double
/// quote; `\\`, `\n`, `\r`, `\t` and the quote escaped, and every other
/// unprintable character as `\xhh`, `\uhhhh` or `\Uhhhhhhhh`, as `CPython`'s
/// `unicode_repr` spells them.
fn smelt_py_str_repr<S: AsRef<str>>(value: S) -> String {
    let value = value.as_ref();
    let quote = if value.contains('\'') && !value.contains('"') { '"' } else { '\'' };
    let mut text = String::with_capacity(value.len().saturating_add(2));
    text.push(quote);
    for character in value.chars() {
        match character {
            '\\' => text.push_str("\\\\"),
            '\n' => text.push_str("\\n"),
            '\r' => text.push_str("\\r"),
            '\t' => text.push_str("\\t"),
            other if other == quote => { text.push('\\'); text.push(other); }
            other if smelt_py_is_printable(other) => text.push(other),
            other => {
                let code = u32::from(other);
                let _ = if code <= 0xff { ::std::fmt::Write::write_fmt(&mut text, format_args!("\\x{code:02x}")) } else if code <= 0xffff { ::std::fmt::Write::write_fmt(&mut text, format_args!("\\u{code:04x}")) } else { ::std::fmt::Write::write_fmt(&mut text, format_args!("\\U{code:08x}")) };
            }
        }
    }
    text.push(quote);
    text
}

// @smelt:item-end

// Emit [`PY_FLOAT_REPR_FN`].
//
// Emitted only for a crate with Python code (a function whose absent value
// is spelled `None`), so TypeScript programs do not carry it.
// @smelt:item smelt_py_float_repr when python_number_format
/// Python's `repr(float)`: `12.0`, `1e+16`, `1.5e-07`, `inf`, `nan`.
fn smelt_py_float_repr<N: ::std::borrow::Borrow<f64>>(value: N) -> String {
    let value = *value.borrow();
    if value.is_nan() { return "nan".to_owned(); }
    if value.is_infinite() { return if value > 0.0 { "inf" } else { "-inf" }.to_owned(); }
    let text = format!("{value:?}");
    match text.split_once('e') {
        Some((mantissa, exponent)) => { let (sign, digits) = exponent.strip_prefix('-').map_or(("+", exponent), |digits| ("-", digits)); format!("{mantissa}e{sign}{digits:0>2}") }
        None => text,
    }
}

// @smelt:item-end

// `Number.prototype`'s formatting methods: `toFixed`, `toPrecision`,
// `toExponential` and `toString(radix)`.
//
// One family for the typed op (`Rvalue::NumericFormat`) and the erased member
// read (`(x as any).toFixed`), so the two spellings cannot round differently.
// The spec rounds an exact tie AWAY from zero ("pick the larger n"), Rust's
// float formatting rounds it to EVEN, and that is the whole difference: Rust
// formats the exact binary value correctly otherwise, so the hand rounding
// runs only when a tie is possible. Checked against Node on 150k values
// (ties, subnormals, `1e308`, every radix) when written.
// @smelt:item smelt_number_format_checked when number_methods
/// The digits of a positive finite `value` rounded to `count` significant
/// digits the way `Number.prototype.toPrecision` rounds: to nearest, an
/// exact tie going AWAY from zero (the spec's "if there are two such sets
/// of e and n, pick the e and n for which n × 10^(e–p+1) is larger").
//~
//~ Answers `(digits, exponent)` with `digits.len() == count` and
//~ `value ≈ d.ddd × 10^exponent`. Rust's `{:.*e}` rounds the EXACT binary
//~ value correctly, but breaks an exact tie to even, so it is right
//~ whenever the value is not a tie. A tie shows as a trailing `5` one
//~ digit further out, and only then is the exact decimal expansion (at
//~ most 767 significant digits for an `f64`) rounded by hand.
fn smelt_number_round_significant(value: f64, count: usize) -> (Vec<u8>, i32) {
    let split = |text: String| -> (Vec<u8>, i32) {
        let (mantissa, exponent) = text.split_once('e').unwrap_or((text.as_str(), "0"));
        (mantissa.bytes().filter(u8::is_ascii_digit).collect(), exponent.parse().unwrap_or(0))
    };
    let (probe, _) = split(format!("{value:.count$e}"));
    if probe.last() != Some(&b'5') { return split(format!("{value:.precision$e}", precision = count - 1)); }
    let (exact, exponent) = split(format!("{value:.800e}"));
    let mut kept = exact[..count].to_vec();
    if exact[count] < b'5' { return (kept, exponent); }
    for digit in kept.iter_mut().rev() {
        if *digit == b'9' { *digit = b'0'; } else { *digit += 1; return (kept, exponent); }
    }
    kept.insert(0, b'1');
    kept.pop();
    (kept, exponent + 1)
}

/// `Number.prototype.toFixed(digits)` with `digits` already checked to be in `0..=100`.
//~
//~ Rounds to nearest with an exact tie going away from zero, as the spec's
//~ "if there are two such n, pick the larger n" does: `(2.5).toFixed(0)` is
//~ `"3"` and `(1.25).toFixed(1)` is `"1.3"`, where Rust's `{:.1}` answers
//~ `1.2`. Rust's fixed formatting is otherwise exact, so the hand rounding
//~ runs only on a tie (see `smelt_number_round_significant`). A magnitude
//~ of `1e21` or more, and a non-finite value, print as `String(value)`.
fn smelt_number_to_fixed(value: f64, digits: usize) -> String {
    if !value.is_finite() || value.abs() >= 1e21 { return smelt_number_to_string(value); }
    let sign = if value < 0.0 { "-" } else { "" };
    let magnitude = value.abs();
    let probe = format!("{magnitude:.probe_digits$}", probe_digits = digits + 1);
    if !probe.ends_with('5') { return format!("{sign}{magnitude:.digits$}"); }
    let exact = format!("{magnitude:.1100}");
    let (whole, fraction) = exact.split_once('.').unwrap_or((exact.as_str(), ""));
    let mut kept: Vec<u8> = whole.bytes().chain(fraction.bytes().take(digits)).collect();
    if fraction.as_bytes()[digits] >= b'5' {
        let mut carry = true;
        for digit in kept.iter_mut().rev() {
            if *digit == b'9' { *digit = b'0'; } else { *digit += 1; carry = false; break; }
        }
        if carry { kept.insert(0, b'1'); }
    }
    let point = kept.len() - digits;
    let text = String::from_utf8_lossy(&kept);
    if digits == 0 { format!("{sign}{text}") } else { format!("{sign}{}.{}", &text[..point], &text[point..]) }
}

/// `Number.prototype.toExponential(digits)`, `digits` checked to be in `0..=100`;
/// `None` is the shortest digit string that round-trips.
fn smelt_number_to_exponential(value: f64, digits: Option<usize>) -> String {
    if !value.is_finite() { return smelt_number_to_string(value); }
    let sign = if value < 0.0 { "-" } else { "" };
    let magnitude = value.abs();
    let (mantissa, exponent) = if magnitude == 0.0 {
        (vec![b'0'; digits.unwrap_or(0) + 1], 0)
    } else if let Some(digits) = digits {
        smelt_number_round_significant(magnitude, digits + 1)
    } else {
        let text = format!("{magnitude:e}");
        let (mantissa, exponent) = text.split_once('e').unwrap_or((text.as_str(), "0"));
        (mantissa.bytes().filter(u8::is_ascii_digit).collect(), exponent.parse().unwrap_or(0))
    };
    let mantissa = String::from_utf8_lossy(&mantissa);
    let point = if mantissa.len() > 1 { format!("{}.{}", &mantissa[..1], &mantissa[1..]) } else { mantissa.into_owned() };
    format!("{sign}{point}e{}{}", if exponent < 0 { '-' } else { '+' }, exponent.abs())
}

/// `Number.prototype.toPrecision(digits)`, `digits` checked to be in `1..=100`;
/// `None` is `String(value)`.
//~
//~ Exponential form when the exponent is below -6 or not below the
//~ precision, fixed form otherwise -- the spec's steps 10-12.
fn smelt_number_to_precision(value: f64, digits: Option<usize>) -> String {
    let Some(digits) = digits else { return smelt_number_to_string(value) };
    if !value.is_finite() { return smelt_number_to_string(value); }
    let sign = if value < 0.0 { "-" } else { "" };
    let magnitude = value.abs();
    let (mantissa, exponent) = if magnitude == 0.0 { (vec![b'0'; digits], 0) } else { smelt_number_round_significant(magnitude, digits) };
    let mantissa = String::from_utf8_lossy(&mantissa);
    let precision = i32::try_from(digits).unwrap_or(i32::MAX);
    if exponent < -6 || exponent >= precision {
        let point = if digits > 1 { format!("{}.{}", &mantissa[..1], &mantissa[1..]) } else { mantissa.into_owned() };
        return format!("{sign}{point}e{}{}", if exponent < 0 { '-' } else { '+' }, exponent.abs());
    }
    if exponent == precision - 1 { return format!("{sign}{mantissa}"); }
    if exponent >= 0 {
        let point = usize::try_from(exponent + 1).unwrap_or(0);
        return format!("{sign}{}.{}", &mantissa[..point], &mantissa[point..]);
    }
    format!("{sign}0.{}{mantissa}", "0".repeat(usize::try_from(-(exponent + 1)).unwrap_or(0)))
}

/// `Number.prototype.toString(radix)` for a radix other than 10: the integer
/// part and as many fraction digits as tell the value apart from its neighbours.
//~
//~ The digit generation is V8's `DoubleToRadixCString`, the reference
//~ implementation Node runs: fraction digits are produced until the
//~ remaining fraction is smaller than half the gap to the next `f64`
//~ (`delta`), rounding the last digit up -- with a carry into the integer
//~ part -- when the rest exceeds one half. `radix` is checked to be in
//~ `2..=36` by the caller.
fn smelt_number_to_string_radix(value: f64, radix: u32) -> String {
    if radix == 10 || !value.is_finite() || value == 0.0 { return smelt_number_to_string(value); }
    let base = f64::from(radix);
    let negative = value < 0.0;
    let value = value.abs();
    let digit_char = |digit: u32| char::from_digit(digit, radix).unwrap_or('0');
    let mut integer = value.floor();
    let mut fraction = value - integer;
    let mut delta = (0.5 * (f64::from_bits(value.to_bits() + 1) - value)).max(f64::from_bits(1));
    let mut fraction_digits: Vec<u32> = Vec::new();
    if fraction >= delta {
        loop {
            fraction *= base;
            delta *= base;
            //~ `fraction` is in `[0, 1)` before the multiply, so the digit is in `0..radix`.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let digit = fraction as u32;
            fraction_digits.push(digit);
            fraction -= f64::from(digit);
            //~ V8 compares the remainder with one half EXACTLY: a tie rounds to an odd digit's neighbour.
            #[allow(clippy::float_cmp)]
            let round_up = (fraction > 0.5 || (fraction == 0.5 && digit & 1 == 1)) && fraction + delta > 1.0;
            if round_up {
                loop {
                    match fraction_digits.pop() {
                        None => { integer += 1.0; break; }
                        Some(last) if last + 1 < radix => { fraction_digits.push(last + 1); break; }
                        Some(_) => {}
                    }
                }
                break;
            }
            if fraction < delta { break; }
        }
    }
    let mut integer_digits: Vec<char> = Vec::new();
    //~ Above 2^53 the low integer digits are not represented; V8 prints them as zeros.
    while (integer / base).log2().total_cmp(&53.0).is_ge() { integer /= base; integer_digits.push('0'); }
    loop {
        let remainder = integer % base;
        //~ `remainder` is a whole number in `0..radix`.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let digit = remainder as u32;
        integer_digits.push(digit_char(digit));
        integer = (integer - remainder) / base;
        if integer <= 0.0 { break; }
    }
    let mut text = String::with_capacity(integer_digits.len() + fraction_digits.len() + 2);
    if negative { text.push('-'); }
    text.extend(integer_digits.iter().rev());
    if !fraction_digits.is_empty() { text.push('.'); text.extend(fraction_digits.into_iter().map(digit_char)); }
    text
}

/// Run `Number.prototype[method](argument)` on `value`, with the spec's
/// `ToIntegerOrInfinity` and range check on the argument: the formatted
/// text, or the method's `RangeError` message.
//~
//~ The order of the checks is each method's own: `toFixed` and
//~ `toString` reject a bad argument even for `NaN`, while `toPrecision` and
//~ `toExponential` answer `String(value)` for a non-finite value first, so
//~ `NaN.toPrecision(0)` is `"NaN"` and `NaN.toFixed(101)` throws.
fn smelt_number_format_checked(method: &str, value: f64, argument: Option<f64>) -> Result<String, String> {
    let integer = argument.map(|argument| if argument.is_nan() { 0.0 } else { argument.trunc() });
    let (low, high, message) = match method {
        "toFixed" => (0.0, 100.0, "toFixed() digits argument must be between 0 and 100"),
        "toPrecision" => (1.0, 100.0, "toPrecision() argument must be between 1 and 100"),
        "toExponential" => (0.0, 100.0, "toExponential() argument must be between 0 and 100"),
        _ => (2.0, 36.0, "toString() radix argument must be between 2 and 36"),
    };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let checked = |integer: f64| if (low..=high).contains(&integer) { Ok(integer as usize) } else { Err(message.to_owned()) };
    Ok(match method {
        "toFixed" => smelt_number_to_fixed(value, checked(integer.unwrap_or(0.0))?),
        "toPrecision" | "toExponential" if !value.is_finite() => smelt_number_to_string(value),
        "toPrecision" => smelt_number_to_precision(value, integer.map(checked).transpose()?),
        "toExponential" => smelt_number_to_exponential(value, integer.map(checked).transpose()?),
        #[allow(clippy::cast_possible_truncation)]
        _ => smelt_number_to_string_radix(value, checked(integer.unwrap_or(10.0))? as u32),
    })
}

// @smelt:item-end

// @smelt:item smelt_number_range_error when number_methods & unknown
/// A JavaScript `RangeError` from a `Number.prototype` formatting method.
fn smelt_number_range_error(message: String) -> Box<dyn ::std::error::Error> { smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("RangeError".into())), ("message".to_owned(), SmeltUnknown::String(message.into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)])))) }

// @smelt:item-end

// @smelt:item smelt_number_to_locale_string when number_locale
//~ `Number.prototype.toLocaleString(locale)` with the default options:
//~ `maximumFractionDigits: 3` rounded half away from zero, no minimum, and
//~ the locale's grouping and decimal separators (CLDR, as Node 24 prints
//~ them). An absent locale is the runtime default `en-US`; a locale this
//~ table does not list falls back to its language, then to `en-US`, as an
//~ unsupported locale falls back to the default in ICU.
/// The digits of `value` as `toLocaleString(locale)` prints them.
fn smelt_number_to_locale_string(value: f64, locale: Option<&str>) -> String {
    if value.is_nan() { return "NaN".to_owned(); }
    if value.is_infinite() { return if value > 0.0 { "∞".to_owned() } else { "-∞".to_owned() }; }
    let tag = locale.unwrap_or("en-US");
    let language = tag.split(['-', '_']).next().unwrap_or("en").to_ascii_lowercase();
    // (grouping separator, decimal separator, minimum grouping digits, Indian grouping)
    let (group, decimal, min_grouping, indian): (&str, &str, usize, bool) = match (language.as_str(), tag) {
        (_, "de-CH" | "de-LI") => ("’", ".", 1, false),
        ("en" | "hi", "en-IN") | ("hi", _) => (",", ".", 1, true),
        ("de" | "it" | "nl" | "id" | "tr" | "da" | "pt" | "el" | "ro" | "hr" | "sl", _) => (".", ",", 1, false),
        ("es", _) => (".", ",", 2, false),
        ("fr", _) => ("\u{202f}", ",", 1, false),
        ("pl", _) => ("\u{a0}", ",", 2, false),
        ("ru" | "uk" | "cs" | "sk" | "sv" | "nb" | "no" | "fi" | "hu" | "bg", _) => ("\u{a0}", ",", 1, false),
        _ => (",", ".", 1, false),
    };
    // Half away from zero at three fraction digits; `{:.3}` would round the
    // binary value half to even.
    let scaled = (value.abs() * 1000.0).round();
    let negative = value.is_sign_negative();
    let text = format!("{:.3}", scaled / 1000.0);
    let (whole, fraction) = text.split_once('.').unwrap_or((text.as_str(), ""));
    let fraction = fraction.trim_end_matches('0');
    let digits: Vec<char> = whole.chars().collect();
    let mut grouped = String::new();
    if digits.len() >= 3 + min_grouping {
        let mut positions = Vec::new();
        let mut index = digits.len();
        let mut first = true;
        while index > 0 {
            let size = if first || !indian { 3 } else { 2 };
            first = false;
            let start = index.saturating_sub(size);
            positions.push(digits[start..index].iter().collect::<String>());
            index = start;
        }
        positions.reverse();
        grouped = positions.join(group);
    } else {
        grouped.extend(digits);
    }
    let mut out = String::new();
    if negative { out.push('-'); }
    out.push_str(&grouped);
    if !fraction.is_empty() { out.push_str(decimal); out.push_str(fraction); }
    out
}
// @smelt:item-end
