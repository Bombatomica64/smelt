// Python's `int`: an arbitrary-precision signed integer.
//
// CPython's `int` never overflows: `2 ** 100`, `x * 2` in a loop that runs 70
// times, and `9223372036854775807 + 1` are all exact. A hand-written port that
// carried `int` as `i64` would panic (debug) or wrap (release) on the first of
// those, so a Python `int` lowers to `smelt_hir::Type::PyInt` and is carried as
// `SmeltPyInt`:
//
// * `Small(i64)`, the fast path every value that fits a machine word takes. An
//   operation on two small values is one checked machine instruction, and only
//   an overflow leaves it.
// * `Big(Rc<BigInt>)`, every value that does not fit. `num-bigint` is the
//   crate a Rust team reaches for; the `Rc` makes a copy of a big value a
//   reference-count bump, which is sound because a Python `int` is immutable.
//
// The two arms are normalised: a value that fits `i64` is always `Small`, so
// the derived `PartialEq`/`Hash` (arm, then payload) is value equality.
//
// `CPython` also caps the digits `str(n)` writes and `int(text)` reads in a
// base that is not a power of two (4300 by default, a configurable defence
// against quadratic-time conversion, `sys.set_int_max_str_digits`). That cap
// is interpreter policy rather than `int` semantics, and enforcing it would
// make every `print` of an `int` a raising call; this runtime behaves as
// `CPython` does with the cap disabled (`-X int_max_str_digits=0`).
//
// Every operator follows CPython, not Rust: `//` and `%` floor toward negative
// infinity, `/` is correctly rounded true division into a `float`, `**` with a
// non-negative exponent is exact, shifts and bitwise operators treat a negative
// value as an infinite two's-complement bit string (which `num-bigint` already
// does), and the failures raise the Python exception CPython raises
// (`ZeroDivisionError`, `OverflowError`, `ValueError`, `IndexError`) through the
// Python error channel, never a panic.

// @smelt:item SmeltPyInt when py_int
/// A Python `int`: arbitrary precision, with a machine-word fast path.
#[derive(Clone, PartialEq, Eq, Hash)]
#[allow(dead_code)]
enum SmeltPyInt {
    //~ Every value in `i64` range.
    Small(i64),
    //~ Every value outside `i64` range; never one that fits it.
    Big(::std::rc::Rc<::num_bigint::BigInt>),
}

/// The code point of the digit zero of every Unicode `Nd` block outside ASCII
/// (Unicode 16, the version `CPython` 3.14 ships). Each block holds the ten
/// digits `0`..`9` in order; `int()` accepts any of them.
const SMELT_PY_UNICODE_DIGIT_ZEROS: [u32; 75] = [
    0x660, 0x6F0, 0x7C0, 0x966, 0x9E6, 0xA66, 0xAE6, 0xB66, 0xBE6, 0xC66, 0xCE6, 0xD66, 0xDE6, 0xE50, 0xED0, 0xF20,
    0x1040, 0x1090, 0x17E0, 0x1810, 0x1946, 0x19D0, 0x1A80, 0x1A90, 0x1B50, 0x1BB0, 0x1C40, 0x1C50, 0xA620, 0xA8D0,
    0xA900, 0xA9D0, 0xA9F0, 0xAA50, 0xABF0, 0xFF10, 0x104A0, 0x10D30, 0x10D40, 0x11066, 0x110F0, 0x11136, 0x111D0,
    0x112F0, 0x11450, 0x114D0, 0x11650, 0x116C0, 0x116D0, 0x116DA, 0x11730, 0x118E0, 0x11950, 0x11BF0, 0x11C50,
    0x11D50, 0x11DA0, 0x11F50, 0x16130, 0x16A60, 0x16AC0, 0x16B50, 0x16D70, 0x1CCF0, 0x1D7CE, 0x1D7D8, 0x1D7E2,
    0x1D7EC, 0x1D7F6, 0x1E140, 0x1E2F0, 0x1E4F0, 0x1E5F1, 0x1E950, 0x1FBF0,
];

/// A Python exception of the builtin class whose chain is `classes`, with one
/// message argument.
#[allow(dead_code)]
fn smelt_py_int_error(classes: &'static [&'static str], message: &str) -> Box<dyn ::std::error::Error> {
    SmeltPyException::new(classes, vec![SmeltPyException::arg(&message.to_owned())]).into_error()
}

/// `ZeroDivisionError` with `message`.
#[allow(dead_code)]
fn smelt_py_zero_division(message: &str) -> Box<dyn ::std::error::Error> {
    smelt_py_int_error(&["ZeroDivisionError", "ArithmeticError", "Exception", "BaseException"], message)
}

/// `OverflowError` with `message`.
#[allow(dead_code)]
fn smelt_py_overflow_error(message: &str) -> Box<dyn ::std::error::Error> {
    smelt_py_int_error(&["OverflowError", "ArithmeticError", "Exception", "BaseException"], message)
}

/// `x * 2**exponent`, exact wherever the result is a normal `f64`: the scale is
/// applied in steps that each stay inside `f64`'s exponent range.
#[allow(dead_code)]
fn smelt_py_ldexp(mut value: f64, mut exponent: i64) -> f64 {
    while exponent > 1000 {
        value *= 2f64.powi(1000);
        exponent -= 1000;
    }
    while exponent < -1000 {
        value *= 2f64.powi(-1000);
        exponent += 1000;
    }
    value * 2f64.powi(i32::try_from(exponent).unwrap_or(0))
}

#[allow(dead_code)]
impl SmeltPyInt {
    //~ The value of a `BigInt`, normalised onto the small arm when it fits.
    fn from_big(value: ::num_bigint::BigInt) -> Self {
        i64::try_from(&value).map_or_else(|_wide| Self::Big(::std::rc::Rc::new(value)), Self::Small)
    }

    //~ The value as a `BigInt` (a copy).
    fn to_big(&self) -> ::num_bigint::BigInt {
        match self {
            Self::Small(value) => ::num_bigint::BigInt::from(*value),
            Self::Big(value) => (**value).clone(),
        }
    }

    //~ The value of an integer literal too wide for `i64`, from the canonical
    //~ decimal digits the frontend validated.
    fn from_literal_digits(digits: &str) -> Self {
        Self::from_big(digits.parse::<::num_bigint::BigInt>().unwrap_or_default())
    }

    //~ The value when it fits `i64`.
    const fn as_small(&self) -> Option<i64> {
        match self {
            Self::Small(value) => Some(*value),
            Self::Big(_) => None,
        }
    }

    //~ Whether the value is zero.
    const fn is_zero(&self) -> bool {
        matches!(self, Self::Small(0))
    }

    //~ Whether the value is below zero.
    fn is_negative(&self) -> bool {
        match self {
            Self::Small(value) => *value < 0,
            Self::Big(value) => value.sign() == ::num_bigint::Sign::Minus,
        }
    }

    //~ Python's `bool(n)`.
    const fn truthy(&self) -> bool {
        !self.is_zero()
    }

    //~ `abs(n)`.
    fn abs(&self) -> Self {
        if self.is_negative() { -self } else { self.clone() }
    }

    //~ `n.bit_length()`.
    fn bit_length(&self) -> Self {
        match self {
            Self::Small(value) => Self::Small(i64::from(64 - value.unsigned_abs().leading_zeros())),
            Self::Big(value) => Self::from(value.bits()),
        }
    }

    //~ `a // b`: the quotient rounded toward negative infinity.
    fn floor_div(&self, rhs: &Self) -> Result<Self, Box<dyn ::std::error::Error>> {
        if rhs.is_zero() {
            return Err(smelt_py_zero_division("division by zero"));
        }
        if let (Self::Small(lhs), Self::Small(rhs)) = (self, rhs)
            && let Some(quotient) = lhs.checked_div_euclid(*rhs)
        {
            //~ Euclidean division keeps the remainder non-negative; floor
            //~ division keeps the divisor's sign. They differ only for a
            //~ negative divisor and a nonzero remainder.
            let remainder = lhs.rem_euclid(*rhs);
            return Ok(Self::Small(if *rhs < 0 && remainder != 0 { quotient - 1 } else { quotient }));
        }
        Ok(Self::from_big(::num_integer::Integer::div_floor(&self.to_big(), &rhs.to_big())))
    }

    //~ `a % b`: the remainder takes the divisor's sign.
    fn modulo(&self, rhs: &Self) -> Result<Self, Box<dyn ::std::error::Error>> {
        if rhs.is_zero() {
            return Err(smelt_py_zero_division("division by zero"));
        }
        if let (Self::Small(lhs), Self::Small(rhs)) = (self, rhs)
            && let Some(remainder) = lhs.checked_rem(*rhs)
        {
            return Ok(Self::Small(if remainder != 0 && (remainder < 0) != (*rhs < 0) { remainder + rhs } else { remainder }));
        }
        Ok(Self::from_big(::num_integer::Integer::mod_floor(&self.to_big(), &rhs.to_big())))
    }

    //~ `divmod(a, b)`.
    fn divmod(&self, rhs: &Self) -> Result<(Self, Self), Box<dyn ::std::error::Error>> {
        if rhs.is_zero() {
            return Err(smelt_py_zero_division("division by zero"));
        }
        Ok((self.floor_div(rhs)?, self.modulo(rhs)?))
    }

    //~ `a / b`: the correctly rounded `float` quotient, as `CPython`'s
    //~ `long_true_divide`. Two operands exact in `f64` divide there, which IEEE
    //~ rounds correctly; wider ones divide as integers scaled to 55 quotient
    //~ bits with a sticky bit, so the one rounding to 53 bits is the last.
    #[allow(clippy::cast_precision_loss, reason = "both operands are exact in f64 below 2^53, and the scaled quotient fits 64 bits")]
    fn true_div(&self, rhs: &Self) -> Result<f64, Box<dyn ::std::error::Error>> {
        const EXACT: u64 = 1 << 53;
        if rhs.is_zero() {
            return Err(smelt_py_zero_division("division by zero"));
        }
        if let (Self::Small(lhs), Self::Small(rhs)) = (self, rhs)
            && lhs.unsigned_abs() <= EXACT
            && rhs.unsigned_abs() <= EXACT
        {
            return Ok(*lhs as f64 / *rhs as f64);
        }
        let negative = self.is_negative() != rhs.is_negative();
        let numerator = self.to_big().magnitude().clone();
        let denominator = rhs.to_big().magnitude().clone();
        if ::num_traits::Zero::is_zero(&numerator) {
            return Ok(if negative { -0.0 } else { 0.0 });
        }
        let shift = 55 + i64::try_from(denominator.bits()).unwrap_or(i64::MAX) - i64::try_from(numerator.bits()).unwrap_or(i64::MAX);
        let (scaled_numerator, scaled_denominator) = if shift >= 0 {
            (numerator << usize::try_from(shift).unwrap_or(0), denominator)
        } else {
            (numerator, denominator << usize::try_from(-shift).unwrap_or(0))
        };
        let (quotient, remainder) = ::num_integer::Integer::div_rem(&scaled_numerator, &scaled_denominator);
        let mut quotient = u64::try_from(&quotient).unwrap_or(u64::MAX);
        if !::num_traits::Zero::is_zero(&remainder) {
            quotient |= 1;
        }
        let magnitude = smelt_py_ldexp(quotient as f64, -shift);
        if magnitude.is_infinite() {
            return Err(smelt_py_overflow_error("integer division result too large for a float"));
        }
        Ok(if negative { -magnitude } else { magnitude })
    }

    //~ `base ** exponent` for a non-negative exponent: exact. A negative
    //~ exponent makes `CPython` answer a `float`, which an `int`-typed result
    //~ cannot hold; the frontend types a `**` whose exponent may be negative
    //~ as `float` wherever the source says so, and this answers the rest with
    //~ a `ValueError` naming the mismatch rather than a wrong value.
    fn pow(&self, exponent: &Self) -> Result<Self, Box<dyn ::std::error::Error>> {
        if exponent.is_negative() {
            return Err(smelt_py_value_error("negative exponent: Python answers a float here, which this int-typed expression cannot hold"));
        }
        if let (Self::Small(base), Self::Small(power)) = (self, exponent)
            && let Ok(power) = u32::try_from(*power)
            && let Some(value) = base.checked_pow(power)
        {
            return Ok(Self::Small(value));
        }
        match self.as_small() {
            Some(0) => return Ok(Self::Small(i64::from(exponent.is_zero()))),
            Some(1) => return Ok(Self::Small(1)),
            Some(-1) => {
                let odd = ::num_integer::Integer::is_odd(&exponent.to_big());
                return Ok(Self::Small(if odd { -1 } else { 1 }));
            }
            _ => {}
        }
        let power = u32::try_from(&exponent.to_big()).map_err(|_too_wide| smelt_py_overflow_error("exponent too large"))?;
        Ok(Self::from_big(::num_traits::Pow::pow(self.to_big(), power)))
    }

    //~ `base ** exponent` as a `float`, for an exponent that is negative:
    //~ `CPython` converts both operands to `float` first.
    fn pow_float(&self, exponent: &Self) -> Result<f64, Box<dyn ::std::error::Error>> {
        smelt_py_float_pow(self.to_f64()?, exponent.to_f64()?)
    }

    //~ `pow(base, exponent, modulus)`: the result takes the modulus's sign,
    //~ and a negative exponent asks for the modular inverse.
    fn pow_mod(&self, exponent: &Self, modulus: &Self) -> Result<Self, Box<dyn ::std::error::Error>> {
        if modulus.is_zero() {
            return Err(smelt_py_value_error("pow() 3rd argument cannot be 0"));
        }
        let modulus_big = modulus.to_big();
        let magnitude = ::num_bigint::BigInt::from(modulus_big.magnitude().clone());
        let mut base = ::num_integer::Integer::mod_floor(&self.to_big(), &magnitude);
        let mut power = exponent.to_big();
        if power.sign() == ::num_bigint::Sign::Minus {
            let gcd = ::num_integer::Integer::extended_gcd(&base, &magnitude);
            if gcd.gcd != ::num_bigint::BigInt::from(1) {
                return Err(smelt_py_value_error("base is not invertible for the given modulus"));
            }
            base = ::num_integer::Integer::mod_floor(&gcd.x, &magnitude);
            power = -power;
        }
        let mut result = base.modpow(&power, &magnitude);
        if modulus.is_negative() && !::num_traits::Zero::is_zero(&result) {
            result += &modulus_big;
        }
        Ok(Self::from_big(result))
    }

    //~ `n << count`: a negative count is a `ValueError`.
    fn shl(&self, count: &Self) -> Result<Self, Box<dyn ::std::error::Error>> {
        if count.is_negative() {
            return Err(smelt_py_value_error("negative shift count"));
        }
        if self.is_zero() {
            return Ok(Self::Small(0));
        }
        let count = count.as_small().and_then(|count| usize::try_from(count).ok()).ok_or_else(|| smelt_py_overflow_error("too many digits in integer"))?;
        if let Self::Small(value) = self
            && count < 63
            && value.unsigned_abs() < (1_u64 << (63 - count))
        {
            return Ok(Self::Small(value << count));
        }
        Ok(Self::from_big(self.to_big() << count))
    }

    //~ `n >> count`: floors, so a negative value shifted past its last bit
    //~ is `-1`; a negative count is a `ValueError`.
    fn shr(&self, count: &Self) -> Result<Self, Box<dyn ::std::error::Error>> {
        if count.is_negative() {
            return Err(smelt_py_value_error("negative shift count"));
        }
        let Some(count) = count.as_small().and_then(|count| usize::try_from(count).ok()) else {
            return Ok(Self::Small(if self.is_negative() { -1 } else { 0 }));
        };
        if let Self::Small(value) = self {
            return Ok(Self::Small(if count >= 64 { if *value < 0 { -1 } else { 0 } } else { value >> count }));
        }
        Ok(Self::from_big(self.to_big() >> count))
    }

    //~ `float(n)`: correctly rounded; a value past `f64`'s range is an
    //~ `OverflowError`.
    #[allow(clippy::cast_precision_loss, reason = "the nearest f64 is what float(n) answers")]
    fn to_f64(&self) -> Result<f64, Box<dyn ::std::error::Error>> {
        let value = match self {
            Self::Small(value) => *value as f64,
            Self::Big(value) => ::num_traits::ToPrimitive::to_f64(&**value).unwrap_or(f64::INFINITY),
        };
        if value.is_infinite() {
            return Err(smelt_py_overflow_error("int too large to convert to float"));
        }
        Ok(value)
    }

    //~ The nearest `f64`, infinite past its range: the value a TypeScript
    //~ `number` holds for this integer.
    #[allow(clippy::cast_precision_loss, reason = "the nearest f64 is what a TypeScript number holds")]
    fn to_f64_lossy(&self) -> f64 {
        match self {
            Self::Small(value) => *value as f64,
            Self::Big(value) => ::num_traits::ToPrimitive::to_f64(&**value).unwrap_or(f64::INFINITY),
        }
    }

    //~ `int(x)` of a `float`: truncation toward zero.
    fn from_f64(value: f64) -> Result<Self, Box<dyn ::std::error::Error>> {
        if value.is_nan() {
            return Err(smelt_py_value_error("cannot convert float NaN to integer"));
        }
        if value.is_infinite() {
            return Err(smelt_py_overflow_error("cannot convert float infinity to integer"));
        }
        let truncated = value.trunc();
        if truncated.abs() < 9.2e18 {
            #[allow(clippy::cast_possible_truncation, reason = "a whole float below 2^63 in magnitude converts exactly")]
            return Ok(Self::Small(truncated as i64));
        }
        Ok(Self::from_big(::num_traits::FromPrimitive::from_f64(truncated).unwrap_or_default()))
    }

    //~ The value as a sequence index: one that does not fit a machine word is
    //~ the `IndexError` `CPython` raises before it looks at the sequence.
    fn to_index(&self) -> Result<i64, Box<dyn ::std::error::Error>> {
        self.as_small().ok_or_else(|| smelt_py_index_error("cannot fit 'int' into an index-sized integer"))
    }

    //~ The value as a size (a repeat count, a width): `OverflowError` when it
    //~ does not fit a machine word, as `CPython`'s `PyNumber_AsSsize_t`.
    fn to_size(&self) -> Result<i64, Box<dyn ::std::error::Error>> {
        self.as_small().ok_or_else(|| smelt_py_overflow_error("cannot fit 'int' into an index-sized integer"))
    }

    //~ The value as a machine index for a position the runtime proved is in
    //~ range (a `range` element below a list's length).
    fn to_i64_saturating(&self) -> i64 {
        match self {
            Self::Small(value) => *value,
            Self::Big(value) => if value.sign() == ::num_bigint::Sign::Minus { i64::MIN } else { i64::MAX },
        }
    }

    //~ Exact comparison with a `float`, as `CPython`'s `float_richcompare`:
    //~ `2**53 + 1 > 2.0**53` is `True` although both round to the same
    //~ `float`. `None` against `NaN`.
    fn cmp_f64(&self, other: f64) -> Option<::std::cmp::Ordering> {
        if other.is_nan() {
            return None;
        }
        if other.is_infinite() {
            return Some(if other > 0.0 { ::std::cmp::Ordering::Less } else { ::std::cmp::Ordering::Greater });
        }
        let whole = other.trunc();
        let whole_int = Self::from_f64(whole).ok()?;
        match self.cmp(&whole_int) {
            ::std::cmp::Ordering::Equal => (0.0).partial_cmp(&(other - whole)),
            unequal => Some(unequal),
        }
    }

    //~ `n == x` for a `float` `x`.
    fn eq_f64(&self, other: f64) -> bool {
        self.cmp_f64(other) == Some(::std::cmp::Ordering::Equal)
    }

    //~ The elements of `range(start, stop, step)`, in order: a zero step is a
    //~ `ValueError`.
    fn range(start: &Self, limit: &Self, step: &Self) -> Result<Vec<Self>, Box<dyn ::std::error::Error>> {
        if step.is_zero() {
            return Err(smelt_py_value_error("range() arg 3 must not be zero"));
        }
        Ok(Self::range_values(start, limit, step))
    }

    //~ The elements of `range(start, stop, step)` for a step the frontend
    //~ proved nonzero (a nonzero constant); a zero step yields nothing rather
    //~ than looping. Small bounds count in machine words.
    fn range_values(start: &Self, limit: &Self, step: &Self) -> Vec<Self> {
        let mut values = Vec::new();
        if step.is_zero() {
            return values;
        }
        if let (Self::Small(first), Self::Small(last), Self::Small(stride)) = (start, limit, step) {
            let (mut current, last, stride) = (i128::from(*first), i128::from(*last), i128::from(*stride));
            while (stride > 0 && current < last) || (stride < 0 && current > last) {
                values.push(Self::Small(i64::try_from(current).unwrap_or_default()));
                current += stride;
            }
            return values;
        }
        let ascending = !step.is_negative();
        let mut current = start.clone();
        while (ascending && current < *limit) || (!ascending && current > *limit) {
            let next = &current + step;
            values.push(current);
            current = next;
        }
        values
    }

    //~ The base a digit string reads in, from its `0x`/`0o`/`0b` prefix when it
    //~ has one that `base` allows: `(prefix base, base, digits, whether a
    //~ prefix was consumed)`.
    fn parse_base(unsigned: &str, base: u32) -> (Option<u32>, u32, &str, bool) {
        let prefix_base = match unsigned.get(..2).map(str::to_ascii_lowercase).as_deref() {
            Some("0x") => Some(16),
            Some("0o") => Some(8),
            Some("0b") => Some(2),
            _ => None,
        };
        match (base, prefix_base) {
            (0 | 2 | 8 | 16, Some(found)) if base == 0 || base == found => (prefix_base, found, unsigned.get(2..).unwrap_or_default(), true),
            (0, _) => (prefix_base, 10, unsigned, false),
            (base, _) => (prefix_base, base, unsigned, false),
        }
    }

    //~ `int(text, base)` with a Python `int` base: one outside 2..=36 (and not
    //~ 0) is a `ValueError`.
    fn parse_with_base(text: &str, base: &Self) -> Result<Self, Box<dyn ::std::error::Error>> {
        match base.as_small().and_then(|base| u32::try_from(base).ok()) {
            Some(base) if base == 0 || (2..=36).contains(&base) => Self::parse(text, base),
            _ => Err(smelt_py_value_error("int() base must be >= 2 and <= 36, or 0")),
        }
    }

    //~ `int(text, base)`, with `CPython`'s grammar: surrounding whitespace,
    //~ an optional sign, a `0x`/`0o`/`0b` prefix (required to match `base`
    //~ 16/8/2 if present, and deciding it for base 0), digits with single
    //~ `_` separators between them, and any Unicode decimal digit. Anything
    //~ else is the `ValueError` naming the text.
    fn parse(text: &str, base: u32) -> Result<Self, Box<dyn ::std::error::Error>> {
        let invalid = || {
            let mut shown = text.to_owned().py_repr();
            if shown.chars().count() > 200 {
                shown = shown.chars().take(200).collect();
            }
            smelt_py_value_error(&format!("invalid literal for int() with base {base}: {shown}"))
        };
        if base == 1 || base > 36 {
            return Err(smelt_py_value_error("int() base must be >= 2 and <= 36, or 0"));
        }
        let digit_of = |ch: char| -> Option<u32> {
            if ch.is_ascii() {
                return ch.to_digit(36);
            }
            let code = u32::from(ch);
            let zero = SMELT_PY_UNICODE_DIGIT_ZEROS.iter().rev().find(|zero| **zero <= code)?;
            (code - zero < 10).then_some(code - zero)
        };
        let trimmed = text.trim_matches(|ch: char| if ch.is_ascii() { matches!(ch, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r') } else { ch.is_whitespace() });
        let (negative, unsigned) = match trimmed.as_bytes().first() {
            Some(b'-') => (true, &trimmed[1..]),
            Some(b'+') => (false, &trimmed[1..]),
            _ => (false, trimmed),
        };
        let requested_base = base;
        let (prefix_base, base, digits, after_prefix) = Self::parse_base(unsigned, base);
        //~ Base 0 refuses a decimal with a leading zero (`010`), which would
        //~ be ambiguous with C's octal; only all-zero text may start with one.
        let mut values = Vec::with_capacity(digits.len());
        let mut previous_underscore = !after_prefix;
        for ch in digits.chars() {
            if ch == '_' {
                if previous_underscore {
                    return Err(invalid());
                }
                previous_underscore = true;
                continue;
            }
            let value = digit_of(ch).filter(|value| *value < base).ok_or_else(invalid)?;
            values.push(value);
            previous_underscore = false;
        }
        if values.is_empty() || previous_underscore {
            return Err(invalid());
        }
        if requested_base == 0 && prefix_base.is_none() && values[0] == 0 && values.iter().any(|value| *value != 0) {
            return Err(invalid());
        }
        let bytes: Vec<u8> = values.iter().map(|value| u8::try_from(*value).unwrap_or(0)).collect();
        let magnitude = ::num_bigint::BigUint::from_radix_be(&bytes, base).ok_or_else(invalid)?;
        let value = ::num_bigint::BigInt::from_biguint(if negative { ::num_bigint::Sign::Minus } else { ::num_bigint::Sign::Plus }, magnitude);
        Ok(Self::from_big(value))
    }
}

/// `float(text)` with `CPython`'s grammar: surrounding whitespace, a decimal
/// or exponent literal with single `_` separators between digits, or a signed
/// `inf`/`infinity`/`nan` in any case. Anything else is the `ValueError`
/// naming the text.
#[allow(dead_code)]
fn smelt_py_float_parse(text: &str) -> Result<f64, Box<dyn ::std::error::Error>> {
    let invalid = || smelt_py_value_error(&format!("could not convert string to float: {}", text.to_owned().py_repr()));
    let trimmed = text.trim_matches(|ch: char| if ch.is_ascii() { matches!(ch, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r') } else { ch.is_whitespace() });
    let bytes = trimmed.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'_' {
            let digit_before = index > 0 && bytes[index - 1].is_ascii_digit();
            let digit_after = bytes.get(index + 1).is_some_and(u8::is_ascii_digit);
            if !digit_before || !digit_after {
                return Err(invalid());
            }
        }
    }
    let cleaned = trimmed.replace('_', "");
    let unsigned = cleaned.trim_start_matches(['+', '-']);
    //~ Rust's parser also takes `infinity`/`nan` spellings Python takes, but
    //~ not a second sign, and Python refuses a hex float or a bare sign.
    if unsigned.len() + 1 < cleaned.len() || unsigned.is_empty() {
        return Err(invalid());
    }
    cleaned.parse::<f64>().map_err(|_malformed| invalid())
}

/// `base ** exponent` for two `float`s, with `CPython`'s failures: zero to a
/// negative power is a `ZeroDivisionError`, a finite result too large for a
/// `float` an `OverflowError`, and a negative base to a fractional power (a
/// complex number in `CPython`, which has no `float` value) a `ValueError`.
#[allow(dead_code)]
fn smelt_py_float_pow(base: f64, exponent: f64) -> Result<f64, Box<dyn ::std::error::Error>> {
    if base == 0.0 && exponent < 0.0 && exponent.is_finite() {
        return Err(smelt_py_zero_division("zero to a negative power"));
    }
    if base < 0.0 && base.is_finite() && exponent.is_finite() && exponent.fract() != 0.0 {
        return Err(smelt_py_value_error("a negative number raised to a fractional power has a complex result, which a float cannot hold"));
    }
    let result = base.powf(exponent);
    if result.is_infinite() && base.is_finite() && exponent.is_finite() {
        return Err(smelt_py_overflow_error("(34, 'Numerical result out of range')"));
    }
    Ok(result)
}

impl Default for SmeltPyInt {
    fn default() -> Self {
        Self::Small(0)
    }
}

impl From<i64> for SmeltPyInt {
    fn from(value: i64) -> Self {
        Self::Small(value)
    }
}

impl From<i32> for SmeltPyInt {
    fn from(value: i32) -> Self {
        Self::Small(i64::from(value))
    }
}

impl From<u64> for SmeltPyInt {
    fn from(value: u64) -> Self {
        i64::try_from(value).map_or_else(|_| Self::from_big(::num_bigint::BigInt::from(value)), Self::Small)
    }
}

impl From<usize> for SmeltPyInt {
    fn from(value: usize) -> Self {
        Self::from(value as u64)
    }
}

impl From<bool> for SmeltPyInt {
    fn from(value: bool) -> Self {
        Self::Small(i64::from(value))
    }
}

impl ::std::fmt::Display for SmeltPyInt {
    fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match self {
            Self::Small(value) => ::std::fmt::Display::fmt(value, formatter),
            Self::Big(value) => ::std::fmt::Display::fmt(&**value, formatter),
        }
    }
}

impl ::std::fmt::Debug for SmeltPyInt {
    fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        ::std::fmt::Display::fmt(self, formatter)
    }
}

impl PartialOrd for SmeltPyInt {
    fn partial_cmp(&self, other: &Self) -> Option<::std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SmeltPyInt {
    fn cmp(&self, other: &Self) -> ::std::cmp::Ordering {
        match (self, other) {
            (Self::Small(lhs), Self::Small(rhs)) => lhs.cmp(rhs),
            (lhs, rhs) => lhs.to_big().cmp(&rhs.to_big()),
        }
    }
}

impl SmeltPyRepr for SmeltPyInt {
    fn py_repr(&self) -> String {
        self.to_string()
    }
}

/// One exact binary operator on two `SmeltPyInt`s: the checked machine
/// operation when both are small, the `BigInt` one otherwise. Every owned and
/// borrowed operand combination forwards to the borrowed one.
macro_rules! smelt_py_int_binary {
    ($trait:ident, $method:ident, $checked:expr) => {
        impl ::std::ops::$trait<&SmeltPyInt> for &SmeltPyInt {
            type Output = SmeltPyInt;
            fn $method(self, rhs: &SmeltPyInt) -> SmeltPyInt {
                if let (SmeltPyInt::Small(lhs), SmeltPyInt::Small(rhs)) = (self, rhs) {
                    let checked: fn(i64, i64) -> Option<i64> = $checked;
                    if let Some(value) = checked(*lhs, *rhs) {
                        return SmeltPyInt::Small(value);
                    }
                }
                SmeltPyInt::from_big(::std::ops::$trait::$method(self.to_big(), rhs.to_big()))
            }
        }
        impl ::std::ops::$trait<SmeltPyInt> for SmeltPyInt {
            type Output = SmeltPyInt;
            fn $method(self, rhs: SmeltPyInt) -> SmeltPyInt {
                ::std::ops::$trait::$method(&self, &rhs)
            }
        }
        impl ::std::ops::$trait<&SmeltPyInt> for SmeltPyInt {
            type Output = SmeltPyInt;
            fn $method(self, rhs: &SmeltPyInt) -> SmeltPyInt {
                ::std::ops::$trait::$method(&self, rhs)
            }
        }
        impl ::std::ops::$trait<SmeltPyInt> for &SmeltPyInt {
            type Output = SmeltPyInt;
            fn $method(self, rhs: SmeltPyInt) -> SmeltPyInt {
                ::std::ops::$trait::$method(self, &rhs)
            }
        }
    };
}

smelt_py_int_binary!(Add, add, i64::checked_add);
smelt_py_int_binary!(Sub, sub, i64::checked_sub);
smelt_py_int_binary!(Mul, mul, i64::checked_mul);
smelt_py_int_binary!(BitAnd, bitand, |lhs, rhs| Some(lhs & rhs));
smelt_py_int_binary!(BitOr, bitor, |lhs, rhs| Some(lhs | rhs));
smelt_py_int_binary!(BitXor, bitxor, |lhs, rhs| Some(lhs ^ rhs));

impl ::std::ops::Neg for &SmeltPyInt {
    type Output = SmeltPyInt;
    fn neg(self) -> SmeltPyInt {
        match self {
            SmeltPyInt::Small(value) => value.checked_neg().map_or_else(|| SmeltPyInt::from_big(-::num_bigint::BigInt::from(*value)), SmeltPyInt::Small),
            SmeltPyInt::Big(value) => SmeltPyInt::from_big(-&**value),
        }
    }
}

impl ::std::ops::Neg for SmeltPyInt {
    type Output = Self;
    fn neg(self) -> Self {
        -&self
    }
}

/// `~n`, which is `-n - 1` at every magnitude.
impl ::std::ops::Not for &SmeltPyInt {
    type Output = SmeltPyInt;
    fn not(self) -> SmeltPyInt {
        match self {
            SmeltPyInt::Small(value) => SmeltPyInt::Small(!value),
            SmeltPyInt::Big(value) => SmeltPyInt::from_big(!&**value),
        }
    }
}

impl ::std::ops::Not for SmeltPyInt {
    type Output = Self;
    fn not(self) -> Self {
        !&self
    }
}

// @smelt:if py_repr
//~ `json.dumps` writes an `int` as its exact decimal digits, and a dict key
//~ as those digits quoted, at every magnitude.
impl SmeltPyJson for SmeltPyInt {
    fn py_json(&self, out: &mut String) {
        out.push_str(&self.to_string());
    }
}

impl SmeltPyJsonKey for SmeltPyInt {
    fn py_json_key(&self, out: &mut String) {
        out.push('"');
        out.push_str(&self.to_string());
        out.push('"');
    }
}
// @smelt:end

// @smelt:if record
//~ A Python dict keyed by `int` is a `SmeltRecord`, the shared container a
//~ dict's reference semantics need. The key hashes its two's-complement
//~ bytes, which the normalised representation makes one spelling per value.
impl SmeltPropertyKey for SmeltPyInt {
    fn smelt_array_index(&self) -> Option<u32> {
        None
    }
    #[inline]
    fn smelt_key_hash(&self) -> u64 {
        smelt_field_hash_bytes(&self.key_bytes())
    }
    #[inline]
    fn smelt_key_fingerprint(&self) -> u32 {
        smelt_field_fingerprint(&self.key_bytes())
    }
    #[inline]
    fn smelt_is_internal_key(&self) -> bool {
        false
    }
}

#[allow(dead_code)]
impl SmeltPyInt {
    //~ The bytes a dict key hashes: the little-endian two's complement.
    fn key_bytes(&self) -> Vec<u8> {
        match self {
            Self::Small(value) => value.to_le_bytes().to_vec(),
            Self::Big(value) => value.to_signed_bytes_le(),
        }
    }
}
// @smelt:end

// @smelt:if unknown
//~ A Python `int` crossing into the erased carrier (a TypeScript `unknown`
//~ or `any` slot of a mixed crate) is a JavaScript `number`, the value Node
//~ reads for a Python integer: exact up to 2^53, the nearest double beyond.
impl IntoSmeltUnknown for SmeltPyInt {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        SmeltUnknown::Number(self.to_f64_lossy())
    }
}

//~ Back out of the carrier, a number is the `int` of its whole part (the
//~ value a `number` holding an integer is) and a boolean is `0` or `1`;
//~ nothing else names an integer.
impl SmeltFromUnknown for SmeltPyInt {
    fn smelt_from_unknown(value: SmeltUnknown) -> Self {
        match value {
            SmeltUnknown::Number(value) => Self::from_f64(value).unwrap_or_default(),
            SmeltUnknown::Bool(value) => Self::from(value),
            _ => Self::default(),
        }
    }
}
// @smelt:end

impl ::std::iter::Sum for SmeltPyInt {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::Small(0), |total, item| &total + &item)
    }
}

impl<'a> ::std::iter::Sum<&'a Self> for SmeltPyInt {
    fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
        iter.fold(Self::Small(0), |total, item| &total + item)
    }
}

// @smelt:item-end
