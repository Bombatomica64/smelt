# A name the program binds itself (`def`, parameter) is that binding, even
# when it is spelled like a builtin; an unbound builtin name is the builtin.
def abs(x: int) -> int:
    return 100 + x


def len(x: list[int]) -> int:
    return -1


def max(a: int, b: int) -> int:
    return a + b


def apply(sum: int, values: list[int]) -> int:
    total = 0
    for value in values:
        total += value
    return total * sum


print(abs(-3), len([1, 2, 3]), max(1, 2), apply(2, [1, 2]), min(4, 5), sum([1, 2]))
