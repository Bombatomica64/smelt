class Bag:
    def __init__(self, n: int) -> None:
        self.n = n

    def __len__(self) -> int:
        return self.n


class Flag:
    def __init__(self, on: bool) -> None:
        self.on = on

    def __bool__(self) -> bool:
        return self.on


class Plain:
    pass


def check(label: str, value: bool) -> None:
    print(label, value)


def main() -> None:
    xs: list[int] = []
    s = ""
    n = 0
    f = 0.0
    d: dict[str, int] = {}
    st: set[int] = set()
    o: int | None = None
    ol: list[int] | None = []
    check("list", bool(xs))
    check("str", bool(s))
    check("int", bool(n))
    check("float", bool(f))
    check("dict", bool(d))
    check("set", bool(st))
    check("opt", bool(o))
    check("optlist", bool(ol))
    if xs:
        print("xs true")
    else:
        print("xs false")
    if not d:
        print("d empty")
    xs.append(3)
    while xs:
        print("pop", xs.pop())
    if ol:
        print("ol true")
    ol = [1]
    if ol:
        print("ol now true")
    check("bag0", bool(Bag(0)))
    check("bag2", bool(Bag(2)))
    check("flag", bool(Flag(False)))
    check("plain", bool(Plain()))
    print("yes" if Bag(1) else "no", "yes" if [] else "no")
    ys = [0, 1, 2, 0, 3]
    print([y for y in ys if y])
    print([w for w in ["", "a", "b"] if w])
    print([v for v in [[], [1], [2, 3]] if v])
    print([u for u in [None, 0, 5] if u])
    print([b.n for b in [Bag(0), Bag(4)] if b])
    print(list(filter(None, ys)))
    print(list(filter(None, ["", "x"])))
    print(list(filter(lambda y: y % 2, ys)))
    name = "" or "anon"
    print(name)
    assert ys, "non-empty"
    print("end")


main()
