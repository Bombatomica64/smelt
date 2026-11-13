"""The builtins, closures, JSON, regex and sequence operations print what CPython prints."""
import json
import re
from typing import Callable


def apply(value: int, fn: Callable[[int], int]) -> int:
    return fn(value)


def make_adder(base: int) -> Callable[[int], int]:
    add: Callable[[int], int] = lambda value: value + base
    return add


def make_nested(base: int) -> Callable[[int], int]:
    def add(value: int) -> int:
        return value + base
    return add


def closures() -> None:
    offset: int = 2
    add_offset: Callable[[int], int] = lambda value: value + offset
    print(add_offset(3), apply(4, add_offset), make_adder(5)(6), make_nested(5)(6))
    bump: Callable[[int], int] = lambda value=1: value + 1
    print(bump(), bump(4))
    sum_two: Callable[[int, int], int] = lambda *values: values[0] + values[1]
    print(sum_two(2, 3))


def variadics() -> int:
    def nested_sum(*values: int) -> int:
        return values[0] + values[1]

    def nested_pick(**kwargs: int) -> int:
        return kwargs["value"] + kwargs["bonus"]

    extras: dict[str, int] = {"bonus": 6}
    return nested_sum(2, 3, 4) + nested_pick(value=5, **extras)


def sum_values(*values: int) -> int:
    return values[0] + values[1]


def pick(**kwargs: int) -> int:
    return kwargs["value"] + kwargs["bonus"]


def aggregates() -> None:
    ints: list[int] = [1, 2]
    floats: list[float] = [1.0, 2.5]
    print(sum(ints), sum(floats))
    flags: list[bool] = [True, False]
    print(all(flags), any(flags))
    factor: int = 2
    values: list[int] = [1, 2, 3]
    print(list(map(lambda value: value * factor, values)))
    print(list(filter(lambda value: value > factor, values)))


def ordering() -> None:
    print(sorted([2, 1, 3]), sorted([2.5, 1.5]))
    bias: int = 10
    print(sorted([2, 1, 3], key=lambda value: value + bias, reverse=True))
    values: list[int] = [3, 1, 2]
    values.sort(key=lambda value: value * 2)
    print(values)
    print(list(reversed([1, 2])))


def pairs() -> None:
    names: list[str] = ["Ada", "Linus"]
    scores: list[int] = [1, 2]
    for index, name in enumerate(names):
        print(index, name)
    for name, score in zip(names, scores):
        print(name, score)


def conversions() -> None:
    print(str(True), int(3.8), int("42"), float(7), float("2.5"), bool(""), bool("x"))


def ranges() -> None:
    print(list(range(3)), list(range(1, 4)), list(range(5, 1, -2)))
    total: int = 0
    for value in range(3):
        total = total + value
    print(total)


def json_round_trip() -> None:
    values: list[int] = [1, 2]
    round_trip: list[int] = json.loads(json.dumps(values))
    print(round_trip)
    parsed: list[int] = json.loads("[1,2]")
    print(parsed[1] + 1)


def regex() -> None:
    text: str = "abc123"
    print(bool(re.search("\\d+", text)))
    print(bool(re.match("\\d+", text)), bool(re.match("[a-z]+", text)))
    print(bool(re.fullmatch("[a-z]+", text)), bool(re.fullmatch("[a-z0-9]+", text)))


def iterate_set_and_dict() -> None:
    total: int = 0
    for item in {1, 2}:
        total = total + item
    print(total)
    last: str = ""
    for name in {"Ada": 1, "Linus": 2}:
        last = name
    print(last)


def text_search() -> None:
    word: str = "Smelt"
    print(word.find("m"), word.rfind("t"), word.find("z"))


def slices() -> None:
    values: list[int] = [1, 2, 3, 4]
    print(values[:], values[1:], values[1:3], values[-2:])
    word: str = "smelting"
    print(word[:], word[1:], word[1:4], word[-3:])
    print(values[-1], word[-1])
    pair: tuple[str, int] = ("Ada", 1)
    print(pair[0], pair[-1], pair[1:])


def list_mutation() -> None:
    values: list[int] = [1, 2]
    values.append(3)
    print(values)
    values.extend([4, 5])
    print(values)
    values.insert(1, 0)
    print(values)
    values.reverse()
    print(values)
    print(values.pop(), values)
    values.clear()
    print(values)
    mapping: dict[str, int] = {"a": 1}
    mapping.clear()
    print(mapping)


closures()
print(variadics())
print(sum_values(2, 3, 4))
print(pick(value=5, bonus=6))
aggregates()
ordering()
pairs()
conversions()
ranges()
json_round_trip()
regex()
iterate_set_and_dict()
text_search()
slices()
list_mutation()
