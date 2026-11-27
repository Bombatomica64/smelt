class P:
    def __init__(self, name: str, a: int, b: int) -> None:
        self.name = name
        self.a = a
        self.b = b


def numbers() -> list[int]:
    print("numbers evaluated")
    return [1, 2, 3, 4, 5]


def main() -> None:
    first, *rest = numbers()
    print(first, rest)
    *init, last = [10, 20, 30]
    print(init, last)
    a, *mid, z = ["a", "b", "c", "d"]
    print(a, mid, z)
    only, *empty = [7]
    print(only, empty)
    try:
        x, y, *more = numbers()[:1]
        print(x, y, more)
    except ValueError as error:
        print("ValueError:", error)
    for i, word in enumerate(["x", "y", "z"], 1):
        print(i, word)
    print(list(enumerate(["p", "q"], start=10)), list(enumerate([5, 6], -2)))
    people = [P("ann", 2, 5), P("bob", 1, 7), P("cid", 2, 9), P("dee", 1, 7)]
    print([p.name for p in sorted(people, key=lambda p: (p.a, -p.b))])
    counts = {"x": 3, "y": 1, "z": 3}
    print(sorted(counts.items(), key=lambda kv: (-kv[1], kv[0])))
    print(sorted(people, key=lambda p: (p.b, p.name), reverse=True)[0].name)


main()
