# `int(...)` and `float(...)` follow CPython's grammars, and a malformed or
# out-of-range input raises an exception an `except` catches.


def parse_int(text: str) -> str:
    try:
        return str(int(text))
    except ValueError as error:
        return "ValueError: " + str(error)


def parse_int_base(text: str, base: int) -> str:
    try:
        return str(int(text, base))
    except ValueError as error:
        return "ValueError: " + str(error)


def parse_float(text: str) -> str:
    try:
        return repr(float(text))
    except ValueError as error:
        return "ValueError: " + str(error)


def truncate(value: float) -> str:
    try:
        return str(int(value))
    except ValueError as error:
        return "ValueError: " + str(error)
    except OverflowError as error:
        return "OverflowError: " + str(error)


def main() -> None:
    for text in ["12", " -42 ", "+7", "1_000", "0", "-0", "99999999999999999999999", "١٢"]:
        print(parse_int(text))
    for text in ["x", "", "1__0", "_1", "1_", "0x1f", "1.5", " ", "12a"]:
        print(parse_int(text))
    print(parse_int_base("ff", 16), parse_int_base("0x_ff", 16), parse_int_base("0b101", 0))
    print(parse_int_base("z", 36), parse_int_base("010", 0), parse_int_base("12", 1), parse_int_base("9", 8))
    for text in ["1.5", " 2 ", "-0.0", "1e3", "1_000.5", ".5", "5.", "inf", "-Infinity", "nan"]:
        print(parse_float(text))
    for text in ["x", "", "1__0", "1.5.5", "--1", "0x1p3"]:
        print(parse_float(text))
    for value in [2.9, -2.9, 1e20, 0.5]:
        print(truncate(value))
    print(truncate(float("nan")), truncate(float("inf")))
    print(bool(0), bool(2 ** 70), int(True), float(10 ** 20), float(-7))


main()
