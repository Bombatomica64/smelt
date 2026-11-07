# A dict is a reference value: a second name for it, a dict stored in a list,
# and a dict passed to a function all reach the same container, so a write
# through any of them is seen through every other.


def add(target: dict[str, int], key: str, value: int) -> None:
    target[key] = value


def drop(target: dict[str, int], key: str) -> None:
    del target[key]


counts: dict[str, int] = {"a": 1}
alias = counts
alias["b"] = 2
add(counts, "c", 3)
print(counts, alias, len(counts), len(alias))
drop(alias, "a")
print(counts)
print(counts == alias)

copy = dict(counts)
copy["z"] = 26
print(counts, copy)

rows: list[dict[str, int]] = [counts, counts]
rows[0]["shared"] = 1
print(rows[1], counts)

numbered: dict[str, str] = {"2": "two", "b": "bee", "1": "one"}
numbered["0"] = "zero"
print(numbered, list(numbered))


def mark(target: dict[int, str], key: int) -> None:
    target[key] = "set"


squares: dict[int, int] = {}
for n in [3, 1, 2]:
    squares[n] = n * n
same = squares
same[0] = 0
print(squares, list(squares.items()))
names: dict[int, str] = {5: "five"}
mark(names, 7)
del names[5]
names[5] = "again"
print(names, 7 in names, names.get(9, "none"))
flags: dict[bool, int] = {True: 1, False: 0}
print(flags)
