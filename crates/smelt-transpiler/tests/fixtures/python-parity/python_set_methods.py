# Set mutation and algebra methods.


def show(values: set[int]) -> str:
    return ",".join(str(item) for item in sorted(values))


def mutation() -> None:
    values: set[int] = {1, 2}
    values.add(3)
    values.discard(2)
    values.discard(42)
    print(show(values))
    values.remove(1)
    print(show(values))
    copied: set[int] = values.copy()
    copied.add(9)
    print(show(values), show(copied))
    values.clear()
    print(len(values), len(copied))


def algebra() -> None:
    left: set[int] = {1, 2}
    right: set[int] = {2, 3}
    print(show(left.union(right)))
    print(show(left.intersection(right)))
    print(show(left.difference(right)))
    print(show(left.symmetric_difference(right)))
    print(left.isdisjoint(right), left.isdisjoint({7}))
    print(left.issubset({1, 2, 3}), left.issubset(right))
    print(left.issuperset({1}), left.issuperset(right))
    print(show(left), show(right))


mutation()
algebra()
