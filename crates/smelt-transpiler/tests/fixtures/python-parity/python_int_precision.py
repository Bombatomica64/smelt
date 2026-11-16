# Python `int` is unbounded: every operator is exact past 64 bits, and the
# operators keep CPython's rounding and errors.
import math


def doubled(times: int) -> int:
    value: int = 1
    for _ in range(times):
        value = value * 2
    return value


def factorial(n: int) -> int:
    result: int = 1
    i: int = 2
    while i <= n:
        result *= i
        i += 1
    return result


def main() -> None:
    print(doubled(10), doubled(64), doubled(70))
    print(factorial(25))
    top: int = 9223372036854775807
    print(top + 1, -top - 2, top * top, -9223372036854775808 - 1)
    print(2 ** 100, (-3) ** 3, 0 ** 0, 7 ** 2, 2 ** -1)
    print(-7 // 2, -7 % 3, 7 // -2, 7 % -2, 10 ** 20 // 3, 10 ** 20 % 7)
    print(7 / 2, 10 ** 20 / 3, 9007199254740993 / 1, (2 ** 1024) // 3 / 2 ** 1000)
    print(1 << 70, -7 >> 1, -1 >> 100, ~5, ~(-(2 ** 70)), 0xFF & -1, 6 | 9, 6 ^ 3)
    print((2 ** 53 + 1) > 2.0 ** 53, 2 ** 53 + 1 == 2.0 ** 53, 3 < 3.5, 4 >= 4.0, 2 ** 70 != 2.0 ** 70)
    print(abs(-(2 ** 70)), max(2 ** 64, 3), min(-(2 ** 64), 3), sum([10 ** 19, 10 ** 19, 1]))
    print(math.floor(1e20), math.ceil(-2.5), math.trunc(-2.5), math.floor(7))
    print([n for n in range(2 ** 63 - 2, 2 ** 63 + 1)], len(range(5)))
    big_keys: dict[int, str] = {2 ** 64: "wide", 1: "one"}
    print(big_keys[2 ** 64], big_keys)
    print(f"{2 ** 65}", str(-(10 ** 21)), repr(10 ** 22), 0x1_0000_0000_0000_0000)
    try:
        print(1 // 0)
    except ZeroDivisionError as error:
        print("ZeroDivisionError:", error)
    try:
        print(5 % 0)
    except ZeroDivisionError as error:
        print("ZeroDivisionError:", error)
    try:
        print(2 ** 2000 / 0)
    except ZeroDivisionError as error:
        print("ZeroDivisionError:", error)
    try:
        print(1 << -1)
    except ValueError as error:
        print("ValueError:", error)
    try:
        print(float(2 ** 1024))
    except OverflowError as error:
        print("OverflowError:", error)
    try:
        print(2 ** 1024 / 1)
    except OverflowError as error:
        print("OverflowError:", error)
    try:
        print(2 ** 1024 + 0.5)
    except OverflowError as error:
        print("OverflowError:", error)
    items: list[int] = [10, 20, 30]
    try:
        print(items[2 ** 70])
    except IndexError as error:
        print("IndexError:", error)
    print(items[-1], items[: 2 ** 70], items[-(2 ** 70):1])
    try:
        print("ab" * 2 ** 70)
    except OverflowError as error:
        print("OverflowError:", error)


main()
