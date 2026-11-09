"""List, dict, set, tuple and str methods, and the math and random helpers."""

import math
import random


def list_methods() -> None:
    values: list[int] = [3, 1, 2, 1]
    copied: list[int] = values.copy()
    copied.append(9)
    print(values, copied)
    print(values.count(1), values.count(7))
    print(values.index(2))
    values.remove(1)
    print(values)
    ints: list[int] = [2, 3, 1]
    ints.sort()
    print(ints)
    floats: list[float] = [2.5, -1.0, 0.5]
    floats.sort()
    print(floats)


def missing_list_items() -> None:
    values: list[int] = [1]
    try:
        values.index(5)
    except ValueError:
        print("index raised ValueError")
    try:
        values.remove(5)
    except ValueError:
        print("remove raised ValueError")


def container_constructors() -> None:
    values: list[int] = [1, 2]
    rebuilt: list[int] = list(values)
    rebuilt.append(3)
    print(values, rebuilt)
    empty_list: list[int] = list()
    empty_set: set[int] = set()
    empty_dict: dict[str, int] = dict()
    print(empty_list, len(empty_set), empty_dict)
    items: set[int] = {1, 2}
    copied_items: set[int] = set(items)
    copied_items.add(3)
    print(len(items), len(copied_items))
    print(sorted(set(values)))
    names: dict[str, int] = {"Ada": 1}
    copied_names: dict[str, int] = dict(names)
    copied_names["Bob"] = 2
    print(names, copied_names)
    pairs: list[tuple[str, int]] = [("Ada", 1), ("Bob", 2)]
    print(dict(pairs))
    print(sorted(list(items)), list(names))
    coords: tuple[int, int] = (1, 2)
    print(list(coords), sorted(set(coords)))


def sets_of_tuples() -> None:
    coords: tuple[float, float] = (1.5, 2.5)
    coord_set: set[float] = set(coords)
    print(sorted(coord_set))
    names: tuple[str, str] = ("a", "a")
    name_set: set[str] = set(names)
    print(sorted(name_set))


def dict_methods() -> None:
    mapping: dict[str, int] = {"a": 1}
    print(mapping.get("a"), mapping.get("z"), mapping.get("z", 5))
    print(mapping.setdefault("b", 2), mapping.setdefault("b", 9), mapping)
    print(mapping.pop("a"), mapping.pop("q", 0), mapping)
    try:
        mapping.pop("q")
    except KeyError:
        print("pop raised KeyError")
    left: dict[str, int] = {"a": 1, "b": 2}
    right: dict[str, int] = {"b": 3, "c": 4}
    left.update(right)
    print(left, right)
    copied: dict[str, int] = left.copy()
    copied["z"] = 0
    print(len(left), len(copied))
    print(list(left.keys()), list(left.values()), list(left.items()))


def string_methods() -> None:
    word: str = "hello hello"
    print(word.replace("hello", "hi"))
    affix: str = "pre-value-suf"
    print(affix.removeprefix("pre-"), affix.removesuffix("-suf"), affix.removeprefix("x"))
    print("123".isdigit(), "12a".isdigit(), "".isdigit())
    print("abc".isalpha(), "ab1".isalpha(), "".isalpha())
    print("ab1".isalnum(), "a b".isalnum(), "".isalnum())
    parts: list[str] = ["a", "b", "c"]
    print("-".join(parts))
    no_parts: list[str] = []
    print("-".join(no_parts))


def math_and_contains() -> None:
    value: float = 4.5
    print(math.sqrt(16.0), math.pow(2.0, 3.0), math.atan2(0.0, 1.0))
    print(math.pi, math.e, math.tau)
    print(math.floor(value), math.ceil(value), math.trunc(value))
    print(math.floor(-1.5), math.ceil(-1.5), math.trunc(-1.5))
    infinity: float = 1e308 * 10.0
    not_a_number: float = infinity - infinity
    print(math.isfinite(value), math.isnan(value), math.isfinite(infinity), math.isnan(not_a_number))
    sample: float = random.random()
    print(0.0 <= sample and sample < 1.0)
    roll: int = random.randint(1, 6)
    print(1 <= roll and roll <= 6)
    choices: list[str] = ["only"]
    print(random.choice(choices))
    values: tuple[int, int] = (1, 2)
    print(2 in values, 3 in values)
    unique: set[int] = {1, 2}
    print(1 in unique, 3 in unique)
    mapping: dict[str, int] = {"a": 1}
    print("a" in mapping, "b" in mapping)


def choose(flag: bool, left: int, right: int) -> int:
    return left if flag else right


list_methods()
missing_list_items()
container_constructors()
sets_of_tuples()
dict_methods()
string_methods()
math_and_contains()
print(choose(True, 1, 2), choose(False, 1, 2))
