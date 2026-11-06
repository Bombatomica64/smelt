"""A raise inside a map, filter or sort callback reaches the enclosing try."""


def boom(value: int) -> int:
    return 10 // (value - 2)


def via_map(values: list[int]) -> int:
    try:
        return len(list(map(lambda v: boom(v), values)))
    except ZeroDivisionError:
        return -1


def via_filter(values: list[int]) -> int:
    try:
        return len(list(filter(lambda v: boom(v) > 0, values)))
    except ZeroDivisionError:
        return -2


def via_sorted(values: list[int]) -> int:
    try:
        return len(sorted(values, key=lambda v: boom(v)))
    except ZeroDivisionError:
        return -3


print(via_map([1, 3]), via_map([1, 2]))
print(via_filter([1, 3]), via_filter([2]))
print(via_sorted([3, 1]), via_sorted([2, 1]))
