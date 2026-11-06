"""A lambda body is an ordinary Python expression: its operators mean what they mean anywhere."""
from typing import Callable

nums: list[int] = [3, -4]
floats: list[float] = [7.5, -7.5]

print(list(map(lambda x: x / 2, nums)))
print(list(map(lambda x: x % 3, nums)))
print(list(map(lambda x: x + 0.5, nums)))
print(list(map(lambda x: x // 2, nums)))
print(list(map(lambda x: x % -3, nums)))
print(list(map(lambda x: x // -2, nums)))
print(list(map(lambda x: x % 2.5, floats)))
print(list(map(lambda x: x // 2.5, floats)))
print(list(map(lambda x: x % -2.5, floats)))
print(list(map(lambda x: x // -2, floats)))
print(list(map(lambda x: x % 3, floats)))

scale: int = 4
print(list(map(lambda x: x * scale % 5, nums)))
print(list(map(lambda x: abs(x) + len(nums), nums)))
print(list(map(lambda x: [x, x + 1][1], nums)))
print(list(map(lambda x: "neg" if x < 0 else "pos", nums))[1])
print(list(filter(lambda x: x % 2 == 0, nums)))
print(sorted(nums, key=lambda x: x % 3))
print(sorted(nums, key=lambda x: x / 2, reverse=True))

half: Callable[[int], float] = lambda x: x / 2
print(half(3))
print(half(-3))
print(list(map(half, nums)))

late: int = 2
times: Callable[[int], int] = lambda x: x * late
late = 3
print(times(5))
late = 4
print(times(5))

both: Callable[[int, int], int] = lambda *values: values[0] % values[1]
print(both(-7, 3))


def apply(fn: Callable[[int], float], value: int) -> float:
    return fn(value)


def mapped(fn: Callable[[int], int], values: list[int]) -> list[int]:
    return list(map(fn, values))


def make_divider(by: int) -> Callable[[int], float]:
    def divide(value: int) -> float:
        return value / by

    return divide


print(apply(lambda x: x / 4, 10))
print(mapped(lambda x: x % 4, [-1, 6]))
print(make_divider(8)(2))


def ratio(a: int, b: int) -> float:
    return a / b


def floored(a: float, b: float) -> float:
    return a // b


def modulo(a: int, b: int) -> int:
    return a % b


print(ratio(7, 2))
print(floored(-7.5, 2.0))
print(modulo(-7, 2))

try:
    print(ratio(1, 0))
except ZeroDivisionError as error:
    print("caught:", error)

try:
    print(modulo(1, 0))
except ZeroDivisionError as error:
    print("caught:", error)

try:
    print(floored(1.0, 0.0))
except ZeroDivisionError as error:
    print("caught:", error)

total: int = 17
total %= 5
print(total)
total //= -2
print(total)
ratio_value: float = 9.0
ratio_value /= 2
print(ratio_value)
ratio_value %= 4
print(ratio_value)
ratio_value //= 0.25
print(ratio_value)

table: dict[str, int] = {"a": 1}
table["b"] = 2
table["b"] += 3
print(table["a"], table["b"])
try:
    print(table["missing"])
except KeyError as error:
    print("caught:", error)


def keyed(values: dict[str, int], key: str) -> Callable[[int], int]:
    def add(x: int) -> int:
        return x + values[key]

    return add


print(keyed(table, "b")(1))
try:
    print(keyed(table, "nope")(1))
except KeyError as error:
    print("caught:", error)
numbers: dict[int, int] = {1: 2}
try:
    print(numbers[3])
except KeyError as error:
    print("caught:", error)

one: int = 1
print(sorted(nums, key=lambda x: x % one))
print(sorted(nums, key=lambda x: x / one, reverse=True))
print(nums)
